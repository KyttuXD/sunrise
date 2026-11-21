# Ticking a registered mob effect

Attach server-side behaviour to a content-declared effect with a `MobEffectPart`:

```rust,ignore
let effect = r.effect_key("meals:comfort");
if let Err(error) = r.mob_effect_part(&effect, Comfort) {
    context.refuse(&format!("Cannot attach Comfort: {error:?}"));
    return;
}
```

Implement the Part with `#[mod_api::sunrise_part]`. Its two methods are:

- `should_apply_effect_tick_this_tick` receives the resolved `effect` key, remaining
  duration (`tick_count`, or the holder's tick count for an infinite effect), and
  amplifier. Set `event.apply = true` to run the effect now; it starts false.
- `apply_effect_tick` runs only when this Part's own cadence method says to apply. It receives the
  holder's `LivingRef`, dimension, health, maximum health and optional `PlayerFood`.
  Queue `heal`, `hurt`, `eat` or `add_exhaustion` changes through the event. These
  apply after every Part returns, in Part order. Clear `event.keep` to end the effect
  immediately, before its duration counts down, and skip later Parts.

These are version 3 methods in the `effect` scope, keyed by `effect`; there are no
corresponding global `Hooks` methods. Each Part has its own `apply`, starting false,
so one package's cadence never drives another package's action. The declaring package's
Parts run first within that effect, even with `order = "before"` or `"none"`. Other Parts
run by mod dispatch order, then registration order. All share the same holder snapshots.
This owner-first rule is specific to effects; it does not change package dispatch or
the order of block entity Parts.

A Part may attach to any registered mod effect, regardless of which package declared it. If you
integrate with an optional dependency, declare it as optional in `[dependencies]` and use
`context.has_optional_dependency("package_id")` in `Hooks::register`. Put both the key request and
Part attachment inside the check. It uses the admitted packages and effective dependency rows for
this installation. Use the package ID in that check, not the effect's namespace. An unguarded request
for a missing effect always refuses startup with the requesting mod and effect ID, even if the
manifest lists an optional dependency.

Invalid keys refuse with `MobEffectPartError::InvalidKey`; built-in vanilla effects refuse
immediately with `MobEffectPartError::VanillaEffect`; attaching the same concrete
Part type twice returns `Duplicate`. A Part attached to several
effects distinguishes them with `event.effect`. `on_effect_added` stays global;
`mob_effect_added`, the NeoForge event, also stays global.

## Reading the holder

The ticking holder may be borrowed out of the read-only Host, so do not depend on
resolving its live entity storage through the handle during this callback. Use the
provided health and food snapshots and `event.has_effect(key)` instead. The
`active_effects: &[EffectKey]` snapshot reflects the holder's active list at this
point in the pass, including changes earlier in that pass. It is borrowed only
for the call: copy a key if you need to keep it.

For example, an effect that heals every 80 ticks can set `apply` when
`event.tick_count % 80 == 0`, then skip healing when
`event.has_effect(regeneration_key)` or when a player's saturation is positive.
A mob has `event.food == None` and can still heal.

The level remains readable. Obtain it with `host.level(event.dimension)` and use
`level.game_rule(host, "minecraft:natural_health_regeneration")` to read vanilla's
natural-regeneration rule. This read works while a mob's entity storage is
borrowed for its tick.

`event.add_exhaustion(amount)` follows vanilla's `FoodData.addExhaustion` on players:
it adds the amount and caps the result at 40, without a lower clamp. Negative
amounts therefore subtract exhaustion. To remove at most 4, read the current food
snapshot and add `-food.exhaustion.min(4.0)` only when exhaustion is positive. Food
changes have no effect on a mob without food data.

A `side = "server"` package can attach a Part because both methods are server-only.
