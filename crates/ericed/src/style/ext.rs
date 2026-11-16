//! The wiring from a kind to a widget.
//!
//! [`StyleExt`] is generic over the kind rather than carrying an associated type,
//! so a widget can be painted by any kind that answers its recipe — including one
//! defined in a crate this one has never heard of.

use iced_core::Theme;
use iced_widget::{Button, Container, Rule, Text};

use crate::style::Tokens;
use crate::style::recipe::{ButtonRecipe, ContainerRecipe, RuleRecipe, TextRecipe};

/// Paints a widget from a kind.
///
/// `'a` is the lifetime of the widget and of the palette function.
pub trait StyleExt<'a, K> {
    /// Paints from the palette the theme itself provides.
    fn styled(self, kind: K) -> Self
    where
        Self: Sized,
    {
        self.styled_with(kind, Tokens::from_theme)
    }

    /// Paints from a palette derived from the theme, once per draw.
    ///
    /// This is how a fade works: iced cannot draw a subtree at reduced opacity,
    /// so the palette itself has to be injectable:
    ///
    /// ```ignore
    /// container(card).styled_with(kind, move |theme| {
    ///     Tokens::from_theme(theme).fade(track.value())
    /// })
    /// ```
    fn styled_with(self, kind: K, tokens: impl Fn(&Theme) -> Tokens + 'a) -> Self;
}

impl<'a, M: 'a, K: ContainerRecipe + Copy + 'a> StyleExt<'a, K> for Container<'a, M> {
    fn styled_with(self, kind: K, tokens: impl Fn(&Theme) -> Tokens + 'a) -> Self {
        let frame = kind.frame();

        self.padding(frame.padding)
            .width(frame.width)
            .height(frame.height)
            .clip(frame.clip)
            .style(move |theme: &Theme| kind.paint(&tokens(theme)))
    }
}

impl<'a, K: TextRecipe + Copy + 'a> StyleExt<'a, K> for Text<'a> {
    fn styled_with(self, kind: K, tokens: impl Fn(&Theme) -> Tokens + 'a) -> Self {
        self.size(kind.size())
            .style(move |theme: &Theme| kind.paint(&tokens(theme)))
    }
}

impl<'a, K: RuleRecipe + Copy + 'a> StyleExt<'a, K> for Rule<'a> {
    fn styled_with(self, kind: K, tokens: impl Fn(&Theme) -> Tokens + 'a) -> Self {
        self.style(move |theme: &Theme| kind.paint(&tokens(theme)))
    }
}

impl<'a, M: 'a, K: ButtonRecipe + Copy + 'a> StyleExt<'a, K> for Button<'a, M> {
    fn styled_with(self, kind: K, tokens: impl Fn(&Theme) -> Tokens + 'a) -> Self {
        self.style(move |theme: &Theme, status| kind.paint(&tokens(theme), status))
    }
}
