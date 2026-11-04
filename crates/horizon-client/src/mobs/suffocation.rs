//! Runtime binding of Blocks' suffocation definitions to the native registry.
use horizon_world::{
    block_collision::BlockCollisionRegistry,
    suffocation::{SuffocationDefinition, SuffocationRegistry},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    sync::Arc,
};

/// The predicates installed by Blocks' registrations. Predicate evaluation,
/// including full-shape tests and block entities, stays in SuffocationRegistry.
pub fn bind<'r>(
    collision: Arc<BlockCollisionRegistry<'r>>,
    causes_suffocation: &BTreeSet<String>,
) -> io::Result<SuffocationRegistry<'r>> {
    use SuffocationDefinition::*;
    let registry = collision.registry();
    let mut definitions = BTreeMap::new();
    for i in 0..registry.len() {
        let state = registry
            .state(horizon_world::registry::BlockStateId(i as u32))
            .unwrap();
        if definitions.contains_key(&state.name) {
            continue;
        }
        let definition = match state.name.strip_prefix("minecraft:") {
            Some("dirt_path" | "farmland" | "mud" | "soul_sand") => Always,
            Some("piston" | "sticky_piston") => NotExtendedPiston,
            Some("mangrove_roots" | "moving_piston") => Never,
            Some(name)
                if name.ends_with("_leaves")
                    || name == "glass"
                    || name == "tinted_glass"
                    || name.ends_with("_stained_glass")
                    || name == "copper_grate"
                    || name.ends_with("_copper_grate") =>
            {
                Never
            }
            Some(name) if name == "shulker_box" || name.ends_with("_shulker_box") => ClosedShulker,
            _ => TaggedFullCollision,
        };
        definitions.insert(state.name.clone(), definition);
    }
    SuffocationRegistry::new(collision, &definitions, causes_suffocation)
}
