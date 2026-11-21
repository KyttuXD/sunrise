//! The `setting_changed` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0020

pub trait Hooks {
    /// Setting `key` of mod `mod_id` changed by a client host write, an Options > Mods edit,
    /// or a reload. The client tick delivers this only to the registered owner, after the live
    /// value has been published. Read the current value with `ClientHost::setting` and the key
    /// returned by `Registrar::register_setting`.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_setting_changed",
        dispatch = "flat",
        side = "client",
        contract = "listen",
        owner = "mod_id",
        neoforge = "ModConfigEvent.Reloading (ModConfigSpec.save or reload)",
        version = 2
    )]
    fn setting_changed(mod_id: &str, key: &str);
}
