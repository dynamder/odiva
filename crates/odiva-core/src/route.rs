//! Which model answers, and where a conversation gets its brain.
//!
//! A route builds a fresh [`Agent`] each time it is asked, because an agent is
//! the context and the toolset — both of which belong to the activation, not to
//! a resident object. Tests substitute a script; nothing else has to change.

use std::collections::VecDeque;
use std::sync::Mutex;

use yoagent::Agent;
use yoagent::provider::ModelConfig;
use yoagent::provider::mock::{MockProvider, MockResponse};

/// Something that can hand out a brain.
pub trait Route: Send + Sync {
    fn agent(&self) -> Agent;
}

/// A real provider, chosen by its preset.
pub struct Model {
    config: ModelConfig,
}

impl Model {
    pub fn new(config: ModelConfig) -> Self {
        Self { config }
    }
}

impl Route for Model {
    fn agent(&self) -> Agent {
        Agent::from_config(self.config.clone())
    }
}

/// A route that answers from a script, one turn's worth at a time.
///
/// The unit is a turn, not a response, because an activation is built per turn:
/// the third activation gets the third script and nothing earlier.
pub struct Scripted {
    turns: Mutex<VecDeque<Vec<MockResponse>>>,
}

impl Scripted {
    pub fn new(turns: Vec<Vec<MockResponse>>) -> Self {
        Self {
            turns: Mutex::new(turns.into()),
        }
    }

    /// One activation, with this whole script.
    pub fn one(responses: Vec<MockResponse>) -> Self {
        Self::new(vec![responses])
    }
}

impl Route for Scripted {
    fn agent(&self) -> Agent {
        let script = self
            .turns
            .lock()
            .expect("no panic while scripting")
            .pop_front()
            .unwrap_or_default();

        Agent::from_provider(MockProvider::new(script), ModelConfig::mock())
    }
}
