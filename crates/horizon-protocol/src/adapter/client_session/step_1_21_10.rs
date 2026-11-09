//! 1.21.11's packets in 1.21.10's layout for a 1.21.10 client (and 1.21.9): the mirror of `translate`'s
//! `mod v1_21_10` (1.21.10 to 1.21.11). Only entity data changed. A bee's and a wolf's anger end went from an
//! INT to a LONG, and a player's or mannequin's main arm from a BYTE (0 left, 1 right) to a HUMANOID_ARM.
//! No packet changes layout: 1.21.10's packets are 1.21.11's but for `horse_screen_open`, which the id tables
//! pair with `mount_screen_open`.
//!
//! Entity data goes through [`super::step_1_21_11::entity_data`] first (it undoes 26.1's index shift), then
//! this step. The serverbound `interact` is the 1.21.11 step's too (see [`super::step_1_21_11::serverbound`]).
use crate::{
    Result, State,
    adapter::translate::v1_21_10::{BEE, MANNEQUIN, PLAYER, WOLF},
    entity_data::{Value, serializer},
    packets::play,
};

/// Nothing: no packet of 1.21.11 is laid out differently for 1.21.10 (see the module doc).
pub(super) fn clientbound(_state: State, _id: i32, _payload: &[u8]) -> Result<Option<Vec<u8>>> {
    Ok(None)
}

/// Entity data in 1.21.10's types, from 1.21.11's (translate's `entity_data`, reversed). `kind` is the entity's
/// type in the pinned ids, `None` when the caller doesn't know it, which leaves the data as it is.
pub(super) fn entity_data(kind: Option<i32>, data: &mut play::SetEntityData) {
    let Some(kind) = kind else {
        return;
    };
    for (index, ser, value) in &mut data.values {
        match (kind, *index, &*value) {
            (BEE, 18, Value::Long(v)) | (WOLF, 21, Value::Long(v)) => {
                *value = Value::Int((*v).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32);
                *ser = serializer::INT;
            }
            (PLAYER | MANNEQUIN, 15, Value::Id(v)) => {
                *value = Value::Byte(i8::try_from(*v).unwrap_or(0));
                *ser = serializer::BYTE;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    //! Each change written here is read back with `translate`'s 1.21.10 reader: the full chain, 26.1's
    //! layout from 1.21.10's and back again.
    use super::*;
    use crate::adapter::{ServerVersion, session::Session, translate};

    /// Translate's way up: 1.21.10's entity data to 26.1's, the 1.21.10 step first.
    fn up(session: &Session, d: &mut play::SetEntityData) {
        translate::v1_21_10::entity_data(session, d);
        translate::v1_21_11::entity_data(session, d);
    }

    /// The way back: 26.1's to 1.21.10's, the index shift undone first.
    fn down(kind: Option<i32>, d: &mut play::SetEntityData) {
        super::super::step_1_21_11::entity_data(kind, d);
        entity_data(kind, d);
    }

    #[test]
    fn a_bee_s_anger_and_a_player_s_arm_are_1_21_10_s_again() {
        let session = Session::new(ServerVersion::V1_21_10);
        session.kinds.lock().unwrap().insert(1, BEE);
        session.kinds.lock().unwrap().insert(2, PLAYER);
        let bee = play::SetEntityData {
            id: 1,
            values: vec![
                (16, serializer::BOOLEAN, Value::Bool(false)),
                (18, serializer::INT, Value::Int(400)),
            ],
        };
        let mut d = bee.clone();
        up(&session, &mut d);
        assert_eq!(d.values[1], (19, serializer::LONG, Value::Long(400)));
        down(Some(BEE), &mut d);
        assert_eq!(d, bee);

        let player = play::SetEntityData {
            id: 2,
            values: vec![(15, serializer::BYTE, Value::Byte(1))],
        };
        let mut d = player.clone();
        up(&session, &mut d);
        assert_eq!(d.values, vec![(15, serializer::HUMANOID_ARM, Value::Id(1))]);
        down(Some(PLAYER), &mut d);
        assert_eq!(d, player);
    }

    #[test]
    fn a_wolf_s_anger_is_an_int_again() {
        let session = Session::new(ServerVersion::V1_21_10);
        session.kinds.lock().unwrap().insert(3, WOLF);
        let wolf = play::SetEntityData {
            id: 3,
            values: vec![(21, serializer::INT, Value::Int(-3))],
        };
        let mut d = wolf.clone();
        up(&session, &mut d);
        down(Some(WOLF), &mut d);
        assert_eq!(d, wolf);
    }

    #[test]
    fn an_unknown_kind_is_left_alone() {
        let mut d = play::SetEntityData {
            id: 4,
            values: vec![(18, serializer::LONG, Value::Long(9))],
        };
        entity_data(None, &mut d);
        assert_eq!(d.values, vec![(18, serializer::LONG, Value::Long(9))]);
    }
}
