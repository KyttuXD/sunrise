//! Observe the complete species invulnerability predicate once per living hit attempt.
use super::*;

impl Mob {
    pub(super) fn remember_accepted_hit(
        &mut self,
        level: &DamageLevel<'_>,
        source: &Damage,
        accepted: bool,
    ) {
        if accepted {
            self.death_canceled = false;
            self.accepted_hit = level.accepted_hit(crate::health::Hurting::Mob(self, source));
        }
    }
    /// WitherBoss refuses its friends after asking isInvulnerableTo, not at the caller.
    pub(super) fn attacker_allows_damage(&self, source: &Damage) -> bool {
        !source
            .attacker_species
            .is_some_and(|attacker| wither::shielded_from(self.species(), attacker))
    }

    /// Returns the observed immunity and whether a listener explicitly allowed
    /// a hit that vanilla's outer predicate refused.
    pub(super) fn invulnerable_to_attempt(
        &self,
        level: &mut DamageLevel<'_>,
        source: &Damage,
    ) -> (bool, bool) {
        let bypasses = source.kind.bypasses_invulnerability();
        let reflected = self.reflected_fireball(source, level.attack());
        let flagged = self.is_invulnerable_to(source);
        let digging =
            self.species() == Species::Warden && warden::digging_or_emerging(self) && !bypasses;
        // Ghast's override can short-circuit the base predicate. Its final answer is still
        // the one observation for this attempt, including a reflected fireball.
        let vanilla = digging
            || if self.species() == Species::Ghast && (flagged || reflected) {
                self.removed || flagged
            } else {
                self.removed
                    || flagged
                    || (source.is_fire() && self.fire_immune() && !reflected)
                    || self.enchantment_immune(level, source)
            };
        let immune =
            level.invulnerability_check(crate::health::Hurting::Mob(self, source), vanilla);
        (immune, vanilla && !immune)
    }

    /// `EnchantmentHelper.isImmuneToDamage`: consult active slots in vanilla's
    /// equipment order, stopping at the first matching damage-immunity effect.
    pub(super) fn enchantment_immune(&self, level: &mut DamageLevel<'_>, source: &Damage) -> bool {
        use sunrise_items::equipment::{BODY, FEET, HEAD, OFFHAND, SLOT_NAMES};

        let Some(items) = level.tags() else {
            return false;
        };
        if self.extra.equipment.is_empty() {
            return false;
        }
        let attack = level.attack();
        let is_direct = attack.is_direct(source);
        let attacker = match source.attacker {
            Some(Who::Player) => Some("minecraft:player"),
            Some(Who::Mob(_)) => source.attacker_species.map(species::vanilla_id),
            None => None,
        };
        let direct_attacker = if is_direct {
            attacker
        } else if attack.direct_is_large_fireball(source) {
            Some("minecraft:fireball")
        } else {
            None
        };
        let context = crate::enchant::EffectContext {
            items: Some(items),
            damage_type: Some(crate::health::source_damage_type(*source)),
            is_direct,
            this: Some(&self.kind.id),
            attacker,
            direct_attacker,
            ..Default::default()
        };
        let gear = gear::gear(self);
        for (slot, name) in SLOT_NAMES.into_iter().enumerate() {
            let Some(stack) = self
                .extra
                .slot_stack(slot)
                .filter(|stack| stack.count != 0 && &*stack.item != "minecraft:air")
            else {
                continue;
            };
            let levels = gear
                .filter(|_| slot <= HEAD)
                .map(|gear| gear.slots[finalize::gear_slot(slot)].as_slice())
                .filter(|levels| !levels.is_empty())
                .unwrap_or_else(|| stack.enchantments());
            for (id, rank) in levels {
                let Some(enchantment) = items.enchantments.get(id) else {
                    continue;
                };
                if enchantment.slots.iter().any(|group| {
                    group == name
                        || group == "any"
                        || (group == "hand" && slot <= OFFHAND)
                        || (group == "armor" && (FEET..=BODY).contains(&slot))
                }) && enchantment.effects["minecraft:damage_immunity"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|effect| {
                        effect.get("requirements").is_none_or(|requirements| {
                            items.enchantments.condition(
                                requirements,
                                *rank,
                                &context,
                                level.random(),
                            )
                        })
                    })
                {
                    return true;
                }
            }
        }
        false
    }
}
