//! What the window showed, in the order it showed it.
//!
//! One file, one message per line, and it is only ever appended to. The lines
//! hold the model's context; this holds the record the person can read, and the
//! two are not the same thing — reasoning and tool calls belong to a line.

use std::io::Write;
use std::path::{Path, PathBuf};

use odiva_proto::WireMessage;

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct Transcript {
    path: PathBuf,
}

impl Transcript {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Everything said so far, oldest first. A missing file is an empty one.
    pub fn read(&self) -> Result<Vec<WireMessage>> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(self.failure(&error)),
        };

        text.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).map_err(|error| self.failure(&error)))
            .collect()
    }

    pub fn append(&self, message: &WireMessage) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| Error::Disk(format!("{}: {error}", parent.display())))?;
        }

        let line = serde_json::to_string(message).map_err(|error| self.failure(&error))?;

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| self.failure(&error))?;

        writeln!(file, "{line}").map_err(|error| self.failure(&error))
    }

    fn failure(&self, error: &dyn std::fmt::Display) -> Error {
        Error::Disk(format!("{}: {error}", self.path.display()))
    }
}
