//! Aimed mouse areas.
//!
//! `mouse_area` hit-tests for you; what it leaves to the caller is three
//! decisions with one right answer:
//!
//! 1. `on_press` and `on_double_click` must name the **same** target.
//! 2. The area must be opaque to the layers below it — a `None` interaction
//!    makes a `Stack` treat it as a filter.
//! 3. It must wrap the node whose bounds **are** the row. Wrapping a `text`
//!    gives a hit box the size of the glyphs.
//!
//! [`AimExt::aimed`] decides those three and nothing else. What a gesture means
//! is the target's business; see [`crate::Meaning`].

use iced_core::mouse;
use iced_widget::{Container, MouseArea, mouse_area};

/// Which gesture happened, with no target attached.
///
/// One variant per `mouse_area` callback that carries a target: `Press` for
/// `on_press`, `DoubleClick` for `on_double_click`, `RightPress` for
/// `on_right_press`. Adding a variant breaks every [`crate::Meaning`] impl.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Aim {
    /// The left button went down.
    Press,
    /// A second press inside the platform's double-click window.
    DoubleClick,
    /// The right button went down.
    RightPress,
}

/// A gesture that already knows what it was aimed at. `T` belongs to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Aimed<T> {
    pub aim: Aim,
    pub target: T,
}

impl<T> Aimed<T> {
    pub fn new(aim: Aim, target: T) -> Self {
        Self { aim, target }
    }

    /// Splits the gesture into the two halves [`crate::interpret`] needs.
    pub fn into_parts(self) -> (Aim, T) {
        (self.aim, self.target)
    }
}

/// Turns a container's bounds into a mouse area aimed at a target.
pub trait AimExt<'a, M: 'a> {
    /// Wraps this container in a [`MouseArea`] that reports every aimed gesture
    /// as `M::from(Aimed::new(aim, target))`.
    ///
    /// The interaction is [`mouse::Interaction::Idle`], not `None`: the area
    /// blocks the layers below without turning the row into a link. Chain
    /// `.interaction(..)` to overwrite it.
    fn aimed<T: Clone + 'a>(self, target: T) -> MouseArea<'a, M>
    where
        M: From<Aimed<T>>;
}

impl<'a, M: 'a> AimExt<'a, M> for Container<'a, M> {
    fn aimed<T: Clone + 'a>(self, target: T) -> MouseArea<'a, M>
    where
        M: From<Aimed<T>>,
    {
        mouse_area(self)
            .interaction(mouse::Interaction::Idle)
            .on_press(M::from(Aimed::new(Aim::Press, target.clone())))
            .on_double_click(M::from(Aimed::new(Aim::DoubleClick, target.clone())))
            .on_right_press(M::from(Aimed::new(Aim::RightPress, target)))
    }
}
