# 001 协议

UI 和 core 之间只有这一个面。UI 是 view，core 是唯一持有真值的地方。

类型定义只有一处：`crates/odiva-proto/src/lib.rs`。这里只记约定——把类型再抄一遍，只会让两边漂移。

## 命名

两个方向各一个 enum，**变体名是完整的句子：主体 + 动作 + 宾语**，不省略。同一个 match 里出现
`Turn(...)` 这种名字时，没人看得出它是"回合开始了"还是"回合结束了"。

- `UiMessage` —— UI 报告"用户做了什么"
- `CoreMessage` —— core 报告"世界发生了什么"

不叫 `UiCommand`：TEA 里 `Cmd` 专指副作用，把"用户报告的事实"叫 Command 自相矛盾。

## CoreMessage

| 变体 | 什么时候发 |
| --- | --- |
| `CoreAttachedSession { snapshot }` | 连接建立时的第一条，也是唯一的全量重建入口 |
| `MessageAppended { message }` | 她说了一条完整的消息 |
| `PresenceChanged { presence }` | 一轮开始 `Typing`，本轮结束 `Online` |
| `CoreFailed { note }` | 某一轮没跑完。窗口显示在输入框上方，直到下一次尝试 |
| `QuestionAsked { question }` | 需要用户做一个结构化决定：此刻禁用输入框 |
| `QuestionClosed { resolution }` | 那个决定有了结果（`Answered` / `Withdrawn`）：恢复输入框 |

## UiMessage

| 变体 | 含义 |
| --- | --- |
| `UserSubmittedText { text }` | 用户说了一句话：打的、点的快捷回复，都一样 |
| `AnswerQuestion { option, note }` | 用户点了提问卡上的第几个选项，可选附一行说明 |
| `UserRequestedStop` | 用户按下停止：只停当前这一轮生成，不是停机 |

## 消息整条到达

她说话的唯一出口是 `say` 工具，所以消息**整条**到达：没有"半条消息"这个状态，也就不存在
"正在输入永久卡住"这种失败模式。窗口想让文字逐字出现，那是窗口自己的动画，core 不参与。

`CoreFailed` 是唯一的例外，也是它的理由：失败没有别的信道——她可以决定不说话，但一个窗口
不该因此假装什么都没发生。它不是她说的话，所以不是一个 `WireMessage`，也不进转录。

## 提问是唯一的结构化决定

`QuestionAsked` 与 `AnswerQuestion` 是一对，理由只有一个：**状态转换不能靠模型理解自由文本**。
用户点〔同意〕暂停时，清理与转换由 core 直接执行，不经过模型；同样，用户回答提权时，
答复直接回到那个 worker。

- **同时至多一个问题**，所以没有关联 id；`QuestionClosed` 之后才恢复输入。
- 选项带 `note` 时卡内展开一行输入（`required` 时不能空着提交）——"拒绝并说明""要求解释"
  要带给模型的信息走这里，不走自由输入。
- 提问卡是纯 UI 态：不进转录，进程重启即作废（启动恒为 Idle，见 [007](007-core.md) §12）。
- 它不携带工作流词汇：只有一段话和几个按钮。

## 快捷回复只是文本

`WireMessage.replies` 是 `Vec<String>`：模型自造的快捷回复，点了等于用户说了那句话，
core 收到的是 `UserSubmittedText`。它没有结构含义，也不需要 id——需要结构的地方用提问卡。

## 五条约定

1. **消息只能经 `CoreMessage` 改变。** UI 不做乐观回显：core 在同进程，一次 channel 送信的
   延迟可以忽略，换来的是只有一处真值。
2. **说话是一个工具，消息整条到达。** core 不合并 token、不做节流，也不发"开始/结束"标记。
3. **一条消息只到达一次。** 重复的 `MessageAppended` 是协议错误，不是"同一句话的更新"。
4. **提问没有 id。** 至多一个挂起的问题；`AnswerQuestion` 里的 `option` 只是"第几个按钮"。
5. **时间是 UTC 毫秒，措辞是窗口的。** core 说"什么时候"，窗口决定写"今天 15:42"还是
   "周三 15:42"——后者要知道读者在哪个时区、今天是几号。

## 协议里没有 task

这是刻意的。任务生命周期、并发配额、提权等待全在 core 内部；agent 用自然语言把它翻译成人话：

> 「拆成六件，五件已经在跑。其中两件要改同一个 crate，我让它们串行。」

**并行任务不产生消息。** 六个任务在跑，是 agent 选择用一条消息说清楚。刷屏的责任在它的说话纪律，
不在 UI 的聚合技巧——UI 也没有能力聚合，它没有 task 这个概念。

想看完整轨迹有两个正当出口：让 agent 贴（markdown 围栏代码块），或者跳去 dsh。
