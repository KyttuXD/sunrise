//! Optional, texture-matched overlays drawn full bright over their base texture in the same
//! terrain fragment: native mods' registered overlays and OptiFine's emissive block textures
//! (`block/stone_e.png` beside `block/stone.png`). No extra draw calls or world light changes.
use crate::resource_source::ResourceSource;
use crate::sprites::Sprites;
use std::collections::BTreeMap;

use mod_api::{Answered, Identifier};

/// The owning mod and the base and overlay identifiers it submitted for this resource load.
pub type Registrations = [Answered<(Identifier, Identifier)>];

/// Resolve the generation's overlays into the same layers terrain geometry uses. A failed
/// overlay does not replace a usable one. Explicit registrations take precedence over pack pairs.
pub fn bake(root: &ResourceSource, sprites: &mut Sprites, native: &Registrations) -> Vec<u32> {
    let mut by_base = BTreeMap::new();
    for entry in native {
        let (base, overlay) = &entry.answer;
        let Some(&layer) = sprites.layers.get(&base.to_string()) else {
            log::warn!(
                "mod {}: emissive overlay {overlay}: terrain sprite {base} was not baked",
                entry.mod_id
            );
            continue;
        };
        match sprites.try_insert_texture_file(&overlay.to_string()) {
            Ok(overlay_layer) => {
                by_base.insert(layer, overlay_layer);
            }
            Err(error) => log::warn!(
                "mod {}: emissive overlay {overlay} for {base} skipped: {error}",
                entry.mod_id
            ),
        }
    }
    for (base, overlay) in load_optifine(root, sprites) {
        by_base
            .entry(sprites.layers[&base])
            .or_insert(sprites.files[&overlay] | CUTOUT);
    }
    // The GPU indexes this table by base layer. Leave the suffix implicit when none of those
    // layers have overlays, including the common case of no registrations at all.
    let mut layers = by_base
        .last_key_value()
        .map_or_else(Vec::new, |(&last, _)| vec![u32::MAX; last as usize + 1]);
    for (base, overlay) in by_base {
        layers[base as usize] = overlay;
    }
    layers
}

/// A connected-texture tile's sprite name without `.png`: tiles outside `textures/` are named by
/// their resource path (`Sprites::insert_file`, which only they use), block sprites by an id.
fn tile_file(name: &str) -> Option<&str> {
    name.strip_prefix("assets/")?;
    name.strip_suffix(".png")
}

/// OptiFine's emissive overlays of the block sprites already loaded (what the terrain draws;
/// items draw from the HUD atlas): for each
/// `<ns>:block/<name>`, or connected-texture tile `assets/<ns>/.../<name>.png`, whose
/// `<name><suffix>.png` sits beside it, the overlay is loaded too and paired with it, blocks
/// first, then tiles, each by name. A pack without such files adds nothing.
pub fn load_optifine(root: &ResourceSource, sprites: &mut Sprites) -> Vec<(String, String)> {
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
        let loaded = sprites.try_insert_texture_file(&overlay);
        match loaded {
            Ok(_) => pairs.push((base, overlay)),
            Err(e) => log::warn!("Sunrise: emissive overlay {overlay} skipped: {e}"),
        }
    }
    pairs
}

/// The overlay layer's flag (its high bit) for an OptiFine overlay, which draws cutout: opaque
/// where its alpha is at least 0.1 (OptiFine renders SOLID textures' overlays as CUTOUT).
pub const CUTOUT: u32 = 1 << 31;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// A 16x16 image of one colour, `shade` its red (images of different shades never share a
    /// sprite layer).
    fn png(path: &Path, shade: u8) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
        let mut encoder = png::Encoder::new(file, 16, 16);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        let pixels: Vec<u8> = (0..16 * 16).flat_map(|_| [shade, 9, 9, 255]).collect();
        writer.write_image_data(&pixels).unwrap();
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
            png(&block.join(format!("{name}.png")), i as u8 * 20);
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
        let pairs = load_optifine(&ResourceSource::directory(root.clone()), &mut sprites);
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
        let pairs = load_optifine(&ResourceSource::directory(root.clone()), &mut sprites);
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
    fn emissive_keys_are_baked_without_the_old_request_cap() {
        use crate::resource_source::Resource;
        let mut data = Vec::new();
        let mut encoder = png::Encoder::new(&mut data, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[1, 2, 3, 255])
            .unwrap();
        let source = ResourceSource::indexed(BTreeMap::from([(
            "assets/test/textures/light/mask.png".into(),
            Resource::bytes(data),
        )]));
        let keys: Vec<_> = (0..140)
            .map(|i| Identifier::parse(&format!("test:block/base_{i}")).unwrap())
            .collect();
        let mut sprites = Sprites::new(source.clone());
        // The model bake has already established different base layers in this generation.
        sprites.layers.extend(
            keys.iter()
                .enumerate()
                .map(|(i, base)| (base.to_string(), i as u32)),
        );
        let overlay = Identifier::parse("test:light/mask").unwrap();
        let mut registrations: Vec<_> = keys
            .iter()
            .map(|base| Answered {
                answer: (base.clone(), overlay.clone()),
                mod_id: "test",
            })
            .collect();
        registrations.push(Answered {
            answer: (
                keys[139].clone(),
                Identifier::parse("test:missing").unwrap(),
            ),
            mod_id: "test",
        });
        assert_eq!(bake(&source, &mut sprites, &registrations), vec![0; 140]);
        assert!(
            bake(&source, &mut sprites, &[]).is_empty(),
            "nothing carries into a reload without the declaration"
        );
    }
}
