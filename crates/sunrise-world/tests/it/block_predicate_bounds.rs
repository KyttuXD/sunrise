use sunrise_core::random::{LegacyRandom, RandomSource};
use sunrise_world::{
    heightmap::HeightmapType,
    worldgen::{
        anchor::WorldGenerationContext,
        block_predicate::BlockPredicate,
        placement::{PlacementContext, PlacementModifier, PlacementQueryError},
    },
};
struct UnboundedWorld(usize);
impl PlacementContext for UnboundedWorld {
    fn generation(&self) -> WorldGenerationContext {
        WorldGenerationContext::new(0, 16, 8)
    }
    fn min_y(&mut self) -> Result<i32, PlacementQueryError> {
        Ok(i32::MIN)
    }
    fn max_y(&mut self) -> Result<i32, PlacementQueryError> {
        Ok(i32::MAX)
    }
    fn height(&mut self, _: HeightmapType, _: i32, _: i32) -> Result<i32, PlacementQueryError> {
        unreachable!()
    }
    fn outside_build_height(&mut self, _: i32) -> Result<bool, PlacementQueryError> {
        self.0 += 1;
        Ok(false)
    }
}
#[test]
fn environment_scan_checks_every_configured_step_without_a_draw() {
    let scan = PlacementModifier::EnvironmentScan {
        upwards: true,
        target: BlockPredicate::Not(Box::new(BlockPredicate::True)),
        allowed: BlockPredicate::True,
        max_steps: 32,
    };
    let mut world = UnboundedWorld(0);
    let mut random = LegacyRandom::new(1);
    let mut expected = random.clone();
    let result = scan.modify_in(
        &mut world,
        &mut random,
        [0, 0, 0],
        |_, _| -> Result<(), ()> { panic!("false target must not emit") },
    );
    assert_eq!(result.ok(), Some(()));
    assert_eq!(world.0, 32);
    assert_eq!(random.next_long(), expected.next_long());
}
#[test]
fn predicate_evaluation_does_not_limit_visits_to_the_decode_budget() {
    let leaf = r#"{"type":"true"}"#;
    let branch = format!(
        r#"{{"type":"all_of","predicates":[{}]}}"#,
        vec![leaf; 256].join(",")
    );
    let tree = format!(
        r#"{{"type":"all_of","predicates":[{}]}}"#,
        vec![branch; 256].join(",")
    );
    assert!(BlockPredicate::from_json(&tree).is_err());
    let tree = BlockPredicate::All(vec![
        BlockPredicate::All(vec![BlockPredicate::True; 256]);
        256
    ]);
    assert!(tree.test(&mut UnboundedWorld(0), [0; 3]).unwrap());
}

#[test]
fn volume_keeps_early_exit() {
    let mut world = UnboundedWorld(0);
    let volume = |predicate| BlockPredicate::Volume {
        min: [i32::MIN; 3],
        max: [i32::MAX; 3],
        predicate: Box::new(predicate),
    };
    assert!(
        !volume(BlockPredicate::Not(Box::new(BlockPredicate::True)))
            .test(&mut world, [0; 3])
            .unwrap()
    );
}
