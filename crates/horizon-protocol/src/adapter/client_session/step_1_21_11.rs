//! 26.1's packets in 1.21.11's layout for a 1.21.11 client: the mirror of `translate`'s `mod v1_21_11`
//! (1.21.11 to 26.1). Four things differ: a chunk section loses 26.1's fluid count, the time packet becomes
//! the overworld's day time and whether it runs, the entity data of an AgeableMob loses the boolean 26.1 added
//! at index 17, and the serverbound `interact` of 1.21.11 is split by its action. Every other packet is laid
//! out alike, so it passes as it is.
//!
//! The 1.21.10 step ([`super::step_1_21_10`]) comes after this one for entity data: entity data is undone
//! here first, then the older entity types.
//!
//! Done by the caller (`ClientSession::older_layout_down` / `older_layout_up`), not here:
//! - The `minecraft:world_clock` registry 26.1 sends during configuration, which 1.21.11 lacks, must be
//!   dropped for a 1.21.11 client. It is its own REGISTRY_DATA packet and a packet can only be rewritten here,
//!   never dropped (the signature gives a payload, not a drop).
//! - Serverbound `interact` action 0 (INTERACT, vanilla sends it right after INTERACT_AT) must be dropped, and
//!   action 1 (ATTACK) becomes 26.3's `attack` under its own id. Neither fits a payload-only answer; see
//!   [`serverbound`].
//! - Login and set_player_team are not in translate's `mod v1_21_11`: translate reads them in `mod v26_1`
//!   (26.1 to 26.2), so the 26.1 step covers them. 1.21.11 lays them out as 26.1 does.
use crate::{
    Error, Result, State,
    adapter::translate::v1_21_11::AGEABLE,
    buf::{Codec, LpVec3, Reader, Writer},
    chunk::{self, Paletted, Strategy},
    ids as packet_ids,
    packets::play,
};

/// The pinned clientbound packet `id` (`payload` in 26.1's layout) in 1.21.11's layout; `None` when laid out
/// alike. ADD_ENTITY and REMOVE_ENTITIES are alike: the entity kinds [`entity_data`] needs are the caller's to
/// keep, as translate keeps them in its `Session::kinds`.
pub(super) fn clientbound(state: State, id: i32, payload: &[u8]) -> Result<Option<Vec<u8>>> {
    use packet_ids::play::clientbound as cb;
    Ok(match (state, id) {
        (State::Play, cb::SET_TIME) => Some(set_time(payload)?),
        (State::Play, cb::LEVEL_CHUNK_WITH_LIGHT) => Some(chunk(payload)?),
        _ => None,
    })
}

/// The pinned serverbound packet `id` as the 1.21.11 client sent it (`payload`) in 26.3's layout; `None` when
/// laid out alike. The client's `interact` (one id) carries an action: 2 (INTERACT_AT, the point and hand) is
/// 26.3's `interact`. The other two actions are the caller's and are errors here, not a silent pass-through:
/// action 0 (INTERACT) must be dropped, and action 1 (ATTACK) is 26.3's `attack`, which
/// [`attack`] gives the payload of, under the attack id.
///
/// `translate`'s serverbound gate is `at_most(1.21.11)`, so this also covers 1.21.10 clients.
pub(super) fn serverbound(state: State, id: i32, payload: &[u8]) -> Result<Option<Vec<u8>>> {
    use packet_ids::play::serverbound as sb;
    match (state, id) {
        (State::Play, sb::INTERACT) => interact(payload).map(Some),
        _ => Ok(None),
    }
}

/// 1.21.11 sends the game time, the day time and whether the day time runs; 26.1 a state per world clock. The
/// overworld is clock 0 (translate's `world_clocks`); the day time stands still at 0 when it is missing.
fn set_time(payload: &[u8]) -> Result<Vec<u8>> {
    let t = play::SetTime::read(&mut Reader::new(payload))?;
    let (day_time, ticking) = t
        .clocks
        .iter()
        .find(|c| c.clock == 0)
        .map_or((0, false), |c| (c.total_ticks, c.rate > 0.0));
    let mut w = Writer::new();
    w.i64(t.game_time);
    w.i64(day_time);
    w.bool(ticking);
    Ok(w.data)
}

/// 26.1's column with its sections' fluid counts removed (1.21.11 has none). The rest of the packet is laid out
/// alike.
fn chunk(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let x = r.i32()?;
    let z = r.i32()?;
    let heightmaps = chunk::Heightmaps::read(&mut r)?;
    let buffer = r.byte_array(r.remaining())?;
    let mut sections = Reader::new(&buffer);
    let mut out = Writer::new();
    while sections.remaining() > 0 {
        out.i16(sections.i16()?);
        // The fluid count 26.1 added after the block count.
        sections.i16()?;
        Paletted::read(&mut sections, Strategy::Blocks, 0)?.write(&mut out);
        Paletted::read(&mut sections, Strategy::Biomes, 0)?.write(&mut out);
    }
    let mut w = Writer::new();
    w.i32(x);
    w.i32(z);
    chunk::Heightmaps::write(&heightmaps, &mut w);
    w.byte_array(&out.data);
    w.bytes(r.rest());
    Ok(w.data)
}

/// Entity data of an AgeableMob, 26.1's layout to 1.21.11's (translate's `entity_data`, reversed): the boolean
/// 26.1 added after the baby flag goes (index 17), and the entries past it move back down one. `kind` is the
/// entity's type in the pinned ids, `None` when the caller doesn't know it, which leaves the data as it is.
pub(super) fn entity_data(kind: Option<i32>, data: &mut play::SetEntityData) {
    if !kind.is_some_and(|k| AGEABLE.contains(&k)) {
        return;
    }
    data.values.retain(|(index, _, _)| *index != 17);
    for (index, _, _) in &mut data.values {
        if *index >= 18 {
            *index -= 1;
        }
    }
}

/// 1.21.11's `interact` with action 2 (INTERACT_AT: the point as three floats, the hand) and the sneak flag,
/// as 26.3's `interact` (the entity, the hand, the point as LpVec3, the sneak flag).
fn interact(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let entity = r.varint()?;
    let action = r.varint()?;
    match action {
        2 => {
            let at = [r.f32()?, r.f32()?, r.f32()?].map(f64::from);
            let hand = r.varint()?;
            let secondary = r.bool()?;
            let mut w = Writer::new();
            w.varint(entity);
            w.varint(hand);
            LpVec3::write(&at, &mut w);
            w.bool(secondary);
            Ok(w.data)
        }
        // Vanilla sends INTERACT (action 0) right after INTERACT_AT; 26.3 has no such packet.
        0 => Err(Error::new(
            "1.21.11 interact action 0 is dropped by the caller",
        )),
        // 26.3's attack, under the attack id: the caller uses [`attack`].
        1 => Err(Error::new(
            "1.21.11 interact action 1 is an attack, sent by the caller",
        )),
        other => Err(Error::new(format!(
            "1.21.11 interact action {other} is unknown"
        ))),
    }
}

/// 1.21.11's `interact` with action 1 (ATTACK) as 26.3's `attack`, which names the entity alone. The caller
/// sends the result under the pinned attack id, which [`serverbound`] can't name.
pub(super) fn attack(payload: &[u8]) -> Result<Vec<u8>> {
    let mut w = Writer::new();
    w.varint(Reader::new(payload).varint()?);
    Ok(w.data)
}

#[cfg(test)]
mod tests {
    //! Each packet written here is read back with `translate`'s 1.21.11 reader, which brings it to 26.1's
    //! layout again.
    use super::*;
    use crate::{
        adapter::{ServerVersion, session::Session, translate},
        entity_data::{Value, serializer},
    };

    /// A chunk section of one state and the biomes of one, in 26.1's layout (fluids 0, which translate
    /// writes back, so the round trip is exact).
    fn chunk_26_1() -> Vec<u8> {
        let sections = vec![chunk::Section {
            non_empty: 4096,
            fluids: 0,
            states: Paletted::from_values(&[5543; chunk::BLOCKS], Strategy::Blocks, 16),
            biomes: Paletted::from_values(&[1; chunk::BIOMES], Strategy::Biomes, 0),
        }];
        let column = chunk::ChunkData {
            heightmaps: vec![(1, vec![0x0101; 37])],
            buffer: chunk::ChunkData::from_sections(&sections),
            block_entities: Vec::new(),
        };
        let mut w = Writer::new();
        w.i32(3);
        w.i32(-7);
        chunk::ChunkData::write(&column, &mut w);
        // Four empty masks and two empty light layer lists.
        (0..4).for_each(|_| w.varint(0));
        let none: Vec<Vec<u8>> = Vec::new();
        chunk::Layers::write(&none, &mut w);
        chunk::Layers::write(&none, &mut w);
        w.data
    }

    #[test]
    fn a_26_1_time_reads_as_1_21_11_s_with_the_overworld_clock() {
        let t = play::SetTime {
            game_time: 100,
            clocks: vec![play::ClockState {
                clock: 0,
                total_ticks: 6000,
                partial_tick: 0.0,
                rate: 1.0,
            }],
        };
        let mut w = Writer::new();
        play::SetTime::write(&t, &mut w);
        let body = clientbound(
            State::Play,
            packet_ids::play::clientbound::SET_TIME,
            &w.data,
        )
        .unwrap()
        .unwrap();
        // 1.21.11's layout: game time, day time, whether it runs.
        let mut r = Reader::new(&body);
        assert_eq!(
            (r.i64().unwrap(), r.i64().unwrap(), r.bool().unwrap()),
            (100, 6000, true)
        );
        // And back, as translate reads it.
        let session = Session::new(ServerVersion::V1_21_11);
        let back = translate::v1_21_11::clientbound(
            &session,
            State::Play,
            packet_ids::play::clientbound::SET_TIME,
            &body,
        )
        .unwrap()
        .unwrap();
        let back = play::SetTime::read(&mut Reader::new(&back)).unwrap();
        assert_eq!(back.game_time, 100);
        assert_eq!(
            (
                back.clocks[0].clock,
                back.clocks[0].total_ticks,
                back.clocks[0].rate
            ),
            (0, 6000, 1.0)
        );
    }

    #[test]
    fn without_an_overworld_clock_the_day_time_stands_still() {
        let t = play::SetTime {
            game_time: 5,
            clocks: vec![play::ClockState {
                clock: 1,
                total_ticks: 99,
                partial_tick: 0.0,
                rate: 1.0,
            }],
        };
        let mut w = Writer::new();
        play::SetTime::write(&t, &mut w);
        let body = set_time(&w.data).unwrap();
        let mut r = Reader::new(&body);
        assert_eq!(
            (r.i64().unwrap(), r.i64().unwrap(), r.bool().unwrap()),
            (5, 0, false)
        );
    }

    #[test]
    fn a_26_1_chunk_reads_back_as_26_1_s() {
        let session = Session::new(ServerVersion::V1_21_11);
        let p = chunk_26_1();
        let body = chunk(&p).unwrap();
        let back = translate::v1_21_11::clientbound(
            &session,
            State::Play,
            packet_ids::play::clientbound::LEVEL_CHUNK_WITH_LIGHT,
            &body,
        )
        .unwrap()
        .unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn an_ageable_mob_s_entity_data_loses_26_1_s_boolean() {
        // Kind 4 is an AgeableMob. 26.1's index 17 is the boolean it added; 18 was 17 in 1.21.11.
        let mut d = play::SetEntityData {
            id: 1,
            values: vec![
                (16, serializer::BOOLEAN, Value::Bool(true)),
                (17, serializer::BOOLEAN, Value::Bool(false)),
                (18, serializer::BYTE, Value::Byte(3)),
            ],
        };
        entity_data(Some(4), &mut d);
        assert_eq!(
            d.values,
            vec![
                (16, serializer::BOOLEAN, Value::Bool(true)),
                (17, serializer::BYTE, Value::Byte(3)),
            ]
        );
        // Not an AgeableMob, or not known: nothing moves.
        let mut e = play::SetEntityData {
            id: 1,
            values: vec![(17, serializer::BOOLEAN, Value::Bool(false))],
        };
        entity_data(None, &mut e);
        entity_data(Some(1), &mut e);
        assert_eq!(e.values[0].0, 17);
    }

    #[test]
    fn ageable_entity_data_goes_up_and_back_down() {
        let session = Session::new(ServerVersion::V1_21_11);
        session.kinds.lock().unwrap().insert(1, 4);
        let original = play::SetEntityData {
            id: 1,
            values: vec![
                (16, serializer::BOOLEAN, Value::Bool(true)),
                (17, serializer::BYTE, Value::Byte(3)),
                (20, serializer::INT, Value::Int(7)),
            ],
        };
        let mut d = original.clone();
        translate::v1_21_11::entity_data(&session, &mut d);
        assert_eq!(d.values[1].0, 18);
        entity_data(Some(4), &mut d);
        assert_eq!(d, original);
    }

    #[test]
    fn an_interaction_at_a_point_reads_as_26_3_s_interact() {
        let mut w = Writer::new();
        w.varint(9); // entity
        w.varint(2); // INTERACT_AT
        [0.5f32, 1.0, 0.0].iter().for_each(|v| w.f32(*v));
        w.varint(1); // off hand
        w.bool(true); // sneaking
        let body = serverbound(
            State::Play,
            packet_ids::play::serverbound::INTERACT,
            &w.data,
        )
        .unwrap()
        .unwrap();
        let p = play::Interact::read(&mut Reader::new(&body)).unwrap();
        assert_eq!((p.id, p.hand, p.secondary), (9, 1, true));
        assert!(
            p.location
                .iter()
                .zip([0.5, 1.0, 0.0])
                .all(|(a, b)| (a - b).abs() < 0.01)
        );
    }

    #[test]
    fn an_interaction_at_a_point_comes_back_as_1_21_11_s() {
        let p = play::Interact {
            id: 9,
            hand: 1,
            location: [0.25, -0.5, 0.75],
            secondary: false,
        };
        let mut w = Writer::new();
        play::Interact::write(&p, &mut w);
        // translate writes INTERACT_AT (the body it returns); its INTERACT (action 0) is pushed aside.
        let session = Session::new(ServerVersion::V1_21_11);
        let (_, first) = translate::v1_21_11::interact(&session, &w.data)
            .unwrap()
            .unwrap();
        let body = serverbound(State::Play, packet_ids::play::serverbound::INTERACT, &first)
            .unwrap()
            .unwrap();
        let q = play::Interact::read(&mut Reader::new(&body)).unwrap();
        assert_eq!((q.id, q.hand, q.secondary), (p.id, p.hand, p.secondary));
        assert!(
            q.location
                .iter()
                .zip(p.location)
                .all(|(a, b)| (a - b).abs() < 0.01)
        );
    }

    #[test]
    fn an_interaction_without_a_point_is_not_forwarded_yet() {
        let mut w = Writer::new();
        w.varint(9);
        w.varint(0); // INTERACT
        w.varint(0);
        w.bool(false);
        assert!(
            serverbound(
                State::Play,
                packet_ids::play::serverbound::INTERACT,
                &w.data
            )
            .is_err()
        );
    }

    #[test]
    fn an_attack_names_the_entity_alone() {
        let mut w = Writer::new();
        play::Attack::write(&play::Attack { id: 9 }, &mut w);
        // translate writes the 1.21.11 interact of action 1 for an attack.
        let (_, sent) = translate::v1_21_11::attack(
            &crate::adapter::session::Session::new(crate::adapter::ServerVersion::V1_21_11),
            &w.data,
        )
        .unwrap()
        .unwrap();
        let body = attack(&sent).unwrap();
        assert_eq!(play::Attack::read(&mut Reader::new(&body)).unwrap().id, 9);
    }
}
