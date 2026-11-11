//! Controllers on iPhone and iPad, read through `horizon-gamecontroller` (Apple's GameController framework,
//! behind a safe API because the game forbids `unsafe`). Read only: no rumble, lights or adaptive triggers yet,
//! so the output half is the same no-op as a build without controllers.
use super::{super::Game, Connection, PadEvent, Pads};
use horizon_client::controller_ui::Device;
use horizon_client::gamepad::{Button, Edges, GlyphStyle, PadState, Pulse, Rumble};
use serde_json::{Value, json};

/// How many controllers the last poll saw, to log connections the way gilrs' events do.
#[derive(Default)]
pub(super) struct Devices {
    connected: usize,
    active: Option<PadId>,
    known: Vec<PadId>,
    started: bool,
    snapshots: Vec<horizon_gamecontroller::ConnectedController>,
    routed: Option<Value>,
}

/// The glyphs to draw, from the product category iOS reports, through the vendor ids gilrs' pads are
/// matched by.
fn style(category: &str) -> GlyphStyle {
    let vendor = if category.contains("DualSense") || category.contains("DualShock") {
        Some(0x054c)
    } else if category.contains("Switch") || category.contains("Joy-Con") {
        Some(0x057e)
    } else {
        None
    };
    GlyphStyle::of_device(vendor, None)
}

fn device_of(controller: &horizon_gamecontroller::Controller) -> Device {
    let vendor = match style(&controller.category) {
        GlyphStyle::PlayStation => Some(0x054c),
        GlyphStyle::Nintendo => Some(0x057e),
        _ => None,
    };
    Device {
        name: controller
            .name
            .clone()
            .unwrap_or_else(|| controller.category.clone()),
        vendor,
        // GameController exposes a product category, not a USB product ID.
        product: None,
    }
}

impl Pads {
    /// Read the active pad.
    pub(super) fn read(&mut self) -> PadState {
        let controllers = horizon_gamecontroller::snapshots();
        let selected = horizon_gamecontroller::active_controller();
        self.read_snapshots(controllers, selected)
    }

    fn read_snapshots(
        &mut self,
        controllers: Vec<horizon_gamecontroller::ConnectedController>,
        selected: Option<horizon_gamecontroller::ConnectedController>,
    ) -> PadState {
        let known: Vec<_> = controllers.iter().map(|controller| controller.id).collect();
        if self.devices.started {
            for &id in &self.devices.known {
                if !known.contains(&id) {
                    self.connections.push(Connection::Disconnected {
                        active: self.devices.active == Some(id),
                    });
                }
            }
            for controller in &controllers {
                if !self.devices.known.contains(&controller.id) {
                    self.connections.push(Connection::Connected(style(
                        &controller.controller.category,
                    )));
                }
            }
        }
        self.devices.started = true;
        self.devices.known = known;
        if controllers.len() != self.devices.connected {
            for controller in &controllers {
                let name = controller
                    .controller
                    .name
                    .as_deref()
                    .unwrap_or(&controller.controller.category);
                log::info!("Controller connected: {name}");
            }
            self.devices.connected = controllers.len();
        }
        self.devices.snapshots = controllers.clone();
        self.device = selected
            .as_ref()
            .or_else(|| controllers.first())
            .map(|pad| device_of(&pad.controller));
        let active = selected.as_ref().map(|pad| pad.id);
        if self.devices.active != active {
            // Changing/disconnecting a controller is not its A release.
            self.menu.armed = None;
            self.owner_epoch = self.owner_epoch.wrapping_add(1);
            self.devices.active = active;
        }
        let Some(pad) = selected else {
            return PadState::default();
        };
        self.style = style(&pad.state.category);
        let state = state_of(&pad.state);
        self.peaks = [state.left, state.right];
        state
    }
}

/// The bridge assigns monotonically increasing IDs to native controller objects.
pub(in crate::game) type PadId = u64;

fn state_of(pad: &horizon_gamecontroller::Snapshot) -> PadState {
    let mut state = PadState::default();
    for (button, down) in [
        (Button::South, pad.a),
        (Button::East, pad.b),
        (Button::West, pad.x),
        (Button::North, pad.y),
        (Button::LeftBumper, pad.left_shoulder),
        (Button::RightBumper, pad.right_shoulder),
        (Button::Select, pad.options),
        (Button::Start, pad.menu),
        (Button::LeftStick, pad.left_stick_button),
        (Button::RightStick, pad.right_stick_button),
        (Button::DPadUp, pad.dpad_up),
        (Button::DPadDown, pad.dpad_down),
        (Button::DPadLeft, pad.dpad_left),
        (Button::DPadRight, pad.dpad_right),
    ] {
        state.set(button, down);
    }
    // GameController's sticks are -1..1 with up positive, as gilrs' are.
    state.left = pad.left_stick;
    state.right = pad.right_stick;
    state.triggers = pad.triggers;
    state
}

struct RoutedPad {
    snapshot: horizon_gamecontroller::ConnectedController,
    state: PadState,
    edges: Edges,
}

/// Independent snapshots for upstream splitscreen. The normal/AirPlay path
/// keeps using `active()` above, so another pad's most recent input does not
/// silently move a controller already assigned to a desktop split seat.
#[derive(Default)]
pub(in crate::game) struct PadRouter {
    pads: std::collections::BTreeMap<PadId, RoutedPad>,
    routing: bool,
    active: Option<PadId>,
}

impl PadRouter {
    /// GameController has no exclusive reader or event queue: reads copy current properties.
    /// Transfer the last normal poll instead, so a held button is not a fresh join press.
    pub(in crate::game) fn adopt(&mut self, pads: &mut Pads) {
        if self.routing {
            return;
        }
        self.routing = true;
        self.active = pads.devices.active;
        self.pads.clear();
        self.update(std::mem::take(&mut pads.devices.snapshots));
    }

    /// Return the connection baseline too: the next normal poll must report changes since
    /// routing stopped, without announcing every already-connected pad as newly attached.
    pub(in crate::game) fn hand_back(&mut self, pads: &mut Pads) {
        if !self.routing {
            return;
        }
        self.routing = false;
        let active = pads
            .feed_source
            .or(self.active)
            .filter(|id| self.connected(*id));
        if active != self.active {
            pads.menu.armed = None;
        }
        pads.devices = Devices {
            connected: self.pads.len(),
            active,
            known: self.pads.keys().copied().collect(),
            started: true,
            routed: None,
            snapshots: self.pads.values().map(|pad| pad.snapshot.clone()).collect(),
        };
        pads.device = active.and_then(|id| self.device(id));
        if let Some(id) = active {
            pads.style = self.style(id);
        }
    }

    pub(in crate::game) fn events(&mut self) -> Vec<PadEvent> {
        self.update(horizon_gamecontroller::snapshots())
    }
    fn update(
        &mut self,
        snapshots: Vec<horizon_gamecontroller::ConnectedController>,
    ) -> Vec<PadEvent> {
        let mut events = Vec::new();
        let connected: std::collections::BTreeSet<_> = snapshots.iter().map(|s| s.id).collect();
        self.pads.retain(|id, _| {
            if connected.contains(id) {
                true
            } else {
                events.push(PadEvent::Gone(*id));
                false
            }
        });
        for snapshot in snapshots {
            let id = snapshot.id;
            let state = state_of(&snapshot.state);
            let pad = self.pads.entry(id).or_insert_with(|| {
                events.push(PadEvent::Back(id));
                RoutedPad {
                    snapshot: snapshot.clone(),
                    state: PadState::default(),
                    edges: Edges::default(),
                }
            });
            pad.state = state;
            pad.snapshot = snapshot;
            for (button, pressed) in pad.edges.update(&state, 0.5) {
                if pressed {
                    events.push(PadEvent::Press(id, button));
                }
            }
        }
        events
    }
    // GameController's output is currently read-only, so there is no output
    // owner to lend/reclaim or an exclusive native input handle to release.
    pub(in crate::game) fn lend_output(&mut self, _: &mut Pads, _: PadId) {}
    pub(in crate::game) fn reclaim_output(&mut self, _: &mut Pads) {}
    pub(in crate::game) fn device(&self, id: PadId) -> Option<Device> {
        self.pads
            .get(&id)
            .map(|pad| device_of(&pad.snapshot.controller))
    }
    pub(in crate::game) fn report_status(&self, pads: &mut Pads, active: Option<PadId>) {
        let controllers: Vec<_> = self.pads.iter().map(|(id, pad)| {
            let device = device_of(&pad.snapshot.controller);
            json!({"name": device.name, "vendor": device.vendor, "product": device.product,
                "category": pad.snapshot.controller.category, "gamepad": true, "active": active == Some(*id)})
        }).collect();
        pads.devices.routed = Some(json!({"backend": true, "controllers": controllers}));
    }
    pub(in crate::game) fn connected(&self, id: PadId) -> bool {
        self.pads.contains_key(&id)
    }
    pub(in crate::game) fn uuid(&self, _: PadId) -> [u8; 16] {
        // Product categories are not hardware identities. Do not guess that
        // reconnecting a second identical pad should take a missing seat.
        [0; 16]
    }
    /// Snapshot input supplies one observation per native poll.
    pub(in crate::game) fn peaks(&self, id: PadId) -> [[f32; 2]; 2] {
        let state = self.state(id);
        [state.left, state.right]
    }
    pub(in crate::game) fn state(&self, id: PadId) -> PadState {
        self.pads
            .get(&id)
            .map_or_else(PadState::default, |p| p.state)
    }
    pub(in crate::game) fn name(&self, id: PadId) -> String {
        self.pads.get(&id).map_or_else(
            || "A controller".to_owned(),
            |p| {
                p.snapshot
                    .controller
                    .name
                    .clone()
                    .unwrap_or_else(|| p.snapshot.controller.category.clone())
            },
        )
    }
    pub(in crate::game) fn style(&self, id: PadId) -> GlyphStyle {
        self.pads
            .get(&id)
            .map_or(GlyphStyle::Auto, |p| style(&p.snapshot.controller.category))
    }
}

impl Pads {
    pub(in crate::game) fn release_hardware(&mut self) {
        self.devices = Devices::default();
        self.device = None;
    }
}

impl Game<'_> {
    pub(super) fn pad_output_off(&mut self) {}
    pub(super) fn pad_output_frame(&mut self) {}
    pub(in crate::game) fn pad_close_output(&mut self) {}
    pub(super) fn pad_vibrate(&mut self, _: Rumble, _: Pulse) {}
    pub(super) fn pad_devices_status(&self) -> Value {
        if let Some(status) = &self.pad.devices.routed {
            return status.clone();
        }
        let pads: Vec<Value> = horizon_gamecontroller::controllers()
            .into_iter()
            .map(|c| json!({ "name": c.name, "category": c.category, "gamepad": c.gamepad }))
            .collect();
        json!({ "backend": true, "controllers": pads })
    }
}
