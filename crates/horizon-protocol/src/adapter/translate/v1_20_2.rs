//! Packet fields before 1.20.3, after the separate JSON text conversion.
use super::*;
use crate::adapter::pinned_ids::particle_type as particle;

pub(crate) mod text;
pub(crate) use text::read_component;

// The legacy protocol has one server-pack slot and no UUID on either direction. Reusing its
// opaque id gives the current pack manager's same-id replacement, rather than stacking packs.
const PACK_ID: u128 = 0;

#[derive(Default)]
pub(crate) struct ResourcePacks {
    requested: bool,
}

pub(super) fn clientbound(
    session: &Session,
    state: State,
    id: i32,
    payload: &[u8],
) -> Result<Option<Vec<u8>>> {
    let resource = match state {
        State::Configuration => id == packet_ids::configuration::clientbound::RESOURCE_PACK_PUSH,
        State::Play => id == packet_ids::play::clientbound::RESOURCE_PACK_PUSH,
        _ => false,
    };
    let mut r = Reader::new(payload);
    let mut w = Writer::new();
    if resource {
        r.string(32767)?;
        r.string(40)?;
        r.bool()?;
        <crate::buf::Opt<crate::buf::Nbt>>::read(&mut r)?;
        finish(&r)?;
        w.uuid(PACK_ID);
        w.bytes(payload);
        session.legacy_resource_packs.lock().unwrap().requested = true;
        return Ok(Some(w.data));
    }
    if state == State::Play && id == packet_ids::play::clientbound::OPEN_SCREEN {
        let mut screen = play::OpenScreen::read(&mut r)?;
        finish(&r)?;
        screen.menu = session
            .version
            .ids()
            .and_then(|ids| ids.menu.up(screen.menu))
            .ok_or_else(|| crate::Error::new("unknown legacy menu type"))?;
        play::OpenScreen::write(&screen, &mut w);
        return Ok(Some(w.data));
    }
    if state != State::Play || id != packet_ids::play::clientbound::SET_OBJECTIVE {
        return Ok(None);
    }
    r.string(32767)?;
    let method = r.i8()?;
    match method {
        0 | 2 => {
            r.nbt()?;
            if !(0..=1).contains(&r.varint()?) {
                return Err(crate::Error::new("invalid legacy objective render type"));
            }
        }
        1 => {}
        _ => return Err(crate::Error::new("invalid legacy objective method")),
    }
    finish(&r)?;
    w.bytes(payload);
    if method != 1 {
        w.bool(false); // Number-format overrides did not exist.
    }
    Ok(Some(w.data))
}

fn finish(r: &Reader) -> Result<()> {
    if r.remaining() == 0 {
        Ok(())
    } else {
        Err(crate::Error::new("trailing legacy packet bytes"))
    }
}

/// The old combined score update/removal becomes one of two current packet types.
/// Both results are canonical and bypass the later packet conversion steps.
pub(super) fn set_score(_session: &Session, payload: &[u8]) -> Result<(i32, Vec<u8>)> {
    let mut r = Reader::new(payload);
    let owner = r.string(32767)?;
    let method = r.varint()?;
    let objective = r.string(32767)?;
    let packet = match method {
        0 => {
            if objective.is_empty() {
                return Err(crate::Error::new("legacy score has no objective"));
            }
            play::Clientbound::SetScore(play::SetScore {
                owner,
                objective,
                score: r.varint()?,
                display: None,
                number_format: None,
            })
        }
        1 => play::Clientbound::ResetScore(play::ResetScore {
            owner,
            objective: (!objective.is_empty()).then_some(objective),
        }),
        _ => return Err(crate::Error::new("invalid legacy score method")),
    };
    finish(&r)?;
    let encoded = packet.encode();
    let mut r = Reader::new(&encoded);
    Ok((r.varint()?, r.rest().to_vec()))
}

/// Supply exactly the old client-side explosion constructor's defaults, then retain geometry,
/// knockback and immediate block destruction through the existing pre-component conversion.
pub(super) fn explosion(session: &Session, payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    crate::buf::Vec3d::read(&mut r)?;
    r.f32()?;
    let count = r.count(1_048_576)?;
    r.bytes(count * 3)?;
    for _ in 0..3 {
        r.f32()?;
    }
    finish(&r)?;
    let ids = session
        .version
        .ids()
        .ok_or_else(|| crate::Error::new("legacy explosion requires an adapter"))?;
    let mut w = Writer::new();
    w.bytes(payload);
    w.varint(2); // Explosion's old convenience constructor uses DESTROY_WITH_DECAY.
    for pinned in [particle::EXPLOSION, particle::EXPLOSION_EMITTER] {
        w.varint(
            ids.particle_type
                .down(pinned)
                .ok_or_else(|| crate::Error::new("missing legacy explosion particle"))?,
        );
    }
    w.string("minecraft:entity.generic.explode");
    w.bool(false); // Direct SoundEvent, without a fixed range.
    v1_20_3::explosion(session, &w.data)
}

/// A single old pack's response has no UUID and only four status values.
pub(super) fn resource_status(session: &Session, payload: &[u8]) -> Result<Option<Vec<u8>>> {
    let mut r = Reader::new(payload);
    let id = r.uuid()?;
    let action = r.varint()?;
    finish(&r)?;
    if !(0..=7).contains(&action) {
        return Err(crate::Error::new("invalid resource-pack response"));
    }
    if id != PACK_ID || !session.legacy_resource_packs.lock().unwrap().requested {
        return Ok(None);
    }
    let action = match action {
        0..=3 => action,
        // 764 reports FAILED_DOWNLOAD for an invalid URL or a failed download/apply future.
        5 | 6 => 2,
        // Download progress and multi-pack discards have no old response.
        4 | 7 => return Ok(None),
        _ => unreachable!(),
    };
    let mut w = Writer::new();
    w.varint(action);
    Ok(Some(w.data))
}

/// The priority fields were added after the old jigsaw edit screen's last string.
pub(super) fn jigsaw(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    r.i64()?;
    for _ in 0..5 {
        r.string(32767)?;
    }
    let end = r.position();
    r.varint()?;
    r.varint()?;
    finish(&r)?;
    Ok(payload[..end].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Actual packet.write outputs from the checksummed vanilla 1.20.2 and 1.20.3 jars.
    fn fixture(label: &str, newer: bool) -> Vec<u8> {
        let rows = [
            (
                "score_change",
                "0b486f72697a6f6e5769726500047769726511",
                "0b486f72697a6f6e576972650477697265110000",
            ),
            (
                "reset_named",
                "0b486f72697a6f6e57697265010477697265",
                "0b486f72697a6f6e57697265010477697265",
            ),
            (
                "reset_all",
                "0b486f72697a6f6e576972650100",
                "0b486f72697a6f6e5769726500",
            ),
            (
                "objective0",
                "047769726500197b2274657874223a2257697265206f626a656374697665227d01",
                "04776972650008000e57697265206f626a6563746976650100",
            ),
            ("objective1", "047769726501", "047769726501"),
            (
                "objective2",
                "047769726502197b2274657874223a2257697265206f626a656374697665227d01",
                "04776972650208000e57697265206f626a6563746976650100",
            ),
            (
                "resource",
                "2368747470733a2f2f6578616d706c652e696e76616c69642f686f72697a6f6e2e7a697028303030303030303030303030303030303030303030303030303030303030303030303030303030300101147b2274657874223a2257697265207061636b227d",
                "000000000000000000000000000000002368747470733a2f2f6578616d706c652e696e76616c69642f686f72697a6f6e2e7a69702830303030303030303030303030303030303030303030303030303030303030303030303030303030010108000957697265207061636b",
            ),
            (
                "resource_status0",
                "00",
                "0000000000000000000000000000000000",
            ),
            (
                "resource_status1",
                "01",
                "0000000000000000000000000000000001",
            ),
            (
                "resource_status2",
                "02",
                "0000000000000000000000000000000002",
            ),
            (
                "resource_status3",
                "03",
                "0000000000000000000000000000000003",
            ),
            (
                "jigsaw",
                "0000007fffffd040136d696e6563726166743a776972655f6e616d65156d696e6563726166743a776972655f7461726765740f6d696e6563726166743a656d7074790f6d696e6563726166743a73746f6e6507616c69676e6564",
                "0000007fffffd040136d696e6563726166743a776972655f6e616d65156d696e6563726166743a776972655f7461726765740f6d696e6563726166743a656d7074790f6d696e6563726166743a73746f6e6507616c69676e6564fbffffff0f07",
            ),
            (
                "explosion",
                "3ff40000000000004050000000000000c00600000000000040800000010100ff3e8000003f000000bf400000",
                "3ff40000000000004050000000000000c00600000000000040800000010100ff3e8000003f000000bf400000021716206d696e6563726166743a656e746974792e67656e657269632e6578706c6f646500",
            ),
            ("resource_status6", "", "0000000000000000000000000000000006"),
            ("resource_status7", "", "0000000000000000000000000000000007"),
            ("resource_status5", "", "0000000000000000000000000000000005"),
            ("resource_status4", "", "0000000000000000000000000000000004"),
        ];
        let (_, old, new) = rows.iter().find(|(name, _, _)| *name == label).unwrap();
        let hex = if newer { new } else { old };
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }
    fn full(session: &Session, state: State, id: i32, old: &[u8]) -> (i32, Vec<u8>) {
        let (id, body) = super::super::clientbound(session, state, id, old)
            .unwrap()
            .unwrap();
        (id, body.unwrap_or_else(|| old.to_vec()))
    }

    #[test]
    fn vanilla_score_updates_and_removals_choose_the_current_packet() {
        let session = Session::new(ServerVersion::V1_20_2);
        use packet_ids::play::clientbound as cb;
        for (label, expected_id) in [
            ("score_change", cb::SET_SCORE),
            ("reset_named", cb::RESET_SCORE),
            ("reset_all", cb::RESET_SCORE),
        ] {
            let old = fixture(label, false);
            let (id, body) = set_score(&session, &old).unwrap();
            assert_eq!(id, expected_id, "{label}");
            assert_eq!(body, fixture(label, true), "{label}");
            assert_eq!(
                full(&session, State::Play, cb::SET_SCORE, &old),
                (id, body.clone())
            );
            let packet = play::Clientbound::decode(id, &body).unwrap();
            match packet {
                play::Clientbound::SetScore(p) => {
                    assert_eq!(
                        (p.owner.as_str(), p.objective.as_str(), p.score),
                        ("HorizonWire", "wire", 17)
                    );
                    assert!(p.display.is_none() && p.number_format.is_none());
                }
                play::Clientbound::ResetScore(p) => {
                    assert_eq!(p.owner, "HorizonWire");
                    assert_eq!(
                        p.objective.as_deref(),
                        (label == "reset_named").then_some("wire")
                    );
                }
                _ => panic!("score packet"),
            }
            let mut trailing = old.clone();
            trailing.push(0);
            assert!(set_score(&session, &trailing).is_err());
            assert!(set_score(&session, &old[..old.len() - 1]).is_err());
        }
        assert!(set_score(&session, &[0, 2, 0]).is_err());
    }

    #[test]
    fn vanilla_objectives_and_pack_prompts_pass_text_before_header_conversion() {
        let session = Session::new(ServerVersion::V1_20_2);
        use packet_ids::play::clientbound as cb;
        for label in ["objective0", "objective1", "objective2"] {
            let old = fixture(label, false);
            let (id, body) = full(&session, State::Play, cb::SET_OBJECTIVE, &old);
            assert_eq!(id, cb::SET_OBJECTIVE);
            assert_eq!(body, fixture(label, true), "{label}");
            let p = play::SetObjective::read(&mut Reader::new(&body)).unwrap();
            assert_eq!(p.name, "wire");
            if p.method == 1 {
                assert!(p.display.is_none());
            } else {
                let (name, render, format) = p.display.unwrap();
                assert_eq!(
                    name,
                    horizon_world::nbt::Tag::String("Wire objective".into())
                );
                assert_eq!(render, 1);
                assert!(format.is_none());
            }
        }
        let old = fixture("resource", false);
        for (state, id) in [
            (
                State::Configuration,
                packet_ids::configuration::clientbound::RESOURCE_PACK_PUSH,
            ),
            (State::Play, cb::RESOURCE_PACK_PUSH),
        ] {
            let (mapped, body) = full(&session, state, id, &old);
            assert_eq!(mapped, id);
            assert_eq!(body, fixture("resource", true));
            let p = configuration::ResourcePackPush::read(&mut Reader::new(&body)).unwrap();
            assert_eq!(
                (p.id, p.url.as_str(), p.hash.len(), p.required),
                (PACK_ID, "https://example.invalid/horizon.zip", 40, true)
            );
            assert_eq!(
                p.prompt,
                Some(horizon_world::nbt::Tag::String("Wire pack".into()))
            );
        }
    }

    #[test]
    fn pack_responses_keep_the_single_slot_across_configuration_and_play() {
        let session = Session::new(ServerVersion::V1_20_2);
        assert_eq!(
            resource_status(&session, &fixture("resource_status3", true)).unwrap(),
            None
        );
        let id = packet_ids::configuration::clientbound::RESOURCE_PACK_PUSH;
        full(
            &session,
            State::Configuration,
            id,
            &fixture("resource", false),
        );
        for action in 0..=7 {
            let incoming = fixture(&format!("resource_status{action}"), true);
            let expected = match action {
                0..=3 => Some(fixture(&format!("resource_status{action}"), false)),
                5 | 6 => Some(fixture("resource_status2", false)),
                _ => None,
            };
            assert_eq!(resource_status(&session, &incoming).unwrap(), expected);
            for (state, id, old_id) in [
                (
                    State::Configuration,
                    packet_ids::configuration::serverbound::RESOURCE_PACK,
                    5,
                ),
                (
                    State::Play,
                    packet_ids::play::serverbound::RESOURCE_PACK,
                    39,
                ),
            ] {
                let actual = super::super::serverbound(&session, state, id, &incoming).unwrap();
                assert_eq!(actual, expected.clone().map(|body| (old_id, body)));
            }
        }
        // A replacement uses the same slot in both directions; a reply cannot refer to another id.
        let id = packet_ids::play::clientbound::RESOURCE_PACK_PUSH;
        full(&session, State::Play, id, &fixture("resource", false));
        assert_eq!(
            resource_status(&session, &fixture("resource_status0", true)).unwrap(),
            Some(vec![0])
        );
        let mut other = fixture("resource_status3", true);
        other[15] = 1;
        assert_eq!(resource_status(&session, &other).unwrap(), None);
        let mut invalid = fixture("resource_status3", true);
        *invalid.last_mut().unwrap() = 8;
        assert!(resource_status(&session, &invalid).is_err());
        invalid = fixture("resource_status3", true);
        invalid.push(0);
        assert!(resource_status(&session, &invalid).is_err());
        assert!(resource_status(&session, &invalid[..15]).is_err());
        let fresh = Session::new(ServerVersion::V1_20_2);
        assert_eq!(
            resource_status(&fresh, &fixture("resource_status0", true)).unwrap(),
            None
        );
    }

    #[test]
    fn vanilla_jigsaw_priorities_are_removed_after_the_last_old_field() {
        let session = Session::new(ServerVersion::V1_20_2);
        let new = fixture("jigsaw", true);
        let old = fixture("jigsaw", false);
        assert_eq!(jigsaw(&new).unwrap(), old);
        let actual = super::super::serverbound(
            &session,
            State::Play,
            packet_ids::play::serverbound::SET_JIGSAW_BLOCK,
            &new,
        )
        .unwrap();
        assert_eq!(actual, Some((47, old)));
        assert!(jigsaw(&new[..new.len() - 1]).is_err());
        let mut trailing = new;
        trailing.push(0);
        assert!(jigsaw(&trailing).is_err());
    }

    #[test]
    fn vanilla_explosion_uses_old_defaults_and_preserves_block_destruction() {
        let session = Session::new(ServerVersion::V1_20_2);
        let next = Session::new(ServerVersion::V1_20_3);
        let old = fixture("explosion", false);
        let body = explosion(&session, &old).unwrap();
        assert_eq!(
            body,
            v1_20_3::explosion(&next, &fixture("explosion", true)).unwrap()
        );
        let p = play::Explode::read(&mut Reader::new(&body)).unwrap();
        assert_eq!(p.center, [1.25, 64.0, -2.75]);
        assert_eq!((p.radius, p.block_count), (4.0, 1));
        assert_eq!(p.knockback, Some([0.25, 0.5, -0.75]));
        assert_eq!(p.particle.id, 29);
        assert_eq!(
            p.sound.1,
            Some(("minecraft:entity.generic.explode".into(), None))
        );
        assert!(p.play_sound);
        let (id, bytes) = session.next_incoming().unwrap();
        assert_eq!(id, packet_ids::play::clientbound::BLOCK_UPDATE);
        let p = play::BlockUpdate::read(&mut Reader::new(&bytes)).unwrap();
        assert_eq!((p.pos, p.state), ([2, 64, -4], 0));
        assert!(session.next_incoming().is_none());
        let mut trailing = old.clone();
        trailing.push(0);
        assert!(explosion(&session, &trailing).is_err());
        assert!(explosion(&session, &old[..old.len() - 1]).is_err());
        assert!(session.next_incoming().is_none());
    }

    #[test]
    fn vanilla_open_screen_writers_map_all_old_menu_ids_after_text_normalization() {
        use packet_ids::play::clientbound as cb;
        let old_session = Session::new(ServerVersion::V1_20_2);
        let next_session = Session::new(ServerVersion::V1_20_3);
        let bytes = |hex: &str| {
            (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect::<Vec<_>>()
        };
        // Actual full ClientboundOpenScreenPacket writers and readers from both vanilla jars.
        // 765/current insert crafter at7; 764 merchant18 must become19, rather than Loom18.
        for (name, old, current) in [
            (
                "minecraft:generic_9x1",
                "0700177b2274657874223a22486f72697a6f6e206d656e75227d",
                "070008000c486f72697a6f6e206d656e75",
            ),
            (
                "minecraft:generic_9x2",
                "0701177b2274657874223a22486f72697a6f6e206d656e75227d",
                "070108000c486f72697a6f6e206d656e75",
            ),
            (
                "minecraft:generic_9x3",
                "0702177b2274657874223a22486f72697a6f6e206d656e75227d",
                "070208000c486f72697a6f6e206d656e75",
            ),
            (
                "minecraft:generic_9x4",
                "0703177b2274657874223a22486f72697a6f6e206d656e75227d",
                "070308000c486f72697a6f6e206d656e75",
            ),
            (
                "minecraft:generic_9x5",
                "0704177b2274657874223a22486f72697a6f6e206d656e75227d",
                "070408000c486f72697a6f6e206d656e75",
            ),
            (
                "minecraft:generic_9x6",
                "0705177b2274657874223a22486f72697a6f6e206d656e75227d",
                "070508000c486f72697a6f6e206d656e75",
            ),
            (
                "minecraft:generic_3x3",
                "0706177b2274657874223a22486f72697a6f6e206d656e75227d",
                "070608000c486f72697a6f6e206d656e75",
            ),
            (
                "minecraft:anvil",
                "0707177b2274657874223a22486f72697a6f6e206d656e75227d",
                "070808000c486f72697a6f6e206d656e75",
            ),
            (
                "minecraft:beacon",
                "0708177b2274657874223a22486f72697a6f6e206d656e75227d",
                "070908000c486f72697a6f6e206d656e75",
            ),
            (
                "minecraft:blast_furnace",
                "0709177b2274657874223a22486f72697a6f6e206d656e75227d",
                "070a08000c486f72697a6f6e206d656e75",
            ),
            (
                "minecraft:brewing_stand",
                "070a177b2274657874223a22486f72697a6f6e206d656e75227d",
                "070b08000c486f72697a6f6e206d656e75",
            ),
            (
                "minecraft:crafting",
                "070b177b2274657874223a22486f72697a6f6e206d656e75227d",
                "070c08000c486f72697a6f6e206d656e75",
            ),
            (
                "minecraft:enchantment",
                "070c177b2274657874223a22486f72697a6f6e206d656e75227d",
                "070d08000c486f72697a6f6e206d656e75",
            ),
            (
                "minecraft:furnace",
                "070d177b2274657874223a22486f72697a6f6e206d656e75227d",
                "070e08000c486f72697a6f6e206d656e75",
            ),
            (
                "minecraft:grindstone",
                "070e177b2274657874223a22486f72697a6f6e206d656e75227d",
                "070f08000c486f72697a6f6e206d656e75",
            ),
            (
                "minecraft:hopper",
                "070f177b2274657874223a22486f72697a6f6e206d656e75227d",
                "071008000c486f72697a6f6e206d656e75",
            ),
            (
                "minecraft:lectern",
                "0710177b2274657874223a22486f72697a6f6e206d656e75227d",
                "071108000c486f72697a6f6e206d656e75",
            ),
            (
                "minecraft:loom",
                "0711177b2274657874223a22486f72697a6f6e206d656e75227d",
                "071208000c486f72697a6f6e206d656e75",
            ),
            (
                "minecraft:merchant",
                "0712177b2274657874223a22486f72697a6f6e206d656e75227d",
                "071308000c486f72697a6f6e206d656e75",
            ),
            (
                "minecraft:shulker_box",
                "0713177b2274657874223a22486f72697a6f6e206d656e75227d",
                "071408000c486f72697a6f6e206d656e75",
            ),
            (
                "minecraft:smithing",
                "0714177b2274657874223a22486f72697a6f6e206d656e75227d",
                "071508000c486f72697a6f6e206d656e75",
            ),
            (
                "minecraft:smoker",
                "0715177b2274657874223a22486f72697a6f6e206d656e75227d",
                "071608000c486f72697a6f6e206d656e75",
            ),
            (
                "minecraft:cartography_table",
                "0716177b2274657874223a22486f72697a6f6e206d656e75227d",
                "071708000c486f72697a6f6e206d656e75",
            ),
            (
                "minecraft:stonecutter",
                "0717177b2274657874223a22486f72697a6f6e206d656e75227d",
                "071808000c486f72697a6f6e206d656e75",
            ),
        ] {
            let old = bytes(old);
            let current = bytes(current);
            assert_eq!(
                full(&old_session, State::Play, cb::OPEN_SCREEN, &old),
                (cb::OPEN_SCREEN, current.clone()),
                "{name}"
            );
            assert_eq!(
                full(&next_session, State::Play, cb::OPEN_SCREEN, &current),
                (cb::OPEN_SCREEN, current.clone()),
                "{name} 765 regression"
            );
            let p = play::OpenScreen::read(&mut Reader::new(&current)).unwrap();
            assert_eq!(p.container, 7);
            assert_eq!(
                p.title,
                horizon_world::nbt::Tag::String("Horizon menu".into())
            );
            if name == "minecraft:merchant" {
                assert_eq!(p.menu, 19);
            }
            let mut trailing = old.clone();
            trailing.push(0);
            assert!(
                super::super::clientbound(&old_session, State::Play, cb::OPEN_SCREEN, &trailing)
                    .is_err()
            );
            assert!(
                super::super::clientbound(
                    &old_session,
                    State::Play,
                    cb::OPEN_SCREEN,
                    &old[..old.len() - 1]
                )
                .is_err()
            );
        }
        let mut invalid = Writer::new();
        play::OpenScreen::write(
            &play::OpenScreen {
                container: 7,
                menu: 24,
                title: horizon_world::nbt::Tag::String("Horizon menu".into()),
            },
            &mut invalid,
        );
        assert!(clientbound(&old_session, State::Play, cb::OPEN_SCREEN, &invalid.data).is_err());
    }
}
