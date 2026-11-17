//! Runs the core without a window: type a line, see what comes out.
//!
//! The same engine the window drives, with its messages printed instead of
//! drawn. Useful when the window is quiet and the question is why.
//!
//! ```sh
//! cargo run -p odiva-core --example drive
//! ```

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;

use odiva_core::{Harness, Model, Route, Store, prompt, start_with};
use odiva_proto::{CoreHandle, CoreMessage, Side, UiMessage};
use yoagent::provider::ModelConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::var_os("ODIVA_HOME")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?.join(".odiva"));

    let store = Store::new(root);
    let journal = odiva_core::Journal::new(store.root().join("odiva.log"));
    let route: Arc<dyn Route> = Arc::new(Model::new(ModelConfig::deepseek(
        "deepseek-flash",
        "DeepSeek Flash",
    )));
    let harness = Harness::new(std::env::var("ODIVA_HARNESS").unwrap_or_else(|_| "dsh".to_owned()));

    println!("odiva: 数据目录 {}", store.root().display());
    println!(
        "odiva: 模型 deepseek-flash，key {}",
        if std::env::var_os("DEEPSEEK_API_KEY").is_some() {
            "有"
        } else {
            "没有（设 DEEPSEEK_API_KEY）"
        }
    );
    println!("odiva: 日志 {}", journal.path().display());
    println!("odiva: 输入一句话回车；空行退出。");

    let summariser = Arc::clone(&route);
    let (handle, mut messages) = start_with(store, route, prompt::idle(), harness, Some(summariser));
    tokio::spawn(async move {
        while let Some(message) = messages.recv().await {
            println!("{}", show(&message));
        }
    });

    let stdin = std::io::stdin();

    for line in stdin.lock().lines() {
        let text = line?;

        if text.trim().is_empty() {
            break;
        }

        handle.send(UiMessage::UserSubmittedText { text });
        std::io::stdout().flush()?;
    }

    Ok(())
}

/// One line per event, in the order the window would have drawn them.
fn show(message: &CoreMessage) -> String {
    match message {
        CoreMessage::CoreAttachedSession { snapshot } => {
            format!("— 已连接，{} 条历史 —", snapshot.messages.len())
        }
        CoreMessage::MessageAppended { message } => {
            let who = match message.from {
                Side::User => "你",
                Side::Assistant => "她",
            };

            format!("{who}：{}", message.markdown)
        }
        CoreMessage::PresenceChanged { presence } => format!("— {presence:?} —"),
        CoreMessage::QuestionAsked { question } => {
            let options: Vec<String> = question
                .options
                .iter()
                .enumerate()
                .map(|(index, option)| format!("{index}. {}", option.label))
                .collect();

            format!("？{}\n  {}", question.question, options.join("  "))
        }
        CoreMessage::QuestionClosed { resolution } => format!("— 提问结束：{resolution:?} —"),
        CoreMessage::CoreFailed { note } => format!("错误：{note}"),
    }
}
