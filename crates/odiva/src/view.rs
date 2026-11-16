//! The window, drawn — as a conversation, not as a log.
//!
//! Three bands, top to bottom: the contact line, the transcript, the composer.
//! The transcript is the only thing that scrolls, and it is the only thing that
//! is a list.
//!
//! Two conventions worth stating because they are easy to undo by accident:
//!
//! * **no harness vocabulary on screen.** No ids, no harness names, no tool
//!   calls, no progress bars, no "task" anything. Not because they are hidden —
//!   because the window has no such concepts to draw. Everything the agent wants
//!   to say, it says in words, in a message, at a moment it chose.
//! * **consecutive messages from one side are a group.** The gap between them is
//!   small and the gap to the other side is large, which is how every messenger
//!   says "same speaker, new thought" without a label.

use iced::font::Weight;
use iced::widget::{
    button, column, container, markdown, row, rule, scrollable, space, text, text_input, Column,
    Stack,
};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Padding, Theme};

use ericed::style::{StyleExt, Tokens};
use odiva_proto::{FileRef, Presence, Side};

use crate::model::Msg;
use crate::style::{
    block_gap, bubble_width, column_width, same_side_gap, side_gap, wide_bubble_width, Act, Line,
    Surface, Type,
};
use crate::{App, Message};

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

    container(
        column![
            text("odiva").font(bold()).styled(Type::Strong),
            text(presence).styled(tone),
        ]
        .spacing(1),
    )
    .styled(Surface::Header)
    .into()
}

// ── Transcript ──────────────────────────────────────────────────────────────

/// The conversation, in a fixed-width column.
///
/// The column is fixed rather than `Fill` because a line of prose that spans a
/// maximised window is unreadable; the two `space::horizontal` are what centre
/// it, which is cheaper and more predictable than a max-width container.
fn transcript(app: &App) -> Element<'_, Message> {
    let mut list = Column::new().spacing(0.0).width(Length::Fill);
    list = list.push(time_divider());

    let mut previous: Option<Side> = None;

    for msg in app.messages() {
        // The gap is the group: small inside one speaker's run, large between
        // sides. It is the only thing that says who is speaking after the first
        // bubble of a run, and it costs four pixels instead of a name label.
        let gap = match previous {
            None => 10.0,
            Some(side) if side == msg.from => same_side_gap(),
            Some(_) => side_gap(),
        };

        list = list.push(
            container(message_view(msg, app.theme()))
                .padding(Padding::ZERO.top(gap)),
        );
        previous = Some(msg.from);
    }

    let content = row![
        space::horizontal(),
        container(list)
            .width(Length::Fixed(column_width()))
            .padding(Padding::new(12.0).horizontal(0.0)),
        space::horizontal(),
    ];

    // Not bottom-anchored, and not for lack of trying: `Scrollable::anchor_bottom`
    // only moves the scrollbar thumb, and a `Length::Fill` child inside a
    // `Scrollable` is measured against an *infinite* height, so it resolves to
    // zero and the whole transcript disappears. Bottom-anchoring needs the content
    // height (a `sensor`) compared against the viewport, which is a real piece of
    // work rather than a flag — and it only pays off once the transcript can also
    // hold its position while streaming, i.e. with the `Pinned` policy.
    let reading = scrollable(content)
        .height(Length::Fill)
        .width(Length::Fill);

    if app.is_pinned() {
        return reading.into();
    }

    let jump = container(
        button(text("↓").styled(Type::Body))
            .on_press(Message::UserJumpedToNewest)
            .padding([5.0, 12.0])
            .styled(Act::Quiet),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(Alignment::End)
    .align_y(Alignment::End)
    .padding(Padding::new(0.0).right(24.0).bottom(12.0));

    Stack::new().push(reading).push(jump).into()
}

/// The time divider above the conversation.
///
/// A constant until the core starts telling the window about time — which it
/// will have to, the moment the transcript can be scrolled back past today.
fn time_divider<'a>() -> Element<'a, Message> {
    container(
        row![
            space::horizontal(),
            text("今天 15:42").styled(Type::Caption),
            space::horizontal(),
        ]
        .width(Length::Fill),
    )
    .into()
}

/// One message: a bubble, on the side of whoever sent it.
fn message_view<'a>(msg: &'a Msg, theme: Theme) -> Element<'a, Message> {
    // A message with a block-level item needs room: iced's markdown renders code
    // blocks and tables at `Length::Fill`, and a `Fill` child of a `Shrink` bubble
    // resolves to zero width — the bubble collapses around nothing. So the ones
    // that carry a block get a filled, wider bubble, and prose stays ragged.
    let blocky = msg.items.iter().any(|item| {
        matches!(
            item,
            markdown::Item::CodeBlock { .. }
                | markdown::Item::Table { .. }
                | markdown::Item::List { .. }
        )
    });

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
        let mut replies = row![].spacing(8);

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
        body = body.push(text("▌").styled(Type::Accent));
    }

    // Width last, after the recipe: `StyleExt::styled` lays the frame out, and the
    // frame says `Shrink` for a bubble. A fixed width here is what gives a code
    // block something to fill — `max_width` alone leaves the child's limits at
    // zero, and the bubble collapses around nothing.
    let bubble = container(body).styled(Surface::Bubble(msg.from));

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

    // 13.5 is the body size this window uses everywhere; the widget derives
    // heading and code sizes from it.
    let settings = markdown::Settings::with_text_size(13.5, style);

    markdown::view(items, settings).map(Message::UserOpenedLink)
}

/// A file, the way a messenger shows one: a card with a name and a size.
fn file_view(file: &FileRef) -> Element<'_, Message> {
    row![
        container(text("▤").styled(Type::Muted))
            .padding(Padding::new(6.0).horizontal(9.0))
            .styled(Surface::Quote),
        column![
            text(file.name.clone()).styled(Type::Body),
            text(file.size.clone()).styled(Type::Meta),
        ]
        .spacing(2),
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
        .padding(Padding::new(10.0).horizontal(12.0))
        .size(13.5)
        .width(Length::Fill)
        .style(input_style);

    let action = if app.is_streaming() {
        button(text("停止").styled(Type::Body))
            .on_press(Message::UserRequestedStop)
            .padding([9.0, 16.0])
            .styled(Act::Quiet)
    } else {
        button(text("发送").styled(Type::Body))
            .on_press(Message::UserSubmittedDraft)
            .padding([9.0, 16.0])
            .styled(Act::Primary)
    };

    container(row![input, action].spacing(8).align_y(Alignment::Center))
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

fn input_style(theme: &Theme, _status: text_input::Status) -> text_input::Style {
    let tokens = Tokens::from_theme(theme);

    text_input::Style {
        background: Background::Color(tokens.surface),
        border: Border {
            color: tokens.border,
            width: 1.0,
            radius: 9.0.into(),
        },
        icon: tokens.text_muted,
        placeholder: tokens.text_muted,
        value: tokens.text,
        selection: tokens.accent,
    }
}
