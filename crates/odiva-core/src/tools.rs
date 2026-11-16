//! What she can do to the world, as tools.
//!
//! Every tool here is a sentence in the prompt's vocabulary and a name in the
//! model's vocabulary; none of them is a workflow the window knows about.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use yoagent::types::{AgentTool, Content, ToolContext, ToolError, ToolResult};

use crate::task::Origin;
use crate::tasks::{self, Tasks};

fn said(text: impl Into<String>) -> ToolResult {
    ToolResult {
        content: vec![Content::Text { text: text.into() }],
        details: Value::Null,
    }
}

fn failed(error: crate::Error) -> ToolError {
    ToolError::Failed(error.to_string())
}

#[derive(Debug, Deserialize)]
struct DispatchArgs {
    goal: String,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    constraints: Vec<String>,
}

pub struct Dispatch {
    tasks: Arc<Tasks>,
    origin: Origin,
}

impl Dispatch {
    pub fn new(tasks: Arc<Tasks>, origin: Origin) -> Self {
        Self { tasks, origin }
    }
}

#[async_trait]
impl AgentTool for Dispatch {
    fn name(&self) -> &str {
        "task_dispatch"
    }

    fn label(&self) -> &str {
        "派活"
    }

    fn description(&self) -> &str {
        "把一个活交给一个独立的 harness 进程去做，立刻返回。它有自己的目录、自己的模型、\
         自己的死亡方式；干完会来找你。所以 brief 要自包含：它看不见这段对话。"
    }

    fn parameters_schema(&self) -> Value {
        tasks::dispatch_arguments()
    }

    async fn execute(&self, params: Value, _ctx: ToolContext) -> Result<ToolResult, ToolError> {
        let args: DispatchArgs = serde_json::from_value(params)
            .map_err(|error| ToolError::InvalidArgs(error.to_string()))?;

        let brief = tasks::brief(&args.goal, &args.files, &args.constraints);
        let id = self.tasks.dispatch(self.origin, brief).map_err(failed)?;

        Ok(said(format!("派出去了：{id}。")))
    }
}

pub struct List {
    tasks: Arc<Tasks>,
}

impl List {
    pub fn new(tasks: Arc<Tasks>) -> Self {
        Self { tasks }
    }
}

#[async_trait]
impl AgentTool for List {
    fn name(&self) -> &str {
        "task_list"
    }

    fn label(&self) -> &str {
        "看看在跑什么"
    }

    fn description(&self) -> &str {
        "所有任务，一行一个：id、在等什么、一句话。"
    }

    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _params: Value, _ctx: ToolContext) -> Result<ToolResult, ToolError> {
        let all = self.tasks.all();

        if all.is_empty() {
            return Ok(said("手上没有任务。"));
        }

        let lines: Vec<String> = all.iter().map(|task| task.line()).collect();
        Ok(said(lines.join("\n")))
    }
}

#[derive(Debug, Deserialize)]
struct IdArgs {
    id: String,
}

pub struct Read {
    tasks: Arc<Tasks>,
}

impl Read {
    pub fn new(tasks: Arc<Tasks>) -> Self {
        Self { tasks }
    }
}

#[async_trait]
impl AgentTool for Read {
    fn name(&self) -> &str {
        "task_read"
    }

    fn label(&self) -> &str {
        "读任务"
    }

    fn description(&self) -> &str {
        "一个任务的全貌：它在等什么、报告、以及它最后说的话。"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "id": { "type": "string" } },
            "required": ["id"]
        })
    }

    async fn execute(&self, params: Value, _ctx: ToolContext) -> Result<ToolResult, ToolError> {
        let args: IdArgs = serde_json::from_value(params)
            .map_err(|error| ToolError::InvalidArgs(error.to_string()))?;

        let task = self.tasks.get(&args.id).map_err(failed)?;
        Ok(said(tasks::reading(&task)))
    }
}

#[derive(Debug, Deserialize)]
struct AnswerArgs {
    id: String,
    text: String,
}

pub struct Answer {
    tasks: Arc<Tasks>,
}

impl Answer {
    pub fn new(tasks: Arc<Tasks>) -> Self {
        Self { tasks }
    }
}

#[async_trait]
impl AgentTool for Answer {
    fn name(&self) -> &str {
        "task_answer"
    }

    fn label(&self) -> &str {
        "回它一句"
    }

    fn description(&self) -> &str {
        "给一个在等你说话的任务回一句，它会带着这句话重新开一轮。\
         它不记得上一次的上下文，所以要交代清楚。"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "text": { "type": "string", "description": "要交代给它的那句话。" }
            },
            "required": ["id", "text"]
        })
    }

    async fn execute(&self, params: Value, _ctx: ToolContext) -> Result<ToolResult, ToolError> {
        let args: AnswerArgs = serde_json::from_value(params)
            .map_err(|error| ToolError::InvalidArgs(error.to_string()))?;

        self.tasks.answer(&args.id, &args.text).map_err(failed)?;
        Ok(said("交代完了。"))
    }
}

pub struct Cancel {
    tasks: Arc<Tasks>,
}

impl Cancel {
    pub fn new(tasks: Arc<Tasks>) -> Self {
        Self { tasks }
    }
}

#[async_trait]
impl AgentTool for Cancel {
    fn name(&self) -> &str {
        "task_cancel"
    }

    fn label(&self) -> &str {
        "停掉"
    }

    fn description(&self) -> &str {
        "停掉一个任务，进程就地结束。已经做出去的副作用不会撤回。"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "id": { "type": "string" } },
            "required": ["id"]
        })
    }

    async fn execute(&self, params: Value, _ctx: ToolContext) -> Result<ToolResult, ToolError> {
        let args: IdArgs = serde_json::from_value(params)
            .map_err(|error| ToolError::InvalidArgs(error.to_string()))?;

        self.tasks.cancel(&args.id).map_err(failed)?;
        Ok(said("停了。"))
    }
}

/// The whole task surface, as one roster entry point.
pub fn roster(tasks: &Arc<Tasks>, origin: Origin) -> Vec<Box<dyn AgentTool>> {
    vec![
        Box::new(Dispatch::new(Arc::clone(tasks), origin)),
        Box::new(List::new(Arc::clone(tasks))),
        Box::new(Read::new(Arc::clone(tasks))),
        Box::new(Answer::new(Arc::clone(tasks))),
        Box::new(Cancel::new(Arc::clone(tasks))),
    ]
}
