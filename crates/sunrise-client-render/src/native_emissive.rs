//! Optional, texture-matched overlays drawn full bright over their base texture in the same
//! terrain fragment: mod-registered emissive sprites (`Hooks::emissive`) and OptiFine's emissive
//! block textures (`block/stone_e.png` beside `block/stone.png`). Resolved once during resource
//! bake/reload and bound to the published [`crate::assets::RenderAssets`] generation. No extra
//! draw calls or world light changes.
use crate::resource_source::ResourceSource;
use crate::sprites::Sprites;
use mod_api::{Answered, EmissiveSpriteRegistration};
use std::collections::{BTreeMap, BTreeSet};

/// Overlay pairs the terrain shader reads (`Globals.emissive_layers`, `terrain.wgsl`).
/// Shared between mod-registered overlays and OptiFine block overlays without a private split.
pub const MAX: usize = 128;

/// The overlay layer's flag (its high bit) for an OptiFine overlay, which draws cutout: opaque
/// where its alpha is at least 0.1 (OptiFine renders SOLID textures' overlays as CUTOUT).
pub const CUTOUT: u32 = 1 << 31;

/// A connected-texture tile's sprite name without `.png`: tiles outside `textures/` are named by
/// their resource path (`Sprites::insert_file`, which only they use), block sprites by an id.
fn tile_file(name: &str) -> Option<&str> {
    name.strip_prefix("assets/")?;
    name.strip_suffix(".png")
}

/// Resolve mod-registered emissive pairs by sprite id before allocating any overlay layers.
/// A later valid overlay for the same id replaces the old one (`TextureManager.register`), but
/// an unreadable replacement leaves the previous candidate intact. The first distinct bases
/// with usable overlays take the GPU uniform slots, in registration order.
pub(crate) fn resolve_registered(
    registrations: &[Answered<EmissiveSpriteRegistration>],
    sprites: &mut Sprites,
) -> BTreeMap<u32, u32> {
    struct Overlay {
        base_layer: u32,
        mod_id: &'static str,
        id: String,
        prepared: Option<crate::sprites::PreparedSprite>,
    }
    let mut by_base: BTreeMap<&mod_api::Identifier, Overlay> = BTreeMap::new();
    for entry in registrations {
        let base = &entry.answer.base;
        let overlay = &entry.answer.overlay;
        let Some(&base_layer) = sprites.layers.get(&base.to_string()) else {
            log::warn!(
                "Sunrise: mod '{}' registered emissive overlay '{overlay}' for unknown base sprite '{base}'; skipping",
                entry.mod_id,
            );
            continue;
        };
        if by_base.len() == MAX && !by_base.contains_key(base) {
            log::warn!(
                "Sunrise: mod '{}' emissive overlay '{overlay}' for '{base}' skipped: GPU uniform table (Globals.emissive_layers) has {MAX} slots",
                entry.mod_id,
            );
            continue;
        }
        let id = overlay.to_string();
        let prepared = match sprites.prepare_texture_file(&id) {
            Ok(prepared) => prepared,
            Err(error) => {
                log::warn!(
                    "Sunrise: mod '{}' emissive overlay '{overlay}' for '{base}' skipped: {error}",
                    entry.mod_id,
                );
                continue;
            }
        };
        if let Some(previous) = by_base.insert(
            base,
            Overlay {
                base_layer,
                mod_id: entry.mod_id,
                id,
                prepared,
            },
        ) {
            log::warn!(
                "Sunrise: mod '{}' emissive overlay '{overlay}' for '{base}' replaces '{}' registered by mod '{}'",
                entry.mod_id,
                previous.id,
                previous.mod_id,
            );
        }
    }
    by_base
        .into_iter()
        .filter_map(|(base, overlay)| {
            match sprites.commit_texture_file(&overlay.id, overlay.prepared) {
                Ok(layer) => Some((overlay.base_layer, layer)),
                Err(error) => {
                    log::warn!(
                        "Sunrise: mod '{}' emissive overlay '{}' for '{base}' skipped: {error}",
                        overlay.mod_id,
                        overlay.id,
                    );
                    None
                }
            }
        })
        .collect()
}

/// OptiFine's emissive overlays of the block sprites already loaded (what the terrain draws;
/// items draw from the HUD atlas): for each `<ns>:block/<name>`, or connected-texture tile
/// `assets/<ns>/.../<name>.png`, whose `<name><suffix>.png` sits beside it, the overlay is loaded
/// too and paired with it, blocks first, then tiles, each by name, skipping base layers already
/// claimed in `claimed_bases` and sharing the GPU uniform table capacity ([`MAX`]). A pack
/// without such files adds nothing.
pub fn load_optifine(
    root: &ResourceSource,
    sprites: &mut Sprites,
    claimed_bases: &BTreeSet<u32>,
) -> Vec<(String, String)> {
    let remaining = MAX.saturating_sub(claimed_bases.len());
    if remaining == 0 {
        return Vec::new();
    }
    let suffix = sunrise_optifine::emissive::suffix(root);
    // Blocks with identical pixels share a layer (and the shader pairs by layer): a shared
    // base would glow on every block that shares it, so it gets no overlay.
    let mut sharing: BTreeMap<u32, usize> = BTreeMap::new();
    for layer in sprites.layers.values() {
        *sharing.entry(*layer).or_default() += 1;
    }
    let mut bases: Vec<String> = sprites
        .layers
        .iter()
        .filter(|(name, layer)| {
            sharing[layer] == 1
                && !claimed_bases.contains(layer)
                && (name
                    .split_once(':')
                    .is_some_and(|(_, p)| p.starts_with("block/") && !p.ends_with(&suffix))
                    || tile_file(name).is_some_and(|stem| !stem.ends_with(&suffix)))
        })
        .map(|(name, _)| name.clone())
        .collect();
    // Blocks before tiles: a pack's many tiles must not crowd vanilla blocks' overlays out.
    bases.sort_by_key(|name| (tile_file(name).is_some(), name.clone()));
    let mut pairs = Vec::new();
    for base in bases {
        // A tile is named by its file, a block sprite by its id.
        let (overlay, file) = match tile_file(&base) {
            Some(stem) => {
                let overlay = format!("{stem}{suffix}.png");
                (overlay.clone(), overlay)
            }
            None => {
                let overlay = format!("{base}{suffix}");
                let file = crate::atlas_sources::texture_resource(&overlay, "textures");
                (overlay, file)
            }
        };
        if !root.contains(&file) {
            continue;
        }
        if pairs.len() == remaining {
            log::warn!(
                "Sunrise: emissive block overlays reached GPU uniform capacity {MAX} (Globals.emissive_layers): {overlay} and later skipped"
            );
            break;
        }
        match sprites.try_insert_texture_file(&overlay) {
            Ok(_) => pairs.push((base, overlay)),
            Err(e) => log::warn!("Sunrise: emissive overlay {overlay} skipped: {e}"),
        }
    }
    pairs
}

/// Resolve both mod-registered and OptiFine emissive overlays during bake/reload into the sorted
/// GPU uniform table (`Globals.emissive_layers`). Mod overlays take precedence over OptiFine
/// overlays for the same base layer; OptiFine overlays use the remaining GPU uniform slots and
/// carry [`CUTOUT`].
pub fn load_layers(
    root: &ResourceSource,
    registrations: &[Answered<EmissiveSpriteRegistration>],
    sprites: &mut Sprites,
) -> Vec<[u32; 4]> {
    let mod_pairs = resolve_registered(registrations, sprites);
    let claimed: BTreeSet<u32> = mod_pairs.keys().copied().collect();
    let optifine = load_optifine(root, sprites, &claimed);
    assemble_table(&sprites.layers, &sprites.files, mod_pairs, &optifine)
}

/// Assemble the sorted GPU overlay table from resolved mod pairs and loaded OptiFine pairs.
fn assemble_table(
    layers: &BTreeMap<String, u32>,
    files: &BTreeMap<String, u32>,
    mod_pairs: BTreeMap<u32, u32>,
    optifine: &[(String, String)],
) -> Vec<[u32; 4]> {
    let mut by_base: BTreeMap<u32, [u32; 4]> = mod_pairs
        .into_iter()
        .map(|(b, o)| (b, [b, o, 0, 0]))
        .collect();
    for (base, overlay) in optifine {
        if let (Some(&b), Some(&o)) = (layers.get(base), files.get(overlay)) {
            by_base.entry(b).or_insert([b, o | CUTOUT, 0, 0]);
        }
    }
    by_base.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mod_api::Identifier;
    use std::path::Path;

    /// A 16x16 image of one colour, `shade` its red/green (images of different shades never share
    /// a sprite layer).
    fn png(path: &Path, shade: u16) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
        let mut encoder = png::Encoder::new(file, 16, 16);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        let r = (shade & 0xff) as u8;
        let g = ((shade >> 8) & 0xff) as u8;
        let pixels: Vec<u8> = (0..16 * 16).flat_map(|_| [r, g, 9, 255]).collect();
        writer.write_image_data(&pixels).unwrap();
    }

    fn registration(
        mod_id: &'static str,
        base: &str,
        overlay: &str,
    ) -> Answered<EmissiveSpriteRegistration> {
        Answered {
            mod_id,
            answer: EmissiveSpriteRegistration::new(
                Identifier::parse(base).unwrap(),
                Identifier::parse(overlay).unwrap(),
            ),
        }
    }

    #[test]
    fn optifine_overlays_pair_with_their_loaded_blocks() {
        let root =
            std::env::temp_dir().join(format!("sunrise-block-emissive-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let block = root.join("assets/minecraft/textures/block");
        let names = [
            "stone",
            "stone_e",
            "dirt",
            "glowstone_e",
            "deepslate",
            "deepslate_glow",
            "andesite",
            "granite",
        ];
        for (i, name) in names.iter().enumerate() {
            png(&block.join(format!("{name}.png")), (i as u16) * 20);
        }
        // Same pixels as andesite: the two share a layer, so neither gets an overlay.
        png(&block.join("diorite.png"), 6 * 20);
        png(&block.join("diorite_e.png"), 200);
        // A broken overlay: skipped, not drawn as the missing texture.
        std::fs::write(block.join("granite_e.png"), b"not a png").unwrap();
        png(&root.join("assets/minecraft/textures/item/stick.png"), 210);
        png(
            &root.join("assets/minecraft/textures/item/stick_e.png"),
            220,
        );
        let mut sprites = Sprites::new(crate::resource_source::ResourceSource::directory(
            root.clone(),
        ));
        for name in [
            "stone",
            "dirt",
            "deepslate",
            "andesite",
            "diorite",
            "granite",
        ] {
            sprites.insert(&format!("minecraft:block/{name}")).unwrap();
        }
        // Items draw from the HUD atlas, not the terrain: no pair.
        sprites.insert("minecraft:item/stick").unwrap();
        // glowstone isn't loaded: its overlay pairs with nothing.
        let pairs = load_optifine(
            &ResourceSource::directory(root.clone()),
            &mut sprites,
            &BTreeSet::new(),
        );
        assert_eq!(
            pairs,
            [(
                "minecraft:block/stone".into(),
                "minecraft:block/stone_e".into()
            )]
        );
        assert!(sprites.files.contains_key("minecraft:block/stone_e"));
        assert!(!sprites.files.contains_key("minecraft:block/granite_e"));
        // The pack's own suffix.
        let props = root.join("assets/minecraft/optifine/emissive.properties");
        std::fs::create_dir_all(props.parent().unwrap()).unwrap();
        std::fs::write(&props, "suffix.emissive=_glow").unwrap();
        // Connected-texture tiles pair with the overlay beside them.
        let tiles = root.join("assets/minecraft/optifine/ctm/lamp");
        png(&tiles.join("0.png"), 230);
        png(&tiles.join("0_glow.png"), 240);
        png(&tiles.join("1.png"), 250);
        // A tile outside `ctm/` (`tiles=~/glass/2`) is a tile all the same.
        png(&root.join("assets/minecraft/optifine/glass/2.png"), 245);
        png(
            &root.join("assets/minecraft/optifine/glass/2_glow.png"),
            246,
        );
        for tile in ["ctm/lamp/0", "ctm/lamp/1", "glass/2"] {
            sprites
                .insert_file(&format!("assets/minecraft/optifine/{tile}.png"))
                .unwrap();
        }
        let pairs = load_optifine(
            &ResourceSource::directory(root.clone()),
            &mut sprites,
            &BTreeSet::new(),
        );
        assert_eq!(
            pairs,
            // Blocks first, then tiles.
            [
                (
                    "minecraft:block/deepslate".into(),
                    "minecraft:block/deepslate_glow".into()
                ),
                (
                    "assets/minecraft/optifine/ctm/lamp/0.png".into(),
                    "assets/minecraft/optifine/ctm/lamp/0_glow.png".into()
                ),
                (
                    "assets/minecraft/optifine/glass/2.png".into(),
                    "assets/minecraft/optifine/glass/2_glow.png".into()
                ),
            ]
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn registered_overlays_support_more_than_32_any_path_and_isolate_failures() {
        let root =
            std::env::temp_dir().join(format!("sunrise-mod-emissive-many-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut sprites = Sprites::new(ResourceSource::directory(root.clone()));
        let mut registrations = Vec::new();
        // Register 40 overlays (past the old NATIVE = 32 cap) under non-emissive/ paths,
        // including minecraft: and mod namespaces, plus broken and missing entries.
        for i in 0..40u16 {
            let base_path = root.join(format!("assets/demo/textures/block/lamp_{i}.png"));
            let overlay_path = root.join(format!("assets/demo/textures/glow/lamp_{i}.png"));
            png(&base_path, 10 + i * 2);
            png(&overlay_path, 11 + i * 2);
            let base = format!("demo:block/lamp_{i}");
            let overlay = format!("demo:glow/lamp_{i}");
            sprites.insert(&base).unwrap();
            registrations.push(registration("demo", &base, &overlay));
        }
        // Broken overlay texture and missing base must not break the valid 40 entries.
        png(
            &root.join("assets/demo/textures/block/broken_base.png"),
            500,
        );
        std::fs::write(
            root.join("assets/demo/textures/glow/corrupt.png"),
            b"not a png",
        )
        .unwrap();
        sprites.insert("demo:block/broken_base").unwrap();
        registrations.push(registration(
            "broken",
            "demo:block/broken_base",
            "demo:glow/corrupt",
        ));
        registrations.push(registration(
            "missing",
            "demo:block/no_such_block",
            "demo:glow/lamp_0",
        ));

        let table = load_layers(
            &ResourceSource::directory(root.clone()),
            &registrations,
            &mut sprites,
        );
        assert_eq!(table.len(), 40);
        assert!(table.windows(2).all(|w| w[0][0] < w[1][0]));
        assert!(table.iter().all(|row| row[1] & CUTOUT == 0));
        assert!(!sprites.files.contains_key("demo:glow/corrupt"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn duplicate_mod_base_replaces_earlier_and_beats_optifine() {
        let root =
            std::env::temp_dir().join(format!("sunrise-mod-emissive-dup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        png(&root.join("assets/minecraft/textures/block/a.png"), 1);
        png(&root.join("assets/minecraft/textures/block/a_e.png"), 2);
        png(&root.join("assets/minecraft/textures/block/b.png"), 3);
        png(&root.join("assets/minecraft/textures/block/b_e.png"), 4);
        png(&root.join("assets/mod_a/textures/overlay/first.png"), 5);
        png(&root.join("assets/mod_b/textures/overlay/second.png"), 6);

        let mut sprites = Sprites::new(ResourceSource::directory(root.clone()));
        let layer_a = sprites.insert("minecraft:block/a").unwrap();
        let layer_b = sprites.insert("minecraft:block/b").unwrap();

        let regs = [
            registration("mod_a", "minecraft:block/a", "mod_a:overlay/first"),
            registration("mod_b", "minecraft:block/a", "mod_b:overlay/second"),
        ];
        let table = load_layers(
            &ResourceSource::directory(root.clone()),
            &regs,
            &mut sprites,
        );
        let second = sprites.files["mod_b:overlay/second"];
        let optifine_b = sprites.files["minecraft:block/b_e"];
        assert_eq!(
            table,
            vec![
                [layer_a, second, 0, 0],
                [layer_b, optifine_b | CUTOUT, 0, 0]
            ]
        );
        assert!(!sprites.files.contains_key("mod_a:overlay/first"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn identical_pixels_do_not_share_emissive_identity() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("assets/demo/textures");
        png(&dir.join("block/a.png"), 1);
        png(&dir.join("block/b.png"), 1);
        png(&dir.join("glow/a.png"), 2);
        png(&dir.join("glow/b.png"), 3);
        let source = ResourceSource::directory(root.path().into());
        let regs = [
            registration("demo", "demo:block/a", "demo:glow/a"),
            registration("demo", "demo:block/b", "demo:glow/b"),
        ];
        for regs in [&regs[..1], &regs[..]] {
            let mut sprites = Sprites::new(source.clone());
            sprites.keep_apart(regs.iter().map(|entry| entry.answer.base.to_string()));
            let a = sprites.insert("demo:block/a").unwrap();
            let b = sprites.insert("demo:block/b").unwrap();
            assert_ne!(a, b);
            let table = load_layers(&source, regs, &mut sprites);
            assert_eq!(table.len(), regs.len());
            assert_eq!(table[0], [a, sprites.files["demo:glow/a"], 0, 0]);
            if regs.len() == 2 {
                assert_eq!(table[1], [b, sprites.files["demo:glow/b"], 0, 0]);
                assert_ne!(table[0][1], table[1][1]);
            }
        }
    }

    #[test]
    fn only_final_gpu_rows_get_layers_and_bad_replacement_leaves_previous() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("assets/demo/textures");
        let source = ResourceSource::directory(root.path().into());
        let mut sprites = Sprites::new(source.clone());
        let mut regs = Vec::new();
        for i in 0..=MAX as u16 {
            png(&dir.join(format!("block/b{i}.png")), i);
            png(&dir.join(format!("glow/g{i}.png")), 200 + i);
            let base = format!("demo:block/b{i}");
            sprites.insert(&base).unwrap();
            regs.push(registration("demo", &base, &format!("demo:glow/g{i}")));
        }
        png(&dir.join("glow/winner.png"), 400);
        regs.push(registration("winner", "demo:block/b0", "demo:glow/winner"));
        regs.push(registration("broken", "demo:block/b0", "demo:glow/missing"));
        let bases = sprites.count();
        let table = load_layers(&source, &regs, &mut sprites);
        assert_eq!(table.len(), MAX);
        assert_eq!(sprites.count(), bases + MAX);
        assert_eq!(table[0][1], sprites.files["demo:glow/winner"]);
        assert!(!sprites.files.contains_key("demo:glow/g0"));
        assert!(!sprites.files.contains_key(&format!("demo:glow/g{MAX}")));
    }

    #[test]
    fn optifine_can_use_full_gpu_capacity_when_no_mods_register() {
        let root = std::env::temp_dir().join(format!(
            "sunrise-optifine-emissive-full-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let block = root.join("assets/minecraft/textures/block");
        let mut sprites = Sprites::new(ResourceSource::directory(root.clone()));
        // 105 OptiFine overlays exceed the old MAX - NATIVE = 96 split and all fit in MAX = 128.
        for i in 0..105u16 {
            png(&block.join(format!("b_{i:03}.png")), 1000 + i * 2);
            png(&block.join(format!("b_{i:03}_e.png")), 1001 + i * 2);
            sprites
                .insert(&format!("minecraft:block/b_{i:03}"))
                .unwrap();
        }
        let table = load_layers(&ResourceSource::directory(root.clone()), &[], &mut sprites);
        assert_eq!(table.len(), 105);
        assert!(table.iter().all(|row| row[1] & CUTOUT != 0));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
