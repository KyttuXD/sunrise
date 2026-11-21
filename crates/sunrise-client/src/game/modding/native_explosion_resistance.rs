//! Explosion resistance reads for server mods: what a blast a mod could set off would meet in one
//! cell or many, by the rule its rays use (`Game::cell_explosion_resistance`), without exploding
//! anything.
use super::{Game, explosion::PartAnswers};
use mod_api::{
    EntityRef, ExplosionInteraction, ExplosionResistance, LevelRef, ResistanceCells,
    ResistanceCellsError,
};

impl Game<'_> {
    /// The resistance the blast `level.explode(source, center, radius, fire, interaction)` would
    /// meet in the cell at `pos`. `Unavailable` for a stale level or source, a chunk that isn't
    /// loaded or an unknown state.
    pub(super) fn level_explosion_resistance(
        &mut self,
        level: LevelRef,
        pos: [i32; 3],
        source: Option<EntityRef>,
        center: [f64; 3],
        radius: f32,
        fire: bool,
        interaction: ExplosionInteraction,
    ) -> Option<ExplosionResistance> {
        let blast = Blast {
            source,
            center,
            radius,
            fire,
            interaction,
        };
        let mut answer = ExplosionResistance::Unavailable;
        self.visit_explosion_resistances(level, [pos], &blast, None, |value| answer = value);
        Some(answer)
    }

    /// `level_explosion_resistance` for every cell of `cells`, in its order.
    pub(super) fn level_explosion_resistances(
        &mut self,
        level: LevelRef,
        cells: &ResistanceCells,
        source: Option<EntityRef>,
        center: [f64; 3],
        radius: f32,
        fire: bool,
        interaction: ExplosionInteraction,
    ) -> Result<Vec<ExplosionResistance>, ResistanceCellsError> {
        let count = cell_count(cells)?;
        let mut out = Vec::new();
        out.try_reserve_exact(count)
            .map_err(|_| ResistanceCellsError::Capacity)?;
        let blast = Blast {
            source,
            center,
            radius,
            fire,
            interaction,
        };
        let mut answers = PartAnswers::default();
        let started = std::time::Instant::now();
        match cells {
            ResistanceCells::Box { min, max } => {
                let positions = (min[1]..=max[1]).flat_map(|y| {
                    (min[2]..=max[2]).flat_map(move |z| (min[0]..=max[0]).map(move |x| [x, y, z]))
                });
                self.visit_explosion_resistances(
                    level,
                    positions,
                    &blast,
                    Some(&mut answers),
                    |value| out.push(value),
                );
            }
            ResistanceCells::Columns {
                columns,
                min_y,
                max_y,
            } => {
                let positions = columns
                    .iter()
                    .flat_map(|&(x, z)| (*min_y..=*max_y).map(move |y| [x, y, z]));
                self.visit_explosion_resistances(
                    level,
                    positions,
                    &blast,
                    Some(&mut answers),
                    |value| out.push(value),
                );
            }
            _ => unreachable!("ResistanceCells: unhandled variant"),
        }
        let ms = started.elapsed().as_secs_f32() * 1000.0;
        if self.options.spikes.is_some_and(|threshold| ms > threshold) {
            log::info!("Sunrise: slow mod explosion resistance read {ms:.1} ms ({count} cells)");
        }
        Ok(out)
    }

    /// Resolve the blast in the level in place, then sample the supplied cells. The calculator
    /// and damage source are the same as `native_explode` uses, independent of centre residency.
    fn visit_explosion_resistances(
        &mut self,
        level: LevelRef,
        positions: impl IntoIterator<Item = [i32; 3]>,
        blast: &Blast,
        mut answers: Option<&mut PartAnswers>,
        mut answer: impl FnMut(ExplosionResistance),
    ) {
        let source = self
            .level_in_place(level)
            .then(|| match blast.source {
                Some(entity) => self.native_explosion_source(entity).map(Some),
                None => Some(None),
            })
            .flatten();
        let Some(source) = source else {
            for _ in positions {
                answer(ExplosionResistance::Unavailable);
            }
            return;
        };
        let interaction =
            self.native_blast_interaction(blast.interaction, source.map(|(id, _)| id));
        let outer = self.take_blast_state();
        if let Some((id, target)) = source {
            self.attribute_native_blast(id, target);
            self.native_blast_calculator(id, target);
        }
        let rules = self.blast_rules(None, false);
        let start = self.explosion_start(
            blast.center,
            blast.radius,
            interaction,
            blast.fire,
            self.blast_damage_type(),
        );
        for pos @ [x, y, z] in positions {
            // Level.getBlockState returns void air beyond build height; no chunk is loaded
            // for these queries. A missing column provides no sample at any height.
            let value = if !self.world.loaded(x, z) {
                ExplosionResistance::Unavailable
            } else if !self.world.shape.contains_y(y) {
                ExplosionResistance::NoResistance
            } else {
                let state = self.world.block(x, y, z);
                if self.pack.assets.registry.state(state).is_none() {
                    ExplosionResistance::Unavailable
                } else {
                    let (resistance, _) = self.cell_explosion_resistance(
                        pos,
                        state,
                        &rules,
                        &start,
                        answers.as_deref_mut(),
                    );
                    resistance.map_or(
                        ExplosionResistance::NoResistance,
                        ExplosionResistance::Resists,
                    )
                }
            };
            answer(value);
        }
        self.restore_blast_state(outer);
    }
}

/// The blast a read describes, as `LevelRef::explode` takes it.
struct Blast {
    source: Option<EntityRef>,
    center: [f64; 3],
    radius: f32,
    fire: bool,
    interaction: ExplosionInteraction,
}

/// Check coordinates and the result vector's representable length before reading any cells.
fn cell_count(cells: &ResistanceCells) -> Result<usize, ResistanceCellsError> {
    let span = |low: i32, high: i32| {
        if high < low {
            Err(ResistanceCellsError::Inverted)
        } else {
            Ok(u128::from(high.abs_diff(low)) + 1)
        }
    };
    // Each i32 axis covers at most 2^32 cells; their product fits u128 even on 64-bit hosts.
    let count = match cells {
        ResistanceCells::Box { min, max } => {
            span(min[0], max[0])? * span(min[1], max[1])? * span(min[2], max[2])?
        }
        ResistanceCells::Columns {
            columns,
            min_y,
            max_y,
        } => (columns.len() as u128) * span(*min_y, *max_y)?,
        _ => unreachable!("ResistanceCells: unhandled variant"),
    };
    usize::try_from(count).map_err(|_| ResistanceCellsError::Capacity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_batch_refuses_inverted_or_unrepresentable_results() {
        assert_eq!(
            cell_count(&ResistanceCells::Box {
                min: [0, 5, 0],
                max: [0, 4, 0]
            }),
            Err(ResistanceCellsError::Inverted)
        );
        assert_eq!(
            cell_count(&ResistanceCells::Box {
                min: [i32::MIN; 3],
                max: [i32::MAX; 3]
            }),
            Err(ResistanceCellsError::Capacity)
        );
    }
}
