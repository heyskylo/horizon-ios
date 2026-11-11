//! `Zombie.doHurtTarget`'s burn, through the game's mob event handling with
//! real resources: it rides a blow that hurt, on the local difficulty's chance.
use super::*;
use crate::game::*;
use horizon_client::mobs::{ai::do_hurt_target, goals::Cx};
use horizon_core::random::{EntityRandom, LegacyRandom};
use horizon_world::food::Difficulty;

fn with_game(test: impl FnOnce(&mut Game<'_>)) {
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
    let mut g = Game::new(
        &loaded.resources,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(&root),
        None,
    )
    .unwrap();
    g.options.dedicated = Some(Vec::new());
    g.world = World::new(g.res.facts().air, 1);
    test(&mut g);
}

/// One blow: a mob of `kind` holding `hand`, burning or not, its own random
/// seeded `seed`, on the level's `difficulty`; `landed` false puts the player
/// inside the hurt cooldown, where `hurtServer` refuses the blow
/// (`damageCooldownTime > 10` and no harder than the last hit).
type Case = (
    &'static str,
    Option<&'static str>,
    bool,
    i64,
    bool,
    Difficulty,
);

/// What each blow did: the player's fire ticks afterwards (`-20`, where a
/// player starts, when nothing set it alight), and the mob's next random float.
fn blows(cases: &[Case]) -> Vec<(i32, f32)> {
    let mut out = Vec::new();
    with_game(|g| {
        for &(kind, hand, burning, seed, landed, difficulty) in cases {
            g.world.health_rules.difficulty = difficulty;
            g.mobs.clear();
            // A whole player each blow: the earlier ones took health away.
            g.player = Some(PlayerMotion::new(g.motion, Vec3::new(0.5, 64.0, 0.5)).unwrap());
            let p = g.player.as_mut().unwrap();
            p.entity_id = 77;
            if !landed {
                p.health.hurt.cooldown = 20;
                p.health.hurt.last_hurt = 100.0;
            }
            let type_ = g.res.mob_types().get(kind).unwrap().clone();
            let mut m = Mob::new(41, type_, Vec3::new(2.5, 64.0, 0.5), 0.0);
            m.fire_ticks = if burning { 100 } else { 0 };
            m.extra.main_hand = hand.map(Arc::from);
            m.random = EntityRandom::own(LegacyRandom::new(seed));
            g.mobs.push(m);
            // The mob out of the level while its own blow runs, as in its tick.
            let mut mobs = std::mem::take(&mut g.mobs);
            let mut lr = LegacyRandom::new(0);
            let mut events = Vec::new();
            {
                let w = g.mob_world_base(&[]);
                let mut cx = Cx {
                    w: &w,
                    lr: &mut lr,
                    events: &mut events,
                };
                do_hurt_target(&mut mobs[0], Who::Player, &mut cx);
            }
            g.mobs = mobs;
            g.handle_mob_events(events).unwrap();
            out.push((
                g.player.as_ref().unwrap().fire_ticks,
                g.mobs[0].random.next_float(),
            ));
        }
    });
    out
}

/// `Zombie.doHurtTarget`: a burning zombie with an empty main hand sets the
/// target of a blow that landed alight, for `2 * (int)effectiveDifficulty`
/// seconds, on `effectiveDifficulty * 0.3` of those blows. Every species that
/// extends `Zombie` has it, but a fire-immune one is never on fire. The level's effective difficulty is 1.5 on normal
/// and 2.25 on hard here (no chunk is inhabited and the clock is at zero), so
/// the chances are 0.45 and 0.675. java.util.Random's first nextFloat is
/// 0.0978928804397583 for seed 4096, 0.6392364501953125 for 1024 and
/// 0.7309677600860596 for 0, so each expected pair below is that seed's first
/// float where the chance was not drawn and its second where it was.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_burning_zombies_burn_rides_a_blow_that_landed() {
    let (normal, hard) = (Difficulty::Normal, Difficulty::Hard);
    let zombie = "minecraft:zombie";
    let cases: [Case; 13] = [
        // 1.5 * 0.3 > 0.098: 2 * (int)1.5 s of fire, 2 * (int)2.25 s on hard.
        (zombie, None, true, 4096, true, normal),
        (zombie, None, true, 4096, true, hard),
        // 1.5 * 0.3 < 0.639 < 2.25 * 0.3: the same draw misses on normal and
        // burns on hard.
        (zombie, None, true, 1024, true, normal),
        (zombie, None, true, 1024, true, hard),
        // Every zombie has the burn, bare handed.
        ("minecraft:husk", None, true, 4096, true, normal),
        ("minecraft:drowned", None, true, 4096, true, normal),
        ("minecraft:zombie_villager", None, true, 4096, true, normal),
        // But a zombified piglin is fire immune, so never `isOnFire`
        // (`Entity.isOnFire`), whatever its fire ticks: no burn, no draw.
        ("minecraft:zombified_piglin", None, true, 4096, true, normal),
        // A skeleton, a weapon in the main hand and a zombie that is not on
        // fire: nothing, and the chance is not drawn.
        ("minecraft:skeleton", None, true, 4096, true, normal),
        (
            zombie,
            Some("minecraft:iron_sword"),
            true,
            4096,
            true,
            normal,
        ),
        (zombie, None, false, 4096, true, normal),
        // 0.731 is past both chances: the chance was drawn, nothing burned.
        (zombie, None, true, 0, true, hard),
        // A blow that hurt nobody (the hurt cooldown refuses it) sets nothing
        // alight. Horizon still draws the chance here, where vanilla does not
        // (the blow resolves after `do_hurt_target`): a known gap.
        (zombie, None, true, 4096, false, normal),
    ];
    let expected = [
        (40, 0.875_477_85),
        (80, 0.875_477_85),
        (-20, 0.320_431_77),
        (80, 0.320_431_77),
        (40, 0.875_477_85),
        (40, 0.875_477_85),
        (40, 0.875_477_85),
        (-20, 0.097_892_88),
        (-20, 0.097_892_88),
        (-20, 0.097_892_88),
        (-20, 0.097_892_88),
        (-20, 0.831_441),
        (-20, 0.875_477_85),
    ];
    assert_eq!(blows(&cases), expected);
}
