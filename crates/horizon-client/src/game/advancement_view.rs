//! The player's entity view for the per-tick advancement triggers, built only
//! when a loaded criterion listens to the trigger.
use super::Game;
use horizon_client::advancements::{EntityView, Tree};

/// Whether `trigger` needs the player's view. Until the advancements are
/// loaded the tree is not read yet, so the view is built as before. After
/// that, only the lookup `trigger` makes (`by_trigger`) can use it.
fn view_needed(loaded: bool, tree: &Tree, trigger: &str) -> bool {
    !loaded || tree.by_trigger.get(trigger).is_some()
}

impl Game<'_> {
    /// [`Self::player_entity_for`] when [`view_needed`] says so, and an empty
    /// view otherwise: `trigger` then finds no listener and never reads it.
    pub(super) fn player_entity_if_listened(&mut self, trigger: &str) -> EntityView {
        if view_needed(self.adv.loaded, &self.adv.tree, trigger) {
            self.player_entity_for(trigger)
        } else {
            EntityView::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_is_needed_only_for_a_listened_trigger_once_loaded() {
        let mut tree = Tree::default();
        // Before loading the tree is empty but not read yet: built as before.
        assert!(view_needed(false, &tree, "minecraft:tick"));
        assert!(!view_needed(true, &tree, "minecraft:tick"));
        tree.by_trigger.insert("minecraft:tick".into(), vec![(0, 0)]);
        assert!(view_needed(true, &tree, "minecraft:tick"));
        assert!(!view_needed(true, &tree, "minecraft:location"));
    }
}
