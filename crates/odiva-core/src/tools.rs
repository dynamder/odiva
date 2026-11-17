//! What she can do to the world, as tools.
//!
//! Every tool here is a sentence in the prompt's vocabulary and a name in the
//! model's vocabulary; none of them is a workflow the window knows about.

use kameo::actor::ActorRef;
use serde::Deserialize;
use serde_json::{Value, json};
use yoagent::types::{AgentTool, Content, ToolContext, ToolError, ToolResult};

use crate::brief;
use crate::ledger::{self, Records};
use crate::odiva::{self, Odiva};
use crate::task::{Origin, Status, Task};

fn said(text: impl Into<String>) -> ToolResult {
    ToolResult {
        content: vec![Content::Text { text: text.into() }],
        details: Value::Null,
    }
}

fn lost() -> ToolError {
    ToolError::Failed("台账不可用".to_owned())
}

fn unknown(id: &str) -> ToolError {
    ToolError::Failed(format!("没有这个任务：{id}"))
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
    records: ActorRef<Records>,
    odiva: ActorRef<Odiva>,
    origin: Origin,
}

impl Dispatch {
    pub fn new(records: ActorRef<Records>, odiva: ActorRef<Odiva>, origin: Origin) -> Self {
        Self {
            records,
            odiva,
            origin,
        }
    }
}

#[async_trait::async_trait]
impl AgentTool for Dispatch {
    fn name(&self) -> &str {
        "task_dispatch"
    }

    fn label(&self) -> &str {
        "Dispatch"
    }

    fn description(&self) -> &str {
        "Send a job to a separate worker process and return at once. It has its own directory, \
         it can fail, and it comes back when it is done."
    }

    fn parameters_schema(&self) -> Value {
        brief::dispatch_arguments()
    }

    async fn execute(&self, params: Value, _ctx: ToolContext) -> Result<ToolResult, ToolError> {
        let args: DispatchArgs = serde_json::from_value(params)
            .map_err(|error| ToolError::InvalidArgs(error.to_string()))?;

        let id = self
            .records
            .ask(ledger::Reserve {
                origin: self.origin,
                brief: brief::brief(&args.goal, &args.files, &args.constraints),
            })
            .await
            .map_err(|_| lost())?;

        let _ = self.odiva.tell(odiva::Fill).await;

        Ok(said(format!("派出去了：{id}。")))
    }
}

pub struct List {
    records: ActorRef<Records>,
}

impl List {
    pub fn new(records: ActorRef<Records>) -> Self {
        Self { records }
    }
}

#[async_trait::async_trait]
impl AgentTool for List {
    fn name(&self) -> &str {
        "task_list"
    }

    fn label(&self) -> &str {
        "What is running"
    }

    fn description(&self) -> &str {
        "Every task, one per line: id, what it waits on, one sentence."
    }

    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _params: Value, _ctx: ToolContext) -> Result<ToolResult, ToolError> {
        let all = self
            .records
            .ask(ledger::Everything)
            .await
            .map_err(|_| lost())?;

        if all.is_empty() {
            return Ok(said("手上没有任务。"));
        }

        let lines: Vec<String> = all.iter().map(Task::line).collect();
        Ok(said(lines.join("\n")))
    }
}

#[derive(Debug, Deserialize)]
struct IdArgs {
    id: String,
}

pub struct Read {
    records: ActorRef<Records>,
}

impl Read {
    pub fn new(records: ActorRef<Records>) -> Self {
        Self { records }
    }
}

#[async_trait::async_trait]
impl AgentTool for Read {
    fn name(&self) -> &str {
        "task_read"
    }

    fn label(&self) -> &str {
        "Read a task"
    }

    fn description(&self) -> &str {
        "One task in full: what it waits on, its report, the last thing it said."
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

        let task = self
            .records
            .ask(ledger::Lookup { id: args.id.clone() })
            .await
            .map_err(|_| lost())?
            .ok_or_else(|| unknown(&args.id))?;

        Ok(said(brief::reading(&task)))
    }
}

#[derive(Debug, Deserialize)]
struct AnswerArgs {
    id: String,
    text: String,
}

pub struct Answer {
    records: ActorRef<Records>,
    odiva: ActorRef<Odiva>,
}

impl Answer {
    pub fn new(records: ActorRef<Records>, odiva: ActorRef<Odiva>) -> Self {
        Self { records, odiva }
    }
}

#[async_trait::async_trait]
impl AgentTool for Answer {
    fn name(&self) -> &str {
        "task_instruct"
    }

    fn label(&self) -> &str {
        "Instruct a task"
    }

    fn description(&self) -> &str {
        "Give a task that is waiting for words its next instruction. It restarts with its brief, \
         its last words and this line."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "text": { "type": "string", "description": "What to tell it." }
            },
            "required": ["id", "text"]
        })
    }

    async fn execute(&self, params: Value, _ctx: ToolContext) -> Result<ToolResult, ToolError> {
        let args: AnswerArgs = serde_json::from_value(params)
            .map_err(|error| ToolError::InvalidArgs(error.to_string()))?;

        let task = self
            .records
            .ask(ledger::Lookup { id: args.id.clone() })
            .await
            .map_err(|_| lost())?
            .ok_or_else(|| unknown(&args.id))?;

        let brief = format!(
            "{}\n\n# 上一次\n{}\n\n# 对方刚说\n{}",
            task.brief, task.said, args.text
        );

        let _ = self
            .records
            .tell(ledger::Update(Task {
                brief,
                status: Status::OnSlot,
                said: String::new(),
                report: None,
                ..task
            }))
            .await;

        let _ = self.odiva.tell(odiva::Fill).await;
        Ok(said("交代完了。"))
    }
}

pub struct Cancel {
    odiva: ActorRef<Odiva>,
}

impl Cancel {
    pub fn new(odiva: ActorRef<Odiva>) -> Self {
        Self { odiva }
    }
}

#[async_trait::async_trait]
impl AgentTool for Cancel {
    fn name(&self) -> &str {
        "task_cancel"
    }

    fn label(&self) -> &str {
        "Cancel"
    }

    fn description(&self) -> &str {
        "Stop a task; its process ends where it stands. Side effects already made are not undone."
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

        let _ = self
            .odiva
            .tell(odiva::CancelTask { task: args.id })
            .await;

        Ok(said("停了。"))
    }
}

#[derive(Debug, Deserialize)]
struct PermitArgs {
    id: String,
    allow: bool,
}

pub struct Permit {
    odiva: ActorRef<Odiva>,
}

impl Permit {
    pub fn new(odiva: ActorRef<Odiva>) -> Self {
        Self { odiva }
    }
}

#[async_trait::async_trait]
impl AgentTool for Permit {
    fn name(&self) -> &str {
        "task_permit"
    }

    fn label(&self) -> &str {
        "Permit"
    }

    fn description(&self) -> &str {
        "Answer a task stopped on a permission request: may it touch something outside its \
         workspace, this once?"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "allow": { "type": "boolean", "description": "True allows it this once; false refuses." }
            },
            "required": ["id", "allow"]
        })
    }

    async fn execute(&self, params: Value, _ctx: ToolContext) -> Result<ToolResult, ToolError> {
        let args: PermitArgs = serde_json::from_value(params)
            .map_err(|error| ToolError::InvalidArgs(error.to_string()))?;

        let _ = self
            .odiva
            .tell(odiva::Permit {
                task: args.id,
                allow: args.allow,
            })
            .await;

        Ok(said(if args.allow { "批了。" } else { "拒了。" }))
    }
}

pub fn roster(records: ActorRef<Records>, odiva: ActorRef<Odiva>) -> Vec<Box<dyn AgentTool>> {
    vec![
        Box::new(Dispatch::new(
            records.clone(),
            odiva.clone(),
            Origin::User,
        )),
        Box::new(List::new(records.clone())),
        Box::new(Read::new(records.clone())),
        Box::new(Answer::new(records.clone(), odiva.clone())),
        Box::new(Cancel::new(odiva.clone())),
        Box::new(Permit::new(odiva)),
    ]
}
