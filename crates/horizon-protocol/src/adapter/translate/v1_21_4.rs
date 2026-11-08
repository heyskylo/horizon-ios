//! 1.21.4 to 1.21.5, from vanilla packet codecs and palette serialization.
use super::*;

pub(super) fn registry_data(session: &Session, payload: &[u8]) -> Result<()> {
    if Reader::new(payload).string(32767)? != "minecraft:trim_pattern" {
        return Ok(());
    }
    let registry = configuration::RegistryData::read(&mut Reader::new(payload))?;
    // The pinned vanilla template item ids. The pattern ids come from this connection's synced
    // registry, so a data pack may reorder them or use a known template for another pattern.
    const TEMPLATES: &[(&str, i32)] = &[
        ("sentry", 1580),
        ("dune", 1581),
        ("coast", 1582),
        ("wild", 1583),
        ("ward", 1584),
        ("eye", 1585),
        ("vex", 1586),
        ("tide", 1587),
        ("snout", 1588),
        ("rib", 1589),
        ("spire", 1590),
        ("wayfinder", 1591),
        ("shaper", 1592),
        ("silence", 1593),
        ("raiser", 1594),
        ("host", 1595),
        ("flow", 1596),
        ("bolt", 1597),
    ];
    let mut patterns = session.trim_patterns.lock().unwrap();
    patterns.clear();
    for (id, entry) in registry.entries.into_iter().enumerate() {
        let template = entry
            .data
            .as_ref()
            .and_then(|data| data.get("template_item"))
            .and_then(|tag| {
                if let horizon_world::nbt::Tag::String(name) = tag {
                    Some(name.to_string_lossy())
                } else {
                    None
                }
            });
        for &(name, item) in TEMPLATES {
            let expected = format!("minecraft:{name}_armor_trim_smithing_template");
            if template.as_deref() == Some(expected.as_str())
                || (template.is_none() && entry.id == format!("minecraft:{name}"))
            {
                patterns.insert(item, id as i32);
            }
        }
    }
    Ok(())
}

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
    Ok(match id {
        cb::LEVEL_CHUNK_WITH_LIGHT => Some(chunk(payload)?),
        cb::UPDATE_ADVANCEMENTS => {
            let mut body = payload.to_vec();
            body.push(1);
            Some(body)
        }
        cb::PLAYER_CHAT => {
            let mut w = Writer::new();
            w.varint(
                session
                    .chat_index
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            );
            w.bytes(payload);
            Some(w.data)
        }
        cb::SET_PLAYER_TEAM => Some(team(payload)?),
        _ => None,
    })
}

fn chunk(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let mut w = Writer::new();
    w.i32(r.i32()?);
    w.i32(r.i32()?);
    let tag = r.nbt()?;
    let heightmaps: Vec<_> = horizon_world::heightmap::HeightmapType::ALL
        .into_iter()
        .enumerate()
        .filter_map(|(id, kind)| match tag.get(kind.name()) {
            Some(horizon_world::nbt::Tag::LongArray(data)) => Some((id as i32, data.clone())),
            _ => None,
        })
        .collect();
    crate::chunk::Heightmaps::write(&heightmaps, &mut w);
    let buffer = r.byte_array(r.remaining())?;
    let mut sections = Reader::new(&buffer);
    let mut out = Writer::new();
    while sections.remaining() > 0 {
        out.i16(sections.i16()?);
        for strategy in [Strategy::Blocks, Strategy::Biomes] {
            Paletted::read_prefixed(&mut sections, strategy, 0)?.write(&mut out);
        }
    }
    w.byte_array(&out.data);
    w.bytes(r.rest());
    Ok(w.data)
}

fn team(payload: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(payload);
    let mut w = Writer::new();
    w.string(&r.string(32767)?);
    let method = r.i8()?;
    w.i8(method);
    if method == 0 || method == 2 {
        w.nbt(&r.nbt()?);
        w.i8(r.i8()?);
        for names in [
            ["always", "never", "hideForOtherTeams", "hideForOwnTeam"],
            ["always", "never", "pushOtherTeams", "pushOwnTeam"],
        ] {
            let name = r.string(32767)?;
            w.varint(names.iter().position(|value| *value == name).unwrap_or(0) as i32);
        }
    }
    w.bytes(r.rest());
    Ok(w.data)
}

pub(super) fn experience_orb(session: &Session, payload: &[u8]) -> Result<(i32, Vec<u8>)> {
    let mut r = Reader::new(payload);
    let entity = r.varint()?;
    let position = crate::buf::Vec3d::read(&mut r)?;
    let value = i32::from(r.i16()?);
    let p = play::AddEntity {
        id: entity,
        uuid: u128::from(entity as u32),
        kind: 50,
        position,
        velocity: [0.0; 3],
        pitch: 0,
        yaw: 0,
        head_yaw: 0,
        data: 0,
    };
    session.kinds.lock().unwrap().insert(entity, 50);
    let metadata = play::Clientbound::SetEntityData(play::SetEntityData {
        id: entity,
        values: vec![(
            8,
            crate::entity_data::serializer::INT,
            crate::entity_data::Value::Int(value),
        )],
    });
    session.incoming.lock().unwrap().push_back((
        packet_ids::play::clientbound::SET_ENTITY_DATA,
        strip_id(metadata.encode()),
    ));
    Ok((
        packet_ids::play::clientbound::ADD_ENTITY,
        strip_id(play::Clientbound::AddEntity(p).encode()),
    ))
}

/// 1.21.4 sends complete predicted stacks; pinned clicks contain only their hashes. Sending no
/// predictions asks the authoritative old menu to resynchronize its slots/cursor after the click.
pub(super) fn container_click(payload: &[u8]) -> Result<Vec<u8>> {
    let play::Serverbound::ContainerClick(p) =
        play::Serverbound::decode(packet_ids::play::serverbound::CONTAINER_CLICK, payload)?
    else {
        unreachable!()
    };
    let mut w = Writer::new();
    w.varint(p.container);
    w.varint(p.state);
    w.i16(p.slot);
    w.i8(p.button);
    w.varint(p.input);
    w.varint(0);
    w.varint(0);
    Ok(w.data)
}

pub(super) fn entity_data(session: &Session, data: &mut play::SetEntityData) {
    use crate::entity_data::{Value, serializer};
    let Some(kind) = session.kinds.lock().unwrap().get(&data.id).copied() else {
        return;
    };
    let mut saddle = None;
    let mut display_changed = false;
    data.values.retain_mut(|(index, _ser, value)| {
        match (kind, *index) {
            (101, 17) | (132, 19) => {
                if let Value::Bool(saddled) = value {
                    saddle = Some(*saddled);
                }
                return false;
            }
            (101, 18) => *index = 17,
            (36, 17) => return false,
            (36, 18..=19) => *index -= 1,
            (141, 17 | 20..=22) => return false,
            (141, 18..=19) => *index -= 1,
            (19 | 37 | 67 | 79 | 88 | 119 | 138 | 155, 17) => {
                if let Value::Byte(flags) = value {
                    saddle = Some(*flags & 4 != 0);
                    *flags &= !4;
                }
            }
            (25 | 29 | 57 | 66 | 86 | 125 | 137, 11 | 13) => {
                let mut cache = session.minecart_display.lock().unwrap();
                let entry = cache.entry(data.id).or_insert((0, false));
                match value {
                    Value::Id(block) if *index == 11 => entry.0 = *block,
                    Value::Int(block) if *index == 11 => {
                        entry.0 = session
                            .version
                            .ids()
                            .and_then(|ids| ids.block_state.up(*block))
                            .unwrap_or(0)
                    }
                    Value::Bool(custom) if *index == 13 => entry.1 = *custom,
                    _ => {}
                }
                display_changed = true;
                return false;
            }
            _ => {}
        }
        true
    });
    if display_changed {
        let (block, custom) = session.minecart_display.lock().unwrap()[&data.id];
        data.values.push((
            11,
            serializer::OPTIONAL_BLOCK_STATE,
            Value::Id(if custom { block } else { 0 }),
        ));
    }
    if let Some(saddled) = saddle {
        let equipment = play::Clientbound::SetEquipment(play::SetEquipment {
            id: data.id,
            slots: vec![(7, saddled.then(|| crate::item::Stack::new(949, 1)))],
        });
        session.incoming.lock().unwrap().push_back((
            packet_ids::play::clientbound::SET_EQUIPMENT,
            strip_id(equipment.encode()),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity_data::{Value, serializer};
    use horizon_world::nbt::Tag;

    #[test]
    fn chat_keeps_the_message_and_ack_bits_without_the_new_checksum() {
        let session = Session::new(ServerVersion::V1_21_4);
        let p = play::Serverbound::Chat(play::Chat {
            message: "Hello".into(),
            timestamp: 1,
            salt: 2,
            signature: None,
            last_seen: play::LastSeenUpdate {
                offset: 3,
                acknowledged: [1, 2, 3],
                checksum: 55,
            },
        });
        let (_, body) = super::super::serverbound(
            &session,
            State::Play,
            packet_ids::play::serverbound::CHAT,
            &strip_id(p.encode()),
        )
        .unwrap()
        .unwrap();
        let mut r = Reader::new(&body);
        assert_eq!(r.string(256).unwrap(), "Hello");
        assert_eq!(
            (r.i64().unwrap(), r.i64().unwrap(), r.bool().unwrap()),
            (1, 2, false)
        );
        assert_eq!(r.varint().unwrap(), 3);
        assert_eq!(r.bytes(3).unwrap(), [1, 2, 3]);
        assert_eq!(r.remaining(), 0);
        let mut old = Writer::new();
        old.uuid(42);
        old.varint(5);
        old.bool(false);
        old.string("Hello");
        old.i64(1);
        old.i64(2);
        old.varint(0);
        old.bool(false);
        old.varint(0);
        play::ChatTypeBound::write(
            &play::ChatTypeBound {
                chat_type: 0,
                name: Tag::String("Player".into()),
                target: None,
            },
            &mut old,
        );
        for index in 0..2 {
            let (id, body) = super::super::clientbound(
                &session,
                State::Play,
                packet_ids::play::clientbound::PLAYER_CHAT,
                &old.data,
            )
            .unwrap()
            .unwrap();
            let play::Clientbound::PlayerChat(chat) =
                play::Clientbound::decode(id, &body.unwrap()).unwrap()
            else {
                panic!("chat")
            };
            assert_eq!(
                (chat.global_index, chat.sender, chat.index, chat.content),
                (index, 42, 5, "Hello".into())
            );
        }
    }

    #[test]
    fn smithing_previews_follow_each_connections_synced_pattern_order() {
        use crate::packets::recipes::{RecipeDisplay, SlotDisplay, TrimPattern};
        for reversed in [false, true] {
            let session = Session::new(ServerVersion::V1_21_4);
            let host = configuration::RegistryEntry {
                id: "minecraft:custom_host".into(),
                data: Some(Tag::compound([(
                    "template_item",
                    Tag::String("minecraft:host_armor_trim_smithing_template".into()),
                )])),
            };
            let other = configuration::RegistryEntry {
                id: "minecraft:other".into(),
                data: None,
            };
            let entries = if reversed {
                vec![host, other]
            } else {
                vec![other, host]
            };
            let mut w = Writer::new();
            configuration::RegistryData::write(
                &configuration::RegistryData {
                    registry: "minecraft:trim_pattern".into(),
                    entries,
                },
                &mut w,
            );
            super::super::clientbound(
                &session,
                State::Configuration,
                packet_ids::configuration::clientbound::REGISTRY_DATA,
                &w.data,
            )
            .unwrap();
            // Old Smithing display, whose result's pattern is a Composite(Item(host template)).
            let body = [
                2, 4, 7, 1, 2, 0xba, 0x0a, 2, 1, 2, 1, 5, 2, 1, 2, 1, 7, 1, 2, 0xba, 0x0a, 2, 1,
            ];
            let (id, body) = super::super::clientbound(
                &session,
                State::Play,
                packet_ids::play::clientbound::PLACE_GHOST_RECIPE,
                &body,
            )
            .unwrap()
            .unwrap();
            let play::Clientbound::PlaceGhostRecipe(p) =
                play::Clientbound::decode(id, &body.unwrap()).unwrap()
            else {
                panic!("ghost")
            };
            let RecipeDisplay::Smithing {
                result: SlotDisplay::SmithingTrim { pattern, .. },
                ..
            } = p.display
            else {
                panic!("trim")
            };
            assert_eq!(pattern, TrimPattern::Id(i32::from(!reversed)));
            assert_eq!(ids::trim_pattern(1595), None); // The next connection must not inherit this context.
        }
    }

    #[test]
    fn nbt_heightmaps_and_prefixed_palettes_reach_the_pinned_chunk() {
        let session = Session::new(ServerVersion::V1_21_4);
        let mut sections = Writer::new();
        sections.bytes(&[0x10, 0, 0, 1, 0, 0, 7, 0]); // stone/biome 7, two zero-long prefixes
        sections.i16(2048);
        sections.bytes(&[4, 2, 0]);
        sections.varint(7006); // old chain state
        sections.varint(256);
        for _ in 0..256 {
            sections.i64(0x1010_1010_1010_1010);
        }
        sections.bytes(&[6, 7]);
        for _ in 0..7 {
            sections.i64(0);
        }
        let mut w = Writer::new();
        w.i32(2);
        w.i32(-3);
        w.nbt(&Tag::compound([
            ("WORLD_SURFACE", Tag::LongArray(vec![42])),
            ("MOTION_BLOCKING", Tag::LongArray(vec![99])),
        ]));
        w.byte_array(&sections.data);
        for _ in 0..7 {
            w.varint(0);
        }
        let (id, body) = super::super::clientbound(
            &session,
            State::Play,
            packet_ids::play::clientbound::LEVEL_CHUNK_WITH_LIGHT,
            &w.data,
        )
        .unwrap()
        .unwrap();
        let play::Clientbound::LevelChunkWithLight(p) =
            play::Clientbound::decode(id, &body.unwrap()).unwrap()
        else {
            panic!("chunk")
        };
        assert_eq!((p.x, p.z), (2, -3));
        assert_eq!(p.chunk.heightmaps, vec![(1, vec![42]), (4, vec![99])]);
        let sections = p.chunk.sections(2, 16, 6).unwrap();
        assert_eq!(
            (sections[0].states.get(0), sections[0].biomes.get(0)),
            (1, 7)
        );
        assert_eq!(sections[1].states.palette, [0, 9980]);
        assert_eq!(
            (
                sections[1].states.get(1),
                sections[1].biomes.get(63),
                sections[1].fluids
            ),
            (9980, 0, 0)
        );
    }

    #[test]
    fn team_rules_are_read_as_strings_before_the_later_team_step() {
        let session = Session::new(ServerVersion::V1_21_4);
        let mut w = Writer::new();
        w.string("red");
        w.i8(0);
        w.nbt(&Tag::String("Red".into()));
        w.i8(3);
        w.string("hideForOwnTeam");
        w.string("pushOtherTeams");
        w.varint(12);
        w.nbt(&Tag::String("[".into()));
        w.nbt(&Tag::String("]".into()));
        w.varint(1);
        w.string("Player");
        let (id, body) = super::super::clientbound(
            &session,
            State::Play,
            packet_ids::play::clientbound::SET_PLAYER_TEAM,
            &w.data,
        )
        .unwrap()
        .unwrap();
        let play::Clientbound::SetPlayerTeam(p) =
            play::Clientbound::decode(id, &body.unwrap()).unwrap()
        else {
            panic!("team")
        };
        let params = p.parameters.unwrap();
        assert_eq!(
            (
                params.name_tag_visibility,
                params.collision_rule,
                params.color
            ),
            (3, 2, Some(12))
        );
        assert_eq!(params.prefix, Tag::String("[".into()));
        assert_eq!(p.players, ["Player"]);
    }

    #[test]
    fn old_orbs_spawn_with_their_value_as_metadata() {
        let session = Session::new(ServerVersion::V1_21_4);
        let mut w = Writer::new();
        w.varint(41);
        for p in [1.0, 2.0, -3.0] {
            w.f64(p);
        }
        w.i16(7);
        let (id, body) = experience_orb(&session, &w.data).unwrap();
        let play::Clientbound::AddEntity(p) = play::Clientbound::decode(id, &body).unwrap() else {
            panic!("orb")
        };
        assert_eq!((p.id, p.kind, p.position), (41, 50, [1.0, 2.0, -3.0]));
        let (id, body) = session.next_incoming().unwrap();
        let play::Clientbound::SetEntityData(p) = play::Clientbound::decode(id, &body).unwrap()
        else {
            panic!("metadata")
        };
        assert_eq!(p.values, [(8, serializer::INT, Value::Int(7))]);
    }

    #[test]
    fn a_saddle_moves_to_equipment_and_boost_stays_in_the_pig_field() {
        let session = Session::new(ServerVersion::V1_21_4);
        session.kinds.lock().unwrap().insert(42, 101);
        let (id, body) = super::super::clientbound(
            &session,
            State::Play,
            packet_ids::play::clientbound::SET_ENTITY_DATA,
            &[42, 17, 8, 1, 18, 1, 20, 255],
        )
        .unwrap()
        .unwrap();
        let play::Clientbound::SetEntityData(p) =
            play::Clientbound::decode(id, &body.unwrap()).unwrap()
        else {
            panic!("pig")
        };
        assert_eq!(p.values, [(18, serializer::INT, Value::Int(20))]);
        let (id, body) = session.next_incoming().unwrap();
        let play::Clientbound::SetEquipment(p) = play::Clientbound::decode(id, &body).unwrap()
        else {
            panic!("saddle")
        };
        assert_eq!(
            (p.id, p.slots[0].0, p.slots[0].1.as_ref().unwrap().item),
            (42, 7, 949)
        );
    }

    #[test]
    fn removed_fields_and_separate_minecart_updates_keep_their_meaning() {
        let session = Session::new(ServerVersion::V1_21_4);
        // Field ids independently obtained from vanilla SynchedEntityData accessors.
        for (kind, old, expected) in [
            (36, vec![17, 18, 19], vec![17, 18]),
            (141, vec![17, 18, 19, 20, 21, 22], vec![17, 18]),
            (87, vec![17], vec![17]),
        ] {
            session.kinds.lock().unwrap().insert(42, kind);
            let mut p = play::SetEntityData {
                id: 42,
                values: old
                    .into_iter()
                    .map(|index| (index, serializer::INT, Value::Int(1)))
                    .collect(),
            };
            entity_data(&session, &mut p);
            assert_eq!(p.values.iter().map(|v| v.0).collect::<Vec<_>>(), expected);
        }
        session.kinds.lock().unwrap().insert(42, 86);
        for (values, expected) in [
            (vec![(11, serializer::INT, Value::Int(7006))], 0),
            (vec![(13, serializer::BOOLEAN, Value::Bool(true))], 9980),
            (vec![(13, serializer::BOOLEAN, Value::Bool(false))], 0),
        ] {
            let mut p = play::SetEntityData { id: 42, values };
            entity_data(&session, &mut p);
            assert_eq!(
                p.values,
                [(11, serializer::OPTIONAL_BLOCK_STATE, Value::Id(expected))]
            );
        }
    }

    #[test]
    fn clicks_preserve_the_operation_while_omitting_hashed_predictions() {
        let p = play::Serverbound::ContainerClick(play::ContainerClick {
            container: 2,
            state: 10,
            slot: 3,
            button: 0,
            input: 1,
            changed: vec![(3, None)],
            carried: Some(crate::item::Hashed {
                item: 1,
                count: 5,
                added: vec![],
                removed: vec![],
            }),
        });
        let encoded = strip_id(p.encode());
        let old = container_click(&encoded).unwrap();
        assert_eq!(old, [2, 10, 0, 3, 0, 1, 0, 0]);
    }
}
