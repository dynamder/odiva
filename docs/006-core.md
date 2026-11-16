# 006 Core

core 里没有常驻的 agent，只有引擎、存档，和一次次短命的激活。

## 两条定义

**agent = 上下文 + 工具集。** 没有"它本人"这个对象：切换 agent 就是换这两项，没有第三件事要切换。

**一条线（lane）= 一个上下文文件 + 一份工具表。** 因此状态转换只做两件事：换文件、换 roster。

```
常驻   Engine   协商门、上下文体检、预算、水位线
       Store    transcript / tasks / memory / contexts

一次激活 Activation { lane, roster, route } —— 构造，跑到边界，写档案，销毁
```

`yoagent` 的分层正好对上：`agent_loop()` 是无状态自由函数，`Agent` 只是便捷构造器。历史从 `Session` 读（`path_messages()` → `Agent::with_messages`），跑完写回（`agent.messages()` → `Session::append_new`）。

## 三条不变量

| 不变量 | 为什么它必然成立 |
| --- | --- |
| 界面没有工作流词汇 | 唯一上行通道是 `say`，它只产出 `WireMessage` |
| 会话可丢弃，台账在磁盘 | agent 从不持有持久物；丢一个激活等于丢一个栈帧 |
| 记忆是 md，索引可重建 | 记忆不在 agent 里，agent 只是它的读者 |

## 外部基座

| 事实 | 后果 |
| --- | --- |
| `dsh --profile acp` 是原生 ACP v1 server | 不需要 adapter |
| 无 `session/load`、无 elicitation、无客户端 fs/terminal 操作 | worker 不能中途插话；上报只能是"结束一轮再问" |
| worker 自带完整工具集与沙箱（`dsh-base` 的 `tool-pwsh/fs/fs-search/jobs/skill/subagent`） | 我们不设定任何 worker 工具；要减只能 `--patch`，要加只能 MCP 透传 |
| `workspaceRoot: process.cwd()` + `DSH_PERMISSION_MODE`（默认 `workspace-write`） | 我们给的 `cwd` 就是沙箱边界；写工作区外会 ask 上来 |
| MCP 支持 stdio 与 Streamable HTTP | 透传孔可用；core 自己不消费 MCP |
| `agent-client-protocol` / `sacp` 是官方 Rust SDK | ACP 走 SDK，类型只出现在 `acp/` 内部 |
| `yoagent`：`AgentTool`、`ToolMiddleware`、`steer`、`TurnHook`、`Session` 树、`CompactionStrategy`、`retry`、`MockProvider` | 工具循环、历史、重试、测试替身都不用自己写 |

worker 的 patch 默认关掉 `tool-subagent`、`tool-subagent-fork`、`tool-jobs`：前两个让 worker 长出子树、绕过台账；第三个让 shell 作业活过 turn 结束，破坏"放槽即安全"。

## 状态

```
Idle  ⇔  没有用户发起的任务未结束
Busy  =  ¬Idle
```

**闲聊是 idle。** 闲聊不构成承诺，所以它不该阻止她整理记忆；长对话里每个间隙都能整理，正是桌宠该有的样子。

判据只有一条：**两个状态之间必须至少换掉上下文或工具表之一，否则它就是冗余状态。** idle/busy 换的是工具表。

## 工具表

| 工具 | Idle | Busy |
| --- | --- | --- |
| `say` | 3 条 / 10 分钟 | 无上限 |
| `memory_search / read / list / append / update / delete` | 全部可用 | **全部不可用** |
| `web` | 可用 | 可用 |
| `task_dispatch` | self 任务 | user 任务 |
| `task_brief / answer / cancel / list / read` | 可用 | 可用 |
| 提权（ACP `request_permission`） | 一律拒绝 | 停下并问人：`OnPermission` 占着槽，`task_permit` 回答 |
| `yield` / `cancel_yield` | 收到用户消息时存在 | — |
| `request_idle` | — | 存在未完成任务时始终存在 |
| `task_permit` | — | 有任务停在权限上时用 |

busy 禁掉记忆的一切操作，于是"模型把任务细节写进记忆"这个失败模式在结构上不存在；忙的时候要查记忆，只能用 `request_idle` 回到 idle —— 这也让那个工具有了正当理由。

`say` 的频率上限只在 idle：那时没有人在等，主动开口要有节制；busy 是有人在等结果，不该被限流。

## 上下文管理

```
contexts/
├── talk.session.<n>.jsonl    对话线：唯一的长期线
├── talk.summary.<n>.md       它自己的压缩残渣
├── work.session.jsonl        一个 busy 期的编排线，busy 结束即封存
├── work.handoff.md           进入 busy 时的交接（≤3%）
├── work.residue.md           退出时留下的残渣（≤3%）
└── roam/<ts>.session.jsonl   一段自 work，一摊一个
```

work 线**不从 talk 的前缀分叉**，它从交接摘要起根。任务窗口里没有一句聊天原文。

### 两处压缩，两个上限

| | 触发 | 上限 | 保留 | 路由 |
| --- | --- | --- | --- | --- |
| 交接（idle → busy） | 每次转换 | **≤3%，四个字段** | 不留原文 | 便宜 |
| 自压缩（任何一条线） | 用量 ≥ 60% | **摘要 ≤8% + 最近 6 轮** | 旧文件轮转保留 | 便宜 |

交接是结构化的，因为"几句话"靠提示词保证不了：

```rust
struct Handoff {
    topic: String,
    intent: String,
    commitments: Vec<String>,
    open: Vec<String>,
}
```

用 `prompt_structured` 让便宜模型产出这个形状，比让它写"简要总结"可靠：**约束由类型承担。**

当前实现先取确定性的版本：话题与意图取最后一条用户消息，承诺取手上的任务，未决取在等人回话的任务（`handoff.rs`）。形状不变，换成一次便宜模型调用只动 `Handoff::of` 的里面。

### 转换时发生什么

**idle → busy**：定 handoff → 建 work 线（root = handoff）→ talk 线原样留在磁盘 → 换 roster。

**busy → idle**：封存 work 线（结论进任务卡，残渣 ≤3%）→ 恢复 talk 线（<60% 就一个字都不压）→ 超了才按 8% + 6 轮压缩。这里**没有交接上限**：回到的是主题本身，不是一份给别人的摘要。

### 为什么压缩放在转换点

工具表变了，prompt 前缀本来就要重算，cache 已经失效；压缩改写历史中部，两者共用同一次 miss。同一状态内的重复激活则一直是追加，cache 持续命中 —— 她一次说三条消息不会让缓存白费。

### 三条结构规则

1. 任何 session 的第一个 entry 是 root：指向 system prompt，然后（talk 为空、work/roam 是 handoff）。
2. 压缩只轮转、不删除：旧文件留着，回跳靠 `checkpoint`。
3. **talk 线只放公开记录**：用户消息、她的 `say`、任务指针。编排推理、工具调用、报告正文一律不进 —— 上下文污染不是靠压缩清掉的，是结构上根本进不来。

## 说话

对用户说话是一个工具，不是流式通道。

```rust
say { markdown: String, attach: Vec<PathBuf>, replies: Vec<String> }
```

- `attach` 的路径由 core `stat` 成 `FileRef{name, size}`，模型不写大小；路径不存在返回 `ToolError`，用户什么都看不到。
- 一轮可以调多次，就是多条气泡：「先说话再干活」天然成立。
- 消息**整条弹出**。打字是窗口自己的效果，core 不为它改协议。
- 模型没调 `say`：**什么都不做**。不催、不兜底、不替她说。沉默是默认值，也是失败模式，只在日志里留痕。

## 任务

一个任务一个独立 harness 子进程，worker 没有任何工具面 —— 这是"记忆不进 executor"的另一面，也是 brief 必须自包含的原因。

状态机、两种等待、报告信封、槽与重启归 005；这里只有两件属于 core：

- **任务的供给方式**（`Worktree | EmptyDir | None`）决定 cwd，而 cwd 就是 dsh 的沙箱边界。改代码用 worktree，打游戏这类什么都不给。
- **`origin` 是 agent 产生的**：用户消息里来的任务是 `user`，自由时段自己派的是 `self`。用户任务可以抢占 self 任务的槽。

## 记忆

记忆只服务 idle：它不进 worker（brief 自包含），也不进 busy（工具表里根本没有）。存储、六个工具、水位线、索引策略都在 004；core 只决定一件事：

**idle 的优先级是整理记忆 → 增量索引 → 自由时段。** 整理是职责，不是爱好：预算和时段都用完时，先保证它发生。

## 预算

```
contexts/budget.toml
date  = "2026-11-16"      # 本地日历日，跨天重置
roam  = { used, limit }   # 自由时段
tidy  = { used, limit }   # 整理，单独记账
```

只约束 idle 的自 work；对话与任务不设预算 —— 用户说话不能因为钱被截断，用户在等的结果不能因为钱被截断。

## 协商与中断

用户消息到达只是记一条 pending，**不是中断**：工具执行完不是安全点，之前可能刚产生了副作用。

挂起请求不靠 steer、也不反复催，它只做一件事：**让 `yield` 存在**。工具在不在，就是"有没有人在等"的唯一信号；系统提示里一句话带过。

- 她调 `yield`：在自认为干净的地方挂起，换线。
- 她调 `cancel_yield`：明确拒绝转换（可能判定这是闲聊），连 checkpoint 都不打。
- 她什么都不调：继续干。core 不设超时，不做第二次覆盖 —— 她有权让用户等到天荒地老。
- 唯一的强制出口是用户自己：`UserRequestedStop` 中止当前激活、不碰 worker、不改台账；中止的是 roam 线就把那帧丢掉。

## 测试

- mind：`MockProvider` + `ModelConfig::mock` 脚本化多轮工具调用，不联网就能钉住 `say` / dispatch / 协商 / 静默。
- ACP：假 harness（用同一个 SDK 写的 agent，按 `ODIVA_FAKE_SCRIPT` 说话）钉住握手、整段到达、权限拒绝、崩溃。
- Session：跑一轮 → `append_new` → 再跑一轮 → `append_new`，两次都不许 `HistoryDiverged`（我们在循环外压缩，这条应当永远成立）。
- 窗口：截图场景重跑；`every_say_is_one_whole_bubble` 钉住"一条消息一条气泡"。

## M0 切片

1. 假 ACP harness + SDK 客户端（已完成）：spawn → initialize → session/new → prompt → 收 update → end_turn。
2. 协议瘦身（已完成）：三变体 + 窗口删流式路径，截图重跑。
3. `say` + talk 激活接真 yoagent（已完成）：`MockProvider` 脚本跑通"一句话进、一条消息出"。
4. `dsh --profile acp` 接上真模型，跑通 M0 验收。
5. 任务台账 + 两种等待 + `task_dispatch`（已完成）：台账在 `tasks/<id>.json`，五个工具，报告信封，槽 3 个。
6. idle 三件事 + 水位线 + 预算。

## 未定

| 项 | 暂定 |
| --- | --- |
| 中文分词 | `jieba-rs` 起步，接口不动再换 `lindera` |
| 自由时段上网 | 只留接口：`search(query,k) / fetch(url)`，默认实现返回不可用 |
| 预算默认值 | roam 300k/日，tidy 60k/日 |
| `say` 频率上限 | 3 条 / 10 分钟 |
| 并发槽 | 3 |
