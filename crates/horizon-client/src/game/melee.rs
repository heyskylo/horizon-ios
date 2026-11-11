//! A player's melee attack (`Player.attack`): the attack strength, critical and sweep attacks, and
//! what a hit on a target that is not a mob does.
use super::*;

impl Game<'_> {
    /// `Player.attack`'s damage against `target_kind` (with the attack
    /// strength and whether it is a critical hit): the weapon and effects
    /// scaled by the attack strength, a mace's smash, the critical hit, then
    /// enchantments.
    pub(super) fn melee_damage(
        &mut self,
        target_kind: &str,
        living: bool,
    ) -> (f32, f32, bool, bool) {
        self.melee_damage_with_admission(target_kind, living).0
    }
    /// Player.attack admits its hurt/tail before the item's damage bonus. Keep
    /// that predicate with the one damage evaluation, including its RNG draws.
    pub(super) fn melee_damage_with_admission(
        &mut self,
        target_kind: &str,
        living: bool,
    ) -> ((f32, f32, bool, bool), bool) {
        if self.legacy_rules() {
            let hit = self.legacy_melee_damage(target_kind, living);
            // The legacy rules use their resulting damage, including the
            // intentionally retained modern mace bonus.
            return (hit, hit.0 > 0.0);
        }
        let hand_damage = self.attack_damage();
        // Riptide's spin replaces the attack damage attribute.
        let spin = self.player.as_ref().map_or(0.0, |p| p.auto_spin_attack_dmg);
        let damage = if spin > 0.0 { spin } else { hand_damage };
        let scale = self.attack_strength_scale();
        // getEnchantedDamage(target, damage, source) - damage, taken before the
        // attack strength scales the base damage.
        let enchant_bonus = self.enchanted_damage(target_kind, damage) - damage;
        let mut amount = damage * (0.2 + scale * scale * 0.8);
        // Player.attack decides the critical hit only for a hit with base or
        // enchantment damage: checked before the mace's smash bonus is added.
        let damaging = amount > 0.0 || enchant_bonus * scale > 0.0;
        // Item.getAttackDamageBonus (a mace's smash), before the critical hit.
        amount += self.smash_damage_bonus(target_kind);
        let vanilla_crit = living && self.critical_attack(scale);
        let mut crit = vanilla_crit;
        let mut multiplier = if vanilla_crit { 1.5 } else { 1.0 };
        if living && damaging {
            self.native_critical_hit(target_kind, vanilla_crit, &mut crit, &mut multiplier);
        }
        if crit {
            amount *= multiplier;
        }
        // Enchantment damage (sharpness, smite, bane) scaled by the attack strength.
        amount += enchant_bonus * scale;
        ((amount, scale, crit, enchant_bonus * scale > 0.0), damaging)
    }
    /// `Player.getAttackStrengthScale(0.5)`.
    pub(super) fn attack_strength_scale(&self) -> f32 {
        // 1.8.9 had no attack cooldown.
        if self.legacy_rules() {
            return 1.0;
        }
        let delay = horizon_client::attack_strength::delay(self.main_hand_attack_speed());
        ((self.attack_ticks as f32 + 0.5) / delay).clamp(0.0, 1.0)
    }
    /// `Player.isSweepAttack`: a charged, grounded sword hit below the movement limit.
    pub(super) fn sweep_attack_position(&mut self, scale: f32, crit: bool) -> Option<[f64; 3]> {
        // The sweep came with 1.9.
        if self.legacy_rules() {
            return None;
        }
        // Player.getSpeed: the movement speed attribute as it is now (the
        // field is only refreshed by the local movement step, never for a
        // server's player).
        let p = self.player.as_mut()?;
        if let Ok(speed) = p.attribute("minecraft:movement_speed") {
            p.speed = speed as f32;
        }
        let p = self.player.as_ref()?;
        let sword = self.held().is_some_and(|s| {
            self.res
                .items()
                .item_tags
                .get("minecraft:swords")
                .is_some_and(|tag| tag.contains(&*s.item))
        });
        if !p.is_sweep_attack(scale, crit, sword) {
            return None;
        }
        // Mth.sin / Mth.cos of yRot * (PI / 180), from its sine table.
        use horizon_core::geometry::trig;
        let yaw = f64::from(self.yaw * trig::DEG_TO_RAD);
        Some([
            p.position.x - f64::from(trig::sin(yaw)),
            p.position.y + f64::from(p.dimensions.height) * 0.5,
            p.position.z + f64::from(trig::cos(yaw)),
        ])
    }
    /// `Player.doSweepAttack` after a successful hit, in vanilla's order: the
    /// sweep sound, the victims around `target` (its box, and the mob that
    /// was hit, when there is one) and then the arc.
    pub(super) fn sweep_attack(
        &mut self,
        at: [f64; 3],
        from: Option<[f64; 3]>,
        target: Option<(Aabb, mace::Spared)>,
        scale: f32,
    ) {
        self.audio
            .play("minecraft:entity.player.attack.sweep", from, 1.0, 1.0);
        if let Some((area, mob)) = target {
            self.sweep_damage(area, mob, scale);
        }
        let arc = horizon_client::particles::ParticleOptions::simple(
            horizon_client::particles::ParticleType::SweepAttack,
        );
        self.particles.add(&arc, at, [0.0; 3]);
        // Player.sweepAttack's `sendParticles(SWEEP_ATTACK, ..., 0, dx, 0, dz, 0)`:
        // the offset is the one from the player to the particle.
        let origin = from.unwrap_or(at);
        let spread = [at[0] - origin[0], 0.0, at[2] - origin[2]];
        self.lan_send_particles(lan_particles::QueuedParticle {
            dimension: self.dim.id(),
            options: arc,
            state: None,
            at,
            spread,
            speed: [0.0; 3],
            count: 0,
            randomization: 0,
        });
    }

    /// `Player.attack` after a target that is not a mob took the hit:
    /// the sprint knockback sound, a living target's critical hit (`living`:
    /// its entity id; `crit`: whether `melee_damage` decided the hit critical,
    /// mods' `critical_hit` included), the sweep around the target's box `area`
    /// and the strong or weak sound.
    pub(super) fn attack_effects(
        &mut self,
        area: Aabb,
        living: Option<i32>,
        crit: bool,
        spared: mace::Spared,
        magic: Option<i32>,
    ) {
        let scale = self.attack_strength_scale();
        let full = scale > 0.9;
        let player = self.player.as_ref();
        let from = player.map(|p| [p.position.x, p.position.y, p.position.z]);
        if full && player.is_some_and(|p| p.sprinting) {
            self.audio
                .play("minecraft:entity.player.attack.knockback", from, 1.0, 1.0);
        }
        let crit = living.is_some() && crit;
        let sweep = self.sweep_attack_position(scale, crit);
        if let Some(at) = sweep {
            self.sweep_attack(at, from, Some((area, spared)), scale);
        }
        if crit {
            self.audio
                .play("minecraft:entity.player.attack.crit", from, 1.0, 1.0);
            if let Some(id) = living {
                self.area_critical_hit(id, area, false);
            }
        } else if sweep.is_none() {
            let sound = if full {
                "minecraft:entity.player.attack.strong"
            } else {
                "minecraft:entity.player.attack.weak"
            };
            self.audio.play(sound, from, 1.0, 1.0);
        }
        // Player.magicCrit: the enchantments added damage.
        if let Some(id) = magic {
            self.area_critical_hit(id, area, true);
        }
    }
    /// `critical_hit` around the box `area` of entity `id` (`magic`: the
    /// enchanted hit instead of the crit).
    pub(super) fn area_critical_hit(&mut self, id: i32, area: Aabb, magic: bool) {
        let (feet, size) = (area.bottom_center(), area.size());
        self.critical_hit(
            self.lan_acting,
            id,
            [feet.x, feet.y, feet.z],
            size.x,
            size.y,
            magic,
        );
    }
    /// `Player.canCriticalAttack` for a living target at attack strength
    /// `scale`: falling, off the ground, not in water, not on a ladder, not blind (mobility
    /// restricted), not riding, not sprinting.
    pub(super) fn critical_attack(&self, scale: f32) -> bool {
        let player = self.player.as_ref();
        let falling = player.is_some_and(|p| {
            !p.ground.on_ground
                && p.fall_distance > 0.0
                && !p.fluids.was_touching_water
                && !p.on_climbable
                && !p.sprinting
        });
        let restricted = self.potions.effects.get("blindness").is_some() || self.riding.is_some();
        scale > 0.9 && falling && !restricted
    }
    /// Player.attack with the attack-strength charge, crits and knockback.
    pub(super) fn attack(&mut self) -> bool {
        if self.spectator() {
            return false;
        }
        // A painting nearer than the targeted block and mob breaks instead.
        if let Some((i, d)) = self.painting_hit()
            && self.mob_hit().is_none_or(|m| d < m.1)
            && self.before_block(d)
        {
            self.break_painting(i, true);
            return true;
        }
        // A ghast's fireball or a wind charge in front: turned back.
        if self.attack_redirectable() {
            return true;
        }
        if self.attack_frame()
            || self.attack_armor_stand()
            || self.attack_cushion()
            || self.attack_boat()
            || self.attack_cart()
            || self.attack_dragon()
        {
            return true;
        }
        let Some((index, distance, _)) = self.mob_hit() else {
            return false;
        };
        if !self.before_block(distance) || !self.held_range_allows(distance) {
            return false;
        }
        self.attack_mob(index);
        // Minecraft.startAttack swings after the attack.
        self.swing_arm();
        true
    }
}

impl Game<'_> {
    /// `Player.attack` on a mob: the damage, knockback, sweep, particles and
    /// sounds.
    pub(super) fn attack_mob(&mut self, index: usize) {
        let _attacker = self.knockback_scope(self.player.as_ref().map_or(0, |p| p.entity_id));
        let target_kind = self.mobs[index].kind.id.clone();
        let target_kind = format!("minecraft:{}", target_kind.trim_start_matches("minecraft:"));
        let (amount, scale, crit, magic) = self.melee_damage(&target_kind, true);
        let sweep = self.sweep_attack_position(scale, crit);
        let (knockback, knockback_attack) = self.extra_knockback(&target_kind, scale);
        self.attack_ticks = 0;
        let player = self.player.as_ref();
        let from = player.map(|p| [p.position.x, p.position.y, p.position.z]);
        if knockback_attack {
            self.audio
                .play("minecraft:entity.player.attack.knockback", from, 1.0, 1.0);
        }
        // LivingEntity.setLastHurtMob (tamed wolves join in).
        self.combat.last_hurt_mob = Some((self.mobs[index].id, self.world.game_time));
        let old_health = self.mobs[index].health;
        let source = match from {
            Some(f) => self.attack_source(f),
            None => horizon_client::mobs::Damage::of(horizon_client::mobs::DamageKind::Generic),
        };
        let landed = self.hurt_by_player(index, amount, source);
        if landed && old_health > 0.0 && !self.mobs[index].alive() {
            self.native_kill_event(index);
        }
        if landed {
            let victim = self.mob_entity(&self.mobs[index]);
            self.player_hit(
                &victim,
                creatures::damage_type(source.kind, true),
                None,
                amount,
            );
        }
        if let Some(p) = self.player.as_mut() {
            if p.attack_log.len() >= 64 {
                p.attack_log.remove(0);
            }
            p.attack_log.push(horizon_client::health::AttackRecord {
                game_time: p.health.game_time(),
                target: target_kind.clone(),
                id: self.mobs[index].id,
                killed: landed && !self.mobs[index].alive(),
                amount,
                scale,
                crit,
                landed,
                dealt: old_health - self.mobs[index].health,
            });
        }
        if landed {
            if knockback > 0.0 {
                let (xd, zd) = self.extra_knockback_direction();
                if self.legacy_rules() {
                    let m = &mut self.mobs[index];
                    let v = [m.velocity.x, m.velocity.y, m.velocity.z];
                    let [x, y, z] = horizon_client::legacy_combat::extra_knockback(
                        v,
                        f64::from(knockback),
                        (xd, zd),
                    );
                    m.velocity = Vec3::new(x, y, z);
                    m.needs_sync = true;
                } else {
                    self.mobs[index].knockback_effect(
                        f64::from(knockback),
                        xd,
                        zd,
                        &source,
                        amount,
                    );
                }
                self.stop_sprint_after_extra_knockback();
            }
            let mob = &self.mobs[index];
            // Player.damageStatsAndHearts: the damage the hit actually did.
            let actual = old_health - mob.health;
            // Player.attack: crit emitter and damage indicator hearts.
            let (feet, width, height) = (
                [mob.position.x, mob.position.y, mob.position.z],
                mob.kind.width,
                mob.kind.height,
            );
            self.award_custom_stat("damage_dealt", (actual * 10.0).round() as i32);
            if crit {
                self.critical_hit(
                    self.lan_acting,
                    self.mobs[index].id as i32,
                    feet,
                    width,
                    height,
                    false,
                );
            }
            // Player.attackVisualEffects: magicCrit when the enchantments added damage.
            if magic {
                self.critical_hit(
                    self.lan_acting,
                    self.mobs[index].id as i32,
                    feet,
                    width,
                    height,
                    true,
                );
            }
            if actual > 2.0 {
                let indicator = horizon_client::particles::ParticleOptions::simple(
                    horizon_client::particles::ParticleType::DamageIndicator,
                );
                self.particles.send_particles(
                    &indicator,
                    [feet[0], feet[1] + height * 0.5, feet[2]],
                    [0.1, 0.0, 0.1],
                    0.2,
                    (actual * 0.5) as u32,
                );
                // The players a server serves are sent them too.
                self.lan_send_particles(lan_particles::QueuedParticle {
                    dimension: self.dim.id(),
                    options: indicator,
                    state: None,
                    at: [feet[0], feet[1] + height * 0.5, feet[2]],
                    spread: [0.1, 0.0, 0.1],
                    speed: [0.2; 3],
                    count: (actual * 0.5) as u32 as i32,
                    randomization: 0,
                });
            }
            self.last_hurt_by_player
                .insert(self.mobs[index].id, self.lan_acting);
            let sound = if crit {
                "minecraft:entity.player.attack.crit"
            } else if scale > 0.9 {
                "minecraft:entity.player.attack.strong"
            } else {
                "minecraft:entity.player.attack.weak"
            };
            // Player.playServerSideSound: at the player.
            if let Some(at) = sweep {
                let target = (
                    self.mobs[index].bounds(),
                    mace::Spared::Mob(self.mobs[index].id),
                );
                self.sweep_attack(at, from, Some(target), scale);
            } else {
                self.audio.play(sound, from, 1.0, 1.0);
            }
            // The hurt or death sound the mob raised (getHurtSound / getDeathSound,
            // its voice pitch from its own random).
            let queued = std::mem::take(&mut self.mobs[index].outbox);
            let mut infested = Vec::new();
            for event in queued {
                match event {
                    horizon_client::mobs::MobEvent::Sound(s, at, volume, pitch) => {
                        self.audio
                            .play(&format!("minecraft:{s}"), Some(at), volume, pitch);
                    }
                    horizon_client::mobs::MobEvent::Infested { at, velocities } => {
                        infested.push((at, velocities))
                    }
                    other => {
                        // Left for the end of the tick.
                        self.mobs[index].outbox.push(other);
                    }
                }
            }
            for (at, velocities) in infested {
                self.spawn_infested(at, velocities);
            }
            self.weapon_hit(index);
            // Player.attack: causeFoodExhaustion(0.1F) only when the hit hurt
            // (inside `if (wasHurt)`), after damageStatsAndHearts.
            if let Some(p) = self.player.as_mut()
                && !self.creative
            {
                p.cause_food_exhaustion(0.1);
            }
        } else {
            self.audio
                .play("minecraft:entity.player.attack.nodamage", from, 1.0, 1.0);
        }
    }
}
