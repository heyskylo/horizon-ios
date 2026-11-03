//! 1.21.8 to 1.21.9, from vanilla's packet codecs and EntityDataSerializers registrations.
use super::*;

/// Changed packet layouts, before the later release steps and registry renumbering.
pub(super) fn clientbound(
    session: &Session,
    state: State,
    id: i32,
    payload: &[u8],
) -> Result<Option<Vec<u8>>> {
    if state != State::Play {
        return Ok(None);
    }
    use packet_ids::play::clientbound as cb;
    let mut r = Reader::new(payload);
    let mut w = Writer::new();
    match id {
        cb::ADD_ENTITY => {
            let entity = r.varint()?;
            let uuid = r.uuid()?;
            let kind = r.varint()?;
            let position = <crate::buf::Vec3d>::read(&mut r)?;
            let angles = [r.i8()?, r.i8()?, r.i8()?];
            let data = r.varint()?;
            let velocity = velocity(&mut r)?;
            w.varint(entity);
            w.uuid(uuid);
            w.varint(kind);
            crate::buf::Vec3d::write(&position, &mut w);
            crate::buf::LpVec3::write(&velocity, &mut w);
            angles.iter().for_each(|&angle| w.i8(angle));
            w.varint(data);
        }
        cb::SET_ENTITY_MOTION => {
            w.varint(r.varint()?);
            crate::buf::LpVec3::write(&velocity(&mut r)?, &mut w);
        }
        cb::PLAYER_ROTATION => {
            w.f32(r.f32()?);
            w.bool(false);
            w.f32(r.f32()?);
            w.bool(false);
        }
        cb::SET_DEFAULT_SPAWN_POSITION => {
            let pos = crate::buf::Pos::read(&mut r)?;
            w.string(&session.dimension.lock().unwrap());
            crate::buf::Pos::write(&pos, &mut w);
            w.f32(r.f32()?);
            w.f32(0.0); // No pitch was sent before RespawnData.
        }
        cb::EXPLODE => {
            crate::buf::Vec3d::write(&crate::buf::Vec3d::read(&mut r)?, &mut w);
            // The old packet supplies neither radius nor destroyed-block count. Its explicit centre
            // particle and knockback still play; no new block particles can be reconstructed.
            w.f32(0.0);
            w.i32(0);
            w.bytes(r.rest());
            w.varint(0); // WeightedList of block particles, added in 1.21.9.
        }
        _ => return Ok(None),
    }
    Ok(Some(w.data))
}

/// ClientboundAddEntityPacket/ClientboundSetEntityMotionPacket used three signed shorts at 1/8000.
fn velocity(r: &mut Reader) -> Result<[f64; 3]> {
    Ok([r.i16()?, r.i16()?, r.i16()?].map(|v| f64::from(v) / 8000.0))
}

/// Moving the main arm and skin mask to Avatar moved these four player fields, but not the shoulders.
/// The later 1.21.11 step changes the arm's serializer after it has reached index 15 here.
pub(super) fn entity_data(session: &Session, data: &mut play::SetEntityData) {
    const PLAYER: i32 = 159; // Pinned minecraft:player.
    if session.kinds.lock().unwrap().get(&data.id) != Some(&PLAYER) {
        return;
    }
    for (index, _, _) in &mut data.values {
        *index = match *index {
            15 => 17, // absorption
            16 => 18, // score
            17 => 16, // skin mask
            18 => 15, // main arm
            other => other,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity_data::{Value, serializer};

    fn translated(id: i32, payload: &[u8]) -> play::Clientbound {
        let current = translated_from(ServerVersion::V1_21_8, id, payload);
        assert_eq!(
            current,
            translated_from(ServerVersion::V1_21_6, id, payload)
        );
        current
    }

    fn translated_from(version: ServerVersion, id: i32, payload: &[u8]) -> play::Clientbound {
        let session = Session::new(version);
        let (id, body) = super::super::clientbound(&session, State::Play, id, payload)
            .unwrap()
            .unwrap();
        play::Clientbound::decode(id, &body.unwrap()).unwrap()
    }

    #[test]
    fn spawn_and_motion_convert_signed_short_velocity_before_the_angles() {
        let mut w = Writer::new();
        w.varint(42);
        w.uuid(7);
        w.varint(49); // 1.21.8's falling block, pinned id 52.
        crate::buf::Vec3d::write(&[1.0, 64.0, -2.0], &mut w);
        for angle in [-64, 32, -128] {
            w.i8(angle);
        }
        w.varint(0);
        for speed in [-8000, 4000, 0] {
            w.i16(speed);
        }
        let play::Clientbound::AddEntity(e) =
            translated(packet_ids::play::clientbound::ADD_ENTITY, &w.data)
        else {
            panic!("spawn");
        };
        assert_eq!((e.id, e.kind, e.position), (42, 52, [1.0, 64.0, -2.0]));
        assert_eq!((e.pitch, e.yaw, e.head_yaw, e.data), (-64, 32, -128, 0));
        assert!((e.velocity[0] + 1.0).abs() < 0.001);
        assert!((e.velocity[1] - 0.5).abs() < 0.001);
        assert!(e.velocity[2].abs() < 0.001);
        let play::Clientbound::SetEntityMotion(m) = translated(
            packet_ids::play::clientbound::SET_ENTITY_MOTION,
            &[42, 0xe0, 0xc0, 0x0f, 0xa0, 0, 0],
        ) else {
            panic!("motion");
        };
        assert_eq!(m.id, 42);
        assert!((m.velocity[0] + 1.0).abs() < 0.001);
        assert!((m.velocity[1] - 0.5).abs() < 0.001);
    }

    #[test]
    fn player_rotation_is_absolute_and_spawn_uses_the_current_dimension() {
        let mut w = Writer::new();
        w.f32(90.0);
        w.f32(-30.0);
        let play::Clientbound::PlayerRotation(p) =
            translated(packet_ids::play::clientbound::PLAYER_ROTATION, &w.data)
        else {
            panic!("rotation");
        };
        assert_eq!(
            (p.yaw, p.pitch, p.relative_yaw, p.relative_pitch),
            (90.0, -30.0, false, false)
        );
        let session = Session::new(ServerVersion::V1_21_8);
        *session.dimension.lock().unwrap() = "minecraft:the_nether".into();
        let mut w = Writer::new();
        crate::buf::Pos::write(&[-5, 70, 8], &mut w);
        w.f32(180.0);
        let body = clientbound(
            &session,
            State::Play,
            packet_ids::play::clientbound::SET_DEFAULT_SPAWN_POSITION,
            &w.data,
        )
        .unwrap()
        .unwrap();
        let p = play::SetDefaultSpawnPosition::read(&mut Reader::new(&body)).unwrap();
        assert_eq!(p.pos.dimension, "minecraft:the_nether");
        assert_eq!((p.pos.pos, p.yaw, p.pitch), ([-5, 70, 8], 180.0, 0.0));
    }

    #[test]
    fn player_fields_survive_all_metadata_steps_and_nbt_shoulders_do_not_end_the_list() {
        for version in [ServerVersion::V1_21_8, ServerVersion::V1_21_6] {
            player_metadata(version);
        }
    }

    fn player_metadata(version: ServerVersion) {
        let session = Session::new(version);
        // The local player has only a play login, never AddEntity. A vanilla 1.21.8 login prefix,
        // followed by its CommonPlayerSpawnInfo and secure-chat flag.
        let mut join = Writer::new();
        join.i32(42);
        join.bool(false);
        join.varint(1);
        join.string("minecraft:overworld");
        for n in [20, 8, 8] {
            join.varint(n);
        }
        for flag in [false, true, false] {
            join.bool(flag);
        }
        join.varint(0);
        join.string("minecraft:overworld");
        join.i64(0);
        join.i8(1);
        join.i8(-1);
        for flag in [false, false, false] {
            join.bool(flag);
        }
        join.varint(0);
        join.varint(63);
        join.bool(false);
        super::super::clientbound(
            &session,
            State::Play,
            packet_ids::play::clientbound::LOGIN,
            &join.data,
        )
        .unwrap();
        let mut w = Writer::new();
        w.varint(42);
        w.bytes(&[15, 3]);
        w.f32(4.0); // absorption
        w.bytes(&[16, 1, 5, 17, 0, 0x7f, 18, 0, 1]); // score, skin mask, right arm
        for (index, tag) in [
            (
                19,
                horizon_world::nbt::Tag::compound([
                    (
                        "id",
                        horizon_world::nbt::Tag::String("minecraft:parrot".into()),
                    ),
                    ("Variant", horizon_world::nbt::Tag::Int(3)),
                ]),
            ),
            (20, horizon_world::nbt::Tag::Compound(Default::default())),
        ] {
            w.u8(index);
            w.varint(16);
            w.nbt(&tag);
        }
        w.bytes(&[0, 0, 2, 0xff]); // A field after the old compound tags must survive.
        let (id, body) = super::super::clientbound(
            &session,
            State::Play,
            packet_ids::play::clientbound::SET_ENTITY_DATA,
            &w.data,
        )
        .unwrap()
        .unwrap();
        let play::Clientbound::SetEntityData(p) =
            play::Clientbound::decode(id, &body.unwrap()).unwrap()
        else {
            panic!("metadata");
        };
        assert_eq!(
            p.values,
            vec![
                (17, serializer::FLOAT, Value::Float(4.0)),
                (18, serializer::INT, Value::Int(5)),
                (16, serializer::BYTE, Value::Byte(0x7f)),
                (15, serializer::HUMANOID_ARM, Value::Id(1)),
                (19, serializer::OPTIONAL_UNSIGNED_INT, Value::Id(4)),
                (20, serializer::OPTIONAL_UNSIGNED_INT, Value::Id(0)),
                (0, serializer::BYTE, Value::Byte(2)),
            ]
        );
    }

    #[test]
    fn an_explosion_keeps_its_push_particle_and_custom_sound() {
        let mut w = Writer::new();
        crate::buf::Vec3d::write(&[1.0, 64.0, -2.0], &mut w);
        w.bool(true);
        crate::buf::Vec3d::write(&[0.5, -1.0, 2.0], &mut w);
        w.varint(22); // 1.21.8 explosion particle, 30 in the pinned report.
        w.varint(0); // Inline sound holder, not a registry id.
        w.string("minecraft:entity.generic.explode");
        w.bool(false);
        let play::Clientbound::Explode(p) =
            translated(packet_ids::play::clientbound::EXPLODE, &w.data)
        else {
            panic!("explosion");
        };
        assert_eq!(p.center, [1.0, 64.0, -2.0]);
        assert_eq!(p.knockback, Some([0.5, -1.0, 2.0]));
        assert_eq!(p.particle.id, 30);
        assert_eq!(
            p.sound,
            (-1, Some(("minecraft:entity.generic.explode".into(), None)))
        );
        assert!(p.play_sound && p.block_particles.is_empty());
        assert_eq!((p.radius, p.block_count), (0.0, 0));
    }

    #[test]
    fn renamed_chains_keep_their_state_and_item_in_both_directions() {
        use crate::item::{Slot, Stack};
        let ids = ServerVersion::V1_21_8.ids().unwrap();
        // Read independently from vanilla's reports: chain axis=x,waterlogged=true and its item.
        let old = Paletted {
            bits: 0,
            palette: vec![7016],
            data: vec![],
        };
        let new = super::super::renumber_states(ids, &old, 16);
        assert_eq!(new.get(0), 9980);
        assert_eq!(ids.block_state.down(9980), Some(7016));
        let sent = [3, 0xfa, 0x02, 0, 0]; // Three chain items, old id 378.
        let stack = {
            let _up = ids::renumbering(ids, Way::Up);
            Slot::read(&mut Reader::new(&sent)).unwrap().unwrap()
        };
        assert_eq!((stack.item, stack.count), (473, 3));
        let mut w = Writer::new();
        {
            let _down = ids::renumbering(ids, Way::Down);
            Slot::write(&Some(Stack::new(473, 3)), &mut w);
        }
        assert_eq!(w.data, sent);
    }

    #[test]
    fn discs_before_and_after_lava_chicken_keep_their_identity_on_1_21_6() {
        use crate::item::{Slot, Stack};
        let ids = ServerVersion::V1_21_6.ids().unwrap();
        // Vanilla reports: cat is 1250 -> 1461; pigstep is 1266 -> 1479. The newer lava chicken
        // disc occupies pinned id 1468, with no 1.21.6 counterpart (1.21.8 has it at 1256).
        for (sent, pinned) in [([3, 0xe2, 0x09, 0, 0], 1461), ([3, 0xf2, 0x09, 0, 0], 1479)] {
            let stack = {
                let _up = ids::renumbering(ids, Way::Up);
                Slot::read(&mut Reader::new(&sent)).unwrap().unwrap()
            };
            assert_eq!((stack.item, stack.count), (pinned, 3));
            let mut w = Writer::new();
            {
                let _down = ids::renumbering(ids, Way::Down);
                Slot::write(&Some(Stack::new(pinned, 3)), &mut w);
            }
            assert_eq!(w.data, sent);
        }
        assert_eq!(ids.item.down(1468), None);
        assert_eq!(
            ServerVersion::V1_21_8.ids().unwrap().item.down(1468),
            Some(1256)
        );
        assert_eq!(ids.block_state.up(7016), Some(9980)); // inherited chain axis=x, waterlogged=true
    }

    #[test]
    fn a_1_21_6_sound_after_the_inserted_disc_is_not_off_by_one() {
        let mut w = Writer::new();
        w.varint(1180); // Level-up holder: old registry id 1179 + 1, pinned id 1321.
        w.varint(0);
        for coordinate in [8, 512, -16] {
            w.i32(coordinate);
        }
        w.f32(1.0);
        w.f32(0.5);
        w.i64(42);
        let play::Clientbound::Sound(p) = translated_from(
            ServerVersion::V1_21_6,
            packet_ids::play::clientbound::SOUND,
            &w.data,
        ) else {
            panic!("sound");
        };
        assert_eq!(p.sound, (1321, None));
        assert_eq!(
            (p.x, p.y, p.z, p.volume, p.pitch, p.seed),
            (8, 512, -16, 1.0, 0.5, 42)
        );
        assert_eq!(
            ServerVersion::V1_21_8.ids().unwrap().sound_event.down(1321),
            Some(1180)
        );
    }
}
