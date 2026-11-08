//! 1.21.6 to 1.21.5: the mirror of `translate`'s `v1_21_5` (1.21.5 to 1.21.6), for a [`super::ClientSession`] whose
//! client is 1.21.5 (roadmap `older-clients`).
//!
//! Clientbound nothing changes (see `translate::v1_21_5`), so there is no packet to write here. Serverbound, a
//! 1.21.5 client's `player_command` has a sneak press and release (actions 0 and 1) that 1.21.6 no longer has;
//! the sneak is in `player_input` (bit 0x20) for both. The press and release are dropped, the other actions are
//! numbered two up in 1.21.5 and come back down here.
//!
//! Item components are written in 1.21.5's layout by `item.rs` (`ids::Quirks::components_before_1_21_6`).
//!
//! TODO(step3): 1.21.6's dialog registries are not written for 1.21.5; see translate's `v1_21_5`.
use crate::{
    Result, State,
    buf::{Reader, Writer},
    ids as packet_ids,
};

/// What becomes of a payload the 1.21.5 client sent.
#[derive(Debug, PartialEq)]
pub(super) enum Out {
    /// Not one of the packets this step rewrites.
    Untouched,
    Rewritten(Vec<u8>),
    /// A sneak press or release (or an action 1.21.5 has no number for), which the pinned version doesn't read.
    Dropped,
}

/// Nothing: no packet the pinned version sends is laid out differently for 1.21.5 (see `translate::v1_21_5`).
pub(super) fn clientbound(_state: State, _id: i32, _payload: &[u8]) -> Result<Option<Vec<u8>>> {
    Ok(None)
}

/// The 1.21.5 serverbound packet `id` (`payload` as the client sent it, under the pinned id) in the pinned layout.
pub(super) fn serverbound(state: State, id: i32, payload: &[u8]) -> Result<Out> {
    use packet_ids::play::serverbound as sb;
    Ok(match (state, id) {
        (State::Play, sb::PLAYER_COMMAND) => player_command(payload)?,
        _ => Out::Untouched,
    })
}

/// 1.21.5's actions 2 to 8 are the pinned 0 to 6; its press (0) and release (1) of the sneak key go.
fn player_command(payload: &[u8]) -> Result<Out> {
    let mut r = Reader::new(payload);
    let entity = r.varint()?;
    let action = r.varint()?;
    let jump = r.varint()?;
    let action = match action {
        2..=8 => action - 2,
        _ => return Ok(Out::Dropped),
    };
    let mut w = Writer::new();
    w.varint(entity);
    w.varint(action);
    w.varint(jump);
    Ok(Out::Rewritten(w.data))
}

#[cfg(test)]
mod tests {
    //! Each change is written here and read back with `translate`'s 1.21.5 reader: the round trip.
    use super::*;
    use crate::adapter::{ServerVersion, session::Session, translate};

    fn command(entity: i32, action: i32, jump: i32) -> Vec<u8> {
        let mut w = Writer::new();
        w.varint(entity);
        w.varint(action);
        w.varint(jump);
        w.data
    }

    #[test]
    fn a_1_21_5_leave_bed_is_the_pinned_zero() {
        let id = packet_ids::play::serverbound::PLAYER_COMMAND;
        assert_eq!(
            serverbound(State::Play, id, &command(4, 2, 0)).unwrap(),
            Out::Rewritten(command(4, 0, 0))
        );
    }

    #[test]
    fn a_sneak_press_and_release_are_dropped() {
        let id = packet_ids::play::serverbound::PLAYER_COMMAND;
        assert_eq!(
            serverbound(State::Play, id, &command(4, 0, 0)).unwrap(),
            Out::Dropped
        );
        assert_eq!(
            serverbound(State::Play, id, &command(4, 1, 0)).unwrap(),
            Out::Dropped
        );
    }

    #[test]
    fn other_packets_are_untouched() {
        let id = packet_ids::play::serverbound::PLAYER_INPUT;
        assert_eq!(
            serverbound(State::Play, id, &[0x20]).unwrap(),
            Out::Untouched
        );
    }

    #[test]
    fn a_pinned_command_reads_back_as_it_was() {
        let session = Session::new(ServerVersion::V1_21_5);
        let id = packet_ids::play::serverbound::PLAYER_COMMAND;
        for action in 0..=6 {
            let pinned = command(9, action, 0);
            let (_, older) =
                translate::client_layout_v1_21_5::player_command(&session, id, &pinned)
                    .unwrap()
                    .unwrap();
            assert_eq!(
                serverbound(State::Play, id, &older).unwrap(),
                Out::Rewritten(pinned)
            );
        }
    }
}
