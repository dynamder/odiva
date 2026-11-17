//! The ACP client: a harness in a child process, one turn at a time.
//!
//! One `talk` is one process: it starts, it is initialized, it answers a single
//! prompt, it is dropped. Nothing here outlives the call, which is what makes a
//! worker disposable — a task that needs another turn starts another process.
//!
//! ACP types stay inside this module: the rest of the core asks for a [`Turn`],
//! not for a `PromptResponse`.

use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::process::Stdio;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::util::MatchDispatch;
use agent_client_protocol::v1::SessionMessage;
use agent_client_protocol::v1::schema::{
    ContentBlock, ContentChunk, InitializeRequest, PermissionOption, PermissionOptionKind,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, SessionUpdate, StopReason, ToolKind,
};
use agent_client_protocol::{ByteStreams, Client, on_receive_request};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// The last lines a harness wrote to its stderr.
///
/// A windowed app has no terminal, so a harness that dies on startup would only
/// say "transport closed". Keeping its own words turns that into a diagnosis.
#[derive(Clone, Default)]
struct Tail(Arc<Mutex<VecDeque<String>>>);

const TAIL_LINES: usize = 40;

impl Tail {
    fn push(&self, line: String) {
        let mut lines = self.0.lock().expect("no panic on the tail");

        if lines.len() == TAIL_LINES {
            lines.pop_front();
        }

        lines.push_back(line);
    }

    fn render(&self) -> String {
        let lines = self.0.lock().expect("no panic on the tail");
        lines.iter().cloned().collect::<Vec<_>>().join("\n")
    }
}

/// A started child, and the reader that keeps its last words.
struct Spawned {
    child: tokio::process::Child,
    tail: Tail,
}

/// What a harness can do, without spending a turn on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// The session `session/new` handed out.
    pub session: String,
    /// Whether it advertises `session/load`.
    pub load_session: bool,
}

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
///
/// Structured on purpose: an escalation policy has to know *what* is being
/// asked — the kind of tool, the paths, the raw input — not just a sentence.
#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    /// The harness's own wording, for whoever has to read it.
    pub title: String,
    /// What kind of thing it wants to do, when the harness says.
    pub tool: Option<Tool>,
    /// The paths it says it wants to touch.
    pub locations: Vec<PathBuf>,
    /// The arguments it wants to run with, verbatim.
    pub input: Option<serde_json::Value>,
}

/// A tool kind, without the protocol's type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Read,
    Edit,
    Delete,
    Move,
    Search,
    Execute,
    Think,
    Fetch,
    SwitchMode,
    Other,
}

impl From<ToolKind> for Tool {
    fn from(kind: ToolKind) -> Self {
        match kind {
            ToolKind::Read => Self::Read,
            ToolKind::Edit => Self::Edit,
            ToolKind::Delete => Self::Delete,
            ToolKind::Move => Self::Move,
            ToolKind::Search => Self::Search,
            ToolKind::Execute => Self::Execute,
            ToolKind::Think => Self::Think,
            ToolKind::Fetch => Self::Fetch,
            ToolKind::SwitchMode => Self::SwitchMode,
            _ => Self::Other,
        }
    }
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
    fn spawn(&self) -> crate::Result<Spawned> {
        let mut child = self
            .program_command()
            .args(&self.args)
            .current_dir(&self.cwd)
            .envs(&self.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                crate::Error::Harness(format!("cannot start {}: {error}", self.program.display()))
            })?;

        let tail = Tail::default();

        if let Some(stderr) = child.stderr.take() {
            let sink = tail.clone();

            tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt;

                let mut lines = tokio::io::BufReader::new(stderr).lines();

                while let Ok(Some(line)) = lines.next_line().await {
                    sink.push(line);
                }
            });
        }

        Ok(Spawned { child, tail })
    }

    /// An error that carries whatever the harness said before it died.
    fn failed(&self, error: agent_client_protocol::Error, tail: &Tail) -> crate::Error {
        let said = tail.render();

        if said.trim().is_empty() {
            return crate::Error::Protocol(error);
        }

        crate::Error::Harness(format!("{error:?}\n--- harness stderr ---\n{said}"))
    }

    fn program_command(&self) -> tokio::process::Command {
        let program = resolve(&self.program);

        let shim = program
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
            });

        if !shim {
            return tokio::process::Command::new(program);
        }

        let mut command = tokio::process::Command::new("cmd");
        command.arg("/C").arg(program);
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
        let Spawned { mut child, tail } = self.spawn()?;

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
                    let fields = &request.tool_call.fields;
                    let ask = Ask {
                        title: fields
                            .title
                            .clone()
                            .unwrap_or_else(|| "要动工作区之外的什么东西".to_owned()),
                        tool: fields.kind.map(Tool::from),
                        locations: fields
                            .locations
                            .iter()
                            .flatten()
                            .map(|location| location.path.clone())
                            .collect(),
                        input: fields.raw_input.clone(),
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
            .map_err(|error| self.failed(error, &tail))
    }

    /// Starts the harness and asks only what it can do: no model, no prompt.
    ///
    /// This is the check a person runs before trusting the wiring — a handshake
    /// and a session, then the process is dropped.
    pub async fn probe(&self) -> crate::Result<Probe> {
        let Spawned { mut child, tail } = self.spawn()?;

        let outgoing = child.stdin.take().expect("stdin is piped").compat_write();
        let incoming = child.stdout.take().expect("stdout is piped").compat();
        let cwd = self.cwd.clone();

        Client
            .v1()
            .connect_with(ByteStreams::new(outgoing, incoming), async move |connection| {
                let _child = child;

                let initialize = connection
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;

                let load_session = initialize.agent_capabilities.load_session;

                connection
                    .build_session(cwd)
                    .block_task()
                    .run_until(async move |setup| {
                        Ok::<Probe, agent_client_protocol::Error>(Probe {
                            session: setup.response.session_id.0.to_string(),
                            load_session,
                        })
                    })
                    .await
            })
            .await
            .map_err(|error| self.failed(error, &tail))
    }

    /// For callers that only ever refuse.
    pub async fn talk_refusing(&self, prompt: &str) -> crate::Result<Turn> {
        self.talk(prompt, Arc::new(Refuse)).await
    }

    /// A turn that can be cut short. Dropping `talk` drops the child with it,
    /// which is the only way to stop a harness that has stopped listening.
    pub async fn talk_until(
        &self,
        prompt: &str,
        permission: Arc<dyn Permission>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> crate::Result<Turn> {
        tokio::select! {
            result = self.talk(prompt, permission) => result,
            _ = cancel.cancelled() => Err(crate::Error::Harness("这一轮被叫停".to_owned())),
        }
    }
}

/// The path to run, with Windows' launcher rules applied.
///
/// A package manager installs `dsh` as `dsh.cmd`, and `Command::new("dsh")` does
/// not try `PATHEXT`. So a bare name is looked up on `PATH` for an executable
/// this process can actually start.
fn resolve(program: &Path) -> PathBuf {
    candidates(program)
        .into_iter()
        .next()
        .unwrap_or_else(|| program.to_path_buf())
}

/// Every program a bare name could mean, in the order `PATH` offers them, so a
/// caller can see what the first one hid.
pub fn candidates(program: &Path) -> Vec<PathBuf> {
    if program.extension().is_some() {
        return vec![program.to_path_buf()];
    }

    let Some(paths) = std::env::var_os("PATH") else {
        return vec![program.to_path_buf()];
    };

    let extensions: &[&str] = if cfg!(windows) {
        &["exe", "cmd", "bat"]
    } else {
        &[]
    };

    let mut found = Vec::new();

    for directory in std::env::split_paths(&paths) {
        for extension in extensions {
            let candidate = directory.join(format!("{}.{extension}", program.display()));

            if candidate.is_file() {
                found.push(candidate);
            }
        }

        let bare = directory.join(program);

        if bare.is_file() {
            found.push(bare);
        }
    }

    if found.is_empty() {
        found.push(program.to_path_buf());
    }

    found
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
