use super::*;

// no wire: identifies an in-process registration, not a serialized key.
/// The owner-bound key returned when a setting is registered. Retain it to read or
/// write that setting through the client host, without naming a mod or a file.
#[derive(Clone, Debug)]
pub struct SettingKey(Arc<SettingRegistration>);

/// The engine's registration row, shared by the registry and its key.
#[derive(Debug)]
pub struct SettingRegistration {
    pub owner: &'static str,
    pub index: usize,
    pub spec: crate::SettingSpec,
}

pub fn make_setting_key(owner: &'static str, index: usize, spec: crate::SettingSpec) -> SettingKey {
    SettingKey(Arc::new(SettingRegistration { owner, index, spec }))
}

pub fn setting_registration(key: &SettingKey) -> &SettingRegistration {
    &key.0
}

/// Registered settings in registration order.
pub fn settings() -> &'static [SettingKey] {
    installed().map_or(&[], |installed| &installed.keys.settings)
}

/// Registrar operations for settings.
pub trait SettingsRegistrar {
    /// Register a setting shown on the calling mod's page in Options > Mods (NeoForge's
    /// `ModConfigSpec` entries). Called from `Hooks::register`; client settings only. A setting
    /// [`SettingSpec::check`](crate::SettingSpec::check) refuses, or a key the mod registered
    /// already, is not registered and comes back as the error. Keep the returned key for
    /// live reads and writes through [`crate::ClientHost`].
    fn register_setting(
        &mut self,
        setting: crate::SettingSpec,
    ) -> Result<SettingKey, crate::SettingError>;
}
