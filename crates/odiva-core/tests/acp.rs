//! The pipe: a real child process, a real handshake, a real turn.
//!
//! The harness under test is `fake_harness`, which proves the client and not the
//! model: what is being pinned here is that a turn comes back whole, that a
//! permission request never reaches a person, and that a harness that dies
//! surfaces as an error instead of a hang.

use std::time::Duration;

use odiva_core::acp::{Stop, Turn, Worker};

const TIMEOUT: Duration = Duration::from_secs(20);

fn harness(script: &str) -> Worker {
    Worker::new(
        env!("CARGO_BIN_EXE_fake_harness"),
        env!("CARGO_MANIFEST_DIR"),
    )
    .env("ODIVA_FAKE_SCRIPT", script)
}

async fn talk(script: &str) -> Turn {
    tokio::time::timeout(TIMEOUT, harness(script).talk("查一下日志"))
        .await
        .expect("the turn finished in time")
        .expect("the turn finished")
}

#[tokio::test]
async fn a_turn_arrives_whole_with_the_test_says() {
    let turn = talk("chat").await;

    assert_eq!(turn.said, "README 改完了。日志那边还有个发现。");
    assert_eq!(turn.stop, Stop::Finished);
}

#[tokio::test]
async fn a_permission_request_is_refused_by_name() {
    let turn = talk("permission").await;

    assert_eq!(turn.said, "denied:reject-once");
    assert_eq!(turn.stop, Stop::Finished);
}

#[tokio::test]
async fn a_harness_that_dies_is_an_error_not_a_hang() {
    let failure = tokio::time::timeout(TIMEOUT, harness("crash").talk("查一下日志"))
        .await
        .expect("the failure came back in time");

    assert!(failure.is_err(), "a dead harness must not look like a turn");
}
