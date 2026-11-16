//! The seam, exercised without a window.
//!
//! One property decides the rest: `App::apply` is the only door. If a transcript
//! can be built by anything other than `CoreMessage`s, the window has state of
//! its own and the protocol is a decoration.

use std::sync::{Arc, Mutex};

use iced::Size;
use ericed::list::Scroll;
use odiva::{App, Message, model, time};
use odiva_proto::{
    CoreHandle, CoreMessage, FileRef, MessageId, Presence, Side, Snapshot, UiMessage, WireMessage,
};

const VIEWPORT: f32 = 600.0;

fn attached(messages: Vec<WireMessage>, presence: Presence) -> CoreMessage {
    CoreMessage::CoreAttachedSession {
        snapshot: Snapshot { messages, presence },
    }
}

fn said(id: MessageId, from: Side, markdown: &str) -> WireMessage {
    WireMessage::new(id, 0, from, markdown)
}

#[derive(Clone, Default)]
struct Recording {
    sent: Arc<Mutex<Vec<UiMessage>>>,
}

impl Recording {
    fn taken(&self) -> Vec<UiMessage> {
        let mut sent = self.sent.lock().expect("no panic while recording");
        std::mem::take(&mut *sent)
    }
}

impl CoreHandle for Recording {
    fn send(&self, message: UiMessage) {
        self.sent
            .lock()
            .expect("no panic while recording")
            .push(message);
    }
}

fn connected_window() -> (App, Recording) {
    let handle = Recording::default();
    let (_core, messages) = tokio::sync::mpsc::unbounded_channel();

    (App::empty().connected(handle.clone(), messages), handle)
}

fn window_with(messages: Vec<WireMessage>) -> App {
    let mut app = App::empty();
    let _ = app.apply(attached(messages, Presence::Online));
    let _ = app.update(Message::TranscriptResized(Size::new(700.0, VIEWPORT)));
    app
}

#[test]
fn a_snapshot_is_enough_to_draw_the_whole_window() {
    let app = window_with(vec![
        said(1, Side::User, "早上好"),
        said(2, Side::Assistant, "早。今天要推哪几件事？"),
    ]);

    assert_eq!(app.messages().len(), 2);
    assert_eq!(app.messages()[0].from, Side::User);
    assert_eq!(app.presence(), Presence::Online);
}

#[test]
fn every_say_is_one_whole_bubble() {
    let mut app = App::empty();

    for (id, markdown) in [(1, "先查日志。"), (2, "日志里有 stderr。"), (3, "改完了。")] {
        let _ = app.apply(CoreMessage::MessageAppended {
            message: said(id, Side::Assistant, markdown),
        });
    }

    let ids: Vec<_> = app.messages().iter().map(|msg| msg.id).collect();
    assert_eq!(ids, vec![1, 2, 3]);
    assert_eq!(app.messages()[1].source, "日志里有 stderr。");
}

#[test]
fn files_and_replies_come_with_the_message() {
    let mut message = said(3, Side::Assistant, "计划写好了。");
    message.files.push(FileRef {
        name: "本周计划.md".to_owned(),
        size: "3.1 KB".to_owned(),
    });
    message.replies = vec!["好".to_owned()];

    let mut app = App::empty();
    let _ = app.apply(CoreMessage::MessageAppended { message });

    let msg = &app.messages()[0];
    assert_eq!(msg.files.len(), 1);
    assert_eq!(msg.replies, vec!["好".to_owned()]);
}

#[test]
fn presence_is_the_cores_word() {
    let mut app = App::empty();
    assert!(!app.is_working());

    let _ = app.apply(CoreMessage::PresenceChanged {
        presence: Presence::Typing,
    });

    assert!(app.is_working());
}

#[test]
fn typing_is_the_persons_word_and_tapping_a_reply_is_the_same_word() {
    let (mut app, handle) = connected_window();

    let _ = app.update(Message::UserChangedDraft("看下日志".to_owned()));
    let _ = app.update(Message::UserSubmittedDraft);

    assert_eq!(app.draft(), "");
    assert_eq!(
        handle.taken(),
        vec![UiMessage::UserSubmittedText {
            text: "看下日志".to_owned()
        }]
    );
    assert!(app.messages().is_empty());

    let _ = app.update(Message::UserTappedReply("改吧".to_owned()));
    assert_eq!(
        handle.taken(),
        vec![UiMessage::UserSubmittedText {
            text: "改吧".to_owned()
        }]
    );
}

#[test]
fn an_empty_draft_says_nothing() {
    let (mut app, handle) = connected_window();

    let _ = app.update(Message::UserChangedDraft("   ".to_owned()));
    let _ = app.update(Message::UserSubmittedDraft);

    assert!(handle.taken().is_empty());
}

#[test]
fn a_re_attach_replaces_the_transcript() {
    let mut app = window_with(vec![said(1, Side::User, "旧")]);
    assert_eq!(app.messages().len(), 1);

    let _ = app.apply(attached(
        vec![
            said(2, Side::Assistant, "新的一页"),
            said(3, Side::User, "好"),
        ],
        Presence::Typing,
    ));

    assert_eq!(app.messages().len(), 2);
    assert_eq!(app.messages()[0].source, "新的一页");
    assert_eq!(app.presence(), Presence::Typing);
}

#[test]
fn a_long_transcript_builds_only_a_window_of_it() {
    let messages = (0..400u64)
        .map(|slot| {
            said(
                slot + 1,
                if slot % 2 == 0 {
                    Side::User
                } else {
                    Side::Assistant
                },
                "一条消息，长度大约一行半，够让估算值和实测值不一样。",
            )
        })
        .collect();

    let app = window_with(messages);

    let built = app.slice().len();
    assert!(built < 40, "built {built} rows for a 400-message transcript");
    assert!(app.total() > VIEWPORT, "the transcript should overflow");
}

#[test]
fn a_failure_is_shown_until_the_next_attempt() {
    let mut app = App::empty();

    let _ = app.apply(CoreMessage::CoreFailed {
        note: "这一轮没跑完：没有 key".to_owned(),
    });

    assert_eq!(app.notice(), Some("这一轮没跑完：没有 key"));

    let _ = app.update(Message::UserChangedDraft("再来".to_owned()));
    let _ = app.update(Message::UserSubmittedDraft);

    assert_eq!(app.notice(), None, "trying again clears the last failure");
}

#[test]
fn a_measured_row_corrects_the_total() {
    let mut app = window_with(vec![said(1, Side::User, "一"), said(2, Side::Assistant, "二")]);

    let before = app.total();
    let _ = app.update(Message::MessageMeasured {
        slot: 0,
        size: Size::new(700.0, 200.0),
    });

    assert!(app.total() > before, "measuring a tall row changes the total");
}

#[test]
fn jumping_to_the_newest_follows_again() {
    let mut app = window_with(vec![said(1, Side::User, "在吗")]);
    app = app.following(false);

    assert!(!app.is_pinned());

    let _ = app.update(Message::UserJumpedToNewest);
    assert!(app.is_pinned());
}

#[test]
fn the_bottom_is_where_the_viewport_says_not_where_the_table_guesses() {
    let messages = (0..20u64)
        .map(|slot| {
            said(
                slot + 1,
                Side::User,
                "一条消息，长度大约一行半，够让估算值和实测值不一样。",
            )
        })
        .collect();

    let mut app = window_with(messages);
    let laid_out = app.total() - 100.0;

    let _ = app.update(Message::TranscriptScrolled(Scroll::at(
        laid_out - VIEWPORT,
        VIEWPORT,
        laid_out,
    )));

    assert!(
        app.is_pinned(),
        "the scrollbar is at the bottom even though the table claims another 100px"
    );

    let _ = app.update(Message::TranscriptScrolled(Scroll::at(
        laid_out - VIEWPORT - 100.0,
        VIEWPORT,
        laid_out,
    )));

    assert!(!app.is_pinned(), "a hundred pixels up is not the bottom");
}

#[test]
fn a_pause_opens_a_new_occasion() {
    assert!(time::needs_divider(None, 1_000_000));
    assert!(!time::needs_divider(Some(1_000_000), 1_060_000));
    assert!(time::needs_divider(Some(1_000_000), 4_600_000));
}

#[test]
fn a_blocky_message_is_known_before_layout() {
    let mut app = App::empty();
    let _ = app.apply(CoreMessage::MessageAppended {
        message: said(1, Side::Assistant, "```\nfn main() {}\n```"),
    });

    assert!(app.messages()[0].is_blocky());

    let mut app = App::empty();
    let _ = app.apply(CoreMessage::MessageAppended {
        message: said(2, Side::Assistant, "只是说话"),
    });
    assert!(!app.messages()[0].is_blocky());
}

#[test]
fn a_message_knows_its_slot() {
    let app = window_with(vec![
        said(1, Side::User, "一"),
        said(2, Side::Assistant, "二"),
        said(3, Side::User, "三"),
    ]);

    let slots: Vec<_> = app.messages().iter().map(|msg| msg.slot).collect();
    assert_eq!(slots, vec![0, 1, 2]);
    let _: &model::Msg = &app.messages()[0];
}

#[test]
fn only_a_connected_window_subscribes_to_a_core() {
    assert_eq!(App::empty().subscription().units(), 0);

    let (app, _handle) = connected_window();
    assert_eq!(app.subscription().units(), 1);
}

#[test]
fn a_re_attach_keeps_the_wire() {
    let (mut app, _handle) = connected_window();

    let _ = app.apply(attached(
        vec![said(1, Side::User, "新的一页")],
        Presence::Online,
    ));

    assert_eq!(app.subscription().units(), 1);
}

#[test]
fn a_reopened_window_shows_everything_it_was_told() {
    let app = window_with(vec![
        said(1, Side::User, "一"),
        said(2, Side::Assistant, "二"),
        said(3, Side::User, "三"),
    ]);

    assert_eq!(app.slice().len(), 3, "the snapshot is not a preview");
}

#[test]
fn a_row_estimate_includes_the_space_above_it() {
    let one = window_with(vec![said(1, Side::User, "一")]);
    let two = window_with(vec![said(1, Side::User, "一"), said(2, Side::User, "一")]);

    assert_eq!(one.gap_at(0), odiva::style::side_gap());
    assert_eq!(two.gap_at(1), odiva::style::same_side_gap());

    let row = two.messages()[1].estimated_height();
    let added = two.total() - one.total();

    assert!(
        (added - (row + odiva::style::same_side_gap())).abs() < 0.01,
        "a row costs its height plus the gap above it: {added} vs {}",
        row + odiva::style::same_side_gap()
    );
}
