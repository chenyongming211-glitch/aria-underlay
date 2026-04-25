# Sprint 1A: NETCONF Backend Contract + Mock 计划

## 1. 背景

当前没有真实物理交换机，因此 Sprint 1 不以真实设备联调作为验收标准。

Sprint 1A 的目标是先把 Python Adapter 的 NETCONF backend 边界、capability profile、错误标准化和 CI 覆盖做扎实。

真实交换机联调后移到：

```text
Sprint 1B: Real Device Capability Probe
```

## 2. Sprint 1A 目标

实现不依赖真实设备的南向能力模拟：

- mock NETCONF backend。
- 多种 capability profile。
- Adapter 错误标准化。
- CI 覆盖 capability 成功与失败。
- Rust onboarding 仍然通过 fake adapter 走完整 gRPC 链路。

## 3. 范围

本阶段做：

- `MockNetconfBackend`。
- fake capability profiles。
- `AdapterError` 到 Protobuf 的转换。
- `FakeDriver` 使用 mock backend。
- 通过环境变量选择 profile。
- Python 单元测试。
- GitHub Actions 继续验证 Sprint 0 integration。
- Rust `adapter_actions_probe` 通过 gRPC 调用 `GetCurrentState` 与 `Prepare`。
- GitHub Actions 覆盖 prepare 成功、lock 失败、validate 失败三条端到端路径。

本阶段不做：

- 真实 `ncclient` 连接。
- 真实交换机 get-config。
- VLAN XML renderer。
- 事务 2PC。
- rollback artifact 真实恢复。
- CLI fallback 真实执行。

## 4. Capability Profiles

第一批 profiles：

```text
confirmed
candidate_only
running_only
cli_only
auth_failed
unreachable
unsupported
lock_failed
validate_failed
```

语义：

| Profile | 语义 |
| --- | --- |
| `confirmed` | 支持 NETCONF candidate / validate / confirmed-commit |
| `candidate_only` | 支持 candidate / validate，但不支持 confirmed-commit |
| `running_only` | 支持 writable-running / rollback-on-error |
| `cli_only` | 不支持 NETCONF，只支持 CLI fallback |
| `auth_failed` | 模拟认证失败 |
| `unreachable` | 模拟设备不可达 |
| `unsupported` | 可连通但不支持任何可用策略 |
| `lock_failed` | 支持 confirmed-commit 能力，但 lock candidate 失败 |
| `validate_failed` | 支持 confirmed-commit 能力，但 validate candidate 失败 |

## 5. 环境变量

```text
ARIA_UNDERLAY_ADAPTER_FAKE=1
ARIA_UNDERLAY_FAKE_PROFILE=confirmed
```

如果不设置，默认：

```text
confirmed
```

## 6. 验收标准

必须满足：

- Python tests 覆盖所有 mock profiles。
- `confirmed` profile 返回 confirmed-commit capability。
- `candidate_only` profile 不返回 confirmed-commit。
- `running_only` profile 返回 writable-running / rollback-on-error。
- `cli_only` profile 返回 CLI backend。
- `auth_failed` 返回标准化 `AdapterError`。
- `unreachable` 返回标准化 `AdapterError`。
- `GetCurrentState` 能返回稳定的 mock VLAN / interface 状态。
- `Prepare` 能模拟 lock / validate 成功与失败。
- CI 中 `confirmed` profile 必须跑通 `GetCurrentState + Prepare` 成功路径。
- CI 中 `lock_failed` profile 必须返回标准化 `LOCK_FAILED`。
- CI 中 `validate_failed` profile 必须返回标准化 `VALIDATE_FAILED`。
- GitHub Actions 保持 Rust / Python / Sprint 0 Integration 全部通过。

## 7. Sprint 1B 入口条件

进入真实设备联调前必须具备：

- 管理 IP。
- NETCONF 端口。
- secret_ref 对应凭据。
- 明确测试 VLAN 范围。
- 明确测试接口。
- 确认是否允许 lock / unlock。
- 确认是否允许 confirmed-commit。
