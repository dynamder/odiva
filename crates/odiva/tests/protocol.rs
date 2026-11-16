//! The seam, exercised without a window.
//!
//! Everything here is about one property: **`App::apply` is the only door.** If a
//! transcript can be built by anything other than `CoreMessage`s, the window has
//! state of its own and the protocol is a decoration.

use odiva::{App, Message};
use odiva_proto::{CoreMessage, FileRef, MessageId, Presence, Side, Snapshot, UiMessage, WireMessage};

fn attached(messages: Vec<WireMessage>, presence: Presence) -> CoreMessage {
    CoreMessage::CoreAttachedSession {
        snapshot: Snapshot { messages, presence },
    }
}

fn said(id: MessageId, from: Side, markdown: &str) -> WireMessage {
    WireMessage::new(id, from, markdown)
}

#[test]
fn a_snapshot_is_enough_to_draw_the_whole_window() {
    let mut app = App::empty();

    app.apply(attached(
        vec![
            said(1, Side::User, "早上好"),
            said(2, Side::Assistant, "早。今天要推哪几件事？"),
        ],
        Presence::Online,
    ));

    assert_eq!(app.messages().len(), 2);
    assert_eq!(app.messages()[0].from, Side::User);
    assert_eq!(app.presence(), Presence::Online);
    assert!(!app.is_streaming());
}

#[test]
fn a_streamed_message_grows_until_it_is_finished() {
    let mut app = App::empty();

    app.apply(CoreMessage::MessageBegan {
        id: 7,
        from: Side::Assistant,
    });
    assert!(app.is_streaming());

    app.apply(CoreMessage::MessageStreamedText {
        id: 7,
        delta: "# 结论\n\n握手超时是".to_owned(),
    });
    app.apply(CoreMessage::MessageStreamedText {
        id: 7,
        delta: "因为 stderr 混进了协议流。".to_owned(),
    });

    let msg = &app.messages()[0];
    assert_eq!(msg.source, "# 结论\n\n握手超时是因为 stderr 混进了协议流。");
    // Parsed once per change, not once per frame: a heading and a paragraph.
    assert_eq!(msg.items.len(), 2);

    app.apply(CoreMessage::MessageFinished { id: 7 });
    assert!(!app.is_streaming());
    assert!(!app.messages()[0].streaming);
}

#[test]
fn finishing_an_unknown_message_is_not_a_panic() {
    let mut app = App::empty();

    // A core that reports on a message the window never saw begun is a bug, not
    // a crash: the worst it may do is nothing.
    app.apply(CoreMessage::MessageFinished { id: 99 });
    app.apply(CoreMessage::MessageStreamedText {
        id: 99,
        delta: "orphan".to_owned(),
    });

    assert!(app.messages().is_empty());
    assert!(!app.is_streaming());
}

#[test]
fn files_and_replies_attach_to_the_message_they_belong_to() {
    let mut app = App::empty();

    app.apply(CoreMessage::MessageBegan {
        id: 3,
        from: Side::Assistant,
    });
    app.apply(CoreMessage::MessageStreamedText {
        id: 3,
        delta: "计划写好了。".to_owned(),
    });
    app.apply(CoreMessage::MessageAttachedFile {
        id: 3,
        file: FileRef {
            name: "本周计划.md".to_owned(),
            size: "3.1 KB".to_owned(),
        },
    });
    app.apply(CoreMessage::MessageOfferedReplies {
        id: 3,
        replies: vec!["好".to_owned()],
    });
    app.apply(CoreMessage::MessageFinished { id: 3 });

    let msg = &app.messages()[0];
    assert_eq!(msg.files.len(), 1);
    assert_eq!(msg.replies, vec!["好".to_owned()]);
}

#[test]
fn typing_is_the_persons_word_and_tapping_a_reply_is_the_same_word() {
    let mut app = App::empty();

    let _ = app.update(Message::UserChangedDraft("看下日志".to_owned()));
    let _ = app.update(Message::UserSubmittedDraft);

    assert_eq!(app.draft(), "");
    assert_eq!(
        app.take_outbox(),
        vec![UiMessage::UserSubmittedText {
            text: "看下日志".to_owned()
        }]
    );

    // The window does not add the line to the transcript: the core echoes it
    // back as a `MessageAppended`, so there is one place where a message is born.
    assert!(app.messages().is_empty());

    let _ = app.update(Message::UserTappedReply("改吧".to_owned()));
    assert_eq!(
        app.take_outbox(),
        vec![UiMessage::UserSubmittedText {
            text: "改吧".to_owned()
        }]
    );
}

#[test]
fn an_empty_draft_says_nothing() {
    let mut app = App::empty();

    let _ = app.update(Message::UserChangedDraft("   ".to_owned()));
    let _ = app.update(Message::UserSubmittedDraft);

    assert!(app.take_outbox().is_empty());
}

#[test]
fn a_re_attach_replaces_the_transcript() {
    let mut app = App::empty();

    app.apply(attached(vec![said(1, Side::User, "旧")], Presence::Online));
    assert_eq!(app.messages().len(), 1);

    app.apply(attached(
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
