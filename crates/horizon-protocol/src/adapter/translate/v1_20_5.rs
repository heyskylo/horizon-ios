//! 1.20.5/1.20.6 layouts before 1.21, from the vanilla stream codecs.
use super::*;

pub(super) fn clientbound(
    _session: &Session,
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
        cb::PLAYER_CHAT | cb::DISGUISED_CHAT => {
            if id == cb::PLAYER_CHAT {
                r.uuid()?;
                r.varint()?; // Per-sender message index; the later step adds the global index.
                if r.bool()? {
                    play::Signature::read(&mut r)?;
                }
                r.string(256)?;
                r.i64()?;
                r.i64()?;
                for _ in 0..r.count(65536)? {
                    play::PackedSignature::read(&mut r)?;
                }
                if r.bool()? {
                    r.nbt()?;
                }
                play::FilterMask::read(&mut r)?;
            } else {
                r.nbt()?; // The disguised message itself, before ChatType.Bound.
            }
            let prefix = r.position();
            let chat_type = r.varint()?;
            let tail = r.position();
            r.nbt()?;
            if r.bool()? {
                r.nbt()?;
            }
            w.bytes(&payload[..prefix]);
            w.varint(
                chat_type
                    .checked_add(1)
                    .filter(|_| chat_type >= 0)
                    .ok_or_else(|| crate::Error::new("invalid legacy chat type"))?,
            );
            w.bytes(&payload[tail..]);
        }
        cb::MOUNT_SCREEN_OPEN => {
            let container = r.u8()?;
            let size = r.varint()?;
            let entity = r.i32()?;
            // The saddle occupies one container slot; body equipment is separate already.
            if !(1..=16).contains(&size) || (size - 1) % 3 != 0 {
                return Err(crate::Error::new("invalid legacy horse inventory size"));
            }
            w.u8(container);
            w.varint((size - 1) / 3);
            w.i32(entity);
        }
        cb::PROJECTILE_POWER => {
            let entity = r.varint()?;
            let x = r.f64()?;
            let y = r.f64()?;
            let z = r.f64()?;
            w.varint(entity);
            // Modern AbstractHurtingProjectile applies acceleration along its motion direction.
            // Its scalar can retain the magnitude of the old three-dimensional acceleration.
            w.f64((x * x + y * y + z * z).sqrt());
        }
        cb::UPDATE_ATTRIBUTES => {
            w.varint(r.varint()?);
            let count = r.count(65536)?;
            w.varint(count as i32);
            for _ in 0..count {
                // Leave registry ids in the server's numbering for the common final remap.
                w.varint(r.varint()?);
                w.f64(r.f64()?);
                let modifiers = r.count(65536)?;
                w.varint(modifiers as i32);
                for _ in 0..modifiers {
                    let uuid = r.uuid()?;
                    w.string(&crate::item::v1_20_5::modifier_id(uuid, ""));
                    w.f64(r.f64()?);
                    w.varint(r.varint()?);
                }
            }
        }
        _ => return Ok(None),
    }
    finish(&r)?;
    Ok(Some(w.data))
}

fn finish(r: &Reader) -> Result<()> {
    if r.remaining() == 0 {
        Ok(())
    } else {
        Err(crate::Error::new("trailing legacy packet bytes"))
    }
}

pub(super) fn use_item(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let p = play::UseItem::read(&mut r)?;
    finish(&r)?;
    let mut w = Writer::new();
    w.varint(p.hand);
    w.varint(p.sequence);
    // Yaw and pitch were added in 1.21.
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
    fn vanilla_766_packets_become_the_767_layout_before_common_remapping() {
        // Actual STREAM_CODEC output from both 1.20.5/1.20.6 and 1.21.1. The attack-damage
        // attribute has registry id 2 in both; this step leaves it for the common remapper.
        use packet_ids::play::clientbound as cb;
        let session = Session::new(ServerVersion::V1_20_5);
        for (id, old, newer) in [
            (cb::MOUNT_SCREEN_OPEN, "c8010000002a", "c8000000002a"),
            (cb::MOUNT_SCREEN_OPEN, "c8100000002a", "c8050000002a"),
            (
                cb::PROJECTILE_POWER,
                "2a3f9eb851eb851eb83fa47ae147ae147b0000000000000000",
                "2a3fa999999999999a",
            ),
            (
                cb::UPDATE_ATTRIBUTES,
                "2a0102400800000000000001000000000000000000000000000000013fc000000000000002",
                "2a01024008000000000000012e6d696e6563726166743a30303030303030302d303030302d303030302d303030302d3030303030303030303030313fc000000000000002",
            ),
        ] {
            let old = bytes(old);
            assert_eq!(
                clientbound(&session, State::Play, id, &old)
                    .unwrap()
                    .unwrap(),
                bytes(newer)
            );
            assert!(clientbound(&session, State::Play, id, &old[..old.len() - 1]).is_err());
            let mut trailing = old;
            trailing.push(0);
            assert!(clientbound(&session, State::Play, id, &trailing).is_err());
        }
        assert!(
            clientbound(
                &session,
                State::Play,
                cb::MOUNT_SCREEN_OPEN,
                &bytes("c8020000002a")
            )
            .is_err()
        );
    }

    #[test]
    fn vanilla_766_player_and_disguised_chat_decode_through_the_complete_pipeline() {
        use horizon_world::nbt::Tag;
        use packet_ids::play::clientbound as cb;
        // Actual full packet STREAM_CODEC outputs from both 766 releases and 1.21.1.
        // Zero-filled signature blocks in the final fixture are abbreviated without changing bytes.
        let prefix = "000000000000000000000000000000010301";
        let middle = "0c57697265206d65737361676500000000000030390000000000000063020600";
        let suffix = "01080008556e7369676e656402010000000000000010";
        let complex = |holder: &str| {
            format!(
                "{prefix}{}{middle}{}{suffix}{holder}0800045769726501080006546172676574",
                "00".repeat(256),
                "00".repeat(256),
            )
        };
        let session = Session::new(ServerVersion::V1_20_5);
        let mut global = 0;
        for (id, old, newer, chat_type, is_complex) in [
            (
                cb::PLAYER_CHAT,
                "0000000000000000000000000000000103000c57697265206d65737361676500000000000030390000000000000063000000000800045769726500".into(),
                "0000000000000000000000000000000103000c57697265206d65737361676500000000000030390000000000000063000000010800045769726500".into(),
                0,
                false,
            ),
            (
                cb::DISGUISED_CHAT,
                "08000c57697265206d657373616765000800045769726500".into(),
                "08000c57697265206d657373616765010800045769726500".into(),
                0,
                false,
            ),
            (
                cb::PLAYER_CHAT,
                "0000000000000000000000000000000103000c57697265206d65737361676500000000000030390000000000000063000000020800045769726501080006546172676574".into(),
                "0000000000000000000000000000000103000c57697265206d65737361676500000000000030390000000000000063000000030800045769726501080006546172676574".into(),
                2,
                false,
            ),
            (
                cb::DISGUISED_CHAT,
                "08000c57697265206d657373616765020800045769726501080006546172676574".into(),
                "08000c57697265206d657373616765030800045769726501080006546172676574".into(),
                2,
                false,
            ),
            (cb::PLAYER_CHAT, complex("02"), complex("03"), 2, true),
        ] {
            let old = bytes(&old);
            let (translated_id, translated) = super::super::clientbound(
                &session,
                State::Play,
                id,
                &old,
            )
            .unwrap()
            .unwrap();
            assert_eq!(translated_id, id);
            let translated = translated.unwrap();
            let mut expected = Vec::new();
            if id == cb::PLAYER_CHAT {
                expected.push(global as u8);
            }
            expected.extend(bytes(&newer));
            assert_eq!(translated, expected);
            let bound = match play::Clientbound::decode(id, &translated).unwrap() {
                play::Clientbound::PlayerChat(p) => {
                    assert_eq!((p.global_index, p.sender, p.index), (global, 1, 3));
                    assert_eq!((p.content.as_str(), p.timestamp, p.salt), ("Wire message", 12345, 99));
                    if is_complex {
                        assert_eq!(p.signature, Some(vec![0; 256]));
                        assert_eq!(p.last_seen, [(5, None), (-1, Some(vec![0; 256]))]);
                        assert_eq!(p.unsigned_content, Some(Tag::String("Unsigned".into())));
                        assert_eq!(p.filter, (2, vec![16]));
                    }
                    global += 1;
                    p.chat_type
                }
                play::Clientbound::DisguisedChat(p) => {
                    assert_eq!(p.message, Tag::String("Wire message".into()));
                    p.chat_type
                }
                _ => panic!("chat"),
            };
            assert_eq!(bound.chat_type, chat_type);
            assert_eq!(bound.name, Tag::String("Wire".into()));
            assert_eq!(bound.target, (chat_type == 2).then(|| Tag::String("Target".into())));
            assert!(clientbound(&session, State::Play, id, &old[..old.len() - 1]).is_err());
            let mut trailing = old;
            trailing.push(0);
            assert!(clientbound(&session, State::Play, id, &trailing).is_err());
        }
    }

    #[test]
    fn vanilla_766_use_item_keeps_hand_and_sequence_without_rotation() {
        let old = bytes("01ac02");
        let newer = bytes("01ac0242b40000c2340000");
        assert_eq!(use_item(&newer).unwrap(), old);
        assert!(use_item(&old).is_err());
        let mut trailing = newer;
        trailing.push(0);
        assert!(use_item(&trailing).is_err());
    }
}
