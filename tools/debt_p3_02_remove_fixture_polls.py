#!/usr/bin/env python3
"""Remove the always-true poll callbacks from existing P3-02 fixtures."""

import re
import subprocess
from pathlib import Path

POLL = re.compile(
    r"^([ ]*)fn (?:entity_tick_listened|hears_projectile_impacts)\("
    r"[^{}]*\) \{\n\1    event\.(?:listened|hears) = true;\n\1\}\n\n?",
    re.MULTILINE,
)

for name in subprocess.check_output(
    ["git", "ls-files", "-z", "crates/sunrise-client/src", "crates/sunrise-hot-paths-game/src"]
).decode().split("\0"):
    if not name.endswith(".rs"):
        continue
    path = Path(name)
    old = path.read_text()
    new, count = POLL.subn("", old)
    if not count:
        continue
    for event in ("EntityTickListenedEvent", "HearsProjectileImpactsEvent"):
        if new.count(event) == 1:
            new = new.replace(event + ", ", "")
    path.write_text(new)
