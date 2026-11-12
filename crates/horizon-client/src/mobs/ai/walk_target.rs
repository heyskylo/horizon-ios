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

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_core::geometry::Vec3;
    use std::sync::Arc;

    /// `PathfinderMob.getWalkTargetValue` and its overrides, at three positions
    /// that differ only in the block below. A cow scores grass 10 and mycelium by
    /// the light; a mushroom cow, whose own override replaces Animal's, mycelium
    /// 10 and grass by the light; a monster the negated light; and an enderman, a
    /// pillager and a warden the same 0 everywhere.
    #[test]
    fn a_stroll_scores_its_ground_the_way_the_jar_does() {
        let ran = crate::mobs::world::with_built_test_world(
            &[],
            |world, registry| {
                let stone = registry.default_state("minecraft:stone").unwrap();
                let grass = registry.default_state("minecraft:grass_block").unwrap();
                let mycelium = registry.default_state("minecraft:mycelium").unwrap();
                for cx in -1..1 {
                    for cz in -1..1 {
                        world
                            .chunks
                            .insert((cx, cz), Arc::new(crate::chunk::Chunk::empty(cx, cz)));
                    }
                }
                for x in -4..4 {
                    for z in -4..4 {
                        world.set(x, 63, z, stone).unwrap();
                    }
                }
                world.set(-2, 63, 0, grass).unwrap();
                world.set(0, 63, 0, stone).unwrap();
                world.set(2, 63, 0, mycelium).unwrap();
            },
            |w, types| {
                let value = |id: &str, x: i32| {
                    let kind = Arc::new(types.types[id].clone());
                    let m = Mob::new(1, kind, Vec3::ZERO, 0.0);
                    walk_target_value(&m, w, [x, 64, 0])
                };
                // The light term of the middle position, which the report quotes.
                let light = w.brightness(0, 64, 0) - 0.5;
                // A cow: grass 10, mycelium and stone by the light.
                assert_eq!(value("minecraft:cow", -2), 10.0, "cow on grass");
                assert_eq!(value("minecraft:cow", 0), light, "cow on stone");
                assert_eq!(value("minecraft:cow", 2), light, "cow on mycelium");
                // A mushroom cow: mycelium 10, grass by the light, not 10.
                assert_eq!(
                    value("minecraft:mooshroom", 2),
                    10.0,
                    "mooshroom on mycelium"
                );
                assert_eq!(
                    value("minecraft:mooshroom", -2),
                    light,
                    "mooshroom on grass"
                );
                // A monster: the negated light, on grass like anywhere else.
                assert_eq!(value("minecraft:zombie", -2), -light, "zombie");
                // The three that score every position the same.
                for flat in [
                    "minecraft:enderman",
                    "minecraft:pillager",
                    "minecraft:warden",
                ] {
                    assert_eq!(value(flat, -2), 0.0, "{flat} on grass");
                    assert_eq!(value(flat, 0), 0.0, "{flat} on stone");
                    assert_eq!(value(flat, 2), 0.0, "{flat} on mycelium");
                }
            },
        );
        // Without the extracted vanilla data the world cannot be built.
        let _ = ran;
    }
}
