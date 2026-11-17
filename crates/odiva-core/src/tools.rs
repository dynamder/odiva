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
use crate::line::{LineId, Mode};
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
                workspace: None,
            })
            .await
            .map_err(|_| lost())?;

        let _ = self.odiva.tell(odiva::Fill).await;

        Ok(said(format!("派出去了：{id}。")))
    }
}

#[derive(Debug, Deserialize)]
struct AcceptArgs {
    brief: String,
    handoff: String,
    workspace: String,
}

/// The idle-to-busy transition, as a tool.
pub struct Accept {
    records: ActorRef<Records>,
    odiva: ActorRef<Odiva>,
}

impl Accept {
    pub fn new(records: ActorRef<Records>, odiva: ActorRef<Odiva>) -> Self {
        Self { records, odiva }
    }
}

#[async_trait::async_trait]
impl AgentTool for Accept {
    fn name(&self) -> &str {
        "accept_task"
    }

    fn label(&self) -> &str {
        "Take the job"
    }

    fn description(&self) -> &str {
        "Take a job the person asked for. Leaving idle is what this does, so the handoff matters: \
         write down what you were doing, because that is what you come back to. `workspace` is the \
         directory the worker may write in."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "brief": { "type": "string", "description": "What has to be done, self-contained." },
                "handoff": { "type": "string", "description": "One or two sentences about what you were doing in idle." },
                "workspace": { "type": "string", "description": "Absolute path the worker may write in." }
            },
            "required": ["brief", "handoff", "workspace"]
        })
    }

    async fn execute(&self, params: Value, _ctx: ToolContext) -> Result<ToolResult, ToolError> {
        let args: AcceptArgs = serde_json::from_value(params)
            .map_err(|error| ToolError::InvalidArgs(error.to_string()))?;

        let id = self
            .records
            .ask(ledger::Reserve {
                origin: Origin::User,
                brief: args.brief,
                workspace: Some(args.workspace.into()),
            })
            .await
            .map_err(|_| lost())?;

        let _ = self
            .odiva
            .tell(odiva::Accept {
                task: id.clone(),
                handoff: crate::handoff::Handoff {
                    note: args.handoff,
                },
            })
            .await;

        Ok(said(format!("接了 {id}，我去忙。")))
    }
}

#[derive(Debug, Deserialize)]
struct SwitchArgs {
    id: String,
    #[serde(default)]
    timeout_secs: Option<u64>,
}

/// Go into one task's context for a while.
pub struct SwitchToTask {
    odiva: ActorRef<Odiva>,
}

impl SwitchToTask {
    pub fn new(odiva: ActorRef<Odiva>) -> Self {
        Self { odiva }
    }
}

#[async_trait::async_trait]
impl AgentTool for SwitchToTask {
    fn name(&self) -> &str {
        "switch_to_task"
    }

    fn label(&self) -> &str {
        "Go to a task"
    }

    fn description(&self) -> &str {
        "Move into one task's context and look at it. This turn ends first; if the task says \
         nothing within the timeout, you are back in the normal line."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "timeout_secs": {
                    "type": "integer",
                    "description": "How long to wait for it to react. Default 600."
                }
            },
            "required": ["id"]
        })
    }

    async fn execute(&self, params: Value, _ctx: ToolContext) -> Result<ToolResult, ToolError> {
        let args: SwitchArgs = serde_json::from_value(params)
            .map_err(|error| ToolError::InvalidArgs(error.to_string()))?;

        let _ = self
            .odiva
            .tell(odiva::Switch {
                line: LineId::task(args.id.clone()),
                timeout: std::time::Duration::from_secs(args.timeout_secs.unwrap_or(600)),
            })
            .await;

        Ok(said(format!("这一轮结束后进 {}。", args.id)))
    }
}

/// Step back out of a task's context.
pub struct SwitchToNormal {
    odiva: ActorRef<Odiva>,
}

impl SwitchToNormal {
    pub fn new(odiva: ActorRef<Odiva>) -> Self {
        Self { odiva }
    }
}

#[async_trait::async_trait]
impl AgentTool for SwitchToNormal {
    fn name(&self) -> &str {
        "switch_to_normal"
    }

    fn label(&self) -> &str {
        "Back to normal"
    }

    fn description(&self) -> &str {
        "Leave this task's context and go back to the normal line. This turn ends first."
    }

    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _params: Value, _ctx: ToolContext) -> Result<ToolResult, ToolError> {
        let _ = self.odiva.tell(odiva::ToNormal).await;
        Ok(said("回 normal。"))
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

/// Where a turn runs: the dispatcher of a state, or one task's own context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Normal,
    Task,
}

/// What a turn may do, from where it runs.
pub fn roster(
    records: ActorRef<Records>,
    odiva: ActorRef<Odiva>,
    mode: Mode,
    position: Position,
) -> Vec<Box<dyn AgentTool>> {
    let mut tools: Vec<Box<dyn AgentTool>> = Vec::new();

    match (mode, position) {
        (Mode::Idle, Position::Normal) => {
            tools.push(Box::new(Dispatch::new(
                records.clone(),
                odiva.clone(),
                Origin::Interest,
            )));
            tools.push(Box::new(List::new(records.clone())));
            tools.push(Box::new(Read::new(records.clone())));
            tools.push(Box::new(Accept::new(records.clone(), odiva.clone())));
            tools.push(Box::new(SwitchToTask::new(odiva.clone())));
        }
        (Mode::Busy, Position::Normal) => {
            tools.push(Box::new(Dispatch::new(
                records.clone(),
                odiva.clone(),
                Origin::User,
            )));
            tools.push(Box::new(List::new(records.clone())));
            tools.push(Box::new(Read::new(records.clone())));
            tools.push(Box::new(SwitchToTask::new(odiva.clone())));
        }
        (_, Position::Task) => {
            tools.push(Box::new(Read::new(records.clone())));
            tools.push(Box::new(Answer::new(records.clone(), odiva.clone())));
            tools.push(Box::new(Cancel::new(odiva.clone())));
            tools.push(Box::new(SwitchToNormal::new(odiva.clone())));

            if mode == Mode::Busy {
                tools.push(Box::new(Permit::new(odiva.clone())));
            }
        }
    }

    tools
}
