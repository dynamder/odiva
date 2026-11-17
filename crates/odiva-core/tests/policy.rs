//! The policy that decides which escalations are worth a person's attention.

use std::path::PathBuf;

use odiva_core::policy::{Roots, Verdict, classify};
use odiva_core::task::Origin;
use odiva_harness::{Ask, Tool};

fn roots() -> Roots {
    Roots {
        workspace: PathBuf::from("/work/project"),
        caches: vec![
            PathBuf::from("/home/atom/.cargo"),
            PathBuf::from("/tmp"),
        ],
    }
}

fn ask(tool: Option<Tool>, locations: &[&str]) -> Ask {
    Ask {
        title: "something".to_owned(),
        tool,
        locations: locations.iter().map(PathBuf::from).collect(),
        input: None,
    }
}

fn verdict(tool: Option<Tool>, locations: &[&str], origin: Origin) -> Verdict {
    classify(&ask(tool, locations), &roots(), origin)
}

#[test]
fn reading_anywhere_is_not_a_question() {
    assert_eq!(
        verdict(Some(Tool::Read), &["/etc/hosts"], Origin::User),
        Verdict::Allow
    );
}

#[test]
fn the_workspace_is_where_it_works() {
    assert_eq!(
        verdict(Some(Tool::Edit), &["/work/project/src/main.rs"], Origin::User),
        Verdict::Allow
    );
}

#[test]
fn a_toolchain_cache_is_not_a_question() {
    assert_eq!(
        verdict(
            Some(Tool::Edit),
            &["/home/atom/.cargo/registry/index/x"],
            Origin::User
        ),
        Verdict::Allow,
        "a build that writes its own cache must not train anyone to click approve"
    );

    assert_eq!(
        verdict(Some(Tool::Edit), &["/tmp/odiva-123/x"], Origin::User),
        Verdict::Allow
    );
}

#[test]
fn build_output_anywhere_is_not_a_question() {
    assert_eq!(
        verdict(
            Some(Tool::Edit),
            &["/work/other/target/debug/x"],
            Origin::User
        ),
        Verdict::Allow
    );
}

#[test]
fn a_write_elsewhere_reaches_the_person() {
    assert_eq!(
        verdict(Some(Tool::Edit), &["/home/atom/.dsh/config"], Origin::User),
        Verdict::AskUser("工作区之外的写入")
    );
}

#[test]
fn a_delete_outside_is_never_allowed() {
    assert_eq!(
        verdict(Some(Tool::Delete), &["/home/atom/documents"], Origin::User),
        Verdict::Deny("删除工作区之外的东西")
    );
}

#[test]
fn an_interest_task_never_leaves_its_workspace() {
    assert_eq!(
        verdict(Some(Tool::Edit), &["/work/other/x"], Origin::Interest),
        Verdict::Deny("兴趣任务不越界")
    );
}

#[test]
fn an_escalation_that_says_nothing_is_a_question() {
    assert_eq!(
        verdict(None, &[], Origin::User),
        Verdict::AskUser("没说清要动什么")
    );
    assert_eq!(
        verdict(Some(Tool::SwitchMode), &[], Origin::User),
        Verdict::AskUser("没说清要动什么")
    );
}

#[test]
fn a_non_writing_touch_outside_is_hers_to_judge() {
    assert_eq!(
        verdict(Some(Tool::Execute), &["/usr/local/bin/make"], Origin::User),
        Verdict::AgentMayDecide
    );
}
