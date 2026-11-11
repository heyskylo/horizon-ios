//! Native touch input: relative menu cursor or direct-touch menus, independent
//! mouse buttons and D-pad/joystick keyboard emulation while playing. Layout
//! geometry and finger ownership live in the reusable touch module.
use super::event_loop::EventLoopCtl;
use super::{Game, Screen};
use horizon_client::{
    Result,
    held::Source,
    hud::{HudBatch, rgba},
    touch::{self, Control, MenuAction, MenuGesture, MenuMode, Out, Touches},
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

#[derive(Default)]
pub(super) struct TouchInput {
    #[cfg(target_os = "ios")]
    cursor: touch::desktop::Cursor,
    #[cfg(target_os = "ios")]
    cursor_screen: Option<u64>,
    #[cfg(target_os = "ios")]
    cursor_pending: Vec<CursorEdge>,
    fingers: Touches,
    more: bool,
    hidden: bool,
    toggle_keys: Vec<KeyCode>,
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
    #[cfg(target_os = "ios")]
    cursor_down: [bool; 3],
    #[cfg(target_os = "ios")]
    cursor_cancelled: [bool; 3],
    #[cfg(target_os = "ios")]
    cursor_delivery: Option<ClickContext>,
    #[cfg(target_os = "ios")]
    cursor_drawn: Option<(u64, (f32, f32))>,
    #[cfg(target_os = "ios")]
    click_contexts: Vec<(u64, ClickContext)>,
    #[cfg(target_os = "ios")]
    native_surface: Option<(u64, (f32, f32))>,
    #[cfg(target_os = "ios")]
    cursor_keys: Vec<Control>,
    #[cfg(target_os = "ios")]
    native_pending: Vec<(touch::desktop::Physical, f64, u64)>,
    #[cfg(target_os = "ios")]
    native_keys: Vec<KeyCode>,
    #[cfg(target_os = "ios")]
    native_mouse: [bool; 2],
    #[cfg(target_os = "ios")]
    native_screen: Option<u64>,
    clock: Option<Instant>,
}

#[cfg(target_os = "ios")]
#[derive(Clone, Debug, PartialEq)]
struct ClickContext {
    at: (f32, f32),
    screen_at: (f32, f32),
    size: (f32, f32),
    screen: u64,
    modifiers: winit::keyboard::ModifiersState,
    touch_shift: bool,
    guard: horizon_client::widgets::PointerGuard,
    surface: Option<(u64, (f32, f32))>,
}
#[cfg(target_os = "ios")]
struct CursorEdge {
    button: Option<(MouseButton, bool)>,
    due: u64,
    context: ClickContext,
}

/// A finger over a menu. Its press waits for a frame drawn with the pointer under it, and its release for a
/// frame after the press: screens find what is under the pointer while drawing, so a pointer that moved and
/// pressed in one go would press whatever was under the old position (or nothing).
struct MenuFinger {
    id: u64,
    gesture: MenuGesture,
    target: MenuTarget,
    at: (f32, f32),
    guard: Option<horizon_client::widgets::PointerGuard>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuTarget {
    Pointer,
    Close,
    Keyboard,
}

/// The key a button stands for: the vanilla default of its mapping.
fn key_of(control: Control) -> Option<KeyCode> {
    Some(match control {
        Control::Forward => KeyCode::KeyW,
        Control::Back => KeyCode::KeyS,
        Control::Left => KeyCode::KeyA,
        Control::Right => KeyCode::KeyD,
        Control::Jump => KeyCode::Space,
        Control::Sneak => KeyCode::ShiftLeft,
        Control::Perspective => KeyCode::F5,
        Control::Chat => KeyCode::KeyT,
        Control::Pause => KeyCode::Escape,
        Control::Inventory => KeyCode::KeyE,
        Control::Sprint => KeyCode::ControlLeft,
        Control::Debug => KeyCode::F3,
        Control::PlayerList => KeyCode::Tab,
        Control::Runtime => KeyCode::Escape,
        Control::More | Control::Hide | Control::MiddleClick => return None,
        Control::Hotbar(_) | Control::LeftClick | Control::RightClick | Control::Joystick => {
            return None;
        }
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
        !self.menu_input_blocked()
            && self.title.is_none()
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
        #[cfg(target_os = "ios")]
        if !self.touch_playing() {
            self.touch_native_handoff();
        }
        if horizon_gamecontroller::keyboard_connected() {
            self.touch.hardware.keyboard();
        }
    }

    pub(super) fn touch_mouse_used(&mut self, event_loop: &dyn EventLoopCtl) {
        self.touch_release_menu(event_loop);
        #[cfg(target_os = "ios")]
        if !self.touch_playing() {
            self.touch_native_handoff();
        }
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
        if self.menu_input_blocked() {
            self.cancel_touch_for_reload();
            return Ok(());
        }
        self.touch.seen = true;
        self.touch.hardware.touch();
        self.last_input = Instant::now();
        let Some(batch) = self.touch_batch() else {
            return Ok(());
        };
        // A real finger owns input just as a hardware pointer does. Menu
        // touches also pass through key_focus_mouse; world controls do not.
        // Releases/cancellations only finish that finger's own actions.
        if matches!(touch.phase, TouchPhase::Started | TouchPhase::Moved) {
            self.pad_yield_input(event_loop);
        }
        let at = (
            touch.location.x as f32 / batch.scale,
            touch.location.y as f32 / batch.scale,
        );
        let playing = self.touch_playing();
        #[cfg(target_os = "ios")]
        if !playing && (touch::cursor_mode() || horizon_ios_launcher::native_gestures_available()) {
            let auxiliary = self
                .touch
                .menu
                .as_ref()
                .is_some_and(|finger| finger.id == touch.id)
                || (touch.phase == TouchPhase::Started
                    && ((self.touch_close_shown()
                        && touch::hit_rect(touch::close_rect(batch.width, batch.height), at))
                        || (self.field_focused()
                            && touch::hit_rect(
                                touch::keyboard_rect(batch.width, batch.height),
                                at,
                            ))));
            if auxiliary {
                self.touch_menu(event_loop, touch.id, touch.phase, at);
                return Ok(());
            }
            self.touch_cursor_event(
                event_loop,
                touch.id,
                touch.phase,
                at,
                (batch.width, batch.height),
            );
            return Ok(());
        }
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
        #[cfg(target_os = "ios")]
        {
            self.touch.fingers.direct_look = horizon_ios_launcher::native_gestures_available();
            self.touch.fingers.capture_buttons = true;
        }
        let layout = self.visible_touch_layout(batch.width, batch.height);
        self.touch.fingers.forward_lock = touch::forward_lock();
        #[cfg(target_os = "ios")]
        let layout: Vec<_> = layout
            .into_iter()
            .filter(|(control, _)| {
                touch::hotbar_gestures() || !matches!(control, Control::Hotbar(_))
            })
            .map(|(control, rect)| {
                if let Control::Hotbar(slot) = control {
                    let physical = self.window.as_ref().map(|w| w.inner_size());
                    let width =
                        physical.map_or(batch.width * batch.scale, |size| size.width as f32);
                    let height =
                        physical.map_or(batch.height * batch.scale, |size| size.height as f32);
                    let left =
                        (width as i32 / 2 - (180.0 * batch.scale) as i32 / 2) as f32 / batch.scale;
                    (
                        control,
                        [
                            left + f32::from(slot) * 20.0,
                            height / batch.scale - 20.0,
                            20.0,
                            20.0,
                        ],
                    )
                } else {
                    (control, rect)
                }
            })
            .collect();
        let out = match touch.phase {
            TouchPhase::Started => {
                let now = self.touch_now();
                self.touch.fingers.start(touch.id, at, now, &layout)
            }
            TouchPhase::Moved => self.touch.fingers.moved(touch.id, at, &layout),
            TouchPhase::Ended => self.touch.fingers.end(touch.id),
            TouchPhase::Cancelled => self.touch.fingers.cancel(touch.id),
        };
        let result = self.touch_act(out, batch.scale);
        #[cfg(target_os = "ios")]
        self.touch_record_click_context();
        result
    }

    /// Drop all pending fingers and deferred clicks at the reload boundary.
    /// No release is routed through a possibly replaced screen; the boundary
    /// already released gameplay holds and the screen's drag state.
    pub(super) fn cancel_touch_for_reload(&mut self) {
        #[cfg(target_os = "ios")]
        {
            self.touch_cursor_reset();
            self.touch_native_reset();
            horizon_ios_launcher::touch_gestures(0, 2, (0.0, 0.0), 1.0);
        }
        self.touch.menu = None;
        self.touch.fingers.release_all();
        self.touch.late.clear();
        self.touch.holding = None;
        self.keys.release_source(Source::Touch);
        self.touch.toggle_keys.clear();
    }

    /// Once a frame: a held finger starts mining, held-back clicks release, and a screen that opened over the
    /// game lets go of every finger on it.
    pub(super) fn touch_frame(&mut self, event_loop: &dyn EventLoopCtl) {
        #[cfg(target_os = "ios")]
        self.touch_cursor_flush(event_loop);
        if self.menu_input_blocked() {
            self.cancel_touch_for_reload();
            return;
        }
        self.touch.hardware.refresh(
            horizon_gamecontroller::keyboard_connected(),
            horizon_gamecontroller::mouse_connected(),
        );
        if !self.touch_shown() {
            #[cfg(target_os = "ios")]
            {
                self.touch_native_reset();
                horizon_ios_launcher::touch_gestures(0, 2, (0.0, 0.0), 1.0);
            }
            return;
        }
        #[cfg(target_os = "ios")]
        self.touch_native_frame(event_loop);
        let playing = self.touch_playing();
        #[cfg(target_os = "ios")]
        self.touch_cursor_frame(event_loop, playing);
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
        if !playing {
            for key in std::mem::take(&mut self.touch.toggle_keys) {
                let _ = self.mapped_key(key, false, false, key, Source::Touch);
            }
        }
        let ticks = self.ticks;
        let (due, keep) = std::mem::take(&mut self.touch.late)
            .into_iter()
            .partition(|(_, at)| ticks > *at || !playing);
        self.touch.late = keep;
        let scale = self.touch_batch().map_or(1.0, |b| b.scale);
        let result = due
            .into_iter()
            .try_for_each(|(button, _)| self.mouse_mapping(button, false, Source::Touch))
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

    pub(super) fn touch_shift_held(&self) -> bool {
        #[cfg(target_os = "ios")]
        {
            self.touch.cursor_delivery.as_ref().map_or_else(
                || {
                    self.touch.fingers.held(Control::Sneak)
                        || self.touch.cursor_keys.contains(&Control::Sneak)
                },
                |context| context.touch_shift,
            )
        }
        #[cfg(not(target_os = "ios"))]
        {
            false
        }
    }

    /// The held item is used for as long as the button stays down: something eaten or drunk (its
    /// `consumable` component), or a bow, crossbow, trident, shield, spyglass, goat horn or brush.
    /// Live touch sources for the shared attack/use mapping. Short taps remain
    /// held until their delayed release; a long press lives in `holding`.
    pub(super) fn touch_holds_mouse_mapping(&self, button: MouseButton) -> bool {
        #[cfg(target_os = "ios")]
        let explicit = self.touch_raw_holds_mouse_mapping(button);
        #[cfg(not(target_os = "ios"))]
        let explicit = match button {
            MouseButton::Left => self.touch.fingers.held(Control::LeftClick),
            MouseButton::Right => self.touch.fingers.held(Control::RightClick),
            _ => false,
        };
        explicit
            || self.touch.holding == Some(button)
            || self.touch.late.iter().any(|(held, _)| *held == button)
    }

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

    #[cfg(target_os = "ios")]
    fn touch_raw_key(&mut self, key: KeyCode, pressed: bool) -> Result<()> {
        for mapped in self.settings.keys.translate_all(key) {
            match mapped {
                Ok(mapped) => self.mapped_key(mapped, pressed, false, key, Source::Touch)?,
                Err(button) if pressed || !self.touch_holds_mouse_mapping(button) => {
                    self.mouse_mapping(button, pressed, Source::Touch)?
                }
                Err(_) => {}
            }
        }
        Ok(())
    }

    #[cfg(target_os = "ios")]
    fn touch_raw_button(&mut self, button: MouseButton, pressed: bool) -> Result<()> {
        let input = horizon_client::keybinds::mouse_name(button);
        for mapped in self.settings.keys.translate_mouse_all(button) {
            match mapped {
                Err(mapped) if self.shared_default_acts(mapped, Some(input.as_str())) => {
                    if pressed || !self.touch_holds_mouse_mapping(mapped) {
                        self.mouse_mapping(mapped, pressed, Source::Touch)?;
                    }
                }
                Err(_) => {}
                Ok(key) => self.mapped_key(key, pressed, false, key, Source::Touch)?,
            }
        }
        Ok(())
    }

    fn touch_act(&mut self, out: Vec<Out>, scale: f32) -> Result<()> {
        if self.menu_input_blocked() {
            self.cancel_touch_for_reload();
            return Ok(());
        }
        for o in out {
            // UIKit recognizes tap/long-press actions in a current bundle.
            // Do not also dispatch the fallback Rust recognizer's outputs.
            #[cfg(target_os = "ios")]
            if horizon_ios_launcher::native_gestures_available()
                && matches!(o, Out::Tap | Out::MineStart | Out::MineEnd)
            {
                continue;
            }
            match o {
                Out::Press(Control::More) => self.touch.more = !self.touch.more,
                Out::Press(Control::Hide) => {
                    self.touch.hidden = !self.touch.hidden;
                    let released = self.touch.fingers.release_all();
                    self.touch_act(released, scale)?;
                    for key in std::mem::take(&mut self.touch.toggle_keys) {
                        self.mapped_key(key, false, false, key, Source::Touch)?;
                    }
                }
                Out::Press(control @ (Control::Sprint | Control::Sneak)) => {
                    let key = if control == Control::Sneak {
                        KeyCode::ShiftLeft
                    } else {
                        KeyCode::ControlLeft
                    };
                    let option = if control == Control::Sneak {
                        self.settings.toggle_crouch
                    } else {
                        self.settings.toggle_sprint
                    };
                    if option {
                        self.mapped_key(key, true, false, key, Source::Touch)?;
                        self.mapped_key(key, false, false, key, Source::Touch)?;
                        continue;
                    }
                    let down = !self.touch.toggle_keys.contains(&key);
                    if down {
                        self.touch.toggle_keys.push(key);
                    } else {
                        self.touch.toggle_keys.retain(|k| *k != key);
                    }
                    self.mapped_key(key, down, false, key, Source::Touch)?;
                }
                Out::Release(Control::Sprint | Control::Sneak) => {}
                #[cfg(target_os = "ios")]
                Out::Press(Control::Hotbar(_))
                    if horizon_ios_launcher::native_gestures_available() => {}
                Out::Press(Control::Hotbar(slot)) => self.selected = usize::from(slot),
                Out::Press(Control::LeftClick | Control::RightClick | Control::MiddleClick)
                | Out::Release(Control::LeftClick | Control::RightClick | Control::MiddleClick) => {
                    let (control, down) = match o {
                        Out::Press(c) => (c, true),
                        Out::Release(c) => (c, false),
                        _ => unreachable!(),
                    };
                    if down {
                        self.touch_grab();
                    }
                    #[cfg(target_os = "ios")]
                    self.touch_raw_button(
                        if control == Control::MiddleClick {
                            MouseButton::Middle
                        } else if control == Control::LeftClick {
                            MouseButton::Left
                        } else {
                            MouseButton::Right
                        },
                        down,
                    )?;
                    #[cfg(not(target_os = "ios"))]
                    self.mouse_mapping(
                        if control == Control::MiddleClick {
                            MouseButton::Middle
                        } else if control == Control::LeftClick {
                            MouseButton::Left
                        } else {
                            MouseButton::Right
                        },
                        down,
                        Source::Touch,
                    )?;
                }
                Out::Press(control) | Out::Release(control) => {
                    if let Some(key) = key_of(control) {
                        #[cfg(target_os = "ios")]
                        self.touch_raw_key(key, matches!(o, Out::Press(_)))?;
                        #[cfg(not(target_os = "ios"))]
                        self.mapped_key(
                            key,
                            matches!(o, Out::Press(_)),
                            false,
                            key,
                            Source::Touch,
                        )?;
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
                    self.mouse_mapping(button, true, Source::Touch)?;
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
                    self.mouse_mapping(button, true, Source::Touch)?;
                }
                Out::MineEnd => {
                    let button = self.touch.holding.take().unwrap_or(MouseButton::Left);
                    self.mouse_mapping(button, false, Source::Touch)?;
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
        if !at.0.is_finite() || !at.1.is_finite() {
            if self
                .touch
                .menu
                .as_ref()
                .is_some_and(|finger| finger.id == id)
            {
                self.touch_release_menu(event_loop);
            }
            return;
        }
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
                let target = if on_close {
                    MenuTarget::Close
                } else if on_keyboard {
                    MenuTarget::Keyboard
                } else {
                    MenuTarget::Pointer
                };
                let mode = if target == MenuTarget::Pointer {
                    self.touch_menu_mode(at)
                } else {
                    MenuMode::Tap
                };
                if target == MenuTarget::Pointer {
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
                    gesture: MenuGesture::new(at, mode),
                    target,
                    at,
                    guard: (mode == MenuMode::Tap)
                        .then(|| horizon_client::widgets::capture_pointer(self.screen_point(at))),
                });
            }
            TouchPhase::Moved | TouchPhase::Ended => {
                let Some(finger) = self.touch.menu.as_mut().filter(|finger| finger.id == id) else {
                    return;
                };
                let target = finger.target;
                finger.at = at;
                let action = if phase == TouchPhase::Ended {
                    finger.gesture.end(at)
                } else {
                    finger.gesture.moved(at)
                };
                if target == MenuTarget::Pointer {
                    self.touch_pointer(event_loop, at);
                }
                self.touch_menu_action(event_loop, target, action);
            }
            TouchPhase::Cancelled => {
                if self
                    .touch
                    .menu
                    .as_ref()
                    .is_some_and(|finger| finger.id == id)
                {
                    self.touch_release_menu(event_loop);
                }
            }
        }
    }

    /// The same drawn targets the controller uses distinguish direct manipulation
    /// from a tap. Fields retain selection dragging; sliders and slots retain holds.
    fn touch_menu_mode(&self, at: (f32, f32)) -> MenuMode {
        if self.screen == Screen::Inventory
            || self.hud_editor_drag_target(self.screen_point(at))
            || horizon_client::widgets::last_targets()
                .iter()
                .rev()
                .any(|target| {
                    matches!(
                        target.kind,
                        horizon_client::widgets::Kind::Slider { .. }
                            | horizon_client::widgets::Kind::Field
                            | horizon_client::widgets::Kind::Slot
                    ) && touch::hit_rect(target.rect, self.screen_point(at))
                })
        {
            MenuMode::Drag
        } else {
            MenuMode::Tap
        }
    }

    fn touch_menu_action(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        target: MenuTarget,
        action: MenuAction,
    ) {
        if action.finished {
            self.touch.menu = None;
        }
        if action.scroll != 0.0 && target == MenuTarget::Pointer {
            self.touch_wheel(event_loop, action.scroll);
        }
        if let Some(pressed) = action.button {
            match (target, pressed) {
                (MenuTarget::Pointer, _) => self.touch_button(event_loop, pressed),
                (MenuTarget::Close, true) => {
                    if let Err(error) = self.pad_back() {
                        log::warn!("Horizon: touch close: {error}");
                    }
                }
                (MenuTarget::Keyboard, true) => self.toggle_system_keyboard(),
                _ => {}
            }
        }
    }

    /// Abandon a menu gesture when another input device or the world takes over.
    /// A missing UIKit end event must neither latch all later fingers nor create a click.
    fn touch_release_menu(&mut self, event_loop: &dyn EventLoopCtl) {
        #[cfg(target_os = "ios")]
        if self.touch.cursor_screen.is_some() {
            self.touch_cursor_reset();
            self.touch_cursor_flush(event_loop);
        }

        if let Some(mut finger) = self.touch.menu.take()
            && finger.gesture.cancel().button == Some(false)
            && finger.target == MenuTarget::Pointer
        {
            self.touch_button(event_loop, false);
        }
    }

    /// Advance the delayed menu press/release after drawing the pointer.
    fn touch_menu_frame(&mut self, event_loop: &dyn EventLoopCtl) {
        let Some(finger) = self.touch.menu.as_mut() else {
            return;
        };
        let target = finger.target;
        let at = finger.at;
        let action = finger.gesture.frame();
        if action.button == Some(true)
            && self
                .touch
                .menu
                .as_ref()
                .and_then(|finger| finger.guard.as_ref())
                .is_some_and(|guard| {
                    *guard != horizon_client::widgets::capture_pointer(self.screen_point(at))
                })
        {
            // No press was dispatched yet: suppress it without inventing a release.
            self.touch.menu = None;
            return;
        }
        self.touch_menu_action(event_loop, target, action);
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
        self.touch_mouse_button(event_loop, MouseButton::Left, pressed);
    }
    fn touch_mouse_button(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        button: MouseButton,
        pressed: bool,
    ) {
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
                button,
            },
        );
    }

    fn touch_wheel(&mut self, event_loop: &dyn EventLoopCtl, lines: f32) {
        self.touch_scroll(event_loop, 0.0, lines);
    }
    fn touch_scroll(&mut self, event_loop: &dyn EventLoopCtl, x: f32, y: f32) {
        let Some(id) = self.window.as_ref().map(|w| w.id()) else {
            return;
        };
        self.touch_window_event(
            event_loop,
            id,
            WindowEvent::MouseWheel {
                device_id: DeviceId::dummy(),
                delta: MouseScrollDelta::LineDelta(x, y),
                phase: TouchPhase::Moved,
            },
        );
    }

    // ---- drawing ----

    /// The buttons over the HUD while playing: vanilla's button, a little see-through as Pocket Edition's
    /// were, lit while held, with white marks shadowed like Minecraft's text.
    pub(super) fn draw_touch_controls(&self, batch: &mut HudBatch) {
        if !touch::controls_visible()
            || !self.touch_shown()
            || self.hardware_hides_touch()
            || !self.touch_playing()
            || self.touch_on_phone()
        {
            return;
        }
        let atlas = &self.gui.atlas;
        for (control, r) in self.visible_touch_layout(batch.width, batch.height) {
            if matches!(control, Control::Hotbar(_)) {
                continue;
            }
            draw_amethyst_control(
                batch,
                atlas,
                control,
                r,
                self.touch.fingers.held(control),
                Some(self.touch.fingers.stick_point(r)),
            );
            if control == Control::Joystick
                && touch::forward_lock()
                && self.touch.fingers.held(Control::Forward)
            {
                draw_lock(batch, atlas, touch::lock_rect(r), self.touch.fingers.locked);
            }
        }
    }
}

impl Game<'_> {
    /// The buttons over a screen: ✕ where the screen has no way out of its own on a touch screen
    /// (`touch_close_shown`), and ⌨ beside it while a text field has focus.
    pub(super) fn draw_touch_close(&mut self, batch: &mut HudBatch) {
        #[cfg(target_os = "ios")]
        {
            self.touch.cursor_drawn = Some((self.key_screen(), self.cursor));
        }
        if !touch::controls_visible() || self.hardware_hides_touch() || self.touch_on_phone() {
            return;
        }
        #[cfg(target_os = "ios")]
        if !self.touch_playing()
            && (touch::cursor_mode() || horizon_ios_launcher::native_gestures_available())
        {
            for (c, r) in self.visible_touch_layout(batch.width, batch.height) {
                if !matches!(c, Control::Hotbar(_)) {
                    let (x, y) = self.touch.cursor.joystick.thumb;
                    let point = if self.touch.cursor.joystick.velocity != (0.0, 0.0) {
                        (r[0] + x as f32, r[1] + y as f32)
                    } else {
                        (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0)
                    };
                    draw_amethyst_control(
                        batch,
                        &self.gui.atlas,
                        c,
                        r,
                        self.touch.cursor_keys.contains(&c),
                        Some(point),
                    );
                }
            }
            let at = self
                .touch
                .cursor
                .position
                .unwrap_or((batch.width / 2.0, batch.height / 2.0));
            for y in 0..if self.touch_menu_cursor_mode() { 9 } else { 0 } {
                batch.fill(
                    &self.gui.atlas,
                    at.0,
                    at.1 + y as f32,
                    (y as f32 * 0.55 + 1.0).ceil(),
                    1.0,
                    rgba(255, 255, 255, 255),
                );
            }
        }
        let held = self
            .touch
            .menu
            .as_ref()
            .is_some_and(|finger| finger.target == MenuTarget::Close);
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
            touch_button(
                batch,
                atlas,
                r,
                self.touch
                    .menu
                    .as_ref()
                    .is_some_and(|finger| finger.target == MenuTarget::Keyboard),
            );
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
        if !touch::controls_visible() {
            return Vec::new();
        }
        let focused = self.field_focused();
        let px = |r: touch::Rect| r.map(|v| v * batch.scale);
        let mut buttons = Vec::new();
        if self.touch_playing() {
            for (control, r) in self.visible_touch_layout(batch.width, batch.height) {
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
                    Control::LeftClick => "LMB".to_owned(),
                    Control::RightClick => "RMB".to_owned(),
                    Control::Joystick => "↕".to_owned(),
                    other => format!("{other:?}"),
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
                    held: self
                        .touch
                        .menu
                        .as_ref()
                        .is_some_and(|finger| finger.target == MenuTarget::Close),
                });
            }
            if focused {
                buttons.push(horizon_airplay::Button {
                    label: "⌨".to_owned(),
                    rect: px(touch::keyboard_rect(batch.width, batch.height)),
                    held: self
                        .touch
                        .menu
                        .as_ref()
                        .is_some_and(|finger| finger.target == MenuTarget::Keyboard),
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
#[cfg(target_os = "ios")]
impl Game<'_> {
    fn touch_click_context(&self) -> Option<ClickContext> {
        let batch = self.touch_batch()?;
        let at = self
            .touch
            .cursor
            .position
            .unwrap_or((batch.width / 2.0, batch.height / 2.0));
        let screen_at = self.screen_point(at);
        Some(ClickContext {
            at,
            screen_at,
            size: (batch.width, batch.height),
            screen: self.key_screen(),
            modifiers: self.modifiers,
            touch_shift: self.touch.fingers.held(Control::Sneak)
                || self.touch.cursor_keys.contains(&Control::Sneak),
            guard: horizon_client::widgets::capture_pointer(screen_at),
            surface: self.touch.native_surface,
        })
    }
    fn touch_record_click_context(&mut self) {
        let Some(context) = self.touch_click_context() else {
            return;
        };
        if let Some((id, last)) = self.touch.click_contexts.last()
            && *last == context
        {
            horizon_ios_launcher::set_touch_context(*id);
            return;
        }
        let id = horizon_ios_launcher::next_touch_context();
        if self.touch.click_contexts.len() >= 512 {
            self.touch.click_contexts.remove(0);
        }
        self.touch.click_contexts.push((id, context));
        horizon_ios_launcher::set_touch_context(id);
    }
    fn touch_cursor_flush(&mut self, event_loop: &dyn EventLoopCtl) {
        for (i, button) in [MouseButton::Left, MouseButton::Right, MouseButton::Middle]
            .into_iter()
            .enumerate()
        {
            if std::mem::take(&mut self.touch.cursor_cancelled[i]) {
                self.touch_mouse_button(event_loop, button, false);
            }
        }
    }
    fn touch_cursor_reset(&mut self) {
        self.touch.cursor.cancel();
        self.touch.cursor_pending.clear();
        self.touch.cursor_screen = None;
        self.touch.cursor_drawn = None;
        for control in std::mem::take(&mut self.touch.cursor_keys) {
            if let Some(key) = key_of(control) {
                let _ = self.touch_raw_key(key, false);
            }
        }
        for (i, button) in [MouseButton::Left, MouseButton::Right, MouseButton::Middle]
            .into_iter()
            .enumerate()
        {
            if std::mem::take(&mut self.touch.cursor_down[i]) {
                // The synthetic down entered the window ledger. Clear it now
                // even when no event loop is available (reload), then route
                // its paired menu/window up at the next dispatch opportunity.
                self.physical_buttons.remove(&button);
                self.touch.cursor_cancelled[i] = true;
            }
        }
    }
    fn touch_cursor_motion(&mut self, event_loop: &dyn EventLoopCtl, at: (f32, f32)) {
        if self
            .touch
            .cursor_pending
            .iter()
            .any(|edge| edge.button.is_some_and(|(_, down)| down))
        {
            if let Some(context) = self.touch_click_context() {
                let due = self
                    .touch
                    .cursor_pending
                    .last()
                    .map_or(self.frames, |edge| edge.due);
                self.touch.cursor_pending.push(CursorEdge {
                    button: None,
                    due,
                    context,
                });
            }
        } else {
            self.touch_pointer(event_loop, at);
        }
    }
    fn touch_cursor_event(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        id: u64,
        phase: TouchPhase,
        at: (f32, f32),
        size: (f32, f32),
    ) {
        if phase == TouchPhase::Cancelled && !self.touch.cursor.surface_finger(id) {
            self.touch_cursor_reset();
            self.touch_cursor_flush(event_loop);
            return;
        }
        if self.touch.cursor_screen != Some(self.key_screen()) {
            self.touch_cursor_reset();
            self.touch_cursor_flush(event_loop);
            self.touch.cursor_screen = Some(self.key_screen());
        }
        let surface = self.touch.cursor.primary_finger() == Some(id);
        let layout = self.visible_touch_layout(size.0, size.1);
        let phase = match phase {
            TouchPhase::Started => 0,
            TouchPhase::Moved => 1,
            TouchPhase::Ended => 2,
            TouchPhase::Cancelled => 3,
        };
        self.touch
            .cursor
            .set_virtual_mouse(self.touch_menu_cursor_mode());
        let events = self
            .touch
            .cursor
            .event_controls(id, phase, at, size, &layout);
        if surface || self.touch.cursor.primary_finger() == Some(id) {
            self.touch.native_surface = Some((id, at));
        }
        for event in events {
            match event {
                touch::desktop::Action::Move(x, y) => self.touch_cursor_motion(event_loop, (x, y)),
                touch::desktop::Action::Key(control, down) => {
                    if down {
                        if !self.touch.cursor_keys.contains(&control) {
                            self.touch.cursor_keys.push(control);
                        }
                    } else {
                        self.touch.cursor_keys.retain(|c| *c != control);
                    }
                    if control == Control::MiddleClick {
                        self.touch_queue_cursor_button(MouseButton::Middle, down);
                    }
                    if control == Control::More && down {
                        self.touch.more = !self.touch.more;
                    }
                    if control == Control::Hide && down {
                        self.touch.hidden = !self.touch.hidden;
                    }
                    if let Some(key) = key_of(control) {
                        let _ = self.touch_raw_key(key, down);
                    }
                    if self.touch.cursor_screen != Some(self.key_screen()) {
                        self.touch_cursor_reset();
                        self.touch_cursor_flush(event_loop);
                        break;
                    }
                }
                touch::desktop::Action::Button(right, down) => self.touch_queue_cursor_button(
                    if right {
                        MouseButton::Right
                    } else {
                        MouseButton::Left
                    },
                    down,
                ),
            }
        }
        self.touch_record_click_context();
    }
    fn touch_queue_cursor_button(&mut self, button: MouseButton, down: bool) {
        let Some(context) = self
            .touch
            .cursor_delivery
            .clone()
            .or_else(|| self.touch_click_context())
        else {
            return;
        };
        let due = self
            .touch
            .cursor_pending
            .iter()
            .filter(|edge| edge.button.is_some_and(|(b, _)| b == button))
            .map(|edge| edge.due + 1)
            .max()
            .unwrap_or(0)
            .max(self.frames + 1);
        self.touch.cursor_pending.push(CursorEdge {
            button: Some((button, down)),
            due,
            context,
        });
    }
    fn touch_cursor_frame(&mut self, event_loop: &dyn EventLoopCtl, playing: bool) {
        self.touch_cursor_flush(event_loop);
        if playing
            || self
                .touch
                .cursor_screen
                .is_some_and(|screen| screen != self.key_screen())
        {
            self.touch_cursor_reset();
            self.touch_cursor_flush(event_loop);
            return;
        }
        let now = self.touch_now();
        if let Some(touch::desktop::Action::Move(x, y)) = self.touch.cursor.tick(now) {
            self.touch_cursor_motion(event_loop, (x, y));
        }
        while let Some(edge) = self.touch.cursor_pending.first() {
            if edge.due > self.frames {
                break;
            }
            if edge.context.screen != self.key_screen()
                || self
                    .touch_batch()
                    .is_none_or(|batch| edge.context.size != (batch.width, batch.height))
            {
                self.touch_cursor_reset();
                self.touch_cursor_flush(event_loop);
                break;
            }
            if edge.button.is_some_and(|(_, down)| down) {
                let context = edge.context.clone();
                if self.touch.cursor_drawn != Some((context.screen, context.screen_at)) {
                    self.touch_pointer(event_loop, context.at);
                    self.touch.cursor_pending[0].due = self.frames + 1;
                    break;
                }
                if context.guard != horizon_client::widgets::capture_pointer(context.screen_at) {
                    self.touch_cursor_reset();
                    self.touch_cursor_flush(event_loop);
                    break;
                }
            }
            let edge = self.touch.cursor_pending.remove(0);
            if edge.button.is_some_and(|(button, down)| {
                !down
                    && !self.touch.cursor_down[if button == MouseButton::Middle {
                        2
                    } else {
                        usize::from(button == MouseButton::Right)
                    }]
            }) {
                continue;
            }
            let old_modifiers = self.modifiers;
            let old_delivery = self.touch.cursor_delivery.replace(edge.context.clone());
            self.modifiers = edge.context.modifiers;
            self.touch_pointer(event_loop, edge.context.at);
            if let Some((button, down)) = edge.button {
                if down {
                    for later in &mut self.touch.cursor_pending {
                        if later.button == Some((button, false)) {
                            later.due = later.due.max(self.frames + 1);
                        }
                    }
                }
                self.touch.cursor_down[if button == MouseButton::Middle {
                    2
                } else {
                    usize::from(button == MouseButton::Right)
                }] = down;
                self.touch_mouse_button(event_loop, button, down);
            }
            self.modifiers = old_modifiers;
            self.touch.cursor_delivery = old_delivery;
            if self.touch.cursor_screen != Some(self.key_screen()) {
                self.touch_cursor_reset();
                self.touch_cursor_flush(event_loop);
                break;
            }
        }
        if self.touch.cursor_pending.is_empty()
            && !self.hardware_hides_touch()
            && let Some(at) = self.touch.cursor.position
        {
            self.touch_pointer(event_loop, at);
        }
        self.touch_record_click_context();
    }
}

#[cfg(target_os = "ios")]
fn gesture_modifiers(bits: u32) -> winit::keyboard::ModifiersState {
    use winit::keyboard::ModifiersState as M;
    let mut value = M::empty();
    for (bit, flag) in [(1, M::SHIFT), (2, M::CONTROL), (4, M::ALT), (8, M::SUPER)] {
        value.set(flag, bits & bit != 0);
    }
    value
}

#[cfg(target_os = "ios")]
fn gesture_key(code: u16) -> Option<KeyCode> {
    Some(match code {
        49 => KeyCode::Digit1,
        50 => KeyCode::Digit2,
        51 => KeyCode::Digit3,
        52 => KeyCode::Digit4,
        53 => KeyCode::Digit5,
        54 => KeyCode::Digit6,
        55 => KeyCode::Digit7,
        56 => KeyCode::Digit8,
        57 => KeyCode::Digit9,
        70 => KeyCode::KeyF,
        81 => KeyCode::KeyQ,
        _ => return None,
    })
}

#[cfg(target_os = "ios")]
impl Game<'_> {
    fn touch_raw_holds_mouse_mapping(&self, button: MouseButton) -> bool {
        for (index, raw, control) in [
            (0, MouseButton::Left, Control::LeftClick),
            (1, MouseButton::Right, Control::RightClick),
        ] {
            let held = self.touch.fingers.held(control)
                || self.touch.cursor.held(index == 1)
                || self.touch.cursor_down[index]
                || self.touch.native_mouse[index];
            if held
                && self
                    .settings
                    .keys
                    .translate_mouse_all(raw)
                    .contains(&Err(button))
            {
                return true;
            }
        }
        for control in [
            Control::Forward,
            Control::Back,
            Control::Left,
            Control::Right,
            Control::Jump,
            Control::Sneak,
            Control::Perspective,
            Control::Chat,
            Control::Pause,
            Control::Inventory,
        ] {
            if (self.touch.fingers.held(control) || self.touch.cursor_keys.contains(&control))
                && let Some(raw) = key_of(control)
                && self.settings.keys.translate_all(raw).contains(&Err(button))
            {
                return true;
            }
        }
        self.touch.native_keys.iter().any(|raw| {
            self.settings
                .keys
                .translate_all(*raw)
                .contains(&Err(button))
        })
    }
    fn touch_native_handoff(&mut self) {
        self.touch_native_reset();
        // Discard old menu gestures without disabling recognition for a fresh
        // touch that arrives before the next rendered frame.
        horizon_ios_launcher::touch_gestures(self.key_screen(), 4, (0.0, 0.0), 1.0);
    }
    fn touch_native_reset(&mut self) {
        self.touch.native_pending.clear();
        self.touch.native_screen = None;
        for raw in std::mem::take(&mut self.touch.native_keys) {
            let _ = self.touch_raw_key(raw, false);
        }
        for (i, button) in [MouseButton::Left, MouseButton::Right]
            .into_iter()
            .enumerate()
        {
            if std::mem::take(&mut self.touch.native_mouse[i]) {
                let _ = self.touch_raw_button(button, false);
            }
        }
    }
    fn touch_native_input(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        input: touch::desktop::Physical,
    ) {
        use touch::desktop::Physical;
        match input {
            Physical::Key(code, down) => {
                if let Some(raw) = gesture_key(code) {
                    if down {
                        if !self.touch.native_keys.contains(&raw) {
                            self.touch.native_keys.push(raw);
                        }
                    } else {
                        self.touch.native_keys.retain(|key| *key != raw);
                    }
                    if let Err(error) = self.touch_raw_key(raw, down) {
                        log::warn!("Horizon: touch key: {error}");
                    }
                }
            }
            Physical::Mouse(index, down) if index < 2 => {
                self.touch.native_mouse[index as usize] = down;
                let button = if index == 0 {
                    MouseButton::Left
                } else {
                    MouseButton::Right
                };
                if self.touch_playing() {
                    if down {
                        self.touch_grab();
                    }
                    if let Err(error) = self.touch_raw_button(button, down) {
                        log::warn!("Horizon: touch button: {error}");
                    }
                } else {
                    self.touch.cursor_screen = Some(self.key_screen());
                    self.touch_queue_cursor_button(button, down);
                }
            }
            Physical::Scroll(x, y) => self.touch_scroll(event_loop, x as f32, y as f32),
            Physical::Keyboard => self.toggle_system_keyboard(),
            Physical::Cancel => self.touch_native_reset(),
            _ => {}
        }
    }
    fn touch_native_frame(&mut self, event_loop: &dyn EventLoopCtl) {
        let screen = self.key_screen();
        if self.touch.native_screen.is_some_and(|old| old != screen) {
            self.touch_native_reset();
        }
        self.touch.native_screen = Some(screen);
        let Some(batch) = self.touch_batch() else {
            return;
        };
        let blocked = horizon_ios_launcher::covered();
        let no_world = self.player.is_none() || self.title.is_some();
        let flags = u32::from(self.touch_playing())
            | if blocked { 2 } else { 0 }
            | if no_world { 8 } else { 0 };
        self.touch_record_click_context();
        let events = horizon_ios_launcher::touch_gestures(
            screen,
            flags,
            (f64::from(batch.width), f64::from(batch.height)),
            f64::from(batch.scale),
        );
        let now = self.touch_now();
        let (due, later) = std::mem::take(&mut self.touch.native_pending)
            .into_iter()
            .partition::<Vec<_>, _>(|(_, deadline, frame)| {
                *deadline <= now && *frame <= self.frames
            });
        self.touch.native_pending = later;
        for (input, _, _) in due {
            self.touch_native_input(event_loop, input);
        }
        for event in events {
            if self.key_screen() != screen {
                self.touch_native_reset();
                break;
            }
            let context = self
                .touch
                .click_contexts
                .iter()
                .find(|(id, _)| *id == event.context)
                .map(|(_, c)| c.clone());
            if event.context != 0 && context.is_none() {
                continue;
            }
            let context = context.or_else(|| self.touch_click_context());
            let context = context.map(|mut context| {
                context.modifiers = gesture_modifiers(event.modifiers);
                if event.kind == 1 {
                    if !self.touch_menu_cursor_mode() {
                        context.at = (event.point.0 as f32, event.point.1 as f32);
                    } else if let Some((id, at)) = context.surface
                        && id == event.touch
                    {
                        context.at.0 =
                            (context.at.0 + event.point.0 as f32 - at.0).clamp(0.0, context.size.0);
                        context.at.1 =
                            (context.at.1 + event.point.1 as f32 - at.1).clamp(0.0, context.size.1);
                    }
                    let screen_at = self.screen_point(context.at);
                    if context.screen_at != screen_at {
                        context.screen_at = screen_at;
                        context.guard = horizon_client::widgets::capture_pointer(screen_at);
                    }
                }
                context
            });
            let recognized = touch::desktop::Recognized {
                kind: event.kind,
                phase: event.phase,
                slot: event.slot,
                point: event.point,
            };
            for (delay, input) in
                recognized.inputs(self.touch_playing(), no_world || touch::mouse_gestures())
            {
                if delay == 0.0 {
                    let old_modifiers = self.modifiers;
                    let old_context = self.touch.cursor_delivery.clone();
                    if let Some(context) = context.clone() {
                        self.modifiers = context.modifiers;
                        self.touch.cursor_delivery = Some(context);
                    }
                    self.touch_native_input(event_loop, input);
                    self.modifiers = old_modifiers;
                    self.touch.cursor_delivery = old_context;
                } else {
                    self.touch.native_pending.push((
                        input,
                        now + (delay - event.age).max(0.0),
                        self.frames + 1,
                    ));
                }
            }
        }
    }
}

impl Game<'_> {
    // Title-screen taps must target the visible button directly, even when the
    // player's in-world menus use a virtual pointer with separate click buttons.
    fn touch_menu_cursor_mode(&self) -> bool {
        self.player.is_some() && self.title.is_none() && touch::cursor_mode()
    }

    fn visible_touch_layout(&self, w: f32, h: f32) -> Vec<(Control, touch::Rect)> {
        touch::layout(w, h)
            .into_iter()
            .filter(|(c, _)| {
                // No controller overlay outside a world, including stale player
                // state while returning to the title. Painting and hit tests agree.
                if self.player.is_none() || self.title.is_some() {
                    return false;
                }
                if self.touch.hidden {
                    matches!(c, Control::Hide | Control::Hotbar(_))
                } else {
                    self.touch.more
                        || !matches!(
                            c,
                            Control::Perspective
                                | Control::Chat
                                | Control::Debug
                                | Control::PlayerList
                        )
                }
            })
            .collect()
    }
}
fn draw_amethyst_control(
    batch: &mut HudBatch,
    atlas: &horizon_client::hud::HudAtlas,
    c: Control,
    r: touch::Rect,
    held: bool,
    point: Option<(f32, f32)>,
) {
    let start = batch.vertices.len();
    if c == Control::Joystick {
        let centre = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
        horizon_client::ui_theme::touch_disc(
            atlas,
            batch,
            centre,
            r[2] * 0.375 + 0.25,
            rgba(0, 0, 0, 77),
        );
        horizon_client::ui_theme::touch_disc(
            atlas,
            batch,
            point.unwrap_or(centre),
            r[2] / 8.0,
            rgba(0, 0, 0, 255),
        );
    } else {
        let label = match c {
            Control::More => "More",
            Control::Hide => "Hide\nGUI",
            Control::Runtime => "Runtime\nMenu",
            Control::Debug => "Debug\nMenu",
            Control::PlayerList => "Player\nList",
            Control::Jump => "Jump",
            Control::Sneak => "Shift",
            Control::Sprint => "Sprint",
            Control::LeftClick => "Left\nClick",
            Control::RightClick => "Right\nClick",
            Control::MiddleClick => "Middle\nClick",
            Control::Perspective => "Third\nperson",
            Control::Chat => "Chat",
            Control::Pause => "Pause",
            Control::Inventory => "Inventory",
            Control::Forward => "W",
            Control::Back => "S",
            Control::Left => "A",
            Control::Right => "D",
            _ => "",
        };
        horizon_client::ui_theme::touch_control(atlas, batch, r, label, held);
    }
    for v in &mut batch.vertices[start..] {
        v.color =
            (v.color & 0x00ffffff) | ((((v.color >> 24) as f32 * touch::opacity(c)) as u32) << 24);
    }
}

fn draw_lock(
    batch: &mut HudBatch,
    atlas: &horizon_client::hud::HudAtlas,
    r: touch::Rect,
    locked: bool,
) {
    let (cx, cy) = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
    horizon_client::ui_theme::touch_disc(
        atlas,
        batch,
        (cx, cy),
        r[2] / 2.0,
        if locked {
            rgba(130, 110, 190, 230)
        } else {
            rgba(60, 60, 60, 200)
        },
    );
    let u = r[2] / 10.0;
    let ink = rgba(255, 255, 255, 240);
    batch.fill(atlas, cx - 3.0 * u, cy, 6.0 * u, 4.0 * u, ink);
    batch.fill(atlas, cx - 2.0 * u, cy - 3.0 * u, 4.0 * u, u, ink);
    batch.fill(atlas, cx - 2.0 * u, cy - 2.0 * u, u, 2.0 * u, ink);
    batch.fill(atlas, cx + u, cy - 2.0 * u, u, 2.0 * u, ink);
}
