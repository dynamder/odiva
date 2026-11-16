//! The talking lane, driven by a script instead of a model.

use std::path::PathBuf;

use odiva_core::{Ids, Lane, Say, Scripted, Store, Talk};
use odiva_proto::CoreMessage;
use serde_json::json;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use yoagent::provider::mock::{MockResponse, MockToolCall};
use yoagent::types::AgentTool;

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

fn tools(out: UnboundedSender<CoreMessage>, ids: &Ids) -> Vec<Box<dyn AgentTool>> {
    vec![Box::new(Say::new(out, ids.clone()))]
}

fn window() -> (UnboundedSender<CoreMessage>, UnboundedReceiver<CoreMessage>, Ids) {
    let (out, inbox) = unbounded_channel();
    (out, inbox, Ids::default())
}

fn said(inbox: &mut UnboundedReceiver<CoreMessage>) -> Vec<String> {
    let mut messages = Vec::new();

    while let Ok(message) = inbox.try_recv() {
        if let CoreMessage::MessageAppended { message } = message {
            messages.push(message.markdown);
        }
    }

    messages
}

#[tokio::test]
async fn one_turn_can_say_two_things_and_they_arrive_separately() {
    let store = scratch("talk-two-messages");
    let (out, mut inbox, ids) = window();
    let route = Scripted::new(vec![saying("先说话，再干活。"), saying("干完了。"), done()]);

    let mut talk = Talk::open(&store, &route, tools(out, &ids), PROMPT).expect("a lane");
    talk.ask("看下日志").await.expect("a turn");

    assert_eq!(said(&mut inbox), vec!["先说话，再干活。", "干完了。"]);
}

#[tokio::test]
async fn a_second_activation_continues_the_same_lane() {
    let store = scratch("talk-two-activations");
    let (out, mut inbox, ids) = window();

    let first = Scripted::new(vec![saying("一"), done()]);
    let mut talk = Talk::open(&store, &first, tools(out.clone(), &ids), PROMPT).expect("a lane");
    talk.ask("第一句").await.expect("a turn");
    drop(talk);

    let second = Scripted::new(vec![saying("二"), done()]);
    let mut talk = Talk::open(&store, &second, tools(out, &ids), PROMPT).expect("a lane");
    talk.ask("第二句").await.expect("a turn");

    assert_eq!(said(&mut inbox), vec!["一", "二"]);
    assert!(store.path(Lane::Talk { generation: 0 }).exists());
    assert!(!store.path(Lane::Talk { generation: 1 }).exists());
}

#[tokio::test]
async fn a_missing_attachment_says_nothing() {
    let store = scratch("talk-missing-file");
    let (out, mut inbox, ids) = window();
    let route = Scripted::new(vec![attaching("/nowhere/plan.md"), done()]);

    let mut talk = Talk::open(&store, &route, tools(out, &ids), PROMPT).expect("a lane");
    talk.ask("把计划发我").await.expect("a turn");

    assert!(said(&mut inbox).is_empty());
}

#[tokio::test]
async fn a_new_prompt_opens_the_next_generation() {
    let store = scratch("talk-new-prompt");
    let (out, _inbox, ids) = window();

    let route = Scripted::new(vec![saying("一"), done()]);
    let mut talk = Talk::open(&store, &route, tools(out.clone(), &ids), "旧的话").expect("a lane");
    talk.ask("第一句").await.expect("a turn");
    drop(talk);

    let route = Scripted::new(vec![saying("二"), done()]);
    let mut talk = Talk::open(&store, &route, tools(out, &ids), "新的话").expect("a lane");
    talk.ask("第二句").await.expect("a turn");

    assert!(store.path(Lane::Talk { generation: 0 }).exists());
    assert!(store.path(Lane::Talk { generation: 1 }).exists());
}
