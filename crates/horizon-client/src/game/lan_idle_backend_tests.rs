use super::*;
use horizon_client::lan::{
    Joined, Lan, NetConfig, Out, PlayerSender,
    chunks::ChunkEncoder,
    data::SyncData,
    entity::{EntityState, States},
    guest::Guest,
    hub::{HubConfig, TickCtx},
};
use horizon_protocol::{
    buf::Reader,
    entity_data::{Value, serializer},
    item::Stack,
    packets::{configuration::ClientInformation, play::Clientbound as C},
    registry::{Builtin, Ids},
};
use std::{
    collections::HashSet,
    sync::{Arc, mpsc},
};

fn fixture() -> (Hub, Vec<mpsc::Receiver<Out>>) {
    let mut sync = SyncData::empty();
    sync.builtin.registries.insert(
        "minecraft:entity_type".into(),
        Ids::from_names(vec![
            "minecraft:player".into(),
            "minecraft:zombie".into(),
            "minecraft:item".into(),
        ]),
    );
    let sync = Arc::new(sync);
    let lan = Lan::start_with(
        sync.clone(),
        NetConfig {
            announce: false,
            ..NetConfig::loopback(0, "IdleBackendFixture".into(), 2)
        },
    )
    .unwrap();
    let encoder = ChunkEncoder::new([] as [(Arc<str>, bool); 0], &Builtin::default(), &[], &[]);
    let config = HubConfig {
        default_game_mode: 0,
        guest_commands: false,
        force_game_mode: false,
        hardcore: false,
        view_distance: 8,
        simulation_distance: 2,
        dimension: "minecraft:overworld".into(),
        allow_flight: false,
        seed: 0,
        max_players: 20,
        integrated: false,
        levels: Default::default(),
    };
    let mut hub = Hub::new(lan, sync, encoder, config);
    let mut receivers = Vec::new();
    for id in [1, 2] {
        let (sender, receiver) = mpsc::channel();
        receivers.push(receiver);
        let at = if id == 1 {
            [0.5, 64.0, 0.5]
        } else {
            [500.5, 64.0, 0.5]
        };
        let mut guest = Guest::new(
            Joined {
                conn: id,
                uuid: u128::from(id),
                name: format!("IdleViewer{id}"),
                properties: Vec::new(),
                info: ClientInformation::new(8),
                brand: "vanilla".into(),
                sender: PlayerSender::new(sender, Arc::default()),
                latency: Arc::new(std::sync::atomic::AtomicI32::new(0)),
                address: "127.0.0.1:25565".parse().unwrap(),
                expected: false,
                configured: Default::default(),
            },
            id as i32,
            at,
            8,
        );
        guest.placed = id == 1;
        guest.held = id == 2;
        guest.dimension = "minecraft:overworld".into();
        if id == 1 {
            guest.sent_chunks.insert((5, 0));
        }
        hub.guests.push(guest);
    }
    (hub, receivers)
}

fn states() -> Vec<EntityState> {
    let mut states = States::default();
    let mob = states.add(101, 101, "minecraft:zombie");
    mob.position = [80.5, 64.0, 0.5];
    mob.on_ground = true;
    mob.idle = true;
    mob.meta.push((9, serializer::FLOAT, Value::Float(7.0)));
    mob.equipment.push((0, Some(Stack::new(1, 1))));
    let item = states.add(102, 102, "minecraft:item");
    item.position = [80.5, 64.0, 0.5];
    item.on_ground = true;
    item.idle = true;
    item.meta.push((
        8,
        serializer::ITEM_STACK,
        Value::Item(Some(Stack::new(1, 3))),
    ));
    states.as_slice().to_vec()
}

fn tick(
    hub: &mut Hub,
    cache: &mut IdleStates,
    world: &mut horizon_client::world::World,
) -> Vec<bool> {
    let mut states = states();
    if let Some(mut pass) = cache.pass(hub, 100) {
        for state in &mut states {
            let place = Place {
                position: state.position,
                rotation: (0.0, 0.0, 0.0),
                on_ground: true,
                velocity: [0.0; 3],
            };
            state.bare = pass.bare(hub, state.id, &state.kind, &place);
            if state.bare {
                state.meta.clear();
                state.equipment.clear();
            }
        }
        pass.finish();
    }
    let bare = states.iter().map(|state| state.bare).collect();
    let mut next = 200;
    let border = horizon_client::worldborder::WorldBorder::default();
    let mut ctx = TickCtx {
        sea_level: 63,
        world,
        entities: &states,
        next_entity: &mut next,
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
    hub.tick_level(&mut ctx);
    hub.end_tick();
    bare
}

fn packets(receiver: &mpsc::Receiver<Out>) -> Vec<C> {
    receiver
        .try_iter()
        .filter_map(|out| {
            let Out::Packet(bytes) = out else { return None };
            let mut reader = Reader::new(&bytes);
            let id = reader.varint().unwrap();
            C::decode(id, reader.rest()).ok()
        })
        .collect()
}

#[test]
fn late_held_viewer_gets_full_idle_entity_metadata_equipment_and_item_stack() {
    let (mut hub, receivers) = fixture();
    let mut cache = IdleStates::default();
    let mut world = horizon_client::world::World::new(horizon_world::registry::BlockStateId(0), 0);
    assert_eq!(tick(&mut hub, &mut cache, &mut world), [false, false]);
    assert!(hub.guests[0].tracker.is_tracking(101) && hub.guests[0].tracker.is_tracking(102));
    tick(&mut hub, &mut cache, &mut world);
    assert_eq!(tick(&mut hub, &mut cache, &mut world), [true, true]);
    for receiver in &receivers {
        packets(receiver);
    }

    // Held arrivals watch the world even before placement. Being near but
    // lacking the column must force full data, without pairing too early.
    hub.guests[1].position = [0.5, 64.0, 0.5];
    hub.guests[1].moved = true;
    assert_eq!(tick(&mut hub, &mut cache, &mut world), [false, false]);
    assert!(!hub.guests[1].tracker.is_tracking(101));
    assert!(!hub.guests[1].tracker.is_tracking(102));
    packets(&receivers[1]);
    hub.guests[1].sent_chunks.insert((5, 0));
    hub.guests[1].moved = true;
    assert_eq!(tick(&mut hub, &mut cache, &mut world), [false, false]);
    let sent = packets(&receivers[1]);
    let added: HashSet<_> = sent
        .iter()
        .filter_map(|packet| match packet {
            C::AddEntity(p) => Some(p.id),
            _ => None,
        })
        .collect();
    assert!(added.contains(&101) && added.contains(&102));
    assert!(sent.iter().any(|packet| matches!(packet, C::SetEntityData(p) if p.id == 101 && p.values.iter().any(|(i, _, value)| *i == 9 && matches!(value, Value::Float(v) if *v == 7.0)))));
    assert!(sent.iter().any(|packet| matches!(packet, C::SetEquipment(p) if p.id == 101 && p.slots.iter().any(|(slot, stack)| *slot == 0 && stack.as_ref().is_some_and(|s| s.item == 1 && s.count == 1)))));
    assert!(sent.iter().any(|packet| matches!(packet, C::SetEntityData(p) if p.id == 102 && p.values.iter().any(|(i, _, value)| *i == 8 && matches!(value, Value::Item(Some(s)) if s.item == 1 && s.count == 3)))));
    assert_eq!(tick(&mut hub, &mut cache, &mut world), [true, true]);
}
