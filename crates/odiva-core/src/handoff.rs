//! What survives the conversation: a small, structured summary of where things
//! stand, which is what a busy period starts from.
//!
//! The four fields are the point. "总结一下" produces paragraphs, and paragraphs
//! grow; a struct that has to be filled produces exactly four things, and the
//! cost of three percent holds because there is nowhere else for the text to go.

use serde::{Deserialize, Serialize};

use crate::store::Log;

/// The smallest true thing that can be said about a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Handoff {
    pub topic: String,
    pub intent: String,
    pub commitments: Vec<String>,
    pub open: Vec<String>,
}

impl Handoff {
    /// Reads the public record and whatever work is still out.
    pub fn of(log: &Log, jobs: &[String]) -> Self {
        let last = log
            .messages()
            .iter()
            .rev()
            .find(|message| message.from == odiva_proto::Side::User);

        let topic = last
            .map(|message| first_line(&message.markdown, 80))
            .unwrap_or_else(|| "还没说什么".to_owned());

        let intent = last
            .map(|message| shorten(&message.markdown, 400))
            .unwrap_or_default();

        let commitments: Vec<String> = jobs.to_vec();
        let open = jobs
            .iter()
            .filter(|job| job.contains("等你说话"))
            .cloned()
            .collect();

        Self {
            topic,
            intent,
            commitments,
            open,
        }
    }

    /// The text a work lane starts with.
    pub fn render(&self) -> String {
        let mut text = format!("【交接】\n话题：{}\n", self.topic);

        if !self.intent.is_empty() {
            text.push_str(&format!("对方的原话：{}\n", self.intent));
        }

        if !self.commitments.is_empty() {
            text.push_str("\n手上的活：\n");
            for line in &self.commitments {
                text.push_str(&format!("- {line}\n"));
            }
        }

        if !self.open.is_empty() {
            text.push_str("\n在等人回话的：\n");
            for line in &self.open {
                text.push_str(&format!("- {line}\n"));
            }
        }

        text
    }
}

fn first_line(text: &str, limit: usize) -> String {
    let line = text.lines().find(|line| !line.trim().is_empty()).unwrap_or("");
    shorten(line, limit)
}

fn shorten(text: &str, limit: usize) -> String {
    let mut shortened: String = text.chars().take(limit).collect();

    if shortened.chars().count() < text.chars().count() {
        shortened.push('…');
    }

    shortened
}
