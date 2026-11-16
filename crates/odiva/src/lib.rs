//! The chat window.
//!
//! This crate is the *view* in front of `odiva-core`. It holds no truth about the
//! conversation: every message on screen arrived through [`App::apply`], and
//! everything the person does leaves through the outbox.
//!
//! It does hold *layout* state — measured row heights, the scroll offset, whether
//! the view is following the newest message. That state is not part of the
//! contract, it is what a window knows about its own pixels, and it is rebuilt
//! from the snapshot on the next attach.

pub mod model;
pub mod style;
pub mod theme;
pub mod time;
pub mod view;

use chrono::Local;
use ericed::list::{Measured, Scroll, Slice, Slots, measured};
use iced::widget::{operation, scrollable};
use iced::{Element, Size, Task as Command, Theme};

use odiva_proto::{CoreMessage, MessageId, Presence, Snapshot, UiMessage};

use crate::style::column_width;

/// The transcript's scrollable, so a task can address it.
pub const SHEET: &str = "odiva-transcript";

/// The height a message is assumed to have before it has reported one.
///
/// One short bubble plus its gap. Being wrong is fine — the value is a cache
/// entry, and `Measured` replaces it the first time the message lays out.
const ESTIMATE: f32 = 96.0;

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
    /// The person tapped a link in a message.
    UserOpenedLink(String),
    /// The core said something. The only way the transcript can change.
    CoreReported(CoreMessage),

    /// The transcript moved: wheel, drag, or a scroll we asked for.
    TranscriptScrolled(scrollable::Viewport),
    /// The transcript's viewport got a size. The first one matters: without it
    /// the window cannot tell "short conversation, sit at the bottom" from
    /// "long conversation, stay at the top".
    TranscriptResized(Size),
    /// One message laid itself out. Its height is the cache's next entry.
    MessageMeasured { slot: usize, size: Size },
}

pub struct App {
    messages: Vec<model::Msg>,
    /// Row heights, index-aligned with `messages`. Owned here because `Measured`
    /// cannot grow, and rebuilding it must not forget what the old rows cost.
    heights: Vec<Option<f32>>,
    slots: Measured,
    scroll: Scroll,
    draft: String,
    presence: Presence,
    /// The theme this window draws itself in. It lives here because the palette
    /// has to be known while *building* the element tree — iced's markdown widget
    /// takes colours as an argument, not as a style closure.
    theme: Theme,
    /// A pinned clock, for tests. `None` means "ask the system".
    clock: Option<i64>,
    /// Whether the view follows the newest message.
    pinned: bool,
    /// What the window has asked the core to do and the core has not been given
    /// yet. A `Vec` and not a channel because there is no core in this crate
    /// yet; the running window drains it into one.
    outbox: Vec<UiMessage>,
}

impl App {
    /// Builds the window from a snapshot. The only way in.
    pub fn attached(snapshot: Snapshot) -> Self {
        let messages: Vec<_> = snapshot
            .messages
            .into_iter()
            .enumerate()
            .map(|(slot, wire)| model::Msg::complete(slot, wire))
            .collect();

        let mut app = Self {
            heights: vec![None; messages.len()],
            slots: measured(ESTIMATE, messages.len()).overscan(2),
            scroll: Scroll::new(),
            messages,
            draft: String::new(),
            presence: snapshot.presence,
            theme: theme::dark(),
            clock: None,
            pinned: true,
            outbox: Vec::new(),
        };

        // A snapshot arrives whole, so nothing else will estimate these rows.
        // Without this the first frame is all `ESTIMATE`s and the total — and so
        // the bottom alignment — is wrong until the sensors report.
        for slot in 0..app.messages.len() {
            app.estimate(slot);
        }

        app
    }

    /// An empty window, for a core that has nothing to say yet.
    pub fn empty() -> Self {
        Self::attached(Snapshot::default())
    }

    /// Draws in another theme.
    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Freezes "now", so a screenshot is the same picture every run.
    pub fn with_clock(mut self, now_ms: i64) -> Self {
        self.clock = Some(now_ms);
        self
    }

    /// Tells the window how tall the transcript is before it lays anything out.
    ///
    /// A running window learns this from a `sensor` on its first frame, but the
    /// *decision* it feeds — short conversation sits at the bottom, long one
    /// stays at the top — has to be right on the very first frame, or the first
    /// paint is the wrong one. Tests and the screenshot harness say it up front.
    pub fn with_viewport(mut self, height: f32) -> Self {
        self.scroll = Scroll::at(0.0, height.max(0.0));
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
    pub fn apply(&mut self, message: CoreMessage) -> Command<Message> {
        match message {
            CoreMessage::CoreAttachedSession { snapshot } => {
                *self = Self::attached(snapshot)
                    .with_theme(self.theme.clone())
                    .with_clock(self.now_ms())
                    .with_viewport(self.scroll.height());

                return self.follow();
            }
            CoreMessage::MessageAppended { message } => {
                let slot = self.messages.len();
                self.messages.push(model::Msg::complete(slot, message));
                self.heights.push(None);
                self.grow_slots();
                self.estimate(slot);
            }
            CoreMessage::MessageBegan { id, sent, from } => {
                let slot = self.messages.len();
                self.messages.push(model::Msg::began(slot, id, sent, from));
                self.heights.push(None);
                self.grow_slots();
                self.estimate(slot);
            }
            CoreMessage::MessageStreamedText { id, delta } => {
                if let Some(slot) = self.message_mut(id).map(|msg| {
                    msg.append(&delta);
                    msg.slot
                }) {
                    // The row is taller than it was: its old height is stale, and
                    // a stale height is worse than a fresh estimate.
                    self.estimate(slot);
                }
            }
            CoreMessage::MessageAttachedFile { id, file } => {
                if let Some(slot) = self.message_mut(id).map(|msg| {
                    msg.files.push(file);
                    msg.slot
                }) {
                    self.estimate(slot);
                }
            }
            CoreMessage::MessageOfferedReplies { id, replies } => {
                if let Some(slot) = self.message_mut(id).map(|msg| {
                    msg.replies = replies;
                    msg.slot
                }) {
                    self.estimate(slot);
                }
            }
            CoreMessage::MessageFinished { id } => {
                if let Some(msg) = self.message_mut(id) {
                    msg.streaming = false;
                }
            }
            CoreMessage::PresenceChanged { presence } => self.presence = presence,
        }

        // A reply that grew while the person was at the bottom should keep them
        // there; one that grew while they were reading history must not move the
        // page under their eyes.
        if self.pinned {
            self.follow()
        } else {
            Command::none()
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
                    return self.follow();
                }
            }
            Message::UserRequestedStop => self.outbox.push(UiMessage::UserRequestedStop),
            // A tapped reply is the person saying that sentence. The core does
            // not need to know whether it was typed or tapped, so it is the same
            // message.
            Message::UserTappedReply(text) => {
                self.outbox.push(UiMessage::UserSubmittedText { text });
                self.pinned = true;
                return self.follow();
            }
            Message::UserJumpedToNewest => {
                self.pinned = true;
                return self.follow();
            }
            Message::UserOpenedLink(url) => {
                self.outbox.push(UiMessage::UserSubmittedText { text: url });
            }
            Message::CoreReported(message) => return self.apply(message),

            Message::TranscriptResized(size) => {
                self.scroll.resized(size.height);

                // The viewport only becomes known here. Until it is, `follow`
                // deliberately does nothing — a target computed against a height
                // of zero is a scroll position nobody asked for.
                if self.pinned {
                    return self.follow();
                }
            }
            Message::TranscriptScrolled(viewport) => {
                self.scroll.update(&viewport);

                // Reading the bottom means following it; anywhere else means the
                // person is looking at something, and new text waits.
                self.pinned = self.scroll.y() + self.scroll.height() >= self.total() - 2.0;
            }
            Message::MessageMeasured { slot, size } => {
                if let Some(height) = self.heights.get_mut(slot) {
                    *height = Some(size.height);
                }

                if self.slots.measure(slot, size) && self.pinned {
                    return self.follow();
                }
            }
        }

        Command::none()
    }

    /// Takes what the window has said since the last call.
    pub fn take_outbox(&mut self) -> Vec<UiMessage> {
        std::mem::take(&mut self.outbox)
    }

    pub fn messages(&self) -> &[model::Msg] {
        &self.messages
    }

    /// The slots this frame should build, and the padding that keeps the content
    /// height equal to the sum of the rows.
    pub fn slice(&self) -> Slice {
        self.slots.slice(&self.scroll)
    }

    /// How tall the whole transcript is: measured rows plus estimates.
    pub fn total(&self) -> f32 {
        self.slots.total()
    }

    pub fn scroll(&self) -> Scroll {
        self.scroll
    }

    /// How much empty space to put above the transcript so a short conversation
    /// sits on the composer.
    ///
    /// This is the bottom alignment the window could not do before: the padding
    /// needs both numbers, and a virtualised list is where they finally exist.
    /// It only appears when there is nothing to scroll, so it cannot fight the
    /// scroll offset.
    pub fn headroom(&self) -> f32 {
        (self.scroll.height() - self.total()).max(0.0)
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
    pub fn is_streaming(&self) -> bool {
        self.messages.last().is_some_and(|msg| msg.streaming)
    }

    /// What "now" is, for time labels.
    pub fn now_ms(&self) -> i64 {
        self.clock
            .unwrap_or_else(|| Local::now().timestamp_millis())
    }

    pub fn view(&self) -> Element<'_, Message> {
        view::window(self)
    }

    pub fn theme(&self) -> Theme {
        self.theme.clone()
    }

    /// Scrolls to the bottom, saying so, so `Scroll`'s copy stays honest.
    fn follow(&mut self) -> Command<Message> {
        // Before the first layout the viewport height is zero, and "bottom" is
        // not a position yet. Following now would park the transcript at a
        // distance from the top that means nothing.
        if self.scroll.height() <= 0.0 {
            return Command::none();
        }

        let target = (self.total() - self.scroll.height()).max(0.0);

        operation::scroll_to(SHEET, self.scroll.to(target))
    }

    /// Rebuilds the slot table when the row count changed, restoring the heights
    /// already known. `Measured` has no way to grow, and losing the cache would
    /// move every row above the viewport back to its estimate.
    fn grow_slots(&mut self) {
        if self.slots.len() == self.heights.len() {
            return;
        }

        self.slots = measured(ESTIMATE, self.heights.len()).overscan(2);

        for (slot, height) in self.heights.iter().enumerate() {
            if let Some(height) = height {
                self.slots
                    .measure(slot, Size::new(column_width(), *height));
            }
        }
    }

    /// Puts this row's best known height into the slot table.
    ///
    /// Called whenever a row's *content* changes: a new message, or a streamed
    /// reply growing. The row's sensor will overwrite it with the truth a frame
    /// later; until then the estimate is what keeps the total — and therefore
    /// the scroll position and the bottom padding — from being nonsense.
    fn estimate(&mut self, slot: usize) {
        let Some(msg) = self.messages.get(slot) else {
            return;
        };

        let previous = slot
            .checked_sub(1)
            .and_then(|before| self.messages.get(before))
            .map(|before| before.sent);

        // The divider belongs to the row: it is built inside it, so it is part of
        // its height.
        let divider = if time::needs_divider(previous, msg.sent) {
            32.0
        } else {
            0.0
        };

        let height = msg.estimated_height() + divider;

        if let Some(cached) = self.heights.get_mut(slot) {
            *cached = Some(height);
        }

        self.slots.measure(slot, Size::new(column_width(), height));
    }

    fn message_mut(&mut self, id: MessageId) -> Option<&mut model::Msg> {
        self.messages.iter_mut().find(|msg| msg.id == id)
    }
}
