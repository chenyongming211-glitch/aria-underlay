# TODOS

延期工作跟踪，用于后续 sprint。每个项目包含足够的上下文，便于数月后接手的人理解。

## P1: 当前进行中 — Phase 1 状态机重构

**前置条件**：当前 bug 清单已经更新到 `docs/bug-inventory-current-2026-05-30.md`，无已知非条件 open bug；Python Adapter gRPC TLS/mTLS 已修复。Rust 本地无 `cargo`，Rust 编译/测试必须通过 GitHub Actions 验证。

**做什么**：先只做显式状态机验证，不和 Product HTTP TLS 或 worker event bus 打包。详细方案见 `~/.gstack/projects/chenyongming211-glitch-aria-underlay/ceo-plans/structural-refactor-20260530.md`，但以本节修正后的边界为准。

- 新增/保留 `src/tx/phase_transition.rs` 的集中转换矩阵。
- 在 `TxJournalRecord` 上实现 `transition_phase(target)`；不要把状态更新方法放在 `TxPhase` 上，因为真正需要更新的是 journal record 的 `phase` 和 `updated_at_unix_secs`。
- 生产调用点从 `.with_phase(...)` 迁移到 `.transition_phase(...)`。
- `Committed -> InDoubt` 必须保留或通过流程重排消除。当前 apply 成功写入 terminal journal 后，如果 shadow 更新失败，会把 journal 拉回 `InDoubt`；状态机不能误杀这条恢复语义。
- 不要只把 `with_phase` 改成 `pub(crate)` 就认为完成强制边界：`TxJournalRecord.phase` 仍是 public 字段，外部仍可直接改 phase。当前阶段目标应先定义为“生产调用点集中化 + 回归测试”，真正的类型级封装需要单独迁移 public fields/serde/test fixture。
- integration tests 里大量使用 `.with_phase()` 构造 fixture。若收窄 `with_phase` 可见性，必须先提供明确的测试 fixture builder 或调整测试构造方式。

**为什么**：状态机验证散布在 apply/recovery/admin 路径里，没有单一位置验证合法转换。这个问题直接影响事务正确性，优先级高于性能优化和条件部署增强。

**外部视角修正**：
- 枚举计数：13 变体（9 非终态 + 4 终态），不是 14/13
- 调用点：20 个（含 admin_ops.rs:173），不是 19
- Rollback 路径特殊处理：先 adapter rollback 再 journal 写入
- ForceResolved 只允许从 InDoubt 转入
- `with_phase pub(crate)` 不是完整强制边界，除非同时收紧 `TxJournalRecord.phase` 字段可见性

**工作量**：M
**优先级**：P1（当前已开始，先完成这个单独切片）
**依赖**：当前未提交 Phase 1 WIP 需要继续补齐；Rust 以 GitHub Actions 为门禁

---

## P1: 没有真实交换机时的下一步 — Offline H3C Acceptance Runner

**做什么**：建立离线 H3C 验收闭环，把 fake/mock adapter、H3C renderer/parser、Product API/Core apply 流程串起来，输出机器可读 JSON 和人可读摘要。

**为什么**：当前没有真实交换机环境，继续扩厂商或做 HA/性能重构都不能直接提高可信度。离线验收可以在 CI 中证明 H3C VLAN、access/trunk、description、IPv4 ACL、ACL rule description、ACL bind/unbind、delete VLAN、delete ACL、unbind ACL 等能力没有退化。

**范围**：
- 不依赖真实交换机。
- 不扩 Huawei/Cisco/Ruijie。
- 不做 Product UI。
- 不替代真实设备验收；真实设备到位后复用同一报告格式。

**工作量**：M
**优先级**：P1（Phase 1 状态机重构完成并 CI 通过后）
**依赖**：当前 Phase 1 WIP 完成，避免验收 runner 建在仍在迁移的事务路径上

---

## P2/条件项: Product HTTP TLS/mTLS

**做什么**：仅当 Product API 需要绑定非 loopback 地址或跨主机访问时，为 `product_http_server.rs` 增加 server-side TLS/mTLS。

**为什么**：Python Adapter gRPC TLS/mTLS 已修复；Product API 当前配置仍强制 loopback 绑定。若仍是本机内部访问，Product HTTP TLS 不是当前最高价值项。

**实现修正**：
- 不迁移到 axum，除非端点数量或 HTTP 能力需求显著增长。
- 不复用 `AdapterClientPool::TlsConfig` 作为 server 配置；adapter 的 `TlsConfig` 是客户端证书配置，Product HTTP 需要独立 server cert/key/CA 配置。
- 如果允许非 loopback，必须同时更新配置校验、示例配置和 runbook，默认保持安全关闭。

**工作量**：M
**优先级**：P2/条件项
**依赖**：确认 Product API 部署边界从 loopback 改为跨主机或非受信网络

---

## P2/P3: Worker Event Bus 优化

**做什么**：在 worker runtime 中引入 `tokio::sync::broadcast` 作为加速提示，让 drift auditor/GC 可被 apply/transaction 事件提前唤醒，同时保留 interval timer 和 journal/store 扫描作为正确性兜底。

**为什么**：当前 6 个 worker 多数按 interval tick；event bus 可减少延迟和无意义扫描，但不是正确性前提。

**计划修正**：
- 不要写“早期事件丢失在架构上不可发生”。runtime reload、worker 未启动、receiver lagged、无订阅者时都可能丢事件。
- event bus 只能是 hint，不是 source of truth。journal/shadow/expected store 仍是恢复和审计的权威来源。
- `ConfirmedCommitTimeoutWatcher` 继续定时器驱动，超时是时间条件，不是离散事件。

**工作量**：M
**优先级**：P2/P3（离线验收闭环之后）
**依赖**：Phase 1 状态机重构完成；offline H3C acceptance runner 能覆盖 apply 后事件触发路径

---

## P3: Active-Passive HA Journal 复制

**做什么**：设计并实现 active/passive 节点之间的 journal 复制，使 HA 故障转移时保留进行中的事务。

**为什么**：当前 journal 是本地 JSON 文件。如果 active 节点在事务执行中崩溃，passive 节点没有 journal 状态。进行中的事务会丢失，需要人工恢复。这是设计文档中的 Open Question #6。

**优势**：
- HA 故障转移变为透明 — 进行中的事务无需人工干预
- Passive 节点可以在故障转移后立即恢复 recovery
- 符合 active-passive HA 可靠性的产品目标

**劣势**：
- 增加分布式状态同步复杂度
- 共享存储（NFS/iSCSI）vs 网络复制（rsync/NATS） — 两者都有运维成本
- 可能需要 journal 格式变更（当前 atomic rename 假设单写者）

**上下文**：从决定存储模型开始：共享文件系统（更简单，需要基础设施）vs journal 事件复制（更复杂，无需共享基础设施）。当前 `JsonFileTxJournalStore` 使用 atomic rename 保证单写者安全 — 任何复制方案必须保留这个不变量。Recovery coordinator（`src/api/recovery_coordinator.rs`）已经能处理从 journal 状态的 recovery — 只需要 journal 在 passive 节点上可用。

**工作量**：XL（人类团队 ~2 周）→ 含 CC+gstack: L
**优先级**：P3
**依赖**：HA 部署需求确认后；Phase 1 状态机重构完成

---

## P4/条件项: Journal Append-Only WAL 格式迁移

**做什么**：将 journal 从全量文件覆盖（每次状态转换重写整个 JSON）迁移到 append-only 预写日志。

**为什么**：全量文件覆盖是 O(n) 每次状态转换，其中 n = journal 记录大小。对于包含大量设备结果或错误历史的事务，这会成为写放大瓶颈。Append-only WAL 每次转换是 O(1)，并保留完整历史用于审计。

**优势**：
- 写入性能：append 是常数时间，与 journal 大小无关
- 完整审计轨迹：每次状态转换都保留，不仅仅是最新的
- 崩溃恢复：WAL 可以回放重建状态

**劣势**：
- 需要 compaction/GC 策略来限制文件大小
- 读取 journal 需要回放（或定期快照 + 回放）
- 迁移路径：现有 journal 在过渡期间需要可读

**上下文**：当前 `JsonFileTxJournalStore`（`src/tx/journal.rs:208`）使用 `write_atomic` 覆盖整个文件。迁移方案：新 WAL 格式带 header magic bytes 用于检测，读取路径在过渡期间处理两种格式，写入路径在迁移后始终使用 WAL。GC worker（`src/worker/gc.rs`）已经处理 journal 清理 — 扩展它以支持 WAL compaction。

**工作量**：L（人类团队 ~1 周）→ 含 CC+gstack: M
**优先级**：P4/条件项
**依赖**：Phase 1 状态机重构完成；有实际事务频率、journal 大小或审计查询压力证明写放大成为问题

---

## P4/条件项: 非 `:candidate` 设备原子性方案

**做什么**：为缺少 NETCONF `:candidate` 能力的设备设计原子性机制。

**为什么**：当前 ACID 事务依赖 NETCONF `:candidate`（锁定 candidate → prepare → commit）。部分白盒交换机和旧版 Ruijie 设备只支持直接编辑 running-config，没有 candidate 暂存区。这些设备无法使用当前的 lock-prepare-commit 协议。

**优势**：
- 扩展设备支持到非 `:candidate` 交换机
- 保持原子性保证（全有或全无的配置下发）
- 启用 Cisco IOS 和旧版 Ruijie 支持路径

**劣势**：
- Running-config 快照 + 回滚脚本本质上不如 `:candidate` 安全
- 无 NETCONF 标准机制 — 必须是厂商特定的
- Running-config 部分失败的回滚是尽力而为，不保证

**上下文**：从采集非 `:candidate` 设备样本的 running-config XML 开始。可能的方案：(1) 在下发前快照 running-config，(2) 将配置变更推送到 running，(3) 失败时，diff 快照与当前状态并生成回滚命令。这是通过 SSH CLI 的 running-config 快照 + 回滚脚本模式，不是 NETCONF。设计文档 Open Question #5。

**工作量**：L（人类团队 ~1 周）→ 含 CC+gstack: M
**优先级**：P4/条件项
**依赖**：特定非 `:candidate` 设备进入部署范围；SSH CLI 后端已实现
