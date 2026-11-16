//! The window, drawn as a conversation.
//!
//! Three bands: the contact line, the transcript, the composer. Only the
//! transcript scrolls and only it is a list — a windowed one, because a
//! conversation is the one thing here that grows without bound.
//!
//! Two conventions that are easy to undo by accident:
//!
//! * no harness vocabulary on screen — no ids, harness names, tool calls or
//!   progress bars. The window has no such concepts; everything the agent says it
//!   says in words, in a message, at a moment it chose.
//! * one column owns the window. The contact line, every bubble and the composer
//!   are centred on the same measure.
//!
//! Consecutive messages from one side are a group: 3px inside a group, 16px
//! between sides, and only the group's outer corner is sharp.

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

/// Who this is, and whether she is here.
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

        items = items.push(
            sensor(view).on_resize(move |size| Message::MessageMeasured { slot, size }),
        );
    }

    let column_of_messages = column![window.surround(items)].width(Length::Fill);

    let reading = scrollable(in_column(column_of_messages.into()))
        .id(SHEET)
        .on_scroll(Message::TranscriptScrolled)
        .height(Length::Fill)
        .width(Length::Fill);

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

    let bubble = container(body).styled(Surface::Bubble(msg.from, shape));

    // After `styled`, which lays the frame out: a `Fill` child of a `Shrink`
    // bubble resolves to zero width, and `max_width` alone does not fix it.
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

/// The message body: iced's markdown widget, dressed in this application's
/// palette.
///
/// The palette is an argument rather than a style closure because that is what
/// the widget takes, which is why [`App`] holds a `Theme` at all: a `view`
/// function is handed no theme.
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

    let settings = markdown::Settings::with_text_size(14.0, style);

    markdown::view_with(items, settings, &Viewer)
}

/// The window's markdown viewer.
///
/// Only `code_block` differs from iced's default, and it has to: the default
/// class is `container::dark` for *every* theme, which is a black block with
/// near-black text in the light theme.
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
        let body = text(code.trim_end().to_owned())
            .font(settings.style.code_block_font)
            .size(settings.code_size);

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

/// The input, the one button that matters, and whatever failed last.
///
/// The send button becomes the stop control while she is working on something;
/// a failure sits above the input until the next attempt, because a window that
/// swallowed an error would be lying about the conversation.
fn composer(app: &App) -> Element<'_, Message> {
    let input = text_input("发消息…", app.draft())
        .on_input(Message::UserChangedDraft)
        .on_submit(Message::UserSubmittedDraft)
        .padding(Padding::new(9.0).horizontal(13.0))
        .size(14.0)
        .width(Length::Fill)
        .style(input_style);

    let action = if app.is_working() {
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

    let mut band = Column::new().spacing(7).width(Length::Fill);

    if let Some(note) = app.notice() {
        band = band.push(text(note.to_owned()).styled(Type::Error).width(Length::Fill));
    }

    band = band.push(line);

    container(in_column(band.into()))
        .styled(Surface::Footer)
        .into()
}

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
