//! The visual language of the chat window.
//!
//! Same shape as `sofaia`'s: a *kind* names what a widget is, `ericed`'s recipe
//! traits say how it paints, and the kind lives here because "a message from the
//! user" only means something in this application.
//!
//! The palette is entirely [`Tokens`] — not one raw colour in a view. That is
//! what makes the light theme a rendering of the same description rather than a
//! second design.

use iced::border::Radius;
use iced::widget::{button, container, rule, text};
use iced::{Background, Border, Length, Padding, Shadow};

use ericed::style::{ButtonRecipe, ContainerRecipe, Frame, RuleRecipe, TextRecipe, Tokens};
use odiva_proto::Side;

// ── Surfaces ────────────────────────────────────────────────────────────────

/// Where a message sits in its speaker's run.
///
/// Consecutive messages from one side are one group, and the group's outer
/// corners are the only ones allowed to be sharp. Three bubbles each carrying
/// their own tail reads as three separate thoughts, which is not what three
/// sentences are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Shape {
    /// The only message in its group.
    Alone,
    First,
    Middle,
    Last,
}

impl Shape {
    fn starts(self) -> bool {
        matches!(self, Self::Alone | Self::First)
    }

    fn ends(self) -> bool {
        matches!(self, Self::Alone | Self::Last)
    }

    /// Works out where a message sits, from whether its neighbours are on the
    /// same side.
    pub fn of(same_before: bool, same_after: bool) -> Self {
        match (same_before, same_after) {
            (false, false) => Self::Alone,
            (false, true) => Self::First,
            (true, true) => Self::Middle,
            (true, false) => Self::Last,
        }
    }
}

/// A box. Every framed region in the window is one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Surface {
    /// The window itself.
    Shell,
    /// The contact line above the transcript. No fill: the transcript scrolls
    /// under it, and a filled bar would read as a separate panel.
    Header,
    /// The composer's frame, below a hairline.
    Footer,
    /// A message.
    Bubble(Side, Shape),
    /// An excerpt inside a message: darker than the bubble, so it reads as
    /// quoted material rather than as more of the same voice.
    Quote,
    /// A code block. Same plane as a quote, without the outline: inside a bubble
    /// a border on both the bubble and the block is a frame inside a frame.
    Code,
    /// The bar that shows a message is still arriving.
    Caret,
}

impl Surface {
    fn radius(self) -> Radius {
        let round = 14.0;
        let tail = 5.0;
        let joint = 8.0;

        match self {
            Self::Shell | Self::Header | Self::Footer => 0.0.into(),
            Self::Bubble(side, shape) => {
                let mut radius = Radius::from(round);

                // The sender's own edge is the one that bends: the corner at the
                // start of a group is the tail, the ones between messages in a
                // group are only slightly cut so the stack reads as one breath.
                match side {
                    Side::User => {
                        radius.top_right = if shape.starts() { tail } else { joint };
                        radius.bottom_right = if shape.ends() { round } else { joint };
                    }
                    Side::Assistant => {
                        radius.top_left = if shape.starts() { tail } else { joint };
                        radius.bottom_left = if shape.ends() { round } else { joint };
                    }
                }

                radius
            }
            Self::Quote => 8.0.into(),
            Self::Code => 6.0.into(),
            Self::Caret => 1.0.into(),
        }
    }
}

impl ContainerRecipe for Surface {
    fn frame(&self) -> Frame {
        let (padding, width, height, clip) = match self {
            Self::Shell => (Padding::ZERO, Length::Fill, Length::Fill, true),
            // The bands carry vertical padding only: their contents are centred
            // in one column by the view, so a horizontal inset here would fight
            // the column and put the name somewhere the bubbles are not.
            Self::Header => (
                Padding::new(11.0).horizontal(0.0),
                Length::Fill,
                Length::Shrink,
                false,
            ),
            Self::Footer => (
                Padding::new(12.0).horizontal(0.0),
                Length::Fill,
                Length::Shrink,
                false,
            ),
            // `Shrink` on both axes: a bubble is as big as what it says. The cap
            // on how wide it may get is a layout decision and lives in the view.
            Self::Bubble(..) => (
                Padding::new(9.0).horizontal(13.0),
                Length::Shrink,
                Length::Shrink,
                false,
            ),
            Self::Quote => (
                Padding::new(8.0).horizontal(10.0),
                Length::Shrink,
                Length::Shrink,
                false,
            ),
            Self::Code => (
                Padding::new(10.0).horizontal(12.0),
                Length::Shrink,
                Length::Shrink,
                false,
            ),
            Self::Caret => (Padding::ZERO, Length::Fixed(2.0), Length::Fixed(15.0), false),
        };

        Frame {
            padding,
            width,
            height,
            clip,
        }
    }

    fn paint(&self, tokens: &Tokens) -> container::Style {
        let plain = |fill, text| container::Style {
            text_color: Some(text),
            background: Some(Background::Color(fill)),
            border: Border {
                color: iced::Color::TRANSPARENT,
                width: 0.0,
                radius: self.radius(),
            },
            shadow: Shadow::default(),
            snap: false,
        };

        match self {
            Self::Shell | Self::Header | Self::Footer => plain(tokens.canvas, tokens.text),
            // Tinted for the person, neutral for the assistant: the same
            // convention every messenger uses, and it costs no extra ink.
            Self::Bubble(Side::User, _) => plain(tokens.surface_selected, tokens.text),
            Self::Bubble(Side::Assistant, _) => plain(tokens.surface, tokens.text),
            Self::Caret => plain(tokens.accent, tokens.accent),
            Self::Code => plain(tokens.canvas, tokens.text),
            Self::Quote => container::Style {
                text_color: Some(tokens.text_muted),
                background: Some(Background::Color(tokens.canvas)),
                border: Border {
                    color: tokens.border,
                    width: 1.0,
                    radius: self.radius(),
                },
                shadow: Shadow::default(),
                snap: false,
            },
        }
    }
}

// ── Type ────────────────────────────────────────────────────────────────────

/// A piece of text, named by its job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Type {
    /// The contact's name.
    Strong,
    /// Prose. 14 rather than 13.5: this window is read in Chinese half the time,
    /// and CJK at 13.5 in a proportional face is where the strokes start to
    /// clog.
    Body,
    Muted,
    /// Ages, the time divider.
    Caption,
    /// A file's size. Smaller than prose because it is about the message.
    Meta,
    /// Code and excerpts.
    Mono,
    /// A status that is fine: "在线".
    Accent,
    /// A status that needs a person.
    Warn,
    Error,
}

impl TextRecipe for Type {
    fn size(&self) -> f32 {
        match self {
            Self::Strong => 14.5,
            Self::Body | Self::Muted => 14.0,
            Self::Caption => 11.5,
            Self::Meta => 11.0,
            Self::Mono => 12.5,
            Self::Accent | Self::Warn | Self::Error => 11.5,
        }
    }

    fn paint(&self, tokens: &Tokens) -> text::Style {
        let color = match self {
            Self::Strong | Self::Body => tokens.text,
            Self::Muted | Self::Caption | Self::Meta | Self::Mono => tokens.text_muted,
            Self::Accent => tokens.accent,
            Self::Warn => tokens.warning_text,
            Self::Error => tokens.danger_text,
        };

        text::Style { color: Some(color) }
    }
}

// ── Buttons ─────────────────────────────────────────────────────────────────

/// A button, named by what pressing it does — not by what it looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Act {
    /// Send the draft; becomes the stop control while a reply is arriving.
    Primary,
    /// A reply chip inside a bubble: a pill, because it is an answer to a
    /// question rather than an action in the window.
    Chip,
    /// Anything secondary: "停止", the jump-to-newest button.
    Quiet,
}

impl ButtonRecipe for Act {
    fn paint(&self, tokens: &Tokens, status: button::Status) -> button::Style {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);

        let (fill, text, radius) = match self {
            Self::Primary => (
                if hovered { tokens.accent } else { tokens.accent },
                tokens.on_accent,
                9.0,
            ),
            Self::Chip => (
                if hovered {
                    tokens.surface_selected
                } else {
                    tokens.surface_hover
                },
                tokens.text,
                13.0,
            ),
            Self::Quiet => (
                if hovered {
                    tokens.surface_cursor
                } else {
                    tokens.surface
                },
                tokens.text_muted,
                9.0,
            ),
        };

        button::Style {
            background: Some(Background::Color(fill)),
            text_color: text,
            border: Border {
                color: match self {
                    Self::Primary => iced::Color::TRANSPARENT,
                    _ => tokens.border,
                },
                width: match self {
                    Self::Primary => 0.0,
                    _ => 1.0,
                },
                radius: radius.into(),
            },
            shadow: Shadow::default(),
            snap: false,
        }
    }
}

// ── Rules ───────────────────────────────────────────────────────────────────

/// The hairlines that separate the bands of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Line {
    Hairline,
}

impl RuleRecipe for Line {
    fn paint(&self, tokens: &Tokens) -> rule::Style {
        rule::Style {
            color: tokens.border,
            radius: 0.0.into(),
            fill_mode: rule::FillMode::Full,
            snap: true,
        }
    }
}

// ── Spacing ─────────────────────────────────────────────────────────────────

/// The gap between two messages from different sides.
pub fn side_gap() -> f32 {
    16.0
}

/// The gap between two messages from the same side.
///
/// Small on purpose: consecutive lines from one speaker belong together, and the
/// gap is what tells a reader they are a group.
pub fn same_side_gap() -> f32 {
    3.0
}

/// The gap between a message's body and whatever hangs under it (files, replies).
pub fn block_gap() -> f32 {
    8.0
}

/// How wide the transcript's column is.
///
/// Fixed, not `Fill`: a line of prose that runs the whole width of a maximised
/// window is unreadable, and centring one column is what every chat client
/// converges on for the same reason.
pub fn column_width() -> f32 {
    700.0
}

/// How wide one bubble may get.
///
/// A bubble that spans the column is no longer a bubble — the ragged edge is most
/// of what makes a conversation scannable. 470 at 14px is roughly 32 Han
/// characters, which is the width a person writes in one breath.
pub fn bubble_width() -> f32 {
    470.0
}

/// How wide a bubble may get when it carries a block-level item.
///
/// Code blocks and tables need the room, and a wrapped code block is worse than a
/// wide bubble. Still short of the column: a message that touches both margins
/// stops reading as a message.
pub fn wide_bubble_width() -> f32 {
    column_width() - 60.0
}
