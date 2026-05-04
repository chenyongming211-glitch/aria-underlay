# Aria Underlay 开发方案

需求基线见 [Aria Underlay 物理管控需求说明](./aria-underlay-requirements.md)。

## 1. 目标

`aria-underlay` 的目标不是做一个松散的 Python 脚本平台，也不是做 CLI 模板渲染平台。

它的目标是做一套“Rust 主控 + Python 适配器”的声明式物理网络配置事务系统：

```text
Desired State
  -> Refresh
  -> Normalize
  -> Diff
  -> Prepare
  -> Validate
  -> Commit / Confirmed Commit
  -> Verify
  -> Final Confirm
  -> Journal
```

它只负责一件事：

把上层给出的期望网络状态，可靠、幂等、可验证、可回滚地落到物理交换机上。

面向 ToB 多厂商真实交付场景，架构采用脑手分离：

- Rust Underlay Core 负责标准 intent、单 endpoint 事务、批量编排、journal、审计和平台一致性。
- Python Underlay 适配器 负责厂商适配、NETCONF/NAPALM/Netmiko/CLI 后端和设备脏活。

第一阶段重点解决当前 Python + NETCONF 方式中经常出现的问题：

- 配置下发错误。
- 设备出现脏数据。
- 重复执行同一配置会产生额外变更。
- 半成功半失败后无法判断设备真实状态。
- 进程崩溃或 session 中断后缺少恢复依据。

## 2. 第一阶段范围

第一版只做少量关键场景，先把正确性做扎实。

覆盖范围：

- 2 台物理交换机。
- VLAN 创建、修改、删除。
- 接口 description。
- access 接口。
- trunk 接口。
- NETCONF `candidate`。
- NETCONF `validate`。
- NETCONF `commit`。
- NETCONF `confirmed-commit`，如果设备支持。
- Python 适配器。
- gRPC / Protobuf 跨语言契约。
- NAPALM / Netmiko / SSH CLI 降级通道。
- dry-run。
- diff。
- 事务日志。
- crash recovery。

暂不覆盖：

- ACL。
- VRF。
- 静态路由。
- EVPN。
- QoS。
- OpenFlow。
- 大规模 fabric 自动编排。
- 多厂商全功能覆盖。

## 3. 核心原则

### 3.1 契约先行

`aria-underlay` 首先要固定 Rust 主控与 Python 适配器 之间的 gRPC / Protobuf 契约。

核心契约稳定后，Rust 主控、Python 适配器、CLI、测试工具和未来 Aria Controller 集成都围绕同一份 Protobuf 演进。

CLI、example、REST wrapper 都只能作为调试或集成入口，不能成为核心接口。

### 3.2 Rust 管控脑与 Python 执行手

系统采用脑手分离：

```text
Rust 负责事务语义和平台一致性
Python 负责厂商适配和设备脏活
```

Rust 不处理厂商 XML、CLI、YANG 暗坑。

Python 可以处理复杂设备适配，但不能吞掉事务语义。最终事务状态、降级策略、审计、告警和结果判定必须由 Rust 主控制面掌握。

### 3.3 声明式配置

上层不提交命令序列，只提交最终期望状态。

错误示例：

```text
create vlan 100
set interface GE1/0/1 trunk allowed 100
commit
```

正确方向：

```text
device leaf-a should have:
  vlan 100
  interface GE1/0/1 trunk allowed 100
```

Underlay 模块负责：

- 刷新当前状态。
- 规范化配置。
- 计算 diff。
- 生成变更集。
- 编排事务。
- 验证结果。
- 更新 shadow state。

### 3.4 结构化模型优先

配置是数据，不是字符串。

业务层不允许直接拼 XML，也不允许直接拼 CLI。

内部应统一使用结构化模型：

```rust
pub struct VlanConfig {
    pub vlan_id: u16,
    pub name: Option<String>,
    pub description: Option<String>,
}
```

Python 适配器 中的厂商 驱动 负责把结构化模型渲染成厂商 NETCONF XML、NAPALM 操作或 CLI 操作。

这样才能支撑：

- 幂等 diff。
- dry-run。
- 审计日志。
- 回滚。
- post-commit verification。
- 多厂商适配。

### 3.5 幂等

同一个 intent 连续 apply 多次，只有第一次真正改设备。

后续请求如果 desired state 和 current state 一致，直接返回：

```text
NoOpSuccess
```

### 3.6 显式事务降级

生产默认策略应优先使用：

```text
candidate + validate + confirmed-commit
```

如果设备不支持 confirmed commit，不能偷偷降级。

降级必须满足两个条件：

- 调用方显式允许。
- 响应中明确返回 warning。
- 审计中标记 degraded。

### 3.7 长期稳态优先

后续开发默认选择长期合理、架构稳定、现场可运维的方案，而不是只补当前缺口的最小方案。

最小补丁只适用于明确的止血场景，并且必须在文档中标出后续稳态方案和退出条件。

具体要求：

- 新功能优先复用核心边界和共享校验逻辑，避免为单个入口复制一套相似但语义不同的路径。
- 失败关闭 不是简单返回错误，而是要给出结构化错误码、归一化错误、原始摘要和 retryable 语义。
- 能在无真实交换机环境验证的部分必须先通过 样本、fake 后端、快照 或 file-backed 测试闭环。
- 不能因为当前没有真实设备就降低 production gate；样本/骨架 能验证框架，但不能冒充生产可用。
- 文档、测试和实现要同步演进，避免“代码能跑但边界没人知道”。

## 4. 总体架构

```text
Aria Controller / CLI / Example
        |
        v
Rust Underlay Core
        |
        | gRPC / Protobuf
        v
Python Underlay Adapter
        |
        +--> Huawei Driver
        +--> H3C Driver
        +--> Cisco Driver
        +--> Ruijie Driver
        +--> Legacy CLI Driver
        |
        v
NETCONF / NAPALM / Netmiko / SSH CLI
        |
        v
Physical Switch A / B
```

模块职责：

| 模块 | 职责 |
| --- | --- |
| `api` | Rust 主控对外 API，请求和响应模型 |
| `intent` | 上层期望状态 |
| `planner` | 把全局意图拆成每台设备的 desired state |
| `model` | VLAN、接口、端口模式等结构化模型 |
| `state` | shadow state、refresh、drift detection |
| `engine` | normalize、diff、dry-run |
| `tx` | Rust 单 endpoint confirmed commit 编排、journal、recovery |
| `proto` | Rust 与 Python 适配器 的 gRPC / Protobuf 契约 |
| `adapter` | Python gRPC Server、厂商 驱动、协议后端 |
| `device` | Rust 侧设备 registration、onboarding、inventory、能力、adapter routing |
| `worker` | Rust 后台 worker，包括 漂移审计器、journal/artifact GC |
| `telemetry` | tracing、审计、指标 |

## 5. 推荐仓库结构

```text
aria-underlay/
├── Cargo.toml                         # Rust Underlay Core
├── build.rs                           # proto codegen, if needed
├── proto/
│   └── aria_underlay_adapter.proto
├── src/
│   ├── lib.rs
│   ├── api/
│   ├── intent/
│   ├── planner/
│   ├── model/
│   ├── proto/
│   ├── adapter_client/
│   ├── device/
│   ├── engine/
│   ├── state/
│   ├── tx/
│   ├── worker/
│   ├── telemetry/
│   └── utils/
├── adapter-python/
│   ├── pyproject.toml
│   ├── aria_underlay_adapter/
│   │   ├── server.py
│   │   ├── proto/
│   │   ├── drivers/
│   │   │   ├── huawei.py
│   │   │   ├── h3c.py
│   │   │   ├── cisco.py
│   │   │   ├── ruijie.py
│   │   │   └── legacy_cli.py
│   │   ├── 后端s/
│   │   │   ├── netconf.py
│   │   │   ├── napalm_后端.py
│   │   │   └── netmiko_后端.py
│   │   ├── diff.py
│   │   ├── rollback.py
│   │   └── state.py
│   └── tests/
├── tests/                              # Rust core tests
│   ├── proto_contract_tests.rs
│   ├── diff_tests.rs
│   ├── transaction_tests.rs
│   └── recovery_tests.rs
├── examples/
│   ├── capability_probe.rs
│   ├── create_vlan.rs
│   └── two_switch_transaction.rs
└── docs/
    ├── aria-underlay-requirements.md
    ├── aria-underlay-development-plan.md
    ├── device-capability-report.md
    ├── vendor/
    └── known-issues.md
```

## 6. 核心数据模型

### 6.1 设备信息

```rust
pub struct DeviceId(pub String);

pub enum Vendor {
    Huawei,
    H3c,
    Unknown,
}

pub enum DeviceRole {
    LeafA,
    LeafB,
}

pub struct DeviceInfo {
    pub id: DeviceId,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthConfig,
    pub vendor: Option<Vendor>,
    pub role: DeviceRole,
}
```

设备纳管状态：

```rust
pub enum DeviceLifecycleState {
    Pending,
    Probing,
    Ready,
    Degraded,
    Unsupported,
    Unreachable,
    AuthFailed,
    Drifted,
    Maintenance,
}
```

设备注册请求：

```rust
pub struct RegisterDeviceRequest {
    pub tenant_id: String,
    pub site_id: String,
    pub device_id: DeviceId,
    pub management_ip: String,
    pub management_port: u16,
    pub vendor_hint: Option<Vendor>,
    pub model_hint: Option<String>,
    pub role: DeviceRole,
    pub secret_ref: String,
    pub host_key_policy: 主机密钥策略,
    pub adapter_endpoint: String,
}
```

设备认证信息不应该直接混进普通资源模型。

第一版可以支持本地 secret provider，但模型上应保留 `secret_ref` 思路：

```rust
pub enum AuthConfig {
    PasswordRef {
        secret_ref: String,
    },
    PrivateKeyRef {
        key_ref: String,
        passphrase_ref: Option<String>,
    },
}
```

### 6.2 上层期望状态

第一版可以先收敛成双机意图模型：

```rust
pub struct SwitchPairIntent {
    pub pair_id: String,
    pub switches: Vec<SwitchIntent>,
    pub vlans: Vec<VlanIntent>,
    pub interfaces: Vec<InterfaceIntent>,
}
```

### 6.3 单设备期望状态

Planner 将双机 intent 拆成每台设备的 desired state：

```rust
pub struct DeviceDesiredState {
    pub device_id: DeviceId,
    pub vlans: BTreeMap<u16, VlanConfig>,
    pub interfaces: BTreeMap<String, InterfaceConfig>,
}
```

使用 `BTreeMap` 的原因：

- diff 输出稳定。
- dry-run 输出稳定。
- 审计 log 稳定。
- 测试 快照 稳定。

### 6.4 ChangeSet

Diff engine 输出结构化变更集：

```rust
pub struct ChangeSet {
    pub device_id: DeviceId,
    pub ops: Vec<ChangeOp>,
}

pub enum ChangeOp {
    CreateVlan(VlanConfig),
    UpdateVlan {
        before: VlanConfig,
        after: VlanConfig,
    },
    DeleteVlan {
        vlan_id: u16,
    },
    UpdateInterface {
        before: Option<InterfaceConfig>,
        after: InterfaceConfig,
    },
    DeleteInterfaceConfig {
        name: String,
    },
}
```

## 7. 幂等设计

幂等依赖四个机制：

1. Refresh current state。
2. Normalize。
3. Compute diff。
4. NoOp fast path。

标准路径：

```text
Receive Intent
    |
    v
Validate Intent
    |
    v
Plan DeviceDesiredState
    |
    v
Refresh touched current state
    |
    v
Normalize desired/current
    |
    v
Compute diff
    |
    +-- all diffs empty -> NoOpSuccess
    |
    +-- has changes -> Transaction
```

Normalize 规则：

- VLAN ID 排序。
- trunk allowed VLAN 排序、去重。
- 接口 name canonicalize。
- 空字符串 description 归一成 `None`。
- 设备默认值不要误判成 diff。

生产建议：

```text
每次事务前 refresh touched subtree
```

也就是只刷新本次会涉及的 VLAN 和 接口，而不是盲目信任内存 shadow。

## 8. 事务策略

### 8.0 ACID 设计底线

配置下发必须以 ACID 为硬约束，不能只以“RPC 调用成功”为成功标准。

ACID 边界：

```text
single management endpoint = one ACID transaction boundary
multi endpoint apply = batch orchestration of independent ACID endpoint transactions
```

四个特性的落地要求：

| ACID 特性 | Aria Underlay 落地要求 |
| --- | --- |
| Atomicity | 单 endpoint 内 `Prepare -> Commit -> Verify -> Finalize` 必须一起成功；失败必须 rollback / recover / InDoubt，不得静默成功 |
| Consistency | intent validate、能力 check、structured diff、post-commit verify 必须全部通过，事务后 running touched subtree 必须收敛到 desired subset |
| Isolation | 同一 endpoint 必须单 writer；Rust 本地锁加设备侧 lock；并发 apply 不得交叉写配置、journal 或 artifact |
| Durability | 事务开始后必须持久化 journal；回滚工件 / running backup / confirmed commit 恢复信息必须可用于进程重启后的 recovery |

开发要求：

- 每新增一个事务阶段，必须同步补 journal phase。
- 每新增一种失败路径，必须明确返回 `Failed`、`RolledBack` 或 `InDoubt`。
- 每新增一种降级策略，必须说明它满足哪些 ACID 能力，削弱了哪些能力。
- 任何 adapter 返回的成功，都必须经过 Rust 主控的状态机确认后才能变成最终成功。

生产级红线要求见 [Aria Underlay 物理管控需求说明 - 生产级红线要求检查清单](./aria-underlay-requirements.md#501-生产级红线要求检查清单)。

开发时必须同时关注：

| 红线 | 开发含义 |
| --- | --- |
| 幂等性 | refresh / normalize / diff / no-op 必须先于下发动作 |
| Fail-closed | 未实现的 驱动 / 渲染器 / adapter 方法必须报错，不允许假成功 |
| Capability 驱动 | 事务策略必须来自 能力，不允许硬编码默认强事务 |
| Drift 检测 | 事务前 touched subtree refresh 与后台巡检都要保留 |
| Recovery 可恢复 | journal、artifact、confirmed-commit 上下文必须能支撑重启恢复 |
| InDoubt 严格处理 | 无法判断最终状态时必须阻断后续写事务并等待人工处置 |
| 凭据安全 | 所有日志、journal、审计 只允许保存 `secret_ref` 和脱敏信息 |
| 可观测性 | 每个事务 phase 和 adapter RPC 都必须可追踪、可审计 |
| 测试优先 | 模拟 adapter 必须覆盖成功、失败、超时、崩溃、InDoubt |
| Isolation / 并发控制 | 同一 endpoint 单 writer，Rust 本地锁和设备侧 lock 缺一不可 |

### 8.1 策略分级

```rust
pub enum TransactionStrategy {
    ConfirmedCommitPersistent,
    ConfirmedCommitSession,
    CandidateCommit,
    RunningRollbackOnError,
    Unsupported,
}
```

选择逻辑：

```text
如果该 endpoint 支持 candidate + validate + confirmed-commit:1.1 + persist-id:
    ConfirmedCommitPersistent

否则如果该 endpoint 支持 candidate + validate + confirmed-commit:1.0:
    ConfirmedCommitSession

否则如果调用方允许降级，且该 endpoint 支持 candidate + validate:
    CandidateCommit

否则如果调用方允许降级，且该 endpoint 支持 writable-running + rollback-on-error:
    RunningRollbackOnError

否则:
    Unsupported
```

默认生产模式：

```text
StrictConfirmedCommit
```

也就是不支持 confirmed commit 就失败。

能力分级：

| 策略 | 能力要求 | 恢复能力 |
| --- | --- | --- |
| `ConfirmedCommitPersistent` | candidate + validate + confirmed-commit:1.1 + persist-id | 支持跨 session confirm/cancel/recover |
| `ConfirmedCommitSession` | candidate + validate + confirmed-commit:1.0 | 有自动回滚窗口，但 session 丢失后可能 InDoubt |
| `CandidateCommit` | candidate + validate | commit 后补偿能力弱 |
| `RunningRollbackOnError` | writable-running + rollback-on-error | 单次 edit-config 内尽量回滚 |

### 8.2 ConfirmedCommit

这是生产首选，其中 `ConfirmedCommitPersistent` 是最优形态。`ConfirmedCommitSession` 仍有自动回滚窗口价值，但恢复能力弱一档，必须在响应 warning / journal 中明确记录。

流程：

```text
1. start journal
2. lock candidate on endpoint
3. edit-config candidate on endpoint
4. validate candidate on endpoint
5. confirmed commit on endpoint
6. get-config running and verify desired subset
7. final confirm on endpoint
8. update shadow
9. mark journal committed
10. unlock candidate
```

失败处理：

```text
prepare 失败:
  discard-changes
  unlock
  running 不变

verification 失败:
  cancel-commit
  journal 标记 rolled back

final confirm 超时:
  get-config 验证 running
  如果配置已收敛 -> SuccessWithWarning
  如果配置未收敛或无法判断 -> InDoubt
```

说明：

原子事务边界是单个 management endpoint。多个 endpoint 的一次 apply 是批量编排，不是跨设备分布式原子 commit。

`confirmed-commit` 的价值是把 commit 阶段的灰区变成一个可自动回滚的短暂窗口。

### 8.3 CandidateCommit

适用于设备支持 candidate + validate，但不支持 confirmed commit。

流程：

```text
1. lock candidate on endpoint
2. edit-config candidate on endpoint
3. validate candidate on endpoint
4. commit on endpoint
5. if prepare failed, discard-changes
```

限制：

- commit 前不会影响 running。
- commit 后如果 session 断开，需要 verify/recovery 判断是否收敛。
- 需要 `InDoubt` 标记。
- 不建议作为生产默认模式。

### 8.4 RunningRollbackOnError

适用于设备不支持 candidate，但支持 writable-running + rollback-on-error。

限制：

- 只能保证单设备一次 edit-config 尽量原子。
- 不能保证跨设备原子性。
- 必须返回 warning。
- 不能宣传为跨设备事务。

## 9. Transaction Journal

事务日志必须持久化。

否则控制器进程崩溃后，无法判断设备是否处于 pending confirmed commit 状态。

核心字段：

```rust
pub struct TxJournalRecord {
    pub tx_id: TxId,
    pub request_id: String,
    pub trace_id: String,
    pub phase: TxPhase,
    pub devices: Vec<DeviceId>,
    pub change_sets: Vec<ChangeSet>,
    pub started_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
    pub status: TxStatus,
}
```

阶段：

```rust
pub enum TxPhase {
    Started,
    Prepared,
    ConfirmedCommitStarted,
    ConfirmedCommitDone,
    VerificationDone,
    FinalConfirmStarted,
    Committed,
    Aborting,
    Aborted,
    InDoubt,
}
```

第一版可以使用文件存储：

```text
{data_dir}/tx-journal/{tx_id}.json
```

不要在库里写死 `/var/lib/aria-underlay`。

`data_dir` 应由配置或上层控制面传入。

启动恢复逻辑：

```text
scan journal

if phase before confirmed commit:
    discard candidate
    unlock
    mark aborted

if phase after confirmed commit but before final confirm:
    cancel-commit(tx_id)
    verify running
    mark aborted or in-doubt

if phase in final confirm:
    verify running
    if converged:
        mark committed
    else:
        mark in-doubt and alert
```

### 9.1 Journal 与 Artifact GC

事务日志和 回滚工件 需要异步 GC，避免长期运行后磁盘膨胀。

默认 retention policy：

```text
committed_journal_retention_days = 30
rolled_back_journal_retention_days = 30
failed_journal_retention_days = 90
rollback_artifact_retention_days = 30
max_artifacts_per_device = 50
in_doubt_retention = never_auto_delete
```

GC 规则：

- `Committed` / `RolledBack` 事务达到 retention 后可以清理。
- `Failed` 事务保留更久。
- `InDoubt` 事务不得自动删除。
- 回滚工件 只有在关联事务进入 terminal 状态后才允许删除。
- 删除前必须确认无 pending recovery 依赖。
- GC 必须产生结构化日志和审计事件。

## 10. Device Onboarding、Drift 与 Lock 策略

### 10.1 产品 Initialization / Switch Pair Registration

产品初始化阶段必须把交换机纳管作为一等流程，而不是让用户先手工准备 inventory。

第一阶段面向 2 台核心交换机，建议提供一个高层 API：

```rust
pub struct InitializeUnderlaySiteRequest {
    pub request_id: String,
    pub tenant_id: String,
    pub site_id: String,
    pub adapter_endpoint: String,
    pub switches: [SwitchBootstrapRequest; 2],
    pub allow_degraded: bool,
}

pub struct SwitchBootstrapRequest {
    pub device_id: DeviceId,
    pub role: DeviceRole,
    pub management_ip: String,
    pub management_port: u16,
    pub vendor_hint: Option<Vendor>,
    pub model_hint: Option<String>,
    pub host_key_policy: 主机密钥策略,
    pub credential: NetconfCredentialInput,
}

pub enum NetconfCredentialInput {
    Password {
        username: String,
        password: String,
    },
    PrivateKey {
        username: String,
        key_pem: String,
        passphrase: Option<String>,
    },
    ExistingSecretRef {
        secret_ref: String,
    },
}
```

该 API 的内部流程：

```text
InitializeUnderlaySite
  -> validate switch pair roles
  -> create secret for each switch credential
  -> obtain secret_ref
  -> register devices into inventory as Pending
  -> trigger onboarding for both devices
  -> collect capability profiles
  -> require Ready by default
  -> allow Degraded only when allow_degraded=true
  -> return per-device result and site initialization status
```

敏感信息处理原则：

- inventory 只保存 `secret_ref`。
- 事务日志 不记录明文用户名、密码、私钥。
- 审计 只记录 secret 创建/引用事件，不记录 secret 内容。
- Python 适配器 只通过 `secret_ref` 解析实际认证信息。

初始化状态建议：

```rust
pub enum SiteInitializationStatus {
    Ready,
    ReadyWithDegradedDevice,
    Failed,
    PartiallyRegistered,
}
```

该高层 API 是产品初始化入口；底层 `RegisterDevice` 和 `DeviceOnboarding` 仍保留为内部能力和运维补救工具。

### 10.2 Device Onboarding

设备必须先纳管，再探测能力，最后才能进入配置事务。

流程：

```text
RegisterDevice
  -> validate secret_ref
  -> store inventory as Pending
  -> connectivity check
  -> call Adapter GetCapabilities
  -> classify 后端 strategy
  -> store capability profile
  -> mark Ready / Degraded / Unsupported / Unreachable / AuthFailed
```

只有 `Ready` 或显式允许 degraded 的 `Degraded` 设备可以进入配置事务。

### 10.3 Periodic Drift Auditor

Rust 主控需要后台巡检 工作进程，用于发现网工绕过 Aria 的带外变更。

当前代码已经具备一次性 drift 审计 能力：可以把 shadow store 中的期望状态与 `DriftObservationSource` 返回的 observed state 做结构化比较。尚未完成的是生产后台闭环：周期调度、真实 adapter observed source 接入、`DriftDetected` 事件发送、以及设备 lifecycle state 更新。

流程：

```text
Periodic Drift Auditor
  -> list managed devices
  -> call Adapter GetCurrentState
  -> normalize observed state
  -> compare with global desired / shadow state
  -> emit DriftDetected event
  -> update device drift status
```

默认每 15 分钟执行一次。支持按站点、设备、资源类型配置周期。

漂移处理策略：

```text
ReportOnly
BlockNewTransaction
AutoReconcile
```

第一阶段默认 `ReportOnly`，关键资源可配置 `BlockNewTransaction`，不默认启用 `AutoReconcile`。

### 10.3.1 Normal 模式的 scoped refresh

性能优化不能跳过正确性校验。第一阶段默认 `Normal` 模式，但 `Normal` 模式不允许只依赖 shadow 返回 `NoOpSuccess`。

`Normal v1` 固定流程：

```text
Receive intent
  -> Plan DeviceDesiredState
  -> Derive StateScope from desired/change-set
  -> Adapter GetCurrentState(scope)
  -> Normalize desired/current subset
  -> Compute diff
  -> empty diff -> NoOpSuccess
  -> non-empty diff -> transaction
```

shadow 的定位：

| shadow 判断 | 第一阶段行为 | 说明 |
| --- | --- | --- |
| no-op | 仍需 touched refresh 确认后才能 NoOp | 防止漏掉人工带外变更 |
| changed | 第一阶段仍建议 scoped refresh 后 diff | 避免旧 shadow 影响 delete / replace / trunk rewrite |

后续 `Normal v2` 可以按操作类型优化：

```text
shadow says no-op -> touched refresh confirms -> NoOpSuccess
shadow says changed and only merge/upsert -> can skip 预检 refresh
shadow says changed and contains delete/replace/trunk rewrite -> must refresh before diff
```

`Fast` 模式暂不启用。它必须等待 DriftAuditor 实际运行、shadow freshness timestamp 可用、漂移策略稳定后再进入生产路径。

### 10.3.2 StateScope

`StateScope` 用于表达本次事务需要读取和验证的状态子树。

建议 Protobuf：

```protobuf
message StateScope {
  bool full = 1;
  repeated uint32 vlan_ids = 2;
  repeated string interface_names = 3;
}
```

约束：

- `full = true` 表示全量读取。
- `full = false` 且列表非空表示 touched subtree。
- `full = false` 且列表为空表示空 scope，不能被解释为全量读取。
- scope 应从 `DeviceDesiredState` 或 `ChangeSet` 自动派生。
- 适配器 如果不能执行 scoped filter，必须 失败关闭 或返回明确 full-refresh warning，不能假装完成 scoped refresh。

### 10.3.3 Scoped Verify

post-commit verify 只校验本次 touched scope：

- touched VLAN 存在、删除或属性符合期望。
- touched 接口 的 description、admin state、access/trunk 符合期望。
- trunk allowed VLAN 先归一化再比较。
- 无法判断最终状态时返回失败或 `InDoubt`，不能返回成功。

### 10.3.4 单次 edit-config 渲染

同一 endpoint 的一次事务应合并为一次 `<edit-config>`：

```text
PrepareRequest
  -> 渲染器根据 ChangeSet 生成一份配置 XML
  -> edit_config(target="candidate", config=xml)
  -> validate(candidate)
```

Renderer 最终应接收 `ChangeSet`，不是只接收完整 `DeviceDesiredState`。原因是 delete / update / replace 需要明确 operation 语义，desired state 只能表达最终状态。

### 10.4 Lock Acquisition Strategy

Rust Tx Coordinator 负责锁获取策略。

默认策略：

```text
ExponentialBackoff
```

建议默认值：

- max_wait_secs = 30。
- initial_delay_ms = 500。
- max_delay_secs = 5。
- jitter = true。

锁获取失败必须产生 `UnderlayDeviceLockTimeout`。

### 10.4 Break-glass Force Unlock

Force Unlock 仅用于极端恢复场景，默认关闭。

要求：

- 只能由显式授权的管理员触发。
- 必须绑定 reason。
- 必须记录审计。
- 必须记录被踢 session id / username / source，如果设备可提供。
- 执行前必须再次确认锁仍被同一 session 占用。
- 执行后必须重新 refresh 当前设备状态。
- 执行后不能直接复用旧 diff。

NETCONF 设备可通过 `<kill-session>` 或厂商等价机制实现。

如果设备不支持安全识别锁持有者，则不得自动执行 Force Unlock。

## 11. Python 适配器 与协议后端

### 11.1 适配器 基本职责

Python 适配器 是独立 gRPC Server，负责对接真实物理设备。

适配器 内部按后端分层：

```text
gRPC Server
    |
    v
Vendor Driver Registry
    |
    +-- Huawei Driver
    +-- H3C Driver
    +-- Cisco Driver
    +-- Ruijie Driver
    +-- Legacy CLI Driver
    |
    v
Protocol Backend
    |
    +-- ncclient / NETCONF
    +-- NAPALM
    +-- Netmiko / SSH CLI
```

Rust 主控不直接处理 SSH、NETCONF framing、厂商 XML 或 CLI。

### 11.2 NETCONF 连接流程

```text
1. TCP connect
2. SSH handshake
3. SSH auth
4. open channel
5. request subsystem "netconf"
6. receive server hello
7. send client hello
8. parse capabilities
9. choose framing
10. enter Ready
```

该流程由 Python 适配器 的 NETCONF 后端 负责实现，优先使用 `ncclient`。

### 11.3 Capability

必须探测：

- `base:1.0`
- `base:1.1`
- `candidate`
- `validate`
- `confirmed-commit`
- `rollback-on-error`
- `writable-running`
- `startup`

注意：

`confirmed-commit:1.0` 和 `confirmed-commit:1.1` 不应简单视为完全等价。

如果依赖 `persist` / `persist-id` 做跨 session 恢复，必须按真实 能力 精确判断。

### 11.4 RPC

NETCONF 后端 第一版实现：

- `get-config`
- `lock`
- `unlock`
- `edit-config`
- `validate`
- `commit`
- `discard-changes`
- `confirmed-commit`
- `confirm-commit`
- `cancel-commit`
- `kill-session`，仅 break-glass 场景使用
- `close-session`

所有 RPC 必须：

- 携带递增 `message-id`。
- 解析 `<rpc-error>`。
- 超时可控。
- 记录 latency。
- 记录 request/response 摘要用于审计。

适配器 返回给 Rust 的是标准化结果，不直接返回未解析 XML 作为主结果。

### 11.5 CLI 降级后端

当设备不支持 NETCONF 或 NETCONF 实现不可用时，适配器 可以使用 NAPALM / Netmiko / SSH CLI 降级。

降级模式必须满足：

- 修改前保存 回滚工件。
- 执行前做 dry-run / diff。
- 返回 degraded warning。
- 失败后支持 best-effort rollback。
- 无法判断最终状态时返回 `InDoubt`。

## 12. gRPC 契约与 Driver 层

### 12.1 gRPC RPC

Rust 主控与 Python 适配器 至少需要以下 RPC：

```text
GetCapabilities
GetCurrentState
DryRun
Prepare
Commit
Rollback
Verify
Recover
ForceUnlock
```

第一阶段这些 RPC 可以保持 unary 形态，重点是把事务状态、错误码、journal 和 失败关闭 行为做正确。

### 12.1.1 最终目标：ExecuteTransaction 双向流

长期最终形态预留为 gRPC 双向流：

```protobuf
rpc ExecuteTransaction(stream TransactionCommand)
    returns (stream TransactionEvent);
```

采用该形态后，Rust 主控不再为一次设备事务反复调用多个独立 RPC，而是在同一个 stream 中发送事务命令：

```text
Begin
Prepare
Commit
Verify
FinalConfirm
Abort
Recover
Close
```

Python 适配器 在这个 stream 生命周期内维护：

- 一个 NETCONF session。
- candidate lock。
- confirmed-commit persist token。
- 厂商 驱动 上下文。
- 回滚工件 / running backup 引用。

事件流返回：

```text
Started
Prepared
ConfirmedCommitPending
Verified
Committed
RolledBack
InDoubt
Failed
AuditEvent
进度
```

这个方案的价值：

- 把单 endpoint 事务的 NETCONF 握手次数从多次降到一次。
- Rust 可以基于中间结果动态决策。
- 适配器 可以自然持有设备 lock，减少重复 lock/unlock。
- 事务事件天然适合接入 telemetry 和 审计。
- 更适合后续复杂厂商适配、CLI 降级和长耗时 recovery。

但它不是当前阶段主路径。原因：

- 需要定义严格 command/event 状态机。
- stream 中断、半关闭、超时和恢复更复杂。
- Python 适配器 会从无状态服务演进为短生命周期有状态事务执行器。
- 测试需要覆盖乱序 command、重复 command、断流、重连、recover。

因此当前开发顺序保持：

```text
阶段 1：unary RPC，保证正确性
阶段 2：事务租约 API，adapter 通过 tx_handle 复用 NETCONF session
阶段 3：ExecuteTransaction 双向流，作为最终高性能事务通道
```

阶段 2 的事务租约 API 可以作为过渡：

```text
BeginTransaction -> tx_handle
PrepareTransaction(tx_handle)
CommitTransaction(tx_handle)
VerifyTransaction(tx_handle)
FinalConfirmTransaction(tx_handle)
AbortTransaction(tx_handle)
```

它比双向流简单，但已经能解决重复 NETCONF 握手问题。阶段 3 再把这些命令收敛到一个双向流里。

### 12.2 Protobuf 核心对象

第一版至少定义：

```text
DeviceRef
DeviceAuthRef
DeviceCapability
DesiredDeviceState
ObservedDeviceState
ChangeSet
PrepareRequest
PrepareResponse
CommitRequest
CommitResponse
RollbackRequest
RollbackResponse
VerifyRequest
VerifyResponse
AdapterError
RollbackArtifactRef
TransactionPhase
BackendKind
DegradeReason
LockAcquisitionPolicy
ForceUnlockRequest
ForceUnlockResponse
DriftReport
RetentionPolicy
```

适配器 返回结果不得只有 `success`，必须包含：

- `request_id`
- `tx_id`
- `trace_id`
- `device_id`
- `phase`
- `后端`
- `capabilities`
- `changed`
- `rollback_artifact_ref`
- `warnings`
- `errors`
- `raw_error_summary`
- `normalized_state`

### 12.3 Driver 层

Python Driver 负责厂商差异：

- namespace。
- VLAN XML。
- 接口 XML。
- access/trunk 表达方式。
- description 字段映射。
- running config parse。
- subtree filter。
- CLI 方言。
- 设备错误标准化。

Python 驱动 抽象接口建议：

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

第一版建议先打通一个标准 NETCONF 厂商的 VLAN + 接口 最小闭环，同时保留 CLI fallback 驱动。

## 13. API 设计

核心入口：

```rust
#[async_trait]
pub trait UnderlayService {
    async fn register_device(
        &self,
        request: RegisterDeviceRequest,
    ) -> Result<RegisterDeviceResponse, UnderlayError>;

    async fn onboard_device(
        &self,
        device_id: DeviceId,
    ) -> Result<DeviceOnboardingResponse, UnderlayError>;

    async fn apply_intent(
        &self,
        request: ApplyIntentRequest,
    ) -> Result<ApplyIntentResponse, UnderlayError>;

    async fn dry_run(
        &self,
        request: ApplyIntentRequest,
    ) -> Result<DryRunResponse, UnderlayError>;

    async fn refresh_state(
        &self,
        request: RefreshStateRequest,
    ) -> Result<RefreshStateResponse, UnderlayError>;

    async fn get_device_state(
        &self,
        device_id: DeviceId,
    ) -> Result<DeviceShadowState, UnderlayError>;

    async fn recover_pending_transactions(
        &self,
    ) -> Result<RecoveryReport, UnderlayError>;

    async fn run_drift_audit(
        &self,
        request: DriftAuditRequest,
    ) -> Result<DriftAuditResponse, UnderlayError>;

    async fn force_unlock(
        &self,
        request: ForceUnlockRequest,
    ) -> Result<ForceUnlockResponse, UnderlayError>;
}
```

请求必须包含：

```rust
pub struct ApplyIntentRequest {
    pub request_id: String,
    pub trace_id: Option<String>,
    pub intent: SwitchPairIntent,
    pub options: ApplyOptions,
}
```

响应必须包含：

```rust
pub struct ApplyIntentResponse {
    pub request_id: String,
    pub trace_id: String,
    pub tx_id: Option<TxId>,
    pub status: ApplyStatus,
    pub strategy: Option<TransactionStrategy>,
    pub device_results: Vec<DeviceApplyResult>,
    pub warnings: Vec<String>,
}
```

状态：

```rust
pub enum ApplyStatus {
    NoOpSuccess,
    Success,
    SuccessWithWarning,
    Failed,
    RolledBack,
    InDoubt,
}
```

## 14. 开发顺序

不要一开始铺完整平台。

建议按以下顺序开发：

### Sprint 0：设备纳管、Protobuf 契约与能力探测

目标：

- 定义设备注册和 onboarding API。
- 实现 Rust inventory 初始模型。
- 定义 Rust 主控与 Python 适配器 的 gRPC / Protobuf 契约。
- Python 适配器 打通一台设备的 能力 probe。
- 优先通过 NETCONF 获取 `<hello>`。
- 解析 能力。
- 判断设备支持的事务能力和降级能力。

交付：

- `proto/aria_underlay_adapter.proto`
- `src/device/registration.rs`
- `src/device/onboarding.rs`
- `adapter-python/aria_underlay_adapter/server.py`
- `examples/capability_probe.rs`
- `docs/device-capability-report.md`

验收：

- Rust 可以通过 gRPC 调用 Python 适配器。
- 设备可以注册到 inventory 并进入 `Pending`。
- onboarding 后设备进入 `Ready` / `Degraded` / `Unsupported` / `Unreachable` / `AuthFailed`。
- 能打印两台设备 能力。
- 能自动判断推荐事务策略和是否需要 degraded fallback。

### Sprint 1：Python 适配器协议后端

目标：

- 使用 `ncclient` 封装 NETCONF 基础能力。
- 实现 hello / 能力。
- 实现 get-config / lock / unlock / edit-config / validate / commit / discard。
- 实现 confirmed-commit / cancel-commit，如果设备支持。
- 实现 NAPALM / Netmiko 降级后端骨架。
- 将设备错误标准化为 Protobuf `AdapterError`。

交付：

- `adapter-python/aria_underlay_adapter/后端s/netconf.py`
- `adapter-python/aria_underlay_adapter/后端s/napalm_后端.py`
- `adapter-python/aria_underlay_adapter/后端s/netmiko_后端.py`
- `adapter-python/tests/`

验收：

- 真实设备上 `get-config` 成功。
- lock / unlock 成功。
- 适配器 能返回标准化 能力 和错误。

### Sprint 2：Rust 核心模型与适配器客户端

目标：

- VLAN 模型。
- 接口 模型。
- access/trunk 模型。
- Rust gRPC adapter client。
- transaction strategy selection。
- request_id / tx_id / trace_id 贯穿。

交付：

- `src/model/*`
- `src/adapter_client/*`
- `src/device/*`
- `src/tx/strategy.rs`

验收：

- Rust 能把标准 desired state 发送给 Python 适配器。
- Rust 能根据 适配器 能力 选择事务策略。

### Sprint 3：Python 驱动与设备级差异计算

目标：

- 厂商 驱动 registry。
- VLAN / 接口 渲染器。
- running config parse。
- vendor-specific normalize。
- device-level diff。
- dry-run / NoChange 拦截。
- 回滚工件 生成。

交付：

- `adapter-python/aria_underlay_adapter/drivers/*`
- `adapter-python/aria_underlay_adapter/diff.py`
- `adapter-python/aria_underlay_adapter/rollback.py`
- `adapter-python/aria_underlay_adapter/state.py`

验收：

- 同一 desired state 重复 dry-run，第二次返回 `NoChange`。
- 给定 VLAN/接口 desired state，可以生成目标厂商操作。

### Sprint 4：Rust 单端点事务与日志

目标：

- Rust endpoint tx coordinator。
- Rust 事务日志。
- 调用 适配器 Prepare / Commit / Rollback / Verify。
- Candidate / ConfirmedCommit 状态机。
- failure path。
- journal。

交付：

- `src/tx/candidate_commit.rs`
- `src/tx/journal.rs`
- `tests/transaction_tests.rs`

验收：

- 任意一台设备 prepare 失败，另一台 running 不变。

### Sprint 5：确认提交与恢复

目标：

- Rust 编排 confirmed commit。
- Python 适配器 执行 confirmed-commit / cancel-commit。
- post-commit verification。
- final confirm。
- recovery。
- InDoubt 标记。

交付：

- `src/tx/confirmed_commit.rs`
- `tests/recovery_tests.rs`

验收：

- A confirmed commit 成功，B confirmed commit 失败时，A 自动 cancel。
- 最终 A/B running 均不保留本次变更。

### Sprint 6：降级模式与真实设备联调

目标：

- VLAN create/update/delete。
- access/trunk 接口。
- lock conflict。
- invalid input。
- session drop。
- running / CLI fallback。
- 回滚工件 restore。
- 记录厂商 XML 差异。

交付：

- `docs/vendor/huawei.md`
- `docs/vendor/h3c.md`
- `docs/known-issues.md`

验收：

- 两台真实交换机上完成 VLAN + 接口 事务配置。
- 失败时能回滚或明确进入 `InDoubt`。

### Sprint 7：生产运维加固

目标：

- Periodic Drift Auditor。
- Lock Acquisition Strategy。
- break-glass force unlock。
- journal / 回滚工件 GC。
- RFC-002 / RFC-015 事件和审计联动。

交付：

- `src/worker/drift_auditor.rs`
- `src/worker/gc.rs`
- `src/tx/lock_strategy.rs`
- `src/api/force_unlock.rs`
- `tests/drift_tests.rs`
- `tests/gc_tests.rs`

验收：

- 周期巡检能发现带外 VLAN / 接口 变更。
- `BlockNewTransaction` 策略能阻断漂移设备的新事务。
- lock 被占用时按指数退避重试，超时后产生结构化事件。
- break-glass force unlock 默认关闭，开启后有完整审计。
- terminal transaction 的 journal / artifact 可以按 retention policy 清理。
- `InDoubt` 事务及其 artifact 不会被自动清理。

## 15. 测试策略

必须做三类测试。

### 15.1 单元测试

覆盖：

- Protobuf contract。
- device registration / onboarding。
- 能力 解析器。
- normalize。
- diff。
- XML 渲染器。
- rpc-error 解析器。
- transaction strategy selection。
- lock acquisition policy。
- retention policy。

### 15.2 适配器 Mock 测试

覆盖：

- lock 失败。
- edit-config 失败。
- validate 失败。
- confirmed commit 失败。
- final confirm 超时。
- session 中断。
- CLI fallback 失败。
- 回滚工件 丢失。
- lock owner 识别。
- force unlock 成功 / 失败。

### 15.3 真实设备测试

覆盖：

- 能力 配置档案。
- namespace 差异。
- VLAN 创建、修改、删除。
- 接口 access/trunk。
- lock 被占用。
- session 断开。
- NETCONF 不可用时降级到 NAPALM / Netmiko。
- 带外修改后 漂移审计器 能发现差异。
- 支持时验证 break-glass force unlock。

## 16. 第一版验收标准

第一版必须满足：

- 能注册设备到 inventory。
- 设备注册后进入 onboarding。
- 未完成 onboarding 的设备不能进入配置事务。
- 两台交换机同时创建 VLAN 成功。
- 两台交换机同时删除 VLAN 成功。
- 单台 prepare 失败时，另一台 running 不变。
- 单台 validate 失败时，另一台 running 不变。
- confirmed commit 阶段单台失败时，另一台自动 cancel。
- 同一 intent 连续 apply 10 次，后 9 次返回 `NoOpSuccess`。
- 设备已有脏数据时，dry-run 能展示差异。
- lock 被占用时，按指数退避重试，超时后失败，不破坏 running。
- NETCONF session 中断时，事务进入可恢复状态。
- running / CLI 降级模式下，修改前生成 回滚工件。
- 降级失败后能 best-effort rollback 或标记 `InDoubt`。
- 进程崩溃重启后，可以根据 journal 恢复或标记 `InDoubt`。
- 周期 Drift Auditor 能发现带外 VLAN / 接口 变更。
- `BlockNewTransaction` 策略下，存在漂移的设备不能继续下发事务。
- break-glass force unlock 默认关闭，开启后必须产生完整审计。
- 已提交事务 journal 和 回滚工件 能按 retention policy 清理。
- `InDoubt` 事务及其 artifact 不会被自动 GC。
- 所有降级事务必须显式返回 warning。
- 所有事务日志必须包含 `request_id`、`tx_id`、`trace_id`、`device_id`、`phase`、`rpc`、`latency`、`result`。

### 16.1 运维入口验收补充

在真实交换机接入前，第一版运维入口还必须满足：

- worker daemon 能通过检入的 JSON 配置启动本地 操作摘要、操作告警、journal GC 和 drift 审计 调度。
- 运维人员可以通过正式 `aria-underlay-ops` CLI 查询 操作摘要 overview、operation 明细、告警 overview、告警 明细和 InDoubt 事务。
- `force-resolve` 只能通过显式 break-glass 参数执行，并且必须写入 journal manual resolution 和 操作摘要。
- 操作摘要 / 告警 的 JSONL 本地模式必须可轮转、可压缩、可保留、可 失败关闭。
- operator 操作手册 必须说明 操作摘要、告警、GC、drift、recovery 风险的判断与处理。
- product 审计 后端 和 RBAC 在上线前必须独立落地；本地 JSONL/CLI 不是最终审计权限边界。

当前配套文档：

- `docs/examples/underlay-worker-daemon.local.json`
- `docs/操作手册s/operator-operations.md`
- `docs/superpowers/specs/2026-05-03-product-audit-rbac-design.md`

## 17. 最小第一步

真正开始写代码时，第一批文件应从这里开始：

```text
proto/aria_underlay_adapter.proto
src/adapter_client/mod.rs
src/device/registration.rs
src/device/onboarding.rs
src/tx/strategy.rs
adapter-python/aria_underlay_adapter/server.py
adapter-python/aria_underlay_adapter/后端s/netconf.py
examples/capability_probe.rs
```

先把以下能力跑通：

- Rust 通过 gRPC 调用 Python 适配器。
- 设备 registration / onboarding。
- Python 适配器 使用 ncclient 获取 `<hello>`。
- 能力 解析。
- `<get-config>`。
- `<lock>` / `<unlock>`。
- 标准化错误返回。

再进入 驱动、diff、transaction。

不要先写大事务框架，否则真实设备 能力、厂商 XML 或 CLI 降级能力不符合预期时，会返工很多。
