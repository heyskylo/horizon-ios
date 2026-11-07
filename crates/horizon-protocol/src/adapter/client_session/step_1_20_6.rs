//! 1.21's packets in 1.20.6's layout for a 1.20.6 client (and 1.20.5): the mirror of `translate`'s `v1_20_6`
//! (1.20.6 to 1.21). Clientbound: level_particles puts the particle before its flags, explode's holder sound becomes
//! an inline sound event (the holder id goes), projectile_power sends a zero vector (1.21 sends only the power), and
//! merchant_offers' trade items become 1.20.6 stacks, an empty stack for a missing second cost. Serverbound, a 1.20.6
//! client's use_item gets the head's yaw and pitch, which 1.21 reads and 1.20.6 does not send.
//!
//! The pinned ids are renumbered down before this step, so the stacks are in the client's numbering; their components
//! are measured with the stack codec under `ids` renumbered up, as [`translate::v1_20_6::slot`] does.
use crate::{
    Result, State,
    adapter::{
        ids::{self, Ids, Way},
        translate::v1_20_6::{LEVEL_PARTICLES_HEAD, slot},
    },
    buf::{Codec, Reader, Writer},
    ids as packet_ids,
    item::Slot,
};

/// The clientbound packet `id` (`payload` in 1.21's layout) in 1.20.6's layout; `None` when laid out alike.
/// `ids` is the client's, which the stacks' item and component ids are numbered in.
pub(super) fn clientbound(
    state: State,
    id: i32,
    payload: &[u8],
    ids: &'static Ids,
) -> Result<Option<Vec<u8>>> {
    use packet_ids::play::clientbound as cb;
    if state != State::Play {
        return Ok(None);
    }
    Ok(match id {
        cb::LEVEL_PARTICLES => Some(level_particles(payload)?),
        cb::EXPLODE => Some(explode(payload)?),
        cb::PROJECTILE_POWER => Some(projectile_power(payload)?),
        cb::MERCHANT_OFFERS => Some(merchant_offers(payload, ids)?),
        _ => None,
    })
}

/// The serverbound `use_item` the 1.20.6 client sent (`payload`, under the pinned id `id`) with the head's yaw and
/// pitch, which 1.21 reads and 1.20.6 does not send: the caller passes the client's last rotation. `None` when `id`
/// is not `use_item`.
// TODO(step3): the last rotation is the one the client sent last (Place in ClientSession); the server sets the
// player's head to it before using the item, so a rotation the client has not sent yet is not known here.
pub(super) fn serverbound(
    state: State,
    id: i32,
    payload: &[u8],
    (yaw, pitch): (f32, f32),
) -> Result<Option<Vec<u8>>> {
    use packet_ids::play::serverbound as sb;
    if (state, id) != (State::Play, sb::USE_ITEM) {
        return Ok(None);
    }
    let mut r = Reader::new(payload);
    let mut w = Writer::new();
    w.varint(r.varint()?);
    w.varint(r.varint()?);
    w.f32(yaw);
    w.f32(pitch);
    Ok(Some(w.data))
}

/// level_particles: 1.21's count is followed by the particle; 1.20.6 has the particle first.
fn level_particles(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let head = r.bytes(LEVEL_PARTICLES_HEAD)?;
    let particle = r.varint()?;
    let mut w = Writer::new();
    w.varint(particle);
    w.bytes(head);
    w.bytes(r.rest());
    Ok(w.data)
}

/// explode: 1.21's sound holder (a VarInt, 0 for an inline sound event) goes; the inline sound follows as it is.
// TODO(step3): a registry sound (a holder id other than 0) has no name in Horizon's tables; the generic explosion
// sound is sent in its place, which is what most explosions play.
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
    let holder = r.varint()?;
    if holder == 0 {
        w.bytes(r.rest());
    } else {
        w.string("minecraft:entity.generic.explode");
        w.bool(false);
    }
    Ok(w.data)
}

/// projectile_power: 1.21 sends the power's length; 1.20.6 wants the vector.
// TODO(step3): the vector is along the direction the projectile travels, which this step does not know; a zero
// vector is sent, so the client's prediction of the projectile's push is off until the next move packet.
fn projectile_power(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let id = r.varint()?;
    r.f64()?;
    let mut w = Writer::new();
    w.varint(id);
    w.f64(0.0);
    w.f64(0.0);
    w.f64(0.0);
    Ok(w.data)
}

/// merchant_offers: each 1.21 trade item is a stack (an empty one for a missing cost), the optional second cost
/// becomes an empty stack when absent, and the output stack, the rest of each offer and the list's end pass.
fn merchant_offers(payload: &[u8], ids: &'static Ids) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let mut w = Writer::new();
    w.varint(r.varint()?);
    let size = r.varint()?;
    w.varint(size);
    for _ in 0..size.max(0) {
        w.bytes(&trade_item(payload, &mut r, ids)?);
        w.bytes(slot(payload, &mut r, ids)?);
        if r.bool()? {
            w.bytes(&trade_item(payload, &mut r, ids)?);
        } else {
            w.varint(0);
        }
        w.bytes(r.bytes(1 + 4 * 4 + 4 + 4)?);
    }
    w.bytes(r.rest());
    Ok(w.data)
}

/// A 1.21 trade item (item id, count, components it adds) as a 1.20.6 stack, which takes no removed components.
/// The components are measured with the stack codec, read under `ids` renumbered up, and kept as they are.
fn trade_item(payload: &[u8], r: &mut Reader, ids: &'static Ids) -> Result<Vec<u8>> {
    let item = r.varint()?;
    let count = r.varint()?;
    let added = r.varint()?;
    if count <= 0 {
        if added != 0 {
            return Err(crate::Error::new("an empty trade item with components"));
        }
        return Ok(vec![0]);
    }
    let mut head = Writer::new();
    head.varint(count);
    head.varint(item);
    head.varint(added);
    head.varint(0);
    let head_len = head.data.len();
    let mut stack = head.data;
    stack.extend_from_slice(&payload[r.position()..]);
    let len = if added == 0 {
        head_len
    } else {
        let _up = ids::renumbering(ids, Way::Up);
        let mut m = Reader::new(&stack);
        Slot::read(&mut m)?;
        m.position()
    };
    r.bytes(len - head_len)?;
    stack.truncate(len);
    Ok(stack)
}

#[cfg(test)]
mod tests {
    //! Each change written here is read back with `translate`'s 1.20.6 reader: the mirror, both ways.
    use super::*;
    use crate::adapter::{ServerVersion, session::Session, translate};

    /// A 1.21 `level_particles` for a particle with no data, as 1.21 sends it.
    fn level_particles_1_21() -> Vec<u8> {
        let mut w = Writer::new();
        w.bool(false);
        for v in [1.5, 64.0, -2.25] {
            w.f64(v);
        }
        for v in [0.5, 0.5, 0.5, 0.1] {
            w.f32(v);
        }
        w.i32(3);
        w.varint(7); // the particle
        w.data
    }

    #[test]
    fn a_particle_comes_before_its_flags_in_1_20_6() {
        let session = Session::new(ServerVersion::V1_20_6);
        let original = level_particles_1_21();
        let id = packet_ids::play::clientbound::LEVEL_PARTICLES;
        let down = clientbound(
            State::Play,
            id,
            &original,
            ServerVersion::V1_20_6.ids().unwrap(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(Reader::new(&down).varint().unwrap(), 7);
        let up = translate::v1_20_6::clientbound(&session, State::Play, id, &down)
            .unwrap()
            .unwrap();
        assert_eq!(up, original);
    }

    #[test]
    fn an_inline_explosion_sound_is_1_20_6_s_alone() {
        let session = Session::new(ServerVersion::V1_20_6);
        let mut w = Writer::new();
        for v in [1.0, 2.0, 3.0] {
            w.f64(v);
        }
        w.f32(4.0);
        w.varint(1);
        w.bytes(&[1, 2, 3]);
        for v in [0.1f32, 0.2, 0.3] {
            w.f32(v);
        }
        w.varint(0); // block interaction
        w.varint(2); // small particle
        w.varint(3); // large particle
        w.varint(0); // the inline sound (holder 0 in 1.21)
        w.string("minecraft:entity.generic.explode");
        w.bool(false);
        let original = w.data;
        let id = packet_ids::play::clientbound::EXPLODE;
        let down = clientbound(
            State::Play,
            id,
            &original,
            ServerVersion::V1_20_6.ids().unwrap(),
        )
        .unwrap()
        .unwrap();
        let mut r = Reader::new(&down);
        r.bytes(24 + 4 + 1 + 3 + 12).unwrap();
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.varint().unwrap(), 2);
        assert_eq!(r.varint().unwrap(), 3);
        assert_eq!(r.string(32767).unwrap(), "minecraft:entity.generic.explode");
        let up = translate::v1_20_6::clientbound(&session, State::Play, id, &down)
            .unwrap()
            .unwrap();
        assert_eq!(up, original);
    }

    #[test]
    fn a_projectile_power_is_sent_as_a_vector_of_zero_and_read_as_its_length() {
        let session = Session::new(ServerVersion::V1_20_6);
        let id = packet_ids::play::clientbound::PROJECTILE_POWER;
        let mut w = Writer::new();
        w.varint(4);
        w.f64(5.0);
        let down = clientbound(
            State::Play,
            id,
            &w.data,
            ServerVersion::V1_20_6.ids().unwrap(),
        )
        .unwrap()
        .unwrap();
        let mut r = Reader::new(&down);
        assert_eq!(r.varint().unwrap(), 4);
        assert_eq!(
            [r.f64().unwrap(), r.f64().unwrap(), r.f64().unwrap()],
            [0.0; 3]
        );
        let mut v = Writer::new();
        v.varint(4);
        v.f64(0.0);
        v.f64(3.0);
        v.f64(4.0);
        let up = translate::v1_20_6::clientbound(&session, State::Play, id, &v.data)
            .unwrap()
            .unwrap();
        let mut r = Reader::new(&up);
        assert_eq!(r.varint().unwrap(), 4);
        assert_eq!(r.f64().unwrap(), 5.0);
    }

    #[test]
    fn a_merchant_offer_round_trips_with_and_without_a_second_cost() {
        let session = Session::new(ServerVersion::V1_20_6);
        let ids = ServerVersion::V1_20_6.ids().unwrap();
        let id = packet_ids::play::clientbound::MERCHANT_OFFERS;
        for second in [true, false] {
            let mut w = Writer::new();
            w.varint(7); // the window
            w.varint(1); // one offer
            w.varint(42); // the first cost's item
            w.varint(2); // its count
            w.varint(0); // no components
            w.varint(1); // the output: a count of 1
            w.varint(9);
            w.varint(0);
            w.varint(0);
            w.bool(second);
            if second {
                w.varint(5);
                w.varint(1);
                w.varint(0);
            }
            w.bool(false); // not disabled
            w.i32(0);
            w.i32(12);
            w.i32(1);
            w.i32(0);
            w.f32(0.05);
            w.i32(0);
            w.varint(2); // level
            w.varint(0); // experience
            w.bool(true);
            w.bool(true);
            let original = w.data;
            let down = clientbound(State::Play, id, &original, ids)
                .unwrap()
                .unwrap();
            let up = translate::v1_20_6::clientbound(&session, State::Play, id, &down)
                .unwrap()
                .unwrap();
            assert_eq!(up, original);
        }
    }

    #[test]
    fn a_1_20_6_use_item_gets_the_clients_last_rotation() {
        let session = Session::new(ServerVersion::V1_20_6);
        let id = packet_ids::play::serverbound::USE_ITEM;
        let mut w = Writer::new();
        w.varint(0); // main hand
        w.varint(5); // the sequence
        let up = serverbound(State::Play, id, &w.data, (1.5, -2.0))
            .unwrap()
            .unwrap();
        let mut r = Reader::new(&up);
        assert_eq!((r.varint().unwrap(), r.varint().unwrap()), (0, 5));
        assert_eq!((r.f32().unwrap(), r.f32().unwrap()), (1.5, -2.0));
        let (mapped, back) = translate::serverbound(&session, State::Play, id, &up)
            .unwrap()
            .unwrap();
        assert_eq!(
            Some(mapped),
            ServerVersion::V1_20_6.serverbound_id(State::Play, id)
        );
        assert_eq!(back, w.data);
    }
}
