//! Powder snow and water cauldrons putting a burning player out: the hiss
//! (`Entity.playEntityOnFireExtinguishedSound`) plays either way, the block
//! change only where `mayInteract` allows it.
use super::super::native_machines::tests::server_game;
use super::*;
use horizon_client::motion::{MotionWorld, PlayerMotion};
use horizon_core::geometry::Vec3;
use horizon_world::chunk::BlockState;
use horizon_world::entity_move::MovementRecord;
use horizon_world::registry::BlockStateId;

/// A server's game on the prepared resources, or None when they are absent
/// (CI has none: the tests then skip quietly).
fn server() -> Option<Game<'static>> {
    std::env::var_os("HORIZON_TEST_RESOURCES")?;
    Some(server_game())
}

fn state(game: &Game<'_>, name: &str, level: &str) -> BlockStateId {
    let mut state = BlockState {
        name: name.into(),
        properties: Default::default(),
    };
    if !level.is_empty() {
        state.properties.insert("level".into(), level.into());
    }
    game.pack.assets.registry.resolve(&state).unwrap()
}

fn block(game: &Game<'_>, pos: [i32; 3]) -> String {
    let state = game.world.block(pos[0], pos[1], pos[2]);
    game.res.facts().get(state).name.to_string()
}

fn heard(game: &Game<'_>) -> u32 {
    game.audio
        .heard
        .get("minecraft:entity.generic.extinguish_fire")
        .copied()
        .unwrap_or(0)
}

/// A server's player standing in the block at `pos` (feet on its floor), on
/// fire: the tick `guest_server_tick` runs for it, its block effects and the
/// consumer of their events.
fn burn_in(game: &mut Game<'static>, pos: [i32; 3]) {
    let at = Vec3::new(
        f64::from(pos[0]) + 0.5,
        f64::from(pos[1]),
        f64::from(pos[2]) + 0.5,
    );
    game.player = Some(PlayerMotion::new(game.motion, at).unwrap());
    game.player.as_mut().unwrap().fire_ticks = 100;
    let events = {
        let player = game.player.as_mut().unwrap();
        // The move the client sent: standing still, inside the block.
        player.history.add(MovementRecord {
            from: at,
            to: at,
            original: None,
        });
        let mut w = MotionWorld {
            entities: &[],
            player,
            scene: &mut game.world,
            assets: &game.pack.assets,
            collision_binding: game.collision,
            resources: game.motion,
            health: &game.health_resources,
        };
        w.server_block_damage(at).unwrap();
        std::mem::take(&mut w.player.events)
    };
    game.block_extinguish_player(&events);
}

/// Outside the world border, as `/worldborder center 1000 1000` and
/// `/worldborder set 100` put the origin.
fn outside_the_border(game: &mut Game<'_>) {
    game.border.set_center([1000.0, 1000.0]);
    game.border.set_size(100.0);
}

/// `PowderSnowBlock.entityInside`: the powder snow is destroyed and the
/// burning player put out, with the hiss.
#[test]
fn powder_snow_puts_a_burning_player_out_with_the_hiss() {
    let Some(mut game) = server() else {
        return;
    };
    let pos = [4, 64, 4];
    let snow = state(&game, "minecraft:powder_snow", "");
    game.world.set(pos[0], pos[1], pos[2], snow).unwrap();
    burn_in(&mut game, pos);
    assert!(game.player.as_ref().unwrap().fire_ticks <= 0, "fire out");
    assert_eq!(block(&game, pos), "minecraft:air", "the snow is destroyed");
    assert_eq!(heard(&game), 1, "one hiss");
}

/// `LayeredCauldronBlock.handleEntityOnFireInside`: the level-3 water
/// cauldron drops to level 2, and the hiss plays.
#[test]
fn a_cauldron_puts_a_burning_player_out_with_the_hiss() {
    let Some(mut game) = server() else {
        return;
    };
    let pos = [4, 64, 4];
    let water = state(&game, "minecraft:water_cauldron", "3");
    game.world.set(pos[0], pos[1], pos[2], water).unwrap();
    burn_in(&mut game, pos);
    assert!(game.player.as_ref().unwrap().fire_ticks <= 0, "fire out");
    let level = game
        .pack
        .assets
        .registry
        .state(game.world.block(pos[0], pos[1], pos[2]))
        .and_then(|s| s.properties.get("level").cloned());
    assert_eq!(level.as_deref(), Some("2"), "the water drops a level");
    assert_eq!(heard(&game), 1, "one hiss");
}

/// `ServerPlayer.mayInteract` outside the world border: the fire still goes
/// out and the hiss still plays, but the powder snow stays.
#[test]
fn outside_the_border_powder_snow_stays_but_the_fire_goes_out() {
    let Some(mut game) = server() else {
        return;
    };
    let pos = [4, 64, 4];
    let snow = state(&game, "minecraft:powder_snow", "");
    game.world.set(pos[0], pos[1], pos[2], snow).unwrap();
    outside_the_border(&mut game);
    burn_in(&mut game, pos);
    assert!(game.player.as_ref().unwrap().fire_ticks <= 0, "fire out");
    assert_eq!(block(&game, pos), "minecraft:powder_snow", "it stays");
    assert_eq!(heard(&game), 1, "the hiss still plays");
}

/// The same for a cauldron: level 3 stays level 3, the fire goes out and the
/// hiss plays.
#[test]
fn outside_the_border_a_cauldron_keeps_its_level_but_the_fire_goes_out() {
    let Some(mut game) = server() else {
        return;
    };
    let pos = [4, 64, 4];
    let water = state(&game, "minecraft:water_cauldron", "3");
    game.world.set(pos[0], pos[1], pos[2], water).unwrap();
    outside_the_border(&mut game);
    burn_in(&mut game, pos);
    assert!(game.player.as_ref().unwrap().fire_ticks <= 0, "fire out");
    let level = game
        .pack
        .assets
        .registry
        .state(game.world.block(pos[0], pos[1], pos[2]))
        .and_then(|s| s.properties.get("level").cloned());
    assert_eq!(level.as_deref(), Some("3"), "it keeps its level");
    assert_eq!(heard(&game), 1, "the hiss still plays");
}
