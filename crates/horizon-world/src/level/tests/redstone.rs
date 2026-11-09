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

/// `ObserverBlock.updateShape` reacts only when the direction to the changed
/// neighbour is its `FACING`: a change on any other side starts no pulse.
#[test]
fn an_observer_ignores_changes_on_the_sides_it_does_not_face() {
    for (side, offset) in [
        ("behind", (-1, 0, 0)),
        ("north", (0, 0, -1)),
        ("south", (0, 0, 1)),
        ("above", (0, 1, 0)),
        ("below", (0, -1, 0)),
    ] {
        let mut w = world!();
        let at = BlockPos::new(0, 5, 0);
        w.put(at, w.id("observer", &[("facing", "east")]));
        let stone = w.id("stone", &[]);
        w.set(BlockPos::new(offset.0, 5 + offset.1, offset.2), stone);
        for _ in 0..8 {
            w.tick();
            assert_eq!(w.prop(at, "powered"), "false", "a change {side} of it");
        }
    }
}

/// `ButtonBlock.press` powers the button and schedules a tick `ticksToStayPressed`
/// out; the tick releases it. Stone buttons stay pressed 20 ticks and every wooden
/// one 30, bamboo and cherry included. A button that is already pressed answers a
/// second click with nothing: the timer is not extended.
#[test]
fn buttons_stay_pressed_for_their_own_ticks_and_a_second_click_does_not_extend_it() {
    for (name, stays) in [
        ("stone_button", 20),
        ("oak_button", 30),
        ("bamboo_button", 30),
        ("cherry_button", 30),
    ] {
        let mut w = world!();
        w.floor(0, 5, "stone");
        let pos = BlockPos::new(0, 1, 0);
        w.set(pos, w.id(name, &[("face", "floor"), ("facing", "north")]));
        assert_eq!(w.prop(pos, "powered"), "false", "{name} starts released");
        w.use_block(pos);
        assert_eq!(w.prop(pos, "powered"), "true", "{name} is pressed");
        for tick in 1..stays {
            w.tick();
            assert_eq!(w.prop(pos, "powered"), "true", "{name} after {tick} ticks");
        }
        w.tick();
        assert_eq!(
            w.prop(pos, "powered"),
            "false",
            "{name} is released after {stays} ticks"
        );
    }

    // A second click half way through changes nothing about when it releases.
    let mut w = world!();
    w.floor(0, 5, "stone");
    let pos = BlockPos::new(0, 1, 0);
    w.set(
        pos,
        w.id("stone_button", &[("face", "floor"), ("facing", "north")]),
    );
    w.use_block(pos);
    for _ in 0..10 {
        w.tick();
    }
    w.use_block(pos);
    for _ in 0..9 {
        w.tick();
    }
    assert_eq!(
        w.prop(pos, "powered"),
        "true",
        "19 ticks after the first click"
    );
    w.tick();
    assert_eq!(
        w.prop(pos, "powered"),
        "false",
        "20 ticks after the first click, not after the second"
    );
}

/// A lever at `start` and a line of wires after it along `step`: the last wire
/// carries `power`, since a wire gives up one level a block (a lever's own wire
/// has 15). Returns the position of that last wire. Levers are switched on by the
/// caller once everything stands, so nothing settles half built.
fn signal_line(
    w: &mut World,
    start: BlockPos,
    step: (i32, i32),
    power: i32,
) -> (BlockPos, BlockPos) {
    let lever = w.id("lever", &[("face", "floor"), ("facing", "north")]);
    let wire = w.id("redstone_wire", &[]);
    w.set(start, lever);
    let mut last = start;
    for n in 1..=(16 - power) {
        last = BlockPos::new(start.x + step.0 * n, start.y, start.z + step.1 * n);
        w.set(last, wire);
    }
    (start, last)
}

fn switch_on(w: &mut World, lever: BlockPos) {
    let on = w
        .b()
        .table
        .set_bool(w.get(lever), w.b().props.powered, true);
    w.set(lever, on);
}

/// `ComparatorBlock.calculateOutputSignal`: no input is no output; a side input
/// above the input silences it; otherwise COMPARE passes the input through and
/// SUBTRACT outputs `input - side`. `shouldTurnOn` (the `powered` state) agrees,
/// with one difference: equal inputs turn a COMPARE comparator on and a SUBTRACT
/// one off. Read from the wire in front of it, which takes the output as its own power.
#[test]
fn a_comparator_compares_and_subtracts_by_vanillas_rules() {
    // (input, side, mode, the output signal, the comparator's `powered`)
    let cases = [
        (15, 0, "compare", 15, true),
        (10, 4, "compare", 10, true),
        (10, 10, "compare", 10, true),
        (10, 11, "compare", 0, false),
        (10, 4, "subtract", 6, true),
        (10, 10, "subtract", 0, false),
        (10, 11, "subtract", 0, false),
        (7, 0, "subtract", 7, true),
        (0, 5, "compare", 0, false),
    ];
    for (input, side, mode, output, powered) in cases {
        let mut w = world!();
        w.floor(0, 24, "stone");
        // The comparator faces west: it reads the wire at its west and outputs east.
        let comparator_x = if input > 0 { 17 - input } else { 3 };
        let at = BlockPos::new(comparator_x, 1, 0);
        let mut levers = Vec::new();
        if input > 0 {
            levers.push(signal_line(&mut w, BlockPos::new(0, 1, 0), (1, 0), input).0);
        }
        if side > 0 {
            // From the south side (z = 1), ending beside the comparator.
            let start = BlockPos::new(comparator_x, 1, 1 + (16 - side));
            levers.push(signal_line(&mut w, start, (0, -1), side).0);
        }
        w.set(
            at,
            w.id("comparator", &[("facing", "west"), ("mode", mode)]),
        );
        let readout = BlockPos::new(comparator_x + 1, 1, 0);
        w.set(readout, w.id("redstone_wire", &[]));
        for lever in levers {
            switch_on(&mut w, lever);
        }
        for _ in 0..10 {
            w.full_tick();
        }
        let case = format!("input {input}, side {side}, {mode}");
        assert_eq!(
            w.prop(readout, "power"),
            output.to_string(),
            "{case}: output"
        );
        assert_eq!(
            w.prop(at, "powered"),
            if powered { "true" } else { "false" },
            "{case}: powered"
        );
    }
}
/// `CopperBulbBlock.checkAndFlip`: a bulb flips `LIT` only when it becomes powered,
/// not when it loses power, and `POWERED` follows the signal either way. So the
/// light alternates on every power-up: on, stays on through the power-down, off, stays
/// off through the next, on again. Every oxidation and waxed variant does the same.
#[test]
fn a_copper_bulb_flips_its_light_only_when_it_becomes_powered() {
    for name in [
        "copper_bulb",
        "exposed_copper_bulb",
        "weathered_copper_bulb",
        "oxidized_copper_bulb",
        "waxed_copper_bulb",
    ] {
        let mut w = world!();
        let (bulb, power, air) = (w.id(name, &[]), w.id("redstone_block", &[]), w.b().air());
        let (at, source) = (BlockPos::new(0, 5, 0), BlockPos::new(1, 5, 0));
        w.set(at, bulb);
        let state = |w: &World| (w.prop(at, "lit"), w.prop(at, "powered"));
        assert_eq!(
            state(&w),
            ("false".into(), "false".into()),
            "{name} placed unpowered"
        );
        // (what the redstone block does, the light and the power the bulb should show)
        let steps = [
            (true, "true", "true"),
            (false, "true", "false"),
            (true, "false", "true"),
            (false, "false", "false"),
            (true, "true", "true"),
        ];
        for (powered, lit, follows) in steps {
            w.set(source, if powered { power } else { air });
            assert_eq!(
                state(&w),
                (lit.to_string(), follows.to_string()),
                "{name}, power {}",
                if powered { "on" } else { "off" }
            );
        }
    }

    // Placed beside power, `onPlace` runs the same check: it lights at once.
    let mut w = world!();
    w.set(BlockPos::new(1, 5, 0), w.id("redstone_block", &[]));
    let at = BlockPos::new(0, 5, 0);
    w.set(at, w.id("copper_bulb", &[]));
    assert_eq!(w.prop(at, "lit"), "true", "placed beside power");
    assert_eq!(w.prop(at, "powered"), "true");
}
