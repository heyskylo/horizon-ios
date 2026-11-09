#[path = "hub/tests/sound_tests.rs"]
mod sound_tests;
use super::*;
use crate::fast_hash::FxHashSet;

/// A hub with one placed guest, the speed effect registered, and what the
/// guest is sent.
fn hub_with_a_guest() -> (Hub, std::sync::mpsc::Receiver<crate::lan::Out>) {
    let mut sync = SyncData::empty();
    sync.builtin.registries.insert(
        "minecraft:mob_effect".to_string(),
        horizon_protocol::registry::Ids::from_names(vec!["minecraft:speed".to_string()]),
    );
    hub_with_sync(sync)
}

/// [`hub_with_a_guest`] over a sync the test built.
fn hub_with_sync(sync: SyncData) -> (Hub, std::sync::mpsc::Receiver<crate::lan::Out>) {
    let config = crate::lan::NetConfig {
        announce: false,
        ..crate::lan::NetConfig::loopback(0, "EffectSyncTest".into(), 2)
    };
    let sync = std::sync::Arc::new(sync);
    let lan = Lan::start_with(sync.clone(), config).unwrap();
    let encoder = ChunkEncoder::new(
        [] as [(std::sync::Arc<str>, bool); 0],
        &horizon_protocol::registry::Builtin::default(),
        &[],
        &[],
    );
    let hub_config = HubConfig {
        default_game_mode: 0,
        guest_commands: false,
        force_game_mode: false,
        hardcore: false,
        view_distance: 10,
        simulation_distance: 10,
        dimension: "minecraft:overworld".into(),
        allow_flight: false,
        seed: 0,
        max_players: 20,
        integrated: false,
        levels: Default::default(),
    };
    let mut hub = Hub::new(lan, sync, encoder, hub_config);
    let (tx, rx) = std::sync::mpsc::channel();
    let mut bob = Guest::new(
        crate::lan::Joined {
            conn: 1,
            uuid: 1,
            name: "Bob".into(),
            properties: Vec::new(),
            info: horizon_protocol::packets::configuration::ClientInformation::new(8),
            brand: "vanilla".into(),
            sender: crate::lan::PlayerSender::new(tx, std::sync::Arc::default()),
            latency: std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0)),
            address: "127.0.0.1:25565".parse().unwrap(),
            expected: false,
            configured: Default::default(),
        },
        1,
        [0.5, 64.0, 0.5],
        8,
    );
    bob.placed = true;
    bob.dimension = "minecraft:overworld".into();
    hub.guests.push(bob);
    (hub, rx)
}

// Added after the old connected/Game pair: sender lifecycle controls only.
#[test]
fn self_pose_wire_lifecycle_preserves_existing_publishers() {
    use crate::pose::Pose;
    fn drain(
        rx: &std::sync::mpsc::Receiver<crate::lan::Out>,
    ) -> (Vec<Vec<(u8, i32, Value)>>, Vec<i32>) {
        let mut frames = Vec::new();
        let mut keeps = Vec::new();
        while let Ok(out) = rx.try_recv() {
            let crate::lan::Out::Packet(body) = out else {
                continue;
            };
            let mut r = horizon_protocol::buf::Reader::new(&body);
            let id = r.varint().unwrap();
            match C::decode(id, r.rest()).unwrap() {
                C::SetEntityData(p) if p.values.iter().any(|(i, _, _)| *i == 0 || *i == 6) => {
                    assert_eq!(p.id, 1, "actual self recipient");
                    frames.push(p.values);
                }
                C::Respawn(p) => keeps.push(p.keep as i32),
                _ => {}
            }
        }
        (frames, keeps)
    }
    let pose = |p: Pose| vec![(6, serializer::POSE, Value::Id(p.id()))];
    let flags = |f| vec![(0, serializer::BYTE, Value::Byte(f))];
    let (mut hub, rx) = hub_with_a_guest();
    hub.sync_player_state(0);
    assert!(drain(&rx).0.is_empty(), "default Standing is not dirty");
    hub.guests[0].sneaking = true;
    hub.sync_player_state(0);
    assert_eq!(drain(&rx).0, [flags(2), pose(Pose::Crouching)]);
    hub.sync_player_state(0);
    assert!(drain(&rx).0.is_empty());
    // The existing refused-glide force-flags marker never forces Pose.
    hub.guests[0].sent_shared ^= -128;
    hub.sync_player_state(0);
    assert_eq!(drain(&rx).0, [flags(2)]);
    hub.guests[0].sneaking = false;
    hub.sync_player_state(0);
    assert_eq!(drain(&rx).0, [flags(0), pose(Pose::Standing)]);
    hub.sync_player_state(0);
    assert!(drain(&rx).0.is_empty());
    hub.guests[0].bed = Some([1, 64, 2]);
    hub.guests[0].sleeping = true;
    hub.sync_player_state(0);
    assert_eq!(
        drain(&rx).0,
        [vec![
            (6, serializer::POSE, Value::Id(2)),
            (
                14,
                serializer::OPTIONAL_BLOCK_POS,
                Value::OptPos(Some([1, 64, 2]))
            ),
        ]]
    );
    assert_eq!(hub.guests[0].sent_pose, Pose::Sleeping);
    hub.wake(0);
    assert_eq!(
        drain(&rx).0,
        [vec![
            (6, serializer::POSE, Value::Id(0)),
            (14, serializer::OPTIONAL_BLOCK_POS, Value::OptPos(None)),
        ]]
    );
    hub.sync_player_state(0);
    assert!(drain(&rx).0.is_empty(), "wake already sent Standing");
    // The non-wake bed-delta publisher also owns its exact existing frame.
    hub.guests[0].bed = Some([1, 64, 2]);
    hub.guests[0].sleeping = true;
    hub.sync_player_state(0);
    drain(&rx);
    hub.guests[0].bed = None;
    hub.guests[0].sleeping = false;
    hub.sync_player_state(0);
    assert_eq!(
        drain(&rx).0,
        [vec![
            (6, serializer::POSE, Value::Id(0)),
            (14, serializer::OPTIONAL_BLOCK_POS, Value::OptPos(None)),
        ]]
    );
    hub.guests[0].fall_flying = true;
    hub.sync_player_state(0);
    assert!(
        drain(&rx)
            .0
            .iter()
            .all(|v| !v.iter().any(|(i, _, _)| *i == 6)),
        "other normal poses stay outside this sender"
    );
    hub.guests[0].fall_flying = false;
    hub.guests[0].sneaking = true;
    hub.sync_player_state(0);
    drain(&rx);
    let mut world = World::new(horizon_world::registry::BlockStateId(0), 0);
    let mut next_entity = 2;
    let border = crate::worldborder::WorldBorder::default();
    let ctx = TickCtx {
        sea_level: 63,
        world: &mut world,
        entities: &[],
        next_entity: &mut next_entity,
        spawn: [0.5, 64.0, 0.5],
        spawn_angle: [0.0; 2],
        difficulty: 2,
        difficulty_locked: false,
        clock_rate: 1.0,
        tick_rate: 20.0,
        tick_frozen: false,
        tick_steps: Default::default(),
        collision: None,
        border: &border,
        streaming: None,
    };
    hub.change_level(0, [100.5, 49.0, 0.5], 0.0, None, false, 0, &ctx);
    assert_eq!(drain(&rx).1, [3]);
    assert_eq!(hub.guests[0].sent_pose, Pose::Crouching);
    hub.sync_player_state(0);
    assert!(drain(&rx).0.is_empty(), "KEEP_ALL_DATA retains crouch");
    hub.guests[0].sneaking = false;
    hub.sync_player_state(0);
    assert_eq!(drain(&rx).0, [flags(0), pose(Pose::Standing)]);
    hub.guests[0].sneaking = true;
    hub.sync_player_state(0);
    drain(&rx);
    hub.respawn(0, [0.5, 64.0, 0.5], [0.0; 2], &ctx);
    assert_eq!(drain(&rx).1, [0]);
    assert_eq!(hub.guests[0].sent_pose, Pose::Standing);
    hub.sync_player_state(0);
    assert_eq!(
        drain(&rx).0,
        [pose(Pose::Crouching)],
        "KEEP_NONE reset makes the actual crouch dirty again"
    );
    hub.sync_player_state(0);
    assert!(drain(&rx).0.is_empty());
}

#[test]
fn transient_client_update_fields_reach_all_viewers_once_and_wait_without_viewers() {
    use crate::containers::{BlockEntity, ModBlockEntity, ModBlockEntityFields};
    use horizon_world::nbt::Compound;
    use std::sync::atomic::AtomicUsize;
    static CONSTRUCTIONS: AtomicUsize = AtomicUsize::new(0);
    CONSTRUCTIONS.store(0, Ordering::Relaxed);
    let (mut hub, first) = hub_with_a_guest();
    let builtin = horizon_protocol::registry::Builtin::parse(
        r#"{"minecraft:block_entity_type":{"entries":{"test:machine":{"protocol_id":49}}}}"#,
    )
    .unwrap();
    let states = ["minecraft:air", "test:machine"].map(|n| (Arc::<str>::from(n), false));
    hub.encoder = ChunkEncoder::new(states, &builtin, &[], &[]);
    hub.encoder.mod_update_tag = Some(|_, entity| {
        CONSTRUCTIONS.fetch_add(1, Ordering::Relaxed);
        let mut tag = entity.nbt.clone();
        tag.extend(entity.take_client_update_fields());
        Some(tag)
    });
    let (pos, retained) = (
        (1, 64, 3),
        Compound::from([("progress".into(), Tag::Int(7))]),
    );
    let mut world = World::new(horizon_world::registry::BlockStateId(0), 0);
    let mut chunk = Chunk::empty(0, 0);
    chunk.set(pos.0, pos.1, pos.2, 1);
    world.chunks.insert((0, 0), Arc::new(chunk));
    let mut machine = ModBlockEntity::new(ModBlockEntityFields {
        id: "test:machine".into(),
        nbt: retained.clone(),
        ..Default::default()
    });
    let revision = machine.revision();
    let effects = Tag::List(vec![Tag::Int(23)]);
    machine.set_client_update_fields(Compound::from([("effects".into(), effects.clone())]));
    world
        .block_entities
        .insert(pos, BlockEntity::Mod(Box::new(machine)));
    world.block_log = Some(vec![pos]);
    // No recipient has the chunk yet: an attempted broadcast must not
    // construct a tag, or consume an effect before a client can see it.
    hub.broadcast_block_changes(&mut world);
    assert_eq!(CONSTRUCTIONS.load(Ordering::Relaxed), 0);
    assert!(first.try_recv().is_err());
    hub.guests[0].sent_chunks.insert((0, 0));
    let (tx, second) = std::sync::mpsc::channel();
    let mut other = Guest::new(
        crate::lan::Joined {
            conn: 2,
            uuid: 2,
            name: "Ada".into(),
            properties: Vec::new(),
            info: horizon_protocol::packets::configuration::ClientInformation::new(8),
            brand: "vanilla".into(),
            sender: crate::lan::PlayerSender::new(tx, Arc::default()),
            latency: Arc::new(std::sync::atomic::AtomicI32::new(0)),
            address: "127.0.0.1:25566".parse().unwrap(),
            expected: false,
            configured: Default::default(),
        },
        2,
        [0.5, 64.0, 0.5],
        8,
    );
    other.placed = true;
    other.dimension = "minecraft:overworld".into();
    other.sent_chunks.insert((0, 0));
    hub.guests.push(other);
    let tags = |rx: &std::sync::mpsc::Receiver<crate::lan::Out>| {
        let mut tags = Vec::new();
        while let Ok(crate::lan::Out::Packet(body)) = rx.try_recv() {
            let mut r = horizon_protocol::buf::Reader::new(&body);
            let id = r.varint().unwrap();
            if let Ok(C::BlockEntityData(packet)) = C::decode(id, r.rest()) {
                assert_eq!(packet.pos, [pos.0, pos.1, pos.2]);
                tags.push(packet.tag);
            }
        }
        tags
    };
    world.block_log.as_mut().unwrap().extend([pos, pos]);
    hub.broadcast_block_changes(&mut world);
    let mut expected = retained.clone();
    expected.insert("effects".into(), effects);
    for recipient in [&first, &second] {
        assert_eq!(tags(recipient), [Tag::Compound(expected.clone())]);
    }
    assert_eq!(CONSTRUCTIONS.load(Ordering::Relaxed), 1);
    world.block_log.as_mut().unwrap().push(pos);
    hub.broadcast_block_changes(&mut world);
    for recipient in [&first, &second] {
        assert_eq!(tags(recipient), [Tag::Compound(retained.clone())]);
    }
    assert_eq!(CONSTRUCTIONS.load(Ordering::Relaxed), 2);
    let BlockEntity::Mod(machine) = &world.block_entities[&pos] else {
        unreachable!();
    };
    assert_eq!(machine.revision(), revision);
    assert_eq!(machine.nbt, retained);
}

/// `store_mod_commands` keeps a new mod tree without sending it (the
/// game sends it after the mods filtered it: `native_refresh_commands`),
/// and `set_mod_commands` still sends a changed tree, and only a changed one.
#[test]
fn stored_mod_commands_are_not_sent_until_asked() {
    let (mut hub, rx) = hub_with_a_guest();
    let trees = |rx: &std::sync::mpsc::Receiver<crate::lan::Out>| {
        let mut sent = 0;
        while let Ok(out) = rx.try_recv() {
            if let crate::lan::Out::Packet(body) = out {
                let mut r = horizon_protocol::buf::Reader::new(&body);
                let pid = r.varint().unwrap();
                if let Ok(C::Commands(_)) = C::decode(pid, r.rest()) {
                    sent += 1;
                }
            }
        }
        sent
    };
    let first = serde_json::json!({"hello": {}});
    assert!(hub.store_mod_commands(0, first.clone()));
    assert!(!hub.store_mod_commands(0, first.clone()));
    assert_eq!(trees(&rx), 0);
    assert_eq!(hub.mod_commands_key(), Some(0));
    hub.set_mod_commands(0, first);
    assert_eq!(trees(&rx), 0, "an unchanged tree is not sent again");
    hub.set_mod_commands(0, serde_json::json!({"hello": {}, "bye": {}}));
    assert_eq!(trees(&rx), 1);
    hub.resend_commands();
    assert_eq!(trees(&rx), 1, "every placed player gets their tree again");
}

/// `onGameRuleChanged`: a changed `reduced_debug_info` reaches each player
/// as their own entity event 22 / 23, and an unchanged one sends nothing.
#[test]
fn reduced_debug_info_changes_reach_players_as_entity_events() {
    let (mut hub, rx) = hub_with_a_guest();
    let id = hub.guests[0].entity_id;
    let events = |rx: &std::sync::mpsc::Receiver<crate::lan::Out>| {
        let mut sent = Vec::new();
        while let Ok(crate::lan::Out::Packet(body)) = rx.try_recv() {
            let mut r = horizon_protocol::buf::Reader::new(&body);
            let pid = r.varint().unwrap();
            if let Ok(C::EntityEvent(p)) = C::decode(pid, r.rest()) {
                sent.push((p.id, p.event));
            }
        }
        sent
    };
    hub.set_reduced_debug_info(false);
    assert!(events(&rx).is_empty());
    hub.set_reduced_debug_info(true);
    hub.set_reduced_debug_info(true);
    assert_eq!(events(&rx), vec![(id, 22)]);
    hub.set_reduced_debug_info(false);
    assert_eq!(events(&rx), vec![(id, 23)]);
}

/// The blocks the guest was sent as `ClientboundBlockUpdatePacket`s, in order, with the
/// state each one carries.
fn sent_block_updates(rx: &std::sync::mpsc::Receiver<crate::lan::Out>) -> Vec<([i32; 3], i32)> {
    let mut sent = Vec::new();
    while let Ok(out) = rx.try_recv() {
        let crate::lan::Out::Packet(body) = out else {
            continue;
        };
        let mut r = horizon_protocol::buf::Reader::new(&body);
        let id = r.varint().unwrap();
        if let Ok(C::BlockUpdate(p)) = C::decode(id, r.rest()) {
            sent.push((p.pos, p.state));
        }
    }
    sent
}

/// A world whose blocks at y = 63 are `state` under the four chunks around the origin.
fn floor_at(state: u32) -> World {
    let mut world = World::new(horizon_world::registry::BlockStateId(0), 0);
    for cz in -1..=0 {
        for cx in -1..=0 {
            let mut chunk = crate::chunk::Chunk::empty(cx, cz);
            for z in 0..16 {
                for x in 0..16 {
                    chunk.set(x, 63, z, state);
                }
            }
            chunk.lit = true;
            world.chunks.insert((cx, cz), std::sync::Arc::new(chunk));
        }
    }
    world
}

/// One tick of the hub that handles a single packet from the guest.
fn tick_packet(hub: &mut Hub, world: &mut World, packet: S) {
    hub.guests[0].inbox.push_back((packet, 0));
    let mut next_entity = 2;
    let border = crate::worldborder::WorldBorder::default();
    let mut ctx = TickCtx {
        sea_level: 63,
        world,
        entities: &[],
        next_entity: &mut next_entity,
        spawn: [0.5, 64.0, 0.5],
        spawn_angle: [0.0; 2],
        difficulty: 2,
        difficulty_locked: false,
        clock_rate: 1.0,
        tick_rate: 20.0,
        tick_frozen: false,
        tick_steps: Default::default(),
        collision: None,
        border: &border,
        streaming: None,
    };
    let _ = hub.drain_packets(&mut ctx);
}

/// One tick of the hub that handles a single position move from the guest.
fn tick_guest_move(hub: &mut Hub, world: &mut World, to: [f64; 3], flags: u8) {
    // One position move per client tick (receivedPositionThisTick).
    hub.guests[0].position_this_tick = false;
    tick_packet(
        hub,
        world,
        S::MovePlayerPos(play::MovePlayerPos {
            x: to[0],
            y: to[1],
            z: to[2],
            flags,
        }),
    );
}

/// `ClientboundRespawnPacket.KEEP_ALL_DATA`: a level change keeps the client's copy of the
/// player's shared flags, so one that clears afterwards still reaches it. (A creative player
/// who leaves lava through a dimension change kept the fire on its screen.)
#[test]
fn a_shared_flag_cleared_after_a_level_change_reaches_the_player() {
    let (mut hub, rx) = hub_with_a_guest();
    let shared_flags_sent = |rx: &std::sync::mpsc::Receiver<crate::lan::Out>| {
        let mut sent = Vec::new();
        while let Ok(out) = rx.try_recv() {
            let crate::lan::Out::Packet(body) = out else {
                continue;
            };
            let mut r = horizon_protocol::buf::Reader::new(&body);
            let id = r.varint().unwrap();
            if let Ok(C::SetEntityData(packet)) = C::decode(id, r.rest()) {
                sent.extend(packet.values.iter().filter_map(|(index, _, value)| {
                    match (index, value) {
                        (0, Value::Byte(flags)) => Some(*flags),
                        _ => None,
                    }
                }));
            }
        }
        sent
    };
    hub.guests[0].sneaking = true;
    hub.sync_player_state(0);
    assert_eq!(shared_flags_sent(&rx), [0x02]);
    let mut world = World::new(horizon_world::registry::BlockStateId(0), 0);
    let mut next_entity = 2;
    let border = crate::worldborder::WorldBorder::default();
    let ctx = TickCtx {
        sea_level: 63,
        world: &mut world,
        entities: &[],
        next_entity: &mut next_entity,
        spawn: [0.5, 64.0, 0.5],
        spawn_angle: [0.0; 2],
        difficulty: 2,
        difficulty_locked: false,
        clock_rate: 1.0,
        tick_rate: 20.0,
        tick_frozen: false,
        tick_steps: Default::default(),
        collision: None,
        border: &border,
        streaming: None,
    };
    hub.change_level(0, [100.5, 49.0, 0.5], 0.0, None, false, 0, &ctx);
    let _ = shared_flags_sent(&rx);
    hub.guests[0].sneaking = false;
    hub.sync_player_state(0);
    assert_eq!(shared_flags_sent(&rx), [0]);
}

/// `ServerGamePacketListenerImpl.forceSendPlayerSupportBlocks`: a client that says it stands
/// while the last move the server resolved came down onto nothing is sent the blocks under
/// its box again (its four corners, each once), so a client that missed a block change cannot
/// stay standing on air. At most once every 200 ticks of the connection.
#[test]
fn a_guest_standing_on_nothing_is_sent_the_blocks_below_it() {
    // Block state 1 is stone in the vanilla registry order.
    const STONE: u32 = 1;
    let (mut hub, rx) = hub_with_a_guest();
    hub.guests[0].load_timeout = 0;
    // Across a block corner: the box's four corners are four different blocks.
    hub.guests[0].position = [0.0, 64.0, 0.0];
    let mut world = floor_at(STONE);
    // The connection's 200th tick is the first that may send: vanilla starts the count at 0.
    hub.ticks = 199;
    tick_guest_move(&mut hub, &mut world, [0.0, 64.0, 0.0], 1);
    assert!(
        sent_block_updates(&rx).is_empty(),
        "the first 200 ticks of a connection send nothing"
    );
    hub.ticks = 200;
    tick_guest_move(&mut hub, &mut world, [0.0, 64.0, 0.0], 1);
    assert_eq!(
        sent_block_updates(&rx),
        vec![
            ([-1, 63, -1], STONE as i32),
            ([0, 63, -1], STONE as i32),
            ([-1, 63, 0], STONE as i32),
            ([0, 63, 0], STONE as i32),
        ],
        "the four blocks under the box, each once, with the server's states"
    );
    // A tick later: the next 200 have to pass first.
    hub.ticks = 201;
    tick_guest_move(&mut hub, &mut world, [0.0, 64.0, 0.0], 1);
    assert!(
        sent_block_updates(&rx).is_empty(),
        "no more than once every 200 ticks"
    );
    // The last move the server resolved came down onto something: it stands on its own.
    hub.guests[0].rests_on_ground = true;
    hub.ticks = 500;
    tick_guest_move(&mut hub, &mut world, [0.0, 64.0, 0.0], 1);
    assert!(
        sent_block_updates(&rx).is_empty(),
        "a player the server saw land is not resent the blocks"
    );
    // A client that does not claim the ground is not helped either.
    hub.guests[0].rests_on_ground = false;
    hub.ticks = 800;
    tick_guest_move(&mut hub, &mut world, [0.0, 64.0, 0.0], 0);
    assert!(
        sent_block_updates(&rx).is_empty(),
        "only a claim of standing on the ground asks for the blocks below"
    );
    // A box over one block gets that block once (the corners are the same cell).
    hub.ticks = 1100;
    hub.guests[0].position = [0.5, 64.0, 0.5];
    tick_guest_move(&mut hub, &mut world, [0.5, 64.0, 0.5], 1);
    assert_eq!(
        sent_block_updates(&rx),
        vec![([0, 63, 0], STONE as i32)],
        "one block under a box that sits across a block"
    );
}

/// The tabs the guest was sent in `ClientboundSelectAdvancementsTabPacket`s, in order.
fn sent_tabs(rx: &std::sync::mpsc::Receiver<crate::lan::Out>) -> Vec<Option<String>> {
    let mut sent = Vec::new();
    while let Ok(out) = rx.try_recv() {
        let crate::lan::Out::Packet(body) = out else {
            continue;
        };
        let mut r = horizon_protocol::buf::Reader::new(&body);
        let id = r.varint().unwrap();
        if let Ok(C::SelectAdvancementsTab(p)) = C::decode(id, r.rest()) {
            sent.push(p.tab);
        }
    }
    sent
}

/// A data folder holding two advancements: `minecraft:root`, a root with a display, and
/// `minecraft:task`, which hangs off it.
fn advancement_data() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "horizon-advancement-tab-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let folder = root.join("data").join("minecraft").join("advancement");
    std::fs::create_dir_all(&folder).unwrap();
    let display = |icon: &str, title: &str| {
        format!(r#"{{"icon":{{"id":"{icon}"}},"title":"{title}","description":"{title}"}}"#)
    };
    std::fs::write(
        folder.join("root.json"),
        format!(
            r#"{{"display":{},"criteria":{{}}}}"#,
            display("minecraft:stone", "Root")
        ),
    )
    .unwrap();
    std::fs::write(
        folder.join("task.json"),
        format!(
            r#"{{"parent":"minecraft:root","display":{},"criteria":{{}}}}"#,
            display("minecraft:dirt", "Task")
        ),
    )
    .unwrap();
    root
}

/// ServerGamePacketListenerImpl.handleSeenAdvancements with PlayerAdvancements.setSelectedTab:
/// the tab a player opens is kept when it names a root with a display, and the player is told
/// the tab the server keeps — none for anything else — whenever that changed. A closed screen
/// says nothing about the tab.
#[test]
fn an_advancement_tab_a_guest_opens_is_kept_and_answered() {
    let data = advancement_data();
    let mut sync = SyncData::empty();
    sync.advancements = crate::lan::advancements::ServerAdvancements::load(
        &data,
        &horizon_world::loot::PackOverlay::NONE,
        Default::default(),
        Default::default(),
    );
    assert_eq!(
        sync.advancements.selectable("minecraft:root"),
        Some(true),
        "the fixture's root is selectable"
    );
    let (mut hub, rx) = hub_with_sync(sync);
    hub.guests[0].load_timeout = 0;
    let mut world = World::new(horizon_world::registry::BlockStateId(0), 0);
    let open = |tab: &str| {
        S::SeenAdvancements(play::SeenAdvancements {
            action: 0,
            tab: Some(tab.to_owned()),
        })
    };
    // The root the client opens is the tab the server keeps now.
    tick_packet(&mut hub, &mut world, open("minecraft:root"));
    assert_eq!(sent_tabs(&rx), vec![Some("minecraft:root".to_owned())]);
    assert_eq!(
        hub.guests[0].selected_tab.as_deref(),
        Some("minecraft:root")
    );
    // Opening it again changes nothing, so nothing is sent.
    tick_packet(&mut hub, &mut world, open("minecraft:root"));
    assert!(
        sent_tabs(&rx).is_empty(),
        "an unchanged tab is not sent again"
    );
    // A tab that is not a root takes the selection away, and the client is told so.
    tick_packet(&mut hub, &mut world, open("minecraft:task"));
    assert_eq!(sent_tabs(&rx), vec![None]);
    assert_eq!(hub.guests[0].selected_tab, None);
    // The root again, then an id the server has no advancement for: vanilla's
    // `ServerAdvancementManager.get` is null, `setSelectedTab` is not called, and the
    // tab stays.
    tick_packet(&mut hub, &mut world, open("minecraft:root"));
    assert_eq!(sent_tabs(&rx), vec![Some("minecraft:root".to_owned())]);
    tick_packet(&mut hub, &mut world, open("minecraft:nowhere"));
    assert!(sent_tabs(&rx).is_empty(), "an unknown id changes nothing");
    assert_eq!(
        hub.guests[0].selected_tab.as_deref(),
        Some("minecraft:root")
    );
    // The closed screen keeps the tab the server has.
    tick_packet(
        &mut hub,
        &mut world,
        S::SeenAdvancements(play::SeenAdvancements {
            action: 1,
            tab: None,
        }),
    );
    assert!(sent_tabs(&rx).is_empty(), "a closed screen keeps the tab");
    assert_eq!(
        hub.guests[0].selected_tab.as_deref(),
        Some("minecraft:root")
    );
    let _ = std::fs::remove_dir_all(&data);
}

/// `handlePlayerCommand` START_RIDING_JUMP (3) and STOP_RIDING_JUMP (4): the
/// hub used to drop both, so a guest's released jump bar never reached the
/// mount they ride, and neither did the horse's rear or its jump sound.
#[test]
fn a_guests_riding_jump_reaches_the_game() {
    let (mut hub, _rx) = hub_with_a_guest();
    // hasClientLoaded: the handlers ignore a player's actions until then.
    hub.guests[0].load_timeout = 0;
    let mut world = crate::world::World::new(horizon_world::registry::BlockStateId(0), 0);
    let mut next_entity = 100;
    let border = crate::worldborder::WorldBorder::default();
    let mut ctx = TickCtx {
        sea_level: 63,
        world: &mut world,
        entities: &[],
        next_entity: &mut next_entity,
        spawn: [0.5, 64.0, 0.5],
        spawn_angle: [0.0; 2],
        difficulty: 2,
        difficulty_locked: false,
        clock_rate: 1.0,
        tick_rate: 20.0,
        tick_frozen: false,
        tick_steps: Default::default(),
        collision: None,
        border: &border,
        streaming: None,
    };
    for action in [3, 4] {
        hub.guests[0].inbox.push_back((
            S::PlayerCommand(play::PlayerCommand {
                id: 1,
                action,
                data: 75,
            }),
            4,
        ));
        hub.guests[0].inbox_bytes += 4;
        let events = hub.tick(&mut ctx);
        let reached = events.iter().any(|event| {
                matches!(event, HostEvent::Packet(1, packet) if matches!(&**packet, S::PlayerCommand(c) if c.action == action && c.data == 75))
            });
        assert!(reached, "action {action} reached the game");
    }
}

/// The durations of the effect packets the guest was sent.
fn sent_durations(rx: &std::sync::mpsc::Receiver<crate::lan::Out>) -> Vec<i32> {
    let mut durations = Vec::new();
    while let Ok(out) = rx.try_recv() {
        let crate::lan::Out::Packet(body) = out else {
            continue;
        };
        let mut r = horizon_protocol::buf::Reader::new(&body);
        let id = r.varint().unwrap();
        if let Ok(C::UpdateMobEffect(p)) = C::decode(id, r.rest()) {
            durations.push(p.duration);
        }
    }
    durations
}

fn sent_frozen(rx: &std::sync::mpsc::Receiver<crate::lan::Out>) -> Vec<i32> {
    let mut sent = Vec::new();
    while let Ok(out) = rx.try_recv() {
        let crate::lan::Out::Packet(body) = out else {
            continue;
        };
        let mut r = horizon_protocol::buf::Reader::new(&body);
        let id = r.varint().unwrap();
        if let Ok(C::SetEntityData(p)) = C::decode(id, r.rest()) {
            for (index, _, value) in p.values {
                if let (7, Value::Int(ticks)) = (index, value) {
                    sent.push(ticks);
                }
            }
        }
    }
    sent
}

#[test]
fn a_guests_frozen_ticks_go_to_its_client() {
    let root = std::env::var_os("HORIZON_TEST_RESOURCES")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../.local/horizon-client")
                .join(horizon_core::VANILLA_VERSION)
        });
    if !root.join("movement-properties.nbt").exists() {
        return;
    }
    let assets = crate::assets::Assets::load(&root).unwrap();
    let resources = crate::motion::MotionResources::load(&root, &assets).unwrap();
    let mut body =
        crate::motion::PlayerMotion::new(&resources, horizon_core::geometry::Vec3::ZERO).unwrap();
    body.ticks_frozen = 140;
    let (mut hub, rx) = hub_with_a_guest();
    hub.guests[0].motion = Some(Box::new(body));
    hub.sync_player_state(0);
    assert_eq!(sent_frozen(&rx), vec![140]);
    hub.sync_player_state(0);
    assert_eq!(sent_frozen(&rx), Vec::<i32>::new());
    hub.guests[0].motion.as_mut().unwrap().ticks_frozen = 0;
    hub.sync_player_state(0);
    assert_eq!(sent_frozen(&rx), vec![0]);
}

/// The host and port of the transfer packets the guest was sent.
fn sent_transfers(rx: &std::sync::mpsc::Receiver<crate::lan::Out>) -> Vec<(String, i32)> {
    let mut sent = Vec::new();
    while let Ok(out) = rx.try_recv() {
        let crate::lan::Out::Packet(body) = out else {
            continue;
        };
        let mut r = horizon_protocol::buf::Reader::new(&body);
        let id = r.varint().unwrap();
        if let Ok(C::Transfer(p)) = C::decode(id, r.rest()) {
            sent.push((p.host, p.port));
        }
    }
    sent
}

/// `TransferCommand.transfer`: the target's client is sent the play-state
/// `ClientboundTransferPacket` with the hostname and the port, and nothing
/// else disconnects it — the client closes and logs in there itself
/// (`ClientPacketListener.handleTransfer`).
#[test]
fn a_transferred_player_is_sent_the_host_and_the_port() {
    let (hub, rx) = hub_with_a_guest();
    hub.transfer(1, "example.net", 25566);
    assert_eq!(sent_transfers(&rx), vec![("example.net".to_owned(), 25566)]);
    // A connection the hub does not know gets nothing.
    hub.transfer(9, "example.net", 25566);
    assert_eq!(
        sent_transfers(&rx),
        Vec::<(String, i32)>::new(),
        "an unknown connection is not a player of this server"
    );
}

/// `/effect give Bob minecraft:speed 30 1` from a dedicated server's
/// console (#7115). Vanilla runs the console after the players' tick
/// (`DedicatedServer.tickConnection`) and `ServerPlayer.onEffectAdded`
/// sends the effect at once: 30 s is 600 ticks, and the tick after, which
/// counts it down to 599, sends nothing (`LivingEntity.tickEffects` only
/// refreshes it at a multiple of 600 left). The console syncs right after
/// its command (`console_command`) for the same packet.
#[test]
fn an_effect_synced_after_its_command_is_sent_once_at_its_full_duration() {
    let (mut hub, rx) = hub_with_a_guest();
    hub.guests[0]
        .effects
        .add(EffectInstance::new("speed", 30 * 20, 1));
    // The console's command, then its sync.
    hub.sync_all_effects();
    // The next tick: the guests' effects count down (`tick_guests`), then
    // the hub syncs them (`tick_level`).
    hub.guests[0].effects.tick();
    hub.sync_all_effects();
    hub.guests[0].effects.tick();
    hub.sync_all_effects();
    assert_eq!(sent_durations(&rx), vec![600]);
    // Without the sync after the command, the first sync comes after the
    // tick has counted it down: what #7115's transcript shows.
    let (mut late, rx) = hub_with_a_guest();
    late.guests[0]
        .effects
        .add(EffectInstance::new("speed", 30 * 20, 1));
    late.guests[0].effects.tick();
    late.sync_all_effects();
    assert_eq!(sent_durations(&rx), vec![599]);
}

/// Minecraft.startAttack sends the attack, then the arm swing, and the next client tick
/// a turn. Vanilla handles both in the tick they came in; the swing starts the attack
/// strength again, so a swing left for the next tick took a tick off every hit's
/// charge: a guest's fists, 5 ticks apart, no longer broke a minecart (#7817).
#[test]
fn an_attacks_swing_goes_with_it_past_the_move_behind() {
    let (mut hub, _rx) = hub_with_a_guest();
    hub.guests[0].load_timeout = 0;
    let inbox = &mut hub.guests[0].inbox;
    inbox.push_back((S::Attack(play::Attack { id: 7 }), 0));
    inbox.push_back((S::Punch(horizon_protocol::packets::Empty), 0));
    inbox.push_back((
        S::MovePlayerRot(play::MovePlayerRot {
            yaw: 90.0,
            pitch: 0.0,
            flags: 1,
        }),
        0,
    ));
    let mut world = World::new(horizon_world::registry::BlockStateId(0), 0);
    let mut next_entity = 2;
    let border = crate::worldborder::WorldBorder::default();
    let mut ctx = TickCtx {
        sea_level: 63,
        world: &mut world,
        entities: &[],
        next_entity: &mut next_entity,
        spawn: [0.5, 64.0, 0.5],
        spawn_angle: [0.0; 2],
        difficulty: 2,
        difficulty_locked: false,
        clock_rate: 1.0,
        tick_rate: 20.0,
        tick_frozen: false,
        tick_steps: Default::default(),
        collision: None,
        border: &border,
        streaming: None,
    };
    let events = hub.drain_packets(&mut ctx);
    let handled: Vec<&S> = events
        .iter()
        .filter_map(|e| match e {
            HostEvent::Packet(_, p) => Some(&**p),
            _ => None,
        })
        .collect();
    assert!(
        matches!(handled[..], [S::Attack(_), S::Punch(_)]),
        "the game was handed {handled:?}"
    );
    assert!(
        matches!(
            hub.guests[0].inbox.iter().collect::<Vec<_>>()[..],
            [(S::MovePlayerRot(_), _)]
        ),
        "the turn did not wait for the next tick"
    );
}

/// Drains guest 0's inbox holding a player action (`action`) and the move
/// its client sent after it in the same client tick: the packets handed to
/// the game, and the guest's position and known movement before.
fn drain_action_then_move(action: i32) -> (Hub, Vec<S>, ([f64; 3], [f64; 3])) {
    let (mut hub, _rx) = hub_with_a_guest();
    hub.guests[0].load_timeout = 0;
    let before = (hub.guests[0].position, hub.guests[0].known_movement);
    let inbox = &mut hub.guests[0].inbox;
    inbox.push_back((
        S::PlayerAction(play::PlayerAction {
            action,
            pos: [0; 3],
            direction: 0,
            sequence: 0,
        }),
        0,
    ));
    inbox.push_back((
        S::MovePlayerPos(play::MovePlayerPos {
            x: before.0[0] + 0.3,
            y: before.0[1],
            z: before.0[2],
            flags: 1,
        }),
        0,
    ));
    let mut world = World::new(horizon_world::registry::BlockStateId(0), 0);
    let mut next_entity = 2;
    let border = crate::worldborder::WorldBorder::default();
    let mut ctx = TickCtx {
        sea_level: 63,
        world: &mut world,
        entities: &[],
        next_entity: &mut next_entity,
        spawn: [0.5, 64.0, 0.5],
        spawn_angle: [0.0; 2],
        difficulty: 2,
        difficulty_locked: false,
        clock_rate: 1.0,
        tick_rate: 20.0,
        tick_frozen: false,
        tick_steps: Default::default(),
        collision: None,
        border: &border,
        streaming: None,
    };
    let handled = hub
        .drain_packets(&mut ctx)
        .into_iter()
        .filter_map(|e| match e {
            HostEvent::Packet(_, p) => Some(*p),
            _ => None,
        })
        .collect();
    (hub, handled, before)
}

/// #8423: a RELEASE_USE_ITEM shoots from the player's position and known
/// movement of the last move the server handled; the move the client sent
/// after it waits for the next tick, as vanilla handles the two in order.
#[test]
fn a_release_shoots_before_the_move_behind_it() {
    let (hub, handled, before) = drain_action_then_move(6);
    assert!(
        matches!(handled[..], [S::PlayerAction(_)]),
        "the game was handed {handled:?}"
    );
    assert!(
        matches!(
            hub.guests[0].inbox.iter().collect::<Vec<_>>()[..],
            [(S::MovePlayerPos(_), _)]
        ),
        "the move did not wait for the next tick"
    );
    let g = &hub.guests[0];
    assert_eq!(
        (g.position, g.known_movement),
        before,
        "the shot's position"
    );
}

/// A STAB's move runs in the same tick: held for the next one, the jump it
/// carries came a tick after the Lunge's impulse (lunge_motion).
#[test]
fn a_stab_keeps_the_move_behind_it_in_its_tick() {
    let (hub, handled, before) = drain_action_then_move(8);
    assert!(
        matches!(handled[..], [S::PlayerAction(_)]),
        "the game was handed {handled:?}"
    );
    let g = &hub.guests[0];
    assert!(g.inbox.is_empty(), "the move waited for the next tick");
    assert_eq!(g.position[0], before.0[0] + 0.3, "the move was not handled");
}

/// The owner's own shoulder parrots are sent when a slot changes.
#[test]
fn the_owner_is_sent_the_shoulder_slots_that_changed() {
    assert!(shoulder_changes([None; 2], [None; 2]).is_empty());
    let landed = shoulder_changes([Some(1), None], [None; 2]);
    assert_eq!(landed.len(), 1);
    assert_eq!((landed[0].0, &landed[0].2), (19, &Value::Id(2)));
    let left = shoulder_changes([None; 2], [Some(1), Some(4)]);
    assert_eq!(left.len(), 2);
    assert_eq!((left[1].0, &left[1].2), (20, &Value::Id(0)));
}

/// The counts of what is stuck in a player are sent when set, and a
/// count back at zero is sent as the default.
#[test]
fn stuck_counts_are_synched_as_entity_data() {
    let mut state = EntityState::default();
    stuck_meta(&mut state, (3, 0));
    assert_eq!(state.meta, vec![(12, serializer::INT, Value::Int(3))]);
    assert_eq!(
        state.defaults,
        vec![
            (12, serializer::INT, Value::Int(0)),
            (13, serializer::INT, Value::Int(0))
        ]
    );
}

/// Joined states are used in the server tick after the one that noted
/// them, never later.
#[test]
fn joined_states_last_only_into_the_next_tick() {
    let joined = vec![EntityState {
        id: 7,
        ..EntityState::default()
    }];
    assert_eq!(noted_last_tick(Some((5, joined.clone())), 6), joined);
    assert!(noted_last_tick(Some((5, joined.clone())), 5).is_empty());
    assert!(noted_last_tick(Some((5, joined)), 9).is_empty());
    assert!(noted_last_tick(None, 6).is_empty());
}

/// `getMaximumFlyingTicks`: 80 ticks at normal gravity, ceil(80 * 0.08 /
/// gravity) where it is weaker, and never a kick without gravity.
#[test]
fn maximum_flying_ticks_follow_gravity() {
    assert_eq!(maximum_flying_ticks(0.08), 80);
    assert_eq!(maximum_flying_ticks(0.16), 80);
    assert_eq!(maximum_flying_ticks(0.04), 160);
    assert_eq!(maximum_flying_ticks(0.03), 214);
    assert_eq!(maximum_flying_ticks(0.0), i32::MAX);
    assert_eq!(maximum_flying_ticks(1.0E-6), i32::MAX);
}

fn textures(value: &str) -> horizon_protocol::packets::login::Property {
    horizon_protocol::packets::login::Property {
        name: "textures".into(),
        value: value.into(),
        signature: Some("c2lnbmVk".into()),
    }
}

/// The owner is listed with the account's skin once the fetch answered,
/// with none before (no wait) or when it failed; other players keep the
/// properties they logged in with, and none of the owner's.
#[test]
fn the_owner_is_listed_with_the_accounts_skin_once_fetched() {
    let (answer, fetched) = std::sync::mpsc::channel();
    let profile = super::super::OwnerProfile::new(move || fetched.recv().unwrap_or_default());
    let start = std::time::Instant::now();
    assert!(listed_properties(&[], true, Some(&profile)).is_empty());
    assert!(start.elapsed() < std::time::Duration::from_millis(500));
    answer.send(vec![textures("owner")]).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while listed_properties(&[], true, Some(&profile)).is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "the fetch never landed"
        );
        std::thread::yield_now();
    }
    assert_eq!(
        listed_properties(&[], true, Some(&profile)),
        vec![textures("owner")]
    );
    // A guest: its own properties (online mode), or none (offline mode).
    assert_eq!(
        listed_properties(&[textures("guest")], false, Some(&profile)),
        vec![textures("guest")]
    );
    assert!(listed_properties(&[], false, Some(&profile)).is_empty());
    // No signed-in account (no fetch): as before.
    assert!(listed_properties(&[], true, None).is_empty());
    let failed = super::super::OwnerProfile::new(Vec::new);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while failed.properties.get().is_none() {
        let _ = failed.properties();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(listed_properties(&[], true, Some(&failed)).is_empty());
}

/// `requeue` as it was, scanning the queue for each chunk of the view.
fn requeue_scan(
    queue: &mut Vec<(i32, i32)>,
    sent: &FxHashSet<(i32, i32)>,
    (cx, cz): (i32, i32),
    r: i32,
) {
    queue.retain(|&(x, z)| tracked(cx, cz, r, x, z));
    for dz in -r - 2..=r + 2 {
        for dx in -r - 2..=r + 2 {
            let key = (cx + dx, cz + dz);
            if tracked(cx, cz, r, key.0, key.1) && !sent.contains(&key) && !queue.contains(&key) {
                queue.push(key);
            }
        }
    }
    queue.sort_by_key(|&(x, z)| i64::from(x - cx).pow(2) + i64::from(z - cz).pow(2));
}

/// A player walking a loop, sending a few chunks a tick: the queue
/// after every move is the one the scan built.
#[test]
fn requeue_matches_the_scan() {
    for r in [2, 5, 10, 12, 32] {
        let (mut a, mut b) = (Vec::new(), Vec::new());
        let mut sent = FxHashSet::default();
        for step in 0..60i32 {
            let center = ((step % 17) - 8, (step * 3 % 11) - 5);
            requeue(&mut a, &sent, center, r);
            requeue_scan(&mut b, &sent, center, r);
            assert_eq!(a, b, "radius {r}, step {step}");
            for key in a.drain(..a.len().min(7)) {
                sent.insert(key);
            }
            b.drain(..b.len().min(7));
            sent.retain(|&(x, z)| tracked(center.0, center.1, r, x, z));
        }
    }
}

/// The far ring's queue is what the view's leaves of the chunks tracked at the reach, closest
/// first, with the sent ones out of both.
#[test]
fn the_far_ring_queues_what_the_view_leaves() {
    let mut sent = FxHashSet::default();
    let (mut near, mut far) = (Vec::new(), Vec::new());
    for step in 0..12 {
        let center = (step, -step / 2);
        requeue(&mut near, &sent, center, 10);
        requeue_ring(&mut far, &sent, center, 10, 24);
        let mut all = Vec::new();
        requeue(&mut all, &sent, center, 24);
        assert!(far.iter().all(|key| !near.contains(key)));
        let mut both = [near.clone(), far.clone()].concat();
        both.sort_unstable();
        all.sort_unstable();
        assert_eq!(both, all);
        let distance =
            |&(x, z): &(i32, i32)| i64::from(x - center.0).pow(2) + i64::from(z - center.1).pow(2);
        assert!(far.windows(2).all(|w| distance(&w[0]) <= distance(&w[1])));
        // A few of each sent.
        for key in near
            .drain(..near.len().min(40))
            .chain(far.drain(..far.len().min(90)))
        {
            sent.insert(key);
        }
    }
    // Without a ring, nothing.
    requeue_ring(&mut far, &sent, (0, 0), 10, 10);
    assert!(far.is_empty());
}

/// `MinecraftServer.onGameRuleChanged`: setting `limited_crafting` updates the hub's state
/// and broadcasts `ClientboundGameEventPacket` (event 12) with 1.0 (on) or 0.0 (off) to guests.
#[test]
fn set_limited_crafting_broadcasts_game_event() {
    let (mut hub, rx) = hub_with_a_guest();
    assert!(!hub.limited_crafting);

    // Setting to true broadcasts game event 12 with param 1.0
    hub.set_limited_crafting(true);
    assert!(hub.limited_crafting);

    let out = rx.try_recv().expect("packet should be sent to guest");
    let body = match out {
        crate::lan::Out::Packet(b) => b,
        _ => panic!("expected Out::Packet"),
    };
    let mut reader = horizon_protocol::buf::Reader::new(&body);
    let id = reader.varint().unwrap();
    let packet = play::Clientbound::decode(id, &body[reader.position()..]).unwrap();
    match packet {
        play::Clientbound::GameEvent(p) => {
            assert_eq!(p.event, game_event::LIMITED_CRAFTING);
            assert_eq!(p.param, 1.0);
        }
        other => panic!("expected GameEvent, got {other:?}"),
    }

    // Setting to the same state should not broadcast another packet
    hub.set_limited_crafting(true);
    assert!(rx.try_recv().is_err());

    // Setting to false broadcasts game event 12 with param 0.0
    hub.set_limited_crafting(false);
    assert!(!hub.limited_crafting);

    let out = rx.try_recv().expect("packet should be sent to guest");
    let body = match out {
        crate::lan::Out::Packet(b) => b,
        _ => panic!("expected Out::Packet"),
    };
    let mut reader = horizon_protocol::buf::Reader::new(&body);
    let id = reader.varint().unwrap();
    let packet = play::Clientbound::decode(id, &body[reader.position()..]).unwrap();
    match packet {
        play::Clientbound::GameEvent(p) => {
            assert_eq!(p.event, game_event::LIMITED_CRAFTING);
            assert_eq!(p.param, 0.0);
        }
        other => panic!("expected GameEvent, got {other:?}"),
    }
}

/// cargo test --release -p horizon-client --lib lan::hub::tests::requeue_cost -- --ignored --nocapture
#[test]
#[ignore]
fn requeue_cost() {
    for r in [10, 16, 32] {
        for (name, f) in [
            ("scan", requeue_scan as fn(&mut _, &_, _, _)),
            ("set", requeue as fn(&mut _, &_, _, _)),
        ] {
            let sent = FxHashSet::default();
            let mut queue = Vec::new();
            let start = std::time::Instant::now();
            let moves = 200;
            for step in 0..moves {
                // Nothing sent yet (an explorer waiting on generation).
                f(&mut queue, &sent, (step, 0), r);
            }
            let per = start.elapsed().as_secs_f64() * 1e6 / f64::from(moves);
            println!(
                "view {r}: {name} {per:.1} us per chunk crossed, {} queued",
                queue.len()
            );
        }
    }
}

/// A single-player memory connection sends every ready view column before
/// `PlayerLoaded`. After that, on a respawn, and for a socket guest, the
/// batch is `chunks_per_tick` again (64). An unlit column stays out, and
/// the gates that send nothing still do.
#[test]
fn memory_connection_first_view_sends_every_ready_column() {
    let (mut hub, _rx) = hub_with_a_guest();
    hub.ticks = 3;
    {
        let g = &mut hub.guests[0];
        g.chunks_per_tick = 64.0;
        g.batch_quota = 0.0;
        g.address = crate::lan::conn::MEMORY_ADDRESS;
        g.loaded = false;
    }
    // The unlit column first, then 80 lit ones (`floor_at`'s empty column, lit).
    let mut world = World::new(horizon_world::registry::BlockStateId(0), 0);
    let mut dark = crate::chunk::Chunk::empty(100, 100);
    dark.lit = false;
    world.chunks.insert((100, 100), std::sync::Arc::new(dark));
    hub.guests[0].chunk_queue.push((100, 100));
    for i in 0..80 {
        let (x, z) = (i % 16, i / 16);
        let mut chunk = crate::chunk::Chunk::empty(x, z);
        chunk.lit = true;
        world.chunks.insert((x, z), std::sync::Arc::new(chunk));
        hub.guests[0].chunk_queue.push((x, z));
    }

    let batch = hub
        .next_batch(0, &world, None)
        .expect("the first view sends");
    assert_eq!(
        batch.near.len(),
        80,
        "every ready column leaves in one batch"
    );
    assert!(
        batch.near.iter().all(|(_, c)| c.lit),
        "an unlit column is not ready"
    );
    assert!(batch.far.is_empty());
    assert!(hub.guests[0].batch_quota >= 80.0);

    hub.guests[0].loaded = true;
    let loaded = hub
        .next_batch(0, &world, None)
        .expect("a loaded client still receives chunks");
    assert_eq!(loaded.near.len(), 64, "PlayerLoaded puts the cap back");

    hub.guests[0].address = "127.0.0.1:25565".parse().unwrap();
    hub.guests[0].loaded = false;
    hub.guests[0].batch_quota = 0.0;
    let socket = hub
        .next_batch(0, &world, None)
        .expect("a socket guest still receives chunks");
    assert_eq!(socket.near.len(), 64, "a socket guest keeps the quota");

    hub.guests[0].address = crate::lan::conn::MEMORY_ADDRESS;
    hub.guests[0].loaded = true;
    hub.guests[0].loading = true;
    hub.guests[0].batch_quota = 0.0;
    let respawn = hub
        .next_batch(0, &world, None)
        .expect("a respawned player still receives chunks");
    assert_eq!(
        respawn.near.len(),
        64,
        "a respawn stays on the vanilla pace"
    );

    // Seventy lit columns, the unlit one still queued: all seventy, in one batch.
    hub.guests[0].loaded = false;
    hub.guests[0].batch_quota = 0.0;
    hub.guests[0].chunk_queue.truncate(71);
    let seventy = hub
        .next_batch(0, &world, None)
        .expect("seventy ready columns send");
    assert_eq!(seventy.near.len(), 70);
    assert!(seventy.near.iter().all(|(_, c)| c.lit));

    hub.guests[0].unacknowledged = hub.guests[0].max_unacknowledged;
    assert!(
        hub.next_batch(0, &world, None).is_none(),
        "too many batches still waiting sends nothing"
    );

    hub.guests[0].unacknowledged = 0;
    hub.ticks = hub.guests[0].placed_at + 1;
    assert!(
        hub.next_batch(0, &world, None).is_none(),
        "the tick after placement still waits"
    );

    hub.ticks = 3;
    hub.guests[0].chunk_queue.clear();
    hub.guests[0].far_queue.clear();
    assert!(
        hub.next_batch(0, &world, None).is_none(),
        "an empty queue sends nothing"
    );
}
