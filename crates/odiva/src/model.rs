//! The window's own model: messages, already parsed.
//!
//! This is a *view* model. It exists because the renderer needs markdown as a
//! list of blocks, and parsing it once per message is cheaper than once per
//! frame. Nothing here is part of the contract — see `odiva-proto` for that.

use iced::widget::markdown;
use odiva_proto::{FileRef, MessageId, Side, WireMessage};

/// One message, ready to draw.
pub struct Msg {
    /// Where it sits in the transcript. The slot table is indexed by this, and
    /// the virtualised list builds only the slots a viewport can see.
    pub slot: usize,
    pub id: MessageId,
    pub from: Side,
    /// When it was said, UTC milliseconds.
    pub sent: i64,
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
    pub fn began(slot: usize, id: MessageId, sent: i64, from: Side) -> Self {
        Self {
            slot,
            id,
            from,
            sent,
            source: String::new(),
            items: Vec::new(),
            files: Vec::new(),
            replies: Vec::new(),
            streaming: true,
        }
    }

    /// A message that arrived whole.
    pub fn complete(slot: usize, message: WireMessage) -> Self {
        let items = markdown::parse(&message.markdown).collect();

        Self {
            slot,
            id: message.id,
            from: message.from,
            sent: message.sent,
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

    /// Whether this message carries something that has to span the bubble.
    ///
    /// iced's markdown renders code blocks and tables at `Length::Fill`, and a
    /// `Fill` child of a `Shrink` bubble resolves to zero width — the bubble
    /// would collapse around nothing.
    pub fn is_blocky(&self) -> bool {
        self.items.iter().any(|item| {
            matches!(
                item,
                markdown::Item::CodeBlock { .. } | markdown::Item::Table { .. }
            )
        })
    }

    /// A first guess at how tall this message will be.
    ///
    /// The exact number is only knowable by laying the text out, which is what
    /// the row's sensor reports a frame later. Until then the window needs
    /// *something*, because the scroll position and the bottom padding are
    /// arithmetic on the total: a constant guess makes the first frame jump when
    /// a code block turns out to be fifteen lines tall.
    ///
    /// So: characters per line at the bubble's measure, a line height, the
    /// bubble's padding, and the things that are not text.
    pub fn estimated_height(&self) -> f32 {
        const CHARS_PER_LINE: f32 = 30.0;
        const LINE: f32 = 21.0;
        const BUBBLE: f32 = 26.0;

        let visible = self
            .source
            .chars()
            .filter(|character| !character.is_whitespace())
            .count() as f32;

        let lines = (visible / CHARS_PER_LINE).ceil().max(1.0);
        // Fenced code is set tighter than prose and is not wrapped by the same
        // measure; a fenced block adds roughly a line of chrome.
        let fences = self.source.matches("```").count() as f32 / 2.0;

        let mut height = lines * LINE + BUBBLE + fences * LINE;

        height += self.files.len() as f32 * 44.0;
        height += if self.replies.is_empty() { 0.0 } else { 36.0 };

        height
    }
}
