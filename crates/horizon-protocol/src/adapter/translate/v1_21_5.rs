//! 1.21.5's player commands and hanging-entity metadata, from vanilla's codecs/registrations.
use super::*;

pub(super) fn clientbound(
    _session: &Session,
    state: State,
    id: i32,
    payload: &[u8],
) -> Result<Option<Vec<u8>>> {
    if (state, id)
        != (
            State::Play,
            packet_ids::play::clientbound::LEVEL_CHUNK_WITH_LIGHT,
        )
    {
        return Ok(None);
    }
    let mut r = Reader::new(payload);
    let mut w = Writer::new();
    w.i32(r.i32()?);
    w.i32(r.i32()?);
    crate::chunk::Heightmaps::write(&crate::chunk::Heightmaps::read(&mut r)?, &mut w);
    let buffer = r.byte_array(r.remaining())?;
    let mut sections = Reader::new(&buffer);
    let mut out = Writer::new();
    // 1.21.5's Data.getSerializedSize still counts two VarInt long-array lengths per section,
    // but Data.write uses writeFixedSizeLongArray. ClientboundLevelChunkPacketData allocates that
    // larger size and transmits the unused zero-filled tail. 1.21.6 fixed the size calculation.
    let mut padding = 0;
    while sections.remaining() > padding {
        out.i16(sections.i16()?);
        for strategy in [Strategy::Blocks, Strategy::Biomes] {
            let palette = Paletted::read(&mut sections, strategy, 0)?;
            padding +=
                ((usize::BITS - palette.data.len().max(1).leading_zeros()).div_ceil(7)) as usize;
            palette.write(&mut out);
        }
    }
    if sections.remaining() != padding || sections.rest().iter().any(|byte| *byte != 0) {
        return Err(crate::Error::new("invalid 1.21.5 chunk padding"));
    }
    w.byte_array(&out.data);
    w.bytes(r.rest());
    Ok(Some(w.data))
}

/// 1.21.6 removed PRESS_SHIFT_KEY and RELEASE_SHIFT_KEY, shifting every other action by two.
pub(super) fn player_command(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let mut w = Writer::new();
    w.varint(r.varint()?);
    w.varint(r.varint()? + 2);
    w.varint(r.varint()?);
    Ok(w.data)
}

/// Input still has the shift bit, but the 1.21.5 server changes the entity flag only on a command.
pub(super) fn input(session: &Session, payload: &[u8]) -> Result<()> {
    let shift = Reader::new(payload).u8()? & 32 != 0;
    let mut local = session.local_player.lock().unwrap();
    if let Some((entity, was_shift)) = local.as_mut()
        && *was_shift != shift
    {
        let mut w = Writer::new();
        w.varint(
            session
                .version
                .serverbound_id(State::Play, packet_ids::play::serverbound::PLAYER_COMMAND)
                .unwrap(),
        );
        w.varint(*entity);
        w.varint(if shift { 0 } else { 1 });
        w.varint(0);
        session.outgoing.lock().unwrap().push(w.data);
        *was_shift = shift;
    }
    Ok(())
}

/// HangingEntity added DATA_DIRECTION at index 8; the subclasses' existing fields moved up one.
pub(super) fn entity_data(session: &Session, data: &mut play::SetEntityData) {
    // Pinned registry: glow_item_frame, item_frame, painting.
    if matches!(
        session.kinds.lock().unwrap().get(&data.id),
        Some(61 | 74 | 94)
    ) {
        for (index, _, _) in &mut data.values {
            if *index >= 8 {
                *index += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity_data::{Value, serializer};

    #[test]
    fn commands_keep_their_actions_and_shift_only_sends_transitions() {
        let session = Session::new(ServerVersion::V1_21_5);
        *session.local_player.lock().unwrap() = Some((42, false));
        for action in 0..=6 {
            let (id, body) = super::super::serverbound(
                &session,
                State::Play,
                packet_ids::play::serverbound::PLAYER_COMMAND,
                &[42, action, 75],
            )
            .unwrap()
            .unwrap();
            assert_eq!((id, body), (40, vec![42, action + 2, 75])); // vanilla 1.21.5 packet table
        }
        for (input, command) in [
            (0, None),
            (32, Some(0)),
            (33, None),
            (0, Some(1)),
            (1, None),
        ] {
            let (_, body) = super::super::serverbound(
                &session,
                State::Play,
                packet_ids::play::serverbound::PLAYER_INPUT,
                &[input],
            )
            .unwrap()
            .unwrap();
            assert_eq!(body, [input]);
            let extra = session.take_outgoing();
            assert_eq!(
                extra,
                command
                    .map(|action| vec![40, 42, action, 0])
                    .into_iter()
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn swings_attacks_and_both_interactions_use_the_1_21_5_packet_ids() {
        let session = Session::new(ServerVersion::V1_21_5);
        use packet_ids::play::serverbound as sb;
        assert_eq!(
            super::super::serverbound(&session, State::Play, sb::PUNCH, &[]).unwrap(),
            Some((59, vec![0]))
        );
        assert_eq!(
            super::super::serverbound(&session, State::Play, sb::ATTACK, &[42]).unwrap(),
            Some((24, vec![42, 1, 0]))
        );
        let mut w = Writer::new();
        w.varint(42);
        w.varint(0);
        crate::buf::LpVec3::write(&[0.0, 0.0, 0.0], &mut w);
        w.bool(false);
        let (id, body) = super::super::serverbound(&session, State::Play, sb::INTERACT, &w.data)
            .unwrap()
            .unwrap();
        assert_eq!(id, 24);
        assert_eq!(body[..2], [42, 2]);
        assert_eq!(session.take_outgoing(), [vec![24, 42, 0, 0, 0]]);
    }

    #[test]
    fn hanging_fields_move_but_other_entity_fields_stay() {
        let session = Session::new(ServerVersion::V1_21_5);
        for kind in [61, 74, 94, 159] {
            session.kinds.lock().unwrap().insert(42, kind);
            let mut data = play::SetEntityData {
                id: 42,
                values: vec![
                    (0, serializer::BYTE, Value::Byte(2)),
                    (8, serializer::INT, Value::Int(3)),
                    (9, serializer::INT, Value::Int(4)),
                ],
            };
            entity_data(&session, &mut data);
            let indices: Vec<_> = data.values.iter().map(|v| v.0).collect();
            assert_eq!(
                indices,
                if kind == 159 {
                    vec![0, 8, 9]
                } else {
                    vec![0, 9, 10]
                }
            );
        }
    }

    #[test]
    fn vanilla_chunk_padding_is_removed_without_losing_sections() {
        let session = Session::new(ServerVersion::V1_21_5);
        let mut sections = Writer::new();
        // Singleton stone/biome 7. Vanilla writes no lengths, but over-allocates two bytes here.
        sections.bytes(&[0x10, 0, 0, 1, 0, 7]);
        sections.i16(2048);
        sections.bytes(&[4, 2, 0, 1]); // local block palette
        for _ in 0..256 {
            sections.i64(0x1010_1010_1010_1010);
        }
        sections.u8(6); // global biome palette, seven longs
        for _ in 0..7 {
            sections.i64(0);
        }
        sections.bytes(&[0; 5]); // VarInt sizes: singleton 1+1, local/global 2+1.
        let mut w = Writer::new();
        w.i32(2);
        w.i32(-3);
        w.varint(0); // x/z, heightmaps
        w.byte_array(&sections.data);
        for _ in 0..7 {
            w.varint(0);
        } // block entities, four masks, two light arrays
        let (id, body) = super::super::clientbound(
            &session,
            State::Play,
            packet_ids::play::clientbound::LEVEL_CHUNK_WITH_LIGHT,
            &w.data,
        )
        .unwrap()
        .unwrap();
        let play::Clientbound::LevelChunkWithLight(p) =
            play::Clientbound::decode(id, &body.unwrap()).unwrap()
        else {
            panic!("chunk")
        };
        assert_eq!((p.x, p.z), (2, -3));
        let sections = p.chunk.sections(2, 16, 6).unwrap();
        assert_eq!(
            (
                sections[0].states.get(0),
                sections[0].biomes.get(0),
                sections[0].fluids
            ),
            (1, 7, 0)
        );
        assert_eq!(sections[1].states.palette, [0, 1]);
        assert_eq!(
            (
                sections[1].states.get(0),
                sections[1].states.get(1),
                sections[1].biomes.get(63)
            ),
            (0, 1, 0)
        );
    }

    #[test]
    fn command_parser_ids_are_mapped_before_reading_their_properties() {
        // Old component (17) has no properties; time (42) has an i32; resource (45) a registry name.
        let mut w = Writer::new();
        w.varint(3);
        for (name, parser) in [("text", 17), ("duration", 42), ("item", 45)] {
            w.bytes(&[2, 0]);
            w.string(name);
            w.varint(parser);
            if parser == 42 {
                w.i32(20);
            }
            if parser == 45 {
                w.string("minecraft:item");
            }
        }
        w.varint(0);
        let session = Session::new(ServerVersion::V1_21_5);
        let (id, body) = super::super::clientbound(
            &session,
            State::Play,
            packet_ids::play::clientbound::COMMANDS,
            &w.data,
        )
        .unwrap()
        .unwrap();
        let play::Clientbound::Commands(p) = play::Clientbound::decode(id, &body.unwrap()).unwrap()
        else {
            panic!("commands")
        };
        assert_eq!(
            p.nodes
                .iter()
                .map(|n| n.parser.as_ref().unwrap().0)
                .collect::<Vec<_>>(),
            [18, 43, 46]
        );
        assert_eq!(p.nodes[1].parser.as_ref().unwrap().1, 20i32.to_be_bytes());
        assert_eq!(p.nodes[2].name, "item");
    }
}
