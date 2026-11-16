//! The ledger: one file per task, and the report a worker leaves behind.
//!
//! A task is a record on disk, not a session in memory. A worker is a process
//! that may die at any moment, so what the core knows about a task has to be
//! readable after a restart without asking anybody.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// What a task is waiting on. Never a status code: a sentence about the wait.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    /// A worker is on it.
    OnHarness,
    /// A turn ended and it is waiting for words from us.
    OnInstruction,
    /// Stuck on a permission request, waiting for a person or for her.
    OnPermission(String),
    /// Waiting for a free slot.
    OnSlot,
    /// Done, waiting for the next thing.
    OnTrigger,
    /// It failed, with a reason worth keeping.
    Failed(String),
}

impl Status {
    /// Whether this task is occupying a slot.
    pub fn holds_a_slot(&self) -> bool {
        matches!(self, Self::OnHarness | Self::OnPermission(_))
    }

    pub fn card(&self) -> &'static str {
        match self {
            Self::OnHarness => "在跑",
            Self::OnInstruction => "等你说话",
            Self::OnPermission(_) => "等你批准",
            Self::OnSlot => "排队",
            Self::OnTrigger => "已完成",
            Self::Failed(_) => "失败",
        }
    }
}

/// Who wanted it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Origin {
    User,
    Own,
}

/// What the worker said about itself. The words are the protocol: a worker
/// writes `done`, `need` or `blocked`, exactly as the brief asked it to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Done,
    Need,
    Blocked,
}

/// The envelope the brief asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub status: Outcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    #[serde(default)]
    pub artifacts: Vec<String>,
}

impl Report {
    /// Reads the last ```` ```report ```` block; a missing envelope is `Done`,
    /// because a worker's output must never be thrown away over punctuation.
    pub fn parse(text: &str) -> Option<Self> {
        let start = text.rfind("```report")?;
        let body = &text[start + "```report".len()..];
        let body = body.trim_start_matches(|character: char| character != '\n');
        let end = body.find("```")?;

        serde_json::from_str(body[..end].trim()).ok()
    }
}

/// One task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub origin: Origin,
    pub cwd: PathBuf,
    pub brief: String,
    pub status: Status,
    #[serde(default)]
    pub report: Option<Report>,
    /// What the worker said in its last turn, kept for `task_read`.
    #[serde(default)]
    pub said: String,
}

impl Task {
    /// One line for the conversation: the card.
    pub fn line(&self) -> String {
        format!("{} · {} · {}", self.id, self.card(), self.headline())
    }

    fn card(&self) -> &'static str {
        self.status.card()
    }

    fn headline(&self) -> String {
        match &self.status {
            Status::Failed(reason) => reason.clone(),
            Status::OnPermission(ask) => ask.clone(),
            _ => match &self.report {
                Some(report) => match report.status {
                    Outcome::Done => "完成了".to_owned(),
                    Outcome::Need => report
                        .question
                        .clone()
                        .unwrap_or_else(|| "需要一句指示".to_owned()),
                    Outcome::Blocked => "卡住了".to_owned(),
                },
                None => self.brief.lines().next().unwrap_or("").to_owned(),
            },
        }
    }
}

/// Every task the core knows about, and the directory they live in.
#[derive(Debug)]
pub struct Ledger {
    dir: PathBuf,
    tasks: BTreeMap<String, Task>,
    next: u32,
}

impl Ledger {
    /// Loads every task ever written.
    pub fn at(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;

        let mut tasks = BTreeMap::new();
        let mut next = 0;

        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }

            let text = std::fs::read_to_string(&path)?;
            let task: Task = match serde_json::from_str(&text) {
                Ok(task) => task,
                Err(_) => continue,
            };

            if let Some(number) = task.id.strip_prefix('t').and_then(|n| n.parse::<u32>().ok()) {
                next = next.max(number);
            }

            tasks.insert(task.id.clone(), task);
        }

        Ok(Self { dir, tasks, next })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Reserves an id; the task itself is written by [`Ledger::put`].
    pub fn reserve(&mut self) -> String {
        self.next += 1;
        format!("t{}", self.next)
    }

    pub fn put(&mut self, task: Task) -> Result<()> {
        let path = self.dir.join(format!("{}.json", task.id));
        let text = serde_json::to_string_pretty(&task)
            .map_err(|error| Error::Disk(error.to_string()))?;

        std::fs::write(&path, text)
            .map_err(|error| Error::Disk(format!("{}: {error}", path.display())))?;

        self.tasks.insert(task.id.clone(), task);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Task> {
        self.tasks
            .get(id)
            .cloned()
            .ok_or_else(|| Error::Disk(format!("没有这个任务：{id}")))
    }

    pub fn all(&self) -> Vec<Task> {
        self.tasks.values().cloned().collect()
    }

    pub fn holding(&self) -> usize {
        self.tasks
            .values()
            .filter(|task| task.status.holds_a_slot())
            .count()
    }

    /// Everything still on the table: running, asking, or waiting for a slot.
    pub fn unfinished(&self) -> Vec<Task> {
        self.tasks
            .values()
            .filter(|task| {
                matches!(
                    task.status,
                    Status::OnHarness | Status::OnInstruction | Status::OnPermission(_) | Status::OnSlot
                )
            })
            .cloned()
            .collect()
    }

    pub fn queued(&self) -> Option<String> {
        self.tasks
            .values()
            .find(|task| task.status == Status::OnSlot)
            .map(|task| task.id.clone())
    }
}
