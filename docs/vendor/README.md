# Vendor 差异记录说明

Aria Underlay 面向 ToB 私有化交付，客户现场交换机品牌和版本不可控。Vendor 文档的作用不是写营销式支持列表，而是记录真实设备行为，指导 driver fail-closed 适配。

每个厂商文档必须记录：

- 设备型号、系统版本、NETCONF capability。
- VLAN / interface 的 XML namespace 和字段差异。
- `candidate`、`validate`、`confirmed-commit`、`rollback-on-error` 的真实行为。
- 锁冲突、认证失败、XML parse error、commit failure 的原始错误摘要。
- 哪些功能已经真实验证，哪些只是 renderer skeleton。

## 当前状态

| 厂商 | 文档 | 当前结论 |
| --- | --- | --- |
| Huawei | [huawei.md](huawei.md) | 未接真实设备，必须 fail-closed |
| H3C | [h3c.md](h3c.md) | 未接真实设备，必须 fail-closed |
| Cisco | [cisco.md](cisco.md) | 未接真实设备，必须 fail-closed |
| Ruijie | [ruijie.md](ruijie.md) | 未接真实设备，必须 fail-closed |

## 记录红线

- 未经真实设备验证的字段必须标记为 `未验证`。
- 不允许根据文档或样例 XML 推断“已支持”。
- vendor driver 未实现时必须返回 `NOT_IMPLEMENTED` / `UNIMPLEMENTED_DRIVER`，不能继承 fake 行为。
- 任何 CLI fallback 都必须标记为 degraded，并返回 warning。
