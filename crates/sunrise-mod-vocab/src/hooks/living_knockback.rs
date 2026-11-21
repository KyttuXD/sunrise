//! The `living_knockback` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// NeoForge's `LivingKnockBackEvent`: `LivingEntity.knockback` is about to push entity
    /// `entity` of type `kind` (`minecraft:zombie`, `minecraft:player`) `strength` away from the
    /// direction (`x`, `z`), on the raw strength, before the entity's knockback resistance takes
    /// its share and its motion changes. Vanilla's direction points at the source of a hit (the
    /// attacker's side), and the entity is pushed the other way. `source` is the push's
    /// `DamageSource`, named as `native_living_incoming_damage` names a hit's. `attacker` is the
    /// attacking entity's network id, falling back to the victim's last hurt-by mob, or 0 when
    /// unknown; `attacker_main` and `attacker_off` lend its captured hand stacks (a player's
    /// selected and off-hand stacks, a mob's main hand), when available. Both vanilla entry
    /// forms reach the same operation, which fires this hook once per push. Edit `strength`,
    /// `x` or `z`, or cancel to leave the entity's motion unchanged. A nonpositive `strength`
    /// gives no push, and a direction too short to tell draws a random one after the event, as
    /// in vanilla. A non-finite output is reported and the original value is kept. The hook
    /// fires for the players, mobs and armor stands the game
    /// knocks back: a hit's own push, a sprinting or Knockback hit, a sweep, a spear, a
    /// shield's push on its holder, a goat's ram and a mob's melee push. It does not fire for a
    /// sulfur cube with a block that a hit with a causing entity knocks back
    /// (`SulfurCube.knockback` does not call `super` there), for the pushes vanilla doesn't make
    /// with `LivingEntity.knockback` (the ender dragon's wings, Punch arrows, explosions, wind
    /// charges, a mace smash's area or a hoglin's throw), nor when a living override skips the
    /// base operation (a sitting ender dragon or a creaking that cannot move). The level is the
    /// hit's: its blocks, random and own attachments.
    /// `kill_credit` is the pushed living entity's kill credit at this pre-motion moment:
    /// a remembered player takes precedence over its last hurting mob, even when that player
    /// cannot resolve (then 0, without falling back). `kill_credit_main` is that entity's
    /// current main-hand stack, or none. This is not the immediate damage-source attacker.
    /// Accepted-hit memories, including a tame wolf's owner, are committed before this read.
    /// Use [`ItemStackView`] to inspect item, count and damage without copying components. A
    /// failed [`ItemStackView::project`] affects only that read, not the push or its dispatch.
    /// The source and credit fields are Sunrise additions; on NeoForge mods read the relevant
    /// entities and their equipment themselves. A listener that panics is disabled for this
    /// hook for the session and its unfinished edits are discarded; other listeners still run.
    #[notes = "Version 3 fires once per knockback operation without an overload discriminator, and lends the attacker and credit stacks as read-only views."]
    #[hook(
        dispatch = "api_carrier",
        side = "server",
        version = 3,
        cancellable = true,
        isolate_listener = true,
        contract = "cancel",
        out = "canceled",
        host = "level",
        neoforge = "LivingKnockBackEvent",
        target = "sunrise_client::game::native_hooks::base::HitLevel<'_>::native_living_knockback"
    )]
    fn living_knockback(
        entity: i32,
        kind: &str,
        source: &HurtSource<'_>,
        attacker: i32,
        attacker_main: Option<&ItemStackView<'_>>,
        attacker_off: Option<&ItemStackView<'_>>,
        kill_credit: i32,
        kill_credit_main: Option<&ItemStackView<'_>>,
        strength: &mut f64,
        x: &mut f64,
        z: &mut f64,
        canceled: &mut bool,
    );
}
