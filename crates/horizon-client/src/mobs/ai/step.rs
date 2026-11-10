//! `Mob.playStepSound` and its overrides: the species' own footstep, or the block
//! sound at 15%, and the block sound ids the facts carry.
use super::{Mob, MobWorld, Species};

/// `playStepSound`: the species' footstep, or the block's step sound at 15%.
pub fn step_sound(
    m: &Mob,
    w: &MobWorld,
    pos: [i32; 3],
    state: horizon_world::registry::BlockStateId,
) -> Option<(&'static str, f32, f32)> {
    use Species as S;
    let baby = m.is_baby();
    let quiet = |s: &'static str| Some((s, 0.15, 1.0));
    match m.species() {
        // `playStepSound` overridden with an empty body: a fish, an allay, a bee,
        // a happy ghast and a nautilus have no footfall of their own, so the
        // block's step sound is not taken either.
        S::Allay
        | S::Bee
        | S::Cod
        | S::Salmon
        | S::TropicalFish
        | S::Pufferfish
        | S::Tadpole
        | S::HappyGhast
        | S::Nautilus
        | S::ZombieNautilus => None,
        S::Armadillo => quiet("entity.armadillo.step"),
        S::Chicken | S::Cow | S::Pig | S::Wolf => {
            crate::mobs::finalize::variant_sound(m, "step").and_then(quiet)
        }
        S::Mooshroom => quiet("entity.cow.step"),
        S::Llama | S::TraderLlama => quiet("entity.llama.step"),
        S::Frog => quiet("entity.frog.step"),
        S::Goat => quiet("entity.goat.step"),
        S::Panda => quiet("entity.panda.step"),
        S::Parrot => quiet("entity.parrot.step"),
        S::PolarBear => quiet("entity.polar_bear.step"),
        S::Sheep => quiet("entity.sheep.step"),
        S::Sniffer => quiet("entity.sniffer.step"),
        S::Turtle if baby => quiet("entity.turtle.shamble_baby"),
        S::Turtle => quiet("entity.turtle.shamble"),
        S::Creaking => quiet("entity.creaking.step"),
        S::Hoglin => quiet("entity.hoglin.step"),
        S::Zoglin => quiet("entity.zoglin.step"),
        S::Piglin => quiet("entity.piglin.step"),
        S::PiglinBrute => quiet("entity.piglin_brute.step"),
        S::Ravager => quiet("entity.ravager.step"),
        S::Skeleton => quiet("entity.skeleton.step"),
        S::Stray => quiet("entity.stray.step"),
        S::Bogged => quiet("entity.bogged.step"),
        S::Parched => quiet("entity.parched.step"),
        S::WitherSkeleton => quiet("entity.wither_skeleton.step"),
        S::Spider | S::CaveSpider => quiet("entity.spider.step"),
        // SulfurCube.playStepSound: a cube carrying a block in its body makes no
        // footstep at all; an empty one takes the block's sound at the bottom.
        S::SulfurCube if crate::mobs::sulfur_cube::has_body(m) => None,
        // Silverfish.playStepSound and Endermite.playStepSound: both ask for
        // their own step at 15%, though neither type's movement emission plays
        // it (`movement_sounds`).
        S::Silverfish => quiet("entity.silverfish.step"),
        S::Endermite => quiet("entity.endermite.step"),
        S::Zombie | S::ZombifiedPiglin => quiet("entity.zombie.step"),
        S::Husk => quiet("entity.husk.step"),
        S::Drowned => quiet("entity.drowned.step"),
        S::ZombieVillager => quiet("entity.zombie_villager.step"),
        // Camel.playStepSound and CamelHusk.playStepSound: sand and concrete
        // powder have their own step.
        S::CamelHusk | S::Camel => {
            let sand = w.has_tag(
                "minecraft:camel_sand_step_sound_blocks",
                &w.facts.get(state).name,
            );
            Some(match (m.species(), sand) {
                (S::Camel, true) => ("entity.camel.step_sand", 1.0, 1.0),
                (S::Camel, false) => ("entity.camel.step", 1.0, 1.0),
                (_, true) => ("entity.camel_husk.step_sand", 0.4, 1.0),
                (_, false) => ("entity.camel_husk.step", 0.4, 1.0),
            })
        }
        S::IronGolem => Some(("entity.iron_golem.step", 1.0, 1.0)),
        // CopperGolem.playStepSound: its oxidation level's step.
        S::CopperGolem => Some((crate::mobs::copper_golem::sound(m, "step"), 1.0, 1.0)),
        S::Strider if m.fluids.in_lava => Some(("entity.strider.step_lava", 1.0, 1.0)),
        S::Strider => Some(("entity.strider.step", 1.0, 1.0)),
        S::Warden => Some(("entity.warden.step", 10.0, 1.0)),
        _ => {
            // Entity.playStepSound: the block's sound type.
            let _ = pos;
            let sounds = &w.facts.get(state).sounds;
            let step: &'static str = intern_sound(&sounds.step);
            Some((step, sounds.volume * 0.15, sounds.pitch))
        }
    }
}
/// A block sound id as a static string (the block facts intern them).
pub(crate) fn intern_sound(s: &std::sync::Arc<str>) -> &'static str {
    use std::{collections::HashSet, sync::Mutex};
    static NAMES: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    let set = names.get_or_insert_with(HashSet::new);
    let key = s.strip_prefix("minecraft:").unwrap_or(s);
    if let Some(found) = set.get(key) {
        return found;
    }
    let leaked: &'static str = Box::leak(key.to_owned().into_boxed_str());
    set.insert(leaked);
    leaked
}

