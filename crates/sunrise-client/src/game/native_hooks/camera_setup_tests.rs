use mod_api::{CameraPositionEvent, ClientHost, Hooks, ModEntry, ViewTransformEvent};
use mod_platform::testing::with_mods;
use sunrise_client::motion::PlayerMotion;
use sunrise_core::geometry::Vec3;

enum PositionEdit {
    Place,
    Refuse,
    Move,
}
#[mod_api::__subscriptions]
impl Hooks for PositionEdit {
    fn camera_position(&self, _: &dyn ClientHost, event: &mut CameraPositionEvent<'_>) {
        match self {
            Self::Place => {
                assert_eq!(*event.position, [2.0, 65.0, 4.0]);
                *event.position = [16.0, 72.0, -4.0];
            }
            Self::Refuse => event.position[0] = f64::NAN,
            Self::Move => {
                assert_eq!(*event.position, [16.0, 72.0, -4.0]);
                event.position[0] += 200.0;
            }
        }
    }
}

#[test]
#[ignore = "daily"]
fn render_position_edits_compose_without_moving_the_entity_eye() {
    let mods = vec![
        ModEntry::new("position-first", Box::new(PositionEdit::Place)),
        ModEntry::new("position-refused", Box::new(PositionEdit::Refuse)),
        ModEntry::new("position-second", Box::new(PositionEdit::Move)),
    ]
    .into();
    with_mods(mods, || {
        crate::game::native_entity_refs::tests::with_client_game(|game| {
            let body = Vec3::new(2.0, 64.0, 4.0);
            game.player = Some(PlayerMotion::new(game.motion, body).unwrap());
            game.title = None;
            game.view.eye_height = 1.0;
            game.view.eye_height_old = 1.0;
            let (camera, yaw, pitch) = game.setup_camera(1.0, true).unwrap();
            assert_eq!(camera.to_array(), [216.0, 72.0, -4.0]);
            assert_eq!(game.view.eye, [2.0, 65.0, 4.0]);
            assert_eq!([yaw, pitch, game.view.roll], [0.0; 3]);
            assert_eq!(game.player.as_ref().unwrap().position, body);
        });
    });
}

struct TranslateView;
#[mod_api::__subscriptions]
impl Hooks for TranslateView {
    fn view_transform(&self, host: &dyn ClientHost, event: &mut ViewTransformEvent<'_>) {
        assert_eq!(*event.transform, glam::Mat4::IDENTITY.to_cols_array());
        assert_eq!(host.graphics_options().screen_effect_scale, 1.0);
        event.transform[12] = 2.0;
        event.transform[13] = -3.0;
    }
}

#[test]
#[ignore = "daily"]
fn view_transform_changes_projection_without_changing_camera_position() {
    with_mods(
        vec![ModEntry::new("translate-view", Box::new(TranslateView))].into(),
        || {
            crate::game::native_entity_refs::tests::with_client_game(|game| {
                game.player = Some(PlayerMotion::new(game.motion, Vec3::ZERO).unwrap());
                game.title = None;
                let camera = game.setup_camera(1.0, true).unwrap();
                assert_eq!(
                    game.bob_matrix(1.0)
                        .transform_point3(glam::Vec3::new(1.0, 2.0, -8.0)),
                    glam::Vec3::new(3.0, -1.0, -8.0),
                );
                assert_eq!(game.view.position, camera.0.to_array());
            });
        },
    );
}
