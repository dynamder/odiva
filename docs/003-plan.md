# 003 Plan

| | 内容 | 验收 |
| --- | --- | --- |
| **M0** | proto + UI 接上 fake core + 一个真 ACP harness + 可运行窗口 | 能派一个任务、能看到她说话、关窗隐藏、进程不死 |
| **M1** | 记忆：journal → facts、tantivy + 中文分词、MCP 暴露 | 她记得上周的事 |
| **M2** | stage 子进程 + Live2D + 口型 | 屏幕上有个会动的东西 |
| **M3** | 第二个 harness、目录隔离、权限策略 | 能并行不打架 |

M0 里就能验证最关键的假设：**任务状态在 core、会话可丢弃** 能不能撑住 harness 异构。撑不住，后面全要重来。

## 现在到哪了

- `crates/ericed`：vendor 自 sofaia，编译通过。
- `crates/odiva-proto`：协议类型，零依赖。
- `crates/odiva`：聊天窗口，跑在协议类型上，截图测试回放一串 `CoreMessage`。
- `crates/odiva-core`：未开始。

## 待定

- DSH 是否原生支持 ACP（没有就需要一层薄 adapter）。
- 第一个 adapter 接谁。
- 底部对齐值不值得付 `sensor` 的代价。
