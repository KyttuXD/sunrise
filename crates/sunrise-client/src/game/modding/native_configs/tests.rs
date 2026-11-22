use super::*;
use mod_api::ConfigKind::{Client, Common, Server};
use mod_api::{
    ConfigListener, ConfigSpec, Hooks, HostSide, ModEntry, Mods, Registrar, RegistrationContext,
};
use std::sync::{Arc, Mutex};

struct CleanupDir(PathBuf);

impl Drop for CleanupDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) type ConfigDecl = (ConfigKind, Option<&'static str>, &'static str);

pub(super) fn with_registered_configs(
    entries: &[(&'static str, &[ConfigDecl])],
    f: impl FnOnce(&'static [RegisteredConfig]),
) {
    struct Decl(Vec<ConfigDecl>);
    #[mod_api::__subscriptions]
    impl Hooks for Decl {
        fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
            for &(kind, file, default) in &self.0 {
                let mut spec = ConfigSpec::new(kind, default);
                if let Some(file) = file {
                    spec = spec.file(file);
                }
                r.register_config(spec, None).unwrap();
            }
        }
    }
    let mods: Mods = entries
        .iter()
        .map(|&(id, specs)| ModEntry::new(id, Box::new(Decl(specs.to_vec()))))
        .collect::<Vec<_>>()
        .into();
    mod_platform::testing::with_mods(mods.clone(), || {
        mod_platform::__private::part_tests::install_parts_fixture(&mods).unwrap();
        f(registered_configs());
    });
}

#[test]
fn engine_test_host_operation_reads_the_real_phase_provider() {
    use mod_api::{
        __private::{ModPhase, PhaseOwner},
        Host,
        prelude::EntitiesHost,
    };
    Game::initialize_mod_runtime_for_tests();
    struct EmptyHost;
    impl Host for EmptyHost {}
    mod_api::impl_host_areas!(EmptyHost);
    let _phase = PhaseOwner::scope(ModPhase::Setup);
    assert_eq!(mod_api::__private::current_phase(), ModPhase::Setup);
    let refused = std::panic::catch_unwind(|| EmptyHost.entity(1)).unwrap_err();
    let message = refused.downcast_ref::<String>().unwrap();
    assert!(message.contains("Setup"), "{message}");
}

#[test]
#[ignore = "daily"]
fn config_permissions_and_once_per_side_survive_two_worlds() {
    use mod_api::{Host, HostTransaction};
    struct Probe(Arc<Mutex<Vec<(bool, ConfigKind)>>>);
    #[mod_api::__subscriptions]
    impl Hooks for Probe {
        fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
            for kind in [Client, Common, Server] {
                let calls = self.0.clone();
                let listener: Arc<ConfigListener> = Arc::new(
                    move |mut host: HostSide<'_>, event: ConfigEvent, config: &ConfigSnapshot| {
                        match event {
                            ConfigEvent::Loading => {
                                assert_eq!(
                                    mod_api::__private::current_phase(),
                                    mod_api::__private::ModPhase::Config
                                );
                                let read: &dyn Host = match &host {
                                    HostSide::Server(host) => &**host,
                                    HostSide::Client(host) => &**host,
                                };
                                let _ = read.setting("phase_probe", "unused");
                                assert_eq!(
                                    read.config_dir(),
                                    Game::game_dir().join("config").as_path()
                                );
                                let refused =
                                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                        read.entity(1)
                                    }))
                                    .unwrap_err();
                                assert_eq!(
                                    refused
                                        .downcast_ref::<mod_api::__private::PhaseRefusal>()
                                        .map(|refusal| refusal.phase),
                                    Some(mod_api::__private::ModPhase::Config)
                                );
                                if let HostSide::Server(host) = &mut host {
                                    let refused = std::panic::catch_unwind(
                                        std::panic::AssertUnwindSafe(|| {
                                            let _: Result<(), ()> = host.transaction(|_, _| Ok(()));
                                        }),
                                    )
                                    .unwrap_err();
                                    assert_eq!(
                                        refused
                                            .downcast_ref::<mod_api::__private::PhaseRefusal>()
                                            .map(|refusal| refusal.phase),
                                        Some(mod_api::__private::ModPhase::Config)
                                    );
                                }
                                calls.lock().unwrap().push((true, config.kind));
                            }
                            ConfigEvent::Unloading => {
                                assert_eq!(
                                    mod_api::__private::current_phase(),
                                    mod_api::__private::ModPhase::World
                                );
                                assert!(matches!(host, HostSide::Server(_)));
                                calls.lock().unwrap().push((false, config.kind));
                            }
                            _ => {}
                        }
                        Ok(())
                    },
                );
                r.register_config(ConfigSpec::new(kind, "enabled=true"), Some(listener))
                    .unwrap();
            }
        }
    }
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mods: Mods = vec![ModEntry::new("phase_probe", Box::new(Probe(calls.clone())))].into();
    mod_platform::testing::with_mods(mods.clone(), || {
        mod_platform::__private::part_tests::install_parts_fixture(&mods).unwrap();
        crate::game::native_entity_refs::tests::with_client_game(|game| {
            game.mod_phase =
                mod_api::__private::PhaseOwner::new(mod_api::__private::ModPhase::Setup);
            game.native_configs_client_start();
            game.native_configs_client_start();
            game.options.execution_role =
                crate::game::execution_role::ExecutionRole::IntegratedServer;
            for world in ["phase-world-one", "phase-world-two"] {
                game.native_configs_server_start(Path::new(world), false);
                game.native_configs_server_stop();
            }
        });
    });
    assert_eq!(
        *calls.lock().unwrap(),
        [
            (true, Client),
            (true, Common),
            (true, Server),
            (false, Server),
            (true, Server),
            (false, Server),
        ]
    );
}

#[test]
#[ignore = "daily"]
fn owner_bound_config_lifecycle_isolates_owners_refusals_and_panics() {
    type TraceRow = (&'static str, ConfigEvent, PathBuf, String);

    let tag = format!("p3-25-lifecycle-{}", std::process::id());
    let alpha_first_rel = PathBuf::from(&tag).join("alpha-first.toml");
    let alpha_second_rel = PathBuf::from(&tag).join("alpha-second.toml");
    let beta_rel = PathBuf::from(&tag).join("beta.toml");

    let world_one = Folder::new("daily-w1");
    let world_two = Folder::new("daily-w2");
    let config_dir = Game::game_dir().join("config");
    let _cleanup = CleanupDir(config_dir.join(&tag));

    let override_path = world_one.0.join("serverconfig").join(&alpha_second_rel);
    fs::create_dir_all(override_path.parent().unwrap()).unwrap();
    fs::write(&override_path, "second = override\n").unwrap();

    struct Alpha {
        first: PathBuf,
        second: PathBuf,
        trace: Arc<Mutex<Vec<TraceRow>>>,
    }
    #[mod_api::__subscriptions]
    impl Hooks for Alpha {
        fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
            let trace = self.trace.clone();
            let first_listener: Arc<ConfigListener> = Arc::new(
                move |host: HostSide<'_>, event: ConfigEvent, snapshot: &ConfigSnapshot| {
                    assert!(matches!(host, HostSide::Server(_)));
                    assert_eq!(mod_api::content::crash::current_native_mod(), Some("alpha"));
                    assert_eq!(snapshot.mod_id, "alpha");
                    trace.lock().unwrap().push((
                        "alpha-first",
                        event,
                        snapshot.path.clone(),
                        snapshot.text.clone(),
                    ));
                    if event == ConfigEvent::Reloading {
                        return Err("refused reload".into());
                    }
                    Ok(())
                },
            );
            r.register_config(
                ConfigSpec::new(Server, "alpha = 1\n").file(self.first.clone()),
                Some(first_listener),
            )
            .unwrap();

            let trace = self.trace.clone();
            let second_listener: Arc<ConfigListener> = Arc::new(
                move |host: HostSide<'_>, event: ConfigEvent, snapshot: &ConfigSnapshot| {
                    assert!(matches!(host, HostSide::Server(_)));
                    assert_eq!(mod_api::content::crash::current_native_mod(), Some("alpha"));
                    assert_eq!(snapshot.mod_id, "alpha");
                    trace.lock().unwrap().push((
                        "alpha-second",
                        event,
                        snapshot.path.clone(),
                        snapshot.text.clone(),
                    ));
                    Ok(())
                },
            );
            r.register_config(
                ConfigSpec::new(Server, "second = 1\n").file(self.second.clone()),
                Some(second_listener),
            )
            .unwrap();
        }
    }

    struct Beta {
        file: PathBuf,
        trace: Arc<Mutex<Vec<TraceRow>>>,
    }
    #[mod_api::__subscriptions]
    impl Hooks for Beta {
        fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
            let trace = self.trace.clone();
            let listener: Arc<ConfigListener> = Arc::new(
                move |host: HostSide<'_>, event: ConfigEvent, snapshot: &ConfigSnapshot| {
                    assert!(matches!(host, HostSide::Server(_)));
                    assert_eq!(mod_api::content::crash::current_native_mod(), Some("beta"));
                    assert_eq!(snapshot.mod_id, "beta");
                    trace.lock().unwrap().push((
                        "beta",
                        event,
                        snapshot.path.clone(),
                        snapshot.text.clone(),
                    ));
                    if event == ConfigEvent::Reloading {
                        panic!("beta reload boom");
                    }
                    Ok(())
                },
            );
            r.register_config(
                ConfigSpec::new(Server, "beta = 1\n").file(self.file.clone()),
                Some(listener),
            )
            .unwrap();
        }
    }

    let trace = Arc::new(Mutex::new(Vec::new()));
    let mods: Mods = vec![
        ModEntry::new(
            "alpha",
            Box::new(Alpha {
                first: alpha_first_rel.clone(),
                second: alpha_second_rel.clone(),
                trace: trace.clone(),
            }),
        ),
        ModEntry::new(
            "beta",
            Box::new(Beta {
                file: beta_rel.clone(),
                trace: trace.clone(),
            }),
        ),
    ]
    .into();

    let alpha_first_path = config_dir.join(&alpha_first_rel);
    let alpha_second_base = config_dir.join(&alpha_second_rel);
    let beta_path = config_dir.join(&beta_rel);

    mod_platform::testing::with_mods(mods.clone(), || {
        mod_platform::__private::part_tests::install_parts_fixture(&mods).unwrap();
        let _ = mod_api::__private::game::take_callback_panic_notices();
        crate::game::native_entity_refs::tests::with_client_game(|game| {
            game.mod_phase =
                mod_api::__private::PhaseOwner::new(mod_api::__private::ModPhase::Setup);
            game.options.execution_role =
                crate::game::execution_role::ExecutionRole::IntegratedServer;

            // World 1: loads in declaration order; world override wins without writing a base file.
            game.native_configs_server_start(&world_one.0, false);
            assert!(!alpha_second_base.exists());
            assert_eq!(
                std::mem::take(&mut *trace.lock().unwrap()),
                [
                    (
                        "alpha-first",
                        ConfigEvent::Loading,
                        alpha_first_path.clone(),
                        "alpha = 1\n".into(),
                    ),
                    (
                        "alpha-second",
                        ConfigEvent::Loading,
                        override_path.clone(),
                        "second = override\n".into(),
                    ),
                    (
                        "beta",
                        ConfigEvent::Loading,
                        beta_path.clone(),
                        "beta = 1\n".into(),
                    ),
                ]
            );

            // Consecutive edits whose listener returns Err must keep delivering only to that owner.
            for text in ["alpha = 2\n", "alpha = 3\n"] {
                fs::write(&alpha_first_path, text).unwrap();
                game.native_configs.next_check = None;
                game.native_configs_poll();
                assert_eq!(
                    std::mem::take(&mut *trace.lock().unwrap()),
                    [(
                        "alpha-first",
                        ConfigEvent::Reloading,
                        alpha_first_path.clone(),
                        text.into(),
                    )]
                );
            }
            assert!(mod_api::__private::game::take_callback_panic_notices().is_empty());

            // Editing the world override notifies only its owning registration with the override path.
            fs::write(&override_path, "second = override2\n").unwrap();
            game.native_configs.next_check = None;
            game.native_configs_poll();
            assert_eq!(
                std::mem::take(&mut *trace.lock().unwrap()),
                [(
                    "alpha-second",
                    ConfigEvent::Reloading,
                    override_path.clone(),
                    "second = override2\n".into(),
                )]
            );

            // A real panic records a notice naming that mod and disables only that registration.
            fs::write(&beta_path, "beta = 2\n").unwrap();
            game.native_configs.next_check = None;
            game.native_configs_poll();
            assert_eq!(
                std::mem::take(&mut *trace.lock().unwrap()),
                [(
                    "beta",
                    ConfigEvent::Reloading,
                    beta_path.clone(),
                    "beta = 2\n".into(),
                )]
            );
            let notices = mod_api::__private::game::take_callback_panic_notices();
            assert!(
                notices.iter().any(|notice| {
                    notice.mod_id == "beta"
                        && notice.operation == "config_reloading"
                        && notice.message.contains("beta reload boom")
                }),
                "missing panic notice for beta: {notices:?}"
            );

            // Further edits still reach alpha-first while poisoned beta stays silent.
            fs::write(&beta_path, "beta = 3\n").unwrap();
            fs::write(&alpha_first_path, "alpha = 4\n").unwrap();
            game.native_configs.next_check = None;
            game.native_configs_poll();
            assert_eq!(
                std::mem::take(&mut *trace.lock().unwrap()),
                [(
                    "alpha-first",
                    ConfigEvent::Reloading,
                    alpha_first_path.clone(),
                    "alpha = 4\n".into(),
                )]
            );
            assert!(mod_api::__private::game::take_callback_panic_notices().is_empty());

            // Unload reaches only live registrations in order.
            game.native_configs_server_stop();
            assert_eq!(
                std::mem::take(&mut *trace.lock().unwrap()),
                [
                    (
                        "alpha-first",
                        ConfigEvent::Unloading,
                        alpha_first_path.clone(),
                        "alpha = 4\n".into(),
                    ),
                    (
                        "alpha-second",
                        ConfigEvent::Unloading,
                        override_path.clone(),
                        "second = override2\n".into(),
                    ),
                ]
            );

            // World 2 has no override: alpha-second creates its base default, while poisoned beta
            // remains disabled across worlds.
            game.native_configs_server_start(&world_two.0, false);
            assert_eq!(
                std::mem::take(&mut *trace.lock().unwrap()),
                [
                    (
                        "alpha-first",
                        ConfigEvent::Loading,
                        alpha_first_path.clone(),
                        "alpha = 4\n".into(),
                    ),
                    (
                        "alpha-second",
                        ConfigEvent::Loading,
                        alpha_second_base.clone(),
                        "second = 1\n".into(),
                    ),
                ]
            );
            game.native_configs_server_stop();
            assert_eq!(
                std::mem::take(&mut *trace.lock().unwrap()),
                [
                    (
                        "alpha-first",
                        ConfigEvent::Unloading,
                        alpha_first_path.clone(),
                        "alpha = 4\n".into(),
                    ),
                    (
                        "alpha-second",
                        ConfigEvent::Unloading,
                        alpha_second_base.clone(),
                        "second = 1\n".into(),
                    ),
                ]
            );
        });
    });
}

struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "sunrise-native-configs-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_missing_file_gets_its_default_and_an_edited_one_is_read() {
    let root = Folder::new("default");
    let config = root.0.join("config");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("gt-common.toml"), "voltage = 512\n").unwrap();
    with_registered_configs(
        &[(
            "gt",
            &[
                (Client, None, "hud = true\n"),
                (Common, None, "voltage = 32\n"),
                (Server, None, "ores = 1\n"),
            ],
        )],
        |registered| {
            let mut configs = NativeConfigs::default();
            let loaded = configs.load(registered, &[Client, Common], &config, None);
            let got: Vec<_> = loaded
                .iter()
                .map(|(_, snapshot)| {
                    (
                        snapshot.file.as_os_str(),
                        snapshot.path.as_path(),
                        snapshot.text.as_str(),
                    )
                })
                .collect();
            assert_eq!(
                got,
                [
                    (
                        Path::new("gt-client.toml").as_os_str(),
                        config.join("gt-client.toml").as_path(),
                        "hud = true\n",
                    ),
                    (
                        Path::new("gt-common.toml").as_os_str(),
                        config.join("gt-common.toml").as_path(),
                        "voltage = 512\n",
                    ),
                ]
            );
            assert_eq!(
                fs::read_to_string(config.join("gt-client.toml")).unwrap(),
                "hud = true\n"
            );
            // Another side's kind is neither read nor written.
            assert!(!config.join("gt-server.toml").exists());
        },
    );
}

#[test]
fn a_world_overrides_a_server_config_and_the_folder_serves_the_rest() {
    let root = Folder::new("world");
    let (config, world) = (root.0.join("config"), root.0.join("saves/W"));
    fs::create_dir_all(world.join("serverconfig/gt")).unwrap();
    fs::write(world.join("serverconfig/gt/ores.toml"), "veins = 9\n").unwrap();
    with_registered_configs(
        &[(
            "gt",
            &[
                (Server, Some("gt/ores.toml"), "veins = 3\n"),
                (Server, None, "tiers = 4\n"),
                // A Common config of the world's folder is not looked for.
                (Common, None, "a = 1\n"),
            ],
        )],
        |registered| {
            let mut configs = NativeConfigs::default();
            let loaded = configs.load(registered, &[Server], &config, Some(&world));
            let got: Vec<_> = configs
                .files
                .iter()
                .map(|t| (t.snapshot.path.clone(), t.snapshot.text.as_str()))
                .collect();
            assert_eq!(loaded.len(), 2);
            assert_eq!(
                got,
                [
                    (world.join("serverconfig/gt/ores.toml"), "veins = 9\n"),
                    (config.join("gt-server.toml"), "tiers = 4\n"),
                ]
            );
            // The world's copy stands for the default: nothing is written beside it.
            assert!(!config.join("gt/ores.toml").exists());
            assert_eq!(configs.drop_server().len(), 2);
            assert!(configs.files.is_empty());
        },
    );
}

#[cfg(unix)]
#[test]
fn a_server_config_override_symlink_only_wins_when_its_target_exists() {
    let root = Folder::new("override-link");
    let config = root.0.join("config");
    let world = root.0.join("world");
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(world.join("serverconfig")).unwrap();

    let base = config.join("gt-server.toml");
    let target = root.0.join("override.toml");
    let override_path = world.join("serverconfig/gt-server.toml");
    fs::write(&base, "value = 7\n").unwrap();
    std::os::unix::fs::symlink(&target, &override_path).unwrap();

    with_registered_configs(&[("gt", &[(Server, None, "value = 0\n")])], |registered| {
        let mut configs = NativeConfigs::default();
        let loaded = configs.load(registered, &[Server], &config, Some(&world));
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].1.path, base);
        assert_eq!(loaded[0].1.text, "value = 7\n");

        configs.drop_server();
        fs::write(&target, "value = 9\n").unwrap();
        let loaded = configs.load(registered, &[Server], &config, Some(&world));
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].1.path, override_path);
        assert_eq!(loaded[0].1.text, "value = 9\n");

        fs::remove_file(&override_path).unwrap();
        std::os::unix::fs::symlink(&override_path, &override_path).unwrap();
        let (path, error) = locate(&registered[0], &config, Some(&world)).unwrap_err();
        assert_eq!(path, override_path);
        assert_ne!(error.kind(), std::io::ErrorKind::NotFound);
    });
}

#[test]
fn registered_configs_load_in_declaration_order_and_keep_io_failures_isolated() {
    let root = Folder::new("order");
    let config = root.0.join("config");
    let broken_world = root.0.join("saves/broken");
    let clean_world = root.0.join("saves/clean");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("z-server.toml"), "fallback = true\n").unwrap();
    // An unreadable world override path (here a directory) must fail on that path,
    // never silently substitute config/z-server.toml or block other configs.
    fs::create_dir_all(broken_world.join("serverconfig/z-server.toml")).unwrap();

    // Resolved dependency order ["lib", "z", "a"] with multiple files under one owner
    // and a native path not prefixed with the mod's id.
    with_registered_configs(
        &[
            (
                "lib",
                &[
                    (Server, Some("shared/first.toml"), "first = 1\n"),
                    (Server, None, "lib = 2\n"),
                ],
            ),
            ("z", &[(Server, None, "z = 3\n")]),
            ("a", &[(Server, None, "a = 4\n")]),
        ],
        |registered| {
            let mut configs = NativeConfigs::default();
            let loaded = configs.load(registered, &[Server], &config, Some(&broken_world));
            assert_eq!(
                loaded
                    .iter()
                    .map(|(_, s)| (s.mod_id, s.file.as_os_str(), s.text.as_str()))
                    .collect::<Vec<_>>(),
                [
                    (
                        "lib",
                        Path::new("shared/first.toml").as_os_str(),
                        "first = 1\n",
                    ),
                    ("lib", Path::new("lib-server.toml").as_os_str(), "lib = 2\n"),
                    ("a", Path::new("a-server.toml").as_os_str(), "a = 4\n"),
                ]
            );
            assert_eq!(configs.drop_server().len(), 3);

            // A failed load is not latched: a later world without the broken override loads all.
            let loaded = configs.load(registered, &[Server], &config, Some(&clean_world));
            assert_eq!(
                loaded
                    .iter()
                    .map(|(_, s)| (s.mod_id, s.text.as_str()))
                    .collect::<Vec<_>>(),
                [
                    ("lib", "first = 1\n"),
                    ("lib", "lib = 2\n"),
                    ("z", "fallback = true\n"),
                    ("a", "a = 4\n"),
                ]
            );
        },
    );
}

#[test]
fn a_change_reloads_once_and_a_touch_without_one_does_not() {
    let root = Folder::new("reload");
    let config = root.0.join("config");
    with_registered_configs(&[("gt", &[(Common, None, "a = 1\n")])], |registered| {
        let mut configs = NativeConfigs::default();
        configs.load(registered, &[Common], &config, None);
        let path = config.join("gt-common.toml");
        assert!(configs.changed().is_empty());
        fs::write(&path, "a = 2\n").unwrap();
        let changed = configs.changed();
        assert_eq!(
            changed
                .iter()
                .map(|(_, s)| s.text.as_str())
                .collect::<Vec<_>>(),
            ["a = 2\n"]
        );
        assert!(configs.changed().is_empty());
        // Saved again with the same text: nothing to reload.
        fs::write(&path, "a = 2\n").unwrap();
        assert!(configs.changed().is_empty());
        // Same length, written at once after the load, as a mod's own save
        // from `ConfigEvent::Loading` is: still a change.
        fs::write(&path, "a = 3\n").unwrap();
        assert_eq!(configs.changed().len(), 1);
        fs::write(&path, "a = 2\n").unwrap();
        configs.changed();
        // Gone: the last snapshot stays, nothing fires, and recovery is not latched.
        fs::remove_file(&path).unwrap();
        assert!(configs.changed().is_empty());
        assert_eq!(configs.files[0].snapshot.text, "a = 2\n");
        fs::write(&path, "a = 4\n").unwrap();
        let recovered = configs.changed();
        assert_eq!(
            recovered
                .iter()
                .map(|(_, s)| s.text.as_str())
                .collect::<Vec<_>>(),
            ["a = 4\n"]
        );
    });
}

#[test]
fn files_are_looked_at_once_a_second_and_never_without_configs() {
    with_registered_configs(&[("gt", &[(Common, None, "")])], |registered| {
        let mut configs = NativeConfigs::default();
        let start = Instant::now();
        assert!(!configs.due(start));
        let config = &registered[0];
        configs.files.push(Tracked {
            config,
            snapshot: config.snapshot(PathBuf::new(), String::new()),
        });
        assert!(configs.due(start));
        assert!(!configs.due(start + Duration::from_millis(999)));
        assert!(configs.due(start + CHECK_EVERY));
    });
}
