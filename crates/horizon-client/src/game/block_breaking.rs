//! Breaking blocks: a held tool mining a block, `break_block` and the player's break with its drops
//! (`Block.popResource` for a broken block), the destroy-progress cracks shown to everyone, and a
//! bundle dropping its content.
use super::*;

impl Game<'_> {
    /// `ItemStack.mineBlock` for the held stack once a survival player has
    /// mined the block `name`: an item with a `tool` component counts as used
    /// and loses its `damage_per_block` on a block that doesn't break
    /// instantly (`ShearsItem`: on any block but fire).
    pub(super) fn mine_block_with_held(&mut self, name: &str, hardness: f32) {
        let Some(stack) = self.inventory[self.selected].clone() else {
            return;
        };
        let Some(per_block) = self.items().mining_damage(&stack) else {
            return;
        };
        let max = self.items().mining_damage_capacity(&stack);
        let wears = if &*stack.item == "minecraft:shears" {
            !self.items().block_in_tag("minecraft:fire", name)
        } else {
            hardness != 0.0
        };
        if wears && per_block > 0 && max > 0 {
            self.damage_held_with_max(per_block, max);
        }
        self.award_stat("used", &stack.item, 1);
    }

    pub(super) fn break_block(&mut self, pos: [i32; 3]) -> Result<()> {
        self.player_break_block(pos).map(|_| ())
    }

    /// `ServerPlayerGameMode.destroyBlock`: whether the block was removed.
    /// A break inside another (a mod breaking more blocks from its
    /// `native_destroy_block`) keeps the outer break's own events.
    pub(super) fn player_break_block(&mut self, pos: [i32; 3]) -> Result<bool> {
        let outer = self.lan_break_pos.replace(pos);
        let broken = self.with_player_actor(|game| game.break_block_as_player(pos));
        self.lan_break_pos = outer;
        broken
    }

    /// `Block.popResource` for a player's break, or into `break_collect`
    /// when a mod takes the drops to the player's inventory: the stack still
    /// gets what the block entity copies into it, and nothing is kept where
    /// blocks don't drop.
    pub(super) fn pop_break_drop(&mut self, pos: [i32; 3], mut stack: ItemStack) {
        if self.break_collect.is_none() {
            return self.pop_resource(pos, stack);
        }
        self.pending.attach(pos, &mut stack);
        if self.ui.rules.bool("block_drops")
            && let Some(kept) = self.break_collect.as_mut()
        {
            kept.push(stack);
        }
    }

    /// `BundleItem.dropContent`: `removeOneItemFromBundle` and `Player.drop`,
    /// then the drop sound and the used statistic.
    pub(super) fn drop_bundle_content(&mut self, slot: usize) {
        let items = self.items();
        let Some(mut bundle) = self.stack_in(slot).cloned() else {
            return;
        };
        let Some(mut contents) = horizon_client::bundle::Contents::of(items, &bundle) else {
            return;
        };
        let Some(removed) = contents.remove_one(items) else {
            return;
        };
        contents.store(&mut bundle);
        let item = Arc::clone(&bundle.item);
        *self.stack_in_mut(slot) = Some(bundle);
        let Some(position) = self.player.as_ref().map(|p| p.position) else {
            return;
        };
        let at = [position.x, position.y, position.z];
        // playRemoveOneSound, then Player.drop and playDropContentsSound at
        // the player's block.
        let pitch = 0.8 + self.world.random.next_float() * 0.4;
        self.audio
            .play(horizon_client::bundle::REMOVE_ONE, Some(at), 0.8, pitch);
        self.player_drop(removed);
        let block = BlockPos::containing(position);
        let pitch = 0.8 + self.world.random.next_float() * 0.4;
        self.audio.play(
            horizon_client::bundle::DROP_CONTENTS,
            Some([block.x, block.y, block.z].map(|v| f64::from(v) + 0.5)),
            0.8,
            pitch,
        );
        self.award_stat("used", &item, 1);
    }

    /// `ServerPlayerGameMode.tick` while a player digs: level event 2020 every
    /// fourth tick since the START (the hit sound), else 2019 (the crack only),
    /// for everyone around, the digger included (`levelEvent(null, ...)`).
    pub(super) fn destroy_progress_event(&mut self, pos: [i32; 3], face: [i32; 3], ticks: u32) {
        let id = if ticks.is_multiple_of(4) { 2020 } else { 2019 };
        let direction = horizon_core::geometry::Direction::ALL
            .iter()
            .position(|d| d.normal() == face)
            .unwrap_or(1);
        self.level_event_at(id, pos, direction as i32);
    }

    /// `LevelRenderer.destroyBlockProgress`: a player's crack on a block (0..=9), or none
    /// (any other progress). Cracks not updated for 400 ticks are dropped.
    pub(crate) fn destroy_block_progress(&mut self, breaker: i32, pos: [i32; 3], progress: i32) {
        let now = self.world.game_time;
        self.destroying
            .retain(|_, c| now.saturating_sub(c.2) <= 400);
        if (0..10).contains(&progress) {
            self.destroying.insert(breaker, (pos, progress as u8, now));
        } else {
            self.destroying.remove(&breaker);
        }
    }
}
