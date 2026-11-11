use super::*;
use horizon_client::{
    assets::Assets,
    lan::{Joined, PlayerSender, guest::SavedBody},
};
use horizon_core::random::RandomSource;
use horizon_protocol::packets::configuration::ClientInformation;
use std::sync::{Arc, atomic::AtomicI32, mpsc};

fn guest(entity_id: i32) -> Guest {
    let (sender, _) = mpsc::channel();
    Guest::new(
        Joined {
            conn: entity_id as u32,
            uuid: entity_id as u128,
            name: "GuestBodyTest".into(),
            properties: Vec::new(),
            info: ClientInformation::new(2),
            brand: "test".into(),
            sender: PlayerSender::new(sender, Arc::default()),
            latency: Arc::new(AtomicI32::new(0)),
            address: "127.0.0.1:25565".parse().unwrap(),
            expected: false,
            configured: Default::default(),
        },
        entity_id,
        [0.5, 64.0, 0.5],
        2,
    )
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn co_located_guests_own_distinct_streams_and_restore_the_saved_body() {
    let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    let assets = Assets::load(&root).unwrap();
    let resources = MotionResources::load(&root, &assets).unwrap();
    let mut owner = PlayerMotion::new(&resources, Vec3::new(0.5, 64.0, 0.5)).unwrap();
    let mut first = guest(2);
    let mut second = guest(3);
    let saved = SavedBody {
        fire_ticks: 43,
        air: 173,
        fall_distance: 4.5,
        velocity: [0.25, -0.5, 0.75],
        food_exhaustion: 1.25,
        food_tick_timer: 17,
        ticks_frozen: 23,
    };
    first.loaded_body = Some(saved);
    initialize(&resources, &mut first);
    initialize(&resources, &mut second);
    assert!(first.loaded_body.is_none());
    assert_eq!(SavedBody::of(first.motion.as_ref().unwrap()), saved);
    let mut first_random = first.motion.as_ref().unwrap().random.clone();
    let mut second_random = second.motion.as_ref().unwrap().random.clone();
    let first_trace: Vec<_> = (0..4).map(|_| first_random.next_int()).collect();
    let second_trace: Vec<_> = (0..4).map(|_| second_random.next_int()).collect();
    let owner_trace: Vec<_> = (0..4).map(|_| owner.random.next_int()).collect();
    assert_ne!(first_trace, second_trace);
    assert_ne!(first_trace, owner_trace);
    assert_ne!(second_trace, owner_trace);
    // Advancing one guest does not borrow or advance the other body.
    first.motion.as_mut().unwrap().random.next_int();
    let unchanged: Vec<_> = (0..4)
        .map(|_| second.motion.as_mut().unwrap().random.next_int())
        .collect();
    assert_eq!(unchanged, second_trace);
}
