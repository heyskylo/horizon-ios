//! `LivingEntity.startUsingItem` and the use over time of items vanilla's
//! `Item` subclasses don't know: what `Item.getUseDuration` answers, a
//! native mod's own item counted down to its `finishUsingItem`, and the hand
//! pose of a `getUseAnimation` (`native_use_duration`, `native_use_animation`,
//! `native_living_entity_use_item_start`, `native_living_entity_use_item_finish`),
//! and each tick and the release of a use (`native_living_entity_use_item_tick`,
//! `native_on_use_tick`, `native_living_entity_use_item_stop`, `native_release_using`).
use super::Game;
use horizon_client::{
    items::{ItemInfo, ItemStack},
    spear::Kinetic,
};
use horizon_world::consumable::UseAnimation;

/// Resolve USE_EFFECTS through the stack's component patch.
pub(super) fn interact_vibrations(stack: &ItemStack, default: Option<bool>) -> bool {
    if stack.count == 0 {
        return false;
    }
    let Some(data) = stack.data() else {
        return default.unwrap_or(false);
    };
    if data.other("!minecraft:use_effects").is_some() {
        return false;
    }
    data.other("minecraft:use_effects").map_or_else(
        || default.unwrap_or(false),
        |tag| horizon_client::save::number(tag.get("interact_vibrations")).unwrap_or(1) != 0,
    )
}

impl Game<'_> {
    /// `LivingEntity.startUsingItem` for the acting player's hand: nothing
    /// when a use runs already or the hand is empty. The use lasts
    /// `getUseDuration` ticks, which mods may change or refuse
    /// (`LivingEntityUseItemEvent.Start`), on both sides as in vanilla. True
    /// when the use started.
    pub(super) fn start_using_item(&mut self) -> bool {
        let Some(stack) = self.held().cloned().filter(|_| self.using.is_none()) else {
            return false;
        };
        let own = self.use_duration(&stack);
        let (mut duration, mut canceled) = (own, false);
        let hand = self.hand;
        self.native_living_entity_use_item_start(&stack, None, hand, &mut duration, &mut canceled);
        let Some(ticks_set) = start_of(own, duration, canceled) else {
            return false;
        };
        self.using = Some((self.held_slot(), 0));
        self.use_duration = duration;
        self.use_ticks_set = ticks_set;
        // `LivingEntity.startUsingItem`: `causeUseVibration(ITEM_INTERACT_START)`
        // when the use effects of the stack ask for interaction vibrations.
        self.use_vibration(&stack, "minecraft:item_interact_start");
        // A new charge hears the crossbow's loading sounds again
        // (`game/crossbow.rs`).
        self.crossbow_sounds = [false; 2];
        true
    }
    /// `ItemStack.getUseDuration`: a consumable's time, 72000 for what is
    /// held until let go, the fixed times of brushes, bundles, spyglasses and
    /// goat horns, 0 for the rest, then a mod's answer. A `consumable` or
    /// `blocks_attacks` the server's patch adds counts as the item's own
    /// (`Item.getUseDuration` reads the stack's components).
    pub(super) fn use_duration(&self, stack: &ItemStack) -> i32 {
        let info = self.items().get(&stack.item);
        let (patched_ticks, patched_blocks) = horizon_client::item_sync::patched_use(stack);
        let mut duration = match &*stack.item {
            "minecraft:bow" | "minecraft:crossbow" | "minecraft:trident" => 72_000,
            "minecraft:spyglass" => 1200,
            "minecraft:brush" => 200,
            _ if info.is_some_and(|i| i.bundle) => 200,
            _ if let Some(i) = self.res.items().instrument(stack) => i.use_ticks(),
            _ if let Some(ticks) = patched_ticks => ticks,
            _ => match info {
                Some(i) if let Some(c) = &i.consumable => c.ticks(),
                _ if patched_blocks => 72_000,
                Some(i) if i.blocks_attacks.is_some() => 72_000,
                _ if Kinetic::of(&stack.item).is_some() => 72_000,
                _ => 0,
            },
        };
        self.native_use_duration(stack, &mut duration);
        duration
    }
    /// Whether `stack` is a mod's item that only a mod uses over time: no
    /// `consumable`, `blocks_attacks` or kinetic weapon of vanilla's own, and
    /// a `native_use_duration` above 0. Its use is its own `Item.use`.
    pub(super) fn mod_use(&self, stack: &ItemStack) -> bool {
        !stack.item.starts_with("minecraft:")
            && self
                .items()
                .get(&stack.item)
                .is_some_and(|i| i.consumable.is_none() && i.blocks_attacks.is_none())
            && Kinetic::of(&stack.item).is_none()
            && self.use_duration(stack) > 0
    }
    /// A right click with a `mod_use` item: `startUsingItem` (CONSUME).
    pub(super) fn start_mod_use(&mut self, stack: &ItemStack) -> bool {
        if !self.mod_use(stack) {
            return false;
        }
        self.start_using_item();
        true
    }
    /// `LivingEntity.updateUsingItem` for a `mod_use` item used `ticks` so far:
    /// `completeUsingItem` once its duration ran out.
    pub(super) fn tick_mod_use(&mut self, slot: usize, ticks: u32, stack: &ItemStack) {
        self.item_on_use_tick(stack, ticks);
        if let Some(ticks) = count_down(ticks, self.use_duration) {
            self.using = Some((slot, ticks));
            return;
        }
        let Some(info) = self.items().get(&stack.item).cloned() else {
            self.using = None;
            return;
        };
        self.complete_using_item(slot, stack, &info);
    }
    /// `ServerPlayer.completeUsingItem` of `stack` in `slot`: the client is
    /// told, `ItemStack.finishUsingItem` (a consumable is eaten, a potion
    /// drunk, a spyglass lowered, anything else given back) with its use
    /// cooldown, then `LivingEntityUseItemEvent.Finish`.
    pub(super) fn complete_using_item(&mut self, slot: usize, stack: &ItemStack, info: &ItemInfo) {
        // a potion is drunk (no last tick of its own: a mod's tick ran it out).
        if self.finish_drinking(slot, stack) {
            self.use_item_finished(stack);
            self.use_vibration(stack, "minecraft:item_interact_finish");
            return;
        }
        self.using = None;
        self.send_use_item_complete();
        if let Some(consumable) = &info.consumable {
            self.finish_consuming(info, consumable, stack);
        } else if &*stack.item == "minecraft:spyglass" {
            self.stop_spyglass();
        }
        // ItemStack.finishUsingItem applies after-use components to the
        // original stack after consumption and its remainder, before the hook.
        self.apply_use_cooldown(stack);
        self.use_item_finished(stack);
        // `LivingEntity.completeUsingItem` ends with `stopUsingItem`.
        self.use_vibration(stack, "minecraft:item_interact_finish");
    }
    /// `LivingEntityUseItemEvent.Finish` after `finishUsingItem` of `stack`
    /// (no ticks left): the hand holds what a mod made the result.
    pub(super) fn use_item_finished(&mut self, stack: &ItemStack) {
        let mut result = self.held().cloned();
        let before = result.clone();
        let hand = self.hand;
        self.native_living_entity_use_item_finish(stack, None, hand, 0, &mut result);
        if result != before {
            *self.held_mut() = result.filter(|s| s.count > 0);
        }
    }
    /// `LivingEntityUseItemEvent.Tick` for `stack` in use `ticks` so far, as
    /// `updatingUsingItem` starts: a mod may change the ticks left, which
    /// moves the end. False when it ran the use out (`tick_ran_out`): the
    /// item's tick and countdown are skipped.
    pub(super) fn use_item_ticked(&mut self, stack: &ItemStack, ticks: u32) -> bool {
        let before = ticks_left(self.use_duration, ticks);
        let (mut remaining, mut canceled) = (before, false);
        let hand = self.used_hand().unwrap_or(self.hand);
        self.native_living_entity_use_item_tick(stack, None, hand, &mut remaining, &mut canceled);
        let (left, set) = tick_left(before, remaining, canceled, self.use_ticks_set);
        self.use_ticks_set = set;
        self.use_duration = duration_leaving(ticks, left);
        left > 0 || !set
    }
    /// A mod's `Item.onUseTick` of `stack` used `ticks`, while ticks are left.
    pub(super) fn item_on_use_tick(&mut self, stack: &ItemStack, ticks: u32) {
        let remaining = ticks_left(self.use_duration, ticks);
        if remaining > 0 {
            self.native_on_use_tick(stack, None, remaining);
        }
    }
    /// How long `stack`, used `ticks`, counts as held: `getUseDuration` less
    /// the ticks left (`BowItem.releaseUsing`, `CrossbowItem.onUseTick`,
    /// `TridentItem.releaseUsing`), `ticks` unless a mod's Start or Tick
    /// moved the ticks left.
    pub(super) fn ticks_held(&self, stack: &ItemStack, ticks: u32) -> u32 {
        held(
            self.use_duration(stack),
            ticks_left(self.use_duration, ticks),
        )
    }
    /// `LivingEntityUseItemEvent.Stop` as `releaseUsingItem` lets go of
    /// `stack` used `ticks`: false when a mod skips its `releaseUsing`.
    pub(super) fn use_item_stopped(&mut self, stack: &ItemStack, ticks: u32) -> bool {
        let mut canceled = false;
        let remaining = ticks_left(self.use_duration, ticks);
        let hand = self.hand;
        self.native_living_entity_use_item_stop(stack, None, hand, remaining, &mut canceled);
        !canceled
    }
    /// A mod's `Item.releaseUsing` of `stack` used `ticks`: when it says it
    /// was used, `ItemStack.releaseUsing` applies the use cooldown.
    pub(super) fn mod_release_using(&mut self, stack: &ItemStack, ticks: u32) {
        let mut used = false;
        let remaining = ticks_left(self.use_duration, ticks);
        self.native_release_using(stack, None, remaining, &mut used);
        if used {
            self.apply_use_cooldown(stack);
        }
    }
    /// `Player.canEat(canAlwaysEat)` for `stack`, food that is eaten only
    /// when hungry, then a mod's answer.
    pub(super) fn can_eat(&self, stack: &ItemStack, can_always_eat: bool) -> bool {
        let mut can_eat = can_always_eat
            || self.creative
            || self
                .player
                .as_ref()
                .is_some_and(|p| p.health.food.food < 20);
        self.native_can_eat(stack, &mut can_eat);
        can_eat
    }
}

/// The ticks left after a mod's `Tick`, a cancel being NeoForge's -1, and
/// whether a mod ever set this use's: once one did, the use goes on only
/// while some are left, else it ran out. A use no mod touched goes on as
/// vanilla's own do (an eye of ender's has none from the start).
fn tick_left(before: i32, remaining: i32, canceled: bool, set: bool) -> (i32, bool) {
    let left = if canceled { -1 } else { remaining };
    (left, set || canceled || remaining != before)
}

/// What `releaseUsingItem` runs of a release: the item's own and a mod's
/// `releaseUsing` (not when a mod's `Stop` canceled, nor when the hand
/// changed item, which stops the use without one), and the spyglass's
/// sound (not when canceled).
pub(super) fn release_parts(canceled: bool, hand_changed: bool) -> (bool, bool) {
    (!canceled && !hand_changed, !canceled)
}

pub(super) fn bow_release_power(has_ammo: bool, charge: i32, canceled: bool) -> Option<f32> {
    if canceled || charge < 0 || !has_ammo {
        return None;
    }
    let f = charge as f32 / 20.0;
    let power = ((f * f + f * 2.0) / 3.0).min(1.0);
    (power >= 0.1).then_some(power)
}

/// `useItemRemaining` of a use of `duration` ticks, `ticks` old.
fn ticks_left(duration: i32, ticks: u32) -> i32 {
    duration.saturating_sub_unsigned(ticks)
}

/// `getUseDuration(stack) - remainingTime` of a use of `own` ticks with
/// `remaining` left, none below 0.
fn held(own: i32, remaining: i32) -> u32 {
    u32::try_from(i64::from(own) - i64::from(remaining)).unwrap_or(0)
}

/// The duration that leaves `remaining` ticks of a use `ticks` old, as a
/// mod's `Tick` set them.
fn duration_leaving(ticks: u32, remaining: i32) -> i32 {
    remaining.saturating_add_unsigned(ticks)
}

/// What a `Start` left of a use of `own` ticks: None refused (a cancel, or
/// a time below 0, as NeoForge's `startUsingItem` returns), else whether a
/// mod set the ticks left to none. Such a use runs out on its first tick
/// (`tick_left`), as NeoForge's `--useItemRemaining <= 0` completes it, while
/// an item's own 0 (an eye of ender) is held until let go, as vanilla's.
fn start_of(own: i32, duration: i32, canceled: bool) -> Option<bool> {
    (!canceled && duration >= 0).then_some(duration == 0 && own != 0)
}

/// `Consumable.startConsuming`: a time of 0 or less is no use, the item is
/// consumed at once (`onConsume`).
pub(super) fn consumes_at_once(consume_ticks: i32) -> bool {
    consume_ticks <= 0
}

/// `updateUsingItem`'s `--useItemRemaining == 0` for a use `ticks` old of
/// `duration` ticks: the ticks used after this one, none on the tick it runs
/// out. A time of 0 or less, or one a mod cut below what was used, never
/// does: the use lasts until let go.
pub(super) fn count_down(ticks: u32, duration: i32) -> Option<u32> {
    let ticks = ticks + 1;
    (i64::from(duration) - i64::from(ticks) != 0).then_some(ticks)
}

/// The first person pose of a hand using an item of `animation`
/// (`ItemInHandRenderer.renderArmWithItem`'s switch), for an item of no
/// pose of its own.
pub(super) fn use_pose(
    animation: UseAnimation,
    remaining: f32,
    duration: f32,
    partial: f32,
) -> horizon_client::first_person::ItemPose {
    use horizon_client::first_person::ItemPose;
    let ticks_used = duration - (remaining - partial + 1.0);
    match animation {
        UseAnimation::Eat | UseAnimation::Drink => ItemPose::Eat {
            remaining,
            duration,
            partial,
        },
        UseAnimation::Bow => ItemPose::Bow { ticks_used },
        UseAnimation::Trident => ItemPose::Trident { ticks_used },
        UseAnimation::Brush => ItemPose::Brush { remaining, partial },
        _ => ItemPose::Using,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bow_release_power, consumes_at_once, count_down, duration_leaving, held, release_parts,
        start_of, tick_left, ticks_left, use_pose,
    };
    use horizon_client::first_person::ItemPose;
    use horizon_world::consumable::UseAnimation;

    #[test]
    fn a_use_runs_out_on_its_last_tick() {
        // a 32 tick food: ticks 0 to 30 go on, the 32nd tick finishes it.
        assert_eq!(count_down(0, 32), Some(1));
        assert_eq!(count_down(30, 32), Some(31));
        assert_eq!(count_down(31, 32), None);
        // a one tick use finishes on its first tick.
        assert_eq!(count_down(0, 1), None);
        // no time, or one below what was used, waits for the release.
        assert_eq!(count_down(0, 0), Some(1));
        assert_eq!(count_down(10, 4), Some(11));
        assert_eq!(count_down(5, 72_000), Some(6));
    }

    #[test]
    fn a_started_use_finishes_after_its_duration() {
        // start at 0 ticks, count down each tick, finish when it runs out.
        for duration in [1, 16, 32, 40] {
            let (mut ticks, mut ticked) = (0, 0);
            while let Some(next) = count_down(ticks, duration) {
                ticks = next;
                ticked += 1;
            }
            assert_eq!(ticked + 1, duration);
        }
    }

    #[test]
    fn a_start_below_0_refuses_the_use_and_a_mods_0_runs_out() {
        // a cancel or a time below 0 refuses it.
        assert_eq!(start_of(32, 32, true), None);
        assert_eq!(start_of(32, -1, false), None);
        assert_eq!(start_of(0, -1, false), None);
        // a mod's 0 starts it, and its first tick runs it out.
        assert_eq!(start_of(32, 0, false), Some(true));
        let (left, set) = tick_left(0, 0, false, true);
        assert!(!(left > 0 || !set));
        // an item's own 0 (an eye of ender) goes on, as a changed time does.
        assert_eq!(start_of(0, 0, false), Some(false));
        let (left, set) = tick_left(0, 0, false, false);
        assert!(left > 0 || !set);
        assert_eq!(start_of(32, 64, false), Some(false));
    }

    #[test]
    fn a_tick_moves_the_end_of_a_use() {
        // a 32 tick use, 10 ticks in: 22 left.
        assert_eq!(ticks_left(32, 10), 22);
        // a mod leaves 1 tick: the next tick finishes it.
        let duration = duration_leaving(10, 1);
        assert_eq!(count_down(10, duration), None);
        // or 30 more: it goes on to tick 40.
        let duration = duration_leaving(10, 30);
        assert_eq!(duration, 40);
        assert_eq!(count_down(38, duration), Some(39));
        assert_eq!(count_down(39, duration), None);
        // left as it was, nothing moves.
        assert_eq!(duration_leaving(10, ticks_left(32, 10)), 32);
    }

    #[test]
    fn a_tick_of_no_time_left_runs_the_use_out() {
        let goes_on = |(left, set): (i32, bool)| left > 0 || !set;
        // untouched, a use goes on, even one of no time (an eye of ender).
        assert_eq!(tick_left(20, 20, false, false), (20, false));
        assert!(goes_on(tick_left(-3, -3, false, false)));
        // a mod's ticks left go on while above 0.
        assert_eq!(tick_left(20, 5, false, false), (5, true));
        assert!(goes_on(tick_left(20, 5, false, false)));
        // a cancel is -1 left, and it or 0 or less set runs the use out.
        assert_eq!(tick_left(20, 20, true, false), (-1, true));
        assert!(!goes_on(tick_left(20, 0, false, false)));
        assert!(!goes_on(tick_left(20, -1, false, false)));
        // a use a mod ran out stays out on the next ticks (a crossbow frozen).
        assert!(!goes_on(tick_left(-1, -1, false, true)));
        // and goes on again if a mod gives it ticks back, as NeoForge's.
        assert!(goes_on(tick_left(-1, 10, false, true)));
    }

    #[test]
    fn a_release_runs_what_vanilla_runs() {
        // let go: the item's release and the spyglass's sound.
        assert_eq!(release_parts(false, false), (true, true));
        // a mod's Stop canceled: neither.
        assert_eq!(release_parts(true, false), (false, false));
        // another item in the hand: no release (Horizon still lowers a spyglass).
        assert_eq!(release_parts(false, true), (false, true));
    }

    #[test]
    fn bow_release_respects_charge_cancel_and_ammo() {
        assert_eq!(bow_release_power(true, 20, false), Some(1.0));
        assert_eq!(bow_release_power(true, 10, false), Some(5.0 / 12.0));
        assert_eq!(bow_release_power(true, 0, false), None);
        assert_eq!(bow_release_power(true, -1, false), None);
        assert_eq!(bow_release_power(true, 20, true), None);
        assert_eq!(bow_release_power(false, 20, false), None);
    }

    #[test]
    fn a_moved_end_moves_the_charge() {
        // a bow held 10 ticks of 72000: 10, as vanilla's.
        assert_eq!(held(72_000, ticks_left(72_000, 10)), 10);
        // a mod's Tick leaves 71000 at tick 10: it counts as held 1000.
        assert_eq!(
            held(72_000, ticks_left(duration_leaving(10, 71_000), 10)),
            1000
        );
        // more ticks left than the item's own time: not held at all.
        assert_eq!(held(72_000, 80_000), 0);
    }

    #[test]
    fn a_release_tells_how_long_the_item_was_held() {
        // a bow-like item held 25 ticks of 72000: the time left gives the charge back.
        let left = ticks_left(72_000, 25);
        assert_eq!(left, 71_975);
        assert_eq!(72_000 - left, 25);
        // released at once.
        assert_eq!(ticks_left(72_000, 0), 72_000);
    }

    #[test]
    fn a_food_of_no_time_is_eaten_at_once() {
        assert!(consumes_at_once(0));
        assert!(consumes_at_once(-3));
        assert!(!consumes_at_once(1));
        assert!(!consumes_at_once(32));
    }

    #[test]
    fn a_use_animation_picks_the_hand_pose() {
        let pose = |animation| use_pose(animation, 12.0, 32.0, 0.5);
        assert!(matches!(
            pose(UseAnimation::Drink),
            ItemPose::Eat { remaining, duration, partial }
                if (remaining, duration, partial) == (12.0, 32.0, 0.5)
        ));
        // ticks used: 32 - (12 - 0.5 + 1).
        assert!(
            matches!(pose(UseAnimation::Bow), ItemPose::Bow { ticks_used } if ticks_used == 19.5)
        );
        assert!(matches!(
            pose(UseAnimation::Trident),
            ItemPose::Trident { ticks_used } if ticks_used == 19.5
        ));
        assert!(matches!(
            pose(UseAnimation::Brush),
            ItemPose::Brush { remaining, partial } if (remaining, partial) == (12.0, 0.5)
        ));
        for animation in [
            UseAnimation::None,
            UseAnimation::Block,
            UseAnimation::Spyglass,
        ] {
            assert!(matches!(pose(animation), ItemPose::Using));
        }
    }
}

#[cfg(test)]
pub(super) mod interact_vibration_tests {
    use super::*;

    type RecordedEvent = (&'static str, [f64; 3], Option<horizon_client::mobs::Who>);
    thread_local! {
        static EVENTS: std::cell::RefCell<Vec<RecordedEvent>> = const { std::cell::RefCell::new(Vec::new()) };
        static RECORDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    pub(in crate::game) fn record_event(
        event: &'static str,
        pos: [f64; 3],
        source: Option<horizon_client::mobs::Who>,
    ) {
        if RECORDING.get() {
            EVENTS.with(|events| events.borrow_mut().push((event, pos, source)));
        }
    }

    /// Runs `f` on a windowless game built from the prepared resources with
    /// `held` in the main hand and returns the game events it posted;
    /// `None` without the resources.
    fn with_game(held: &str, f: impl FnOnce(&mut Game<'_>)) -> Option<Vec<String>> {
        use crate::game::{Audio, Options, Progress, Settings, load_resources};
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES")?);
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .ok()?;
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.player = Some(
            horizon_client::motion::PlayerMotion::new(
                &game.motion,
                horizon_core::geometry::Vec3::new(10.5, 64.0, -3.25),
            )
            .unwrap(),
        );
        *game.held_mut() = Some(ItemStack::new(held, 1));
        crate::game::dispense_fixture::start_recording();
        RECORDING.set(true);
        f(&mut game);
        RECORDING.set(false);
        Some(crate::game::dispense_fixture::stop_recording())
    }

    /// `LivingEntity.startUsingItem` ends with
    /// `this.useItem.causeUseVibration(this, GameEvent.ITEM_INTERACT_START)`,
    /// and `ItemStack.causeUseVibration` posts only when the stack's use effects
    /// ask for interaction vibrations (the default). `stopUsingItem` posts the
    /// finish the same way, which Horizon already did, so a use is a pair of
    /// vibrations and an item that turns them off is neither.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_use_start_is_an_item_interact_start_vibration() {
        let bow = || ItemStack::new("minecraft:bow", 1);
        let Some(lines) = with_game("minecraft:bow", |game| {
            assert!(game.start_using_item(), "the bow's use starts");
            game.stop_using_item(&bow());
        }) else {
            return;
        };
        assert_eq!(
            lines,
            vec![
                "G 10,64,-4 minecraft:item_interact_start".to_owned(),
                "G 10,64,-4 minecraft:item_interact_finish".to_owned(),
            ]
        );
        // A spear's use effects set `interact_vibrations` false: no vibration.
        let spear = || ItemStack::new("minecraft:iron_spear", 1);
        let Some(lines) = with_game("minecraft:iron_spear", |game| {
            assert!(game.start_using_item(), "the spear's use starts");
            game.stop_using_item(&spear());
        }) else {
            return;
        };
        assert!(lines.is_empty(), "{lines:?}");
    }

    #[test]
    fn stack_use_effects_patch_controls_vibrations() {
        let mut rod = ItemStack::new("minecraft:fishing_rod", 1);
        assert!(interact_vibrations(&rod, Some(true)));
        rod.data_mut().set_loot_component(
            "minecraft:use_effects",
            &serde_json::json!({"interact_vibrations": false}),
        );
        assert!(!interact_vibrations(&rod, Some(true)));
        rod.data_mut()
            .set_loot_component("minecraft:use_effects", &serde_json::json!({}));
        assert!(interact_vibrations(&rod, Some(false)));
        rod.data_mut().set_other("minecraft:use_effects", None);
        rod.data_mut().set_other(
            "!minecraft:use_effects",
            Some(horizon_world::nbt::Tag::Compound(Default::default())),
        );
        assert!(!interact_vibrations(&rod, Some(true)));
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn fishing_cast_and_reel_post_player_vibrations() {
        use horizon_client::mobs::Who;
        let Some(_) = with_game("minecraft:fishing_rod", |game| {
            let rod = ItemStack::new("minecraft:fishing_rod", 1);
            EVENTS.with(|events| events.borrow_mut().clear());
            assert!(game.use_fishing_rod(&rod));
            assert!(game.use_fishing_rod(&rod));
            EVENTS.with(|events| {
                assert_eq!(
                    *events.borrow(),
                    vec![
                        (
                            "minecraft:item_interact_start",
                            [10.5, 64.0, -3.25],
                            Some(Who::Player)
                        ),
                        (
                            "minecraft:item_interact_finish",
                            [10.5, 64.0, -3.25],
                            Some(Who::Player)
                        ),
                    ]
                );
            });
            let mut silent = rod.clone();
            silent.data_mut().set_loot_component(
                "minecraft:use_effects",
                &serde_json::json!({"interact_vibrations": false}),
            );
            *game.held_mut() = Some(silent.clone());
            EVENTS.with(|events| events.borrow_mut().clear());
            assert!(game.use_fishing_rod(&silent));
            assert!(game.use_fishing_rod(&silent));
            EVENTS.with(|events| assert!(events.borrow().is_empty()));
            *game.held_mut() = Some(rod.clone());
            assert!(game.use_fishing_rod(&rod));
            game.hook
                .as_mut()
                .unwrap()
                .move_to([10.5, 64.0, -3.25], true);
            let max = game
                .items()
                .get("minecraft:fishing_rod")
                .unwrap()
                .max_damage;
            game.held_mut().as_mut().unwrap().damage = max - 1;
            EVENTS.with(|events| events.borrow_mut().clear());
            assert!(game.use_fishing_rod(&rod));
            EVENTS.with(|events| assert!(events.borrow().is_empty(), "{:?}", *events.borrow()));
        }) else {
            panic!("prepared resources failed to load")
        };
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn thrown_eye_posts_projectile_shoot_at_eye_from_player() {
        use horizon_client::mobs::Who;
        let Some(_) = with_game("minecraft:ender_eye", |game| {
            let root =
                std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
            let terrain = horizon_client::vanillagen::VanillaTerrain::new(
                &root.join("worldgen"),
                game.pack.assets.registry,
                &game.res.core.biomes,
                1234,
            )
            .unwrap();
            game.end.set_locator(Some(std::sync::Arc::new(terrain)));
            EVENTS.with(|events| events.borrow_mut().clear());
            assert!(
                game.use_ender_eye(&ItemStack::new("minecraft:ender_eye", 1), false)
                    .unwrap()
            );
            EVENTS.with(|events| {
                let events = events.borrow();
                assert!(
                    events.contains(&(
                        "minecraft:projectile_shoot",
                        [10.5, 64.0 + f64::from(1.8_f32) * 0.5, -3.25],
                        Some(Who::Player),
                    )),
                    "{events:?}"
                );
            });
        }) else {
            panic!("prepared resources failed to load")
        };
    }
}
