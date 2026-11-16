//! The talking lane: one activation, one turn.
//!
//! An activation is built, run to a boundary, written to disk, and dropped. It
//! owns the agent — context and toolset — for exactly as long as it runs, which
//! is why a restart is not a recovery: there is never any state to recover.

use odiva_proto::WireMessage;
use yoagent::Agent;
use yoagent::types::AgentTool;

use crate::error::Result;
use crate::route::Route;
use crate::say::Said;
use crate::store::{Lane, Log, Store};

/// A conversation with its history loaded and its roster installed.
pub struct Talk {
    agent: Agent,
    log: Log,
    lane: Lane,
    store: Store,
    said: Said,
}

impl Talk {
    /// Opens the talking lane, starting the next generation when the prompt
    /// under which it was opened is no longer this one.
    pub fn open(
        store: &Store,
        route: &dyn Route,
        prompt: &str,
        tools: Vec<Box<dyn AgentTool>>,
        said: Said,
    ) -> Result<Self> {
        let (lane, log) = store.open(Lane::Talk { generation: 0 }, prompt)?;

        let agent = route
            .agent()
            .with_system_prompt(prompt)
            .with_tools(tools)
            .with_messages(log.history())
            .without_context_management();

        Ok(Self {
            agent,
            log,
            lane,
            store: store.clone(),
            said,
        })
    }

    pub fn lane(&self) -> Lane {
        self.lane
    }

    /// Records what the person said, before anything is run.
    ///
    /// Written immediately: a message the window showed must survive the turn
    /// that answered it.
    pub fn note(&mut self, message: &WireMessage) -> Result<()> {
        self.log.push(message.clone());
        self.store.write(self.lane, &self.log)
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
            self.log.push(message);
        }

        self.store.write(self.lane, &self.log)
    }
}
