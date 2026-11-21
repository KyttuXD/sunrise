//! Exports of `crate::parts` items defined in `parts/items.rs`.
pub use parts::{ClientItemUseBehavior, ItemCapabilityProvider, ItemProvider};
mod __private {
    pub use crate::parts::{
        ItemKeyError, TooltipAppenderPart, VANILLA_TOOLTIP_APPENDERS, VanillaTooltipAppender,
        client_item_use, item_listens, tooltip_appenders,
    };
}
