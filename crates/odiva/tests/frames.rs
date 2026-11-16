//! The transcript, redrawn frame by frame.
//!
//! A list that flickers is a list whose frames disagree, and one frame can never
//! show that. `iced_winit` draws a frame like this: hand a `RedrawRequested` to
//! the viewport, hand whatever that produced to the app, run the widget
//! operations the app asked for, and go round again while the round was not
//! quiet — at most three times, and it warns in the log when it hits that limit.
//! This is that loop with no window manager and no clock in it, so frames can be
//! counted, printed and compared.
//!
//! The frame is a picture, so the picture is what gets compared: the same offset
//! and the same range can still draw two different things. Set `ODIVA_FRAMES=1`
//! to keep them in `target/frame-NN.png`.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use iced_test::core::clipboard::{self, Clipboard};
use iced_test::core::renderer::{self, Headless};
use iced_test::core::time::Instant;
use iced_test::core::widget::{self, Id};
use iced_test::core::{Event, Font, Point, Rectangle, Size, Theme, Vector, mouse, window};
use iced_test::futures::futures::StreamExt;
use iced_test::futures::futures::executor;
use iced_test::renderer::Renderer;
use iced_test::runtime::{Action, Task, UserInterface, task, user_interface};

use odiva::{App, Message, SHEET, theme};
use odiva_proto::{CoreMessage, Presence, Side, Snapshot, WireMessage};

const VIEWPORT: Size = Size::new(1040.0, 700.0);
const CURSOR: Point = Point::new(520.0, 300.0);
const NOW: i64 = 1_773_000_000_000;

/// How many rounds `iced_winit` runs inside one frame before giving up on it.
const ROUNDS: usize = 3;

/// How close to the end still counts as the end, as the app defines it.
const SLACK: f32 = 2.0;

struct Nothing;

impl Clipboard for Nothing {
    fn read(&self, _kind: clipboard::Kind) -> Option<String> {
        None
    }

    fn write(&mut self, _kind: clipboard::Kind, _contents: String) {}
}

/// What one frame looked like.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Shot {
    rounds: usize,
    y: f32,
    height: f32,
    content: f32,
    first: usize,
    last: usize,
    pinned: bool,
    pixels: u64,
}

impl Shot {
    /// Whether the viewport is showing the end of the content.
    fn at_end(&self) -> bool {
        self.content <= self.height || self.y + self.height >= self.content - SLACK
    }

    /// The picture, without what it cost to draw it: a frame that takes four
    /// rounds of measuring to land somewhere still draws the same thing as one
    /// that lands in one.
    fn same_picture(&self, other: &Self) -> bool {
        self.y == other.y
            && self.height == other.height
            && self.content == other.content
            && self.first == other.first
            && self.last == other.last
            && self.pinned == other.pinned
            && self.pixels == other.pixels
    }
}

/// Where the transcript's scrollable is.
#[derive(Default)]
struct Reading {
    read: Option<(f32, f32, f32)>,
}

impl widget::Operation for Reading {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        content: Rectangle,
        translation: Vector,
        _state: &mut dyn widget::operation::Scrollable,
    ) {
        if id == Some(&Id::new(SHEET)) {
            self.read = Some((translation.y, bounds.height, content.height));
        }
    }
}

struct Frames {
    renderer: Renderer,
    cache: user_interface::Cache,
    clipboard: Nothing,
    /// Operations the app asked for before the next redraw.
    pending: Vec<Action<Message>>,
    painted: usize,
}

impl Frames {
    fn new() -> Self {
        let renderer = executor::block_on(<Renderer as Headless>::new(
            Font::DEFAULT,
            16.0.into(),
            None,
        ))
        .expect("a headless renderer");

        Self {
            renderer,
            cache: user_interface::Cache::default(),
            clipboard: Nothing,
            pending: Vec::new(),
            painted: 0,
        }
    }

    /// One redraw, and every round `iced_winit` would run inside it.
    fn draw(&mut self, app: &mut App, events: &[Event]) -> Option<Shot> {
        let mut messages = Vec::new();
        let mut actions: Vec<Action<Message>> = std::mem::take(&mut self.pending);
        let mut rounds = 0;

        let cache = std::mem::take(&mut self.cache);

        let mut ui = UserInterface::build(app.view(), VIEWPORT, cache, &mut self.renderer);

        let (y, height, content) = loop {
            let mut told = vec![Event::Window(window::Event::RedrawRequested(
                Instant::now(),
            ))];

            if rounds == 0 {
                told.extend_from_slice(events);
            }

            let _ = ui.update(
                &told,
                mouse::Cursor::Available(CURSOR),
                &mut self.renderer,
                &mut self.clipboard,
                &mut messages,
            );

            for action in actions.drain(..) {
                if let Action::Widget(mut operation) = action {
                    ui.operate(
                        &self.renderer,
                        &mut widget::operation::black_box(&mut *operation),
                    );
                }
            }

            let mut reading = Reading::default();
            ui.operate(
                &self.renderer,
                &mut widget::operation::black_box(&mut reading),
            );

            let read = reading.read?;

            if messages.is_empty() || rounds >= ROUNDS {
                break read;
            }

            rounds += 1;

            self.cache = ui.into_cache();

            let told: Vec<Message> = std::mem::take(&mut messages);

            for message in told {
                let (produced, wanted) = collect(app.update(message));
                messages.extend(produced);
                actions.extend(wanted);
            }

            let cache = std::mem::take(&mut self.cache);
            ui = UserInterface::build(app.view(), VIEWPORT, cache, &mut self.renderer);
        };

        let pixels = self.paint(&mut ui, app.theme());
        self.cache = ui.into_cache();

        let slice = app.slice();

        Some(Shot {
            rounds,
            y,
            height,
            content,
            first: slice.range().start,
            last: slice.range().end,
            pinned: app.is_pinned(),
            pixels,
        })
    }

    fn paint(
        &mut self,
        ui: &mut UserInterface<'_, Message, Theme, Renderer>,
        theme: Theme,
    ) -> u64 {
        let style = renderer::Style {
            text_color: theme.palette().text,
        };

        ui.draw(
            &mut self.renderer,
            &theme,
            &style,
            mouse::Cursor::Available(CURSOR),
        );

        let rgba = self.renderer.screenshot(
            Size::new(VIEWPORT.width as u32, VIEWPORT.height as u32),
            1.0,
            theme.palette().background,
        );

        if std::env::var_os("ODIVA_FRAMES").is_some() {
            save(&rgba, self.painted);
        }

        self.painted += 1;

        let mut hasher = DefaultHasher::new();
        rgba.hash(&mut hasher);

        hasher.finish()
    }
}

fn save(rgba: &[u8], at: usize) {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("crate lives in <workspace>/crates/<crate>")
        .join("target")
        .join(format!("frame-{at:02}.png"));

    let Ok(file) = std::fs::File::create(&path) else {
        return;
    };

    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(file),
        VIEWPORT.width as u32,
        VIEWPORT.height as u32,
    );

    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);

    if let Ok(mut writer) = encoder.write_header() {
        let _ = writer.write_image_data(rgba);
    }
}

/// Drains a task into the actions it wants performed, feeding back what it
/// produced.
fn collect(task: Task<Message>) -> (Vec<Message>, Vec<Action<Message>>) {
    let Some(mut stream) = task::into_stream(task) else {
        return (Vec::new(), Vec::new());
    };

    let produced: Vec<Action<Message>> = executor::block_on(async {
        let mut produced = Vec::new();

        while let Some(action) = stream.next().await {
            produced.push(action);
        }

        produced
    });

    let mut messages = Vec::new();
    let mut actions = Vec::new();

    for action in produced {
        match action {
            Action::Output(message) => messages.push(message),
            other => actions.push(other),
        }
    }

    (messages, actions)
}

fn said(id: u64, from: Side, markdown: &str) -> WireMessage {
    WireMessage::new(id, 0, from, markdown)
}

/// A conversation long enough that building it whole would be the bug.
fn conversation() -> CoreMessage {
    let messages = (0..60u64)
        .map(|slot| {
            let from = if slot % 2 == 0 {
                Side::User
            } else {
                Side::Assistant
            };

            let markdown = match slot % 17 {
                0 => "```sh\ncargo test --workspace --offline\n```".to_owned(),
                _ => format!("第 {slot} 条消息。这一行故意写长一点，让高度不均匀。"),
            };

            said(slot + 1, from, &markdown)
        })
        .collect();

    CoreMessage::CoreAttachedSession {
        snapshot: Snapshot {
            messages,
            presence: Presence::Online,
        },
    }
}

/// A window on its first frame: nothing said yet, which is how it starts.
fn booted() -> (App, Frames) {
    (
        App::empty().with_theme(theme::dark()).with_clock(NOW),
        Frames::new(),
    )
}

/// Says something, exactly as the core would.
fn tell(app: &mut App, frames: &mut Frames, message: CoreMessage) {
    let (_, wanted) = collect(app.apply(message));
    frames.pending.extend(wanted);
}

fn wheel(lines: f32) -> Vec<Event> {
    vec![
        Event::Mouse(mouse::Event::CursorMoved { position: CURSOR }),
        Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: lines },
        }),
    ]
}

fn show(frames: &[Option<Shot>]) {
    for (at, shot) in frames.iter().enumerate() {
        let Some(shot) = shot else {
            eprintln!("frame {at:2}: no transcript yet");
            continue;
        };

        eprintln!(
            "frame {at:2}: rounds {}  y {:9.2}  height {:6.2}  content {:9.2}  rows {:3}..{:3}  pinned {:5}  end {:5}  pixels {:016x}",
            shot.rounds,
            shot.y,
            shot.height,
            shot.content,
            shot.first,
            shot.last,
            shot.pinned,
            shot.at_end(),
            shot.pixels
        );
    }
}

/// The picture must stop: every frame since the last thing that changed must be
/// the frame before it, pixel for pixel — and it has to be the end of the
/// conversation, not the top of it. A window that believes it is following while
/// its viewport sits at the top draws a slice nobody can see, and it does it
/// still, so "unchanged" alone would pass.
///
/// Three frames at the end of each script, at least: two frames can agree by
/// luck when a list alternates.
fn assert_still(shots: &[Option<Shot>]) {
    let shots: Vec<&Shot> = shots.iter().filter_map(Option::as_ref).collect();

    let last = shots.last().expect("some frames");

    let tail: Vec<&&Shot> = shots
        .iter()
        .rev()
        .take_while(|shot| shot.content == last.content && shot.last == last.last)
        .collect();

    assert!(
        tail.len() >= 3,
        "the script needs three quiet frames at the end, not {}",
        tail.len()
    );

    for pair in tail.windows(2) {
        assert!(
            pair[0].same_picture(pair[1]),
            "redraws of a still window must be the same picture: {:?} then {:?}",
            pair[0],
            pair[1]
        );
    }

    for shot in tail {
        assert!(shot.pinned, "a still window stays with the newest message");
        assert!(
            shot.at_end(),
            "and shows its end, not some other part: {} + {} vs {}",
            shot.y,
            shot.height,
            shot.content
        );
    }
}

#[test]
fn the_bottom_of_a_fresh_conversation_is_still() {
    let (mut app, mut frames) = booted();

    let mut shots = vec![frames.draw(&mut app, &[])];

    tell(&mut app, &mut frames, conversation());

    for _ in 0..6 {
        shots.push(frames.draw(&mut app, &[]));
    }

    show(&shots);

    assert_still(&shots);
}

#[test]
fn scrolling_up_and_back_down_lands_on_a_still_bottom() {
    let (mut app, mut frames) = booted();

    let mut shots = vec![frames.draw(&mut app, &[])];

    tell(&mut app, &mut frames, conversation());

    for _ in 0..2 {
        shots.push(frames.draw(&mut app, &[]));
    }

    for _ in 0..3 {
        shots.push(frames.draw(&mut app, &wheel(3.0)));
    }

    for _ in 0..6 {
        shots.push(frames.draw(&mut app, &wheel(-3.0)));
    }

    show(&shots);

    assert_still(&shots);
}

#[test]
fn a_message_that_arrives_at_the_bottom_keeps_the_viewport_at_the_bottom() {
    let (mut app, mut frames) = booted();

    let mut shots = vec![frames.draw(&mut app, &[])];

    tell(&mut app, &mut frames, conversation());

    for _ in 0..2 {
        shots.push(frames.draw(&mut app, &[]));
    }

    for at in 0..3 {
        let markdown = format!(
            "第 {at} 条新消息。{}",
            "这一行故意写长一点，让新来的行和估算值差得足够远。".repeat(3)
        );

        tell(
            &mut app,
            &mut frames,
            CoreMessage::MessageAppended {
                message: said(100 + at, Side::Assistant, &markdown),
            },
        );

        for _ in 0..3 {
            shots.push(frames.draw(&mut app, &[]));
        }
    }

    show(&shots);

    for (at, shot) in shots.iter().enumerate().skip(2) {
        let shot = shot.as_ref().expect("the transcript is there");

        assert!(shot.pinned, "frame {at} stopped following");
        assert!(shot.at_end(), "frame {at} is not at the end");
    }

    assert_still(&shots);
}
