use super::*;
use crate::{
    chunk::BlockEntityInfo,
    packets::{recipes::RecipeBookSeenRecipe, round_trip},
    text::literal,
};

#[test]
fn a_command_tree_with_a_cycle_is_refused() {
    let node = |kind: u8, name: &str, children: Vec<i32>, redirect: Option<i32>| CommandNode {
        kind,
        executable: false,
        restricted: false,
        children,
        redirect,
        name: name.into(),
        parser: None,
        suggestions: None,
    };
    let read = |nodes: Vec<CommandNode>| {
        let mut w = Writer::new();
        Commands::write(&Commands { nodes, root: 0 }, &mut w);
        Commands::read(&mut Reader::new(&w.data))
    };
    // `execute` with `run` redirecting back to the root, as vanilla's tree has it.
    let execute = vec![
        node(0, "", vec![1], None),
        node(1, "execute", vec![2], None),
        node(1, "run", vec![], Some(0)),
    ];
    assert!(read(execute).is_ok());
    // root -> a -> b -> a
    let children = vec![
        node(0, "", vec![1], None),
        node(1, "a", vec![2], None),
        node(1, "b", vec![1], None),
    ];
    assert!(read(children).is_err());
    assert!(read(vec![node(0, "", vec![0], None)]).is_err());
    // a redirects to b, b to a
    let redirects = vec![
        node(0, "", vec![1, 2], None),
        node(1, "a", vec![], Some(2)),
        node(1, "b", vec![], Some(1)),
    ];
    assert!(read(redirects).is_err());
    // A long chain is fine and checked without recursing.
    let mut chain: Vec<_> = (1..=100_000).map(|i| node(1, "x", vec![i], None)).collect();
    chain[0].kind = 0;
    chain.push(node(1, "x", vec![], None));
    assert!(read(chain.clone()).is_ok());
    chain.last_mut().unwrap().children.push(50_000);
    assert!(read(chain).is_err());
}

#[test]
fn vec_delta_quantizes_the_base_like_math_round() {
    // Math.round(-2.5) is -2, so a delta of 1 lands on -1/4096, not -2/4096.
    let out = VecDelta::apply([-2.5 / 4096.0, 0.0, 0.0], [1, 0, 0]);
    assert_eq!(out[0], -1.0 / 4096.0);
}

#[test]
fn merchant_cost_predicate_is_a_typed_list_without_patch_counts() {
    // ItemCost.STREAM_CODEC: item=7, count=1, one typed component,
    // potion_contents=53, potion present (water=0), no color/effects/name.
    // Result: count=1, item=8, empty component patch. Secondary cost:
    // item=9, count=2, empty predicate, followed by the offer fields.
    let mut bytes = vec![1, 7, 1, 1, 53, 1, 0, 0, 0, 0, 1, 8, 0, 0, 1, 9, 2, 0, 0];
    bytes.extend_from_slice(&[0; 4]); // uses
    bytes.extend_from_slice(&2i32.to_be_bytes()); // max uses
    bytes.extend_from_slice(&[0; 16]); // xp, special price, multiplier, demand
    let mut r = Reader::new(&bytes);
    let offers = Offers::read(&mut r).unwrap();
    assert_eq!(r.remaining(), 0);
    assert_eq!(
        offers[0].cost_a.components,
        vec![(
            53,
            Component::Potion {
                potion: Some(0),
                color: None,
                rest: Vec::new()
            }
        )]
    );
    assert!(offers[0].cost_b.as_ref().unwrap().components.is_empty());
    let mut w = Writer::new();
    Offers::write(&offers, &mut w);
    assert_eq!(w.data, bytes);
    for len in 0..bytes.len() {
        assert!(Offers::read(&mut Reader::new(&bytes[..len])).is_err());
    }
}

#[test]
fn custom_chat_completions_are_an_action_and_a_string_list() {
    // ClientboundCustomChatCompletionsPacket.STREAM_CODEC: Action.STREAM_CODEC
    // (ByteBufCodecs.idMapper, a VarInt), then STRING_UTF8 in a list.
    let payload = b"Steve#tag";
    let packet = Clientbound::CustomChatCompletions(CustomChatCompletions {
        action: custom_chat_action::SET,
        entries: vec!["Steve".into(), "#tag".into()],
    });
    assert_eq!(
        Clientbound::decode(
            crate::ids::play::clientbound::CUSTOM_CHAT_COMPLETIONS,
            payload
        )
        .unwrap(),
        packet
    );
    assert_eq!(&packet.encode()[1..], payload);
    round_trip(packet, Clientbound::encode, Clientbound::decode);
    assert!(
        Clientbound::decode(
            crate::ids::play::clientbound::CUSTOM_CHAT_COMPLETIONS,
            &payload[..payload.len() - 1]
        )
        .is_err()
    );
}

#[test]
fn serverbound_abilities_carry_only_flying() {
    // ServerboundPlayerAbilitiesPacket.write: one byte, 2 when flying.
    let flying = Serverbound::PlayerAbilities(PlayerAbilitiesRequest { flags: 2 });
    assert_eq!(
        flying.encode(),
        [crate::ids::play::serverbound::PLAYER_ABILITIES as u8, 2]
    );
    round_trip(flying, Serverbound::encode, Serverbound::decode);
}

#[test]
fn world_options_packets_match_vanilla_stream_codecs() {
    // mcsrc.dev 26.3: difficulty is a VarInt, locking is a Boolean,
    // rule entries are ResourceKey identifiers followed by UTF-8 values.
    let difficulty = Serverbound::ChangeDifficulty(ChangeDifficultyRequest { difficulty: 3 });
    assert_eq!(&difficulty.encode()[1..], &[3]);
    round_trip(difficulty, Serverbound::encode, Serverbound::decode);
    let lock = Serverbound::LockDifficulty(LockDifficulty { locked: true });
    assert_eq!(&lock.encode()[1..], &[1]);
    round_trip(lock, Serverbound::encode, Serverbound::decode);
    let payload = b"\x01\x18minecraft:keep_inventory\x04true";
    let entry = GameRuleEntry {
        name: "minecraft:keep_inventory".into(),
        value: "true".into(),
    };
    let set = Serverbound::SetGameRule(SetGameRule {
        entries: vec![entry.clone()],
    });
    assert_eq!(&set.encode()[1..], payload);
    assert_eq!(
        Serverbound::decode(crate::ids::play::serverbound::SET_GAME_RULE, payload).unwrap(),
        set
    );
    let values = Clientbound::GameRuleValues(GameRuleValues {
        entries: vec![entry],
    });
    assert_eq!(&values.encode()[1..], payload);
    assert_eq!(
        Clientbound::decode(crate::ids::play::clientbound::GAME_RULE_VALUES, payload).unwrap(),
        values
    );
    assert!(
        Serverbound::decode(
            crate::ids::play::serverbound::SET_GAME_RULE,
            &payload[..payload.len() - 1]
        )
        .is_err()
    );
}

/// ServerboundRecipeBookSeenRecipePacket is one `RecipeDisplayId`, a
/// VarInt: display 300 is the two bytes `AC 02`, and a byte list that
/// stops early is not a display id.
#[test]
fn a_seen_recipe_is_one_varint_display_id() {
    let payload = [0xAC, 0x02];
    let seen = Serverbound::RecipeBookSeenRecipe(RecipeBookSeenRecipe { recipe: 300 });
    assert_eq!(
        Serverbound::decode(
            crate::ids::play::serverbound::RECIPE_BOOK_SEEN_RECIPE,
            &payload
        )
        .unwrap(),
        seen
    );
    assert_eq!(&seen.encode()[1..], payload);
    assert!(
        Serverbound::decode(
            crate::ids::play::serverbound::RECIPE_BOOK_SEEN_RECIPE,
            &payload[..1]
        )
        .is_err()
    );
}

/// ServerboundSeenAdvancementsPacket: the action, then the tab only when the player opened
/// one — a closed screen is the action on its own.
#[test]
fn a_seen_advancements_packet_carries_its_tab_only_when_one_was_opened() {
    let opened = SeenAdvancements {
        action: 0,
        tab: Some("minecraft:story/root".into()),
    };
    let mut w = crate::buf::Writer::new();
    w.varint(0);
    w.string("minecraft:story/root");
    assert_eq!(
        Serverbound::decode(crate::ids::play::serverbound::SEEN_ADVANCEMENTS, &w.data).unwrap(),
        Serverbound::SeenAdvancements(opened.clone())
    );
    assert_eq!(
        &Serverbound::SeenAdvancements(opened).encode()[1..],
        &w.data[..]
    );
    // CLOSED_SCREEN: the action alone, with nothing after it.
    let closed = [0x01];
    assert_eq!(
        Serverbound::decode(crate::ids::play::serverbound::SEEN_ADVANCEMENTS, &closed).unwrap(),
        Serverbound::SeenAdvancements(SeenAdvancements {
            action: 1,
            tab: None,
        })
    );
    // Both packets are typed now, so neither reads as an unknown opcode, and a packet too
    // short for its tab is malformed rather than a tab-less open.
    assert!(!matches!(
        Serverbound::decode(crate::ids::play::serverbound::SEEN_ADVANCEMENTS, &closed).unwrap(),
        Serverbound::Unknown(..)
    ));
    assert!(
        Serverbound::decode(crate::ids::play::serverbound::SEEN_ADVANCEMENTS, &[0x00]).is_err()
    );
    let tab = Clientbound::SelectAdvancementsTab(SelectAdvancementsTab {
        tab: Some("minecraft:story/root".into()),
    });
    assert_eq!(
        Clientbound::decode(
            crate::ids::play::clientbound::SELECT_ADVANCEMENTS_TAB,
            &tab.encode()[1..]
        )
        .unwrap(),
        tab
    );
    let none = Clientbound::SelectAdvancementsTab(SelectAdvancementsTab { tab: None });
    assert_eq!(
        &none.encode()[1..],
        &[0x00],
        "no tab is the absence flag alone"
    );
}

fn spawn() -> SpawnInfo {
    SpawnInfo {
        dimension_type: 0,
        dimension: "minecraft:overworld".into(),
        seed: -123,
        game_type: 0,
        previous_game_type: None,
        debug: false,
        flat: false,
        last_death: Some(GlobalPos {
            dimension: "minecraft:the_nether".into(),
            pos: [1, 2, 3],
        }),
        portal_cooldown: 0,
        sea_level: 63,
    }
}

#[test]
fn tracked_waypoint_bytes() {
    // TRACK, a UUID, the default style, red (#FF5555), a block position.
    let track = Clientbound::TrackedWaypoint(TrackedWaypoint {
        operation: waypoint_operation::TRACK,
        id: WaypointId::Uuid(0x0102_0304_0506_0708_090a_0b0c_0d0e_0f10),
        style: "minecraft:default".into(),
        color: Some(0xFFFF_5555_u32 as i32),
        target: WaypointTarget::Block([1, -2, 300]),
    });
    let mut expected = vec![141, 1, 0, 1];
    expected.extend(1..=16u8);
    expected.push(17);
    expected.extend(b"minecraft:default");
    expected.extend([
        1, 0xFF, 0x55, 0x55, 1, 1, 0xFE, 0xFF, 0xFF, 0xFF, 0x0F, 0xAC, 0x02,
    ]);
    assert_eq!(track.encode(), expected);
    assert_eq!(Clientbound::decode(141, &expected[2..]).unwrap(), track);
    for target in [
        WaypointTarget::Empty,
        WaypointTarget::Chunk([-3, 4]),
        WaypointTarget::Azimuth(-1.25),
    ] {
        let packet = Clientbound::TrackedWaypoint(TrackedWaypoint {
            operation: waypoint_operation::UPDATE,
            id: WaypointId::Name("spawn".into()),
            style: "minecraft:bowtie".into(),
            color: None,
            target,
        });
        let bytes = packet.encode();
        assert_eq!(Clientbound::decode(141, &bytes[2..]).unwrap(), packet);
    }
}

#[test]
fn clientbound_round_trips() {
    let packets = vec![
        Clientbound::BundleDelimiter(Empty),
        Clientbound::LowDiskSpaceWarning(Empty),
        Clientbound::PongResponse(crate::packets::status::Ping { time: 1_234_567 }),
        Clientbound::SetBorderCenter(SetBorderCenter { x: 1.5, z: -2.0 }),
        Clientbound::SetBorderLerpSize(SetBorderLerpSize {
            old_size: 10.0,
            new_size: 200.0,
            lerp_time: 1200,
        }),
        Clientbound::SetBorderSize(SetBorderSize { size: 30.0 }),
        Clientbound::SetBorderWarningDelay(SetBorderWarningDelay { delay: 300 }),
        Clientbound::SetBorderWarningDistance(SetBorderWarningDistance { blocks: 5 }),
        Clientbound::SetDisplayObjective(SetDisplayObjective {
            slot: 0,
            objective: "kills".into(),
        }),
        Clientbound::ResetScore(ResetScore {
            owner: "Steve".into(),
            objective: Some("kills".into()),
        }),
        Clientbound::SetScore(SetScore {
            owner: "Steve".into(),
            objective: "kills".into(),
            score: -3,
            display: Some(Tag::String("S".into())),
            number_format: Some(NumberFormat::Styled(Tag::compound([(
                "color",
                Tag::String("red".into()),
            )]))),
        }),
        Clientbound::SetObjective(SetObjective {
            name: "kills".into(),
            method: 0,
            display: Some((Tag::String("Kills".into()), 1, Some(NumberFormat::Blank))),
        }),
        Clientbound::SetObjective(SetObjective {
            name: "kills".into(),
            method: 1,
            display: None,
        }),
        Clientbound::SetPlayerTeam(SetPlayerTeam {
            name: "red".into(),
            method: 0,
            parameters: Some(TeamParameters {
                display_name: Tag::String("Red".into()),
                prefix: Tag::String("[R] ".into()),
                suffix: Tag::String("".into()),
                name_tag_visibility: 0,
                collision_rule: 0,
                color: Some(12),
                options: 3,
            }),
            players: vec!["Steve".into(), "Alex".into()],
        }),
        Clientbound::SetPlayerTeam(SetPlayerTeam {
            name: "red".into(),
            method: 4,
            parameters: None,
            players: vec!["Alex".into()],
        }),
        Clientbound::AddEntity(AddEntity {
            id: 5,
            uuid: 9,
            kind: 100,
            position: [1.5, 64.0, -3.25],
            velocity: [0.0; 3],
            pitch: 0,
            yaw: 64,
            head_yaw: 64,
            data: 0,
        }),
        Clientbound::BlockUpdate(BlockUpdate {
            pos: [-5, -60, 7],
            state: 1,
        }),
        Clientbound::AddTransientBlock(AddTransientBlock {
            pos: [-5, -60, 7],
            state: 1,
        }),
        Clientbound::ContainerSetContent(ContainerSetContent {
            container: 0,
            state: 3,
            items: vec![None, Some(Stack::new(1, 64))],
            carried: None,
        }),
        Clientbound::ContainerSetSlot(ContainerSetSlot {
            container: 0,
            state: 4,
            slot: 36,
            item: Some(Stack::new(2, 1)),
        }),
        Clientbound::Disconnect(Disconnect {
            reason: literal("kicked"),
        }),
        Clientbound::EntityPositionSync(EntityPositionSync {
            id: 1,
            path: PositionPath::Stepped(vec![([1.0, 2.0, 3.0], 0), ([1.5, 2.0, 3.0], 2)]),
            yaw: 1.0,
            pitch: 2.0,
            on_ground: true,
        }),
        Clientbound::ChunksBiomes(ChunksBiomes {
            chunks: vec![
                ChunkBiomeData {
                    pos: [-3, 7],
                    buffer: vec![0, 4, 1, 2],
                },
                ChunkBiomeData {
                    pos: [i32::MAX, i32::MIN],
                    buffer: Vec::new(),
                },
            ],
        }),
        Clientbound::ForgetLevelChunk(ForgetLevelChunk { pos: [-1, 2] }),
        Clientbound::GameEvent(GameEvent {
            event: 13,
            param: 0.0,
        }),
        Clientbound::LevelChunkWithLight(LevelChunkWithLight {
            x: 3,
            z: -4,
            chunk: ChunkData {
                heightmaps: vec![(4, vec![0; 37])],
                buffer: vec![0, 0, 0, 0, 0, 0, 0, 0],
                block_entities: vec![BlockEntityInfo {
                    packed_xz: 1,
                    y: 70,
                    kind: 3,
                    tag: Tag::End,
                }],
            },
            light: LightData {
                sky_mask: vec![1],
                block_mask: vec![],
                empty_sky_mask: vec![],
                empty_block_mask: vec![2],
                sky: vec![vec![0xFF; 2048]],
                block: vec![],
            },
        }),
        Clientbound::Login(Login {
            player_id: 42,
            hardcore: false,
            levels: vec!["minecraft:overworld".into(), "minecraft:the_nether".into()],
            max_players: 20,
            chunk_radius: 10,
            simulation_distance: 10,
            reduced_debug_info: false,
            show_death_screen: true,
            limited_crafting: false,
            spawn: spawn(),
            online_mode: false,
            enforces_secure_chat: false,
        }),
        Clientbound::MoveEntityPos(MoveEntityPos(MoveEntity {
            id: 3,
            delta: Some(VecDelta::Linear([4096, 0, -4096])),
            rotation: None,
            on_ground: true,
        })),
        Clientbound::MoveEntityPosRot(MoveEntityPosRot(MoveEntity {
            id: 3,
            delta: Some(VecDelta::Stepped(vec![([1, 2, 3], 1), ([0, 0, 1], 2)])),
            rotation: Some((10, -10)),
            on_ground: false,
        })),
        Clientbound::MoveEntityRot(MoveEntityRot(MoveEntity {
            id: 3,
            delta: None,
            rotation: Some((1, 2)),
            on_ground: true,
        })),
        Clientbound::OpenScreen(OpenScreen {
            container: 1,
            menu: 2,
            title: literal("Chest"),
        }),
        Clientbound::MountScreenOpen(MountScreenOpen {
            container: 4,
            columns: 5,
            entity: 300,
        }),
        Clientbound::MerchantOffers(MerchantOffers {
            container: 3,
            offers: vec![NetOffer {
                cost_a: Stack::new(5, 24).into(),
                result: Stack::new(7, 1),
                cost_b: Some(Stack::new(9, 1).into()),
                out_of_stock: false,
                uses: 2,
                max_uses: 16,
                xp: 2,
                special_price: -3,
                price_multiplier: 0.05,
                demand: 4,
            }],
            level: 2,
            xp: 12,
            show_progress: true,
            can_restock: true,
        }),
        Clientbound::DeleteChat(DeleteChat {
            signature: (4, None),
        }),
        Clientbound::DeleteChat(DeleteChat {
            signature: (-1, Some(vec![9; 256])),
        }),
        Clientbound::PlayerChat(PlayerChat {
            global_index: 0,
            sender: 5,
            index: 0,
            signature: None,
            content: "hello".into(),
            timestamp: 1,
            salt: 2,
            last_seen: vec![(3, None), (-1, Some(vec![7; 256]))],
            unsigned_content: None,
            filter: (0, vec![]),
            chat_type: ChatTypeBound {
                chat_type: 0,
                name: literal("Steve"),
                target: None,
            },
        }),
        Clientbound::PlayerInfoUpdate(PlayerInfoUpdate {
            actions: 0b0001_1101,
            entries: vec![PlayerInfo {
                uuid: 1,
                name: Some("Steve".into()),
                game_mode: Some(1),
                listed: Some(true),
                latency: Some(12),
                ..PlayerInfo::default()
            }],
        }),
        Clientbound::PlayerPosition(PlayerPosition {
            teleport: 1,
            position: [0.5, 70.0, 0.5],
            delta: [0.0; 3],
            yaw: 90.0,
            pitch: 0.0,
            relatives: relative::DELTA_X,
        }),
        Clientbound::Respawn(Respawn {
            spawn: spawn(),
            keep: 3,
        }),
        Clientbound::SectionBlocksUpdate(SectionBlocksUpdate {
            section: [1, -4, 2],
            changes: vec![(9 << 12) | 0x123],
        }),
        Clientbound::SetEntityData(SetEntityData {
            id: 2,
            values: vec![(8, 7, Value::Item(Some(Stack::new(5, 3))))],
        }),
        Clientbound::SetEntityMotion(SetEntityMotion {
            id: 2,
            velocity: [0.0; 3],
        }),
        Clientbound::SetPassengers(SetPassengers {
            vehicle: 2,
            passengers: vec![3, 300],
        }),
        Clientbound::SetEntityLink(SetEntityLink { source: 2, dest: 0 }),
        Clientbound::LevelParticles(LevelParticles {
            particle: Particle {
                id: 21,
                data: Some(crate::entity_data::ParticleData::ColorScale(0xFF0000, 1.5)),
            },
            override_limiter: true,
            always_show: false,
            pos: [0.5, 101.0, 3.0],
            dist: [0.2, 0.0, 0.2],
            max_speed: [0.0; 3],
            count: 60,
            randomization: 0,
        }),
        Clientbound::Explode(Explode {
            center: [1.0, 2.0, 3.0],
            radius: 4.0,
            block_count: 12,
            knockback: Some([0.5, 0.1, 0.0]),
            particle: Particle { id: 29, data: None },
            sound: (-1, Some(("minecraft:entity.generic.explode".into(), None))),
            block_particles: vec![ExplosionParticle {
                particle: Particle { id: 69, data: None },
                scaling: 0.5,
                speed: 1.0,
                weight: 1,
            }],
            play_sound: true,
        }),
        Clientbound::ProjectilePower(ProjectilePower { id: 4, power: 0.1 }),
        Clientbound::SetEquipment(SetEquipment {
            id: 2,
            slots: vec![(0, None), (5, Some(Stack::new(1, 1)))],
        }),
        Clientbound::UpdateAttributes(UpdateAttributes {
            id: 2,
            values: vec![AttributeSnapshot {
                attribute: 26,
                base: 0.23,
                modifiers: vec![AttributeModifier {
                    id: "minecraft:baby".into(),
                    amount: 0.5,
                    operation: 1,
                }],
            }],
        }),
        Clientbound::DamageEvent(DamageEvent {
            id: 7,
            source_type: 31,
            cause: Some(0),
            direct: None,
            position: Some([1.5, -2.0, 3.25]),
        }),
        Clientbound::SetHealth(SetHealth {
            health: 17.5,
            food: 18,
            saturation: 3.0,
        }),
        Clientbound::SetTime(SetTime {
            game_time: 100,
            clocks: vec![ClockState {
                clock: 0,
                total_ticks: 6000,
                partial_tick: 0.0,
                rate: 1.0,
            }],
        }),
        Clientbound::Sound(Sound {
            sound: (12, None),
            source: 4,
            x: 8,
            y: 9,
            z: 10,
            volume: 1.0,
            pitch: 1.0,
            seed: 3,
        }),
        Clientbound::Sound(Sound {
            sound: (-1, Some(("minecraft:x".into(), Some(16.0)))),
            source: 0,
            x: 0,
            y: 0,
            z: 0,
            volume: 1.0,
            pitch: 1.0,
            seed: 0,
        }),
        Clientbound::SystemChat(SystemChat {
            content: literal("hi"),
            overlay: false,
        }),
        Clientbound::TeleportEntity(TeleportEntity {
            id: 1,
            position: [1.0; 3],
            delta: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            relatives: 0,
            on_ground: true,
        }),
        Clientbound::TickingState(TickingState {
            rate: 40.0,
            frozen: true,
        }),
        Clientbound::TickingStep(TickingStep { steps: 7 }),
        Clientbound::AwardStats(AwardStats {
            stats: vec![(8, 3, 120), (0, 1, 2)],
        }),
        Clientbound::UpdateAdvancements(UpdateAdvancements {
            reset: true,
            added: vec![Advancement {
                id: "minecraft:story/root".into(),
                parent: None,
                display: Some(AdvancementDisplay {
                    title: literal("Minecraft"),
                    description: literal("The heart and story of the game"),
                    icon: crate::item::Stack::new(42, 1),
                    frame: 0,
                    show_toast: false,
                    hidden: false,
                    background: Some("minecraft:block/stone".into()),
                }),
                requirements: vec![vec!["crafting_table".into()]],
                telemetry: false,
                x: 0.0,
                y: 1.5,
            }],
            removed: vec!["minecraft:story/gone".into()],
            progress: vec![(
                "minecraft:story/root".into(),
                vec![("crafting_table".into(), Some(1_792_216_800_000))],
            )],
            show: true,
        }),
    ];
    for p in packets {
        round_trip(p, Clientbound::encode, Clientbound::decode);
    }
}

#[test]
fn delete_chat_decodes_a_packed_signature() {
    use crate::ids::play::clientbound::DELETE_CHAT;
    // MessageSignature.Packed: id + 1, so 5 names cache slot 4...
    assert_eq!(
        Clientbound::decode(DELETE_CHAT, &[5]).unwrap(),
        Clientbound::DeleteChat(DeleteChat {
            signature: (4, None)
        })
    );
    // ...and 0 carries the full 256-byte signature.
    let mut full = vec![0];
    full.extend([3; 256]);
    assert_eq!(
        Clientbound::decode(DELETE_CHAT, &full).unwrap(),
        Clientbound::DeleteChat(DeleteChat {
            signature: (-1, Some(vec![3; 256]))
        })
    );
}

#[test]
fn serverbound_round_trips() {
    let packets = vec![
        Serverbound::PingRequest(crate::packets::status::Ping { time: -7 }),
        Serverbound::AcceptTeleportation(AcceptTeleportation {
            id: 1,
            x: 0.5,
            y: 64.0,
            z: 0.5,
            yaw: 0.0,
            pitch: 0.0,
        }),
        Serverbound::Chat(Chat {
            message: "hello".into(),
            timestamp: 1,
            salt: 0,
            signature: None,
            last_seen: LastSeenUpdate {
                offset: 0,
                acknowledged: [0; 3],
                checksum: 0,
            },
        }),
        Serverbound::ChatCommand(ChatCommand {
            command: "time set day".into(),
        }),
        Serverbound::ChatAck(ChatAck { offset: 3 }),
        Serverbound::ChatCommandSigned(ChatCommandSigned {
            command: "say hi".into(),
            timestamp: 1_700_000_000_000,
            salt: -5,
            arguments: vec![("message".into(), vec![9; 256])],
            last_seen: LastSeenUpdate {
                offset: 1,
                acknowledged: [1, 0, 8],
                checksum: -3,
            },
        }),
        Serverbound::ChatSessionUpdate(ChatSessionUpdate {
            session: 0x1234,
            expires_at: 1_800_000_000_000,
            key: vec![0x30; 294],
            key_signature: vec![7; 512],
        }),
        Serverbound::ChunkBatchReceived(ChunkBatchReceived {
            chunks_per_tick: 9.5,
        }),
        Serverbound::ClientCommand(ClientCommand { action: 0 }),
        Serverbound::ClientTickEnd(Empty),
        Serverbound::ContainerClick(ContainerClick {
            container: 0,
            state: 5,
            slot: 36,
            button: 0,
            input: 0,
            changed: vec![(36, None)],
            carried: Some(Hashed::of(&Stack::new(1, 64))),
        }),
        Serverbound::MovePlayerPosRot(MovePlayerPosRot {
            x: 1.0,
            y: 2.0,
            z: 3.0,
            yaw: 4.0,
            pitch: 5.0,
            flags: 1,
        }),
        Serverbound::MovePlayerStatus(MovePlayerStatus { flags: 3 }),
        Serverbound::PlayerAction(PlayerAction {
            action: 0,
            pos: [1, 2, 3],
            direction: 1,
            sequence: 7,
        }),
        Serverbound::PlayerInput(PlayerInput { flags: 0b0101_0001 }),
        Serverbound::PlayerLoaded(Empty),
        Serverbound::Punch(Empty),
        Serverbound::SetCarriedItem(SetCarriedItem { slot: 4 }),
        Serverbound::UseItemOn(UseItemOn {
            hand: 0,
            pos: [1, 64, 1],
            face: 1,
            cursor: [0.5, 1.0, 0.5],
            inside: false,
            world_border: false,
            sequence: 8,
        }),
        Serverbound::Interact(Interact {
            id: 9,
            hand: 0,
            location: [0.25, 1.0, 0.0],
            secondary: false,
        }),
    ];
    for p in packets {
        if let Serverbound::Interact(_) = &p {
            // LpVec3 is lossy; check bytes instead.
            let body = p.encode();
            let mut r = Reader::new(&body);
            let id = r.varint().unwrap();
            assert!(matches!(
                Serverbound::decode(id, r.rest()).unwrap(),
                Serverbound::Interact(Interact { id: 9, .. })
            ));
            continue;
        }
        round_trip(p, Serverbound::encode, Serverbound::decode);
    }
    round_trip(
        Serverbound::SelectTrade(SelectTrade { item: 3 }),
        Serverbound::encode,
        Serverbound::decode,
    );
    // CONTAINER_ID and the button, both VarInts.
    let button = Serverbound::ContainerButtonClick(ContainerButtonClick {
        container: 3,
        button: 200,
    });
    assert_eq!(&button.encode()[1..], &[3, 0xC8, 1]);
    round_trip(button, Serverbound::encode, Serverbound::decode);
    // The name, a UTF-8 string with its VarInt length.
    let rename = Serverbound::RenameItem(RenameItem {
        name: "Blade".into(),
    });
    assert_eq!(&rename.encode()[1..], &[5, b'B', b'l', b'a', b'd', b'e']);
    round_trip(rename, Serverbound::encode, Serverbound::decode);
    // The slot, the container id (VarInts), then the new state.
    let toggle = Serverbound::ContainerSlotStateChanged(ContainerSlotStateChanged {
        slot: 4,
        container: 2,
        enabled: false,
    });
    assert_eq!(&toggle.encode()[1..], &[4, 2, 0]);
    round_trip(toggle, Serverbound::encode, Serverbound::decode);
    // Two optional mob effect ids: a presence byte, then the VarInt.
    let beacon = Serverbound::SetBeacon(SetBeacon {
        primary: Some(2),
        secondary: None,
    });
    assert_eq!(&beacon.encode()[1..], &[1, 2, 0]);
    round_trip(beacon, Serverbound::encode, Serverbound::decode);
}

#[test]
fn a_cooldown_packet_carries_its_group_and_ticks() {
    let packet = Clientbound::Cooldown(Cooldown {
        group: "minecraft:ender_pearl".into(),
        duration: 20,
    });
    round_trip(packet, Clientbound::encode, Clientbound::decode);
    // The group is an identifier, the ticks a VarInt; a duration of 0
    // removes the entry.
    let body = Clientbound::encode(&Clientbound::Cooldown(Cooldown {
        group: "minecraft:ender_pearl".into(),
        duration: 0,
    }));
    assert_eq!(body[0], crate::ids::play::clientbound::COOLDOWN as u8);
    assert_eq!(&body[1..3], [21, b'm']);
    assert_eq!(body[body.len() - 1], 0);
}

#[test]
fn debug_tag_queries_decode_as_their_packets() {
    use crate::ids::play::{clientbound, serverbound};
    // ServerboundBlockEntityTagQueryPacket: VarInt transaction, BlockPos.
    let pos = [3, 64, -2];
    let mut body = vec![7];
    body.extend_from_slice(&crate::buf::pack_block_pos(pos).to_be_bytes());
    let packet = Serverbound::decode(serverbound::BLOCK_ENTITY_TAG_QUERY, &body).unwrap();
    assert_eq!(
        packet,
        Serverbound::BlockEntityTagQuery(BlockEntityTagQuery {
            transaction: 7,
            pos
        })
    );
    round_trip(packet, Serverbound::encode, Serverbound::decode);
    // ServerboundEntityTagQueryPacket: VarInt transaction, VarInt entity id.
    let packet = Serverbound::decode(serverbound::ENTITY_TAG_QUERY, &[0, 42]).unwrap();
    assert_eq!(
        packet,
        Serverbound::EntityTagQuery(EntityTagQuery {
            transaction: 0,
            entity: 42
        })
    );
    round_trip(packet, Serverbound::encode, Serverbound::decode);
    // ClientboundTagQueryPacket: VarInt transaction, then the nullable
    // compound of FriendlyByteBuf.writeNbt (a bare End tag for null).
    let packet = Clientbound::decode(clientbound::TAG_QUERY, &[1, 0]).unwrap();
    assert_eq!(
        packet,
        Clientbound::TagQuery(TagQuery {
            transaction: 1,
            tag: Tag::End
        })
    );
    let mut items = horizon_world::nbt::Compound::new();
    items.insert("Lock".into(), Tag::String("key".into()));
    round_trip(
        Clientbound::TagQuery(TagQuery {
            transaction: 2,
            tag: Tag::Compound(items),
        }),
        Clientbound::encode,
        Clientbound::decode,
    );
}

#[test]
fn change_game_mode_uses_the_game_type_varint() {
    for mode in 0..=3 {
        let packet = Serverbound::ChangeGameMode(ChangeGameMode { mode });
        assert_eq!(packet.encode(), vec![5, mode as u8]);
        round_trip(packet, Serverbound::encode, Serverbound::decode);
    }
}

#[test]
fn move_entity_deltas_decode_like_vec_delta_codec() {
    let base = [10.0, 64.0, -3.5];
    assert_eq!(
        VecDelta::Linear([4096, 0, -2048]).end(base),
        [11.0, 64.0, -4.0]
    );
    // Zero deltas keep unquantized base coordinates exactly.
    assert_eq!(
        VecDelta::Linear([0, 0, 0]).end([0.123_456_789, 1.0, 2.0]),
        [0.123_456_789, 1.0, 2.0]
    );
    assert_eq!(
        VecDelta::Stepped(vec![([4096, 0, 0], 1), ([4096, 0, 0], 2)]).end(base),
        [12.0, 64.0, -3.5]
    );
}

#[test]
fn player_look_at_round_trips() {
    let at_point = PlayerLookAt {
        from: 1,
        target: [1.5, 64.0, -2.25],
        entity: None,
    };
    let at_entity = PlayerLookAt {
        from: 0,
        target: [0.0, 70.0, 0.0],
        entity: Some(LookAtEntity { id: 42, anchor: 1 }),
    };
    for p in [at_point, at_entity] {
        let bytes = Clientbound::PlayerLookAt(p.clone()).encode();
        // 1 id byte, 1 anchor, 3 doubles, the flag; the entity adds 2 VarInts.
        let len = 1 + 1 + 24 + 1 + if p.entity.is_some() { 2 } else { 0 };
        assert_eq!(bytes.len(), len);
        assert_eq!(
            Clientbound::decode(crate::ids::play::clientbound::PLAYER_LOOK_AT, &bytes[1..])
                .unwrap(),
            Clientbound::PlayerLookAt(p)
        );
    }
}

#[test]
fn post_effects_round_trips() {
    let p = PostEffects {
        effects: vec!["minecraft:invert".into(), "pack:glow".into()],
    };
    let bytes = Clientbound::PostEffects(p.clone()).encode();
    assert_eq!(
        Clientbound::decode(crate::ids::play::clientbound::POST_EFFECTS, &bytes[1..]).unwrap(),
        Clientbound::PostEffects(p)
    );
    // The empty list the server sends on join.
    assert_eq!(
        Clientbound::decode(crate::ids::play::clientbound::POST_EFFECTS, &[0]).unwrap(),
        Clientbound::PostEffects(PostEffects {
            effects: Vec::new()
        })
    );
}

/// ClientboundChunksBiomesPacket is read, not passed through as an
/// unknown packet: one column (1, -2) with two bytes of biomes.
#[test]
fn chunks_biomes_is_a_known_packet() {
    let id = crate::ids::play::clientbound::CHUNKS_BIOMES;
    let payload = [1, 0xFF, 0xFF, 0xFF, 0xFE, 0, 0, 0, 1, 2, 5, 6];
    let p = Clientbound::decode(id, &payload).unwrap();
    assert!(!matches!(p, Clientbound::Unknown(..)), "{p:?}");
    assert_eq!(p.encode()[1..], payload);
    // Cut short, it is an error rather than a packet.
    assert!(Clientbound::decode(id, &payload[..10]).is_err());
}

/// ClientboundChunksBiomesPacket.STREAM_CODEC: a list of (ChunkPos as
/// one long, x low and z high; `byteArray(2097152)`), the bytes being
/// each section's biome container (`ChunkBiomeData.extractChunkData`).
#[test]
fn chunks_biomes_wire_and_biomes() {
    let p = Clientbound::ChunksBiomes(ChunksBiomes {
        chunks: vec![ChunkBiomeData {
            pos: [1, -2],
            buffer: vec![5, 6],
        }],
    });
    let bytes = p.encode();
    assert_eq!(
        bytes,
        [
            crate::ids::play::clientbound::CHUNKS_BIOMES as u8,
            1,
            0xFF,
            0xFF,
            0xFF,
            0xFE,
            0,
            0,
            0,
            1,
            2,
            5,
            6
        ]
    );
    assert_eq!(
        Clientbound::decode(bytes[0].into(), &bytes[1..]).unwrap(),
        p
    );
    use crate::chunk::{Paletted, Strategy};
    let sections = [
        Paletted::from_values(&[3; 64], Strategy::Biomes, 6),
        Paletted::from_values(
            &(0..64).map(|i| i % 5).collect::<Vec<_>>(),
            Strategy::Biomes,
            6,
        ),
    ];
    let data = ChunkBiomeData::from_biomes([4, 4], &sections);
    assert_eq!(data.biomes(2, 6).unwrap(), sections);
}

/// ClientboundSetCameraPacket: one VarInt, the camera entity's id.
#[test]
fn set_camera_is_the_camera_entity_id() {
    // 300 as a VarInt: 0xAC 0x02.
    let p = Clientbound::decode(crate::ids::play::clientbound::SET_CAMERA, &[0xAC, 0x02]).unwrap();
    assert!(!matches!(p, Clientbound::Unknown(..)), "{p:?}");
    assert!(format!("{p:?}").contains("camera_id: 300"), "{p:?}");
    assert_eq!(p.encode()[1..], [0xAC, 0x02]);
}

/// ServerboundSpectatorActionPacket: `OPTIONAL_VAR_INT`, the clicked
/// entity's id plus one, or 0 for a click on nothing.
#[test]
fn spectator_action_is_an_optional_entity_id() {
    let id = crate::ids::play::serverbound::SPECTATOR_ACTION;
    for (bytes, shown) in [([8u8], "Some(7)"), ([0u8], "None")] {
        let p = Serverbound::decode(id, &bytes).unwrap();
        assert!(!matches!(p, Serverbound::Unknown(..)), "{p:?}");
        assert!(format!("{p:?}").contains(shown), "{p:?}");
        assert_eq!(p.encode()[1..], bytes);
    }
}

#[test]
fn unknown_packets_keep_their_payload() {
    let p = Clientbound::decode(crate::ids::play::clientbound::DEBUG_SAMPLE, &[1, 2, 3]).unwrap();
    assert_eq!(
        p,
        Clientbound::Unknown(crate::ids::play::clientbound::DEBUG_SAMPLE, vec![1, 2, 3])
    );
    assert_eq!(p.encode()[1..], [1, 2, 3]);
}

#[test]
fn update_attributes_encode_like_vanilla() {
    let p = Clientbound::UpdateAttributes(UpdateAttributes {
        id: 5,
        values: vec![AttributeSnapshot {
            attribute: 1,
            base: 0.0,
            modifiers: vec![AttributeModifier {
                id: "minecraft:armor.helmet".into(),
                amount: 3.0,
                operation: 0,
            }],
        }],
    });
    let body = p.encode();
    // The packet id (134) is a two-byte VarInt.
    let mut expected = vec![134, 1, 5, 1, 1];
    expected.extend(0.0f64.to_be_bytes());
    expected.extend([1, 22]);
    expected.extend(b"minecraft:armor.helmet");
    expected.extend(3.0f64.to_be_bytes());
    expected.push(0);
    assert_eq!(body, expected);
    assert_eq!(crate::ids::play::clientbound::UPDATE_ATTRIBUTES, 134);
    assert_eq!(Clientbound::decode(134, &body[2..]).unwrap(), p);
}

#[test]
fn map_item_data_carries_a_server_patch() {
    let patch = MapPatch {
        start_x: 3,
        start_y: 4,
        width: 2,
        height: 1,
        colors: vec![1, 2],
    };
    let mut w = Writer::new();
    MapPatchOpt::write(&Some(patch.clone()), &mut w);
    assert_eq!(w.data, [2, 1, 3, 4, 2, 1, 2]);
    let p = Clientbound::MapItemData(MapItemData {
        map_id: 2146483647,
        scale: 1,
        locked: true,
        decorations: None,
        patch: Some(patch),
    });
    round_trip(p, Clientbound::encode, Clientbound::decode);
}

#[test]
fn sign_packets_match_vanilla_stream_codecs() {
    // ClientboundOpenSignEditorPacket: BlockPos, then SignTextSlot as a VarInt (FRONT is 1).
    let open = Clientbound::OpenSignEditor(OpenSignEditor {
        pos: [1, 2, 3],
        front: true,
    });
    let body = open.encode();
    assert_eq!(
        body[1..9],
        crate::buf::pack_block_pos([1, 2, 3]).to_be_bytes()
    );
    assert_eq!(body[9..], [1]);
    round_trip(open, Clientbound::encode, Clientbound::decode);
    // ServerboundSignUpdatePacket: BlockPos, four strings with no count, then the slot.
    let update = Serverbound::SignUpdate(SignUpdate {
        pos: [-4, 70, 9],
        lines: ["Hi".into(), String::new(), "x".into(), String::new()],
        front: false,
    });
    let body = update.encode();
    assert_eq!(body[9..], [2, b'H', b'i', 0, 1, b'x', 0, 0]);
    round_trip(update, Serverbound::encode, Serverbound::decode);
}

#[test]
fn book_packets_match_vanilla_stream_codecs() {
    // ClientboundOpenBookPacket: InteractionHand as a VarInt.
    let open = Clientbound::OpenBook(OpenBook { hand: 1 });
    assert_eq!(open.encode()[1..], [1]);
    round_trip(open, Clientbound::encode, Clientbound::decode);
    // ServerboundEditBookPacket: the slot, the pages (a counted list), then an optional title.
    let sign = Serverbound::EditBook(EditBook {
        slot: 40,
        pages: vec!["a".into(), String::new()],
        title: Some("T".into()),
    });
    assert_eq!(sign.encode()[1..], [40, 2, 1, b'a', 0, 1, 1, b'T']);
    round_trip(sign, Serverbound::encode, Serverbound::decode);
    let mut w = Writer::new();
    w.varint(101);
    assert!(BookPages::read(&mut Reader::new(&w.data)).is_err());
}

#[test]
fn boss_event_round_trips() {
    let add = Clientbound::BossEvent(BossEvent {
        id: 0x0123_4567_89ab_cdef_fedc_ba98_7654_3210,
        action: BossAction::Add {
            name: Tag::String("Wither".into()),
            progress: 0.5,
            color: 5,
            overlay: 0,
            darken: true,
            music: false,
            fog: true,
        },
    });
    let body = add.encode();
    // Packet id 9, then the uuid, operation 0, and the property byte
    // (darken 1, fog 4).
    assert_eq!(body[0], 9);
    assert_eq!(*body.last().unwrap(), 5);
    round_trip(add, Clientbound::encode, Clientbound::decode);
    round_trip(
        Clientbound::BossEvent(BossEvent {
            id: 7,
            action: BossAction::UpdateProgress { progress: 0.25 },
        }),
        Clientbound::encode,
        Clientbound::decode,
    );
    round_trip(
        Clientbound::BossEvent(BossEvent {
            id: 7,
            action: BossAction::Remove,
        }),
        Clientbound::encode,
        Clientbound::decode,
    );
}

/// A registry entry is its id + 1, the dialog itself a 0 then its NBT.
#[test]
fn dialogs_round_trip() {
    let notice = Tag::compound([("type", Tag::String("minecraft:notice".into()))]);
    for dialog in [DialogHolder::Registry(3), DialogHolder::Direct(notice)] {
        crate::packets::round_trip(
            Clientbound::ShowDialog(ShowDialog { dialog }),
            Clientbound::encode,
            Clientbound::decode,
        );
    }
    let mut w = Writer::new();
    DialogHolder::write(&DialogHolder::Registry(3), &mut w);
    assert_eq!(w.data, [4]);
    crate::packets::round_trip(
        Serverbound::CustomClickAction(CustomClickAction {
            id: "minecraft:custom".into(),
            payload: None,
        }),
        Serverbound::encode,
        Serverbound::decode,
    );
}

#[test]
fn set_command_minecart_writes_entity_command_then_track_output() {
    let packet = SetCommandMinecart {
        entity: 300,
        command: "say hi".into(),
        track_output: true,
    };
    round_trip(
        Serverbound::SetCommandMinecart(packet.clone()),
        Serverbound::encode,
        Serverbound::decode,
    );
    // ServerboundSetCommandMinecartPacket.write: VarInt, UTF, boolean.
    let mut w = Writer::new();
    SetCommandMinecart::write(&packet, &mut w);
    assert_eq!(
        w.data,
        [0xAC, 0x02, 6, b's', b'a', b'y', b' ', b'h', b'i', 1]
    );
}

#[test]
fn set_structure_block_round_trips_and_clamps_like_vanilla() {
    let packet = SetStructureBlock {
        pos: [-3, 70, 12],
        update_type: 1,
        mode: 0,
        name: "minecraft:house".into(),
        offset: [-48, 1, 48],
        size: [0, 48, 5],
        mirror: 2,
        rotation: 3,
        data: "meta".into(),
        ignore_entities: true,
        strict: false,
        show_air: true,
        show_bounding_box: true,
        integrity: 0.5,
        seed: -7,
    };
    round_trip(
        Serverbound::SetStructureBlock(packet.clone()),
        Serverbound::encode,
        Serverbound::decode,
    );
    // The flag byte: ignoreEntities 1, showAir 2, showBoundingBox 4, strict 8.
    let mut w = Writer::new();
    SetStructureBlock::write(&packet, &mut w);
    assert_eq!(*w.data.last().unwrap(), 1 | 2 | 4);
    // Offsets clamp to +-48, sizes to 0..=48 and the integrity to 0..=1.
    let wild = SetStructureBlock {
        offset: [-100, 100, 0],
        size: [-5, 100, 7],
        integrity: 4.0,
        ..packet
    };
    let mut w = Writer::new();
    SetStructureBlock::write(&wild, &mut w);
    let read = SetStructureBlock::read(&mut Reader::new(&w.data)).unwrap();
    assert_eq!(read.offset, [-48, 48, 0]);
    assert_eq!(read.size, [0, 48, 7]);
    assert_eq!(read.integrity, 1.0);
}

#[test]
fn test_instance_packets_round_trip() {
    let data = TestInstanceData {
        test: Some("minecraft:always_pass".into()),
        size: [3, 4, 5],
        rotation: 2,
        ignore_entities: true,
        status: 1,
        error_message: Some(Tag::String("bad".into())),
    };
    round_trip(
        Serverbound::TestInstanceBlockAction(TestInstanceBlockAction {
            pos: [1, 2, 3],
            action: 6,
            data: data.clone(),
        }),
        Serverbound::encode,
        Serverbound::decode,
    );
    let empty = TestInstanceData {
        test: None,
        error_message: None,
        ..data
    };
    let mut w = Writer::new();
    TestInstanceData::write(&empty, &mut w);
    // Absent test, three sizes, rotation, flag, status, absent error.
    assert_eq!(w.data, [0, 3, 4, 5, 2, 1, 1, 0]);
    round_trip(
        Clientbound::TestInstanceBlockStatus(TestInstanceBlockStatus {
            status: Tag::String("ok".into()),
            size: Some([1, 2, 3]),
        }),
        Clientbound::encode,
        Clientbound::decode,
    );
}

#[test]
fn game_test_highlight_pos_round_trips() {
    round_trip(
        Clientbound::GameTestHighlightPos(GameTestHighlightPos {
            absolute: [10, -64, 30],
            relative: [1, 2, 3],
        }),
        Clientbound::encode,
        Clientbound::decode,
    );
}

#[test]
fn set_test_block_round_trips() {
    round_trip(
        Serverbound::SetTestBlock(SetTestBlock {
            pos: [1, 2, 3],
            mode: 3,
            message: "passed".into(),
        }),
        Serverbound::encode,
        Serverbound::decode,
    );
}

/// A `SetTime` packet captured from a vanilla 26.3 server at join, two
/// clocks: `Long 13071`, then `VarInt 2`, `(0, 66678, 0.0, 1.0)` and
/// `(1, 13071, 0.0, 1.0)`. Reading the clock as a holder id + 1 turns the
/// first key into an inline definition and the packet stops decoding.
#[test]
fn a_time_clock_is_a_plain_registry_id() {
    let payload = [
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x33, 0x0f, // game_time 13071
        0x02, // two clocks
        0x00, 0xf6, 0x88, 0x04, 0x00, 0x00, 0x00, 0x00, 0x3f, 0x80, 0x00, 0x00, // clock 0
        0x01, 0x8f, 0x66, 0x00, 0x00, 0x00, 0x00, 0x3f, 0x80, 0x00, 0x00, // clock 1
    ];
    let decoded = Clientbound::decode(crate::ids::play::clientbound::SET_TIME, &payload).unwrap();
    let Clientbound::SetTime(t) = decoded else {
        panic!("expected SetTime")
    };
    assert_eq!(t.game_time, 13071);
    assert_eq!(t.clocks.len(), 2);
    assert_eq!((t.clocks[0].clock, t.clocks[0].total_ticks), (0, 66678));
    assert_eq!((t.clocks[1].clock, t.clocks[1].total_ticks), (1, 13071));
    assert_eq!(t.clocks[0].rate, 1.0);
}

#[test]
fn jigsaw_packets_round_trip() {
    round_trip(
        Serverbound::SetJigsawBlock(SetJigsawBlock {
            pos: [4, -60, 9],
            name: "minecraft:bottom".into(),
            target: "minecraft:top".into(),
            pool: "minecraft:village/plains/houses".into(),
            final_state: "minecraft:oak_planks".into(),
            joint: "aligned".into(),
            selection_priority: 3,
            placement_priority: -2,
        }),
        Serverbound::encode,
        Serverbound::decode,
    );
    round_trip(
        Serverbound::JigsawGenerate(JigsawGenerate {
            pos: [0, 64, 0],
            levels: 20,
            keep_jigsaws: true,
        }),
        Serverbound::encode,
        Serverbound::decode,
    );
}
