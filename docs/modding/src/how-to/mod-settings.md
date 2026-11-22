# Config files and settings

A mod keeps its options in one of two ways:

| | Config files | Settings |
| --- | --- | --- |
| Lives in | The game's `config/` folder (on a server, `serverconfig/` per world) | `sunrise-mods/.sunrise-mod-data/<mod id>/`, one file per key |
| Edited by | The player, in a text editor | The player on the mod's page in **Options > Mods**, or the mod through `ClientHost::set_setting` |
| You declare it with | `Hooks::register_configs` | `Registrar::register_setting` (returns `SettingKey`) |
| You read it in | `config_loading` and `config_reloading` | `ClientHost::setting(key)` and the typed `setting_*` reads |
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

Register each setting from `Hooks::register` with `Registrar::register_setting(SettingSpec::…)` to receive an
owner-bound `SettingKey`: a key (`a-z`, `0-9`, `_`, `-` and `.`, not starting with `.`), translation keys for its
label and tooltip, and its kind with the default. A setting the spec check refuses, or a key you registered
already, comes back as the error. Settings are client settings and appear on your mod's page in
**Options > Mods**, opened from its **Config** button.

- Read a setting with `client_host.setting(key)` or the typed getters (`setting_bool`, `setting_choice`,
  `setting_f64`, `setting_color`), all of which take the `SettingKey` minted at registration and return
  `Result<_, SettingAccessError>`.
- Update a setting with `client_host.set_setting(key, value) -> Result<bool, SettingAccessError>`. It validates
  against the registered spec, persists the file atomically (refusing symlinks and directories), updates the
  single client settings store and Options > Mods view, and delivers `setting_changed` synchronously when the
  value changed (comparing `Range` floats by bits so `-0.0` and `+0.0` are distinct, matching vanilla's
  `OptionInstance.set`).
- `setting_changed` also fires when an **Options > Mods** edit changes the value, or when resource reload or
  opening the mod's page picks up an external change. The hook receives the key name; read the effective value
  through the `SettingKey` you retained at registration. Startup does not fire the hook. If your listener writes
  another setting during a reload, that write fires synchronously and replaces any unsent reload notice for that key.

A mod only holds the `SettingKey` handles minted for its own registrations, so there is no raw mod-id read/write
bypass and no thread-local caller guessing.

The live store reads only registered keys. Generic load snapshots still enumerate files for admitted mods and
can omit entries that exceed their budgets. The settings folder is not bulk storage; use the bulk services above
instead. A server has no live client settings setter: use the config-file service for server-owned settings, or
world data files for mod-owned persistence per world.
