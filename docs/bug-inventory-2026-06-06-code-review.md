# 全量代码 Review 缺陷清单 — 2026-06-06

> 5 个并行 review agent 对最近 ~50 commits 的全量代码审查结果，已逐项验证。

## 核实基线

- 代码：`main` / `0002abf feat: calibrate bgp policy evidence reports`
- 时间：2026-06-06
- 审查范围：BGP intent/model、engine change_plan/diff/normalize、adapter mapper/planner、Python adapter 全栈、Rust service/state/HA
- 初始发现：28 个 | 验证后误报：2 个 | 有效 Bug：26 个

## 总览

| 严重级别 | 数量 | 说明 |
|---------|------|------|
| HIGH | 7 | 生产环境会导致错误行为 |
| MEDIUM | 6 | 特定条件下导致错误结果 |
| LOW | 13 | 影响有限或边缘场景 |
| **合计** | **26** | |

## HIGH — 生产环境会导致错误行为

### H1: ACL 写入被 ReadOnly 误拦截

- **状态**: 已修复 — `fix: separate acl readiness from pbr bgp gates`
- **文件**: `src/engine/change_plan.rs:265-273`
- **描述**: `classify_write_decision` 用 `||` 检查 `pbr_write_readiness == ReadOnly || bgp_write_readiness == ReadOnly`，匹配 `BlastRadius::PolicyReference`（含 ACL 操作）。当 BGP 为 ReadOnly 时，纯 ACL 变更也被错误地返回 `ReadOnly`
- **影响**: 设备 profile 标记 BGP 为 ReadOnly 后，ACL 写配置被意外阻断
- **根因**: `classify_blast_radius` 将 ACL 归类为 `PolicyReference`，但 ReadOnly 检查不区分 ACL 和 PBR/BGP

### H2: ACL 变更被当作 PBR 拒绝

- **状态**: 已修复 — `fix: separate acl readiness from pbr bgp gates`
- **文件**: `src/engine/change_plan.rs:193`
- **描述**: `collect_unsupported_paths` 用 `touches_policy_reference()` 判断 PBR，但该函数匹配所有 ACL 操作。当 `pbr_write_readiness == WriteRejected` 时，纯 ACL 变更被错误标记为 PBR 拒绝
- **影响**: ACL-only 变更在 PBR 不支持的设备上被拒
- **根因**: 与 H1 同一根因 — `touches_policy_reference` 将 ACL 与 PBR 混为一谈
- **修复建议**: 引入 `touches_acl()` 和 `touches_pbr()` 分离判断，ACL 不应受 PBR/BGP readiness 影响

### H3: Apply 锁作用域绕过互斥

- **文件**: `src/tx/domain_lock.rs:19-48` + `src/api/service.rs:165-206`
- **描述**: `Domain`/`Region`/`SwitchPair` 三种锁作用域生成不同的 key（如 `"domain:X"` vs `"switch_pair:X:ep1"`），并发操作同一 domain 时不同 scope 不互斥
- **影响**: 可能导致 lost update 和 shadow 状态不一致
- **状态**: 已修复 — `fix: serialize apply lock scopes by domain`
- **修复建议**: 锁作用域需要层级化或统一使用最细粒度锁

### H4: writable-running rollback 不可达

- **文件**: `adapter-python/aria_underlay_adapter/backends/netconf.py:508-509, 711-717`
- **描述**: `prepare_candidate` 对 writable-running 设备直接将变更写入 running config，但 `rollback_candidate` 对 `RUNNING_ROLLBACK_ON_ERROR` 策略直接抛 `NETCONF_ROLLBACK_STRATEGY_UNSUPPORTED`
- **影响**: prepare 成功后无法回滚（实际影响取决于 Rust coordinator 是否会对 writable-running 路径调用 rollback）
- **状态**: 部分确认 — `commit_candidate` 对该策略正确返回 no-op，但 rollback 路径未实现

### H5: BGP neighbor 地址缺少 IPv4 格式验证

- **文件**: `src/intent/validation.rs:452, 479`
- **描述**: BGP neighbor address 只做 `validate_non_empty`，不验证 IPv4 格式。对比 `validate_acl_endpoint`（line 545）使用 `parse::<Ipv4Addr>()`
- **影响**: `"not-an-ip"` 或 `"10.0.0.999"` 可通过验证直达设备
- **状态**: 已修复 — `fix: validate bgp intent addresses and process conflicts`
- **修复建议**: 添加 `address.parse::<Ipv4Addr>()` 验证

### H6: BGP router_id 零验证

- **文件**: `src/intent/validation.rs:390-415`
- **描述**: `validate_bgp_processes` 从未检查 `router_id: Option<String>`。`normalize.rs:94` 只处理 `Some("")`，不处理 `Some("   ")`（纯空白）或 `Some("foo")`
- **影响**: 无效 router_id 可直达设备导致 NETCONF 错误
- **状态**: 已修复 — `fix: validate bgp intent addresses and process conflicts`
- **修复建议**: 当 `router_id` 为 `Some(value)` 时解析为 `Ipv4Addr`，同时 trim 空白

### H7: BGP process 删除与 neighbor 新增不交叉验证

- **文件**: `src/intent/validation.rs:195-206`
- **描述**: 可以在同一 intent 中删除 BGP process（`delete_bgp_processes`）又在该 VRF 下新增 neighbor（`bgp_neighbors`），无交叉验证
- **影响**: Change plan 会先删 process 再创建 neighbor，设备上会失败
- **状态**: 已修复 — `fix: validate bgp intent addresses and process conflicts`
- **修复建议**: 添加交叉验证，检查 neighbor 的 VRF 不在 process delete 列表中

## MEDIUM — 特定条件下导致错误结果

### M1: `apply_to_shadow` 不级联删除子对象

- **文件**: `src/engine/diff.rs:289-290`
- **描述**: `DeleteBgpProcess` 只 `remove(vrf)` 不清理该 VRF 下的 neighbors。`DeleteAcl` 同理不清理 bindings
- **影响**: Shadow 中出现孤立引用（neighbors 引用不存在的 process，bindings 引用不存在的 ACL）
- **状态**: 已修复 — `fix: cascade shadow deletes and stage bgp neighbor updates`
- **修复建议**: `DeleteBgpProcess` 时 `retain` 移除同 VRF 的 neighbors；`DeleteAcl` 时移除同 acl_id 的 bindings

### M2: `UpdateBgpNeighbor` 分阶段错误

- **文件**: `src/engine/change_plan.rs:130-137`
- **描述**: `UpdateBgpNeighbor` 在 `update_base`（stage 4），而 `UpdateAclBinding` 在 `bind`（stage 5），`CreateBgpNeighbor` 也在 `bind`
- **影响**: 三者中 UpdateBgpNeighbor 分阶段不一致，可能导致更新顺序问题
- **状态**: 已修复 — `fix: cascade shadow deletes and stage bgp neighbor updates`
- **修复建议**: 将 `UpdateBgpNeighbor` 移到 `bind` 阶段

### M3: Python 验证函数对非数字字符串 crash

- **文件**: `adapter-python/aria_underlay_adapter/backends/netconf_state.py:514, 567, 577, 597, 616`
- **描述**: `int(kind or 0)` 对非数字字符串（如 `"hybrid"`）抛 `ValueError` 而非 `_verify_mismatch`。同模式在 `_acl_action_text`、`_acl_protocol_text`、`_acl_kind_text`、`_acl_direction_text` 中重复出现（共 5 处）
- **影响**: Verify 路径 crash 而非返回清晰的 mismatch 错误
- **状态**: 已修复 — `fix: harden python verify enums and h3c interface deletes`
- **修复建议**: 统一用 try/except 处理 `ValueError`，返回 `_verify_mismatch`

### M4: H3C renderer 接口 delete 在 Access 和 Trunk 区段重复发送

- **文件**: `adapter-python/aria_underlay_adapter/renderers/h3c.py:86-93`
- **描述**: 同一个 `delete_interface_names` 列表生成两份 delete 节点，分别放入 `AccessInterfaces` 和 `TrunkInterfaces`
- **影响**: 设备收到同一接口在两个互斥区段中的 delete 操作，可能导致错误或警告
- **状态**: 已修复重复输出 — `fix: harden python verify enums and h3c interface deletes`；精确按当前接口模式选择 Access/Trunk 仍需扩展 desired-state 输入
- **修复建议**: 根据接口实际模式（access/trunk）分别放入对应区段

### M5: 幂等性持久化缺口

- **文件**: `src/api/service.rs:577-583`
- **描述**: `put` 失败后仍执行 `*record = Some(stored_record)`，in-memory slot 被设置。进程内幂等有效，但重启后持久化记录丢失
- **影响**: 进程重启后同一 `idempotency_key` 的请求会重新执行（实际触发概率低）
- **修复建议**: `put` 失败时不设置 in-memory slot，或重试持久化

### M7: H3C 接口正则硬编码 slot `1/0/`

- **文件**: `adapter-python/aria_underlay_adapter/renderers/h3c.py:16-18`
- **描述**: `_H3C_INTERFACE_RE` 正则硬编码 `1/0/`。多 slot 设备（IRF 堆叠的 `GigabitEthernet2/0/1`）会触发 `ValueError("unsupported H3C interface name")`
- **影响**: 多 slot 设备无法使用该 renderer
- **状态**: 已知设计约束，非意外 bug。如需支持多 slot 需扩展正则

## LOW — 影响有限或边缘场景

### L1: BGP 操作不产生 state scope 条目

- **文件**: `src/adapter_client/mapper.rs:182-299`
- **描述**: BGP 操作匹配但不产生 scope 条目
- **根因**: Proto schema 缺 BGP 字段，非代码 bug

### L2: `VerifyScopeSummary` 无 BGP 计数字段

- **文件**: `src/api/response.rs:70-80`
- **描述**: 无 BGP 计数字段
- **根因**: 同 L1，proto schema gap

### L3: `route_policy_refs` 未序列化到 proto

- **文件**: `src/adapter_client/mapper.rs:124-180`
- **描述**: `route_policy_refs` 未传输到 adapter
- **根因**: 同 L1，proto schema gap

### L4: `UpdateAclBinding` rollback 描述用 `after` 而非 `before`

- **文件**: `src/engine/change_plan.rs:494-501`
- **描述**: Rollback 描述用了 `after`（新 binding），正确应该用 `before`（旧 binding）。对比 `UpdateBgpNeighbor`（line 518）正确使用了 `before`
- **影响**: Rollback 日志描述不准确

### L5: commit/verify endpoint 中 `?` 后有不可达死代码

- **文件**: `src/api/apply_coordinator.rs:798-801, 862-865, 876-879`
- **描述**: `rollback_after_endpoint_failure_preserving_primary` 保证返回 `Err`，`?` 传播后下面的 `Err(Internal(...))` 不可达
- **影响**: 3 处相同模式的死代码

### L6: 幂等性 key 无最大长度限制

- **文件**: `src/api/idempotency.rs:79-81`
- **描述**: `normalize_idempotency_key` 只检查空，无长度限制。hex 编码后 key 长度翻倍，可能超 255 字节文件名限制
- **影响**: 超长 key 导致文件写入失败

### L7: 多设备恢复中部分 shadow 写入失败导致不一致

- **文件**: `src/api/recovery_coordinator.rs:479-498`
- **描述**: `shadow_store.put` 按序执行，中间失败导致部分 shadow 已更新、部分未更新
- **影响**: Shadow/journal 状态不一致

### L8: `_normalized_port_mode_kind` 对未知 int 返回原始值

- **文件**: `adapter-python/aria_underlay_adapter/drivers/netconf_backed.py:779-786`
- **描述**: 对未知 int 返回原始整数，导致下游比较逻辑混乱
- **影响**: 会落入错误处理，影响有限

### L9: 密码元素检测遗漏部分变体

- **文件**: `adapter-python/aria_underlay_adapter/tools/sample_collector.py:65-76, 203-206`
- **描述**: 元素名检测是精确匹配，但属性级 `_is_password_attribute` 用后缀匹配，能捕获 `userPassword`、`preSharedKey` 等变体。仍有遗漏（如 `PASSWORD` 全大写元素名）
- **状态**: 部分确认 — 属性级覆盖较好，元素级有遗漏

### L10: `_scope_names_by_ifindex` 正则不处理部分 H3C 接口命名

- **文件**: `adapter-python/aria_underlay_adapter/state_parsers/h3c.py:838-842`
- **描述**: 正则 `r"/(\d+)(?:\.\d+)?$"` 不处理 `Vlan-interface1`、`Bridge-Aggregation1` 等无 `/` 分隔的接口命名
- **影响**: Scope 过滤遗漏部分接口

### L11: `_extract_schema_text` 可能返回 XML 包裹的 schema

- **文件**: `adapter-python/aria_underlay_adapter/backends/yang_schema.py:158-177`
- **描述**: `data.text` 为 None 时 fall through 到 `data_xml`，可能返回 XML 包裹的 schema 文本
- **影响**: 保存的 .yang 文件包含 XML 标签

### L12: ACL binding 删除不验证 ACL 是否存在

- **文件**: `src/intent/validation.rs:617-623`
- **描述**: `validate_acl_binding_deletes` 只检查 ACL ID 范围（2000..=3999），不检查 ACL 是否声明。对比 upsert 路径（line 575）会检查 `declared_acls`
- **影响**: 删除不存在的 binding 时不会报错

### L13: `PartialSuccess` 在补偿计划中被静默丢弃

- **文件**: `src/api/apply_compensation.rs:84`
- **描述**: `PartialSuccess => {}` 空 arm
- **影响**: 当前 `PartialSuccess` 只出现在整体 response status，不会出现在单个 `DeviceApplyResult`，所以是潜在风险而非当前 bug
- **状态**: 部分确认 — 当前无影响，未来可能成为问题

## 下一步优先级建议

**P1（影响正确性，已修复）：**
1. **H1+H2** — ACL readiness gate 已与 PBR/BGP gate 分离
2. **H5+H6+H7** — BGP intent 地址、router_id、process/neighbor 冲突校验已补齐
3. **M1+M2** — Shadow 级联删除与 BGP neighbor stage 已修复
4. **M3+M4** — Python verify enum crash 与 H3C interface delete 重复输出已修复
5. **H3** — Apply lock scope 已按 domain 串行化，防止跨 scope 绕过互斥

**P2（剩余设计/低频正确性项）：**
1. **H4** — writable-running rollback 实现
2. **M5** — 幂等性持久化失败时的 in-memory slot 处理

**P3（边缘/低影响）：**
- L1-L13 可纳入后续迭代，其中 L1-L3 需要 proto schema 扩展
