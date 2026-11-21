# Config files and settings

A mod keeps its options in one of two ways:

| | Config files | Settings |
| --- | --- | --- |
| Lives in | The game's `config/` folder (on a server, `serverconfig/` per world) | `sunrise-mods/.sunrise-mod-data/<mod id>/`, one file per key |
| Edited by | The player, in a text editor | The player, on the mod's page in **Options > Mods** |
| You declare it with | `Hooks::register_configs` | `Registrar::register_setting` |
| You read it in | `config_loading` and `config_reloading` | `ClientHost::setting(&key)` |
| NeoForge counterpart | `ModContainer.registerConfig`, `ModConfigEvent` | `ModConfigSpec` entries on the config screen |

## Config files

Config files sit in the game's shared `config/` folder, beside `options.txt` (beside `server.properties` on a
dedicated server), as Fabric and NeoForge mods' do. Your mod picks its own names: a folder `config/<mod id>/`, or
files `config/<mod id>.<ext>` and `config/<mod id>-<rest>`.

1. In `register_configs`, push each file with its kind (`Client`, `Common` or `Server`) and the default text to
   write when the file is missing. Push all of them every time: each game reads only its own kinds.
2. The game reads each file and calls `config_loading` with the mod id, the file and its text, which your mod
   parses (TOML by convention). Every mod hears every config, so check the mod id.
3. When the file changes on disk, `config_reloading` brings the new text. The game checks modification times once a
   second; a save with the same text, a removed file or an unreadable one fires nothing and keeps the last text.
4. A `Server` config is per world: it loads before `server_about_to_start` from the world's `serverconfig/`, and
   `config_unloading` drops it after `server_stopping`. `Client` and `Common` configs load once and stay until the
   game quits.

These hooks run in the Config [load phase](../concepts/load-phases.md): config files and client settings
reads (on the client side) are available, world handles and transactions are not. A file outside `config/`, or one already registered, is
skipped.

For files the hooks don't manage, `Host::config_files()` gives your mod its own files in `config/`. Paths retain
the ownership spelling above, such as `<mod id>/extra.toml`. The service is bound to the mod whose hook or Part
is running; you cannot reach another mod's files.

## Bulk data

Keep map tiles, waypoints, seeds and exports out of settings and config files:

- `Host::data_files()` is available on both sides, rooted at `<game directory>/sunrise-mod-data/<mod id>/`.
  A client-only mod can key its own subfolders by server or save and dimension.
- `ServerHost::world_data_files()` is server-only, rooted at `<world>/sunrise-mod-data/<mod id>/`.
  Its contents are copied, deleted and backed up with the save. Without a current world it refuses calls with
  `ConfigFileError::NoWorld`; a client has no world-folder service.

Both use the same `ConfigFiles`, `ConfigPath`, `ConfigEntry` and `ConfigFileError` types as config files. Bulk paths are relative to your implicit mod root: `tiles/overworld/0.bin`, not
`<mod id>/tiles/overworld/0.bin`. The game never scans bulk data as settings or config.

`read`, `write` and `stat` operate on checked `/`-separated paths; traversal (`..`), backslashes and symbolic links
are refused. Writes replace the whole file atomically, never exposing partial contents. Sunrise's own per-file
limits are `DATA_FILE_MAX` (64 MiB) for both bulk areas and `CONFIG_FILE_MAX` (4 MiB) for config files.
Oversized files and unavailable operations return typed errors, rather than partially writing.
`remove` deletes one file under the same confinement and returns whether it was there; it refuses a folder
with `ConfigFileError::NotAFile` and never removes one.

Walk a folder with `list(&folder, after, limit)`: start with `after = None`, then pass the last returned entry's
name as the exclusive cursor. Names are lexicographically ordered; each page has at most `limit` entries.
`limit` must be between 1 and `CONFIG_LIST_MAX` (1024), or the call returns `ConfigFileError::InvalidPage`.
Use `ConfigPath::root()` to list your own root, and stop when a page is empty.

## Settings

Register each setting from `Hooks::register`, for example with
`r.register_setting(SettingSpec::toggle("show_grid", "example.show_grid", false))`.
A spec declares a key (`a-z`, `0-9`, `_`, `-` and `.`, not starting with `.`), translation keys for its label
and optional tooltip, and its kind with the default. Handle the registration error and retain the returned
`SettingKey` on your mod. The key identifies that registration: no live settings operation takes a mod ID or
looks at the currently running callback to find the owner. Settings appear on your mod's page in **Options > Mods**.

- `host.setting(&show_grid)` on a `ClientHost` returns `Option<SettingValue>`; match `Some(SettingValue::Toggle(on))`
  for this example. The value is the live choice, or the registered default if there is no valid saved value.
  `None` means the key is not available on that host.
- `host.set_setting(&show_grid, SettingValue::Toggle(true))` on a mutable `ClientHost` uses the same save path as
  the settings page. Handle `SettingWriteError`: an invalid value or failed save does not replace the live value.
- After a value actually changes (an edit in Options > Mods, a mod write, or a reload), `setting_changed` tells
  only the owner. The new value is already visible to `host.setting`. A write from inside `setting_changed`
  queues its notification for the next client tick instead of invoking the hook recursively. Opening the mod's
  page also reloads that mod's files. Load and worker contexts read frozen snapshots, not live client values.

The settings folder is settings only, not bulk storage: ordinary files there are read as setting keys and
subfolders or unsafe entries still warn during snapshots. Use the bulk services above instead; their files are
not included in settings snapshots.
