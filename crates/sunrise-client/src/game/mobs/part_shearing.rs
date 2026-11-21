//! Shears and entity parts (NeoForge's `IShearable`, #3351): a player's shears and a dispenser's
//! ask the mob's entity parts first (`EntityPart::is_shearable`), and a part that says yes
//! shears it (`EntityPart::on_sheared`) in place of the species' own shearing.
use super::api_values::{TryIntoEngine, project_stack, stack_read_metadata};
use super::{Game, Hand};
use mod_api::{Attached, EntityPart, ItemStackView, PlayerRef};
use std::sync::Arc;
use sunrise_client::{
    items::ItemStack,
    mobs::{Species, Who},
};
use sunrise_core::{geometry::Vec3, random::RandomSource};

type ShearingPart = Attached<dyn EntityPart>;

impl Game<'_> {
    /// The first subscribed Part to answer, in dispatch order. With no matching subscription
    /// this does not look up the entity handle or inspect the shears.
    pub(super) fn part_shearable(
        &self,
        index: usize,
        player: Option<PlayerRef>,
        shears: &ItemStack,
    ) -> Option<(ShearingPart, bool)> {
        let mob = &self.mobs[index];
        if !mob.alive() {
            return None;
        }
        mod_api::dispatch_is_shearable(&mob.kind.id, |mut parts| {
            let entity = self.entity(mob.id)?;
            let metadata = stack_read_metadata(shears)
                .inspect_err(|error| log::warn!("is_shearable could not lend shears: {error:?}"))
                .ok()?;
            let project = |metadata| project_stack(shears, metadata).ok();
            parts.find_map(|part| {
                let registration = part.registration;
                let answer = part.invoke(|part| {
                    part.is_shearable(
                        self,
                        entity,
                        player,
                        ItemStackView::resident(metadata, &project),
                    )
                })??;
                Some((registration.clone(), answer))
            })
        })?
    }

    /// Invoke the same Part that accepted the shear, then place its drops. A callback that
    /// refused or panicked is not a successful shear, even when it returned no drops.
    pub(super) fn part_shear(
        &mut self,
        index: usize,
        winner: &ShearingPart,
        player: Option<PlayerRef>,
        shears: &ItemStack,
    ) -> bool {
        let (id, at, kind) = (
            self.mobs[index].id,
            self.mobs[index].position,
            self.mobs[index].kind.clone(),
        );
        let drops = mod_api::dispatch_on_sheared(&kind.id, |mut parts| {
            let Some(part) =
                parts.find(|part| Arc::ptr_eq(part.registration.part(), winner.part()))
            else {
                // An omitted on_sheared does nothing; it does not give another Part the action.
                return Some(Vec::new());
            };
            let entity = self.entity(id)?;
            let metadata = stack_read_metadata(shears)
                .inspect_err(|error| {
                    log::warn!(
                        "mod {}: on_sheared could not lend shears: {error:?}",
                        winner.mod_id()
                    )
                })
                .ok()?;
            let project = |metadata| project_stack(shears, metadata).ok();
            part.invoke(|part| {
                part.on_sheared(
                    self,
                    entity,
                    player,
                    ItemStackView::resident(metadata, &project),
                )
            })
        });
        let Some(drops) = drops.unwrap_or_else(|| Some(Vec::new())) else {
            return false;
        };
        // The part may have moved or removed the mob: its drops fall where it is now.
        let index = self.mobs.iter().position(|m| m.id == id && !m.removed);
        let at = index.map_or(at, |i| self.mobs[i].position);
        for drop in drops {
            let stack = match drop.try_into_engine(self.items()) {
                Ok(stack) => stack,
                Err(error) => {
                    log::warn!(
                        "mod {}: on_sheared drop refused: {error:?}",
                        winner.mod_id()
                    );
                    continue;
                }
            };
            self.spawn_at_location(at, 1.0, stack);
            let Some(i) = index else { continue };
            let r = &mut self.mobs[i].random;
            let push = [
                f64::from((r.next_float() - r.next_float()) * 0.1),
                f64::from(r.next_float() * 0.05),
                f64::from((r.next_float() - r.next_float()) * 0.1),
            ];
            if let Some(item) = self.just_spawned_item() {
                let v = item.velocity;
                item.velocity = Vec3::new(v.x + push[0], v.y + push[1], v.z + push[2]);
            }
        }
        true
    }

    /// Shears in the acting player's hand on mob `index` (`ShearsItem.interactLivingEntity`):
    /// the entity parts' answer, else the species' own (`Sheep`, `MushroomCow`, `SnowGolem`,
    /// `Bogged`), and a shearable mob fires `player_shear_entity`. `Some(consumed)` ends the
    /// interaction: a part's refusal (false), the hook's cancel or a part's shearing (true);
    /// none leaves the shearing to the species' own interaction.
    pub(super) fn player_shear(&mut self, mut index: usize) -> Option<bool> {
        let player = self.native_acting_player();
        let part = self.part_shearable(index, player, self.held()?);
        if part.as_ref().is_some_and(|(_, shearable)| !shearable) {
            return Some(false);
        }
        let mob = &self.mobs[index];
        let (id, at) = (mob.id, mob.position);
        let shearable = part.is_some()
            || match mob.species() {
                Species::Sheep => !mob.extra.sheared && !mob.is_baby(),
                Species::Mooshroom => !mob.is_baby(),
                Species::SnowGolem | Species::Bogged => mob.alive() && !mob.extra.sheared,
                _ => false,
            };
        if !shearable {
            return None;
        }
        let shears = self.held().cloned()?;
        if let (Some(uuid), Some(name)) = (self.lan_acting_uuid, self.lan_acting_name.clone()) {
            let mut cancel = false;
            let off_hand = self.hand == Hand::Off;
            self.native_player_shear_entity(uuid, &name, id as i32, off_hand, &mut cancel);
            if cancel {
                return Some(true);
            }
            // The listener had the game in its hands: find the mob again by
            // its id, and it must still be there, not removed, with the same
            // shears in hand. Otherwise the click is spent and nothing is
            // sheared.
            match self.mobs.iter().position(|m| m.id == id) {
                Some(now)
                    if !self.mobs[now].removed
                        && Self::same_item_and_count(self.held(), Some(&shears)) =>
                {
                    index = now;
                }
                _ => return Some(true),
            }
        }
        let (part, _) = part?;
        if !self.part_shear(index, &part, player, &shears) {
            return Some(false);
        }
        let ctx = self.who_context(Some(Who::Player), None);
        self.post_game_event("minecraft:shear", [at.x, at.y, at.z], ctx);
        self.damage_held_tool();
        self.swing_arm();
        Some(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sunrise_client::mobs::Mob;

    /// A cow that shears into one leather.
    struct LeatherCow;
    #[mod_api::sunrise_part]
    impl mod_api::EntityPart for LeatherCow {
        fn is_shearable(
            &self,
            _: &dyn mod_api::Host,
            _: mod_api::EntityRef,
            player: Option<PlayerRef>,
            shears: mod_api::ItemStackView<'_>,
        ) -> Option<bool> {
            assert!(player.is_none(), "a dispenser has no player");
            assert_eq!((shears.count(), shears.damage()), (1, 7));
            Some(true)
        }
        fn on_sheared(
            &self,
            _: &mut dyn mod_api::ServerHost,
            _: mod_api::EntityRef,
            _: Option<PlayerRef>,
            shears: mod_api::ItemStackView<'_>,
        ) -> Vec<mod_api::ItemStack> {
            assert_eq!(shears.damage(), 7);
            let leather = mod_api::__private::item_key("minecraft:leather").unwrap();
            vec![mod_api::ItemStack::new(leather, 1).unwrap()]
        }
    }
    /// A sheep that refuses the shears.
    struct WoollySheep;
    #[mod_api::sunrise_part]
    impl mod_api::EntityPart for WoollySheep {
        fn is_shearable(
            &self,
            _: &dyn mod_api::Host,
            _: mod_api::EntityRef,
            _: Option<PlayerRef>,
            _: mod_api::ItemStackView<'_>,
        ) -> Option<bool> {
            Some(false)
        }
    }
    struct UnselectedAction;
    #[mod_api::sunrise_part]
    impl EntityPart for UnselectedAction {
        fn on_sheared(
            &self,
            _: &mut dyn mod_api::ServerHost,
            _: mod_api::EntityRef,
            _: Option<PlayerRef>,
            _: ItemStackView<'_>,
        ) -> Vec<mod_api::ItemStack> {
            panic!("an action without the winning readiness answer must not run");
        }
    }
    struct ShearMod;
    #[mod_api::__subscriptions]
    impl mod_api::Hooks for ShearMod {
        fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
            r.entity_part("minecraft:cow", UnselectedAction);
            r.entity_part("minecraft:cow", LeatherCow);
            r.entity_part("minecraft:sheep", WoollySheep);
        }
    }

    /// A dispenser's shears ask the mob's entity parts: a cow whose part says yes drops what
    /// the part returns, and a woolly sheep whose part says no keeps its wool.
    #[test]
    #[ignore = "daily"]
    fn entity_parts_decide_and_perform_a_dispensers_shearing() {
        use super::super::native_entity_refs::tests::{run_fresh_cases, with_game};
        if run_fresh_cases(
            "game::part_shearing::tests::entity_parts_decide_and_perform_a_dispensers_shearing",
            "SUNRISE_PART_SHEARING_CHILD",
            &["valid"],
        ) {
            return;
        }
        let entries = vec![mod_api::ModEntry::new("shearing", Box::new(ShearMod))];
        mod_platform::__private::install(entries.into());
        crate::game::api_values::install_parts(&[mod_api::ModEntry::new(
            "shearing",
            Box::new(ShearMod),
        )])
        .unwrap();
        with_game(|game| {
            game.resolve_parts().unwrap();
            // The mobs below take ids 1 and 2 by hand: reserve them in the game's allocator, or the
            // first drop is given id 1 too.
            game.next_entity = game.next_entity.max(2);
            let mut add = |id: u32, kind: &str, x: i32| {
                let kind = game.res.mob_types().get(kind).unwrap().clone();
                let at = Vec3::new(f64::from(x) + 0.5, 64.0, 0.5);
                game.mobs.push(Mob::new(id, kind, at, 0.0));
            };
            add(1, "minecraft:cow", 0);
            add(2, "minecraft:sheep", 4);

            let mut shears = ItemStack::new("minecraft:shears", 1);
            shears.damage = 7;
            assert!(
                game.shear_entity((0, 64, 0), &shears).unwrap(),
                "the cow is sheared"
            );
            let leather: Vec<_> = game
                .dropped
                .iter()
                .filter(|d| &*d.stack.item == "minecraft:leather")
                .map(|d| d.stack.count)
                .collect();
            assert_eq!(leather, [1]);

            assert!(
                !game
                    .shear_entity(
                        (4, 64, 0),
                        &sunrise_client::items::ItemStack::new("minecraft:shears", 1)
                    )
                    .unwrap(),
                "the sheep refuses"
            );
            assert!(!game.mobs.iter().any(|m| m.id == 2 && m.extra.sheared));
            assert!(!game.dropped.iter().any(|d| d.stack.item.ends_with("_wool")));
        });
    }

    /// A cow whose part reads the world through the Host, as a mod's part may.
    struct GatedCow;
    #[mod_api::sunrise_part]
    impl mod_api::EntityPart for GatedCow {
        fn is_shearable(
            &self,
            host: &dyn mod_api::Host,
            _: mod_api::EntityRef,
            _: Option<PlayerRef>,
            _: mod_api::ItemStackView<'_>,
        ) -> Option<bool> {
            let _ = host.level("minecraft:overworld");
            Some(true)
        }
        fn on_sheared(
            &self,
            host: &mut dyn mod_api::ServerHost,
            _: mod_api::EntityRef,
            _: Option<PlayerRef>,
            _: mod_api::ItemStackView<'_>,
        ) -> Vec<mod_api::ItemStack> {
            let _ = host.level("minecraft:overworld");
            let leather = mod_api::__private::item_key("minecraft:leather").unwrap();
            vec![mod_api::ItemStack::new(leather, 1).unwrap()]
        }
    }
    /// A sheep whose part agrees to shear without a Host call, then reads the world in `on_sheared`:
    /// the shearing starts and its performing is the refused call.
    struct GatedSheep;
    #[mod_api::sunrise_part]
    impl mod_api::EntityPart for GatedSheep {
        fn is_shearable(
            &self,
            _: &dyn mod_api::Host,
            _: mod_api::EntityRef,
            _: Option<PlayerRef>,
            _: mod_api::ItemStackView<'_>,
        ) -> Option<bool> {
            Some(true)
        }
        fn on_sheared(
            &self,
            host: &mut dyn mod_api::ServerHost,
            _: mod_api::EntityRef,
            _: Option<PlayerRef>,
            _: mod_api::ItemStackView<'_>,
        ) -> Vec<mod_api::ItemStack> {
            let _ = host.level("minecraft:overworld");
            Vec::new()
        }
    }
    struct GatedMod;
    #[mod_api::__subscriptions]
    impl mod_api::Hooks for GatedMod {
        fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
            r.entity_part("minecraft:cow", GatedCow);
            r.entity_part("minecraft:sheep", GatedSheep);
        }
    }

    /// Entity parts run in ticks, after the thread reached World. Fired in Setup and in Reloading anyway,
    /// a part's world read is a contained refusal (no answer, no drops), never the no-scope panic; in World
    /// the same part shears.
    #[test]
    #[ignore = "daily"]
    fn a_part_shearing_outside_the_world_is_refused_not_a_panic() {
        use super::super::native_entity_refs::tests::{run_fresh_cases, with_game};
        use mod_api::__private::{ModPhase, PhaseOwner};
        if run_fresh_cases(
            "game::part_shearing::tests::a_part_shearing_outside_the_world_is_refused_not_a_panic",
            "SUNRISE_PART_SHEARING_PHASE_CHILD",
            &["valid"],
        ) {
            return;
        }
        let entries = vec![mod_api::ModEntry::new("gated", Box::new(GatedMod))];
        mod_platform::__private::install(entries.into());
        crate::game::api_values::install_parts(&[mod_api::ModEntry::new(
            "gated",
            Box::new(GatedMod),
        )])
        .unwrap();
        with_game(|game| {
            game.resolve_parts().unwrap();
            game.next_entity = game.next_entity.max(2);
            let kind = game.res.mob_types().get("minecraft:cow").unwrap().clone();
            game.mobs
                .push(Mob::new(1, kind, Vec3::new(0.5, 64.0, 0.5), 0.0));
            let sheep = game.res.mob_types().get("minecraft:sheep").unwrap().clone();
            game.mobs
                .push(Mob::new(2, sheep, Vec3::new(4.5, 64.0, 0.5), 0.0));
            let shears = sunrise_client::items::ItemStack::new("minecraft:shears", 1);
            for reloading in [false, true] {
                let setup = PhaseOwner::scope(ModPhase::Setup);
                let _reload = reloading.then(|| setup.reloading().unwrap());
                assert!(game.part_shearable(0, None, &shears).is_none());
                assert!(game.dropped.is_empty(), "a refused part drops nothing");
                // The dispenser's shearing is not a success when the part's performing is refused.
                assert!(
                    !game
                        .shear_entity(
                            (4, 64, 0),
                            &sunrise_client::items::ItemStack::new("minecraft:shears", 1)
                        )
                        .unwrap(),
                    "a refused on_sheared shears nothing"
                );
                assert!(!game.mobs[1].extra.sheared);
            }
            assert_eq!(
                game.part_shearable(0, None, &shears)
                    .map(|(_, answer)| answer),
                Some(true)
            );
        });
    }
}
