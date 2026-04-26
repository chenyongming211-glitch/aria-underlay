# Cisco 交换机适配记录

## 当前状态

未接入真实 Cisco 设备。当前只能视为适配占位，生产路径必须 fail-closed。

## 需要确认的设备信息

| 字段 | 记录 |
| --- | --- |
| 设备系列 | 未验证 |
| 型号 | 未验证 |
| IOS XE / NX-OS 版本 | 未验证 |
| NETCONF 端口 | 未验证 |
| YANG / XML 模型版本 | 未验证 |

## Capability Checklist

| 能力 | 结果 | 备注 |
| --- | --- | --- |
| base:1.0 | 未验证 | |
| base:1.1 | 未验证 | |
| candidate | 未验证 | |
| validate:1.0 / 1.1 | 未验证 | |
| confirmed-commit:1.0 | 未验证 | |
| confirmed-commit:1.1 + persist-id | 未验证 | |
| rollback-on-error | 未验证 | |
| writable-running | 未验证 | |

## 重点差异项

- IOS XE 与 NX-OS 的 YANG 模型不可混用。
- VLAN、switchport mode、trunk allowed VLAN 的 XML 路径需要分别验证。
- CLI fallback 如果启用，必须作为 degraded 策略记录 warning。

## 未验证红线

未接真实 Cisco 设备前，不允许返回生产成功状态。
