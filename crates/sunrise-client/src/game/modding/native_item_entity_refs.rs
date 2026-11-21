//! Dropped-item handles for native server mods.
use super::{
    Game,
    api_values::{project_stack, stack_read_metadata},
    commands::Target,
};
use mod_api::{EntityRef, ItemEntityRef, ItemStackView};
use sunrise_client::{
    items::ItemStack,
    lan::playerdata::{ints_uuid, uuid_ints},
};

impl Game<'_> {
    fn native_item_entity_index(&self, item: ItemEntityRef) -> Option<usize> {
        let Target::Item(id) = self.native_entity_target(item.entity())? else {
            return None;
        };
        self.dropped.iter().position(|entity| entity.id == id)
    }

    /// Narrows an entity to a dropped item, or None for another kind, a
    /// stale handle, a client, or an item temporarily taken out for ticking.
    pub(super) fn item_entity(&self, entity: EntityRef) -> Option<ItemEntityRef> {
        let item = ItemEntityRef::new(entity);
        self.native_item_entity_index(item).map(|_| item)
    }

    /// Lend `ItemEntity.getItem`. Encode components only when the reader projects the view.
    pub(super) fn item_entity_stack(
        &self,
        item: ItemEntityRef,
        read: &mut dyn for<'a> FnMut(ItemStackView<'a>),
    ) -> Option<()> {
        let stack = &self
            .dropped
            .get(self.native_item_entity_index(item)?)?
            .stack;
        let metadata = stack_read_metadata(stack).ok()?;
        let project = |metadata| project_stack(stack, metadata).ok();
        read(ItemStackView::resident(metadata, &project));
        Some(())
    }
    /// `ItemEntity.setItem` leaves the entity in the level even if the new stack is empty.
    pub(super) fn item_entity_set_stack(
        &mut self,
        item: ItemEntityRef,
        stack: Option<ItemStack>,
    ) -> bool {
        let Some(index) = self.native_item_entity_index(item) else {
            return false;
        };
        self.journal_set_item_entity(index, stack);
        true
    }

    /// The signed `ItemEntity.pickupDelay`, if the item is available.
    pub(super) fn item_entity_pickup_delay(&self, item: ItemEntityRef) -> Option<i32> {
        Some(self.dropped[self.native_item_entity_index(item)?].pickup_delay)
    }

    /// `ItemEntity.setPickUpDelay`.
    pub(super) fn item_entity_set_pickup_delay(&mut self, item: ItemEntityRef, ticks: i32) -> bool {
        let Some(index) = self.native_item_entity_index(item) else {
            return false;
        };
        self.dropped[index].pickup_delay = ticks;
        true
    }

    /// `ItemEntity.setDefaultPickUpDelay`: ten ticks.
    pub(super) fn item_entity_set_default_pickup_delay(&mut self, item: ItemEntityRef) -> bool {
        self.item_entity_set_pickup_delay(item, 10)
    }

    /// `ItemEntity.setNoPickUpDelay`.
    pub(super) fn item_entity_set_no_pickup_delay(&mut self, item: ItemEntityRef) -> bool {
        self.item_entity_set_pickup_delay(item, 0)
    }

    /// `ItemEntity.setNeverPickUp`: the infinite-delay sentinel 32767.
    pub(super) fn item_entity_set_never_pickup(&mut self, item: ItemEntityRef) -> bool {
        self.item_entity_set_pickup_delay(item, i32::from(i16::MAX))
    }

    /// `ItemEntity.getAge`, including negative lifetime sentinels.
    pub(super) fn item_entity_age(&self, item: ItemEntityRef) -> Option<i32> {
        Some(self.dropped.get(self.native_item_entity_index(item)?)?.age)
    }

    /// The UUID set by `ItemEntity.setTarget`, limiting pickup to that player;
    /// None also means an unavailable item. This is not the thrower.
    pub(super) fn item_entity_target(&self, item: ItemEntityRef) -> Option<u128> {
        self.dropped
            .get(self.native_item_entity_index(item)?)?
            .target
            .map(ints_uuid)
    }

    /// `ItemEntity.setTarget`; None allows any player to pick it up.
    pub(super) fn item_entity_set_target(
        &mut self,
        item: ItemEntityRef,
        target: Option<u128>,
    ) -> bool {
        let Some(index) = self.native_item_entity_index(item) else {
            return false;
        };
        self.dropped[index].target = target.map(uuid_ints);
        true
    }

    /// `ItemEntity.getOwner`: the thrower as a resolved entity, not the pickup
    /// target. None for no thrower, an unloaded or different-level thrower,
    /// an unavailable handle, or one Sunrise cannot resolve.
    pub(super) fn item_entity_owner(&self, item: ItemEntityRef) -> Option<EntityRef> {
        let uuid = self
            .dropped
            .get(self.native_item_entity_index(item)?)?
            .thrower?;
        let target = self.target_by_uuid(ints_uuid(uuid))?;
        if self.target_level(target) != self.dim {
            return None;
        }
        let id = u32::try_from(self.network_entity_id(target)?).ok()?;
        self.entity(id)
    }
}

#[cfg(test)]
mod tests {
    use super::super::native_entity_refs::tests::with_game;
    use super::*;
    use crate::game::api_values::test_snapshot;
    use mod_api::WorldEpoch;
    use sunrise_client::{entities::ArmorStand, mobs::Mob};
    use sunrise_core::geometry::Vec3;

    #[test]
    #[ignore = "daily"]
    fn item_handles_preserve_stacks_and_distinguish_target_from_thrower() {
        with_game(|game| {
            game.options.dedicated = Some(Vec::new());
            let mut stack = ItemStack {
                item: "minecraft:diamond_pickaxe".into(),
                count: 1,
                damage: 17,
                extra: None,
            };
            stack.data_mut().custom_name =
                Some(sunrise_core::nbt::Tag::String("Named pick".into()));
            game.spawn_item(stack.clone(), [0.0; 3], [0.0; 3], 0);
            let entity = game.entity(game.dropped[0].id).unwrap();
            let item = game.item_entity(entity).unwrap();
            assert_eq!(item.entity(), entity);
            assert_eq!(
                item.with_stack(game, |read| (
                    read.item().id(game),
                    read.count(),
                    read.damage()
                )),
                Some((Some("minecraft:diamond_pickaxe"), 1, 17))
            );
            let mut changed = stack;
            changed.damage = 21;
            let changed = test_snapshot(&changed);
            assert_eq!(item.set_stack(game, Some(changed.clone())), Ok(true));
            assert_eq!(
                item.with_stack(game, |stack| stack.project()).flatten(),
                Some(changed)
            );
            assert!(item.set_pickup_delay(game, -7));
            assert_eq!(item.pickup_delay(game), Some(-7));
            assert!(item.set_default_pickup_delay(game));
            assert_eq!(item.pickup_delay(game), Some(10));
            assert!(item.set_never_pickup(game));
            assert_eq!(game.dropped[0].pickup_delay, 32767);
            assert_eq!(item.pickup_delay(game), Some(32767));
            assert!(item.set_no_pickup_delay(game));
            assert_eq!(item.pickup_delay(game), Some(0));
            game.dropped[0].age = -32768;
            assert_eq!(item.age(game), Some(-32768));
            let target = 0x12345678_9abcdef0_10203040_50607080_u128;
            assert!(item.set_target(game, Some(target)));
            assert_eq!(item.target(game), Some(target));
            assert_eq!(item.owner(game), None);
            let kind = game
                .res
                .mob_types()
                .get("minecraft:zombie")
                .unwrap()
                .clone();
            game.mobs.push(Mob::new(99, kind, Vec3::ZERO, 0.0));
            let thrower = game.entity(99).unwrap();
            game.dropped[0].thrower = Some(uuid_ints(game.entity_uuid(thrower).unwrap()));
            assert_eq!(item.owner(game), Some(thrower));
            assert_eq!(item.target(game), Some(target));
            assert!(item.set_target(game, None));
            assert_eq!(item.target(game), None);
            game.armor_stands
                .push(ArmorStand::new(100, Vec3::ZERO, 0.0));
            assert!(game.item_entity(game.entity(100).unwrap()).is_none());
            let stale = ItemEntityRef::new(EntityRef::new(
                WorldEpoch::new(entity.epoch().get() + 1),
                entity.id(),
            ));
            assert_eq!(stale.age(game), None);
            assert!(stale.with_stack(game, |_| ()).is_none());
            assert!(!stale.set_target(game, Some(target)));
            let taken = game.dropped.lift();
            assert!(item.with_stack(game, |_| ()).is_none());
            assert!(!item.set_no_pickup_delay(game));
            game.dropped = taken;
        });
    }

    #[test]
    #[ignore = "daily"]
    fn setting_an_empty_item_stack_leaves_discard_to_the_entity_tick() {
        crate::game::in_own_process(
            "game::native_item_entity_refs::tests::setting_an_empty_item_stack_leaves_discard_to_the_entity_tick",
            || {
                with_game(|game| {
                    game.options.dedicated = Some(Vec::new());
                    game.world.chunks.insert(
                        (0, 0),
                        std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
                    );
                    let air = test_snapshot(&ItemStack::new("minecraft:air", 1));
                    for empty in [None, Some(air)] {
                        game.spawn_item(
                            ItemStack::new("minecraft:stone", 1),
                            [2.5, 64.0, 2.5],
                            [0.0; 3],
                            0,
                        );
                        let id = game.next_entity;
                        game.ticked_entities_through = id;
                        let entity = game.entity(id).unwrap();
                        let item = game.item_entity(entity).unwrap();

                        assert_eq!(item.set_stack(game, empty), Ok(true));
                        assert_eq!(
                            game.item_entity(entity),
                            Some(item),
                            "setItem does not discard"
                        );
                        assert_eq!(item.with_stack(game, |stack| stack.count()), None);

                        game.tick_items();
                        assert!(!game.dropped.iter().any(|row| row.id == id));
                        assert_eq!(game.item_entity(entity), None);
                    }
                });
            },
        );
    }
}
