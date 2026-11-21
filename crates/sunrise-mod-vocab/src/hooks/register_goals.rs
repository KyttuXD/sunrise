//! The `register_goals` entity Part hook.

trait Hooks {
    /// The mob's mod goals, added to its two selectors just after vanilla's `Mob.registerGoals`.
    /// Goals are fresh at every construction, so their state is never saved. Read the mob through
    /// `goals.mob()`.
    #[hook(
        target = "sunrise_client_lib::mobs::part_goals::attach",
        side = "server",
        contract = "listen",
        scope = "entity",
        key = "kind",
        dispatch = "direct",
        callable = true,
        neoforge = "none (vanilla Mob.registerGoals)"
    )]
    fn register_goals(#[engine] kind: &str, goals: &mut GoalRegistrar<'_>);
}
