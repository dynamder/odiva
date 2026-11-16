//! A typed visual language for iced widgets.
//!
//! iced styles are closures from a theme to a `Style` struct, which say nothing
//! at a call site about what a widget *is*. Here a **kind** (the application's
//! type) names the widget, a **recipe** says how it paints, and
//! [`StyleExt::styled`] wires the two once per widget type.
//!
//! Kinds stay in the application. What lives here is the machinery and
//! [`Tokens`], derived from `Theme` so a recipe never touches raw colours.

pub mod ext;
pub mod recipe;
pub mod tokens;

pub use ext::StyleExt;
pub use recipe::{ButtonRecipe, ContainerRecipe, Frame, RuleRecipe, TextRecipe};
pub use tokens::Tokens;
