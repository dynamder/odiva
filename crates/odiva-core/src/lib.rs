//! The core: what sits behind the window.
//!
//! No resident agent. There is an engine (paths, ids, the ledger, negotiation),
//! the things it keeps on disk, and activations — an activation is a context
//! plus a toolset, built when a turn starts and dropped when it ends.

pub mod acp;
pub mod engine;
pub mod error;
pub mod harness;
pub mod ids;
pub mod prompt;
pub mod roster;
pub mod route;
pub mod say;
pub mod store;
pub mod talk;
pub mod task;
pub mod tasks;
pub mod tools;
pub mod waiting;

pub use acp::{Stop, Turn, Worker};
pub use engine::{Engine, Handle};
pub use error::{Error, Result};
pub use harness::Harness;
pub use ids::Ids;
pub use roster::Roster;
pub use route::{Model, Route, Scripted};
pub use say::Say;
pub use store::{Lane, Store};
pub use talk::Talk;
pub use task::{Ledger, Origin, Outcome, Report, Status, Task};
pub use tasks::Tasks;
pub use waiting::Waiting;
