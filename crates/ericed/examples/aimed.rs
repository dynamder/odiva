//! The smallest consumer: two impls and three lines of view.
//!
//! `cargo run -p ericed --example aimed` proves that a second crate can use the
//! extension without copying anything out of it, and asserts the seam as it runs.

use ericed::{Aim, AimExt, Aimed, Meaning};
use iced_widget::{MouseArea, container, text};

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct Entry(usize);

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum Message {
    Entry(Aimed<Entry>),
}

impl From<Aimed<Entry>> for Message {
    fn from(aimed: Aimed<Entry>) -> Self {
        Message::Entry(aimed)
    }
}

impl Meaning<&'static str> for Entry {
    fn meaning(self, aim: Aim) -> Option<&'static str> {
        match aim {
            Aim::Press => Some("focus"),
            Aim::DoubleClick => Some("open"),
            Aim::RightPress => Some("menu"),
        }
    }
}

fn row() -> MouseArea<'static, Message> {
    container(text("README.md")).aimed(Entry(3))
}

fn main() {
    let _row = row();

    assert_eq!(
        ericed::interpret(Aimed::new(Aim::DoubleClick, Entry(3))),
        Some("open")
    );
}
