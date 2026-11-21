use super::{Hook, RegistrationAppend};
use serde_json::json;

#[test]
fn attributed_append_only_applies_to_global_non_answered_collectors() {
    let parse = |api| {
        Hook::parse(json!({
            "name": "attributed", "doc": "Doc.", "side": "both", "target": "crate::native_attributed",
            "mod_api": api,
        }))
    };
    assert_eq!(
        parse(json!({"contract":"collect", "out":"items", "registration_append":"attributed"}))
            .unwrap()
            .registration_append,
        Some(RegistrationAppend::Attributed)
    );
    for api in [
        json!({"contract":"listen", "registration_append":"attributed"}),
        json!({"contract":"collect", "out":"items", "scope":"item", "key":"stack", "registration_append":"attributed"}),
        json!({"contract":"collect", "out":"items", "answered_by":"mod", "registration_append":"attributed"}),
    ] {
        assert!(parse(api).is_err());
    }
}

#[test]
fn an_output_validator_is_a_path_on_an_isolated_hook_only() {
    let parse = |api| {
        Hook::parse(json!({
            "name": "checked", "doc": "Doc.", "side": "server", "target": "crate::native_checked",
            "mod_api": api,
        }))
    };
    assert_eq!(
        parse(json!({"contract":"cancel", "out":"canceled", "isolate":true, "output_validator":"crate::check"}))
            .unwrap()
            .output_validator
            .as_deref(),
        Some("crate::check")
    );
    for api in [
        json!({"contract":"cancel", "out":"canceled", "output_validator":"crate::check"}),
        json!({"contract":"cancel", "out":"canceled", "isolate":true, "output_validator":"not a path"}),
    ] {
        assert!(parse(api).is_err());
    }
}
