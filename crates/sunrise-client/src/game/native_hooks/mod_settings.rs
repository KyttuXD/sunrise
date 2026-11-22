use super::*;
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use mod_api::__private::game::CallbackOwner;
use mod_api::{SettingAccessError, SettingKey, SettingKind, SettingSpec, SettingValue};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{ErrorKind, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use sunrise_world::native_load_context::{CapturedSetting, capture_registered_settings};

/// The single client-owned store for settings registered through
/// `SettingsRegistrar::register_setting`. Startup and reload publish the
/// resource generation's capture; Options > Mods can refresh one owner.
#[derive(Default)]
pub(in crate::game) struct NativeSettings {
    values: BTreeMap<(&'static str, &'static str), SettingValue>,
    initialized: bool,
    /// Unsent notices for the current capture. Synchronous writes remove any
    /// superseded notice before dispatching their own.
    pending: BTreeSet<(&'static str, &'static str)>,
    /// A panic disables only that mod's `setting_changed` callback for the
    /// session; phase refusals report each time without disabling it.
    callback_owners: BTreeMap<&'static str, CallbackOwner>,
}

impl NativeSettings {
    /// Read a registered setting's current live value without touching the
    /// filesystem (falling back to the spec's default if the store has not yet
    /// been initialized).
    pub(in crate::game) fn value(
        &self,
        owner: &'static str,
        spec: &'static SettingSpec,
    ) -> SettingValue {
        self.values
            .get(&(owner, spec.key.as_str()))
            .copied()
            .unwrap_or_else(|| spec.default_value())
    }

    /// Validate, persist, and publish a new value for a registered setting.
    ///
    /// Returns `Ok(true)` when the value changed (by `SettingValue::exact_eq`),
    /// `Ok(false)` when it already matched without writing or notifying, or an
    /// explicit `SettingAccessError` when validation or persistence failed
    /// (keeping the previous live value untouched on failure).
    fn commit(
        &mut self,
        root: &Path,
        owner: &'static str,
        spec: &'static SettingSpec,
        value: SettingValue,
    ) -> Result<bool, SettingAccessError> {
        static WRITES: AtomicU64 = AtomicU64::new(0);
        let encoded = encode_setting(spec, value)?;
        let current = self.value(owner, spec);
        if current.exact_eq(value) {
            return Ok(false);
        }
        let sequence = WRITES.fetch_add(1, Ordering::Relaxed);
        persist_setting(root, owner, &spec.key, &encoded, sequence)
            .map_err(|_| SettingAccessError::IoFailed)?;
        let slot = (owner, spec.key.as_str());
        self.values.insert(slot, value);
        self.pending.remove(&slot);
        Ok(true)
    }

    /// Return the session-scoped `CallbackOwner` for `owner`'s `setting_changed`
    /// callback.
    fn callback_owner(&mut self, owner: &'static str) -> CallbackOwner {
        self.callback_owners
            .entry(owner)
            .or_insert_with(|| CallbackOwner::for_mod(owner))
            .clone()
    }
}

/// Check that `value` has the right kind and falls within `spec`'s registered
/// bounds, returning its canonical persisted string form.
fn encode_setting(spec: &SettingSpec, value: SettingValue) -> Result<String, SettingAccessError> {
    let kind_matches = matches!(
        (&spec.kind, value),
        (SettingKind::Toggle { .. }, SettingValue::Toggle(_))
            | (SettingKind::Choice { .. }, SettingValue::Choice(_))
            | (SettingKind::Range { .. }, SettingValue::Range(_))
            | (SettingKind::Color { .. }, SettingValue::Color(_))
    );
    if !kind_matches {
        return Err(SettingAccessError::WrongKind);
    }
    spec.store(value).ok_or(SettingAccessError::InvalidValue)
}

/// Validate that `component` is a single safe normal path component.
fn is_plain_component(component: &str) -> bool {
    let mut components = Path::new(component).components();
    matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
}

/// Persist `value` at `<root>/<owner>/<key>` without following symlinks at the
/// root, mod directory, or setting file, writing to a temporary sibling file
/// and atomically renaming it into place.
fn persist_setting(
    root: &Path,
    owner: &str,
    key: &str,
    value: &str,
    sequence: u64,
) -> std::io::Result<()> {
    if !is_plain_component(owner) || !is_plain_component(key) {
        return Err(std::io::Error::new(
            ErrorKind::InvalidInput,
            "unsafe setting path component",
        ));
    }
    let root_dir = open_or_create_dir_nofollow(root)?;
    let owner_dir = open_or_create_child_dir_nofollow(&root_dir, owner)?;
    match owner_dir.symlink_metadata(key) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
            return Err(std::io::Error::new(
                ErrorKind::InvalidInput,
                "setting target is not a regular file",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let temp = format!(".{key}.{}-{sequence}.tmp", std::process::id());
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    // A name collision is not ours to clean up.
    let result = {
        let mut file = owner_dir.open_with(&temp, &options)?;
        file.write_all(value.as_bytes())
            .and_then(|()| file.sync_all())
    }
    .and_then(|()| owner_dir.rename(&temp, &owner_dir, key));
    if result.is_err() {
        let _ = owner_dir.remove_file(&temp);
    }
    result
}

/// Ensure `root` exists (creating ancestors if needed) and open it as a
/// capability handle without following a symlink at `root` itself.
fn open_or_create_dir_nofollow(root: &Path) -> std::io::Result<Dir> {
    let parent = root.parent().ok_or_else(|| {
        std::io::Error::new(ErrorKind::InvalidInput, "settings root has no parent")
    })?;
    let name = root
        .file_name()
        .ok_or_else(|| std::io::Error::new(ErrorKind::InvalidInput, "settings root has no name"))?;
    if !parent.as_os_str().is_empty() {
        fs::create_dir_all(parent)?;
    }
    let parent_dir = Dir::open_ambient_dir(
        if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        },
        cap_std::ambient_authority(),
    )?;
    open_or_create_child_dir_nofollow(&parent_dir, name)
}

fn open_or_create_child_dir_nofollow(parent: &Dir, name: impl AsRef<Path>) -> std::io::Result<Dir> {
    let name = name.as_ref();
    match parent.symlink_metadata(name) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
            return Err(std::io::Error::new(
                ErrorKind::InvalidInput,
                "settings directory is a symlink or not a directory",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => match parent.create_dir(name) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        },
        Err(error) => return Err(error),
    }
    parent.open_dir_nofollow(name)
}

/// Convert a `(spec, value)` pair to the Options > Mods widget value.
pub(super) fn ui_setting_value(spec: &SettingSpec, value: SettingValue) -> NativeSettingValue {
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
        _ => NativeSettingValue::Toggle(false),
    }
}

/// Typed client settings reads and writes on `Game` using owner-bound
/// `SettingKey` handles (vanilla `OptionInstance`, NeoForge `ModConfigSpec`).
impl Game<'_> {
    fn resolve_setting_key(
        &self,
        key: SettingKey,
    ) -> Result<(&'static str, &'static SettingSpec), SettingAccessError> {
        if !self.options.execution_role.is_client() {
            return Err(SettingAccessError::Unavailable);
        }
        mod_api::__private::setting_entry(key)
            .map(|(owner, spec)| (*owner, spec))
            .ok_or(SettingAccessError::UnknownKey)
    }

    /// Read a client setting through its owner-bound `SettingKey`.
    pub(in crate::game) fn setting(
        &self,
        key: SettingKey,
    ) -> Result<SettingValue, SettingAccessError> {
        let (owner, spec) = self.resolve_setting_key(key)?;
        Ok(self.native_settings.value(owner, spec))
    }

    /// Write a client setting through its owner-bound `SettingKey`, persisting
    /// to disk, updating live state and any open Options > Mods row, and
    /// notifying the owning mod synchronously on change.
    pub(in crate::game) fn set_setting(
        &mut self,
        key: SettingKey,
        value: SettingValue,
    ) -> Result<bool, SettingAccessError> {
        let (owner, spec) = self.resolve_setting_key(key)?;
        self.apply_native_setting_write(owner, spec, value)
    }

    /// Read a toggle setting through its owner-bound `SettingKey`.
    pub(in crate::game) fn setting_bool(
        &self,
        key: SettingKey,
    ) -> Result<bool, SettingAccessError> {
        match self.setting(key)? {
            SettingValue::Toggle(on) => Ok(on),
            _ => Err(SettingAccessError::WrongKind),
        }
    }

    /// Read a choice setting's index through its owner-bound `SettingKey`.
    pub(in crate::game) fn setting_choice(
        &self,
        key: SettingKey,
    ) -> Result<u32, SettingAccessError> {
        match self.setting(key)? {
            SettingValue::Choice(index) => {
                u32::try_from(index).map_err(|_| SettingAccessError::InvalidValue)
            }
            _ => Err(SettingAccessError::WrongKind),
        }
    }

    /// Read a range setting's number through its owner-bound `SettingKey`.
    pub(in crate::game) fn setting_f64(&self, key: SettingKey) -> Result<f64, SettingAccessError> {
        match self.setting(key)? {
            SettingValue::Range(value) => Ok(value),
            _ => Err(SettingAccessError::WrongKind),
        }
    }

    /// Read a colour setting (`0xRRGGBB`) through its owner-bound `SettingKey`.
    pub(in crate::game) fn setting_color(
        &self,
        key: SettingKey,
    ) -> Result<u32, SettingAccessError> {
        match self.setting(key)? {
            SettingValue::Color(rgb) => Ok(rgb),
            _ => Err(SettingAccessError::WrongKind),
        }
    }

    /// Shared commit path for both `ClientHost::set_setting` and Options > Mods
    /// UI writes: validates, persists, updates live state + UI row, and fires
    /// `setting_changed` synchronously through per-owner panic containment.
    pub(super) fn apply_native_setting_write(
        &mut self,
        owner: &'static str,
        spec: &'static SettingSpec,
        value: SettingValue,
    ) -> Result<bool, SettingAccessError> {
        let changed =
            self.native_settings
                .commit(self.loop_context.settings_root(), owner, spec, value)?;
        if changed {
            self.sync_native_setting_ui_row(owner, spec, value);
            self.notify_setting_changed(owner, spec.key.as_str());
        }
        Ok(changed)
    }

    /// Publish the owning load's registered settings. The first publication
    /// aligns the UI with the captured baseline without notifying listeners.
    pub(in crate::game) fn sync_native_settings_from_loop_context(&mut self) {
        if !self.options.execution_role.is_client() {
            return;
        }
        let notify = std::mem::replace(&mut self.native_settings.initialized, true);
        let context = self.loop_context.clone();
        self.apply_captured_native_settings(context.registered_client_settings(), notify);
    }

    /// Refresh one mod's live settings when its page opens (from either
    /// Options > Mods or the recipe browser).
    pub(super) fn refresh_native_settings_owner(&mut self, owner: &str) {
        if !self.options.execution_role.is_client() {
            return;
        }
        let captured = capture_registered_settings(self.loop_context.settings_root(), Some(owner));
        self.apply_captured_native_settings(&captured, self.native_settings.initialized);
    }

    fn apply_captured_native_settings(&mut self, captured: &[CapturedSetting], notify: bool) {
        // Publish the entire capture before calling any mod. A listener may
        // read or write any of its settings with its retained keys.
        for &CapturedSetting { owner, spec, value } in captured {
            let slot = (owner, spec.key.as_str());
            let previous = self
                .native_settings
                .values
                .insert(slot, value)
                .unwrap_or_else(|| spec.default_value());
            let changed = !previous.exact_eq(value);
            if !notify || changed {
                self.sync_native_setting_ui_row(owner, spec, value);
            }
            if notify && changed {
                self.native_settings.pending.insert(slot);
            }
        }
        if notify {
            for &CapturedSetting { owner, spec, .. } in captured {
                let key = spec.key.as_str();
                if self.native_settings.pending.remove(&(owner, key)) {
                    self.notify_setting_changed(owner, key);
                }
            }
        }
    }

    /// Keep the Options > Mods row for `(owner, spec.key)` aligned with `value`
    /// whenever the setting changes through code or a reload.
    fn sync_native_setting_ui_row(&mut self, owner: &str, spec: &SettingSpec, value: SettingValue) {
        let Some(row) = self
            .settings
            .mods
            .installed
            .iter_mut()
            .find(|row| row.id == owner)
        else {
            return;
        };
        let Some(page) = row.native.as_mut() else {
            return;
        };
        if let Some(setting) = page.settings.iter_mut().find(|s| s.key == spec.key) {
            setting.value = ui_setting_value(spec, value);
        }
    }

    /// Deliver `setting_changed` synchronously for `owner`, contained per owner
    /// so a panic disables only that mod's setting callback for the session and
    /// surfaces an error toast, while phase refusals report without disabling.
    fn notify_setting_changed(&mut self, owner: &'static str, key: &str) {
        let callback_owner = self.native_settings.callback_owner(owner);
        let _ = callback_owner.call("setting_changed", "setting_changed", || {
            self.native_setting_changed(owner, key);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mod_api::{
        ClientHost, Hooks, ModEntry, Registrar, RegistrationContext, SettingAccessError,
        SettingKey, SettingSpec, SettingValue,
    };
    use std::sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use sunrise_world::native_load_context::{
        DataLoadOrigin, LoadContext, LoadInputs, LoopSide, MAX_SETTING_KEYS,
    };

    struct RegisteredMod {
        toggle: OnceLock<SettingKey>,
        range: OnceLock<SettingKey>,
        choice: OnceLock<SettingKey>,
        color: OnceLock<SettingKey>,
        seen: Arc<Mutex<Vec<(String, SettingValue)>>>,
        panic_on_change: Arc<AtomicBool>,
        write_scale_on_toggle: bool,
    }

    impl RegisteredMod {
        fn new() -> Self {
            Self {
                toggle: OnceLock::new(),
                range: OnceLock::new(),
                choice: OnceLock::new(),
                color: OnceLock::new(),
                seen: Arc::new(Mutex::new(Vec::new())),
                panic_on_change: Arc::new(AtomicBool::new(false)),
                write_scale_on_toggle: false,
            }
        }
    }

    #[mod_api::__subscriptions]
    impl Hooks for RegisteredMod {
        fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
            let _ = self.toggle.set(
                r.register_setting(SettingSpec::toggle("on", "test.on", false))
                    .unwrap(),
            );
            let _ = self.range.set(
                r.register_setting(SettingSpec::range(
                    "scale",
                    "test.scale",
                    -2.0,
                    2.0,
                    0.5,
                    0.0,
                ))
                .unwrap(),
            );
            let _ = self.choice.set(
                r.register_setting(SettingSpec::choice("mode", "test.mode", ["a", "b"], 0))
                    .unwrap(),
            );
            let _ = self.color.set(
                r.register_setting(SettingSpec::color("tint", "test.tint", 0x11_2233))
                    .unwrap(),
            );
        }

        fn setting_changed(
            &self,
            host: &mut dyn ClientHost,
            event: &mut mod_api::SettingChangedEvent<'_>,
        ) {
            if self.panic_on_change.load(Ordering::Relaxed) {
                panic!("simulated setting_changed failure");
            }
            let key = match event.key {
                "on" => *self.toggle.get().unwrap(),
                "scale" => *self.range.get().unwrap(),
                "mode" => *self.choice.get().unwrap(),
                "tint" => *self.color.get().unwrap(),
                other => panic!("unexpected setting key {other}"),
            };
            let current = host.setting(key).unwrap();
            self.seen
                .lock()
                .unwrap()
                .push((event.key.to_owned(), current));
            if self.write_scale_on_toggle && event.key == "on" {
                let scale = *self.range.get().unwrap();
                assert_eq!(host.setting_f64(scale), Ok(1.0));
                assert_eq!(host.set_setting(scale, SettingValue::Range(2.0)), Ok(true));
                // Return to the captured value. Comparing only values when
                // dispatching the pending notice would wrongly send it again.
                assert_eq!(host.set_setting(scale, SettingValue::Range(1.0)), Ok(true));
            }
        }
    }

    struct OtherMod {
        toggle: OnceLock<SettingKey>,
        calls: Arc<AtomicUsize>,
    }

    #[mod_api::__subscriptions]
    impl Hooks for OtherMod {
        fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
            let _ = self.toggle.set(
                r.register_setting(SettingSpec::toggle("on", "other.on", true))
                    .unwrap(),
            );
        }

        fn setting_changed(
            &self,
            _: &mut dyn ClientHost,
            _: &mut mod_api::SettingChangedEvent<'_>,
        ) {
            self.calls.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn persist_setting_confines_paths_and_refuses_symlinks_and_directories() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("settings");
        fs::create_dir_all(root.join("b")).unwrap();
        fs::write(root.join("b/secret"), "secret").unwrap();

        assert!(persist_setting(&root, "a", "../b/secret", "stolen", 0).is_err());
        assert!(persist_setting(&root, "../b", "secret", "stolen", 0).is_err());
        assert_eq!(fs::read_to_string(root.join("b/secret")).unwrap(), "secret");

        persist_setting(&root, "a", "on", "true", 0).unwrap();
        assert_eq!(fs::read_to_string(root.join("a/on")).unwrap(), "true");

        fs::remove_file(root.join("a/on")).unwrap();
        fs::create_dir(root.join("a/on")).unwrap();
        assert!(persist_setting(&root, "a", "on", "false", 0).is_err());

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("b/secret"), root.join("a/link")).unwrap();
            assert!(persist_setting(&root, "a", "link", "overwrite", 0).is_err());
            assert_eq!(fs::read_to_string(root.join("b/secret")).unwrap(), "secret");
        }
    }

    #[test]
    fn failed_exclusive_open_does_not_delete_an_existing_temporary_setting_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("settings");
        let owner = root.join("a");
        fs::create_dir_all(&owner).unwrap();
        fs::write(owner.join("on"), "false").unwrap();
        let temp = owner.join(format!(".on.{}-0.tmp", std::process::id()));
        fs::write(&temp, "already here").unwrap();

        let error = persist_setting(&root, "a", "on", "true", 0).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(temp).unwrap(), "already here");
        assert_eq!(fs::read_to_string(owner.join("on")).unwrap(), "false");
    }

    #[test]
    #[ignore = "daily"]
    fn single_owner_reads_writes_reloads_exact_f64_and_contains_callback_panics() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("settings");
        fs::create_dir_all(root.join("alpha")).unwrap();
        fs::write(root.join("alpha/on"), "true").unwrap();
        // Earlier names exhaust the generic budget before any of alpha's
        // registered keys; the live store must still see their saved values.
        for index in 0..MAX_SETTING_KEYS {
            fs::write(root.join("alpha").join(format!("{index:04}")), "true").unwrap();
        }

        let alpha = RegisteredMod::new();
        let alpha_seen = alpha.seen.clone();
        let alpha_panic = alpha.panic_on_change.clone();
        let beta_calls = Arc::new(AtomicUsize::new(0));
        let beta = OtherMod {
            toggle: OnceLock::new(),
            calls: beta_calls.clone(),
        };

        let mods: mod_api::Mods = vec![
            ModEntry::new("alpha", Box::new(alpha)),
            ModEntry::new("beta", Box::new(beta)),
        ]
        .into();

        mod_platform::testing::with_mods(mods.clone(), || {
            mod_platform::__private::part_tests::install_parts_fixture(&mods).unwrap();
            let alpha_on = mod_api::__private::setting_key_by_name("alpha", "on").unwrap();
            let alpha_scale = mod_api::__private::setting_key_by_name("alpha", "scale").unwrap();
            let alpha_mode = mod_api::__private::setting_key_by_name("alpha", "mode").unwrap();
            let alpha_tint = mod_api::__private::setting_key_by_name("alpha", "tint").unwrap();
            let beta_on = mod_api::__private::setting_key_by_name("beta", "on").unwrap();
            let foreign = mod_api::__private::setting_key("ghost", 999);

            crate::game::native_entity_refs::tests::with_client_game(|game| {
                let inputs = Arc::new(LoadInputs::new(BTreeMap::new(), root.clone()));
                let load =
                    LoadContext::new(DataLoadOrigin::ClientPreload, inputs, ["alpha", "beta"])
                        .with_registered_client_settings();
                assert_eq!(load.setting("alpha", "on"), None);
                fs::remove_file(root.join("alpha/on")).unwrap();
                // The already captured value establishes the baseline, not a
                // second read at publication, and does not fire setting_changed.
                game.loop_context = Arc::new(load.captured_loop_context(LoopSide::Client));
                game.sync_native_settings_from_loop_context();
                assert_eq!(game.setting_bool(alpha_on), Ok(true));
                assert_eq!(game.setting_f64(alpha_scale), Ok(0.0));
                assert_eq!(game.setting_choice(alpha_mode), Ok(0));
                assert_eq!(game.setting_color(alpha_tint), Ok(0x11_2233));
                assert_eq!(game.setting_bool(beta_on), Ok(true));
                assert!(alpha_seen.lock().unwrap().is_empty());

                // Unknown key and wrong-kind reads return typed errors.
                assert_eq!(game.setting(foreign), Err(SettingAccessError::UnknownKey));
                assert_eq!(
                    game.setting_f64(alpha_on),
                    Err(SettingAccessError::WrongKind)
                );

                // Authoritative write persists, updates live state, and notifies synchronously.
                assert_eq!(
                    game.set_setting(alpha_on, SettingValue::Toggle(false)),
                    Ok(true)
                );
                assert_eq!(game.setting_bool(alpha_on), Ok(false));
                assert_eq!(fs::read_to_string(root.join("alpha/on")).unwrap(), "false");
                assert_eq!(
                    *alpha_seen.lock().unwrap(),
                    vec![("on".to_owned(), SettingValue::Toggle(false))]
                );
                assert_eq!(beta_calls.load(Ordering::Relaxed), 0);

                // Rewriting the same value is a no-op with no notification.
                assert_eq!(
                    game.set_setting(alpha_on, SettingValue::Toggle(false)),
                    Ok(false)
                );
                assert_eq!(alpha_seen.lock().unwrap().len(), 1);

                // Exact f64 comparison: -0.0 over +0.0 is treated as a real change
                // (matching vanilla OptionInstance.set's Objects.equals on boxed Double).
                assert_eq!(
                    game.set_setting(alpha_scale, SettingValue::Range(-0.0)),
                    Ok(true)
                );
                let live_scale = game.setting_f64(alpha_scale).unwrap();
                assert_eq!(live_scale.to_bits(), (-0.0f64).to_bits());
                assert_eq!(alpha_seen.lock().unwrap().len(), 2);

                // Invalid kind or out-of-range write refuses without changing live state.
                assert_eq!(
                    game.set_setting(alpha_scale, SettingValue::Range(99.0)),
                    Err(SettingAccessError::InvalidValue)
                );
                assert_eq!(
                    game.set_setting(alpha_scale, SettingValue::Toggle(true)),
                    Err(SettingAccessError::WrongKind)
                );
                assert_eq!(
                    game.setting_f64(alpha_scale).unwrap().to_bits(),
                    (-0.0f64).to_bits()
                );

                // External disk edit is ignored until reload or Options > Mods refresh,
                // then picked up from a single LoopContext snapshot and notified.
                fs::write(root.join("alpha/on"), "true").unwrap();
                assert_eq!(game.setting_bool(alpha_on), Ok(false));
                let reload = load
                    .for_load(DataLoadOrigin::ClientPreload, ["alpha", "beta"])
                    .with_registered_client_settings();
                game.loop_context = Arc::new(reload.captured_loop_context(LoopSide::Client));
                game.sync_native_settings_from_loop_context();
                assert_eq!(game.setting_bool(alpha_on), Ok(true));
                assert_eq!(alpha_seen.lock().unwrap().len(), 3);

                // Single-owner page refresh picks up external edits and notifies.
                fs::write(root.join("alpha/mode"), "1").unwrap();
                game.refresh_native_settings_owner("alpha");
                assert_eq!(game.setting_choice(alpha_mode), Ok(1));
                assert_eq!(alpha_seen.lock().unwrap().len(), 4);

                // A panic inside alpha's setting_changed disables only alpha's callback
                // for the session while keeping beta's notifications working.
                let _ = mod_api::__private::game::take_callback_panic_notices();
                alpha_panic.store(true, Ordering::Relaxed);
                assert_eq!(
                    game.set_setting(alpha_on, SettingValue::Toggle(false)),
                    Ok(true)
                );
                let notices = mod_api::__private::game::take_callback_panic_notices();
                assert!(notices.iter().any(|n| n.mod_id == "alpha"));

                // Even after clearing the panic flag, alpha's disabled callback does not run again,
                // while live persistence still works and beta still receives its own changes.
                alpha_panic.store(false, Ordering::Relaxed);
                assert_eq!(
                    game.set_setting(alpha_on, SettingValue::Toggle(true)),
                    Ok(true)
                );
                assert_eq!(alpha_seen.lock().unwrap().len(), 4);
                assert_eq!(
                    game.set_setting(beta_on, SettingValue::Toggle(false)),
                    Ok(true)
                );
                assert_eq!(beta_calls.load(Ordering::Relaxed), 1);
            });
        });
    }

    #[test]
    #[ignore = "daily"]
    fn reload_and_page_refresh_skip_notices_superseded_by_listener_writes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("settings");
        fs::create_dir_all(root.join("alpha")).unwrap();
        let alpha = RegisteredMod {
            write_scale_on_toggle: true,
            ..RegisteredMod::new()
        };
        let seen = alpha.seen.clone();
        let mods: mod_api::Mods = vec![ModEntry::new("alpha", Box::new(alpha))].into();
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_client_game(|game| {
                let _phase =
                    mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
                let scale = mod_api::__private::setting_key_by_name("alpha", "scale").unwrap();
                let load = LoadContext::new(
                    DataLoadOrigin::ClientPreload,
                    Arc::new(LoadInputs::new(BTreeMap::new(), root.clone())),
                    ["alpha"],
                )
                .with_registered_client_settings();
                game.loop_context = Arc::new(load.captured_loop_context(LoopSide::Client));
                game.sync_native_settings_from_loop_context();

                for (resource_reload, on) in [(true, true), (false, false)] {
                    game.set_setting(scale, SettingValue::Range(0.0)).unwrap();
                    seen.lock().unwrap().clear();
                    fs::write(root.join("alpha/on"), on.to_string()).unwrap();
                    fs::write(root.join("alpha/scale"), "1").unwrap();
                    if resource_reload {
                        let reload = load
                            .for_load(DataLoadOrigin::ClientPreload, ["alpha"])
                            .with_registered_client_settings();
                        game.loop_context =
                            Arc::new(reload.captured_loop_context(LoopSide::Client));
                        game.sync_native_settings_from_loop_context();
                    } else {
                        game.refresh_native_settings_owner("alpha");
                    }
                    assert_eq!(
                        *seen.lock().unwrap(),
                        [
                            ("on".to_owned(), SettingValue::Toggle(on)),
                            ("scale".to_owned(), SettingValue::Range(2.0)),
                            ("scale".to_owned(), SettingValue::Range(1.0)),
                        ],
                    );
                    assert_eq!(game.setting_f64(scale), Ok(1.0));
                    assert_eq!(fs::read_to_string(root.join("alpha/scale")).unwrap(), "1");
                }
            });
        });
    }
}
