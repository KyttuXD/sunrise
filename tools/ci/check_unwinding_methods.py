#!/usr/bin/env python3
"""The list of gated Host methods that can only refuse by unwinding only shrinks (#3123).

crates/sunrise-mod-api-gen/unwinding-methods.txt names each `#[access]` method without a typed refusal or
optional callback. The generator checks exact `area::method` locations against that list. This check
allows an existing generated game method to move between areas, but never admits another occurrence or a
new method name. A base that cannot be read is an error, never a new list.

    python3 tools/ci/check_unwinding_methods.py BASE_SHA
"""
import subprocess
import sys
from collections import Counter

LIST = "crates/sunrise-mod-api-gen/unwinding-methods.txt"


def lines(text):
    return {line for line in text.splitlines() if line and not line.startswith("#")}


def git(*args):
    done = subprocess.run(["git", *args], capture_output=True, text=True)
    if done.returncode != 0:
        sys.exit(f"git {' '.join(args)} failed, so the base list cannot be read:\n{done.stderr}")
    return done.stdout


def main():
    base = sys.argv[1]
    git("cat-file", "-e", f"{base}^{{tree}}")
    # An existing base tree without the list means the list is new in this change.
    if not git("ls-tree", base, "--", LIST):
        return
    before = lines(git("show", f"{base}:{LIST}"))
    with open(LIST) as current:
        after = lines(current.read())
    # Forwarders still call the same Game method by name when its declaring area moves.
    removed = Counter(line.split("::", 1)[1] for line in before - after)
    added = []
    for line in sorted(after - before):
        name = line.split("::", 1)[1]
        if removed[name]:
            removed[name] -= 1
        else:
            added.append(line)
    if added:
        sys.exit(f"{LIST} only shrinks; these methods were added since the base:\n" + "\n".join(added))


if __name__ == "__main__":
    main()
