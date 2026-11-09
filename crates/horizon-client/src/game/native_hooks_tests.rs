#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn held_durability_retains_the_stack_hand_and_explicit_break_slot() {
    use crate::game::{Audio, Game, Hand, Options, Progress, Settings, load_resources};
    use horizon_client::{items::ItemStack, motion::PlayerMotion};
    use horizon_core::geometry::Vec3;
    use horizon_world::player_inventory::EquipmentSlot;
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
    options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
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
    game.player = Some(PlayerMotion::new(game.motion, Vec3::new(0.0, 70.0, 0.0)).unwrap());
    game.set_mode_id(0);
    // Stage an acting guest: break events are queued instead of rendering local crumbs.
    game.lan_acting = Some(1);
    game.hand = Hand::Off;
    let main = ItemStack::new("minecraft:stick", 3);
    game.inventory[game.selected] = Some(main.clone());
    let max = game
        .items()
        .get("minecraft:flint_and_steel")
        .unwrap()
        .max_damage;
    game.offhand = Some(ItemStack::new("minecraft:flint_and_steel", 1));
    game.damage_held_in_slot(1, EquipmentSlot::OffHand);
    assert_eq!(game.offhand.as_ref().unwrap().damage, 1);
    assert!(game.guest_equipment_breaks.is_empty());
    for (explicit, event) in [(true, 48), (false, 47)] {
        let mut tool = ItemStack::new("minecraft:flint_and_steel", 1);
        tool.damage = max - 1;
        game.offhand = Some(tool);
        if explicit {
            game.damage_held_in_slot(1, EquipmentSlot::OffHand);
        } else {
            game.damage_held(1);
        }
        assert!(game.offhand.is_none());
        assert_eq!(game.inventory[game.selected], Some(main.clone()));
        assert_eq!(game.guest_equipment_breaks.last().unwrap().0, event);
    }
    game.set_mode_id(1);
    game.offhand = Some(ItemStack::new("minecraft:flint_and_steel", 1));
    game.damage_held_in_slot(1, EquipmentSlot::OffHand);
    assert_eq!(game.offhand.as_ref().unwrap().damage, 0);
}

#[test]
fn hand_pose_rejects_invalid_components() {
    let identity = super::hand_pose_matrix([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
    assert_eq!(identity, glam::Mat4::IDENTITY);
    assert_eq!(
        super::hand_pose_matrix([
            f32::NAN,
            17.0,
            f32::INFINITY,
            3601.0,
            -3601.0,
            f32::NAN,
            0.0
        ]),
        identity
    );
    let point = super::hand_pose_matrix([1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 2.0])
        .transform_point3(glam::Vec3::X);
    assert_eq!(point, glam::Vec3::new(3.0, 2.0, 3.0));
}
#[test]
fn observed_overlay_prefers_first_open_screen() {
    assert_eq!(
        super::screen_observer::observed_overlay_id(&[(false, "sign"), (false, "book")]),
        None
    );
    assert_eq!(
        super::screen_observer::observed_overlay_id(&[(false, "sign"), (true, "book")]),
        Some("book")
    );
    assert_eq!(
        super::screen_observer::observed_overlay_id(&[(true, "sign"), (true, "book")]),
        Some("sign")
    );
}

/// Exercise the gameplay constructor, where name-based entities compete
/// with a mod block's sign template.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn mod_sign_constructor_prioritizes_templates_over_suffixes() {
    use super::super::{Audio, Game, Options, Progress, Settings, load_resources};
    use horizon_client::mod_content::{ContentMod, build};
    use horizon_mods::content::{Block, Content};

    let base = std::path::PathBuf::from(
        std::env::var_os("HORIZON_TEST_RESOURCES")
            .expect("set HORIZON_TEST_RESOURCES to prepared resources"),
    );
    let root = std::env::temp_dir().join(format!(
        "horizon-sign-constructor-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let cases = [
        ("ruby:notice_banner", "minecraft:oak_sign", false),
        ("ruby:board_shelf", "minecraft:oak_wall_sign", false),
        ("ruby:lantern_head", "minecraft:oak_hanging_sign", true),
        (
            "ruby:bracket_skull",
            "minecraft:oak_wall_hanging_sign",
            true,
        ),
    ];
    let mut content = Content::declared("ruby");
    let blocks: Vec<_> = cases
        .iter()
        .map(|&(id, template, _)| (id, template))
        .chain([("ruby:road_sign", "minecraft:stone")])
        .map(|(id, template)| Block {
            id: id.into(),
            template: template.into(),
            item: false,
            ..Default::default()
        })
        .collect();
    content.declare(&blocks, &[]).unwrap();
    build(
        &base,
        &[ContentMod {
            id: "ruby".into(),
            content,
        }],
        &root,
    )
    .unwrap();
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
        &mut |_, _| unreachable!("no menu handoff"),
    )
    .unwrap();
    let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
    options.resources = root.clone();
    options.execution_role = super::ExecutionRole::IntegratedServer;
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

    for (name, hanging) in cases
        .map(|(name, _, hanging)| (name, hanging))
        .into_iter()
        .chain([
            ("minecraft:oak_sign", false),
            ("minecraft:oak_hanging_sign", true),
        ])
    {
        match game.new_block_entity(name) {
            Some(super::BlockEntity::Sign(sign)) => {
                assert_eq!(sign.hanging, hanging, "{name}")
            }
            entity => panic!("{name}: expected a sign, got {entity:?}"),
        }
    }
    assert!(game.new_block_entity("ruby:road_sign").is_none());
    assert!(matches!(
        game.new_block_entity("minecraft:white_banner"),
        Some(super::BlockEntity::Banner(_))
    ));
    let _ = std::fs::remove_dir_all(&root);
}

/// A mod's own screen opens only over the world, never over another
/// screen, an overlay (the title screen, a book, loading, no window) or
/// another mod's; opening it again is a no-op.
#[test]
fn a_mod_screen_opens_only_over_the_world() {
    use super::{Screen, mod_screen_opens};
    assert_eq!(mod_screen_opens(Screen::Playing, false, None, "a"), None);
    assert_eq!(
        mod_screen_opens(Screen::Playing, true, None, "a"),
        Some(false)
    );
    for other in [
        Screen::Paused,
        Screen::Inventory,
        Screen::Chat,
        Screen::Options,
    ] {
        assert_eq!(mod_screen_opens(other, false, None, "a"), Some(false));
    }
    assert_eq!(
        mod_screen_opens(Screen::Mod, false, Some("a"), "a"),
        Some(true)
    );
    assert_eq!(
        mod_screen_opens(Screen::Mod, false, Some("b"), "a"),
        Some(false)
    );
}

/// The characters a `ScreenEvent.CharacterTyped.Pre` sees are the
/// printable ones of a text: a cancelled one leaves that text and a
/// control character (a key, not text) is never offered a hook.
#[test]
fn typed_text_keeps_control_characters_and_drops_cancelled_ones() {
    let mut offered = Vec::new();
    let left = super::screen_text_hooked("a\u{7f}b\u{1b}c", |c| {
        offered.push(c);
        c == 'b'
    });
    assert_eq!(offered, ['a', 'b', 'c']);
    assert_eq!(left.as_deref(), Some("a\u{7f}\u{1b}c"));
    // Every printable character cancelled: no text is left to type.
    assert_eq!(super::screen_text_hooked("ab", |_| true), None);
    // Nothing to offer: an empty text stays empty and keys alone stay whole.
    assert_eq!(super::screen_text_hooked("", |_| true).as_deref(), Some(""));
    assert_eq!(
        super::screen_text_hooked("\u{7f}\t", |_| true).as_deref(),
        Some("\u{7f}\t")
    );
    // What no hook cancels is handed back as it came in.
    assert_eq!(
        super::screen_text_hooked("hello", |_| false).as_deref(),
        Some("hello")
    );
}

/// The window's key path: a key whose press a mod cancelled still types
/// the characters no mod cancelled (vanilla's separate `charTyped`), while
/// the key characters never reach the text box.
#[test]
fn a_cancelled_key_still_types_the_characters_left() {
    // `axb` with 'x' cancelled: the key went no further, `ab` still types.
    assert_eq!(
        super::chars_to_type("axb", |c| c == 'x').as_deref(),
        Some("ab")
    );
    // Backspace, Escape and the like are keys, not text.
    assert_eq!(
        super::chars_to_type("a\u{7f}\u{1b}b", |_| false).as_deref(),
        Some("ab")
    );
    // Every character cancelled: the box takes none of them.
    assert_eq!(super::chars_to_type("ab", |_| true), None);
    // Nothing typed: nothing to hand over.
    assert_eq!(super::chars_to_type("", |_| false), None);
}

/// A mod's camera replaces vanilla's value by value; one that isn't finite keeps vanilla's.
#[test]
fn a_mod_camera_replaces_each_finite_value() {
    use super::posed_camera;
    let vanilla = (glam::DVec3::new(1.0, 2.0, 3.0), 10.0, 20.0);
    assert_eq!(
        posed_camera(vanilla, [-40.5, 90.0, 1e7], 270.0, -35.0),
        (glam::DVec3::new(-40.5, 90.0, 1e7), 270.0, -35.0)
    );
    assert_eq!(
        posed_camera(
            vanilla,
            [f64::NAN, 5.0, f64::INFINITY],
            f32::NAN,
            f32::NEG_INFINITY
        ),
        (glam::DVec3::new(1.0, 5.0, 3.0), 10.0, 20.0)
    );
}

/// A shake is scaled by Distortion Effects, bounded on each axis, and a
/// value that isn't finite is none.
#[test]
fn a_shake_is_scaled_and_bounded() {
    use super::{NativeShake, bounded_shake, shake_pose};
    let shake = NativeShake {
        pitch: 4.0,
        yaw: -30.0,
        roll: f32::NAN,
        offset: [0.2, 2.0, f32::INFINITY],
    };
    assert_eq!(
        bounded_shake(shake, 1.0),
        NativeShake {
            pitch: 4.0,
            yaw: -10.0,
            roll: 0.0,
            offset: [0.2, 0.5, 0.0],
        }
    );
    assert_eq!(bounded_shake(shake, 0.5).pitch, 2.0);
    assert_eq!(bounded_shake(shake, 0.0), NativeShake::default());
    assert_eq!(shake_pose(NativeShake::default()), glam::Mat4::IDENTITY);
}

/// A mod's shader values that aren't finite read as 0.
#[test]
fn shader_params_that_arent_finite_read_as_zero() {
    let mut params = [[1.0; 4]; 8];
    params[0] = [f32::NAN, 2.0, f32::INFINITY, -3.0];
    let out = super::finite_params(params);
    assert_eq!(out[0], [0.0, 2.0, 0.0, -3.0]);
    assert_eq!(out[7], [1.0; 4]);
}

/// A client mod's sound needs a namespaced event, finite values and a
/// delay of a minute at most.
#[test]
fn a_local_sound_is_checked() {
    use super::local_sound_ok;
    let at = [0.5, 64.0, 0.5];
    assert!(local_sound_ok("ironfall:boom_far", at, 8.0, 1.0, 140));
    assert!(local_sound_ok(
        "minecraft:entity.generic.explode",
        at,
        1.0,
        1.0,
        0
    ));
    assert!(!local_sound_ok("boom", at, 1.0, 1.0, 0));
    assert!(!local_sound_ok("ironfall:", at, 1.0, 1.0, 0));
    assert!(!local_sound_ok("Ironfall:Boom", at, 1.0, 1.0, 0));
    assert!(!local_sound_ok(
        "ironfall:boom",
        [f64::NAN, 0.0, 0.0],
        1.0,
        1.0,
        0
    ));
    assert!(!local_sound_ok("ironfall:boom", at, -1.0, 1.0, 0));
    assert!(!local_sound_ok("ironfall:boom", at, 1.0, 1.0, 1201));
    assert!(!local_sound_ok("ironfall:boom", at, 1.0, 1.0, -1));
}

/// `native_play_sound` takes vanilla's and mods' sound ids, nothing else.
#[test]
fn sound_ids_are_identifiers() {
    for id in [
        "minecraft:block.note_block.harp",
        "mymod:machine/hum",
        "my_mod.x-1:a",
    ] {
        assert!(super::sound_id(id), "{id}");
    }
    for id in [
        "",
        "hum",
        ":hum",
        "mymod:",
        "MyMod:hum",
        "mymod:Hum",
        "my/mod:hum",
        "a:b c",
    ] {
        assert!(!super::sound_id(id), "{id}");
    }
}

/// A mod's property write: none for a key or value the block lacks, the
/// state itself when it already has the value, else the new state.
#[test]
fn with_property_finds_the_state() {
    let facts = br#"{"defaults":{"minecraft:air":0,"test:crop":1},"states":[
            ["air",true,false,"","minecraft:air"],
            ["crop0",false,false,"",{"id":"test:crop","properties":{"age":"0"}}],
            ["crop1",false,false,"",{"id":"test:crop","properties":{"age":"1"}}]
        ]}"#;
    let registry = horizon_world::registry::BlockRegistry::from_facts_json(&facts[..]).unwrap();
    use horizon_world::registry::BlockStateId;
    let (age0, age1) = (BlockStateId(1), BlockStateId(2));
    assert_eq!(
        super::with_property(&registry, age0, "age", "0"),
        Some(age0)
    );
    assert_eq!(
        super::with_property(&registry, age0, "age", "1"),
        Some(age1)
    );
    assert_eq!(super::with_property(&registry, age0, "age", "9"), None);
    assert_eq!(super::with_property(&registry, age0, "lit", "true"), None);
    // What both property setters write: nothing (false) when the
    // property already has the value, as `Level.setBlock`.
    assert_eq!(super::property_change(&registry, age0, "age", "0"), None);
    assert_eq!(
        super::property_change(&registry, age0, "age", "1"),
        Some(age1)
    );
    assert_eq!(super::property_change(&registry, age0, "age", "9"), None);
    assert_eq!(super::property_change(&registry, age0, "lit", "true"), None);
}

/// The state a mod block is placed in is built from its own default state
/// and one of its properties, its declared ones (`state_properties` in its
/// block file) included, and keeps the properties it already had.
#[test]
fn a_placement_state_is_built_from_a_blocks_own_properties() {
    let facts = br#"{"defaults":{"minecraft:air":0,"test:pipe":1},"states":[
            ["air",true,false,"","minecraft:air"],
            ["north_gas",false,false,"",{"id":"test:pipe","properties":{"carries":"gas","facing":"north"}}],
            ["east_gas",false,false,"",{"id":"test:pipe","properties":{"carries":"gas","facing":"east"}}],
            ["north_items",false,false,"",{"id":"test:pipe","properties":{"carries":"items","facing":"north"}}],
            ["east_items",false,false,"",{"id":"test:pipe","properties":{"carries":"items","facing":"east"}}]
        ]}"#;
    let registry = horizon_world::registry::BlockRegistry::from_facts_json(&facts[..]).unwrap();
    use horizon_world::registry::BlockStateId;
    let (north_gas, east_gas, east_items) = (BlockStateId(1), BlockStateId(2), BlockStateId(4));
    // The declared property changes, and `carries` stays as it was.
    assert_eq!(
        super::with_property(&registry, north_gas, "facing", "east"),
        Some(east_gas)
    );
    assert_eq!(
        super::with_property(&registry, east_gas, "carries", "items"),
        Some(east_items)
    );
    // A value the property does not have builds nothing.
    assert_eq!(
        super::with_property(&registry, east_gas, "facing", "up"),
        None
    );
}
/// Both property setters return false for a write that changes nothing:
/// each goes through `property_change`, none returns true on its own.
#[test]
fn property_setters_refuse_no_change() {
    // A Windows checkout has CRLF line ends: the searches below look for "\n".
    let source = include_str!("native_hooks.rs").replace("\r\n", "\n");
    let code = source.split("#[cfg(test)]").next().unwrap_or_default();
    for head in [
        "    pub(super) fn native_set_block_property(\n        &mut self,\n        ticks:",
        "    pub(super) fn native_set_block_property_with(",
    ] {
        let body = code.split(head).nth(1).expect(head);
        let body = &body[..body.find("\n    }\n").expect(head)];
        assert!(body.contains("property_change("), "{head}");
        assert!(!body.contains("return true"), "{head}");
    }
}
/// horizon-mixins refuses targets whose body has a `cfg` or a macro:
/// the hook points and helpers must stay free of both. The launcher
/// reads the API line as it is.
/// A data packet for an old window leaves the open mod screen alone.
#[test]
fn mod_screen_data_is_its_windows_only() {
    let mut s = super::OpenNativeScreen {
        key: (0, 0, 0),
        window: 4,
        block: "gtmod:machine".into(),
        screen: super::NativeScreen::default(),
        data: Vec::new(),
        extra: None,
    };
    assert!(s.set_data(4, 1, 7));
    assert!(!s.set_data(3, 1, 99));
    assert_eq!(s.data, [0, 7]);
}

#[test]
fn hook_points_stay_hookable() {
    let source = include_str!("native_hooks.rs");
    let code = source.split("#[cfg(test)]").next().unwrap_or_default();
    assert!(code.contains("\npub(crate) const NATIVE_HOOKS_API: u32 = "));
    let lines = code.lines().filter(|l| !l.trim_start().starts_with("//"));
    for line in lines {
        assert!(!line.contains("cfg"), "a cfg in the hook points: {line}");
        for mac in ["!(", "![", "! {", "!{"] {
            assert!(!line.contains(mac), "a macro in the hook points: {line}");
        }
    }
}

#[test]
fn native_breathing_stays_hookable() {
    let source = include_str!("../breathing.rs").replace("\r\n", "\n");
    let hook = source.find("pub fn native_living_breathe(").unwrap();
    // From its doc comment's first line, so an attribute above it counts.
    let start = source[..hook].find("/// An entity's breathing").unwrap();
    let body = &source[start..hook + source[hook..].find("\n}\n").unwrap()];
    assert!(body.contains("#[inline(never)]"));
    for line in body.lines() {
        assert!(!line.contains("cfg"), "a cfg in the breathing hook: {line}");
        for mac in ["!(", "![", "! {", "!{"] {
            assert!(!line.contains(mac), "a macro in the breathing hook: {line}");
        }
    }
}

#[test]
fn native_loot_kinds_stay_hookable() {
    let source = include_str!("../../../horizon-world/src/loot.rs").replace("\r\n", "\n");
    for hook in [
        "pub fn native_loot_function(",
        "pub fn native_loot_condition(",
    ] {
        let at = source
            .find(hook)
            .unwrap_or_else(|| panic!("{hook} not found"));
        let start = source[..at].rfind("///").unwrap();
        let body = &source[start..start + source[start..].find("\n}\n").unwrap()];
        for line in body.lines() {
            assert!(!line.contains("cfg"), "a cfg in a loot hook point: {line}");
            for mac in ["!(", "![", "! {", "!{"] {
                assert!(!line.contains(mac), "a macro in a loot hook point: {line}");
            }
        }
    }
}

#[test]
fn native_ingredient_stays_hookable() {
    let source = include_str!("../crafting.rs").replace("\r\n", "\n");
    let at = source.find("pub fn native_ingredient(").unwrap();
    let start = source[..at].rfind("///").unwrap();
    let body = &source[start..start + source[start..].find("\n}\n").unwrap()];
    for line in body.lines() {
        assert!(
            !line.contains("cfg"),
            "a cfg in the ingredient hook: {line}"
        );
        for mac in ["!(", "![", "! {", "!{"] {
            assert!(!line.contains(mac), "a macro: {line}");
        }
    }
}

#[test]
fn loot_table_load_stays_hookable() {
    let source = include_str!("../../../horizon-world/src/loot.rs").replace("\r\n", "\n");
    // From its doc comment's last line, so an attribute above it counts.
    let hook = source.find("pub fn native_loot_table_load(").unwrap();
    let start = source[..hook].rfind("///").unwrap();
    let end = source[start..]
        .find("\n}\n")
        .or_else(|| source[start..].find("\r\n}\r\n"))
        .unwrap();
    let body = &source[start..start + end];
    for line in body.lines() {
        assert!(
            !line.contains("cfg"),
            "a cfg in the loot hook point: {line}"
        );
        for mac in ["!(", "![", "! {", "!{"] {
            assert!(
                !line.contains(mac),
                "a macro in the loot hook point: {line}"
            );
        }
    }
}

#[test]
fn lifecycle_source_stays_hookable() {
    let source = include_str!("native_lifecycle.rs");
    let code = source.split("#[cfg(test)]").next().unwrap_or_default();
    for line in code.lines().filter(|l| !l.trim_start().starts_with("//")) {
        assert!(!line.contains("cfg"), "a cfg in lifecycle source: {line}");
        for mac in ["!(", "![", "! {", "!{"] {
            assert!(!line.contains(mac), "a macro in lifecycle source: {line}");
        }
    }
}

#[test]
fn catalog_matches_hook_points() {
    use std::collections::BTreeSet;

    let catalog: serde_json::Value =
        serde_json::from_str(include_str!("../../../../native-hooks.json")).unwrap();
    let mut hooks = catalog["hooks"].as_array().unwrap().clone();
    let fragments =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/native-hook-fragments");
    for entry in std::fs::read_dir(fragments).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let fragment: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            hooks.extend(
                fragment["hooks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .cloned()
                    .map(|mut hook| {
                        hook["version"] = 1.into();
                        hook
                    }),
            );
        }
    }
    let source = include_str!("native_hooks.rs");
    let section = source
        .split("/// The hook points. Each does nothing in Horizon.")
        .nth(1)
        .unwrap()
        .split("/// Server-level helpers for native mods.")
        .next()
        .unwrap();
    let level_section = source
        .split("impl GameLevel<'_> {")
        .nth(1)
        .unwrap()
        .split("\n}")
        .next()
        .unwrap();
    let level_hooks: BTreeSet<_> = level_section
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("pub(super) fn native_")?
                .split('(')
                .next()
        })
        .collect();
    // Hook points that are free functions in other crates.
    let free_hooks = |source: &'static str| -> BTreeSet<&'static str> {
        source
            .lines()
            .filter_map(|line| line.strip_prefix("pub fn native_")?.split('(').next())
            .collect()
    };
    // The renderer's (`horizon_client_render::gfx::native_render`).
    let render_hooks = free_hooks(include_str!(
        "../../../horizon-client-render/src/gfx/native_render.rs"
    ));
    // Items' default components (`horizon_world::item_defaults`).
    let item_hooks = free_hooks(include_str!("../../../horizon-world/src/item_defaults.rs"));
    // The loot tables' hook point (`horizon_world::loot`).
    let loot_hooks = free_hooks(include_str!("../../../horizon-world/src/loot.rs"));
    // World generation's hook points (`horizon_world::worldgen::native_generate`).
    let gen_hooks = free_hooks(include_str!(
        "../../../horizon-world/src/worldgen/native_generate.rs"
    ));
    // The menu engine's (`horizon_client::menu`).
    let menu_hooks = free_hooks(include_str!("../menu.rs"));
    // The air supply's (`horizon_client::breathing`).
    let breath_hooks = free_hooks(include_str!("../breathing.rs"));
    // The crafting loaders' (`horizon_client_lib::crafting`).
    let crafting_hooks = free_hooks(include_str!("../crafting.rs"));
    let hunger_hooks = free_hooks(include_str!("../hunger.rs"));
    let landing_hooks = free_hooks(include_str!("../landing.rs"));
    let motion_hooks = free_hooks(include_str!("../motion.rs"));
    let sound_hooks = free_hooks(include_str!("../sound_playback.rs"));
    // The chat relay's (`horizon_client_lib::lan::hub::chat`).
    let chat_hooks = free_hooks(include_str!("../lan/hub/chat.rs"));
    // The mob AI's (`horizon_client_lib::mobs::ai`).
    let mob_hooks = free_hooks(include_str!("../mobs/ai.rs"));
    // The connections' (`horizon_client_lib::lan::conn`).
    let conn_hooks = free_hooks(include_str!("../lan/conn.rs"));
    // The log writer's (`horizon_client_lib::native_client_hooks`).
    let client_hooks = free_hooks(include_str!("../native_client_hooks.rs"));
    let declared: BTreeSet<_> = section
        .lines()
        .chain(level_section.lines())
        .filter_map(|line| {
            line.trim()
                .strip_prefix("pub(super) fn native_")?
                .split('(')
                .next()
        })
        .chain(render_hooks.iter().copied())
        .chain(item_hooks.iter().copied())
        .chain(loot_hooks.iter().copied())
        .chain(gen_hooks.iter().copied())
        .chain(menu_hooks.iter().copied())
        .chain(breath_hooks.iter().copied())
        .chain(crafting_hooks.iter().copied())
        .chain(hunger_hooks.iter().copied())
        .chain(landing_hooks.iter().copied())
        .chain(motion_hooks.iter().copied())
        .chain(sound_hooks.iter().copied())
        .chain(chat_hooks.iter().copied())
        .chain(mob_hooks.iter().copied())
        .chain(conn_hooks.iter().copied())
        .chain(client_hooks.iter().copied())
        .collect();
    let mut names = BTreeSet::new();
    let mut contracts = BTreeSet::new();
    for hook in &hooks {
        let name = hook["name"].as_str().unwrap();
        let version = hook["version"].as_u64().unwrap();
        assert!(
            version > 0 && contracts.insert((name, version)),
            "invalid or duplicate hook: {name} v{version}"
        );
        names.insert(name);
        let target = if render_hooks.contains(name) {
            format!("horizon_client_render::gfx::native_render::native_{name}")
        } else if item_hooks.contains(name) {
            format!("horizon_world::item_defaults::native_{name}")
        } else if loot_hooks.contains(name) {
            format!("horizon_world::loot::native_{name}")
        } else if gen_hooks.contains(name) {
            format!("horizon_world::worldgen::native_generate::native_{name}")
        } else if crafting_hooks.contains(name) {
            // crafting.rs compiles into horizon-client-lib (#5446's
            // split): horizon-mixins resolves the crate by its package.
            format!("horizon_client_lib::crafting::native_{name}")
        } else if hunger_hooks.contains(name) {
            format!("horizon_client_lib::hunger::native_{name}")
        } else if landing_hooks.contains(name) {
            format!("horizon_client_lib::landing::native_{name}")
        } else if motion_hooks.contains(name) {
            format!("horizon_client_lib::motion::native_{name}")
        } else if sound_hooks.contains(name) {
            format!("horizon_client_lib::sound_playback::native_{name}")
        } else if chat_hooks.contains(name) {
            format!("horizon_client_lib::lan::hub::chat::native_{name}")
        } else if menu_hooks.contains(name) {
            // menu.rs compiles into horizon-client-lib (#5446's
            // split): horizon-mixins resolves the crate by its package.
            format!("horizon_client_lib::menu::native_{name}")
        } else if breath_hooks.contains(name) {
            // breathing.rs compiles into horizon-client-lib too.
            format!("horizon_client_lib::breathing::native_{name}")
        } else if mob_hooks.contains(name) {
            format!("horizon_client_lib::mobs::ai::native_{name}")
        } else if conn_hooks.contains(name) {
            format!("horizon_client_lib::lan::conn::native_{name}")
        } else if client_hooks.contains(name) {
            format!("horizon_client_lib::native_client_hooks::native_{name}")
        } else if level_hooks.contains(name) {
            format!("horizon_client::game::native_hooks::GameLevel<'_>::native_{name}")
        } else {
            format!("horizon_client::game::native_hooks::Game<'_>::native_{name}")
        };
        assert_eq!(hook["target"].as_str().unwrap(), target);
    }
    assert_eq!(names, declared);
    // Every target's crate really compiles its module: #5446 moved
    // files between horizon-client and horizon-client-lib, and a
    // target left on the old crate applies to no bundle — the string
    // comparison above shares its expectation with the catalog, so it
    // alone cannot catch a move. The first path segment after the
    // crate must be a module of that crate's lib root.
    let client_root = include_str!("../lib.rs");
    let lib_root = include_str!("../client_lib.rs");
    for hook in &hooks {
        let target = hook["target"].as_str().unwrap();
        let (root, crate_name, rest) = if let Some(rest) = target.strip_prefix("horizon_client::") {
            (client_root, "horizon_client", rest)
        } else if let Some(rest) = target.strip_prefix("horizon_client_lib::") {
            (lib_root, "horizon_client_lib", rest)
        } else {
            // The other crates (render, world) are their own packages,
            // never split.
            continue;
        };
        let module = rest.split("::").next().unwrap_or(rest);
        assert!(
            root.contains(&format!("mod {module};")),
            "{crate_name} does not compile module `{module}` (target {target})"
        );
    }
}

/// The damage hook gets a mob's source from its damage: the causing mob,
/// the direct one when it deals the damage itself.
#[test]
fn the_damage_hook_names_the_source() {
    use super::{HurtSource, Hurting, hurt_server_args, mob_source};
    use horizon_client::mobs::{Damage, DamageKind as K, Who};
    let source = |kind, attacker| {
        let s = mob_source(&Damage {
            kind,
            attacker,
            from: None,
            attacker_species: None,
        });
        (s.damage_type, s.causing, s.direct)
    };
    let zombie = Some(Who::Mob(3));
    assert_eq!(source(K::MobAttack, zombie), ("minecraft:mob_attack", 3, 3));
    assert_eq!(source(K::Thorns, zombie), ("minecraft:thorns", 3, 3));
    assert_eq!(source(K::Arrow, zombie), ("minecraft:arrow", 3, 0));
    let blast = source(K::Explosion, zombie);
    assert_eq!(blast, ("minecraft:player_explosion", 3, 0));
    // A player behind a mob's hit isn't known yet, nor is a fall's source.
    let hit = source(K::PlayerAttack, Some(Who::Player));
    assert_eq!(hit, ("minecraft:player_attack", 0, 0));
    assert_eq!(source(K::Fall, None), ("minecraft:fall", 0, 0));
    let arrow = HurtSource {
        damage_type: "minecraft:arrow",
        causing: 4,
        direct: 0,
    };
    let player = (2, "minecraft:player", arrow);
    assert_eq!(hurt_server_args(Hurting::Player(2, arrow)), player);
    let dragon = (5, "minecraft:ender_dragon", arrow);
    assert_eq!(hurt_server_args(Hurting::Dragon(5, arrow)), dragon);
}

/// A mod container opens a slot grid or a furnace's menu, by its slot
/// count; no menu with rules of its own (a brewing stand's, a crafter's).
#[test]
fn mod_menus_are_slot_grids_and_furnaces() {
    use super::mod_menu;
    assert_eq!(mod_menu("minecraft:hopper"), Some(("minecraft:hopper", 5)));
    assert_eq!(mod_menu("minecraft:generic_9x3").map(|m| m.1), Some(27));
    assert_eq!(mod_menu("minecraft:furnace").map(|m| m.1), Some(3));
    assert_eq!(mod_menu("minecraft:brewing_stand"), None);
    assert_eq!(mod_menu("hopper"), None);
}

/// The config folder is `config/` beside `options.txt`, made when
/// missing.
#[test]
fn config_dir_is_config_in_the_game_folder() {
    // Callable without a game, from any hook.
    let _: fn() -> Result<std::path::PathBuf, String> = super::Game::native_config_dir;
    use super::config_dir;
    let root = std::env::temp_dir().join(format!("horizon-config-dir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(!root.join("config").exists());
    let dir = config_dir(&root).unwrap();
    assert_eq!(dir, root.join("config"));
    assert!(dir.is_dir());
    assert_eq!(config_dir(&root).unwrap(), dir);
    let _ = std::fs::remove_dir_all(&root);
}

/// Vanilla's F3 draws a column in each top corner (`extractLines`, left
/// and right aligned), and its charts along the bottom.
#[test]
fn the_debug_screen_takes_both_top_corners_and_its_charts_corners() {
    use super::{Corner, debug_takes};
    for corner in Corner::ALL {
        assert!(!debug_takes(corner, false, [true, true]), "{corner:?}");
    }
    assert!(debug_takes(Corner::TopLeft, true, [false, false]));
    assert!(debug_takes(Corner::TopRight, true, [false, false]));
    assert!(!debug_takes(Corner::BottomLeft, true, [false, false]));
    assert!(!debug_takes(Corner::BottomRight, true, [false, false]));
    assert!(debug_takes(Corner::BottomLeft, true, [true, false]));
    assert!(!debug_takes(Corner::BottomRight, true, [true, false]));
    assert!(debug_takes(Corner::BottomRight, true, [false, true]));
}

/// An image is taken whole, and only when it fits its slot.
#[test]
fn a_mod_image_is_taken_whole_and_only_when_it_fits() {
    use super::mod_image_fits;
    assert!(mod_image_fits(4, 2, 32, 1024, 1024));
    assert!(mod_image_fits(1024, 1024, 1024 * 1024 * 4, 1024, 1024));
    assert!(!mod_image_fits(0, 2, 0, 1024, 1024));
    assert!(!mod_image_fits(4, 2, 31, 1024, 1024));
    assert!(!mod_image_fits(4, 2, 33, 1024, 1024));
    assert!(!mod_image_fits(2048, 1, 2048 * 4, 1024, 1024));
}
