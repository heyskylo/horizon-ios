//! Packet and entity data layouts before 1.20.5, from vanilla's readers and writers.
use super::*;
use crate::entity_data::{
    LEGACY_AMBIENT_ENTITY_EFFECT_PARTICLE_ID, LEGACY_ENTITY_EFFECT_PARTICLE_ID, Particle,
    ParticleData, Value, serializer,
};
use std::collections::HashMap;

pub(super) fn clientbound(
    session: &Session,
    state: State,
    id: i32,
    payload: &[u8],
) -> Result<Option<Vec<u8>>> {
    if state == State::Login {
        let mut r = Reader::new(payload);
        match id {
            packet_ids::login::clientbound::LOGIN_FINISHED => {
                crate::packets::login::GameProfile::read(&mut r)?;
            }
            packet_ids::login::clientbound::HELLO => {
                r.string(20)?;
                r.byte_array(r.remaining())?;
                r.byte_array(r.remaining())?;
            }
            _ => return Ok(None),
        }
        finish(&r)?;
        let mut w = Writer::new();
        w.bytes(payload);
        // Encryption requests always authenticated; profile success had no strict-error switch.
        w.bool(id == packet_ids::login::clientbound::HELLO);
        return Ok(Some(w.data));
    }
    if state != State::Play {
        return Ok(None);
    }
    use packet_ids::play::clientbound as cb;
    let mut r = Reader::new(payload);
    let mut w = Writer::new();
    match id {
        cb::MOUNT_SCREEN_OPEN => {
            let container = r.u8()?;
            let size = r.varint()?;
            let entity = r.i32()?;
            if !(2..=17).contains(&size) || (size - 2) % 3 != 0 {
                return Err(crate::Error::new("invalid legacy horse inventory size"));
            }
            w.u8(container);
            w.varint(size - 1);
            w.i32(entity);
        }
        cb::LEVEL_PARTICLES => {
            let particle = r.varint()?;
            let start = r.position();
            r.bool()?;
            crate::buf::Vec3d::read(&mut r)?;
            for _ in 0..4 {
                r.f32()?;
            }
            r.i32()?;
            let options = r.position();
            w.bytes(&payload[start..options]);
            w.varint(particle);
            w.bytes(r.rest());
            r.bytes(r.remaining())?;
        }
        cb::UPDATE_ATTRIBUTES => {
            w.varint(r.varint()?);
            let count = r.count(65536)?;
            w.varint(count as i32);
            for _ in 0..count {
                let name = r.string(32767)?;
                let attribute = crate::adapter::v1_20_3_ids::ATTRIBUTE_NAMES
                    .iter()
                    .position(|old| *old == name)
                    .ok_or_else(|| crate::Error::new("unknown legacy attribute"))?;
                w.varint(attribute as i32);
                w.f64(r.f64()?);
                let count = r.count(65536)?;
                w.varint(count as i32);
                for _ in 0..count {
                    w.uuid(r.uuid()?);
                    w.f64(r.f64()?);
                    let operation = i32::from(r.i8()?);
                    if !(0..=2).contains(&operation) {
                        return Err(crate::Error::new("invalid legacy attribute operation"));
                    }
                    w.varint(operation);
                }
            }
        }
        cb::UPDATE_MOB_EFFECT => {
            w.varint(r.varint()?);
            w.varint(r.varint()?);
            w.varint(i32::from(r.u8()?));
            w.varint(r.varint()?);
            let flags = r.u8()?;
            let blending = r.bool()?;
            if blending {
                r.nbt()?;
            }
            w.u8(flags | if blending { 8 } else { 0 });
        }
        cb::SERVER_DATA => {
            play::ServerData::read(&mut r)?;
            let end = r.position();
            // The current ServerData wire has no flag; retain it for the native chat validator.
            let enforced = r.bool()?;
            finish(&r)?;
            session.set_legacy_secure_chat_enforcement(Some(enforced));
            w.bytes(&payload[..end]);
        }
        cb::STOP_SOUND => {
            let flags = r.u8()?;
            w.u8(flags);
            if flags & 1 != 0 {
                w.varint(r.varint()?);
            }
            if flags & 2 != 0 {
                w.string(sound_name(&r.string(32767)?));
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

/// Normalize the direct sound event, then use the shared explosion conversion exactly once.
pub(super) fn explosion(session: &Session, payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    crate::buf::Vec3d::read(&mut r)?;
    r.f32()?;
    let count = r.count(1_048_576)?;
    r.bytes(count * 3)?;
    for _ in 0..3 {
        r.f32()?;
    }
    r.varint()?;
    {
        let _up = ids::renumbering(session.version.ids().unwrap(), Way::Up);
        Particle::read(&mut r)?;
        Particle::read(&mut r)?;
    }
    let prefix = r.position();
    let name = r.string(32767)?;
    let range = if r.bool()? { Some(r.f32()?) } else { None };
    finish(&r)?;
    let mut w = Writer::new();
    w.bytes(&payload[..prefix]);
    w.varint(0); // The older packet always carried a direct SoundEvent.
    w.string(sound_name(&name));
    w.bool(range.is_some());
    if let Some(range) = range {
        w.f32(range);
    }
    v1_21_1::explosion(session, &w.data)
}

fn sound_name(name: &str) -> &str {
    if name == "minecraft:entity.generic.wind_burst" {
        "minecraft:entity.breeze.wind_burst"
    } else {
        name
    }
}

/// Old costs were full ItemStacks; current costs are item/count/component predicates.
/// The result is canonical and must bypass further item registry normalization.
pub(super) fn merchant_offers(session: &Session, payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let container = r.varint()?;
    let count = r.count(65536)?;
    let mut offers = Vec::with_capacity(count.min(256));
    {
        let _up = ids::renumbering(session.version.ids().unwrap(), Way::Up);
        for _ in 0..count {
            let cost_a = crate::item::Slot::read(&mut r)?
                .ok_or_else(|| crate::Error::new("empty legacy merchant cost"))?;
            let result = crate::item::Slot::read(&mut r)?
                .ok_or_else(|| crate::Error::new("empty legacy merchant result"))?;
            let cost_b = crate::item::Slot::read(&mut r)?;
            offers.push(play::NetOffer {
                cost_a: cost_a.into(),
                result,
                cost_b: cost_b.map(Into::into),
                out_of_stock: r.bool()?,
                uses: r.i32()?,
                max_uses: r.i32()?,
                xp: r.i32()?,
                special_price: r.i32()?,
                price_multiplier: r.f32()?,
                demand: r.i32()?,
            });
        }
    }
    let packet = play::MerchantOffers {
        container,
        offers,
        level: r.varint()?,
        xp: r.varint()?,
        show_progress: r.bool()?,
        can_restock: r.bool()?,
    };
    finish(&r)?;
    let mut w = Writer::new();
    play::MerchantOffers::write(&packet, &mut w);
    Ok(w.data)
}

/// Horse armor moved from its chest slot to body equipment in 1.20.5.
/// This returns canonical item ids and must bypass another older-item decode.
pub(super) fn equipment(session: &Session, payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let mut packet = {
        let _up = ids::renumbering(session.version.ids().unwrap(), Way::Up);
        play::SetEquipment::read(&mut r)?
    };
    finish(&r)?;
    if session.kinds.lock().unwrap().get(&packet.id) == Some(&67) {
        for (slot, _) in &mut packet.slots {
            if *slot == 4 {
                *slot = 6;
            }
        }
    }
    let mut w = Writer::new();
    play::SetEquipment::write(&packet, &mut w);
    Ok(w.data)
}

pub(super) fn container_button(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let container = i8::try_from(r.varint()?)
        .map_err(|_| crate::Error::new("legacy container id out of range"))?;
    let button = i8::try_from(r.varint()?)
        .map_err(|_| crate::Error::new("legacy container button out of range"))?;
    finish(&r)?;
    Ok(vec![container as u8, button as u8])
}

#[derive(Default)]
pub(crate) struct Metadata {
    living: HashMap<i32, (i32, bool)>,
    clouds: HashMap<i32, (i32, Particle)>,
}
impl Metadata {
    pub(crate) fn clear(&mut self) {
        self.living.clear();
        self.clouds.clear();
    }
    pub(crate) fn forget(&mut self, entity: i32) {
        self.living.remove(&entity);
        self.clouds.remove(&entity);
    }
}

// Positive classification from vanilla 1.20.4's EntityType generic classes and
// LivingEntity.isAssignableFrom, matched by registry name to the pinned entity ids.
const LIVING: &[i32] = &[
    2, 5, 7, 10, 11, 14, 17, 19, 21, 22, 26, 27, 30, 32, 36, 37, 39, 41, 42, 43, 44, 47, 55, 56,
    58, 60, 62, 63, 64, 65, 67, 68, 69, 71, 79, 81, 87, 88, 92, 97, 99, 100, 101, 102, 103, 104,
    105, 110, 111, 112, 113, 114, 115, 117, 118, 119, 120, 122, 124, 127, 130, 131, 132, 134, 138,
    140, 141, 142, 143, 144, 145, 146, 148, 149, 150, 152, 153, 154, 155, 157, 158, 159,
];

pub(super) fn entity_data(session: &Session, data: &mut play::SetEntityData) {
    let Some(kind) = session.kinds.lock().unwrap().get(&data.id).copied() else {
        return;
    };
    if LIVING.contains(&kind) {
        let mut cache = session.legacy_metadata.lock().unwrap();
        let (color, ambient) = cache.living.entry(data.id).or_default();
        let mut changed = false;
        data.values.retain_mut(|(index, _, value)| {
            if *index == 10
                && let Value::Int(value) = value
            {
                *color = *value;
                changed = true;
                return false;
            }
            if *index == 11
                && let Value::Bool(value) = value
            {
                *ambient = *value;
                changed = true;
            }
            true
        });
        if changed {
            let particles = if *color <= 0 {
                Vec::new()
            } else {
                vec![Particle {
                    id: 28, // minecraft:entity_effect
                    data: Some(ParticleData::Color(
                        (if *ambient { 38_i32 } else { 255_i32 } << 24) | (*color & 0xffffff),
                    )),
                }]
            };
            data.values
                .push((10, serializer::PARTICLES, Value::Particles(particles)));
        }
    }
    if kind == 3 {
        let mut cache = session.legacy_metadata.lock().unwrap();
        let first = !cache.clouds.contains_key(&data.id);
        let (color, particle) = cache.clouds.entry(data.id).or_insert_with(|| {
            (
                0,
                Particle {
                    id: 28,
                    data: Some(ParticleData::Color(-1)),
                },
            )
        });
        let mut changed = first;
        data.values.retain_mut(|(index, _, value)| {
            match (*index, value) {
                (9, Value::Int(value)) => {
                    *color = *value;
                    changed = true;
                    return false;
                }
                (10, _) => *index = 9,
                (11, Value::Particle(value)) => {
                    *particle = value.clone();
                    changed = true;
                    return false;
                }
                _ => {}
            }
            true
        });
        if changed {
            let mut particle = particle.clone();
            if matches!(
                particle.id,
                28 | LEGACY_ENTITY_EFFECT_PARTICLE_ID | LEGACY_AMBIENT_ENTITY_EFFECT_PARTICLE_ID
            ) {
                let alpha = if particle.id == LEGACY_AMBIENT_ENTITY_EFFECT_PARTICLE_ID {
                    38_i32 << 24
                } else if let Some(ParticleData::Color(value)) = particle.data {
                    value & !0xffffff
                } else {
                    -0x1000000
                };
                particle.id = 28;
                particle.data = Some(ParticleData::Color(alpha | (*color & 0xffffff)));
            }
            data.values
                .push((10, serializer::PARTICLE, Value::Particle(particle)));
        }
    }
    if matches!(kind, 79 | 138) {
        let mut carpet = None;
        data.values.retain_mut(|(index, _, value)| {
            if *index == 20
                && let Value::Int(color) = value
            {
                carpet = Some(*color);
                return false;
            }
            if *index == 21 {
                *index = 20;
            }
            true
        });
        if let Some(color) = carpet {
            let item = if color < 0 {
                None
            } else {
                let color = if color < 16 { color } else { 0 };
                session
                    .version
                    .ids()
                    .unwrap()
                    .item
                    .up(445 + color)
                    .map(|item| crate::item::Stack::new(item, 1))
            };
            let packet = play::Clientbound::SetEquipment(play::SetEquipment {
                id: data.id,
                slots: vec![(6, item)],
            });
            session.incoming.lock().unwrap().push_back((
                packet_ids::play::clientbound::SET_EQUIPMENT,
                strip_id(packet.encode()),
            ));
        }
    }
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
    fn vanilla_packet_encoders_cover_the_765_to_766_field_changes() {
        use packet_ids::{login::clientbound as login, play::clientbound as cb};
        let session = Session::new(ServerVersion::V1_20_3);
        // Actual vanilla packet writers from both 1.20.3/1.20.4, compared with 1.20.6.
        for (state, id, old, new) in [
            (
                State::Login,
                login::LOGIN_FINISHED,
                "000000000000000000000000000000010b486f72697a6f6e5769726500",
                "000000000000000000000000000000010b486f72697a6f6e576972650000",
            ),
            (
                State::Login,
                login::HELLO,
                "00020102020304",
                "0002010202030401",
            ),
            (
                State::Play,
                cb::SERVER_DATA,
                "08000b57697265207365727665720001",
                "08000b576972652073657276657200",
            ),
            (
                State::Play,
                cb::MOUNT_SCREEN_OPEN,
                "c8110000002a",
                "c8100000002a",
            ),
            (
                State::Play,
                cb::UPDATE_ATTRIBUTES,
                "2a011f6d696e6563726166743a67656e657269632e61747461636b5f64616d616765400800000000000001000000000000000000000000000000013fc000000000000002",
                "2a0102400800000000000001000000000000000000000000000000013fc000000000000002",
            ),
            (
                State::Play,
                cb::UPDATE_MOB_EFFECT,
                "2a00c8ac020300",
                "2a00c801ac0203",
            ),
            (
                State::Play,
                cb::UPDATE_MOB_EFFECT,
                "2a2001ac0206010a050015666163746f725f70726576696f75735f6672616d650000000005000c666163746f725f73746172740000000003000c7469636b735f6163746976650000000005000d666163746f725f7461726765743f80000005000e666163746f725f63757272656e74000000000100146861645f6566666563745f6c6173745f7469636b0003001070616464696e675f6475726174696f6e0000001600",
                "2a2001ac020e",
            ),
        ] {
            let old = bytes(old);
            assert_eq!(
                clientbound(&session, state, id, &old).unwrap(),
                Some(bytes(new))
            );
            assert!(clientbound(&session, state, id, &old[..old.len() - 1]).is_err());
            let mut trailing = old;
            trailing.push(0);
            assert!(clientbound(&session, state, id, &trailing).is_err());
        }
        // The old signed byte button can represent -1; its newer VarInt encoding has five bytes.
        assert_eq!(
            container_button(&bytes("64ffffffff0f")).unwrap(),
            bytes("64ff")
        );
        assert!(container_button(&bytes("640100")).is_err());
    }

    fn particle(version: ServerVersion, hex: &str) -> Particle {
        let mut wire = bytes(hex);
        wire.push(99);
        let mut r = Reader::new(&wire);
        let particle = {
            let _up = ids::renumbering(version.ids().unwrap(), Way::Up);
            Particle::read(&mut r).unwrap()
        };
        assert_eq!(r.u8().unwrap(), 99);
        assert_eq!(r.remaining(), 0);
        particle
    }

    #[test]
    fn vanilla_particle_options_and_headers_survive_both_registry_and_layout_changes() {
        for (old, new) in [
            (
                "0f3e8000003f0000003f400000400000003f800000000000003f000000",
                "0e3e8000003f0000003f4000003f800000000000003f00000040000000",
            ),
            ("2b000000007fffffd04007", "2d000000007fffffd04007"),
        ] {
            assert_eq!(
                particle(ServerVersion::V1_20_3, old),
                particle(ServerVersion::V1_20_5, new)
            );
        }
        // The old effect providers took RGB from their velocity arguments. Preserve that
        // behavior through Horizon's explicit legacy-provider markers rather than fixed white.
        for (old, marker, new, color) in [
            ("15", LEGACY_ENTITY_EFFECT_PARTICLE_ID, "14ffffffff", -1),
            (
                "00",
                LEGACY_AMBIENT_ENTITY_EFFECT_PARTICLE_ID,
                "1426ffffff",
                0x26ffffff,
            ),
        ] {
            assert_eq!(
                particle(ServerVersion::V1_20_3, old),
                Particle {
                    id: marker,
                    data: None
                }
            );
            assert_eq!(
                particle(ServerVersion::V1_20_5, new),
                Particle {
                    id: 28,
                    data: Some(ParticleData::Color(color))
                }
            );
        }
        let old = bytes(
            "0f013ff40000000000004050000000000000c0060000000000003e8000003f0000003f4000003fc000000000000c3e8000003f0000003f400000400000003f800000000000003f000000",
        );
        let new = bytes(
            "013ff40000000000004050000000000000c0060000000000003e8000003f0000003f4000003fc000000000000c0e3e8000003f0000003f4000003f800000000000003f00000040000000",
        );
        let decode = |version, payload: &[u8]| {
            let session = Session::new(version);
            let (id, body) = super::super::clientbound(
                &session,
                State::Play,
                packet_ids::play::clientbound::LEVEL_PARTICLES,
                payload,
            )
            .unwrap()
            .unwrap();
            play::Clientbound::decode(id, &body.unwrap()).unwrap()
        };
        assert_eq!(
            decode(ServerVersion::V1_20_3, &old),
            decode(ServerVersion::V1_20_5, &new)
        );
    }

    fn metadata(version: ServerVersion, hex: &str) -> play::SetEntityData {
        let wire = bytes(hex);
        let mut r = Reader::new(&wire);
        let _up = ids::renumbering(version.ids().unwrap(), Way::Up);
        let data = play::SetEntityData::read(&mut r).unwrap();
        assert_eq!(r.remaining(), 0);
        data
    }

    #[test]
    fn vanilla_metadata_preserves_partial_effect_updates_cloud_color_and_llama_equipment() {
        // Actual SetEntityData packet writers. Each pair describes the same visible state.
        let session = Session::new(ServerVersion::V1_20_3);
        for (kind, pairs) in [
            (
                159,
                vec![
                    ("2a0a0199cdcd010b0801ff", "2a0a120114263366990b0801ff"),
                    ("2a0b0800ff", "2a0a120114ff3366990b0800ff"),
                ],
            ),
            (
                3,
                vec![
                    ("2a0901b3c4440a08000b1115ff", "2a0908000a1114ff112233ff"),
                    ("2a0901e6aa9102ff", "2a0a1114ff445566ff"),
                ],
            ),
            (
                79,
                vec![
                    ("2a14010e150102ff", "2a140102ff"),
                    ("2a1401ffffffff0fff", "2aff"),
                ],
            ),
        ] {
            session.legacy_metadata.lock().unwrap().forget(42);
            session.kinds.lock().unwrap().insert(42, kind);
            for (old, new) in pairs {
                let mut old = metadata(ServerVersion::V1_20_3, old);
                entity_data(&session, &mut old);
                let mut expected = metadata(ServerVersion::V1_20_5, new);
                old.values.sort_by_key(|v| v.0);
                expected.values.sort_by_key(|v| v.0);
                assert_eq!(old, expected);
            }
        }
        for item in [Some(594), None] {
            // pinned minecraft:red_carpet, then unequipped
            let (id, body) = session.next_incoming().unwrap();
            let play::Clientbound::SetEquipment(p) = play::Clientbound::decode(id, &body).unwrap()
            else {
                panic!("equipment")
            };
            assert_eq!(
                p.slots
                    .iter()
                    .map(|(slot, stack)| (*slot, stack.as_ref().map(|s| s.item)))
                    .collect::<Vec<_>>(),
                [(6, item)]
            );
        }
        // ItemFrame and BlockDisplay are not LivingEntity: their integer entry must stay an integer.
        for kind in [74, 15] {
            session.kinds.lock().unwrap().insert(42, kind);
            let mut data = play::SetEntityData {
                id: 42,
                values: vec![(10, serializer::INT, Value::Int(7))],
            };
            let expected = data.clone();
            entity_data(&session, &mut data);
            assert_eq!(data, expected);
        }
        session.legacy_metadata.lock().unwrap().forget(42);
        session.kinds.lock().unwrap().insert(42, 159);
        let mut data = metadata(ServerVersion::V1_20_3, "2a0b0800ff");
        entity_data(&session, &mut data);
        assert!(
            data.values
                .iter()
                .any(|(_, _, v)| matches!(v, Value::Particles(p) if p.is_empty()))
        );
    }

    #[test]
    fn vanilla_trade_and_horse_armor_stacks_are_canonicalized_once() {
        let session = Session::new(ServerVersion::V1_20_3);
        // These helpers normally run inside Session::clientbound's connection-local NBT context.
        let _registries = ids::legacy_registries(session.legacy_registries.lock().unwrap().clone());
        let _items = ids::legacy_items(session.legacy_items.clone());
        let wire = bytes(
            "070101a206050001a10601000000000000000000000800000002000000003d4ccccd0000000003630101",
        );
        let body = merchant_offers(&session, &wire).unwrap();
        let p = play::MerchantOffers::read(&mut Reader::new(&body)).unwrap();
        assert_eq!(
            (p.container, p.level, p.xp, p.show_progress, p.can_restock),
            (7, 3, 99, true, true)
        );
        let offer = &p.offers[0];
        assert_eq!(
            (
                offer.cost_a.item,
                offer.cost_a.count,
                offer.result.item,
                offer.result.count
            ),
            (1013, 5, 1012, 1)
        );
        assert!(offer.cost_b.is_none());
        assert_eq!(
            (offer.uses, offer.max_uses, offer.xp, offer.price_multiplier),
            (0, 8, 2, 0.05)
        );
        for (kind, slot) in [(67, 6), (159, 4)] {
            session.kinds.lock().unwrap().insert(42, kind);
            let body = equipment(&session, &bytes("2a0401df080100")).unwrap();
            let p = play::SetEquipment::read(&mut Reader::new(&body)).unwrap();
            assert_eq!(p.slots[0].0, slot);
            assert_eq!(p.slots[0].1.as_ref().unwrap().item, 1409);
        }
    }

    #[test]
    fn vanilla_explosion_keeps_geometry_and_reads_its_direct_sound() {
        let session = Session::new(ServerVersion::V1_20_3);
        let wire = bytes(
            "3ff40000000000004050000000000000c00600000000000040800000010100ff3e8000003f000000bf400000011716206d696e6563726166743a656e746974792e67656e657269632e6578706c6f646500",
        );
        let body = explosion(&session, &wire).unwrap();
        let p = play::Explode::read(&mut Reader::new(&body)).unwrap();
        assert_eq!(
            (p.center, p.radius, p.block_count),
            ([1.25, 64.0, -2.75], 4.0, 1)
        );
        assert_eq!(p.knockback, Some([0.25, 0.5, -0.75]));
        assert_eq!(p.particle.id, 29);
        assert_eq!(
            p.sound,
            (-1, Some(("minecraft:entity.generic.explode".into(), None)))
        );
        let (id, body) = session.next_incoming().unwrap();
        let play::Clientbound::BlockUpdate(p) = play::Clientbound::decode(id, &body).unwrap()
        else {
            panic!("block update")
        };
        assert_eq!((p.pos, p.state), ([2, 64, -4], 0));
    }
}
