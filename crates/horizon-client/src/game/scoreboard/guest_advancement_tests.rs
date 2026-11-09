use super::*;
use crate::game::Dim;
use horizon_core::geometry::Vec3;

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn selector_reads_independent_host_and_guest_progress() {
    use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
    use horizon_client::{
        lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig},
        motion::PlayerMotion,
    };
    use horizon_protocol::packets::configuration::ClientInformation;
    use std::sync::{Arc, atomic::AtomicI32, mpsc};
    let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    let settings = Settings::default();
    let loaded = load_resources(
        root.clone(),
        None,
        &root,
        &settings,
        std::time::Instant::now(),
        false,
        true,
        &Progress::new(),
        &mut |_, _| unreachable!(),
    )
    .unwrap();
    let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
    options.resources = root.clone();
    options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
    let mut game = Game::new(
        &loaded.resources,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(&root),
        None,
    )
    .unwrap();
    let scratch = std::env::temp_dir().join(format!(
        "horizon-guest-selector-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&scratch).unwrap();
    game.world_root = scratch.clone();
    game.player = Some(PlayerMotion::new(&game.motion, Vec3::new(10.5, 64.0, 0.5)).unwrap());
    let net = NetConfig {
        announce: false,
        ..NetConfig::lan(0, "selector-test".into(), 2)
    };
    game.lan_host(
        super::super::lan::LanSettings {
            port: 0,
            guest_commands: true,
            force_game_mode: false,
        },
        net,
        HubConfig {
            default_game_mode: 0,
            guest_commands: true,
            force_game_mode: false,
            hardcore: false,
            view_distance: 2,
            simulation_distance: 2,
            dimension: Dim::Overworld.id().into(),
            allow_flight: false,
            seed: 0,
            max_players: 2,
            integrated: false,
            levels: Default::default(),
        },
    )
    .unwrap();
    let (tx, _rx) = mpsc::channel();
    let mut guest = Guest::new(
        Joined {
            conn: 2,
            uuid: 2,
            name: "SelectorGuest".into(),
            properties: Vec::new(),
            info: ClientInformation::new(2),
            brand: "test".into(),
            sender: PlayerSender::new(tx, Arc::default()),
            latency: Arc::new(AtomicI32::new(0)),
            address: "127.0.0.1:25565".parse().unwrap(),
            expected: false,
            configured: Default::default(),
        },
        2,
        [20.5, 64.0, 0.5],
        2,
    );
    guest.dimension = Dim::Overworld.id().into();
    guest.placed = true;
    guest.loaded = true;
    game.lan.as_mut().unwrap().hub.guests.push(guest);

    const ROOT: &str = "minecraft:story/root";
    const CRITERION: &str = "crafting_table";
    game.advancement_tree();
    assert!(game.adv.tree.get(ROOT).is_some());
    assert!(game.lan.as_mut().unwrap().hub.guests[0].advancements.grant(
        ROOT,
        CRITERION,
        "2026-11-05 00:00:00 +0000"
    ));
    let select = |g: &mut Game<'_>, condition: &str| {
        let text = format!("@a[advancements={{{ROOT}={condition}}}]");
        let s = horizon_client::command::selector::parse(
            &mut horizon_client::command::Reader::new(&text),
            &horizon_client::command::args::Registries::default(),
            true,
        )
        .unwrap();
        g.select(&s)
    };
    // The real Game traversal must read the selected guest, not the host.
    assert_eq!(select(&mut game, "true"), vec![Target::Guest(2)]);
    assert_eq!(select(&mut game, "false"), vec![Target::Player]);
    assert_eq!(
        select(&mut game, "{crafting_table=true}"),
        vec![Target::Guest(2)]
    );
    assert_eq!(
        select(&mut game, "{crafting_table=false}"),
        vec![Target::Player]
    );
    assert!(
        game.adv
            .progress
            .grant(ROOT, CRITERION, "2026-11-05 00:00:00 +0000")
    );
    assert!(
        game.lan.as_mut().unwrap().hub.guests[0]
            .advancements
            .revoke(ROOT, CRITERION)
    );
    assert_eq!(select(&mut game, "true"), vec![Target::Player]);
    assert_eq!(select(&mut game, "false"), vec![Target::Guest(2)]);
    assert_eq!(
        select(&mut game, "{crafting_table=true}"),
        vec![Target::Player]
    );
    assert_eq!(
        select(&mut game, "{crafting_table=false}"),
        vec![Target::Guest(2)]
    );
    drop(game);
    std::fs::remove_dir_all(scratch).unwrap();
}
