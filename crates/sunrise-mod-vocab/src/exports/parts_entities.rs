//! Exports of `crate::parts` items defined in `parts/entities.rs`.
pub use parts::{
    EntityCapabilityProvider, EntityProvider, EntityProviders, entity_providers,
    has_entity_providers,
};
mod __private {
    pub use crate::parts::{EntityTickEach, entity_tick_each};
}
