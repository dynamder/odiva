//! The core: what sits behind the window.
//!
//! No resident agent. There is an engine (paths, ids, negotiation), the things it
//! keeps on disk, and activations — an activation is a context plus a toolset,
//! built when a turn starts and dropped when it ends.

pub mod acp;
pub mod error;
pub mod ids;
pub mod route;
pub mod say;
pub mod store;
pub mod talk;

pub use error::{Error, Result};
pub use ids::Ids;
pub use route::{Model, Route, Scripted};
pub use say::Say;
pub use store::{Lane, Store};
pub use talk::Talk;
