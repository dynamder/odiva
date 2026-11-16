//! Opaque layers.
//!
//! A `Stack` stops presses at the first layer that captures them, but `on_enter`
//! never captures, so a cursor over a panel still lights up the rows below it.
//! What stops those rows is a non-[`None`] [`mouse::Interaction`] reported at the
//! cursor: the background colour does not block clicks, only the interaction
//! does.

use iced_core::mouse;
use iced_widget::{Container, MouseArea, mouse_area};

/// Marks a container as solid to everything below it, without a gesture of its
/// own. Use [`AimExt::aimed`](crate::AimExt::aimed) when the region also has a
/// gesture; that one is already opaque.
pub trait OpaqueExt<'a, M: 'a> {
    fn opaque(self) -> MouseArea<'a, M>;
}

impl<'a, M: 'a> OpaqueExt<'a, M> for Container<'a, M> {
    fn opaque(self) -> MouseArea<'a, M> {
        mouse_area(self).interaction(mouse::Interaction::Idle)
    }
}
