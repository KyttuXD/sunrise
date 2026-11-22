//! The `register_behaviors` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// The mob's mod brain behaviors, joined into the species' brain beside
    /// vanilla's (#824): built wherever the brain is built — its first tick and
    /// every rebuild — started and stopped in `Brain.tick`'s order, fresh state
    /// each build, nothing saved. Branch on the mob with `event.behaviors.mob()`.
    #[hook(
        target = "sunrise_client_lib::mobs::mod_behaviors::native_register_behaviors",
        side = "server",
        contract = "listen",
        isolate = true,
        scope = "entity",
        key = "behaviors",
        neoforge = "none (vanilla LivingEntity.makeBrain)"
    )]
    fn register_behaviors(behaviors: &mut BehaviorRegistrar<'_>);
}
