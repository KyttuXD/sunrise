//! Host method declarations; access defaults and game forwarders are generated.
#![allow(unused_variables)]
use crate::*;
/// The client host's methods of the `client_render` area, implemented by the game and reached through [`crate::ClientHost`].
#[sunrise_mod_api_macros::host_area]
pub trait ClientRenderClientHost {
    /// Atomically overwrite retained model copies; use [`InstanceGroupRef::write`].
    #[access(PublishedResources)]
    fn instance_group_write(
        &self,
        group: InstanceGroupRef,
        first: u32,
        instances: &[ModelInstance],
    ) -> Result<(), GroupError>;
    /// Set the drawn length; use [`InstanceGroupRef::set_len`].
    #[access(PublishedResources)]
    fn instance_group_set_len(&self, group: InstanceGroupRef, len: u32) -> Result<(), GroupError>;
    /// Replace the group pose; use [`InstanceGroupRef::set_group_transform`].
    #[access(PublishedResources)]
    fn instance_group_set_transform(
        &self,
        group: InstanceGroupRef,
        transform: [f32; 12],
    ) -> Result<(), GroupError>;
    /// Retire retained model copies; use [`InstanceGroupRef::destroy`].
    #[access(PublishedResources)]
    fn instance_group_destroy(&self, group: InstanceGroupRef) -> Result<(), GroupError>;
    /// Side: client.
    /// `ItemStackRenderState.usesBlockLight` of `stack`'s model resolved for `context`
    /// (`ItemModelResolver.updateForTopItem`, no owner or level): whether the model is lit as a
    /// block is, from the side (`gui_light: side`, vanilla's 3D item models), where a flat item
    /// is lit from the front. It is what the 1.21.3 `BakedModel.isGui3d` became: a mod uses it to
    /// draw a stack as a flat sprite or as a 3D model. False for an
    /// empty or invalid stack, and for a model that draws nothing. Reads the model the client
    /// resolved from its resource packs.
    #[access(ClientSetup)]
    fn item_uses_block_light(&self, stack: &ItemStack, context: ItemDisplayContext) -> bool;
    /// Side: client.
    /// Asks `block_render_visibility` again for every state and, when the answers differ
    /// from those in use, remeshes every loaded section with them. While a mod listens to
    /// `section_visibility`, rebuilds meshes in the player's current level and asks about them
    /// again, except for a first all-air compile with no section geometry contributors. Other
    /// viewed levels do not call that hook; no NeoForge counterpart has been verified. Call it
    /// when a decision changes (a toggle key, a scan finishing), not every frame. With no part
    /// answering or listener, the mesher keeps its no-mod path.
    #[access(WorldOnly)]
    fn refresh_render_visibility(&mut self);
    /// Side: client.
    /// Asks `biome_colors` again for every biome and, when the colours
    /// differ from those in use, rebuilds every loaded section and the far
    /// terrain with them (`LevelRenderer.allChanged`, as changing Biome Blend
    /// does). Call it when your colours change (a new season, a new day of a
    /// slow fade) and once when the player joins, not every frame: a rebuild
    /// takes the workers a few seconds.
    #[access(WorldOnly)]
    fn refresh_biome_colors(&mut self);
    /// Side: client.
    /// Vanilla's `TextureManager.register` of a `DynamicTexture` under `id`, then its `upload`:
    /// the calling mod's RGBA image `id`, in its own namespace (`mymod:dynamic/panel`), made from
    /// `rgba`, `width * height` pixels of four bytes, row by row from the top left. The image
    /// takes a slot of the HUD atlas's dynamic texture pool, of the smallest class
    /// ([`DYNAMIC_TEXTURE_CLASSES`]) that holds it, and `id` names it as a GUI sprite of
    /// `width` x `height` pixels: [`DrawContext::sprite`] (`src` in the image's own pixels),
    /// [`DrawContext::gui_nine_slice`], [`DrawContext::tiled_sprite`] and
    /// [`gui_sprite`](Self::gui_sprite) draw and find it as they do any GUI sprite, from the next
    /// frame on. It lasts, through resource reloads, until
    /// [`free_dynamic_texture`](Self::free_dynamic_texture) or the game's end.
    ///
    /// Refused by name, with nothing changed and nothing allocated, as [`DynamicTextureError`]
    /// lists: `Unavailable` when the atlas has no pool, `NotOwned` for an id outside the mod's
    /// namespace, `BadSize` for a side of 0 or pixels that are not `width * height * 4` bytes,
    /// `TooLarge` for a side past [`DYNAMIC_TEXTURE_MAX_SIDE`], `NameTaken` for an id that
    /// already names an image or a GUI sprite, `OverBudget` past the mod's
    /// [`DYNAMIC_TEXTURE_MOD_IMAGES`] images or [`DYNAMIC_TEXTURE_MOD_BYTES`] of pixels, and
    /// `NoSlot` when every slot of the image's class is held. The game keeps a copy of the
    /// pixels, counted in the byte budget, to upload them again into a rebuilt atlas.
    #[access(WorldOnly)]
    fn create_dynamic_texture(
        &mut self,
        id: &Identifier,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<(), DynamicTextureError>;
    /// Side: client.
    /// Vanilla's `DynamicTexture.upload` of new pixels: the calling mod's image `id` takes
    /// `rgba`, `width * height` pixels of four bytes, written into the atlas slot it already holds
    /// from the next frame on. The size may change within the image's class; the sprite `id`
    /// takes the new size. Each update is one copy into the kept pixels and one upload of the
    /// image, so send one when the image changed, not every frame.
    ///
    /// Refused by name, with nothing changed, as [`DynamicTextureError`] lists: `Unavailable`,
    /// `NotOwned`, `Unknown` for an id that names no image, `BadSize`, `TooLarge`,
    /// `ClassChange` for a size of another class than the image's (free it and create it again),
    /// and `OverBudget` when the new size would pass [`DYNAMIC_TEXTURE_MOD_BYTES`].
    #[access(WorldOnly)]
    fn update_dynamic_texture(
        &mut self,
        id: &Identifier,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<(), DynamicTextureError>;
    /// Side: client.
    /// Vanilla's `TextureManager.release`: frees the calling mod's image `id`. The GUI sprite
    /// `id` is gone at once, the slot goes back to the pool for any mod, and the image and its
    /// bytes go back to the mod's budget. Refused by name, with nothing changed:
    /// `Unavailable`, `NotOwned` for an id outside the mod's namespace and `Unknown` for one
    /// that names no image.
    #[access(WorldOnly)]
    fn free_dynamic_texture(&mut self, id: &Identifier) -> Result<(), DynamicTextureError>;
    /// Side: client.
    /// What the calling mod may still create: images and bytes of pixels left of its
    /// [`DYNAMIC_TEXTURE_MOD_IMAGES`] and [`DYNAMIC_TEXTURE_MOD_BYTES`]. These are limits of the
    /// atlas pool, which vanilla's `TextureManager` does not have. A free slot of a class is not
    /// part of it: the slots are shared by every mod. `Unavailable` when the atlas has no pool,
    /// `NotOwned` when no mod is calling.
    #[access(WorldOnly)]
    fn dynamic_texture_budget(&self) -> Result<DynamicTextureBudget, DynamicTextureError>;
    /// The resolver's blended colour at `pos` in the current client level, as opaque ARGB.
    /// Like `ClientLevel.getBlockTint` (`BiomeColors.getAverageGrassColor` and its counterparts),
    /// this samples a square at the block's height using the client's biome blend radius and the
    /// same biome colours as the mesher, including updates from [`Self::refresh_biome_colors`].
    /// Use [`ColorResolver::Grass`] or [`ColorResolver::Foliage`] without looking up a block state or
    /// a model. For a specific block or fluid face, use [`Self::block_tint_color`].
    /// Returns [`LevelReadError::Unavailable`] outside the World phase or when the host has
    /// no client level to read.
    #[access(WorldOnly)]
    fn biome_tint(&self, pos: [i32; 3], resolver: ColorResolver) -> Result<u32, LevelReadError>;
    /// Side: client.
    /// For a client mod drawing a block's faces itself (a copycat drawing its material's, from
    /// `visit_block_sprites`): the 0xRRGGBB colour a face of `state` with tint index `tint_index`
    /// (`FaceSprite::tint_index`) is multiplied by at `pos`, as the chunk mesher tints it there
    /// (biome blend included). Grass, foliage and water use the client's current biome colours,
    /// including updates from [`Self::refresh_biome_colors`]. A face's own tint source or a pack
    /// colormap can give it a different colour. Returns `None` for an untinted face (-1, or an
    /// index past the state's tints). For a biome colour independent of the face's tint source,
    /// use [`Self::biome_tint`].
    #[access(WorldOnly)]
    fn block_tint_color(&self, state: BlockState, tint_index: i32, pos: [i32; 3]) -> Option<u32>;
    /// Side: client.
    /// Published asset/data read: available in Setup, Reloading and World.
    /// Lends a resolved block state's side faces in model order, without collecting them or
    /// copying their texture names. Off-axis faces are omitted; a fluid lends its still texture
    /// on each side. Returns the particle sprite and cube classification, or `Ok(None)` for an
    /// unknown state without calling the visitor. A known state can have no side faces.
    /// Use [`FaceSprite::quad_on`] to draw a face on another block's box. A face and its name
    /// borrow the published resources; keep only the [`SpriteRef`] for later atlas use.
    #[access(PublishedResources)]
    #[forward_api_inputs]
    fn visit_block_sprites<'a>(
        &'a self,
        state: BlockState,
        visit: &mut dyn FnMut(Direction, FaceSprite<'a>),
    ) -> Result<Option<BlockSpriteInfo>, LevelReadError>;
    /// Side: client.
    /// Published asset/data read: available in Setup, Reloading and World.
    /// Lends the sprite named by the namespaced atlas id (`minecraft:block/stone`, `mymod:panel`).
    /// Returns `Ok(None)` for a missing sprite. The blocks atlas also holds entries published by
    /// sprite sources, even when unused by models; sprites exclusive to the items atlas are absent.
    /// Keep the [`SpriteRef`] rather than the borrowed atlas entry across callbacks.
    ///
    /// ```compile_fail
    /// # use mod_api::{ClientHost, TextureSprite};
    /// fn escape(host: &dyn ClientHost) -> TextureSprite<'static> {
    ///     host.texture_sprite("minecraft:block/stone").unwrap().unwrap()
    /// }
    /// ```
    #[access(PublishedResources)]
    fn texture_sprite(&self, name: &str) -> Result<Option<TextureSprite<'_>>, LevelReadError>;
    /// Side: client.
    /// Published asset/data read: available in Setup, Reloading and World.
    /// For a client mod: where GUI sprite `sprite` lies in the GUI atlas and its size, none for one the atlas lacks
    /// or whose entry its definition removed (a server's metrics-only atlas lacks them all). The atlas names its
    /// sprites by GUI id, the id its definition lists them under: `minecraft:container/slot` (also given as
    /// `container/slot`), a mod's `mymod:panel`; a sprite a registered sprite source made in `gui.json` is found
    /// by the id it named, and a live texture (`create_dynamic_texture`) by its own.
    #[access(PublishedResources)]
    fn gui_sprite(&self, sprite: &Identifier) -> Option<AtlasRegion>;
    /// Side: client.
    /// Published asset/data read: available in Setup, Reloading and World.
    /// The current published generation for [`SpriteRef::location`]. It changes on each
    /// successful resource load; reacquire sprites in `texture_atlas_stitched`.
    #[access(PublishedResources)]
    fn block_sprites_version(&self) -> u64;
}
