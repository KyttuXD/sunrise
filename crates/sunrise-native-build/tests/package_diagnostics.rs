use std::fs;
use sunrise_native_build::{NATIVE_MANIFEST, bundle_files, bundle_in, host};

#[test]
fn native_state_refusal_is_localized_during_discovery_and_staging() {
    // This integration test has its own process, so installing a host cannot affect other tests.
    host::install(host::Host {
        tr: |en| match en {
            "E_MIXIN_STATE_LAYOUT: {mod}:{state}: native state changes the prebuilt game's layout; use an attachment instead" => {
                "E_MIXIN_STATE_LAYOUT: Feld {state} im Mod {mod} wird nicht unterstützt"
            }
            _ => en,
        },
        root: std::env::temp_dir,
        get: |_| Err("this test has no network".into()),
        mark_used: |_| Ok(()),
    });

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='stateful'\nversion='0.1.0'\n",
    )
    .unwrap();
    let path = root.join(NATIVE_MANIFEST);
    let valid = "id='stateful'\nname='Stateful'\nversion='0.1.0'\n[native]\ngame='26.3'\nsunrise='aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'\n";
    fs::write(&path, valid).unwrap();
    let bundle = bundle_in(root).unwrap().unwrap();

    fs::write(&path, format!("{valid}[[native.state]]\nid='count'\n")).unwrap();
    let expected = "E_MIXIN_STATE_LAYOUT: Feld count im Mod stateful wird nicht unterstützt";
    assert_eq!(bundle_in(root).unwrap_err(), expected);
    assert_eq!(bundle_files(&bundle).unwrap_err(), expected);
}
