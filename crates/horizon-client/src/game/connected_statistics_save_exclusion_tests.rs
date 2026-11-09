/// Source controls for the bounded eligibility gates; these excluded
/// guests are not counted as successfully saved players.
#[test]
#[ignore = "needs prepared resources and scratch homes/cwd"]
fn excluded_guest_records_are_not_invented_or_written() {
    let mut game = super::super::native_machines::tests::server_game();
    let _rx = super::world_spawn_sync_tests::host_for(&mut game);
    game.world_root = std::env::current_dir().unwrap().join("stats-excluded");
    std::fs::create_dir_all(&game.world_root).unwrap();
    let path = super::super::stats::player_json(&game.world_root, "stats", [0, 0, 0, 1]);
    let mut host = game.lan.take().unwrap();
    host.play
        .entry(1)
        .or_default()
        .stats
        .add("used", "minecraft:diamond_sword", 1);
    host.hub.guests[0].placed = false;
    game.save_connected_guest_statistics(&host);
    assert!(!path.exists(), "not placed: no statistics file");
    host.hub.guests[0].placed = true;
    host.hub.guests[0].released = true;
    game.save_connected_guest_statistics(&host);
    assert!(!path.exists(), "released: no statistics file");
    host.hub.guests[0].released = false;
    let play = host.play.remove(&1).unwrap();
    game.save_connected_guest_statistics(&host);
    assert!(!path.exists(), "no play record: no invented statistics");
    host.play.insert(1, play);
    game.lan = Some(host);
    game.options.dedicated = None;
    game.save_game_metadata().unwrap();
    assert!(!path.exists(), "the existing dedicated gate is preserved");
    game.options.dedicated = Some(Vec::new());
    game.options.execution_role = super::super::execution_role::ExecutionRole::Client;
    game.save_game_metadata().unwrap();
    assert!(!path.exists(), "the existing nonclient gate is preserved");
    game.options.execution_role = super::super::execution_role::ExecutionRole::DedicatedServer;
    let host = game.lan.take().unwrap();
    game.save_game_metadata().unwrap();
    assert!(!path.exists(), "no hub: no substituted record");
    assert_eq!(
        host.play[&1].stats.get("used", "minecraft:diamond_sword"),
        1
    );
}
