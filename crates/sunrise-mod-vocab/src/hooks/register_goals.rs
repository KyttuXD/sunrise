//! The `register_goals` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// The mob's mod goals, added to its two selectors as `GoalSelector.addGoal`
    /// does in vanilla's `registerGoals`: fresh at every construction, so a goal's
    /// state is never saved. Branch on the mob with `event.goals.mob()`.
    #[hook(
        target = "sunrise_client_lib::mobs::part_goals::native_register_goals",
        side = "server",
        contract = "listen",
        isolate = true,
        scope = "entity",
        key = "goals",
        neoforge = "none (vanilla Mob.registerGoals)"
    )]
    fn register_goals(goals: &mut GoalRegistrar<'_>);
}
