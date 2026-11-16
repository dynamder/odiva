//! A toolset, and whatever makes it change while she works.

use std::sync::Arc;

use yoagent::Agent;
use yoagent::ToolSource;
use yoagent::types::AgentTool;

/// The tools an activation runs with.
///
/// A roster is also the state: idle and busy differ by what is in here, and by
/// nothing else, which is why "switching state" is not a third thing the core
/// has to do.
pub struct Roster {
    tools: Vec<Box<dyn AgentTool>>,
    source: Option<Arc<dyn ToolSource>>,
}

impl Roster {
    pub fn of(tools: Vec<Box<dyn AgentTool>>) -> Self {
        Self {
            tools,
            source: None,
        }
    }

    /// Adds a source for tools that appear and disappear mid-run — the only way
    /// a roster changes without a new activation.
    #[must_use]
    pub fn sourcing(mut self, source: Arc<dyn ToolSource>) -> Self {
        self.source = Some(source);
        self
    }

    pub fn install(self, agent: Agent) -> Agent {
        let agent = agent.with_tools(self.tools);

        match self.source {
            Some(source) => agent.with_tool_source(source),
            None => agent,
        }
    }
}
