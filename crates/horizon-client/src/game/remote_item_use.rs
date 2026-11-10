//! Predicting item-on-block interactions on the client level, out of remote.rs,
//! which may not grow.
use super::super::Game;
use horizon_client::first_person::Hand;
use horizon_client::items::ItemStack;

impl Game<'_> {
    /// `FlintAndSteelItem.useOn` / `FireChargeItem.useOn` on this client's level:
    /// lighting an unlit campfire, candle or candle cake, or placing fire at the hit face.
    /// True when it succeeded (SUCCESS, swings and ends the click); false when
    /// placement is rejected (FAIL, no swing, ends the click).
    pub(super) fn predict_fire_starter(
        &mut self,
        stack: &ItemStack,
        hit: &super::super::Hit,
        hand: Hand,
        seq: i32,
    ) -> bool {
        let is_fire_charge = &*stack.item == "minecraft:fire_charge";
        let clicked = self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]);
        let name = self.block_name(hit.pos);
        // TntBlock.useItemOn primes TNT unless sneaking with hands full.
        let sneaking = self.remote_shift_down();
        let hands_full = self.stack_in(self.selected).is_some() || self.offhand.is_some();
        if !(sneaking && hands_full) && &*name == "minecraft:tnt" {
            let sound = horizon_client::items::fire_starter_sound(is_fire_charge);
            let pitch =
                horizon_client::items::fire_starter_pitch(is_fire_charge, &mut self.world.random);
            let at = [
                f64::from(hit.pos[0]) + 0.5,
                f64::from(hit.pos[1]) + 0.5,
                f64::from(hit.pos[2]) + 0.5,
            ];
            self.audio.play(sound, Some(at), 1.0, pitch);
            if is_fire_charge && !self.creative {
                self.with_hand(hand, Game::consume_held);
            }
            return true;
        }
        if self.can_light(clicked, &name) {
            let registry = self.pack.assets.registry;
            if let Some(mut state) = registry.state(clicked).cloned() {
                state.properties.insert("lit".into(), "true".into());
                if let Ok(lit_state) = registry.resolve(&state) {
                    let pos =
                        horizon_core::position::BlockPos::new(hit.pos[0], hit.pos[1], hit.pos[2]);
                    self.predict_blocks(&vec![(pos, lit_state)], seq);
                }
            }
            let at = [
                f64::from(hit.pos[0]) + 0.5,
                f64::from(hit.pos[1]) + 0.5,
                f64::from(hit.pos[2]) + 0.5,
            ];
            let sound = if is_fire_charge {
                "minecraft:item.firecharge.use"
            } else {
                "minecraft:item.flintandsteel.use"
            };
            let pitch =
                horizon_client::items::fire_starter_pitch(is_fire_charge, &mut self.world.random);
            self.audio.play(sound, Some(at), 1.0, pitch);
            if is_fire_charge && !self.creative {
                self.with_hand(hand, Game::consume_held);
            }
            return true;
        }
        let target = [
            hit.pos[0] + hit.face[0],
            hit.pos[1] + hit.face[1],
            hit.pos[2] + hit.face[2],
        ];
        if self.can_place_used_fire(target) {
            let pos = horizon_core::position::BlockPos::new(target[0], target[1], target[2]);
            let state = self.with_level(|b, level, ticks| b.fire_state_for(level, ticks, pos));
            self.predict_blocks(&vec![(pos, state)], seq);
            let sound = horizon_client::items::fire_starter_sound(is_fire_charge);
            let pitch =
                horizon_client::items::fire_starter_pitch(is_fire_charge, &mut self.world.random);
            let target_at = [
                f64::from(target[0]) + 0.5,
                f64::from(target[1]) + 0.5,
                f64::from(target[2]) + 0.5,
            ];
            self.audio.play(sound, Some(target_at), 1.0, pitch);
            if is_fire_charge && !self.creative {
                self.with_hand(hand, Game::consume_held);
            }
            return true;
        }
        false
    }

    /// `Item.useOn` on this client's level for the items whose effects are
    /// the clicker's own (`Level.levelEvent(player, ...)` and
    /// `playSound(player, ...)`: the server sends them to the others only):
    /// honeycomb waxing copper, and an axe, hoe or shovel transforming a
    /// block (stripping, scraping, removing wax, tilling, making a path).
    /// A cushion succeeds where it may be placed and spawns on the server, a
    /// firework rocket wherever it may be used, and the server launches it.
    /// Whether the item's use succeeded, which ends the click.
    pub(super) fn predict_item_on_block(
        &mut self,
        stack: &ItemStack,
        hit: &super::super::Hit,
        hand: Hand,
    ) -> bool {
        // FireworkRocketItem.useOn: SUCCESS on the client too, so neither
        // UseItem nor the off hand follows. It passes while gliding (the use
        // boosts instead), cooling down (MultiPlayerGameMode.performUseItemOn
        // skips useOn) and in adventure mode off its can_place_on
        // (ItemStack.useOn).
        if &*stack.item == "minecraft:firework_rocket" {
            let cooling = self.items().get(&stack.item).is_some_and(|info| {
                let group = horizon_client::cooldowns::group(stack, info.use_cooldown.as_ref());
                self.cooldowns.is_on_cooldown(&group)
            });
            let gliding = self.player.as_ref().is_some_and(|p| p.fall_flying);
            return !cooling
                && !gliding
                && (self.may_build()
                    || self.with_hand(hand, |g| g.can_place_on_in_adventure(hit.pos)));
        }
        let at = [
            f64::from(hit.pos[0]) + 0.5,
            f64::from(hit.pos[1]) + 0.5,
            f64::from(hit.pos[2]) + 0.5,
        ];
        if &*stack.item == "minecraft:honeycomb" {
            // HoneycombItem.useOn.
            let registry = self.pack.assets.registry;
            let clicked = self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]);
            let old = registry.state(clicked);
            let waxes = old
                .and_then(|old| self.res.transformers().waxed(registry, old))
                .is_some();
            if waxes {
                self.level_event_at(3003, hit.pos, 0);
                self.audio
                    .play("minecraft:item.honeycomb.wax_on", Some(at), 1.0, 1.0);
                // The other half of a double chest: its particles, and a second
                // sound from the clicked block.
                if let Some(other) =
                    old.and_then(|old| super::super::storage::chest_connected_pos(old, hit.pos))
                {
                    self.level_event_at(3003, other, 0);
                    self.audio
                        .play("minecraft:item.honeycomb.wax_on", Some(at), 1.0, 1.0);
                }
            }
            waxes
        } else if let Some((_, sound, _, _, particle, _)) =
            self.block_transform(stack, hit.pos, hit.face)
        {
            self.transform_particle(&particle, hit.pos);
            self.audio.play(&sound, Some(at), 1.0, 1.0);
            true
        } else if self
            .res
            .items()
            .get(&stack.item)
            .is_some_and(|i| i.cushion_color.is_some())
        {
            // CushionItem.useOn on a client level: SUCCESS once
            // `canBePlacedAt` holds, before the server's own checks.
            self.cushion_spawn(hit).is_some()
        } else if &*stack.item == "minecraft:painting" {
            // HangingEntityItem.useOn on a client level: SUCCESS once
            // `Painting.create` finds a surviving variant, before the
            // server spawns it.
            self.painting_fits(hit, stack)
        } else {
            false
        }
    }

    /// `MapItem.useOn` on this client: a map used on a banner is SUCCESS
    /// whatever its data says (the server toggles the marker, or fails
    /// quietly), so this hand swings and the click ends. `ItemStack.useOn`
    /// passes in adventure mode.
    pub(super) fn predict_map_on_banner(&self, stack: &ItemStack, hit: &super::super::Hit) -> bool {
        super::super::maps::map_id(stack).is_some()
            && self.may_build()
            && self.block_in_tag("minecraft:banners", &self.block_name(hit.pos))
    }
}
