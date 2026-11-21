#!/usr/bin/env python3
"""Mark ordinary server menu removal sites as doCloseContainer notifications."""
import re
from pathlib import Path

GAME = Path(__file__).resolve().parents[1] / "crates/sunrise-client/src/game"

for name in (
    "modding/mod_container_openers_tests.rs",
    "network/lan_actions/packets.rs",
    "network/lan_crafting.rs",
    "network/lan_mount_menu.rs",
    "network/lan_stations.rs",
    "network/lan_trading.rs",
):
    path = GAME / name
    source = path.read_text()
    result, count = re.subn(r"\.player_menu_closing\((conn|12|13)\);", r".player_menu_closing(\1, true);", source)
    if count:
        path.write_text(result)
