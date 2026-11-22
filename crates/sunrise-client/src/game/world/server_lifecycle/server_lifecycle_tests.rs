use crate::game::{
    execution_role::ExecutionRole, native_entity_refs::tests::with_game,
    native_guest_turn_tests::with_guests,
};
use mod_api::{
    ConfigEvent, ConfigKind, ConfigListener, ConfigSnapshot, ConfigSpec, Hooks, HostSide, ModEntry,
    Mods, Registrar, RegistrationContext, ServerHost,
};
use mod_platform::testing::with_mods;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

struct PanicAt {
    stopping: AtomicBool,
    save: AtomicBool,
    config_unloading: AtomicBool,
    config_registered: AtomicBool,
    logout: AtomicBool,
}

struct StoppedProbe {
    serving_stopped: Box<dyn Fn() -> bool + Send + Sync>,
    lan: std::sync::Weak<sunrise_client::lan::Shared>,
    root: std::path::PathBuf,
    save: Arc<sunrise_client::save::SaveDir>,
}

/// Exercise the real stop/save paths, not just direct calls to hook dispatch.
#[test]
#[ignore = "daily"]
fn every_started_server_stops_and_saves_even_when_startup_or_ticks_fail() {
    struct Log(
        Arc<Mutex<Vec<&'static str>>>,
        Arc<PanicAt>,
        Arc<Mutex<Option<StoppedProbe>>>,
    );
    #[mod_api::__subscriptions]
    impl Hooks for Log {
        fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
            if self.1.config_registered.load(Ordering::Relaxed) {
                let log = self.0.clone();
                let panic_on_unload = self.1.config_unloading.load(Ordering::Relaxed);
                let listener: Arc<ConfigListener> = Arc::new(
                    move |host: HostSide<'_>, event: ConfigEvent, _: &ConfigSnapshot| {
                        assert!(matches!(host, HostSide::Server(_)));
                        match event {
                            ConfigEvent::Loading => log.lock().unwrap().push("config-loading"),
                            ConfigEvent::Unloading => {
                                log.lock().unwrap().push("config-unloading");
                                if panic_on_unload {
                                    panic!("config unloading listener");
                                }
                            }
                            _ => {}
                        }
                        Ok(())
                    },
                );
                r.register_config(ConfigSpec::new(ConfigKind::Server, ""), Some(listener))
                    .unwrap();
            }
        }
        fn server_starting(&self, _: &mut dyn ServerHost, _: &mut mod_api::ServerStartingEvent) {
            self.0.lock().unwrap().push("starting");
        }
        fn server_stopping(&self, _: &mut dyn ServerHost, _: &mut mod_api::ServerStoppingEvent) {
            self.0.lock().unwrap().push("stopping");
            if self.1.stopping.load(Ordering::Relaxed) {
                panic!("stopping listener");
            }
        }
        fn world_saved(&self, _: &mut dyn ServerHost, _: &mut mod_api::WorldSavedEvent) {
            self.0.lock().unwrap().push("saved");
            if self.1.save.load(Ordering::Relaxed) {
                panic!("save listener");
            }
        }
        fn player_logged_out(
            &self,
            _: &mut dyn ServerHost,
            _: &mut mod_api::PlayerLoggedOutEvent<'_>,
        ) {
            if self.1.logout.load(Ordering::Relaxed) {
                self.0.lock().unwrap().push("logout");
                panic!("logout listener");
            }
        }
        fn server_stopped(&self, _: &mut dyn ServerHost, _: &mut mod_api::ServerStoppedEvent) {
            if let Some(probe) = self.2.lock().unwrap().as_ref() {
                assert!((probe.serving_stopped)(), "serving retired before Stopped");
                assert!(probe.lan.upgrade().is_none(), "LAN dropped before Stopped");
                let saved = sunrise_client::save::Level::read(&probe.root).unwrap();
                assert_eq!(saved.name, "lifecycle final save");
                assert!(
                    probe.save.raw_chunk(0, 0).is_some(),
                    "dirty chunk saved before Stopped"
                );
                assert_eq!(self.0.lock().unwrap().last(), Some(&"saved"));
                // Storage is released before the notification, even if the
                // final-save listener itself panicked after writing metadata.
                let _lock = sunrise_client::save::SessionLock::acquire(&probe.root).unwrap();
            }
            self.0.lock().unwrap().push("stopped");
        }
    }
    let log = Arc::new(Mutex::new(Vec::new()));
    let panic_at = Arc::new(PanicAt {
        stopping: AtomicBool::new(false),
        save: AtomicBool::new(false),
        config_unloading: AtomicBool::new(false),
        config_registered: AtomicBool::new(false),
        logout: AtomicBool::new(false),
    });
    let stopped_probe = Arc::new(Mutex::new(None));
    for dedicated in [false, true] {
        for case in [
            "normal",
            "startup-error",
            "tick-error",
            "panic",
            "save-error",
            "unopened",
            "locked-unadopted",
            "stopping-panic",
            "save-panic",
            "config-unloading-panic",
            "config-unloading-save-panic",
            "config-unloading-logout-panic",
            "before-start",
            "dedicated-prepare-error",
        ] {
            let prepare_error = case == "dedicated-prepare-error";
            if prepare_error && !dedicated {
                continue;
            }
            log.lock().unwrap().clear();
            panic_at
                .stopping
                .store(case == "stopping-panic", Ordering::Relaxed);
            panic_at.save.store(
                matches!(case, "save-panic" | "config-unloading-save-panic"),
                Ordering::Relaxed,
            );
            panic_at
                .logout
                .store(case == "config-unloading-logout-panic", Ordering::Relaxed);
            let config_panic = case.starts_with("config-unloading");
            panic_at
                .config_unloading
                .store(config_panic, Ordering::Relaxed);
            panic_at
                .config_registered
                .store(config_panic || prepare_error, Ordering::Relaxed);
            *stopped_probe.lock().unwrap() = None;
            let mods: Mods = vec![ModEntry::new(
                "log",
                Box::new(Log(log.clone(), panic_at.clone(), stopped_probe.clone())),
            )]
            .into();
            with_mods(mods.clone(), || {
                mod_platform::__private::part_tests::install_parts_fixture(&mods).unwrap();
                let _ = mod_api::__private::game::take_callback_panic_notices();
                let run_case = |game: &mut crate::game::Game<'_>| {
                    game.options.execution_role = if dedicated {
                        ExecutionRole::DedicatedServer
                    } else {
                        ExecutionRole::IntegratedServer
                    };
                    let root = std::env::temp_dir().join(format!(
                        "server-lifecycle-{}-{dedicated}-{case}",
                        std::process::id()
                    ));
                    std::fs::create_dir_all(&root).unwrap();
                    game.world_root = root.clone();
                    game.save = Arc::new(sunrise_client::save::SaveDir::new(
                        root.clone(),
                        sunrise_client::save::LevelShape::OVERWORLD,
                        Some(Default::default()),
                    ));
                    game.level.name = "lifecycle final save".into();
                    let adopted = !prepare_error
                        && !matches!(case, "unopened" | "before-start" | "locked-unadopted");
                    if adopted || case == "locked-unadopted" {
                        game.session_lock =
                            Some(sunrise_client::save::SessionLock::acquire(&root).unwrap());
                    }
                    // The lock is held while tags are readable, before the prepared
                    // level replaces the placeholder. Only adoption may save.
                    game.world_adopted = adopted;
                    if prepare_error || matches!(case, "unopened" | "locked-unadopted") {
                        // A failed open must not overwrite an existing, unreadable save.
                        std::fs::write(root.join("level.dat"), b"unadopted save").unwrap();
                    }
                    // Give this case's world an override so the mod never writes config into
                    // the process's game directory. Keep the file readable even on failed open.
                    if config_panic || prepare_error {
                        std::fs::create_dir_all(root.join("serverconfig")).unwrap();
                        std::fs::write(
                            root.join("serverconfig/log-server.toml"),
                            "fixture = true\n",
                        )
                        .unwrap();
                    }
                    if config_panic {
                        game.level.name = "previous save".into();
                        game.level.write(&root, &Default::default()).unwrap();
                        // Nothing queues these metadata/block changes. A flush
                        // without the final save must leave the old name and no chunk.
                        game.level.name = "lifecycle final save".into();
                        game.world
                            .chunks
                            .insert((0, 0), Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)));
                        let stone = game
                            .pack
                            .assets
                            .registry
                            .default_state("minecraft:stone")
                            .unwrap();
                        game.world.set(1, 64, 1, stone).unwrap();
                        assert!(game.save.raw_chunk(0, 0).is_none());
                        let serving_stopped = if dedicated {
                            Box::new(|| true) as Box<dyn Fn() -> bool + Send + Sync>
                        } else {
                            // Cover both a published and an unpublished integrated
                            // server. Neither may merely unpublish on shutdown.
                            Box::new(super::super::integrated::tests::install_serving(
                                game,
                                case == "config-unloading-save-panic",
                            ))
                        };
                        let lan = Arc::downgrade(&game.lan.as_ref().unwrap().hub.lan.shared);
                        *stopped_probe.lock().unwrap() = Some(StoppedProbe {
                            serving_stopped,
                            lan,
                            root: root.clone(),
                            save: game.save.clone(),
                        });
                    }
                    let result = game.run_server_lifecycle(dedicated, |game, began| {
                        if case == "before-start" {
                            return Err("before startup".into());
                        }
                        if config_panic || prepare_error {
                            game.native_configs_server_start(&root, dedicated);
                        }
                        if prepare_error {
                            let request = crate::game::world_loading::WorldRequest::new(
                                root.to_str().unwrap(),
                                None,
                                Some(0),
                                false,
                                None,
                                Arc::new(crate::game::api_values::game_rule_registry()),
                            )?;
                            game.prepare_world_and_rules(request)?;
                        }
                        *began = true;
                        game.native_server_about_to_start(dedicated);
                        game.native_server_starting(dedicated);
                        game.native_lifecycle_open_started();
                        if matches!(case, "startup-error" | "unopened" | "locked-unadopted") {
                            return Err("failed init".into());
                        }
                        game.native_lifecycle_world_ready();
                        match case {
                            "tick-error" => Err("failed tick".into()),
                            "panic" => panic!("failed tick"),
                            "save-error" => {
                                let file = root.join("not-a-directory");
                                std::fs::write(&file, b"occupied").unwrap();
                                game.world_root = file;
                                Ok(())
                            }
                            _ => Ok(()),
                        }
                    });
                    // A contained config unloading panic must not become the shutdown error.
                    assert_eq!(
                        result.is_ok(),
                        matches!(case, "normal" | "config-unloading-panic"),
                        "{dedicated} {case}"
                    );
                    if prepare_error {
                        let message = result.as_ref().unwrap_err().to_string();
                        assert!(
                            message.contains("its level data does not read"),
                            "{message}"
                        );
                    }
                    if matches!(
                        case,
                        "stopping-panic"
                            | "save-panic"
                            | "config-unloading-save-panic"
                            | "config-unloading-logout-panic"
                    ) {
                        let message = result.unwrap_err().to_string();
                        let needle = match case {
                            "stopping-panic" => "stopping listener",
                            "save-panic" | "config-unloading-save-panic" => "save listener",
                            "config-unloading-logout-panic" => "logout listener",
                            _ => unreachable!(),
                        };
                        assert!(message.contains(needle), "{dedicated} {case}: {message}");
                    }
                    let notices = mod_api::__private::game::take_callback_panic_notices();
                    if config_panic {
                        assert!(
                            notices.iter().any(|notice| {
                                notice.mod_id == "log"
                                    && notice.operation == "config_unloading"
                                    && notice.message.contains("config unloading listener")
                            }),
                            "{dedicated} {case}: missing contained config panic notice: {notices:?}"
                        );
                    } else {
                        assert!(notices.is_empty(), "{dedicated} {case}: {notices:?}");
                    }
                    let expected: &[&str] = match case {
                        "before-start" => &[],
                        "dedicated-prepare-error" => &["config-loading", "config-unloading"],
                        "unopened" | "save-error" | "locked-unadopted" => {
                            &["starting", "stopping", "stopped"]
                        }
                        "config-unloading-panic" | "config-unloading-save-panic" => &[
                            "config-loading",
                            "starting",
                            "stopping",
                            "saved",
                            "stopped",
                            "config-unloading",
                        ],
                        "config-unloading-logout-panic" => &[
                            "config-loading",
                            "starting",
                            "stopping",
                            "logout",
                            "saved",
                            "stopped",
                            "config-unloading",
                        ],
                        _ => &["starting", "stopping", "saved", "stopped"],
                    };
                    assert_eq!(*log.lock().unwrap(), expected, "{dedicated} {case}");
                    assert!(game.session_lock.is_none(), "storage released after {case}");
                    assert!(!game.world_adopted, "adoption cleared after {case}");
                    assert!(game.serving.is_none(), "serving retired after {case}");
                    assert!(game.lan.is_none(), "LAN dropped after {case}");
                    if config_panic {
                        let registry = &game.pack.assets.registry;
                        let chunk = game.save.load(0, 0, registry, world_biomes!(game)).unwrap();
                        assert_eq!(
                            chunk.block(1, 64, 1),
                            registry.default_state("minecraft:stone").unwrap()
                        );
                    }
                    if prepare_error || matches!(case, "unopened" | "locked-unadopted") {
                        assert_eq!(
                            std::fs::read(root.join("level.dat")).unwrap(),
                            b"unadopted save",
                            "{dedicated} {case}"
                        );
                    } else if !matches!(case, "save-error" | "before-start") {
                        let saved = sunrise_client::save::Level::read(&root).unwrap();
                        assert_eq!(saved.name, "lifecycle final save");
                    }
                    std::fs::remove_dir_all(root).unwrap();
                };
                if config_panic {
                    with_guests(run_case);
                } else {
                    with_game(run_case);
                }
            });
        }
    }
}
