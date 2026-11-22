pub use world::ticket_controller::{LoadingValidationCallback, TicketController};

mod __private {
    // hidden: opaque authoring input and game-side dispatch.
    #[doc(hidden)]
    pub use crate::world::ticket_controller::RegisteredTicketValidator;
}
