# 当前缺陷 / 技术债清单 — 2026-05-05

> 2026-05-07 已按“单 active Core + 可选 active-passive HA + NETCONF 事务下发”目标补充复核清单：见 `docs/bug-inventory-current-2026-05-07.md`。本文保留完整源码审查结果和历史证据。

## 当前基线

最新有效基线：`main` / `e87fe41 test: harden vendor fixture boundaries`。

本次审查：4 个并行 agent 对全部 Rust (~90 文件) + Python (~40 文件) 源码进行事务性专项审查，结合人工逐文件分析。共发现 **31 个问题**，其中 7 高、12 中、12 低。

## 高严重度 (7)

### 1. ConfirmedCommit 恢复盲点

**Files:** `src/api/recovery_coordinator.rs:146-149`, `adapter-python/aria_underlay_adapter/drivers/netconf_backed.py:251-307`, `adapter-python/aria_underlay_adapter/backends/netconf.py:435-457`

**Root cause:** 当 `final_confirm` 在适配器上成功（NETCONF persist-id 已消费，commit 已确认）但 journal `Committed` 写入之前 Rust 进程崩溃，journal 停留在 `FinalConfirming`。恢复将 `FinalConfirming` 归类为 `AdapterRecover`，调用 `NetconfBackedDriver.recover()`。该方法对 `AdapterRecover + ConfirmedCommit` 调用 `backend.rollback_candidate(strategy=ConfirmedCommit, tx_id=tx_id)`，执行 `session.cancel_commit(persist_id=tx_id)`。由于 persist-id 已被成功的 `final_confirm` 消费，cancel 必然失败。事务永久卡在 `InDoubt`，每次恢复尝试都撞同一堵墙。只能通过 `force_resolve_transaction` (break-glass) 清除。

**Crash window:** `final_confirm_with_context` 返回 `Ok` 之后，`self.journal.put(Committed)` 之前。

**Fix direction:** `NetconfBackedDriver.recover()` 对 `AdapterRecover + ConfirmedCommit` 应先尝试 `final_confirm(tx_id)`：若成功 → Committed（commit pending，现已确认）；若失败且错误为 "unknown persist-id" → Committed（commit 已确认，persist-id 已消费）；只有其他错误才 fallback 到 `cancel_commit`。

### 2. 漂移检测未归一化比较

**Files:** `src/state/drift.rs:75` (`detect_drift`), `src/engine/normalize.rs:70` (`normalize_shadow_state`)

**Root cause:** `detect_drift` 直接对原始 `DeviceShadowState` 做字段级 `==` 比较，未调用 `normalize_shadow_state`。而 `compute_diff` (`diff.rs:34-35`) 总是对两侧归一化。normalize 处理：空字符串→None、接口名称规范化（如 `GigabitEthernet0/1`→`GE0/1`）、`allowed_vlans` 排序去重。未归一化导致每次漂移审计对所有设备产生误报。

**Impact:** `ReportOnly` → 误报警淹没监控；`BlockNewTransaction` → 所有设备永久阻塞新事务；`AutoReconcile` → 系统不断"修复"不存在的漂移。

**Fix direction:** 在 `detect_drift` 开头对 `expected` 和 `observed` 均调用 `normalize_shadow_state`。

### 3. InMemoryShadowStateStore put 竞争导致静默数据丢失

**Files:** `src/state/shadow.rs:57-65`

**Root cause:** `InMemoryShadowStateStore::put` 执行非原子的 read-modify-write：先 `get` 读取 revision，计算 n+1，再 `insert`。两个并发线程可读取相同 revision，各自计算 n+1，后写者静默覆盖先写者。`JsonFileShadowStateStore` 通过每设备 `Mutex` 避免了此问题，但 `InMemory` 变体无等效保护。

**Impact:** 所有使用默认构造函数（`AriaUnderlayService::new()` 等）的部署在并发下会丢失影子状态更新。`AriaUnderlayService` 的四个构造函数都默认使用 `InMemoryShadowStateStore`。

**Fix direction:** 添加每设备 `Mutex`（与 JsonFile 版本一致），或使用 DashMap 的原子 RMW API。

### 4. recover DISCARD_PREPARED_CHANGES 对 ConfirmedCommit 使用错误回滚策略

**Files:** `adapter-python/aria_underlay_adapter/drivers/netconf_backed.py:251-254`

**Root cause:** `NetconfBackedDriver.recover()` 对 `DISCARD_PREPARED_CHANGES` 无条件强制 `rollback_strategy = CANDIDATE_COMMIT`，忽略原始事务策略。对于 ConfirmedCommit 事务，"prepared changes" 已在 running 中（confirmed-commit 将其移入 running 并启动 timer）。`discard_changes()` 只清空 candidate buffer，**不取消 pending confirmed-commit**。

**Impact:** running config 保持修改状态 + device timer 仍在滴答。适配器返回 `ROLLED_BACK` 但设备实际未回滚。timer 到期后设备自动回滚，但期间 Rust core 认为设备干净而实际不干净——状态不一致。

**Fix direction:** 保留原始 strategy 作为 rollback_strategy。对 ConfirmedCommit 应调用 `cancel_commit(persist_id=tx_id)` 而非 `discard_changes()`。

### 5. DeviceInventory.insert TOCTOU 竞争

**Files:** `src/device/inventory.rs:22-26`

**Root cause:** `insert` 先 `contains_key` 检查再 `insert`，非原子操作。两个并发线程可同时通过检查，第二个 `insert` 静默覆盖第一个。两个调用者都得到 `Ok(())`，第一个 `ManagedDevice` 数据丢失。

**Fix direction:** 使用 DashMap 的 `entry` API 实现原子 check-and-insert。

### 6. Candidate datastore TOCTOU：prepare 与 commit 之间的竞争窗口

**Files:** `adapter-python/aria_underlay_adapter/backends/netconf.py:207-235` (prepare_candidate), `netconf.py:357-410` (commit_candidate)

**Root cause:** `prepare_candidate()` 在 finally 块中 unlock candidate（line 231），然后关闭 session。`commit_candidate()` 打开**新** session 执行 commit。在 unlock 和 commit 之间，外部 NETCONF 客户端可修改 candidate datastore。Rust 侧 `EndpointLockTable` 阻止同服务并发，但不能阻止其他管理系统直接操作设备。

**Fix direction:** commit 前重新 lock + validate candidate，或 prepare 返回 candidate 内容 checksum 供 commit 时校验。

### 7. Journal 写在 rollback RPC 之前

**Files:** `src/api/apply_coordinator.rs:623-624`

**Root cause:** `rollback_after_endpoint_failure` 先写 `RollingBack` phase 到 journal（line 624），再调用 adapter rollback RPC（line 626）。若 journal write 失败，`?` 立即传播错误，adapter rollback **从未被调用**。设备上残留 prepared-but-uncommitted 状态。随后 `finish_failed_apply` 将 journal 写为 `Failed`（终态，`requires_recovery() == false`）。恢复永远不会清理孤立的 adapter 状态。

**Fix direction:** 先调用 adapter rollback RPC，再写 journal。若 journal write 失败，rollback 已发生，状态不孤立。

## 中严重度 (12)

### 8. 工作线程单点失败终止全部工作线程

**Files:** `src/worker/runtime.rs:184-206`

**Root cause:** `run_until_shutdown` 在 `record_worker_outcome` 上对 `Err` 传播 `?`，触发 `shutdown_tx.send(true)` 并清空所有 worker。任一 worker（GC、漂移审计、告警压缩等）的单个错误会停止所有后台维护。

**Fix direction:** 捕获错误并记录日志，返回零报告让其他 worker 继续。或为每个 worker 实现独立错误域。

### 9. GC 单文件错误致命传播

**Files:** `src/worker/gc.rs:207-245`

**Root cause:** GC 的 `for` 循环对单个文件操作使用 `?`（`fs::remove_file`、`delete_artifacts_for_tx`）。一个损坏的 journal 文件或权限错误导致整个 GC 周期失败 → 错误传播到 runtime → 关闭所有 worker（Bug #8）。

**Fix direction:** 在 for 循环内吞下单个文件错误，记录日志并继续。在 `JournalGcReport` 中加入成功/失败计数。

### 10. 漂移审计单设备错误中止全部审计

**Files:** `src/worker/drift_auditor.rs:111-118`

**Root cause:** `observed_source.get_observed_state(&expected.device_id).await?` 中任一设备网络超时或适配器错误通过 `?` 中止整个审计运行。之前已成功处理的设备结果全部丢失。在 100 台设备中第 99 台不可达 → 0 台获得漂移报告。

**Fix direction:** 改为 match 收集结果，对 `Err` 跳过并计入警告计数，继续处理剩余设备。

### 11. ensure_no_in_doubt_for_devices 只过滤 InDoubt 阶段

**Files:** `src/tx/recovery.rs:45`, `src/api/apply_coordinator.rs:662-680`

**Root cause:** `in_doubt_records_for_devices` 只返回 `TxPhase::InDoubt` 记录。但 `list_recoverable()` 返回所有非终态记录（Started, Preparing, Prepared, Committing, Verifying, FinalConfirming, RollingBack, Recovering, InDoubt）。进程崩溃后 journal 的 Committing 等阶段记录不阻塞新事务，新事务可能在旧 confirmed-commit timer 仍在滴答时操作同一设备。

**Fix direction:** 将过滤器扩展为所有 `requires_recovery() == true` 的记录，或至少加入 Prepared, Committing, Verifying, FinalConfirming, RollingBack, Recovering。

### 12. 跨设备 apply 无原子性

**Files:** `src/api/apply_coordinator.rs:69-91`

**Root cause:** 多设备 intent（如 switch-pair）中 `apply_desired_states` 对每个 device 独立创建事务（独立 UUID tx_id）。第一个设备 commit 后第二个失败时，无补偿回滚。API 名称 `apply_intent` 暗示原子性，实际不提供。

**Fix direction:** 实现真多设备分布式事务（prepare all → commit all，失败时补偿回滚已提交设备），或将此限制明确文档化。

### 13. prepare_candidate finally 块错误掩码

**Files:** `adapter-python/aria_underlay_adapter/backends/netconf.py:217-235`

**Root cause:** `finally` 块中 `_unlock_candidate` 若抛异常，Python 语义下 unlock 异常**替代**原始 edit/validate 异常。调用者看到 "NETCONF unlock failed"，真正原因（validate failed 或 edit-config failed）丢失。unlock 失败标记 `retryable=True`，调用者可能重试，但原始错误（如 invalid config）不可重试。

**Fix direction:** 类似 `_discard_candidate_preserving_error` 的模式，捕获 unlock 错误并附加到原始错误而非替代。

### 14. prepare_candidate unlock 失败后 candidate 脏残留

**Files:** `adapter-python/aria_underlay_adapter/backends/netconf.py:217-235`

**Root cause:** 成功 edit+validate 后若 finally 中 unlock 抛异常，candidate 变更留在设备上且保持 locked。无 discard 被调用。调用者看到 unlock 失败并可能重试 prepare，但因 candidate locked 而失败，或放弃并留下 dirty candidate。

**Fix direction:** finally 块中若 unlock 失败，尝试丢弃变更（新 session 上 discard_changes）以清理。

### 15. AdapterOperation.errors 诊断数据被丢弃

**Files:** `src/error.rs:19-25`, `src/api/apply.rs:129-161`

**Root cause:** `UnderlayError::AdapterOperation.errors` 在 `extract_adapter_errors()` 中被正确填充，但 `journal_error_fields()` 使用 `..` 丢弃该字段。`#[allow(dead_code)]` 注释确认已知技术债务。

**Fix direction:** 在 `journal_error_fields` 输出中包含 errors，或至少记录日志。

### 16. rollback_after_endpoint_failure 总是返回 Ok

**Files:** `src/api/apply_coordinator.rs:616-660`

**Root cause:** `rollback_after_endpoint_failure` 无论 adapter 级别回滚结果如何都返回 `Ok(())`。rollback RPC 失败或返回意外状态时，函数在 journal 中记录 `InDoubt` 但返回成功。调用者返回原始操作错误，完全掩码 rollback 失败。API 响应不体现事务已变 InDoubt。

**Fix direction:** 从 `rollback_after_endpoint_failure` 返回回滚结果或组合错误，传播到 `finish_failed_apply` 在 `DeviceApplyResult` 中包含回滚失败信息。

### 17. 影子状态写在 journal Committed 之前

**Files:** `src/api/apply_coordinator.rs:283, 317-320`

**Root cause:** `finish_successful_apply` 先写 shadow store（line 283），再将 journal 标为 `Committed`（line 317-320）。若在两者之间崩溃：shadow 已更新但 journal 未 committed。恢复时若 adapter 报告 `RolledBack`，journal 写 `RolledBack`，但**不修复 shadow store**。shadow 永久与实际状态背离。

**Fix direction:** 先写 journal `Committed`，再写 shadow store。若 shadow write 失败，将记录标为 `InDoubt` 以便恢复在下一轮修复 shadow。

### 18. AdapterClientPool.client() TOCTOU

**Files:** `src/adapter_client/client.rs:415-428`

**Root cause:** `client()` 先 `self.channels.get(endpoint)` 再 `or_insert`，非原子操作。并发冷启动时 N 个线程都看到 `None`，N 个都创建 Channel，N-1 个被丢弃。虽不导致数据损坏，但浪费资源且若 `connect_lazy` 有副作用可能产生瞬态失败。

**Fix direction:** 使用 DashMap `entry` API 原子化 check-and-insert。

### 19. Confirmed commit 无后台超时监控

**Files:** `src/adapter_client/client.rs:189`

**Root cause:** `confirm_timeout_secs=120` 硬编码，完全依赖设备侧超时。Rust 侧无后台任务监控即将超时的 confirmed commit、主动 cancel 或超时后及时更新 journal。若 recovery 因故未运行，journal 停留在非终态，设备回滚后 journal 与实际状态不一致。

**Fix direction:** 添加 confirmed commit timeout watcher 后台任务。

## 低严重度 (12)

### 20–24. Mock 后端与真实后端行为偏离（5 项）

**Files:** `adapter-python/aria_underlay_adapter/backends/mock_netconf.py`

| # | 问题 | 行号 |
|---|------|------|
| 20 | `final_confirm` 对空/缺失 tx_id 静默成功（真实后端会报 MISSING_TX_ID） | 219-232 |
| 21 | `rollback_candidate` ConfirmedCommit 不要求 tx_id | 234-250 |
| 22 | `commit` 在 `_candidate is None`（无 prepare）时静默成功 | 212-217 |
| 23 | `commit` 第二次调用静默成功（真实设备 persist-id 冲突或 confirmed commit 已过期） | 212-217 |
| 24 | `_is_confirmed_commit_strategy` 接受字符串值，真实后端只认整数常量 | 370-373 |

**Impact:** 降低测试覆盖率，CI 通过的测试在真实设备上可能失败。

**Fix direction:** 对齐 mock 与真实后端的校验逻辑。

### 25. `list_in_doubt_transactions` 只显示 InDoubt，隐藏其他卡住的记录

**Files:** `src/api/admin_ops.rs:60`

**Root cause:** 先 `list_recoverable()`（返回所有非终态）再 filter 只保留 `InDoubt`。Started/Preparing/Prepared/Committing 等卡住的记录不显示，运维人员误判系统健康。

**Fix direction:** 显示所有非终态记录或新增 `list_pending_transactions` API。

### 26. `force_resolve_transaction` 冗余双重 journal fetch

**Files:** `src/api/admin_ops.rs:118, 128`

**Root cause:** 先 fetch 获取 device list（加锁用），加锁后再 fetch 同一个 tx_id。第二次 fetch 返回 `None` 几乎不可能（journal 无 delete 操作），但错误消息误导为 `TX_NOT_FOUND`。

**Fix direction:** 复用第一次 fetch 结果，移除第二次 fetch。

### 27. `now_unix_secs()` 秒级精度

**Files:** `src/utils/time.rs:5`

**Root cause:** 所有 journal 时间戳使用秒级精度。同一秒内多次 phase 转换的 `updated_at_unix_secs` 相同，无法区分时间顺序。

**Fix direction:** 改为毫秒或纳秒精度。

### 28. Journal 文件名消毒可能碰撞

**Files:** `src/tx/journal.rs:290-301`

**Root cause:** `journal_file_stem` 将多个不同非字母数字字符映射为同一 `_`。如 `tx-a/b` 和 `tx-a_b` 都映射为 `tx-a_b.json`，后者静默覆盖前者。实际 tx_id 是 UUID（仅字母数字+连字符），当前无触发条件但缺少防御。

**Fix direction:** 在 tx_id 创建时校验字符集，或使用 base64url 编码文件名。

### 29. DryRun RPC proto 定义但 AdapterClient 未包装

**Files:** `proto/aria_underlay_adapter.proto:9`, `src/adapter_client/client.rs`

**Root cause:** proto 定义了 `DryRun` RPC 但 Rust `AdapterClient` 无对应方法。Python adapter 实现了该 RPC 但 Rust 侧无法调用。

**Fix direction:** 添加 `dry_run` 方法到 `AdapterClient`，或从 proto 中移除（若有意不使用）。

### 30. `shadow_state_from_proto` 硬编码 revision: 0

**Files:** `src/adapter_client/mapper.rs:218`

**Root cause:** 适配器返回的 observed state 总是 `revision: 0`。若未来适配器提供 revision（乐观并发控制），此代码静默丢弃。

**Fix direction:** 若适配器永不需要 revision 则文档化；否则扩展 protobuf `ObservedDeviceState` 加 `optional uint64 revision`。

### 31. GC journal_root 与 TxJournalStore root 无关联校验

**Files:** `src/worker/gc.rs`, `src/tx/journal.rs`

**Root cause:** `JournalGc` 和 `JsonFileTxJournalStore` 各自接受独立路径配置，无编译时或运行时校验两者指向同一目录。配置错误时 GC 可能清理错误目录或漏清理。

**Fix direction:** 添加运行时校验或从 `JsonFileTxJournalStore` 复用 root 配置。

## 已确认修复/已排除的已知问题

以下问题在本次审查中确认已修复或已排除：

- **HostKeyPolicy 传递:** `device_ref_from_info()` 已包含 `host_key_policy`、`known_hosts_path`、`pinned_host_key_fingerprint`（`mapper.rs:34-35, 44-46`）。之前 bug 报告中此问题已修复。
- **Jitter 随机源:** `endpoint_lock.rs:68-75` 已改为 `rand::thread_rng()`，修复了原来 `SystemTime::subsec_nanos()` 的相关性问题。

## 汇总

| 严重度 | 数量 | 关键主题 |
|--------|------|----------|
| 高 | 7 | 恢复盲点、漂移归一化缺失、影子状态竞争、回滚策略错误、TOCTOU×2、journal/rollback 顺序 |
| 中 | 12 | Worker 单点失败放大、跨设备原子性缺失、prepare 错误处理、AdapterOperation.errors 丢弃、shadow 写入顺序、AdapterClientPool 竞争 |
| 低 | 12 | Mock 行为偏离×5、API 可见性、精度问题、文件名碰撞、proto 未实现 |

修复优先级：Bug #2 (漂移归一化) → Bug #1 (ConfirmedCommit 恢复) → Bug #4 (回滚策略) → Bug #3 (影子状态竞争) → Bug #5 (DeviceInventory TOCTOU) → Bug #7 (journal/rollback 顺序)
