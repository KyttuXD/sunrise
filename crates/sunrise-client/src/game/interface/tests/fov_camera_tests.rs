//! The modifier hook reads the effective camera entity, not the local spectator.
use super::Game;
use crate::game::native_entity_refs::tests::with_client_game;
use mod_api::{
    ClientHost, ComputeFovEvent, ComputeFovModifierEvent, Hooks, ModEntry,
    client::camera::{CameraType, FovPlayer, FovProjection, UseItem},
};
use std::sync::{Arc, Mutex};
use sunrise_client::{items::ItemStack, motion::PlayerMotion};
use sunrise_core::geometry::Vec3;
use sunrise_protocol::{
    connection::Pipe,
    packets::play::{self, Clientbound as C},
};

type Calls = Arc<Mutex<Vec<(FovPlayer, f32, f32, f32)>>>;

struct Modifier(Calls);

#[mod_api::__subscriptions]
impl Hooks for Modifier {
    fn compute_fov_modifier(&self, _: &dyn ClientHost, event: &mut ComputeFovModifierEvent<'_>) {
        self.0.lock().unwrap().push((
            *event.player,
            event.base,
            event.effect_scale,
            event.modifier,
        ));
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
            calls.lock().unwrap().clear();
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
        game.settings.file.set(
            "fovEffectScale",
            sunrise_client::options_file::V::Double(0.5),
        );
        game.view.fov_modifier = 1.0;
        game.camera_type = CameraType::FirstPerson;
        game.tick_fov();
        assert_eq!(game.view.fov_modifier, 0.55, "ease halfway to 0.1");
        assert!(
            calls.lock().unwrap().is_empty(),
            "scope returns before the hook"
        );

        game.camera_type = CameraType::ThirdPersonBack;
        game.view.fov_modifier = 1.0;
        game.tick_fov();
        assert_eq!(
            game.view.fov_modifier, 0.75,
            "ease halfway to the mod's 0.5"
        );
        // The synced movement attribute already includes its modifiers. Vanilla's raw
        // (0.2 / 0.1 + 1) / 2 is 1.5; NeoForge lerps from 1 by 0.5 before listeners.
        assert_eq!(*calls.lock().unwrap(), [(inputs, 1.5, 0.5, 1.25)]);
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

#[test]
#[ignore = "daily"]
fn camera_fov_listeners_receive_world_then_hand_at_the_actual_viewpoint() {
    const AT: [f64; 3] = [4.0, 200.0, -2.0];
    struct Lens(Arc<Mutex<Vec<(FovProjection, f32)>>>);

    #[mod_api::__subscriptions]
    impl Hooks for Lens {
        fn compute_fov_modifier(
            &self,
            _: &dyn ClientHost,
            event: &mut ComputeFovModifierEvent<'_>,
        ) {
            // Camera.tickFov must apply its own clamp to this proposal.
            event.modifier = 12.0;
        }

        fn compute_fov(&self, _: &dyn ClientHost, event: &mut ComputeFovEvent) {
            // Record input outside the closure assertion path: panics in hooks are contained.
            if event.camera == AT && event.partial == 0.5 && !event.scoping {
                self.0
                    .lock()
                    .unwrap()
                    .push((event.projection, event.degrees));
            }
            event.degrees = if event.projection == FovProjection::World {
                0.5
            } else {
                f32::NAN
            };
        }
    }

    let calls = Arc::new(Mutex::new(Vec::new()));
    let mods = vec![ModEntry::new("fov_lens", Box::new(Lens(calls.clone())))].into();
    mod_platform::testing::with_mods(mods, || {
        with_client_game(|game| {
            game.title = None;
            game.player = Some(PlayerMotion::new(game.motion, Vec3::ZERO).unwrap());
            game.settings.fov = 70.0;
            game.view.fov_modifier = 1.0;
            game.tick_fov();
            assert_eq!(game.view.fov_modifier, 1.5);
            // Interpolated modifier halfway between 1 and 1.5 at partial 0.5.
            assert_eq!(game.camera_fovs(0.5, AT), (0.5, 70.0));
            assert_eq!(
                *calls.lock().unwrap(),
                [(FovProjection::World, 87.5), (FovProjection::Hud, 70.0)]
            );

            game.studio.freecam.active = true;
            game.studio.freecam.fov = 42.0;
            assert_eq!(game.camera_fovs(0.5, AT), (42.0, 70.0));
            assert_eq!(
                calls.lock().unwrap().len(),
                2,
                "freecam did not fire either projection hook"
            );
        });
    });
}
