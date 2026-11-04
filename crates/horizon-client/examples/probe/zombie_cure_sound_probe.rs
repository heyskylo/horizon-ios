#![forbid(unsafe_code)]
//! Replays tools/test_zombie_cure_sound_parity.py without prepared resources.
use horizon_client::{
    Result,
    mobs::{Anim, Brain, Flags, Mob, MobEvent, MobType, Part, Species, zombie_villager},
};
use horizon_core::{
    geometry::Vec3,
    random::{LegacyRandom, RandomSource},
};
use std::{
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
    sync::Arc,
};

fn kind(eye_height: f64) -> Arc<MobType> {
    Arc::new(MobType {
        id: "minecraft:zombie_villager".into(),
        species: Species::ZombieVillager,
        width: 0.6,
        height: 1.95,
        eye_height,
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
        anim: Anim::Zombie,
        sounds: [""; 3],
        step_sound: None,
        food: Arc::default(),
        tempt: Arc::default(),
    })
}

pub fn main() -> Result<()> {
    let args: Vec<String> = crate::args().collect();
    let mut out = BufWriter::new(File::create(&args[2])?);
    for line in BufReader::new(File::open(&args[1])?).lines() {
        let line = line?;
        let p: Vec<&str> = line.split_whitespace().collect();
        let mut m = Mob::new(
            1,
            kind(p[5].parse()?),
            Vec3::new(p[2].parse()?, p[3].parse()?, p[4].parse()?),
            0.0,
        );
        m.random = LegacyRandom::new(p[0].parse()?);
        m.silent = p[1] == "1";
        if let Some(MobEvent::Sound(name, at, volume, pitch)) = zombie_villager::cure_sound(&mut m)
        {
            write!(
                out,
                "minecraft:{name} {:016x} {:016x} {:016x} {:08x} {:08x}",
                at[0].to_bits(),
                at[1].to_bits(),
                at[2].to_bits(),
                volume.to_bits(),
                pitch.to_bits()
            )?;
        } else {
            write!(out, "silent")?;
        }
        writeln!(out, " next {}", m.random.next_long())?;
    }
    out.flush()?;
    Ok(())
}
