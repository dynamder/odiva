//! Whether the person has said something the running turn has not seen yet.
//!
//! A message that arrives mid-turn waits its turn. What it also does is make one
//! thing true: someone is waiting. The turn hears that as a note before its next
//! model request — not as a tool, and not as an interruption.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use yoagent::types::{TurnContext, TurnHook};

#[derive(Debug, Clone, Default)]
pub struct Told {
    waiting: Arc<AtomicBool>,
}

impl Told {
    pub fn tell(&self) {
        self.waiting.store(true, Ordering::SeqCst);
    }

    /// Whether anyone was waiting, and forgets it: the note is said once.
    pub fn take(&self) -> bool {
        self.waiting.swap(false, Ordering::SeqCst)
    }
}

/// The hook that turns "someone is waiting" into a sentence in the request.
pub struct Note {
    told: Told,
}

impl Note {
    pub fn new(told: Told) -> Self {
        Self { told }
    }
}

#[async_trait::async_trait]
impl TurnHook for Note {
    async fn before_turn(&self, _turn: &TurnContext<'_>) -> Option<String> {
        self.told.take().then(|| {
            "（对方刚说了话，在等你。想停下来收个尾就停，不要硬撑着做完。）".to_owned()
        })
    }
}
