//! Context lines: one model context each, with its own generation history.
//!
//! A line is a `yoagent::Session` on disk: every entry carries an id, a parent,
//! and the full `AgentMessage` — tool calls and results included, because they
//! are what `view_history` has to be able to return later.

pub mod file;
pub mod id;

pub use id::{LineId, Mode};

use serde::{Deserialize, Serialize};
use yoagent::Session;
use yoagent::types::{AgentMessage, Content, CustomMessage, Message};

use crate::error::Result;
use crate::store::Store;

const ROOT: &str = "root";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Root {
    system: String,
}

pub struct Line {
    store: Store,
    id: LineId,
    generation: u32,
    system: String,
    session: Session,
    /// How many entries are already on disk. Everything past it is unsaved.
    written: usize,
}

impl Line {
    /// Opens a line, rotating to the next generation when the prompt under
    /// which it was opened is no longer this one: a different prompt is a
    /// different cache prefix, so the old file becomes history.
    pub fn open(store: &Store, id: LineId, system: &str) -> Result<Self> {
        let generation = store.newest_line(&id).unwrap_or(0);

        let Some(session) = store.read_line(&id, generation)? else {
            return Self::start(store, id, generation, system, None);
        };

        match system_of(&session) {
            Some(previous) if previous == system => {
                Ok(Self::install(store, id, generation, system, session))
            }
            _ => Self::start(store, id, generation + 1, system, Some(&session)),
        }
    }

    fn start(
        store: &Store,
        id: LineId,
        generation: u32,
        system: &str,
        carry: Option<&Session>,
    ) -> Result<Self> {
        let mut session = Session::new();
        session.append(root(system));

        for message in carry.map(Session::path_messages).unwrap_or_default().into_iter().skip(1) {
            session.append(message);
        }

        store.write_line(&id, generation, &session)?;
        Ok(Self::install(store, id, generation, system, session))
    }

    fn install(store: &Store, id: LineId, generation: u32, system: &str, session: Session) -> Self {
        let written = session.entries().len();

        Self {
            store: store.clone(),
            id,
            generation,
            system: system.to_owned(),
            session,
            written,
        }
    }

    pub fn id(&self) -> &LineId {
        &self.id
    }

    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// Whether this line has never carried a message.
    pub fn is_fresh(&self) -> bool {
        self.session.entries().len() <= 1
    }

    /// The conversation as the model reads it, root included.
    pub fn messages(&self) -> Vec<AgentMessage> {
        self.session.path_messages()
    }

    /// What the person said. Written down before anything runs: a message the
    /// window showed has to survive the turn that answered it.
    pub fn push_user(&mut self, text: &str) -> Result<()> {
        self.session.append(AgentMessage::Llm(Message::user(text)));
        self.persist_new()
    }

    /// A line the core writes about itself — a handoff, a residue.
    pub fn push_note(&mut self, text: impl Into<String>) -> Result<()> {
        self.session.append(AgentMessage::Llm(Message::user(text.into())));
        self.persist_new()
    }

    /// Keeps everything the run produced. The history must extend the path;
    /// anything else is a bug worth an error, not a quiet rewrite.
    pub fn absorb(&mut self, messages: &[AgentMessage]) -> Result<()> {
        self.session.append_new(messages)?;
        self.persist_new()
    }

    /// Rotates to the next generation: a residue, then the last `keep`
    /// messages. The old file stays where it is.
    /// Rotates to the next generation: a summary, then the last `keep`
    /// messages. The old file stays where it is. Without a summary, a
    /// deterministic residue stands in for one.
    pub fn compact(&mut self, keep: usize, summary: Option<String>) -> Result<()> {
        let messages = self.session.path_messages();
        let tail = messages.len().saturating_sub(keep).max(1);

        let summary = summary.unwrap_or_else(|| residue(&messages[1..tail]));

        let mut session = Session::new();
        session.append(root(&self.system));
        session.append(AgentMessage::Llm(Message::user(format!(
            "【压缩】\n{summary}"
        ))));

        for message in &messages[tail..] {
            session.append(message.clone());
        }

        self.generation += 1;
        self.store.write_line(&self.id, self.generation, &session)?;

        self.written = session.entries().len();
        self.session = session;

        Ok(())
    }

    /// What a compaction would drop.
    pub fn dropped(&self, keep: usize) -> Vec<AgentMessage> {
        let messages = self.session.path_messages();
        let tail = messages.len().saturating_sub(keep).max(1);

        messages[1..tail].to_vec()
    }

    fn persist_new(&mut self) -> Result<()> {
        let count = self.session.entries().len();

        if self.written < count {
            self.store
                .append_line(&self.id, self.generation, &self.session.entries()[self.written..])?;
            self.written = count;
        }

        Ok(())
    }
}

fn root(system: &str) -> AgentMessage {
    AgentMessage::Custom(CustomMessage::new(
        ROOT,
        Root {
            system: system.to_owned(),
        },
    ))
}

fn system_of(session: &Session) -> Option<String> {
    let AgentMessage::Custom(custom) = &session.entries().first()?.message else {
        return None;
    };

    custom.data.get("system")?.as_str().map(str::to_owned)
}

/// A few lines about what came before — the shape of a summary, not one.
fn residue(dropped: &[AgentMessage]) -> String {
    if dropped.is_empty() {
        return "【压缩】前面没有落下什么。".to_owned();
    }

    let mut text = format!("【压缩】前面 {} 条的大意：\n", dropped.len());
    let spoken: Vec<_> = dropped.iter().filter_map(spoken).collect();
    let tail = spoken.len().saturating_sub(4);

    for (side, said) in &spoken[tail..] {
        let line: String = said.chars().take(80).collect();
        text.push_str(&format!("- {side}：{line}\n"));
    }

    text
}

fn spoken(message: &AgentMessage) -> Option<(&'static str, String)> {
    let AgentMessage::Llm(message) = message else {
        return None;
    };

    let (side, content) = match message {
        Message::User { content, .. } => ("对方", content),
        Message::Assistant { content, .. } => ("你", content),
        _ => return None,
    };

    let text: String = content
        .iter()
        .filter_map(|block| match block {
            Content::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();

    (!text.trim().is_empty()).then_some((side, text))
}
