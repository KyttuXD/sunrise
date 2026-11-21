//! The `register_behaviors` entity Part hook.

trait Hooks {
    /// Register behaviors beside the species' own when its brain is built or rebuilt. They run
    /// in `Brain.tick` order and their state is never saved. Read the mob through `behaviors.mob()`.
    /// For a type with no supported brain the registrar refuses additions.
    #[hook(
        target = "sunrise_client_lib::mobs::mod_behaviors::attach",
        side = "server",
        contract = "listen",
        scope = "entity",
        key = "kind",
        dispatch = "direct",
        callable = true,
        neoforge = "none (vanilla Brain.Provider.makeBrain)"
    )]
    fn register_behaviors(#[engine] kind: &str, behaviors: &mut BehaviorRegistrar<'_>);
}
