//! Where the core keeps things: one root, and the paths inside it.
//!
//! ```text
//! contexts/
//! ├── talk.session.<n>.jsonl
//! ├── work.session.jsonl
//! └── roam/<ts>.session.jsonl
//! tasks/<id>.md
//! ```
//!
//! Files are only ever appended or rotated: a conversation that grew past its
//! lane starts the next generation and the old one stays where it is.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use yoagent::session::Session;
use yoagent::types::{AgentMessage, CustomMessage};

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
            Self::Talk { generation } => contexts.join(format!("{TALK}.session.{generation}.jsonl")),
            Self::Work => contexts.join("work.session.jsonl"),
            Self::Roam { started } => contexts.join("roam").join(format!("{started}.session.jsonl")),
        }
    }
}

/// What a session's first entry says: the system prompt it ran under.
///
/// A lane's identity includes its prompt, so a session whose prompt changed is
/// not the same conversation — opening it starts the next generation instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Root {
    pub system: String,
}

/// The first entry of any session.
pub fn root(system: &str) -> AgentMessage {
    AgentMessage::Custom(CustomMessage::new(
        "root",
        Root {
            system: system.to_owned(),
        },
    ))
}

/// The prompt a session was opened with, if it has a root at all.
pub fn rooted_with(session: &Session) -> Option<String> {
    let entry = session.entries().first()?;
    let AgentMessage::Custom(CustomMessage { kind, data, .. }) = &entry.message else {
        return None;
    };

    if kind != "root" {
        return None;
    }

    serde_json::from_value::<Root>(data.clone())
        .ok()
        .map(|root| root.system)
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

    /// Reads a lane, or an empty session when it has never been written.
    pub fn open(&self, lane: Lane) -> Result<Session> {
        let path = self.path(lane);

        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Session::new()),
            Err(error) => return Err(Error::Disk(format!("{}: {error}", path.display()))),
        };

        Session::from_jsonl(&text).map_err(|error| Error::Disk(format!("{}: {error}", path.display())))
    }

    /// Writes a lane as it stands. Whole file, because it is the thing that must
    /// survive a crash, not an incremental log we replay.
    pub fn write(&self, lane: Lane, session: &Session) -> Result<()> {
        let path = self.path(lane);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&path, session.to_jsonl())
            .map_err(|error| Error::Disk(format!("{}: {error}", path.display())))
    }
}
