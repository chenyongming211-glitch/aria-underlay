# Aria Underlay 详细开发计划

本文档是 `aria-underlay` 的工程实施计划。需求基线见：

- [Aria Underlay 物理管控需求说明](./aria-underlay-requirements.md)
- [Aria Underlay 开发方案](./aria-underlay-development-plan.md)

## 1. 当前结论

项目采用脑手分离架构：

```text
Rust Underlay Core
    |
    | gRPC / Protobuf
    v
Python Underlay Adapter
    |
    +-- ncclient / NETCONF
    +-- NAPALM
    +-- Netmiko / SSH CLI
    |
    v
Physical Switches
```

Rust 负责：

- 设备纳管。
- inventory。
- 标准 intent。
- 单 endpoint 事务状态机。
- 多 endpoint 批量编排。
- transaction journal。
- drift auditor。
- lock strategy。
- GC。
- RFC-002 事件。
- RFC-015 审计。

Python 负责：

- 设备连接。
- capability probe。
- 厂商 driver。
- NETCONF / NAPALM / Netmiko 后端。
- 设备级 diff。
- rollback artifact。
- 单设备 prepare / commit / rollback / verify。

第一阶段只实现：

- 小规模 underlay 管控域，通常 1 到 2 个管理 endpoint，后续自然扩展到少量多 endpoint 场景。
- VLAN。
- interface description。
- access port。
- trunk port。
- confirmed-commit，如果设备支持。
- CLI / running fallback 的 best-effort rollback。
- drift auditor。
- lock retry。
- force unlock，默认关闭。
- journal / artifact GC。

当前代码仍保留 `SwitchPairIntent` 作为 MLAG 双管理 IP 场景的兼容入口。后续主模型应演进为 `UnderlayDomainIntent`，详见：

- [Underlay Domain 模型演进计划](./underlay-domain-model-plan.md)

## 2. 开发原则

### 2.1 Contract First

先稳定 `proto/aria_underlay_adapter.proto`，再写 Rust/Python 实现。

Rust 和 Python 之间不能传厂商 XML、CLI 命令或未结构化字符串作为主输入。

允许传：

```text
DesiredDeviceState
ChangeSet
DeviceRef
TransactionPhase
RollbackArtifactRef
AdapterError
```

不允许传：

```text
"vlan 100"
"<vlan><id>100</id></vlan>"
```

### 2.2 Transaction Owned by Rust

Python Adapter 可以执行单设备动作，但不能决定最终 operation 状态。

Rust 决定：

- 是否进入事务。
- 使用哪种事务策略。
- 是否允许降级。
- 是否 rollback。
- 是否进入 `InDoubt`。
- 最终 operation 状态。

### 2.3 Adapter Must Be Recoverable

Python Adapter 可以进程无状态，但事务产物不能无状态。

必须持久化：

- rollback artifact。
- running config backup。
- prepared tx metadata。
- backend result summary。

恢复 RPC 不能只传 `tx_id`。Rust journal 是恢复决策的唯一权威来源，调用 Adapter `Recover` 时必须同时传：

- `strategy`：原事务使用的下发策略，例如 confirmed-commit 或 candidate commit。
- `action`：Rust 根据 journal phase 得出的恢复动作，例如 discard prepared changes 或 adapter recover。

Adapter 只能执行该动作，不能自行推断最终事务语义。无法证明最终状态时必须返回 `InDoubt` 或结构化失败，不能返回成功。

### 2.4 Degraded Must Be Explicit

任何非 candidate / confirmed-commit 路径都必须显式标记 degraded。

返回结果必须包含：

- warning。
- degrade reason。
- backend kind。
- rollback artifact reference。

### 2.5 ACID Must Be Preserved

产品开发阶段必须把事务性作为硬要求，单 management endpoint 的配置下发必须满足 ACID 四个特性。

开发检查表：

- Atomicity：单 endpoint 内 prepare、commit、verify、finalize 不能半成功；失败必须 rollback / recover / InDoubt。
- Consistency：事务前后必须满足 intent validation、capability check、structured diff 和 post-commit verify。
- Isolation：同一 endpoint 只能有一个 writer；必须有 Rust 本地锁和设备侧 lock / 后端锁。
- Durability：事务开始后必须写 journal；rollback artifact、running backup、confirmed commit 恢复信息必须可恢复。

实现约束：

- 不允许把“adapter RPC 返回成功”直接当作最终事务成功。
- 不允许无 journal 的配置变更进入 running。
- 不允许 `InDoubt` 事务被 GC 自动删除。
- 不允许在降级模式中宣传强 ACID；必须明确说明削弱点和补偿方式。

## 3. 推荐目录结构

```text
aria-underlay/
├── Cargo.toml
├── build.rs
├── proto/
│   └── aria_underlay_adapter.proto
├── src/
│   ├── lib.rs
│   ├── api/
│   │   ├── mod.rs
│   │   ├── underlay_service.rs
│   │   ├── request.rs
│   │   ├── response.rs
│   │   └── force_unlock.rs
│   ├── intent/
│   │   ├── mod.rs
│   │   ├── switch_pair.rs
│   │   ├── vlan.rs
│   │   ├── interface.rs
│   │   └── validation.rs
│   ├── planner/
│   │   ├── mod.rs
│   │   └── device_plan.rs
│   ├── model/
│   │   ├── mod.rs
│   │   ├── common.rs
│   │   ├── vlan.rs
│   │   └── interface.rs
│   ├── proto/
│   │   └── mod.rs
│   ├── adapter_client/
│   │   ├── mod.rs
│   │   ├── client.rs
│   │   └── mapper.rs
│   ├── device/
│   │   ├── mod.rs
│   │   ├── info.rs
│   │   ├── registration.rs
│   │   ├── onboarding.rs
│   │   ├── inventory.rs
│   │   └── capability.rs
│   ├── engine/
│   │   ├── mod.rs
│   │   ├── normalize.rs
│   │   ├── diff.rs
│   │   └── dry_run.rs
│   ├── state/
│   │   ├── mod.rs
│   │   ├── shadow.rs
│   │   ├── drift.rs
│   │   └── snapshot.rs
│   ├── tx/
│   │   ├── mod.rs
│   │   ├── strategy.rs
│   │   ├── coordinator.rs
│   │   ├── candidate_commit.rs
│   │   ├── confirmed_commit.rs
│   │   ├── lock_strategy.rs
│   │   ├── journal.rs
│   │   └── recovery.rs
│   ├── worker/
│   │   ├── mod.rs
│   │   ├── drift_auditor.rs
│   │   └── gc.rs
│   ├── telemetry/
│   │   ├── mod.rs
│   │   ├── events.rs
│   │   ├── audit.rs
│   │   └── metrics.rs
│   └── utils/
│       ├── mod.rs
│       ├── retry.rs
│       └── time.rs
├── adapter-python/
│   ├── pyproject.toml
│   ├── aria_underlay_adapter/
│   │   ├── __init__.py
│   │   ├── server.py
│   │   ├── config.py
│   │   ├── errors.py
│   │   ├── proto/
│   │   ├── drivers/
│   │   │   ├── __init__.py
│   │   │   ├── base.py
│   │   │   ├── huawei.py
│   │   │   ├── h3c.py
│   │   │   ├── cisco.py
│   │   │   ├── ruijie.py
│   │   │   └── legacy_cli.py
│   │   ├── backends/
│   │   │   ├── __init__.py
│   │   │   ├── base.py
│   │   │   ├── netconf.py
│   │   │   ├── napalm_backend.py
│   │   │   └── netmiko_backend.py
│   │   ├── state.py
│   │   ├── normalize.py
│   │   ├── diff.py
│   │   ├── rollback.py
│   │   └── artifact_store.py
│   └── tests/
├── tests/
│   ├── proto_contract_tests.rs
│   ├── onboarding_tests.rs
│   ├── diff_tests.rs
│   ├── transaction_tests.rs
│   ├── recovery_tests.rs
│   ├── drift_tests.rs
│   └── gc_tests.rs
├── examples/
│   ├── capability_probe.rs
│   ├── register_device.rs
│   ├── create_vlan.rs
│   └── two_switch_transaction.rs
└── docs/
```

## 4. Protobuf 第一版设计

第一版 proto 只覆盖 VLAN 和接口基础能力。

### 4.1 Service

```proto
service UnderlayAdapter {
  rpc GetCapabilities(GetCapabilitiesRequest) returns (GetCapabilitiesResponse);
  rpc GetCurrentState(GetCurrentStateRequest) returns (GetCurrentStateResponse);
  rpc DryRun(DryRunRequest) returns (DryRunResponse);
  rpc Prepare(PrepareRequest) returns (PrepareResponse);
  rpc Commit(CommitRequest) returns (CommitResponse);
  rpc Rollback(RollbackRequest) returns (RollbackResponse);
  rpc Verify(VerifyRequest) returns (VerifyResponse);
  rpc Recover(RecoverRequest) returns (RecoverResponse);
  rpc ForceUnlock(ForceUnlockRequest) returns (ForceUnlockResponse);
}
```

### 4.2 必须包含的枚举

```proto
enum Vendor {
  VENDOR_UNSPECIFIED = 0;
  VENDOR_HUAWEI = 1;
  VENDOR_H3C = 2;
  VENDOR_CISCO = 3;
  VENDOR_RUIJIE = 4;
  VENDOR_UNKNOWN = 100;
}

enum BackendKind {
  BACKEND_KIND_UNSPECIFIED = 0;
  BACKEND_KIND_NETCONF = 1;
  BACKEND_KIND_NAPALM = 2;
  BACKEND_KIND_NETMIKO = 3;
  BACKEND_KIND_CLI = 4;
}

enum TransactionStrategy {
  TRANSACTION_STRATEGY_UNSPECIFIED = 0;
  TRANSACTION_STRATEGY_CONFIRMED_COMMIT = 1;
  TRANSACTION_STRATEGY_CANDIDATE_COMMIT = 2;
  TRANSACTION_STRATEGY_RUNNING_ROLLBACK_ON_ERROR = 3;
  TRANSACTION_STRATEGY_BEST_EFFORT_CLI = 4;
  TRANSACTION_STRATEGY_UNSUPPORTED = 100;
}

enum AdapterOperationStatus {
  ADAPTER_OPERATION_STATUS_UNSPECIFIED = 0;
  ADAPTER_OPERATION_STATUS_NO_CHANGE = 1;
  ADAPTER_OPERATION_STATUS_PREPARED = 2;
  ADAPTER_OPERATION_STATUS_COMMITTED = 3;
  ADAPTER_OPERATION_STATUS_ROLLED_BACK = 4;
  ADAPTER_OPERATION_STATUS_FAILED = 5;
  ADAPTER_OPERATION_STATUS_IN_DOUBT = 6;
  ADAPTER_OPERATION_STATUS_CONFIRMED_COMMIT_PENDING = 7;
}

enum RecoveryAction {
  RECOVERY_ACTION_UNSPECIFIED = 0;
  RECOVERY_ACTION_DISCARD_PREPARED_CHANGES = 1;
  RECOVERY_ACTION_ADAPTER_RECOVER = 2;
}
```

### 4.3 关键 message

```proto
message RequestContext {
  string request_id = 1;
  string tx_id = 2;
  string trace_id = 3;
  string tenant_id = 4;
  string site_id = 5;
}

message DeviceRef {
  string device_id = 1;
  string management_ip = 2;
  uint32 management_port = 3;
  Vendor vendor_hint = 4;
  string model_hint = 5;
  string secret_ref = 6;
}

message DeviceCapability {
  Vendor vendor = 1;
  string model = 2;
  string os_version = 3;
  repeated string raw_capabilities = 4;
  bool supports_netconf = 5;
  bool supports_candidate = 6;
  bool supports_validate = 7;
  bool supports_confirmed_commit = 8;
  bool supports_persist_id = 9;
  bool supports_rollback_on_error = 10;
  bool supports_writable_running = 11;
  repeated BackendKind supported_backends = 12;
}

message VlanConfig {
  uint32 vlan_id = 1;
  optional string name = 2;
  optional string description = 3;
}

message InterfaceConfig {
  string name = 1;
  AdminState admin_state = 2;
  optional string description = 3;
  PortMode mode = 4;
}

message DesiredDeviceState {
  string device_id = 1;
  repeated VlanConfig vlans = 2;
  repeated InterfaceConfig interfaces = 3;
}

message RollbackArtifactRef {
  string artifact_id = 1;
  string tx_id = 2;
  string device_id = 3;
  string storage_uri = 4;
  string checksum = 5;
}

message AdapterError {
  string code = 1;
  string message = 2;
  string normalized_error = 3;
  string raw_error_summary = 4;
  bool retryable = 5;
}

message RecoverRequest {
  RequestContext context = 1;
  DeviceRef device = 2;
  TransactionStrategy strategy = 3;
  RecoveryAction action = 4;
}
```

字段编号在第一版确定后不要随意重排。后续新增字段只追加。

## 5. Rust Core 第一批模块

### 5.1 `api`

第一批类型：

- `UnderlayService`
- `RegisterDeviceRequest`
- `RegisterDeviceResponse`
- `ApplyIntentRequest`
- `ApplyIntentResponse`
- `DryRunResponse`
- `RefreshStateRequest`
- `RecoveryReport`
- `ForceUnlockRequest`
- `DriftAuditRequest`

第一阶段 `UnderlayService` 可以先用内存实现：

```text
InMemoryUnderlayService
```

后续再接 Aria Controller metadata store。

### 5.2 `device`

第一批类型：

- `DeviceInfo`
- `DeviceInventory`
- `DeviceLifecycleState`
- `DeviceCapabilityProfile`
- `DeviceRegistrationService`
- `DeviceOnboardingService`

第一版 inventory 可以用内存 `DashMap`，但接口要抽象。

产品初始化不能要求调用方手工串联 secret 创建、设备注册和 onboarding。

第一阶段需要在 `api` 或 `device` 层补一个高层入口：

```text
InitializeUnderlaySite / RegisterSwitchPair
```

该入口接收客户录入的交换机 A/B 管理 IP、NETCONF 端口、用户名、密码或私钥、角色和厂商提示，内部完成：

```text
create secret
  -> receive secret_ref
  -> register A/B into inventory
  -> trigger onboarding
  -> collect capability
  -> return site initialization status
```

明文凭据不得进入 inventory、journal 或审计内容。普通设备模型只保存 `secret_ref`。

必须支持状态流转：

```text
Pending -> Probing -> Ready
Pending -> Probing -> Degraded
Pending -> Probing -> Unsupported
Pending -> Probing -> Unreachable
Pending -> Probing -> AuthFailed
Ready -> Drifted
Drifted -> Ready
Ready -> Maintenance
```

### 5.3 `adapter_client`

职责：

- 封装 tonic gRPC client。
- 把 Rust model 映射到 Protobuf。
- 把 Adapter result 映射回 Rust result。
- 统一 timeout。
- 统一 retry。
- 统一 tracing fields。

第一批方法：

- `get_capabilities`
- `get_current_state`
- `dry_run`
- `prepare`
- `commit`
- `rollback`
- `verify`
- `recover`
- `force_unlock`

### 5.4 `tx`

第一批模块：

- `strategy.rs`
- `coordinator.rs`
- `journal.rs`
- `lock_strategy.rs`

先不要直接写完整 confirmed-commit。先把状态机和 Adapter 调用链打通。

第一版状态流：

```text
Started
Preparing
Prepared
Committing
Verifying
Committed
RollingBack
RolledBack
Failed
InDoubt
```

### 5.5 `worker`

第一批 worker：

- `drift_auditor.rs`
- `gc.rs`

可以先不做常驻 runtime，先实现可被测试调用的 `run_once()`。

## 6. Python Adapter 第一批模块

### 6.1 `server.py`

职责：

- 启动 gRPC server。
- 加载配置。
- 注册 service。
- 注入 driver registry。
- 注入 artifact store。

第一版可通过环境变量配置：

```text
ARIA_UNDERLAY_ADAPTER_LISTEN=127.0.0.1:50051
ARIA_UNDERLAY_ARTIFACT_DIR=/tmp/aria-underlay-adapter/artifacts
```

### 6.2 `drivers/base.py`

Driver 抽象：

```python
class DeviceDriver:
    def get_capabilities(self, device): ...
    def get_current_state(self, device, scope): ...
    def dry_run(self, device, desired_state): ...
    def prepare(self, tx_id, device, desired_state): ...
    def commit(self, tx_id, device): ...
    def rollback(self, tx_id, device): ...
    def verify(self, tx_id, device, desired_state): ...
    def recover(self, tx_id, device): ...
    def force_unlock(self, device, lock_owner, reason): ...
```

### 6.3 `backends/netconf.py`

第一批实现：

- connect。
- get server hello / capabilities。
- get-config。
- lock。
- unlock。
- edit-config。
- validate。
- commit。
- confirmed-commit。
- cancel-commit。
- discard-changes。
- kill-session，仅 force unlock 使用。

依赖优先使用：

```text
ncclient
```

### 6.4 `artifact_store.py`

职责：

- 保存 rollback artifact。
- 读取 rollback artifact。
- 删除 artifact。
- 计算 checksum。
- 按 tx_id / device_id 查询 artifact。

第一版本地文件路径：

```text
{artifact_dir}/{device_id}/{tx_id}/rollback.json
```

### 6.5 `diff.py`

职责：

- vendor-specific normalize 后比较。
- 判断 `NoChange` / `NeedChange`。
- 生成 device-level change summary。

第一版只支持：

- VLAN。
- interface description。
- access。
- trunk。

## 7. Sprint 0 详细任务

Sprint 0 目标：

```text
Rust 能注册设备
Rust 能通过 gRPC 调用 Python Adapter
Python Adapter 能返回 capability
Rust 能完成 onboarding 状态流转
```

### 7.1 文件任务

#### 7.1.1 `proto/aria_underlay_adapter.proto`

必须定义：

- `UnderlayAdapter` service。
- `GetCapabilities` RPC。
- `DeviceRef`。
- `DeviceCapability`。
- `AdapterError`。
- `BackendKind`。
- `Vendor`。

其余 RPC 可以先定义 message 占位，但不实现业务逻辑。

#### 7.1.2 `Cargo.toml`

第一批 Rust 依赖：

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
tonic = "0.12"
prost = "0.13"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
anyhow = "1"
tracing = "0.1"
uuid = { version = "1", features = ["v4", "serde"] }
dashmap = "6"
async-trait = "0.1"

[build-dependencies]
tonic-build = "0.12"
```

版本号建仓时可以根据实际可用版本调整。

#### 7.1.3 `build.rs`

生成 Rust gRPC stub。

#### 7.1.4 `src/device/registration.rs`

实现：

- `RegisterDeviceRequest`。
- `RegisterDeviceResponse`。
- inventory insert。
- 初始状态 `Pending`。

#### 7.1.5 `src/device/onboarding.rs`

实现：

- `onboard_device(device_id)`。
- 状态切换 `Pending -> Probing`。
- 调用 adapter `GetCapabilities`。
- 根据结果写入 capability profile。
- 进入 `Ready` / `Degraded` / `Unsupported` / `Unreachable` / `AuthFailed`。

#### 7.1.6 `src/adapter_client/client.rs`

实现：

- gRPC connect。
- `get_capabilities`。
- timeout。
- error mapping。

#### 7.1.7 `adapter-python/pyproject.toml`

第一批 Python 依赖：

```toml
[project]
dependencies = [
  "grpcio",
  "grpcio-tools",
  "protobuf",
  "ncclient",
  "pydantic",
  "structlog",
]
```

NAPALM / Netmiko 可以 Sprint 1 加。

#### 7.1.8 `adapter-python/aria_underlay_adapter/server.py`

实现：

- gRPC server。
- `GetCapabilities`。
- 先支持 fake mode。
- 如果配置了真实设备，则调用 `NetconfBackend.get_capabilities`。

#### 7.1.9 `adapter-python/aria_underlay_adapter/backends/netconf.py`

实现最小 capability probe。

真实设备不可用时，测试使用 fake backend。

#### 7.1.10 `examples/register_device.rs`

实现：

- 构造设备注册请求。
- 调用 Rust service 注册设备。
- 打印 inventory 状态。

#### 7.1.11 `examples/capability_probe.rs`

实现：

- 注册设备。
- 触发 onboarding。
- 打印 capability profile。

### 7.2 Sprint 0 验收命令

Rust:

```bash
cargo check
cargo test
```

Python:

```bash
cd adapter-python
python -m pytest
```

本地联调：

```bash
cd adapter-python
python -m aria_underlay_adapter.server
```

另一个终端：

```bash
cargo run --example capability_probe
```

验收标准：

- Rust 能生成 proto stub。
- Python 能生成 proto stub。
- Python Adapter 能启动。
- Rust 能连上 Python Adapter。
- `GetCapabilities` fake backend 返回成功。
- 注册设备后状态为 `Pending`。
- onboarding 后状态为 `Ready` 或 `Degraded`。
- capability profile 被写入 inventory。

## 8. Sprint 1 详细任务

Sprint 1 目标：

```text
Python Adapter 打通 NETCONF 基础 RPC
Rust 能通过 Adapter 完成真实设备 capability probe
```

### 8.1 Python NETCONF backend

实现：

- SSH / NETCONF connect。
- get capabilities。
- get-config。
- lock。
- unlock。
- edit-config。
- validate。
- commit。
- discard-changes。
- confirmed-commit。
- cancel-commit。

注意：

- 所有异常必须转成 `AdapterError`。
- 不把原始 XML 作为主要成功结果。
- raw error 只进入 `raw_error_summary`。

### 8.2 Capability Classification

Rust 侧实现策略选择：

```text
candidate + validate + confirmed-commit -> ConfirmedCommit
candidate + validate -> CandidateCommit
writable-running + rollback-on-error -> RunningRollbackOnError
CLI fallback allowed -> BestEffortCli
otherwise -> Unsupported
```

### 8.3 验收标准

- 真实或 mock NETCONF 设备上 `GetCapabilities` 成功。
- 真实或 mock NETCONF 设备上 `get-config` 成功。
- lock / unlock 成功。
- capability classification 正确。
- 不支持 NETCONF 的 fake 设备返回 degraded fallback。

## 9. Sprint 2 详细任务

Sprint 2 目标：

```text
Rust 结构化模型成型
Adapter 能接收 DesiredDeviceState
```

任务：

- 定义 Rust `SwitchPairIntent`。
- 定义 Rust `DeviceDesiredState`。
- 定义 VLAN / interface model。
- 定义 Protobuf mapping。
- 实现 planner。
- 实现 intent validation。
- 实现 `DryRun` RPC 调用链。

验收：

- Rust 可以从 `SwitchPairIntent` 生成两台设备的 `DeviceDesiredState`。
- Rust 可以调用 Adapter `DryRun`。
- Adapter fake driver 返回 `NoChange` / `NeedChange`。

## 10. Sprint 3 详细任务

Sprint 3 目标：

```text
Python Driver 完成 VLAN / interface 的设备级 diff 和 renderer
```

任务：

- 实现 driver registry。
- 实现 Huawei 或 H3C 的第一个真实 driver。
- 实现 fake driver。
- 实现 VLAN normalize。
- 实现 interface normalize。
- 实现 dry-run diff。
- 实现 rollback artifact 生成。
- 实现 XML renderer 或 CLI renderer。

验收：

- 同一 desired state 重复 dry-run，第二次返回 `NoChange`。
- VLAN create/update/delete 能生成 change summary。
- interface description/access/trunk 能生成 change summary。
- 特殊字符能正确 escape。
- rollback artifact 能保存到本地。

## 11. Sprint 4 详细任务

Sprint 4 目标：

```text
Rust 单 endpoint CandidateCommit 跑通
```

任务：

- 实现 `TxCoordinator`。
- 实现 `TxJournalStore`。
- 实现 `LockAcquisitionPolicy`。
- 实现 Prepare 阶段。
- 实现 Commit 阶段。
- 实现 Rollback 阶段。
- 实现 journal recovery 基础扫描。

验收：

- 单 endpoint Prepare 成功后才进入 Commit。
- Prepare 失败不会 Commit。
- Validate 失败时本 endpoint running 不变。
- Journal 记录每个 phase。
- 进程重启后能扫描未完成 journal。
- Atomicity：任意 prepare/validate/commit 失败都不能静默成功。
- Isolation：同一 endpoint 并发 apply 时只能有一个事务进入写路径。
- Durability：事务开始后崩溃，重启后能从 journal 看见未完成事务。

## 12. Sprint 5 详细任务

Sprint 5 目标：

```text
ConfirmedCommit、Verify、FinalConfirm 和 InDoubt 处理跑通
```

任务：

- Adapter 实现 confirmed-commit。
- Adapter 实现 cancel-commit。
- Rust 实现 ConfirmedCommit。
- Rust 实现 post-commit verify。
- Rust 实现 final confirm。
- Rust 实现 final confirm timeout 后 get-current-state 判断。
- Rust 实现 `InDoubt`。

验收：

- confirmed 后 verify 失败时，执行 cancel。
- Final confirm timeout 后能判断 converged / in-doubt。
- `InDoubt` journal 不会被 GC。
- Consistency：final confirm 前必须完成 desired subset verify。
- Durability：confirmed commit 后进程崩溃，恢复流程能识别 pending / in-doubt。

### 12.1 P0 正确性与性能优化任务

在 Sprint 5 前后优先补齐 scoped state / scoped verify / 单次 edit-config。目标是在不牺牲正确性的前提下减少全量读取和重复 RPC。

任务顺序：

| 顺序 | 任务 | 交付物 | 验收 |
| --- | --- | --- | --- |
| 1 | Protobuf 增加 `StateScope` | `GetCurrentStateRequest.scope`、`VerifyRequest.scope` | Rust/Python proto 均生成通过 |
| 2 | Rust 派生 scope | `DeviceDesiredState` / `ChangeSet` -> `StateScope` | VLAN ID、interface name 精确传递 |
| 3 | Adapter scoped state | Python Adapter 接收 scope | 先用于 verify；preflight 在没有 ownership index 前保持 full/authoritative refresh |
| 4 | scoped verify | Verify 按 `ChangeSet` scope 校验 touched subtree | verify 不做全量状态读取 |
| 5 | 单次 edit-config | renderer 输出一次 candidate XML | 一次 prepare 只调用一次 `edit_config` |

`Normal v1` 固定路径：

```text
apply intent
  -> GetCurrentState(full or authoritative owned set)
  -> normalize
  -> diff
  -> empty diff -> NoOpSuccess
  -> non-empty diff -> transaction
  -> Verify(change-set scope)
```

注意：preflight diff 不能仅按 desired scope 读取 current。delete 类变更的对象已经不在 desired 中，若只查 desired scope 会漏掉待删除资源。后续引入资源 ownership index 后，preflight 可从“Aria 管理过的资源集合”派生更小 scope。

第一阶段不启用只基于 shadow 的 Fast NoOp。Fast 模式依赖 DriftAuditor、shadow freshness 和漂移策略稳定后再实现。

### 12.2 confirmed-commit 分层

confirmed-commit 能力按恢复能力分层：

| 策略 | 能力要求 | 处理 |
| --- | --- | --- |
| `ConfirmedCommitPersistent` | confirmed-commit:1.1 + persist-id | 生产首选，可跨 session confirm/cancel/recover |
| `ConfirmedCommitSession` | confirmed-commit:1.0 | 可使用自动回滚窗口，但 session 断开后可能 InDoubt |
| `CandidateCommit` | candidate + validate | 仅在允许降级时使用 |

当前代码如果暂时只有统一 `ConfirmedCommit`，实现时必须至少保留 capability 字段，后续拆分策略枚举时不得破坏 journal 和 recovery 语义。

## 13. Sprint 6 详细任务

Sprint 6 目标：

```text
Running / CLI fallback 可控降级
```

任务：

- Adapter 实现 NAPALM backend 骨架。
- Adapter 实现 Netmiko backend 骨架。
- Adapter 在 Commit 前保存 running backup。
- Adapter 实现 reverse diff rollback。
- Adapter 实现 replace rollback，如果设备支持。
- Rust 对 degraded 策略做显式授权检查。

验收：

- 未授权 degraded 时直接失败。
- 授权 degraded 时返回 warning。
- Commit 前生成 rollback artifact。
- Commit 失败后可 best-effort rollback。
- 无法判断状态时返回 `InDoubt`。

## 14. Sprint 7 详细任务

Sprint 7 目标：

```text
生产运维加固
```

任务：

- 实现 Periodic Drift Auditor。
- 实现 DriftReport。
- 实现 `BlockNewTransaction`。
- 实现 lock exponential backoff。
- 实现 force unlock API。
- 实现 force unlock audit。
- 实现 journal GC。
- 实现 rollback artifact GC。
- 接入 RFC-002 event mapper。
- 接入 RFC-015 audit mapper。

验收：

- 带外 VLAN / interface 修改可被发现。
- `BlockNewTransaction` 可阻断新事务。
- lock 占用时按退避策略重试。
- force unlock 默认关闭。
- force unlock 开启后必须带 reason。
- terminal transaction 可按 retention 清理。
- `InDoubt` 永不自动清理。

## 15. 测试计划

### 15.1 Rust 单元测试

必须覆盖：

- Protobuf mapping。
- device registration。
- onboarding 状态流转。
- transaction strategy selection。
- planner。
- normalize。
- diff。
- journal phase update。
- retention policy。
- lock retry policy。

### 15.2 Python 单元测试

必须覆盖：

- driver registry。
- fake backend。
- netconf error mapping。
- normalize。
- dry-run diff。
- rollback artifact store。
- force unlock disabled。

### 15.3 集成测试

第一阶段先用 fake adapter。

测试场景：

- 两台设备 capability probe。
- 一台 Ready，一台 Unsupported。
- dry-run NoChange。
- prepare failure。
- commit failure。
- verify failure。
- final confirm timeout。
- rollback artifact missing。
- drift detected。

### 15.4 真实设备测试

前置条件：

- 两台可访问交换机。
- 管理 IP。
- secret_ref。
- NETCONF enabled。
- 已知测试 VLAN 范围。
- 已知测试 interface。
- 明确回滚窗口。

测试顺序：

1. capability probe。
2. get-config。
3. lock / unlock。
4. VLAN create。
5. VLAN delete。
6. interface description。
7. access mode。
8. trunk mode。
9. repeat apply 10 次。
10. lock conflict。
11. session drop。
12. manual drift。

## 16. 风险与前置决策

### 16.1 必须尽早确认

- 目标华为 / 华三设备是否启用 NETCONF。
- 设备是否支持 candidate。
- 设备是否支持 confirmed-commit。
- 设备是否支持 persist-id。
- 设备是否支持 kill-session。
- 客户是否允许 force unlock。
- 客户是否允许 CLI fallback。
- 回滚 artifact 存储目录和容量限制。
- Aria RFC-002 / RFC-015 的字段要求。

### 16.2 高风险点

- 厂商 YANG / XML namespace 不一致。
- NETCONF capability 声明支持但实际行为不完整。
- 设备 lock owner 无法识别。
- CLI rollback 不能强保证。
- 手工带外变更和 Aria desired state 冲突。
- Python Adapter 重启时 artifact 未落盘。
- gRPC 契约频繁变更导致两端不同步。

### 16.3 风险控制

- Sprint 0 必须先打通 capability。
- Sprint 1 必须真实设备验证 get-config。
- 所有 degraded 路径必须显式 warning。
- `InDoubt` 不自动恢复、不自动 GC。
- 所有 force unlock 必须人工授权。
- Protobuf 字段只追加不重排。

### 16.4 gRPC 事务通道演进决策

当前阶段不直接上 gRPC 双向流。第一阶段仍以 unary RPC 为主，先把单 endpoint ACID、幂等、fail-closed、journal 和 recovery 做正确。

最终形态预留为：

```protobuf
rpc ExecuteTransaction(stream TransactionCommand)
    returns (stream TransactionEvent);
```

演进路线：

| 阶段 | 形态 | 目标 | 是否当前实现 |
| --- | --- | --- | --- |
| 阶段 1 | unary RPC | 快速打通事务正确性 | 是 |
| 阶段 2 | 事务租约 API | adapter 通过 `tx_handle` 持有 NETCONF session / candidate lock | 否 |
| 阶段 3 | gRPC 双向流 | 最终高性能事务通道，支持实时事件和动态决策 | 否 |

阶段 2 过渡接口建议：

```text
BeginTransaction
PrepareTransaction
CommitTransaction
VerifyTransaction
FinalConfirmTransaction
AbortTransaction
RecoverTransaction
```

阶段 3 双向流命令建议：

```text
Begin
Prepare
Commit
Verify
FinalConfirm
Abort
Recover
KeepAlive
Close
```

阶段 3 双向流事件建议：

```text
Started
Prepared
ConfirmedCommitPending
Verified
Committed
RolledBack
InDoubt
Failed
Progress
AuditEvent
```

进入阶段 3 前必须满足：

- 真实设备 NETCONF prepare / commit / verify 已联调。
- Adapter recovery 和 `InDoubt` 处理已经成熟。
- telemetry / audit 事件字段已经稳定。
- Protobuf command/event 状态机已经文档化。
- stream 断开、重复 command、乱序 command、超时和重连都有测试。

## 17. 立即开工顺序

按这个顺序开始开发：

```text
1. 初始化 Rust crate
2. 初始化 adapter-python
3. 写 proto/aria_underlay_adapter.proto
4. 配置 Rust tonic-build
5. 配置 Python grpcio-tools
6. 实现 Python fake GetCapabilities
7. 实现 Rust AdapterClient.get_capabilities
8. 实现 DeviceRegistration
9. 实现 DeviceOnboarding
10. 跑通 examples/capability_probe.rs
```

不要先写事务层。

第一天的目标只有一个：

```text
Rust 注册设备 -> 调 Python Adapter -> 拿 capability -> 更新 inventory 状态
```
