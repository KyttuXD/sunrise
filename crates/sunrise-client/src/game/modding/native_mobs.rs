//! Mob and player equipment-change dispatch and server mob actions.
use super::{Game, commands::Target};

/// The `EquipmentSlot` names of `Mob::equipment_stacks`, in its order.
const MOB_SLOTS: [&str; 8] = [
    "mainhand", "offhand", "feet", "legs", "chest", "head", "body", "saddle",
];

impl Game<'_> {
    /// `LivingEntity.collectEquipmentChanges` for the mobs that tick: each
    /// slot that holds another stack than the mods last heard is told
    /// (`native_living_equipment_change`). A mob new to the level counts from
    /// empty, as vanilla's `lastEquipmentItems` does. Only while a mod
    /// listens (`native_living_equipment_listened`): a mob's first tick after
    /// that counts from empty too.
    pub(super) fn native_mobs_equipment(&mut self) {
        if self.remote.is_some() || self.options.execution_role.is_client() {
            return;
        }
        // Reading every mob's equipment each tick costs: only when a mod listens.
        let mut listened = false;
        self.native_living_equipment_listened(&mut listened);
        if !listened {
            return;
        }
        let ticking = self.entity_ticking_area();
        let mut changes = Vec::new();
        for m in self.mobs.iter_mut().filter(|m| !m.removed) {
            if !ticking(m.position.x.floor() as i32, m.position.z.floor() as i32) {
                continue;
            }
            let now = m.equipment_stacks();
            let heard = m.extra.equipment_heard.get_or_insert_with(Default::default);
            if **heard == now {
                continue;
            }
            for (slot, name) in MOB_SLOTS.into_iter().enumerate() {
                if now[slot] != heard[slot] {
                    let from = heard[slot].take();
                    changes.push((m.id, m.kind.id.clone(), name, from, now[slot].clone()));
                }
            }
            **heard = now;
        }
        for (id, kind, slot, from, to) in changes {
            self.native_living_equipment_change(id, &kind, slot, from.as_ref(), to.as_ref());
        }
    }
}

impl Game<'_> {
    /// `LivingEntity.collectEquipmentChanges` for the server's players (they are living entities
    /// too): each slot whose stack differs from what the mods last heard is told, the previous
    /// stack with it, so a hotbar selection that moves the main hand counts. A player new to the
    /// level counts from empty, as for mobs, and only while a mod listens.
    pub(super) fn native_players_equipment(&mut self) {
        let mut listened = false;
        self.native_living_equipment_listened(&mut listened);
        if !listened {
            return;
        }
        let Some(host) = self.lan.as_mut() else {
            return;
        };
        let mut changes = Vec::new();
        for g in host.hub.guests.iter().filter(|g| host.hub.here(g)) {
            let now: [Option<crate::items::ItemStack>; 6] = [
                g.inventory.get(g.selected).cloned().flatten(),
                g.offhand.clone(),
                g.armor[3].clone(),
                g.armor[2].clone(),
                g.armor[1].clone(),
                g.armor[0].clone(),
            ];
            let Some(play) = host.play.get_mut(&g.conn) else {
                continue;
            };
            let heard = play.equipment_heard.get_or_insert_with(Default::default);
            if **heard == now {
                continue;
            }
            for (slot, name) in PLAYER_SLOTS.into_iter().enumerate() {
                if now[slot] != heard[slot] {
                    let from = heard[slot].take();
                    changes.push((g.entity_id as u32, name, from, now[slot].clone()));
                }
            }
            **heard = now;
        }
        for (id, slot, from, to) in changes {
            self.native_living_equipment_change(
                id,
                "minecraft:player",
                slot,
                from.as_ref(),
                to.as_ref(),
            );
        }
    }
}

/// The `EquipmentSlot` names of a player's equipment, in the order of `native_players_equipment`.
const PLAYER_SLOTS: [&str; 6] = ["mainhand", "offhand", "feet", "legs", "chest", "head"];

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

impl Game<'_> {
    /// Kills mob `id` as `/kill` does: it falls and drops its loot. False for
    /// no such living mob or another dimension.
    pub(super) fn kill_mob(&mut self, dimension: &str, id: u32) -> bool {
        if self.remote.is_some()
            || dimension != self.dim.id()
            || !self
                .mobs
                .iter()
                .find(|m| m.id == id && !m.removed)
                .is_some_and(|m| m.alive())
        {
            return false;
        }
        self.kill_target(Target::Mob(id));
        true
    }
}
