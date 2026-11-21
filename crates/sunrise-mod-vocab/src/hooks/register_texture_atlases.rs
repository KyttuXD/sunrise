//! Client texture atlases a mod adds before each atlas build.

pub trait Hooks {
    /// Declare atlases beside the game's (`RegisterTextureAtlasesEvent`). Call
    /// `event.register(atlas)` with each `TextureAtlasRegistration`, which names the atlas
    /// definition that lists its sprites. Called at first client load and every resource reload,
    /// before textures are stitched.
    #[notes = r#"## Texture atlases

`register_texture_atlases` receives a registration context. Call `event.register(atlas)`
with a `TextureAtlasRegistration` giving a typed texture slot (`mymod:icons`)
and a typed definition id (`mymod:icons` reads `assets/mymod/atlases/icons.json`).
Use any valid namespace. Definitions are keyed by id; a duplicate definition
(including a built-in definition) is refused and reported. As in the texture
manager, a later admitted definition for the same texture slot replaces it.

Sunrise stitches these sprites into its GUI atlas. Use
`TextureAtlasRegistration::sprite_name` to get the GUI key, then `gui_sprite`
in `texture_atlas_stitched` for `TextureAtlas::Gui` to look it up. Draw it with
`DrawContext::sprite`. Do not construct the GUI key by hand.

Submit declarations at first client load and each resource reload. Definitions
are read from the active pack stack, and sprite sources replace earlier entries
with the same key. A built-in slot is identified by its texture location, e.g.
`minecraft:textures/atlas/gui.png`. Unreadable definitions and sprites and atlases
that do not fit the GUI sheet are reported without discarding unrelated atlases."#]
    #[hook(
        receiver = "associated",
        dispatch = "flat",
        side = "client",
        contract = "collect",
        out = "atlases",
        registration_append = "register",
        context = "registration",
        answered_by = "mod",
        owned_answer_batch = true,
        neoforge = "RegisterTextureAtlasesEvent",
        version = 3
    )]
    fn register_texture_atlases(atlases: &mut Vec<Answered<TextureAtlasRegistration>>);
}
