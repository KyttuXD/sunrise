#!/usr/bin/env python3
"""Remove the obsolete event opt-in methods and imports from Rust hook fixtures."""

from pathlib import Path
import re
import subprocess


POLL = re.compile(
    r"^(?P<indent> +)fn (?:entity_tick_listened|living_equipment_listened|hears_projectile_impacts)"
    r"\([^{}]*\) \{\n(?:(?!^(?P=indent)\}).*\n)*^(?P=indent)\}\n\n?",
    re.MULTILINE,
)
EVENT = re.compile(
    r"\b(?:EntityTickListenedEvent|LivingEquipmentListenedEvent|HearsProjectileImpactsEvent), "
)


def main():
    root = Path(__file__).resolve().parents[1]
    paths = subprocess.check_output(
        ["git", "ls-files", "-z", "--", "crates/sunrise-client", "crates/sunrise-hot-paths-game"],
        cwd=root,
    ).decode().split("\0")
    for path in paths:
        if not path.endswith(".rs"):
            continue
        source = root / path
        original = source.read_text()
        updated = POLL.sub("", original)
        if original != updated:
            source.write_text(EVENT.sub("", updated))


if __name__ == "__main__":
    main()
