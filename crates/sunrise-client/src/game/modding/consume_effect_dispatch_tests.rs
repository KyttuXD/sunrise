//! Explicit living consume effects reach real native listeners, including nested kinds.
#![cfg(test)]
use crate::game::{
    Dim, api_values::TryIntoApi, native_entity_refs::tests::with_game,
    native_guest_turn_tests::with_guests,
};
use mod_api::{ConsumeEffectEvent, Hooks, ModEntry, ServerHost};
use std::sync::{Arc, Mutex};
use sunrise_client::{
    items::{ConsumeEffect, ConsumeSound, ItemStack, parse_consumable},
    mobs::Mob,
};
use sunrise_core::{geometry::Vec3, nbt::Tag};

struct First(Arc<Mutex<Vec<&'static str>>>);
#[mod_api::__subscriptions]
impl Hooks for First {
    fn consume_effect(&self, host: &mut dyn ServerHost, event: &mut ConsumeEffectEvent<'_>) {
        // Both listeners subscribe to all custom consume entries. The engine,
        // not a guard in the listener, must keep other kinds out of this callback.
        assert_eq!(event.kind, "first:effect");
        self.0.lock().unwrap().push("first");
        let source = host.entity(21).unwrap();
        assert_eq!(event.user, source);
        assert_eq!(
            event.stack.item(),
            host.item_key(&mod_api::Identifier::parse("minecraft:pufferfish").unwrap())
                .unwrap()
        );
        if event.stack.count() == 1 {
            return;
        }
        assert_eq!(event.stack.count(), 2);
        let living = host.living(source).unwrap();
        let same = ItemStack::new("minecraft:pufferfish", 1)
            .try_into_api()
            .unwrap();
        assert_eq!(living.apply_consume_effect(host, &same, 0), Ok(Some(true)));
        let other = ItemStack::new("minecraft:honey_bottle", 1)
            .try_into_api()
            .unwrap();
        assert_eq!(living.apply_consume_effect(host, &other, 0), Ok(Some(true)));
        self.0.lock().unwrap().push("first_after");
    }
}
struct Second(Arc<Mutex<Vec<&'static str>>>);
#[mod_api::__subscriptions]
impl Hooks for Second {
    fn consume_effect(&self, _: &mut dyn ServerHost, event: &mut ConsumeEffectEvent<'_>) {
        assert_eq!(event.kind, "second:effect");
        self.0.lock().unwrap().push("second");
    }
}

#[test]
#[ignore = "daily"]
fn nested_consume_effects_follow_call_order_even_for_the_same_kind() {
    // A definition admitted without its callback must not look it up on execution.
    let unbound = mod_platform::testing::with_mods(Vec::<ModEntry>::new().into(), || {
        parse_consumable(&serde_json::json!({
            "on_consume_effects": [{"type": "second:effect"}]
        }))
        .unwrap()
    });
    let trace = Arc::new(Mutex::new(Vec::new()));
    let mods = vec![
        ModEntry::new("first", Box::new(First(trace.clone()))),
        ModEntry::new("second", Box::new(Second(trace.clone()))),
    ];
    mod_platform::testing::with_mods(mods.into(), || {
        with_game(|game| {
            let mut items = game.items().clone();
            for (item, kind) in [
                ("minecraft:pufferfish", "first:effect"),
                ("minecraft:honey_bottle", "second:effect"),
            ] {
                let component = parse_consumable(&serde_json::json!({
                    "on_consume_effects": [{"type": kind}]
                }))
                .unwrap();
                items.items.get_mut(item).unwrap().consumable = Some(Arc::new(component));
            }
            items.items.get_mut("minecraft:apple").unwrap().consumable = Some(Arc::new(unbound));
            game.server_items = Some(Arc::new(items));
            let kind = game.res.mob_types().get("minecraft:cow").unwrap().clone();
            game.mobs
                .push(Mob::new(21, kind, Vec3::new(0.0, 64.0, 0.0), 0.0));
            let living = game.living(game.entity(21).unwrap()).unwrap();
            let stack = ItemStack::new("minecraft:pufferfish", 2)
                .try_into_api()
                .unwrap();
            assert_eq!(living.apply_consume_effect(game, &stack, 0), Ok(Some(true)));
            let apple = ItemStack::new("minecraft:apple", 1).try_into_api().unwrap();
            assert_eq!(
                living.apply_consume_effect(game, &apple, 0),
                Ok(Some(false))
            );
            assert_eq!(
                *trace.lock().unwrap(),
                ["first", "first", "second", "first_after"]
            );
        });
    });
}

struct PlayerConsumption(Arc<Mutex<Vec<(mod_api::EntityRef, u32)>>>);
#[mod_api::__subscriptions]
impl Hooks for PlayerConsumption {
    fn consume_effect(&self, host: &mut dyn ServerHost, event: &mut ConsumeEffectEvent<'_>) {
        if event.kind == "player_meal:effect" {
            assert_eq!(host.entity(event.user.id()), Some(event.user));
            self.0
                .lock()
                .unwrap()
                .push((event.user, event.stack.count()));
        }
    }
}

#[test]
#[ignore = "daily"]
fn consuming_player_is_supplied_outside_a_guest_action() {
    let heard = Arc::new(Mutex::new(Vec::new()));
    mod_platform::testing::with_mods(
        vec![ModEntry::new(
            "player_meal",
            Box::new(PlayerConsumption(heard.clone())),
        )]
        .into(),
        || {
            with_game(|game| {
                let mut body = sunrise_client::motion::PlayerMotion::new(
                    game.motion,
                    Vec3::new(0.0, 65.0, 0.0),
                )
                .unwrap();
                body.entity_id = 7;
                game.player = Some(body);
                game.bind_local_player_residency();
                assert!(game.native_acting_player().is_none());
                let user = game.entity(7).unwrap();
                let stack = ItemStack::new("minecraft:pufferfish", 4);
                let info = game.items().get(&stack.item).unwrap().clone();
                let consumable = parse_consumable(&serde_json::json!({
                    "on_consume_effects": [{"type": "player_meal:effect"}]
                }))
                .unwrap();
                game.creative = false;
                game.inventory[game.selected] = Some(stack.clone());
                game.finish_consuming(&info, &consumable, &stack);
                assert_eq!(*heard.lock().unwrap(), [(user, 4)]);
                assert_eq!(game.held().unwrap().count, 3);
            })
        },
    );
}

struct ReplaceDrink;
#[mod_api::__subscriptions]
impl Hooks for ReplaceDrink {
    fn consume_effect(&self, host: &mut dyn ServerHost, event: &mut ConsumeEffectEvent<'_>) {
        let expected =
            host.item_key(&mod_api::Identifier::parse("minecraft:honey_bottle").unwrap());
        assert_eq!(Some(event.stack.item()), expected);
        let living = host.living(event.user).unwrap();
        let replacement = ItemStack::new("minecraft:stick", 1).try_into_api().unwrap();
        assert_eq!(
            living.set_hand_item(host, mod_api::Hand::Main, Some(replacement)),
            Ok(true)
        );
    }
}

#[test]
#[ignore = "daily"]
fn replacing_the_consumed_hand_keeps_the_new_item_and_emits_no_remainder() {
    let mods = vec![ModEntry::new("replace_drink", Box::new(ReplaceDrink))].into();
    mod_platform::testing::with_mods(mods, || {
        with_game(|game| {
            let mut body =
                sunrise_client::motion::PlayerMotion::new(game.motion, Vec3::new(0.0, 65.0, 0.0))
                    .unwrap();
            body.entity_id = 7;
            game.player = Some(body);
            game.bind_local_player_residency();
            game.creative = false;
            game.inventory.fill(None);
            let mut stack = ItemStack::new("minecraft:honey_bottle", 1);
            stack.data_mut().set_other(
                "minecraft:consumable",
                Some(Tag::compound([(
                    "on_consume_effects",
                    Tag::List(vec![Tag::compound([(
                        "type",
                        Tag::String("replace_drink:drink".into()),
                    )])]),
                )])),
            );
            let info = game.items().get(&stack.item).unwrap().clone();
            let consumable = game.consumable(&stack).unwrap();
            game.inventory[game.selected] = Some(stack.clone());
            game.finish_consuming(&info, &consumable, &stack);
            assert_eq!(game.held(), Some(&ItemStack::new("minecraft:stick", 1)));
            assert_eq!(game.inventory.iter().flatten().count(), 1);
            assert!(game.dropped.is_empty());
        });
    });
}

struct ObservePotion(Arc<Mutex<Vec<(String, bool)>>>);
#[mod_api::__subscriptions]
impl Hooks for ObservePotion {
    fn consume_effect(&self, host: &mut dyn ServerHost, event: &mut ConsumeEffectEvent<'_>) {
        let living = host.living(event.user).unwrap();
        let breathing = host.effect_key("minecraft:water_breathing").unwrap();
        let has_effect = living
            .effects(host)
            .unwrap()
            .iter()
            .any(|effect| effect.id == breathing);
        assert_eq!(event.stack.count(), 1);
        self.0
            .lock()
            .unwrap()
            .push((event.kind.to_owned(), has_effect));
    }
}

#[test]
#[ignore = "daily"]
fn potion_finishes_with_its_patched_effects_after_potion_contents() {
    use sunrise_core::random::RandomSource;

    let seen = Arc::new(Mutex::new(Vec::new()));
    let mods = vec![ModEntry::new(
        "potion_order",
        Box::new(ObservePotion(seen.clone())),
    )]
    .into();
    mod_platform::testing::with_mods(mods, || {
        with_game(|game| {
            let mut body =
                sunrise_client::motion::PlayerMotion::new(game.motion, Vec3::new(0.0, 65.0, 0.0))
                    .unwrap();
            body.entity_id = 7;
            game.player = Some(body);
            game.bind_local_player_residency();
            game.creative = false;
            game.player.as_mut().unwrap().random.set_seed(41);
            let mut potion =
                ItemStack::new("minecraft:potion", 1).with_potion("minecraft:water_breathing");
            potion.data_mut().set_other(
                "minecraft:consumable",
                Some(Tag::compound([(
                    "on_consume_effects",
                    Tag::List(
                        [
                            "potion_order:before_clear",
                            "minecraft:clear_all_effects",
                            "potion_order:after_clear",
                        ]
                        .map(|id| Tag::compound([("type", Tag::String(id.into()))]))
                        .into(),
                    ),
                )])),
            );
            let info = game.items().get(&potion.item).unwrap().clone();
            let slot = game.selected;
            game.inventory[slot] = Some(potion.clone());
            game.complete_using_item(slot, &potion, &info);
            // LegacyRandomSource after the emission's boolean, three floats and 16 crumbs.
            assert_eq!(
                game.player.as_ref().unwrap().random.clone().next_long(),
                -4_022_577_910_208_863_423,
            );
            assert_eq!(
                *seen.lock().unwrap(),
                [
                    ("potion_order:before_clear".into(), true),
                    ("potion_order:after_clear".into(), false)
                ]
            );
            assert_eq!(
                game.held(),
                Some(&ItemStack::new("minecraft:glass_bottle", 1))
            );
        });
    });
}

struct PanicsDuringConsumption(Arc<std::sync::atomic::AtomicUsize>);
#[mod_api::__subscriptions]
impl Hooks for PanicsDuringConsumption {
    fn consume_effect(&self, _: &mut dyn ServerHost, _: &mut ConsumeEffectEvent<'_>) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        panic!("consume effect fixture");
    }
}

#[test]
#[ignore = "daily"]
fn a_consume_callback_panic_leaves_later_effects_and_other_callbacks_running() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mods = vec![
        ModEntry::new(
            "panics_during_meal",
            Box::new(PanicsDuringConsumption(calls.clone())),
        ),
        ModEntry::new("observes_meal", Box::new(ObservePotion(seen.clone()))),
    ];
    mod_platform::testing::with_mods(mods.into(), || {
        with_game(|game| {
            let mut body =
                sunrise_client::motion::PlayerMotion::new(game.motion, Vec3::new(0.0, 65.0, 0.0))
                    .unwrap();
            body.entity_id = 7;
            game.player = Some(body);
            game.bind_local_player_residency();
            game.creative = false;
            let stack = ItemStack::new("minecraft:apple", 1);
            let info = game.items().get(&stack.item).unwrap().clone();
            let consumable = parse_consumable(&serde_json::json!({
                "on_consume_effects": [
                    {"type": "panics_during_meal:first"},
                    {"type": "minecraft:apply_effects", "effects": [
                        {"id": "minecraft:water_breathing", "duration": 120}
                    ]},
                    {"type": "observes_meal:after_builtin"},
                    {"type": "panics_during_meal:again"}
                ]
            }))
            .unwrap();
            for _ in 0..2 {
                game.inventory[game.selected] = Some(stack.clone());
                game.finish_consuming(&info, &consumable, &stack);
                assert!(game.held().is_none());
            }
            assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
            assert_eq!(
                *seen.lock().unwrap(),
                [
                    ("observes_meal:after_builtin".into(), true),
                    ("observes_meal:after_builtin".into(), true)
                ]
            );
        });
    });
}

struct DelegatingMeal;
#[mod_api::__subscriptions]
impl Hooks for DelegatingMeal {
    fn consume_effect(&self, host: &mut dyn ServerHost, _: &mut ConsumeEffectEvent<'_>) {
        let guest = host.living(host.entity(13).unwrap()).unwrap();
        let speed = host.effect_key("minecraft:speed").unwrap();
        let effect = mod_api::ActiveEffect::new(speed, 120, 0, false, true, true);
        // Even a consumer that catches an unwind cannot turn a foreign listener's fault into
        // a successful return from this effect.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assert!(guest.add_effect(host, &effect));
        }));
    }
}

struct PanickingEffectListener(std::sync::atomic::AtomicBool);
#[mod_api::__subscriptions]
impl Hooks for PanickingEffectListener {
    fn mob_effect_applicable(
        &self,
        _: &mut dyn ServerHost,
        event: &mut mod_api::MobEffectApplicableEvent,
    ) -> Option<mod_api::EffectApplicable> {
        if event.entity.entity().id() == 13
            && self.0.swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            panic!("foreign effect listener");
        }
        None
    }
}

#[test]
#[ignore = "daily"]
fn a_foreign_listener_unwinds_consumption_after_restoring_the_outer_guest() {
    let mods = vec![
        ModEntry::new("delegating_meal", Box::new(DelegatingMeal)),
        ModEntry::new(
            "effect_listener",
            Box::new(PanickingEffectListener(true.into())),
        ),
    ];
    mod_platform::testing::with_mods(mods.into(), || {
        with_guests(|game| {
            let world = tempfile::tempdir().unwrap();
            game.world_root = world.path().to_path_buf();
            game.lan
                .as_mut()
                .unwrap()
                .hub
                .update_guest(13, |guest| guest.dimension = Dim::Nether.id().into())
                .unwrap();
            let outer_dim = game.dim;
            game.as_guest(12, |game| {
                use sunrise_client::text::Text;
                game.chat_capture = Some(vec![Text::literal("outer guest")]);
                let stack = ItemStack::new("minecraft:apple", 1);
                let info = game.items().get(&stack.item).unwrap().clone();
                let consumable = parse_consumable(&serde_json::json!({
                    "on_consume_effects": [
                        {"type": "delegating_meal:effect"},
                        {"type": "minecraft:apply_effects", "effects": [
                            {"id": "minecraft:water_breathing", "duration": 120}
                        ]}
                    ]
                }))
                .unwrap();
                game.inventory[game.selected] = Some(stack.clone());
                let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    game.finish_consuming(&info, &consumable, &stack);
                }))
                .unwrap_err();
                assert_eq!(
                    panic.downcast_ref::<&str>(),
                    Some(&"foreign effect listener")
                );
                assert_eq!(game.lan_acting, Some(12));
                assert_eq!(game.dim, outer_dim);
                assert_eq!(
                    game.chat_capture.as_ref().unwrap()[0].plain(),
                    "outer guest"
                );
                assert_eq!(game.held(), Some(&stack));
                assert!(game.potions.effects.get("water_breathing").is_none());

                // The foreign panic neither disabled the delegating callback nor let the
                // remaining built-in effect run in the interrupted guest/level.
                game.finish_consuming(&info, &consumable, &stack);
                assert!(game.potions.effects.get("water_breathing").is_some());
                let other = game.lan.as_ref().unwrap().hub.guest(13).unwrap();
                assert!(other.effects.get("speed").is_some());
                assert!(other.effects.get("water_breathing").is_none());
                assert_eq!(other.dimension, Dim::Nether.id());
                assert_eq!(game.dim, outer_dim);
                game.chat_capture = None;
            })
            .unwrap();
        });
    });
}

#[test]
#[ignore = "daily"]
fn interleaved_guests_keep_their_vanilla_consume_draw_order() {
    use sunrise_core::random::RandomSource;

    mod_platform::testing::with_mods(Vec::<ModEntry>::new().into(), || {
        with_guests(|game| {
            game.world.chunks.clear(); // No candidate has a loaded block to land on.
            let mut level_random = game.world.random.clone();
            let stack = ItemStack::new("minecraft:apple", 1);
            let info = game.items().get(&stack.item).unwrap().clone();
            let consumable = parse_consumable(&serde_json::json!({
                "on_consume_effects": [
                    {"type": "minecraft:apply_effects", "probability": 0.5, "effects": [
                        {"id": "minecraft:water_breathing", "duration": 120}
                    ]},
                    {"type": "minecraft:teleport_randomly"}
                ]
            }))
            .unwrap();
            for (conn, seed) in [(12, 29), (13, 13)] {
                game.as_guest(conn, |game| {
                    game.player.as_mut().unwrap().random.set_seed(seed)
                })
                .unwrap();
            }

            // LegacyRandomSource: emission takes a boolean and three floats,
            // spawnItemParticles four floats per crumb (16), FoodProperties
            // three more floats, apply_effects one, and teleport three doubles
            // for each of Vanilla TeleportRandomlyConsumeEffect's 16 attempts.
            let expected = [
                (12, true, 5_428_492_878_003_447_481),
                (13, false, -1_008_312_113_262_950_709),
                (12, false, -2_516_599_419_424_563_847),
            ];
            for (user, breathing, next_random_long) in expected {
                game.as_guest(user, |game| {
                    let at = game.player.as_ref().unwrap().position;
                    game.remove_player_effects(None);
                    game.inventory[game.selected] = Some(stack.clone());
                    game.finish_consuming(&info, &consumable, &stack);
                    assert_eq!(
                        game.potions.effects.get("water_breathing").is_some(),
                        breathing
                    );
                    let player = game.player.as_ref().unwrap();
                    assert_eq!(player.position, at);
                    assert_eq!(player.random.clone().next_long(), next_random_long);
                })
                .unwrap();
            }
            assert_eq!(game.world.random.next_long(), level_random.next_long());
        });
    });
}

struct FoxConsumption(Arc<Mutex<Vec<(mod_api::EntityRef, mod_api::ItemStack)>>>);
#[mod_api::__subscriptions]
impl Hooks for FoxConsumption {
    fn consume_effect(&self, host: &mut dyn ServerHost, event: &mut ConsumeEffectEvent<'_>) {
        match event.kind {
            "fox_meal:discard" => assert!(event.user.discard(host)),
            "fox_meal:remember" => {
                self.0
                    .lock()
                    .unwrap()
                    .push((event.user, event.stack.clone()));
            }
            _ => {}
        }
    }
}

#[test]
#[ignore = "daily"]
fn a_foxs_later_custom_effect_keeps_the_original_user_and_stack_after_discard() {
    let heard = Arc::new(Mutex::new(Vec::new()));
    let mods = vec![ModEntry::new(
        "fox_meal",
        Box::new(FoxConsumption(heard.clone())),
    )];
    mod_platform::testing::with_mods(mods.into(), || {
        with_game(|game| {
            let mut items = game.items().clone();
            let consumable = items
                .items
                .get_mut("minecraft:chorus_fruit")
                .unwrap()
                .consumable
                .as_mut()
                .unwrap();
            let effects = &mut Arc::make_mut(consumable).effects;
            assert!(
                effects
                    .iter()
                    .any(|effect| matches!(effect, ConsumeEffect::TeleportRandomly(_)))
            );
            let custom = |kind: &str| {
                parse_consumable(&serde_json::json!({"on_consume_effects": [{"type": kind}]}))
                    .unwrap()
                    .effects
                    .remove(0)
            };
            effects.insert(0, custom("fox_meal:discard"));
            effects.push(ConsumeEffect::PlaySound(ConsumeSound::Registered(
                "minecraft:entity.fox.eat".into(),
            )));
            effects.push(custom("fox_meal:remember"));
            game.server_items = Some(Arc::new(items));

            let kind = game.res.mob_types().get("minecraft:fox").unwrap().clone();
            let mut fox = Mob::new(21, kind, Vec3::new(8.5, 64.0, 8.5), 0.0);
            fox.on_ground = true;
            fox.extra.ints[1] = 601;
            let mut stack = ItemStack::new("minecraft:chorus_fruit", 7);
            stack
                .data_mut()
                .set_loot_name("minecraft:custom_name", &serde_json::json!("Fox fruit"));
            let expected_stack = stack.try_into_api().unwrap();
            fox.extra.set_main_hand_stack(Some(stack));
            let decision =
                sunrise_client::mobs::fox::decide_consumption(&fox, &game.mob_world_base(&[]))
                    .unwrap();
            game.mobs.push(fox);
            let user = game.entity(21).unwrap();
            assert!(!game.apply_fox_consumption(&decision));
            assert!(game.entity(21).is_none());
            assert_eq!(*heard.lock().unwrap(), [(user, expected_stack)]);
        });
    });
}
