//! One task's turn in a harness: started, watched, and reported back.

use std::sync::Arc;

use kameo::actor::ActorRef;
use odiva_harness::{Ask, Decision, Permission};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::harness::Harness;
use crate::odiva::{Event, Odiva};
use crate::policy::{Roots, Verdict, classify};
use crate::task::{Origin, Task};

/// A run in flight: its identity, and the way to stop it.
pub struct Run {
    pub id: u64,
    pub cancel: CancellationToken,
}

pub fn start(id: u64, task: &Task, harness: &Harness, odiva: ActorRef<Odiva>) -> Run {
    let worker = harness.worker(&task.cwd);
    let prompt = crate::brief::prompt(&task.brief);
    let cancel = CancellationToken::new();

    let permission: Arc<dyn Permission> = Arc::new(Policy {
        task: task.id.clone(),
        roots: Roots::new(&task.cwd),
        origin: task.origin,
        odiva: odiva.clone(),
    });

    let task_id = task.id.clone();
    let token = cancel.clone();

    tokio::spawn(async move {
        let outcome = worker
            .talk_until(&prompt, permission, token)
            .await
            .map_err(|error| error.to_string());

        let _ = odiva
            .tell(Event::Ended {
                run: id,
                task: task_id,
                outcome,
            })
            .await;
    });

    Run { id, cancel }
}

/// The policy, and the person behind it.
struct Policy {
    task: String,
    roots: Roots,
    origin: Origin,
    odiva: ActorRef<Odiva>,
}

#[async_trait::async_trait]
impl Permission for Policy {
    async fn decide(&self, ask: &Ask) -> Decision {
        match classify(ask, &self.roots, self.origin) {
            Verdict::Allow => Decision::Allow,
            Verdict::Deny(_) => Decision::Refuse,
            Verdict::AgentMayDecide => self.ask(ask, false).await,
            Verdict::AskUser(_) => self.ask(ask, true).await,
        }
    }
}

impl Policy {
    /// Carries the question to whoever can answer it. While she is inside a
    /// turn, the answer waits for that turn to end; the worker waits too, which
    /// is what "stopped on a question" means.
    async fn ask(&self, ask: &Ask, escalate: bool) -> Decision {
        let (answer, told) = oneshot::channel();

        let sent = self
            .odiva
            .tell(Event::Asked {
                task: self.task.clone(),
                ask: ask.clone(),
                answer,
                escalate,
            })
            .await;

        if sent.is_err() {
            return Decision::Refuse;
        }

        told.await.unwrap_or(Decision::Refuse)
    }
}
