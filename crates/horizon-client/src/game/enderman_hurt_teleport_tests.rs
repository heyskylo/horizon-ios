//! `Enderman.hurtServer` teleports inside the hurt the game layer deals.
use super::{
    Audio, Game, Options, Progress, Settings, explosion::BlockInteraction, load_resources,
};
use horizon_client::{
    chunk::Chunk,
    mobs::{Mob, MobEvent},
};
use horizon_core::{
    geometry::Vec3,
    position::ChunkPos,
    random::{LegacyRandom, RandomSource},
};
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
    mob.random = LegacyRandom::new(seed);
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
    // A seed whose first draw passes the effect's 1-in-10 chance (`nextFloat() <= 0.1`).
    let seed = (0..)
        .find(|&s| LegacyRandom::new(s).next_float() <= 0.1)
        .unwrap();
    let mut mob = enderman(&game, 600, at, seed);
    mob.add_effect("minecraft:infested", 200, 0);
    // What `Mob::damage` leaves for a projectile's hit: the effect's spawn and
    // `repeatedlyTryToTeleport`.
    mob.outbox.push(MobEvent::Infested { mob: 600 });
    mob.extra.counter2 = 64;
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
