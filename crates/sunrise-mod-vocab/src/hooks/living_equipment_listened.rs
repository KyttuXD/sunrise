//! The `living_equipment_listened` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0158

pub trait Hooks {
    /// Whether a mod listens to the mobs' and players'
    /// `living_equipment_change`: a mod that hooks it hooks this one too
    /// and sets `listened` (HEAD, `*listened = true`). Without it the game does
    /// not dispatch `living_equipment_change` for mobs and connected players
    /// each tick (armor stands still fire). A mob that ticks, or a player whose
    /// body ticks, once it is set counts from empty. Asked once a tick before
    /// the mobs' equipment update pass and once before the players' connection
    /// tick pass. Server-side.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_living_equipment_listened",
        dispatch = "flat",
        side = "server",
        contract = "collect",
        out = "listened",
        cancellable = false,
        neoforge = "none (Sunrise-only; NeoForge has no counterpart); an opt-in gate for a hook"
    )]
    fn living_equipment_listened(listened: &mut bool);
}
