//! Run alone: installs a process-wide trait mod. `/kill` runs the mob's
//! death before the command returns (`LivingEntity.die`), with the native
//! `living_death` veto first: a vetoed death leaves the mob alive, with no
//! event, loot or second call, and a later kill dies once.
use super::kill_die_vibration_tests::{PIG, deaths, game, run};
use super::*;
use crate::mod_api::{self, Flow, Hooks, Host, LivingDeathEvent};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
struct Probe {
    calls: Mutex<Vec<(u32, String)>>,
    veto: AtomicBool,
}
impl Hooks for Arc<Probe> {
    fn living_death(&self, _: &mut dyn Host, e: &mut LivingDeathEvent<'_>) -> Flow {
        let mut calls = self.calls.lock().unwrap();
        calls.push((e.entity, e.damage_type.to_owned()));
        if self.veto.load(Ordering::Relaxed) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }
}

fn porkchops(game: &Game<'_>) -> usize {
    game.dropped
        .iter()
        .filter(|i| &*i.stack.item == "minecraft:porkchop")
        .count()
}

#[test]
#[ignore = "run alone with prepared resources in HORIZON_TEST_RESOURCES"]
fn a_vetoed_kill_keeps_the_mob_and_a_later_kill_dies_once() {
    let probe = Arc::new(Probe::default());
    assert!(
        mod_api::MODS
            .set(Arc::from(vec![Box::new(probe.clone()) as Box<dyn Hooks>]))
            .is_ok(),
        "run this test alone"
    );
    let mut game = game();
    let pig = game
        .mobs
        .iter()
        .find(|m| m.species() == horizon_client::mobs::Species::Pig)
        .unwrap()
        .id;
    let calls = |p: &Probe| std::mem::take(&mut *p.calls.lock().unwrap());
    // Vetoed: the death is asked during the command and refused.
    probe.veto.store(true, Ordering::Relaxed);
    let posted = deaths(&mut game, |g| assert!(run(g, &format!("kill {PIG}")).0));
    assert_eq!(calls(&probe).len(), 1, "the veto is asked during /kill");
    assert_eq!(posted, [], "no event for a vetoed death");
    let m = game.mobs.iter().find(|m| m.id == pig).unwrap();
    assert_eq!((m.health, m.death_time), (1.0, 0), "alive at 1 health");
    assert_eq!(porkchops(&game), 0, "no loot");
    let later = deaths(&mut game, |g| g.tick_creature_entities().unwrap());
    assert_eq!((calls(&probe).len(), later), (0, vec![]), "nothing more");
    assert!(game.mobs.iter().any(|m| m.id == pig && m.alive()));
    // Allowed: one call, one event, one loot drop, and no more next tick.
    probe.veto.store(false, Ordering::Relaxed);
    let posted = deaths(&mut game, |g| assert!(run(g, &format!("kill {PIG}")).0));
    // Its damage type is the one the hook already reads for /kill (separate).
    let allowed = calls(&probe);
    assert_eq!(allowed.iter().map(|c| c.0).collect::<Vec<_>>(), [pig]);
    assert_eq!(posted.len(), 1, "one death event");
    assert_eq!(porkchops(&game), 1, "its loot, during the command");
    let again = deaths(&mut game, |g| assert!(run(g, &format!("kill {PIG}")).0));
    let later = deaths(&mut game, |g| g.tick_creature_entities().unwrap());
    assert_eq!(
        (calls(&probe).len(), again, later, porkchops(&game)),
        (0, vec![], vec![], 1),
        "a second kill and the next tick do not die again"
    );
    let m = game.mobs.iter().find(|m| m.id == pig).unwrap();
    assert!(!m.removed && m.health == 0.0, "the body stays");
}
