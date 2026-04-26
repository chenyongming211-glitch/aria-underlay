# Ruijie 交换机适配记录

## 当前状态

未接入真实锐捷设备。当前只能视为适配占位，生产路径必须 fail-closed。

## 需要确认的设备信息

| 字段 | 记录 |
| --- | --- |
| 设备系列 | 未验证 |
| 型号 | 未验证 |
| 系统版本 | 未验证 |
| NETCONF 支持 | 未验证 |
| CLI fallback 需求 | 未验证 |

## Capability Checklist

| 能力 | 结果 | 备注 |
| --- | --- | --- |
| NETCONF over SSH | 未验证 | |
| candidate | 未验证 | |
| validate | 未验证 | |
| confirmed-commit | 未验证 | |
| rollback-on-error | 未验证 | |
| CLI fallback | 未验证 | |

## 重点差异项

- 如果设备不支持 NETCONF，必须走 degraded CLI fallback，不得宣称强事务。
- CLI fallback 必须在修改前保存 rollback artifact。
- 无法确认最终状态时必须返回 `InDoubt`。

## 未验证红线

未接真实锐捷设备前，不允许返回生产成功状态。
