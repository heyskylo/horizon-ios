//! 1.21.2's packets in 1.21.1's layout for a 1.21.1 client (and 1.21): the mirror of `translate`'s `mod v1_21_1`
//! (1.21.1 to 1.21.2). Clientbound only, for now: the serverbound packets 1.21.2 changed are not written yet.
//!
//! Done here, packet by packet:
//! - The window id of container_close, container_set_content, container_set_data and mount_screen_open is a
//!   VarInt from 1.21.2, a byte in 1.21.1; container_set_slot's is a signed byte in 1.21.1 (-1 is the cursor).
//! - set_held_slot is a VarInt, a byte in 1.21.1 (see the TODO in translate's `v1_21_1`: the id tables call 1.21.1's
//!   set_carried_item unknown, so this layout is reached only once that is renamed).
//! - set_time: 1.21.2's day time and whether it runs become 1.21.1's day time, negative while it stands still.
//! - login and respawn: 1.21.2's sea level after the portal cooldown goes.
//! - level_particles: 1.21.2's second boolean (always visible) goes.
//! - teleport_entity: the velocity goes, the angles are bytes (1/256 of a turn) and the relative flags are dropped
//!   (an absolute position only; a relative one loses its meaning here).
//! - player_position: the teleport id goes last, the velocity goes, the flags' low five bits are 1.21.1's.
//! - player_info_update: 1.21.2's list order (0x40, its VarInt after the display name) and hat (0x80, a boolean
//!   last) are left out with their bits; 1.21.1 has neither.
//!
//! Not done (TODO(step1_21_1)):
//! - explode: 1.21.2's explosion (radius, block count, optional knockback, particles as a list) is not the 1.21.1
//!   one (strength, records, motion, block interaction, two particles). Needs the 1.21.2 particle mapping.
//! - recipe_book_add, recipe_book_remove, recipe_book_settings, update_recipes, place_ghost_recipe, and the
//!   1.21.1 `recipe` packet which 1.21.2 dropped: the recipe book was reworked. 1.21.2 sends recipe displays, not
//!   the ids 1.21.1 knows. Needs a drop (`Out::Dropped`-style) answer and the recipe data.
//! - set_cursor_item and set_player_inventory (1.21.2): no 1.21.1 packet, so the id tables drop them. The client's
//!   cursor and inventory slots are not updated for it until they are sent as container_set_slot.
//! - entity_position_sync (1.21.2's position sync): no 1.21.1 packet; 1.21.1's teleport_entity is its equal, but the
//!   id tables drop it and client_session's id mapping would have to point it at teleport_entity.
//! - Entity data: the serializer list of 1.21.1 is 1.21.4's (checked on the wiki's 1.21.4 metadata page: the same
//!   31 types), so the ids table's runs are the only change (see adapter.rs V1_21_1).
use crate::{
    Error, Result, State,
    buf::{Codec, Opt, Reader, Vec3d, Writer},
    entity_data::Particle,
    ids as packet_ids,
    packets::{login::Property, play},
};

/// The strength 1.21.1 is sent for an explosion. 1.21.2's explode has no radius: this is 1.21.8's guess too
/// (translate's `EXPLODE_RADIUS`), and only chooses the blast's particles and sound volume on a 1.21.1 client.
// TODO(step1_21_1): the radius is unknown here; 3.0 is a guess.
const EXPLODE_STRENGTH: f32 = 3.0;

/// The pinned clientbound packet `id` (`payload` in 1.21.2's layout) in 1.21.1's layout; `None` when laid out alike.
pub(super) fn clientbound(state: State, id: i32, payload: &[u8]) -> Result<Option<Vec<u8>>> {
    use packet_ids::play::clientbound as cb;
    // 1.21.1's login_finished (game_profile) ends with strict error handling: off, so that the client ignores a
    // packet it can't process rather than disconnecting.
    if (state, id) == (State::Login, packet_ids::login::clientbound::LOGIN_FINISHED) {
        let mut out = payload.to_vec();
        out.push(0);
        return Ok(Some(out));
    }
    if state != State::Play {
        return Ok(None);
    }
    Ok(Some(match id {
        cb::CONTAINER_CLOSE
        | cb::CONTAINER_SET_CONTENT
        | cb::CONTAINER_SET_DATA
        | cb::MOUNT_SCREEN_OPEN => window_down(payload, false)?,
        cb::CONTAINER_SET_SLOT => window_down(payload, true)?,
        cb::SET_HELD_SLOT => held_slot(payload)?,
        cb::SET_TIME => set_time(payload)?,
        cb::LOGIN => spawn_info_down(payload, true)?,
        cb::RESPAWN => spawn_info_down(payload, false)?,
        cb::LEVEL_PARTICLES => particles(payload)?,
        cb::TELEPORT_ENTITY => teleport_entity(payload)?,
        cb::PLAYER_POSITION => player_position(payload)?,
        cb::PLAYER_INFO_UPDATE => player_info(payload)?,
        cb::EXPLODE => explode(payload)?,
        _ => return Ok(None),
    }))
}

/// A VarInt window id as 1.21.1's byte (`signed`: container_set_slot's signed one); the rest passes.
fn window_down(payload: &[u8], signed: bool) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let window = r.varint()?;
    let mut w = Writer::new();
    if signed {
        w.i8(window as i8);
    } else {
        w.u8(window as u8);
    }
    w.bytes(r.rest());
    Ok(w.data)
}

/// The held slot as 1.21.1's byte.
fn held_slot(payload: &[u8]) -> Result<Vec<u8>> {
    let slot = Reader::new(payload).varint()?;
    let mut w = Writer::new();
    w.i8(slot as i8);
    Ok(w.data)
}

/// 1.21.2's day time and whether it runs as 1.21.1's day time, negative while it stands still.
fn set_time(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let game_time = r.i64()?;
    let day_time = r.i64()?;
    let ticking = r.bool()?;
    let time = day_time.saturating_abs();
    let mut w = Writer::new();
    w.i64(game_time);
    w.i64(if ticking { time } else { -time });
    Ok(w.data)
}

/// Login and respawn: the sea level 1.21.2 sends after the portal cooldown goes; the rest is laid out alike.
fn spawn_info_down(payload: &[u8], login: bool) -> Result<Vec<u8>> {
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
    let sea_level = r.position();
    r.varint()?;
    let after = r.position();
    let mut w = Writer::new();
    w.bytes(&payload[..sea_level]);
    w.bytes(&payload[after..]);
    Ok(w.data)
}

/// 1.21.2's two booleans (long distance, always visible) as 1.21.1's one: the second goes.
fn particles(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let long_distance = r.bool()?;
    r.bool()?;
    let mut w = Writer::new();
    w.bool(long_distance);
    w.bytes(r.rest());
    Ok(w.data)
}

/// Degrees as 1.21.1's angle: a byte of a turn (1/256), wrapped.
fn angle(degrees: f32) -> u8 {
    let turns = degrees.rem_euclid(360.0) * 256.0 / 360.0;
    (turns.round() as i64 % 256) as u8
}

/// 1.21.2's teleport_entity as 1.21.1's: the velocity goes, the angles are bytes, and the relative flags are
/// dropped. A relative teleport is sent to a 1.21.1 client as the absolute position it names.
// TODO(step1_21_1): relative flags have no 1.21.1 form; a relative teleport is sent as its absolute position.
fn teleport_entity(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let id = r.varint()?;
    let position = [r.f64()?, r.f64()?, r.f64()?];
    let _velocity = [r.f64()?, r.f64()?, r.f64()?];
    let yaw = r.f32()?;
    let pitch = r.f32()?;
    let _relatives = r.i32()?;
    let on_ground = r.bool()?;
    let mut w = Writer::new();
    w.varint(id);
    position.iter().for_each(|v| w.f64(*v));
    w.u8(angle(yaw));
    w.u8(angle(pitch));
    w.bool(on_ground);
    Ok(w.data)
}

/// 1.21.2's player_position as 1.21.1's: the position and the rotation, the flags' low five bits as a byte, the
/// teleport id last. The velocity goes. Its flags for the velocity (bits 5 to 8) have no 1.21.1 form and are dropped.
// TODO(step1_21_1): velocity-relative flags (bits 5 to 8) are dropped.
fn player_position(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let teleport = r.varint()?;
    let position = [r.f64()?, r.f64()?, r.f64()?];
    let _velocity = [r.f64()?, r.f64()?, r.f64()?];
    let yaw = r.f32()?;
    let pitch = r.f32()?;
    let relatives = r.i32()?;
    let mut w = Writer::new();
    position.iter().for_each(|v| w.f64(*v));
    w.f32(yaw);
    w.f32(pitch);
    w.u8((relatives & 0x1F) as u8);
    w.varint(teleport);
    Ok(w.data)
}

/// 1.21.2's player list entries as 1.21.1's: the list order (0x40, a VarInt after the display name) and the hat
/// (0x80, a boolean last) go with their bits; 1.21.1 has neither. The other actions are laid out alike.
fn player_info(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let actions = r.u8()?;
    let n = r.count(r.remaining())?;
    let mut w = Writer::new();
    w.u8(actions & 0x3F);
    w.varint(n as i32);
    for _ in 0..n {
        w.uuid(r.uuid()?);
        if actions & 0x01 != 0 {
            w.string(&r.string(16)?);
            let count = r.count(16)?;
            w.varint(count as i32);
            for _ in 0..count {
                Property::write(&Property::read(&mut r)?, &mut w);
            }
        }
        if actions & 0x02 != 0 {
            let present = r.bool()?;
            w.bool(present);
            if present {
                w.uuid(r.uuid()?);
                w.i64(r.i64()?);
                w.byte_array(&r.byte_array(512)?);
                w.byte_array(&r.byte_array(4096)?);
            }
        }
        if actions & 0x04 != 0 {
            w.varint(r.varint()?);
        }
        if actions & 0x08 != 0 {
            w.bool(r.bool()?);
        }
        if actions & 0x10 != 0 {
            w.varint(r.varint()?);
        }
        if actions & 0x20 != 0 {
            let present = r.bool()?;
            w.bool(present);
            if present {
                w.nbt(&r.nbt()?);
            }
        }
        if actions & 0x40 != 0 {
            r.varint()?;
        }
        if actions & 0x80 != 0 {
            r.bool()?;
        }
    }
    Ok(w.data)
}

/// 1.21.2's explode as 1.21.1's. What 1.21.2 sends here is the centre, the knockback (an optional vector), the
/// particle and the sound (the step after 1.21.8's has taken the radius, the block count and the block particles
/// out). 1.21.1 sends the strength, a record count (0: no block is set to air), the motion (the knockback, or zero),
/// the block interaction (0 keep), and two particles: the small and the large one. Both are the one particle here.
fn explode(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let center = Vec3d::read(&mut r)?;
    let knockback = <Opt<Vec3d>>::read(&mut r)?;
    let particle = Particle::read(&mut r)?;
    let sound = play::SoundHolder::read(&mut r)?;
    let mut w = Writer::new();
    Vec3d::write(&center, &mut w);
    w.f32(EXPLODE_STRENGTH);
    w.varint(0);
    for v in knockback.unwrap_or([0.0; 3]) {
        w.f32(v as f32);
    }
    w.varint(0);
    particle.write(&mut w);
    particle.write(&mut w);
    play::SoundHolder::write(&sound, &mut w);
    Ok(w.data)
}

/// The serverbound packet `id` a 1.21.1 client sent, in 1.21.2's layout (`None`: laid out alike). The movement
/// packets end with an on-ground boolean where 1.21.2 has a flags byte (0x01 on ground; 1.21.1 never reports a wall).
/// Its steering (player_input: sideways and forward floats, a flags byte) becomes 1.21.2's key flags.
pub(super) fn serverbound(state: State, id: i32, payload: &[u8]) -> Result<Option<Vec<u8>>> {
    use packet_ids::play::serverbound as sb;
    if state != State::Play {
        return Ok(None);
    }
    match id {
        sb::MOVE_PLAYER_POS
        | sb::MOVE_PLAYER_POS_ROT
        | sb::MOVE_PLAYER_ROT
        | sb::MOVE_PLAYER_STATUS_ONLY => {
            let on_ground = *payload
                .last()
                .ok_or_else(|| Error::new("1.21.1 movement without its on-ground boolean"))?;
            let mut out = payload[..payload.len() - 1].to_vec();
            out.push(on_ground & 1);
            Ok(Some(out))
        }
        sb::PLAYER_INPUT => player_input(payload).map(Some),
        _ => Ok(None),
    }
}

/// 1.21.1's steering as 1.21.2's key flags: the sign of forward (0x01 forward, 0x02 backward) and of sideways
/// (positive to the left: 0x04 left, 0x08 right), the jump bit (0x10), and the unmount bit as sneak (0x20).
/// 1.21.1 has no sprint bit.
fn player_input(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let sideways = r.f32()?;
    let forward = r.f32()?;
    let flags = r.u8()?;
    let mut keys = 0u8;
    if forward > 0.0 {
        keys |= 0x01;
    } else if forward < 0.0 {
        keys |= 0x02;
    }
    if sideways > 0.0 {
        keys |= 0x04;
    } else if sideways < 0.0 {
        keys |= 0x08;
    }
    if flags & 0x01 != 0 {
        keys |= 0x10;
    }
    if flags & 0x02 != 0 {
        keys |= 0x20;
    }
    let mut w = Writer::new();
    w.u8(keys);
    Ok(w.data)
}

/// 1.21.2's entity_position_sync (pinned 35), which a 1.21.1 client has no packet for, as the teleport it stands
/// in for: its 1.21.2 form (absolute, with zero velocity and no relative flags), which the writer above then
/// brings to 1.21.1's teleport_entity. The path's end is taken (a stepped path loses its steps).
// TODO(step1_21_1): a stepped path is sent as its end position only.
pub(super) fn entity_position_sync(
    state: State,
    id: i32,
    payload: &[u8],
) -> Result<Option<Vec<u8>>> {
    if state != State::Play || id != packet_ids::play::clientbound::ENTITY_POSITION_SYNC {
        return Ok(None);
    }
    let sync = play::EntityPositionSync::read(&mut Reader::new(payload))?;
    let mut w = Writer::new();
    w.varint(sync.id);
    sync.path.end().iter().for_each(|v| w.f64(*v));
    (0..3).for_each(|_| w.f64(0.0));
    w.f32(sync.yaw);
    w.f32(sync.pitch);
    w.i32(0);
    w.bool(sync.on_ground);
    Ok(Some(w.data))
}

#[cfg(test)]
mod tests {
    //! Each change written here is read back with `translate`'s 1.21.1 reader: 1.21.1's layout, 1.21.2's and back.
    use super::*;
    use crate::adapter::{ServerVersion, session::Session, translate};

    /// Through the reader and back: `payload` (1.21.1's) must come out as it went in.
    fn round_trip(id: i32, payload: &[u8]) {
        let session = Session::new(ServerVersion::V1_21_1);
        let up = translate::v1_21_1::clientbound(&session, State::Play, id, payload)
            .unwrap()
            .expect("the reader changes this packet");
        let down = clientbound(State::Play, id, &up)
            .unwrap()
            .expect("the writer changes it");
        assert_eq!(down, payload, "packet {id}");
    }

    #[test]
    fn login_finished_gets_strict_error_handling_off_and_back() {
        let profile = [7u8; 20];
        let id = packet_ids::login::clientbound::LOGIN_FINISHED;
        let down = clientbound(State::Login, id, &profile).unwrap().unwrap();
        assert_eq!(down.len(), 21);
        assert_eq!(down[20], 0);
        let session = Session::new(ServerVersion::V1_21_1);
        let up = translate::v1_21_1::clientbound(&session, State::Login, id, &down)
            .unwrap()
            .unwrap();
        assert_eq!(up, profile);
    }

    #[test]
    fn window_ids_are_bytes_again() {
        round_trip(packet_ids::play::clientbound::CONTAINER_CLOSE, &[200]);
        let mut w = Writer::new();
        w.u8(3);
        w.i16(7);
        round_trip(packet_ids::play::clientbound::CONTAINER_SET_DATA, &w.data);
        let mut w = Writer::new();
        w.i8(-1);
        w.varint(4);
        w.i16(2);
        round_trip(packet_ids::play::clientbound::CONTAINER_SET_SLOT, &w.data);
    }

    #[test]
    fn a_stopped_day_time_is_negative_again() {
        let mut w = Writer::new();
        w.i64(100);
        w.i64(-6000);
        round_trip(packet_ids::play::clientbound::SET_TIME, &w.data);
    }

    #[test]
    fn login_and_respawn_lose_the_sea_level() {
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
        round_trip(packet_ids::play::clientbound::LOGIN, &w.data);

        let mut w = Writer::new();
        w.varint(0);
        w.string("minecraft:overworld");
        w.i64(42);
        w.u8(1);
        w.i8(-1);
        w.bool(false);
        w.bool(false);
        w.bool(false);
        w.varint(0);
        w.u8(3);
        round_trip(packet_ids::play::clientbound::RESPAWN, &w.data);
    }

    #[test]
    fn a_particle_has_one_boolean() {
        let mut w = Writer::new();
        w.bool(true);
        w.f64(1.0);
        round_trip(packet_ids::play::clientbound::LEVEL_PARTICLES, &w.data);
    }

    #[test]
    fn teleports_come_back_as_bytes_and_absolute() {
        let mut w = Writer::new();
        w.varint(9);
        [1.5, 64.0, -2.0].iter().for_each(|v| w.f64(*v));
        w.u8(64);
        w.u8(0);
        w.bool(true);
        round_trip(packet_ids::play::clientbound::TELEPORT_ENTITY, &w.data);
    }

    #[test]
    fn a_player_position_has_its_teleport_id_first_and_the_flags_as_a_byte() {
        let mut w = Writer::new();
        [1.0, 2.0, 3.0].iter().for_each(|v| w.f64(*v));
        w.f32(90.0);
        w.f32(-15.0);
        w.u8(0x18);
        w.varint(12);
        round_trip(packet_ids::play::clientbound::PLAYER_POSITION, &w.data);
    }

    #[test]
    fn a_held_slot_is_a_byte() {
        let mut w = Writer::new();
        w.i8(3);
        round_trip(packet_ids::play::clientbound::SET_HELD_SLOT, &w.data);
    }

    #[test]
    fn a_player_entry_without_list_order_or_hat_is_1_21_1_s() {
        // 1.21.2's add player with a list order (0x40) and a hat (0x80): both go.
        let mut w = Writer::new();
        w.u8(0x01 | 0x40 | 0x80);
        w.varint(1);
        w.uuid(5);
        w.string("name");
        w.varint(0);
        w.varint(3);
        w.bool(true);
        let down = clientbound(
            State::Play,
            packet_ids::play::clientbound::PLAYER_INFO_UPDATE,
            &w.data,
        )
        .unwrap()
        .unwrap();
        let mut expected = Writer::new();
        expected.u8(0x01);
        expected.varint(1);
        expected.uuid(5);
        expected.string("name");
        expected.varint(0);
        assert_eq!(down, expected.data);
    }

    #[test]
    fn a_1_21_1_on_ground_boolean_is_a_flags_byte() {
        let mut w = Writer::new();
        [1.0, 2.0, 3.0].iter().for_each(|v| w.f64(*v));
        w.bool(true);
        let up = serverbound(
            State::Play,
            packet_ids::play::serverbound::MOVE_PLAYER_POS,
            &w.data,
        )
        .unwrap()
        .unwrap();
        let mut r = Reader::new(&up);
        assert_eq!(
            [r.f64().unwrap(), r.f64().unwrap(), r.f64().unwrap()],
            [1.0, 2.0, 3.0]
        );
        assert_eq!(r.u8().unwrap(), 1);
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn steering_becomes_key_flags() {
        let mut w = Writer::new();
        w.f32(1.0); // left
        w.f32(-1.0); // backward
        w.u8(0x03); // jump and unmount
        let up = serverbound(
            State::Play,
            packet_ids::play::serverbound::PLAYER_INPUT,
            &w.data,
        )
        .unwrap()
        .unwrap();
        assert_eq!(up, vec![0x02 | 0x04 | 0x10 | 0x20]);
    }

    #[test]
    fn a_position_sync_is_the_teleport_it_stands_in_for() {
        let body = play::Clientbound::EntityPositionSync(play::EntityPositionSync {
            id: 4,
            path: play::PositionPath::Linear([1.5, 70.0, -3.0]),
            yaw: 45.0,
            pitch: 10.0,
            on_ground: true,
        })
        .encode();
        let mut r = Reader::new(&body);
        r.varint().unwrap();
        let teleport = entity_position_sync(
            State::Play,
            packet_ids::play::clientbound::ENTITY_POSITION_SYNC,
            r.rest(),
        )
        .unwrap()
        .unwrap();
        // Back to 1.21.1's teleport_entity: the angles are bytes of a turn again.
        let down = clientbound(
            State::Play,
            packet_ids::play::clientbound::TELEPORT_ENTITY,
            &teleport,
        )
        .unwrap()
        .unwrap();
        let mut r = Reader::new(&down);
        assert_eq!(r.varint().unwrap(), 4);
        assert_eq!(
            [r.f64().unwrap(), r.f64().unwrap(), r.f64().unwrap()],
            [1.5, 70.0, -3.0]
        );
        assert_eq!((r.u8().unwrap(), r.u8().unwrap()), (32, 7));
        assert!(r.bool().unwrap());
    }

    #[test]
    fn an_explosion_is_1_21_1_s_bytes() {
        // 1.21.2 layout in: centre, a knockback, the particle (id 5, no data), the sound inline ("a:b", no range).
        let mut i = Writer::new();
        [1.0, 2.0, 3.0].iter().for_each(|v| i.f64(*v));
        i.bool(true);
        [0.5, 0.0, -1.0].iter().for_each(|v| i.f64(*v));
        i.varint(5);
        i.varint(0);
        i.string("a:b");
        i.bool(false);
        let out = explode(&i.data).unwrap();
        // 1.21.1 layout expected: centre, strength, no records, the motion, keep, the particle twice, the sound.
        let mut e = Writer::new();
        [1.0, 2.0, 3.0].iter().for_each(|v| e.f64(*v));
        e.f32(3.0);
        e.varint(0);
        [0.5f32, 0.0, -1.0].iter().for_each(|v| e.f32(*v));
        e.varint(0);
        e.varint(5);
        e.varint(5);
        e.varint(0);
        e.string("a:b");
        e.bool(false);
        assert_eq!(out, e.data);
    }
}
