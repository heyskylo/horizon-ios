use super::*;
use horizon_protocol::buf::Reader;

fn trident() -> EntityState {
    EntityState {
        id: 37,
        uuid: 37,
        kind: "minecraft:trident".into(),
        position: [0.5, 65.5, 0.5],
        velocity: [0.25, 0.0, 0.0],
        ..EntityState::default()
    }
}

fn decode(bytes: &[u8]) -> play::Clientbound {
    let mut r = Reader::new(bytes);
    let id = r.varint().unwrap();
    play::Clientbound::decode(id, r.rest()).unwrap()
}

#[test]
fn late_pairing_does_not_duplicate_or_spend_the_next_arrow_update() {
    let mut level = Entities::default();
    let mut tracker = Tracker::default();
    let mut out = Vec::new();
    let chunks = [(0, 0)].into_iter().collect();
    let mut viewers = [Viewer {
        entity_id: 1,
        spectator: false,
        permission: 0,
        moved: false,
        position: [0.5, 64.0, 0.5],
        view_distance: 10,
        chunks: &chunks,
        tracker: &mut tracker,
        out: &mut out,
        gone: None,
    }];
    let riding = Riding::default();
    level.tick(&[], &riding, &mut viewers, |_| Some(0));
    assert_eq!(level.tick, 1);
    let mut e = trident();
    level.pair_after_tick(std::slice::from_ref(&e), &riding, &mut viewers, |_| Some(0));
    assert_eq!(level.tick, 1);
    assert_eq!(level.states.get(&37).unwrap().tick, 0);
    assert!(viewers[0].tracker.is_tracking(37));
    assert_eq!(
        viewers[0]
            .out
            .iter()
            .filter(|b| matches!(decode(b), play::Clientbound::AddEntity(_)))
            .count(),
        1
    );
    assert_eq!(viewers[0].tracker.take_events().len(), 1);
    viewers[0].out.clear();
    level.pair_after_tick(std::slice::from_ref(&e), &riding, &mut viewers, |_| Some(0));
    assert!(viewers[0].out.is_empty());
    assert!(viewers[0].tracker.take_events().is_empty());
    assert_eq!(level.states.get(&37).unwrap().tick, 0);
    // It hits a block between its introduction and the next tracking pass.
    e.position[0] += 0.25;
    e.velocity = [0.0; 3];
    e.meta.push((10, serializer::BOOLEAN, Value::Bool(true)));
    level.tick(&[&e], &riding, &mut viewers, |_| Some(0));
    assert_eq!(level.tick, 2);
    assert_eq!(level.states.get(&37).unwrap().tick, 1);
    assert!(
        !viewers[0]
            .out
            .iter()
            .any(|b| matches!(decode(b), play::Clientbound::AddEntity(_)))
    );
    assert!(
        viewers[0]
            .out
            .iter()
            .any(|b| matches!(decode(b), play::Clientbound::SetEntityData(p)
        if p.id == 37 && p.values.contains(&(10, serializer::BOOLEAN, Value::Bool(true)))))
    );
}

#[test]
fn late_pairing_keeps_chunk_range_and_own_entity_filters() {
    for (x, held, own, scale, expected) in [
        (0.5, true, 1, 100, true),
        (64.5, true, 1, 100, true),
        (64.5001, true, 1, 100, false),
        (40.5, true, 1, 50, false),
        (0.5, false, 1, 100, false),
        (0.5, true, 37, 100, false),
    ] {
        let mut level = Entities {
            range_percent: Some(scale),
            ..Default::default()
        };
        let mut tracker = Tracker::default();
        let mut out = Vec::new();
        let chunks = if held {
            [(0, 0)].into_iter().collect()
        } else {
            Default::default()
        };
        let mut viewers = [Viewer {
            entity_id: own,
            spectator: false,
            permission: 0,
            moved: false,
            position: [x, 64.0, 0.5],
            view_distance: 10,
            chunks: &chunks,
            tracker: &mut tracker,
            out: &mut out,
            gone: None,
        }];
        level.tick(&[], &Riding::default(), &mut viewers, |_| Some(0));
        level.pair_after_tick(&[trident()], &Riding::default(), &mut viewers, |_| Some(0));
        assert_eq!(
            viewers[0].tracker.is_tracking(37),
            expected,
            "x={x} chunk={held} own={own} scale={scale}"
        );
        assert_eq!(
            viewers[0]
                .out
                .iter()
                .any(|b| matches!(decode(b), play::Clientbound::AddEntity(_))),
            expected
        );
    }
}

#[test]
fn a_vehicle_uses_a_later_passengers_range_in_the_same_late_batch() {
    let mut boat = trident();
    boat.kind = "minecraft:oak_boat".into();
    let ghast = EntityState {
        id: 38,
        uuid: 38,
        kind: "minecraft:ghast".into(),
        ..boat.clone()
    };
    for entities in [[boat.clone(), ghast.clone()], [ghast.clone(), boat.clone()]] {
        let mut level = Entities::default();
        let mut tracker = Tracker::default();
        let mut out = Vec::new();
        let chunks = [(0, 0)].into_iter().collect();
        let mut viewers = [Viewer {
            entity_id: 1,
            spectator: false,
            permission: 0,
            moved: false,
            position: [144.5, 64.0, 0.5],
            view_distance: 10,
            chunks: &chunks,
            tracker: &mut tracker,
            out: &mut out,
            gone: None,
        }];
        let mut riding = Riding::default();
        riding.add(37, 38);
        level.tick(&[], &riding, &mut viewers, |_| Some(0));
        level.pair_after_tick(&entities, &riding, &mut viewers, |_| Some(0));
        // Outside the boat's 128 blocks, inside its ghast passenger's 160.
        assert!(viewers[0].tracker.is_tracking(37));
        assert!(viewers[0].tracker.is_tracking(38));
    }
}
