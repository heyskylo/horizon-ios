//! 1.21.5 to 1.21.4 (protocol 770 to 769): the mirror of `translate`'s `v1_21_4` (1.21.4 to 1.21.5), for a
//! [`super::ClientSession`] whose client is 1.21.4 (roadmap `older-clients`).
//!
//! Clientbound, what 1.21.5 added is taken out again: the explosion's player knockback is a double and two floats,
//! the player chat has no leading global index, the team's name tag visibility and collision rule are names rather
//! than VarInt enums, and the advancements have no trailing show flag.
//! Serverbound, the chat packets have no trailing checksum (the client sends none: the server reads a 0 as "not
//! checked"), and a container click's changed slots and carried item are full stacks, not hashes: they are hashed
//! here with `HashedStack`'s rules (`Hashed::of`), with the client's item ids mapped to the pinned ones first.
//!
//! TODO(step3): the hashes of a stack with a component that names a registry entry are incomplete (`Hashed::of`
//! hashes without registry names), and 1.21.5's item components are not written in 1.21.4's layout (see the
//! TODOs in `translate::v1_21_4`).
use crate::{
    Result, State,
    adapter::{
        ids::{self, Ids, Way},
        translate,
    },
    buf::{Codec, Reader, Writer},
    chunk::storage_len,
    ids as packet_ids,
    item::{Hashed, HashedSlot, Slot, Stack},
    packets::play,
};

/// What becomes of a payload the 1.21.4 client sent.
#[derive(Debug, PartialEq)]
pub(super) enum Out {
    /// Not one of the packets this step rewrites.
    Untouched,
    Rewritten(Vec<u8>),
}

/// The clientbound packet `id` (`payload` in 1.21.5's layout) in 1.21.4's layout; `None` when it is laid out alike.
pub(super) fn clientbound(state: State, id: i32, payload: &[u8]) -> Result<Option<Vec<u8>>> {
    use packet_ids::play::clientbound as cb;
    if state != State::Play {
        return Ok(None);
    }
    Ok(Some(match id {
        cb::EXPLODE => explode_down(payload)?,
        cb::PLAYER_CHAT => {
            // The global index goes; 1.21.4 numbers no messages globally.
            let mut r = Reader::new(payload);
            r.varint()?;
            r.rest().to_vec()
        }
        cb::SET_PLAYER_TEAM => team_down(payload)?,
        // The show flag is the last field; 1.21.4 has none.
        cb::UPDATE_ADVANCEMENTS => payload[..payload.len().saturating_sub(1)].to_vec(),
        cb::LEVEL_CHUNK_WITH_LIGHT => chunk_down(payload)?,
        cb::CHUNKS_BIOMES => chunks_biomes_down(payload)?,
        _ => return Ok(None),
    }))
}

/// The serverbound packet `id` the 1.21.4 client sent (`payload`, under the pinned id) in 1.21.5's layout. `ids` is
/// the 1.21.4 table, which the stacks of a container click are read by.
pub(super) fn serverbound(
    state: State,
    id: i32,
    payload: &[u8],
    ids: Option<&'static Ids>,
) -> Result<Out> {
    use packet_ids::play::serverbound as sb;
    if state != State::Play {
        return Ok(Out::Untouched);
    }
    Ok(match id {
        sb::CHAT | sb::CHAT_COMMAND_SIGNED => {
            // 0 is the checksum 1.21.5 reads as "not checked".
            let mut p = payload.to_vec();
            p.push(0);
            Out::Rewritten(p)
        }
        sb::CONTAINER_CLICK => match ids {
            Some(ids) => Out::Rewritten(container_click(ids, payload)?),
            None => Out::Untouched,
        },
        _ => Out::Untouched,
    })
}

/// Entity data in 1.21.4's indexes, from 1.21.5's (`translate::client_layout_v1_21_4::remap_entity_data`, back). `kind` is the
/// entity's type in the pinned ids, `None` when unknown, which leaves the data as it is.
pub(super) fn entity_data(kind: Option<i32>, data: &mut play::SetEntityData) {
    if let Some(kind) = kind {
        translate::client_layout_v1_21_4::remap_entity_data(kind, data, false);
    }
}

/// A paletted container as 1.21.5 sends it, back to 1.21.4's: its data array's length (a VarInt) comes before
/// the longs, computed from the bits per entry (`storage_len`, with a block palette stored in at least 4 bits).
fn paletted_down(r: &mut Reader, w: &mut Writer, blocks: bool) -> Result<()> {
    let bits = r.u8()?;
    w.u8(bits);
    let mut storage = bits;
    if bits == 0 {
        w.varint(r.varint()?);
    } else if translate::client_layout_v1_21_4::indirect(bits, blocks) {
        let n = r.varint()?;
        w.varint(n);
        for _ in 0..n {
            w.varint(r.varint()?);
        }
        if blocks {
            storage = bits.max(4);
        }
    }
    let entries = if blocks { 4096 } else { 64 };
    let longs = storage_len(storage, entries);
    w.varint(longs as i32);
    w.bytes(r.bytes(longs * 8)?);
    Ok(())
}

/// The sections of a chunk column (or only their biomes) in 1.21.5's layout, back to 1.21.4's.
fn sections_down(data: &[u8], biomes_only: bool) -> Result<Vec<u8>> {
    let mut r = Reader::new(data);
    let mut w = Writer::new();
    while r.remaining() > 0 {
        if !biomes_only {
            w.i16(r.i16()?);
            paletted_down(&mut r, &mut w, true)?;
        }
        paletted_down(&mut r, &mut w, false)?;
    }
    Ok(w.data)
}

/// The heightmaps as 1.21.4 sends them: an NBT compound with a long array per name. A type 1.21.4 has no name for
/// is dropped.
fn heightmaps_down(r: &mut Reader, w: &mut Writer) -> Result<()> {
    let n = r.count(16)?;
    let mut map = std::collections::BTreeMap::new();
    for _ in 0..n {
        let kind = r.varint()?;
        let longs = r.long_array()?;
        if let Some(&(_, name)) = translate::client_layout_v1_21_4::HEIGHTMAPS
            .iter()
            .find(|(k, _)| *k == kind)
        {
            map.insert(
                horizon_world::nbt::JavaString::from(name),
                horizon_world::nbt::Tag::LongArray(longs),
            );
        }
    }
    w.nbt(&horizon_world::nbt::Tag::Compound(map));
    Ok(())
}

/// Level chunk with light, 1.21.5's layout to 1.21.4's (the mirror of `translate::client_layout_v1_21_4::chunk_up`).
fn chunk_down(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let mut w = Writer::new();
    w.i32(r.i32()?);
    w.i32(r.i32()?);
    heightmaps_down(&mut r, &mut w)?;
    let data = r.byte_array(r.remaining())?;
    w.byte_array(&sections_down(&data, false)?);
    w.bytes(r.rest());
    Ok(w.data)
}

/// Chunk biomes, 1.21.5's layout to 1.21.4's (the mirror of `translate::client_layout_v1_21_4::chunks_biomes_up`).
fn chunks_biomes_down(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let mut w = Writer::new();
    w.i32(r.i32()?);
    w.i32(r.i32()?);
    let data = r.byte_array(r.remaining())?;
    w.byte_array(&sections_down(&data, true)?);
    w.bytes(r.rest());
    Ok(w.data)
}

/// The explosion's player knockback: a double and two floats in 1.21.4, three doubles in 1.21.5.
fn explode_down(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let mut w = Writer::new();
    for _ in 0..3 {
        w.f64(r.f64()?);
    }
    let knockback = r.bool()?;
    w.bool(knockback);
    if knockback {
        w.f64(r.f64()?);
        w.f32(r.f64()? as f32);
        w.f32(r.f64()? as f32);
    }
    w.bytes(r.rest());
    Ok(w.data)
}

/// An update to a team in 1.21.4's layout: the name tag visibility and collision rule are sent by name.
fn team_down(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let mut w = Writer::new();
    w.string(&r.string(32767)?);
    let method = r.u8()?;
    w.u8(method);
    if method == 0 || method == 2 {
        w.nbt(&r.nbt()?);
        w.u8(r.u8()?);
        w.string(translate::client_layout_v1_21_4::name_of(
            &translate::client_layout_v1_21_4::VISIBILITY,
            r.varint()?,
        )?);
        w.string(translate::client_layout_v1_21_4::name_of(
            &translate::client_layout_v1_21_4::COLLISION,
            r.varint()?,
        )?);
        w.varint(r.varint()?);
        w.nbt(&r.nbt()?);
        w.nbt(&r.nbt()?);
    }
    if method == 0 || method == 3 || method == 4 {
        let n = r.count(r.remaining())?;
        w.varint(n as i32);
        for _ in 0..n {
            w.string(&r.string(32767)?);
        }
    }
    w.bytes(r.rest());
    Ok(w.data)
}

/// A container click with its stacks as 1.21.4 sends them (full slots, item ids of 1.21.4) in 1.21.5's hashed layout.
fn container_click(ids: &'static Ids, payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let mut w = Writer::new();
    w.varint(r.varint()?);
    w.varint(r.varint()?);
    w.i16(r.i16()?);
    w.i8(r.i8()?);
    w.varint(r.varint()?);
    let n = r.count(r.remaining())?;
    w.varint(n as i32);
    for _ in 0..n {
        w.i16(r.i16()?);
        let stack = slot(ids, &mut r)?;
        HashedSlot::write(&hashed(stack), &mut w);
    }
    let carried = slot(ids, &mut r)?;
    HashedSlot::write(&hashed(carried), &mut w);
    w.bytes(r.rest());
    Ok(w.data)
}

/// A full slot as 1.21.4 wrote it, its item and component ids mapped to the pinned ones.
fn slot(ids: &'static Ids, r: &mut Reader<'_>) -> Result<Option<Stack>> {
    let _up = ids::renumbering(ids, Way::Up);
    Slot::read(r)
}

fn hashed(stack: Option<Stack>) -> Option<Hashed> {
    stack.as_ref().map(Hashed::of)
}

#[cfg(test)]
mod tests {
    //! Each change is written here and read back with `translate`'s 1.21.4 reader: the round trip.
    use super::*;
    use crate::adapter::{ServerVersion, session::Session, translate};
    use crate::entity_data::{Value, serializer};

    /// An empty compound: a text component (network NBT, no root name) with no text.
    const EMPTY_TEXT: [u8; 2] = [0x0a, 0x00];

    fn read_back_clientbound(id: i32, down: &[u8]) -> Vec<u8> {
        let session = Session::new(ServerVersion::V1_21_4);
        translate::client_layout_v1_21_4::clientbound(&session, State::Play, id, down)
            .unwrap()
            .unwrap()
    }

    #[test]
    fn an_explosion_knockback_is_three_doubles_and_back() {
        let mut w = Writer::new();
        w.f64(1.5);
        w.f64(-2.0);
        w.f64(3.25);
        w.bool(true);
        w.f64(0.5);
        w.f64(1.0);
        w.f64(-0.25);
        w.varint(7);
        w.bytes(&[9, 9]);
        let pinned = w.data;
        let id = packet_ids::play::clientbound::EXPLODE;
        let older = clientbound(State::Play, id, &pinned).unwrap().unwrap();
        assert_ne!(older, pinned);
        assert_eq!(read_back_clientbound(id, &older), pinned);
    }

    #[test]
    fn an_explosion_without_knockback_is_the_same_in_both() {
        let mut w = Writer::new();
        w.f64(1.0);
        w.f64(2.0);
        w.f64(3.0);
        w.bool(false);
        w.varint(0);
        let pinned = w.data;
        let id = packet_ids::play::clientbound::EXPLODE;
        let older = clientbound(State::Play, id, &pinned).unwrap().unwrap();
        assert_eq!(older, pinned);
        assert_eq!(read_back_clientbound(id, &older), pinned);
    }

    #[test]
    fn a_player_chat_has_no_global_index_in_1_21_4() {
        let older: [u8; 3] = [1, 2, 3];
        let id = packet_ids::play::clientbound::PLAYER_CHAT;
        let up = read_back_clientbound(id, &older);
        assert_eq!(up, vec![0u8, 1, 2, 3]);
        let pinned: [u8; 4] = [5, 1, 2, 3];
        let down = clientbound(State::Play, id, &pinned).unwrap().unwrap();
        assert_eq!(down, vec![1u8, 2, 3]);
    }

    #[test]
    fn a_team_names_its_visibility_and_collision_and_reads_back() {
        let id = packet_ids::play::clientbound::SET_PLAYER_TEAM;
        let mut w = Writer::new();
        w.string("red");
        w.u8(0); // create
        w.bytes(&EMPTY_TEXT);
        w.u8(1);
        w.string("hideForOwnTeam");
        w.string("pushOwnTeam");
        w.varint(4);
        w.bytes(&EMPTY_TEXT);
        w.bytes(&EMPTY_TEXT);
        w.varint(1);
        w.string("Steve");
        let older = w.data;
        let up = read_back_clientbound(id, &older);
        let mut r = Reader::new(&up);
        assert_eq!(r.string(32767).unwrap(), "red");
        assert_eq!(r.u8().unwrap(), 0);
        r.nbt().unwrap();
        assert_eq!(r.u8().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 3); // hideForOwnTeam
        assert_eq!(r.varint().unwrap(), 3); // pushOwnTeam
        assert_eq!(clientbound(State::Play, id, &up).unwrap().unwrap(), older);
    }

    #[test]
    fn a_team_removal_is_only_its_name_and_method() {
        let id = packet_ids::play::clientbound::SET_PLAYER_TEAM;
        let mut w = Writer::new();
        w.string("red");
        w.u8(1);
        let older = w.data;
        let up = read_back_clientbound(id, &older);
        assert_eq!(clientbound(State::Play, id, &up).unwrap().unwrap(), older);
    }

    #[test]
    fn an_advancement_update_loses_its_show_flag() {
        let id = packet_ids::play::clientbound::UPDATE_ADVANCEMENTS;
        let mut w = Writer::new();
        w.bool(false); // reset
        w.varint(0);
        w.varint(0);
        let older = w.data;
        let up = read_back_clientbound(id, &older);
        assert_eq!(up.last(), Some(&0));
        assert_eq!(clientbound(State::Play, id, &up).unwrap().unwrap(), older);
    }

    #[test]
    fn a_chat_message_has_no_checksum_in_1_21_4() {
        let id = packet_ids::play::serverbound::CHAT;
        let mut w = Writer::new();
        w.string("hi");
        w.i64(1);
        w.i64(2);
        w.bool(false);
        w.varint(0);
        w.bytes(&[0, 0, 0]);
        let older = w.data;
        let Out::Rewritten(pinned) = serverbound(State::Play, id, &older, None).unwrap() else {
            panic!("a chat is rewritten");
        };
        assert_eq!(pinned.last(), Some(&0));
        assert_eq!(
            translate::client_layout_v1_21_4::without_checksum(&pinned),
            older
        );
    }

    /// A level chunk in 1.21.5's layout: two heightmaps, then a single-valued section, an indirect one (4 bits
    /// over a block palette) with direct biomes (6 bits).
    fn chunk_1_21_5() -> Vec<u8> {
        let mut sections = Writer::new();
        sections.i16(5);
        sections.u8(0);
        sections.varint(1);
        sections.u8(0);
        sections.varint(2);
        sections.i16(4096);
        sections.u8(4);
        sections.varint(2);
        sections.varint(7);
        sections.varint(9);
        for _ in 0..storage_len(4, 4096) {
            sections.i64(0x1234);
        }
        sections.u8(6);
        for _ in 0..storage_len(6, 64) {
            sections.i64(3);
        }
        let mut w = Writer::new();
        w.i32(3);
        w.i32(-7);
        w.varint(2);
        w.varint(1);
        w.long_array(&[1, 2]);
        w.varint(4);
        w.long_array(&[3]);
        w.byte_array(&sections.data);
        w.bytes(&[9, 9]);
        w.data
    }

    #[test]
    fn a_level_chunk_round_trips_through_1_21_4s_layout() {
        let id = packet_ids::play::clientbound::LEVEL_CHUNK_WITH_LIGHT;
        let newer = chunk_1_21_5();
        let older = clientbound(State::Play, id, &newer).unwrap().unwrap();
        assert_ne!(older, newer);
        assert_eq!(
            translate::client_layout_v1_21_4::chunk_up(&older).unwrap(),
            newer
        );
    }

    #[test]
    fn a_1_21_4_heightmap_is_named_and_its_type_is_dropped_when_unknown() {
        let mut w = Writer::new();
        w.i32(0);
        w.i32(0);
        let mut map = std::collections::BTreeMap::new();
        map.insert(
            horizon_world::nbt::JavaString::from("WORLD_SURFACE"),
            horizon_world::nbt::Tag::LongArray(vec![5]),
        );
        map.insert(
            horizon_world::nbt::JavaString::from("WORLD_SURFACE_WG"),
            horizon_world::nbt::Tag::LongArray(vec![6]),
        );
        w.nbt(&horizon_world::nbt::Tag::Compound(map));
        w.byte_array(&[]);
        let up = translate::client_layout_v1_21_4::chunk_up(&w.data).unwrap();
        let mut r = Reader::new(&up);
        assert_eq!((r.i32().unwrap(), r.i32().unwrap()), (0, 0));
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 1); // WORLD_SURFACE
        assert_eq!(r.long_array().unwrap(), vec![5]);
    }

    #[test]
    fn chunk_biomes_round_trip_through_1_21_4s_layout() {
        let id = packet_ids::play::clientbound::CHUNKS_BIOMES;
        let mut biomes = Writer::new();
        biomes.u8(0);
        biomes.varint(2);
        biomes.u8(6);
        for _ in 0..storage_len(6, 64) {
            biomes.i64(3);
        }
        let mut w = Writer::new();
        w.i32(4);
        w.i32(-1);
        w.byte_array(&biomes.data);
        let newer = w.data;
        let older = clientbound(State::Play, id, &newer).unwrap().unwrap();
        assert_ne!(older, newer);
        assert_eq!(
            translate::client_layout_v1_21_4::chunks_biomes_up(&older).unwrap(),
            newer
        );
    }

    /// Entity data of `kind` taken through translate's way up, then this step's way back.
    fn up_then_down(
        kind: i32,
        older: Vec<(u8, i32, Value)>,
    ) -> (Vec<(u8, i32, Value)>, Vec<(u8, i32, Value)>) {
        let mut d = play::SetEntityData {
            id: 1,
            values: older,
        };
        translate::client_layout_v1_21_4::remap_entity_data(kind, &mut d, true);
        let newer = d.values.clone();
        entity_data(Some(kind), &mut d);
        (newer, d.values)
    }

    #[test]
    fn a_dolphin_loses_its_treasure_position_and_keeps_the_rest() {
        let older = vec![
            (17, serializer::BLOCK_POS, Value::Pos([1, 2, 3])),
            (18, serializer::BOOLEAN, Value::Bool(true)),
            (19, serializer::INT, Value::Int(5)),
        ];
        let (newer, back) = up_then_down(translate::client_layout_v1_21_4::DOLPHIN, older);
        assert_eq!(
            newer,
            vec![
                (17, serializer::BOOLEAN, Value::Bool(true)),
                (18, serializer::INT, Value::Int(5)),
            ]
        );
        assert_eq!(
            back,
            vec![
                (18, serializer::BOOLEAN, Value::Bool(true)),
                (19, serializer::INT, Value::Int(5)),
            ]
        );
    }

    #[test]
    fn a_turtle_keeps_its_eggs_and_loses_its_positions() {
        let older = vec![
            (17, serializer::BLOCK_POS, Value::Pos([1, 2, 3])),
            (18, serializer::BOOLEAN, Value::Bool(true)),
            (19, serializer::BOOLEAN, Value::Bool(false)),
            (20, serializer::BLOCK_POS, Value::Pos([4, 5, 6])),
            (21, serializer::BOOLEAN, Value::Bool(true)),
            (22, serializer::BOOLEAN, Value::Bool(true)),
        ];
        let (newer, back) = up_then_down(translate::client_layout_v1_21_4::TURTLE, older);
        assert_eq!(
            newer,
            vec![
                (17, serializer::BOOLEAN, Value::Bool(true)),
                (18, serializer::BOOLEAN, Value::Bool(false)),
            ]
        );
        assert_eq!(
            back,
            vec![
                (18, serializer::BOOLEAN, Value::Bool(true)),
                (19, serializer::BOOLEAN, Value::Bool(false)),
            ]
        );
    }

    #[test]
    fn a_strider_loses_its_saddle() {
        let older = vec![
            (17, serializer::INT, Value::Int(3)),
            (18, serializer::BOOLEAN, Value::Bool(true)),
            (19, serializer::BOOLEAN, Value::Bool(true)),
        ];
        let (newer, back) = up_then_down(translate::client_layout_v1_21_4::STRIDER, older);
        let kept = vec![
            (17, serializer::INT, Value::Int(3)),
            (18, serializer::BOOLEAN, Value::Bool(true)),
        ];
        assert_eq!(newer, kept);
        assert_eq!(back, kept);
    }

    #[test]
    fn a_pig_loses_its_saddle_and_gains_a_variant_it_is_not_sent() {
        let older = vec![
            (17, serializer::BOOLEAN, Value::Bool(true)),
            (18, serializer::INT, Value::Int(7)),
        ];
        let (newer, back) = up_then_down(translate::client_layout_v1_21_4::PIG, older);
        assert_eq!(newer, vec![(17, serializer::INT, Value::Int(7))]);
        assert_eq!(back, vec![(18, serializer::INT, Value::Int(7))]);
    }

    #[test]
    fn a_mooshroom_s_type_is_a_string_then_a_varint() {
        let older = vec![(17, serializer::STRING, Value::String("brown".into()))];
        let (newer, back) =
            up_then_down(translate::client_layout_v1_21_4::MOOSHROOM, older.clone());
        assert_eq!(newer, vec![(17, serializer::INT, Value::Int(1))]);
        assert_eq!(back, older);
    }

    #[test]
    fn a_minecart_s_block_moves_to_11_and_12_and_its_shaking_goes() {
        let older = vec![
            (8, serializer::INT, Value::Int(5)),
            (9, serializer::INT, Value::Int(2)),
            (10, serializer::BOOLEAN, Value::Bool(true)),
        ];
        let (newer, back) = up_then_down(translate::client_layout_v1_21_4::MINECARTS[0], older);
        assert_eq!(
            newer,
            vec![
                (11, serializer::OPTIONAL_BLOCK_STATE, Value::Id(5)),
                (12, serializer::INT, Value::Int(2)),
            ]
        );
        assert_eq!(
            back,
            vec![
                (8, serializer::INT, Value::Int(5)),
                (10, serializer::BOOLEAN, Value::Bool(true)),
                (9, serializer::INT, Value::Int(2)),
            ]
        );
    }

    #[test]
    fn a_hidden_minecart_block_is_not_sent() {
        let older = vec![
            (8, serializer::INT, Value::Int(5)),
            (9, serializer::INT, Value::Int(2)),
            (10, serializer::BOOLEAN, Value::Bool(false)),
        ];
        let (newer, back) = up_then_down(translate::client_layout_v1_21_4::MINECARTS[1], older);
        assert_eq!(newer, vec![(12, serializer::INT, Value::Int(2))]);
        assert_eq!(back, vec![(9, serializer::INT, Value::Int(2))]);
    }

    #[test]
    fn an_experience_orb_s_value_is_dropped_going_down() {
        let mut d = play::SetEntityData {
            id: 1,
            values: vec![(8, serializer::INT, Value::Int(4))],
        };
        entity_data(
            Some(translate::client_layout_v1_21_4::EXPERIENCE_ORB),
            &mut d,
        );
        assert!(d.values.is_empty());
    }

    #[test]
    fn a_piglin_gains_an_immunity_first_and_loses_it_again() {
        let older = vec![
            (16, serializer::BOOLEAN, Value::Bool(true)),
            (17, serializer::BOOLEAN, Value::Bool(false)),
            (18, serializer::BOOLEAN, Value::Bool(true)),
        ];
        let (newer, back) = up_then_down(translate::client_layout_v1_21_4::PIGLIN, older.clone());
        assert_eq!(
            newer,
            vec![
                (17, serializer::BOOLEAN, Value::Bool(true)),
                (18, serializer::BOOLEAN, Value::Bool(false)),
                (19, serializer::BOOLEAN, Value::Bool(true)),
            ]
        );
        assert_eq!(back, older);
    }

    #[test]
    fn the_variants_1_21_5_added_are_dropped_going_down() {
        let mut chicken = play::SetEntityData {
            id: 1,
            values: vec![(17, serializer::CHICKEN_VARIANT, Value::Id(3))],
        };
        entity_data(
            Some(translate::client_layout_v1_21_4::CHICKEN),
            &mut chicken,
        );
        assert!(chicken.values.is_empty());
        let mut wolf = play::SetEntityData {
            id: 1,
            values: vec![
                (22, serializer::WOLF_VARIANT, Value::Id(1)),
                (23, serializer::WOLF_SOUND_VARIANT, Value::Id(2)),
            ],
        };
        entity_data(Some(translate::client_layout_v1_21_4::WOLF), &mut wolf);
        assert_eq!(
            wolf.values,
            vec![(22, serializer::WOLF_VARIANT, Value::Id(1))]
        );
    }

    #[test]
    fn a_kind_of_no_change_passes_and_an_unknown_one_is_left_alone() {
        let values = vec![
            (0, serializer::BYTE, Value::Byte(1)),
            (9, serializer::FLOAT, Value::Float(2.0)),
        ];
        let mut d = play::SetEntityData {
            id: 1,
            values: values.clone(),
        };
        entity_data(Some(1), &mut d);
        assert_eq!(d.values, values);
        let mut unknown = play::SetEntityData {
            id: 1,
            values: values.clone(),
        };
        entity_data(None, &mut unknown);
        assert_eq!(unknown.values, values);
    }

    #[test]
    fn a_container_click_of_empty_slots_is_hashed_and_back() {
        let id = packet_ids::play::serverbound::CONTAINER_CLICK;
        let ids = ServerVersion::V1_21_4.ids().unwrap();
        let mut w = Writer::new();
        w.varint(1); // window
        w.varint(2); // state
        w.i16(36);
        w.i8(0);
        w.varint(0); // mode
        w.varint(1); // one changed slot
        w.i16(36);
        w.varint(0); // empty
        w.varint(0); // carried: empty
        let older = w.data;
        let Out::Rewritten(pinned) = serverbound(State::Play, id, &older, Some(ids)).unwrap()
        else {
            panic!("a container click is rewritten");
        };
        let back = translate::client_layout_v1_21_4::container_click(&pinned).unwrap();
        assert_eq!(back, older);
    }
}
