//! Damage continuations at the two equipment boundaries. The gameplay owner
//! keeps the complete acting player installed while running durability hooks.
use super::*;
use horizon_world::living_hurt::{self, PendingHurt, PreparedHurt};

#[doc(hidden)]
pub struct EquipmentHurt {
    source: DamageTypeId,
    rules: HealthRules,
    amount: f32,
    blocking: Option<Blocking>,
    living_cause: bool,
    causing: i32,
    before: f32,
    fire_ticks: i32,
    prepared: Option<PreparedHurt<()>>,
    pending: Option<PendingHurt<()>>,
    inner: Option<bool>,
    blocked: f32,
    blocking_item_used: bool,
    fully_blocked: bool,
    block_sound: bool,
    knocked: bool,
    totem_fired: bool,
    canceled: bool,
    damages_helmet: bool,
}
impl EquipmentHurt {
    pub fn damage(&self) -> Option<f32> {
        self.prepared.as_ref().map(|p| {
            if p.damage.is_finite() {
                p.damage
            } else {
                f32::MAX
            }
        })
    }
    pub fn helmet_damage(&self) -> Option<f32> {
        self.prepared
            .as_ref()
            .filter(|_| self.damages_helmet)
            .map(|p| p.damage)
    }
    pub fn helmet_worn(&mut self) {
        if let Some(prepared) = &mut self.prepared {
            prepared.damage *= 0.75;
        }
    }
}
impl PlayerMotion {
    fn equipment_step<T>(
        &mut self,
        resources: &HealthResources,
        hit: &mut EquipmentHurt,
        step: impl FnOnce(
            &mut State,
            &mut ServerPlayerCombat<'_, Environment<'_>, Attributes>,
        ) -> Result<T>,
    ) -> Result<T> {
        let source = resources
            .damage
            .source::<()>(hit.source, None, None, None)
            .ok_or("unknown resident damage source")?;
        let h = &mut self.health;
        let mut environment = Environment {
            entity: self.entity_id,
            causing: hit.causing,
            invulnerable: self.invulnerable,
            client_loaded: self.client_loaded,
            rules: hit.rules,
            source,
            living_cause: hit.living_cause,
            events: &mut self.events,
            dead: &mut h.dead,
            sync_velocity: &mut h.sync_velocity,
            game_time: h.game_time,
            status: self.status.clone(),
            blocking: hit.blocking.clone(),
            blocked: hit.blocked,
            blocking_item_used: hit.blocking_item_used,
            fully_blocked: hit.fully_blocked,
            block_sound: hit.block_sound,
            knocked: hit.knocked,
            totem: h.totem_ready,
            totem_fired: hit.totem_fired,
            canceled: hit.canceled,
        };
        let mut combat = ServerPlayerCombat::new(
            &mut self.attributes,
            &mut h.data,
            &mut h.food,
            h.ids,
            &mut environment,
        )?;
        let result = step(&mut h.hurt, &mut combat);
        hit.blocked = environment.blocked;
        hit.blocking_item_used = environment.blocking_item_used;
        hit.fully_blocked = environment.fully_blocked;
        hit.block_sound = environment.block_sound;
        hit.knocked = environment.knocked;
        hit.totem_fired = environment.totem_fired;
        hit.canceled = environment.canceled;
        result
    }
    #[doc(hidden)]
    pub fn prepare_equipment_hurt(
        &mut self,
        resources: &HealthResources,
        rules: HealthRules,
        source: DamageTypeId,
        amount: f32,
        blocking: Option<Blocking>,
        living_cause: bool,
        causing: i32,
    ) -> Result<EquipmentHurt> {
        let mut hit = EquipmentHurt {
            source,
            rules,
            amount,
            blocking,
            living_cause,
            causing,
            before: self.health.health() + self.health.absorption(),
            fire_ticks: self.fire_ticks,
            prepared: None,
            pending: None,
            inner: None,
            blocked: 0.0,
            blocking_item_used: false,
            fully_blocked: false,
            block_sound: false,
            knocked: false,
            totem_fired: false,
            canceled: false,
            damages_helmet: resources.damage.in_tag(source, "minecraft:damages_helmet"),
        };
        if !self.client_side {
            hit.prepared = self.equipment_step(resources, &mut hit, |state, combat| {
                Ok(combat.prepare_hurt(state, amount)?)
            })?;
        }
        Ok(hit)
    }
    #[doc(hidden)]
    pub fn select_equipment_hurt(
        &mut self,
        resources: &HealthResources,
        hit: &mut EquipmentHurt,
    ) -> Result<Option<f32>> {
        let Some(prepared) = hit.prepared.take() else {
            return Ok(None);
        };
        hit.pending = self.equipment_step(resources, hit, |state, combat| {
            Ok(living_hurt::select_hurt_server(state, combat, prepared)?)
        })?;
        if let Some(pending) = &hit.pending {
            let damage = pending.damage;
            hit.inner = self.equipment_step(resources, hit, |_, combat| {
                Ok(combat.prepare_equipment_inner()?)
            })?;
            if hit.inner == Some(false) {
                return Ok(Some(damage));
            }
        }
        Ok(None)
    }
    #[doc(hidden)]
    pub fn finish_equipment_hurt(
        &mut self,
        resources: &HealthResources,
        mut hit: EquipmentHurt,
    ) -> Result<BlockOutcome> {
        let hurt = if let Some(pending) = hit.pending.take() {
            let inner = hit.inner;
            self.equipment_step(resources, &mut hit, |state, combat| {
                if let Some(bypass) = inner {
                    combat.apply_equipment_inner(pending.damage, bypass)?;
                }
                Ok(living_hurt::finish_hurt_server(state, combat, pending)?)
            })?
        } else {
            false
        };
        let h = &mut self.health;
        if hit.totem_fired {
            h.totem_used = true;
            h.totem_ready = false;
        }
        if hurt {
            h.hurt_animation_time = h.hurt.hurt_time;
        }
        if hit.knocked && hit.blocked <= 0.0 {
            h.indicate_damage(0.0, 0.0, self.yaw);
        }
        let after = h.health() + h.absorption();
        if self.hurt_log.len() >= HURT_LOG {
            self.hurt_log.remove(0);
        }
        self.hurt_log.push(HurtRecord {
            game_time: h.game_time,
            source: resources
                .damage
                .definition(hit.source)
                .ok_or("unknown resident damage source")?
                .name
                .clone(),
            amount: hit.amount,
            blocked: hit.blocked,
            taken: hit.before - after,
            hurt,
            armor: self
                .attributes
                .cached_value_of("minecraft:armor")
                .unwrap_or(0.0) as f32,
            toughness: self
                .attributes
                .cached_value_of("minecraft:armor_toughness")
                .unwrap_or(0.0) as f32,
            health: after,
            fire_ticks: hit.fire_ticks,
            knockback: None,
        });
        Ok(BlockOutcome {
            hurt,
            blocked: hit.blocked,
            blocking_item_used: hit.blocking_item_used,
            fully_blocked: hit.fully_blocked,
            sound: hit.block_sound,
            knockback: hit.knocked,
            canceled: hit.canceled,
        })
    }
}
