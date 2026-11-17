//! One activation, driven by a script instead of a model.

use std::path::PathBuf;

use odiva_core::line::{LineId, Mode};
use odiva_core::line::repair::repair;
use odiva_core::say::Said;
use odiva_core::halt::Halt;
use odiva_core::{Ids, Line, Roster, Route, Say, Scripted, Store, Talk, Transcript};
use odiva_proto::{CoreMessage, Side};
use serde_json::json;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use yoagent::Session;
use yoagent::provider::mock::{MockResponse, MockToolCall};
use yoagent::types::{AgentMessage, CustomMessage, Message, StopReason, Usage};

const PROMPT: &str = "你是 odiva。";
const BUSY: LineId = LineId::Normal(Mode::Busy);

fn idle() -> LineId {
    LineId::normal(Mode::Idle)
}

fn scratch(name: &str) -> Store {
    let path: PathBuf = std::env::temp_dir().join(format!("odiva-core-{name}"));
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

fn attaching(path: &str) -> MockResponse {
    MockResponse::ToolCalls(vec![MockToolCall {
        name: "say".to_owned(),
        arguments: json!({ "markdown": "给你。", "attach": [path] }),
        provider_metadata: None,
    }])
}

fn done() -> MockResponse {
    MockResponse::Text("好。".to_owned())
}

fn window() -> (UnboundedSender<CoreMessage>, UnboundedReceiver<CoreMessage>, Ids) {
    let (out, inbox) = unbounded_channel();
    (out, inbox, Ids::default())
}

fn shown(inbox: &mut UnboundedReceiver<CoreMessage>) -> Vec<(Side, String)> {
    let mut messages = Vec::new();

    while let Ok(message) = inbox.try_recv() {
        if let CoreMessage::MessageAppended { message } = message {
            messages.push((message.from, message.markdown));
        }
    }

    messages
}

fn opening(
    store: &Store,
    id: LineId,
    route: &dyn Route,
    prompt: &str,
    roster: Roster,
    said: &Said,
    seed: Option<String>,
) -> Talk {
    Talk::open(odiva_core::talk::Activation {
        store,
        id,
        route,
        prompt,
        roster,
        said: said.clone(),
        seed,
        halt: Halt::default(),
        told: odiva_core::Told::default(),
    })
    .expect("a line")
}

fn tools(out: UnboundedSender<CoreMessage>, ids: &Ids, said: &Said) -> Roster {
    Roster::of(vec![Box::new(Say::new(out, ids.clone(), said.clone()))])
}

/// Every text a line carries, root aside.
fn text(store: &Store, id: LineId, generation: u32) -> Vec<String> {
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
            yoagent::types::Content::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();

    (!text.trim().is_empty()).then_some(text)
}

#[tokio::test]
async fn one_turn_can_say_two_things_and_they_arrive_separately() {
    let store = scratch("talk-two-messages");
    let (out, mut inbox, ids) = window();
    let said = Said::default();
    let route = Scripted::one(vec![saying("先说话，再干活。"), saying("干完了。"), done()]);

    let mut talk = opening(&store, idle(), &route, PROMPT, tools(out, &ids, &said), &said, None);

    talk.turn(&ids.theirs("看下日志")).await.expect("a turn");

    let messages = shown(&mut inbox);
    assert_eq!(
        messages
            .iter()
            .map(|(_, markdown)| markdown.as_str())
            .collect::<Vec<_>>(),
        vec!["先说话，再干活。", "干完了。"]
    );
}

#[tokio::test]
async fn a_second_activation_continues_the_same_line() {
    let store = scratch("talk-two-activations");
    let (out, _inbox, ids) = window();

    let first = Scripted::one(vec![saying("一"), done()]);
    let said = Said::default();
    let mut talk = opening(&store, idle(), &first, PROMPT, tools(out.clone(), &ids, &said), &said, None);
    talk.turn(&ids.theirs("第一句")).await.expect("a turn");

    let second = Scripted::one(vec![saying("二"), done()]);
    let said = Said::default();
    let mut talk = opening(&store, idle(), &second, PROMPT, tools(out, &ids, &said), &said, None);
    talk.turn(&ids.theirs("第二句")).await.expect("a turn");

    assert_eq!(store.newest_line(&idle()), Some(0), "one line, not two");

    let carried = text(&store, idle(), 0);
    assert!(carried.iter().any(|message| message == "第一句"));
    assert!(carried.iter().any(|message| message == "第二句"));
}

#[tokio::test]
async fn the_transcript_keeps_what_the_window_showed() {
    let store = scratch("talk-transcript");
    let (out, _inbox, ids) = window();
    let said = Said::default();
    let route = Scripted::one(vec![saying("在。"), done()]);

    let mut talk = opening(&store, idle(), &route, PROMPT, tools(out, &ids, &said), &said, None);

    talk.turn(&ids.theirs("在吗")).await.expect("a turn");

    let transcript = Transcript::new(store.transcript())
        .read()
        .expect("a transcript");

    let said: Vec<_> = transcript
        .iter()
        .map(|message| (message.from, message.markdown.as_str()))
        .collect();

    assert_eq!(
        said,
        vec![(Side::User, "在吗"), (Side::Assistant, "在。")],
        "the record is the person's lines and hers, nothing else"
    );
}

#[tokio::test]
async fn a_missing_attachment_says_nothing() {
    let store = scratch("talk-missing-file");
    let (out, mut inbox, ids) = window();
    let said = Said::default();
    let route = Scripted::one(vec![attaching("/nowhere/plan.md"), done()]);

    let mut talk = opening(&store, idle(), &route, PROMPT, tools(out, &ids, &said), &said, None);

    talk.turn(&ids.theirs("把计划发我")).await.expect("a turn");

    assert!(shown(&mut inbox).is_empty());
}

#[tokio::test]
async fn a_new_prompt_opens_the_next_generation() {
    let store = scratch("talk-new-prompt");
    let (out, _inbox, ids) = window();

    let route = Scripted::one(vec![saying("一"), done()]);
    let said = Said::default();
    let mut talk = opening(&store, idle(), &route, "旧的话", tools(out.clone(), &ids, &said), &said, None);
    talk.turn(&ids.theirs("第一句")).await.expect("a turn");

    let route = Scripted::one(vec![saying("二"), done()]);
    let said = Said::default();
    let mut talk = opening(&store, idle(), &route, "新的话", tools(out, &ids, &said), &said, None);
    talk.turn(&ids.theirs("第二句")).await.expect("a turn");

    assert!(store.line_path(&idle(), 0).exists(), "the old file stays");

    let carried = text(&store, idle(), 1);
    assert!(
        carried.iter().any(|message| message == "第一句"),
        "a new prompt is a new prefix, not a new conversation"
    );
}

#[tokio::test]
async fn a_seed_only_enters_a_fresh_line() {
    let store = scratch("talk-seed");
    let (out, _inbox, ids) = window();
    let said = Said::default();

    let route = Scripted::one(vec![done()]);
    let mut talk = opening(
        &store,
        BUSY,
        &route,
        PROMPT,
        tools(out.clone(), &ids, &said),
        &said,
        Some("【交接】t1".to_owned()),
    );
    talk.turn(&ids.theirs("继续")).await.expect("a turn");

    let route = Scripted::one(vec![done()]);
    let mut again = opening(
        &store,
        BUSY,
        &route,
        PROMPT,
        tools(out, &ids, &said),
        &said,
        Some("【交接】不该进来".to_owned()),
    );
    again.turn(&ids.theirs("再继续")).await.expect("a turn");

    let carried = text(&store, BUSY, 0);
    assert_eq!(carried[0], "【交接】t1");
    assert!(
        !carried.iter().any(|message| message.contains("不该进来")),
        "a seed belongs to the first turn, not to every one"
    );
}

#[tokio::test]
async fn a_failed_turn_leaves_nothing_a_provider_would_refuse() {
    let store = scratch("talk-failed-turn");
    let (out, _inbox, ids) = window();
    let said = Said::default();
    let route = Scripted::one(vec![MockResponse::ErrorWithUsage(
        "400 Bad Request".to_owned(),
        Usage::default(),
    )]);

    let mut talk = opening(&store, idle(), &route, PROMPT, tools(out, &ids, &said), &said, None);
    talk.turn(&ids.theirs("在吗")).await.expect("a turn");

    let session = store
        .read_line(&idle(), 0)
        .expect("a line file")
        .expect("a written line");

    assert_eq!(
        repair(&session.path_messages()),
        None,
        "an assistant with nothing to send would poison the next turn"
    );
    assert!(
        text(&store, idle(), 0).iter().any(|said| said == "在吗"),
        "the person's line survives the turn that failed"
    );
}

#[tokio::test]
async fn startup_cleans_a_line_no_provider_would_take() {
    let store = scratch("talk-startup-repair");

    let mut broken = Session::new();
    broken.append(AgentMessage::Custom(CustomMessage::new(
        "root",
        json!({ "system": PROMPT }),
    )));
    broken.append(AgentMessage::Llm(Message::user("在吗")));
    broken.append(AgentMessage::Llm(Message::assistant(
        Vec::new(),
        StopReason::Aborted,
        "deepseek-flash",
        "deepseek",
        Usage::default(),
    )));
    store.write_line(&idle(), 0, &broken).expect("a broken line");

    let notes = store.repair_lines().expect("a cleanup");
    assert_eq!(notes.len(), 1, "the broken line is named");

    assert!(
        store.line_path(&idle(), 0).exists(),
        "the broken file stays where it is"
    );

    let cleaned = store
        .read_line(&idle(), 1)
        .expect("a line file")
        .expect("a cleaned line");

    assert_eq!(cleaned.path_messages().len(), 2, "the root and what was said");
    assert_eq!(repair(&cleaned.path_messages()), None);
    assert!(store.repair_lines().expect("a second cleanup").is_empty());
}

#[tokio::test]
async fn a_line_that_outgrew_itself_rotates_and_keeps_the_tail() {
    let store = scratch("talk-compaction");
    let mut line = Line::open(&store, idle(), PROMPT).expect("a line");

    for index in 0..10 {
        line.push_note(format!("第 {index} 条")).expect("a note");
    }

    line.compact(6, None).expect("a rotation");

    let rotated = text(&store, idle(), 1);
    assert_eq!(rotated.len(), 7, "a residue and six messages");
    assert!(rotated[0].contains("【压缩】"));
    assert_eq!(rotated[1], "第 4 条");

    let original = text(&store, idle(), 0);
    assert_eq!(original.len(), 10, "rotation never deletes");
}
