//! The talking lane: one activation, one turn at a time.
//!
//! An activation is built, run to a boundary, written to disk, and dropped. It
//! owns the agent (context + toolset) for exactly as long as it runs, which is
//! why a restart is not a recovery: there is never state to recover.

use yoagent::Agent;
use yoagent::session::Session;
use yoagent::types::AgentTool;

use crate::error::Result;
use crate::route::Route;
use crate::store::{Lane, Store, TALK, root, rooted_with};

/// A conversation with its history loaded and its roster installed.
pub struct Talk {
    agent: Agent,
    session: Session,
    lane: Lane,
    store: Store,
}

impl Talk {
    /// Opens the talking lane, starting the next generation when the prompt
    /// under which it was opened is no longer this one.
    ///
    /// The prompt is part of the lane's identity: a session that ran under
    /// another prompt is another conversation, and mixing them would poison the
    /// cache for every turn that follows.
    pub fn open(
        store: &Store,
        route: &dyn Route,
        tools: Vec<Box<dyn AgentTool>>,
        prompt: &str,
    ) -> Result<Self> {
        let mut generation = store.newest(TALK);
        let mut session = store.open(Lane::Talk { generation })?;

        if session.entries().is_empty() {
            session.append(root(prompt));
        } else if rooted_with(&session).as_deref() != Some(prompt) {
            generation += 1;
            session = Session::new();
            session.append(root(prompt));
        }

        let agent = route
            .agent()
            .with_system_prompt(prompt)
            .with_tools(tools)
            .with_messages(session.path_messages())
            .without_context_management();

        Ok(Self {
            agent,
            session,
            lane: Lane::Talk { generation },
            store: store.clone(),
        })
    }

    pub fn lane(&self) -> Lane {
        self.lane
    }

    /// Runs one turn: what the model does, it does through its tools.
    ///
    /// Dropping the returned future aborts the run, which is the only way the
    /// person's "stop" reaches a model already answering.
    pub async fn ask(&mut self, text: &str) -> Result<()> {
        let (events, mut stream) = tokio::sync::mpsc::unbounded_channel();
        let drained = tokio::spawn(async move { while stream.recv().await.is_some() {} });

        self.agent.prompt_with_sender(text, events).await;

        drained.await.ok();
        self.session.append_new(self.agent.messages())?;
        self.store.write(self.lane, &self.session)?;

        Ok(())
    }
}
