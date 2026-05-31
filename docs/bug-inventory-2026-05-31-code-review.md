# 代码 Review 缺陷清单 — 2026-05-31

> 本次 review 覆盖 Rust Core（事务引擎、状态机、worker、GC、recovery）和 Python Adapter（NETCONF backend、driver、tools、YANG schema），共发现并核实 22 个 bug。

## 核实基线

- 代码：`main` / commit `91147ee`
- 本地验证：`python3 -m pytest -q adapter-python/tests` → `377 passed`
- Rust 测试：本地无 `cargo`，以 GitHub Actions 为准
- Review 方法：三个并行 agent 分别审查 sample collector、Python adapter backends、Rust transaction engine，再人工逐一读代码核实

---

## CRITICAL

### Bug #1: 非 ConfirmedCommit 策略的成功事务全部报告 InDoubt

**文件**：`src/api/apply_coordinator.rs:295`、`src/api/apply_coordinator.rs:605-613`、`src/tx/phase_transition.rs:24-56`

**现象**：`CandidateCommit`、`RunningRollbackOnError`、`BestEffortCli` 三种策略的成功事务，即使设备已正确配置，也会报告 `ApplyStatus::InDoubt`。

**根因**：
1. `apply_changed_endpoint_states` 对每个 endpoint 依次调用 prepare → commit → verify → (可选) final_confirm
2. `verify_endpoint` 把 journal phase 转为 `Verifying`（line 723）
3. 只有 `ConfirmedCommit` 策略才调用 `final_confirm_endpoint`（line 605-613），其他策略直接返回
4. `finish_successful_apply` 尝试 `journal_record.transition_phase(TxPhase::Committed)`（line 295）
5. 状态机 `phase_transition.rs:31` 只允许 `Verifying → FinalConfirming → Committed`，不允许 `Verifying → Committed`
6. 转换失败 → `InvalidPhaseTransition` → 返回 `ApplyStatus::InDoubt`

**影响**：
- 设备已配置但 shadow 不更新
- 事务卡在 InDoubt，需要人工介入或 recovery
- 除 ConfirmedCommit 外，所有策略实质不可用

**修复方向**：在 `finish_successful_apply` 中，对非 ConfirmedCommit 策略，先插入 `Verifying → FinalConfirming` 过渡，再 `FinalConfirming → Committed`；或在状态机中允许 `Verifying → Committed`（需限制仅在非 CC 策略下）。

**优先级**：P0 — 直接影响事务正确性

---

## HIGH

### Bug #2: recover() 吞掉 final_confirm 的 AdapterError，误触发 cancel_commit

**文件**：`adapter-python/aria_underlay_adapter/drivers/netconf_backed.py:296-317`

**现象**：当 `recover()` RPC 调用 `final_confirm` 时，如果 `final_confirm` 抛出非 persist-id 类的 `AdapterError`（如网络超时、session 断开、设备 RPC 错误），错误被吞掉，执行继续走到 `rollback_candidate`。

**根因**：
```python
# line 302-307
except AdapterError as error:
    if _persist_id_already_consumed(error):
        return _recover_response(...)
    # ← 没有 return！执行流落到 line 316
```

**影响**：
- 对 ConfirmedCommit 策略，rollback 会调用 `cancel_commit(persist_id=tx_id)`
- 如果设备端 commit 实际已 pending（只是 `final_confirm` RPC 传输超时），`cancel_commit` 会撤销一个有效的 pending commit
- 与 recovery 的本意完全相反

**修复方向**：在 `_persist_id_already_consumed` 检查后加 `return pb2.RecoverResponse(result=_failed_result(error))`。

**优先级**：P0 — 直接影响 recovery 正确性

---

### Bug #3: _commit_locked_candidate commit 失败后不 discard candidate

**文件**：`adapter-python/aria_underlay_adapter/backends/netconf.py:524-577`

**现象**：当 `_commit_locked_candidate` 中 commit 失败时，只 unlock candidate 但不 discard 未提交的 changes。

**根因**：对比 `prepare_candidate`（line 308-313）在 edit/validate 失败时正确调用了 `_discard_candidate_preserving_error`，commit 路径（line 551-565）只捕获错误并 unlock，没有 discard。

**影响**：
- Candidate datastore 保留 stale changes
- 重试时：新的 `prepare` 锁定 candidate（仍有旧 changes），`edit-config merge` 在 stale changes 之上叠加新 changes
- 如果旧 commit 实际已在设备端成功（只是 response 丢失），重试会导致配置重复应用

**修复方向**：在 `_commit_locked_candidate` 的错误路径中，unlock 前调用 `_discard_candidate_preserving_error(session, original_error)`，与 `prepare_candidate` 保持一致。

**优先级**：P1 — 影响重试正确性

---

### Bug #4: Rollback API 调用在 journal persist 之前（崩溃安全）

**文件**：`src/api/apply_coordinator.rs:826-839`

**现象**：在 `rollback_after_endpoint_failure` 中，先调用 `rollback_with_context()`（line 833-835），再写 `RollingBack` journal（line 837-839）。

**根因**：
```rust
let rollback_result = client
    .rollback_with_context(device, context, journal_record.strategy)
    .await;                           // ← API 调用先执行

let mut rolling_back_record = journal_record.clone();
rolling_back_record.transition_phase(TxPhase::RollingBack)?;
self.journal.put(&rolling_back_record)?;  // ← journal 写入后执行
```

**影响**：
- 如果进程在这两步之间崩溃，设备已 rollback 但 journal 仍显示之前的 phase（如 `Committing`）
- Recovery 看到 `Committing` → `classify_recovery` 映射为 `RecoveryAction::AdapterRecover` → 调用 `adapter.recover()`
- Recovery 可能重新应用已被 rollback 的变更

**修复方向**：先写 `RollingBack` journal，再调用 rollback API。

**优先级**：P1 — 崩溃安全窗口

---

### Bug #5: 脱敏工具 IP 替换存在双重替换 bug

**文件**：`adapter-python/aria_underlay_adapter/tools/sample_collector.py:180-186`

**现象**：用 `str.replace()` 循环替换 IP 时，新产生的文档地址可能被后续循环再次替换。

**根因**：
```python
ips_found = _IPV4_PATTERN.findall(element.text)
for ip in ips_found:
    new_ip = self._replace_ip(ip)
    element.text = element.text.replace(ip, new_ip)  # 新 IP 也在文本中被替换
```

**场景**：原始文本 `"src 10.0.0.1 dst 192.0.2.1"`
- `findall` 返回 `['10.0.0.1', '192.0.2.1']`
- 替换 `10.0.0.1` → `192.0.2.1`：文本变成 `"src 192.0.2.1 dst 192.0.2.1"`
- 替换 `192.0.2.1` → `198.51.100.2`：文本变成 `"src 198.51.100.2 dst 198.51.100.2"`
- `_ip_mapping['10.0.0.1']` 是 `192.0.2.1`，但文本中显示 `198.51.100.2` — **不一致**

**修复方向**：用 `re.sub` 配合 callback 函数，一次性替换所有匹配，避免链式替换。

**优先级**：P1 — 数据正确性

---

### Bug #6: 脱敏工具不处理 XML tail text — IP/密码泄漏

**文件**：`adapter-python/aria_underlay_adapter/tools/sample_collector.py:180`

**现象**：`element.tail`（子元素闭合标签后的文本）从不被检查。混合内容中的敏感数据会泄漏。

**根因**：`_sanitize_element` 只处理 `element.text`（line 180），不处理 `element.tail`。

**场景**：
```xml
<Route>
  <NextHop>via</NextHop>10.0.0.1
</Route>
```
IP `10.0.0.1` 在 `<NextHop>` 的 tail text 中，不会被脱敏。

**修复方向**：在 `_sanitize_element` 中对 `element.tail` 也执行 IP regex 扫描和替换。

**优先级**：P1 — 敏感数据泄漏

---

## MEDIUM

### Bug #7: 脱敏工具 IP 替换 762 个后碰撞

**文件**：`adapter-python/aria_underlay_adapter/tools/sample_collector.py:117-122`

**现象**：3 个前缀 × 254 个 host，LCM = 762。超过 762 个唯一 IP 后，新产生的替换地址与已分配的地址碰撞。

**根因**：
```python
prefix = _DOCUMENTATION_IPS[self._ip_counter % len(_DOCUMENTATION_IPS)]  # mod 3
new_ip = f"{prefix}{(self._ip_counter % 254) + 1}"  # mod 254
```
无碰撞检测。大型配置（如 BGP full table 的 core router）可能超过 762 个唯一 IP。

**修复方向**：跟踪已分配的替换地址，碰撞时跳过或扩展地址空间。

**优先级**：P2 — 大型配置场景

---

### Bug #8: Lock DashMap 永不淘汰（内存泄漏）

**文件**：`src/tx/journal.rs:219`、`src/state/shadow.rs:97`

**现象**：`JsonFileTxJournalStore` 和 `JsonFileShadowStateStore` 的 `DashMap<String/DeviceId, Arc<Mutex<()>>>` 锁表只有 `or_insert_with`，没有淘汰机制。

**影响**：长期运行的 daemon，每个完成的事务和每个接触过的设备都会在 DashMap 中留下永久条目，内存线性增长。

**修复方向**：
1. 用弱引用模式（`Arc::downgrade`），无持有者时自动清理
2. 在 GC 删除 journal/artifact 时同步删除 DashMap 条目
3. 定期扫描并清理无活跃等待者的条目

**优先级**：P2 — 长期运行稳定性

---

### Bug #9: GC 不清理孤立 artifacts

**文件**：`src/worker/gc.rs:396`

**现象**：`prune_artifacts_per_device` 只考虑 tx_id 匹配 terminal journal 的 artifact。当 journal record 被 retention 策略删除后，对应的 artifact 目录成为孤儿，永远不被清理。

**根因**：
```rust
let Some(updated_at) = terminal_by_tx.get(tx_id) else {
    continue;  // 跳过孤立 artifact
};
```

**影响**：artifact 目录无限积累，消耗磁盘空间。

**修复方向**：将没有匹配 journal 的 artifact 视为孤立，立即删除或在单独的 retention period 后删除。

**优先级**：P2 — 磁盘空间管理

---

### Bug #10: Worker panic 后不重启

**文件**：`src/worker/runtime.rs:217-235`

**现象**：worker task panic 时，`JoinSet` 捕获 `JoinError` 并记录，但不重启 worker。

**影响**：
- `ConfirmedCommitTimeoutWatcher` panic → timed-out confirmed commits 永远不被 recovery
- `DriftAuditor` panic → drift 永远不被检测
- `JournalGcWorker` panic → journal 和 artifacts 永远不被清理
- 需要重启整个 daemon 才能恢复

**修复方向**：用指数退避重启 panic 的 worker，或至少发出 critical alert 触发外部进程监控。

**优先级**：P2 — 长期运行可靠性

---

### Bug #11: _port_mode_to_proto 不支持数值 kind

**文件**：`adapter-python/aria_underlay_adapter/drivers/netconf_backed.py:448-453`

**现象**：`_port_mode_to_proto` 对 ACL/protocol/direction 等字段支持数值枚举，但 port mode 只处理字符串 `"trunk"` / `"access"`。

**根因**：
```python
kind = raw_kind.strip().lower() if isinstance(raw_kind, str) else raw_kind
# 后续只比较 kind == "trunk" / kind == "access"
```
如果 state parser 返回数值 protobuf enum（如 `1`），会落到 line 477-481 的 `INVALID_PORT_MODE` 错误。

**影响**：不确定 state parser 是否会实际产生数值 kind。当前 H3C state parser 返回字符串，实际影响可能较低。

**修复方向**：统一处理字符串和数值 kind，或文档明确只接受字符串。

**优先级**：P3 — 一致性，实际影响待验证

---

### Bug #12: YANG namespace 提取截断到前 2000 字符

**文件**：`adapter-python/aria_underlay_adapter/backends/yang_schema.py:180-193`

**现象**：`_extract_namespace` 只搜索前 2000 字符。大型 YANG module 的 namespace 可能超出此范围。

**根因**：
```python
header = schema_text[:2000]
match = re.search(r'namespace\s+["\']([^"\']+)["\']', header)
```

**影响**：2000 字符通常足够覆盖 YANG module preamble（`module` 声明 + `import` + `revision` + `namespace`）。但 H3C Comware 等大型 YANG module 可能有极长的 `description`、`organization`、`contact` 或多个 `import`/`revision` 块，使 namespace 超出 2000 字符。此时返回空字符串，污染 YANG library 索引。

**修复方向**：搜索整个 schema 文本，或至少搜索到 `module <name> {` 后的第一个 `{` 所界定的 preamble 结束。

**优先级**：P3 — 大型 YANG module 场景

---

## LOW

### Bug #13: --password CLI 参数暴露在 ps aux

**文件**：`adapter-python/aria_underlay_adapter/tools/sample_collector.py:334-338`

**现象**：`--password` 接受密码作为命令行参数。在多用户系统上，`ps aux` 可见。

**修复方向**：移除 `--password` 参数，强制使用 `getpass` 交互输入；或至少在检测到 `--password` 时打印安全警告。

**优先级**：P3 — 安全最佳实践

---

### Bug #14: hostkey_verify=False 默认启用 MITM 风险

**文件**：`adapter-python/aria_underlay_adapter/tools/sample_collector.py:229`

**现象**：`collect_and_sanitize_sample` 默认 `hostkey_verify=False`，跳过 SSH host key 验证。

**影响**：连接可能被 MITM 攻击，密码在 SSH 会话建立前以明文传输。

**修复方向**：至少打印安全警告，或改为默认 `True` 并要求显式 `--skip-hostkey-verify`。

**优先级**：P3 — 安全最佳实践

---

### Bug #15: --from-file 模式未处理非 UTF-8/binary 异常

**文件**：`adapter-python/aria_underlay_adapter/tools/sample_collector.py:355-373`

**现象**：`--from-file` 路径没有 try/except。如果文件是 binary、非 UTF-8 或包含 malformed XML，用户看到 raw Python traceback。

**修复方向**：添加 try/except，捕获 `UnicodeDecodeError`、`ValueError`（来自 `sanitize_xml`），打印用户友好的错误信息。

**优先级**：P3 — 用户体验

---

### Bug #16: --from-file 和 --output 相同时覆盖原文件

**文件**：`adapter-python/aria_underlay_adapter/tools/sample_collector.py:355-368`

**现象**：如果用户传入相同的 `--from-file` 和 `--output` 路径，原始文件被静默覆盖为脱敏版本，原始数据丢失。

**修复方向**：检查 `args.from_file.resolve() == args.output.resolve()`，如果相同则报错或提示确认。

**优先级**：P3 — 数据安全

---

### Bug #17: XML attributes 中的 IP 不脱敏

**文件**：`adapter-python/aria_underlay_adapter/tools/sample_collector.py:180`

**现象**：`element.attrib` 从不被检查。IP、密码、community string 如果在 XML 属性中，会原样保留。

**影响**：H3C NETCONF 配置中确实使用属性值 IP（如 `<Peer IP="10.0.0.1"/>`）。测试 `test_ip_in_attributes`（line 277-290）记录了此行为但未修复。

**修复方向**：对 `element.attrib` 也执行 IP regex 扫描和敏感字段检查。

**优先级**：P3 — 完整性

---

### Bug #18: _safe_path_component 不过滤 .. 路径穿越

**文件**：`adapter-python/aria_underlay_adapter/backends/yang_schema.py:330-335`

**现象**：`_safe_path_component` 替换 `/` 和 `\` 但不过滤 `..`。`vendor="../.."` 会通过。

**影响**：`save_yang_library` 只创建目录，风险较低（输入来自设备 capabilities，通常可信）。但如果 vendor/model/os_version 来自不可信源，可能在 YANG library root 之外创建目录。

**修复方向**：额外替换 `..`，或验证解析后的路径在预期 root 之下。

**优先级**：P3 — 安全边界

---

### Bug #19: confirm_timeout_secs=0 被静默替换为 120

**文件**：`adapter-python/aria_underlay_adapter/backends/netconf.py:612`

**现象**：`session.commit(timeout=confirm_timeout_secs or 120)` 把 `0` 当 falsy，静默替换为 120。

**修复方向**：改为 `confirm_timeout_secs if confirm_timeout_secs else 120`，或修改参数默认值。

**优先级**：P3 — 语义清晰度

---

### Bug #20: load_yang_library 文件缺失时 schema_downloaded=True 但 schema_text=""

**文件**：`adapter-python/aria_underlay_adapter/backends/yang_schema.py:286-306`

**现象**：当 index 标记 `schema_downloaded=True` 但 `.yang` 文件已从磁盘删除（部分清理、磁盘损坏），返回的 `YangSchemaResult` 有 `schema_downloaded=True` 但 `schema_text=""`。

**影响**：信任 `schema_downloaded` 的调用者会拿到空字符串。

**修复方向**：文件缺失时将 `schema_downloaded` 设为 `False`，或发出 warning。

**优先级**：P3 — 数据一致性

---

### Bug #21: 脱敏工具设备名匿名化碰撞概率

**文件**：`adapter-python/aria_underlay_adapter/tools/sample_collector.py:139-143`

**现象**：`device-{hash_val:04d}` 产生 10000 种可能名称。按生日悖论，~120 台设备时碰撞概率约 50%。

**影响**：两个不同的真实设备名可能映射到同一个 `device-XXXX`，使分析不可靠。不是安全 bug（匿名化不是加密），但影响数据可用性。

**修复方向**：增加输出空间（如 6 位十六进制 = 16M 种可能），或跟踪已分配名称并避免碰撞。

**优先级**：P3 — 数据可用性

---

### Bug #22: Recovery 第一个设备失败后放弃剩余设备

**文件**：`src/api/recovery_coordinator.rs:345`

**现象**：`recover_final_confirming_record` 遍历设备时，第一个设备失败即返回 `Err`，放弃剩余设备。

**影响**：多设备事务中，单个设备 recovery 失败导致整个事务标记为 InDoubt，即使其他设备可以成功 recovery。

**修复方向**：尝试所有设备，聚合结果为 `PartialSuccess`，类似 `apply_desired_states` 的处理方式。

**优先级**：P3 — 多设备 recovery 健壮性

---

## 总结

| 严重程度 | 数量 | 编号 |
|---------|------|------|
| CRITICAL | 1 | #1 |
| HIGH | 5 | #2, #3, #4, #5, #6 |
| MEDIUM | 6 | #7, #8, #9, #10, #11, #12 |
| LOW | 10 | #13-#22 |

**修复优先级建议**：
1. **立即修复**：#1（CRITICAL，事务正确性）
2. **尽快修复**：#2, #3, #4（HIGH，错误处理和崩溃安全）、#5, #6（HIGH，脱敏工具正确性）
3. **后续迭代**：#7-#12（MEDIUM，长期运行稳定性和边缘场景）
4. **低优先级**：#13-#22（LOW，最佳实践、用户体验、安全边界）

---

## 下一步行动

- [ ] 创建 GitHub issue 跟踪 #1-#6
- [ ] 为 #1（状态机转换 bug）编写复现测试
- [ ] 为 #2（recover 错误吞掉）编写复现测试
- [ ] 为 #5（IP 双重替换）编写复现测试
- [ ] 修复 #1 并通过 GitHub Actions 验证
- [ ] 修复 #2-#6 并本地 pytest 验证
- [ ] 更新 `docs/bug-inventory-current-*.md` 反映本次 review 结果
