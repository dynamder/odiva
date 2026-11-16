//! The window's own model: messages, already parsed.
//!
//! This is a *view* model. It exists because the renderer needs markdown as a
//! list of blocks, and parsing it once per message is cheaper than once per
//! frame. Nothing here is part of the contract — see `odiva-proto` for that.

use iced::widget::markdown;
use odiva_proto::{FileRef, MessageId, Side, WireMessage};

/// One message, ready to draw.
pub struct Msg {
    pub id: MessageId,
    pub from: Side,
    /// The source, kept because streaming appends to it.
    pub source: String,
    /// Parsed from `source`. Re-parsed only when `source` changes.
    pub items: Vec<markdown::Item>,
    pub files: Vec<FileRef>,
    pub replies: Vec<String>,
    /// Whether the message is still being written.
    pub streaming: bool,
}

impl Msg {
    /// A message that has started but has no text yet.
    pub fn began(id: MessageId, from: Side) -> Self {
        Self {
            id,
            from,
            source: String::new(),
            items: Vec::new(),
            files: Vec::new(),
            replies: Vec::new(),
            streaming: true,
        }
    }

    /// A message that arrived whole.
    pub fn complete(message: WireMessage) -> Self {
        let items = markdown::parse(&message.markdown).collect();

        Self {
            id: message.id,
            from: message.from,
            source: message.markdown,
            items,
            files: message.files,
            replies: message.replies,
            streaming: false,
        }
    }

    /// Appends a streamed piece.
    ///
    /// The whole message is re-parsed, not the tail: a delta can open or close a
    /// fence, so the block structure is a function of the entire source. At chat
    /// sizes this is a few microseconds; if it ever matters, the fix is to
    /// re-parse only the last block and cache the rest.
    pub fn append(&mut self, delta: &str) {
        self.source.push_str(delta);
        self.items = markdown::parse(&self.source).collect();
    }
}
