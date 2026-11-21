use mod_api::{
    __private::{game::CallbackOwner, setting_registration},
    SettingKey, SettingValue, SettingWriteError,
};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
use sunrise_client::options_ui::Page;

use super::Game;

/// One client window owns the live values. Load and loop contexts retain their own
/// immutable inputs; opening a page or reloading the client rereads the persisted values.
#[derive(Default)]
pub(in crate::game) struct NativeSettings {
    entries: Vec<Entry>,
    owners: HashMap<&'static str, CallbackOwner>,
    changed: Vec<SettingKey>,
}

struct Entry {
    key: SettingKey,
    path: PathBuf,
    value: SettingValue,
}

impl NativeSettings {
    pub(in crate::game) fn load(root: &Path, keys: &[SettingKey]) -> Self {
        let mut settings = Self::default();
        settings.entries = keys
            .iter()
            .map(|key| {
                let registration = setting_registration(key);
                settings
                    .owners
                    .entry(registration.owner)
                    .or_insert_with(|| CallbackOwner::for_mod(registration.owner));
                Entry {
                    key: key.clone(),
                    path: root.join(registration.owner).join(&registration.spec.key),
                    value: registration.spec.default_value(),
                }
            })
            .collect();
        settings.reload(None);
        // The initial load establishes the values; it is not a change notification.
        settings.changed.clear();
        settings
    }

    pub(super) fn reload(&mut self, owner: Option<&str>) {
        for entry in &mut self.entries {
            let registration = setting_registration(&entry.key);
            if owner.is_some_and(|owner| registration.owner != owner) {
                continue;
            }
            let raw = match fs::read_to_string(&entry.path) {
                Ok(raw) => Some(raw),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
                Err(err) => {
                    log::warn!(
                        "mod {} setting {}: could not load {}: {err}",
                        registration.owner,
                        registration.spec.key,
                        entry.path.display()
                    );
                    continue;
                }
            };
            let value = registration.spec.read(raw.as_deref());
            if value != entry.value {
                entry.value = value;
                self.changed.push(entry.key.clone());
            }
        }
    }

    pub(super) fn owner_settings(
        &self,
        owner: &str,
    ) -> impl Iterator<Item = (&mod_api::SettingSpec, SettingValue)> {
        self.entries.iter().filter_map(move |entry| {
            let registration = setting_registration(&entry.key);
            (registration.owner == owner).then_some((&registration.spec, entry.value))
        })
    }

    pub(super) fn key_for(&self, owner: &str, key: &str) -> Option<SettingKey> {
        self.entries.iter().find_map(|entry| {
            let registration = setting_registration(&entry.key);
            (registration.owner == owner && registration.spec.key == key).then(|| entry.key.clone())
        })
    }

    fn setting(&self, key: &SettingKey) -> Option<SettingValue> {
        let registration = setting_registration(key);
        self.entries
            .get(registration.index)
            .filter(|entry| std::ptr::eq(setting_registration(&entry.key), registration))
            .map(|entry| entry.value)
    }

    pub(super) fn set_setting(
        &mut self,
        key: &SettingKey,
        value: SettingValue,
    ) -> Result<(), SettingWriteError> {
        let registration = setting_registration(key);
        let Some(entry) = self
            .entries
            .get_mut(registration.index)
            .filter(|entry| std::ptr::eq(setting_registration(&entry.key), registration))
        else {
            return Err(SettingWriteError::Unavailable);
        };
        let Some(text) = registration.spec.store(value) else {
            log::warn!(
                "mod {} setting {}: invalid value {value:?}",
                registration.owner,
                registration.spec.key
            );
            return Err(SettingWriteError::InvalidValue);
        };
        if let Err(err) =
            fs::create_dir_all(entry.path.parent().expect("a setting has a mod directory"))
                .and_then(|()| fs::write(&entry.path, text))
        {
            log::warn!(
                "mod {} setting {}: could not save {}: {err}",
                registration.owner,
                registration.spec.key,
                entry.path.display()
            );
            return Err(SettingWriteError::SaveFailed);
        }
        if entry.value != value {
            entry.value = value;
            self.changed.push(key.clone());
        }
        Ok(())
    }
}

impl Game<'_> {
    pub(in crate::game) fn setting(&self, key: &SettingKey) -> Option<SettingValue> {
        self.native_settings.setting(key)
    }

    pub(in crate::game) fn set_setting(
        &mut self,
        key: &SettingKey,
        value: SettingValue,
    ) -> Result<(), SettingWriteError> {
        self.native_settings.set_setting(key, value)?;
        if let Page::NativeMod(i) = self.ui.options.page()
            && self
                .settings
                .mods
                .installed
                .get(i)
                .is_some_and(|row| row.id == setting_registration(key).owner)
        {
            self.sync_native_settings_page(i);
        }
        Ok(())
    }

    pub(in crate::game) fn reload_native_settings(&mut self) {
        self.native_settings.reload(None);
        if let Page::NativeMod(i) = self.ui.options.page() {
            self.sync_native_settings_page(i);
        }
    }

    pub(in crate::game) fn notify_native_setting_changes(&mut self) {
        // Dispatch the committed batch, not recursively from the host setter. A listener
        // can save another setting; that change is delivered on the next client tick.
        for key in std::mem::take(&mut self.native_settings.changed) {
            let registration = setting_registration(&key);
            let owner = self.native_settings.owners[registration.owner].clone();
            let _ = owner.call("settings", "setting_changed", || {
                self.native_setting_changed(registration.owner, &registration.spec.key);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mod_api::{__private::make_setting_key, SettingSpec};

    #[test]
    #[ignore = "daily"]
    fn client_host_and_mod_page_notify_only_the_registered_owner() {
        use mod_api::{ClientHost, Hooks, ModEntry, SettingChangedEvent};
        use std::sync::{Arc, Mutex};
        use sunrise_client::mod_list::{Installed, NativePage, NativeSetting, NativeSettingValue};

        type Observed = Arc<Mutex<Vec<(&'static str, String, SettingValue)>>>;
        struct Listener(&'static str, SettingKey, Observed);
        #[mod_api::__subscriptions]
        impl Hooks for Listener {
            fn setting_changed(
                &self,
                host: &mut dyn ClientHost,
                event: &mut SettingChangedEvent<'_>,
            ) {
                let value = host.setting(&self.1).unwrap();
                self.2
                    .lock()
                    .unwrap()
                    .push((self.0, event.key.to_owned(), value));
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let alpha = make_setting_key(
            "setting_alpha",
            0,
            SettingSpec::toggle("mode", "alpha.mode", false),
        );
        let beta = make_setting_key(
            "setting_beta",
            1,
            SettingSpec::toggle("mode", "beta.mode", true),
        );
        let observed = Observed::default();
        let mods = vec![
            ModEntry::new(
                "setting_alpha",
                Box::new(Listener("setting_alpha", alpha.clone(), observed.clone())),
            ),
            ModEntry::new(
                "setting_beta",
                Box::new(Listener("setting_beta", beta.clone(), observed.clone())),
            ),
        ];
        mod_platform::testing::with_mods(mods.into(), || {
            crate::game::native_entity_refs::tests::with_client_game(|game| {
                game.native_settings = NativeSettings::load(dir.path(), &[alpha.clone(), beta]);
                let host: &mut dyn ClientHost = game;
                host.set_setting(&alpha, SettingValue::Toggle(true))
                    .unwrap();
                game.settings.mods.installed = vec![Installed {
                    id: "setting_beta".into(),
                    title: "Beta".into(),
                    description: String::new(),
                    problem: None,
                    dev: false,
                    native: Some(NativePage {
                        mappings: Vec::new(),
                        settings: vec![NativeSetting {
                            key: "mode".into(),
                            label: "beta.mode".into(),
                            tooltip: None,
                            value: NativeSettingValue::Toggle(false),
                        }],
                    }),
                }];
                game.native_setting_event(0, 0);
                assert!(observed.lock().unwrap().is_empty());
                game.notify_native_setting_changes();
                assert_eq!(
                    *observed.lock().unwrap(),
                    [
                        ("setting_alpha", "mode".into(), SettingValue::Toggle(true)),
                        ("setting_beta", "mode".into(), SettingValue::Toggle(false)),
                    ]
                );
                assert_eq!(
                    fs::read_to_string(dir.path().join("setting_alpha/mode")).unwrap(),
                    "true"
                );
                assert_eq!(
                    fs::read_to_string(dir.path().join("setting_beta/mode")).unwrap(),
                    "false"
                );
            });
        });
    }

    #[test]
    fn owner_values_reload_and_only_successful_changes_notify() {
        let dir = tempfile::tempdir().unwrap();
        let alpha = make_setting_key("alpha", 0, SettingSpec::toggle("mode", "alpha.mode", false));
        let beta = make_setting_key("beta", 1, SettingSpec::toggle("mode", "beta.mode", true));
        let lookalike =
            make_setting_key("alpha", 0, SettingSpec::toggle("mode", "alpha.mode", false));
        let mut settings = NativeSettings::load(dir.path(), &[alpha.clone(), beta.clone()]);
        assert_eq!(settings.setting(&alpha), Some(SettingValue::Toggle(false)));
        assert_eq!(settings.setting(&beta), Some(SettingValue::Toggle(true)));
        assert_eq!(settings.setting(&lookalike), None);
        assert!(settings.changed.is_empty());
        assert_eq!(
            settings.set_setting(&lookalike, SettingValue::Toggle(true)),
            Err(SettingWriteError::Unavailable)
        );
        assert_eq!(
            settings.set_setting(&alpha, SettingValue::Color(0)),
            Err(SettingWriteError::InvalidValue)
        );

        settings
            .set_setting(&alpha, SettingValue::Toggle(true))
            .unwrap();
        settings
            .set_setting(&alpha, SettingValue::Toggle(true))
            .unwrap();
        let path = dir.path().join("alpha/mode");
        assert_eq!(fs::read_to_string(&path).unwrap(), "true");
        assert_eq!(settings.setting(&alpha), Some(SettingValue::Toggle(true)));
        assert_eq!(settings.changed.len(), 1);
        assert_eq!(
            setting_registration(&settings.changed.remove(0)).owner,
            "alpha"
        );
        assert!(!dir.path().join("beta").exists());

        fs::write(&path, "false").unwrap();
        settings.reload(Some("alpha"));
        assert_eq!(settings.setting(&alpha), Some(SettingValue::Toggle(false)));
        assert_eq!(settings.changed.len(), 1);
        assert_eq!(
            setting_registration(&settings.changed.remove(0)).owner,
            "alpha"
        );

        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert_eq!(
            settings.set_setting(&alpha, SettingValue::Toggle(true)),
            Err(SettingWriteError::SaveFailed)
        );
        assert_eq!(settings.setting(&alpha), Some(SettingValue::Toggle(false)));
        assert!(settings.changed.is_empty());
    }
}
