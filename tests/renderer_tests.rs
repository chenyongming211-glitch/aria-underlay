use aria_underlay::device::{renderer_for_vendor, DeviceConfigRenderer, HuaweiRenderer};
use aria_underlay::engine::diff::ChangeSet;
use aria_underlay::model::{DeviceId, Vendor};

#[test]
fn renderer_registry_selects_supported_vendor_renderer() {
    let renderer = renderer_for_vendor(Vendor::Huawei).expect("Huawei renderer should be selected");

    assert_eq!(renderer.vendor(), Vendor::Huawei);
}

#[test]
fn renderer_registry_rejects_unknown_vendor() {
    let error = renderer_for_vendor(Vendor::Unknown).expect_err("unknown vendor should fail");

    assert!(error.to_string().contains("unknown vendor"));
}

#[test]
fn vendor_renderer_skeleton_fails_closed_until_implemented() {
    let renderer = HuaweiRenderer;
    let change_set = ChangeSet::empty(DeviceId("leaf-a".into()));
    let error = renderer
        .render_change_set(&change_set)
        .expect_err("skeleton renderer should not produce production config");

    assert!(error.to_string().contains("not implemented yet"));
}
