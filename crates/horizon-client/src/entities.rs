//! Non-player entities simulated on the client world. Dropped items follow
//! `ItemEntity.tick`: gravity 0.04, 0.98F drag (block friction on the ground),
//! a ground bounce of -0.5, slime landing restitution (0.8, including a moving
//! piston that is carrying slime), water buoyancy, pickup delay, 6000-tick despawn,
//! `HoneyBlock.entityInside` when falling against a honey block's side, and
//! `BubbleColumnBlock.entityInside` (lift / drag, including the surface jump).
use crate::entity_rules::EntityRules;
use crate::{
    blocks::{BlockFacts, Fluid},
    collisions::SceneCollisions,
    items::ItemStack,
    world::World,
};
use horizon_core::{
    geometry::{Aabb, Direction, Vec3, VoxelShape, border::BorderGeometry},
    position::BlockPos,
    random::{LegacyRandom, RandomSource},
};
use horizon_world::{
    block_collision::BlockCollisionRegistry,
    level::portal_travel::{BorderBounds, find_free_position},
    movement_properties::BoundMovementProperties,
};

/// Gravity applied each tick when the item is not in a fluid (`ItemEntity.tick`).
const ITEM_GRAVITY: f64 = 0.04;
/// Non-living slime `bounceRestitution` factor (`0.8f`) from
/// `Entity.restituteMovementAfterCollisions`.
const ITEM_SLIME_BOUNCE: f32 = 0.8;

/// `Entity.restituteMovementAfterCollisions` for the vertical axis of a
/// non-living entity: `current_y` is its velocity before the move, `moved_y`
/// how far the move got. Landing on slime (`below`) bounces by the block's
/// restitution `1.0f * 0.8f`, with the gravity and air drag the part of the tick
/// after the hit would have added back; any other collision zeroes Y.
pub fn vertical_collision_velocity(
    current_y: f64,
    moved_y: f64,
    gravity: f64,
    below: bool,
    slime: bool,
) -> f64 {
    if !(below && slime && -current_y > gravity) {
        return 0.0;
    }
    let restitution = f64::from(1.0_f32 * ITEM_SLIME_BOUNCE);
    let portion = moved_y / current_y;
    let drag = 1.0 + portion * (f64::from(0.98_f32) - 1.0);
    (portion * gravity - current_y) * drag * restitution
}

/// `Entity.move`: the position only changes when the collided step is longer
/// than `sqrt(1e-7)` or nothing was cut off; a tiny step against a wall or floor
/// is dropped (its collision effects still run).
pub fn move_applies(wanted: Vec3, moved: Vec3) -> bool {
    let moved_sq = moved.length_squared();
    moved_sq > 1.0e-7 || wanted.length_squared() - moved_sq < 1.0e-7
}

/// `SlimeBlock.stepOn`: a slow walk or slide on slime drags the horizontal
/// speed (`0.4 + |dy| * 0.2`); a fast vertical speed leaves it alone.
pub fn slime_step_on_scale(velocity_y: f64) -> Option<f64> {
    let abs_delta_y = velocity_y.abs();
    (abs_delta_y < 0.1).then_some(0.4 + abs_delta_y * 0.2)
}

fn slime_support_name(block: &str, moving_moved: Option<&str>) -> bool {
    block == "minecraft:slime_block" || moving_moved == Some("minecraft:slime_block")
}

/// A piston-moved slime is `moving_piston` with `movedState = slime`.
fn slime_at(world: &World, facts: &BlockFacts, position: BlockPos) -> bool {
    let block = &*facts
        .get(world.block(position.x, position.y, position.z))
        .name;
    let moving_moved = world
        .moving_pistons
        .iter()
        .find(|(pos, _)| *pos == [position.x, position.y, position.z])
        .map(|(_, piston)| &*facts.get(piston.moved_state).name);
    slime_support_name(block, moving_moved)
}

mod fluids;
mod item_fall;
mod item_no_physics;
mod obstacles;
pub use fluids::{fall_reset_hit, fluid_flow, update_entity_fluids, water_clip};
pub use obstacles::ObstacleGrid;

#[cfg(test)]
mod drop_motion_tests {
    use super::drop_motion;
    use horizon_core::random::{LegacyRandom, RandomSource};

    /// Vanilla's own `Mth` and `LegacyRandomSource(7)` running
    /// `createItemStackToDrop`'s arithmetic: three throws along the view,
    /// then two scattered drops, bit for bit, then the random's next int.
    #[test]
    fn drops_match_vanillas_motion_bit_for_bit() {
        let mut r = LegacyRandom::new(7);
        let bits = |v: [f64; 3]| v.map(|c| c.to_bits() as i64);
        let thrown = [
            (
                0.0,
                0.0,
                [
                    -4658604453255681268,
                    4595435376155295744,
                    4598847571894135436,
                ],
            ),
            (
                37.5,
                -21.25,
                [
                    -4627867408251048793,
                    4597245257657090048,
                    // Vanilla's `Math.sin` is HotSpot's intrinsic, which rounds this
                    // one a bit lower on AArch64 than on x86-64 (`jmath`).
                    if cfg!(target_arch = "aarch64") {
                        4597448471913462162
                    } else {
                        4597448471913462163
                    },
                ],
            ),
            (
                -123.4,
                60.0,
                [
                    4593476083220988603,
                    -4626282285467959296,
                    -4631965182103987462,
                ],
            ),
        ];
        for (yaw, pitch, want) in thrown {
            assert_eq!(
                bits(drop_motion(&mut r, yaw, pitch, false)),
                want,
                "{yaw} {pitch}"
            );
        }
        let scattered = [
            [
                -4634512480041697280,
                4596373779801702400,
                -4661085178024689664,
            ],
            [
                4600899508711194624,
                4596373779801702400,
                -4629159495333838848,
            ],
        ];
        for want in scattered {
            assert_eq!(bits(drop_motion(&mut r, 0.0, 0.0, true)), want);
        }
        assert_eq!(r.next_int(), 357425922);
    }
}

/// `LivingEntity.createItemStackToDrop`'s motion for a stack thrown by an
/// entity facing `yaw` and `pitch` (degrees), from that entity's random:
/// `randomly` scatters it in any direction (a death's drops), else it goes
/// along the view (the drop key).
pub fn drop_motion(
    r: &mut impl horizon_core::random::RandomSource,
    yaw: f32,
    pitch: f32,
    randomly: bool,
) -> [f64; 3] {
    use horizon_core::{
        geometry::trig::{DEG_TO_RAD, cos, sin},
        jmath,
    };
    let tau = std::f32::consts::TAU;
    if randomly {
        let pow = r.next_float() * 0.5;
        let dir = f64::from(r.next_float() * tau);
        return [
            f64::from(-sin(dir) * pow),
            f64::from(0.2f32),
            f64::from(cos(dir) * pow),
        ];
    }
    let (x, y) = (f64::from(pitch * DEG_TO_RAD), f64::from(yaw * DEG_TO_RAD));
    let (sin_x, cos_x, sin_y, cos_y) = (sin(x), cos(x), sin(y), cos(y));
    let dir = f64::from(r.next_float() * tau);
    let pow = f64::from(0.02 * r.next_float());
    [
        f64::from(-sin_y * cos_x * 0.3) + jmath::cos(dir) * pow,
        f64::from(-sin_x * 0.3 + 0.1 + (r.next_float() - r.next_float()) * 0.1),
        f64::from(cos_y * cos_x * 0.3) + jmath::sin(dir) * pow,
    ]
}

#[derive(Clone, Debug)]
pub struct ItemEntity {
    /// `Entity.yRot` and `xRot` (`/tp`, `/data get ... Rotation`).
    pub rotation: [f32; 2],
    /// When it joined its list ([`crate::join_seq`]); set by the list, not by the spawn site.
    pub join: crate::join_seq::JoinSeq,
    pub health: i32,
    pub fire_ticks: i32,
    pub id: u32,
    pub stack: ItemStack,
    pub thrower: Option<[i32; 4]>,
    /// `ItemEntity.target`: the only player who may pick the item up.
    pub target: Option<[i32; 4]>,
    pub position: Vec3,
    pub previous: Vec3,
    pub velocity: Vec3,
    pub on_ground: bool,
    pub age: i32,
    /// Signed NBT counter; widened to retain the legacy internal u32::MAX sentinel.
    pub pickup_delay: i64,
    pub bob_offset: f32,
    /// `Entity.tickCount`: ticks since the item entered the level, which
    /// paces `ItemEntity.mergeWithNeighbours`.
    pub ticks: u32,
    /// `Entity.needsSync` / `syncPosition`: immediate `ServerEntity.sendChanges`
    /// after a bounce or velocity jump (`> 0.1` in `ItemEntity.tick`).
    pub needs_sync: bool,
    /// `Entity.stuckSpeedMultiplier`: set after a move by the cobweb or powder snow
    /// the item is in, scales (and stops) the next one.
    pub stuck: Vec3,
    /// `Entity.wasTouchingWater`: the last fluid push. Entering water after
    /// the first tick is what splashes.
    pub was_touching_water: bool,
    /// `Entity.noGravity` (`NoGravity` in its save): `applyGravity` does nothing.
    pub no_gravity: bool,
    /// `Entity.verticalCollision`: the last move was stopped vertically.
    /// `ServerEntity.isFullPrecisionEncodingRequired` reads it for items, whose
    /// resting position is exact while a 1/4096 delta is not.
    pub vertical_collision: bool,
    /// `Entity.horizontalCollision`: the last move was stopped on x or z.
    pub horizontal_collision: bool,
    /// `Entity.random`, after the constructor's draws: `moveTowardsClosestSpace`
    /// takes an item's escape speed from it.
    pub random: horizon_core::random::LegacyRandom,
    /// `Entity.fallDistance` (`fall_distance` in its save): what the item has
    /// fallen since it last touched ground or water.
    pub fall_distance: f64,
}
/// What `Entity.doWaterSplashEffect` asks the level to do for a dropped item.
/// Particles are drawn on the client (`Level.addParticle` is empty on the
/// server). The sound is `Entity.playSound`, which the server broadcasts;
/// `ClientLevel.playSeededSound` ignores it when the except entity is null.
#[derive(Clone, Copy, Debug)]
pub enum ItemSplash {
    Sound {
        at: [f64; 3],
        volume: f32,
        pitch: f32,
    },
    Particle {
        bubble: bool,
        at: [f64; 3],
        velocity: [f64; 3],
    },
}
/// `Entity.doWaterSplashEffect` of an entity with no rider, `width` wide, at
/// `position` moving at `velocity` (a dropped item, a firework rocket).
pub fn water_splash_effect(
    position: Vec3,
    velocity: Vec3,
    width: f32,
    random: &mut horizon_core::random::LegacyRandom,
    splash: &mut impl FnMut(ItemSplash),
) {
    let mut water = ItemWater {
        position,
        velocity,
        width,
        random,
        splash,
    };
    let _ = horizon_world::water_splash::water_splash(&mut water, 1_000_000);
}
struct ItemWater<'a, F> {
    position: Vec3,
    velocity: Vec3,
    width: f32,
    random: &'a mut horizon_core::random::LegacyRandom,
    splash: &'a mut F,
}
impl<F: FnMut(ItemSplash)> horizon_world::water_splash::WaterSplashWorld for ItemWater<'_, F> {
    type Error = std::convert::Infallible;
    type Entity = ();
    type Sound = ();
    fn this_entity(&self) {}
    fn controlling_passenger(&mut self) -> Result<Option<()>, Self::Error> {
        Ok(None)
    }
    fn velocity(&mut self, _: ()) -> Result<Vec3, Self::Error> {
        Ok(self.velocity)
    }
    fn splash_sound(&mut self, _: bool) -> Result<(), Self::Error> {
        Ok(())
    }
    fn random_float(&mut self) -> Result<f32, Self::Error> {
        Ok(self.random.next_float())
    }
    fn random_double(&mut self) -> Result<f64, Self::Error> {
        Ok(self.random.next_double())
    }
    fn play_sound(&mut self, _: (), volume: f32, pitch: f32) -> Result<(), Self::Error> {
        let at = [self.position.x, self.position.y, self.position.z];
        (self.splash)(ItemSplash::Sound { at, volume, pitch });
        Ok(())
    }
    fn position(&self) -> Vec3 {
        self.position
    }
    fn width(&self) -> f32 {
        self.width
    }
    fn particle(
        &mut self,
        kind: horizon_world::water_splash::SplashParticle,
        position: Vec3,
        velocity: Vec3,
    ) -> Result<(), Self::Error> {
        (self.splash)(ItemSplash::Particle {
            bubble: matches!(kind, horizon_world::water_splash::SplashParticle::Bubble),
            at: [position.x, position.y, position.z],
            velocity: [velocity.x, velocity.y, velocity.z],
        });
        Ok(())
    }
    fn splash_event(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}
/// Environment results needed by the entity owner, including lethal lava damage.
#[derive(Debug)]
pub struct ItemEnvironmentTick {
    pub alive: bool,
    pub lava_hurt: bool,
    /// `ItemStack.onDestroyed` of the damage that emptied the health: the
    /// stacks the destroyed item left, which `ItemUtils.onContainerDestroyed`
    /// adds to the level where it lay.
    pub spilled: Vec<ItemStack>,
}
impl ItemEnvironmentTick {
    pub fn should_play_lava_hurt_sound(&self, tick: u32) -> bool {
        self.lava_hurt && (!self.alive || tick.is_multiple_of(10))
    }
}
impl crate::entity_rules::EntityRules for ItemEntity {
    fn is_no_gravity(&self) -> bool {
        self.no_gravity
    }
    /// `ItemEntity.getDefaultGravity`.
    fn get_default_gravity(&self) -> f64 {
        0.04
    }
}
impl crate::entity_rules::EntityRules for ExperienceOrb {
    fn is_no_gravity(&self) -> bool {
        self.no_gravity
    }
    /// `ExperienceOrb.getDefaultGravity`.
    fn get_default_gravity(&self) -> f64 {
        0.03
    }
}
impl crate::entity_rules::EntityRules for ArmorStand {
    fn is_no_gravity(&self) -> bool {
        self.no_gravity
    }
    /// `LivingEntity.getDefaultGravity`.
    fn get_default_gravity(&self) -> f64 {
        0.08
    }
    /// The code's reading of `Entity.isPushedByFluid` for a marker stand: it
    /// takes no fluid push and is never in water.
    fn is_pushed_by_fluid(&self) -> bool {
        !self.marker
    }
}
/// What an item's tick carries from [`ItemEntity::tick_move`] to
/// [`ItemEntity::tick_settle`], across the game's bubble-column push.
#[derive(Clone, Copy)]
pub struct ItemStep {
    old_velocity: Vec3,
    did_move: bool,
    /// The contact block for a clipped vertical move; idle ticks have none.
    pub vertical_contact: Option<BlockPos>,
    /// `Entity.move` with physics: where it started, what it moved, and
    /// whether the move was applied. `None` while idle or without physics,
    /// where `checkFallDamage` does not run.
    pub fall_move: Option<(Vec3, Vec3, bool)>,
}
#[cfg(test)]
impl ItemEntity {
    /// A whole `ItemEntity.tick` for the unit tests, in the order the game
    /// runs it: fluid push, move, bubble column, settle, fluid push.
    pub(crate) fn tick(
        &mut self,
        world: &World,
        registry: &horizon_world::registry::BlockRegistry,
        facts: &BlockFacts,
        collision: &BlockCollisionRegistry<'_>,
        no_physics: bool,
        fluids: &crate::motion::FluidResources,
        entity_boxes: &[Aabb],
        border: Option<&BorderGeometry>,
        blocks: Option<&BoundMovementProperties>,
    ) -> bool {
        let before = self.velocity;
        let (water, lava) = self.update_fluids(world, fluids);
        self.water_entry(water > 0.0, self.ticks == 0, before, &mut |_| {});
        self.lava_fall(lava > 0.0);
        let step = self.tick_move(
            world,
            facts,
            collision,
            no_physics,
            (water, lava),
            entity_boxes,
            border,
            blocks,
        );
        crate::motion::bubble_column_push(
            world,
            registry,
            facts,
            collision,
            self.bounds(),
            &mut self.velocity,
        );
        let alive = self.tick_settle(world, facts, no_physics, step);
        let before = self.velocity;
        let (water, _) = self.update_fluids(world, fluids);
        self.water_entry(water > 0.0, false, before, &mut |_| {});
        alive
    }
}
impl ItemEntity {
    pub const WIDTH: f64 = 0.25;
    /// `EntityType.ITEM` declares `eyeHeight(0.2125)`: an item is lit from the
    /// block its eye is in, most of a block above the one its base rests in.
    pub const EYE_HEIGHT: f64 = 0.2125;
    /// `Entity.getLightProbePosition(partialTick)` for a dropped item: the eye
    /// position (`getEyePosition`) over the position drawn this frame
    /// (`getPosition(partialTick)`), not this tick's own.
    pub fn light_probe(&self, partial: f64) -> [f64; 3] {
        [
            self.previous.x + (self.position.x - self.previous.x) * partial,
            self.previous.y + (self.position.y - self.previous.y) * partial + Self::EYE_HEIGHT,
            self.previous.z + (self.position.z - self.previous.z) * partial,
        ]
    }
    /// `EntityRenderer.getPackedLightCoords` for a dropped item: the level's
    /// sky light over the block light of the cell it is lit from, which
    /// `EntityRenderer.getBlockLightLevel` puts at 15 while the item
    /// `isOnFire`. `light` is the level's packed light (sky high, block low);
    /// `environment_tick` clears the fire ticks of a fire-immune stack, as
    /// `ItemEntity.fireImmune` makes it never on fire.
    pub fn render_light(&self, light: u8) -> u32 {
        let block = if self.fire_ticks > 0 { 15 } else { light & 15 };
        (u32::from(light >> 4) * 16) | ((u32::from(block) * 16) << 8)
    }
    pub fn bounds(&self) -> Aabb {
        let h = Self::WIDTH / 2.0;
        Aabb::new(
            Vec3::new(self.position.x - h, self.position.y, self.position.z - h),
            Vec3::new(
                self.position.x + h,
                self.position.y + Self::WIDTH,
                self.position.z + h,
            ),
        )
    }
    pub fn environment_tick(
        &mut self,
        world: &World,
        facts: &BlockFacts,
        registry: &horizon_world::registry::BlockRegistry,
        items: &crate::items::Items,
    ) -> ItemEnvironmentTick {
        let at = world.block(
            self.position.x.floor() as i32,
            (self.position.y + 0.1).floor() as i32,
            self.position.z.floor() as i32,
        );
        let fluid = facts.get(at).fluid;
        let fire_immune = items.resists_fire(&self.stack, "minecraft:in_fire");
        let mut spilled = Vec::new();
        if fluid == Fluid::Water || fire_immune {
            self.fire_ticks = 0;
        }
        if self.fire_ticks > 0 {
            if self.fire_ticks % 20 == 0
                && fluid != Fluid::Lava
                && !items.resists_fire(&self.stack, "minecraft:on_fire")
            {
                spilled.extend(self.hurt(1).unwrap_or_default());
            }
            self.fire_ticks -= 1;
        }
        let mut lava_hurt = false;
        // LavaCauldronBlock.entityInside applies LAVA_IGNITE and then lavaHurt.
        // A cauldron is not a lava fluid, and removed items get no block effects.
        if self.health > 0 && !fire_immune {
            let contacts = usize::from(fluid == Fluid::Lava)
                + lava_cauldron_contacts(world, registry, self.bounds());
            if contacts > 0 {
                self.fire_ticks = self.fire_ticks.max(300);
            }
            // StepBasedCollector coalesces ignition, but keeps each block's
            // runAfter(lavaHurt) callback. Stop once an item is destroyed.
            for _ in 0..contacts {
                if self.health <= 0 {
                    break;
                }
                if !items.resists_fire(&self.stack, "minecraft:lava") {
                    spilled.extend(self.hurt(4).unwrap_or_default());
                    lava_hurt = true;
                }
            }
        }
        let fire_damage = crate::motion::environment::fire_contact(world, registry, self.bounds());
        if fire_damage > 0.0 && !fire_immune {
            self.fire_ticks = self.fire_ticks.max(160);
            if !items.resists_fire(&self.stack, "minecraft:in_fire") {
                spilled.extend(self.hurt(fire_damage as i32).unwrap_or_default());
            }
        }
        // CactusBlock.entityInside: `entity.hurt(cactus, 1.0)`. An item has no
        // hurt cooldown, so it loses 1 health every tick it touches a cactus
        // (5 health: gone in 5 ticks). Fire resistance does not help;
        // `ItemStack.canBeHurtBy` only skips it for a damage_resistant stack.
        if crate::motion::environment::cactus_contact(world, registry, self.bounds())
            && !items.resists_cactus(&self.stack)
        {
            spilled.extend(self.hurt(1).unwrap_or_default());
        }
        ItemEnvironmentTick {
            alive: self.health > 0,
            lava_hurt,
            spilled,
        }
    }
    /// `ItemEntity.hurtServer`'s health: `amount` off the health, and an item
    /// whose health reaches 0 is destroyed at once (`ItemStack.onDestroyed`,
    /// then `discard`), so what it held leaves it here. None while the item
    /// survives; the emptied stacks otherwise.
    pub fn hurt(&mut self, amount: i32) -> Option<Vec<ItemStack>> {
        self.health -= amount;
        if self.health > 0 {
            return None;
        }
        // `ItemStack.onDestroyed` runs the item's own arm, and the component
        // the stack holds is the dispatch: a bundle's (`BundleItem.onDestroyed`)
        // `bundle_contents` and a container item's (`BlockItem.onDestroyed`)
        // `container`, which a mod's own item carries in the same way.
        let mut spilled = crate::bundle::destroyed(&mut self.stack);
        spilled.extend(crate::item_data::destroyed_container(&mut self.stack));
        Some(spilled)
    }
    /// The ItemEntity snapshot used by both its move and its noCollision check.
    fn collision_context(&self) -> horizon_world::collision_context::CollisionContext<'static> {
        use horizon_world::collision_context::{
            CollisionContext, CollisionEntity, EntityContextKind,
        };
        CollisionContext::for_entity(
            CollisionEntity {
                identity: horizon_world::entity_obstruction::EntityIdentity(u64::from(self.id)),
                kind: EntityContextKind::Item,
                y: self.position.y,
                descending: false,
                main_hand: None,
            },
            false,
        )
    }
    /// `Entity.updateFluidInteraction`: water resets the fall, and entering
    /// it after the first tick runs `doWaterSplashEffect` at `velocity`, the
    /// motion before the fluids' currents push it.
    pub fn water_entry<F: FnMut(ItemSplash)>(
        &mut self,
        in_water: bool,
        first_tick: bool,
        velocity: Vec3,
        splash: &mut F,
    ) {
        if in_water {
            self.fall_distance = 0.0;
        }
        if in_water && !self.was_touching_water && !first_tick {
            let mut random = horizon_core::random::LegacyRandom::new(
                (self.id as i64)
                    .wrapping_mul(0x5DEECE66D)
                    .wrapping_add(self.ticks as i64),
            );
            let width = Self::WIDTH as f32;
            water_splash_effect(self.position, velocity, width, &mut random, splash);
        }
        self.was_touching_water = in_water;
    }
    /// `Entity.moveTowardsClosestSpace` from the middle of the item's box:
    /// towards the nearest side of its block (north, south, west, east or
    /// up) without a full collision cube, at `speed` (`nextFloat() * 0.2 +
    /// 0.1`).
    pub fn move_towards_closest_space(&mut self, world: &World, facts: &BlockFacts, speed: f32) {
        self.velocity = towards_closest_space(
            self.position,
            self.bounds(),
            self.velocity,
            world,
            facts,
            speed,
        );
    }
    /// `ItemEntity.setUnderwaterMovement` / `setUnderLavaMovement`: horizontal
    /// drag is a float (`0.99F` / `0.95F`), and a slow item is lifted by
    /// `5.0E-4F` while slower than `0.06F`.
    fn set_fluid_movement(&mut self, drag: f32) {
        let drag = f64::from(drag);
        let lift = if self.velocity.y < f64::from(0.06_f32) {
            f64::from(5.0e-4_f32)
        } else {
            0.0
        };
        self.velocity = Vec3::new(
            self.velocity.x * drag,
            self.velocity.y + lift,
            self.velocity.z * drag,
        );
    }
    /// `ItemEntity.tick` up to the blocks' push. The game has just run the
    /// fluid push (`Entity::push_by_fluid`) and passes the heights it found;
    /// it pushes by a bubble column next (`Entity::push_by_bubble_column`) and
    /// then calls [`ItemEntity::tick_settle`]. `no_physics`: the item moves
    /// through blocks this tick ([`ItemEntity::inside_block`]).
    pub fn tick_move(
        &mut self,
        world: &World,
        facts: &BlockFacts,
        collision: &BlockCollisionRegistry<'_>,
        no_physics: bool,
        heights: (f64, f64),
        entity_boxes: &[Aabb],
        border: Option<&BorderGeometry>,
        blocks: Option<&BoundMovementProperties>,
    ) -> ItemStep {
        let step = self.tick_move_before_block_effects(
            world,
            facts,
            collision,
            no_physics,
            None,
            heights,
            entity_boxes,
            border,
            blocks,
        );
        self.finish_move_block_effects(world, facts, no_physics, blocks, step);
        step
    }

    /// `Entity.move`: resolve the item's move before the owner handles block
    /// contact and resumes block effects. `escape_speed`: with `no_physics`
    /// on a server, the speed of `Entity.moveTowardsClosestSpace`
    /// (`nextFloat() * 0.2 + 0.1`).
    pub fn tick_move_before_block_effects(
        &mut self,
        world: &World,
        facts: &BlockFacts,
        collision: &BlockCollisionRegistry<'_>,
        no_physics: bool,
        escape_speed: Option<f32>,
        (water_height, lava_height): (f64, f64),
        entity_boxes: &[Aabb],
        border: Option<&BorderGeometry>,
        blocks: Option<&BoundMovementProperties>,
    ) -> ItemStep {
        self.ticks += 1;
        self.previous = self.position;
        if self.pickup_delay > 0 && !self.has_infinite_pickup_delay() {
            self.pickup_delay -= 1;
        }
        let old_velocity = self.velocity;
        let in_water = water_height > f64::from(0.1f32);
        if in_water {
            // ItemEntity.setUnderwaterMovement: 0.99F, 5.0E-4F, 0.06F.
            self.set_fluid_movement(0.99_f32);
        } else if lava_height > f64::from(0.1f32) {
            // ItemEntity.setUnderLavaMovement: 0.95F, 5.0E-4F, 0.06F.
            self.set_fluid_movement(0.95_f32);
        } else {
            self.velocity.y -= self.get_gravity();
        }
        // ItemEntity.tick: out of a block placed on it after gravity, so the
        // escape's 0.75 scales the fallen vertical speed (#12018).
        if let Some(speed) = escape_speed {
            self.move_towards_closest_space(world, facts, speed);
        }
        let horizontal = self.velocity.x * self.velocity.x + self.velocity.z * self.velocity.z;
        // ItemEntity.tick: skip move 3 of 4 ticks while grounded with no
        // horizontal speed. The idle branch still runs bubble-column push
        // (`applyEffectsFromBlocksForLastMovements`), so lift builds up and
        // the item moves on the next wake-up (#4558).
        if !self.on_ground
            || horizontal > 1.0e-5
            || self.ticks.wrapping_add(self.id).is_multiple_of(4)
        {
            let scene = SceneCollisions {
                world,
                binding: collision,
                subject: None,
            };
            let from = self.position;
            let mut wanted = self.velocity;
            // Entity.move: a block the item sits in (cobweb, powder snow)
            // scales this move once and stops it.
            if self.stuck.length_squared() > 1.0e-7 {
                wanted = wanted.multiply(self.stuck);
                self.stuck = Vec3::ZERO;
                self.velocity = Vec3::ZERO;
            }
            // Entity.move with noPhysics: straight through, the ground as it was.
            let moved = if no_physics {
                wanted
            } else {
                // `Entity.collide`: the entities that `canBeCollidedWith` the item stop it
                // too (a boat, a shulker, a happy ghast held still; not a minecart), and
                // so does the world border (`Entity.collectColliders`).
                crate::collisions::EntityCollisions {
                    blocks: scene,
                    entities: entity_boxes,
                    border: border.map(|b| (b, [self.position.x, self.position.z])),
                }
                .resolve_with_context(
                    self.bounds(),
                    wanted,
                    0.0,
                    self.on_ground,
                    self.collision_context(),
                )
                .unwrap_or(Vec3::ZERO)
            };
            let applied = move_applies(wanted, moved);
            if applied {
                self.position = Vec3::new(
                    self.position.x + moved.x,
                    self.position.y + moved.y,
                    self.position.z + moved.z,
                );
            }
            if !no_physics {
                self.on_ground = wanted.y < 0.0 && moved.y != wanted.y;
            }
            // Entity.move: what the move was stopped on. They outlive the
            // tick, as vanilla's fields do, for the tracker that sends the
            // position at the type's update interval.
            self.vertical_collision = moved.y != wanted.y;
            // `Mth.equal` on x and z (1e-5 as a float), exact on y.
            let differs = |a: f64, b: f64| (b - a).abs() >= 9.999_999_747_378_752e-6;
            self.horizontal_collision = differs(moved.x, wanted.x) || differs(moved.z, wanted.z);
            if moved.x != wanted.x {
                self.velocity.x = 0.0;
            }
            if moved.z != wanted.z {
                self.velocity.z = 0.0;
            }
            let vertical_contact = if self.vertical_collision && !no_physics {
                let contact = self.vertical_contact(world, collision, blocks, moved);
                let slime =
                    self.on_ground && contact.is_some_and(|pos| slime_at(world, facts, pos));
                self.velocity.y = vertical_collision_velocity(
                    wanted.y,
                    moved.y,
                    ITEM_GRAVITY,
                    self.on_ground,
                    slime,
                );
                contact
            } else {
                None
            };
            return ItemStep {
                old_velocity,
                did_move: true,
                vertical_contact,
                fall_move: (!no_physics).then_some((from, moved, applied)),
            };
        }
        // applyEffectsFromBlocksForLastMovements: still lift/drag while idle.
        if !no_physics {
            self.slime_step_on(world, facts);
        }
        ItemStep {
            old_velocity,
            did_move: false,
            vertical_contact: None,
            fall_move: None,
        }
    }

    /// `Entity.getOnPosLegacy` for a clipped move. Resolve the support under
    /// the full item box: at a honey wall, its center can be outside the slime
    /// block even though its box landed on it.
    fn vertical_contact(
        &self,
        world: &World,
        collision: &BlockCollisionRegistry<'_>,
        blocks: Option<&BoundMovementProperties>,
        movement: Vec3,
    ) -> Option<BlockPos> {
        let mut scene = SceneCollisions {
            world,
            binding: collision,
            subject: None,
        };
        let mut ground = horizon_world::ground_support::GroundState::default();
        ground
            .check_support(self.on_ground, self.bounds(), Some(movement), |bounds| {
                horizon_world::ground_support::find_supporting_block(
                    &mut scene,
                    horizon_world::collision_context::CollisionContext::empty(false),
                    self.position,
                    bounds,
                    1_000_000,
                )
            })
            .ok()?;
        if let Some(blocks) = blocks {
            return Some(crate::motion::legacy_block_support_at(
                world,
                blocks,
                self.position,
                ground.supporting_block,
            ));
        }
        // Standalone tests can omit the movement table and its fence/wall tags.
        Some(ground.supporting_block.unwrap_or_else(|| {
            let offset = f64::from(horizon_world::movement_support::LEGACY_SUPPORT_OFFSET);
            BlockPos::containing(self.position.subtract(Vec3::new(0.0, offset, 0.0)))
        }))
    }

    /// Resume the normal block effects after the collision response.
    pub fn finish_move_block_effects(
        &mut self,
        world: &World,
        facts: &BlockFacts,
        no_physics: bool,
        blocks: Option<&BoundMovementProperties>,
        step: ItemStep,
    ) {
        if step.did_move {
            // Entity.move applies the block's speed factor before block effects.
            if !no_physics && let Some(blocks) = blocks {
                let factor =
                    crate::motion::block_speed_factor_at(world, blocks, self.position, None);
                self.velocity =
                    horizon_world::entity_move::scale_by_block_speed(self.velocity, factor);
            }
            self.apply_honey_slide(world, facts);
            self.stuck = match self.stuck_multiplier(world, facts) {
                // Entity.makeStuckInBlock resets the fall.
                Some(stuck) => {
                    self.fall_distance = 0.0;
                    stuck
                }
                None => Vec3::ZERO,
            };
        }
    }
    /// The rest of `ItemEntity.tick`, after the bubble column's push: slime,
    /// drag, block contact, age. Returns false when the item should be
    /// removed. The game then runs the fluid push again (`ItemEntity.tick`
    /// refreshes immersion after moving).
    pub fn tick_settle(
        &mut self,
        world: &World,
        facts: &BlockFacts,
        no_physics: bool,
        step: ItemStep,
    ) -> bool {
        if step.did_move {
            // Entity.applyEffectsFromBlocks -> SlimeBlock.stepOn, with the
            // vertical speed the bounce just left (before the air drag).
            if !no_physics {
                self.slime_step_on(world, facts);
            }
            // `float friction = getAirDrag()` (0.98F); on the ground, times
            // the block's friction as a float, then widened for the scale.
            let mut friction = 0.98_f32;
            if self.on_ground {
                let below = world.block(
                    self.position.x.floor() as i32,
                    (self.position.y - 0.999_999).floor() as i32,
                    self.position.z.floor() as i32,
                );
                friction *= facts.get(below).friction;
            }
            let friction = f64::from(friction);
            let air = f64::from(0.98_f32);
            self.velocity = Vec3::new(
                self.velocity.x * friction,
                self.velocity.y * air,
                self.velocity.z * friction,
            );
            if self.on_ground && self.velocity.y < 0.0 {
                self.velocity.y *= -0.5;
            }
        }
        if !no_physics {
            // Entity.applyEffectsFromBlocks: stepOn, then checkInsideBlocks.
            if self.on_ground {
                let p = BlockPos::containing(self.position.add_vector(Vec3::new(
                    0.0,
                    -f64::from(0.2_f32),
                    0.0,
                )));
                let state = world.block(p.x, p.y, p.z);
                crate::block_contact::emit(crate::block_contact::Event::StepOn(
                    crate::block_contact::Contact {
                        entity: self.id as i32,
                        kind: "minecraft:item",
                        block: &facts.get(state).name,
                        state,
                        position: p,
                    },
                ));
            }
            crate::block_contact::swept_inside(
                self.id as i32,
                "minecraft:item",
                self.previous,
                self.position,
                self.bounds(),
                |p| {
                    let state = world.block(p.x, p.y, p.z);
                    (state, &facts.get(state).name)
                },
            );
        }
        if self.velocity.subtract(step.old_velocity).length_squared() > 0.01 {
            self.needs_sync = true;
        }
        if self.age != i16::MIN as i32 {
            self.age += 1;
        }
        self.age < 6000 && self.position.y > -128.0
    }
    /// `Entity.makeStuckInBlock` as `WebBlock.entityInside` and
    /// `PowderSnowBlock.entityInside` call it for an item (which, unlike a
    /// living entity, is stuck whatever block its feet are in): the speed
    /// factor of the last such block the item's box (deflated by `1.0E-5`) is
    /// in.
    fn stuck_multiplier(&self, world: &World, facts: &BlockFacts) -> Option<Vec3> {
        let b = self.bounds().deflate(Vec3::new(1e-5, 1e-5, 1e-5));
        let (lo, hi) = (b.min_position(), b.max_position());
        let mut stuck = None;
        for x in lo.x.floor() as i32..=hi.x.floor() as i32 {
            for y in lo.y.floor() as i32..=hi.y.floor() as i32 {
                for z in lo.z.floor() as i32..=hi.z.floor() as i32 {
                    match &*facts.get(world.block(x, y, z)).name {
                        "minecraft:cobweb" => {
                            stuck = Some(Vec3::new(0.25, f64::from(0.05_f32), 0.25));
                        }
                        "minecraft:powder_snow" => {
                            stuck = Some(Vec3::new(f64::from(0.9_f32), 1.5, f64::from(0.9_f32)));
                        }
                        _ => {}
                    }
                }
            }
        }
        stuck
    }
    /// `SlimeBlock.stepOn` for an item on the ground: slow vertical speed
    /// drags the horizontal speed, on idle ticks too.
    fn slime_step_on(&mut self, world: &World, facts: &BlockFacts) {
        if !self.on_ground {
            return;
        }
        let p = BlockPos::containing(self.position.add_vector(Vec3::new(
            0.0,
            -f64::from(0.2_f32),
            0.0,
        )));
        if &*facts.get(world.block(p.x, p.y, p.z)).name != "minecraft:slime_block" {
            return;
        }
        if let Some(scale) = slime_step_on_scale(self.velocity.y) {
            self.velocity.x *= scale;
            self.velocity.z *= scale;
        }
    }

    /// `PistonMovingBlockEntity.causeBounce` for a dropped item. Vanilla then
    /// runs `Entity.move(MoverType.PISTON)`, which leaves the ground on an
    /// upward push; without that, `ItemEntity.tick` skips movement 3 of 4 ticks
    /// while `onGround` with no horizontal speed and gravity eats the bounce.
    pub fn piston_slime_bounce(&mut self, movement: Direction) {
        self.velocity = horizon_world::piston_push::bounce_velocity(movement, self.velocity);
        self.on_ground = false;
        self.needs_sync = true;
    }
    /// `HoneyBlock.entityInside` for the honey blocks this item's box is in
    /// (`Entity.checkInsideBlocks`, deflated by `1.0E-5`). Slide sounds and
    /// entity event 53 are skipped (`doesEntityDoHoneyBlockSlideEffects` is
    /// false for an item).
    fn apply_honey_slide(&mut self, world: &World, facts: &BlockFacts) {
        let b = self.bounds().deflate(Vec3::new(1e-5, 1e-5, 1e-5));
        let (lo, hi) = (b.min_position(), b.max_position());
        for x in lo.x.floor() as i32..=hi.x.floor() as i32 {
            for y in lo.y.floor() as i32..=hi.y.floor() as i32 {
                for z in lo.z.floor() as i32..=hi.z.floor() as i32 {
                    if &*facts.get(world.block(x, y, z)).name == "minecraft:honey_block" {
                        self.honey_block_inside([x, y, z]);
                    }
                }
            }
        }
    }
    /// `HoneyBlock.isSlidingDown` then `doSlideMovement`. `getOldDeltaY` /
    /// `getNewDeltaY` wrap the stored velocity the same way they do for every
    /// entity (`vy / 0.98F + 0.08`).
    pub(crate) fn honey_block_inside(&mut self, pos: [i32; 3]) {
        let old_delta_y = |dy: f64| dy / f64::from(0.98_f32) + 0.08;
        let new_delta_y = |dy: f64| (dy - 0.08) * f64::from(0.98_f32);
        let [x, y, z] = pos.map(f64::from);
        if self.on_ground
            || self.position.y > y + 0.9375 - 1.0e-7
            || old_delta_y(self.velocity.y) >= -0.08
        {
            return;
        }
        let dx = (x + 0.5 - self.position.x).abs();
        let dz = (z + 0.5 - self.position.z).abs();
        let overlap = 0.4375 + Self::WIDTH / 2.0;
        if dx + 1.0e-7 <= overlap && dz + 1.0e-7 <= overlap {
            return;
        }
        let v = self.velocity;
        let old = old_delta_y(v.y);
        self.velocity = if old < -0.13 {
            let factor = -0.05 / old;
            Vec3::new(v.x * factor, new_delta_y(-0.05), v.z * factor)
        } else {
            Vec3::new(v.x, new_delta_y(-0.05), v.z)
        };
    }
    fn has_infinite_pickup_delay(&self) -> bool {
        self.pickup_delay == i64::from(i16::MAX) || self.pickup_delay == i64::from(u32::MAX)
    }
    /// `ItemEntity.isMergable` at `age`: a stack below its size whose pickup
    /// delay isn't infinite, before its lifetime ends.
    fn mergeable(&self, age: i32, max_stack: u32) -> bool {
        !self.has_infinite_pickup_delay()
            && age != i16::MIN as i32
            && age < 6000
            && self.stack.count < max_stack
    }
}

/// `LavaCauldronBlock.getEntityInsideCollisionShape`: the cauldron's solid
/// shape plus its contents (12 pixels wide, from 4 through 15 pixels high).
/// In particular, the rim and feet count too, but air above the contents does not.
fn lava_cauldron_contacts(
    world: &World,
    registry: &horizon_world::registry::BlockRegistry,
    bounds: Aabb,
) -> usize {
    use horizon_core::geometry::ShapeOp;
    static SHAPE: std::sync::OnceLock<VoxelShape> = std::sync::OnceLock::new();
    let epsilon = f64::from(1e-5_f32);
    let mut contacts = 0;
    let candidates = bounds.deflate(Vec3::new(epsilon, epsilon, epsilon));
    for x in candidates.min_position().x.floor() as i32..=candidates.max_position().x.floor() as i32
    {
        for y in
            candidates.min_position().y.floor() as i32..=candidates.max_position().y.floor() as i32
        {
            for z in candidates.min_position().z.floor() as i32
                ..=candidates.max_position().z.floor() as i32
            {
                if !registry
                    .state(world.block(x, y, z))
                    .is_some_and(|s| s.name == "minecraft:lava_cauldron")
                {
                    continue;
                }
                let shape = SHAPE.get_or_init(|| {
                    let cuboid = |x0, y0, z0, x1, y1, z1| {
                        VoxelShape::from_box(Aabb::new(
                            Vec3::new(x0 / 16.0, y0 / 16.0, z0 / 16.0),
                            Vec3::new(x1 / 16.0, y1 / 16.0, z1 / 16.0),
                        ))
                        .expect("valid cauldron box")
                    };
                    let mut solid = VoxelShape::block();
                    // AbstractCauldronBlock.SHAPE: the openings under its feet
                    // and its hollow centre are cut out of the unit block.
                    for hole in [
                        cuboid(0.0, 0.0, 4.0, 16.0, 3.0, 12.0),
                        cuboid(4.0, 0.0, 0.0, 12.0, 3.0, 16.0),
                        cuboid(2.0, 0.0, 2.0, 14.0, 3.0, 14.0),
                        cuboid(2.0, 4.0, 2.0, 14.0, 16.0, 14.0),
                    ] {
                        solid = solid
                            .join(&hole, ShapeOp::ONLY_FIRST)
                            .expect("valid cauldron cutout");
                    }
                    solid
                        .join(&cuboid(2.0, 4.0, 2.0, 14.0, 15.0, 14.0), ShapeOp::OR)
                        .expect("valid filled cauldron")
                });
                let local = bounds.move_by(Vec3::new(-f64::from(x), -f64::from(y), -f64::from(z)));
                // Only block enumeration uses the deflated box. The actual
                // shape contact follows Entity.collidedWithShapeMovingFrom.
                if local.collided_along_vector(Vec3::ZERO, shape.boxes()) {
                    contacts += 1;
                }
            }
        }
    }
    contacts
}
/// Dropped items by block cell, for `ItemEntity.mergeWithNeighbours`'s
/// search around an item. Holds indexes into the tick's item list.
#[derive(Default)]
pub struct ItemCells {
    cells: crate::fast_hash::FxHashMap<(i32, i32, i32), Vec<u32>>,
}
impl ItemCells {
    pub fn new(items: &[ItemEntity]) -> Self {
        let mut cells = Self::default();
        for (i, item) in items.iter().enumerate() {
            cells.add(i, item.position);
        }
        cells
    }
    fn cell(p: Vec3) -> (i32, i32, i32) {
        (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)
    }
    fn add(&mut self, i: usize, at: Vec3) {
        self.cells.entry(Self::cell(at)).or_default().push(i as u32);
    }
    pub fn remove(&mut self, i: usize, at: Vec3) {
        if let Some(list) = self.cells.get_mut(&Self::cell(at)) {
            list.retain(|&j| j as usize != i);
        }
    }
    pub fn moved(&mut self, i: usize, from: Vec3, to: Vec3) {
        if Self::cell(from) != Self::cell(to) {
            self.remove(i, from);
            self.add(i, to);
        }
    }
    /// The items whose cells could hold a box meeting `area`, in the order
    /// they were added (vanilla lists a section's entities in the order they
    /// entered it).
    fn near(&self, area: Aabb, out: &mut Vec<u32>) {
        out.clear();
        // An item's box reaches 0.125 sideways and 0.25 up from its position.
        let (min, max) = (area.min_position(), area.max_position());
        let (lo, hi) = (
            Self::cell(Vec3::new(min.x - 0.125, min.y - 0.25, min.z - 0.125)),
            Self::cell(Vec3::new(max.x + 0.125, max.y, max.z + 0.125)),
        );
        for x in lo.0..=hi.0 {
            for y in lo.1..=hi.1 {
                for z in lo.2..=hi.2 {
                    if let Some(list) = self.cells.get(&(x, y, z)) {
                        out.extend_from_slice(list);
                    }
                }
            }
        }
        out.sort_unstable();
    }
}
/// `ItemEntity.mergeWithNeighbours` for `items[i]`, which has just moved in
/// its tick (its `age` already counts this tick; vanilla's doesn't yet): each
/// mergeable item whose box meets this one's grown by 0.5 sideways takes this
/// stack or gives it its own, the smaller into the larger
/// (`ItemEntity.tryToMerge`), until this item is used up. An emptied item is
/// marked in `removed`.
pub fn merge_with_neighbours(
    items: &mut [ItemEntity],
    removed: &mut [bool],
    cells: &mut ItemCells,
    i: usize,
    max_stack: &dyn Fn(&ItemStack) -> u32,
) {
    let age = |items: &[ItemEntity], j: usize| {
        if j == i && items[j].age != i16::MIN as i32 {
            items[j].age - 1
        } else {
            items[j].age
        }
    };
    if removed[i] || !items[i].mergeable(age(items, i), max_stack(&items[i].stack)) {
        return;
    }
    let area = items[i].bounds().inflate(Vec3::new(0.5, 0.0, 0.5));
    let mut near = Vec::new();
    cells.near(area, &mut near);
    for j in near {
        let j = j as usize;
        if j == i
            || removed[j]
            || !items[j].bounds().intersects(area)
            || !items[j].mergeable(age(items, j), max_stack(&items[j].stack))
        {
            continue;
        }
        // ItemEntity.areMergable: both fit one stack of the same item and components.
        let (this, other) = (&items[i].stack, &items[j].stack);
        if items[i].target != items[j].target
            || this.count + other.count > max_stack(other)
            || !this.same_item_same_components(other)
        {
            continue;
        }
        let (to, from) = if other.count < this.count {
            (i, j)
        } else {
            (j, i)
        };
        // ItemEntity.merge(toStack, fromStack, 64).
        // A component limit above 64 permits an overstacked target. Vanilla's
        // signed room then moves the excess back into the source stack.
        let room =
            i64::from(max_stack(&items[to].stack).min(64)) - i64::from(items[to].stack.count);
        let moved = room.min(i64::from(items[from].stack.count));
        items[to].stack.count = (i64::from(items[to].stack.count) + moved) as u32;
        items[from].stack.count = (i64::from(items[from].stack.count) - moved) as u32;
        let (delay, from_age, to_age) =
            (items[from].pickup_delay, age(items, from), age(items, to));
        let to_item = &mut items[to];
        to_item.pickup_delay = to_item.pickup_delay.max(delay);
        // This item's age goes up once its tick ends.
        to_item.age = to_age.min(from_age) + i32::from(to == i);
        if items[from].stack.count == 0 {
            removed[from] = true;
            cells.remove(from, items[from].position);
        }
        if removed[i] {
            return;
        }
    }
}
/// `ExperienceOrbRenderer.submit`'s billboard before its 0.3 scale, in the
/// order of its four `vertex` calls (bottom left, bottom right, top right, top
/// left). The quad is a whole block tall about the orb's centre and hangs a
/// quarter of it below.
pub const ORB_QUAD: [[f32; 2]; 4] = [[-0.5, -0.25], [0.5, -0.25], [0.5, 0.75], [-0.5, 0.75]];
/// `poseStack.scale(0.3F, 0.3F, 0.3F)`.
pub const ORB_QUAD_SCALE: f32 = 0.3;
/// ExperienceOrb: gravity 0.03, homing towards the player within 8 blocks.
#[derive(Clone, Debug)]
pub struct ExperienceOrb {
    /// `Entity.yRot` and `xRot` (`/tp`, `/data get ... Rotation`).
    pub rotation: [f32; 2],
    /// When it joined its list ([`crate::join_seq`]); set by the list, not by the spawn site.
    pub join: crate::join_seq::JoinSeq,
    /// The entity id servers track it by.
    pub id: u32,
    pub position: Vec3,
    pub previous: Vec3,
    pub velocity: Vec3,
    /// `ExperienceOrb.DATA_VALUE`: an int that a save or command can leave at
    /// 0 or below, which a pickup turns into no experience.
    pub value: i32,
    pub age: u32,
    pub on_ground: bool,
    /// `ExperienceOrb.count`: orbs merged into this one; each pickup gives
    /// `value` once and takes one off.
    pub count: u32,
    /// `Entity.tickCount`, which paces `ExperienceOrb.scanForMerges`.
    pub ticks: u32,
    /// `ExperienceOrb.health`: damage wears it down from 5 (`hurtServer`).
    pub health: i32,
    /// `Entity.airSupply`: loaded and saved as is; an orb never breathes.
    pub air: i32,
    /// `Entity.noGravity`: `ExperienceOrb.tick`'s `applyGravity` does nothing.
    pub no_gravity: bool,
    /// `ExperienceOrb.followingPlayer`: the followed player's entity id, kept
    /// until they are gone or further than 8 blocks. Never saved.
    pub following: Option<u32>,
}
impl ExperienceOrb {
    /// `ExperienceOrb.hurtServer`: the damage comes off the health as an
    /// `int`; an orb at 0 or below is discarded. True when it is gone.
    pub fn hurt(&mut self, damage: f32) -> bool {
        self.health = (self.health as f32 - damage) as i32;
        self.health <= 0
    }
    /// The box of an orb whose `position` (the bottom of the box) is `at`.
    pub fn box_at(at: Vec3) -> Aabb {
        let h = 0.25;
        Aabb::new(
            Vec3::new(at.x - h, at.y, at.z - h),
            Vec3::new(at.x + h, at.y + 0.5, at.z + h),
        )
    }
    pub fn bounds(&self) -> Aabb {
        Self::box_at(self.position)
    }
    /// `Entity.unstuckIfPossible`, which `ExperienceOrb`'s constructor runs as
    /// `if (!level.noCollision(box)) unstuckIfPossible(box.getSize())`: the free
    /// spot nearest the box's centre, searched in a cube of the box's size
    /// around it. `None` when the box is already free (the orb then keeps its
    /// exact position) or when nothing free is that close. `shape(p)` is the
    /// block's collision shape at `p`, in world coordinates.
    pub fn unstuck(
        at: Vec3,
        border: &BorderBounds,
        mut shape: impl FnMut(BlockPos) -> Option<VoxelShape>,
    ) -> Option<Vec3> {
        let bounds = Self::box_at(at);
        let size = bounds.average_size();
        let lo = bounds.min_position();
        let cells = |v: f64| (v - 1.0e-7).floor() as i32 - 1..=(v + 1.0e-7).floor() as i32 + 1;
        // noCollision(box): a block shape reaching into it leaves the orb stuck.
        let mut stuck = false;
        'cells: for x in cells(lo.x) {
            for y in cells(lo.y) {
                for z in cells(lo.z) {
                    if shape(BlockPos::new(x, y, z))
                        .is_some_and(|s| s.boxes().iter().any(|b| b.intersects(bounds)))
                    {
                        stuck = true;
                        break 'cells;
                    }
                }
            }
        }
        if !stuck {
            return None;
        }
        // `AABB.ofSize(center, size, size, size)`: where the box's centre may
        // go, a cube of the box's mean size around its centre.
        let allowed = Aabb::of_size(bounds.center(), Vec3::new(size, size, size));
        find_free_position(allowed, bounds.center(), bounds.size(), border, shape)
            .map(|p| p.subtract(Vec3::new(0.0, bounds.size().y / 2.0, 0.0)))
    }
    /// `ExperienceOrb.canMerge(orb, id, value)` for an orb that is not removed:
    /// the same value and ids a multiple of 40 apart.
    pub fn can_merge(&self, id: u32, value: i32) -> bool {
        self.value == value && (self.id as i32).wrapping_sub(id as i32) % 40 == 0
    }
    /// The experience one pickup gives: `playerTouch` gives only a positive
    /// remainder, so an orb of value 0 or below gives none. Mods see this too.
    pub fn points(&self) -> u32 {
        self.value.max(0) as u32
    }
    /// ExperienceOrb.getExperienceValue: the largest orb size not above `value`.
    pub fn split(value: u32) -> u32 {
        [2477, 1237, 617, 307, 149, 73, 37, 17, 7, 3, 1]
            .into_iter()
            .find(|&v| value >= v)
            .unwrap_or(1)
    }
    /// ExperienceOrb.getIcon
    pub fn icon(&self) -> u32 {
        match self.value {
            2477.. => 10,
            1237.. => 9,
            617.. => 8,
            307.. => 7,
            149.. => 6,
            73.. => 5,
            37.. => 4,
            17.. => 3,
            7.. => 2,
            3.. => 1,
            _ => 0,
        }
    }
    /// `Entity.getLightProbePosition`: the block an entity is lit from is the
    /// one its eye is in. An orb's box is half a block, so its eye is
    /// `0.5 * 0.85` above its feet (`EntityDimensions.defaultEyeHeight`).
    pub fn light_probe_y(&self) -> f64 {
        self.position.y + 0.5 * 0.85
    }
    /// `ExperienceOrbRenderer.getBlockLightLevel` over `EntityRenderer`'s
    /// packing: the level's block light raised by 7 and clamped, the sky
    /// light unchanged. `light` is the level's packed light.
    pub fn render_light(&self, light: i32) -> u32 {
        let block = ((light & 15) + 7).clamp(0, 15);
        let sky = light >> 4;
        (u32::try_from(sky).unwrap_or(0) * 16) | ((u32::try_from(block).unwrap_or(15) * 16) << 8)
    }
    /// `ExperienceOrb.setUnderwaterMovement`: buoyancy and drag before moving.
    fn set_underwater_movement(&mut self) {
        // Vanilla widens float constants before applying double arithmetic.
        self.velocity = Vec3::new(
            self.velocity.x * f64::from(0.99_f32),
            (self.velocity.y + f64::from(5.0e-4_f32)).min(f64::from(0.06_f32)),
            self.velocity.z * f64::from(0.99_f32),
        );
    }
    pub fn tick(
        &mut self,
        world: &World,
        facts: &BlockFacts,
        collision: &BlockCollisionRegistry<'_>,
        player: Option<[f64; 3]>,
        server: bool,
        next_float: &mut dyn FnMut() -> f32,
        border: Option<&BorderGeometry>,
        fluids: &crate::motion::FluidResources,
        on_sound: &mut dyn FnMut([f64; 3], f32),
    ) -> bool {
        self.tick_among(
            world,
            facts,
            collision,
            player,
            server,
            next_float,
            border,
            fluids,
            on_sound,
            &[],
        )
    }
    /// [`Self::tick`], its move also stopped by `entities`: the boxes of the
    /// entities that `canBeCollidedWith` it (`EntityGetter.getEntityCollisions`).
    #[allow(clippy::too_many_arguments)]
    pub fn tick_among(
        &mut self,
        world: &World,
        facts: &BlockFacts,
        collision: &BlockCollisionRegistry<'_>,
        player: Option<[f64; 3]>,
        server: bool,
        next_float: &mut dyn FnMut() -> f32,
        border: Option<&BorderGeometry>,
        fluids: &crate::motion::FluidResources,
        on_sound: &mut dyn FnMut([f64; 3], f32),
        entities: &[Aabb],
    ) -> bool {
        self.ticks += 1;
        self.previous = self.position;
        // `Entity.baseTick`'s fluid step, whose `updateFluidOnEyes` answer the
        // water branch below reads: the orb's eye, not its feet, decides.
        let eye_in_water = self.update_fluids(world, fluids);
        let block = world.block(
            self.position.x.floor() as i32,
            self.position.y.floor() as i32,
            self.position.z.floor() as i32,
        );
        let bounds = self.bounds();
        let mut scene = SceneCollisions {
            world,
            binding: collision,
            subject: None,
        };
        // `boolean colliding = !level.noCollision(getBoundingBox())`: an orb
        // inside blocks takes no gravity until it is pushed out below.
        let colliding = !crate::collisions::no_collision_empty(&mut scene, bounds);
        // ExperienceOrb.tick: `isEyeInFluid(FluidTags.WATER)` floats the orb,
        // so an orb whose eye dips into the surface floats with its feet
        // still in the air block above it.
        if eye_in_water {
            self.set_underwater_movement();
        } else if !colliding {
            self.velocity.y -= self.get_gravity();
        }
        if facts.get(block).fluid == Fluid::Lava {
            // ExperienceOrb.tick: lava throws the orb back out.
            self.velocity = Vec3::new(
                f64::from((next_float() - next_float()) * 0.2),
                f64::from(0.2f32),
                f64::from((next_float() - next_float()) * 0.2),
            );
        }
        if let Some(p) = player {
            let d = [
                (p[0] - self.position.x) / 8.0,
                (p[1] - self.position.y) / 8.0,
                (p[2] - self.position.z) / 8.0,
            ];
            let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            let power = 1.0 - length;
            if power > 0.0 && length > 1e-6 {
                let power = power * power;
                self.velocity = Vec3::new(
                    self.velocity.x + d[0] / length * power * 0.1,
                    self.velocity.y + d[1] / length * power * 0.1,
                    self.velocity.z + d[2] / length * power * 0.1,
                );
            }
        }
        if player.is_none() && server && colliding {
            // `followingPlayer == null && !level().isClientSide() && colliding`: an orb that would still
            // be stuck at its next position gets
            // `Entity.moveTowardsClosestSpace`, a nudge towards the nearest
            // free side of its block.
            if !crate::collisions::no_collision_empty(&mut scene, bounds.move_by(self.velocity)) {
                let speed = next_float() * 0.2 + 0.1;
                self.move_towards_closest_space(world, facts, speed);
            }
        }
        // `double fallSpeed = getDeltaMovement().y`, taken after the stuck
        // push and before the move.
        let wanted = self.velocity;
        // Entity.collide: the world border's walls and the entities that
        // `canBeCollidedWith` it (a boat, a shulker) stop the orb too.
        let border = border.map(|b| (b, [self.position.x, self.position.z]));
        let moved = scene
            .resolve_in_border_with(bounds, wanted, 0.0, self.on_ground, border, entities)
            .unwrap_or(Vec3::ZERO);
        self.position = Vec3::new(
            self.position.x + moved.x,
            self.position.y + moved.y,
            self.position.z + moved.z,
        );
        self.on_ground = wanted.y < 0.0 && moved.y != wanted.y;
        // Entity.move's restituteMovementAfterCollisions, an orb's bounciness
        // 0: each collided axis's velocity is zeroed — x and z by Mth.equal's
        // 1.0E-5, y exactly.
        if (wanted.x - moved.x).abs() >= 1.0e-5 {
            self.velocity.x = 0.0;
        }
        if (wanted.z - moved.z).abs() >= 1.0e-5 {
            self.velocity.z = 0.0;
        }
        if moved.y != wanted.y {
            self.velocity.y = 0.0;
        }
        // ExperienceOrb.tick applies block effects after moving, before drag.
        let discarded =
            server && self.apply_lava_contact_damage(world, fluids, wanted, next_float, on_sound);
        // `float friction = getAirDrag()` (0.98F), the ground's block
        // friction multiplied in as a float, widened once for the scale.
        let mut friction = 0.98_f32;
        if self.on_ground {
            let below = world.block(
                self.position.x.floor() as i32,
                (self.position.y - 0.999_999).floor() as i32,
                self.position.z.floor() as i32,
            );
            friction *= facts.get(below).friction;
        }
        let friction = f64::from(friction);
        // The drag scales every axis by the same friction, the ground's
        // block friction included.
        self.velocity = Vec3::new(
            self.velocity.x * friction,
            self.velocity.y * friction,
            self.velocity.z * friction,
        );
        // `if (verticalCollisionBelow && fallSpeed < -getGravity())`: the
        // landing bounce is 0.4 of the pre-move speed, not a fixed
        // restitution.
        if self.on_ground && wanted.y < -0.03 {
            self.velocity.y = -wanted.y * 0.4;
        }
        self.age += 1;
        !discarded && self.age < 6000
    }
    /// `LavaFluid.entityInside` -> `Entity.lavaHurt`: four damage on the
    /// server, through the orb's existing hurt/discard policy, playing the generic
    /// burn sound (`minecraft:entity.generic.burn`, volume 0.4, pitch 2.0 + random * 0.4)
    /// on each hurt contact. This is separate from the lava impulse at the start of the tick.
    pub fn apply_lava_contact_damage(
        &mut self,
        world: &World,
        fluids: &crate::motion::FluidResources,
        wanted: Vec3,
        next_float: &mut dyn FnMut() -> f32,
        on_sound: &mut dyn FnMut([f64; 3], f32),
    ) -> bool {
        if self.health <= 0 {
            return true;
        }
        let contacts = orb_lava_contacts(self.previous, self.position, wanted, |p| {
            let fluid = fluids.fluid(world.block(p.x, p.y, p.z));
            if !fluid
                .kind()
                .same_family(horizon_world::fluid::FluidKind::Lava)
            {
                return None;
            }
            let above = fluids.fluid(world.block(p.x, p.y + 1, p.z));
            Some(if fluid.kind().same_family(above.kind()) {
                1.0
            } else {
                f64::from(fluid.own_height())
            })
        });
        for _ in 0..contacts {
            let dead = self.hurt(4.0);
            let pitch = 2.0 + next_float() * 0.4;
            on_sound([self.position.x, self.position.y, self.position.z], pitch);
            if dead {
                return true;
            }
        }
        false
    }
    /// `Entity.moveTowardsClosestSpace` from the middle of the orb's box:
    /// towards the nearest side of its block (north, south, west, east or
    /// up) without a full collision cube, at `speed` (the orb's own
    /// random, `nextFloat() * 0.2 + 0.1`).
    fn move_towards_closest_space(&mut self, world: &World, facts: &BlockFacts, speed: f32) {
        self.velocity = towards_closest_space(
            self.position,
            self.bounds(),
            self.velocity,
            world,
            facts,
            speed,
        );
    }
}

/// `Entity.moveTowardsClosestSpace` from the middle of an entity's box
/// (`bounds`, its feet at `position`): the velocity pushed towards the
/// nearest side of its block (north, south, west, east or up) without a full
/// collision cube, at `speed` (`nextFloat() * 0.2 + 0.1`). Items and
/// experience orbs share it.
fn towards_closest_space(
    position: Vec3,
    bounds: Aabb,
    velocity: Vec3,
    world: &World,
    facts: &BlockFacts,
    speed: f32,
) -> Vec3 {
    let at = Vec3::new(
        position.x,
        (bounds.min_position().y + bounds.max_position().y) / 2.0,
        position.z,
    );
    let pos = [
        at.x.floor() as i32,
        at.y.floor() as i32,
        at.z.floor() as i32,
    ];
    let delta = [
        at.x - f64::from(pos[0]),
        at.y - f64::from(pos[1]),
        at.z - f64::from(pos[2]),
    ];
    // (axis, step): NORTH, SOUTH, WEST, EAST, UP.
    let mut closest = (1, 1);
    let mut nearest = f64::MAX;
    for (axis, step) in [(2, -1), (2, 1), (0, -1), (0, 1), (1, 1)] {
        let mut n = pos;
        n[axis] += step;
        if !facts.get(world.block(n[0], n[1], n[2])).full_collision {
            let d = if step > 0 {
                1.0 - delta[axis]
            } else {
                delta[axis]
            };
            if d < nearest {
                nearest = d;
                closest = (axis, step);
            }
        }
    }
    let push = f64::from(closest.1 as f32 * speed);
    let v = velocity.scale(0.75);
    match closest.0 {
        0 => Vec3::new(push, v.y, v.z),
        1 => Vec3::new(v.x, push, v.z),
        _ => Vec3::new(v.x, v.y, push),
    }
}
/// Player experience (Player.giveExperiencePoints / getXpNeededForNextLevel).
#[derive(Clone, Copy, Debug, Default)]
pub struct Experience {
    pub level: i32,
    pub progress: f32,
    pub total: i32,
}
impl Experience {
    /// `Player.getBaseExperienceReward`: keep-inventory and spectators drop none.
    pub fn death_reward(&self, keep: bool) -> u32 {
        if keep {
            0
        } else {
            self.level.wrapping_mul(7).clamp(0, 100) as u32
        }
    }
    /// `ServerPlayer.restoreFrom`: only keep-inventory and spectators retain XP.
    pub fn respawn(&mut self, keep: bool) {
        if !keep {
            *self = Self::default();
        }
    }
    pub fn needed(level: i32) -> i32 {
        if level >= 30 {
            112 + (level - 30) * 9
        } else if level >= 15 {
            37 + (level - 15) * 5
        } else {
            7 + level * 2
        }
    }
    /// Returns the first multiple of 5 the player reached, where each level
    /// gained would play the level-up sound (`Player.giveExperienceLevels`).
    pub fn give(&mut self, points: i32) -> Option<i32> {
        let mut sound_level = None;
        self.progress += points as f32 / Self::needed(self.level) as f32;
        self.total = self.total.saturating_add(points).max(0);
        while self.progress >= 1.0 {
            self.progress = (self.progress - 1.0) * Self::needed(self.level) as f32;
            self.level += 1;
            if self.level % 5 == 0 {
                sound_level.get_or_insert(self.level);
            }
            self.progress /= Self::needed(self.level) as f32;
        }
        sound_level
    }
}

/// A player's `Entity.tickCount` and `Player.lastLevelUpTime`. This state belongs
/// to the player entity, not the level or the server's clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct LevelUpSound {
    ticks: i32,
    last: i32,
}

impl LevelUpSound {
    /// `Entity.commonTick`: called once for this player, including spectators.
    pub fn tick(&mut self) {
        self.ticks = self.ticks.wrapping_add(1);
    }

    /// `Player.giveExperienceLevels`: return the sound's volume and start its
    /// cooldown when a positive change lands on a fifth level.
    pub fn give_levels(&mut self, amount: i32, level: i32) -> Option<f32> {
        if amount > 0 && level % 5 == 0 && (self.last as f32) < self.ticks as f32 - 100.0 {
            self.last = self.ticks;
            Some((if level > 30 { 1.0 } else { level as f32 / 30.0 }) * 0.75)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod level_up_sound {
    use super::LevelUpSound;

    #[test]
    fn players_have_independent_ages_and_cooldowns() {
        let mut a = LevelUpSound::default();
        let mut b = LevelUpSound::default();
        for _ in 0..100 {
            a.tick();
            b.tick();
        }
        assert_eq!(a.give_levels(1, 5), None);
        a.tick();
        b.tick();
        assert_eq!(a.give_levels(1, 5), Some(0.125));
        assert_eq!(b.give_levels(1, 10), Some(0.25));
        assert_eq!(a.give_levels(1, 10), None);
        for _ in 0..100 {
            a.tick();
        }
        assert_eq!(a.give_levels(1, 30), None);
        a.tick();
        assert_eq!(a.give_levels(1, 30), Some(0.75));
        let mut respawned = LevelUpSound::default();
        assert_eq!(respawned.give_levels(1, 35), None);
        for _ in 0..101 {
            respawned.tick();
        }
        assert_eq!(respawned.give_levels(-1, 35), None);
        assert_eq!(respawned.give_levels(1, 34), None);
        assert_eq!(respawned.give_levels(1, 35), Some(0.75));
    }
}

/// A painting (`Painting`, a `HangingEntity`): the block it hangs in, the
/// 2D data value of the direction it faces, and its variant.
#[derive(Clone, Debug)]
pub struct Painting {
    pub id: u32,
    pub pos: [i32; 3],
    /// Direction.get2DDataValue: south 0, west 1, north 2, east 3.
    pub facing: u8,
    pub variant: std::sync::Arc<str>,
    pub width: i32,
    pub height: i32,
    /// BlockAttachedEntity.checkInterval.
    pub check_interval: u32,
}
impl Painting {
    pub fn step(&self) -> [i32; 3] {
        [[0, 0, 1], [-1, 0, 0], [0, 0, -1], [1, 0, 0]][usize::from(self.facing & 3)]
    }
    /// Direction.getCounterClockWise (the painting's left as it faces out).
    pub fn left(&self) -> [i32; 3] {
        [[1, 0, 0], [0, 0, 1], [-1, 0, 0], [0, 0, -1]][usize::from(self.facing & 3)]
    }
    /// Painting.calculateBoundingBox: (min, max).
    pub fn bounds(&self) -> ([f64; 3], [f64; 3]) {
        let s = self.step().map(f64::from);
        let l = self.left().map(f64::from);
        let off = |n: i32| if n % 2 == 0 { 0.5 } else { 0.0 };
        let (h, v) = (off(self.width), off(self.height));
        let c = [
            f64::from(self.pos[0]) + 0.5 - s[0] * 0.46875 + l[0] * h,
            f64::from(self.pos[1]) + 0.5 + v,
            f64::from(self.pos[2]) + 0.5 - s[2] * 0.46875 + l[2] * h,
        ];
        let along_x = s[0] != 0.0;
        let size = [
            if along_x {
                0.0625
            } else {
                f64::from(self.width)
            },
            f64::from(self.height),
            if along_x {
                f64::from(self.width)
            } else {
                0.0625
            },
        ];
        (
            std::array::from_fn(|i| c[i] - size[i] / 2.0),
            std::array::from_fn(|i| c[i] + size[i] / 2.0),
        )
    }
}
/// The `DyeColor` ordinal of a colour name (`DyeColor.byName`), white when
/// the name is unknown.
pub fn dye_index(name: &str) -> u8 {
    crate::mobs::render::DYE_NAMES
        .iter()
        .position(|n| *n == name)
        .unwrap_or(0) as u8
}
/// A cushion (`Cushion`, a `BlockAttachedEntity`): a 1 x 0.25 block pillow
/// that seats a player. It sits on the top face of a block, at the clicked
/// height, and is coloured by its `DyeColor`.
#[derive(Clone, Debug)]
pub struct Cushion {
    pub id: u32,
    /// `Entity.position`: the bottom centre of the cushion.
    pub position: Vec3,
    /// `BlockAttachedEntity.pos` (the saved `block_pos`).
    pub pos: [i32; 3],
    pub yaw: f32,
    /// `DyeColor` ordinal: white 0, orange 1, ..., black 15.
    pub color: u8,
    /// BlockAttachedEntity.ticksSinceLastCheck.
    pub check_interval: u32,
    /// `Entity.getCustomName` (`CustomName`), copied onto the dropped cushion.
    pub custom_name: Option<horizon_world::nbt::Tag>,
}
impl Cushion {
    /// `Entity.makeBoundingBox`: x and z half a block out, y to y + 0.25.
    pub fn bounds(&self) -> ([f64; 3], [f64; 3]) {
        let p = self.position;
        (
            [p.x - 0.5, p.y, p.z - 0.5],
            [p.x + 0.5, p.y + 0.25, p.z + 0.5],
        )
    }
    /// The `DyeColor` name of the cushion.
    pub fn color_name(&self) -> &'static str {
        crate::mobs::render::DYE_NAMES[usize::from(self.color.min(15))]
    }
    /// `Cushion.getCushionItemStackWithData`: the matching coloured item.
    pub fn item(&self) -> String {
        format!("minecraft:{}_cushion", self.color_name())
    }
    /// `Cushion.getCushionItemStackWithData`: the coloured item with
    /// `DataComponents.CUSTOM_NAME` from `getCustomName`, as the item frame
    /// beside it does.
    pub fn item_stack_with_data(&self) -> ItemStack {
        let mut stack = ItemStack::new(&self.item(), 1);
        if let Some(name) = &self.custom_name {
            stack.data_mut().custom_name = Some(name.clone());
        }
        stack
    }
    /// `Cushion.interact`'s seat: the vehicle's PASSENGER attachment
    /// `(0, 0.25, 0)` less the player's `(0, 0.6, 0)`.
    pub fn seat(&self) -> Vec3 {
        Vec3::new(self.position.x, self.position.y - 0.35, self.position.z)
    }
    /// `Entity.getDismountLocationForPassenger`: on top of the cushion.
    pub fn dismount(&self) -> Vec3 {
        Vec3::new(self.position.x, self.position.y + 0.25, self.position.z)
    }
}
/// An item frame or glow item frame (`ItemFrame`).
#[derive(Clone, Debug)]
pub struct ItemFrame {
    pub id: u32,
    pub pos: [i32; 3],
    /// Direction.get3DDataValue: down 0, up 1, north 2, south 3, west 4, east 5.
    pub facing: u8,
    pub glow: bool,
    pub item: Option<crate::items::ItemStack>,
    /// Java `%` remainder of ItemRotation, so -7..=7 in 45 degree steps.
    pub rotation: i8,
    pub drop_chance: f32,
    pub fixed: bool,
    pub invisible: bool,
    pub check_interval: u32,
    /// `Entity.getCustomName` (`CustomName`), copied onto the dropped frame.
    pub custom_name: Option<horizon_world::nbt::Tag>,
    pub random: LegacyRandom,
}
impl ItemFrame {
    pub fn step(&self) -> [i32; 3] {
        [
            [0, -1, 0],
            [0, 1, 0],
            [0, 0, -1],
            [0, 0, 1],
            [-1, 0, 0],
            [1, 0, 0],
        ][usize::from(self.facing.min(5))]
    }
    pub fn kind(&self) -> &'static str {
        if self.glow {
            "minecraft:glow_item_frame"
        } else {
            "minecraft:item_frame"
        }
    }
    /// ItemFrame.createBoundingBox (without a framed map): (min, max).
    pub fn bounds(&self) -> ([f64; 3], [f64; 3]) {
        let s = self.step().map(f64::from);
        let c: [f64; 3] = std::array::from_fn(|i| f64::from(self.pos[i]) + 0.5 - s[i] * 0.46875);
        let size: [f64; 3] = std::array::from_fn(|i| if s[i] != 0.0 { 0.0625 } else { 0.75 });
        (
            std::array::from_fn(|i| c[i] - size[i] / 2.0),
            std::array::from_fn(|i| c[i] + size[i] / 2.0),
        )
    }
    /// `ItemFrame.getFrameItemStackWithData`: `getFrameItemStack` (a glow
    /// frame's own item), then `DataComponents.CUSTOM_NAME` from
    /// `getCustomName`.
    pub fn item_stack_with_data(&self) -> ItemStack {
        let mut item_stack = ItemStack::new(self.kind(), 1);
        if let Some(name) = &self.custom_name {
            item_stack.data_mut().custom_name = Some(name.clone());
        }
        item_stack
    }
}
/// An armor stand (`ArmorStand`).
#[derive(Clone, Debug)]
pub struct ArmorStand {
    /// `Entity.firstTick`: command equipment events wait for an actual entity tick.
    pub first_tick: bool,
    /// `Entity.xRot`: kept for `/tp` and `Rotation`; the kind turns only in yaw.
    pub pitch: f32,
    /// When it joined its list ([`crate::join_seq`]); set by the list, not by the spawn site.
    pub join: crate::join_seq::JoinSeq,
    pub fire_ticks: i32,
    pub health: f32,
    pub id: u32,
    pub position: Vec3,
    /// `Entity.xo`, `yo`, `zo`: the position at the start of the tick, which
    /// the renderer blends toward `position` by the partial tick.
    pub previous: Vec3,
    pub velocity: Vec3,
    pub on_ground: bool,
    /// `yRot`, which the body follows (`ArmorStand.tickHeadTurn`: `yBodyRot`).
    pub yaw: f32,
    /// `yBodyRotO`: the body's yaw at the start of the tick.
    pub previous_yaw: f32,
    /// mainhand, offhand, feet, legs, chest, head (EquipmentSlot order).
    pub equipment: [Option<crate::items::ItemStack>; 6],
    pub small: bool,
    pub show_arms: bool,
    pub no_base_plate: bool,
    pub invisible: bool,
    pub marker: bool,
    /// `Entity.isNoGravity` (`NoGravity` in the save). `ArmorStand.hasPhysics`
    /// is false with it, as with `Marker`: `travel` does nothing, so the stand
    /// never moves, and the blocks it stands in do not touch it.
    pub no_gravity: bool,
    /// `CustomName`: the text of a hologram.
    pub custom_name: Option<crate::text::Text>,
    /// `CustomNameVisible`.
    pub name_visible: bool,
    /// Game time of the last hit (the wobble and the second-hit window).
    pub last_hit: i64,
    /// `DATA_HEAD_POSE` to `DATA_RIGHT_LEG_POSE`: head, body, left arm, right
    /// arm, left leg, right leg, in degrees (`POSE_KEYS` order).
    pub pose: [[f32; 3]; 6],
    /// `Entity.vehicle` on a server's client (`SetPassengers`): the mob it
    /// rides, which seats it every tick (`positionRider`), as NPC plugins hang
    /// a second name over a player's head.
    pub vehicle: Option<u32>,
    /// `ArmorStand.disabledSlots`: bit `filterFlag` locks a slot, bit
    /// `filterFlag + 8` takes from it and bit `filterFlag + 16` puts into it.
    pub disabled_slots: i32,
    /// Server-only `LivingEntity.lastPos` for location-based enchantments.
    pub enchantment_block: Option<BlockPos>,
    /// `Entity.fallDistance`, also used by `LivingEntity.checkFallDamage`'s landing callback.
    /// Each downward movement is narrowed to float before adding to this double.
    pub enchantment_fall_distance: f64,
    /// Last equipment seen by `collectEquipmentChanges`; not saved to NBT.
    pub enchantment_equipment: [Option<ItemStack>; 6],
    /// `Entity.mainSupportingBlockPos` after the last move.
    pub support: horizon_world::ground_support::GroundState,
    /// Its `AttributeMap` holds a `movement_speed` instance, which
    /// `AttributeMap.pack` saves. `LivingEntity.aiStep`'s `removeFrost` makes it
    /// (`getAttribute`) on the stand's first server tick, and a save that lists
    /// it loads it back, so a stand summoned this tick has none yet.
    pub speed_instance: bool,
}
/// The fluid a stand travels in (`LivingEntity.travelInFluid`: water first,
/// then lava), from the heights `Entity::push_by_fluid` measured.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum StandMedium {
    Air,
    Water,
    /// `shallow`: `isInShallowFluid(LAVA)`, the lava is no deeper than
    /// `getFluidJumpThreshold` (0.4 for a stand, whose eyes are above 0.4).
    Lava {
        shallow: bool,
    },
}
impl StandMedium {
    pub fn from_heights(water_height: f64, lava_height: f64) -> Self {
        if water_height > 0.0 {
            Self::Water
        } else if lava_height > 0.0 {
            Self::Lava {
                shallow: lava_height <= 0.4,
            }
        } else {
            Self::Air
        }
    }
}
/// Armor stands share LivingEntity's block/tag and trapdoor climb predicate.
struct StandClimbing<'a> {
    world: &'a World,
    position: BlockPos,
}
impl horizon_world::climbing::ClimbingWorld for StandClimbing<'_> {
    type Error = std::convert::Infallible;
    fn is_spectator(&mut self) -> Result<bool, Self::Error> {
        Ok(false)
    }
    fn is_fall_flying(&mut self) -> Result<bool, Self::Error> {
        Ok(false)
    }
    fn block_position(&mut self) -> Result<BlockPos, Self::Error> {
        Ok(self.position)
    }
    fn in_block_state(&mut self) -> Result<horizon_world::registry::BlockStateId, Self::Error> {
        self.block_state(self.position)
    }
    fn block_state(
        &mut self,
        p: BlockPos,
    ) -> Result<horizon_world::registry::BlockStateId, Self::Error> {
        Ok(self.world.block(p.x, p.y, p.z))
    }
    fn set_last_climbable_position(&mut self, _: BlockPos) {}
}
/// What a stand's travel carries from [`ArmorStand::tick_move`] to
/// [`ArmorStand::tick_settle`].
#[derive(Clone, Copy)]
pub struct StandStep {
    falling: bool,
    friction: f32,
}
#[cfg(test)]
impl ArmorStand {
    /// A whole travel for the unit tests, with no bubble column.
    pub(crate) fn tick(
        &mut self,
        scene: &mut crate::collisions::SceneCollisions,
        facts: &BlockFacts,
        climbing: &horizon_world::climbing::ClimbingRegistry<'_>,
        in_water: bool,
        border: Option<&BorderGeometry>,
    ) {
        if let Some(step) = self.tick_move(scene, facts, climbing, in_water, border, &[]) {
            let medium = if in_water {
                StandMedium::Water
            } else {
                StandMedium::Air
            };
            self.tick_settle(medium, step);
        }
        self.apply_honey_slide(scene.world, facts);
    }
}
impl ArmorStand {
    /// `NameTagItem.interactLivingEntity`: a custom name handles the use;
    /// only a living target changes. Stack consumption belongs to the caller.
    pub fn name_with_tag(&mut self, stack: &ItemStack) -> bool {
        if &*stack.item != "minecraft:name_tag" {
            return false;
        }
        let Some(name) = stack.data().and_then(|data| data.custom_name.as_ref()) else {
            return false;
        };
        if self.health > 0.0 {
            self.custom_name = Some(crate::component::name_text(name));
        }
        true
    }
    pub const SLOTS: [&'static str; 6] = ["mainhand", "offhand", "feet", "legs", "chest", "head"];
    /// The `Pose` compound's keys.
    pub const POSE_KEYS: [&'static str; 6] =
        ["Head", "Body", "LeftArm", "RightArm", "LeftLeg", "RightLeg"];
    /// `ArmorStand.DEFAULT_*_POSE`.
    pub const DEFAULT_POSE: [[f32; 3]; 6] = [
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0],
        [-10.0, 0.0, -10.0],
        [-15.0, 0.0, 10.0],
        [-1.0, 0.0, -1.0],
        [1.0, 0.0, 1.0],
    ];
    /// Where the renderer draws the stand `alpha` of the way through the tick
    /// (`EntityRenderer`'s `Mth.lerp(partialTick, xo, x)` and so on).
    pub fn render_position(&self, alpha: f64) -> Vec3 {
        self.previous.lerp(self.position, alpha)
    }
    /// `ArmorStand.getDimensionsMarker(false)`: the width, height and eye
    /// height of the box a stand has without the marker flag, which is the box
    /// a marker's light probe walks (`MARKER_DIMENSIONS` itself is empty, and
    /// `isBaby()` is `isSmall()`).
    pub fn probe_dimensions(&self) -> (f64, f64, f64) {
        if self.small {
            (0.25, 0.9875, 0.9875)
        } else {
            (0.5, 1.975, 1.7775)
        }
    }
    /// `ArmorStand.getLightProbePosition` as the cell the renderer reads
    /// (`BlockPos.containing` of the position it returns): a marker walks
    /// `BlockPos.betweenClosed` of its non-marker box at `position()` - it
    /// takes no partial tick - and returns the first cell whose brightness
    /// `max(block, sky)` is 15, else the brightest, ties going to the first
    /// cell walked; every other stand is lit from its eye
    /// (`Entity.getLightProbePosition` is `getEyePosition(partialTick)`).
    /// `light` is the level's packed light.
    pub fn light_probe(
        &self,
        partial: f64,
        mut light: impl FnMut(i32, i32, i32) -> u8,
    ) -> [i32; 3] {
        let (width, height, eye) = self.probe_dimensions();
        if !self.marker {
            let at = self.render_position(partial);
            return [
                at.x.floor() as i32,
                (at.y + eye).floor() as i32,
                at.z.floor() as i32,
            ];
        }
        let half = width / 2.0;
        let (lo_x, hi_x) = (
            (self.position.x - half).floor() as i32,
            (self.position.x + half).floor() as i32,
        );
        let (lo_z, hi_z) = (
            (self.position.z - half).floor() as i32,
            (self.position.z + half).floor() as i32,
        );
        let (lo_y, hi_y) = (
            self.position.y.floor() as i32,
            (self.position.y + height).floor() as i32,
        );
        // `BlockPos.betweenClosed` walks x fastest, then y, then z; the stand's
        // own cell is what it falls back to when the box is empty.
        let mut best = [
            self.position.x.floor() as i32,
            self.position.y.floor() as i32,
            self.position.z.floor() as i32,
        ];
        let mut brightest = i32::MIN;
        for cell_z in lo_z..=hi_z {
            for cell_y in lo_y..=hi_y {
                for cell_x in lo_x..=hi_x {
                    let level = light(cell_x, cell_y, cell_z);
                    let brightness = i32::from((level & 15).max(level >> 4));
                    if brightness == 15 {
                        return [cell_x, cell_y, cell_z];
                    }
                    if brightness > brightest {
                        brightest = brightness;
                        best = [cell_x, cell_y, cell_z];
                    }
                }
            }
        }
        best
    }
    /// `EntityRenderer.getPackedLightCoords` for the cell `light_probe` chose:
    /// the level's sky light over the cell's block light, at 15 block light
    /// while the stand `isOnFire`. `light` is the level's packed light.
    pub fn render_light(&self, light: u8) -> u32 {
        let block = if self.fire_ticks > 0 { 15 } else { light & 15 };
        (u32::from(light >> 4) * 16) | ((u32::from(block) * 16) << 8)
    }
    /// The body's yaw `alpha` of the way through the tick
    /// (`LivingEntityRenderer.solveBodyRot`: `Mth.rotLerp(partialTick,
    /// yBodyRotO, yBodyRot)`).
    pub fn render_yaw(&self, alpha: f32) -> f32 {
        self.previous_yaw + alpha * crate::camera::wrap_degrees(self.yaw - self.previous_yaw)
    }
    /// A server's stand on its client, each tick:
    /// `setOldPosAndRot` (`ClientLevel.tickNonPassenger`), then the step of
    /// its `SteppedInterpolationHandler` (`step`: the position and yaw it is
    /// put at, when one is due). The yaw
    /// turns the short way and stays continuous, so a frame between ticks
    /// never spins it.
    pub fn interpolate(&mut self, step: Option<(Vec3, f32)>) {
        self.previous = self.position;
        self.previous_yaw = self.yaw;
        if let Some((at, yaw)) = step {
            self.position = at;
            self.yaw += crate::camera::wrap_degrees(yaw - self.yaw);
        }
    }
    /// Put there at once, with nothing to blend from
    /// (`Entity.snapTo`).
    pub fn snap(&mut self, at: Vec3, yaw: f32) {
        self.position = at;
        self.previous = at;
        self.yaw = yaw;
        self.previous_yaw = yaw;
    }
    pub fn new(id: u32, position: Vec3, yaw: f32) -> Self {
        Self {
            first_tick: true,
            pitch: 0.0,
            join: Default::default(),
            fire_ticks: 0,
            health: 20.0,
            id,
            position,
            previous: position,
            velocity: Vec3::ZERO,
            on_ground: false,
            yaw,
            previous_yaw: yaw,
            equipment: Default::default(),
            small: false,
            show_arms: false,
            no_base_plate: false,
            invisible: false,
            marker: false,
            no_gravity: false,
            custom_name: None,
            name_visible: false,
            last_hit: i64::MIN / 2,
            pose: Self::DEFAULT_POSE,
            vehicle: None,
            disabled_slots: 0,
            enchantment_block: None,
            enchantment_fall_distance: 0.0,
            enchantment_equipment: Default::default(),
            support: Default::default(),
            speed_instance: false,
        }
    }
    /// `Entity.getOnPos(0.2)` (`getOnPosLegacy`): the supporting block's
    /// column 0.2 below the feet, the supporting block itself when `tall`
    /// says it is a fence, wall or fence gate, else the cell 0.2 below.
    pub fn on_pos_legacy(&self, tall: impl Fn(BlockPos) -> bool) -> BlockPos {
        let y = (self.position.y - f64::from(0.2_f32)).floor() as i32;
        match self.support.supporting_block {
            Some(p) if tall(p) => p,
            Some(p) => BlockPos::new(p.x, y, p.z),
            None => BlockPos::new(
                self.position.x.floor() as i32,
                y,
                self.position.z.floor() as i32,
            ),
        }
    }
    /// `EquipmentSlot.getFilterFlag` of an `equipment` index (main hand, off
    /// hand, feet, legs, chest, head).
    fn filter_flag(slot: usize) -> u32 {
        [0, 5, 1, 2, 3, 4][slot]
    }
    /// `ArmorStand.isDisabled`: the slot is locked, or it is a hand and the
    /// stand has no arms.
    pub fn is_disabled(&self, slot: usize) -> bool {
        self.disabled_slots & (1 << Self::filter_flag(slot)) != 0 || (slot < 2 && !self.show_arms)
    }
    /// The `swapItem` locks: taking from a filled slot or putting into an
    /// empty one can each be disabled.
    pub fn swap_locked(&self, slot: usize) -> bool {
        let offset = if self.equipment[slot].is_some() {
            8
        } else {
            16
        };
        self.disabled_slots & (1 << (Self::filter_flag(slot) + offset)) != 0
    }
    pub fn environment_tick(
        &mut self,
        world: &World,
        facts: &BlockFacts,
        registry: &horizon_world::registry::BlockRegistry,
    ) -> bool {
        let (lo, hi) = (self.bounds().min_position(), self.bounds().max_position());
        let mut water = false;
        let mut lava = false;
        for x in lo.x.floor() as i32..=hi.x.floor() as i32 {
            for y in lo.y.floor() as i32..hi.y.ceil() as i32 {
                for z in lo.z.floor() as i32..=hi.z.floor() as i32 {
                    match facts.get(world.block(x, y, z)).fluid {
                        Fluid::Water => water = true,
                        Fluid::Lava => lava = true,
                        _ => {}
                    }
                }
            }
        }
        if water {
            self.fire_ticks = 0;
        }
        if self.fire_ticks > 0 {
            if self.fire_ticks % 20 == 0
                && !lava
                && !self.invisible
                && !self.marker
                && self.health > 0.5
            {
                self.health -= 4.0;
            }
            self.fire_ticks -= 1;
        }
        if lava {
            self.fire_ticks = self.fire_ticks.max(300);
        }
        let fire = crate::motion::environment::fire_contact(world, registry, self.bounds()) > 0.0;
        // The fire block reaches a stand through `applyEffectsFromBlocks`,
        // which a NoGravity stand does not feel (measured beside the server).
        if fire && !self.invisible && !self.marker && !self.no_gravity {
            if self.fire_ticks > 0 {
                self.health -= 0.15;
            } else {
                self.fire_ticks = 100;
            }
        }
        self.health > 0.5
    }
    /// EntityType.ARMOR_STAND dimensions (0.5 x 1.975F, half for small).
    pub fn bounds(&self) -> Aabb {
        let s = if self.small { 0.5 } else { 1.0 };
        // EntityDimensions.makeBoundingBox adds the float height, widened
        // (halving it for the small stand is exact).
        let (w, h) = (0.25 * s, f64::from(1.975_f32) * s);
        let p = self.position;
        Aabb::new(
            Vec3::new(p.x - w, p.y, p.z - w),
            Vec3::new(p.x + w, p.y + h, p.z + w),
        )
    }
    /// Leather boots on its feet: it walks on powder snow (`PowderSnowBlock.canEntityWalkOnPowderSnow`).
    fn feet_are_leather_boots(&self) -> bool {
        self.equipment[2]
            .as_ref()
            .is_some_and(|s| &*s.item == "minecraft:leather_boots")
    }
    /// LivingEntity.travel (armor stands have no other movement): clamp tiny
    /// velocity components (`LivingEntity.aiStep`), move by the velocity (a
    /// landing stops the fall, `Block.updateEntityMovementAfterFallOn`, and a
    /// wall collision zeros that horizontal axis), let the blocks it is inside
    /// push it (`push`: a bubble column), then in water the 0.8 slowdown and
    /// `getFluidFallingAdjustedMovement`, in air gravity 0.08, ground/air
    /// horizontal drag (`friction * 0.91`), and the 0.98 vertical drag, so a
    /// stand at rest on the ground keeps -0.0784 as vanilla's.
    ///
    /// The game runs the fluid push first (`Entity::push_by_fluid`), then
    /// this up to the blocks' push, then the bubble column
    /// (`Entity::push_by_bubble_column`), then [`ArmorStand::tick_settle`].
    /// `entities` are the boxes `EntityGetter.getEntityCollisions` gives the
    /// stand (a living shulker, a boat, a happy ghast held still), which stop
    /// its move as blocks do. None: a marker or a stand with `NoGravity` does
    /// not travel.
    pub fn tick_move(
        &mut self,
        scene: &mut crate::collisions::SceneCollisions,
        facts: &BlockFacts,
        climbing: &horizon_world::climbing::ClimbingRegistry<'_>,
        in_water: bool,
        border: Option<&BorderGeometry>,
        entities: &[Aabb],
    ) -> Option<StandStep> {
        use crate::entity_rules::EntityRules;
        self.previous = self.position;
        if self.marker {
            return None;
        }
        scene.subject =
            self.feet_are_leather_boots()
                .then_some(crate::collisions::PowderSnowSubject {
                    leather_boots: true,
                    ..Default::default()
                });
        let v = self.velocity;
        let mut wanted = Vec3::new(
            if v.x.abs() < 0.003 { 0.0 } else { v.x },
            if v.y.abs() < 0.003 { 0.0 } else { v.y },
            if v.z.abs() < 0.003 { 0.0 } else { v.z },
        );
        self.velocity = wanted;
        // ArmorStand.travel: no physics with NoGravity, the velocity stays.
        if self.is_no_gravity() {
            return None;
        }
        // `travelInFluid` reads `isFalling` before the move and any push;
        // `travelInAir` reads ground friction before the move.
        let falling = wanted.y <= 0.0;
        let friction = if !in_water && self.on_ground {
            let below = scene.world.block(
                self.position.x.floor() as i32,
                (self.position.y - 0.500_001).floor() as i32,
                self.position.z.floor() as i32,
            );
            facts.get(below).friction
        } else {
            1.0
        };
        let on_climbable = |position| {
            horizon_world::climbing::on_climbable(
                &mut StandClimbing {
                    world: scene.world,
                    position: BlockPos::containing(position),
                },
                climbing,
            )
            .unwrap_or(false)
        };
        if !in_water && on_climbable(self.position) {
            self.enchantment_fall_distance = 0.0;
            // LivingEntity.handleOnClimbable; suppressing slide is Player-only.
            let limit = f64::from(0.15f32);
            wanted.x = wanted.x.clamp(-limit, limit);
            wanted.z = wanted.z.clamp(-limit, limit);
            if !wanted.y.is_nan() {
                wanted.y = wanted.y.max(-limit);
            }
        }
        let mut velocity = wanted;
        let mut horizontal_collision = false;
        // Entity.collide: the world border's walls and the entities that
        // `canBeCollidedWith` the stand stop it too.
        let moved = SceneCollisions {
            world: scene.world,
            binding: scene.binding,
            subject: scene.subject,
        }
        .resolve_in_border_with(
            self.bounds(),
            wanted,
            0.0,
            self.on_ground,
            border.map(|b| (b, [self.position.x, self.position.z])),
            entities,
        );
        if let Ok(moved) = moved {
            self.position = Vec3::new(
                self.position.x + moved.x,
                self.position.y + moved.y,
                self.position.z + moved.z,
            );
            self.on_ground = wanted.y < 0.0 && moved.y > wanted.y;
            // Entity.setOnGroundWithMovement: the block under the feet.
            self.ground_support(scene, wanted);
            if (wanted.x - moved.x).abs() >= f64::from(1.0e-5_f32) {
                horizontal_collision = true;
                velocity.x = 0.0;
            }
            if moved.y != wanted.y {
                velocity.y = 0.0;
            }
            if (wanted.z - moved.z).abs() >= f64::from(1.0e-5_f32) {
                horizontal_collision = true;
                velocity.z = 0.0;
            }
        }
        if !in_water && horizontal_collision && on_climbable(self.position) {
            velocity.y = 0.2;
        }
        self.velocity = velocity;
        Some(StandStep { falling, friction })
    }
    /// `Entity.move(MoverType.SHULKER, delta)`: an opening shulker lid
    /// shoves the stand (one with physics) by `delta`. Blocks, the world
    /// border and `entities` (the boxes `getEntityCollisions` gives the stand)
    /// cut the move short, a move cut to almost nothing is not made, and an
    /// axis that hits something loses its speed
    /// (`restituteMovementAfterCollisions`: a stand does not bounce, so the
    /// speed becomes `-speed * 0`).
    ///
    /// The stand has already travelled this tick (stands tick before mobs
    /// here). `travels_after` says vanilla ticks it after the shulker (it
    /// joined the level later, as a stand placed on the shell does), so
    /// there the push comes first and its own travel follows:
    /// - a push nothing stops leaves `on_ground` and the speed as that
    ///   travel left them (it lands the stand again on the risen lid);
    /// - a push down that something stops: the travel that follows starts
    ///   with no vertical speed, moves nothing vertically (`on_ground`
    ///   false) and ends with gravity's speed only, which the stand's own
    ///   travel, stopped by the same floor, already gave it;
    /// - a push up that a ceiling stops loses the vertical speed, so the
    ///   stand stays pinned on the next tick as vanilla's does (vanilla
    ///   shows gravity's speed by then, and once the lid stops, the stand
    ///   falls through it one tick later than vanilla's).
    ///
    /// Otherwise vanilla ticks it before the shulker, as here, and the push
    /// sets `on_ground` and the block under the feet itself. Left out: the
    /// block speed factor on the horizontal speed, the fall distance a push
    /// down through the air adds, and a slime or bed bounce.
    pub fn shove(
        &mut self,
        scene: SceneCollisions,
        delta: Vec3,
        border: Option<&BorderGeometry>,
        entities: &[Aabb],
        travels_after: bool,
    ) {
        let mut scene = SceneCollisions {
            subject: self.feet_are_leather_boots().then_some(
                crate::collisions::PowderSnowSubject {
                    leather_boots: true,
                    ..Default::default()
                },
            ),
            ..scene
        };
        let Ok(moved) = SceneCollisions {
            world: scene.world,
            binding: scene.binding,
            subject: scene.subject,
        }
        .resolve_in_border_with(
            self.bounds(),
            delta,
            0.0,
            self.on_ground,
            border.map(|b| (b, [self.position.x, self.position.z])),
            entities,
        ) else {
            return;
        };
        let length = moved.length_squared();
        if length > 1.0e-7 || delta.length_squared() - length < 1.0e-7 {
            self.position = self.position.add_vector(moved);
        }
        // Mth.equal on x and z; y only when the push had a vertical part.
        if (delta.x - moved.x).abs() >= f64::from(1.0e-5_f32) {
            self.velocity.x = -self.velocity.x * 0.0;
        }
        if (delta.z - moved.z).abs() >= f64::from(1.0e-5_f32) {
            self.velocity.z = -self.velocity.z * 0.0;
        }
        let vertical = moved.y != delta.y;
        if travels_after {
            if vertical {
                self.on_ground = false;
                if delta.y > 0.0 {
                    self.velocity.y = -self.velocity.y * 0.0;
                }
                self.ground_support(&mut scene, moved);
            }
        } else {
            if vertical && delta.y != 0.0 {
                self.velocity.y = -self.velocity.y * 0.0;
            }
            self.on_ground = vertical && delta.y < 0.0;
            self.ground_support(&mut scene, moved);
        }
    }
    /// `Entity.checkSupportingBlock` after a move by `movement`: the block
    /// under the feet while `on_ground`, none otherwise.
    fn ground_support(&mut self, scene: &mut SceneCollisions, movement: Vec3) {
        let context = horizon_world::collision_context::CollisionContext::for_entity(
            horizon_world::collision_context::CollisionEntity {
                identity: horizon_world::entity_obstruction::EntityIdentity(u64::from(self.id)),
                kind: horizon_world::collision_context::EntityContextKind::Living,
                y: self.position.y,
                descending: false,
                main_hand: Some(horizon_world::collision_context::CollisionItem::EMPTY),
            },
            false,
        );
        let (position, bounds) = (self.position, self.bounds());
        let _ = self
            .support
            .check_support(self.on_ground, bounds, Some(movement), |area| {
                horizon_world::ground_support::find_supporting_block(
                    scene, context, position, area, 1_000_000,
                )
            });
    }
    /// `HoneyBlock.entityInside`: side-slide movement after the stand's travel.
    /// `ArmorStand.hasPhysics` / `Entity.isAffectedByBlocks` exclude markers
    /// and NoGravity stands. Slide effects are separate from this movement.
    pub fn apply_honey_slide(&mut self, world: &World, facts: &BlockFacts) {
        let old_delta_y = |dy: f64| dy / f64::from(0.98_f32) + 0.08;
        if self.marker || self.no_gravity || self.on_ground || old_delta_y(self.velocity.y) >= -0.08
        {
            return;
        }
        // Entity.checkInsideBlocks: test the deflated body against block cells,
        // not their inset collision shape (the stand slides beside that shape).
        let bounds = self.bounds().deflate(Vec3::new(1e-5, 1e-5, 1e-5));
        let (lo, hi) = (bounds.min_position(), bounds.max_position());
        let overlap = 0.4375 + if self.small { 0.125 } else { 0.25 };
        for x in lo.x.floor() as i32..=hi.x.floor() as i32 {
            for y in lo.y.floor() as i32..=hi.y.floor() as i32 {
                for z in lo.z.floor() as i32..=hi.z.floor() as i32 {
                    if &*facts.get(world.block(x, y, z)).name != "minecraft:honey_block"
                        || self.position.y > f64::from(y) + 0.9375 - 1.0e-7
                    {
                        continue;
                    }
                    let dx = (f64::from(x) + 0.5 - self.position.x).abs();
                    let dz = (f64::from(z) + 0.5 - self.position.z).abs();
                    if dx + 1.0e-7 <= overlap && dz + 1.0e-7 <= overlap {
                        continue;
                    }
                    // HoneyBlock.doSlideMovement, including its float-widened
                    // getOldDeltaY/getNewDeltaY conversion around air travel.
                    let old = old_delta_y(self.velocity.y);
                    if old < -0.13 {
                        let factor = -0.05 / old;
                        self.velocity.x *= factor;
                        self.velocity.z *= factor;
                    }
                    self.velocity.y = (-0.05 - 0.08) * f64::from(0.98_f32);
                    self.enchantment_fall_distance = 0.0;
                    // The new oldDeltaY is -0.05, so another honey cell cannot slide again.
                    return;
                }
            }
        }
    }
    /// The rest of the stand's travel: the water slowdown, the lava slowdown
    /// and sink, or the air drag and gravity. The blocks' push (a bubble
    /// column) comes after this, as `LivingEntity.aiStep` applies the block
    /// effects after `travel`.
    pub fn tick_settle(&mut self, medium: StandMedium, step: StandStep) {
        use crate::entity_rules::EntityRules;
        let StandStep { falling, friction } = step;
        let velocity = self.velocity;
        let gravity = self.get_gravity();
        self.velocity = if let StandMedium::Lava { shallow } = medium {
            // travelInLava: halve it (a shallow lava also slows the fall by
            // 0.8 with `getFluidFallingAdjustedMovement`), then sink by
            // gravity / 4.
            let (x, y, z) = if shallow {
                let y = velocity.y * f64::from(0.8f32);
                let sink = gravity / 16.0;
                let adjusted = if falling && (y - 0.005).abs() >= 0.003 && (y - sink).abs() < 0.003
                {
                    -0.003
                } else {
                    y - sink
                };
                (velocity.x * 0.5, adjusted, velocity.z * 0.5)
            } else {
                (velocity.x * 0.5, velocity.y * 0.5, velocity.z * 0.5)
            };
            Vec3::new(x, y - gravity / 4.0, z)
        } else if medium == StandMedium::Water {
            // travelInWater: slowDown 0.8, then `getFluidFallingAdjustedMovement`
            // with gravity / 16 (the stand is falling while its fall is not
            // already nearly stopped).
            let slow = f64::from(0.8f32);
            let y = velocity.y * slow;
            let sink = self.get_gravity() / 16.0;
            let adjusted = if falling && (y - 0.005).abs() >= 0.003 && (y - sink).abs() < 0.003 {
                -0.003
            } else {
                y - sink
            };
            Vec3::new(velocity.x * slow, adjusted, velocity.z * slow)
        } else {
            let drag = f64::from(friction * 0.91f32);
            Vec3::new(
                velocity.x * drag,
                (velocity.y - self.get_gravity()) * f64::from(0.98f32),
                velocity.z * drag,
            )
        };
    }
}

#[cfg(test)]
mod experience_tests;

#[cfg(test)]
mod fire_environment_tests {
    use super::*;
    use horizon_world::loot::PackOverlay;
    #[test]
    fn ordinary_items_burn_but_native_netherite_resists_lava() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let assets = crate::assets::Assets::load(&root).unwrap();
        let render = assets.reload(&root).unwrap();
        let facts = BlockFacts::load(&root, assets.registry).unwrap();
        let items =
            crate::items::Items::load(&root, assets.registry, &facts, &PackOverlay::NONE).unwrap();
        let mut world = World::test_scene(&assets, &render, 3).unwrap();
        let at = Vec3::new(20.5, 4.0, 20.5);
        world
            .set(
                20,
                4,
                20,
                assets.registry.default_state("minecraft:lava").unwrap(),
            )
            .unwrap();
        let make = |id: &str| {
            ItemEntity::new(
                1,
                ItemStack {
                    item: std::sync::Arc::from(id),
                    count: 1,
                    damage: 0,
                    extra: None,
                },
                at,
                Vec3::ZERO,
                0.0,
            )
        };
        let mut wood = make("minecraft:oak_planks");
        let mut metal = make("minecraft:netherite_ingot");
        let first = wood.environment_tick(&world, &facts, assets.registry, &items);
        assert!(first.alive && first.lava_hurt);
        assert!(
            !first.should_play_lava_hurt_sound(1),
            "ordinary entry damage must not repeat the burn sound"
        );
        assert!(first.should_play_lava_hurt_sound(10));
        assert!(
            wood.fire_ticks > 0,
            "lava contact must ignite dropped items"
        );
        let last = wood.environment_tick(&world, &facts, assets.registry, &items);
        assert!(
            !last.alive && last.lava_hurt,
            "lethal lava damage still emits burn sound"
        );
        assert!(
            last.should_play_lava_hurt_sound(2),
            "the lethal hit must always play the sound"
        );
        for _ in 0..40 {
            let result = metal.environment_tick(&world, &facts, assets.registry, &items);
            assert!(result.alive && !result.lava_hurt);
            assert!(!result.should_play_lava_hurt_sound(10));
        }
        assert_eq!(metal.health, 5);
        assert_eq!(metal.fire_ticks, 0, "fire-resistant items must not ignite");
        world
            .set(
                20,
                4,
                20,
                assets.registry.default_state("minecraft:fire").unwrap(),
            )
            .unwrap();
        let mut burning = make("minecraft:oak_planks");
        let result = burning.environment_tick(&world, &facts, assets.registry, &items);
        assert!(result.alive && !result.lava_hurt);
        assert!(burning.fire_ticks > 0);
        world
            .set(
                20,
                4,
                20,
                assets
                    .registry
                    .default_state("minecraft:soul_fire")
                    .unwrap(),
            )
            .unwrap();
        let mut soul = make("minecraft:oak_planks");
        soul.environment_tick(&world, &facts, assets.registry, &items);
        assert_eq!(soul.health, 3);
        let mut stand = ArmorStand::new(2, at, 0.0);
        world
            .set(
                20,
                4,
                20,
                assets.registry.default_state("minecraft:lava").unwrap(),
            )
            .unwrap();
        for _ in 0..150 {
            assert!(stand.environment_tick(&world, &facts, assets.registry));
        }
        assert_eq!(stand.health, 20.0, "lava does not hurt armor stands");
    }
}
#[cfg(test)]
mod destroyed_bundle_environment_tests {
    use super::*;
    use horizon_world::loot::PackOverlay;

    /// `BundleItem.onDestroyed`: the lava that destroys a dropped bundle
    /// hands its stacks to the item tick (`ItemUtils.onContainerDestroyed`),
    /// and the bundle keeps none of them.
    #[test]
    fn lava_that_destroys_a_bundle_spills_its_contents() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let assets = crate::assets::Assets::load(&root).unwrap();
        let render = assets.reload(&root).unwrap();
        let facts = BlockFacts::load(&root, assets.registry).unwrap();
        let items =
            crate::items::Items::load(&root, assets.registry, &facts, &PackOverlay::NONE).unwrap();
        let mut world = World::test_scene(&assets, &render, 3).unwrap();
        let at = Vec3::new(20.5, 4.0, 20.5);
        world
            .set(
                20,
                4,
                20,
                assets.registry.default_state("minecraft:lava").unwrap(),
            )
            .unwrap();
        let mut stack = ItemStack::new("minecraft:bundle", 1);
        stack.data_mut().bundle_contents = vec![
            ItemStack::new("minecraft:stone", 3),
            ItemStack::new("minecraft:diamond", 1),
        ];
        let mut bundle = ItemEntity::new(1, stack, at, Vec3::ZERO, 0.0);
        // 5 health against 4 a tick: the first tick leaves 1 and nothing out.
        let first = bundle.environment_tick(&world, &facts, assets.registry, &items);
        assert!(first.alive && first.spilled.is_empty());
        let second = bundle.environment_tick(&world, &facts, assets.registry, &items);
        assert!(!second.alive);
        assert_eq!(
            second
                .spilled
                .iter()
                .map(|s| (&*s.item, s.count))
                .collect::<Vec<_>>(),
            [("minecraft:stone", 3), ("minecraft:diamond", 1)]
        );
        assert!(
            bundle
                .stack
                .data()
                .is_none_or(|d| d.bundle_contents.is_empty()),
            "the destroyed bundle keeps nothing inside"
        );
    }
}
#[cfg(test)]
mod armor_stand_render_tests {
    use super::*;

    /// A falling stand is drawn between last tick's position and this one's,
    /// so it slides down every frame instead of jumping once a tick (#4416).
    #[test]
    fn a_falling_stand_is_drawn_between_its_ticks() {
        let mut stand = ArmorStand::new(1, Vec3::new(0.5, 70.0, 0.5), 0.0);
        // A new stand has no move behind it.
        assert_eq!(stand.render_position(0.5), stand.position);
        let fall = |s: &ArmorStand| Some((Vec3::new(0.5, s.position.y - 1.0, 0.5), 0.0));
        stand.interpolate(fall(&stand));
        assert_eq!(stand.render_position(0.0).y, 70.0);
        assert_eq!(stand.render_position(0.25).y, 69.75);
        assert_eq!(stand.render_position(1.0).y, 69.0);
        stand.interpolate(fall(&stand));
        assert_eq!(stand.render_position(0.5).y, 68.5);
        // Landed: no step is due, so it stays put, and so does every frame.
        stand.interpolate(None);
        assert_eq!(stand.render_position(0.5).y, 68.0);
    }

    /// A turning stand's body turns between ticks too, the short way round.
    #[test]
    fn a_turning_stand_is_drawn_between_its_ticks() {
        let mut stand = ArmorStand::new(1, Vec3::ZERO, 170.0);
        assert_eq!(stand.render_yaw(0.5), 170.0);
        // To -170 is 20 degrees on through 180, not 340 back through 0.
        stand.interpolate(Some((Vec3::ZERO, -170.0)));
        assert_eq!(stand.render_yaw(0.0), 170.0);
        assert_eq!(stand.render_yaw(0.5), 180.0);
        assert_eq!(stand.render_yaw(1.0), 190.0);
        stand.interpolate(None);
        assert_eq!(stand.render_yaw(0.5), 190.0);
    }

    /// A snap (a move too far to interpolate) leaves nothing to blend from.
    #[test]
    fn a_snapped_stand_is_drawn_where_it_landed() {
        let mut stand = ArmorStand::new(1, Vec3::new(0.5, 70.0, 0.5), 0.0);
        stand.interpolate(Some((Vec3::new(0.5, 69.0, 0.5), 0.0)));
        stand.snap(Vec3::new(200.5, 64.0, 0.5), 90.0);
        assert_eq!(stand.render_position(0.5), Vec3::new(200.5, 64.0, 0.5));
        assert_eq!(stand.render_yaw(0.5), 90.0);
    }
}
#[cfg(test)]
mod armor_stand_light_tests {
    use super::*;

    fn stand(x: f64, y: f64, z: f64) -> ArmorStand {
        ArmorStand::new(1, Vec3::new(x, y, z), 0.0)
    }

    /// `ArmorStand.getLightProbePosition` for a stand that is not a marker is
    /// `Entity.getEyePosition`: `EntityType.ARMOR_STAND` declares
    /// `eyeHeight(1.7775)`, and a small stand (`isBaby()`) is the same type
    /// scaled by 0.5 with `withEyeHeight(0.9875)`. Horizon read
    /// `floor(base + 0.5)` for every stand, so a stand standing on a lit floor
    /// took the floor's light instead of the light at its head.
    #[test]
    fn a_stand_is_lit_from_the_block_its_eye_is_in() {
        let dark = |_: i32, _: i32, _: i32| 0u8;
        // Full stand on y = 64: eye 65.7775, cell 65 (the old 64.5 was 64).
        assert_eq!(stand(0.5, 64.0, 0.5).light_probe(1.0, dark), [0, 65, 0]);
        assert_eq!((64.0_f64 + 0.5).floor() as i32, 64);
        // Small stand: eye 0.9875 above its base, so y = 64.2 is 65.1875.
        let mut small = stand(0.5, 64.2, 0.5);
        small.small = true;
        assert_eq!(small.light_probe(1.0, dark), [0, 65, 0]);
        assert_eq!((64.2_f64 + 0.5).floor() as i32, 64);
        // A small stand at y = 64.0 stays in its own cell: eye 64.9875.
        let mut low = stand(0.5, 64.0, 0.5);
        low.small = true;
        assert_eq!(low.light_probe(1.0, dark), [0, 64, 0]);
    }

    /// `getEyePosition(partialTick)` interpolates: a stand falling from 70 to
    /// 69 is probed 0.5 down at the middle of the frame, 69.5 + 1.7775 in the
    /// cell at 71, while its feet's cell is 69.
    #[test]
    fn a_stand_probe_follows_the_frame() {
        let dark = |_: i32, _: i32, _: i32| 0u8;
        let mut falling = stand(0.5, 70.0, 0.5);
        falling.interpolate(Some((Vec3::new(0.5, 69.0, 0.5), 0.0)));
        assert_eq!(falling.light_probe(0.0, dark), [0, 71, 0]);
        assert_eq!(falling.light_probe(0.5, dark), [0, 71, 0]);
        assert_eq!(falling.light_probe(1.0, dark), [0, 70, 0]);
        assert_eq!(falling.position.y.floor() as i32, 69);
    }

    /// A marker walks `BlockPos.betweenClosed` of its own box - the size it has
    /// without the marker flag, 0.5 by 1.975 - from `position()`, taking the
    /// first cell at brightness 15 and else the brightest. Horizon used the
    /// stand's own cell, so a marker (a hologram on a dark ceiling) was lit by
    /// its feet's block.
    #[test]
    fn a_marker_scans_the_cells_of_its_own_box() {
        let at = |want: [i32; 3], level: u8| {
            move |x: i32, y: i32, z: i32| {
                if [x, y, z] == want { level } else { 0 }
            }
        };
        let mut marker = stand(0.5, 64.0, 0.5);
        marker.marker = true;
        // The box is x/z 0.25..0.75 - the cell at 0 - and y 64..65.975.
        assert_eq!(marker.light_probe(1.0, at([0, 65, 0], 15)), [0, 65, 0]);
        assert_eq!(marker.light_probe(1.0, at([0, 65, 0], 9)), [0, 65, 0]);
        // Nothing lit: the first cell walked, x fastest then y then z.
        assert_eq!(marker.light_probe(1.0, |_, _, _| 0u8), [0, 64, 0]);
        // The cell above the box is not scanned, so its 15 does not win.
        assert_eq!(marker.light_probe(1.0, at([0, 66, 0], 15)), [0, 64, 0]);
        // 15 wins over a brighter-looking neighbour, and y is walked before z:
        // (0, 65, 0) comes before (0, 64, 1) though both are at 15.
        let mut wide = stand(0.5, 64.0, 0.9);
        wide.marker = true;
        let two = |x: i32, y: i32, z: i32| {
            if (x, y, z) == (0, 65, 0) || (x, y, z) == (0, 64, 1) {
                15
            } else {
                0
            }
        };
        assert_eq!(wide.light_probe(1.0, two), [0, 65, 0]);
        // `getLightProbePosition`'s marker branch reads `position()`, not the
        // partial-tick position: falling does not move the scan.
        let mut moving = stand(0.5, 64.0, 0.5);
        moving.marker = true;
        moving.interpolate(Some((Vec3::new(0.5, 70.0, 0.5), 0.0)));
        assert_eq!(moving.light_probe(0.0, |_, _, _| 0u8), [0, 70, 0]);
        assert_eq!(moving.light_probe(0.5, |_, _, _| 0u8), [0, 70, 0]);
    }

    /// `EntityRenderer.getBlockLightLevel`: a stand `isOnFire` is drawn at 15
    /// block light whatever the level says, and the sky light is the probe
    /// cell's own.
    #[test]
    fn a_burning_stand_is_drawn_at_full_block_light() {
        let light = |sky: u8, block: u8| (sky << 4) | block;
        let cold = stand(0.5, 64.0, 0.5);
        assert_eq!(cold.render_light(light(15, 0)), (15 * 16) as u32);
        assert_eq!(cold.render_light(light(0, 9)), ((9 * 16) << 8) as u32);
        let burning = ArmorStand {
            fire_ticks: 100,
            ..stand(0.5, 64.0, 0.5)
        };
        assert_eq!(
            burning.render_light(light(15, 0)),
            ((15 * 16) | ((15 * 16) << 8)) as u32
        );
        assert_eq!(burning.render_light(light(0, 0)), ((15 * 16) << 8) as u32);
    }
}
#[cfg(test)]
mod merge_tests {
    use super::*;

    fn item(id: &str, count: u32, x: f64, age: i32) -> ItemEntity {
        let at = Vec3::new(x, 64.0, 0.5);
        ItemEntity {
            random: horizon_core::random::LegacyRandom::new(0),
            rotation: [0.0, 0.0],
            join: Default::default(),
            no_gravity: false,
            id: 0,
            thrower: None,
            target: None,
            stack: ItemStack {
                item: std::sync::Arc::from(id),
                count,
                damage: 0,
                extra: None,
            },
            health: 5,
            fire_ticks: 0,
            position: at,
            previous: at,
            velocity: Vec3::ZERO,
            on_ground: true,
            vertical_collision: false,
            horizontal_collision: false,
            fall_distance: 0.0,
            age,
            pickup_delay: 0,
            bob_offset: 0.0,
            ticks: age.max(0) as u32,
            needs_sync: false,
            stuck: Vec3::ZERO,
            was_touching_water: false,
        }
    }
    fn max_stack(stack: &ItemStack) -> u32 {
        if &*stack.item == "minecraft:ender_pearl" {
            16
        } else {
            64
        }
    }
    /// Merges around `items[i]` as its tick would; returns the counts left.
    fn merge(items: &mut [ItemEntity], i: usize) -> Vec<Option<u32>> {
        let mut removed = vec![false; items.len()];
        let mut cells = ItemCells::new(items);
        merge_with_neighbours(items, &mut removed, &mut cells, i, &max_stack);
        items
            .iter()
            .zip(removed)
            .map(|(item, gone)| (!gone).then_some(item.stack.count))
            .collect()
    }

    #[test]
    fn the_smaller_stack_goes_into_the_larger() {
        let mut items = [
            item("minecraft:sand", 3, 0.5, 41),
            item("minecraft:sand", 5, 1.2, 7),
        ];
        assert_eq!(merge(&mut items, 0), [None, Some(8)]);
        // ItemEntity.merge keeps the younger age; the ticking item's is
        // still the one before its tick.
        assert_eq!(items[1].age, 7);
        let mut items = [
            item("minecraft:sand", 5, 0.5, 41),
            item("minecraft:sand", 3, 1.2, 7),
        ];
        assert_eq!(merge(&mut items, 0), [Some(8), None]);
        assert_eq!(items[0].age, 8, "age 7, plus the rest of this tick");
    }

    /// ItemEntity.merge uses signed room, including zero and negative transfers.
    #[test]
    fn overstacked_items_transfer_back_without_losing_merge_metadata() {
        for (counts, expected) in [([80, 10], [64, 26]), ([64, 26], [64, 26])] {
            let mut items = [
                item("minecraft:stone", counts[0], 0.5, 41),
                item("minecraft:stone", counts[1], 0.6, 7),
            ];
            for item in &mut items {
                item.stack.data_mut().set_other(
                    "minecraft:max_stack_size",
                    Some(horizon_world::nbt::Tag::Int(99)),
                );
            }
            let before = [items[0].stack.clone(), items[1].stack.clone()];
            items[1].pickup_delay = 30;
            let mut removed = [false; 2];
            let mut cells = ItemCells::new(&items);
            merge_with_neighbours(&mut items, &mut removed, &mut cells, 0, &|_| 99);
            assert_eq!([items[0].stack.count, items[1].stack.count], expected);
            assert_eq!(removed, [false, false]);
            assert_eq!(items[0].pickup_delay, 30);
            assert_eq!(items[0].age, 8, "younger age 7, then this tick ends");
            for (item, stack) in items.iter().zip(&before) {
                assert!(item.stack.same_item_same_components(stack));
            }
        }
    }

    #[test]
    fn items_merge_only_within_half_a_block_sideways() {
        // Boxes 0.25 wide grown by 0.5: centres closer than 0.75 meet.
        let mut items = [
            item("minecraft:sand", 1, 0.5, 1),
            item("minecraft:sand", 1, 1.24, 1),
        ];
        assert_eq!(merge(&mut items, 0), [None, Some(2)]);
        let mut items = [
            item("minecraft:sand", 1, 0.5, 1),
            item("minecraft:sand", 1, 1.26, 1),
        ];
        assert_eq!(merge(&mut items, 0), [Some(1), Some(1)]);
        let mut items = [
            item("minecraft:sand", 1, 0.5, 1),
            item("minecraft:sand", 1, 0.5, 1),
        ];
        items[1].position.y += 0.3;
        assert_eq!(
            merge(&mut items, 0),
            [Some(1), Some(1)],
            "no vertical growth"
        );
    }

    #[test]
    fn different_or_full_stacks_stay_apart() {
        let mut items = [
            item("minecraft:sand", 1, 0.5, 1),
            item("minecraft:gravel", 1, 0.5, 1),
        ];
        assert_eq!(merge(&mut items, 0), [Some(1), Some(1)]);
        let mut items = [
            item("minecraft:sand", 60, 0.5, 1),
            item("minecraft:sand", 10, 0.5, 1),
        ];
        assert_eq!(
            merge(&mut items, 0),
            [Some(60), Some(10)],
            "70 is over a stack"
        );
        let mut items = [
            item("minecraft:ender_pearl", 10, 0.5, 1),
            item("minecraft:ender_pearl", 6, 0.5, 1),
        ];
        assert_eq!(merge(&mut items, 0), [Some(16), None]);
        let mut items = [
            item("minecraft:ender_pearl", 16, 0.5, 1),
            item("minecraft:ender_pearl", 1, 0.5, 1),
        ];
        assert_eq!(
            merge(&mut items, 0),
            [Some(16), Some(1)],
            "a full stack doesn't merge"
        );
    }

    #[test]
    fn unmergeable_items_are_skipped() {
        for delay in [i64::from(i16::MAX), i64::from(u32::MAX)] {
            let mut items = [
                item("minecraft:sand", 1, 0.5, 1),
                item("minecraft:sand", 1, 0.5, 1),
            ];
            items[1].pickup_delay = delay;
            assert_eq!(merge(&mut items, 0), [Some(1), Some(1)]);
        }
        // At age 6000 the other item's lifetime is over; this one's tick
        // started at 5999, so it may still merge into a live item.
        let mut items = [
            item("minecraft:sand", 1, 0.5, 6000),
            item("minecraft:sand", 1, 0.5, 6000),
        ];
        assert_eq!(merge(&mut items, 0), [Some(1), Some(1)]);
        items[1].age = 10;
        assert_eq!(merge(&mut items, 0), [None, Some(2)]);
        assert_eq!(items[1].age, 10);
    }

    #[test]
    fn a_stack_takes_its_neighbours_in_spawn_order_until_full() {
        let mut items = [
            item("minecraft:sand", 40, 0.5, 1),
            item("minecraft:sand", 20, 0.7, 1),
            item("minecraft:sand", 10, 0.3, 1),
            item("minecraft:sand", 3, 0.4, 1),
        ];
        // 40 + 20 = 60; 60 + 10 is over 64; then 3 more.
        assert_eq!(merge(&mut items, 0), [Some(63), None, Some(10), None]);
        let mut items = [
            item("minecraft:sand", 1, 0.5, 1),
            item("minecraft:sand", 1, 0.5, 1),
        ];
        items[1].pickup_delay = 30;
        assert_eq!(merge(&mut items, 0), [None, Some(2)]);
        assert_eq!(items[1].pickup_delay, 30, "the longer pickup delay stays");
    }

    #[test]
    fn the_cells_follow_moves() {
        let mut items = [
            item("minecraft:sand", 1, 0.5, 1),
            item("minecraft:sand", 1, 5.5, 1),
        ];
        let mut removed = [false; 2];
        let mut cells = ItemCells::new(&items);
        let from = items[1].position;
        items[1].position.x = 0.9;
        cells.moved(1, from, items[1].position);
        merge_with_neighbours(&mut items, &mut removed, &mut cells, 0, &max_stack);
        assert_eq!(removed, [true, false]);
        assert_eq!(items[1].stack.count, 2);
    }
}

#[cfg(test)]
mod world_border_tests {
    use super::*;

    /// `Entity.move`'s last step, `getBlockSpeedFactor`: a dropped item sliding
    /// over soul sand or honey keeps 40% of the horizontal speed it keeps over
    /// stone, in the same tick. Skips quietly without the vanilla data (CI has none).
    #[test]
    fn a_dropped_item_on_soul_sand_or_honey_is_slowed_by_the_move() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("movement-properties.nbt").exists() {
            return;
        }
        let assets = crate::assets::Assets::load(&root).unwrap();
        let render = assets.reload(&root).unwrap();
        let facts = crate::blocks::BlockFacts::load(&root, assets.registry).unwrap();
        let fluids = crate::motion::FluidResources::new(&assets, &facts).unwrap();
        let collision = crate::collisions::load(&root, assets.registry).unwrap();
        let motion = crate::motion::MotionResources::load(&root, &assets).unwrap();
        let mut after = Vec::new();
        // The tops of the blocks: stone 1, soul sand 7/8, honey 15/16.
        for (block, top) in [
            ("minecraft:stone", 4.0),
            ("minecraft:soul_sand", 3.875),
            ("minecraft:honey_block", 3.9375),
        ] {
            let mut world = World::test_scene(&assets, &render, 3).unwrap();
            let id = assets.registry.default_state(block).unwrap();
            for x in 18..=23 {
                for z in 19..=21 {
                    world.set(x, 3, z, id).unwrap();
                }
            }
            let mut item = super::honey_slide_tests::falling_item(19.5, top, 20.5, 0.2, 0.0, 0.0);
            item.on_ground = true;
            item.tick(
                &world,
                assets.registry,
                &facts,
                &collision,
                false,
                &fluids,
                &[],
                None,
                Some(&motion.blocks),
            );
            after.push(item.velocity.x);
        }
        let (stone, soul, honey) = (after[0], after[1], after[2]);
        assert!(stone > 0.0);
        assert!(
            (soul - stone * f64::from(0.4_f32)).abs() < 1.0e-9,
            "soul sand {soul} vs stone {stone}"
        );
        assert!(
            (honey - stone * f64::from(0.4_f32)).abs() < 1.0e-9,
            "honey {honey} vs stone {stone}"
        );
    }

    /// `Entity.collectColliders` (#7080): the world border's wall stops an
    /// item and an experience orb as it stops the player. A border of size
    /// 10 around (20, 20) has its east wall at x = 25; each body starts at
    /// x = 24 on the flat ground of the test scene, moving 1.5 east.
    #[test]
    fn the_world_border_stops_items_and_experience_orbs() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("movement-properties.nbt").exists() {
            return;
        }
        let assets = crate::assets::Assets::load(&root).unwrap();
        let render = assets.reload(&root).unwrap();
        let facts = crate::blocks::BlockFacts::load(&root, assets.registry).unwrap();
        let fluids = crate::motion::FluidResources::new(&assets, &facts).unwrap();
        let collision = crate::collisions::load(&root, assets.registry).unwrap();
        let world = World::test_scene(&assets, &render, 3).unwrap();
        let border = BorderGeometry::new([20.0, 20.0], 10.0, 29_999_984).unwrap();
        for bordered in [false, true] {
            let border = bordered.then_some(&border);
            let mut item = super::honey_slide_tests::falling_item(24.0, 4.0, 20.5, 1.5, 0.0, 0.0);
            item.tick(
                &world,
                assets.registry,
                &facts,
                &collision,
                false,
                &fluids,
                &[],
                border,
                None,
            );
            let mut orb = ExperienceOrb {
                rotation: [0.0, 0.0],
                join: Default::default(),
                no_gravity: false,
                id: 2,
                position: Vec3::new(24.0, 4.0, 20.5),
                previous: Vec3::new(24.0, 4.0, 20.5),
                velocity: Vec3::new(1.5, 0.0, 0.0),
                value: 1,
                age: 0,
                on_ground: true,
                count: 1,
                ticks: 0,
                health: 5,
                air: 300,
                following: None,
            };
            orb.tick(
                &world,
                &facts,
                &collision,
                None,
                true,
                &mut || 0.0,
                border,
                &fluids,
                &mut |_, _| {},
            );
            for (what, east) in [
                ("item", item.bounds().max_position().x),
                ("orb", orb.bounds().max_position().x),
            ] {
                if bordered {
                    assert!(
                        (east - 25.0).abs() < 1.0e-7,
                        "the {what} stops on the wall at x = 25, got {east}"
                    );
                } else {
                    assert!(east > 25.0, "no border: the {what} goes on, got {east}");
                }
            }
        }
    }
}

#[cfg(test)]
mod honey_slide_tests {
    use super::*;

    /// Gravity still stacks on idle ticks; waking every 4th tick zeroes Y on
    /// the ground (`ItemEntity.tick` with `onGround`).
    #[test]
    fn a_resting_item_skips_move_three_of_four_ticks() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("movement-properties.nbt").exists() {
            return;
        }
        let assets = crate::assets::Assets::load(&root).unwrap();
        let render = assets.reload(&root).unwrap();
        let facts = crate::blocks::BlockFacts::load(&root, assets.registry).unwrap();
        let fluids = crate::motion::FluidResources::new(&assets, &facts).unwrap();
        let collision = crate::collisions::load(&root, assets.registry).unwrap();
        let mut world = World::test_scene(&assets, &render, 3).unwrap();
        let stone = assets.registry.default_state("minecraft:stone").unwrap();
        for x in 19..=21 {
            for z in 19..=21 {
                world.set(x, 3, z, stone).unwrap();
            }
        }
        let at = Vec3::new(20.5, 4.0, 20.5);
        let mut item = falling_item(at.x, at.y, at.z, 0.0, 0.0, 0.0);
        item.on_ground = true;
        item.age = 0;
        item.ticks = 0;
        item.id = 0;
        let mut ys = Vec::new();
        for _ in 0..8 {
            item.tick(
                &world,
                assets.registry,
                &facts,
                &collision,
                false,
                &fluids,
                &[],
                None,
                None,
            );
            ys.push(item.velocity.y);
        }
        // Idle ticks keep gravity; the 4th tick's move hits the floor and
        // clears Y (vanilla's sequence for a resting item).
        assert_eq!(ys, vec![-0.04, -0.08, -0.12, 0.0, -0.04, -0.08, -0.12, 0.0]);
        assert_eq!(item.position.y, at.y);
    }

    /// #4558: idle ticks still run `BubbleColumnBlock.entityInside`, so lift
    /// builds and the next wake-up rises off the soul sand.
    #[test]
    fn a_grounded_item_in_a_soul_sand_column_rises() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("movement-properties.nbt").exists() {
            return;
        }
        let assets = crate::assets::Assets::load(&root).unwrap();
        let render = assets.reload(&root).unwrap();
        let facts = crate::blocks::BlockFacts::load(&root, assets.registry).unwrap();
        let fluids = crate::motion::FluidResources::new(&assets, &facts).unwrap();
        let collision = crate::collisions::load(&root, assets.registry).unwrap();
        let mut world = World::test_scene(&assets, &render, 3).unwrap();
        let soul = assets
            .registry
            .default_state("minecraft:soul_sand")
            .unwrap();
        let mut column = assets
            .registry
            .state(
                assets
                    .registry
                    .default_state("minecraft:bubble_column")
                    .unwrap(),
            )
            .unwrap()
            .clone();
        column.properties.insert("drag".into(), "false".into());
        let column = assets.registry.resolve(&column).unwrap();
        for x in 19..=21 {
            for z in 19..=21 {
                world.set(x, 3, z, soul).unwrap();
                for y in 4..=8 {
                    world.set(x, y, z, column).unwrap();
                }
            }
        }
        let at = Vec3::new(20.5, 4.0, 20.5);
        let mut item = falling_item(at.x, at.y, at.z, 0.0, 0.0, 0.0);
        item.on_ground = true;
        item.age = 0;
        item.id = 1;
        let before = item.position.y;
        for _ in 0..16 {
            item.tick(
                &world,
                assets.registry,
                &facts,
                &collision,
                false,
                &fluids,
                &[],
                None,
                None,
            );
        }
        assert!(
            item.position.y > before + 0.5,
            "expected rise from soul-sand column, y={} (was {})",
            item.position.y,
            before
        );
    }

    /// `Entity.sendBubbleColumnParticles` is for the top of a column only: an item
    /// inside the column shows nothing, one in its surface block shows the block.
    #[test]
    fn an_item_at_the_top_of_a_bubble_column_is_at_its_surface() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("movement-properties.nbt").exists() {
            return;
        }
        let assets = crate::assets::Assets::load(&root).unwrap();
        let render = assets.reload(&root).unwrap();
        let facts = crate::blocks::BlockFacts::load(&root, assets.registry).unwrap();
        let collision = crate::collisions::load(&root, assets.registry).unwrap();
        let mut world = World::test_scene(&assets, &render, 3).unwrap();
        let mut column = assets
            .registry
            .state(
                assets
                    .registry
                    .default_state("minecraft:bubble_column")
                    .unwrap(),
            )
            .unwrap()
            .clone();
        column.properties.insert("drag".into(), "false".into());
        let column = assets.registry.resolve(&column).unwrap();
        for y in 4..=8 {
            world.set(20, y, 20, column).unwrap();
        }
        let surfaces = |y: f64| {
            let item = falling_item(20.5, y, 20.5, 0.0, 0.0, 0.0);
            crate::motion::bubble_column_surfaces(
                &world,
                assets.registry,
                &facts,
                &collision,
                item.bounds(),
            )
        };
        assert_eq!(surfaces(8.2), vec![BlockPos::new(20, 8, 20)]);
        assert_eq!(surfaces(5.0), Vec::new());
        // Straddling the two top blocks, it is still at the top of one column.
        assert_eq!(surfaces(7.9), vec![BlockPos::new(20, 8, 20)]);
        assert_eq!(surfaces(9.0), Vec::new());
    }

    pub(super) fn falling_item(x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64) -> ItemEntity {
        let at = Vec3::new(x, y, z);
        ItemEntity {
            random: horizon_core::random::LegacyRandom::new(0),
            rotation: [0.0, 0.0],
            join: Default::default(),
            no_gravity: false,
            id: 1,
            thrower: None,
            target: None,
            stack: ItemStack {
                item: std::sync::Arc::from("minecraft:stone"),
                count: 1,
                damage: 0,
                extra: None,
            },
            health: 5,
            fire_ticks: 0,
            position: at,
            previous: at,
            velocity: Vec3::new(vx, vy, vz),
            on_ground: false,
            vertical_collision: false,
            horizontal_collision: false,
            fall_distance: 0.0,
            age: 10,
            pickup_delay: 0,
            bob_offset: 0.0,
            ticks: 10,
            needs_sync: false,
            stuck: Vec3::ZERO,
            was_touching_water: false,
        }
    }

    fn old_delta_y(dy: f64) -> f64 {
        dy / f64::from(0.98_f32) + 0.08
    }
    fn new_delta_y(dy: f64) -> f64 {
        (dy - 0.08) * f64::from(0.98_f32)
    }

    #[test]
    fn a_falling_item_beside_honey_slides_down() {
        // Item width 0.25: overlap 0.5625. Honey at (0, 10, 0); centre 0.5.
        // x = -0.0625 is against the west face (inset 1/16).
        let mut item = falling_item(-0.0625, 10.4, 0.5, 0.12, -0.20, 0.04);
        assert!(old_delta_y(-0.20) < -0.08);
        assert!(old_delta_y(-0.20) >= -0.13);
        item.honey_block_inside([0, 10, 0]);
        assert_eq!(item.velocity.x, 0.12);
        assert_eq!(item.velocity.z, 0.04);
        assert_eq!(item.velocity.y, new_delta_y(-0.05));
    }

    #[test]
    fn a_fast_fall_scales_the_horizontal_speed() {
        let mut item = falling_item(-0.0625, 10.4, 0.5, 0.2, -0.30, -0.1);
        let old = old_delta_y(-0.30);
        assert!(old < -0.13);
        item.honey_block_inside([0, 10, 0]);
        let factor = -0.05 / old;
        assert_eq!(item.velocity.x, 0.2 * factor);
        assert_eq!(item.velocity.z, -0.1 * factor);
        assert_eq!(item.velocity.y, new_delta_y(-0.05));
    }

    #[test]
    fn an_item_on_the_ground_or_on_top_does_not_slide() {
        let mut grounded = falling_item(-0.0625, 10.4, 0.5, 0.0, -0.20, 0.0);
        grounded.on_ground = true;
        grounded.honey_block_inside([0, 10, 0]);
        assert_eq!(grounded.velocity.y, -0.20);

        let mut on_top = falling_item(0.5, 10.9375, 0.5, 0.0, -0.20, 0.0);
        on_top.honey_block_inside([0, 10, 0]);
        assert_eq!(on_top.velocity.y, -0.20, "standing on the honey");
    }

    #[test]
    fn a_centred_or_slow_item_does_not_slide() {
        let mut centred = falling_item(0.5, 10.4, 0.5, 0.0, -0.20, 0.0);
        centred.honey_block_inside([0, 10, 0]);
        assert_eq!(centred.velocity.y, -0.20, "inside, not on a side");

        let mut slow = falling_item(-0.0625, 10.4, 0.5, 0.0, -0.04, 0.0);
        assert!(old_delta_y(-0.04) >= -0.08);
        slow.honey_block_inside([0, 10, 0]);
        assert_eq!(slow.velocity.y, -0.04);
    }
}

#[cfg(test)]
mod slime_bounce_tests {
    use super::*;

    #[test]
    fn landing_on_slime_bounces_a_dropped_item_like_vanilla() {
        // Measured on a vanilla 26.3 server: an item falling at -0.6097 that
        // is stopped 0.4865 down leaves with 0.4950 / 0.98 after the bounce.
        let y = vertical_collision_velocity(-0.6097, -0.4865, ITEM_GRAVITY, true, true);
        assert!((y - 0.4950 / 0.98).abs() < 1e-3, "{y}");
        // A tiny landing: -0.0792 stopped after 0.06 gives 0.0862 (vanilla).
        let y = vertical_collision_velocity(-0.0792, -0.06, ITEM_GRAVITY, true, true);
        assert!((y - 0.0862).abs() < 1e-3, "{y}");
        assert_eq!(
            vertical_collision_velocity(-0.04, -0.01, ITEM_GRAVITY, true, true),
            0.0,
            "gravity alone is not a bounce"
        );
    }

    #[test]
    fn landing_on_stone_or_hitting_a_ceiling_zeros_y() {
        assert_eq!(
            vertical_collision_velocity(-1.0, -0.5, ITEM_GRAVITY, true, false),
            0.0
        );
        assert_eq!(
            vertical_collision_velocity(1.0, 0.5, ITEM_GRAVITY, false, true),
            0.0
        );
    }

    #[test]
    fn slime_step_on_drags_slow_vertical_speed_only() {
        let scale = slime_step_on_scale(0.0862).unwrap();
        assert!((scale - 0.41724).abs() < 1e-5);
        assert_eq!(slime_step_on_scale(0.1), None);
    }

    #[test]
    fn a_moving_piston_carrying_slime_still_counts_as_slime() {
        assert!(slime_support_name("minecraft:slime_block", None));
        assert!(slime_support_name(
            "minecraft:moving_piston",
            Some("minecraft:slime_block")
        ));
        assert!(slime_support_name(
            "minecraft:air",
            Some("minecraft:slime_block")
        ));
        assert!(!slime_support_name(
            "minecraft:moving_piston",
            Some("minecraft:stone")
        ));
        assert!(!slime_support_name("minecraft:stone", None));
        assert!(!slime_support_name(
            "minecraft:honey_block",
            Some("minecraft:honey_block")
        ));
    }

    fn grounded_item() -> ItemEntity {
        let at = Vec3::new(0.5, 2.0, 0.5);
        ItemEntity {
            random: horizon_core::random::LegacyRandom::new(0),
            rotation: [0.0, 0.0],
            join: Default::default(),
            no_gravity: false,
            id: 1,
            thrower: None,
            target: None,
            stack: ItemStack {
                item: std::sync::Arc::from("minecraft:stone"),
                count: 1,
                damage: 0,
                extra: None,
            },
            health: 5,
            fire_ticks: 0,
            position: at,
            previous: at,
            velocity: Vec3::ZERO,
            on_ground: true,
            vertical_collision: false,
            horizontal_collision: false,
            fall_distance: 0.0,
            age: 10,
            pickup_delay: 0,
            bob_offset: 0.0,
            ticks: 10,
            needs_sync: false,
            stuck: Vec3::ZERO,
            was_touching_water: false,
        }
    }

    #[test]
    fn a_piston_slime_bounce_launches_the_item_off_the_ground() {
        let mut item = grounded_item();
        item.piston_slime_bounce(Direction::Up);
        assert_eq!(item.velocity, Vec3::new(0.0, 1.0, 0.0));
        assert!(!item.on_ground);
        assert!(item.needs_sync);
        item.needs_sync = false;
        item.piston_slime_bounce(Direction::East);
        assert_eq!(item.velocity, Vec3::new(1.0, 1.0, 0.0));
        assert!(!item.on_ground);
        assert!(item.needs_sync);
    }

    #[test]
    fn a_slime_landing_stays_upward_after_item_friction() {
        // ItemEntity then multiplies Y by 0.98F and applies -0.5 only if still
        // negative on the ground. A slime bounce must remain positive.
        let bounced =
            vertical_collision_velocity(-1.0, -0.5, ITEM_GRAVITY, true, true) * f64::from(0.98_f32);
        assert!(bounced > 0.0);
    }

    #[test]
    fn item_air_drag_widens_get_air_drag() {
        // ItemEntity.tick: getAirDrag 0.98F, not the f64 0.98 (#6814).
        let mut item = super::honey_slide_tests::falling_item(0.5, 64.0, 0.5, 0.1, 0.2, -0.1);
        let world = World::new(horizon_world::registry::BlockStateId(0), 0);
        let facts = BlockFacts {
            states: Vec::new(),
            air: horizon_world::registry::BlockStateId(0),
            woods: Default::default(),
        };
        item.tick_settle(
            &world,
            &facts,
            true,
            ItemStep {
                old_velocity: item.velocity,
                did_move: true,
                vertical_contact: None,
                fall_move: None,
            },
        );
        assert_eq!(item.velocity.x.to_bits(), 0x3fb9_1687_3333_3334);
        assert_eq!(item.velocity.y.to_bits(), 0x3fc9_1687_3333_3334);
        assert_eq!(item.velocity.z.to_bits(), 0xbfb9_1687_3333_3334);
    }

    #[test]
    fn item_fluid_drags_widen_float_constants() {
        // setUnderwaterMovement 0.99F / 5.0E-4F; setUnderLavaMovement 0.95F.
        let mut water = super::honey_slide_tests::falling_item(0.5, 64.0, 0.5, 0.1, 0.01, -0.2);
        water.set_fluid_movement(0.99_f32);
        assert_eq!(water.velocity.x.to_bits(), 0x3fb9_5810_6666_6667);
        assert_eq!(water.velocity.y.to_bits(), 0x3f85_8106_25ae_147b);
        assert_eq!(water.velocity.z.to_bits(), 0xbfc9_5810_6666_6667);
        let mut lava = super::honey_slide_tests::falling_item(0.5, 64.0, 0.5, 0.1, 0.01, -0.2);
        lava.set_fluid_movement(0.95_f32);
        assert_eq!(lava.velocity.x.to_bits(), 0x3fb8_51eb_8000_0000);
        assert_eq!(lava.velocity.y.to_bits(), 0x3f85_8106_25ae_147b);
        assert_eq!(lava.velocity.z.to_bits(), 0xbfc8_51eb_8000_0000);
    }

    #[test]
    fn a_slime_bounce_marks_the_item_for_immediate_sync() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("movement-properties.nbt").exists() {
            return;
        }
        let assets = crate::assets::Assets::load(&root).unwrap();
        let render = assets.reload(&root).unwrap();
        let facts = crate::blocks::BlockFacts::load(&root, assets.registry).unwrap();
        let fluids = crate::motion::FluidResources::new(&assets, &facts).unwrap();
        let collision = crate::collisions::load(&root, assets.registry).unwrap();
        let mut world = World::test_scene(&assets, &render, 3).unwrap();
        let slime = assets
            .registry
            .default_state("minecraft:slime_block")
            .unwrap();
        for x in 19..=21 {
            for z in 19..=21 {
                world.set(x, 3, z, slime).unwrap();
            }
        }
        // ItemEntity.tick sets needsSync when getDeltaMovement().subtract(oldMovement).lengthSqr() > 0.01.
        let mut falling = grounded_item();
        falling.position = Vec3::new(20.5, 4.2, 20.5);
        falling.previous = falling.position;
        falling.velocity = Vec3::new(0.0, -0.5, 0.0);
        falling.on_ground = false;
        falling.age = 0;
        falling.id = 1;
        falling.tick(
            &world,
            assets.registry,
            &facts,
            &collision,
            false,
            &fluids,
            &[],
            None,
            None,
        );
        assert_eq!(falling.position.y, 4.0);
        assert!(falling.velocity.y > 0.0);
        assert!(falling.on_ground);
        assert!(falling.needs_sync);

        // An idle tick where velocity only steps by -0.04 (0.04^2 = 0.0016 <= 0.01)
        // does not set needs_sync.
        falling.velocity = Vec3::ZERO;
        falling.needs_sync = false;
        falling.tick(
            &world,
            assets.registry,
            &facts,
            &collision,
            false,
            &fluids,
            &[],
            None,
            None,
        );
        assert!(!falling.needs_sync);
    }
}

#[cfg(test)]
mod unstuck_tests {
    use super::*;

    const BORDER: BorderBounds = BorderBounds {
        min_x: -1.0e6,
        min_z: -1.0e6,
        max_x: 1.0e6,
        max_z: 1.0e6,
    };

    /// A level of full blocks at `solid`; every other cell is air.
    fn shapes(solid: &[(i32, i32, i32)]) -> impl FnMut(BlockPos) -> Option<VoxelShape> + '_ {
        move |p| {
            if solid.contains(&(p.x, p.y, p.z)) {
                VoxelShape::from_box(Aabb::block(p)).ok()
            } else {
                None
            }
        }
    }

    /// `level.noCollision(box)` over the same cells the search reads.
    fn free(solid: &[(i32, i32, i32)], b: Aabb) -> bool {
        !solid
            .iter()
            .any(|&(x, y, z)| Aabb::block(BlockPos::new(x, y, z)).intersects(b))
    }

    #[test]
    fn an_orb_spawned_in_a_block_moves_to_a_free_box() {
        // One block at y 0..1 and an orb standing in its upper half, as a mob
        // killed inside one leaves it: its box overlaps the block by 0.1.
        let solid = [(0, 0, 0)];
        let at = Vec3::new(0.5, 0.9, 0.5);
        let moved = ExperienceOrb::unstuck(at, &BORDER, shapes(&solid)).expect("the orb moves");
        // The nearest free box sits on the block's top face.
        assert_eq!(moved, Vec3::new(0.5, 1.0, 0.5));
        assert!(!ExperienceOrb::box_at(moved).intersects(Aabb::block(BlockPos::new(0, 0, 0))));
    }

    #[test]
    fn an_orb_in_open_air_keeps_its_position() {
        let solid = [(0, 0, 0)];
        let at = Vec3::new(0.5, 2.0, 0.5);
        assert!(free(&solid, ExperienceOrb::box_at(at)));
        // Nothing to move for: the spawn sites keep the position as given.
        assert_eq!(ExperienceOrb::unstuck(at, &BORDER, shapes(&solid)), None);
    }
}

#[cfg(test)]
mod entity_collision_tests {
    use super::honey_slide_tests::falling_item;
    use super::*;

    /// `Entity.collide` stops an item at the entities that `canBeCollidedWith` it: dropped
    /// above a boat's box (or a still happy ghast's) it lands on the box's top and rests,
    /// where with no box it falls to the floor of the scene.
    #[test]
    fn an_item_lands_on_an_entity_box_under_it() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("movement-properties.nbt").exists() {
            return;
        }
        let assets = crate::assets::Assets::load(&root).unwrap();
        let render = assets.reload(&root).unwrap();
        let facts = crate::blocks::BlockFacts::load(&root, assets.registry).unwrap();
        let fluids = crate::motion::FluidResources::new(&assets, &facts).unwrap();
        let collision = crate::collisions::load(&root, assets.registry).unwrap();
        let mut world = World::test_scene(&assets, &render, 3).unwrap();
        let stone = assets.registry.default_state("minecraft:stone").unwrap();
        world.set(20, 3, 20, stone).unwrap();
        let rest = |boxes: &[Aabb]| {
            let mut item = falling_item(20.5, 12.0, 20.5, 0.0, 0.0, 0.0);
            item.id = 1;
            for _ in 0..80 {
                item.tick(
                    &world,
                    assets.registry,
                    &facts,
                    &collision,
                    false,
                    &fluids,
                    boxes,
                    None,
                    None,
                );
            }
            item.position.y
        };
        // The stone's top is at 4.0; a box from 5.0 to 9.0 is in the way.
        let platform = Aabb::new(Vec3::new(19.0, 5.0, 19.0), Vec3::new(22.0, 9.0, 22.0));
        assert!((rest(&[]) - 4.0).abs() < 1.0e-6, "on the stone with no box");
        assert!(
            (rest(&[platform]) - 9.0).abs() < 1.0e-6,
            "on top of the box, at its top"
        );
    }

    /// `WebBlock.entityInside` / `PowderSnowBlock.entityInside` make an item stuck: a
    /// fast item sliding into a cobweb is stopped inside it, in powder snow it is slowed.
    #[test]
    fn a_cobweb_and_powder_snow_slow_an_item() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("movement-properties.nbt").exists() {
            return;
        }
        let assets = crate::assets::Assets::load(&root).unwrap();
        let render = assets.reload(&root).unwrap();
        let facts = crate::blocks::BlockFacts::load(&root, assets.registry).unwrap();
        let fluids = crate::motion::FluidResources::new(&assets, &facts).unwrap();
        let collision = crate::collisions::load(&root, assets.registry).unwrap();
        let mut world = World::test_scene(&assets, &render, 3).unwrap();
        let state = |n: &str| assets.registry.default_state(n).unwrap();
        for x in 16..32 {
            world.set(x, 3, 20, state("minecraft:stone")).unwrap();
        }
        let mut slide = |block: Option<&str>| {
            world
                .set(21, 4, 20, state(block.unwrap_or("minecraft:air")))
                .unwrap();
            let mut item = falling_item(20.5, 4.0, 20.5, 0.4, 0.0, 0.0);
            item.id = 1;
            for _ in 0..12 {
                item.tick(
                    &world,
                    assets.registry,
                    &facts,
                    &collision,
                    false,
                    &fluids,
                    &[],
                    None,
                    None,
                );
            }
            item.position.x
        };
        let free = slide(None);
        let web = slide(Some("minecraft:cobweb"));
        let snow = slide(Some("minecraft:powder_snow"));
        assert!(free > 21.4, "an unobstructed item slides on to {free}");
        assert!(
            web > 20.8 && web < free - 0.3,
            "the web holds it: {web} vs {free}"
        );
        assert!(snow < free, "powder snow slows it: {snow} vs {free}");
    }

    #[test]
    fn obstacle_grid_returns_only_the_boxes_near_an_item() {
        // A boat every 100 blocks along a line, 1000 of them.
        let boat = |x: f64| {
            Aabb::new(
                Vec3::new(x, 64.0, 0.0),
                Vec3::new(x + 1.375, 64.5625, 1.375),
            )
        };
        let mut grid = ObstacleGrid::new((0..1000).map(|i| boat(f64::from(i) * 100.0)).collect());
        let mut out = Vec::new();
        // Around the 7th boat: only it.
        let area = Aabb::new(Vec3::new(699.0, 63.0, -1.0), Vec3::new(701.0, 66.0, 1.0));
        grid.near(area, &mut out);
        assert_eq!(out, vec![boat(700.0)]);
        // Between two boats: none.
        let area = Aabb::new(Vec3::new(750.0, 63.0, 0.0), Vec3::new(751.0, 66.0, 1.0));
        grid.near(area, &mut out);
        assert!(out.is_empty());
        // A box spanning cells is found from any of them, once.
        let wide = Aabb::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(20.0, 1.0, 1.0));
        let mut grid = ObstacleGrid::new(vec![wide]);
        let area = Aabb::new(Vec3::new(17.0, 0.0, 0.0), Vec3::new(18.0, 1.0, 1.0));
        grid.near(area, &mut out);
        assert_eq!(out, vec![wide]);
    }

    #[test]
    fn obstacle_grid_leaves_out_a_movers_riders_and_answers_a_huge_area_at_once() {
        let unit = |x: f64| Aabb::new(Vec3::new(x, 64.0, 0.0), Vec3::new(x + 1.0, 65.0, 1.0));
        // The shulker at x = 2 rides stand 7; the one at x = 0 and the boat
        // at x = 4 ride nothing.
        let mut grid = ObstacleGrid::with_roots(
            vec![unit(0.0), unit(2.0), unit(4.0)],
            vec![None, Some(7), None],
        );
        let mut out = Vec::new();
        let area = Aabb::new(Vec3::new(-1.0, 63.0, -1.0), Vec3::new(6.0, 66.0, 2.0));
        grid.near_except(area, Some(7), &mut out);
        assert_eq!(out, vec![unit(0.0), unit(4.0)], "stand 7 ignores its rider");
        grid.near_except(area, Some(8), &mut out);
        assert_eq!(
            out,
            vec![unit(0.0), unit(2.0), unit(4.0)],
            "stand 8 does not"
        );
        // A stand loaded with Motion 1e10 asks for an area 1e10 blocks long
        // (about 1.25e9 cells): the boxes in it come back in order, at once.
        let huge = Aabb::new(Vec3::new(-1.0, 63.0, -1.0), Vec3::new(1.0e10, 66.0, 2.0));
        let start = std::time::Instant::now();
        grid.near(huge, &mut out);
        assert_eq!(out, vec![unit(0.0), unit(2.0), unit(4.0)]);
        let far = Aabb::new(
            Vec3::new(-1.0e300, 63.0, -1.0),
            Vec3::new(1.0e300, 66.0, 2.0),
        );
        grid.near(far, &mut out);
        assert_eq!(out.len(), 3);
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
    }

    /// A farm: 5000 drops and 1000 separated vehicles. Run with `-- --ignored --nocapture`.
    #[test]
    #[ignore = "timing"]
    fn obstacle_grid_beats_scanning_every_box_for_a_big_farm() {
        let boat = |i: u32| {
            let (x, z) = (f64::from(i % 40) * 6.0, f64::from(i / 40) * 6.0);
            Aabb::new(
                Vec3::new(x, 64.0, z),
                Vec3::new(x + 1.375, 64.5625, z + 1.375),
            )
        };
        let boxes: Vec<Aabb> = (0..1000).map(boat).collect();
        let items: Vec<Aabb> = (0..5000)
            .map(|i| {
                let (x, z) = (f64::from(i % 70) * 3.4, f64::from(i / 70) * 3.4);
                Aabb::new(Vec3::new(x, 64.0, z), Vec3::new(x + 0.25, 64.25, z + 0.25))
            })
            .collect();
        let start = std::time::Instant::now();
        let mut hits = 0;
        for item in &items {
            let area = item.inflate(Vec3::new(1.0, 1.0, 1.0));
            hits += boxes.iter().filter(|b| b.intersects(area)).count();
        }
        let scan = start.elapsed();
        let mut grid = ObstacleGrid::new(boxes);
        let start = std::time::Instant::now();
        let (mut out, mut found) = (Vec::new(), 0);
        for item in &items {
            grid.near(item.inflate(Vec3::new(1.0, 1.0, 1.0)), &mut out);
            found += out.len();
        }
        let binned = start.elapsed();
        assert_eq!(hits, found);
        println!("scan {scan:?}, grid {binned:?}");
    }
}

#[cfg(test)]
mod armor_stand_motion_tests {
    use super::*;

    #[test]
    fn honey_slide_runs_after_stand_travel_and_preserves_no_physics_controls() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("movement-properties.nbt").exists() {
            return;
        }
        let assets = crate::assets::Assets::load(&root).unwrap();
        let facts = BlockFacts::load(&root, assets.registry).unwrap();
        let collision = crate::collisions::load(&root, assets.registry).unwrap();
        let motion = crate::motion::MotionResources::load(&root, &assets).unwrap();
        let mut world = World::new(assets.registry.default_state("minecraft:air").unwrap(), 3);
        world.chunks.insert(
            (0, 0),
            std::sync::Arc::new(crate::chunk::Chunk::empty(0, 0)),
        );
        for (z, name) in [(4, "minecraft:honey_block"), (10, "minecraft:stone")] {
            for y in 100..=120 {
                world
                    .set(6, y, z, assets.registry.default_state(name).unwrap())
                    .unwrap();
            }
        }
        // Independently captured Java values after five actual ticks.
        for (z, no_gravity, marker, y, vy, grounded) in [
            (
                4.5,
                false,
                false,
                117.51156798899842,
                -0.12740000247955321,
                false,
            ),
            (10.5, false, false, 118.0, -0.0784000015258789, true),
            (4.5, true, false, 118.0, 0.0, false),
            (4.5, false, true, 118.0, 0.0, false),
        ] {
            let mut stand = ArmorStand::new(1, Vec3::new(5.78, 118.0, z), 0.0);
            // Match the freshly summoned entity rather than the placement constructor.
            stand.on_ground = false;
            stand.no_gravity = no_gravity;
            stand.marker = marker;
            let mut scene = SceneCollisions {
                world: &world,
                binding: &collision,
                subject: None,
            };
            for _ in 0..5 {
                stand.tick(&mut scene, &facts, &motion.climbing, false, None);
            }
            assert_eq!(
                stand.position,
                Vec3::new(5.78, y, z),
                "NoGravity={no_gravity}, Marker={marker}"
            );
            assert_eq!(stand.velocity, Vec3::new(0.0, vy, 0.0));
            assert_eq!(stand.on_ground, grounded);
        }
    }

    #[test]
    fn a_name_tag_names_a_live_stand_and_unnamed_tags_pass() {
        let mut stand = ArmorStand::new(1, Vec3::ZERO, 0.0);
        let mut tag = ItemStack::new("minecraft:name_tag", 4);
        assert!(!stand.name_with_tag(&tag));
        assert!(stand.custom_name.is_none());
        let name = horizon_world::nbt::Tag::compound([
            ("text", horizon_world::nbt::Tag::String("Bob".into())),
            ("color", horizon_world::nbt::Tag::String("gold".into())),
        ]);
        tag.data_mut().custom_name = Some(name.clone());
        assert!(stand.name_with_tag(&tag));
        assert_eq!(stand.custom_name, Some(crate::component::name_text(&name)));
        assert_eq!(stand.health, 20.0);
        assert!(!stand.name_with_tag(&ItemStack::new("minecraft:name_tag", 3)));
        stand.health = 0.0;
        tag.data_mut().custom_name = Some(horizon_world::nbt::Tag::String("Other".into()));
        assert!(
            stand.name_with_tag(&tag),
            "a dead target still handles the named tag"
        );
        assert_eq!(stand.custom_name, Some(crate::component::name_text(&name)));
    }

    #[test]
    fn a_new_stand_starts_with_matching_previous() {
        let at = Vec3::new(1.0, 2.0, 3.0);
        let stand = ArmorStand::new(1, at, 90.0);
        assert_eq!(stand.previous, at);
        assert_eq!(stand.position, at);
    }

    fn tick_stand(
        stand: &mut ArmorStand,
        world: &World,
        facts: &BlockFacts,
        collision: &BlockCollisionRegistry<'_>,
    ) {
        let mut scene = SceneCollisions {
            world,
            binding: collision,
            subject: None,
        };
        let empty = Default::default();
        let climbing = horizon_world::climbing::ClimbingRegistry::new(
            collision.registry(),
            &empty,
            &empty,
            &empty,
        )
        .unwrap();
        stand.tick(&mut scene, facts, &climbing, false, None);
    }

    #[test]
    fn climbing_uses_actual_blocks_and_post_collision_position() {
        let root = std::env::var_os("HORIZON_TEST_RESOURCES")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../.local/horizon-client")
                    .join(horizon_core::VANILLA_VERSION)
            });
        if !root.join("movement-properties.nbt").exists() {
            return;
        }
        let assets = crate::assets::Assets::load(&root).unwrap();
        let facts = BlockFacts::load(&root, assets.registry).unwrap();
        let collision = crate::collisions::load(&root, assets.registry).unwrap();
        let motion = crate::motion::MotionResources::load(&root, &assets).unwrap();
        let registry = assets.registry;
        let state = |name, properties: &[(&str, &str)]| {
            let mut state = registry
                .state(registry.default_state(name).unwrap())
                .unwrap()
                .clone();
            for (key, value) in properties {
                state.properties.insert((*key).into(), (*value).into());
            }
            registry.id(&state).unwrap()
        };
        let new_world = || {
            let mut world = World::new(registry.default_state("minecraft:air").unwrap(), 3);
            world.chunks.insert(
                (1, 1),
                std::sync::Arc::new(crate::chunk::Chunk::empty(1, 1)),
            );
            world
        };
        for (name, properties, ladder_below, climbable) in [
            ("minecraft:air", vec![], false, false),
            ("minecraft:ladder", vec![("facing", "west")], false, true),
            ("minecraft:vine", vec![("east", "true")], false, true),
            ("minecraft:twisting_vines_plant", vec![], false, true),
            ("minecraft:weeping_vines_plant", vec![], false, true),
            (
                "minecraft:oak_trapdoor",
                vec![("open", "true"), ("facing", "west")],
                true,
                true,
            ),
            (
                "minecraft:oak_trapdoor",
                vec![("open", "true"), ("facing", "east")],
                true,
                false,
            ),
        ] {
            let mut world = new_world();
            world.set(20, 10, 20, state(name, &properties)).unwrap();
            if ladder_below {
                world
                    .set(20, 9, 20, state("minecraft:ladder", &[("facing", "west")]))
                    .unwrap();
            }
            let mut stand = ArmorStand::new(1, Vec3::new(20.5, 10.0, 20.5), 0.0);
            stand.velocity.y = -0.5;
            let mut scene = SceneCollisions {
                world: &world,
                binding: &collision,
                subject: None,
            };
            stand.tick(&mut scene, &facts, &motion.climbing, false, None);
            let fall = if climbable { f64::from(0.15f32) } else { 0.5 };
            assert!(
                (stand.position.y - (10.0 - fall)).abs() < 1e-9,
                "{name} {properties:?}"
            );
        }
        // A wall collision inside a ladder lifts before gravity and drag.
        let mut world = new_world();
        world
            .set(20, 10, 20, state("minecraft:ladder", &[("facing", "west")]))
            .unwrap();
        for y in 10..13 {
            world.set(21, y, 20, state("minecraft:stone", &[])).unwrap();
        }
        let mut stand = ArmorStand::new(1, Vec3::new(20.74, 10.0, 20.5), 0.0);
        stand.velocity = Vec3::new(1.0, 0.0, 0.0);
        let mut scene = SceneCollisions {
            world: &world,
            binding: &collision,
            subject: None,
        };
        stand.tick(&mut scene, &facts, &motion.climbing, false, None);
        assert!((stand.velocity.y - (0.2 - 0.08) * f64::from(0.98f32)).abs() < 1e-9);
    }

    /// `Entity.collectColliders` (#7080): the world border's wall stops an
    /// armor stand as it stops the player. A border of size 10 around
    /// (20, 20) has its east wall at x = 25; the stand starts at x = 24 on the
    /// test scene's flat ground, moving 1.5 east.
    #[test]
    fn the_world_border_stops_an_armor_stand() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("movement-properties.nbt").exists() {
            return;
        }
        let assets = crate::assets::Assets::load(&root).unwrap();
        let render = assets.reload(&root).unwrap();
        let facts = BlockFacts::load(&root, assets.registry).unwrap();
        let collision = crate::collisions::load(&root, assets.registry).unwrap();
        let world = World::test_scene(&assets, &render, 3).unwrap();
        let border = BorderGeometry::new([20.0, 20.0], 10.0, 29_999_984).unwrap();
        for bordered in [false, true] {
            let mut stand = ArmorStand::new(1, Vec3::new(24.0, 4.0, 20.5), 0.0);
            stand.on_ground = true;
            stand.velocity = Vec3::new(1.5, 0.0, 0.0);
            let mut scene = SceneCollisions {
                world: &world,
                binding: &collision,
                subject: None,
            };
            let empty = Default::default();
            let climbing = horizon_world::climbing::ClimbingRegistry::new(
                collision.registry(),
                &empty,
                &empty,
                &empty,
            )
            .unwrap();
            stand.tick(
                &mut scene,
                &facts,
                &climbing,
                false,
                bordered.then_some(&border),
            );
            let east = stand.bounds().max_position().x;
            if bordered {
                assert!(
                    (east - 25.0).abs() < 1.0e-7,
                    "stops on the wall, got {east}"
                );
            } else {
                assert!(east > 25.0, "no border: goes on, got {east}");
            }
        }
    }

    /// `Entity.collide` (#11877): the boxes `getEntityCollisions` gives a
    /// stand (a shulker's here, one block on the test scene's ground, whose
    /// top is at y = 4) stop it as blocks do. Dropped from y = 8 it rests on
    /// the box's top at y = 5; sliding east at 2 blocks a tick from x = 18 it
    /// stops with its east side on the box's west face, x = 20. With no boxes
    /// it falls to the ground and slides on.
    #[test]
    fn a_stand_lands_on_and_stops_at_the_entity_boxes_it_is_given() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("movement-properties.nbt").exists() {
            return;
        }
        let assets = crate::assets::Assets::load(&root).unwrap();
        let render = assets.reload(&root).unwrap();
        let facts = BlockFacts::load(&root, assets.registry).unwrap();
        let collision = crate::collisions::load(&root, assets.registry).unwrap();
        let world = World::test_scene(&assets, &render, 3).unwrap();
        let empty = Default::default();
        let climbing = horizon_world::climbing::ClimbingRegistry::new(
            collision.registry(),
            &empty,
            &empty,
            &empty,
        )
        .unwrap();
        let shulker = Aabb::new(Vec3::new(20.0, 4.0, 20.0), Vec3::new(21.0, 5.0, 21.0));
        let run = |stand: &mut ArmorStand, boxes: &[Aabb], ticks: usize| {
            for _ in 0..ticks {
                let mut scene = SceneCollisions {
                    world: &world,
                    binding: &collision,
                    subject: None,
                };
                if let Some(step) =
                    stand.tick_move(&mut scene, &facts, &climbing, false, None, boxes)
                {
                    stand.tick_settle(StandMedium::Air, step);
                }
            }
        };
        for (boxes, floor) in [(&[shulker][..], 5.0), (&[][..], 4.0)] {
            let mut stand = ArmorStand::new(1, Vec3::new(20.5, 8.0, 20.5), 0.0);
            run(&mut stand, boxes, 20);
            let y = stand.position.y;
            assert!((y - floor).abs() < 1.0e-9, "rests at {floor}, got {y}");
            assert!(stand.on_ground, "on the ground at {floor}");
        }
        for (boxes, blocked) in [(&[shulker][..], true), (&[][..], false)] {
            let mut stand = ArmorStand::new(1, Vec3::new(18.0, 4.0, 20.5), 0.0);
            stand.on_ground = true;
            stand.velocity = Vec3::new(2.0, 0.0, 0.0);
            run(&mut stand, boxes, 1);
            let east = stand.bounds().max_position().x;
            if blocked {
                assert!((east - 20.0).abs() < 1.0e-7, "stops at the box, got {east}");
                assert_eq!(stand.velocity.x, 0.0, "the wall takes its speed");
            } else {
                assert!((east - 20.25).abs() < 1.0e-7, "goes on, got {east}");
            }
        }
    }

    #[test]
    fn a_stand_in_lava_halves_its_motion_and_sinks_by_a_quarter_of_gravity() {
        // LivingEntity.travelInLava, measured on the real server: a stand at
        // rest in deep lava has Motion.y -0.02, then -0.03, then -0.035.
        let step = StandStep {
            falling: true,
            friction: 1.0,
        };
        let mut stand = ArmorStand::new(1, Vec3::new(0.5, 100.0, 0.5), 0.0);
        let mut seen = Vec::new();
        for _ in 0..3 {
            stand.tick_settle(StandMedium::Lava { shallow: false }, step);
            seen.push(stand.velocity.y);
        }
        for (got, want) in seen.iter().zip([-0.02, -0.03, -0.035]) {
            assert!((got - want).abs() < 1e-12, "got {got}, want {want}");
        }
        // Shallow lava also applies the 0.8 and the gravity / 16 sink.
        let mut shallow = ArmorStand::new(1, Vec3::new(0.5, 100.0, 0.5), 0.0);
        shallow.velocity = Vec3::new(0.2, -0.1, 0.0);
        shallow.tick_settle(StandMedium::Lava { shallow: true }, step);
        assert!((shallow.velocity.x - 0.1).abs() < 1e-12);
        let want = -0.1 * f64::from(0.8f32) - 0.08 / 16.0 - 0.08 / 4.0;
        assert!((shallow.velocity.y - want).abs() < 1e-12);
        // Air is unchanged: gravity, then the 0.98 drag.
        let mut air = ArmorStand::new(1, Vec3::new(0.5, 100.0, 0.5), 0.0);
        air.tick_settle(StandMedium::Air, step);
        assert!((air.velocity.y + 0.0784).abs() < 1e-6);
    }

    #[test]
    fn lava_depth_picks_the_shallow_or_deep_travel() {
        assert_eq!(StandMedium::from_heights(0.0, 0.0), StandMedium::Air);
        assert_eq!(StandMedium::from_heights(0.3, 1.0), StandMedium::Water);
        assert_eq!(
            StandMedium::from_heights(0.0, 0.4),
            StandMedium::Lava { shallow: true }
        );
        assert_eq!(
            StandMedium::from_heights(0.0, 1.975),
            StandMedium::Lava { shallow: false }
        );
    }

    #[test]
    fn a_tick_keeps_the_previous_position_for_interpolation() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("movement-properties.nbt").exists() {
            return;
        }
        let assets = crate::assets::Assets::load(&root).unwrap();
        let render = assets.reload(&root).unwrap();
        let collision = crate::collisions::load(&root, assets.registry).unwrap();
        let facts = BlockFacts::load(&root, assets.registry).unwrap();
        let world = World::test_scene(&assets, &render, 3).unwrap();
        let start = Vec3::new(20.5, 20.0, 20.5);
        let mut stand = ArmorStand::new(1, start, 0.0);
        stand.on_ground = false;
        // A summoned stand has no motion yet: the first tick only builds it
        // (vanilla moves by the velocity, then applies gravity), so it falls
        // from the second tick on (measured on the real server: drop 0, 0, 0.0784 blocks).
        tick_stand(&mut stand, &world, &facts, &collision);
        assert_eq!(stand.position, start);
        assert!((stand.velocity.y + 0.0784).abs() < 1e-6);
        tick_stand(&mut stand, &world, &facts, &collision);
        assert_eq!(stand.previous, start);
        assert!(
            stand.position.y < start.y,
            "an airborne stand must fall, y={}",
            stand.position.y
        );
        let mid = stand.previous.y + (stand.position.y - stand.previous.y) * 0.5;
        assert!(
            mid < start.y && mid > stand.position.y,
            "alpha 0.5 must sit between previous and current"
        );
    }

    #[test]
    fn a_no_gravity_stand_stays_put() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("movement-properties.nbt").exists() {
            return;
        }
        let assets = crate::assets::Assets::load(&root).unwrap();
        let render = assets.reload(&root).unwrap();
        let collision = crate::collisions::load(&root, assets.registry).unwrap();
        let facts = BlockFacts::load(&root, assets.registry).unwrap();
        let world = World::test_scene(&assets, &render, 3).unwrap();
        let start = Vec3::new(20.5, 20.0, 20.5);
        let mut stand = ArmorStand::new(1, start, 0.0);
        stand.on_ground = false;
        stand.no_gravity = true;
        // ArmorStand.travel does nothing without physics: measured on the real
        // server, Pos and Motion stay exactly as summoned for every tick.
        for _ in 0..5 {
            tick_stand(&mut stand, &world, &facts, &collision);
            assert_eq!(stand.position, start);
            assert_eq!(stand.velocity, Vec3::ZERO);
        }
        stand.no_gravity = false;
        tick_stand(&mut stand, &world, &facts, &collision);
        tick_stand(&mut stand, &world, &facts, &collision);
        assert!(stand.position.y < start.y, "gravity again once cleared");
    }

    #[test]
    fn leather_boots_walk_on_powder_snow() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("movement-properties.nbt").exists() {
            return;
        }
        let assets = crate::assets::Assets::load(&root).unwrap();
        let render = assets.reload(&root).unwrap();
        let collision = crate::collisions::load(&root, assets.registry).unwrap();
        let facts = BlockFacts::load(&root, assets.registry).unwrap();
        let mut world = World::test_scene(&assets, &render, 3).unwrap();
        let snow = assets
            .registry
            .default_state("minecraft:powder_snow")
            .unwrap();
        for x in 19..=21 {
            for z in 19..=21 {
                world.set(x, 10, z, snow).unwrap();
            }
        }
        let start = Vec3::new(20.5, 11.0, 20.5);
        let mut bare = ArmorStand::new(1, start, 0.0);
        bare.on_ground = true;
        for _ in 0..8 {
            tick_stand(&mut bare, &world, &facts, &collision);
        }
        assert!(
            bare.position.y < start.y - 0.2,
            "bare feet must sink into powder snow, y={}",
            bare.position.y
        );

        let mut booted = ArmorStand::new(2, start, 0.0);
        booted.on_ground = true;
        booted.equipment[2] = Some(ItemStack::new("minecraft:leather_boots", 1));
        for _ in 0..8 {
            tick_stand(&mut booted, &world, &facts, &collision);
        }
        assert!(
            (booted.position.y - start.y).abs() < 1.0e-6,
            "leather boots must walk on powder snow, y={}",
            booted.position.y
        );
    }
}

#[cfg(test)]
mod item_destroyed_tests {
    use super::*;

    fn item(id: &str, contents: &[(&str, u32)]) -> ItemEntity {
        let mut stack = ItemStack::new(id, 1);
        stack.data_mut().bundle_contents = contents
            .iter()
            .map(|(id, count)| ItemStack::new(id, *count))
            .collect();
        ItemEntity {
            random: horizon_core::random::LegacyRandom::new(0),
            rotation: [0.0, 0.0],
            join: Default::default(),
            no_gravity: false,
            id: 1,
            thrower: None,
            target: None,
            stack,
            health: 5,
            fire_ticks: 0,
            position: Vec3::new(8.5, 64.0, 8.5),
            previous: Vec3::new(8.5, 64.0, 8.5),
            velocity: Vec3::ZERO,
            on_ground: false,
            vertical_collision: false,
            horizontal_collision: false,
            fall_distance: 0.0,
            age: 0,
            pickup_delay: 0,
            bob_offset: 0.0,
            ticks: 0,
            needs_sync: false,
            stuck: Vec3::ZERO,
            was_touching_water: false,
        }
    }

    /// `BundleItem.onDestroyed` (`ItemStack.onDestroyed` → `dropContents`):
    /// the damage that empties an item's health (`ItemEntity.hurtServer`,
    /// which `environment_tick` calls for fire, lava and cactus) hands back
    /// the bundle's stacks, and the bundle keeps none of them.
    #[test]
    fn a_destroyed_bundle_spills_its_contents() {
        let mut bundle = item(
            "minecraft:bundle",
            &[("minecraft:stone", 3), ("minecraft:diamond", 1)],
        );
        // Above zero after the hit: nothing leaves the bundle.
        assert_eq!(bundle.hurt(1), None);
        assert_eq!(bundle.health, 4);
        let spent = bundle.hurt(4).expect("the hit that destroys it");
        assert_eq!(
            spent
                .iter()
                .map(|s| (&*s.item, s.count))
                .collect::<Vec<_>>(),
            [("minecraft:stone", 3), ("minecraft:diamond", 1)]
        );
        assert!(
            bundle
                .stack
                .data()
                .is_none_or(|d| d.bundle_contents.is_empty()),
            "the destroyed bundle keeps nothing inside"
        );
        // A second hit has nothing left to spill, and an item that holds
        // nothing never reports any.
        assert_eq!(bundle.hurt(1), Some(Vec::new()));
        assert_eq!(item("minecraft:stone", &[]).hurt(5), Some(Vec::new()));
    }

    /// `BlockItem.onDestroyed`: the same death path spills a container item's
    /// `container` — a shulker box's stacks — in slot order, and the box keeps
    /// no slot filled.
    #[test]
    fn a_destroyed_shulker_box_spills_its_container() {
        let mut box_item = item("minecraft:shulker_box", &[]);
        box_item.stack.data_mut().container = vec![
            (20, ItemStack::new("minecraft:diamond", 4)),
            (0, ItemStack::new("minecraft:oak_log", 8)),
            (3, ItemStack::new("minecraft:cobblestone", 64)),
        ];
        let spent = box_item.hurt(5).expect("the hit that destroys it");
        assert_eq!(
            spent
                .iter()
                .map(|s| (&*s.item, s.count))
                .collect::<Vec<_>>(),
            [
                ("minecraft:oak_log", 8),
                ("minecraft:cobblestone", 64),
                ("minecraft:diamond", 4),
            ]
        );
        assert!(
            box_item.stack.data().is_none_or(|d| d.container.is_empty()),
            "the destroyed box keeps no slot filled"
        );
    }
}
#[cfg(test)]
mod orb_underwater_tests {
    use super::*;

    #[test]
    fn matches_the_executed_java_private_method() {
        // Actual 26.3 setUnderwaterMovement calls on server-level orbs,
        // captured by test_orb_water_parity.py (orb-water-reference.json).
        // Cover both horizontal signs, falling/rising, and the vertical cap.
        let cases: [([f64; 3], [u64; 3]); 6] = [
            (
                [0.0, 0.0, 0.0],
                [0x0000000000000000, 0x3f40624de0000000, 0x0000000000000000],
            ),
            (
                [0.1, 0.01, -0.2],
                [0x3fb9581066666667, 0x3f85810625ae147b, 0xbfc9581066666667],
            ),
            (
                [-0.25, -0.03, 0.125],
                [0xbfcfae1480000000, 0xbf9e353f7c851eb8, 0x3fbfae1480000000],
            ),
            (
                [0.0, 0.059, 0.0],
                [0x0000000000000000, 0x3fae76c8b46d9168, 0x0000000000000000],
            ),
            (
                [0.5, 0.0595, -0.5],
                [0x3fdfae1480000000, 0x3faeb851e0000000, 0xbfdfae1480000000],
            ),
            (
                [-1.0, 0.2, 1.0],
                [0xbfefae1480000000, 0x3faeb851e0000000, 0x3fefae1480000000],
            ),
        ];
        let actual: Vec<_> = cases
            .iter()
            .map(|(v, _)| {
                let mut orb = ExperienceOrb {
                    rotation: [0.0, 0.0],
                    join: Default::default(),
                    no_gravity: false,
                    id: 1,
                    position: Vec3::ZERO,
                    previous: Vec3::ZERO,
                    velocity: Vec3::new(v[0], v[1], v[2]),
                    value: 1,
                    age: 0,
                    on_ground: false,
                    count: 1,
                    ticks: 0,
                    health: 5,
                    air: 300,
                    following: None,
                };
                orb.set_underwater_movement();
                [
                    orb.velocity.x.to_bits(),
                    orb.velocity.y.to_bits(),
                    orb.velocity.z.to_bits(),
                ]
            })
            .collect();
        let expected: Vec<_> = cases.into_iter().map(|(_, bits)| bits).collect();
        assert_eq!(actual, expected);
    }
}

#[cfg(test)]
mod render_light_tests {
    use super::*;

    fn orb(y: f64) -> ExperienceOrb {
        ExperienceOrb {
            rotation: [0.0, 0.0],
            join: Default::default(),
            no_gravity: false,
            id: 1,
            position: Vec3::new(0.5, y, 0.5),
            previous: Vec3::new(0.5, y, 0.5),
            velocity: Vec3::new(0.0, 0.0, 0.0),
            value: 3,
            age: 0,
            on_ground: false,
            count: 1,
            ticks: 0,
            health: 5,
            air: 300,
            following: None,
        }
    }

    /// `ExperienceOrbRenderer.getBlockLightLevel`: the level's block light
    /// plus 7, clamped to 15; the sky light is the level's own. The orb used
    /// to be drawn at 10 in both nibbles, so an orb in a dark cave was washed
    /// out and one in daylight at block light 5 was too dim.
    #[test]
    fn an_orb_is_seven_brighter_than_the_block_it_sits_in() {
        let light = |sky: i32, block: i32| (sky << 4) | block;
        // Sky 15, block 0 (daylight): sky stays 15, block goes to 7.
        assert_eq!(
            orb(64.0).render_light(light(15, 0)),
            (15 * 16) | ((7 * 16) << 8)
        );
        // A dark cave: sky 0, block 0 -> block 7, and no invented sky light.
        assert_eq!(orb(20.0).render_light(light(0, 0)), (7 * 16) << 8);
        // Torchlight 9 -> 15, the clamp, not 16.
        assert_eq!(orb(20.0).render_light(light(0, 9)), (15 * 16) << 8);
        assert_eq!(
            orb(20.0).render_light(light(15, 15)),
            (15 * 16) | ((15 * 16) << 8)
        );
    }

    /// `Entity.getLightProbePosition` is the eye position: an orb's box is
    /// half a block, so it reads the light of the cell 0.425 above its feet.
    #[test]
    fn an_orb_is_lit_from_the_block_its_eye_is_in() {
        assert_eq!(orb(64.6).light_probe_y().floor(), 65.0);
        assert_eq!(orb(64.6).position.y.floor(), 64.0);
        assert!((orb(64.0).light_probe_y() - 64.425).abs() < 1e-9);
    }

    /// `ExperienceOrbRenderer.submit`'s quad: a block tall about the orb's
    /// centre, scaled by 0.3, so it spans -0.075 to +0.225 above the 0.1 the
    /// pose is translated by. Horizon drew a symmetric 0.3 square, a quarter
    /// of the sprite too low.
    #[test]
    fn the_orb_sprite_hangs_a_quarter_below_its_centre() {
        let off = ORB_QUAD.map(|[x, y]| [x * ORB_QUAD_SCALE, 0.1 + y * ORB_QUAD_SCALE]);
        let want = [
            [-0.15, 0.1 - 0.075],
            [0.15, 0.1 - 0.075],
            [0.15, 0.1 + 0.225],
            [-0.15, 0.1 + 0.225],
        ];
        for (got, want) in off.into_iter().zip(want) {
            assert!((got[0] - want[0]).abs() < 1e-6, "{got:?} vs {want:?}");
            assert!((got[1] - want[1]).abs() < 1e-6, "{got:?} vs {want:?}");
        }
    }
}

#[cfg(test)]
mod dropped_item_light_tests {
    use super::*;

    fn item(previous_y: f64, y: f64) -> ItemEntity {
        ItemEntity {
            random: horizon_core::random::LegacyRandom::new(0),
            rotation: [0.0, 0.0],
            join: Default::default(),
            no_gravity: false,
            id: 1,
            thrower: None,
            target: None,
            stack: ItemStack {
                item: std::sync::Arc::from("minecraft:stone"),
                count: 1,
                damage: 0,
                extra: None,
            },
            health: 5,
            fire_ticks: 0,
            position: Vec3::new(0.5, y, 0.5),
            previous: Vec3::new(0.5, previous_y, 0.5),
            velocity: Vec3::ZERO,
            on_ground: false,
            vertical_collision: false,
            horizontal_collision: false,
            fall_distance: 0.0,
            age: 0,
            pickup_delay: 0,
            bob_offset: 0.0,
            ticks: 0,
            needs_sync: false,
            stuck: Vec3::ZERO,
            was_touching_water: false,
        }
    }

    /// `Entity.getLightProbePosition` is the eye position, and `EntityType.ITEM`
    /// declares `eyeHeight(0.2125)`. The item used to be lit from the cell of
    /// `base + 0.1`, so a falling item at y = 64.85 read the cell at 64 (block
    /// light of the floor) while its eye at 65.0625 is in the cell at 65.
    #[test]
    fn a_dropped_item_is_lit_from_the_block_its_eye_is_in() {
        let falling = item(64.85, 64.85);
        let probe = falling.light_probe(1.0);
        assert!((probe[1] - 65.062_5).abs() < 1e-9);
        assert_eq!(probe[1].floor(), 65.0);
        // The old probe, `(y + 0.1).floor()`, and the base cell itself.
        assert_eq!((64.85_f64 + 0.1).floor(), 64.0);
        assert_eq!(64.85_f64.floor(), 64.0);
        // Resting on a block top the two agree: the eye is 0.2125 up.
        let resting = item(64.0, 64.0);
        assert_eq!(resting.light_probe(1.0)[1].floor(), 64.0);
        assert!((resting.light_probe(1.0)[1] - 64.212_5).abs() < 1e-9);
    }

    /// `Entity.getLightProbePosition(partialTick)` follows `getPosition(partialTick)`:
    /// an item falling a block in one tick is probed halfway between the two
    /// positions in the middle of the frame. The old code read the tick's own
    /// position, so the light jumped a frame early.
    #[test]
    fn an_item_probe_follows_the_frame() {
        let falling = item(64.0, 65.0);
        let mid = falling.light_probe(0.5);
        assert!((mid[1] - 64.712_5).abs() < 1e-9);
        assert_eq!(mid[1].floor(), 64.0);
        assert_eq!(falling.light_probe(1.0)[1].floor(), 65.0);
        // Across x and z too, not just down.
        let sliding = ItemEntity {
            random: horizon_core::random::LegacyRandom::new(0),
            position: Vec3::new(1.5, 64.0, 0.5),
            previous: Vec3::new(0.5, 64.0, 0.5),
            ..item(64.0, 64.0)
        };
        assert!((sliding.light_probe(0.5)[0] - 1.0).abs() < 1e-9);
    }

    /// `EntityRenderer.getBlockLightLevel` is `isOnFire() ? 15 : the level's
    /// block light`, and the sky light is never overridden. A dropped item was
    /// drawn at the level's block light while it burned.
    #[test]
    fn a_burning_item_is_drawn_at_full_block_light() {
        let light = |sky: u8, block: u8| (sky << 4) | block;
        let cold = item(64.0, 64.0);
        assert_eq!(cold.render_light(light(15, 0)), (15 * 16) as u32);
        assert_eq!(cold.render_light(light(0, 9)), ((9 * 16) << 8) as u32);
        let burning = ItemEntity {
            random: horizon_core::random::LegacyRandom::new(0),
            fire_ticks: 160,
            ..item(64.0, 64.0)
        };
        // Daylight, dark block: the block light goes to 15, the sky stays.
        assert_eq!(
            burning.render_light(light(15, 0)),
            ((15 * 16) | ((15 * 16) << 8)) as u32
        );
        // A dark cave: 15 block light, still no invented sky light.
        assert_eq!(burning.render_light(light(0, 0)), ((15 * 16) << 8) as u32);
    }
}
crate::impl_joined!(ItemEntity, ExperienceOrb, ArmorStand);
#[cfg(test)]
mod orb_lava_tests {
    use super::*;

    fn with_fixture(
        mut f: impl FnMut(
            &mut World,
            &BlockFacts,
            &BlockCollisionRegistry<'_>,
            &crate::motion::FluidResources,
            &crate::assets::Assets,
        ),
    ) {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("movement-properties.nbt").exists() {
            eprintln!("orb lava fixture unavailable: prepared resources missing");
            return;
        }
        let assets = crate::assets::Assets::load(&root).unwrap();
        let render = assets.reload(&root).unwrap();
        let facts = BlockFacts::load(&root, assets.registry).unwrap();
        let fluids = crate::motion::FluidResources::new(&assets, &facts).unwrap();
        let collision = crate::collisions::load(&root, assets.registry).unwrap();
        let mut world = World::test_scene(&assets, &render, 3).unwrap();
        f(&mut world, &facts, &collision, &fluids, &assets);
    }

    fn sample_orb(at: Vec3, health: i32) -> ExperienceOrb {
        ExperienceOrb {
            rotation: [0.0, 0.0],
            join: Default::default(),
            no_gravity: false,
            id: 1,
            position: at,
            previous: at,
            velocity: Vec3::ZERO,
            value: 1,
            age: 0,
            on_ground: false,
            count: 1,
            ticks: 0,
            health,
            air: 300,
            following: None,
        }
    }

    #[test]
    fn a_server_orb_takes_lava_damage_after_moving_and_discards_at_zero() {
        with_fixture(|world, facts, collision, fluids, assets| {
            let lava = assets.registry.default_state("minecraft:lava").unwrap();
            world.set(20, 4, 20, lava).unwrap();
            let at = Vec3::new(20.5, 4.1, 20.5);
            let mut orb = sample_orb(at, 5);
            let mut client = orb.clone();
            for expected_age in 1..=2 {
                assert!(client.tick(
                    world,
                    facts,
                    collision,
                    None,
                    false,
                    &mut || 0.0,
                    None,
                    fluids,
                    &mut |_, _| {}
                ));
                assert_eq!((client.health, client.age), (5, expected_age));
            }
            let mut sounds = Vec::new();
            assert!(orb.tick(
                world,
                facts,
                collision,
                None,
                true,
                &mut || 0.0,
                None,
                fluids,
                &mut |pos, pitch| sounds.push((pos, pitch)),
            ));
            assert_eq!((orb.health, orb.age), (1, 1));
            assert_eq!(sounds.len(), 1);
            sounds.clear();
            assert!(!orb.tick(
                world,
                facts,
                collision,
                None,
                true,
                &mut || 0.0,
                None,
                fluids,
                &mut |pos, pitch| sounds.push((pos, pitch)),
            ));
            assert_eq!((orb.health, orb.age), (-3, 2));
            assert_eq!(sounds.len(), 1);
        });
    }

    #[test]
    fn legacy_orb_in_air_or_water_or_non_lava_does_not_play_sound_or_consume_random() {
        with_fixture(|world, facts, collision, fluids, assets| {
            // Air scenario
            {
                let mut orb_air = sample_orb(Vec3::new(20.5, 6.0, 20.5), 5);
                let mut draws = 0;
                let mut next_float = || {
                    draws += 1;
                    0.5
                };
                let mut sounds = Vec::new();
                assert!(orb_air.tick(
                    world,
                    facts,
                    collision,
                    None,
                    true,
                    &mut next_float,
                    None,
                    fluids,
                    &mut |pos, pitch| sounds.push((pos, pitch)),
                ));
                assert!(sounds.is_empty(), "orb in air must not play burn sound");
                assert_eq!(
                    draws, 0,
                    "orb in air must not consume random float for lava"
                );
                assert_eq!(orb_air.health, 5);
            }

            // Water scenario
            {
                let water = assets.registry.default_state("minecraft:water").unwrap();
                world.set(20, 4, 20, water).unwrap();
                let mut orb_water = sample_orb(Vec3::new(20.5, 4.1, 20.5), 5);
                let mut draws = 0;
                let mut next_float = || {
                    draws += 1;
                    0.5
                };
                let mut sounds = Vec::new();
                assert!(orb_water.tick(
                    world,
                    facts,
                    collision,
                    None,
                    true,
                    &mut next_float,
                    None,
                    fluids,
                    &mut |pos, pitch| sounds.push((pos, pitch)),
                ));
                assert!(sounds.is_empty(), "orb in water must not play burn sound");
                assert_eq!(
                    draws, 0,
                    "orb in water must not consume random float for lava"
                );
                assert_eq!(orb_water.health, 5);
            }
        });
    }

    #[test]
    fn negative_case_non_contact_or_client_orb_does_not_play_burn_sound() {
        with_fixture(|world, facts, collision, fluids, assets| {
            let lava = assets.registry.default_state("minecraft:lava").unwrap();
            world.set(20, 4, 20, lava).unwrap();

            // Distant orb (no contact)
            let mut orb_away = sample_orb(Vec3::new(22.5, 4.1, 22.5), 5);
            let mut draws = 0;
            let mut next_float = || {
                draws += 1;
                0.5
            };
            let mut sounds = Vec::new();
            assert!(orb_away.tick(
                world,
                facts,
                collision,
                None,
                true,
                &mut next_float,
                None,
                fluids,
                &mut |pos, pitch| sounds.push((pos, pitch)),
            ));
            assert!(
                sounds.is_empty(),
                "non-contact orb must not play burn sound"
            );
            assert_eq!(draws, 0, "non-contact orb must not consume random float");
            assert_eq!(orb_away.health, 5);

            // Client orb in lava (server = false)
            let mut client_orb = sample_orb(Vec3::new(20.5, 4.1, 20.5), 5);
            sounds.clear();
            assert!(client_orb.tick(
                world,
                facts,
                collision,
                None,
                false,
                &mut || 0.5,
                None,
                fluids,
                &mut |pos, pitch| sounds.push((pos, pitch)),
            ));
            assert!(
                sounds.is_empty(),
                "client orb in lava must not play burn sound"
            );
            assert_eq!(client_orb.health, 5);

            // Dead orb (health <= 0) takes no contact damage and emits no sound
            let mut dead_orb = sample_orb(Vec3::new(20.5, 4.1, 20.5), 0);
            sounds.clear();
            let mut dead_draws = 0;
            let discarded = dead_orb.apply_lava_contact_damage(
                world,
                fluids,
                Vec3::ZERO,
                &mut || {
                    dead_draws += 1;
                    0.5
                },
                &mut |pos, pitch| sounds.push((pos, pitch)),
            );
            assert!(discarded, "dead orb remains discarded");
            assert!(sounds.is_empty(), "dead orb must not play burn sound");
            assert_eq!(dead_draws, 0, "dead orb must not consume random float");
        });
    }

    #[test]
    fn positive_case_server_orb_in_lava_plays_generic_burn_sound() {
        with_fixture(|world, facts, collision, fluids, assets| {
            let lava = assets.registry.default_state("minecraft:lava").unwrap();
            world.set(20, 4, 20, lava).unwrap();

            let at = Vec3::new(20.5, 4.1, 20.5);
            let mut orb = sample_orb(at, 5);
            let mut sounds = Vec::new();
            let float_val = 0.35_f32;
            let stays = orb.tick(
                world,
                facts,
                collision,
                None,
                true,
                &mut || float_val,
                None,
                fluids,
                &mut |pos, pitch| sounds.push((pos, pitch)),
            );
            assert!(stays, "orb survives first contact with 1 HP");
            assert_eq!(orb.health, 1);
            assert_eq!(sounds.len(), 1, "exactly one burn sound played");
            let expected_pitch = 2.0_f32 + float_val * 0.4_f32;
            assert!((sounds[0].1 - expected_pitch).abs() < 1e-6);
            assert_eq!(
                sounds[0].0,
                [orb.position.x, orb.position.y, orb.position.z]
            );
        });
    }

    #[test]
    fn boundary_case_surviving_and_discarding_and_multi_contact_in_one_tick() {
        with_fixture(|world, facts, collision, fluids, assets| {
            let lava = assets.registry.default_state("minecraft:lava").unwrap();
            world.set(20, 4, 20, lava).unwrap();

            // 1. Surviving at 1 HP vs discarding at <= 0 HP
            let mut orb = sample_orb(Vec3::new(20.5, 4.1, 20.5), 5);
            let mut sounds = Vec::new();

            // Tick 1: 5 HP -> 1 HP (survives)
            assert!(orb.tick(
                world,
                facts,
                collision,
                None,
                true,
                &mut || 0.1,
                None,
                fluids,
                &mut |pos, pitch| sounds.push((pos, pitch)),
            ));
            assert_eq!(orb.health, 1);
            assert_eq!(sounds.len(), 1);
            assert!((sounds[0].1 - (2.0 + 0.1 * 0.4)).abs() < 1e-6);

            // Tick 2: 1 HP -> -3 HP (discarded)
            sounds.clear();
            assert!(!orb.tick(
                world,
                facts,
                collision,
                None,
                true,
                &mut || 0.9,
                None,
                fluids,
                &mut |pos, pitch| sounds.push((pos, pitch)),
            ));
            assert_eq!(orb.health, -3);
            assert_eq!(sounds.len(), 1);
            assert!((sounds[0].1 - (2.0 + 0.9 * 0.4)).abs() < 1e-6);

            // 2. Multiple lava contacts in a single tick
            world.set(21, 4, 20, lava).unwrap();
            let mut multi_orb = sample_orb(Vec3::new(21.5, 4.1, 20.5), 5);
            multi_orb.previous = Vec3::new(19.8, 4.1, 20.5);
            let wanted = Vec3::new(1.7, 0.0, 0.0);
            let mut float_draws = vec![0.2_f32, 0.8_f32];
            let mut multi_sounds = Vec::new();
            let discarded = multi_orb.apply_lava_contact_damage(
                world,
                fluids,
                wanted,
                &mut || float_draws.remove(0),
                &mut |pos, pitch| multi_sounds.push((pos, pitch)),
            );
            assert!(discarded, "orb is discarded after two contacts");
            assert_eq!(multi_orb.health, -3);
            assert_eq!(multi_sounds.len(), 2, "both contacts triggered burn sound");
            assert!((multi_sounds[0].1 - (2.0 + 0.2 * 0.4)).abs() < 1e-6);
            assert!((multi_sounds[1].1 - (2.0 + 0.8 * 0.4)).abs() < 1e-6);
        });
    }
}

/// `Entity.checkInsideBlocks` and `collidedWithFluid`: fluid AABB contacts
/// along Entity.move's axis-ordered path, each block visited once.
fn orb_lava_contacts(
    from: Vec3,
    to: Vec3,
    wanted: Vec3,
    mut lava_height: impl FnMut(BlockPos) -> Option<f64>,
) -> usize {
    let delta = to.subtract(from);
    let mut visited = std::collections::HashSet::new();
    let mut contacts = 0;
    let mut remaining = 16;
    let mut segment = |start: Vec3, end: Vec3, budget: i32| {
        let mut iterations = 0;
        let epsilon = f64::from(1.0e-5_f32);
        let target = ExperienceOrb::box_at(end).deflate(Vec3::new(epsilon, epsilon, epsilon));
        let _ = horizon_core::geometry::sweep_intersected_blocks(
            start,
            end,
            target,
            1_000_000,
            |p, step| {
                if step >= budget {
                    return false;
                }
                iterations = step;
                if visited.contains(&p) {
                    return true;
                }
                if let Some(height) = lava_height(p) {
                    let min = Vec3::new(f64::from(p.x), f64::from(p.y), f64::from(p.z));
                    let fluid = Aabb::new(min, min.add_vector(Vec3::new(1.0, height, 1.0)));
                    if ExperienceOrb::box_at(start)
                        .collided_along_vector(end.subtract(start), &[fluid])
                    {
                        visited.insert(p);
                        contacts += 1;
                    }
                }
                true
            },
        );
        iterations + 1
    };
    if delta.length_squared() > 0.0 {
        // Direction.axisStepOrder uses the original, uncollided movement.
        let order = if wanted.x.abs() < wanted.z.abs() {
            [1, 2, 0]
        } else {
            [1, 0, 2]
        };
        let components = delta.components();
        let mut start = from;
        for axis in order {
            if components[axis] != 0.0 {
                let mut end = start.components();
                end[axis] += components[axis];
                let end = Vec3::new(end[0], end[1], end[2]);
                remaining -= segment(start, end, remaining);
                start = end;
            }
        }
    } else {
        remaining -= segment(from, to, remaining);
    }
    if remaining <= 0 {
        segment(to, to, 1);
    }
    contacts
}

#[cfg(test)]
mod orb_lava_shape_tests {
    use super::*;

    fn contacts(from: Vec3, to: Vec3) -> usize {
        orb_lava_contacts(from, to, to.subtract(from), |p| {
            (p == BlockPos::new(0, 0, 0)).then_some(f64::from(8.0_f32 / 9.0))
        })
    }

    #[test]
    fn lava_uses_the_swept_box_and_fluid_height_not_the_feet_block() {
        let side = Vec3::new(-0.2, 0.1, 0.5);
        assert_eq!(contacts(side, side), 1);
        let above = Vec3::new(0.5, 0.95, 0.5);
        assert_eq!(contacts(above, above), 0);
        let through = Vec3::new(-1.0, 0.1, 0.5);
        let end = Vec3::new(2.0, 0.1, 0.5);
        assert_eq!(contacts(through, end), 1);
        let away = Vec3::new(-1.0, 0.1, -1.0);
        assert_eq!(contacts(away, away), 0);
    }

    #[test]
    fn axis_segments_do_not_damage_twice_for_the_same_lava_cell() {
        let from = Vec3::new(0.1, 0.1, 0.1);
        let to = Vec3::new(0.7, 0.2, 0.7);
        assert_eq!(contacts(from, to), 1);
        assert_eq!(
            orb_lava_contacts(from, to, Vec3::new(0.1, 0.1, 0.9), |p| {
                (p == BlockPos::new(0, 0, 0)).then_some(1.0)
            }),
            1
        );
    }
}
