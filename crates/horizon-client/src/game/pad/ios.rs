//! Controllers on iPhone and iPad, read through `horizon-gamecontroller` (Apple's GameController framework,
//! behind a safe API because the game forbids `unsafe`). Read only: no rumble, lights or adaptive triggers yet,
//! so the output half is the same no-op as a build without controllers.
use super::{super::Game, PadEvent, Pads};
use horizon_client::gamepad::{Button, Edges, GlyphStyle, PadState, Pulse, Rumble};
use serde_json::{Value, json};

/// How many controllers the last poll saw, to log connections the way gilrs' events do.
#[derive(Default)]
pub(super) struct Devices {
    connected: usize,
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
    GlyphStyle::of_vendor(vendor)
}

impl Pads {
    /// Read the active pad.
    pub(super) fn read(&mut self) -> PadState {
        let controllers = horizon_gamecontroller::controllers();
        if controllers.len() != self.devices.connected {
            for controller in &controllers {
                let name = controller.name.as_deref().unwrap_or(&controller.category);
                log::info!("Controller connected: {name}");
            }
            self.devices.connected = controllers.len();
        }
        let Some(pad) = horizon_gamecontroller::active() else {
            return PadState::default();
        };
        self.style = style(&pad.category);
        state_of(&pad)
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
}

impl PadRouter {
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
    pub(in crate::game) fn connected(&self, id: PadId) -> bool {
        self.pads.contains_key(&id)
    }
    pub(in crate::game) fn uuid(&self, _: PadId) -> [u8; 16] {
        // Product categories are not hardware identities. Do not guess that
        // reconnecting a second identical pad should take a missing seat.
        [0; 16]
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
    }
}

impl Game<'_> {
    pub(super) fn pad_output_off(&mut self) {}
    pub(super) fn pad_output_frame(&mut self) {}
    pub(in crate::game) fn pad_close_output(&mut self) {}
    pub(super) fn pad_vibrate(&mut self, _: Rumble, _: Pulse) {}
    pub(super) fn pad_devices_status(&self) -> Value {
        let pads: Vec<Value> = horizon_gamecontroller::controllers()
            .into_iter()
            .map(|c| json!({ "name": c.name, "category": c.category, "gamepad": c.gamepad }))
            .collect();
        json!({ "backend": true, "controllers": pads })
    }
}
