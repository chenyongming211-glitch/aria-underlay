# 真实交换机联调用例计划

## 1. 目标

本文档用于真实设备到位后的现场联调。当前项目阶段没有真实交换机，因此这里记录的是可执行用例和验收口径，不记录任何虚假的设备结果。

真实联调的核心目标只有三个：

- 确认 adapter 能稳定连接真实设备并读取 NETCONF capability。
- 确认 VLAN / interface 的结构化 desired state 能被正确渲染、下发、验证。
- 确认失败路径不会产生假成功，必要时进入 `RolledBack` 或 `InDoubt`。

## 2. 联调前置条件

| 项目 | 要求 |
| --- | --- |
| 管理连通性 | adapter 所在机器能访问交换机管理 IP |
| 协议 | 优先 NETCONF over SSH，默认端口 830 |
| 账号 | 使用专用测试账号，不使用个人管理员账号 |
| 凭据 | 只通过 `secret_ref` 读取，不写入 inventory / journal / audit |
| 测试资源 | 明确测试 VLAN 范围、测试接口、可回滚窗口 |
| 变更窗口 | 下发类测试必须在客户允许的维护窗口执行 |

## 3. 用例分级

### L0: 只读探测

| 用例 | 操作 | 预期 |
| --- | --- | --- |
| Capability Probe | 运行 `real_capability_probe` | 输出 raw capabilities 和 recommended strategy |
| 认证失败 | 使用错误 `secret_ref` | 返回标准化认证错误，不创建事务 |
| 端口不可达 | 使用不可达 IP / 端口 | 返回 `DEVICE_UNREACHABLE` 或 `NETCONF_CONNECT_FAILED` |

### L1: 状态读取

| 用例 | 操作 | 预期 |
| --- | --- | --- |
| VLAN subtree | 读取指定 VLAN | 只返回目标 VLAN 子树 |
| Interface subtree | 读取指定接口 | 只返回目标 interface 子树 |
| 空结果 | 查询不存在 VLAN / 接口 | 返回空状态，不报假错误 |
| XML 解析异常 | 设备返回非预期 XML | 返回标准化 parse error，不更新 shadow |

### L2: 单设备事务

| 用例 | 操作 | 预期 |
| --- | --- | --- |
| 创建 VLAN | desired state 增加 VLAN | `Prepare -> Commit -> Verify -> FinalConfirm` 成功 |
| 修改 VLAN name | 修改现有 VLAN name | 只产生 Update diff |
| 删除 VLAN | desired state 删除测试 VLAN | running 验证为空 |
| 配置 access 接口 | access VLAN 绑定 | verify 子树与 desired 一致 |
| 配置 trunk 接口 | allowed VLAN 集合变化 | VLAN 顺序归一后幂等 |
| 重复 apply | 同一 intent 连续执行 10 次 | 第一次成功，后 9 次 `NoOpSuccess` |

### L3: 异常与恢复

| 用例 | 注入点 | 预期 |
| --- | --- | --- |
| lock 失败 | 设备配置锁被占用 | 按策略退避，超时后失败，不创建假成功 |
| prepare 失败 | 非法 VLAN / 接口不存在 | rollback/discard，journal 到 `RolledBack` 或 `Failed` |
| commit 失败 | 设备拒绝 commit | rollback，无法确认时 `InDoubt` |
| verify 失败 | running 与 desired 不一致 | cancel/rollback，journal 记录失败摘要 |
| final confirm 超时 | confirmed-commit 窗口异常 | verify running；不能确认则 `InDoubt` |
| adapter 进程重启 | 事务中断 | recovery 根据 journal 调用 adapter recover |

## 4. 现场记录要求

每台设备必须补齐：

- `docs/device-capability-report.md` 的一份实例。
- `docs/vendor/<vendor>.md` 中的 capability、namespace、XML、异常行为记录。
- 真实下发日志中的 `request_id`、`tx_id`、`trace_id`。
- 每个失败用例的 adapter 错误码和原始错误摘要。

## 5. 不通过标准

出现以下任一情况，不允许标记真实联调通过：

- adapter 返回假 `NoChange` 或假 `Committed`。
- 未实现 driver 仍能进入生产下发路径。
- journal 缺失事务中间相位。
- `InDoubt` 被自动当作成功清理。
- 设备密码出现在日志、journal、audit 或 CI 输出中。
