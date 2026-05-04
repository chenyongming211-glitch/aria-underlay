# 产品 HTTP 路由契约设计文档

> 本文档已经中文化。代码标识符、命令、文件路径和错误码保留英文原文。

## 设计目标

定义 framework-neutral method/path/status/body JSON 语义，避免 handler 绕过业务边界。

## 设计原则

- 复用现有架构边界，不为单个需求新造大平台。
- 读写路径要可测试、可审计、失败语义清晰。
- 本地/样本/骨架 能力只证明开发边界，不代表生产可用。
- 涉及真实交换机、真实 ingress、安装包、外部系统的内容默认不在当前范围。

## 行为边界

- 对外暴露的 API 或 CLI 必须有明确输入、输出和错误码。
- 高风险操作必须保留 request_id、trace_id、operator、reason 等可追踪字段。
- 文件写入采用原子写或 append-only 语义，避免半写入状态。
- 配置无效时拒绝启动或拒绝采用新配置，不静默降级。

## 测试要求

- 覆盖成功路径。
- 覆盖权限/输入/配置错误。
- 覆盖写失败或外部依赖失败时的 失败关闭 行为。
- 没有真实交换机时，只允许 模拟适配器、样本、快照 和离线 校验器 验证。


## 当前收敛边界

- 当前是内部系统，不做外部系统集成。
- 不做 SSO、OIDC、JWT、JWKS、refresh token、浏览器会话。
- 不做产品 UI、外部告警投递、企业 IM、PagerDuty、Webhook。
- 不在仓库内实现 ingress、TLS、client auth、rate limit、proxy header。
- 不生成 deb/rpm/tar 安装包；systemd、tmpfiles 和 JSON 文件只作为部署样例。
- 没有真实交换机前，Huawei/H3C 解析器 和 渲染器 只能 样本/快照 验证，不能标记 生产就绪。
