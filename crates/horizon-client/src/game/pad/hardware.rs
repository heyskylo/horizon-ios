//! The real controllers (the `controllers` feature): gilrs' pads, and the DualSense's own output
//! over HID: its rumble, adaptive triggers, lights and HD haptics. A build without the feature (the
//! server build) has the stub in pad.rs instead, and only `horizon-ctl pad`'s virtual pad.
use super::{
    super::{Game, Screen},
    Connection, PadEvent, Pads,
};
use horizon_client::controller_ui::Device;
use horizon_client::gamepad::{self, Button, GlyphStyle, PadState, Rumble, TriggerFeel};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

mod face_buttons;
mod hid_layout;
mod sdl_row;
mod triggers;

/// The controllers gilrs sees and the DualSense's output.
#[derive(Default)]
pub(super) struct Devices {
    gilrs: Option<gilrs::Gilrs>,
    started: bool,
    /// The pads seen so far, so one already there when gilrs opens isn't news.
    known: Vec<gilrs::GamepadId>,
    /// The pad last touched; only its input counts.
    active: Option<gilrs::GamepadId>,
    /// Vibrations playing (an effect stops when dropped) and when they end.
    rumbles: Vec<(gilrs::ff::Effect, Instant)>,
    /// The active pad is a DualSense.
    dualsense: bool,
    /// Its own output over HID (rumble here; gilrs only reads it), started once this window
    /// has the pad in hand; dropped, it releases the pad.
    output: Option<horizon_dualsense::Link>,
    /// Its HD haptics, on USB: looked for once each time the output opens.
    haptics: Option<horizon_dualsense::haptics::Haptics>,
    haptics_tried: bool,
    /// What it was last told to show.
    shown: Option<horizon_dualsense::device::State>,
    /// While `PadRouter` has the reader and isn't lending it: what `pad status` lists.
    routed: Option<Vec<Value>>,
    /// While `PadRouter` has the reader and isn't lending it: vibrations it hasn't played yet.
    routed_rumbles: Vec<gamepad::Pulse>,
}

/// `pad status`'s line for a controller. `uuid` is the id an SDL mapping row starts with, so a
/// report says which row a pad needs; `mapping_source` says whether one was found.
fn controller_status(
    id: gilrs::GamepadId,
    pad: &gilrs::Gamepad,
    active: Option<gilrs::GamepadId>,
) -> Value {
    let mapping_source = match pad.mapping_source() {
        gilrs::MappingSource::SdlMappings => "sdl",
        gilrs::MappingSource::Driver => "driver",
        gilrs::MappingSource::None => "none",
    };
    let uuid: String = pad.uuid().iter().map(|b| format!("{b:02x}")).collect();
    json!({
        "name": pad.name(),
        "vendor": pad.vendor_id(),
        "product": pad.product_id(),
        "mapping_source": mapping_source,
        "uuid": uuid,
        "active": active == Some(id),
    })
}

impl Devices {
    /// The hardware part of `pad status`.
    fn status(&self) -> Value {
        let pads: Vec<Value> = match (&self.gilrs, &self.routed) {
            (Some(g), _) => g
                .gamepads()
                .map(|(id, p)| controller_status(id, &p, self.active))
                .collect(),
            (None, Some(routed)) => routed.clone(),
            (None, None) => Vec::new(),
        };
        json!({
            "backend": self.gilrs.is_some() || self.routed.is_some(),
            "controllers": pads,
            "dualsense_output": self.output.as_ref().and_then(|o| o.open()).map(|t| format!("{t:?}")),
            "trigger_effects": self.shown.map(|s| s.triggers),
        })
    }
}

fn gilrs_button(button: Button) -> gilrs::Button {
    use gilrs::Button as G;
    match button {
        Button::South => G::South,
        Button::East => G::East,
        Button::West => G::West,
        Button::North => G::North,
        Button::LeftBumper => G::LeftTrigger,
        Button::RightBumper => G::RightTrigger,
        Button::LeftTrigger => G::LeftTrigger2,
        Button::RightTrigger => G::RightTrigger2,
        Button::Select => G::Select,
        Button::Start => G::Start,
        Button::LeftStick => G::LeftThumb,
        Button::RightStick => G::RightThumb,
        Button::DPadUp => G::DPadUp,
        Button::DPadDown => G::DPadDown,
        Button::DPadLeft => G::DPadLeft,
        Button::DPadRight => G::DPadRight,
    }
}

fn is_dualsense(pad: &gilrs::Gamepad) -> bool {
    matches!((pad.vendor_id(), pad.product_id()),
        (Some(vendor), Some(product)) if horizon_dualsense::is_dualsense(vendor, product))
}

/// SDL rows gilrs' bundled DB is missing (or only ships for Windows). Newer
/// than the crate's copy; `GilrsBuilder::add_mappings` prefers these. The
/// Ultimate 2's Linux/BT rows put the right stick on a2/a3 — without them
/// look never moves (#4343).
const EXTRA_SDL_MAPPINGS: &str = concat!(
    "03000000c82d00001260000011010000,8BitDo Ultimate 2,a:b0,b:b1,back:b10,dpdown:h0.4,dpleft:h0.8,dpright:h0.2,dpup:h0.1,guide:b12,leftshoulder:b6,leftstick:b13,lefttrigger:a5,leftx:a0,lefty:a1,paddle1:b17,paddle2:b16,paddle3:b2,paddle4:b5,rightshoulder:b7,rightstick:b14,righttrigger:a4,rightx:a2,righty:a3,start:b11,x:b3,y:b4,platform:Linux,\n",
    "05000000c82d00001260000001000000,8BitDo Ultimate 2,a:b0,b:b1,back:b10,dpdown:h0.4,dpleft:h0.8,dpright:h0.2,dpup:h0.1,guide:b12,leftshoulder:b6,leftstick:b13,lefttrigger:a5,leftx:a0,lefty:a1,paddle1:b17,paddle2:b16,paddle3:b2,paddle4:b5,rightshoulder:b7,rightstick:b14,righttrigger:a4,rightx:a2,righty:a3,start:b11,x:b3,y:b4,platform:Linux,\n",
    "03000000c82d00000b31000014010000,8BitDo Ultimate 2,a:b0,b:b1,back:b6,dpdown:h0.4,dpleft:h0.8,dpright:h0.2,dpup:h0.1,guide:b8,leftshoulder:b4,leftstick:b9,lefttrigger:a2,leftx:a0,lefty:a1,rightshoulder:b5,rightstick:b10,righttrigger:a5,rightx:a3,righty:a4,start:b7,x:b2,y:b3,platform:Linux,\n",
);

/// A vibration on pad `id` through gilrs' force feedback, kept alive until its end. Platforms
/// without it (gilrs on macOS) skip it.
fn play_pulse(
    gilrs: &mut gilrs::Gilrs,
    id: PadId,
    pulse: gamepad::Pulse,
) -> Option<(gilrs::ff::Effect, Instant)> {
    if !gilrs
        .connected_gamepad(id)
        .is_some_and(|g| g.is_ff_supported())
    {
        return None;
    }
    use gilrs::ff::{BaseEffect, BaseEffectType, EffectBuilder, Repeat, Replay, Ticks};
    let length = Ticks::from_ms(pulse.millis);
    let motor = |kind| BaseEffect {
        kind,
        scheduling: Replay {
            play_for: length,
            ..Default::default()
        },
        envelope: Default::default(),
    };
    let magnitude = |v: f32| (v.clamp(0.0, 1.0) * f32::from(u16::MAX)) as u16;
    let effect = EffectBuilder::new()
        .add_effect(motor(BaseEffectType::Strong {
            magnitude: magnitude(pulse.strong),
        }))
        .add_effect(motor(BaseEffectType::Weak {
            magnitude: magnitude(pulse.weak),
        }))
        .repeat(Repeat::For(length))
        .gamepads(&[id])
        .finish(gilrs);
    match effect.and_then(|e| e.play().map(|()| e)) {
        Ok(effect) => Some((
            effect,
            Instant::now() + Duration::from_millis(u64::from(pulse.millis) + 50),
        )),
        Err(e) => {
            log::debug!("Horizon: controller vibration: {e}");
            None
        }
    }
}

/// The one way to open gilrs: each reader needs the extra rows.
fn gilrs_builder() -> gilrs::GilrsBuilder {
    gilrs::GilrsBuilder::new().add_mappings(EXTRA_SDL_MAPPINGS)
}

impl Pads {
    /// Open gilrs on first use. A platform without it (or a Linux without
    /// udev) leaves controllers off; the game runs as before.
    fn start(&mut self) {
        if self.devices.started {
            return;
        }
        self.devices.started = true;
        self.devices.gilrs = match gilrs_builder().build() {
            Ok(g) => {
                for (id, pad) in g.gamepads() {
                    log::info!("Controller connected: {}", pad.name());
                    self.devices.known.push(id);
                }
                Some(g)
            }
            Err(gilrs::Error::NotImplemented(_)) => None,
            Err(e) => {
                log::warn!("Horizon: controllers are unavailable: {e}");
                None
            }
        };
    }

    /// Splitscreen reads the pads for every player (`PadRouter`): this game's own gilrs closes, or
    /// its events would queue up unread. It opens again if this game reads the pads itself.
    /// While several play, the router plays their rumble (`PadRouter::play_rumbles`), and nobody
    /// gets adaptive triggers or the lightbar: it has no output path for those yet.
    pub(in crate::game) fn release_hardware(&mut self) {
        if self.devices.started {
            self.devices.gilrs = None;
            self.devices.started = false;
            self.devices.active = None;
            self.devices.known.clear();
            self.device = None;
            self.devices.rumbles.clear();
            self.devices.dualsense = false;
            self.devices.output = None;
            self.devices.shown = None;
        }
    }

    /// Drain gilrs' events and read the active pad (opening gilrs the first time).
    pub(super) fn read(&mut self) -> PadState {
        self.start();
        let Some(gilrs) = self.devices.gilrs.as_mut() else {
            return PadState::default();
        };
        let active = self.devices.active;
        while let Some(gilrs::Event { id, event, .. }) = gilrs.next_event() {
            use gilrs::EventType as E;
            match event {
                E::Connected => {
                    log::info!("Controller connected: {}", gilrs.gamepad(id).name());
                    // Some backends report the controllers already there as they open.
                    if !self.devices.known.contains(&id) {
                        self.devices.known.push(id);
                        let pad = gilrs.gamepad(id);
                        let style = GlyphStyle::of_device(pad.vendor_id(), pad.product_id());
                        self.connections.push(Connection::Connected(style));
                    }
                    if let Some(output) = &self.devices.output {
                        output.scan();
                    }
                }
                E::Disconnected => {
                    log::info!("Controller disconnected: {}", gilrs.gamepad(id).name());
                    self.devices.known.retain(|known| *known != id);
                    self.connections.push(Connection::Disconnected {
                        active: self.devices.active == Some(id),
                    });
                    if self.devices.active == Some(id) {
                        self.devices.active = None;
                    }
                    if let Some(output) = &self.devices.output {
                        output.scan();
                    }
                }
                E::ButtonPressed(..) => self.devices.active = Some(id),
                E::AxisChanged(axis, value, _)
                    if axis_takes_over(axis, value, z_is_a_stick(&gilrs.gamepad(id))) =>
                {
                    self.devices.active = Some(id)
                }
                E::ButtonChanged(_, value, _) if value > TAKEOVER => self.devices.active = Some(id),
                _ => {}
            }
        }
        // Another pad, or none: its A going away isn't a release.
        if self.devices.active != active {
            self.menu.armed = None;
            self.menu.stop_craft();
        }
        let Some(pad) = self
            .devices
            .active
            .and_then(|id| gilrs.connected_gamepad(id))
        else {
            // Before any pad is touched (the virtual pad playing), a DualSense still shows it.
            self.devices.dualsense = gilrs.gamepads().any(|(_, pad)| is_dualsense(&pad));
            self.device = gilrs.gamepads().next().map(|(_, pad)| device_of(&pad));
            return PadState::default();
        };
        self.device = Some(device_of(&pad));
        self.style = GlyphStyle::of_device(pad.vendor_id(), pad.product_id());
        self.devices.dualsense = is_dualsense(&pad);
        state_of(&pad)
    }
}

/// How far a stick or trigger has to go for its pad to become the active one.
const TAKEOVER: f32 = 0.5;

/// Whether this pad has a stick on Z, where most pads have triggers.
fn z_is_a_stick(pad: &gilrs::Gamepad) -> bool {
    hid_layout::right_stick_on_z(pad)
        || (pad.mapping_source() != gilrs::MappingSource::SdlMappings
            && (pad.vendor_id(), pad.product_id())
                == (
                    Some(gamepad::BITDO_ULTIMATE2_VID_PID.0),
                    Some(gamepad::BITDO_ULTIMATE2_VID_PID.1),
                ))
}

/// Whether an axis this far along means someone is using its pad. Z and axes gilrs has no name for
/// can be triggers, which rest at -1, so only their pulled end counts: touching an idle pad's
/// trigger mustn't take over from the pad in use.
fn axis_takes_over(axis: gilrs::Axis, value: f32, z_is_a_stick: bool) -> bool {
    use gilrs::Axis as A;
    match axis {
        A::LeftZ | A::RightZ if z_is_a_stick => value.abs() > TAKEOVER,
        A::LeftZ | A::RightZ | A::Unknown => value > TAKEOVER,
        _ => value.abs() > TAKEOVER,
    }
}

/// What the controller settings say about a pad.
fn device_of(pad: &gilrs::Gamepad) -> Device {
    Device {
        name: pad.name().to_owned(),
        vendor: pad.vendor_id(),
        product: pad.product_id(),
    }
}

/// A pad's buttons, sticks and triggers now.
fn state_of(pad: &gilrs::Gamepad) -> PadState {
    let mut state = PadState::default();
    let crossed = face_buttons::crossed(pad);
    for button in Button::ALL {
        if !matches!(button, Button::LeftTrigger | Button::RightTrigger) {
            let named = face_buttons::named(gilrs_button(button), crossed);
            state.set(button, pad.is_pressed(named));
        }
    }
    let axis = |a| pad.value(a);
    use gilrs::Axis as A;
    let sdl_mapped = pad.mapping_source() == gilrs::MappingSource::SdlMappings;
    let (left, right) = gamepad::stick_axes(
        [axis(A::LeftStickX), axis(A::LeftStickY)],
        [axis(A::RightStickX), axis(A::RightStickY)],
        axis(A::LeftZ),
        pad.vendor_id(),
        pad.product_id(),
        sdl_mapped,
    );
    let (left, right) = sdl_row::sticks(pad, left, right);
    state.left = left;
    state.right = if hid_layout::right_stick_on_z(pad) {
        hid_layout::right_stick([axis(A::LeftZ), axis(A::RightZ)])
    } else {
        right
    };
    state.triggers = triggers::triggers(pad);
    state
}

/// A controller, as splitscreen tells them apart.
pub(in crate::game) type PadId = gilrs::GamepadId;

/// Splitscreen: one gilrs for every player's game in this process. The split decides which
/// player each pad drives; this reads them.
#[derive(Default)]
pub(in crate::game) struct PadRouter {
    gilrs: Option<gilrs::Gilrs>,
    started: bool,
    /// Vibrations playing while several play, each until its end.
    rumbles: Vec<(gilrs::ff::Effect, Instant)>,
}

impl PadRouter {
    /// Take over the reader P1 already has open, instead of opening a second and closing the
    /// first. A compositor can count who holds a pad to pause its own use of it (KWin's pointer
    /// emulation), and that count goes wrong when readers overlap: gilrs opens a pad twice at
    /// once, which inotify can merge into one event, and closes it twice apart (#9730).
    pub(in crate::game) fn adopt(&mut self, pads: &mut Pads) {
        if !self.started
            && let Some(gilrs) = pads.devices.gilrs.take()
        {
            self.started = true;
            self.gilrs = Some(gilrs);
        }
    }

    /// Play on pad `id` what its player's turn asked for, and let finished vibrations go.
    pub(in crate::game) fn play_rumbles(&mut self, pads: &mut Pads, id: Option<PadId>) {
        let now = Instant::now();
        self.rumbles.retain(|(_, end)| *end > now);
        let pulses = std::mem::take(&mut pads.devices.routed_rumbles);
        let (Some(gilrs), Some(id)) = (self.gilrs.as_mut(), id) else {
            return;
        };
        self.rumbles
            .extend(pulses.into_iter().filter_map(|p| play_pulse(gilrs, id, p)));
    }

    /// Routing ends: P1 gets the reader back, open as it was.
    pub(in crate::game) fn hand_back(&mut self, pads: &mut Pads) {
        self.started = false;
        pads.devices.routed = None;
        if let Some(gilrs) = self.gilrs.take() {
            pads.devices.gilrs = Some(gilrs);
            pads.devices.started = true;
        }
    }

    /// Solo play uses this reader's device IDs for feedback too: another pad cannot steal
    /// the active output while its input is being offered to join. Reclaimed after P1's turn.
    pub(in crate::game) fn lend_output(&mut self, pads: &mut Pads, id: PadId) {
        if pads.devices.gilrs.is_some() {
            pads.release_hardware();
        }
        if pads.devices.active != Some(id) {
            pads.devices.rumbles.clear();
            pads.devices.output = None;
            pads.devices.shown = None;
        }
        pads.devices.dualsense = self
            .gilrs
            .as_ref()
            .and_then(|g| g.connected_gamepad(id))
            .is_some_and(|p| is_dualsense(&p));
        pads.devices.active = Some(id);
        pads.devices.started = true;
        pads.devices.gilrs = self.gilrs.take();
    }

    pub(in crate::game) fn reclaim_output(&mut self, pads: &mut Pads) {
        self.gilrs = pads.devices.gilrs.take();
    }

    /// `pad status` is answered in P1's turn, which can't reach this reader: leave it what the
    /// reader sees. `owner`: the pad driving P1.
    pub(in crate::game) fn report_status(&self, pads: &mut Pads, owner: Option<PadId>) {
        pads.devices.routed = self.gilrs.as_ref().map(|g| {
            g.gamepads()
                .map(|(id, p)| controller_status(id, &p, owner))
                .collect()
        });
    }

    /// The buttons pressed and the controllers gone or back since the last call, in order.
    pub(in crate::game) fn events(&mut self) -> Vec<PadEvent> {
        if !self.started {
            self.started = true;
            self.gilrs = gilrs_builder().build().ok();
        }
        let Some(gilrs) = self.gilrs.as_mut() else {
            return Vec::new();
        };
        let mut events = Vec::new();
        while let Some(gilrs::Event { id, event, .. }) = gilrs.next_event() {
            match event {
                gilrs::EventType::ButtonPressed(pressed, _) => {
                    let crossed = face_buttons::crossed(&gilrs.gamepad(id));
                    if let Some(button) = Button::ALL
                        .into_iter()
                        .find(|&b| face_buttons::named(gilrs_button(b), crossed) == pressed)
                    {
                        events.push(PadEvent::Press(id, button));
                    }
                }
                gilrs::EventType::Disconnected => {
                    log::info!("Controller disconnected: {}", gilrs.gamepad(id).name());
                    events.push(PadEvent::Gone(id));
                }
                gilrs::EventType::Connected => events.push(PadEvent::Back(id)),
                _ => {}
            }
        }
        events
    }

    pub(in crate::game) fn connected(&self, id: PadId) -> bool {
        self.gilrs
            .as_ref()
            .is_some_and(|g| g.connected_gamepad(id).is_some())
    }

    /// The kind of controller (its maker, model and bus; two of one model share it), to know it
    /// back when it reconnects under a new id; zero when the system doesn't say.
    pub(in crate::game) fn uuid(&self, id: PadId) -> [u8; 16] {
        self.gilrs
            .as_ref()
            .map_or([0; 16], |g| g.gamepad(id).uuid())
    }

    /// The pad's state now (released when it is gone).
    pub(in crate::game) fn state(&self, id: PadId) -> PadState {
        self.gilrs
            .as_ref()
            .and_then(|g| g.connected_gamepad(id))
            .map_or_else(PadState::default, |pad| state_of(&pad))
    }

    /// Its glyphs (Xbox, PlayStation, Nintendo), as `read` finds them for the active pad.
    pub(in crate::game) fn style(&self, id: PadId) -> GlyphStyle {
        self.gilrs
            .as_ref()
            .and_then(|g| g.connected_gamepad(id))
            .map_or(GlyphStyle::Auto, |pad| {
                GlyphStyle::of_device(pad.vendor_id(), pad.product_id())
            })
    }

    pub(in crate::game) fn device(&self, id: PadId) -> Option<Device> {
        self.gilrs
            .as_ref()
            .and_then(|g| g.connected_gamepad(id))
            .map(|pad| device_of(&pad))
    }

    pub(in crate::game) fn name(&self, id: PadId) -> String {
        self.gilrs
            .as_ref()
            .and_then(|g| g.connected_gamepad(id))
            .map_or_else(|| "A controller".into(), |pad| pad.name().to_owned())
    }
}

impl Game<'_> {
    /// Controllers are turned off: dropping the DualSense's output frees its triggers.
    pub(super) fn pad_output_off(&mut self) {
        self.pad.devices.output = None;
        self.pad.devices.shown = None;
    }

    /// After each poll: the DualSense's output opens once this window has the pad in hand, then
    /// shows what the game is doing; vibrations that are over stop.
    pub(super) fn pad_output_frame(&mut self) {
        if self.pad.devices.dualsense
            && self.pad.in_use
            && self.pad_has_window()
            && self.pad.devices.output.is_none()
        {
            self.pad.devices.output = Some(horizon_dualsense::Link::start());
        }
        self.pad_haptics();
        self.pad_show();
        let now = Instant::now();
        self.pad.devices.rumbles.retain(|(_, end)| *end > now);
    }

    /// The DualSense's HID output, while it is the active pad, open, and this window has focus
    /// (another game on the same pad keeps it otherwise).
    fn pad_output(&self) -> Option<&horizon_dualsense::Link> {
        let output = self.pad.devices.output.as_ref()?;
        (self.pad.devices.dualsense && self.pad_has_window() && output.open().is_some())
            .then_some(output)
    }

    /// What the DualSense shows: while playing, its triggers feel what their actions do; in
    /// menus, paused, unfocused or with another pad active, they are free.
    fn pad_show(&mut self) {
        if self.pad.devices.output.is_none() {
            return;
        }
        let playing = self.pad.devices.dualsense
            && self.pad_has_window()
            && self.title.is_none()
            && self.screen == Screen::Playing
            && !self.screen_over_playing()
            && !self.pause;
        let (using, attacking) = if playing {
            self.trigger_feels()
        } else {
            (TriggerFeel::Free, TriggerFeel::Free)
        };
        // The lightbar follows the player; menus keep the last colour.
        let in_world = self.title.is_none();
        let lightbar = match self.player.as_mut().filter(|_| in_world) {
            Some(p) => {
                let max = p.attribute("minecraft:max_health").unwrap_or(20.0) as f32;
                let health = p.health.health() / max.max(1.0);
                self.settings
                    .controller
                    .lightbar
                    .colour(health, self.dim.id())
            }
            None => self.pad.devices.shown.and_then(|s| s.lightbar),
        };
        let lights = self.settings.controller.lightbar != gamepad::Lightbar::Off;
        let state = horizon_dualsense::device::State {
            triggers: self.settings.controller.trigger_effects(using, attacking),
            lightbar: lightbar.filter(|_| lights),
            player_leds: lights.then_some(horizon_dualsense::PLAYER_1),
        };
        if self.pad.devices.shown != Some(state)
            && let Some(output) = &self.pad.devices.output
        {
            output.set(state);
            self.pad.devices.shown = Some(state);
        }
    }

    /// What `key.use` and `key.attack` are doing, for the triggers bound to them.
    fn trigger_feels(&self) -> (TriggerFeel, TriggerFeel) {
        let using = match self.using {
            Some((slot, ticks)) => match self.stack_in(slot) {
                Some(s) if &*s.item == "minecraft:bow" => {
                    TriggerFeel::Draw(gamepad::bow_power(ticks))
                }
                // TridentItem.THROW_THRESHOLD_TIME.
                Some(s) if &*s.item == "minecraft:trident" => {
                    TriggerFeel::Draw(ticks as f32 / 10.0)
                }
                Some(s) if &*s.item == "minecraft:crossbow" => {
                    TriggerFeel::Draw(ticks as f32 / Self::charge_ticks(s).max(1) as f32)
                }
                Some(s) if self.blocks_attacks(s).is_some() => TriggerFeel::Brace,
                _ => TriggerFeel::Free,
            },
            None => self.idle_use_feel(),
        };
        // Creative breaks at once; blocks that can't be broken have a negative hardness.
        let attacking = match &self.mining {
            Some(m) if !self.creative => {
                let state = self.world.block(m.pos[0], m.pos[1], m.pos[2]);
                TriggerFeel::Texture(self.res.facts().get(state).hardness)
            }
            _ => TriggerFeel::Free,
        };
        (using, attacking)
    }

    /// The use trigger while nothing is in use: a loaded crossbow clicks, an item that boosts (a
    /// rocket while gliding, the stick of the mount being ridden) catches, a shield weighs, and
    /// locks while an axe has disabled it.
    fn idle_use_feel(&self) -> TriggerFeel {
        let gliding = self.player.as_ref().is_some_and(|p| p.fall_flying);
        let mount = self
            .riding
            .and_then(|id| self.mobs.iter().find(|m| m.id == id));
        let mount = mount.map_or("", |m| m.kind.id.as_str());
        let feel = |stack: Option<&horizon_client::items::ItemStack>| match stack
            .map(|s| (s, &*s.item))
        {
            Some((s, "minecraft:crossbow")) if s.data().is_some_and(|d| !d.charged.is_empty()) => {
                TriggerFeel::Click
            }
            Some((_, "minecraft:firework_rocket")) if gliding => TriggerFeel::Boost,
            Some((_, "minecraft:carrot_on_a_stick")) if mount == "minecraft:pig" => {
                TriggerFeel::Boost
            }
            Some((_, "minecraft:warped_fungus_on_a_stick")) if mount == "minecraft:strider" => {
                TriggerFeel::Boost
            }
            Some((s, _)) if self.blocks_attacks(s).is_some() => match self.shield_cooldown {
                0 => TriggerFeel::Brace,
                _ => TriggerFeel::Locked,
            },
            _ => TriggerFeel::Free,
        };
        // Use tries the main hand first.
        match feel(self.stack_in(self.selected)) {
            TriggerFeel::Free => feel(self.offhand.as_ref()),
            main => main,
        }
    }

    /// Open the pad's haptics once its output is open over USB (its audio device is there
    /// only then); close them when it goes. Opt-in with `HORIZON_DUALSENSE_HAPTICS=1` until
    /// they are proven on a pad (docs/controllers.md); never for games without sound (tests).
    fn pad_haptics(&mut self) {
        let usb = self.pad.devices.output.as_ref().and_then(|o| o.open())
            == Some(horizon_dualsense::Transport::Usb);
        if !usb {
            self.pad.devices.haptics = None;
            self.pad.devices.haptics_tried = false;
        } else if !self.pad.devices.haptics_tried {
            self.pad.devices.haptics_tried = true;
            if std::env::var_os("HORIZON_DUALSENSE_HAPTICS").is_some_and(|v| v == "1")
                && std::env::var_os("HORIZON_NO_AUDIO").is_none()
            {
                self.pad.devices.haptics = horizon_dualsense::haptics::Haptics::open();
            }
        }
    }

    /// Release the DualSense's output (the game is quitting).
    pub(in crate::game) fn pad_close_output(&mut self) {
        self.pad.devices.haptics = None;
        self.pad.devices.output = None;
    }

    /// Play a vibration on the active pad: a DualSense through its own report, else gilrs' force
    /// feedback, which platforms without it (gilrs on macOS) skip.
    pub(super) fn pad_vibrate(&mut self, rumble: Rumble, pulse: gamepad::Pulse) {
        let level = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        if let Some(haptics) = self
            .pad
            .devices
            .haptics
            .as_ref()
            .filter(|_| self.pad_output().is_some())
        {
            let steps = rumble.shape().into_iter().map(|step| {
                (
                    step.strong,
                    step.weak,
                    Duration::from_millis(u64::from(step.millis)),
                )
            });
            haptics.play(&steps.collect::<Vec<_>>());
            return;
        }
        if let Some(output) = self.pad_output() {
            let steps = rumble.shape().into_iter().map(|step| {
                let length = Duration::from_millis(u64::from(step.millis));
                ([level(step.strong), level(step.weak)], length)
            });
            output.rumble(steps.collect());
            return;
        }
        // Splitscreen's router holds the only reader, and plays this after the player's turn.
        if self.pad.feed.is_some() && self.pad.devices.gilrs.is_none() {
            self.pad.devices.routed_rumbles.push(pulse);
            return;
        }
        let (Some(gilrs), Some(id)) = (self.pad.devices.gilrs.as_mut(), self.pad.devices.active)
        else {
            return;
        };
        if let Some(playing) = play_pulse(gilrs, id, pulse) {
            self.pad.devices.rumbles.push(playing);
        }
    }

    pub(super) fn pad_devices_status(&self) -> Value {
        self.pad.devices.status()
    }
}

#[cfg(test)]
mod one_reader_tests {
    use super::*;

    /// Whether a machine can open gilrs at all; a test without it has nothing to check.
    fn reader() -> Option<gilrs::Gilrs> {
        gilrs_builder().build().ok()
    }

    #[test]
    fn the_router_takes_a_players_rumble_even_with_no_pad_to_play_it_on() {
        let mut pads = Pads::default();
        pads.devices.routed_rumbles.push(gamepad::Pulse {
            strong: 1.0,
            weak: 1.0,
            millis: 100,
        });
        PadRouter::default().play_rumbles(&mut pads, None);
        assert!(pads.devices.routed_rumbles.is_empty(), "it can't queue up");
    }

    #[test]
    fn routing_takes_p1s_reader_and_gives_it_back_without_closing_it() {
        let Some(gilrs) = reader() else {
            return;
        };
        let mut pads = Pads::default();
        pads.devices.gilrs = Some(gilrs);
        pads.devices.started = true;
        let mut router = PadRouter::default();

        router.adopt(&mut pads);
        assert!(pads.devices.gilrs.is_none(), "P1 no longer holds it");
        assert!(router.gilrs.is_some(), "the router holds the same reader");
        assert!(router.started, "so the router won't open a second");

        pads.release_hardware();
        router.hand_back(&mut pads);
        assert!(router.gilrs.is_none());
        assert!(pads.devices.gilrs.is_some(), "P1 holds it again");
        assert!(pads.devices.started, "so P1 won't open a second");
    }

    /// While the router has the reader, `pad status` still says controllers can be read.
    #[test]
    fn status_sees_the_routers_reader_until_it_comes_back() {
        let Some(gilrs) = reader() else {
            return;
        };
        let mut pads = Pads::default();
        pads.devices.gilrs = Some(gilrs);
        pads.devices.started = true;
        let mut router = PadRouter::default();
        let backend = |pads: &Pads| pads.devices.status()["backend"].as_bool().unwrap();

        router.adopt(&mut pads);
        assert!(!backend(&pads), "nothing reported yet");
        router.report_status(&mut pads, None);
        assert!(backend(&pads), "the router's reader counts");

        router.hand_back(&mut pads);
        assert!(pads.devices.routed.is_none(), "P1 reads its own again");
        assert!(backend(&pads));
    }

    #[test]
    fn a_router_with_its_own_reader_leaves_a_lent_ones_owner_alone() {
        let (Some(own), Some(other)) = (reader(), reader()) else {
            return;
        };
        let mut pads = Pads::default();
        pads.devices.gilrs = Some(other);
        let mut router = PadRouter {
            gilrs: Some(own),
            started: true,
            ..Default::default()
        };
        router.adopt(&mut pads);
        assert!(pads.devices.gilrs.is_some());
    }
}

#[cfg(test)]
mod takeover_tests {
    use super::*;
    use gilrs::Axis as A;

    #[test]
    fn an_idle_pads_resting_trigger_doesnt_take_over() {
        for axis in [A::LeftZ, A::RightZ, A::Unknown] {
            assert!(!axis_takes_over(axis, -0.99, false), "{axis:?}");
            assert!(axis_takes_over(axis, 0.9, false), "{axis:?}");
        }
    }

    #[test]
    fn a_stick_takes_over_in_either_direction() {
        for axis in [A::LeftStickX, A::LeftStickY, A::RightStickX, A::RightStickY] {
            assert!(axis_takes_over(axis, -0.9, false), "{axis:?}");
            assert!(axis_takes_over(axis, 0.9, false), "{axis:?}");
            assert!(!axis_takes_over(axis, 0.2, false), "{axis:?}");
        }
    }

    #[test]
    fn a_right_stick_on_z_takes_over_in_either_direction() {
        // An Xbox pad over Bluetooth pushed left, or up.
        assert!(axis_takes_over(A::LeftZ, -0.9, true));
        assert!(axis_takes_over(A::RightZ, -0.9, true));
        assert!(
            !axis_takes_over(A::Unknown, -0.99, true),
            "its triggers rest here"
        );
    }
}
