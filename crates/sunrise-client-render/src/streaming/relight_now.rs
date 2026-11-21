//! Scheduling and publishing whole-column light jobs.
use super::{Done, Job, Streaming, nearest_first};
use crate::{
    chunk::Light,
    light::{LightChanges, apply_lighting, apply_relighting},
    world::World,
};
use std::sync::Arc;

impl Streaming {
    /// Whether an already submitted light job is still outstanding for this column.
    pub fn light_in_flight(&self, key: (i32, i32)) -> bool {
        self.lighting.contains_key(&key)
    }

    /// Advance the light engine of a resident level without running generation, unloads or
    /// meshing. Other worker results stay in order for the next full streaming pass.
    pub fn advance_resident_light(&mut self, world: &mut World) {
        if self.paused
            || (self.lighting.is_empty()
                && self.viewing.is_empty()
                && world.dirty_light.is_empty()
                && world.light_edits.is_empty())
        {
            return;
        }
        self.relight_edits(world);
        if !self.lighting.is_empty() || !self.viewing.is_empty() {
            let mut deferred = std::mem::take(&mut self.deferred);
            deferred.extend(self.shared.done.take());
            for done in deferred.extract_if(.., |done| {
                matches!(done, Done::Lit(..) | Done::LightView(..))
            }) {
                match done {
                    Done::LightView(x, z, proto, view) => {
                        self.receive_light_view(x, z, proto, view)
                    }
                    Done::Lit(x, z, sections, seq) => self.receive_lit(world, x, z, sections, seq),
                    _ => unreachable!("only light results were selected"),
                }
            }
            self.deferred = deferred;
        }
        self.schedule_light(world);
        if !self.mesh {
            world.dirty_sections.clear();
        }
    }

    /// Both the full streaming pass and the resident-only pass batch jobs in the same order.
    /// Keep the existing worker budget for 3x3 snapshots (#3629); pending columns consume no
    /// snapshot until submitted. `ThreadedLevelLightEngine.runUpdate` also batches execution,
    /// not admission. A re-dirtied column still occupying a worker slot waits for that result.
    pub(super) fn schedule_light(&mut self, world: &mut World) {
        let mut light = std::mem::take(&mut self.light_scratch);
        let mut gone = std::mem::take(&mut self.gone_scratch);
        for &(x, z) in &world.dirty_light {
            if !world.chunks.contains_key(&(x, z)) {
                gone.push((x, z));
            } else if !self.light_in_flight((x, z)) && self.light_region_ready(world, x, z) {
                light.push((self.distance(x, z), x, z));
            }
        }
        for key in gone.drain(..) {
            world.dirty_light.remove(&key);
        }
        let room = (self.workers * 4).saturating_sub(self.lighting.len());
        nearest_first(&mut light, room);
        for (_, x, z) in light.drain(..) {
            let Some(region) = self.light_region(world, x, z) else {
                continue;
            };
            world.dirty_light.remove(&(x, z));
            self.sequence += 1;
            self.lighting.insert((x, z), self.sequence);
            self.light_snapshots
                .insert((x, z), self.light_edit_sequence);
            self.submit(
                Some((x, z)),
                1,
                Job::Light(x, z, region, self.sequence, self.sky_light),
            );
        }
        self.light_scratch = light;
        self.gone_scratch = gone;
    }

    pub(super) fn receive_lit(
        &mut self,
        world: &mut World,
        x: i32,
        z: i32,
        sections: Vec<Light>,
        seq: u64,
    ) {
        let key = (x, z);
        if self.lighting.get(&key) != Some(&seq) {
            return;
        }
        self.lighting.remove(&key);
        let snapshot = self.light_snapshots.remove(&key).unwrap_or(u64::MAX);
        if world.dirty_light.contains(&key) {
            // This snapshot predates a new whole-column request. Do not publish it; that
            // request remains queued and may now take a new snapshot in the freed worker slot.
            self.trim_light_log();
        } else {
            self.apply_lit(world, x, z, sections, snapshot);
        }
    }

    /// Takes a lit column's light into its chunk (`Done::Lit`), `snapshot` being
    /// the edit sequence the blocks it was lit from had reached.
    pub(super) fn apply_lit(
        &mut self,
        world: &mut World,
        x: i32,
        z: i32,
        sections: Vec<Light>,
        snapshot: u64,
    ) {
        if let Some(chunk) = world.chunks.get_mut(&(x, z)) {
            let chunk = Arc::make_mut(chunk);
            let before = chunk.light_support;
            // Keep blocks edited since the snapshot; take only light.
            // A chunk relit (a neighbour loaded or changed) remeshes
            // only around the light that changed, as vanilla's light
            // updates dirty only their sections (#270).
            let first = self.lit.insert((x, z));
            let mut changed = LightChanges::default();
            if first {
                apply_lighting(chunk, sections);
                self.light_tables.publish_section_status(chunk, world.shape);
            } else {
                apply_relighting(chunk, sections, &mut changed);
            }
            // Saved unlit (at the edge of what was loaded), or its light
            // changed: saved again.
            if (first && !chunk.light_loaded) || !changed.is_empty() {
                world.modified.insert((x, z));
            }
            chunk.lit = true;
            if first {
                world.publish_light_border((x, z), before);
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        for sy in 0..world.layout().sections() as i32 {
                            world.dirty_sections.insert((x + dx, sy, z + dz));
                        }
                    }
                }
            } else {
                Self::mark_light_changes(world, &changed);
            }
            self.replay_light_edits(world, x, z, snapshot);
        }
    }
}
