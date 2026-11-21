use super::*;
use mod_api::__private::make_setting_key;

#[test]
fn only_registered_keys_write_and_refused_writes_leave_the_value_intact() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("settings");
    let toggle = || SettingSpec::toggle("enabled", "example.enabled", false);
    let key = make_setting_key("example", 0, toggle());
    let lookalike = make_setting_key("example", 0, toggle());
    let mut settings = NativeSettings::load(root.clone(), std::slice::from_ref(&key), |_, _| None);
    assert_eq!(settings.setting(&lookalike), None);
    assert_eq!(
        settings.write(&lookalike, SettingValue::Toggle(true)),
        Err(SettingWriteError::Unavailable)
    );
    assert_eq!(
        settings.write(&key, SettingValue::Choice(0)),
        Err(SettingWriteError::InvalidValue)
    );
    assert!(!root.exists());

    let saved = root.join("example/enabled");
    fs::create_dir_all(&saved).unwrap();
    assert_eq!(
        settings.write(&key, SettingValue::Toggle(true)),
        Err(SettingWriteError::SaveFailed)
    );
    assert_eq!(settings.setting(&key), Some(SettingValue::Toggle(false)));
    fs::remove_dir(&saved).unwrap();
    #[cfg(unix)]
    {
        let other = root.join("other");
        fs::write(&other, "false").unwrap();
        std::os::unix::fs::symlink(&other, &saved).unwrap();
        assert_eq!(
            settings.write(&key, SettingValue::Toggle(true)),
            Err(SettingWriteError::SaveFailed),
        );
        assert_eq!(fs::read_to_string(other).unwrap(), "false");
        fs::remove_file(&saved).unwrap();
    }
    assert_eq!(settings.write(&key, SettingValue::Toggle(true)), Ok(true));
    assert_eq!(settings.setting(&key), Some(SettingValue::Toggle(true)));
    assert_eq!(fs::read_to_string(&saved).unwrap(), "true");
    assert_eq!(settings.write(&key, SettingValue::Toggle(true)), Ok(false));
}

#[test]
fn signed_zero_is_a_distinct_range_value() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("settings");
    let key = make_setting_key(
        "example",
        0,
        SettingSpec::range("offset", "example.offset", -1., 1., 0.25, 0.),
    );
    let mut settings = NativeSettings::load(root.clone(), std::slice::from_ref(&key), |_, _| None);
    assert_eq!(settings.write(&key, SettingValue::Range(-0.0)), Ok(true));
    assert_eq!(
        fs::read_to_string(root.join("example/offset")).unwrap(),
        "-0"
    );
    assert_eq!(settings.refresh(None, |_, _| Some("0")).len(), 1);
    let Some(SettingValue::Range(value)) = settings.setting(&key) else {
        panic!("missing range")
    };
    assert_eq!(value.to_bits(), 0);
}

#[test]
#[ignore = "daily"]
fn mod_page_host_and_reload_share_the_live_values() {
    use mod_api::{
        __private::{ModPhase, PhaseOwner},
        ClientHost, Hooks, ModEntry, SettingChangedEvent,
    };
    use std::sync::{Arc, Mutex};
    use sunrise_client::mod_list::{Installed, NativePage};
    use sunrise_world::native_load_context::{DataLoadOrigin, LoadContext, LoadInputs, LoopSide};

    type Seen = Arc<Mutex<Vec<(&'static str, String, SettingValue)>>>;
    struct Listener {
        owner: &'static str,
        primary: SettingKey,
        nested: Option<SettingKey>,
        seen: Seen,
    }
    #[mod_api::__subscriptions]
    impl Hooks for Listener {
        fn setting_changed(&self, host: &mut dyn ClientHost, event: &mut SettingChangedEvent<'_>) {
            let key = if event.key == "nested" {
                self.nested.as_ref().unwrap()
            } else {
                &self.primary
            };
            let value = host.setting(key).unwrap();
            self.seen
                .lock()
                .unwrap()
                .push((self.owner, event.key.to_owned(), value));
            if event.key == "enabled"
                && value == SettingValue::Toggle(true)
                && let Some(nested) = &self.nested
            {
                host.set_setting(nested, SettingValue::Toggle(true))
                    .unwrap();
            }
        }
    }

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("settings");
    let primary = |owner, index| {
        make_setting_key(
            owner,
            index,
            SettingSpec::toggle("enabled", "example.enabled", false),
        )
    };
    let a = primary("settings_a", 0);
    let b = primary("settings_b", 1);
    let nested = make_setting_key(
        "settings_a",
        2,
        SettingSpec::toggle("nested", "example.nested", false),
    );
    let seen = Seen::default();
    let mods = vec![
        ModEntry::new(
            "settings_a",
            Box::new(Listener {
                owner: "settings_a",
                primary: a.clone(),
                nested: Some(nested.clone()),
                seen: seen.clone(),
            }),
        ),
        ModEntry::new(
            "settings_b",
            Box::new(Listener {
                owner: "settings_b",
                primary: b.clone(),
                nested: None,
                seen: seen.clone(),
            }),
        ),
    ];
    mod_platform::testing::with_mods(mods.into(), || {
        crate::game::native_entity_refs::tests::with_client_game(|game| {
            let _world = PhaseOwner::scope(ModPhase::World);
            game.native_settings = NativeSettings::load(
                root.clone(),
                &[a.clone(), b.clone(), nested.clone()],
                |_, _| None,
            );
            game.settings.mods.installed = ["settings_a", "settings_b"]
                .map(|owner| Installed {
                    id: owner.into(),
                    title: owner.into(),
                    description: String::new(),
                    problem: None,
                    dev: false,
                    native: Some(NativePage {
                        mappings: Vec::new(),
                        settings: game.native_page_settings(owner),
                    }),
                })
                .into();

            let host: &mut dyn ClientHost = game;
            host.set_setting(&a, SettingValue::Toggle(true)).unwrap();
            assert_eq!(host.setting(&nested), Some(SettingValue::Toggle(true)));
            assert_eq!(host.setting(&b), Some(SettingValue::Toggle(false)));
            assert_eq!(
                *seen.lock().unwrap(),
                [("settings_a", "enabled".into(), SettingValue::Toggle(true))]
            );
            assert_eq!(
                game.settings.mods.installed[0]
                    .native
                    .as_ref()
                    .unwrap()
                    .settings[0]
                    .value,
                NativeSettingValue::Toggle(true)
            );
            game.notify_nested_setting_changes();

            game.settings.mods.installed[1]
                .native
                .as_mut()
                .unwrap()
                .settings[0]
                .value = NativeSettingValue::Toggle(true);
            game.native_setting_event(1, 0);
            assert_eq!(game.setting(&b), Some(SettingValue::Toggle(true)));
            assert_eq!(
                fs::read_to_string(root.join("settings_b/enabled")).unwrap(),
                "true"
            );

            let file = root.join("settings_a/enabled");
            fs::write(&file, "false").unwrap();
            let captured = LoadContext::new(
                DataLoadOrigin::ClientPreload,
                Arc::new(LoadInputs::new(Default::default(), root.clone())),
                ["settings_a", "settings_b"],
            );
            fs::write(&file, "true").unwrap();
            game.loop_context = Arc::new(captured.captured_loop_context(LoopSide::Client));
            game.sync_native_settings();
            assert_eq!(game.setting(&a), Some(SettingValue::Toggle(false)));
            game.native_settings_refresh(0);
            assert_eq!(game.setting(&a), Some(SettingValue::Toggle(true)));
            assert_eq!(
                *seen.lock().unwrap(),
                [
                    ("settings_a", "enabled".into(), SettingValue::Toggle(true)),
                    ("settings_a", "nested".into(), SettingValue::Toggle(true)),
                    ("settings_b", "enabled".into(), SettingValue::Toggle(true)),
                    ("settings_a", "enabled".into(), SettingValue::Toggle(false)),
                    ("settings_a", "enabled".into(), SettingValue::Toggle(true)),
                ]
            );
        });
    });
}
