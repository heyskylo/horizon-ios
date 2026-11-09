//! `/damage` on an armor stand: `ArmorStand.hurtServer` returns false after
//! an explosion has broken the stand (its equipment pops, then `kill`), after
//! `in_fire` has set it on fire and after `on_fire` has burnt 4 health off a
//! burning one, so the command fails while the stand changes
//! (tools/test_stand_command_hurt_parity.py measures the same on vanilla 26.3).
use super::*;
use horizon_client::chunk::Chunk;
use horizon_client::mobs::Who;
use horizon_core::position::ChunkPos;
use std::sync::Arc;

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
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    game.block_ticks
        .add_empty_chunk(ChunkPos::new(0, 0))
        .unwrap();
    assert!(game.run_command("fill 0 99 0 15 99 15 stone"));
    game
}

/// What a command says, and whether it succeeded.
fn say(game: &mut Game<'_>, command: &str) -> (bool, String) {
    game.chat_capture = Some(Vec::new());
    let ok = game.run_command(command);
    let said = game.chat_capture.take().unwrap_or_default();
    (
        ok,
        said.iter().map(horizon_client::text::Text::plain).collect(),
    )
}

const STAND: &str = "00000ad5-0000-0000-0000-000000000001";

fn summon(game: &mut Game<'_>, extra: &str) {
    assert!(game.run_command(&format!(
        "summon armor_stand 2.5 100 8.5 {{NoGravity:1b,UUID:[I;2773,0,0,1]{extra}}}"
    )));
    assert!(game.run_command(&format!("execute if entity {STAND}")));
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_blast_from_damage_breaks_the_stand_though_the_command_fails() {
    let mut game = game();
    summon(
        &mut game,
        r#",equipment:{head:{id:"minecraft:diamond_helmet",count:1}}"#,
    );
    let (ok, said) = say(&mut game, &format!("damage {STAND} 1 minecraft:explosion"));
    assert!(!ok, "hurtServer returns false: {said}");
    assert!(said.contains("invulnerable"), "{said}");
    assert!(
        !game.run_command(&format!("execute if entity {STAND}")),
        "the blast kills the stand"
    );
    assert!(
        game.run_command(
            "execute if entity @e[type=item,nbt={Item:{id:\"minecraft:diamond_helmet\"}}]"
        ),
        "its helmet pops above its block"
    );
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn fire_from_damage_lights_or_burns_the_stand_though_the_command_fails() {
    let mut game = game();
    summon(&mut game, "");
    assert!(!game.run_command(&format!("damage {STAND} 1 minecraft:in_fire")));
    let (_, fire) = say(&mut game, &format!("data get entity {STAND} Fire"));
    assert!(
        fire.ends_with(": 100s"),
        "in_fire lights it for 5 s: {fire}"
    );
    // Burning, on_fire takes 4 health.
    assert!(!game.run_command(&format!("damage {STAND} 1 minecraft:on_fire")));
    let (_, health) = say(&mut game, &format!("data get entity {STAND} Health"));
    assert!(health.ends_with(": 16.0f"), "{health}");
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn igniting_a_frozen_stand_clears_its_freeze_while_burning_keeps_it() {
    let mut game = game();
    summon(&mut game, ",TicksFrozen:140");
    let (_, frozen) = say(&mut game, &format!("data get entity {STAND} TicksFrozen"));
    assert!(frozen.ends_with(": 140"), "{frozen}");
    assert!(!game.run_command(&format!("damage {STAND} 1 minecraft:in_fire")));
    let (_, fire) = say(&mut game, &format!("data get entity {STAND} Fire"));
    assert!(fire.ends_with(": 100s"), "{fire}");
    let (ok, frozen_after) = say(&mut game, &format!("data get entity {STAND} TicksFrozen"));
    assert!(!ok, "{frozen_after}");

    // When already burning, in_fire calls causeDamage(0.15) instead of igniteForSeconds.
    assert!(game.run_command(&format!("data merge entity {STAND} {{TicksFrozen:140}}")));
    assert!(!game.run_command(&format!("damage {STAND} 1 minecraft:in_fire")));
    let (ok, still_frozen) = say(&mut game, &format!("data get entity {STAND} TicksFrozen"));
    assert!(ok && still_frozen.ends_with(": 140"), "{still_frozen}");
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn damage_immunity_on_worn_boots_refuses_campfire_but_not_in_fire_or_wrong_slot() {
    let mut game = game();
    summon(
        &mut game,
        r#",equipment:{feet:{id:"minecraft:diamond_boots",count:1,components:{"minecraft:enchantments":{"minecraft:frost_walker":1}}}}"#,
    );
    // campfire is in both #ignites_armor_stands and #burn_from_stepping.
    assert!(!game.run_command(&format!("damage {STAND} 1 minecraft:campfire")));
    let (_, fire) = say(&mut game, &format!("data get entity {STAND} Fire"));
    assert!(fire.ends_with(": 0s"), "{fire}");
    // in_fire is not in #burn_from_stepping, so it still ignites the stand.
    assert!(!game.run_command(&format!("damage {STAND} 1 minecraft:in_fire")));
    let (_, fire) = say(&mut game, &format!("data get entity {STAND} Fire"));
    assert!(fire.ends_with(": 100s"), "{fire}");

    // Frost Walker on a helmet (head) is not in an active slot for the enchantment.
    let mut helmeted = self::game();
    summon(
        &mut helmeted,
        r#",equipment:{head:{id:"minecraft:diamond_helmet",count:1,components:{"minecraft:enchantments":{"minecraft:frost_walker":1}}}}"#,
    );
    assert!(!helmeted.run_command(&format!("damage {STAND} 1 minecraft:campfire")));
    let (_, fire) = say(&mut helmeted, &format!("data get entity {STAND} Fire"));
    assert!(fire.ends_with(": 100s"), "{fire}");
}

fn capture() {
    super::sculk::TEST_POSTED_EVENTS.with(|events| *events.borrow_mut() = Some(Vec::new()));
}

fn take_events() -> Vec<(&'static str, [f64; 3], Option<horizon_client::mobs::Who>)> {
    super::sculk::TEST_POSTED_EVENTS.with(|events| events.borrow_mut().take().unwrap_or_default())
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn cause_damage_and_explosion_post_attributed_game_events() {
    use horizon_client::mobs::Who;
    let mut game = game();
    summon(&mut game, ",Fire:200s");
    assert!(game.run_command("summon pig 14.5 100 2.5 {NoAI:1b}"));
    let pig_id = game.mobs[0].id;
    let at = [2.5, 100.0, 8.5];

    // Burning stand hurt by in_fire (0.15) posts entity_damage attributed to the pig.
    capture();
    assert!(!game.run_command(&format!(
        "damage {STAND} 1 minecraft:in_fire by @e[type=pig,limit=1]"
    )));
    assert_eq!(
        take_events(),
        vec![("minecraft:entity_damage", at, Some(Who::Mob(pig_id)))]
    );

    // Burning stand hurt by on_fire (4.0) while surviving posts entity_damage.
    capture();
    assert!(!game.run_command(&format!(
        "damage {STAND} 1 minecraft:on_fire by @e[type=pig,limit=1]"
    )));
    assert_eq!(
        take_events(),
        vec![("minecraft:entity_damage", at, Some(Who::Mob(pig_id)))]
    );

    // Burning stand broken by on_fire (health <= 0.5) posts entity_die attributed to the pig.
    assert!(game.run_command(&format!("data merge entity {STAND} {{Health:4f}}")));
    capture();
    assert!(!game.run_command(&format!(
        "damage {STAND} 1 minecraft:on_fire by @e[type=pig,limit=1]"
    )));
    assert!(!game.run_command(&format!("execute if entity {STAND}")));
    assert_eq!(
        take_events(),
        vec![("minecraft:entity_die", at, Some(Who::Mob(pig_id)))]
    );

    // Stand broken by explosion posts entity_die attributed to the pig.
    summon(&mut game, "");
    capture();
    assert!(!game.run_command(&format!(
        "damage {STAND} 1 minecraft:explosion by @e[type=pig,limit=1]"
    )));
    assert!(!game.run_command(&format!("execute if entity {STAND}")));
    assert_eq!(
        take_events(),
        vec![("minecraft:entity_die", at, Some(Who::Mob(pig_id)))]
    );
}

// Controls added after the baseline: the guards before the acting sources,
// and a stand broken once however often it is hurt.

fn items(game: &mut Game<'_>) -> String {
    say(game, "execute if entity @e[type=item]").1
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn control_an_absent_or_removed_stand_is_not_broken_again() {
    let mut game = game();
    assert!(!game.run_command(&format!("damage {STAND} 1 minecraft:explosion")));
    assert_eq!(items(&mut game), "Test failed");
    summon(
        &mut game,
        r#",equipment:{head:{id:"minecraft:diamond_helmet",count:1}}"#,
    );
    assert!(!game.run_command(&format!("damage {STAND} 1 minecraft:explosion")));
    assert!(!game.run_command(&format!("damage {STAND} 1 minecraft:explosion")));
    assert_eq!(items(&mut game), "Test passed. Count: 1");
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn control_invulnerable_and_marker_stands_refuse_the_blast() {
    for flag in ["Invulnerable:1b", "Marker:1b"] {
        let mut game = game();
        summon(
            &mut game,
            &format!(r#",{flag},equipment:{{head:{{id:"minecraft:diamond_helmet",count:1}}}}"#),
        );
        assert!(!game.run_command(&format!("damage {STAND} 1 minecraft:explosion")));
        assert!(
            game.run_command(&format!("execute if entity {STAND}")),
            "{flag}"
        );
        assert_eq!(items(&mut game), "Test failed", "{flag}");
        assert!(!game.run_command(&format!("damage {STAND} 1 minecraft:in_fire")));
        let (_, fire) = say(&mut game, &format!("data get entity {STAND} Fire"));
        assert!(fire.ends_with(": 0s"), "{flag}: {fire}");
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn control_repeated_burning_breaks_the_stand_once() {
    let mut game = game();
    summon(
        &mut game,
        r#",Fire:200s,equipment:{head:{id:"minecraft:diamond_helmet",count:1}}"#,
    );
    // Burning, in_fire takes 0.15 instead of lighting it again.
    assert!(!game.run_command(&format!("damage {STAND} 1 minecraft:in_fire")));
    let (_, health) = say(&mut game, &format!("data get entity {STAND} Health"));
    assert!(
        health.ends_with(&format!(": {}f", 20.0f32 - 0.15)),
        "{health}"
    );
    assert!(game.run_command(&format!("data merge entity {STAND} {{Health:20f}}")));
    // 20 -> 16 -> 12 -> 8 -> 4, then 0 breaks it.
    for _ in 0..5 {
        assert!(!game.run_command(&format!("damage {STAND} 1 minecraft:on_fire")));
    }
    assert!(!game.run_command(&format!("execute if entity {STAND}")));
    assert!(!game.run_command(&format!("damage {STAND} 1 minecraft:on_fire")));
    assert_eq!(items(&mut game), "Test passed. Count: 1");
}

// The sources that return true: `#can_break_armor_stand` and
// `#always_kills_armor_stands` hurt the stand as a hit does, and the command
// succeeds.

fn with_player(game: &mut Game<'_>) {
    game.player = Some(
        horizon_client::motion::PlayerMotion::new(
            game.motion,
            horizon_core::geometry::Vec3::new(14.5, 100.0, 14.5),
        )
        .unwrap(),
    );
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_player_attack_from_damage_wobbles_then_breaks_the_stand() {
    let mut game = game();
    summon(
        &mut game,
        r#",equipment:{head:{id:"minecraft:diamond_helmet",count:1}}"#,
    );
    assert!(game.run_command("summon pig 14.5 100 2.5 {NoAI:1b}"));
    // First hit: hurtServer sets lastHit and returns true; the stand stays.
    let (ok, said) = say(
        &mut game,
        &format!("damage {STAND} 1 minecraft:player_attack by @e[type=pig,limit=1]"),
    );
    assert!(ok, "hurtServer returns true: {said}");
    assert!(said.starts_with("Applied"), "{said}");
    assert!(game.run_command(&format!("execute if entity {STAND}")));
    assert_eq!(items(&mut game), "Test failed");
    assert_eq!(game.armor_stands[0].last_hit, game.world.game_time);
    // A second hit within five ticks breaks it (brokenByPlayer): the stand
    // item and its helmet drop.
    assert!(game.run_command(&format!(
        "damage {STAND} 1 minecraft:player_attack by @e[type=pig,limit=1]"
    )));
    assert!(!game.run_command(&format!("execute if entity {STAND}")));
    assert_eq!(items(&mut game), "Test passed. Count: 2");
    assert!(
        game.run_command(
            "execute if entity @e[type=item,nbt={Item:{id:\"minecraft:armor_stand\"}}]"
        )
    );
    assert!(game.run_command(
        "execute if entity @e[type=item,nbt={Item:{id:\"minecraft:diamond_helmet\"}}]"
    ));
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn an_always_killing_source_breaks_the_stand_on_the_first_hit() {
    let mut game = game();
    summon(&mut game, "");
    // minecraft:arrow is in #always_kills_armor_stands.
    assert!(game.run_command(&format!("damage {STAND} 1 minecraft:arrow")));
    assert!(!game.run_command(&format!("execute if entity {STAND}")));
    assert_eq!(items(&mut game), "Test passed. Count: 1");
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_creative_player_breaks_the_stand_at_once_without_drops() {
    let mut game = game();
    with_player(&mut game);
    summon(
        &mut game,
        r#",equipment:{head:{id:"minecraft:diamond_helmet",count:1}}"#,
    );
    assert!(game.run_command("gamemode creative @p"));
    assert!(game.run_command(&format!("damage {STAND} 1 minecraft:player_attack by @p")));
    assert!(!game.run_command(&format!("execute if entity {STAND}")));
    assert_eq!(items(&mut game), "Test failed");
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_player_who_may_not_build_cannot_hurt_the_stand() {
    for mode in ["adventure", "spectator"] {
        let mut game = game();
        with_player(&mut game);
        summon(&mut game, "");
        assert!(game.run_command(&format!("gamemode {mode} @p")));
        for _ in 0..2 {
            assert!(
                !game.run_command(&format!("damage {STAND} 1 minecraft:arrow by @p")),
                "{mode}"
            );
        }
        assert!(
            game.run_command(&format!("execute if entity {STAND}")),
            "{mode}"
        );
        assert_eq!(items(&mut game), "Test failed", "{mode}");
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn control_a_source_in_neither_tag_still_fails_without_a_hurt() {
    let mut game = game();
    summon(&mut game, "");
    assert!(!game.run_command(&format!("damage {STAND} 1 minecraft:generic")));
    assert!(game.run_command(&format!("execute if entity {STAND}")));
}

/// The game events (name, source) posted while `f` runs.
fn events(game: &mut Game<'_>, f: impl FnOnce(&mut Game<'_>)) -> Vec<(&'static str, Option<Who>)> {
    super::sculk::TEST_POSTED_EVENTS.with(|e| *e.borrow_mut() = Some(Vec::new()));
    f(game);
    super::sculk::TEST_POSTED_EVENTS.with(|e| {
        e.borrow_mut()
            .take()
            .unwrap()
            .into_iter()
            .filter(|e| e.0 == "minecraft:entity_damage" || e.0 == "minecraft:entity_die")
            .map(|e| (e.0, e.2))
            .collect()
    })
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_hit_from_damage_posts_entity_damage_then_entity_die_for_its_causing_entity() {
    let mut game = game();
    summon(&mut game, "");
    assert!(game.run_command("summon pig 14.5 100 2.5 {NoAI:1b}"));
    let by_pig = format!("damage {STAND} 1 minecraft:player_attack by @e[type=pig,limit=1]");
    // hurtServer: the first hit gameEvent(ENTITY_DAMAGE, pig); the second,
    // within five ticks, brokenByPlayer then kill: gameEvent(ENTITY_DIE, pig).
    let first = events(&mut game, |g| assert!(g.run_command(&by_pig)));
    assert!(matches!(
        &first[..],
        [("minecraft:entity_damage", Some(Who::Mob(_)))]
    ));
    let second = events(&mut game, |g| assert!(g.run_command(&by_pig)));
    assert!(matches!(
        &second[..],
        [("minecraft:entity_die", Some(Who::Mob(_)))]
    ));
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn an_always_killing_source_posts_only_entity_die_for_a_stand_without_a_cause() {
    let mut game = game();
    summon(&mut game, "");
    // No causing entity: kill attributes the event to the stand itself, which
    // the listeners see as no source. always_kills skips the wobble.
    let posted = events(&mut game, |g| {
        assert!(g.run_command(&format!("damage {STAND} 1 minecraft:arrow")));
    });
    assert!(matches!(&posted[..], [("minecraft:entity_die", None)]));
}
