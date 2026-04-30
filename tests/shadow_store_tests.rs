use std::collections::BTreeMap;
use std::sync::Arc;

use aria_underlay::model::{DeviceId, VlanConfig};
use aria_underlay::state::{
    DeviceShadowState, InMemoryShadowStateStore, JsonFileShadowStateStore, ShadowStateStore,
};

#[test]
fn in_memory_shadow_store_put_get_and_revision_increment() {
    let store = InMemoryShadowStateStore::default();
    let first = shadow_state("leaf-a", 100);

    let stored = store.put(first).expect("shadow put should succeed");
    assert_eq!(stored.revision, 1);

    let second = shadow_state("leaf-a", 200);
    let stored = store.put(second).expect("shadow update should succeed");
    assert_eq!(stored.revision, 2);

    let loaded = store
        .get(&DeviceId("leaf-a".into()))
        .expect("shadow get should succeed")
        .expect("shadow should exist");
    assert_eq!(loaded.revision, 2);
    assert!(loaded.vlans.contains_key(&200));
}

#[test]
fn in_memory_shadow_store_lists_states_in_device_order() {
    let store = InMemoryShadowStateStore::default();
    store
        .put(shadow_state("leaf-b", 200))
        .expect("shadow put should succeed");
    store
        .put(shadow_state("leaf-a", 100))
        .expect("shadow put should succeed");

    let states = store.list().expect("shadow list should succeed");

    assert_eq!(states[0].device_id.0, "leaf-a");
    assert_eq!(states[1].device_id.0, "leaf-b");
}

#[test]
fn file_shadow_store_round_trips_across_store_recreation() {
    let root = temp_shadow_dir("round-trip");
    let store = JsonFileShadowStateStore::new(&root);

    let stored = store
        .put(shadow_state("leaf-a", 100))
        .expect("file shadow put should succeed");
    assert_eq!(stored.revision, 1);

    let recreated = JsonFileShadowStateStore::new(&root);
    let loaded = recreated
        .get(&DeviceId("leaf-a".into()))
        .expect("file shadow get should succeed")
        .expect("file shadow should exist after store recreation");

    assert_eq!(loaded.device_id, DeviceId("leaf-a".into()));
    assert_eq!(loaded.revision, 1);
    assert!(loaded.vlans.contains_key(&100));

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn file_shadow_store_increments_revision_across_store_recreation() {
    let root = temp_shadow_dir("revision");
    JsonFileShadowStateStore::new(&root)
        .put(shadow_state("leaf-a", 100))
        .expect("first file shadow put should succeed");

    let recreated = JsonFileShadowStateStore::new(&root);
    let updated = recreated
        .put(shadow_state("leaf-a", 200))
        .expect("second file shadow put should succeed");

    assert_eq!(updated.revision, 2);
    assert!(updated.vlans.contains_key(&200));

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn file_shadow_store_lists_states_in_device_order_after_recreation() {
    let root = temp_shadow_dir("list");
    let store = JsonFileShadowStateStore::new(&root);
    store
        .put(shadow_state("leaf-b", 200))
        .expect("leaf-b file shadow put should succeed");
    store
        .put(shadow_state("leaf-a", 100))
        .expect("leaf-a file shadow put should succeed");

    let states = JsonFileShadowStateStore::new(&root)
        .list()
        .expect("file shadow list should succeed");

    assert_eq!(states.len(), 2);
    assert_eq!(states[0].device_id.0, "leaf-a");
    assert_eq!(states[1].device_id.0, "leaf-b");

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn file_shadow_store_removes_state() {
    let root = temp_shadow_dir("remove");
    let store = JsonFileShadowStateStore::new(&root);
    store
        .put(shadow_state("leaf-a", 100))
        .expect("file shadow put should succeed");

    let removed = store
        .remove(&DeviceId("leaf-a".into()))
        .expect("file shadow remove should succeed")
        .expect("removed file shadow should be returned");

    assert_eq!(removed.device_id, DeviceId("leaf-a".into()));
    assert!(store
        .get(&DeviceId("leaf-a".into()))
        .expect("file shadow get should succeed")
        .is_none());

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn file_shadow_store_rejects_non_canonical_device_id() {
    let root = temp_shadow_dir("reject-invalid-id");
    let store = JsonFileShadowStateStore::new(&root);

    let err = store
        .put(shadow_state("../bad/device", 100))
        .expect_err("file shadow put should reject non-canonical device id");

    assert!(format!("{err}").contains("device_id"));
    assert!(!root.join("___bad_device.json").exists());

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn file_shadow_store_serializes_concurrent_same_device_writes() {
    let root = temp_shadow_dir("concurrent");
    let store = Arc::new(JsonFileShadowStateStore::new(&root));

    let writers = (1..=24)
        .map(|vlan_id| {
            let store = store.clone();
            std::thread::spawn(move || {
                store
                    .put(shadow_state("leaf-a", vlan_id))
                    .expect("concurrent file shadow put should succeed");
            })
        })
        .collect::<Vec<_>>();

    for writer in writers {
        writer
            .join()
            .expect("shadow writer thread should not panic");
    }

    let loaded = store
        .get(&DeviceId("leaf-a".into()))
        .expect("file shadow get should succeed")
        .expect("file shadow should exist");
    assert_eq!(loaded.revision, 24);
    assert!(
        std::fs::read_dir(&root)
            .expect("shadow root should be readable")
            .all(|entry| !entry
                .expect("shadow entry should be readable")
                .path()
                .to_string_lossy()
                .ends_with(".tmp"))
    );

    std::fs::remove_dir_all(root).ok();
}

fn shadow_state(device_id: &str, vlan_id: u16) -> DeviceShadowState {
    DeviceShadowState {
        device_id: DeviceId(device_id.into()),
        revision: 0,
        vlans: BTreeMap::from([(
            vlan_id,
            VlanConfig {
                vlan_id,
                name: Some(format!("vlan-{vlan_id}")),
                description: None,
            },
        )]),
        interfaces: BTreeMap::new(),
        warnings: Vec::new(),
    }
}

fn temp_shadow_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("aria-underlay-shadow-{name}-{}", uuid::Uuid::new_v4()))
}
