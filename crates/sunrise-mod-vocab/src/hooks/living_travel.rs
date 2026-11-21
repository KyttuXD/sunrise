//! The `living_travel` hooks and helpers: declarations the mod API generator
//! reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// Before the server's `Player.travel` branch (after swim steering) or a
    /// mob's `LivingEntity.travel` branch. `entity`, `kind` and `mode` identify
    /// the body and the branch it is about to run, including species overrides.
    /// A body that doesn't travel, such as a shulker, fires no hook. Neither does
    /// a replayed player tick. A branch that calls `entity_move` can still meet
    /// a move veto there.
    ///
    /// Use `motion.set_input` or `motion.set_velocity` to edit the branch's
    /// input or the body's delta movement, not the chosen branch. Both accept
    /// finite vectors without a magnitude limit. A refused edit returns
    /// `false` and is logged with the mod and field; it does not undo the other
    /// edit or affect cancellation. The branch still normalizes input and runs
    /// its usual motion and collision code.
    ///
    /// Set `cancelled` or return `Flow::Cancel` to skip the branch, its nested
    /// moves and flying vertical damping. Neither travel already done (such as
    /// swim steering) nor a successful velocity write is undone. A panic logs
    /// and shows an error toast naming the mod, disables only that callback for
    /// the session, and does not undo completed writes or stop later listeners.
    /// Only a completed listener can cancel. The hook takes a loop context,
    /// not a `Host`.
    ///
    /// For a player, the veto takes effect at the server's travel boundary:
    /// reports it already accepted are not rewound. While the veto stands,
    /// the server corrects new movement reports to its accepted position.
    /// If travel runs, an edit to input or velocity sends one correction,
    /// even if collision absorbs it, if the player remains eligible and no
    /// correction awaits acknowledgement. A separate teleport in the same
    /// tick takes priority. The correction carries the live server position
    /// and velocity. The server compares the vectors bit for bit, except that
    /// inputs within vanilla's movement dead zone are equivalent.
    /// The hook never runs on the client.
    /// Beyond NeoForge: there is no counterpart.
    #[hook(
        side = "server",
        contract = "cancel",
        out = "canceled",
        context = "loop",
        cancellable = true,
        isolate = "live",
        version = 2,
        neoforge = "none (fork addition; see travel-and-move.md, #2308)",
        target = "sunrise_client_lib::travel_gate::native_living_travel"
    )]
    fn living_travel(
        entity: u32,
        kind: &str,
        mode: TravelMode,
        motion: TravelMotion<'_>,
        canceled: &mut bool,
    );
}
