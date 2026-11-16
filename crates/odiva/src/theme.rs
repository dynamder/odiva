//! The palette.
//!
//! Two themes, both this application's own: quiet surfaces and one accent for
//! what a hand can act on. Everything downstream derives from these six values —
//! `ericed::style::Tokens` reads iced's *extended* palette, which iced generates
//! from the base one.

use iced::theme::Palette;
use iced::{Color, Theme};

/// The theme the window opens in. Dark because a companion on a desk shares the
/// screen with an editor and a terminal all day.
pub fn dark() -> Theme {
    Theme::custom(
        "odiva dark",
        Palette {
            background: color(0x101_116),
            text: color(0xE8_EA_EF),
            primary: color(0x5B_8CFF),
            success: color(0x46_C08A),
            warning: color(0xE3_AE_57),
            danger: color(0xEB_5F5A),
        },
    )
}

/// The same window in daylight.
pub fn light() -> Theme {
    Theme::custom(
        "odiva light",
        Palette {
            background: color(0xFF_FF_FF),
            text: color(0x17_18_1C),
            primary: color(0x2F_6BFF),
            success: color(0x1F_9A_66),
            warning: color(0xB4_7210),
            danger: color(0xC4_3B36),
        },
    )
}

const fn color(rgb: u32) -> Color {
    Color::from_rgb(
        ((rgb >> 16) & 0xFF) as f32 / 255.0,
        ((rgb >> 8) & 0xFF) as f32 / 255.0,
        (rgb & 0xFF) as f32 / 255.0,
    )
}
