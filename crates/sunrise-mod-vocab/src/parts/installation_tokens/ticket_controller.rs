//! Construction before the immutable controller table exists.
use crate::{
    LoadingValidationCallback,
    runtime::RetainedCallback,
    world::ticket_controller::{RegisteredTicketValidator, issue_token},
};
use std::sync::Arc;

/// The sentinel handle returned for a refused controller registration.
pub fn refused_ticket_controller() -> crate::TicketController {
    crate::TicketController {
        slot: u32::MAX,
        token: 0,
    }
}

/// Build one controller table entry together with its newly issued opaque handle.
/// Mints a fresh non-zero token, so it cannot forge a handle for an already-installed table.
pub fn register_ticket_controller(
    slot: u32,
    id: Box<str>,
    mod_id: &'static str,
    mod_index: usize,
    validator: Option<Arc<LoadingValidationCallback>>,
) -> (super::ControllerInfoInput, crate::TicketController) {
    let token = issue_token();
    let validator = validator.map(|callback| {
        RegisteredTicketValidator::new(
            callback,
            RetainedCallback::new(mod_id, mod_index, "validate_tickets", &id),
        )
    });
    let info = super::ControllerInfoInput {
        id,
        token,
        validator,
    };
    let handle = crate::TicketController { slot, token };
    (info, handle)
}
