# 设备 Capability 探测报告模板

## 基本信息

| 字段 | 值 |
| --- | --- |
| 客户 / 站点 | |
| 设备 ID | |
| 角色 | LeafA / LeafB |
| 厂商 | |
| 型号 | |
| 系统版本 | |
| 管理 IP | |
| NETCONF 端口 | 830 |
| 探测时间 | |
| 探测人 | |

## 探测结果

| 能力 | 是否支持 | 备注 |
| --- | --- | --- |
| NETCONF base:1.0 | | |
| NETCONF base:1.1 | | |
| candidate | | |
| validate:1.0 | | |
| validate:1.1 | | |
| confirmed-commit:1.0 | | |
| confirmed-commit:1.1 | | |
| persist-id | | 只有 confirmed-commit:1.1 才可视为支持 |
| rollback-on-error | | |
| writable-running | | |
| startup | | |

## 推荐事务策略

```text
ConfirmedCommit2Pc / Candidate2Pc / RunningRollbackOnError / BestEffortCli / Unsupported
```

结论：

```text
TODO
```

## 原始 Capabilities

把 `real_capability_probe` 输出的 `raw_capabilities` 粘贴到这里：

```text
TODO
```

## 错误与异常

| 项目 | 结果 |
| --- | --- |
| 认证失败时错误码 | |
| 端口不通时错误码 | |
| 超时时错误码 | |
| 设备 hello 是否异常 | |

## 备注

```text
TODO
```
