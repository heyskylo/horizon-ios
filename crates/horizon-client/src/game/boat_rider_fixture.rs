//! Headless passenger-order regression through the real remote client paths.
use super::{Game, Keys, boats::Boat};
use horizon_client::{Result, held::Source, motion::PlayerMotion};
use horizon_core::geometry::Vec3;
use horizon_protocol::{connection::Pipe, packets::play::Serverbound};
use serde::Deserialize;
use serde_json::json;
use std::path::Path;
use winit::keyboard::KeyCode;

#[derive(Deserialize)]
struct Case {
    name: String,
    first: Vec<bool>,
    velocity: [f64; 3],
    spin: f32,
    keys: [bool; 4],
    event: i8,
}
impl Game<'_> {
    pub(super) fn run_boat_rider_fixture(&mut self, directory: &Path) -> Result<()> {
        let cases: Vec<Case> =
            serde_json::from_str(&std::fs::read_to_string(directory.join("inputs.json"))?)?;
        let (client, peer) = Pipe::pair();
        self.connect_local(client, "BoatSeatFixture")?;
        self.remote.as_mut().unwrap().entity_id = 1;
        self.player = Some(PlayerMotion::new(self.motion, Vec3::new(0.0, 100.0, 0.0))?);

        let mut output = Vec::new();
        for case in cases {
            self.boats.clear();
            let mut b = Boat::new(3, "oak".into(), Vec3::new(0.0, 100.0, 0.0), 0.0);
            let [x, y, z] = case.velocity;
            b.velocity = Vec3::new(x, y, z);
            b.delta_rotation = case.spin;
            self.keys.release_source(Source::Agent);
            for (key, held) in [KeyCode::KeyA, KeyCode::KeyD, KeyCode::KeyW, KeyCode::KeyS]
                .into_iter()
                .zip(case.keys)
            {
                if held {
                    self.keys.press(key, Source::Agent);
                }
            }
            self.boats.push(b);
            let mut steps = Vec::new();
            for first in case.first {
                self.remote_set_passengers(3, if first { &[1, 2] } else { &[2, 1] });
                let mut accepts_server_moves = false;
                self.tick_remote_objects(
                    |_, p| {
                        accepts_server_moves = true;
                        p
                    },
                    |_, yaw| yaw,
                    |_| false,
                );
                self.remote_ride(
                    Keys {
                        left: case.keys[0],
                        right: case.keys[1],
                        forward: case.keys[2],
                        backward: case.keys[3],
                        ..Keys::default()
                    },
                    7,
                )?;
                let packets = self.remote_vehicle_packets();
                if case.event != 0 {
                    self.remote_boat_event(3, case.event);
                }
                let boat = &self.boats[0];
                steps.push(json!({
                    "controlled": self.remote.as_ref().unwrap().first_passenger,
                    "accepts_server_moves": accepts_server_moves,
                    "hands_busy": self.hands_busy(),
                    "moves": packets.iter().filter(|p| matches!(p, Serverbound::MoveVehicle(_))).count(),
                    "paddles": packets.iter().filter(|p| matches!(p, Serverbound::PaddleBoat(_))).count(),
                    "spin": format!("{:08x}", boat.delta_rotation.to_bits()),
                    "velocity": boat.velocity.components().map(|v| format!("{:016x}", v.to_bits())),
                }));
            }
            output.push(json!({"name": case.name, "steps": steps}));
        }
        peer.shutdown();
        self.remote = None;
        std::fs::write(directory.join("rust.json"), serde_json::to_vec(&output)?)?;
        Ok(())
    }
}
