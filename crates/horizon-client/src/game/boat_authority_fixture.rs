//! The native parity harness runs the actual client boat ticker without a
//! renderer, on an empty world; movement ownership changes between ticks.
use super::{Game, boats::Boat};
use horizon_client::Result;
use horizon_core::geometry::Vec3;
use serde::Deserialize;
use serde_json::json;
use std::path::Path;

#[derive(Deserialize)]
struct Case {
    name: String,
    velocity: [f64; 3],
    spin: f32,
    #[serde(default)]
    owned: Vec<bool>,
    #[serde(default)]
    paddles: Option<PaddleCase>,
}

#[derive(Deserialize)]
struct PaddleCase {
    initial: [f32; 2],
    steps: Vec<PaddleStep>,
}
#[derive(Deserialize)]
struct PaddleStep {
    occupied: bool,
    #[serde(default)]
    owned: bool,
    #[serde(default = "is_player")]
    first_player: bool,
    flags: [bool; 2],
}

fn is_player() -> bool {
    true
}

impl Game<'_> {
    pub(super) fn run_boat_authority_fixture(&mut self, directory: &Path) -> Result<()> {
        let cases: Vec<Case> =
            serde_json::from_str(&std::fs::read_to_string(directory.join("inputs.json"))?)?;
        let mut output = Vec::new();
        for case in cases {
            self.boats.clear();
            let mut boat = Boat::new(1, "oak".into(), Vec3::new(0.0, 100.0, 0.0), 0.0);
            let [x, y, z] = case.velocity;
            boat.velocity = Vec3::new(x, y, z);
            boat.delta_rotation = case.spin;
            self.boats.push(boat);
            let mut steps = Vec::new();
            if let Some(paddles) = case.paddles {
                self.riding = None;
                self.boats[0].paddles = paddles.initial;
                for step in paddles.steps {
                    self.boats[0].paddling = step.flags;
                    self.boats[0].paddle_controller = step.occupied && step.first_player;
                    self.riding = step.owned.then_some(1);
                    if step.owned {
                        self.boats[0].input = [
                            step.flags[1] && !step.flags[0],
                            step.flags[0] && !step.flags[1],
                            step.flags[0] && step.flags[1],
                            false,
                        ];
                        let boat = self.tick_boat(self.boats[0].clone(), true);
                        self.boats[0] = boat;
                    }
                    self.tick_remote_boat_bubbles();
                    let boat = &self.boats[0];
                    let rowing = [0.0, 0.25, 0.5, 1.0].map(|a| {
                        [0, 1].map(|side| format!("{:08x}", boat.rowing_time(side, a).to_bits()))
                    });
                    steps.push(json!({
                        "phase": boat.paddles.map(|p| format!("{:08x}", p.to_bits())),
                        "rowing": rowing,
                    }));
                }
                output.push(json!({"name": case.name, "steps": steps}));
                continue;
            }
            for owned in case.owned {
                self.riding = owned.then_some(1);
                if owned {
                    let boat = self.boats.remove(0);
                    let boat = self.tick_boat(boat, true);
                    self.boats.push(boat);
                }
                self.tick_remote_boat_bubbles();
                let boat = &self.boats[0];
                steps.push(json!({
                    "owned": owned,
                    "spin": format!("{:08x}", boat.delta_rotation.to_bits()),
                    "velocity": boat.velocity.components().map(|v| format!("{:016x}", v.to_bits())),
                }));
            }
            output.push(json!({"name": case.name, "steps": steps}));
        }
        std::fs::write(directory.join("rust.json"), serde_json::to_vec(&output)?)?;
        Ok(())
    }
}
