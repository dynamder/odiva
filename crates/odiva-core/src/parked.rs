//! Asking to be let out of a busy period.
//!
//! Busy is a fact — a task is unfinished — not a mode she chose. But sometimes
//! what she needs is something only idle has, and the honest way there is to
//! say so and stop, not to look for a back door.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use kameo::actor::ActorRef;
use serde_json::{Value, json};
use yoagent::types::{AgentTool, Content, ToolContext, ToolError, ToolResult};

use crate::odiva::{AskIdle, Odiva};

/// Whether she has asked to be idle until something happens.
#[derive(Debug, Clone, Default)]
pub struct Parked {
    flag: Arc<AtomicBool>,
}

impl Parked {
    pub fn park(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    /// Something happened: a worker moved, or the person spoke.
    pub fn wake(&self) {
        self.flag.store(false, Ordering::SeqCst);
    }

    pub fn is_parked(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

/// The tool that asks to go idle.
pub struct RequestIdle {
    odiva: ActorRef<Odiva>,
}

impl RequestIdle {
    pub fn new(odiva: ActorRef<Odiva>) -> Self {
        Self { odiva }
    }
}

#[async_trait::async_trait]
impl AgentTool for RequestIdle {
    fn name(&self) -> &str {
        "request_idle"
    }

    fn label(&self) -> &str {
        "Park"
    }

    fn description(&self) -> &str {
        "回到空闲，这一轮就地结束；要等某个任务动了才回到忙。\
         副作用还悬着的时候别调它。"
    }

    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _params: Value, ctx: ToolContext) -> Result<ToolResult, ToolError> {
        let _ = self.odiva.tell(AskIdle).await;
        ctx.cancel.cancel();

        Ok(ToolResult {
            content: vec![Content::Text {
                text: "停一下。".to_owned(),
            }],
            details: Value::Null,
        })
    }
}
