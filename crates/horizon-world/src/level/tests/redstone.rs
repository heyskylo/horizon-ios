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
