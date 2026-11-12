use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
use horizon_client::items::ItemStack;

/// #13541: a portal's `ClientboundRespawnPacket` keeps the attribute modifiers
/// (`KEEP_ATTRIBUTE_MODIFIERS`): `handleRespawn` gives the new `LocalPlayer`
/// `assignAllValues` of the old one's, the worn armour's among them, and a death's packet
/// only `assignBaseValues`. A new player without them had no armour points, toughness or
/// custom modifier after an End or Nether portal until the armour was worn again.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_new_player_after_a_portal_keeps_the_old_ones_modifiers() {
    use horizon_protocol::packets::play::{
        self, AttributeModifier, AttributeSnapshot, Clientbound as C,
    };
    let root = std::path::PathBuf::from(
        std::env::var_os("HORIZON_TEST_RESOURCES").expect("prepared resources required"),
    );
    let settings = Settings::default();
    let loaded = load_resources(
        root.clone(),
        None,
        &root,
        &settings,
        std::time::Instant::now(),
        false,
        true,
        &Progress::new(),
        &mut |_, _| unreachable!(),
    )
    .unwrap();
    let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
    options.resources = root.clone();
    let mut game = Game::new(
        &loaded.resources,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(&root),
        None,
    )
    .unwrap();
    let (pipe, _server) = horizon_protocol::connection::Pipe::pair();
    game.connect_local(pipe, "Steve").unwrap();
    let spawn = |dimension: &str| play::SpawnInfo {
        dimension_type: 0,
        dimension: dimension.into(),
        seed: 1,
        game_type: 0,
        previous_game_type: None,
        debug: false,
        flat: false,
        last_death: None,
        portal_cooldown: 0,
        sea_level: 63,
    };
    game.handle_packet(C::Login(play::Login {
        player_id: 7,
        hardcore: false,
        levels: vec!["minecraft:overworld".into()],
        max_players: 20,
        chunk_radius: 8,
        simulation_distance: 8,
        reduced_debug_info: false,
        show_death_screen: true,
        limited_crafting: false,
        spawn: spawn("minecraft:overworld"),
        online_mode: false,
        enforces_secure_chat: false,
    }))
    .unwrap();
    let position = |teleport| {
        C::PlayerPosition(play::PlayerPosition {
            teleport,
            position: [0.5, 65.0, 0.5],
            delta: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            relatives: 0,
        })
    };
    game.handle_packet(position(1)).unwrap();
    // A diamond chestplate (ArmorMaterials.DIAMOND: 8 defence, 2 toughness) and a
    // modifier a command or a datapack gave.
    game.armor[1] = Some(ItemStack::new("minecraft:diamond_chestplate", 1));
    game.apply_equipment();
    game.player
        .as_mut()
        .unwrap()
        .set_modifier("minecraft:armor", "test:bonus", 3.0, "add_value", true)
        .unwrap();
    let read = |game: &mut Game, name: &str| game.player.as_mut().unwrap().attribute(name).unwrap();
    assert_eq!(read(&mut game, "minecraft:armor"), 11.0);
    assert_eq!(read(&mut game, "minecraft:armor_toughness"), 2.0);
    // Through the portal: the Respawn keeps all data, the position makes the player.
    game.handle_packet(C::Respawn(play::Respawn {
        spawn: spawn("minecraft:the_end"),
        keep: 3,
    }))
    .unwrap();
    assert!(game.player.is_none());
    game.handle_packet(position(2)).unwrap();
    assert_eq!(read(&mut game, "minecraft:armor"), 11.0);
    assert_eq!(read(&mut game, "minecraft:armor_toughness"), 2.0);
    // Then a Nether portal, the server's attributes arriving before the position: the new
    // player holds them as vanilla's already does.
    game.handle_packet(C::Respawn(play::Respawn {
        spawn: spawn("minecraft:the_nether"),
        keep: 3,
    }))
    .unwrap();
    let armor = game.motion.attributes.id("minecraft:armor").unwrap();
    game.handle_packet(C::UpdateAttributes(play::UpdateAttributes {
        id: 7,
        values: vec![AttributeSnapshot {
            attribute: armor.registry_id(),
            base: 0.0,
            modifiers: vec![AttributeModifier {
                id: "minecraft:armor.chestplate".into(),
                amount: 8.0,
                operation: 0,
            }],
        }],
    }))
    .unwrap();
    game.handle_packet(position(3)).unwrap();
    assert_eq!(read(&mut game, "minecraft:armor"), 8.0);
    assert_eq!(read(&mut game, "minecraft:armor_toughness"), 2.0);
    // A death's Respawn keeps no modifiers: base values only.
    game.handle_packet(C::Respawn(play::Respawn {
        spawn: spawn("minecraft:overworld"),
        keep: 0,
    }))
    .unwrap();
    game.handle_packet(position(4)).unwrap();
    assert_eq!(read(&mut game, "minecraft:armor"), 0.0);
    assert_eq!(read(&mut game, "minecraft:armor_toughness"), 0.0);
}
