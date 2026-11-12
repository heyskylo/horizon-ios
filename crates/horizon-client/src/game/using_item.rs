//! An item in use, tick by tick (`LivingEntity.startUsingItem`, `updatingUsingItem`,
//! `releaseUsingItem`, `stopUsingItem`): eating and drinking, goat horns, bundles, and the
//! vibrations a use makes.
use super::*;

impl Game<'_> {
    /// `Consumable.startConsuming` for a held consumable: food needs
    /// `Player.canEat` (`Consumable.canConsume`), anything else is always
    /// consumed. The right click is held for `consume_seconds`.
    pub(super) fn start_eating(&mut self, info: &horizon_client::items::ItemInfo) -> bool {
        let Some(stack) = self.held().cloned() else {
            return false;
        };
        // Consumable.canConsume asks the stack's own `food`.
        let consumable = self.consumable(&stack);
        if consumable.is_none()
            || (horizon_client::item_sync::food(&stack, info.food).is_some()
                && (self.player.is_none()
                    || !self.can_eat(
                        &stack,
                        horizon_client::item_sync::can_always_eat(&stack, info.can_always_eat),
                    )))
        {
            return false;
        }
        // as Consumable.startConsuming does, no time is eaten at once, without a use
        // (the server's eating reaches a client through the inventory).
        if let Some(consumable) = consumable.as_ref()
            && item_use::consumes_at_once(consumable.ticks())
        {
            if self.remote.is_none() {
                // the cooldown of ItemStack.use's applyAfterUseComponentSideEffects.
                self.complete_consuming(info, consumable, &stack);
            }
            return true;
        }
        self.start_using_item();
        true
    }
    /// `InstrumentItem.use` for a goat horn: the player holds it up for the
    /// instrument's `use_duration`, its sound plays at the player
    /// (`SoundSource.RECORDS`, volume `range / 16`), `INSTRUMENT_PLAY` is
    /// posted, the used statistic counts and the horn is on cooldown as long.
    /// A horn whose instrument is unknown fails, which still ends the click.
    pub(super) fn use_instrument(&mut self, stack: &ItemStack) {
        let Some(instrument) = self.res.items().instrument(stack) else {
            return;
        };
        self.start_using_item();
        self.play_player_bound_sound(&instrument.sound, 2, instrument.range / 16.0, 1.0, true);
        self.post_entity_event(
            "minecraft:instrument_play",
            horizon_client::mobs::Who::Player,
        );
        if instrument.use_duration > 0.0 {
            let group = horizon_client::cooldowns::group(stack, None);
            self.add_cooldown(group, instrument.use_ticks());
        }
        self.award_stat("used", &stack.item, 1);
    }

    pub(super) fn consume_held(&mut self) {
        if self.creative {
            return;
        }
        let cell = self.held_mut();
        if let Some(s) = cell.as_mut() {
            s.count -= 1;
            if s.count == 0 {
                *cell = None;
            }
        }
    }
    pub(super) fn tick_using(&mut self) {
        let Some(hand) = self.used_hand() else {
            return;
        };
        self.with_hand(hand, Self::tick_used_item);
    }
    /// `LivingEntity.updatingUsingItem` for the item in use, its hand acting.
    pub(super) fn tick_used_item(&mut self) {
        let Some((slot, ticks)) = self.using else {
            return;
        };
        let Some(stack) = self.stack_in(slot).cloned() else {
            self.using = None;
            return;
        };
        // Switching hotbar slots only ends a main-hand use.
        let hand_changed = slot != self.selected && slot != shield::OFFHAND_SLOT;
        if !self.right || hand_changed || self.screen != Screen::Playing {
            self.using = None;
            self.release_used_item(&stack, ticks, hand_changed);
            return;
        }
        // ServerPlayer.updateUsingItem: USING_ITEM every tick of the use.
        self.trigger_using_item(&stack);
        let Some(info) = self.items().get(&stack.item).cloned() else {
            self.using = None;
            return;
        };
        // a mod's tick ran the use out: finished now, or frozen until let go
        // for an item used on release (CrossbowItem.useOnRelease).
        if !self.use_item_ticked(&stack, ticks) {
            if &*stack.item != "minecraft:crossbow" {
                self.complete_using_item(slot, &stack, &info);
            }
            return;
        }
        if self.blocks_attacks(&stack).is_some() {
            // ShieldItem: used for 72000 ticks, i.e. until released.
            self.item_on_use_tick(&stack, ticks);
            self.using = Some((slot, ticks + 1));
            return;
        }
        if self.tick_drinking(slot, ticks, &stack) {
            if self.using.is_none() {
                self.use_item_finished(&stack);
            }
            return;
        }
        if &*stack.item == "minecraft:bow" {
            self.using = Some((slot, ticks + 1));
            return;
        }
        if &*stack.item == "minecraft:crossbow" {
            self.tick_crossbow(slot, ticks, &stack);
            return;
        }
        // TridentItem and EnderEyeItem: used until released (an eye's
        // getUseDuration 0 never counts down to finishUsingItem).
        if matches!(&*stack.item, "minecraft:trident" | "minecraft:ender_eye") {
            self.using = Some((slot, ticks + 1));
            return;
        }
        // ItemStack.onUseTick: a spear stabs what its speed allows, until released.
        if horizon_client::spear::Kinetic::of(&stack.item).is_some() {
            self.spear_tick(&stack, ticks);
            self.using = Some((slot, ticks + 1));
            return;
        }
        if &*stack.item == "minecraft:spyglass" {
            // SpyglassItem.getUseDuration (1200, as a mod left it), then
            // completeUsingItem: finishUsingItem lowers it, and the finish hook.
            match item_use::count_down(ticks, self.use_duration) {
                Some(ticks) => self.using = Some((slot, ticks)),
                None => self.complete_using_item(slot, &stack, &info),
            }
            return;
        }
        if let Some(duration) = self
            .res
            .items()
            .instrument(&stack)
            .map(|i| i.use_ticks().max(0) as u32)
        {
            // InstrumentItem.getUseDuration: held up that long. When it runs
            // out, LivingEntity.stopUsingItem ends the use and causes the
            // interaction vibration; letting go of the button already does.
            if ticks + 1 < duration {
                self.using = Some((slot, ticks + 1));
            } else {
                self.stop_using_item(&stack);
            }
            return;
        }
        if &*stack.item == "minecraft:brush" {
            self.tick_brush(&stack, slot, ticks);
            return;
        }
        if info.bundle {
            self.tick_bundle(slot, ticks);
            return;
        }
        if self.mod_use(&stack) {
            self.tick_mod_use(slot, ticks, &stack);
            return;
        }
        let Some(consumable) = self.consumable(&stack) else {
            self.using = None;
            return;
        };
        // ItemStack.onUseTick: Consumable.shouldEmitParticlesAndSounds with
        // the remaining ticks before this tick counts them down.
        // the duration is getUseDuration's when the use started, as a mod's
        // Start and Tick left it.
        let remaining = self.use_duration - ticks as i32;
        if consumable.should_emit(remaining) {
            self.emit_consume(&consumable, &stack, 5);
        }
        self.item_on_use_tick(&stack, ticks);
        match item_use::count_down(ticks, self.use_duration) {
            Some(ticks) => self.using = Some((slot, ticks)),
            None => self.complete_using_item(slot, &stack, &info),
        }
    }
    /// `BundleItem.onUseTick` and `getUseDuration` (200): the first tick and
    /// then every second tick from the 11th on drop one stack out of the
    /// bundle in `slot`; the use ends after 200 ticks.
    pub(super) fn tick_bundle(&mut self, slot: usize, ticks: u32) {
        const USE_DURATION: u32 = 200;
        let remaining = USE_DURATION - ticks.min(USE_DURATION);
        if ticks == 0 || (remaining < USE_DURATION - 10 && remaining.is_multiple_of(2)) {
            self.drop_bundle_content(slot);
        }
        self.using = (ticks + 1 < USE_DURATION).then_some((slot, ticks + 1));
    }

    /// `LivingEntity.releaseUsingItem` (`Item.releaseUsing`) for `stack`,
    /// used for `ticks`, its hand acting: a bow or a trident lets go (not
    /// when its hand changed), a spyglass stops scoping, and a mod's own
    /// release follows. A mod's `Stop` may skip the release. `stopUsingItem`
    /// ends the use after it.
    pub(super) fn release_used_item(&mut self, stack: &ItemStack, ticks: u32, hand_changed: bool) {
        let canceled = !hand_changed && !self.use_item_stopped(stack, ticks);
        let (release, spyglass) = item_use::release_parts(canceled, hand_changed);
        match &*stack.item {
            "minecraft:bow" if release => self.release_bow(stack, self.ticks_held(stack, ticks)),
            "minecraft:trident" if release => self.release_trident(self.ticks_held(stack, ticks)),
            "minecraft:spyglass" if spyglass => self.stop_spyglass(),
            _ => {}
        }
        if release {
            self.mod_release_using(stack, ticks);
        }
        self.stop_using_item(stack);
    }
    /// `LivingEntity.stopUsingItem`: the use of `stack` is over (`useItem` and
    /// its remaining ticks are cleared), and when the use effects of the stack
    /// ask for it, `ItemStack.causeUseVibration` sends an `ITEM_INTERACT_FINISH`
    /// game event from the player, at its feet.
    pub(super) fn stop_using_item(&mut self, stack: &ItemStack) {
        self.using = None;
        self.use_vibration(stack, "minecraft:item_interact_finish");
    }
    /// `ItemStack.causeUseVibration` for `stack` used by the player: the
    /// `event` game event (`ITEM_INTERACT_START` when the use begins,
    /// `ITEM_INTERACT_FINISH` when it ends) from the player, at its feet, when
    /// the stack's use effects ask for interaction vibrations (the default).
    pub(super) fn use_vibration(&mut self, stack: &ItemStack, event: &'static str) {
        let default = self.items().get(&stack.item).map(|i| i.interact_vibrations);
        if !item_use::interact_vibrations(stack, default) {
            return;
        }
        let Some(p) = self.player.as_ref().map(|p| p.position) else {
            return;
        };
        let ctx = self.player_context(None);
        self.post_game_event(event, [p.x, p.y, p.z], ctx);
    }
}
