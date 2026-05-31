use aria_underlay::api::request::ApplyOptions;

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
