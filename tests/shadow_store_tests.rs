use std::collections::BTreeMap;

use aria_underlay::model::{DeviceId, VlanConfig};
use aria_underlay::state::{
    DeviceShadowState, InMemoryShadowStateStore, ShadowStateStore,
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
