//! Session-only, spatially indexed watered areas, invalidated on master-chunk unload or mod disable.
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, hash_map::Entry},
    hash::Hash,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static LEVELS: Mutex<Vec<Weak<Shared>>> = Mutex::new(Vec::new());

/// Opaque, non-reused identity for one watered-area ticket, issued only by [`WaterTickets::add`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WaterTicketId(u64);

#[derive(Clone, Copy)]
struct TileKey {
    depth: u8,
    coords: [u32; 2],
}

struct Ticket {
    owner: Option<&'static str>,
    min: [i32; 3],
    max: [i32; 3],
    tile: TileKey,
    master: (i32, i32),
}
#[derive(Default)]
struct Index {
    tickets: HashMap<WaterTicketId, Ticket>,
    depths: BTreeMap<u8, HashMap<[u32; 2], BTreeSet<WaterTicketId>>>,
    masters: HashMap<(i32, i32), BTreeSet<WaterTicketId>>,
}

impl Index {
    fn remove(&mut self, id: WaterTicketId) -> bool {
        let Some(ticket) = self.tickets.remove(&id) else {
            return false;
        };
        remove_from_bucket(&mut self.masters, ticket.master, id);
        if let std::collections::btree_map::Entry::Occupied(mut tiles) =
            self.depths.entry(ticket.tile.depth)
        {
            remove_from_bucket(tiles.get_mut(), ticket.tile.coords, id);
            if tiles.get().is_empty() {
                tiles.remove();
            }
        }
        true
    }
}

fn remove_from_bucket<K: Eq + Hash>(
    buckets: &mut HashMap<K, BTreeSet<WaterTicketId>>,
    key: K,
    id: WaterTicketId,
) {
    if let Entry::Occupied(mut bucket) = buckets.entry(key) {
        bucket.get_mut().remove(&id);
        if bucket.get().is_empty() {
            bucket.remove();
        }
    }
}

struct Shared {
    nonempty: AtomicBool,
    index: Mutex<Index>,
}

impl Shared {
    fn update_nonempty(&self, index: &Index) {
        self.nonempty
            .store(!index.tickets.is_empty(), Ordering::Release);
    }
}

/// Bias a signed chunk coordinate (`block >> 4`, in `-2^27..2^27`) into `0..2^28`.
fn biased_chunk(block: i32) -> u32 {
    ((block >> 4) as u32).wrapping_add(1u32 << 27)
}

/// A level's transient watered areas. Empty levels allocate nothing and queries only test the
/// empty-index guard; neither queries nor farmland ticks call mod code.
#[derive(Default)]
pub struct WaterTickets {
    shared: Option<Arc<Shared>>,
}

impl WaterTickets {
    /// Add a half-open integer box (`min <= pos < max`) and issue its opaque [`WaterTicketId`].
    /// Empty or reversed boxes are refused.
    /// Attribute the ticket to `owner` for disable cleanup, if supplied. Check `may_register`
    /// under the same lock as disable cleanup so a disabled owner cannot leave a ticket behind.
    pub fn add(
        &mut self,
        min: [i32; 3],
        max: [i32; 3],
        owner: Option<&'static str>,
        may_register: impl FnOnce() -> bool,
    ) -> Option<WaterTicketId> {
        if (0..3).any(|axis| min[axis] >= max[axis]) {
            return None;
        }
        let min_x = biased_chunk(min[0]);
        let max_x = biased_chunk(max[0] - 1);
        let min_z = biased_chunk(min[2]);
        let max_z = biased_chunk(max[2] - 1);
        let depth = (u32::BITS - ((min_x ^ max_x) | (min_z ^ max_z)).leading_zeros()) as u8;
        let tile = TileKey {
            depth,
            coords: [min_x >> depth, min_z >> depth],
        };
        // FarmlandWaterManager.addAABBTicket picks the chunk whose center is closest to the
        // box center. The chunk containing the center is nearest, including at an exact tie.
        let center_chunk =
            |axis: usize| ((i64::from(min[axis]) + i64::from(max[axis])) >> 5) as i32;
        let master = (center_chunk(0), center_chunk(2));
        let mut levels = LEVELS.lock().unwrap();
        if !may_register() {
            return None;
        }
        let id = NEXT_ID
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .ok()
            .map(WaterTicketId)?;
        let shared = self.shared.get_or_insert_with(|| {
            let shared = Arc::new(Shared {
                nonempty: AtomicBool::new(false),
                index: Mutex::new(Index::default()),
            });
            levels.retain(|level| level.strong_count() != 0);
            levels.push(Arc::downgrade(&shared));
            shared
        });
        let mut index = shared.index.lock().unwrap();
        index.tickets.insert(
            id,
            Ticket {
                owner,
                min,
                max,
                tile,
                master,
            },
        );
        index
            .depths
            .entry(tile.depth)
            .or_default()
            .entry(tile.coords)
            .or_default()
            .insert(id);
        index.masters.entry(master).or_default().insert(id);
        shared.nonempty.store(true, Ordering::Release);
        Some(id)
    }

    /// Remove the ticket with this issued identity; overlapping boxes remain independent.
    pub fn remove(&mut self, id: WaterTicketId) -> bool {
        let Some(shared) = &self.shared else {
            return false;
        };
        let mut index = shared.index.lock().unwrap();
        let removed = index.remove(id);
        shared.update_nonempty(&index);
        removed
    }

    /// Invalidate tickets whose master chunk left the level (`SimpleTicket.unload`). Merely
    /// overlapping the unloading chunk does not invalidate a ticket.
    pub fn unload_chunk(&mut self, chunk: (i32, i32)) {
        let Some(shared) = self
            .shared
            .as_ref()
            .filter(|shared| shared.nonempty.load(Ordering::Acquire))
        else {
            return;
        };
        let mut index = shared.index.lock().unwrap();
        if let Some(ids) = index.masters.remove(&chunk) {
            for id in ids {
                index.remove(id);
            }
            shared.update_nonempty(&index);
        }
    }

    /// Test only the position's tile buckets at populated depths, without allocating or invoking a
    /// mod callback.
    pub fn contains(&self, pos: [i32; 3]) -> bool {
        let Some(shared) = &self.shared else {
            return false;
        };
        if !shared.nonempty.load(Ordering::Acquire) {
            return false;
        }
        let px = biased_chunk(pos[0]);
        let pz = biased_chunk(pos[2]);
        let index = shared.index.lock().unwrap();
        index.depths.iter().any(|(&depth, tiles)| {
            tiles.get(&[px >> depth, pz >> depth]).is_some_and(|ids| {
                ids.iter().any(|id| {
                    let ticket = &index.tickets[id];
                    (0..3).all(|axis| ticket.min[axis] <= pos[axis] && pos[axis] < ticket.max[axis])
                })
            })
        })
    }

    /// Closing a level invalidates all its handles, even if the same level object is reused.
    pub fn clear(&mut self) {
        if let Some(shared) = self.shared.take() {
            shared.nonempty.store(false, Ordering::Release);
        }
    }
}

/// Called at a mod's disable boundary, not on a tick. Weak registrations do not keep worlds alive.
/// Releases only tickets attributed to `owner`; anonymous and other mods' tickets stay intact.
pub fn remove_owner(owner: &str) {
    LEVELS.lock().unwrap().retain(|level| {
        let Some(shared) = level.upgrade() else {
            return false;
        };
        let mut index = shared.index.lock().unwrap();
        let owned: Vec<_> = index
            .tickets
            .iter()
            .filter_map(|(&id, ticket)| (ticket.owner == Some(owner)).then_some(id))
            .collect();
        for id in owned {
            index.remove(id);
        }
        shared.update_nonempty(&index);
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_open_boxes_cross_negative_chunk_edges_and_remove_independently() {
        let mut level = WaterTickets::default();
        assert!(!level.contains([0, 64, 0]));
        let a = level
            .add([-17, 64, -1], [18, 65, 2], Some("irrigator"), || true)
            .unwrap();
        for pos in [[-17, 64, -1], [-16, 64, 0], [0, 64, 0], [17, 64, 1]] {
            assert!(level.contains(pos), "{pos:?}");
        }
        for pos in [
            [-18, 64, 0],
            [18, 64, 0],
            [0, 63, 0],
            [0, 65, 0],
            [0, 64, -2],
            [0, 64, 2],
        ] {
            assert!(!level.contains(pos), "{pos:?}");
        }
        let anon = level.add([0, 64, 0], [5, 65, 5], None, || true).unwrap();
        let b = level
            .add([2, 64, 2], [6, 65, 6], Some("neighbor"), || true)
            .unwrap();
        assert!(level.remove(a));
        assert!(!level.remove(a));
        assert!(!level.contains([-17, 64, 0]));
        assert!(level.contains([0, 64, 0]));
        assert!(level.contains([5, 64, 5]));
        // Unrelated owner cleanup leaves anonymous and separately attributed overlapping rows.
        remove_owner("irrigator");
        assert!(level.contains([0, 64, 0]));
        assert!(level.contains([5, 64, 5]));
        assert!(level.remove(anon));
        assert!(!level.remove(anon));
        assert!(!level.contains([0, 64, 0]));
        assert!(level.contains([2, 64, 2]));
        assert!(level.remove(b));
        assert!(!level.remove(b));
        assert!(!level.contains([2, 64, 2]));
    }

    #[test]
    fn owner_disable_releases_all_levels_without_resurrecting_old_handles() {
        let mut overworld = WaterTickets::default();
        let mut nether = WaterTickets::default();
        let old = overworld
            .add([0, 64, 0], [5, 65, 5], Some("water_test_owner"), || true)
            .unwrap();
        let anon = overworld
            .add([1, 64, 1], [2, 65, 2], None, || true)
            .unwrap();
        assert!(!nether.contains([0, 64, 0]));
        let nether_owned = nether
            .add([0, 64, 0], [5, 65, 5], Some("water_test_owner"), || true)
            .unwrap();
        let nether_other = nether
            .add([3, 64, 3], [4, 65, 4], Some("water_test_other"), || true)
            .unwrap();
        remove_owner("water_test_owner");
        assert!(!overworld.contains([0, 64, 0]));
        assert!(overworld.contains([1, 64, 1]));
        assert!(!nether.contains([0, 64, 0]));
        assert!(nether.contains([3, 64, 3]));
        assert!(!overworld.remove(old));
        assert!(!nether.remove(nether_owned));
        // A genuinely unattributed call can add a ticket without reviving old keys.
        let unattributed = overworld
            .add([4, 64, 4], [5, 65, 5], None, || true)
            .unwrap();
        assert_ne!(old, unattributed);
        assert!(overworld.contains([4, 64, 4]));
        assert!(!overworld.contains([0, 64, 0]));
        assert!(!overworld.remove(old));
        let new = overworld
            .add([0, 64, 0], [5, 65, 5], Some("water_test_owner"), || true)
            .unwrap();
        assert_ne!(old, new);
        assert!(!overworld.remove(old));
        assert!(overworld.contains([0, 64, 0]));
        assert!(overworld.remove(anon));
        assert!(overworld.remove(unattributed));
        overworld.clear();
        assert!(!overworld.contains([0, 64, 0]));
        assert!(!overworld.remove(new));
        assert!(nether.remove(nether_other));
        assert!(!nether.contains([3, 64, 3]));
        let reopened = WaterTickets::default();
        assert!(!reopened.contains([0, 64, 0]));
    }

    #[test]
    fn wide_boxes_index_past_the_old_threshold_and_preserve_overlapping_narrow_tickets() {
        let mut level = WaterTickets::default();
        // 257 x 256 chunk columns: past the old 65,536-column threshold.
        let past_old_cap = level
            .add([0, 64, 0], [4097, 65, 4096], Some("wide"), || true)
            .unwrap();
        assert!(level.contains([0, 64, 0]));
        assert!(level.contains([4096, 64, 4095]));
        assert!(!level.contains([4097, 64, 4095]));
        assert!(!level.contains([4096, 64, 4096]));
        assert!(!level.contains([-1, 64, 0]));

        let narrow = level
            .add([10, 64, 10], [15, 65, 15], Some("sprinkler"), || true)
            .unwrap();
        let near_full = level
            .add(
                [i32::MIN, 64, i32::MIN],
                [i32::MAX, 65, i32::MAX],
                Some("wide"),
                || true,
            )
            .unwrap();
        for pos in [
            [i32::MIN, 64, i32::MIN],
            [-1_000_000, 64, -17],
            [12, 64, 12],
            [1_000_000, 64, 500_000],
            [i32::MAX - 1, 64, i32::MAX - 1],
        ] {
            assert!(level.contains(pos), "{pos:?}");
        }
        for pos in [[i32::MAX, 64, 0], [0, 64, i32::MAX], [0, 63, 0], [0, 65, 0]] {
            assert!(!level.contains(pos), "{pos:?}");
        }

        assert!(level.remove(near_full));
        assert!(!level.remove(near_full));
        assert!(!level.contains([-1_000_000, 64, -17]));
        assert!(!level.contains([i32::MAX - 1, 64, i32::MAX - 1]));
        assert!(level.contains([4096, 64, 4095]));
        assert!(level.contains([12, 64, 12]));

        assert!(level.remove(past_old_cap));
        assert!(!level.contains([4096, 64, 4095]));
        assert!(!level.contains([0, 64, 0]));
        assert!(level.contains([10, 64, 10]));
        assert!(level.contains([14, 64, 14]));
        assert!(!level.contains([15, 64, 10]));
        assert!(!level.contains([10, 64, 15]));
        {
            let index = level.shared.as_ref().unwrap().index.lock().unwrap();
            assert_eq!(index.depths.len(), 1);
            assert!(index.depths.contains_key(&0));
        }

        assert!(level.remove(narrow));
        assert!(!level.contains([12, 64, 12]));
        let shared = level.shared.as_ref().unwrap();
        assert!(!shared.nonempty.load(Ordering::Acquire));
        assert!(shared.index.lock().unwrap().depths.is_empty());
    }

    #[test]
    fn invalid_boxes_do_not_create_an_index() {
        let mut level = WaterTickets::default();
        for (min, max) in [
            ([0, 64, 0], [0, 65, 5]),
            ([5, 64, 0], [0, 65, 5]),
            ([0, 64, 0], [5, 64, 5]),
            ([0, 64, 5], [5, 65, 0]),
            ([i32::MIN, 0, 0], [i32::MIN, 1, 1]),
            ([0, 0, 0], [i32::MIN, 1, 1]),
        ] {
            assert!(
                level.add(min, max, Some("invalid"), || true).is_none(),
                "{min:?}..{max:?}"
            );
        }
        assert!(level.shared.is_none());
    }
}
