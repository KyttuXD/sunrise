#!/usr/bin/env python3
"""Remove callers and fixtures of the superseded raw native settings path."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GAME = "crates/sunrise-client/src/game"
GEN = "crates/sunrise-mod-api-gen"


def replace(path: str, old: str, new: str = "") -> None:
    file = ROOT / path
    text = file.read_text()
    if text.count(old) != 1:
        raise SystemExit(f"{path}: expected one occurrence of {old!r}")
    file.write_text(text.replace(old, new))


def remove_section(path: str, start: str, end: str) -> None:
    text = (ROOT / path).read_text()
    first = text.index(start)
    replace(path, text[first:text.index(end, first)])


remove_section(
    f"{GAME}/native_hooks/base.rs",
    "    /// A mod's setting `key` (one file per key in `native_data_dir`).",
    "    /// Shows the panel beside container screens",
)
remove_section(
    f"{GAME}/native_hooks/shared/system_helpers.rs",
    "    /// A native mod's own folder:",
    "    pub(in crate::game) fn mod_data_root()",
)
replace(
    f"{GAME}/modding/native_configs.rs",
    '                let _ = read.setting("phase_probe", "unused");\n',
)
replace(
    f"{GAME}/modding/mods/mod.rs",
    "        self.setting_overrides.borrow_mut().clear();\n",
)
replace(
    f"{GAME}.rs",
    "    setting_overrides: std::cell::RefCell<std::collections::BTreeMap<(String, String), String>>,\n",
)
replace(f"{GAME}.rs", "            setting_overrides: Default::default(),\n")
path = f"{GEN}/src/dispatch.rs"
text = (ROOT / path).read_text()
start = text.index('    #[test]\n    #[ignore = "daily"]\n    fn settings_boundary_owns_and_caches_values()')
replace(path, text[start:text.index('    #[test]\n', start + 1)])
for suffix in ("bool", "choice", "color", "f64"):
    replace(f"{GEN}/unwinding-methods.txt", f"config::setting_{suffix}\n")
replace(
    "tools/ci/ignored-tests/daily-sunrise-mod-api-gen.txt",
    "sunrise-mod-api-gen dispatch::tests::settings_boundary_owns_and_caches_values lib daily child-process\n",
)
world = "crates/sunrise-world/src/native_load_context.rs"
remove_section(world, "/// Read the same per-key storage", "#[cfg(test)]")
replace(world, '        assert_eq!(read_setting(&active, "mode").as_deref(), Some("after"));\n')
