#!/usr/bin/env python3
"""Rewrite the literal registered-atlas sprite names on the P3-73 base."""
from pathlib import Path
import re
import subprocess


def encode(value):
    return "x" + value.replace(".", "..").replace("/", ".s")


def main():
    paths = subprocess.check_output(
        ["git", "grep", "-l", "gui/atlases/", "--", "crates", "docs"], text=True
    ).splitlines()
    pattern = re.compile(r"(?<=gui/atlases/)([\w.-]+)/([\w.-]+)/((?:\{name\}|[\w./-])*)")
    for path in map(Path, paths):
        if path.suffix != ".rs" or "/src/" not in path.as_posix() or (
            "/tests/" not in path.as_posix()
            and path.name not in {"texture_atlases.rs", "hud_layout_items_tests.rs", "menu_pictures.rs"}
        ):
            continue
        old = path.read_text()

        def replace(match):
            atlas, namespace, sprite = match.groups()
            if not sprite:  # prefix used to construct a full path, rather than a name
                return "/".join((encode(atlas), encode(namespace), ""))
            stem, suffix = (sprite[:-4], ".png") if sprite.endswith(".png") else (sprite, "")
            return "/".join((encode(atlas), encode(namespace), encode(stem))) + suffix

        new = pattern.sub(replace, old)
        if new != old:
            path.write_text(new)


if __name__ == "__main__":
    main()
