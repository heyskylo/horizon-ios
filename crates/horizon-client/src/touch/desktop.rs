//! Independently written input adapter. Source pins, differential oracle and
//! deliberate platform adaptations are documented in docs/ios/pojav-input-parity.md.
use super::{Control, Rect, hit_rect};
pub const DIRECTIONS: [Control; 4] = [
    Control::Forward,
    Control::Back,
    Control::Left,
    Control::Right,
];
/// Keyboard order at the control-to-game boundary: W, A, S, D.
pub const KEY_ORDER: [Control; 4] = [
    Control::Forward,
    Control::Left,
    Control::Back,
    Control::Right,
];
#[derive(Clone, Debug)]
pub struct Joystick {
    sector: i8,
    pub mask: u8,
    pub thumb: (f64, f64),
    pub velocity: (f64, f64),
}
impl Default for Joystick {
    fn default() -> Self {
        Self {
            sector: -2,
            mask: 0,
            thumb: (0.0, 0.0),
            velocity: (0.0, 0.0),
        }
    }
}
impl Joystick {
    fn direction(&mut self, x: f64, y: f64) -> Vec<(Control, bool)> {
        let next = if x == 0.0 || y == 0.0 {
            -1
        } else {
            let radians = (y as f32).atan2(x as f32) as f64;
            let degrees = (radians * 180.0 / std::f64::consts::PI).rem_euclid(360.0);
            (((degrees + 22.5) / 45.0) as i32 % 8) as i8
        };
        if self.sector == next {
            return vec![];
        }
        self.sector = next;
        self.mask = if next < 0 {
            0
        } else {
            [8, 9, 1, 5, 4, 6, 2, 10][next as usize]
        };
        KEY_ORDER
            .into_iter()
            .map(|c| {
                let bit = match c {
                    Control::Forward => 1,
                    Control::Back => 2,
                    Control::Left => 4,
                    _ => 8,
                };
                (c, self.mask & bit != 0)
            })
            .collect()
    }
    /// UIKit reference geometry: quarter-width knob, 3/8-side travel. In
    /// grabbed mode, inner-zone samples retain the prior direction until end.
    pub fn sample(&mut self, r: Rect, at: (f32, f32), grabbed: bool) -> Vec<(Control, bool)> {
        let (w, h) = (f64::from(r[2]), f64::from(r[3]));
        if w <= 0.0 || h <= 0.0 || !at.0.is_finite() || !at.1.is_finite() {
            return self.finish(r, grabbed);
        }
        let (x, y) = (
            f64::from(at.0) - f64::from(r[0]) - w / 2.0,
            f64::from(at.1) - f64::from(r[1]) - h / 2.0,
        );
        let distance = x.hypot(y);
        let travel = w.min(h) * 0.375;
        let ratio = if distance > travel {
            travel / distance
        } else {
            1.0
        };
        self.thumb = (w / 2.0 + x * ratio, h / 2.0 + y * ratio);
        if grabbed && distance < travel * 0.35 {
            return vec![];
        }
        let v = (self.thumb.0 / w * 2.0 - 1.0, 1.0 - self.thumb.1 / h * 2.0);
        if grabbed {
            self.direction(v.0, v.1)
        } else {
            self.velocity = v;
            vec![]
        }
    }
    pub fn finish(&mut self, r: Rect, grabbed: bool) -> Vec<(Control, bool)> {
        self.thumb = (f64::from(r[2]) / 2.0, f64::from(r[3]) / 2.0);
        self.velocity = (0.0, 0.0);
        if grabbed {
            self.direction(0.0, 0.0)
        } else {
            vec![]
        }
    }
}
pub fn stick_keys(r: Rect, at: (f32, f32)) -> u8 {
    let mut state = Joystick::default();
    state.sample(r, at, true);
    state.mask
}

/// Coordinate transform before the backend-specific cursor callback. Values
/// stay double precision between samples; rendering may round the final point.
#[derive(Clone, Debug)]
pub struct Pointer {
    pub position: (f64, f64),
    last: (f64, f64),
    pub speed: f64,
    pub scale: f64,
    pub resolution: f64,
    pub size: (f64, f64),
}
impl Pointer {
    pub fn new(size: (f64, f64)) -> Self {
        Self {
            position: (size.0 / 2.0, size.1 / 2.0),
            last: (0.0, 0.0),
            speed: 1.0,
            scale: 1.0,
            resolution: 1.0,
            size,
        }
    }
    /// phase: down=0, up=1, absolute move=2, relative motion=3.
    pub fn point(
        &mut self,
        phase: u8,
        at: (f64, f64),
        grabbed: bool,
        virtual_mouse: bool,
    ) -> (f64, f64) {
        if !grabbed && virtual_mouse {
            let delta = match phase {
                2 => (at.0 - self.last.0, at.1 - self.last.1),
                3 => at,
                _ => (0.0, 0.0),
            };
            self.position.0 = (self.position.0 + delta.0 * self.speed).clamp(0.0, self.size.0);
            self.position.1 = (self.position.1 + delta.1 * self.speed).clamp(0.0, self.size.1);
            self.last = at;
            return (
                self.position.0 * self.scale * self.resolution,
                self.position.1 * self.scale * self.resolution,
            );
        }
        if !grabbed {
            self.last = at;
        }
        let factor = self.scale * if grabbed { 1.0 } else { self.resolution };
        (at.0 * factor, at.1 * factor)
    }
}

/// Pojav's frame-time scaled menu joystick acceleration, before cursor clamping.
#[derive(Default)]
pub struct MotionClock {
    last: f64,
}
impl MotionClock {
    pub fn tick(&mut self, time: f64, velocity: (f64, f64)) -> Option<(f64, f64)> {
        let previous = self.last;
        self.last = time;
        if previous == 0.0 || velocity == (0.0, 0.0) {
            return None;
        }
        let acceleration = velocity.0.hypot(velocity.1).powi(2).min(1.0);
        let elapsed = (time - previous) / (1.0 / 60.0);
        Some((
            velocity.0 * acceleration * 18.0 * elapsed,
            -velocity.1 * acceleration * 18.0 * elapsed,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Move(f32, f32),
    Button(bool, bool),
    Key(Control, bool),
}
#[derive(Default)]
pub struct Cursor {
    pub position: Option<(f32, f32)>,
    direct: bool,
    pointer: Option<Pointer>,
    primary: Option<u64>,
    fingers: Vec<(u64, (f32, f32), Option<bool>)>,
    key_fingers: Vec<(u64, Control)>,
    ignored: Vec<u64>,
    pub joystick_rect: Option<Rect>,
    pub joystick: Joystick,
    stick_finger: Option<u64>,
    clock: MotionClock,
}
impl Cursor {
    pub fn primary_finger(&self) -> Option<u64> {
        self.primary
    }
    pub fn surface_finger(&self, id: u64) -> bool {
        self.fingers
            .iter()
            .any(|(finger, _, button)| *finger == id && button.is_none())
    }
    pub fn set_virtual_mouse(&mut self, enabled: bool) {
        self.direct = !enabled;
    }
    /// Button capture is identical over menus and worlds: a key press is not
    /// converted into a click or a different key when the finger moves away.
    pub fn event_controls(
        &mut self,
        id: u64,
        phase: u8,
        at: (f32, f32),
        size: (f32, f32),
        layout: &[(Control, Rect)],
    ) -> Vec<Action> {
        if !at.0.is_finite()
            || !at.1.is_finite()
            || !size.0.is_finite()
            || !size.1.is_finite()
            || size.0 <= 0.0
            || size.1 <= 0.0
        {
            return self.cancel();
        }
        if let Some(i) = self
            .key_fingers
            .iter()
            .position(|(finger, _)| *finger == id)
        {
            if phase < 2 {
                return vec![];
            }
            let (_, control) = self.key_fingers.remove(i);
            return if self.key_fingers.iter().any(|(_, c)| *c == control) {
                vec![]
            } else {
                vec![Action::Key(control, false)]
            };
        }
        if phase == 0
            && let Some(control) = super::hit(layout, at)
            && !matches!(
                control,
                Control::Joystick | Control::Hotbar(_) | Control::LeftClick | Control::RightClick
            )
        {
            if self.key_fingers.len() >= 32 {
                return vec![];
            }
            let held = self.key_fingers.iter().any(|(_, c)| *c == control);
            if held {
                self.ignored.push(id);
            } else {
                self.key_fingers.push((id, control));
            }
            return if held {
                vec![]
            } else {
                vec![Action::Key(control, true)]
            };
        }
        self.joystick_rect = layout
            .iter()
            .find(|(c, _)| *c == Control::Joystick)
            .map(|(_, r)| *r);
        let rect = |control| {
            layout
                .iter()
                .find(|(c, _)| *c == control)
                .map_or([0.; 4], |(_, r)| *r)
        };
        self.event(
            id,
            phase,
            at,
            size,
            [rect(Control::LeftClick), rect(Control::RightClick)],
        )
    }
    pub fn event(
        &mut self,
        id: u64,
        phase: u8,
        at: (f32, f32),
        size: (f32, f32),
        buttons: [Rect; 2],
    ) -> Vec<Action> {
        if !size.0.is_finite() || !size.1.is_finite() || size.0 <= 0.0 || size.1 <= 0.0 {
            return self.cancel();
        }
        if !at.0.is_finite() || !at.1.is_finite() {
            return self.event(id, 3, (0.0, 0.0), size, buttons);
        }
        if self.ignored.contains(&id) {
            if phase >= 2 {
                self.ignored.retain(|finger| *finger != id);
            }
            return vec![];
        }
        let pointer = self
            .pointer
            .get_or_insert_with(|| Pointer::new((f64::from(size.0), f64::from(size.1))));
        pointer.size = (f64::from(size.0), f64::from(size.1));
        pointer.position.0 = pointer.position.0.clamp(0.0, pointer.size.0);
        pointer.position.1 = pointer.position.1.clamp(0.0, pointer.size.1);
        if self.position.is_none() || !self.direct {
            self.position = Some((pointer.position.0 as f32, pointer.position.1 as f32));
        }
        let mut out = Vec::new();
        if self.stick_finger == Some(id) {
            let Some(r) = self.joystick_rect else {
                self.stick_finger = None;
                self.joystick.velocity = (0.0, 0.0);
                return out;
            };
            if phase >= 2 {
                self.joystick.finish(r, false);
                self.stick_finger = None;
            } else if phase == 1 {
                self.joystick.sample(r, at, false);
            }
            return out;
        }
        if phase == 0
            && self.stick_finger.is_some()
            && self.joystick_rect.is_some_and(|r| hit_rect(r, at))
        {
            self.ignored.push(id);
            return out;
        }
        if phase == 0
            && self.stick_finger.is_none()
            && let Some(r) = self.joystick_rect.filter(|r| hit_rect(*r, at))
        {
            self.stick_finger = Some(id);
            self.joystick.sample(r, at, false);
            return out;
        }
        if phase == 0 {
            out.extend(self.event(id, 3, at, size, buttons));
            if self.fingers.len() >= 32 {
                return out;
            }
            let button = if hit_rect(buttons[0], at) {
                Some(false)
            } else if hit_rect(buttons[1], at) {
                Some(true)
            } else {
                None
            };
            if let Some(right) = button
                && self.held(right)
            {
                self.ignored.push(id);
                return out;
            }
            if let Some(right) = button {
                out.push(Action::Button(right, true));
            }
            self.fingers.push((id, at, button));
            if button.is_none() && self.primary.is_none() {
                self.primary = Some(id);
                let p = self.pointer.as_mut().unwrap().point(
                    0,
                    (f64::from(at.0), f64::from(at.1)),
                    false,
                    !self.direct,
                );
                self.position = Some((p.0 as f32, p.1 as f32));
                out.push(Action::Move(p.0 as f32, p.1 as f32));
            }
        } else if let Some(i) = self.fingers.iter().position(|f| f.0 == id) {
            let (_, _last, button) = self.fingers[i];
            if phase == 1 && button.is_none() {
                if self.primary.is_none() {
                    self.primary = Some(id);
                    let p = self.pointer.as_mut().unwrap().point(
                        0,
                        (f64::from(at.0), f64::from(at.1)),
                        false,
                        !self.direct,
                    );
                    self.position = Some((p.0 as f32, p.1 as f32));
                    out.push(Action::Move(p.0 as f32, p.1 as f32));
                }
                if self.primary == Some(id) {
                    let p = self.pointer.as_mut().unwrap().point(
                        2,
                        (f64::from(at.0), f64::from(at.1)),
                        false,
                        !self.direct,
                    );
                    self.position = Some((p.0 as f32, p.1 as f32));
                    out.push(Action::Move(p.0 as f32, p.1 as f32));
                }
                self.fingers[i].1 = at;
            }
            if phase >= 2 {
                self.fingers.remove(i);
                if self.primary == Some(id) {
                    self.primary = None;
                }
                if let Some(right) = button
                    && !self.held(right)
                {
                    out.push(Action::Button(right, false));
                }
            }
        }
        out
    }
    pub fn tick(&mut self, time: f64) -> Option<Action> {
        let delta = self.clock.tick(time, self.joystick.velocity)?;
        let pointer = self.pointer.as_mut()?;
        let mut at = pointer.point(3, delta, false, !self.direct);
        if self.direct {
            let previous = self.position.unwrap_or((0.0, 0.0));
            at.0 += f64::from(previous.0);
            at.1 += f64::from(previous.1);
        }
        self.position = Some((at.0 as f32, at.1 as f32));
        Some(Action::Move(at.0 as f32, at.1 as f32))
    }
    pub fn held(&self, right: bool) -> bool {
        self.fingers.iter().any(|f| f.2 == Some(right))
    }
    pub fn cancel(&mut self) -> Vec<Action> {
        let mut out: Vec<Action> = [false, true]
            .into_iter()
            .filter(|r| self.held(*r))
            .map(|r| Action::Button(r, false))
            .collect();
        for (_, control) in std::mem::take(&mut self.key_fingers) {
            if !out.contains(&Action::Key(control, false)) {
                out.push(Action::Key(control, false));
            }
        }
        self.ignored.clear();
        self.fingers.clear();
        self.primary = None;
        if let Some(r) = self.joystick_rect {
            self.joystick.finish(r, false);
        }
        self.stick_finger = None;
        self.joystick.velocity = (0.0, 0.0);
        self.clock = MotionClock::default();
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const B: [Rect; 2] = [[0., 80., 20., 20.], [80., 80., 20., 20.]];
    #[test]
    fn relative_cursor_and_independent_drag_clicks() {
        let mut c = Cursor::default();
        assert_eq!(
            c.event(1, 0, (30., 30.), (100., 100.), B),
            [Action::Move(50., 50.)]
        );
        assert_eq!(
            c.event(1, 1, (40., 35.), (100., 100.), B),
            [Action::Move(60., 55.)]
        );
        assert_eq!(
            c.event(2, 0, (5., 85.), (100., 100.), B),
            [Action::Button(false, true)]
        );
        assert_eq!(
            c.event(3, 0, (85., 85.), (100., 100.), B),
            [Action::Button(true, true)]
        );
        assert_eq!(
            c.event(1, 1, (50., 35.), (100., 100.), B),
            [Action::Move(70., 55.)]
        );
        assert_eq!(
            c.event(2, 2, (5., 85.), (100., 100.), B),
            [Action::Button(false, false)]
        );
        assert_eq!(c.cancel(), [Action::Button(true, false)]);
    }
    #[test]
    fn ui_button_ignores_a_second_finger_until_it_lifts() {
        let mut c = Cursor::default();
        c.event(1, 0, (5., 85.), (100., 100.), B);
        assert!(c.event(2, 0, (5., 85.), (100., 100.), B).is_empty());
        assert_eq!(
            c.event(1, 3, (5., 85.), (100., 100.), B),
            [Action::Button(false, false)]
        );
        assert!(c.event(2, 3, (5., 85.), (100., 100.), B).is_empty());
    }
    #[test]
    fn joystick_emits_diagonal_keys_and_has_deadzone() {
        assert_eq!(stick_keys([0., 0., 100., 100.], (50., 50.)), 0);
        assert_eq!(stick_keys([0., 0., 100., 100.], (100., 0.)), 9);
        assert_eq!(stick_keys([0., 0., 100., 100.], (0., 100.)), 6);
    }
    #[test]
    fn primary_finger_handoff_has_no_jump_and_cancel_drops_old_origin() {
        let mut c = Cursor::default();
        c.event(1, 0, (30., 30.), (100., 100.), B);
        assert!(c.event(2, 0, (70., 30.), (100., 100.), B).is_empty());
        assert!(c.event(2, 1, (80., 40.), (100., 100.), B).is_empty());
        c.event(1, 2, (30., 30.), (100., 100.), B);
        assert_eq!(
            c.event(2, 1, (90., 50.), (100., 100.), B),
            [Action::Move(50., 50.), Action::Move(50., 50.)]
        );
        assert_eq!(
            c.event(2, 1, (95., 55.), (100., 100.), B),
            [Action::Move(55., 55.)]
        );
        c.cancel();
        assert_eq!(
            c.event(3, 0, (10., 10.), (100., 100.), B),
            [Action::Move(55., 55.)]
        );
        assert_eq!(
            c.event(3, 1, (11., 12.), (100., 100.), B),
            [Action::Move(56., 57.)]
        );
    }
    #[test]
    fn menu_joystick_drives_cursor_while_click_finger_remains_held() {
        let mut c = Cursor {
            joystick_rect: Some([0., 0., 40., 40.]),
            ..Default::default()
        };
        c.event(1, 0, (30., 10.), (100., 100.), B);
        assert!(c.event(9, 0, (20., 20.), (100., 100.), B).is_empty());
        assert!(c.event(9, 1, (30., 30.), (100., 100.), B).is_empty());
        c.event(9, 2, (30., 30.), (100., 100.), B);
        assert!(c.tick(1.0).is_none());
        assert_eq!(c.tick(1.0 + 1.0 / 60.0), Some(Action::Move(54.5, 45.5)));
        assert_eq!(
            c.event(2, 0, (5., 85.), (100., 100.), B),
            [Action::Button(false, true)]
        );
        c.event(1, 2, (30., 10.), (100., 100.), B);
        assert!(c.tick(2.0).is_none());
        assert!(c.held(false));
        assert_eq!(c.cancel(), [Action::Button(false, false)]);
        assert!(c.tick(3.0).is_none());
    }
    #[test]
    fn source_axis_quirk_and_sticky_deadzone_remain_explicit() {
        let mut j = Joystick::default();
        let r = [0., 0., 100., 100.];
        assert_eq!(j.sample(r, (99., 1.), true).len(), 4);
        assert_eq!(j.mask, 9);
        assert!(j.sample(r, (50., 50.), true).is_empty());
        assert_eq!(j.mask, 9);
        assert_eq!(
            j.sample(r, (100., 50.), true),
            KEY_ORDER.map(|k| (k, false))
        );
        assert_eq!(j.mask, 0);
        assert!(j.finish(r, true).is_empty());
    }
    #[test]
    fn keys_capture_their_button_and_screen_reset_releases_them() {
        let mut c = Cursor::default();
        let layout = [
            (Control::Forward, [0., 0., 20., 20.]),
            (Control::Left, [20., 0., 20., 20.]),
        ];
        assert_eq!(
            c.event_controls(1, 0, (10., 10.), (100., 100.), &layout),
            [Action::Key(Control::Forward, true)]
        );
        assert!(
            c.event_controls(1, 1, (30., 10.), (100., 100.), &layout)
                .is_empty()
        );
        assert_eq!(
            c.event_controls(1, 2, (90., 90.), (100., 100.), &layout),
            [Action::Key(Control::Forward, false)]
        );
        c.event_controls(2, 0, (30., 10.), (100., 100.), &layout);
        assert_eq!(c.cancel(), [Action::Key(Control::Left, false)]);
        assert!(c.cancel().is_empty());
    }
    #[test]
    fn direct_pointer_and_joystick_keep_the_callback_cursor_position() {
        let mut c = Cursor::default();
        c.set_virtual_mouse(false);
        assert_eq!(
            c.event(1, 0, (30., 40.), (100., 100.), B),
            [Action::Move(30., 40.)]
        );
        assert_eq!(
            c.event(1, 1, (35., 45.), (100., 100.), B),
            [Action::Move(35., 45.)]
        );
        c.event(2, 0, (5., 85.), (100., 100.), B);
        assert_eq!(c.position, Some((35., 45.)));
        c.joystick.velocity = (0.5, 0.5);
        assert!(c.tick(1.).is_none());
        assert_eq!(c.tick(1. + 1. / 60.), Some(Action::Move(39.5, 40.5)));
    }
}

/// A physical input emitted by a recognized native gesture. Delays are wall
/// time, not frame counts: a reference tap releases after 33 milliseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Physical {
    Key(u16, bool),
    CustomKey(u16,bool),
    CustomMouse(u8,bool),
    Mouse(u8, bool),
    Scroll(f64, f64),
    Keyboard,
    ToggleControls,
    Cancel,
}
#[derive(Clone, Copy, Debug)]
pub struct Recognized {
    pub kind: u8,
    pub phase: u8,
    pub slot: i8,
    pub point: (f64, f64),
}
impl Recognized {
    pub fn inputs(self, grabbed: bool, mouse_gestures: bool) -> Vec<(f64, Physical)> {
        let slot = (0..9).contains(&self.slot);
        match self.kind {
            0 => vec![(0.0, Physical::Cancel)],
            7 if self.phase == 1 || self.phase == 3 => {
                let down = self.phase == 1;
                match self.point.0 as i32 {
                    key @ 1..=65535 => vec![(0.0, Physical::CustomKey(key as u16, down))],
                    -3 => vec![(0.0, Physical::CustomMouse(0, down))],
                    -4 => vec![(0.0, Physical::CustomMouse(1, down))],
                    -6 => vec![(0.0, Physical::CustomMouse(2, down))],
                    -7 if !down => vec![(0.0, Physical::Scroll(0.0, -1.0))],
                    -8 if !down => vec![(0.0, Physical::Scroll(0.0, 1.0))],
                    -1 if !down => vec![(0.0, Physical::Keyboard)],
                    -2 if !down => vec![(0.0, Physical::ToggleControls)],
                    _ => vec![],
                }
            }
            1 if self.phase == 3 => {
                if slot {
                    let key = 49 + self.slot as u16;
                    vec![
                        (0.0, Physical::Key(key, true)),
                        (0.0, Physical::Key(key, false)),
                    ]
                } else if mouse_gestures {
                    let button = u8::from(grabbed);
                    vec![
                        (0.0, Physical::Mouse(button, true)),
                        (0.033, Physical::Mouse(button, false)),
                    ]
                } else {
                    vec![]
                }
            }
            2 => {
                let pressed = self.phase == 1;
                if !pressed && ![3, 4, 5].contains(&self.phase) {
                    return vec![];
                }
                if slot {
                    vec![(0.0, Physical::Key(81, pressed))]
                } else if mouse_gestures {
                    vec![(0.0, Physical::Mouse(0, pressed))]
                } else {
                    vec![]
                }
            }
            6 if [1, 2].contains(&self.phase) && grabbed && slot => {
                let key = 49 + self.slot as u16;
                vec![
                    (0.0, Physical::Key(key, true)),
                    (0.0, Physical::Key(key, false)),
                ]
            }
            3 if self.phase == 3 && grabbed && slot => vec![
                (0.0, Physical::Key(70, true)),
                (0.0, Physical::Key(70, false)),
            ],
            4 if !grabbed && [1, 2, 3].contains(&self.phase) && self.point != (0.0, 0.0) => {
                vec![(0.0, Physical::Scroll(self.point.0, self.point.1))]
            }
            5 if self.phase == 1 => vec![(0.0, Physical::Keyboard)],
            _ => vec![],
        }
    }
}
