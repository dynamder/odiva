//! The talking lane, driven by a script instead of a model.

use std::path::PathBuf;

use odiva_core::say::Said;
use odiva_core::{Ids, Lane, Roster, Say, Scripted, Store, Talk};
use odiva_proto::{CoreMessage, Side};
use serde_json::json;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use yoagent::provider::mock::{MockResponse, MockToolCall};

const PROMPT: &str = "你是 odiva。";

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

fn tools(out: UnboundedSender<CoreMessage>, ids: &Ids, said: &Said) -> Roster {
    Roster::of(vec![Box::new(Say::new(out, ids.clone(), said.clone()))])
}

#[tokio::test]
async fn one_turn_can_say_two_things_and_they_arrive_separately() {
    let store = scratch("talk-two-messages");
    let (out, mut inbox, ids) = window();
    let said = Said::default();
    let route = Scripted::one(vec![saying("先说话，再干活。"), saying("干完了。"), done()]);

    let mut talk =
        Talk::open(&store, &route, PROMPT, tools(out, &ids, &said), said.clone()).expect("a lane");
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
async fn a_second_activation_continues_the_same_lane() {
    let store = scratch("talk-two-activations");
    let (out, mut inbox, ids) = window();

    let first = Scripted::one(vec![saying("一"), done()]);
    let said = Said::default();
    let mut talk =
        Talk::open(&store, &first, PROMPT, tools(out.clone(), &ids, &said), said.clone())
            .expect("a lane");
    talk.turn(&ids.theirs("第一句")).await.expect("a turn");

    let second = Scripted::one(vec![saying("二"), done()]);
    let said = Said::default();
    let mut talk = Talk::open(&store, &second, PROMPT, tools(out, &ids, &said), said.clone())
        .expect("a lane");
    talk.turn(&ids.theirs("第二句")).await.expect("a turn");

    let messages = shown(&mut inbox);
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1].1, "二");
    assert!(store.path(Lane::Talk { generation: 0 }).exists());
    assert!(!store.path(Lane::Talk { generation: 1 }).exists());
}

#[tokio::test]
async fn the_lane_keeps_what_the_window_showed() {
    let store = scratch("talk-lane-record");
    let (out, _inbox, ids) = window();
    let said = Said::default();
    let route = Scripted::one(vec![saying("在。"), done()]);

    let mut talk =
        Talk::open(&store, &route, PROMPT, tools(out, &ids, &said), said.clone()).expect("a lane");
    let user = ids.theirs("在吗");
    talk.turn(&user).await.expect("a turn");

    let log = store
        .read(Lane::Talk { generation: 0 })
        .expect("a lane file")
        .expect("a written lane");

    let sides: Vec<_> = log.messages().iter().map(|message| message.from).collect();
    assert_eq!(sides, vec![Side::User, Side::Assistant]);
    assert_eq!(log.messages()[0].markdown, "在吗");
    assert_eq!(log.messages()[1].markdown, "在。");
    assert_eq!(log.system(), PROMPT);
}

#[tokio::test]
async fn a_missing_attachment_says_nothing() {
    let store = scratch("talk-missing-file");
    let (out, mut inbox, ids) = window();
    let said = Said::default();
    let route = Scripted::one(vec![attaching("/nowhere/plan.md"), done()]);

    let mut talk =
        Talk::open(&store, &route, PROMPT, tools(out, &ids, &said), said.clone()).expect("a lane");
    talk.turn(&ids.theirs("把计划发我")).await.expect("a turn");

    assert!(shown(&mut inbox).is_empty());
}

#[tokio::test]
async fn a_new_prompt_opens_the_next_generation() {
    let store = scratch("talk-new-prompt");
    let (out, _inbox, ids) = window();

    let route = Scripted::one(vec![saying("一"), done()]);
    let said = Said::default();
    let mut talk = Talk::open(&store, &route, "旧的话", tools(out.clone(), &ids, &said), said)
        .expect("a lane");
    talk.turn(&ids.theirs("第一句")).await.expect("a turn");

    let route = Scripted::one(vec![saying("二"), done()]);
    let said = Said::default();
    let mut talk =
        Talk::open(&store, &route, "新的话", tools(out, &ids, &said), said).expect("a lane");
    talk.turn(&ids.theirs("第二句")).await.expect("a turn");

    assert!(store.path(Lane::Talk { generation: 0 }).exists());

    let carried = store
        .read(Lane::Talk { generation: 1 })
        .expect("the next generation")
        .expect("a written generation");

    assert!(
        carried
            .messages()
            .iter()
            .any(|message| message.markdown == "第一句"),
        "a new prompt is a new prefix, not a new conversation"
    );
}

#[tokio::test]
async fn a_busy_activation_writes_two_lanes() {
    let store = scratch("talk-busy-lanes");
    let (out, mut inbox, ids) = window();
    let said = Said::default();
    let route = Scripted::one(vec![saying("好。"), done()]);

    let mut work = Talk::busy(
        &store,
        0,
        &route,
        PROMPT,
        tools(out, &ids, &said),
        said.clone(),
        vec!["t1 · 在跑 · 改 README".to_owned()],
    )
    .expect("a work lane");

    work.turn(&ids.theirs("顺便问一句")).await.expect("a turn");

    let work_log = store
        .read(Lane::Work { generation: 0 })
        .expect("a work file")
        .expect("a written work lane");

    assert!(work_log.messages()[0].markdown.contains("【交接】"));
    assert!(work_log.messages()[0].markdown.contains("t1"));
    assert_eq!(work_log.messages().len(), 3);

    let talk_log = store
        .read(Lane::Talk { generation: 0 })
        .expect("a talk file")
        .expect("a written talk lane");

    assert_eq!(talk_log.messages().len(), 2);
    assert!(
        talk_log
            .messages()
            .iter()
            .all(|message| !message.markdown.contains("【交接】")),
        "the handoff never reaches the public record"
    );

    assert_eq!(shown(&mut inbox).len(), 1);
}

#[tokio::test]
async fn a_lane_that_outgrew_itself_rotates_and_keeps_the_tail() {
    let store = scratch("talk-compaction");

    let mut log = odiva_core::store::Log::new(PROMPT);

    for index in 0..10u64 {
        let side = if index % 2 == 0 {
            Side::User
        } else {
            Side::Assistant
        };

        log.push(odiva_proto::WireMessage::new(
            index + 1,
            0,
            side,
            format!("第 {index} 条"),
        ));
    }

    store
        .write(Lane::Talk { generation: 0 }, &log)
        .expect("a written lane");

    let (out, _inbox, ids) = window();
    let said = Said::default();
    let route = Scripted::one(vec![]);

    let mut talk =
        Talk::open(&store, &route, PROMPT, tools(out, &ids, &said), said.clone()).expect("a lane");

    talk.compact(6).expect("a rotation");

    let rotated = store
        .read(Lane::Talk { generation: 1 })
        .expect("a rotated file")
        .expect("a written rotation");

    assert_eq!(rotated.messages().len(), 7, "a residue and six messages");
    assert!(rotated.messages()[0].markdown.contains("【压缩】"));
    assert_eq!(rotated.messages()[1].markdown, "第 4 条");

    let original = store
        .read(Lane::Talk { generation: 0 })
        .expect("the old file")
        .expect("kept");

    assert_eq!(original.messages().len(), 10, "rotation never deletes");
}
