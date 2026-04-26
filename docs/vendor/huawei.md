# Huawei 交换机适配记录

## 当前状态

未接入真实华为设备。当前只能视为适配占位，生产路径必须 fail-closed。

## 需要确认的设备信息

| 字段 | 记录 |
| --- | --- |
| 设备系列 | 未验证 |
| 型号 | 未验证 |
| VRP 版本 | 未验证 |
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
| VLAN create | namespace、字段名、operation 属性 |
| VLAN update | name / description 是否支持 merge |
| VLAN delete | 删除不存在 VLAN 的返回行为 |
| access interface | 接口命名、access VLAN 字段 |
| trunk interface | allowed VLAN 表达方式、range 表达方式 |
| description | 空字符串与删除 description 的表达方式 |

## 事务行为

真实设备到位后必须验证：

- `lock(candidate)` 是否会被其他 CLI / NETCONF session 阻塞。
- `edit-config(candidate)` 是否支持 `rollback-on-error`。
- `validate(candidate)` 是否真的校验 VLAN 范围和接口存在性。
- `confirmed-commit` 是否支持 `persist-id`。
- session 断开后 pending confirmed commit 的恢复行为。

## 已知风险

- 华为不同 VRP 版本的 namespace 和字段可能不同。
- 部分设备可能 capability 声明与实际行为不一致。
- 未验证前不得把 Huawei driver 标记为生产可用。
