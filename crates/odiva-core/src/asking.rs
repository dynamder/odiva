//! The one question that may be outstanding, and the line the answer arrives on.
//!
//! A tool that asks cannot wait on the actor's mailbox: the actor is inside the
//! turn that called the tool, so a message would sit in the mailbox until the
//! tool returned. The window's answer therefore joins "stop" in the small set of
//! things that reach a running turn from outside it.

use std::sync::{Arc, Mutex};

use odiva_proto::{Answer, Question};
use tokio::sync::oneshot;

#[derive(Clone, Default)]
pub struct Asking {
    inner: Arc<Mutex<Option<Pending>>>,
}

struct Pending {
    answer: oneshot::Sender<Option<Answer>>,
}

impl Asking {
    /// Registers a question and answers with what the tool waits on.
    pub fn begin(&self, _question: Question) -> oneshot::Receiver<Option<Answer>> {
        let (answer, told) = oneshot::channel();

        *self.inner.lock().expect("no panic on a question") = Some(Pending { answer });

        told
    }

    /// The person picked an option. `false` when there was nothing to answer.
    pub fn answer(&self, answer: Answer) -> bool {
        match self.inner.lock().expect("no panic on a question").take() {
            Some(pending) => pending.answer.send(Some(answer)).is_ok(),
            None => false,
        }
    }

    /// Nobody is going to answer: the turn was stopped. `false` when nothing was
    /// outstanding.
    pub fn withdraw(&self) -> bool {
        match self.inner.lock().expect("no panic on a question").take() {
            Some(pending) => pending.answer.send(None).is_ok(),
            None => false,
        }
    }
}
