use super::*;

/// The hook points. Each does nothing in Sunrise.
impl Game<'_> {
    /// `key.togglePerspective` (F5) cycles the camera type: `previous` is the type it
    /// leaves, and `next` comes in as `CameraType.cycle` and may be set to another.
    pub(in crate::game) fn native_camera_type_cycle(
        &mut self,
        previous: mod_api::CameraType,
        next: &mut mod_api::CameraType,
    ) {
        self.fire_camera_type_cycle(previous, next);
    }
    /// The window's camera type (`Options.getCameraType`).
    pub(in crate::game) fn camera_type(&self) -> CameraType {
        self.requested_camera_type.get().unwrap_or(self.camera_type)
    }
    /// Queue the window's camera type without cycling, including from shared hooks.
    pub(in crate::game) fn set_camera_type(&self, camera_type: CameraType) {
        self.requested_camera_type.set(Some(camera_type));
    }
}

#[cfg(test)]
mod camera_type_tests {
    use crate::game::native_entity_refs::tests::{run_fresh_cases, with_client_game};
    use mod_api::{CameraType, CameraTypeCycleEvent, ClientHost, Hooks, ModEntry};
    use mod_platform::testing::with_mods;
    use std::sync::{Arc, Mutex};
    use sunrise_client::motion::PlayerMotion;
    use sunrise_core::geometry::Vec3;

    /// Skips the front view: F5 goes first person, behind, first person.
    struct NoFront(Arc<Mutex<Vec<(CameraType, CameraType)>>>);
    #[mod_api::__subscriptions]
    impl Hooks for NoFront {
        fn camera_type_cycle(
            &self,
            host: &mut dyn ClientHost,
            event: &mut CameraTypeCycleEvent<'_>,
        ) {
            self.0.lock().unwrap().push((event.previous, *event.next));
            host.set_camera_type(CameraType::ThirdPersonFront);
            if *event.next == CameraType::ThirdPersonFront {
                *event.next = CameraType::FirstPerson;
            }
        }
    }

    /// `key.togglePerspective`: the mod hears each cycle with vanilla's next type and its
    /// answer is the new type; setting the type does not cycle, and without a listener F5
    /// cycles as vanilla does.
    #[test]
    #[ignore = "daily"]
    fn a_client_mod_picks_the_camera_type_f5_cycles_to() {
        crate::game::native_entity_refs::tests::with_game(|game| {
            game.options.execution_role = super::ExecutionRole::Client;
            let mut seen = Vec::new();
            for _ in 0..3 {
                game.cycle_camera();
                seen.push(game.camera_type());
            }
            use CameraType::*;
            assert_eq!(seen, [ThirdPersonBack, ThirdPersonFront, FirstPerson]);
        });
        let heard = Arc::new(Mutex::new(Vec::new()));
        let mods: mod_api::Mods = vec![ModEntry::new(
            "no-front",
            Box::new(NoFront(Arc::clone(&heard))),
        )]
        .into();
        with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                use CameraType::*;
                game.options.execution_role = super::ExecutionRole::Client;
                game.cycle_camera();
                assert_eq!(game.camera_type(), ThirdPersonBack);
                game.cycle_camera();
                assert_eq!(game.camera_type(), FirstPerson, "the front view is skipped");
                game.set_camera_type(ThirdPersonFront);
                assert_eq!(game.camera_type(), ThirdPersonFront);
                assert_eq!(game.camera_type, FirstPerson, "the request is pending");
                assert_eq!(heard.lock().unwrap().len(), 2, "setting is not a cycle");
                game.cycle_camera();
                assert_eq!(game.camera_type, FirstPerson, "F5's final choice wins");
            });
        });
        use CameraType::*;
        assert_eq!(
            *heard.lock().unwrap(),
            [
                (FirstPerson, ThirdPersonBack),
                (ThirdPersonBack, ThirdPersonFront),
                (ThirdPersonFront, FirstPerson)
            ],
            "a prior request changes the next F5's previous type without an extra cycle"
        );
    }

    #[derive(Default)]
    struct Calls {
        angles: std::sync::atomic::AtomicUsize,
        positions: std::sync::atomic::AtomicUsize,
        cycles: std::sync::atomic::AtomicUsize,
    }
    struct SharedCamera(Arc<Calls>);
    #[mod_api::__subscriptions]
    impl Hooks for SharedCamera {
        fn compute_camera_angles(
            &self,
            host: &dyn ClientHost,
            event: &mut mod_api::ComputeCameraAnglesEvent,
        ) {
            self.0
                .angles
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            host.set_camera_type(CameraType::ThirdPersonFront);
            assert_eq!(host.camera_type(), CameraType::ThirdPersonFront);
            host.set_camera_type(CameraType::ThirdPersonBack);
            assert_eq!(host.camera_type(), CameraType::ThirdPersonBack);
            event.yaw += 180.0;
            event.roll = 180.0;
        }
        fn camera_position(&self, _: &dyn ClientHost, _: &mut mod_api::CameraPositionEvent<'_>) {
            self.0
                .positions
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        fn camera_type_cycle(&self, _: &mut dyn ClientHost, _: &mut CameraTypeCycleEvent<'_>) {
            self.0
                .cycles
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    #[test]
    #[ignore = "daily"]
    fn shared_camera_pose_requests_apply_before_frame_consumers() {
        use std::sync::atomic::Ordering;

        let calls = Arc::new(Calls::default());
        let mods: mod_api::Mods = vec![ModEntry::new(
            "shared-camera",
            Box::new(SharedCamera(Arc::clone(&calls))),
        )]
        .into();
        with_mods(mods, || {
            with_client_game(|game| {
                game.player = Some(PlayerMotion::new(game.motion, Vec3::ZERO).unwrap());
                game.title = None;
                game.spectated_post_effect = Some("minecraft:creeper");
                let posed = game.frame_camera(1.0).unwrap();
                assert_eq!(game.camera_type, CameraType::ThirdPersonBack);
                assert_eq!(game.camera_type(), CameraType::ThirdPersonBack);
                assert_eq!(game.spectated_post_effect, None);
                // The overridden yaw faces north, backing off 4 blocks south along +Z.
                assert_eq!([posed.0.x, posed.0.z], [0.0, 4.0]);
                assert_eq!((posed.1, posed.2, posed.3), (180.0, 0.0, 180.0));
                assert_eq!(
                    game.view.eye_rotation,
                    [0.0, 0.0],
                    "entity look is unchanged"
                );
            });
        });
        assert_eq!(calls.angles.load(Ordering::Relaxed), 1);
        assert_eq!(calls.positions.load(Ordering::Relaxed), 1);
        assert_eq!(calls.cycles.load(Ordering::Relaxed), 0);
    }

    struct EditX;
    #[mod_api::__subscriptions]
    impl Hooks for EditX {
        fn camera_position(
            &self,
            host: &dyn ClientHost,
            event: &mut mod_api::CameraPositionEvent<'_>,
        ) {
            event.position[0] = 12.0;
            host.set_camera_type(CameraType::FirstPerson);
        }
    }

    #[test]
    #[ignore = "daily"]
    fn frame_camera_position_keeps_only_changed_axes_when_realigning() {
        if run_fresh_cases(
            "game::native_hooks::camera_type::camera_type_tests::frame_camera_position_keeps_only_changed_axes_when_realigning",
            "SUNRISE_CAMERA_POSITION_CHILD",
            &["position"],
        ) {
            return;
        }
        with_mods(
            vec![ModEntry::new("edit-x", Box::new(EditX))].into(),
            || {
                with_client_game(|game| {
                    game.player = Some(PlayerMotion::new(game.motion, Vec3::ZERO).unwrap());
                    game.previous_position = Vec3::ZERO;
                    game.title = None;
                    game.yaw = 180.0;
                    game.view.eye_height = 1.0;
                    game.view.eye_height_old = 1.0;
                    game.camera_type = CameraType::ThirdPersonBack;
                    let behind = game.camera_view(1.0).unwrap().0;
                    assert_eq!(
                        behind.z, 4.0,
                        "third person looked north, backing off south"
                    );
                    let (position, _, _, _) = game.frame_camera(1.0).unwrap();
                    assert_eq!(game.camera_type, CameraType::FirstPerson);
                    assert_eq!(position.to_array(), [12.0, 1.0, 0.0]);

                    let custom = glam::DVec3::new(2.0, 31.0, 5.0);
                    game.studio.freecam.activate(custom, 180.0, 0.0, 70.0);
                    game.studio.freecam.roll = 90.0;
                    let (position, yaw, pitch, roll) = game.frame_camera(1.0).unwrap();
                    assert_eq!(position.to_array(), [12.0, 31.0, 5.0]);
                    assert_eq!((yaw, pitch, roll), (180.0, 0.0, -90.0));
                    assert_eq!(game.studio.freecam.position, custom);
                    assert_eq!(game.player.as_ref().unwrap().position, Vec3::ZERO);
                });
            },
        );
    }

    enum AngleWriter {
        Set,
        Invalid(Arc<std::sync::atomic::AtomicUsize>),
        Panic(Arc<std::sync::atomic::AtomicUsize>),
        Add,
    }
    #[mod_api::__subscriptions]
    impl Hooks for AngleWriter {
        fn compute_camera_angles(
            &self,
            _: &dyn ClientHost,
            event: &mut mod_api::ComputeCameraAnglesEvent,
        ) {
            use std::sync::atomic::Ordering;
            match self {
                Self::Set => (event.yaw, event.pitch, event.roll) = (17.0, 2.0, 10.0),
                Self::Invalid(calls) => {
                    calls.fetch_add(1, Ordering::Relaxed);
                    event.yaw = -5.0;
                    event.pitch = f32::NAN;
                }
                Self::Panic(calls) => {
                    calls.fetch_add(1, Ordering::Relaxed);
                    event.roll = 90.0;
                    panic!("fixture camera callback panicked");
                }
                Self::Add => event.yaw += 10.0,
            }
        }
    }

    #[test]
    #[ignore = "daily"]
    fn camera_angle_failures_keep_accepted_angles_and_only_panics_disable_the_listener() {
        if run_fresh_cases(
            "game::native_hooks::camera_type::camera_type_tests::camera_angle_failures_keep_accepted_angles_and_only_panics_disable_the_listener",
            "SUNRISE_CAMERA_ANGLE_FAILURE_CHILD",
            &["angles"],
        ) {
            return;
        }
        use std::sync::atomic::{AtomicUsize, Ordering};
        let invalid = Arc::new(AtomicUsize::new(0));
        let panics = Arc::new(AtomicUsize::new(0));
        let mods = vec![
            ModEntry::new("set", Box::new(AngleWriter::Set)),
            ModEntry::new("invalid", Box::new(AngleWriter::Invalid(invalid.clone()))),
            ModEntry::new("camera-panic", Box::new(AngleWriter::Panic(panics.clone()))),
            ModEntry::new("add", Box::new(AngleWriter::Add)),
        ];
        with_mods(mods.into(), || {
            with_client_game(|game| {
                game.player = Some(PlayerMotion::new(game.motion, Vec3::ZERO).unwrap());
                game.title = None;
                for _ in 0..2 {
                    let (_, yaw, pitch, roll) = game.frame_camera(1.0).unwrap();
                    assert_eq!((yaw, pitch, roll), (27.0, 2.0, 10.0));
                }
            });
        });
        assert_eq!(invalid.load(Ordering::Relaxed), 2);
        assert_eq!(panics.load(Ordering::Relaxed), 1);
        assert!(
            mod_api::__private::take_hook_panics()
                .iter()
                .any(|notice| notice.mod_id == "camera-panic"
                    && notice.hook == "compute_camera_angles")
        );
    }
}
