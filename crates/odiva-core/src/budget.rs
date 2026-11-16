//! What the idle hours may spend.
//!
//! Only self-directed work is metered. A conversation is never cut off for cost,
//! and neither is anything someone is waiting on — so this file has exactly two
//! accounts, and both of them are idle's.
//!
//! The day is the local calendar day. A stale file is not an error: it is
//! yesterday, and yesterday's spending does not follow anyone into today.

use std::path::{Path, PathBuf};

use chrono::Local;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Tokens a day of roaming and a day of tidying are allowed.
pub const ROAM_LIMIT: u64 = 300_000;
pub const TIDY_LIMIT: u64 = 60_000;

/// One account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spent {
    pub used: u64,
    pub limit: u64,
}

impl Spent {
    pub fn new(limit: u64) -> Self {
        Self { used: 0, limit }
    }

    pub fn left(&self) -> u64 {
        self.limit.saturating_sub(self.used)
    }

    pub fn is_exhausted(&self) -> bool {
        self.used >= self.limit
    }
}

/// Both accounts, for one day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    /// A local calendar day, `YYYY-MM-DD`. A string because it is only ever
    /// compared for equality and shown to whoever opens the file.
    pub date: String,
    pub roam: Spent,
    pub tidy: Spent,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            date: today(),
            roam: Spent::new(ROAM_LIMIT),
            tidy: Spent::new(TIDY_LIMIT),
        }
    }
}

impl Budget {
    /// Reads today's accounts, rolling over anything older.
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };

        let Ok(budget) = serde_json::from_str::<Self>(&text) else {
            return Self::default();
        };

        if budget.date != today() {
            return Self::default();
        }

        budget
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let text =
            serde_json::to_string_pretty(self).map_err(|error| Error::Disk(error.to_string()))?;

        std::fs::write(path, text)
            .map_err(|error| Error::Disk(format!("{}: {error}", path.display())))
    }

    /// Charges the roaming account; `false` means the day is spent.
    pub fn charge_roam(&mut self, tokens: u64) -> bool {
        if self.roam.used.saturating_add(tokens) > self.roam.limit {
            return false;
        }

        self.roam.used += tokens;
        true
    }

    /// Charges the tidying account; `false` means it is spent.
    pub fn charge_tidy(&mut self, tokens: u64) -> bool {
        if self.tidy.used.saturating_add(tokens) > self.tidy.limit {
            return false;
        }

        self.tidy.used += tokens;
        true
    }
}

fn today() -> String {
    Local::now().date_naive().to_string()
}

/// Where the accounts live.
pub fn path(store: &crate::store::Store) -> PathBuf {
    store.contexts().join("budget.json")
}
