//! Accepted melee hits on special targets and their shared attacker tail.
use super::{dragon::Hurt, *};
use horizon_client::mobs::DamageKind;

impl Game<'_> {
    /// Player.attack's cost after an accepted special-entity hurt. Hanging
    /// decorations consume their hit in skipAttackInteraction, before this tail.
    pub(super) fn special_attack_exhaustion(&mut self) {
        if let Some(player) = self.player.as_mut() {
            player.cause_food_exhaustion(0.1);
        }
    }
    /// The attacker's side of Player.causeExtraKnockback, including when the
    /// accepted target was removed by its hurt implementation.
    pub(super) fn special_attack_slowdown(&mut self, knockback: f32) {
        if knockback > 0.0 {
            if let Some(player) = self.player.as_mut() {
                player.velocity.x *= horizon_client::attack_strength::ATTACKER_SLOWDOWN;
                player.velocity.z *= horizon_client::attack_strength::ATTACKER_SLOWDOWN;
            }
            self.stop_sprint_after_extra_knockback();
        }
    }

    pub(super) fn attack_dragon(&mut self) -> bool {
        let Some((id, d)) = self.dragon_hit() else {
            return false;
        };
        if !self.before_block(d) || self.mob_hit().is_some_and(|m| m.1 < d) {
            return false;
        }
        // A dragon part is no LivingEntity: `canCriticalAttack` is false.
        let (amount, scale, _, _) = self.melee_damage("minecraft:ender_dragon", false);
        self.swing_arm();
        // The part's box.
        let area = self
            .special_targets()
            .into_iter()
            .find(|(part, _)| *part == id)
            .map(|(_, b)| b);
        let hurt = self.hit_special(id, amount, true, DamageKind::PlayerAttack, None);
        // The sweep and the sounds read the charge, which the hit then spends.
        if let (Some(area), Hurt::Landed { .. }) = (area, hurt) {
            let (knockback, _) = self.extra_knockback("minecraft:ender_dragon", scale);
            self.attack_effects(area, None, false, super::mace::Spared::Nothing, None);
            self.special_attack_slowdown(knockback);
            self.special_attack_exhaustion();
        }
        self.attack_ticks = 0;
        true
    }
    /// `VehicleEntity.hurtServer` from the player's attack.
    pub(super) fn attack_cart(&mut self) -> bool {
        let Some((i, d)) = self.cart_hit() else {
            return false;
        };
        if !self.before_block(d) || self.mob_hit().is_some_and(|m| m.1 < d) {
            return false;
        }
        let burning = self
            .held()
            .is_some_and(|s| s.enchantment_level("minecraft:fire_aspect") > 0);
        let area = self.carts[i].bounds();
        // Player.attack's full damage against the cart's entity type.
        let id = self.carts[i].id as i32;
        let kind = self.carts[i].kind.id();
        let (damage, scale, _, magic) = self.melee_damage(kind, false);
        if self.hurt_cart_result(i, damage, self.creative, burning) && damage > 0.0 {
            self.vehicle_attack_sounds(area, magic.then_some(id));
            let (knockback, _) = self.extra_knockback(kind, scale);
            self.special_attack_slowdown(knockback);
            self.special_attack_exhaustion();
        }
        // Player.onAttack: the hit starts the attack strength again.
        self.attack_ticks = 0;
        self.swing_arm();
        true
    }
}
