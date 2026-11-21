//! Code-defined entity renderers, as a client mod registers them in `register_entity_renderers`
//! (NeoForge's `EntityRenderersEvent.RegisterRenderers` and `EntityRenderer`).
//!
//! A renderer draws every entity of one entity type, named by its id (`mymod:bolt`). The game
//! keeps the registrations from start and resolves each type's renderer on every resource load, so
//! a frame hands an entity to the renderer of its type's slot and to no other mod.
use crate::BlockEntityDrawContext;
use std::sync::Arc;

/// One entity's frame, as `EntityRenderer.extractRenderState` fills an `EntityRenderState`:
/// what a renderer reads to place and turn its model. Angles are in degrees, interpolated to the
/// frame's partial tick, except for primed TNT and experience orbs, which supply their current
/// rotation.
// no wire: this frame is passed directly to native renderers.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub struct EntityRenderState {
    /// The entity's network id, as mod messages name it.
    pub id: u32,
    /// The frame's partial tick, 0 to 1.
    pub partial: f32,
    /// `ageInTicks`: ticks lived plus the partial tick, for motion of its own.
    pub age: f32,
    /// `Entity.getYRot` at the frame: the way it faces (0 is south, 90 west).
    pub yaw: f32,
    /// `Entity.getXRot` at the frame: its pitch, positive looking down.
    pub pitch: f32,
    /// A living entity's own frame (`LivingEntityRenderState`); none for any other entity.
    pub living: Option<LivingRenderState>,
    /// The remaining primed-TNT fuse, in ticks, as vanilla draws it (`fuse - partial + 1`);
    /// `Some` for `minecraft:tnt`, none for any other entity.
    pub tnt_fuse: Option<f32>,
    /// An experience orb's value (`ExperienceOrb.DATA_VALUE`); some for an orb, none for
    /// every other family.
    pub orb_value: Option<i32>,
    /// A painting's own frame ([`PaintingRenderState`]); none for any other entity.
    pub painting: Option<PaintingRenderState>,
    /// An item frame or glow item frame's own frame ([`ItemFrameRenderState`]); none for any
    /// other entity.
    pub item_frame: Option<ItemFrameRenderState>,
    /// A general entity's last published immutable generation of synced fields, even if empty.
    /// An over-budget generation leaves the previous one in place. The general entity does not
    /// draw until its first generation is published. `None` for other entity families.
    pub attachments: Option<crate::EntityRenderAttachments>,
}

/// What a living entity's frame adds to [`EntityRenderState`], as
/// `LivingEntityRenderer.extractRenderState` fills a `LivingEntityRenderState`: what a model
/// reads to pose and animate a body. Angles are in degrees, interpolated to the frame's partial
/// tick. Plain values, so a frame allocates nothing.
// no wire: lives in the native entity render state.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LivingRenderState {
    /// `bodyRot`: the way the body faces (0 is south, 90 west).
    pub body_rot: f32,
    /// `yRot`: the head's turn from the body, -180 to 180 (its sign flipped when
    /// `is_upside_down`).
    pub y_rot: f32,
    /// `xRot`: the head's pitch, positive looking down (its sign flipped when `is_upside_down`).
    pub x_rot: f32,
    /// `deathTime`: ticks since it died plus the partial tick, 0 while alive (vanilla tips the
    /// body over the first 20).
    pub death_time: f32,
    /// `walkAnimationPos`: how far the legs have swung, for a walk cycle (a vanilla quadruped's
    /// legs swing `cos(pos * 0.6662) * 1.4 * speed` radians); 0 riding or dead.
    pub walk_animation_pos: f32,
    /// `walkAnimationSpeed`: how fast it walks, 0 standing to 1; 0 riding or dead.
    pub walk_animation_speed: f32,
    /// `scale`: the entity's `minecraft:scale` (`LivingEntity.getScale`), by which vanilla scales
    /// the whole model.
    pub scale: f32,
    /// `ageScale`: 0.5 for a baby, else 1 (`LivingEntity.getAgeScale`).
    pub age_scale: f32,
    /// `isUpsideDown`: named Dinnerbone or Grumm, drawn head down.
    pub is_upside_down: bool,
    /// `isBaby`.
    pub is_baby: bool,
    /// `isInWater`.
    pub is_in_water: bool,
    /// `hasRedOverlay`: hurt or dying, drawn tinted red.
    pub has_red_overlay: bool,
    /// `pose`: its stance (`Entity.getPose`).
    pub pose: crate::Pose,
    /// `bedOrientation`: the way the bed it sleeps in faces, while it sleeps in one.
    pub bed_orientation: Option<crate::Direction>,
}

/// What a painting's frame adds to [`EntityRenderState`]: the way it faces out from the
/// wall, and its size in blocks (`Painting.direction`, `PaintingVariant.width` /
/// `height` as the entity already holds them). The variant id is not here: the state
/// is only plain numbers, so a renderer that needs the variant reads it in a hook and
/// keeps what it read.
// no wire: lives in the native entity render state.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaintingRenderState {
    /// The way the painting faces out from the wall (`Direction.get2DDataValue`: south,
    /// west, north, east).
    pub facing: crate::Direction,
    /// Width in blocks.
    pub width: i32,
    /// Height in blocks.
    pub height: i32,
}

/// What an item frame's frame adds to [`EntityRenderState`]: the way it faces, whether it is a
/// glow frame, whether the frame model is hidden, its item rotation, and whether it holds an
/// item or a filled map. The stack itself is not here: the state is only plain numbers, so a
/// renderer that needs the item reads it in a hook and keeps what it read.
// no wire: lives in the native entity render state.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemFrameRenderState {
    /// The face it hangs on (`Direction.get3DDataValue`: down, up, north, south, west, east).
    pub facing: crate::Direction,
    /// A glow item frame (`minecraft:glow_item_frame`).
    pub glow: bool,
    /// The frame model is hidden (`Entity.isInvisible`); the item may still be drawn.
    pub invisible: bool,
    /// `ItemRotation`, in 45-degree steps (`-7..=7`).
    pub rotation: i8,
    /// Whether the frame holds an item.
    pub has_item: bool,
    /// Whether that item is a filled map (vanilla draws the map on the face).
    pub has_map: bool,
}

/// A mod's entity renderer: NeoForge's `EntityRenderer`, held by the game for every entity of the
/// type it is registered for. Entities of a type share one renderer, so what differs between them
/// comes from [`EntityRenderState`].
///
/// [`submit`](Self::submit) has no `Host`: it draws from what the state and `draw` give. A
/// renderer that needs more reads it in a hook and keeps what it read. A general entity type's
/// renderer (`declare_general_entity`) reads its synced fields from the snapshot in
/// [`EntityRenderState::attachments`].
pub trait EntityRenderer: Send + Sync + 'static {
    /// `EntityRenderer.submit`: draw `entity` with `draw`'s helpers (`quad`, `cube`, `block`,
    /// `item`, `model_part`) under its pose stack, which starts at the entity's position, not
    /// turned: turn it by `entity.yaw` and `entity.pitch` as the model needs. `draw` also has
    /// the partial tick, the game time and the entity's light. Asked once a frame for each entity
    /// of the type in view; a general entity is drawn only after its first synced attachment
    /// generation is published. What this method draws replaces the type's own drawing. A frame
    /// is drawn afresh: `draw.keep()` does nothing here. `Ok(())` keeps what it drew. An `Err` is
    /// logged for each failure and discards just this submission; later draws still call the
    /// renderer. A panic discards this submission and disables this callback for the session,
    /// with an error log and toast naming its mod. In either case the entity draws as it would
    /// without a registered renderer on that draw.
    fn submit(
        &self,
        entity: &EntityRenderState,
        draw: &mut dyn BlockEntityDrawContext,
    ) -> Result<(), String>;
}

/// The entity renderer registry `register_entity_renderers` hands out: NeoForge's
/// `EntityRenderersEvent.RegisterRenderers`.
pub trait EntityRenderers {
    /// `registerEntityRenderer(type, provider)`: `renderer` draws every entity of the type
    /// `entity_type` (`mymod:bolt`). A later registration for a type replaces the earlier one,
    /// whichever mod made it. A type the game does not know is logged with its id when resources
    /// load, and nothing is drawn by it.
    fn register(&mut self, entity_type: String, renderer: Arc<dyn EntityRenderer>);
}
