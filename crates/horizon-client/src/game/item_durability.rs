//! Durability lost by what the player wears and holds: armour hurt with its wearer
//! (`wear_armor`, `hurt_armor`, `hurt_equipment`, `hurt_player_equipment`) and the held stack's wear
//! (`damage_held` and its variants, `apply_held_damage`, `hurt_held_without_breaking`).
use super::*;

impl Game<'_> {
    /// `LivingEntity.hurtServer` with the two equipment services performed
    /// synchronously. Keeping the acting player installed lets durability
    /// hooks observe and modify its real inventory before this hit continues.
    pub(super) fn hurt_player_equipment(
        &mut self,
        source: horizon_world::damage_source::DamageTypeId,
        amount: f32,
        blocking: Option<horizon_client::health::Blocking>,
        living_cause: bool,
        causing: i32,
    ) -> horizon_client::Result<(horizon_client::health::BlockOutcome, f32)> {
        let mut hit = self
            .player
            .as_mut()
            .ok_or("missing damage target")?
            .prepare_equipment_hurt(
                &self.health_resources,
                self.world.health_rules,
                source,
                amount,
                blocking,
                living_cause,
                causing,
            )?;
        if let Some(damage) = hit.helmet_damage()
            && self.armor[0]
                .as_ref()
                .is_some_and(|s| s.count > 0 && &*s.item != "minecraft:air")
        {
            self.hurt_equipment(source, damage, &[0]);
            hit.helmet_worn();
            self.refresh_armor_protection();
        }
        let actual = hit.damage().unwrap_or(amount);
        let armor_damage = self
            .player
            .as_mut()
            .ok_or("missing damage target")?
            .select_equipment_hurt(&self.health_resources, &mut hit)?;
        if let Some(damage) = armor_damage {
            self.hurt_armor(source, damage);
            self.refresh_armor_protection();
        }
        let outcome = self
            .player
            .as_mut()
            .ok_or("missing damage target")?
            .finish_equipment_hurt(&self.health_resources, hit)?;
        Ok((outcome, actual))
    }
    /// `push_seen_with_hurt` for the host: the push is kept only when the hurt
    /// marks the velocity as seen, and the hurt goes through
    /// `hurt_player_equipment` so the worn armor loses durability. Answers
    /// whether a native hook canceled the hurt.
    pub(super) fn push_seen_with_equipment_hurt(
        &mut self,
        push: Vec3,
        source: Option<horizon_world::damage_source::DamageTypeId>,
        amount: f32,
    ) -> bool {
        let Some(p) = self.player.as_mut() else {
            return false;
        };
        let before = p.velocity;
        let marked_before = std::mem::take(&mut p.health.sync_velocity);
        p.velocity = before.add_vector(push);
        let canceled = source
            .and_then(|id| self.hurt_player_equipment(id, amount, None, false, 0).ok())
            .is_some_and(|(o, _)| o.canceled);
        if let Some(p) = self.player.as_mut() {
            if !p.health.sync_velocity {
                p.velocity = before;
            }
            p.health.sync_velocity |= marked_before;
        }
        canceled
    }
    /// The armor hits among a tick's motion events (`Player.hurtArmor` from
    /// `getDamageAfterArmorAbsorb`), whatever the damage: a mob, an explosion,
    /// a cactus, lava, a campfire.
    pub(super) fn wear_armor(&mut self, events: &[MotionEvent]) {
        for event in events {
            if let MotionEvent::Health(horizon_client::health::HealthEvent::ArmorHurt {
                source,
                amount,
            }) = event
            {
                self.hurt_armor(*source, *amount);
            }
        }
    }
    /// `Player.hurtArmor` -> `LivingEntity.doHurtEquipment`: each worn piece that
    /// wears when its wearer is hurt loses max(1, damage / 4) durability, except
    /// to a fire source when the piece resists fire (`ItemStack.canBeHurtBy`).
    pub(super) fn hurt_armor(
        &mut self,
        source: horizon_world::damage_source::DamageTypeId,
        damage: f32,
    ) {
        self.hurt_equipment(source, damage, &[3, 2, 1, 0]);
    }
    /// Vanilla visits FEET, LEGS, CHEST, HEAD, applying each break before the
    /// next slot's durability hook and enchantment random draws.
    pub(super) fn hurt_equipment(
        &mut self,
        source: horizon_world::damage_source::DamageTypeId,
        damage: f32,
        slots: &[usize],
    ) {
        if damage <= 0.0 {
            return;
        }
        // The maximum of 1.0F and damage / 4.0F, then Java's saturating float-to-int.
        let amount = if damage.is_nan() {
            0
        } else {
            (damage / 4.0).max(1.0) as i32
        };
        if amount <= 0 {
            return;
        }
        let amount = amount as u32;
        let name = self
            .health_resources
            .damage
            .definition(source)
            .map(|d| d.name.clone())
            .unwrap_or_default();
        let fire = self
            .health_resources
            .damage
            .in_tag(source, "minecraft:is_fire");
        for &i in slots {
            let Some(stack) = self.armor[i].clone() else {
                continue;
            };
            let Some(info) = world_items!(self).get(&stack.item) else {
                continue;
            };
            let component = |id| stack.data().and_then(|data| data.other(id));
            // ItemStack.isDamageableItem: component removal and
            // unbreakable are checked before enchantment RNG or hooks.
            if stack.count == 0
                || !info.equip_damage_on_hurt
                || component("minecraft:unbreakable").is_some()
                || component("!minecraft:max_damage").is_some()
                || component("!minecraft:damage").is_some()
                || (fire && world_items!(self).resists_fire(&stack, &name))
            {
                continue;
            }
            let max = match component("minecraft:max_damage") {
                Some(horizon_world::nbt::Tag::Int(value)) => match u32::try_from(*value) {
                    Ok(value) => value,
                    Err(_) => continue,
                },
                _ => info.max_damage,
            };
            if max == 0 {
                continue;
            }
            let loss = self.wear(&stack, amount);
            if loss == 0 {
                continue;
            }
            let Some(worn) = self.armor[i].as_mut() else {
                continue;
            };
            worn.damage = worn.damage.saturating_add(loss).min(max);
            if worn.damage >= max {
                let broken = worn.clone();
                self.armor[i] = None;
                self.player_equipment_broke(armor_slot(i), &broken);
                self.apply_equipment();
            }
        }
    }
    pub(super) fn damage_held(&mut self, amount: u32) {
        self.damage_held_in_slot(
            amount,
            horizon_world::player_inventory::EquipmentSlot::MainHand,
        );
    }
    /// Hurt the acting hand's stack through ordinary wear/criterion/break handling,
    /// reporting its break in the caller's explicit equipment slot.
    pub(super) fn damage_held_in_slot(
        &mut self,
        amount: u32,
        break_slot: horizon_world::player_inventory::EquipmentSlot,
    ) {
        if self.creative {
            return;
        }
        let amount = match self.held().cloned() {
            Some(held) => self.wear(&held, amount),
            None => amount,
        };
        let max = self.held().map_or(0, |s| {
            if &*s.item == "minecraft:trident" {
                self.trident_max_damage(s)
            } else {
                self.items().stack_max_damage(s)
            }
        });
        self.apply_held_damage(amount, max, break_slot);
    }
    pub(super) fn damage_held_with_max(&mut self, amount: u32, max: u32) {
        if self.creative {
            return;
        }
        let amount = match self.held().cloned() {
            Some(held) => self.wear(&held, amount),
            None => amount,
        };
        self.apply_held_damage(
            amount,
            max,
            horizon_world::player_inventory::EquipmentSlot::MainHand,
        );
    }
    /// `ItemStack.hurtAndBreak`: apply nonzero processed wear to the acting hand.
    pub(super) fn apply_held_damage(
        &mut self,
        amount: u32,
        max: u32,
        break_slot: horizon_world::player_inventory::EquipmentSlot,
    ) {
        // ItemStack.hurtAndBreak does not apply damage or break a stack when
        // processDurabilityChange leaves no wear (including Unbreaking).
        if amount == 0 {
            return;
        }
        let cell = self.held_mut();
        if let Some(stack) = cell.as_mut()
            && max > 0
        {
            stack.damage += amount;
            if stack.damage >= max {
                let gone = stack.clone();
                *cell = None;
                // LivingEntity.onEquippedItemBroken: the sound and the crumbs.
                self.player_equipment_broke(break_slot, &gone);
            }
        }
    }
    /// `ItemStack.hurtWithoutBreaking`: Unbreaking applies, but the stack stops at
    /// one point of durability left instead of breaking.
    pub(super) fn hurt_held_without_breaking(&mut self, amount: u32) {
        if self.creative {
            return;
        }
        let Some(held) = self.held().cloned() else {
            return;
        };
        let amount = self.wear(&held, amount);
        let max = self.trident_max_damage(&held);
        if amount == 0 || max == 0 {
            return;
        }
        if let Some(stack) = self.held_mut().as_mut() {
            stack.damage = (stack.damage + amount).min(max - 1);
        }
    }
}
