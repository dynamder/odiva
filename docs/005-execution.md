# 005 执行

一个任务 = 一个工作区 + 一条上下文线 + 一串 `Run`。odiva 的状态机与上下文线见 [007](007-core.md)；
本文只讲 harness 那一侧的进程、配额与台账。

## harness agent

harness 是外部产品，经 ACP 调用：dsh、codex、pi……它们各有自己的模型、工具、沙箱和死亡方式。
core 只需要知道三件事：怎么启动它（`HarnessSpec`：程序、参数、环境）、它的工作目录（就是沙箱边界）、
它支不支持 `session/load`（`initialize` 的能力位，探测出来的）。

它们的会话不由我们管理。台账只记 `sessions: [SessionId]`（可能多个），我们从不假设能续；
续不上就开新会话，把同一份 brief 再给一次。

## 一个 Run

```
spawn → initialize → session/load(有 id 且能力位允许) 否则 session/new(cwd)
      → prompt(brief 或指令) → 收 update → 结束（end_turn / cancel / 死）
```

- **进程在任务在飞期间保留**，同一个 ACP 会话连续喂多轮。这样"等你说话"的任务不必每轮重开进程。
- 一个 Run 结束不等于任务结束：`end_turn` 只代表这一轮说完了。
- 只有 `Run` 会被杀。odiva 的生成不归这里管。

## 配额

- `max_concurrent_runs`（默认 3）管的是**在飞 Run**：已经发出 prompt、还没结束。活着的进程不占配额。
- 卡在提权上的会话不占配额——它没有在跑，占着配额会死锁（三个任务同时提权就排不进第四个）。
- **同一工作区同时只允许一个在飞 Run**：同一个目录里两个 harness 并行改，冲突比省下的时间贵。
  不同工作区并行。
- 用户任务优先于兴趣任务；`Status::OnSlot` 只表示"等一个 Run 配额"。

## 在等谁

任务的状态不写成状态码，写成一句"它在等什么"：

| | 含义 | 卡面 | 占配额 |
| --- | --- | --- | --- |
| `OnHarness` | 一轮在跑 | 在跑 | ✔ |
| `OnPermission` | 卡在提权上，等人或 odiva 答 | 等你批准 | — |
| `OnInstruction` | 一轮结束了，在等一句指示 | 等你说话 | — |
| `OnSlot` | 等一个 Run 配额（或等工作区空出来） | 排队 | — |
| `Paused` | 回 idle 时被暂停，进程已停 | 暂停 | — |
| `Done` | 干完了，等下一件事 | 已完成 | — |
| `Cancelled` | 被叫停（odiva 或用户的意图） | 已停止 | — |
| `Failed` | 失败，附原因 | 失败 | — |

`Paused` 与 `Failed` 必须分开：暂停是用户的决定，失败是事实。

## 报告

harness 的一轮用 `end_turn` 结束时，最后要留一个信封：

````
```report
{"status": "done" | "need" | "blocked", "question": "…", "artifacts": ["…"]}
```
````

core 只解析 `status`；缺信封按 `done` 处理并记诊断，永不因为解析失败丢掉工人的产出。
`need` 让任务进 `OnInstruction`，`blocked` 让它进 `Failed`。

这套东西不进协议，也不进 UI。协议只传消息与提问；odiva 把状态翻译成人话。

## brief

派活时写 brief：目标 + 相关文件 + 约束。harness 看不到任何对话，所以 brief 必须自包含。
`accept_task` 的参数就是交接，`task_dispatch` 的参数就是 brief——两者的形状都由类型保证。

worker 自己的环境：`ODIVA_TASK_DIR`、`ODIVA_TASK_ID`，以及 harness 侧的两个开关
`DSH_HOME`（默认用用户自己的 `~/.dsh`，可用 `ODIVA_DSH_HOME` 改）与 `DSH_PERMISSION_MODE`
（默认 `workspace-write`）。怎么启动它由 `ODIVA_HARNESS` / `ODIVA_HARNESS_ARGS` 决定，默认
`dsh --profile acp`。

`cargo run -p odiva-core --example doctor` 只做握手 + `session/new`，不花一个 token，就能确认
程序解析、`.cmd` 启动器、协议、目录都对。

## 暂停与恢复

暂停（busy→idle）在进程侧的顺序：

1. 对每个在飞 Run 发 ACP `session/cancel`；
2. 宽限 5s，仍未结束 → kill 子进程；
3. 台账写 `Paused`，记下 `session_id` 与 `pid`。

恢复（回到 busy 后跑 `Paused` 任务）：spawn → initialize → 能力位允许且有 id 则 `session/load`，
否则 `session/new` + brief（并记下"这个 harness 不能续"）。长任务因此不会从头做一遍。

## 重启

- 启动恒为 Idle（[007](007-core.md) §12）：所有非终态任务标 `Paused`。
- 按台账里的 `pid` 清掉仍存活的孤儿进程（Windows 上父进程被强杀不会带走子进程）。
- 断线的任务不自动重派：由 odiva 在 idle 里看到它们，再由用户决定。

## 权限

提权是 harness 的请求上行到 client，判定在 core：四档策略与结构化 `Ask` 见 [007](007-core.md) §8。
本文只补一条：**这不是安全边界，是摩擦力的调节。** harness 跑在同一个用户下，能按绝对路径做任何事；
真正拦住它的是 harness 自己的沙箱，我们的策略只是决定"哪些越界值得打断人"。
