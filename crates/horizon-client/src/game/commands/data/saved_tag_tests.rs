use super::*;
use crate::game::{Audio, Options, Progress, Settings, load_resources};
use horizon_client::motion::PlayerMotion;
use horizon_core::geometry::Vec3;

/// A server-side game that runs commands, its world in a scratch folder.
fn with_command_game(name: &str, test: impl FnOnce(&mut Game<'_>)) {
    let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
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
    let scratch = std::env::temp_dir().join(format!(
        "horizon-saved-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&scratch).unwrap();
    game.world_root = scratch.clone();
    game.player = Some(PlayerMotion::new(game.motion, Vec3::new(10.5, 200.0, 0.5)).unwrap());
    game.build_registries();
    game.set_allow_commands(true);
    test(&mut game);
    drop(game);
    std::fs::remove_dir_all(scratch).unwrap();
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn tag_commands_and_explicit_empty_loads_save_without_empty_tags() {
    with_command_game("tags", tag_commands_and_explicit_empty);
}
fn tag_commands_and_explicit_empty(game: &mut Game<'_>) {
    let start = game.world.game_time;
    let uuid = "00000148-0000-0000-0000-000000000001";
    assert!(game.run_command("summon pig 4.5 200 4.5 {UUID:[I;328,0,0,1],Health:10f,NoAI:1b,NoGravity:1b,PortalCooldown:17}"));
    let target = game.target_with_uuid([328, 0, 0, 1]).unwrap();
    assert!(matches!(target, Target::Mob(_)));
    assert!(game.tags_of(target).is_empty());
    assert!(
        game.data_of(&DataRef::Entity(target))
            .unwrap()
            .get("Tags")
            .is_none()
    );
    for tagged in [true, false, true, false] {
        assert!(game.run_command(&format!(
            "tag {uuid} {} audit",
            if tagged { "add" } else { "remove" }
        )));
        let expected: Vec<String> = if tagged { vec!["audit".into()] } else { vec![] };
        assert_eq!(game.tags_of(target), expected);
        for _ in 0..2 {
            let saved = game.data_of(&DataRef::Entity(target)).unwrap();
            assert_eq!(saved.get("UUID"), Some(&Tag::IntArray(vec![328, 0, 0, 1])));
            assert_eq!(saved.get("Health"), Some(&Tag::Float(10.0_f32.to_bits())));
            assert_eq!(saved.get("NoAI"), Some(&Tag::Byte(1)));
            assert_eq!(saved.get("NoGravity"), Some(&Tag::Byte(1)));
            assert_eq!(saved.get("PortalCooldown"), Some(&Tag::Int(17)));
            assert_eq!(
                saved.get("Tags"),
                if tagged {
                    Some(Tag::List(vec![Tag::String("audit".into())]))
                } else {
                    None
                }
                .as_ref()
            );
            assert_eq!(game.tags_of(target), expected);
        }
    }
    assert!(game.run_command(
        "summon pig 8.5 200 4.5 {UUID:[I;328,0,0,2],NoAI:1b,Tags:[],PortalCooldown:23}"
    ));
    let empty = game.target_with_uuid([328, 0, 0, 2]).unwrap();
    assert!(game.tags_of(empty).is_empty());
    let saved = game.data_of(&DataRef::Entity(empty)).unwrap();
    assert!(saved.get("Tags").is_none());
    assert_eq!(saved.get("PortalCooldown"), Some(&Tag::Int(23)));
    assert!(game.run_command("tag 00000148-0000-0000-0000-000000000002 add keep"));
    assert_eq!(game.tags_of(empty), ["keep"]);
    assert_eq!(
        game.data_of(&DataRef::Entity(empty)).unwrap().get("Tags"),
        Some(&Tag::List(vec![Tag::String("keep".into())]))
    );
    assert_eq!(game.world.game_time, start);
}

/// `/data get` on a cloud that was never saved shows `Entity.saveWithoutId`'s
/// common fields, as 26.3 prints them (#13057).
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_live_area_effect_cloud_reads_with_the_common_entity_fields() {
    with_command_game("cloud", |game| {
        assert!(game.run_command("summon area_effect_cloud 4.5 200 4.5 {UUID:[I;328,0,0,3]}"));
        let target = game.target_with_uuid([328, 0, 0, 3]).unwrap();
        let read = game.data_of(&DataRef::Entity(target)).unwrap();
        let zero = Tag::Double(0);
        assert_eq!(read.get("Motion"), Some(&Tag::List(vec![zero.clone(); 3])));
        assert_eq!(read.get("fall_distance"), Some(&zero));
        assert_eq!(read.get("Air"), Some(&Tag::Short(300)));
        assert_eq!(read.get("OnGround"), Some(&Tag::Byte(0)));
        assert_eq!(read.get("Invulnerable"), Some(&Tag::Byte(0)));
        assert_eq!(read.get("PortalCooldown"), Some(&Tag::Int(0)));
    });
}
