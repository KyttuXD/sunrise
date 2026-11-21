//! The `living_travel` hook's contract at its one firing point,
//! `travel_gate::decide` (#2414 PR 2, design #2308): what a listener may
//! decide for one travel invocation, what is refused, and what a panic
//! leaves behind.
use mod_api::{Flow, Hooks, LivingTravelEvent, LoopContext, TravelMode};
use std::sync::{Arc, Mutex};
use sunrise_core::geometry::Vec3;
use sunrise_world::native_load_context::{DataLoadOrigin, LoadContext, LoadInputs, LoopSide};

/// The server's loop context, the side the hook fires on: no resources, just
/// the side and the origin a tick's own loop context carries.
fn server_context() -> sunrise_world::native_load_context::LoopContext {
    LoadContext::new(
        DataLoadOrigin::ServerDataLoad,
        Arc::new(LoadInputs::new(Default::default(), Default::default())),
        [],
    )
    .loop_context(LoopSide::Server, [])
}

/// The same context on the client's side, whose own prediction the hook
/// must leave alone (#2414: no client-side prediction of a custom travel).
fn client_context() -> sunrise_world::native_load_context::LoopContext {
    LoadContext::new(
        DataLoadOrigin::ServerDataLoad,
        Arc::new(LoadInputs::new(Default::default(), Default::default())),
        [],
    )
    .loop_context(LoopSide::Client, [])
}

/// What the probe listener answers for one travel.
#[derive(Clone, Copy)]
enum Answer {
    Continue,
    Cancel,
    Replace { input: [f64; 3], velocity: [f64; 3] },
    PanicAfterWrite,
}

/// What the probe heard: entity, mode, and the resolved input and velocity.
type Heard = Vec<(u32, TravelMode, [f64; 3], [f64; 3])>;

/// A listener that records the travel it heard and answers `Answer`.
struct Probe {
    answer: Answer,
    heard: Arc<Mutex<Heard>>,
}

#[mod_api::__subscriptions]
impl Hooks for Probe {
    fn living_travel(&self, _: &dyn LoopContext, event: &mut LivingTravelEvent<'_>) -> Flow {
        self.heard.lock().unwrap().push((
            event.entity,
            event.mode,
            event.motion.input(),
            event.motion.velocity(),
        ));
        match self.answer {
            Answer::Continue => Flow::Continue,
            Answer::Cancel => Flow::Cancel,
            Answer::Replace { input, velocity } => {
                let _ = event.motion.set_input(input);
                let _ = event.motion.set_velocity(velocity);
                Flow::Continue
            }
            Answer::PanicAfterWrite => {
                let _ = event.motion.set_velocity([71.0, 0.0, 0.0]);
                event.cancelled = true;
                panic!("probe listener unwinds")
            }
        }
    }
}

/// One asked travel with the probe installed: whether it runs, the values
/// its branch starts from, and what the listener heard. The mode closure
/// counts its own calls, so a no-listener travel can prove it never runs.
fn decided_with(
    answer: Answer,
    context: &sunrise_world::native_load_context::LoopContext,
) -> (bool, Vec3, Vec3, Heard, usize) {
    let (log, mods) = install(answer);
    let mut input = Vec3::new(0.216, 0.0, 0.0);
    let mut velocity = Vec3::new(1.0, -0.078_4, 0.0);
    let mode_calls = Arc::new(Mutex::new(0usize));
    let counted = Arc::clone(&mode_calls);
    mod_platform::testing::with_mods(mods, || {
        let runs = sunrise_client::travel_gate::decide(
            context,
            7,
            "minecraft:zombie",
            move || {
                *counted.lock().unwrap() += 1;
                sunrise_world::travel::TravelMode::Water
            },
            &mut input,
            &mut velocity,
        );
        assert!(runs, "the travel always runs in these tests' answers");
    });
    (
        true,
        input,
        velocity,
        std::mem::take(&mut *log.lock().unwrap()),
        *mode_calls.lock().unwrap(),
    )
}

/// The player's walk step (0.216 blocks) and its landing speed, as a travel
/// resolves them: small values, not just big ones.
fn install(answer: Answer) -> (Arc<Mutex<Heard>>, mod_api::Mods) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
        "probe",
        Box::new(Probe {
            answer,
            heard: Arc::clone(&log),
        }),
    )]
    .into();
    (log, mods)
}

/// A Continue listener leaves the resolved travel as it was, and hears the
/// entity, its mode, and the resolved input and velocity.
#[test]
fn continue_runs_the_resolved_travel() {
    let (_, input, velocity, heard, mode_calls) = decided_with(Answer::Continue, &server_context());
    assert_eq!(
        heard,
        vec![(
            7,
            TravelMode::Water,
            [0.216, 0.0, 0.0],
            [1.0, -0.078_4, 0.0]
        )],
        "the listener hears the resolved values"
    );
    assert_eq!(mode_calls, 1, "the mode is built for the listener");
    assert_eq!(input, Vec3::new(0.216, 0.0, 0.0));
    assert_eq!(velocity, Vec3::new(1.0, -0.078_4, 0.0));
}

/// A cancelling listener skips the invocation: the travel does not run.
#[test]
fn cancel_skips_the_invocation() {
    let (log, mods) = install(Answer::Cancel);
    let mut input = Vec3::new(0.216, 0.0, 0.0);
    let mut velocity = Vec3::new(1.0, -0.078_4, 0.0);
    mod_platform::testing::with_mods(mods, || {
        let runs = sunrise_client::travel_gate::decide(
            &server_context(),
            7,
            "minecraft:zombie",
            || sunrise_world::travel::TravelMode::Air,
            &mut input,
            &mut velocity,
        );
        assert!(!runs, "a cancelled travel does not run");
    });
    assert_eq!(
        (input, velocity),
        (Vec3::new(0.216, 0.0, 0.0), Vec3::new(1.0, -0.078_4, 0.0)),
        "a cancelled travel leaves the resolved values untouched"
    );
    assert_eq!(log.lock().unwrap().len(), 1);
}

#[test]
fn finite_travel_edits_have_no_synthetic_component_caps() {
    let (_, input, velocity, _, _) = decided_with(
        Answer::Replace {
            input: [-2.0, 3.0, 0.0],
            velocity: [80.0, -100.0, 0.0],
        },
        &server_context(),
    );
    assert_eq!(input, Vec3::new(-2.0, 3.0, 0.0));
    assert_eq!(velocity, Vec3::new(80.0, -100.0, 0.0));
}

/// The live writes already completed when a callback panics are not replies to undo.
#[test]
fn a_panic_leaves_completed_writes_and_only_disables_its_own_listener() {
    let entries = [
        (
            "replacer",
            Answer::Replace {
                input: [0.5, 0.0, 0.0],
                velocity: [9.0, 0.0, 0.0],
            },
        ),
        ("panicker", Answer::PanicAfterWrite),
        ("observer", Answer::Continue),
    ];
    let [first, panicked, later] = std::array::from_fn(|_| Arc::new(Mutex::new(Vec::new())));
    let mods: mod_api::Mods = entries
        .into_iter()
        .zip([first, panicked.clone(), later.clone()])
        .map(|((id, answer), heard)| mod_api::ModEntry::new(id, Box::new(Probe { answer, heard })))
        .collect::<Vec<_>>()
        .into();
    mod_platform::testing::with_mods(mods, || {
        for expected in [71.0, 9.0] {
            let mut input = Vec3::ZERO;
            let mut velocity = Vec3::ZERO;
            let runs = sunrise_client::travel_gate::decide(
                &server_context(),
                7,
                "minecraft:zombie",
                || sunrise_world::travel::TravelMode::Air,
                &mut input,
                &mut velocity,
            );
            assert!(
                runs,
                "a listener that panicked did not complete its cancellation"
            );
            assert_eq!(input, Vec3::new(0.5, 0.0, 0.0));
            assert_eq!(velocity, Vec3::new(expected, 0.0, 0.0));
        }
    });
    assert_eq!(panicked.lock().unwrap().len(), 1);
    assert_eq!(
        *later.lock().unwrap(),
        [
            (7, TravelMode::Air, [0.5, 0.0, 0.0], [71.0, 0.0, 0.0]),
            (7, TravelMode::Air, [0.5, 0.0, 0.0], [9.0, 0.0, 0.0]),
        ],
        "the later listener runs both times and sees completed writes",
    );
}

/// A client's own prediction asks nobody: the hook fires on the
/// authoritative side only (#2414: no client-side prediction until PR 3),
/// and the mode closure never runs for it.
#[test]
fn a_client_side_travel_asks_nobody() {
    let (_, input, velocity, heard, mode_calls) = decided_with(Answer::Continue, &client_context());
    assert!(heard.is_empty(), "a client travel fires nobody");
    assert_eq!(mode_calls, 0, "the mode is not built without a dispatch");
    assert_eq!(
        (input, velocity),
        (Vec3::new(0.216, 0.0, 0.0), Vec3::new(1.0, -0.078_4, 0.0))
    );
}

/// With no listener the mode closure never runs: the resolved values stand
/// and the tick cost is one branch (#2414 ruling: no snapshot, no scan).
#[test]
fn without_listeners_the_mode_is_never_built() {
    let mut input = Vec3::new(0.216, 0.0, 0.0);
    let mut velocity = Vec3::new(1.0, -0.078_4, 0.0);
    let mode_calls = Arc::new(Mutex::new(0usize));
    let counted = Arc::clone(&mode_calls);
    mod_platform::testing::with_mods(
        Vec::<mod_api::ModEntry>::new().into_iter().collect(),
        || {
            let runs = sunrise_client::travel_gate::decide(
                &server_context(),
                7,
                "minecraft:zombie",
                move || {
                    *counted.lock().unwrap() += 1;
                    sunrise_world::travel::TravelMode::Lava
                },
                &mut input,
                &mut velocity,
            );
            assert!(runs);
        },
    );
    assert_eq!(*mode_calls.lock().unwrap(), 0, "no listener, no mode");
    assert_eq!(
        (input, velocity),
        (Vec3::new(0.216, 0.0, 0.0), Vec3::new(1.0, -0.078_4, 0.0))
    );
}

/// A mod whose `living_travel` listener panicked keeps every other hook:
/// the poison skips this hook alone (#2414 ruling A), so its `entity_move`
/// still hears the same entity on the same session — the two hooks of one
/// travel, the decision above and the move below.
#[test]
fn a_poisoned_travel_listener_keeps_its_move_hook() {
    use mod_api::EntityMoveEvent;
    struct Both {
        travels: Arc<Mutex<usize>>,
        moves: Arc<Mutex<usize>>,
    }
    #[mod_api::__subscriptions]
    impl Hooks for Both {
        fn living_travel(&self, _: &dyn LoopContext, _: &mut LivingTravelEvent<'_>) -> Flow {
            *self.travels.lock().unwrap() += 1;
            panic!("the travel listener unwinds")
        }
        fn entity_move(&self, _: &dyn LoopContext, _: &mut EntityMoveEvent<'_>) -> Flow {
            *self.moves.lock().unwrap() += 1;
            Flow::Continue
        }
    }
    let travels = Arc::new(Mutex::new(0usize));
    let moves = Arc::new(Mutex::new(0usize));
    let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
        "both",
        Box::new(Both {
            travels: travels.clone(),
            moves: moves.clone(),
        }),
    )]
    .into();
    mod_platform::testing::with_mods(mods, || {
        let context = server_context();
        let mut input = Vec3::new(0.216, 0.0, 0.0);
        let mut velocity = Vec3::new(1.0, 0.0, 0.0);
        let runs = sunrise_client::travel_gate::decide(
            &context,
            7,
            "minecraft:zombie",
            || sunrise_world::travel::TravelMode::Air,
            &mut input,
            &mut velocity,
        );
        assert!(runs);
        // The other hook of the same mod still runs, on the same session.
        let mut gate = sunrise_world::move_gate::MoveGate::new();
        sunrise_client::move_gate::arm(
            &mut gate,
            &context,
            7,
            "minecraft:zombie",
            sunrise_world::entity_move::MoverType::SelfMovement,
            Vec3::new(1.0, 0.0, 0.0),
        );
        assert_eq!(*moves.lock().unwrap(), 1, "the move hook still runs");
    });
    assert_eq!(*travels.lock().unwrap(), 1);
}

#[test]
fn a_refused_edit_leaves_the_other_write_and_cancellation_in_place() {
    struct Decide {
        invalid_input: bool,
        accepted: Arc<Mutex<Vec<(bool, bool)>>>,
    }
    #[mod_api::__subscriptions]
    impl Hooks for Decide {
        fn living_travel(&self, _: &dyn LoopContext, event: &mut LivingTravelEvent<'_>) -> Flow {
            let input = if self.invalid_input {
                [0.0, f64::NAN, 0.0]
            } else {
                [2.0, 0.0, 0.0]
            };
            let velocity = if self.invalid_input {
                [80.0, 0.0, 0.0]
            } else {
                [0.0, f64::INFINITY, 0.0]
            };
            let wrote_input = event.motion.set_input(input);
            let wrote_velocity = event.motion.set_velocity(velocity);
            self.accepted
                .lock()
                .unwrap()
                .push((wrote_input, wrote_velocity));
            if self.invalid_input {
                event.cancelled = true;
                Flow::Continue
            } else {
                Flow::Cancel
            }
        }
    }

    for (invalid_input, expected_input, expected_velocity) in
        [(true, 0.25, 80.0), (false, 2.0, 1.0)]
    {
        let accepted = Arc::new(Mutex::new(Vec::new()));
        let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
            "decide",
            Box::new(Decide {
                invalid_input,
                accepted: accepted.clone(),
            }),
        )]
        .into();
        mod_platform::testing::with_mods(mods, || {
            // Repeating a refusal must not disable this listener on the next invocation.
            for _ in 0..2 {
                let mut input = Vec3::new(0.25, 0.0, 0.0);
                let mut velocity = Vec3::new(1.0, 0.0, 0.0);
                let runs = sunrise_client::travel_gate::decide(
                    &server_context(),
                    7,
                    "minecraft:zombie",
                    || sunrise_world::travel::TravelMode::Air,
                    &mut input,
                    &mut velocity,
                );
                assert!(!runs);
                assert_eq!(input, Vec3::new(expected_input, 0.0, 0.0));
                assert_eq!(velocity, Vec3::new(expected_velocity, 0.0, 0.0));
            }
        });
        assert_eq!(
            *accepted.lock().unwrap(),
            [(!invalid_input, invalid_input); 2]
        );
    }
}
