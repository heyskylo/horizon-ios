//! The identity of a natural fence knot (`LeashFenceKnotEntity`): the tie
//! makes one real entity, with one id and one UUID, which commands and a
//! hosted server's tracker both name. Two leads on one fence share it, a
//! knot made again after its last lead went is a new entity, and the same
//! fence position in another level is another knot.
use super::*;
use horizon_client::{
    chunk::Chunk,
    lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig},
    mobs::leash::Holder,
    motion::PlayerMotion,
};
use horizon_core::position::ChunkPos;
use horizon_protocol::packets::configuration::ClientInformation;
use std::sync::{Arc, atomic::AtomicI32, mpsc};

const PIG: &str = "0000609a-0000-0000-0000-000000000001";
const OTHER_PIG: &str = "0000609a-0000-0000-0000-000000000003";
const AT_FENCE: &str = "@e[type=minecraft:leash_knot,x=8.5,y=201.375,z=0.5,distance=..0.001]";

fn game() -> Game<'static> {
    let root = std::path::PathBuf::from(
        std::env::var_os("HORIZON_TEST_RESOURCES")
            .expect("set HORIZON_TEST_RESOURCES to prepared resources"),
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
    options.execution_role = execution_role::ExecutionRole::IntegratedServer;
    let res: &'static _ = Box::leak(Box::new(loaded.resources));
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
    game.build_registries();
    game.set_allow_commands(true);
    for x in 0..2 {
        game.world
            .chunks
            .insert((x, 0), Arc::new(Chunk::empty(x, 0)));
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(x, 0))
            .unwrap();
    }
    game.player = Some(PlayerMotion::new(game.motion, Vec3::new(12.5, 201.0, 2.5)).unwrap());
    for command in [
        "forceload add 0 0 31 15",
        "fill 0 200 0 31 200 15 stone",
        "tick freeze",
        "setblock 8 201 0 oak_fence",
    ] {
        assert!(game.run_command(command), "{command}");
    }
    game
}

fn summon_pig(game: &mut Game<'_>, n: i32, x: f64) {
    let command = format!(
        "summon pig {x} 201 0.5 {{NoAI:1b,PersistenceRequired:1b,NoGravity:1b,leash:[I;8,201,0],UUID:[I;24730,0,0,{n}]}}"
    );
    assert!(game.run_command(&command), "{command}");
}

fn step(game: &mut Game<'_>) {
    assert!(game.run_command("tick step 1"));
    game.tick_server(false).unwrap();
}

fn mob(game: &mut Game<'_>, uuid: &str) -> usize {
    match game.target_with_uuid(horizon_client::save::uuid_from_text(uuid).unwrap()) {
        Some(super::commands::Target::Mob(id)) => {
            game.mobs.iter().position(|m| m.id == id).unwrap()
        }
        other => panic!("{other:?}"),
    }
}

/// The knots the level in place's selectors list, by entity id.
fn knots(game: &Game<'_>) -> Vec<u32> {
    game.level_candidates()
        .into_iter()
        .filter_map(|t| match t {
            super::commands::Target::Knot(id) => Some(id),
            _ => None,
        })
        .collect()
}

/// The UUID `/data` reads from the one knot of the level in place.
fn knot_uuid(game: &mut Game<'_>) -> [i32; 4] {
    let [id] = knots(game)[..] else {
        panic!("one knot: {:?}", knots(game));
    };
    let target = super::commands::Target::Knot(id);
    match game.entity_nbt(target).and_then(|t| t.get("UUID").cloned()) {
        Some(horizon_world::nbt::Tag::IntArray(v)) => [v[0], v[1], v[2], v[3]],
        other => panic!("{other:?}"),
    }
}

fn host(game: &mut Game<'_>) {
    let net = NetConfig {
        announce: false,
        ..NetConfig::lan(0, "knot-identity-test".into(), 2)
    };
    game.lan_host(
        super::lan::LanSettings {
            port: 0,
            guest_commands: true,
            force_game_mode: false,
        },
        net,
        HubConfig {
            default_game_mode: 1,
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
            name: "KnotGuest".into(),
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
        [12.5, 201.0, 4.5],
        1,
    );
    guest.placed = true;
    guest.loaded = true;
    game.lan.as_mut().unwrap().hub.guests.push(guest);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_server_that_starts_tracking_later_shows_the_same_knot() {
    let mut game = game();
    summon_pig(&mut game, 1, 16.5);
    step(&mut game);
    assert!(game.lan.is_none());
    let [id] = knots(&game)[..] else {
        panic!("{:?}", knots(&game));
    };
    let uuid = knot_uuid(&mut game);
    assert_eq!(game.persist.uuids.get(&id), Some(&uuid));
    // A later save prunes the memory of dead entities, not of this knot.
    game.prune_entity_memory();
    assert_eq!(knot_uuid(&mut game), uuid);
    // The world opens to the network; the tracker shows the same entity.
    host(&mut game);
    step(&mut game);
    step(&mut game);
    assert!(game.lan.is_some());
    game.lan_assign_ids();
    assert_eq!(
        game.knot_ids.get(&[8, 201, 0]),
        Some(&id),
        "the tracker's id"
    );
    assert_eq!(game.uuid_for(id), uuid, "the tracker's UUID");
    assert_eq!(knots(&game), [id]);
    assert_eq!(knot_uuid(&mut game), uuid);
    assert_eq!(
        game.network_entity_id(super::commands::Target::Knot(id)),
        Some(id as i32)
    );
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn two_leads_on_one_fence_share_one_knot() {
    let mut game = game();
    summon_pig(&mut game, 1, 16.5);
    summon_pig(&mut game, 3, 4.5);
    step(&mut game);
    for uuid in [PIG, OTHER_PIG] {
        let i = mob(&mut game, uuid);
        assert_eq!(game.mobs[i].extra.leash, Some(Holder::Knot([8, 201, 0])));
    }
    assert_eq!(knots(&game).len(), 1);
    assert!(game.run_command(&format!("execute if entity {AT_FENCE}")));
    // One lead goes: the knot stays the same entity.
    let id = knots(&game)[0];
    let uuid = knot_uuid(&mut game);
    let other = mob(&mut game, OTHER_PIG);
    let other = game.mobs[other].id;
    game.break_leash(other, false, false);
    assert_eq!(knots(&game), [id]);
    assert_eq!(knot_uuid(&mut game), uuid);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_knot_tied_again_after_its_last_lead_is_a_new_entity() {
    let mut game = game();
    summon_pig(&mut game, 1, 13.5);
    step(&mut game);
    let first = knots(&game)[0];
    let first_uuid = knot_uuid(&mut game);
    let pig = mob(&mut game, PIG);
    let pig = game.mobs[pig].id;
    game.break_leash(pig, false, false);
    assert!(knots(&game).is_empty(), "no lead holds it: gone");
    assert!(!game.run_command(&format!("execute if entity {AT_FENCE}")));
    // The player leads the pig and ties it to the fence again.
    assert!(game.run_command("item replace entity @s weapon.mainhand with lead"));
    let i = mob(&mut game, PIG);
    assert!(game.interact_leash(i, "minecraft:lead"));
    assert!(game.use_knot([8, 201, 0], false));
    let i = mob(&mut game, PIG);
    assert_eq!(game.mobs[i].extra.leash, Some(Holder::Knot([8, 201, 0])));
    let [second] = knots(&game)[..] else {
        panic!("{:?}", knots(&game));
    };
    assert_ne!(second, first, "a new entity id");
    assert_ne!(knot_uuid(&mut game), first_uuid, "a new UUID");
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn the_same_fence_position_in_another_level_is_another_knot() {
    let mut game = game();
    game.enter_command_level(Dim::Nether);
    assert_eq!(game.dim, Dim::Nether);
    for x in 0..2 {
        game.world
            .chunks
            .insert((x, 0), Arc::new(Chunk::empty(x, 0)));
        let _ = game.block_ticks.add_empty_chunk(ChunkPos::new(x, 0));
    }
    for command in [
        "forceload add 0 0 31 15",
        "fill 0 200 0 31 200 15 stone",
        "setblock 8 201 0 oak_fence",
        "summon pig 16.5 201 0.5 {NoAI:1b,PersistenceRequired:1b,NoGravity:1b,leash:[I;8,201,0],UUID:[I;24730,0,0,5]}",
    ] {
        assert!(game.run_command(command), "{command}");
    }
    game.enter_command_level(Dim::Overworld);
    assert_eq!(game.dim, Dim::Overworld);
    summon_pig(&mut game, 1, 16.5);
    step(&mut game);
    let here = knots(&game);
    game.enter_command_level(Dim::Nether);
    let there = knots(&game);
    game.enter_command_level(Dim::Overworld);
    let ([here], [there]) = (&here[..], &there[..]) else {
        panic!("one knot in each level: {here:?} {there:?}");
    };
    let (here, there) = (*here, *there);
    assert_ne!(here, there, "two entities");
    assert_ne!(game.persist.uuids.get(&here), None);
    // Each level's selector finds its own knot, by its own UUID (a selector
    // with a position looks in the source's level only).
    let uuid_here = knot_uuid(&mut game);
    assert!(game.run_command(&format!(
        "execute in minecraft:the_nether if entity {AT_FENCE}"
    )));
    let nether = format!(
        "execute in minecraft:the_nether if entity @e[type=minecraft:leash_knot,x=8.5,y=201.375,z=0.5,distance=..0.001,nbt={{UUID:[I;{},{},{},{}]}}]",
        uuid_here[0], uuid_here[1], uuid_here[2], uuid_here[3]
    );
    assert!(
        !game.run_command(&nether),
        "the overworld knot is not the nether's"
    );
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn commands_that_would_move_or_remove_a_natural_knot_fail() {
    let mut game = game();
    summon_pig(&mut game, 1, 16.5);
    step(&mut game);
    let id = knots(&game)[0];
    for command in [
        format!("kill {AT_FENCE}"),
        format!("tp {AT_FENCE} 20 201 4"),
        format!("teleport {AT_FENCE} ~ ~1 ~"),
        format!("rotate {AT_FENCE} 90 0"),
        format!("tag {AT_FENCE} add moved"),
        format!("spreadplayers 16 8 0 2 false {AT_FENCE}"),
        format!("data merge entity {AT_FENCE} {{Invulnerable:1b}}"),
    ] {
        assert!(!game.run_command(&command), "{command}");
    }
    assert_eq!(knots(&game), [id], "the knot is still there");
    let i = mob(&mut game, PIG);
    assert_eq!(game.mobs[i].extra.leash, Some(Holder::Knot([8, 201, 0])));
    let at = horizon_client::mobs::leash::knot_state([8, 201, 0]).position;
    assert_eq!(
        game.target_position(super::commands::Target::Knot(id)),
        Some([at.x, at.y, at.z])
    );
}
