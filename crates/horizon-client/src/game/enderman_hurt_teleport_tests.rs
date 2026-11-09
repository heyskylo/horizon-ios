//! `Enderman.hurtServer` teleports inside the hurt the game layer deals.
use super::{
    Audio, Game, Options, Progress, Settings, explosion::BlockInteraction, load_resources,
};
use horizon_client::{
    chunk::Chunk,
    mobs::{Mob, MobEvent},
};
use horizon_core::{geometry::Vec3, position::ChunkPos, random::RandomSource};
use std::sync::Arc;

/// A game over 5x5 empty chunks around (0, 0) with a stone floor at y = 3:
/// every enderman teleport from (8.5, 4, 8.5) lands on it or fails.
fn floored_game(root: &std::path::Path) -> Game<'static> {
    let settings = Settings::default();
    let loaded = load_resources(
        root.to_path_buf(),
        None,
        root,
        &settings,
        std::time::Instant::now(),
        false,
        true,
        &Progress::new(),
        &mut |_, _| unreachable!(),
    )
    .unwrap();
    let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
    options.resources = root.to_path_buf();
    options.execution_role = super::execution_role::ExecutionRole::IntegratedServer;
    let resources = Box::leak(Box::new(loaded.resources));
    let mut game = Game::new(
        resources,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(root),
        None,
    )
    .unwrap();
    for cx in -2..=2 {
        for cz in -2..=2 {
            game.world
                .chunks
                .insert((cx, cz), Arc::new(Chunk::empty(cx, cz)));
            game.block_ticks
                .add_empty_chunk(ChunkPos::new(cx, cz))
                .unwrap();
        }
    }
    let stone = game
        .pack
        .assets
        .registry
        .default_state("minecraft:stone")
        .unwrap();
    for x in -32..48 {
        for z in -32..48 {
            game.world.set(x, 3, z, stone).unwrap();
        }
    }
    game
}

fn enderman(game: &Game, id: u32, at: Vec3, seed: i64) -> Mob {
    let kind = game
        .res
        .mob_types()
        .get("minecraft:enderman")
        .unwrap()
        .clone();
    let mut mob = Mob::new(id, kind, at, 0.0);
    mob.random.set_seed(seed);
    mob.persistent = true;
    mob
}

/// `ServerExplosion.hurtEntities` -> `Enderman.hurtServer`: an enderman the
/// blast hurts has teleported (or drawn not to) when the explosion returns.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn an_explosion_teleports_endermen_before_it_returns() {
    let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    let mut game = floored_game(&root);
    let center = Vec3::new(8.5, 4.0, 8.5);
    for k in 0..12 {
        let angle = f64::from(k) * std::f64::consts::TAU / 12.0;
        let at = Vec3::new(
            center.x + 2.5 * angle.cos(),
            center.y,
            center.z + 2.5 * angle.sin(),
        );
        let mob = enderman(&game, 500 + k as u32, at, 900 + i64::from(k));
        game.mobs.push(mob);
    }
    let before: Vec<Vec3> = game.mobs.iter().map(|m| m.position).collect();
    game.server_explode(
        [center.x, center.y, center.z],
        4.0,
        BlockInteraction::Keep,
        false,
    )
    .unwrap();
    let pending: Vec<u32> = game
        .mobs
        .iter()
        .filter(|m| m.extra.counter2 > 0)
        .map(|m| m.id)
        .collect();
    assert!(
        pending.is_empty(),
        "teleports left for the AI step: {pending:?}"
    );
    let moved = game
        .mobs
        .iter()
        .zip(&before)
        .filter(|(m, b)| m.position.distance(**b) > 3.0)
        .count();
    assert!(moved > 0, "no enderman teleported");
}

/// `LivingEntity.hurtServer` runs `InfestedMobEffect.onMobHurt` before
/// `Enderman.hurtServer` teleports: the silverfish come out where the enderman
/// was hurt, from its random's next draws.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn an_infested_endermans_silverfish_come_out_where_it_was_hurt() {
    let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    let mut game = floored_game(&root);
    let at = Vec3::new(8.5, 4.0, 8.5);
    // A hurt with nothing living behind it, from a seed whose draws after the
    // hurt sound pass the effect's 1-in-10 chance and then `nextInt(10) != 0`:
    // `Mob::damage` leaves the silverfish and one `teleport()`.
    let hurt = |seed| {
        let mut mob = enderman(&game, 600, at, seed);
        mob.add_effect("minecraft:infested", 200, 0);
        mob.damage(
            1.0,
            horizon_client::mobs::Damage::of(horizon_client::mobs::DamageKind::Generic),
        );
        mob
    };
    let mob = (0..10_000)
        .map(hurt)
        .find(|m| {
            m.extra.counter2 == 1
                && m.outbox
                    .iter()
                    .any(|e| matches!(e, MobEvent::Infested { .. }))
        })
        .expect("no seed spawns silverfish and teleports");
    game.mobs.push(mob);
    game.settle_hurt_teleports().unwrap();
    let ender = game.mobs.iter().find(|m| m.id == 600).unwrap();
    assert!(
        ender.position.distance(at) > 3.0,
        "the enderman did not teleport"
    );
    let silverfish: Vec<Vec3> = game
        .mobs
        .iter()
        .filter(|m| m.kind.id == "minecraft:silverfish")
        .map(|m| m.position)
        .collect();
    assert!(!silverfish.is_empty(), "no silverfish");
    for p in silverfish {
        assert!(p.distance(at) < 3.0, "silverfish at {p:?}, hurt at {at:?}");
    }
}

fn shulker(game: &Game, id: u32, at: Vec3, seed: i64) -> Mob {
    let kind = game
        .res
        .mob_types()
        .get("minecraft:shulker")
        .unwrap()
        .clone();
    let mut mob = Mob::new(id, kind, at, 0.0);
    mob.random.set_seed(seed);
    mob.persistent = true;
    mob
}

/// `ShulkerBullet.onHitEntity` -> `Shulker.hurtServer`: an open shulker a
/// bullet hurts runs `hitByShulkerBullet` (its teleport) inside the hurt, so
/// it has moved when the hit returns, and the levitation still lands on it.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_shulker_bullets_hit_teleports_an_open_shulker_before_it_returns() {
    use horizon_client::projectiles::{Hit, Projectile, ProjectileKind};
    let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    let mut game = floored_game(&root);
    let at = Vec3::new(8.5, 4.0, 8.5);
    let mut moved = 0;
    // On a flat floor one try in 17 finds a block to cling to: some seeds teleport.
    for seed in 0..40 {
        let id = 700 + seed as u32;
        let mut mob = shulker(&game, id, at, seed);
        // Open: hitByShulkerBullet is for an open shell only.
        mob.extra.counter = 100;
        game.mobs.push(mob);
        let bullet = Projectile::new(
            ProjectileKind::ShulkerBullet {
                target: None,
                dir: None,
                steps: 0,
                delta: [0.0, 0.0, 0.0],
                seed: 0,
            },
            Vec3::new(at.x, at.y + 0.5, at.z - 1.0),
            None,
        );
        game.projectile_hit(
            &bullet,
            Hit::Entity {
                who: horizon_client::mobs::Who::Mob(id),
                at: Vec3::new(at.x, at.y + 0.5, at.z),
            },
        )
        .unwrap();
        let m = game.mobs.iter().find(|m| m.id == id).unwrap();
        assert!(
            !m.extra.flag && !m.extra.flag2,
            "seed {seed}: a teleport left for the AI step"
        );
        assert!(
            m.has_effect("minecraft:levitation"),
            "seed {seed}: no levitation"
        );
        if m.position.distance(at) > 0.5 {
            moved += 1;
        }
        game.mobs.clear();
    }
    assert!(moved > 0, "no shulker teleported");
}

/// `Shulker.teleportSomewhere` -> `Entity.unRide`: a mob riding the shulker
/// gets off where the shulker stood (`ejectPassengers`) instead of being
/// carried to where it teleports.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_teleporting_shulker_leaves_its_rider_behind() {
    let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    let mut game = floored_game(&root);
    let at = Vec3::new(8.5, 4.0, 8.5);
    let mut teleported = 0;
    for seed in 0..40 {
        let (id, rider) = (800 + 2 * seed as u32, 801 + 2 * seed as u32);
        let mut mob = shulker(&game, id, at, seed);
        // What `after_hurt` leaves after a low-health roll.
        mob.extra.flag = true;
        game.mobs.push(mob);
        let mut zombie = enderman(&game, rider, at, 0);
        zombie.kind = game
            .res
            .mob_types()
            .get("minecraft:zombie")
            .unwrap()
            .clone();
        game.mobs.push(zombie);
        horizon_client::mobs::riders::link(&mut game.mobs, rider, id);
        game.settle_hurt_teleports().unwrap();
        let s = game.mobs.iter().find(|m| m.id == id).unwrap();
        let z = game.mobs.iter().find(|m| m.id == rider).unwrap();
        if s.position.distance(at) > 0.5 {
            teleported += 1;
            assert_eq!(z.extra.mount, None, "seed {seed}: still riding");
            assert_eq!(s.extra.rider, None, "seed {seed}: still carrying");
            assert!(
                z.position.distance(at) < 2.0,
                "seed {seed}: rider at {:?}, shulker left {at:?}",
                z.position
            );
        } else {
            assert_eq!(
                z.extra.mount,
                Some(id),
                "seed {seed}: thrown off without a teleport"
            );
        }
        game.mobs.clear();
    }
    assert!(teleported > 0, "no shulker teleported");
}
