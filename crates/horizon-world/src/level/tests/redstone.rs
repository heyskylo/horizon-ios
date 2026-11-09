use super::*;

/// `TripWireHookBlock.calculateState`, run when a hook is placed, scans
/// `for i in 1..42` along the hook's facing for a hook facing back, and ends
/// with `attached &= receiverPos > 1`: hooks attach from two to forty-one blocks
/// apart, and never when side by side or forty-two apart.
#[test]
fn tripwire_hooks_attach_from_two_to_forty_one_blocks_apart() {
    for (distance, attached) in [(1, false), (2, true), (3, true), (41, true), (42, false)] {
        let mut w = world!();
        let (stone, string) = (w.id("stone", &[]), w.id("tripwire", &[]));
        // A support behind each hook, and string between them.
        w.set(BlockPos::new(-1, 5, 0), stone);
        w.set(BlockPos::new(distance + 1, 5, 0), stone);
        for x in 1..distance {
            w.set(BlockPos::new(x, 5, 0), string);
        }
        // The first hook faces east (its player looks west, yaw 90); the second,
        // placed last, faces west and finds it.
        let first = w.place(
            "tripwire_hook",
            BlockPos::new(-1, 5, 0),
            Direction::East,
            90.0,
            0.0,
            0.0,
        );
        let second = w.place(
            "tripwire_hook",
            BlockPos::new(distance + 1, 5, 0),
            Direction::West,
            270.0,
            0.0,
            0.0,
        );
        assert!(
            matches!(first, PlaceOutcome::Placed { .. })
                && matches!(second, PlaceOutcome::Placed { .. }),
            "both hooks are placed {distance} apart"
        );
        for _ in 0..4 {
            w.full_tick();
        }
        let (a, b) = (BlockPos::new(0, 5, 0), BlockPos::new(distance, 5, 0));
        assert_eq!(w.prop(a, "facing"), "east", "{distance} apart");
        assert_eq!(w.prop(b, "facing"), "west", "{distance} apart");
        let expected = if attached { "true" } else { "false" };
        assert_eq!(
            w.prop(a, "attached"),
            expected,
            "first hook, {distance} apart"
        );
        assert_eq!(
            w.prop(b, "attached"),
            expected,
            "second hook, {distance} apart"
        );
    }
}

/// `ObserverBlock.updateShape` schedules a tick two ticks out when the block on
/// the side it faces changes; that tick turns the observer on and schedules
/// another two ticks out, which turns it off. A lamp behind it is lit by it
/// (`getSignal` answers 15 only for the observer's facing); one at its front or
/// beside it never is.
#[test]
fn an_observer_pulses_from_two_to_four_ticks_after_its_faced_side_changes() {
    let mut w = world!();
    let (observer, stone, lamp) = (
        w.id("observer", &[("facing", "east")]),
        w.id("stone", &[]),
        w.id("redstone_lamp", &[]),
    );
    let at = BlockPos::new(0, 5, 0);
    // Written raw, so placing it starts no pulse of its own.
    w.put(at, observer);
    let lamps = [
        ("behind", BlockPos::new(-1, 5, 0)),
        ("in front", BlockPos::new(1, 5, 0)),
        ("beside", BlockPos::new(0, 5, 1)),
        ("above", BlockPos::new(0, 6, 0)),
    ];
    for (_, pos) in lamps {
        w.put(pos, lamp);
    }
    // The block in front of the observer changes: the lamp that stood there
    // gives way to stone.
    w.set(BlockPos::new(1, 5, 0), stone);
    let powered = |w: &World| w.prop(at, "powered");
    let lit = |w: &World, pos: BlockPos| w.prop(pos, "lit");
    assert_eq!(powered(&w), "false", "nothing happens at once");
    w.tick();
    assert_eq!(powered(&w), "false", "one tick on");
    w.tick();
    assert_eq!(powered(&w), "true", "two ticks on: the pulse starts");
    assert_eq!(
        lit(&w, BlockPos::new(-1, 5, 0)),
        "true",
        "the lamp behind it lights with it"
    );
    for (side, pos) in lamps {
        if side != "behind" && side != "in front" {
            assert_eq!(lit(&w, pos), "false", "the lamp {side} is not lit");
        }
    }
    w.tick();
    assert_eq!(powered(&w), "true", "three ticks on: still pulsing");
    w.tick();
    assert_eq!(powered(&w), "false", "four ticks on: the pulse is over");
    for _ in 0..6 {
        w.tick();
    }
    assert_eq!(powered(&w), "false", "and it does not start again");
}
