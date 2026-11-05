//! Motion through the same avatar-state helper the real remote entity tick calls.
#[path = "../../src/game/remote/avatar_animation.rs"]
mod avatar_animation;

use horizon_client::mobs::{Mob, MobTypes};
use serde::Deserialize;
use serde_json::json;
use std::{fs, io::Write, path::PathBuf, sync::Arc};

#[derive(Deserialize)]
struct Motion {
    position: [f64; 3],
    velocity: [f64; 3],
    #[serde(default)]
    pose: u8,
    #[serde(default)]
    flying: bool,
    #[serde(default)]
    sneaking: bool,
    #[serde(default)]
    in_water: bool,
}
#[derive(Deserialize)]
struct Scenario {
    name: String,
    entity: String,
    player: bool,
    running: bool,
    alive: bool,
    on_ground: bool,
    motions: Vec<Motion>,
}

pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<PathBuf> = crate::args_os().skip(1).map(PathBuf::from).collect();
    let [input, resources, output] = args.as_slice() else {
        return Err("usage: mannequin_avatar_fixture INPUT RESOURCES OUTPUT".into());
    };
    let scenarios: Vec<Scenario> = serde_json::from_slice(&fs::read(input)?)?;
    let types = MobTypes::load_for_server(resources)?;
    let mut out = fs::File::create(output)?;
    for scenario in scenarios {
        // Remote player classification comes from the network's player map,
        // not Species (the production player type uses Zombie too). The
        // state helper reads no model/AI data, so these scalar player controls
        // use vanilla zombie facts under the player id without loading models.
        let mut kind = types.types[if scenario.player {
            "minecraft:zombie"
        } else {
            &scenario.entity
        }]
        .clone();
        kind.id = scenario.entity.clone();
        let mut mob = Mob::new(1, Arc::new(kind), Default::default(), 0.0);
        mob.on_ground = scenario.on_ground;
        mob.extra.immovable = false;
        if !scenario.alive {
            mob.health = 0.0;
        }
        for (step, motion) in scenario.motions.into_iter().enumerate() {
            mob.previous = mob.position;
            (mob.position.x, mob.position.y, mob.position.z) =
                (motion.position[0], motion.position[1], motion.position[2]);
            (mob.velocity.x, mob.velocity.y, mob.velocity.z) =
                (motion.velocity[0], motion.velocity[1], motion.velocity[2]);
            let flags =
                (if motion.flying { 0x80 } else { 0 }) | (if motion.sneaking { 2 } else { 0 });
            avatar_animation::apply(
                &mut mob,
                &[
                    (
                        0,
                        0,
                        horizon_protocol::entity_data::Value::Byte(flags as i8),
                    ),
                    (
                        6,
                        0,
                        horizon_protocol::entity_data::Value::Id(i32::from(motion.pose)),
                    ),
                ],
            );
            mob.fluids.in_water = motion.in_water;
            avatar_animation::tick(&mut mob, scenario.player, scenario.running);
            let state = &mob.extra.avatar;
            writeln!(
                out,
                "{}",
                json!({
                    "case": scenario.name,
                    "step": step,
                    "cloak": state.cloak,
                    "old": state.cloak_o,
                    "bob_bits": state.bob.to_bits(),
                    "bob_old_bits": state.bob_o.to_bits(),
                    "walk_bits": state.walk_dist.to_bits(),
                    "walk_old_bits": state.walk_dist_o.to_bits(),
                    "wing_bits": mob.extra.elytra.angles.map(f32::to_bits),
                    "wing_old_bits": mob.extra.elytra.old.map(f32::to_bits),
                    "wing_half_bits": mob.extra.elytra.angles(0.5).map(f32::to_bits),
                    "fall_fly_ticks": mob.extra.fall_fly_ticks,
                    "swim_bits": mob.extra.swim.amount.to_bits(),
                    "swim_old_bits": mob.extra.swim.old.to_bits(),
                    "swim_half_bits": mob.extra.swim.amount(0.5).to_bits(),
                    "visual_crouching": mob.visual_crouching(),
                })
            )?;
        }
    }
    Ok(())
}
