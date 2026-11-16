//! What pointing at something means.
//!
//! The same right-click opens a menu on a row and does nothing on a panel, so
//! the meaning is a method on the target type rather than a table over every
//! target kind. This module knows neither the target nor `I`.

use crate::Aim;
use crate::Aimed;

/// What a target means when it is aimed at.
///
/// Implemented per component and matched exhaustively over [`Aim`], so a new
/// gesture forces a decision in every component instead of defaulting.
pub trait Meaning<I> {
    fn meaning(self, aim: Aim) -> Option<I>;
}

/// The bridge: the only place that switches on the gesture, and it does so
/// generically, so it never has to grow.
pub fn interpret<I, T: Meaning<I>>(aimed: Aimed<T>) -> Option<I> {
    let (aim, target) = aimed.into_parts();

    target.meaning(aim)
}
