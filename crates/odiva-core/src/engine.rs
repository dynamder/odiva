//! The engine: the part that stays.
//!
//! It owns the paths, the ids, the queue of things waiting to be said to her,
//! and the task ledger. Everything that thinks is an activation it builds and
//! drops.

use std::sync::Arc;

use odiva_proto::{CoreHandle, CoreMessage, Presence, Snapshot, UiMessage, WireMessage};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::error::Result;
use crate::harness::Harness;
use crate::ids::Ids;
use crate::roster::Roster;
use crate::route::Route;
use crate::say::{Said, Say};
use crate::store::{Lane, Store, TALK};
use crate::talk::Talk;
use crate::task::Origin;
use crate::parked::{Parked, RequestIdle};
use crate::tasks::{Nudge, Tasks, reading};
use crate::tools;
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
    /// The busy period in progress, if any: a work lane's generation.
    work: Option<u32>,
    /// Whether she asked to be idle until something happens.
    parked: Parked,
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
            work: None,
            parked: Parked::default(),
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

    /// The whole window, from the lane on disk.
    fn attach(&self) {
        let generation = self.store.newest(TALK);

        let snapshot = match self.store.read(Lane::Talk { generation }) {
            Ok(Some(log)) => Snapshot {
                messages: log.messages().to_vec(),
                presence: Presence::Online,
            },
            _ => Snapshot::default(),
        };

        self.emit(CoreMessage::CoreAttachedSession { snapshot });
    }

    /// Something the person said.
    async fn turn(&mut self, text: String, inbox: &mut UnboundedReceiver<UiMessage>) {
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
            eprintln!("odiva: a turn failed: {error}");
        }
    }

    async fn run_activation(
        &mut self,
        prompt: &str,
        user: Option<&WireMessage>,
        inbox: &mut UnboundedReceiver<UiMessage>,
    ) -> Result<()> {
        let said = Said::default();
        let busy = self.work_generation();
        let mut talk = match busy {
            Some(generation) => {
                let jobs = self
                    .tasks
                    .unfinished()
                    .iter()
                    .map(|task| task.line())
                    .collect();

                Talk::busy(
                    &self.store,
                    generation,
                    &*self.route,
                    &self.prompt,
                    self.roster(&said, true),
                    said,
                    jobs,
                )?
            }
            None => Talk::open(
                &self.store,
                &*self.route,
                &self.prompt,
                self.roster(&said, false),
                said,
            )?,
        };

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
        talk.flush()?;

        if talk.used() * 100 >= CONTEXT_WINDOW * COMPACT_AT {
            talk.compact(KEEP)?;
        }

        Ok(())
    }

    /// The work lane this busy period runs on, opening one if the period is new.
    fn work_generation(&mut self) -> Option<u32> {
        if self.parked.is_parked() {
            return None;
        }

        if self.tasks.unfinished().is_empty() {
            self.work = None;
            return None;
        }

        if let Some(generation) = self.work {
            return Some(generation);
        }

        let newest = self.store.newest("work");
        let occupied = self
            .store
            .read(Lane::Work { generation: newest })
            .map(|log| log.is_some())
            .unwrap_or(false);

        let generation = if occupied { newest + 1 } else { newest };

        self.work = Some(generation);
        Some(generation)
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
