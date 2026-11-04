//! 1.21/1.21.1 packet layouts, from the vanilla stream codecs and client handlers.
use super::*;

pub(crate) mod boats;
pub(crate) mod recipes;

pub(super) fn clientbound(
    _session: &Session,
    state: State,
    id: i32,
    payload: &[u8],
) -> Result<Option<Vec<u8>>> {
    if (state, id) == (State::Login, packet_ids::login::clientbound::LOGIN_FINISHED) {
        let mut r = Reader::new(payload);
        crate::packets::login::GameProfile::read(&mut r)?;
        let end = r.position();
        r.bool()?; // strictErrorHandling, removed in 1.21.2.
        ensure_end(&r)?;
        return Ok(Some(payload[..end].to_vec()));
    }
    if state != State::Play {
        return Ok(None);
    }
    use packet_ids::play::clientbound as cb;
    let mut r = Reader::new(payload);
    let mut w = Writer::new();
    match id {
        cb::PLAYER_POSITION => {
            let pos = crate::buf::Vec3d::read(&mut r)?;
            let yaw = r.f32()?;
            let pitch = r.f32()?;
            let flags = i32::from(r.u8()?);
            let teleport = r.varint()?;
            ensure_end(&r)?;
            // The old handler retained velocity on an axis only when that position was relative.
            play::PlayerPosition::write(
                &play::PlayerPosition {
                    teleport,
                    position: pos,
                    delta: [0.0; 3],
                    yaw,
                    pitch,
                    relatives: flags | ((flags & 7) << 5),
                },
                &mut w,
            );
        }
        cb::TELEPORT_ENTITY => {
            let entity = r.varint()?;
            let position = crate::buf::Vec3d::read(&mut r)?;
            let yaw = f32::from(r.i8()?) * (360.0 / 256.0);
            let pitch = f32::from(r.i8()?) * (360.0 / 256.0);
            let on_ground = r.bool()?;
            ensure_end(&r)?;
            // Old entity teleports never changed its velocity, unlike an absolute PositionMoveRotation.
            play::TeleportEntity::write(
                &play::TeleportEntity {
                    id: entity,
                    position,
                    delta: [0.0; 3],
                    yaw,
                    pitch,
                    relatives: 0xe0,
                    on_ground,
                },
                &mut w,
            );
        }
        cb::SET_TIME => {
            let game_time = r.i64()?;
            let old_day = r.i64()?;
            ensure_end(&r)?;
            let day = old_day
                .checked_abs()
                .ok_or_else(|| crate::Error::new("invalid legacy day time"))?;
            w.i64(game_time);
            w.i64(day);
            w.bool(old_day >= 0);
        }
        cb::PLAYER_INFO_UPDATE => {
            let mut p = play::PlayerInfoUpdate::read(&mut r)?;
            ensure_end(&r)?;
            if p.actions & 1 != 0 {
                p.actions |= 64; // UPDATE_LIST_ORDER arrived in 1.21.2; old entries all used zero.
                for entry in &mut p.entries {
                    entry.list_order = Some(0);
                }
            }
            play::PlayerInfoUpdate::write(&p, &mut w);
        }
        cb::CONTAINER_CLOSE
        | cb::CONTAINER_SET_CONTENT
        | cb::CONTAINER_SET_DATA
        | cb::MOUNT_SCREEN_OPEN => {
            w.varint(i32::from(r.u8()?));
            w.bytes(r.rest());
        }
        cb::CONTAINER_SET_SLOT => {
            let container = i32::from(r.i8()?);
            if container < 0 {
                return Err(crate::Error::new("legacy special slot was not routed"));
            }
            w.varint(container);
            w.bytes(r.rest());
        }
        cb::COOLDOWN => {
            let item = usize::try_from(r.varint()?)
                .map_err(|_| crate::Error::new("negative cooldown item"))?;
            let name = crate::adapter::v1_21_1_ids::ITEM_NAMES
                .get(item)
                .filter(|name| !name.is_empty())
                .ok_or_else(|| crate::Error::new("unknown cooldown item"))?;
            w.string(name);
            w.varint(r.varint()?);
            ensure_end(&r)?;
        }
        _ => return Ok(None),
    }
    Ok(Some(w.data))
}

fn ensure_end(r: &Reader) -> Result<()> {
    if r.remaining() == 0 {
        Ok(())
    } else {
        Err(crate::Error::new("trailing legacy packet bytes"))
    }
}

/// ClientInformation is shared by the configuration and play packets.
pub(super) fn client_information(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let p = configuration::ClientInformation::read(&mut r)?;
    ensure_end(&r)?;
    let mut w = Writer::new();
    w.string(&p.language);
    w.i8(p.view_distance);
    w.varint(p.chat_visibility);
    w.bool(p.chat_colors);
    w.u8(p.model_customisation);
    w.varint(p.main_hand);
    w.bool(p.text_filtering);
    w.bool(p.allows_listing);
    // ParticleStatus was added in 1.21.2.
    Ok(w.data)
}

/// BlockHitResult gained its world-border flag in 1.21.2.
pub(super) fn use_item_on(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let p = play::UseItemOn::read(&mut r)?;
    ensure_end(&r)?;
    let mut w = Writer::new();
    w.varint(p.hand);
    crate::buf::Pos::write(&p.pos, &mut w);
    w.varint(p.face);
    play::Cursor::write(&p.cursor, &mut w);
    w.bool(p.inside);
    w.varint(p.sequence);
    Ok(w.data)
}

/// The old negative window ids are different packet types in the pinned representation.
pub(super) fn container_slot(session: &Session, payload: &[u8]) -> Result<Option<(i32, Vec<u8>)>> {
    let mut r = Reader::new(payload);
    let container = r.i8()?;
    if container >= 0 {
        return Ok(None);
    }
    r.varint()?; // state id: cursor/direct inventory updates carry no menu state in the pinned packet.
    let slot = r.i16()?;
    let item = {
        let _up = ids::renumbering(session.version.ids().unwrap(), Way::Up);
        crate::item::Slot::read(&mut r)?
    };
    ensure_end(&r)?;
    let packet = match container {
        -1 => play::Clientbound::SetCursorItem(play::SetCursorItem { item }),
        -2 => play::Clientbound::SetPlayerInventory(play::SetPlayerInventory {
            slot: i32::from(slot),
            item,
        }),
        _ => return Err(crate::Error::new("unknown legacy inventory window")),
    };
    let bytes = packet.encode();
    let mut r = Reader::new(&bytes);
    Ok(Some((r.varint()?, r.rest().to_vec())))
}

/// The old vehicle input used analog axes and separate jump/shift bits.
pub(super) fn serverbound_input(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let flags = r.u8()?;
    ensure_end(&r)?;
    let mut w = Writer::new();
    w.f32(f32::from((flags & 4 != 0) as i8 - (flags & 8 != 0) as i8));
    w.f32(f32::from((flags & 1 != 0) as i8 - (flags & 2 != 0) as i8));
    w.u8((flags >> 4) & 3);
    Ok(w.data)
}

/// Preserve the older explosion's real radius/count before the later steps that lack those fields.
pub(super) fn explosion(session: &Session, payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let center = crate::buf::Vec3d::read(&mut r)?;
    let radius = r.f32()?;
    let count = r.count(1_048_576)?;
    let offsets = r.bytes(count * 3)?.to_vec();
    let knockback = [r.f32()?, r.f32()?, r.f32()?].map(f64::from);
    let interaction = r.varint()?;
    if !(0..=3).contains(&interaction) {
        return Err(crate::Error::new("unknown explosion block interaction"));
    }
    let (small, large, sound) = {
        let _up = ids::renumbering(session.version.ids().unwrap(), Way::Up);
        (
            crate::entity_data::Particle::read(&mut r)?,
            crate::entity_data::Particle::read(&mut r)?,
            play::SoundHolder::read(&mut r)?,
        )
    };
    ensure_end(&r)?;
    let mut w = Writer::new();
    play::Explode::write(
        &play::Explode {
            center,
            radius,
            block_count: count as i32,
            knockback: Some(knockback),
            particle: if radius >= 2.0 && interaction != 0 {
                large
            } else {
                small
            },
            sound,
            block_particles: Vec::new(),
            play_sound: true,
        },
        &mut w,
    );
    if matches!(interaction, 1 | 2) {
        // The old client applied the listed destructive block changes immediately. Trigger-block
        // explosions have different block-specific behavior and must not erase their blocks.
        let origin = center.map(|value| value.floor() as i32);
        let mut queued = session.incoming.lock().unwrap();
        for offset in offsets.chunks_exact(3) {
            let pos = std::array::from_fn(|axis| {
                origin[axis].wrapping_add(i32::from(offset[axis] as i8))
            });
            let packet = play::Clientbound::BlockUpdate(play::BlockUpdate { pos, state: 0 });
            queued.push_back((
                packet_ids::play::clientbound::BLOCK_UPDATE,
                strip_id(packet.encode()),
            ));
        }
    }
    Ok(w.data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn position_flags_preserve_only_the_velocity_components_the_old_handler_kept() {
        let session = Session::new(ServerVersion::V1_21_1);
        // Independent vanilla 1.21.1 ClientboundPlayerPositionPacket.STREAM_CODEC output.
        let old = bytes("3ff40000000000004050000000000000c00600000000000042b40000c234000019ac02");
        let body = clientbound(
            &session,
            State::Play,
            packet_ids::play::clientbound::PLAYER_POSITION,
            &old,
        )
        .unwrap()
        .unwrap();
        let p = play::PlayerPosition::read(&mut Reader::new(&body)).unwrap();
        assert_eq!(p.teleport, 300);
        assert_eq!(p.position, [1.25, 64.0, -2.75]);
        assert_eq!((p.yaw, p.pitch, p.relatives), (90.0, -45.0, 0x39));
        assert_eq!(p.delta, [0.0; 3]);
        let mut old = Writer::new();
        old.varint(7);
        for p in [1.25, 64.0, -2.75] {
            old.f64(p);
        }
        old.i8(-64);
        old.i8(32);
        old.bool(true);
        let body = clientbound(
            &session,
            State::Play,
            packet_ids::play::clientbound::TELEPORT_ENTITY,
            &old.data,
        )
        .unwrap()
        .unwrap();
        let p = play::TeleportEntity::read(&mut Reader::new(&body)).unwrap();
        assert_eq!(
            (p.id, p.yaw, p.pitch, p.relatives, p.on_ground),
            (7, -90.0, 45.0, 0xe0, true)
        );
    }

    #[test]
    fn negative_slot_windows_become_cursor_and_direct_inventory_updates() {
        let session = Session::new(ServerVersion::V1_21_1);
        // Independent vanilla 1.21.1 packet codec outputs, with state=300 and an empty stack.
        for (old, expected) in [
            (
                "ffac02ffff00",
                packet_ids::play::clientbound::SET_CURSOR_ITEM,
            ),
            (
                "feac02002400",
                packet_ids::play::clientbound::SET_PLAYER_INVENTORY,
            ),
        ] {
            let (id, body) = container_slot(&session, &bytes(old)).unwrap().unwrap();
            assert_eq!(id, expected);
            let packet = play::Clientbound::decode(id, &body).unwrap();
            match packet {
                play::Clientbound::SetCursorItem(p) => assert!(p.item.is_none()),
                play::Clientbound::SetPlayerInventory(p) => {
                    assert_eq!((p.slot, p.item), (36, None))
                }
                _ => panic!("slot"),
            }
        }
        let body = clientbound(
            &session,
            State::Play,
            packet_ids::play::clientbound::CONTAINER_CLOSE,
            &[180],
        )
        .unwrap()
        .unwrap();
        assert_eq!(body, [0xb4, 1]);
    }

    #[test]
    fn old_analog_input_and_stopped_time_keep_their_values() {
        let body = serverbound_input(&[0b0111_0101]).unwrap();
        let mut r = Reader::new(&body);
        assert_eq!(
            (r.f32().unwrap(), r.f32().unwrap(), r.u8().unwrap()),
            (1.0, 1.0, 3)
        );
        assert_eq!(serverbound_input(&[15]).unwrap(), [0; 9]); // opposing keys cancel each axis
        let session = Session::new(ServerVersion::V1_21_1);
        for (old, day, ticking) in [
            ("0000000000000064ffffffffffffe890", 6000, false),
            ("0000000000000064ffffffffffffffff", 1, false),
            ("00000000000000640000000000001770", 6000, true),
        ] {
            let body = clientbound(
                &session,
                State::Play,
                packet_ids::play::clientbound::SET_TIME,
                &bytes(old),
            )
            .unwrap()
            .unwrap();
            let mut r = Reader::new(&body);
            assert_eq!(
                (r.i64().unwrap(), r.i64().unwrap(), r.bool().unwrap()),
                (100, day, ticking)
            );
        }
    }

    #[test]
    fn vanilla_login_containers_and_cooldown_keep_their_values() {
        let session = Session::new(ServerVersion::V1_21_1);
        let old = bytes("000000000000000000000000000000010d486f72697a6f6e5061636b65740001");
        let body = clientbound(
            &session,
            State::Login,
            packet_ids::login::clientbound::LOGIN_FINISHED,
            &old,
        )
        .unwrap()
        .unwrap();
        let mut r = Reader::new(&body);
        let p = crate::packets::login::GameProfile::read(&mut r).unwrap();
        assert_eq!((p.uuid, p.name), (1, "HorizonPacket".into()));
        assert_eq!(r.remaining(), 0);
        assert!(
            clientbound(
                &session,
                State::Login,
                packet_ids::login::clientbound::LOGIN_FINISHED,
                &old[..old.len() - 1]
            )
            .is_err()
        );
        let body = clientbound(
            &session,
            State::Play,
            packet_ids::play::clientbound::CONTAINER_SET_CONTENT,
            &bytes("c8ac0202000000"),
        )
        .unwrap()
        .unwrap();
        let p = play::ContainerSetContent::read(&mut Reader::new(&body)).unwrap();
        assert_eq!(
            (p.container, p.state, p.items, p.carried),
            (200, 300, vec![None, None], None)
        );
        let body = clientbound(
            &session,
            State::Play,
            packet_ids::play::clientbound::MOUNT_SCREEN_OPEN,
            &bytes("c8050000002a"),
        )
        .unwrap()
        .unwrap();
        let p = play::MountScreenOpen::read(&mut Reader::new(&body)).unwrap();
        assert_eq!((p.container, p.columns, p.entity), (200, 5, 42));
        let body = clientbound(
            &session,
            State::Play,
            packet_ids::play::clientbound::COOLDOWN,
            &bytes("e10714"),
        )
        .unwrap()
        .unwrap();
        let p = play::Cooldown::read(&mut Reader::new(&body)).unwrap();
        assert_eq!((p.group, p.duration), ("minecraft:ender_pearl".into(), 20));
        assert_eq!(
            serverbound_input(&[2 | 4 | 16 | 32]).unwrap(),
            bytes("3f800000bf80000003")
        );
    }

    #[test]
    fn vanilla_explosions_keep_radius_count_particle_sound_and_immediate_block_changes() {
        for (old, particle, destroy) in [
            (
                "3ff40000000000004050000000000000c00600000000000040800000020100ffff01013e8000003f000000bf400000011615c104",
                29,
                true,
            ),
            (
                "3ff40000000000004050000000000000c00600000000000040800000020100ffff01013e8000003f000000bf400000001615c104",
                30,
                false,
            ),
            (
                "3ff40000000000004050000000000000c0060000000000003f800000020100ffff01013e8000003f000000bf400000011615c104",
                30,
                true,
            ),
        ] {
            let session = Session::new(ServerVersion::V1_21_1);
            let body = explosion(&session, &bytes(old)).unwrap();
            let p = play::Explode::read(&mut Reader::new(&body)).unwrap();
            assert_eq!(p.center, [1.25, 64.0, -2.75]);
            assert_eq!(
                p.radius,
                if old
                    == "3ff40000000000004050000000000000c0060000000000003f800000020100ffff01013e8000003f000000bf400000011615c104"
                {
                    1.0
                } else {
                    4.0
                }
            );
            assert_eq!(p.block_count, 2);
            assert_eq!(p.knockback, Some([0.25, 0.5, -0.75]));
            assert_eq!(
                (p.particle.id, p.sound.0, p.play_sound),
                (particle, 703, true)
            );
            if destroy {
                for pos in [[2, 64, -4], [0, 65, -2]] {
                    let (id, body) = session.next_incoming().unwrap();
                    let play::Clientbound::BlockUpdate(p) =
                        play::Clientbound::decode(id, &body).unwrap()
                    else {
                        panic!("block update")
                    };
                    assert_eq!((p.pos, p.state), (pos, 0));
                }
            }
            assert!(session.next_incoming().is_none());
        }
    }

    #[test]
    fn vanilla_client_information_omits_particle_status() {
        // Independent ClientInformation.write output from vanilla 1.21.1 and 1.21.2:
        // fr_fr, view distance -4, hidden chat, left hand, filtering, minimal particles.
        let old = bytes("0566725f6672fc020055000100");
        let modern = bytes("0566725f6672fc02005500010002");
        assert_eq!(client_information(&modern).unwrap(), old);
        assert!(client_information(&old).is_err());
        let mut trailing = modern;
        trailing.push(0);
        assert!(client_information(&trailing).is_err());
    }

    #[test]
    fn vanilla_use_item_on_omits_world_border_without_losing_sequence() {
        // Independent ServerboundUseItemOnPacket.STREAM_CODEC output from 1.21.1/1.21.2.
        let old = bytes("010000007fffffd040023e8000003f0000003f40000001ac02");
        let modern = bytes("010000007fffffd040023e8000003f0000003f4000000101ac02");
        assert_eq!(use_item_on(&modern).unwrap(), old);
        assert!(use_item_on(&modern[..modern.len() - 1]).is_err());
        let mut trailing = modern;
        trailing.push(0);
        assert!(use_item_on(&trailing).is_err());
    }
}
