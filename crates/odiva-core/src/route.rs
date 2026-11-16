//! Which model answers, and where a conversation gets its brain.
//!
//! A route builds a fresh [`Agent`] each time it is asked, because an agent is
//! the context and the toolset — both of which belong to the activation, not to
//! a resident object. Tests substitute a script; nothing else has to change.

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

/// A route that answers from a script, for tests.
pub struct Scripted {
    responses: Mutex<Vec<MockResponse>>,
}

impl Scripted {
    pub fn new(responses: Vec<MockResponse>) -> Self {
        Self {
            responses: Mutex::new(responses),
        }
    }

    /// One turn of plain text per line.
    pub fn texts(lines: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::new(
            lines
                .into_iter()
                .map(|line| MockResponse::Text(line.into()))
                .collect(),
        )
    }
}

impl Route for Scripted {
    fn agent(&self) -> Agent {
        let script = std::mem::take(&mut *self.responses.lock().expect("no panic while scripting"));

        Agent::from_provider(MockProvider::new(script), ModelConfig::mock())
    }
}
