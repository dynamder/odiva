//! One activation: a line, a toolset, and one turn.
//!
//! An activation is built, run to a boundary, written back, and dropped. The
//! line is the record; the agent is what reads it for exactly as long as one
//! turn lasts, which is why a restart is not a recovery — there is never any
//! state to recover.

use odiva_proto::WireMessage;
use yoagent::Agent;
use yoagent::types::{AgentMessage, Content, Message, StopReason};

use crate::error::Result;
use crate::line::{Line, LineId};
use crate::roster::Roster;
use crate::route::Route;
use crate::say::Said;
use crate::store::Store;
use crate::transcript::Transcript;

/// A conversation with its history loaded and its roster installed.
pub struct Talk {
    agent: Agent,
    line: Line,
    said: Said,
    transcript: Transcript,
}

impl Talk {
    /// Opens a line and installs the roster on it.
    ///
    /// `seed` is what the core says into a line before its first turn — a
    /// handoff. It is only used when the line has never carried a message.
    pub fn open(
        store: &Store,
        id: LineId,
        route: &dyn Route,
        prompt: &str,
        roster: Roster,
        said: Said,
        seed: Option<String>,
    ) -> Result<Self> {
        let mut line = Line::open(store, id, prompt)?;

        if line.is_fresh()
            && let Some(seed) = seed
        {
            line.push_note(seed)?;
        }

        let agent = assemble(route, prompt, roster, &line.messages());

        Ok(Self {
            agent,
            line,
            said,
            transcript: Transcript::new(store.transcript()),
        })
    }

    pub fn id(&self) -> &LineId {
        self.line.id()
    }

    pub fn generation(&self) -> u32 {
        self.line.generation()
    }

    /// What the provider said when the turn went wrong, if it did.
    ///
    /// A provider that fails mid-stream ends the turn normally, with an empty
    /// assistant message and the reason in `error_message`. Without this, a
    /// missing key looks exactly like a model with nothing to say.
    pub fn failure(&self) -> Option<String> {
        self.agent.messages().iter().rev().find_map(|message| {
            let AgentMessage::Llm(Message::Assistant {
                error_message,
                stop_reason,
                ..
            }) = message
            else {
                return None;
            };

            match (error_message, stop_reason) {
                (Some(error), _) => Some(error.clone()),
                (None, StopReason::Error) => Some("模型返回了一个错误".to_owned()),
                _ => None,
            }
        })
    }

    /// Whether she said anything during this activation.
    pub fn spoke(&self) -> bool {
        !self.said.is_empty()
    }

    /// What the model said in its own voice, tools aside.
    pub fn monologue(&self) -> Option<String> {
        self.agent.messages().iter().rev().find_map(|message| {
            let AgentMessage::Llm(Message::Assistant { content, .. }) = message else {
                return None;
            };

            let text: String = content
                .iter()
                .filter_map(|block| match block {
                    Content::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();

            (!text.trim().is_empty()).then_some(text)
        })
    }

    /// Records what the person said in the window's record, before anything
    /// runs: a message the window showed must survive the turn that answered it.
    pub fn note(&mut self, message: &WireMessage) -> Result<()> {
        self.transcript.append(message)
    }

    /// Runs one turn. What the model does, it does through its tools.
    ///
    /// Dropping the returned future aborts the run, which is how the person's
    /// "stop" reaches a model that is already answering.
    pub async fn run(&mut self, prompt: &str) -> Result<()> {
        let (events, mut stream) = tokio::sync::mpsc::unbounded_channel();
        let drained = tokio::spawn(async move { while stream.recv().await.is_some() {} });

        self.agent.prompt_with_sender(prompt, events).await;

        drained.await.ok();
        Ok(())
    }

    /// Note, run, keep: one turn, for callers with nothing to cancel.
    pub async fn turn(&mut self, message: &WireMessage) -> Result<()> {
        self.note(message)?;
        self.run(&message.markdown).await?;
        self.flush()
    }

    /// Keeps everything this turn produced, closed cleanly or not: what she
    /// said goes to the record, everything else to the line.
    pub fn flush(&mut self) -> Result<()> {
        for message in self.said.drain() {
            self.transcript.append(&message)?;
        }

        self.line.absorb(self.agent.messages())
    }

    /// How big this lane's context was, in the provider's own count.
    pub fn used(&self) -> u64 {
        let usage = self.agent.total_usage();
        usage.input + usage.cache_read + usage.cache_write + usage.output
    }

    /// Rotates the line when it has outgrown itself. The old file stays where
    /// it is; the new one starts with a residue and the last few exchanges.
    pub fn compact(&mut self, keep: usize) -> Result<()> {
        self.line.compact(keep)
    }
}

fn assemble(route: &dyn Route, prompt: &str, roster: Roster, messages: &[AgentMessage]) -> Agent {
    roster.install(
        route
            .agent()
            .with_system_prompt(prompt)
            .with_messages(messages.to_vec())
            .without_context_management(),
    )
}
