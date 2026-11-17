use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use iced::Task;
use odiva::App;
use odiva_core::{Harness, Journal, Model, Route, Store, prompt, start};
use yoagent::provider::ModelConfig;

fn main() -> iced::Result {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a tokio runtime");

    let root = data_root();
    let journal = Journal::new(root.join("odiva.log"));
    let harness = Harness::new(std::env::var("ODIVA_HARNESS").unwrap_or_else(|_| "dsh".to_owned()));

    listen(journal.clone());

    for line in [
        format!("启动：数据目录 {}", root.display()),
        format!("启动：模型 deepseek-flash，key {}", key_state()),
        format!("启动：harness {}", harness_name()),
        format!("启动：日志 {}（ODIVA_LOG=debug 逐条记）", journal.path().display()),
    ] {
        println!("odiva: {line}");
        journal.line(&line);
    }

    let store = Store::new(root);
    let route: Arc<dyn Route> = Arc::new(Model::new(ModelConfig::deepseek(
        "deepseek-flash",
        "DeepSeek Flash",
    )));
    let (handle, messages) = runtime.block_on(async { start(store, route, prompt::idle(), harness) });

    let mut app = App::empty().connected(handle, messages);

    if std::env::var_os("DEEPSEEK_API_KEY").is_none() {
        app = app.with_notice("没有 DEEPSEEK_API_KEY：模型不会回话。设好再启动，原因记在日志里。");
        eprintln!("odiva: 没有 DEEPSEEK_API_KEY，模型不会回话");
    }

    let booting = Mutex::new(Some(app));

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

/// Sends `log` records to the terminal and the journal.
///
/// iced reports the one thing a window cannot see about itself — a frame that
/// never settles ("More than 3 consecutive RedrawRequested events produced
/// layout invalidation") — through `log`, and a window that drops it is a window
/// that hides its own flicker. Warnings and up by default; `ODIVA_LOG=debug` adds
/// the window's own line per message, which is where a list that moves on its own
/// can be read.
fn listen(journal: Journal) {
    struct Records(Journal);

    impl log::Log for Records {
        fn enabled(&self, metadata: &log::Metadata) -> bool {
            metadata.level() <= log::max_level()
        }

        fn log(&self, record: &log::Record) {
            if !self.enabled(record.metadata()) {
                return;
            }

            let line = format!(
                "{} {}: {}",
                record.level(),
                record.target(),
                record.args()
            );

            eprintln!("odiva: {line}");
            self.0.line(&line);
        }

        fn flush(&self) {}
    }

    if let Err(error) = log::set_boxed_logger(Box::new(Records(journal))) {
        eprintln!("odiva: 装不上日志器，iced 的警告看不到：{error}");
        return;
    }

    log::set_max_level(level());
}

fn level() -> log::LevelFilter {
    match std::env::var("ODIVA_LOG").as_deref().map(str::trim) {
        Ok("trace") => log::LevelFilter::Trace,
        Ok("debug") => log::LevelFilter::Debug,
        Ok("info") => log::LevelFilter::Info,
        Ok("error") => log::LevelFilter::Error,
        Ok("off") => log::LevelFilter::Off,
        _ => log::LevelFilter::Warn,
    }
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
