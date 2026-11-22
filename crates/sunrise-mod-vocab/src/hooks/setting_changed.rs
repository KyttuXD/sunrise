//! The `setting_changed` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0020

pub trait Hooks {
    /// A registered setting changed through a typed write, Options > Mods, or a settings reload.
    /// Runs on the side that changed the value, after publishing it. Only the setting's owner
    /// hears it and can read the new value using its [`SettingKey`](crate::SettingKey).
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_setting_changed",
        dispatch = "flat",
        side = "both",
        contract = "listen",
        owner = "mod_id",
        neoforge = "ModConfigEvent.Reloading",
        version = 2
    )]
    fn setting_changed(mod_id: &str, key: &str);
}
