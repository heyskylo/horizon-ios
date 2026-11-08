//! 1.21.2/1.21.3 to 1.21.4, from vanilla packet stream codecs.
use super::*;

pub(super) fn clientbound(
    _session: &Session,
    state: State,
    id: i32,
    payload: &[u8],
) -> Result<Option<Vec<u8>>> {
    if state != State::Play {
        return Ok(None);
    }
    use packet_ids::play::clientbound as cb;
    let mut r = Reader::new(payload);
    let mut w = Writer::new();
    match id {
        cb::LEVEL_PARTICLES => {
            w.bool(r.bool()?);
            // The old packet had no alwaysShow flag: the normal particle visibility rule applies.
            w.bool(false);
            w.bytes(r.rest());
        }
        cb::SET_HELD_SLOT => w.varint(i32::from(r.i8()?)),
        cb::PLAYER_INFO_UPDATE => {
            let mut p = play::PlayerInfoUpdate::read(&mut r)?;
            if p.actions & 1 != 0 {
                // Before UPDATE_HAT, tab-list player skins always included their hat layer.
                p.actions |= 128;
                for entry in &mut p.entries {
                    entry.show_hat = Some(true);
                }
            }
            play::PlayerInfoUpdate::write(&p, &mut w);
        }
        _ => return Ok(None),
    }
    Ok(Some(w.data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_particle_flags_survive_the_whole_pipeline_with_registry_ids() {
        let session = Session::new(ServerVersion::V1_21_3);
        let mut old = Writer::new();
        old.bool(true);
        for v in [1.0, 2.0, -3.0] {
            old.f64(v);
        }
        for v in [0.25, 0.5, 0.75, 1.5] {
            old.f32(v);
        }
        old.i32(12);
        // Vanilla 1.21.3 minecraft:block, followed by minecraft:chain[axis=y,waterlogged=false].
        old.varint(1);
        old.varint(7006);
        let (id, body) = super::super::clientbound(
            &session,
            State::Play,
            packet_ids::play::clientbound::LEVEL_PARTICLES,
            &old.data,
        )
        .unwrap()
        .unwrap();
        let play::Clientbound::LevelParticles(p) =
            play::Clientbound::decode(id, &body.unwrap()).unwrap()
        else {
            panic!("particle")
        };
        assert!(p.override_limiter);
        assert!(!p.always_show);
        assert_eq!(p.pos, [1.0, 2.0, -3.0]);
        assert_eq!(p.dist, [0.25, 0.5, 0.75]);
        assert_eq!(p.max_speed, [1.5; 3]);
        assert_eq!(p.count, 12);
        assert_eq!(p.particle.id, 1);
        assert_eq!(
            p.particle.data,
            Some(crate::entity_data::ParticleData::State(9983))
        );
    }

    #[test]
    fn trail_options_leave_the_lifetime_to_the_legacy_provider() {
        let ids = ServerVersion::V1_21_3.ids().unwrap();
        let mut old = Writer::new();
        old.varint(46); // minecraft:trail in vanilla 1.21.3.
        for at in [1.0, 2.0, -3.0] {
            old.f64(at);
        }
        old.i32(0x7f3f1f);
        old.u8(99); // the next field is not a duration
        let mut r = Reader::new(&old.data);
        let particle = {
            let _up = ids::renumbering(ids, Way::Up);
            crate::entity_data::Particle::read(&mut r).unwrap()
        };
        assert_eq!(particle.id, 59);
        assert_eq!(
            particle.data,
            Some(crate::entity_data::ParticleData::Trail(
                [1.0, 2.0, -3.0],
                0x7f3f1f,
                -1
            ))
        );
        assert_eq!(r.u8().unwrap(), 99);
    }

    #[test]
    fn old_heart_states_and_both_creaking_types_retain_their_meaning() {
        let ids = ServerVersion::V1_21_3.ids().unwrap();
        // All three axes and disabled/dormant/active states, from independent vanilla reports.
        for (old, pinned) in
            (2917..2926).zip([5446, 5448, 5450, 5452, 5454, 5456, 5458, 5460, 5462])
        {
            assert_eq!(ids.block_state.up(old), Some(pinned));
            assert_eq!(ids.block_state.down(pinned), Some(old));
            assert_eq!(ids.block_state.down(pinned - 1), Some(old)); // natural=true also maps back
        }
        assert_eq!(ids.entity_type.up(29), Some(31));
        assert_eq!(ids.entity_type.up(30), Some(31));
        assert_eq!(ids.entity_type.down(31), Some(29));
    }

    #[test]
    fn player_list_additions_show_hats_and_partial_updates_keep_their_scope() {
        let session = Session::new(ServerVersion::V1_21_3);
        let mut old = Writer::new();
        old.u8(0x7f);
        old.varint(2);
        for (uuid, name, latency, order) in [(1, "First", 150, 4), (2, "Second", 20, -1)] {
            old.uuid(uuid);
            old.string(name);
            old.varint(0); // properties
            old.bool(false); // chat session
            old.varint(1); // game mode
            old.bool(true);
            old.varint(latency);
            old.bool(false); // display name
            old.varint(order);
        }
        let id = packet_ids::play::clientbound::PLAYER_INFO_UPDATE;
        let translated = super::super::clientbound(&session, State::Play, id, &old.data)
            .unwrap()
            .unwrap()
            .1
            .unwrap();
        let p = play::PlayerInfoUpdate::read(&mut Reader::new(&translated)).unwrap();
        assert_eq!(p.actions, 255);
        assert_eq!(p.entries[0].name.as_deref(), Some("First"));
        assert_eq!(p.entries[0].latency, Some(150));
        assert_eq!(p.entries[1].list_order, Some(-1));
        assert!(p.entries.iter().all(|p| p.show_hat == Some(true)));
        let mut partial = Writer::new();
        partial.u8(16);
        partial.varint(1);
        partial.uuid(1);
        partial.varint(201);
        let translated = super::super::clientbound(&session, State::Play, id, &partial.data)
            .unwrap()
            .unwrap()
            .1
            .unwrap();
        assert_eq!(translated, partial.data);
    }

    #[test]
    fn riding_and_swinging_use_the_legacy_server_layout_and_ids() {
        let session = Session::new(ServerVersion::V1_21_3);
        let request = play::Serverbound::MoveVehicle(play::MoveVehicleRequest {
            position: [1.5, -3.0, 4.5],
            yaw: 90.0,
            pitch: -10.0,
            on_ground: true,
        });
        let (id, payload) = super::super::serverbound(
            &session,
            State::Play,
            packet_ids::play::serverbound::MOVE_VEHICLE,
            &strip_id(request.encode()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(id, 32);
        assert_eq!(payload.len(), 32);
        let mut r = Reader::new(&payload);
        assert_eq!(
            [r.f64().unwrap(), r.f64().unwrap(), r.f64().unwrap()],
            [1.5, -3.0, 4.5]
        );
        assert_eq!([r.f32().unwrap(), r.f32().unwrap()], [90.0, -10.0]);
        assert_eq!(r.remaining(), 0);
        let swing = super::super::serverbound(
            &session,
            State::Play,
            packet_ids::play::serverbound::PUNCH,
            &[],
        )
        .unwrap()
        .unwrap();
        assert_eq!(swing, (56, vec![0]));
    }
}
