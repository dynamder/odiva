//! Screenshots, because the thing being decided is what the window looks like.
//!
//! Each case is a *script* of `CoreMessage`s replayed into a fresh window, not a
//! hand-built model. That makes every screenshot a test of the seam as well as a
//! picture: if the protocol stops being enough to describe a conversation, the
//! picture stops being producible.

use std::path::PathBuf;
use std::time::Duration;

use iced::{Element, Size, Task, Theme, window};

use odiva::{App, Message};
use odiva_proto::{CoreMessage, FileRef, Presence, Side, Snapshot, WireMessage};

fn attached(messages: Vec<WireMessage>) -> CoreMessage {
    CoreMessage::CoreAttachedSession {
        snapshot: Snapshot {
            messages,
            presence: Presence::Online,
        },
    }
}

fn said(id: u64, from: Side, markdown: &str) -> WireMessage {
    WireMessage::new(id, from, markdown)
}

/// A request, an answer in pieces, and one question with replies.
fn chat() -> Vec<CoreMessage> {
    let mut question = said(5, Side::Assistant, "另外 README 第 12–18 行还是旧路径，我直接覆盖？");
    question.replies = vec!["改吧".to_owned(), "先看 diff".to_owned()];

    vec![attached(vec![
        said(
            1,
            Side::User,
            "odiva 的 ACP 客户端连不上 dsh，先看日志；顺便把 README 里的启动命令改一下",
        ),
        said(2, Side::Assistant, "两个问题，我一起看。日志这边先抓到 `initialize` 这一段："),
        said(
            3,
            Side::Assistant,
            "```\nerror: initialize timed out after 30s\n  at acp::client::transport.rs:41\n```",
        ),
        said(
            4,
            Side::Assistant,
            "读完 214 行日志了，怀疑 stdio 没有 flush。继续查，有结论再跟你说。",
        ),
        question,
    ])]
}

/// The same conversation while another message is being written.
fn streaming() -> Vec<CoreMessage> {
    let mut script = chat();

    script.push(CoreMessage::MessageBegan {
        id: 6,
        from: Side::Assistant,
    });
    script.push(CoreMessage::PresenceChanged {
        presence: Presence::Typing,
    });
    script.push(CoreMessage::MessageStreamedText {
        id: 6,
        delta: "README 那里改完了：命令从旧路径换成 `odiva acp --stdio`，另外补了一行依赖说明。\n\n"
            .to_owned(),
    });
    script.push(CoreMessage::MessageStreamedText {
        id: 6,
        delta: "日志那边还有个发现——dsh 在 stdio 模式下把 **stderr 也写进了协议流**，\
                所以握手读到的第一行不是 JSON-RPC，解析器直接把它当垃圾吞了，看起来就像超时。"
            .to_owned(),
    });

    script
}

/// Six jobs at once, said in two messages.
///
/// The count lives in the agent's sentence and a list, because that is how a
/// person would report it — not in six bubbles, and not in an interface widget
/// that knows what a task is.
fn busy() -> Vec<CoreMessage> {
    let mut question = said(
        3,
        Side::Assistant,
        "`migrate_v1.sh` 还有别的地方在引用，删之前要不要我全仓搜一遍？",
    );
    question.replies = vec!["搜一遍".to_owned(), "直接删".to_owned()];

    let mut report = said(
        2,
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

    vec![
        attached(vec![
            said(1, Side::User, "这周的几件事都推进一下，顺手把那个 flaky 测试修了"),
            report,
            question,
        ]),
    ]
}

/// Which conversation to photograph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fixture {
    Chat,
    Streaming,
    Busy,
}

impl Fixture {
    fn script(self) -> Vec<CoreMessage> {
        match self {
            Self::Chat => chat(),
            Self::Streaming => streaming(),
            Self::Busy => busy(),
        }
    }
}

struct Harness {
    fixture: Fixture,
    following: bool,
    light: bool,
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
        let mut app = App::empty();

        for message in self.fixture.script() {
            app.apply(message);
        }

        (app.following(self.following), Task::none())
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
        if self.light {
            Some(Theme::Light)
        } else {
            Some(state.theme())
        }
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
        ("chat", Fixture::Chat, true, false),
        // Mid-reply, scrolled away from the newest line: the state that decides
        // whether the scroll policy and the "jump to newest" affordance are
        // right.
        ("chat-streaming", Fixture::Streaming, false, false),
        // The one that decides the design: six jobs, two messages.
        ("chat-busy", Fixture::Busy, true, false),
        // The same description rendered by another theme, which is the claim a
        // palette makes.
        ("chat-light", Fixture::Chat, true, true),
    ];

    for (name, fixture, following, light) in cases {
        let shot = iced_test::screenshot(
            &Harness {
                fixture,
                following,
                light,
            },
            &if light {
                Theme::Light
            } else {
                Theme::CatppuccinMocha
            },
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
