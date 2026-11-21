#!/usr/bin/env python3
"""Prepare command source readers for an owned, cloneable output destination."""

from pathlib import Path
import re


game = Path(__file__).resolve().parents[1] / "crates/sunrise-client/src/game"
for path in sorted(game.rglob("*.rs")):
    old = path.read_text()
    new = re.sub(
        r"(\bcommand_source)(\s*\.)(and_then|is_some_and|map_or|map)\(",
        r"\1.as_ref()\2\3(",
        old,
    )
    new = re.sub(
        r"(\bcommand_source)(\s*\.)unwrap_or_else\(\|\| self\.player_source\(\)\)",
        r"\1.clone()\2unwrap_or_else(|| self.player_source())",
        new,
    )
    new = new.replace("match self.command_source {", "match self.command_source.as_ref() {")
    new = new.replace("match self.g.command_source {", "match self.g.command_source.as_ref() {")
    new = new.replace("(s.kind, self.command_source)", "(s.kind, self.command_source.as_ref())")
    new = new.replace("source: self.command_source,", "source: self.command_source.clone(),")
    new = new.replace("let previous = g.command_source;", "let previous = g.command_source.clone();")
    new = new.replace(
        "let previous = game.command_source;", "let previous = game.command_source.take();"
    )
    new = new.replace("game.command_source.unwrap()", "game.command_source.as_ref().unwrap()")
    if old != new:
        path.write_text(new)
