//! One line per thing that happened, appended where a person can find it.
//!
//! A window started from a shortcut has no terminal, and a turn that ends
//! without saying anything looks exactly like a turn that never ran. So the core
//! writes down what it did: what was asked, whether the model spoke, what a
//! worker did. It is a log, not a transcript — the transcript is the lane.

use std::io::Write;
use std::path::{Path, PathBuf};

/// An append-only file. Writing never fails a turn.
#[derive(Debug, Clone)]
pub struct Journal {
    path: PathBuf,
}

impl Journal {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn line(&self, message: impl AsRef<str>) {
        if let Some(parent) = self.path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            eprintln!("odiva: 写不了日志目录 {}：{error}", parent.display());
            return;
        }

        // One buffer, one append: two threads writing a stamp each can otherwise
        // interleave inside one line, and a line that reads as two is a log that
        // lies about the order.
        let line = format!(
            "{} {}\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            message.as_ref()
        );

        let written = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut file| file.write_all(line.as_bytes()));

        if let Err(error) = written {
            eprintln!("odiva: 写不了日志 {}：{error}", self.path.display());
        }
    }
}
