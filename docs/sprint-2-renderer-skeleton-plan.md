# Sprint 2: Vendor Renderer Skeleton 计划

## 1. 目标

Sprint 2 的目标是建立厂商 XML 渲染边界，不直接进入真实设备下发。

当前 Aria Underlay 的架构已经确定为：

```text
Rust Controller / AriaUnderlayService
  -> gRPC
  -> Python Adapter
  -> Vendor Renderer
  -> NETCONF Backend
```

因此厂商适配和 XML 渲染放在 Python Adapter 侧，而不是 Rust 主控侧。

## 2. 当前已落地

已新增 renderer skeleton：

```text
adapter-python/aria_underlay_adapter/renderers/
  __init__.py
  base.py
  xml.py
  huawei.py
  h3c.py
```

当前能力：

- `XmlElement` 结构化 XML AST。
- `render_xml()` 使用 `xml.etree.ElementTree` 输出 XML，自动处理 escape。
- `VendorRenderer` Protocol 定义厂商渲染边界。
- `HuaweiRenderer` / `H3cRenderer` 提供 VLAN/interface skeleton。
- `HuaweiRenderer.production_ready = False`。
- `H3cRenderer.production_ready = False`。
- 真实 NETCONF backend 会拒绝 `production_ready = False` 的 renderer，返回 `NETCONF_RENDERER_NOT_PRODUCTION_READY`，防止 skeleton XML 被下发到设备。
- 单元测试覆盖：
  - XML escape。
  - VLAN create。
  - VLAN delete。
  - access interface update。
  - skeleton renderer 不是生产可用 renderer。
  - 真实 NETCONF prepare 拒绝 skeleton renderer。
  - unknown port mode 拒绝。

## 3. 明确限制

当前 renderer 还是 skeleton。

```text
urn:aria:underlay:renderer:*:skeleton
```

这些 namespace 不是最终厂商 YANG namespace，不能用于真实设备下发。

当前 renderer 的作用是：

- 固定结构化渲染接口。
- 禁止重新退回字符串模板拼接。
- 为后续 Huawei / H3C 真实 YANG 字段映射提供测试框架。

## 4. 下一步

Sprint 2 后续任务：

1. 定义 adapter 内部 change set 输入结构。
2. 把 Rust `ChangeSet` 映射到 Python Adapter 的渲染输入。
3. 查询 Huawei CE / H3C Comware 的真实 VLAN 与接口 YANG namespace。
4. 将 skeleton namespace 替换为真实 namespace。
5. 增加 snapshot tests。
6. 为真实 renderer 显式设置 `production_ready = True`，且必须有真实设备验证记录。
7. 等真实设备确认后，再进入 candidate edit/validate。

## 5. 不做事项

Sprint 2 仍然不做：

- 真实 `edit-config`。
- 真实 `commit`。
- confirmed commit。
- rollback。
- CLI fallback 下发。

这些进入 Sprint 3 / Sprint 4。
