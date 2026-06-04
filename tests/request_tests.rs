use aria_underlay::api::request::{ApplyLockScope, ApplyOptions};

#[test]
fn apply_options_default_to_domain_lock_scope() {
    let options = ApplyOptions::default();

    assert_eq!(options.lock_scope, ApplyLockScope::Domain);
    assert!(options.region_id.is_none());
}

#[test]
fn apply_options_parse_region_lock_scope() {
    let options = serde_json::from_str::<ApplyOptions>(
        r#"{
            "dry_run": false,
            "allow_degraded_atomicity": false,
            "lock_scope": "Region",
            "region_id": "region-a"
        }"#,
    )
    .expect("region lock scope should parse");

    assert_eq!(options.lock_scope, ApplyLockScope::Region);
    assert_eq!(options.region_id.as_deref(), Some("region-a"));
}

#[test]
fn apply_options_reject_legacy_full_replace_field() {
    let err = serde_json::from_str::<ApplyOptions>(
        r#"{
            "dry_run": false,
            "allow_degraded_atomicity": false,
            "reconcile_mode": "full_replace"
        }"#,
    )
    .expect_err("legacy full_replace mode must fail closed");

    assert!(err.to_string().contains("unknown field `reconcile_mode`"));
}
