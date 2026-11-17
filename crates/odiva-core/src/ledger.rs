//! The task records, owned by one actor.
//!
//! Nothing here runs anything: a task is a line on disk, and the actor exists
//! so that every reader gets the same copy without a lock.

use std::path::PathBuf;

use kameo::Actor;
use kameo::actor::ActorRef;
use kameo::message::{Context, Message};
use odiva_proto::CoreMessage;
use tokio::sync::mpsc::UnboundedSender;

use crate::error::{Error, Result};
use crate::journal::Journal;
use crate::task::{Ledger, Origin, Status, Task};

pub struct Args {
    pub dir: PathBuf,
    pub out: UnboundedSender<CoreMessage>,
    pub journal: Journal,
}

pub struct Records {
    ledger: Ledger,
    out: UnboundedSender<CoreMessage>,
    journal: Journal,
}

impl Actor for Records {
    type Args = Args;
    type Error = Error;

    async fn on_start(args: Self::Args, _ctx: ActorRef<Self>) -> Result<Self> {
        Ok(Self {
            ledger: Ledger::at(args.dir)?,
            out: args.out,
            journal: args.journal,
        })
    }
}

/// Writes a new task down and answers with its id.
pub struct Reserve {
    pub origin: Origin,
    pub brief: String,
    /// Where the task works. The person's own directory when they named one;
    /// otherwise a directory of the ledger's making.
    pub workspace: Option<PathBuf>,
}

impl Message<Reserve> for Records {
    type Reply = String;

    async fn handle(&mut self, msg: Reserve, _ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {
        let id = self.ledger.reserve();
        let task = Task {
            cwd: msg
                .workspace
                .unwrap_or_else(|| self.ledger.dir().join(&id)),
            id: id.clone(),
            origin: msg.origin,
            brief: msg.brief,
            status: Status::OnSlot,
            report: None,
            said: String::new(),
        };

        self.put(task);
        id
    }
}

pub struct Lookup {
    pub id: String,
}

impl Message<Lookup> for Records {
    type Reply = Option<Task>;

    async fn handle(&mut self, msg: Lookup, _ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {
        self.ledger.get(&msg.id).ok()
    }
}

pub struct Everything;

impl Message<Everything> for Records {
    type Reply = Vec<Task>;

    async fn handle(&mut self, _msg: Everything, _ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {
        self.ledger.all()
    }
}

pub struct Unfinished;

impl Message<Unfinished> for Records {
    type Reply = Vec<Task>;

    async fn handle(&mut self, _msg: Unfinished, _ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {
        self.ledger.unfinished()
    }
}

/// One task, as it stands now.
pub struct Update(pub Task);

impl Message<Update> for Records {
    type Reply = ();

    async fn handle(&mut self, msg: Update, _ctx: &mut Context<Self, Self::Reply>) {
        self.put(msg.0);
    }
}

impl Records {
    fn put(&mut self, task: Task) {
        if let Err(error) = self.ledger.put(task) {
            self.fail(&error);
        }
    }

    fn fail(&self, error: &Error) {
        self.journal.line(format!("台账：{error}"));
        let _ = self.out.send(CoreMessage::CoreFailed {
            note: format!("台账：{error}"),
        });
    }
}
