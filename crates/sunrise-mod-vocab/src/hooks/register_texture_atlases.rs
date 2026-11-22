//! Client texture atlases a mod adds before each atlas build.

pub trait Hooks {
    /// Declare atlases beside the game's (`RegisterTextureAtlasesEvent`). Call
    /// `event.register(atlas)` with each `TextureAtlasRegistration`, which names the atlas
    /// definition that lists its sprites. Called at first client load and every resource reload,
    /// before textures are stitched.
    #[notes = r#"## Texture atlases

`register_texture_atlases` receives a registration context. Call `event.register(atlas)`
with `TextureAtlasRegistration { atlas }`. The atlas ID also names its definition:
`mymod:icons` reads `assets/mymod/atlases/icons.json`. The definition is a `sources`
list like vanilla's atlas definitions, and resource packs change it the same way.

Sunrise packs vanilla's atlases into two, so a registered atlas's sprites are
stitched into the GUI atlas as a batch, each under the key returned by
`TextureAtlasRegistration::sprite_name(&sprite_id)`. Look it up with `gui_sprite` in `texture_atlas_stitched` for `TextureAtlas::Gui`
and draw it with `DrawContext::sprite` by that name. Every reload lists the
sprites again from the definition as the packs now give it.

The hook runs at client startup and on the game-owning thread before reload
staging; re-submit the atlases on each call. Any namespace is allowed, including
`minecraft:` and `sunrise:`. Collisions with vanilla's built-in engine atlases
(`AtlasManager.KNOWN_ATLASES`, such as `minecraft:blocks` or `minecraft:gui`)
and duplicate atlas ids are logged as errors naming the registering mod and
earlier owner, keeping the first registration (matching NeoForge's
`RegisterTextureAtlasesEvent.register`). There is no synthetic atlas or sprite
count cap. If a batch does not fit the renderer's texture limit, the engine logs
and skips that atlas without changing the previously accepted layout. A bad image
is skipped with its own diagnostic. A registered sprite replaces a raw pack file
at the same key with a warning and uses only its own source metadata."#]
    #[hook(
        receiver = "associated",
        dispatch = "flat",
        side = "client",
        contract = "collect",
        out = "atlases",
        answered_by = "mod",
        registration_append = "register",
        context = "registration",
        neoforge = "RegisterTextureAtlasesEvent",
        version = 3
    )]
    fn register_texture_atlases(atlases: &mut Vec<Answered<TextureAtlasRegistration>>);
}
