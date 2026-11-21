# Farmer's Delight's calls against existing hooks

The open rows of the converter's map of Farmer's Delight's mixins (#1027; the full 34-point table is
reservedbytes' comment on that issue), each exercised against the hook Sunrise already has. Each
row names what Farmer's Delight's injection does at its vanilla method, the hook's contract and the
test that runs it. No Farmer's Delight-specific hook is proposed. The three contract mismatches the
map found have their own issues: `can_eat` changing the answer (#1897), the attack strength at the
blow (#1898) and the dispenser's behaviour by the facing block (#1899).

Prefix **API** = `crates/sunrise-mod-vocab/src/`; **Game** = `crates/sunrise-client/src/game/`.

| Farmer's Delight's injection | Hook and contract | Status | Evidence |
|---|---|---|---|
| `CampfireBlock#isSmokeSource`, `@Inject` at HEAD, cancellable: answers for its own blocks. | `BlockPart::smoke_source` (API `hooks/base.rs:3327`): `replace`, scoped to the block under the campfire, asked on an authoritative server as the campfire is placed or that block changes. | **Works** | Game `modding/native_smoke_source_tests.rs:44`: a Part's yes makes a campfire over stone a signal fire, and a Part's no over a hay bale replaces vanilla's yes, through the shape update when the block under it changes. The map's location note (only the hook's rustdoc names the method) is settled: `sunrise-world` `level/survive.rs:504` is `CampfireBlock.isSmokeSource` and asks the hook. |
| `LivingEntity#hurtServer`, `@ModifyVariable` at HEAD: changes the damage amount. | `living_incoming_damage` (API `hooks/base.rs:906`): `cancel`, with `container.set_new_damage`; fires past `hurtServer`'s guards. | **Works** | Game `modding/native_damage_tests.rs:119`: a listener sets 4 on a 10-point hit and the pig keeps 6 health, so the changed amount is the one dealt. The map's "contract mismatch" read the hook's name (`cancel`), not its mutable container. Farmer's Delight changes the amount before the guards, and the hook runs after them. In vanilla 26.3's `hurtServer`, the guards (`isInvulnerableTo`, `isDeadOrDying`, fire damage under Fire Resistance) return before the method first reads the amount, so the dealt damage is the same. |
| `LivingEntity#knockback` (both overloads), `@ModifyVariable` at HEAD, ordinal 0: changes the strength. | `living_knockback` (API `hooks/living_knockback.rs`): one cancellable event with mutable `strength`, `x` and `z` before knockback resistance. | **One push rule** | `one_event_per_living_knockback_operation` in `modding/native_knockback_tests.rs` exercises both callers. |
| `MinecraftServer#onGameRuleChanged`, `@Inject` at HEAD: is told a rule changed. | `game_rule_changed` (API `hooks/game_rule_changed.rs:15`): `listen`, with the rule, the new value and the actor. | **Works** | Game `modding/native_level_world.rs:523`: setting `keep_inventory` tells a listener the rule, that no player caused it, and the whole rule set with the new value. The map found no candidate, because it tagged the method to two `Hub` setters rather than to the hook. |

## Cake eligibility

`can_eat` also supplies cake and candle cake's `Player.canEat(false)` answer.
For cake and candle cake, the event's `stack` is `None` regardless of what the
hand holds, with `can_always_eat = false`. Answer `Some(false)` to refuse a bite even when
hungry, or `Some(true)` to allow a full player to eat. Leaving the answer unset
keeps vanilla hunger and creative-mode behavior.

## Gaps

Port the intended strength adjustment to one `living_knockback` listener. As with
NeoForge's `LivingKnockBackEvent`, the API does not expose how vanilla entered the operation,
so a Java mixin's two separate edits cannot be reproduced by branching on the overload.
#2767 tracks cross-target API admission. For hearing refused
damage attempts without changing the amount, use the existing
[invulnerability observation](../../modding/src/how-to/entities/refused-damage-attempts.md), with its explicit vanilla exclusions (#2752).
`living_incoming_damage` already allows amount changes before the cooldown comparison; no new
amount-bearing entry event is needed for that use case.
