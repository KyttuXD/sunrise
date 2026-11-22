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
    /// Replace `input` or `velocity` to change the branch's starting values,
    /// not the branch itself. Edits and cancellation are independent: there is
    /// no per-listener output validator or synthetic magnitude cap. Velocity
    /// writes follow vanilla `Entity.setDeltaMovement` (any finite vector is
    /// applied; a non-finite vector leaves the previous velocity intact without
    /// reverting valid input edits or cancellation). Input flows through
    /// vanilla `LivingEntity.travel` (`Entity.moveRelative` and
    /// `Entity.getInputVector`: vectors with squared length above `1.0` are
    /// normalized, and a non-finite input produces no acceleration without
    /// reverting valid velocity edits or cancellation).
    ///
    /// Set `cancelled` or return `Flow::Cancel` to skip the branch, its nested
    /// moves and flying vertical damping. Listeners registered with
    /// `#[listener(receive_cancelled)]` still run and may set `cancelled` back
    /// to `false`. Work before travel, such as swim steering, is not undone. If
    /// a listener panics or incurs a phase refusal, only its own edits and
    /// cancellation are discarded and later eligible listeners still run.
    /// A panic disables the failed callback for this hook for the session.
    /// The hook takes a loop context, not a `Host`.
    ///
    /// For a player, the veto takes effect at the server's travel boundary:
    /// reports it already accepted are not rewound. While the veto stands,
    /// the server corrects new movement reports to its accepted position.
    /// An edit that changes the stored velocity or the acceleration the
    /// chosen branch applies sends one correction, even if collision absorbs
    /// it, if the player remains eligible and no correction awaits
    /// acknowledgement. The server compares the inputs at `moveRelative`
    /// using the branch's own speed and yaw; inputs that have the same effect
    /// or are unused by the branch do not trigger a correction. A separate
    /// teleport in the same tick takes priority. The correction carries the
    /// live server position and velocity.
    /// The hook never runs on the client.
    /// Beyond NeoForge: there is no counterpart.
    ///
    /// Version 3: removed the per-listener output validator, its synthetic
    /// `[-1, 1]` input and `±64` velocity limits, and its cancellation latch.
    /// Edits and cancellation apply independently through vanilla's travel and
    /// velocity operations, and `#[listener(receive_cancelled)]` listeners may
    /// clear `cancelled`.
    #[hook(
        side = "server",
        contract = "cancel",
        out = "canceled",
        context = "loop",
        cancellable = true,
        version = 3,
        neoforge = "none (fork addition; see travel-and-move.md, #2308)",
        target = "sunrise_client_lib::travel_gate::native_living_travel"
    )]
    fn living_travel(
        entity: u32,
        kind: &str,
        mode: TravelMode,
        input: &mut [f64; 3],
        velocity: &mut [f64; 3],
        canceled: &mut bool,
    );
}
