//! `say`: the only thing that puts a message in front of the person.
//!
//! There is no streaming channel behind it. A tool call happened or it did not,
//! so a message arrives whole — and if the model does not call this, nothing is
//! said, which is a legitimate outcome and not a failure to recover from.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use odiva_proto::{CoreMessage, FileRef, WireMessage};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedSender;
use yoagent::types::{AgentTool, Content, ToolContext, ToolError, ToolResult};

use crate::ids::Ids;

/// What the window is told.
#[derive(Debug, Deserialize)]
struct Args {
    markdown: String,
    #[serde(default)]
    attach: Vec<PathBuf>,
    #[serde(default)]
    replies: Vec<String>,
}

/// What she said during one activation, in order.
///
/// The tool records it, the lane writes it: a message the person saw is a
/// message the lane keeps, even when the turn was cut short afterwards.
#[derive(Debug, Clone, Default)]
pub struct Said {
    messages: Arc<Mutex<Vec<WireMessage>>>,
}

impl Said {
    fn note(&self, message: WireMessage) {
        self.messages
            .lock()
            .expect("no panic while recording")
            .push(message);
    }

    pub fn is_empty(&self) -> bool {
        self.messages
            .lock()
            .expect("no panic while recording")
            .is_empty()
    }

    /// Everything said since the last drain, oldest first.
    pub fn drain(&self) -> Vec<WireMessage> {
        std::mem::take(&mut *self.messages.lock().expect("no panic while recording"))
    }
}

/// The tool named `say`.
pub struct Say {
    out: UnboundedSender<CoreMessage>,
    ids: Ids,
    said: Said,
}

impl Say {
    pub fn new(out: UnboundedSender<CoreMessage>, ids: Ids, said: Said) -> Self {
        Self { out, ids, said }
    }
}

#[async_trait]
impl AgentTool for Say {
    fn name(&self) -> &str {
        "say"
    }

    fn label(&self) -> &str {
        "说出来"
    }

    fn description(&self) -> &str {
        "把一条消息发到窗口上。不调用它，对方什么都不会看到。"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "markdown": {
                    "type": "string",
                    "description": "对方会看到的文字。markdown。"
                },
                "attach": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "要附上的文件路径。大小由 core 量，不要自己写。"
                },
                "replies": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "建议对方点的回话。点了等于替他打这句话。"
                }
            },
            "required": ["markdown"]
        })
    }

    async fn execute(&self, params: Value, _ctx: ToolContext) -> Result<ToolResult, ToolError> {
        let args: Args = serde_json::from_value(params)
            .map_err(|error| ToolError::InvalidArgs(error.to_string()))?;

        let files = args
            .attach
            .iter()
            .map(|path| file_ref(path))
            .collect::<Result<Vec<_>, ToolError>>()?;

        let mut message = self.ids.hers(args.markdown);
        message.files = files;
        message.replies = args.replies;

        self.said.note(message.clone());
        self.out
            .send(CoreMessage::MessageAppended { message })
            .map_err(|_| ToolError::Failed("没有窗口在听".to_owned()))?;

        Ok(ToolResult {
            content: vec![Content::Text {
                text: "发出去了。".to_owned(),
            }],
            details: Value::Null,
        })
    }
}

/// Measures a file the way the window shows it.
fn file_ref(path: &Path) -> Result<FileRef, ToolError> {
    let meta = std::fs::metadata(path).map_err(|error| {
        ToolError::Failed(format!("附不上 {}：{error}", path.display()))
    })?;

    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file")
        .to_owned();

    Ok(FileRef {
        name,
        size: human_size(meta.len()),
    })
}

fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let size = bytes as f64;

    if size < KB {
        format!("{bytes} B")
    } else if size < KB * KB {
        format!("{:.1} KB", size / KB)
    } else {
        format!("{:.1} MB", size / (KB * KB))
    }
}
