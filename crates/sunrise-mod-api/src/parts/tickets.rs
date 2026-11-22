//! Concrete authoring extracted verbatim from the vocabulary layer.
use super::*;
use crate::__private::ticket_controllers::{issue_controller, refused_controller};

/// The ticket controllers `mods` register, for a test's mods.
#[cfg(any(test, feature = "testing"))]
pub(crate) fn registered_controllers(mods: &[ModEntry]) -> (Vec<ControllerInfo>, Vec<PoseInfo>) {
    let registered = register_all(mods, |_| false);
    (registered.controllers, registered.arm_poses)
}

/// Fixtures that tests of other crates use to install Parts.
#[cfg(any(test, feature = "testing"))]
pub mod tests {
    #[cfg(test)]
    use super::*;

    #[cfg(test)]
    fn noop_validator(_: &mut dyn ServerHost, _: LevelRef, _: &mut dyn TicketValidation) {}

    #[test]
    pub(crate) fn ticket_controllers_get_the_callers_namespace_and_refuse_anothers() {
        let mut r = super::PartRegistrar::new("first");
        r.register_ticket_controller("quarry", Some(Arc::new(noop_validator)));
        r.register_ticket_controller("first:drill", None);
        assert!(r.refused.is_empty(), "{:?}", r.refused);
        let ids: Vec<_> = r
            .controllers
            .iter()
            .map(|c| (c.id, c.validator.is_some()))
            .collect();
        assert_eq!(ids, [("first:quarry", true), ("first:drill", false)]);
        // Another mod's namespace, and no name at all.
        r.register_ticket_controller("second:quarry", Some(Arc::new(noop_validator)));
        r.register_ticket_controller("first:", Some(Arc::new(noop_validator)));
        r.register_ticket_controller("", Some(Arc::new(noop_validator)));
        assert_eq!(r.refused.len(), 3, "{:?}", r.refused);
        assert!(r.refused[0].contains("first declares \"second:quarry\""));
        assert!(r.refused[0].contains("unowned namespace \"second\""));
        assert!(
            r.refused[1].contains("invalid identifier")
                && r.refused[2].contains("invalid identifier")
        );
        assert_eq!(r.controllers.len(), 2);
    }

    #[test]
    fn installed_ticket_controllers_read_the_installed_table() {
        let mut registrar = PartRegistrar::new("first");
        registrar.register_ticket_controller("quarry", Some(Arc::new(noop_validator)));
        install_registrar(registrar, &crate::parts::tests::rule_schema()).unwrap();
        let controllers = mod_vocab::__private::parts_installation::testing_controllers();
        assert_eq!(controllers, [("first:quarry", true)]);
    }

    #[test]
    pub(crate) fn a_ticket_controller_registered_twice_refuses_the_mods_naming_mod_and_id() {
        let mut r = super::PartRegistrar::new("first");
        r.register_ticket_controller("quarry", Some(Arc::new(noop_validator)));
        r.register_ticket_controller("first:quarry", None);
        r.mod_id = "second";
        // The same path under another mod is another id.
        r.register_ticket_controller("quarry", None);
        assert_eq!(r.refused.len(), 1, "{:?}", r.refused);
        assert_eq!(
            r.refused[0],
            "first registered ticket controller first:quarry twice"
        );
        assert_eq!(r.controllers.len(), 2);
        assert_eq!(r.controllers[1].id, "second:quarry");
        assert!(r.controllers[0].validator.is_some());
        assert!(r.controllers[1].validator.is_none());
    }
}

impl TicketsRegistrar for PartRegistrar {
    fn register_ticket_controller(
        &mut self,
        name: &str,
        validator: Option<Arc<LoadingValidationCallback>>,
    ) -> TicketController {
        register(
            &mut self.controllers,
            &mut self.refused,
            self.mod_id,
            self.mod_index,
            name,
            validator,
        )
    }
}

/// Adds controller `name` of mod `mod_id` to `table`. A name in another mod's namespace, an empty
/// one and a repeated id are written to `refused` and give a handle no table resolves.
pub(crate) fn register(
    table: &mut Vec<ControllerInfo>,
    refused: &mut Vec<String>,
    mod_id: &'static str,
    mod_index: usize,
    name: &str,
    validator: Option<Arc<LoadingValidationCallback>>,
) -> TicketController {
    let id = match owned_declaration_id(mod_id, name) {
        Ok(id) => id,
        Err(reason) => {
            refused.push(reason);
            return refused_controller();
        }
    };
    if table.iter().any(|info| info.id == id.as_str()) {
        refused.push(format!("{mod_id} registered ticket controller {id} twice"));
        return refused_controller();
    }
    let (handle, row) =
        issue_controller(table.len() as u32, id.into(), mod_id, mod_index, validator);
    table.push(row);
    handle
}
