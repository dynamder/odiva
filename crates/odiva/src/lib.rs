//! The chat window.
//!
//! This crate is the *view* in front of `odiva-core`. It holds no truth about the
//! conversation: every message on screen arrived through [`App::apply`], and
//! everything the person does leaves through the [`CoreHandle`] it was connected
//! to.
//!
//! It does hold *layout* state — measured row heights, the scroll offset, whether
//! the view follows the newest message. That is what a window knows about its own
//! pixels, and it is rebuilt from the snapshot on the next attach.

pub mod model;
pub mod style;
pub mod theme;
pub mod time;
pub mod view;
mod wire;

use chrono::Local;
use ericed::list::{Measured, Scroll, Slice, Slots, measured};
use iced::widget::{operation, scrollable};
use iced::{Element, Size, Subscription, Task as Command, Theme};
use tokio::sync::mpsc::UnboundedReceiver;

use odiva_proto::{CoreHandle, CoreMessage, Presence, Snapshot, UiMessage};

use crate::style::{column_width, same_side_gap, side_gap};
use crate::wire::Wire;

/// The transcript's scrollable, so a task can address it.
pub const SHEET: &str = "odiva-transcript";

/// The height a message is assumed to have before it has reported one.
const ESTIMATE: f32 = 96.0;

/// What the window can be told.
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
    TranscriptScrolled(scrollable::Viewport),
    /// The transcript's viewport got a size. Without the first one the window
    /// cannot tell "short conversation, sit at the bottom" from "long
    /// conversation, stay at the top".
    TranscriptResized(Size),
    /// One message laid itself out. Its height is the cache's next entry.
    MessageMeasured { slot: usize, size: Size },
}

pub struct App {
    messages: Vec<model::Msg>,
    slots: Measured,
    scroll: Scroll,
    draft: String,
    presence: Presence,
    /// iced's markdown widget takes colours as an argument rather than as a style
    /// closure, so the theme has to be known while building the element tree.
    theme: Theme,
    /// A pinned clock, for tests. `None` means "ask the system".
    clock: Option<i64>,
    pinned: bool,
    /// The core, when there is one. Both directions are optional so a fresh
    /// window can be built for a screenshot or a protocol test.
    wire: Wire,
    /// The last failure, shown until the next attempt.
    notice: Option<String>,
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
            slots: measured(ESTIMATE, messages.len()).overscan(2),
            scroll: Scroll::new(),
            messages,
            draft: String::new(),
            presence: snapshot.presence,
            theme: theme::dark(),
            clock: None,
            pinned: true,
            wire: Wire::default(),
            notice: None,
        };

        for slot in 0..app.messages.len() {
            app.estimate(slot);
        }

        app
    }

    /// An empty window, for a core that has nothing to say yet.
    pub fn empty() -> Self {
        Self::attached(Snapshot::default())
    }

    /// Connects the window to a core.
    pub fn connected(
        mut self,
        handle: impl CoreHandle,
        messages: UnboundedReceiver<CoreMessage>,
    ) -> Self {
        self.wire = Wire::connected(handle, messages);
        self
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
    /// decision it feeds — a short conversation sits at the bottom, a long one
    /// stays at the top — has to be right on the first paint.
    pub fn with_viewport(mut self, height: f32) -> Self {
        self.scroll = Scroll::at(0.0, height.max(0.0));
        self
    }

    /// Whether the view follows the newest message.
    pub fn following(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }

    fn with_wire(mut self, wire: Wire) -> Self {
        self.wire = wire;
        self
    }

    /// The only door for the core.
    pub fn apply(&mut self, message: CoreMessage) -> Command<Message> {
        match message {
            CoreMessage::CoreAttachedSession { snapshot } => {
                let notice = self.notice.clone();

                *self = Self::attached(snapshot)
                    .with_wire(self.wire.clone())
                    .with_theme(self.theme.clone())
                    .with_clock(self.now_ms())
                    .with_viewport(self.scroll.height());

                self.notice = notice;

                return self.follow();
            }
            CoreMessage::MessageAppended { message } => {
                let slot = self.messages.len();
                self.messages.push(model::Msg::complete(slot, message));
                self.slots.push();
                self.estimate(slot);
            }
            CoreMessage::PresenceChanged { presence } => self.presence = presence,
            CoreMessage::CoreFailed { note } => self.notice = Some(note),
        }

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
                    self.wire.send(UiMessage::UserSubmittedText { text });
                    self.notice = None;
                    self.pinned = true;
                    return self.follow();
                }
            }
            Message::UserRequestedStop => self.wire.send(UiMessage::UserRequestedStop),
            Message::UserTappedReply(text) => {
                self.wire.send(UiMessage::UserSubmittedText { text });
                self.notice = None;
                self.pinned = true;
                return self.follow();
            }
            Message::UserJumpedToNewest => {
                self.pinned = true;
                return self.follow();
            }
            Message::UserOpenedLink(url) => {
                self.wire.send(UiMessage::UserSubmittedText { text: url })
            }
            Message::CoreReported(message) => return self.apply(message),

            Message::TranscriptResized(size) => {
                self.scroll.resized(size.height);

                if self.pinned {
                    return self.follow();
                }
            }
            Message::TranscriptScrolled(viewport) => {
                self.scroll.update(&viewport);
                self.pinned = self.scroll.y() + self.scroll.height() >= self.total() - 2.0;
            }
            Message::MessageMeasured { slot, size } => {
                if self.slots.measure(slot, size) && self.pinned {
                    return self.follow();
                }
            }
        }

        Command::none()
    }

    /// Everything the core reports, as it reports it.
    pub fn subscription(&self) -> Subscription<Message> {
        self.wire.subscription()
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

    pub fn draft(&self) -> &str {
        &self.draft
    }

    pub fn presence(&self) -> Presence {
        self.presence
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned
    }

    /// Starts with something already wrong — a missing key, say — so the window
    /// says it before anyone types.
    #[must_use]
    pub fn with_notice(mut self, note: impl Into<String>) -> Self {
        self.notice = Some(note.into());
        self
    }

    /// The last thing that failed, until the next attempt replaces it.
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// Whether she is working on something, which is what the contact line and
    /// the stop control both read.
    pub fn is_working(&self) -> bool {
        self.presence == Presence::Typing
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

    /// Scrolls to the bottom, saying so, so [`Scroll`]'s copy stays honest.
    ///
    /// `Scrollable::anchor_bottom()` is unrelated: it moves the thumb when
    /// there is nothing to scroll, and never moves the content.
    fn follow(&mut self) -> Command<Message> {
        if self.scroll.height() <= 0.0 {
            return Command::none();
        }

        let target = (self.total() - self.scroll.height()).max(0.0);

        operation::scroll_to(SHEET, self.scroll.to(target))
    }

    /// Puts this row's best known height into the slot table.
    ///
    /// Called whenever a row's *content* changes: the row's sensor overwrites it
    /// with the truth a frame later, and until then the estimate keeps the total —
    /// and therefore the scroll position and the bottom padding — from being
    /// nonsense.
    /// The space the view puts above the row at `slot`.
    ///
    /// The estimate has to include it, or every row is short by the gap and the
    /// list ends before its content does — which is exactly what "scrolled to the
    /// bottom and the last message is cut" looks like.
    pub fn gap_at(&self, slot: usize) -> f32 {
        let same_side = slot > 0
            && self.messages.get(slot).is_some_and(|message| {
                self.messages
                    .get(slot - 1)
                    .is_some_and(|before| before.from == message.from)
            });

        if same_side {
            same_side_gap()
        } else {
            side_gap()
        }
    }

    fn estimate(&mut self, slot: usize) {
        let Some(msg) = self.messages.get(slot) else {
            return;
        };

        let previous = slot
            .checked_sub(1)
            .and_then(|before| self.messages.get(before))
            .map(|before| before.sent);

        let divider = if time::needs_divider(previous, msg.sent) {
            32.0
        } else {
            0.0
        };

        let height = msg.estimated_height() + divider + self.gap_at(slot);

        self.slots.measure(slot, Size::new(column_width(), height));
    }
}
