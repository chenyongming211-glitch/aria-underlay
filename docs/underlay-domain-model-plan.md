# Underlay Domain 模型演进计划

## 1. 结论

`aria-underlay` 不应长期绑定为“固定两台交换机”模型。

真实 ToB 现场更准确的抽象是：

```text
Underlay 管控域
  -> 少量管理 endpoint
  -> 少量交换机成员
  -> VLAN / interface / binding intent
```

第一阶段仍兼容现有 `SwitchPairIntent`，但后续主模型应演进为：

```text
UnderlayDomainIntent
```

交换机规模定位：

```text
small underlay domain
```

这里不设置硬编码数量上限。`aria-underlay` 的产品定位是小规模物理交换机管控，不做大规模 fabric controller，不做自动 EVPN fabric 编排。实际注册数量和一次 apply 的规模后续由产品配额、部署规格、并发配置和运维策略控制，不在 planner 中用固定数字拒绝。

## 2. 必须覆盖的现场形态

### 2.1 堆叠 / 虚拟化集群

典型形态：

```text
2 台物理交换机
1 个管理 IP
1 个 NETCONF endpoint
对外表现为 1 台逻辑交换机
```

事务含义：

```text
单 endpoint 事务
```

也就是说，虽然物理上有 2 台设备，但 Aria Underlay 的南向原子操作目标只有 1 个 endpoint。

### 2.2 MLAG / 双 ToR

典型形态：

```text
2 台物理交换机
2 个管理 IP
2 个 NETCONF endpoint
```

事务含义：

```text
两个独立单 endpoint 事务
```

这是当前 `LeafA / LeafB` 模型最接近的场景。MLAG 两台设备可以在一次批量 intent 中同时出现，但原子性边界仍然是单个 management endpoint；其中一台失败时，应记录失败并支持单独重试，而不是要求另一台回滚成跨设备强一致事务。

### 2.3 小规模多交换机

典型形态：

```text
多台交换机
每台或部分交换机有独立管理 IP
```

事务含义：

```text
N 个独立单 endpoint 事务
```

该模式仍属于小规模私有化交付，不升级为完整 SDN fabric controller。

## 3. 核心抽象

### 3.1 UnderlayTopology

```rust
pub enum UnderlayTopology {
    StackSingleManagementIp,
    MlagDualManagementIp,
    SmallFabric,
}
```

含义：

| Topology | 管理 endpoint | 说明 |
| --- | --- | --- |
| `StackSingleManagementIp` | 1 | 堆叠/虚拟化集群，对外一个管理面 |
| `MlagDualManagementIp` | 2 | 双 ToR / MLAG |
| `SmallFabric` | 多个 | 少量交换机的小型管控域，不设硬编码数量上限 |

### 3.2 ManagementEndpoint

```rust
pub struct ManagementEndpoint {
    pub endpoint_id: String,
    pub host: String,
    pub port: u16,
    pub secret_ref: String,
    pub vendor_hint: Option<Vendor>,
    pub model_hint: Option<String>,
}
```

`ManagementEndpoint` 是真正的 NETCONF / SSH 连接对象。

原子操作目标按 endpoint 计算，而不是按物理交换机成员计算。更准确地说，原子事务边界是单个 endpoint；多个 endpoint 的 apply 属于批量编排，不承诺跨 endpoint 原子提交。

### 3.3 SwitchMember

```rust
pub struct SwitchMember {
    pub member_id: String,
    pub role: Option<DeviceRole>,
    pub management_endpoint_id: String,
}
```

`SwitchMember` 表示逻辑管控域内的交换机成员。

堆叠场景下：

```text
member-a -> endpoint-stack
member-b -> endpoint-stack
```

MLAG 场景下：

```text
leaf-a -> endpoint-leaf-a
leaf-b -> endpoint-leaf-b
```

### 3.4 UnderlayDomainIntent

```rust
pub struct UnderlayDomainIntent {
    pub domain_id: String,
    pub topology: UnderlayTopology,
    pub endpoints: Vec<ManagementEndpoint>,
    pub members: Vec<SwitchMember>,
    pub vlans: Vec<VlanIntent>,
    pub interfaces: Vec<InterfaceIntent>,
}
```

后续可以扩展：

```text
bindings
mlag peer settings
stack member metadata
uplink groups
```

## 4. Planner 调整

当前：

```text
SwitchPairIntent
  -> DeviceDesiredState per switch
```

目标：

```text
UnderlayDomainIntent
  -> EndpointDesiredState per management endpoint
```

命名建议：

```rust
pub struct EndpointDesiredState {
    pub endpoint_id: String,
    pub vlans: BTreeMap<u16, VlanConfig>,
    pub interfaces: BTreeMap<String, InterfaceConfig>,
}
```

在第一阶段可以继续复用 `DeviceDesiredState`，但语义上应逐步改为 endpoint desired state。

## 5. 事务调整

原子事务边界：

```text
management endpoint
```

不是：

```text
physical switch member
```

因此：

- 堆叠单 IP：1 个 endpoint，1 个单设备原子事务。
- MLAG 双 IP：2 个 endpoint，2 个彼此独立的单设备原子事务。
- 小规模多交换机：多个 endpoint，多个彼此独立的单设备原子事务。

一次 `UnderlayDomainIntent` 可以描述多个 endpoint 的期望状态，但它不是全局分布式事务。执行层可以串行或按配置并发下发，每个 endpoint 独立保证：

- diff 前置。
- candidate / validate / confirmed-commit，如果设备支持。
- rollback / recovery / InDoubt。
- journal 和审计。

跨 endpoint 不做强 2PC，不承诺“一起成功或一起回滚”。如果 MLAG 的一台设备成功、另一台失败，系统必须返回部分失败、记录审计，并允许失败设备单独重试。

这会影响：

- device lock table。
- NETCONF session pool。
- tx journal endpoint list。
- drift auditor scan list。
- dry-run change set 按 endpoint 输出。

## 6. 兼容策略

短期不删除：

```rust
SwitchPairIntent
DeviceRole::LeafA
DeviceRole::LeafB
```

短期策略：

```text
SwitchPairIntent 作为 MlagDualManagementIp 的兼容包装
```

后续新增：

```rust
UnderlayDomainIntent
```

再逐步将 API 从：

```rust
ApplyIntentRequest { intent: SwitchPairIntent }
```

演进为：

```rust
ApplyDomainIntentRequest { intent: UnderlayDomainIntent }
```

## 7. 开发顺序

1. 文档更新，明确 Underlay Domain 模型。
2. 新增 Rust domain intent structs，不替换现有 API。
3. 新增 domain validation。
4. 新增 domain planner。
5. 增加堆叠单 IP测试：
   - 2 个 member。
   - 1 个 endpoint。
   - 输出 1 个 desired state。
6. 增加 MLAG 测试：
   - 2 个 member。
   - 2 个 endpoint。
   - 输出 2 个 desired state。
7. 增加 small fabric 测试：
   - 3 个 endpoint。
   - 输出 3 个 desired state。
8. 再考虑 API 迁移。

## 8. 当前风险

如果继续只强化 `SwitchPairIntent`，后面会遇到：

- 堆叠单 IP 无法自然表达。
- 原子事务边界、管理 endpoint 与物理交换机数量混淆。
- journal 记录里 `device_id` 语义不清。
- drift auditor 不知道该按 member 还是 endpoint 扫描。
- 小规模多交换机需要重新拆 API。

因此本阶段必须先补 domain 模型，再继续事务层。
