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
    use mod_api::{CameraType, CameraTypeCycleEvent, ClientHost, Hooks, ModEntry};
    use mod_platform::testing::with_mods;
    use std::sync::{Arc, Mutex};

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
            event.yaw = 180.0;
            event.roll = 180.0;
        }
        fn camera_position(
            &self,
            _: &dyn ClientHost,
            event: &mut mod_api::CameraPositionEvent<'_>,
        ) {
            self.0
                .positions
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // Third-person-back at yaw 180 faces -Z, so detached placement moved into +Z.
            // Retain that coordinate to check placement order from the engine's result.
            event.position[0] = 32.25;
            event.position[1] = 80.0;
        }
        fn render_view_transform(
            &self,
            _: &dyn ClientHost,
            event: &mut mod_api::RenderViewTransformEvent<'_>,
        ) {
            event.transform.translate(8.0, -2.0, 1.0);
        }
        fn camera_type_cycle(&self, _: &mut dyn ClientHost, _: &mut CameraTypeCycleEvent<'_>) {
            self.0
                .cycles
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    #[test]
    #[ignore = "daily"]
    fn angles_position_and_view_transform_apply_at_their_frame_stages() {
        use std::sync::atomic::Ordering;
        use sunrise_client::motion::PlayerMotion;
        use sunrise_core::geometry::Vec3;

        let calls = Arc::new(Calls::default());
        let mods: mod_api::Mods = vec![ModEntry::new(
            "shared-camera",
            Box::new(SharedCamera(Arc::clone(&calls))),
        )]
        .into();
        with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_client_game(|game| {
                game.player = Some(PlayerMotion::new(game.motion, Vec3::ZERO).unwrap());
                game.title = None;
                game.spectated_post_effect = Some("minecraft:creeper");
                let posed = game.frame_camera(1.0).unwrap();
                assert_eq!(game.camera_type, CameraType::ThirdPersonBack);
                assert_eq!(game.camera_type(), CameraType::ThirdPersonBack);
                assert_eq!(game.spectated_post_effect, None);
                assert_eq!(
                    posed,
                    (glam::DVec3::new(32.25, 80.0, 4.0), 180.0, 0.0, 180.0)
                );
                assert_eq!(game.view.eye, [0.0; 3]);
                assert_eq!(game.view.eye_rotation, [0.0; 2]);
                assert_eq!(game.player.as_ref().unwrap().position, Vec3::ZERO);
                game.settings.view_bobbing = false;
                assert_eq!(
                    game.bob_matrix(1.0).w_axis,
                    glam::Vec4::new(8.0, -2.0, 1.0, 1.0)
                );
                assert_eq!(game.view.position, [32.25, 80.0, 4.0]);
            });
        });
        assert_eq!(calls.angles.load(Ordering::Relaxed), 1);
        assert_eq!(calls.positions.load(Ordering::Relaxed), 1);
        assert_eq!(calls.cycles.load(Ordering::Relaxed), 0);
    }
}
