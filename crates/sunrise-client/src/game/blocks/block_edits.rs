//! A server mod's bulk block edit (`LevelRef::edit_blocks`): its palette and
//! filters compile to the command matcher, its heights read, and its runs made
//! at once through `World::edit_runs`, with what `fill ... strict` does for
//! the block entities and the watched blocks of each block changed.
use super::Game;
use mod_api::{BlockEdit, BlockEditError, Heightmap};
use std::sync::Arc;
use sunrise_client::{
    block_edit::{EditJournal, Run},
    containers::BlockEntity,
    fast_hash::{FxHashMap, FxHashSet},
    world::World,
};
use sunrise_world::{heightmap::HeightmapType, registry::BlockStateId};

/// A change `World::edit_runs` reported: position, previous and new state.
pub(super) type Tracked = ((i32, i32, i32), u32, u32);

impl Game<'_> {
    /// Make a mod's block edit; how many blocks changed, or why none did.
    /// Every palette entry, filter and run is checked before anything
    /// changes: an invalid typed state or predicate (NBT cannot be represented),
    /// a run naming a palette entry
    /// or filter past the end, or more than 65536 filters, and nothing is
    /// made.
    pub(super) fn apply_block_edit(&mut self, edit: &BlockEdit) -> Result<u64, BlockEditError> {
        let super::api_values::block_edit::PreparedBlockEdit { palette, filters } =
            super::api_values::block_edit::prepare(edit)?;
        let runs = resolve_runs(edit, &palette, filters.len(), |h, x, z| {
            self.world_column_height(&self.world, h, x, z)
        })?;
        // (what the registry, the tags and the facts tell of a state, beside the world being changed)
        self.ensure_stateful_entity_table();
        let stateful = self.world.block_entities.stateful_entities();
        let registry = self.pack.assets.registry;
        // (the tags as `block_matches` reads them for commands)
        let world_items = match &self.world_tables {
            Some(t) => &t.items,
            None => self.res.items(),
        };
        let tags = [&self.res.items().block_tags, &world_items.block_tags];
        let facts = self.res.facts();
        let watched = std::mem::take(&mut self.world.watched);
        let mut journal = (!self.transactions.is_empty()).then(EditJournal::default);
        let out = self.world.edit_runs(
            journal.as_mut(),
            &runs,
            |f, s| {
                super::commands::block_input_matches(
                    registry,
                    tags,
                    &filters[f as usize],
                    BlockStateId(s),
                )
            },
            |s| {
                let f = facts.get(BlockStateId(s));
                watched.get(s as usize).copied().unwrap_or(false)
                    || f.block_entity
                    || stateful.get(s as usize).copied().unwrap_or(false)
                    || BlockEntity::for_block(&f.name).is_some()
            },
        );
        self.world.watched = watched;
        if let Some(journal) = &mut journal {
            // Keep the first state in the same journal as ordinary writes. Capture each
            // holder before a strict replacement can retire it or install a new generation.
            for (pos, was) in std::mem::take(&mut journal.originals) {
                self.journal_bulk_block(pos, BlockStateId(was));
            }
            for &(pos, _, _) in &out.tracked {
                self.journal_bulk_holder(pos);
            }
        }
        // The block entities the new blocks bring, as `native_set_block` makes them (a mod block's own,
        // included), in the order `replace_block_entities` takes them: the positions needing a fresh entity.
        let name = |s: u32| self.res.facts().get(BlockStateId(s)).name.clone();
        let wanted: Vec<BlockStateId> = out
            .tracked
            .iter()
            .filter(|&&((x, y, z), old, new)| {
                name(old) != name(new)
                    || !self
                        .world
                        .block_entities
                        .get(&(x, y, z))
                        .is_some_and(|e| e.fits(&name(new)))
            })
            .map(|&(_, _, new)| BlockStateId(new))
            .collect();
        let mut made =
            make_block_entities(&wanted, |state| self.new_block_entity_for_state(*state))
                .into_iter();
        let facts = self.res.facts();
        let removed = replace_block_entities(
            &mut self.world,
            &out.tracked,
            journal.is_none(),
            |s| facts.get(BlockStateId(s)).name.clone(),
            |_| made.next().flatten(),
        );
        if let Some(journal) = journal {
            for chunk in &journal.chunks {
                self.journal_chunk_save(chunk.key);
            }
            self.after_commit(super::transactions::After::BulkBlocks(
                journal.chunks,
                out.tracked,
            ));
        } else {
            for pos in removed {
                self.jukebox_removed(pos);
            }
        }
        Ok(out.changed)
    }

    /// One above the highest block of column `(x, z)` in `world` that counts on heightmap `h`, or
    /// the bottom of the column (also where it is not loaded).
    pub(super) fn world_column_height(&self, world: &World, h: Heightmap, x: i32, z: i32) -> i32 {
        let properties = &self.pack.assets.properties;
        column_height(
            world,
            h,
            (x, z),
            |s| self.res.facts().is_air(s),
            |t, s| t.is_opaque(properties.heightmap_state(s)),
        )
    }
}

/// One above the highest block of column `(x, z)` on heightmap `h`, or the
/// bottom of the column (also where it is not loaded): `air` says which
/// blocks the world surface passes over, and `counts(kind, state)` which
/// count on the other heightmaps.
fn column_height(
    world: &World,
    h: Heightmap,
    (x, z): (i32, i32),
    air: impl Fn(BlockStateId) -> bool,
    counts: impl Fn(HeightmapType, BlockStateId) -> bool,
) -> i32 {
    // MotionBlockingNoLeaves and OceanFloor only count states at or below MotionBlocking.
    let top = if h == Heightmap::WorldSurface {
        world.world_surface(x, z, &air)
    } else {
        world.motion_blocking(x, z, |s| counts(HeightmapType::MotionBlocking, s))
    };
    // Heightmap.getFirstAvailable uses the level's build range, not the chunk's padding.
    let (bottom, ceiling) = (world.shape.min_y, world.shape.max_y());
    let mut y = top.clamp(bottom, ceiling);
    if top <= ceiling && matches!(h, Heightmap::WorldSurface | Heightmap::MotionBlocking) {
        return y;
    }
    while y > bottom {
        let state = world.block(x, y - 1, z);
        let matches = match h {
            Heightmap::WorldSurface => !air(state),
            Heightmap::MotionBlockingNoLeaves => {
                counts(HeightmapType::MotionBlockingNoLeaves, state)
            }
            Heightmap::OceanFloor => counts(HeightmapType::OceanFloor, state),
            _ => counts(HeightmapType::MotionBlocking, state),
        };
        if matches {
            break;
        }
        y -= 1;
    }
    y
}

/// `edit`'s runs as `World::edit_runs` takes them: each palette entry `block`
/// as its state in `palette`, each `y` counted from `height(surface, x, z)`
/// when the run names a heightmap (read before any change, once a column).
/// Err, with nothing resolved, for a run naming a palette entry or a filter
/// past the end, or for more filters than a run can name (65536).
fn resolve_runs(
    edit: &BlockEdit,
    palette: &[u32],
    filters: usize,
    mut height: impl FnMut(Heightmap, i32, i32) -> i32,
) -> Result<Vec<Run>, BlockEditError> {
    if filters > usize::from(u16::MAX) + 1 {
        return Err(BlockEditError::TooManyFilters);
    }
    let mut heights: FxHashMap<(i32, i32, u8), i32> = FxHashMap::default();
    let mut runs = Vec::with_capacity(edit.runs.len());
    for r in &edit.runs {
        let Some(&state) = usize::try_from(r.block).ok().and_then(|b| palette.get(b)) else {
            return Err(BlockEditError::PaletteOutOfRange);
        };
        let only = match r.only {
            None => None,
            Some(f) if (f as usize) < filters => Some(f as u16),
            Some(_) => return Err(BlockEditError::FilterOutOfRange),
        };
        if r.height == 0 {
            continue;
        }
        let base = match r.surface {
            None => i64::from(r.y),
            Some(h) => {
                let top = *heights
                    .entry((r.x, r.z, h as u8))
                    .or_insert_with(|| height(h, r.x, r.z));
                i64::from(top) + i64::from(r.y)
            }
        };
        let clamp = |v: i64| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        runs.push(Run {
            x: r.x,
            z: r.z,
            y0: clamp(base),
            y1: clamp(base + i64::from(r.height) - 1),
            state,
            only,
        });
    }
    Ok(runs)
}

/// What `fill ... strict` does for the block entities and the watched blocks
/// of `tracked`'s changes: the watched writes recorded, the block entities
/// told of their new state, and where a different block replaced the block,
/// its block entity removed (as `place_state` does) and `new_entity(name)`'s,
/// if any, put in its place. Returns the positions whose block was replaced
/// (a jukebox there stops).
/// A block entity for each new block, in order: made for each position, as vanilla calls
/// `EntityBlock.newBlockEntity` for each (a mod's may number its instances). A block that makes none (stone) is
/// asked once.
fn make_block_entities<S: Eq + std::hash::Hash>(
    states: &[S],
    mut make: impl FnMut(&S) -> Option<BlockEntity>,
) -> Vec<Option<BlockEntity>> {
    let mut none: FxHashSet<&S> = FxHashSet::default();
    states
        .iter()
        .map(|n| {
            if none.contains(n) {
                return None;
            }
            let entity = make(n);
            if entity.is_none() {
                none.insert(n);
            }
            entity
        })
        .collect()
}

fn replace_block_entities(
    world: &mut World,
    tracked: &[Tracked],
    publish: bool,
    name: impl Fn(u32) -> Arc<str>,
    mut new_entity: impl FnMut(&str) -> Option<BlockEntity>,
) -> Vec<[i32; 3]> {
    let mut replaced = Vec::new();
    for &((x, y, z), old, new) in tracked {
        if publish {
            notify_bulk_block(world, ((x, y, z), old, new));
        }
        let (was, now) = (name(old), name(new));
        if was != now {
            replaced.push([x, y, z]);
        }
        // LevelChunk.setBlockState: the entity at the position keeps its
        // place only while it is valid for the block now there
        // (`BlockEntity.isValidBlockState`); a mismatched or missing one
        // makes way for the block's own.
        if was != now
            || !world
                .block_entities
                .get(&(x, y, z))
                .is_some_and(|e| e.fits(&now))
        {
            world.block_entities.remove(&(x, y, z));
            if let Some(entity) = new_entity(&now) {
                world.block_entities.insert((x, y, z), entity);
            }
        }
    }
    replaced
}

pub(super) fn notify_bulk_block(world: &mut World, change @ (pos, old, new): Tracked) {
    let watches = |id: u32| world.watched.get(id as usize).copied().unwrap_or(false);
    if watches(old) || watches(new) {
        world.watched_writes.push(change);
    }
    world.block_entities.state_changed(pos, old, new);
}

#[cfg(test)]
mod tests {
    use super::*;
    use mod_api::BlockRun;
    use sunrise_client::chunk::Chunk;

    #[test]
    #[ignore = "daily"]
    fn typed_bulk_late_refusals_preserve_the_world_and_emit_no_changes() {
        use crate::game::api_values::TryIntoApi;
        use mod_api::{
            StateConstraint, StatePredicate, StatePredicateRecord, StatePredicateTarget,
        };
        crate::game::native_entity_refs::tests::with_game(|game| {
            let level = game.level_ref(game.native_lifecycle.epoch().unwrap(), game.dim.key());
            game.world
                .chunks
                .insert((0, 0), std::sync::Arc::new(Chunk::empty(0, 0)));
            let registry = game.pack.assets.registry;
            let air = registry.default_state("minecraft:air").unwrap();
            let stone = registry.default_state("minecraft:stone").unwrap();
            let api_air = air.try_into_api().unwrap();
            let api_stone = stone.try_into_api().unwrap();
            let metadata = mod_api::__private::block_definitions().unwrap();
            let good = StatePredicate::block(metadata, api_air.block(), &[]).unwrap();
            let bad = StatePredicate::from(StatePredicateRecord::new(
                StatePredicateTarget::Block(api_air.block()),
                vec![StateConstraint::new("unknown", "true")],
            ));
            let mut edit = BlockEdit {
                palette: vec![api_stone],
                filters: vec![good, bad],
                runs: vec![BlockRun {
                    x: 0,
                    z: 0,
                    y: 64,
                    height: 1,
                    surface: None,
                    block: 0,
                    only: Some(0),
                }],
            };
            game.world.block_log = Some(Vec::new());
            assert_eq!(game.world.block(0, 64, 0), air);
            let refused = game.level_edit_blocks(level, &edit);
            assert_eq!(
                game.world.block(0, 64, 0),
                air,
                "late filter refusal must preserve the world"
            );
            assert!(game.world.block_log.as_ref().unwrap().is_empty());
            assert_eq!(refused, Err(BlockEditError::InvalidPredicate));

            edit.filters.pop();
            edit.palette
                .push(mod_api::__private::block_state_from_ordinal(
                    api_stone.block(),
                    u16::MAX,
                ));
            let refused = game.level_edit_blocks(level, &edit);
            assert_eq!(
                game.world.block(0, 64, 0),
                air,
                "late palette refusal must preserve the world"
            );
            assert!(game.world.block_log.as_ref().unwrap().is_empty());
            assert!(matches!(refused, Err(BlockEditError::InvalidState(_))));

            edit.palette.pop();
            assert_eq!(game.level_edit_blocks(level, &edit), Ok(1));
            assert_eq!(
                game.world.block(0, 64, 0),
                stone,
                "the same valid run must actually mutate"
            );
            game.world.block_log = None;
        });
    }

    #[test]
    #[ignore = "daily"]
    fn mixed_bulk_and_ordinary_writes_publish_only_on_outer_commit() {
        use crate::game::{api_values::TryIntoApi, native_entity_refs::tests::with_game};
        use mod_api::BlockUpdateFlags;

        with_game(|game| {
            game.world
                .chunks
                .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
            game.world.block_log = Some(Vec::new());
            let level = game.level(game.dim.id()).unwrap();
            let state = |name| game.pack.assets.registry.default_state(name).unwrap();
            let (air, stone, chest, dirt) = (
                state("minecraft:air"),
                state("minecraft:stone"),
                state("minecraft:chest"),
                state("minecraft:dirt"),
            );
            let original = [1, 64, 0];
            let new_holder = [2, 64, 0];
            let ordinary = [3, 64, 0];
            let sync = [4, 64, 0];
            for pos in [original, sync] {
                assert_eq!(
                    level.set_block(
                        game,
                        pos,
                        chest.try_into_api().unwrap(),
                        BlockUpdateFlags::NONE
                    ),
                    Ok(true)
                );
            }
            let holder = game.block_entity(level, original).unwrap();
            let explicit = game.block_entity(level, sync).unwrap();
            let edit = BlockEdit {
                palette: vec![stone.try_into_api().unwrap(), chest.try_into_api().unwrap()],
                filters: vec![],
                runs: [(original, 0), (new_holder, 1)]
                    .into_iter()
                    .map(|([x, y, z], block)| BlockRun {
                        x,
                        y,
                        z,
                        block,
                        height: 1,
                        surface: None,
                        only: None,
                    })
                    .collect(),
            };
            game.world.modified.clear();
            game.world.dirty_light.clear();
            game.world.block_log.as_mut().unwrap().clear();

            for commit in [false, true] {
                let tx = game.transaction_open();
                let nested = game.transaction_open();
                assert_eq!(level.edit_blocks(game, &edit), Ok(2));
                game.transaction_close(&nested, true);
                assert!(game.block_entity(level, original).is_none());
                assert!(game.block_entity(level, new_holder).is_some());
                for pos in [ordinary, original] {
                    assert_eq!(
                        level.set_block(
                            game,
                            pos,
                            dirt.try_into_api().unwrap(),
                            BlockUpdateFlags::CLIENTS
                        ),
                        Ok(true)
                    );
                }
                assert!(game.world.bulk_log.is_empty());
                assert!(game.world.dirty_light.is_empty());
                assert!(game.world.modified.is_empty());
                assert!(game.world.block_log.as_ref().unwrap().is_empty());
                game.transaction_close(&tx, commit);
                assert_eq!(
                    game.world.block(1, 64, 0),
                    if commit { dirt } else { chest }
                );
                assert_eq!(game.world.block(2, 64, 0), if commit { chest } else { air });
                assert_eq!(game.world.block(3, 64, 0), if commit { dirt } else { air });
                assert_eq!(
                    game.block_entity(level, original),
                    (!commit).then_some(holder)
                );
                assert_eq!(game.block_entity(level, new_holder).is_some(), commit);
                assert_eq!(game.world.modified.contains(&(0, 0)), commit);
                assert_eq!(game.world.dirty_light.contains(&(0, 0)), commit);
                assert_eq!(game.world.bulk_log.contains(&(0, 0)), commit);
            }
            assert_eq!(explicit.sync(game), Ok(()));
            assert!(game.world.block_log.as_ref().unwrap().contains(&(4, 64, 0)));
        });
    }

    fn run(block: u32, only: Option<u32>) -> BlockRun {
        BlockRun {
            x: 1,
            z: 2,
            y: 3,
            height: 4,
            surface: None,
            block,
            only,
        }
    }
    fn edit(runs: Vec<BlockRun>) -> BlockEdit {
        BlockEdit {
            palette: vec![],
            filters: vec![],
            runs,
        }
    }

    #[test]
    fn runs_naming_entries_past_the_end_are_refused_whole() {
        let palette = [1, 0];
        let ok = resolve_runs(&edit(vec![run(1, Some(0))]), &palette, 1, |_, _, _| 0).unwrap();
        assert_eq!(
            ok,
            [Run {
                x: 1,
                z: 2,
                y0: 3,
                y1: 6,
                state: 0,
                only: Some(0),
            }]
        );
        // a bad run after a good one: nothing at all, and no height read
        let mut asked = 0;
        let mut bad = |runs| {
            resolve_runs(&edit(runs), &palette, 1, |_, _, _| {
                asked += 1;
                0
            })
        };
        assert_eq!(
            bad(vec![run(0, None), run(2, None)]),
            Err(BlockEditError::PaletteOutOfRange)
        );
        assert_eq!(
            bad(vec![run(0, None), run(u32::MAX, None)]),
            Err(BlockEditError::PaletteOutOfRange)
        );
        assert_eq!(
            bad(vec![run(0, Some(1))]),
            Err(BlockEditError::FilterOutOfRange)
        );
        assert_eq!(
            bad(vec![run(0, Some(u32::MAX))]),
            Err(BlockEditError::FilterOutOfRange)
        );
        assert_eq!(asked, 0);
        // more filters than a run's u16 names
        assert_eq!(
            resolve_runs(&edit(vec![]), &palette, 65_537, |_, _, _| 0),
            Err(BlockEditError::TooManyFilters)
        );
        assert!(resolve_runs(&edit(vec![]), &palette, 65_536, |_, _, _| 0).is_ok());
    }

    #[test]
    fn a_run_counts_from_its_heightmap_read_once_a_column() {
        let mut r = run(0, None);
        r.surface = Some(Heightmap::OceanFloor);
        r.y = -2;
        let mut asked = Vec::new();
        let runs = resolve_runs(&edit(vec![r, r]), &[7], 0, |h, x, z| {
            asked.push((h, x, z));
            70
        })
        .unwrap();
        assert_eq!(asked, [(Heightmap::OceanFloor, 1, 2)]);
        assert_eq!((runs[0].y0, runs[0].y1, runs[0].state), (68, 71, 7));
        assert_eq!(runs[1], runs[0]);
        // a run far past the world's height clamps rather than wraps
        let mut high = run(0, None);
        high.y = i32::MAX;
        high.height = u32::MAX;
        let runs = resolve_runs(&edit(vec![high]), &[7], 0, |_, _, _| 0).unwrap();
        assert_eq!((runs[0].y0, runs[0].y1), (i32::MAX, i32::MAX));
    }

    fn world() -> World {
        let mut w = World::new(BlockStateId(0), 0);
        w.chunks.insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        w
    }
    fn names(s: u32) -> Arc<str> {
        match s {
            0 => "minecraft:air",
            1 => "minecraft:stone",
            2 => "minecraft:chest",
            // another chest state (a `facing` flip): the name is unchanged
            4 => "minecraft:chest",
            _ => "machines:press",
        }
        .into()
    }

    #[test]
    fn a_chest_replaced_by_stone_loses_its_block_entity() {
        let mut w = world();
        w.set(1, 5, 1, BlockStateId(2)).unwrap();
        let chest = BlockEntity::for_block("minecraft:chest").expect("a chest's");
        w.block_entities.insert((1, 5, 1), chest);
        let out = w.edit_runs(
            None,
            &[Run {
                x: 1,
                z: 1,
                y0: 4,
                y1: 6,
                state: 1,
                only: None,
            }],
            |_, _| true,
            |s| s == 2,
        );
        assert_eq!(out.tracked, [((1, 5, 1), 2, 1)]);
        let replaced =
            replace_block_entities(&mut w, &out.tracked, true, names, BlockEntity::for_block);
        assert_eq!(replaced, [[1, 5, 1]]);
        assert!(w.block_entities.get(&(1, 5, 1)).is_none());
        assert_eq!(w.block(1, 5, 1).0, 1);
    }

    #[test]
    fn a_block_entitys_published_shapes_go_with_it_when_its_block_changes() {
        let mut w = world();
        w.set(1, 5, 1, BlockStateId(3)).unwrap();
        let mut press = sunrise_client::containers::ModBlockEntity::new(
            "machines:press".into(),
            Default::default(),
        );
        press.set_shapes(Some(Arc::new(
            sunrise_client::containers::PublishedShapes::new(mod_api::BlockEntityShapes {
                outline: vec![],
                collision: vec![[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]],
            })
            .unwrap(),
        )));
        w.block_entities
            .insert((1, 5, 1), BlockEntity::Mod(Box::new(press)));
        // Another block, of the same entity type or none: the entity is made again, bare.
        let out = w.edit_runs(
            None,
            &[Run {
                x: 1,
                z: 1,
                y0: 5,
                y1: 5,
                state: 1,
                only: None,
            }],
            |_, _| true,
            |s| s == 3,
        );
        let made = |n: &str| {
            (n == "machines:press").then(|| {
                BlockEntity::Mod(Box::new(sunrise_client::containers::ModBlockEntity::new(
                    "machines:press".into(),
                    Default::default(),
                )))
            })
        };
        replace_block_entities(&mut w, &out.tracked, true, names, made);
        assert!(w.block_entities.get(&(1, 5, 1)).is_none());
    }

    #[test]
    fn a_new_block_gets_the_block_entity_made_for_it() {
        let mut w = world();
        let out = w.edit_runs(
            None,
            &[Run {
                x: 3,
                z: 3,
                y0: 10,
                y1: 10,
                state: 3,
                only: None,
            }],
            |_, _| true,
            |s| s == 3,
        );
        // a mod block's, as `Game::new_block_entity` makes it
        let made = |n: &str| {
            (n == "machines:press").then(|| {
                BlockEntity::Mod(Box::new(sunrise_client::containers::ModBlockEntity::new(
                    "machines:press".into(),
                    Default::default(),
                )))
            })
        };
        replace_block_entities(&mut w, &out.tracked, true, names, made);
        assert!(matches!(
            w.block_entities.get(&(3, 10, 3)),
            Some(BlockEntity::Mod(e)) if e.id() == "machines:press"
        ));
    }

    /// `LevelChunk.setBlockState` keeps an entity only while
    /// `BlockEntity.isValidBlockState` holds: a stale one goes for the
    /// block's own (a sculk listener spilled under a same-name write), and
    /// a fitting one survives a same-name write (a chest's items stay).
    #[test]
    fn a_kept_block_entity_must_fit_its_block() {
        let sculk = BlockEntity::for_block("minecraft:sculk_sensor").expect("a sensor's");
        let chest = BlockEntity::for_block("minecraft:chest").expect("a chest's");
        assert!(!sculk.fits("minecraft:stone"));
        assert!(chest.fits("minecraft:chest"));
        // a 27-slot container never fits a hopper's five
        assert!(!chest.fits("minecraft:hopper"));

        let mut w = world();
        w.set(1, 5, 1, BlockStateId(2)).unwrap();
        w.block_entities.insert((1, 5, 1), sculk);
        let out = w.edit_runs(
            None,
            &[Run {
                x: 1,
                z: 1,
                y0: 5,
                y1: 5,
                state: 4,
                only: None,
            }],
            |_, _| true,
            |_| true,
        );
        assert_eq!(out.tracked, [((1, 5, 1), 2, 4)]);
        replace_block_entities(&mut w, &out.tracked, true, names, BlockEntity::for_block);
        assert!(
            matches!(
                w.block_entities.get(&(1, 5, 1)),
                Some(BlockEntity::Chest(_))
            ),
            "the chest's own entity replaces the spilled sculk one"
        );

        w.set(2, 5, 1, BlockStateId(2)).unwrap();
        w.block_entities.insert((2, 5, 1), chest);
        let out = w.edit_runs(
            None,
            &[Run {
                x: 2,
                z: 1,
                y0: 5,
                y1: 5,
                state: 4,
                only: None,
            }],
            |_, _| true,
            |_| true,
        );
        assert_eq!(out.tracked, [((2, 5, 1), 2, 4)]);
        replace_block_entities(&mut w, &out.tracked, true, names, BlockEntity::for_block);
        assert!(matches!(
            w.block_entities.get(&(2, 5, 1)),
            Some(BlockEntity::Chest(_))
        ));
    }

    /// air 0, stone 1, leaves 2, water 3: column (1, 1) stone to 60, water, then leaves to 64; column (2, 2)
    /// stone to 60, leaves, then water to 64; at (3, 3) stone at the top of the world.
    fn columns_world() -> World {
        let mut w = world();
        for (x, z, low, high) in [(1, 1, 3, 2), (2, 2, 2, 3)] {
            for y in -64..=64 {
                let s = match y {
                    ..=60 => 1,
                    61..=62 => low,
                    _ => high,
                };
                w.set(x, y, z, BlockStateId(s)).unwrap();
            }
        }
        w.set(3, 319, 3, BlockStateId(1)).unwrap();
        w
    }
    fn height(w: &World, h: Heightmap, x: i32, z: i32) -> i32 {
        column_height(
            w,
            h,
            (x, z),
            |s| s.0 == 0,
            |kind, s| match kind {
                HeightmapType::MotionBlockingNoLeaves => matches!(s.0, 1 | 3),
                HeightmapType::OceanFloor => matches!(s.0, 1 | 2),
                _ => s.0 != 0,
            },
        )
    }

    #[test]
    fn each_heightmap_has_its_own_height() {
        let w = columns_world();
        assert_eq!(height(&w, Heightmap::WorldSurface, 1, 1), 65);
        assert_eq!(height(&w, Heightmap::MotionBlocking, 1, 1), 65);
        assert_eq!(height(&w, Heightmap::MotionBlockingNoLeaves, 1, 1), 63);
        assert_eq!(height(&w, Heightmap::OceanFloor, 1, 1), 65);
        assert_eq!(height(&w, Heightmap::OceanFloor, 2, 2), 63);
        assert_eq!(height(&w, Heightmap::MotionBlockingNoLeaves, 2, 2), 65);
        // an empty column, and one not loaded: the bottom
        assert_eq!(height(&w, Heightmap::OceanFloor, 5, 5), -64);
        assert_eq!(height(&w, Heightmap::WorldSurface, 100, 100), -64);
    }

    #[test]
    fn column_heights_exclude_storage_padding() {
        let mut w = columns_world();
        w.shape = sunrise_client::chunk::LevelShape::NETHER;
        w.set(1, 319, 1, BlockStateId(1)).unwrap();
        w.set(5, -1, 5, BlockStateId(1)).unwrap();
        assert_eq!(height(&w, Heightmap::WorldSurface, 1, 1), 65);
        assert_eq!(height(&w, Heightmap::MotionBlocking, 1, 1), 65);
        assert_eq!(height(&w, Heightmap::WorldSurface, 5, 5), 0);
        assert_eq!(height(&w, Heightmap::OceanFloor, 5, 5), 0);
    }

    #[test]
    fn each_new_block_gets_its_own_block_entity() {
        // a mod's constructor numbering its instances; stone makes none
        let mut calls = Vec::new();
        let made = make_block_entities(
            &[
                "machines:press",
                "minecraft:stone",
                "machines:press",
                "minecraft:stone",
            ]
            .map(Arc::<str>::from),
            |n| {
                calls.push(n.to_string());
                (n.as_ref() == "machines:press").then(|| {
                    BlockEntity::Mod(Box::new(sunrise_client::containers::ModBlockEntity::new(
                        format!("press-{}", calls.len()),
                        Default::default(),
                    )))
                })
            },
        );
        let ids: Vec<Option<String>> = made
            .iter()
            .map(|e| match e {
                Some(BlockEntity::Mod(e)) => Some(e.id().to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(
            ids,
            [
                Some("press-1".to_string()),
                None,
                Some("press-3".to_string()),
                None
            ]
        );
        assert_eq!(
            calls,
            ["machines:press", "minecraft:stone", "machines:press"]
        );
    }
    #[test]
    fn null_factories_are_cached_per_state_not_per_block_name() {
        // One block can create no BE in one state and a BE in another.
        // A cached null for the first state must not suppress the second.
        let mut calls = Vec::new();
        let made = make_block_entities(&[7_u32, 8, 7, 8], |state| {
            calls.push(*state);
            (*state == 8).then(|| {
                BlockEntity::Mod(Box::new(sunrise_client::containers::ModBlockEntity::new(
                    "machines:heater".into(),
                    Default::default(),
                )))
            })
        });
        assert_eq!(calls, [7, 8, 8]);
        assert!(made[0].is_none() && made[2].is_none());
        assert!(made[1].is_some() && made[3].is_some());
    }
}
