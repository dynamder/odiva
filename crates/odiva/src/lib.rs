//! The chat window.
//!
//! This crate is the *view* in front of `odiva-core`. It holds no truth: every
//! message on screen arrived through [`App::apply`], and everything the person
//! does leaves through the outbox. That is the whole contract, and it is what
//! lets the window be driven by a script in a test and by a socket later without
//! changing a line of drawing code.
//!
//! The window has no opinion about work, because it has no vocabulary for it:
//! see `odiva-proto`.

pub mod model;
pub mod style;
pub mod view;

use iced::{Element, Task as Command, Theme};

use odiva_proto::{CoreMessage, MessageId, Presence, Side, Snapshot, UiMessage, WireMessage};

use crate::model::Msg;

/// What the window can be told.
///
/// Named as a sentence — subject, verb, object — because the two directions of
/// this application (a person acting, and a core reporting) meet here, and a
/// variant called `Toggle` would say nothing about which.
#[derive(Debug, Clone)]
pub enum Message {
    /// The person typed.
    UserChangedDraft(String),
    /// The person pressed Enter, or the send button.
    UserSubmittedDraft,
    /// The person asked for the reply to stop.
    UserRequestedStop,
    /// The person tapped one of the offered replies.
    UserTappedReply(String),
    /// The person asked to go back to the newest message.
    UserJumpedToNewest,
    /// The person tapped a link in a message. Opening it is not this window's
    /// job yet, so it goes out to the core like any other intent.
    UserOpenedLink(String),
    /// The core said something. The only way the transcript can change.
    CoreReported(CoreMessage),
}

pub struct App {
    messages: Vec<Msg>,
    draft: String,
    presence: Presence,
    /// The theme this window draws itself in.
    ///
    /// It lives here because the palette has to be known while *building* the
    /// element tree — iced's markdown widget takes colours as an argument, not as
    /// a style closure — and a `view` function is not handed the theme.
    theme: Theme,
    /// Whether the view follows the newest message. `true` until the person
    /// scrolls away; new text then arrives without moving what they are reading.
    pinned: bool,
    /// What the window has asked the core to do and the core has not been given
    /// yet. A `Vec` and not a channel because there is no core in this crate
    /// yet; the running window drains it into one.
    outbox: Vec<UiMessage>,
}

impl App {
    /// Builds the window from a snapshot. The only way in.
    pub fn attached(snapshot: Snapshot) -> Self {
        Self {
            messages: snapshot.messages.into_iter().map(Msg::complete).collect(),
            draft: String::new(),
            presence: snapshot.presence,
            theme: Theme::CatppuccinMocha,
            pinned: true,
            outbox: Vec::new(),
        }
    }

    /// An empty window, for a core that has nothing to say yet.
    pub fn empty() -> Self {
        Self::attached(Snapshot::default())
    }

    /// Draws in another theme. One place decides both the iced theme and the
    /// palette the markdown widget is handed, or the two drift.
    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Whether the view follows the newest message.
    pub fn following(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }

    /// The only door for the core.
    ///
    /// Exhaustive over `CoreMessage`: adding a variant upstream breaks this
    /// build rather than silently dropping a message.
    pub fn apply(&mut self, message: CoreMessage) {
        match message {
            CoreMessage::CoreAttachedSession { snapshot } => {
                self.messages = snapshot.messages.into_iter().map(Msg::complete).collect();
                self.presence = snapshot.presence;
                self.pinned = true;
            }
            CoreMessage::MessageAppended { message } => {
                self.messages.push(Msg::complete(message));
            }
            CoreMessage::MessageBegan { id, from } => {
                self.messages.push(Msg::began(id, from));
            }
            CoreMessage::MessageStreamedText { id, delta } => {
                if let Some(msg) = self.message_mut(id) {
                    msg.append(&delta);
                }
            }
            CoreMessage::MessageAttachedFile { id, file } => {
                if let Some(msg) = self.message_mut(id) {
                    msg.files.push(file);
                }
            }
            CoreMessage::MessageOfferedReplies { id, replies } => {
                if let Some(msg) = self.message_mut(id) {
                    msg.replies = replies;
                }
            }
            CoreMessage::MessageFinished { id } => {
                if let Some(msg) = self.message_mut(id) {
                    msg.streaming = false;
                }
            }
            CoreMessage::PresenceChanged { presence } => self.presence = presence,
        }
    }

    pub fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::UserChangedDraft(text) => self.draft = text,
            Message::UserSubmittedDraft => {
                let text = std::mem::take(&mut self.draft);

                if !text.trim().is_empty() {
                    // No optimistic echo: the transcript is the core's, and it
                    // echoes the line back as a `MessageAppended`. In-process,
                    // that round trip is a channel send.
                    self.outbox.push(UiMessage::UserSubmittedText { text });
                    self.pinned = true;
                }
            }
            Message::UserRequestedStop => self.outbox.push(UiMessage::UserRequestedStop),
            // A tapped reply is the person saying that sentence. The core does
            // not need to know whether it was typed or tapped, so it is the same
            // message.
            Message::UserTappedReply(text) => {
                self.outbox.push(UiMessage::UserSubmittedText { text });
                self.pinned = true;
            }
            Message::UserJumpedToNewest => self.pinned = true,
            Message::UserOpenedLink(url) => {
                self.outbox.push(UiMessage::UserSubmittedText { text: url });
            }
            Message::CoreReported(message) => self.apply(message),
        }

        Command::none()
    }

    /// Takes what the window has said since the last call.
    pub fn take_outbox(&mut self) -> Vec<UiMessage> {
        std::mem::take(&mut self.outbox)
    }

    pub fn messages(&self) -> &[Msg] {
        &self.messages
    }

    pub fn draft(&self) -> &str {
        &self.draft
    }

    pub fn presence(&self) -> Presence {
        self.presence
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned
    }

    /// Whether the last message is still arriving.
    ///
    /// Read from the message, not from a counter: a reply that failed to send a
    /// `MessageFinished` shows here, which is exactly where a stuck caret should
    /// be visible.
    pub fn is_streaming(&self) -> bool {
        self.messages.last().is_some_and(|msg| msg.streaming)
    }

    pub fn view(&self) -> Element<'_, Message> {
        view::window(self)
    }

    pub fn theme(&self) -> Theme {
        self.theme.clone()
    }

    fn message_mut(&mut self, id: MessageId) -> Option<&mut Msg> {
        self.messages.iter_mut().find(|msg| msg.id == id)
    }
}

/// Convenience for the tests and the demo core: a completed message.
pub fn said(id: MessageId, from: Side, markdown: impl Into<String>) -> WireMessage {
    WireMessage::new(id, from, markdown)
}
