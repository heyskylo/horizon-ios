//! What changes inside packets between an older release and the pinned one, after their ids are renumbered:
//! registry ids (block states, items, entity types, ...) and the packets whose fields changed. A packet not
//! named here passes byte for byte.
//!
//! Most packets that carry registry ids are decoded with the older version's ids renumbered as they are read
//! ([`ids::renumbering`]) and encoded again for Horizon; ids in plain fields are renumbered between the two.
//! A packet whose layout changed is read in the older layout by hand here.
use super::{
    ServerVersion,
    ids::{self, Ids, Way},
    session::Session,
};
use crate::{
    Result, State,
    buf::{Codec, Reader, Writer},
    chunk::{Paletted, Section, Strategy},
    ids as packet_ids,
    packets::{configuration, play},
};

pub(super) mod v1_20_2;
pub(super) mod v1_20_3;
mod v1_20_5;
pub(super) mod v1_21_1;
mod v1_21_3;
mod v1_21_4;
mod v1_21_5;

pub(super) fn old_experience_orb(session: &Session, payload: &[u8]) -> Result<(i32, Vec<u8>)> {
    v1_21_4::experience_orb(session, payload)
}
pub(super) mod v1_21_8;

impl Ids {
    /// The table of a registry tags are sent for, by its name; registries the protocol syncs from the
    /// server's own data (biomes, enchantments, ...) are numbered by that data and alike on both sides.
    pub fn by_registry(&self, registry: &str) -> ids::Remap {
        match registry {
            "minecraft:block" => self.block,
            "minecraft:item" => self.item,
            "minecraft:entity_type" => self.entity_type,
            "minecraft:fluid" => self.fluid,
            "minecraft:game_event" => self.game_event,
            "minecraft:particle_type" => self.particle_type,
            "minecraft:sound_event" => self.sound_event,
            "minecraft:mob_effect" => self.mob_effect,
            "minecraft:attribute" => self.attribute,
            "minecraft:block_entity_type" => self.block_entity_type,
            "minecraft:menu" => self.menu,
            "minecraft:villager_type" => self.villager_type,
            "minecraft:villager_profession" => self.villager_profession,
            "minecraft:data_component_type" => self.data_component_type,
            _ => ids::Remap::Same,
        }
    }
}

/// A clientbound packet, renumbered to the pinned `id`, as the pinned version lays it out: the pinned id
/// it now has (an older packet can become a different one) and its new payload, `None` when the payload
/// stays as it is. `None` altogether drops the packet.
pub fn clientbound(
    session: &Session,
    state: State,
    id: i32,
    payload: &[u8],
) -> Result<Option<(i32, Option<Vec<u8>>)>> {
    let Some(ids) = session.version.ids() else {
        return Ok(Some((id, None)));
    };
    use packet_ids::play::clientbound as cb;
    let normalized_text = if session.version.at_most(ServerVersion::V1_20_2) {
        v1_20_2::text::clientbound(session, state, id, payload)?
    } else {
        None
    };
    let payload = normalized_text.as_deref().unwrap_or(payload);
    if session.version.at_most(ServerVersion::V1_20_2) && state == State::Play {
        match id {
            cb::SET_SCORE => {
                return v1_20_2::set_score(session, payload)
                    .map(|(id, body)| Some((id, Some(body))));
            }
            cb::EXPLODE => {
                return v1_20_2::explosion(session, payload).map(|body| Some((id, Some(body))));
            }
            _ => {}
        }
    }
    if session.version.at_most(ServerVersion::V1_20_3)
        && (state, id)
            == (
                State::Configuration,
                packet_ids::configuration::clientbound::REGISTRY_DATA,
            )
    {
        let mut r = Reader::new(payload);
        let packets = super::legacy::split_registry_data(&r.nbt()?)?;
        if r.remaining() != 0 || packets.is_empty() {
            return Err(crate::Error::new("invalid legacy registry packet"));
        }
        *session.legacy_items.lock().unwrap() = Default::default();
        let mut bodies = Vec::new();
        for packet in packets {
            super::legacy::remember(session, &packet);
            let mut w = Writer::new();
            configuration::RegistryData::write(&packet, &mut w);
            v1_21_4::registry_data(session, &w.data)?;
            bodies.push(w.data);
        }
        let first = bodies.remove(0);
        session
            .incoming
            .lock()
            .unwrap()
            .extend(bodies.into_iter().map(|body| (id, body)));
        return Ok(Some((id, Some(first))));
    }
    if session.version.at_most(ServerVersion::V1_20_3) && state == State::Play {
        match id {
            cb::EXPLODE => {
                return v1_20_3::explosion(session, payload).map(|body| Some((id, Some(body))));
            }
            cb::MERCHANT_OFFERS => {
                return v1_20_3::merchant_offers(session, payload)
                    .map(|body| Some((id, Some(body))));
            }
            cb::SET_EQUIPMENT => {
                return v1_20_3::equipment(session, payload).map(|body| Some((id, Some(body))));
            }
            _ => {}
        }
    }
    if session.version.at_most(ServerVersion::V1_21_1) && state == State::Play {
        if matches!(id, cb::UPDATE_RECIPES | cb::PLACE_GHOST_RECIPE) {
            return v1_21_1::recipes::clientbound(session, id, payload)
                .map(|body| body.map(|body| (id, Some(body))));
        }
        if id == cb::CONTAINER_SET_SLOT
            && let Some((id, body)) = v1_21_1::container_slot(session, payload)?
        {
            return Ok(Some((id, Some(body))));
        }
        if id == cb::EXPLODE {
            return v1_21_1::explosion(session, payload).map(|body| Some((id, Some(body))));
        }
    }
    if session.version.at_most(ServerVersion::V1_21_4)
        && state == State::Configuration
        && id == packet_ids::configuration::clientbound::REGISTRY_DATA
    {
        v1_21_4::registry_data(session, payload)?;
    }
    let _trims = (session.version.at_most(ServerVersion::V1_21_4)
        && state == State::Play
        && matches!(id, cb::RECIPE_BOOK_ADD | cb::PLACE_GHOST_RECIPE))
    .then(|| ids::trim_patterns(session.trim_patterns.lock().unwrap().clone()));
    // 1.21.11's configuration ends with the world clocks 26.1 syncs as a registry.
    if session.version.at_most(ServerVersion::V1_21_11)
        && (state, id)
            == (
                State::Configuration,
                packet_ids::configuration::clientbound::FINISH_CONFIGURATION,
            )
    {
        let clocks = v1_21_11::world_clocks();
        let mut incoming = session.incoming.lock().unwrap();
        let mut registries = legacy_registries(session);
        registries.push(clocks);
        let first = registries.remove(0);
        incoming.extend(
            registries
                .into_iter()
                .map(|body| (packet_ids::configuration::clientbound::REGISTRY_DATA, body)),
        );
        incoming.push_back((id, payload.to_vec()));
        return Ok(Some((
            packet_ids::configuration::clientbound::REGISTRY_DATA,
            Some(first),
        )));
    }
    // A release at a time: an older server's packets are brought to 26.2's layout first, oldest step first.
    let mut before: Option<Vec<u8>> = None;
    type Step = fn(&Session, State, i32, &[u8]) -> Result<Option<Vec<u8>>>;
    let steps: [(ServerVersion, Step); 10] = [
        (ServerVersion::V1_20_2, v1_20_2::clientbound),
        (ServerVersion::V1_20_3, v1_20_3::clientbound),
        (ServerVersion::V1_20_6, v1_20_5::clientbound),
        (ServerVersion::V1_21_1, v1_21_1::clientbound),
        (ServerVersion::V1_21_3, v1_21_3::clientbound),
        (ServerVersion::V1_21_4, v1_21_4::clientbound),
        (ServerVersion::V1_21_5, v1_21_5::clientbound),
        (ServerVersion::V1_21_8, v1_21_8::clientbound),
        (ServerVersion::V1_21_11, v1_21_11::clientbound),
        (ServerVersion::V26_1, v26_1::clientbound),
    ];
    for (from, step) in steps {
        if session.version.at_most(from)
            && let Some(next) = step(session, state, id, before.as_deref().unwrap_or(payload))?
        {
            before = Some(next);
        }
    }
    let payload = before.as_deref().unwrap_or(payload);
    let out = match (state, id) {
        (State::Configuration, packet_ids::configuration::clientbound::UPDATE_TAGS) => {
            let mut p = configuration::Clientbound::decode(id, payload)?;
            if let configuration::Clientbound::UpdateTags(t) = &mut p {
                tags(ids, &mut t.registries);
            }
            strip_id(p.encode())
        }
        (State::Play, cb::UPDATE_TAGS) => {
            let mut p = decode_play(ids, id, payload)?;
            if let play::Clientbound::UpdateTags(t) = &mut p {
                tags(ids, &mut t.registries);
            }
            strip_id(p.encode())
        }
        (State::Play, cb::LOGIN) => {
            session.legacy_boats.lock().unwrap().clear();
            session.legacy_metadata.lock().unwrap().clear();
            let body = login(payload, session.version)?;
            let login = play::Login::read(&mut Reader::new(&body))?;
            *session.local_player.lock().unwrap() = Some((login.player_id, false));
            *session.dimension.lock().unwrap() = login.spawn.dimension;
            // The local player receives no AddEntity packet; its metadata still needs the player step.
            session
                .kinds
                .lock()
                .unwrap()
                .insert(login.player_id, packet_ids_entity::PLAYER);
            body
        }
        (State::Play, cb::RESPAWN) => {
            if let Some((_, shift)) = session.local_player.lock().unwrap().as_mut() {
                *shift = false;
            }
            let body = respawn(payload, session.version)?;
            let dimension = play::Respawn::read(&mut Reader::new(&body))?
                .spawn
                .dimension;
            if *session.dimension.lock().unwrap() != dimension {
                session.legacy_boats.lock().unwrap().clear();
                session.legacy_metadata.lock().unwrap().clear();
            }
            *session.dimension.lock().unwrap() = dimension;
            body
        }
        (State::Play, cb::LEVEL_CHUNK_WITH_LIGHT) => chunk(session, ids, payload)?,
        (State::Play, cb::LIGHT_UPDATE) => light_update(payload)?,
        (State::Play, cb::UPDATE_ADVANCEMENTS) => advancements(ids, payload)?,
        (State::Play, cb::LEVEL_PARTICLES) => particles(ids, payload)?,
        (State::Play, cb::EXPLODE) => {
            // 26.3 adds whether the blast's sound plays, last; 26.2 always played it.
            let mut body = payload.to_vec();
            body.push(1);
            redecode(ids, id, &body)?
        }
        (State::Play, cb::MOVE_ENTITY_POS) => move_entity(payload, true, false)?,
        (State::Play, cb::MOVE_ENTITY_POS_ROT) => move_entity(payload, true, true)?,
        (State::Play, cb::MOVE_ENTITY_ROT) => move_entity(payload, false, true)?,
        (State::Play, cb::ENTITY_POSITION_SYNC) => position_sync(payload)?,
        (State::Play, cb::ANIMATE) => {
            return animate(payload).map(|(id, body)| Some((id, Some(body))));
        }
        (State::Play, cb::ADD_ENTITY) => {
            let mut p = decode_play(ids, id, payload)?;
            if let play::Clientbound::AddEntity(e) = &mut p {
                session.legacy_metadata.lock().unwrap().forget(e.id);
                let falling_block = ids.entity_type.down(packet_ids_entity::FALLING_BLOCK);
                if Some(e.kind) == falling_block {
                    e.data = ids.block_state.up(e.data).unwrap_or(0);
                }
                let Some(kind) = ids.entity_type.up(e.kind) else {
                    return Ok(None);
                };
                e.kind = kind;
                if session.version.at_most(ServerVersion::V1_21_1) {
                    v1_21_1::boats::spawn(session, ids.entity_type.down(e.kind), e);
                }
            }
            strip_id(p.encode())
        }
        (State::Play, cb::BLOCK_ENTITY_DATA) => {
            let mut p = decode_play(ids, id, payload)?;
            if let play::Clientbound::BlockEntityData(b) = &mut p {
                let Some(kind) = ids.block_entity_type.up(b.kind) else {
                    return Ok(None);
                };
                v1_20_3::upgrade_block_entity(session, b.kind, &mut b.tag);
                b.kind = kind;
            }
            strip_id(p.encode())
        }
        (State::Play, cb::BLOCK_UPDATE) => {
            let mut p = decode_play(ids, id, payload)?;
            if let play::Clientbound::BlockUpdate(b) = &mut p {
                b.state = ids.block_state.up(b.state).unwrap_or(0);
            }
            strip_id(p.encode())
        }
        (State::Play, cb::SECTION_BLOCKS_UPDATE) => {
            let mut p = decode_play(ids, id, payload)?;
            if let play::Clientbound::SectionBlocksUpdate(s) = &mut p {
                for change in &mut s.changes {
                    let state = ids.block_state.up((*change >> 12) as i32).unwrap_or(0);
                    *change = i64::from(state) << 12 | (*change & 0xfff);
                }
            }
            strip_id(p.encode())
        }
        (State::Play, cb::LEVEL_EVENT) => {
            let mut p = decode_play(ids, id, payload)?;
            if let play::Clientbound::LevelEvent(e) = &mut p {
                // LevelEvent.PARTICLES_DESTROY_BLOCK and the brush's BLOCK_FINISHED_BRUSHING carry a state.
                if matches!(e.kind, 2001 | 3008) {
                    e.data = ids.block_state.up(e.data).unwrap_or(0);
                }
            }
            strip_id(p.encode())
        }
        (State::Play, cb::UPDATE_ATTRIBUTES) => {
            let mut p = decode_play(ids, id, payload)?;
            if let play::Clientbound::UpdateAttributes(a) = &mut p {
                // An attribute the pinned version lacks can't be: 26.3 only added some.
                for v in &mut a.values {
                    v.attribute = ids.attribute.up(v.attribute).unwrap_or(v.attribute);
                }
            }
            strip_id(p.encode())
        }
        (State::Play, cb::SET_ENTITY_DATA) if session.version.at_most(ServerVersion::V1_21_11) => {
            let mut p = decode_play(ids, id, payload)?;
            if let play::Clientbound::SetEntityData(d) = &mut p {
                if session.version.at_most(ServerVersion::V1_20_3) {
                    v1_20_3::entity_data(session, d);
                }
                if session.version.at_most(ServerVersion::V1_21_1) {
                    v1_21_1::boats::metadata(session, d);
                }
                if session.version.at_most(ServerVersion::V1_21_4) {
                    v1_21_4::entity_data(session, d);
                }
                if session.version.at_most(ServerVersion::V1_21_5) {
                    v1_21_5::entity_data(session, d);
                }
                if session.version.at_most(ServerVersion::V1_21_8) {
                    v1_21_8::entity_data(session, d);
                }
                if session.version.at_most(ServerVersion::V1_21_10) {
                    v1_21_10::entity_data(session, d);
                }
                v1_21_11::entity_data(session, d);
            }
            strip_id(p.encode())
        }
        (
            State::Play,
            cb::SET_ENTITY_DATA
            | cb::CONTAINER_SET_CONTENT
            | cb::CONTAINER_SET_SLOT
            | cb::SET_CURSOR_ITEM
            | cb::SET_PLAYER_INVENTORY
            | cb::SET_EQUIPMENT
            | cb::MERCHANT_OFFERS
            | cb::SOUND
            | cb::SOUND_ENTITY
            | cb::RECIPE_BOOK_ADD
            | cb::PLACE_GHOST_RECIPE
            | cb::COMMANDS,
        ) => redecode(ids, id, payload)?,
        _ => return Ok(Some((id, before.or(normalized_text)))),
    };
    Ok(Some((id, Some(out))))
}

pub(super) use super::pinned_ids::entity_type as packet_ids_entity;

/// A serverbound packet with Horizon's `id`, as the older server numbers and lays it out: its id there and
/// the payload. `None`: nothing is sent.
pub fn serverbound(
    session: &Session,
    state: State,
    id: i32,
    payload: &[u8],
) -> Result<Option<(i32, Vec<u8>)>> {
    let renumbered = |id: i32, payload: Vec<u8>| {
        Ok(session
            .version
            .serverbound_id(state, id)
            .map(|mapped| (mapped, payload)))
    };
    let Some(ids) = session.version.ids() else {
        return renumbered(id, payload.to_vec());
    };
    use packet_ids::play::serverbound as sb;
    if session.version.at_most(ServerVersion::V1_20_2) {
        if matches!(
            (state, id),
            (
                State::Configuration,
                packet_ids::configuration::serverbound::RESOURCE_PACK
            ) | (State::Play, sb::RESOURCE_PACK)
        ) {
            return v1_20_2::resource_status(session, payload)?
                .map_or(Ok(None), |body| renumbered(id, body));
        }
        if (state, id) == (State::Play, sb::SET_JIGSAW_BLOCK) {
            return renumbered(id, v1_20_2::jigsaw(payload)?);
        }
    }
    match (state, id) {
        (State::Play, sb::CONTAINER_BUTTON_CLICK)
            if session.version.at_most(ServerVersion::V1_20_3) =>
        {
            renumbered(id, v1_20_3::container_button(payload)?)
        }
        (State::Play, sb::USE_ITEM) if session.version.at_most(ServerVersion::V1_20_6) => {
            renumbered(id, v1_20_5::use_item(payload)?)
        }
        (State::Play, sb::USE_ITEM_ON) if session.version.at_most(ServerVersion::V1_21_1) => {
            renumbered(id, v1_21_1::use_item_on(payload)?)
        }
        (State::Configuration, packet_ids::configuration::serverbound::CLIENT_INFORMATION)
        | (State::Play, sb::CLIENT_INFORMATION)
            if session.version.at_most(ServerVersion::V1_21_1) =>
        {
            renumbered(id, v1_21_1::client_information(payload)?)
        }
        (State::Play, sb::PLACE_RECIPE | sb::RECIPE_BOOK_SEEN_RECIPE)
            if session.version.at_most(ServerVersion::V1_21_1) =>
        {
            match v1_21_1::recipes::serverbound(session, id, payload)? {
                Some(body) => renumbered(id, body),
                None => Ok(None),
            }
        }
        (
            State::Play,
            sb::MOVE_PLAYER_POS
            | sb::MOVE_PLAYER_POS_ROT
            | sb::MOVE_PLAYER_ROT
            | sb::MOVE_PLAYER_STATUS_ONLY,
        ) if session.version.at_most(ServerVersion::V1_21_1) => {
            play::Serverbound::decode(id, payload)?;
            let mut body = payload.to_vec();
            if let Some(flags) = body.last_mut() {
                *flags &= 1;
            }
            renumbered(id, body)
        }
        (State::Play, sb::CONTAINER_CLOSE) if session.version.at_most(ServerVersion::V1_21_1) => {
            let container = Reader::new(payload).varint()?;
            let container = u8::try_from(container)
                .map_err(|_| crate::Error::new("legacy container id out of range"))?;
            renumbered(id, vec![container])
        }
        (State::Play, sb::MOVE_VEHICLE) if session.version.at_most(ServerVersion::V1_21_3) => {
            let p = play::Serverbound::decode(id, payload)?;
            if session.version.at_most(ServerVersion::V1_21_1)
                && let play::Serverbound::MoveVehicle(p) = p
            {
                v1_21_1::boats::controlled(session, p.position, p.yaw, p.pitch);
            }
            renumbered(id, payload[..payload.len() - 1].to_vec())
        }
        (State::Play, sb::CHAT | sb::CHAT_COMMAND_SIGNED)
            if session.version.at_most(ServerVersion::V1_21_4) =>
        {
            // LastSeenMessages.Update gained its trailing checksum in 1.21.5.
            play::Serverbound::decode(id, payload)?;
            renumbered(id, payload[..payload.len() - 1].to_vec())
        }
        (State::Play, sb::CONTAINER_CLICK) if session.version.at_most(ServerVersion::V1_21_4) => {
            let body = v1_21_4::container_click(payload)?;
            if session.version.at_most(ServerVersion::V1_21_1) {
                let mut r = Reader::new(&body);
                let container = u8::try_from(r.varint()?)
                    .map_err(|_| crate::Error::new("legacy container id out of range"))?;
                let mut w = Writer::new();
                w.u8(container);
                w.bytes(r.rest());
                renumbered(id, w.data)
            } else {
                renumbered(id, body)
            }
        }
        (State::Play, sb::PLAYER_COMMAND) if session.version.at_most(ServerVersion::V1_21_5) => {
            renumbered(id, v1_21_5::player_command(payload)?)
        }
        (State::Play, sb::PLAYER_INPUT) if session.version.at_most(ServerVersion::V1_21_5) => {
            v1_21_5::input(session, payload)?;
            let body = if session.version.at_most(ServerVersion::V1_21_1) {
                v1_21_1::serverbound_input(payload)?
            } else {
                payload.to_vec()
            };
            renumbered(id, body)
        }
        // 26.3 also states where the client was teleported to; 26.2 only the teleport's id.
        (State::Play, sb::ACCEPT_TELEPORTATION) => {
            let mut r = Reader::new(payload);
            let mut w = Writer::new();
            w.varint(r.varint()?);
            renumbered(id, w.data)
        }
        // 26.3 inserted CHANGE_DESTROY_DIRECTION at 1; 26.2 has no such action.
        (State::Play, sb::PLAYER_ACTION) => {
            let mut r = Reader::new(payload);
            let action = r.varint()?;
            let action = match action {
                0 => 0,
                1 | 8 => return Ok(None),
                n => n - 1,
            };
            let mut w = Writer::new();
            w.varint(action);
            w.bytes(r.rest());
            renumbered(id, w.data)
        }
        (State::Play, sb::INTERACT) if session.version.at_most(ServerVersion::V1_21_11) => {
            v1_21_11::interact(session, payload)
        }
        (State::Play, sb::ATTACK) if session.version.at_most(ServerVersion::V1_21_11) => {
            v1_21_11::attack(session, payload)
        }
        // 1.21.6 names a test by a VarInt id, 26.3 by its identifier: a named test is dropped (v1_21_6).
        (State::Play, sb::TEST_INSTANCE_BLOCK_ACTION)
            if session.version.at_most(ServerVersion::V1_21_6) =>
        {
            if v1_21_6::names_test(payload)? {
                Ok(None)
            } else {
                renumbered(id, payload.to_vec())
            }
        }
        // 26.1 is told the entity a spectator picked by its UUID (spectate_entity, at the same id).
        (State::Play, sb::SPECTATOR_ACTION) if session.version == ServerVersion::V26_1 => {
            v26_1::spectate(session, payload)
        }
        // 26.3's punch took over 26.2's swing of the main hand.
        (State::Play, sb::PUNCH) => {
            let mut w = Writer::new();
            w.varint(0);
            Ok(Some((swing_id(session.version), w.data)))
        }
        (State::Play, sb::SET_CREATIVE_MODE_SLOT | sb::CONTAINER_CLICK) => {
            let p = play::Serverbound::decode(id, payload)?;
            let payload = {
                let _down = ids::renumbering(ids, Way::Down);
                strip_id(p.encode())
            };
            renumbered(id, payload)
        }
        // 26.3 ends sign_update with the side (a VarInt); every older release reads it as a boolean right
        // after the position, before the four lines.
        (State::Play, sb::SIGN_UPDATE) => renumbered(id, sign_update(payload)?),
        _ => renumbered(id, payload.to_vec()),
    }
}

/// A sign_update payload as the older releases read it: position, side, four lines. 26.3 sends the side after
/// the lines as a VarInt, which for 0 and 1 is one byte equal to the boolean.
fn sign_update(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let pos = r.i64()?;
    let mut lines = Vec::with_capacity(4);
    for _ in 0..4 {
        lines.push(r.string(384)?);
    }
    let front = r.varint()? == 1;
    let mut w = Writer::new();
    w.i64(pos);
    w.bool(front);
    for line in &lines {
        w.string(line);
    }
    Ok(w.data)
}

/// 26.2's serverbound `swing` (play), which 26.3 replaced with `punch`.
const V26_2_SWING: i32 = 63;

/// The id an older `version` numbers its serverbound `swing` (play) by.
pub(super) fn swing_id(version: ServerVersion) -> i32 {
    if version.at_most(ServerVersion::V1_20_2) {
        50
    } else if version.at_most(ServerVersion::V1_20_3) {
        51
    } else if version.at_most(ServerVersion::V1_21_1) {
        54 // Vanilla 1.21/1.21.1, before bundle selection and client tick-end.
    } else if version.at_most(ServerVersion::V1_21_3) {
        56 // Vanilla 1.21.2/1.21.3, before block/entity picking and player_loaded.
    } else if version.at_most(ServerVersion::V1_21_4) {
        58 // Vanilla 1.21.4, before the two 1.21.5 test-block packets.
    } else if version.at_most(ServerVersion::V1_21_5) {
        59 // Vanilla 1.21.5's swing, before change_game_mode shifted the later ids.
    } else if version.at_most(ServerVersion::V1_21_11) {
        v1_21_11::SWING
    } else {
        V26_2_SWING
    }
}

/// A play packet decoded with the older version's ids renumbered as they are read.
fn decode_play(ids: &'static Ids, id: i32, payload: &[u8]) -> Result<play::Clientbound> {
    let _up = ids::renumbering(ids, Way::Up);
    play::Clientbound::decode(id, payload)
}

/// The packet's payload once its registry ids are renumbered by decoding it.
fn redecode(ids: &'static Ids, id: i32, payload: &[u8]) -> Result<Vec<u8>> {
    let p = decode_play(ids, id, payload)?;
    Ok(strip_id(p.encode()))
}

fn strip_id(body: Vec<u8>) -> Vec<u8> {
    let mut r = Reader::new(&body);
    match r.varint() {
        Ok(_) => r.rest().to_vec(),
        Err(_) => body,
    }
}

/// Before 1.21 these registries were static. Keep their old ids and provide the names the native
/// client resolves through RegistryData, without pretending to synchronize newer custom definitions.
fn legacy_registries(session: &Session) -> Vec<Vec<u8>> {
    let version = session.version;
    use super::v1_20_6_ids::{ENCHANTMENT_NAMES, PAINTING_VARIANT_NAMES};
    let mut tables: Vec<(&str, &[&str])> = Vec::new();
    if version.at_most(ServerVersion::V1_20_3) {
        use super::v1_20_3_ids as old;
        tables.extend([
            ("minecraft:enchantment", old::ENCHANTMENT_NAMES),
            ("minecraft:painting_variant", old::PAINTING_VARIANT_NAMES),
            ("minecraft:banner_pattern", old::BANNER_PATTERN_NAMES),
            ("minecraft:wolf_variant", &["minecraft:pale"]),
        ]);
    } else if version.at_most(ServerVersion::V1_20_6) {
        tables.extend([
            ("minecraft:enchantment", ENCHANTMENT_NAMES),
            ("minecraft:painting_variant", PAINTING_VARIANT_NAMES),
        ]);
    }
    if version.at_most(ServerVersion::V1_21_1) {
        tables.push(("minecraft:instrument", super::v1_20_3_ids::INSTRUMENT_NAMES));
    }
    tables
        .into_iter()
        .map(|(registry, names)| {
            let mut w = Writer::new();
            let data = configuration::RegistryData {
                registry: registry.into(),
                entries: names
                    .iter()
                    .map(|name| configuration::RegistryEntry {
                        id: (*name).into(),
                        data: None,
                    })
                    .collect(),
            };
            if version.at_most(ServerVersion::V1_20_3) {
                super::legacy::remember(session, &data);
            }
            configuration::RegistryData::write(&data, &mut w);
            w.data
        })
        .collect()
}

fn tags(ids: &Ids, registries: &mut [configuration::RegistryTags]) {
    for registry in registries {
        let remap = ids.by_registry(&registry.registry);
        for tag in &mut registry.tags {
            let mut mapped = Vec::new();
            for &id in &tag.ids {
                if ids.quirks.components_before_1_21_2
                    && registry.registry == "minecraft:entity_type"
                    && matches!(
                        remap.up(id),
                        Some(packet_ids_entity::OAK_BOAT | packet_ids_entity::OAK_CHEST_BOAT)
                    )
                {
                    mapped.extend(if remap.up(id) == Some(packet_ids_entity::OAK_BOAT) {
                        v1_21_1::boats::TYPES
                    } else {
                        v1_21_1::boats::CHEST_TYPES
                    });
                } else if let Some(id) = remap.up(id) {
                    mapped.push(id);
                }
            }
            tag.ids = mapped;
        }
    }
}

/// 26.2's CommonPlayerSpawnInfo, read into Horizon's: the game types went from bytes (-1 for no previous
/// one) to VarInts (an optional one as id + 1).
pub(super) fn spawn_info(r: &mut Reader, version: ServerVersion) -> Result<play::SpawnInfo> {
    let dimension_type = if version.at_most(ServerVersion::V1_20_3) {
        let name = r.string(32767)?;
        ids::legacy_registry_id("minecraft:dimension_type", &name)
            .ok_or_else(|| crate::Error::new("unknown legacy dimension type"))?
    } else {
        r.varint()?
    };
    let dimension = r.string(32767)?;
    let seed = r.i64()?;
    let game_type = i32::from(r.i8()?);
    let previous = r.i8()?;
    let debug = r.bool()?;
    let flat = r.bool()?;
    let last_death = <crate::buf::Opt<play::GlobalPos>>::read(r)?;
    Ok(play::SpawnInfo {
        dimension_type,
        dimension,
        seed,
        game_type,
        previous_game_type: (previous >= 0).then_some(i32::from(previous)),
        debug,
        flat,
        last_death,
        portal_cooldown: r.varint()?,
        // Before the field was synced, ClientLevel inherited Level.getSeaLevel() = 63.
        sea_level: if version.at_most(ServerVersion::V1_21_1) {
            63
        } else {
            r.varint()?
        },
    })
}

fn login(payload: &[u8], version: ServerVersion) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let mut w = Writer::new();
    w.i32(r.i32()?);
    w.bool(r.bool()?);
    let n = r.count(r.remaining())?;
    w.varint(n as i32);
    for _ in 0..n {
        w.string(&r.string(32767)?);
    }
    for _ in 0..3 {
        w.varint(r.varint()?);
    }
    for _ in 0..3 {
        w.bool(r.bool()?);
    }
    play::SpawnInfo::write(&spawn_info(&mut r, version)?, &mut w);
    w.bytes(r.rest());
    // Before 1.20.5 this arrived later in ServerData, after the initial play login.
    if version.at_most(ServerVersion::V1_20_3) {
        w.bool(false);
    }
    Ok(w.data)
}

fn respawn(payload: &[u8], version: ServerVersion) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let mut w = Writer::new();
    play::SpawnInfo::write(&spawn_info(&mut r, version)?, &mut w);
    w.bytes(r.rest());
    Ok(w.data)
}

/// The column's sections with their block states renumbered and packed for the pinned palette width (a
/// section on the older global palette uses fewer bits per entry), and its light masks as 26.3 sends them.
/// Biomes are the server's own registry and stay as they are.
pub(super) fn chunk(session: &Session, ids: &'static Ids, payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let x = r.i32()?;
    let z = r.i32()?;
    let heightmaps = crate::chunk::Heightmaps::read(&mut r)?;
    let buffer = r.byte_array(r.remaining())?;
    let mut block_entities = <crate::buf::List<crate::chunk::BlockEntityInfo>>::read(&mut r)?;
    // A block entity type the pinned version dropped (26.1's bed) goes; the block still draws.
    block_entities.retain_mut(|b| match ids.block_entity_type.up(b.kind) {
        Some(kind) => {
            v1_20_3::upgrade_block_entity(session, b.kind, &mut b.tag);
            b.kind = kind;
            true
        }
        None => false,
    });
    let light = light(&mut r)?;
    let (old_bits, new_bits) = ids.state_bits();
    let mut sections = Reader::new(&buffer);
    let mut out = Vec::new();
    while sections.remaining() > 0 {
        let mut s = Section::read(&mut sections, old_bits, 0)?;
        s.states = renumber_states(ids, &s.states, new_bits);
        out.push(s);
    }
    let mut w = Writer::new();
    play::LevelChunkWithLight::write(
        &play::LevelChunkWithLight {
            x,
            z,
            chunk: crate::chunk::ChunkData {
                heightmaps,
                buffer: crate::chunk::ChunkData::from_sections(&out),
                block_entities,
            },
            light,
        },
        &mut w,
    );
    Ok(w.data)
}

/// 26.2's ClientboundLightUpdatePacketData: its four masks are `BitSet.toLongArray` (a VarInt count of
/// longs); 26.3 sends `toByteArray`, the same bits as little-endian bytes without trailing zeros.
fn light(r: &mut Reader) -> Result<crate::chunk::LightData> {
    let mut mask = || -> Result<Vec<u8>> {
        let longs = r.long_array()?;
        let mut bytes: Vec<u8> = longs.iter().flat_map(|l| l.to_le_bytes()).collect();
        let len = bytes.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
        bytes.truncate(len);
        Ok(bytes)
    };
    let sky_mask = mask()?;
    let block_mask = mask()?;
    let empty_sky_mask = mask()?;
    let empty_block_mask = mask()?;
    Ok(crate::chunk::LightData {
        sky_mask,
        block_mask,
        empty_sky_mask,
        empty_block_mask,
        sky: crate::chunk::Layers::read(r)?,
        block: crate::chunk::Layers::read(r)?,
    })
}

pub(super) fn light_update(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let x = r.varint()?;
    let z = r.varint()?;
    let light = light(&mut r)?;
    let mut w = Writer::new();
    play::LightUpdate::write(&play::LightUpdate { x, z, light }, &mut w);
    Ok(w.data)
}

fn renumber_states(ids: &Ids, states: &Paletted, new_bits: u8) -> Paletted {
    let up = |id: i32| ids.block_state.up(id).unwrap_or(0);
    if !states.palette.is_empty() {
        // A palette: renumbering its entries is enough.
        let mut out = states.clone();
        out.palette.iter_mut().for_each(|p| *p = up(*p));
        return out;
    }
    let values: Vec<i32> = (0..crate::chunk::BLOCKS)
        .map(|i| up(states.get(i)))
        .collect();
    Paletted::from_values(&values, Strategy::Blocks, new_bits)
}

/// 26.2 keeps an advancement's tab position in its display (two floats after the background); 26.3 sends
/// it after the whole advancement, displayed or not. What follows the added advancements is unchanged.
fn advancements(ids: &'static Ids, payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let mut w = Writer::new();
    w.bool(r.bool()?);
    let n = r.count(r.remaining())?;
    w.varint(n as i32);
    for _ in 0..n {
        w.string(&r.string(32767)?);
        let parent = r.bool()?;
        w.bool(parent);
        if parent {
            w.string(&r.string(32767)?);
        }
        let mut position = (0.0, 0.0);
        let display = r.bool()?;
        w.bool(display);
        if display {
            w.nbt(&r.nbt()?);
            w.nbt(&r.nbt()?);
            let icon = {
                let _up = ids::renumbering(ids, Way::Up);
                crate::item::Template::read(&mut r)?
            };
            crate::item::Template::write(&icon, &mut w);
            w.varint(r.varint()?);
            let flags = r.i32()?;
            w.i32(flags);
            if flags & 1 != 0 {
                w.string(&r.string(32767)?);
            }
            position = (r.f32()?, r.f32()?);
        }
        let groups = r.count(r.remaining())?;
        w.varint(groups as i32);
        for _ in 0..groups {
            let k = r.count(r.remaining())?;
            w.varint(k as i32);
            for _ in 0..k {
                w.string(&r.string(32767)?);
            }
        }
        w.bool(r.bool()?);
        w.f32(position.0);
        w.f32(position.1);
    }
    w.bytes(r.rest());
    Ok(w.data)
}

/// 26.2: flags, position, spread, one max speed, an Int count, then the particle. 26.3 sends the particle
/// first, a speed per axis, a VarInt count and how the spread is drawn (0, gaussian, as 26.2 did).
pub(super) fn particles(ids: &'static Ids, payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let override_limiter = r.bool()?;
    let always_show = r.bool()?;
    let pos = [r.f64()?, r.f64()?, r.f64()?];
    let dist = [r.f32()?, r.f32()?, r.f32()?];
    let speed = r.f32()?;
    let count = r.i32()?;
    let particle = {
        let _up = ids::renumbering(ids, Way::Up);
        crate::entity_data::Particle::read(&mut r)?
    };
    let mut w = Writer::new();
    play::LevelParticles::write(
        &play::LevelParticles {
            particle,
            override_limiter,
            always_show,
            pos,
            dist,
            max_speed: [speed; 3],
            count,
            randomization: 0,
        },
        &mut w,
    );
    Ok(w.data)
}

/// 26.2 sends a move as three Short deltas (1/4096 of a block) then on-ground, and a turn as the two angles
/// then on-ground; 26.3 packs on-ground with the delta's step count, and sends it before the angles.
pub(super) fn move_entity(payload: &[u8], pos: bool, rot: bool) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let id = r.varint()?;
    let delta = if pos {
        Some(play::VecDelta::Linear([r.i16()?, r.i16()?, r.i16()?]))
    } else {
        None
    };
    let rotation = if rot { Some((r.i8()?, r.i8()?)) } else { None };
    let on_ground = r.bool()?;
    let m = play::MoveEntity {
        id,
        delta,
        rotation,
        on_ground,
    };
    let mut w = Writer::new();
    match (pos, rot) {
        (true, false) => play::MoveEntityPos::write(&play::MoveEntityPos(m), &mut w),
        (true, true) => play::MoveEntityPosRot::write(&play::MoveEntityPosRot(m), &mut w),
        _ => play::MoveEntityRot::write(&play::MoveEntityRot(m), &mut w),
    }
    Ok(w.data)
}

/// 26.2: the entity, PositionMoveRotation (position, motion, the two angles), on-ground. 26.3 sends where it
/// goes as a path, then the angles; the motion is no longer sent.
pub(super) fn position_sync(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let id = r.varint()?;
    let position = [r.f64()?, r.f64()?, r.f64()?];
    let _motion = [r.f64()?, r.f64()?, r.f64()?];
    let yaw = r.f32()?;
    let pitch = r.f32()?;
    let on_ground = r.bool()?;
    let mut w = Writer::new();
    play::EntityPositionSync::write(
        &play::EntityPositionSync {
            id,
            path: play::PositionPath::Linear(position),
            yaw,
            pitch,
            on_ground,
        },
        &mut w,
    );
    Ok(w.data)
}

/// 26.2's animate swings hands too (0 main, 3 off), which 26.3 sends as `swing_animation`; the others were
/// renumbered (wake up 2 -> 0, critical hits 4, 5 -> 1, 2).
pub(super) fn animate(payload: &[u8]) -> Result<(i32, Vec<u8>)> {
    let mut r = Reader::new(payload);
    let id = r.varint()?;
    let action = r.u8()?;
    let mut w = Writer::new();
    if action == 0 || action == 3 {
        // A plain swing: SwingAnimationType WHACK over the default 6 ticks.
        play::SwingAnimation::write(
            &play::SwingAnimation {
                id,
                hand: i32::from(action == 3),
                kind: 1,
                duration: 6,
            },
            &mut w,
        );
        return Ok((packet_ids::play::clientbound::SWING_ANIMATION, w.data));
    }
    let action = match action {
        2 => 0,
        4 => 1,
        5 => 2,
        other => other,
    };
    w.varint(id);
    w.u8(action);
    Ok((packet_ids::play::clientbound::ANIMATE, w.data))
}

/// 1.21.10 (and 1.21.9) to 1.21.11.
pub(crate) mod v1_21_10 {
    use super::*;

    // 1.21.10's packets are laid out as 1.21.11's but for `horse_screen_open`, which 1.21.11 renamed
    // `mount_screen_open` (the id tables pair them): only entity data needs a step.
    use crate::entity_data::{Value, serializer};

    pub(crate) use crate::adapter::pinned_ids::entity_type::{BEE, MANNEQUIN, PLAYER, WOLF};

    /// 1.21.11 made the bee's and the wolf's anger end a LONG (an INT before) and an avatar's main arm a
    /// HUMANOID_ARM (a BYTE before, with the same 0 left, 1 right). Indexes as 1.21.10 numbers them.
    pub fn entity_data(session: &Session, data: &mut play::SetEntityData) {
        let Some(kind) = session.kinds.lock().unwrap().get(&data.id).copied() else {
            return;
        };
        for (index, ser, value) in &mut data.values {
            match (kind, *index, &*value) {
                (BEE, 18, Value::Int(v)) | (WOLF, 21, Value::Int(v)) => {
                    *value = Value::Long(i64::from(*v));
                    *ser = serializer::LONG;
                }
                (PLAYER | MANNEQUIN, 15, Value::Byte(v)) => {
                    *value = Value::Id(i32::from(*v));
                    *ser = serializer::HUMANOID_ARM;
                }
                _ => {}
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_bee_s_anger_and_a_player_s_arm_take_1_21_11_s_types() {
            let session = Session::new(ServerVersion::V1_21_10);
            session.kinds.lock().unwrap().insert(1, BEE);
            session.kinds.lock().unwrap().insert(2, PLAYER);
            let mut bee = play::SetEntityData {
                id: 1,
                values: vec![(18, serializer::INT, Value::Int(400))],
            };
            entity_data(&session, &mut bee);
            assert_eq!(bee.values, vec![(18, serializer::LONG, Value::Long(400))]);
            let mut player = play::SetEntityData {
                id: 2,
                values: vec![(15, serializer::BYTE, Value::Byte(1))],
            };
            entity_data(&session, &mut player);
            assert_eq!(
                player.values,
                vec![(15, serializer::HUMANOID_ARM, Value::Id(1))]
            );
        }
    }
}

/// 1.21.11 to 26.1.
pub(crate) mod v1_21_11 {
    use super::*;
    use crate::adapter::pinned_ids::entity_type as entity;

    /// 1.21.11's serverbound `swing` and `interact` (play).
    pub const SWING: i32 = 60;
    #[cfg(test)]
    const INTERACT: i32 = 25;

    /// Entity types (26.3 ids) whose class extends AgeableMob: 26.1 added a second boolean to it, after the
    /// baby flag (16), so their later entity data entries moved up one. Read off 26.1.2's EntityType
    /// registrations and class hierarchy.
    #[rustfmt::skip]
    pub(crate) const AGEABLE: [i32; 39] = [
        entity::ARMADILLO, entity::AXOLOTL, entity::BEE, entity::CAMEL, entity::CAMEL_HUSK,
        entity::CAT, entity::CHICKEN, entity::COW, entity::DOLPHIN, entity::DONKEY, entity::FOX,
        entity::FROG, entity::HAPPY_GHAST, entity::GLOW_SQUID, entity::GOAT, entity::HOGLIN,
        entity::HORSE, entity::LLAMA, entity::MOOSHROOM, entity::MULE, entity::NAUTILUS,
        entity::OCELOT, entity::PANDA, entity::PARROT, entity::PIG, entity::POLAR_BEAR,
        entity::RABBIT, entity::SHEEP, entity::SKELETON_HORSE, entity::SNIFFER, entity::SQUID,
        entity::STRIDER, entity::TRADER_LLAMA, entity::TURTLE, entity::VILLAGER,
        entity::WANDERING_TRADER, entity::WOLF, entity::ZOMBIE_HORSE, entity::ZOMBIE_NAUTILUS,
    ];

    /// 1.21.11's packet in 26.1's layout; `None` when it is laid out alike. Ids stay the server's.
    pub fn clientbound(
        session: &Session,
        state: State,
        id: i32,
        payload: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        use packet_ids::play::clientbound as cb;
        Ok(match (state, id) {
            (State::Play, cb::ADD_ENTITY) => {
                let mut r = Reader::new(payload);
                let entity = r.varint()?;
                r.uuid()?;
                let kind = r.varint()?;
                if let Some(ids) = session.version.ids()
                    && let Some(kind) = ids.entity_type.up(kind)
                {
                    session.kinds.lock().unwrap().insert(entity, kind);
                }
                None
            }
            (State::Play, cb::REMOVE_ENTITIES) => {
                let mut r = Reader::new(payload);
                let n = r.count(r.remaining())?;
                let mut kinds = session.kinds.lock().unwrap();
                for _ in 0..n {
                    let entity = r.varint()?;
                    kinds.remove(&entity);
                    session.minecart_display.lock().unwrap().remove(&entity);
                    session.legacy_boats.lock().unwrap().remove(&entity);
                    session.legacy_metadata.lock().unwrap().forget(entity);
                }
                None
            }
            (State::Play, cb::SET_TIME) => Some(set_time(payload)?),
            (State::Play, cb::LEVEL_CHUNK_WITH_LIGHT) => Some(chunk(payload)?),
            _ => None,
        })
    }

    /// 1.21.11's chunk sections hold the non-empty block count, then the states and biomes; 26.1 adds the
    /// fluid count after the block count (Horizon reads it but goes by the states). The rest of the packet
    /// is laid out alike.
    fn chunk(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let x = r.i32()?;
        let z = r.i32()?;
        let heightmaps = crate::chunk::Heightmaps::read(&mut r)?;
        let buffer = r.byte_array(r.remaining())?;
        let mut sections = Reader::new(&buffer);
        let mut out = Writer::new();
        while sections.remaining() > 0 {
            out.i16(sections.i16()?);
            out.i16(0);
            Paletted::read(&mut sections, Strategy::Blocks, 0)?.write(&mut out);
            Paletted::read(&mut sections, Strategy::Biomes, 0)?.write(&mut out);
        }
        let mut w = Writer::new();
        w.i32(x);
        w.i32(z);
        crate::chunk::Heightmaps::write(&heightmaps, &mut w);
        w.byte_array(&out.data);
        w.bytes(r.rest());
        Ok(w.data)
    }

    /// 1.21.11 sends the game time, the day time and whether the day time runs; 26.1 the game time and a
    /// state per world clock. The day time becomes the overworld clock's (id 0 in [`world_clocks`]),
    /// standing still when it doesn't run.
    fn set_time(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let game_time = r.i64()?;
        let day_time = r.i64()?;
        let ticking = r.bool()?;
        let mut w = Writer::new();
        play::SetTime::write(
            &play::SetTime {
                game_time,
                clocks: vec![play::ClockState {
                    clock: 0,
                    total_ticks: day_time,
                    partial_tick: 0.0,
                    rate: if ticking { 1.0 } else { 0.0 },
                }],
            },
            &mut w,
        );
        Ok(w.data)
    }

    /// The `minecraft:world_clock` registry 1.21.11 has none of: the overworld's clock alone.
    pub fn world_clocks() -> Vec<u8> {
        let mut w = Writer::new();
        configuration::RegistryData::write(
            &configuration::RegistryData {
                registry: "minecraft:world_clock".into(),
                entries: vec![configuration::RegistryEntry {
                    id: "minecraft:overworld".into(),
                    data: None,
                }],
            },
            &mut w,
        );
        w.data
    }

    /// Entity data of an AgeableMob moves past the entry 26.1 inserted after the baby flag.
    pub fn entity_data(session: &Session, data: &mut play::SetEntityData) {
        let kind = session.kinds.lock().unwrap().get(&data.id).copied();
        if kind.is_some_and(|k| AGEABLE.contains(&k)) {
            for (index, _, _) in &mut data.values {
                if *index >= 17 {
                    *index += 1;
                }
            }
        }
    }

    /// Horizon's interact (entity, hand, where on it, sneaking) is 1.21.11's INTERACT_AT (action 2: the
    /// point as floats, then the hand), followed as vanilla 1.21.11 did by INTERACT (action 0: the hand).
    pub fn interact(session: &Session, payload: &[u8]) -> Result<Option<(i32, Vec<u8>)>> {
        let Some(interact) = session
            .version
            .serverbound_id(State::Play, packet_ids::play::serverbound::INTERACT)
        else {
            return Ok(None);
        };
        let mut r = Reader::new(payload);
        let entity = r.varint()?;
        let hand = r.varint()?;
        let at = crate::buf::LpVec3::read(&mut r)?;
        let sneaking = r.bool()?;
        let mut first = Writer::new();
        first.varint(entity);
        first.varint(2);
        at.iter().for_each(|v| first.f32(*v as f32));
        first.varint(hand);
        first.bool(sneaking);
        let mut then = Writer::new();
        then.varint(interact);
        then.varint(entity);
        then.varint(0);
        then.varint(hand);
        then.bool(sneaking);
        session.outgoing.lock().unwrap().push(then.data);
        Ok(Some((interact, first.data)))
    }

    /// 26.1's attack is 1.21.11's interact with action 1 (ATTACK).
    pub fn attack(session: &Session, payload: &[u8]) -> Result<Option<(i32, Vec<u8>)>> {
        let Some(interact) = session
            .version
            .serverbound_id(State::Play, packet_ids::play::serverbound::INTERACT)
        else {
            return Ok(None);
        };
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.varint(r.varint()?);
        w.varint(1);
        w.bool(false);
        Ok(Some((interact, w.data)))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_1_21_11_time_becomes_the_overworld_clock() {
            let mut w = Writer::new();
            w.i64(100);
            w.i64(6000);
            w.bool(false);
            let body = set_time(&w.data).unwrap();
            let p =
                play::Clientbound::decode(packet_ids::play::clientbound::SET_TIME, &body).unwrap();
            let play::Clientbound::SetTime(t) = p else {
                panic!("{p:?}")
            };
            assert_eq!(t.game_time, 100);
            assert_eq!((t.clocks[0].clock, t.clocks[0].total_ticks), (0, 6000));
            assert_eq!(t.clocks[0].rate, 0.0); // the day time stands still
        }

        #[test]
        fn an_interaction_is_sent_as_interact_at_then_interact() {
            let session = Session::new(ServerVersion::V1_21_11);
            let mut w = Writer::new();
            w.varint(9); // entity
            w.varint(1); // off hand
            crate::buf::LpVec3::write(&[0.5, 1.0, 0.0], &mut w);
            w.bool(true);
            let (id, first) = interact(&session, &w.data).unwrap().unwrap();
            assert_eq!(id, INTERACT);
            let mut r = Reader::new(&first);
            assert_eq!((r.varint().unwrap(), r.varint().unwrap()), (9, 2)); // INTERACT_AT
            assert!((r.f32().unwrap() - 0.5).abs() < 0.01); // LpVec3 is approximate
            let then = session.take_outgoing();
            // INTERACT (action 0) with the off hand, sneaking.
            assert_eq!(then, vec![vec![INTERACT as u8, 9, 0, 1, 1]]);
        }
    }
}

/// 26.1 to 26.2: what changed in the packets of the release before.
pub(crate) mod v26_1 {
    use super::*;

    /// 26.1's packet in 26.2's layout; `None` when it is laid out alike.
    pub fn clientbound(
        session: &Session,
        state: State,
        id: i32,
        payload: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        use packet_ids::play::clientbound as cb;
        Ok(match (state, id) {
            // 26.2 adds the session's id after the profile; 26.1 has none to give.
            (State::Login, packet_ids::login::clientbound::LOGIN_FINISHED) => {
                let mut body = payload.to_vec();
                body.extend_from_slice(&[0; 16]);
                Some(body)
            }
            (State::Play, cb::LOGIN) => Some(login(
                payload,
                session.version,
                session.login_requires_authentication(),
            )?),
            (State::Play, cb::SET_PLAYER_TEAM) => Some(team(payload)?),
            (State::Play, cb::ADD_ENTITY) => {
                let mut r = Reader::new(payload);
                let entity = r.varint()?;
                let uuid = r.uuid()?;
                session.uuids.lock().unwrap().insert(entity, uuid);
                None
            }
            (State::Play, cb::REMOVE_ENTITIES) => {
                let mut r = Reader::new(payload);
                let n = r.count(r.remaining())?;
                let mut uuids = session.uuids.lock().unwrap();
                for _ in 0..n {
                    uuids.remove(&r.varint()?);
                }
                None
            }
            _ => None,
        })
    }

    /// Before 26.2 the encryption request tells the client whether the server checks accounts.
    fn login(payload: &[u8], version: ServerVersion, authenticates: bool) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        r.i32()?;
        r.bool()?;
        let n = r.count(r.remaining())?;
        for _ in 0..n {
            r.string(32767)?;
        }
        for _ in 0..3 {
            r.varint()?;
        }
        for _ in 0..3 {
            r.bool()?;
        }
        super::spawn_info(&mut r, version)?;
        let at = r.position();
        let mut body = payload[..at].to_vec();
        body.push(u8::from(authenticates));
        body.extend_from_slice(&payload[at..]);
        Ok(body)
    }

    /// 26.1's team parameters: name, options, visibility, collision, colour as a ChatFormatting (RESET for
    /// none), prefix, suffix. 26.2 sends name, prefix, suffix, visibility, collision, an optional
    /// TeamColor (ChatFormatting's first 16, in order) and the options last.
    fn team(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.string(&r.string(32767)?);
        let method = r.i8()?;
        w.i8(method);
        if method == 0 || method == 2 {
            let name = r.nbt()?;
            let options = r.i8()?;
            let visibility = r.varint()?;
            let collision = r.varint()?;
            let color = r.varint()?;
            let prefix = r.nbt()?;
            let suffix = r.nbt()?;
            w.nbt(&name);
            w.nbt(&prefix);
            w.nbt(&suffix);
            w.varint(visibility);
            w.varint(collision);
            <crate::buf::Opt<crate::buf::VarInt>>::write(
                &(0..16).contains(&color).then_some(color),
                &mut w,
            );
            w.i8(options);
        }
        w.bytes(r.rest());
        Ok(w.data)
    }

    /// Horizon's spectator_action names an entity by its network id; 26.1's spectate_entity (at the same
    /// id) by its UUID. Nothing is sent for one the client never saw added, or for no entity.
    pub fn spectate(session: &Session, payload: &[u8]) -> Result<Option<(i32, Vec<u8>)>> {
        let Some(target) = <crate::buf::OptVarInt>::read(&mut Reader::new(payload))? else {
            return Ok(None);
        };
        let Some(uuid) = session.uuids.lock().unwrap().get(&target).copied() else {
            return Ok(None);
        };
        let mut w = Writer::new();
        w.uuid(uuid);
        Ok(Some((SPECTATE_ENTITY, w.data)))
    }

    /// 26.1's serverbound `spectate_entity` (play).
    const SPECTATE_ENTITY: i32 = 62;

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_26_1_team_reads_as_26_2s() {
            let mut w = Writer::new();
            w.string("red");
            w.i8(0);
            w.nbt(&horizon_world::nbt::Tag::String("Red".into()));
            w.i8(3);
            w.varint(1);
            w.varint(2);
            w.varint(12); // RED
            w.nbt(&horizon_world::nbt::Tag::String("[".into()));
            w.nbt(&horizon_world::nbt::Tag::String("]".into()));
            w.varint(1);
            w.string("Steve");
            let body = team(&w.data).unwrap();
            let p =
                play::Clientbound::decode(packet_ids::play::clientbound::SET_PLAYER_TEAM, &body)
                    .unwrap();
            let play::Clientbound::SetPlayerTeam(t) = p else {
                panic!("{p:?}")
            };
            let params = t.parameters.unwrap();
            assert_eq!(params.color, Some(12));
            assert_eq!(params.options, 3);
            assert_eq!(params.name_tag_visibility, 1);
            assert_eq!(params.prefix, horizon_world::nbt::Tag::String("[".into()));
            assert_eq!(t.players, vec!["Steve".to_string()]);
        }

        #[test]
        fn reset_is_no_team_colour() {
            let mut w = Writer::new();
            w.string("t");
            w.i8(2);
            w.nbt(&horizon_world::nbt::Tag::String("T".into()));
            w.i8(0);
            w.varint(0);
            w.varint(0);
            w.varint(21); // RESET
            w.nbt(&horizon_world::nbt::Tag::String("".into()));
            w.nbt(&horizon_world::nbt::Tag::String("".into()));
            let body = team(&w.data).unwrap();
            let p =
                play::Clientbound::decode(packet_ids::play::clientbound::SET_PLAYER_TEAM, &body)
                    .unwrap();
            let play::Clientbound::SetPlayerTeam(t) = p else {
                panic!("{p:?}")
            };
            assert_eq!(t.parameters.unwrap().color, None);
        }
    }
}

/// 1.21.6 to 1.21.8. Clientbound nothing changes for a server: 1.21.6 sends set_equipment without the saddle
/// slot (7), which 1.21.8 added, and entity data is laid out alike (the serializer ids differ only from 1.21.9,
/// see adapter.rs). Serverbound, 1.21.8's `test_instance_block_action` names its test by identifier and 1.21.6
/// by a VarInt id in a registry.
pub(crate) mod v1_21_6 {
    use super::*;

    /// Whether a serverbound `test_instance_block_action` (play) names a test: the position, the action, then
    /// the test's presence flag, which is the only field that differs. A test is an identifier in 26.3 and an
    /// id in 1.21.6, which needs the registry's entries in the server's order; Horizon doesn't keep those.
    // TODO(step3): translate a named test to 1.21.6's VarInt id. Until then a packet that names one is dropped.
    pub(crate) fn names_test(payload: &[u8]) -> Result<bool> {
        let mut r = Reader::new(payload);
        r.i64()?;
        r.varint()?;
        r.bool()
    }
}

/// Adjacent-layout support for reverse-client step fixtures; the older-server reader lives in `translate/v1_21_5.rs`.
/// 1.21.5 to 1.21.6 (protocol 770 to 771). Clientbound nothing changes for a server: 1.21.6 adds packets
/// (waypoints, dialogs), which the id tables drop for a 1.21.5 server, and changes no field, enum or entity data
/// serializer (checked against both Packets pages and both Entity metadata pages). Serverbound, `player_command`
/// lost its sneak press and release (actions 0 and 1) and its other actions are numbered two lower in 1.21.6; the
/// sneak itself is `player_input`'s bit 0x20 in both versions.
///
/// Item components are read in 1.21.5's layout by `item.rs` (`ids::Quirks::components_before_1_21_6`), without
/// a display on `attribute_modifiers` and the shearing fields of `equippable`. Not translated (see the TODOs):
/// 1.21.6's dialog registries, which 1.21.5 lacks.
#[cfg(test)]
pub(crate) mod client_layout_v1_21_5 {
    use super::*;

    /// The pinned `player_command` with 1.21.5's actions: 26.3's leave bed (0) to start flying with an elytra
    /// (6) are 1.21.5's 2 to 8. The jump boost is kept.
    pub fn player_command(
        session: &Session,
        id: i32,
        payload: &[u8],
    ) -> Result<Option<(i32, Vec<u8>)>> {
        let mut r = Reader::new(payload);
        let entity = r.varint()?;
        let action = r.varint()?;
        let jump = r.varint()?;
        if !(0..=6).contains(&action) {
            return Ok(None);
        }
        let mut w = Writer::new();
        w.varint(entity);
        w.varint(action + 2);
        w.varint(jump);
        Ok(session
            .version
            .serverbound_id(State::Play, id)
            .map(|mapped| (mapped, w.data)))
    }

    /// The pinned `player_input` goes as it is (its sneak bit 0x20 is 1.21.5's too). 1.21.5's server also takes a
    /// sneak from `player_command`'s press (0) and release (1), so one of those is queued after each input, as
    /// the bit says: the server sets the same state each time, so repeats do nothing to it.
    // TODO(step3): unverified against 1.21.5's handlePlayerCommand: that its press and release only set the sneak state.
    pub fn player_input(
        session: &Session,
        id: i32,
        payload: &[u8],
    ) -> Result<Option<(i32, Vec<u8>)>> {
        let sneak = Reader::new(payload).u8()? & 0x20 != 0;
        if let Some(command) = session
            .version
            .serverbound_id(State::Play, packet_ids::play::serverbound::PLAYER_COMMAND)
        {
            let mut w = Writer::new();
            w.varint(command);
            w.varint(0); // the player's entity: the vanilla server ignores it
            w.varint(if sneak { 0 } else { 1 });
            w.varint(0);
            session.outgoing.lock().unwrap().push(w.data);
        }
        Ok(session
            .version
            .serverbound_id(State::Play, id)
            .map(|mapped| (mapped, payload.to_vec())))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_sprint_is_1_21_5_s_action_three() {
            let session = Session::new(ServerVersion::V1_21_5);
            let mut w = Writer::new();
            w.varint(9);
            w.varint(1); // start sprinting in 26.3
            w.varint(0);
            let id = packet_ids::play::serverbound::PLAYER_COMMAND;
            let (mapped, body) = player_command(&session, id, &w.data).unwrap().unwrap();
            assert_eq!(
                Some(mapped),
                ServerVersion::V1_21_5.serverbound_id(State::Play, id)
            );
            let mut r = Reader::new(&body);
            assert_eq!((r.varint().unwrap(), r.varint().unwrap()), (9, 3));
        }

        #[test]
        fn a_sneaking_input_queues_a_sneak_press_after_it() {
            let session = Session::new(ServerVersion::V1_21_5);
            let input = packet_ids::play::serverbound::PLAYER_INPUT;
            let (mapped, _) = player_input(&session, input, &[0x21]).unwrap().unwrap();
            assert_eq!(
                Some(mapped),
                ServerVersion::V1_21_5.serverbound_id(State::Play, input)
            );
            let sent = session.take_outgoing();
            let command = ServerVersion::V1_21_5
                .serverbound_id(State::Play, packet_ids::play::serverbound::PLAYER_COMMAND)
                .unwrap();
            let mut r = Reader::new(&sent[0]);
            assert_eq!(r.varint().unwrap(), command);
            r.varint().unwrap(); // the entity
            assert_eq!(r.varint().unwrap(), 0); // press sneak key
        }
    }
}

/// Reverse-client layout helpers and test readers, independent of the older-server session reader.
/// 1.21.4 to 1.21.5 (protocol 769 to 770): the mirror of `client_session::step_1_21_4`. Clientbound, the explosion's
/// player knockback is three doubles in 1.21.5 and a double and two floats in 1.21.4; the player chat starts with a
/// global index (a VarInt) 1.21.4 lacks; the team's name tag visibility and collision rule are VarInt enums where
/// 1.21.4 sends their names; and the advancements end with a show flag. Serverbound, the chat packets end with a
/// checksum byte, and a container click's changed slots and carried item are hashes where 1.21.4 reads full stacks.
/// The packets 1.21.5 added (the test instance ones, the copper golem's data) and add_experience_orb, which it
/// removed, need no step: the id tables drop them.
///
/// Chunks (level chunk, chunk biomes): the heightmaps are NBT long arrays by name up to 1.21.4 and VarInt-typed
/// ones from 1.21.5, and no container's data array has a length from 1.21.5 on (`chunk_up`, `chunks_biomes_up`).
/// Entity data (`remap_entity_data`) moves the indexes of the classes the 1.21.4 wiki lists (its index numbers
/// counted by hierarchy).
///
/// Not translated (see the TODOs): the cooldown, whose item id 1.21.5 sends as an identifier (the item's name is
/// needed: `ids.item` and the registry).
pub(crate) mod client_layout_v1_21_4 {
    use super::*;
    use crate::adapter::pinned_ids::entity_type as entity;
    use crate::entity_data::{Value, serializer};

    /// The clientbound packet `id` a 1.21.4 server sent (`payload` in 1.21.4's layout), in 1.21.5's layout.
    #[cfg(test)]
    pub fn clientbound(
        _session: &Session,
        state: State,
        id: i32,
        payload: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        use packet_ids::play::clientbound as cb;
        if state != State::Play {
            return Ok(None);
        }
        Ok(Some(match id {
            cb::EXPLODE => explode_up(payload)?,
            cb::PLAYER_CHAT => player_chat_up(payload)?,
            cb::SET_PLAYER_TEAM => team_up(payload)?,
            cb::UPDATE_ADVANCEMENTS => advancements_up(payload),
            cb::LEVEL_CHUNK_WITH_LIGHT => chunk_up(payload)?,
            cb::CHUNKS_BIOMES => chunks_biomes_up(payload)?,
            _ => return Ok(None),
        }))
    }

    /// 1.21.4's team enum names, by 1.21.5's VarInt id: name tag visibility, then collision rule.
    pub(crate) const VISIBILITY: [&str; 4] =
        ["always", "never", "hideForOtherTeams", "hideForOwnTeam"];
    pub(crate) const COLLISION: [&str; 4] = ["always", "never", "pushOtherTeams", "pushOwnTeam"];

    /// The name a 1.21.5 team enum id has in 1.21.4, from `table`.
    pub(crate) fn name_of(table: &'static [&'static str], id: i32) -> Result<&'static str> {
        usize::try_from(id)
            .ok()
            .and_then(|i| table.get(i).copied())
            .ok_or_else(|| crate::Error::new(format!("team enum {id} is out of range")))
    }

    /// The 1.21.5 id of a team enum name, from `table`.
    #[cfg(test)]
    fn id_of(table: &[&str], name: &str) -> Result<i32> {
        table
            .iter()
            .position(|n| *n == name)
            .map(|i| i as i32)
            .ok_or_else(|| crate::Error::new(format!("unknown team enum {name}")))
    }

    /// The explosion's player knockback: 1.21.4 writes a double, then two floats; 1.21.5 three doubles.
    #[cfg(test)]
    fn explode_up(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        for _ in 0..3 {
            w.f64(r.f64()?);
        }
        let knockback = r.bool()?;
        w.bool(knockback);
        if knockback {
            w.f64(r.f64()?);
            w.f64(f64::from(r.f32()?));
            w.f64(f64::from(r.f32()?));
        }
        w.bytes(r.rest());
        Ok(w.data)
    }

    /// A player chat with the global index 1.21.5 starts it with.
    // TODO(step3): what 1.21.5's global index counts (the messages of the whole server, vanilla's count). 0 until known.
    #[cfg(test)]
    fn player_chat_up(payload: &[u8]) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        w.varint(0);
        w.bytes(payload);
        Ok(w.data)
    }

    /// A team update with its name tag visibility and collision rule as 1.21.5's VarInt enums. Methods: 0 create
    /// (entities after the suffix), 1 remove, 2 update, 3 add entities, 4 remove entities.
    #[cfg(test)]
    fn team_up(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.string(&r.string(32767)?);
        let method = r.u8()?;
        w.u8(method);
        if method == 0 || method == 2 {
            w.nbt(&r.nbt()?);
            w.u8(r.u8()?);
            w.varint(id_of(&VISIBILITY, &r.string(40)?)?);
            w.varint(id_of(&COLLISION, &r.string(40)?)?);
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

    /// The advancements with 1.21.5's show flag after the removed identifiers, the last field of 1.21.4's.
    // TODO(step3): what 1.21.5's show flag does (the downloaded pages give only its name); false keeps 1.21.4's behaviour.
    #[cfg(test)]
    fn advancements_up(payload: &[u8]) -> Vec<u8> {
        let mut p = payload.to_vec();
        p.push(0);
        p
    }

    /// A serverbound chat packet without the checksum 1.21.5 adds as its last byte.
    #[cfg(test)]
    pub(crate) fn without_checksum(payload: &[u8]) -> Vec<u8> {
        payload[..payload.len().saturating_sub(1)].to_vec()
    }

    /// A container click whose changed slots and carried item 1.21.5 sends as hashes: 1.21.4 reads each as an
    /// empty slot, since a hash names no item.
    // TODO(step3): a 1.21.4 server reads the changed slots only to track what the client shows; an empty one
    // makes it resend the true stack. Check that against 1.21.4's handleContainerClick.
    #[cfg(test)]
    pub(crate) fn container_click(payload: &[u8]) -> Result<Vec<u8>> {
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
            crate::item::HashedSlot::read(&mut r)?;
            w.varint(0);
        }
        crate::item::HashedSlot::read(&mut r)?;
        w.varint(0);
        w.bytes(r.rest());
        Ok(w.data)
    }

    pub(crate) use crate::adapter::pinned_ids::entity_type::{
        CHICKEN, COW, DOLPHIN, EXPERIENCE_ORB, MOOSHROOM, PIG, PIGLIN, STRIDER, TURTLE, WOLF,
    };
    /// Minecart, chest, furnace, hopper, spawner, TNT and command block minecarts: Abstract Minecart's fields.
    pub(crate) const MINECARTS: [i32; 7] = [
        entity::MINECART,
        entity::CHEST_MINECART,
        entity::FURNACE_MINECART,
        entity::HOPPER_MINECART,
        entity::SPAWNER_MINECART,
        entity::TNT_MINECART,
        entity::COMMAND_BLOCK_MINECART,
    ];

    /// The index an entry of `kind` has on the other side: `up` from 1.21.4 to 1.21.5, otherwise back. `None` when
    /// the other release has no such field. Minecarts are not here (see `remap_entity_data`).
    pub(crate) fn retarget(kind: i32, index: u8, up: bool) -> Option<u8> {
        match (kind, up, index) {
            (DOLPHIN, true, 17) | (TURTLE, true, 17 | 20 | 21 | 22) | (STRIDER, true, 19) => None,
            (PIG, true, 17) | (PIG, false, 18) | (PIGLIN, false, 16) => None,
            (EXPERIENCE_ORB, false, 8) | (CHICKEN | COW, false, 17) | (WOLF, false, 23) => None,
            (DOLPHIN | TURTLE | PIG, true, 18) => Some(17),
            (DOLPHIN | TURTLE, true, 19) => Some(18),
            (PIGLIN, true, 16) => Some(17),
            (PIGLIN, true, 17) => Some(18),
            (PIGLIN, true, 18) => Some(19),
            (DOLPHIN | TURTLE | PIG, false, 17) => Some(18),
            (DOLPHIN | TURTLE, false, 18) => Some(19),
            (PIGLIN, false, 17) => Some(16),
            (PIGLIN, false, 18) => Some(17),
            (PIGLIN, false, 19) => Some(18),
            _ => Some(index),
        }
    }

    /// Moves a set of entity data between 1.21.4's layout and 1.21.5's (`up` from 1.21.4), for an entity of `kind`.
    /// Entries the other release lacks are dropped. Serializers are the pinned ones.
    ///
    /// A minecart's custom block was 1.21.4's VarInt at 8, shown only when the flag at 10 is set; 1.21.5 puts its
    /// state (an optional block state) at 11 and its Y at 12, and 8 to 10 are the vehicle's shaking, which is dropped
    /// both ways (1.21.4 has none). A mooshroom's type is a String up to 1.21.4 and a VarInt in 1.21.5.
    pub(crate) fn remap_entity_data(kind: i32, data: &mut play::SetEntityData, up: bool) {
        let show = data
            .values
            .iter()
            .any(|(i, _, v)| *i == 10 && matches!(v, Value::Bool(true)));
        let mut out = Vec::with_capacity(data.values.len());
        for (index, ser, value) in std::mem::take(&mut data.values) {
            if MINECARTS.contains(&kind) {
                match (up, index, value) {
                    (true, 8, Value::Int(state)) if show => {
                        out.push((11, serializer::OPTIONAL_BLOCK_STATE, Value::Id(state)));
                    }
                    (true, 9, Value::Int(y)) => out.push((12, serializer::INT, Value::Int(y))),
                    (true, 8..=10, _) | (false, 8..=10, _) => {}
                    (false, 11, Value::Id(state)) if state != 0 => {
                        out.push((8, serializer::INT, Value::Int(state)));
                        out.push((10, serializer::BOOLEAN, Value::Bool(true)));
                    }
                    (false, 11, _) => {}
                    (false, 12, Value::Int(y)) => out.push((9, serializer::INT, Value::Int(y))),
                    (_, index, value) => out.push((index, ser, value)),
                }
                continue;
            }
            let Some(to) = retarget(kind, index, up) else {
                continue;
            };
            let (ser, value) = match (kind, up, value) {
                (MOOSHROOM, true, Value::String(s)) => {
                    (serializer::INT, Value::Int(i32::from(s == "brown")))
                }
                (MOOSHROOM, false, Value::Int(v)) => (
                    serializer::STRING,
                    Value::String(if v == 1 { "brown" } else { "red" }.to_owned()),
                ),
                (_, _, value) => (ser, value),
            };
            out.push((to, ser, value));
        }
        data.values = out;
    }

    /// Heightmap ids (26.3's, which 1.21.5 sends) and the names 1.21.4 sends them under, in its NBT.
    pub(crate) const HEIGHTMAPS: [(i32, &str); 4] = [
        (1, "WORLD_SURFACE"),
        (3, "OCEAN_FLOOR"),
        (4, "MOTION_BLOCKING"),
        (5, "MOTION_BLOCKING_NO_LEAVES"),
    ];

    /// Whether a container with `bits` per entry sends a palette: blocks 1 to 8 (stored in at least 4 bits),
    /// biomes 1 to 3. Other values are direct, the registry ids themselves.
    pub(crate) fn indirect(bits: u8, blocks: bool) -> bool {
        if blocks {
            (1..=8).contains(&bits)
        } else {
            (1..=3).contains(&bits)
        }
    }

    /// A paletted container as 1.21.4 sends it, its data array's length (a VarInt) left out: 1.21.5's.
    #[cfg(test)]
    fn paletted_up(r: &mut Reader, w: &mut Writer, blocks: bool) -> Result<()> {
        let bits = r.u8()?;
        w.u8(bits);
        if bits == 0 {
            w.varint(r.varint()?);
        } else if indirect(bits, blocks) {
            let n = r.varint()?;
            w.varint(n);
            for _ in 0..n {
                w.varint(r.varint()?);
            }
        }
        let longs = usize::try_from(r.varint()?)
            .map_err(|_| crate::Error::new("negative data array length"))?;
        w.bytes(r.bytes(longs * 8)?);
        Ok(())
    }

    /// The sections of a chunk column (or its biomes only) in 1.21.4's layout to 1.21.5's: the block count stays,
    /// and each container loses its data array's length.
    #[cfg(test)]
    fn sections_up(data: &[u8], biomes_only: bool) -> Result<Vec<u8>> {
        let mut r = Reader::new(data);
        let mut w = Writer::new();
        while r.remaining() > 0 {
            if !biomes_only {
                w.i16(r.i16()?);
                paletted_up(&mut r, &mut w, true)?;
            }
            paletted_up(&mut r, &mut w, false)?;
        }
        Ok(w.data)
    }

    /// The heightmaps: 1.21.4's NBT compound of long arrays by name; 1.21.5's a VarInt count, then a VarInt type and
    /// a long array per heightmap. A name 1.21.5 has no id for is dropped.
    #[cfg(test)]
    fn heightmaps_up(r: &mut Reader, w: &mut Writer) -> Result<()> {
        let horizon_world::nbt::Tag::Compound(map) = r.nbt()? else {
            return Err(crate::Error::new("heightmaps: not a compound"));
        };
        let known: Vec<(i32, Vec<i64>)> = HEIGHTMAPS
            .iter()
            .filter_map(|&(kind, name)| {
                match map.get(&horizon_world::nbt::JavaString::from(name)) {
                    Some(horizon_world::nbt::Tag::LongArray(longs)) => Some((kind, longs.clone())),
                    _ => None,
                }
            })
            .collect();
        w.varint(known.len() as i32);
        for (kind, longs) in known {
            w.varint(kind);
            w.long_array(&longs);
        }
        Ok(())
    }

    /// Level chunk with light, 1.21.4's layout to 1.21.5's (heightmaps and sections); the block entities and the
    /// light are alike.
    #[cfg(test)]
    pub(crate) fn chunk_up(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.i32(r.i32()?);
        w.i32(r.i32()?);
        heightmaps_up(&mut r, &mut w)?;
        let data = r.byte_array(r.remaining())?;
        w.byte_array(&sections_up(&data, false)?);
        w.bytes(r.rest());
        Ok(w.data)
    }

    /// Chunk biomes, 1.21.4's layout to 1.21.5's: the biome containers of the column's sections, lengths left out.
    #[cfg(test)]
    pub(crate) fn chunks_biomes_up(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.i32(r.i32()?);
        w.i32(r.i32()?);
        let data = r.byte_array(r.remaining())?;
        w.byte_array(&sections_up(&data, true)?);
        w.bytes(r.rest());
        Ok(w.data)
    }
}

/// 1.21.8 to 1.21.10: the mirror of `client_session::step_1_21_8`. What 1.21.8 lays out differently:
/// add_entity and set_entity_motion carry their velocity as three shorts (1/8000 of a block per tick) and
/// 1.21.10 as an LpVec3; set_default_spawn_position names the dimension and has a yaw and a pitch where 1.21.8
/// has one angle; player_rotation has a relative flag after each angle; explode lacks the radius, the block
/// count and the block particles; and a player's main hand and displayed skin parts moved into Avatar. The
/// packets 1.21.10 added (the debug ones, code_of_conduct) and the serverbound debug subscription need no step:
/// the id tables drop them.
#[cfg(test)]
pub(crate) mod client_layout_v1_21_8 {
    use super::*;
    use crate::buf::LpVec3;

    /// 1.21.8's velocity unit: the shorts of add_entity and set_entity_motion count 1/8000 of a block per tick.
    pub(crate) const VELOCITY_UNIT: f64 = 8000.0;
    // TODO(step1.21.8): 1.21.8 sends no radius and no block count: 3.0 and 0 are guesses.
    const EXPLODE_RADIUS: f32 = 3.0;

    /// A clientbound packet `id` (pinned) of 1.21.8 with `payload` in 1.21.8's layout, in 1.21.10's; `None` when
    /// laid out alike.
    pub fn clientbound(
        _session: &Session,
        state: State,
        id: i32,
        payload: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        use packet_ids::play::clientbound as cb;
        if state != State::Play {
            return Ok(None);
        }
        Ok(match id {
            cb::ADD_ENTITY => Some(add_entity(payload)?),
            cb::SET_ENTITY_MOTION => Some(set_entity_motion(payload)?),
            cb::SET_DEFAULT_SPAWN_POSITION => Some(spawn_position(payload)?),
            cb::PLAYER_ROTATION => Some(player_rotation(payload)?),
            cb::EXPLODE => Some(explode(payload)?),
            _ => None,
        })
    }

    /// add_entity: the three trailing shorts become an LpVec3 after the position.
    fn add_entity(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.varint(r.varint()?);
        w.uuid(r.uuid()?);
        w.varint(r.varint()?);
        for _ in 0..3 {
            w.f64(r.f64()?);
        }
        let angles = [r.u8()?, r.u8()?, r.u8()?];
        let data = r.varint()?;
        let velocity = [r.i16()?, r.i16()?, r.i16()?].map(|s| f64::from(s) / VELOCITY_UNIT);
        LpVec3::write(&velocity, &mut w);
        w.bytes(&angles);
        w.varint(data);
        Ok(w.data)
    }

    /// set_entity_motion: the three shorts become an LpVec3.
    fn set_entity_motion(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.varint(r.varint()?);
        let velocity = [r.i16()?, r.i16()?, r.i16()?].map(|s| f64::from(s) / VELOCITY_UNIT);
        LpVec3::write(&velocity, &mut w);
        Ok(w.data)
    }

    /// set_default_spawn_position: 1.21.8 does not say which dimension the spawn is in, so the overworld is
    /// named. The one angle is the yaw; the pitch is 0.
    // TODO(step1.21.8): a spawn in another dimension would be sent under the overworld's name.
    fn spawn_position(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let position = r.i64()?;
        let angle = r.f32()?;
        let mut w = Writer::new();
        w.string("minecraft:overworld");
        w.i64(position);
        w.f32(angle);
        w.f32(0.0);
        Ok(w.data)
    }

    /// player_rotation: 1.21.10 adds a relative flag after each angle; 1.21.8 only has absolute angles.
    fn player_rotation(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let yaw = r.f32()?;
        let pitch = r.f32()?;
        let mut w = Writer::new();
        w.f32(yaw);
        w.bool(false);
        w.f32(pitch);
        w.bool(false);
        Ok(w.data)
    }

    /// explode: the centre stays; 1.21.10 puts the radius and the block count after it, and the block particles
    /// (a prefixed array, empty here) after the sound.
    fn explode(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.bytes(r.bytes(24)?);
        w.f32(EXPLODE_RADIUS);
        w.i32(0);
        w.bytes(r.rest());
        w.varint(0);
        Ok(w.data)
    }

    /// Player entity data: 1.21.10's Avatar has the main hand (15) and the displayed skin parts (16), and the
    /// player's additional hearts (17) and score (18) follow them. 1.21.8 had the hearts and the score at 15 and
    /// 16 and the skin parts and the main hand at 17 and 18. The shoulder data (19, 20) is an NBT in 1.21.8,
    /// which is where decoding stops. `session` knows the entity's kind; other kinds are left alone.
    // TODO(step1.21.8): translate the shoulder data into 1.21.10's Optional VarInt instead of dropping it.
    pub fn entity_data(session: &Session, data: &mut play::SetEntityData) {
        let Some(kind) = session.kinds.lock().unwrap().get(&data.id).copied() else {
            return;
        };
        if kind != v1_21_10::PLAYER {
            return;
        }
        for (index, _, _) in &mut data.values {
            *index = match *index {
                15 => 17,
                16 => 18,
                17 => 16,
                18 => 15,
                other => other,
            };
        }
    }
}

/// 1.20.6 to 1.21 (protocol 766 to 767): the mirror of `client_session::step_1_20_6`. Clientbound, from the two
/// Packets pages:
/// - `level_particles` sends the particle (a VarInt) first in 1.20.6 and after the count in 1.21.
/// - `explode`'s sound is a sound event holder in 1.21 (a VarInt, 0 for an inline one, then the location and the
///   range); 1.20.6 sends the inline one alone.
/// - `projectile_power` is one double, the power, in 1.21; 1.20.6 sends the vector.
/// - `merchant_offers`: a trade's costs are trade items in 1.21 (an item id, a count and the components they add,
///   no stack flag), and the second cost is optional. The stacks are read under 1.20.6's ids and components.
///
/// Serverbound, 1.21's `use_item` adds the head's yaw and pitch (see [`use_item`]). The sound packets are the same
/// bytes in both, and the other packets 1.21 adds or drops are left to the id tables.
///
/// Not translated: the entity data serializers (the list of 1.20.6 and 1.21 is not in the wiki pages we have).
pub(crate) mod client_layout_v1_20_6 {
    use super::*;

    /// The bytes of 1.20.6's stack (a Slot, its components read under 1.20.6's ids) at `r`, which moves past them.
    /// A stack with no components is measured alone; the stack codec only reads the ones that have some.
    pub(crate) fn slot<'a>(
        payload: &'a [u8],
        r: &mut Reader<'a>,
        ids: &'static Ids,
    ) -> Result<&'a [u8]> {
        let tail = &payload[r.position()..];
        let mut h = Reader::new(tail);
        let len = if h.varint()? <= 0 {
            h.position()
        } else {
            h.varint()?;
            let added = h.varint()?;
            if h.varint()? == 0 && added == 0 {
                h.position()
            } else {
                let _up = ids::renumbering(ids, Way::Up);
                let mut m = Reader::new(tail);
                crate::item::Slot::read(&mut m)?;
                m.position()
            }
        };
        r.bytes(len)?;
        Ok(&tail[..len])
    }

    /// The bytes of `LEVEL_PARTICLES` between the particle and its data in both layouts: the long-distance flag,
    /// three doubles, four floats and the count.
    pub(crate) const LEVEL_PARTICLES_HEAD: usize = 1 + 24 + 16 + 4;

    /// The clientbound packet `id` a 1.20.6 server sent (`payload` in 1.20.6's layout), in 1.21's layout; `None`
    /// when laid out alike.
    #[cfg(test)]
    pub fn clientbound(
        session: &Session,
        state: State,
        id: i32,
        payload: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        use packet_ids::play::clientbound as cb;
        if state != State::Play {
            return Ok(None);
        }
        Ok(match id {
            cb::LEVEL_PARTICLES => Some(level_particles(payload)?),
            cb::EXPLODE => Some(explode(payload)?),
            cb::PROJECTILE_POWER => Some(projectile_power(payload)?),
            cb::MERCHANT_OFFERS => Some(merchant_offers(session, payload)?),
            _ => None,
        })
    }

    /// level_particles: the particle goes after the count, before its data.
    #[cfg(test)]
    fn level_particles(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let particle = r.varint()?;
        let head = r.bytes(LEVEL_PARTICLES_HEAD)?;
        let mut w = Writer::new();
        w.bytes(head);
        w.varint(particle);
        w.bytes(r.rest());
        Ok(w.data)
    }

    /// explode: the sound is a holder in 1.21, and an inline one (0) here; the two explosion particles have no
    /// data, so they are read as their ids alone.
    #[cfg(test)]
    fn explode(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        r.bytes(24 + 4)?; // the centre and the strength
        let records = r.count(1 << 20)?;
        r.bytes(records * 3)?;
        r.bytes(12)?; // the push the player takes
        r.varint()?; // the block interaction
        r.varint()?; // the small explosion particle's id
        r.varint()?; // the large explosion particle's id
        let mut w = Writer::new();
        w.bytes(r.since(0));
        w.varint(0);
        w.bytes(r.rest());
        Ok(w.data)
    }

    /// projectile_power: 1.21 sends the power's length; 1.20.6 sends the vector it is along.
    // TODO(step3): verify that 1.21's power is the length of 1.20.6's vector (the wiki gives only the field).
    #[cfg(test)]
    fn projectile_power(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let id = r.varint()?;
        let (x, y, z) = (r.f64()?, r.f64()?, r.f64()?);
        let mut w = Writer::new();
        w.varint(id);
        w.f64((x * x + y * y + z * z).sqrt());
        Ok(w.data)
    }

    /// merchant_offers: each cost is a trade item in 1.21 and a stack in 1.20.6; the second is optional. The
    /// output stack, the rest of each offer and the list's end are the same bytes.
    #[cfg(test)]
    fn merchant_offers(session: &Session, payload: &[u8]) -> Result<Vec<u8>> {
        let Some(ids) = session.version.ids() else {
            return Ok(payload.to_vec());
        };
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.varint(r.varint()?);
        let size = r.varint()?;
        w.varint(size);
        for _ in 0..size.max(0) {
            trade_item(slot(payload, &mut r, ids)?, &mut w)?;
            w.bytes(slot(payload, &mut r, ids)?);
            let second = slot(payload, &mut r, ids)?;
            if Reader::new(second).varint()? > 0 {
                w.bool(true);
                trade_item(second, &mut w)?;
            } else {
                w.bool(false);
            }
            // The disabled flag, the uses, the max uses, the xp, the special price, the multiplier and the demand.
            w.bytes(r.bytes(1 + 4 * 4 + 4 + 4)?);
        }
        w.bytes(r.rest());
        Ok(w.data)
    }

    /// A 1.20.6 stack as a 1.21 trade item: its item id, its count and the components it adds. A trade item has no
    /// stack flag, and no removed components: a cost never has any, so one is refused.
    // TODO(step3): a cost with removed components cannot be translated yet; it makes the packet fail.
    #[cfg(test)]
    fn trade_item(raw: &[u8], w: &mut Writer) -> Result<()> {
        let mut r = Reader::new(raw);
        let count = r.varint()?;
        if count <= 0 {
            w.varint(0);
            w.varint(0);
            w.varint(0);
            return Ok(());
        }
        let item = r.varint()?;
        let added = r.varint()?;
        if r.varint()? != 0 {
            return Err(crate::Error::new(
                "merchant cost with removed components is not translated (TODO(step3))",
            ));
        }
        w.varint(item);
        w.varint(count);
        w.varint(added);
        w.bytes(r.rest());
        Ok(())
    }
}

/// 1.21 and 1.21.1 (protocol 767) to 1.21.2 and 1.21.3 (768). Packets not named here are laid out alike.
///
/// Not done (TODO(step1_21_1)): explode (1.21.2 sends the radius, a block count, the knockback as an optional
/// vector and the explosion particles as a list; 1.21.1 sends its records and a block interaction), the recipe
/// packets (recipe_book_add/remove/settings, update_recipes, place_ghost_recipe: the recipe book was reworked and
/// the display carries no 1.21.1 recipe id), and serverbound packets (1.21.1's player_input is steer_vehicle).
#[cfg(test)]
pub(crate) mod client_layout_v1_21_1 {
    use super::*;

    /// The sea level 1.21.2 sends after the portal cooldown; 1.21.1 has none, and the overworld's is 63.
    const SEA_LEVEL: i32 = 63;

    /// A window id 1.21.1 sends as an unsigned byte (`signed`: a signed one, as container_set_slot's) and 1.21.2
    /// as a VarInt; the rest of the packet follows as it is.
    fn window_up(payload: &[u8], signed: bool) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let window = if signed {
            i32::from(r.i8()?)
        } else {
            i32::from(r.u8()?)
        };
        let mut w = Writer::new();
        w.varint(window);
        w.bytes(r.rest());
        Ok(w.data)
    }

    /// 1.21.1 sends the game time and the day time, which is negative while the day time stands still (its
    /// absolute value being the time); 1.21.2 sends the day time's absolute value and whether it runs.
    fn set_time(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let game_time = r.i64()?;
        let day_time = r.i64()?;
        let mut w = Writer::new();
        w.i64(game_time);
        w.i64(day_time.checked_abs().unwrap_or(i64::MAX));
        w.bool(day_time >= 0);
        Ok(w.data)
    }

    /// The login's or respawn's common spawn info: 1.21.2 has the sea level after the portal cooldown, 1.21.1 not.
    /// The login's entity id, hardcore flag, dimension names, the three VarInts and three booleans come first.
    fn spawn_info_up(payload: &[u8], login: bool) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        if login {
            r.i32()?;
            r.bool()?;
            let n = r.count(r.remaining())?;
            for _ in 0..n {
                r.string(32767)?;
            }
            for _ in 0..3 {
                r.varint()?;
            }
            for _ in 0..3 {
                r.bool()?;
            }
        }
        r.varint()?;
        r.string(32767)?;
        r.i64()?;
        r.u8()?;
        r.i8()?;
        r.bool()?;
        r.bool()?;
        <crate::buf::Opt<play::GlobalPos>>::read(&mut r)?;
        r.varint()?;
        let at = r.position();
        let mut w = Writer::new();
        w.bytes(&payload[..at]);
        w.varint(SEA_LEVEL);
        w.bytes(&payload[at..]);
        Ok(w.data)
    }

    /// 1.21.1 has one boolean (long distance) where 1.21.2 has two: the second, always visible, is off.
    fn particles(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let long_distance = r.bool()?;
        let mut w = Writer::new();
        w.bool(long_distance);
        w.bool(false);
        w.bytes(r.rest());
        Ok(w.data)
    }

    /// 1.21.1's angle is a byte of a turn (1/256); 1.21.2 sends degrees as floats.
    fn degrees(angle: u8) -> f32 {
        f32::from(angle) * 360.0 / 256.0
    }

    /// 1.21.1 sends an entity's absolute position, its angles as bytes and on ground. 1.21.2 adds the velocity
    /// (zero: 1.21.1 has none), sends the angles as degrees and the relative flags (none: the position is absolute).
    fn teleport_entity(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let id = r.varint()?;
        let position = [r.f64()?, r.f64()?, r.f64()?];
        let yaw = r.u8()?;
        let pitch = r.u8()?;
        let on_ground = r.bool()?;
        let mut w = Writer::new();
        w.varint(id);
        position.iter().for_each(|v| w.f64(*v));
        (0..3).for_each(|_| w.f64(0.0));
        w.f32(degrees(yaw));
        w.f32(degrees(pitch));
        w.i32(0);
        w.bool(on_ground);
        Ok(w.data)
    }

    /// 1.21.1 sends the position, the rotation, the relative flags as a byte and the teleport id last; 1.21.2 sends
    /// the teleport id first and the velocity (zero) before the rotation, and the flags as an Int. Its low five
    /// bits (X, Y, Z, Y rotation, X rotation) are 1.21.1's.
    fn player_position(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let position = [r.f64()?, r.f64()?, r.f64()?];
        let yaw = r.f32()?;
        let pitch = r.f32()?;
        let flags = r.u8()?;
        let teleport = r.varint()?;
        let mut w = Writer::new();
        w.varint(teleport);
        position.iter().for_each(|v| w.f64(*v));
        (0..3).for_each(|_| w.f64(0.0));
        w.f32(yaw);
        w.f32(pitch);
        w.i32(i32::from(flags));
        Ok(w.data)
    }

    /// 1.21.1's held slot is a byte; 1.21.2's a VarInt.
    fn held_slot(payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let mut w = Writer::new();
        w.varint(i32::from(r.i8()?));
        Ok(w.data)
    }

    /// 1.21.1's packet in 1.21.2's layout (the pinned ids): `None` when laid out alike.
    pub fn clientbound(
        _session: &Session,
        state: State,
        id: i32,
        payload: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        use packet_ids::play::clientbound as cb;
        // 1.20.5 to 1.21.1 end login_finished (then game_profile) with strict error handling, which 1.21.2
        // dropped.
        if (state, id) == (State::Login, packet_ids::login::clientbound::LOGIN_FINISHED) {
            return Ok(payload.split_last().map(|(_, rest)| rest.to_vec()));
        }
        if state != State::Play {
            return Ok(None);
        }
        Ok(Some(match id {
            cb::CONTAINER_CLOSE
            | cb::CONTAINER_SET_CONTENT
            | cb::CONTAINER_SET_DATA
            | cb::MOUNT_SCREEN_OPEN => window_up(payload, false)?,
            cb::CONTAINER_SET_SLOT => window_up(payload, true)?,
            cb::SET_HELD_SLOT => held_slot(payload)?,
            cb::SET_TIME => set_time(payload)?,
            cb::LOGIN => spawn_info_up(payload, true)?,
            cb::RESPAWN => spawn_info_up(payload, false)?,
            cb::LEVEL_PARTICLES => particles(payload)?,
            cb::TELEPORT_ENTITY => teleport_entity(payload)?,
            cb::PLAYER_POSITION => player_position(payload)?,
            _ => return Ok(None),
        }))
    }

    /// 26.3's movement packets end with the flags byte (0x01 on ground); 1.21.1's end with the on-ground boolean.
    pub fn movement(session: &Session, id: i32, payload: &[u8]) -> Result<Option<(i32, Vec<u8>)>> {
        let flags = *payload
            .last()
            .ok_or_else(|| crate::Error::new("a movement packet without its flags"))?;
        let mut out = payload[..payload.len() - 1].to_vec();
        out.push(flags & 1);
        Ok(session
            .version
            .serverbound_id(State::Play, id)
            .map(|mapped| (mapped, out)))
    }

    /// 26.3's key flags as 1.21.1's steering: sideways (positive to the left) and forward as floats of their
    /// signs, the jump bit as 1.21.1's jump, and sneak as its unmount. 1.21.1 reads a sneak from player_command's
    /// press (0) and release (1) too, queued after the input, as 1.21.5's does.
    pub fn player_input(
        session: &Session,
        id: i32,
        payload: &[u8],
    ) -> Result<Option<(i32, Vec<u8>)>> {
        let flags = Reader::new(payload).u8()?;
        let axis =
            |plus: u8, minus: u8| f32::from(flags & plus != 0) - f32::from(flags & minus != 0);
        let sneak = flags & 0x20 != 0;
        let mut w = Writer::new();
        w.f32(axis(0x04, 0x08));
        w.f32(axis(0x01, 0x02));
        let mut keys = 0u8;
        if flags & 0x10 != 0 {
            keys |= 0x01;
        }
        if sneak {
            keys |= 0x02;
        }
        w.u8(keys);
        if let Some(command) = session
            .version
            .serverbound_id(State::Play, packet_ids::play::serverbound::PLAYER_COMMAND)
        {
            let mut c = Writer::new();
            c.varint(command);
            c.varint(0);
            c.varint(if sneak { 0 } else { 1 });
            c.varint(0);
            session.outgoing.lock().unwrap().push(c.data);
        }
        Ok(session
            .version
            .serverbound_id(State::Play, id)
            .map(|mapped| (mapped, w.data)))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_window_id_is_a_varint_from_1_21_2() {
            let mut w = Writer::new();
            w.u8(200);
            w.i16(3);
            let body = window_up(&w.data, false).unwrap();
            let mut r = Reader::new(&body);
            assert_eq!(r.varint().unwrap(), 200);
            assert_eq!(r.i16().unwrap(), 3);
        }

        #[test]
        fn a_negative_day_time_stops_the_clock() {
            let mut w = Writer::new();
            w.i64(100);
            w.i64(-6000);
            let body = set_time(&w.data).unwrap();
            let mut r = Reader::new(&body);
            assert_eq!(
                (r.i64().unwrap(), r.i64().unwrap(), r.bool().unwrap()),
                (100, 6000, false)
            );
        }

        #[test]
        fn a_login_gets_the_overworld_sea_level_after_the_portal_cooldown() {
            let mut w = Writer::new();
            w.i32(7);
            w.bool(false);
            w.varint(1);
            w.string("minecraft:overworld");
            for v in [20, 10, 8] {
                w.varint(v);
            }
            for b in [false, true, false] {
                w.bool(b);
            }
            w.varint(0);
            w.string("minecraft:overworld");
            w.i64(42);
            w.u8(0);
            w.i8(-1);
            w.bool(false);
            w.bool(false);
            w.bool(false);
            w.varint(5);
            w.bool(true);
            let body = spawn_info_up(&w.data, true).unwrap();
            let mut r = Reader::new(&body);
            r.i32().unwrap();
            r.bool().unwrap();
            let n = r.count(r.remaining()).unwrap();
            for _ in 0..n {
                r.string(32767).unwrap();
            }
            for _ in 0..3 {
                r.varint().unwrap();
            }
            for _ in 0..3 {
                r.bool().unwrap();
            }
            let info = spawn_info(&mut r, ServerVersion::V1_21_3).unwrap();
            assert_eq!((info.portal_cooldown, info.sea_level), (5, SEA_LEVEL));
            assert!(r.bool().unwrap());
            assert_eq!(r.remaining(), 0);
        }

        #[test]
        fn a_teleport_s_angles_become_degrees_and_its_velocity_is_zero() {
            let mut w = Writer::new();
            w.varint(9);
            [1.5, 64.0, -2.0].iter().for_each(|v| w.f64(*v));
            w.u8(64);
            w.u8(0);
            w.bool(true);
            let body = teleport_entity(&w.data).unwrap();
            let mut r = Reader::new(&body);
            assert_eq!(r.varint().unwrap(), 9);
            assert_eq!(
                [r.f64().unwrap(), r.f64().unwrap(), r.f64().unwrap()],
                [1.5, 64.0, -2.0]
            );
            assert_eq!(
                [r.f64().unwrap(), r.f64().unwrap(), r.f64().unwrap()],
                [0.0; 3]
            );
            assert_eq!(
                (r.f32().unwrap(), r.f32().unwrap(), r.i32().unwrap()),
                (90.0, 0.0, 0)
            );
            assert!(r.bool().unwrap());
        }

        #[test]
        fn a_movement_flag_byte_is_the_on_ground_boolean_and_steering_its_floats() {
            let session = Session::new(ServerVersion::V1_21_1);
            let mv = packet_ids::play::serverbound::MOVE_PLAYER_STATUS_ONLY;
            let (id, body) = movement(&session, mv, &[1]).unwrap().unwrap();
            assert_eq!((id, body), (mv_1_21_1(&session, mv), vec![1]));
            let input = packet_ids::play::serverbound::PLAYER_INPUT;
            let (_, steer) = player_input(&session, input, &[0x01 | 0x04 | 0x10 | 0x20])
                .unwrap()
                .unwrap();
            let mut r = Reader::new(&steer);
            assert_eq!((r.f32().unwrap(), r.f32().unwrap()), (1.0, 1.0));
            assert_eq!(r.u8().unwrap(), 0x01 | 0x02);
        }

        fn mv_1_21_1(session: &Session, id: i32) -> i32 {
            session.version.serverbound_id(State::Play, id).unwrap()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_1_21_11_unknown_entity_is_dropped_and_vanilla_entities_are_remapped() {
        let session = Session::new(ServerVersion::V1_21_11);
        for (kind, data, expected) in [
            (157, 0, None),
            (0, 0, Some((0, 0))),
            (51, 27, Some((52, 28))), // falling block, with a state whose id changed
        ] {
            let packet = play::Clientbound::AddEntity(play::AddEntity {
                id: 7,
                uuid: 1,
                kind,
                position: [0.0; 3],
                velocity: [0.0; 3],
                pitch: 0,
                yaw: 0,
                head_yaw: 0,
                data,
            });
            let body = strip_id(packet.encode());
            let translated = clientbound(
                &session,
                State::Play,
                packet_ids::play::clientbound::ADD_ENTITY,
                &body,
            )
            .unwrap();
            match expected {
                None => assert!(translated.is_none()),
                Some(expected) => {
                    let (id, body) = translated.unwrap();
                    assert_eq!(id, packet_ids::play::clientbound::ADD_ENTITY);
                    let p = play::Clientbound::decode(id, &body.unwrap()).unwrap();
                    let play::Clientbound::AddEntity(e) = p else {
                        panic!("{p:?}");
                    };
                    assert_eq!((e.kind, e.data), expected);
                    assert_eq!(e.id, 7);
                }
            }
        }
    }

    #[test]
    fn a_1_21_11_unknown_block_entity_update_is_dropped() {
        let session = Session::new(ServerVersion::V1_21_11);
        for (kind, expected) in [(49, None), (1, Some(1))] {
            let packet = play::Clientbound::BlockEntityData(play::BlockEntityData {
                pos: [1, 2, 3],
                kind,
                tag: horizon_world::nbt::Tag::Compound(Default::default()),
            });
            let body = strip_id(packet.encode());
            let translated = clientbound(
                &session,
                State::Play,
                packet_ids::play::clientbound::BLOCK_ENTITY_DATA,
                &body,
            )
            .unwrap();
            match expected {
                None => assert!(translated.is_none()),
                Some(expected) => {
                    let (id, body) = translated.unwrap();
                    assert_eq!(id, packet_ids::play::clientbound::BLOCK_ENTITY_DATA);
                    let p = play::Clientbound::decode(id, &body.unwrap()).unwrap();
                    let play::Clientbound::BlockEntityData(b) = p else {
                        panic!("{p:?}");
                    };
                    assert_eq!(b.kind, expected);
                    assert_eq!(b.pos, [1, 2, 3]);
                }
            }
        }
    }

    #[test]
    fn punches_cross_both_adapters_with_each_releases_swing_id() {
        // Packet IDs are the releases' vanilla serverbound reports, independent of the shared helper.
        for (version, swing) in [
            (ServerVersion::V1_20_2, 50),
            (ServerVersion::V1_20_3, 51),
            (ServerVersion::V1_20_6, 54),
            (ServerVersion::V1_21_1, 54),
            (ServerVersion::V1_21_3, 56),
            (ServerVersion::V1_21_4, 58),
            (ServerVersion::V1_21_5, 59),
            (ServerVersion::V1_21_6, 60),
            (ServerVersion::V1_21_8, 60),
            (ServerVersion::V1_21_10, 60),
            (ServerVersion::V1_21_11, 60),
            (ServerVersion::V26_1, 63),
            (ServerVersion::V26_2, 63),
        ] {
            let session = Session::new(version);
            let punch = packet_ids::play::serverbound::PUNCH;
            assert_eq!(
                serverbound(&session, State::Play, punch, &[]).unwrap(),
                Some((swing, vec![0])),
                "{version:?}",
            );
            // The reverse server adapter deliberately stops at protocol 766.
            if version.at_most(ServerVersion::V1_20_3) {
                continue;
            }
            let reverse = super::super::client_session::ClientSession::new(version);
            for (state, id) in [
                (
                    State::Login,
                    packet_ids::login::serverbound::LOGIN_ACKNOWLEDGED,
                ),
                (
                    State::Configuration,
                    packet_ids::configuration::serverbound::FINISH_CONFIGURATION,
                ),
            ] {
                reverse
                    .serverbound(version.serverbound_id(state, id).unwrap(), Vec::new())
                    .unwrap();
            }
            assert_eq!(reverse.reading(), State::Play);
            assert_eq!(
                reverse.serverbound(swing, vec![0]).unwrap(),
                Some((punch, vec![])),
                "{version:?}"
            );
            assert_eq!(
                reverse.serverbound(swing, vec![1]).unwrap(),
                None,
                "{version:?}"
            );
        }
    }

    fn ids() -> &'static Ids {
        super::super::ServerVersion::V26_2.ids().unwrap()
    }

    #[test]
    fn a_26_2_chunk_section_on_the_global_palette_is_repacked_wider() {
        let ids = ids();
        // Every block a different state from 0..4096: past 8 bits, so the global palette at 15 bits.
        let values: Vec<i32> = (0..4096).map(|i| 13000 + i).collect();
        let old = Paletted::from_values(&values, Strategy::Blocks, 15);
        assert_eq!(old.bits, 15);
        let new = renumber_states(ids, &old, 16);
        assert_eq!(new.bits, 16);
        // oak_slab's first state, 13330 in 26.2, is 15177 in 26.3.
        assert_eq!(new.get(330), 15177);
        assert_eq!(new.get(0), ids.block_state.up(13000).unwrap());
    }

    #[test]
    fn a_26_2_move_reads_as_26_3s() {
        let mut w = Writer::new();
        w.varint(7);
        w.i16(4096);
        w.i16(0);
        w.i16(-2048);
        w.bool(true);
        let body = move_entity(&w.data, true, false).unwrap();
        let p = play::Clientbound::decode(packet_ids::play::clientbound::MOVE_ENTITY_POS, &body)
            .unwrap();
        assert_eq!(
            p,
            play::Clientbound::MoveEntityPos(play::MoveEntityPos(play::MoveEntity {
                id: 7,
                delta: Some(play::VecDelta::Linear([4096, 0, -2048])),
                rotation: None,
                on_ground: true,
            }))
        );
        let mut w = Writer::new();
        w.varint(7);
        w.i8(10);
        w.i8(-20);
        w.bool(false);
        let body = move_entity(&w.data, false, true).unwrap();
        let p = play::Clientbound::decode(packet_ids::play::clientbound::MOVE_ENTITY_ROT, &body)
            .unwrap();
        assert_eq!(
            p,
            play::Clientbound::MoveEntityRot(play::MoveEntityRot(play::MoveEntity {
                id: 7,
                delta: None,
                rotation: Some((10, -20)),
                on_ground: false,
            }))
        );
    }

    #[test]
    fn a_26_2_swing_becomes_a_swing_animation() {
        let (id, body) = animate(&[5, 3]).unwrap();
        assert_eq!(id, packet_ids::play::clientbound::SWING_ANIMATION);
        let p = play::Clientbound::decode(id, &body).unwrap();
        assert_eq!(
            p,
            play::Clientbound::SwingAnimation(play::SwingAnimation {
                id: 5,
                hand: 1,
                kind: 1,
                duration: 6
            })
        );
        assert_eq!(
            animate(&[5, 4]).unwrap(),
            (packet_ids::play::clientbound::ANIMATE, vec![5, 1])
        );
    }

    #[test]
    fn falling_blocks_are_known_in_both_numberings() {
        let ids = ids();
        assert_eq!(
            ids.entity_type
                .down(packet_ids_entity::FALLING_BLOCK)
                .and_then(|o| ids.entity_type.up(o)),
            Some(packet_ids_entity::FALLING_BLOCK)
        );
    }

    #[test]
    fn a_26_2_light_mask_of_longs_becomes_26_3s_bytes() {
        let mut w = Writer::new();
        w.varint(3);
        w.varint(-2);
        // Sky mask: sections 0 and 2 (one long, 0b101); block mask: section 9 (bit 9); both empty masks none.
        for mask in [&[0b101_i64][..], &[1 << 9], &[], &[]] {
            w.varint(mask.len() as i32);
            mask.iter().for_each(|&l| w.i64(l));
        }
        w.varint(0);
        w.varint(0);
        let body = light_update(&w.data).unwrap();
        let p =
            play::Clientbound::decode(packet_ids::play::clientbound::LIGHT_UPDATE, &body).unwrap();
        let play::Clientbound::LightUpdate(l) = p else {
            panic!("{p:?}");
        };
        assert_eq!((l.x, l.z), (3, -2));
        assert_eq!(l.light.sky_mask, vec![0b101]);
        assert_eq!(l.light.block_mask, vec![0, 0b10]);
        assert!(l.light.empty_sky_mask.is_empty());
    }

    #[test]
    fn a_26_3_destroy_direction_change_is_not_sent_to_26_2() {
        let session = Session::new(super::super::ServerVersion::V26_2);
        let action = |n: i32| {
            let mut w = Writer::new();
            w.varint(n);
            w.i64(0);
            w.u8(1);
            serverbound(
                &session,
                State::Play,
                packet_ids::play::serverbound::PLAYER_ACTION,
                &w.data,
            )
            .unwrap()
            .map(|(_, payload)| payload[0])
        };
        assert_eq!(action(0), Some(0)); // start destroying
        assert_eq!(action(1), None); // change_destroy_direction, new in 26.3
        assert_eq!(action(3), Some(2)); // stop destroying: 3 in 26.3, 2 in 26.2
        assert_eq!(action(8), None); // stab, new in 26.3
    }
}

#[cfg(test)]
mod legacy_registry_tests {
    use super::*;

    #[test]
    fn vanilla_765_login_and_respawn_resolve_the_synced_dimension_type() {
        use horizon_world::nbt::Tag;
        let session = Session::new(ServerVersion::V1_20_3);
        let mut handshake = Writer::new();
        handshake.varint(packet_ids::handshake::serverbound::INTENTION);
        handshake.varint(crate::PROTOCOL_VERSION);
        handshake.string("horizon.invalid");
        handshake.u16(25565);
        handshake.varint(2);
        session.serverbound(&handshake.data).unwrap();
        // Actual vanilla 1.20.3 and 1.20.4 GameProfile packet writers produce these bytes.
        let bytes = |hex: &str| {
            (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect::<Vec<_>>()
        };
        session
            .clientbound(
                2,
                bytes("000000000000000000000000000000010b486f72697a6f6e5769726500"),
            )
            .unwrap();
        let entry = |name: &str, id| {
            Tag::compound([
                ("name", Tag::String(name.into())),
                ("id", Tag::Int(id)),
                ("element", Tag::compound([] as [(&str, Tag); 0])),
            ])
        };
        let registry = Tag::compound([(
            "minecraft:dimension_type",
            Tag::compound([
                ("type", Tag::String("minecraft:dimension_type".into())),
                (
                    "value",
                    Tag::List(vec![
                        entry("horizon_wire:custom_type", 1),
                        entry("minecraft:overworld", 0),
                    ]),
                ),
            ]),
        )]);
        let mut config = Writer::new();
        config.nbt(&registry);
        session.clientbound(5, config.data).unwrap();
        session.clientbound(2, vec![]).unwrap();
        while session.next_incoming().is_some() {}
        assert_eq!(session.reading(), State::Play);

        // Independently encoded by each official release's full packet writer, including the
        // dimension-type identifier, previous mode, last-death position and portal cooldown.
        let old_login = bytes(
            "0000002a0003136d696e6563726166743a6f766572776f726c64146d696e6563726166743a7468655f6e657468657213686f72697a6f6e5f776972653a637573746f6d14080600010118686f72697a6f6e5f776972653a637573746f6d5f7479706513686f72697a6f6e5f776972653a637573746f6d11223344556677880103000101146d696e6563726166743a7468655f6e65746865720000007fffffd04007",
        );
        let (id, body) = session.clientbound(41, old_login).unwrap().unwrap();
        let play::Clientbound::Login(login) = play::Clientbound::decode(id, &body).unwrap() else {
            panic!("login")
        };
        assert_eq!(login.player_id, 42);
        assert_eq!(
            login.levels,
            [
                "minecraft:overworld",
                "minecraft:the_nether",
                "horizon_wire:custom"
            ]
        );
        assert_eq!(
            (
                login.max_players,
                login.chunk_radius,
                login.simulation_distance
            ),
            (20, 8, 6)
        );
        assert!(login.show_death_screen && login.limited_crafting);
        assert!(!login.online_mode && !login.enforces_secure_chat);
        assert_eq!(login.spawn.dimension_type, 1);
        assert_eq!(login.spawn.dimension, "horizon_wire:custom");
        assert_eq!(login.spawn.seed, 0x1122334455667788);
        assert_eq!(
            (login.spawn.game_type, login.spawn.previous_game_type),
            (1, Some(3))
        );
        assert!(!login.spawn.debug && login.spawn.flat);
        let death = login.spawn.last_death.as_ref().unwrap();
        assert_eq!(death.dimension, "minecraft:the_nether");
        assert_eq!(death.pos, [1, 64, -3]);
        assert_eq!(
            (login.spawn.portal_cooldown, login.spawn.sea_level),
            (7, 63)
        );
        assert_eq!(*session.local_player.lock().unwrap(), Some((42, false)));
        assert_eq!(*session.dimension.lock().unwrap(), "horizon_wire:custom");

        let old_respawn = bytes(
            "18686f72697a6f6e5f776972653a637573746f6d5f7479706513686f72697a6f6e5f776972653a637573746f6d11223344556677880103000101146d696e6563726166743a7468655f6e65746865720000007fffffd0400703",
        );
        let (id, body) = session.clientbound(69, old_respawn).unwrap().unwrap();
        let play::Clientbound::Respawn(respawn) = play::Clientbound::decode(id, &body).unwrap()
        else {
            panic!("respawn")
        };
        assert_eq!(respawn.spawn, login.spawn);
        assert_eq!(respawn.keep, 3);
    }

    #[test]
    fn static_registries_arrive_before_clocks_and_configuration_finishes() {
        let session = Session::new(ServerVersion::V1_20_6);
        let finish = packet_ids::configuration::clientbound::FINISH_CONFIGURATION;
        let registry = packet_ids::configuration::clientbound::REGISTRY_DATA;
        let (id, body) = clientbound(&session, State::Configuration, finish, &[])
            .unwrap()
            .unwrap();
        assert_eq!(id, registry);
        let enchantments =
            configuration::RegistryData::read(&mut Reader::new(&body.unwrap())).unwrap();
        assert_eq!(enchantments.registry, "minecraft:enchantment");
        assert_eq!(enchantments.entries.len(), 42);
        assert_eq!(enchantments.entries[0].id, "minecraft:protection");
        assert_eq!(enchantments.entries[13].id, "minecraft:sharpness");
        assert_eq!(enchantments.entries[40].id, "minecraft:mending");
        assert!(
            enchantments
                .entries
                .iter()
                .all(|entry| entry.data.is_none())
        );
        let (id, body) = session.next_incoming().unwrap();
        assert_eq!(id, registry);
        let paintings = configuration::RegistryData::read(&mut Reader::new(&body)).unwrap();
        assert_eq!(paintings.registry, "minecraft:painting_variant");
        assert_eq!(paintings.entries.len(), 30);
        assert_eq!(paintings.entries[0].id, "minecraft:kebab");
        assert_eq!(paintings.entries[29].id, "minecraft:donkey_kong");
        let (id, body) = session.next_incoming().unwrap();
        assert_eq!(id, registry);
        let instruments = configuration::RegistryData::read(&mut Reader::new(&body)).unwrap();
        assert_eq!(instruments.registry, "minecraft:instrument");
        assert_eq!(instruments.entries.len(), 8);
        assert_eq!(instruments.entries[7].id, "minecraft:dream_goat_horn");
        let (id, body) = session.next_incoming().unwrap();
        assert_eq!(id, registry);
        assert_eq!(
            configuration::RegistryData::read(&mut Reader::new(&body))
                .unwrap()
                .registry,
            "minecraft:world_clock"
        );
        assert_eq!(session.next_incoming(), Some((finish, vec![])));
        assert!(session.next_incoming().is_none());
    }

    #[test]
    fn item_cooldowns_use_the_target_registry_after_the_disc_insertions() {
        let session = Session::new(ServerVersion::V1_20_6);
        // Vanilla 1.20.5/1.20.6: trial_key is item 1325, versus 1328 in 1.21.
        let mut old = Writer::new();
        old.varint(1325);
        old.varint(17);
        let (_, body) = clientbound(
            &session,
            State::Play,
            packet_ids::play::clientbound::COOLDOWN,
            &old.data,
        )
        .unwrap()
        .unwrap();
        let body = body.unwrap();
        let mut r = Reader::new(&body);
        assert_eq!(r.string(32767).unwrap(), "minecraft:trial_key");
        assert_eq!(r.varint().unwrap(), 17);
        assert_eq!(r.remaining(), 0);
    }
}

/// 26.2's readers, for the tests of the layout written for a 26.2 client (`layout_26_2`).
#[cfg(test)]
pub(super) mod tests_support {
    use super::*;

    pub fn login_26_2(payload: &[u8]) -> Result<Vec<u8>> {
        login(payload, ServerVersion::V26_2)
    }
    pub fn respawn_26_2(payload: &[u8]) -> Result<Vec<u8>> {
        respawn(payload, ServerVersion::V26_2)
    }
    pub fn advancements_26_2(ids: &'static Ids, payload: &[u8]) -> Result<Vec<u8>> {
        advancements(ids, payload)
    }
}

#[cfg(test)]
mod sign_update_tests {
    use super::*;

    #[test]
    fn an_older_server_reads_the_side_before_the_lines() {
        let mut w = Writer::new();
        w.i64(0x0102_0304_0506_0708);
        for line in ["Hi", "", "x", ""] {
            w.string(line);
        }
        w.varint(1); // the front
        let out = sign_update(&w.data).unwrap();
        let mut expected = Writer::new();
        expected.i64(0x0102_0304_0506_0708);
        expected.bool(true);
        for line in ["Hi", "", "x", ""] {
            expected.string(line);
        }
        assert_eq!(out, expected.data);
        w.data.pop();
        w.varint(0); // the back
        assert_eq!(sign_update(&w.data).unwrap()[8], 0);
    }
}
