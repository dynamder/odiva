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
use std::sync::Arc;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::util::MatchDispatch;
use agent_client_protocol::v1::SessionMessage;
use agent_client_protocol::v1::schema::{
    ContentBlock, ContentChunk, InitializeRequest, PermissionOption, PermissionOptionKind,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, SessionUpdate, StopReason,
};
use agent_client_protocol::{ByteStreams, Client, on_receive_request};
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

/// What a worker wants to do that it may not do on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub title: String,
}

/// What the core says to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Refuse,
}

/// Who answers a worker's question.
///
/// Deciding may take as long as it takes: a worker that asked for something is
/// stopped, and stopped is the correct state to be in until someone answers.
#[async_trait::async_trait]
pub trait Permission: Send + Sync {
    async fn decide(&self, ask: &Ask) -> Decision;
}

/// The default answer: nobody is watching a worker.
pub struct Refuse;

#[async_trait::async_trait]
impl Permission for Refuse {
    async fn decide(&self, _ask: &Ask) -> Decision {
        Decision::Refuse
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

    /// The command to start, with a shell shim unwrapped.
    ///
    /// A package manager installs CLIs as `.cmd` on Windows, and the system's
    /// process spawner will not run one directly; going through `cmd /C` is the
    /// only way to start what the user configured.
    fn program_command(&self) -> tokio::process::Command {
        let shim = self
            .program
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
            });

        if !shim {
            return tokio::process::Command::new(&self.program);
        }

        let mut command = tokio::process::Command::new("cmd");
        command.arg("/C").arg(&self.program);
        command
    }

    /// Starts the harness, asks it one thing, and waits for it to stop talking.
    ///
    /// The child inherits this process's stderr, so a harness's own complaints
    /// stay visible instead of filling a pipe nobody reads. A permission request
    /// is answered by `permission`, and answering it is allowed to take a while:
    /// the worker waits, which is what "stopped on a question" looks like.
    pub async fn talk(
        &self,
        prompt: &str,
        permission: Arc<dyn Permission>,
    ) -> crate::Result<Turn> {
        let mut child = self
            .program_command()
            .args(&self.args)
            .current_dir(&self.cwd)
            .envs(&self.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                crate::Error::Harness(format!("cannot start {}: {error}", self.program.display()))
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
        let asked = Arc::clone(&permission);

        Client
            .v1()
            .on_receive_request(
                async move |request: RequestPermissionRequest, responder, _connection| {
                    let permission = Arc::clone(&asked);
                    let ask = Ask {
                        title: request
                            .tool_call
                            .fields
                            .title
                            .clone()
                            .unwrap_or_else(|| "要动工作区之外的什么东西".to_owned()),
                    };
                    let options = request.options.clone();

                    tokio::spawn(async move {
                        let decision = permission.decide(&ask).await;
                        let _ = responder.respond(reply(decision, &options));
                    });

                    Ok(())
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
            .map_err(Into::into)
    }

    /// For callers that only ever refuse.
    pub async fn talk_refusing(&self, prompt: &str) -> crate::Result<Turn> {
        self.talk(prompt, Arc::new(Refuse)).await
    }
}

/// Answers by name, never by position: the narrow form of whatever was decided,
/// or a cancellation when the harness offered no way to say it.
fn reply(decision: Decision, options: &[PermissionOption]) -> RequestPermissionResponse {
    let wanted = match decision {
        Decision::Allow => [
            PermissionOptionKind::AllowOnce,
            PermissionOptionKind::AllowAlways,
        ],
        Decision::Refuse => [
            PermissionOptionKind::RejectOnce,
            PermissionOptionKind::RejectAlways,
        ],
    };

    let chosen = wanted
        .iter()
        .find_map(|kind| options.iter().find(|option| option.kind == *kind))
        .map(|option| option.option_id.clone());

    let outcome = match chosen {
        Some(option_id) => {
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id))
        }
        None => RequestPermissionOutcome::Cancelled,
    };

    RequestPermissionResponse::new(outcome)
}
