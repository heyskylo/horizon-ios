//! A player's break for native mods: what a vein miner needs to break the
//! blocks around the one its player broke as that player breaks them
//! (docs/native-mods.md, "Breaking as a player").
use super::Game;

impl Game<'_> {
    /// Breaks the block at `pos` as the player `uuid` breaks it, through the
    /// game's own `ServerPlayerGameMode.destroyBlock`, as FTB Ultimine does:
    /// a spectator, an adventure player whose tool can't break it, a held item
    /// that can't break blocks (a sword in creative) and a game master block
    /// without the permission leave it, and so does spawn protection on a
    /// dedicated server. Then the mods' `native_destroy_block` hears it (and
    /// can cancel it), and the break runs as a player's: the block's own
    /// effects, its events, particles and sound, the drops rolled for the
    /// held item and its enchantments, the item's wear, the mined statistic,
    /// the exhaustion and the experience. True when the block was removed.
    ///
    /// With `into_inventory`, the block's own drops go to the player's
    /// inventory instead of popping, and what doesn't fit is dropped from
    /// them as `Inventory.placeItemBackInInventory` drops it; what else the
    /// break sets loose (a torch that loses its support, a container's
    /// contents) still drops in the world. A creative player gets no drops.
    ///
    /// The player is the acting one inside their own hook (the usual place:
    /// their `native_destroy_block`, as NeoForge's `BlockEvent.BreakEvent`),
    /// or any online player from a hook with no player swapped in, such as
    /// `native_server_tick`. False, with nothing broken, for anyone else
    /// while a player acts, for a player who isn't here or is dead, for air,
    /// an unloaded block, a block no survival player can break (bedrock),
    /// and off a server. The player needn't reach or look at the block.
    /// Breaking from `native_destroy_block` makes it hear these breaks too:
    /// a mod keeps its own guard against breaking again from them.
    /// API @API:player_break@.
    pub(super) fn native_player_break_block(
        &mut self,
        uuid: u128,
        pos: [i32; 3],
        into_inventory: bool,
    ) -> bool {
        if !self.native_authoritative() {
            return false;
        }
        let mut broke = false;
        self.native_as_player(uuid, |game| {
            broke = game.break_as_acting_player(pos, into_inventory);
        });
        broke
    }

    fn break_as_acting_player(&mut self, pos: [i32; 3], into_inventory: bool) -> bool {
        if !self.world.loaded(pos[0], pos[2]) {
            return false;
        }
        let state = self.world.block(pos[0], pos[1], pos[2]);
        // A survival player never finishes mining a block with no hardness:
        // their client never sends the break.
        if self.res.behaviours().is_air(state)
            || (!self.creative && self.res.facts().get(state).hardness < 0.0)
        {
            return false;
        }
        // ServerPlayerGameMode.handleBlockBreakAction's `mayInteract`.
        if self
            .lan_acting_spawn_protection
            .is_some_and(|radius| self.spawn_protects(radius, self.dim.id(), pos))
        {
            return false;
        }
        // A break inside another's hook keeps where the outer one's drops go.
        let outer = std::mem::replace(&mut self.break_collect, into_inventory.then(Vec::new));
        let broke = self.player_break_block(pos).unwrap_or_else(|e| {
            log::warn!("a mod's break at {pos:?} failed: {e}");
            false
        });
        let drops = std::mem::replace(&mut self.break_collect, outer).unwrap_or_default();
        for stack in drops {
            self.give_or_drop(stack);
        }
        broke
    }
}

#[cfg(test)]
mod tests {
    use super::super::native_machines::tests::server_game;
    use super::*;
    use horizon_client::{command::args::GameMode, items::ItemStack, motion::PlayerMotion};
    use horizon_core::geometry::Vec3;

    /// A server's game with its own player in survival standing at the
    /// origin chunk, holding `held`: their UUID.
    fn with_player(game: &mut Game<'_>, held: ItemStack) -> u128 {
        game.player = Some(PlayerMotion::new(&game.motion, Vec3::new(4.5, 64.0, 4.5)).unwrap());
        game.set_mode(GameMode::Survival);
        game.inventory[game.selected] = Some(held);
        game.native_acting_id()
    }
    fn place(game: &mut Game<'_>, pos: [i32; 3], block: &str) {
        let state = game.pack.assets.registry.default_state(block).unwrap();
        game.world.set(pos[0], pos[1], pos[2], state).unwrap();
    }
    fn block(game: &Game<'_>, pos: [i32; 3]) -> String {
        let state = game.world.block(pos[0], pos[1], pos[2]);
        game.res.facts().get(state).name.to_string()
    }
    fn carried(game: &Game<'_>, item: &str) -> u32 {
        let held = game.inventory.iter().flatten();
        held.filter(|s| &*s.item == item).map(|s| s.count).sum()
    }
    fn popped(game: &Game<'_>, item: &str) -> bool {
        game.dropped.iter().any(|e| &*e.stack.item == item)
    }

    /// Stone broken with a diamond pickaxe drops cobblestone, wears the
    /// pickaxe by one and counts as mined (`Block.playerDestroy`,
    /// `ItemStack.mineBlock`), as a player's own break does.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_mod_breaks_stone_as_the_player_with_their_pickaxe() {
        let mut game = server_game();
        let uuid = with_player(&mut game, ItemStack::new("minecraft:diamond_pickaxe", 1));
        let pos = [6, 64, 4];
        place(&mut game, pos, "minecraft:stone");
        assert!(game.native_player_break_block(uuid, pos, false));
        assert_eq!(block(&game, pos), "minecraft:air");
        assert!(popped(&game, "minecraft:cobblestone"));
        assert_eq!(game.held().unwrap().damage, 1);
        assert_eq!(game.stats.get("mined", "minecraft:stone"), 1);
    }

    /// Into the inventory: silk touch keeps the stone whole, and a diamond
    /// ore gives its diamond and still its experience (3 to 7 points).
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn drops_go_into_the_inventory_rolled_for_the_tools_enchantments() {
        let mut game = server_game();
        let mut silk = ItemStack::new("minecraft:diamond_pickaxe", 1);
        silk.enchant("minecraft:silk_touch", 1);
        let uuid = with_player(&mut game, silk);
        let (stone, ore) = ([6, 64, 4], [7, 64, 4]);
        place(&mut game, stone, "minecraft:stone");
        assert!(game.native_player_break_block(uuid, stone, true));
        assert_eq!(carried(&game, "minecraft:stone"), 1);
        assert!(!popped(&game, "minecraft:stone"));

        game.inventory[game.selected] = Some(ItemStack::new("minecraft:diamond_pickaxe", 1));
        place(&mut game, ore, "minecraft:diamond_ore");
        assert!(game.native_player_break_block(uuid, ore, true));
        assert_eq!(carried(&game, "minecraft:diamond"), 1);
        assert!(!game.orbs.is_empty(), "the ore's experience");
    }

    /// The player's own checks hold: an adventure player whose tool has no
    /// `can_break`, a survival player at bedrock, air, and someone who isn't
    /// playing break nothing.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn the_players_checks_hold() {
        let mut game = server_game();
        let uuid = with_player(&mut game, ItemStack::new("minecraft:diamond_pickaxe", 1));
        let (stone, bedrock, air) = ([6, 64, 4], [7, 64, 4], [8, 64, 4]);
        place(&mut game, stone, "minecraft:stone");
        place(&mut game, bedrock, "minecraft:bedrock");
        assert!(!game.native_player_break_block(uuid, bedrock, false));
        assert_eq!(block(&game, bedrock), "minecraft:bedrock");
        assert!(!game.native_player_break_block(uuid, air, false));
        assert!(!game.native_player_break_block(uuid ^ 1, stone, false));
        game.set_mode(GameMode::Adventure);
        assert!(!game.native_player_break_block(uuid, stone, false));
        assert_eq!(block(&game, stone), "minecraft:stone");
        assert_eq!(game.held().unwrap().damage, 0, "nothing wore the pickaxe");
    }
}
