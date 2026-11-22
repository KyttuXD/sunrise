//! Named ticket controllers (NeoForge's `TicketController` and `RegisterTicketControllersEvent`): a
//! mod registers each controller once at startup, from [`Hooks::register`](crate::Hooks::register),
//! and holds the [`TicketController`] it gets back to hold chunks with.

use super::mod_chunk_tickets::{ControllerTickets, ModTicket};
use crate::{LevelRef, ServerHost, TicketValidation, callback_owner::CallbackOwner};
use std::{collections::BTreeMap, sync::Arc};

// no wire: retained executable code and a borrow of staged tickets stay in-process.
/// Validate one controller's saved chunk tickets in the supplied server level
/// (NeoForge's `LoadingValidationCallback.validateTickets`). The callback runs
/// after `level_load` and before the first tick, only when that controller has
/// saved rows in the level. At startup all built-in levels are available before
/// validation begins. Remove stale tickets through the borrowed validation view;
/// the survivors force their chunks after validation. An owner chunk that is not loaded
/// during validation is not evidence that its owner is gone. A panic is reported and
/// disables only this validator for the session. A phase refusal is reported
/// each time without disabling it. Neither undoes removals already made.
pub type LoadingValidationCallback =
    dyn Fn(&mut dyn ServerHost, LevelRef, &mut dyn TicketValidation) + Send + Sync + 'static;

// no wire: retained code, attribution and session poison state stay in-process.
/// One frozen controller's callback and its registered owner.
// hidden: the authoring bridge moves the binding without inspecting it.
#[doc(hidden)]
#[derive(Clone)]
// exhaustive: private fields keep callback ownership inside the binding.
pub struct RegisteredTicketValidator {
    callback: Arc<LoadingValidationCallback>,
    owner: CallbackOwner,
}

/// What the game knows about a registered controller.
pub(crate) struct ControllerInfo {
    /// The namespaced id, `mymod:quarry`, which the tickets are saved under.
    pub(crate) id: &'static str,
    pub(crate) identity: usize,
    /// The controller's retained saved-ticket validator, if registered.
    pub(crate) validator: Option<RegisteredTicketValidator>,
}

/// A ticket controller a mod registered with
/// [`Registrar::register_ticket_controller`](crate::TicketsRegistrar::register_ticket_controller): the
/// handle `chunk.set_mod_forced` and `level.mod_forced_chunks` take. Only the registrar makes
/// one, resolved once there, and it is `Copy`: keep it from `register`, for example in a
/// `OnceLock`. Active chunk-ticket reads and writes authorize by this registry-issued handle
/// rather than the current caller; separate controllers cannot read or release each other's
/// tickets, and a handle from a refused registration does nothing. Use its namespaced `id()` when
/// you need to store a controller name; that name cannot recover the handle.
// no wire: the handle resolves only in the registry that issued it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
// exhaustive: its fields are private, so mods cannot build or destructure it
pub struct TicketController {
    slot: u32,
    identity: usize,
}

impl TicketController {
    /// The controller's namespaced id (`mymod:quarry`), as its tickets are saved under it; empty
    /// for a registration that was refused or is no longer installed.
    pub fn id(self) -> &'static str {
        self.info().map_or("", |info| info.id)
    }

    fn info(self) -> Option<&'static ControllerInfo> {
        controllers()
            .get(self.slot as usize)
            .filter(|info| info.identity == self.identity)
    }
}

/// Prepare a new controller row at registration, never by looking up an existing name or slot.
pub fn issue_controller(
    slot: u32,
    id: Box<str>,
    mod_id: &'static str,
    mod_index: usize,
    validator: Option<Arc<LoadingValidationCallback>>,
) -> (
    TicketController,
    crate::__private::parts_installation::ControllerInfoInput,
) {
    // The nonempty save name already lives for the session. Retain its allocation so even a
    // handle from an abandoned registration cannot alias a later registration at the same slot.
    let id: &'static str = Box::leak(id);
    let identity = id.as_ptr().addr();
    (
        TicketController { slot, identity },
        crate::__private::parts_installation::ControllerInfoInput {
            id,
            identity,
            validator: validator.map(|callback| RegisteredTicketValidator {
                callback,
                owner: CallbackOwner::for_index(mod_id, mod_index),
            }),
        },
    )
}

/// The inert handle returned for a refused registration.
pub fn refused_controller() -> TicketController {
    TicketController {
        slot: u32::MAX,
        identity: 0,
    }
}

/// Resolve an issued handle for an active ticket operation. Save names and slots cannot do this.
pub fn active_controller_slot(controller: TicketController) -> Option<u32> {
    controller.info().map(|_| controller.slot)
}

/// Resolve a persisted controller name to a store slot, not to an active handle.
pub fn saved_controller_slot(id: &str) -> Option<u32> {
    controllers()
        .iter()
        .position(|info| info.id == id)
        .map(|slot| slot as u32)
}

/// Resolve a store slot to its persisted name, not to an active handle.
pub fn saved_controller_name(slot: u32) -> Option<&'static str> {
    controllers().get(slot as usize).map(|info| info.id)
}

/// Validate a level's staged rows at activation, without lending out callbacks or active handles.
/// Only controllers with staged rows and an enabled validator get a borrowed validation view.
pub fn validate_staged_tickets(
    host: &mut dyn ServerHost,
    level: LevelRef,
    staged: Vec<ModTicket>,
) -> Vec<ModTicket> {
    let controllers = controllers();
    let mut ready = Vec::new();
    let mut groups = BTreeMap::new();
    for ticket in staged {
        let validator = controllers
            .get(ticket.controller as usize)
            .and_then(|info| info.validator.as_ref())
            .filter(|validator| validator.owner.is_enabled());
        if let Some(validator) = validator {
            groups
                .entry(ticket.controller)
                .or_insert_with(|| (validator, Vec::new()))
                .1
                .push(ticket);
        } else {
            ready.push(ticket);
        }
    }
    for (validator, mut rows) in groups.into_values() {
        // CallbackOwner reports refusals and panics; activation still promotes the survivors.
        let _ = validator
            .owner
            .call("chunk ticket validator", "validate_tickets", || {
                (validator.callback)(host, level, &mut ControllerTickets::new(&mut rows));
            });
        ready.append(&mut rows);
    }
    ready
}

#[cfg(any(test, feature = "testing"))]
thread_local! {
    /// The controllers a test's mods registered (`testing::with_dispatch`), in place of the
    /// process-wide table.
    pub(crate) static TEST_CONTROLLERS: std::cell::Cell<Option<&'static [ControllerInfo]>> =
        const { std::cell::Cell::new(None) };
}

fn controllers() -> &'static [ControllerInfo] {
    #[cfg(any(test, feature = "testing"))]
    if let Some(table) = TEST_CONTROLLERS.get() {
        return table;
    }
    crate::parts::installed_controllers()
}
