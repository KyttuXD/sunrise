#!/usr/bin/env python3
"""Remove the duplicate thunder veto from the p3-51 setter baseline."""

from pathlib import Path


root = Path(__file__).resolve().parents[1]
game = root / "crates/sunrise-client/src/game"
vocab = root / "crates/sunrise-mod-vocab/src"
rewrites: dict[Path, str] = {}


def replace(path: Path, old: str, new: str) -> None:
    source = rewrites.get(path)
    if source is None:
        source = path.read_text(encoding="utf-8")
    if source.count(old) != 1:
        raise SystemExit(f"{path.relative_to(root)}: expected one match for {old!r}")
    rewrites[path] = source.replace(old, new, 1)


thunder_hook = vocab / "hooks/thunder_change.rs"
replace(
    thunder_hook,
    '''//! The `thunder_change` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0335

pub trait Hooks {
    /// The server changed its thunder state through `/weather`. Set `cancel`
    /// to keep the previous thunder state and timer. Server-side.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_thunder_change",
        dispatch = "flat",
        side = "server",
        contract = "cancel",
        out = "cancel",
        neoforge = "none (NeoForge has no such event)",
        cancellable = true
    )]
    fn thunder_change(dimension: &str, thundering: bool, cancel: &mut bool);
}
''',
    "",
)
if rewrites.pop(thunder_hook):
    raise SystemExit(f"{thunder_hook.relative_to(root)}: unexpected content outside the declaration")

base = game / "native_hooks/base.rs"
replace(
    base,
    '''    /// The server changed its thunder state through `/weather`. Set `cancel`
    /// to keep the previous thunder state and timer. Server-side.
    pub(in crate::game) fn native_thunder_change(
        &mut self,
        dimension: &str,
        thundering: bool,
        cancel: &mut bool,
    ) {
        crate::game::level_control_hooks::LevelControl::Weather
            .hear(|| self.fire_thunder_change(dimension, thundering, cancel));
    }

''',
    "",
)
weather_doc = '''    /// The server changed its weather parameters. `raining` and `thundering`
    /// are the requested final states. Set `cancel` to keep the previous
    /// weather parameters. Server-side.
'''
weather_doc_new = '''    /// Before the server writes weather parameters, `raining` and `thundering`
    /// are the requested final states. Cancel to skip the entire write: the
    /// clear, rain and thunder timers and both flags. Server-side.
'''
replace(base, weather_doc, weather_doc_new)
weather_hook = vocab / "hooks/weather_change.rs"
replace(weather_hook, weather_doc, weather_doc_new)
replace(weather_hook, "        cancellable = true\n", "        cancellable = true,\n        version = 2\n")

weather = game / "modding/native_level_weather.rs"
replace(
    weather,
    "//! `/weather`: `ServerLevel.setWeatherParameters` with the weather and thunder hooks, and\n",
    "//! `/weather`: `MinecraftServer.setWeatherParameters` with the weather hook, and\n",
)
replace(
    weather,
    '''    /// `ServerLevel.setWeatherParameters(clear, time, raining, thundering)` in the level in
    /// place, as `/weather` and mods set it: `weather_change` may cancel all of it and
    /// `thunder_change` the thunder half.
''',
    '''    /// Writes the whole request as in `MinecraftServer.setWeatherParameters`, unless
    /// `weather_change` vetoes it. Both rain and thunder use the same duration.
''',
)
replace(
    weather,
    '''        let previous = (
            self.world.weather.clear_time,
            self.world.weather.rain_time,
            self.world.weather.thunder_time,
            self.world.weather.raining,
            self.world.weather.thundering,
        );
        let mut cancel = false;
        self.native_weather_change(self.dim.id(), raining, thundering, &mut cancel);
        let old_thundering = self.world.weather.thundering;
        let old_thunder_time = self.world.weather.thunder_time;
        let mut cancel_thunder = false;
        self.native_thunder_change(self.dim.id(), thundering, &mut cancel_thunder);
        if cancel {
            let w = &mut self.world.weather;
            (
                w.clear_time,
                w.rain_time,
                w.thunder_time,
                w.raining,
                w.thundering,
            ) = previous;
        } else {
            // ServerLevel.setWeatherParameters.
            let w = &mut self.world.weather;
            w.clear_time = clear;
            w.rain_time = time;
            w.thunder_time = time;
            w.raining = raining;
            w.thundering = thundering;
            if cancel_thunder {
                w.thunder_time = old_thunder_time;
                w.thundering = old_thundering;
            }
        }
''',
    '''        let mut cancel = false;
        self.native_weather_change(self.dim.id(), raining, thundering, &mut cancel);
        if cancel {
            return;
        }
        let w = &mut self.world.weather;
        w.clear_time = clear;
        w.rain_time = time;
        w.thunder_time = time;
        w.raining = raining;
        w.thundering = thundering;
''',
)
replace(
    weather,
    '''    /// time, inside the `weather_change` or `thunder_change` hook, a parked level, a stale
    /// handle or a client.
''',
    '''    /// time, inside the `weather_change` hook, a parked level, a stale handle or a client.
''',
)
replace(
    game / "modding/level_control_hooks.rs",
    '''//! own hook is refused instead of firing that hook again without end: `weather_change` and
//! `thunder_change` for `set_weather`, `world_border_center_changed` and
''',
    '''//! own hook is refused instead of firing that hook again without end: `weather_change`
//! for `set_weather`, `world_border_center_changed` and
''',
)
replace(
    vocab / "host/level_weather.rs",
    '''    /// Sets the weather as `/weather` does (`ServerLevel.setWeatherParameters`): clear for
    /// `clear_ticks`, or raining (and thundering) for `weather_ticks`, after which the
    /// weather cycle goes on as usual. Rain and thunder then fade in or out as vanilla's do;
    /// `set_weather_levels` sets their strength at once. The `weather_change` hook may cancel
    /// all of it and `thunder_change` the thunder. The players see it after the next sync.
    /// False for a negative time, a call from inside `weather_change` or `thunder_change`, a
    /// parked level, a stale handle or a client.
''',
    '''    /// Sets the weather as `/weather` does (`MinecraftServer.setWeatherParameters`):
    /// `clear_ticks` sets the clear timer and `weather_ticks` sets both rain and thunder
    /// timers. Rain and thunder fade in or out as vanilla's do; `set_weather_levels` sets
    /// their strength at once. The `weather_change` hook may cancel the entire request
    /// before any of its timers or flags are written. Players see an accepted change
    /// after the next sync.
    /// False for a negative time, a call from inside `weather_change`, a parked level,
    /// a stale handle or a client.
''',
)

notes = vocab / "hooks/notes.rs"
replace(
    notes,
    '''- **`weather_change(dimension, raining, thundering, cancel)`**: the server's `/weather` command is about to set
  its final rain and thunder states in `dimension`. Set `cancel` to keep the previous weather parameters, including
  their timers.
- **`thunder_change(dimension, thundering, cancel)`**: the server's `/weather` command is about to change
  thunder. Set `cancel` to keep the previous thunder state and timer.
''',
    '''- **`weather_change(dimension, raining, thundering, cancel)`**: `/weather` or a mod is about to write
  weather parameters in `dimension`. Cancel to skip the entire request, including the clear, rain and
  thunder timers and the two requested flags.
''',
)
replace(
    notes,
    "- The server's `/weather` command is about to change thunder: `thunder_change(dimension, thundering, cancel)`. Set `cancel` and the previous thunder state and timer remain.\n\n",
    "",
)
replace(
    notes,
    "- The server's `/weather` command is about to change its rain and thunder parameters: `weather_change(dimension, raining, thundering, cancel)`. Set `cancel` and the prior weather and timers remain.\n",
    "- `/weather` or a mod is about to write weather parameters: `weather_change(dimension, raining, thundering, cancel)`. Cancel to skip all five fields of the request.\n",
)

fabric = root / "docs/fabric-ledger.md"
replace(
    fabric,
    "Scope: the twelve `base` hooks `prepare_item_enchant`, `prepare_smithing`, `projectile_launch`, `raid_spawn_wave`, `raid_state_change`, `sculk_bloom`, `sleep_finished_time`, `sponge_absorb`, `target_hit`, `thunder_change`, `time_skip` and `trade_select`. All twelve record a gap and no keys are added.",
    "The `base` hooks below record gaps; no keys are added.",
)
replace(
    fabric,
    "| `thunder_change` | **Gap:** none | Fabric API 26.3 declares no weather or thunder event, and the level events it has fire on load, unload or per tick without weather arguments. `cmd_weather` calls `native_thunder_change` for a `/weather` request before its cancel flags are applied, even if the requested thunder state is unchanged, with the dimension id, the new thundering flag and a cancel flag, after `native_weather_change` was already asked. No key. |\n",
    "",
)
replace(
    fabric,
    "`cmd_weather` calls `native_weather_change` before either cancel flag is applied, with the dimension id, the requested raining and thundering flags and a cancel flag, so a veto can restore the previous weather parameters.",
    "`set_weather_parameters` calls `native_weather_change` with the dimension id and the requested raining and thundering flags before writing weather parameters. A veto skips all five fields of the request.",
)

paper = root / "docs/paper-ledger.md"
replace(
    paper,
    "| `thunder_change` | **Partial:** [`ThunderChangeEvent`][paper-ThunderChangeEvent] | Same server-side thunder-state change, with `toThunderState` and cancellation matching the hook's `cancel`, but Paper exposes a cause that the command-only hook does not pass. No key. |\n",
    "",
)
replace(paper, "[paper-ThunderChangeEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/weather/ThunderChangeEvent.html\n", "")
replace(
    paper,
    "The event covers a server-side weather change and cancellation, but it exposes only the rain state (`toWeatherState`) and cause, not the hook's requested thundering state.",
    "Paper exposes only the rain state (`toWeatherState`) and cause. The Sunrise hook sees the rain and thunder flags of one parameter request, and a veto skips both flags and all three timers.",
)

for path, source in rewrites.items():
    path.write_text(source, encoding="utf-8")
thunder_hook.unlink()
