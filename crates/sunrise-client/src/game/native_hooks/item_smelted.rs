use super::*;

impl Game<'_> {
    /// NeoForge's `ItemSmeltedEvent` after `FurnaceResultSlot.checkTakeAchievements`
    /// pays out used recipes for the take, only when the slot tallied a nonzero count.
    pub(in crate::game) fn native_item_smelted(
        &mut self,
        call: mod_api::__private::ItemSmeltedCall<'_>,
    ) {
        self.fire_item_smelted(call);
    }
}

#[cfg(test)]
mod tests {
    use crate::game::native_guest_turn_tests::with_guests;
    use mod_api::{EntityJoinLevelEvent, Flow, Hooks, ItemSmeltedEvent, ModEntry, ServerHost};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    use sunrise_client::{
        chunk::Chunk,
        containers::{BlockEntity, Furnace, FurnaceKind},
        items::ItemStack,
    };
    use sunrise_protocol::packets::play::{ContainerClick, Serverbound};

    #[derive(Debug, PartialEq)]
    enum Observed {
        Experience,
        Smelted {
            player: u128,
            result: Option<(&'static str, u32)>,
            amount_removed: u32,
            hotbar_count: Option<u32>,
        },
    }

    struct Listener(Arc<Mutex<Vec<Observed>>>);

    #[mod_api::__subscriptions]
    impl Hooks for Listener {
        fn entity_join_level(
            &self,
            _: &mut dyn ServerHost,
            event: &mut EntityJoinLevelEvent<'_>,
        ) -> Flow {
            if event.kind == "minecraft:experience_orb" {
                self.0.lock().unwrap().push(Observed::Experience);
            }
            Flow::Continue
        }

        fn item_smelted(&self, host: &mut dyn ServerHost, event: &mut ItemSmeltedEvent<'_>) {
            let hotbar_count = event
                .player
                .with_inventory(host, |slots| slots.get(0).map(|stack| stack.count()))
                .flatten();
            let result = event.stack.project().and_then(|stack| {
                Some((
                    mod_api::__private::item_key_id(stack.item())?,
                    stack.count(),
                ))
            });
            self.0.lock().unwrap().push(Observed::Smelted {
                player: event.player.uuid(),
                result,
                amount_removed: event.amount_removed,
                hotbar_count,
            });
        }
    }

    struct Panics(Arc<AtomicUsize>);

    #[mod_api::__subscriptions]
    impl Hooks for Panics {
        #[listener(priority = 10)]
        fn item_smelted(&self, _: &mut dyn ServerHost, _: &mut ItemSmeltedEvent<'_>) {
            self.0.fetch_add(1, Ordering::Relaxed);
            panic!("furnace result listener failed");
        }
    }

    #[test]
    #[ignore = "daily"]
    fn a_committed_furnace_result_take_reports_the_live_player_after_experience() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let attempts = Arc::new(AtomicUsize::new(0));
        let mods = vec![
            ModEntry::new("panics", Box::new(Panics(Arc::clone(&attempts)))),
            ModEntry::new("smelted", Box::new(Listener(Arc::clone(&observed)))),
        ]
        .into();
        mod_platform::testing::with_mods(mods, || {
            with_guests(|game| {
                let pos = (1, 64, 1);
                game.world
                    .chunks
                    .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
                let state = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:furnace")
                    .unwrap();
                game.world.set(pos.0, pos.1, pos.2, state).unwrap();
                game.world.block_entities.insert(
                    pos,
                    BlockEntity::Furnace(Furnace {
                        kind: FurnaceKind::Furnace,
                        items: [None, None, Some(ItemStack::new("minecraft:glass", 64))],
                        lit_remaining: 0,
                        lit_total: 0,
                        cooking_timer: 0,
                        cooking_total: 200,
                        speed: 1.0,
                        // Ten glass recipes award exactly one point.
                        recipes_used: [("minecraft:glass".to_owned(), 10)].into(),
                    }),
                );
                game.lan.as_mut().unwrap().hub.guests.update(0, |guest| {
                    guest
                        .inventory
                        .fill(Some(ItemStack::new("minecraft:dirt", 64)));
                    guest.inventory[0] = Some(ItemStack::new("minecraft:glass", 62));
                });
                game.open_block_menu(12, pos);
                let window = game.lan.as_ref().unwrap().hub.guests[0]
                    .menu
                    .as_ref()
                    .unwrap()
                    .window;
                let click = |input| {
                    Serverbound::ContainerClick(ContainerClick {
                        container: window,
                        state: 0,
                        slot: 2,
                        button: 0,
                        input,
                        changed: Vec::new(),
                        carried: None,
                    })
                };

                // The shift-click moves only two items, yet the event borrows the
                // original stack. Its player handle sees the committed hotbar slot.
                game.lan_guest_packet(12, click(1));
                assert_eq!(
                    *observed.lock().unwrap(),
                    [
                        Observed::Experience,
                        Observed::Smelted {
                            player: 12,
                            result: Some(("minecraft:glass", 64)),
                            amount_removed: 2,
                            hotbar_count: Some(64),
                        },
                    ]
                );
                let Some(BlockEntity::Furnace(furnace)) = game.world.block_entities.get(&pos)
                else {
                    panic!("the furnace is gone");
                };
                assert!(furnace.recipes_used.is_empty());
                assert_eq!(
                    furnace.items[2],
                    Some(ItemStack::new("minecraft:glass", 62))
                );

                // The first subscriber panicked. The other subscriber still hears this
                // pickup, and the furnace neither repays the recipe nor calls the panicker.
                observed.lock().unwrap().clear();
                game.lan_guest_packet(12, click(0));
                assert_eq!(attempts.load(Ordering::Relaxed), 1);
                assert_eq!(
                    *observed.lock().unwrap(),
                    [Observed::Smelted {
                        player: 12,
                        result: Some(("minecraft:glass", 62)),
                        amount_removed: 62,
                        hotbar_count: Some(64),
                    }]
                );
            });
        });
    }
}
