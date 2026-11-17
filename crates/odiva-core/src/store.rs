//! Where the core keeps things: one root, and the paths inside it.
//!
//! ```text
//! transcript.jsonl              what the window showed, one line per message
//! contexts/lines/*.jsonl        one file per line generation, `yoagent` sessions
//! contexts/budget.json
//! tasks/<id>.json
//! ```

use std::path::{Path, PathBuf};

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

    pub fn lines(&self) -> PathBuf {
        self.contexts().join("lines")
    }

    pub fn transcript(&self) -> PathBuf {
        self.root.join("transcript.jsonl")
    }

    pub fn state(&self) -> PathBuf {
        self.root.join("state.json")
    }

    pub fn tasks(&self) -> PathBuf {
        self.root.join("tasks")
    }

    pub fn task(&self, id: &str) -> PathBuf {
        self.tasks().join(format!("{id}.json"))
    }
}
