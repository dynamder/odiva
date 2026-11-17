//! The engine, driven through the window's wire.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use odiva_core::line::{LineId, Mode};
use odiva_core::task::{Ledger, Status, Task};
use odiva_core::{Handle, Harness, Route, Scripted, Store, Transcript, start};
use odiva_proto::{Answer, CoreHandle, CoreMessage, Presence, Side, UiMessage};
use serde_json::json;
use tokio::sync::mpsc::UnboundedReceiver;
use yoagent::provider::mock::{MockResponse, MockToolCall};
use yoagent::Agent;
use yoagent::provider::{
    ModelConfig, ProviderError, StreamConfig, StreamEvent, StreamProvider, ToolDefinition,
};
use yoagent::types::{AgentMessage, Content, Message, StopReason, Usage};

const PROMPT: &str = "你是 odiva。";

fn scratch(name: &str) -> Store {
    let path = std::env::temp_dir().join(format!("odiva-engine-{name}"));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a scratch directory");
    Store::new(path)
}

fn saying(markdown: &str) -> MockResponse {
    MockResponse::ToolCalls(vec![MockToolCall {
        name: "say".to_owned(),
        arguments: json!({ "markdown": markdown }),
        provider_metadata: None,
    }])
}

fn done() -> MockResponse {
    MockResponse::Text("好。".to_owned())
}

fn harness(script: &str) -> Harness {
    Harness::new(support::fake_harness()).env("ODIVA_FAKE_SCRIPT", script)
}

fn scripted(
    turns: Vec<Vec<MockResponse>>,
    store: &Store,
) -> (Handle, UnboundedReceiver<CoreMessage>) {
    scripted_with(turns, store, "chat")
}

fn scripted_with(
    turns: Vec<Vec<MockResponse>>,
    store: &Store,
    script: &str,
) -> (Handle, UnboundedReceiver<CoreMessage>) {
    let route = Arc::new(Scripted::new(turns));
    start(store.clone(), route, PROMPT, harness(script))
}

fn dispatching(goal: &str) -> MockResponse {
    MockResponse::ToolCalls(vec![MockToolCall {
        name: "task_dispatch".to_owned(),
        arguments: json!({ "goal": goal }),
        provider_metadata: None,
    }])
}

fn idling(message: &str) -> MockResponse {
    MockResponse::ToolCalls(vec![MockToolCall {
        name: "request_idle".to_owned(),
        arguments: json!({ "message": message }),
        provider_metadata: None,
    }])
}

fn permitting(id: &str, allow: bool) -> MockResponse {
    MockResponse::ToolCalls(vec![MockToolCall {
        name: "task_permit".to_owned(),
        arguments: json!({ "id": id, "allow": allow }),
        provider_metadata: None,
    }])
}

/// Waits for the one task on disk to stop running, and returns it.
async fn settled(store: &Store) -> Task {
    for _ in 0..400 {
        let ledger = Ledger::at(store.tasks()).expect("a ledger");

        if let Some(task) = ledger.all().into_iter().find(|task| {
            !matches!(
                task.status,
                Status::OnHarness | Status::OnSlot | Status::OnPermission(_)
            )
        }) {
            return task;
        }

        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    panic!("the task never settled");
}

/// Collects the next `count` messages, or panics when the engine goes quiet.
async fn drain(inbox: &mut UnboundedReceiver<CoreMessage>, count: usize) -> Vec<CoreMessage> {
    let mut seen = Vec::new();

    for _ in 0..count {
        let message = tokio::time::timeout(Duration::from_secs(5), inbox.recv())
            .await
            .expect("the engine said something")
            .expect("the engine is alive");

        seen.push(message);
    }

    seen
}

fn bubbles(messages: &[CoreMessage]) -> Vec<(Side, String)> {
    messages
        .iter()
        .filter_map(|message| match message {
            CoreMessage::MessageAppended { message } => {
                Some((message.from, message.markdown.clone()))
            }
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_line_from_the_person_becomes_a_bubble_and_an_answer() {
    let store = scratch("engine-one-turn");
    let (handle, mut inbox) = scripted(vec![vec![saying("在。"), done()]], &store);

    handle.send(UiMessage::UserSubmittedText {
        text: "在吗".to_owned(),
    });

    let messages = drain(&mut inbox, 5).await;

    assert!(matches!(
        messages[0],
        CoreMessage::CoreAttachedSession { .. }
    ));
    assert_eq!(
        bubbles(&messages),
        vec![
            (Side::User, "在吗".to_owned()),
            (Side::Assistant, "在。".to_owned()),
        ]
    );
    assert!(matches!(
        messages[2],
        CoreMessage::PresenceChanged {
            presence: Presence::Typing
        }
    ));
    assert!(matches!(
        messages[4],
        CoreMessage::PresenceChanged {
            presence: Presence::Online
        }
    ));
}

#[tokio::test]
async fn the_next_window_opens_on_what_was_already_said() {
    let store = scratch("engine-snapshot");
    let (handle, mut inbox) = scripted(vec![vec![saying("在。"), done()]], &store);

    handle.send(UiMessage::UserSubmittedText {
        text: "在吗".to_owned(),
    });
    drain(&mut inbox, 5).await;

    let (handle, mut inbox) = scripted(vec![], &store);
    let _ = handle;
    let messages = drain(&mut inbox, 1).await;

    let CoreMessage::CoreAttachedSession { snapshot } = &messages[0] else {
        panic!("the first message is a snapshot");
    };

    assert_eq!(snapshot.messages.len(), 2);
    assert_eq!(snapshot.messages[1].markdown, "在。");
    assert_eq!(snapshot.presence, Presence::Online);
}

#[tokio::test]
async fn a_second_question_gets_its_own_turn() {
    let store = scratch("engine-two-turns");
    let (handle, mut inbox) = scripted(
        vec![vec![saying("一"), done()], vec![saying("二"), done()]],
        &store,
    );

    handle.send(UiMessage::UserSubmittedText {
        text: "第一句".to_owned(),
    });
    drain(&mut inbox, 5).await;

    handle.send(UiMessage::UserSubmittedText {
        text: "第二句".to_owned(),
    });
    let messages = drain(&mut inbox, 4).await;

    assert_eq!(bubbles(&messages)[1], (Side::Assistant, "二".to_owned()));
    assert!(store.line_path(&LineId::normal(Mode::Idle), 0).exists());
}

fn idle() -> LineId {
    LineId::normal(Mode::Idle)
}

fn busy() -> LineId {
    LineId::normal(Mode::Busy)
}

/// Every text a line carries, root aside.
fn line(store: &Store, id: LineId, generation: u32) -> Vec<String> {
    let session = store
        .read_line(&id, generation)
        .expect("a line file")
        .expect("a written line");

    session.path_messages().iter().filter_map(spoken).collect()
}

fn spoken(message: &AgentMessage) -> Option<String> {
    let AgentMessage::Llm(message) = message else {
        return None;
    };

    let content = match message {
        Message::User { content, .. } | Message::Assistant { content, .. } => content,
        _ => return None,
    };

    let text: String = content
        .iter()
        .filter_map(|block| match block {
            Content::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();

    (!text.trim().is_empty()).then_some(text)
}

fn transcript(store: &Store) -> Vec<(Side, String)> {
    Transcript::new(store.transcript())
        .read()
        .expect("a transcript")
        .into_iter()
        .map(|message| (message.from, message.markdown))
        .collect()
}

/// Answers from the next script line, but slowly enough for a second message to
/// arrive mid-turn.
#[derive(Clone, Default)]
struct Patient(Arc<Counted>);

#[derive(Default)]
struct Counted {
    saw_yield: AtomicBool,
}

impl Route for Patient {
    fn agent(&self) -> Agent {
        Agent::from_provider(self.clone(), ModelConfig::mock())
    }
}

#[async_trait::async_trait]
impl StreamProvider for Patient {
    async fn stream(
        &self,
        config: StreamConfig,
        _tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<Message, ProviderError> {
        if config.tools.iter().any(|tool: &ToolDefinition| tool.name == "yield") {
            self.0.saw_yield.store(true, Ordering::SeqCst);
        }

        tokio::select! {
            _ = cancel.cancelled() => Err(ProviderError::Cancelled),
            _ = tokio::time::sleep(Duration::from_millis(60)) => Ok(Message::assistant(
                vec![Content::Text {
                    text: "慢的答案".to_owned(),
                }],
                StopReason::Stop,
                "mock",
                "mock",
                Usage::default(),
            )),
        }
    }
}

#[tokio::test]
async fn a_message_sent_mid_turn_waits_its_turn() {
    let store = scratch("engine-queued");
    let route = Arc::new(Patient::default());
    let (handle, mut inbox) = start(store, route, PROMPT, harness("chat"));

    handle.send(UiMessage::UserSubmittedText {
        text: "一".to_owned(),
    });
    handle.send(UiMessage::UserSubmittedText {
        text: "二".to_owned(),
    });

    let messages = drain(&mut inbox, 6).await;
    let lines: Vec<_> = bubbles(&messages)
        .into_iter()
        .filter(|(side, _)| *side == Side::User)
        .map(|(_, markdown)| markdown)
        .collect();

    assert_eq!(lines, vec!["一", "二"]);
}

#[tokio::test]
async fn stopping_ends_the_turn_without_an_answer() {
    let store = scratch("engine-stop");
    let route = Arc::new(Patient::default());
    let (handle, mut inbox) = start(store, route, PROMPT, harness("chat"));

    handle.send(UiMessage::UserSubmittedText {
        text: "慢慢想".to_owned(),
    });
    drain(&mut inbox, 3).await;

    handle.send(UiMessage::UserRequestedStop);
    let messages = drain(&mut inbox, 1).await;

    assert!(matches!(
        messages[0],
        CoreMessage::PresenceChanged {
            presence: Presence::Online
        }
    ));
}

#[tokio::test]
async fn a_dispatched_task_comes_back_as_a_sentence() {
    let store = scratch("engine-dispatch");
    let (handle, mut inbox) = scripted(
        vec![
            vec![
                MockResponse::ToolCalls(vec![MockToolCall {
                    name: "task_dispatch".to_owned(),
                    arguments: json!({ "goal": "改 README 的启动命令" }),
                    provider_metadata: None,
                }]),
                done(),
            ],
            vec![saying("改完了，README 的命令换成新的了。"), done()],
        ],
        &store,
    );

    handle.send(UiMessage::UserSubmittedText {
        text: "把 README 的启动命令改一下".to_owned(),
    });

    let mut seen = Vec::new();

    loop {
        let message = tokio::time::timeout(Duration::from_secs(10), inbox.recv())
            .await
            .expect("the engine kept talking")
            .expect("the engine is alive");

        let answered = matches!(
            &message,
            CoreMessage::MessageAppended { message } if message.from == Side::Assistant
        );

        seen.push(message);

        if answered {
            break;
        }
    }

    assert_eq!(
        bubbles(&seen).last(),
        Some(&(Side::Assistant, "改完了，README 的命令换成新的了。".to_owned()))
    );
}

#[tokio::test]
async fn a_task_in_flight_moves_the_conversation_to_the_busy_line() {
    let store = scratch("engine-work-lane");
    let route = Arc::new(Scripted::new(vec![
        vec![
            MockResponse::ToolCalls(vec![MockToolCall {
                name: "task_dispatch".to_owned(),
                arguments: json!({ "goal": "改 README" }),
                provider_metadata: None,
            }]),
            done(),
        ],
        vec![saying("在。"), done()],
    ]));

    let (handle, mut inbox) = start(store.clone(), route, PROMPT, harness("slow"));

    handle.send(UiMessage::UserSubmittedText {
        text: "把 README 改一下".to_owned(),
    });
    drain(&mut inbox, 3).await;

    handle.send(UiMessage::UserSubmittedText {
        text: "顺便问一句".to_owned(),
    });
    drain(&mut inbox, 4).await;

    let busy = line(&store, busy(), 0);
    assert!(busy[0].contains("【交接】"), "the busy line starts from the handoff");
    assert!(busy[0].contains("t1"));

    let shown = transcript(&store);
    assert!(
        shown.iter().all(|(_, markdown)| !markdown.contains("【交接】")),
        "the handoff never reaches the record"
    );
    assert_eq!(
        shown.iter().filter(|(side, _)| *side == Side::User).count(),
        2,
        "the record keeps every line the person typed"
    );
}

#[tokio::test]
async fn agreeing_to_pause_moves_the_next_turn_off_the_busy_line() {
    let store = scratch("engine-parked");
    let route = Arc::new(Scripted::new(vec![
        vec![dispatching("改 README"), saying("派出去了。"), done()],
        vec![idling("先停一下"), done()],
        vec![saying("在。"), done()],
    ]));

    let (handle, mut inbox) = start(store.clone(), route, PROMPT, harness("slow"));

    handle.send(UiMessage::UserSubmittedText {
        text: "把 README 改一下".to_owned(),
    });

    drain_until(&mut inbox, |message| {
        matches!(message, CoreMessage::MessageAppended { message } if message.markdown == "派出去了。")
    })
    .await;

    handle.send(UiMessage::UserSubmittedText {
        text: "先停一下".to_owned(),
    });

    let asked = drain_until(&mut inbox, |message| {
        matches!(message, CoreMessage::QuestionAsked { .. })
    })
    .await;

    assert!(asked, "a pause is requested as a question, not as a sentence");

    handle.send(UiMessage::AnswerQuestion(Answer {
        option: 0,
        note: None,
    }));

    assert!(
        paused(&store).await,
        "agreeing stops the work and puts the tasks down"
    );

    handle.send(UiMessage::UserSubmittedText {
        text: "那你说说现在在干嘛".to_owned(),
    });

    drain_until(&mut inbox, |message| {
        matches!(message, CoreMessage::MessageAppended { message } if message.markdown == "在。")
    })
    .await;

    let idle = line(&store, idle(), 0);
    assert!(
        idle.iter().any(|text| text.contains("那你说说现在在干嘛")),
        "the turn after parking runs on the idle line"
    );
}

/// Waits until every unfinished task has been put down.
async fn paused(store: &Store) -> bool {
    for _ in 0..400 {
        let ledger = Ledger::at(store.tasks()).expect("a ledger");

        if ledger
            .all()
            .iter()
            .any(|task| task.status == Status::Paused)
        {
            return true;
        }

        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    false
}

/// Drains until the predicate matches something, or gives up.
async fn drain_until(
    inbox: &mut UnboundedReceiver<CoreMessage>,
    wanted: impl Fn(&CoreMessage) -> bool,
) -> bool {
    for _ in 0..200 {
        let Ok(Some(message)) = tokio::time::timeout(Duration::from_millis(50), inbox.recv()).await
        else {
            continue;
        };

        if wanted(&message) {
            return true;
        }
    }

    false
}

#[tokio::test]
async fn a_turn_that_fills_the_window_rotates_the_lane() {
    let store = scratch("engine-compaction");
    let usage = Usage {
        input: 90_000,
        output: 1_000,
        ..Usage::default()
    };

    let (handle, mut inbox) = scripted(
        vec![vec![MockResponse::TextWithUsage("好。".to_owned(), usage)]],
        &store,
    );

    handle.send(UiMessage::UserSubmittedText {
        text: "说句话".to_owned(),
    });
    drain(&mut inbox, 3).await;

    assert!(
        store.line_path(&idle(), 1).exists(),
        "a line past the threshold starts the next generation"
    );
    assert!(store.line_path(&idle(), 0).exists());
}

#[tokio::test]
async fn a_task_that_dies_is_never_silent() {
    let store = scratch("engine-task-failure");
    let route = Arc::new(Scripted::new(vec![
        vec![
            MockResponse::ToolCalls(vec![MockToolCall {
                name: "task_dispatch".to_owned(),
                arguments: json!({ "goal": "跑一个会死的活" }),
                provider_metadata: None,
            }]),
            done(),
        ],
        vec![done()],
    ]));

    let (handle, mut inbox) = start(store, route, PROMPT, harness("crash"));

    handle.send(UiMessage::UserSubmittedText {
        text: "去试试".to_owned(),
    });

    let mut notice = None;

    for _ in 0..12 {
        let message = tokio::time::timeout(Duration::from_secs(10), inbox.recv())
            .await
            .expect("the engine kept talking")
            .expect("the engine is alive");

        if let CoreMessage::CoreFailed { note } = message {
            notice = Some(note);
            break;
        }
    }

    let note = notice.expect("a dead worker reaches the window, not just the log");
    assert!(note.contains("t1"), "the notice names the task: {note}");
}

#[tokio::test]
async fn a_turn_that_says_nothing_is_written_down() {
    let store = scratch("engine-journal");
    let (handle, mut inbox) = scripted(
        vec![vec![done(), MockResponse::Text("还是不说。".to_owned())]],
        &store,
    );

    handle.send(UiMessage::UserSubmittedText {
        text: "在吗".to_owned(),
    });
    drain(&mut inbox, 4).await;

    let log = std::fs::read_to_string(store.root().join("odiva.log")).expect("a journal");

    assert!(log.contains("对方：在吗"), "the log has the turn in it: {log}");
    assert!(
        log.contains("她没说话"),
        "silence is a fact worth writing down: {log}"
    );
    assert!(
        log.contains("还是不说。"),
        "and the words the model did produce are kept: {log}"
    );
}

#[tokio::test]
async fn a_provider_failure_is_not_a_quiet_turn() {
    let store = scratch("engine-provider-failure");
    let (handle, mut inbox) = scripted(
        vec![vec![
            MockResponse::Text(String::new()),
            MockResponse::Text(String::new()),
        ]],
        &store,
    );

    handle.send(UiMessage::UserSubmittedText {
        text: "在吗".to_owned(),
    });

    let mut note = None;

    for _ in 0..8 {
        let message = tokio::time::timeout(Duration::from_secs(5), inbox.recv())
            .await
            .expect("the engine kept talking")
            .expect("the engine is alive");

        if let CoreMessage::CoreFailed { note: said } = message {
            note = Some(said);
            break;
        }
    }

    let note = note.expect("a failed provider must say so");

    assert!(
        note.contains("模型没有回话"),
        "an empty turn is never a quiet turn: {note}"
    );
}

#[tokio::test]
async fn a_turn_that_forgot_to_speak_gets_one_more_chance() {
    let store = scratch("engine-after-silence");
    let (handle, mut inbox) = scripted(
        vec![vec![
            MockResponse::Text("我在想，先不说话。".to_owned()),
            saying("在。"),
            done(),
        ]],
        &store,
    );

    handle.send(UiMessage::UserSubmittedText {
        text: "在吗".to_owned(),
    });

    let messages = drain(&mut inbox, 5).await;

    assert_eq!(
        bubbles(&messages),
        vec![
            (Side::User, "在吗".to_owned()),
            (Side::Assistant, "在。".to_owned()),
        ],
        "the reminder turns a forgotten say into a message"
    );
}

#[tokio::test]
async fn silence_after_the_reminder_is_a_choice() {
    let store = scratch("engine-after-silence-refused");
    let (handle, mut inbox) = scripted(
        vec![vec![
            MockResponse::Text("没什么可说的。".to_owned()),
            MockResponse::Text("还是不说。".to_owned()),
        ]],
        &store,
    );

    handle.send(UiMessage::UserSubmittedText {
        text: "随便说点".to_owned(),
    });

    let messages = drain(&mut inbox, 4).await;

    assert_eq!(
        bubbles(&messages),
        vec![(Side::User, "随便说点".to_owned())],
        "silence is still an answer"
    );
}

#[tokio::test]
async fn a_task_that_asks_waits_for_words() {
    let store = scratch("engine-need");
    let (handle, _inbox) = scripted_with(
        vec![
            vec![dispatching("删掉 migrate_v1.sh"), done()],
            vec![saying("它问了一件事。"), done()],
        ],
        &store,
        "need",
    );

    handle.send(UiMessage::UserSubmittedText {
        text: "把那个脚本处理掉".to_owned(),
    });

    let task = settled(&store).await;

    assert_eq!(task.status, Status::OnInstruction, "said: {}", task.said);
    assert_eq!(
        task.report.and_then(|report| report.question).as_deref(),
        Some("这个脚本还有别处引用，删掉吗？")
    );
}

#[tokio::test]
async fn a_permission_request_reaches_her_and_the_answer_goes_back() {
    let store = scratch("engine-permit");
    let (handle, _inbox) = scripted_with(
        vec![
            vec![dispatching("删掉 migrate_v1.sh"), done()],
            vec![permitting("t1", false), done()],
        ],
        &store,
        "permission",
    );

    handle.send(UiMessage::UserSubmittedText {
        text: "把那个脚本处理掉".to_owned(),
    });

    let task = settled(&store).await;

    assert!(
        task.said.contains("denied:reject-once"),
        "the worker was told no, by name: {}",
        task.said
    );
}
