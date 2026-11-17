//! Reading a line back, exactly as it was said.
//!
//! Compaction rewrites what the model sees, not what the line recorded: every
//! generation file stays, so a turn can ask for the words it no longer holds in
//! context. This is the only reader of those files.

use yoagent::types::{AgentMessage, Content, Message};

use crate::error::Result;
use crate::line::LineId;
use crate::store::Store;

/// What to look for. Every field is optional; the defaults are "everything,
/// newest first".
#[derive(Debug, Clone, Default)]
pub struct Query {
    /// First position in the line, oldest entry is 0.
    pub from: Option<usize>,
    /// Last position, inclusive.
    pub to: Option<usize>,
    /// Only entries whose text contains this.
    pub contains: Option<String>,
    /// How many to return; the newest of the matches.
    pub limit: Option<usize>,
}

/// One thing that was said, and where it lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub generation: u32,
    pub position: usize,
    pub side: &'static str,
    pub text: String,
}

/// Every entry of a line, across its generations, oldest first.
pub fn entries(store: &Store, id: &LineId) -> Result<Vec<Entry>> {
    let newest = store.newest_line(id).unwrap_or(0);
    let mut all = Vec::new();

    for generation in 0..=newest {
        let Some(session) = store.read_line(id, generation)? else {
            continue;
        };

        for (position, entry) in session.entries().iter().enumerate() {
            if let Some((side, text)) = spoken(&entry.message) {
                all.push(Entry {
                    generation,
                    position,
                    side,
                    text,
                });
            }
        }
    }

    Ok(all)
}

/// The matching entries, newest last, as the model reads them.
pub fn render(store: &Store, id: &LineId, query: &Query) -> Result<String> {
    let mut found: Vec<Entry> = entries(store, id)?
        .into_iter()
        .filter(|entry| {
            query.from.is_none_or(|from| entry.position >= from)
                && query.to.is_none_or(|to| entry.position <= to)
                && query
                    .contains
                    .as_ref()
                    .is_none_or(|needle| entry.text.contains(needle.as_str()))
        })
        .collect();

    if found.is_empty() {
        return Ok(format!("这条线里没有匹配的历史（{id}）。"));
    }

    let limit = query.limit.unwrap_or(20).max(1);
    let start = found.len().saturating_sub(limit);
    let total = found.len();

    found = found.split_off(start);

    let mut text = format!("{id}：{total} 条匹配，下面是最后 {}。\n", found.len());

    for entry in found {
        text.push_str(&format!(
            "- 第{}代 · {} · {}：{}\n",
            entry.generation,
            entry.position,
            entry.side,
            clip(&entry.text)
        ));
    }

    Ok(text)
}

fn clip(text: &str) -> String {
    let line: String = text.chars().take(400).collect();

    if line.chars().count() < text.chars().count() {
        format!("{line}…")
    } else {
        line.replace('\n', " ")
    }
}

fn spoken(message: &AgentMessage) -> Option<(&'static str, String)> {
    let AgentMessage::Llm(message) = message else {
        return None;
    };

    match message {
        Message::User { content, .. } => Some(("对方", plain(content))),
        Message::Assistant { content, .. } => {
            let text = plain(content);

            (!text.trim().is_empty()).then_some(("你", text))
        }
        Message::ToolResult {
            tool_name,
            content,
            is_error,
            ..
        } => {
            let text = plain(content);

            (!text.trim().is_empty())
                .then(|| ("工具", format!("{tool_name}{}：{text}", if *is_error { "（失败）" } else { "" })))
        }
    }
}

fn plain(content: &[Content]) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            Content::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}
