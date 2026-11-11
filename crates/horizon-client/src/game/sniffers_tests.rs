//! Sniffer digging effects: the `entity_action` a dig posts every ten ticks
//! (`Sniffer.emitDiggingParticles`), the local sounds silence does not mute
//! (`playSearchingSound`, the block's hit sound) and the egg's own-random push
//! (`Sniffer.spawnChildFromBreeding`'s `new ItemEntity(..)`).
use super::super::kill_die_vibration_tests::game as sculk_game;
use super::super::sculk::TEST_POSTED_EVENTS;
use super::super::{Game, PlayerMotion};
use super::{Mob, sniffer};
use horizon_client::mobs::Who;
use horizon_core::{geometry::Vec3, random::RandomSource};

fn game() -> Game<'static> {
    let mut game = sculk_game();
    game.player = Some(PlayerMotion::new(&game.motion, Vec3::new(8.5, 100.0, 8.5)).unwrap());
    game.mobs.clear();
    game
}

/// A digging sniffer at `(8.5, 100, 8.5)`, facing +z, its head above the stone.
fn digging_sniffer(game: &mut Game<'_>) -> (u32, [i32; 3]) {
    let kind = game
        .res
        .mob_types()
        .get("minecraft:sniffer")
        .unwrap()
        .clone();
    let mut m = Mob::new(42, kind, Vec3::new(8.5, 100.0, 8.5), 0.0);
    sniffer::set_state(&mut m, sniffer::DIGGING);
    let head = sniffer::head_block(&m);
    game.mobs.push(m);
    (42, head)
}

fn posted() -> Vec<(&'static str, [f64; 3], Option<Who>)> {
    TEST_POSTED_EVENTS.with(|events| events.borrow().clone().unwrap_or_default())
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn digging_posts_entity_action_every_ten_ticks_outside_the_dust_window() {
    let mut game = game();
    let (id, head) = digging_sniffer(&mut game);
    let at = [
        head[0] as f64 + 0.5,
        head[1] as f64 + 0.5,
        head[2] as f64 + 0.5,
    ];
    // 0 is the first dig tick, 20 is before the window, 40 inside it and 130
    // after it; 25 is not a tenth tick.
    for age in [0_u32, 20, 40, 130, 25] {
        let m = &mut game.mobs[0];
        m.age = age;
        m.anim.states[3].start(0.0);
        TEST_POSTED_EVENTS.with(|events| *events.borrow_mut() = Some(Vec::new()));
        game.tick_sniffer_effects();
        let events = posted();
        let action = events
            .iter()
            .filter(|(event, ..)| *event == "minecraft:entity_action")
            .collect::<Vec<_>>();
        if age.is_multiple_of(10) {
            assert_eq!(
                action,
                vec![&("minecraft:entity_action", at, Some(Who::Mob(id)))],
                "age {age}"
            );
        } else {
            assert!(action.is_empty(), "age {age} posted {events:?}");
        }
    }
    TEST_POSTED_EVENTS.with(|events| *events.borrow_mut() = None);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_silent_sniffer_still_plays_the_local_searching_and_hit_sounds() {
    let mut game = game();
    let _ = digging_sniffer(&mut game);
    game.mobs[0].silent = true;
    game.mobs[0].age = 20;
    sniffer::set_state(&mut game.mobs[0], sniffer::SEARCHING);
    game.audio.heard.clear();
    game.tick_sniffer_effects();
    assert_eq!(
        game.audio.heard.get("minecraft:entity.sniffer.searching"),
        Some(&1)
    );
    sniffer::set_state(&mut game.mobs[0], sniffer::DIGGING);
    game.mobs[0].age = 40;
    game.mobs[0].anim.states[3].start(0.0);
    game.audio.heard.clear();
    game.tick_sniffer_effects();
    assert_eq!(game.audio.heard.get("minecraft:block.stone.hit"), Some(&1));
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn the_egg_push_comes_from_the_items_own_random() {
    let mut game = game();
    let kind = game
        .res
        .mob_types()
        .get("minecraft:sniffer")
        .unwrap()
        .clone();
    let parent = Mob::new(43, kind, Vec3::new(8.5, 100.0, 8.5), 0.0);
    let mut level = game.world.random.clone();
    game.lay_sniffer_egg(&parent);
    let egg = game.dropped.last().unwrap();
    assert_eq!(&*egg.stack.item, "minecraft:sniffer_egg");
    assert_eq!(egg.pickup_delay, 10);
    // `new ItemEntity(level, x, y, z, stack)` draws its motion from the item's
    // random: the level's stream does not move.
    assert_eq!(game.world.random.next_double(), level.next_double());
}
