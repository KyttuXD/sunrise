//! Mod feature types through the engine: decoding, the caller's random, the placement chain's
//! order, nesting, direct region writes, error preservation and block entities.
use super::*;
use crate::worldgen::anchor::WorldGenerationContext;
use crate::worldgen::feature::{FeatureBlocks, FeatureWorld};
use crate::worldgen::feature_graph::{FeatureDefinition, PlacedFeatureDefinition};
use crate::worldgen::placement::{PlacementContext, PlacementModifier, PlacementQueryError};
use crate::worldgen::value_provider::IntProvider;
use std::collections::HashMap;
use std::sync::Mutex;
use sunrise_core::nbt::Tag;
use sunrise_core::random::{LegacyRandom, WorldgenRandom};

/// A world 64 blocks wide and 16 tall (y 0..16): reads inside it, writes inside its write zone
/// (x and z 0..48), every write and block entity kept in order. State 1 is in
/// `minecraft:dirt`; so is anything at y 0.
#[derive(Default)]
struct World {
    writes: Vec<([i32; 3], BlockStateId, i32)>,
    latest: HashMap<[i32; 3], BlockStateId>,
    entities: Vec<([i32; 3], Compound)>,
    full_chunks: Vec<[i32; 2]>,
}
impl FeatureWorld for World {
    type Error = PlacementQueryError;
    fn block(&mut self, p: [i32; 3]) -> Result<BlockStateId, Self::Error> {
        if !(0..16).contains(&p[1]) || !(0..64).contains(&p[0]) || !(0..64).contains(&p[2]) {
            return Err(PlacementQueryError::Unavailable);
        }
        Ok(self.latest.get(&p).copied().unwrap_or(BlockStateId(0)))
    }
    fn set_block(&mut self, p: [i32; 3], s: BlockStateId, flags: i32) -> Result<bool, Self::Error> {
        if !self.ensure_can_write(p)? || !self.inside_build_height(p)? {
            return Ok(false);
        }
        let current = self.block(p)?;
        if self.full_chunks.contains(&[p[0] >> 4, p[2] >> 4]) {
            return Ok(true);
        }
        if current == s {
            return Ok(false);
        }
        self.writes.push((p, s, flags));
        self.latest.insert(p, s);
        Ok(true)
    }
}
impl PlacementContext for World {
    fn generation(&self) -> WorldGenerationContext {
        WorldGenerationContext::new(0, 16, 8)
    }
    fn min_y(&mut self) -> Result<i32, PlacementQueryError> {
        Ok(0)
    }
    fn max_y(&mut self) -> Result<i32, PlacementQueryError> {
        Ok(15)
    }
    fn height(&mut self, _: HeightmapType, x: i32, z: i32) -> Result<i32, PlacementQueryError> {
        if !(0..64).contains(&x) || !(0..64).contains(&z) {
            return Err(PlacementQueryError::Unavailable);
        }
        let top = (0..16)
            .rev()
            .find(|&y| {
                self.latest
                    .get(&[x, y, z])
                    .is_some_and(|&state| state != BlockStateId(0))
            })
            .map_or(0, |y| y + 1);
        Ok(top)
    }
    fn ensure_can_write(&mut self, p: [i32; 3]) -> Result<bool, PlacementQueryError> {
        Ok((0..48).contains(&p[0]) && (0..48).contains(&p[2]))
    }
    fn full_chunk_at(&mut self, p: [i32; 3]) -> Result<bool, PlacementQueryError> {
        Ok(self.full_chunks.contains(&[p[0] >> 4, p[2] >> 4]))
    }
    fn block_matches_tag(&mut self, p: [i32; 3], tag: &str) -> Result<bool, PlacementQueryError> {
        let state = self.block(p)?;
        Ok(tag == "minecraft:dirt" && (state == BlockStateId(1) || p[1] == 0))
    }
    fn set_block_entity(&mut self, p: [i32; 3], nbt: Compound) -> Result<(), PlacementQueryError> {
        self.entities.push((p, nbt));
        Ok(())
    }
}
fn blocks() -> FeatureBlocks {
    FeatureBlocks {
        air: BlockStateId(0),
        stone: BlockStateId(1),
        cobblestone: BlockStateId(2),
        end_stone: BlockStateId(3),
        obsidian: BlockStateId(4),
    }
}

#[derive(Debug)]
struct InnerCause;
impl std::fmt::Display for InnerCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("inner detail")
    }
}
impl std::error::Error for InnerCause {}

#[derive(Debug)]
struct OuterCause(InnerCause);
impl std::fmt::Display for OuterCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("second call")
    }
}
impl std::error::Error for OuterCause {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

/// What a test type does when placed:
/// - `Marks` draws `nextInt(1)`, records the origin and the draw, and writes state 1 there;
/// - `Column` writes its config's height of state 1 upward, then answers false;
/// - `Fails` writes state 2 and chest NBT at `origin`, then returns `Err`;
/// - `ManyReads` performs 70,000 valid reads (> 65,536), then writes state 2 and answers `Ok(true)`;
/// - `Refuses` writes at `origin`, tries an out-of-region write (verifying `ensure_can_write` and
///   `set_block` both return `false`), then writes above `origin`;
/// - `Fill` writes 8,000 blocks with block entities, then reads 8,000 positions it never wrote;
/// - `SecondFails` draws `nextInt(1)`, writes state 5 and chest NBT at `origin`, and returns `Err`
///   on its second call without skipping the third;
/// - `Chest` writes state 5 at `origin` and sets block entities at `origin`, a writable neighbor
///   it did not write, an out-of-zone position, an out-of-height position, an extreme coordinate,
///   and a position in chunk `[1, 0]`.
#[derive(Clone, Copy)]
enum Kind {
    Marks,
    Column,
    Fails,
    ManyReads,
    Chest,
    SecondFails,
    Fill,
    Refuses,
}
type Log = Arc<Mutex<Vec<([i32; 3], i32)>>>;
struct Test(Kind, Log);
impl ModFeatureType for Test {
    fn prepare_config(&self, config: &serde_json::Value) -> Result<ModFeatureConfig, String> {
        match config.get("height") {
            None => Ok(Arc::new(1_i32)),
            Some(serde_json::Value::Number(n))
                if n.as_i64().is_some_and(|n| (1..=8).contains(&n)) =>
            {
                Ok(Arc::new(n.as_i64().unwrap_or(1) as i32))
            }
            Some(_) => Err("height must be 1 to 8".into()),
        }
    }
    fn place(
        &self,
        target: &mut dyn ModFeatureTarget,
        random: &mut dyn ModFeatureRandom,
        origin: [i32; 3],
        config: &(dyn Any + Send + Sync),
    ) -> Result<bool, FeatureError> {
        let [x, y, z] = origin;
        match self.0 {
            Kind::Marks => {
                let draw = random.int(1);
                self.1.lock().unwrap().push((origin, draw));
                Ok(target.set_block(origin, BlockStateId(1), 3))
            }
            Kind::Column => {
                let height = *config
                    .downcast_ref::<i32>()
                    .ok_or_else(|| FeatureError::new(std::io::Error::other("config")))?;
                for dy in 0..height {
                    target.set_block([x, y + dy, z], BlockStateId(1), 2);
                }
                Ok(false)
            }
            Kind::Fails => {
                target.set_block(origin, BlockStateId(2), 3);
                let chest = Compound::from([("id".into(), Tag::String("minecraft:chest".into()))]);
                target.set_block_entity(origin, chest);
                Err(FeatureError::new(std::io::Error::other("no room")))
            }
            Kind::ManyReads => {
                let mut readable = 0;
                for _ in 0..70_000 {
                    if target.block(origin).is_some() {
                        readable += 1;
                    }
                }
                self.1.lock().unwrap().push((origin, readable));
                Ok(target.set_block(origin, BlockStateId(2), 3))
            }
            Kind::Refuses => {
                let first = target.set_block(origin, BlockStateId(1), 3);
                let far = [x + 100, y, z];
                let can_far = target.ensure_can_write(far);
                let wrote_far = target.set_block(far, BlockStateId(1), 3);
                let after = target.set_block([x, y + 1, z], BlockStateId(1), 3);
                self.1.lock().unwrap().push((
                    origin,
                    i32::from(first) * 8
                        + i32::from(can_far) * 4
                        + i32::from(wrote_far) * 2
                        + i32::from(after),
                ));
                Ok(first && after)
            }
            Kind::Fill => {
                let chest = Compound::from([("id".into(), Tag::String("minecraft:chest".into()))]);
                let at = |i: i32| [i % 48, 1 + i / 2304, (i / 48) % 48];
                for i in 0..8_000 {
                    target.set_block(at(i), BlockStateId(5), 3);
                    target.set_block_entity(at(i), chest.clone());
                }
                let missed = (8_000..16_000)
                    .filter(|&i| target.block(at(i)) == Some(BlockStateId(0)))
                    .count();
                self.1.lock().unwrap().push((origin, missed as i32));
                Ok(true)
            }
            Kind::SecondFails => {
                let draw = random.int(1);
                let calls = {
                    let mut log = self.1.lock().unwrap();
                    log.push((origin, draw));
                    log.len()
                };
                let placed = target.set_block(origin, BlockStateId(5), 3);
                let chest = Compound::from([("id".into(), Tag::String("minecraft:chest".into()))]);
                target.set_block_entity(origin, chest);
                if calls == 2 {
                    return Err(FeatureError::new(OuterCause(InnerCause)));
                }
                Ok(placed)
            }
            Kind::Chest => {
                let nbt = Compound::from([("id".into(), Tag::String("minecraft:chest".into()))]);
                let placed = target.set_block(origin, BlockStateId(5), 3);
                let own = target.set_block_entity(origin, nbt.clone());
                let neighbor = target.set_block_entity([x + 1, y, z], nbt.clone());
                let out_zone = target.set_block_entity([50, y, z], nbt.clone());
                let out_height = target.set_block_entity([x, 20, z], nbt.clone());
                let extreme = target.set_block_entity([i32::MAX, i32::MIN, i32::MAX], nbt.clone());
                let full_chunk = target.set_block_entity([16, y, z], nbt);
                self.1.lock().unwrap().push((
                    origin,
                    i32::from(placed) * 32
                        + i32::from(own) * 16
                        + i32::from(neighbor) * 8
                        + i32::from(out_zone) * 4
                        + i32::from(out_height) * 2
                        + i32::from(extreme || full_chunk),
                ));
                Ok(placed)
            }
        }
    }
}
fn types(log: &Log) -> ModFeatureTypes {
    [
        ("test:marks", Kind::Marks),
        ("test:column", Kind::Column),
        ("test:fails", Kind::Fails),
        ("test:many_reads", Kind::ManyReads),
        ("test:chest", Kind::Chest),
        ("test:fill", Kind::Fill),
        ("test:refuses", Kind::Refuses),
        ("test:second_fails", Kind::SecondFails),
    ]
    .into_iter()
    .map(|(id, kind)| {
        let ty: Arc<dyn ModFeatureType> = Arc::new(Test(kind, log.clone()));
        (id.to_owned(), Arc::new(ModFeatureEntry::new("test", ty)))
    })
    .collect()
}
fn feature(json: &str) -> FeatureDefinition {
    FeatureDefinition::from_json(json).unwrap()
}
fn place(feature: &FeatureDefinition, world: &mut World, origin: [i32; 3]) -> Result<bool, String> {
    feature
        .place(world, &mut LegacyRandom::new(1), origin, blocks())
        .map_err(|error| error.to_string())
}

/// A registered type's id decodes to the mod's feature, with its config decoded once and its
/// registering mod id retained; a config its type refuses refuses the definition at load, naming
/// the type; without the type the id is the unknown type it was before.
#[test]
fn a_registered_type_decodes_and_a_bad_config_is_refused_at_load() {
    let log = Log::default();
    with_test_feature_types(&types(&log), || {
        let FeatureDefinition::Mod(column) = feature(r#"{"type":"test:column","height":3}"#) else {
            panic!("not a mod feature")
        };
        assert_eq!(column.id, "test:column");
        assert_eq!(column.entry.mod_id, "test");
        assert_eq!(column.config.downcast_ref::<i32>(), Some(&3));
        let error = FeatureDefinition::from_json(r#"{"type":"test:column","height":99}"#)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("test:column") && error.contains("1 to 8"),
            "{error}"
        );
        assert!(FeatureDefinition::from_json(r#"{"type":"test:missing"}"#).is_err());
    });
    assert!(FeatureDefinition::from_json(r#"{"type":"test:column"}"#).is_err());
}

/// The feature draws from the caller's own random: its `nextInt(1)` is 0 and spends one
/// draw, so the caller's next draws are the jar's (`oracle_tests`: 13, 3, then
/// -7261648964369397258 from seed 0).
#[test]
fn the_feature_draws_from_the_callers_stream() {
    let log = Log::default();
    with_test_feature_types(&types(&log), || {
        let marks = feature(r#"{"type":"test:marks"}"#);
        let mut random = WorldgenRandom::new(LegacyRandom::new(0));
        let mut world = World::default();
        assert!(
            marks
                .place(&mut world, &mut random, [0, 4, 0], blocks())
                .unwrap()
        );
        assert_eq!(*log.lock().unwrap(), [([0, 4, 0], 0)]);
        assert_eq!(world.writes, [([0, 4, 0], BlockStateId(1), 3)]);
        assert_eq!(random.next_int_bound(16).unwrap(), 13);
        assert_eq!(random.next_int_bound(16).unwrap(), 3);
        assert_eq!(random.next_long(), -7261648964369397258);
    });
}

/// Through `count 2` then `in_square`, depth first as `FeaturePlacer.place` runs them: the
/// feature runs at the first position before `in_square` draws for the second. The expected
/// values are `java.util.Random`'s from seed 42 (the LCG vanilla's `LegacyRandomSource` is),
/// worked out apart from Sunrise: in_square draws 11, 0, the feature's `nextInt(1)` is 0,
/// in_square draws 0, 4, the feature 0, and the next `nextLong` is 5111195811822994797.
#[test]
fn modifiers_feed_the_feature_in_vanilla_order() {
    let log = Log::default();
    with_test_feature_types(&types(&log), || {
        let placed = PlacedFeatureDefinition {
            feature: Box::new(feature(r#"{"type":"test:marks"}"#)),
            placement: vec![
                PlacementModifier::Count(IntProvider::Constant(2)),
                PlacementModifier::InSquare,
            ],
        };
        let mut random = WorldgenRandom::new(LegacyRandom::new(42));
        let mut world = World::default();
        assert!(
            placed
                .place(&mut world, &mut random, [16, 4, 32], blocks())
                .unwrap()
        );
        assert_eq!(*log.lock().unwrap(), [([27, 4, 32], 0), ([16, 4, 36], 0)]);
        assert_eq!(random.next_long(), 5111195811822994797);
    });
}

/// A position given twice is placed twice: the second call finds its own block already there,
/// so its write answers false, as `Level.setBlock` does for an unchanged state.
#[test]
fn a_duplicate_final_position_is_placed_twice() {
    let log = Log::default();
    with_test_feature_types(&types(&log), || {
        let placed = PlacedFeatureDefinition {
            feature: Box::new(feature(r#"{"type":"test:marks"}"#)),
            placement: vec![PlacementModifier::Count(IntProvider::Constant(2))],
        };
        let mut world = World::default();
        // The first call placed, so the placement did.
        assert!(
            placed
                .place(&mut world, &mut LegacyRandom::new(1), [3, 4, 3], blocks())
                .unwrap()
        );
        assert_eq!(*log.lock().unwrap(), [([3, 4, 3], 0), ([3, 4, 3], 0)]);
        assert_eq!(world.writes, [([3, 4, 3], BlockStateId(1), 3)]);
    });
}

/// A nested feature (a random selector's default) reaches the mod's type, and `Ok(false)` keeps
/// what it wrote, as vanilla does.
#[test]
fn a_nested_feature_reaches_the_type_and_false_keeps_its_writes() {
    let log = Log::default();
    with_test_feature_types(&types(&log), || {
        let selector = feature(
            r#"{"type":"minecraft:random_selector","features":[],
                "default":{"feature":{"type":"test:column","height":4},"placement":[]}}"#,
        );
        let mut world = World::default();
        assert_eq!(place(&selector, &mut world, [3, 12, 3]), Ok(false));
        assert_eq!(
            world.writes.iter().map(|w| w.0).collect::<Vec<_>>(),
            [[3, 12, 3], [3, 13, 3], [3, 14, 3], [3, 15, 3]]
        );
    });
}

/// A write the region refuses (outside the write zone or build height) returns `false` as an
/// ordinary operation result: earlier writes stand, valid writes after it succeed, and later
/// invocations of the same type stay alive.
#[test]
fn refused_write_leaves_earlier_writes_and_later_calls_alive() {
    let log = Log::default();
    with_test_feature_types(&types(&log), || {
        let refuses = feature(r#"{"type":"test:refuses"}"#);
        let mut world = World::default();
        assert_eq!(place(&refuses, &mut world, [3, 1, 3]), Ok(true));
        // first (8) + !can_far (0) + !wrote_far (0) + after (1) = 9
        assert_eq!(*log.lock().unwrap(), [([3, 1, 3], 9)]);
        assert_eq!(
            world.writes.iter().map(|w| w.0).collect::<Vec<_>>(),
            [[3, 1, 3], [3, 2, 3]]
        );
        // Later invocation of the same type is still alive and places normally.
        assert_eq!(place(&refuses, &mut world, [4, 1, 4]), Ok(true));
        assert_eq!(
            world.writes.iter().map(|w| w.0).collect::<Vec<_>>(),
            [[3, 1, 3], [3, 2, 3], [4, 1, 4], [4, 2, 4]]
        );

        // Out-of-build-height writes inside a column stop at the height limit without poisoning.
        let column = feature(r#"{"type":"test:column","height":4}"#);
        let mut tall = World::default();
        assert_eq!(place(&column, &mut tall, [3, 14, 3]), Ok(false));
        assert_eq!(
            tall.writes.iter().map(|w| w.0).collect::<Vec<_>>(),
            [[3, 14, 3], [3, 15, 3]]
        );
        assert_eq!(place(&column, &mut tall, [5, 10, 5]), Ok(false));
        assert_eq!(
            tall.writes.iter().map(|w| w.0).collect::<Vec<_>>(),
            [
                [3, 14, 3],
                [3, 15, 3],
                [5, 10, 5],
                [5, 11, 5],
                [5, 12, 5],
                [5, 13, 5],
            ]
        );
    });
}

/// A returned `Err` in a count or nested root preserves valid blocks and block entities already
/// written, keeps the caller's RNG stream identical, and does not skip following calls.
#[test]
fn returned_err_preserves_writes_nbt_and_rng_and_continues_root() {
    let rooted = |ty: &str| PlacedFeatureDefinition {
        feature: Box::new(feature(&format!(r#"{{"type":"{ty}"}}"#))),
        placement: vec![
            PlacementModifier::Count(IntProvider::Constant(3)),
            PlacementModifier::InSquare,
        ],
    };

    let fail_log = Log::default();
    let mut fail_world = World::default();
    let mut fail_random = WorldgenRandom::new(LegacyRandom::new(42));
    let fail_answer = with_test_feature_types(&types(&fail_log), || {
        rooted("test:second_fails")
            .place(&mut fail_world, &mut fail_random, [16, 4, 32], blocks())
            .map_err(|error| error.to_string())
    });

    let marks_log = Log::default();
    let mut marks_world = World::default();
    let mut marks_random = WorldgenRandom::new(LegacyRandom::new(42));
    let marks_answer = with_test_feature_types(&types(&marks_log), || {
        rooted("test:marks")
            .place(&mut marks_world, &mut marks_random, [16, 4, 32], blocks())
            .map_err(|error| error.to_string())
    });

    assert_eq!(fail_answer, Ok(true));
    assert_eq!(marks_answer, Ok(true));
    let fail_calls = fail_log.lock().unwrap().clone();
    let marks_calls = marks_log.lock().unwrap().clone();
    assert_eq!(fail_calls.len(), 3, "the third call still runs after Err");
    assert_eq!(fail_calls, marks_calls);
    assert_eq!(
        fail_world.writes.iter().map(|w| w.0).collect::<Vec<_>>(),
        marks_world.writes.iter().map(|w| w.0).collect::<Vec<_>>()
    );
    assert_eq!(
        fail_world.entities.iter().map(|e| e.0).collect::<Vec<_>>(),
        marks_world.writes.iter().map(|w| w.0).collect::<Vec<_>>()
    );
    assert_eq!(fail_random.next_long(), marks_random.next_long());

    // Nested selector and sequence: valid writes before `Err` stay, and later calls still run.
    let nested_log = Log::default();
    with_test_feature_types(&types(&nested_log), || {
        let nested = feature(
            r#"{"type":"minecraft:random_selector","features":[],
                "default":{"feature":{"type":"test:fails"},"placement":[]}}"#,
        );
        let mut world = World::default();
        assert_eq!(place(&nested, &mut world, [1, 1, 1]), Ok(false));
        assert_eq!(place(&nested, &mut world, [2, 1, 2]), Ok(false));
        assert_eq!(
            world.writes,
            [
                ([1, 1, 1], BlockStateId(2), 3),
                ([2, 1, 2], BlockStateId(2), 3),
            ]
        );
        assert_eq!(
            world.entities.iter().map(|e| e.0).collect::<Vec<_>>(),
            [[1, 1, 1], [2, 1, 2]]
        );

        let root = feature(
            r#"{"type":"minecraft:sequence","features":[
                {"feature":{"type":"test:marks"},"placement":[]},
                {"feature":{"type":"minecraft:end_platform"},"placement":[]},
                {"feature":{"type":"test:fails"},"placement":[]}]}"#,
        );
        let origin = [10, 4, 10];
        let mut seq_world = World::default();
        assert_eq!(place(&root, &mut seq_world, origin), Ok(false));
        let at_origin: Vec<_> = seq_world
            .writes
            .iter()
            .filter(|w| w.0 == origin)
            .map(|w| w.1)
            .collect();
        assert_eq!(
            at_origin,
            [BlockStateId(1), BlockStateId(0), BlockStateId(2)]
        );
        assert_eq!(seq_world.block(origin), Ok(BlockStateId(2)));
        assert_eq!(
            seq_world
                .writes
                .iter()
                .filter(|w| w.1 == BlockStateId(4))
                .count(),
            25
        );
        assert_eq!(
            seq_world.entities.iter().map(|e| e.0).collect::<Vec<_>>(),
            [origin]
        );
    });
}

/// A callback succeeds after more than 65,536 valid reads: there is no invented custom work quota.
#[test]
fn callback_succeeds_after_more_than_65536_valid_reads() {
    let log = Log::default();
    with_test_feature_types(&types(&log), || {
        let many_reads = feature(r#"{"type":"test:many_reads"}"#);
        let mut world = World::default();
        assert_eq!(place(&many_reads, &mut world, [0, 1, 0]), Ok(true));
        assert_eq!(*log.lock().unwrap(), [([0, 1, 0], 70_000)]);
        assert_eq!(world.writes, [([0, 1, 0], BlockStateId(2), 3)]);
    });
}

/// A call that writes thousands of blocks with block entities, then reads thousands of other
/// positions, writes them directly and reads back from the world.
#[test]
fn a_root_of_thousands_of_writes_publishes_them_all() {
    let log = Log::default();
    with_test_feature_types(&types(&log), || {
        let mut world = World::default();
        assert_eq!(
            place(&feature(r#"{"type":"test:fill"}"#), &mut world, [0, 1, 0]),
            Ok(true)
        );
        assert_eq!(*log.lock().unwrap(), [([0, 1, 0], 8_000)]);
        assert_eq!(world.writes.len(), 8_000);
        assert_eq!(world.entities.len(), 8_000);
    });
}

/// Direct writes answer as the world does, and immediate block, tag and heightmap reads see the
/// just-written world.
#[test]
fn direct_writes_are_immediately_visible_to_reads_tags_and_heightmaps() {
    struct Probe;
    impl ModFeatureType for Probe {
        fn prepare_config(&self, _: &serde_json::Value) -> Result<ModFeatureConfig, String> {
            Ok(Arc::new(()))
        }
        fn place(
            &self,
            t: &mut dyn ModFeatureTarget,
            _: &mut dyn ModFeatureRandom,
            o: [i32; 3],
            _: &(dyn Any + Send + Sync),
        ) -> Result<bool, FeatureError> {
            let answers = [
                t.ensure_can_write(o),
                !t.ensure_can_write([50, o[1], o[2]]),
                t.block_in_tag(o, "minecraft:dirt") == Some(false),
                t.height(HeightmapType::WorldSurfaceWg, o[0], o[2]) == Some(0),
                t.set_block(o, BlockStateId(1), 3),
                t.block(o) == Some(BlockStateId(1)),
                t.block_in_tag(o, "minecraft:dirt") == Some(true),
                t.height(HeightmapType::WorldSurfaceWg, o[0], o[2]) == Some(o[1] + 1),
                !t.set_block(o, BlockStateId(1), 3),
                t.block([i32::MAX, 5, 5]).is_none(),
            ];
            Ok(answers.iter().all(|a| *a))
        }
    }
    let types: ModFeatureTypes = [(
        "test:probe".to_owned(),
        Arc::new(ModFeatureEntry::new("test", Arc::new(Probe))),
    )]
    .into();
    with_test_feature_types(&types, || {
        let mut world = World::default();
        assert_eq!(
            place(&feature(r#"{"type":"test:probe"}"#), &mut world, [5, 7, 5]),
            Ok(true)
        );
        assert_eq!(world.writes, [([5, 7, 5], BlockStateId(1), 3)]);
    });
}

/// Block-entity writes follow actual write admissibility, build height and full-chunk imposter
/// policy without requiring this call to have written the block first.
#[test]
fn block_entities_follow_admissibility_and_full_chunk_policy() {
    let log = Log::default();
    with_test_feature_types(&types(&log), || {
        let chest = feature(r#"{"type":"test:chest"}"#);
        let mut world = World {
            full_chunks: vec![[1, 0]],
            ..Default::default()
        };
        assert_eq!(place(&chest, &mut world, [2, 2, 2]), Ok(true));
        // placed (32) + own (16) + writable neighbor not written by this call (8) = 56;
        // out_zone, out_height, extreme, and full_chunk [16, 2, 2] all refused (0).
        assert_eq!(*log.lock().unwrap(), [([2, 2, 2], 56)]);
        assert_eq!(world.writes, [([2, 2, 2], BlockStateId(5), 3)]);
        assert_eq!(
            world.entities.iter().map(|e| e.0).collect::<Vec<_>>(),
            [[2, 2, 2], [3, 2, 2]]
        );
    });
}

/// The random adapter: a bound below 1 answers 0 without a draw.
#[test]
fn a_bound_below_one_draws_nothing() {
    let mut random = LegacyRandom::new(3);
    let mut copy = random.clone();
    assert_eq!(Draws(&mut random).int(0), 0);
    assert_eq!(random.next_long(), copy.next_long());
}
