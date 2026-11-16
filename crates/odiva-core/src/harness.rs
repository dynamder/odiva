//! How a worker is started: a command, and the arguments it always gets.
//!
//! The command is configuration, not knowledge — the core does not know what
//! `dsh` is, only that it speaks ACP and takes a directory to work in.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::acp::Worker;

/// A configured harness binary.
#[derive(Debug, Clone)]
pub struct Harness {
    program: PathBuf,
    args: Vec<String>,
    env: BTreeMap<String, String>,
}

impl Harness {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
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
        let name = name.into();
        self.env.insert(name, value.into());
        self
    }

    /// A worker that will run in `cwd`, which is also its sandbox boundary.
    pub fn worker(&self, cwd: &Path) -> Worker {
        let mut worker = Worker::new(&self.program, cwd);

        for arg in &self.args {
            worker = worker.arg(arg.clone());
        }

        for (name, value) in &self.env {
            worker = worker.env(name.clone(), value.clone());
        }

        worker
    }
}
