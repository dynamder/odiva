use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use iced::Task;
use odiva::App;
use odiva_core::{Engine, Harness, Journal, Model, Route, Store, prompt};
use yoagent::provider::ModelConfig;

fn main() -> iced::Result {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a tokio runtime");

    let root = data_root();
    let journal = Journal::new(root.join("odiva.log"));
    let harness = Harness::new(std::env::var("ODIVA_HARNESS").unwrap_or_else(|_| "dsh".to_owned()));

    for line in [
        format!("启动：数据目录 {}", root.display()),
        format!("启动：模型 deepseek-flash，key {}", key_state()),
        format!("启动：harness {}", harness_name()),
        format!("启动：日志 {}", journal.path().display()),
    ] {
        println!("odiva: {line}");
        journal.line(&line);
    }

    let store = Store::new(root);
    let route: Arc<dyn Route> = Arc::new(Model::new(ModelConfig::deepseek(
        "deepseek-flash",
        "DeepSeek Flash",
    )));
    let (engine, handle, messages) =
        Engine::new(store, route, prompt::idle(), harness).expect("the engine starts");

    runtime.spawn(engine.run());

    let booting = Mutex::new(Some(App::empty().connected(handle, messages)));

    iced::application(
        move || {
            let app = booting
                .lock()
                .expect("no panic while booting")
                .take()
                .expect("iced boots once");

            (app, Task::none())
        },
        App::update,
        App::view,
    )
    .subscription(App::subscription)
    .theme(App::theme)
    .run()
}

fn key_state() -> &'static str {
    match std::env::var_os("DEEPSEEK_API_KEY") {
        Some(key) if !key.is_empty() => "有",
        _ => "没有（设 DEEPSEEK_API_KEY）",
    }
}

fn harness_name() -> String {
    std::env::var("ODIVA_HARNESS").unwrap_or_else(|_| "dsh".to_owned())
}

fn data_root() -> PathBuf {
    std::env::var_os("ODIVA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_dir()
                .expect("a current directory")
                .join(".odiva")
        })
}
