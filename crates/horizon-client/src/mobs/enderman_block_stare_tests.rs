//! Endermen against vanilla 26.3: the carried block's occupancy check
//! (`EndermanLeaveBlockGoal.canPlaceBlock`), the take-block reach
//! (`EndermanTakeBlockGoal.tick`), the stare sound (`playStareSound` from
//! `onSyncedDataUpdated`) and `targetChangeTime` (`setTarget`).
use super::*;
use crate::mobs::world::{with_built_test_world, with_test_world};
use horizon_core::random::{EntityRandom, LegacyRandom};
use std::sync::Arc;

/// The vanilla enderman type, from the extracted data.
fn enderman_kind() -> Option<Arc<crate::mobs::MobType>> {
    let mut kind = None;
    with_test_world(&[], |_, types| {
        kind = types.types.get("minecraft:enderman").cloned().map(Arc::new);
    });
    kind
}

/// Runs `goal.tick` with the first mob seed whose three draws pick `cube`,
/// as `pick` maps them; the events it raised and the mob after.
fn tick_at(
    m: &mut Mob,
    w: &MobWorld,
    cube: [i32; 3],
    pick: impl Fn(&Mob, [f64; 3]) -> [i32; 3],
    goal: &mut dyn Goal,
) -> Vec<MobEvent> {
    let seed = (0..100_000)
        .find(|&seed| {
            let mut r = LegacyRandom::new(seed);
            pick(m, [r.next_double(), r.next_double(), r.next_double()]) == cube
        })
        .expect("a seed picks the cube");
    m.random = EntityRandom::own(LegacyRandom::new(seed));
    let mut events = Vec::new();
    let mut lr = LegacyRandom::new(0);
    goal.tick(
        m,
        &mut Cx {
            w,
            lr: &mut lr,
            events: &mut events,
        },
    );
    events
}

/// `level.getEntities(this.enderman, cube)`: the enderman's own box never
/// blocks its feet cube, another mob's does.
#[test]
fn the_enderman_itself_never_blocks_a_placement() {
    let Some(kind) = enderman_kind() else { return };
    let at = Vec3::new(0.5, 0.0, 0.5);
    let mut m = Mob::new(1, kind.clone(), at, 0.0);
    m.extra.carried = Some(BlockStateId(1));
    let other = Mob::new(2, kind, at, 0.0);
    // EndermanLeaveBlockGoal.tick: x - 1 + 2d, y + 2d, z - 1 + 2d.
    let pick = |m: &Mob, d: [f64; 3]| {
        [
            (m.position.x - 1.0 + d[0] * 2.0).floor() as i32,
            (m.position.y + d[1] * 2.0).floor() as i32,
            (m.position.z - 1.0 + d[2] * 2.0).floor() as i32,
        ]
    };
    for (others, blocked) in [
        (vec![m.seen(None)], false),
        (vec![m.seen(None), other.seen(None)], true),
    ] {
        with_test_world(&others, |w, _| {
            let events = tick_at(&mut m, w, [0, 0, 0], pick, &mut LeaveBlock);
            let occupants = events.iter().find_map(|e| match e {
                MobEvent::EndermanPlaceBlock { pos, occupants, .. } => {
                    assert_eq!(*pos, [0, 0, 0]);
                    Some(occupants.clone())
                }
                _ => None,
            });
            assert_eq!(occupants.map(|o| !o.is_empty()), Some(blocked), "{blocked}");
        });
    }
}

/// `EndermanTakeBlockGoal.tick` clips with OUTLINE shapes from the
/// enderman's column: a dandelion or a torch, which have no collision,
/// hide a pumpkin behind them; a dandelion whose random offset moves it off
/// the line does not.
#[test]
fn take_block_reach_stops_at_outline_shapes() {
    let Some(kind) = enderman_kind() else { return };
    // EndermanTakeBlockGoal.tick: x - 2 + 4d, y + 3d, z - 2 + 4d.
    let pick = |m: &Mob, d: [f64; 3]| {
        [
            (m.position.x - 2.0 + d[0] * 4.0).floor() as i32,
            (m.position.y + d[1] * 3.0).floor() as i32,
            (m.position.z - 2.0 + d[2] * 4.0).floor() as i32,
        ]
    };
    // (enderman column, pumpkin, block between, taken). Mth.getSeed(1, 0, 0)
    // puts the dandelion at z offset -0.017, on the line z = 0.5; (2, 0, -1)
    // at x offset -0.25, so its box ends at x = 0.4375, off the line x = 2.5.
    let cases = [
        ([0, 0], [2, 0, 0], None, true),
        ([0, 0], [2, 0, 0], Some(("dandelion", [1, 0, 0])), false),
        ([0, 0], [2, 0, 0], Some(("torch", [1, 0, 0])), false),
        ([0, 0], [2, 0, 0], Some(("stone", [1, 0, 0])), false),
        ([2, 0], [2, 0, -2], Some(("dandelion", [2, 0, -1])), true),
    ];
    for (column, pumpkin, between, taken) in cases {
        let build = |world: &mut crate::world::World, registry: &BlockRegistry| {
            for cx in -1..1 {
                for cz in -1..1 {
                    world
                        .chunks
                        .insert((cx, cz), Arc::new(crate::chunk::Chunk::empty(cx, cz)));
                }
            }
            let grass = registry.default_state("minecraft:grass_block").unwrap();
            for x in -4..5 {
                for z in -4..5 {
                    world.set(x, -1, z, grass).unwrap();
                }
            }
            let state = |name: &str| {
                registry
                    .default_state(&format!("minecraft:{name}"))
                    .unwrap()
            };
            world
                .set(pumpkin[0], pumpkin[1], pumpkin[2], state("pumpkin"))
                .unwrap();
            if let Some((name, at)) = between {
                world.set(at[0], at[1], at[2], state(name)).unwrap();
            }
        };
        with_built_test_world(&[], build, |w, _| {
            let at = Vec3::new(f64::from(column[0]) + 0.5, 0.0, f64::from(column[1]) + 0.5);
            let mut m = Mob::new(1, kind.clone(), at, 0.0);
            tick_at(&mut m, w, pumpkin, pick, &mut TakeBlock);
            assert_eq!(m.extra.carried.is_some(), taken, "{between:?}");
        });
    }
}

/// Plays one `setTarget` the way the goals do, then the AI step's end.
fn step(
    m: &mut Mob,
    w: &MobWorld,
    age: u32,
    change: impl FnOnce(&mut Mob, &mut Cx),
) -> Vec<MobEvent> {
    m.age = age;
    let mut events = Vec::new();
    let mut lr = LegacyRandom::new(0);
    let mut cx = Cx {
        w,
        lr: &mut lr,
        events: &mut events,
    };
    change(m, &mut cx);
    custom_server_ai_step(m, &mut cx);
    events
        .into_iter()
        .filter(|e| matches!(e, MobEvent::Sound("entity.enderman.stare", ..)))
        .collect()
}

fn look() -> LookForPlayer {
    LookForPlayer {
        pending: false,
        aggro: 0,
        teleport: 0,
    }
}
/// A stare (`setBeingStaredAt`) that turns into anger when the goal's
/// `aggroTime` runs out.
fn stare_anger(m: &mut Mob, cx: &mut Cx) {
    let mut goal = look();
    goal.start(m, cx);
    goal.pending = true;
    goal.aggro = 1;
    goal.tick(m, cx);
    assert_eq!(m.ai.target, Some(Who::Player));
}
/// `TargetGoal.stop`: `setTarget(null)`.
fn calm(m: &mut Mob, cx: &mut Cx) {
    look().stop(m, cx);
}

/// `playStareSound`: once in 400 ticks, at the eyes (2.55 up), and never
/// from a silent enderman, which still uses up the 400 ticks. The client
/// hears `DATA_CREEPY` change before `DATA_STARED_AT` clears, so the end of
/// a stare's anger plays it too; anger from a hit does not.
#[test]
fn the_stare_sound_is_throttled_silenced_and_at_the_eyes() {
    let Some(kind) = enderman_kind() else { return };
    with_test_world(&[], |w, _| {
        let mut w = w.clone();
        w.player = Some(crate::mobs::world::test_player(Vec3::new(0.5, 10.0, 30.5)));
        let w = &w;
        let mut m = Mob::new(1, kind.clone(), Vec3::new(0.5, 10.0, 0.5), 0.0);
        let sounds = step(&mut m, w, 100, stare_anger);
        let [MobEvent::Sound(_, at, volume, pitch)] = sounds.as_slice() else {
            panic!("one stare sound: {sounds:?}");
        };
        // getEyeY: getY() + the float eye height 2.55 (EntityTypes.ENDERMAN).
        assert_eq!(*at, [0.5, 10.0 + f64::from(2.55f32), 0.5]);
        assert_eq!((*volume, *pitch), (2.5, 1.0));
        assert!(step(&mut m, w, 150, calm).is_empty(), "within 400 ticks");
        assert!(
            step(&mut m, w, 200, stare_anger).is_empty(),
            "within 400 ticks"
        );
        assert_eq!(step(&mut m, w, 520, calm).len(), 1, "the end of the anger");
        assert!(
            step(&mut m, w, 560, stare_anger).is_empty(),
            "within 400 ticks"
        );

        let mut hit = Mob::new(2, kind.clone(), Vec3::ZERO, 0.0);
        let sounds = step(&mut hit, w, 100, |m, _| m.ai.target = Some(Who::Player));
        assert!(sounds.is_empty(), "no stare");
        assert!(step(&mut hit, w, 150, calm).is_empty(), "no stare");

        let mut silent = Mob::new(3, kind.clone(), Vec3::ZERO, 0.0);
        silent.silent = true;
        assert!(step(&mut silent, w, 100, stare_anger).is_empty(), "Silent");
        step(&mut silent, w, 150, calm);
        silent.silent = false;
        assert!(
            step(&mut silent, w, 200, stare_anger).is_empty(),
            "the silent one stamped it"
        );
        step(&mut silent, w, 250, calm);
        assert_eq!(step(&mut silent, w, 500, stare_anger).len(), 1);
    });
}

/// `setTarget` stamps `targetChangeTime` for any target, so an enderman a
/// hit angers waits 600 ticks before its daylight teleport, as one a stare
/// angers does; `setTarget(null)` zeroes it.
#[test]
fn every_target_stamps_the_target_change_time() {
    let Some(kind) = enderman_kind() else { return };
    with_test_world(&[], |w, _| {
        let mut hit = Mob::new(1, kind.clone(), Vec3::ZERO, 0.0);
        step(&mut hit, w, 300, |m, _| m.ai.target = Some(Who::Mob(7)));
        assert_eq!(hit.extra.counter, 300);
        let mut stared = Mob::new(2, kind.clone(), Vec3::ZERO, 0.0);
        step(&mut stared, w, 290, |m, cx| look().start(m, cx));
        assert_eq!(stared.extra.counter, 0, "a stare alone sets no target");
        step(&mut stared, w, 300, |m, _| m.ai.target = Some(Who::Player));
        assert_eq!(stared.extra.counter, 300);
        step(&mut stared, w, 400, calm);
        assert_eq!(stared.extra.counter, 0);
    });
}
