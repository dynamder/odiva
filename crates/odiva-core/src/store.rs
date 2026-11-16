//! Where the core keeps things: one root, and the paths inside it.
//!
//! ```text
//! contexts/
//! ├── talk.session.<n>.jsonl     the public record, one line per message
//! ├── work.session.jsonl
//! ├── roam/<ts>.session.jsonl
//! └── ...
//! tasks/<id>.md
//! ```
//!
//! A lane file is a root line followed by messages. The root says which system
//! prompt the lane was opened under, so a lane is readable on its own; the rest
//! is exactly what the person saw. Files are only ever appended or rotated — a
//! conversation that outgrew its lane starts the next generation, and the old
//! one stays where it is.

use std::path::{Path, PathBuf};

use odiva_proto::{Side, WireMessage};
use serde::{Deserialize, Serialize};
use yoagent::types::{AgentMessage, Content, CustomMessage, Message, StopReason, Usage};

use crate::error::{Error, Result};

/// The one lane that is always open.
pub const TALK: &str = "talk";

/// One conversation: a file, and the toolset that ran against it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    /// The public record: what the person said, what she said, pointers to work.
    Talk { generation: u32 },
    /// One busy period's orchestration.
    Work,
    /// A stretch of self-directed work, rooted at the moment it began.
    Roam { started: i64 },
}

impl Lane {
    fn path(self, contexts: &Path) -> PathBuf {
        match self {
            Self::Talk { generation } => {
                contexts.join(format!("{TALK}.session.{generation}.jsonl"))
            }
            Self::Work => contexts.join("work.session.jsonl"),
            Self::Roam { started } => {
                contexts.join("roam").join(format!("{started}.session.jsonl"))
            }
        }
    }

    fn stem(self) -> &'static str {
        match self {
            Self::Talk { .. } => TALK,
            Self::Work => "work",
            Self::Roam { .. } => "roam",
        }
    }
}

/// What a lane's first line says: the system prompt it was opened under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Root {
    system: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum Line {
    Root { root: Root },
    Said { message: WireMessage },
}

/// A lane's contents: the prompt it runs under, and everything said in it.
#[derive(Debug, Clone)]
pub struct Log {
    system: String,
    messages: Vec<WireMessage>,
}

impl Log {
    pub fn new(system: impl Into<String>) -> Self {
        Self {
            system: system.into(),
            messages: Vec::new(),
        }
    }

    pub fn system(&self) -> &str {
        &self.system
    }

    pub fn messages(&self) -> &[WireMessage] {
        &self.messages
    }

    pub fn push(&mut self, message: WireMessage) {
        self.messages.push(message);
    }

    /// The conversation as the model reads it.
    ///
    /// Only what was actually said, in order. A lane never carries tool calls,
    /// so a turn starts from the public record and nothing else — which is why
    /// compaction is a decision the core makes, not something the loop does.
    pub fn history(&self) -> Vec<AgentMessage> {
        let messages = self.messages.iter().map(|message| match message.from {
            Side::User => AgentMessage::Llm(Message::user(message.markdown.clone())),
            Side::Assistant => AgentMessage::Llm(Message::assistant(
                vec![Content::Text {
                    text: message.markdown.clone(),
                }],
                StopReason::Stop,
                "",
                "",
                Usage::default(),
            )),
        });

        std::iter::once(root(&self.system)).chain(messages).collect()
    }

    fn to_jsonl(&self) -> String {
        let root = Line::Root {
            root: Root {
                system: self.system.clone(),
            },
        };

        let mut text = serde_json::to_string(&root).expect("a root line serializes");

        for message in &self.messages {
            text.push('\n');
            text.push_str(
                &serde_json::to_string(&Line::Said {
                    message: message.clone(),
                })
                .expect("a message line serializes"),
            );
        }

        text.push('\n');
        text
    }

    fn parse(text: &str) -> Result<Self> {
        let mut lines = text.lines().filter(|line| !line.trim().is_empty());
        let first = lines.next().ok_or_else(|| Error::Disk("empty lane file".to_owned()))?;

        let system = match serde_json::from_str::<Line>(first) {
            Ok(Line::Root { root }) => root.system,
            _ => return Err(Error::Disk("a lane file must start with its root".to_owned())),
        };

        let mut messages = Vec::new();

        for line in lines {
            match serde_json::from_str::<Line>(line) {
                Ok(Line::Said { message }) => messages.push(message),
                Ok(Line::Root { .. }) => {
                    return Err(Error::Disk("a lane file has one root".to_owned()));
                }
                Err(error) => return Err(Error::Disk(error.to_string())),
            }
        }

        Ok(Self { system, messages })
    }
}

/// The root of any lane, for the model: a custom message no provider sees.
fn root(system: &str) -> AgentMessage {
    AgentMessage::Custom(CustomMessage::new("root", Root { system: system.to_owned() }))
}

/// The directory the core owns.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn contexts(&self) -> PathBuf {
        self.root.join("contexts")
    }

    pub fn tasks(&self) -> PathBuf {
        self.root.join("tasks")
    }

    pub fn task(&self, id: &str) -> PathBuf {
        self.tasks().join(format!("{id}.md"))
    }

    pub fn path(&self, lane: Lane) -> PathBuf {
        lane.path(&self.contexts())
    }

    /// The highest generation of a lane that exists on disk, or 0.
    pub fn newest(&self, stem: &str) -> u32 {
        let Ok(entries) = std::fs::read_dir(self.contexts()) else {
            return 0;
        };

        let prefix = format!("{stem}.session.");

        entries
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter_map(|name| {
                let rest = name.strip_prefix(&prefix)?.strip_suffix(".jsonl")?;
                rest.parse::<u32>().ok()
            })
            .max()
            .unwrap_or(0)
    }

    /// Reads a lane, or nothing when it has never been written.
    pub fn read(&self, lane: Lane) -> Result<Option<Log>> {
        let path = self.path(lane);

        match std::fs::read_to_string(&path) {
            Ok(text) => Log::parse(&text).map(Some),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(Error::Disk(format!("{}: {error}", path.display()))),
        }
    }

    /// Writes a lane as it stands.
    pub fn write(&self, lane: Lane, log: &Log) -> Result<()> {
        let path = self.path(lane);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&path, log.to_jsonl())
            .map_err(|error| Error::Disk(format!("{}: {error}", path.display())))
    }

    /// A lane opened under `system`, and the lane to write it back to.
    ///
    /// A lane whose prompt has changed is a different conversation: the cache
    /// prefix is gone, so a new generation starts instead of pretending the old
    /// one continues. Only the talking lane rotates; the others are one period
    /// each by construction.
    pub fn open(&self, lane: Lane, system: &str) -> Result<(Lane, Log)> {
        let lane = match lane {
            Lane::Talk { .. } => Lane::Talk {
                generation: self.newest(lane.stem()),
            },
            fixed => fixed,
        };

        match self.read(lane)? {
            None => Ok((lane, Log::new(system))),
            Some(log) if log.system() == system => Ok((lane, log)),
            Some(_) => match lane {
                Lane::Talk { generation } => Ok((
                    Lane::Talk {
                        generation: generation + 1,
                    },
                    Log::new(system),
                )),
                fixed => Ok((fixed, Log::new(system))),
            },
        }
    }
}
