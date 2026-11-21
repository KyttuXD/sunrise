//! Mobs for native server mods (docs/modding/src/how-to/native-mods.md, "Mobs"): spawn one,
//! read the mobs of the level in place, and move, hurt or remove one by its id.
//! They act where the world is simulated, never on a client's copy, and only on
//! the level in place, which each names by its `dimension` (`minecraft:overworld`)
//! as `native_set_block` does: another one is refused. While the game ticks its
//! mobs they are out of the level: a mob a hook spawns then joins it after the
//! tick, and the others read as gone.
use super::api_values::{project_stack, stack_read_metadata};
use super::native_living_refs::player_slot;
use super::native_ticks::place;
use super::{Game, commands::Target};
use mod_api::{EntityRef, EquipmentSlot, HookId, ItemStackView, LivingRef};
use sunrise_client::mobs::finalize::{EquipmentRead, equipment_slot};
use sunrise_items::equipment::Equipment;
use sunrise_items::items::ItemStack;

impl Game<'_> {
    /// `LivingEntity.collectEquipmentChanges`: compare in slot order and lend
    /// each difference before looking at the next slot. `read` returns `None`
    /// if the subject has left, or `Some(None)` for a slot that is empty.
    /// Only the new historical value is owned across dispatch: the callback
    /// borrows it and the previous value, even if it edits the live subject.
    pub(super) fn observe_equipment(
        &mut self,
        living: Option<LivingRef>,
        history: &mut Option<Box<Equipment>>,
        mut read: impl for<'g> FnMut(&'g Self, EquipmentSlot) -> Option<Option<EquipmentRead<'g>>>,
    ) -> [bool; EquipmentSlot::ALL.len()] {
        let mut changed = [false; EquipmentSlot::ALL.len()];
        for slot in EquipmentSlot::ALL {
            let index = slot as usize;
            let Some(current) = read(self, slot) else {
                break;
            };
            let previous = history.as_deref().and_then(|worn| worn.get(index));
            if EquipmentRead::matches(current.as_ref(), previous) {
                continue;
            }
            changed[index] = true;
            let current = current.map(|stack| stack.project().into_owned());
            if let Some(living) = living {
                self.dispatch_living_equipment_change(living, slot, previous, current.as_ref());
            }
            history.get_or_insert_with(Default::default)[index] = current;
        }
        if history.as_deref().is_some_and(Equipment::is_empty) {
            *history = None;
        }
        changed
    }

    /// Lend the retained stacks using the held-stack API's lazy projection.
    fn dispatch_living_equipment_change(
        &mut self,
        entity: LivingRef,
        slot: EquipmentSlot,
        from: Option<&ItemStack>,
        to: Option<&ItemStack>,
    ) {
        let report = |error: &super::api_values::StackReadError| {
            log::warn!(
                "living_equipment_change for entity {} in {slot:?}: {error:?}",
                entity.id(),
            );
        };
        let metadata = |stack: Option<&ItemStack>| stack.map(stack_read_metadata).transpose();
        let (from_metadata, to_metadata) = match (metadata(from), metadata(to)) {
            (Ok(from), Ok(to)) => (from, to),
            (Err(error), _) | (_, Err(error)) => {
                report(&error);
                return;
            }
        };
        let project_from = |metadata| project_stack(from?, metadata).inspect_err(report).ok();
        let project_to = |metadata| project_stack(to?, metadata).inspect_err(report).ok();
        let from_view =
            from_metadata.map(|metadata| ItemStackView::resident(metadata, &project_from));
        let to_view = to_metadata.map(|metadata| ItemStackView::resident(metadata, &project_to));
        self.native_living_equipment_change(mod_api::__private::LivingEquipmentChangeCall {
            entity,
            slot,
            from: from_view.as_ref(),
            to: to_view.as_ref(),
        });
    }

    /// A mob paused its living tick at the first difference. The live row is
    /// back in the level; only changed slots need to be copied into history.
    pub(super) fn native_mob_equipment(
        &mut self,
        id: u32,
        mut index: usize,
        history: &mut Option<Box<Equipment>>,
    ) {
        let Some(epoch) = self.native_lifecycle.epoch() else {
            return;
        };
        let living = LivingRef::new(EntityRef::new(epoch, id));
        self.observe_equipment(Some(living), history, |game, slot| {
            index = place(&game.mobs, index, |m| m.id == id && !m.removed)?;
            Some(equipment_slot(&game.mobs[index], slot))
        });
    }

    /// A connected player's `LivingEntity.detectEquipmentUpdates` during its
    /// connection's living tick. Read the selected inventory slot as `MainHand` and
    /// the rest through the same slot lookup as `LivingRef::with_equipment`.
    pub(super) fn native_player_equipment(&mut self, conn: u32) {
        if self.remote.is_some()
            || self.options.execution_role.is_client()
            || !mod_api::__private::enabled_listened(HookId::LivingEquipmentChange)
        {
            return;
        }
        let Some(epoch) = self.native_lifecycle.epoch() else {
            return;
        };
        let Some(host) = self.lan.as_mut() else {
            return;
        };
        let Some(guest) = host.hub.guest(conn).filter(|g| host.hub.here(g)) else {
            return;
        };
        let id = guest.entity_id;
        let Some(play) = host.play.get_mut(&conn) else {
            return;
        };
        let mut history = play.equipment_heard.take();
        let living = LivingRef::new(EntityRef::new(epoch, id as u32));
        self.observe_equipment(Some(living), &mut history, |game, slot| {
            let host = game.lan.as_ref()?;
            let guest = host
                .hub
                .guest(conn)
                .filter(|g| g.entity_id == id && host.hub.here(g))?;
            Some(
                player_slot(
                    &guest.inventory,
                    guest.selected,
                    &guest.armor,
                    &guest.offhand,
                    &guest.equipment,
                    slot,
                )
                .map(EquipmentRead::resident),
            )
        });
        if let Some(host) = self.lan.as_mut()
            && host.hub.guest(conn).is_some_and(|g| g.entity_id == id)
            && let Some(play) = host.play.get_mut(&conn)
        {
            play.equipment_heard = history;
        }
    }
}

/// A mob as a native mod reads it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeMob {
    pub(crate) id: u32,
    /// The level it is in (`minecraft:overworld`).
    pub(crate) dimension: String,
    /// `minecraft:zombie`.
    pub(crate) kind: String,
    pub(crate) pos: [f64; 3],
    /// Degrees; 0 faces south.
    pub(crate) yaw: f32,
    pub(crate) health: f32,
    pub(crate) max_health: f32,
}

/// What a mob starts with (`native_spawn_mob`); the default is a plain mob.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NativeSpawn {
    /// A name over it, shown to everyone.
    pub(crate) name: Option<String>,
    /// Cut to its maximum.
    pub(crate) health: Option<f32>,
    pub(crate) no_ai: bool,
    pub(crate) invisible: bool,
    pub(crate) silent: bool,
    pub(crate) invulnerable: bool,
    pub(crate) no_gravity: bool,
}

fn finite(v: [f64; 3]) -> bool {
    v.into_iter().all(f64::is_finite)
}

/// `/summon`'s world bounds (`Level.isInSpawnableBounds`).
pub(super) fn spawnable(pos: [f64; 3]) -> bool {
    finite(pos)
        && (-30_000_000.0..30_000_000.0).contains(&pos[0])
        && (-30_000_000.0..30_000_000.0).contains(&pos[2])
        && (-20_000_000.0..20_000_000.0).contains(&pos[1].floor())
}

// Native mods use these; nothing in Sunrise does.
#[allow(dead_code, reason = "native mod API (docs/native-mods.md)")]
impl Game<'_> {
    /// Whether this is a level that simulates (not a client's copy) and the one
    /// in place is `dimension`.
    fn native_mob_level(&self, dimension: &str) -> bool {
        self.remote.is_none() && dimension == self.dim.id()
    }
    /// Spawns a mob of `kind` (`minecraft:zombie`) at `pos` in the level in
    /// place, `dimension`, finalized as a natural spawn is, and kept when the
    /// chunk unloads: its id. None for a kind the game doesn't know, another
    /// dimension, a position outside `/summon`'s bounds or in a chunk that isn't
    /// loaded (it would never tick or be saved), or a monster on peaceful (it
    /// would despawn at once).
    pub(super) fn native_spawn_mob(
        &mut self,
        dimension: &str,
        kind: &str,
        pos: [f64; 3],
        options: &NativeSpawn,
    ) -> Option<u32> {
        if !self.native_mob_level(dimension) || !spawnable(pos) || !self.native_chunk_loaded(pos) {
            return None;
        }
        let qualified = if kind.contains(':') {
            kind.to_owned()
        } else {
            ["minecraft:", kind].concat()
        };
        let monster = self
            .res
            .mob_types()
            .get(&qualified)
            .is_some_and(|t| t.category == "monster");
        if monster && self.ui.difficulty == 0 {
            return None;
        }
        let id = self.spawn_mob(
            kind,
            pos,
            true,
            super::creatures::SpawnSite::Finalizing(
                sunrise_client::mobs::spawn_rules::Reason::Command,
            ),
        )?;
        // A jockey's other mob comes after it: this one is found by its id.
        if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
            m.no_ai = options.no_ai;
            m.invisible = options.invisible;
            m.silent = options.silent;
            m.invulnerable_flag = options.invulnerable;
            m.no_gravity = options.no_gravity;
            if let Some(name) = &options.name {
                m.custom_name = Some(name.clone());
                // A plain name replaces any styled one the spawn gave.
                m.custom_name_text = None;
                m.name_visible = true;
            }
            if let Some(health) = options.health.filter(|h| h.is_finite() && *h > 0.0) {
                m.health = health.min(m.attrs.max_health);
            }
        }
        Some(id)
    }
    /// Whether the chunk holding `pos` is loaded in the level in place.
    fn native_chunk_loaded(&self, pos: [f64; 3]) -> bool {
        let (x, z) = (pos[0].floor() as i32, pos[2].floor() as i32);
        self.world.chunks.contains_key(&(x >> 4, z >> 4))
    }
    /// The living mob `id` in the level in place, `dimension`; none on a
    /// client's copy.
    pub(super) fn native_mob(&self, dimension: &str, id: u32) -> Option<NativeMob> {
        if !self.native_mob_level(dimension) {
            return None;
        }
        let mob = self.mobs.iter().find(|m| m.id == id && !m.removed)?;
        mob.alive().then(|| self.native_mob_of(mob))
    }
    /// The living mobs in the level in place, `dimension`; none on a client's
    /// copy.
    pub(super) fn native_mobs(&self, dimension: &str) -> Vec<NativeMob> {
        if !self.native_mob_level(dimension) {
            return Vec::new();
        }
        self.mobs
            .iter()
            .filter(|m| !m.removed && m.alive())
            .map(|m| self.native_mob_of(m))
            .collect()
    }
    /// Kills mob `id` as `/kill` does: it falls and drops its loot. False for
    /// no such living mob or another dimension.
    pub(super) fn kill_mob(&mut self, dimension: &str, id: u32) -> bool {
        if self.native_mob(dimension, id).is_none() {
            return false;
        }
        self.kill_target(Target::Mob(id));
        true
    }
    fn native_mob_of(&self, m: &sunrise_client::mobs::Mob) -> NativeMob {
        NativeMob {
            id: m.id,
            dimension: self.dim.id().to_owned(),
            kind: sunrise_client::command::args::normalize(&m.kind.id),
            pos: [m.position.x, m.position.y, m.position.z],
            yaw: m.yaw,
            health: m.health,
            max_health: m.attrs.max_health,
        }
    }
}
