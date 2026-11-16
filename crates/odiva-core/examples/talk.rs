//! One turn against a harness you name, for trying the client on a real one.
//!
//! ```sh
//! cargo run -p odiva-core --example talk -- /path/to/harness --any --args
//! ```

use std::path::PathBuf;

use odiva_core::acp::Worker;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut command = std::env::args().skip(1);
    let program = command
        .next()
        .expect("usage: talk <harness> [args...]");
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/scratch");

    std::fs::create_dir_all(&cwd)?;

    let mut worker = Worker::new(program, &cwd);

    for arg in command {
        worker = worker.arg(arg);
    }

    let turn = worker.talk("用一句话说明你是做什么的。").await?;

    println!("stop: {:?}", turn.stop);
    println!("said: {}", turn.said);

    Ok(())
}
