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

pub(super) mod v1_20_3;
mod v1_20_5;
pub(super) mod v1_21_1;
mod v1_21_3;
mod v1_21_4;
mod v1_21_5;

pub(super) fn old_experience_orb(session: &Session, payload: &[u8]) -> Result<(i32, Vec<u8>)> {
    v1_21_4::experience_orb(session, payload)
}
mod v1_21_8;

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
    let steps: [(ServerVersion, Step); 9] = [
        (ServerVersion::V1_20_3, v1_20_3::clientbound),
        (ServerVersion::V1_20_5, v1_20_5::clientbound),
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
        (State::Play, cb::LEVEL_CHUNK_WITH_LIGHT) => chunk(ids, payload)?,
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
                e.kind = ids.entity_type.up(e.kind).unwrap_or(e.kind);
                if session.version.at_most(ServerVersion::V1_21_1) {
                    v1_21_1::boats::spawn(session, ids.entity_type.down(e.kind), e);
                }
            }
            strip_id(p.encode())
        }
        (State::Play, cb::BLOCK_ENTITY_DATA) => {
            let mut p = decode_play(ids, id, payload)?;
            if let play::Clientbound::BlockEntityData(b) = &mut p {
                b.kind = ids.block_entity_type.up(b.kind).unwrap_or(b.kind);
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
        _ => return Ok(Some((id, before))),
    };
    Ok(Some((id, Some(out))))
}

/// Pinned entity type ids the translation looks for.
mod packet_ids_entity {
    /// `minecraft:falling_block` in 26.3.
    pub const FALLING_BLOCK: i32 = 52;
    pub const PLAYER: i32 = 159;
}

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
    match (state, id) {
        (State::Play, sb::CONTAINER_BUTTON_CLICK)
            if session.version.at_most(ServerVersion::V1_20_3) =>
        {
            renumbered(id, v1_20_3::container_button(payload)?)
        }
        (State::Play, sb::USE_ITEM) if session.version.at_most(ServerVersion::V1_20_5) => {
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
                1 => return Ok(None),
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
        // 26.1 is told the entity a spectator picked by its UUID (spectate_entity, at the same id).
        (State::Play, sb::SPECTATOR_ACTION) if session.version == ServerVersion::V26_1 => {
            v26_1::spectate(session, payload)
        }
        // 26.3's punch took over 26.2's swing of the main hand.
        (State::Play, sb::PUNCH) => {
            let mut w = Writer::new();
            w.varint(0);
            let swing = if session.version.at_most(ServerVersion::V1_20_3) {
                51
            } else if session.version.at_most(ServerVersion::V1_21_1) {
                54 // Vanilla 1.21/1.21.1, before bundle selection and client tick-end.
            } else if session.version.at_most(ServerVersion::V1_21_3) {
                56 // Vanilla 1.21.2/1.21.3, before block/entity picking and player_loaded.
            } else if session.version.at_most(ServerVersion::V1_21_4) {
                58 // Vanilla 1.21.4, before the two 1.21.5 test-block packets.
            } else if session.version.at_most(ServerVersion::V1_21_5) {
                59 // Vanilla 1.21.5's swing, before change_game_mode shifted the later ids.
            } else if session.version.at_most(ServerVersion::V1_21_11) {
                v1_21_11::SWING
            } else {
                V26_2_SWING
            };
            Ok(Some((swing, w.data)))
        }
        (State::Play, sb::SET_CREATIVE_MODE_SLOT | sb::CONTAINER_CLICK) => {
            let p = play::Serverbound::decode(id, payload)?;
            let payload = {
                let _down = ids::renumbering(ids, Way::Down);
                strip_id(p.encode())
            };
            renumbered(id, payload)
        }
        _ => renumbered(id, payload.to_vec()),
    }
}

/// 26.2's serverbound `swing` (play), which 26.3 replaced with `punch`.
const V26_2_SWING: i32 = 63;

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
    use super::v1_20_5_ids::{ENCHANTMENT_NAMES, PAINTING_VARIANT_NAMES};
    let mut tables: Vec<(&str, &[&str])> = Vec::new();
    if version.at_most(ServerVersion::V1_20_3) {
        use super::v1_20_3_ids as old;
        tables.extend([
            ("minecraft:enchantment", old::ENCHANTMENT_NAMES),
            ("minecraft:painting_variant", old::PAINTING_VARIANT_NAMES),
            ("minecraft:banner_pattern", old::BANNER_PATTERN_NAMES),
            ("minecraft:wolf_variant", &["minecraft:pale"]),
        ]);
    } else if version.at_most(ServerVersion::V1_20_5) {
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
                    && matches!(remap.up(id), Some(90 | 91))
                {
                    mapped.extend(if remap.up(id) == Some(90) {
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
fn spawn_info(r: &mut Reader, version: ServerVersion) -> Result<play::SpawnInfo> {
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
fn chunk(ids: &'static Ids, payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let x = r.i32()?;
    let z = r.i32()?;
    let heightmaps = crate::chunk::Heightmaps::read(&mut r)?;
    let buffer = r.byte_array(r.remaining())?;
    let mut block_entities = <crate::buf::List<crate::chunk::BlockEntityInfo>>::read(&mut r)?;
    // A block entity type the pinned version dropped (26.1's bed) goes; the block still draws.
    block_entities.retain_mut(|b| match ids.block_entity_type.up(b.kind) {
        Some(kind) => {
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

fn light_update(payload: &[u8]) -> Result<Vec<u8>> {
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
fn particles(ids: &'static Ids, payload: &[u8]) -> Result<Vec<u8>> {
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
fn move_entity(payload: &[u8], pos: bool, rot: bool) -> Result<Vec<u8>> {
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
fn position_sync(payload: &[u8]) -> Result<Vec<u8>> {
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
fn animate(payload: &[u8]) -> Result<(i32, Vec<u8>)> {
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
mod v1_21_10 {
    use super::*;

    // 1.21.10's packets are laid out as 1.21.11's but for `horse_screen_open`, which 1.21.11 renamed
    // `mount_screen_open` (the id tables pair them): only entity data needs a step.
    use crate::entity_data::{Value, serializer};

    /// Entity types (26.3 ids) whose entity data 1.21.11 changed.
    const BEE: i32 = 11;
    const WOLF: i32 = 152;
    const MANNEQUIN: i32 = 84;
    const PLAYER: i32 = 159;

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
mod v1_21_11 {
    use super::*;

    /// 1.21.11's serverbound `swing` and `interact` (play).
    pub const SWING: i32 = 60;
    #[cfg(test)]
    const INTERACT: i32 = 25;

    /// Entity types (26.3 ids) whose class extends AgeableMob: 26.1 added a second boolean to it, after the
    /// baby flag (16), so their later entity data entries moved up one. Read off 26.1.2's EntityType
    /// registrations and class hierarchy.
    const AGEABLE: [i32; 39] = [
        4, 7, 11, 19, 20, 21, 26, 30, 36, 37, 55, 56, 59, 62, 63, 65, 67, 79, 87, 88, 89, 92, 97,
        99, 101, 105, 111, 114, 119, 122, 130, 132, 138, 141, 143, 145, 152, 155, 156,
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
mod v26_1 {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let session = Session::new(ServerVersion::V1_20_5);
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
        let session = Session::new(ServerVersion::V1_20_5);
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
