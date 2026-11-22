//! Stage the sprites of mod-registered texture atlases (`register_texture_atlases`) and collect
//! mod-registered emissive sprite pairs (`emissive`) for a client resource generation.
use super::Game;
use mod_api::__private::game::*;
use mod_api::{Answered, EmissiveSpriteRegistration, Identifier, TextureAtlasRegistration};
use std::collections::BTreeMap;
#[cfg(test)]
use sunrise_assets::resource_source::ResourceSource;
use sunrise_assets::resource_source::{Resource, ResourceRoot, RuntimeTextureAtlas};
use sunrise_client::atlas_sources::{self, SpriteOrigin};

pub(in crate::game) fn collect_texture_atlases() -> Vec<Answered<TextureAtlasRegistration>> {
    let mut atlases = Vec::new();
    if mod_api::__private::listened(mod_api::HookId::RegisterTextureAtlases) {
        let _phase =
            mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::Registration);
        Game::native_register_texture_atlases(&mod_api::RegistrationContext::new(), &mut atlases);
    }
    atlases
}

pub(in crate::game) fn collect_emissive_sprites() -> Vec<Answered<EmissiveSpriteRegistration>> {
    let mut layers = Vec::new();
    if mod_api::__private::listened(mod_api::HookId::Emissive) {
        let _phase =
            mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::Registration);
        Game::native_emissive(&mod_api::RegistrationContext::new(), &mut layers);
    }
    layers
}

/// The root with every registered atlas's sprites added as GUI textures: a file sprite as the
/// texture itself (with its `.mcmeta`), a generated one (`unstitch`, `paletted_permutations`)
/// encoded once. Passive data stays as it was.
pub(in crate::game) fn stage_texture_atlases(
    root: ResourceRoot,
    atlases: &[Answered<TextureAtlasRegistration>],
) -> ResourceRoot {
    stage_with(
        root,
        atlases,
        &sunrise_client_render::mod_sprite_sources::Registered,
    )
}

/// [`stage_texture_atlases`] asking `mods` for the sources' answers.
fn stage_with(
    root: ResourceRoot,
    atlases: &[Answered<TextureAtlasRegistration>],
    mods: &dyn atlas_sources::ModSources,
) -> ResourceRoot {
    if atlases.is_empty() {
        return root;
    }
    let source = root.source();
    // The game's atlases are listed (and the mods' sources asked for them) over the stack the packs
    // gave, before the staged textures extend it: no reader or directory source sees one.
    atlas_sources::shared(source, mods);
    // Admit atlases following NeoForge `RegisterTextureAtlasesEvent.register` (`putIfAbsent`
    // against `AtlasManager.KNOWN_ATLASES` and earlier registrations): first registration stands,
    // with per-atlas diagnostics naming the mod and earlier owner.
    let mut seen: BTreeMap<&Identifier, &'static str> = BTreeMap::new();
    let mut admitted = Vec::new();
    for entry in atlases {
        let atlas = &entry.answer.atlas;
        if atlas_sources::is_builtin_atlas(atlas) {
            log::error!(
                "Sunrise: mod '{}' cannot register texture atlas '{atlas}': collides with built-in engine atlas (AtlasManager.KNOWN_ATLASES)",
                entry.mod_id,
            );
            continue;
        }
        match seen.entry(atlas) {
            std::collections::btree_map::Entry::Occupied(previous) => log::error!(
                "Sunrise: mod '{}' duplicate texture atlas definition '{atlas}' ignored; already registered by mod '{}'",
                entry.mod_id,
                previous.get(),
            ),
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(entry.mod_id);
                admitted.push(entry);
            }
        }
    }
    let mut listed = atlas_sources::registered(
        source,
        mods,
        &admitted
            .iter()
            .map(|entry| &entry.answer.atlas)
            .collect::<Vec<_>>(),
    );
    let mut groups = Vec::new();
    for entry in admitted {
        let registration = &entry.answer;
        let atlas = &registration.atlas;
        let Some(listed) = listed.remove(atlas) else {
            log::warn!(
                "Sunrise: mod '{}' texture atlas '{atlas}' skipped: no usable definition assets/{}/atlases/{}.json",
                entry.mod_id,
                atlas.namespace(),
                atlas.path(),
            );
            continue;
        };
        let mut textures = BTreeMap::new();
        for (id, origin) in &listed.sprites {
            let Ok(sprite) = Identifier::parse(id) else {
                log::warn!(
                    "Sunrise: mod '{}' texture atlas '{atlas}': invalid sprite identifier '{id}'; skipping",
                    entry.mod_id,
                );
                continue;
            };
            let pair = if let SpriteOrigin::File(texture) = origin {
                let file = atlas_sources::texture_resource(texture, "textures");
                let sidecar = format!("{file}.mcmeta");
                source.contains(&file).then(|| {
                    let metadata = source
                        .contains(&sidecar)
                        .then(|| Resource::file(source.clone(), sidecar));
                    (Resource::file(source.clone(), file), metadata)
                })
            } else {
                let load = |texture: &str| {
                    let file = atlas_sources::texture_resource(texture, "textures");
                    sunrise_client::hud::decode_bytes(&source.read(&file).ok()?).ok()
                };
                atlas_sources::generate(origin, &load)
                    .and_then(encode)
                    .map(|png| (Resource::bytes(png), None))
            };
            let Some(pair) = pair else {
                log::warn!(
                    "Sunrise: mod '{}' texture atlas '{atlas}': sprite '{id}' could not be loaded",
                    entry.mod_id
                );
                continue;
            };
            let name = registration.sprite_name(&sprite);
            let path = atlas_sources::texture_resource(&name.to_string(), "textures");
            if source.contains(&path) {
                log::warn!(
                    "Sunrise: mod '{}' texture atlas '{atlas}' sprite '{id}' replaces existing pack texture '{path}'",
                    entry.mod_id
                );
            }
            textures.insert(path, pair);
        }
        groups.push(RuntimeTextureAtlas {
            id: atlas.to_string(),
            mod_id: entry.mod_id,
            textures,
        });
    }
    root.with_runtime_texture_atlases(groups)
}

fn encode((width, height, pixels): atlas_sources::Image) -> Option<Vec<u8>> {
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .ok()?
        .write_image_data(&pixels)
        .ok()?;
    Some(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_png(width: u32, height: u32, channel: u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&vec![channel; (width * height * 4) as usize])
            .unwrap();
        bytes
    }

    fn registration(atlas: &str) -> TextureAtlasRegistration {
        TextureAtlasRegistration {
            atlas: Identifier::parse(atlas).unwrap(),
        }
    }

    fn answered(mod_id: &'static str, atlas: &str) -> Answered<TextureAtlasRegistration> {
        Answered {
            mod_id,
            answer: registration(atlas),
        }
    }

    fn root(files: &[(&str, Vec<u8>)]) -> ResourceRoot {
        let files = files
            .iter()
            .map(|(name, bytes)| ((*name).to_owned(), Resource::bytes(bytes.clone())))
            .collect();
        ResourceRoot::new(
            "prepared".into(),
            "cache".into(),
            ResourceSource::indexed(files),
        )
    }

    const ICONS: &str = r#"{"sources":[
        {"type":"directory","source":"icons","prefix":"icons/"},
        {"type":"single","resource":"icons_mod:misc/badge","sprite":"icons_mod:renamed"},
        {"type":"unstitch","resource":"icons_mod:misc/sheet","divisor_x":2,"divisor_y":1,
         "regions":[{"sprite":"icons_mod:left","x":0,"y":0,"width":1,"height":1}]}
    ]}"#;

    #[test]
    fn a_registered_atlas_stages_each_sprite_under_its_gui_name_and_refuses_the_rest() {
        let heart = fixture_png(2, 2, 200);
        let staged = stage_texture_atlases(
            root(&[
                ("assets/icons_mod/atlases/icons.json", ICONS.into()),
                ("assets/icons_mod/textures/icons/heart.png", heart.clone()),
                (
                    "assets/icons_mod/textures/icons/heart.png.mcmeta",
                    b"{}".to_vec(),
                ),
                (
                    "assets/icons_mod/textures/misc/badge.png",
                    fixture_png(3, 3, 9),
                ),
                (
                    "assets/icons_mod/textures/misc/sheet.png",
                    fixture_png(4, 2, 77),
                ),
                ("assets/minecraft/atlases/blocks.json", ICONS.into()),
            ]),
            &[
                answered("icons_mod", "icons_mod:icons"),
                answered("dup_mod", "icons_mod:icons"),
                answered("bad_mod", "minecraft:blocks"),
                answered("icons_mod", "icons_mod:missing"),
            ],
        );
        let source = staged.source();
        let atlas = registration("icons_mod:icons");
        let file = |sprite: &str| {
            let name = atlas.sprite_name(&Identifier::parse(sprite).unwrap());
            atlas_sources::texture_resource(&name.to_string(), "textures")
        };
        assert_eq!(
            file("icons_mod:icons/heart"),
            "assets/icons_mod/textures/gui/atlases/xicons/xicons_mod/xicons.sheart.png"
        );
        assert_eq!(source.read(&file("icons_mod:icons/heart")).unwrap(), heart);
        assert!(source.contains(&format!("{}.mcmeta", file("icons_mod:icons/heart"))));
        let badge = source.read(&file("icons_mod:renamed")).unwrap();
        assert_eq!(sunrise_client::hud::png_bytes_size(&badge), Some((3, 3)));
        let left =
            sunrise_client::hud::decode_bytes(&source.read(&file("icons_mod:left")).unwrap())
                .unwrap();
        assert_eq!(
            (left.0, left.1, left.2[0]),
            (2, 2, 77),
            "the sheet's left half"
        );
        let staged_names: Vec<_> = source
            .list("")
            .into_iter()
            .filter(|name| name.contains("/gui/atlases/"))
            .collect();
        assert_eq!(staged_names.len(), 4, "{staged_names:?}");
        assert!(
            !source.contains(
                "assets/minecraft/textures/gui/atlases/xblocks/xicons_mod/xicons.sheart.png"
            ),
            "built-in engine atlases are refused"
        );
    }

    #[test]
    fn minecraft_and_sunrise_non_builtin_atlases_are_accepted_while_builtin_atlases_are_refused() {
        let heart = fixture_png(2, 2, 123);
        let def = r#"{"sources":[{"type":"single","resource":"icons_mod:icons/heart","sprite":"icons_mod:heart"}]}"#;
        let staged = stage_texture_atlases(
            root(&[
                ("assets/minecraft/atlases/custom_icons.json", def.into()),
                ("assets/sunrise/atlases/custom_icons.json", def.into()),
                ("assets/minecraft/atlases/blocks.json", def.into()),
                ("assets/minecraft/atlases/gui.json", def.into()),
                ("assets/minecraft/atlases/beds.json", def.into()),
                ("assets/icons_mod/textures/icons/heart.png", heart.clone()),
            ]),
            &[
                answered("mod_a", "minecraft:custom_icons"),
                answered("mod_b", "sunrise:custom_icons"),
                answered("mod_c", "minecraft:blocks"),
                answered("mod_d", "minecraft:gui"),
                answered("mod_e", "minecraft:beds"),
            ],
        );
        let source = staged.source();
        assert_eq!(
            source
                .read("assets/minecraft/textures/gui/atlases/xcustom_icons/xicons_mod/xheart.png")
                .unwrap(),
            heart
        );
        assert_eq!(
            source
                .read("assets/sunrise/textures/gui/atlases/xcustom_icons/xicons_mod/xheart.png")
                .unwrap(),
            heart
        );
        assert!(
            !source.contains("assets/minecraft/textures/gui/atlases/xblocks/xicons_mod/xheart.png")
        );
        assert!(
            !source.contains("assets/minecraft/textures/gui/atlases/xgui/xicons_mod/xheart.png")
        );
        assert_eq!(
            source
                .read("assets/minecraft/textures/gui/atlases/xbeds/xicons_mod/xheart.png")
                .unwrap(),
            heart
        );
    }

    #[test]
    fn more_than_32_atlases_and_more_than_4096_sprites_are_staged_without_private_caps() {
        let dot = fixture_png(1, 1, 55);
        let single_def =
            r#"{"sources":[{"type":"single","resource":"bulk:dot","sprite":"bulk:dot"}]}"#;
        let many_sources: String = (0..4150)
            .map(|i| format!(r#"{{"type":"single","resource":"bulk:dot","sprite":"bulk:s{i}"}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let big_def = format!(r#"{{"sources":[{many_sources}]}}"#);
        let staged_many_atlases = {
            let atlas_names: Vec<_> = (0..40)
                .map(|i| format!("assets/bulk/atlases/atlas_{i}.json"))
                .collect();
            let mut files: Vec<_> = atlas_names
                .iter()
                .map(|name| (name.as_str(), single_def.as_bytes().to_vec()))
                .collect();
            files.push(("assets/bulk/textures/dot.png", dot.clone()));
            let registrations: Vec<_> = (0..40)
                .map(|i| answered("bulk", &format!("bulk:atlas_{i}")))
                .collect();
            stage_texture_atlases(root(&files), &registrations)
        };
        let atlas_count = staged_many_atlases
            .source()
            .list("assets/bulk/textures/gui/atlases/")
            .len();
        assert_eq!(atlas_count, 40);

        let staged_many_sprites = stage_texture_atlases(
            root(&[
                ("assets/bulk/atlases/big.json", big_def.into_bytes()),
                ("assets/bulk/textures/dot.png", dot),
            ]),
            &[answered("bulk", "bulk:big")],
        );
        let sprite_count = staged_many_sprites
            .source()
            .list("assets/bulk/textures/gui/atlases/xbig/")
            .len();
        assert_eq!(sprite_count, 4150);
    }

    #[test]
    fn registered_atlas_sprite_replaces_pack_png_and_does_not_inherit_its_animation() {
        let chosen = fixture_png(2, 4, 111);
        let path = "assets/mod_a/textures/gui/atlases/xicons/xmod_a/xicon.png";
        let staged = stage_texture_atlases(
            root(&[
                ("assets/mod_a/atlases/icons.json", br#"{"sources":[{"type":"single","resource":"mod_a:static","sprite":"mod_a:icon"}]}"#.to_vec()),
                ("assets/mod_a/textures/static.png", chosen.clone()),
                (path, fixture_png(2, 4, 10)),
                ("assets/mod_a/textures/gui/atlases/xicons/xmod_a/xicon.png.mcmeta", br#"{"animation":{"width":2,"height":2}}"#.to_vec()),
            ]),
            &[answered("mod_a", "mod_a:icons")],
        );
        assert_eq!(staged.source().read(path).unwrap(), chosen);
        assert!(!staged.source().contains(&format!("{path}.mcmeta")));
        let atlas = sunrise_client::hud::HudAtlas::build(staged.source()).unwrap();
        let rect = atlas
            .gui_rect("mod_a:gui/atlases/xicons/xmod_a/xicon")
            .unwrap();
        assert_eq!((rect.w, rect.h), (2, 4));
        assert!(atlas.sprite_animations.is_empty());
    }

    #[test]
    fn nested_atlas_paths_do_not_alias_other_atlases_sprite_keys() {
        let registrations = [answered("demo", "demo:a"), answered("demo", "demo:a/b")];
        let stage = |height| {
            stage_texture_atlases(root(&[
            ("assets/demo/atlases/a.json", br#"{"sources":[{"type":"single","resource":"demo:one","sprite":"b:c/x"}]}"#.to_vec()),
            ("assets/demo/atlases/a/b.json", br#"{"sources":[{"type":"single","resource":"demo:two","sprite":"c:x"}]}"#.to_vec()),
            ("assets/demo/textures/one.png", fixture_png(1, height, 1)),
            ("assets/demo/textures/two.png", fixture_png(2, height, 2)),
        ]), &registrations)
        };
        for height in [1, 2] {
            let staged = stage(height);
            let atlas = sunrise_client::hud::HudAtlas::build(staged.source()).unwrap();
            let first = atlas.gui_rect("demo:gui/atlases/xa/xb/xc.sx").unwrap();
            let second = atlas.gui_rect("demo:gui/atlases/xa.sb/xc/xx").unwrap();
            assert_eq!((first.w, first.h), (1, height));
            assert_eq!((second.w, second.h), (2, height));
        }
    }

    #[test]
    fn a_reload_lists_the_sprites_from_the_definition_as_the_packs_now_give_it() {
        let registrations = [answered("icons_mod", "icons_mod:icons")];
        let first = stage_texture_atlases(
            root(&[
                ("assets/icons_mod/atlases/icons.json", ICONS.into()),
                (
                    "assets/icons_mod/textures/icons/heart.png",
                    fixture_png(2, 2, 1),
                ),
            ]),
            &registrations,
        );
        let changed = r#"{"sources":[{"type":"directory","source":"other","prefix":""}]}"#;
        let second = stage_texture_atlases(
            root(&[
                ("assets/icons_mod/atlases/icons.json", changed.into()),
                (
                    "assets/icons_mod/textures/icons/heart.png",
                    fixture_png(2, 2, 1),
                ),
                (
                    "assets/icons_mod/textures/other/star.png",
                    fixture_png(2, 2, 5),
                ),
            ]),
            &registrations,
        );
        let heart = "assets/icons_mod/textures/gui/atlases/xicons/xicons_mod/xicons.sheart.png";
        let star = "assets/icons_mod/textures/gui/atlases/xicons/xicons_mod/xstar.png";
        assert!(first.source().contains(heart) && !first.source().contains(star));
        assert!(second.source().contains(star) && !second.source().contains(heart));
    }

    /// The definition is the packs' resource stack, as `SpriteSourceList.load` reads it: a
    /// mod's file and a pack's file above it both add their sprites, and a pack filter above
    /// them hides the lower files whole.
    #[test]
    fn the_definition_stacks_every_pack_and_a_filter_hides_the_ones_below() {
        use sunrise_client::packs::{self, Pack, Source};
        let layer = |files: &[(&str, &[u8])]| {
            ResourceSource::indexed(
                files
                    .iter()
                    .map(|(name, bytes)| ((*name).to_owned(), Resource::bytes(bytes.to_vec())))
                    .collect(),
            )
        };
        let definition = |folder: &str| {
            format!(r#"{{"sources":[{{"type":"directory","source":"{folder}","prefix":""}}]}}"#)
        };
        let heart = fixture_png(2, 2, 1);
        let star = fixture_png(2, 2, 2);
        let moon = fixture_png(2, 2, 3);
        let mod_layer = Pack::mod_layer(
            "icons_mod".into(),
            Source::Opened(layer(&[
                (
                    "assets/icons_mod/atlases/icons.json",
                    definition("hearts").as_bytes(),
                ),
                ("assets/icons_mod/textures/hearts/heart.png", &heart),
            ])),
        );
        let pack = |meta: &str, folder: &str, texture: &str, png: &[u8]| {
            packs::pack_for(
                format!("file/{folder}"),
                sunrise_client::text::Text::literal(folder),
                Source::Opened(layer(&[
                    ("pack.mcmeta", meta.as_bytes()),
                    (
                        "assets/icons_mod/atlases/icons.json",
                        definition(folder).as_bytes(),
                    ),
                    (texture, png),
                ])),
                packs::RESOURCE_FORMAT,
            )
            .unwrap()
        };
        let format = packs::RESOURCE_FORMAT.0;
        let plain = format!(r#"{{"pack":{{"pack_format":{format},"description":""}}}}"#);
        let filtering = format!(
            r#"{{"pack":{{"pack_format":{format},"description":""}},
                "filter":{{"block":[{{"namespace":"icons_mod","path":"atlases/icons\\.json"}}]}}}}"#
        );
        let stars = pack(
            &plain,
            "stars",
            "assets/icons_mod/textures/stars/star.png",
            &star,
        );
        let moons = pack(
            &filtering,
            "moons",
            "assets/icons_mod/textures/moons/moon.png",
            &moon,
        );
        let staged = |selected: &[Pack]| {
            let source = packs::resources(layer(&[]), selected).unwrap();
            let root = ResourceRoot::new("prepared".into(), "cache".into(), source);
            stage_texture_atlases(root, &[answered("icons_mod", "icons_mod:icons")])
        };
        let sprite = |name: &str| {
            format!("assets/icons_mod/textures/gui/atlases/xicons/xicons_mod/x{name}.png")
        };
        let both = staged(&[mod_layer.clone(), stars.clone()]);
        assert_eq!(both.source().read(&sprite("heart")).unwrap(), heart);
        assert_eq!(both.source().read(&sprite("star")).unwrap(), star);
        let filtered = staged(&[mod_layer, stars, moons]);
        assert!(!filtered.source().contains(&sprite("heart")));
        assert!(!filtered.source().contains(&sprite("star")));
        assert_eq!(filtered.source().read(&sprite("moon")).unwrap(), moon);
    }

    /// A mod registering two atlases on the first load only.
    struct Maps(std::sync::Arc<std::sync::Mutex<(u32, Vec<Option<mod_api::AtlasRegion>>)>>);

    fn maps() -> TextureAtlasRegistration {
        registration("maps_mod:maps")
    }

    /// An atlas whose name holds `presets`, which the menu's own picture folder also has.
    fn presets() -> TextureAtlasRegistration {
        registration("maps_mod:presets")
    }

    fn player(atlas: &TextureAtlasRegistration) -> Identifier {
        atlas.sprite_name(&Identifier::parse("minecraft:player").unwrap())
    }

    #[mod_api::__subscriptions]
    impl mod_api::Hooks for Maps {
        fn register_texture_atlases(
            &self,
            _: &mod_api::RegistrationContext,
            event: &mut mod_api::RegisterTextureAtlasesEvent<'_>,
        ) {
            let mut kept = self.0.lock().unwrap();
            kept.0 += 1;
            if kept.0 == 1 {
                event.register(maps());
                event.register(presets());
            }
        }

        fn texture_atlas_stitched(
            &self,
            host: &mut dyn mod_api::ClientHost,
            event: &mut mod_api::TextureAtlasStitchedEvent,
        ) {
            if event.atlas == mod_api::TextureAtlas::Gui {
                self.0
                    .lock()
                    .unwrap()
                    .1
                    .push(host.gui_sprite(&player(&maps())));
            }
        }
    }

    /// The registered atlas's sprite is in the GUI atlas at its own size after the first load,
    /// drawable by its name, and gone after a reload that no longer registers it.
    #[test]
    #[ignore = "daily"]
    fn a_mod_atlas_sprite_is_stitched_into_the_gui_atlas_until_a_reload_drops_it() {
        let kept = std::sync::Arc::default();
        let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
            "maps_mod",
            Box::new(Maps(std::sync::Arc::clone(&kept))),
        )]
        .into();
        mod_platform::testing::with_mods(mods, || {
            use crate::game::native_entity_refs::tests::{fixture_root, with_game_resource_root};
            let prepared = fixture_root();
            let source = ResourceSource::directory(prepared.clone()).extended(BTreeMap::from([
                ("assets/maps_mod/atlases/maps.json".into(), Resource::bytes(br#"{"sources":[{"type":"single","resource":"maps_mod:dot","sprite":"minecraft:player"}]}"#.to_vec())),
                ("assets/maps_mod/atlases/presets.json".into(), Resource::bytes(br#"{"sources":[{"type":"single","resource":"maps_mod:dot","sprite":"minecraft:player"}]}"#.to_vec())),
                ("assets/maps_mod/textures/dot.png".into(), Resource::bytes(fixture_png(8, 8, 42))),
            ]));
            let cache = tempfile::tempdir().unwrap();
            let resources = ResourceRoot::new(prepared, cache.path().to_path_buf(), source);
            with_game_resource_root(
                resources,
                crate::game::execution_role::ExecutionRole::Client,
                |game| {
                    game.texture_atlases_stitched();
                    let player = player(&maps());
                    assert_eq!(
                        game.gui_sprite_pixels(&player.to_string()),
                        Some([8.0, 8.0])
                    );
                    assert_eq!(
                        game.gui_sprite_pixels(&self::player(&presets()).to_string()),
                        Some([8.0, 8.0]),
                        "a sprite under an atlas named presets is stitched too"
                    );
                    {
                        let kept = kept.lock().unwrap();
                        assert_eq!(kept.0, 1);
                        assert_eq!(
                            kept.1.last().copied().flatten().map(|r| r.size),
                            Some([8, 8])
                        );
                    }
                    game.spawn_pack_reload(crate::game::packs::ReloadKind::Selection)
                        .unwrap();
                    let started = std::time::Instant::now();
                    while game.pack_reload.is_some() {
                        assert!(
                            started.elapsed().as_secs() < 300,
                            "the reload never finished"
                        );
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        game.drive_pack_reload();
                    }
                    let kept = kept.lock().unwrap();
                    assert_eq!(kept.0, 2);
                    assert_eq!(
                        kept.1.last().copied(),
                        Some(None),
                        "looked up in the new atlas"
                    );
                    assert_eq!(game.gui_sprite_pixels(&player.to_string()), None);
                },
            );
        });
    }

    /// Source types a test registers for itself, recording what each question's reader could
    /// list of the load's resources.
    struct Recording {
        /// The folder each question's reader lists.
        prefix: &'static str,
        seen: std::sync::Mutex<Vec<(String, Vec<String>)>>,
    }
    impl atlas_sources::ModSources for Recording {
        fn is_registered(&self, kind: &str) -> bool {
            kind == "staged_mod:own"
        }
        fn ask(
            &self,
            atlas: &Identifier,
            _: &str,
            _: &sunrise_core::nbt::Tag,
            resources: &std::sync::Arc<dyn mod_api::ResourceReader>,
        ) -> Vec<mod_api::SpriteSourceOutput> {
            self.seen
                .lock()
                .unwrap()
                .push((atlas.to_string(), resources.list(self.prefix)));
            Vec::new()
        }
    }

    /// The game's atlases and the registered ones are all listed over the stack the packs gave:
    /// no reader sees a texture staged for a registered atlas, and no directory source of a game
    /// atlas lists one. Distinct registered definitions are each asked under their own id; another
    /// registration of the same definition does not ask its sources twice.
    #[test]
    fn the_staged_textures_join_the_generation_after_every_listing() {
        let blocks = r#"{"sources":[
            {"type":"directory","source":"gui/atlases","prefix":"leak/"},
            {"type":"staged_mod:own"}]}"#;
        let icons = r#"{"sources":[
            {"type":"directory","source":"icons","prefix":"icons/"},
            {"type":"staged_mod:own"}]}"#;
        let unasked = r#"{"sources":[{"type":"staged_mod:own"}]}"#;
        let staged_root = root(&[
            ("assets/minecraft/atlases/blocks.json", blocks.into()),
            ("assets/icons_mod/atlases/first.json", icons.into()),
            ("assets/icons_mod/atlases/second.json", icons.into()),
            ("assets/icons_mod/atlases/unregistered.json", unasked.into()),
            (
                "assets/icons_mod/textures/icons/heart.png",
                fixture_png(2, 2, 1),
            ),
        ]);
        let mods = Recording {
            prefix: "assets/icons_mod/textures/",
            seen: Default::default(),
        };
        let staged = stage_with(
            staged_root,
            &[
                answered("icons_mod", "icons_mod:first"),
                answered("icons_mod", "icons_mod:second"),
                answered("dupe", "icons_mod:second"),
            ],
            &mods,
        );
        assert!(
            staged
                .source()
                .list("assets/icons_mod/textures/")
                .iter()
                .any(|name| name.contains("gui/atlases")),
            "the sprites are staged"
        );
        let mut seen = mods.seen.lock().unwrap().clone();
        seen.sort();
        let listed = vec!["assets/icons_mod/textures/icons/heart.png".to_owned()];
        assert_eq!(
            seen,
            [
                ("icons_mod:first".to_owned(), listed.clone()),
                ("icons_mod:second".to_owned(), listed.clone()),
                ("minecraft:blocks".to_owned(), listed),
            ],
            "each asked once, none of them over a staged texture"
        );
        let listing = atlas_sources::shared(staged.source(), &mods);
        assert_eq!(
            listing
                .sprites("minecraft:blocks")
                .keys()
                .collect::<Vec<_>>(),
            Vec::<&String>::new(),
            "the blocks atlas's directory source lists no staged texture"
        );
        assert_eq!(
            mods.seen.lock().unwrap().len(),
            3,
            "and nothing is asked again"
        );
    }

    #[test]
    fn no_registration_leaves_the_root_as_it_was() {
        let base = root(&[("assets/icons_mod/atlases/icons.json", ICONS.into())]);
        let staged = stage_texture_atlases(base.clone(), &[]);
        assert!(staged.source().same_generation(base.source()));
    }

    /// A mod's sprite source, answering its entries in `icons_mod:icons`: a 2x2 sprite of the
    /// entry's `shade`, and the removal of every listed star.
    struct Checker;
    #[mod_api::__subscriptions]
    impl mod_api::Hooks for Checker {
        fn register_sprite_sources(
            &self,
            _: &mod_api::RegistrationContext,
            event: &mut mod_api::RegisterSpriteSourcesEvent<'_>,
        ) {
            event.register(Identifier::parse("checker_mod:checker").unwrap(), |input| {
                if input.atlas.path() != "icons" {
                    return Ok(Vec::new());
                }
                let shade = match input.config.get("shade") {
                    Some(sunrise_core::nbt::Tag::Byte(shade)) => *shade as u8,
                    _ => 0,
                };
                Ok(vec![
                    mod_api::SpriteSourceOutput::Pixels {
                        sprite: Identifier::parse("icons_mod:checker").unwrap(),
                        width: 2,
                        height: 2,
                        rgba: [shade, shade, shade, 255].repeat(4),
                    },
                    mod_api::SpriteSourceOutput::Remove {
                        sprite: Identifier::parse("icons_mod:icons/star").unwrap(),
                    },
                ])
            });
            event.register(Identifier::parse("checker_mod:empty").unwrap(), |_| {
                Ok(Vec::new())
            });
        }
    }

    /// A registered source type lists the mod's sprites where its entry stands, a reload asks
    /// again with the definition as the packs now give it, and a provider returning no outputs
    /// leaves earlier sources intact.
    #[test]
    fn a_mods_sprite_source_lists_its_sprites_and_answers_again_on_reload() {
        let stage = |kind: &str, shade: u8| {
            let definition = format!(
                r#"{{"sources":[
                    {{"type":"directory","source":"icons","prefix":"icons/"}},
                    {{"type":"{kind}","shade":{shade}}}
                ]}}"#
            );
            stage_texture_atlases(
                root(&[
                    (
                        "assets/icons_mod/atlases/icons.json",
                        definition.into_bytes(),
                    ),
                    (
                        "assets/icons_mod/textures/icons/heart.png",
                        fixture_png(2, 2, 1),
                    ),
                    (
                        "assets/icons_mod/textures/icons/star.png",
                        fixture_png(2, 2, 2),
                    ),
                ]),
                &[answered("icons_mod", "icons_mod:icons")],
            )
        };
        let gui = "assets/icons_mod/textures/gui/atlases/xicons/xicons_mod";
        let (checker, heart, star) = (
            format!("{gui}/xchecker.png"),
            format!("{gui}/xicons.sheart.png"),
            format!("{gui}/xicons.sstar.png"),
        );
        let checker_pixels = |staged: &ResourceRoot| {
            sunrise_client::hud::decode_bytes(&staged.source().read(&checker).unwrap()).unwrap()
        };
        let mods: mod_api::Mods =
            vec![mod_api::ModEntry::new("checker_mod", Box::new(Checker))].into();
        mod_platform::testing::with_mods(mods, || {
            let mut types = Vec::new();
            Game::native_register_sprite_sources(&mod_api::RegistrationContext::new(), &mut types);
            assert_eq!(
                sunrise_client_render::mod_sprite_sources::register(types),
                ["checker_mod:checker", "checker_mod:empty"]
            );
            let first = stage("checker_mod:checker", 40);
            assert_eq!(checker_pixels(&first), (2, 2, [40, 40, 40, 255].repeat(4)));
            assert!(first.source().contains(&heart));
            assert!(
                !first.source().contains(&star),
                "the source removed the star"
            );
            let reloaded = stage("checker_mod:checker", 90);
            assert_eq!(checker_pixels(&reloaded).2[..4], [90, 90, 90, 255]);
            let empty = stage("checker_mod:empty", 40);
            assert!(!empty.source().contains(&checker));
            assert!(empty.source().contains(&star) && empty.source().contains(&heart));
        });
    }

    struct EmissiveMod(&'static str, &'static str);
    #[mod_api::__subscriptions]
    impl mod_api::Hooks for EmissiveMod {
        fn emissive(
            &self,
            _: &mod_api::RegistrationContext,
            event: &mut mod_api::EmissiveEvent<'_>,
        ) {
            event.register(EmissiveSpriteRegistration::new(
                Identifier::parse(self.0).unwrap(),
                Identifier::parse(self.1).unwrap(),
            ));
        }
    }

    #[test]
    fn collect_emissive_sprites_gathers_typed_registrations_with_mod_attribution() {
        let mods: mod_api::Mods = vec![
            mod_api::ModEntry::new(
                "alpha",
                Box::new(EmissiveMod("minecraft:block/stone", "alpha:glow/stone")),
            ),
            mod_api::ModEntry::new(
                "beta",
                Box::new(EmissiveMod("beta:block/lamp", "beta:block/lamp_e")),
            ),
        ]
        .into();
        mod_platform::testing::with_mods(mods, || {
            let collected = collect_emissive_sprites();
            assert_eq!(collected.len(), 2);
            assert_eq!(collected[0].mod_id, "alpha");
            assert_eq!(
                collected[0].answer,
                EmissiveSpriteRegistration::new(
                    Identifier::parse("minecraft:block/stone").unwrap(),
                    Identifier::parse("alpha:glow/stone").unwrap(),
                )
            );
            assert_eq!(collected[1].mod_id, "beta");
            assert_eq!(
                collected[1].answer,
                EmissiveSpriteRegistration::new(
                    Identifier::parse("beta:block/lamp").unwrap(),
                    Identifier::parse("beta:block/lamp_e").unwrap(),
                )
            );
        });
    }
}
