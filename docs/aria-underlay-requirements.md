# Aria Underlay 物理管控需求说明

## 1. 背景

Aria 面向 ToB 私有化交付场景。单个客户现场的物理交换机规模通常不大，第一阶段以 2 台核心交换机为主，但不能把系统长期绑定为固定双交换机模型。真实现场常见三种形态：两台交换机堆叠但只有一个管理 IP、两台交换机做 MLAG 且有两个管理 IP、以及 20 台以内的小规模多交换机管控域。客户群体庞大，设备品牌、型号、系统版本和协议支持情况高度不可控。

真实交付环境可能包含：

- 华为 CE。
- 华三 Comware。
- Cisco IOS XE / NX-OS。
- 锐捷。
- 白盒交换机。
- 老旧型号交换机。
- 标准 NETCONF 支持不完整的设备。
- 只能通过 SSH CLI 管理的设备。

因此，`aria-underlay` 的核心价值不是单纯“能下发 VLAN”，而是为 Aria 平台提供一个面向复杂客户现场的、多厂商可适配的、可事务化控制的物理网络管控能力。

## 2. 核心目标

`aria-underlay` 必须解决当前 Python + NETCONF 方式中暴露出的核心问题：

- 配置下发错误。
- 脏数据残留。
- 重复下发不幂等。
- 多台设备下发半成功半失败。
- 老旧设备缺少标准事务能力。
- 厂商私有 YANG / XML / CLI 差异导致适配成本高。
- 设备侧错误无法进入 Aria 统一事件和审计体系。

最终目标：

```text
向上暴露统一网络意图
向下适配多厂商物理交换机
中间通过事务、幂等、回滚和审计保证配置正确落地
```

## 3. 业务与商业场景需求

### 3.1 多厂商开盲盒适配能力

客户机房里的物理交换机品牌与型号不可控。

系统必须具备强厂商无关性：

- 上层 Aria 不感知华为、华三、思科、锐捷等厂商差异。
- 上层 Aria 不感知 YANG namespace、XML schema、CLI 方言。
- 上层 Aria 只提交标准化网络意图。
- Underlay 适配层负责将标准意图翻译为厂商实际可执行操作。

多厂商适配能力是该模块的核心商业壁垒。

### 3.2 极简私有化交付

虽然客户群体设备类型复杂，但单个客户现场通常只需要接管少量核心交换机。第一阶段重点覆盖 1 到 2 个管理 endpoint，后续模型必须自然扩展到 20 台以内的小规模 underlay 管控域。

系统必须支持：

- 本地化部署。
- 快速配置设备清单。
- 快速探测设备能力。
- 快速验证 VLAN / interface 基础能力。
- 不依赖复杂外部平台即可完成最小闭环。

第一阶段应优先保障小规模场景的稳定性，而不是先追求大规模 fabric 编排。

### 3.3 Underlay 管控域模型

Aria Underlay 的长期业务对象不是固定的 `SwitchPair`，而是：

```text
UnderlayDomain
```

`UnderlayDomain` 由以下对象组成：

- management endpoint：真正的 NETCONF / SSH 管理入口。
- switch member：管控域内的交换机成员。
- topology：堆叠、MLAG、小规模多交换机。
- VLAN / interface / binding intent。

必须支持：

| 场景 | 物理交换机 | 管理 IP | 事务参与者 |
| --- | --- | --- | --- |
| 堆叠 | 通常 2 台 | 1 个 | 1 个 endpoint |
| MLAG | 通常 2 台 | 2 个 | 2 个 endpoint |
| 小规模多交换机 | 3 到 20 台 | 1 到 20 个 | N 个 endpoint |

事务参与者按 management endpoint 计算，而不是按物理交换机成员计算。

## 4. 系统架构需求

### 4.1 脑手分离架构

系统采用异构微服务架构：

```text
Aria Controller / Underlay Core - Rust
        |
        | gRPC / Protobuf
        v
Aria Underlay Adapter - Python
        |
        +-- Huawei Driver
        +-- H3C Driver
        +-- Cisco Driver
        +-- Ruijie Driver
        +-- Legacy CLI Driver
        |
        v
Physical Switches
```

该架构将系统拆分为：

- 大脑：Rust 主控制面。
- 双手：Python 南向适配器。

### 4.2 Rust 主控制面职责

Rust 侧负责平台级一致性和事务控制。

职责包括：

- 统一业务模型。
- 多租户模型。
- 标准化 intent。
- 全局资源模型。
- 设备 inventory。
- secret reference。
- 全局事务协调。
- 事务状态机。
- transaction journal。
- operation lifecycle。
- 审计。
- tracing。
- metrics。
- 与 Aria RFC-002 事件模型对接。
- 与 Aria RFC-015 审计视图对接。

Rust 主控制面不得处理：

- 厂商 XML 拼接。
- 厂商 CLI 模板。
- SSH 协议细节。
- NETCONF 私有方言。
- 厂商 YANG 暗坑。

### 4.3 Python Underlay Adapter 职责

Python 侧负责物理设备适配。

职责包括：

- 独立运行 gRPC Server。
- 接收 Rust 下发的标准化设备操作。
- 动态加载厂商 driver。
- 使用 `ncclient` 对接标准 NETCONF。
- 使用 NAPALM 读取设备状态和执行部分配置能力。
- 使用 Netmiko / SSH CLI 兜底老旧设备。
- 处理厂商 XML / YANG / CLI 方言。
- 执行单设备 prepare / commit / rollback / verify。
- 采集设备原始错误并标准化返回。

Python Adapter 可以作为进程无状态服务运行，但事务产物不能丢失。

以下数据必须可持久化或可从 Rust 主控恢复：

- rollback artifact。
- running config backup。
- candidate snapshot。
- tx phase。
- device operation result。
- raw error summary。

### 4.4 跨语言通信契约

Rust 与 Python 必须通过 gRPC / Protobuf 通信。

Protobuf 中传递的是标准化意图和设备操作，不是厂商命令，也不是底层 XML。

禁止：

```text
Rust -> Python: "<vlan><id>100</id></vlan>"
Rust -> Python: "vlan 100\n name prod"
```

允许：

```text
Rust -> Python: DesiredDeviceState {
  vlans: [{ vlan_id: 100, name: "prod" }]
}
```

## 5. 核心控制域需求

### 5.1 绝对幂等性

同一条配置意图，下发一次和下发一万次，设备最终状态必须一致。

重复下发不得导致：

- 重复创建报错。
- 接口 flap。
- 网络抖动。
- 冗余 commit。
- 设备配置顺序无意义变化。
- 审计日志中产生误导性变更记录。

幂等实现采用双层机制：

Rust 侧：

- intent normalize。
- request 去重。
- operation 状态管理。
- 全局 transaction journal。
- 全局 desired state 管理。

Python 侧：

- 拉取设备真实 running state。
- 厂商特定 normalize。
- 设备级 diff。
- NoChange 拦截。
- NETCONF `merge` / `replace` 等声明式原语。

Adapter 在真实下发前必须返回本设备判断结果：

```text
NoChange
NeedChange
Unsupported
Conflict
DriftDetected
```

### 5.2 原子性与 2PC

多台交换机的配置下发必须尽量做到同生共死。

目标：

- 不允许静默出现 A 成功、B 失败。
- 不允许失败后不记录状态。
- 不允许无法判断设备当前是否已变更。

Rust 主控必须实现全局 2PC 状态机：

```text
Prepare
  -> Validate
  -> Commit
  -> Verify
  -> Finalize

Failure
  -> Rollback
  -> Recover / InDoubt
```

Python Adapter 负责单设备事务动作：

```text
Prepare(tx_id, device, desired_state)
Commit(tx_id, device)
Rollback(tx_id, device)
Verify(tx_id, device)
Recover(tx_id, device)
```

### 5.3 事务降级与补偿

客户环境中可能存在不支持 NETCONF `:candidate` 的设备。

系统必须支持按设备能力选择事务策略。

#### 5.3.1 理想模式

设备支持：

- NETCONF。
- candidate。
- validate。
- confirmed-commit。

流程：

```text
Rust Prepare
  -> Python lock candidate + edit candidate + validate

Rust Commit
  -> Python confirmed-commit

Rust Verify
  -> Python get running + compare desired subset

Rust Finalize
  -> Python final confirm
```

该模式作为生产首选。

#### 5.3.2 标准 Candidate 模式

设备支持：

- NETCONF。
- candidate。
- validate。
- commit。

但不支持 confirmed-commit。

流程：

```text
Prepare:
  lock candidate
  edit candidate
  validate

Commit:
  commit

Rollback:
  commit 前 discard
  commit 后进入补偿或 InDoubt
```

该模式需要明确标记事务能力弱于 confirmed-commit。

#### 5.3.3 Running / CLI 降级模式

设备不支持 candidate，或者不支持 NETCONF，只能修改 running 或通过 CLI 操作。

流程：

```text
Prepare:
  fetch running config
  persist rollback artifact
  compute diff
  precheck

Commit:
  apply running config or CLI commands

Rollback:
  reverse diff or replace from backup
```

该模式只能定义为：

```text
BestEffortRollback
```

必须满足：

- 调用方显式允许降级。
- 响应中返回 warning。
- 审计中标记 degraded。
- 失败后能够进入 `RolledBack` 或 `InDoubt`。

不能将该模式宣传为强跨设备原子事务。

## 6. 协议需求

### 6.1 主力协议

南向通信主力协议为：

```text
NETCONF over SSH
```

优先使用 NETCONF 的结构化能力替代脆弱的 CLI 文本解析。

Adapter 必须探测并记录设备能力：

- `base:1.0`
- `base:1.1`
- `candidate`
- `validate`
- `confirmed-commit`
- `rollback-on-error`
- `writable-running`
- `startup`

### 6.2 降级协议

当设备不支持 NETCONF，或 NETCONF 实现不可用时，Python Adapter 必须保留降级通道：

- NAPALM。
- Netmiko。
- SSH CLI。

降级协议仅用于商业交付兜底，不作为强事务默认路径。

## 7. gRPC / Protobuf 接口需求

第一版至少需要定义以下 RPC：

```text
GetCapabilities
GetCurrentState
DryRun
Prepare
Commit
Rollback
Verify
Recover
```

核心对象：

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
```

Adapter 返回结果不得只有 `success`。

必须包含：

- `request_id`
- `tx_id`
- `trace_id`
- `device_id`
- `phase`
- `backend`
- `capabilities`
- `changed`
- `rollback_artifact_ref`
- `warnings`
- `errors`
- `raw_error_summary`
- `normalized_state`

## 8. 可观测性与审计需求

物理交换机每一次配置变更必须进入 Aria 统一可观测体系。

必须记录：

- request_id。
- tx_id。
- trace_id。
- tenant_id。
- site_id。
- device_id。
- vendor。
- model。
- backend。
- transaction strategy。
- phase。
- rpc / command。
- latency。
- result。
- warning。
- normalized error。
- raw error summary。
- rollback artifact reference。

必须接入：

- RFC-002 统一事件模型。
- RFC-015 审计视图。

典型事件：

```text
UnderlayTransactionStarted
UnderlayPrepareSucceeded
UnderlayPrepareFailed
UnderlayCommitStarted
UnderlayCommitSucceeded
UnderlayCommitFailed
UnderlayRollbackStarted
UnderlayRollbackSucceeded
UnderlayRollbackFailed
UnderlayTransactionInDoubt
UnderlayDeviceLockTimeout
UnderlayDeviceCapabilityDetected
UnderlayDegradedStrategyUsed
UnderlayDeviceRegistered
UnderlayDeviceOnboardingFailed
UnderlayDriftDetected
UnderlayForceUnlockRequested
UnderlayForceUnlockSucceeded
UnderlayForceUnlockFailed
UnderlayJournalGcCompleted
```

## 9. 设备纳管需求

真实客户现场中，设备不能从 Capability Probe 直接开始。系统必须先完成设备纳管，再进入探测和事务流程。

### 9.0 产品初始化时的交换机录入

Aria Underlay 的产品初始化流程必须包含交换机录入步骤。

客户在初始化产品或站点时，需要一次性录入第一阶段要接管的 underlay 管控域信息。当前实现仍保留两台交换机初始化入口用于 MLAG 双 ToR 兼容场景，但产品模型必须向 domain 初始化演进：

- topology，例如堆叠单管理 IP、MLAG 双管理 IP、小规模多交换机。
- management endpoint 列表。
- switch member 到 management endpoint 的映射关系。
- 交换机管理 IP。
- NETCONF 管理端口，默认 830。
- NETCONF 用户名。
- NETCONF 密码或私钥。
- 设备角色，例如 `LeafA` / `LeafB`，仅作为 MLAG 兼容角色。
- 厂商提示，可选，例如 Huawei / H3C / Cisco / Ruijie。
- host key 策略，例如 TOFU、known_hosts 或 pinned fingerprint。

产品初始化流程不得把用户名、密码、私钥等敏感信息直接写入普通 inventory 或资源模型。

正确流程是：

```text
Product Initialize
  -> collect underlay topology, management endpoints and NETCONF credentials
  -> create secret in secret provider
  -> receive secret_ref
  -> register management endpoints into inventory
  -> map switch members to management endpoints
  -> trigger onboarding automatically
  -> adapter resolves secret_ref
  -> NETCONF capability probe
  -> mark Ready / Degraded / Unsupported / Unreachable / AuthFailed
```

初始化完成条件：

- 所有 management endpoint 都成功写入 inventory。
- 所有 management endpoint 都完成 onboarding。
- 默认要求所有 endpoint 都进入 `Ready`。
- 如果允许降级交付，必须由初始化选项显式允许 `Degraded`。
- 任意设备进入 `AuthFailed`、`Unreachable`、`Unsupported` 时，产品初始化必须返回失败或部分失败状态，并给出明确原因。

因此产品层需要提供一个高层初始化入口，而不是要求调用方手动串联多个底层接口。

建议命名：

```text
InitializeUnderlaySite
InitializeUnderlayDomain
```

该入口内部负责：

- 写入 secret。
- 生成或接收 `secret_ref`。
- 调用 Device Registration。
- 触发 Device Onboarding。
- 汇总 endpoint 初始化结果。
- 产生审计事件。

### 9.1 Device Registration

Rust 主控必须提供设备注册入口。

注册信息至少包含：

- tenant_id。
- site_id。
- device_id。
- management_ip。
- management_port。
- vendor hint。
- model hint。
- role。
- secret_ref。
- host key policy。
- adapter endpoint。

设备注册后进入 inventory，初始状态为：

```text
Pending
```

### 9.2 Device Onboarding

注册完成后，由 Rust 主控触发异步 Onboarding 流程：

```text
RegisterDevice
  -> validate secret_ref
  -> store inventory as Pending
  -> connectivity check
  -> call Adapter GetCapabilities
  -> classify backend strategy
  -> store capability profile
  -> mark Ready / Degraded / Unsupported / Unreachable / AuthFailed
```

设备状态至少包括：

```text
Pending
Probing
Ready
Degraded
Unsupported
Unreachable
AuthFailed
Drifted
Maintenance
```

只有 `Ready` 或显式允许 degraded 的 `Degraded` 设备可以进入配置事务。

## 10. Drift 巡检需求

客户现场存在网工绕过 Aria 直接 SSH 登录设备修改配置的情况。系统不能只依赖事务前的 touched subtree refresh。

Rust 主控必须提供后台巡检 Worker：

```text
Periodic Drift Auditor
  -> list managed devices
  -> call Adapter GetCurrentState
  -> normalize observed state
  -> compare with global desired / shadow state
  -> emit DriftDetected event
  -> update device drift status
```

巡检策略：

- 默认每 15 分钟执行一次。
- 支持按站点、设备、资源类型配置周期。
- 支持全量状态巡检。
- 支持关键子树巡检。
- 巡检失败必须产生结构化事件。

漂移处理策略：

```text
ReportOnly
BlockNewTransaction
AutoReconcile
```

默认策略为 `ReportOnly`。

关键 underlay 资源可以配置 `BlockNewTransaction`。

第一阶段不默认启用 `AutoReconcile`，避免覆盖现场应急变更。

## 11. Lock Acquisition 需求

物理交换机配置锁可能被人工会话或其他系统长期占用。系统必须有明确的锁获取策略。

Rust Tx Coordinator 负责定义 Lock Acquisition Strategy。

基础策略：

```text
ExponentialBackoff
```

建议默认值：

- max_wait_secs = 30。
- initial_delay_ms = 500。
- max_delay_secs = 5。
- jitter = true。

锁获取失败必须产生：

```text
UnderlayDeviceLockTimeout
```

### 11.1 Break-glass Force Unlock

在极端恢复场景下，系统可以支持强制解锁。

Force Unlock 仅作为 break-glass 能力，不是普通事务路径。

必须满足：

- 默认关闭。
- 只能由显式授权的管理员触发。
- 必须绑定 reason。
- 必须记录审计。
- 必须记录被踢 session id / username / source，如果设备可提供。
- 执行前必须再次确认锁仍被同一 session 占用。
- 执行后必须重新 refresh 当前设备状态。
- 执行后不能直接复用旧 diff。

Force Unlock 对 NETCONF 设备可通过 `<kill-session>` 或厂商等价机制实现。

如果设备不支持安全识别锁持有者，则不得自动执行 Force Unlock。

## 12. Journal 与 Artifact GC 需求

Transaction Journal 和 rollback artifact 必须持久化，但不能无限增长。

系统必须提供异步 GC 任务和 retention policy。

推荐默认策略：

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
- `Failed` 事务保留更久，便于排障。
- `InDoubt` 事务不得自动删除，必须人工 resolve。
- rollback artifact 只有在关联事务进入 terminal 状态后才允许删除。
- 删除 artifact 前必须确认无 pending recovery 依赖。
- GC 必须记录结构化日志和审计事件。

## 13. 安全与凭据需求

设备凭据不得硬编码在 intent 或普通资源模型中。

必须使用：

```text
secret_ref
```

第一版可以支持本地 secret provider，但接口上必须保留未来接入 Aria Secret Store 的能力。

Adapter 只接收解析后的短期凭据或 secret reference，不应长期保存明文密码。

日志和审计中必须脱敏：

- password。
- private key。
- token。
- enable password。
- SNMP community。

## 14. 第一阶段功能范围

第一版只做：

- 2 台交换机。
- device registration。
- device onboarding。
- VLAN。
- interface description。
- access port。
- trunk port。
- NETCONF candidate / validate / commit。
- confirmed-commit，如果设备支持。
- running / CLI best-effort rollback 降级。
- dry-run。
- diff。
- transaction journal。
- recovery。
- periodic drift auditor。
- lock acquisition strategy。
- break-glass force unlock，默认关闭。
- journal / rollback artifact GC。
- capability report。
- structured audit events。

暂不做：

- ACL。
- VRF。
- 静态路由。
- EVPN。
- QoS。
- 大规模自动 fabric 编排。
- 全厂商全功能覆盖。

## 15. 第一版验收标准

必须满足：

- 能注册设备到 inventory。
- 设备注册后进入 onboarding。
- 能探测两台交换机 capability。
- 能根据 capability 选择事务策略。
- 未完成 onboarding 的设备不能进入配置事务。
- 两台交换机同时创建 VLAN 成功。
- 两台交换机同时删除 VLAN 成功。
- interface description 可正确设置。
- access port 可正确设置。
- trunk port 可正确设置。
- 同一 intent 连续 apply 10 次，后 9 次不产生设备变更。
- 单台 Prepare 失败时，另一台不进入 Commit。
- 单台 Validate 失败时，另一台 running 不变。
- confirmed-commit 阶段单台失败时，已 confirmed 的设备自动 cancel。
- running / CLI 降级模式下，修改前生成 rollback artifact。
- 降级模式失败后，能够执行 best-effort rollback。
- 无法判断最终状态时，必须标记 `InDoubt`。
- 周期 Drift Auditor 能发现带外 VLAN / interface 变更。
- `BlockNewTransaction` 策略下，存在漂移的设备不能继续下发事务。
- lock 被占用时，系统按指数退避重试，并在超时后产生结构化事件。
- break-glass force unlock 默认关闭，开启后必须产生完整审计。
- 已提交事务 journal 和 rollback artifact 能按 retention policy 清理。
- `InDoubt` 事务及其 artifact 不会被自动 GC。
- 所有降级事务必须返回 warning。
- 所有设备错误必须结构化上报。
- 所有事务事件必须进入 RFC-002 事件模型。
- 所有配置变更必须进入 RFC-015 审计视图。

## 16. 非目标

第一阶段不追求：

- 完整 SDN 控制器。
- OpenFlow。
- EVPN 自动化。
- 多站点 fabric 全自动规划。
- 任意厂商任意功能一次性覆盖。
- 将 CLI fallback 包装成强事务能力。

## 17. 架构结论

面向 ToB 多厂商真实交付场景，`aria-underlay` 应采用：

```text
Rust Underlay Core
+ gRPC / Protobuf Contract
+ Python Underlay Adapter
+ Vendor Driver Plugin System
+ ncclient / NAPALM / Netmiko
+ Transaction Journal
+ Rollback Artifact Store
+ Capability-based Strategy Selection
+ RFC-002 Event Integration
+ RFC-015 Audit Integration
```

核心原则：

```text
Rust 负责事务语义和平台一致性
Python 负责厂商适配和设备脏活
```

Python 可以处理复杂设备适配，但不能吞掉事务语义。

最终事务状态、降级策略、审计、告警和结果判定，必须由 Rust 主控制面掌握。
