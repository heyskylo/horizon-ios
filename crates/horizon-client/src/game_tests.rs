#[test]
fn weather_follows_the_dimension_type_not_the_id() {
    use super::level_can_have_weather as w;
    assert!(w(true, false, false)); // open sky
    assert!(!w(true, true, false)); // ceiling (Nether, overworld_caves)
    assert!(!w(false, false, false)); // no sky light
    assert!(!w(true, false, true)); // the End is named by id
}

use super::*;

/// A falling block is drawn moved by its column's model offset, as
/// `ModelBlockRenderer.tesselateBlock` does with `BlockState.getOffset`
/// of `BlockPos.containing(x, maxY, z)`: a pointed dripstone that breaks
/// off stays where it hung instead of jumping to the block's centre (#4102).
#[test]
fn a_falling_block_keeps_its_model_offset() {
    let at = Vec3::new(3.5, 70.25, -1.5);
    let falling = FallingBlock {
        rotation: [0.0, 0.0],
        join: Default::default(),
        id: 0,
        state: BlockStateId(7),
        start_pos: BlockPos::new(3, 72, -2),
        position: at,
        previous: Vec3::new(3.5, 71.0, -1.5),
        velocity: Vec3::ZERO,
        age: 5,
        no_gravity: false,
        portal: Default::default(),
        drop_item: true,
        fall_distance: 0.0,
        on_ground: false,
        stuck: Vec3::ZERO,
        damage: horizon_client::falling_block::Damage::default(),
        saved: None,
    };
    // OffsetType.XZ: one offset per column (Mth.getSeed(x, 0, z)).
    let offset = |state: BlockStateId, x: i32, z: i32| {
        assert_eq!(state, BlockStateId(7));
        ((x, z) == (3, -2)).then_some([0.125, 0.0, -0.0625])
    };
    let origin = falling.model_origin(0.5, offset);
    assert_eq!(origin, glam::DVec3::new(3.125, 70.625, -2.0625));
    // Blocks without a random offset are drawn centred on the entity.
    let origin = falling.model_origin(1.0, |_, _, _| None);
    assert_eq!(origin, glam::DVec3::new(3.0, 70.25, -2.0));
}

/// `FallingBlockRenderer.shouldRender` emits nothing for a falling block whose
/// own state is the state of the cell it is in (a `NoGravity` block, or one
/// that has just landed), where Horizon drew a z-fighting copy; the same block
/// draws its model again once that cell is cleared.
#[test]
fn a_falling_block_in_its_own_state_emits_nothing() {
    let state = BlockStateId(7);
    assert!(!falling_block_visible(state, state));
    assert!(falling_block_visible(state, BlockStateId(0)));
}

/// A falling block does not sneak, so `Entity.isDescending()` is false.
/// Scaffolding top surface collides solidly with non-descending entities
/// so falling scaffolding lands on top of existing scaffolding instead
/// of falling through and breaking (#7553).
#[test]
fn falling_block_does_not_descend_through_scaffolding() {
    let pos = BlockPos::new(0, 10, 0);
    let scaffold = horizon_world::block_collision::ScaffoldingCollision::from_state(
        &horizon_world::chunk::BlockState {
            name: "minecraft:scaffolding".to_string(),
            properties: [
                ("bottom".to_string(), "false".to_string()),
                ("distance".to_string(), "0".to_string()),
                ("waterlogged".to_string(), "false".to_string()),
            ]
            .into_iter()
            .collect(),
        },
    )
    .unwrap()
    .expect("scaffolding collision rule");

    // When a falling block is falling downward (wanted.y < 0) above the scaffolding top (y = 11.0):
    let falling_y = 11.0;
    let buggy_descending_context = CollisionContext::for_entity(
        CollisionEntity {
            identity: EntityIdentity(0),
            kind: horizon_world::collision_context::EntityContextKind::Other,
            y: falling_y,
            descending: true, // the old buggy behavior: wanted.y < 0.0
            main_hand: None,
        },
        false,
    );
    let fixed_context = CollisionContext::for_entity(
        CollisionEntity {
            identity: EntityIdentity(0),
            kind: horizon_world::collision_context::EntityContextKind::Other,
            y: falling_y,
            descending: false, // fixed: FallingBlockEntity does not sneak
            main_hand: None,
        },
        false,
    );

    // Before the fix, descending=true caused scaffolding collision to be empty:
    assert!(buggy_descending_context.is_descending());
    let buggy_shape = scaffold.shape(&buggy_descending_context, pos);
    assert!(
        buggy_shape.is_empty(),
        "descending entities fall through scaffolding"
    );

    // With descending=false, scaffolding provides stable top collision:
    assert!(!fixed_context.is_descending());
    let solid_shape = scaffold.shape(&fixed_context, pos);
    assert!(
        !solid_shape.is_empty(),
        "falling block must collide with scaffolding top"
    );
}

#[test]
fn quit_shortcut_blocks_window_close_when_option_disabled() {
    use winit::keyboard::ModifiersState;

    let mut keys = HashSet::new();
    let alt = ModifiersState::ALT;
    let super_key = ModifiersState::SUPER;

    let quit_shortcuts_enabled = true;
    let quit_shortcuts_disabled = false;

    keys.insert(if cfg!(target_os = "macos") {
        KeyCode::KeyQ
    } else {
        KeyCode::F4
    });
    let mods = if cfg!(target_os = "macos") {
        super_key
    } else {
        alt
    };

    let should_ignore =
        |quit_shortcuts: bool, modifiers: ModifiersState, keys: &HashSet<KeyCode>| {
            !quit_shortcuts && horizon_client::input::is_quit_shortcut_down(modifiers, keys)
        };

    // When enabled, close requests from shortcuts proceed normally.
    assert!(!should_ignore(quit_shortcuts_enabled, mods, &keys));

    // When disabled and the quit shortcut is held, close requests are ignored.
    assert!(should_ignore(quit_shortcuts_disabled, mods, &keys));

    // When disabled but shortcut NOT held (e.g. 'X' button clicked), close request proceeds.
    assert!(!should_ignore(
        quit_shortcuts_disabled,
        ModifiersState::empty(),
        &HashSet::new()
    ));
}

#[test]
fn eager_saves_keep_dirty_order_cooldowns_and_writer_budget() {
    use horizon_client::world::ModifiedChunks;
    use std::cell::Cell;
    let now = Instant::now();
    let mut dirty = ModifiedChunks::default();
    dirty.extend((0..27).map(|x| (x, 0)));
    dirty.autosaved(&[(0, 0), (2, 0)], now);
    dirty.remove(&(1, 0)); // already clean
    let writes = Cell::new(127);
    let mut chosen = Vec::new();
    dirty.save_eagerly(
        now,
        || writes.get() < 128,
        |k| k != (3, 0),
        |k| {
            chosen.push(k);
            writes.set(writes.get() + 1);
            true
        },
    );
    assert_eq!(chosen, [(4, 0)]); // cooldown, clean, absent, then budget exhausted
    chosen.clear();
    dirty.save_eagerly(
        now,
        || true,
        |_| true,
        |k| {
            chosen.push(k);
            true
        },
    );
    assert_eq!(chosen, (5..25).map(|x| (x, 0)).collect::<Vec<_>>()); // exactly 20
    dirty.insert((4, 0)); // a later edit goes AFTER the still-cooled entries
    chosen.clear();
    dirty.save_eagerly(
        now + Duration::from_secs(10),
        || true,
        |_| true,
        |k| {
            chosen.push(k);
            true
        },
    );
    assert_eq!(chosen, [(0, 0), (2, 0), (25, 0), (26, 0), (4, 0)]);
    dirty.insert((4, 0));
    dirty.save_eagerly(
        now + Duration::from_secs(11),
        || false,
        |_| true,
        |_| panic!("no time"),
    );
    assert!(dirty.contains(&(4, 0)));
    dirty.unloaded(&(4, 0)); // unloading removes the cooldown even if reloaded immediately
    chosen.clear();
    dirty.save_eagerly(
        now + Duration::from_secs(11),
        || true,
        |_| true,
        |k| {
            chosen.push(k);
            true
        },
    );
    assert_eq!(chosen, [(4, 0)]);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn eager_saves_wait_for_an_open_world_and_for_light() {
    let mut game = super::native_machines::tests::server_game();
    let dir = std::env::temp_dir().join(format!("horizon-eager-game-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    game.save = Arc::new(horizon_client::save::SaveDir::new(
        dir.clone(),
        horizon_client::save::LevelShape::OVERWORLD,
    ));
    Arc::make_mut(game.world.chunks.get_mut(&(0, 0)).unwrap()).lit = true;
    game.world
        .chunks
        .insert((1, 0), Arc::new(horizon_client::chunk::Chunk::empty(1, 0)));
    game.world.modified.extend([(0, 0), (1, 0)]);
    game.save_chunks_eagerly(); // no world open: nothing to save into
    assert!(game.world.modified.contains(&(0, 0)));
    game.world_root = dir.clone();
    game.save_chunks_eagerly();
    assert!(!game.world.modified.contains(&(0, 0)));
    assert!(game.world.modified.contains(&(1, 0))); // not lit yet
    Arc::make_mut(game.world.chunks.get_mut(&(1, 0)).unwrap()).lit = true;
    game.save_chunks_eagerly(); // still queued: no new dirty transition needed
    assert!(!game.world.modified.contains(&(1, 0)));
    game.save.flush().unwrap();
    assert!(dir.join("region/r.0.0.mca").exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn integrated_server_save_persists_owner_player_data() {
    let dir = std::env::temp_dir().join(format!(
        "horizon-integrated-player-save-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);

    // Initial world creation writes `level.dat` before the player spawns.
    let mut level = Level {
        name: "Creative".into(),
        creative: true,
        game_mode: 1,
        ..Level::default()
    };
    level.write(&dir).unwrap();

    // The integrated server stores the owner's state in `level` and saves the world.
    level.position = Some([128.5, 72.0, -64.25]);
    level.inventory = vec![(0, "minecraft:diamond_pickaxe".into(), 1, 0)];
    write_level_for_role(
        &mut level,
        &dir,
        execution_role::ExecutionRole::IntegratedServer,
    )
    .unwrap();

    let reopened = Level::read(&dir).unwrap();
    assert_eq!(reopened.position, Some([128.5, 72.0, -64.25]));
    assert_eq!(
        reopened.inventory,
        vec![(0, "minecraft:diamond_pickaxe".into(), 1, 0)]
    );

    // A dedicated server save updates world metadata without overwriting the player file.
    let mut dedicated = Level {
        player_uuid: level.player_uuid,
        day_time: 6000,
        ..Level::default()
    };
    write_level_for_role(
        &mut dedicated,
        &dir,
        execution_role::ExecutionRole::DedicatedServer,
    )
    .unwrap();

    let reopened = Level::read(&dir).unwrap();
    assert_eq!(reopened.day_time, 6000);
    assert_eq!(reopened.position, Some([128.5, 72.0, -64.25]));
    assert_eq!(
        reopened.inventory,
        vec![(0, "minecraft:diamond_pickaxe".into(), 1, 0)]
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `Hud.extractRenderState`: the subtitles follow the HUD when no screen
/// is over the game, are deferred over a screen that keeps the game
/// behind it (so F1 does not take them away), and vanish under F1 alone.
#[test]
fn the_subtitles_follow_the_hud_unless_a_screen_keeps_them() {
    use super::{Screen, Subtitles, in_game_ui, subtitles};
    // A container screen defers them above itself, hidden HUD or not.
    assert_eq!(subtitles(true, true), Subtitles::Deferred);
    assert_eq!(subtitles(false, true), Subtitles::Deferred);
    // No such screen: in the HUD's pass, and gone under F1.
    assert_eq!(subtitles(true, false), Subtitles::Now);
    assert_eq!(subtitles(false, false), Subtitles::Hidden);
    // `Screen.isInGameUi` is `AbstractContainerScreen` and the block
    // editors' answer; a pause menu, the chat and the death screen say no.
    assert!(in_game_ui(Screen::Inventory, false));
    assert!(in_game_ui(Screen::Creative, false));
    assert!(in_game_ui(Screen::CommandBlock, false));
    assert!(in_game_ui(Screen::BlockEditor, false));
    // A book and a sign editor are in-game UI drawn over `Playing`.
    assert!(in_game_ui(Screen::Playing, true));
    assert!(!in_game_ui(Screen::Paused, false));
    assert!(!in_game_ui(Screen::Chat, false));
    assert!(!in_game_ui(Screen::Dead, false));
    assert!(!in_game_ui(Screen::Playing, false));
}
