//! Mobs for native server mods (docs/modding/src/how-to/native-mods.md, "Mobs"): spawn one,
//! read the mobs of the level in place, and move, hurt or remove one by its id.
//! They act where the world is simulated, never on a client's copy, and only on
//! the level in place, which each names by its `dimension` (`minecraft:overworld`)
//! as `native_set_block` does: another one is refused. While the game ticks its
//! mobs they are out of the level: a mob a hook spawns then joins it after the
//! tick, and the others read as gone.
use super::api_values::{StackReadMetadata, project_stack, stack_read_metadata};
use super::{Game, commands::Target};
use mod_api::{EquipmentSlot, HookId, ItemStackView, LivingRef, WorldEpoch};
use sunrise_client::items::ItemStack;
use sunrise_client::lan::guest::Guest;
use sunrise_client::mobs::Mob;
use sunrise_items::equipment::{BODY, Equipment, SADDLE};

/// `LivingEntity.equipmentHasChanged` negated (`ItemStack.matches`).
pub(super) fn equipment_stack_matches(now: Option<&ItemStack>, prev: Option<&ItemStack>) -> bool {
    match (now, prev) {
        (None, None) => true,
        (Some(now), Some(prev)) => now.matches(prev),
        _ => false,
    }
}

pub(super) fn mob_index_by_hint(mobs: &[Mob], hint: usize, id: u32) -> Option<usize> {
    if mobs.get(hint).is_some_and(|m| m.id == id) {
        return Some(hint);
    }
    mobs.iter().position(|m| m.id == id)
}

pub(super) fn guest_equipment_slot(g: &Guest, slot: EquipmentSlot) -> Option<&ItemStack> {
    match slot {
        EquipmentSlot::MainHand => g.inventory.get(g.selected)?.as_ref(),
        EquipmentSlot::OffHand => g.offhand.as_ref(),
        EquipmentSlot::Feet => g.armor[3].as_ref(),
        EquipmentSlot::Legs => g.armor[2].as_ref(),
        EquipmentSlot::Chest => g.armor[1].as_ref(),
        EquipmentSlot::Head => g.armor[0].as_ref(),
        EquipmentSlot::Body => g.equipment[BODY].as_ref(),
        EquipmentSlot::Saddle => g.equipment[SADDLE].as_ref(),
    }
}

pub(super) fn guest_equipment_changed(g: &Guest, last: Option<&Equipment>) -> bool {
    EquipmentSlot::ALL.into_iter().any(|slot| {
        let current = guest_equipment_slot(g, slot);
        let prev = last.and_then(|last| last[slot as usize].as_ref());
        !equipment_stack_matches(current, prev)
    })
}

/// Retain only the slots needed for vanilla's `lastEquipmentItems` comparison,
/// freeing the boxed table once every slot is empty again.
pub(super) fn store_equipment_history(
    history: &mut Option<Box<Equipment>>,
    index: usize,
    to: Option<ItemStack>,
) {
    match to {
        Some(stack) => {
            history.get_or_insert_with(Default::default)[index] = Some(stack);
        }
        None => {
            if let Some(slots) = history.as_deref_mut() {
                slots[index] = None;
                if slots.is_empty() {
                    *history = None;
                }
            }
        }
    }
}

fn equipment_view_metadata(stack: Option<&ItemStack>) -> Option<StackReadMetadata> {
    let stack = stack.filter(|s| s.count > 0 && &*s.item != "minecraft:air")?;
    match stack_read_metadata(stack) {
        Ok(metadata) => Some(metadata),
        Err(error) => {
            log::warn!(
                "living_equipment_change: omitting unrepresentable stack '{}' (count {}): {error:?}",
                stack.item,
                stack.count,
            );
            None
        }
    }
}

impl Game<'_> {
    /// The active server epoch when an enabled mod subscribes to equipment changes.
    pub(super) fn living_equipment_notify_epoch(&self) -> Option<WorldEpoch> {
        if self.remote.is_some()
            || self.options.execution_role.is_client()
            || !mod_api::__private::enabled_listened(HookId::LivingEquipmentChange)
        {
            return None;
        }
        self.native_lifecycle.epoch()
    }

    /// Lend `from` and `to` lazily to `living_equipment_change` without eagerly
    /// cloning stacks or encoding component patches.
    pub(super) fn dispatch_living_equipment_change(
        &mut self,
        entity: LivingRef,
        slot: EquipmentSlot,
        from: Option<&ItemStack>,
        to: Option<&ItemStack>,
    ) {
        let from_metadata = equipment_view_metadata(from);
        let to_metadata = equipment_view_metadata(to);
        let project_from = |metadata| from.and_then(|stack| project_stack(stack, metadata).ok());
        let project_to = |metadata| to.and_then(|stack| project_stack(stack, metadata).ok());
        let from_view =
            from_metadata.map(|metadata| ItemStackView::resident(metadata, &project_from));
        let to_view = to_metadata.map(|metadata| ItemStackView::resident(metadata, &project_to));
        self.native_living_equipment_change(mod_api::__private::LivingEquipmentChangeCall {
            entity,
            slot,
            from: from_view,
            to: to_view,
        });
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
