//! Reading a line back: the record keeps what compaction dropped.

use std::path::PathBuf;

use odiva_core::history::{self, Query};
use odiva_core::line::{LineId, Mode};
use odiva_core::{Line, Store};

fn scratch(name: &str) -> Store {
    let path: PathBuf = std::env::temp_dir().join(format!("odiva-history-{name}"));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a scratch directory");
    Store::new(path)
}

fn seed(store: &Store) -> LineId {
    let id = LineId::normal(Mode::Idle);
    let mut line = Line::open(store, id.clone(), "你是 odiva。").expect("a line");

    for index in 0..6 {
        line.push_note(format!("第 {index} 条")).expect("a note");
    }

    id
}

#[test]
fn everything_a_line_said_is_still_there() {
    let store = scratch("all");
    let id = seed(&store);

    let entries = history::entries(&store, &id).expect("a line");

    assert_eq!(entries.len(), 6);
    assert_eq!(entries[0].text, "第 0 条");
    assert_eq!(entries[5].position, 6, "the root is position 0");
    assert_eq!(entries[0].generation, 0);
}

#[test]
fn a_compaction_does_not_lose_the_old_words() {
    let store = scratch("compacted");
    let id = seed(&store);

    let mut line = Line::open(&store, id.clone(), "你是 odiva。").expect("a line");
    line.compact(3).expect("a rotation");
    line.push_note("压缩之后").expect("a note");

    let entries = history::entries(&store, &id).expect("a line");
    let words: Vec<&str> = entries.iter().map(|entry| entry.text.as_str()).collect();

    assert!(words.contains(&"第 0 条"), "the first generation stays");
    assert!(words.contains(&"压缩之后"));
    assert!(
        entries.iter().any(|entry| entry.generation == 1),
        "the newer generation is read too"
    );
}

#[test]
fn a_query_can_ask_for_one_stretch_or_one_word() {
    let store = scratch("query");
    let id = seed(&store);

    let window = history::render(
        &store,
        &id,
        &Query {
            from: Some(2),
            to: Some(4),
            ..Query::default()
        },
    )
    .expect("a line");

    assert!(window.contains("第 1 条"));
    assert!(window.contains("第 3 条"));
    assert!(!window.contains("第 0 条"));
    assert!(!window.contains("第 5 条"));

    let found = history::render(
        &store,
        &id,
        &Query {
            contains: Some("第 4 条".to_owned()),
            ..Query::default()
        },
    )
    .expect("a line");

    assert!(found.contains("第 4 条"));
    assert!(found.contains("1 条匹配"), "only one entry matches");
}

#[test]
fn the_limit_keeps_the_newest_matches() {
    let store = scratch("limit");
    let id = seed(&store);

    let text = history::render(
        &store,
        &id,
        &Query {
            limit: Some(2),
            ..Query::default()
        },
    )
    .expect("a line");

    assert!(text.contains("第 4 条"));
    assert!(text.contains("第 5 条"));
    assert!(!text.contains("第 3 条"));
}

#[test]
fn another_line_is_not_in_this_one() {
    let store = scratch("scoped");
    let id = seed(&store);

    let mut other = Line::open(&store, LineId::task("t9"), "你是 odiva。").expect("a line");
    other.push_note("别的任务的事").expect("a note");

    let text = history::render(&store, &id, &Query::default()).expect("a line");

    assert!(!text.contains("别的任务的事"));
}
