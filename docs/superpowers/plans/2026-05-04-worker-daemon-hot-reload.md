# 工作进程守护模式热加载实施计划

> 本文档已经中文化。代码标识符、命令、文件路径和错误码保留英文原文。

## 目标

让运行中的 worker 能感知有效配置变更，非法变更写入 rejected checkpoint。

## 实施范围

- 保持改动聚焦在该主题对应的文件和测试。
- 优先使用现有 trait、manager、驱动、registry 和 CLI 边界。
- 所有失败路径保持 失败关闭；不能把 骨架、样本 或本地样例冒充生产可用。
- 只做当前内部系统需要的最小能力，不扩展成产品平台。

## 主要任务

1. 先补或保留对应回归测试。
2. 实现最小闭环，保持已有边界不被绕过。
3. 更新 操作手册、progress 或 bug inventory，明确完成状态和剩余限制。
4. 运行本地可执行检查；Rust 本地不可用时，以 GitHub Actions 作为 Rust 编译和测试门禁。

## 验证要求

- `git diff --check` 必须通过。
- Python adapter 相关变更运行 `python3 -m pytest adapter-python/tests -q`。
- Rust 相关变更运行对应 `cargo test`；如果本机没有 `cargo`，必须推送后等待 GitHub Actions 绿色。


## 当前收敛边界

- 当前是内部系统，不做外部系统集成。
- 不做 SSO、OIDC、JWT、JWKS、refresh token、浏览器会话。
- 不做产品 UI、外部告警投递、企业 IM、PagerDuty、Webhook。
- 不在仓库内实现 ingress、TLS、client auth、rate limit、proxy header。
- 不生成 deb/rpm/tar 安装包；systemd、tmpfiles 和 JSON 文件只作为部署样例。
- 没有真实交换机前，Huawei/H3C 解析器 和 渲染器 只能 样本/快照 验证，不能标记 生产就绪。
