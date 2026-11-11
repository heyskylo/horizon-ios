//! Silence mutes only what goes through `Entity.playSound`: the state
//! transitions' and the seed drop's sounds, not the client's local sounds
//! (`Sniffer.playSearchingSound`, the dig's hit sound).
use super::super::animal_brain::{Bx, transition};
use super::super::world::with_test_world;
use super::*;
use horizon_core::{geometry::Vec3, random::LegacyRandom};
use std::sync::Arc;

fn sniffer(types: &super::super::MobTypes) -> Mob {
    Mob::new(
        42,
        Arc::new(types.types["minecraft:sniffer"].clone()),
        Vec3::new(8.5, 100.0, 8.5),
        0.0,
    )
}

#[test]
fn silent_sniffer_mutes_the_state_and_seed_sounds() {
    let ran = with_test_world(&[], |w, types| {
        let states = [
            (FEELING_HAPPY, "entity.sniffer.happy"),
            (SCENTING, "entity.sniffer.scenting"),
            (SNIFFING, "entity.sniffer.sniffing"),
            (RISING, "entity.sniffer.digging_stop"),
        ];
        for (state, name) in states {
            for silent in [true, false] {
                let mut m = sniffer(types);
                m.silent = silent;
                let mut lr = LegacyRandom::new(1);
                let mut events = Vec::new();
                {
                    let mut cx = Bx {
                        w,
                        lr: &mut lr,
                        events: &mut events,
                        now: 0,
                    };
                    transition(&mut m, &mut cx, state);
                }
                let sound = events
                    .iter()
                    .any(|e| matches!(e, MobEvent::Sound(n, ..) if *n == name));
                assert_eq!(sound, !silent, "state {state}, silent={silent}: {events:?}");
            }
        }
        // onDiggingStart has no sound, so the drop happens first at age 120.
        for silent in [true, false] {
            let mut m = sniffer(types);
            m.silent = silent;
            set_state(&mut m, DIGGING);
            m.age = 120;
            m.extra.ints[3] = 120;
            let mut events = Vec::new();
            ai_step(&mut m, &mut events);
            assert!(
                events
                    .iter()
                    .any(|e| matches!(e, MobEvent::GiftLoot { .. })),
                "silent={silent}: {events:?}"
            );
            let drop_sound = events
                .iter()
                .any(|e| matches!(e, MobEvent::Sound(n, ..) if *n == "entity.sniffer.drop_seed"));
            assert_eq!(
                drop_sound, !silent,
                "seed drop, silent={silent}: {events:?}"
            );
        }
    });
    let _ = ran;
}
