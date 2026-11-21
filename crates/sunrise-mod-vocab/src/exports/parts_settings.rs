//! Exports of `crate::parts` items defined in `parts/settings.rs`.
pub use crate::parts::SettingKey;
mod __private {
    pub use crate::parts::{make_setting_key, setting_registration, settings};
}
