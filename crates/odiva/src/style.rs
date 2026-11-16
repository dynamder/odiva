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
    /// A message. Which corner is sharp says who sent it.
    Bubble(Side),
    /// An excerpt inside a message: darker than the bubble, so it reads as
    /// quoted material rather than as more of the same voice.
    Quote,
}

impl Surface {
    fn radius(self) -> Radius {
        let round = 14.0;
        let tail = 5.0;

        match self {
            Self::Shell | Self::Header | Self::Footer => 0.0.into(),
            // The corner nearest the sender is sharp — the one thing that makes
            // two same-coloured rectangles read as a conversation.
            Self::Bubble(Side::User) => Radius {
                top_left: round,
                top_right: tail,
                bottom_right: round,
                bottom_left: round,
            },
            Self::Bubble(Side::Assistant) => Radius {
                top_left: tail,
                top_right: round,
                bottom_right: round,
                bottom_left: round,
            },
            Self::Quote => 8.0.into(),
        }
    }
}

impl ContainerRecipe for Surface {
    fn frame(&self) -> Frame {
        let (padding, width, height, clip) = match self {
            Self::Shell => (Padding::ZERO, Length::Fill, Length::Fill, true),
            Self::Header => (
                Padding::new(10.0).horizontal(20.0),
                Length::Fill,
                Length::Shrink,
                false,
            ),
            Self::Footer => (
                Padding::new(12.0).horizontal(20.0),
                Length::Fill,
                Length::Shrink,
                false,
            ),
            // `Shrink` on both axes: a bubble is as big as what it says. The cap
            // on how wide it may get is a layout decision and lives in the view.
            Self::Bubble(_) => (
                Padding::new(9.0).horizontal(13.0),
                Length::Shrink,
                Length::Shrink,
                false,
            ),
            // A quote is sized by its own text: it is a chip of evidence inside
            // the bubble, and a `Fill` child of a `Shrink` parent resolves to
            // zero width — the bubble would collapse around it.
            Self::Quote => (
                Padding::new(8.0).horizontal(10.0),
                Length::Shrink,
                Length::Shrink,
                false,
            ),
        };

        Frame {
            padding,
            width,
            height,
            clip,
        }
    }

    fn paint(&self, tokens: &Tokens) -> container::Style {
        let (fill, text, border) = match self {
            Self::Shell | Self::Header | Self::Footer => {
                (Some(tokens.canvas), tokens.text, Border::default())
            }
            // Tinted for the person, neutral for the assistant: the same
            // convention every messenger uses, and it costs no extra ink.
            Self::Bubble(Side::User) => (
                Some(tokens.surface_selected),
                tokens.text,
                Border {
                    color: iced::Color::TRANSPARENT,
                    width: 0.0,
                    radius: self.radius(),
                },
            ),
            Self::Bubble(Side::Assistant) => (
                Some(tokens.surface),
                tokens.text,
                Border {
                    color: iced::Color::TRANSPARENT,
                    width: 0.0,
                    radius: self.radius(),
                },
            ),
            Self::Quote => (
                Some(tokens.canvas),
                tokens.text_muted,
                Border {
                    color: tokens.border,
                    width: 1.0,
                    radius: self.radius(),
                },
            ),
        };

        container::Style {
            text_color: Some(text),
            background: fill.map(Background::Color),
            border,
            shadow: Shadow::default(),
            snap: false,
        }
    }
}

// ── Type ────────────────────────────────────────────────────────────────────

/// A piece of text, named by its job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Type {
    /// The contact's name; a chip's label.
    Strong,
    Body,
    Muted,
    /// Ages, keyboard hints, the time divider.
    Caption,
    /// The status line at the foot of a bubble. Smaller than prose because it is
    /// about the message, not part of it.
    Meta,
    /// Code and excerpts.
    Mono,
    /// A status that is fine: the dot, "进行中".
    Accent,
    /// A status that needs a person.
    Warn,
    Error,
}

impl TextRecipe for Type {
    fn size(&self) -> f32 {
        match self {
            Self::Strong => 14.0,
            Self::Body | Self::Muted => 13.5,
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
    /// Anything secondary: "先看 diff", "停止".
    Quiet,
}

impl ButtonRecipe for Act {
    fn paint(&self, tokens: &Tokens, status: button::Status) -> button::Style {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);

        let (fill, text, border) = match self {
            Self::Primary => (
                tokens.accent,
                tokens.on_accent,
                Border {
                    color: iced::Color::TRANSPARENT,
                    width: 0.0,
                    radius: 9.0.into(),
                },
            ),
            Self::Chip => (
                if hovered {
                    tokens.surface_selected
                } else {
                    tokens.surface_hover
                },
                tokens.text,
                Border {
                    color: tokens.border,
                    width: 1.0,
                    radius: 13.0.into(),
                },
            ),
            Self::Quiet => (
                if hovered {
                    tokens.surface_cursor
                } else {
                    tokens.surface
                },
                tokens.text_muted,
                Border {
                    color: tokens.border,
                    width: 1.0,
                    radius: 9.0.into(),
                },
            ),
        };

        button::Style {
            background: Some(Background::Color(fill)),
            text_color: text,
            border,
            shadow: Shadow::default(),
            snap: false,
        }
    }
}

// ── Rules ───────────────────────────────────────────────────────────────────

/// The hairlines that separate the three bands of the window.
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
    4.0
}

/// The gap between two blocks inside one message.
pub fn block_gap() -> f32 {
    8.0
}

/// How wide the transcript's column is.
///
/// Fixed, not `Fill`: a line of prose that runs the whole width of a maximised
/// window is unreadable, and centring the column is what every chat client
/// converges on for the same reason.
pub fn column_width() -> f32 {
    700.0
}

/// How wide one bubble may get.
///
/// A bubble that spans the column is no longer a bubble — the ragged edge is most
/// of what makes a conversation scannable.
pub fn bubble_width() -> f32 {
    470.0
}

/// How wide a bubble may get when it carries a block-level item.
///
/// Code blocks and tables need the room, and a code block that wraps is worse
/// than a wide bubble. Still short of the column: a message that touches both
/// margins stops reading as a message.
pub fn wide_bubble_width() -> f32 {
    column_width() - 60.0
}
