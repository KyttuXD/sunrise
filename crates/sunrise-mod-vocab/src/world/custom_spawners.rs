//! Server-level custom spawners and their level-construction registration scope.

use crate::{DimensionKey, ServerHost};

/// A built-in custom spawner in the Overworld's starting list.
#[derive(sunrise_mod_api_macros::Wire, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SpawnerKind {
    /// `PhantomSpawner`: phantoms for players who have not slept.
    Phantom,
    /// `PatrolSpawner`: pillager patrols.
    Patrol,
    /// `CatSpawner`: cats in villages and swamp huts.
    Cat,
    /// `VillageSiege`: zombie sieges on a village at night.
    VillageSiege,
    /// `WanderingTraderSpawner`: the wandering trader and its llamas.
    WanderingTrader,
}

// no wire: a native spawner owns code and mutable state for one live server level.
/// A custom spawner owned by one server level, like vanilla's `CustomSpawner`.
pub trait CustomSpawner: Send + 'static {
    /// Run once on each eligible level tick, while `spawn_mobs` is enabled.
    /// `spawn_enemies` is true when `spawn_monsters` is enabled and the difficulty
    /// is not Peaceful. The spawner decides whether to act; updates to `self`
    /// carry over to its next tick. Use `host` to act on `dimension`.
    /// A panic disables this callback; a phase refusal is reported without disabling it.
    fn tick(&mut self, host: &mut dyn ServerHost, dimension: DimensionKey, spawn_enemies: bool);
}

// no wire: this registration scope borrows the server level's in-process spawner list.
/// Edit a server level's custom spawners during `modify_custom_spawners`.
pub trait CustomSpawners {
    /// Append a new owning spawner instance for this level. It is dropped when the level closes.
    fn add_custom_spawner(&mut self, spawner: Box<dyn CustomSpawner>);

    /// Whether the level still has this built-in spawner.
    fn contains(&self, kind: SpawnerKind) -> bool;

    /// Remove a built-in spawner, returning whether it was present.
    fn remove(&mut self, kind: SpawnerKind) -> bool;
}
