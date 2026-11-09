use super::*;
use horizon_core::geometry::Vec3;

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn empty_tag_selectors_follow_actual_tag_edits() {
    use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
    use horizon_client::motion::PlayerMotion;
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
        "horizon-empty-tags-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&scratch).unwrap();
    game.world_root = scratch.clone();
    game.player = Some(PlayerMotion::new(&game.motion, Vec3::new(10.5, 64.0, 0.5)).unwrap());

    game.build_registries();
    game.set_allow_commands(true);
    for (n, x, tags) in [(1, 4.5, ""), (2, 8.5, ",Tags:[\"audit\"]")] {
        assert!(game.run_command(&format!(
            "summon pig {x} 64 4.5 {{UUID:[I;267,0,0,{n}],NoAI:1b,NoGravity:1b{tags}}}"
        )));
    }
    let a = game.target_with_uuid([267, 0, 0, 1]).unwrap();
    let b = game.target_with_uuid([267, 0, 0, 2]).unwrap();
    assert_ne!(a, b);
    let select = |g: &mut Game<'_>, tag: &str| {
        let text = format!("@e[type=minecraft:pig,tag={tag}]");
        let selector = horizon_client::command::selector::parse(
            &mut horizon_client::command::Reader::new(&text),
            &g.ui.registries,
            true,
        )
        .unwrap();
        g.select(&selector).into_iter().collect::<HashSet<_>>()
    };
    for (phase, tagged_a) in [false, true, false].into_iter().enumerate() {
        if phase != 0 {
            for (n, tagged) in [(1, tagged_a), (2, !tagged_a)] {
                assert!(game.run_command(&format!(
                    "tag 0000010b-0000-0000-0000-{n:012} {} audit",
                    if tagged { "add" } else { "remove" }
                )));
            }
        }
        assert_eq!(game.target_with_uuid([267, 0, 0, 1]), Some(a));
        assert_eq!(game.target_with_uuid([267, 0, 0, 2]), Some(b));
        assert_eq!(
            game.tags_of(a),
            if tagged_a {
                vec!["audit".to_owned()]
            } else {
                vec![]
            }
        );
        assert_eq!(
            game.tags_of(b),
            if tagged_a {
                vec![]
            } else {
                vec!["audit".to_owned()]
            }
        );
        let (empty, tagged) = if tagged_a { (b, a) } else { (a, b) };
        // Named-tag admissions pass before the old empty-tag assertion fails.
        assert_eq!(select(&mut game, "audit"), HashSet::from([tagged]));
        assert_eq!(select(&mut game, "!audit"), HashSet::from([empty]));
        assert_eq!(select(&mut game, ""), HashSet::from([empty]));
        assert_eq!(select(&mut game, "!"), HashSet::from([tagged]));
    }
    drop(game);
    std::fs::remove_dir_all(scratch).unwrap();
}
