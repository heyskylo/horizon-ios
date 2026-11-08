//! `/data modify storage … value[] set value "bad"`: `StorageDataAccessor`
//! edits the stored compound itself, and `AllElementsNode.setTag` clears a
//! non-empty array before it rejects the string. The command fails, and the
//! storage keeps the cleared array, with no `setData` and so no save of its
//! own (tools/test_storage_alias_parity.py measures the same on vanilla 26.3).
use super::*;
use horizon_world::nbt::{Tag, parse_snbt};

fn game() -> Game<'static> {
    let root = std::path::PathBuf::from(
        std::env::var_os("HORIZON_TEST_RESOURCES")
            .expect("set HORIZON_TEST_RESOURCES to prepared resources"),
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
    options.execution_role = execution_role::ExecutionRole::IntegratedServer;
    let res: &'static _ = Box::leak(Box::new(loaded.resources));
    let mut game = Game::new(
        res,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(&root),
        None,
    )
    .unwrap();
    game.build_registries();
    game.set_allow_commands(true);
    game
}

fn stored(game: &Game<'_>, id: &str) -> Tag {
    game.score.storage.get(id)
}
fn snbt(text: &str) -> Tag {
    parse_snbt(text).unwrap().0
}
/// Whether saving the storage now writes a file (it is dirty).
fn writes(game: &mut Game<'_>, name: &str) -> bool {
    let dir = std::env::temp_dir().join(format!("h246-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    game.score.storage.save(&dir);
    let wrote = dir.join("data").exists();
    let _ = std::fs::remove_dir_all(&dir);
    wrote
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_rejected_wildcard_set_keeps_the_storage_s_cleared_array() {
    let mut game = game();
    assert!(game.run_command("data merge storage h246:witness {v:123}"));
    for (kind, initial, cleared) in [
        ("byte", "[B;1b,2b]", "[B;]"),
        ("int", "[I;1,2]", "[I;]"),
        ("long", "[L;1L,2L]", "[L;]"),
    ] {
        let id = format!("h246:{kind}");
        assert!(game.run_command(&format!("data merge storage {id} {{value:{initial}}}")));
        assert!(writes(&mut game, kind), "{kind}: the merge is saved");
        assert!(
            !game.run_command(&format!(
                "data modify storage {id} value[] set value \"bad\""
            )),
            "{kind}: the command fails"
        );
        assert_eq!(
            stored(&game, &id),
            snbt(&format!("{{value:{cleared}}}")),
            "{kind}: the array was cleared in place"
        );
        assert!(!writes(&mut game, kind), "{kind}: no save of its own");
        assert_eq!(stored(&game, "h246:witness"), snbt("{v:123}"));
    }
    // Nothing to change: nothing cleared.
    assert!(game.run_command("data merge storage h246:same {value:[I;7,7]}"));
    assert!(!game.run_command("data modify storage h246:same value[] set value 7"));
    assert_eq!(stored(&game, "h246:same"), snbt("{value:[I;7,7]}"));
}

/// Runs `command`, which must fail because nothing changed.
fn fails_unchanged(game: &mut Game<'_>, command: &str) {
    game.chat_capture = Some(Vec::new());
    assert!(!game.run_command(command), "{command}");
    let said = game.chat_capture.take().unwrap_or_default();
    assert_eq!(
        said.first()
            .map(horizon_client::text::Text::plain)
            .as_deref(),
        Some("Nothing changed. The specified properties already have these values"),
        "{command}"
    );
}

/// The storage file a save writes under `dir`, read back.
fn saved(
    game: &mut Game<'_>,
    name: &str,
) -> Option<horizon_client::command_storage::CommandStorage> {
    let dir = std::env::temp_dir().join(format!("h246-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    game.score.storage.save(&dir);
    let back = dir
        .join("data")
        .exists()
        .then(|| horizon_client::command_storage::CommandStorage::load(&dir));
    let _ = std::fs::remove_dir_all(&dir);
    back
}

/// Added controls (after the baseline): the failure keeps the storage's
/// save state as it was, a later set saves what the failure left, and a
/// failure on an id that is not stored, or one that changed nothing, stores
/// nothing new. An entity's data stays a copy.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn the_failure_keeps_the_save_state_and_stores_nothing_new() {
    let mut game = game();
    // Already marked for saving: the failure leaves it so, and the save
    // writes the cleared array.
    assert!(game.run_command("data merge storage h246:dirty {value:[I;1,2]}"));
    assert!(!game.run_command("data modify storage h246:dirty value[] set value \"bad\""));
    let back = saved(&mut game, "dirty").expect("still marked for saving");
    assert_eq!(back.get("h246:dirty"), snbt("{value:[I;]}"));
    // Saved, then failed: nothing to write, until a later set saves both.
    assert!(game.run_command("data merge storage h246:clean {value:[I;1,2]}"));
    assert!(saved(&mut game, "clean-before").is_some());
    assert!(!game.run_command("data modify storage h246:clean value[] set value \"bad\""));
    assert!(
        saved(&mut game, "clean-after").is_none(),
        "the failure saves nothing"
    );
    assert!(game.run_command("data merge storage h246:later {v:1}"));
    let back = saved(&mut game, "later").expect("the set marks it");
    assert_eq!(back.get("h246:clean"), snbt("{value:[I;]}"));
    assert_eq!(back.get("h246:later"), snbt("{v:1}"));
    // A failure on an id that is not stored stores nothing.
    let keys = game.score.storage.keys();
    fails_unchanged(
        &mut game,
        "data modify storage h246ghost:none {} merge value {}",
    );
    assert_eq!(game.score.storage.keys(), keys, "no new id");
    assert!(game.run_command("data merge storage h246:mark {v:2}"));
    let back = saved(&mut game, "ghost").unwrap();
    assert!(
        !back.keys().iter().any(|k| k.starts_with("h246ghost:")),
        "no new namespace"
    );
    // A failure that changed nothing keeps every id and its contents.
    assert!(game.run_command("data merge storage h246:same {value:[I;7,7]}"));
    let keys = game.score.storage.keys();
    fails_unchanged(
        &mut game,
        "data modify storage h246:same value[] set value 7",
    );
    assert_eq!(game.score.storage.keys(), keys);
    assert_eq!(stored(&game, "h246:same"), snbt("{value:[I;7,7]}"));
    // An entity's data is a copy: its UUID stays.
    game.world.chunks.insert(
        (0, 0),
        std::sync::Arc::new(horizon_client::chunk::Chunk::empty(0, 0)),
    );
    assert!(game.run_command("summon armor_stand 4.5 100 4.5 {NoGravity:1b,UUID:[I;582,0,0,1]}"));
    let stand = "00000246-0000-0000-0000-000000000001";
    fails_unchanged(
        &mut game,
        &format!("data modify entity {stand} UUID[] set value \"bad\""),
    );
    assert!(game.run_command(&format!(
        "execute if data entity {stand} {{UUID:[I;582,0,0,1]}}"
    )));
}
