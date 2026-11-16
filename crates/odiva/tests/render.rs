//! Screenshots, because the thing being decided is what the window looks like.
//!
//! Each case is a *script* of `CoreMessage`s replayed into a fresh window, not a
//! hand-built model, so every screenshot is a test of the seam as well as a
//! picture. Times are fixed rather than "now", so the pictures are the same every
//! run.

use std::path::PathBuf;
use std::time::Duration;

use iced::{Element, Size, Task, Theme, window};

use odiva::{App, Message, theme};
use odiva_proto::{CoreMessage, FileRef, Presence, Side, Snapshot, WireMessage};

const NOW: i64 = 1_773_000_000_000;

fn minutes_before(now: i64, minutes: i64) -> i64 {
    now - minutes * 60_000
}

fn attached(messages: Vec<WireMessage>) -> CoreMessage {
    CoreMessage::CoreAttachedSession {
        snapshot: Snapshot {
            messages,
            presence: Presence::Online,
        },
    }
}

fn said(id: u64, sent: i64, from: Side, markdown: &str) -> WireMessage {
    WireMessage::new(id, sent, from, markdown)
}

/// A request, an answer in pieces, and one question with replies.
fn chat() -> Vec<CoreMessage> {
    let mut question = said(
        5,
        minutes_before(NOW, 7),
        Side::Assistant,
        "另外 README 第 12–18 行还是旧路径，我直接覆盖？",
    );
    question.replies = vec!["改吧".to_owned(), "先看 diff".to_owned()];

    vec![attached(vec![
        said(
            1,
            minutes_before(NOW, 9),
            Side::User,
            "odiva 的 ACP 客户端连不上 dsh，先看日志；顺便把 README 里的启动命令改一下",
        ),
        said(
            2,
            minutes_before(NOW, 8),
            Side::Assistant,
            "两个问题，我一起看。日志这边先抓到 `initialize` 这一段：",
        ),
        said(
            3,
            minutes_before(NOW, 8),
            Side::Assistant,
            "```\nerror: initialize timed out after 30s\n  at acp::client::transport.rs:41\n```",
        ),
        said(
            4,
            minutes_before(NOW, 7),
            Side::Assistant,
            "读完 214 行日志了，怀疑 stdio 没有 flush。继续查，有结论再跟你说。",
        ),
        question,
    ])]
}

/// The same conversation, with her working on the next thing: nothing new to
/// read, and the composer offering to stop.
fn working() -> Vec<CoreMessage> {
    let mut script = chat();

    script.push(CoreMessage::PresenceChanged {
        presence: Presence::Typing,
    });

    script
}

/// The same conversation, with the last turn having failed.
fn failed() -> Vec<CoreMessage> {
    let mut script = chat();

    script.push(CoreMessage::CoreFailed {
        note: "这一轮没跑完：no API key for provider route \"deepseek\"".to_owned(),
    });

    script
}

/// Six jobs at once, said in two messages: the count lives in the agent's
/// sentence and a list, not in six bubbles or a widget that knows what a task is.
fn busy() -> Vec<CoreMessage> {
    let mut question = said(
        3,
        minutes_before(NOW, 12),
        Side::Assistant,
        "`migrate_v1.sh` 还有别的地方在引用，删之前要不要我全仓搜一遍？",
    );
    question.replies = vec!["搜一遍".to_owned(), "直接删".to_owned()];

    let mut report = said(
        2,
        minutes_before(NOW, 13),
        Side::Assistant,
        "拆成六件，五件已经在跑了。

- 修 flaky 测试 · dsh · 2m10s
- 更新 README 命令 · 等 #44 落地（两件会碰同一个 crate，我串行了）
- 查依赖里那个 *yanked* 版本 · codex · 1m02s
- 删旧迁移脚本 · 卡在等你点头
- 补 acp 集成测试 · 已完成，12 个用例
- 整理上周会议记录 · 已完成，存进 memory",
    );
    report.files.push(FileRef {
        name: "本周计划.md".to_owned(),
        size: "3.1 KB".to_owned(),
    });

    vec![attached(vec![
        said(
            1,
            minutes_before(NOW, 14),
            Side::User,
            "这周的几件事都推进一下，顺手把那个 flaky 测试修了",
        ),
        report,
        question,
    ])]
}

/// A conversation long enough that building it whole would be the bug.
fn long() -> Vec<CoreMessage> {
    let mut messages = Vec::new();

    for slot in 0..300u64 {
        let from = if slot % 2 == 0 {
            Side::User
        } else {
            Side::Assistant
        };

        let markdown = match slot % 37 {
            0 => "```sh\ncargo test --workspace --offline\n```".to_owned(),
            5 => format!("第 {slot} 条：这一条的用处是让列表足够长，长到必须开窗。"),
            _ => format!(
                "第 {slot} 条消息。{}",
                "这一行故意写长一点，让高度不均匀，虚拟列表的估算才有东西可纠正。"
            ),
        };

        messages.push(said(slot + 1, minutes_before(NOW, 300 - slot as i64), from, &markdown));
    }

    vec![attached(messages)]
}

/// Which conversation to photograph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fixture {
    Empty,
    Chat,
    Working,
    Failed,
    Busy,
    Long,
}

impl Fixture {
    fn script(self) -> Vec<CoreMessage> {
        match self {
            Self::Empty => Vec::new(),
            Self::Chat => chat(),
            Self::Working => working(),
            Self::Failed => failed(),
            Self::Busy => busy(),
            Self::Long => long(),
        }
    }
}

struct Harness {
    fixture: Fixture,
    following: bool,
    light: bool,
}

impl Harness {
    fn palette(&self) -> Theme {
        if self.light {
            theme::light()
        } else {
            theme::dark()
        }
    }
}

impl iced::Program for Harness {
    type State = App;
    type Message = Message;
    type Theme = Theme;
    type Renderer = iced::Renderer;
    type Executor = iced::executor::Default;

    fn name() -> &'static str {
        "odiva"
    }

    fn settings(&self) -> iced::Settings {
        iced::Settings::default()
    }

    fn window(&self) -> Option<window::Settings> {
        None
    }

    fn boot(&self) -> (App, Task<Message>) {
        let mut app = App::empty()
            .with_theme(self.palette())
            .with_clock(NOW);

        for message in self.fixture.script() {
            let _ = app.apply(message);
        }

        let app = app.with_viewport(583.0).following(self.following);

        (app, Task::none())
    }

    fn update(&self, state: &mut App, message: Message) -> Task<Message> {
        state.update(message)
    }

    fn view<'a>(
        &self,
        state: &'a App,
        _window: window::Id,
    ) -> Element<'a, Message, Theme, iced::Renderer> {
        state.view()
    }

    fn theme(&self, state: &App, _window: window::Id) -> Option<Theme> {
        Some(state.theme())
    }
}

/// The workspace's `target/`, not the crate's.
fn target_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("crate lives in <workspace>/crates/<crate>")
        .join("target")
}

#[test]
fn render_screenshots() {
    let cases = [
        ("chat-empty", Fixture::Empty, true, false),
        ("chat", Fixture::Chat, true, false),
        ("chat-working", Fixture::Working, true, false),
        ("chat-failed", Fixture::Failed, true, false),
        ("chat-busy", Fixture::Busy, true, false),
        ("chat-long", Fixture::Long, false, false),
        ("chat-light", Fixture::Chat, true, true),
    ];

    for (name, fixture, following, light) in cases {
        let harness = Harness {
            fixture,
            following,
            light,
        };

        let shot = iced_test::screenshot(
            &harness,
            &harness.palette(),
            Size::from((1040, 700)),
            1.0,
            Duration::from_millis(300),
        );

        let path = target_dir().join(format!("ui-{name}.png"));
        let file = std::fs::File::create(&path).unwrap();
        let mut encoder = png::Encoder::new(
            std::io::BufWriter::new(file),
            shot.size.width,
            shot.size.height,
        );

        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&shot.rgba)
            .unwrap();

        eprintln!("screenshot: {}", path.display());
    }
}
