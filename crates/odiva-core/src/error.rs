//! What can go wrong, in the core's own words.
//!
//! The two libraries under this crate — ACP and `yoagent` — have their own error
//! types, and neither says anything a task ledger can record. So everything
//! lands here as a sentence, and the ledger keeps the sentence.

#[derive(Debug)]
pub enum Error {
    /// The harness could not be started, or the protocol broke while it ran.
    Harness(String),
    /// A conversation file could not be read or written.
    Disk(String),
    /// The history no longer extends the session it claims to be.
    History(String),
    /// The model refused or failed; there is no reply to read.
    Brain(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Harness(detail) => write!(formatter, "harness: {detail}"),
            Self::Disk(detail) => write!(formatter, "disk: {detail}"),
            Self::History(detail) => write!(formatter, "history: {detail}"),
            Self::Brain(detail) => write!(formatter, "brain: {detail}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<yoagent::session::SessionError> for Error {
    fn from(error: yoagent::session::SessionError) -> Self {
        Self::History(error.to_string())
    }
}

impl From<agent_client_protocol::Error> for Error {
    fn from(error: agent_client_protocol::Error) -> Self {
        Self::Harness(format!("{error:?}"))
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Disk(error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
