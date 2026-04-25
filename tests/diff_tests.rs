use aria_underlay::engine::diff::ChangeSet;
use aria_underlay::model::DeviceId;

#[test]
fn empty_change_set_is_noop() {
    let change_set = ChangeSet::empty(DeviceId("leaf-a".into()));
    assert!(change_set.is_empty());
}

