//! Run alone with private prepared resources: installs process-wide trait mods.
use super::super::*;
use super::{Hook, State};
use crate::mod_api::{self, FishingMediumEvent, Hooks, Host, NativeFishingMedium};
use horizon_client::chunk::Chunk;
use std::sync::Mutex;

/// Fishes the air east of x = 3, four blocks under its owner's feet, rolling the junk table.
#[derive(Default)]
struct Probe {
    asked: Mutex<Vec<(String, Option<[f64; 3]>)>>,
}
const JUNK: &str = "minecraft:gameplay/fishing/junk";
impl Hooks for Arc<Probe> {
    fn fishing_medium(
        &self,
        _: &mut dyn Host,
        event: &mut FishingMediumEvent<'_>,
    ) -> Option<NativeFishingMedium> {
        self.asked
            .lock()
            .unwrap()
            .push((event.dimension.to_owned(), event.owner));
        (event.position[0] > 3.0).then(|| NativeFishingMedium {
            surface: event.owner.map_or(60.0, |o| o[1] - 4.0),
            loot_table: Some(JUNK.into()),
            lure_ticks: Some((5, 10)),
        })
    }
}

const FISH: [&str; 4] = [
    "minecraft:cod",
    "minecraft:salmon",
    "minecraft:pufferfish",
    "minecraft:tropical_fish",
];

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_bobber_floats_bites_and_rolls_the_table_of_a_mods_medium() {
    crate::mod_api::in_own_process(
        "game::fishing::medium_tests::a_bobber_floats_bites_and_rolls_the_table_of_a_mods_medium",
        a_bobber_floats_bites_and_rolls_the_table_of_a_mods_medium_in_own_process,
    );
}

fn a_bobber_floats_bites_and_rolls_the_table_of_a_mods_medium_in_own_process() {
    let root = PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    let settings = Settings::default();
    let loaded = load_resources(
        root.clone(),
        None,
        &root,
        &settings,
        Instant::now(),
        false,
        true,
        &Progress::new(),
        &mut |_, _| unreachable!(),
    )
    .unwrap();
    let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
    options.resources = root.clone();
    options.execution_role = execution_role::ExecutionRole::IntegratedServer;
    let probe = Arc::new(Probe::default());
    assert!(
        mod_api::MODS
            .set(Arc::from(vec![Box::new(probe.clone()) as Box<dyn Hooks>]))
            .is_ok()
    );
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
    game.world = World::new(game.res.facts().air, 1);
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    game.player = Some(PlayerMotion::new(&game.motion, Vec3::new(0.5, 64.0, 0.5)).unwrap());
    game.selected = 0;
    game.set_mode_id(0);
    let rod = ItemStack::new("minecraft:fishing_rod", 1);
    game.inventory[0] = Some(rod.clone());

    // Over air the mod doesn't claim, the bobber falls as vanilla's does.
    game.hook = Some(Hook::remote(1, Vec3::new(1.5, 66.0, 0.5)));
    for _ in 0..40 {
        game.tick_fishing();
    }
    let hook = game.hook.as_ref().expect("still within reach");
    assert!(hook.position.y < 58.0, "fell to {}", hook.position.y);
    assert_eq!(game.hook_status().unwrap()["state"], "Flying");
    assert!(game.hook_status().unwrap()["loot_table"].is_null());
    {
        let asked = probe.asked.lock().unwrap();
        assert!(!asked.is_empty());
        assert!(
            asked.iter().all(
                |(dim, owner)| dim == "minecraft:overworld" && *owner == Some([0.5, 64.0, 0.5])
            )
        );
    }

    // In the mod's medium it falls to the surface, floats there and gets a bite.
    let mut caught = Vec::new();
    for cast in 0..6 {
        let id = 10 + cast;
        game.hook = Some(Hook::remote(id, Vec3::new(5.5, 66.0, 0.5)));
        let mut bit = false;
        for tick in 0..400 {
            game.tick_fishing();
            let status = game.hook_status().expect("the bobber stays out");
            let y = game.hook.as_ref().unwrap().position.y;
            if tick > 60 {
                assert_eq!(status["state"], "Bobbing");
                assert!((59.0..61.0).contains(&y), "floats at {y} on tick {tick}");
            }
            if status["nibble"].as_i64().unwrap() > 0 {
                assert_eq!(status["loot_table"], JUNK);
                bit = true;
                break;
            }
        }
        assert!(bit, "no bite in 20 seconds on cast {cast}");
        let before = game.dropped.len();
        assert!(game.use_fishing_rod(&rod));
        assert!(game.hook.is_none());
        let items: Vec<String> = game.dropped[before..]
            .iter()
            .map(|i| i.stack.item.to_string())
            .collect();
        assert!(!items.is_empty(), "a bite reeled in nothing");
        caught.extend(items);
    }
    // The junk table has no fish, where vanilla's would have landed some by now.
    assert!(
        caught.iter().all(|item| !FISH.contains(&item.as_str())),
        "{caught:?}"
    );

    // A client draws its own bobber floating in the same medium.
    let mut state = (Vec3::new(5.5, 62.0, 0.5), Vec3::ZERO, State::Flying, false);
    let mut random = horizon_core::random::LegacyRandom::new(7);
    for tick in 0..100 {
        state = game.client_hook_step(1, Some(Vec3::new(0.5, 64.0, 0.5)), state, &mut random);
        if tick > 60 {
            assert!((59.0..61.0).contains(&state.0.y), "client at {}", state.0.y);
        }
    }
}
