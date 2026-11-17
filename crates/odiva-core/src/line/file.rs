//! A line's files: `<name>.<generation>.jsonl`, one `Session` entry per line.
//!
//! Appending is the only write that grows a line. A new generation is written
//! whole, and the old file stays where it is.

use std::io::Write;
use std::path::PathBuf;

use yoagent::{Session, SessionEntry};

use crate::error::{Error, Result};
use crate::store::Store;

use super::LineId;

impl Store {
    pub fn line_path(&self, id: &LineId, generation: u32) -> PathBuf {
        self.lines()
            .join(format!("{}.{generation}.jsonl", id.name()))
    }

    /// The newest generation of a line that exists on disk.
    pub fn newest_line(&self, id: &LineId) -> Option<u32> {
        let prefix = format!("{}.", id.name());
        let entries = std::fs::read_dir(self.lines()).ok()?;

        entries
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter_map(|name| {
                let rest = name.strip_prefix(&prefix)?.strip_suffix(".jsonl")?;
                rest.parse::<u32>().ok()
            })
            .max()
    }

    pub fn read_line(&self, id: &LineId, generation: u32) -> Result<Option<Session>> {
        let path = self.line_path(id, generation);

        match std::fs::read_to_string(&path) {
            Ok(text) => Session::from_jsonl(&text).map(Some).map_err(|error| {
                Error::Disk(format!("{}: {error}", path.display()))
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(Error::Disk(format!("{}: {error}", path.display()))),
        }
    }

    pub fn write_line(&self, id: &LineId, generation: u32, session: &Session) -> Result<()> {
        let path = self.line_path(id, generation);
        self.make_parent(&path)?;

        let mut text = session.to_jsonl();
        text.push('\n');

        std::fs::write(&path, text)
            .map_err(|error| Error::Disk(format!("{}: {error}", path.display())))
    }

    pub fn append_line(
        &self,
        id: &LineId,
        generation: u32,
        entries: &[SessionEntry],
    ) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        let path = self.line_path(id, generation);
        self.make_parent(&path)?;

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| Error::Disk(format!("{}: {error}", path.display())))?;

        for entry in entries {
            let line = serde_json::to_string(entry)
                .map_err(|error| Error::Disk(format!("{}: {error}", path.display())))?;

            writeln!(file, "{line}")
                .map_err(|error| Error::Disk(format!("{}: {error}", path.display())))?;
        }

        Ok(())
    }

    fn make_parent(&self, path: &std::path::Path) -> Result<()> {
        match path.parent() {
            Some(parent) => std::fs::create_dir_all(parent)
                .map_err(|error| Error::Disk(format!("{}: {error}", parent.display()))),
            None => Ok(()),
        }
    }
}
