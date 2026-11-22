//! The modifier hook reads the effective camera entity, not the local spectator.
use super::Game;
use crate::game::native_entity_refs::tests::with_client_game;
use mod_api::{
    ClientHost, ComputeFovModifierEvent, Hooks, ModEntry,
    client::camera::{CameraType, FovPlayer, UseItem},
};
use std::sync::{Arc, Mutex};
use sunrise_client::{items::ItemStack, motion::PlayerMotion};
use sunrise_core::geometry::Vec3;
use sunrise_protocol::{
    connection::Pipe,
    packets::play::{self, Clientbound as C},
};

type Calls = Arc<Mutex<Vec<(FovPlayer, f32)>>>;

struct Modifier(Calls);

#[mod_api::__subscriptions]
impl Hooks for Modifier {
    fn compute_fov_modifier(&self, _: &dyn ClientHost, event: &mut ComputeFovModifierEvent<'_>) {
        self.0.lock().unwrap().push((*event.player, event.base));
        event.modifier = 0.5;
    }
}

fn with_spectator(kind: &str, test: impl FnOnce(&mut Game<'_>, &Calls)) {
    let calls = Calls::default();
    let mods = vec![ModEntry::new(
        "camera_fov",
        Box::new(Modifier(calls.clone())),
    )]
    .into();
    mod_platform::testing::with_mods(mods, || {
        with_client_game(|game| {
            let (client, _server) = Pipe::pair();
            game.connect_local(client, "CameraFovTest").unwrap();
            game.remote.as_mut().unwrap().entity_id = 7;
            let mut player = PlayerMotion::new(game.motion, Vec3::ZERO).unwrap();
            player.flying = true;
            player.speed = 0.4;
            game.player = Some(player);
            game.spawn_entity(31, kind, [10.0, 64.0, 12.0], [0.0; 3]);
            game.handle_packet(C::SetCamera(play::SetCamera { camera_id: 31 }))
                .unwrap();
            test(game, &calls);
        });
    });
}

#[test]
#[ignore = "daily"]
fn spectated_player_fov_uses_their_own_attributes_and_item_use() {
    with_spectator("minecraft:player", |game, calls| {
        let attribute = game
            .motion
            .attributes
            .id("minecraft:movement_speed")
            .unwrap()
            .registry_id();
        game.handle_packet(C::UpdateAttributes(play::UpdateAttributes {
            id: 31,
            values: vec![play::AttributeSnapshot {
                attribute,
                base: 0.1,
                modifiers: vec![play::AttributeModifier {
                    id: "test:movement_bonus".into(),
                    amount: 0.1,
                    operation: 0,
                }],
            }],
        }))
        .unwrap();
        let m = game.mob_index.get_mut(&mut game.mobs, 31).unwrap();
        m.extra.sprinting = true;
        m.extra.using_item = 11;
        m.extra.using_off_hand = true;
        m.set_equipment_stack(
            sunrise_items::equipment::OFFHAND,
            Some(ItemStack::new("minecraft:spyglass", 1)),
        );

        let inputs = FovPlayer {
            flying: false,
            walking_speed: 0.1,
            movement_speed: 0.2,
            using: Some((UseItem::Spyglass, 10)),
        };
        game.view.fov_modifier = 1.0;
        game.camera_type = CameraType::FirstPerson;
        game.tick_fov();
        assert_eq!(
            game.view.fov_modifier, 0.55,
            "ease halfway toward the 0.1 first-person spyglass target"
        );
        assert!(
            calls.lock().unwrap().is_empty(),
            "first-person spyglass returns 0.1 before compute_fov_modifier"
        );

        game.camera_type = CameraType::ThirdPersonBack;
        game.tick_fov();
        assert_eq!(
            game.view.fov_modifier, 0.525,
            "ease halfway toward the mod's 0.5"
        );
        // Vanilla: (0.2 / 0.1 + 1) / 2 in third person; the packet's speed is
        // used directly and the sprint flag adds nothing to it.
        assert_eq!(*calls.lock().unwrap(), [(inputs, 1.5)]);
    });
}

#[test]
#[ignore = "daily"]
fn spectated_non_player_fov_does_not_dispatch_the_player_hook() {
    with_spectator("minecraft:creeper", |game, calls| {
        game.view.fov_modifier = 0.5;
        game.tick_fov();
        assert_eq!(game.view.fov_modifier, 0.75, "ease halfway to neutral 1.0");
        assert!(calls.lock().unwrap().is_empty());
    });
}
