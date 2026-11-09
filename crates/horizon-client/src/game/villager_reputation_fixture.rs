//! Test-only logical players and merchant state; no socket or server loop.
use super::{Game, LanHost, LanSettings};
use crate::game::{Audio, Options, Progress, Settings, load_resources, trading::MerchantState};
use horizon_client::{
    items::ItemStack,
    lan::{
        Lan, NetConfig,
        chunks::ChunkEncoder,
        data::SyncData,
        guest::Guest,
        hub::{Hub, HubConfig},
    },
    merchant::{ItemCost, MerchantOffer, Session},
    mobs::Mob,
};
use horizon_core::geometry::Vec3;
use std::sync::Arc;

pub(in crate::game) fn game() -> Game<'static> {
    let home =
        std::path::PathBuf::from(std::env::var_os("HORIZON_HOME").expect("isolated fixture home"));
    let root = std::path::PathBuf::from(
        std::env::var_os("HORIZON_TEST_RESOURCES").expect("explicit read-only fixture resources"),
    );
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
    options.game_dir = Some(home.join(format!("fixture-{:?}", std::thread::current().id())));
    options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
    let res = Box::leak(Box::new(loaded.resources));
    let mut game = Game::new(
        res,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(&root),
        None,
    )
    .unwrap();
    game.options.dedicated = Some(Vec::new());
    game.world
        .chunks
        .insert((0, 0), Arc::new(horizon_client::chunk::Chunk::empty(0, 0)));
    game.block_ticks
        .add_empty_chunk(horizon_core::position::ChunkPos::new(0, 0))
        .unwrap();
    game.build_registries();
    game.set_allow_commands(true);
    game
}

fn hub(root: &std::path::Path) -> Hub {
    let mut sync = SyncData::empty();
    sync.builtin = horizon_protocol::registry::Builtin::load(
        &horizon_protocol::registry::Builtin::find(root).expect("prepared wire registries"),
    )
    .unwrap();
    let sync = Arc::new(sync);
    let net = NetConfig {
        listen: false,
        announce: false,
        ..NetConfig::lan(0, "reputation-fixture".into(), 2)
    };
    let transport = Lan::start_with(sync.clone(), net).unwrap();
    assert_eq!(transport.port, 0, "fixture never binds a listener");
    let encoder = ChunkEncoder::new([] as [(Arc<str>, bool); 0], &sync.builtin, &[], &[]);
    Hub::new(
        transport,
        sync,
        encoder,
        HubConfig {
            default_game_mode: 0,
            guest_commands: true,
            force_game_mode: false,
            hardcore: false,
            view_distance: 8,
            simulation_distance: 8,
            dimension: "minecraft:overworld".into(),
            allow_flight: false,
            seed: 0,
            max_players: 2,
            integrated: false,
            levels: Default::default(),
        },
    )
}
pub(in crate::game) fn guest(
    conn: u32,
    uuid: u128,
) -> (Guest, std::sync::mpsc::Receiver<horizon_client::lan::Out>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut g = Guest::new(
        horizon_client::lan::Joined {
            conn,
            uuid,
            name: format!("fixture-{conn}"),
            properties: Vec::new(),
            info: horizon_protocol::packets::configuration::ClientInformation::new(8),
            brand: "vanilla".into(),
            sender: horizon_client::lan::PlayerSender::new(tx, Arc::default()),
            latency: Arc::new(std::sync::atomic::AtomicI32::new(0)),
            address: "127.0.0.1:1".parse().unwrap(),
            expected: false,
            configured: Default::default(),
        },
        conn as i32,
        [0.5, 1.0, 0.5],
        8,
    );
    g.placed = true;
    g.dimension = "minecraft:overworld".into();
    g.permission = 4;
    (g, rx)
}
pub(in crate::game) fn offer() -> MerchantOffer {
    MerchantOffer {
        base_cost_a: ItemCost {
            stack: ItemStack::new("minecraft:emerald", 20),
        },
        cost_b: None,
        result: ItemStack::new("minecraft:bread", 1),
        uses: 0,
        max_uses: 12,
        reward_exp: false,
        special_price_diff: 0,
        demand: 0,
        price_multiplier: 0.2,
        xp: 1,
    }
}
pub(in crate::game) fn state() -> MerchantState {
    let mut session = Session::new(vec![offer()], true);
    session.hero = Some(0);
    session.update_special_prices();
    MerchantState {
        session: Box::new(session),
        scroll: 0,
        dragging: false,
        focused: None,
        mob: Some(91),
        name: None,
    }
}
pub(in crate::game) fn price(host: &LanHost) -> (i32, Option<i32>, i32) {
    let s = &host.play[&1].merchant.as_ref().unwrap().merchant.session;
    (s.reputation, s.hero, s.offers[0].special_price_diff)
}
pub(in crate::game) fn villager(game: &Game<'_>, id: u32) -> Mob {
    Mob::new(
        id,
        game.res
            .mob_types()
            .get("minecraft:villager")
            .unwrap()
            .clone(),
        Vec3::new(0.5, 1.0, 0.5),
        0.0,
    )
}

pub(in crate::game) fn host(root: &std::path::Path) -> LanHost {
    let hub = hub(root);
    LanHost {
        hub,
        players_told_left: false,
        settings: LanSettings {
            port: 0,
            guest_commands: true,
            force_game_mode: false,
        },
        play: Default::default(),
        broken: Vec::new(),
        mob_state: Default::default(),
        poses: Default::default(),
        mob_players: Default::default(),
        owner: None,
        waypoints: Default::default(),
        waypoint_dead: Default::default(),
        border_sent: None,
        commands: Vec::new(),
        quiet: Default::default(),
        far: None,
        native_commands: None,
        native_usages: Vec::new(),
        sleep_ignored: Default::default(),
        states: Default::default(),
        states_next_entity: None,
        idle_states: Default::default(),
    }
}
