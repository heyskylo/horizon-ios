//! The player's hands and what it wears: the stack in the acting hand (`getItemInHand`), the hand of
//! the item in use and running code with the other hand acting, `Inventory.add`, and the worn glider
//! and freeze-immune gear.
use super::*;

impl Game<'_> {
    /// The stack in the hand that acts (`getItemInHand(hand)`).
    pub(super) fn held(&self) -> Option<&ItemStack> {
        self.stack_in(self.held_slot())
    }

    pub(super) fn held_mut(&mut self) -> &mut Option<ItemStack> {
        self.stack_in_mut(self.held_slot())
    }

    /// The slot of the hand that acts: the selected one, or `OFFHAND_SLOT`.
    /// `LivingEntity.canGlide`: some worn piece `gliding_pieces` lets glide,
    /// or a stack in a hand that glides there (`canGlideUsing` walks every
    /// `EquipmentSlot`).
    pub(super) fn glider_worn(&self) -> bool {
        let items = self.res.items();
        let hand = |stack: Option<&ItemStack>, slot: &str| {
            stack.is_some_and(|s| horizon_client::item_sync::can_glide_using(s, slot, items))
        };
        gliding_pieces(items, &self.armor).contains(&true)
            || hand(self.inventory[self.selected].as_ref(), "mainhand")
            || hand(self.offhand.as_ref(), "offhand")
    }

    /// `LivingEntity.updateFallFlying`: the glide's wear. `Util.getRandom`
    /// picks one of the gliders that will not break (from the entity's own
    /// random), and
    /// `ItemStack.hurtAndBreak` takes `points` of its durability (Unbreaking
    /// may save it). Callers skip creative.
    pub(super) fn wear_glider(&mut self, points: u32) {
        let gliding = gliding_pieces(self.res.items(), &self.armor);
        let pieces: Vec<usize> = (0..4).filter(|&i| gliding[i]).collect();
        if pieces.is_empty() {
            return;
        }
        // `Util.getRandom(slotsWithGliders, this.random)`: the entity's own
        // random, which never touches the level's stream.
        let pick = random_source::entity().next_int_bound(pieces.len() as i32);
        let pick = pieces[pick.unwrap_or(0) as usize];
        if let Some(stack) = self.armor[pick].clone() {
            let loss = self.wear(&stack, points);
            if let Some(piece) = self.armor[pick].as_mut() {
                piece.damage += loss;
            }
        }
    }

    /// `LivingEntity.canFreeze`'s armor half: any worn piece in
    /// `freeze_immune_wearables`.
    pub(crate) fn wears_freeze_immune(&self) -> bool {
        let immune = self
            .res
            .items()
            .item_tags
            .get("minecraft:freeze_immune_wearables");
        self.armor
            .iter()
            .flatten()
            .any(|s| immune.is_some_and(|t| t.contains(&*s.item)))
    }

    pub(super) fn held_slot(&self) -> usize {
        match self.hand {
            Hand::Main => self.selected,
            Hand::Off => shield::OFFHAND_SLOT,
        }
    }

    /// `LivingEntity.getUsedItemHand`: the hand of the item in use.
    pub(super) fn used_hand(&self) -> Option<Hand> {
        self.using.map(|(slot, _)| {
            if slot == shield::OFFHAND_SLOT {
                Hand::Off
            } else {
                Hand::Main
            }
        })
    }

    /// Run `f` with `hand` acting (`held`, `consume_held`, `swing_arm`).
    pub(super) fn with_hand<T>(&mut self, hand: Hand, f: impl FnOnce(&mut Self) -> T) -> T {
        let prior = std::mem::replace(&mut self.hand, hand);
        let out = f(self);
        self.hand = prior;
        out
    }

    /// Inventory.add: returns the stack that did not fit.
    pub(super) fn give(&mut self, mut stack: ItemStack) -> Option<ItemStack> {
        let tutorial_item = matches!(
            self.tutorial.step,
            tutorial::Step::FindTree | tutorial::Step::PunchTree | tutorial::Step::CraftPlanks
        )
        .then(|| stack.clone());
        let max = self
            .res
            .items()
            .get(&stack.item)
            .map_or(64, |i| i.max_stack);
        let before = stack.count;
        let left = inventory_add(
            &mut self.inventory,
            &mut self.offhand,
            self.selected,
            max,
            &mut stack,
        );
        if stack.count < before {
            self.tutorial_get_item(tutorial_item.as_ref());
        }
        left.then_some(stack)
    }
}
