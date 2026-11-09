//! Finishing a consumable (`Consumable.onConsume` and the use remainder),
//! its particles and sounds, and `TeleportRandomlyConsumeEffect` (chorus
//! fruit) for the local player. The level random stands in for the player's
//! own, as for every other player draw.
use super::{Dim, Game};
use horizon_client::{
    items::{ConsumeEffect, ItemInfo, ItemStack},
    mobs::Who,
    particles::{ParticleOptions, ParticleType},
};
use horizon_core::{
    geometry::Vec3, position::BlockPos, random::LegacyRandom, random::RandomSource,
};
use horizon_world::{
    consumable::{Consumable, EmissionWorld, UseAnimation},
    teleport_consume::{TeleportConsume, TeleportConsumeWorld},
};
use std::convert::Infallible;

type ItemConsumable = Consumable<String, ConsumeEffect>;

/// `EntityEvent.USE_ITEM_COMPLETE`: the server finished the player's use.
pub(super) const USE_ITEM_COMPLETE_EVENT: i8 = 9;

impl Game<'_> {
    /// `ServerPlayer.completeUsingItem`: the player's own client is told
    /// first, and completes the use itself (eating the food there too).
    pub(super) fn send_use_item_complete(&mut self) {
        if self.lan_acting.is_some() {
            self.guest_entity_events.push(USE_ITEM_COMPLETE_EVENT);
        }
    }
    /// `Consumable.emitParticlesAndSounds(user.getRandom(), user, stack, count)`.
    pub(super) fn emit_consume(
        &mut self,
        consumable: &ItemConsumable,
        stack: &ItemStack,
        count: i32,
    ) {
        let Ok(()) = consumable.emit(&mut Emitter(self), (), stack.clone(), count);
    }
    /// `LivingEntity.completeUsingItem` for a consumable: `Consumable.onConsume`,
    /// then `ItemStack.applyAfterUseComponentSideEffects`' use remainder.
    pub(super) fn finish_consuming(
        &mut self,
        info: &ItemInfo,
        consumable: &ItemConsumable,
        stack: &ItemStack,
    ) {
        let consumable = {
            let mut consumable = consumable.clone();
            if let Some(sound) = horizon_client::item_sync::consumable_sound(stack) {
                consumable.sound = sound;
        };
        self.emit_consume(&consumable, stack, 16);
        // ServerPlayer: the used statistic and the consume_item trigger.
        self.award_stat("used", &stack.item, 1);
        self.trigger_item("minecraft:consume_item", stack);
        // FoodProperties.onConsume, the player's one ConsumableListener.
        if let Some((nutrition, saturation)) = horizon_client::item_sync::food(stack, info.food) {
            let at = self.feet();
            let pitch = self.world.random.triangle_float(1.0, 0.4);
            self.audio.play(&consumable.sound, at, 1.0, pitch);
            if let Some(p) = self.player.as_mut() {
                // FoodData.eat(nutrition, saturation)
                let food = &mut p.health.food;
                food.food = (food.food + nutrition as i32).clamp(0, 20);
                food.saturation = (food.saturation + saturation).clamp(0.0, food.food as f32);
            }
            // Mth.randomBetween(random, 0.9, 1.0)
            let pitch = self.world.random.next_float() * (1.0f32 - 0.9f32) + 0.9;
            self.audio
                .play("minecraft:entity.player.burp", self.feet(), 0.5, pitch);
        }
        // SuspiciousStewEffects.onConsume, a ConsumableListener: each entry is
        // added as a `MobEffectInstance(effect, duration)` (amplifier 0).
        if let Some(data) = stack.extra.as_deref() {
            for (id, duration) in &data.stew_effects {
                if let Some(def) = horizon_client::brewing::effect(id) {
                    self.add_player_effect(horizon_client::effects::EffectInstance::new(
                        def.id, *duration, 0,
                    ));
                }
            }
        }
        for effect in &consumable.effects {
            self.apply_consume_effect(effect);
        }
        let event = if consumable.animation == UseAnimation::Drink {
            "minecraft:drink"
        } else {
            "minecraft:eat"
        };
        self.post_entity_event(event, Who::Player);
        // ItemStack.consume, then UseRemainder.convertIntoRemainder: nothing
        // with infinite materials, the hand when emptied, else the inventory
        // (ServerPlayer.handleExtraItemsCreatedOnUse) or a drop.
        self.consume_held();
        if let Some(remainder) =
            horizon_client::item_sync::use_remainder(stack, info.use_remainder.clone())
            && !self.creative
        {
            let remainder = ItemStack::new(&remainder, 1);
            if self.held().is_none() {
                *self.held_mut() = Some(remainder);
            } else if let Some(rest) = self.give(remainder) {
                self.player_drop(rest);
            }
        }
    }
    fn feet(&self) -> Option<[f64; 3]> {
        self.player
            .as_ref()
            .map(|p| [p.position.x, p.position.y, p.position.z])
    }
    /// `TeleportRandomlyConsumeEffect.apply` for the local player.
    pub(super) fn teleport_randomly(&mut self, teleport: &TeleportConsume) {
        if self.player.is_none() {
            return;
        }
        let dim = self.dim;
        let Ok(_) = teleport.apply(&mut Teleporter(self), dim);
    }
    /// `LivingEntity.randomTeleport(x, y, z, true, avoid)`: down to the first
    /// block entities can teleport onto, inside the border, in a loaded chunk.
    fn random_teleport(&mut self, target: Vec3, avoid: &str) -> bool {
        let [min_x, min_z, max_x, max_z] = self.border.bounds();
        // WorldBorder.clampVec3ToBound (Mth.clamp, 1.0E-5F inside the far edge).
        let clamp = |v: f64, low: f64, high: f64| if v < low { low } else { v.min(high) };
        let x = clamp(target.x, min_x, max_x - f64::from(1.0e-5_f32));
        let z = clamp(target.z, min_z, max_z - f64::from(1.0e-5_f32));
        let mut y = target.y;
        let mut pos = BlockPos::containing(Vec3::new(x, y, z));
        if !self.world.loaded(pos.x, pos.z) {
            return false;
        }
        while pos.y > self.world.shape.min_y {
            pos.y -= 1;
            let state = self.world.block(pos.x, pos.y, pos.z);
            if self.state_in_tag("minecraft:entities_can_teleport_to", state) {
                return self.check_position_and_teleport(Vec3::new(x, y, z), state, avoid);
            }
            y -= 1.0;
        }
        false
    }
    /// `LivingEntity.checkPositionAndTeleport`, then `ServerPlayer.teleportTo`
    /// (the motion and rotation kept) and entity event 46.
    fn check_position_and_teleport(
        &mut self,
        at: Vec3,
        ground: horizon_world::registry::BlockStateId,
        avoid: &str,
    ) -> bool {
        if self.state_in_tag(avoid, ground) {
            return false;
        }
        let entities = self.player_entity_collisions();
        let Some(player) = self.player.as_mut() else {
            return false;
        };
        let bounds = player.dimensions.bounds(at);
        let free = horizon_client::motion::MotionWorld {
            entities: &entities,
            player,
            scene: &mut self.world,
            assets: &self.pack.assets,
            collision_binding: self.collision,
            resources: self.motion,
            health: &self.health_resources,
        }
        .no_player_collision(bounds)
        .unwrap_or(false);
        // LevelReader.containsAnyLiquid and findBlocksIn: the blocks from the
        // box's minimum corner to the one holding its maximum corner.
        let (lo, hi) = (
            BlockPos::containing(bounds.min_position()),
            BlockPos::containing(bounds.max_position()),
        );
        let blocks = (lo.x..=hi.x)
            .flat_map(|x| (lo.y..=hi.y).flat_map(move |y| (lo.z..=hi.z).map(move |z| (x, y, z))));
        let states: Vec<_> = blocks.map(|(x, y, z)| self.world.block(x, y, z)).collect();
        let liquid = states
            .iter()
            .any(|&s| self.res.facts().get(s).fluid != horizon_client::blocks::Fluid::None);
        if !free || liquid || states.iter().any(|&s| self.state_in_tag(avoid, s)) {
            return false;
        }
        if let Some(p) = self.player.as_mut() {
            p.position = at;
        }
        // The position packet moves the old position along (absolute xyz).
        self.previous_position = at;
        // `broadcastEntityEvent(46)` after the teleport. A server-thread window
        // is a remote client, so the packet is what draws the particles there.
        if let Some(id) = self.lan_acting_entity {
            self.lan_entity_event(id as i32, 46, [at.x, at.y, at.z]);
        } else {
            self.portal_particles();
        }
        true
    }
    fn state_in_tag(&self, tag: &str, state: horizon_world::registry::BlockStateId) -> bool {
        let name = &self.res.facts().get(state).name;
        self.res
            .items()
            .block_tags
            .get(tag)
            .is_some_and(|s| s.contains(&**name))
    }
    /// `LivingEntity.handleEntityEvent(46)` for this client: 128 portal
    /// particles from the old position to the new one. The local player's
    /// random is the particle engine's.
    fn portal_particles(&mut self) {
        let (from, to, width, height) = {
            let Some(p) = self.player.as_ref() else {
                return;
            };
            (
                [
                    self.previous_position.x,
                    self.previous_position.y,
                    self.previous_position.z,
                ],
                [p.position.x, p.position.y, p.position.z],
                f64::from(p.dimensions.width),
                f64::from(p.dimensions.height),
            )
        };
        self.emit_living_teleport(from, to, width, height);
    }
    /// `LivingEntity.handleEntityEvent(46)` for the entity the packet names:
    /// this player, or a mob the client is drawing.
    pub(super) fn entity_teleport_particles(&mut self, id: i32) {
        if self.remote.as_ref().is_some_and(|r| r.entity_id == id) {
            self.portal_particles();
            return;
        }
        let spawned = {
            let Some(m) = self.mobs.iter_mut().find(|m| m.id == id as u32) else {
                return;
            };
            let from = [m.previous.x, m.previous.y, m.previous.z];
            let to = [m.position.x, m.position.y, m.position.z];
            let (width, height) = (f64::from(m.width()), f64::from(m.height()));
            let mut spawned = Vec::with_capacity(128);
            horizon_client::particles::living_teleport_particles(
                &mut m.random,
                from,
                to,
                width,
                height,
                |p, v| spawned.push((p, v)),
            );
            spawned
        };
        let options = ParticleOptions::simple(ParticleType::Portal);
        for (p, v) in spawned {
            self.particles.add(&options, p, v);
        }
    }
    fn emit_living_teleport(&mut self, from: [f64; 3], to: [f64; 3], width: f64, height: f64) {
        let mut spawned = Vec::with_capacity(128);
        horizon_client::particles::living_teleport_particles(
            self.particles.random(),
            from,
            to,
            width,
            height,
            |p, v| spawned.push((p, v)),
        );
        let options = ParticleOptions::simple(ParticleType::Portal);
        for (p, v) in spawned {
            self.particles.add(&options, p, v);
        }
    }
    /// `LivingEntity.stopRiding`.
    pub(super) fn stop_riding(&mut self) {
        let Some(id) = self.riding else {
            return;
        };
        if let Some(i) = self.boats.iter().position(|b| b.id == id) {
            self.dismount(i);
        } else if let Some(i) = self.carts.iter().position(|c| c.id == id) {
            self.dismount_cart(i);
        } else if let Some(c) = self.cushions.iter().find(|c| c.id == id).cloned() {
            self.get_up_from_cushion(&c);
        } else if self.seat(id).is_some() {
            self.get_off_seat();
        } else {
            self.dismount_mount();
        }
    }
}

/// `Consumable.emitParticlesAndSounds` against the local player.
struct Emitter<'a, 'g>(&'a mut Game<'g>);
impl EmissionWorld for Emitter<'_, '_> {
    type Error = Infallible;
    type Stack = ItemStack;
    type Sound = String;
    type Random = ();
    type Source = LegacyRandom;
    fn random_source(&mut self, (): ()) -> &mut LegacyRandom {
        &mut self.0.world.random
    }
    /// `LivingEntity.spawnItemParticles`.
    fn spawn_item_particles(&mut self, stack: ItemStack, count: i32) -> Result<(), Infallible> {
        self.0.eat_particles(&stack, count.max(0) as u32);
        Ok(())
    }
    fn overrides_consume_sound(&self) -> bool {
        false
    }
    fn consume_sound(&mut self, _: ItemStack) -> Result<String, Infallible> {
        unreachable!("players play the consumable's own sound")
    }
    /// `Player.playSound`: at the player's position, for everyone but the
    /// player, whose own client plays it (`level.playSound(player, ...)`):
    /// a guest's server turn leaves the guest out.
    fn play_sound(&mut self, sound: String, volume: f32, pitch: f32) -> Result<(), Infallible> {
        let at = self.0.feet();
        if self.0.lan_acting.is_some() {
            self.0.audio.play_unshared(&sound, at, volume, pitch);
            if let Some(at) = at {
                self.0.guest_player_sounds.push((sound, at, volume, pitch));
            }
        } else {
            self.0.audio.play(&sound, at, volume, pitch);
        }
        Ok(())
    }
}

/// The local player as `TeleportRandomlyConsumeEffect`'s user.
struct Teleporter<'a, 'g>(&'a mut Game<'g>);
impl Teleporter<'_, '_> {
    fn at(&self) -> Vec3 {
        self.0.player.as_ref().map_or(Vec3::ZERO, |p| p.position)
    }
}
impl TeleportConsumeWorld for Teleporter<'_, '_> {
    type Error = Infallible;
    type Level = Dim;
    type Random = ();
    type Source = LegacyRandom;
    fn position_field(&self) -> Vec3 {
        self.at()
    }
    fn get_random(&mut self) -> Result<(), Infallible> {
        Ok(())
    }
    fn random_source(&mut self, (): ()) -> &mut LegacyRandom {
        &mut self.0.world.random
    }
    fn min_y(&mut self, level: Dim) -> Result<i32, Infallible> {
        Ok(self.0.level_shapes.get(level.id()).min_y)
    }
    fn logical_height(&mut self, level: Dim) -> Result<i32, Infallible> {
        Ok(self.0.level_shapes.get(level.id()).logical_height)
    }
    fn is_passenger(&mut self) -> Result<bool, Infallible> {
        Ok(self.0.riding.is_some())
    }
    fn stop_riding(&mut self) -> Result<(), Infallible> {
        self.0.stop_riding();
        Ok(())
    }
    fn position(&mut self) -> Result<Vec3, Infallible> {
        Ok(self.at())
    }
    fn teleport_allowed(&mut self, target: &mut Vec3) -> Result<bool, Infallible> {
        // EntityTeleportEvent.ChorusFruit: only a server's player is heard.
        let (Some(uuid), Some(name)) = (self.0.lan_acting_uuid, self.0.lan_acting_name.clone())
        else {
            return Ok(true);
        };
        let from = self.at();
        let mut to = [target.x, target.y, target.z];
        let mut cancel = false;
        self.0.native_chorus_fruit_teleport(
            uuid,
            &name,
            [from.x, from.y, from.z],
            &mut to,
            &mut cancel,
        );
        // Keep the mod's spot inside the level as vanilla's own draw is (a
        // huge y would walk down a long way); one that is not a number keeps
        // the draw.
        if to.iter().all(|v| v.is_finite()) {
            let shape = self.0.level_shapes.get(self.0.dim.id());
            let high = shape.min_y + shape.logical_height - 1;
            let y = to[1].clamp(f64::from(shape.min_y), f64::from(high));
            *target = Vec3::new(to[0], y, to[2]);
        }
        if !cancel {
            horizon_client::lan::hub::teleport_cause(Some("chorus_fruit"));
        }
        Ok(!cancel)
    }
    fn random_teleport(&mut self, target: Vec3) -> Result<bool, Infallible> {
        Ok(self
            .0
            .random_teleport(target, "minecraft:consumable_does_not_teleport_to"))
    }
    fn teleport_event(&mut self, _: Dim, old: Vec3) -> Result<(), Infallible> {
        let ctx = self.0.who_context(Some(Who::Player), None);
        self.0
            .post_game_event("minecraft:teleport", [old.x, old.y, old.z], ctx);
        Ok(())
    }
    fn is_fox(&self) -> bool {
        false
    }
    fn teleport_sound(&mut self, _: Dim, at: Vec3, _: bool) -> Result<(), Infallible> {
        self.0.audio.play(
            "minecraft:item.chorus_fruit.teleport",
            Some([at.x, at.y, at.z]),
            1.0,
            1.0,
        );
        Ok(())
    }
    fn block_position(&mut self) -> Result<BlockPos, Infallible> {
        Ok(BlockPos::containing(self.at()))
    }
    fn directional_particles(
        &mut self,
        _: Dim,
        origin: BlockPos,
        offset: i32,
    ) -> Result<(), Infallible> {
        self.0
            .level_event_at(2017, [origin.x, origin.y, origin.z], offset);
        Ok(())
    }
    fn reset_fall_distance(&mut self) -> Result<(), Infallible> {
        if let Some(p) = self.0.player.as_mut() {
            p.fall_distance = 0.0;
        }
        Ok(())
    }
    fn reset_impulse(&mut self) -> Result<(), Infallible> {
        if let Some(p) = self.0.player.as_mut() {
            p.impulse.reset();
        }
        Ok(())
    }
}
