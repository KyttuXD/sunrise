//! The candidate budget bounds a scan, never a movement: a movement big enough
//! to pass the caller's budget is walked in full, as vanilla's uncounted
//! collision walk does (#6124).
use std::io;
use sunrise_core::{
    geometry::{Aabb, Vec3, VoxelShape},
    position::{BlockPos, ChunkPos},
};
use sunrise_world::{
    block_collision_scan::{BlockCollisionWorld, CollisionBlock, scan_cells},
    collision_context::CollisionContext,
    entity_obstruction::EntityIdentity,
    registry::BlockStateId,
    world_movement::{
        CollisionCheckRequest, EntityMovementWorld, MovementRequest, no_world_collision,
        resolve_world_movement,
    },
};

/// Open air: every cell is reachable and has no shape, so a scan visits all of
/// them and the element count is the walk it performed.
struct OpenAir {
    cells: usize,
}
impl BlockCollisionWorld for OpenAir {
    type Error = io::Error;
    type Chunk = ();
    fn collision_chunk(&mut self, _: ChunkPos) -> io::Result<Option<()>> {
        Ok(Some(()))
    }
    fn collision_block(&mut self, _: (), _: BlockPos) -> io::Result<CollisionBlock> {
        self.cells += 1;
        Ok(CollisionBlock {
            state: BlockStateId(0),
            large_collision_shape: false,
            moving_piston: false,
            empty_shape: true,
        })
    }
    fn is_suffocating(&mut self, _: (), _: CollisionBlock, _: BlockPos) -> io::Result<bool> {
        unreachable!()
    }
    fn context_shape(
        &mut self,
        _: CollisionBlock,
        _: BlockPos,
        _: &CollisionContext<'_>,
    ) -> io::Result<VoxelShape> {
        unreachable!()
    }
}
impl EntityMovementWorld for OpenAir {
    type Error = io::Error;
    fn entity_collisions(&mut self, _: EntityIdentity, _: Aabb) -> io::Result<Vec<VoxelShape>> {
        Ok(Vec::new())
    }
    fn movement_border(&mut self, _: EntityIdentity, _: Aabb) -> io::Result<Option<VoxelShape>> {
        Ok(None)
    }
}

fn player() -> Aabb {
    Aabb::new(Vec3::new(0.5, 64.0, 0.5), Vec3::new(1.1, 65.8, 1.1))
}

fn resolve(movement: Vec3, candidate_budget: usize) -> (Vec3, OpenAir) {
    let mut world = OpenAir { cells: 0 };
    let resolved = resolve_world_movement(
        &mut world,
        MovementRequest {
            entity: EntityIdentity(0),
            context: CollisionContext::empty(false),
            bounds: player(),
            movement,
            max_step: 0.6,
            on_ground: true,
            candidate_budget,
        },
    )
    .expect("a movement is never aborted by the candidate budget");
    (resolved, world)
}

#[test]
fn a_diagonal_sweep_past_the_budget_is_walked_in_full() {
    // 40 blocks each way pads to about 42 x 4 x 42 cells, far past the budget.
    let movement = Vec3::new(40.0, 0.0, 40.0);
    let sweep = scan_cells(player().expand_towards(movement)).expect("a scannable box");
    assert!(sweep > 1_000, "the sweep is {sweep} cells");
    let (resolved, world) = resolve(movement, 10);
    assert_eq!(resolved, movement);
    assert!(
        world.cells > 0,
        "the whole sweep was walked: {} cells",
        world.cells
    );
}

#[test]
fn scan_cells_covers_what_the_scan_visits() {
    // The count includes the corners the cursor skips, so it is an upper bound
    // on the walk that is never short of it by more than the eight corners.
    for movement in [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, -0.5, 2.0),
        Vec3::new(40.0, 3.0, 40.0),
    ] {
        let (_, world) = resolve(movement, 0);
        let sweep = scan_cells(player().expand_towards(movement)).expect("a scannable box");
        if movement.length_squared() == 0.0 {
            // A zero movement is never scanned.
            assert_eq!(world.cells, 0);
        } else {
            assert!(world.cells <= sweep, "movement {movement:?}");
            assert!(sweep - world.cells <= 8, "movement {movement:?}");
        }
    }
}

#[test]
fn ordinary_movements_are_unchanged() {
    // Resolved movement does not depend on the budget once it covers the sweep.
    for movement in [
        Vec3::new(0.2, -0.0784, 0.1),
        Vec3::new(1.0, 0.0, -3.0),
        Vec3::new(0.0, -0.5, 0.0),
    ] {
        assert_eq!(
            resolve(movement, 1_000_000).0,
            resolve(movement, 1).0,
            "movement {movement:?}"
        );
    }
}

/// Sparse collision world for large `no_world_collision` queries: one optional
/// full block and one optional entity box, with empty sections skipped just as
/// `SceneCollisions` skips air sections.
struct SparseWorld {
    block: Option<BlockPos>,
    entity: Option<Aabb>,
}
impl BlockCollisionWorld for SparseWorld {
    type Error = io::Error;
    type Chunk = ();
    fn collision_chunk(&mut self, _: ChunkPos) -> io::Result<Option<()>> {
        Ok(Some(()))
    }
    fn collision_block(&mut self, _: (), p: BlockPos) -> io::Result<CollisionBlock> {
        let solid = self.block == Some(p);
        Ok(CollisionBlock {
            state: BlockStateId(u32::from(solid)),
            large_collision_shape: false,
            moving_piston: false,
            empty_shape: !solid,
        })
    }
    fn is_suffocating(&mut self, _: (), _: CollisionBlock, _: BlockPos) -> io::Result<bool> {
        Ok(false)
    }
    fn context_shape(
        &mut self,
        _: CollisionBlock,
        _: BlockPos,
        _: &CollisionContext<'_>,
    ) -> io::Result<VoxelShape> {
        Ok(VoxelShape::block())
    }
    fn may_reach_out(&mut self, _: (), _: i32) -> io::Result<bool> {
        Ok(false)
    }
    fn section_is_empty(&mut self, _: (), section_y: i32) -> io::Result<bool> {
        Ok(self.block.is_none_or(|b| (b.y >> 4) != section_y))
    }
    fn missing_chunks_stay_missing(&self) -> bool {
        true
    }
}
impl EntityMovementWorld for SparseWorld {
    type Error = io::Error;
    fn entity_collisions(
        &mut self,
        _: EntityIdentity,
        bounds: Aabb,
    ) -> io::Result<Vec<VoxelShape>> {
        let mut shapes = Vec::new();
        if let Some(aabb) = self.entity
            && aabb.intersects(bounds)
        {
            shapes.push(VoxelShape::from_box(aabb).map_err(io::Error::other)?);
        }
        Ok(shapes)
    }
    fn movement_border(&mut self, _: EntityIdentity, _: Aabb) -> io::Result<Option<VoxelShape>> {
        Ok(None)
    }
}

#[test]
fn large_no_collision_query_covers_box_past_budget_and_preserves_hits() {
    // A 128 x 128 x 128 player box spans >2M candidate cells, past the old 1M budget.
    let large = Aabb::new(Vec3::new(-64.0, 0.0, -64.0), Vec3::new(64.0, 128.0, 64.0));
    let cells = scan_cells(large).expect("128^3 box fits in Cursor3D domain");
    assert!(cells > 2_000_000, "expected >2M cells, got {cells}");

    let check = |world: &mut SparseWorld, budget: usize| {
        no_world_collision(
            world,
            CollisionCheckRequest {
                entity: EntityIdentity(0),
                context: CollisionContext::empty(false),
                bounds: large,
                candidate_budget: budget,
            },
        )
        .expect("no_world_collision must cover the candidate box without VisitBudget")
    };

    assert!(check(
        &mut SparseWorld {
            block: None,
            entity: None
        },
        1_000_000
    ));
    assert!(check(
        &mut SparseWorld {
            block: None,
            entity: None
        },
        10
    ));
    assert!(!check(
        &mut SparseWorld {
            block: Some(BlockPos::new(50, 100, -40)),
            entity: None,
        },
        1_000_000,
    ));
    assert!(!check(
        &mut SparseWorld {
            block: None,
            entity: Some(Aabb::new(
                Vec3::new(50.0, 100.0, 50.0),
                Vec3::new(51.0, 101.0, 51.0),
            )),
        },
        1_000_000,
    ));

    // Also verify a world without empty-section skipping walks past a tiny budget.
    let medium = Aabb::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(20.0, 20.0, 20.0));
    let medium_cells = scan_cells(medium).expect("20^3 box fits in Cursor3D domain");
    let mut open = OpenAir { cells: 0 };
    assert!(
        no_world_collision(
            &mut open,
            CollisionCheckRequest {
                entity: EntityIdentity(0),
                context: CollisionContext::empty(false),
                bounds: medium,
                candidate_budget: 10,
            },
        )
        .expect("must not fail with VisitBudget")
    );
    assert!(open.cells > 10 && open.cells <= medium_cells);
}
