//! The `explosion_resistance` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// On an authoritative server, the explosion resistance of the block `state` at `pos`
    /// against `explosion` (`IBlockExtension.getExplosionResistance`), asked as a ray of the
    /// blast enters the block (`ExplosionDamageCalculator.getBlockExplosionResistance`). The
    /// answer replaces only the block's own value (`BlockProperties.explosion_resistance`);
    /// the calculator takes the greater of that answer and the fluid's resistance (0 for
    /// empty fluid and 100 for water or lava). What vanilla applies after that keeps winning:
    /// a respawn anchor's water cell, a TNT minecart's rails and the blocks under them (never
    /// asked) and a dangerous wither skull's 0.8 cap. A non-finite answer is reported and
    /// ignored. A panic disables that mod's resistance callback for the session. In both cases
    /// the next part can answer; otherwise the static value is used. A ray asks once per block it enters, so
    /// keep the answer a pure function of the inputs. The level is read-only. With no part
    /// attached to the state, the cost is one table lookup. Diverges from upstream: new hook.
    #[notes = r#"## Block explosion resistance

- **`BlockPart::explosion_resistance(level, event)`** (`ExplosionResistanceEvent { pos, state, explosion }`): answer `Some(resistance)` to replace the block's own explosion resistance (NeoForge's `IBlockExtension.getExplosionResistance`) against the blast `event.explosion` describes, the same `ExplosionStart` the global `explosion_start` hears. A ray of the blast asks as it enters the block; its next steps in the same block reuse the answer, so keep it a pure function of the inputs. The answer replaces only the block's static value: the calculator takes the greater of that answer and the fluid's resistance (0 for empty fluid and 100 for water or lava), before applying the source calculator's overrides. A finite negative answer is a valid replacement. `None` leaves it to another part and then the block's value. Non-finite answers are reported and skipped each time; a panic disables that mod's resistance callback for the session. Neither prevents other mods' parts from answering. Authoritative servers only; the level is read-only. With no part attached to the state, one table lookup. Diverges from upstream: new hook."#]
    #[hook(
        receiver = "read",
        dispatch = "flat",
        side = "server",
        contract = "replace",
        out = "resistance",
        scope = "block",
        key = "state",
        version = 2,
        isolate = true,
        output_validator = "crate::world::types::validate_explosion_resistance",
        target = "sunrise_client::game::native_hooks::explosion_resistance::GameLevel<'_>::native_explosion_resistance",
        neoforge = "IBlockExtension.getExplosionResistance"
    )]
    fn explosion_resistance(
        pos: BlockPos,
        state: BlockState,
        explosion: &ExplosionStart<'_>,
        resistance: &mut Option<f32>,
    );
}
