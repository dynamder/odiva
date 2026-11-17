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
use ericed::list::{Measured, Scroll, Slice, Slots, bottom, measured};
use iced::widget::operation;
use iced::{Element, Size, Subscription, Task as Command, Theme};
use tokio::sync::mpsc::UnboundedReceiver;

use odiva_proto::{Answer, CoreHandle, CoreMessage, Presence, Question, Snapshot, UiMessage};

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
    /// The person picked an option that answers on the spot.
    QuestionChose(usize),
    /// The person picked an option that wants a line of explanation.
    QuestionOpened(usize),
    /// The person typed that line.
    QuestionTyped(String),
    /// The person sent the answer.
    QuestionAnswered,
    /// The person asked to go back to the newest message.
    UserJumpedToNewest,
    /// The person tapped a link in a message.
    UserOpenedLink(String),
    /// The core said something. The only way the transcript can change.
    CoreReported(CoreMessage),
    /// Where the transcript really is: what the viewport reported, not what the
    /// height table predicts.
    TranscriptScrolled(Scroll),
    /// The transcript's viewport got a size. Without the first one the window
    /// cannot tell "short conversation, sit at the bottom" from "long
    /// conversation, stay at the top".
    TranscriptResized(Size),
    /// One message laid itself out. Its height is the cache's next entry.
    MessageMeasured { slot: usize, size: Size },
}

impl Message {
    /// A name for the trace, without the payload.
    fn label(&self) -> &'static str {
        match self {
            Self::UserChangedDraft(_) => "draft",
            Self::UserSubmittedDraft => "submit",
            Self::UserRequestedStop => "stop",
            Self::UserTappedReply(_) => "tapped",
            Self::QuestionChose(_) => "chose",
            Self::QuestionOpened(_) => "opened",
            Self::QuestionTyped(_) => "typed",
            Self::QuestionAnswered => "answered",
            Self::UserJumpedToNewest => "jumped",
            Self::UserOpenedLink(_) => "link",
            Self::CoreReported(_) => "core",
            Self::TranscriptScrolled(_) => "scrolled",
            Self::TranscriptResized(_) => "resized",
            Self::MessageMeasured { .. } => "measured",
        }
    }
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
    /// A decision nobody can type: while this is set, the composer is off.
    question: Option<Question>,
    /// Which option of that question is asking for its one line.
    opening: Option<usize>,
    /// What the person has typed into it so far.
    answer: String,
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
            question: None,
            opening: None,
            answer: String::new(),
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
        self.scroll = Scroll::at(0.0, height.max(0.0), 0.0);
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
            CoreMessage::QuestionAsked { question } => {
                self.question = Some(question);
                self.opening = None;
                self.answer.clear();
            }
            CoreMessage::QuestionClosed { .. } => {
                self.question = None;
                self.opening = None;
                self.answer.clear();
            }
            CoreMessage::CoreFailed { note } => self.notice = Some(note),
        }

        if self.pinned {
            self.follow()
        } else {
            Command::none()
        }
    }

    /// Takes one message and answers with the work it caused.
    ///
    /// Every message is also a line of the trace (`ODIVA_LOG=debug`): the four
    /// numbers that decide what the transcript shows, so a list that moves when
    /// nobody touched it says which of them moved.
    pub fn update(&mut self, message: Message) -> Command<Message> {
        let label = message.label();
        let command = self.handle(message);

        log::debug!(
            "{label}: y {:.1} height {:.1} content {:.1} total {:.1} pinned {} rows {}..{}",
            self.scroll.y(),
            self.scroll.height(),
            self.scroll.content(),
            self.total(),
            self.pinned,
            self.slice().range().start,
            self.slice().range().end
        );

        command
    }

    fn handle(&mut self, message: Message) -> Command<Message> {
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
            Message::QuestionChose(option) => {
                self.wire
                    .send(UiMessage::AnswerQuestion(Answer { option, note: None }));
                self.question = None;
                self.opening = None;
            }
            Message::QuestionOpened(option) => self.opening = Some(option),
            Message::QuestionTyped(text) => self.answer = text,
            Message::QuestionAnswered => {
                if let Some(option) = self.opening {
                    let note = (!self.answer.trim().is_empty()).then(|| std::mem::take(&mut self.answer));

                    self.wire.send(UiMessage::AnswerQuestion(Answer { option, note }));
                    self.question = None;
                    self.opening = None;
                }
            }
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
            Message::TranscriptScrolled(told) => {
                self.scroll = told;
                self.pinned = told.at_end();
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
    /// height in step with the rows.
    pub fn slice(&self) -> Slice {
        self.slots.slice_at(self.window_top(), self.scroll.height())
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

    /// The decision the window is waiting on, if any.
    pub fn question(&self) -> Option<&Question> {
        self.question.as_ref()
    }

    /// Which option of it is asking for a line.
    pub fn opening(&self) -> Option<usize> {
        self.opening
    }

    /// What the person typed into that line.
    pub fn answer(&self) -> &str {
        &self.answer
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

    /// Scrolls to the bottom by asking iced for the end, rather than by
    /// computing where the end is.
    ///
    /// The table's total is not the content's height: rows that were never built
    /// are estimates. An absolute target derived from it lands a few pixels off,
    /// iced clamps it somewhere else, the window reports that place back, and the
    /// two numbers take turns — which is a list that shivers at the bottom.
    /// `snap_to` is resolved by iced against what it laid out, so there is only
    /// one number, and it stays the end while the content grows.
    ///
    /// `Scrollable::anchor_bottom()` is unrelated: it moves the thumb when there
    /// is nothing to scroll, and never moves the content.
    fn follow(&mut self) -> Command<Message> {
        if self.scroll.height() <= 0.0 {
            return Command::none();
        }

        operation::snap_to(SHEET, bottom())
    }

    /// Where this frame's window starts, in the table's own coordinates.
    ///
    /// While following, the window is the end of the table rather than the last
    /// place the viewport was seen at. A message that was just appended lives
    /// there, so it is built, measured and drawn in the same frame; the offset
    /// iced resolves for it only arrives a frame later, and a frame of blank
    /// where a new message should be is a flash.
    fn window_top(&self) -> f32 {
        if self.pinned {
            (self.total() - self.scroll.height()).max(0.0)
        } else {
            self.scroll.y()
        }
    }

    /// The space the view puts above the row at `slot`.
    ///
    /// One rule, read by both the view and the estimate. The estimate has to
    /// include it, or every row is short by the gap and the list ends before its
    /// content does — which is exactly what "scrolled to the bottom and the last
    /// message is cut" looks like.
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

    /// Puts this row's best known height into the slot table.
    ///
    /// Called whenever a row's *content* changes: the row's sensor overwrites it
    /// with the truth a frame later, and until then the estimate keeps the total —
    /// and therefore the padding and the scrollbar — from being nonsense.
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
