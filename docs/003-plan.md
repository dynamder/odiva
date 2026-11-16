# 003 Plan

| | 内容 | 验收 |
| --- | --- | --- |
| **M0** | proto + UI（已完成） + 一个真 ACP harness + 可运行窗口 | 能派一个任务、能看到她说话、关窗隐藏、进程不死 |
| **M1** | 记忆（004）：journal → facts、tantivy + 中文分词 | 她记得上周的事 |
| **M2** | stage 子进程 + Live2D + 口型 | 屏幕上有个会动的东西 |
| **M3** | 第二个 harness、目录隔离、权限策略（005） | 能并行不打架 |

M0 里就能验证最关键的假设：**任务状态在 core、会话可丢弃** 能不能撑住 harness 异构。撑不住，后面全要重来。

## 现在到哪了

| | 状态 |
| --- | --- |
| `crates/ericed` | vendor 自 `../sofaia`，编译通过，示例自带断言 |
| `crates/odiva-proto` | 三个变体 + `CoreHandle`，零依赖 |
| `crates/odiva` | 聊天窗口：markdown、文件、快捷回复、时间分隔、虚拟化、贴底与跟随；跑在 fixtures 上 |
| `crates/odiva-core` | ACP 客户端：起进程、握手、一轮说完、拒绝权限；假 harness 三个脚本 |

窗口的验证方式有两个，都在 CI 里跑：

- `crates/odiva/tests/protocol.rs`：`App::apply` 是唯一入口；一条 `say` 一条气泡；400 条消息只建 <40 行；短对话有 headroom。
- `crates/odiva/tests/render.rs`：六个场景出图（空、对话、干活中、六任务、300 条、浅色），图在 `target/`。

## 下一步

M0 的切片顺序和每个切片的产物写在 006 末节 —— 这里不重复，避免两份清单漂移。

切片 1（假 harness + SDK 客户端）和切片 2（协议瘦身）已完成。下一个是切片 3：`say` 加上 talk 激活，用 yoagent 的 `MockProvider` 跑通"一句话进、一条消息出"。

## 阻塞（不是设计问题）

- 沙箱内进程拿不到 TLS（`SEC_E_NO_CREDENTIALS`）：`cargo add` / `cargo fetch` 要在提权下跑，之后依赖留在本地 registry，`--offline` 照常。
- `dsh --profile acp` 写 `$DSH_HOME/profiles/<name>`，而沙箱内 `~/.dsh` 不可写：跑真 harness 得把 `DSH_HOME` 指到工作区（`target/dsh-home` 已验证可用）。
- 真模型还差一个模型凭据：隔离出来的 `DSH_HOME` 里没有 `.credentials.yaml`，prompt 会以 `no API key for provider route` 报错——这一条要人来给。

## 待定

- 桌面窗口怎么拿到初始视口高度：现在靠 `sensor` + 首帧重排，真窗口里要在 boot 时问一次 `window::size`。
- 拖动滚动条时 `pinned` 只按"离底 2px"判定，没区分"用户主动拖到底"和"内容变短后自然到底"。

core 内部的默认值与未定项只在 006 维护。
