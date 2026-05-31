# Aria Underlay 产品实现原理 PPT

这是用 `html-ppt-skill` 生成的内部技术培训 HTML PPT。

打开方式：

```bash
open docs/ppt/aria-underlay-implementation-principles/index.html
```

键盘操作：

- `←` / `→` / `Space`：翻页
- `S`：打开演讲者模式，查看当前页、下一页、讲稿和计时器
- `F`：全屏
- `O`：总览

主题内容：

- 产品目标和边界
- Rust Core / Python Adapter 分层
- intent 到 NETCONF 的 apply pipeline
- journal / shadow / recovery 事务可靠性
- MergeUpsert 与显式删除
- H3C renderer / parser / offline acceptance
- 运维日志和真实设备上线边界
