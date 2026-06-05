# 当前缺陷 / 技术债清单 — 2026-06-05

> 当前有效 bug 基线。本文接续 `docs/bug-inventory-2026-05-31-code-review.md`
> 的 22 项 code-review 缺陷清单，并按 `main` / commit `f28d0ee`
> 重新核实状态。旧清单保留原始 review 证据，不再直接作为 open bug 队列使用。

## 核实基线

- 代码：`main` / `f28d0ee test: add s5560 acl parser fixtures`
- 时间：2026-06-05 16:21:28 +0800
- 本次核实方式：对照 2026-05-31 缺陷清单、后续提交记录和当前源码。
- 本地限制：本机仍无 `cargo`；Rust 编译/测试以 GitHub Actions 为准。

## 当前结论

当前没有已知 P0/P1 事务正确性或 recovery 崩溃安全 open bug。

2026-05-31 code review 发现的 22 个 bug 中：

- 已关闭：#1-#10、#15-#17、#22。
- 旧描述已不准确：#21 设备名匿名化不再是 10000 空间；当前实现使用 24-bit hash
  输出空间，原“约 120 台设备 50% 碰撞概率”不成立。
- 仍待修：7 个 deferred bug/hardening 项，均不阻塞当前 PBR/BGP read-only、
  H3C ACL 验收和事务主路径。

## 已修复

| 编号 | 缺陷 | 当前状态 | 主要证据 |
| --- | --- | --- | --- |
| #1 | 非 ConfirmedCommit 策略成功事务报告 InDoubt | 已修复 | `23f0e27 fix: resolve state machine transition bug for non-ConfirmedCommit strategies` |
| #2 | `recover()` 吞掉 `final_confirm` 的 `AdapterError` | 已修复 | `1aee4c7 fix transaction recovery correctness` |
| #3 | `_commit_locked_candidate` commit 失败后不 discard candidate | 已修复 | `1aee4c7 fix transaction recovery correctness` |
| #4 | Rollback API 调用在 journal persist 之前 | 已修复 | `1aee4c7 fix transaction recovery correctness`, `00da3e6 fix rolling back recovery transition` |
| #5 | 脱敏工具 IP 双重替换 | 已修复 | `1cd5b89 fix sample collector ip sanitization`; `test_ip_replacement_does_not_rewrite_generated_addresses` |
| #6 | 脱敏工具不处理 XML tail text | 已修复 | `1cd5b89 fix sample collector ip sanitization`; `test_sanitize_ip_addresses_in_tail_text` |
| #7 | 脱敏工具 IP 替换池碰撞 | 已修复 | `1cd5b89 fix sample collector ip sanitization`; `test_ip_replacement_pool_fails_closed_instead_of_colliding` |
| #8 | journal/shadow lock table 不淘汰 | 已修复 | `4f56ab1 fix gc orphan artifacts and lock pruning` |
| #9 | GC 不清理孤立 artifacts | 已修复 | `4f56ab1 fix gc orphan artifacts and lock pruning` |
| #10 | Worker panic 后不重启 | 已修复 | `888b8a8 restart workers after panic` |
| #15 | `--from-file` 非 UTF-8 / binary / malformed XML 可能 raw traceback | 已修复 | `test_from_file_rejects_non_utf8_input`, `test_from_file_rejects_malformed_xml` |
| #16 | `--from-file` 和 `--output` 同路径会覆盖原始文件 | 已修复 | `test_from_file_rejects_same_input_and_output_path` |
| #17 | XML attribute secret/community 字段未整值 redaction | 已修复 | `test_sensitive_attributes_are_redacted_by_attribute_name` |
| #22 | Recovery 第一个设备失败后放弃剩余设备 | 已修复 | `ecfb642 fix multi-device recovery attempts` |

## 旧描述失效

| 编号 | 当前状态 | 说明 |
| --- | --- | --- |
| #21 | 旧描述失效 | 当前 `_anonymize_device_name` 使用 MD5 前 6 个十六进制字符转整数，输出空间约 16M，不是原清单描述的 10000。仍可作为“如需零碰撞就跟踪已分配名称”的可选增强，但不再算当前 confirmed bug。 |

## 当前 Confirmed-open / Deferred

这些问题当前仍能从代码中复核到，优先级低于事务/recovery/PBR-BGP 验证主线。

| 编号 | 优先级 | 缺陷 | 当前证据 | 建议 |
| --- | --- | --- | --- | --- |
| #11 | P3 | `_port_mode_to_proto` 不接受数值 `kind` | `adapter-python/aria_underlay_adapter/drivers/netconf_backed.py` 仍只接受 `"trunk"` / `"access"` 字符串。当前 H3C parser 返回字符串，影响低。 | 修测试和映射，或明确 parser contract 只允许字符串。 |
| #12 | P3 | YANG namespace 只搜索前 2000 字符 | `_extract_namespace()` 仍使用 `schema_text[:2000]`。 | 改为全文搜索或 preamble 范围搜索。 |
| #13 | P3 | `collect-device-sample --password` 暴露在进程列表 | CLI 仍提供 `--password` 参数。 | 移除参数或打印安全警告并推荐交互/环境变量。 |
| #14 | P3 | sample collector 默认跳过 host-key verify | `collect_and_sanitize_sample(... hostkey_verify=False)`，CLI 用 `--verify-hostkey` opt-in。 | 改成显式 `--skip-hostkey-verify` 或在默认跳过时警告。 |
| #18 | P3 | `_safe_path_component` 不过滤 `..` | 仍只替换 `/` 和 `\`。 | 替换 `..` 或验证最终路径仍在 YANG library root 下。 |
| #19 | P3 | `confirm_timeout_secs=0` 传入 Python NETCONF commit 时被替换为 120 | `session.commit(timeout=confirm_timeout_secs or 120)` 仍存在。Rust service 会把 timeout clamp 到至少 1，实际触发面窄。 | 明确禁止 0 或在 adapter 端也 clamp/校验。 |
| #20 | P3 | `load_yang_library` index 标记已下载但文件缺失时返回 `schema_downloaded=True` 和空 text | 仍按 index 原值返回 `schema_downloaded`。 | 文件缺失时改为 `schema_downloaded=False` 并带 warning/error。 |

## 下一步建议

短期不建议把这些 P3 hardening 当作最高优先级全部清完后再开发。当前更合理的顺序是：

1. 第二批修 YANG schema 边界：#18、#20、#12，因为 YANG schema collection 会支撑后续 OpenConfig / native YANG path-level profile。
2. 第三批修 sample collector CLI 安全策略：#13、#14。
3. 第四批修 adapter 输入一致性：#11、#19。
4. 功能开发上仍建议继续 BGP 相关路线：先做真实 H3C BGP running XML 样本校准和 path-level profile evidence，不直接做 BGP 写配置 renderer。
