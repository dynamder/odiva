//! The window's two ends of one wire: what it says, and what it is told.
//!
//! A window can be built without a core — screenshots and protocol tests are —
//! so both ends are optional. The inbox is consumed by the subscription, which
//! is the only thing that can move the receiver.

use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use iced::Subscription;
use iced::futures::stream::{self, Stream};
use odiva_proto::{CoreHandle, CoreMessage, UiMessage};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::Message;

/// Both directions of the connection to a core.
#[derive(Clone, Default)]
pub(crate) struct Wire {
    handle: Option<Arc<dyn CoreHandle>>,
    inbox: Option<Inbox>,
}

impl Wire {
    pub(crate) fn connected(
        handle: impl CoreHandle,
        messages: UnboundedReceiver<CoreMessage>,
    ) -> Self {
        Self {
            handle: Some(Arc::new(handle)),
            inbox: Some(Inbox::new(messages)),
        }
    }

    pub(crate) fn send(&self, message: UiMessage) {
        if let Some(handle) = &self.handle {
            handle.send(message);
        }
    }

    /// The inbox as an iced subscription.
    ///
    /// The recipe's identity is the boxed receiver, not its contents, so every
    /// call returns the same recipe and iced keeps the stream it already runs.
    pub(crate) fn subscription(&self) -> Subscription<Message> {
        match &self.inbox {
            Some(inbox) => {
                Subscription::run_with(inbox.clone(), Inbox::stream).map(Message::CoreReported)
            }
            None => Subscription::none(),
        }
    }
}

#[derive(Clone)]
struct Inbox {
    messages: Arc<Mutex<Option<UnboundedReceiver<CoreMessage>>>>,
}

impl Inbox {
    fn new(messages: UnboundedReceiver<CoreMessage>) -> Self {
        Self {
            messages: Arc::new(Mutex::new(Some(messages))),
        }
    }

    /// `use<>` keeps the stream's type free of the borrow, which is what lets
    /// [`Subscription::run_with`] take a `fn(&Inbox) -> S`.
    fn stream(&self) -> impl Stream<Item = CoreMessage> + use<> {
        let messages = self.messages.lock().expect("no panic on the inbox").take();

        stream::unfold(messages, |messages| async move {
            let mut messages = messages?;

            messages
                .recv()
                .await
                .map(|message| (message, Some(messages)))
        })
    }
}

impl Hash for Inbox {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.messages).hash(state);
    }
}
