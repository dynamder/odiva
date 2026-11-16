//! The window's own model: messages, already parsed.
//!
//! A *view* model: the renderer needs markdown as a list of blocks, and parsing
//! once per message is cheaper than once per frame. Nothing here is part of the
//! contract — that is `odiva-proto`.

use iced::widget::markdown;
use odiva_proto::{FileRef, MessageId, Side, WireMessage};

/// One message, ready to draw.
pub struct Msg {
    /// Where it sits in the transcript: the slot table is indexed by this, and
    /// only the slots a viewport can see are built.
    pub slot: usize,
    pub id: MessageId,
    pub from: Side,
    /// When it was said, UTC milliseconds.
    pub sent: i64,
    /// The markdown `items` came from. Kept because the height estimate is made
    /// from it.
    pub source: String,
    /// Parsed from `source`.
    pub items: Vec<markdown::Item>,
    pub files: Vec<FileRef>,
    pub replies: Vec<String>,
}

impl Msg {
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
        }
    }

    /// Whether this message carries something that has to span the bubble.
    ///
    /// iced's markdown renders code blocks and tables at `Length::Fill`, and a
    /// `Fill` child of a `Shrink` bubble resolves to zero width.
    pub fn is_blocky(&self) -> bool {
        self.items.iter().any(|item| {
            matches!(
                item,
                markdown::Item::CodeBlock { .. } | markdown::Item::Table { .. }
            )
        })
    }

    /// A first guess at how tall this message will be, until its `sensor`
    /// reports the truth a frame later.
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
        let fences = self.source.matches("```").count() as f32 / 2.0;

        let mut height = lines * LINE + BUBBLE + fences * LINE;

        height += self.files.len() as f32 * 44.0;
        height += if self.replies.is_empty() { 0.0 } else { 36.0 };

        height
    }
}
