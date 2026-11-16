//! A value that moves towards a target, and the clock that moves it.
//!
//! iced has the pieces — `Animation` interpolates, `RedrawRequested` is a frame,
//! `Subscription` turns one into a message — and nothing that joins them.
//!
//! A moving value must not move the layout. `pin(content).x(offset)` moves the
//! *layout node*, so the child is laid out with `limits.max() - position`
//! available and text re-wraps on every frame. [`translate`] and [`scale`] apply
//! the offset while drawing instead: the child keeps its width and its shape,
//! and only the pixels move.
//!
//! [`Track`] never reads a clock; it is advanced by [`Track::tick`], which is
//! idempotent, so either an application frame message (`update`) or a widget's
//! `RedrawRequested` (`draw`) can own it without changing a line of drawing code.
//! [`frames`] is the one place that decides whether the subscription exists.

use iced_core::animation::{Animation, Easing};
use iced_core::layout;
use iced_core::mouse;
use iced_core::overlay;
use iced_core::renderer;
use iced_core::time::{Duration, Instant};
use iced_core::widget::tree::{self, Tree};
use iced_core::widget::{self, Widget};
use iced_core::{
    Clipboard, Element, Event, Layout, Length, Point, Rectangle, Shell, Size, Transformation,
    Vector,
};
use iced_futures::Subscription;

/// A duration and the shape of the motion across it.
///
/// The shape is iced's [`Easing`], so a hand-written `fn(f32) -> f32` is
/// [`Curve::within`] rather than a second mechanism.
///
/// A shape may leave the unit interval: `EaseInBack` and `EaseOutElastic`
/// overshoot on purpose, so [`Track::value`] on such a curve can land outside the
/// range between the two ends. Mixing two `Color`s should therefore go through
/// `iced::theme::palette::mix`, which clamps.
#[derive(Debug, Clone, Copy)]
pub struct Curve {
    duration: Duration,
    easing: Easing,
}

impl Curve {
    /// A curve of the given duration, shaped by `easing`.
    pub const fn eased(duration: Duration, easing: Easing) -> Self {
        Self { duration, easing }
    }

    /// A curve of the given duration, shaped by `shape`.
    ///
    /// `shape` takes progress in `0.0..=1.0` and is expected to map `0` to `0`
    /// and `1` to `1`; nothing here checks that it does.
    pub const fn within(duration: Duration, shape: fn(f32) -> f32) -> Self {
        Self::eased(duration, Easing::Custom(shape))
    }

    /// Constant speed.
    pub const fn linear(duration: Duration) -> Self {
        Self::eased(duration, Easing::Linear)
    }

    /// Starts fast, settles into the target.
    pub const fn out(duration: Duration) -> Self {
        Self::eased(duration, Easing::EaseOutCubic)
    }

    /// Slow at both ends.
    pub const fn in_out(duration: Duration) -> Self {
        Self::within(duration, shapes::smootherstep)
    }

    /// How long the motion takes.
    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// The shape, as iced names it.
    pub fn easing(&self) -> Easing {
        self.easing
    }

    /// The shape, from unit progress to unit progress.
    pub fn shape(&self, progress: f32) -> f32 {
        self.easing.value(progress.clamp(0.0, 1.0))
    }
}

mod shapes {
    /// Perlin's smootherstep: zero first *and* second derivative at both ends.
    /// No named iced easing has that — `EaseInOutQuint` comes close — so it stays
    /// a `Custom` shape rather than an approximation.
    pub(super) fn smootherstep(p: f32) -> f32 {
        (6.0 * p.powi(5) - 15.0 * p.powi(4) + 10.0 * p.powi(3)).clamp(0.0, 1.0)
    }
}

/// A value that moves towards a target.
///
/// A plain value: `update` mutates it, `view` reads it. [`Clone`] copies the
/// state, not a handle, so a track per row is cheap to write down.
#[derive(Debug, Clone)]
pub struct Track {
    curve: Curve,
    animation: Animation<f32>,
    pending: Option<f32>,
    now: Instant,
}

impl Track {
    /// A track that is already at `value` and not moving.
    pub fn new(value: f32) -> Self {
        Self {
            curve: Curve::in_out(Duration::from_millis(200)),
            animation: Animation::new(value),
            pending: None,
            now: Instant::now(),
        }
    }

    /// Sets the curve used by *later* transitions. Set it once, at construction.
    pub fn curve(mut self, curve: Curve) -> Self {
        self.curve = curve;
        self
    }

    /// Declares where the value should end up.
    ///
    /// This does **not** start the motion: the start time comes from the next
    /// [`Track::tick`], so a motion always begins on a frame. Declaring the
    /// target that is already on its way is a no-op, so `update` can declare it
    /// on every message without stalling the motion.
    pub fn to(&mut self, target: f32) {
        let upcoming = self.pending.unwrap_or_else(|| self.animation.value());

        if upcoming == target {
            return;
        }

        self.pending = Some(target);
    }

    /// Advances the track to `now`, starting a declared target if there is one.
    ///
    /// Idempotent: two calls with the same instant change nothing.
    pub fn tick(&mut self, now: Instant) {
        self.now = now;

        let Some(target) = self.pending.take() else {
            return;
        };

        self.animation = self
            .animation
            .clone()
            .duration(self.curve.duration())
            .easing(self.curve.easing());

        self.animation.go_mut(target, now);
    }

    /// The value as of the last ticked instant.
    ///
    /// Between ticks the value is exactly stable, so an unrelated rebuild cannot
    /// make the picture jump ahead.
    pub fn value(&self) -> f32 {
        self.at(self.now)
    }

    /// The value at an arbitrary instant, for a caller that already has one.
    pub fn at(&self, now: Instant) -> f32 {
        self.animation.interpolate_with(|value| value, now)
    }

    /// Whether anything is left to do: a target waiting to start, or a motion
    /// that has not finished.
    pub fn is_moving(&self) -> bool {
        self.pending.is_some() || self.animation.is_animating(self.now)
    }
}

/// Applies a track's current value to whatever the caller wants to build.
pub fn animate<B>(track: &Track, f: impl FnOnce(f32) -> B) -> B {
    f(track.value())
}

/// "Is anything in here moving?" Implemented by the application so [`frames`] can
/// ask one question.
pub trait Motion {
    fn is_moving(&self) -> bool;
}

/// The frame subscription, installed only while something moves.
///
/// Subscribed unconditionally, the application redraws forever. `f` has to be a
/// function item or a **non-capturing** closure: `Subscription::map` requires a
/// zero-sized mapper, checked at compile time. A mapper that needs state should
/// be `Subscription::with` instead.
///
/// ```no_run
/// # use ericed::motion::{self, Motion};
/// # use iced_core::time::Instant;
/// # use iced_futures::Subscription;
/// # #[derive(Clone)] enum Message { Frame(Instant) }
/// # struct App;
/// # impl Motion for App { fn is_moving(&self) -> bool { false } }
/// # let app = App;
/// let subscription: Subscription<Message> = motion::frames(&app, Message::Frame);
/// ```
pub fn frames<A, F, Message>(app: &A, f: F) -> Subscription<Message>
where
    A: Motion,
    F: Fn(Instant) -> Message + Clone + iced_futures::MaybeSend + 'static,
    Message: 'static,
{
    if app.is_moving() {
        iced_runtime::window::frames().map(f)
    } else {
        Subscription::none()
    }
}

/// Where a [`Transform`] puts its content.
///
/// Closed on purpose: iced exposes translations and *uniform* scales and nothing
/// else, because `scale_factor()` is one number that both backends use to size
/// glyphs, borders and shadows. An enum makes that limit visible; a raw
/// `Transformation` would let a caller write a matrix the renderer cannot draw.
///
/// Two kinds, because a transformation can be known at two times: an absolute
/// one before layout, and a scaling whose pivot is a property of the content's
/// own box, which only exists after layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Placement {
    /// The same transformation wherever the content lands.
    Absolute(Transformation),
    /// A scaling about a point of the content's own layout.
    Scale { factor: f32, about: Anchor },
}

impl Placement {
    fn resolve(&self, bounds: Rectangle) -> Transformation {
        match *self {
            Self::Absolute(transformation) => transformation,
            Self::Scale { factor, about } => {
                let pivot = about.of(bounds);

                Transformation::translate(pivot.x, pivot.y)
                    * Transformation::scale(factor)
                    * Transformation::translate(-pivot.x, -pivot.y)
            }
        }
    }
}

/// A point of a content's own box to scale about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Anchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
}

impl Anchor {
    /// Where this anchor sits in `bounds`.
    pub fn of(self, bounds: Rectangle) -> Point {
        match self {
            Self::TopLeft => bounds.position(),
            Self::TopRight => Point::new(bounds.x + bounds.width, bounds.y),
            Self::BottomLeft => Point::new(bounds.x, bounds.y + bounds.height),
            Self::BottomRight => Point::new(bounds.x + bounds.width, bounds.y + bounds.height),
            Self::Center => bounds.center(),
        }
    }
}

/// A widget drawn through a [`Transformation`], without being laid out through
/// one. Build one with [`translate`], [`scale`] or [`transform`].
pub struct Transform<'a, Message, Theme = iced_widget::Theme, Renderer = iced_widget::Renderer>
where
    Renderer: iced_core::Renderer,
{
    content: Element<'a, Message, Theme, Renderer>,
    placement: Placement,
    clip: bool,
}

/// Draws `content` shifted by `offset`, leaving the layout exactly as it was.
///
/// The offset is invisible to layout: the space the content *would* occupy is
/// still its unshifted box, so a caller that wants the shift clipped asks for
/// [`Transform::clip`]. Hit testing is adjusted, so the content stays clickable
/// while it moves.
pub fn translate<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    offset: Vector,
) -> Transform<'a, Message, Theme, Renderer>
where
    Renderer: iced_core::Renderer,
{
    transform(
        content,
        Placement::Absolute(Transformation::translate(offset.x, offset.y)),
    )
}

/// Draws `content` scaled by `factor` about its own centre, leaving the layout
/// exactly as it was. A factor below `1.0` works: unlike `iced_widget::Float`,
/// this scales the drawing either way.
pub fn scale<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    factor: f32,
) -> Transform<'a, Message, Theme, Renderer>
where
    Renderer: iced_core::Renderer,
{
    transform(
        content,
        Placement::Scale {
            factor,
            about: Anchor::Center,
        },
    )
}

/// Draws `content` through `placement`, leaving the layout exactly as it was.
///
/// What the widget owes its child, for a transformation to be invisible to
/// everything but the pixels:
///
/// | what | how |
/// | --- | --- |
/// | layout | forwarded untouched |
/// | painting | `with_transformation` around the child's draw |
/// | clipping | the child is handed `viewport * inverse` |
/// | hit testing | the child is handed `cursor * inverse` |
/// | overlays | `translation + transformation.translation()` |
///
/// The last one is a limit of iced, not a trick: `overlay::Element` carries an
/// additive translation and no matrix, so a popup inside a *scaled* subtree is
/// moved but not scaled.
///
/// Unlike `iced_widget::Float`, this stays a widget: it is clipped by its
/// parents, ordered by the tree and hit-testable in place. `Float` leaves the
/// tree and draws through `overlay`, which loses the ancestors' clipping.
pub fn transform<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    placement: Placement,
) -> Transform<'a, Message, Theme, Renderer>
where
    Renderer: iced_core::Renderer,
{
    Transform {
        content: content.into(),
        placement,
        clip: false,
    }
}

impl<Message, Theme, Renderer> Transform<'_, Message, Theme, Renderer>
where
    Renderer: iced_core::Renderer,
{
    /// Clips the drawing to the box the content was laid out in.
    ///
    /// Off by default: a transformation is invisible to layout, so a caller that
    /// wants the overflow gone has to say so. On, it is the *untransformed* box —
    /// what a slide wants, and what a growing card usually does not.
    pub fn clip(mut self, clip: bool) -> Self {
        self.clip = clip;
        self
    }
}

/// The window-space cursor, moved into the child's own space.
///
/// `Cursor * Transformation` applies the matrix without inverting it, so the
/// inverse is the caller's job; this is the line that keeps the child clickable
/// while it moves.
fn into_content(cursor: mouse::Cursor, transformation: Transformation) -> mouse::Cursor {
    cursor * transformation.inverse()
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Transform<'_, Message, Theme, Renderer>
where
    Renderer: iced_core::Renderer,
{
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let transformation = self.placement.resolve(layout.bounds());

        self.content.as_widget_mut().update(
            tree,
            event,
            layout,
            into_content(cursor, transformation),
            renderer,
            clipboard,
            shell,
            &(*viewport * transformation.inverse()),
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let transformation = self.placement.resolve(layout.bounds());

        self.content.as_widget().mouse_interaction(
            tree,
            layout,
            into_content(cursor, transformation),
            &(*viewport * transformation.inverse()),
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let transformation = self.placement.resolve(bounds);
        let cursor = into_content(cursor, transformation);
        let viewport = &(*viewport * transformation.inverse());

        let paint = |renderer: &mut Renderer| {
            self.content
                .as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        };

        if self.clip {
            renderer.with_layer(bounds, paint);
        } else if transformation == Transformation::IDENTITY {
            paint(renderer);
        } else {
            renderer.with_transformation(transformation, paint);
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let transformation = self.placement.resolve(layout.bounds());

        self.content.as_widget_mut().overlay(
            tree,
            layout,
            renderer,
            &(*viewport * transformation.inverse()),
            translation + transformation.translation(),
        )
    }
}

impl<'a, Message, Theme, Renderer> From<Transform<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced_core::Renderer + 'a,
{
    fn from(transformed: Transform<'a, Message, Theme, Renderer>) -> Self {
        Self::new(transformed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(millis: u64) -> Instant {
        Instant::now() + Duration::from_millis(millis)
    }

    #[test]
    fn a_curve_maps_the_unit_interval_onto_itself() {
        for curve in [
            Curve::linear(Duration::from_millis(100)),
            Curve::out(Duration::from_millis(100)),
            Curve::in_out(Duration::from_millis(100)),
        ] {
            assert_eq!(curve.shape(0.0), 0.0);
            assert_eq!(curve.shape(1.0), 1.0);
            assert!(curve.shape(0.5) > 0.0 && curve.shape(0.5) < 1.0);
            assert_eq!(curve.shape(-1.0), 0.0);
            assert_eq!(curve.shape(2.0), 1.0);
        }
    }

    #[test]
    fn the_named_curves_are_iced_easings() {
        let hundred = Duration::from_millis(100);

        assert_eq!(Curve::linear(hundred).easing(), Easing::Linear);
        assert_eq!(Curve::out(hundred).easing(), Easing::EaseOutCubic);
        assert!(matches!(
            Curve::within(hundred, shapes::smootherstep).easing(),
            Easing::Custom(_)
        ));
        assert!(matches!(Curve::in_out(hundred).easing(), Easing::Custom(_)));

        assert_eq!(Curve::eased(hundred, Easing::EaseOutBack).shape(0.0), 0.0);
    }

    #[test]
    fn an_overshooting_easing_goes_past_the_target() {
        let mut track = Track::new(0.0).curve(Curve::eased(
            Duration::from_millis(200),
            Easing::EaseOutBack,
        ));
        let start = at(0);

        track.to(1.0);
        track.tick(start);
        track.tick(start + Duration::from_millis(100));

        assert!(track.value() > 1.0, "got {}", track.value());

        track.tick(start + Duration::from_millis(200));
        assert_eq!(track.value(), 1.0);
        assert!(!track.is_moving());
    }

    #[test]
    fn a_new_track_is_not_moving() {
        let track = Track::new(3.0);

        assert_eq!(track.value(), 3.0);
        assert!(!track.is_moving());
    }

    #[test]
    fn declaring_a_target_does_not_start_it() {
        let mut track = Track::new(0.0);

        track.to(1.0);

        assert!(track.is_moving());
        assert_eq!(track.value(), 0.0);
    }

    #[test]
    fn the_first_tick_starts_the_motion() {
        let mut track = Track::new(0.0).curve(Curve::linear(Duration::from_millis(200)));
        let start = at(0);

        track.to(1.0);
        track.tick(start);

        assert_eq!(track.value(), 0.0);
        assert!(track.is_moving());

        track.tick(start + Duration::from_millis(100));
        assert!((track.value() - 0.5).abs() < 1e-4);

        track.tick(start + Duration::from_millis(200));
        assert_eq!(track.value(), 1.0);
        assert!(!track.is_moving());
    }

    #[test]
    fn a_value_between_ticks_is_stable() {
        let mut track = Track::new(0.0).curve(Curve::linear(Duration::from_millis(200)));
        let start = at(0);

        track.to(1.0);
        track.tick(start);
        track.tick(start + Duration::from_millis(100));

        let sampled = track.value();

        assert!(track.at(start + Duration::from_millis(300)) == 1.0);
        assert_eq!(track.value(), sampled);
    }

    #[test]
    fn a_second_target_continues_from_where_the_value_is() {
        let mut track = Track::new(0.0).curve(Curve::linear(Duration::from_millis(200)));
        let start = at(0);

        track.to(1.0);
        track.tick(start);
        track.tick(start + Duration::from_millis(100));

        let interrupted = track.value();
        assert!((interrupted - 0.5).abs() < 1e-4);

        track.to(0.0);
        track.tick(start + Duration::from_millis(100));

        assert!((track.value() - interrupted).abs() < 1e-4);

        track.tick(start + Duration::from_millis(300));
        assert_eq!(track.value(), 0.0);
    }

    #[test]
    fn repeating_the_same_target_does_not_stall_the_motion() {
        let mut track = Track::new(0.0).curve(Curve::linear(Duration::from_millis(200)));
        let start = at(0);

        track.to(1.0);

        for step in 0..=4 {
            track.to(1.0);
            track.tick(start + Duration::from_millis(step * 50));
        }

        assert!((track.value() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn translating_does_not_change_the_size() {
        use iced_core::widget::Widget as _;

        let content: Element<'_, (), iced_widget::Theme, iced_widget::Renderer> =
            iced_widget::container(iced_widget::text("x")).into();

        let laid_out = content.as_widget().size();
        let moved = translate(content, Vector::new(120.0, 0.0));

        assert_eq!(moved.size(), laid_out);
    }

    #[test]
    fn scaling_does_not_change_the_size() {
        use iced_core::widget::Widget as _;

        let content: Element<'_, (), iced_widget::Theme, iced_widget::Renderer> =
            iced_widget::container(iced_widget::text("x")).into();

        let laid_out = content.as_widget().size();
        let grown = scale(content, 1.5);

        assert_eq!(grown.size(), laid_out);
    }

    #[test]
    fn an_anchor_is_a_point_of_the_contents_own_box() {
        let bounds = Rectangle::new(Point::new(10.0, 20.0), Size::new(100.0, 50.0));

        assert_eq!(Anchor::TopLeft.of(bounds), Point::new(10.0, 20.0));
        assert_eq!(Anchor::TopRight.of(bounds), Point::new(110.0, 20.0));
        assert_eq!(Anchor::BottomLeft.of(bounds), Point::new(10.0, 70.0));
        assert_eq!(Anchor::BottomRight.of(bounds), Point::new(110.0, 70.0));
        assert_eq!(Anchor::Center.of(bounds), Point::new(60.0, 45.0));
    }

    #[test]
    fn scaling_keeps_its_pivot_fixed_and_moves_the_rest() {
        let bounds = Rectangle::new(Point::new(0.0, 0.0), Size::new(100.0, 100.0));
        let doubling = Placement::Scale {
            factor: 2.0,
            about: Anchor::Center,
        };

        let transformation = doubling.resolve(bounds);

        assert_eq!(bounds.center() * transformation, bounds.center());
        assert_eq!(
            Point::new(0.0, 0.0) * transformation,
            Point::new(-50.0, -50.0)
        );
    }

    #[test]
    fn the_cursor_maps_back_into_the_untransformed_content() {
        let bounds = Rectangle::new(Point::new(10.0, 20.0), Size::new(100.0, 50.0));
        let transformation = Placement::Scale {
            factor: 2.0,
            about: Anchor::Center,
        }
        .resolve(bounds);

        let content_point = Point::new(30.0, 25.0);
        let on_screen = content_point * transformation;

        let cursor = into_content(mouse::Cursor::Available(on_screen), transformation);

        assert_eq!(cursor.position(), Some(content_point));
    }

    #[test]
    fn clipping_is_off_until_it_is_asked_for() {
        fn content<'a>() -> Element<'a, (), iced_widget::Theme, iced_widget::Renderer> {
            iced_widget::text("x").into()
        }

        assert!(!translate(content(), Vector::ZERO).clip);
        assert!(!scale(content(), 1.2).clip);
        assert!(scale(content(), 1.2).clip(true).clip);
    }

    #[test]
    fn animate_is_only_a_read() {
        let track = Track::new(0.0);

        assert_eq!(animate(&track, |value| value * 2.0), 0.0);
    }

    #[test]
    fn a_zero_duration_curve_lands_immediately() {
        let mut track = Track::new(0.0).curve(Curve::linear(Duration::ZERO));

        track.to(1.0);
        track.tick(at(0));

        assert_eq!(track.value(), 1.0);
        assert!(!track.is_moving());
    }
}
