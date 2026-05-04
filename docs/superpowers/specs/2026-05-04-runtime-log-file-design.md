# 运行日志落盘设计

## 目标

把现场排障入口收敛到 `/var/log/aria/aria-underlay.log`。运维通过 `tail`、`grep` 或现有日志采集系统读取这个文件，不再扩展 operation audit CLI、查询 API、UI、数据库或外部投递。

## 范围

- `aria-underlay-worker` 的运行事件写到 stderr。
- `aria-underlay-product-api` 的启动、监听、连接失败、退出信息写到 stderr。
- systemd 样例把 stdout/stderr 追加到 `/var/log/aria/aria-underlay.log`。
- tmpfiles 样例创建 `/var/log/aria` 和 `aria-underlay.log`。
- operation summary、operation audit JSONL 和 alert JSONL 继续保留为结构化内部文件，但不是主排障入口。

## 不做

- 不做日志查询 CLI。
- 不做日志查询 HTTP API。
- 不做 UI。
- 不做日志数据库。
- 不做外部 IM、Webhook、PagerDuty 或 email 投递。
- 不做正式安装包。

## 日志格式

运行日志采用单行 key=value 格式，便于 `grep`：

```text
ts=... level=error action=transaction.in_doubt request_id=... trace_id=... tx_id=... device_id=... error_code=...
```

包含空格或特殊字符的值用 JSON 字符串转义。事务、drift、GC、recovery、force-resolve、summary/audit 写失败等事件都来自现有 `UnderlayEvent`，避免另建一套事件模型。

## 落盘方式

应用只负责向 stderr/stdout 打印运行日志。systemd 样例负责将 stdout/stderr 追加到固定文件：

```ini
StandardOutput=append:/var/log/aria/aria-underlay.log
StandardError=append:/var/log/aria/aria-underlay.log
```

这样本地开发仍能直接看到 stderr，生产部署也能统一落到固定日志文件。
