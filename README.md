# Aria Underlay

Aria Underlay 是 Aria 的物理交换机管控子系统，面向 ToB 私有化交付场景。

它的目标不是做脚本下发平台，而是把上层给出的标准网络意图，可靠、幂等、可回滚地落到客户机房里的物理交换机上。

强约束：配置下发必须按事务系统设计。单 management endpoint 的配置事务必须尽最大工程能力满足 ACID 四个特性：Atomicity、Consistency、Isolation、Durability。多个 endpoint 的一次 apply 是批量编排，不承诺跨 endpoint 全局 ACID，但每个 endpoint 内部必须独立满足 ACID，并且批量结果必须可审计、可重试、可恢复。

## 架构

```text
Rust Underlay Core（主控 / 大脑）
    |
    | gRPC / Protobuf
    v
Python Underlay Adapter（适配器 / 双手）
    |
    +-- ncclient / NETCONF（当前可用路径）
    +-- NAPALM（规划中，未实现）
    +-- Netmiko / SSH CLI（规划中，未实现）
    |
    v
Physical Switches
```

## 职责边界

Rust 主控负责：

- 设备纳管和 inventory
- 标准 intent / desired state
- 单 endpoint 事务状态机
- 多 endpoint 批量编排
- transaction journal
- drift auditor
- lock strategy
- journal / artifact GC
- 事件和审计对接

Python Adapter 负责：

- 厂商 driver
- NETCONF 后端
- NAPALM / Netmiko / SSH CLI 降级后端规划
- capability probe
- 厂商 renderer / running state parser
- 设备级 prepare / commit / rollback / verify

核心原则：

```text
Rust 负责事务语义和平台一致性
Python 负责厂商适配和设备脏活
```

## 当前状态

当前仓库已经越过 Sprint 0 骨架阶段，重点在“无真实交换机条件下把事务可靠性、适配器边界和厂商 XML 工具链做稳”。

已经具备：

- Rust Core：intent validation、planner、diff/normalize、事务 journal、endpoint lock、recovery、drift audit、GC worker、force-resolve、adapter client pool。
- Python Adapter：fake/mock backend、NETCONF backend、fail-closed renderer/parser registry、TOFU known-host trust store、dry-run preflight、offline state parser validator、offline renderer snapshot validator。
- gRPC / Protobuf：Rust Core 和 Python Adapter 的 10 个设备操作 RPC 契约。
- CI：Rust、Python adapter、fake adapter integration matrix 已接入 GitHub Actions。

仍然明确不是生产完成：

- Huawei/H3C renderer 仍是 skeleton，只能离线 snapshot，默认不能进入真实 `prepare` 下发路径。
- Huawei/H3C running state parser 只是 fixture-verified，仍需要真实设备 XML 验证后才能 `production_ready=True`。
- Cisco/Ruijie renderer/parser 未实现。
- NAPALM / Netmiko / SSH CLI 后端未实现。
- fingerprint-only host-key pinning、完整现场 audit/metrics、真实设备联调仍是后续工作。

## 文档

- [需求说明](docs/aria-underlay-requirements.md)
- [开发方案](docs/aria-underlay-development-plan.md)
- [详细开发计划](docs/implementation-plan.md)

## 开发入口

本地如果没有 Rust 编译环境，可以直接依赖 GitHub Actions 做编译验证。

推荐开发顺序：

```text
1. 保持事务正确性优先：新增功能必须补 crash/recovery、journal/shadow、drift/verify 相关测试。
2. 在没有真实交换机时，继续强化 parser fixture、renderer snapshot、dry-run、audit/metrics 和代码边界。
3. 真实交换机到位后，先采集 running XML 并验证 parser，再验证 renderer 下发。
4. 只有真实样本和测试闭环通过后，才允许厂商 renderer/parser 提升到 production_ready=True。
```
