//! The engine, driven through the window's wire.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use odiva_core::{Engine, Handle, Harness, Lane, Route, Scripted, Store};
use odiva_proto::{CoreHandle, CoreMessage, Presence, Side, UiMessage};
use serde_json::json;
use tokio::sync::mpsc::UnboundedReceiver;
use yoagent::provider::mock::{MockResponse, MockToolCall};
use yoagent::Agent;
use yoagent::provider::{
    ModelConfig, ProviderError, StreamConfig, StreamEvent, StreamProvider, ToolDefinition,
};
use yoagent::types::{Content, Message, StopReason, Usage};

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
    Harness::new(env!("CARGO_BIN_EXE_fake_harness")).env("ODIVA_FAKE_SCRIPT", script)
}

fn scripted(
    turns: Vec<Vec<MockResponse>>,
    store: &Store,
) -> (Handle, UnboundedReceiver<CoreMessage>) {
    let route = Arc::new(Scripted::new(turns));
    let (engine, handle, messages) =
        Engine::new(store.clone(), route, PROMPT, harness("chat")).expect("an engine");

    tokio::spawn(engine.run());
    (handle, messages)
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
    assert!(store.path(Lane::Talk { generation: 0 }).exists());
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
    let (engine, handle, mut inbox) = Engine::new(store, route, PROMPT, harness("chat")).expect("an engine");
    tokio::spawn(engine.run());

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
    let (engine, handle, mut inbox) = Engine::new(store, route, PROMPT, harness("chat")).expect("an engine");
    tokio::spawn(engine.run());

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
async fn a_task_in_flight_moves_the_conversation_to_a_work_lane() {
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

    let (engine, handle, mut inbox) =
        Engine::new(store.clone(), route, PROMPT, harness("slow")).expect("an engine");
    tokio::spawn(engine.run());

    handle.send(UiMessage::UserSubmittedText {
        text: "把 README 改一下".to_owned(),
    });
    drain(&mut inbox, 3).await;

    handle.send(UiMessage::UserSubmittedText {
        text: "顺便问一句".to_owned(),
    });
    drain(&mut inbox, 4).await;

    let work = store
        .read(Lane::Work { generation: 0 })
        .expect("a work file")
        .expect("a written work lane");

    assert!(work.messages()[0].markdown.contains("【交接】"));
    assert!(work.messages()[0].markdown.contains("t1"));

    let talk = store
        .read(Lane::Talk { generation: 0 })
        .expect("a talk file")
        .expect("a written talk lane");

    assert_eq!(talk.messages().len(), 3);
    assert!(
        talk
            .messages()
            .iter()
            .all(|message| !message.markdown.contains("【交接】")),
        "the handoff never reaches the public record"
    );
}

#[tokio::test]
async fn asking_to_be_idle_moves_the_next_turn_off_the_work_lane() {
    let store = scratch("engine-parked");
    let route = Arc::new(Scripted::new(vec![
        vec![
            MockResponse::ToolCalls(vec![MockToolCall {
                name: "task_dispatch".to_owned(),
                arguments: json!({ "goal": "改 README" }),
                provider_metadata: None,
            }]),
            done(),
        ],
        vec![
            MockResponse::ToolCalls(vec![MockToolCall {
                name: "request_idle".to_owned(),
                arguments: json!({}),
                provider_metadata: None,
            }]),
            done(),
        ],
        vec![saying("在。"), done()],
    ]));

    let (engine, handle, mut inbox) =
        Engine::new(store.clone(), route, PROMPT, harness("slow")).expect("an engine");
    tokio::spawn(engine.run());

    handle.send(UiMessage::UserSubmittedText {
        text: "把 README 改一下".to_owned(),
    });
    drain(&mut inbox, 3).await;

    handle.send(UiMessage::UserSubmittedText {
        text: "先停一下".to_owned(),
    });
    drain(&mut inbox, 4).await;

    handle.send(UiMessage::UserSubmittedText {
        text: "那你说说现在在干嘛".to_owned(),
    });
    drain(&mut inbox, 4).await;

    let work = store
        .read(Lane::Work { generation: 0 })
        .expect("a work file")
        .expect("a written work lane");

    assert_eq!(
        work.messages().len(),
        2,
        "only the parked turn wrote to the work lane"
    );

    let talk = store
        .read(Lane::Talk { generation: 0 })
        .expect("a talk file")
        .expect("a written talk lane");

    assert_eq!(
        talk.messages().len(),
        4,
        "every line the person typed, and her answer"
    );
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
        store.path(Lane::Talk { generation: 1 }).exists(),
        "a lane past the threshold starts the next generation"
    );
    assert!(store.path(Lane::Talk { generation: 0 }).exists());
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

    let (engine, handle, mut inbox) =
        Engine::new(store, route, PROMPT, harness("crash")).expect("an engine");
    tokio::spawn(engine.run());

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
    let (handle, mut inbox) = scripted(vec![vec![done()]], &store);

    handle.send(UiMessage::UserSubmittedText {
        text: "在吗".to_owned(),
    });
    drain(&mut inbox, 3).await;

    let log = std::fs::read_to_string(store.root().join("odiva.log")).expect("a journal");

    assert!(log.contains("对方：在吗"), "the log has the turn in it: {log}");
    assert!(
        log.contains("她没说话"),
        "silence is a fact worth writing down: {log}"
    );
    assert!(
        log.contains("好。"),
        "and the words the model did produce are kept: {log}"
    );
}

#[tokio::test]
async fn a_provider_failure_is_not_a_quiet_turn() {
    let store = scratch("engine-provider-failure");
    let (handle, mut inbox) = scripted(vec![vec![MockResponse::Text(String::new())]], &store);

    handle.send(UiMessage::UserSubmittedText {
        text: "在吗".to_owned(),
    });

    let messages = drain(&mut inbox, 5).await;

    let note = messages
        .iter()
        .find_map(|message| match message {
            CoreMessage::CoreFailed { note } => Some(note.clone()),
            _ => None,
        })
        .expect("a failed provider must say so");

    assert!(
        note.contains("模型没有回话"),
        "an empty turn is never a quiet turn: {note}"
    );
}
