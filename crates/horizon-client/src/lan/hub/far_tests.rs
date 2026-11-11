use super::*;
use crate::lan::far::{Far, Saved};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

fn far_hub(
    read: impl Fn(i32, i32) -> Option<Vec<u8>> + Send + Sync + 'static,
) -> (Hub, std::sync::mpsc::Receiver<crate::lan::Out>) {
    let (mut hub, rx) = hub_with_a_guest();
    hub.config.view_distance = 8;
    hub.ticks = 3;
    hub.guests[0].center = Some((0, 0));
    hub.guests[0].info.view_distance = 24;
    hub.guests[0].chunks_per_tick = 2.0;
    hub.far = Some(Far::reading(24, Arc::new(move |_, x, z| read(x, z))));
    let save = Arc::new(crate::save::SaveDir::new(
        std::env::temp_dir().join("horizon-far-send-test"),
        crate::save::LevelShape::OVERWORLD,
    ));
    hub.far_level(&save);
    (hub, rx)
}

fn send_chunks(hub: &mut Hub, world: &mut World) {
    let mut next_entity = 2;
    let border = crate::worldborder::WorldBorder::default();
    hub.update_chunks(&TickCtx {
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
    });
}

fn received(rx: &std::sync::mpsc::Receiver<crate::lan::Out>) -> Vec<play::LevelChunkWithLight> {
    rx.try_iter()
        .flat_map(|out| match out {
            crate::lan::Out::Packet(body) => vec![body],
            crate::lan::Out::Packets(bodies) => bodies,
            _ => Vec::new(),
        })
        .filter_map(|body| {
            let mut r = horizon_protocol::buf::Reader::new(&body);
            let id = r.varint().unwrap();
            match C::decode(id, r.rest()).unwrap() {
                C::LevelChunkWithLight(p) => Some(p),
                _ => None,
            }
        })
        .collect()
}

fn lit(world: &mut World, key: (i32, i32)) {
    let mut chunk = Chunk::empty(key.0, key.1);
    chunk.lit = true;
    world.chunks.insert(key, Arc::new(chunk));
}

#[test]
fn far_buffer_edge_sends_all_light_and_live_block_entities_without_neighbours() {
    let (mut hub, rx) = far_hub(|_, _| panic!("a lit resident chunk needs no saved copy"));
    let builtin = horizon_protocol::registry::Builtin::parse(
        r#"{"minecraft:block_entity_type":{"entries":{"minecraft:sign":{"protocol_id":7}}}}"#,
    )
    .unwrap();
    hub.encoder = ChunkEncoder::new(
        ["minecraft:air", "minecraft:oak_sign"].map(|s| (Arc::<str>::from(s), false)),
        &builtin,
        &[],
        &[],
    );
    let mut world = World::new(horizon_world::registry::BlockStateId(0), 0);
    lit(&mut world, (12, 0));
    Arc::make_mut(world.chunks.get_mut(&(12, 0)).unwrap()).set(1, 70, 2, 1);
    world.block_entities.insert(
        (193, 70, 2),
        crate::containers::BlockEntity::Sign(crate::signs::SignData::default()),
    );
    hub.guests[0].far_queue = vec![(12, 0)];
    send_chunks(&mut hub, &mut world);
    let packets = received(&rx);
    assert_eq!(packets.len(), 1);
    assert_eq!((packets[0].x, packets[0].z), (12, 0));
    assert_eq!(packets[0].light.sky.len(), world.layout().sections() + 1);
    assert!(
        packets[0]
            .light
            .sky
            .iter()
            .all(|layer| layer.iter().all(|&b| b == 255))
    );
    assert_eq!(packets[0].chunk.block_entities.len(), 1);
    assert_ne!(packets[0].chunk.block_entities[0].tag, Tag::End);
    assert!(hub.guests[0].far_queue.is_empty());
}

#[test]
fn far_scan_passes_more_than_a_ticks_worth_of_waiting_columns() {
    let (mut hub, rx) = far_hub(|_, _| None);
    let mut world = World::new(horizon_world::registry::BlockStateId(0), 0);
    let mut queue = Vec::new();
    requeue_ring(&mut queue, &Default::default(), (0, 0), 8, 24);
    queue.truncate(crate::lan::far::LOOKS + 1);
    let last = *queue.last().unwrap();
    for &(x, z) in &queue[..queue.len() - 1] {
        world.chunks.insert((x, z), Arc::new(Chunk::empty(x, z)));
    }
    lit(&mut world, last);
    hub.guests[0].far_queue = queue;
    send_chunks(&mut hub, &mut world);
    assert!(
        received(&rx).is_empty(),
        "the scan is still bounded per tick"
    );
    send_chunks(&mut hub, &mut world);
    let packets = received(&rx);
    assert_eq!(packets.len(), 1);
    assert_eq!((packets[0].x, packets[0].z), last);
    assert_eq!(hub.guests[0].far_queue.len(), crate::lan::far::LOOKS);
}

#[test]
fn missing_far_column_retries_after_a_change_without_player_movement() {
    let available = Arc::new(AtomicBool::new(false));
    let ready = available.clone();
    let mut world = World::new(horizon_world::registry::BlockStateId(0), 0);
    let mut chunk = Chunk::empty(12, 0);
    chunk.lit = true;
    let encoder = ChunkEncoder::new(
        [(Arc::<str>::from("minecraft:air"), false)],
        &horizon_protocol::registry::Builtin::default(),
        &[],
        &[],
    );
    let body = encoder.body_alone(&chunk);
    let (mut hub, rx) = far_hub(move |_, _| ready.load(Ordering::Relaxed).then(|| body.clone()));
    // An unlit resident column must not mask a usable saved copy.
    world.chunks.insert((12, 0), Arc::new(Chunk::empty(12, 0)));
    hub.guests[0].far_queue = vec![(12, 0)];
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        send_chunks(&mut hub, &mut world);
        if matches!(hub.far.as_mut().unwrap().saved(12, 0), Saved::Missing) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(hub.guests[0].far_queue, [(12, 0)]);
    assert!(!hub.guests[0].sent_chunks.contains(&(12, 0)));
    available.store(true, Ordering::Relaxed);
    hub.far.as_mut().unwrap().changed(12, 0);
    while !hub.guests[0].sent_chunks.contains(&(12, 0)) {
        send_chunks(&mut hub, &mut world);
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(hub.guests[0].center, Some((0, 0)));
    assert!(hub.guests[0].far_queue.is_empty());
    let packets = received(&rx);
    assert_eq!(packets.len(), 1);
    assert_eq!((packets[0].x, packets[0].z), (12, 0));
    assert!(!packets[0].light.sky.is_empty());
}

#[test]
fn wrapped_far_scan_removes_only_sent_columns_and_keeps_the_near_quota() {
    let (mut hub, rx) = far_hub(|_, _| None);
    let mut world = World::new(horizon_world::registry::BlockStateId(0), 0);
    let keys = [(12, 0), (13, 0), (14, 0), (15, 0)];
    for key in keys.into_iter().chain([(0, 0)]) {
        lit(&mut world, key);
    }
    hub.guests[0].chunks_per_tick = 3.0;
    hub.guests[0].chunk_queue = vec![(0, 0)];
    hub.guests[0].far_queue = keys.into();
    hub.guests[0].far_cursor = 3;
    send_chunks(&mut hub, &mut world);
    assert_eq!(
        received(&rx).iter().map(|p| (p.x, p.z)).collect::<Vec<_>>(),
        [(0, 0), (12, 0), (15, 0)]
    );
    hub.guests[0].unacknowledged = 0;
    send_chunks(&mut hub, &mut world);
    assert_eq!(
        received(&rx).iter().map(|p| (p.x, p.z)).collect::<Vec<_>>(),
        [(13, 0), (14, 0)]
    );
    assert!(hub.guests[0].far_queue.is_empty());
}
