//! Scoped Player.attack follow-up probe; invoked by the dispenser's headless world.
use super::{Game, boats::Boat, dragon::Dragon, minecarts::{CartKind, Minecart}};
use horizon_client::{Result, entities::{ArmorStand, ItemFrame, Painting}, item_data::parse_stack,
    motion::PlayerMotion};
use horizon_core::geometry::Vec3;
use serde_json::Value;

impl Game<'_> {
    pub(super) fn fixture_special_attack(&mut self, op: &Value) -> Result<Vec<String>> {
        let kind = op["target"].as_str().ok_or("special attack target missing")?;
        let creative = op["creative"].as_bool().unwrap_or(false);
        let mut player = PlayerMotion::new(self.motion, Vec3::new(0.5, 0.0, -2.0))?;
        player.invulnerable = creative;
        player.may_fly = creative;
        player.ground.on_ground = true;
        player.velocity = Vec3::new(0.1, 0.0, 0.1);
        player.set_sprinting(op["sprint"].as_bool().unwrap_or(false))?;
        self.player = Some(player);
        self.mode = if creative { horizon_client::command::args::GameMode::Creative }
            else { horizon_client::command::args::GameMode::Survival };
        self.creative = creative;
        self.inventory.fill(None);
        self.inventory[self.selected] = parse_stack(op["item"].as_str().unwrap_or("-"))?;
        self.offhand = None;
        self.attack_ticks = op["ticks"].as_u64().unwrap_or(20) as u32;
        self.world.game_time = 100;
        let at = Vec3::new(0.5, 1.0, 0.5);
        let center = match kind {
            "boat" => {
                self.boats.push(Boat::new(1, "oak".into(), at, 0.0));
                self.boats[0].bounds().center()
            }
            "cart" => {
                self.carts.push(Minecart::new(1, CartKind::Rideable, at));
                self.carts[0].bounds().center()
            }
            "stand" => {
                let mut stand = ArmorStand::new(1, Vec3::new(0.5, 0.0, 0.5), 0.0);
                stand.last_hit = if op["break"].as_bool().unwrap_or(false) { 100 } else { 0 };
                self.armor_stands.push(stand);
                self.armor_stands[0].bounds().center()
            }
            "dragon" => {
                let mut dragon = Dragon::new(Vec3::ZERO, 0.0, 0);
                dragon.parts.fill((Vec3::new(100.0, 100.0, 100.0), Vec3::ZERO));
                dragon.parts[0] = (at, at);
                self.dragon = Some(dragon);
                self.special_targets()[0].1.center()
            }
            "painting" => {
                self.paintings.push(Painting { id: 1, pos: [0, 1, 0], facing: 2,
                    variant: "minecraft:kebab".into(), width: 1, height: 1, check_interval: 0 });
                let (lo, hi) = self.paintings[0].bounds();
                Vec3::new((lo[0]+hi[0])/2.0, (lo[1]+hi[1])/2.0, (lo[2]+hi[2])/2.0)
            }
            "frame" | "fixed_frame" | "filled_frame" => {
                self.item_frames.push(ItemFrame { id: 1, pos: [0,1,0], facing: 2,
                    glow: false, item: if kind == "filled_frame" {
                        Some(horizon_client::items::ItemStack::new("minecraft:stone", 1))
                    } else { None }, rotation: 0, drop_chance: 1.0,
                    fixed: kind == "fixed_frame", invisible: false, check_interval: 0, custom_name: None });
                let (lo, hi) = self.item_frames[0].bounds();
                Vec3::new((lo[0]+hi[0])/2.0, (lo[1]+hi[1])/2.0, (lo[2]+hi[2])/2.0)
            }
            _ => return Err(format!("unknown special attack target {kind}").into()),
        };
        let eye = self.eye_position().ok_or("special attack player missing")?;
        let dx = center.x-eye[0];
        let dy = center.y-eye[1];
        let dz = center.z-eye[2];
        self.yaw = (-dx).atan2(dz).to_degrees() as f32;
        self.pitch = (-dy).atan2((dx*dx+dz*dz).sqrt()).to_degrees() as f32;
        self.attack();
        let player = self.player.take().ok_or("special attack player missing")?;
        Ok(vec![format!("special {:08x} {:016x} {:016x} {} {}",
            player.health.food.exhaustion.to_bits(), player.velocity.x.to_bits(), player.velocity.z.to_bits(),
            player.sprinting, self.attack_ticks)])
    }
}
