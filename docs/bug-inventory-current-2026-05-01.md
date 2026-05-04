# 当前缺陷 / 技术债清单 — 2026-05-04

## 当前基线

最新有效基线：`main` / `6efbf75 fix: validate netconf observed state output`。

GitHub Actions：`25328357356`，结论 `success`。

本地环境仍没有 `cargo` / `rustfmt`，Rust 编译和测试以 GitHub Actions 为准。当前产品方向已经收敛为内部系统：不做 SSO/OIDC/JWT/JWKS，不做 RBAC，不做产品 UI，不做外部告警投递，不在仓库内实现 ingress/TLS，不生成安装包。

## 已收敛完成的历史问题

以下问题已经通过前序修复包关闭，不应重复作为 open bug：

- 事务 committed 与 shadow 写入顺序。
- 批量恢复 TOCTOU。
- 部分失败被汇总为成功。
- journal 错误历史丢失。
- 孤儿密钥 清理。
- 适配器客户端 连接 churn。
- Drifted 生命周期清理。
- desired baseline 与 observed cache 混用。
- file-backed journal/shadow 并发和崩溃弱点。
- Rust 到 Python 的 主机密钥策略 传递。
- TOFU host-key store。
- Python placeholder 后端 改为明确 unsupported。
- Rust 死 渲染器骨架 清理。
- 轻量 operation 审计 log：append-only JSONL、关键字段、retention/rotation 和写失败可观测性。
- product API 身份收敛：静态 token 只映射 `operator_id`，已移除 role、issuer、subject、session_id、expiry 和 production ingress 模式。
- runtime 日志收敛：运行日志统一写入 `/var/log/aria/aria-underlay.log`，提供 systemd/logrotate 样例；不再要求运维手工用 CLI 查日志。
- 产品平台过度设计清理：旧 RBAC/JWT/OIDC/product audit DB/外部告警等方案已在文档中标记为历史方案，不作为后续开发依据。
- NETCONF state driver 输出校验：`GetCurrentState` 的 parsed-state 到 protobuf 映射已补 admin-state 归一化、非法 mode 结构化错误、VLAN/接口基本 shape 校验。
- parser / renderer fixture 边界增强：新增 Huawei/H3C `missing_port_mode`、`non_integer_access_vlan` 负例；renderer 端口模式大小写归一；snapshot 对未知 `admin_state` 返回结构化失败。
- 事务 crash/restart 核心矩阵：
  - process-level kill 覆盖 `Preparing`、`Committing`、`Verifying`、`FinalConfirming`、`RollingBack`。
  - recovery 覆盖 adapter session down、roll-forward 前 shadow 持久化、多设备 mixed outcome、recover RPC transient retry。
  - file-backed restart 覆盖 `Committed` / `Failed` / `RolledBack` 终态不参与 recovery。
  - atomic write `.tmp` 残留不参与 journal scan。
  - adapter 已确认 `Committed` 但 shadow 持久化失败时保持 `InDoubt`。

## 当前仍需要做：被真实设备或真实样本阻塞

| 区域 | 状态 | 下一步 |
| --- | --- | --- |
| 真实 NETCONF 解析器 | Huawei/H3C 仅 样本验证 | 等真实 XML 或真实交换机后验证，不提前 生产就绪 |
| 真实 NETCONF 渲染器 | Huawei/H3C 仍是 骨架/快照 验证 | 等真实设备验证后再讨论 生产就绪 |
| Vendor scope | Cisco/Ruijie 未实现 | 等样本和明确需求 |
| Alternate 后端s | NAPALM/Netmiko/SSH CLI 未实现 | 等明确 后端 合同和测试需求 |
| Force unlock | NETCONF kill-session/force-unlock 未实现 | 等设备会话身份和审计需求明确后再设计 |

## 当前仍需要做：无真实交换机也可以继续的小范围工作

| 区域 | 状态 | 下一步 |
| --- | --- | --- |
| targeted code review | 2026-05-04 已审查 recovery、journal/shadow、drift、adapter pool、parser/validator 无真机路径；新增 P2 state 输出缺陷已修复 | 继续 fixture 边界测试 |
| parser / renderer fixture 边界 | Huawei/H3C 已有样本、负例和 snapshot 边界测试，但不能替代真机 | 后续只随真实样本或明确 bug 增量补，不作为持续扩功能入口 |
| recovery backoff | recover RPC transient retry 已有固定短重试 | 如后续 CI 或现场暴露抖动，再做可配置 backoff；当前不是阻塞项 |
| intent validation | VLAN/interface/domain 基础校验已补 | 后续新增 ACL/VRF/route/BGP 等模型时同步补校验 |
| 文档一致性 | 当前范围已收敛 | 后续只做小范围状态刷新，避免旧历史计划误导开发 |

### 2026-05-04 targeted code review 已修复缺陷

| 优先级 | 缺陷 | 证据 | 修复方案 |
| --- | --- | --- | --- |
| P2 | `NetconfBackedDriver.get_current_state()` 将解析出的缺省、未知或大小写变体 `admin_state` 静默映射为 `ADMIN_STATE_DOWN` | 已补测试覆盖 `None`、`UP`、`down`、未知值 | 已修复：`None`/空值按既有语义归一为 `up`，`up/down` 大小写归一，未知值返回 `NETCONF_STATE_PARSE_FAILED` |
| P2 | `GetCurrentState` 的 parser output 到 protobuf 映射不在 `AdapterError` 捕获范围内，malformed parser output 会逃逸成未结构化异常 | 已补测试覆盖 `mode.kind="hybrid"` | 已修复：parsed-state 到 protobuf 转换纳入同一 `AdapterError` 捕获边界，并补 VLAN/interface/mode 基本 shape 校验 |

## 当前明确保持 unsupported / fail-closed

| 区域 | 状态 | 当前处理 |
| --- | --- | --- |
| 主机密钥策略 | fingerprint-only pinning 仍未完整实现 | 暂不扩展；保持 失败关闭 |
| Drift policy | AutoReconcile 明确未实现 | 暂不开发；保持 unsupported |
| parser / renderer production_ready | 无真实设备验证 | 不允许标记生产就绪 |

## 明确不做

- product 审计 database。
- RBAC。
- token 创建、轮换、撤销工具。
- SSO/OIDC/JWT/JWKS。
- 产品 UI。
- 外部告警投递。
- 仓库内 ingress/TLS/client-auth/rate-limit/proxy-header。
- deb/rpm/tar 安装包或多平台 installer。

## 当前执行计划

1. 没有真实交换机前，不推进 解析器/渲染器 生产化。
2. 后续只做小范围清理和回归测试加固，不继续扩展产品平台能力。
3. 优先顺序：必要的 recovery/backoff 小修；其余等待真实样本或真实交换机证据。
