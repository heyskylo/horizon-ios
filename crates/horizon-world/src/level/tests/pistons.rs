use super::*;

#[test]
fn pistons_pop_fragile_blocks_and_refuse_obsidian() {
    let mut w = world!();
    w.floor(0, 10, "stone");
    let piston = w.id("piston", &[("facing", "east")]);
    let block = w.id("redstone_block", &[]);
    let torch = w.id("torch", &[]);
    let obsidian = w.id("obsidian", &[]);
    w.set(BlockPos::new(0, 1, 0), piston);
    w.set(BlockPos::new(1, 1, 0), torch);
    w.set(BlockPos::new(-1, 1, 0), block);
    for _ in 0..4 {
        w.full_tick();
    }
    assert_eq!(
        w.name(BlockPos::new(1, 1, 0)),
        "piston_head[facing=east,short=false,type=normal]"
    );
    assert!(
        w.level
            .events
            .iter()
            .any(|e| matches!(e, LevelEvent::DropResources { state, .. } if *state == torch))
    );
    let mut w = world!();
    w.floor(0, 10, "stone");
    w.set(BlockPos::new(0, 1, 0), piston);
    w.set(BlockPos::new(1, 1, 0), obsidian);
    w.set(BlockPos::new(-1, 1, 0), block);
    for _ in 0..4 {
        w.full_tick();
    }
    assert_eq!(w.prop(BlockPos::new(0, 1, 0), "extended"), "false");
    assert_eq!(w.get(BlockPos::new(1, 1, 0)), obsidian);
}

/// `MovingPistonBlock.getDrops`: a broken moving piston drops the block it is
/// moving, and drops nothing at all once its moving-piston block entity is
/// gone.
#[test]
fn a_broken_moving_piston_drops_the_block_it_carries() {
    let mut w = world!();
    let moving = w.id("moving_piston", &[("facing", "east"), ("type", "normal")]);
    let sand = w.id("sand", &[]);
    let carried = BlockPos::new(0, 1, 0);
    let orphan = BlockPos::new(5, 1, 5);
    for pos in [carried, orphan] {
        w.put(pos, moving);
    }
    w.ticks.moving_pistons.push((
        carried,
        MovingPiston {
            moved_state: sand,
            direction: Direction::East,
            extending: true,
            source: false,
            progress: 0.5,
            progress_o: 0.4,
            last_ticked: 0,
            death_ticks: 0,
            id: 0,
        },
    ));
    w.level.events.clear();
    for pos in [carried, orphan] {
        w.b().destroy_block(&mut w.level, &mut w.ticks, pos, true);
    }
    let drops: Vec<(BlockPos, BlockStateId)> = w
        .level
        .events
        .iter()
        .filter_map(|e| match e {
            LevelEvent::DropResources { pos, state } => Some((*pos, *state)),
            _ => None,
        })
        .collect();
    assert_eq!(
        drops,
        vec![(carried, sand)],
        "the moving piston drops the sand it carries; without its block entity there is nothing to drop"
    );
}

/// `MovingPistonBlock.useWithoutItem`: a moving piston with no block entity (its
/// move is not in flight) is removed by the click, and one that is still moving
/// keeps its block and passes the click on.
#[test]
fn a_moving_piston_without_a_block_entity_is_removed_by_a_click() {
    let mut w = world!();
    let moving = w.id("moving_piston", &[("facing", "east"), ("type", "normal")]);
    let orphan = BlockPos::new(0, 1, 0);
    let in_flight = BlockPos::new(4, 1, 4);
    let air = w.id("air", &[]);
    for pos in [orphan, in_flight] {
        w.put(pos, moving);
    }
    w.ticks.moving_pistons.push((
        in_flight,
        MovingPiston {
            moved_state: w.id("sand", &[]),
            direction: Direction::East,
            extending: true,
            source: false,
            progress: 0.5,
            progress_o: 0.4,
            last_ticked: 0,
            death_ticks: 0,
            id: 0,
        },
    ));
    assert_eq!(w.use_block(orphan), InteractionResult::Consume);
    assert_eq!(w.get(orphan), air, "the orphan moving piston is removed");
    assert_eq!(w.use_block(in_flight), InteractionResult::Pass);
    assert_eq!(
        w.get(in_flight),
        moving,
        "a moving piston in flight keeps its block"
    );
}

/// `MovingPistonBlock.destroy`: a moving piston a player breaks takes the
/// extended piston base behind it with it; an unextended base stays.
#[test]
fn breaking_a_moving_piston_removes_its_extended_base() {
    let mut w = world!();
    let moving = w.id("moving_piston", &[("facing", "east"), ("type", "normal")]);
    let extended = w.id("piston", &[("facing", "east"), ("extended", "true")]);
    let retracted = w.id("piston", &[("facing", "east"), ("extended", "false")]);
    let air = w.id("air", &[]);
    // The head at (1, 1, 0) faces east, so its base is at (0, 1, 0).
    w.put(BlockPos::new(0, 1, 0), extended);
    w.put(BlockPos::new(1, 1, 0), moving);
    w.f.behaviours.player_destroy_block(
        &mut w.level,
        &mut w.ticks,
        BlockPos::new(1, 1, 0),
        false,
        true,
    );
    assert_eq!(
        w.get(BlockPos::new(0, 1, 0)),
        air,
        "the extended base goes with the moving piston"
    );
    w.put(BlockPos::new(5, 1, 0), retracted);
    w.put(BlockPos::new(6, 1, 0), moving);
    w.f.behaviours.player_destroy_block(
        &mut w.level,
        &mut w.ticks,
        BlockPos::new(6, 1, 0),
        false,
        true,
    );
    assert_eq!(
        w.get(BlockPos::new(5, 1, 0)),
        retracted,
        "an unextended base is left alone"
    );
}

/// `LevelChunk.setBlockState` over a moving piston only runs the block
/// entity's `finalTick`: `MovingPistonBlock.destroy`, which removes the
/// extended base, is `ServerPlayerGameMode.destroyBlock`'s alone. `/setblock`,
/// `/fill` and `Level.destroyBlock` over the head leave a headless piston.
#[test]
fn replacing_a_moving_piston_leaves_its_extended_base() {
    let mut w = world!();
    let moving = w.id("moving_piston", &[("facing", "east"), ("type", "normal")]);
    let extended = w.id("piston", &[("facing", "east"), ("extended", "true")]);
    let air = w.id("air", &[]);
    // /setblock ~ ~ ~ air on the head: the extended base stays.
    w.put(BlockPos::new(0, 1, 0), extended);
    w.put(BlockPos::new(1, 1, 0), moving);
    w.set(BlockPos::new(1, 1, 0), air);
    assert_eq!(w.get(BlockPos::new(1, 1, 0)), air);
    assert_eq!(
        w.get(BlockPos::new(0, 1, 0)),
        extended,
        "a setBlock over the head leaves a headless extended piston"
    );
    // Level.destroyBlock (explosions, decay) is not a player break either.
    w.put(BlockPos::new(4, 1, 0), extended);
    w.put(BlockPos::new(5, 1, 0), moving);
    w.b()
        .destroy_block(&mut w.level, &mut w.ticks, BlockPos::new(5, 1, 0), false);
    assert_eq!(
        w.get(BlockPos::new(4, 1, 0)),
        extended,
        "destroyBlock over the head leaves a headless extended piston"
    );
}

/// Runs the scheduled ticks and block events a piston's move needs: the event
/// fires on the tick after the neighbour change, and the head is placed when it
/// runs.
fn settle(w: &mut World) {
    for _ in 0..8 {
        w.full_tick();
    }
}

/// `PistonStructureResolver.addBlockLine` refuses once `toPush` would hold more
/// than 12 blocks: a line of twelve moves, and a thirteenth makes the whole
/// push fail (the piston stays retracted), not move twelve of them.
#[test]
fn a_piston_moves_twelve_blocks_and_refuses_a_thirteenth() {
    for (count, moves) in [(11, true), (12, true), (13, false)] {
        let mut w = world!();
        let piston = w.id("piston", &[("facing", "east")]);
        let dirt = w.id("dirt", &[]);
        let air = w.b().air();
        for x in 1..=count {
            w.set(BlockPos::new(x, 5, 0), dirt);
        }
        w.set(BlockPos::new(0, 5, 0), piston);
        w.set(BlockPos::new(-1, 5, 0), w.id("redstone_block", &[]));
        settle(&mut w);
        let origin = BlockPos::new(0, 5, 0);
        assert_eq!(
            w.prop(origin, "extended"),
            if moves { "true" } else { "false" },
            "{count} blocks in a line"
        );
        if moves {
            assert!(
                w.name(BlockPos::new(1, 5, 0)).starts_with("piston_head"),
                "{count} blocks: the head stands where the first block was"
            );
            for x in 2..=count + 1 {
                assert_eq!(
                    w.get(BlockPos::new(x, 5, 0)),
                    dirt,
                    "{count} blocks: every one moved a block along, to x = {x}"
                );
            }
            assert_eq!(w.get(BlockPos::new(count + 2, 5, 0)), air);
        } else {
            for x in 1..=count {
                assert_eq!(
                    w.get(BlockPos::new(x, 5, 0)),
                    dirt,
                    "{count} blocks: a refused push moves none of them"
                );
            }
        }
    }
}

/// `PistonStructureResolver.canStickToEachOther`: a slime block drags slime and
/// a honey block drags honey, but slime and honey never stick to each other.
/// Built in open air: a slime block on stone drags the floor too, which is
/// vanilla's as well but would make the check about something else.
#[test]
fn slime_and_honey_each_drag_their_own_kind_and_never_each_other() {
    // (the block the piston faces, the block at its side, is the side one dragged)
    for (front, side, dragged) in [
        ("slime_block", "slime_block", true),
        ("honey_block", "honey_block", true),
        ("slime_block", "honey_block", false),
        ("honey_block", "slime_block", false),
    ] {
        let mut w = world!();
        let (front_id, side_id) = (w.id(front, &[]), w.id(side, &[]));
        let air = w.b().air();
        let (ahead, beside) = (BlockPos::new(1, 5, 0), BlockPos::new(1, 5, 1));
        w.set(ahead, front_id);
        w.set(beside, side_id);
        w.set(
            BlockPos::new(0, 5, 0),
            w.id("piston", &[("facing", "east")]),
        );
        w.set(BlockPos::new(-1, 5, 0), w.id("redstone_block", &[]));
        settle(&mut w);
        assert_eq!(
            w.prop(BlockPos::new(0, 5, 0), "extended"),
            "true",
            "{front} ahead, {side} beside: the front block always moves"
        );
        assert_eq!(w.get(BlockPos::new(2, 5, 0)), front_id, "{front} moved");
        let moved = w.get(BlockPos::new(2, 5, 1));
        let stayed = w.get(beside);
        if dragged {
            assert_eq!(moved, side_id, "{front} ahead drags {side} beside it");
            assert_eq!(stayed, air, "{front} ahead: {side} left its place");
        } else {
            assert_eq!(moved, air, "{front} ahead does not drag {side}");
            assert_eq!(stayed, side_id, "{front} ahead: {side} stays where it was");
        }
    }
}
