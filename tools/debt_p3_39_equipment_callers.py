#!/usr/bin/env python3
"""Migrate the existing equipment listeners to living handles, slots and stack loans."""
from pathlib import Path

GAME = Path(__file__).resolve().parents[1] / "crates/sunrise-client/src/game"


def migrate(path, replacements):
    path = GAME / path
    source = path.read_text()
    for old, new in replacements:
        if old not in source:
            raise SystemExit(f"{path}: missing old call {old!r}")
        source = source.replace(old, new)
    path.write_text(source)


def main():
    stack_loans = [
        (f"event.{field}.cloned()", f"event.{field}.as_ref().and_then(mod_api::ItemStackView::project)")
        for field in ("from", "to")
    ]
    migrate("interface/stand_equipment_change_tests.rs", [
        ("self, Hooks,", "self, EquipmentSlot, Hooks,"),
        ("    String,", "    EquipmentSlot,"),
        ("event.entity,", "event.entity.id(),"),
        ("event.slot.to_owned(),", "event.slot,"),
        *stack_loans,
        ("slot.as_str()", "*slot"),
        ('(16, "head",', '(16, EquipmentSlot::Head,'),
        ('(16, "body",', '(16, EquipmentSlot::Body,'),
        ('(16, "saddle",', '(16, EquipmentSlot::Saddle,'),
    ])
    migrate("modding/player_equipment_change_tests.rs", [
        ("self, Hooks,", "self, EquipmentSlot, Hooks,"),
        ("    String,\n    String,", "    Option<mod_api::EntityTypeKey>,\n    EquipmentSlot,"),
        ("_: &mut dyn ServerHost,", "host: &mut dyn ServerHost,"),
        ("event.entity,", "event.entity.id(),"),
        ("event.kind.to_owned(),", "event.entity.entity().kind(host),"),
        ("event.slot.to_owned(),", "event.slot,"),
        *stack_loans,
        ("fn player(slot: &str,", "fn player(slot: EquipmentSlot,"),
        ('(12, "minecraft:player".into(), slot.into(),', '(12, mod_api::__private::resolved_entity_type_key("minecraft:player"), slot,'),
        ('player("mainhand",', 'player(EquipmentSlot::MainHand,'),
        ('player("head",', 'player(EquipmentSlot::Head,'),
        ("game.native_players_equipment()", "game.native_player_equipment(12)"),
    ])
    migrate("modding/native_living_refs.rs", [
        ('if event.entity == 18 && event.slot == "mainhand" {\n                    let entity = host.entity(event.entity).unwrap();\n                    let living = host.living(entity).unwrap();',
         'if event.entity.id() == 18 && event.slot == EquipmentSlot::MainHand {\n                    let living = event.entity;'),
    ])
    migrate("mobs/creatures/mob_tick_access_tests.rs", [
        ('assert_eq!(event.kind, "minecraft:pig");', 'assert_eq!(event.entity.entity().kind(host), mod_api::__private::resolved_entity_type_key("minecraft:pig"));'),
        ('assert_eq!(event.slot, "head");', 'assert_eq!(event.slot, mod_api::EquipmentSlot::Head);'),
        ('host.entity(event.entity).unwrap().position(host).unwrap()', 'event.entity.entity().position(host).unwrap()'),
        ('EquippedTurn::Equipment(event.entity, pos)', 'EquippedTurn::Equipment(event.entity.id(), pos)'),
    ])


if __name__ == "__main__":
    main()
