# NETCONF 状态样本采集操作手册

## 目标

当真实 Huawei/H3C 交换机可用时，采集 `get-config` running XML，用于验证 状态解析器。没有真实设备时不要把 样本解析器 标记为 生产就绪。

## 采集前检查

- 明确设备厂商、型号、软件版本。
- 确认只采集配置状态，不执行 edit-config。
- 准备脱敏输出目录。
- 记录采集命令、时间、设备角色和配置范围。

## 采集流程

1. 使用只读 NETCONF 账号连接设备。
2. 执行 scoped `get-config`，优先采集 VLAN 和 接口 相关 subtree。
3. 保存原始 XML 到受控目录。
4. 脱敏 IP、用户名、描述中的敏感信息。
5. 用 `aria-underlay-state-parse` 离线校验。
6. 将失败样本裁剪成最小 样本，并补充测试。

## 成功标准

- 校验器 能输出标准 `ObservedDeviceState`。
- 缺字段、非法 VLAN、重复接口、未知端口模式都 失败关闭。
- 解析器 配置档案 仍保持 `production_ready=False`，直到样本集和真实设备行为一起完成评审。

## 不做

- 不连接生产下发路径。
- 不执行 candidate edit。
- 不因为一个样本通过就设置 生产就绪。
