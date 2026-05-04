# 运维操作手册

## 范围

本文记录当前内部系统的本地运维入口。当前不做产品 UI、外部系统、SSO/OIDC/JWT/JWKS、外部告警投递、仓库内 ingress/TLS、安装包生成。

## 已有入口

- `aria-underlay-ops`：读取 操作摘要、告警、worker reload status 等本地状态。
- `aria-underlay-worker`：运行 GC、drift 审计、summary compaction、告警 生成等 worker。
- `aria-underlay-product-api`：loopback HTTP API，使用 `static_tokens` 中配置的 bearer token。

## 本地文件

- 运行日志：`/var/log/aria/aria-underlay.log`，主排障入口。
- 操作摘要：JSONL。
- operation 审计日志：append-only JSONL，记录从 `UnderlayEvent` 派生的完整本地审计记录。
- 操作告警：JSONL。
- 告警生命周期状态：JSON。
- 工作进程热加载检查点：JSON。

## 操作摘要、审计和告警的区别

- 运行日志用于日常查错和现场排障，优先从 `/var/log/aria/aria-underlay.log` 看错误、事务状态、drift、GC、worker reload 和 product API 连接问题。
- 操作摘要用于运维查询和 overview 统计，字段较少，面向“现在有什么风险”。
- operation 审计日志用于内部结构化留痕，字段更完整，包含 request_id、trace_id、tx_id、device_id、action、result、operator、reason、error 和 fields；当前不要求通过 CLI 查询它。
- 操作告警由摘要派生，面向需要人工处理的风险。
- 三者都使用本地 JSON/JSONL 文件；当前不做审计数据库、UI 或外部投递。

## 运行日志

systemd 样例把 `aria-underlay-worker` 和 `aria-underlay-product-api` 的 stdout/stderr 追加到：

```text
/var/log/aria/aria-underlay.log
```

常用排障命令：

```bash
tail -f /var/log/aria/aria-underlay.log
grep 'level=error' /var/log/aria/aria-underlay.log
grep 'tx_id=<事务ID>' /var/log/aria/aria-underlay.log
grep 'device_id=<设备ID>' /var/log/aria/aria-underlay.log
```

日志使用单行 key=value 格式，便于 `grep`。关键字段包括 `ts`、`level`、`component` 或 `action`、`request_id`、`trace_id`、`tx_id`、`device_id`、`result`、`error_code` 和 `error_message`。

## 产品 API 身份

产品 API 只用于内部调用。请求头使用：

```http
Authorization: Bearer <token>
```

Token 在 `product-api.local.json` 或 `product-api.production.json` 的 `static_tokens` 中手工配置。当前不做 token 创建、轮换、撤销工具。

## 产品 API 部署

默认绑定 `127.0.0.1:8088`。跨机器访问、TLS、client auth、rate limit 和 proxy header 策略由部署侧自行处理，仓库内不实现 ingress。

## 工作进程部署样例

仓库提供 systemd、tmpfiles、logrotate 和 JSON 配置样例，但不生成 deb/rpm/tar 安装包。部署方负责用户创建、二进制放置、服务启用和目录权限；样例中的运行日志统一落到 `/var/log/aria/aria-underlay.log`。

`docs/examples/logrotate.d/aria-underlay` 是运行日志轮转样例：默认每天轮转，保留 30 份，跳过空文件，使用 `copytruncate` 兼容当前 systemd `append:` 写法，避免日志文件无限增长。

## 告警处理

1. 读取 告警 summary。
2. 按 severity 或 dedupe key 查看 告警。
3. 对需要处理的告警执行 acknowledge。
4. 处理完成后 resolve，或按运维策略 suppress/expire。
5. 高危事务先查看 InDoubt 状态，再决定是否 force-resolve。

## 当前不做

- 外部 IM、PagerDuty、email、webhook。
- product 审计 database。
- 产品 UI。
- token 生命周期。
- 仓库内 ingress/TLS。
- 安装包。
