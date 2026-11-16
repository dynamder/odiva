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
| `crates/odiva` | 聊天窗口：markdown、文件、快捷回复、时间分隔、虚拟化、贴底与跟随；`CoreHandle` 出站、subscription 入站，接上 core |
| `crates/odiva-core` | ACP 客户端、talk 线、引擎（presence/排队/中止）、任务台账与五个任务工具；假 harness 四个脚本 |

窗口的验证方式有两个，都在 CI 里跑：

- `crates/odiva/tests/protocol.rs`：`App::apply` 是唯一入口；一条 `say` 一条气泡；400 条消息只建 <40 行；短对话有 headroom。
- `crates/odiva/tests/render.rs`：六个场景出图（空、对话、干活中、六任务、300 条、浅色），图在 `target/`。

## 下一步

M0 的切片顺序和每个切片的产物写在 006 末节 —— 这里不重复，避免两份清单漂移。

切片 1、2、3、5 已完成：假 harness、协议瘦身、`say` 与 talk 激活、任务台账；窗口也已接上 core（引擎由 `main` 起、`App::subscription` 收消息）。剩下的是切片 4（真模型，缺凭据）与切片 6（idle 三件事里的预算与自由时段；记忆与水位线不在这一轮）。

busy 态的线分离已完成：任务未完成时激活跑在 `work.session.<n>.jsonl` 上，以交接摘要起根；公开记录（用户消息、她的 `say`）仍然只写 talk 线，交接永远进不去。剩下两件：

1. **两处压缩。** 交接 ≤3%（四个字段），任何一条线 ≥60% 时压到 ≤8% + 最近 6 轮，旧文件轮转保留。`Talk` 现在既不交接也不压缩。
2. **预算。** `contexts/budget.toml`，只约束 idle 的自由时段与整理；自由时段本身还没写，所以预算先不落地。

## 阻塞（不是设计问题）

- 沙箱内进程拿不到 TLS（`SEC_E_NO_CREDENTIALS`）：`cargo add` / `cargo fetch` 要在提权下跑，之后依赖留在本地 registry，`--offline` 照常。
- `dsh --profile acp` 写 `$DSH_HOME/profiles/<name>`，而沙箱内 `~/.dsh` 不可写：跑真 harness 得把 `DSH_HOME` 指到工作区（`target/dsh-home` 已验证可用）。
- 真模型还差一个模型凭据：隔离出来的 `DSH_HOME` 里没有 `.credentials.yaml`，prompt 会以 `no API key for provider route` 报错——这一条要人来给。

## 待定

- 桌面窗口怎么拿到初始视口高度：现在靠 `sensor` + 首帧重排，真窗口里要在 boot 时问一次 `window::size`。
- 拖动滚动条时 `pinned` 只按"离底 2px"判定，没区分"用户主动拖到底"和"内容变短后自然到底"。
- `Worker::talk` 没有墙钟超时：卡住的 harness 会一直占着（它自己的 `--timeoutMs` 只管工具）。要不要加、加在哪，归 005 的槽与重启决定。

core 内部的默认值与未定项只在 006 维护。
