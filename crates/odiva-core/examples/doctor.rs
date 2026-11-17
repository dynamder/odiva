//! Asks the configured harness whether it really speaks ACP — no model, no turn.
//!
//! ```sh
//! cargo run -p odiva-core --example doctor
//! cargo run -p odiva-core --example doctor -- D:\some\workspace
//! ```
//!
//! What it proves: the program resolves, the launcher shim (`.cmd`) starts, the
//! handshake completes, and `session/new` opens a session in the directory. What
//! it cannot prove: that the harness has a model credential — that takes a turn.

use std::path::PathBuf;

use odiva_core::configure;

#[tokio::main]
async fn main() {
    let cwd = match std::env::args().nth(1) {
        Some(path) => PathBuf::from(path),
        None => std::env::current_dir().expect("a current directory"),
    };

    let harness = configure::harness();

    println!("harness: {harness:?}");

    for line in configure::diagnostics() {
        println!("注意：{line}");
    }
    println!("工作目录: {}", cwd.display());

    let worker = harness.worker(&cwd);

    match worker.probe().await {
        Ok(probe) => {
            println!("OK · session {}", probe.session);
            println!(
                "session/load: {}",
                if probe.load_session { "支持" } else { "不支持（恢复会开新会话）" }
            );
        }
        Err(error) => {
            eprintln!("失败：{error}");
            std::process::exit(1);
        }
    }
}
