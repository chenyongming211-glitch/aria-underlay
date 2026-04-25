# Sprint 1B: 真实 NETCONF Backend 接入计划

## 1. 目标

Sprint 1B 的目标不是立即完成真实配置下发，而是先把真实设备接入路径打通：

- Python Adapter 支持非 fake 模式。
- `NcclientNetconfBackend` 能连接真实设备并读取 server capabilities。
- capability 解析逻辑与 mock backend 共用同一套语义字段。
- 真实 backend 的状态解析和 prepare 下发在未完成前必须显式返回 `NOT_IMPLEMENTED` 类错误，不能静默伪成功。

## 2. 当前已完成边界

Python Adapter 侧已经拆出统一 backend contract：

```text
BackendCapability
NetconfBackend Protocol
NetconfBackedDriver
MockNetconfBackend
NcclientNetconfBackend
```

`FakeDriver` 现在只负责把 `MockNetconfBackend` 注入 `NetconfBackedDriver`，不再单独维护一套 Protobuf 响应转换逻辑。

非 fake 模式下，adapter 会基于 `DeviceRef.management_ip` 和 `DeviceRef.management_port` 创建 `NcclientNetconfBackend`。

非 fake 模式下，adapter 还会通过 `DeviceRef.secret_ref` 解析本地 secret。第一版支持两种来源：

```text
ARIA_UNDERLAY_SECRET_<SECRET_REF_KEY>_USERNAME
ARIA_UNDERLAY_SECRET_<SECRET_REF_KEY>_PASSWORD
ARIA_UNDERLAY_SECRET_<SECRET_REF_KEY>_KEY_PATH
ARIA_UNDERLAY_SECRET_<SECRET_REF_KEY>_PASSPHRASE
```

其中 `SECRET_REF_KEY` 会把 `local/test-device` 转成 `LOCAL_TEST_DEVICE`。

也支持 JSON 文件：

```json
{
  "secrets": {
    "local/test-device": {
      "username": "netconf",
      "password": "secret"
    }
  }
}
```

文件路径通过环境变量指定：

```text
ARIA_UNDERLAY_SECRET_FILE=/etc/aria-underlay/secrets.json
```

如果 `secret_ref` 找不到或内容不完整，adapter 必须返回标准化 `SECRET_NOT_FOUND`，不能让 gRPC 请求变成未分类的 transport error。

Rust 侧已经补出第一版产品初始化入口：

```text
InitializeUnderlaySite
  -> create secret_ref through SecretStore
  -> register LeafA / LeafB
  -> trigger onboarding
  -> summarize site status
```

该入口依赖 `SecretStore` trait。第一版提供 `InMemorySecretStore` 用于测试和本地集成，占住产品初始化边界；后续接入 Aria Controller 时，只需要替换为正式 secret provider / metadata store。

## 3. 本阶段明确不做

- 不做真实 VLAN/interface XML renderer。
- 不做真实 running config parser。
- 不做 candidate edit/validate/commit。
- 不做 confirmed-commit 事务。
- 不做 CLI fallback 下发。

这些能力必须等 capability probe、secret provider、renderer、parser 边界稳定后再进入。

## 4. 下一步开发项

1. 扩展 `NcclientNetconfBackend`。
   - 使用 secret provider 提供认证信息。
   - 增加 host key 策略占位。
   - 保留连接错误、认证错误、超时错误的标准化映射。

2. 增加真实 capability probe example。
   - 非 fake adapter 模式。
   - 只执行 `GetCapabilities`。
   - 输出 raw capabilities 和推荐事务策略。

3. 真实设备联调前补充 checklist。
   - 管理 IP。
   - NETCONF 端口。
   - `secret_ref`。
   - 测试 VLAN 范围。
   - 测试接口。
   - 是否允许 lock/unlock。
   - 是否允许 confirmed-commit。

## 5. 验收标准

- fake adapter 的现有 CI 全部保持绿色。
- `Product Initialization` CI 覆盖 2 台交换机初始化：
  - `confirmed` profile -> `Ready`。
  - `running_only` profile 且允许 degraded -> `ReadyWithDegradedDevice`。
- `capability_from_raw` 覆盖：
  - `candidate`
  - `validate:1.0`
  - `validate:1.1`
  - `confirmed-commit:1.0`
  - `confirmed-commit:1.1`
  - `writable-running`
  - `rollback-on-error`
- 非 fake 模式下，真实设备不可达时返回标准化 `DEVICE_UNREACHABLE` 或 `NETCONF_CONNECT_FAILED`。
- 未实现的真实 `GetCurrentState` 和 `Prepare` 必须返回显式错误，不能返回成功。

## 6. 当前前置工具

已经提供真实设备 capability 探测入口：

```text
examples/real_capability_probe.rs
```

该 example 只做：

```text
register device
  -> onboard device
  -> adapter GetCapabilities
  -> print lifecycle state / raw capabilities / recommended strategy
```

它不会执行：

```text
get-config
edit-config
lock
validate
commit
confirmed-commit
```

## 7. 本地运行方式

启动 Python Adapter，必须关闭 fake mode：

```bash
ARIA_UNDERLAY_ADAPTER_FAKE=0 \
ARIA_UNDERLAY_ADAPTER_LISTEN=127.0.0.1:50051 \
ARIA_UNDERLAY_SECRET_LOCAL_REAL_DEVICE_USERNAME=netconf \
ARIA_UNDERLAY_SECRET_LOCAL_REAL_DEVICE_PASSWORD='replace-me' \
python -m aria_underlay_adapter.server
```

`local/real-device` 会被转换成环境变量 key：

```text
LOCAL_REAL_DEVICE
```

也就是：

```text
ARIA_UNDERLAY_SECRET_LOCAL_REAL_DEVICE_USERNAME
ARIA_UNDERLAY_SECRET_LOCAL_REAL_DEVICE_PASSWORD
ARIA_UNDERLAY_SECRET_LOCAL_REAL_DEVICE_KEY_PATH
ARIA_UNDERLAY_SECRET_LOCAL_REAL_DEVICE_PASSPHRASE
```

运行 Rust probe：

```bash
ARIA_UNDERLAY_ADAPTER_ENDPOINT=http://127.0.0.1:50051 \
ARIA_UNDERLAY_DEVICE_ID=leaf-a \
ARIA_UNDERLAY_MGMT_IP=192.0.2.10 \
ARIA_UNDERLAY_MGMT_PORT=830 \
ARIA_UNDERLAY_SECRET_REF=local/real-device \
cargo run --example real_capability_probe
```

如果已经知道期望策略，可以增加：

```bash
ARIA_UNDERLAY_EXPECTED_STRATEGY=ConfirmedCommit2Pc
```

允许值参考 Rust enum：

```text
ConfirmedCommit2Pc
Candidate2Pc
RunningRollbackOnError
BestEffortCli
Unsupported
```

## 8. GitHub 手动 Workflow

已经提供手动 workflow：

```text
.github/workflows/real-netconf-probe.yml
```

它不会在 push / pull_request 时自动运行，只能手动触发。

运行前需要在 GitHub repository secrets 配置：

```text
ARIA_UNDERLAY_REAL_NETCONF_USERNAME
ARIA_UNDERLAY_REAL_NETCONF_PASSWORD
```

手动输入：

```text
device_id
management_ip
management_port
expected_strategy
```

该 workflow 适用于临时现场联调。真实客户环境如果没有从 GitHub runner 到客户机房管理网的连通性，应在现场本地执行 `real_capability_probe`，不要使用 GitHub runner 直连客户交换机。

## 9. 现场联调 Checklist

执行 Sprint 1B 前必须确认：

- 交换机管理 IP 可从运行 adapter 的机器访问。
- NETCONF over SSH 已启用。
- TCP 830 端口可达，或明确设备使用的 NETCONF 端口。
- 账号只具备测试所需权限，不使用个人管理员账号。
- 测试期间不会下发配置，本阶段只读 capability。
- 确认是否支持：
  - `base:1.0`
  - `base:1.1`
  - `candidate`
  - `validate:1.0`
  - `validate:1.1`
  - `confirmed-commit:1.0`
  - `confirmed-commit:1.1`
  - `rollback-on-error`
  - `writable-running`
- 保存输出到 capability report。
