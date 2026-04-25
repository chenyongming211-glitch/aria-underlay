use aria_underlay::worker::gc::RetentionPolicy;

#[test]
fn retention_policy_defaults_are_conservative() {
    let policy = RetentionPolicy::default();
    assert_eq!(policy.failed_journal_retention_days, 90);
    assert_eq!(policy.max_artifacts_per_device, 50);
}

