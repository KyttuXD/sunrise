#!/usr/bin/env python3
"""Assert successful command-tree decoding in existing wire fixtures."""

from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
CALL_SITES = (
    "crates/sunrise-command/src/network.rs",
    "crates/sunrise-client/src/lan/commands.rs",
    "crates/sunrise-client/src/lan/hub.rs",
)
CALL = re.compile(r"(\bfrom_packet\((?![^\n]*\.unwrap)[^\n]*\))(?=;?$)", re.MULTILINE)


def main() -> None:
    for name in CALL_SITES:
        path = ROOT / name
        text = path.read_text()
        path.write_text(CALL.sub(r"\1.unwrap()", text))


if __name__ == "__main__":
    main()
