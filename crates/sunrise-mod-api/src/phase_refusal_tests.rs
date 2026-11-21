#![cfg(test)]
// A native Host call the load phase refuses is contained and latched, not a crash (#3123).
use crate::*;
use mod_vocab::__private::{
    LoadAccess, ModPhase, PhaseOwner, PhaseRefusal, check_access, contain_refusal,
};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};

fn refusal(mod_id: &'static str, operation: &'static str, phase: ModPhase) -> PhaseRefusal {
    PhaseRefusal {
        mod_id,
        operation,
        phase,
    }
}

#[test]
fn a_refused_call_comes_back_naming_the_mod_the_operation_and_the_phase() {
    crate::__private::initialize().unwrap();
    let _owner = PhaseOwner::new(ModPhase::Setup);
    assert_eq!(
        contain_refusal("m", || check_access("level", LoadAccess::WorldOnly)),
        Err(refusal("m", "level", ModPhase::Setup))
    );
    // What the phase admits is not refused.
    assert_eq!(
        contain_refusal("m", || {
            check_access("setting", LoadAccess::WorldOnly);
            check_access("block_state_default", LoadAccess::Metadata);
            7
        }),
        Ok(7)
    );
}

#[test]
fn the_latch_refuses_every_later_call_whatever_its_class_and_survives_a_mods_own_catch() {
    crate::__private::initialize().unwrap();
    let _owner = PhaseOwner::new(ModPhase::Setup);
    let result = contain_refusal("m", || {
        let swallowed = catch_unwind(|| check_access("first", LoadAccess::WorldOnly));
        assert!(swallowed.is_err());
        // Allowed in Setup by itself, refused now, and the first refusal stays the one reported.
        let again = catch_unwind(|| check_access("setting", LoadAccess::WorldOnly));
        assert!(again.unwrap_err().is::<PhaseRefusal>());
        let again = catch_unwind(|| check_access("block_state_default", LoadAccess::Metadata));
        assert!(again.unwrap_err().is::<PhaseRefusal>());
        "returned normally"
    });
    assert_eq!(result, Err(refusal("m", "first", ModPhase::Setup)));
    // The scope ended with the call: a later scope starts clean.
    assert_eq!(
        contain_refusal("m", || check_access("setting", LoadAccess::WorldOnly)),
        Ok(())
    );
}

#[test]
fn nested_scopes_save_and_restore_and_other_panics_go_on_unwinding() {
    crate::__private::initialize().unwrap();
    let _owner = PhaseOwner::new(ModPhase::Setup);
    let outer = contain_refusal("outer", || {
        let inner = contain_refusal("inner", || check_access("a", LoadAccess::WorldOnly));
        assert_eq!(inner, Err(refusal("inner", "a", ModPhase::Setup)));
        // The inner refusal did not latch the outer scope.
        check_access("setting", LoadAccess::WorldOnly);
    });
    assert_eq!(outer, Ok(()));
    let panic = catch_unwind(|| contain_refusal("m", || panic!("a real fault"))).unwrap_err();
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"a real fault"));
}

#[test]
fn without_a_scope_the_gate_panics_with_the_named_message() {
    crate::__private::initialize().unwrap();
    let _owner = PhaseOwner::new(ModPhase::Reloading);
    let panic = catch_unwind(|| check_access("level", LoadAccess::WorldOnly)).unwrap_err();
    assert_eq!(
        panic.downcast_ref::<String>().unwrap(),
        "mod API level is unavailable during Reloading"
    );
}

#[test]
fn the_phase_is_per_thread_one_thread_reloading_another_in_world_is_allowed() {
    crate::__private::initialize().unwrap();
    let owner = PhaseOwner::new(ModPhase::Setup);
    let _reloading = owner.reloading().unwrap();
    assert!(catch_unwind(|| check_access("level", LoadAccess::WorldOnly)).is_err());
    // The integrated server's thread keeps its own phase while the window reloads.
    std::thread::spawn(|| check_access("level", LoadAccess::WorldOnly))
        .join()
        .unwrap();
}

struct Probe {
    seen: Arc<Mutex<Vec<&'static str>>>,
    name: &'static str,
    write: Option<usize>,
    refuse: bool,
    cancel: bool,
}
#[crate::__subscriptions]
impl Hooks for Probe {
    fn potion_brew_pre(&self, _: &mut dyn ServerHost, event: &mut PotionBrewPreEvent<'_>) -> Flow {
        self.seen.lock().unwrap().push(self.name);
        if let Some(slot) = self.write {
            event.stacks[slot] = Some(crate::__private::part_tests::stack(
                "minecraft:diamond",
                1,
                0,
            ));
        }
        if self.refuse {
            // A mod's own catch does not undo the refusal.
            let _ = catch_unwind(AssertUnwindSafe(|| {
                check_access("entity", LoadAccess::WorldOnly)
            }));
            event.cancelled = true;
        }
        if self.cancel {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }
}

struct NoHost;
impl Host for NoHost {}
crate::impl_host_areas!(NoHost);
impl ServerHost for NoHost {}
crate::impl_server_host_areas!(NoHost);

#[test]
fn a_persistence_write_without_a_backend_returns_unavailable() {
    let entity = EntityRef::new(WorldEpoch::new(1), 1);
    assert_eq!(
        entity.set_persistent(&mut NoHost, false),
        Err(LevelWriteError::Unavailable)
    );
}

fn brew(phase: ModPhase, listeners: Vec<Probe>) -> ([Option<ItemStack>; 5], bool) {
    crate::__private::initialize().unwrap();
    crate::__private::install_parts(&[], &crate::__private::part_tests::rule_schema(), |_| false)
        .unwrap();
    let mods = listeners
        .into_iter()
        .enumerate()
        .map(|(index, probe)| ModEntry::new(["first", "refused", "last"][index], Box::new(probe)))
        .collect::<Vec<_>>()
        .into();
    let _owner = PhaseOwner::new(phase);
    testing::with_mods(mods, || {
        let mut stacks = [None, None, None, None, None];
        let mut cancelled = false;
        dispatch_potion_brew_pre(&mut NoHost, [1, 2, 3], &mut stacks, &mut cancelled);
        (stacks, cancelled)
    })
}

#[test]
fn a_refused_listener_is_dropped_its_outputs_restored_and_the_hook_goes_on() {
    for phase in [ModPhase::Setup, ModPhase::Reloading] {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let probe = |name, write, refuse, cancel| Probe {
            seen: seen.clone(),
            name,
            write,
            refuse,
            cancel,
        };
        let (stacks, cancelled) = brew(
            phase,
            vec![
                probe("first", Some(0), false, false),
                probe("refused", Some(1), true, true),
                probe("last", Some(2), false, false),
            ],
        );
        assert_eq!(*seen.lock().unwrap(), ["first", "refused", "last"]);
        // The earlier listener's answer stands, the refused one's write and cancel are undone, the
        // later listener still ran.
        assert!(stacks[0].is_some() && stacks[1].is_none() && stacks[2].is_some());
        assert!(!cancelled);
    }
}

#[test]
fn in_world_a_listener_runs_the_plain_call() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (stacks, cancelled) = brew(
        ModPhase::World,
        vec![Probe {
            seen,
            name: "only",
            write: Some(0),
            refuse: false,
            cancel: true,
        }],
    );
    assert!(stacks[0].is_some() && cancelled);
}

#[test]
fn a_phase_refusal_is_reported_each_time_and_never_disables_the_listener() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    crate::__private::initialize().unwrap();
    let _phase = PhaseOwner::scope(ModPhase::Setup);

    struct Refused {
        before: Arc<AtomicUsize>,
        after: Arc<AtomicUsize>,
    }
    #[crate::__subscriptions]
    impl Hooks for Refused {
        fn client_tick_pre(&self, _: &mut dyn ClientHost, _: &mut ClientTickPreEvent) {
            self.before.fetch_add(1, Ordering::Relaxed);
            check_access("world_only_op", LoadAccess::WorldOnly);
            self.after.fetch_add(1, Ordering::Relaxed);
        }
    }

    let before = Arc::new(AtomicUsize::new(0));
    let after = Arc::new(AtomicUsize::new(0));

    testing::with_dispatch(
        vec![ModEntry::new(
            "phase_refused_mod",
            Box::new(Refused {
                before: before.clone(),
                after: after.clone(),
            }),
        )]
        .into(),
        || {
            let mut host = testing::FakeHost::new();
            dispatch_client_tick_pre(&mut host);
            dispatch_client_tick_pre(&mut host);
            assert!(mod_vocab::__private::take_hook_panics().is_empty());
        },
    );

    assert_eq!(before.load(Ordering::Relaxed), 2);
    assert_eq!(after.load(Ordering::Relaxed), 0);
}

#[test]
fn panicking_registration_discards_owned_renderers_even_if_cleanup_panics() {
    use crate::__private::game::RegistrationContextNew;
    use mod_vocab::__private::{HookPanicReport, take_hook_panics};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Healthy(&'static str);
    #[crate::__subscriptions]
    impl Hooks for Healthy {
        fn register_debug_renderers(
            &self,
            _: &RegistrationContext,
            event: &mut RegisterDebugRenderersEvent<'_>,
        ) {
            event.register(DebugRenderer::new(self.0.into(), |_, _, _| {}));
        }
    }

    struct Capture {
        drops: Arc<Mutex<Vec<&'static str>>>,
        name: &'static str,
        panic: bool,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            self.drops.lock().unwrap().push(self.name);
            assert!(!self.panic, "registration cleanup failed");
        }
    }

    struct Broken {
        calls: Arc<AtomicUsize>,
        drops: Arc<Mutex<Vec<&'static str>>>,
    }
    #[crate::__subscriptions]
    impl Hooks for Broken {
        fn register_debug_renderers(
            &self,
            _: &RegistrationContext,
            event: &mut RegisterDebugRenderersEvent<'_>,
        ) {
            self.calls.fetch_add(1, Ordering::Relaxed);
            for (name, panic) in [("broken:one", true), ("broken:two", false)] {
                let capture = Capture {
                    drops: self.drops.clone(),
                    name,
                    panic,
                };
                event.register(DebugRenderer::new(name.into(), move |_, _, _| {
                    let _ = &capture;
                }));
            }
            panic!("registration failed");
        }
    }

    let _phase = PhaseOwner::scope(ModPhase::Registration);
    let calls = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(Mutex::new(Vec::new()));
    let mods = vec![
        ModEntry::new("first", Box::new(Healthy("switches:first"))),
        ModEntry::new(
            "broken",
            Box::new(Broken {
                calls: calls.clone(),
                drops: drops.clone(),
            }),
        ),
        ModEntry::new("last", Box::new(Healthy("switches:last"))),
    ];
    testing::with_mods(mods.into(), || {
        for _ in 0..2 {
            let mut renderers = Vec::new();
            dispatch_register_debug_renderers(&RegistrationContext::new(), &mut renderers);
            assert_eq!(
                renderers
                    .iter()
                    .map(|entry| (entry.mod_id, entry.answer.id()))
                    .collect::<Vec<_>>(),
                [("first", "switches:first"), ("last", "switches:last")]
            );
        }
        assert_eq!(
            take_hook_panics(),
            [HookPanicReport {
                mod_id: "broken",
                hook: "register_debug_renderers",
            }; 2]
        );
    });
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(*drops.lock().unwrap(), ["broken:one", "broken:two"]);
}

#[test]
fn transition_registration_reports_duplicates_and_does_not_mint_refused_keys() {
    use crate::__private::game::RegistrationContextNew;
    use crate::__private::{registry_entry_key_slot, take_hook_panics};

    struct Probe {
        results: Arc<Mutex<Vec<bool>>>,
        keys: &'static [&'static str],
        refuse: bool,
    }
    #[crate::__subscriptions]
    impl Hooks for Probe {
        fn register_dimension_transition_screen(
            &self,
            _: &RegistrationContext,
            event: &mut RegisterDimensionTransitionScreenEvent<'_>,
        ) {
            for &key in self.keys {
                let accepted = event.register(DimensionTransitionScreen::new(
                    "example:loading".into(),
                    DimensionTransition::Incoming(key.into()),
                    |_, _| Box::new(|_: &dyn ClientHost, _, _, _: &mut dyn DrawContext| {}),
                ));
                self.results.lock().unwrap().push(accepted);
            }
            if self.refuse {
                check_access("transition_world_only", LoadAccess::WorldOnly);
            }
        }
    }

    const REFUSED_ONLY: &str = "transition_refusal:unpublished";
    const LISTENERS: [(&str, &[&str], bool); 3] = [
        ("first", &["example:a", "example:a"], false),
        ("refused", &["example:a", "example:b", REFUSED_ONLY], true),
        ("last", &["example:a", "example:b"], false),
    ];
    let results = Arc::new(Mutex::new(Vec::new()));
    let mods = LISTENERS
        .into_iter()
        .map(|(id, keys, refuse)| {
            ModEntry::new(
                id,
                Box::new(Probe {
                    results: results.clone(),
                    keys,
                    refuse,
                }),
            )
        })
        .collect::<Vec<_>>()
        .into();
    testing::with_mods(mods, || {
        let _phase = PhaseOwner::scope(ModPhase::Registration);
        let refused_only = RegistryEntry::new(
            Identifier::parse("minecraft:dimension").unwrap(),
            Identifier::parse(REFUSED_ONLY).unwrap(),
        );
        assert_eq!(registry_entry_key_slot(&refused_only), None);
        let mut screens = Vec::new();
        dispatch_register_dimension_transition_screen(&RegistrationContext::new(), &mut screens);
        assert_eq!(
            *results.lock().unwrap(),
            [true, false, false, true, true, false, true]
        );
        assert_eq!(
            screens
                .iter()
                .map(|screen| screen.owner)
                .collect::<Vec<_>>(),
            ["first", "last"]
        );
        assert_eq!(registry_entry_key_slot(&refused_only), None);
        assert!(take_hook_panics().is_empty());
    });
}

#[test]
fn command_registrars_keep_earlier_usages_and_discard_a_refused_batch() {
    struct Commands(&'static str, bool);
    #[crate::__subscriptions]
    impl Hooks for Commands {
        fn register_commands(&self, _: &mut dyn ServerHost, event: &mut RegisterCommandsEvent<'_>) {
            event.register(Command::new(self.0));
            if self.1 {
                check_access("server_world_only", LoadAccess::WorldOnly);
            }
        }
        fn register_client_commands(
            &self,
            _: &mut dyn ClientHost,
            event: &mut RegisterClientCommandsEvent<'_>,
        ) {
            event.register(Command::new(self.0));
            if self.1 {
                check_access("client_world_only", LoadAccess::WorldOnly);
            }
        }
    }

    crate::__private::initialize().unwrap();
    let _phase = PhaseOwner::scope(ModPhase::Setup);
    testing::with_dispatch(
        vec![
            ModEntry::new("first", Box::new(Commands("first <argument:word>", false))),
            ModEntry::new("refused", Box::new(Commands("refused", true))),
            ModEntry::new("last", Box::new(Commands("last", false))),
        ]
        .into(),
        || {
            let mut host = testing::FakeHost::new();
            let mut server = Vec::new();
            let mut client = Vec::new();
            dispatch_register_commands(&mut host, &mut server);
            dispatch_register_client_commands(&mut host, &mut client);
            for commands in [server, client] {
                assert_eq!(
                    commands
                        .into_iter()
                        .map(|command| command.usage)
                        .collect::<Vec<_>>(),
                    ["first <argument:word>", "last"]
                );
            }
        },
    );
}
