//! Stage the sprites of mod-registered texture atlases (`register_texture_atlases`) into a
//! client resource generation, as GUI textures under each sprite's `sprite_name`.
use super::Game;
use mod_api::__private::game::*;
use mod_api::{Answered, Identifier, TextureAtlasRegistration};
use std::collections::{BTreeMap, btree_map::Entry};
use sunrise_assets::resource_source::{Resource, ResourceRoot, ResourceSource};
use sunrise_client::atlas_sources::{self, SpriteOrigin};

pub(in crate::game) fn collect_emissive_sprites() -> Vec<Answered<(Identifier, Identifier)>> {
    let mut layers = Vec::new();
    if mod_api::__private::listened(mod_api::HookId::RegisterEmissiveSprites) {
        let _phase =
            mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::Registration);
        Game::native_register_emissive_sprites(&mod_api::RegistrationContext::new(), &mut layers);
    }
    layers
}

pub(in crate::game) fn collect_texture_atlases() -> Vec<Answered<TextureAtlasRegistration>> {
    let mut atlases = Vec::new();
    if mod_api::__private::listened(mod_api::HookId::RegisterTextureAtlases) {
        let _phase =
            mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::Registration);
        Game::native_register_texture_atlases(&mod_api::RegistrationContext::new(), &mut atlases);
    }
    atlases
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
    // NeoForge registers by definition id; TextureManager.register replaces by texture slot.
    let mut definitions = BTreeMap::new();
    let mut slots = BTreeMap::new();
    for registration in atlases {
        let TextureAtlasRegistration { atlas, definition } = &registration.answer;
        let builtin = definition.namespace() == "minecraft"
            && atlas_sources::BUILTIN_ATLASES
                .iter()
                .any(|(id, _)| *id == definition.path());
        let owner = registration.mod_id;
        if builtin {
            log::error!(
                "mod {owner}: duplicate atlas definition {definition} for slot {atlas}; the game already registered it"
            );
            continue;
        }
        match definitions.entry(definition) {
            Entry::Occupied(old) => {
                let (previous_owner, previous_atlas) = old.get();
                log::error!(
                    "mod {owner}: duplicate atlas definition {definition} for slot {atlas}; mod {previous_owner} registered it for {previous_atlas}"
                );
                continue;
            }
            Entry::Vacant(entry) => {
                entry.insert((owner, atlas));
            }
        }
        if let Some(previous) = slots.insert(atlas.clone(), registration) {
            log::info!(
                "mod {owner}: texture atlas slot {atlas} replaces definition {} from mod {} with {definition}",
                previous.answer.definition,
                previous.mod_id
            );
        }
    }
    let mut listed = atlas_sources::registered(
        source,
        mods,
        &slots
            .values()
            .map(|entry| (&entry.answer.atlas, &entry.answer.definition))
            .collect::<Vec<_>>(),
    );
    // Only the winners are listed. A missing or failed definition is an empty slot, not a
    // reason to re-enable the old slot or abort another atlas. All sources see the original packs.
    for atlas in slots.keys() {
        listed.entry(atlas.clone()).or_insert_with(|| {
            let entry = slots[atlas];
            log::warn!(
                "mod {}: texture atlas {atlas}: no usable definition {}",
                entry.mod_id,
                entry.answer.definition
            );
            atlas_sources::Listed::default()
        });
    }
    atlas_sources::publish_texture_slots(source, mods, &listed);
    let mut files = BTreeMap::new();
    for (atlas, content) in listed {
        let registration = slots[&atlas];
        let owner = registration.mod_id;
        for (id, origin) in &content.sprites {
            let Ok(sprite) = Identifier::parse(id) else {
                log::warn!("mod {owner}: texture atlas {atlas}: invalid sprite id {id:?}");
                continue;
            };
            let name = registration.answer.sprite_name(&sprite);
            let path = format!("assets/{}/textures/{}.png", name.namespace(), name.path());
            if !sunrise_assets::resource_source::stays_inside(&path) {
                log::warn!(
                    "mod {owner}: texture atlas {atlas}: sprite {id} has no usable resource path"
                );
                continue;
            }
            if let Err(error) = stage_sprite(source, origin, &path, &mut files) {
                log::warn!("mod {owner}: texture atlas {atlas}: sprite {id} skipped: {error}");
            } else if source.contains(&path) {
                log::info!(
                    "mod {owner}: texture atlas {atlas}: sprite {id} replaces pack texture {path}"
                );
            }
        }
    }
    root.with_runtime_textures(files)
}

fn stage_sprite(
    source: &ResourceSource,
    origin: &SpriteOrigin,
    path: &str,
    files: &mut BTreeMap<String, Resource>,
) -> Result<(), String> {
    if let SpriteOrigin::File(texture) = origin {
        let file = atlas_sources::texture_resource(texture, "textures");
        if !source.contains(&file) {
            return Err(format!("missing texture {file}"));
        }
        let mcmeta = format!("{file}.mcmeta");
        let staged_meta = format!("{path}.mcmeta");
        if source.contains(&mcmeta) {
            files.insert(staged_meta, Resource::file(source.clone(), mcmeta));
        } else if source.contains(&staged_meta) {
            // A replaced pack texture cannot supply metadata for a different image.
            files.insert(staged_meta, Resource::bytes(b"{}".to_vec()));
        }
        files.insert(path.to_owned(), Resource::file(source.clone(), file));
        return Ok(());
    }
    let load = |texture: &str| {
        let file = atlas_sources::texture_resource(texture, "textures");
        sunrise_client::hud::decode_bytes(&source.read(&file).ok()?).ok()
    };
    let png = atlas_sources::generate(origin, &load)
        .and_then(encode)
        .ok_or_else(|| "could not generate texture".to_owned())?;
    // An unrelated pack file at this texture slot must not supply its metadata either.
    let staged_meta = format!("{path}.mcmeta");
    if source.contains(&staged_meta) {
        files.insert(staged_meta, Resource::bytes(b"{}".to_vec()));
    }
    files.insert(path.to_owned(), Resource::bytes(png));
    Ok(())
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

    fn registration(atlas: &str, definition: &str) -> Answered<TextureAtlasRegistration> {
        Answered {
            mod_id: "icons_mod",
            answer: TextureAtlasRegistration {
                atlas: Identifier::parse(atlas).unwrap(),
                definition: Identifier::parse(definition).unwrap(),
            },
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
                registration("icons_mod:icons", "icons_mod:icons"),
                registration("icons_mod:icons", "icons_mod:icons"),
                registration("minecraft:blocks", "minecraft:blocks"),
                registration("icons_mod:missing", "icons_mod:missing"),
            ],
        );
        let source = staged.source();
        let atlas = registration("icons_mod:icons", "icons_mod:icons").answer;
        let file = |sprite: &str| {
            let name = atlas.sprite_name(&Identifier::parse(sprite).unwrap());
            atlas_sources::texture_resource(&name.to_string(), "textures")
        };
        assert_eq!(
            file("icons_mod:icons/heart"),
            "assets/icons_mod/textures/gui/atlases/5/icons/icons_mod/icons/heart.png"
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
                "assets/minecraft/textures/gui/atlases/6/blocks/icons_mod/icons/heart.png"
            ),
            "an already registered built-in definition is refused"
        );
    }

    #[test]
    fn texture_slots_replace_but_definitions_register_once() {
        let definition = |texture| {
            format!(r#"{{"sources":[{{"type":"single","resource":"test:{texture}","sprite":"test:icon"}}]}}"#).into_bytes()
        };
        let red = fixture_png(2, 2, 10);
        let green = fixture_png(2, 2, 20);
        let staged = stage_texture_atlases(
            root(&[
                ("assets/test/atlases/first.json", definition("red")),
                ("assets/test/atlases/second.json", definition("green")),
                ("assets/minecraft/atlases/gui.json", definition("red")),
                ("assets/test/textures/red.png", red.clone()),
                ("assets/test/textures/green.png", green.clone()),
                (
                    "assets/minecraft/textures/gui/atlases/22/textures/atlas/gui.png/test/icon.png",
                    red,
                ),
            ]),
            &[
                registration("minecraft:textures/atlas/gui.png", "test:first"),
                registration("minecraft:textures/atlas/gui.png", "test:second"),
                registration("sunrise:duplicate", "test:second"),
                registration("test:missing", "test:missing"),
            ],
        );
        assert_eq!(
            staged
                .source()
                .read(
                    "assets/minecraft/textures/gui/atlases/22/textures/atlas/gui.png/test/icon.png"
                )
                .unwrap(),
            green
        );
        assert!(
            !staged
                .source()
                .contains("assets/sunrise/textures/gui/atlases/9/duplicate/test/icon.png")
        );
        let assets = atlas_sources::listing(staged.source());
        assert!(
            matches!(assets.sprites("minecraft:gui").get("test:icon"), Some(SpriteOrigin::File(name)) if name == "test:green")
        );
    }

    #[test]
    fn atlas_and_sprite_counts_do_not_end_a_resource_generation() {
        let definition =
            br#"{"sources":[{"type":"single","resource":"test:one","sprite":"test:icon"}]}"#;
        let pixel = Resource::bytes(fixture_png(1, 1, 55));
        let mut resources =
            BTreeMap::from([("assets/test/textures/one.png".into(), pixel.clone())]);
        let mut registrations: Vec<_> = (0..33)
            .map(|i| {
                resources.insert(
                    format!("assets/sunrise/atlases/atlas_{i}.json"),
                    Resource::bytes(definition.to_vec()),
                );
                registration(&format!("sunrise:atlas_{i}"), &format!("sunrise:atlas_{i}"))
            })
            .collect();
        resources.insert(
            "assets/test/atlases/many.json".into(),
            Resource::bytes(
                br#"{"sources":[{"type":"directory","source":"many","prefix":""}]}"#.to_vec(),
            ),
        );
        for i in 0..4100 {
            resources.insert(
                format!("assets/test/textures/many/s{i:04}.png"),
                pixel.clone(),
            );
        }
        registrations.push(registration("test:many", "test:many"));
        let resources = ResourceRoot::new(
            "prepared".into(),
            "cache".into(),
            ResourceSource::indexed(resources),
        );
        let staged = stage_texture_atlases(resources, &registrations);
        assert!(
            staged
                .source()
                .contains("assets/sunrise/textures/gui/atlases/8/atlas_32/test/icon.png")
        );
        assert!(
            staged
                .source()
                .contains("assets/test/textures/gui/atlases/4/many/test/s4099.png")
        );
    }

    #[test]
    fn a_reload_lists_the_sprites_from_the_definition_as_the_packs_now_give_it() {
        let registrations = [registration("icons_mod:icons", "icons_mod:icons")];
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
        let heart = "assets/icons_mod/textures/gui/atlases/5/icons/icons_mod/icons/heart.png";
        let star = "assets/icons_mod/textures/gui/atlases/5/icons/icons_mod/star.png";
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
            stage_texture_atlases(root, &[registration("icons_mod:icons", "icons_mod:icons")])
        };
        let sprite = |name: &str| {
            format!("assets/icons_mod/textures/gui/atlases/5/icons/icons_mod/{name}.png")
        };
        let both = staged(&[mod_layer.clone(), stars.clone()]);
        assert_eq!(both.source().read(&sprite("heart")).unwrap(), heart);
        assert_eq!(both.source().read(&sprite("star")).unwrap(), star);
        let filtered = staged(&[mod_layer, stars, moons]);
        assert!(!filtered.source().contains(&sprite("heart")));
        assert!(!filtered.source().contains(&sprite("star")));
        assert_eq!(filtered.source().read(&sprite("moon")).unwrap(), moon);
    }

    /// A mod registering its atlases on the first load only.
    struct Maps(std::sync::Arc<std::sync::Mutex<(u32, Vec<Option<mod_api::AtlasRegion>>)>>);

    fn maps() -> TextureAtlasRegistration {
        registration("maps_mod:maps", "maps_mod:maps").answer
    }

    /// An atlas whose name holds `presets`, which the menu's own picture folder also has.
    fn presets() -> TextureAtlasRegistration {
        registration("maps_mod:presets", "maps_mod:presets").answer
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
            use crate::game::native_entity_refs::tests::{fixture_root, with_game_resources};
            let definition = br#"{"sources":[{"type":"single","resource":"maps_mod:icon","sprite":"minecraft:player"}]}"#;
            let resources =
                ResourceRoot::directory(fixture_root()).with_runtime_textures(BTreeMap::from([
                    (
                        "assets/maps_mod/atlases/maps.json".into(),
                        Resource::bytes(definition.to_vec()),
                    ),
                    (
                        "assets/maps_mod/atlases/presets.json".into(),
                        Resource::bytes(definition.to_vec()),
                    ),
                    (
                        "assets/maps_mod/textures/icon.png".into(),
                        Resource::bytes(fixture_png(8, 8, 255)),
                    ),
                ]));
            with_game_resources(
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
    /// atlas lists one. The registered definitions are asked under their texture slot ids.
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
            ("assets/minecraft/atlases/pack_icons.json", icons.into()),
            (
                "assets/minecraft/atlases/pack_icons_second.json",
                icons.into(),
            ),
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
                registration("icons_mod:first", "minecraft:pack_icons"),
                registration("icons_mod:second", "minecraft:pack_icons_second"),
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
    fn a_panicking_registrar_loses_only_its_own_batch_and_is_not_called_on_reload() {
        struct Registrar {
            fail: bool,
            calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        }
        #[mod_api::__subscriptions]
        impl mod_api::Hooks for Registrar {
            fn register_texture_atlases(
                &self,
                _: &mod_api::RegistrationContext,
                event: &mut mod_api::RegisterTextureAtlasesEvent<'_>,
            ) {
                self.calls
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let id = if self.fail { "test:bad" } else { "test:good" };
                event.register(registration(id, id).answer);
                assert!(
                    !self.fail,
                    "registrar fixture panic after submitting an atlas"
                );
            }
        }
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let bad = Arc::new(AtomicUsize::new(0));
        let good = Arc::new(AtomicUsize::new(0));
        let mods: mod_api::Mods = vec![
            mod_api::ModEntry::new(
                "bad",
                Box::new(Registrar {
                    fail: true,
                    calls: bad.clone(),
                }),
            ),
            mod_api::ModEntry::new(
                "good",
                Box::new(Registrar {
                    fail: false,
                    calls: good.clone(),
                }),
            ),
        ]
        .into();
        mod_platform::testing::with_mods(mods, || {
            for _ in 0..2 {
                let atlases = collect_texture_atlases();
                assert_eq!(atlases.len(), 1);
                assert_eq!(atlases[0].mod_id, "good");
                assert_eq!(atlases[0].answer.atlas.to_string(), "test:good");
            }
        });
        assert_eq!(bad.load(Ordering::Relaxed), 1);
        assert_eq!(good.load(Ordering::Relaxed), 2);
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
                &[registration("icons_mod:icons", "icons_mod:icons")],
            )
        };
        let gui = "assets/icons_mod/textures/gui/atlases/5/icons/icons_mod";
        let (checker, heart, star) = (
            format!("{gui}/checker.png"),
            format!("{gui}/icons/heart.png"),
            format!("{gui}/icons/star.png"),
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
}
