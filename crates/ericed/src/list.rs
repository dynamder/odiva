//! A window into a list: only the slots a viewport can see.
//!
//! iced hands a `scrollable`'s [`Viewport`] over as a message and nothing that
//! turns it into *which slots to build*; this module is that gap.
//!
//! ```text
//! scrollable.content_bounds().height() == slots.total()
//! ```
//!
//! If the two disagree, every absolute offset is clamped against the wrong
//! number and the list drifts one row per notch. The invariant is a property of
//! the types: a [`Slice`] can only come from a [`Slots`], and its padding can
//! only be produced by [`Slice::surround`], so both spacers come from the same
//! arithmetic as the range they surround.
//!
//! The application has to keep the offset itself ([`Scroll`]): `scrollable`
//! reports it only through `on_scroll`, and `scroll_to` does not fire
//! `on_scroll`, which is why [`Scroll::to`] records the offset and returns the
//! payload together.

use std::ops::Range;

use iced_core::{Element, Length, Pixels, Size};
use iced_widget::Column;
use iced_widget::scrollable::{AbsoluteOffset, Viewport};
use iced_widget::space;

/// The stored counterpart of a [`Viewport`]: the same two numbers, kept as state
/// because `view` needs them now and the message arrives later.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Scroll {
    y: f32,
    height: f32,
}

impl Scroll {
    /// A scroll that has not been told anything yet: top, no height.
    pub const fn new() -> Self {
        Self {
            y: 0.0,
            height: 0.0,
        }
    }

    /// A scroll at a known position, for tests and for restoring state.
    pub const fn at(y: f32, height: f32) -> Self {
        Self { y, height }
    }

    /// Where the viewport sits, in pixels from the content's top.
    pub fn y(&self) -> f32 {
        self.y
    }

    /// How tall the viewport is, in pixels.
    pub fn height(&self) -> f32 {
        self.height
    }

    /// The payload of `on_scroll`.
    pub fn update(&mut self, viewport: &Viewport) {
        self.y = viewport.absolute_offset().y;
        self.height = viewport.bounds().height;
    }

    /// The height of the viewport, without touching the offset.
    ///
    /// For a `sensor` that reports its own size: the height is known from the
    /// first frame, the offset is not.
    pub fn resized(&mut self, height: f32) {
        self.height = height.max(0.0);
    }

    /// Records a programmatic scroll and returns the payload to hand to
    /// `iced::widget::operation::scroll_to`.
    ///
    /// Call it in the same `update` that starts the task: `scroll_to` does not
    /// run `on_scroll`, so doing the two separately leaves the list drawing one
    /// window while the scrollbar shows another.
    pub fn to(&mut self, y: f32) -> AbsoluteOffset<Option<f32>> {
        self.y = y.max(0.0);

        AbsoluteOffset {
            x: None,
            y: Some(self.y),
        }
    }
}

/// The slots a viewport can see, plus the padding that keeps the scrollbar
/// honest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slice {
    start: usize,
    end: usize,
    before: f32,
    after: f32,
    total: f32,
}

impl Slice {
    /// Which slots to build, in order.
    pub fn range(&self) -> Range<usize> {
        self.start..self.end
    }

    /// How many slots to build.
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Whether the viewport shows nothing at all.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The height that belongs above the built slots.
    pub fn before(&self) -> f32 {
        self.before
    }

    /// The height that belongs below them.
    pub fn after(&self) -> f32 {
        self.after
    }

    /// The height the scrollable's content must have.
    pub fn total(&self) -> f32 {
        self.total
    }

    /// Wraps the built slots in exactly the padding this slice needs.
    ///
    /// The spacing is zero on purpose: a `Column` adds its spacing between every
    /// pair, which would grow the content by `2 * spacing` on every rebuild.
    ///
    /// The height is pinned to the slice's total, and that is what keeps the
    /// invariant true while heights are still being discovered. Without it the
    /// content's real height is "padding plus whatever the built rows laid out
    /// to", which equals the table's total only once every one of them has been
    /// measured. A gap of a few pixels is enough for the scrollable to clamp an
    /// offset against one number while the list was built from the other: a row
    /// enters, the content shrinks, the clamp moves the view, another row
    /// enters — and at the bottom edge of a row that is exactly the flicker.
    pub fn surround<'a, Message, Theme, Renderer>(
        &self,
        content: impl Into<Element<'a, Message, Theme, Renderer>>,
    ) -> Element<'a, Message, Theme, Renderer>
    where
        Message: 'a,
        Theme: 'a,
        Renderer: iced_core::Renderer + 'a,
    {
        Column::with_children(Vec::from([
            space().height(self.before).into(),
            content.into(),
            space().height(self.after).into(),
        ]))
        .spacing(0.0)
        .height(Length::Fixed(self.total))
        .into()
    }
}

/// A list whose slots can be indexed, measured and placed.
///
/// [`Fixed`] knows every height up front, [`Measured`] learns them; everything
/// here is written in terms of `top`/`height`, so both share it.
pub trait Slots {
    /// How many slots there are.
    fn len(&self) -> usize;

    /// Whether there are none.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The height of everything, which the scrollable's content must match.
    fn total(&self) -> f32;

    /// Where a slot starts, in pixels from the content's top.
    fn top(&self, slot: usize) -> f32;

    /// How tall a slot is.
    fn height(&self, slot: usize) -> f32;

    /// The slots visible in a viewport at `y` with the given `height`.
    fn slice_at(&self, y: f32, height: f32) -> Slice;

    /// The same, from the numbers the app keeps.
    fn slice(&self, scroll: &Scroll) -> Slice {
        self.slice_at(scroll.y(), scroll.height())
    }

    /// The largest offset that still shows content, never negative.
    fn max_scroll(&self, height: f32) -> f32 {
        (self.total() - height).max(0.0)
    }

    /// The offset that moves a slot into view with the least scrolling.
    ///
    /// Returns the current offset when the slot is already visible, and when the
    /// viewport height is still unknown — guessing there would jump the list to
    /// its end on the first frame.
    fn reveal(&self, slot: usize, scroll: &Scroll) -> f32 {
        if scroll.height() <= 0.0 || self.is_empty() {
            return scroll.y();
        }

        let slot = slot.min(self.len() - 1);
        let top = self.top(slot);
        let bottom = top + self.height(slot);

        let view_top = scroll.y();
        let view_bottom = view_top + scroll.height();

        let target = if top < view_top {
            top
        } else if bottom > view_bottom {
            bottom - scroll.height()
        } else {
            view_top
        };

        target.clamp(0.0, self.max_scroll(scroll.height()))
    }

    /// The offset that puts `slot`'s top `dy` pixels below the viewport's top.
    ///
    /// For keeping a reader's place when content is inserted above them: record
    /// `(slot, dy)` before the change, and ask for the same pair afterwards with
    /// `slot` shifted by however many slots appeared before it.
    fn anchor(&self, slot: usize, dy: f32, scroll: &Scroll) -> f32 {
        if self.is_empty() {
            return 0.0;
        }

        let target = self.top(slot.min(self.len() - 1)) - dy;

        target.clamp(0.0, self.max_scroll(scroll.height()))
    }
}

fn clamp_offset(y: f32, height: f32, max_scroll: f32) -> f32 {
    if height <= 0.0 {
        y.max(0.0)
    } else {
        y.clamp(0.0, max_scroll)
    }
}

fn empty_slice(total: f32) -> Slice {
    Slice {
        start: 0,
        end: 0,
        before: 0.0,
        after: total,
        total,
    }
}

/// A list of slots that all have the same height. Build one with [`fixed`].
///
/// No measurement, so no feedback loop and no drift: the scrollbar, the reveal
/// arithmetic and the content height are exact from the first frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fixed {
    row: f32,
    len: usize,
    overscan: usize,
}

/// A list of `len` slots, each `row` pixels tall.
///
/// `row` is also the `Length::Fixed` the row itself must use; the two are the
/// invariant, so both should come from one function.
pub fn fixed(row: impl Into<Pixels>, len: usize) -> Fixed {
    Fixed {
        row: row.into().0.max(0.0),
        len,
        overscan: 0,
    }
}

impl Fixed {
    /// Slots to build beyond the visible ones, on both sides.
    ///
    /// Not an optimisation: a viewport rarely starts on a slot boundary, so the
    /// partially visible slots at either end have to be in the tree.
    pub fn overscan(self, slots: usize) -> Self {
        Self {
            overscan: slots,
            ..self
        }
    }

    /// How many slots fit in a viewport of this height.
    pub fn visible(&self, height: f32) -> usize {
        if self.row <= 0.0 {
            return 0;
        }

        (height / self.row).ceil() as usize + 1
    }
}

impl Slots for Fixed {
    fn len(&self) -> usize {
        self.len
    }

    fn total(&self) -> f32 {
        self.row * self.len as f32
    }

    fn top(&self, slot: usize) -> f32 {
        self.row * slot as f32
    }

    fn height(&self, _slot: usize) -> f32 {
        self.row
    }

    fn slice_at(&self, y: f32, height: f32) -> Slice {
        let total = self.total();

        if self.len == 0 || self.row <= 0.0 {
            return empty_slice(total);
        }

        let y = clamp_offset(y, height, self.max_scroll(height));
        let first = ((y / self.row).floor() as usize).min(self.len - 1);

        let start = first.saturating_sub(self.overscan);
        let end = (first + self.visible(height) + self.overscan).min(self.len);

        Slice {
            start,
            end,
            before: self.top(start),
            after: total - self.top(end),
            total,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SlotHeight {
    Estimated,
    Measured(f32),
}

/// A list of slots whose heights are discovered by laying them out. Build one
/// with [`measured`].
///
/// Every slot starts at `estimate`; [`Measured::measure`] corrects one from a
/// `sensor`, so the heights are a cache rather than a fact. Three rules keep the
/// cache from fighting the layout:
///
/// 1. Measuring never changes a slot that is already measured, so the table
///    converges instead of oscillating between frames.
/// 2. The window is positioned by the prefix sum as it stands. Unmeasured slots
///    are estimates, so the scrollbar is approximate there.
/// 3. A measurement is only valid at one width. [`Measured::measure`] takes the
///    whole [`Size`] and invalidates the table when the width moves, because a
///    re-wrap changes every height.
#[derive(Debug, Clone, PartialEq)]
pub struct Measured {
    len: usize,
    overscan: usize,
    estimate: f32,
    heights: Vec<SlotHeight>,
    width: Option<f32>,
    prefix: Vec<f32>,
}

/// A list of `len` slots, each assumed to be `estimate` pixels tall until a
/// measurement says otherwise.
pub fn measured(estimate: impl Into<Pixels>, len: usize) -> Measured {
    let estimate = estimate.into().0.max(0.0);

    Measured {
        len,
        overscan: 0,
        estimate,
        heights: vec![SlotHeight::Estimated; len],
        width: None,
        prefix: (0..=len).map(|i| estimate * i as f32).collect(),
    }
}

impl Measured {
    /// Slots to build beyond the visible ones. See [`Fixed::overscan`].
    pub fn overscan(mut self, slots: usize) -> Self {
        self.overscan = slots;
        self
    }

    /// What a slot claims to be: measured if known, estimated if not.
    pub fn height_of(&self, slot: usize) -> f32 {
        match self.heights.get(slot) {
            Some(SlotHeight::Measured(height)) => *height,
            _ => self.estimate,
        }
    }

    /// Whether a slot has been measured at the current width.
    pub fn is_measured(&self, slot: usize) -> bool {
        matches!(self.heights.get(slot), Some(SlotHeight::Measured(_)))
    }

    /// The width every measurement so far was taken at.
    pub fn width(&self) -> Option<f32> {
        self.width
    }

    /// Records a slot's real size, as reported by a `sensor`.
    ///
    /// Returns whether the content height changed — the app needs that to know
    /// whether the anchor it is holding still means anything.
    pub fn measure(&mut self, slot: usize, size: Size) -> bool {
        if slot >= self.len {
            return false;
        }

        let reflowed = self
            .width
            .is_some_and(|known| (known - size.width).abs() > 1.0);

        if reflowed {
            self.heights.fill(SlotHeight::Estimated);
        }

        self.width = Some(size.width);

        let total_before = self.total();

        let height = size.height.max(0.0);

        if !reflowed && (self.height_of(slot) - height).abs() < 0.5 {
            return false;
        }

        self.heights[slot] = SlotHeight::Measured(height);
        self.rebuild_prefix(if reflowed { 0 } else { slot });

        self.total() != total_before
    }

    /// Appends a slot at the estimate. `measure` corrects it once it lays out.
    pub fn push(&mut self) {
        let end = self.prefix[self.len] + self.estimate;

        self.len += 1;
        self.heights.push(SlotHeight::Estimated);
        self.prefix.push(end);
    }

    /// Inserts `slots` unmeasured slots at the front, for loading older items
    /// above what is shown. The app shifts any anchor it holds by the same count
    /// and then asks [`Slots::anchor`] for the offset that keeps the reader in
    /// place.
    ///
    /// A block grid only survives this if the number of inserted slots is a
    /// multiple of the block size — otherwise every block boundary moves.
    pub fn prepend(&mut self, slots: usize) {
        if slots == 0 {
            return;
        }

        self.len += slots;
        self.heights = core::iter::repeat_n(SlotHeight::Estimated, slots)
            .chain(self.heights.drain(..))
            .collect();

        self.prefix.clear();
        self.prefix.push(0.0);

        for i in 0..self.len {
            let next = self.prefix[i] + self.height_of(i);
            self.prefix.push(next);
        }
    }

    fn rebuild_prefix(&mut self, from: usize) {
        for i in from.max(1)..=self.len {
            self.prefix[i] = self.prefix[i - 1] + self.height_of(i - 1);
        }
    }

    /// The slot that owns the pixel at `y`.
    fn slot_at(&self, y: f32) -> usize {
        if self.len == 0 {
            return 0;
        }

        let after = self.prefix.partition_point(|&edge| edge <= y.max(0.0));

        after.saturating_sub(1).min(self.len - 1)
    }
}

impl Slots for Measured {
    fn len(&self) -> usize {
        self.len
    }

    fn total(&self) -> f32 {
        self.prefix[self.len]
    }

    fn top(&self, slot: usize) -> f32 {
        self.prefix[slot.min(self.len)]
    }

    fn height(&self, slot: usize) -> f32 {
        self.height_of(slot)
    }

    fn slice_at(&self, y: f32, height: f32) -> Slice {
        let total = self.total();

        if self.len == 0 {
            return empty_slice(total);
        }

        let y = clamp_offset(y, height, self.max_scroll(height));
        let first = self.slot_at(y);
        let last = self.slot_at(y + height.max(0.0));

        let start = first.saturating_sub(self.overscan);
        let end = (last + 1 + self.overscan).min(self.len);

        Slice {
            start,
            end,
            before: self.top(start),
            after: total - self.top(end),
            total,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROW: f32 = 24.0;

    #[test]
    fn scroll_records_what_the_writer_and_the_view_agree_on() {
        let mut scroll = Scroll::new();
        assert_eq!(scroll.y(), 0.0);
        assert_eq!(scroll.height(), 0.0);

        let payload = scroll.to(120.0);

        assert_eq!(scroll.y(), 120.0);
        assert_eq!(payload.y, Some(120.0));
        assert_eq!(payload.x, None);
    }

    #[test]
    fn resizing_keeps_the_offset_and_on_scroll_keeps_both() {
        let mut scroll = Scroll::at(240.0, 480.0);

        scroll.resized(600.0);

        assert_eq!(scroll.y(), 240.0);
        assert_eq!(scroll.height(), 600.0);
    }

    #[test]
    fn fixed_slices_the_viewport() {
        let path = fixed(ROW, 100);
        let slice = path.slice_at(240.0, 480.0);

        assert_eq!(slice.range(), 10..31);
        assert_eq!(slice.before(), 240.0);
        assert_eq!(slice.total(), 2400.0);
        assert_eq!(slice.before() + slice.after() + 21.0 * ROW, 2400.0);
    }

    #[test]
    fn overscan_widens_both_sides_without_moving_the_invariant() {
        let path = fixed(ROW, 100).overscan(2);
        let slice = path.slice_at(240.0, 480.0);

        assert_eq!(slice.range(), 8..33);
        assert_eq!(slice.before(), 8.0 * ROW);
        assert_eq!(slice.total(), 2400.0);
        assert_eq!(slice.before() + slice.after() + 25.0 * ROW, 2400.0);
    }

    #[test]
    fn fixed_clamps_at_the_end() {
        let path = fixed(ROW, 100);
        let slice = path.slice_at(100_000.0, 480.0);

        assert_eq!(slice.range().end, 100);
        assert_eq!(slice.after(), 0.0);
        assert_eq!(
            slice.before() + slice.after() + slice.len() as f32 * ROW,
            path.total()
        );
    }

    #[test]
    fn an_offset_past_the_end_is_clamped_rather_than_blank() {
        let path = fixed(ROW, 10);

        let slice = path.slice_at(10_000.0, 480.0);

        assert!(!slice.is_empty());
        assert_eq!(slice.range().end, 10);
    }

    #[test]
    fn an_empty_list_still_reports_its_total() {
        let path = fixed(ROW, 0);
        let slice = path.slice_at(0.0, 480.0);

        assert!(slice.is_empty());
        assert_eq!(slice.total(), 0.0);
        assert_eq!(slice.before(), 0.0);
        assert_eq!(slice.after(), 0.0);
    }

    #[test]
    fn reveal_moves_the_minimum() {
        let path = fixed(ROW, 100);
        let scroll = Scroll::at(240.0, 480.0);

        assert_eq!(path.reveal(15, &scroll), 240.0);
        assert_eq!(path.reveal(3, &scroll), 72.0);
        assert_eq!(path.reveal(40, &scroll), 41.0 * ROW - 480.0);
    }

    #[test]
    fn reveal_refuses_to_guess_without_a_height() {
        let path = fixed(ROW, 100);
        let scroll = Scroll::at(240.0, 0.0);

        assert_eq!(path.reveal(90, &scroll), 240.0);
    }

    #[test]
    fn anchor_puts_a_slot_at_a_known_depth() {
        let path = fixed(ROW, 100);
        let scroll = Scroll::at(240.0, 480.0);

        assert_eq!(path.anchor(20, 10.0, &scroll), 470.0);
        assert_eq!(path.anchor(0, 10.0, &scroll), 0.0);
        assert_eq!(path.anchor(99, 0.0, &scroll), 2400.0 - 480.0);
    }

    #[test]
    fn measured_starts_at_the_estimate() {
        let path = measured(40.0, 10);

        assert_eq!(path.total(), 400.0);
        assert_eq!(path.top(3), 120.0);
        assert_eq!(path.height_of(3), 40.0);
        assert!(!path.is_measured(3));
        assert_eq!(path.width(), None);
    }

    #[test]
    fn measuring_reports_whether_the_total_moved() {
        let mut path = measured(40.0, 10);

        assert!(path.measure(2, Size::new(600.0, 100.0)));
        assert_eq!(path.height_of(2), 100.0);
        assert_eq!(path.top(3), 2.0 * 40.0 + 100.0);
        assert_eq!(path.total(), 10.0 * 40.0 + 60.0);
        assert_eq!(path.width(), Some(600.0));

        assert!(!path.measure(2, Size::new(600.0, 100.0)));
        assert_eq!(path.total(), 460.0);
    }

    #[test]
    fn a_new_width_invalidates_every_height() {
        let mut path = measured(40.0, 4);
        path.measure(1, Size::new(600.0, 100.0));
        assert_eq!(path.height_of(1), 100.0);

        path.measure(2, Size::new(400.0, 70.0));

        assert_eq!(path.height_of(1), 40.0);
        assert_eq!(path.height_of(2), 70.0);
        assert!(!path.is_measured(1));
        assert_eq!(path.width(), Some(400.0));
    }

    #[test]
    fn measured_finds_the_slot_under_a_pixel() {
        let mut path = measured(40.0, 5);
        path.measure(1, Size::new(600.0, 100.0));

        assert_eq!(path.slot_at(0.0), 0);
        assert_eq!(path.slot_at(39.0), 0);
        assert_eq!(path.slot_at(40.0), 1);
        assert_eq!(path.slot_at(139.0), 1);
        assert_eq!(path.slot_at(140.0), 2);
        assert_eq!(path.slot_at(10_000.0), 4);
    }

    #[test]
    fn measured_slice_and_reveal_agree_with_the_prefix_sum() {
        let mut path = measured(40.0, 20).overscan(1);
        path.measure(5, Size::new(600.0, 80.0));

        let scroll = Scroll::at(path.top(5), 200.0);
        let slice = path.slice(&scroll);

        assert_eq!(slice.total(), path.total());
        assert_eq!(slice.before(), path.top(slice.range().start));
        assert_eq!(slice.after(), path.total() - path.top(slice.range().end));
        assert!(slice.range().contains(&5));
        assert_eq!(path.reveal(5, &scroll), scroll.y());
    }

    #[test]
    fn push_appends_at_the_estimate() {
        let mut path = measured(40.0, 2);
        path.measure(0, Size::new(600.0, 100.0));

        path.push();

        assert_eq!(path.len(), 3);
        assert_eq!(path.total(), 100.0 + 40.0 + 40.0);
        assert_eq!(path.top(2), 140.0);
        assert_eq!(path.height_of(2), 40.0);
        assert!(!path.is_measured(2));
        assert_eq!(path.slice_at(0.0, 80.0).total(), path.total());
    }

    #[test]
    fn prepend_shifts_measurements_and_keeps_the_shapes_valid() {
        let mut path = measured(40.0, 4);
        path.measure(0, Size::new(600.0, 100.0));
        assert_eq!(path.total(), 4.0 * 40.0 + 60.0);

        path.prepend(2);

        assert_eq!(path.len(), 6);
        assert_eq!(path.total(), 6.0 * 40.0 + 60.0);
        assert_eq!(path.top(0), 0.0);
        assert!(!path.is_measured(0));
        assert!(path.is_measured(2));
        assert_eq!(path.height_of(2), 100.0);
        assert_eq!(path.top(3), 2.0 * 40.0 + 100.0);
        assert_eq!(path.slot_at(0.0), 0);
        assert_eq!(path.slice_at(0.0, 80.0).total(), path.total());
    }
}

#[cfg(test)]
mod settling {
    use super::*;

    const ROW: f32 = 24.0;

    #[test]
    fn a_remeasure_below_half_a_pixel_is_not_a_change() {
        let mut slots = measured(ROW, 3);

        assert!(slots.measure(0, Size::new(600.0, ROW * 2.0)));
        assert!(
            !slots.measure(0, Size::new(600.0, ROW * 2.0 + 0.3)),
            "sub-pixel wobble must not rebuild the list"
        );
        assert!(slots.measure(0, Size::new(600.0, ROW * 2.0 + 4.0)));
    }

    #[test]
    fn a_width_that_wobbles_by_a_pixel_does_not_reflow() {
        let mut slots = measured(ROW, 3);

        slots.measure(1, Size::new(600.0, ROW * 2.0));
        assert!(!slots.is_measured(0), "only slot one was measured");

        slots.measure(0, Size::new(600.6, ROW * 2.0));

        assert!(slots.is_measured(0));
        assert!(
            slots.is_measured(1),
            "a hair of width must not throw away every measurement"
        );
    }
}
