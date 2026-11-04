//! The launcher's motion, as the website's launcher plays it (horizon-site's `launcher.css` and
//! Tailwind's defaults): hovers ease over the design's quick curve (150 ms,
//! `cubic-bezier(0.4, 0, 0.2, 1)`); menus, popovers, toasts and a new page arrive as `hzl-pop`
//! does (a fade and a 6 px rise over 200 ms); a switch's knob slides for 200 ms. Under reduced
//! motion ([`crate::sky::motion`]) nothing moves: everything is drawn where it ends.
use gpui::{
    div, px, Animation, AnimationExt, AnyElement, Div, ElementId, IntoElement, ParentElement,
    Styled,
};
use horizon_theme as tokens;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

/// `hzl-pop`: `0.2s cubic-bezier(0.2, 0.7, 0.2, 1)`, from transparent and 6 px lower.
pub const POP: tokens::Motion = tokens::Motion {
    name: "pop",
    ms: 200,
    curve: [0.2, 0.7, 0.2, 1.],
};
pub const POP_RISE: f32 = 6.;

/// A switch's knob (`transition-[translate] duration-200`, Tailwind's default curve).
const SLIDE: tokens::Motion = tokens::Motion {
    name: "slide",
    ms: 200,
    curve: tokens::QUICK.curve,
};

/// Whether the player's system asks for reduced motion (or `HORIZON_LAUNCHER_MOTION=off`).
pub fn still() -> bool {
    crate::sky::motion() == crate::sky::Motion::Still
}

/// `motion` as GPUI plays it: its length on its own curve; at its end from the first frame under
/// reduced motion.
pub fn eased(motion: tokens::Motion) -> Animation {
    let animation = Animation::new(motion.duration());
    if still() {
        return animation.with_easing(|_| 1.);
    }
    animation.with_easing(move |t| motion.ease(t))
}

/// `content` arriving as `hzl-pop` does. `id` names the arrival: an element shown again under the
/// same id after a frame without it plays it again.
pub fn pop(id: impl Into<ElementId>, content: impl IntoElement) -> AnyElement {
    div()
        .child(content)
        .with_animation(id, eased(POP), |d, p| {
            d.opacity(p).mt(px(POP_RISE * (1. - p)))
        })
        .into_any_element()
}

thread_local! {
    /// Each switch's last state, and when it last changed.
    static SWITCHES: RefCell<HashMap<String, (bool, Option<Instant>)>> = RefCell::new(HashMap::new());
}

/// When switch `id`'s knob started sliding towards `on`, while it still slides: a switch seen
/// for the first time, or changed long ago, is already there.
fn slide(id: &str, on: bool, now: Instant) -> Option<Instant> {
    SWITCHES.with_borrow_mut(|switches| {
        let seen = switches.entry(id.to_string()).or_insert((on, None));
        if seen.0 != on {
            *seen = (on, Some(now));
        }
        seen.1
            .filter(|since| now.duration_since(*since) < SLIDE.duration())
    })
}

/// A switch's knob at `left` px when off and `left + travel` when on, sliding there when the
/// switch `id` changed in the last 200 ms.
pub fn knob(id: &str, on: bool, knob: Div, left: f32, travel: f32) -> AnyElement {
    let at = |p: f32| left + travel * if on { p } else { 1. - p };
    match slide(id, on, Instant::now()) {
        Some(since) => {
            // Each slide gets its own animation id, from when it started, so it plays once.
            static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
            let start = *START.get_or_init(Instant::now);
            let key = since.saturating_duration_since(start).as_micros() as usize;
            knob.with_animation(
                ElementId::named_usize(format!("{id}-knob"), key),
                eased(SLIDE),
                move |d, p| d.left(px(at(p))),
            )
            .into_any_element()
        }
        None => knob.left(px(at(1.))).into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_switch_slides_only_after_it_changed() {
        let t0 = Instant::now();
        assert_eq!(slide("test-a", true, t0), None, "first seen: already there");
        assert_eq!(slide("test-a", true, t0 + Duration::from_millis(50)), None);
        let flip = t0 + Duration::from_millis(100);
        assert_eq!(slide("test-a", false, flip), Some(flip));
        assert_eq!(
            slide("test-a", false, flip + Duration::from_millis(150)),
            Some(flip),
            "the same slide while it runs"
        );
        assert_eq!(slide("test-a", false, flip + Duration::from_millis(250)), None);
    }
}
