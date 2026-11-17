//! The one thing that reaches into a turn from outside it.
//!
//! A kameo actor does not read its mailbox while it is handling a message, and a
//! turn can run for minutes. "Stop" therefore does not travel as a message: the
//! wire hands it to the token of the turn in progress.

use std::sync::{Arc, Mutex};

use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Default)]
pub struct TurnCancel {
    current: Arc<Mutex<Option<CancellationToken>>>,
}

impl TurnCancel {
    /// Marks this turn as the one a stop would cut short.
    pub fn begin(&self) -> CancellationToken {
        let token = CancellationToken::new();
        *self.current.lock().expect("no panic on the turn") = Some(token.clone());
        token
    }

    pub fn end(&self) {
        *self.current.lock().expect("no panic on the turn") = None;
    }

    /// Whether a turn is in flight, which is what makes a new message "mid-turn".
    pub fn running(&self) -> bool {
        self.current.lock().expect("no panic on the turn").is_some()
    }

    pub fn cancel(&self) {
        if let Some(token) = self.current.lock().expect("no panic on the turn").as_ref() {
            token.cancel();
        }
    }
}
