//! 1.21.8's packets in 1.21.6's layout for a 1.21.6 client (and 1.21.5): the mirror of `translate`'s `mod v1_21_6`
//! (1.21.6 to 1.21.8). Two packets differ, and entity data is laid out alike.
//! - set_equipment: 1.21.8 added the saddle slot (7) to its equipment enum. A 1.21.6 client has no such slot, so
//!   its entries are removed. A packet with nothing else in it is dropped.
//! - serverbound test_instance_block_action: a test is an identifier in 1.21.8 and a VarInt id in 1.21.6. A
//!   packet that names a test is dropped (see `translate::v1_21_6::names_test`); one that names none is laid
//!   out alike and passes.
//!
//! Entity data: no index or type changed between 1.21.6 and 1.21.8. The serializer ids differ only from 1.21.9.
use crate::{
    Result, State,
    adapter::translate,
    buf::{Codec, Reader, Writer},
    ids as packet_ids,
    packets::play,
};

/// What becomes of a payload the 1.21.8 side sent or the client was sent.
#[derive(Debug, PartialEq)]
pub(super) enum Out {
    /// Laid out alike: the payload passes as it is.
    Untouched,
    Rewritten(Vec<u8>),
    /// The 1.21.6 client can't read it: it is not sent (clientbound) or not read (serverbound).
    Dropped,
}

/// EquipmentSlot.SADDLE, new in 1.21.8.
const SADDLE: u8 = 7;

/// The pinned clientbound packet `id` (`payload` in 1.21.8's layout) in 1.21.6's layout.
pub(super) fn clientbound(state: State, id: i32, payload: &[u8]) -> Result<Out> {
    use packet_ids::play::clientbound as cb;
    if (state, id) != (State::Play, cb::SET_EQUIPMENT) {
        return Ok(Out::Untouched);
    }
    let mut e = play::SetEquipment::read(&mut Reader::new(payload))?;
    let before = e.slots.len();
    e.slots.retain(|(slot, _)| *slot != SADDLE);
    if e.slots.len() == before {
        return Ok(Out::Untouched);
    }
    if e.slots.is_empty() {
        return Ok(Out::Dropped);
    }
    let mut w = Writer::new();
    play::SetEquipment::write(&e, &mut w);
    Ok(Out::Rewritten(w.data))
}

/// The pinned serverbound packet `id` as the 1.21.6 client sent it (`payload`) in 26.3's layout, for the server.
pub(super) fn serverbound(state: State, id: i32, payload: &[u8]) -> Result<Out> {
    use packet_ids::play::serverbound as sb;
    if (state, id) == (State::Play, sb::TEST_INSTANCE_BLOCK_ACTION)
        && translate::v1_21_6::names_test(payload)?
    {
        return Ok(Out::Dropped);
    }
    Ok(Out::Untouched)
}

#[cfg(test)]
mod tests {
    //! Each packet written here is read back with `translate`'s 1.21.6 reader (the serverbound one, which
    //! takes the 1.21.8 layout the client sends) or the pinned codec for the clientbound one.
    use super::*;
    use crate::{
        adapter::{ServerVersion, session::Session},
        item::Stack,
    };

    #[test]
    fn set_equipment_loses_the_saddle_and_keeps_the_other_slots() {
        let stack = Stack::new(1, 1);
        let p = play::SetEquipment {
            id: 4,
            slots: vec![
                (0, Some(stack.clone())),
                (SADDLE, Some(stack.clone())),
                (5, None),
            ],
        };
        let mut w = Writer::new();
        play::SetEquipment::write(&p, &mut w);
        let Out::Rewritten(body) = clientbound(
            State::Play,
            packet_ids::play::clientbound::SET_EQUIPMENT,
            &w.data,
        )
        .unwrap() else {
            panic!("the saddle should be removed");
        };
        // The top bit of the last entry is clear again: read back, the list is the two others.
        let back = play::SetEquipment::read(&mut Reader::new(&body)).unwrap();
        assert_eq!(
            back,
            play::SetEquipment {
                id: 4,
                slots: vec![(0, Some(stack)), (5, None)],
            }
        );
    }

    #[test]
    fn set_equipment_of_only_a_saddle_is_dropped() {
        let p = play::SetEquipment {
            id: 4,
            slots: vec![(SADDLE, Some(Stack::new(1, 1)))],
        };
        let mut w = Writer::new();
        play::SetEquipment::write(&p, &mut w);
        assert_eq!(
            clientbound(
                State::Play,
                packet_ids::play::clientbound::SET_EQUIPMENT,
                &w.data
            )
            .unwrap(),
            Out::Dropped
        );
    }

    #[test]
    fn set_equipment_without_a_saddle_is_untouched() {
        let p = play::SetEquipment {
            id: 4,
            slots: vec![(6, None)],
        };
        let mut w = Writer::new();
        play::SetEquipment::write(&p, &mut w);
        assert_eq!(
            clientbound(
                State::Play,
                packet_ids::play::clientbound::SET_EQUIPMENT,
                &w.data
            )
            .unwrap(),
            Out::Untouched
        );
    }

    /// A serverbound test_instance_block_action with the given test, in 26.3's layout.
    fn test_action(test: Option<&str>) -> Vec<u8> {
        let p = play::TestInstanceBlockAction {
            pos: [1, 2, 3],
            action: 6,
            data: play::TestInstanceData {
                test: test.map(str::to_string),
                size: [3, 4, 5],
                rotation: 2,
                ignore_entities: true,
                status: 1,
                error_message: None,
            },
        };
        let mut w = Writer::new();
        play::TestInstanceBlockAction::write(&p, &mut w);
        w.data
    }

    #[test]
    fn a_test_action_naming_no_test_passes_both_ways() {
        let session = Session::new(ServerVersion::V1_21_6);
        let payload = test_action(None);
        let sb = packet_ids::play::serverbound::TEST_INSTANCE_BLOCK_ACTION;
        // The client's side: the reader passes it, renumbered to 1.21.6's id.
        let (_, read) = translate::serverbound(&session, State::Play, sb, &payload)
            .unwrap()
            .unwrap();
        assert_eq!(read, payload);
        // The writer (the client's own packet, back up to the pinned layout) leaves it alone.
        assert_eq!(
            serverbound(State::Play, sb, &payload).unwrap(),
            Out::Untouched
        );
    }

    #[test]
    fn a_test_action_naming_a_test_is_dropped_both_ways() {
        let session = Session::new(ServerVersion::V1_21_6);
        let payload = test_action(Some("minecraft:always_pass"));
        let sb = packet_ids::play::serverbound::TEST_INSTANCE_BLOCK_ACTION;
        assert_eq!(
            translate::serverbound(&session, State::Play, sb, &payload).unwrap(),
            None
        );
        assert_eq!(
            serverbound(State::Play, sb, &payload).unwrap(),
            Out::Dropped
        );
    }
}
