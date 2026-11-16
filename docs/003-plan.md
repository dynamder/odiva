# 003 Plan

| | 内容 | 验收 |
| --- | --- | --- |
| **M0** | proto + UI（已完成） + 一个真 ACP harness + 可运行窗口 | 能派一个任务、能看到她说话、关窗隐藏、进程不死 |
| **M1** | 记忆（004）：journal → facts、tantivy + 中文分词、MCP 暴露 | 她记得上周的事 |
| **M2** | stage 子进程 + Live2D + 口型 | 屏幕上有个会动的东西 |
| **M3** | 第二个 harness、目录隔离、权限策略（005） | 能并行不打架 |

M0 里就能验证最关键的假设：**任务状态在 core、会话可丢弃** 能不能撑住 harness 异构。撑不住，后面全要重来。

## 现在到哪了

| | 状态 |
| --- | --- |
| `crates/ericed` | vendor 自 `../sofaia`，编译通过，示例自带断言 |
| `crates/odiva-proto` | 协议类型 + `CoreHandle`，零依赖 |
| `crates/odiva` | 聊天窗口：markdown、文件、快捷回复、时间分隔、虚拟化、贴底与跟随 |
| `crates/odiva-core` | 未开始 |

窗口的验证方式有两个，都在 CI 里跑：

- `crates/odiva/tests/protocol.rs`：`App::apply` 是唯一入口；400 条消息只建 <40 行；短对话有 headroom；流式状态能结束。
- `crates/odiva/tests/render.rs`：六个场景出图（空、静止、流式、六任务、300 条、浅色），图在 `target/`。

## 下一步：M0 剩下的

1. `odiva-core` 骨架：一条消息进来 → 调模型 → 流式回 `CoreMessage`。
2. ACP client 最小闭环：spawn → `initialize` → `session/new` → 一句话 → 收流 → 结束。
3. 一个真 harness 接上，跑通"派一个任务、她说结果"。
4. 窗口的 outbox 接上 channel（现在是一个 `Vec`）。

建议顺序是先 2 后 1：ACP 这条路如果在你手上的 harness 走不通，M0 的形态会变，早撞早知道。可以先写一个假 agent 进程把 client 的形态钉住。

## 待定

- DSH 是否原生支持 ACP；没有就要一层薄 adapter。
- 第一个 adapter 接谁。
- 桌面窗口怎么拿到初始视口高度：现在靠 `sensor` + 首帧重排，真窗口里要在 boot 时问一次 `window::size`。
- 拖动滚动条时 `pinned` 只按"离底 2px"判定，没区分"用户主动拖到底"和"内容变短后自然到底"。
