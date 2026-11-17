//! Ending a run without another model request.
//!
//! When the person agrees to a pause, the tool result is already written, so the
//! history keeps its pairing. What must not happen is one more generation: this
//! extension stops the loop between the tool batch and the next request.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use yoagent::extension::{Extension, ExtensionError, RunContext, RunHooks, TurnDecision};
use yoagent::types::TurnContext;

/// A flag that means "no more model requests this turn".
#[derive(Debug, Clone, Default)]
pub struct Halt {
    flag: Arc<AtomicBool>,
}

impl Halt {
    pub fn stop(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn clear(&self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}

#[async_trait::async_trait]
impl Extension for Halt {
    fn name(&self) -> &str {
        "halt"
    }

    async fn start_run(&self, _run: &RunContext<'_>) -> Result<Box<dyn RunHooks>, ExtensionError> {
        Ok(Box::new(Stops {
            flag: Arc::clone(&self.flag),
        }))
    }
}

struct Stops {
    flag: Arc<AtomicBool>,
}

#[async_trait::async_trait]
impl RunHooks for Stops {
    async fn before_model(&mut self, _turn: &TurnContext<'_>) -> TurnDecision {
        if self.flag.load(Ordering::SeqCst) {
            TurnDecision::Stop("本轮到此为止".to_owned())
        } else {
            TurnDecision::Continue
        }
    }
}
