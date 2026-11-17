//! Asking to be let out of a busy period.
//!
//! The request is a question, not a sentence: a state transition must not depend
//! on a model reading free text. While it is open, the window's composer is off
//! and the only way forward is one of its options.

use kameo::actor::ActorRef;
use odiva_proto::{CoreMessage, Note, Question, QuestionOption};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedSender;
use yoagent::types::{AgentTool, Content, ToolContext, ToolError, ToolResult};

use crate::asking::Asking;
use crate::halt::Halt;
use crate::odiva::{self, Odiva};

const AGREE: usize = 0;
const REFUSE: usize = 1;
const EXPLAIN: usize = 2;

fn option(label: &str, note: Option<Note>) -> QuestionOption {
    QuestionOption {
        label: label.to_owned(),
        note,
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    message: String,
}

/// The tool that asks to go idle.
pub struct RequestIdle {
    out: UnboundedSender<CoreMessage>,
    asking: Asking,
    halt: Halt,
    odiva: ActorRef<Odiva>,
}

impl RequestIdle {
    pub fn new(
        out: UnboundedSender<CoreMessage>,
        asking: Asking,
        halt: Halt,
        odiva: ActorRef<Odiva>,
    ) -> Self {
        Self {
            out,
            asking,
            halt,
            odiva,
        }
    }
}

#[async_trait::async_trait]
impl AgentTool for RequestIdle {
    fn name(&self) -> &str {
        "request_idle"
    }

    fn label(&self) -> &str {
        "Ask to pause"
    }

    fn description(&self) -> &str {
        "Ask the person whether the work may stop for now. They answer with one of the offered \
         options; nothing is free-typed. Only call this when the work can be paused safely."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "What the person reads above the options."
                }
            },
            "required": ["message"]
        })
    }

    async fn execute(&self, params: Value, ctx: ToolContext) -> Result<ToolResult, ToolError> {
        let args: Args = serde_json::from_value(params)
            .map_err(|error| ToolError::InvalidArgs(error.to_string()))?;

        let question = Question {
            question: args.message,
            options: vec![
                option("同意", None),
                option(
                    "拒绝",
                    Some(Note {
                        prompt: "为什么先不停？（可以留空）".to_owned(),
                        required: false,
                    }),
                ),
                option(
                    "要求解释",
                    Some(Note {
                        prompt: "你想先知道什么？".to_owned(),
                        required: true,
                    }),
                ),
            ],
        };

        let told = self.asking.begin(question.clone());

        self.out
            .send(CoreMessage::QuestionAsked { question })
            .map_err(|_| ToolError::Failed("没有窗口在听".to_owned()))?;

        let answer = tokio::select! {
            told = told => told.ok().flatten(),
            _ = ctx.cancel.cancelled() => None,
        };

        let text = match answer {
            Some(answer) if answer.option == AGREE => {
                self.halt.stop();
                let _ = self.odiva.tell(odiva::Pause).await;
                "对方同意暂停。这一轮到此为止。".to_owned()
            }
            Some(answer) if answer.option == REFUSE => match answer.note {
                Some(note) if !note.trim().is_empty() => {
                    format!("对方拒绝，继续干。对方说：{note}")
                }
                _ => "对方拒绝，继续干。".to_owned(),
            },
            Some(answer) if answer.option == EXPLAIN => format!(
                "对方要求解释：{}。解释完再决定要不要重新申请暂停。",
                answer.note.unwrap_or_default()
            ),
            _ => "对方没有回答。".to_owned(),
        };

        Ok(ToolResult {
            content: vec![Content::Text { text }],
            details: Value::Null,
        })
    }
}
