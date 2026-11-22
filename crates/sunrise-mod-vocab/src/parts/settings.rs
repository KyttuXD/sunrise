use super::*;
/// Owner-bound settings retained for live access and the client's mod pages.
pub fn settings() -> &'static [(&'static str, crate::SettingSpec)] {
    installed().map_or(&[], |installed| &installed.keys.settings)
}

/// The setting `key` mod `mod_id` registered, if any.
pub fn setting(mod_id: &str, key: &str) -> Option<&'static crate::SettingSpec> {
    settings()
        .iter()
        .find(|(owner, spec)| *owner == mod_id && spec.key == key)
        .map(|(_, spec)| spec)
}

/// Mint an owner-bound [`SettingKey`](crate::SettingKey) during registration.
pub fn setting_key(owner: &'static str, slot: usize) -> crate::SettingKey {
    crate::SettingKey { owner, slot }
}

/// Resolve an owner-bound [`SettingKey`](crate::SettingKey) to its installed `(owner, spec)`.
pub fn setting_entry(
    key: crate::SettingKey,
) -> Option<&'static (&'static str, crate::SettingSpec)> {
    let entry = settings().get(key.slot)?;
    (entry.0 == key.owner).then_some(entry)
}

/// Resolve an installed `(owner, key)` pair to its minted [`SettingKey`](crate::SettingKey).
pub fn setting_key_by_name(mod_id: &str, key: &str) -> Option<crate::SettingKey> {
    settings()
        .iter()
        .enumerate()
        .find(|(_, (owner, spec))| *owner == mod_id && spec.key == key)
        .map(|(slot, (owner, _))| crate::SettingKey { owner, slot })
}

/// Registrar operations for settings.
pub trait SettingsRegistrar {
    /// Register a setting (NeoForge's `ModConfigSpec` entries). Called from `Hooks::register`.
    /// Returns an owner-bound [`SettingKey`](crate::SettingKey) used to read and write the setting
    /// on [`ClientHost`](crate::ClientHost) or [`ServerHost`](crate::ServerHost). The client also
    /// shows it on the calling mod's page in Options > Mods. A setting
    /// [`SettingSpec::check`](crate::SettingSpec::check) refuses, or a key the mod registered
    /// already, is not registered and comes back as the error.
    fn register_setting(
        &mut self,
        setting: crate::SettingSpec,
    ) -> Result<crate::SettingKey, crate::SettingError>;
}
