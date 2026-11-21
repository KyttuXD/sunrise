#[cfg(test)]
thread_local! { static STACK_CONVERSIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

use super::*;
use crate::game::native_ticks;

/// The hook points. Each does nothing in Sunrise.
impl Game<'_> {
    pub(in crate::game) fn item_update_round(&mut self) -> native_ticks::TickRound {
        // EntityTickEvent.Pre for the items about to tick, and the Post after them.
        let mut round = self.native_tick_round();
        let merging = self.remote.is_none();
        // One read says whether any item part listens; with none, no stack is looked at.
        let update_hook =
            merging && mod_api::__private::has_part_hook(mod_api::HookId::OnEntityItemUpdate);
        // Items a mod named (`entity_tick_each`) hear their Pre, and then their part, alone in
        // their own turn (`tick_dropped_items`).
        if (round.listening() || update_hook) && !round.alone_kind(native_ticks::ITEM) {
            let ticking = self.entity_ticking_area();
            let mut i = 0;
            while let Some(item) = self.dropped.get(i) {
                let (id, x, z) = (
                    item.id,
                    item.position.x.floor() as i32,
                    item.position.z.floor() as i32,
                );
                if !(merging && self.joined_during_entity_pass(id)) && ticking(x, z) {
                    if round.listening() {
                        round.add(id, native_ticks::TickKind::Fixed(native_ticks::ITEM));
                    } else if update_hook && self.item_update_halts(i) {
                        round.halt(id);
                    }
                }
                i += 1;
            }
            self.native_tick_pre_batch(&mut round);
            // Each item the Pre let through, in order, hears its part.
            let (mut n, mut from) = (0, 0);
            while update_hook && let Some(id) = round.let_through(n) {
                if let Some(i) = native_ticks::place(&self.dropped, from, |item| item.id == id) {
                    if self.item_update_halts(i) {
                        round.halt(id);
                    }
                    from = i + 1;
                }
                n += 1;
            }
        }
        round
    }

    /// `IItemExtension.onEntityItemUpdate`: a dropped item entity holding
    /// `stack` is about to tick (`ItemEntity.tick`). Set `canceled` and its
    /// movement and merging are skipped for this tick. Server-side.
    pub(in crate::game) fn native_on_entity_item_update(
        call: mod_api::__private::OnEntityItemUpdateCall<'_>,
    ) {
        Self::fire_on_entity_item_update(call);
    }

    /// Asks the parts of the item entity at `index` of `dropped` whether its tick
    /// ends here. An item no part is attached to is told nothing: its stack is not
    /// converted and no scope is made.
    pub(in crate::game) fn item_update_halts(&mut self, index: usize) -> bool {
        let item = &self.dropped[index];
        if !mod_api::__private::item_listens(mod_api::HookId::OnEntityItemUpdate, &item.stack.item)
        {
            return false;
        }
        let id = item.id;
        #[cfg(test)]
        STACK_CONVERSIONS.with(|count| count.set(count.get() + 1));
        let Ok(stack) = item.stack.try_into_api() else {
            return false;
        };
        let Some(handle) = self.entity(id).and_then(|entity| self.item_entity(entity)) else {
            return false;
        };
        let mut canceled = false;
        let mut access = ItemEntityTickAccess {
            game: self,
            subject: handle,
        };
        Self::native_on_entity_item_update(mod_api::__private::OnEntityItemUpdateCall {
            stack: &stack,
            subject: mod_api::ItemEntityServerScope::new(&mut access),
            canceled: &mut canceled,
        });
        canceled
    }
}

struct ResidentDroppedItem<'g, 'w> {
    game: &'g mut Game<'w>,
    subject: mod_api::ItemEntityRef,
    index: usize,
}

impl mod_api::ResidentItemEntity for ResidentDroppedItem<'_, '_> {
    fn position(&self) -> [f64; 3] {
        let p = self.game.dropped[self.index].position;
        [p.x, p.y, p.z]
    }
    fn velocity(&self) -> [f64; 3] {
        let v = self.game.dropped[self.index].velocity;
        [v.x, v.y, v.z]
    }
    fn with_stack(&self, call: &mut dyn for<'a> FnMut(mod_api::ItemStackView<'a>)) {
        use crate::game::api_values::{project_stack, stack_read_metadata};
        let stack = &self.game.dropped[self.index].stack;
        let Ok(metadata) = stack_read_metadata(stack) else {
            return;
        };
        let project = |metadata| project_stack(stack, metadata).ok();
        call(mod_api::ItemStackView::resident(metadata, &project));
    }
    fn pickup_delay(&self) -> Option<i32> {
        let delay = self.game.dropped[self.index].pickup_delay;
        if delay == i64::from(u32::MAX) {
            Some(i32::from(i16::MAX))
        } else {
            i32::try_from(delay).ok()
        }
    }
    fn age(&self) -> i32 {
        self.game.dropped[self.index].age
    }
    fn target(&self) -> Option<u128> {
        self.game.dropped[self.index]
            .target
            .map(sunrise_client::lan::playerdata::ints_uuid)
    }
    fn owner(&self) -> Option<mod_api::EntityRef> {
        self.game.item_entity_owner(self.subject)
    }
    fn set_stack(&mut self, stack: mod_api::ItemStack) -> Result<bool, mod_api::StackWriteError> {
        use crate::game::api_values::TryIntoEngine;
        let stack = stack.try_into_engine(self.game.res.items())?;
        if stack.count == 0 || &*stack.item == "minecraft:air" {
            return Ok(false);
        }
        Ok(self
            .game
            .journal_set_item_entity(self.subject.entity().id(), stack))
    }
    fn set_pickup_delay(&mut self, ticks: i32) -> bool {
        self.game.dropped[self.index].pickup_delay = i64::from(ticks);
        true
    }
    fn set_target(&mut self, target: Option<u128>) -> bool {
        self.game.dropped[self.index].target =
            target.map(sunrise_client::lan::playerdata::uuid_ints);
        true
    }
    fn set_velocity(&mut self, velocity: [f64; 3]) -> bool {
        if !velocity.iter().all(|v| v.is_finite()) {
            return false;
        }
        self.game.dropped[self.index].velocity =
            sunrise_core::geometry::Vec3::new(velocity[0], velocity[1], velocity[2]);
        true
    }
}

struct ItemEntityTickAccess<'g, 'w> {
    game: &'g mut Game<'w>,
    subject: mod_api::ItemEntityRef,
}

impl mod_api::ItemEntityServerAccess for ItemEntityTickAccess<'_, '_> {
    fn with_view(&mut self, call: &mut dyn FnMut(mod_api::ItemEntitySubjectView<'_>)) {
        if !self.game.is_logical_server() {
            return;
        }
        let Some(crate::game::commands::Target::Item(id)) =
            self.game.native_entity_target(self.subject.entity())
        else {
            return;
        };
        let Some(index) = self.game.dropped.iter().position(|item| item.id == id) else {
            return;
        };
        let mut data = ResidentDroppedItem {
            game: self.game,
            subject: self.subject,
            index,
        };
        call(mod_api::ItemEntitySubjectView::new(self.subject, &mut data));
    }

    fn host(&mut self) -> &mut dyn mod_api::ServerHost {
        self.game
    }
}

#[cfg(test)]
mod on_entity_item_update_tests {
    use super::*;
    use mod_api::{Flow, Hooks, ItemPart, OnEntityItemUpdateEvent, Registrar, RegistrationContext};
    use std::sync::{Arc, Mutex};
    use sunrise_client::chunk::Chunk;

    /// An item part that writes the entity's pickup delay to 7 on every update and
    /// records the entities it was told about; `cancels` ends the tick.
    #[derive(Clone)]
    struct Update {
        item: &'static str,
        cancels: bool,
        remove: bool,
        mixed: bool,
        heard: Arc<Mutex<Vec<u32>>>,
    }

    #[mod_api::sunrise_part]
    impl ItemPart for Update {
        fn on_entity_item_update(&self, event: &mut OnEntityItemUpdateEvent<'_>) -> Flow {
            let handle = event
                .subject
                .with_view(|view| view.handle())
                .expect("live resident item entity");
            ORDER.with(|order| {
                order.borrow_mut().push((handle.entity().id(), "update"));
            });
            self.heard.lock().unwrap().push(handle.entity().id());
            if self.remove {
                assert!(
                    handle
                        .entity()
                        .remove(event.subject.host(), mod_api::RemovalReason::Discarded)
                );
                assert!(event.subject.with_view(|_| ()).is_none());
                return Flow::Continue;
            }
            assert_eq!(
                event.subject.with_view(|mut view| view.set_pickup_delay(7)),
                Some(true)
            );
            if self.cancels && (!self.mixed || handle.entity().id().is_multiple_of(2)) {
                Flow::Cancel
            } else {
                Flow::Continue
            }
        }
    }

    struct Mod(Vec<Update>, bool);

    thread_local! { static ORDER: std::cell::RefCell<Vec<(u32, &'static str)>> = const { std::cell::RefCell::new(Vec::new()) }; }

    /// Each entity a tick hook was told, in order, with the hook's phase.
    fn told(entities: &[u32], phase: &'static str) {
        ORDER.with(|order| {
            let mut order = order.borrow_mut();
            order.extend(entities.iter().map(|&entity| (entity, phase)));
        });
    }

    #[mod_api::__subscriptions]
    impl Hooks for Mod {
        fn entity_tick_listened(
            &self,
            _: mod_api::HostSide<'_>,
            event: &mut mod_api::EntityTickListenedEvent,
        ) {
            event.listened = true;
        }
        fn entity_tick_pre(
            &self,
            _: mod_api::HostSide<'_>,
            event: &mut mod_api::EntityTickPreEvent<'_>,
        ) {
            told(event.entities, "pre");
        }
        fn entity_tick_post(
            &self,
            _: mod_api::HostSide<'_>,
            event: &mut mod_api::EntityTickPostEvent<'_>,
        ) {
            told(event.entities, "post");
        }
        fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
            if self.1 {
                r.entity_tick_each("minecraft:item");
            }
            for part in &self.0 {
                let key = r.item_key(part.item);
                r.item_part(key, part.clone());
            }
        }
    }

    /// A serving level with a stone floor at y 63 and nothing dropped.
    fn floor(game: &mut Game<'_>) {
        game.options.dedicated = Some(Vec::new());
        game.native_lifecycle
            .begin_open(Some(mod_api::Authority::IntegratedServer));
        game.world = sunrise_client::world::World::new(game.res.facts().air, 1);
        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        let stone = game
            .pack
            .assets
            .registry
            .default_state("minecraft:stone")
            .unwrap();
        for x in 0..16 {
            for z in 0..16 {
                game.world.set(x, 63, z, stone).unwrap();
            }
        }
        game.dropped.clear();
    }

    /// One stack of `item` at rest on the floor, a pickup delay of 20 ticks.
    fn rest(game: &mut Game<'_>, item: &str, x: f64, z: f64) -> u32 {
        game.spawn_item(ItemStack::new(item, 1), [x, 64.0, z], [0.0; 3], 20);
        game.ticked_entities_through = game.next_entity;
        game.dropped.last().unwrap().id
    }

    fn counts(game: &Game<'_>, ids: &[u32]) -> Vec<u32> {
        game.dropped
            .iter()
            .filter(|item| ids.contains(&item.id))
            .map(|item| item.stack.count)
            .collect()
    }

    /// An item the part of its own item cancels does not age, merge or lose pickup
    /// delay for the tick, and keeps the part's write; an item whose part does not
    /// cancel ticks as vanilla does after the write; an item without a part ticks
    /// and merges as before and is never told.
    #[test]
    #[ignore = "daily"]
    fn a_canceling_part_holds_its_own_item_and_nothing_else() {
        crate::game::in_own_process(
            "game::native_hooks::on_entity_item_update::on_entity_item_update_tests::a_canceling_part_holds_its_own_item_and_nothing_else",
            || a_canceling_part_holds_its_own_item_and_nothing_else_isolated(false),
        );
    }

    #[test]
    #[ignore = "daily"]
    fn a_named_item_still_hears_post_when_its_part_halts_physics() {
        crate::game::in_own_process(
            "game::native_hooks::on_entity_item_update::on_entity_item_update_tests::a_named_item_still_hears_post_when_its_part_halts_physics",
            || a_canceling_part_holds_its_own_item_and_nothing_else_isolated(true),
        );
    }

    fn a_canceling_part_holds_its_own_item_and_nothing_else_isolated(named: bool) {
        use crate::game::native_entity_refs::tests::with_game;
        let iron = Update {
            item: "minecraft:iron_ingot",
            cancels: true,
            remove: false,
            mixed: false,
            heard: Default::default(),
        };
        let gold = Update {
            item: "minecraft:gold_ingot",
            cancels: false,
            remove: false,
            mixed: false,
            heard: Default::default(),
        };
        let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
            "item_update",
            Box::new(Mod(
                vec![
                    iron.clone(),
                    gold.clone(),
                    Update {
                        item: "minecraft:emerald",
                        cancels: false,
                        remove: true,
                        mixed: false,
                        heard: Default::default(),
                    },
                    Update {
                        item: "minecraft:copper_ingot",
                        cancels: true,
                        remove: false,
                        mixed: true,
                        heard: Default::default(),
                    },
                ],
                named,
            )),
        )]
        .into();
        mod_platform::testing::with_mods(mods.clone(), || {
            crate::game::api_values::install_parts(&mods).unwrap();
            with_game(|game| {
                floor(game);
                // Two of each of iron and diamond side by side, a gold one apart.
                let irons = [
                    rest(game, "minecraft:iron_ingot", 2.5, 2.5),
                    rest(game, "minecraft:iron_ingot", 2.7, 2.5),
                ];
                let diamonds = [
                    rest(game, "minecraft:diamond", 10.5, 2.5),
                    rest(game, "minecraft:diamond", 10.7, 2.5),
                ];
                let gold_id = rest(game, "minecraft:gold_ingot", 6.5, 10.5);

                ORDER.with(|order| order.borrow_mut().clear());
                STACK_CONVERSIONS.with(|count| count.set(0));
                game.tick_items();
                assert_eq!(STACK_CONVERSIONS.with(|count| count.get()), 3);
                for id in irons.into_iter().chain([gold_id]) {
                    assert_eq!(
                        ORDER.with(|order| order
                            .borrow()
                            .iter()
                            .filter(|(entity, _)| *entity == id)
                            .map(|(_, phase)| *phase)
                            .collect::<Vec<_>>()),
                        ["pre", "update", "post"]
                    );
                }
                for id in irons {
                    let held = game.dropped.iter().find(|item| item.id == id).unwrap();
                    // Nothing of the entity's own tick ran: not its age, nor its delay's
                    // countdown; only the part's write is there.
                    assert_eq!((held.age, held.ticks, held.pickup_delay), (0, 0, 7));
                }
                for id in diamonds {
                    let ticked = game.dropped.iter().find(|item| item.id == id).unwrap();
                    assert_eq!((ticked.age, ticked.pickup_delay), (1, 19));
                }
                let gold_item = game.dropped.iter().find(|item| item.id == gold_id).unwrap();
                // The part wrote 7, and then the item's own tick counted down.
                assert_eq!((gold_item.age, gold_item.pickup_delay), (1, 6));

                // A resting item merges within 40 ticks (`ItemEntity.tick`): the part-less
                // pair does, the pair held by its part never does.
                for _ in 0..40 {
                    game.tick_items();
                }
                assert_eq!(counts(game, &irons), [1, 1]);
                assert_eq!(counts(game, &diamonds), [2]);
                assert!(
                    game.dropped
                        .iter()
                        .filter(|item| irons.contains(&item.id))
                        .all(|item| item.age == 0)
                );

                // Told once per tick of each item of its own: 41 ticks, two iron items; 41, one gold item.
                let mut heard = iron.heard.lock().unwrap().clone();
                heard.sort_unstable();
                let mut expected: Vec<u32> = irons.iter().flat_map(|id| [*id; 41]).collect();
                expected.sort_unstable();
                assert_eq!(heard, expected);
                assert_eq!(*gold.heard.lock().unwrap(), vec![gold_id; 41]);
                let removed = rest(game, "minecraft:emerald", 12.5, 12.5);
                game.tick_items();
                assert_eq!(
                    ORDER.with(|order| order
                        .borrow()
                        .iter()
                        .filter(|(id, _)| *id == removed)
                        .map(|(_, phase)| *phase)
                        .collect::<Vec<_>>()),
                    ["pre", "update", "post"]
                );
                assert!(game.entity(removed).is_none());
                game.native_removals_flush();
                assert!(!game.dropped.iter().any(|item| item.id == removed));
                let pair = [
                    rest(game, "minecraft:copper_ingot", 8.5, 8.5),
                    rest(game, "minecraft:copper_ingot", 8.7, 8.5),
                ];
                for _ in 0..80 {
                    game.tick_items();
                }
                assert_eq!(counts(game, &pair), [2]);
            });
        });
    }

    /// With no part attached to any item, every item ticks and merges as it did before the hook.
    #[test]
    #[ignore = "daily"]
    fn without_parts_items_tick_and_merge() {
        crate::game::native_entity_refs::tests::with_game(|game| {
            floor(game);
            let ids = [
                rest(game, "minecraft:iron_ingot", 2.5, 2.5),
                rest(game, "minecraft:iron_ingot", 2.7, 2.5),
            ];
            STACK_CONVERSIONS.with(|count| count.set(0));
            game.tick_items();
            assert_eq!(STACK_CONVERSIONS.with(|count| count.get()), 0);
            assert!(
                game.dropped
                    .iter()
                    .all(|item| (item.age, item.pickup_delay) == (1, 19))
            );
            for _ in 0..40 {
                game.tick_items();
            }
            assert_eq!(counts(game, &ids), [2]);
        });
    }
}
