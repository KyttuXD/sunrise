//! The `pickup_xp` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0212

use crate::ExperienceOrbRef;

pub trait Hooks {
    /// Before `player` takes the live `orb` in `dimension` (`ExperienceOrb.playerTouch`,
    /// NeoForge's `PlayerXpEvent.PickupXp`). Fires only when `takeXpDelay` is 0.
    /// Use `orb` to read or edit its value and count: later listeners see the same
    /// orb, including edits made by earlier listeners. Canceling skips the pickup
    /// without starting the delay or undoing a listener's writes.
    ///
    /// A pickup sets `takeXpDelay` to 2, takes the orb, runs Mending with its signed
    /// value, awards only positive remaining experience, then decrements the count
    /// and discards the orb if it reaches 0. `ExperienceOrbRef::set_count` refuses
    /// 0 and values above `i32::MAX` (`ExtraCodecs.POSITIVE_INT`). To remove an orb
    /// without taking it, call `orb.entity().discard(host)` and cancel the pickup.
    /// A surviving orb saves its value as a signed short, as vanilla does. Server-side.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_pickup_xp",
        dispatch = "flat",
        side = "server",
        contract = "cancel",
        out = "canceled",
        neoforge = "PlayerXpEvent.PickupXp",
        cancellable = true,
        version = 2
    )]
    fn pickup_xp(player: u128, dimension: &str, orb: ExperienceOrbRef, canceled: &mut bool);
}
