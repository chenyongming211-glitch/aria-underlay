# 当前缺陷 / 技术债清单 — 2026-05-04

## 当前基线

最新有效基线：`main` / `8a9b0d1 docs: refresh current bug inventory baseline`。

GitHub Actions：`25326262048`，结论 `success`。

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
| targeted code review | 2026-05-04 已审查 recovery、journal/shadow、drift、adapter pool、parser/validator 无真机路径 | 按下方新增 bug 分包修复，修完再继续 fixture 边界测试 |
| NETCONF state driver 输出校验 | 已确认 `GetCurrentState` 的 parser 输出到 protobuf 映射仍有 fail-closed 缺口 | 修复 admin_state 归一化和 malformed parser output 结构化错误返回 |
| parser / renderer fixture 边界 | Huawei/H3C 已有样本和负例，但不能替代真机 | 继续补 XML namespace、字段缺失、非法 VLAN、重复接口、unknown mode 等边界样本 |
| recovery backoff | recover RPC transient retry 已有固定短重试 | 如后续 CI 或现场暴露抖动，再做可配置 backoff；当前不是阻塞项 |
| intent validation | VLAN/interface/domain 基础校验已补 | 后续新增 ACL/VRF/route/BGP 等模型时同步补校验 |
| 文档一致性 | 当前范围已收敛 | 后续只做小范围状态刷新，避免旧历史计划误导开发 |

### 2026-05-04 targeted code review 新增确认缺陷

| 优先级 | 缺陷 | 证据 | 修复方案 |
| --- | --- | --- | --- |
| P2 | `NetconfBackedDriver.get_current_state()` 将解析出的缺省、未知或大小写变体 `admin_state` 静默映射为 `ADMIN_STATE_DOWN` | `adapter-python/aria_underlay_adapter/drivers/netconf_backed.py:73-76` 只判断 `iface["admin_state"] == "up"`；fixture parser 在 `state_parsers/common.py:63` 允许 `admin-state` 缺省；本地复现显示 `admin_state=None` 输出 protobuf 值 `2` | 增加统一的 observed admin-state 归一化函数：`None`/空值按项目已有语义归一为 `up`，`up/down` 大小写归一，未知值返回 `AdapterError(code="NETCONF_STATE_PARSE_FAILED")`；补 driver 单元测试覆盖 `None`、`UP`、`down`、未知值 |
| P2 | `GetCurrentState` 的 parser output 到 protobuf 映射不在 `AdapterError` 捕获范围内，malformed parser output 会逃逸成未结构化异常 | `adapter-python/aria_underlay_adapter/drivers/netconf_backed.py:53-58` 只捕获 backend state read；`netconf_backed.py:60-83` 的 list comprehension 和 `_port_mode_to_proto()` 在 try 外；本地复现 `mode.kind="hybrid"` 直接抛出 `AdapterError` traceback | 将 parsed-state 到 protobuf 的转换移动到同一个 `try` 内，或抽出 `_observed_state_to_proto()` 并统一捕获 `AdapterError` / `KeyError` / `TypeError`；同时加强 `netconf_state._validate_observed_state_shape()` 对 VLAN/interface 必填字段、admin_state、mode kind、VLAN 范围的结构校验 |

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
3. 优先顺序：targeted code review 记录项修复 > fixture 边界测试 > 必要的 recovery/backoff 小修。
