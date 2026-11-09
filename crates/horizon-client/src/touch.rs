//! Touch controls for phones and tablets (Horizon's own), after Minecraft: Pocket Edition's: a D-pad on the
//! left with sneak in its centre, jump on the right, perspective, chat and pause at the top, and the hotbar
//! and an inventory button along the bottom. A finger anywhere else turns the view; a short tap uses (or
//! attacks the entity under the crosshair), and holding still mines.
//!
//! This module is the layout and the fingers' state: positions in GUI pixels in, [`Out`]s for the game to act
//! on out. game/touch.rs feeds it winit's touches and acts on what comes back.

#[path = "touch/menu.rs"]
mod menu;
pub use menu::{MenuAction, MenuGesture, MenuMode};

/// A button of the layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Forward,
    Back,
    Left,
    Right,
    /// The D-pad's centre: sneaking, toggled.
    Sneak,
    Jump,
    Perspective,
    Chat,
    Pause,
    Inventory,
    Hotbar(u8),
}

impl Control {
    /// The D-pad's directions, which a finger can slide between.
    fn moves(self) -> bool {
        matches!(
            self,
            Control::Forward | Control::Back | Control::Left | Control::Right
        )
    }
}

/// `[x, y, width, height]` in GUI pixels.
pub type Rect = [f32; 4];

/// The D-pad's and jump's button size on a `h` GUI-pixel-high screen: about a seventh of the height, so a
/// phone (GUI scale 4, 292 high) gets 40 GUI pixels, some 53 points, over Apple's 44-point minimum.
pub fn button_size(h: f32) -> f32 {
    (h * 0.14).clamp(26.0, 48.0)
}
/// The gap kept from the screen's edges.
const MARGIN: f32 = 14.0;
/// How far above the hotbar its slots still take a tap: a thumb lands high.
const HOTBAR_REACH: f32 = 8.0;

/// Where every button is on a `w` x `h` GUI-pixel screen.
fn default_layout(w: f32, h: f32) -> Vec<(Control, Rect)> {
    let s = button_size(h);
    let small = (s * 0.65).round();
    // The D-pad's centre cell, high enough that its bottom row clears the screen's edge.
    let (cx, cy) = (MARGIN + s, h - MARGIN - 2.0 * s - 8.0);
    let mut v = vec![
        (Control::Forward, [cx, cy - s, s, s]),
        (Control::Left, [cx - s, cy, s, s]),
        (Control::Sneak, [cx, cy, s, s]),
        (Control::Right, [cx + s, cy, s, s]),
        (Control::Back, [cx, cy + s, s, s]),
        (Control::Jump, [w - MARGIN - s, cy, s, s]),
    ];
    let top = [Control::Perspective, Control::Chat, Control::Pause];
    let x0 = (w - 3.0 * small - 8.0) / 2.0;
    for (i, control) in top.into_iter().enumerate() {
        v.push((control, [x0 + i as f32 * (small + 4.0), 2.0, small, small]));
    }
    // Gui.extractItemHotbar's slots: the bar is 182 wide, centred, 22 high at the bottom.
    let (hx, hy) = ((w / 2.0 - 91.0).floor(), h - 22.0);
    for slot in 0..9u8 {
        v.push((
            Control::Hotbar(slot),
            [
                hx + 1.0 + 20.0 * f32::from(slot),
                hy - HOTBAR_REACH,
                20.0,
                22.0 + HOTBAR_REACH,
            ],
        ));
    }
    v.push((
        Control::Inventory,
        [hx + 184.0, hy - HOTBAR_REACH, 30.0, 22.0 + HOTBAR_REACH],
    ));
    v
}

/// Stable order shared with the app's layout editor; hotbar slots remain on the HUD.
const EDITABLE: [Control; 10] = [
    Control::Forward,
    Control::Back,
    Control::Left,
    Control::Right,
    Control::Sneak,
    Control::Jump,
    Control::Perspective,
    Control::Chat,
    Control::Pause,
    Control::Inventory,
];

/// Apply normalized centres to the same rectangles used by paint and hit tests.
/// Missing/non-finite coordinates use the original position; valid centres are
/// clamped by the actual button size, including after resizing or a GUI scale change.
pub fn layout_with_positions(w: f32, h: f32, positions: &[f32; 20]) -> Vec<(Control, Rect)> {
    let mut buttons = default_layout(w, h);
    for (control, rect) in &mut buttons {
        let Some(i) = EDITABLE.iter().position(|c| c == control) else {
            continue;
        };
        let (x, y) = (positions[i * 2], positions[i * 2 + 1]);
        if x.is_finite() && y.is_finite() {
            rect[0] = (x * w - rect[2] / 2.0).clamp(0.0, (w - rect[2]).max(0.0));
            rect[1] = (y * h - rect[3] / 2.0).clamp(0.0, (h - rect[3]).max(0.0));
        }
    }
    buttons
}

pub fn layout(w: f32, h: f32) -> Vec<(Control, Rect)> {
    #[cfg(target_os = "ios")]
    return layout_with_positions(w, h, &horizon_ios_launcher::touch_preferences().0);
    #[cfg(not(target_os = "ios"))]
    default_layout(w, h)
}

/// Paint can disappear for a clean capture without disabling the touch regions.
pub fn controls_visible() -> bool {
    #[cfg(target_os = "ios")]
    return !horizon_ios_launcher::touch_preferences().1;
    #[cfg(not(target_os = "ios"))]
    true
}

/// The close button over a screen that has none of its own (an inventory, chat, a sign): the top right.
pub fn close_rect(w: f32, h: f32) -> Rect {
    let s = (button_size(h) * 0.75).round();
    [w - s - 4.0, 4.0, s, s]
}

/// The keyboard button, left of the close button: over a focused text field, it asks for the system's
/// keyboard again.
pub fn keyboard_rect(w: f32, h: f32) -> Rect {
    let c = close_rect(w, h);
    [c[0] - c[2] - 4.0, c[1], c[2], c[3]]
}

/// The point is inside the rectangle.
pub fn hit_rect(r: Rect, at: (f32, f32)) -> bool {
    contains(r, at)
}

fn contains(r: Rect, (x, y): (f32, f32)) -> bool {
    x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]
}

/// The button under a point.
pub fn hit(layout: &[(Control, Rect)], at: (f32, f32)) -> Option<Control> {
    // Movable controls paint after the hotbar HUD, with later buttons on top.
    // Match that order even when a user deliberately overlaps two controls.
    layout
        .iter()
        .rev()
        .find(|(c, r)| !matches!(c, Control::Hotbar(_)) && contains(*r, at))
        .or_else(|| layout.iter().find(|(_, r)| contains(*r, at)))
        .map(|(c, _)| *c)
}

/// What the game does with the fingers' changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Out {
    Press(Control),
    Release(Control),
    /// Turn the view by this much, in GUI pixels dragged.
    Look(f32, f32),
    /// A short tap away from the buttons: use, or attack what the crosshair is on.
    Tap,
    /// A finger held still away from the buttons: mining starts, and stops when it lifts.
    MineStart,
    MineEnd,
}

/// How far a finger moves (GUI pixels) before a touch is a drag and not a tap.
const SLOP: f32 = 8.0;
/// How long a finger stays still (seconds) before it mines.
const HOLD: f64 = 0.35;

#[derive(Clone, Debug)]
enum Kind {
    Button(Control),
    World { dragged: bool, mining: bool },
}

#[derive(Clone, Debug)]
struct Finger {
    id: u64,
    kind: Kind,
    start: (f32, f32),
    last: (f32, f32),
    since: f64,
}

/// The fingers on the screen while playing.
#[derive(Clone, Debug, Default)]
pub struct Touches {
    fingers: Vec<Finger>,
}

impl Touches {
    /// A finger went down at `at`, `now` seconds into some clock.
    pub fn start(
        &mut self,
        id: u64,
        at: (f32, f32),
        now: f64,
        layout: &[(Control, Rect)],
    ) -> Vec<Out> {
        let mut out = self.end(id);
        let kind = match hit(layout, at) {
            Some(control) => {
                out.push(Out::Press(control));
                Kind::Button(control)
            }
            None => Kind::World {
                dragged: false,
                mining: false,
            },
        };
        self.fingers.push(Finger {
            id,
            kind,
            start: at,
            last: at,
            since: now,
        });
        out
    }

    /// A finger moved: the D-pad follows it from direction to direction, and a finger on the world turns
    /// the view.
    pub fn moved(&mut self, id: u64, at: (f32, f32), layout: &[(Control, Rect)]) -> Vec<Out> {
        let Some(f) = self.fingers.iter_mut().find(|f| f.id == id) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        match &mut f.kind {
            Kind::Button(control) if control.moves() => {
                if let Some(next) = hit(layout, at).filter(|c| c.moves() && c != control) {
                    out.push(Out::Release(*control));
                    out.push(Out::Press(next));
                    *control = next;
                }
            }
            Kind::Button(_) => {}
            Kind::World { dragged, .. } => {
                let (dx, dy) = (at.0 - f.start.0, at.1 - f.start.1);
                if !*dragged && dx.hypot(dy) > SLOP {
                    *dragged = true;
                }
                if *dragged {
                    out.push(Out::Look(at.0 - f.last.0, at.1 - f.last.1));
                }
            }
        }
        f.last = at;
        out
    }

    /// A finger lifted.
    pub fn end(&mut self, id: u64) -> Vec<Out> {
        let Some(i) = self.fingers.iter().position(|f| f.id == id) else {
            return Vec::new();
        };
        match self.fingers.remove(i).kind {
            Kind::Button(control) => vec![Out::Release(control)],
            Kind::World { mining: true, .. } => vec![Out::MineEnd],
            Kind::World { dragged: false, .. } => vec![Out::Tap],
            Kind::World { .. } => Vec::new(),
        }
    }

    /// UIKit cancelled a finger: release its holds without interpreting it as a tap.
    pub fn cancel(&mut self, id: u64) -> Vec<Out> {
        let mut out = self.end(id);
        out.retain(|o| *o != Out::Tap);
        out
    }

    /// Once a frame: a finger on the world that has stayed still long enough starts mining.
    pub fn frame(&mut self, now: f64) -> Vec<Out> {
        let mut out = Vec::new();
        for f in &mut self.fingers {
            if let Kind::World {
                dragged: false,
                mining,
            } = &mut f.kind
                && !*mining
                && now - f.since >= HOLD
            {
                *mining = true;
                out.push(Out::MineStart);
            }
        }
        out
    }

    /// Every finger lifted at once (a screen opened over the game).
    pub fn release_all(&mut self) -> Vec<Out> {
        let ids: Vec<u64> = self.fingers.iter().map(|f| f.id).collect();
        ids.into_iter().flat_map(|id| self.cancel(id)).collect()
    }

    /// A finger holds this button down (drawn pressed).
    pub fn held(&self, control: Control) -> bool {
        self.fingers
            .iter()
            .any(|f| matches!(f.kind, Kind::Button(c) if c == control))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(layout: &[(Control, Rect)], control: Control) -> (f32, f32) {
        let r = layout.iter().find(|(c, _)| *c == control).unwrap().1;
        (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0)
    }

    #[test]
    fn buttons_do_not_overlap_and_fit_the_screen() {
        for (w, h) in [(633.0, 292.0), (427.0, 240.0), (480.0, 270.0)] {
            let l = layout(w, h);
            for (i, (a, ra)) in l.iter().enumerate() {
                assert!(
                    ra[0] >= 0.0 && ra[1] >= 0.0 && ra[0] + ra[2] <= w && ra[1] + ra[3] <= h,
                    "{a:?} off screen"
                );
                for (b, rb) in &l[i + 1..] {
                    let apart = ra[0] + ra[2] <= rb[0]
                        || rb[0] + rb[2] <= ra[0]
                        || ra[1] + ra[3] <= rb[1]
                        || rb[1] + rb[3] <= ra[1];
                    assert!(apart, "{a:?} overlaps {b:?} at {w}x{h}");
                }
            }
        }
    }

    #[test]
    fn the_dpad_follows_a_sliding_finger() {
        let l = layout(633.0, 292.0);
        let mut t = Touches::default();
        assert_eq!(
            t.start(1, at(&l, Control::Forward), 0.0, &l),
            [Out::Press(Control::Forward)]
        );
        assert!(t.held(Control::Forward));
        assert_eq!(
            t.moved(1, at(&l, Control::Right), &l),
            [Out::Release(Control::Forward), Out::Press(Control::Right)]
        );
        // Off the pad, the last direction stays held, as in Pocket Edition.
        assert_eq!(t.moved(1, (300.0, 100.0), &l), []);
        assert_eq!(t.end(1), [Out::Release(Control::Right)]);
        assert!(!t.held(Control::Right));
    }

    #[test]
    fn a_quick_still_touch_taps_and_a_held_one_mines() {
        let l = layout(633.0, 292.0);
        let mut t = Touches::default();
        assert_eq!(t.start(1, (300.0, 100.0), 0.0, &l), []);
        assert_eq!(t.frame(0.1), []);
        assert_eq!(t.end(1), [Out::Tap]);

        t.start(2, (300.0, 100.0), 1.0, &l);
        assert_eq!(t.frame(1.3), []);
        assert_eq!(t.frame(1.4), [Out::MineStart]);
        assert_eq!(t.frame(1.5), []);
        assert_eq!(t.end(2), [Out::MineEnd]);
    }

    #[test]
    fn a_dragging_finger_turns_the_view_and_does_not_tap() {
        let l = layout(633.0, 292.0);
        let mut t = Touches::default();
        t.start(1, (300.0, 100.0), 0.0, &l);
        // Within the slop (a thumb's wobble): no look yet, and still a tap candidate.
        assert_eq!(t.moved(1, (305.0, 100.0), &l), []);
        assert_eq!(t.moved(1, (313.0, 104.0), &l), [Out::Look(8.0, 4.0)]);
        assert_eq!(t.frame(5.0), []);
        assert_eq!(t.end(1), []);
    }

    #[test]
    fn two_fingers_move_and_look_at_once() {
        let l = layout(633.0, 292.0);
        let mut t = Touches::default();
        t.start(1, at(&l, Control::Forward), 0.0, &l);
        t.start(2, (400.0, 100.0), 0.0, &l);
        assert_eq!(t.moved(2, (420.0, 100.0), &l), [Out::Look(20.0, 0.0)]);
        assert!(t.held(Control::Forward));
        let mut all = t.release_all();
        all.sort_by_key(|o| format!("{o:?}"));
        assert_eq!(all, [Out::Release(Control::Forward)]);
    }
}

#[cfg(test)]
mod custom_layout_tests {
    use super::*;
    #[test]
    fn unset_reset_and_invalid_positions_preserve_defaults() {
        for (w, h) in [(844.0, 390.0), (1024.0, 768.0), (390.0, 844.0)] {
            assert_eq!(
                layout_with_positions(w, h, &[f32::NAN; 20]),
                default_layout(w, h)
            );
            assert_eq!(
                layout_with_positions(w, h, &[f32::INFINITY; 20]),
                default_layout(w, h)
            );
        }
    }
    #[test]
    fn moved_jump_uses_identical_paint_and_hit_rectangle_after_resize() {
        let mut positions = [f32::NAN; 20];
        positions[10] = 0.65;
        positions[11] = 0.35;
        for (w, h) in [(844.0, 390.0), (1194.0, 834.0), (390.0, 844.0)] {
            let layout = layout_with_positions(w, h, &positions);
            let r = layout.iter().find(|(c, _)| *c == Control::Jump).unwrap().1;
            let centre = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
            assert_eq!(hit(&layout, centre), Some(Control::Jump));
            let mut fingers = Touches::default();
            assert_eq!(
                fingers.start(7, centre, 0.0, &layout),
                vec![Out::Press(Control::Jump)]
            );
            assert_eq!(fingers.end(7), vec![Out::Release(Control::Jump)]);
            assert!((centre.0 / w - 0.65).abs() < 0.0001);
            assert!((centre.1 / h - 0.35).abs() < 0.0001);
        }
    }
    #[test]
    fn overlapping_controls_hit_the_button_painted_on_top_of_other_buttons_and_hotbar() {
        let mut positions = [f32::NAN; 20];
        let default = default_layout(650.0, 300.0);
        let forward = default
            .iter()
            .find(|(c, _)| *c == Control::Forward)
            .unwrap()
            .1;
        positions[10] = (forward[0] + forward[2] / 2.0) / 650.0;
        positions[11] = (forward[1] + forward[3] / 2.0) / 300.0;
        let buttons = layout_with_positions(650.0, 300.0, &positions);
        assert_eq!(
            hit(&buttons, (positions[10] * 650.0, positions[11] * 300.0)),
            Some(Control::Jump)
        );
        positions[10] = 0.5;
        positions[11] = 0.95;
        let buttons = layout_with_positions(650.0, 300.0, &positions);
        assert_eq!(hit(&buttons, (325.0, 285.0)), Some(Control::Jump));
    }

    #[test]
    fn offscreen_centres_clamp_without_moving_hotbar_or_other_buttons() {
        let mut positions = [f32::NAN; 20];
        positions[10] = -20.0;
        positions[11] = 30.0;
        let layout = layout_with_positions(844.0, 390.0, &positions);
        for ((c, r), (_, default)) in layout.iter().zip(default_layout(844.0, 390.0)) {
            if *c == Control::Jump {
                assert_eq!(r[0], 0.0);
                assert_eq!(r[1] + r[3], 390.0);
            } else {
                assert_eq!(*r, default);
            }
        }
    }
}
