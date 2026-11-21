//! Levels a mod opens and closes while the server runs (`host.runtime_levels()`,
//! docs/modding/src/how-to/world/runtime-levels.md).
//!
//! A request is checked when the mod makes it and queued; the queue is applied after the server
//! tick's level loop (`apply_runtime_levels`), never inside a level's tick, so no hook ever sees a
//! level appear or vanish under it. The table below holds what is open, waiting or closing, and
//! the generation each open of an id was given: a `LevelRef` from before an unload never names a
//! later open of the same id. A delete is an unload and then the removal of the level's files, on
//! the server thread in the same pass, so a reopen is applied only after the files are gone.
use super::{Dim, Game};
use mod_api::{LevelRef, RuntimeLevel, RuntimeLevelError};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};
use sunrise_client::chunk::{ExtraLevel, LevelShape};
use sunrise_core::{LevelKey, identifier::Identifier};
use sunrise_world::{
    dimension_type::DimensionFacts,
    worldgen::{
        mod_generators::{ModChunkGenerator, ModChunkGenerators, mod_chunk_generators},
        overrides::{DIMENSION_TYPES, DIMENSIONS, LevelHeight, WorldgenOverrides, mod_dimensions},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Queued to open.
    Opening,
    Open,
    /// Queued to close.
    Closing,
}

struct Entry {
    generation: u32,
    state: State,
    level: OpenLevel,
}

enum Request {
    Open(&'static str, String),
    Unload(&'static str, String),
    Delete(&'static str, String),
}

/// The runtime levels of one world open: pure bookkeeping, which the game applies.
#[derive(Default)]
pub(in crate::game) struct RuntimeTable {
    entries: BTreeMap<String, Entry>,
    /// Ids whose level is closed but whose files a delete could not all remove: not open, and not
    /// to be opened before a `delete` has finished the removal.
    erasing: BTreeSet<String>,
    queue: Vec<Request>,
    /// The last generation given; never reset, so no two opens of an id share one.
    last_generation: u32,
    #[cfg(test)]
    pub(super) fail_open_after_publication: bool,
    #[cfg(test)]
    pub(super) fail_erase: bool,
}

impl RuntimeTable {
    /// Forget every level, at the end of a world open. The generations stay.
    pub(in crate::game) fn clear(&mut self) {
        self.entries.clear();
        self.erasing.clear();
        self.queue.clear();
    }

    /// Queue the checked level, refusing to replace a pending or existing open.
    pub(super) fn queue_open(
        &mut self,
        owner: &'static str,
        id: &str,
        level: OpenLevel,
    ) -> Result<u32, RuntimeLevelError> {
        if self.entries.contains_key(id) {
            return Err(RuntimeLevelError::AlreadyOpen);
        }
        if self.erasing.contains(id) {
            return Err(RuntimeLevelError::Removing);
        }
        self.last_generation += 1;
        let generation = self.last_generation;
        self.entries.insert(
            id.to_owned(),
            Entry {
                generation,
                state: State::Opening,
                level,
            },
        );
        self.queue.push(Request::Open(owner, id.to_owned()));
        Ok(generation)
    }

    /// Queue the close of `id`: it must be open, not waiting to open or to close.
    pub(in crate::game) fn queue_unload(
        &mut self,
        owner: &'static str,
        id: &str,
    ) -> Result<(), RuntimeLevelError> {
        match self.entries.get_mut(id) {
            Some(entry) if entry.state == State::Open => {
                entry.state = State::Closing;
                self.queue.push(Request::Unload(owner, id.to_owned()));
                Ok(())
            }
            _ => Err(RuntimeLevelError::NotOpen),
        }
    }

    /// Queue the removal of `id`, which the caller has checked except against the table. A level
    /// that is open is marked closing until the removal is applied; one that is not in the table
    /// has only files to remove. A level waiting to open is refused: its open comes first.
    pub(in crate::game) fn queue_delete(
        &mut self,
        owner: &'static str,
        id: &str,
    ) -> Result<(), RuntimeLevelError> {
        if let Some(entry) = self.entries.get_mut(id) {
            if entry.state == State::Opening {
                return Err(RuntimeLevelError::AlreadyOpen);
            }
            entry.state = State::Closing;
        }
        self.queue.push(Request::Delete(owner, id.to_owned()));
        Ok(())
    }

    /// Whether `id` is open: a level waiting to open is not, one waiting to close still is, and so
    /// is one the idle close dropped from memory (it opens again when entered).
    pub(in crate::game) fn is_open(&self, id: &str) -> bool {
        self.entries
            .get(id)
            .is_some_and(|entry| entry.state != State::Opening)
    }

    /// Whether `id` is a runtime level, open or not yet.
    pub(in crate::game) fn holds(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }

    /// The generation a handle to the level `id` must carry: 0 for a level every world has.
    pub(in crate::game) fn generation_of(&self, id: &str) -> u32 {
        self.entries.get(id).map_or(0, |entry| entry.generation)
    }

    fn level_of(&self, id: &str) -> Option<&OpenLevel> {
        self.entries.get(id).map(|entry| &entry.level)
    }

    /// Whether a level below `id` (`id/...`) is in the table, open, waiting or with files to
    /// remove.
    pub(in crate::game) fn any_below(&self, id: &str) -> bool {
        let below = format!("{id}/");
        self.entries
            .keys()
            .chain(self.erasing.iter())
            .any(|other| other.starts_with(&below))
    }

    /// Whether the table knows `id` at all, as a level or as files left to remove.
    fn knows(&self, id: &str) -> bool {
        self.entries.contains_key(id) || self.erasing.contains(id)
    }

    fn is_erasing(&self, id: &str) -> bool {
        self.erasing.contains(id)
    }

    /// `id` is closed and its files are not all gone: it waits for a `delete` to finish.
    fn mark_erasing(&mut self, id: &str) {
        self.erasing.insert(id.to_owned());
    }

    fn finish_erasing(&mut self, id: &str) {
        self.erasing.remove(id);
    }

    /// Whether `id` is a runtime level that is open or closing: a level whose open is still
    /// waiting has nothing to close.
    fn is_live(&self, id: &str) -> bool {
        self.entries
            .get(id)
            .is_some_and(|entry| entry.state != State::Opening)
    }

    /// The own seed of each level in the table, by id: what the hub tells a joining client.
    pub(in crate::game) fn seeds(&self) -> BTreeMap<String, i64> {
        self.entries
            .iter()
            .map(|(id, entry)| (id.clone(), entry.level.seed))
            .collect()
    }

    /// The requests waiting, in the order made. Later requests wait for the next tick.
    fn take_queue(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.queue)
    }

    /// Put a level that was to close back in use: a player came in, or the close failed.
    fn keep_open(&mut self, id: &str) {
        if let Some(entry) = self.entries.get_mut(id)
            && entry.state == State::Closing
        {
            entry.state = State::Open;
        }
    }

    /// Forget `id`: it is closed, or never opened.
    pub(in crate::game) fn remove(&mut self, id: &str) {
        self.entries.remove(id);
    }

    pub(in crate::game) fn mark_open(&mut self, id: &str) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.state = State::Open;
        }
    }
}

/// What runtime levels' opens leaked into the game's `&'static` tables, kept so that every later
/// open reuses it: opening and unloading one id over and over leaks once (#4019).
#[derive(Default)]
struct Leaked {
    // A pack may replace a type under the same id between worlds, so its definition
    // participates in the identity of the facts.
    levels: HashMap<LevelDefinition, LeakedLevel>,
    /// Every table of extra levels an open or unload made, reused when the same set comes back.
    tables: Vec<&'static [ExtraLevel]>,
}

type LevelDefinition = (String, String, Option<String>);
type LeakedLevel = (&'static str, &'static str, &'static DimensionFacts);

static LEAKED: LazyLock<Mutex<Leaked>> = LazyLock::new(Mutex::default);

/// The names and facts of the level `id` of the dimension type `dimension_type`, leaked on its
/// first open only.
fn leaked_level(
    id: &str,
    dimension_type: &str,
    type_json: Option<String>,
    facts: DimensionFacts,
) -> LeakedLevel {
    let mut leaked = LEAKED.lock().unwrap_or_else(|e| e.into_inner());
    *leaked
        .levels
        .entry((id.to_owned(), dimension_type.to_owned(), type_json))
        .or_insert_with(|| {
            (
                id.to_owned().leak(),
                dimension_type.to_owned().leak(),
                Box::leak(Box::new(facts)),
            )
        })
}

/// `levels` as a `&'static` table: one leaked before with the same levels, or a new one.
fn leaked_table(levels: Vec<ExtraLevel>) -> &'static [ExtraLevel] {
    if levels.is_empty() {
        return &[];
    }
    let mut leaked = LEAKED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(table) = leaked.tables.iter().find(|table| ***table == levels[..]) {
        return table;
    }
    let table: &'static [ExtraLevel] = levels.leak();
    leaked.tables.push(table);
    table
}

/// The checked stem and type the open request carries to the end of the tick.
pub(super) struct OpenLevel {
    dimension_type: String,
    shape: LevelShape,
    facts: DimensionFacts,
    type_json: Option<String>,
    generate: Generate,
    seed: i64,
}

impl OpenLevel {
    pub(super) fn resolve(
        overrides: &WorldgenOverrides,
        level: RuntimeLevel,
        generators: &ModChunkGenerators,
    ) -> Result<Self, RuntimeLevelError> {
        let (dimension_type, mut generate, seed) = match level {
            RuntimeLevel::Flat {
                dimension_type,
                seed,
            } => (dimension_type, Generate::Flat, seed),
            RuntimeLevel::Void {
                dimension_type,
                seed,
            } => (dimension_type, Generate::Void, seed),
            RuntimeLevel::Stem { stem, seed } => {
                let id = Identifier::parse(&stem)
                    .map_err(|_| RuntimeLevelError::UnknownStem)?
                    .to_string();
                let mut stem: serde_json::Value = overrides
                    .entry(DIMENSIONS, &id)
                    .and_then(|json| serde_json::from_str(json).ok())
                    .ok_or(RuntimeLevelError::UnknownStem)?;
                let dimension_type = stem["type"]
                    .as_str()
                    .ok_or(RuntimeLevelError::UnknownDimensionType)?
                    .to_owned();
                let generator_id = stem["generator"]["type"]
                    .as_str()
                    .and_then(|name| Identifier::parse(name).ok())
                    .ok_or(RuntimeLevelError::UnknownGenerator)?
                    .to_string();
                stem["generator"]["type"] = serde_json::Value::String(generator_id.clone());
                let generate = if generator_id == "minecraft:noise" {
                    Generate::Noise(stem)
                } else {
                    let generator = generators
                        .get(&generator_id)
                        .ok_or(RuntimeLevelError::UnknownGenerator)?
                        .clone();
                    let sea_level = mod_dimensions()
                        .iter()
                        .find(|registered| registered.id == id)
                        .and_then(|registered| registered.sea_level);
                    Generate::Custom {
                        stem,
                        generator,
                        sea_level,
                    }
                };
                (dimension_type, generate, seed)
            }
            _ => return Err(RuntimeLevelError::UnknownGenerator),
        };
        let dimension_type = Identifier::parse(&dimension_type)
            .map_err(|_| RuntimeLevelError::UnknownDimensionType)?
            .to_string();
        if let Generate::Noise(stem) | Generate::Custom { stem, .. } = &mut generate {
            stem["type"] = serde_json::Value::String(dimension_type.clone());
        }
        let type_json = overrides.entry(DIMENSION_TYPES, &dimension_type);
        let (height, facts) = if let Some(json) = type_json {
            let definition =
                serde_json::from_str(json).map_err(|_| RuntimeLevelError::UnknownDimensionType)?;
            let height =
                LevelHeight::of_json(&definition).ok_or(RuntimeLevelError::UnknownDimensionType)?;
            let base = DimensionFacts::base_for_type(&dimension_type);
            let facts = DimensionFacts::patched(base, &dimension_type, &definition)
                .map_err(|_| RuntimeLevelError::UnknownDimensionType)?;
            (height, facts)
        } else {
            let height = LevelHeight::vanilla(&dimension_type)
                .ok_or(RuntimeLevelError::UnknownDimensionType)?;
            let facts = DimensionFacts::base_for_type(&dimension_type).clone();
            (height, facts)
        };
        let shape = LevelShape {
            min_y: height.min_y,
            height: height.height,
            logical_height: height.logical_height,
            sky_light: height.has_skylight,
            has_ceiling: height.has_ceiling,
        };
        let type_json = type_json.map(str::to_owned);
        Ok(Self {
            dimension_type,
            shape,
            facts,
            type_json,
            generate,
            seed,
        })
    }
}

/// Whether every component of `id`'s namespace and path names one folder on every system: no
/// component ends with a dot or a space, and none is a Windows device name (`con`, `nul`,
/// `com1`, `lpt9`, with or without an extension), which Windows maps to something else.
fn portable_level_id(id: &str) -> bool {
    const DEVICES: [&str; 4] = ["con", "prn", "aux", "nul"];
    id.split([':', '/']).all(|component| {
        let stem = component
            .split('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let numbered = ["com", "lpt"].iter().any(|device| {
            stem.strip_prefix(device)
                .is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit())
        });
        !component.ends_with(['.', ' ']) && !DEVICES.contains(&stem.as_str()) && !numbered
    })
}

/// The entries of `folder`, or what to answer for it when it cannot be listed: a folder that is
/// not there holds nothing, one that cannot be read may hold a level.
fn listing(folder: &Path) -> Result<std::fs::ReadDir, bool> {
    std::fs::read_dir(folder).map_err(|error| error.kind() != std::io::ErrorKind::NotFound)
}

/// Whether `folder`, or a folder below it, has the shape of a level: it holds a folder named
/// `region`, `entities` or `poi`, as every level's save writes. A link counts, since it cannot be
/// looked into, and so does anything that cannot be read.
fn tree_has_level_shape(folder: &Path) -> bool {
    let entries = match listing(folder) {
        Ok(entries) => entries,
        Err(unreadable) => return unreadable,
    };
    entries.into_iter().any(|entry| {
        let Ok(entry) = entry else {
            return true;
        };
        let Ok(kind) = entry.file_type() else {
            return true;
        };
        if kind.is_file() {
            return false;
        }
        kind.is_symlink()
            || matches!(
                entry.file_name().to_str(),
                Some("region" | "entities" | "poi")
            )
            || tree_has_level_shape(&entry.path())
    })
}

/// Whether the folder of a level holds a level of its own: a folder that is not one a level's
/// save writes (`region`, `entities`, `poi`, `data`), one of the first three holding a folder, as
/// the level `mymod:a/region` does under `mymod:a`, or a folder of `data` with a level's shape
/// somewhere below (`mymod:a/data/ab`). Plain files are not levels, and what else `data` holds
/// (mods keep their own files there) is not in the way. What cannot be read counts as a level:
/// the delete is refused rather than remove what it could not look at.
fn folder_holds_levels(folder: &Path) -> bool {
    let entries = match listing(folder) {
        Ok(entries) => entries,
        Err(unreadable) => return unreadable,
    };
    for entry in entries {
        let Ok(entry) = entry else {
            return true;
        };
        let Ok(kind) = entry.file_type() else {
            return true;
        };
        if kind.is_file() {
            continue;
        }
        // A link, or a folder no level's save writes.
        let name = entry.file_name();
        let own = !kind.is_symlink()
            && match name.to_str() {
                Some("data") => !tree_has_level_shape(&entry.path()),
                Some("region" | "entities" | "poi") => match listing(&entry.path()) {
                    Ok(inner) => !inner.into_iter().any(|child| {
                        child
                            .and_then(|child| child.file_type())
                            .map_or(true, |kind| !kind.is_file())
                    }),
                    Err(unreadable) => !unreadable,
                },
                _ => false,
            };
        if !own {
            return true;
        }
    }
    false
}

/// The ids other than `id` whose structure references file is `id`'s: each way of putting a `:`
/// back in place of a `_` of the file name that gives a valid id.
fn sharing_references_file(id: &str) -> Vec<String> {
    let name = id.replace(':', "_");
    let namespace_end = name.find('/').unwrap_or(name.len());
    name.char_indices()
        .filter(|(at, c)| *c == '_' && *at < namespace_end)
        .map(|(at, _)| format!("{}:{}", &name[..at], &name[at + 1..]))
        .filter(|other| other != id && sunrise_core::level_id::valid_level_id(other))
        .collect()
}

/// Fail if `path`, or a folder above it up to `stop` (included), is a link: a link would be
/// followed out of the world. What does not exist is no link.
fn ensure_no_links(path: &Path, stop: &Path, include_path: bool) -> std::io::Result<()> {
    for folder in path.ancestors().skip(usize::from(!include_path)) {
        // `stop` itself, `dimensions` or `data`, is looked at too: a link there leads out of the
        // world as well.
        match std::fs::symlink_metadata(folder) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(std::io::Error::other(format!(
                    "{} is a link",
                    folder.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if folder == stop {
            break;
        }
    }
    Ok(())
}

/// Remove the files of the level `id` from the world `root`: its folder, and the structure
/// references kept beside the world for it unless `references` says another level shares that
/// file, with the folders they leave empty. Whatever is not there is not an error; a link on the
/// way is, before anything is removed. `id` has passed `validate_runtime_id`; the folder comes from it
/// alone.
pub(in crate::game) fn erase_level_files(
    root: &Path,
    id: &str,
    references: bool,
) -> std::io::Result<()> {
    let dimensions = root.join("dimensions");
    let folder = sunrise_client::save::dimension_dir(root, id);
    // Never the folder of anything but a level: `dimension_dir` gives one under `dimensions/`.
    if folder == dimensions || !folder.starts_with(&dimensions) {
        return Err(std::io::Error::other("not a level folder"));
    }
    let file = super::world_loading::structure_references_file(root, id);
    let data = root.join("data");
    ensure_no_links(&folder, &dimensions, true)?;
    if references {
        ensure_no_links(&file, &data, false)?;
    }
    let folder_gone = ignore_missing(std::fs::remove_dir_all(&folder));
    let file_gone = if references {
        ignore_missing(std::fs::remove_file(&file))
    } else {
        Ok(())
    };
    prune_empty_parents(&folder, &dimensions);
    if references {
        prune_empty_parents(&file, &data);
    }
    folder_gone.and(file_gone)
}

fn ignore_missing(result: std::io::Result<()>) -> std::io::Result<()> {
    match result {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Remove the folders above `path` that are empty, up to but not including `stop`.
fn prune_empty_parents(path: &Path, stop: &Path) {
    for parent in path.ancestors().skip(1) {
        if parent == stop || !parent.starts_with(stop) || std::fs::remove_dir(parent).is_err() {
            break;
        }
    }
}

/// What `make_streaming` generates a runtime level with.
pub(in crate::game) enum Generate {
    /// The default superflat layers.
    Flat,
    /// Air.
    Void,
    /// A registered noise stem, including its noise settings and biome source.
    Noise(serde_json::Value),
    /// A registered stem and the custom generator it names.
    Custom {
        stem: serde_json::Value,
        generator: Arc<dyn ModChunkGenerator>,
        sea_level: Option<i32>,
    },
}

impl Dim {
    /// Drops the dimension `id` that [`Self::add_extra`] added: its level is closed, or a remote
    /// server's level that its client left.
    pub(in crate::game) fn remove_extra(id: &str) {
        super::EXTRA_DIMS
            .write()
            .unwrap()
            .retain(|key| mod_api::__private::dimension_id(*key) != Some(id));
    }
}

impl Game<'_> {
    /// The mod running, if it is one and a server world is open: who a runtime level request
    /// belongs to.
    fn runtime_caller(&self) -> Result<&'static str, RuntimeLevelError> {
        if !self.is_logical_server() || self.native_lifecycle.epoch().is_none() {
            return Err(RuntimeLevelError::Unavailable);
        }
        mod_api::content::crash::current_native_mod().ok_or(RuntimeLevelError::Unavailable)
    }

    /// Check the resource id before it is ever used as a save folder.
    fn validate_runtime_id(id: &str) -> Result<(), RuntimeLevelError> {
        if !sunrise_core::level_id::valid_level_id(id) || !portable_level_id(id) {
            return Err(RuntimeLevelError::InvalidId);
        }
        Ok(())
    }

    /// `RuntimeLevels::open`: check the request now, queue it for after the level loop.
    pub(in crate::game) fn runtime_level_open(
        &mut self,
        id: &str,
        level: RuntimeLevel,
    ) -> Result<LevelRef, RuntimeLevelError> {
        let result = self.runtime_level_queue_open(id, level);
        if let Err(error) = &result {
            self.runtime_refusal("open", id, *error);
        }
        result
    }

    fn runtime_level_queue_open(
        &mut self,
        id: &str,
        level: RuntimeLevel,
    ) -> Result<LevelRef, RuntimeLevelError> {
        let owner = self.runtime_caller()?;
        Self::validate_runtime_id(id)?;
        if !self.levels.runtime.holds(id) && Dim::exists(id) {
            return Err(RuntimeLevelError::ReservedId);
        }
        let mut overrides = super::world_loading::worldgen_overrides_of(
            &self.world_root,
            &self.options.resources,
            self.world_biomes
                .as_ref()
                .unwrap_or_else(|| self.res.biomes()),
            &self.level.enabled_features,
            &self.pack.additional_data,
        )
        .0;
        if let Some(generator) = &self.level.generator {
            generator.merge_dimensions(&mut overrides);
        }
        let level = OpenLevel::resolve(&overrides, level, mod_chunk_generators())?;
        let generation = self.levels.runtime.queue_open(owner, id, level)?;
        let epoch = self
            .native_lifecycle
            .epoch()
            .expect("checked by the caller");
        Ok(LevelRef::with_generation(
            epoch,
            mod_api::__private::dimension_key(id),
            generation,
        ))
    }

    /// `RuntimeLevels::unload`: check the request now, queue it for after the level loop.
    pub(in crate::game) fn runtime_level_unload(
        &mut self,
        id: &str,
    ) -> Result<(), RuntimeLevelError> {
        let result = self.runtime_level_queue_unload(id);
        if let Err(error) = &result {
            self.runtime_refusal("unload", id, *error);
        }
        result
    }

    fn runtime_level_queue_unload(&mut self, id: &str) -> Result<(), RuntimeLevelError> {
        let owner = self.runtime_caller()?;
        Self::validate_runtime_id(id)?;
        if self.levels.runtime.is_open(id) && self.level_has_players(Dim::from_id(id)) {
            return Err(RuntimeLevelError::PlayersInside);
        }
        self.levels.runtime.queue_unload(owner, id)
    }

    /// `RuntimeLevels::delete`: check the request now, queue it for after the level loop.
    pub(in crate::game) fn runtime_level_delete(
        &mut self,
        id: &str,
    ) -> Result<(), RuntimeLevelError> {
        let result = self.runtime_level_queue_delete(id);
        if let Err(error) = &result {
            self.runtime_refusal("delete", id, *error);
        }
        result
    }

    fn runtime_level_queue_delete(&mut self, id: &str) -> Result<(), RuntimeLevelError> {
        let owner = self.runtime_caller()?;
        Self::validate_runtime_id(id)?;
        if !self.levels.runtime.holds(id) && Dim::exists(id) {
            return Err(RuntimeLevelError::ReservedId);
        }
        if self.levels.runtime.is_open(id) && self.level_has_players(Dim::from_id(id)) {
            return Err(RuntimeLevelError::PlayersInside);
        }
        if self.levels_below(id) {
            return Err(RuntimeLevelError::LevelsBelow);
        }
        self.levels.runtime.queue_delete(owner, id)
    }

    /// Whether a level exists below `id` (`id/...`): in the table, among the world's levels, or
    /// as a folder in the level's own. Folders nest, so removing `id` would remove it.
    fn levels_below(&self, id: &str) -> bool {
        let below = format!("{id}/");
        self.levels.runtime.any_below(id)
            || Dim::all().iter().any(|dim| dim.id().starts_with(&below))
            || folder_holds_levels(&sunrise_client::save::dimension_dir(&self.world_root, id))
    }

    /// Whether the structure references file of `id` is also the file of a level that exists. A
    /// folder that cannot be looked at may be one: only one that is not there is no level.
    fn references_file_shared(&self, id: &str) -> bool {
        sharing_references_file(id).iter().any(|other| {
            self.levels.runtime.knows(other)
                || Dim::exists(other)
                || !std::fs::symlink_metadata(sunrise_client::save::dimension_dir(
                    &self.world_root,
                    other,
                ))
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        })
    }

    /// `RuntimeLevels::is_open`.
    pub(in crate::game) fn runtime_level_is_open(&self, id: &str) -> bool {
        self.levels.runtime.is_open(id)
    }

    fn runtime_refusal(&self, call: &str, id: &str, error: RuntimeLevelError) {
        log::warn!(
            "mod {}: runtime_levels {call} {id}: refused ({error:?})",
            mod_api::content::crash::current_native_mod().unwrap_or("?"),
        );
    }

    /// Whether a player, or an arrival on its way, is in the level `dim`.
    fn level_has_players(&self, dim: Dim) -> bool {
        let id = dim.id();
        self.lan
            .as_ref()
            .is_some_and(|host| host.hub.guests.as_slice().iter().any(|g| g.dimension == id))
            || !self.arrival_chunks(dim).is_empty()
            || (self.options.dedicated.is_none() && self.player.is_some() && self.dim == dim)
    }

    /// Whether `level` names the level now open under its id: the world open it was made in, and
    /// for a runtime level the open it was given. A handle of an earlier open reads as stale.
    pub(in crate::game) fn level_handle_current(&self, level: LevelRef) -> bool {
        self.native_lifecycle.epoch() == Some(level.epoch())
            && level.generation() == self.levels.runtime.generation_of(level.dimension_id())
    }

    /// A handle to the server level `key`, of the generation it is open with.
    pub(in crate::game) fn level_ref(
        &self,
        epoch: mod_api::WorldEpoch,
        key: mod_api::DimensionKey,
    ) -> LevelRef {
        let id = mod_api::__private::dimension_id(key).expect("the game's table made the key");
        LevelRef::with_generation(epoch, key, self.levels.runtime.generation_of(id))
    }

    /// Apply the requests mods made since the last time, in order. Runs after the level loop of a
    /// server tick, on the server thread, with no level in the middle of a tick. A request
    /// that no longer holds is dropped, with a line in the log.
    pub(in crate::game) fn apply_runtime_levels(&mut self) {
        for request in self.levels.runtime.take_queue() {
            match request {
                Request::Open(owner, id) => {
                    if let Err(why) = self.apply_runtime_open(&id) {
                        // Nothing of it stays: `is_open` says false and the id opens again.
                        self.unpublish_runtime_level(&id);
                        log::warn!("mod {owner}: runtime level {id} did not open: {why}");
                    }
                }
                Request::Unload(owner, id) => {
                    if let Err(why) = self.apply_runtime_unload(&id) {
                        // Still open: `is_open` says true and the mod can unload it again.
                        self.levels.runtime.keep_open(&id);
                        log::warn!(
                            "mod {owner}: runtime level {id} stays open, its close failed: {why}"
                        );
                    }
                }
                Request::Delete(owner, id) => self.apply_runtime_delete(owner, &id),
            }
        }
    }

    fn apply_runtime_open(&mut self, id: &str) -> sunrise_client::Result<()> {
        if self.levels.runtime.is_erasing(id) {
            return Err("the files of an earlier delete are not all removed".into());
        }
        let level = self
            .levels
            .runtime
            .level_of(id)
            .ok_or("the request is gone")?;
        let shape = level.shape;
        // The tables the game reads a level's shape, type and place from learn it first; a failure
        // below takes it out of them again (`unpublish_runtime_level`).
        let (id_name, dimension_type, facts) = leaked_level(
            id,
            &level.dimension_type,
            level.type_json.clone(),
            level.facts.clone(),
        );
        let mut extras = self.level_shapes.extra.to_vec();
        extras.push(ExtraLevel {
            id: id_name,
            dimension_type,
            shape,
        });
        self.level_shapes.extra = leaked_table(extras);
        if let Some(host) = self.lan.as_mut() {
            host.hub.config.levels = self.level_shapes;
            host.hub.config.level_seeds = self.levels.runtime.seeds();
        }
        if let Ok(key) = LevelKey::parse(id) {
            DimensionFacts::add_level(key, facts);
        }
        Dim::add_extra(id);
        let dim = Dim::extra(id).ok_or("the level was not added")?;
        #[cfg(test)]
        if self.levels.runtime.fail_open_after_publication {
            return Err("injected level construction failure".into());
        }
        let mut parked = self.open_runtime_level(dim)?;
        parked.world.game_time = self.world.game_time;
        self.levels.parked.insert(dim, parked);
        self.levels.runtime.mark_open(id);
        self.native_level_opened(dim);
        self.validate_mod_chunk_tickets(dim);
        Ok(())
    }

    fn apply_runtime_unload(&mut self, id: &str) -> sunrise_client::Result<()> {
        let dim = Dim::extra(id).ok_or("the level is not open")?;
        if self.level_has_players(dim) {
            return Err("a player came in".into());
        }
        // The level in place is parked by leaving it; the Overworld is always there to enter.
        let back = self.dim;
        if self.dim == dim {
            self.enter_level(Dim::Overworld)?;
        }
        // A level the idle close already saved and dropped is only forgotten.
        if self.levels.parked.contains_key(&dim)
            && let Err(why) = self.close_parked_level(dim)
        {
            // The level stays as it was, and in place if it was.
            if self.dim != back {
                self.enter_level(back)?;
            }
            return Err(why);
        }
        self.unpublish_runtime_level(id);
        Ok(())
    }

    /// Delete the level `id`: close it if it is open, then remove its files. A level that cannot
    /// close, or has a level below it, stays as it was. One that closed but whose files cannot
    /// all be removed stays closed, with its id kept from `open` until a `delete` finishes.
    fn apply_runtime_delete(&mut self, owner: &'static str, id: &str) {
        if let Err(why) = self.close_to_delete(id) {
            // Still open, for another try.
            self.levels.runtime.keep_open(id);
            log::warn!("mod {owner}: runtime level {id} was not deleted: {why}");
            return;
        }
        match self.erase_runtime_files(id) {
            Ok(()) => self.levels.runtime.finish_erasing(id),
            Err(why) => {
                self.levels.runtime.mark_erasing(id);
                log::warn!(
                    "mod {owner}: runtime level {id} is closed but its files were not all \
                     removed ({why}); delete it again"
                );
            }
        }
    }

    fn close_to_delete(&mut self, id: &str) -> sunrise_client::Result<()> {
        if !self.levels.runtime.holds(id) && Dim::exists(id) {
            return Err("a level every world has".into());
        }
        if self.levels_below(id) {
            return Err("a level exists below it".into());
        }
        // A level whose open is still waiting comes after this request: only files are removed.
        if self.levels.runtime.is_live(id) {
            self.apply_runtime_unload(id)?;
        }
        Ok(())
    }

    fn erase_runtime_files(&self, id: &str) -> std::io::Result<()> {
        #[cfg(test)]
        if self.levels.runtime.fail_erase {
            return Err(std::io::Error::other("injected removal failure"));
        }
        erase_level_files(&self.world_root, id, !self.references_file_shared(id))
    }

    /// The runtime level `id` was unloaded, or failed to open: it leaves every table it was added
    /// to, and its id is free again.
    fn unpublish_runtime_level(&mut self, id: &str) {
        // A save a mod asked for has nothing left to write, and must not open the level again.
        if let Some(dim) = Dim::extra(id) {
            self.levels.cancel_requested_save(dim);
        }
        self.levels.runtime.remove(id);
        Dim::remove_extra(id);
        let extras: Vec<_> = self
            .level_shapes
            .extra
            .iter()
            .copied()
            .filter(|level| level.id != id)
            .collect();
        self.level_shapes.extra = leaked_table(extras);
        if let Some(host) = self.lan.as_mut() {
            host.hub.config.levels = self.level_shapes;
            host.hub.config.level_seeds = self.levels.runtime.seeds();
            host.hub.close_views_of(id);
        }
        if let Ok(key) = LevelKey::parse(id) {
            DimensionFacts::remove_level(key);
            sunrise_world::level::remove_sea_level(key);
        }
    }

    /// What the level `dim` is opened with: its generator, and its seed, which the world's is for
    /// a level every world has.
    pub(in crate::game) fn runtime_terrain(&self, dim: Dim) -> Option<(&Generate, i64)> {
        let level = self.levels.runtime.level_of(dim.id())?;
        Some((&level.generate, level.seed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_runtime_instance_uses_the_registered_stem_without_substituting_its_generator() {
        struct Empty;
        impl ModChunkGenerator for Empty {
            fn build_terrain(
                &self,
                _: &mut dyn sunrise_world::worldgen::mod_generators::ChunkAccess,
            ) {
            }
        }
        let registered: Arc<dyn ModChunkGenerator> = Arc::new(Empty);
        let generators = ModChunkGenerators::from([("terrain:arena".into(), registered.clone())]);
        let mut overrides = WorldgenOverrides::default();
        let stem = json!({
            "type": "minecraft:the_nether",
            "generator": {
                "type": "terrain:arena",
                "settings": "terrain:arena_noise",
                "biome_source": {"type": "biomes:stripes"},
            },
        });
        overrides.set(DIMENSIONS, "maps:arena", stem.to_string());
        let level = OpenLevel::resolve(
            &overrides,
            RuntimeLevel::from_stem("maps:arena", 42),
            &generators,
        )
        .unwrap();
        assert_eq!(level.dimension_type, "minecraft:the_nether");
        assert!(!level.shape.sky_light);
        assert_eq!(level.seed, 42);
        let Generate::Custom {
            stem: bound_stem,
            generator,
            ..
        } = level.generate
        else {
            panic!("expected a registered custom generator")
        };
        assert_eq!(bound_stem, stem);
        assert!(Arc::ptr_eq(&registered, &generator));

        assert_eq!(
            OpenLevel::resolve(
                &overrides,
                RuntimeLevel::from_stem("maps:missing", 42),
                &generators,
            )
            .err(),
            Some(RuntimeLevelError::UnknownStem),
        );
        assert_eq!(
            OpenLevel::resolve(
                &overrides,
                RuntimeLevel::from_stem("maps:arena", 42),
                &ModChunkGenerators::new(),
            )
            .err(),
            Some(RuntimeLevelError::UnknownGenerator),
        );
    }
}
