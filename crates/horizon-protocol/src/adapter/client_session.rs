//! One connection from a client of an older release to a Horizon server: [`super::Session`] run the other
//! way (roadmap `older-clients`). What the client sends is renumbered to the pinned ids before the server
//! reads it; what the server sends is renumbered to the client's, registry ids included, with a stand-in
//! ([`ids::Substitutes`]) for a block, item, entity, particle or sound the client's version lacks.
//!
//! This layer maps ids only: a packet whose layout changed between the client's release and the pinned one
//! passes in the pinned layout (the mirror of `translate`'s readers comes next).
//!
//! The states change where vanilla's server-side `Connection` switches its codecs, after the handshake the
//! server read itself: `login_acknowledged` and `login_finished` move each direction to configuration,
//! `finish_configuration` to play, and `configuration_acknowledged` / `start_configuration` back.
use std::sync::atomic::{AtomicU8, Ordering};

use super::{
    ServerVersion,
    ids::{self, Ids, Way, down_or_substitute},
    translate::packet_ids_entity::FALLING_BLOCK,
};
use crate::{
    Result, State,
    buf::{Reader, Writer},
    entity_data::{ParticleData, Value, serializer},
    ids as packet_ids,
    packets::{configuration, play},
};

fn to_u8(state: State) -> u8 {
    match state {
        State::Handshake => 0,
        State::Status => 1,
        State::Login => 2,
        State::Configuration => 3,
        State::Play => 4,
    }
}
fn from_u8(v: u8) -> State {
    match v {
        1 => State::Status,
        2 => State::Login,
        3 => State::Configuration,
        4 => State::Play,
        _ => State::Handshake,
    }
}

/// Shared by a connection's reader and writer.
pub struct ClientSession {
    pub version: ServerVersion,
    /// What the client sends.
    reading: AtomicU8,
    /// What the server sends.
    writing: AtomicU8,
}

impl ClientSession {
    /// A client whose login handshake stated `version`'s protocol: both directions are in login.
    pub fn new(version: ServerVersion) -> Self {
        Self {
            version,
            reading: AtomicU8::new(to_u8(State::Login)),
            writing: AtomicU8::new(to_u8(State::Login)),
        }
    }
    pub fn reading(&self) -> State {
        from_u8(self.reading.load(Ordering::Acquire))
    }
    pub fn writing(&self) -> State {
        from_u8(self.writing.load(Ordering::Acquire))
    }

    /// A packet the client sent, with the id the pinned version knows it by. `None` drops it: the pinned
    /// version has no such packet (26.2's `swing`).
    pub fn serverbound(&self, id: i32, payload: Vec<u8>) -> Result<Option<(i32, Vec<u8>)>> {
        let state = self.reading();
        let Some(pinned) = self.version.serverbound_id_up(state, id) else {
            return Ok(None);
        };
        use packet_ids::{configuration::serverbound as cfg, login::serverbound as login};
        let next = match (state, pinned) {
            (State::Login, login::LOGIN_ACKNOWLEDGED) => State::Configuration,
            (State::Configuration, cfg::FINISH_CONFIGURATION) => State::Play,
            (State::Play, packet_ids::play::serverbound::CONFIGURATION_ACKNOWLEDGED) => {
                State::Configuration
            }
            _ => state,
        };
        self.reading.store(to_u8(next), Ordering::Release);
        let Some(ids) = self.version.ids() else {
            return Ok(Some((pinned, payload)));
        };
        use packet_ids::play::serverbound as sb;
        let payload = match (state, pinned) {
            // The stacks a client names carry its item and component ids.
            (State::Play, sb::SET_CREATIVE_MODE_SLOT | sb::CONTAINER_CLICK) => {
                let p = {
                    let _up = ids::renumbering(ids, Way::Up);
                    play::Serverbound::decode(pinned, &payload)?
                };
                strip_id(p.encode())
            }
            _ => payload,
        };
        Ok(Some((pinned, payload)))
    }

    /// A packet body (VarInt id + payload) the server sends, as the client numbers it. `None`: the client
    /// has no such packet, or nothing it could show for what the packet names, so nothing is sent.
    pub fn clientbound(&self, body: &[u8]) -> Result<Option<Vec<u8>>> {
        let state = self.writing();
        let mut r = Reader::new(body);
        let id = r.varint()?;
        let payload = r.rest();
        use packet_ids::{configuration::clientbound as cfg, login::clientbound as login};
        let next = match (state, id) {
            (State::Login, login::LOGIN_FINISHED) => State::Configuration,
            (State::Configuration, cfg::FINISH_CONFIGURATION) => State::Play,
            (State::Play, packet_ids::play::clientbound::START_CONFIGURATION) => {
                State::Configuration
            }
            _ => state,
        };
        self.writing.store(to_u8(next), Ordering::Release);
        let Some(mapped) = self.version.clientbound_id_down(state, id) else {
            return Ok(None);
        };
        // A packet the renumbering can't read goes as it is, rather than the connection dropping here.
        let rewritten = match self.version.ids().map(|ids| down(ids, state, id, payload)) {
            None | Some(Ok(Sent::AsIs)) => None,
            Some(Ok(Sent::Rewritten(payload))) => Some(payload),
            Some(Ok(Sent::Dropped)) => return Ok(None),
            Some(Err(e)) => {
                log::warn!(
                    "Horizon: could not renumber packet {id} ({state:?}) for a {} client: {e}",
                    self.version.name()
                );
                None
            }
        };
        let mut w = Writer::new();
        w.varint(mapped);
        w.bytes(rewritten.as_deref().unwrap_or(payload));
        Ok(Some(w.data))
    }
}

/// What becomes of a clientbound payload on its way to an older client.
enum Sent {
    AsIs,
    Rewritten(Vec<u8>),
    /// It names something the client has nothing to show for.
    Dropped,
}

fn strip_id(body: Vec<u8>) -> Vec<u8> {
    let mut r = Reader::new(&body);
    match r.varint() {
        Ok(_) => r.rest().to_vec(),
        Err(_) => body,
    }
}

/// A block state as the older client numbers it, or its stand-in's; air when it has neither.
fn state_down(ids: &Ids, id: i32) -> i32 {
    down_or_substitute(ids.block_state, ids.substitutes.block_state, id).unwrap_or(0)
}

/// A pinned play packet encoded for the older client: items and components in its numbering.
fn encode_down(ids: &'static Ids, p: &play::Clientbound) -> Vec<u8> {
    let _down = ids::renumbering(ids, Way::Down);
    strip_id(p.encode())
}

/// The registry ids in a pinned clientbound packet, renumbered for the older client.
fn down(ids: &'static Ids, state: State, id: i32, payload: &[u8]) -> Result<Sent> {
    use packet_ids::play::clientbound as cb;
    let tags = |registries: &mut [configuration::RegistryTags]| {
        for registry in registries {
            let remap = ids.by_registry(&registry.registry);
            for tag in &mut registry.tags {
                tag.ids = tag.ids.iter().filter_map(|&id| remap.down(id)).collect();
            }
        }
    };
    if (state, id)
        == (
            State::Configuration,
            packet_ids::configuration::clientbound::UPDATE_TAGS,
        )
    {
        let mut p = configuration::Clientbound::decode(id, payload)?;
        if let configuration::Clientbound::UpdateTags(t) = &mut p {
            tags(&mut t.registries);
        }
        return Ok(Sent::Rewritten(strip_id(p.encode())));
    }
    if state != State::Play {
        return Ok(Sent::AsIs);
    }
    match id {
        cb::UPDATE_TAGS
        | cb::ADD_ENTITY
        | cb::BLOCK_ENTITY_DATA
        | cb::BLOCK_UPDATE
        | cb::SECTION_BLOCKS_UPDATE
        | cb::LEVEL_EVENT
        | cb::BLOCK_EVENT
        | cb::UPDATE_ATTRIBUTES
        | cb::SOUND
        | cb::SOUND_ENTITY
        | cb::SET_ENTITY_DATA
        | cb::CONTAINER_SET_CONTENT
        | cb::CONTAINER_SET_SLOT
        | cb::SET_CURSOR_ITEM
        | cb::SET_PLAYER_INVENTORY
        | cb::SET_EQUIPMENT
        | cb::MERCHANT_OFFERS
        | cb::RECIPE_BOOK_ADD
        | cb::PLACE_GHOST_RECIPE => {}
        _ => return Ok(Sent::AsIs),
    }
    let mut p = play::Clientbound::decode(id, payload)?;
    if let play::Clientbound::UpdateTags(t) = &mut p {
        tags(&mut t.registries);
    }
    Ok(match renumber(ids, &mut p) {
        Some(()) => Sent::Rewritten(encode_down(ids, &p)),
        None => Sent::Dropped,
    })
}

/// The plain registry ids in a decoded play packet, renumbered for the older client: `None` when it names
/// something the client has nothing to show for.
fn renumber(ids: &Ids, p: &mut play::Clientbound) -> Option<()> {
    let sound = |sound: &mut (i32, Option<(String, Option<f32>)>)| match sound.1 {
        // An inline sound is named, not numbered.
        Some(_) => Some(()),
        None => {
            sound.0 = down_or_substitute(ids.sound_event, ids.substitutes.sound_event, sound.0)?;
            Some(())
        }
    };
    match p {
        play::Clientbound::AddEntity(e) => {
            if e.kind == FALLING_BLOCK {
                e.data = state_down(ids, e.data);
            }
            e.kind = down_or_substitute(ids.entity_type, ids.substitutes.entity_type, e.kind)?;
            Some(())
        }
        play::Clientbound::BlockEntityData(b) => {
            b.kind = ids.block_entity_type.down(b.kind)?;
            Some(())
        }
        play::Clientbound::BlockUpdate(b) => {
            b.state = state_down(ids, b.state);
            Some(())
        }
        play::Clientbound::SectionBlocksUpdate(s) => {
            for change in &mut s.changes {
                let state = state_down(ids, (*change >> 12) as i32);
                *change = i64::from(state) << 12 | (*change & 0xfff);
            }
            Some(())
        }
        // LevelEvent.PARTICLES_DESTROY_BLOCK and the brush's BLOCK_FINISHED_BRUSHING carry a state.
        play::Clientbound::LevelEvent(e) if matches!(e.kind, 2001 | 3008) => {
            e.data = state_down(ids, e.data);
            Some(())
        }
        play::Clientbound::BlockEvent(e) => {
            e.block = down_or_substitute(ids.block, ids.substitutes.block, e.block)?;
            Some(())
        }
        play::Clientbound::UpdateAttributes(a) => {
            a.values.retain_mut(|v| {
                ids.attribute
                    .down(v.attribute)
                    .map(|a| v.attribute = a)
                    .is_some()
            });
            Some(())
        }
        play::Clientbound::Sound(s) => sound(&mut s.sound),
        play::Clientbound::SoundEntity(s) => sound(&mut s.sound),
        play::Clientbound::SetEntityData(d) => {
            d.values
                .retain_mut(|(_, kind, value)| entity_value(ids, *kind, value));
            Some(())
        }
        // Item stacks and recipe displays: their writers renumber items and components (encode_down).
        _ => Some(()),
    }
}

/// One entity data value renumbered for the older client: false drops it (a particle it lacks).
fn entity_value(ids: &Ids, kind: i32, value: &mut Value) -> bool {
    let particle = |p: &mut crate::entity_data::Particle| {
        if let Some(ParticleData::State(s)) = &mut p.data {
            *s = state_down(ids, *s);
        }
        match down_or_substitute(ids.particle_type, ids.substitutes.particle_type, p.id) {
            Some(id) => {
                p.id = id;
                true
            }
            None => false,
        }
    };
    match value {
        Value::Id(id)
            if matches!(
                kind,
                serializer::BLOCK_STATE | serializer::OPTIONAL_BLOCK_STATE
            ) =>
        {
            *id = state_down(ids, *id);
            true
        }
        Value::Particle(p) => particle(p),
        Value::Particles(list) => {
            list.retain_mut(particle);
            true
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buf::Codec;

    fn body(id: i32, payload: &[u8]) -> Vec<u8> {
        let mut w = Writer::new();
        w.varint(id);
        w.bytes(payload);
        w.data
    }

    fn in_play() -> ClientSession {
        let s = ClientSession::new(ServerVersion::V26_2);
        s.reading.store(to_u8(State::Play), Ordering::Release);
        s.writing.store(to_u8(State::Play), Ordering::Release);
        s
    }

    // Expected ids are read off the 26.2 and 26.3 packets and registries reports by hand, not from the
    // generated tables.

    #[test]
    fn each_direction_follows_its_own_switches() {
        let s = ClientSession::new(ServerVersion::V26_2);
        assert_eq!((s.reading(), s.writing()), (State::Login, State::Login));
        // The server finishes the login: it speaks configuration, the client still login.
        s.clientbound(&body(packet_ids::login::clientbound::LOGIN_FINISHED, &[]))
            .unwrap();
        assert_eq!(
            (s.reading(), s.writing()),
            (State::Login, State::Configuration)
        );
        s.serverbound(
            packet_ids::login::serverbound::LOGIN_ACKNOWLEDGED,
            Vec::new(),
        )
        .unwrap();
        assert_eq!(s.reading(), State::Configuration);
        // finish_configuration is 3 both ways in 26.2 as in 26.3.
        s.clientbound(&body(3, &[])).unwrap();
        s.serverbound(3, Vec::new()).unwrap();
        assert_eq!((s.reading(), s.writing()), (State::Play, State::Play));
    }

    #[test]
    fn packets_are_renumbered_both_ways_and_unknown_ones_dropped() {
        let s = in_play();
        // keep_alive: 26.3's clientbound 45 is 26.2's 44; serverbound 28 in both.
        let sent = s
            .clientbound(&body(packet_ids::play::clientbound::KEEP_ALIVE, &[0; 8]))
            .unwrap()
            .unwrap();
        assert_eq!(sent[0], 44);
        assert_eq!(
            s.serverbound(28, vec![0; 8]).unwrap(),
            Some((packet_ids::play::serverbound::KEEP_ALIVE, vec![0; 8]))
        );
        // add_transient_block (37) is new in 26.3: not sent.
        let transient = packet_ids::play::clientbound::ADD_TRANSIENT_BLOCK;
        assert_eq!(s.clientbound(&body(transient, &[0; 9])).unwrap(), None);
        // 26.2's resource_pack (49) is 26.3's 50; its swing (63) has no pinned packet.
        assert_eq!(
            s.serverbound(49, vec![7]).unwrap(),
            Some((packet_ids::play::serverbound::RESOURCE_PACK, vec![7]))
        );
        assert_eq!(s.serverbound(63, vec![0]).unwrap(), None);
    }

    #[test]
    fn block_states_the_client_lacks_get_a_stand_in() {
        let s = in_play();
        let update = |state: i32| {
            let p = play::Clientbound::BlockUpdate(play::BlockUpdate {
                pos: [1, 2, 3],
                state,
            });
            let sent = s.clientbound(&p.encode()).unwrap().unwrap();
            let mut r = Reader::new(&sent);
            // block_update: 8 in both.
            assert_eq!(r.varint().unwrap(), 8);
            match play::Clientbound::decode(8, r.rest()).unwrap() {
                play::Clientbound::BlockUpdate(b) => b.state,
                other => panic!("{other:?}"),
            }
        };
        // chest's first state: 5543 in 26.3, 3987 in 26.2; grass_block 8 in both.
        assert_eq!(update(5543), 3987);
        assert_eq!(update(8), 8);
        // poplar_log axis=y (167, new in 26.3) shows as birch_log axis=y (146 in 26.3, 143 in 26.2).
        assert_eq!(update(167), 143);
        // shelf_mushroom (11227, new in 26.3) is solid: it shows as a brown_mushroom_block (9500 in 26.3, 7766 in
        // 26.2), not as the brown mushroom players would fall through.
        assert_eq!(update(11227), 7766);
    }

    #[test]
    fn items_take_the_clients_ids_and_stand_ins() {
        use crate::item::Stack;
        let s = in_play();
        let slot = |item: i32| {
            let p = play::Clientbound::SetCursorItem(play::SetCursorItem {
                item: Some(Stack::new(item, 3)),
            });
            let sent = s.clientbound(&p.encode()).unwrap().unwrap();
            let mut r = Reader::new(&sent);
            r.varint().unwrap();
            let mut payload = Reader::new(r.rest());
            crate::item::Slot::read(&mut payload).unwrap().unwrap().item
        };
        // diamond: 1012 in 26.3, 926 in 26.2.
        assert_eq!(slot(1012), 926);
        // poplar_planks (72, new in 26.3) shows as birch_planks (65 in both).
        assert_eq!(slot(72), 65);
        // ocean_monument_map (1239, new in 26.3) is a filled map: filled_map (1238 in 26.3, 1133 in 26.2).
        assert_eq!(slot(1239), 1133);
    }

    #[test]
    fn an_entity_the_client_lacks_is_not_spawned() {
        let s = in_play();
        let add = |kind: i32| {
            play::Clientbound::AddEntity(play::AddEntity {
                id: 5,
                uuid: 1,
                kind,
                position: [0.0; 3],
                velocity: [0.0; 3],
                pitch: 0,
                yaw: 0,
                head_yaw: 0,
                data: 0,
            })
            .encode()
        };
        // cushion (33, new in 26.3) has no stand-in; poplar_boat (106) sails as birch_boat (12).
        assert_eq!(s.clientbound(&add(33)).unwrap(), None);
        let sent = s.clientbound(&add(106)).unwrap().unwrap();
        let mut r = Reader::new(&sent);
        // add_entity: 1 in both.
        assert_eq!(r.varint().unwrap(), 1);
        match play::Clientbound::decode(1, r.rest()).unwrap() {
            play::Clientbound::AddEntity(e) => assert_eq!(e.kind, 12),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_creative_slot_from_the_client_takes_the_pinned_item() {
        let s = in_play();
        // set_creative_mode_slot: 26.2's 56 is 26.3's 57. Slot 36, three diamonds (926 in 26.2), no components.
        let mut w = Writer::new();
        w.i16(36);
        crate::item::DelimitedSlot::write(&Some(crate::item::Stack::new(926, 3)), &mut w);
        let (id, payload) = s.serverbound(56, w.data).unwrap().unwrap();
        assert_eq!(id, packet_ids::play::serverbound::SET_CREATIVE_MODE_SLOT);
        match play::Serverbound::decode(id, &payload).unwrap() {
            play::Serverbound::SetCreativeModeSlot(p) => {
                assert_eq!(p.item.map(|i| (i.item, i.count)), Some((1012, 3)));
            }
            other => panic!("{other:?}"),
        }
    }
}
