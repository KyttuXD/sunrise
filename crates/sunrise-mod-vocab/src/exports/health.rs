pub use crate::entity::health::{
    DamageContainer, DamageContainerView, DamageWriteRefusal, HurtSource, Reduction, ShieldBlock,
    Victim,
};

mod __private {
    pub use crate::entity::health::{
        DamageContainerBackend, DamageContainerRead, wrap_reduction_modifier,
    };
}
