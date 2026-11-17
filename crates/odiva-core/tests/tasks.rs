//! The records on disk: one file per task, and the envelope a turn ends with.

use std::path::PathBuf;

use odiva_core::task::{Ledger, Origin, Outcome, Report, Status, Task};

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("odiva-tasks-{name}"));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a scratch directory");
    path
}

fn task(id: &str, status: Status) -> Task {
    Task {
        id: id.to_owned(),
        origin: Origin::User,
        cwd: PathBuf::from("somewhere"),
        brief: "干点什么".to_owned(),
        status,
        report: None,
        said: String::new(),
    }
}

#[test]
fn a_record_survives_being_read_again() {
    let dir = scratch("reload");

    let mut ledger = Ledger::at(&dir).expect("a ledger");
    let id = ledger.reserve();
    ledger.put(task(&id, Status::Done)).expect("a record");

    let again = Ledger::at(&dir).expect("a ledger");
    let read = again.get(&id).expect("the record is still there");

    assert_eq!(read.status, Status::Done);
    assert_eq!(read.brief, "干点什么");
}

#[test]
fn the_next_id_is_past_everything_already_written() {
    let dir = scratch("ids");

    let mut ledger = Ledger::at(&dir).expect("a ledger");
    let first = ledger.reserve();
    ledger.put(task(&first, Status::Done)).expect("a record");

    let mut again = Ledger::at(&dir).expect("a ledger");
    let second = again.reserve();

    assert_eq!(first, "t1");
    assert_eq!(second, "t2");
}

#[test]
fn only_a_running_task_holds_a_slot() {
    assert!(Status::OnHarness.holds_a_slot());
    assert!(
        !Status::OnPermission("删一个文件".to_owned()).holds_a_slot(),
        "a worker waiting on an answer is not burning a turn"
    );
    assert!(!Status::Paused.holds_a_slot());
    assert!(!Status::Done.holds_a_slot());
}

#[test]
fn only_live_work_is_unfinished() {
    let dir = scratch("unfinished");

    let mut ledger = Ledger::at(&dir).expect("a ledger");
    for status in [
        Status::OnHarness,
        Status::Paused,
        Status::Done,
        Status::Cancelled,
    ] {
        let id = ledger.reserve();
        ledger.put(task(&id, status)).expect("a record");
    }

    let live: Vec<_> = ledger.unfinished().into_iter().map(|task| task.status).collect();

    assert_eq!(live, vec![Status::OnHarness]);
}

#[test]
fn a_report_is_read_from_the_last_block() {
    let said = "做了一半。\n\n```report\n{\"status\": \"need\", \"question\": \"删掉吗？\"}\n```";
    let report = Report::parse(said).expect("the envelope");

    assert_eq!(report.status, Outcome::Need);
    assert_eq!(report.question.as_deref(), Some("删掉吗？"));
}

#[test]
fn words_without_an_envelope_are_a_finish() {
    assert_eq!(Report::parse("做完了，没有信封"), None);
}

#[test]
fn a_spent_day_does_not_follow_anyone_into_the_next_one() {
    use odiva_core::budget::{self, Budget};

    let store = odiva_core::Store::new(scratch("budget"));
    let path = budget::path(&store);

    let mut today = Budget::default();
    assert!(today.charge_roam(1_000));
    assert!(!today.charge_roam(u64::MAX));
    today.save(&path).expect("a saved budget");

    let reloaded = Budget::load(&path);
    assert_eq!(reloaded.roam.used, 1_000);
    assert_eq!(reloaded.date, today.date);

    let yesterday = Budget {
        date: "2020-01-01".to_owned(),
        ..Budget::default()
    };
    yesterday.save(&path).expect("a stale budget");

    let rolled = Budget::load(&path);
    assert_eq!(rolled.roam.used, 0, "yesterday's spending is yesterday's");
    assert_eq!(rolled.date, today.date);
}
