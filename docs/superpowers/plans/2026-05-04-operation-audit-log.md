# 轻量 operation 审计日志实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 增加内部本地 append-only JSONL operation 审计日志，补齐关键运维事件留痕。

**Architecture:** 以 `UnderlayEvent` 为唯一事件源，新增 `OperationAuditRecord` 和 `JsonFileOperationAuditStore`。`RecordingEventSink` 可选写入 audit log，写失败发 `audit.write_failed`，但不阻塞主业务路径。

**Tech Stack:** Rust、serde、JSONL、现有 `EventSink`、现有 worker daemon config。

---

## 文件结构

- 修改 `src/telemetry/audit.rs`：新增 operation audit record/store/jsonl/retention。
- 修改 `src/telemetry/sink.rs`：让 `RecordingEventSink` 可选写 operation audit。
- 修改 `src/telemetry/mod.rs`：导出新增类型。
- 修改 `src/worker/daemon.rs`：增加 `operation_audit` worker 配置。
- 修改 `src/worker/runtime.rs`：如需要，挂载 audit retention worker。
- 修改 `tests/telemetry_tests.rs`：新增 audit JSONL、retention、sink failure 测试。
- 修改 `tests/worker_daemon_tests.rs`：新增 daemon 配置接入测试。
- 修改 `docs/examples/underlay-worker-daemon.local.json` 和 production 样例。
- 修改 `docs/runbooks/operator-operations.md` 与当前缺陷清单。

## 任务 1：测试 JSONL 审计 store

- [x] 写 `operation_audit_record_preserves_event_context_and_operator_fields`。
- [x] 写 `json_file_operation_audit_store_persists_records_across_restarts`。
- [x] 实现 `OperationAuditRecord::from_event`、`OperationAuditStore`、`JsonFileOperationAuditStore::record_event/list`。
- [x] 本地 Rust 测试命令已尝试运行；当前机器缺少 `cargo`，需以 GitHub Actions 为 Rust 门禁。

## 任务 2：测试读取损坏文件和 retention

- [x] 写损坏 JSONL fail-closed 测试。
- [x] 写 max_records / max_bytes / rotation 测试。
- [x] 实现 `OperationAuditRetentionPolicy` 和 `compact`。
- [x] 本地 Rust 测试命令已尝试运行；当前机器缺少 `cargo`，需以 GitHub Actions 为 Rust 门禁。

## 任务 3：接入 RecordingEventSink

- [x] 写 `recording_event_sink_persists_operation_audit_records`。
- [x] 写 `recording_event_sink_emits_audit_write_failed_when_operation_audit_persistence_fails`。
- [x] 扩展 `RecordingEventSink`，保留旧构造兼容，并新增 `with_operation_audit_store`。
- [x] 本地 Rust 测试命令已尝试运行；当前机器缺少 `cargo`，需以 GitHub Actions 为 Rust 门禁。

## 任务 4：接入 worker daemon 配置

- [x] 写 worker daemon config 测试，确认 `operation_audit.path` 可被解析并接入。
- [x] 在 `UnderlayWorkerDaemonConfig` 增加 `operation_audit` 配置块。
- [x] worker daemon 构建 `JsonFileOperationAuditStore` 并挂到 `RecordingEventSink`。
- [x] retention 启用时挂载 audit compaction worker。
- [x] 本地 Rust 测试命令已尝试运行；当前机器缺少 `cargo`，需以 GitHub Actions 为 Rust 门禁。

## 任务 5：文档和样例

- [x] 更新 worker daemon local / production JSON 样例。
- [x] 更新操作手册，说明 audit log、summary 和 alert 的区别。
- [x] 更新当前缺陷清单，把轻量 operation 审计 log 标为完成。
- [x] 运行 `git diff --check`。

## 验证

- `git diff --check`
- `cargo test telemetry_tests`
- `cargo test worker_daemon_tests`
- 如果本地没有 `cargo`，推送后以 GitHub Actions 为 Rust 编译和测试门禁。
