use aria_underlay::device::model_profile::{
    DeviceModelProfile, FeatureSupport, ModelPathSupport, ModelProtocol, WriteDecision,
    WriteReadiness,
};
use aria_underlay::model::Vendor;
use aria_underlay::proto::adapter;

#[test]
fn pbr_write_requires_verified_model_and_candidate_validate() {
    let supported_path = ModelPathSupport {
        protocol: ModelProtocol::OpenConfigGnmi,
        model: "openconfig-policy-forwarding".to_string(),
        revision: Some("2024-10-30".to_string()),
        path: "/network-instances/network-instance/policy-forwarding".to_string(),
        readable: true,
        writable: true,
        verified_on_device: true,
        deviations: vec![],
        notes: vec![],
    };

    let decision = FeatureSupport {
        feature: "pbr_write".to_string(),
        required_paths: vec![supported_path],
        requires_candidate: true,
        requires_validate: true,
        supports_candidate: true,
        supports_validate: true,
    }
    .write_decision();

    assert_eq!(decision, WriteDecision::AllowedStandardModel);
}

#[test]
fn pbr_write_is_rejected_when_only_running_write_is_available() {
    let native_path = ModelPathSupport {
        protocol: ModelProtocol::VendorNativeYang,
        model: "h3c-policy-routing".to_string(),
        revision: None,
        path: "/PolicyRoute".to_string(),
        readable: true,
        writable: true,
        verified_on_device: true,
        deviations: vec![],
        notes: vec!["device lacks candidate".to_string()],
    };

    let decision = FeatureSupport {
        feature: "pbr_write".to_string(),
        required_paths: vec![native_path],
        requires_candidate: true,
        requires_validate: true,
        supports_candidate: false,
        supports_validate: true,
    }
    .write_decision();

    assert_eq!(decision, WriteDecision::RejectedUnsafeTransaction);
}

#[test]
fn unverified_paths_are_not_write_safe() {
    let path = ModelPathSupport {
        protocol: ModelProtocol::OpenConfigNetconf,
        model: "openconfig-bgp".to_string(),
        revision: Some("2024-10-30".to_string()),
        path: "/network-instances/network-instance/protocols/protocol/bgp".to_string(),
        readable: true,
        writable: true,
        verified_on_device: false,
        deviations: vec![],
        notes: vec![],
    };

    let decision = FeatureSupport {
        feature: "bgp_write".to_string(),
        required_paths: vec![path],
        requires_candidate: true,
        requires_validate: true,
        supports_candidate: true,
        supports_validate: true,
    }
    .write_decision();

    assert_eq!(decision, WriteDecision::RejectedMissingPath);
}

#[test]
fn maps_proto_device_model_profile_into_rust_profile() {
    let proto_profile = adapter::DeviceModelProfile {
        profile_id: "h3c:S5560:Comware7".to_string(),
        vendor: adapter::Vendor::H3c as i32,
        model: "S5560".to_string(),
        os_version: "Comware7".to_string(),
        paths: vec![adapter::ModelPathSupport {
            protocol: adapter::ModelProtocol::OpenconfigGnmi as i32,
            model: "openconfig-bgp".to_string(),
            revision: "2024-10-30".to_string(),
            path: "/network-instances/network-instance/protocols/protocol/bgp".to_string(),
            readable: true,
            writable: false,
            verified_on_device: true,
            deviations: vec![],
            notes: vec!["readback only".to_string()],
        }],
        pbr_write_readiness: adapter::WriteReadiness::WriteRejected as i32,
        bgp_write_readiness: adapter::WriteReadiness::ReadOnly as i32,
        rejection_reasons: vec!["BGP path is not writable".to_string()],
    };

    let profile = DeviceModelProfile::from_proto(proto_profile);

    assert_eq!(profile.profile_id, "h3c:S5560:Comware7");
    assert_eq!(profile.vendor, Vendor::H3c);
    assert_eq!(profile.bgp_write_readiness, WriteReadiness::ReadOnly);
    assert_eq!(profile.pbr_write_readiness, WriteReadiness::WriteRejected);
    assert_eq!(profile.paths[0].protocol, ModelProtocol::OpenConfigGnmi);
    assert_eq!(profile.paths[0].revision.as_deref(), Some("2024-10-30"));
    assert_eq!(profile.rejection_reasons, vec!["BGP path is not writable"]);
}
