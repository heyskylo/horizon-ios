use super::*;
use horizon_core::geometry::Vec3;

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn selectors_reject_criteria_absent_from_the_selected_definition() {
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
        "horizon-criterion-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&scratch).unwrap();
    game.world_root = scratch.clone();
    game.player = Some(PlayerMotion::new(&game.motion, Vec3::new(10.5, 64.0, 0.5)).unwrap());

    const ROOT: &str = "minecraft:story/root";
    let tree = game.advancement_tree();
    let root = tree.get(ROOT).unwrap();
    assert_eq!(root.criteria.len(), 1);
    assert_eq!(root.criteria[0].name, "crafting_table");
    assert!(
        tree.get("minecraft:story/mine_stone")
            .unwrap()
            .criteria
            .iter()
            .any(|c| c.name == "get_stone")
    );
    let select = |g: &mut Game<'_>, condition: &str| {
        let text = format!("@a[advancements={{{condition}}}]");
        let selector = horizon_client::command::selector::parse(
            &mut horizon_client::command::Reader::new(&text),
            &horizon_client::command::args::Registries::default(),
            true,
        )
        .unwrap();
        g.select(&selector)
    };
    for done in [false, true, false] {
        if done {
            game.adv
                .progress
                .grant(ROOT, "crafting_table", "2026-11-05 00:00:00 +0000");
        } else {
            game.adv.progress.revoke(ROOT, "crafting_table");
        }
        let want = |yes| {
            if yes {
                vec![Target::Player]
            } else {
                Vec::new()
            }
        };
        assert_eq!(
            select(&mut game, &format!("{ROOT}={{crafting_table=false}}")),
            want(!done)
        );
        assert!(select(&mut game, &format!("{ROOT}={{h266_missing=false}}")).is_empty());
        assert!(select(&mut game, &format!("{ROOT}={{h266_missing=true}}")).is_empty());
        // A name defined in another advancement is still missing in this one.
        assert!(select(&mut game, &format!("{ROOT}={{get_stone=false}}")).is_empty());
        assert_eq!(
            select(&mut game, &format!("{ROOT}={{crafting_table=true}}")),
            want(done)
        );
        assert_eq!(select(&mut game, &format!("{ROOT}=true")), want(done));
        assert_eq!(select(&mut game, &format!("{ROOT}=false")), want(!done));
        assert_eq!(select(&mut game, &format!("{ROOT}={{}}")), want(true));
        assert!(select(&mut game, "h266:missing=false").is_empty());
        assert!(select(&mut game, "h266:missing={}").is_empty());
    }
    drop(game);
    std::fs::remove_dir_all(scratch).unwrap();
}
