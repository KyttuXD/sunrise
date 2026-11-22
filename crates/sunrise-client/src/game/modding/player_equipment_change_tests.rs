//! `living_equipment_change` for a server's players and mobs (#3417, debt p3-39):
//! borrow-based comparison without blanket equipment cloning, all eight
//! `EquipmentSlot`s matching vanilla `LivingEntity.collectEquipmentChanges` and
//! `PlayerEquipment`, lazy `ItemStackView` delivery, and minimal history retention.
use std::sync::{Arc, Mutex};

use mod_api::{self, EquipmentSlot, Hooks, LivingEquipmentChangeEvent, ServerHost};
use sunrise_client::{
    items::ItemStack,
    mobs::{Mob, finalize},
};
use sunrise_core::geometry::Vec3;
use sunrise_items::equipment::{BODY, CHEST, SADDLE};

use super::api_values::TryIntoApi;
use super::native_guest_turn_tests::with_guests;

/// What each call saw: the entity id, its resolved kind, the slot, and projected stacks.
type Heard = Vec<(
    u32,
    Option<String>,
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
        let kind = event
            .entity
            .entity()
            .kind(host)
            .and_then(|kind| kind.id(host));
        self.heard.lock().unwrap().push((
            event.entity.id(),
            kind,
            event.slot,
            event.from.as_ref().and_then(|stack| stack.project()),
            event.to.as_ref().and_then(|stack| stack.project()),
        ));
    }
}

fn with_counter(test: impl FnOnce(&mut crate::game::Game<'_>, &Mutex<Heard>)) {
    let heard = Arc::new(Mutex::new(Heard::new()));
    let counter = Counter {
        heard: heard.clone(),
    };
    let mods: mod_api::Mods = vec![mod_api::ModEntry::new("counter", Box::new(counter))].into();
    mod_platform::testing::with_mods(mods, || {
        with_guests(|game| {
            game.world
                .chunks
                .insert((0, 0), Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)));
            let host = game.lan.as_mut().unwrap();
            for conn in [12, 13] {
                host.hub
                    .update_guest(conn, |guest| guest.loaded = true)
                    .unwrap();
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
    vec![(12, Some("minecraft:player".into()), slot, from, to)]
}

#[test]
#[ignore = "daily"]
fn a_players_hotbar_selection_pickup_and_helmet_are_each_one_change() {
    with_counter(|game, heard| {
        let sword = ItemStack::new("minecraft:iron_sword", 1);
        let stone = ItemStack::new("minecraft:stone", 1);
        let helmet = ItemStack::new("minecraft:iron_helmet", 1);
        let body = ItemStack::new("minecraft:diamond_horse_armor", 1);
        let saddle = ItemStack::new("minecraft:saddle", 1);
        // Both guests start empty-handed in slot 0: nothing changed.
        game.tick_guests();
        assert_eq!(take(heard), Heard::new());

        // Selecting hotbar slot 1, which holds a sword, moves it into the main hand.
        let guests = &mut game.lan.as_mut().unwrap().hub.guests;
        guests.update(0, |g| {
            g.inventory[1] = Some(sword.clone());
            g.selected = 1;
        });
        game.tick_guests();
        assert_eq!(
            take(heard),
            player(EquipmentSlot::MainHand, None, api(&sword))
        );

        // Back to the empty slot 0, then a stone picked up into it.
        let guests = &mut game.lan.as_mut().unwrap().hub.guests;
        guests.update(0, |g| g.selected = 0);
        game.tick_guests();
        assert_eq!(
            take(heard),
            player(EquipmentSlot::MainHand, api(&sword), None)
        );

        let guests = &mut game.lan.as_mut().unwrap().hub.guests;
        guests.update(0, |g| g.inventory[0] = Some(stone.clone()));
        game.tick_guests();
        assert_eq!(
            take(heard),
            player(EquipmentSlot::MainHand, None, api(&stone))
        );

        // A helmet put on from the inventory screen (`armor` runs head to feet).
        let guests = &mut game.lan.as_mut().unwrap().hub.guests;
        guests.update(0, |g| g.armor[0] = Some(helmet.clone()));
        game.tick_guests();
        assert_eq!(take(heard), player(EquipmentSlot::Head, None, api(&helmet)));

        // Body and saddle slots follow vanilla `PlayerEquipment` / `EquipmentSlot.VALUES`.
        let guests = &mut game.lan.as_mut().unwrap().hub.guests;
        guests.update(0, |g| {
            g.equipment[BODY] = Some(body.clone());
            g.equipment[SADDLE] = Some(saddle.clone());
        });
        game.tick_guests();
        assert_eq!(
            take(heard),
            vec![
                (
                    12,
                    Some("minecraft:player".into()),
                    EquipmentSlot::Body,
                    None,
                    api(&body)
                ),
                (
                    12,
                    Some("minecraft:player".into()),
                    EquipmentSlot::Saddle,
                    None,
                    api(&saddle)
                ),
            ]
        );

        // A tick with no change tells nothing.
        game.tick_guests();
        assert_eq!(take(heard), Heard::new());
    });
}

#[test]
#[ignore = "daily"]
fn finishing_item_use_is_reported_in_the_same_player_body_tick() {
    with_counter(|game, heard| {
        let potion = ItemStack::new("minecraft:potion", 1).with_potion("minecraft:water");
        game.lan
            .as_mut()
            .unwrap()
            .hub
            .update_guest(12, |guest| {
                guest.inventory[0] = Some(potion.clone());
            })
            .unwrap();
        game.tick_guests();
        assert_eq!(
            take(heard),
            player(EquipmentSlot::MainHand, None, api(&potion))
        );

        game.as_guest(12, |game| {
            assert!(game.start_using_item());
            // Enter the next body tick one tick before consumption completes.
            game.use_duration = 1;
        })
        .unwrap();
        game.tick_guests();
        let bottle = ItemStack::new("minecraft:glass_bottle", 1);
        assert_eq!(
            take(heard),
            player(EquipmentSlot::MainHand, api(&potion), api(&bottle))
        );
        game.tick_guests();
        assert!(take(heard).is_empty());
    });
}

#[test]
#[ignore = "daily"]
fn an_unloaded_spectator_does_not_detect_equipment_until_its_body_ticks() {
    with_counter(|game, heard| {
        let stone = ItemStack::new("minecraft:stone", 1);
        game.lan
            .as_mut()
            .unwrap()
            .hub
            .update_guest(12, |guest| {
                guest.game_mode = 3;
                guest.position = [32.0, 64.0, 32.0];
                guest.inventory[0] = Some(stone.clone());
            })
            .unwrap();
        game.tick_guests();
        assert!(take(heard).is_empty());

        game.lan
            .as_mut()
            .unwrap()
            .hub
            .update_guest(12, |guest| {
                guest.position = [3.0, 64.0, 5.0];
            })
            .unwrap();
        game.tick_guests();
        assert_eq!(
            take(heard),
            player(EquipmentSlot::MainHand, None, api(&stone))
        );
    });
}

#[test]
#[ignore = "daily"]
fn mobs_detect_enchantments_held_only_in_gear_on_an_undamaged_stack() {
    with_counter(|game, heard| {
        let zombie = game
            .res
            .mob_types()
            .get("minecraft:zombie")
            .unwrap()
            .clone();
        let mut mob = Mob::new(42, zombie, Vec3::new(3.0, 64.0, 5.0), 0.0);
        finalize::set_item(&mut mob, CHEST, "minecraft:iron_chestplate");
        finalize::set_slot_enchantments(
            &mut mob,
            CHEST,
            vec![(Arc::from("minecraft:protection"), 2)],
        );
        game.mobs.push(mob);

        let mut chest = ItemStack::new("minecraft:iron_chestplate", 1);
        chest.set_crafting_enchantments(vec![(Arc::from("minecraft:protection"), 2)]);
        let expected = |from, to| -> Heard {
            vec![(
                42,
                Some("minecraft:zombie".into()),
                EquipmentSlot::Chest,
                from,
                to,
            )]
        };
        // An attribute refresh before the mob body ticks must not consume the
        // slot change. Only the equipment pass of a ticked mob reports it.
        game.tick_equipment_attributes();
        assert!(take(heard).is_empty());
        game.creatures.ticked.insert(42);
        game.tick_equipment_attributes();
        assert_eq!(take(heard), expected(None, api(&chest)));

        finalize::set_slot_enchantments(
            &mut game.mobs[0],
            CHEST,
            vec![(Arc::from("minecraft:protection"), 3)],
        );
        let mut upgraded = chest.clone();
        upgraded.set_crafting_enchantments(vec![(Arc::from("minecraft:protection"), 3)]);
        game.tick_equipment_attributes();
        assert_eq!(take(heard), expected(api(&chest), api(&upgraded)));

        game.tick_equipment_attributes();
        assert!(take(heard).is_empty());
        game.mobs[0].set_equipment_stack(CHEST, None);
        game.tick_equipment_attributes();
        assert_eq!(take(heard), expected(api(&upgraded), None));
    });
}
