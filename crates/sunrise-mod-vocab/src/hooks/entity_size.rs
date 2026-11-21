//! The `entity_size` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// An entity's dimensions are being resolved for `pose`: after a pose or vanilla-size change
    /// (NeoForge's `EntityEvent.Size`, vanilla's `Entity.refreshDimensions`: its pose changed to
    /// `pose`, it grew up, its `minecraft:scale` changed, or it is new), or on demand at a
    /// player's vanilla pose-fit check (`LocalPlayer.aiStep` crouching and
    /// `Player.updatePlayerPose` / `canPlayerFitWithinBlocksAndEntitiesWhen`). `size` (width,
    /// height and eye height, in blocks) starts as vanilla's new size and `old_size` is the size
    /// it had. Change `size` and the entity's box and eyes take it; later mods see earlier mods'
    /// edits. The current-pose answer holds while the entity keeps that pose and vanilla size;
    /// the next change asks again, starting from vanilla's. Changing the width or height leaves
    /// the eye height as it is: set it too. Any finite non-negative answer (including zero) is
    /// accepted with no synthetic upper cap; non-finite or negative components are logged and
    /// ignored, keeping vanilla's dimensions. `host` reads the side that asks; the entity may be
    /// new, so read only what you need of it.
    ///
    /// A mob is asked on the server at construction and when its pose or vanilla dimensions
    /// refresh. A player is asked on demand only
    /// for the candidate poses vanilla's fit checks actually test (including `FallFlying`,
    /// `Sleeping`, and `SpinAttack` when desired), reusing an unchanged current-pose answer, and
    /// for current-pose changes outside motion (initial tick, sleeping, dead, spectator, or an
    /// external scale/pose edit). Nothing is dispatched when no mod subscribes. Does not fire
    /// for: other kinds of entity, whose vanilla sizes are fixed, and a client's copies of other
    /// entities, which keep vanilla's size. Not cancellable.
    ///
    /// Version 3: removes the synthetic 32-block upper limit (reporting invalid non-finite or
    /// negative answers via warning log) and removes eager pre-motion precomputation of inactive
    /// standing/crouching/swimming candidates. Candidate dimensions are now resolved on demand at
    /// `update_local_crouching` and `update_player_pose` (including replayed ticks and
    /// `FallFlying`/`Sleeping`/`SpinAttack` desired poses) using live `minecraft:scale`.
    #[hook(
        receiver = "read",
        dispatch = "flat",
        side = "both",
        version = 3,
        contract = "collect",
        out = "size",
        host = "read",
        target = "sunrise_client::game::native_hooks::entity_size::NativeHookHost<'_>::native_entity_size",
        neoforge = "EntityEvent.Size",
        cancellable = false
    )]
    fn entity_size(entity: EntityRef, pose: mod_api::Pose, old_size: [f32; 3], size: &mut [f32; 3]);
}
