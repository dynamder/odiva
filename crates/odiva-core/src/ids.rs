//! The message ids the window is given, in the order it is given them.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use odiva_proto::{MessageId, WireMessage};

/// A counter the core shares with whatever needs to stamp a message.
///
/// Handed out by cloning: every clone advances the same counter, so two
/// activations cannot hand out the same id.
#[derive(Debug, Clone, Default)]
pub struct Ids {
    next: Arc<AtomicU64>,
}

impl Ids {
    /// The id the next message gets.
    pub fn next(&self) -> MessageId {
        self.next.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// A message from her, stamped now.
    pub fn hers(&self, markdown: impl Into<String>) -> WireMessage {
        WireMessage::new(
            self.next(),
            chrono::Utc::now().timestamp_millis(),
            odiva_proto::Side::Assistant,
            markdown,
        )
    }

    /// A message from the person, stamped now.
    pub fn theirs(&self, markdown: impl Into<String>) -> WireMessage {
        WireMessage::new(
            self.next(),
            chrono::Utc::now().timestamp_millis(),
            odiva_proto::Side::User,
            markdown,
        )
    }
}
