//! The `entity_size` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// Resolve an entity's dimensions for `pose`. `size` (width, height, eye height,
    /// in blocks) starts as vanilla's result and `old_size` is the size it had.
    /// Later listeners see earlier edits. Changing width or height leaves the eye height
    /// unchanged unless you edit it too. Zero-size answers are valid. The engine currently
    /// reports and refuses a proposal with a non-finite, negative or greater-than-32-block component.
    /// `host` reads the side that asks. A panic disables only this mod's callback. Not cancellable.
    ///
    /// Mobs on the server resolve a new current size before or after their tick when
    /// their pose or vanilla size changes, including their first tick. A local or guest
    /// player resolves the current box on a pose or scale change and queries a candidate
    /// only when the movement code reaches its collision test. The selected candidate's
    /// answer supplies the new box. Use `pose`, not the entity's current pose, when
    /// deciding what to return: a collision candidate need not become the current pose.
    /// Other entity families and remote client copies do not fire this hook.
    #[hook(
        receiver = "read",
        dispatch = "flat",
        side = "both",
        version = 3,
        contract = "collect",
        out = "size",
        isolate = true,
        collect_validator = "crate::entity::types::validate_size",
        host = "read",
        neoforge = "EntityEvent.Size",
        cancellable = false
    )]
    fn entity_size(entity: EntityRef, pose: mod_api::Pose, old_size: [f32; 3], size: &mut [f32; 3]);
}
