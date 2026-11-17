//! The engine: the part that stays.
//!
//! It owns the paths, the ids, the queue of things waiting to be said to her,
//! and the task ledger. Everything that thinks is an activation it builds and
//! drops.

use std::sync::Arc;

use odiva_proto::{CoreHandle, CoreMessage, Presence, Snapshot, UiMessage, WireMessage};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::error::Result;
use crate::handoff::Handoff;
use crate::harness::Harness;
use crate::ids::Ids;
use crate::journal::Journal;
use crate::line::{LineId, Mode};
use crate::parked::{Parked, RequestIdle};
use crate::roster::Roster;
use crate::route::Route;
use crate::say::{Said, Say};
use crate::store::Store;
use crate::talk::Talk;
use crate::task::Origin;
use crate::tasks::{Nudge, Tasks, reading};
use crate::tools;
use crate::transcript::Transcript;
use crate::waiting::Waiting;

/// The context window compaction is measured against.
const CONTEXT_WINDOW: u64 = 128_000;

/// Where in the window a lane rotates.
const COMPACT_AT: u64 = 60;

/// How many messages survive a rotation.
const KEEP: usize = 6;

/// The window's end of the wire.
#[derive(Debug, Clone)]
pub struct Handle {
    out: UnboundedSender<UiMessage>,
}

impl CoreHandle for Handle {
    fn send(&self, message: UiMessage) {
        let _ = self.out.send(message);
    }
}

/// What survives a turn: the store, the counter, the ledger, who is waiting.
pub struct Engine {
    store: Store,
    ids: Ids,
    route: Arc<dyn Route>,
    prompt: String,
    out: UnboundedSender<CoreMessage>,
    inbox: Option<UnboundedReceiver<UiMessage>>,
    nudges: Option<UnboundedReceiver<Nudge>>,
    waiting: Waiting,
    tasks: Arc<Tasks>,
    /// Whether she asked to be idle until something happens.
    parked: Parked,
    /// Where the core writes down what it did.
    journal: Journal,
}

impl Engine {
    pub fn new(
        store: Store,
        route: Arc<dyn Route>,
        prompt: impl Into<String>,
        harness: Harness,
    ) -> Result<(Self, Handle, UnboundedReceiver<CoreMessage>)> {
        let (out, messages) = unbounded_channel();
        let (handle, inbox) = unbounded_channel();
        let (tasks, nudges) = Tasks::new(store.tasks(), harness, crate::tasks::SLOTS)?;
        let journal = Journal::new(store.root().join("odiva.log"));

        let engine = Self {
            store,
            ids: Ids::default(),
            route,
            prompt: prompt.into(),
            out,
            inbox: Some(inbox),
            nudges: Some(nudges),
            waiting: Waiting::default(),
            tasks,
            parked: Parked::default(),
            journal,
        };

        Ok((engine, Handle { out: handle }, messages))
    }

    /// Attaches, then answers as long as the window is there.
    pub async fn run(mut self) {
        let (Some(mut inbox), Some(mut nudges)) = (self.inbox.take(), self.nudges.take()) else {
            return;
        };

        self.attach();

        loop {
            tokio::select! {
                message = inbox.recv() => {
                    let Some(message) = message else {
                        break;
                    };

                    match message {
                        UiMessage::UserSubmittedText { text } => {
                            self.turn(text, &mut inbox).await;

                            while let Some(queued) = self.waiting.next() {
                                self.turn(queued, &mut inbox).await;
                            }
                        }
                        UiMessage::UserRequestedStop => {}
                    }
                }
                nudge = nudges.recv() => {
                    let Some(nudge) = nudge else {
                        continue;
                    };

                    self.notice(&nudge.id, &mut inbox).await;
                }
            }
        }
    }

    /// The whole window, from the record on disk.
    fn attach(&self) {
        let snapshot = match Transcript::new(self.store.transcript()).read() {
            Ok(messages) => Snapshot {
                messages,
                presence: Presence::Online,
            },
            Err(error) => {
                self.emit(CoreMessage::CoreFailed {
                    note: format!("读不了窗口的记录：{error}"),
                });

                Snapshot::default()
            }
        };

        self.emit(CoreMessage::CoreAttachedSession { snapshot });
    }

    /// Something the person said.
    async fn turn(&mut self, text: String, inbox: &mut UnboundedReceiver<UiMessage>) {
        self.journal.line(format!("对方：{text}"));

        let user = self.ids.theirs(text);

        self.emit(CoreMessage::MessageAppended {
            message: user.clone(),
        });

        self.answer(&user.markdown, Some(&user), inbox).await;
    }

    /// Something that happened outside the conversation.
    ///
    /// Not a user message: it enters the model's context and nothing else, so
    /// the transcript stays a record of what was actually said.
    async fn notice(&mut self, id: &str, inbox: &mut UnboundedReceiver<UiMessage>) {
        self.parked.wake();

        let Ok(task) = self.tasks.get(id) else {
            return;
        };

        self.journal
            .line(format!("任务 {}：{}", task.id, task.line()));

        if let crate::task::Status::Failed(reason) = &task.status {
            self.emit(CoreMessage::CoreFailed {
                note: format!("{} 失败了：{reason}", task.id),
            });
        }

        let text = format!(
            "（后台的变化，不是对方说的）\n\n{}\n\n要不要说、说什么，你决定。",
            reading(&task)
        );

        self.answer(&text, None, inbox).await;
    }

    async fn answer(
        &mut self,
        prompt: &str,
        user: Option<&WireMessage>,
        inbox: &mut UnboundedReceiver<UiMessage>,
    ) {
        self.presence(Presence::Typing);

        let outcome = self.run_activation(prompt, user, inbox).await;

        self.presence(Presence::Online);

        if let Err(error) = outcome {
            let note = format!("这一轮没跑完：{error}");

            self.journal.line(format!("失败：{error}"));
            self.emit(CoreMessage::CoreFailed { note: note.clone() });
            eprintln!("odiva: {note}");
        }
    }

    async fn run_activation(
        &mut self,
        prompt: &str,
        user: Option<&WireMessage>,
        inbox: &mut UnboundedReceiver<UiMessage>,
    ) -> Result<()> {
        let said = Said::default();
        let busy = self.in_busy();
        let id = LineId::normal(if busy { Mode::Busy } else { Mode::Idle });

        let jobs: Vec<String> = self
            .tasks
            .unfinished()
            .iter()
            .map(|task| task.line())
            .collect();

        let seed = busy.then(|| Handoff::of(user, &jobs).render());

        let mut talk = Talk::open(
            &self.store,
            id,
            &*self.route,
            &self.prompt,
            self.roster(&said, busy),
            said,
            seed,
        )?;

        if let Some(user) = user {
            talk.note(user)?;
        }

        let mut running = Box::pin(talk.run(prompt));

        loop {
            tokio::select! {
                result = &mut running => {
                    result?;
                    break;
                }
                message = inbox.recv() => match message {
                    Some(UiMessage::UserRequestedStop) | None => break,
                    Some(UiMessage::UserSubmittedText { text }) => self.waiting.offer(text),
                },
            }
        }

        drop(running);

        if !talk.spoke() && talk.failure().is_none() {
            talk.run(crate::prompt::AFTER_SILENCE).await?;
        }

        let spoke = talk.spoke();
        talk.flush()?;

        if let Some(reason) = talk.failure() {
            return Err(crate::Error::Brain(reason));
        }

        let monologue = talk.monologue();

        match (&monologue, spoke) {
            (Some(words), false) => {
                self.journal.line(format!("她没说话；模型的原话：{words}"));
            }
            (None, false) => {
                self.journal.line("她没说话；模型也没有回话");
                return Err(crate::Error::Brain(
                    "模型没有回话（没有 key，或者 provider 报错）".to_owned(),
                ));
            }
            _ => {}
        }

        if talk.used() * 100 >= CONTEXT_WINDOW * COMPACT_AT {
            talk.compact(KEEP)?;
        }

        Ok(())
    }

    /// Busy is a fact, not a mode she chose: a task is unfinished and nobody has
    /// asked for idle. The line follows from it.
    fn in_busy(&self) -> bool {
        !self.parked.is_parked() && !self.tasks.unfinished().is_empty()
    }

    fn roster(&self, said: &Said, busy: bool) -> Roster {
        let mut tools = Vec::new();

        tools.push(Box::new(Say::new(
            self.out.clone(),
            self.ids.clone(),
            said.clone(),
        )) as Box<dyn yoagent::types::AgentTool>);

        tools.extend(tools::roster(&self.tasks, Origin::User));

        if busy {
            tools.push(Box::new(RequestIdle::new(self.parked.clone())));
        }

        Roster::of(tools).sourcing(Arc::new(self.waiting.clone()))
    }

    fn emit(&self, message: CoreMessage) {
        let _ = self.out.send(message);
    }

    fn presence(&self, presence: Presence) {
        self.emit(CoreMessage::PresenceChanged { presence });
    }
}
