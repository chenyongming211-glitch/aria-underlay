# TODOS

延期工作跟踪，用于后续 sprint。每个项目包含足够的上下文，便于数月后接手的人理解。

## P1: 已完成 — Phase 1 状态机重构

**状态**：已完成并通过 GitHub Actions（run `26683402767`）。生产事务路径通过 `TxJournalRecord::transition_phase()` 进行 phase 变更；`with_phase()` 仍保留给测试 fixture 和后续 public-field 封装迁移。

**前置条件**：当前 bug 清单已经更新到 `docs/bug-inventory-current-2026-05-30.md`，无已知非条件 open bug；Python Adapter gRPC TLS/mTLS 已修复。Rust 本地无 `cargo`，Rust 编译/测试必须通过 GitHub Actions 验证。

**已完成内容**：本阶段只做显式状态机验证，没有和 Product HTTP TLS 或 worker event bus 打包。详细方案见 `~/.gstack/projects/chenyongming211-glitch-aria-underlay/ceo-plans/structural-refactor-20260530.md`，但以本节修正后的边界为准。

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
**优先级**：P1（已完成）
**依赖**：Rust 以 GitHub Actions 为门禁；本阶段已在 run `26683402767` 通过

---

## P1: 已完成 — Offline H3C Acceptance Runner

**状态**：已完成并合入 `main`。基础 runner 通过 GitHub Actions run `26684186596`；parser-in-the-loop 升级通过分支 run `26684551948`，并在 `main` run `26684655861` 通过完整 CI。当前 `main` 对应 commit `2cc59f1`。

**已完成内容**：建立离线 H3C 验收闭环，输出机器可读 JSON 和人可读摘要。当前 runner 覆盖 Python adapter 侧 fake/mock backend、H3C renderer、mock NETCONF dry-run/prepare/commit/final-confirm、H3C readback XML 生成、`H3cStateParser` 解析和 parsed-vs-observed verify。Rust Core / Product API apply 流程继续由现有 fake-adapter integration matrix 覆盖，不在该 runner 内重复编排。

**为什么**：当前没有真实交换机环境，继续扩厂商或做 HA/性能重构都不能直接提高可信度。离线验收可以在 CI 中证明 H3C VLAN、access/trunk、description、IPv4 ACL、ACL rule description、ACL bind/unbind、delete VLAN、delete ACL、unbind ACL 等能力没有退化。

**范围**：
- 不依赖真实交换机。
- 不扩 Huawei/Cisco/Ruijie。
- 不做 Product UI。
- 不替代真实设备验收；真实设备到位后复用同一报告格式。
- 不声称真实交换机通过；真实设备到位后仍必须按 runbook 做 running XML 采集、parser 验证和 renderer 下发验收。

**工作量**：M
**优先级**：P1（已完成）
**依赖**：Phase 1 状态机重构已完成并通过 CI，验收 runner 已建在稳定事务路径之后

---

## P1: 已完成 — 标准模型 / SoT / ChangePlan 基础

**状态**：已完成并合入 `main`。`codex/device-model-profile-contract` 经 CI 通过后由 commit `cf7d0f5` 合入；后续 commit `7d9a61d` 继续补齐 YANG schema 采集能力。已落地：`DeviceModelProfile`（含 `WriteReadiness`）、`SotSnapshot` 输入边界、`ChangePlan`（含 stage 顺序/dependency_edges/rollback_order/blast_radius/unsupported_paths/`DryRunWriteDecision`）、NETCONF YANG module evidence、可选 gNMI capability 探测、YANG schema collection。

**做什么**：把 OpenConfig/gNMI 评估、Source of Truth 输入边界和 ChangePlan dry-run 落地到核心开发计划和代码骨架中。详细执行计划见 `docs/superpowers/plans/2026-05-30-standard-model-sot-changeplan.md`。

**为什么**：PBR、BGP、QoS、NQA 这类功能不是简单的“多渲染几条 H3C 命令”。它们有跨对象依赖、引用顺序、删除顺序和业务 blast radius。如果继续按“厂商命令 renderer 先行”的方式扩展，会把型号/固件差异、命令依赖和回滚风险都压到 adapter 里，后续很难稳定。当前没有真实交换机环境，最应该先补的是可在 CI 和离线报告中验证的变更前决策层。

**范围**：
- 新增 `DeviceModelProfile` 合约，记录 vendor、model、os_version、YANG modules/revisions、OpenConfig/gNMI 支持、厂商 native YANG 支持、path 级 read/write 验证结果和最终 write readiness。
- Python Adapter 增加 NETCONF YANG Library / capability 探测入口；如果后续接 gNMI，则通过同一 profile 输出，不让 Rust Core 直接依赖具体探测实现。
- 定义 `SotSnapshot` 输入边界，让 NetBox、Nautobot、文件或外部 API 都先转换成项目内部稳定结构；Core 不直接绑定外部 SoT 的 SDK、分页或 schema。
- 在 Rust Core 的 diff 和 renderer 之间加入 `ChangePlan`：包含 stage 顺序、dependency_edges、rollback_order、touched_scope、blast_radius、unsupported_paths 和 `DryRunWriteDecision`。
- Dry-run 和 offline H3C acceptance report 输出 ChangePlan 摘要；没有真实设备时先证明 renderer/parser 没退化，同时证明复杂变更会给出顺序、依赖和拒绝原因。
- PBR/BGP 写入必须先通过 profile 和 ChangePlan 门禁；没有 path 级读写证据时只能做 read-only parser/audit 或结构化拒绝。

**验收标准**：
- `DeviceModelProfile` 在 proto/Rust/Python 之间可序列化并进入 capability report。
- OpenConfig/gNMI 或 native YANG 只在 path 级 read/write 验证通过后才允许标记为 writable；模块存在不等于可写。
- `SotSnapshot` 能表达 device/interface/VLAN/ACL/policy/BGP neighbor 的归属和来源，并能被后续 planner 消费。
- `ChangePlan` 对 create/update/delete 都能输出稳定顺序；删除引用类对象时必须先解绑再删除。
- Dry-run JSON 中能看到 dependency graph、blast radius、rollback order、unsupported paths 和最终 write decision。
- GitHub Actions 通过 Rust `cargo check` / `cargo test` 和 Python pytest；本地无 `cargo` 时以 CI 为准。

**不做**：
- 不直接实现 PBR/BGP 写入。
- 不把 Ansible/Nornir/NAPALM 塞进核心事务路径。
- 不要求马上接入 NetBox 或 Nautobot。
- 不声称 OpenConfig 一定可用；本阶段目标是探测、分类和门禁。

**工作量**：L
**优先级**：P1（复杂网络功能写入前的基础层）
**依赖**：已由 GitHub Actions 验证并合入 `main`；后续 PBR/BGP 写入仍必须以 path 级 profile evidence 和 ChangePlan 门禁为前置

---

## P1: 进行中 — PBR/BGP read-only parser/audit

**状态**：初版已落地在 Python adapter：`H3cStateParser` 会从 running XML 中识别 PBR/BGP 高风险配置，输出 `high_risk_audit` 和结构化 `touched_scope`；offline H3C acceptance report 新增 `read_only_audits`，明确 `write_decision=read_only`、`blast_radius=routing_control_plane`、unsupported paths、affected VRFs、BGP neighbors、route-policy refs、PBR policy refs、ACL refs 和 interfaces。PBR/BGP real-sample calibration harness 已接入：可从脱敏 H3C running XML 样本目录生成 `real_sample_audits`；目录缺失或当前无样本时 CI 不失败。当前仍不生成 PBR/BGP renderer，不进入写配置路径。

**做什么**：先把 PBR/BGP 作为只读审计对象接入 parser 和离线报告，让系统能发现现网中是否已经存在 PBR/BGP、涉及哪些 VRF/neighbor/policy/ACL/interface，以及为什么当前必须拒绝自动写入。

**范围**：
- H3C running XML parser 输出 PBR/BGP high-risk audit，不写入 `ObservedDeviceState` proto 主模型，避免误导为已支持配置面。
- Offline H3C acceptance runner 输出 `read_only_audits`，覆盖 parser-only audit、read-only decision、blast radius、unsupported paths、`touched_scope` 和 warnings。
- Offline H3C acceptance runner 可通过 `--pbr-bgp-sample-dir` 加载脱敏真实样本，输出每个样本的 `real_sample_audits`；样本缺失时保持非失败。
- PBR 默认 blast radius 为 `policy_reference`；BGP 默认 blast radius 为 `routing_control_plane`。
- 未满足 path-level read/write evidence 时，PBR/BGP 写入保持 read-only/rejected。

**不做**：
- 不实现 PBR/BGP intent。
- 不实现 PBR/BGP renderer。
- 不把识别到的 PBR/BGP 节点写入 shadow/expected store。
- 不声明真实 H3C 设备 PBR/BGP XML 结构已经完整覆盖；真实设备样本到位后继续校准 parser。

**下一步**：
- 收集脱敏真实 H3C running XML 样本，放入 `adapter-python/tests/fixtures/state_parsers/real_samples/h3c/comware7/` 或通过 `--pbr-bgp-sample-dir` 指定目录跑 calibration harness。
- 再决定是否推进 H3C Basic IPv4 ACL 或继续做 PBR/BGP path-level profile 验证。

**工作量**：M
**优先级**：P1（高风险功能写入前的只读证据层）
**依赖**：DeviceModelProfile / ChangePlan / Offline H3C Acceptance Runner 已落地

---

## P1/P2: 已完成离线初版 — H3C Batch 2 Basic IPv4 ACL

**已完成内容**：Basic IPv4 ACL 已作为独立 ACL family 接入 domain/proto、Rust intent validation/planner/adapter mapper、Python H3C renderer/parser、mock NETCONF backend 和 offline H3C acceptance runner。系统现在显式区分 `advanced_ipv4` 与 `basic_ipv4`，Basic ACL 使用 `2000..2999`，Advanced ACL 保持 `3000..3999`；旧 JSON/shadow payload 未携带 kind 时默认按 Advanced IPv4 兼容。

**为什么**：Basic IPv4 ACL 是 H3C Batch 2 中风险最低、复用现有 ACL renderer/parser/verify/offline acceptance 基础最多的一步。它不是 PBR/BGP 方案的替代品，但能先补齐后续 PBR/QoS/BGP 可能引用的 ACL family 表达和 readback 解析基础。

**已完成范围**：
- 明确 Basic IPv4 ACL 的 domain/proto 表达方式，避免和已有 numeric IPv4 advanced ACL 混淆。
- 增加 H3C renderer tests、parser tests、Rust intent validation / mapper tests。
- 扩展 offline H3C acceptance runner，把 Basic IPv4 ACL 纳入 parser-in-the-loop 验收。
- Basic ACL 只允许 `ip` 协议、source 匹配，不允许 destination 或端口匹配；kind 与 ACL ID 段不一致时 fail closed。
- 更新 real-device acceptance checklist，明确没有真实交换机前只记录待验收，不标记真机通过。

**仍不做**：
- 不做 named ACL、IPv6 ACL、ACL 引用到 QoS/PBR/NQA/BGP。

**工作量**：M/L
**优先级**：P1/P2（离线初版完成；真机验收待设备环境）
**依赖**：Offline H3C Acceptance Runner 已完成；ChangePlan 基础层至少完成 dry-run/report 形态

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

## P2: LLM 辅助适配 MVP

**做什么**：用 LLM 加速新厂商 renderer/parser 的初始代码生成。以现有 H3C renderer（1193 行）和 state parser（1188 行）作为 golden reference，结合新厂商 running-config 样本和 YANG modules，由 LLM 生成 renderer/parser 骨架、fixtures 和 acceptance scenario。人工只修 offline acceptance 中失败的 case。

**为什么**：当前每接入一个新厂商需要手写 ~1700 行代码（renderer ~400 + parser ~400 + fixtures ~600 + acceptance ~100 + tests ~200）。不同厂商的 renderer 结构高度相似，只是 namespace 和 element name 不同，LLM 擅长这种 pattern matching + 转换。

**工作流**：
1. **Phase 1（只读采集）**：采集新设备的 running-config 样本、NETCONF capabilities、YANG schema library。不写设备。
2. **Phase 2（LLM 生成）**：用 H3C reference + 新厂商样本作为 prompt，生成 renderer/parser/fixtures/acceptance。最多 3 轮迭代（失败 → 喂回 LLM → 重新生成）。
3. **Phase 3（自动验证）**：跑 offline acceptance runner。这是现有基础设施的天然复用。
4. **Phase 4（人工修复）**：只修失败的 3-5 个 case，不需要从零写 1000 行。
5. **Phase 5（真机验收）**：和现有 `real-device-acceptance.md` runbook 完全一致，不降低标准。

**代码结构**：
```text
renderers/
  ├── generated/          # LLM 生成且通过 acceptance
  ├── handwritten/        # 现有手写（h3c.py, huawei.py 迁入）
  └── overrides/          # 对生成代码的人工 override（最高优先级）
```

**安全约束**：
- 生成的代码不过 acceptance 就不进生产
- 生成代码头部标注 `AUTO-GENERATED` + `UNVERIFIED` / `VERIFIED` 状态
- Renderer 来源进入 journal 和审计日志
- 不让 LLM 生成事务/恢复/审计相关代码
- 不让 LLM 决定写路径准入（仍由 `DeviceModelProfile` + `DryRunWriteDecision` 决定）

**ROI**：新厂商接入时间从 ~4 周降至 ~1.5 周（~60% 节省）。后续每新增一个厂商边际成本更低。

**详细方案**：`docs/adapter-acceleration-strategy.md` §3

**工作量**：M（1-2 周）
**优先级**：P2（标准模型计划和 YANG schema collection 已合入 main 之后）
**依赖**：至少一个新厂商设备的 running-config 样本、NETCONF capabilities 和 YANG schema library 样本

---

## P2/P3: 已完成 — YANG Schema 采集

**状态**：已完成并合入 `main`（commit `7d9a61d`，GitHub Actions run `26704193631`）。已落地：`backends/yang_schema.py`（collect/save/load）、`YangModuleSummary` proto 消息、`DeviceCapability.yang_modules` 字段、`DeviceModelProfile.yang_module_count`、`ARIA_UNDERLAY_YANG_SCHEMA_COLLECTION_ENABLED` 配置开关、`ARIA_UNDERLAY_YANG_LIBRARY_DIR` 归档路径覆盖、save/load 单元测试和 NETCONF backend 测试。

**做什么**：把所有接入设备的 YANG modules 通过 NETCONF `get-schema`（RFC 6022）采集并归档，建立 YANG library。结果存入 `data/yang-library/{vendor}/{model}/{os_version}/` 并通过 `DeviceCapability.yang_modules` 返回，`DeviceModelProfile.yang_module_count` 记录总数。

**为什么**：YANG library 是三个适配加速方案的共同数据基础。即使后续不走 YANG 驱动方向，采集数据也有独立价值（设备能力归档、firmware 变更追踪）。

**范围**：
- 只读操作：从 NETCONF server capabilities 提取 module hints，并通过 `get-schema` 下载 schema 文本
- 默认关闭，通过 `ARIA_UNDERLAY_YANG_SCHEMA_COLLECTION_ENABLED=1` 启用；可用 `ARIA_UNDERLAY_YANG_LIBRARY_DIR` 覆盖归档根目录
- 归档到 `data/yang-library/{vendor}/{model}/{os_version}/`，写入 `yang-modules.json` 和 `{name}@{revision}.yang`
- Proto 增加 `YangModuleSummary`、`DeviceCapability.yang_modules`，`DeviceModelProfile.yang_module_count` 记录数量
- 采集失败不影响 capability probe；失败 module 以 skipped/error summary 记录

**不做**：
- 不做 YANG schema diff（需要真实设备 candidate 探测，是独立工作项）
- 不做 renderer/parser 自动生成
- 不把 schema 下载成功视为 path-level writable 证据

**详细方案**：`docs/adapter-acceleration-strategy.md` §2.3 阶段 A

**工作量**：S（2-3 天）
**优先级**：P2/P3（已完成，作为 LLM 辅助适配和 YANG conformance 的输入）
**依赖**：后续真实设备验收需要采集并归档至少一组真实设备 YANG library 样本

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

## P3: Runtime YANG Validator

**做什么**：在 renderer 输出发送到设备之前，用 YANG schema 验证 XML 结构是否符合 schema。验证失败 fail-closed，不发送 edit-config。

**为什么**：作为 renderer 的运行时安全网，在 edit-config 发送到设备之前捕获 XML 结构错误（namespace 不匹配、缺少必填 leaf、类型越界）。减少设备侧 rpc-error 和可能的配置污染。

**范围**：
- 验证 namespace 匹配、必填 leaf 存在、类型约束（uint16 range、string length、enumeration）
- YANG schema 加载失败 → fail-closed 降级为不验证 + 结构化 warning
- 验证结果进入 journal 和审计日志
- 在 `drivers/netconf_backed.py` 的 `prepare()` 方法中，`edit_config` 调用之前插入验证
- 可以通过配置关闭（`yang_runtime_validation: false`），但默认开启

**不做**：
- 不做完整的 runtime discovery（自动生成配置）
- 不用 runtime validation 替代 offline acceptance
- 不让验证结果影响 `WriteDecision`

**前置条件**：YANG Schema 采集完成，有可用的 YANG library。

**详细方案**：`docs/adapter-acceleration-strategy.md` §4.2

**工作量**：M（1 周）
**优先级**：P3
**依赖**：YANG Schema 采集完成

---

## P3/P4: YANG Schema Diff 和 Deviation 发现

**做什么**：对比设备实际行为和 YANG schema，自动发现 deviation。对每个 YANG path（目标功能面），在 candidate 上尝试无害写探测，记录 schema-conformant vs deviated 结果。

**为什么**：H3C Comware7、Huawei VRP8 的 YANG 实现经常和 schema 不一致。在走 YANG 驱动自动生成之前，必须先知道哪些 path 是 schema-conformant 的。

**范围**：
- 在 candidate 上操作，validate 后立即 discard-changes
- 使用隔离测试对象（VLAN ID 4090、ACL ID 3999）
- 每个 probe 必须有 cleanup
- 默认 dry-run only；实际探测需显式 `--probe-mode=active`
- 结果写入 `DeviceModelProfile.yang_conformance`

**不做**：
- 不在 production VLAN/ACL 上做探测
- 不实际提交 candidate 配置

**详细方案**：`docs/adapter-acceleration-strategy.md` §2.3 阶段 B

**工作量**：M（1-2 周）
**优先级**：P3/P4
**依赖**：YANG Schema 采集完成；真实设备可用

---

## P4/条件项: Schema-Driven Renderer 自动生成

**做什么**：对 YANG conformance report 中标记为 schema-conformant 的 paths，从 YANG schema 自动生成 renderer/parser。Deviated paths 仍走手写 renderer。

**为什么**：如果设备 YANG 实现足够规范，可以从 schema 自动生成 renderer/parser，不需要手写 XML 模板。长期看可以大幅减少 O(V × F) 的适配工作量。

**前提条件**：
- YANG Schema Diff 已积累足够的 conformance 数据
- 已知哪些 path 是 schema-conformant 的
- 有 offline acceptance 作为生成代码的质量门禁

**代码结构**：
```text
renderers/
  ├── generated/          # YANG-driven 自动生成
  ├── handwritten/        # 手写（deviated paths）
  └── overrides/          # 人工 override（最高优先级）
```

优先级：override > generated > handwritten。

**安全约束**：
- 生成的代码必须通过 offline acceptance
- Deviated paths 永远不自动生成
- 生成代码头部标注来源和 conformance 比例

**当前现实约束**：H3C Comware7 / Huawei VRP8 的 YANG 实现碎片化严重，短期不具备全面自动生成的条件。

**详细方案**：`docs/adapter-acceleration-strategy.md` §2.3 阶段 C

**工作量**：L（1-2 月）
**优先级**：P4/条件项
**依赖**：YANG Schema Diff 完成；有足够的 schema-conformant paths

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
