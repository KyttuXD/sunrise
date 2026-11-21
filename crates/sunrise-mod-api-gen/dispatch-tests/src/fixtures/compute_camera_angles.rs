struct AngleListener {
    edit: fn(&mut ComputeCameraAnglesEvent),
    heard: Arc<Mutex<Vec<[f32; 3]>>>,
}

#[mod_api::__subscriptions]
impl Hooks for AngleListener {
    fn compute_camera_angles(&self, _: &dyn ClientHost, event: &mut ComputeCameraAnglesEvent) {
        self.heard
            .lock()
            .unwrap()
            .push([event.yaw, event.pitch, event.roll]);
        (self.edit)(event);
    }
}

#[test]
fn camera_listeners_compose_and_only_a_panicking_listener_is_disabled() {
    let first = Arc::new(Mutex::new(Vec::new()));
    let refused = Arc::new(Mutex::new(Vec::new()));
    let panicking = Arc::new(Mutex::new(Vec::new()));
    let last = Arc::new(Mutex::new(Vec::new()));
    let listen = |id, heard: &Arc<Mutex<Vec<[f32; 3]>>>, edit| {
        ModEntry::new(
            id,
            Box::new(AngleListener {
                edit,
                heard: Arc::clone(heard),
            }),
        )
    };
    install(
        vec![
            listen("first", &first, |e| e.yaw += 30.0),
            listen("refused", &refused, |e| {
                e.yaw = f32::NAN;
                e.pitch = 99.0;
            }),
            listen("panicking", &panicking, |e| {
                e.yaw = 999.0;
                panic!("bad camera callback");
            }),
            listen("last", &last, |e| e.roll -= e.pitch),
        ]
        .into(),
    );

    for _ in 0..2 {
        let (mut yaw, mut pitch, mut roll) = (10.0, 5.0, 0.0);
        mod_api::dispatch_compute_camera_angles(&TestHost, 0.5, &mut yaw, &mut pitch, &mut roll);
        assert_eq!([yaw, pitch, roll], [40.0, 5.0, -5.0]);
    }
    assert_eq!(*first.lock().unwrap(), [[10.0, 5.0, 0.0]; 2]);
    assert_eq!(*refused.lock().unwrap(), [[40.0, 5.0, 0.0]; 2]);
    assert_eq!(*panicking.lock().unwrap(), [[40.0, 5.0, 0.0]]);
    assert_eq!(*last.lock().unwrap(), [[40.0, 5.0, 0.0]; 2]);
}

mod crate_root {
    mod client {
        pub(crate) mod camera {
            pub(crate) fn validate_camera_angles(
                yaw: &f32,
                _: &f32,
                _: &f32,
            ) -> Result<(), &'static str> {
                yaw.is_finite().then_some(()).ok_or("non-finite yaw")
            }

            pub(crate) fn validate_camera_position(_: &[f64; 3]) -> Result<(), &'static str> {
                Ok(())
            }
        }
    }
}
