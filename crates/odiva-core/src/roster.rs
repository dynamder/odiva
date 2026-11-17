//! A toolset, installed on the agent for exactly one activation.

use yoagent::Agent;
use yoagent::types::AgentTool;

pub struct Roster {
    tools: Vec<Box<dyn AgentTool>>,
}

impl Roster {
    pub fn of(tools: Vec<Box<dyn AgentTool>>) -> Self {
        Self { tools }
    }

    pub fn install(self, agent: Agent) -> Agent {
        agent.with_tools(self.tools)
    }
}
