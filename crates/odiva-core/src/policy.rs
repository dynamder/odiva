//! Which escalations are worth a person's attention.
//!
//! A harness stops whenever it wants to leave its workspace, and most of those
//! stops are routine: a build writes `~/.cargo`, a test writes a temp directory.
//! Asking about those would train the person to approve without reading, which
//! is worse than not asking at all. So the policy allows what daily work needs,
//! refuses what is destructive, and leaves the middle to her.

use std::path::{Path, PathBuf};

use odiva_harness::{Ask, Tool};

use crate::task::Origin;

/// Where a task may write, and which caches are never worth a question.
#[derive(Debug, Clone)]
pub struct Roots {
    pub workspace: PathBuf,
    pub caches: Vec<PathBuf>,
}

impl Roots {
    /// The workspace plus the places a toolchain writes without meaning to.
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        let mut caches = Vec::new();

        if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
            let home = PathBuf::from(home);

            for cache in [".cargo", ".rustup", ".cache", "go", ".npm"] {
                caches.push(home.join(cache));
            }
        }

        if let Some(temp) = std::env::var_os("TEMP").or_else(|| std::env::var_os("TMPDIR")) {
            caches.push(PathBuf::from(temp));
        }

        Self {
            workspace: workspace.into(),
            caches,
        }
    }
}

/// What the policy decided about one escalation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Nobody needs to look at it.
    Allow,
    /// Nobody may allow it.
    Deny(&'static str),
    /// She may answer it herself.
    AgentMayDecide,
    /// It has to reach the person.
    AskUser(&'static str),
}

/// Reads an escalation the way the person would.
pub fn classify(ask: &Ask, roots: &Roots, origin: Origin) -> Verdict {
    if ask
        .tool
        .is_some_and(|tool| matches!(tool, Tool::Read | Tool::Search | Tool::Think | Tool::Fetch))
    {
        return Verdict::Allow;
    }

    let writes = ask
        .tool
        .is_some_and(|tool| matches!(tool, Tool::Edit | Tool::Delete | Tool::Move));

    let outside: Vec<&Path> = ask
        .locations
        .iter()
        .map(PathBuf::as_path)
        .filter(|path| !allowed(path, roots))
        .collect();

    if ask.tool == Some(Tool::Delete) && !outside.is_empty() {
        return Verdict::Deny("删除工作区之外的东西");
    }

    if outside.is_empty() {
        return match ask.tool {
            Some(Tool::Execute | Tool::Edit | Tool::Delete | Tool::Move) => Verdict::Allow,
            Some(Tool::SwitchMode) | None => Verdict::AskUser("没说清要动什么"),
            Some(Tool::Other) if ask.locations.is_empty() => Verdict::AskUser("没说清要动什么"),
            Some(Tool::Other) => Verdict::Allow,
            Some(Tool::Read | Tool::Search | Tool::Think | Tool::Fetch) => Verdict::Allow,
        };
    }

    if origin == Origin::Interest {
        return Verdict::Deny("兴趣任务不越界");
    }

    if writes {
        return Verdict::AskUser("工作区之外的写入");
    }

    Verdict::AgentMayDecide
}

/// Whether a path is inside the workspace or a cache that always gets written.
fn allowed(path: &Path, roots: &Roots) -> bool {
    if path.starts_with(&roots.workspace) {
        return true;
    }

    if roots.caches.iter().any(|cache| path.starts_with(cache)) {
        return true;
    }

    path.components()
        .any(|part| matches!(part.as_os_str().to_str(), Some("target" | "node_modules")))
}
