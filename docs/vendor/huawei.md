# Huawei 交换机适配记录

## 当前状态

Huawei 解析器 目前是 样本验证，渲染器 仍是 骨架/快照 验证。没有真实设备 XML 前不能标记 生产就绪。

## 能力检查清单

- NETCONF 能力。
- candidate / confirmed-commit 支持情况。
- running config 的 VLAN namespace。
- 接口 description、access、trunk 字段路径。
- 错误返回格式和锁行为。

## VLAN / 接口差异项

- 以真实 `get-config` XML 为准。
- namespace、字段路径和端口模式都必须通过 校验器 固化。
- trunk native VLAN 和 allowed VLAN 缺失时保持 失败关闭。
