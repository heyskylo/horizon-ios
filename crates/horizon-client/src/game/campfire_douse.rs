//! `CampfireBlock.useItemOn`: an item in `#minecraft:douses_campfires` (the
//! shovels) put on a lit campfire puts it out.
use super::Game;
use horizon_client::{Result, animate_tick::make_campfire_smoke, items::ItemStack};
use horizon_core::position::BlockPos;
use horizon_world::chunk::BlockState;
use winit::keyboard::KeyCode;

impl Game<'_> {
    /// The lit campfire at `pos` that `held` puts out, when the item is in
    /// `#minecraft:douses_campfires` (`CampfireBlock.useItemOn`).
    fn campfire_to_douse(&self, pos: [i32; 3], held: Option<&ItemStack>) -> Option<BlockState> {
        held.filter(|s| {
            self.res
                .items()
                .item_tags
                .get("minecraft:douses_campfires")
                .is_some_and(|members| members.contains(&*s.item))
        })?;
        self.pack
            .assets
            .registry
            .state(self.world.block(pos[0], pos[1], pos[2]))
            .filter(|s| matches!(&*s.name, "minecraft:campfire" | "minecraft:soul_campfire"))
            .filter(|s| s.properties.get("lit").map(String::as_str) == Some("true"))
            .cloned()
    }

    /// `CampfireBlock.useItemOn` as this client runs it before the server's
    /// answer (`MultiPlayerGameMode.useItemOn`): the twenty smoke puffs of
    /// `CampfireBlock.douse` and the campfire gone out, kept as predicted
    /// under `seq`. The level event's sound comes from the server. True when
    /// it doused (SUCCESS), which ends the click for both hands.
    pub(super) fn predict_campfire_douse(
        &mut self,
        pos: [i32; 3],
        held: Option<&ItemStack>,
        seq: i32,
    ) -> bool {
        // Player.isSecondaryUseActive: a held item means both hands count.
        if self.keys.contains(&KeyCode::ShiftLeft) {
            return false;
        }
        let Some(mut state) = self.campfire_to_douse(pos, held) else {
            return false;
        };
        let signal = state.properties.get("signal_fire").map(String::as_str) == Some("true");
        for _ in 0..20 {
            make_campfire_smoke(&mut self.particles, pos, signal, true);
        }
        state.properties.insert("lit".into(), "false".into());
        if let Ok(unlit) = self.pack.assets.registry.resolve(&state) {
            let at = BlockPos::new(pos[0], pos[1], pos[2]);
            self.predict_blocks(&vec![(at, unlit)], seq);
        }
        true
    }

    /// `CampfireBlock.useItemOn` with `held` in the hand that acts, the block
    /// at `pos` clicked: true when it doused a lit campfire (SUCCESS).
    pub(super) fn use_campfire_douse(
        &mut self,
        pos: [i32; 3],
        held: Option<&ItemStack>,
    ) -> Result<bool> {
        let (Some(used), Some(mut state)) = (held.cloned(), self.campfire_to_douse(pos, held))
        else {
            return Ok(false);
        };
        let registry = self.pack.assets.registry;
        // Level event 1009 (SOUND_EXTINGUISH_FIRE) goes to every player.
        self.level_event_at(1009, pos, 0);
        // CampfireBlock.douse: the acting client's own smoke (twenty puffs),
        // then the block change game event from the player.
        if self.lan_acting.is_none() {
            let signal = state.properties.get("signal_fire").map(String::as_str) == Some("true");
            for _ in 0..20 {
                make_campfire_smoke(&mut self.particles, pos, signal, true);
            }
        }
        self.post_block_game_event("minecraft:block_change", pos, self.sculk_actor(), None);
        state.properties.insert("lit".into(), "false".into());
        if let Ok(unlit) = registry.resolve(&state) {
            self.level_set(pos, unlit)?;
        }
        self.damage_held(1);
        // ServerPlayerGameMode.useItemOn: the action was consumed.
        self.trigger_block("minecraft:item_used_on_block", pos, &used);
        Ok(true)
    }
}
