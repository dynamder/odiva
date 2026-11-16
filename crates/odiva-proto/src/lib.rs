//! The seam between the chat window and the core.
//!
//! Only messages cross it: no task, no status, no progress, no harness, no tool
//! call. Those live in the core; what arrives here is what the agent decided to
//! say, after deciding to say it.
//!
//! Variants read as a sentence — subject, verb, object — because a variant named
//! `Turn(..)` cannot tell a reader whether the turn started or ended. The type
//! carries the direction ([`UiMessage`] is what the person did, [`CoreMessage`]
//! is what happened).

#![forbid(unsafe_code)]

/// Identifies one message. Handed out by the core, monotonic per core.
pub type MessageId = u64;

/// Who a message came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    User,
    Assistant,
}

/// What the contact line says. A messenger has two states, so this has two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Presence {
    Typing,
    #[default]
    Online,
}

/// A file attached to a message: the name and size a messenger shows. Where the
/// bytes live is the core's business.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileRef {
    pub name: String,
    pub size: String,
}

/// One message.
///
/// The body is markdown rather than a block array, so "what a message can
/// contain" is exactly "what a chat application can contain".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireMessage {
    pub id: MessageId,
    pub from: Side,
    /// Milliseconds since the Unix epoch, UTC. Formatting for the reader is the
    /// window's job.
    pub sent: i64,
    pub markdown: String,
    pub files: Vec<FileRef>,
    /// Tappable replies. Pressing one is [`UiMessage::UserSubmittedText`].
    pub replies: Vec<String>,
}

impl WireMessage {
    pub fn new(
        id: MessageId,
        sent: i64,
        from: Side,
        markdown: impl Into<String>,
    ) -> Self {
        Self {
            id,
            from,
            sent,
            markdown: markdown.into(),
            files: Vec::new(),
            replies: Vec::new(),
        }
    }
}

/// Everything the window needs to draw itself from nothing.
///
/// The single rebuild entry point: after one `CoreAttachedSession` the window is
/// correct, because it holds no state of its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub messages: Vec<WireMessage>,
    pub presence: Presence,
}

/// What the person did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiMessage {
    UserSubmittedText { text: String },
    UserRequestedStop,
}

/// What the core reports.
///
/// Flat on purpose: the window matches once, exhaustively, so a new variant
/// breaks its build instead of falling into a `_` arm. There is no variant for a
/// message in progress: speech is a tool the agent calls, and a tool call either
/// happened or did not, so a message arrives whole or not at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreMessage {
    /// Always the first message of a connection.
    CoreAttachedSession { snapshot: Snapshot },
    /// A complete message, files and replies included. The person's own line is
    /// echoed back here, because the core owns the transcript.
    MessageAppended { message: WireMessage },
    /// Whether the contact line reads "typing": the agent is working on
    /// something. Independent of messages, which arrive on their own schedule.
    PresenceChanged { presence: Presence },
}

/// The window's way out. A trait rather than a channel, so a script in tests and
/// a socket later are not special cases.
pub trait CoreHandle: Send + Sync + 'static {
    fn send(&self, message: UiMessage);
}
