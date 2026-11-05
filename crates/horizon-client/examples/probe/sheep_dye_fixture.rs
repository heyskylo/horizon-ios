//! Actual production sheep-dye interaction without prepared resources.
use horizon_client::{
    Result,
    items::ItemStack,
    mobs::{Anim, Brain, Flags, Mob, MobType, Part, Species, sheep_dye},
};
use horizon_core::geometry::Vec3;
use horizon_world::nbt::Tag;
use std::{io::Write, sync::Arc};

fn kind() -> Arc<MobType> {
    Arc::new(MobType {
        id: "minecraft:sheep".into(),
        species: Species::Sheep,
        width: 0.6,
        height: 1.95,
        eye_height: 0.36,
        category: "monster".into(),
        max_health: 20.0,
        speed: 0.23,
        flying_speed: 0.4,
        attack_damage: 3.0,
        attack_knockback: 0.0,
        follow_range: 35.0,
        step_height: 0.6,
        armor: 2.0,
        armor_toughness: 0.0,
        knockback_resistance: 0.0,
        jump_strength: 0.42,
        attributes: Arc::default(),
        item_stack_limits: Arc::default(),
        gravity: 0.08,
        safe_fall_distance: 3.0,
        fall_damage_multiplier: 1.0,
        water_efficiency: 0.0,
        water_slowdown: 0.8,
        air_speed: 0.02,
        baby_scale: 0.5,
        head_rot_speed: 10.0,
        max_head_x_rot: 40.0,
        max_head_y_rot: 75.0,
        ambient_interval: 120,
        flags: Flags::default(),
        model: Arc::new(Part {
            name: "root".into(),
            pose: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            polygons: Vec::new(),
            children: Vec::new(),
        }),
        layers: Default::default(),
        texture: String::new(),
        brain: Brain::Passive,
        anim: Anim::Quadruped,
        sounds: [""; 3],
        step_sound: None,
        food: Arc::default(),
        tempt: Arc::default(),
    })
}

pub fn main() -> Result<()> {
    let args: Vec<String> = crate::args().collect();
    let mut out = std::io::BufWriter::new(std::fs::File::create(&args[1])?);
    for initial in [0, 7, 14, 15] {
        for hand in ["MAIN_HAND", "OFF_HAND"] {
            for item in [
                "minecraft:white_dye",
                "minecraft:red_dye",
                "minecraft:black_dye",
            ] {
                for patch in 0..3 {
                    for sheared in [false, true] {
                        for alive in [false, true] {
                            for client in [false, true] {
                                let mut mob = Mob::new(37, kind(), Vec3::ZERO, 0.0);
                                mob.extra.color = initial;
                                mob.extra.sheared = sheared;
                                if !alive {
                                    mob.health = 0.0;
                                }
                                let mut stack = ItemStack::new(item, 2);
                                if patch == 1 {
                                    stack.data_mut().set_other(
                                        "minecraft:dye",
                                        Some(Tag::String("blue".into())),
                                    );
                                }
                                if patch == 2 {
                                    stack.data_mut().set_other(
                                        "!minecraft:dye",
                                        Some(Tag::Compound(Default::default())),
                                    );
                                }
                                for step in 0..2 {
                                    let result = sheep_dye::interact(&mut mob, &mut stack, client);
                                    let sound = if result {
                                        "minecraft:item.dye.use:7:1.0:1.0:true"
                                    } else {
                                        ""
                                    };
                                    writeln!(
                                        out,
                                        "{initial} {hand} {item} {patch} {sheared} {alive} {client} {step} => result={result} color={} count={} sounds=[{sound}]",
                                        mob.extra.color, stack.count
                                    )?;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
