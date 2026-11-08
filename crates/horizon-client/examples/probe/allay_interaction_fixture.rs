//! Client interaction state through the production Allay predictor; no prepared resources.
use horizon_client::{
    Result,
    items::ItemStack,
    mobs::{
        Anim, Brain, Flags, Mob, MobType, Part, Species,
        allay::interaction::{Interaction, predict},
    },
};
use horizon_core::geometry::Vec3;
use std::{io::Write, sync::Arc};

fn kind() -> Arc<MobType> {
    Arc::new(MobType {
        id: "minecraft:allay".into(),
        species: Species::Allay,
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
        anim: Anim::Allay,
        sounds: [""; 3],
        step_sound: None,
        food: Arc::default(),
        tempt: Arc::default(),
    })
}

fn stack(s: Option<&ItemStack>) -> String {
    s.filter(|s| s.count > 0)
        .map_or_else(|| "empty".into(), |s| format!("{}x{}", s.item, s.count))
}

pub fn main() -> Result<()> {
    let args: Vec<String> = crate::args().collect();
    let mut out = std::io::BufWriter::new(std::fs::File::create(&args[1])?);
    for tag in 0..3 {
        for main in [true, false] {
            for item in [
                "minecraft:air",
                "minecraft:stone",
                "minecraft:amethyst_shard",
            ] {
                for holding in [false, true] {
                    for dancing in [false, true] {
                        for ready in [false, true] {
                            for creative in [false, true] {
                                let mut mob = Mob::new(37, kind(), Vec3::ZERO, 0.0);
                                mob.extra.flag = dancing;
                                mob.extra.egg_time = i32::from(!ready);
                                if holding {
                                    mob.extra.set_main_hand_stack(Some(ItemStack::new(
                                        "minecraft:stone",
                                        1,
                                    )));
                                }
                                let mut offer =
                                    (item != "minecraft:air").then(|| ItemStack::new(item, 1));
                                for step in 0..3 {
                                    let duplicates = offer.as_ref().is_some_and(|s| {
                                        (tag != 2 && &*s.item == "minecraft:amethyst_shard")
                                            || (tag == 1 && &*s.item == "minecraft:stone")
                                    });
                                    let result = predict(
                                        &mut mob,
                                        offer.as_ref(),
                                        main,
                                        duplicates,
                                        [0, 0, 0, 11],
                                    );
                                    let sounds = result.as_ref().map_or_else(String::new, |r| {
                                        format!("{}:6:2.0:1.0:true", r.sound())
                                    });
                                    let mut returned = String::new();
                                    match result {
                                        Some(Interaction::Take(s)) => returned = stack(Some(&s)),
                                        Some(Interaction::Duplicate | Interaction::Give)
                                            if !creative =>
                                        {
                                            if let Some(s) = offer.as_mut() {
                                                s.count -= 1;
                                            }
                                            if offer.as_ref().is_some_and(|s| s.count == 0) {
                                                offer = None;
                                            }
                                        }
                                        _ => {}
                                    }
                                    let hand = if main { "MAIN_HAND" } else { "OFF_HAND" };
                                    writeln!(
                                        out,
                                        "{tag} {hand} {item} {holding} {dancing} {ready} {creative} {step} => held={} offer={} can={} liked={} returned=[{returned}] sounds=[{sounds}]",
                                        stack(mob.extra.main_hand_stack()),
                                        stack(offer.as_ref()),
                                        mob.extra.egg_time == 0,
                                        mob.extra.owner.is_some()
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
