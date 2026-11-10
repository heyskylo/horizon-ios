//! Shields and other `blocks_attacks` items: raising them (`ShieldItem.use`,
//! from either hand), `LivingEntity.getItemBlockingWith` and
//! `applyItemBlocking` inputs, and what follows a block: item damage,
//! the block sound, attacker knockback (`blockedByItem`) and axes disabling
//! the shield (`Player.blockUsingItem`, `BlocksAttacks.disable`).
use super::Game;
use horizon_client::{
    health::{BlockOutcome, Blocking, HurtSource},
    items::ItemStack,
};
use horizon_core::random::RandomSource;
use horizon_world::blocking_rules::{BlockingRules, DamageReduction, ItemDamageFunction};

/// `Inventory.SLOT_OFFHAND`: the `using` slot of an off-hand item.
pub(super) const OFFHAND_SLOT: usize = 40;

impl Game<'_> {
    /// The stack in an inventory slot, the off hand at `OFFHAND_SLOT`.
    pub(super) fn stack_in(&self, slot: usize) -> Option<&ItemStack> {
        if slot == OFFHAND_SLOT {
            self.offhand.as_ref()
        } else {
            self.inventory.get(slot)?.as_ref()
        }
    }
    pub(super) fn stack_in_mut(&mut self, slot: usize) -> &mut Option<ItemStack> {
        if slot == OFFHAND_SLOT {
            &mut self.offhand
        } else {
            &mut self.inventory[slot]
        }
    }
    /// The stack's `blocks_attacks`: its own component, else its item's.
    pub(super) fn blocks_attacks(
        &self,
        stack: &ItemStack,
    ) -> Option<std::borrow::Cow<'_, horizon_client::items::BlocksAttacks>> {
        horizon_client::items::BlocksAttacks::of_stack(stack, self.items().get(&stack.item))
    }
    /// `ShieldItem.use` (ItemUtils.startUsingInstantly) for the hand at `slot`;
    /// false when that hand holds no blocking item or it is on cooldown.
    pub(super) fn raise_shield(&mut self, slot: usize) -> bool {
        let Some(stack) = self.stack_in(slot) else {
            return false;
        };
        if self.blocks_attacks(stack).is_none() {
            return false;
        }
        if self.shield_cooldown == 0 {
            self.start_using_item();
        }
        true
    }
    /// `LivingEntity.getItemBlockingWith` and the inputs of `applyItemBlocking`
    /// for an attack from `from` (`DamageSource.getSourcePosition`).
    pub(super) fn player_blocking(
        &self,
        from: Option<[f64; 3]>,
        damage_type: &str,
        pierce: bool,
    ) -> Option<Blocking> {
        let (slot, ticks) = self.using?;
        let component = self.blocks_attacks(self.stack_in(slot)?)?;
        // BlocksAttacks.blockDelayTicks: Math.round(seconds * 20).
        if (ticks as i32) < (component.delay_seconds * 20.0).round() as i32 {
            return None;
        }
        let tags = &self.stations().enchantments.damage_tags;
        let matches = |spec: &str| match spec.strip_prefix('#') {
            Some(tag) => tags.get(tag).is_some_and(|t| t.contains(damage_type)),
            None => spec == damage_type,
        };
        let p = self.player.as_ref()?;
        let angle = match from {
            Some(src) => {
                // calculateViewVector(0, yHeadRot) against the flattened direction.
                let yaw = (-self.yaw).to_radians();
                let view = [yaw.sin() as f64, yaw.cos() as f64];
                let (dx, dz) = (src[0] - p.position.x, src[2] - p.position.z);
                let len = (dx * dx + dz * dz).sqrt();
                let to = if len < 1.0e-4 {
                    [0.0, 0.0]
                } else {
                    [dx / len, dz / len]
                };
                (to[0] * view[0] + to[1] * view[1]).acos()
            }
            None => f64::from(std::f32::consts::PI),
        };
        let (threshold, base, factor) = component.item_damage;
        Some(Blocking {
            rules: BlockingRules {
                delay_seconds: component.delay_seconds,
                disable_scale: component.disable_scale,
                reductions: component
                    .reductions
                    .iter()
                    .map(|r| DamageReduction {
                        horizontal_angle: r.horizontal_angle,
                        types: r.types.as_ref().map(|t| t.iter().any(|s| matches(s))),
                        base: r.base,
                        factor: r.factor,
                    })
                    .collect(),
                item_damage: ItemDamageFunction {
                    threshold,
                    base,
                    factor,
                },
            },
            bypassed: component.bypassed_by.as_deref().is_some_and(matches),
            pierce,
            angle,
        })
    }
    /// After a hurt with the shield raised: `hurtBlockingItem`, `onBlocked`
    /// and, for melee from a mob, `blockUsingItem`. `source` is the hit's.
    pub(super) fn after_block(
        &mut self,
        outcome: BlockOutcome,
        (attacker, projectile): (Option<u32>, bool),
        source: HurtSource<'_>,
    ) {
        let Some((slot, _)) = self.using else { return };
        let Some(stack) = self.stack_in(slot).cloned() else {
            return;
        };
        let Some(component) = self
            .blocks_attacks(&stack)
            .map(std::borrow::Cow::into_owned)
        else {
            return;
        };
        let at = self
            .player
            .as_ref()
            .map(|p| [p.position.x, p.position.y, p.position.z]);
        if outcome.sound
            && let Some(sound) = &component.block_sound
        {
            let pitch = 0.8 + self.world.random.next_float() * 0.4;
            self.audio.play(sound, at, 1.0, pitch);
        }
        // BlocksAttacks.hurtBlockingItem: the hit counts the blocking item as
        // used, before the blocked-damage gate (vanilla awards it on the
        // server even when the item blocks none of the damage).
        if self.remote.is_none() {
            self.award_stat("used", &stack.item, 1);
        }
        if outcome.blocked <= 0.0 {
            return;
        }
        // ItemDamageFunction.apply, then hurtAndBreak.
        let (threshold, base, factor) = component.item_damage;
        let item_damage = if outcome.blocked < threshold {
            0
        } else {
            (base + factor * outcome.blocked).floor() as i32
        };
        if item_damage > 0
            && !self.creative
            && let Some(max) = blocking_item_max_damage(
                &stack,
                self.items()
                    .get(&stack.item)
                    .map_or(0, |item| item.max_damage),
            )
        {
            let loss = self.wear(&stack, item_damage as u32);
            let cell = self.stack_in_mut(slot);
            if loss > 0
                && let Some(s) = cell.as_mut()
            {
                s.damage = s.damage.saturating_add(loss).min(max);
                if s.damage >= max {
                    let broken = s.clone();
                    s.count -= 1;
                    if s.count == 0 {
                        *cell = None;
                    }
                    self.using = None;
                    // ItemStack.hurtAndBreak: onEquippedItemBroken for the hand it was used from.
                    let hand = if slot == OFFHAND_SLOT {
                        horizon_world::player_inventory::EquipmentSlot::OffHand
                    } else {
                        horizon_world::player_inventory::EquipmentSlot::MainHand
                    };
                    self.player_equipment_broke(hand, &broken);
                    return;
                }
            }
        }
        if projectile {
            return;
        }
        let Some(id) = attacker else {
            // Another player's melee hit has no mob behind it.
            if let Some((from, disable)) = self.combat.player_attacker {
                let fully = outcome.fully_blocked;
                let attacker = ([from[0], from[2]], disable);
                self.blocked_by_attacker(&component, attacker, fully, at, source);
            }
            return;
        };
        let fully = outcome.fully_blocked;
        let Some(m) = self.mobs.iter().find(|m| m.id == id) else {
            return;
        };
        let (mx, mz) = (m.position.x, m.position.z);
        // LivingEntity.getSecondsToDisableBlocking: the held weapon's value;
        // Warden overrides it with a flat 5 seconds.
        let disable = if m.species() == horizon_client::mobs::Species::Warden {
            5.0
        } else {
            m.extra
                .main_hand
                .as_deref()
                .and_then(|item| self.items().get(item))
                .map_or(0.0, |i| i.disable_blocking_seconds)
        };
        // Ravager.blockedByItem: stunned half the time (the defender pushed
        // apart), else the defender flung away; no roar in progress.
        if m.species() == horizon_client::mobs::Species::Ravager {
            let mut events = Vec::new();
            let roaring = m.extra.anim_ticks != 0;
            let stunned = self
                .mobs
                .iter_mut()
                .find(|m| m.id == id)
                .is_some_and(|m| horizon_client::mobs::ravager::blocked(m, &mut events));
            if !roaring && let Some(p) = self.player.as_mut() {
                let (xd, zd) = (p.position.x - mx, p.position.z - mz);
                let push = if stunned {
                    // Entity.push(entity): apart by 0.05 per block of overlap.
                    let d = (xd * xd + zd * zd).sqrt().max(0.01);
                    let k = (1.0 / d).min(1.0) * 0.05;
                    horizon_core::geometry::Vec3::new(xd / d * k, 0.0, zd / d * k)
                } else {
                    let dd = (xd * xd + zd * zd).max(0.001);
                    horizon_core::geometry::Vec3::new(xd / dd * 4.0, 0.2, zd / dd * 4.0)
                };
                p.velocity = p.velocity.add_vector(push);
            }
            let _ = self.handle_mob_events(events);
            return;
        }
        self.blocked_by_attacker(&component, ([mx, mz], disable), fully, at, source);
    }

    /// What a blocked melee hit does to the defender once the shield took
    /// it: `LivingEntity.blockedByItem` knocks them back unless fully
    /// blocked, then `Player.blockUsingItem` disables the shield. `attacker`
    /// is where the attacker stands (x, z) and its seconds of disabling.
    fn blocked_by_attacker(
        &mut self,
        component: &horizon_client::items::BlocksAttacks,
        ([mx, mz], disable): ([f64; 2], f32),
        fully: bool,
        at: Option<[f64; 3]>,
        source: HurtSource<'_>,
    ) {
        // LivingEntity.blockedByItem: defender.knockback(0.5, defender - attacker,
        // source), which moves the defender along -(xd, zd).
        if !fully && let Some(p) = self.player.as_ref() {
            let (xd, zd) = (p.position.x - mx, p.position.z - mz);
            self.knock_back_player(0.5, xd, zd, source);
        }
        // BlocksAttacks.disable: item cooldown and stopUsingItem.
        let ticks = disable * component.disable_scale;
        let ticks = if ticks > 0.0 {
            (ticks * 20.0).round() as u32
        } else {
            0
        };
        if ticks > 0 {
            if let (Some(player), Some(name)) = (self.lan_acting_uuid, self.lan_acting_name.clone())
            {
                self.native_player_shield_disable(player, &name, ticks);
            }
            let used = self
                .using
                .and_then(|(slot, _)| self.stack_in(slot))
                .cloned();
            let item = used.as_ref().map(|stack| stack.item.clone());
            let mut cancel = false;
            if let (Some(player), Some(name), Some(item)) =
                (self.lan_acting_uuid, self.lan_acting_name.clone(), item)
            {
                self.native_player_item_cooldown(player, &name, &item, ticks as i32, &mut cancel);
            }
            if !cancel {
                self.shield_cooldown = ticks;
                // ItemCooldowns.addCooldown(stack, ticks) on the shield's group: a guest is
                // sent it (ServerItemCooldowns), which keeps the shield down on their screen,
                // and the 0 that ends it.
                if let Some(stack) = &used {
                    let default = self
                        .items()
                        .get(&stack.item)
                        .and_then(|i| i.use_cooldown.clone());
                    let group = horizon_client::cooldowns::group(stack, default.as_ref());
                    self.add_cooldown(group, ticks as i32);
                }
            }
            self.using = None;
            if let Some(sound) = &component.disabled_sound {
                let pitch = 0.8 + self.world.random.next_float() * 0.4;
                self.audio.play(sound, at, 0.8, pitch);
            }
        }
    }
}

/// `ItemStack.isDamageableItem` and `getMaxDamage` for the blocking stack.
fn blocking_item_max_damage(stack: &ItemStack, prototype_max: u32) -> Option<u32> {
    let data = stack.data();
    let component = |id| data.and_then(|data| data.other(id));
    if stack.count == 0
        || component("minecraft:unbreakable").is_some()
        || component("!minecraft:max_damage").is_some()
        || component("!minecraft:damage").is_some()
    {
        return None;
    }
    // A non-damageable prototype supplies neither MAX_DAMAGE nor DAMAGE.
    if prototype_max == 0
        && stack.damage == 0
        && !data.is_some_and(|data| data.explicit_zero_damage)
    {
        return None;
    }
    let max = match component("minecraft:max_damage") {
        Some(value) => horizon_world::datafix::dynamic::as_int(Some(value), 0).max(0) as u32,
        None => prototype_max,
    };
    (max > 0).then_some(max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::native_machines::tests::server_game;
    use horizon_client::items::ItemStack;

    /// `BlocksAttacks.hurtBlockingItem`: a hit taken while using a shield
    /// counts it as used, even when the shield blocks none of the damage.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and scratch homes/cwd"]
    fn a_blocked_hit_counts_the_shield_as_used() {
        let mut game = server_game();
        game.build_registries();
        game.offhand = Some(ItemStack::new("minecraft:shield", 1));
        game.using = Some((OFFHAND_SLOT, 0));
        let source = HurtSource {
            damage_type: "minecraft:mob_attack",
            causing: 1,
            direct: 1,
        };
        let outcome = BlockOutcome {
            hurt: true,
            blocked: 5.0,
            fully_blocked: true,
            sound: false,
            knockback: false,
            canceled: false,
        };
        assert_eq!(game.stats.get("used", "minecraft:shield"), 0);
        game.after_block(outcome, (None, false), source);
        assert_eq!(
            game.stats.get("used", "minecraft:shield"),
            1,
            "a hit the shield takes whole counts it"
        );
        // The gate below the award: a hit the shield blocks nothing of still
        // counts it (hurtBlockingItem runs before the `blocked > 0` branch).
        let outcome = BlockOutcome {
            blocked: 0.0,
            ..outcome
        };
        game.after_block(outcome, (None, false), source);
        assert_eq!(game.stats.get("used", "minecraft:shield"), 2);
    }
}
