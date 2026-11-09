//! Run alone with private prepared resources: installs process-wide trait mods.
use super::*;
use crate::mod_api::{self, Flow, Hooks, Host, InteractEntityEvent, ItemInteractMobEvent};
use horizon_client::{
    chunk::Chunk,
    merchant::{ItemCost, MerchantOffer, Trader},
    mobs::Mob,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

#[derive(Default)]
struct Probe {
    calls: Mutex<Vec<(i32, String, String)>>,
    answer: AtomicU8,
    claim_entity: AtomicBool,
    discard: AtomicBool,
}
impl Hooks for Arc<Probe> {
    fn interact_entity(&self, _: &mut dyn Host, _: &mut InteractEntityEvent<'_>) -> Flow {
        if self.claim_entity.load(Ordering::Relaxed) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }
    fn item_interact_mob(
        &self,
        host: &mut dyn Host,
        event: &mut ItemInteractMobEvent<'_>,
    ) -> Option<bool> {
        let stack = event.stack.expect("only a nonempty item has this fallback");
        assert!(stack.count > 0);
        self.calls
            .lock()
            .unwrap()
            .push((event.entity, event.kind.into(), stack.item.to_string()));
        let answer = self.answer.load(Ordering::Relaxed);
        if answer != 0 && self.discard.load(Ordering::Relaxed) {
            assert!(host.remove_mob("minecraft:overworld", event.entity as u32));
        }
        match answer {
            0 => None,
            1 => Some(false),
            _ => Some(true),
        }
    }
}
fn setup(game: &mut Game<'_>, probe: &Probe, kind: &str, id: u32) {
    game.mobs.clear();
    let kind = game.res.mob_types().get(kind).unwrap().clone();
    game.mobs
        .push(Mob::new(id, kind, Vec3::new(0.5, 64.0, 2.5), 0.0));
    game.station = None;
    game.screen = Screen::Playing;
    game.inventory[0] = Some(ItemStack::new("minecraft:stick", 2));
    game.offhand = Some(ItemStack::new("minecraft:paper", 3));
    game.swing = Default::default();
    game.swings_started = 0;
    game.tick_hit = None;
    game.tick_mob = None;
    game.riding = None;
    probe.calls.lock().unwrap().clear();
    probe.answer.store(1, Ordering::Relaxed);
    probe.claim_entity.store(false, Ordering::Relaxed);
    probe.discard.store(false, Ordering::Relaxed);
}
fn target(id: i32) -> Option<InteractTarget> {
    Some(InteractTarget {
        id,
        location: [0.0, 1.0, 0.0],
    })
}

#[test]
#[ignore = "private prepared resources in HORIZON_TEST_RESOURCES"]
fn actual_item_interact_mob_fallback_priority_and_filters() {
    crate::mod_api::in_own_process(
        "game::item_interact_mob_tests::actual_item_interact_mob_fallback_priority_and_filters",
        actual_item_interact_mob_fallback_priority_and_filters_in_own_process,
    );
}

fn actual_item_interact_mob_fallback_priority_and_filters_in_own_process() {
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
    game.player = Some(PlayerMotion::new(game.motion, Vec3::new(0.5, 64.0, 0.5)).unwrap());
    game.yaw = 0.0;
    game.pitch = 0.0;
    game.selected = 0;
    game.set_mode_id(0);

    // A custom item willing to own every mob still cannot preempt trading.
    setup(&mut game, &probe, "minecraft:villager", 42);
    game.mobs[0].extra.trade = Some(Box::new(Trader {
        offers: vec![MerchantOffer {
            base_cost_a: ItemCost {
                stack: ItemStack::new("minecraft:emerald", 1),
            },
            cost_b: None,
            result: ItemStack::new("minecraft:bread", 1),
            uses: 0,
            max_uses: 12,
            reward_exp: true,
            special_price_diff: 0,
            demand: 0,
            price_multiplier: 0.0,
            xp: 1,
        }],
        ..Default::default()
    }));
    assert!(game.interact_entity(target(42)).unwrap());
    assert!(probe.calls.lock().unwrap().is_empty());
    assert!(!game.mobs[0].removed);
    assert_eq!(
        game.station
            .as_ref()
            .unwrap()
            .merchant
            .as_ref()
            .unwrap()
            .mob,
        Some(42)
    );

    // Existing API 29 owns the earlier phase and retains its usual swing.
    setup(&mut game, &probe, "minecraft:zombie", 42);
    probe.claim_entity.store(true, Ordering::Relaxed);
    assert!(game.interact_entity(target(42)).unwrap());
    assert!(probe.calls.lock().unwrap().is_empty());
    assert_eq!(game.swings_started, 1);
    for (answer, consumed, swings) in [(0, false, 0), (1, true, 0), (2, true, 1)] {
        setup(&mut game, &probe, "minecraft:zombie", 42);
        probe.answer.store(answer, Ordering::Relaxed);
        assert_eq!(game.interact_entity(target(42)).unwrap(), consumed);
        assert_eq!(game.swings_started, swings);
        assert_eq!(
            *probe.calls.lock().unwrap(),
            [(42, "minecraft:zombie".into(), "minecraft:stick".into())]
        );
    }

    // An ordinary mob with no own item action reaches the fallback exactly
    // once. The mod may discard it with no loot and end without an arm swing.
    setup(&mut game, &probe, "minecraft:blaze", 42);
    probe.discard.store(true, Ordering::Relaxed);
    assert!(game.interact_entity(None).unwrap());
    assert!(game.mobs[0].removed);
    assert!(game.dropped.is_empty());
    assert_eq!(game.swings_started, 0);
    assert_eq!(probe.calls.lock().unwrap().len(), 1);
    assert_eq!(game.inventory[0].as_ref().unwrap().count, 2);

    // Panda's early false is still a normal pass into the outer fallback.
    setup(&mut game, &probe, "minecraft:panda", 42);
    assert!(game.interact_entity(target(42)).unwrap());
    assert_eq!(probe.calls.lock().unwrap().len(), 1);
    assert_eq!(game.swings_started, 0);

    setup(&mut game, &probe, "minecraft:zombie", 42);
    assert!(
        game.with_hand(Hand::Off, |g| g.interact_entity(target(42)))
            .unwrap()
    );
    assert_eq!(probe.calls.lock().unwrap()[0].2, "minecraft:paper");
    assert_eq!(game.hand, Hand::Main);
    assert_eq!(game.inventory[0].as_ref().unwrap().count, 2);
    assert_eq!(game.offhand.as_ref().unwrap().count, 3);
    assert_eq!(game.swings_started, 0);

    for invalid in ["dead", "removed", "riding", "wrong_id", "empty", "zero"] {
        setup(&mut game, &probe, "minecraft:zombie", 42);
        match invalid {
            "dead" => game.mobs[0].health = 0.0,
            "removed" => game.mobs[0].removed = true,
            "riding" => game.riding = Some(42),
            "empty" => game.inventory[0] = None,
            "zero" => game.inventory[0].as_mut().unwrap().count = 0,
            _ => {}
        }
        assert!(
            !game
                .interact_entity(target(if invalid == "wrong_id" { 99 } else { 42 }))
                .unwrap(),
            "{invalid}"
        );
        assert!(probe.calls.lock().unwrap().is_empty(), "{invalid}");
    }
    setup(&mut game, &probe, "minecraft:zombie", 42);
    game.mobs[0].health = 0.0;
    assert!(!game.interact_entity(None).unwrap());
    assert!(probe.calls.lock().unwrap().is_empty());

    // Local selection is occluded; a validated server packet keeps its exact
    // entity ID, independent of the acting player's camera and local ray.
    setup(&mut game, &probe, "minecraft:blaze", 42);
    let stone = game
        .pack
        .assets
        .registry
        .default_state("minecraft:stone")
        .unwrap();
    game.world.set(0, 65, 1, stone).unwrap();
    assert!(!game.interact_entity(None).unwrap());
    assert!(probe.calls.lock().unwrap().is_empty());
    assert!(game.interact_entity(target(42)).unwrap());
    assert_eq!(probe.calls.lock().unwrap().len(), 1);
    setup(&mut game, &probe, "minecraft:zombie", 42);
    game.set_mode_id(3);
    assert_eq!(game.use_item_as_player().unwrap(), None);
    assert!(probe.calls.lock().unwrap().is_empty());
}
