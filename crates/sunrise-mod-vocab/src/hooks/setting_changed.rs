//! The `setting_changed` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0020

pub trait Hooks {
    /// Setting `key` of mod `mod_id` (`SettingsRegistrar::register_setting`) changed through
    /// Options > Mods, [`ConfigClientHost::set_setting`](crate::prelude::ConfigClientHost::set_setting),
    /// or a disk change picked up at resource reload or when opening the mod's options page.
    /// Read the published value with [`ConfigClientHost::setting`](crate::prelude::ConfigClientHost::setting)
    /// or the typed getters using the [`SettingKey`](crate::SettingKey) returned at registration.
    /// An earlier listener call can supersede a pending reload change by writing that key;
    /// the write notifies synchronously and the superseded notice is not sent.
    /// Only mod `mod_id` hears it; startup does not fire this hook.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_setting_changed",
        dispatch = "flat",
        side = "client",
        contract = "listen",
        owner = "mod_id",
        neoforge = "ModConfigEvent.Reloading (ModConfigSpec.save or a config-file reload)",
        version = 2
    )]
    fn setting_changed(mod_id: &str, key: &str);
}
