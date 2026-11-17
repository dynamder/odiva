# 003 计划

设计与事实分家：新模型在 [007](007-core.md)，外部基座在 [006](006-core.md)，执行层在 [005](005-execution.md)。
本文只有两件东西：迁移阶段，和"现在到哪了"。

## 迁移阶段

| 阶段 | 内容 | 验收 |
| --- | --- | --- |
| **P0** | 文档改写：README + 000–007 全部对齐新设计 | 只有一份模型描述，没有互相矛盾的段落 |
| **P1** | 拆出 `odiva-harness`：ACP 客户端、结构化 `Ask`、假 harness（`session/load` 与多轮会话留到 P3 的 `Run`） | `cargo test --workspace` 全绿，零行为变化 |
| **P2** | `line/` + `transcript`：线改用 `yoagent::Session`，工具调用进历史；窗口记录单独一份 | 旧 engine 改跑在 normal-idle 上，现有 engine 测试全绿 |
| **P3** | `Odiva` / `Ledger` / wire pump 三个新模块；删 `engine` `talk` `tasks` `parked` `waiting` | 599 行 engine 测试逐条迁移后通过 |
| **P4** | 状态机、两条 normal 线 + 任务线、四个 roster、`Intent`、超时与饥饿、提问组件、提权策略（**进行中**：提问组件、`request_idle` 的暂停转换、提权四档策略、四个 roster、`accept_task`（含自写的交接与 `state.json`）、任务线与开关线、switch 超时与饥饿上限已完成。"问用户"那一档仍走她的 `task_permit`，尚未复用提问卡） | 转换载荷、超时回 normal、饥饿强制回、兴趣任务提权被拒、暂停三选项全路径 |
| **P5** | 压缩、`view_history`、预算与 say 限流接上（**进行中**：`view_history` 已完成，跨 generation、只读自己的线；压缩仍是确定性的残渣；预算与限流未接） | 压缩后 `HistoryDiverged` 不出现；预算只扣 idle 自触发 |
| **P6** | `drive` 示例、README、注释清洗 | 人能跑的东西还在，注释占比 < 5% |

每个阶段都可编译、可跑、可提交；一个阶段 1–3 个 conventional commit。

## 之后的功能里程碑

| | 内容 | 验收 |
| --- | --- | --- |
| **M1** | 记忆（[004](004-memory.md)）：journal → facts、tantivy + 中文分词 | 她记得上周的事 |
| **M2** | stage 子进程 + Live2D + 口型 | 屏幕上有个会动的东西 |
| **M3** | 第二个 harness 产品（codex / pi）、多产品注册表 | 能并行不打架 |

P4 之前就能验证最关键的假设：**状态是她的决定、线是记录、进程可丢弃** 能不能撑住 harness 异构。
撑不住，后面全要重来。

## 现在到哪了

| | 状态 |
| --- | --- |
| `crates/odiva-proto` | 四个 `CoreMessage` + 两个 `UiMessage`；提问组件尚未加（P4） |
| `crates/odiva-harness` | **P1 完成**：ACP 客户端、结构化 `Ask`、`Harness`、假 harness 与它的测试 |
| `crates/odiva` | 聊天窗口：markdown、文件、快捷回复、时间分隔、虚拟化、贴底与跟随；接上 core |
| `crates/odiva-core` | **P3 完成**：`Odiva` 与 `Records` 两个 actor 取代 engine 与 tasks；`line/` 用 `yoagent::Session` 存上下文，`transcript.jsonl` 单独记窗口的东西 |
| `crates/ericed` | vendor 自 `../sofaia`，编译通过，示例自带断言 |
| 文档 | P0 完成：007 是新设计的唯一出处 |
| 提问 | P4 已落地：proto 的 `QuestionAsked` / `QuestionClosed` / `AnswerQuestion`，窗口的提问卡与禁用输入，core 的阻塞式 `request_idle` 与暂停清理（截图 `target/ui-chat-question.png`） |

P4 之前的差异只剩状态机部分：忙碌仍由台账推导（没有 `accept_task` / `request_idle` 的转换），
工具表还是两个而不是四个，提权还没有策略。P4 一次补齐。

## 阻塞（不是设计问题）

- 沙箱内进程曾经拿不到 TLS（`SEC_E_NO_CREDENTIALS`）：`cargo add` / `cargo fetch` 需要一次提权，
  之后依赖留在本地 registry，`--offline` 照常。
- `dsh --profile acp` 写 `$DSH_HOME/profiles/<name>`，而沙箱内 `~/.dsh` 不可写：跑真 harness 得把
  `DSH_HOME` 指到工作区（`target/dsh-home` 已验证可用）。
- 真模型需要模型凭据：隔离出来的 `DSH_HOME` 里没有凭据文件，prompt 会以 `no API key for provider route`
  报错——这一条要人来给。

## 待定

这些不影响 P1–P6，但迟早要定：

- 记忆的具体实现（分词的起步方案、水位线）。
- "上网"的形态：MCP 还是专用浏览器。
- 兴趣任务的工作区配置；harness 注册表（多产品）与各自参数。
- 提权白名单文件的格式与位置。
- 预算、配额、各定时器的默认数值。
- 封存的任务线保留多久（`view_history` 能回溯多远）。
- 用户"验收"的交互形态；同一 busy 期多个用户任务的排队与优先级。
