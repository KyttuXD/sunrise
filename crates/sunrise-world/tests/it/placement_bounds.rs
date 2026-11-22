use sunrise_core::random::{LegacyRandom, RandomSource};
use sunrise_world::{
    heightmap::HeightmapType,
    worldgen::{
        anchor::WorldGenerationContext,
        placement::{
            FeaturePlacer, LayerBlock, PlacementContext, PlacementModifier, PlacementQueryError,
        },
        value_provider::IntProvider,
    },
};

#[test]
fn negative_cuboid_extent_skips_empty_loops_even_with_huge_other_extents() {
    let modifier = PlacementModifier::Cuboid {
        xz_size: IntProvider::Constant(i32::MAX),
        y_size: IntProvider::Constant(-1),
        include_edges: true,
        include_interior: true,
    };
    let result = modifier.modify(
        WorldGenerationContext::new(0, 16, 8),
        &mut LegacyRandom::new(0),
        [0, 0, 0],
        |_, _| -> Result<(), ()> { panic!("empty cuboid emitted") },
    );
    assert!(result.is_ok());
}
struct DeepColumn;
impl PlacementContext for DeepColumn {
    fn generation(&self) -> WorldGenerationContext {
        WorldGenerationContext::new(-64, 384, 63)
    }
    fn min_y(&mut self) -> Result<i32, PlacementQueryError> {
        Ok(-64)
    }
    fn height(&mut self, _: HeightmapType, _: i32, _: i32) -> Result<i32, PlacementQueryError> {
        Ok(320)
    }
    fn layer_block(&mut self, [_, y, _]: [i32; 3]) -> Result<LayerBlock, PlacementQueryError> {
        Ok(match y {
            319 => LayerBlock::Bedrock,
            -60 => LayerBlock::Ground,
            _ => LayerBlock::Empty,
        })
    }
}

#[test]
fn layered_scans_visit_every_attempt_in_a_deep_column() {
    let mut random = LegacyRandom::new(5);
    let mut expected = random.clone();
    let mut attempts = 0;
    PlacementModifier::CountOnEveryLayer(IntProvider::Constant(256))
        .modify_in(&mut DeepColumn, &mut random, [0; 3], |random, pos| {
            let x = expected.next_int_bound(16).unwrap();
            let z = expected.next_int_bound(16).unwrap();
            assert_eq!(pos, [x, -59, z]);
            assert_eq!(random.next_int(), expected.next_int());
            attempts += 1;
            Ok::<_, ()>(())
        })
        .unwrap();
    assert_eq!(attempts, 256);
    // The next layer still scans every column even though it finds no positions.
    for _ in 0..512 {
        expected.next_int_bound(16).unwrap();
    }
    assert_eq!(random.next_long(), expected.next_long());
}

#[test]
fn noise_count_is_not_limited_by_count_placement_codec() {
    let mut random = LegacyRandom::new(6);
    let mut expected = random.clone();
    let mut draws = 0;
    let placed = FeaturePlacer::new()
        .place_in(
            &[PlacementModifier::NoiseThresholdCount {
                level: 0.,
                below: 4097,
                above: 4097,
            }],
            &mut DeepColumn,
            &mut random,
            [1, 2, 3],
            |random, position| {
                assert_eq!(position, [1, 2, 3]);
                assert_eq!(random.next_int(), expected.next_int());
                draws += 1;
                Ok::<_, ()>(true)
            },
        )
        .unwrap();
    assert!(placed);
    assert_eq!(draws, 4097);
    assert_eq!(random.next_long(), expected.next_long());
}

#[test]
fn failed_root_does_not_pass_pending_siblings_to_next_root() {
    let mut placer = FeaturePlacer::new();
    let mut random = LegacyRandom::new(7);
    let mut expected = random.clone();
    let mut positions = Vec::new();
    assert!(
        placer
            .place_in(
                &[
                    PlacementModifier::Fixed(vec![[0, 1, 2], [3, 4, 5], [6, 7, 8]]),
                    PlacementModifier::Count(IntProvider::Constant(2)),
                ],
                &mut DeepColumn,
                &mut random,
                [0; 3],
                |random, pos| {
                    assert_eq!(random.next_int(), expected.next_int());
                    positions.push(pos);
                    if positions.len() == 2 {
                        Err("failed leaf")
                    } else {
                        Ok(true)
                    }
                },
            )
            .is_err()
    );
    assert!(
        placer
            .place_in(
                &[PlacementModifier::Fixed(vec![[11, 12, 13]])],
                &mut DeepColumn,
                &mut random,
                [0; 3],
                |random, pos| {
                    assert_eq!(random.next_int(), expected.next_int());
                    positions.push(pos);
                    Ok::<_, ()>(true)
                },
            )
            .unwrap()
    );
    assert_eq!(positions, [[0, 1, 2], [0, 1, 2], [11, 12, 13]]);
    assert_eq!(random.next_long(), expected.next_long());
}
