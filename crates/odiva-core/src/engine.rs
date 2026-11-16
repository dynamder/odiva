//! The engine: the part that stays.
//!
//! It owns the paths, the ids, the queue of things waiting to be said to her, and
//! nothing else. Everything that thinks is an activation it builds and drops.

use std::sync::Arc;

use odiva_proto::{CoreHandle, CoreMessage, Presence, Snapshot, UiMessage, WireMessage};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::error::Result;
use crate::ids::Ids;
use crate::roster::Roster;
use crate::route::Route;
use crate::say::{Said, Say};
use crate::store::{Lane, Store, TALK};
use crate::talk::Talk;
use crate::waiting::Waiting;

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

/// What survives a turn: the store, the counter, and who is waiting.
pub struct Engine {
    store: Store,
    ids: Ids,
    route: Arc<dyn Route>,
    prompt: String,
    out: UnboundedSender<CoreMessage>,
    inbox: Option<UnboundedReceiver<UiMessage>>,
    waiting: Waiting,
}

impl Engine {
    pub fn new(
        store: Store,
        route: Arc<dyn Route>,
        prompt: impl Into<String>,
    ) -> (Self, Handle, UnboundedReceiver<CoreMessage>) {
        let (out, messages) = unbounded_channel();
        let (handle, inbox) = unbounded_channel();

        let engine = Self {
            store,
            ids: Ids::default(),
            route,
            prompt: prompt.into(),
            out,
            inbox: Some(inbox),
            waiting: Waiting::default(),
        };

        (engine, Handle { out: handle }, messages)
    }

    /// Attaches, then answers as long as the window is there.
    pub async fn run(mut self) {
        let Some(mut inbox) = self.inbox.take() else {
            return;
        };

        self.attach();

        while let Some(message) = inbox.recv().await {
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

    async fn turn(&mut self, text: String, inbox: &mut UnboundedReceiver<UiMessage>) {
        let user = self.ids.theirs(text);

        self.emit(CoreMessage::MessageAppended {
            message: user.clone(),
        });
        self.presence(Presence::Typing);

        let outcome = self.answer(&user, inbox).await;

        self.presence(Presence::Online);

        if let Err(error) = outcome {
            eprintln!("odiva: a turn failed: {error}");
        }
    }

    async fn answer(
        &mut self,
        user: &WireMessage,
        inbox: &mut UnboundedReceiver<UiMessage>,
    ) -> Result<()> {
        let said = Said::default();
        let mut talk = Talk::open(
            &self.store,
            &*self.route,
            &self.prompt,
            self.roster(&said),
            said,
        )?;

        talk.note(user)?;

        let mut running = Box::pin(talk.run(&user.markdown));

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
        talk.flush()
    }

    fn roster(&self, said: &Said) -> Roster {
        Roster::of(vec![Box::new(Say::new(
            self.out.clone(),
            self.ids.clone(),
            said.clone(),
        ))])
        .sourcing(Arc::new(self.waiting.clone()))
    }

    fn emit(&self, message: CoreMessage) {
        let _ = self.out.send(message);
    }

    fn presence(&self, presence: Presence) {
        self.emit(CoreMessage::PresenceChanged { presence });
    }
}
