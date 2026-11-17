//! What a provider will take back, and nothing else.
//!
//! A turn can end with nothing: cancelled mid-stream, or dead before the first
//! token. What it leaves is an assistant message with no content, no tool call
//! and no reasoning — which every API rejects, so the whole line becomes
//! unusable and the next turn dies with `400 Invalid assistant message`. A tool
//! call whose result never arrived is the same fault one step later.
//!
//! A line is resident, so it is cleaned at both ends: a turn keeps only what it
//! can replay, and startup rewrites a line that was already broken as its next
//! generation, leaving the broken file where it is.

use std::collections::HashSet;

use yoagent::Session;
use yoagent::types::{AgentMessage, Content, Message};

use crate::error::Result;
use crate::store::Store;

use super::LineId;

/// The same path, with every message a provider would reject taken out.
/// `None` when there was nothing to take out.
pub fn repair(messages: &[AgentMessage]) -> Option<Vec<AgentMessage>> {
    let mut legal = Vec::with_capacity(messages.len());
    let mut index = 0;

    while index < messages.len() {
        let Some(Message::Assistant { content, .. }) = llm(&messages[index]) else {
            if !matches!(llm(&messages[index]), Some(Message::ToolResult { .. })) {
                legal.push(messages[index].clone());
            }

            index += 1;
            continue;
        };

        let (results, next) = results_after(messages, index + 1);
        let asked = calls(content);
        let answered: HashSet<&str> = results.iter().filter_map(|result| call_id(result)).collect();

        if speaks(content) && asked.iter().all(|id| answered.contains(id)) {
            legal.push(messages[index].clone());

            legal.extend(
                results
                    .iter()
                    .filter(|result| call_id(result).is_some_and(|id| asked.contains(&id)))
                    .map(|result| (*result).clone()),
            );
        }

        index = next;
    }

    (legal.as_slice() != messages).then_some(legal)
}

impl Store {
    /// Rewrites every line that holds a message no provider will take back as
    /// its next generation, and says what it cleaned.
    pub fn repair_lines(&self) -> Result<Vec<String>> {
        let mut notes = Vec::new();

        for id in self.line_names() {
            let Some(generation) = self.newest_line(&id) else {
                continue;
            };

            let Some(session) = self.read_line(&id, generation)? else {
                continue;
            };

            let messages = session.path_messages();

            let Some(legal) = repair(&messages) else {
                continue;
            };

            let mut repaired = Session::new();

            for message in &legal {
                repaired.append(message.clone());
            }

            let next = generation + 1;
            self.write_line(&id, next, &repaired)?;

            notes.push(format!(
                "{}：第 {generation} 代有 {} 条重放不了的记录，清成第 {next} 代",
                id.name(),
                messages.len() - legal.len()
            ));
        }

        Ok(notes)
    }

    /// Every line that has a file, however old.
    fn line_names(&self) -> Vec<LineId> {
        let Ok(entries) = std::fs::read_dir(self.lines()) else {
            return Vec::new();
        };

        let mut ids: Vec<LineId> = Vec::new();

        for entry in entries.filter_map(|entry| entry.ok()) {
            let Some(name) = entry.file_name().into_string().ok() else {
                continue;
            };

            let Some((line, generation)) = name.strip_suffix(".jsonl").and_then(|stem| stem.rsplit_once('.'))
            else {
                continue;
            };

            if generation.parse::<u32>().is_err() {
                continue;
            }

            let Some(id) = LineId::named(line) else {
                continue;
            };

            if !ids.contains(&id) {
                ids.push(id);
            }
        }

        ids
    }
}

fn llm(message: &AgentMessage) -> Option<&Message> {
    match message {
        AgentMessage::Llm(message) => Some(message),
        AgentMessage::Custom(_) => None,
    }
}

/// Whether there is anything in this message to send: the three things every
/// API wants at least one of.
fn speaks(content: &[Content]) -> bool {
    content.iter().any(|block| match block {
        Content::Text { text } => !text.trim().is_empty(),
        Content::Thinking {
            thinking, redacted, ..
        } => !thinking.trim().is_empty() || redacted.is_some(),
        _ => true,
    })
}

fn calls(content: &[Content]) -> Vec<&str> {
    content
        .iter()
        .filter_map(|block| match block {
            Content::ToolCall { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect()
}

fn call_id(message: &AgentMessage) -> Option<&str> {
    match llm(message) {
        Some(Message::ToolResult { tool_call_id, .. }) => Some(tool_call_id),
        _ => None,
    }
}

/// The results answering one assistant message: everything until something
/// else speaks.
fn results_after(messages: &[AgentMessage], start: usize) -> (Vec<&AgentMessage>, usize) {
    let mut end = start;

    while end < messages.len() && call_id(&messages[end]).is_some() {
        end += 1;
    }

    (messages[start..end].iter().collect(), end)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use yoagent::types::{AgentMessage, Content, Message, StopReason, Usage};

    use super::repair;

    fn user(text: &str) -> AgentMessage {
        AgentMessage::Llm(Message::user(text))
    }

    fn assistant(content: Vec<Content>) -> AgentMessage {
        AgentMessage::Llm(Message::assistant(
            content,
            StopReason::Stop,
            "deepseek-flash",
            "deepseek",
            Usage::default(),
        ))
    }

    fn result(id: &str) -> AgentMessage {
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id: id.to_owned(),
            tool_name: "say".to_owned(),
            content: vec![Content::Text {
                text: "好".to_owned(),
            }],
            is_error: false,
            timestamp: 0,
        })
    }

    fn text(text: &str) -> Vec<Content> {
        vec![Content::Text {
            text: text.to_owned(),
        }]
    }

    fn call(id: &str) -> Vec<Content> {
        vec![Content::tool_call(id, "say", json!({ "markdown": "好" }))]
    }

    #[test]
    fn a_line_that_replays_is_left_alone() {
        let messages = vec![user("在吗"), assistant(text("在。"))];

        assert_eq!(repair(&messages), None);
    }

    #[test]
    fn a_turn_that_ended_with_nothing_is_dropped() {
        let messages = vec![user("在吗"), assistant(Vec::new())];

        assert_eq!(repair(&messages), Some(vec![user("在吗")]));
    }

    /// What a failed turn really leaves: one text block, empty.
    #[test]
    fn an_empty_text_block_is_still_nothing() {
        let messages = vec![user("在吗"), assistant(text(""))];

        assert_eq!(repair(&messages), Some(vec![user("在吗")]));
    }

    #[test]
    fn a_call_without_its_result_goes_with_its_text() {
        let asked = assistant(call("a"));

        assert_eq!(
            repair(&[user("在吗"), asked]),
            Some(vec![user("在吗")]),
            "half a pair is refused as surely as an empty message"
        );
    }

    #[test]
    fn a_result_without_its_call_is_dropped() {
        let messages = vec![user("在吗"), result("a")];

        assert_eq!(repair(&messages), Some(vec![user("在吗")]));
    }

    #[test]
    fn a_pair_survives() {
        let messages = vec![user("在吗"), assistant(call("a")), result("a")];

        assert_eq!(repair(&messages), None);
    }

    #[test]
    fn reasoning_alone_is_something_to_send() {
        let messages = vec![
            user("在吗"),
            assistant(vec![Content::thinking("想一想")]),
            assistant(text("在。")),
        ];

        assert_eq!(repair(&messages), None);
    }

    #[test]
    fn one_unanswered_call_takes_the_answered_ones_with_it() {
        let mixed = assistant(vec![
            Content::tool_call("a", "say", json!({})),
            Content::tool_call("b", "say", json!({})),
        ]);

        assert_eq!(
            repair(&[user("在吗"), mixed, result("a")]),
            Some(vec![user("在吗")]),
            "no provider takes a call whose result is missing"
        );
    }
}
