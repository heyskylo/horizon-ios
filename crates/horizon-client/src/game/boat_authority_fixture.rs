//! The native parity harness runs the client boat ticker and authoritative
//! boat attacks without a renderer, on an empty world.
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
    owned: Vec<bool>,
    #[serde(default)]
    attack: Option<Attack>,
}

#[derive(Deserialize)]
struct Attack {
    yaw: f32,
    sprint: bool,
    ticks: u32,
    player_velocity: [f64; 3],
    #[serde(default)]
    knockback: u8,
    #[serde(default)]
    creative: bool,
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
            if let Some(attack) = case.attack {
                let mut player =
                    horizon_client::motion::PlayerMotion::new(self.motion, Vec3::ZERO)?;
                player.velocity = Vec3::new(
                    attack.player_velocity[0],
                    attack.player_velocity[1],
                    attack.player_velocity[2],
                );
                player.set_sprinting(attack.sprint)?;
                self.player = Some(player);
                self.inventory.fill(None);
                self.selected = 0;
                if attack.knockback > 0 {
                    self.inventory[0] = horizon_client::item_data::parse_stack(&format!(
                        r#"{{id:"minecraft:stick",count:1,components:{{"minecraft:enchantments":{{"minecraft:knockback":{}}}}}}}"#,
                        attack.knockback
                    ))?;
                }
                self.yaw = attack.yaw;
                self.attack_ticks = attack.ticks;
                self.creative = attack.creative;
                self.strike_boat(0);
                let player = self.player.as_ref().unwrap();
                output.push(json!({
                    "name": case.name,
                    "velocity": self.boats.first().map(|boat| boat.velocity.components().map(|v| format!("{:016x}", v.to_bits()))),
                    "destroyed": self.boats.is_empty(),
                    "player_velocity": player.velocity.components().map(|v| format!("{:016x}", v.to_bits())),
                    "sprint": player.sprinting,
                }));
                continue;
            }
            let mut steps = Vec::new();
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
