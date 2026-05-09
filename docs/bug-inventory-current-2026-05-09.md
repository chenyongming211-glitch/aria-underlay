# 当前缺陷 / 技术债清单 — 2026-05-09

> 当前有效基线。本文在 `docs/bug-inventory-current-2026-05-07.md` 和
> `docs/bug-inventory-current-2026-05-05.md` 基础上，重新按当前 `main`
> 代码核实并分类。旧文件保留审查证据和历史上下文，但不要再直接作为 open bug
> 队列使用。

## 核实基线

- 代码：`main` / `10db2f7 test: expect acl creation before binding`
- 本地验证：`python3 -m pytest -q adapter-python/tests` -> `289 passed`
- 本地限制：当前机器 `cargo` 不在 `PATH`，Rust 结论来自源码核实；Rust CI 仍以
  GitHub Actions 为准。

## 当前部署边界

- 单个 active Rust Core 通过 NETCONF 写交换机。
- 可选 active-passive HA；不支持 active-active。
- 同一时刻只能有一个 Core active 写 journal/shadow 并对设备下发。
- Python Adapter 默认 loopback 部署；跨主机时必须有外部安全边界或后续 TLS/mTLS。
- 多设备 apply 是多个 endpoint transaction 的编排，不提供全局 all-or-nothing。

## Fixed in current branch, pending GitHub CI

| 项目 | 当前确认 | 主要证据 | 验证状态 |
| --- | --- | --- | --- |
| Product API action-level RBAC | Static bearer-token principal 已要求声明 `allowed_actions`；bearer session 会携带 action set；`ProductOpsApi` 不再对 bearer session 使用 `PermitAllAuthorizationPolicy`，而是用 request-scoped `StaticAuthorizationPolicy` 按 action 授权。 | `src/api/product_identity.rs`, `src/api/product_api.rs`, `src/authz.rs`, `docs/examples/product-api.*.json` | 本机无 `cargo`，需要 GitHub Actions 跑 Rust compile/test；已新增拒绝越权 audit export 的测试和 config allowed_actions 测试。 |

## Confirmed-open

这些项在当前代码中仍真实存在。是否阻塞发布取决于实际部署形态。

| 优先级 | 项目 | 当前确认 | 主要证据 | 建议 |
| --- | --- | --- | --- | --- |
| P0/条件阻塞 | Python Adapter gRPC 无 TLS/mTLS | server 只调用 `add_insecure_port(config.listen)`，配置也没有证书/client-auth 字段。 | `adapter-python/aria_underlay_adapter/server.py:163-169`, `adapter-python/aria_underlay_adapter/config.py:7-31` | 若 Core/Adapter 跨主机或网络不可信，先修；loopback/sidecar 部署可后置。 |
| P0/条件阻塞 | Candidate datastore prepare/commit 外部 TOCTOU | `prepare_candidate()` unlock 并关闭 session，`commit_candidate()` 后续新 session commit；外部 NETCONF writer 可在窗口内改 candidate。 | `adapter-python/aria_underlay_adapter/backends/netconf.py:211-277`, `adapter-python/aria_underlay_adapter/backends/netconf.py:417-474` | 若现场可能有外部 NETCONF writer，优先修；若保证独占写设备，可文档化后置。 |
| P1 | Confirmed-commit timeout watcher 缺失 | 已有 `with_confirmed_commit_timeout_secs` 和 commit 参数传递，但无后台 watcher 主动处理超时 journal。 | `src/api/service.rs:226-228`, `src/api/apply_coordinator.rs:550-556` | 增加后台监控或明确只依赖 recovery/manual ops。 |
| P1 | Worker panic/join error 仍终止 runtime | 普通 worker 返回错误已隔离，但 `JoinError` 会 shutdown 其他 worker 并返回 runtime error。 | `src/worker/runtime.rs:174-200` | 后续修成 panic 隔离和事件/报告记录。 |
| P1 | Journal GC 目录级/删除级失败仍全局失败 | 单个坏 journal 文件已跳过；`read_dir`、`remove_file`、artifact dir 遍历/删除错误仍直接返回 Err。 | `src/worker/gc.rs:210-264`, `src/worker/gc.rs:267-356` | 继续把目录/删除错误降级为报告字段，避免停止 runtime。 |
| P1 | Drift audit expected-store listing 失败仍全局失败 | 单设备 observed 失败已记录并继续；`expected_store.list()?` 失败仍中止整个审计。 | `src/worker/drift_auditor.rs:115-140` | 文件/存储层 listing 故障需要单独报告和事件化。 |
| P2 | `_persist_id_already_consumed` 保留 vendor 字符串 fallback | 已优先识别结构化 code/normalized_error，但仍 fallback 到 `"persist" + marker` 字符串匹配。 | `adapter-python/aria_underlay_adapter/drivers/netconf_backed.py:694-719` | 等真实厂商错误码覆盖后逐步收窄或按 vendor profile 限定。 |
| P2/功能缺口 | NETCONF force unlock 未实现 | Rust/API/RPC 已接线，Python real NETCONF driver 直接返回 `NOT_IMPLEMENTED`。 | `src/api/admin_ops.rs:75-105`, `adapter-python/aria_underlay_adapter/drivers/netconf_backed.py:305-312` | 只有需要 break-glass kill-session/force-unlock 时升优先级。 |

## Intentional-boundary

这些不是当前代码声称支持但实现错误，而是当前产品边界。不要误当作 bug 修。

| 项目 | 当前状态 | 说明 |
| --- | --- | --- |
| `EndpointLockTable` 跨节点互斥 | 不支持 | 代码是进程内 `DashMap<DeviceId, Mutex<()>>`。跨节点互斥依赖 active-passive lease/fencing。 |
| `JsonFileTxJournalStore` 跨进程文件锁 | 不支持 | store 只有进程内 per-tx mutex 和 atomic rename。跨进程单写者由 active-passive lease 保证。 |
| Active-active Core | 不支持 | 当前目标是 active-passive。不要为了 active-active 改事务锁，除非产品目标变更。 |
| 多设备全局原子事务 | 不支持 | 当前逐 endpoint 下发，顶层通过 `PartialSuccess` 和 `device_results` 表达混合结果。 |
| AutoReconcile | 不支持但 fail-closed | 遇到 drifted 设备时返回 `DRIFT_AUTORECONCILE_UNIMPLEMENTED`，不会假装自动修复。 |
| Rust AdapterClient 调用 Python `DryRun` RPC | 当前不用 | Rust dry-run 走本地 diff；若要求真实 NETCONF preflight，再升级为功能需求。 |
| `now_unix_secs` 秒级精度 | 可接受低优先级债 | 审计/排序排障精度有限，但当前不是事务正确性 bug。 |
| 非 H3C vendor 写路径 | 后置功能 | Cisco/Ruijie/NAPALM/Netmiko/SSH CLI 等待样本和明确需求。 |

## Stale-fixed

这些在旧清单中曾是 open bug，但当前代码已经修复或旧描述已经不准确。

| 旧项 | 当前确认 | 证据 |
| --- | --- | --- |
| 漂移检测未归一化比较 | 已修复 | `src/state/drift.rs:82-84` 已对 expected/observed 调用 `normalize_shadow_state`。 |
| rollback RPC 前写 journal 导致孤儿设备状态 | 已修复 | `src/api/apply_coordinator.rs:701-713` 先执行 adapter rollback，再写 `RollingBack` journal。 |
| 非 `InDoubt` recoverable 事务未阻塞新事务 | 已修复 | `src/tx/recovery.rs:38-48` 使用 `phase.requires_recovery()`。 |
| `InMemoryShadowStateStore::put` RMW 竞争 | 已修复 | `src/state/shadow.rs:64-76` 使用 DashMap entry。 |
| `DeviceInventory.insert` TOCTOU | 已修复 | `src/device/inventory.rs:22-32` 使用 DashMap entry。 |
| ConfirmedCommit recovery 只会 cancel-commit | 已修复 | `adapter-python/aria_underlay_adapter/drivers/netconf_backed.py:271-289` 先尝试 `final_confirm`，persist-id 已消费视为 committed。 |
| `DISCARD_PREPARED_CHANGES` 丢原始策略 | 已修复 | recovery rollback 保留 `strategy`，confirmed-commit 走对应 rollback strategy。 |
| `prepare_candidate` unlock 掩盖原始错误/dirty candidate | 已修复 | `adapter-python/aria_underlay_adapter/backends/netconf.py:248-273` 保存原始错误，unlock 失败附加诊断，必要时 best-effort discard。 |
| `NetconfBackedDriver` 非 `AdapterError` 异常逃逸 | 已修复 | driver 多个 RPC 分支已 `except Exception as exc` 并映射 `_unexpected_error`。 |
| confirmed-commit timeout 写死为 120 秒 | 部分已修 | 已有 service 配置入口和 commit 参数传递；timeout watcher 仍 open。 |
| rollback cleanup 覆盖 primary error | 已修复 | rollback 失败会作为 secondary diagnostic 附加，primary error 保留。 |
| 顶层缺 `PartialSuccess` | 已修复 | `src/api/apply.rs:62-69` 聚合 mixed outcome 为 `ApplyStatus::PartialSuccess`。 |
| Worker 普通错误终止全部 worker | 已修复 | worker 返回 Err 会记录到 `worker_errors`，不再直接停止 runtime；panic/join 仍 open。 |
| Journal GC 单个坏文件终止整轮 | 已修复 | malformed/unreadable 单文件记录 `journals_failed` 并继续。 |
| Drift audit 单设备 observed 失败中止全部审计 | 已修复 | 单设备失败记录到 `failed_devices` 并继续。 |
| H3C parser/renderer 不能标记 production-ready | 旧文档已过期 | H3C renderer/parser 已 `production_ready=True`，real-device runbook 记录 S5560/S6800 代表验证。Huawei 仍不是同等级生产就绪。 |

## 下一步建议

默认先做最小可验证切片，不一次性铺开所有 open 项：

1. 若 Product API 会被多个 operator 使用：先修 action-level RBAC。
2. 若 Core/Adapter 有跨主机部署：先修 adapter gRPC TLS/mTLS 或写入强制 sidecar/tunnel 配置边界。
3. 若现场可能有外部 NETCONF writer：先修 candidate prepare/commit TOCTOU。
4. 然后修 confirmed-commit timeout watcher。
5. 最后集中修 worker panic/join、GC 目录级失败、drift expected listing 失败。

当前不建议先做 active-active、跨设备全局事务、AutoReconcile 或非 H3C vendor 扩展。
