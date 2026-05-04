# 轻量 operation 审计日志设计文档

## 目标

为内部系统增加一条本地 append-only JSONL 审计日志，用于保存关键运维事件的原始审计记录。该日志补齐 `OperationSummaryStore` 只保存摘要的缺口，但不扩展为产品审计数据库、RBAC 平台、UI 或外部投递系统。

## 范围

- 新增 `OperationAuditRecord`，从现有 `UnderlayEvent` 派生。
- 新增 `OperationAuditStore` trait 和本地 `JsonFileOperationAuditStore`。
- 审计日志采用 JSONL append-only 写入，单条记录一行。
- 记录字段包括时间、request_id、trace_id、tx_id、device_id、action、result、operator、reason、error 和 fields。
- 支持与 operation summary 类似的本地 retention / rotation。
- 写失败必须可观测：通过 `audit.write_failed` 事件上报，但不阻塞事务、GC、drift 或 worker 主路径。
- worker daemon 支持可选 `operation_audit` 配置块。

## 明确不做

- 不做产品级审计数据库。
- 不做审计查询 API。
- 不做 RBAC 扩展。
- 不做 token 生命周期管理。
- 不做产品 UI。
- 不做外部告警、Webhook、企业 IM、PagerDuty。
- 不修改真实交换机路径。

## 架构

`UnderlayEvent` 仍然是唯一事件源。`RecordingEventSink` 继续负责把事件写入 `OperationSummaryStore`，同时可选写入 `OperationAuditStore`。summary 用于运维查询和告警派生，audit log 用于本地留痕和事后复盘。

写入顺序：

```text
UnderlayEvent
  -> OperationSummaryStore
  -> OperationAuditStore
  -> inner EventSink
```

如果 summary 或 audit 写入失败，`RecordingEventSink` 向 inner sink 发出 `audit.write_failed`。为了避免失败递归，`audit.write_failed` 本身的写失败不会再生成新的失败事件。

## 数据语义

`OperationAuditRecord` 字段：

- `appended_at_unix_secs`：写入时间。
- `request_id` / `trace_id`：请求追踪字段。
- `tx_id` / `device_id`：事务与设备定位字段。
- `action`：由 `AuditRecord::from_event()` 归一化得到。
- `result`：事件结果，缺省为 `observed`。
- `operator_id` / `reason`：从 event fields 中读取，当前只做透传。
- `error_code` / `error_message`：失败上下文。
- `fields`：保留事件字段，供现场排障使用。

## 错误处理

- append 失败返回 `UnderlayError::Internal("operation audit ...")`。
- list / compact 遇到损坏 JSONL 必须 fail-closed，不覆盖原文件。
- compaction 只保留完整 JSONL 行，不拆半行。
- 写失败通过 `UnderlayAuditWriteFailed` 上报，业务路径不因此失败。

## 测试

- JSONL append 后可 list。
- store 重建后仍能读取已有记录。
- 损坏 JSONL 读取 fail-closed。
- retention 按 max_records / max_bytes 保留最新完整记录，并轮转 active 文件。
- `RecordingEventSink` 在 audit 写失败时发出 `audit.write_failed`。
- worker daemon config 能构建带 operation audit 的 runtime。
