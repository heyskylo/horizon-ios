use super::*;
use crate::game::{
    Audio, Options, Progress, Settings, execution_role::ExecutionRole, load_resources,
};
use horizon_client::lan::{Joined, PlayerSender, guest::Guest};

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn empty_advancement_tree_does_not_skip_normal_or_durable_statistics() {
    let root = std::path::PathBuf::from(
        std::env::var_os("HORIZON_TEST_RESOURCES").expect("prepared resources required"),
    );
    let settings = Settings::default();
    let loaded = load_resources(
        root.clone(),
        None,
        &root,
        &settings,
        Instant::now(),
        false,
        true,
        &Progress::new(),
        &mut |_, _| unreachable!("no menu handoff"),
    )
    .unwrap();
    let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
    options.resources = root.clone();
    options.execution_role = ExecutionRole::Client;
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
    game.adv.tree = Arc::new(Default::default());
    let directory = std::env::temp_dir().join(format!(
        "horizon-arrival-records-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    struct Scratch(std::path::PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _scratch = Scratch(directory.clone());
    game.world_root = directory;

    let sync = Arc::new(SyncData::empty());
    let network = Lan::start_with(
        sync.clone(),
        NetConfig {
            announce: false,
            ..NetConfig::loopback(0, "RecordsFixture".into(), 2)
        },
    )
    .unwrap();
    let encoder = ChunkEncoder::new(
        [] as [(Arc<str>, bool); 0],
        &horizon_protocol::registry::Builtin::default(),
        &[],
        &[],
    );
    let config = HubConfig {
        default_game_mode: 0,
        guest_commands: false,
        force_game_mode: false,
        hardcore: false,
        view_distance: 2,
        simulation_distance: 2,
        dimension: "minecraft:overworld".into(),
        allow_flight: false,
        seed: 0,
        max_players: 20,
        integrated: false,
        levels: Default::default(),
    };
    let mut host = LanHost::fixture(Hub::new(network, sync, encoder, config));
    let (sender, _receiver) = std::sync::mpsc::channel();
    let mut guest = Guest::new(
        Joined {
            conn: 1,
            uuid: 3,
            name: "RecordsFixture".into(),
            properties: vec![],
            info: horizon_protocol::packets::configuration::ClientInformation::new(2),
            brand: "vanilla".into(),
            sender: PlayerSender::new(sender, Arc::default()),
            latency: Arc::new(std::sync::atomic::AtomicI32::new(0)),
            address: "127.0.0.1:25565".parse().unwrap(),
            expected: false,
            configured: Default::default(),
        },
        7,
        [0.5, 64.0, 0.5],
        2,
    );
    guest.placed = true;
    let mut play = super::super::lan_actions::GuestPlay::default();
    play.stats.custom("jump", 13);
    host.play.insert(1, play);
    let uuid = playerdata::uuid_ints(guest.uuid);
    let advancements = super::super::stats::player_json(&game.world_root, "advancements", uuid);
    let stats = super::super::stats::player_json(&game.world_root, "stats", uuid);
    std::fs::create_dir_all(advancements.parent().unwrap()).unwrap();
    let existing = b"{\"existing-advancements\":true}";
    std::fs::write(&advancements, existing).unwrap();
    game.save_guest_records(&host, &guest);
    assert_eq!(
        super::super::stats::Stats::read(&stats).get("custom", "minecraft:jump"),
        13
    );
    assert_eq!(std::fs::read(&advancements).unwrap(), existing);

    host.play.get_mut(&1).unwrap().stats.custom("jump", 4);
    guest.placed = false;
    guest.held = true;
    game.write_arrival_records(&host, &guest).unwrap();
    assert_eq!(
        super::super::stats::Stats::read(&stats).get("custom", "minecraft:jump"),
        17
    );
    assert_eq!(std::fs::read(&advancements).unwrap(), existing);
    std::fs::remove_file(&stats).unwrap();
    std::fs::create_dir(&stats).unwrap();
    let error = game
        .write_arrival_records(&host, &guest)
        .expect_err("blocked statistics path must reject confirmation");
    assert!(
        error.contains("stats") && error.contains("not saved"),
        "{error}"
    );
    assert_eq!(std::fs::read(&advancements).unwrap(), existing);
}
