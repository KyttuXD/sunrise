//! The entity types of a resource root's `entity-types.json`: vanilla's and the content mods' (each
//! mod type has its template's row). A living entity's row lists its default attributes, so a row
//! with `attributes` is a living type (`LivingEntity`), and one without is not (a boat, an arrow, a
//! display). Read once per root: the file doesn't change while the game runs.
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

/// Player renderers are keyed by model, not by this type id.
pub(in crate::game) const PLAYER: &str = "minecraft:player";

/// The entity types under one root, each list sorted.
#[derive(Debug, Default, PartialEq)]
pub(in crate::game) struct EntityTypeRows {
    /// Types listed by the layer registration event. Players have their own model list.
    pub(super) non_player_ids: Vec<String>,
    player: bool,
    /// The living ones.
    living: Vec<String>,
}

impl EntityTypeRows {
    /// The rows of `entity-types.json`'s text; none when it isn't an object.
    pub(in crate::game) fn parse(bytes: &[u8]) -> Self {
        let rows = serde_json::from_slice::<serde_json::Value>(bytes).unwrap_or_default();
        let mut out = Self::default();
        for (id, row) in rows.as_object().into_iter().flatten() {
            let id = sunrise_client::command::args::normalize(id);
            if row.get("attributes").is_some() {
                out.living.push(id.clone());
            }
            if id == PLAYER {
                out.player = true;
            } else {
                out.non_player_ids.push(id);
            }
        }
        out.non_player_ids.sort();
        out.living.sort();
        out
    }

    /// All types, including the player, in command suggestion order.
    pub(in crate::game) fn ids(&self) -> impl Iterator<Item = &str> {
        let (before, after) = self.non_player_ids.split_at(if self.player {
            self.non_player_ids
                .partition_point(|id| id.as_str() < PLAYER)
        } else {
            self.non_player_ids.len()
        });
        before
            .iter()
            .map(String::as_str)
            .chain(self.player.then_some(PLAYER))
            .chain(after.iter().map(String::as_str))
    }

    /// Whether `id` is an entity type.
    pub(in crate::game) fn contains(&self, id: &str) -> bool {
        if id == PLAYER {
            self.player
        } else {
            self.non_player_ids
                .binary_search_by(|t| t.as_str().cmp(id))
                .is_ok()
        }
    }

    /// Whether `id` is a living entity type.
    pub(in crate::game) fn is_living(&self, id: &str) -> bool {
        self.living.binary_search_by(|t| t.as_str().cmp(id)).is_ok()
    }
}

/// Each root's rows, read on first use.
static ROWS: Mutex<Vec<(PathBuf, Arc<EntityTypeRows>)>> = Mutex::new(Vec::new());

/// The entity types under `root` (none when its `entity-types.json` is missing or unreadable).
pub(in crate::game) fn entity_type_rows(root: &Path) -> Arc<EntityTypeRows> {
    let mut cache = ROWS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((_, rows)) = cache.iter().find(|(cached, _)| cached == root) {
        return rows.clone();
    }
    let bytes = std::fs::read(root.join("entity-types.json")).unwrap_or_default();
    let rows = Arc::new(EntityTypeRows::parse(&bytes));
    cache.push((root.to_owned(), rows.clone()));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_with_attributes_is_a_living_type() {
        let rows = EntityTypeRows::parse(
            br#"{"minecraft:oak_boat": {"width": 1.375},
                 "minecraft:zombie": {"width": 0.6, "attributes": {"minecraft:armor": 2.0}},
                 "minecraft:player": {"attributes": {}},
                 "ruby:golem": {"template": "minecraft:zombie", "attributes": {}}}"#,
        );
        assert_eq!(
            rows.ids().collect::<Vec<_>>(),
            [
                "minecraft:oak_boat",
                PLAYER,
                "minecraft:zombie",
                "ruby:golem"
            ]
        );
        assert!(rows.contains(PLAYER));
        assert!(rows.contains("minecraft:oak_boat"));
        assert!(!rows.contains("nope:ghost"));
        assert!(!rows.is_living("minecraft:oak_boat"));
        assert!(rows.is_living("minecraft:zombie"));
        assert!(rows.is_living("ruby:golem"));
    }
}
