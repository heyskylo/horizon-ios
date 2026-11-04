//! A panel that stays in view beside a long list as the page scrolls (CSS `position: sticky;
//! top: 0`, which GPUI has no style for): the Mods page's project card (the mockup's
//! `lg:sticky lg:top-0`).
//!
//! The row holding the list and the panel records where it lies in the page and how tall it and
//! the panel are while it paints; the panel is then drawn moved down (`top`, which takes nothing
//! from the layout) by how far the page has scrolled past the row's top, never past the row's
//! end. GPUI Fast draws a scrolled page from its layer without rendering it again, so the page's
//! scroll asks [`Sticky::moved`] on the next frame whether the panel must be drawn elsewhere.
use gpui::{canvas, prelude::*, ScrollHandle};
use std::cell::Cell;
use std::rc::Rc;

#[derive(Clone, Copy, Default, PartialEq, Debug)]
struct Measured {
    /// The row's top in the page's content (unscrolled), its height, the panel's height.
    top: f32,
    row: f32,
    panel: f32,
}

#[derive(Clone, Default)]
pub struct Sticky {
    measured: Rc<Cell<Option<Measured>>>,
    /// The panel's height, as it last painted.
    panel: Rc<Cell<f32>>,
    /// The offset the panel was last drawn at.
    drawn: Rc<Cell<f32>>,
}

/// How far the panel moves down: what the page has scrolled past the row's top, up to where the
/// panel's foot meets the row's.
fn offset(m: Measured, scrolled: f32) -> f32 {
    (scrolled - m.top)
        .clamp(0., (m.row - m.panel).max(0.))
        .round()
}

impl Sticky {
    /// The panel's offset for the page scrolled as `handle` is now; remembered as drawn.
    pub fn offset(&self, handle: &ScrollHandle) -> f32 {
        let at = self
            .measured
            .get()
            .map_or(0., |m| offset(m, scrolled(handle)));
        self.drawn.set(at);
        at
    }

    /// Whether the panel would now be drawn somewhere else than it was.
    pub fn moved(&self, handle: &ScrollHandle) -> bool {
        self.measured
            .get()
            .is_some_and(|m| offset(m, scrolled(handle)) != self.drawn.get())
    }

    /// Forgets the row (the page left), so a new one starts unmoved.
    pub fn reset(&self) {
        self.measured.set(None);
        self.drawn.set(0.);
    }

    /// Goes in the row (absolute, filling it): records where it lies in the page scrolled by
    /// `handle`, with the panel's height ([`Sticky::panel`]).
    pub fn row(&self, handle: &ScrollHandle) -> impl IntoElement {
        let (measured, panel, handle) = (self.measured.clone(), self.panel.clone(), handle.clone());
        canvas(
            move |bounds, _, _| {
                let top = f32::from(bounds.top() - handle.bounds().top()) + scrolled(&handle);
                measured.set(Some(Measured {
                    top,
                    row: f32::from(bounds.size.height),
                    panel: panel.get(),
                }));
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    /// Goes in the panel (absolute, filling it): records its height.
    pub fn panel(&self) -> impl IntoElement {
        let into = self.panel.clone();
        canvas(
            move |bounds, _, _| into.set(f32::from(bounds.size.height)),
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }
}

fn scrolled(handle: &ScrollHandle) -> f32 {
    f32::from(-handle.offset().y).max(0.)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_follows_the_scroll_between_the_rows_top_and_its_end() {
        // A row 300 px down the page, 2000 tall, beside a 500 px panel.
        let m = Measured {
            top: 300.,
            row: 2000.,
            panel: 500.,
        };
        assert_eq!(offset(m, 0.), 0., "above the row: where the layout put it");
        assert_eq!(offset(m, 300.), 0.);
        assert_eq!(offset(m, 1000.), 700., "its top stays at the page's top");
        assert_eq!(offset(m, 5000.), 1500., "its foot stops at the row's");
        // A panel taller than the list beside it never moves.
        let tall = Measured { panel: 2500., ..m };
        assert_eq!(offset(tall, 1000.), 0.);
    }
}
