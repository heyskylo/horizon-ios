use super::*;
#[test]
fn world_entry_invalidates_loaded_progress() {
    let mut adv = advancements::AdvState::default();
    let mut stats = stats::Stats::default();
    adv.loaded = true;
    stats.loaded = true;
    adv.progress
        .criteria
        .insert("minecraft:story/root".into(), Default::default());
    stats.add("mined", "minecraft:stone", 4);
    reset_world_progress(&mut adv, &mut stats);
    assert!(!adv.loaded && !stats.loaded);
    assert!(adv.progress.criteria.is_empty());
    assert!(stats.values.is_empty());
}
