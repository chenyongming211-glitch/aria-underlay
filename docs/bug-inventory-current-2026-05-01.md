# 当前缺陷 / 技术债清单 — 2026-05-01

## 当前基线

最新有效基线以 `main` 上的 CI 绿色提交为准。当前产品方向已经收敛为内部系统：不做 SSO/OIDC/JWT/JWKS，不做产品 UI，不做外部告警投递，不在仓库内实现 ingress/TLS，不生成安装包。

## 已收敛完成的历史问题

以下问题已经通过前序修复包关闭，不应重复作为 open bug：

- 事务 committed 与 shadow 写入顺序。
- 批量恢复 TOCTOU。
- 部分失败被汇总为成功。
- journal 错误历史丢失。
- 孤儿密钥 清理。
- 适配器客户端 连接 churn。
- Drifted 生命周期清理。
- desired baseline 与 observed cache 混用。
- file-backed journal/shadow 并发和崩溃弱点。
- Rust 到 Python 的 主机密钥策略 传递。
- TOFU host-key store。
- Python placeholder 后端 改为明确 unsupported。
- Rust 死 渲染器骨架 清理。
- 轻量 operation 审计 log：append-only JSONL、关键字段、retention/rotation 和写失败可观测性。

## 当前仍需要做

| 区域 | 状态 | 下一步 |
| --- | --- | --- |
| 真实 NETCONF 解析器 | Huawei/H3C 仅 样本验证 | 等真实 XML 或真实交换机后验证，不提前 生产就绪 |
| 真实 NETCONF 渲染器 | Huawei/H3C 仍是 骨架/快照 验证 | 等真实设备验证后再讨论 生产就绪 |
| 主机密钥策略 | fingerprint-only pinning 仍未完整实现 | 暂不扩展；保持 失败关闭 |
| Drift policy | AutoReconcile 明确未实现 | 暂不开发；保持 unsupported |
| Vendor scope | Cisco/Ruijie 未实现 | 等样本和明确需求 |
| Alternate 后端s | NAPALM/Netmiko/SSH CLI 未实现 | 等明确 后端 合同和测试需求 |
| Force unlock | NETCONF kill-session/force-unlock 未实现 | 等设备会话身份和审计需求明确后再设计 |

## 明确不做

- product 审计 database。
- 扩展 RBAC 平台。
- token 创建、轮换、撤销工具。
- SSO/OIDC/JWT/JWKS。
- 产品 UI。
- 外部告警投递。
- 仓库内 ingress/TLS/client-auth/rate-limit/proxy-header。
- deb/rpm/tar 安装包或多平台 installer。

## 当前执行计划

1. 没有真实交换机前，不推进 解析器/渲染器 生产化。
2. 后续只做小范围清理和回归测试加固，不继续扩展产品平台能力。
