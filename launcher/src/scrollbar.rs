//! Pointer-operated scrollbars for the launcher's vertical scroll areas.
use crate::theme::Theme;
use gpui::{
    canvas, div, fill, point, prelude::*, px, size, Bounds, Div, HitboxBehavior, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ScrollEvent, ScrollHandle, Stateful,
};
use std::{cell::Cell, rc::Rc};

/// Space reserved beside scroll content for the track and its padding.
pub const GUTTER: f32 = 16.;

/// Smallest move, in pixels, that counts as the list having moved.
const MOVED: f32 = 0.01;

/// Whether a wheel step moved a list that scrolls in `max` pixels, from its vertical offset
/// `before` the step and `after` it. gpui's own handler adds the step to the offset without
/// clamping (the next layout does), so both offsets are clamped here to where the list is
/// drawn: a step that only pushes past an end moves nothing.
fn list_moved(before: f32, after: f32, max: f32) -> bool {
    let end = -max.max(0.);
    (after.clamp(end, 0.) - before.clamp(end, 0.)).abs() > MOVED
}

/// A list inside a page that scrolls too: the list takes the wheel while it can still move
/// that way (so the page does not move with it, #11853) and hands it to the page when its
/// content fits or it is at the end the wheel pushes towards (#12650).
pub(crate) trait TakeWheel {
    /// `list` is the list's own scroll, its handle set with `track_scroll`.
    fn take_wheel(self, list: &Scrollbar) -> Self;
}

impl TakeWheel for Stateful<Div> {
    fn take_wheel(self, list: &Scrollbar) -> Self {
        // In the bubble phase gpui's own handler has run before ours (it registers after the
        // listeners, and bubbling goes backwards), so the offset is already the new one. The
        // old one is read in the capture phase, before any handler, by a probe that paints
        // inside the list. The cell lives with the list's scroll, not with this render: a list
        // drawn from its layer keeps the probe of an earlier frame but gets this frame's
        // listener, and both must reach the same cell.
        let before = list.wheel_before.clone();
        let probe = canvas(|_, _, _| (), {
            let (before, handle) = (before.clone(), list.handle.clone());
            move |_, _, window, _| {
                window.on_mouse_event(move |_: &ScrollEvent, phase, _, _| {
                    if phase.capture() {
                        before.set(f32::from(handle.offset().y));
                    }
                });
            }
        })
        .absolute()
        .size(px(0.));
        let handle = list.handle.clone();
        self.on_scroll(move |_, _, cx| {
            let after = f32::from(handle.offset().y);
            if list_moved(before.get(), after, handle.max_offset().y.into()) {
                cx.stop_propagation();
            }
        })
        .child(probe)
    }
}

#[derive(Clone, Default)]
pub(crate) struct Scrollbar {
    pub handle: ScrollHandle,
    /// Distance from the top of the thumb to the point grabbed by the pointer.
    grabbed: Rc<Cell<Option<f32>>>,
    /// The vertical offset the last wheel event found, before any handler moved it (`take_wheel`).
    wheel_before: Rc<Cell<f32>>,
}

/// Geometry in logical pixels. A minimum thumb size keeps long lists easy to grab.
#[derive(Clone, Copy, Debug)]
struct Geometry {
    thumb: f32,
    travel: f32,
    top: f32,
    max: f32,
}

impl Geometry {
    fn new(height: f32, viewport: f32, max: f32, offset: f32) -> Option<Self> {
        if height <= 0. || viewport <= 0. || max <= 0. {
            return None;
        }
        let thumb = (height * viewport / (viewport + max)).max(32_f32.min(height / 2.));
        let travel = height - thumb;
        Some(Self {
            thumb,
            travel,
            top: (-offset / max).clamp(0., 1.) * travel,
            max,
        })
    }

    fn offset(self, thumb_top: f32) -> f32 {
        if self.travel <= 0. {
            return 0.;
        }
        -(thumb_top / self.travel).clamp(0., 1.) * self.max
    }
}

impl Scrollbar {
    /// Wrap the scroll container, reserving a narrow gutter so the bar never covers controls.
    pub fn wrap(&self, content: Stateful<Div>, theme: &Theme) -> Div {
        let state = self.clone();
        // Dark wears the glass look's thin bar: white at 14 % on no track (`scrollbar-color`).
        let (track_color, thumb_color) = if theme.glass {
            (gpui::transparent_black(), crate::glass_ui::white(0.14))
        } else {
            (theme.well, theme.muted)
        };
        div()
            .relative()
            .flex()
            .flex_col()
            .min_h_0()
            .min_w_0()
            .child(
                content
                    .track_scroll(&self.handle)
                    // The glass look's bar lies over the page's own margin, as an overlay
                    // scrollbar does; the old look keeps a gutter for its track.
                    .mr(px(if theme.glass { 0. } else { GUTTER }))
                    // Nested scroll areas must not also move the page behind them, but at
                    // their ends the wheel goes on to it.
                    .take_wheel(self),
            )
            .child(
                canvas(
                    move |bounds, window, _| {
                        let geometry = Geometry::new(
                            bounds.size.height.into(),
                            state.handle.bounds().size.height.into(),
                            state.handle.max_offset().y.into(),
                            state.handle.offset().y.into(),
                        );
                        let hitbox =
                            geometry.map(|_| window.insert_hitbox(bounds, HitboxBehavior::Normal));
                        (state, geometry, hitbox)
                    },
                    move |bounds, (state, geometry, hitbox), window, _| {
                        let Some(g) = geometry else {
                            state.grabbed.set(None);
                            return;
                        };
                        let hitbox = hitbox.unwrap();
                        window.paint_quad(fill(bounds, track_color));
                        window.paint_quad(fill(
                            Bounds::new(
                                point(bounds.left() + px(3.), bounds.top() + px(g.top)),
                                size(bounds.size.width - px(6.), px(g.thumb)),
                            ),
                            thumb_color,
                        ));
                        window.on_mouse_event({
                            let state = state.clone();
                            move |ev: &MouseDownEvent, phase, window, cx| {
                                if !phase.bubble()
                                    || ev.button != MouseButton::Left
                                    || !hitbox.is_hovered(window)
                                {
                                    return;
                                }
                                let y = f32::from(ev.position.y - bounds.top());
                                let grab = if (g.top..=g.top + g.thumb).contains(&y) {
                                    y - g.top
                                } else {
                                    // Clicking the track centers the thumb at the pointer.
                                    let grab = g.thumb / 2.;
                                    let mut offset = state.handle.offset();
                                    offset.y = px(g.offset(y - grab));
                                    state.handle.set_offset(offset);
                                    grab
                                };
                                state.grabbed.set(Some(grab));
                                cx.stop_propagation();
                                window.refresh();
                            }
                        });
                        window.on_mouse_event({
                            let state = state.clone();
                            move |ev: &MouseMoveEvent, phase, window, cx| {
                                if !phase.capture() {
                                    return;
                                }
                                let Some(grab) = state.grabbed.get() else {
                                    return;
                                };
                                if ev.pressed_button != Some(MouseButton::Left) {
                                    state.grabbed.set(None);
                                    return;
                                }
                                let mut offset = state.handle.offset();
                                offset.y =
                                    px(g.offset(f32::from(ev.position.y - bounds.top()) - grab));
                                state.handle.set_offset(offset);
                                cx.stop_propagation();
                                window.refresh();
                            }
                        });
                        window.on_mouse_event(move |ev: &MouseUpEvent, phase, window, cx| {
                            if phase.capture()
                                && ev.button == MouseButton::Left
                                && state.grabbed.take().is_some()
                            {
                                cx.stop_propagation();
                                window.refresh();
                            }
                        });
                    },
                )
                .absolute()
                .right(px(2.))
                .top_0()
                .bottom_0()
                .w(px(12.)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{list_moved, Geometry};

    #[test]
    fn a_step_that_moves_the_list_keeps_the_wheel() {
        // 600 px to scroll: mid-list down and up, and a step that stops 3 px short of the top.
        assert!(list_moved(-260., -300., 600.));
        assert!(list_moved(-300., -280., 600.));
        assert!(list_moved(-3., 17., 600.));
        // 10 px above the bottom, a step that overshoots it still moved the list 10 px.
        assert!(list_moved(-590., -640., 600.));
    }

    #[test]
    fn a_step_at_an_end_or_without_overflow_goes_to_the_page() {
        // Up at the top, down at the bottom, whatever the step's size.
        assert!(!list_moved(0., 21., 600.));
        assert!(!list_moved(-600., -626., 600.));
        // Another step before the layout clamped the first: still at the end.
        assert!(!list_moved(21., 42., 600.));
        assert!(!list_moved(-626., -652., 600.));
        // Back from an overshoot at the bottom lands on the end it is drawn at.
        assert!(!list_moved(-626., -610., 600.));
        // Nothing to scroll.
        assert!(!list_moved(0., -30., 0.));
        assert!(!list_moved(0., 20., 0.));
        assert!(!list_moved(-300., -300., 600.));
    }

    #[test]
    fn the_other_direction_at_an_end_moves_the_list() {
        assert!(list_moved(-600., -570., 600.));
        assert!(list_moved(0., -30., 600.));
        // After an overshoot, a step back that reaches into the range moves it.
        assert!(list_moved(-626., -590., 600.));
    }

    #[test]
    fn hides_without_overflow() {
        assert!(Geometry::new(200., 200., 0., 0.).is_none());
        assert!(Geometry::new(0., 200., 100., 0.).is_none());
    }

    #[test]
    fn thumb_tracks_wheel_offsets_and_pointer_reaches_both_ends() {
        let g = Geometry::new(200., 200., 600., -300.).unwrap();
        assert_eq!(g.thumb, 50.);
        assert_eq!(g.top, 75.);
        assert_eq!(g.offset(g.top), -300.);
        assert_eq!(g.offset(-100.), 0.);
        assert_eq!(g.offset(1000.), -600.);
    }

    #[test]
    fn long_lists_and_small_viewports_keep_geometry_bounded() {
        let g = Geometry::new(200., 200., 20000., -20000.).unwrap();
        assert_eq!(g.thumb, 32.);
        assert_eq!(g.top, g.travel);
        assert_eq!(g.offset(g.travel), -20000.);
        let tiny = Geometry::new(20., 20., 1000., -500.).unwrap();
        assert_eq!(tiny.thumb, 10.);
        assert_eq!(tiny.offset(10.), -1000.);
    }
}
