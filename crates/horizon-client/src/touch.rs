//! Original touch control layout and per-finger input state. Directional keys,
//! explicit mouse buttons and an optional joystick share the rectangles used by
//! drawing and hit tests. Legacy tap/hold outputs remain available to frontends;
//! the iOS desktop-style frontend uses explicit attack/use buttons instead.

#[path = "touch/desktop.rs"]
pub mod desktop;
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
    /// The D-pad's centre: Shift (hold on the iOS desktop-style frontend).
    Sneak,
    Jump,
    Perspective,
    Chat,
    Pause,
    Inventory,
    Hotbar(u8),
    LeftClick,
    RightClick,
    Joystick,
    Sprint,
    MiddleClick,
    More,
    Hide,
    Runtime,
    Debug,
    PlayerList,
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
    // Amethyst's default proportions, expressed in GUI coordinates. Scale the reference
    // 204.8-point stick with the short edge to keep the overlay usable on phones/tablets.
    let unit = w.min(h) / 768.0;
    let s = 69.6 * unit;
    let stick = 204.8 * unit;
    let gap = 2.0 * unit;
    let right = 0.94878495 * w - s;
    let mut v = vec![
        (
            Control::Joystick,
            [0.02326631 * w, 0.9753418 * h - stick, stick, stick],
        ),
        (Control::Sneak, [right, 0.31131014 * h, s, s]),
        (Control::Jump, [right, 0.31131014 * h + s + gap, s, s]),
        (Control::Sprint, [right - s - gap, 0.31131014 * h, s, s]),
        (
            Control::LeftClick,
            [0.7756397 * w - s, 0.8224885 * h - s, s, s],
        ),
        (
            Control::RightClick,
            [0.8756154 * w - s, 0.8178498 * h - s, s, s],
        ),
        (
            Control::MiddleClick,
            [0.83548373 * w - s, 0.67798775 * h - s, s, s],
        ),
        (
            Control::Perspective,
            [98.0 * unit, 0.0, 96.0 * unit, 40.0 * unit],
        ),
        (Control::Chat, [0.0, 42.0 * unit, 96.0 * unit, 40.0 * unit]),
        (
            Control::Pause,
            [w - 3.0 * 106.8 * unit, 0.0, 104.8 * unit, 39.2 * unit],
        ),
        (
            Control::Inventory,
            [
                w - 3.0 * 106.8 * unit - 102.0 * unit,
                0.0,
                100.0 * unit,
                40.0 * unit,
            ],
        ),
    ];
    v.extend([
        (Control::More, [0.0, 0.0, 96.0 * unit, 40.0 * unit]),
        (
            Control::Debug,
            [294.0 * unit, 0.0, 96.0 * unit, 40.0 * unit],
        ),
        (
            Control::PlayerList,
            [196.0 * unit, 0.0, 96.0 * unit, 40.0 * unit],
        ),
        (
            Control::Hide,
            [w - 69.6 * unit, 0.0, 69.6 * unit, 40.0 * unit],
        ),
        (
            Control::Runtime,
            [w - 176.4 * unit, 0.0, 104.8 * unit, 39.2 * unit],
        ),
    ]);
    let d = button_size(h);
    let (cx, cy) = (MARGIN + d, h - MARGIN - 2.0 * d - 16.0);
    v.extend([
        (Control::Forward, [cx, cy - d, d, d]),
        (Control::Back, [cx, cy + d, d, d]),
        (Control::Left, [cx - d, cy, d, d]),
        (Control::Right, [cx + d, cy, d, d]),
    ]);
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
    if h > w {
        for (control, r) in &mut v {
            if matches!(
                control,
                Control::Perspective | Control::Debug | Control::PlayerList
            ) {
                r[1] = 42.0 * unit;
            }
        }
        // Keep the native hotbar reachable when the landscape preset is rotated.
        if let Some((_, r)) = v.iter_mut().find(|(c, _)| *c == Control::Joystick) {
            r[1] = r[1].min(h - 34.0 - r[3]);
        }
    }
    v
}

/// Stable order shared with the app's layout editor; hotbar slots remain on the HUD.
const EDITABLE: [Control; 20] = [
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
    Control::LeftClick,
    Control::RightClick,
    Control::Joystick,
    Control::Sprint,
    Control::MiddleClick,
    Control::More,
    Control::Hide,
    Control::Runtime,
    Control::Debug,
    Control::PlayerList,
];

/// Apply normalized centres to the same rectangles used by paint and hit tests.
/// Missing/non-finite coordinates use the original position; valid centres are
/// clamped by the actual button size, including after resizing or a GUI scale change.
pub fn layout_with_positions(w: f32, h: f32, positions: &[f32]) -> Vec<(Control, Rect)> {
    let mut buttons = default_layout(w, h);
    buttons.retain(|(c, _)| {
        if joystick_mode() {
            !c.moves()
        } else {
            *c != Control::Joystick
        }
    });
    buttons.sort_by_key(|(c, _)| EDITABLE.iter().position(|v| v == c).unwrap_or(usize::MAX));
    for (control, rect) in &mut buttons {
        let Some(i) = EDITABLE.iter().position(|c| c == control) else {
            continue;
        };
        if let (Some(&width), Some(&height)) =
            (positions.get(40 + i * 2), positions.get(41 + i * 2))
            && width.is_finite()
            && height.is_finite()
            && width > 0.0
            && height > 0.0
        {
            let centre = (rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0);
            let height = if *control == Control::Joystick {
                width
            } else {
                height
            };
            rect[2] = (width.clamp(0.02, 0.8) * w.min(h)).min(w);
            rect[3] = (height.clamp(0.02, 0.8) * w.min(h)).min(h);
            rect[0] = (centre.0 - rect[2] / 2.0).clamp(0.0, (w - rect[2]).max(0.0));
            rect[1] = (centre.1 - rect[3] / 2.0).clamp(0.0, (h - rect[3]).max(0.0));
        }
        let (Some(&x), Some(&y)) = (positions.get(i * 2), positions.get(i * 2 + 1)) else {
            continue;
        };
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
    layout_with_positions(w, h, &[])
}

pub fn cursor_mode() -> bool {
    #[cfg(target_os = "ios")]
    {
        horizon_ios_launcher::touch_preferences().1 & 2 != 0
    }
    #[cfg(not(target_os = "ios"))]
    {
        false
    }
}
pub fn mouse_gestures() -> bool {
    #[cfg(target_os = "ios")]
    {
        horizon_ios_launcher::touch_preferences().1 & 8 == 0
    }
    #[cfg(not(target_os = "ios"))]
    {
        true
    }
}

pub fn hotbar_gestures() -> bool {
    #[cfg(target_os = "ios")]
    {
        horizon_ios_launcher::touch_preferences().1 & 16 == 0
    }
    #[cfg(not(target_os = "ios"))]
    {
        true
    }
}

pub fn joystick_mode() -> bool {
    #[cfg(target_os = "ios")]
    {
        horizon_ios_launcher::touch_preferences().1 & 4 != 0
    }
    #[cfg(not(target_os = "ios"))]
    {
        false
    }
}

/// Paint can disappear for a clean capture without disabling the touch regions.
pub fn controls_visible() -> bool {
    #[cfg(target_os = "ios")]
    return horizon_ios_launcher::touch_preferences().1 & 1 == 0;
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
    Ignored,
    Button(Control),
    World { dragged: bool, mining: bool },
    Stick { mask: u8 },
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
    /// Desktop-style camera input has no tap-disambiguation threshold.
    pub direct_look: bool,
    /// UIKit-style buttons retain the initial control until the finger lifts.
    pub capture_buttons: bool,
    stick_state: desktop::Joystick,
    pub forward_lock: bool,
    pub locked: bool,
    stick_rect: Rect,
    look_primary: Option<u64>,
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
        let mut out = self.cancel(id);
        let kind = match hit(layout, at) {
            Some(Control::Joystick) => {
                if self
                    .fingers
                    .iter()
                    .any(|f| matches!(f.kind, Kind::Stick { .. }))
                {
                    Kind::Button(Control::Joystick)
                } else {
                    let r = layout
                        .iter()
                        .find(|(c, _)| *c == Control::Joystick)
                        .unwrap()
                        .1;
                    self.locked = false;
                    self.stick_rect = r;
                    for (c, down) in self.stick_state.sample(r, at, true) {
                        out.push(if down { Out::Press(c) } else { Out::Release(c) });
                    }
                    let mask = self.stick_state.mask;
                    Kind::Stick { mask }
                }
            }
            Some(control) if self.capture_buttons && self.held(control) => Kind::Ignored,
            Some(control) => {
                out.push(Out::Press(control));
                Kind::Button(control)
            }
            None => Kind::World {
                dragged: false,
                mining: false,
            },
        };
        if self.direct_look && matches!(kind, Kind::World { .. }) && self.look_primary.is_none() {
            self.look_primary = Some(id);
        }
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
            Kind::Button(control) if control.moves() && !self.capture_buttons => {
                if let Some(next) = hit(layout, at).filter(|c| c.moves() && c != control) {
                    out.push(Out::Release(*control));
                    out.push(Out::Press(next));
                    *control = next;
                }
            }
            Kind::Stick { mask } => {
                if let Some((_, r)) = layout.iter().find(|(c, _)| *c == Control::Joystick) {
                    for (c, down) in self.stick_state.sample(*r, at, true) {
                        out.push(if down { Out::Press(c) } else { Out::Release(c) });
                    }
                    *mask = self.stick_state.mask;
                }
            }
            Kind::Button(_) | Kind::Ignored => {}
            Kind::World { dragged, .. } => {
                if self.direct_look {
                    if self.look_primary.is_none() {
                        self.look_primary = Some(id);
                    }
                    if self.look_primary != Some(id) {
                        f.last = at;
                        return vec![];
                    }
                }
                let (dx, dy) = (at.0 - f.start.0, at.1 - f.start.1);
                if !*dragged && (self.direct_look || dx.hypot(dy) > SLOP) {
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
        if self.look_primary == Some(id) {
            self.look_primary = None;
        }
        let finger = self.fingers.remove(i);
        if matches!(finger.kind, Kind::Stick { .. })
            && self.forward_lock
            && self.stick_state.mask == 1
            && contains(lock_rect(self.stick_rect), finger.last)
        {
            self.locked = true;
            return vec![];
        }
        match finger.kind {
            Kind::Ignored => vec![],
            Kind::Stick { .. } => self
                .stick_state
                .finish([0., 0., 1., 1.], true)
                .into_iter()
                .map(|(c, down)| if down { Out::Press(c) } else { Out::Release(c) })
                .collect(),
            Kind::Button(control) => {
                if self.held(control) {
                    vec![]
                } else {
                    vec![Out::Release(control)]
                }
            }
            Kind::World { mining: true, .. } => vec![Out::MineEnd],
            Kind::World { dragged: false, .. } => vec![Out::Tap],
            Kind::World { .. } => Vec::new(),
        }
    }

    /// UIKit cancelled a finger: release its holds without interpreting it as a tap.
    pub fn cancel(&mut self, id: u64) -> Vec<Out> {
        let enabled = self.forward_lock;
        self.forward_lock = false;
        let mut out = self.end(id);
        self.forward_lock = enabled;
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
        let mut out: Vec<_> = ids.into_iter().flat_map(|id| self.cancel(id)).collect();
        if self.locked {
            self.locked = false;
            out.extend(
                self.stick_state
                    .finish(self.stick_rect, true)
                    .into_iter()
                    .map(|(c, d)| if d { Out::Press(c) } else { Out::Release(c) }),
            );
        }
        out
    }

    pub fn stick_point(&self, r: Rect) -> (f32, f32) {
        let centre = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
        let Some(f) = self
            .fingers
            .iter()
            .find(|f| matches!(f.kind, Kind::Stick { .. }))
        else {
            return centre;
        };
        let (dx, dy) = (f.last.0 - centre.0, f.last.1 - centre.1);
        let length = dx.hypot(dy);
        let radius = r[2].min(r[3]) * 0.375;
        let scale = if length > radius {
            radius / length
        } else {
            1.0
        };
        (centre.0 + dx * scale, centre.1 + dy * scale)
    }
    /// A finger holds this button down (drawn pressed).
    pub fn held(&self, control: Control) -> bool {
        (self.locked && control == Control::Forward)
            || self.fingers.iter().any(|f| match f.kind {
                Kind::Ignored => false,
                Kind::Button(c) => c == control,
                Kind::Stick { mask } => {
                    control == Control::Joystick
                        || desktop::DIRECTIONS
                            .iter()
                            .enumerate()
                            .any(|(i, c)| *c == control && mask & (1 << i) != 0)
                }
                _ => false,
            })
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
        for (w, h) in [
            (633.0, 292.0),
            (427.0, 240.0),
            (480.0, 270.0),
            (404.0, 278.0),
            (320.0, 240.0),
        ] {
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
                layout_with_positions(w, h, &[])
            );
            assert_eq!(
                layout_with_positions(w, h, &[f32::INFINITY; 20]),
                layout_with_positions(w, h, &[])
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
        for ((c, r), (_, default)) in layout.iter().zip(layout_with_positions(844.0, 390.0, &[])) {
            if *c == Control::Jump {
                assert_eq!(r[0], 0.0);
                assert_eq!(r[1] + r[3], 390.0);
            } else {
                assert_eq!(*r, default);
            }
        }
    }
}

#[cfg(test)]
mod stick_gesture_tests {
    use super::*;
    #[test]
    fn desktop_camera_reports_small_relative_motion_without_tap_slop() {
        let mut fingers = Touches {
            direct_look: true,
            ..Default::default()
        };
        fingers.start(1, (10., 10.), 0., &[]);
        assert_eq!(fingers.moved(1, (11., 12.), &[]), vec![Out::Look(1., 2.)]);
        assert!(fingers.end(1).is_empty());
    }
    #[test]
    fn joystick_down_diagonal_and_release_use_keyboard_edges() {
        let mut fingers = Touches::default();
        let layout = [(Control::Joystick, [0., 0., 100., 100.])];
        assert_eq!(
            fingers.start(1, (99., 1.), 0., &layout),
            vec![
                Out::Press(Control::Forward),
                Out::Release(Control::Left),
                Out::Release(Control::Back),
                Out::Press(Control::Right)
            ]
        );
        assert!(fingers.held(Control::Forward) && fingers.held(Control::Right));
        assert!(fingers.start(2, (1., 99.), 0., &layout).is_empty());
        assert_eq!(fingers.moved(1, (50., 50.), &layout), vec![]);
        assert!(fingers.held(Control::Forward) && fingers.held(Control::Right));
        assert_eq!(
            fingers.moved(1, (1., 99.), &layout),
            vec![
                Out::Release(Control::Forward),
                Out::Press(Control::Left),
                Out::Press(Control::Back),
                Out::Release(Control::Right)
            ]
        );
        assert_eq!(
            fingers.cancel(1),
            vec![
                Out::Release(Control::Forward),
                Out::Release(Control::Left),
                Out::Release(Control::Back),
                Out::Release(Control::Right)
            ]
        );
        assert!(!fingers.held(Control::Forward) && !fingers.held(Control::Back));
    }
    #[test]
    fn desktop_camera_uses_only_primary_finger_and_hands_off_on_release() {
        let mut fingers = Touches {
            direct_look: true,
            ..Default::default()
        };
        fingers.start(1, (10., 10.), 0., &[]);
        fingers.start(2, (20., 20.), 0., &[]);
        assert!(fingers.moved(2, (21., 22.), &[]).is_empty());
        assert_eq!(fingers.moved(1, (12., 13.), &[]), [Out::Look(2., 3.)]);
        fingers.end(1);
        assert_eq!(fingers.moved(2, (24., 26.), &[]), [Out::Look(3., 4.)]);
        fingers.cancel(2);
        assert!(fingers.moved(2, (28., 30.), &[]).is_empty());
    }
}

pub fn opacity(control: Control) -> f32 {
    #[cfg(target_os = "ios")]
    {
        if let Some(i) = EDITABLE.iter().position(|c| *c == control) {
            let v = horizon_ios_launcher::touch_preferences().0[80 + i];
            if v.is_finite() {
                return v.clamp(0.1, 1.0);
            }
        }
    }
    let _ = control;
    1.0
}
pub fn forward_lock() -> bool {
    #[cfg(target_os = "ios")]
    {
        horizon_ios_launcher::touch_preferences().1 & 32 != 0
    }
    #[cfg(not(target_os = "ios"))]
    false
}
pub fn lock_rect(r: Rect) -> Rect {
    [
        r[0] + r[2] * 0.375,
        r[1] - r[2] * 0.625,
        r[2] * 0.25,
        r[2] * 0.25,
    ]
}

#[cfg(test)]
mod amethyst_port_tests {
    use super::*;
    #[test]
    fn lock_releases_on_cancel_and_retakes_on_touch() {
        let r = [0.0, 100.0, 100.0, 100.0];
        let layout = [(Control::Joystick, r)];
        let mut t = Touches {
            forward_lock: true,
            ..Default::default()
        };
        t.start(1, (50.01, 110.0), 0.0, &layout);
        t.moved(1, (50.01, 50.0), &layout);
        t.end(1);
        assert!(t.locked);
        assert!(t.held(Control::Forward));
        let out = t.release_all();
        assert!(!t.locked);
        assert!(out.contains(&Out::Release(Control::Forward)));
        t.start(2, (50.01, 110.0), 0.0, &layout);
        t.moved(2, (50.01, 50.0), &layout);
        t.cancel(2);
        assert!(!t.locked);
        assert!(!t.held(Control::Forward));
    }
    #[test]
    fn sizing_preserves_default_center_and_opacity_ids() {
        let mut values = [f32::NAN; 100];
        let a = layout_with_positions(800.0, 400.0, &values);
        let before = a.iter().find(|(c, _)| *c == Control::Jump).unwrap().1;
        values[50] = 0.2;
        values[51] = 0.15;
        let after = layout_with_positions(800.0, 400.0, &values)
            .into_iter()
            .find(|(c, _)| *c == Control::Jump)
            .unwrap()
            .1;
        assert_eq!(after[2], 80.0);
        assert!((before[0] + before[2] / 2.0 - after[0] - after[2] / 2.0).abs() < 0.001);
        assert_eq!(EDITABLE[13], Control::Sprint);
        assert_eq!(EDITABLE[14], Control::MiddleClick);
    }
}
