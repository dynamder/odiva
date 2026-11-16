//! The lanes: one activation, one context, one toolset.
//!
//! An activation is built, run to a boundary, written to disk, and dropped. It
//! owns the agent — context and toolset — for exactly as long as it runs, which
//! is why a restart is not a recovery: there is never any state to recover.
//!
//! Two lanes share this type. Idle runs on the talking lane, which is the public
//! record. Busy runs on a work lane rooted at a handoff, and while it does, the
//! public record still receives the person's lines and her `say`s — the work
//! lane is where the orchestration lives, the talking lane is what anyone can
//! read.

use odiva_proto::{Side, WireMessage};
use yoagent::Agent;
use yoagent::types::{AgentMessage, Content, Message, StopReason};

use crate::error::Result;
use crate::handoff::Handoff;
use crate::roster::Roster;
use crate::route::Route;
use crate::say::Said;
use crate::store::{Lane, Log, Store};

/// A conversation with its history loaded and its roster installed.
pub struct Talk {
    agent: Agent,
    store: Store,
    said: Said,
    lane: Lane,
    log: Log,
    /// The talking lane, when this activation runs somewhere else.
    public: Option<(Lane, Log)>,
}

impl Talk {
    /// Opens the talking lane, starting the next generation when the prompt
    /// under which it was opened is no longer this one.
    pub fn open(
        store: &Store,
        route: &dyn Route,
        prompt: &str,
        roster: Roster,
        said: Said,
    ) -> Result<Self> {
        let (lane, log) = store.open(Lane::Talk { generation: 0 }, prompt)?;
        let agent = assemble(route, prompt, roster, &log);

        Ok(Self {
            agent,
            store: store.clone(),
            said,
            lane,
            log,
            public: None,
        })
    }

    /// Opens a busy period's lane, rooted at a handoff.
    ///
    /// A fresh work lane starts from what the conversation was about and what is
    /// still out there; it never inherits the talking lane's prefix, so the
    /// orchestration cannot drift into the conversation.
    pub fn busy(
        store: &Store,
        generation: u32,
        route: &dyn Route,
        prompt: &str,
        roster: Roster,
        said: Said,
        jobs: Vec<String>,
    ) -> Result<Self> {
        let (public_lane, public) = store.open(Lane::Talk { generation: 0 }, prompt)?;
        let lane = Lane::Work { generation };
        let mut log = store.exactly(lane, prompt)?;

        if store.read(lane)?.is_none() {
            log.push_note(Side::User, Handoff::of(&public, &jobs).render());
            store.write(lane, &log)?;
        }

        let agent = assemble(route, prompt, roster, &log);

        Ok(Self {
            agent,
            store: store.clone(),
            said,
            lane,
            log,
            public: Some((public_lane, public)),
        })
    }

    pub fn lane(&self) -> Lane {
        self.lane
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
    ///
    /// A turn that ends without a `say` still produced something, and that text
    /// is the difference between "she chose silence" and "the tool was ignored".
    pub fn monologue(&self) -> Option<String> {
        let said = self.agent.messages().iter().rev().find_map(|message| {
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
        })?;

        Some(said)
    }

    /// Records what the person said, before anything is run.
    ///
    /// Written immediately: a message the window showed must survive the turn
    /// that answered it.
    pub fn note(&mut self, message: &WireMessage) -> Result<()> {
        self.log.push(message.clone());
        self.store.write(self.lane, &self.log)?;

        if let Some((lane, log)) = &mut self.public {
            log.push(message.clone());
            self.store.write(*lane, log)?;
        }

        Ok(())
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

    /// Keeps everything she said during the turn, closed cleanly or not.
    pub fn flush(&mut self) -> Result<()> {
        for message in self.said.drain() {
            self.log.push(message.clone());

            if let Some((lane, log)) = &mut self.public {
                log.push(message);
                self.store.write(*lane, log)?;
            }
        }

        self.store.write(self.lane, &self.log)
    }

    /// How big this lane's context was, in the provider's own count.
    pub fn used(&self) -> u64 {
        let usage = self.agent.total_usage();
        usage.input + usage.cache_read + usage.cache_write + usage.output
    }

    /// Rotates the talking lane when it has outgrown itself.
    ///
    /// The old file stays where it is; the new one starts with a residue and the
    /// last few exchanges, so what follows is a conversation that remembers what
    /// it was about and nothing else. Work lanes are one period each and are not
    /// rotated under themselves.
    pub fn compact(&mut self, keep: usize) -> Result<()> {
        let Lane::Talk { generation } = self.lane else {
            return Ok(());
        };

        let next = Lane::Talk {
            generation: generation + 1,
        };

        let mut log = Log::new(self.log.system());
        log.push_note(Side::User, residue(self.log.messages(), keep));

        for message in tail(self.log.messages(), keep) {
            log.push(message.clone());
        }

        self.store.write(next, &log)?;
        self.lane = next;
        self.log = log;

        Ok(())
    }
}

/// The last `keep` messages, oldest first.
fn tail(messages: &[WireMessage], keep: usize) -> &[WireMessage] {
    let start = messages.len().saturating_sub(keep);
    &messages[start..]
}

/// A few lines about what came before — the shape of a summary, not a summary.
fn residue(messages: &[WireMessage], keep: usize) -> String {
    let start = messages.len().saturating_sub(keep);
    let dropped = &messages[..start];

    if dropped.is_empty() {
        return "【压缩】前面没有落下什么。".to_owned();
    }

    let mut text = format!("【压缩】前面 {} 条的大意：\n", dropped.len());

    for message in dropped.iter().rev().take(4).rev() {
        let line = message
            .markdown
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("");

        let side = match message.from {
            Side::User => "对方",
            Side::Assistant => "你",
        };

        text.push_str(&format!("- {side}：{}\n", line.chars().take(80).collect::<String>()));
    }

    text
}

fn assemble(route: &dyn Route, prompt: &str, roster: Roster, log: &Log) -> Agent {
    roster.install(
        route
            .agent()
            .with_system_prompt(prompt)
            .with_messages(log.history())
            .without_context_management(),
    )
}
