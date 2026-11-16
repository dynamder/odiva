//! Running tasks: slots, the worker process, and what its turn meant.
//!
//! `task_dispatch` never waits for a worker. It writes the task, starts it if a
//! slot is free, and returns — the answer arrives later, as an event the engine
//! delivers when it has nothing else to do.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::json;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::acp::Decision;
use crate::error::{Error, Result};
use crate::harness::Harness;
use crate::task::{Ledger, Origin, Outcome, Report, Status, Task};

/// The most workers that may run at once.
pub const SLOTS: usize = 3;

/// A tap on the shoulder: a task moved, look at the ledger.
#[derive(Debug, Clone)]
pub struct Nudge {
    pub id: String,
}

#[derive(Debug)]
pub struct Tasks {
    ledger: Mutex<Ledger>,
    running: Mutex<BTreeMap<String, JoinHandle<()>>>,
    harness: Harness,
    nudges: UnboundedSender<Nudge>,
    permits: Mutex<HashMap<String, oneshot::Sender<Decision>>>,
    slots: usize,
}

impl Tasks {
    pub fn new(
        dir: PathBuf,
        harness: Harness,
        slots: usize,
    ) -> Result<(Arc<Self>, tokio::sync::mpsc::UnboundedReceiver<Nudge>)> {
        let (nudges, inbox) = unbounded_channel();

        let tasks = Arc::new(Self {
            ledger: Mutex::new(Ledger::at(dir)?),
            running: Mutex::new(BTreeMap::new()),
            harness,
            nudges,
            permits: Mutex::new(HashMap::new()),
            slots: slots.max(1),
        });

        Ok((tasks, inbox))
    }

    /// Writes a task and starts it if there is room.
    pub fn dispatch(self: &Arc<Self>, origin: Origin, brief: String) -> Result<String> {
        let id = {
            let mut ledger = self.ledger.lock().expect("no panic on the ledger");
            let id = ledger.reserve();
            let cwd = ledger.dir().join(&id);

            ledger.put(Task {
                cwd,
                id: id.clone(),
                origin,
                brief,
                status: Status::OnSlot,
                report: None,
                said: String::new(),
            })?;

            id
        };

        self.fill_slots();
        Ok(id)
    }

    pub fn get(&self, id: &str) -> Result<Task> {
        self.ledger
            .lock()
            .expect("no panic on the ledger")
            .get(id)
    }

    /// The tasks that still hold the core in a busy state.
    pub fn unfinished(&self) -> Vec<Task> {
        self.ledger
            .lock()
            .expect("no panic on the ledger")
            .unfinished()
    }

    pub fn all(&self) -> Vec<Task> {
        self.ledger
            .lock()
            .expect("no panic on the ledger")
            .all()
    }

    /// Gives a task waiting for words another turn — a new process, told what
    /// happened so far, because a worker is disposable and its session is not
    /// the record.
    pub fn answer(self: &Arc<Self>, id: &str, text: &str) -> Result<()> {
        let mut ledger = self.ledger.lock().expect("no panic on the ledger");
        let task = ledger.get(id)?;

        let brief = format!(
            "{}\n\n# 上一次\n{}\n\n# 对方刚说\n{}",
            task.brief, task.said, text
        );

        let mut next = Task {
            brief,
            status: Status::OnSlot,
            said: String::new(),
            ..task
        };

        next.report = None;
        ledger.put(next)?;
        drop(ledger);

        self.fill_slots();
        Ok(())
    }

    /// Answers the question a worker is stopped on.
    pub fn permit(&self, id: &str, allow: bool) -> Result<()> {
        let asker = self
            .permits
            .lock()
            .expect("no panic on the permits")
            .remove(id);

        match asker {
            Some(asker) => {
                let _ = asker.send(if allow {
                    Decision::Allow
                } else {
                    Decision::Refuse
                });
                Ok(())
            }
            None => Err(Error::Disk(format!("{id} 没有在问什么"))),
        }
    }

    /// Parks the task on the question and returns the line the answer arrives on.
    fn ask_permission(&self, id: &str, ask: &crate::acp::Ask) -> oneshot::Receiver<Decision> {
        let (sender, receiver) = oneshot::channel();

        {
            let mut ledger = self.ledger.lock().expect("no panic on the ledger");

            if let Ok(mut task) = ledger.get(id) {
                task.status = Status::OnPermission(ask.title.clone());
                let _ = ledger.put(task);
            }
        }

        self.permits
            .lock()
            .expect("no panic on the permits")
            .insert(id.to_owned(), sender);

        let _ = self.nudges.send(Nudge { id: id.to_owned() });

        receiver
    }

    /// Stops a task where it stands.
    pub fn cancel(self: &Arc<Self>, id: &str) -> Result<()> {
        if let Some(handle) = self
            .running
            .lock()
            .expect("no panic on the running map")
            .remove(id)
        {
            handle.abort();
        }

        let mut ledger = self.ledger.lock().expect("no panic on the ledger");
        let mut task = ledger.get(id)?;
        task.status = Status::Failed("取消".to_owned());
        ledger.put(task)?;
        drop(ledger);

        self.fill_slots();
        Ok(())
    }

    /// Starts queued tasks until every slot is busy.
    fn fill_slots(self: &Arc<Self>) {
        loop {
            let next = {
                let mut ledger = self.ledger.lock().expect("no panic on the ledger");

                if ledger.holding() >= self.slots {
                    return;
                }

                let Some(id) = ledger.queued() else {
                    return;
                };

                let mut task = ledger.get(&id).expect("a queued task exists");

                if let Err(error) = std::fs::create_dir_all(&task.cwd) {
                    task.status = Status::Failed(format!("{}: {error}", task.cwd.display()));
                    let _ = ledger.put(task);
                    continue;
                }

                task.status = Status::OnHarness;

                if ledger.put(task.clone()).is_err() {
                    return;
                }

                task
            };

            self.run(next);
        }
    }

    fn run(self: &Arc<Self>, task: Task) {
        let worker = self.harness.worker(&task.cwd);
        let prompt = brief_prompt(&task.brief);
        let id = task.id.clone();
        let tasks = Arc::clone(self);
        let permission: Arc<dyn crate::acp::Permission> = match task.origin {
            Origin::User => Arc::new(AskPermission {
                tasks: Arc::clone(self),
                id: task.id.clone(),
            }),
            Origin::Own => Arc::new(crate::acp::Refuse),
        };

        let handle = tokio::spawn(async move {
            let outcome = worker.talk(&prompt, permission).await;
            tasks.settle(&id, outcome);
        });

        self.running
            .lock()
            .expect("no panic on the running map")
            .insert(task.id, handle);
    }

    /// Records what the last turn meant, frees the slot, and tells the engine.
    fn settle(
        self: &Arc<Self>,
        id: &str,
        outcome: std::result::Result<crate::acp::Turn, crate::Error>,
    ) {
        {
            let mut ledger = self.ledger.lock().expect("no panic on the ledger");
            let Ok(mut task) = ledger.get(id) else {
                return;
            };

            match outcome {
                Ok(turn) => {
                    task.said = turn.said.clone();
                    task.report = Report::parse(&turn.said);

                    task.status = match task.report.as_ref().map(|report| &report.status) {
                        Some(Outcome::Need) => Status::OnInstruction,
                        Some(Outcome::Blocked) => Status::Failed(
                            task.report
                                .as_ref()
                                .and_then(|report| report.question.clone())
                                .unwrap_or_else(|| "卡住了".to_owned()),
                        ),
                        Some(Outcome::Done) | None => Status::OnTrigger,
                    };
                }
                Err(error) => task.status = Status::Failed(error.to_string()),
            }

            let _ = ledger.put(task);
        }

        self.running
            .lock()
            .expect("no panic on the running map")
            .remove(id);
        self.permits
            .lock()
            .expect("no panic on the permits")
            .remove(id);
        self.fill_slots();
        let _ = self.nudges.send(Nudge { id: id.to_owned() });
    }
}

/// A worker's question, answered by whoever calls [`Tasks::permit`].
struct AskPermission {
    tasks: Arc<Tasks>,
    id: String,
}

#[async_trait::async_trait]
impl crate::acp::Permission for AskPermission {
    async fn decide(&self, ask: &crate::acp::Ask) -> Decision {
        self.tasks
            .ask_permission(&self.id, ask)
            .await
            .unwrap_or(Decision::Refuse)
    }
}

/// What a worker is told, every time. The envelope is not optional: it is how a
/// worker's turn ends with something the core can act on.
pub fn brief_prompt(brief: &str) -> String {
    format!(
        "你在一份独立的工作目录里干活，干完这一轮就结束。\n\
         \n\
         最后一段必须是报告，长这样：\n\
         \n\
         ```report\n\
         {{\"status\": \"done\" | \"need\" | \"blocked\", \"question\": \"…\", \"artifacts\": [\"…\"]}}\n\
         ```\n\
         \n\
         done 是干完了，need 是要一句话才能继续（把问题写进 question），\
         blocked 是卡住了。除这三行以外的话会被人读到，但不会被解析。\n\
         \n\
         # 这活是什么\n\
         {brief}\n"
    )
}

/// The `task_dispatch` arguments, rendered into a brief.
pub fn brief(goal: &str, files: &[String], constraints: &[String]) -> String {
    let mut brief = goal.trim().to_owned();

    if !files.is_empty() {
        brief.push_str("\n\n# 相关的文件\n");
        for file in files {
            brief.push_str(&format!("- {file}\n"));
        }
    }

    if !constraints.is_empty() {
        brief.push_str("\n# 约束\n");
        for constraint in constraints {
            brief.push_str(&format!("- {constraint}\n"));
        }
    }

    brief
}

/// What the engine shows a worker's card as, and what `task_read` returns.
pub fn reading(task: &Task) -> String {
    let mut text = format!("# {}\n{}\n\n", task.id, task.line());

    if let Some(report) = &task.report {
        text.push_str(&format!(
            "报告：{}\n",
            serde_json::to_string(report).unwrap_or_default()
        ));
    }

    if !task.said.is_empty() {
        text.push_str("\n## 它说的最后一段\n");
        text.push_str(&task.said);
        text.push('\n');
    }

    text
}

/// A tool's reply when the task id is unknown, phrased for the caller.
pub fn unknown(id: &str) -> Error {
    Error::Disk(format!("没有这个任务：{id}"))
}

/// Kept next to the tools so a schema and its struct stay together.
pub fn dispatch_arguments() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "goal": { "type": "string", "description": "要它做什么，一两句话说清。" },
            "files": {
                "type": "array",
                "items": { "type": "string" },
                "description": "相关的文件路径。它拿到的是自己的目录，所以要写绝对路径。"
            },
            "constraints": {
                "type": "array",
                "items": { "type": "string" },
                "description": "约束：不要动哪里、用哪个分支、怎么算完成。"
            }
        },
        "required": ["goal"]
    })
}
