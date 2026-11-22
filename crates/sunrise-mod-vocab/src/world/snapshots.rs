//! Owned placement and loot data; none of these snapshots borrows an engine context.
use crate::{BlockPos, Direction, ItemKey, PlayerRef};

/// Inputs to selecting a block's placement state.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
#[derive(sunrise_mod_api_macros::Wire)]
pub struct PlacementContext {
    /// Resolved destination of the block being placed.
    pub pos: BlockPos,
    /// Position of the clicked block.
    pub hit_pos: BlockPos,
    /// Clicked face.
    pub face: Direction,
    /// World-space click location.
    pub hit: [f64; 3],
    /// Whether the click originated inside the block's shape.
    pub inside: bool,
    /// Placer yaw in degrees.
    pub yaw: f32,
    /// Placer pitch in degrees.
    pub pitch: f32,
    /// Whether secondary use is active.
    pub sneaking: bool,
    /// The resolved item in the placing hand; `None` for an empty hand.
    pub item: Option<ItemKey>,
    /// Whether the clicked block itself is replaced.
    pub replace_clicked: bool,
}

impl PlacementContext {
    /// A placement at `pos`, with a click on `face` and an optional held item.
    /// The click starts at the block origin, with zero view angles and no flags set.
    pub fn new(pos: BlockPos, face: Direction, item: Option<ItemKey>) -> Self {
        Self {
            pos,
            hit_pos: pos,
            face,
            hit: [f64::from(pos.x), f64::from(pos.y), f64::from(pos.z)],
            inside: false,
            yaw: 0.0,
            pitch: 0.0,
            sneaking: false,
            item,
            replace_clicked: false,
        }
    }
}

/// Vanilla's `BlockPlaceContext.getPlayer()` and the placer's eyes at the time of the click.
/// The engine reads the acting or locally predicted player's eyes before borrowing the level.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct PlacementPlacer {
    player: PlayerRef,
    eye_position: [f64; 3],
}

impl PlacementPlacer {
    // hidden: only the engine constructs the snapshot of the acting player.
    #[doc(hidden)]
    pub fn new(player: PlayerRef, eye_position: [f64; 3]) -> Self {
        Self {
            player,
            eye_position,
        }
    }

    /// The placing player on this logical side.
    pub fn player(self) -> PlayerRef {
        self.player
    }

    /// World-space eye position when this placement was attempted.
    pub fn eye_position(self) -> [f64; 3] {
        self.eye_position
    }
}

/// A loot function's intermediate item candidate, before inventory materialization.
/// Counts can be non-positive while loot functions run; unlike `ItemStack`, this
/// is a roll's intermediate data and does not represent an occupied inventory slot.
// no wire: in-process loot candidate snapshot for explicit copy and replacement.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct LootStack {
    /// Resolved candidate item.
    pub item: ItemKey,
    /// Intermediate count; a non-positive result produces no item.
    pub count: i32,
    /// Intermediate damage value.
    pub damage: i32,
    /// Enchantment identifiers and levels in loot-function order.
    pub enchantments: Vec<(String, i32)>,
    /// Potion identifier.
    pub potion: Option<String>,
    /// Whether to create the map ID component after the roll.
    pub map: bool,
    /// A deferred exploration-map search.
    pub exploration: Option<Box<ExplorationMap>>,
    /// Ominous bottle amplifier.
    pub ominous_amplifier: Option<i32>,
    /// Suspicious stew effect identifiers and durations in ticks.
    pub stew_effects: Vec<(String, i32)>,
    /// Custom or item name components, still unresolved styled trees.
    pub names: Vec<(String, crate::text::Text)>,
    /// Ordered component patches; `None` removes a component.
    pub components: Vec<(String, Option<sunrise_core::nbt::Tag>)>,
}

/// A deferred exploration-map search requested by a loot function.
// no wire: in-process deferred exploration-map request.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExplorationMap {
    /// Destination structure tag, without `#`.
    pub destination: String,
    /// Map decoration identifier.
    pub decoration: String,
    /// Map zoom.
    pub zoom: u8,
    /// Search radius in chunks.
    pub search_radius: i32,
    /// Whether to skip previously discovered structures.
    pub skip_existing_chunks: bool,
}

impl LootStack {
    /// A candidate with signed count, before inventory materialization.
    pub fn new(item: ItemKey, count: i32) -> Self {
        Self {
            item,
            count,
            damage: 0,
            enchantments: Vec::new(),
            potion: None,
            map: false,
            exploration: None,
            ominous_amplifier: None,
            stew_effects: Vec::new(),
            names: Vec::new(),
            components: Vec::new(),
        }
    }
}
impl ExplorationMap {
    /// A deferred vanilla exploration-map request, without running its search.
    pub fn new(
        destination: String,
        decoration: String,
        zoom: u8,
        search_radius: i32,
        skip_existing_chunks: bool,
    ) -> Self {
        Self {
            destination,
            decoration,
            zoom,
            search_radius,
            skip_existing_chunks,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sunrise_core::nbt::Tag;

    #[test]
    fn loot_snapshot_retains_unknown_patch_removal_and_order() {
        let patches = vec![
            ("test:unknown".to_owned(), Some(Tag::Int(42))),
            ("minecraft:damage".to_owned(), None),
            ("test:unknown".to_owned(), Some(Tag::String("later".into()))),
        ];
        let stack = LootStack {
            item: ItemKey(0),
            count: -1,
            damage: 0,
            enchantments: vec![],
            potion: None,
            map: false,
            exploration: None,
            ominous_amplifier: None,
            stew_effects: vec![],
            names: vec![],
            components: patches,
        };
        assert_eq!(
            stack.components[0],
            ("test:unknown".into(), Some(Tag::Int(42)))
        );
        assert_eq!(stack.components[1], ("minecraft:damage".into(), None));
        assert_eq!(
            stack.components[2],
            ("test:unknown".into(), Some(Tag::String("later".into())))
        );
    }
}
