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
