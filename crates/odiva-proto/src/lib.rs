//! The seam between the chat window and the core.
//!
//! One rule decides everything in this crate: **the window knows about messages
//! and nothing else.** There is no task here, no status, no progress, no
//! harness, no tool call. Those exist inside the core; what crosses this seam is
//! what the agent chose to *say*, after it decided to say it.
//!
//! That is what keeps the window from turning into a component of the harness.
//! The moment a `Task` appears in this file, the interface has opinions about
//! workflow, and it is a trace viewer with rounded corners.
//!
//! # Naming
//!
//! Both directions are enums whose variants read as a sentence — subject, verb,
//! object, none omitted — because a variant called `Turn(..)` cannot tell a
//! reader whether the turn started or ended. The type name carries the direction
//! ([`UiMessage`] is what the person did, [`CoreMessage`] is what happened); the
//! variant says what, in words.

#![forbid(unsafe_code)]

/// Identifies one message for the life of a session.
///
/// Monotonic per core, not per side: an id is handed out when the core decides
/// the message exists, which is always before the window hears of it.
pub type MessageId = u64;

/// Who a message came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    User,
    Assistant,
}

/// What the contact line says.
///
/// Two states, because a messenger has two. Anything more would be the interface
/// claiming to know what she is doing, which it does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Presence {
    Typing,
    #[default]
    Online,
}

/// A file attached to a message.
///
/// Deliberately inert: a name and a size, which is what a messenger shows. Where
/// the bytes live is the core's business.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileRef {
    pub name: String,
    pub size: String,
}

/// One message.
///
/// The body is markdown, not a structured block array. Which means "what a
/// message can contain" is exactly "what a chat application can contain" — and
/// the renderer's job is to read markdown, not to interpret an agent protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireMessage {
    pub id: MessageId,
    pub from: Side,
    pub markdown: String,
    pub files: Vec<FileRef>,
    /// Tappable replies offered by the sender. Pressing one is a message from
    /// the person; see [`UiMessage::UserSubmittedText`].
    pub replies: Vec<String>,
}

impl WireMessage {
    pub fn new(id: MessageId, from: Side, markdown: impl Into<String>) -> Self {
        Self {
            id,
            from,
            markdown: markdown.into(),
            files: Vec::new(),
            replies: Vec::new(),
        }
    }
}

/// Everything the window needs to draw itself from nothing.
///
/// The single rebuild entry point. A window that has just started, or has just
/// been reopened, is correct after one `CoreAttachedSession` and needs no other
/// mechanism — which is only true because the window holds no state of its own.
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
/// breaks its build instead of falling into a `_` arm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreMessage {
    /// The window may start drawing. Always the first message of a connection.
    CoreAttachedSession { snapshot: Snapshot },

    /// A complete message. Carries both sides: the person's own line is echoed
    /// back by the core, because the core owns the transcript.
    MessageAppended { message: WireMessage },

    /// A message that is still being written.
    MessageBegan { id: MessageId, from: Side },
    /// The next piece of it. The core coalesces tokens before sending; the
    /// window does not throttle.
    MessageStreamedText { id: MessageId, delta: String },
    MessageAttachedFile { id: MessageId, file: FileRef },
    MessageOfferedReplies { id: MessageId, replies: Vec<String> },
    /// The end of it. Sent on every path, including failure, or the window's
    /// "typing…" never stops.
    MessageFinished { id: MessageId },

    PresenceChanged { presence: Presence },
}

/// The window's way out.
///
/// A trait, not a channel, so the window can be driven by a script in tests and
/// by a socket later without either of them being a special case.
pub trait CoreHandle: Send + Sync + 'static {
    fn send(&self, message: UiMessage);
}
