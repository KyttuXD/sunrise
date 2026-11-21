//! The entries of the datapack registries mods declared, as one world loads them:
//! a candidate built from the winning pack files and checked
//! as a whole, then published as a set and read directly by `(registry, entry)`. Nothing here
//! touches the world; the game builds a candidate before it commits any table and keeps the old
//! set when building fails. See `docs/design/mod-api/mod-datapack-registries.md`.
use super::datapack_registry::{
    DatapackDecodeError, DatapackEntry, DatapackFieldKind, DatapackRegistryDeclaration,
    DatapackValue, valid_namespaced,
};
use std::collections::{BTreeMap, BTreeSet};

/// Why a candidate was refused. The whole candidate fails, never one entry.
#[derive(Clone, Debug, PartialEq, Eq)]
// no wire: loading runs in the game; mods read published entries, never load failures
#[non_exhaustive]
pub enum DatapackLoadReason {
    /// The winning file could not be read. Carries the underlying I/O error message.
    ReadFile(String),
    /// The winning file is not UTF-8 text.
    NotUtf8,
    /// The entry's id (from its namespace and file name) is not a lowercase `namespace:path`.
    InvalidEntryId,
    /// The file did not decode against the registry's schema.
    Decode(DatapackDecodeError),
    /// A resolved tag has an invalid name or names an entry outside its accepted registry.
    InvalidTag {
        /// The tag's canonical name.
        tag: String,
        /// The invalid member, or empty when the tag name or registry is invalid.
        member: String,
    },
    /// A reference names an entry of a declared registry that no pack defines.
    UnresolvedReference {
        /// The referring field.
        field: String,
        /// The registry the field names.
        registry: String,
        /// The entry it names (`namespace:path`).
        target: String,
    },
}

/// A refused candidate: which registry, entry and file, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
// no wire: loading runs in the game; mods read published entries, never load failures
// exhaustive: the game builds it with a literal and reports it
pub struct DatapackLoadError {
    /// The registry's key (`example:spell`).
    pub registry: String,
    /// The entry's id (`addon:fire`), as far as it was known.
    pub entry: String,
    /// The file the entry came from.
    pub file: String,
    /// The id of the pack whose version of `file` won (`file/override`); empty when no pack is
    /// involved, as for resolved tags or a snapshot a server sent.
    pub pack: String,
    /// What was wrong.
    pub reason: DatapackLoadReason,
}

/// Every entry of every declared registry, decoded and checked, not yet published.
#[derive(Clone, Debug, PartialEq)]
// no wire: the game's candidate; a mod reads the published set through a level handle
// exhaustive: built only by [`DatapackCandidate::build`]
pub struct DatapackCandidate {
    pub(super) registries: BTreeMap<String, BTreeMap<String, DatapackEntry>>,
    pub(super) tags: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    /// The synchronized registries, limited to those actually sent for a received candidate.
    pub(super) synchronized: BTreeSet<String>,
}

/// The id of the entry in `file` (`data/addon/example/spell/fire.json` is `addon:fire`), when
/// the file is in `declaration`'s folder.
fn entry_of(declaration: &DatapackRegistryDeclaration, file: &str) -> Option<String> {
    let rest = file.strip_prefix("data/")?;
    let (namespace, rest) = rest.split_once('/')?;
    let (registry_namespace, registry_path) = declaration.key.split_once(':')?;
    let path = rest
        .strip_prefix(registry_namespace)?
        .strip_prefix('/')?
        .strip_prefix(registry_path)?
        .strip_prefix('/')?
        .strip_suffix(".json")?;
    (!path.is_empty()).then(|| format!("{namespace}:{path}"))
}

impl DatapackCandidate {
    /// Decodes the winning file of every entry of every declared registry in `files`
    /// (`data/<entry namespace>/<registry namespace>/<registry path>/<entry>.json`, each with
    /// its versions in pack order, the last winning as `ResourceManager` hands them out), then
    /// resolves every reference to a declared registry against the whole candidate, so a
    /// forward reference works and an absent target refuses everything. A reference to a
    /// registry no declaration here covers (a vanilla one) is not checked. The caller has
    /// already dropped files whose pack conditions do not hold. Each version is its pack's id
    /// and its bytes, so a refusal names the pack. Local loading does not check snapshot size
    /// or entry counts. The error is boxed: it is large, and a refused candidate is rare.
    pub fn build(
        declarations: &[DatapackRegistryDeclaration],
        files: &[(String, Vec<(String, Vec<u8>)>)],
    ) -> Result<Self, Box<DatapackLoadError>> {
        let mut registries: BTreeMap<String, BTreeMap<String, DatapackEntry>> = declarations
            .iter()
            .map(|declaration| (declaration.key.clone(), BTreeMap::new()))
            .collect();
        for declaration in declarations {
            let fail = |entry: &str, file: &str, pack: &str, reason| {
                Box::new(DatapackLoadError {
                    registry: declaration.key.clone(),
                    entry: entry.to_owned(),
                    file: file.to_owned(),
                    pack: pack.to_owned(),
                    reason,
                })
            };
            for (file, versions) in files {
                let Some(entry) = entry_of(declaration, file) else {
                    continue;
                };
                let winner = versions.last();
                let pack = winner.map_or("", |(pack, _)| pack.as_str());
                let fail = |reason| fail(&entry, file, pack, reason);
                if !valid_namespaced(&entry) {
                    return Err(fail(DatapackLoadReason::InvalidEntryId));
                }
                let Some((_, bytes)) = winner else {
                    continue;
                };
                let entries = registries
                    .get_mut(&declaration.key)
                    .expect("every declaration has a registry");
                let text =
                    std::str::from_utf8(bytes).map_err(|_| fail(DatapackLoadReason::NotUtf8))?;
                // Conditions belong to the engine, not to the declaring mod's schema.
                let stripped = serde_json::from_str::<serde_json::Value>(text)
                    .ok()
                    .and_then(|mut json| {
                        json.as_object_mut()?.remove("neoforge:conditions")?;
                        serde_json::to_string(&json).ok()
                    });
                let decoded = declaration
                    .decode_entry(stripped.as_deref().unwrap_or(text))
                    .map_err(|error| fail(DatapackLoadReason::Decode(error)))?;
                entries.insert(entry, decoded);
            }
        }
        resolve_references(declarations, &registries).map_err(|mut error| {
            // The referring entry's winning pack.
            if let Some((_, versions)) = files.iter().find(|(file, _)| *file == error.file) {
                error.pack = versions
                    .last()
                    .map_or_else(String::new, |(pack, _)| pack.clone());
            }
            error
        })?;
        Ok(Self {
            registries,
            tags: BTreeMap::new(),
            synchronized: declarations
                .iter()
                .filter(|d| d.sync != super::datapack_registry::DatapackRegistrySync::ServerOnly)
                .map(|d| d.key.clone())
                .collect(),
        })
    }

    /// The accepted entry names of a registry, sorted, before publication.
    pub fn entries(&self, registry: &str) -> Result<Vec<&str>, DatapackReadError> {
        Ok(self
            .registries
            .get(registry)
            .ok_or(DatapackReadError::UnknownRegistry)?
            .keys()
            .map(String::as_str)
            .collect())
    }

    /// Binds the engine's fully resolved tags atomically against accepted entries.
    /// An unknown registry, invalid tag name or missing member leaves the candidate unchanged.
    pub fn bind_tags(
        &mut self,
        registry: &str,
        tags: BTreeMap<String, Vec<String>>,
    ) -> Result<(), Box<DatapackLoadError>> {
        let fail = |reason| {
            Box::new(DatapackLoadError {
                registry: registry.to_owned(),
                entry: String::new(),
                file: String::new(),
                pack: String::new(),
                reason,
            })
        };
        let entries = self.registries.get(registry).ok_or_else(|| {
            fail(DatapackLoadReason::InvalidTag {
                tag: String::new(),
                member: String::new(),
            })
        })?;
        for (tag, members) in &tags {
            if !valid_namespaced(tag) {
                return Err(fail(DatapackLoadReason::InvalidTag {
                    tag: tag.clone(),
                    member: String::new(),
                }));
            }
            if let Some(member) = members.iter().find(|id| !entries.contains_key(*id)) {
                return Err(fail(DatapackLoadReason::InvalidTag {
                    tag: tag.clone(),
                    member: member.clone(),
                }));
            }
        }
        self.tags.insert(registry.to_owned(), tags);
        Ok(())
    }
}

/// Why a lookup failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
#[derive(sunrise_mod_api_macros::Wire)]
pub enum DatapackReadError {
    /// No registry has this key.
    UnknownRegistry,
    /// The registry has no such entry (a reload removed it, or no pack defines it).
    MissingEntry,
    /// The game has no world open to read from, or no host answered.
    Unavailable,
}
impl crate::NoBackend for DatapackReadError {
    fn no_backend() -> Self {
        Self::Unavailable
    }
}

/// Checks every reference to a declared registry against the whole `registries`, so a forward
/// reference works and an absent target refuses everything. A reference to a registry no
/// declaration covers (a vanilla one) is not checked.
pub(super) fn resolve_references(
    declarations: &[DatapackRegistryDeclaration],
    registries: &BTreeMap<String, BTreeMap<String, DatapackEntry>>,
) -> Result<(), Box<DatapackLoadError>> {
    for declaration in declarations {
        for (entry, decoded) in &registries[&declaration.key] {
            for field in &declaration.fields {
                let DatapackFieldKind::Reference { registry } = &field.kind else {
                    continue;
                };
                let Some(targets) = registries.get(registry) else {
                    continue;
                };
                let Some(DatapackValue::Text(target)) = decoded.get(&field.name) else {
                    continue;
                };
                if !targets.contains_key(target) {
                    return Err(Box::new(DatapackLoadError {
                        registry: declaration.key.clone(),
                        entry: entry.clone(),
                        file: declaration.entry_file(entry).unwrap_or_default(),
                        pack: String::new(),
                        reason: DatapackLoadReason::UnresolvedReference {
                            field: field.name.clone(),
                            registry: registry.clone(),
                            target: target.clone(),
                        },
                    }));
                }
            }
        }
    }
    Ok(())
}

/// The published entries of a world's datapack registries.
#[derive(Clone, Debug, PartialEq)]
// no wire: the game holds it; a mod reads through a level handle
// exhaustive: built only by [`DatapackRegistrySet::from_candidate`] and [`DatapackRegistrySet::received`]
pub struct DatapackRegistrySet {
    pub(super) registries: BTreeMap<String, BTreeMap<String, DatapackEntry>>,
    pub(super) tags: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    /// None on a server; on a client, exactly the registries in its accepted snapshot.
    pub(super) received_registries: Option<BTreeSet<String>>,
}

impl DatapackRegistrySet {
    /// Publishes `candidate` as a world's active registry set.
    pub fn from_candidate(candidate: DatapackCandidate) -> Self {
        Self {
            registries: candidate.registries,
            tags: candidate.tags,
            received_registries: None,
        }
    }

    /// Whether generic reads may access this registry. Typed reads retain their existing
    /// empty defaults for omitted declarations; a received empty registry is still available.
    pub fn registry_available(&self, registry: &str) -> bool {
        self.received_registries.as_ref().map_or_else(
            || self.registries.contains_key(registry),
            |received| received.contains(registry),
        )
    }

    /// The entry `entry` of `registry` in this set.
    pub fn get(&self, registry: &str, entry: &str) -> Result<&DatapackEntry, DatapackReadError> {
        self.registries
            .get(registry)
            .ok_or(DatapackReadError::UnknownRegistry)?
            .get(entry)
            .ok_or(DatapackReadError::MissingEntry)
    }

    /// The entry ids of `registry` in this set, sorted.
    pub fn entries(&self, registry: &str) -> Result<Vec<&str>, DatapackReadError> {
        let entries = self
            .registries
            .get(registry)
            .ok_or(DatapackReadError::UnknownRegistry)?;
        Ok(entries.keys().map(String::as_str).collect())
    }

    /// Whether the current set contains this entry.
    /// Unknown registries and missing entries both return false.
    pub fn contains_entry(&self, registry: &str, id: &str) -> bool {
        self.get(registry, id).is_ok()
    }

    /// Current merged-tag membership. None for an unknown registry or entry; absent tags are false.
    pub fn tag_contains(&self, registry: &str, entry: &str, tag: &str) -> Option<bool> {
        self.get(registry, entry).ok()?;
        Some(
            self.tags
                .get(registry)
                .and_then(|tags| tags.get(tag))
                .is_some_and(|members| members.iter().any(|member| member == entry)),
        )
    }
}

impl From<DatapackCandidate> for DatapackRegistrySet {
    fn from(candidate: DatapackCandidate) -> Self {
        Self::from_candidate(candidate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::datapack_registry::{DatapackField, DatapackRegistrySync};

    /// `example:spell`: `{power: integer 0..100, next: reference to example:spell = example:fire}`.
    fn spell() -> DatapackRegistryDeclaration {
        DatapackRegistryDeclaration {
            mod_id: "example".into(),
            key: "example:spell".into(),
            fields: vec![
                DatapackField::required("power", DatapackFieldKind::Int { min: 0, max: 100 }),
                DatapackField::with_default(
                    "next",
                    DatapackFieldKind::Reference {
                        registry: "example:spell".into(),
                    },
                    DatapackValue::Text("example:fire".into()),
                ),
            ],
            sync: DatapackRegistrySync::ServerOnly,
        }
    }

    /// A file whose version `n` is in the pack `pack/<n>`.
    fn file(path: &str, versions: &[&str]) -> (String, Vec<(String, Vec<u8>)>) {
        (
            path.to_owned(),
            versions
                .iter()
                .enumerate()
                .map(|(n, v)| (format!("pack/{n}"), v.as_bytes().to_vec()))
                .collect(),
        )
    }

    fn power(set: &DatapackRegistrySet, entry: &str) -> DatapackValue {
        set.get("example:spell", entry)
            .unwrap()
            .get("power")
            .unwrap()
            .clone()
    }

    fn candidate(files: &[(String, Vec<(String, Vec<u8>)>)]) -> DatapackCandidate {
        DatapackCandidate::build(&[spell()], files).unwrap()
    }

    /// Base fire power 3, a higher pack's 7: one entry, the winner's value.
    #[test]
    fn the_winning_pack_file_is_the_one_entry() {
        let files = [file(
            "data/example/example/spell/fire.json",
            &[r#"{"power": 3}"#, r#"{"power": 7}"#],
        )];
        let set = DatapackRegistrySet::from_candidate(candidate(&files));
        assert_eq!(power(&set, "example:fire"), DatapackValue::Int(7));
        assert_eq!(set.entries("example:spell").unwrap(), ["example:fire"]);
        assert!(set.contains_entry("example:spell", "example:fire"));
        assert!(!set.contains_entry("example:spell", "example:missing"));
        assert!(!set.contains_entry("example:unknown", "example:fire"));
    }

    /// An entry in another mod's namespace sits in that namespace's folder; nested paths work;
    /// files of other registries and other folders are not entries.
    #[test]
    fn entries_come_from_their_namespaced_folder_only() {
        let files = [
            file("data/example/example/spell/fire.json", &[r#"{"power": 3}"#]),
            file("data/addon/example/spell/ice.json", &[r#"{"power": 4}"#]),
            file(
                "data/addon/example/spell/nested/hail.json",
                &[r#"{"power": 5}"#],
            ),
            file("data/example/example/other/fire.json", &[r#"{"power": 1}"#]),
            file("data/example/spell/fire.json", &[r#"{"power": 1}"#]),
            file("data/addon/example/spell/ice.txt", &[r#"{"power": 1}"#]),
        ];
        let set = DatapackRegistrySet::from_candidate(candidate(&files));
        assert_eq!(
            set.entries("example:spell").unwrap(),
            ["addon:ice", "addon:nested/hail", "example:fire"]
        );
        assert_eq!(
            set.entries("example:nothing"),
            Err(DatapackReadError::UnknownRegistry)
        );
    }

    /// Publishing a new candidate reflects updated values and removed entries while leaving
    /// the prior immutable set untouched.
    #[test]
    fn a_new_candidate_updates_and_removes_entries() {
        let files = [file(
            "data/example/example/spell/fire.json",
            &[r#"{"power": 7}"#],
        )];
        let one = DatapackRegistrySet::from_candidate(candidate(&files));
        let nine = [file(
            "data/example/example/spell/fire.json",
            &[r#"{"power": 7}"#, r#"{"power": 9}"#],
        )];
        let two = DatapackRegistrySet::from_candidate(candidate(&nine));
        assert_eq!(power(&two, "example:fire"), DatapackValue::Int(9));
        assert_eq!(power(&one, "example:fire"), DatapackValue::Int(7));
        // Remove fire.
        let empty = DatapackRegistrySet::from_candidate(candidate(&[]));
        assert_eq!(
            empty.get("example:spell", "example:fire"),
            Err(DatapackReadError::MissingEntry)
        );
    }

    /// Invalid winning data (power 101, malformed JSON) fails the candidate with the file and
    /// field, and does not fall back to the lower-priority version; the old set is untouched.
    #[test]
    fn invalid_winning_data_refuses_the_candidate_and_keeps_the_old_set() {
        let good = [file(
            "data/example/example/spell/fire.json",
            &[r#"{"power": 7}"#],
        )];
        let old = DatapackRegistrySet::from_candidate(candidate(&good));
        let bad = [file(
            "data/example/example/spell/fire.json",
            &[r#"{"power": 7}"#, r#"{"power": 101}"#],
        )];
        assert_eq!(
            DatapackCandidate::build(&[spell()], &bad),
            Err(Box::new(DatapackLoadError {
                registry: "example:spell".into(),
                entry: "example:fire".into(),
                file: "data/example/example/spell/fire.json".into(),
                pack: "pack/1".into(),
                reason: DatapackLoadReason::Decode(DatapackDecodeError::OutOfRange {
                    field: "power".into()
                }),
            }))
        );
        let malformed = [file(
            "data/example/example/spell/fire.json",
            &[r#"{"power": 7}"#, "{"],
        )];
        let refused = DatapackCandidate::build(&[spell()], &malformed).unwrap_err();
        assert!(matches!(
            refused.reason,
            DatapackLoadReason::Decode(DatapackDecodeError::Malformed(_))
        ));
        assert_eq!(power(&old, "example:fire"), DatapackValue::Int(7));
        let next = DatapackRegistrySet::from_candidate(candidate(&good));
        assert_eq!(power(&next, "example:fire"), DatapackValue::Int(7));
    }

    /// A forward reference resolves after the candidate completes; an absent target refuses it
    /// all.
    #[test]
    fn references_resolve_against_the_whole_candidate() {
        let forward = [
            file(
                "data/example/example/spell/fire.json",
                &[r#"{"power": 4, "next": "addon:ice"}"#],
            ),
            file("data/addon/example/spell/ice.json", &[r#"{"power": 2}"#]),
        ];
        // `ice` leaves `next` at its default, `example:fire`, which exists: a cycle, which
        // references by name allow.
        let set = DatapackRegistrySet::from_candidate(candidate(&forward));
        assert_eq!(power(&set, "example:fire"), DatapackValue::Int(4));
        let missing = [file(
            "data/example/example/spell/fire.json",
            &[r#"{"power": 4, "next": "addon:ice"}"#],
        )];
        assert_eq!(
            DatapackCandidate::build(&[spell()], &missing),
            Err(Box::new(DatapackLoadError {
                registry: "example:spell".into(),
                entry: "example:fire".into(),
                file: "data/example/example/spell/fire.json".into(),
                pack: "pack/0".into(),
                reason: DatapackLoadReason::UnresolvedReference {
                    field: "next".into(),
                    registry: "example:spell".into(),
                    target: "addon:ice".into(),
                },
            }))
        );
    }

    /// A reference to a registry no declaration covers (a vanilla one) is not checked here.
    #[test]
    fn a_reference_to_an_undeclared_registry_is_left_to_its_owner() {
        let mut declaration = spell();
        declaration.fields[1].kind = DatapackFieldKind::Reference {
            registry: "minecraft:block".into(),
        };
        declaration.fields[1].default = Some(DatapackValue::Text("minecraft:stone".into()));
        let files = [file(
            "data/example/example/spell/fire.json",
            &[r#"{"power": 1, "next": "minecraft:nothing"}"#],
        )];
        assert!(DatapackCandidate::build(&[declaration], &files).is_ok());
    }

    /// An entry whose id is not a lowercase `namespace:path`, or whose file is not UTF-8, refuses
    /// the candidate.
    #[test]
    fn an_unusable_entry_file_refuses_the_candidate() {
        let reason = |files: &[(String, Vec<(String, Vec<u8>)>)]| {
            DatapackCandidate::build(&[spell()], files)
                .unwrap_err()
                .reason
        };
        assert_eq!(
            reason(&[file(
                "data/example/example/spell/Fire.json",
                &[r#"{"power": 1}"#]
            )]),
            DatapackLoadReason::InvalidEntryId
        );
        assert_eq!(
            reason(&[(
                "data/example/example/spell/fire.json".to_owned(),
                vec![("pack/0".to_owned(), vec![0xff, 0xfe])]
            )]),
            DatapackLoadReason::NotUtf8
        );
    }

    /// With nothing declared, no file is read and the set is empty.
    #[test]
    fn nothing_declared_decodes_nothing() {
        let files = [file(
            "data/example/example/spell/fire.json",
            &[r#"{"power": 101}"#],
        )];
        let candidate = DatapackCandidate::build(&[], &files).unwrap();
        let set = DatapackRegistrySet::from_candidate(candidate);
        assert_eq!(
            set.entries("example:spell"),
            Err(DatapackReadError::UnknownRegistry)
        );
    }

    #[test]
    fn a_local_entry_over_one_mib_loads() {
        let text = format!(r#"{{"power": 7{}}}"#, " ".repeat(1 << 20));
        let files = [file("data/example/example/spell/fire.json", &[&text])];
        let set = DatapackRegistrySet::from_candidate(candidate(&files));
        assert_eq!(power(&set, "example:fire"), DatapackValue::Int(7));
    }
}

#[cfg(feature = "testing")]
#[path = "datapack_registry_fixture_support.rs"]
mod fixture_support;
