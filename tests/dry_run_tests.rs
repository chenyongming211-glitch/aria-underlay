use std::collections::BTreeMap;

use aria_underlay::engine::diff::ChangeOp;
use aria_underlay::engine::dry_run::build_dry_run_plan;
use aria_underlay::model::{AdminState, DeviceId, InterfaceConfig, PortMode, VlanConfig};
use aria_underlay::planner::device_plan::DeviceDesiredState;
use aria_underlay::state::DeviceShadowState;

#[test]
fn dry_run_reports_noop_when_all_device_diffs_are_empty() {
    let desired = vec![desired_state("leaf-a", vec![vlan(100, "prod")])];
    let current = vec![shadow_state("leaf-a", vec![vlan(100, "prod")])];

    let plan = build_dry_run_plan(&desired, &current).expect("dry-run should build");

    assert!(plan.is_noop());
    assert_eq!(plan.change_sets.len(), 1);
    assert!(plan.change_sets[0].is_empty());
}

#[test]
fn dry_run_reports_per_device_change_sets() {
    let desired = vec![
        desired_state("leaf-a", vec![vlan(100, "prod")]),
        desired_state("leaf-b", vec![vlan(200, "backup")]),
    ];
    let current = vec![
        shadow_state("leaf-a", vec![]),
        shadow_state("leaf-b", vec![vlan(200, "old")]),
    ];

    let plan = build_dry_run_plan(&desired, &current).expect("dry-run should build");

    assert!(!plan.is_noop());
    assert_eq!(
        plan.change_sets[0].ops,
        vec![ChangeOp::CreateVlan(vlan(100, "prod"))]
    );
    assert_eq!(
        plan.change_sets[1].ops,
        vec![ChangeOp::UpdateVlan {
            before: vlan(200, "old"),
            after: vlan(200, "backup"),
        }]
    );
}

#[test]
fn dry_run_fails_when_current_state_is_missing() {
    let desired = vec![desired_state("leaf-a", vec![vlan(100, "prod")])];
    let current = vec![];

    let err = build_dry_run_plan(&desired, &current).unwrap_err();

    assert!(format!("{err}").contains("missing current state for device leaf-a"));
}

fn desired_state(device_id: &str, vlans: Vec<VlanConfig>) -> DeviceDesiredState {
    DeviceDesiredState {
        device_id: DeviceId(device_id.into()),
        vlans: vlans.into_iter().map(|vlan| (vlan.vlan_id, vlan)).collect(),
        interfaces: BTreeMap::from([(
            "GE1/0/1".into(),
            InterfaceConfig {
                name: "GE1/0/1".into(),
                admin_state: AdminState::Up,
                description: None,
                mode: PortMode::Access { vlan_id: 100 },
            },
        )]),
    }
}

fn shadow_state(device_id: &str, vlans: Vec<VlanConfig>) -> DeviceShadowState {
    DeviceShadowState {
        device_id: DeviceId(device_id.into()),
        revision: 1,
        vlans: vlans.into_iter().map(|vlan| (vlan.vlan_id, vlan)).collect(),
        interfaces: BTreeMap::from([(
            "GE1/0/1".into(),
            InterfaceConfig {
                name: "GE1/0/1".into(),
                admin_state: AdminState::Up,
                description: None,
                mode: PortMode::Access { vlan_id: 100 },
            },
        )]),
        warnings: vec![],
    }
}

fn vlan(vlan_id: u16, name: &str) -> VlanConfig {
    VlanConfig {
        vlan_id,
        name: Some(name.into()),
        description: None,
    }
}
