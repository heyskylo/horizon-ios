//! `PathfinderMob.getWalkTargetValue` and the classes that override it: the score
//! a stroll gives a candidate position, of which it keeps the best of ten.
use super::{Family, Mob, MobWorld, Species, family};
use crate::blocks::Fluid;

pub fn walk_target_value(m: &Mob, w: &MobWorld, p: [i32; 3]) -> f32 {
    let light = || w.brightness(p[0], p[1], p[2]) - 0.5;
    // Guardian.getWalkTargetValue: water first.
    if family(m.species()) == Family::Guardian && w.fluid(p[0], p[1], p[2]) == Fluid::Water {
        return 10.0 + light();
    }
    // HappyGhast.getWalkTargetValue: open air, best just above the ground.
    if m.species() == Species::HappyGhast {
        let air = |y: i32| w.facts.is_air(w.world.block(p[0], y, p[2]));
        return if !air(p[1]) {
            0.0
        } else if air(p[1] - 1) && !air(p[1] - 2) {
            10.0
        } else {
            5.0
        };
    }
    if m.species() == Species::Strider {
        return crate::mobs::strider::walk_target_value(m, w, p);
    }
    if m.species() == Species::Turtle {
        return crate::mobs::turtle::walk_target_value(m, w, p, light());
    }
    // Hoglin.getWalkTargetValue: nowhere near a repellent, crimson nylium best.
    if m.species() == Species::Hoglin {
        let near = m.extra.repellent.is_some_and(|r| {
            let d = |i: usize| i64::from(r[i] - p[i]);
            d(0) * d(0) + d(1) * d(1) + d(2) * d(2) < 64
        });
        return if near {
            -1.0
        } else if w.block_name(p[0], p[1] - 1, p[2]) == "minecraft:crimson_nylium" {
            10.0
        } else {
            0.0
        };
    }
    // Bee.getWalkTargetValue: any air block.
    if m.species() == Species::Bee {
        return if w.facts.is_air(w.world.block(p[0], p[1], p[2])) {
            10.0
        } else {
            0.0
        };
    }
    // AbstractNautilus, Axolotl and Creaking.getWalkTargetValue: everywhere alike.
    if matches!(
        m.species(),
        Species::Nautilus | Species::ZombieNautilus | Species::Axolotl | Species::Creaking
    ) {
        return 0.0;
    }
    // Silverfish.getWalkTargetValue: a block sitting on an infested host is
    // the best place to walk to, so it goes back into the stone.
    if m.species() == Species::Silverfish
        && crate::mobs::hostile::is_infested_host(w.block_name(p[0], p[1] - 1, p[2]))
    {
        return 10.0;
    }
    // Enderman.getWalkTargetValue, Pillager's and Warden's: every position scores
    // the same, so their strolls pick one of the ten at random.
    if matches!(
        m.species(),
        Species::Enderman | Species::Pillager | Species::Warden
    ) {
        return 0.0;
    }
    if m.hostile()
        && !matches!(
            m.species(),
            Species::Slime | Species::MagmaCube | Species::Ghast | Species::Phantom
        )
    {
        return -light();
    }
    // MushroomCow.getWalkTargetValue replaces Animal's: mycelium is the best
    // ground and any other is scored by the light, grass or not.
    if m.species() == Species::Mooshroom {
        return if w.block_name(p[0], p[1] - 1, p[2]) == "minecraft:mycelium" {
            10.0
        } else {
            w.brightness(p[0], p[1], p[2]) - 0.5
        };
    }
    if m.kind.flags.animal {
        if w.block_name(p[0], p[1] - 1, p[2]) == "minecraft:grass_block" {
            return 10.0;
        }
        return light();
    }
    0.0
}

