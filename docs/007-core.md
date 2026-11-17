# 007 Core

> 状态：目标设计，代码迁移见 [003](003-plan.md)。本文是新模型的唯一出处；
> [006](006-core.md) 只保留外部基座的事实，凡是与本文冲突的地方以本文为准。

## 1 术语

| 词 | 代码 | 含义 |
| --- | --- | --- |
| odiva agent | `Odiva` | 唯一与用户说话、统筹 harness 的模型实例（一个 actor） |
| harness agent | `Harness` / `Run` | 经 ACP 调用的外部产品（dsh、codex、pi…）；`Run` 是一次 prompt 执行 |
| 用户任务 / 兴趣任务 | `Origin::{User, Interest}` | 用户请求的活 / 空闲时自己派出去的活 |
| 上下文线 | `Line` | 一条独立的模型上下文 |
| normal-idle / normal-busy | `Line::Normal(Mode)` | 常驻分发线 |
| 任务线 | `Line::Task(TaskId)` | 一个任务的上下文 |
| 交接 | `Handoff` | idle→busy 时写下、回 idle 时原样注入 |
| 提权 | `Escalation` | harness 请求越过沙箱边界 |
| 意图 | `Intent` | 工具提出的状态变更，在轮边界执行 |
| 提问 | `Question` | core 发出的阻塞式结构化问题 |

## 2 架构

```
  iced ──UiMessage──▶ wire pump ──▶ ┌────────┐ ◀── events ── run task ×N ──▶ harness 子进程
    ▲  CoreMessage                  │ Odiva  │ ── 指令 / 答复 ──▶ ACP 会话
    └──── say / question ──────────│        │
                                   └───┬────┘
                                       │ ask / tell
                                   ┌───▼────────┐
                                   │   Ledger   │ tasks/*.json
                                   └────────────┘
```

两个 kameo actor：`Odiva`、`Ledger`。窗口不是 actor：一条出站 channel 加一个把 `UiMessage` 泵进信箱的任务。一次 harness 调用是普通 tokio task，不是 actor。

**纪律**

1. 可变状态只在 actor 的字段里；工具拿窄句柄（`SayHandle` / `LedgerHandle` / `RunHandle`），只 `tell`。
2. kameo 的类型不出 `actor/`。
3. handler 里不 `ask` 自己。
4. 线只在轮边界落盘，轮中途不写。
5. 工具 = 立即副作用 + 一个 `Intent`；意图在轮边界执行。

## 3 状态与上下文线

```
Mode::Idle ⇄ Mode::Busy        她的决定，不由台账推导
normal-idle / normal-busy      常驻，不随转换重建
task-t3 …                      每任务一条，创建时开，结束后封存保留
```

- `active: LineId`；"切换上下文"就是改这一个字段，不造新上下文。
- 线的身份稳定；`generation` 只在系统提示词变化或上下文压缩时轮转，旧文件留着。
- 任务线与状态互斥：idle 只会有兴趣任务线，busy 只会有用户任务线。
- 每个任务对应一个工作区，任务按工作区归类；同一工作区同时只允许一个在飞 `Run`。

| 转换 | 触发 | 载荷 | 谁决定 |
| --- | --- | --- | --- |
| idle → busy | `accept_task { brief, workspace, handoff }` | 交接存进 `state.json`；写台账、开任务线；normal-busy 追加任务表 | 她（协商式，§6 N1） |
| busy → idle | 用户点〔同意〕（§9） | 注入：暂存交接 + 未完成任务 + 本 busy 期完成的任务；normal-busy 追加〔暂停〕 | **core**，不经过模型 |

busy→idle 没有交接文档：退回 idle 的全部内容都是推导物。

## 4 四个工具表

`say`、`web`、`view_history` 在四处都有，下表只列差异。

| 工具 | normal-idle | task-idle | normal-busy | task-busy |
| --- | --- | --- | --- | --- |
| `memory_*` | ✔ | — | — | — |
| `accept_task` | ✔ | — | — | — |
| `dispatch` | 兴趣 | — | 用户 | — |
| `task_list` | ✔ | — | ✔ | — |
| `task_read` | ✔ | ✔ | ✔ | ✔ |
| `switch_to_task` | 兴趣 | — | 用户 | — |
| `switch_to_normal` | — | ✔ | — | ✔ |
| `task_instruct` | — | ✔ | — | ✔ |
| `task_permit` | — | — | — | ✔ |
| `task_cancel` | — | ✔ | — | ✔ |
| `request_idle` | — | — | ✔ | ✔ |
| `switch_to_task` | 兴趣任务 | — | 用户任务 | — |
| `switch_to_normal` | — | ✔ | — | ✔ |

- `say` 只在 idle 限流（3 条 / 10 分钟）；busy 有人在等，不限。
- 记忆只服务 idle 的整理，所以只出现在 normal-idle；兴趣任务线里也读不到。
- `task_permit` 只出现在 task-busy：兴趣任务的提权由策略直接拒，到不了她手里。
- 四个 roster 各配一条系统提示词，缓存前缀按线固定。

## 5 意图与轮边界

```rust
enum Intent {
    Accept { brief: Brief, workspace: PathBuf, handoff: Handoff },
    Switch { line: LineId, timeout: Duration },
    ToNormal,
    RequestIdle,
}
```

工具做两件事：立即副作用（写台账、发消息、发 ACP 指令），然后 `tell` 一个 `Intent`。意图**在当前轮结束后**执行。

- `switch_to_task` 不打断本轮；工具结果说明"本轮结束后进入 t3"。
- `accept_task` 在工具里就把任务写进台账并拿到 id，转换在轮边界发生。
- 已 tell 的意图不可撤销：它已经在线的历史里，撤销会让历史与现实矛盾。

## 6 取消

### 强制取消

| | 谁 → 谁 | 怎么做 | 留下什么 |
| --- | --- | --- | --- |
| F1 | 用户 → odiva | 轮内 `CancellationToken` | 半截文本丢弃，已完成的工具结果保留，线里追加〔用户中断〕 |
| F2 | odiva → harness | ACP `session/cancel` → 宽限 5s → kill 进程 | 副作用由 harness 自理；台账 `Cancelled` |

F1 的边界：

- 正在执行工具时不打断工具，本轮在该工具返回后的边界结束。
- 未配对的 tool_call 由 yoagent 补 `Tool call not run: the run was cancelled.`，历史始终合法。
- 她睡着时 Stop 没有生成可停：语义是"消停一会儿"——把下一次 idle 自唤醒推迟一个静默期；事件唤醒不受影响。
- 排队的用户消息照常处理，它不是生成。

### 协商式取消

| | 谁 → 谁 | 请求放弃 | 谁决定何时执行 |
| --- | --- | --- | --- |
| N1 | 用户 → odiva | 她的闲事（idle 的兴趣任务、整理） | 她：判断这是不是任务，以及在哪个轮边界切 |
| N2 | odiva → 用户 | 全部在跑的活（回 idle） | 用户：答复到达时由 core 执行 |

### 不是取消

switch 超时、饥饿上限强制回 normal（只是离开那条线，harness 继续跑）、harness 崩溃（事实）、关窗（退出）、轮自然结束。

## 7 提问：唯一的结构化答复通道

```rust
pub struct QuestionOption { pub label: String, pub note: Option<Note> }
pub struct Note { pub prompt: String, pub required: bool }
pub struct Question { pub question: String, pub options: Vec<QuestionOption> }

// CoreMessage
QuestionAsked { question: Question },
QuestionClosed { resolution: Resolution },   // Answered | Withdrawn
// UiMessage
AnswerQuestion { option: usize, note: Option<String> }

// WireMessage.replies 保持 Vec<String>：模型自造的快捷回复，没有结构含义
```

- 同时至多一个问题，所以没有关联 id。
- `QuestionAsked` 到达即**禁用输入框**，问题卡渲染在消息流里；带 `note` 的选项在卡内展开一行输入（`required` 时不能空着提交）；提交发 `AnswerQuestion`，随后 `QuestionClosed` 才恢复输入。
- 提问卡是纯 UI 态，不进 `transcript.jsonl`；进程重启即作废（§12）。
- 两个使用方：`request_idle`（§9）与提权（§8）。问到答案的路径都不经过模型。

## 8 提权

`Ask` 必须结构化，来自 ACP `RequestPermissionRequest` 的 `tool_call.kind`、`locations`、`raw_input`、`title`。策略是对它的纯函数：

```rust
enum Verdict {
    Allow,                       // 白名单，连模型都不打扰
    Deny(&'static str),          // 硬禁区，模型无权推翻
    AgentMayDecide(Scope),       // 她可以在 Scope 内批，且必须说一句
    AskUser(Reason),
}
fn classify(ask: &Ask) -> Verdict;
```

| 档 | 例子 |
| --- | --- |
| 允许 | 读任意路径；写工作区；写工具链缓存与临时目录（`~/.cargo`、`~/.rustup`、`target/`、`%TEMP%`、`node_modules`）；拉依赖；起编译 / 测试进程 |
| 她可批 | 写任务 brief 点名的目录；读别的项目；非破坏性 shell |
| 必须问用户 | 写工作区外的其他位置；`git push`；改 home 下的点文件；删非空目录；改系统配置 |
| 一律拒 | 兴趣任务的任何越界；递归删工作区外、格式化、改 ACL / 注册表 / 权限位 |

- 白名单是配置文件，不是代码；解析失败一律 `AskUser`，永不 `Allow`。
- 问用户时一次只问一个，走同一个提问组件；用户点完由 core 直接把答复送回 `Run`，不经模型。

## 9 请求暂停（N2）

`request_idle { message }` 是**挂起的工具调用**：它自己发一条提问，然后等用户答复。

```
她调 request_idle { message }
  → QuestionAsked：message + 〔同意〕〔拒绝·note 可选〕〔要求解释·note 必填〕
  → 工具挂起，只接受 AnswerQuestion
      ├─ 同意     → 工具返回"对方同意"（配对完成）→ TurnDecision::Stop 结束本轮
      │             → core 清理并转换为 idle，全程不请求模型
      ├─ 拒绝     → 工具返回"对方拒绝" + note → 本轮继续，她接着干
      └─ 要求解释 → 工具返回"对方要求解释：" + note → 她解释，需要的话再次 request_idle
```

同意后的清理，按顺序：

1. 对每个在飞 `Run` 发 ACP `session/cancel`，宽限 5s；
2. 仍未结束 → kill 子进程；
3. 台账全部 `Paused`，记下 `session_id` 与 `pid`；
4. `mode = Idle`，`active = normal-idle`，注入交接与任务状态。

其它规则：

- 先把 tool_result 写进历史再结束本轮：assistant 的 tool_call 必须有配对，否则部分 API 拒绝继续生成。结束用 `TurnDecision::Stop`，是"停"，不是"取消"。
- 挂起期间 actor 不处理别的消息：用户正在决定要不要停摆，别的都不该往前跑。Stop 会取消这次挂起（提问以 `Withdrawn` 关闭）。
- 挂起期间不落盘，所以进程崩在等待中不会留下悬空的 tool_call。
- 要求解释后若 10 分钟内没有新的 `request_idle`：轮结束起定时器，到点在同一线注入提醒轮（"解释完就再发一次 `request_idle`；不打算停了就明说"），最多两次，之后记日志放弃。

## 10 唤醒与定时器

| 唤醒源 | 何时 | 谁 arm | 什么取消它 |
| --- | --- | --- | --- |
| 用户消息 | 随时 | — | — |
| Run 事件（一轮结束、提权、进程死亡） | 随时 | run task | — |
| idle 自唤醒 | 上轮结束 + 间隔（默认 1h，加抖动） | 她在轮尾 | 任何外部事件重置；Stop 推迟 |
| switch 超时 | 切进任务线后 | 她 | 每次 harness 反应重置（一轮结束、提权、死亡；工具调用不算） |
| 饥饿上限 | 从上次待在 normal 起算 | 她 | 回 normal |

- 事件一律进当前状态的 normal 线，它就是唤醒点与分发线。她正待在任务线时，别的任务的事件不抢占。
- 用户消息是唯一例外：收到时若在任务线，先把消息带回 normal 再处理。
- busy 没有心跳：没有事件就是没事。
- idle 自唤醒只在预算内 arm；预算耗尽就只等事件与用户。

## 11 上下文与存储

```
transcript.jsonl                      窗口的真相：每条 WireMessage，append-only
contexts/lines/normal-idle.<gen>.jsonl    yoagent Session 的 JSONL，第一行 root(system prompt)
contexts/lines/normal-busy.<gen>.jsonl
contexts/lines/task-t3.<gen>.jsonl
contexts/budget.json
tasks/t3.json                         台账：origin、brief、workspace、status、report、sessions、pid
state.json                            mode 不存；只存交接与计数（§12）
memory/  odiva.log
```

- 线直接用 `yoagent::Session`：entry 自带 `id`、`parent`、`AgentMessage`，工具调用与结果都在里面。
- 轮边界写盘：`session.append_new(agent.messages())`。压缩在循环外做，所以 `HistoryDiverged` 应当永不出现。
- 压缩：用量到窗口 60% 时轮转 generation（旧文件留着），新代以摘要 + 最近若干轮起根。
- `view_history` 只查自己这条线，跨 generation，按 entry id / 区间 / 过滤条件精确返回。
- 预算只扣 idle；对话与任务不设预算。
- `say` 限流只在 idle（3 条 / 10 分钟）。

## 12 启动与崩溃

- **启动恒为 Idle**：`mode` 不持久化，`active = normal-idle`。于是"提问挂起时被杀"和"任何时候被杀"是同一条规则：世界已经停了。
- 启动时把所有非终态任务标 `Paused`（不是 `Failed`），写进 idle 的注入里。
- 按台账里的 `pid` 清掉仍存活的孤儿 harness 进程（Windows 上父进程被强杀不会带走子进程）。
- 各类 deadline 不恢复；挂起中的提问作废（窗口没有它，输入框自然可用）。

## 13 旧模型里作废的结论

| 旧说法 | 现在 |
| --- | --- |
| 没有常驻 agent，只有一次次短命的激活 | 常驻 `Odiva` actor；线稳定，模型按需唤醒 |
| talk / work / roam 三条线 | normal-idle、normal-busy、每任务一条 |
| 状态 = 有没有未完成的用户任务 | 状态是她的决定，由 `accept_task` / `request_idle` 驱动 |
| 线只存 `WireMessage` | 线存完整 `AgentMessage`；窗口记录单独一份 |
| 交接由 `Handoff::of` 从最后一条用户消息推导 | 交接由 `accept_task` 的参数给出，形状由类型保证 |
| `yield` / `cancel_yield` 让出本轮 | 删除：她自愿收尾就是轮结束；`steer` 负责提醒有人等 |
| 提权归窗口，"不是你的决定" | 四档策略；中间一档她自己批，严重的只问用户 |
| 会话一问一进程 | 进程在任务在飞期间保留，暂停时走 ACP cancel 再杀 |
