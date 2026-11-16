//! The ledger, and what a worker's last turn meant.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use odiva_core::task::{Origin, Status, Task};
use odiva_core::{Harness, Tasks};

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("odiva-tasks-{name}"));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a scratch directory");
    path
}

fn harness(script: &str) -> Harness {
    Harness::new(env!("CARGO_BIN_EXE_fake_harness")).env("ODIVA_FAKE_SCRIPT", script)
}

/// Waits until the task stops holding a slot, or gives up.
async fn settled(tasks: &Arc<Tasks>, id: &str) -> Task {
    for _ in 0..200 {
        let task = tasks.get(id).expect("the task is in the ledger");

        if !task.status.holds_a_slot() && task.status != Status::OnSlot {
            return task;
        }

        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    panic!("the task never settled");
}

#[tokio::test]
async fn a_worker_that_finishes_waits_for_the_next_thing() {
    let (tasks, _nudges) =
        Tasks::new(scratch("done"), harness("chat"), 3).expect("a ledger");

    let id = tasks
        .dispatch(Origin::User, "改一下 README 的启动命令".to_owned())
        .expect("a task");

    let task = settled(&tasks, &id).await;

    assert_eq!(task.status, Status::OnTrigger);
    assert!(task.said.contains("README 改完了"));
    assert!(std::fs::metadata(std::path::Path::new(&task.cwd)).is_ok());
}

#[tokio::test]
async fn a_worker_that_asks_waits_for_words() {
    let (tasks, _nudges) = Tasks::new(scratch("need"), harness("need"), 3).expect("a ledger");

    let id = tasks
        .dispatch(Origin::User, "删掉 migrate_v1.sh".to_owned())
        .expect("a task");

    let task = settled(&tasks, &id).await;

    assert_eq!(task.status, Status::OnInstruction, "said: {}", task.said);

    let report = task.report.expect("the envelope");
    assert_eq!(report.question.as_deref(), Some("这个脚本还有别处引用，删掉吗？"));
}

#[tokio::test]
async fn a_worker_that_dies_is_a_failure_with_a_reason() {
    let (tasks, _nudges) = Tasks::new(scratch("crash"), harness("crash"), 3).expect("a ledger");

    let id = tasks
        .dispatch(Origin::User, "随便干点什么".to_owned())
        .expect("a task");

    let task = settled(&tasks, &id).await;

    match task.status {
        Status::Failed(reason) => assert!(!reason.is_empty()),
        other => panic!("a dead worker must fail, got {other:?}"),
    }
}

#[tokio::test]
async fn answering_an_asking_worker_gives_it_another_turn() {
    let (tasks, _nudges) = Tasks::new(scratch("answer"), harness("need"), 3).expect("a ledger");

    let id = tasks
        .dispatch(Origin::User, "删掉 migrate_v1.sh".to_owned())
        .expect("a task");

    settled(&tasks, &id).await;
    tasks.answer(&id, "别删，先全仓搜一遍").expect("an answer");

    let task = settled(&tasks, &id).await;

    assert_eq!(task.status, Status::OnInstruction);
    assert!(
        task.brief.contains("别删，先全仓搜一遍"),
        "the next worker is told what was said: {}",
        task.brief
    );
}

#[tokio::test]
async fn the_ledger_survives_being_read_again() {
    let root = scratch("reload");
    let (tasks, _nudges) = Tasks::new(root.clone(), harness("chat"), 3).expect("a ledger");

    let id = tasks
        .dispatch(Origin::User, "留下一条记录".to_owned())
        .expect("a task");
    settled(&tasks, &id).await;

    let (again, _nudges) = Tasks::new(root, harness("chat"), 3).expect("a ledger");
    let task = again.get(&id).expect("the task is still there");

    assert_eq!(task.status, Status::OnTrigger);
    assert_eq!(task.brief, "留下一条记录");
}

#[tokio::test]
async fn a_worker_that_asks_waits_on_a_verdict() {
    let (tasks, _nudges) =
        Tasks::new(scratch("permit"), harness("permission"), 3).expect("a ledger");

    let id = tasks
        .dispatch(Origin::User, "删掉 migrate_v1.sh".to_owned())
        .expect("a task");

    let mut asked = None;

    for _ in 0..200 {
        let task = tasks.get(&id).expect("the task is in the ledger");

        if let Status::OnPermission(question) = &task.status {
            asked = Some(question.clone());
            break;
        }

        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    let question = asked.expect("the worker asked before it was allowed to act");
    assert!(
        question.contains("migrate_v1.sh"),
        "the question carries the tool call's title: {question}"
    );

    tasks.permit(&id, false).expect("a verdict");

    let task = settled(&tasks, &id).await;
    assert!(
        task.said.contains("denied:reject-once"),
        "the worker was told no, by name: {}",
        task.said
    );
}

#[tokio::test]
async fn a_spent_day_does_not_follow_anyone_into_the_next_one() {
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
