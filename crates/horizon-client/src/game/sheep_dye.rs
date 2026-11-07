use super::Game;
use horizon_client::{first_person::Hand, mobs::sheep_dye};

impl Game<'_> {
    /// Whether `interact_sheep_dye` would act on a client, on a copy of the sheep.
    pub(super) fn sheep_would_take_dye(&mut self, index: usize, hand: Hand) -> bool {
        let Some(mut stack) = self.with_hand(hand, |g| g.held().cloned()) else {
            return false;
        };
        sheep_dye::interact(&mut self.mobs[index].clone(), &mut stack, true)
    }

    /// `DyeItem.interactLivingEntity`: PLAYERS, fixed pitch, excluding the
    /// actor on the server because that client's item interaction plays it.
    pub(super) fn interact_sheep_dye(&mut self, index: usize, hand: Hand, client: bool) -> bool {
        let Some(mut stack) = self.with_hand(hand, |g| g.held().cloned()) else {
            return false;
        };
        if !sheep_dye::interact(&mut self.mobs[index], &mut stack, client) {
            return false;
        }
        self.play_mob_bound_sound(index, "minecraft:item.dye.use", 7, 1.0, 1.0, !client);
        if !client && !self.creative {
            self.with_hand(hand, |g| *g.held_mut() = (stack.count > 0).then_some(stack));
        }
        true
    }
}
