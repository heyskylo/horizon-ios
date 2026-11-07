//! Before 1.21.2 one boat type carried its wood in metadata. The pinned protocol has separate types.
use super::*;
use crate::entity_data::Value;

// Vanilla Boat.Type ordinal order, matched to the pinned entity registry.
pub(crate) const TYPES: [i32; 9] = [90, 128, 12, 75, 0, 23, 34, 82, 9];
pub(crate) const CHEST_TYPES: [i32; 9] = [91, 129, 13, 76, 1, 24, 35, 83, 8];

pub(crate) struct Boat {
    spawn: play::AddEntity,
    chest: bool,
    replace: bool,
    metadata: Vec<(u8, i32, Value)>,
    passengers: Option<Vec<i32>>,
    leash: Option<i32>,
}

pub(crate) fn spawn(session: &Session, old_kind: Option<i32>, entity: &play::AddEntity) {
    let kind = old_kind.and_then(|kind| session.version.ids()?.entity_type.up(kind));
    if !matches!(kind, Some(90 | 91)) {
        session.legacy_boats.lock().unwrap().remove(&entity.id);
        return;
    }
    session.legacy_boats.lock().unwrap().insert(
        entity.id,
        Boat {
            spawn: entity.clone(),
            chest: kind == Some(91),
            replace: false,
            metadata: Vec::new(),
            passengers: None,
            leash: None,
        },
    );
}

pub(crate) fn metadata(session: &Session, data: &mut play::SetEntityData) {
    let kind = {
        let mut boats = session.legacy_boats.lock().unwrap();
        let Some(boat) = boats.get_mut(&data.id) else {
            return;
        };
        data.values.retain_mut(|(index, _, value)| {
            if *index == 11 {
                if let Value::Int(variant) = value {
                    let types = if boat.chest { CHEST_TYPES } else { TYPES };
                    let kind = usize::try_from(*variant)
                        .ok()
                        .and_then(|v| types.get(v))
                        .copied()
                        .unwrap_or(types[0]);
                    boat.replace |= boat.spawn.kind != kind;
                    boat.spawn.kind = kind;
                }
                return false;
            }
            if (12..=14).contains(index) {
                *index -= 1;
            }
            true
        });
        boat.spawn.kind
    };
    session.kinds.lock().unwrap().insert(data.id, kind);
}

fn body(packet: play::Clientbound) -> (i32, Vec<u8>) {
    let encoded = packet.encode();
    let mut r = Reader::new(&encoded);
    (r.varint().unwrap(), r.rest().to_vec())
}
fn angle(degrees: f32) -> i8 {
    (degrees * (256.0 / 360.0)).floor() as i32 as i8
}
fn movement(boat: &mut Boat, p: &play::MoveEntity) {
    if let Some(delta) = &p.delta {
        boat.spawn.position = delta.end(boat.spawn.position);
    }
    if let Some((yaw, pitch)) = p.rotation {
        boat.spawn.yaw = yaw;
        boat.spawn.pitch = pitch;
    }
}

/// Vehicle requests and corrections name no entity; the local player controls the first seat.
pub(crate) fn controlled(session: &Session, position: [f64; 3], yaw: f32, pitch: f32) {
    let local = session.local_player.lock().unwrap().map(|p| p.0);
    let Some(local) = local else {
        return;
    };
    for boat in session.legacy_boats.lock().unwrap().values_mut() {
        if boat.passengers.as_ref().and_then(|p| p.first()) == Some(&local) {
            boat.spawn.position = position;
            boat.spawn.yaw = angle(yaw);
            boat.spawn.pitch = angle(pitch);
        }
    }
}

/// Track canonical movement so a wood change never respawns the boat at its old position.
/// Recreate only when its type actually changes, retaining its metadata, passengers and lead.
pub(crate) fn finish(session: &Session, id: i32, payload: Vec<u8>) -> Result<(i32, Vec<u8>)> {
    use packet_ids::play::clientbound as cb;
    if session.legacy_boats.lock().unwrap().is_empty()
        || !matches!(
            id,
            cb::SET_ENTITY_DATA
                | cb::MOVE_ENTITY_POS
                | cb::MOVE_ENTITY_POS_ROT
                | cb::MOVE_ENTITY_ROT
                | cb::ENTITY_POSITION_SYNC
                | cb::TELEPORT_ENTITY
                | cb::SET_ENTITY_MOTION
                | cb::SET_PASSENGERS
                | cb::SET_ENTITY_LINK
                | cb::ROTATE_HEAD
                | cb::MOVE_VEHICLE
        )
    {
        return Ok((id, payload));
    }
    let packet = play::Clientbound::decode(id, &payload)?;
    if let play::Clientbound::MoveVehicle(p) = &packet {
        controlled(session, p.position, p.yaw, p.pitch);
        return Ok((id, payload));
    }
    let mut queued = Vec::new();
    let mut replaced = None;
    {
        let mut boats = session.legacy_boats.lock().unwrap();
        match packet {
            play::Clientbound::SetEntityData(p) => {
                if let Some(boat) = boats.get_mut(&p.id) {
                    for value in p.values {
                        if let Some(old) = boat.metadata.iter_mut().find(|old| old.0 == value.0) {
                            *old = value;
                        } else {
                            boat.metadata.push(value);
                        }
                    }
                    if boat.replace {
                        boat.replace = false;
                        replaced = Some(p.id);
                        queued.push(body(play::Clientbound::AddEntity(boat.spawn.clone())));
                        queued.push(body(play::Clientbound::SetEntityData(
                            play::SetEntityData {
                                id: p.id,
                                values: boat.metadata.clone(),
                            },
                        )));
                        if let Some(passengers) = &boat.passengers {
                            queued.push(body(play::Clientbound::SetPassengers(
                                play::SetPassengers {
                                    vehicle: p.id,
                                    passengers: passengers.clone(),
                                },
                            )));
                        }
                        if let Some(dest) = boat.leash {
                            queued.push(body(play::Clientbound::SetEntityLink(
                                play::SetEntityLink { source: p.id, dest },
                            )));
                        }
                    }
                }
            }
            play::Clientbound::MoveEntityPos(p) => {
                if let Some(boat) = boats.get_mut(&p.0.id) {
                    movement(boat, &p.0);
                }
            }
            play::Clientbound::MoveEntityPosRot(p) => {
                if let Some(boat) = boats.get_mut(&p.0.id) {
                    movement(boat, &p.0);
                }
            }
            play::Clientbound::MoveEntityRot(p) => {
                if let Some(boat) = boats.get_mut(&p.0.id) {
                    movement(boat, &p.0);
                }
            }
            play::Clientbound::EntityPositionSync(p) => {
                if let Some(boat) = boats.get_mut(&p.id) {
                    boat.spawn.position = p.path.end();
                    boat.spawn.yaw = angle(p.yaw);
                    boat.spawn.pitch = angle(p.pitch);
                }
            }
            play::Clientbound::TeleportEntity(p) => {
                if let Some(boat) = boats.get_mut(&p.id) {
                    for i in 0..3 {
                        boat.spawn.position[i] = p.position[i]
                            + if p.relatives & (1 << i) != 0 {
                                boat.spawn.position[i]
                            } else {
                                0.0
                            };
                        boat.spawn.velocity[i] = p.delta[i]
                            + if p.relatives & (1 << (i + 5)) != 0 {
                                boat.spawn.velocity[i]
                            } else {
                                0.0
                            };
                    }
                    let yaw = p.yaw
                        + if p.relatives & 8 != 0 {
                            f32::from(boat.spawn.yaw) * (360.0 / 256.0)
                        } else {
                            0.0
                        };
                    let pitch = p.pitch
                        + if p.relatives & 16 != 0 {
                            f32::from(boat.spawn.pitch) * (360.0 / 256.0)
                        } else {
                            0.0
                        };
                    boat.spawn.yaw = angle(yaw);
                    boat.spawn.pitch = angle(pitch);
                }
            }
            play::Clientbound::SetEntityMotion(p) => {
                if let Some(boat) = boats.get_mut(&p.id) {
                    boat.spawn.velocity = p.velocity;
                }
            }
            play::Clientbound::RotateHead(p) => {
                if let Some(boat) = boats.get_mut(&p.id) {
                    boat.spawn.head_yaw = p.head_yaw;
                }
            }
            play::Clientbound::SetPassengers(p) => {
                if let Some(boat) = boats.get_mut(&p.vehicle) {
                    boat.passengers = Some(p.passengers);
                }
            }
            play::Clientbound::SetEntityLink(p) => {
                if let Some(boat) = boats.get_mut(&p.source) {
                    boat.leash = Some(p.dest);
                }
            }
            _ => {}
        }
    }
    if let Some(entity) = replaced {
        session.incoming.lock().unwrap().extend(queued);
        return Ok(body(play::Clientbound::RemoveEntities(
            play::RemoveEntities { ids: vec![entity] },
        )));
    }
    Ok((id, payload))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity_data::serializer;

    #[test]
    fn controlled_requests_and_corrections_update_the_cached_boat_and_reused_ids_are_cleared() {
        let session = Session::new(ServerVersion::V1_21_1);
        *session.local_player.lock().unwrap() = Some((7, false));
        let mut entity = play::AddEntity {
            id: 42,
            uuid: 123,
            kind: 90,
            position: [0.0, 64.0, 0.0],
            velocity: [0.0; 3],
            pitch: 0,
            yaw: 0,
            head_yaw: 0,
            data: 0,
        };
        spawn(&session, Some(10), &entity);
        let (id, payload) = body(play::Clientbound::SetPassengers(play::SetPassengers {
            vehicle: 42,
            passengers: vec![7],
        }));
        finish(&session, id, payload).unwrap();
        controlled(&session, [10.0, 64.0, 2.0], 90.0, -45.0);
        assert_eq!(
            session.legacy_boats.lock().unwrap()[&42].spawn.position,
            [10.0, 64.0, 2.0]
        );
        let (id, payload) = body(play::Clientbound::MoveVehicle(play::MoveVehicle {
            position: [20.0, 65.0, 3.0],
            yaw: 180.0,
            pitch: 0.0,
        }));
        finish(&session, id, payload).unwrap();
        assert_eq!(
            session.legacy_boats.lock().unwrap()[&42].spawn.position,
            [20.0, 65.0, 3.0]
        );
        entity.kind = 30;
        spawn(&session, Some(24), &entity);
        assert!(session.legacy_boats.lock().unwrap().is_empty());
    }

    #[test]
    fn a_variant_change_keeps_motion_metadata_passengers_and_the_lead() {
        let session = Session::new(ServerVersion::V1_21_1);
        let entity = play::AddEntity {
            id: 42,
            uuid: 123,
            kind: 90,
            position: [0.0, 64.0, 0.0],
            velocity: [0.0; 3],
            pitch: 0,
            yaw: 0,
            head_yaw: 0,
            data: 0,
        };
        spawn(&session, Some(10), &entity);
        for packet in [
            play::Clientbound::MoveEntityPosRot(play::MoveEntityPosRot(play::MoveEntity {
                id: 42,
                delta: Some(play::VecDelta::Linear([4096, 0, -4096])),
                rotation: Some((64, -32)),
                on_ground: true,
            })),
            play::Clientbound::SetEntityMotion(play::SetEntityMotion {
                id: 42,
                velocity: [0.125; 3],
            }),
            play::Clientbound::SetPassengers(play::SetPassengers {
                vehicle: 42,
                passengers: vec![7],
            }),
            play::Clientbound::SetEntityLink(play::SetEntityLink {
                source: 42,
                dest: 8,
            }),
        ] {
            let (id, payload) = body(packet);
            finish(&session, id, payload).unwrap();
        }
        let mut data = play::SetEntityData {
            id: 42,
            values: vec![
                (11, serializer::INT, Value::Int(8)),
                (12, serializer::BOOLEAN, Value::Bool(true)),
                (14, serializer::INT, Value::Int(5)),
            ],
        };
        metadata(&session, &mut data);
        let (id, payload) = body(play::Clientbound::SetEntityData(data));
        let (id, payload) = finish(&session, id, payload).unwrap();
        let play::Clientbound::RemoveEntities(p) = play::Clientbound::decode(id, &payload).unwrap()
        else {
            panic!("remove")
        };
        assert_eq!(p.ids, [42]);
        let (id, payload) = session.next_incoming().unwrap();
        let play::Clientbound::AddEntity(p) = play::Clientbound::decode(id, &payload).unwrap()
        else {
            panic!("spawn")
        };
        assert_eq!(
            (p.id, p.uuid, p.kind, p.position, p.yaw, p.pitch),
            (42, 123, 9, [1.0, 64.0, -1.0], 64, -32)
        );
        // LpVec3 quantizes the velocity each time it crosses the pinned wire format.
        assert!(p.velocity.iter().all(|v| (*v - 0.125).abs() < 0.0001));
        let (id, payload) = session.next_incoming().unwrap();
        let play::Clientbound::SetEntityData(p) = play::Clientbound::decode(id, &payload).unwrap()
        else {
            panic!("data")
        };
        assert_eq!(
            p.values,
            [
                (11, serializer::BOOLEAN, Value::Bool(true)),
                (13, serializer::INT, Value::Int(5))
            ]
        );
        let (id, payload) = session.next_incoming().unwrap();
        assert!(
            matches!(play::Clientbound::decode(id,&payload).unwrap(),play::Clientbound::SetPassengers(p) if p.passengers==[7])
        );
        let (id, payload) = session.next_incoming().unwrap();
        assert!(
            matches!(play::Clientbound::decode(id,&payload).unwrap(),play::Clientbound::SetEntityLink(p) if p.dest==8)
        );
    }
}
