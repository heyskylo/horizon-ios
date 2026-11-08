//! Controllers through gilrs (Horizon's own; docs/controllers.md): the
//! active pad's state each loop iteration, its button edges dispatched as
//! the vanilla key mappings they are bound to, the left stick as an analog
//! move vector and the right stick turning the view. `horizon-ctl pad`
//! injects a virtual pad on top of the real one.
use super::event_loop::EventLoopCtl;
use super::{Game, Screen};
use horizon_client::actions::Action;
use horizon_client::held::Source;
use horizon_client::{
    Result,
    controller_notice::{Glyph, Notice, NoticeId, Priority},
    controller_ui::Device,
    gamepad::{self, Button, Edges, GlyphStyle, HealthWatch, PadState, Route, Rumble},
    keymapping::SNEAK,
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

pub(super) const HELP: &[&str] = &[
    "pad BUTTON down|up|tap         a virtual controller button (south east west north lb rb lt rt select start ls rs up down left right)",
    "pad stick left|right X Y       tilt a virtual stick (-1..1, up positive); 0 0 centres it",
    "pad trigger left|right VALUE   pull a virtual trigger (0..1)",
    "pad release                    centre and release the virtual controller",
    "pad status                     connected controllers, the active one and its state",
    "pad connection connected|disconnected   as if a controller came or went at this game's own reader",
];

/// The controllers and the state the game last acted on.
#[derive(Default)]
pub(super) struct Pads {
    /// Splitscreen: this player's pad, read by `PadRouter` for every game, instead of the pads.
    pub(super) feed: Option<PadState>,
    /// The pad behind `feed`.
    feed_source: Option<PadId>,
    /// The real controllers and the DualSense's output.
    #[cfg(feature = "controllers")]
    devices: hardware::Devices,
    /// The virtual pad of `horizon-ctl pad`.
    injected: PadState,
    /// The virtual pad was in use at the last poll.
    virtual_input: bool,
    /// Buttons a `pad ... tap` releases at the next poll.
    taps: Vec<Button>,
    pub(super) edges: Edges,
    activity: gamepad::Activity,
    context: Option<Context>,
    blocked_sticks: [bool; 2],
    /// Releases use the action that received the press, even after a rebind.
    actions: [Vec<&'static str>; 16],
    /// The game tick each button went down in, and releases held back until
    /// a tick has passed: vanilla samples keys once per tick, so a click
    /// shorter than that (a quick stick click for sprint) would be lost.
    pressed_at: [Option<u64>; 16],
    late_releases: Vec<Button>,
    /// The merged state of the last poll.
    pub(super) state: PadState,
    /// The hardware's buttons at the last poll, for `input-log`'s `pad`
    /// events: the merged state hides which source moved.
    last_hardware: PadState,
    /// The pad drove the game last (the mouse and keyboard hand control back).
    pub(super) in_use: bool,
    /// A key or mouse button was pressed in play since the pad was last touched: the keyboard and
    /// mouse have it, though only moving the mouse takes the pad out of use.
    pub(super) keyboard_took_over: bool,
    pub(super) style: GlyphStyle,
    /// The controller driving this player, or else the first one its own reader sees.
    pub(super) device: Option<Device>,
    /// Controllers that came and went at this game's own reader since the last poll.
    connections: Vec<Connection>,
    /// The cursor the pad drives in menus.
    pub(super) menu: super::pad_menu::MenuCursor,
    /// The last vibration asked for, for `pad status`.
    last_rumble: Option<(&'static str, gamepad::Pulse)>,
    /// The drop button, while it is held in play.
    held_drop: Option<gamepad::HeldDrop>,
    /// The drop about to happen is the pad's stack form: it has no Control for `drop_whole_stack`.
    /// On a server the drop waits for the tick.
    pub(super) drop_stack: bool,
    /// The player's health between polls (a drop is damage taken).
    health: HealthWatch,
    /// The gameplay guide's facts and the tick they were read in.
    guide_facts: Option<(u64, horizon_client::gameplay_guide::Facts)>,
    /// The guide's rows as last drawn.
    pub(super) guide_shown: Vec<guide::Shown>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Context {
    screen: u64,
    focus_navigation: bool,
    focused: bool,
    osk: bool,
    dialog: bool,
    enabled: bool,
}

mod guide;
#[cfg(feature = "controllers")]
mod hardware;
#[cfg(test)]
mod parity_tests;
pub(super) use hardware::{PadId, PadRouter};

/// How long the death screen's confirm ignores cancelling (`ConfirmScreen`).
const DEATH_CONFIRM_WAIT: Duration = Duration::from_secs(1);

/// A controller coming or going, for this player's notice (`pad_connection_notice`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Connection {
    /// A controller of this glyph family connected.
    Connected(GlyphStyle),
    /// One disconnected; `active`: it was driving this player.
    Disconnected { active: bool },
}

/// What a controller did since the last frame.
#[cfg_attr(not(feature = "controllers"), allow(dead_code))]
pub(in crate::game) enum PadEvent {
    Press(PadId, Button),
    /// Disconnected (out of battery, unplugged, out of range).
    Gone(PadId),
    /// Connected again under the same id (Windows keeps it; macOS gives a new one). On Linux
    /// gilrs gives a new device the slot of a disconnected one of the same model, so there this
    /// can be another controller of that model.
    Back(PadId),
}

/// A build without the `controllers` feature (the server build, which must not need libudev): no
/// hardware pads, only `horizon-ctl pad`'s virtual one.
#[cfg(not(feature = "controllers"))]
mod hardware {
    use super::{super::Game, Pads};
    use horizon_client::gamepad::{PadState, Pulse, Rumble};
    use serde_json::{Value, json};

    impl Pads {
        pub(super) fn read(&mut self) -> PadState {
            PadState::default()
        }
        pub(in crate::game) fn release_hardware(&mut self) {}
    }

    pub(in crate::game) type PadId = usize;
    #[derive(Default)]
    pub(in crate::game) struct PadRouter {}
    impl PadRouter {
        pub(in crate::game) fn adopt(&mut self, _: &mut Pads) {}
        pub(in crate::game) fn hand_back(&mut self, _: &mut Pads) {}
        pub(in crate::game) fn events(&mut self) -> Vec<super::PadEvent> {
            Vec::new()
        }
        pub(in crate::game) fn device(
            &self,
            _: PadId,
        ) -> Option<horizon_client::controller_ui::Device> {
            None
        }
        pub(in crate::game) fn lend_output(&mut self, _: &mut Pads, _: PadId) {}
        pub(in crate::game) fn reclaim_output(&mut self, _: &mut Pads) {}
        pub(in crate::game) fn play_rumbles(&mut self, _: &mut Pads, _: Option<PadId>) {}
        pub(in crate::game) fn report_status(&self, _: &mut Pads, _: Option<PadId>) {}
        pub(in crate::game) fn connected(&self, _: PadId) -> bool {
            false
        }
        pub(in crate::game) fn uuid(&self, _: PadId) -> [u8; 16] {
            [0; 16]
        }
        pub(in crate::game) fn state(&self, _: PadId) -> PadState {
            PadState::default()
        }
        pub(in crate::game) fn name(&self, _: PadId) -> String {
            String::new()
        }
        pub(in crate::game) fn style(&self, _: PadId) -> horizon_client::gamepad::GlyphStyle {
            horizon_client::gamepad::GlyphStyle::Auto
        }
    }

    impl Game<'_> {
        pub(super) fn pad_output_off(&mut self) {}
        pub(super) fn pad_output_frame(&mut self) {}
        pub(in crate::game) fn pad_close_output(&mut self) {}
        pub(super) fn pad_vibrate(&mut self, _: Rumble, _: Pulse) {}
        pub(super) fn pad_devices_status(&self) -> Value {
            json!({ "backend": false, "controllers": [] })
        }
    }
}

impl Pads {
    /// The digital buttons the person's pad changed since the last poll.
    pub(super) fn hardware_edges(&mut self, hardware: PadState) -> Vec<(Button, bool)> {
        let edges = Button::ALL
            .into_iter()
            .filter_map(|button| {
                let was = self.last_hardware.held(button);
                let now = hardware.held(button);
                (was != now).then_some((button, now))
            })
            .collect();
        self.last_hardware = hardware;
        edges
    }

    /// A replay's pad button, in place of `pad`'s line.
    pub(super) fn inject_button(&mut self, button: Button, down: bool) {
        self.injected.set(button, down);
    }
    /// Splitscreen: this player's pad state, from `source`. Another pad, or
    /// none: the old one's A going away isn't a release.
    pub(super) fn feed_from(&mut self, source: Option<PadId>, state: PadState) {
        if self.feed_source != source {
            self.feed_source = source;
            self.menu.armed = None;
            self.menu.stop_craft();
        }
        self.feed = Some(state);
    }
    /// The device the controller settings name: a real one, else `horizon-ctl`'s virtual pad.
    pub(super) fn device_or_virtual(&self) -> Option<Device> {
        self.device.clone().or_else(|| {
            self.in_use.then(|| Device {
                name: "Virtual controller".to_owned(),
                vendor: None,
                product: None,
            })
        })
    }
    /// A replay ended: let go of everything it held.
    pub(super) fn clear_injected(&mut self) {
        self.menu.armed = None;
        self.menu.stop_craft();
        self.injected = PadState::default();
    }

    /// Include releases delayed until the next simulation tick.
    pub(super) fn holds_action(&self, settings: &gamepad::PadSettings, action: &str) -> bool {
        settings.binding(action).is_some_and(|button| {
            self.edges.is_held(button) || self.late_releases.contains(&button)
        })
    }

    /// A new player entity took over (spawn, respawn, a server joined, a
    /// dimension change): its health is not compared with the old one's.
    pub(super) fn player_replaced(&mut self) {
        self.health.forget();
    }

    /// The active pad is a Nintendo one going by its letters.
    fn by_letter(&self, settings: &gamepad::PadSettings) -> bool {
        self.style == GlyphStyle::Nintendo && settings.nintendo_letters
    }
}

impl Game<'_> {
    fn pad_context(&self) -> Context {
        Context {
            screen: self.key_screen(),
            focus_navigation: self.pad_navigation_mode(),
            focused: self.focused,
            osk: self.pad.menu.osk.is_some(),
            dialog: self.dialog_open() || self.code_of_conduct.is_some(),
            enabled: self.settings.controller.enabled,
        }
    }

    /// Once per loop iteration: read the pads and act on button changes.
    pub(super) fn poll_pads(&mut self, event_loop: &dyn EventLoopCtl) {
        self.pad.virtual_input = self.pad.injected != PadState::default();
        let context = self.pad_context();
        if let Some(old) = self.pad.context.filter(|old| *old != context) {
            if old.screen != context.screen {
                self.pad_remember_focus(old.screen);
            }
            self.pad_cancel_inputs(event_loop);
        }
        self.pad.context = Some(context);
        if !self.settings.controller.enabled {
            self.pad_output_off();
            self.pad_cancel_inputs(event_loop);
            self.pad.state = PadState::default();
            self.pad.in_use = false;
            // The keyboard's menu focus follows its widget all the same.
            self.pad_menu_frame(event_loop);
            return;
        }
        let hardware = match self.pad.feed {
            Some(state) => state,
            None => self.pad.read(),
        };
        // A Nintendo pad going by its letters is read that way: its own, or the one
        // splitscreen's router feeds this player.
        let hardware = if self.pad.by_letter(&self.settings.controller) {
            hardware.by_letter()
        } else {
            hardware
        };
        // A person's pad, for `input-log` and the repro recording; the virtual
        // pad is an agent's `pad` line and is recorded as that.
        let hardware_edges = self.pad.hardware_edges(hardware);
        for (button, down) in hardware_edges {
            self.note_input(
                super::input_trace::InputOrigin::User,
                super::input_trace::TraceKind::Pad {
                    button: button.name().to_owned(),
                    action: if down { "down" } else { "up" }.to_owned(),
                },
            );
        }
        for change in std::mem::take(&mut self.pad.connections) {
            self.pad_connection_notice(change);
        }
        let style = self.settings.controller.glyphs.resolve(self.pad.style);
        self.ui.options.pad_style = style;
        // Nintendo glyphs on a pad going by its letters name the button playing each part.
        self.ui.options.pad_by_letter =
            style == GlyphStyle::Nintendo && self.pad.by_letter(&self.settings.controller);
        self.ui.options.pad_driving = self.pad.in_use && self.pad_focus_mode();
        let mut state = hardware.merge(&self.pad.injected);
        let activity = self.pad.activity.observe(&state, &self.settings.controller);
        for (i, (stick, deadzone)) in [
            (&mut state.left, self.settings.controller.move_deadzone),
            (&mut state.right, self.settings.controller.look_deadzone),
        ]
        .into_iter()
        .enumerate()
        {
            if gamepad::radial(*stick, deadzone) == [0.0; 2] {
                self.pad.blocked_sticks[i] = false;
            }
            if self.pad.blocked_sticks[i] || !self.pad_focused() {
                *stick = [0.0; 2];
            }
        }
        for button in std::mem::take(&mut self.pad.taps) {
            self.pad.injected.set(button, false);
        }
        let threshold = self.settings.controller.trigger_threshold;
        let edges = self.pad.edges.update(&state, threshold);
        if activity && self.pad_focused() {
            self.last_input = Instant::now();
            self.pad.in_use = true;
            self.pad.keyboard_took_over = false;
        }
        self.pad.state = state;
        self.pad_drop_frame();
        self.pad_output_frame();
        // Damage from any source, here or on a server; nothing counts on
        // the title screen.
        let health = self
            .player
            .as_ref()
            .filter(|_| self.title.is_none())
            .map(|p| p.health.health());
        if let Some(damage) = self.pad.health.observe(health) {
            self.pad_rumble(Rumble::Hurt(damage));
        }
        let ticking = self.title.is_none() && self.screen == Screen::Playing && !self.pause;
        for button in std::mem::take(&mut self.pad.late_releases) {
            if !ticking || self.pad.pressed_at[button as usize] != Some(self.ticks) {
                self.pad_button_logged(event_loop, button, false);
            } else {
                self.pad.late_releases.push(button);
            }
        }
        for (button, down) in edges {
            self.pad_edge(event_loop, button, down);
            let context = self.pad_context();
            if let Some(old) = self.pad.context.filter(|old| *old != context) {
                if old.screen != context.screen {
                    self.pad_remember_focus(old.screen);
                }
                self.pad_cancel_inputs(event_loop);
                self.pad.context = Some(context);
                break;
            }
        }
        self.pad_menu_frame(event_loop);
    }

    fn pad_edge(&mut self, event_loop: &dyn EventLoopCtl, button: Button, down: bool) {
        let ticking = self.title.is_none() && self.screen == Screen::Playing && !self.pause;
        if down {
            // Pressed again before its held-back release went out.
            if let Some(i) = self.pad.late_releases.iter().position(|b| *b == button) {
                self.pad.late_releases.remove(i);
                self.pad_button_logged(event_loop, button, false);
            }
            self.pad.pressed_at[button as usize] = Some(self.ticks);
        } else if ticking && self.pad.pressed_at[button as usize] == Some(self.ticks) {
            self.pad.late_releases.push(button);
            return;
        }
        self.pad_button_logged(event_loop, button, down);
    }

    /// End held actions and captures before another context receives input.
    fn pad_cancel_inputs(&mut self, event_loop: &dyn EventLoopCtl) {
        // Before the releases below: a cancelled press must not click, nor a cancelled trigger
        // change the recipe book's category as it comes up.
        self.pad.menu.armed = None;
        self.pad.menu.trigger = None;
        self.direct_craft_stop();
        let held_sticks = [
            gamepad::radial(self.pad.state.left, self.settings.controller.move_deadzone)
                != [0.0; 2],
            gamepad::radial(self.pad.state.right, self.settings.controller.look_deadzone)
                != [0.0; 2],
        ];
        for (blocked, held) in self.pad.blocked_sticks.iter_mut().zip(held_sticks) {
            *blocked |= held;
        }
        self.pad.state.left = [0.0; 2];
        self.pad.state.right = [0.0; 2];
        self.pad.late_releases.clear();
        for (button, down) in self.pad.edges.cancel() {
            self.pad_button_logged(event_loop, button, down);
        }
        // A short tap may already be up in Edges but still await its simulation tick.
        for button in Button::ALL {
            for action in std::mem::take(&mut self.pad.actions[button as usize]) {
                if let Err(e) = self.pad_action(action, false) {
                    log::warn!("Horizon: releasing controller action {action}: {e}");
                }
            }
        }
        self.pad_cancel_menu(event_loop);
    }

    pub(super) fn pad_yield_input(&mut self, event_loop: &dyn EventLoopCtl) {
        if self.pad.in_use {
            self.pad.in_use = false;
            self.pad_cancel_inputs(event_loop);
        }
    }

    fn pad_button_logged(&mut self, event_loop: &dyn EventLoopCtl, button: Button, down: bool) {
        if let Err(e) = self.pad_button(event_loop, button, down) {
            log::warn!("Horizon: controller {}: {e}", button.name());
        }
    }

    /// Input from the pad counts while the window has focus, or always for
    /// the virtual pad (a script can't focus the window).
    fn pad_focused(&self) -> bool {
        self.pad_has_window() || self.pad.virtual_input
    }

    /// The window has focus, or the player lets the pad play without it (Play in Background).
    pub(super) fn pad_has_window(&self) -> bool {
        self.focused || self.settings.controller.play_in_background
    }

    /// Losing focus doesn't pause a game the pad is playing in the background.
    pub(super) fn pad_keeps_playing_unfocused(&self) -> bool {
        self.settings.controller.enabled
            && self.settings.controller.play_in_background
            && self.pad.in_use
    }

    fn pad_button(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        button: Button,
        down: bool,
    ) -> Result<()> {
        if down && !self.pad_focused() {
            return Ok(());
        }
        let playing = !self.pad_in_menu();
        if down && playing && !self.captured && self.focused {
            // Like a click into the window: take the pointer.
            self.capture(true);
        }
        if !playing {
            self.pad_menu_button(event_loop, button, down)?;
            if down {
                return Ok(());
            }
        } else if button == Button::Start {
            return if down { self.pad_back() } else { Ok(()) };
        }
        // Releases always reach the mappings, so nothing stays held when a
        // screen opened in between.
        let actions = if down {
            let actions = self.settings.controller.actions_of(button);
            self.pad.actions[button as usize] = actions.clone();
            actions
        } else {
            std::mem::take(&mut self.pad.actions[button as usize])
        };
        for action in actions {
            self.pad_action(action, down)?;
        }
        Ok(())
    }

    /// A bound action: a vanilla mapping acts as its default key or mouse
    /// button, like a rebound keyboard key.
    fn pad_action(&mut self, action: &str, down: bool) -> Result<()> {
        let Some(route) = gamepad::route(action) else {
            log::warn!("Horizon: controller action {action} has no handler");
            return Ok(());
        };
        match route {
            Route::HotbarStep { next } => {
                if down {
                    self.hotbar_step(next);
                }
                Ok(())
            }
            Route::Sneak
                if self.settings.controller.toggle_sneak && !self.settings.toggle_crouch =>
            {
                // A stick click flips sneaking instead of holding it, as
                // vanilla's toggle crouch does: a screen opening releases it
                // and closing restores it (ToggleKeyMapping).
                if !down {
                    return Ok(());
                }
                let screen_open = self.screen != Screen::Playing || self.title.is_some();
                let was = self.keys.held(Action::Sneak);
                self.toggles.pad_toggle(SNEAK, screen_open);
                self.sync_toggle(SNEAK);
                if !was && self.keys.held(Action::Sneak) {
                    // The pad's latch: a keyboard handoff leaves it.
                    self.keys.press(KeyCode::ShiftLeft, Source::Pad);
                    self.keys.release(&KeyCode::ShiftLeft, Source::Keyboard);
                }
                self.key_pressed(KeyCode::ShiftLeft, KeyCode::ShiftLeft)
            }
            Route::Sneak => self.mapped_key(
                KeyCode::ShiftLeft,
                down,
                false,
                KeyCode::ShiftLeft,
                Source::Pad,
            ),
            Route::Screenshot => {
                if down {
                    self.screenshot_action();
                }
                Ok(())
            }
            Route::SmoothCamera => {
                if down {
                    self.smooth_camera_action();
                }
                Ok(())
            }
            Route::Fullscreen => {
                if down {
                    self.toggle_fullscreen();
                }
                Ok(())
            }
            Route::Held => Ok(()),
            Route::Mouse(button) => {
                let playing = self.title.is_none() && self.screen == Screen::Playing;
                // What the window path tells mods and the click counters. Pick block isn't one
                // they count.
                if playing
                    && matches!(button, MouseButton::Left | MouseButton::Right)
                    && (down || !self.mouse_mapping_held_elsewhere(button, Source::Pad))
                {
                    self.extras_mouse(button, down);
                }
                if !down || playing {
                    self.mouse_mapping(button, down, Source::Pad)?;
                }
                Ok(())
            }
            Route::Key(KeyCode::KeyQ) => {
                self.pad.drop_stack = false;
                self.pad.held_drop = down.then(|| {
                    let delay = self.settings.controller.repeat_delay;
                    gamepad::HeldDrop::new(Instant::now(), Duration::from_secs_f32(delay))
                });
                self.mapped_key(KeyCode::KeyQ, down, false, KeyCode::KeyQ, Source::Pad)
            }
            Route::Key(key) => self.mapped_key(key, down, false, key, Source::Pad),
        }
    }

    /// Each poll: a held drop button, as a held Q's key repeats reach the mapping.
    fn pad_drop_frame(&mut self) {
        if self.pad_in_menu() || self.screen_over_playing() {
            return;
        }
        let interval = Duration::from_secs_f32(self.settings.controller.repeat_interval);
        let step = self
            .pad
            .held_drop
            .as_mut()
            .and_then(|held| held.due(Instant::now(), interval));
        let Some(step) = step else {
            return;
        };
        self.pad.drop_stack = step == gamepad::DropStep::Stack;
        if let Err(e) = self.mapped_key(KeyCode::KeyQ, true, true, KeyCode::KeyQ, Source::Pad) {
            log::warn!("Horizon: controller drop: {e}");
        }
    }

    /// B and Start: what Escape does where the pad is.
    pub(super) fn pad_back(&mut self) -> Result<()> {
        // The death confirm's buttons wait a second, as its Escape does.
        if self
            .ui
            .death_confirm
            .is_some_and(|shown| shown.elapsed() < DEATH_CONFIRM_WAIT)
        {
            return Ok(());
        }
        if self.prompt_open() {
            return self.pad_prompt_back();
        }
        // The options open over the title screen too: they come first, as
        // for the keyboard's Escape.
        if self.screen == Screen::Options {
            return self.options_key(KeyCode::Escape, None);
        }
        if let Some(title) = self.title.as_mut() {
            title.escape();
            self.save_title_options();
            return Ok(());
        }
        if self.sign_edit.is_some() {
            self.close_sign_editor();
            return Ok(());
        }
        if self.book.is_some() {
            self.close_book();
            return Ok(());
        }
        self.key_pressed(KeyCode::Escape, KeyCode::Escape)
    }

    /// The left stick as the move vector, while playing.
    pub(super) fn pad_move(&self) -> Option<[f32; 2]> {
        let settings = &self.settings.controller;
        if !settings.enabled || self.pad_in_menu() || !self.pad_focused() {
            return None;
        }
        gamepad::move_vector(self.pad.state.left, settings.move_deadzone)
    }

    /// Whether `key` is held, a movement key by the left stick too: as the keys the stick holds
    /// (`eight_direction_keys`), whatever moves the player. For readers of the keys that aren't
    /// the movement itself.
    pub(super) fn movement_key_held(&self, key: KeyCode) -> bool {
        if self.keys.raw(&key) {
            return true;
        }
        let Some(stick) = self.pad_move() else {
            return false;
        };
        let [forward, backward, left, right] = gamepad::eight_direction_keys(stick);
        match key {
            KeyCode::KeyW => forward,
            KeyCode::KeyS => backward,
            KeyCode::KeyA => left,
            KeyCode::KeyD => right,
            _ => false,
        }
    }

    /// The right stick turns the view, once per frame (`Entity.turn`).
    pub(super) fn pad_look(&mut self, seconds: f32) {
        let settings = &self.settings.controller;
        if !settings.enabled || self.pad_in_menu() || self.player.is_none() || !self.pad_focused() {
            return;
        }
        // A long frame (a hitch) must not spin the view.
        let (yaw, pitch) = gamepad::look_delta(
            self.pad.state.right,
            settings,
            seconds.min(0.1),
            self.scoping(),
        );
        if yaw == 0.0 && pitch == 0.0 {
            return;
        }
        self.look_turn(f64::from(yaw / 0.15), f64::from(pitch / 0.15));
    }

    /// Vibrate the active pad, when vibration is on and the pad is the one
    /// playing (a pad lying idle while the mouse plays stays still).
    pub(super) fn pad_rumble(&mut self, rumble: Rumble) {
        let settings = &self.settings.controller;
        if !settings.enabled || !settings.vibration || !self.pad.in_use {
            return;
        }
        let Some(pulse) = rumble.pulse() else {
            return;
        };
        self.pad.last_rumble = Some((rumble.name(), pulse));
        self.pad_vibrate(rumble, pulse);
    }

    /// `horizon-ctl pad ...`.
    /// A controller came or went with no splitscreen router: say so on this view, in its one
    /// notice. Nothing here decides what the controller drives; a player who turned controllers
    /// off isn't told about them.
    fn pad_connection_notice(&mut self, change: Connection) {
        // The pad that was playing is gone: nothing is driving until another is touched.
        if matches!(change, Connection::Disconnected { active: true }) {
            self.pad.in_use = false;
        }
        if !self.settings.controller.enabled {
            return;
        }
        let split =
            |key: &str| horizon_client::lang::get(&format!("horizon.splitscreen.prompt.{key}"));
        let notice = match change {
            Connection::Connected(family) => Notice::new(
                NoticeId::Connection,
                Priority::Info,
                split("controller_connected"),
                horizon_client::lang::get("horizon.pad.notice.connected.press"),
            )
            .with_glyph(Glyph {
                button: Button::South,
                style: self.settings.controller.glyphs.resolve(family),
            }),
            Connection::Disconnected { active } => Notice::new(
                NoticeId::Connection,
                if active {
                    Priority::Attention
                } else {
                    Priority::Info
                },
                split("controller_disconnected"),
                split("controller_disconnected.hint"),
            ),
        };
        self.split_hud.notice.raise(notice);
    }

    pub(super) fn pad_command(&mut self, words: &[&str]) -> Result<Value> {
        let arg = |i: usize| -> Result<&str> {
            words
                .get(i)
                .copied()
                .ok_or_else(|| format!("usage: {}", HELP[0]).into())
        };
        let number = |i: usize| -> Result<f32> {
            let v: f32 = arg(i)?.parse()?;
            if v.is_finite() {
                Ok(v)
            } else {
                Err("not a number".into())
            }
        };
        match arg(1)? {
            "status" => {
                let s = &self.pad.state;
                let held: Vec<&str> = Button::ALL
                    .into_iter()
                    .filter(|b| self.pad.edges.is_held(*b))
                    .map(Button::name)
                    .collect();
                let mut status = json!({
                    "enabled": self.settings.controller.enabled,
                    "glyphs": self.pad.style.name(),
                    "last_rumble": self.pad.last_rumble.map(|(why, p)| json!({
                        "reason": why, "strong": p.strong, "weak": p.weak, "ms": p.millis,
                    })),
                    "in_use": self.pad.in_use,
                    "notice": self.split_hud.notice.showing().map(|(title, body)| {
                        json!({ "title": title, "body": body })
                    }),
                    "menu": self.pad_menu_status(),
                    "guide": self.pad_guide_status(),
                    "held": held,
                    "left": s.left,
                    "right": s.right,
                    "triggers": s.triggers,
                });
                if let (Some(status), Value::Object(devices)) =
                    (status.as_object_mut(), self.pad_devices_status())
                {
                    status.extend(devices);
                }
                Ok(status)
            }
            "connection" => {
                let change = match arg(2)? {
                    "connected" => Connection::Connected(GlyphStyle::PlayStation),
                    "disconnected" => Connection::Disconnected { active: true },
                    other => {
                        return Err(format!(
                            "connection must be connected or disconnected, not {other}"
                        )
                        .into());
                    }
                };
                self.pad.connections.push(change);
                Ok(json!({ "ok": true }))
            }
            "release" => {
                self.pad.menu.armed = None;
                self.direct_craft_stop();
                self.pad.injected = PadState::default();
                self.pad.taps.clear();
                Ok(json!({ "ok": true }))
            }
            "stick" => {
                let v = [number(3)?.clamp(-1.0, 1.0), number(4)?.clamp(-1.0, 1.0)];
                match arg(2)? {
                    "left" => self.pad.injected.left = v,
                    "right" => self.pad.injected.right = v,
                    other => return Err(format!("stick must be left or right, not {other}").into()),
                }
                Ok(json!({ "ok": true }))
            }
            "trigger" => {
                let v = number(3)?.clamp(0.0, 1.0);
                match arg(2)? {
                    "left" => self.pad.injected.triggers[0] = v,
                    "right" => self.pad.injected.triggers[1] = v,
                    other => {
                        return Err(format!("trigger must be left or right, not {other}").into());
                    }
                }
                Ok(json!({ "ok": true }))
            }
            name => {
                let button = Button::parse(name)
                    .ok_or_else(|| format!("unknown pad button {name}; {}", HELP[0]))?;
                match arg(2)? {
                    "down" => self.pad.injected.set(button, true),
                    "up" => self.pad.injected.set(button, false),
                    "tap" => {
                        self.pad.injected.set(button, true);
                        self.pad.taps.push(button);
                    }
                    other => {
                        return Err(
                            format!("pad action must be down, up or tap, not {other}").into()
                        );
                    }
                }
                Ok(json!({ "ok": true }))
            }
        }
    }
}
