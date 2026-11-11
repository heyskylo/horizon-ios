//! Powder snow and water cauldrons putting a burning player out: the block
//! change they make is gated on `mayInteract`, and the move that put the fire
//! out plays the extinguish hiss (`Entity.playEntityOnFireExtinguishedSound`).
use super::Game;
use horizon_client::motion::MotionEvent;
use horizon_core::position::BlockPos;

impl Game<'_> {
    /// `ServerLevel.mayInteract` for the player whose turn it is: a player
    /// under spawn protection or outside the world border may not change the
    /// block (`ServerPlayer.mayInteract`). The fire goes out either way.
    pub(super) fn may_interact_at(&self, pos: BlockPos) -> bool {
        self.border
            .geometry()
            .is_within_bounds(f64::from(pos.x), f64::from(pos.z), 0.0)
            && !self.lan_acting_spawn_protection.is_some_and(|radius| {
                self.spawn_protects(radius, self.dim.id(), [pos.x, pos.y, pos.z])
            })
    }
    /// `Entity.applyEffectsFromBlocks` along a server player's moves: the
    /// blocks that put the fire out (`PowderSnowBlock.entityInside`'s
    /// runBefore EXTINGUISH, `LayeredCauldronBlock.entityInside`) change
    /// first, then the hiss plays once, as vanilla plays it when the move
    /// left `wasOnFire && !isOnFire()`.
    pub(super) fn block_extinguish_player(&mut self, events: &[MotionEvent]) {
        let mut out = false;
        for event in events {
            match event {
                MotionEvent::CauldronExtinguish { pos } => self.cauldron_extinguish(*pos),
                MotionEvent::PowderSnowMelt { pos } => self.powder_snow_melt(*pos),
                _ => continue,
            }
            out = true;
        }
        let at = self
            .player
            .as_ref()
            .map(|p| [p.position.x, p.position.y, p.position.z]);
        if out && let Some(at) = at {
            self.extinguish_sound(at);
        }
    }
    /// `LayeredCauldronBlock.handleEntityOnFireInside` for a burning player
    /// in the cauldron at `pos` (`mayInteract`): the fill level drops
    /// (`lowerFillLevel`, a `block_change` with no source). A server's
    /// cauldron is the server's.
    pub(super) fn cauldron_extinguish(&mut self, pos: BlockPos) {
        if self.remote.is_some() || !self.may_interact_at(pos) {
            return;
        }
        let at = [pos.x, pos.y, pos.z];
        let registry = self.pack.assets.registry;
        let Some(old) = registry
            .state(self.world.block(at[0], at[1], at[2]))
            .filter(|s| {
                matches!(
                    s.name.as_str(),
                    "minecraft:water_cauldron" | "minecraft:powder_snow_cauldron"
                )
            })
            .cloned()
        else {
            return;
        };
        let Ok(new) = registry.resolve(&horizon_client::cauldron::extinguished(&old)) else {
            return;
        };
        let level = |state: &horizon_world::chunk::BlockState| {
            if matches!(state.name.as_str(), "minecraft:cauldron") {
                0
            } else if state.name == "minecraft:lava_cauldron" {
                3
            } else {
                state
                    .properties
                    .get("level")
                    .and_then(|level| level.parse().ok())
                    .unwrap_or(1)
            }
        };
        let Some(new_state) = registry.state(new).cloned() else {
            return;
        };
        if !self.native_cauldron_level_change_allowed(
            pos,
            level(&old),
            level(&new_state),
            "extinguishing",
        ) {
            return;
        }
        if self.level_set(at, new).is_ok() {
            self.post_block_game_event("minecraft:block_change", at, None, Some(new));
        }
    }
}

#[cfg(test)]
#[path = "extinguish_tests.rs"]
mod extinguish_tests;
