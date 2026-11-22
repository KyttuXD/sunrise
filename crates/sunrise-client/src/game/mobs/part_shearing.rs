//! Shears and entity parts (NeoForge's `IShearable`, #3351): a player's shears and a dispenser's
//! ask the mob's entity parts first (`EntityPart::is_shearable`), and a part that says yes
//! shears it (`EntityPart::on_sheared`) in place of the species' own shearing.
use super::api_values::TryIntoEngine;
use super::{Game, Hand};
use mod_api::PlayerRef;
use sunrise_client::{
    items::ItemStack,
    mobs::{Species, Who},
};
use sunrise_core::{geometry::Vec3, random::RandomSource};

impl Game<'_> {
    /// `IShearable.isShearable` as live mob `index`'s entity parts answer it: the answering part's
    /// registration slot and its answer, the first part to answer winning; none when no part
    /// answers. A type with no subscribed part costs one table lookup. Server-side.
    pub(super) fn part_shearable(
        &self,
        index: usize,
        player: Option<PlayerRef>,
        shears: &ItemStack,
    ) -> Option<((usize, usize), bool)> {
        let mob = &self.mobs[index];
        let kind = &mob.kind.id;
        if !mob.alive()
            || !mod_api::__private::entity_part_hooked(
                mod_api::HookId::IsShearable,
                "is_shearable",
                kind,
            )
        {
            return None;
        }
        let entity = self.entity(mob.id)?;
        let metadata = super::api_values::stack_read_metadata(shears).ok()?;
        let project = |metadata| super::api_values::project_stack(shears, metadata).ok();
        let shears_view = mod_api::ItemStackView::resident(metadata, &project);
        let mut shearable = None;
        let slot = self.native_is_shearable(kind, entity, player, shears_view, &mut shearable)?;
        shearable.map(|answer| (slot, answer))
    }

    /// `IShearable.onSheared` on mob `index`'s entity part `part` (as [`Self::part_shearable`]
    /// named it), then `spawnShearedDrop` for each drop it returns: one block above where the mob
    /// stands afterwards, nudged by the mob's own random as `Sheep.shear` nudges its wool.
    /// A call the load phase refuses (reported) is no shearing: it comes back as `Err(())`, not
    /// as a shearing that dropped nothing.
    pub(super) fn part_shear(
        &mut self,
        index: usize,
        part: (usize, usize),
        player: Option<PlayerRef>,
        shears: &ItemStack,
    ) -> Result<(), ()> {
        let mob = &self.mobs[index];
        if !mod_api::__private::entity_part_hooked(
            mod_api::HookId::OnSheared,
            "on_sheared",
            &mob.kind.id,
        ) {
            return Ok(());
        }
        let (id, at, kind) = (mob.id, mob.position, mob.kind.clone());
        let (Some(entity), Ok(metadata)) = (
            self.entity(id),
            super::api_values::stack_read_metadata(shears),
        ) else {
            return Ok(());
        };
        let project = |metadata| super::api_values::project_stack(shears, metadata).ok();
        let shears_view = mod_api::ItemStackView::resident(metadata, &project);
        let mut drops = Vec::new();
        self.native_on_sheared(&kind.id, entity, player, shears_view, &mut drops, part)?;
        // The part may have moved or removed the mob: its drops fall where it is now.
        let index = self.mobs.iter().position(|m| m.id == id && !m.removed);
        let at = index.map_or(at, |i| self.mobs[i].position);
        for (drop_index, drop) in drops.into_iter().enumerate() {
            let stack = match drop.answer.try_into_engine(self.items()) {
                Ok(stack) => stack,
                Err(error) => {
                    log::warn!(
                        "mod {} hook on_sheared drop {drop_index} refused: {error:?}",
                        drop.mod_id
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
        Ok(())
    }

    /// Shears in the acting player's hand on mob `index` (`ShearsItem.interactLivingEntity`):
    /// the entity parts' answer, else the species' own (`Sheep`, `MushroomCow`, `SnowGolem`,
    /// `Bogged`), and a shearable mob fires `player_shear_entity`. `Some(consumed)` ends the
    /// interaction: a part's refusal (false), the hook's cancel or a part's shearing (true);
    /// none leaves the shearing to the species' own interaction.
    pub(super) fn player_shear(&mut self, mut index: usize) -> Option<bool> {
        let shears = self.held().cloned()?;
        let player = self.native_acting_player();
        let part = self.part_shearable(index, player, &shears);
        if part.is_some_and(|(_, shearable)| !shearable) {
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
        if shearable
            && let (Some(uuid), Some(name)) = (self.lan_acting_uuid, self.lan_acting_name.clone())
        {
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
        if self.part_shear(index, part, player, &shears).is_err() {
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

    /// A cow that shears into leather and a feather, with one invalid drop between them.
    struct LeatherCow;
    #[mod_api::sunrise_part]
    impl mod_api::EntityPart for LeatherCow {
        fn is_shearable(
            &self,
            _: &dyn mod_api::Host,
            event: &mut mod_api::IsShearableEvent<'_>,
        ) -> Option<bool> {
            assert!(event.player.is_none(), "a dispenser has no player");
            assert_eq!(event.shears.damage(), 7);
            Some(true)
        }
        fn on_sheared(
            &self,
            _: &mut dyn mod_api::ServerHost,
            event: &mut mod_api::OnShearedEvent<'_>,
        ) {
            assert!(event.player.is_none(), "a dispenser has no player");
            assert_eq!(event.shears.damage(), 7);
            for (item, count) in [
                ("minecraft:leather", 1),
                ("minecraft:iron_sword", 2), // vanilla swords stack to one
                ("minecraft:feather", 3),
            ] {
                let key = mod_api::__private::item_key(item).unwrap();
                event
                    .drops
                    .push(mod_api::ItemStack::new(key, count).unwrap());
            }
        }
    }
    /// A sheep that refuses the shears.
    struct WoollySheep;
    #[mod_api::sunrise_part]
    impl mod_api::EntityPart for WoollySheep {
        fn is_shearable(
            &self,
            _: &dyn mod_api::Host,
            _: &mut mod_api::IsShearableEvent<'_>,
        ) -> Option<bool> {
            Some(false)
        }
    }
    struct ShearMod;
    #[mod_api::__subscriptions]
    impl mod_api::Hooks for ShearMod {
        fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
            r.entity_part("minecraft:cow", LeatherCow);
            r.entity_part("minecraft:sheep", WoollySheep);
        }
    }

    /// Both cow callbacks see the actual dispensed stack. Valid drops stand on either side of a
    /// rejected output. A woolly sheep whose part says no keeps its wool and doesn't wear the tool.
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
            use sunrise_core::geometry::Direction;
            let mut shears = ItemStack::new("minecraft:shears", 1);
            shears.damage = 7;
            let worn = game
                .dispense_item((-1, 64, 0), Direction::East, shears)
                .unwrap()
                .unwrap();
            assert_eq!(worn.damage, 8, "the successful shear damages the tool");
            let before = game.dropped.len();
            let unchanged = game
                .dispense_item((3, 64, 0), Direction::East, worn)
                .unwrap()
                .unwrap();
            assert_eq!(
                unchanged.damage, 8,
                "the refused shear doesn't damage the tool"
            );
            assert!(!game.mobs.iter().any(|m| m.id == 2 && m.extra.sheared));
            assert_eq!(game.dropped.len(), before);
            let drops: Vec<_> = game
                .dropped
                .iter()
                .map(|d| (d.stack.item.as_ref(), d.stack.count))
                .collect();
            assert_eq!(drops, [("minecraft:leather", 1), ("minecraft:feather", 3)]);
        });
    }

    struct PanickingCow;
    #[mod_api::sunrise_part]
    impl mod_api::EntityPart for PanickingCow {
        fn is_shearable(
            &self,
            _: &dyn mod_api::Host,
            _: &mut mod_api::IsShearableEvent<'_>,
        ) -> Option<bool> {
            panic!("faulty shear probe");
        }
    }
    struct PanickingMod;
    #[mod_api::__subscriptions]
    impl mod_api::Hooks for PanickingMod {
        fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
            r.entity_part("minecraft:cow", PanickingCow);
        }
    }

    #[test]
    #[ignore = "daily"]
    fn a_panicking_shear_callback_is_disabled_for_the_session() {
        use super::super::native_entity_refs::tests::{run_fresh_cases, with_game};
        if run_fresh_cases(
            "game::part_shearing::tests::a_panicking_shear_callback_is_disabled_for_the_session",
            "SUNRISE_PART_SHEARING_PANIC_CHILD",
            &["valid"],
        ) {
            return;
        }
        let entries = || {
            vec![
                mod_api::ModEntry::new("faulty", Box::new(PanickingMod)),
                mod_api::ModEntry::new("shearing", Box::new(ShearMod)),
            ]
        };
        mod_platform::__private::install(entries().into());
        crate::game::api_values::install_parts(&entries()).unwrap();
        with_game(|game| {
            game.resolve_parts().unwrap();
            let kind = game.res.mob_types().get("minecraft:cow").unwrap().clone();
            game.mobs
                .push(Mob::new(1, kind, Vec3::new(0.5, 64.0, 0.5), 0.0));
            let mut shears = ItemStack::new("minecraft:shears", 1);
            shears.damage = 7;
            for _ in 0..2 {
                assert_eq!(
                    game.part_shearable(0, None, &shears)
                        .map(|(_, answer)| answer),
                    Some(true),
                    "the other mod's answer still stands"
                );
            }
            let notices = mod_api::__private::game::take_callback_panic_notices();
            assert_eq!(notices.len(), 1, "the panicking callback is not run again");
            assert_eq!(notices[0].mod_id, "faulty");
            assert_eq!(notices[0].operation, "is_shearable");
        });
    }

    /// A cow whose part reads the world through the Host, as a mod's part may.
    struct GatedCow;
    #[mod_api::sunrise_part]
    impl mod_api::EntityPart for GatedCow {
        fn is_shearable(
            &self,
            host: &dyn mod_api::Host,
            _: &mut mod_api::IsShearableEvent<'_>,
        ) -> Option<bool> {
            let _ = host.level("minecraft:overworld");
            Some(true)
        }
        fn on_sheared(
            &self,
            host: &mut dyn mod_api::ServerHost,
            event: &mut mod_api::OnShearedEvent<'_>,
        ) {
            let _ = host.level("minecraft:overworld");
            let leather = mod_api::__private::item_key("minecraft:leather").unwrap();
            event
                .drops
                .push(mod_api::ItemStack::new(leather, 1).unwrap());
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
            _: &mut mod_api::IsShearableEvent<'_>,
        ) -> Option<bool> {
            Some(true)
        }
        fn on_sheared(
            &self,
            host: &mut dyn mod_api::ServerHost,
            _: &mut mod_api::OnShearedEvent<'_>,
        ) {
            let _ = host.level("minecraft:overworld");
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
                assert_eq!(game.part_shearable(0, None, &shears), None);
                assert!(game.part_shear(0, (0, 0), None, &shears).is_err());
                assert!(game.dropped.is_empty(), "a refused part drops nothing");
                // The dispenser's shearing is not a success when the part's performing is refused.
                assert!(
                    !game.shear_entity((4, 64, 0), &shears).unwrap(),
                    "a refused on_sheared shears nothing"
                );
                assert!(!game.mobs[1].extra.sheared);
            }
            assert_eq!(game.part_shearable(0, None, &shears), Some(((0, 0), true)));
        });
    }
}
