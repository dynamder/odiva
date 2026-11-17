//! The core: what sits behind the window.
//!
//! An `Odiva` actor owns the state — one mailbox, one turn at a time — and a
//! `Records` actor owns the task ledger, so no state has two owners. A turn is
//! one activation on one line; the window's record is a file of its own.

pub mod asking;
pub mod brief;
pub mod budget;
pub mod error;
pub mod halt;
pub mod handoff;
pub mod history;
pub mod ids;
pub mod journal;
pub mod ledger;
pub mod line;
pub mod idle;
pub mod odiva;
pub mod policy;
pub mod prompt;
pub mod roster;
pub mod route;
pub mod run;
pub mod say;
pub mod state;
pub mod store;
pub mod talk;
pub mod task;
pub mod told;
pub mod tools;
pub mod transcript;
pub mod turn;

pub use budget::Budget;
pub use error::{Error, Result};
pub use handoff::Handoff;
pub use ids::Ids;
pub use journal::Journal;
pub use line::{Line, LineId, Mode};
pub use odiva::{Handle, start, start_with};
pub use odiva_harness::{Harness, Stop, Turn, Worker, acp, harness};
pub use roster::Roster;
pub use route::{Model, Route, Scripted};
pub use say::Say;
pub use state::State;
pub use store::Store;
pub use talk::Talk;
pub use task::{Ledger, Origin, Outcome, Report, Status, Task};
pub use told::Told;
pub use transcript::Transcript;
