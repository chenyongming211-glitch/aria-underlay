# 当前缺陷 / 技术债清单 — 2026-05-07

## 当前需求边界

目标部署形态：

- 单个 active Rust Core 节点通过 NETCONF 事务性配置交换机。
- 可选两节点 HA，但推荐 active-passive，不做 active-active。
- 同一时刻只能有一个 Core active 写 journal/shadow 并对同一设备下发。
- Python Adapter 可与 Core 同机部署；若跨主机通信，需要额外安全边界。

本文是在 `docs/bug-inventory-current-2026-05-05.md` 基础上的部署目标复核。05-05 清单仍保留完整源码审查结果；本文只记录和当前需求直接相关的确认项、增量项和修复优先级。

## 发布前必须修复：事务正确性

### 1. 漂移检测未归一化比较

**Status:** 已在 2026-05-05 清单记录，当前需求下仍是发布前阻塞项。

**Files:** `src/state/drift.rs:75`, `src/engine/normalize.rs:70`

**Why it matters:** `BlockNewTransaction` 会依赖 drift 状态阻止新事务。若接口别名、空字符串、trunk VLAN 顺序差异被误判为漂移，会错误阻塞真实下发。

**Fix direction:** `detect_drift` 比较前对 expected/observed 调用 `normalize_shadow_state`。

### 2. rollback RPC 前写 journal 导致孤儿设备状态

**Status:** 已在 2026-05-05 清单记录，当前需求下仍是发布前阻塞项。

**Files:** `src/api/apply_coordinator.rs:623-624`

**Why it matters:** 若 journal 写 `RollingBack` 失败，adapter rollback 根本不会发出，设备可能保留 prepared/candidate 状态；恢复也可能看不到需要清理的状态。

**Fix direction:** 先尽最大努力调用 adapter rollback，再写 journal 结果。若 journal 写失败，设备侧至少已经收到 rollback。

### 3. 非 `InDoubt` 可恢复事务未阻塞新事务

**Status:** 已在 2026-05-05 清单记录，当前需求下仍是发布前阻塞项。

**Files:** `src/tx/recovery.rs:38-48`, `src/api/apply_coordinator.rs:662-680`

**Why it matters:** 进程崩溃后停在 `Prepared`、`Committing`、`FinalConfirming`、`RollingBack` 等阶段时，新 apply 仍可能操作同一设备，和旧 confirmed-commit 窗口重叠。

**Fix direction:** 将 gate 从 `in_doubt_records_for_devices` 扩展为阻塞所有 `phase.requires_recovery()` 的同设备事务。

### 4. `InMemoryShadowStateStore::put` RMW 竞争

**Status:** 已在 2026-05-05 清单记录，当前需求下仍建议发布前修复。

**Files:** `src/state/shadow.rs:55-65`

**Why it matters:** 默认 service 构造函数使用 InMemory shadow。即使生产最终用 file-backed store，默认路径也不应在并发下静默丢状态。

**Fix direction:** 使用 DashMap entry API 或每设备 mutex 原子化 revision read-modify-write。

### 5. NETCONF recover 对 confirmed-commit 丢失原始策略

**Status:** 已在 2026-05-05 清单记录，当前需求下仍是发布前阻塞项。

**Files:** `adapter-python/aria_underlay_adapter/drivers/netconf_backed.py:251-254`

**Why it matters:** confirmed-commit 的 prepared changes 已在 running 并由设备 timer 管理。将 `DISCARD_PREPARED_CHANGES` 强制映射为 candidate discard 不能取消 pending confirmed-commit。

**Fix direction:** recover 保留原始 transaction strategy。ConfirmedCommit rollback 应走 `cancel_commit(persist_id=tx_id)`。

### 6. `prepare_candidate` unlock 错误掩盖原始错误并可能留下 dirty candidate

**Status:** 已在 2026-05-05 清单记录，当前需求下仍是发布前阻塞项。

**Files:** `adapter-python/aria_underlay_adapter/backends/netconf.py:217-235`

**Why it matters:** edit/validate 失败时 unlock 异常会覆盖真实根因；edit+validate 成功但 unlock 失败时可能保留 locked dirty candidate。

**Fix direction:** 捕获 unlock 错误并附加到原始错误，不替换原始错误。若 unlock 失败且存在 candidate 变更，尽量 discard 清理。

### 7. `NetconfBackedDriver` 未兜底非 `AdapterError` 异常

**Status:** 2026-05-07 新增确认。

**Files:** `adapter-python/aria_underlay_adapter/drivers/netconf_backed.py`

**Why it matters:** `prepare`、`commit`、`final_confirm`、`rollback`、`verify`、`recover` 等路径只捕获 `AdapterError`。renderer/backend 中的 `ValueError`、`TypeError` 或其他未归类异常会逃逸到 gRPC 层，Rust 侧只能看到 transport/RPC failure，而不是结构化 `AdapterResult.errors[]`。

**Fix direction:** 在 driver 层增加统一 unexpected-exception mapper，所有 RPC 返回对应 response，`result.status=FAILED`，错误码类似 `ADAPTER_INTERNAL_ERROR`，保留 bounded raw summary。

### 8. Confirmed-commit timeout 写死为 120 秒

**Status:** 2026-05-07 新增确认；和 2026-05-05 的 confirmed-commit timeout watcher 技术债相关。

**Files:** `src/adapter_client/client.rs:177-190`

**Why it matters:** 现场设备、链路延迟和变更规模可能要求不同 confirmed-commit timer。Core 固定传 `120`，无法按部署或设备 profile 调整。

**Fix direction:** 先增加全局/配置级 `confirm_timeout_secs`，再考虑按设备 capability/profile 覆盖。后续可补 watcher 主动监控超时。

## HA / 部署边界必须明确

### 9. `JsonFileTxJournalStore` 无跨进程锁

**Status:** 2026-05-07 新增确认。

**Files:** `src/tx/journal.rs:208-260`

**Scope:** 仅在多个 Core 进程/节点同时写同一 journal 目录时成立。

**Why it matters:** 进程内 `Mutex` 不能保护另一个进程。若 HA 误配置成 active-active 或 split-brain，同一 tx_id 可能被并发写入。

**Fix direction:** 当前目标采用 active-passive。实现层面至少增加启动期 active lease / lock 文件防重入；如共享目录可能被多进程写，给 JsonFile journal 增加跨进程 advisory lock。

### 10. `EndpointLockTable` 仅进程内互斥

**Status:** 2026-05-07 新增确认。

**Files:** `src/tx/endpoint_lock.rs:12-15`

**Scope:** 单 active Core 下是正确实现；多 active Core 下不成立。

**Why it matters:** 两个 active Core 会各自持有本地 mutex，仍可能同时对同一交换机下发。

**Fix direction:** 不为当前版本实现 active-active。通过 active-passive lease/fencing 保证只有一个 Core active；文档和启动校验要明确 `EndpointLockTable` 不提供跨节点互斥。

### 11. Python Adapter gRPC 只支持 insecure port

**Status:** 2026-05-07 新增确认。

**Files:** `adapter-python/aria_underlay_adapter/server.py:163-169`, `adapter-python/aria_underlay_adapter/config.py`

**Scope:** adapter 仅监听 `127.0.0.1` 时可接受；跨主机或非信任网络暴露时是安全缺口。

**Why it matters:** 当前默认 listen 是 `127.0.0.1:50051`。如果 Core 与 Adapter 分布在不同节点，必须引入 TLS/mTLS、隧道或受信任网络隔离。

**Fix direction:** 当前最小 HA 建议 Core 与 Adapter 同机，或让 adapter 仅绑定 loopback。若必须跨主机，增加 TLS/mTLS 配置或由外部 sidecar/隧道提供加密和认证。

## 语义 / 可观测性，可后置

### 12. 多设备部分成功聚合为总体 `Failed`

**Status:** 2026-05-07 新增确认。

**Files:** `src/api/apply.rs:28-64`

**Scope:** 若产品约定批量 apply 不提供跨 endpoint 原子性，则这不是单设备结果计算错误；`device_results` 仍保留明细。

**Why it matters:** 仅看顶层 status 会丢失“部分成功”的语义，运维和上层调用方可能误读。

**Fix direction:** 后续可新增 `ApplyStatus::PartialSuccess`，或在 API 文档中明确顶层 `Failed` 代表“批量未全成功，详情看 device_results”。

### 13. Product API token 缺少动作级授权

**Status:** 2026-05-07 复核确认；是否修复取决于产品暴露范围。

**Files:** `src/api/product_api.rs:182-188`

**Scope:** 如果产品 API 只由本机可信运维调用，可后置。若多个操作者共享或需要最小权限，必须修。

**Fix direction:** 将 principal scope/role 引入 authorization policy，替换 `PermitAllAuthorizationPolicy`。

### 14. InMemory telemetry poison 后 panic

**Status:** 2026-05-07 新增确认，低优先级。

**Files:** `src/telemetry/sink.rs`, `src/telemetry/audit.rs`, `src/telemetry/alerts.rs`

**Scope:** 仅影响相关 InMemory 类型和 poison 场景。生产若使用 file/noop/stderr store，影响有限。

**Fix direction:** 将 `expect` 改为错误记录或 fail-closed 返回值；不作为当前 NETCONF 事务发布阻塞项。

## 旧文档中已经记录的其他 open / deferred 项

以下内容已经存在于旧 docs 中，和当前需求不完全重叠，但仍有追踪价值：

| 来源 | 条目 | 当前判断 |
| --- | --- | --- |
| `bug-inventory-current-2026-05-05.md` | DeviceInventory.insert TOCTOU | 仍是代码级并发 bug，建议随事务正确性批次修复，但不一定阻塞单节点首发。 |
| `bug-inventory-current-2026-05-05.md` | Candidate datastore prepare/commit TOCTOU | 真实设备上仍有外部 NETCONF 客户端竞争窗口；若现场保证只有本系统写设备，可后置但需文档化。 |
| `bug-inventory-current-2026-05-05.md` | Worker 单点失败终止全部 worker | HA/运维可靠性问题，建议在事务核心之后修。 |
| `bug-inventory-current-2026-05-05.md` | GC 单文件错误致命传播 | 会放大成 worker runtime 停止，建议和 worker 单点失败一起修。 |
| `bug-inventory-current-2026-05-05.md` | 漂移审计单设备错误中止全部审计 | 影响运维可观测性，建议在 drift 归一化之后修。 |
| `bug-inventory-current-2026-05-05.md` | rollback_after_endpoint_failure 总是返回 Ok | 会掩盖 rollback 失败，与 rollback 顺序修复同批处理。 |
| `bug-inventory-current-2026-05-05.md` | shadow 写在 journal Committed 之前 | 05-01 文档曾列为已修复，但 05-05 复核再次指出当前代码仍有风险；实现前需按当前代码重新验证。 |
| `bug-inventory-current-2026-05-05.md` | AdapterClientPool.client TOCTOU | 资源浪费/并发冷启动问题，低于事务状态机优先级。 |
| `bug-inventory-current-2026-05-05.md` | Mock 后端与真实后端行为偏离 | 测试可信度问题，建议在 Python adapter 修复批次中顺手对齐。 |
| `bug-inventory-current-2026-05-05.md` | list_in_doubt_transactions 隐藏其他 recoverable 记录 | 运维可见性问题，和“非 InDoubt gate”相关，建议改名或新增 pending listing。 |
| `bug-inventory-current-2026-05-05.md` | now_unix_secs 秒级精度 | 审计排序/排障质量问题，可后置。 |
| `bug-inventory-current-2026-05-05.md` | journal 文件名 sanitize 碰撞 | 当前 UUID tx_id 基本不可触发；建议做 tx_id 字符集校验。 |
| `bug-inventory-current-2026-05-05.md` | DryRun RPC proto 定义但 Rust AdapterClient 未包装 | 当前 Rust dry-run 走本地 diff，不调用 adapter preflight；若要求真实 NETCONF dry-run 预检，应升优先级。 |
| `bug-inventory-current-2026-05-01.md` | Huawei/H3C parser/renderer 未真机验证 | 当前仍成立。没有真实 XML/设备前不能标记 production_ready。 |
| `bug-inventory-current-2026-05-01.md` | Cisco/Ruijie 未实现 | 当前仍成立，等待样本和明确需求。 |
| `bug-inventory-current-2026-05-01.md` | NAPALM/Netmiko/SSH CLI 未实现 | 当前需求只要求 NETCONF，可继续后置。 |
| `bug-inventory-current-2026-05-01.md` | Force unlock 未实现 | 非当前事务下发主路径，可后置。 |
| `bug-inventory-2026-04-30.md`, `bug-inventory-2026-04-26.md` | 多个历史缺陷 | 这些文件明确标记为历史快照，不应直接驱动当前修复；重新打开前必须按当前代码复核。 |

## 建议修复批次

### 批次 A：单节点 NETCONF 事务可信度

1. Drift 归一化。
2. recoverable transaction gate。
3. rollback 顺序和 rollback 失败暴露。
4. shadow / inventory RMW 竞争。
5. Python driver unexpected exception 结构化。
6. confirmed-commit recover strategy。
7. prepare_candidate unlock/dirty candidate 处理。
8. confirmed-commit timeout 配置化。

### 批次 B：active-passive HA 最小闭环

1. 启动期 active lease / lock，防止两个 Core 同时 active。（已实现到工作区，待 CI）
2. 明确 journal/shadow/artifact 的共享或接管路径。（已补 runbook，待 CI）
3. 新 active 启动后先跑 `recover_pending_transactions`，再接受新 apply。（已实现到工作区，待 CI）
4. 文档化 `EndpointLockTable` 只保证进程内互斥。（已补 runbook，待 CI）
5. 若 adapter 跨主机，补 TLS/mTLS 或明确要求外部安全隧道。（未实现，当前建议 loopback/sidecar）

### 批次 C：运维可见性和测试可信度

1. pending transaction listing。
2. worker runtime 单 worker 失败隔离。
3. GC / drift auditor 单项失败不中止全局周期。
4. Mock backend 对齐真实 NETCONF 语义。
5. 顶层 `PartialSuccess` 或等价 API 说明。
## 2026-05-07 working-tree fix status

本节记录本轮已经落到工作区的修复状态。由于当前 Windows 环境缺少 `cargo` 和可用 Python 解释器，状态先标记为“已实现/待工具链验证”，不能视为已发布完成。

### 已实现，待工具链验证

- `detect_drift` 比较前归一化 expected/observed，覆盖接口别名、空字符串和 trunk VLAN 顺序/重复项。
- `rollback_after_endpoint_failure` 先 best-effort 调用 adapter rollback，再写 `RollingBack` journal；rollback 失败或异常状态会写 `InDoubt` 并向 apply 结果暴露。
- 新事务 gate 从仅 `InDoubt` 扩展为阻塞所有 `phase.requires_recovery()` 的同设备事务；`TX_REQUIRES_RECOVERY` 也聚合为 `InDoubt`。
- `InMemoryShadowStateStore::put` 改为 DashMap entry 内原子 revision RMW；`DeviceInventory.insert` 和 `AdapterClientPool.client` 也消除 get-then-insert 竞争窗口。
- NETCONF `recover` 保留原始 transaction strategy；`AdapterRecover + ConfirmedCommit` 先尝试 `final_confirm`，persist-id 已消费时视为 committed，其他错误才 fallback cancel-commit。
- NETCONF `prepare_candidate` 不再用 unlock 错误覆盖原始 edit/validate 错误；成功 edit+validate 后 unlock 失败时会 best-effort discard。
- `NetconfBackedDriver` 为非 `AdapterError` 异常增加统一 `ADAPTER_INTERNAL_ERROR` 结构化返回。
- Rust Core confirmed-commit timeout 增加 `with_confirmed_commit_timeout_secs` 配置入口，默认仍为 120 秒。
- `list_in_doubt_transactions` 现在列出所有 recoverable records，不再隐藏 `Prepared`/`Committing` 等 pending 事务。
- JsonFile journal 对非法 `tx_id` 直接 fail closed，不再 sanitize 到可能碰撞的文件名。

### 仍待后续批次

- active-passive HA 最小代码边界已加入工作区：`ActiveLeaseGuard` 使用共享 lease 文件和 heartbeat 防双 active，`AriaUnderlayService::activate_active_passive()` 会先拿 lease、再执行 `recover_pending_transactions`，之后才返回 active wrapper。该项待 GitHub Actions 验证。
- active-passive HA 部署手册已加入 `docs/runbooks/active-passive-ha.md`，覆盖共享 journal/shadow/artifact 路径、启动顺序、failover 顺序和 `EndpointLockTable` 进程内边界。该项待 GitHub Actions 验证。
- `EndpointLockTable` 仍只保证进程内互斥；跨节点互斥依赖 active-passive lease/fencing，不支持 active-active。
- Python adapter gRPC TLS/mTLS 尚未实现；跨主机部署仍需外部隧道/sidecar 或后续 TLS 配置。
- Worker runtime 单 worker 失败隔离、GC 单文件错误隔离、drift auditor 单设备错误隔离尚未实现。
- Product API 动作级 RBAC、顶层 `PartialSuccess` 语义、真实设备 parser/renderer 验证仍为后续项。
