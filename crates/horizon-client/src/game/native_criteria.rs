//! A mod's own advancement criteria triggers (docs/native-mods.md,
//! "Advancement triggers"). An advancement of a content mod or a world's data
//! pack names a trigger of the mod's (`mymod:pipe_connected`); the tree keeps
//! it as it keeps vanilla's, and the mod fires it where its event happens, as
//! a NeoForge mod calls its own `SimpleCriterionTrigger.trigger`.
use super::{Game, advancements::item_view, commands::Target};
use horizon_client::{
    advancements::{self as adv, EntityView, Tree},
    items::ItemStack,
};
use serde_json::Value;

/// One open criterion of a mod's trigger, as the mod's test sees it.
pub struct NativeCriterion<'a> {
    /// The criterion's `conditions`, `Null` when it has none. Its `player`
    /// predicate is the game's to test, as vanilla's `SimpleInstance.player`.
    pub conditions: &'a Value,
    world: &'a dyn adv::World,
}

impl NativeCriterion<'_> {
    /// Whether `stack` passes the item predicate under `key` in the
    /// conditions, as the game's own triggers read an `ItemPredicate`:
    /// `items` (an id, a list or a `#tag`), `count`, the `potion_contents`
    /// and `damage` components, and the `enchantments` and `damage`
    /// predicates; other components and predicates aren't checked. A missing
    /// key passes, as an absent `Optional<ItemPredicate>` does.
    pub fn item(&self, key: &str, stack: &ItemStack) -> bool {
        adv::item(&self.conditions[key], &item_view(stack), self.world)
    }
}

/// Whether a mod may fire `trigger`: a namespaced id outside `minecraft`,
/// whose triggers the game fires from its own events.
fn mod_trigger(trigger: &str) -> bool {
    trigger
        .split_once(':')
        .is_some_and(|(ns, path)| !ns.is_empty() && !path.is_empty() && ns != "minecraft")
}

/// Helpers for server mods, kept as stable as the hook points.
// Native mods use these; nothing in Horizon does.
impl Game<'_> {
    /// Fires the mod's criteria trigger `trigger` (`mymod:pipe_connected`)
    /// for the player with that UUID: vanilla's
    /// `SimpleCriterionTrigger.trigger`. Each criterion of theirs with that
    /// trigger, not yet granted, of an advancement not yet done, is tested
    /// by `test` and then by its `player` predicate; those that pass are
    /// granted, and an advancement they complete is awarded with its toast,
    /// chat line and rewards, as the game's own triggers award.
    ///
    /// The acting player is reached inside their hook; any player on the
    /// server, in their own level, only where no player's state is swapped in
    /// (`native_tick`, `native_menu_clicked`), as `native_run_command`. True
    /// when the player was reached, whether or not anything was granted;
    /// false off a server, when the player isn't on or can't be reached from
    /// here, and for a `minecraft:` trigger or a name without a namespace.
    /// API 69.
    pub(super) fn native_trigger_criterion(
        &mut self,
        uuid: u128,
        trigger: &str,
        test: impl Fn(&NativeCriterion<'_>) -> bool,
    ) -> bool {
        if !mod_trigger(trigger) || self.remote.is_some() {
            return false;
        }
        if self.lan_acting_uuid == Some(uuid) {
            self.trigger_mod_criteria(trigger, &test);
            return true;
        }
        if self.lan_acting.is_some() {
            return false;
        }
        let Some(conn) = self.lan.as_ref().and_then(|host| {
            host.hub
                .guests
                .iter()
                .find(|g| g.placed && g.uuid == uuid)
                .map(|g| g.conn)
        }) else {
            return false;
        };
        // In their level: the predicates read where they are, and a reward
        // that doesn't fit drops there.
        self.for_player(Target::Guest(conn), |g| {
            g.trigger_mod_criteria(trigger, &test)
        })
        .is_some()
    }

    /// `native_trigger_criterion` with its test behind a reference, as a
    /// trait mod calls it through `dyn Host`: the same players, criteria and
    /// answer. API @API:host_bridge@.
    pub(super) fn native_trigger_mod_criterion(
        &mut self,
        uuid: u128,
        trigger: &str,
        test: &dyn Fn(&NativeCriterion<'_>) -> bool,
    ) -> bool {
        self.native_trigger_criterion(uuid, trigger, test)
    }

    /// `trigger` for a mod's trigger, for the player in place.
    fn trigger_mod_criteria(&mut self, trigger: &str, test: &dyn Fn(&NativeCriterion<'_>) -> bool) {
        let me = self.player_entity_for(trigger);
        self.trigger(trigger, |conditions, world| {
            passes(conditions, &me, world, test)
        });
    }
}

/// `SimpleCriterionTrigger.trigger`'s test of one criterion for the player
/// `me`: the mod's matcher, then the `player` predicate, in the player's own
/// context (`EntityPredicate.createContext(player, player)`).
fn passes(
    conditions: &Value,
    me: &EntityView,
    world: &dyn adv::World,
    test: &dyn Fn(&NativeCriterion<'_>) -> bool,
) -> bool {
    let player = adv::Entities {
        this: Some(me),
        origin: me.position,
        tool: None,
        block_pos: None,
    };
    test(&NativeCriterion { conditions, world })
        && adv::conditions(&conditions["player"], &player, world)
}

/// Whether a per-tick trigger needs the player's view. Until the advancements
/// are loaded the tree is not read yet, so the view is built as before. After
/// that, only the lookup `trigger` makes (`by_trigger`) can use it.
fn view_needed(loaded: bool, tree: &Tree, trigger: &str) -> bool {
    !loaded || tree.by_trigger.contains_key(trigger)
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
    use serde_json::json;
    use std::collections::BTreeMap;

    struct Plains;
    impl adv::World for Plains {
        fn dimension(&self) -> &str {
            "minecraft:overworld"
        }
        fn weather(&self) -> (bool, bool) {
            (false, false)
        }
        fn biome(&self, _: [i32; 3]) -> String {
            "minecraft:plains".into()
        }
        fn structures(&self, _: [i32; 3]) -> Vec<String> {
            Vec::new()
        }
        fn block(&self, _: [i32; 3]) -> (String, BTreeMap<String, String>) {
            ("minecraft:air".into(), BTreeMap::new())
        }
        fn fluid(&self, _: [i32; 3]) -> Option<String> {
            None
        }
        fn can_see_sky(&self, _: [i32; 3]) -> bool {
            true
        }
        fn light(&self, _: [i32; 3]) -> i32 {
            15
        }
        fn smokey(&self, _: [i32; 3]) -> bool {
            false
        }
        fn in_tag(&self, registry: &str, tag: &str, id: &str) -> bool {
            registry == "item" && tag == "c:ingots/copper" && id == "minecraft:copper_ingot"
        }
    }

    /// A mod's `pipe_connected` criterion: the pipe's item by a convention
    /// tag, and only for a player on fire (a loot condition list, as
    /// vanilla's advancements write `player`).
    fn criterion() -> Value {
        json!({
            "pipe": {"items": "#c:ingots/copper", "count": {"min": 2}},
            "player": [{
                "condition": "minecraft:entity_properties",
                "entity": "this",
                "predicate": {"flags": {"is_on_fire": true}}
            }]
        })
    }

    #[test]
    fn a_mods_criterion_takes_its_own_test_and_the_player_predicate() {
        let burning = EntityView {
            kind: "minecraft:player".into(),
            on_fire: true,
            ..EntityView::default()
        };
        let calm = EntityView {
            on_fire: false,
            ..burning.clone()
        };
        let copper = ItemStack::new("minecraft:copper_ingot", 2);
        let pipe = |stack: &ItemStack| {
            let stack = stack.clone();
            move |c: &NativeCriterion<'_>| c.item("pipe", &stack)
        };
        let c = criterion();
        assert!(passes(&c, &burning, &Plains, &pipe(&copper)));
        assert!(
            !passes(&c, &calm, &Plains, &pipe(&copper)),
            "the player predicate"
        );
        let one = ItemStack::new("minecraft:copper_ingot", 1);
        assert!(
            !passes(&c, &burning, &Plains, &pipe(&one)),
            "the item's count"
        );
        let iron = ItemStack::new("minecraft:iron_ingot", 2);
        assert!(!passes(&c, &burning, &Plains, &pipe(&iron)), "the tag");
        // A criterion without the key or any conditions passes the item test.
        let any = |c: &NativeCriterion<'_>| c.item("filter", &iron);
        assert!(passes(&c, &burning, &Plains, &any));
        assert!(passes(&Value::Null, &calm, &Plains, &any));
    }

    #[test]
    fn mods_fire_only_their_own_namespaced_triggers() {
        assert!(mod_trigger("mymod:pipe_connected"));
        assert!(!mod_trigger("minecraft:inventory_changed"));
        assert!(!mod_trigger("inventory_changed"));
        assert!(!mod_trigger(":pipe_connected"));
    }

    #[test]
    fn view_is_needed_only_for_a_listened_trigger_once_loaded() {
        let mut tree = Tree::default();
        // Before loading the tree is empty but not read yet: built as before.
        assert!(view_needed(false, &tree, "minecraft:tick"));
        assert!(!view_needed(true, &tree, "minecraft:tick"));
        tree.by_trigger
            .insert("minecraft:tick".into(), vec![(0, 0)]);
        assert!(view_needed(true, &tree, "minecraft:tick"));
        assert!(!view_needed(true, &tree, "minecraft:location"));
    }
}
