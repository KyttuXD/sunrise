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
        self.explosion_resistances_of(level, [pos].into_iter(), &blast, None, |value| {
            answer = value;
        });
        Some(answer)
    }

    /// `level_explosion_resistance` for every cell of `cells`, in its order. Refused, with
    /// nothing read, for an inverted area or when the results cannot be allocated.
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
        let push = |value| out.push(value);
        match cells {
            ResistanceCells::Box { min, max } => {
                let positions = (min[1]..=max[1]).flat_map(move |y| {
                    (min[2]..=max[2]).flat_map(move |z| (min[0]..=max[0]).map(move |x| [x, y, z]))
                });
                self.explosion_resistances_of(level, positions, &blast, Some(&mut answers), push);
            }
            ResistanceCells::Columns {
                columns,
                min_y,
                max_y,
            } => {
                let (min_y, max_y) = (*min_y, *max_y);
                let positions = columns
                    .iter()
                    .flat_map(move |&(x, z)| (min_y..=max_y).map(move |y| [x, y, z]));
                self.explosion_resistances_of(level, positions, &blast, Some(&mut answers), push);
            }
            _ => unreachable!("ResistanceCells: unhandled variant"),
        }
        let ms = started.elapsed().as_secs_f32() * 1000.0;
        if self.options.spikes.is_some_and(|threshold| ms > threshold) {
            log::info!("Sunrise: slow mod explosion resistance read {ms:.1} ms ({count} cells)");
        }
        Ok(out)
    }

    /// The resistance `blast` meets in each cell of `positions`. The blast's source and cause
    /// are set as `native_explode` sets them, and the enclosing blast's are put back after.
    fn explosion_resistances_of(
        &mut self,
        level: LevelRef,
        positions: impl Iterator<Item = [i32; 3]>,
        blast: &Blast,
        mut answers: Option<&mut PartAnswers>,
        mut output: impl FnMut(ExplosionResistance),
    ) {
        let source = match (self.level_in_place(level), blast.source) {
            (true, Some(entity)) => self.native_explosion_source(entity).map(Some),
            (true, None) => Some(None),
            _ => None,
        };
        let Some(source) = source else {
            positions.for_each(|_| output(ExplosionResistance::Unavailable));
            return;
        };
        let id = source.map(|(id, _)| id);
        let interaction = self.native_blast_interaction(blast.interaction, id);
        let outer = self.take_blast_state();
        if let Some((id, target)) = source {
            self.attribute_native_blast(id, target);
            self.native_blast_calculator(id, target);
        }
        // The calculator `level.explode` would have: the same selection, read the same way.
        let rules = self.blast_rules(None, false);
        let start = self.explosion_start(
            blast.center,
            blast.radius,
            interaction,
            blast.fire,
            self.blast_damage_type(),
        );
        for pos in positions {
            // Level.getBlockState returns void air outside build height before reading a chunk.
            if !self.world.shape.contains_y(pos[1]) {
                output(ExplosionResistance::NoResistance);
                continue;
            }
            // Reads never load a column, or inspect the described blast's centre chunk.
            if !self.world.loaded(pos[0], pos[2]) {
                output(ExplosionResistance::Unavailable);
                continue;
            }
            let state = self.world.block(pos[0], pos[1], pos[2]);
            if self.pack.assets.registry.state(state).is_none() {
                output(ExplosionResistance::Unavailable);
                continue;
            }
            let (resistance, _) =
                self.cell_explosion_resistance(pos, state, &rules, &start, answers.as_deref_mut());
            output(resistance.map_or(
                ExplosionResistance::NoResistance,
                ExplosionResistance::Resists,
            ));
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

/// How many cells `cells` covers, or why it is refused.
fn cell_count(cells: &ResistanceCells) -> Result<usize, ResistanceCellsError> {
    let span = |low: i32, high: i32| {
        if high < low {
            Err(ResistanceCellsError::Inverted)
        } else {
            Ok(u64::from(high.abs_diff(low)) + 1)
        }
    };
    let count = match cells {
        ResistanceCells::Box { min, max } => {
            let x = span(min[0], max[0])?;
            let y = span(min[1], max[1])?;
            let z = span(min[2], max[2])?;
            x.checked_mul(y).and_then(|xy| xy.checked_mul(z))
        }
        ResistanceCells::Columns {
            columns,
            min_y,
            max_y,
        } => {
            let y = span(*min_y, *max_y)?;
            u64::try_from(columns.len())
                .ok()
                .and_then(|n| n.checked_mul(y))
        }
        _ => unreachable!("ResistanceCells: unhandled variant"),
    };
    count
        .and_then(|n| usize::try_from(n).ok())
        .ok_or(ResistanceCellsError::Capacity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_counts_inclusive_cells_without_a_fixed_cap() {
        let cells = |min: [i32; 3], max: [i32; 3]| cell_count(&ResistanceCells::Box { min, max });
        assert_eq!(cells([0, 0, 0], [0, 0, 0]), Ok(1));
        assert_eq!(cells([-1, 60, -1], [1, 61, 1]), Ok(18));
        assert_eq!(cells([0, 0, 0], [63, 63, 64]), Ok(266_240));
        assert_eq!(
            cells([i32::MIN, 5, i32::MIN], [i32::MAX, 4, i32::MAX]),
            Err(ResistanceCellsError::Inverted)
        );
        assert_eq!(
            cells([i32::MIN; 3], [i32::MAX; 3]),
            Err(ResistanceCellsError::Capacity)
        );
        let columns = ResistanceCells::Columns {
            columns: vec![(0, 0), (0, 0), (5, -3)],
            min_y: -64,
            max_y: 319,
        };
        assert_eq!(cell_count(&columns), Ok(1152));
        let inverted_columns = ResistanceCells::Columns {
            columns: vec![],
            min_y: 10,
            max_y: 9,
        };
        assert_eq!(
            cell_count(&inverted_columns),
            Err(ResistanceCellsError::Inverted)
        );
    }
}
