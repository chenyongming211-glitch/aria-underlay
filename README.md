# Aria Underlay

Aria Underlay 是 Aria 的物理交换机管控子系统，面向 ToB 私有化交付场景。

它的目标不是做脚本下发平台，而是把上层给出的标准网络意图，可靠、幂等、可回滚地落到客户机房里的物理交换机上。

## 架构

```text
Rust Underlay Core（主控 / 大脑）
    |
    | gRPC / Protobuf
    v
Python Underlay Adapter（适配器 / 双手）
    |
    +-- ncclient / NETCONF
    +-- NAPALM
    +-- Netmiko / SSH CLI
    |
    v
Physical Switches
```

## 职责边界

Rust 主控负责：

- 设备纳管和 inventory
- 标准 intent / desired state
- 全局事务状态机
- transaction journal
- drift auditor
- lock strategy
- journal / artifact GC
- 事件和审计对接

Python Adapter 负责：

- 厂商 driver
- NETCONF / NAPALM / Netmiko / SSH CLI 后端
- capability probe
- 设备级 diff
- rollback artifact
- 单设备 prepare / commit / rollback / verify

核心原则：

```text
Rust 负责事务语义和平台一致性
Python 负责厂商适配和设备脏活
```

## 当前阶段

当前仓库处于 Sprint 0 初始化阶段，已包含：

- Rust crate 骨架
- Python Adapter 骨架
- gRPC / Protobuf 初版契约
- Device Registration / Onboarding 初始模型
- GitHub Actions CI
- 需求、开发方案和详细实施计划文档

## 文档

- [需求说明](docs/aria-underlay-requirements.md)
- [开发方案](docs/aria-underlay-development-plan.md)
- [详细开发计划](docs/implementation-plan.md)

## 开发入口

本地如果没有 Rust 编译环境，可以直接依赖 GitHub Actions 做编译验证。

推荐开发顺序：

```text
1. 完善 proto/aria_underlay_adapter.proto
2. 跑通 Python fake Adapter
3. 跑通 Rust -> Python GetCapabilities
4. 完成 Device Registration / Onboarding 闭环
5. 再进入 NETCONF capability probe
```
