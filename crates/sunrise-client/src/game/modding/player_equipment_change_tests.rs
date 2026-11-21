//! `living_equipment_change` for a server's players (#3417): a hotbar selection, a pickup into
//! the selected slot and a helmet put on are each one change on the next tick, and nothing is
//! prepared or told while no enabled mod subscribes to `living_equipment_change`.
use std::sync::{Arc, Mutex};

use mod_api::{self, EquipmentSlot, Hooks, LivingEquipmentChangeEvent, ServerHost};
use sunrise_client::items::ItemStack;

use super::api_values::TryIntoApi;
use super::native_guest_turn_tests::with_guests;

/// What each call saw: the entity, its kind, the slot, and the stacks before and after.
type Heard = Vec<(
    u32,
    Option<mod_api::EntityTypeKey>,
    EquipmentSlot,
    Option<mod_api::ItemStack>,
    Option<mod_api::ItemStack>,
)>;

struct Counter {
    heard: Arc<Mutex<Heard>>,
}
#[mod_api::__subscriptions]
impl Hooks for Counter {
    fn living_equipment_change(
        &self,
        host: &mut dyn ServerHost,
        event: &mut LivingEquipmentChangeEvent<'_>,
    ) {
        self.heard.lock().unwrap().push((
            event.entity.id(),
            event.entity.entity().kind(host),
            event.slot,
            event.from.and_then(mod_api::ItemStackView::project),
            event.to.and_then(mod_api::ItemStackView::project),
        ));
    }
}

struct Quiet;
#[mod_api::__subscriptions]
impl Hooks for Quiet {}

fn with_counter(listens: bool, test: impl FnOnce(&mut crate::game::Game<'_>, &Mutex<Heard>)) {
    let heard = Arc::new(Mutex::new(Heard::new()));
    let entry = if listens {
        mod_api::ModEntry::new(
            "counter",
            Box::new(Counter {
                heard: heard.clone(),
            }),
        )
    } else {
        mod_api::ModEntry::new("quiet", Box::new(Quiet))
    };
    let mods: mod_api::Mods = vec![entry].into();
    mod_platform::testing::with_mods(mods, || {
        with_guests(|game| {
            // A joined guest has its play state from the login on.
            let host = game.lan.as_mut().unwrap();
            for conn in [12, 13] {
                host.play.entry(conn).or_default();
            }
            test(game, &heard)
        })
    });
}

fn api(stack: &ItemStack) -> Option<mod_api::ItemStack> {
    Some(stack.try_into_api().unwrap())
}

fn take(heard: &Mutex<Heard>) -> Heard {
    std::mem::take(&mut *heard.lock().unwrap())
}

fn player(
    slot: EquipmentSlot,
    from: Option<mod_api::ItemStack>,
    to: Option<mod_api::ItemStack>,
) -> Heard {
    vec![(
        12,
        mod_api::__private::resolved_entity_type_key("minecraft:player"),
        slot,
        from,
        to,
    )]
}

#[test]
#[ignore = "daily"]
fn a_players_hotbar_selection_pickup_and_helmet_are_each_one_change() {
    with_counter(true, |game, heard| {
        let sword = ItemStack::new("minecraft:iron_sword", 1);
        let stone = ItemStack::new("minecraft:stone", 1);
        let helmet = ItemStack::new("minecraft:iron_helmet", 1);
        // The guest starts empty-handed in slot 0: counting from empty, nothing changed.
        game.native_player_equipment(12);
        assert_eq!(take(heard), Heard::new());

        // Selecting hotbar slot 1, which holds a sword, moves it into the main hand.
        let guests = &mut game.lan.as_mut().unwrap().hub.guests;
        guests.update(0, |g| {
            g.inventory[1] = Some(sword.clone());
            g.selected = 1;
        });
        game.native_player_equipment(12);
        assert_eq!(
            take(heard),
            player(EquipmentSlot::MainHand, None, api(&sword))
        );

        // Back to the empty slot 0, then a stone picked up into it.
        let guests = &mut game.lan.as_mut().unwrap().hub.guests;
        guests.update(0, |g| g.selected = 0);
        game.native_player_equipment(12);
        assert_eq!(
            take(heard),
            player(EquipmentSlot::MainHand, api(&sword), None)
        );
        let guests = &mut game.lan.as_mut().unwrap().hub.guests;
        guests.update(0, |g| g.inventory[0] = Some(stone.clone()));
        game.native_player_equipment(12);
        assert_eq!(
            take(heard),
            player(EquipmentSlot::MainHand, None, api(&stone))
        );

        // A helmet put on from the inventory screen (`armor` runs head to feet).
        let guests = &mut game.lan.as_mut().unwrap().hub.guests;
        guests.update(0, |g| g.armor[0] = Some(helmet.clone()));
        game.native_player_equipment(12);
        assert_eq!(take(heard), player(EquipmentSlot::Head, None, api(&helmet)));

        // A tick with no change tells nothing.
        game.native_player_equipment(12);
        assert_eq!(take(heard), Heard::new());
    });
}

#[test]
#[ignore = "daily"]
fn a_players_body_and_saddle_are_observed_during_its_tick() {
    use sunrise_items::equipment::{BODY, SADDLE};

    with_counter(true, |game, heard| {
        let body = ItemStack::new("minecraft:iron_horse_armor", 1);
        let saddle = ItemStack::new("minecraft:saddle", 1);
        game.lan.as_mut().unwrap().hub.guests.update(0, |g| {
            g.equipment[BODY] = Some(body);
            g.equipment[SADDLE] = Some(saddle);
        });
        let expected =
            |id, count| mod_api::ItemStack::new(mod_api::__private::item_key(id).unwrap(), count);
        game.tick_guests();
        assert_eq!(
            take(heard),
            [
                player(
                    EquipmentSlot::Body,
                    None,
                    expected("minecraft:iron_horse_armor", 1)
                ),
                player(EquipmentSlot::Saddle, None, expected("minecraft:saddle", 1)),
            ]
            .concat(),
        );
        game.tick_guests();
        assert!(take(heard).is_empty());

        game.lan.as_mut().unwrap().hub.guests.update(0, |g| {
            g.equipment[BODY] = None;
            g.equipment[SADDLE] = None;
        });
        game.tick_guests();
        assert_eq!(
            take(heard),
            [
                player(
                    EquipmentSlot::Body,
                    expected("minecraft:iron_horse_armor", 1),
                    None
                ),
                player(EquipmentSlot::Saddle, expected("minecraft:saddle", 1), None),
            ]
            .concat(),
        );
        assert!(
            game.lan.as_ref().unwrap().play[&12]
                .equipment_heard
                .is_none()
        );
    });
}

#[test]
#[ignore = "daily"]
fn an_equipment_listener_panic_does_not_stop_other_listeners_or_repeat() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fault(Arc<AtomicUsize>);
    #[mod_api::__subscriptions]
    impl Hooks for Fault {
        fn living_equipment_change(
            &self,
            _: &mut dyn ServerHost,
            _: &mut LivingEquipmentChangeEvent<'_>,
        ) {
            self.0.fetch_add(1, Ordering::Relaxed);
            panic!("equipment observer fault");
        }
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let heard = Arc::new(Mutex::new(Heard::new()));
    let mods = vec![
        mod_api::ModEntry::new("equipment-fault", Box::new(Fault(calls.clone()))),
        mod_api::ModEntry::new(
            "equipment-witness",
            Box::new(Counter {
                heard: heard.clone(),
            }),
        ),
    ]
    .into();
    mod_platform::testing::with_mods(mods, || {
        with_guests(|game| {
            game.lan.as_mut().unwrap().play.entry(12).or_default();
            for id in ["minecraft:stone", "minecraft:dirt"] {
                game.lan.as_mut().unwrap().hub.guests.update(0, |g| {
                    g.inventory[0] = Some(ItemStack::new(id, 1));
                });
                game.native_player_equipment(12);
            }
            assert_eq!(calls.load(Ordering::Relaxed), 1);
            assert_eq!(take(&heard).len(), 2);
        });
    });
}

#[test]
#[ignore = "daily"]
fn players_are_not_told_without_the_listened_opt_in() {
    with_counter(false, |game, heard| {
        let guests = &mut game.lan.as_mut().unwrap().hub.guests;
        guests.update(0, |g| {
            g.inventory[1] = Some(ItemStack::new("minecraft:iron_sword", 1));
            g.selected = 1;
            g.armor[0] = Some(ItemStack::new("minecraft:iron_helmet", 1));
        });
        game.native_player_equipment(12);
        assert_eq!(take(heard), Heard::new());
        assert!(
            game.lan
                .as_ref()
                .unwrap()
                .play
                .get(&12)
                .unwrap()
                .equipment_heard
                .is_none()
        );
    });
}
