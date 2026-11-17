//! What survives a conversation: a small note about where things stood, which
//! is what a busy period starts from and what idle is told when it comes back.
//!
//! It is written by her, at the moment she accepts a task, not derived from the
//! last message: "what I was doing" is a judgment, and a judgment needs a place
//! in the tool call where it can be made.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Handoff {
    /// One or two sentences: what the idle line was on, and what it meant to do.
    pub note: String,
}

impl Handoff {
    /// The text the busy line starts with, and idle is handed back.
    pub fn render(&self) -> String {
        format!("【交接】\n{}\n", self.note.trim())
    }
}
