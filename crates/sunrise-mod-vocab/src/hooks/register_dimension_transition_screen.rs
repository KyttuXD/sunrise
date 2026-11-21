//! Client dimension-loading screen registration.

pub trait Hooks {
    /// Register loading-screen factories once at client startup (NeoForge's
    /// `RegisterDimensionTransitionScreenEvent`). Call `event.register(screen)` for each trip.
    /// Conditional pairs take priority over incoming, then outgoing. The first valid
    /// factory for a key wins; `event.register` returns `false` for a duplicate or invalid key
    /// and `true` to stage an accepted factory. Dimension IDs resolve to keys only when
    /// successful registrations are published.
    /// Unmatched travel, login and death use vanilla for a new screen.
    /// The client keeps whichever presentation is already open on an in-place load update.
    #[notes = r#"## Dimension transition screens

Pass `DimensionTransitionScreen::new(id, transition, factory)` to `event.register(screen)`.
Use `DimensionTransition::Incoming(to)`, `Outgoing(from)` or `Conditional { to, from }`
with namespaced dimension IDs. A screen ID is for validation and diagnostics, not draw dispatch;
screen IDs in the `minecraft` namespace are refused. `register` reports `false` for invalid or
occupied keys, including a key already staged by this listener. Only published registrations
resolve dimension IDs to keys; a failed listener does not make new dimension keys available.

On a matching trip, the client calls the selected factory once as the registering mod and
keeps its presentation until the loading screen closes. An existing loading screen (including
vanilla's) keeps its presentation if another level load starts while it is open. Login and
death respawn do not select factories when they open a new loading screen. Return a boxed
`DimensionTransitionPresentation` (a draw closure or an object); each draw receives the current
reason and optional smoothed integrated-server progress. The existing tracker still governs
readiness, timeout and closing."#]
    #[hook(
        receiver = "associated",
        dispatch = "flat",
        side = "client",
        contract = "collect",
        out = "screens",
        registration_append = "custom_with_owner",
        context = "registration",
        neoforge = "RegisterDimensionTransitionScreenEvent",
        version = 3
    )]
    fn register_dimension_transition_screen(
        screens: &mut Vec<mod_api::__private::RegisteredTransitionScreen>,
    );
}
