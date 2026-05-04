# 缺陷清单历史快照 — 2026-04-30

> 历史快照。当前 open bug 和技术债以 [当前 缺陷 / 技术债清单](./bug-inventory-current-2026-05-01.md) 为准。本文只保留复盘价值，不作为当前开发计划。

## 当时状态矩阵

| 优先级 | 问题 | 当前状态 | 影响区域 |
| --- | --- | --- | --- |
| P1 | drifted 生命周期 clean 审计 后未清理 | 已修复 | Rust drift/shadow |
| P1 | ShadowStateStore 混用 desired baseline 和 observed cache | 已修复 | Rust state model |
| P1 | file shadow store 路径 sanitize 可能碰撞 | 已修复 | Rust shadow store |
| P1 | file-backed journal/shadow 并发写和崩溃窗口弱 | 已修复 | Rust persistence |
| P1 | 产品初始化/注册接受非法连接输入 | 已修复 | Rust bootstrap / Python server |
| P1 | UnderlayError 诊断 details 丢失 | 已修复 | Rust error mapping |
| P2 | TrustOnFirstUse 语义不是真正 TOFU | 已修复为真实 TOFU store | Python NETCONF host key |
| P2 | Rust 渲染器骨架 死代码 | 已清理 | Rust device render |
| P2 | Python admin_state 文本转换重复且行为不同 | 已收敛 | Python 后端/渲染器 |
| P2 | Python 驱动 stub 构造即 panic | 已改为明确 unsupported | Python 驱动 registry |
| P2 | SmallFabric topology 缺少 endpoint 数量校验 | 已修复或文档化 | Rust intent validation |
| P2 | endpoint lock jitter 随机性不足 | 已修复 | Rust lock/backoff |
| P2 | scope VLAN ID 转换缺少防御错误信息 | 已修复 | Python 后端/渲染器 |

## 关键修复方向复盘

### 漂移状态生命周期清理

问题：clean 审计 后设备生命周期没有从 `Drifted` 回到正常状态。
修复方向：在 clean observation 持久化后更新 lifecycle，并补回归测试。

### Shadow 状态语义拆分

问题：同一个 shadow record 同时承载 desired baseline 和 observed cache，容易掩盖 drift。
修复方向：拆分 desired baseline 与 observed cache 的持久化语义。

### 文件持久化加固

问题：journal/shadow 使用固定临时文件或单写假设，崩溃和并发写入下不够稳。
修复方向：使用同目录唯一临时文件、原子 rename，并在需要时同步目录。

### 初始化输入校验

问题：注册和 bootstrap 接受非法 host、port、vendor、device id 等输入。
修复方向：共享输入校验，尽早 失败关闭。

### 错误上下文保留

问题：adapter 详细错误丢失，排障只能看泛化错误码。
修复方向：保留 bounded diagnostic details，并写入 journal / apply result。

### 主机密钥策略

问题：Rust 已有策略枚举，但 Python NETCONF 后端 没完全消费。
修复方向：protobuf 带上 主机密钥策略，Python 后端 执行 known-hosts、TOFU 或 unsupported 失败关闭。

## 已废弃的 2026-04-26 旧判断

2026-04-26 的部分问题在后续代码审查中被证明已修复、不可达或被更准确的问题替代。当前不再使用旧清单驱动开发。

## 当前处理规则

- 本文所有条目默认视为历史记录。
- 重新打开任何问题前，必须先读当前代码和最新 CI 结果。
- 当前内部系统范围已经收敛：不做产品级审计数据库、扩展 RBAC、token 生命周期、外部告警、UI、仓库内 ingress/TLS 或安装包。
