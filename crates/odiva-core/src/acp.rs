//! The ACP client: a harness in a child process, one turn at a time.
//!
//! One `talk` is one process: it starts, it is initialized, it answers a single
//! prompt, it is dropped. Nothing here outlives the call, which is what makes a
//! worker disposable — a task that needs another turn starts another process.
//!
//! ACP types stay inside this module: the rest of the core asks for a [`Turn`],
//! not for a `PromptResponse`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::util::MatchDispatch;
use agent_client_protocol::v1::SessionMessage;
use agent_client_protocol::v1::schema::{
    ContentBlock, ContentChunk, InitializeRequest, PermissionOptionKind, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome,
    SessionNotification, SessionUpdate, StopReason,
};
use agent_client_protocol::{ByteStreams, Client, Error, Result, on_receive_request};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// What a harness said in one turn, and why it stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub said: String,
    pub stop: Stop,
}

/// Why a turn ended. Only [`Stop::Finished`] means the harness did what it was
/// asked; the rest are all "it stopped early", kept apart because the task
/// ledger records a reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// It said everything it had, and is waiting for the next instruction.
    Finished,
    /// It ran out of room before finishing.
    Truncated,
    /// It refused the prompt.
    Refused,
    /// Something cancelled the turn.
    Cancelled,
    /// The protocol named a reason this build does not model.
    Unrecognized,
}

impl From<StopReason> for Stop {
    fn from(reason: StopReason) -> Self {
        match reason {
            StopReason::EndTurn => Self::Finished,
            StopReason::MaxTokens | StopReason::MaxTurnRequests => Self::Truncated,
            StopReason::Refusal => Self::Refused,
            StopReason::Cancelled => Self::Cancelled,
            _ => Self::Unrecognized,
        }
    }
}

/// How to start one harness, and where it is allowed to work.
///
/// `cwd` is not optional: it is what the harness treats as its workspace, and
/// therefore the boundary it asks permission to cross.
#[derive(Debug, Clone)]
pub struct Worker {
    program: PathBuf,
    args: Vec<String>,
    cwd: PathBuf,
    env: BTreeMap<String, String>,
}

impl Worker {
    pub fn new(program: impl Into<PathBuf>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: cwd.into(),
            env: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    #[must_use]
    pub fn env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(name.into(), value.into());
        self
    }

    /// Starts the harness, asks it one thing, and waits for it to stop talking.
    ///
    /// The child inherits this process's stderr, so a harness's own complaints
    /// stay visible instead of filling a pipe nobody reads. Every permission it
    /// asks for is refused: nobody is watching a worker, so it must not be able
    /// to do something that needed a person.
    pub async fn talk(&self, prompt: &str) -> Result<Turn> {
        let mut child = tokio::process::Command::new(&self.program)
            .args(&self.args)
            .current_dir(&self.cwd)
            .envs(&self.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                Error::internal_error().data(format!(
                    "cannot start {}: {error}",
                    self.program.display()
                ))
            })?;

        let outgoing = child
            .stdin
            .take()
            .expect("stdin is piped")
            .compat_write();
        let incoming = child
            .stdout
            .take()
            .expect("stdout is piped")
            .compat();

        let prompt = prompt.to_owned();
        let cwd = self.cwd.clone();

        Client
            .v1()
            .on_receive_request(
                async |request: RequestPermissionRequest, responder, _connection| {
                    responder.respond(refusal(&request))
                },
                on_receive_request!(),
            )
            .connect_with(ByteStreams::new(outgoing, incoming), async move |connection| {
                let _child = child;

                connection
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;

                connection
                    .build_session(cwd)
                    .block_task()
                    .run_until(async move |setup| {
                        let mut session = setup.session;
                        session.send_prompt(prompt)?;

                        let mut said = String::new();

                        loop {
                            match session.read_update().await? {
                                SessionMessage::PromptResponse(response) => {
                                    return Ok(Turn {
                                        said,
                                        stop: Stop::from(response?.stop_reason),
                                    });
                                }
                                SessionMessage::Message(message) => {
                                    MatchDispatch::new(message.into_raw())
                                        .if_notification(async |notification: SessionNotification| {
                                            if let SessionUpdate::AgentMessageChunk(
                                                ContentChunk {
                                                    content: ContentBlock::Text(text),
                                                    ..
                                                },
                                            ) = notification.update
                                            {
                                                said.push_str(&text.text);
                                            }

                                            Ok(())
                                        })
                                        .await
                                        .otherwise_ignore()?;
                                }
                                _ => {}
                            }
                        }
                    })
                    .await
            })
            .await
    }
}

/// Refuses a permission request by name, falling back to cancelling it when the
/// harness offered no way to say no.
fn refusal(request: &RequestPermissionRequest) -> RequestPermissionResponse {
    let reject = request
        .options
        .iter()
        .find(|option| option.kind == PermissionOptionKind::RejectOnce)
        .or_else(|| {
            request
                .options
                .iter()
                .find(|option| option.kind == PermissionOptionKind::RejectAlways)
        })
        .map(|option| option.option_id.clone());

    let outcome = match reject {
        Some(option_id) => {
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id))
        }
        None => RequestPermissionOutcome::Cancelled,
    };

    RequestPermissionResponse::new(outcome)
}
