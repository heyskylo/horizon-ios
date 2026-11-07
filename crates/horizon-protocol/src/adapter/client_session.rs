//! One connection from a client of an older release to a Horizon server: [`super::Session`] run the other
//! way (roadmap `older-clients`). What the client sends is renumbered to the pinned ids before the server
//! reads it; what the server sends is renumbered to the client's, registry ids included, with a stand-in
//! ([`ids::Substitutes`]) for a block, item, entity, particle or sound the client's version lacks.
//!
//! This layer maps ids; the world packets whose layout changed between 26.2 and the pinned release are
//! written in 26.2's layout by [`world`]. Serverbound, 26.2's `accept_teleportation`, `player_action` and
//! `swing` are read as 26.3's (the mirror of `translate`'s writers); other packets whose layout changed
//! still pass in the pinned layout.
//!
//! The states change where vanilla's server-side `Connection` switches its codecs, after the handshake the
//! server read itself: `login_acknowledged` and `login_finished` move each direction to configuration,
//! `finish_configuration` to play, and `configuration_acknowledged` / `start_configuration` back.
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Mutex,
        atomic::{AtomicU8, Ordering},
    },
};

mod step_1_20_6;
mod step_1_21_1;
mod step_1_21_10;
mod step_1_21_11;
mod step_1_21_4;
mod step_1_21_5;
mod step_1_21_6;
mod step_1_21_8;
mod step_26_1;
mod world;

use super::{
    ServerVersion,
    ids::{self, Ids, Way, down_or_substitute},
    layout_26_2,
    translate::{packet_ids_entity::FALLING_BLOCK, swing_id},
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
    /// Where the client last said or was told it is, for the place 26.3's `accept_teleportation` states and
    /// 26.2's leaves out.
    place: Mutex<Place>,
    /// Teleports (`player_position`) sent and not yet accepted, oldest first.
    teleports: Mutex<VecDeque<Teleport>>,
    /// The entities the client was sent, by network id: their pinned type and UUID, for the older
    /// releases' entity data and 26.1's spectating by UUID.
    entities: Mutex<HashMap<i32, (i32, u128)>>,
}

/// A position, yaw and pitch.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Place {
    position: [f64; 3],
    yaw: f32,
    pitch: f32,
}

/// A `player_position` the server sent.
#[derive(Clone, Copy, Debug)]
struct Teleport {
    id: i32,
    to: Place,
    relatives: i32,
}

impl Teleport {
    /// Where a client at `from` is once it took this teleport, as `LocalPlayer` applies
    /// `ClientboundPlayerPositionPacket`'s relative flags.
    fn applied(&self, from: Place) -> Place {
        use play::relative as rel;
        let pick = |bit: i32, base: f64, v: f64| {
            if self.relatives & bit != 0 {
                base + v
            } else {
                v
            }
        };
        let angle = |bit: i32, base: f32, v: f32| {
            if self.relatives & bit != 0 {
                base + v
            } else {
                v
            }
        };
        Place {
            position: [
                pick(rel::X, from.position[0], self.to.position[0]),
                pick(rel::Y, from.position[1], self.to.position[1]),
                pick(rel::Z, from.position[2], self.to.position[2]),
            ],
            yaw: angle(rel::Y_ROT, from.yaw, self.to.yaw),
            pitch: angle(rel::X_ROT, from.pitch, self.to.pitch).clamp(-90.0, 90.0),
        }
    }
}

impl ClientSession {
    /// A client whose login handshake stated `version`'s protocol: both directions are in login.
    pub fn new(version: ServerVersion) -> Self {
        Self {
            version,
            reading: AtomicU8::new(to_u8(State::Login)),
            writing: AtomicU8::new(to_u8(State::Login)),
            place: Mutex::new(Place::default()),
            teleports: Mutex::new(VecDeque::new()),
            entities: Mutex::new(HashMap::new()),
        }
    }
    pub fn reading(&self) -> State {
        from_u8(self.reading.load(Ordering::Acquire))
    }
    pub fn writing(&self) -> State {
        from_u8(self.writing.load(Ordering::Acquire))
    }

    /// A packet the client sent, with the id the pinned version knows it by. `None` drops it: the pinned
    /// version has no such packet.
    pub fn serverbound(&self, id: i32, payload: Vec<u8>) -> Result<Option<(i32, Vec<u8>)>> {
        let state = self.reading();
        // 26.3's punch took over the older swing of the main hand; an off-hand swing has no packet there.
        if state == State::Play && id == swing_id(self.version) {
            let hand = Reader::new(&payload).varint()?;
            return Ok((hand == 0).then_some((packet_ids::play::serverbound::PUNCH, Vec::new())));
        }
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
        let Some((pinned, payload)) = self.older_layout_up(state, pinned, payload)? else {
            return Ok(None);
        };
        use packet_ids::play::serverbound as sb;
        let payload = match (state, pinned) {
            (State::Play, sb::ACCEPT_TELEPORTATION) => self.accept_teleport(&payload)?,
            // 26.3 inserted CHANGE_DESTROY_DIRECTION at 1: 26.2's actions past 0 are one up.
            (State::Play, sb::PLAYER_ACTION) => {
                let mut r = Reader::new(&payload);
                let action = r.varint()?;
                let mut w = Writer::new();
                w.varint(if action == 0 { 0 } else { action + 1 });
                w.bytes(r.rest());
                w.data
            }
            (State::Play, sb::MOVE_PLAYER_POS | sb::MOVE_PLAYER_POS_ROT | sb::MOVE_PLAYER_ROT) => {
                self.moved(pinned, &payload)?;
                payload
            }
            _ => payload,
        };
        let Some(ids) = self.version.ids() else {
            return Ok(Some((pinned, payload)));
        };
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
        // 1.21.1 has no entity_position_sync: the teleport it stands in for is sent under teleport_entity's id.
        let synced = if self.version.at_most(ServerVersion::V1_21_1) {
            step_1_21_1::entity_position_sync(state, id, payload)?
        } else {
            None
        };
        let (id, payload) = match &synced {
            Some(p) => (packet_ids::play::clientbound::TELEPORT_ENTITY, &p[..]),
            None => (id, payload),
        };
        // 26.2 has no swing_animation: its hands swing through animate (world.rs).
        if state == State::Play
            && id == packet_ids::play::clientbound::SWING_ANIMATION
            && self.version.ids().is_some()
        {
            let animate = packet_ids::play::clientbound::ANIMATE;
            let Some(mapped) = self.version.clientbound_id_down(state, animate) else {
                return Ok(None);
            };
            let mut w = Writer::new();
            w.varint(mapped);
            w.bytes(&world::swing(payload)?);
            return Ok(Some(w.data));
        }
        if (state, id) == (State::Play, packet_ids::play::clientbound::PLAYER_POSITION) {
            self.teleporting(payload)?;
        }
        let tracked;
        let payload = match self.track_entities(state, id, payload)? {
            Some(p) => {
                tracked = p;
                &tracked[..]
            }
            None => payload,
        };
        let Some(mapped) = self.version.clientbound_id_down(state, id) else {
            return Ok(None);
        };
        // A packet the renumbering can't read goes as it is, rather than the connection dropping here.
        let rewritten = match self
            .version
            .ids()
            .map(|ids| layout_then_ids(self.version, ids, state, id, payload))
        {
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
        let out = rewritten.unwrap_or_else(|| payload.to_vec());
        let Some(out) = self.older_layout_down(state, id, out)? else {
            return Ok(None);
        };
        let mut w = Writer::new();
        w.varint(mapped);
        w.bytes(&out);
        Ok(Some(w.data))
    }

    /// Keeps the entities the client is sent (`add_entity`, `remove_entities`), and writes a pinned
    /// `set_entity_data` in 1.21.11's or 1.21.10's entity data (still in pinned ids, which [`down`]
    /// renumbers): `Some` when it changed.
    fn track_entities(&self, state: State, id: i32, payload: &[u8]) -> Result<Option<Vec<u8>>> {
        use packet_ids::play::clientbound as cb;
        if state != State::Play
            || !matches!(
                id,
                cb::ADD_ENTITY | cb::REMOVE_ENTITIES | cb::SET_ENTITY_DATA
            )
        {
            return Ok(None);
        }
        let mut p = play::Clientbound::decode(id, payload)?;
        let mut entities = self.entities.lock().unwrap();
        match &mut p {
            play::Clientbound::AddEntity(e) => {
                entities.insert(e.id, (e.kind, e.uuid));
            }
            play::Clientbound::RemoveEntities(r) => {
                for id in &r.ids {
                    entities.remove(id);
                }
            }
            play::Clientbound::SetEntityData(d)
                if self.version.at_most(ServerVersion::V1_21_11) =>
            {
                let kind = entities.get(&d.id).map(|&(kind, _)| kind);
                step_1_21_11::entity_data(kind, d);
                if self.version.at_most(ServerVersion::V1_21_10) {
                    step_1_21_10::entity_data(kind, d);
                }
                if self.version.at_most(ServerVersion::V1_21_8) {
                    step_1_21_8::entity_data(kind, d);
                }
                // 1.21.5's indexes go back to 1.21.4's last.
                if self.version.at_most(ServerVersion::V1_21_4) {
                    step_1_21_4::entity_data(kind, d);
                }
                return Ok(Some(strip_id(p.encode())));
            }
            _ => {}
        }
        Ok(None)
    }

    /// A packet in 26.2's layout (and the client's ids) brought down to the client's own release, a step at a
    /// time: `None` when that release has no such packet.
    fn older_layout_down(
        &self,
        state: State,
        id: i32,
        mut payload: Vec<u8>,
    ) -> Result<Option<Vec<u8>>> {
        let v = self.version;
        // 1.21.2 reworked the recipe book (recipe displays, no ids a 1.21.1 client knows): a 1.21.1 client gets no
        // recipe book, so its update_recipes and place_ghost_recipe are not sent. Its recipe_book_add, _remove and
        // _settings have no 1.21.1 id at all (the id tables drop them).
        if v.at_most(ServerVersion::V1_21_1)
            && state == State::Play
            && matches!(
                id,
                packet_ids::play::clientbound::UPDATE_RECIPES
                    | packet_ids::play::clientbound::PLACE_GHOST_RECIPE
            )
        {
            return Ok(None);
        }
        // 1.21.11 has world clocks in no registry (26.1 synced them): its client is not sent that one.
        if v.at_most(ServerVersion::V1_21_11)
            && (state, id)
                == (
                    State::Configuration,
                    packet_ids::configuration::clientbound::REGISTRY_DATA,
                )
            && Reader::new(&payload).string(32767)? == "minecraft:world_clock"
        {
            return Ok(None);
        }
        type Step = fn(State, i32, &[u8]) -> Result<Option<Vec<u8>>>;
        let steps: [(ServerVersion, Step); 4] = [
            (ServerVersion::V26_1, step_26_1::clientbound),
            (ServerVersion::V1_21_11, step_1_21_11::clientbound),
            (ServerVersion::V1_21_10, step_1_21_10::clientbound),
            (ServerVersion::V1_21_8, step_1_21_8::clientbound),
        ];
        for (to, step) in steps {
            if v.at_most(to)
                && let Some(next) = step(state, id, &payload)?
            {
                payload = next;
            }
        }
        // 1.21.8 is newer than 1.21.6: its packets are brought down after the steps above.
        if v.at_most(ServerVersion::V1_21_6) {
            match step_1_21_6::clientbound(state, id, &payload)? {
                step_1_21_6::Out::Untouched => {}
                step_1_21_6::Out::Rewritten(p) => payload = p,
                step_1_21_6::Out::Dropped => return Ok(None),
            }
        }
        // 1.21.5 is older than 1.21.6 (and than every step above): its packets are brought down last.
        if v.at_most(ServerVersion::V1_21_5)
            && let Some(next) = step_1_21_5::clientbound(state, id, &payload)?
        {
            payload = next;
        }
        // 1.21.4 is older than 1.21.5 too: after its step, the last one.
        if v.at_most(ServerVersion::V1_21_4)
            && let Some(next) = step_1_21_4::clientbound(state, id, &payload)?
        {
            payload = next;
        }
        // 1.21.1 is older than 1.21.2 and than 1.21.4: its packets are brought down after those, and before 1.20.6's.
        if v.at_most(ServerVersion::V1_21_1)
            && let Some(next) = step_1_21_1::clientbound(state, id, &payload)?
        {
            payload = next;
        }
        // 1.20.6 is the oldest release: its packets are brought down after every step above.
        if v.at_most(ServerVersion::V1_20_6)
            && let Some(ids) = v.ids()
            && let Some(next) = step_1_20_6::clientbound(state, id, &payload, ids)?
        {
            payload = next;
        }
        Ok(Some(payload))
    }

    /// A packet an older client sent (already under its pinned id) in 26.2's layout, oldest step first:
    /// `None` drops it.
    fn older_layout_up(
        &self,
        state: State,
        pinned: i32,
        mut payload: Vec<u8>,
    ) -> Result<Option<(i32, Vec<u8>)>> {
        use packet_ids::play::serverbound as sb;
        let v = self.version;
        // 1.20.6 is the oldest release: its use_item is read first, with the client's last rotation.
        if v.at_most(ServerVersion::V1_20_6) {
            let (yaw, pitch) = {
                let place = self.place.lock().unwrap();
                (place.yaw, place.pitch)
            };
            if let Some(next) = step_1_20_6::serverbound(state, pinned, &payload, (yaw, pitch))? {
                payload = next;
            }
        }
        // 1.21.1 is older than 1.21.2 and 1.21.4: its movement and steering are read before theirs (step_1_21_1).
        if v.at_most(ServerVersion::V1_21_1)
            && let Some(p) = step_1_21_1::serverbound(state, pinned, &payload)?
        {
            payload = p;
        }
        // 1.21.4 is the oldest step: its packets are read before 1.21.5's.
        if v.at_most(ServerVersion::V1_21_4) {
            match step_1_21_4::serverbound(state, pinned, &payload, v.ids())? {
                step_1_21_4::Out::Untouched => {}
                step_1_21_4::Out::Rewritten(p) => payload = p,
            }
        }
        // 1.21.5 is the next oldest: its packets are read before 1.21.6's.
        if v.at_most(ServerVersion::V1_21_5) {
            match step_1_21_5::serverbound(state, pinned, &payload)? {
                step_1_21_5::Out::Untouched => {}
                step_1_21_5::Out::Rewritten(p) => payload = p,
                step_1_21_5::Out::Dropped => return Ok(None),
            }
        }
        // 1.21.6 is the oldest of these: its packets are read first.
        if v.at_most(ServerVersion::V1_21_6) {
            match step_1_21_6::serverbound(state, pinned, &payload)? {
                step_1_21_6::Out::Untouched => {}
                step_1_21_6::Out::Rewritten(p) => payload = p,
                step_1_21_6::Out::Dropped => return Ok(None),
            }
        }
        if v.at_most(ServerVersion::V1_21_11) && (state, pinned) == (State::Play, sb::INTERACT) {
            let mut r = Reader::new(&payload);
            let _entity = r.varint()?;
            return Ok(match r.varint()? {
                // INTERACT follows each INTERACT_AT; 26.3's interact is the latter alone.
                0 => None,
                1 => Some((sb::ATTACK, step_1_21_11::attack(&payload)?)),
                _ => {
                    let p = step_1_21_11::serverbound(state, pinned, &payload)?;
                    Some((pinned, p.unwrap_or(payload)))
                }
            });
        }
        if v.at_most(ServerVersion::V26_1) {
            let entities = self.entities.lock().unwrap();
            let entity_of = |uuid: u128| {
                entities
                    .iter()
                    .find(|(_, (_, u))| *u == uuid)
                    .map(|(&id, _)| id)
            };
            match step_26_1::serverbound(state, pinned, &payload, entity_of)? {
                step_26_1::Out::Untouched => {}
                step_26_1::Out::Rewritten(p) => return Ok(Some((pinned, p))),
                step_26_1::Out::Dropped => return Ok(None),
            }
        }
        Ok(Some((pinned, payload)))
    }
}

/// A 26.2 client's packets whose layout changed are written in its layout ([`layout_26_2`], when enabled),
/// the others renumbered by [`down`].
fn layout_then_ids(
    version: ServerVersion,
    ids: &'static Ids,
    state: State,
    id: i32,
    payload: &[u8],
) -> Result<Sent> {
    if version.at_most(ServerVersion::V26_2) && layout_26_2::enabled() {
        match layout_26_2::clientbound(ids, state, id, payload)? {
            layout_26_2::Out::Untouched => {}
            layout_26_2::Out::Rewritten(p) => return Ok(Sent::Rewritten(p)),
            layout_26_2::Out::Dropped => return Ok(Sent::Dropped),
        }
    }
    down(ids, state, id, payload)
}

impl ClientSession {
    /// A `player_position` on its way: the client accepts it later by its id alone.
    fn teleporting(&self, payload: &[u8]) -> Result<()> {
        let mut r = Reader::new(payload);
        let id = r.varint()?;
        let position = [r.f64()?, r.f64()?, r.f64()?];
        let _delta = [r.f64()?, r.f64()?, r.f64()?];
        let to = Place {
            position,
            yaw: r.f32()?,
            pitch: r.f32()?,
        };
        let relatives = r.i32()?;
        self.teleports
            .lock()
            .unwrap()
            .push_back(Teleport { id, to, relatives });
        Ok(())
    }

    /// A move the client reports (`move_player_pos`, `_pos_rot`, `_rot`): where it now is.
    fn moved(&self, pinned: i32, payload: &[u8]) -> Result<()> {
        use packet_ids::play::serverbound as sb;
        let mut r = Reader::new(payload);
        let mut place = self.place.lock().unwrap();
        if pinned != sb::MOVE_PLAYER_ROT {
            place.position = [r.f64()?, r.f64()?, r.f64()?];
        }
        if pinned != sb::MOVE_PLAYER_POS {
            place.yaw = r.f32()?;
            place.pitch = r.f32()?;
        }
        Ok(())
    }

    /// 26.2's `accept_teleportation` (the teleport's id) as 26.3's, which also states where the client
    /// now is: the teleport it accepts, and any sent before it, applied in order to where it was.
    fn accept_teleport(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let id = Reader::new(payload).varint()?;
        let mut place = self.place.lock().unwrap();
        let mut teleports = self.teleports.lock().unwrap();
        if let Some(i) = teleports.iter().position(|t| t.id == id) {
            for t in teleports.drain(..=i) {
                *place = t.applied(*place);
            }
        }
        let mut w = Writer::new();
        w.varint(id);
        for v in place.position {
            w.f64(v);
        }
        w.f32(place.yaw);
        w.f32(place.pitch);
        Ok(w.data)
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
    // Packets whose layout 26.2 has differently (world.rs).
    if let Some(sent) = world::down(ids, id, payload)? {
        return Ok(sent);
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
        // 26.2's resource_pack (49) is 26.3's 50.
        assert_eq!(
            s.serverbound(49, vec![7]).unwrap(),
            Some((packet_ids::play::serverbound::RESOURCE_PACK, vec![7]))
        );
    }

    #[test]
    fn a_26_2_main_hand_swing_is_a_punch_and_an_off_hand_one_nothing() {
        let s = in_play();
        // swing: 63 in 26.2, its hand a VarInt (0 main, 1 off); 26.3's punch has no payload.
        assert_eq!(
            s.serverbound(63, vec![0]).unwrap(),
            Some((packet_ids::play::serverbound::PUNCH, Vec::new()))
        );
        assert_eq!(s.serverbound(63, vec![1]).unwrap(), None);
    }

    #[test]
    fn a_26_2_digging_action_past_0_is_one_up() {
        let s = in_play();
        let action = |n: i32| {
            let mut w = Writer::new();
            w.varint(n);
            w.i64(crate::buf::pack_block_pos([1, 2, 3]));
            w.u8(1);
            w.varint(9);
            // player_action: 41 in both.
            let (id, payload) = s.serverbound(41, w.data).unwrap().unwrap();
            assert_eq!(id, packet_ids::play::serverbound::PLAYER_ACTION);
            match play::Serverbound::decode(id, &payload).unwrap() {
                play::Serverbound::PlayerAction(p) => {
                    assert_eq!((p.pos, p.direction, p.sequence), ([1, 2, 3], 1, 9));
                    p.action
                }
                other => panic!("{other:?}"),
            }
        };
        assert_eq!(action(0), 0); // start destroying
        assert_eq!(action(1), 2); // abort: 1 in 26.2, 2 in 26.3
        assert_eq!(action(2), 3); // stop destroying
        assert_eq!(action(6), 7); // swap held items
    }

    #[test]
    fn a_26_2_teleport_confirmation_states_where_the_teleport_put_the_client() {
        use play::relative as rel;
        let s = in_play();
        let teleport = |id: i32, position: [f64; 3], yaw: f32, relatives: i32| {
            let p = play::Clientbound::PlayerPosition(play::PlayerPosition {
                teleport: id,
                position,
                delta: [0.0; 3],
                yaw,
                pitch: 30.0,
                relatives,
            });
            s.clientbound(&p.encode()).unwrap().unwrap();
        };
        let accept = |id: i32| {
            let mut w = Writer::new();
            w.varint(id);
            // accept_teleportation: 0 in both.
            let (pinned, payload) = s.serverbound(0, w.data).unwrap().unwrap();
            assert_eq!(pinned, packet_ids::play::serverbound::ACCEPT_TELEPORTATION);
            match play::Serverbound::decode(pinned, &payload).unwrap() {
                play::Serverbound::AcceptTeleportation(p) => {
                    assert_eq!(p.id, id);
                    ([p.x, p.y, p.z], p.yaw, p.pitch)
                }
                other => panic!("{other:?}"),
            }
        };
        teleport(1, [8.5, -60.0, 8.5], 90.0, 0);
        assert_eq!(accept(1), ([8.5, -60.0, 8.5], 90.0, 30.0));
        // The client walks (move_player_pos: 30 in both), then is moved by (1, 2, 3) and turned by 10.
        let mut w = Writer::new();
        for v in [10.0, -60.0, 10.0] {
            w.f64(v);
        }
        w.u8(1);
        s.serverbound(30, w.data).unwrap().unwrap();
        teleport(
            2,
            [1.0, 2.0, 3.0],
            10.0,
            rel::X | rel::Y | rel::Z | rel::Y_ROT,
        );
        assert_eq!(accept(2), ([11.0, -58.0, 13.0], 100.0, 30.0));
        // An unknown id: where the client last was.
        assert_eq!(accept(7), ([11.0, -58.0, 13.0], 100.0, 30.0));
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
