use super::*;
/// Owner-bound settings retained for the client's mod pages and typed reads.
pub fn settings() -> &'static [(&'static str, crate::SettingSpec)] {
    installed().map_or(&[], |installed| &installed.keys.settings)
}

/// Mint an owner-bound [`SettingKey`](crate::SettingKey) during registration.
pub fn setting_key(owner: &'static str, slot: u32) -> crate::SettingKey {
    crate::SettingKey { owner, slot }
}

/// Resolve an owner-bound [`SettingKey`](crate::SettingKey) to its installed `(owner, spec)`.
pub fn setting_entry(
    key: crate::SettingKey,
) -> Option<&'static (&'static str, crate::SettingSpec)> {
    let entry = settings().get(key.slot as usize)?;
    (entry.0 == key.owner).then_some(entry)
}

/// Resolve an installed `(owner, key)` pair to its minted [`SettingKey`](crate::SettingKey).
pub fn setting_key_by_name(mod_id: &str, key: &str) -> Option<crate::SettingKey> {
    settings()
        .iter()
        .enumerate()
        .find(|(_, (owner, spec))| *owner == mod_id && spec.key == key)
        .and_then(|(index, (owner, _))| {
            Some(crate::SettingKey {
                owner,
                slot: u32::try_from(index).ok()?,
            })
        })
}

/// Registrar operations for settings.
pub trait SettingsRegistrar {
    /// Register a setting shown on the calling mod's page in Options > Mods (NeoForge's
    /// `ModConfigSpec` entries). Called from `Hooks::register`; client settings only. Returns an
    /// owner-bound [`SettingKey`](crate::SettingKey) used to read and write this setting on
    /// [`ClientHost`](crate::ClientHost). A setting
    /// [`SettingSpec::check`](crate::SettingSpec::check) refuses, or a key the mod registered
    /// already, is not registered and comes back as the error.
    fn register_setting(
        &mut self,
        setting: crate::SettingSpec,
    ) -> Result<crate::SettingKey, crate::SettingError>;
}
