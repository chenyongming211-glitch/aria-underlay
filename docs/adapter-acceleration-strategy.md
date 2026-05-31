# 适配加速战略

> 解决多厂商 × 多功能 × 多固件的 O(V × F × S) 适配工作量问题。
> 本文档定义三个互补的技术方向，不替代现有的事务安全和验收标准。

## 1. 问题定义

当前适配模型：

```
标准 intent → vendor renderer（手写 XML/CLI）→ 设备
设备 running XML → vendor parser（手写解析）→ 标准 observed state
```

每接入一个新厂商，需要写：

| 交付物 | 行数 | 说明 |
|--------|-----:|------|
| Renderer | ~400 | VLAN + interface + ACL + binding |
| State Parser | ~400 | 反向解析 |
| XML Fixtures | ~600 | ~30 个文件，正负样本 |
| Acceptance Scenario | ~100 | offline acceptance |
| 测试 | ~200 | renderer/parser/verify |
| **总计** | **~1700** | 每个新厂商 |

每新增一个功能（PBR、BGP、QoS），又要在每个厂商上重复一遍。加上固件差异，工作量是 O(V × F × S)。

**本文档的三个方向**：

1. **YANG 驱动**：从 YANG schema 自动生成 renderer/parser，减少手写
2. **LLM 辅助**：用 LLM 加速新厂商的初始代码生成，人工只修失败 case
3. **Runtime Discovery**：运行时用 YANG schema 验证配置，作为安全网

三个方向独立可落地，但共享 YANG library 数据基础。

---

## 2. YANG 驱动 Renderer/Parser 自动生成

### 2.1 核心原理

YANG（RFC 7950）是 IETF 标准的网络设备配置 schema 语言。每个 YANG module 定义了数据结构、类型约束、必选/可选、引用关系。NETCONF XML 严格按 YANG schema 组织。

**核心洞察**：如果设备 YANG 实现足够规范，可以从 schema 自动生成 renderer/parser，不需要手写 XML 模板。

举例，H3C Comware7 的 VLAN YANG（简化）：

```yang
module h3c-vlan {
  namespace "http://www.h3c.com/h3c_config:3.1.0:h3c-vlan";
  prefix h3c-vlan;

  container VLANs {
    list VLAN {
      key "VLANID";
      leaf VLANID {
        type uint16 { range "1..4094"; }
      }
      leaf Description {
        type string { length "1..255"; }
      }
      leaf Name {
        type string { length "1..31"; }
      }
    }
  }
}
```

从这个 schema 可以自动生成等价的 renderer：

```python
# AUTO-GENERATED from YANG schema: h3c-vlan@2024-01-15
def render_vlan(vlan: dict, module_schema: YangModule) -> XmlElement:
    ns = module_schema.namespace  # "http://www.h3c.com/h3c_config:3.1.0:h3c-vlan"
    children = [
        XmlElement("VLANID", namespace=ns, text=str(vlan["vlan_id"]))
    ]
    if vlan.get("description"):
        children.append(XmlElement("Description", namespace=ns, text=vlan["description"]))
    if vlan.get("name"):
        children.append(XmlElement("Name", namespace=ns, text=vlan["name"]))
    return XmlElement("VLAN", namespace=ns, children=children)
```

和自动生成的 parser：

```python
# AUTO-GENERATED from YANG schema: h3c-vlan@2024-01-15
def parse_vlan(vlan_node: Element, module_schema: YangModule) -> dict:
    ns = module_schema.namespace
    return {
        "vlan_id": int(_text(vlan_node, "VLANID", ns)),
        "description": _optional_text(vlan_node, "Description", ns),
        "name": _optional_text(vlan_node, "Name", ns),
    }
```

### 2.2 当前现实约束

**为什么不能直接跳到 YANG 驱动**：

1. **H3C Comware7 YANG 实现问题**：
   - 很多功能没有 YANG module，只有私有 XML namespace
   - 已有 YANG module 经常和实际设备行为不一致（schema 说有这个 leaf，设备上写入报 rpc-error）
   - Namespace 在不同固件版本间变化
   - `deviation` 声明不完整，schema 和实现差异未文档化

2. **Huawei VRP8 类似问题**：
   - YANG 覆盖面比 H3C 好，但仍有大量私有扩展
   - 部分 YANG path 和实际 edit-config path 不一致

3. **OpenConfig 在国内厂商上基本不可用**：
   - H3C、Huawei、Ruijie 没有原生 OpenConfig 支持
   - OpenConfig-over-NETCONF 需要设备支持 OpenConfig YANG modules

### 2.3 渐进落地方案

#### 阶段 A：YANG Schema 采集和归档

**目标**：把所有接入设备的 YANG modules 采集并归档，建立 YANG library。

```
接入新设备
  → NETCONF get-schema（RFC 6022）列出所有可用 YANG modules
  → 逐个下载 module 文本
  → 存入 data/yang-library/{vendor}/{model}/{os_version}/
  → 记录 module name + revision + namespace
  → 写入 DeviceModelProfile.yang_modules
```

**采集脚本**（复用现有 NETCONF 探测能力）：

```python
# adapter-python/aria_underlay_adapter/yang_collector.py

class YangCollector:
    """只读采集设备 YANG library，不修改任何配置。"""
    
    def collect(self, device_config: dict) -> YangLibraryBundle:
        with self._connect(device_config) as session:
            # 1. 从 NETCONF hello 提取 module hints
            hello_modules = extract_yang_modules_from_capabilities(
                [str(cap) for cap in session.server_capabilities]
            )
            
            # 2. 通过 get-schema 下载完整 module 文本
            modules = {}
            for module_name, revision in hello_modules.items():
                try:
                    module_text = session.get_schema(module_name, revision)
                    modules[module_name] = YangModule(
                        name=module_name,
                        revision=revision,
                        text=module_text,
                    )
                except RpcError:
                    # 部分设备不支持 get-schema，记录为 unavailable
                    modules[module_name] = YangModule.unavailable(module_name, revision)
            
            return YangLibraryBundle(
                vendor=device_config["vendor"],
                model=device_config["model"],
                os_version=device_config["os_version"],
                modules=modules,
            )
    
    def save(self, bundle: YangLibraryBundle, output_dir: Path) -> None:
        vendor_dir = output_dir / bundle.vendor / bundle.model / bundle.os_version
        vendor_dir.mkdir(parents=True, exist_ok=True)
        for module in bundle.modules.values():
            if module.available:
                (vendor_dir / f"{module.name}@{module.revision}.yang").write_text(module.text)
        # 写入结构化摘要
        (vendor_dir / "summary.json").write_text(bundle.to_json())
```

**安全约束**：
- 只做只读操作（`get-schema`、`get-config(source="running", filter=...yang-library...)`）
- 不影响设备配置
- 采集结果作为 evidence 存入 `DeviceModelProfile`

**与现有架构集成**：
- `netconf_model_profile.py` 已有 `extract_yang_modules_from_capabilities`，扩展为完整 module 下载
- `DeviceModelProfile` proto 增加 `repeated YangModuleSummary yang_modules` 字段
- 离线 acceptance report 新增 `yang_library` 字段

**工作量**：2-3 天

**产出**：每台设备的完整 YANG library 文本 + 结构化摘要。

#### 阶段 B：YANG Schema Diff 和 Deviation 发现

**目标**：对比设备实际行为和 YANG schema，自动发现 deviation。

```
对每个 YANG path（目标功能面）：
  → 生成无害的测试配置（隔离 VLAN、隔离 ACL）
  → 尝试 edit-config(target="candidate")
  → 成功：path 标记为 schema-conformant
  → 失败：记录 rpc-error，标记为 deviated
  → 对比 schema 声称的 namespace/类型 和实际 XML
  → 结果写入 DeviceModelProfile.yang_conformance
```

**代码结构**：

```python
# adapter-python/aria_underlay_adapter/yang_probe.py

class YangPathProbe:
    """从 YANG leaf/container 定义自动生成无害的写探测。"""
    
    SAFE_VLAN_ID = 4090      # 不在生产使用的隔离 VLAN
    SAFE_ACL_ID = 3999       # 不在生产使用的隔离 ACL
    
    def generate_probe_config(self, path: str, schema: YangNode) -> Optional[XmlElement]:
        """根据 YANG 类型生成无害值。"""
        match schema.yang_type:
            case YangType.Uint16:
                value = str(schema.default or self._safe_uint16(path))
            case YangType.String:
                value = f"aria-probe-{uuid4().hex[:8]}"
            case YangType.Enumeration:
                value = schema.allowed_values[0] if schema.allowed_values else None
            case YangType.Boolean:
                value = "false"
            case _:
                return None  # 无法自动生成
        
        return XmlElement(schema.name, namespace=schema.namespace, text=value)
    
    def probe_path(self, path: str, module: YangModule) -> YangProbeResult:
        """在 candidate 上尝试写入，验证 schema 一致性。"""
        probe_config = self.generate_probe_config(path, module.get_node(path))
        if probe_config is None:
            return YangProbeResult(path=path, status="skipped", reason="cannot generate probe")
        
        try:
            # 1. lock candidate
            self.session.lock(target="candidate")
            # 2. edit-config with probe config
            self.session.edit_config(target="candidate", config=probe_config)
            # 3. validate
            self.session.validate(source="candidate")
            # 4. discard-changes（不实际提交）
            self.session.discard_changes()
            # 5. unlock candidate
            self.session.unlock(target="candidate")
            
            return YangProbeResult(path=path, status="conformant")
        except RpcError as exc:
            self.session.discard_changes()
            self.session.unlock(target="candidate")
            return YangProbeResult(
                path=path,
                status="deviated",
                rpc_error=exc.error_tag,
                raw_error=exc.raw_error_summary,
            )
```

**安全约束**：
- 只在 candidate datastore 上操作，validate 后立即 discard-changes
- 使用隔离的测试对象（VLAN ID 4090、ACL ID 3999 等不在生产使用的范围）
- 每个 probe 必须有 cleanup
- 默认 dry-run only；实际探测需要显式开启 `--probe-mode=active`
- 探测结果进入 `DeviceModelProfile.yang_conformance`，不直接进入写路径

**工作量**：1-2 周（需要真实设备）

**产出**：每台设备的 YANG conformance report — 哪些 path 符合 schema、哪些 deviated、deviation 的具体形式。

#### 阶段 C：Schema-Driven Renderer 生成

**前提**：阶段 B 已产出 YANG conformance report，已知哪些 path 是 schema-conformant 的。

**目标**：对 schema-conformant paths，自动生成 renderer/parser。

```python
# adapter-python/aria_underlay_adapter/yang_codegen.py

class YangRendererGenerator:
    """从 YANG module 生成 renderer/parser 代码。"""
    
    def generate(self, module: YangModule, conformance: YangConformanceReport) -> GeneratedCode:
        renderer_code = []
        parser_code = []
        
        for path in module.paths:
            if not conformance.is_conformant(path):
                # deviated path → 跳过，留给手写
                continue
            
            if path.is_leaf:
                renderer_code.append(self._gen_leaf_renderer(path))
                parser_code.append(self._gen_leaf_parser(path))
            elif path.is_container:
                renderer_code.append(self._gen_container_renderer(path))
                parser_code.append(self._gen_container_parser(path))
            elif path.is_list:
                renderer_code.append(self._gen_list_renderer(path))
                parser_code.append(self._gen_list_parser(path))
        
        return GeneratedCode(
            renderer=renderer_code,
            parser=parser_code,
            acceptance_scenario=self._gen_acceptance_scenario(module),
            metadata={
                "source": "yang-driven",
                "module": module.name,
                "revision": module.revision,
                "conformant_paths": len([p for p in module.paths if conformance.is_conformant(p)]),
                "total_paths": len(module.paths),
            },
        )
```

**代码目录结构**：

```
renderers/
  ├── generated/          # YANG-driven 自动生成
  │   ├── h3c/
  │   │   ├── vlan.py
  │   │   └── interface.py
  │   └── huawei/
  ├── handwritten/        # 现有的手写 renderer（从 renderers/h3c.py 迁入）
  │   ├── h3c.py
  │   └── huawei.py
  ├── overrides/          # 对生成代码的人工 override
  │   └── h3c/
  └── registry.py         # 优先级：override > generated > handwritten
```

**安全约束**：
- 生成的代码必须通过 offline acceptance 才能进入生产
- 每个生成的 renderer/parser 都有对应的 acceptance scenario
- Deviated paths 永远不自动生成，必须手写 + 单独测试
- 生成代码可以被人手 override：`overrides/h3c/vlan.py` 优先于 `generated/h3c/vlan.py`
- 生成代码头部必须包含 metadata：

```python
# AUTO-GENERATED from YANG schema
# Module: h3c-vlan@2024-01-15
# Conformant paths: 12/15
# Generated: 2026-06-01T10:00:00Z
# Status: VERIFIED — passed offline acceptance on 2026-06-01
# Override: place a file in renderers/overrides/h3c/ to replace
```

**工作量**：1-2 个月（需要阶段 B 的数据积累）

### 2.4 风险和缓解

| 风险 | 影响 | 缓解 |
|------|------|------|
| YANG schema 和设备行为不一致 | 生成的 renderer 下发失败 | 阶段 B 的 conformance report 前置检查 |
| 固件升级后 YANG 变化 | 旧 renderer 不兼容 | 每次 firmware 变化重跑 conformance probe |
| 自动生成代码质量不可控 | 生产事故 | 必须过 offline acceptance + 人工 review |
| 覆盖的功能面有限 | 只能覆盖 schema-conformant paths | deviated paths 仍走手写，不降级 |

### 2.5 不做

- 不为没有 YANG module 的功能自动生成 renderer
- 不在 YANG conformance 未验证的情况下自动生成
- 不因为代码是自动生成的就跳过 offline acceptance
- 不替代手写 renderer 处理 deviated paths

---

## 3. LLM 辅助适配

### 3.1 核心原理

你已有 H3C 作为 golden reference：1193 行 renderers、1188 行 state_parsers、31 个 XML fixtures、5 个 offline acceptance scenarios。不同厂商的 renderer 结构高度相似，只是 namespace 和 element name 不同。LLM 擅长这种 pattern matching + 转换。

**关键洞察**：新厂商接入时，LLM 可以对照 H3C reference + 新厂商 running-config 样本，生成初始 renderer/parser 骨架。人工只需要修 offline acceptance 中失败的 case，不需要从零写 1000+ 行代码。

### 3.2 工作流设计

```
Phase 1: 采集（只读）
  │  设备 running-config 样本
  │  + NETCONF capabilities
  │  + YANG modules
  │
Phase 2: LLM 生成（离线）
  │  - renderer 骨架
  │  - state parser 骨架
  │  - 初始 fixtures
  │  - acceptance scenario 骨架
  │
Phase 3: 自动验证（CI）
  │  - offline acceptance runner
  │  - renderer tests
  │  - parser tests
  │  - verify tests
  │
Phase 4: 人工修复（聚焦）
  │  - 只修失败的 test case
  │  - review 生成的代码
  │  - 补充 edge case
  │
Phase 5: 真实设备验收
     - 按 real-device acceptance runbook 执行
     - 通过后合入 main
```

### 3.3 Phase 1：采集（只读，零风险）

**输入要求**：

```json
{
  "vendor": "ruijie",
  "model": "RG-S6510",
  "os_version": "RGOS 12.5",
  "artifacts": {
    "running_config_xml": "samples/ruijie/running-config.xml",
    "scoped_configs": {
      "vlan": "samples/ruijie/running-vlan.xml",
      "interface": "samples/ruijie/running-interface.xml",
      "acl": "samples/ruijie/running-acl.xml"
    },
    "netconf_hello": "samples/ruijie/hello.xml",
    "yang_modules": "samples/ruijie/yang/"
  }
}
```

**安全约束**：
- 只做 `get-config(source="running")` 和 `get-schema`，不写设备
- 样本文件不入库（可能包含敏感 IP/hostname），存入 `.gitignore` 的 `samples/` 目录
- 脱敏脚本在入库前自动替换 IP、hostname、password hash

### 3.4 Phase 2：LLM 生成（离线，零风险）

**Prompt 模板**：

```
你是一个网络设备配置适配器生成器。

## 参考：H3C Comware7 VLAN Renderer
{h3c_vlan_renderer_code}

## 参考：H3C Comware7 VLAN Parser
{h3c_vlan_parser_code}

## 参考：H3C Running Config VLAN Sample
{h3c_running_vlan_xml}

## 目标设备：{vendor} {os_version} VLAN Running Config Sample
{target_running_vlan_xml}

## NETCONF Capabilities
{target_capabilities}

## YANG Module (if available)
{target_vlan_yang}

## 要求
1. 生成 {vendor} VLAN renderer，结构对标 H3C renderer，但使用 {vendor} 的
   namespace 和 element name（从 running config 样本推断）
2. 生成 {vendor} VLAN state parser，对标 H3C parser
3. 生成 3 个 XML fixture（create vlan、access port、trunk port）
4. 如果 YANG module 可用，优先使用 YANG 定义的 namespace 和 element name
5. 如果 running config 样本和 YANG module 有冲突，以 running config 为准，
   并在注释中标注差异
6. 生成的代码必须使用 dataclass 和类型标注
```

**迭代生成**：

```python
def iterative_generate_and_verify(
    vendor_sample: VendorSampleBundle,
    reference_code: H3CReferenceCode,
    max_iterations: int = 3,
) -> GeneratedCodeBundle:
    
    generated = None
    report = None
    
    for iteration in range(max_iterations):
        if iteration == 0:
            generated = llm_generate(vendor_sample, reference_code)
        else:
            generated = llm_regenerate(
                vendor_sample, reference_code,
                previous_code=generated,
                failures=report.failures,
            )
        
        report = run_vendor_acceptance(vendor_sample.vendor, generated.path)
        
        if report.failed == 0:
            return GeneratedCodeBundle(code=generated, report=report, status="verified")
        
        print(f"Iteration {iteration + 1}: {report.passed} passed, {report.failed} failed")
    
    return GeneratedCodeBundle(
        code=generated,
        report=report,
        status="needs_human_review",
        failed_scenarios=[s for s in report.scenarios if s["status"] == "failed"],
    )
```

### 3.5 Phase 3：自动验证（CI，零风险）

你已有的 offline acceptance runner 天然就是 LLM 生成代码的质量门禁。

**验证标准**：

| 验证项 | 通过条件 | 失败处理 |
|--------|----------|----------|
| Renderer 输出合法 XML | XML parser 不报错 | 生成代码有语法/结构错误 |
| Mock NETCONF 不报错 | dry-run/prepare/commit 全过 | XML 结构不符合设备期望 |
| Readback XML 生成 | 模拟设备返回的 running config | renderer 的 XML 结构有误 |
| Parser 正确解析 readback | parsed state 结构完整 | parser 逻辑错误 |
| Parsed vs observed 一致 | 结构和值匹配 | renderer 和 parser 不对称 |
| ChangePlan 输出 | stages/dependency/blast_radius | ChangePlan 集成缺失 |

**最多 3 轮迭代**。3 轮后仍有失败，标记为需要人工介入。

### 3.6 Phase 4：人工修复（聚焦）

**关键改变**：人工不再是"从零写 1000 行"，而是"修 LLM 生成的代码中的 3-5 个失败 case"。

| 失败类型 | LLM 常见错误 | 人工修复 |
|----------|-------------|----------|
| Namespace 错误 | 用了错误的 YANG namespace | 查 running-config 样本，修正 |
| Element name 错误 | 猜了错误的 XML 元素名 | 查设备文档或样本，修正 |
| 类型转换错误 | 把 VLAN ID 当 string 而不是 int | 修正类型 |
| 缺少必填字段 | 遗漏了厂商必填的 leaf | 查 YANG schema，补充 |
| Parser 正则错误 | XML 解析用了不正确的 XPath | 修正 XPath |

每个修复都有 acceptance test 兜底——修完立即跑，确认修复有效且不引入回归。

### 3.7 Phase 5：真实设备验收

和现有的 `real-device-acceptance.md` runbook 完全一致。LLM 生成不降低验收标准。

### 3.8 与现有架构集成

```python
# adapter-python/aria_underlay_adapter/renderers/registry.py

class RendererRegistry:
    """优先级：override > generated > handwritten"""
    
    def get_renderer(self, vendor: str) -> BaseRenderer:
        if vendor in self._overrides:
            return self._overrides[vendor]
        if vendor in self._generated and self._generated[vendor].verified:
            return self._generated[vendor].renderer
        if vendor in self._handwritten:
            return self._handwritten[vendor]
        raise UnsupportedVendorError(vendor)
```

### 3.9 ROI 估算

| 阶段 | 手写（当前） | LLM 辅助 | 节省 |
|------|-----:|------:|-----:|
| 采集样本 | — | 1 天 | — |
| 初始代码 | 2 周 | 2 天 | ~80% |
| 测试编写 | 3 天 | 1 天 | ~65% |
| 人工修复 | — | 2-3 天 | — |
| 真机验收 | 3 天 | 3 天 | 0% |
| **总计** | **~4 周** | **~1.5 周** | **~60%** |

后续每新增一个厂商，边际成本更低（LLM 有更多的 reference）。

### 3.10 风险和缓解

| 风险 | 影响 | 缓解 |
|------|------|------|
| LLM 生成代码质量不稳定 | acceptance 反复失败 | 最多 3 轮迭代，超出则人工介入 |
| LLM 幻觉出不存在的 YANG namespace | 设备 rpc-error | offline acceptance 的 mock NETCONF 会捕获 |
| LLM 生成代码有安全漏洞 | 注入/信息泄露 | 人工 review + 静态分析 |
| 过度依赖 LLM | 团队不理解 renderer 逻辑 | 生成代码必须有人 review 和理解 |

### 3.11 不做

- 不让 LLM 直接修改生产代码
- 不让 LLM 生成的代码跳过 offline acceptance
- 不让 LLM 决定写路径准入（仍由 `DeviceModelProfile` + `WriteDecision` 决定）
- 不用 LLM 生成事务/恢复/审计相关的代码

---

## 4. Runtime Discovery（运行时自适应）

### 4.1 完整的 Runtime Discovery（远期方向）

**目标**：不在代码里硬编码任何厂商逻辑，运行时通过 YANG schema + 探测 + 反馈循环自动适配。

```
新设备接入
  → 下载完整 YANG library
  → 解析 schema，建立 path → type 映射
  → 根据标准 intent 模型，自动生成 YANG path 映射
  → 构造 edit-config XML
  → 在 candidate 上验证
  → 成功 → 缓存映射，后续直接使用
  → 失败 → 标记为 unsupported，不写入
```

**为什么现在不可行**：

1. YANG 实现碎片化：H3C Comware7 的 YANG 和实际行为差异太大
2. Intent → YANG path 映射没有标准：OpenConfig 定义了标准 path，但国内厂商不用
3. 安全性无法保证：runtime 自动生成的配置如果出错，可能影响生产网络
4. 性能问题：每次新设备都要做大量探测，冷启动时间长

**结论**：3-5 年方向，当前不做。但可以做其安全子集。

### 4.2 可行的子集：Runtime YANG Validation

**目标**：在 renderer 输出发送到设备之前，用 YANG schema 验证 XML 结构。

这是 runtime discovery 的安全子集——不做自动生成，只做运行时校验。

```python
# adapter-python/aria_underlay_adapter/yang_validator.py

class RuntimeYangValidator:
    """在 renderer 输出发送到设备之前，用 YANG schema 验证。"""
    
    def __init__(self, yang_library: YangLibrary):
        self.schemas = yang_library.load_schemas()
        self._cache: dict[str, YangModule] = {}
    
    def validate_edit_config(
        self, xml: XmlElement, target_module: str
    ) -> YangValidationResult:
        """验证 edit-config XML 是否符合 YANG schema。"""
        schema = self._get_schema(target_module)
        if schema is None:
            return YangValidationResult(
                valid=False,
                error=f"unknown YANG module: {target_module}",
                mode="fail_closed",
            )
        
        errors = []
        self._validate_node(xml, schema.root, errors)
        
        return YangValidationResult(
            valid=len(errors) == 0,
            errors=errors,
            module=target_module,
            revision=schema.revision,
        )
    
    def _validate_node(self, xml: XmlElement, schema_node: YangNode, errors: list):
        # 1. Namespace 匹配
        if xml.namespace != schema_node.namespace:
            errors.append(YangValidationError(
                kind="namespace_mismatch",
                path=xml.path,
                expected=schema_node.namespace,
                actual=xml.namespace,
            ))
        
        # 2. 必填 leaf 检查
        for child in schema_node.mandatory_children:
            if not xml.has_child(child.name, child.namespace):
                errors.append(YangValidationError(
                    kind="missing_mandatory",
                    path=f"{xml.path}/{child.name}",
                    expected=child.name,
                ))
        
        # 3. 类型约束检查
        for leaf in xml.children:
            schema_leaf = schema_node.get_child(leaf.name)
            if schema_leaf is None:
                errors.append(YangValidationError(
                    kind="unknown_element",
                    path=f"{xml.path}/{leaf.name}",
                ))
                continue
            
            self._validate_type(leaf, schema_leaf, errors)
        
        # 4. 递归验证子节点
        for child in xml.children:
            child_schema = schema_node.get_child(child.name)
            if child_schema:
                self._validate_node(child, child_schema, errors)
    
    def _validate_type(self, leaf: XmlElement, schema: YangLeaf, errors: list):
        match schema.yang_type:
            case YangType.Uint16:
                try:
                    value = int(leaf.text)
                    if schema.range and not schema.range.contains(value):
                        errors.append(YangValidationError(
                            kind="out_of_range",
                            path=leaf.path,
                            expected=str(schema.range),
                            actual=str(value),
                        ))
                except ValueError:
                    errors.append(YangValidationError(
                        kind="type_mismatch",
                        path=leaf.path,
                        expected="uint16",
                        actual=leaf.text,
                    ))
            case YangType.String:
                if schema.length and not schema.length.contains(len(leaf.text)):
                    errors.append(YangValidationError(
                        kind="length_violation",
                        path=leaf.path,
                        expected=str(schema.length),
                        actual=str(len(leaf.text)),
                    ))
                if schema.pattern and not schema.pattern.match(leaf.text):
                    errors.append(YangValidationError(
                        kind="pattern_violation",
                        path=leaf.path,
                        expected=str(schema.pattern),
                    ))
            case YangType.Enumeration:
                if leaf.text not in schema.allowed_values:
                    errors.append(YangValidationError(
                        kind="invalid_enum",
                        path=leaf.path,
                        expected=str(schema.allowed_values),
                        actual=leaf.text,
                    ))
```

**与现有架构集成**：

```python
# adapter-python/aria_underlay_adapter/drivers/netconf_backed.py

class NetconfBackedDriver:
    async def prepare(self, tx_id, device, desired_state):
        renderer = self.renderer_registry.get_renderer(device.vendor)
        config_xml = renderer.render(desired_state)
        
        # Runtime YANG validation（新增）
        if self.yang_validator:
            validation = self.yang_validator.validate_edit_config(
                config_xml, target_module=renderer.primary_module
            )
            if not validation.valid:
                return PrepareResponse(
                    status="failed",
                    error=AdapterError(
                        code="YANG_VALIDATION_FAILED",
                        message=f"renderer output does not conform to YANG schema",
                        details=validation.errors,
                    ),
                )
        
        # 原有的 edit-config 流程
        return await self._edit_config(device, config_xml)
```

**安全约束**：
- 验证失败 → fail-closed，不发送 edit-config
- YANG schema 加载失败 → fail-closed，降级为不验证 + 结构化 warning
- 验证本身不做任何写操作
- 验证结果进入 journal 和审计日志
- 验证可以通过配置关闭（`yang_runtime_validation: false`），但默认开启

**工作量**：1 周

**产出**：renderer 输出的运行时安全网，在 edit-config 发送到设备之前捕获 XML 结构错误。

### 4.3 Intent-to-YANG 自动映射（远期方向）

如果 YANG 实现的规范化趋势继续，3-5 年后可能出现：

```
标准 intent（VLAN、ACL、BGP）
  → 标准 YANG path（OpenConfig 或 IETF）
  → 设备 YANG deviation mapping
  → 自动翻译成设备 YANG path
  → 自动生成 edit-config XML
  → runtime schema validation
  → 发送到设备
```

这个架构下，**renderer 不再是代码，而是数据**（YANG deviation mapping 表）。新增厂商 = 提供 deviation mapping，不是写 renderer。

**前提条件**：
- 阶段 A/B 积累了足够的 YANG library 和 conformance 数据
- 国内厂商 YANG 实现规范化程度显著提高
- 有稳定的 OpenConfig → vendor YANG deviation mapping 标准

**当前不做**。但阶段 A/B 的数据积累是它的必要前置。

### 4.4 风险和缓解

| 风险 | 影响 | 缓解 |
|------|------|------|
| YANG schema 加载失败 | 验证不可用 | fail-closed 降级为不验证 + warning |
| 验证性能开销 | prepare 延迟增加 | schema 缓存 + 异步验证 |
| Schema 和实际行为不一致 | 误报/漏报 | 阶段 B 的 conformance report 修正 schema |
| 过度依赖验证 | 放松 renderer 测试 | 验证是补充，不替代测试 |

### 4.5 不做

- 不做完整的 runtime discovery（自动生成配置）
- 不用 runtime validation 替代 offline acceptance
- 不在 YANG schema 不可用时强行验证
- 不让 runtime validation 的结果影响 `WriteDecision`（写路径准入仍由 `DeviceModelProfile` 决定）

---

## 5. 三个方案的关系

```
                    ┌──────────────────────────────────────────────────┐
                    │              共享数据基础                          │
                    │         YANG Library 采集（阶段 A）               │
                    └──────┬───────────────┬──────────────┬───────────┘
                           │               │              │
              ┌────────────▼───┐  ┌────────▼────────┐  ┌─▼──────────────────┐
              │  YANG 驱动     │  │  LLM 辅助       │  │  Runtime Discovery │
              │  (中长期)      │  │  (短期)         │  │  (中期子集)        │
              └────────┬───────┘  └────────┬────────┘  └─┬──────────────────┘
                       │                   │              │
              ┌────────▼───────────────────▼──────────────▼──────────┐
              │                                                      │
              │          统一的 Offline Acceptance Runner             │
              │          统一的 Real-Device Acceptance Runbook        │
              │          统一的 DeviceModelProfile + WriteDecision    │
              │                                                      │
              └──────────────────────────────────────────────────────┘
```

**互补关系**：

| 方案 | 解决什么 | 不解决什么 |
|------|----------|-----------|
| YANG 驱动 | 减少 schema-conformant paths 的手写工作 | deviated paths 仍需手写 |
| LLM 辅助 | 加速新厂商冷启动 | 不减少长期维护成本 |
| Runtime Discovery | 运行时安全网 | 不生成 renderer |

**数据流**：

```
YANG 采集 → YANG library → DeviceModelProfile.yang_modules
                              │
YANG Diff → conformance report → DeviceModelProfile.yang_conformance
                                    │
                                    ├──→ YANG 驱动（conformant paths 自动生成）
                                    ├──→ LLM 辅助（conformance 作为 prompt context）
                                    └──→ Runtime Validator（运行时 schema 校验）
```

---

## 6. 安全红线（三个方案共同遵守）

无论走哪个方向，这些红线不能破：

1. **生成的代码不过 acceptance 就不进生产** — 无论代码来源是手写、LLM 还是 YANG 自动生成，必须过同一套 offline acceptance runner
2. **Path-level 证据是写路径的硬门槛** — 不因代码是自动生成的就跳过 `DeviceModelProfile` 验证
3. **ACID 事务语义不降级** — renderer 来源不影响 confirmed-commit / rollback / InDoubt 处理
4. **真实设备验收不可省略** — 离线 acceptance 通过只意味着代码结构正确，不意味着设备行为正确
5. **审计日志记录代码来源** — journal 和 telemetry 中记录 renderer 是手写、LLM 生成还是 YANG 自动生成，便于排障
6. **生成代码可被人手 override** — `renderers/overrides/` 优先级高于 `renderers/generated/`
7. **YANG schema 不可用时 fail-closed** — 不降级为"假设 schema 正确"

这些红线和现有开发计划的原则完全一致（见 `docs/aria-underlay-development-plan.md` §3 核心原则）。三个新方案只是改变了 renderer/parser 的生产方式，不改变事务安全和验收标准。

---

## 7. 推荐执行顺序

| 序号 | 事项 | 前置条件 | 工作量 | 风险 | 价值 |
|------|------|----------|--------|------|------|
| 1 | **LLM 辅助适配 MVP** | 现有 H3C reference + 一个新厂商样本 | 1-2 周 | 低 | 高：新厂商接入时间减半 |
| 2 | **YANG Schema 采集** | NETCONF get-schema 支持 | 2-3 天 | 极低 | 中：数据基础 |
| 3 | **Runtime YANG Validator** | YANG library 采集完成 | 1 周 | 低 | 中：运行时安全网 |
| 4 | **YANG Schema Diff** | 真实设备 + YANG library | 1-2 周 | 中 | 高：自动化基础 |
| 5 | **Schema-Driven Renderer** | Schema diff 数据积累 | 1-2 月 | 中 | 高：长期减少手写 |

**第一件事应该做 LLM 辅助适配**，因为：
- 投入产出比最高（直接减少新厂商接入时间）
- 风险最低（生成的代码必须过 acceptance）
- 不依赖真实设备（可以用采集的样本离线做）
- 和现有架构完全兼容（只是在 `renderers/` 下加一个 `generated/` 层级）

**YANG 采集应该同步启动**，因为：
- 只读操作，零风险
- 是三个方案的共同数据基础
- 工作量极小（2-3 天）
- 即使后续不走 YANG 驱动方向，采集数据也有独立价值（设备能力归档）
