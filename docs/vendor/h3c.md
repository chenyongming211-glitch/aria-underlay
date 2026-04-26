# H3C 交换机适配记录

## 当前状态

未接入真实 H3C 设备。当前只能视为适配占位，生产路径必须 fail-closed。

## 需要确认的设备信息

| 字段 | 记录 |
| --- | --- |
| 设备系列 | 未验证 |
| 型号 | 未验证 |
| Comware 版本 | 未验证 |
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

## VLAN / Interface 差异项

| 对象 | 需要记录 |
| --- | --- |
| VLAN create | Comware XML model、namespace、字段名 |
| VLAN update | name / description 行为 |
| VLAN delete | 删除不存在 VLAN 的返回行为 |
| access interface | 接口命名规范、access VLAN 字段 |
| trunk interface | allowed VLAN 表达方式 |
| description | 空值、删除、特殊字符 escape |

## 事务行为

真实设备到位后必须验证：

- `candidate` 是否真实可写。
- `validate` 是否会捕获非法 VLAN / 不存在接口。
- `confirmed-commit` 是否可用，是否支持跨 session `persist-id`。
- 不支持 `candidate` 时，running fallback 的 rollback artifact 是否可用。

## 已知风险

- Comware 版本差异可能导致 XML model 不一致。
- 部分设备对 `rollback-on-error` 的支持可能只停留在 capability 声明。
- 未验证前不得把 H3C driver 标记为生产可用。
