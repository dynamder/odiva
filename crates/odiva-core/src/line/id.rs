//! What a line is: the state it belongs to, and the task it is about.

use std::fmt;

/// The two states odiva is in. They pick the line, the roster, and the prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    Idle,
    Busy,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Busy => "busy",
        }
    }
}

/// Which context a turn runs in.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LineId {
    /// The dispatcher of a state: always open, never about one task.
    Normal(Mode),
    /// One task's context, opened when the task is accepted and kept after it.
    Task(String),
}

impl LineId {
    pub fn normal(mode: Mode) -> Self {
        Self::Normal(mode)
    }

    pub fn task(id: impl Into<String>) -> Self {
        Self::Task(id.into())
    }

    /// The stable name a line's files are named after.
    pub fn name(&self) -> String {
        match self {
            Self::Normal(mode) => format!("normal-{}", mode.name()),
            Self::Task(id) => format!("task-{id}"),
        }
    }
}

impl fmt::Display for LineId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.name())
    }
}
