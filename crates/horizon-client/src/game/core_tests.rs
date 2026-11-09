//! Unit tests that lived in game.rs's inline `tests` module.
use super::*;
use horizon_client::container_screen::{Button, Clicks, Release, Target};
use horizon_client::keymapping::{KeyAction, SNEAK, SPRINT, ToggleKeys};

/// `ClipContext.Block.OUTLINE` for the SOURCE_ONLY ray (#8180): a ray
/// through the empty part of a torch's cell is not stopped by it.
#[test]
fn the_fluid_ray_passes_through_the_hollow_of_a_torch() {
    // A torch: a 2x10x2 pillar in the middle of its cell, at (0, 0, 0).
    let torch = || vec![[0.4375, 0.0, 0.4375, 0.5625, 0.625, 0.5625]];
    let dir = [0.0, 0.0, 1.0];
    // Beside the pillar: no hit, the ray goes on to the water behind.
    assert!(outline_stop([0.1, 0.3, -1.0], dir, 5.0, [0, 0, 0], [0, 0, -1], torch).is_none());
    // Above the pillar (a torch is 10/16 tall): no hit either.
    assert!(outline_stop([0.5, 0.8, -1.0], dir, 5.0, [0, 0, 0], [0, 0, -1], torch).is_none());
    // Through the pillar: stopped on its front face, not at the cell's edge.
    let hit = outline_stop([0.5, 0.3, -1.0], dir, 5.0, [0, 0, 0], [0, 0, -1], torch)
        .expect("the pillar stops the ray");
    assert_eq!(hit.face, [0, 0, -1]);
    assert!((hit.point[2] - 0.4375).abs() < 1e-9);
}

#[test]
fn a_death_drops_the_off_hand_before_the_armour() {
    let stack = |item: &str| Some(ItemStack::new(item, 1));
    let mut inventory = vec![stack("minecraft:diamond"), None, stack("minecraft:stick")];
    let mut offhand = stack("minecraft:shield");
    let mut armor = [
        stack("minecraft:iron_helmet"),
        stack("minecraft:iron_chestplate"),
        stack("minecraft:iron_leggings"),
        stack("minecraft:iron_boots"),
    ];
    let dropped: Vec<String> = death_drops(&mut inventory, &mut offhand, &mut armor)
        .into_iter()
        .map(|s| s.item.to_string())
        .collect();
    assert_eq!(
        dropped,
        [
            "minecraft:diamond",
            "minecraft:stick",
            "minecraft:shield",
            "minecraft:iron_boots",
            "minecraft:iron_leggings",
            "minecraft:iron_chestplate",
            "minecraft:iron_helmet",
        ]
    );
    assert!(inventory.iter().all(Option::is_none));
    assert!(offhand.is_none() && armor.iter().all(Option::is_none));
}

/// `MinecraftServer.computeNextAutosaveInterval`: three hundred seconds of
/// ticks at the rate the server runs at, never fewer than a hundred.
#[test]
fn the_autosave_waits_for_three_hundred_seconds_of_ticks() {
    // 20 ticks a second is the six thousand ticks a world autosaved on
    // before the rate could be changed (`/tick rate`).
    assert_eq!(autosave_interval(20.0), 6000);
    assert_eq!(autosave_interval(10.0), 3000);
    assert_eq!(autosave_interval(40.0), 12000);
    assert_eq!(autosave_interval(1000.0), 300_000);
    // `Math.max(100, ...)`: a very low rate still autosaves every hundred
    // ticks, and a frozen clock (0) does too.
    assert_eq!(autosave_interval(0.5), 150);
    assert_eq!(autosave_interval(0.0), 100);
}

/// `Minecraft.<init>`: only an explicit `startedCleanly: false` is an
/// unclean start, and it leaves fullscreen; the packs stay selected.
#[test]
fn an_unclean_start_leaves_fullscreen_and_keeps_the_packs() {
    let fullscreen = || Settings {
        fullscreen: true,
        resource_packs: vec!["pack".into()],
        ..Default::default()
    };
    // The key absent (a first boot) is a clean start.
    let mut settings = fullscreen();
    assert!(!unclean_start_recovery(&mut settings));
    assert!(settings.fullscreen);
    let mut settings = fullscreen();
    settings.file.fullscreen_resolution = Some("1920x1080@60:24".into());
    settings.file.set(
        "startedCleanly",
        horizon_client::options_file::V::Bool(false),
    );
    assert!(unclean_start_recovery(&mut settings));
    assert!(!settings.fullscreen);
    assert_eq!(settings.file.fullscreen_resolution, None);
    assert_eq!(settings.resource_packs, ["pack"]);
    let mut settings = fullscreen();
    settings.file.set(
        "startedCleanly",
        horizon_client::options_file::V::Bool(true),
    );
    assert!(!unclean_start_recovery(&mut settings));
    assert!(settings.fullscreen);
}

/// `NativeScreenView.id` (the `screen_render_post` hook) is a mod's
/// contract: every `Screen` has its own id, and it does not change.
/// `Screen::id`'s match makes a new variant give itself one; this list
/// makes the test cover it too.
#[test]
fn screen_ids_are_distinct_and_stable() {
    use std::collections::BTreeSet;
    let all = [
        Screen::Playing,
        Screen::GameModeSwitcher,
        Screen::EmoteWheel,
        Screen::Paused,
        Screen::Options,
        Screen::Chat,
        Screen::Creative,
        Screen::Inventory,
        Screen::Dead,
        Screen::Stats,
        Screen::Advancements,
        Screen::WorldOptions,
        Screen::Friends,
        Screen::Social,
        Screen::CommandBlock,
        Screen::BlockEditor,
        Screen::ServerLinks,
        Screen::ConfirmLink,
        Screen::DialogWarning,
        Screen::Win,
        Screen::DebugOptions,
        Screen::Mod,
        Screen::HudEditor,
        Screen::Studio,
    ];
    let ids: Vec<&'static str> = all.iter().map(|screen| screen.id()).collect();
    let unique: BTreeSet<&'static str> = ids.iter().copied().collect();
    assert_eq!(unique.len(), all.len(), "two screens share an id");
    for id in &ids {
        assert!(!id.is_empty(), "an empty screen id");
        assert!(
            id.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{id} is not a snake_case id"
        );
    }
    // The ones a mod can match on in the hook's docs.
    assert_eq!(Screen::Playing.id(), "playing");
    assert_eq!(Screen::Inventory.id(), "inventory");
    assert_eq!(Screen::Win.id(), "win");
    assert_eq!(Screen::Studio.id(), "studio");
    assert_ne!(Screen::Studio.id(), Screen::Chat.id());
}

/// `Screen::Studio` is the Studio script slot: it never draws the chat screen
/// (`Screen::Studio => {}` in `game.rs`'s draw match, unlike
/// `Screen::Chat => draw_chat_screen`), it never blurs the background, and
/// while it is up gameplay does not own the keyboard (WASD cannot walk
/// through it, and movement keys are cleared on open).
#[test]
fn studio_screen_stays_off_the_chat_slot() {
    assert!(Screen::Studio != Screen::Chat);
    assert_eq!(Screen::Studio.id(), "studio");
    assert!(!Screen::Studio.blurs_background(false));
    assert!(!Screen::Studio.blurs_background(true));
    assert!(!gameplay_has_keyboard(Screen::Studio, false, false));
    // The pure gate behind `Game::open_studio_screen`, as
    // `native_mod_screen_open`'s `mod_screen_opens` is gated.
    use super::studio::studio_screen_opens;
    assert_eq!(studio_screen_opens(Screen::Playing, false), None);
    assert_eq!(studio_screen_opens(Screen::Playing, true), Some(false));
    assert_eq!(studio_screen_opens(Screen::Chat, false), Some(false));
    assert_eq!(studio_screen_opens(Screen::Studio, false), Some(true));
    assert_eq!(studio_screen_opens(Screen::Studio, true), Some(true));
}

/// The `modifiers` a native mod's input hooks hand over are GLFW's bits
/// (`GLFW_MOD_SHIFT` 1, `GLFW_MOD_CONTROL` 2, `GLFW_MOD_ALT` 4,
/// `GLFW_MOD_SUPER` 8), each on its own and all together.
#[test]
fn glfw_modifier_bits_are_glfw_modifier_bits() {
    use winit::keyboard::ModifiersState;
    for (state, bits) in [
        (ModifiersState::empty(), 0),
        (ModifiersState::SHIFT, 1),
        (ModifiersState::CONTROL, 2),
        (ModifiersState::ALT, 4),
        (ModifiersState::SUPER, 8),
        (ModifiersState::SHIFT | ModifiersState::ALT, 5),
        (ModifiersState::CONTROL | ModifiersState::SUPER, 10),
        (ModifiersState::SHIFT | ModifiersState::CONTROL, 3),
        (
            ModifiersState::SHIFT
                | ModifiersState::CONTROL
                | ModifiersState::ALT
                | ModifiersState::SUPER,
            15,
        ),
    ] {
        assert_eq!(glfw_modifiers(state), bits, "{state:?}");
    }
    // Every modifier GLFW has a bit for, and no other.
    assert_eq!(glfw_modifiers(ModifiersState::all()), 15);
}

/// The `button` a native mod's mouse hooks hand over is GLFW's index (0
/// left, 1 right, 2 middle, 3 back, 4 forward, else the button's number),
/// the one `keybinds::mouse_name` names (`key.mouse.<index + 1>` from the
/// back button on).
#[test]
fn glfw_button_indices_are_glfw_indices() {
    for (button, index, name) in [
        (MouseButton::Left, 0, "key.mouse.left"),
        (MouseButton::Right, 1, "key.mouse.right"),
        (MouseButton::Middle, 2, "key.mouse.middle"),
        (MouseButton::Back, 3, "key.mouse.4"),
        (MouseButton::Forward, 4, "key.mouse.5"),
    ] {
        assert_eq!(glfw_button(button), index);
        assert_eq!(horizon_client::keybinds::mouse_name(button), name);
    }
    for n in 5..12u16 {
        assert_eq!(glfw_button(MouseButton::Other(n)), i32::from(n));
        assert_eq!(
            horizon_client::keybinds::mouse_name(MouseButton::Other(n)),
            format!("key.mouse.{}", n + 1)
        );
    }
}

/// F3 left on when leaving a world kept the axes on the title screen.
#[test]
fn the_debug_crosshair_is_gone_once_the_world_is() {
    assert!(debug_crosshair_drawn(true, true, false, false));
    assert!(!debug_crosshair_drawn(true, true, false, true));
}

/// Opening a world in front of a block left its outline over Loading terrain's panorama (#12872).
#[test]
fn the_block_outline_stays_off_the_panorama() {
    assert!(block_outline_drawn(false, false, true, false));
    assert!(!block_outline_drawn(false, false, true, true));
}

/// Snuffing a candle or a candle cake draws one smoke per
/// `getParticleOffsets`: by `candles` for a candle, a block up for a cake.
#[test]
fn a_snuffed_candle_and_candle_cake_smoke_at_vanillas_offsets() {
    assert_eq!(
        Game::candle_offsets("minecraft:candle", Some(1)),
        [[8u8, 8, 8]].as_slice()
    );
    assert_eq!(
        Game::candle_offsets("minecraft:red_candle", Some(3)),
        [[8u8, 5, 10], [6, 7, 8], [9, 8, 7]].as_slice()
    );
    assert_eq!(
        Game::candle_offsets("minecraft:candle_cake", None),
        [[8u8, 16, 8]].as_slice()
    );
}

/// Held items apply their main-hand and off-hand modifiers to the player (#4118, #4777).
#[test]
fn equipment_modifiers_follow_their_slot_group() {
    assert!(equipment_modifier_applies("mainhand", "mainhand"));
    assert!(equipment_modifier_applies("hand", "mainhand"));
    assert!(equipment_modifier_applies("any", "mainhand"));
    assert!(!equipment_modifier_applies("armor", "mainhand"));
    assert!(!equipment_modifier_applies("mainhand", "head"));
    assert!(equipment_modifier_applies("armor", "chest"));
    assert!(!equipment_modifier_applies("hand", "feet"));
    assert!(equipment_modifier_applies("offhand", "offhand"));
    assert!(equipment_modifier_applies("hand", "offhand"));
    assert!(equipment_modifier_applies("any", "offhand"));
    assert!(!equipment_modifier_applies("armor", "offhand"));
    assert!(!equipment_modifier_applies("mainhand", "offhand"));
    assert!(!equipment_modifier_applies("offhand", "mainhand"));
}

#[path = "leash_knot_uuid_tests.rs"]
mod leash_knot_uuid_tests;
/// A guest's interact packet names its entity and the server keeps it:
/// `ServerGamePacketListenerImpl.handleInteract` resolves
/// `packet.entityId()` once and `Player.interactOn` clicks that entity,
/// while a fresh pick would take the nearest one. Two cows stand one
/// behind the other; the packet names the far one.
#[test]
fn an_interact_packet_clicks_the_entity_it_names_not_the_nearest() {
    // (server id, distance from the guest) in the level's own order.
    let cows = [(17u32, 1.5f64), (23, 2.5)];
    assert_eq!(Game::named_entity(&cows, 23, |c| c.0), Some(1));
    // The near cow is reachable too, when that is the one named.
    assert_eq!(Game::named_entity(&cows, 17, |c| c.0), Some(0));
    // An id the level does not hold clicks nothing (Level.getEntity).
    assert_eq!(Game::named_entity(&cows, 31, |c| c.0), None);
}

/// The id path keeps the crosshair pick's filters, so a packet cannot name
/// a mob the pick would refuse: one in its death animation, the one the
/// player rides, or the camera entity. Without them a guest could shear,
/// feed or leash a dying mob that `mob_hit` never returns.
#[test]
fn an_interact_packet_cannot_name_a_mob_the_pick_would_refuse() {
    use super::creatures::named_mob_pickable;
    assert!(named_mob_pickable(true, 17, None, None));
    // Dead: `Mob.alive()` is health > 0.
    assert!(!named_mob_pickable(false, 17, None, None));
    // Ridden, and the camera entity (`remote_camera_id`).
    assert!(!named_mob_pickable(true, 17, Some(17), None));
    assert!(!named_mob_pickable(true, 17, None, Some(17)));
    // Another mob being ridden or watched does not block this one.
    assert!(named_mob_pickable(true, 17, Some(23), Some(29)));
}

/// The packet's location is `EntityHitResult.getLocation() -
/// entity.position()`, the point the view ray met the box at measured
/// from the mob's feet (`MultiPlayerGameMode.interact`). The old fixed
/// [0, 1, 0] was a point inside every mob, which the server's
/// `interactAt` paths (armour stands, saddles, leashes) read as the click.
#[test]
fn an_interact_packet_carries_the_point_the_ray_hit() {
    let feet = Vec3::new(0.5, 0.0, 2.0);
    let eye = [0.5, 1.5, 1.0];
    let dir = [0.0, 0.0, 1.0];
    let box_at = Aabb::new(feet, Vec3::new(1.5, 2.0, 3.0));
    let t = ray_box(eye, dir, 3.0, box_at).unwrap();
    let point = Vec3::new(
        eye[0] + dir[0] * t,
        eye[1] + dir[1] * t,
        eye[2] + dir[2] * t,
    );
    // The ray enters the box's near face halfway up it, directly above
    // the feet: not the old centre point of the entity.
    assert_eq!(interact_location(point, feet), [0.0, 1.5, 0.0]);
}

/// A mod screen on a hopper or dispenser menu owns its slots: the stock
/// grid stands back so `slot_position` reaches the mod's own slots
/// (`native_screen_slot`) and its `inventory_y` (#5141). Without a mod
/// screen the stock layout answers exactly as before, which is what keeps
/// this invisible to players who run no native mods.
#[test]
fn a_mod_screen_on_a_machine_menu_takes_the_stock_layout_out() {
    let stock = Some((8.0, 109.0));
    // A mod screen is open: the stock grid does not answer.
    assert_eq!(Game::stock_machine_slot(stock, true), None);
    // No mod screen: the hopper and dispenser layouts are untouched.
    assert_eq!(Game::stock_machine_slot(stock, false), stock);
    // Other menus have no stock layout to stand back from.
    assert_eq!(Game::stock_machine_slot(None, true), None);
    assert_eq!(Game::stock_machine_slot(None, false), None);
}

/// A key bound to key.attack keeps attacking while it is held: each
/// repeat clicks like the press (#3243).
#[test]
fn a_held_key_bound_to_a_button_repeats_its_clicks() {
    assert_eq!(keyboard_button(true, KeyAction::Press), Some(true));
    assert_eq!(keyboard_button(true, KeyAction::Repeat), Some(true));
    assert_eq!(keyboard_button(true, KeyAction::Release), Some(false));
    for action in [KeyAction::Press, KeyAction::Repeat, KeyAction::Release] {
        assert_eq!(keyboard_button(false, action), None);
    }
}

/// A server dialog, a book, a sign editor or a prompt over the world is a
/// screen: held keys and the mouse buttons are released
/// (`KeyMapping.releaseAll`), and toggled sneak comes back when it closes
/// (#4974, #9729).
#[test]
fn a_dialog_releases_held_keys_like_a_screen() {
    assert!(gameplay_has_keyboard(Screen::Playing, false, false));
    assert!(!gameplay_has_keyboard(Screen::Playing, false, true));
    assert!(!gameplay_has_keyboard(Screen::Playing, true, false));
    assert!(!gameplay_has_keyboard(Screen::Chat, false, false));

    let mut keys = horizon_client::held::HeldKeys::from_iter([KeyCode::KeyW]);
    let mut left = true;
    let mut right = true;
    drop_held_behind_dialog(false, &mut keys, &mut left, &mut right);
    assert!(keys.raw(&KeyCode::KeyW) && left && right);
    drop_held_behind_dialog(true, &mut keys, &mut left, &mut right);
    assert!(keys.is_empty() && !left && !right);

    let options = [true, false, false, false];
    let mut toggles = ToggleKeys::default();
    toggles.key(SNEAK, KeyAction::Press, false, true);
    toggles.key(SNEAK, KeyAction::Release, false, true);
    assert!(toggles.is_down(SNEAK));
    toggles.screen_opened(options);
    assert!(!toggles.is_down(SNEAK));
    toggles.screen_closed(options);
    assert!(toggles.is_down(SNEAK));
}
/// A key bound to key.pickItem clones over a container screen, as the
/// middle button does; a key bound to attack or use does nothing there (#5378).
#[test]
fn a_key_bound_to_pick_block_clones_behind_a_container_screen() {
    assert!(container_key_clones(MouseButton::Middle, KeyAction::Press));
    assert!(container_key_clones(MouseButton::Middle, KeyAction::Repeat));
    assert!(!container_key_clones(
        MouseButton::Middle,
        KeyAction::Release
    ));
    for button in [MouseButton::Left, MouseButton::Right] {
        assert!(!container_key_clones(button, KeyAction::Press));
    }
}
#[test]
fn sleep_fade_lasts_ten_ticks_after_waking() {
    let alpha = |timer| match hud_overlays::sleep_overlay(320, 240, timer).first() {
        Some(hud_layout::GuiDraw::Fill { color, .. }) => color >> 24,
        None => 0,
        _ => panic!("sleep overlay must be a full-screen fill"),
    };
    assert_eq!(tick_sleep_timer(0, false), 0);
    let mut timer = 0;
    for _ in 0..50 {
        timer = tick_sleep_timer(timer, true);
    }
    assert_eq!(alpha(timer as i32), 110);
    for _ in 0..100 {
        timer = tick_sleep_timer(timer, true);
    }
    assert_eq!(timer, 100);
    assert_eq!(alpha(timer as i32), 220);
    for _ in 0..5 {
        timer = tick_sleep_timer(timer, false);
    }
    assert_eq!(alpha(timer as i32), 110);
    for _ in 0..5 {
        timer = tick_sleep_timer(timer, false);
    }
    assert_eq!(timer, 0);
    assert!(hud_overlays::sleep_overlay(320, 240, timer as i32).is_empty());
}

#[test]
fn world_rest_poll_does_not_wait_for_a_blocked_worker() {
    let (release, wait) = std::sync::mpsc::channel();
    let loader = WorldRestLoader {
        progress: Progress::new(),
        handle: std::thread::spawn(move || {
            wait.recv().unwrap();
            Ok(())
        }),
    };
    assert!(!loader.ready().unwrap());
    release.send(()).unwrap();
    loader.finish().unwrap();
}

#[test]
fn world_rest_poll_waits_for_a_slow_worker() {
    let (release, wait) = std::sync::mpsc::channel();
    let loader = WorldRestLoader {
        progress: Progress::new(),
        handle: std::thread::spawn(move || {
            wait.recv().unwrap();
            Ok(())
        }),
    };
    // However long it takes, a worker still loading is not an error.
    assert!(!loader.ready().unwrap());
    release.send(()).unwrap();
    while !loader.ready().unwrap() {
        std::thread::sleep(Duration::from_millis(1));
    }
    loader.finish().unwrap();
}

#[test]
fn world_rest_worker_failure_reaches_the_client() {
    let loader = WorldRestLoader {
        progress: Progress::new(),
        handle: std::thread::spawn(|| Err("missing world table".to_owned())),
    };
    assert_eq!(
        loader.finish().unwrap_err().to_string(),
        "missing world table"
    );
}

#[test]
fn world_rest_worker_panic_reaches_the_client() {
    let loader = WorldRestLoader {
        progress: Progress::new(),
        handle: std::thread::spawn(|| panic!("broken loader")),
    };
    assert_eq!(
        loader.finish().unwrap_err().to_string(),
        "world resource loader panicked"
    );
}

/// An integrated server thread shares the loaded resources with the client.
#[test]
fn shared_resources_can_cross_threads() {
    fn shared<T: Send + Sync>() {}
    shared::<Resources>();
    shared::<PackResources>();
}

#[test]
fn selecting_creative_tabs_does_not_drop_the_carried_stack_on_release() {
    let mut creative = Creative::from_json(&serde_json::json!({
        "schema": 2,
        "tabs": [
            {"type": "CATEGORY", "items": ["{count:1,id:\"minecraft:stone\"}"]},
            {"type": "INVENTORY"}
        ]
    }))
    .unwrap();
    for button in [Button::Left, Button::Right] {
        let mut clicks = Clicks::default();
        for tab in [1, 1, 0, 1] {
            select_creative_tab(&mut creative, &mut clicks, tab);
            assert_eq!(creative.selected, tab);
            assert_eq!(creative.tab().inventory, tab == 1);
            assert_eq!(
                clicks.release(
                    &Target::<usize>::Outside,
                    button,
                    false,
                    false,
                    false,
                    false
                ),
                Release::Nothing
            );
        }
    }
}

/// Eyes under water: the ray hits the source the eyes are in, at the eyes
/// (#3787), whichever way it looks.
#[test]
fn a_ray_starting_inside_a_source_hits_it_at_the_start() {
    let eye = [0.5, 65.62, 0.5];
    let down = [0.0, -1.0, 0.0];
    assert_eq!(
        clip_start_source(eye, down, 4.5, [0, 65, 0], true),
        Some(([0, 1, 0], eye))
    );
    // Without water above, 0.62 is still below the surface (8/9).
    assert_eq!(
        clip_start_source(eye, [0.0, 1.0, 0.0], 4.5, [0, 65, 0], false),
        Some(([0, -1, 0], eye))
    );
    // Looking ahead, slightly down: SOUTH's dot product is largest.
    let ahead = [0.0, -0.2f64, 0.98f64.sqrt()];
    assert_eq!(
        clip_start_source(eye, ahead, 4.5, [0, 65, 0], true),
        Some(([0, 0, -1], eye))
    );
}

/// Eyes above a surface source's 8/9: a ray going down enters it through
/// the top inside the block; level or rising rays miss it.
#[test]
fn a_ray_starting_above_a_source_surface_enters_through_the_top() {
    let eye = [0.5, 65.95, 0.5];
    let hit = clip_start_source(eye, [0.0, -1.0, 0.0], 4.5, [0, 65, 0], false);
    let (face, point) = hit.unwrap();
    assert_eq!(face, [0, 1, 0]);
    assert!((point[1] - (65.0 + f64::from(8.0f32 / 9.0f32))).abs() < 1e-12);
    assert_eq!(
        clip_start_source(eye, [1.0, 0.0, 0.0], 4.5, [0, 65, 0], false),
        None
    );
    // Leaves the block sideways before it comes down to the surface.
    let shallow = [0.995f64.sqrt(), -0.0707, 0.0];
    assert_eq!(
        clip_start_source(eye, shallow, 4.5, [0, 65, 0], false),
        None
    );
}

#[test]
fn stack_drop_requires_ctrl_even_with_sprint_latched() {
    for sprint in [false, true] {
        let mut toggles = ToggleKeys::default();
        if sprint {
            toggles.key(SPRINT, KeyAction::Press, false, true);
            toggles.key(SPRINT, KeyAction::Release, false, true);
        }
        assert_eq!(toggles.is_down(SPRINT), sprint);

        for held in 0..4 {
            let mut modifiers = winit::keyboard::ModifiersState::empty();
            let mut physical_keys = HashSet::from([KeyCode::ShiftLeft, KeyCode::KeyQ]);
            let sides = [KeyCode::ControlLeft, KeyCode::ControlRight];
            for (bit, key) in sides.into_iter().enumerate() {
                if held & (1 << bit) != 0 {
                    horizon_client_input::modifiers::set_key(
                        &mut modifiers,
                        &mut physical_keys,
                        key,
                        true,
                    );
                }
            }
            assert_eq!(drop_whole_stack(modifiers), held != 0);
            for key in sides {
                horizon_client_input::modifiers::set_key(
                    &mut modifiers,
                    &mut physical_keys,
                    key,
                    false,
                );
            }
            assert!(!drop_whole_stack(modifiers));
            assert_eq!(toggles.is_down(SPRINT), sprint);
        }
    }
}
