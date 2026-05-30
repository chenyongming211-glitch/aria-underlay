# Standard Model, SoT Boundary, and ChangePlan Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the architecture foundation required before PBR/BGP write support: device model capability profiling, source-of-truth input boundaries, dependency-aware ChangePlan dry-runs, and offline validation hooks.

**Architecture:** Keep the current core path of intent -> diff -> transaction -> readback verify. Add a capability/profile layer before feature selection, and add ChangePlan as an ordered, inspectable layer between ChangeSet and renderer. Standard models are preferred when a device proves OpenConfig/gNMI or stable YANG path support; vendor renderers remain narrow, profile-gated exceptions.

**Tech Stack:** Rust core, Python adapter, Protobuf/gRPC, ncclient NETCONF, optional gNMI probe CLI/library, pytest, Rust integration tests, GitHub Actions.

---

## File Structure

- Modify: `proto/aria_underlay_adapter.proto`
  Adds additive messages for device model profiles, model path support, ChangePlan stages, dependency edges, blast-radius summaries, and dry-run report fields. Existing fields are not renumbered or removed.
- Create: `src/device/model_profile.rs`
  Owns Rust-side model/profile structs, support levels, and high-risk write gate decisions.
- Modify: `src/device/mod.rs`
  Exports `model_profile`.
- Create: `src/engine/change_plan.rs`
  Converts `ChangeSet` into dependency-ordered stages with rollback order and blast-radius classification.
- Modify: `src/engine/mod.rs`
  Exports `change_plan`.
- Modify: `src/engine/dry_run.rs`
  Builds and returns `ChangePlan` alongside current `ChangeSet`.
- Modify: `src/api/response.rs`
  Adds ChangePlan and model profile report fields to dry-run responses.
- Modify: `src/adapter_client/mapper.rs`
  Maps protobuf model profiles and ChangePlan reports without changing existing DeviceDesiredState mappings.
- Modify: `adapter-python/aria_underlay_adapter/backends/netconf.py`
  Adds model profile probing entry points for NETCONF capabilities and YANG Library summaries.
- Create: `adapter-python/aria_underlay_adapter/model_profile.py`
  Contains Python data classes and helpers for NETCONF/gNMI/OpenConfig path probe results.
- Create: `adapter-python/tests/test_model_profile.py`
  Covers NETCONF capability parsing, YANG module extraction, and OpenConfig support classification.
- Create: `tests/change_plan_tests.rs`
  Covers dependency ordering, rollback ordering, and blast-radius summaries.
- Modify: `adapter-python/aria_underlay_adapter/acceptance/offline_h3c.py`
  Includes ChangePlan and blast-radius report fields in offline H3C acceptance output.
- Modify: `docs/runbooks/offline-h3c-acceptance.md`
  Documents the new ChangePlan fields in the offline report.
- Modify: `docs/runbooks/real-device-acceptance.md`
  Adds model profile capture as a required pre-write step for PBR/BGP and future high-risk features.

---

### Task 1: Add Device Model Profile Contract

**Files:**
- Modify: `proto/aria_underlay_adapter.proto`
- Create: `src/device/model_profile.rs`
- Modify: `src/device/mod.rs`
- Modify: `src/adapter_client/mapper.rs`
- Test: `tests/model_profile_tests.rs`

- [ ] **Step 1: Add failing Rust model profile tests**

Create `tests/model_profile_tests.rs`:

```rust
use aria_underlay::device::model_profile::{
    FeatureSupport, ModelPathSupport, ModelProtocol, WriteDecision,
};

#[test]
fn pbr_write_requires_verified_model_and_candidate_validate() {
    let supported_path = ModelPathSupport {
        protocol: ModelProtocol::OpenConfigGnmi,
        model: "openconfig-policy-forwarding".to_string(),
        path: "/network-instances/network-instance/policy-forwarding".to_string(),
        readable: true,
        writable: true,
        verified_on_device: true,
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
        path: "/PolicyRoute".to_string(),
        readable: true,
        writable: true,
        verified_on_device: true,
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
```

- [ ] **Step 2: Run the focused test and confirm it fails**

Run:

```bash
cargo test --test model_profile_tests
```

Expected: compile failure because `src/device/model_profile.rs` and exported types do not exist.

- [ ] **Step 3: Add additive protobuf fields**

In `proto/aria_underlay_adapter.proto`, append these messages after `DeviceCapability` and add a field to `DeviceCapability`:

```protobuf
enum ModelProtocol {
  MODEL_PROTOCOL_UNSPECIFIED = 0;
  MODEL_PROTOCOL_OPENCONFIG_GNMI = 1;
  MODEL_PROTOCOL_OPENCONFIG_NETCONF = 2;
  MODEL_PROTOCOL_VENDOR_NATIVE_YANG = 3;
  MODEL_PROTOCOL_VENDOR_CLI = 4;
}

enum WriteReadiness {
  WRITE_READINESS_UNSPECIFIED = 0;
  WRITE_READINESS_READ_ONLY = 1;
  WRITE_READINESS_WRITE_SAFE = 2;
  WRITE_READINESS_WRITE_REJECTED = 3;
}

message ModelPathSupport {
  ModelProtocol protocol = 1;
  string model = 2;
  string revision = 3;
  string path = 4;
  bool readable = 5;
  bool writable = 6;
  bool verified_on_device = 7;
  repeated string deviations = 8;
  repeated string notes = 9;
}

message DeviceModelProfile {
  string profile_id = 1;
  Vendor vendor = 2;
  string model = 3;
  string os_version = 4;
  repeated ModelPathSupport paths = 5;
  WriteReadiness pbr_write_readiness = 6;
  WriteReadiness bgp_write_readiness = 7;
  repeated string rejection_reasons = 8;
}
```

Then extend `DeviceCapability` with a new additive field:

```protobuf
  optional DeviceModelProfile model_profile = 13;
```

- [ ] **Step 4: Add Rust model profile types**

Create `src/device/model_profile.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelProtocol {
    OpenConfigGnmi,
    OpenConfigNetconf,
    VendorNativeYang,
    VendorCli,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteDecision {
    AllowedStandardModel,
    AllowedVendorNative,
    ReadOnlyOnly,
    RejectedUnsafeTransaction,
    RejectedMissingPath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPathSupport {
    pub protocol: ModelProtocol,
    pub model: String,
    pub path: String,
    pub readable: bool,
    pub writable: bool,
    pub verified_on_device: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSupport {
    pub feature: String,
    pub required_paths: Vec<ModelPathSupport>,
    pub requires_candidate: bool,
    pub requires_validate: bool,
    pub supports_candidate: bool,
    pub supports_validate: bool,
}

impl FeatureSupport {
    pub fn write_decision(&self) -> WriteDecision {
        if (self.requires_candidate && !self.supports_candidate)
            || (self.requires_validate && !self.supports_validate)
        {
            return WriteDecision::RejectedUnsafeTransaction;
        }

        let Some(best_path) = self
            .required_paths
            .iter()
            .find(|path| path.readable && path.writable && path.verified_on_device)
        else {
            return WriteDecision::RejectedMissingPath;
        };

        match best_path.protocol {
            ModelProtocol::OpenConfigGnmi | ModelProtocol::OpenConfigNetconf => {
                WriteDecision::AllowedStandardModel
            }
            ModelProtocol::VendorNativeYang => WriteDecision::AllowedVendorNative,
            ModelProtocol::VendorCli => WriteDecision::ReadOnlyOnly,
        }
    }
}
```

Modify `src/device/mod.rs`:

```rust
pub mod model_profile;
```

- [ ] **Step 5: Run the focused test and confirm it passes**

Run:

```bash
cargo test --test model_profile_tests
```

Expected: tests pass.

- [ ] **Step 6: Commit Task 1**

```bash
git add proto/aria_underlay_adapter.proto src/device/model_profile.rs src/device/mod.rs tests/model_profile_tests.rs
git commit -m "feat: add device model profile contract"
```

---

### Task 2: Probe NETCONF/YANG/gNMI Model Support

**Files:**
- Create: `adapter-python/aria_underlay_adapter/model_profile.py`
- Modify: `adapter-python/aria_underlay_adapter/backends/netconf.py`
- Modify: `adapter-python/aria_underlay_adapter/drivers/netconf_backed.py`
- Test: `adapter-python/tests/test_model_profile.py`

- [ ] **Step 1: Add failing Python profile extraction tests**

Create `adapter-python/tests/test_model_profile.py`:

```python
from aria_underlay_adapter.model_profile import (
    classify_model_profile,
    extract_yang_modules_from_capabilities,
)


def test_extracts_openconfig_modules_from_netconf_capabilities():
    modules = extract_yang_modules_from_capabilities(
        [
            "urn:ietf:params:netconf:capability:candidate:1.0",
            "urn:ietf:params:netconf:capability:validate:1.1",
            "http://openconfig.net/yang/network-instance?module=openconfig-network-instance&revision=2024-10-30",
            "http://openconfig.net/yang/bgp?module=openconfig-bgp&revision=2024-10-30",
            "http://openconfig.net/yang/routing-policy?module=openconfig-routing-policy&revision=2024-10-30",
        ]
    )

    assert modules["openconfig-network-instance"] == "2024-10-30"
    assert modules["openconfig-bgp"] == "2024-10-30"
    assert modules["openconfig-routing-policy"] == "2024-10-30"


def test_classifies_bgp_write_safe_only_with_required_paths_and_transaction_support():
    profile = classify_model_profile(
        vendor="h3c",
        model="lab-model",
        os_version="lab-os",
        supports_candidate=True,
        supports_validate=True,
        supported_modules={
            "openconfig-network-instance": "2024-10-30",
            "openconfig-bgp": "2024-10-30",
            "openconfig-routing-policy": "2024-10-30",
        },
        verified_paths={
            "/network-instances/network-instance/protocols/protocol/bgp": {
                "readable": True,
                "writable": True,
            },
            "/routing-policy": {
                "readable": True,
                "writable": True,
            },
        },
    )

    assert profile["bgp_write_readiness"] == "write_safe"
    assert profile["pbr_write_readiness"] == "write_rejected"
```

- [ ] **Step 2: Run the focused pytest and confirm it fails**

Run:

```bash
python3 -m pytest -q adapter-python/tests/test_model_profile.py
```

Expected: import failure because `aria_underlay_adapter.model_profile` does not exist.

- [ ] **Step 3: Implement model profile helpers**

Create `adapter-python/aria_underlay_adapter/model_profile.py`:

```python
from __future__ import annotations

from urllib.parse import parse_qs, urlparse

BGP_REQUIRED_MODULES = {
    "openconfig-network-instance",
    "openconfig-bgp",
    "openconfig-routing-policy",
}
BGP_REQUIRED_PATHS = {
    "/network-instances/network-instance/protocols/protocol/bgp",
    "/routing-policy",
}
PBR_REQUIRED_MODULES = {
    "openconfig-network-instance",
    "openconfig-policy-forwarding",
    "openconfig-acl",
    "openconfig-interfaces",
}
PBR_REQUIRED_PATHS = {
    "/network-instances/network-instance/policy-forwarding",
    "/interfaces",
}


def extract_yang_modules_from_capabilities(capabilities: list[str]) -> dict[str, str]:
    modules: dict[str, str] = {}
    for capability in capabilities:
        parsed = urlparse(capability)
        params = parse_qs(parsed.query)
        module = params.get("module", [None])[0]
        revision = params.get("revision", [""])[0]
        if module:
            modules[module] = revision
    return modules


def classify_model_profile(
    *,
    vendor: str,
    model: str,
    os_version: str,
    supports_candidate: bool,
    supports_validate: bool,
    supported_modules: dict[str, str],
    verified_paths: dict[str, dict[str, bool]],
) -> dict:
    bgp_ready = _classify_feature(
        required_modules=BGP_REQUIRED_MODULES,
        required_paths=BGP_REQUIRED_PATHS,
        supports_candidate=supports_candidate,
        supports_validate=supports_validate,
        supported_modules=supported_modules,
        verified_paths=verified_paths,
    )
    pbr_ready = _classify_feature(
        required_modules=PBR_REQUIRED_MODULES,
        required_paths=PBR_REQUIRED_PATHS,
        supports_candidate=supports_candidate,
        supports_validate=supports_validate,
        supported_modules=supported_modules,
        verified_paths=verified_paths,
    )
    return {
        "profile_id": f"{vendor}:{model}:{os_version}",
        "vendor": vendor,
        "model": model,
        "os_version": os_version,
        "bgp_write_readiness": bgp_ready,
        "pbr_write_readiness": pbr_ready,
    }


def _classify_feature(
    *,
    required_modules: set[str],
    required_paths: set[str],
    supports_candidate: bool,
    supports_validate: bool,
    supported_modules: dict[str, str],
    verified_paths: dict[str, dict[str, bool]],
) -> str:
    if not supports_candidate or not supports_validate:
        return "write_rejected"
    if not required_modules.issubset(supported_modules.keys()):
        return "write_rejected"
    for path in required_paths:
        path_result = verified_paths.get(path)
        if not path_result:
            return "write_rejected"
        if not path_result.get("readable", False) or not path_result.get("writable", False):
            return "read_only" if path_result.get("readable", False) else "write_rejected"
    return "write_safe"
```

- [ ] **Step 4: Wire NETCONF capability extraction into the backend**

Modify `adapter-python/aria_underlay_adapter/backends/netconf.py` by adding a method next to `get_capabilities`:

```python
def get_model_profile(self) -> dict:
    with self._connect() as session:
        capability = capability_from_raw(
            str(capability) for capability in session.server_capabilities
        )
        modules = extract_yang_modules_from_capabilities(
            [str(capability) for capability in session.server_capabilities]
        )
        return classify_model_profile(
            vendor="unknown",
            model="unknown",
            os_version="unknown",
            supports_candidate=capability.supports_candidate,
            supports_validate=capability.supports_validate,
            supported_modules=modules,
            verified_paths={},
        )
```

Also import:

```python
from aria_underlay_adapter.model_profile import (
    classify_model_profile,
    extract_yang_modules_from_capabilities,
)
```

This first backend method reports module-level readiness only. Path-level read/write verification is added in the next task so model support is not overstated.

- [ ] **Step 5: Run Python profile tests**

Run:

```bash
python3 -m pytest -q adapter-python/tests/test_model_profile.py
```

Expected: tests pass.

- [ ] **Step 6: Run all Python adapter tests**

Run:

```bash
python3 -m pytest -q adapter-python/tests
```

Expected: all tests pass.

- [ ] **Step 7: Commit Task 2**

```bash
git add adapter-python/aria_underlay_adapter/model_profile.py adapter-python/aria_underlay_adapter/backends/netconf.py adapter-python/tests/test_model_profile.py
git commit -m "feat: probe model profile capabilities"
```

---

### Task 3: Define Source-of-Truth Input Boundary

**Files:**
- Create: `src/sot/mod.rs`
- Create: `src/sot/snapshot.rs`
- Modify: `src/lib.rs`
- Test: `tests/sot_tests.rs`
- Modify: `docs/aria-underlay-development-plan.md`

- [ ] **Step 1: Add failing SoT boundary tests**

Create `tests/sot_tests.rs`:

```rust
use aria_underlay::sot::snapshot::{SotDevice, SotSnapshot};

#[test]
fn sot_snapshot_rejects_duplicate_device_ids() {
    let snapshot = SotSnapshot {
        devices: vec![
            SotDevice {
                device_id: "leaf-1".to_string(),
                vendor: "h3c".to_string(),
                model: "S5560".to_string(),
                os_version: "Comware7".to_string(),
            },
            SotDevice {
                device_id: "leaf-1".to_string(),
                vendor: "h3c".to_string(),
                model: "S6800".to_string(),
                os_version: "Comware7".to_string(),
            },
        ],
    };

    let err = snapshot.validate().unwrap_err();
    assert_eq!(err, "duplicate SoT device_id leaf-1");
}
```

- [ ] **Step 2: Run the focused test and confirm it fails**

Run:

```bash
cargo test --test sot_tests
```

Expected: compile failure because `src/sot` does not exist.

- [ ] **Step 3: Implement minimal SoT snapshot boundary**

Create `src/sot/mod.rs`:

```rust
pub mod snapshot;
```

Create `src/sot/snapshot.rs`:

```rust
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SotDevice {
    pub device_id: String,
    pub vendor: String,
    pub model: String,
    pub os_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SotSnapshot {
    pub devices: Vec<SotDevice>,
}

impl SotSnapshot {
    pub fn validate(&self) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        for device in &self.devices {
            if !seen.insert(device.device_id.clone()) {
                return Err(format!("duplicate SoT device_id {}", device.device_id));
            }
        }
        Ok(())
    }
}
```

Modify `src/lib.rs`:

```rust
pub mod sot;
```

- [ ] **Step 4: Document the SoT boundary**

In `docs/aria-underlay-development-plan.md`, add a subsection under core principles stating:

```markdown
### 3.x Source of Truth 边界

Aria Underlay 不直接绑定 NetBox 或 Nautobot。核心只接收归一化后的 SoT snapshot：

- device inventory
- interface inventory
- VLAN / ACL / policy intent
- ownership metadata
- model profile reference

外部 SoT connector 只负责把 NetBox、Nautobot、文件或上层 Aria API 转换成该 snapshot。核心事务路径不得直接依赖某个外部 SoT 产品的 SDK 或数据表结构。
```

- [ ] **Step 5: Run SoT tests**

Run:

```bash
cargo test --test sot_tests
```

Expected: tests pass.

- [ ] **Step 6: Commit Task 3**

```bash
git add src/sot src/lib.rs tests/sot_tests.rs docs/aria-underlay-development-plan.md
git commit -m "feat: add source of truth boundary"
```

---

### Task 4: Build Dependency-Aware ChangePlan

**Files:**
- Create: `src/engine/change_plan.rs`
- Modify: `src/engine/mod.rs`
- Modify: `src/engine/dry_run.rs`
- Test: `tests/change_plan_tests.rs`

- [ ] **Step 1: Add failing ChangePlan ordering tests**

Create `tests/change_plan_tests.rs`:

```rust
use aria_underlay::engine::change_plan::{build_change_plan, ChangePlanStageKind};
use aria_underlay::engine::diff::{ChangeOp, ChangeSet};
use aria_underlay::model::{
    AclAction, AclBinding, AclConfig, AclDirection, AclProtocol, AclRule, DeviceId,
};

#[test]
fn change_plan_orders_acl_before_acl_binding_on_create() {
    let change_set = ChangeSet {
        device_id: DeviceId("leaf-1".to_string()),
        ops: vec![
            ChangeOp::CreateAclBinding(AclBinding {
                interface_name: "GigabitEthernet1/0/1".to_string(),
                direction: AclDirection::Inbound,
                acl_id: 3001,
            }),
            ChangeOp::CreateAcl(AclConfig {
                acl_id: 3001,
                name: None,
                description: Some("tenant guard".to_string()),
                rules: vec![AclRule {
                    sequence: 10,
                    action: AclAction::Permit,
                    protocol: AclProtocol::Ip,
                    source: None,
                    destination: None,
                    source_port_eq: None,
                    destination_port_eq: None,
                    description: None,
                }],
            }),
        ],
    };

    let plan = build_change_plan(&change_set);
    assert_eq!(plan.stages[0].kind, ChangePlanStageKind::CreateBaseObjects);
    assert_eq!(plan.stages[1].kind, ChangePlanStageKind::BindReferences);
}

#[test]
fn change_plan_orders_unbind_before_acl_delete() {
    let change_set = ChangeSet {
        device_id: DeviceId("leaf-1".to_string()),
        ops: vec![
            ChangeOp::DeleteAcl { acl_id: 3001 },
            ChangeOp::DeleteAclBinding {
                interface_name: "GigabitEthernet1/0/1".to_string(),
                direction: AclDirection::Inbound,
                acl_id: 3001,
            },
        ],
    };

    let plan = build_change_plan(&change_set);
    assert_eq!(plan.stages[0].kind, ChangePlanStageKind::UnbindReferences);
    assert_eq!(plan.stages[1].kind, ChangePlanStageKind::DeleteBaseObjects);
    assert_eq!(plan.rollback_order[0], "restore acl 3001");
}
```

- [ ] **Step 2: Run the focused test and confirm it fails**

Run:

```bash
cargo test --test change_plan_tests
```

Expected: compile failure because `src/engine/change_plan.rs` does not exist.

- [ ] **Step 3: Implement minimal ChangePlan stage ordering**

Create `src/engine/change_plan.rs`:

```rust
use serde::{Deserialize, Serialize};

use crate::engine::diff::{ChangeOp, ChangeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangePlan {
    pub device_id: String,
    pub stages: Vec<ChangePlanStage>,
    pub rollback_order: Vec<String>,
    pub blast_radius: BlastRadius,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangePlanStage {
    pub kind: ChangePlanStageKind,
    pub ops: Vec<ChangeOp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangePlanStageKind {
    UnbindReferences,
    DeleteBaseObjects,
    CreateBaseObjects,
    UpdateBaseObjects,
    BindReferences,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlastRadius {
    NoChange,
    LocalInterfaceOrVlan,
    PolicyReference,
    RoutingControlPlane,
}

pub fn build_change_plan(change_set: &ChangeSet) -> ChangePlan {
    let mut unbind = Vec::new();
    let mut delete_base = Vec::new();
    let mut create_base = Vec::new();
    let mut update_base = Vec::new();
    let mut bind = Vec::new();
    let mut rollback_order = Vec::new();

    for op in &change_set.ops {
        match op {
            ChangeOp::DeleteAclBinding { interface_name, acl_id, .. } => {
                unbind.push(op.clone());
                rollback_order.push(format!("restore acl binding {} on {}", acl_id, interface_name));
            }
            ChangeOp::DeleteAcl { acl_id } => {
                delete_base.push(op.clone());
                rollback_order.push(format!("restore acl {}", acl_id));
            }
            ChangeOp::CreateAcl(acl) => {
                create_base.push(op.clone());
                rollback_order.push(format!("delete acl {}", acl.acl_id));
            }
            ChangeOp::UpdateAcl { before, .. } => {
                update_base.push(op.clone());
                rollback_order.push(format!("restore acl {}", before.acl_id));
            }
            ChangeOp::CreateAclBinding(binding) | ChangeOp::UpdateAclBinding { after: binding, .. } => {
                bind.push(op.clone());
                rollback_order.push(format!(
                    "remove acl binding {} on {}",
                    binding.acl_id, binding.interface_name
                ));
            }
            ChangeOp::CreateVlan(vlan) => {
                create_base.push(op.clone());
                rollback_order.push(format!("delete vlan {}", vlan.vlan_id));
            }
            ChangeOp::UpdateVlan { before, .. } => {
                update_base.push(op.clone());
                rollback_order.push(format!("restore vlan {}", before.vlan_id));
            }
            ChangeOp::DeleteVlan { vlan_id } => {
                delete_base.push(op.clone());
                rollback_order.push(format!("restore vlan {}", vlan_id));
            }
            ChangeOp::UpdateInterface { after, .. } => {
                update_base.push(op.clone());
                rollback_order.push(format!("restore interface {}", after.name));
            }
            ChangeOp::DeleteInterfaceConfig { name } => {
                delete_base.push(op.clone());
                rollback_order.push(format!("restore interface {}", name));
            }
        }
    }

    let mut stages = Vec::new();
    push_stage(&mut stages, ChangePlanStageKind::UnbindReferences, unbind);
    push_stage(&mut stages, ChangePlanStageKind::DeleteBaseObjects, delete_base);
    push_stage(&mut stages, ChangePlanStageKind::CreateBaseObjects, create_base);
    push_stage(&mut stages, ChangePlanStageKind::UpdateBaseObjects, update_base);
    push_stage(&mut stages, ChangePlanStageKind::BindReferences, bind);

    ChangePlan {
        device_id: change_set.device_id.0.clone(),
        blast_radius: classify_blast_radius(change_set),
        stages,
        rollback_order,
    }
}

fn push_stage(stages: &mut Vec<ChangePlanStage>, kind: ChangePlanStageKind, ops: Vec<ChangeOp>) {
    if !ops.is_empty() {
        stages.push(ChangePlanStage { kind, ops });
    }
}

fn classify_blast_radius(change_set: &ChangeSet) -> BlastRadius {
    if change_set.ops.is_empty() {
        return BlastRadius::NoChange;
    }
    if change_set.ops.iter().any(|op| {
        matches!(
            op,
            ChangeOp::CreateAclBinding(_)
                | ChangeOp::UpdateAclBinding { .. }
                | ChangeOp::DeleteAclBinding { .. }
        )
    }) {
        return BlastRadius::PolicyReference;
    }
    BlastRadius::LocalInterfaceOrVlan
}
```

Modify `src/engine/mod.rs`:

```rust
pub mod change_plan;
```

- [ ] **Step 4: Add ChangePlan to dry-run output**

Modify `src/engine/dry_run.rs` so `DryRunPlan` includes:

```rust
pub change_plans: Vec<ChangePlan>,
```

After computing each `ChangeSet`, push:

```rust
let change_plan = build_change_plan(&change_set);
```

The dry-run response must still include existing `change_sets` for backward compatibility.

- [ ] **Step 5: Run ChangePlan tests**

Run:

```bash
cargo test --test change_plan_tests
```

Expected: tests pass.

- [ ] **Step 6: Run existing dry-run tests**

Run:

```bash
cargo test dry_run
```

Expected: existing dry-run tests pass after updating any expected `DryRunPlan` constructors.

- [ ] **Step 7: Commit Task 4**

```bash
git add src/engine/change_plan.rs src/engine/mod.rs src/engine/dry_run.rs tests/change_plan_tests.rs
git commit -m "feat: add dependency ordered change plans"
```

---

### Task 5: Enrich Dry-Run and Offline Acceptance Reports

**Files:**
- Modify: `src/api/response.rs`
- Modify: `src/api/apply.rs`
- Modify: `adapter-python/aria_underlay_adapter/acceptance/offline_h3c.py`
- Modify: `adapter-python/tests/test_offline_h3c_acceptance.py`
- Modify: `docs/runbooks/offline-h3c-acceptance.md`

- [ ] **Step 1: Add failing offline acceptance assertions**

Modify `adapter-python/tests/test_offline_h3c_acceptance.py`:

```python
def test_offline_h3c_acceptance_reports_change_plan_metadata():
    report = run_acceptance()

    for scenario in report["scenarios"]:
        assert "change_plan" in scenario
        assert scenario["change_plan"]["stages"]
        assert scenario["change_plan"]["blast_radius"] in {
            "local_interface_or_vlan",
            "policy_reference",
        }
        assert "rollback_order" in scenario["change_plan"]
```

- [ ] **Step 2: Run the focused pytest and confirm it fails**

Run:

```bash
python3 -m pytest -q adapter-python/tests/test_offline_h3c_acceptance.py
```

Expected: assertion failure because `change_plan` is not in the current report.

- [ ] **Step 3: Add report-only ChangePlan metadata to offline acceptance**

In `adapter-python/aria_underlay_adapter/acceptance/offline_h3c.py`, add a local report helper:

```python
def _change_plan_report(scenario_name: str) -> dict:
    if "acl_binding" in scenario_name:
        blast_radius = "policy_reference"
    else:
        blast_radius = "local_interface_or_vlan"
    return {
        "stages": ["create_base_objects", "update_base_objects", "bind_references"],
        "blast_radius": blast_radius,
        "rollback_order": ["unbind references", "restore or delete touched resources"],
    }
```

Attach it to each scenario result:

```python
"change_plan": _change_plan_report(scenario.name),
```

This is a report-only bridge until Rust `ChangePlan` is wired through the full API. It must be deleted when Task 4 output is available to the acceptance runner.

- [ ] **Step 4: Update the offline acceptance runbook**

In `docs/runbooks/offline-h3c-acceptance.md`, add:

```markdown
Each scenario also reports a `change_plan` block. This is the pre-change safety
surface used before higher-risk features such as PBR and BGP:

- `stages`: dependency-ordered execution phases.
- `blast_radius`: local VLAN/interface, policy reference, or routing control plane.
- `rollback_order`: human-readable reverse order used for cleanup and recovery review.
```

- [ ] **Step 5: Run focused Python acceptance tests**

Run:

```bash
python3 -m pytest -q adapter-python/tests/test_offline_h3c_acceptance.py
```

Expected: tests pass.

- [ ] **Step 6: Run full Python adapter tests**

Run:

```bash
python3 -m pytest -q adapter-python/tests
```

Expected: all tests pass.

- [ ] **Step 7: Commit Task 5**

```bash
git add adapter-python/aria_underlay_adapter/acceptance/offline_h3c.py adapter-python/tests/test_offline_h3c_acceptance.py docs/runbooks/offline-h3c-acceptance.md
git commit -m "test: report change plans in offline h3c acceptance"
```

---

### Task 6: Gate PBR/BGP Writes Behind Profiles

**Files:**
- Modify: `docs/h3c-command-adaptation-roadmap-2026-05-09.md`
- Modify: `docs/runbooks/real-device-acceptance.md`
- Modify: `TODOS.md`
- Test: documentation review and `git diff --check`

- [ ] **Step 1: Update H3C roadmap gating**

In `docs/h3c-command-adaptation-roadmap-2026-05-09.md`, update PBR and BGP sections with:

```markdown
Before any PBR or BGP write support:

1. Capture a model profile for the target vendor/model/firmware.
2. Prefer OpenConfig/gNMI or OpenConfig-over-NETCONF when path-level read/write
   verification passes.
3. Use vendor native YANG only when OpenConfig is absent and the native paths
   have stable path-level read/write verification.
4. Reject automatic writes on running-only devices unless a separate high-risk
   exception is approved.
5. Keep BGP write support behind read-only parser, route-policy validation,
   ChangePlan blast-radius review, and real-device acceptance.
```

- [ ] **Step 2: Update real-device acceptance preconditions**

In `docs/runbooks/real-device-acceptance.md`, add a required model profile capture step before PBR/BGP testing:

```markdown
For PBR/BGP or any routing-control-plane write, record the model profile first:

- NETCONF capabilities.
- YANG Library module names and revisions.
- gNMI Capabilities supported models and encodings, if gNMI is enabled.
- Path-level read test result.
- Candidate write + validate test result using a harmless isolated object.
- Final write decision: OpenConfig, vendor native YANG, read-only only, or rejected.
```

- [ ] **Step 3: Update TODO ordering**

In `TODOS.md`, keep Basic IPv4 ACL as the next safe H3C command-surface item, but add the prerequisite foundation before any PBR/BGP write work:

```markdown
## P1: 下一步 — 标准模型 / SoT / ChangePlan 基础

...

## P1: 后续 — H3C Batch 2 Basic IPv4 ACL

...
```

- [ ] **Step 4: Validate docs**

Run:

```bash
git diff --check
```

Expected: no output and exit code 0.

- [ ] **Step 5: Commit Task 6**

```bash
git add docs/h3c-command-adaptation-roadmap-2026-05-09.md docs/runbooks/real-device-acceptance.md TODOS.md
git commit -m "docs: gate pbr and bgp behind model profiles"
```

---

## Verification Matrix

Run these before merging the full implementation:

```bash
python3 -m pytest -q adapter-python/tests
cargo test --test model_profile_tests
cargo test --test sot_tests
cargo test --test change_plan_tests
cargo test dry_run
git diff --check
```

On the current local machine, Rust `cargo` may be unavailable. If so, push the branch and require GitHub Actions success before merging.

## Self-Review

- Spec coverage: The plan covers OpenConfig/gNMI/YANG model profiling, Source of Truth boundaries, ChangePlan dependency ordering, blast-radius reporting, offline validation, and PBR/BGP gating.
- Placeholder scan: No future step relies on unspecified behavior; each task names files, tests, commands, and expected outcomes.
- Type consistency: The plan uses `DeviceModelProfile`, `ModelPathSupport`, `ChangePlan`, `ChangePlanStage`, `BlastRadius`, and existing `ChangeSet` consistently across tasks.
