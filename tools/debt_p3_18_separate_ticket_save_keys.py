#!/usr/bin/env python3
"""Use internal save-key lookups instead of reconstructing ticket handles."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "crates/sunrise-client/src/game"
RUNTIME = "mod_api::__ticket_controller_runtime"
REPLACEMENTS = {
    "world/levels.rs": {
        "save.load_mod_tickets(|id| mod_api::TicketController::from_id(id).map(|c| c.slot()))":
            f"save.load_mod_tickets({RUNTIME}::saved_controller_slot)",
        'let controller = TicketController::from_id("startup_tickets:quarry").unwrap();':
            f'let controller = {RUNTIME}::saved_controller_slot("startup_tickets:quarry").unwrap();',
        "controller: controller.slot(),": "controller,",
        ".of_controller(controller.slot())": ".of_controller(controller)",
        "|slot| {\n                    TicketController::from_slot(slot).map(TicketController::id)\n                }":
            f"{RUNTIME}::saved_controller_name",
        "TicketController, TicketValidation,": "TicketValidation,",
    },
    "world/world_loading.rs": {
        "save.load_mod_tickets(|id| mod_api::TicketController::from_id(id).map(|c| c.slot()))":
            f"save.load_mod_tickets({RUNTIME}::saved_controller_slot)",
    },
    "world/saving.rs": {
        "|slot| {\n                mod_api::TicketController::from_slot(slot).map(|c| c.id())\n            }":
            f"{RUNTIME}::saved_controller_name",
    },
    "modding/native_level_refs.rs": {
        "TicketController::from_id(id).map(TicketController::slot)":
            f"{RUNTIME}::saved_controller_slot(id)",
        "|slot| {\n                    TicketController::from_slot(slot).map(TicketController::id)\n                }":
            f"{RUNTIME}::saved_controller_name",
        "controller(id).slot()": "known_slot(id).unwrap()",
    },
}

for relative, replacements in REPLACEMENTS.items():
    path = ROOT / relative
    source = path.read_text()
    for old, new in replacements.items():
        if old not in source:
            raise SystemExit(f"{path}: missing {old!r}")
        source = source.replace(old, new)
    path.write_text(source)
