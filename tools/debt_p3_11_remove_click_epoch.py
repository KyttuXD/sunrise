#!/usr/bin/env python3
"""Remove the click-only epoch argument from the remaining action callers."""

from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1] / "crates/sunrise-client/src/game"
for path, old_import, new_import in [
    ("modding/native_quick_move.rs", "use super::{Game, decided_action::Epoch};\n", "use super::Game;\n"),
    ("network/lan_stations.rs", "    decided_action::Epoch,\n", ""),
    ("network/lan_trade_decisions.rs", "decided_action::{DecidedTarget, Epoch}", "decided_action::DecidedTarget"),
]:
    file = ROOT / path
    source = file.read_text()
    assert source.count(old_import) == 1, path
    source = source.replace(old_import, new_import)
    source, arguments = re.subn(r"(?m)^ +Epoch::Player,\n", "", source)
    assert arguments == (2 if path.endswith("lan_stations.rs") else 1), path
    file.write_text(source)
