# Sprint 2 渲染器骨架计划

## 目标

为 Huawei/H3C 建立 骨架 渲染器 和 快照 工具，但生产路径保持 失败关闭。

## 当前边界

- 没有真实交换机前，只做 模拟、样本、快照 和离线 校验器。
- 骨架 渲染器/解析器 不能标记 生产就绪。
- 真实设备 XML 或硬件到位后再做生产化验证。

## 验证

- Python 相关变更运行 `python3 -m pytest adapter-python/tests -q`。
- Rust 相关变更以 GitHub Actions 为准。

## 不做

- 不做外部系统、产品 UI、安装包、仓库内 ingress/TLS。
