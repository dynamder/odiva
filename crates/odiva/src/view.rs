//! The window, drawn — as a conversation, not as a log.
//!
//! Three bands, top to bottom: the contact line, the transcript, the composer.
//! The transcript is the only thing that scrolls, and the only thing that is a
//! list — and it is a *windowed* list: only the messages a viewport can see are
//! built. A conversation is the one thing in this window that grows without
//! bound, so it is the one thing that cannot be built whole.
//!
//! Two conventions worth stating because they are easy to undo by accident:
//!
//! * **no harness vocabulary on screen.** No ids, no harness names, no tool
//!   calls, no progress bars, no "task" anything. Not because they are hidden —
//!   because the window has no such concepts to draw. Everything the agent wants
//!   to say, it says in words, in a message, at a moment it chose.
//! * **one column owns the window.** The contact line, every bubble and the
//!   composer are centred in the same measure. Anything that drifts off it — a
//!   name at the window's edge while the bubbles start 170px in — reads as an
//!   unfinished window, no matter how good the parts are.
//!
//! Consecutive messages from one side are a group: the gap inside a group is
//! 3px, the gap between sides is 16px, and only the group's own corner is sharp.
//! That is the whole of "who is speaking" — there is no label anywhere.

use iced::font::Weight;
use iced::widget::{
    button, column, container, markdown, row, rule, scrollable, sensor, space, text, text_input,
    Column, Stack,
};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Padding, Theme};

use ericed::style::{StyleExt, Tokens};
use odiva_proto::{FileRef, Presence, Side};

use crate::model::Msg;
use crate::style::{
    block_gap, bubble_width, column_width, same_side_gap, side_gap, wide_bubble_width, Act, Line,
    Shape, Surface, Type,
};
use crate::{App, Message, SHEET, time};

/// The whole window.
pub fn window(app: &App) -> Element<'_, Message> {
    let shell = column![
        header(app),
        rule::horizontal(1).styled(Line::Hairline),
        transcript(app),
        rule::horizontal(1).styled(Line::Hairline),
        composer(app),
    ]
    .width(Length::Fill)
    .height(Length::Fill);

    container(shell).styled(Surface::Shell).into()
}

/// Puts a band's contents into the one column the window is built on.
fn in_column<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    row![
        space::horizontal(),
        container(content).width(Length::Fixed(column_width())),
        space::horizontal(),
    ]
    .width(Length::Fill)
    .into()
}

// ── The contact line ────────────────────────────────────────────────────────

/// Who this is, and whether she is here.
///
/// Two states, the way a messenger has two. Anything more would be the interface
/// claiming to know what she is doing, which it does not.
fn header(app: &App) -> Element<'_, Message> {
    let (presence, tone) = match app.presence() {
        Presence::Typing => ("正在输入…", Type::Accent),
        Presence::Online => ("在线", Type::Caption),
    };

    let who = column![
        text("odiva")
            .font(bold())
            .styled(Type::Strong)
            .line_height(1.15),
        text(presence).styled(tone),
    ]
    .spacing(1);

    container(in_column(who.into()))
        .styled(Surface::Header)
        .into()
}

// ── Transcript ──────────────────────────────────────────────────────────────

fn transcript(app: &App) -> Element<'_, Message> {
    if app.messages().is_empty() {
        return empty();
    }

    let messages = app.messages();
    let window = app.slice();
    let now = app.now_ms();

    let mut items = Column::new().spacing(0.0).width(Length::Fill);

    for slot in window.range() {
        let msg = &messages[slot];

        // Where this message sits in its speaker's run. The gap is the group and
        // the corner is the group's own edge, so three sentences from one
        // speaker read as one breath instead of three remarks.
        let same_before = slot > 0 && messages[slot - 1].from == msg.from;
        let same_after = slot + 1 < messages.len() && messages[slot + 1].from == msg.from;
        let shape = Shape::of(same_before, same_after);

        let previous = slot.checked_sub(1).map(|before| messages[before].sent);
        let divider = time::needs_divider(previous, msg.sent).then(|| time::label(msg.sent, now));

        let gap = if same_before {
            same_side_gap()
        } else {
            side_gap()
        };

        let view = item_view(msg, shape, divider.as_deref(), gap, app.theme());

        // The row reports its own height, which is the only way a windowed list
        // can know where the next row starts.
        items = items.push(
            sensor(view).on_resize(move |size| Message::MessageMeasured { slot, size }),
        );
    }

    // `headroom` is the bottom alignment: a conversation shorter than the window
    // sits on the composer. It is zero whenever there is anything to scroll, so
    // it can never fight the scroll offset.
    let column_of_messages = column![
        space::vertical().height(app.headroom()),
        window.surround(items),
    ]
    .width(Length::Fill);

    let reading = scrollable(in_column(column_of_messages.into()))
        .id(SHEET)
        .on_scroll(Message::TranscriptScrolled)
        .height(Length::Fill)
        .width(Length::Fill);

    // The viewport's height is needed *before* anything scrolls: it is what
    // decides whether the transcript is short enough to sit at the bottom.
    let fitted = sensor(reading).on_resize(Message::TranscriptResized);

    if app.is_pinned() {
        return fitted.into();
    }

    let jump = container(
        button(text("↓").styled(Type::Body))
            .on_press(Message::UserJumpedToNewest)
            .padding([6.0, 13.0])
            .styled(Act::Quiet),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(Alignment::End)
    .align_y(Alignment::End)
    .padding(Padding::new(0.0).right(24.0).bottom(14.0));

    Stack::new().push(fitted).push(jump).into()
}

/// Nothing has been said yet.
///
/// A blank window is the first thing a new install shows, and it is the only
/// screen that has to explain itself without any content to lean on. One line of
/// what this is, one line of what to do — and no illustration standing in for
/// either.
fn empty<'a>() -> Element<'a, Message> {
    let hint = column![
        text("还没有对话").styled(Type::Strong).line_height(1.3),
        text("说点什么，或者把一件事交给我。").styled(Type::Caption),
    ]
    .spacing(4)
    .align_x(Alignment::Center);

    container(hint)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into()
}

/// A message, with the time divider that belongs above it.
fn item_view<'a>(
    msg: &'a Msg,
    shape: Shape,
    divider: Option<&str>,
    gap: f32,
    theme: Theme,
) -> Element<'a, Message> {
    let mut stack = Column::new().spacing(0.0).width(Length::Fill);

    if let Some(label) = divider {
        stack = stack.push(
            container(
                row![
                    space::horizontal(),
                    text(label.to_owned()).styled(Type::Caption),
                    space::horizontal(),
                ]
                .width(Length::Fill),
            )
            .padding(Padding::new(6.0).bottom(9.0)),
        );
    }

    stack = stack.push(message_view(msg, shape, theme));

    container(stack).padding(Padding::ZERO.top(gap)).into()
}

/// One message: a bubble, on the side of whoever sent it.
fn message_view<'a>(msg: &'a Msg, shape: Shape, theme: Theme) -> Element<'a, Message> {
    let blocky = msg.is_blocky();
    let mut body = Column::new().spacing(block_gap());

    if blocky {
        body = body.width(Length::Fill);
    }

    if !msg.items.is_empty() {
        body = body.push(markdown_view(&msg.items, &theme));
    }

    for file in &msg.files {
        body = body.push(file_view(file));
    }

    if !msg.replies.is_empty() {
        let mut replies = row![].spacing(6);

        for reply in &msg.replies {
            replies = replies.push(
                button(text(reply.clone()).styled(Type::Body))
                    .on_press(Message::UserTappedReply(reply.clone()))
                    .padding([5.0, 13.0])
                    .styled(Act::Chip),
            );
        }

        body = body.push(replies);
    }

    if msg.streaming {
        // A bar, not a glyph: a "▌" is a block of the fallback font at a size
        // nothing else uses, and it reads as a typo rather than a cursor.
        body = body.push(
            row![container(space::horizontal()).styled(Surface::Caret)]
                .padding(Padding::new(2.0).top(1.0)),
        );
    }

    // Width last, after the recipe: `StyleExt::styled` lays the frame out, and the
    // frame says `Shrink` for a bubble. A fixed width here is what gives a code
    // block something to fill — `max_width` alone leaves the child's limits at
    // zero, and the bubble collapses around nothing.
    let bubble = container(body).styled(Surface::Bubble(msg.from, shape));

    let bubble = if blocky {
        bubble.width(Length::Fixed(wide_bubble_width()))
    } else {
        bubble.max_width(bubble_width())
    };

    container(bubble)
        .width(Length::Fill)
        .align_x(match msg.from {
            Side::User => Alignment::End,
            Side::Assistant => Alignment::Start,
        })
        .into()
}

/// The message body.
///
/// iced's own markdown widget, dressed in this application's palette: inline
/// emphasis, links, inline code, code blocks, lists and tables are all its work,
/// not ours.
///
/// The palette is an *argument* here rather than a style closure, because that is
/// what the widget takes — which is why `App` holds a `Theme` at all. A `view`
/// function is handed no theme, and hard-coding one would make the light theme a
/// lie in exactly the place it is most visible: inline code.
fn markdown_view<'a>(items: &'a [markdown::Item], theme: &Theme) -> Element<'a, Message> {
    let tokens = Tokens::from_theme(theme);

    let style = markdown::Style {
        inline_code_highlight: markdown::Highlight {
            background: Background::Color(tokens.surface_hover),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 4.0.into(),
            },
        },
        inline_code_color: tokens.text,
        link_color: tokens.accent,
        ..markdown::Style::from(theme.clone())
    };

    // The body size this window uses everywhere; the widget derives heading and
    // code sizes from it.
    let settings = markdown::Settings::with_text_size(14.0, style);

    markdown::view_with(items, settings, &Viewer)
}

/// The window's markdown viewer.
///
/// Only `code_block` differs from iced's default, and it has to: the default
/// class is `container::dark` for *every* theme, which in the light theme is a
/// black block with near-black text and in the dark theme is a black block
/// inside a lighter bubble. Both are wrong for the same reason — the block should
/// be an inset of the window's own palette.
struct Viewer;

impl<'a> markdown::Viewer<'a, Message, Theme, iced::Renderer> for Viewer {
    fn on_link_click(url: markdown::Uri) -> Message {
        Message::UserOpenedLink(url)
    }

    fn code_block(
        &self,
        settings: markdown::Settings,
        _language: Option<&'a str>,
        code: &'a str,
        _lines: &'a [markdown::Text],
    ) -> Element<'a, Message, Theme, iced::Renderer> {
        // Built from the raw source rather than from the pre-split lines: without
        // the syntax highlighter the lines carry no styling the plain text does
        // not, and a single text is simpler to wrap.
        let body = text(code.trim_end().to_owned())
            .font(settings.style.code_block_font)
            .size(settings.code_size);

        // Width after `styled`, because the recipe lays the frame out and a code
        // block's frame is `Shrink`.
        container(body).styled(Surface::Code).width(Length::Fill).into()
    }
}

/// A file, the way a messenger shows one: a card with a name and a size.
fn file_view(file: &FileRef) -> Element<'_, Message> {
    row![
        container(text("▤").styled(Type::Muted))
            .padding(Padding::new(6.0).horizontal(9.0))
            .styled(Surface::Quote),
        column![
            text(file.name.clone()).styled(Type::Body).line_height(1.2),
            text(file.size.clone()).styled(Type::Meta),
        ]
        .spacing(1),
    ]
    .spacing(9)
    .align_y(Alignment::Center)
    .into()
}

// ── Composer ────────────────────────────────────────────────────────────────

/// The input, and the one button that matters.
///
/// While a reply is arriving the send button becomes the stop control, which is
/// exactly where a hand already is — no separate strip of window furniture for
/// it.
fn composer(app: &App) -> Element<'_, Message> {
    let input = text_input("发消息…", app.draft())
        .on_input(Message::UserChangedDraft)
        .on_submit(Message::UserSubmittedDraft)
        .padding(Padding::new(9.0).horizontal(13.0))
        .size(14.0)
        .width(Length::Fill)
        .style(input_style);

    let action = if app.is_streaming() {
        button(text("停止").styled(Type::Body))
            .on_press(Message::UserRequestedStop)
            .padding([9.0, 18.0])
            .styled(Act::Quiet)
    } else {
        button(text("发送").styled(Type::Body))
            .on_press(Message::UserSubmittedDraft)
            .padding([9.0, 18.0])
            .styled(Act::Primary)
    };

    let line = row![input, action].spacing(8).align_y(Alignment::Center);

    container(in_column(line.into()))
        .styled(Surface::Footer)
        .into()
}

/// The one weight used for the contact's name.
fn bold() -> Font {
    Font {
        weight: Weight::Bold,
        ..Font::DEFAULT
    }
}

fn input_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let tokens = Tokens::from_theme(theme);
    let focused = matches!(status, text_input::Status::Focused { .. });

    text_input::Style {
        background: Background::Color(tokens.surface),
        border: Border {
            // The focus ring is the accent, and it is the only place in the
            // window where that colour appears without being pressed.
            color: if focused { tokens.accent } else { tokens.border },
            width: 1.0,
            radius: 10.0.into(),
        },
        icon: tokens.text_muted,
        placeholder: tokens.text_muted,
        value: tokens.text,
        selection: tokens.accent,
    }
}
