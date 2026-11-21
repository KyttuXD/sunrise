pub use crate::entity::landing_entity::{LandingEntity, LandingEntityAccess};
mod __private {
    pub use crate::entity::landing_entity::LandingDeferred;
    mod game {
        pub use crate::entity::landing_entity::DeferredLandingAction;
    }
}
