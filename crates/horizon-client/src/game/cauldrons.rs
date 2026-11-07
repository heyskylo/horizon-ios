//! Using an item on a cauldron (`CauldronInteraction`): what the click does, and doing it.
use super::*;

impl Game<'_> {
    /// What the cauldron at `pos` answers to `held` on a client, which only
    /// asks (`CauldronInteractions` acts `if (!level.isClientSide())`). A
    /// sneak with something in hand skips `useItemOn`
    /// (`Player.isSecondaryUseActive`).
    pub(super) fn cauldron_outcome(
        &self,
        pos: [i32; 3],
        held: Option<&ItemStack>,
    ) -> horizon_client::cauldron::Outcome {
        use horizon_client::cauldron::{self, Outcome};
        let Some(held) = held else {
            return Outcome::Pass;
        };
        if self.keys.held(Mapping::Sneak) {
            return Outcome::Pass;
        }
        let Some((state, under_water)) = self.cauldron_at(pos) else {
            return Outcome::Pass;
        };
        let removes_dye = cauldron::removes_dye(self.items(), &held.item);
        cauldron::outcome(&state, under_water, removes_dye, held)
    }
    /// `CauldronInteractions` for the held item aimed at a cauldron.
    pub(super) fn use_on_cauldron(&mut self, pos: [i32; 3]) -> Result<bool> {
        use horizon_client::cauldron::{self, Outcome};
        struct Hand<'g, 'a>(&'g mut Game<'a>);
        impl cauldron::Player for Hand<'_, '_> {
            fn creative(&self) -> bool {
                self.0.creative
            }
            fn hand(&mut self) -> &mut Option<ItemStack> {
                self.0.held_mut()
            }
            fn add(&mut self, stack: &mut ItemStack) -> bool {
                let before = stack.count;
                match self.0.give(stack.clone()) {
                    None => stack.count = 0,
                    Some(rest) => stack.count = rest.count,
                }
                stack.count < before
            }
            fn contains(&self, stack: &ItemStack) -> bool {
                self.0
                    .inventory
                    .iter()
                    .flatten()
                    .any(|have| have.same_item_same_components(stack))
            }
            fn drop_stack(&mut self, stack: ItemStack) {
                if let Some(eye) = self.0.eye_position() {
                    // ItemUtils.createFilledResult: `player.drop(stack, false)`.
                    self.0.throw_from_eyes(stack, eye, false);
                }
            }
        }
        let registry = self.pack.assets.registry;
        let Some((state, under_water)) = self.cauldron_at(pos) else {
            return Ok(false);
        };
        let Some(held) = self.held() else {
            return Ok(false);
        };
        // Player.isSecondaryUseActive with an item in hand skips useItemOn.
        if self.keys.held(Mapping::Sneak) {
            return Ok(false);
        }
        let removes_dye = cauldron::removes_dye(self.items(), &held.item);
        let result = cauldron::interact(&state, under_water, removes_dye, &mut Hand(self));
        if result.outcome == Outcome::Pass {
            return Ok(false);
        }
        if let Some(stat) = result.stat {
            self.award_custom_stat(stat, 1);
        }
        if let Some(block) = result.block
            && let Ok(new) = registry.resolve(&block)
        {
            self.level_set(pos, new)?;
            if result.lower_fill_level {
                self.post_block_game_event("minecraft:block_change", pos, None, Some(new));
            }
        }
        if let Some(sound) = result.sound {
            self.audio.play(
                sound,
                Some([
                    f64::from(pos[0]) + 0.5,
                    f64::from(pos[1]) + 0.5,
                    f64::from(pos[2]) + 0.5,
                ]),
                1.0,
                1.0,
            );
        }
        // `fillBucket`/`emptyBucket` and the bottle's two interactions end with
        // `level.gameEvent(null, GameEvent.FLUID_PICKUP or FLUID_PLACE, pos)`.
        if let Some(event) = result.fluid_event {
            self.post_block_game_event(event, pos, None, None);
        }
        Ok(true)
    }
}
