// MSVC's link.exe prints its import-library line to stdout; rustc 1.97 reports any
// linker stdout as `linker_messages`. That line is not a diagnostic.
#![allow(unknown_lints, linker_messages)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use iced::Task;
use odiva::App;
use odiva_core::{Engine, Harness, Model, Route, Store, prompt};
use yoagent::provider::ModelConfig;

fn main() -> iced::Result {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a tokio runtime");

    let store = Store::new(data_root());
    let route: Arc<dyn Route> = Arc::new(Model::new(ModelConfig::deepseek(
        "deepseek-flash",
        "DeepSeek Flash",
    )));
    let harness = Harness::new(std::env::var("ODIVA_HARNESS").unwrap_or_else(|_| "dsh".to_owned()));
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

fn data_root() -> PathBuf {
    std::env::var_os("ODIVA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_dir()
                .expect("a current directory")
                .join(".odiva")
        })
}
