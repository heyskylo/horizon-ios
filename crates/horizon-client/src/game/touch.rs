//! Touch screens (phones and tablets; Horizon's own): Pocket Edition's controls while playing, and a finger
//! as the mouse over every screen. The layout and the fingers' state are the lib's `touch`; this file acts on
//! what they say the way the keyboard and mouse would: the D-pad and buttons are their default keys, a tap is
//! a click, holding still is a held left button, and a drag turns the view as the mouse does.
use super::event_loop::EventLoopCtl;
use super::{Game, Screen};
use horizon_client::{
    Result,
    hud::{HudBatch, rgba},
    touch::{self, Control, Out, Touches},
};
use std::time::Instant;
use winit::{
    dpi::PhysicalPosition,
    event::{
        DeviceId, ElementState, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent,
    },
    keyboard::KeyCode,
    window::WindowId,
};

/// GUI pixels a finger drags over a menu for one line of scrolling.
const SCROLL_LINE: f32 = 12.0;

#[derive(Default)]
pub(super) struct TouchInput {
    fingers: Touches,
    /// A pointer event derived from a finger; the trace records the original
    /// touch once, not this translation a second time.
    synthetic: bool,
    /// A touch has been seen: the controls are drawn (from the start on iPhone and iPad).
    seen: bool,
    /// The last input device controls only the visibility of touch overlays.
    hardware: super::input_mode::InputMode,
    /// The finger that is the pointer over a menu.
    menu: Option<MenuFinger>,
    /// Clicks to release once a tick has passed: vanilla samples buttons once a tick, so a tap released in
    /// the same frame would be lost (the pad holds its short presses back the same way).
    late: Vec<(MouseButton, u64)>,
    /// The button a finger held still on the world is holding down: attack to mine, or use to eat, drink or
    /// draw a bow.
    holding: Option<MouseButton>,
    clock: Option<Instant>,
}

/// A finger over a menu. Its press waits for a frame drawn with the pointer under it, and its release for a
/// frame after the press: screens find what is under the pointer while drawing, so a pointer that moved and
/// pressed in one go would press whatever was under the old position (or nothing).
struct MenuFinger {
    id: u64,
    /// Where the scrolling last caught up with the finger (GUI pixels).
    scrolled: (f32, f32),
    /// Loop iterations since it went down.
    frames: u8,
    pressed: bool,
    lifted: bool,
    /// It went down on the close button, which already acted: the rest of it is ignored.
    closed: bool,
}

/// The key a button stands for: the vanilla default of its mapping.
fn key_of(control: Control) -> Option<KeyCode> {
    Some(match control {
        Control::Forward => KeyCode::KeyW,
        Control::Back => KeyCode::KeyS,
        Control::Left => KeyCode::KeyA,
        Control::Right => KeyCode::KeyD,
        Control::Jump => KeyCode::Space,
        Control::Perspective => KeyCode::F5,
        Control::Chat => KeyCode::KeyT,
        Control::Pause => KeyCode::Escape,
        Control::Inventory => KeyCode::KeyE,
        Control::Sneak | Control::Hotbar(_) => return None,
    })
}

impl Game<'_> {
    /// Where touches land, in GUI pixels: the screen the game draws on, or the phone's window when the
    /// phone is splitscreen's controller ([`Self::touch_on_phone`]).
    fn touch_batch(&self) -> Option<HudBatch> {
        if self.touch_on_phone() {
            return self.window.as_ref().map(|w| {
                let size = w.inner_size();
                HudBatch::new([size.width, size.height])
            });
        }
        self.screen_batch()
    }

    /// The screen the game draws on, in GUI pixels.
    fn screen_batch(&self) -> Option<HudBatch> {
        self.renderer
            .as_ref()
            .map(|r| HudBatch::new([r.config.width, r.config.height]))
    }

    /// Splitscreen's second player on the AirPlay display (game/splitscreen.rs): the phone is their
    /// controller, with the buttons drawn on it and not on the display, laid out in its own pixels.
    pub(super) fn touch_on_phone(&self) -> bool {
        self.splitscreen.guest && self.on_airplay()
    }

    /// A point of the touch screen on the screen the game draws on: the same point, or, when the phone is the
    /// controller, the point as far across and down the display's half as it was across and down the phone.
    fn screen_point(&self, at: (f32, f32)) -> (f32, f32) {
        if !self.touch_on_phone() {
            return at;
        }
        match (self.touch_batch(), self.screen_batch()) {
            (Some(phone), Some(screen)) => (
                at.0 / phone.width.max(1.0) * screen.width,
                at.1 / phone.height.max(1.0) * screen.height,
            ),
            _ => at,
        }
    }

    fn touch_now(&mut self) -> f64 {
        self.touch
            .clock
            .get_or_insert_with(Instant::now)
            .elapsed()
            .as_secs_f64()
    }

    /// The world is under the fingers, with no screen over it.
    pub(super) fn touch_playing(&self) -> bool {
        self.title.is_none()
            && self.screen == Screen::Playing
            && self.sign_edit.is_none()
            && self.book.is_none()
            && self.server_packs.prompt.is_none()
            && !self.dialog_open()
            && self.player.is_some()
    }

    /// The controls are on screen: after a touch, from the start on iPhone and iPad, or with `HORIZON_TOUCH=1`
    /// (laying them out on a computer).
    /// Hide only touch overlays, and only while the last-used hardware is still connected.
    fn hardware_hides_touch(&self) -> bool {
        self.touch.hardware.hides_touch(
            horizon_gamecontroller::keyboard_connected(),
            horizon_gamecontroller::mouse_connected(),
        )
    }

    pub(super) fn touch_keyboard_used(&mut self, event_loop: &dyn EventLoopCtl) {
        self.touch_release_menu(event_loop);
        if horizon_gamecontroller::keyboard_connected() {
            self.touch.hardware.keyboard();
        }
    }

    pub(super) fn touch_mouse_used(&mut self, event_loop: &dyn EventLoopCtl) {
        self.touch_release_menu(event_loop);
        self.touch.hardware.mouse();
    }

    fn touch_shown(&self) -> bool {
        // On AirPlay the phone is covered and the game is played with a controller (game/airplay.rs), but for
        // splitscreen's second player, whose controller the phone is.
        (!self.on_airplay() || self.splitscreen.guest)
            && (self.touch.seen
                || cfg!(any(target_os = "ios", target_os = "android"))
                || std::env::var_os("HORIZON_TOUCH").is_some_and(|v| v == "1"))
    }

    /// winit's `WindowEvent::Touch`.
    pub(super) fn touch_event(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        touch: Touch,
    ) -> Result<()> {
        self.touch.seen = true;
        self.touch.hardware.touch();
        self.last_input = Instant::now();
        let Some(batch) = self.touch_batch() else {
            return Ok(());
        };
        let at = (
            touch.location.x as f32 / batch.scale,
            touch.location.y as f32 / batch.scale,
        );
        let playing = self.touch_playing();
        if playing {
            // Loading or keyboard navigation can consume a menu finger's release.
            // That old finger must not route a new world touch back into the menu.
            self.touch_release_menu(event_loop);
        } else {
            if touch.phase == TouchPhase::Cancelled {
                if self.touch.menu.as_ref().is_some_and(|f| f.id == touch.id) {
                    self.touch_release_menu(event_loop);
                }
            } else {
                self.touch_menu(event_loop, touch.id, touch.phase, at);
            }
            return Ok(());
        }
        let layout = touch::layout(batch.width, batch.height);
        let out = match touch.phase {
            TouchPhase::Started => {
                let now = self.touch_now();
                self.touch.fingers.start(touch.id, at, now, &layout)
            }
            TouchPhase::Moved => self.touch.fingers.moved(touch.id, at, &layout),
            TouchPhase::Ended => self.touch.fingers.end(touch.id),
            TouchPhase::Cancelled => self.touch.fingers.cancel(touch.id),
        };
        self.touch_act(out, batch.scale)
    }

    /// Once a frame: a held finger starts mining, held-back clicks release, and a screen that opened over the
    /// game lets go of every finger on it.
    pub(super) fn touch_frame(&mut self, event_loop: &dyn EventLoopCtl) {
        self.touch.hardware.refresh(
            horizon_gamecontroller::keyboard_connected(),
            horizon_gamecontroller::mouse_connected(),
        );
        if !self.touch_shown() {
            return;
        }
        let playing = self.touch_playing();
        if playing {
            self.touch_release_menu(event_loop);
        } else {
            self.touch_menu_frame(event_loop);
        }
        let out = if playing {
            let now = self.touch_now();
            self.touch.fingers.frame(now)
        } else {
            self.touch.fingers.release_all()
        };
        let ticks = self.ticks;
        let (due, keep) = std::mem::take(&mut self.touch.late)
            .into_iter()
            .partition(|(_, at)| ticks > *at || !playing);
        self.touch.late = keep;
        let scale = self.touch_batch().map_or(1.0, |b| b.scale);
        let result = due
            .into_iter()
            .try_for_each(|(button, _)| self.play_mouse(button, false))
            .and_then(|()| self.touch_act(out, scale));
        if let Err(e) = result {
            log::warn!("Horizon: touch: {e}");
        }
    }

    /// Mouse look and clicks only count with the pointer grabbed, which a touch screen has no pointer for:
    /// grab it as a click into the window would, or act as an agent's pointer where it cannot be grabbed.
    fn touch_grab(&mut self) {
        // Only when neither holds: a grab that fails (iOS has no pointer to grab) would otherwise be retried
        // on every drag, and each try makes the mouse drop its next move (`grabMouse`'s ignoreFirstMove).
        if self.captured || self.agent_pointer {
            return;
        }
        self.capture(true);
        if !self.captured {
            self.agent_pointer = true;
        }
    }

    /// The held item is used for as long as the button stays down: something eaten or drunk (its
    /// `consumable` component), or a bow, crossbow, trident, shield, spyglass, goat horn or brush.
    fn touch_holds_use(&self) -> bool {
        let Some(stack) = self.held() else {
            return false;
        };
        matches!(
            &*stack.item,
            "minecraft:bow"
                | "minecraft:crossbow"
                | "minecraft:trident"
                | "minecraft:shield"
                | "minecraft:spyglass"
                | "minecraft:goat_horn"
                | "minecraft:brush"
        ) || self
            .res
            .items()
            .get(&stack.item)
            .is_some_and(|info| info.consumable.is_some())
    }

    fn touch_act(&mut self, out: Vec<Out>, scale: f32) -> Result<()> {
        for o in out {
            match o {
                Out::Press(Control::Hotbar(slot)) => self.selected = usize::from(slot),
                Out::Press(Control::Sneak) => {
                    // A toggle either way: the crouch option toggles a press, otherwise flip the held key
                    // (as a controller's stick click does).
                    if self.settings.toggle_crouch {
                        self.mapped_key(KeyCode::ShiftLeft, true, false, KeyCode::ShiftLeft)?;
                        self.mapped_key(KeyCode::ShiftLeft, false, false, KeyCode::ShiftLeft)?;
                    } else {
                        let sneaking = self.keys.contains(&KeyCode::ShiftLeft);
                        self.mapped_key(KeyCode::ShiftLeft, !sneaking, false, KeyCode::ShiftLeft)?;
                    }
                }
                Out::Press(control) | Out::Release(control) => {
                    if let Some(key) = key_of(control) {
                        // Touch supplies a default action directly, with no physical-key rebinding.
                        self.mapped_key(key, matches!(o, Out::Press(_)), false, key)?;
                    }
                }
                Out::Look(dx, dy) => {
                    self.touch_grab();
                    let (dx, dy) = (f64::from(dx * scale), f64::from(dy * scale));
                    // Relative motion, as an agent's `mouse-move` adds it: no pointer position to compare with.
                    self.mouse.add_motion(dx, dy);
                }
                // Pocket Edition: a tap hits the entity it is on, and uses the item or block otherwise.
                Out::Tap => {
                    self.touch_grab();
                    let button = if self.entity_targeted() {
                        MouseButton::Left
                    } else {
                        MouseButton::Right
                    };
                    self.play_mouse(button, true)?;
                    self.touch.late.push((button, self.ticks));
                }
                // Holding still: what the hand holds decides, as in Pocket Edition. Food, potions and the items
                // used for as long as they are held (a bow, a shield...) are used until the finger lifts;
                // anything else mines.
                Out::MineStart => {
                    self.touch_grab();
                    let button = if self.touch_holds_use() {
                        MouseButton::Right
                    } else {
                        MouseButton::Left
                    };
                    self.touch.holding = Some(button);
                    self.play_mouse(button, true)?;
                }
                Out::MineEnd => {
                    let button = self.touch.holding.take().unwrap_or(MouseButton::Left);
                    self.play_mouse(button, false)?;
                }
            }
        }
        Ok(())
    }

    // ---- menus: a finger is the mouse ----

    /// The ✕ over a screen with no Done or Back of its own: an inventory, a container, creative, chat, the
    /// statistics and advancements, a sign or a book. The title screens and the options have theirs.
    fn touch_close_shown(&self) -> bool {
        self.touch_shown()
            && self.title.is_none()
            && (self.sign_edit.is_some()
                || self.book.is_some()
                || !matches!(
                    self.screen,
                    Screen::Playing | Screen::Paused | Screen::Dead | Screen::Win | Screen::Options
                ))
    }

    fn touch_menu(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        id: u64,
        phase: TouchPhase,
        at: (f32, f32),
    ) {
        match phase {
            TouchPhase::Started => {
                if self.touch.menu.is_some() {
                    return;
                }
                let on_close = self.touch_close_shown()
                    && self
                        .touch_batch()
                        .is_some_and(|b| touch::hit_rect(touch::close_rect(b.width, b.height), at));
                let on_keyboard = !on_close
                    && self.touch_shown()
                    && self.field_focused()
                    && self.touch_batch().is_some_and(|b| {
                        touch::hit_rect(touch::keyboard_rect(b.width, b.height), at)
                    });
                if on_close {
                    // What Escape does where the finger is (the pad's B).
                    if let Err(e) = self.pad_back() {
                        log::warn!("Horizon: touch close: {e}");
                    }
                } else if on_keyboard {
                    self.toggle_system_keyboard();
                } else {
                    // A finger outside the field the keyboard belongs to puts the system's keyboard away, as
                    // it would on any phone (and one on the field brings it back after the button hid it);
                    // the tap still does what it was on.
                    match self.focused_field_rect() {
                        Some(rect) if touch::hit_rect(rect, self.screen_point(at)) => {
                            if self.system_keyboard_hidden() {
                                self.show_system_keyboard();
                            }
                        }
                        Some(_) if self.system_keyboard_up() => self.hide_system_keyboard(),
                        _ => {}
                    }
                    self.touch_pointer(event_loop, at);
                }
                self.touch.menu = Some(MenuFinger {
                    id,
                    scrolled: at,
                    frames: 0,
                    pressed: false,
                    lifted: false,
                    closed: on_close || on_keyboard,
                });
            }
            TouchPhase::Moved => {
                let Some(finger) = self.touch.menu.as_ref().filter(|f| f.id == id && !f.closed)
                else {
                    return;
                };
                let last = finger.scrolled;
                self.touch_pointer(event_loop, at);
                // A drag scrolls lists as a wheel would, the content following the finger. Not in an
                // inventory, where a drag carries the item it pressed on.
                let lines = ((at.1 - last.1) / SCROLL_LINE).trunc();
                let scrolls = self.screen != Screen::Inventory;
                if let Some(f) = self.touch.menu.as_mut() {
                    f.scrolled = (at.0, last.1 + lines * SCROLL_LINE);
                }
                if scrolls && lines != 0.0 {
                    self.touch_wheel(event_loop, lines);
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                let Some(finger) = self.touch.menu.as_mut().filter(|f| f.id == id) else {
                    return;
                };
                finger.lifted = true;
                if !finger.closed {
                    self.touch_pointer(event_loop, at);
                }
            }
        }
    }

    /// Abandon a menu gesture when another input device or the world takes over.
    /// A missing UIKit end event must neither latch all later fingers nor create a click.
    fn touch_release_menu(&mut self, event_loop: &dyn EventLoopCtl) {
        if let Some(finger) = self.touch.menu.take()
            && finger.pressed
            && !finger.closed
        {
            self.touch_button(event_loop, false);
        }
    }

    /// Advance the delayed menu press/release after drawing the pointer.
    fn touch_menu_frame(&mut self, event_loop: &dyn EventLoopCtl) {
        let Some(finger) = self.touch.menu.as_mut() else {
            return;
        };
        finger.frames = finger.frames.saturating_add(1);
        if finger.closed {
            if finger.lifted {
                self.touch.menu = None;
            }
        } else if !finger.pressed {
            if finger.frames >= 2 {
                finger.pressed = true;
                self.touch_button(event_loop, true);
            }
        } else if finger.lifted {
            self.touch.menu = None;
            self.touch_button(event_loop, false);
        }
    }

    pub(super) fn touch_synthetic_input(&self) -> bool {
        self.touch.synthetic
    }

    fn touch_window_event(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        id: WindowId,
        event: WindowEvent,
    ) {
        let previous = std::mem::replace(&mut self.touch.synthetic, true);
        self.handle_window_event(event_loop, id, event, super::input_trace::InputOrigin::Game);
        self.touch.synthetic = previous;
    }

    fn touch_pointer(&mut self, event_loop: &dyn EventLoopCtl, at: (f32, f32)) {
        let (Some(id), Some(batch)) = (self.window.as_ref().map(|w| w.id()), self.screen_batch())
        else {
            return;
        };
        let at = self.screen_point(at);
        let position =
            PhysicalPosition::new(f64::from(at.0 * batch.scale), f64::from(at.1 * batch.scale));
        self.touch_window_event(
            event_loop,
            id,
            WindowEvent::CursorMoved {
                device_id: DeviceId::dummy(),
                position,
            },
        );
    }

    fn touch_button(&mut self, event_loop: &dyn EventLoopCtl, pressed: bool) {
        let Some(id) = self.window.as_ref().map(|w| w.id()) else {
            return;
        };
        let state = if pressed {
            ElementState::Pressed
        } else {
            ElementState::Released
        };
        self.touch_window_event(
            event_loop,
            id,
            WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                state,
                button: MouseButton::Left,
            },
        );
    }

    fn touch_wheel(&mut self, event_loop: &dyn EventLoopCtl, lines: f32) {
        let Some(id) = self.window.as_ref().map(|w| w.id()) else {
            return;
        };
        self.touch_window_event(
            event_loop,
            id,
            WindowEvent::MouseWheel {
                device_id: DeviceId::dummy(),
                delta: MouseScrollDelta::LineDelta(0.0, lines),
                phase: TouchPhase::Moved,
            },
        );
    }

    // ---- drawing ----

    /// The buttons over the HUD while playing: vanilla's button, a little see-through as Pocket Edition's
    /// were, lit while held, with white marks shadowed like Minecraft's text.
    pub(super) fn draw_touch_controls(&self, batch: &mut HudBatch) {
        if !self.touch_shown()
            || self.hardware_hides_touch()
            || !self.touch_playing()
            || self.touch_on_phone()
        {
            return;
        }
        let atlas = &self.gui.atlas;
        for (control, r) in touch::layout(batch.width, batch.height) {
            if matches!(control, Control::Hotbar(_)) {
                continue;
            }
            touch_button(batch, atlas, r, self.touch.fingers.held(control));
            shadowed(r, |r, color| glyph(batch, atlas, control, r, color));
        }
    }
}

impl Game<'_> {
    /// The buttons over a screen: ✕ where the screen has no way out of its own on a touch screen
    /// (`touch_close_shown`), and ⌨ beside it while a text field has focus.
    pub(super) fn draw_touch_close(&mut self, batch: &mut HudBatch) {
        if self.hardware_hides_touch() || self.touch_on_phone() {
            return;
        }
        let held = self.touch.menu.as_ref().is_some_and(|f| f.closed);
        let keyboard = self.touch_shown() && self.field_focused();
        let atlas = &self.gui.atlas;
        if self.touch_close_shown() {
            let r = touch::close_rect(batch.width, batch.height);
            touch_button(batch, atlas, r, held);
            shadowed(r, |r, color| {
                // Two diagonals of cells.
                let u = cell_size(r);
                let (cx, cy) = ((r[0] + r[2] / 2.0).floor(), (r[1] + r[3] / 2.0).floor());
                for i in -3..=3 {
                    let d = i as f32 * u;
                    batch.fill(atlas, cx + d - u / 2.0, cy + d - u / 2.0, u, u, color);
                    batch.fill(atlas, cx - d - u / 2.0, cy + d - u / 2.0, u, u, color);
                }
            });
        }
        if keyboard {
            let r = touch::keyboard_rect(batch.width, batch.height);
            touch_button(batch, atlas, r, false);
            shadowed(r, |r, color| {
                // A keyboard: two rows of keys over a space bar.
                let u = cell_size(r) * 0.75;
                let (cx, cy) = ((r[0] + r[2] / 2.0).floor(), (r[1] + r[3] / 2.0).floor());
                for row in 0..2 {
                    for key in 0..5 {
                        let x = cx + (key as f32 - 2.5) * u * 1.6;
                        let y = cy + (row as f32 - 1.5) * u * 1.6;
                        batch.fill(atlas, x, y, u, u, color);
                    }
                }
                batch.fill(atlas, cx - 2.5 * u, cy + 1.7 * u, 5.0 * u, u, color);
            });
        }
    }
}

impl Game<'_> {
    /// What the phone shows when it is splitscreen's controller (`touch_on_phone`): the buttons that are
    /// otherwise drawn over the game, in the phone window's pixels.
    pub(super) fn phone_buttons(&mut self) -> Vec<horizon_airplay::Button> {
        let Some(batch) = self.touch_batch().filter(|_| self.touch_shown()) else {
            return Vec::new();
        };
        let focused = self.field_focused();
        let px = |r: touch::Rect| r.map(|v| v * batch.scale);
        let mut buttons = Vec::new();
        if self.touch_playing() {
            for (control, r) in touch::layout(batch.width, batch.height) {
                let label = match control {
                    Control::Forward => "▲".to_owned(),
                    Control::Back => "▼".to_owned(),
                    Control::Left => "◀".to_owned(),
                    Control::Right => "▶".to_owned(),
                    Control::Sneak => "Sneak".to_owned(),
                    Control::Jump => "Jump".to_owned(),
                    Control::Perspective => "View".to_owned(),
                    Control::Chat => "Chat".to_owned(),
                    Control::Pause => "II".to_owned(),
                    Control::Inventory => "•••".to_owned(),
                    Control::Hotbar(slot) => (slot + 1).to_string(),
                };
                buttons.push(horizon_airplay::Button {
                    label,
                    rect: px(r),
                    held: self.touch.fingers.held(control),
                });
            }
        } else {
            if self.touch_close_shown() {
                buttons.push(horizon_airplay::Button {
                    label: "✕".to_owned(),
                    rect: px(touch::close_rect(batch.width, batch.height)),
                    held: self.touch.menu.as_ref().is_some_and(|f| f.closed),
                });
            }
            if focused {
                buttons.push(horizon_airplay::Button {
                    label: "⌨".to_owned(),
                    rect: px(touch::keyboard_rect(batch.width, batch.height)),
                    held: false,
                });
            }
        }
        buttons
    }
}

/// Vanilla's button sprite, nine-sliced to the touch button's size: highlighted while held, and see-through
/// enough to keep the world behind it in sight.
fn touch_button(
    batch: &mut HudBatch,
    atlas: &horizon_client::hud::HudAtlas,
    r: touch::Rect,
    held: bool,
) {
    let sprite = if held {
        "gui/sprites/widget/button_highlighted"
    } else {
        "gui/sprites/widget/button"
    };
    let alpha = if held { 230 } else { 170 };
    horizon_client::ui::nine_slice(
        batch,
        atlas,
        sprite,
        r[0],
        r[1],
        r[2],
        r[3],
        rgba(255, 255, 255, alpha),
    );
}

/// The size of a mark's cell on a button `r`: two GUI pixels on the old 26-pixel buttons, growing with them.
fn cell_size(r: touch::Rect) -> f32 {
    (r[2] / 13.0).round().max(2.0)
}

/// Draw a mark twice, as Minecraft draws text: a dark copy a little down and right, then the white one.
fn shadowed(r: touch::Rect, mut mark: impl FnMut(touch::Rect, u32)) {
    let off = (cell_size(r) / 2.0).max(1.0);
    mark([r[0] + off, r[1] + off, r[2], r[3]], rgba(63, 63, 63, 230));
    mark(r, rgba(255, 255, 255, 240));
}

/// A button's mark, in cells of a few pixels centred on it.
fn glyph(
    batch: &mut HudBatch,
    atlas: &horizon_client::hud::HudAtlas,
    control: Control,
    r: touch::Rect,
    color: u32,
) {
    let (cx, cy) = ((r[0] + r[2] / 2.0).floor(), (r[1] + r[3] / 2.0).floor());
    let u = cell_size(r);
    let cell = |batch: &mut HudBatch, x: f32, y: f32, w: f32, h: f32| {
        batch.fill(atlas, cx + x * u, cy + y * u, w * u, h * u, color);
    };
    // A triangle pointing up, down, left or right: rows (or columns) narrowing to its tip.
    let arrow = |batch: &mut HudBatch, dir: (f32, f32)| {
        for i in 0..4 {
            let i = i as f32;
            let len = 7.0 - 2.0 * i;
            let at = 1.5 - i;
            match dir {
                (0.0, d) => cell(batch, -len / 2.0, at * -d - 0.5, len, 1.0),
                (d, _) => cell(batch, at * -d - 0.5, -len / 2.0, 1.0, len),
            }
        }
    };
    match control {
        Control::Forward => arrow(batch, (0.0, -1.0)),
        Control::Back => arrow(batch, (0.0, 1.0)),
        Control::Left => arrow(batch, (-1.0, 0.0)),
        Control::Right => arrow(batch, (1.0, 0.0)),
        // Sneak: a hollow diamond; jump: a filled one.
        Control::Sneak | Control::Jump => {
            for i in 0..4 {
                let i = i as f32;
                let len = 7.0 - 2.0 * i;
                for y in [-i - 0.5, i - 0.5] {
                    if control == Control::Jump {
                        cell(batch, -len / 2.0, y, len, 1.0);
                    } else {
                        cell(batch, -len / 2.0, y, 1.0, 1.0);
                        cell(batch, len / 2.0 - 1.0, y, 1.0, 1.0);
                    }
                }
            }
        }
        // Perspective: an eye.
        Control::Perspective => {
            cell(batch, -2.5, -1.5, 5.0, 1.0);
            cell(batch, -3.5, -0.5, 1.0, 1.0);
            cell(batch, -0.5, -0.5, 1.0, 1.0);
            cell(batch, 2.5, -0.5, 1.0, 1.0);
            cell(batch, -2.5, 0.5, 5.0, 1.0);
        }
        // Chat: a speech bubble with its tail.
        Control::Chat => {
            cell(batch, -3.0, -2.5, 6.0, 4.0);
            cell(batch, -2.0, 1.5, 1.0, 1.0);
        }
        // Pause: two bars.
        Control::Pause => {
            cell(batch, -2.0, -2.5, 1.5, 5.0);
            cell(batch, 0.5, -2.5, 1.5, 5.0);
        }
        // The inventory: three dots, as Pocket Edition's button beside the hotbar.
        Control::Inventory => {
            for x in [-3.0, -0.5, 2.0] {
                cell(batch, x, -0.5, 1.0, 1.0);
            }
        }
        Control::Hotbar(_) => {}
    }
}
