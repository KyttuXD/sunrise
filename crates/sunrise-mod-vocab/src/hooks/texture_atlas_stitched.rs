//! The `texture_atlas_stitched` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// An atlas was stitched and published (`TextureAtlasStitchedEvent`): `atlas` names it. Fires
    /// once per atlas, the blocks' first and then the GUI's, on the first load before
    /// `client_setup` and after every successful resource reload (F3+T, a pack turned on or off, a
    /// server's pack) before `client_resources_reloaded`; a reload that fails fires nothing. Look
    /// the sprites a mod draws up here, once. For [`TextureAtlas::Blocks`], use
    /// `ClientHost::texture_sprite` and keep its `SpriteRef`, not the borrowed atlas entry.
    /// For [`TextureAtlas::Gui`], use `gui_sprite` and keep its `AtlasRegion` until the next
    /// stitch. Never look them up by name every frame. Client-side.
    #[hook(
        dispatch = "flat",
        side = "client",
        contract = "listen",
        cancellable = false,
        neoforge = "TextureAtlasStitchedEvent"
    )]
    fn texture_atlas_stitched(atlas: mod_api::TextureAtlas);
}
