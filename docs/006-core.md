# 006 外部基座

本文只记 odiva 之外的事实：ACP、harness 产品、yoagent 的能力，以及由此推出的约束。
core 自身的模型在 [007](007-core.md)；两者冲突时以 007 为准。

## ACP 与 harness

| 事实 | 后果 |
| --- | --- |
| `dsh --profile acp` 是原生 ACP v1 server | 不需要 adapter；codex、pi 等按同一协议接 |
| 无 elicitation；无客户端 fs / terminal 操作 | 主动上报只能是"结束一轮再问"；我们不能替它读写文件 |
| `RequestPermissionRequest` 带 `tool_call.kind / locations / raw_input / title` | 提权可以结构化分类，不必靠标题猜（007 §8） |
| `session/cancel` 是通知，不是 kill | 取消先走它，给一个宽限期再杀进程（007 §6） |
| `initialize` 回 `agentCapabilities.load_session` | 能不能续会话是**探测**出来的，不写死 |
| `workspaceRoot = process.cwd()`，`DSH_PERMISSION_MODE` 默认 `workspace-write` | 我们给的 `cwd` 就是沙箱边界；越界写会作为提权上行 |
| worker 自带完整工具集与沙箱（`tool-pwsh/fs/fs-search/jobs/skill/subagent`） | 我们不设定任何 worker 工具；要减只能 `--patch`，要加只能 MCP 透传 |
| MCP 支持 stdio 与 Streamable HTTP | 透传孔可用；core 自己不消费 MCP |
| `agent-client-protocol` 是官方 Rust SDK | ACP 类型只出现在 `odiva-harness` 里 |

worker 的 patch 默认关掉 `tool-subagent`、`tool-subagent-fork`、`tool-jobs`：前两个让 worker 长出子树、绕过台账；第三个让 shell 作业活过 turn 结束，破坏"暂停即安全"。

`DSH_HOME` 必须指到任务目录或工作区内：默认的 `~/.dsh` 对沙箱内进程不可写，`dsh --profile acp` 会在 `mkdir profiles/<name>` 上 EPERM，连 `initialize` 都过不去。

## yoagent

| 能力 | 用在哪 |
| --- | --- |
| `agent_loop()` 无状态；`Agent` 是便捷构造器 | 一轮 = 构建、跑到边界、写回、丢弃 |
| `Session` 树 + JSONL + `append_new` | 线的磁盘格式就是它；entry 自带 id / parent / AgentMessage |
| `ToolSource` | 不再需要（`yield` 已删）；工具表按 roster 静态构建 |
| `steer` | 用户消息在她轮中途到达时，往里塞一句"对方在等"，不是取消 |
| `Extension` / `RunHooks`（`before_model` → `TurnDecision::Stop`） | 暂停同意后"本轮不再请求模型"的实现手段 |
| `RunHooks::finish` 的 `RunEnd`（`Completed` / `Cancelled` / `Stopped` / `Failed`） | 判断这一轮怎么结束，取代扫消息猜失败或沉默 |
| `CompactionStrategy` / `LlmCompaction` | 备选；我们仍然在循环外压缩，省掉 `HistoryDiverged` |
| 取消令牌（`ToolContext.cancel`） | 用户 Stop 用它；工具里只有这一处用到 |
| `MockProvider` / `ModelConfig::mock` | 不联网钉住 say / dispatch / 提问 / 静默 |
| `prompt_structured` | 交接与报告这类"形状必须对"的输出 |

## 由此定下的三条

1. **权限上行归 core，不归窗口。** ACP 天然把请求送到 client；窗口只认识消息与提问卡。
2. **我们不做编排协议。** ACP 没有依赖、并发、重试，这些都是 core 的判断，不是协议的事。
3. **会话可丢弃，台账在磁盘。** harness 会崩、会换、会续不上；任务的真相在 `tasks/<id>.json`，进程只是当下的一次尝试。
