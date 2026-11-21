//! The `level_collision` area: block shapes, collision and rays through them. Host method declarations; access defaults and game forwarders are generated.
#![allow(unused_variables)]
use crate::*;
/// The handles' reads of the `level_collision` area, implemented by the game and reached through [`Backend`].
// hidden: a handle's backend, reached through `Backend`; mods call the handle, never this trait
#[doc(hidden)]
#[sunrise_mod_api_macros::host_area]
pub trait LevelCollisionBackend {
    /// Side: both.
    /// `BlockGetter.getBlockFloorHeight` with the normal collision shapes and an empty
    /// entity context. The result is relative to the block: negative infinity means no floor.
    /// None for a stale level, an unloaded column or a position outside build height.
    /// Reads parked server levels and the current client level without loading chunks.
    #[access(WorldOnly)]
    fn level_block_floor_height(&self, level: LevelRef, pos: [i32; 3]) -> Option<f64>;
    /// Side: server.
    /// The supplied state's block-local outline or collision boxes at `pos`,
    /// with an empty context (`BlockState.getShape` / `getCollisionShape`).
    /// The state need not be the block stored there; level-dependent shapes
    /// still read the real level. Pass `block_state(host, pos)` to query the
    /// state at that position. Interaction, visual, occlusion and support shapes
    /// are not exposed. Returns None on a client, for an unknown state, a stale
    /// handle or a closed level. Collision errors return no boxes.
    #[access(WorldOnly)]
    fn level_block_shape(
        &self,
        level: LevelRef,
        state: BlockState,
        pos: [i32; 3],
        kind: BlockShapeKind,
    ) -> Option<Vec<[f64; 6]>>;
    /// Side: both.
    /// Clip an empty-context ray against collider or outline shapes, without fluids.
    /// None on failed reads, dynamic shapes, invalid rays or traversal limits, and for a ray
    /// that enters a column the level doesn't hold. On a client, the level it shows or a
    /// viewed one (`host.level`), each through its own world.
    #[access(WorldOnly)]
    fn level_clip(
        &self,
        level: LevelRef,
        from: [f64; 3],
        to: [f64; 3],
        blocks: ClipBlock,
        fluids: ClipFluid,
    ) -> Option<ClipHit>;
    /// Side: both.
    /// `BlockState.isFaceSturdy` in an open server level or the current client level,
    /// including shapes published by local block entities. None for stale or unavailable
    /// levels, unloaded or out-of-height blocks, or a shape query error. Never loads chunks.
    #[access(WorldOnly)]
    fn level_block_face_sturdy(
        &self,
        level: LevelRef,
        pos: [i32; 3],
        face: BlockFace,
        support: SupportType,
    ) -> Option<bool>;
    /// Side: both.
    /// Visit intersecting block collision shapes in `CollisionGetter.getBlockCollisions`
    /// order, with an empty collision context, without entities or the world border.
    /// `aabb` is `[min_x, min_y, min_z, max_x, max_y, max_z]`. Each borrowed shape is
    /// in world coordinates and is not clipped to `aabb`. Clone it only if it must outlive
    /// the callback.
    ///
    /// Reads the named world, including a parked server level or a viewed client level.
    /// Skips missing chunks without loading them. `Some(Continue(()))` means the scan
    /// finished, including when empty; `Some(Break(()))` stops at the visitor's first
    /// break with no further reads. `None` means invalid bounds (including ones outside
    /// vanilla `Cursor3D`'s positive signed-int domain), unavailable world access (such as
    /// a stale handle), or a query error. On a query error, earlier visits stand.
    #[access(WorldOnly)]
    #[decline_unavailable]
    fn level_visit_block_collisions(
        &self,
        level: LevelRef,
        aabb: [f64; 6],
        visit: &mut dyn FnMut(&VoxelShape) -> std::ops::ControlFlow<()>,
    ) -> Option<std::ops::ControlFlow<()>>;
    /// Side: server.
    /// Whether nothing solid lies on the segment `from`..`to`, as
    /// `LivingEntity.hasLineOfSight` clips between two eyes (`ClipContext.Block.COLLIDER`,
    /// `ClipContext.Fluid.NONE`): `clip`'s miss. Fluids, entities and the border don't block it.
    /// None as for `clip`.
    #[access(WorldOnly)]
    fn level_line_of_sight(&self, level: LevelRef, from: [f64; 3], to: [f64; 3]) -> Option<bool>;
    /// Side: both.
    /// Whether the block collision walk finds a shape for `aabb` (`[min_x, min_y, min_z,
    /// max_x, max_y, max_z]`): `CollisionGetter.noBlockCollision(null, aabb)`, negated.
    /// Like [`LevelRef::visit_block_collisions`], it skips missing chunks, ignores entities
    /// and the world border, and returns `None` for unavailable levels, invalid bounds or
    /// query errors. Stops at the first hit without lending or collecting geometry.
    #[access(WorldOnly)]
    #[decline_unavailable]
    fn level_has_collisions(&self, level: LevelRef, aabb: [f64; 6]) -> Option<bool>;
}
