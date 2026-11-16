//! ICED pieces every application writes again.
//!
//! Nothing here knows the application using it: no target type, no intent type,
//! no colours, no pane names.
//!
//! | item | what it provides |
//! | --- | --- |
//! | [`style`] | typed paint: a kind names what a widget is, a recipe paints it |
//! | [`AimExt`] | one mouse area for every aimed gesture on a container |
//! | [`Meaning`] | what a gesture means, stated per target type |
//! | [`OpaqueExt`] | a layer the ones below cannot see through |
//! | [`WheelArea`] | the wheel, claimed over a region and reported as a direction |
//! | [`list`] | a windowed list: which slots to build, and the padding around them |
//! | [`motion`] | a value that moves, and the frame subscription that moves it |
//!
//! Three mouse facts the types cannot express:
//!
//! * `on_release` never captures and only fires while the pointer is inside, so
//!   a "button is down" flag must be cleared from the event subscription, never
//!   from the widget tree.
//! * Hit testing ignores clipping: `mouse_area` drops the `viewport` it is
//!   handed, and inside a `scrollable` the cursor is levitated by the container
//!   instead; nothing else will save you.
//! * A `Stack` asks its children top-down, so an upper layer's `on_enter`
//!   arrives **before** the lower layer's `on_exit` — the opposite order from a
//!   `Column`. Give each layer its own concern rather than relying on the order.

pub mod list;
pub mod motion;
pub mod style;

mod aim;
mod meaning;
mod opaque;
mod wheel;

pub use aim::{Aim, AimExt, Aimed};
pub use meaning::{Meaning, interpret};
pub use opaque::OpaqueExt;
pub use wheel::{WheelArea, WheelDirection, wheel_area};
