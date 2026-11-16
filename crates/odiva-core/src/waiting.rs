//! Whether anyone is waiting for her to stop.
//!
//! A message that arrives mid-turn is not an interrupt: it is queued, and it
//! makes one thing true — `yield` exists. That is the whole signal. She can take
//! it, refuse it, or ignore it and keep working; the core sets no deadline and
//! asks a second time.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};
use yoagent::ToolSource;
use yoagent::types::{AgentTool, Content, ToolContext, ToolError, ToolResult};

/// Requests that arrived while she was working, and the offer they create.
#[derive(Debug, Clone, Default)]
pub struct Waiting {
    inner: Arc<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    queued: Mutex<VecDeque<String>>,
    waiting: AtomicBool,
    generation: AtomicU64,
}

impl Waiting {
    /// Takes note of a message that arrived mid-turn.
    pub fn offer(&self, text: String) {
        self.inner
            .queued
            .lock()
            .expect("no panic while queueing")
            .push_back(text);

        self.bump(true);
    }

    /// The next thing to answer, if anything is left.
    pub fn next(&self) -> Option<String> {
        let mut queued = self.inner.queued.lock().expect("no panic while queueing");
        let next = queued.pop_front();

        if queued.is_empty() {
            drop(queued);
            self.bump(false);
        }

        next
    }

    /// Whether anyone is waiting, which is whether `yield` exists.
    pub fn anyone(&self) -> bool {
        self.inner.waiting.load(Ordering::SeqCst)
    }

    /// Refuses the switch: the tools go away, the queued words stay queued.
    fn refuse(&self) {
        self.bump(false);
    }

    fn bump(&self, waiting: bool) {
        self.inner.waiting.store(waiting, Ordering::SeqCst);
        self.inner.generation.fetch_add(1, Ordering::SeqCst);
    }
}

#[async_trait]
impl ToolSource for Waiting {
    async fn tools(&self) -> Vec<Arc<dyn AgentTool>> {
        if !self.anyone() {
            return Vec::new();
        }

        vec![
            Arc::new(Yield) as Arc<dyn AgentTool>,
            Arc::new(CancelYield(self.clone())),
        ]
    }

    fn generation(&self) -> u64 {
        self.inner.generation.load(Ordering::SeqCst)
    }
}

/// Hands the turn over: cancelling the run is what "stop here" means.
struct Yield;

/// Refuses the switch, and makes the offer disappear.
struct CancelYield(Waiting);

#[async_trait]
impl AgentTool for Yield {
    fn name(&self) -> &str {
        "yield"
    }

    fn label(&self) -> &str {
        "Yield"
    }

    fn description(&self) -> &str {
        "只在有人等你时存在。调用就结束这一轮：你立刻去处理对方的请求。\
         副作用还悬着的时候别调它。"
    }

    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _params: Value, ctx: ToolContext) -> Result<ToolResult, ToolError> {
        ctx.cancel.cancel();

        Ok(ToolResult {
            content: vec![Content::Text {
                text: "挂起了。".to_owned(),
            }],
            details: Value::Null,
        })
    }
}

#[async_trait]
impl AgentTool for CancelYield {
    fn name(&self) -> &str {
        "cancel_yield"
    }

    fn label(&self) -> &str {
        "Not yet"
    }

    fn description(&self) -> &str {
        "Refuse the switch: this turn continues, and their words wait their turn."
    }

    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _params: Value, _ctx: ToolContext) -> Result<ToolResult, ToolError> {
        self.0.refuse();

        Ok(ToolResult {
            content: vec![Content::Text {
                text: "继续干。".to_owned(),
            }],
            details: Value::Null,
        })
    }
}
