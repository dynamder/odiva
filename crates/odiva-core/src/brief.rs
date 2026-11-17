//! What a task is handed: a brief a stranger could act on, and the envelope a
//! turn has to end with.

use serde_json::json;

use crate::task::Task;

/// The most workers that may run at once.
pub const SLOTS: usize = 3;

/// What a harness is told, every time. The envelope is not optional: it is how
/// a turn ends with something the core can act on.
pub fn prompt(brief: &str) -> String {
    format!(
        "You work in a directory of your own, and this turn is all the time you get.\n\
         Someone reads what you write; the last block is parsed.\n\
         \n\
         End with a report:\n\
         \n\
         ```report\n\
         {{\"status\": \"done\" | \"need\" | \"blocked\", \"question\": \"…\", \"artifacts\": [\"…\"]}}\n\
         ```\n\
         \n\
         done means finished; need means one sentence would unblock you (put it in question);\n\
         blocked means you are stuck. Write plainly; nobody wants a status report.\n\
         \n\
         # The job\n\
         {brief}\n"
    )
}

/// The `dispatch` arguments, rendered into a brief.
pub fn brief(goal: &str, files: &[String], constraints: &[String]) -> String {
    let mut brief = goal.trim().to_owned();

    if !files.is_empty() {
        brief.push_str("\n\n# Files worth reading\n");
        for file in files {
            brief.push_str(&format!("- {file}\n"));
        }
    }

    if !constraints.is_empty() {
        brief.push_str("\n# Constraints\n");
        for constraint in constraints {
            brief.push_str(&format!("- {constraint}\n"));
        }
    }

    brief
}

/// What `task_read` returns and a card shows.
pub fn reading(task: &Task) -> String {
    let mut text = format!("# {}\n{}\n\n", task.id, task.line());

    if let Some(report) = &task.report {
        text.push_str(&format!(
            "report: {}\n",
            serde_json::to_string(report).unwrap_or_default()
        ));
    }

    if !task.said.is_empty() {
        text.push_str("\n## Its last words\n");
        text.push_str(&task.said);
        text.push('\n');
    }

    text
}

/// Kept next to the tool so a schema and its struct stay together.
pub fn dispatch_arguments() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "goal": { "type": "string", "description": "要它做什么，一两句话说清。" },
            "files": {
                "type": "array",
                "items": { "type": "string" },
                "description": "相关的文件路径。它拿到的是自己的目录，所以要写绝对路径。"
            },
            "constraints": {
                "type": "array",
                "items": { "type": "string" },
                "description": "约束：不要动哪里、用哪个分支、怎么算完成。"
            }
        },
        "required": ["goal"]
    })
}
