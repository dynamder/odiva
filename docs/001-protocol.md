# 001 Protocol

UI 和 core 之间只有这一个面。UI 是 view，core 是唯一持有真值的地方。

## 命名

两个方向各一个 enum，**变体名是完整的句子：主体 + 动作 + 宾语**，不省略。同一个 match 里出现 `Turn(...)` 这种名字时，没人看得出它是"回合开始了"还是"回合结束了"。

类型名承担方向，变体名只说发生了什么：

- `UiMessage` —— UI 报告"用户做了什么"
- `CoreMessage` —— core 报告"世界发生了什么"

不叫 `UiCommand`：TEA 里 `Cmd` 专指副作用，把"用户报告的事实"叫 Command 自相矛盾。

## 类型

```rust
pub type MessageId = u64;

pub enum Side { User, Assistant }
pub enum Presence { Typing, Online }

pub struct FileRef { pub name: String, pub size: String }

/// 一条消息。正文是 markdown 字符串，不是结构化 block 数组——
/// 于是"能发什么"的边界就是"聊天软件能发什么"。
pub struct WireMessage {
    pub id: MessageId,
    pub from: Side,
    pub markdown: String,
    pub files: Vec<FileRef>,
    pub replies: Vec<String>,
}

pub struct Snapshot { pub messages: Vec<WireMessage>, pub presence: Presence }

pub enum UiMessage {
    UserSubmittedText { text: String },
    UserRequestedStop,
}

pub enum CoreMessage {
    CoreAttachedSession { snapshot: Snapshot },
    MessageAppended { message: WireMessage },
    MessageBegan { id: MessageId, from: Side },
    MessageStreamedText { id: MessageId, delta: String },
    MessageAttachedFile { id: MessageId, file: FileRef },
    MessageOfferedReplies { id: MessageId, replies: Vec<String> },
    MessageFinished { id: MessageId },
    PresenceChanged { presence: Presence },
}
```

## 四条约定

1. **消息只能经 `CoreMessage` 改变。** UI 不做乐观回显：core 在同进程，一次 channel 送信的延迟可以忽略，换来的是只有一处真值。
2. **流式文本先合并再发。** core 侧按 16~33ms 合并 token，`MessageStreamedText` 一次带一段。UI 不做节流。
3. **一条消息以 `MessageBegan` 开始、以 `MessageFinished` 结束。** 异常路径也必须发 `MessageFinished`，否则 UI 的"正在输入"会永久卡住。
4. **`replies` 是文本。** 点一下等于替用户说那句话，core 收到的是 `UserSubmittedText`——它不需要知道你是打的还是点的。

## 协议里没有 task

这是刻意的。任务生命周期、并发槽、权限等待全在 core 内部；agent 用自然语言把它翻译成人话：

> 「拆成六件，五件已经在跑。其中两件要改同一个 crate，我让它们串行。」

**并行任务不产生消息。** 六个任务在跑，是 agent 选择用一条消息说清楚。刷屏的责任在它的说话纪律，不在 UI 的聚合技巧——UI 也没有能力聚合，它没有 task 这个概念。

想看完整轨迹有两个正当出口：让 agent 贴（`Code` block），或者跳去 dsh。
