//! What can go wrong on the ACP side.

use std::fmt;

/// A failure that belongs to the harness, not to the core.
#[derive(Debug)]
pub enum Error {
    /// The harness could not be started, or the protocol broke while it ran.
    Harness(String),
    /// The protocol itself failed: a request was refused, or the peer hung up.
    Protocol(agent_client_protocol::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Harness(detail) => write!(formatter, "harness: {detail}"),
            Self::Protocol(detail) => write!(formatter, "acp: {detail:?}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<agent_client_protocol::Error> for Error {
    fn from(error: agent_client_protocol::Error) -> Self {
        Self::Protocol(error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
