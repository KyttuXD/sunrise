//! The client window owns live mod settings. Resource readers keep their separate,
//! immutable load inputs; a newly installed client generation supplies the live values.
use super::Game;
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use mod_api::{
    __private::{game::CallbackOwner, setting_registration},
    SettingKey, SettingKind, SettingSpec, SettingValue, SettingWriteError,
};
use std::{
    collections::HashMap,
    fs,
    io::{self, ErrorKind, Write},
    path::{Path, PathBuf},
    ptr,
};
use sunrise_client::mod_list::{NativeSetting, NativeSettingValue};

#[derive(Default)]
pub(in crate::game) struct NativeSettings {
    root: PathBuf,
    entries: Vec<Entry>,
    callbacks: HashMap<&'static str, CallbackOwner>,
    notifying: bool,
    nested_changes: Vec<SettingKey>,
}

struct Entry {
    key: SettingKey,
    value: SettingValue,
}

impl NativeSettings {
    fn load<'a>(
        root: PathBuf,
        keys: &[SettingKey],
        read: impl Fn(&str, &str) -> Option<&'a str>,
    ) -> Self {
        let mut settings = Self {
            root,
            ..Self::default()
        };
        for key in keys {
            let registration = setting_registration(key);
            let spec = &registration.spec;
            settings.entries.push(Entry {
                key: key.clone(),
                value: spec.read(read(registration.owner, &spec.key)),
            });
            settings
                .callbacks
                .entry(registration.owner)
                .or_insert_with(|| CallbackOwner::for_mod(registration.owner));
        }
        settings
    }

    fn refresh<'a>(
        &mut self,
        only_owner: Option<&str>,
        read: impl Fn(&str, &str) -> Option<&'a str>,
    ) -> Vec<SettingKey> {
        let mut changed = Vec::new();
        for entry in &mut self.entries {
            let registration = setting_registration(&entry.key);
            if only_owner.is_some_and(|owner| owner != registration.owner) {
                continue;
            }
            let spec = &registration.spec;
            let value = spec.read(read(registration.owner, &spec.key));
            if changed_value(entry.value, value) {
                entry.value = value;
                changed.push(entry.key.clone());
            }
        }
        changed
    }

    fn setting(&self, key: &SettingKey) -> Option<SettingValue> {
        let registration = setting_registration(key);
        self.entries
            .get(registration.index)
            .filter(|entry| ptr::eq(setting_registration(&entry.key), registration))
            .map(|entry| entry.value)
    }

    fn write(&mut self, key: &SettingKey, value: SettingValue) -> Result<bool, SettingWriteError> {
        let registration = setting_registration(key);
        let owner = registration.owner;
        let spec = &registration.spec;
        let Some(entry) = self
            .entries
            .get_mut(registration.index)
            .filter(|entry| ptr::eq(setting_registration(&entry.key), registration))
        else {
            log::warn!("mod {owner} setting {}: unavailable registration", spec.key);
            return Err(SettingWriteError::Unavailable);
        };
        if !changed_value(entry.value, value) {
            return Ok(false);
        }
        let Some(encoded) = spec.store(value) else {
            log::warn!("mod {owner} setting {}: invalid value {value:?}", spec.key);
            return Err(SettingWriteError::InvalidValue);
        };
        if let Err(error) = persist(&self.root, owner, &spec.key, &encoded) {
            log::warn!("mod {owner} setting {}: could not save: {error}", spec.key);
            return Err(SettingWriteError::SaveFailed);
        }
        entry.value = value;
        Ok(true)
    }
}

// OptionInstance.set compares boxed Doubles, for which the sign of zero matters.
// SettingSpec rejects non-finite ranges at the input boundary.
fn changed_value(before: SettingValue, after: SettingValue) -> bool {
    match (before, after) {
        (SettingValue::Range(before), SettingValue::Range(after)) => {
            before.to_bits() != after.to_bits()
        }
        _ => before != after,
    }
}

/// Keep the original per-key format. The registered owner and filename are already
/// checked; use the same no-follow directory boundary as the reload's snapshot.
fn persist(root: &Path, owner: &str, key: &str, encoded: &str) -> io::Result<()> {
    let (Some(parent), Some(name)) = (root.parent(), root.file_name()) else {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "missing settings directory",
        ));
    };
    fs::create_dir_all(parent)?;
    let mut dir = Dir::open_ambient_dir(parent, cap_std::ambient_authority())?;
    for component in [name, owner.as_ref()] {
        match dir.create_dir(component) {
            Ok(()) => (),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error),
        }
        dir = dir.open_dir_nofollow(component)?;
    }
    match dir.symlink_metadata(key) {
        Ok(meta) if !meta.is_file() => {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "not a regular settings file",
            ));
        }
        Ok(_) => (),
        Err(error) if error.kind() == ErrorKind::NotFound => (),
        Err(error) => return Err(error),
    }
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create(true)
        .truncate(true)
        .follow(FollowSymlinks::No);
    dir.open_with(key, &options)?.write_all(encoded.as_bytes())
}

/// Both initial client loading and reload capture enough input for every registered
/// settings owner, including a built-in native mod without a content package.
pub(in crate::game) fn client_setting_owners<'a>(
    ids: impl Iterator<Item = &'a str>,
) -> Vec<&'a str> {
    ids.chain(
        mod_api::__private::settings()
            .iter()
            .map(|key| setting_registration(key).owner),
    )
    .collect()
}

fn ui_value(spec: &SettingSpec, value: SettingValue) -> NativeSettingValue {
    match (&spec.kind, value) {
        (_, SettingValue::Toggle(on)) => NativeSettingValue::Toggle(on),
        (SettingKind::Choice { options, .. }, SettingValue::Choice(index)) => {
            NativeSettingValue::Choice {
                options: options.clone(),
                index,
            }
        }
        (SettingKind::Range { min, max, step, .. }, SettingValue::Range(value)) => {
            NativeSettingValue::Range {
                min: *min,
                max: *max,
                step: *step,
                value,
            }
        }
        (_, SettingValue::Color(rgb)) => NativeSettingValue::Color(rgb),
        _ => unreachable!("the live value was validated against this setting's spec"),
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
        let changed = self.native_settings.write(key, value)?;
        if changed {
            self.publish_native_setting(key);
        }
        Ok(())
    }

    pub(in crate::game) fn sync_native_settings(&mut self) {
        if !self.options.execution_role.is_client() {
            return;
        }
        let context = &self.loop_context;
        if self.native_settings.entries.is_empty() {
            self.native_settings = NativeSettings::load(
                Self::mod_data_root(),
                mod_api::__private::settings(),
                |owner, key| context.setting(owner, key),
            );
            return;
        }
        for key in self
            .native_settings
            .refresh(None, |owner, key| context.setting(owner, key))
        {
            self.publish_native_setting(&key);
        }
    }

    pub(super) fn native_page_settings(&self, owner: &str) -> Vec<NativeSetting> {
        self.native_settings
            .entries
            .iter()
            .map(|entry| (setting_registration(&entry.key), entry.value))
            .filter(|(registration, _)| registration.owner == owner)
            .map(|(registration, value)| {
                let spec = &registration.spec;
                NativeSetting {
                    key: spec.key.clone(),
                    label: spec.label.clone(),
                    tooltip: spec.tooltip.clone(),
                    value: ui_value(spec, value),
                }
            })
            .collect()
    }

    /// Opening a page rereads that owner's files and publishes any external edit.
    pub(in crate::game) fn native_settings_refresh(&mut self, i: usize) {
        let Some(row) = self
            .settings
            .mods
            .installed
            .get(i)
            .filter(|row| row.native.is_some())
        else {
            return;
        };
        let captured = sunrise_world::native_load_context::capture_settings(
            &self.native_settings.root,
            [row.id.as_str()],
        );
        for key in self.native_settings.refresh(Some(&row.id), |owner, key| {
            captured.get(owner)?.get(key).map(String::as_str)
        }) {
            self.publish_native_setting(&key);
        }
    }

    /// Widgets propose a typed value. A failed save restores the displayed live value.
    pub(in crate::game) fn native_setting_event(&mut self, i: usize, j: usize) {
        let Some(row) = self.settings.mods.installed.get(i) else {
            return;
        };
        let Some(setting) = row.native.as_ref().and_then(|page| page.settings.get(j)) else {
            return;
        };
        let Some(entry) = self.native_settings.entries.iter().find(|entry| {
            let registration = setting_registration(&entry.key);
            registration.owner == row.id && registration.spec.key == setting.key
        }) else {
            return;
        };
        let key = entry.key.clone();
        let previous = entry.value;
        let value = match &setting.value {
            NativeSettingValue::Toggle(on) => SettingValue::Toggle(*on),
            NativeSettingValue::Choice { index, .. } => SettingValue::Choice(*index),
            NativeSettingValue::Range { value, .. } => SettingValue::Range(*value),
            NativeSettingValue::Color(rgb) => SettingValue::Color(*rgb),
        };
        if self.set_setting(&key, value).is_err()
            && let Some(setting) = self
                .settings
                .mods
                .installed
                .get_mut(i)
                .and_then(|row| row.native.as_mut())
                .and_then(|page| page.settings.get_mut(j))
        {
            setting.value = ui_value(&setting_registration(&key).spec, previous);
        }
    }

    fn publish_native_setting(&mut self, key: &SettingKey) {
        let registration = setting_registration(key);
        let owner = registration.owner;
        let spec = &registration.spec;
        let value = self.native_settings.entries[registration.index].value;
        if let Some(widget) = self
            .settings
            .mods
            .installed
            .iter_mut()
            .find(|row| row.id == owner)
            .and_then(|row| row.native.as_mut())
            .and_then(|page| {
                page.settings
                    .iter_mut()
                    .find(|setting| setting.key == spec.key)
            })
        {
            widget.value = ui_value(spec, value);
        }
        if self.native_settings.notifying {
            // A listener can save a setting. Do not let a chain of those writes recurse
            // through callbacks and exhaust the stack before unrelated work can run.
            self.native_settings.nested_changes.push(key.clone());
        } else {
            self.notify_setting_change(key);
        }
    }

    pub(in crate::game) fn notify_nested_setting_changes(&mut self) {
        for key in std::mem::take(&mut self.native_settings.nested_changes) {
            self.notify_setting_change(&key);
        }
    }

    fn notify_setting_change(&mut self, key: &SettingKey) {
        let registration = setting_registration(key);
        let owner = registration.owner;
        let callback = self.native_settings.callbacks[owner].clone();
        self.native_settings.notifying = true;
        let _ = callback.call("setting_changed", "setting_changed", || {
            self.native_setting_changed(owner, &registration.spec.key);
        });
        self.native_settings.notifying = false;
    }
}

#[cfg(test)]
mod tests;
