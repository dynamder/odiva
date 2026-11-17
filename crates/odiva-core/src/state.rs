//! What the core remembers between runs: the handoff, and nothing else.
//!
//! The mode is not stored on purpose: every start is idle, so a crash can never
//! bring back a half-running busy period.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::handoff::Handoff;
use crate::store::Store;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub handoff: Option<Handoff>,
}

impl State {
    pub fn load(store: &Store) -> Self {
        let Ok(text) = std::fs::read_to_string(store.state()) else {
            return Self::default();
        };

        serde_json::from_str(&text).unwrap_or_default()
    }

    pub fn save(&self, store: &Store) -> Result<()> {
        let path = store.state();

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| Error::Disk(format!("{}: {error}", parent.display())))?;
        }

        let text = serde_json::to_string_pretty(self)
            .map_err(|error| Error::Disk(error.to_string()))?;

        std::fs::write(&path, text)
            .map_err(|error| Error::Disk(format!("{}: {error}", path.display())))
    }
}
