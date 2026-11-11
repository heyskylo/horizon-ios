//! `ChunkMap.forEachBlockTickingChunk`: the order chunks random tick in must
//! not follow the order they finished loading in, or the level random's draws
//! land on other blocks from one run of a world to the next (#15164).

use super::Game;
use super::native_machines::tests::server_game;
use horizon_client::chunk::Chunk;
use horizon_core::geometry::Vec3;
use std::sync::Arc;

/// A game whose player stands in chunk (0, 0), with lit chunks loaded in
/// `order`.
fn game_loading(order: &[(i32, i32)]) -> Game<'static> {
    let mut game = server_game();
    game.settings.simulation_distance = 5;
    game.settings.render_distance = 8;
    game.player = Some(
        horizon_client::motion::PlayerMotion::new(game.motion, Vec3::new(8.5, 65.0, 8.5)).unwrap(),
    );
    game.world.chunks.clear();
    for &(x, z) in order {
        let mut chunk = Chunk::empty(x, z);
        chunk.lit = true;
        game.world.chunks.insert((x, z), Arc::new(chunk));
    }
    game.update_chunk_tickets();
    game
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn chunks_random_tick_in_the_same_order_however_they_loaded() {
    let mut loaded: Vec<(i32, i32)> = (-8..=8)
        .flat_map(|x| (-8..=8).map(move |z| (x, z)))
        .collect();
    let first = game_loading(&loaded);
    // The streaming workers finish in another order on another run.
    loaded.reverse();
    loaded.rotate_left(97);
    let second = game_loading(&loaded);

    let (mut a, mut b) = (Vec::new(), Vec::new());
    first.random_tick_chunks(&mut a);
    second.random_tick_chunks(&mut b);
    assert!(
        a.contains(&(0, 0)) && a.contains(&(4, -4)),
        "the simulation distance's chunks random tick: {a:?}"
    );
    assert!(
        !a.contains(&(8, 0)),
        "a chunk past the simulation distance doesn't"
    );
    assert_eq!(a, b);
}
