use super::*;

#[test]
fn a_barrel_recount_with_openers_left_on_both_sides_changes_nothing() {
    use horizon_core::ticks::TickPriority;
    let mut w = world!();
    let barrel = w.id("barrel", &[("facing", "north"), ("open", "true")]);
    let pos = BlockPos::new(0, 1, 0);
    w.put(pos, barrel);
    let block = w.b().block(barrel);
    let sounds = |w: &World, name: &str| {
        w.level
            .events
            .iter()
            .filter(|e| matches!(e, LevelEvent::Sound { sound, .. } if &**sound == name))
            .count()
    };
    let game_events = |w: &World, name: &str| {
        w.level
            .events
            .iter()
            .filter(|e| matches!(e, LevelEvent::GameEvent { event, .. } if *event == name))
            .count()
    };

    // Two players had the barrel open and one left: it stays open, and
    // ContainerOpenersCounter.onOpen/onClose only run when the count reaches
    // or leaves zero.
    w.ticks
        .schedule_block(w.level.time, pos, block, 1, TickPriority::Normal);
    w.level.openers.insert(pos, (2, 1));
    w.tick();

    assert_eq!(
        w.level.opener_rechecks.len(),
        1,
        "the barrel's scheduled recheck ran"
    );
    assert_eq!(w.prop(pos, "open"), "true", "a half-open barrel stays open");
    assert_eq!(sounds(&w, "minecraft:block.barrel.open"), 0);
    assert_eq!(sounds(&w, "minecraft:block.barrel.close"), 0);
    assert_eq!(game_events(&w, "minecraft:container_open"), 0);
    assert_eq!(game_events(&w, "minecraft:container_close"), 0);
}
