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

These hooks run in the Config [load phase](../concepts/load-phases.md): settings reads and config files are
available, world handles and transactions are not. A file outside `config/`, or one already registered, is
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

Register each client setting from `Hooks::register` with `Registrar::register_setting`. Pass a `SettingSpec`
with a key (`a-z`, `0-9`, `_`, `-` and `.`, not starting with `.`), a translation key for the label, the kind
and its default. Use `.tooltip(...)` to add a tooltip translation key. Registration returns a `SettingKey`
bound to your declaration, or a `SettingError` if the declaration is invalid or its key is already registered.
Settings appear on your mod's page in **Options > Mods**, opened from its **Config** button.

Keep the returned key in your mod. In a client callback, use it instead of passing a mod ID or file name:

```rust,ignore
fn register_enabled(registrar: &mut dyn Registrar) -> Result<SettingKey, SettingError> {
    registrar.register_setting(SettingSpec::toggle("enabled", "example.enabled", false))
}

// Call from a client callback with the key your mod retained from registration.
fn enable(host: &mut dyn ClientHost, key: &SettingKey) -> Result<(), SettingWriteError> {
    host.set_setting(key, SettingValue::Toggle(true))
}

fn enabled(host: &dyn ClientHost, key: &SettingKey) -> bool {
    matches!(host.setting(key), Some(SettingValue::Toggle(true)))
}
```

`setting` reads the live value, without parsing or file I/O. A missing or invalid saved value uses the declared
default. `set_setting` validates the value against the registered kind and persists it through the same path as
the UI. An unavailable key, invalid value or failed save returns a `SettingWriteError` and leaves the live value
unchanged. An Options > Mods page open rereads that mod's values; a client resource reload rereads them all.

A successful change, whether made by the player, by a mod or by a reload, is visible before the client tick
notifies only the registered mod through `setting_changed`. Read the current value using the retained key.
Writes performed inside that notification are delivered on the next tick, not recursively. A panic disables
only that mod's `setting_changed` callback for the session and reports the mod in the log and error toast.

The one-file-per-key format is unchanged. These live settings are separate from the frozen inputs of load,
loop and worldgen callbacks. Keep bulk storage in the bulk data folders above, not in the settings folder.
