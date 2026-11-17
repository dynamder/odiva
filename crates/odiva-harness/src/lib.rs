//! The harness layer: an ACP client, and what a turn meant.
//!
//! Nothing here knows what a task is. A [`Worker`] is started with a directory,
//! asked one thing, and dropped; the caller decides what to do with the answer.

pub mod acp;
pub mod error;
pub mod harness;

pub use acp::{Ask, Decision, Permission, Refuse, Stop, Tool, Turn, Worker, candidates};
pub use error::{Error, Result};
pub use harness::Harness;
