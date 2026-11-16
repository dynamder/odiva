//! The palette every recipe reads.
//!
//! Recipes ask for a role, never for a raw colour, so every recipe agrees with
//! the rest of the application in every theme. All recipes read the same struct,
//! so a whole visual language can be moved at once by [`Tokens::mix`].

use iced_core::animation::Interpolable;
use iced_core::theme::palette;
use iced_core::{Color, Theme};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    pub canvas: Color,
    pub surface: Color,
    pub surface_hover: Color,
    pub surface_cursor: Color,
    pub surface_selected: Color,
    pub border: Color,
    pub text: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub on_accent: Color,
    pub danger: Color,
    pub on_danger: Color,
    pub info: Color,
    pub warning: Color,
    pub danger_text: Color,
    pub info_text: Color,
    pub warning_text: Color,
    pub scrim: Color,
}

impl Tokens {
    pub fn from_theme(theme: &Theme) -> Self {
        let extended = theme.extended_palette();
        let canvas = extended.background.base.color;
        let text = extended.background.base.text;

        Self {
            canvas,
            surface: extended.background.weak.color,
            surface_hover: extended.background.neutral.color,
            surface_cursor: extended.background.strong.color,
            surface_selected: extended.primary.weak.color,
            border: extended.background.stronger.color,
            text,
            text_muted: palette::deviate(text, 0.15),
            accent: extended.primary.base.color,
            on_accent: extended.primary.base.text,
            danger: extended.danger.base.color,
            on_danger: extended.danger.base.text,
            info: extended.secondary.base.color,
            warning: extended.warning.base.color,
            danger_text: palette::readable(canvas, extended.danger.base.color),
            info_text: palette::readable(canvas, extended.secondary.base.color),
            warning_text: palette::readable(canvas, extended.warning.base.color),
            scrim: Color {
                a: 0.45,
                ..Color::BLACK
            },
        }
    }

    /// The palette `ratio` of the way from `self` to `other`. Same line as
    /// [`Interpolable::interpolated`], spelled as a free function of the two ends.
    pub fn mix(from: Self, to: Self, ratio: f32) -> Self {
        from.interpolated(to, ratio)
    }

    /// The same palette, `factor` times as opaque.
    ///
    /// This is what a fade is: iced draws no subtree at reduced opacity, so a
    /// fading panel has to be a panel whose colours are fading. Every role is
    /// scaled, so an already translucent role — the scrim — fades proportionally
    /// instead of jumping to full opacity.
    pub fn fade(self, factor: f32) -> Self {
        let faded = |color: Color| color.scale_alpha(factor);

        Self {
            canvas: faded(self.canvas),
            surface: faded(self.surface),
            surface_hover: faded(self.surface_hover),
            surface_cursor: faded(self.surface_cursor),
            surface_selected: faded(self.surface_selected),
            border: faded(self.border),
            text: faded(self.text),
            text_muted: faded(self.text_muted),
            accent: faded(self.accent),
            on_accent: faded(self.on_accent),
            danger: faded(self.danger),
            on_danger: faded(self.on_danger),
            info: faded(self.info),
            warning: faded(self.warning),
            danger_text: faded(self.danger_text),
            info_text: faded(self.info_text),
            warning_text: faded(self.warning_text),
            scrim: faded(self.scrim),
        }
    }
}

impl Interpolable for Tokens {
    /// Linear in light, not in sRGB: each role goes through `palette::mix`,
    /// which converts before averaging.
    fn interpolated(&self, other: Self, ratio: f32) -> Self {
        if ratio <= 0.0 {
            return *self;
        }

        if ratio >= 1.0 {
            return other;
        }

        let mix = |a, b| palette::mix(a, b, ratio);

        Self {
            canvas: mix(self.canvas, other.canvas),
            surface: mix(self.surface, other.surface),
            surface_hover: mix(self.surface_hover, other.surface_hover),
            surface_cursor: mix(self.surface_cursor, other.surface_cursor),
            surface_selected: mix(self.surface_selected, other.surface_selected),
            border: mix(self.border, other.border),
            text: mix(self.text, other.text),
            text_muted: mix(self.text_muted, other.text_muted),
            accent: mix(self.accent, other.accent),
            on_accent: mix(self.on_accent, other.on_accent),
            danger: mix(self.danger, other.danger),
            on_danger: mix(self.on_danger, other.on_danger),
            info: mix(self.info, other.info),
            warning: mix(self.warning, other.warning),
            danger_text: mix(self.danger_text, other.danger_text),
            info_text: mix(self.info_text, other.info_text),
            warning_text: mix(self.warning_text, other.warning_text),
            scrim: mix(self.scrim, other.scrim),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_palette_walk_fixes_its_ends_and_moves_every_role() {
        let dark = Tokens::from_theme(&Theme::Dark);
        let light = Tokens::from_theme(&Theme::Light);

        assert_eq!(Tokens::mix(dark, light, 0.0), dark);
        assert_eq!(Tokens::mix(dark, light, 1.0), light);

        let middle = Tokens::mix(dark, light, 0.5);

        assert_ne!(middle, dark);
        assert_ne!(middle, light);
        assert_eq!(middle.canvas, palette::mix(dark.canvas, light.canvas, 0.5));
        assert_eq!(middle.text, palette::mix(dark.text, light.text, 0.5));
    }

    #[test]
    fn a_mid_point_is_a_mid_point_of_light() {
        let middle = palette::mix(Color::BLACK, Color::WHITE, 0.5);

        assert!((middle.r - 0.5).abs() > 0.1, "got {}", middle.r);
        assert!((middle.r - 0.735).abs() < 0.01, "got {}", middle.r);
    }

    #[test]
    fn alpha_stays_linear() {
        let a = Color {
            a: 0.0,
            ..Color::BLACK
        };
        let b = Color {
            a: 1.0,
            ..Color::BLACK
        };

        assert_eq!(palette::mix(a, b, 0.5).a, 0.5);
    }

    #[test]
    fn a_fade_scales_every_role_and_nothing_else() {
        let tokens = Tokens::from_theme(&Theme::Dark);

        let gone = tokens.fade(0.0);
        let half = tokens.fade(0.5);

        assert_eq!(gone.canvas.a, 0.0);
        assert_eq!(gone.text.a, 0.0);
        assert_eq!(
            half.scrim.a,
            tokens.scrim.a * 0.5,
            "a scrim fades, it does not jump"
        );

        assert_eq!(half.accent.r, tokens.accent.r);
        assert_eq!(half.text.g, tokens.text.g);
    }
}
