use aria_underlay::api::AriaUnderlayService;
use aria_underlay::device::DeviceInventory;
use aria_underlay::ha::{ActiveLeaseConfig, ActiveLeaseGuard, ActiveLeaseRecord};
use aria_underlay::utils::time::now_unix_secs;
use aria_underlay::UnderlayError;

#[test]
fn active_lease_rejects_second_holder() {
    let path = temp_lease_path("second-holder");
    let _first = ActiveLeaseGuard::acquire(
        ActiveLeaseConfig::new(&path, "node-a").with_heartbeat_interval_secs(1),
    )
    .expect("first active lease holder should acquire lease");

    let err = ActiveLeaseGuard::acquire(
        ActiveLeaseConfig::new(&path, "node-b").with_heartbeat_interval_secs(1),
    )
    .expect_err("second active lease holder should be rejected");

    assert_adapter_code(err, "HA_LEASE_HELD");
    std::fs::remove_file(path).ok();
}

#[test]
fn active_lease_drop_releases_current_lease() {
    let path = temp_lease_path("drop-release");
    {
        let _lease = ActiveLeaseGuard::acquire(
            ActiveLeaseConfig::new(&path, "node-a").with_heartbeat_interval_secs(1),
        )
        .expect("active lease should be acquired");
    }

    assert!(
        !path.exists(),
        "dropping the current holder should remove the lease file"
    );
}

#[test]
fn active_lease_can_take_over_stale_record() {
    let path = temp_lease_path("stale-takeover");
    std::fs::create_dir_all(path.parent().expect("lease path should have parent"))
        .expect("temp lease dir should be created");
    let stale = ActiveLeaseRecord {
        owner_id: "node-a".into(),
        token: "stale-token".into(),
        acquired_at_unix_secs: now_unix_secs().saturating_sub(10),
        updated_at_unix_secs: now_unix_secs().saturating_sub(10),
    };
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&stale).expect("stale lease should serialize"),
    )
    .expect("stale lease should be written");

    let lease = ActiveLeaseGuard::acquire(
        ActiveLeaseConfig::new(&path, "node-b")
            .with_ttl_secs(1)
            .with_heartbeat_interval_secs(1),
    )
    .expect("stale active lease should be replaced");

    assert_eq!(lease.record().owner_id, "node-b");
    drop(lease);
    std::fs::remove_file(path).ok();
}

#[tokio::test]
async fn active_passive_activation_runs_recovery_and_holds_lease() {
    let path = temp_lease_path("service-activation");
    let service = AriaUnderlayService::new(DeviceInventory::default());

    let active = service
        .activate_active_passive(
            ActiveLeaseConfig::new(&path, "node-a").with_heartbeat_interval_secs(1),
        )
        .await
        .expect("service should become active when lease is free");

    assert_eq!(active.startup_recovery().pending, 0);

    let standby = AriaUnderlayService::new(DeviceInventory::default())
        .activate_active_passive(
            ActiveLeaseConfig::new(&path, "node-b").with_heartbeat_interval_secs(1),
        )
        .await
        .expect_err("standby service must not become active while lease is held");

    assert_adapter_code(standby, "HA_LEASE_HELD");
    drop(active);
    std::fs::remove_file(path).ok();
}

fn assert_adapter_code(err: UnderlayError, expected: &str) {
    match err {
        UnderlayError::AdapterOperation { code, .. } => assert_eq!(code, expected),
        other => panic!("unexpected error: {other:?}"),
    }
}

fn temp_lease_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir()
        .join(format!("aria-underlay-ha-{name}-{}", uuid::Uuid::new_v4()))
        .join("active.lock")
}
