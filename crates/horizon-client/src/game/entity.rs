//! `Entity`: what every kind in the game's entity lists answers to, so a system
//! can treat them alike and only write down where vanilla's kinds differ.
//!
//! Default methods follow vanilla's `Entity`; a kind overrides one only where
//! its vanilla class does. [`EntityHandle`] names one entity by list and index,
//! so a system can walk handles and reach each entity after other borrows of
//! `Game` end. The walk allocates nothing and dispatch is static: hot paths pay
//! no vtable. A new kind is one line in `entity_lists!` and one `impl Entity`.

use super::commands::Target;
use super::persist::ChunkKey;
use super::{FallingBlock, Game};
use super::{
    boats::Boat, fireworks::Rocket, minecarts::Minecart, tnt::PrimedTnt, trident::ThrownTrident,
};
use horizon_client::entities::{
    ArmorStand, Cushion, ExperienceOrb, ItemEntity, ItemFrame, Painting,
};
use horizon_client::entity_rules::EntityRules;
use horizon_client::join_seq::JoinSeq;
use horizon_client::join_seq::Joined as _;
use horizon_client::mobs::{Mob, Species};
use horizon_client::projectiles::Projectile;
use horizon_core::geometry::{Aabb, Direction, Vec3};
use horizon_core::position::BlockPos;
use horizon_world::nbt::Tag;
use horizon_world::{level::data::PushReaction, piston_push};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

/// What `Entity.thunderHit` does to a kind: it sets the entity on fire and
/// calls `hurtServer(lightning_bolt, 5)`, so a kind whose `hurtServer` does
/// nothing (projectiles, rockets, TNT, falling blocks, armor stands here) stays
/// untouched. A strike runs the stages in this order.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ThunderHit {
    Untouched,
    /// Burns, takes 5 damage, and the kinds with their own `thunderHit` change.
    Mob,
    /// `VehicleEntity.hurtServer`: 5 x 10 is over the 40 that destroys it.
    Vehicle,
    /// `ExperienceOrb.hurtServer`: 5 damage off 5 health.
    Orb,
    /// `ItemEntity.hurtServer`: burns up unless fire resistant.
    Item,
}

/// What a blast knows about itself, for the entities in its reach to answer.
#[derive(Clone, Copy)]
pub(super) struct Blast {
    /// `VehicleEntity.ignoreExplosion`: a mob's blast, with mob griefing off.
    pub(super) vehicles_ignore: bool,
    /// `ItemEntity.ignoreExplosion` (`shouldAffectBlocklikeEntities`).
    pub(super) items_ignore: bool,
    /// The exploding mob, which `getEntities(source, box)` leaves out.
    pub(super) spared: Option<u32>,
}

pub(super) trait Entity: EntityRules {
    /// `Entity.getId`.
    fn id(&self) -> u32;
    fn position(&self) -> Vec3;
    fn position_mut(&mut self) -> &mut Vec3;
    /// `Entity.getDeltaMovement`.
    fn velocity(&self) -> Vec3;
    fn velocity_mut(&mut self) -> &mut Vec3;
    /// `Entity.getYRot` and `getXRot`: yaw, pitch.
    fn rotation(&self) -> [f32; 2];
    /// The rotation part of `Entity.teleportSetPosition`: `setYRot`, `setXRot`
    /// (clamped to -90..90), then `setOldRot`.
    fn set_rotation(&mut self, rotation: [f32; 2]);
    fn bounds(&self) -> Aabb;
    /// What a selector finds this entity as (`@e`), or `None` for one it cannot
    /// name yet. Required, so a new kind states which.
    fn selector_target(&self) -> Option<Target>;
    /// The entity type id a native mod's join hook names (`minecraft:oak_boat`).
    fn native_type(&self) -> Cow<'static, str>;
    /// Saves entity `index` of this kind into its chunk's entity list: the
    /// chunk and the NBT when it lies in `keys` and is saved with its chunk.
    /// Required, so a new kind states how it saves.
    fn save_nbt(
        game: &mut Game<'_>,
        index: usize,
        keys: &HashSet<ChunkKey>,
    ) -> Option<(ChunkKey, Tag)>;
    /// Loads a saved entity whose type id is `kind` as this kind and joins it to
    /// the world, or hands `tag` back when `kind` is not one this kind loads.
    /// Required, so a new kind states how it loads: it cannot be saved and then
    /// dropped silently on the way back in.
    fn load_nbt(
        game: &mut Game<'_>,
        kind: &str,
        uuid: Option<[i32; 4]>,
        player_uuid: Option<[i32; 4]>,
        tag: Tag,
    ) -> Result<(), Tag>;

    fn on_ground(&self) -> bool {
        false
    }
    /// `Entity.maxUpStep`.
    fn get_step_height(&self) -> f32 {
        0.0
    }
    fn is_alive(&self) -> bool {
        true
    }
    /// `Entity.isRemoved`.
    fn is_removed(&self) -> bool {
        false
    }
    /// `instanceof LivingEntity`.
    fn is_living(&self) -> bool {
        false
    }
    /// `Entity.isIgnoringBlockTriggers`.
    fn is_ignoring_block_triggers(&self) -> bool {
        false
    }
    /// Whether it presses plates and tripwires: `getEntitiesOfClass` skips
    /// removed entities, and the block's predicate skips the ones above.
    fn triggers_blocks(&self) -> bool {
        !self.is_removed() && !self.is_ignoring_block_triggers()
    }
    /// Whether `tick_blocks` sends the blocks in its box to `entityInside`
    /// (`applyEffectsFromBlocks`). A kind that function sweeps another way, or
    /// whose tick skips that step, says no.
    fn sweeps_blocks_inside(&self) -> bool {
        true
    }
    /// `Entity.thunderHit` for this kind.
    fn thunder_hit(&self) -> ThunderHit {
        ThunderHit::Untouched
    }
    /// The box a lightning strike tests against.
    fn thunder_bounds(&self) -> Aabb {
        self.bounds()
    }
    /// `Entity.ignoreExplosion`: the blast neither hurts nor pushes it. Also
    /// answers for what `getEntities(source, box)` never lists: the exploding
    /// mob, and the dead.
    fn ignores_explosion(&self, _blast: &Blast) -> bool {
        false
    }
    /// Whether a burst of wind moves it: what `Entity.push` moves, unless the
    /// kind says otherwise.
    fn pushed_by_wind(&self) -> bool {
        self.pushed_by_explosion()
    }
    /// Whether `Entity.push` moves it. An arrow in the ground, a wind charge and
    /// an armour stand (`explosion_breaks_decorations` takes those) stay put.
    fn pushed_by_explosion(&self) -> bool {
        true
    }
    /// The box the blast casts its rays at: `getSeenPercent`.
    fn explosion_box(&self) -> Aabb {
        self.bounds()
    }
    /// How far above its feet `pushFromExplosion` aims: `getEyeHeight`, which
    /// defaults to 0.85 of the box.
    fn explosion_eye_height(&self) -> f64 {
        let b = self.explosion_box();
        f64::from((b.max_position().y - b.min_position().y) as f32 * 0.85f32)
    }
    /// Whether this kind can still be waiting for its tick when a projectile
    /// ticks, though it joined before it (orbs, falling blocks, primed TNT: they
    /// tick between the batches, `game/ordered_tick.rs`).
    fn ticks_after_projectiles(&self) -> bool {
        false
    }
    /// `Entity.push` with the explosion's knockback.
    fn push_from_explosion(&mut self, push: Vec3) {
        let v = self.velocity_mut();
        *v = Vec3::new(v.x + push.x, v.y + push.y, v.z + push.z);
    }
    /// `Entity.isSupportedBy`.
    fn is_supported_by(&self, _pos: BlockPos) -> bool {
        false
    }
    /// `Entity.getEyeY`, where `updateFluidHeightAndDoFluidPushing` reads the
    /// eye's fluid; the top of the box unless a kind sets it.
    fn fluid_eye_y(&self) -> f64 {
        self.bounds().max_position().y
    }
    /// `Entity.updateFluidHeightAndDoFluidPushing`: its water and lava heights
    /// (`(0, 0)` for a kind that is not pushed by fluid), and the current of
    /// each fluid added to its velocity. One body for every kind.
    fn push_by_fluid(
        &mut self,
        world: &horizon_client::world::World,
        fluids: &horizon_client::motion::FluidResources,
    ) -> (f64, f64) {
        if !self.is_pushed_by_fluid() {
            return (0.0, 0.0);
        }
        let (bounds, at, eye) = (self.bounds(), self.position(), self.fluid_eye_y());
        let (water, lava, _) = horizon_client::entities::update_entity_fluids(
            bounds,
            at,
            eye,
            self.velocity_mut(),
            world,
            fluids,
        );
        (water, lava)
    }
    /// `BubbleColumnBlock.entityInside` through `applyEffectsFromBlocks`: a
    /// column lifts or drags every kind alike.
    fn push_by_bubble_column(
        &mut self,
        world: &horizon_client::world::World,
        registry: &horizon_world::registry::BlockRegistry,
        facts: &horizon_client::blocks::BlockFacts,
        collision: &horizon_world::block_collision::BlockCollisionRegistry<'_>,
    ) -> bool {
        self.push_by_bubble_column_over(world, registry, facts, collision, |_| {})
    }
    /// [`Entity::push_by_bubble_column`], telling `surface` the position of
    /// each column block it lies in (for the bubbles at the top).
    fn push_by_bubble_column_over(
        &mut self,
        world: &horizon_client::world::World,
        registry: &horizon_world::registry::BlockRegistry,
        facts: &horizon_client::blocks::BlockFacts,
        collision: &horizon_world::block_collision::BlockCollisionRegistry<'_>,
        surface: impl FnMut(BlockPos),
    ) -> bool {
        let bounds = self.bounds();
        horizon_client::motion::bubble_column_push_over(
            world,
            registry,
            facts,
            collision,
            bounds,
            self.velocity_mut(),
            surface,
        )
    }
    /// `Entity.getPistonPushReaction`.
    fn get_piston_push_reaction(&self) -> PushReaction {
        PushReaction::PushPull
    }
    /// Width and height of the box a portal sweep follows, or None for a kind
    /// the game does not carry through portals (so far armor stands and
    /// falling blocks; fishing hooks are off the walk, and
    /// `FishingHook.canUsePortal` is false in vanilla).
    fn portal_size(&self) -> Option<(f32, f32)> {
        None
    }
    /// Whether [`Self::portal_size`] is Some, without working the size out (a
    /// mob's depends on its age, scale and pose).
    fn sweeps_portals(&self) -> bool {
        self.portal_size().is_some()
    }
    /// `Entity.canUsePortal(false)` for a kind in `portal_size`. A kind that
    /// cannot stays tracked, so its cooldown and process run out.
    fn can_use_portal(&self, _game: &Game<'_>) -> bool {
        true
    }
    /// A boat or minecart: the kinds whose portal pass opens the tick, before
    /// they move (`AbstractMinecart.tick` handles its portal first).
    fn is_vehicle(&self) -> bool {
        false
    }
    /// Where it stood at the start of its tick, for the kinds that store it
    /// (None: `portals.before` has it).
    fn portal_from(&self) -> Option<Vec3> {
        None
    }
    /// Whether this tick's movement can have entered a portal block: a stuck
    /// arrow or a trident in the ground that did not move enters none.
    fn portal_sweeps(&self) -> bool {
        true
    }
    /// `Entity.move(MoverType.PISTON, …)` once the movement is resolved.
    fn move_by_piston(&mut self, movement: Vec3) {
        let position = self.position_mut();
        *position = position.add_vector(movement);
    }
    /// Bounds for `AbstractArrow.shouldFall` after a non-SELF piston move.
    fn piston_release_bounds(&self) -> Option<Aabb> {
        None
    }
    fn release_after_piston(&mut self, _no_block_collision: bool) {}
    /// `setOnGroundWithMovement` at the end of a piston's or shulker lid's
    /// `Entity.move`, for the kinds that keep the flag: on a server it runs
    /// for every push, sideways ones and those the collision turned away included.
    fn set_piston_on_ground(&mut self, _on_ground: bool) {}
    /// `Entity.setOnGround` from outside the entity's own move (`/tp`'s
    /// `TeleportCommand.performTeleport`); kinds that keep no ground flag ignore it.
    fn set_on_ground(&mut self, _on_ground: bool) {}
    /// `PistonMovingBlockEntity.causeBounce`: a slime block sets the velocity
    /// along the movement axis.
    fn piston_slime_bounce(&mut self, movement: Direction) {
        let velocity = self.velocity_mut();
        *velocity = piston_push::bounce_velocity(movement, *velocity);
    }
}

/// Bounds and identity needed by the render occlusion culler. Moving renderers
/// use interpolated entity bounds; attached entities use their own world bounds.
pub(super) trait RenderEntityBounds {
    fn render_id(&self) -> u32;
    fn render_bounds(&self, alpha: f64) -> Aabb;
    fn render_glowing(&self, game: &Game<'_>) -> bool {
        game.entity_glowing(self.render_id())
    }
}

impl RenderEntityBounds for Mob {
    fn render_id(&self) -> u32 {
        self.id
    }

    fn render_bounds(&self, alpha: f64) -> Aabb {
        // Keep models with large independent appendages on the old envelope:
        // their tentacles and wings can extend several blocks past the entity
        // dimensions. Equipment and custom rendering bypass this pre-check at
        // the call site.
        let (width, height, _) = self.awake_dims();
        let at = self.previous.lerp(self.position, f64::from(alpha as f32));
        if matches!(
            self.species(),
            Species::Guardian | Species::ElderGuardian | Species::Phantom
        ) {
            let radius = width.max(height) * 2.5 + 3.0;
            return Aabb::of_size(at, Vec3::new(radius * 2.0, radius * 2.0, radius * 2.0));
        }

        // Vanilla's ordinary models can rotate their head, limbs and death
        // pose beyond the collision box. This envelope covers those poses while
        // remaining close to the rendered mesh for the common upright mobs.
        // A few quadruped death and riding poses reach several blocks past
        // their standing width. Keep a bounded fallback radius for ordinary
        // models while still shrinking the common pig/zombie envelope below
        // the old five-block radius.
        let horizontal = (width * 2.0).max(height * 1.5).max(3.5);
        let vertical = height * 3.0 + 0.5;
        Aabb::new(
            at.subtract(Vec3::new(horizontal, vertical, horizontal)),
            at.add_vector(Vec3::new(horizontal, vertical, horizontal)),
        )
    }

    fn render_glowing(&self, _game: &Game<'_>) -> bool {
        self.is_currently_glowing()
    }
}

impl RenderEntityBounds for ArmorStand {
    fn render_id(&self) -> u32 {
        self.id
    }

    fn render_bounds(&self, alpha: f64) -> Aabb {
        let at = self.previous.lerp(self.position, alpha);
        // ArmorStandRenderer poses the limbs, then attaches hand items, head
        // items, armour and folded elytra. Those can extend well beyond the
        // 0.5 x 1.975 collision box (especially raised arms and head banners).
        // This axis-aligned envelope contains every vanilla pose while staying
        // independent of the stand's yaw and partial-tick rotation.
        let scale = if self.small { 0.5 } else { 1.0 };
        let equipped = self.equipment.iter().any(Option::is_some);
        let horizontal = if equipped { 2.0 } else { 1.5 * scale };
        let lower = if equipped { -1.0 } else { -0.5 * scale };
        let upper = 3.5 * scale
            + if self.equipment[5].is_some() {
                1.5
            } else {
                0.0
            };
        Aabb::new(
            at.subtract(Vec3::new(horizontal, -lower, horizontal)),
            at.add_vector(Vec3::new(horizontal, upper, horizontal)),
        )
    }
}

impl RenderEntityBounds for ItemEntity {
    fn render_id(&self) -> u32 {
        self.id
    }

    fn render_bounds(&self, alpha: f64) -> Aabb {
        let at = self.previous.lerp(self.position, alpha);
        // DroppedItemRenderer bobs, spins, and fans out up to five item copies.
        // The draw path uses the resolved model's exact posed extents; this
        // fallback envelope also covers the largest vanilla ground item.
        Aabb::new(
            at.subtract(Vec3::new(0.75, 0.1, 0.75)),
            at.add_vector(Vec3::new(0.75, 1.0, 0.75)),
        )
    }
}

impl RenderEntityBounds for Boat {
    fn render_id(&self) -> u32 {
        self.id
    }

    fn render_bounds(&self, alpha: f64) -> Aabb {
        let at = self.previous.lerp(self.position, alpha);
        // The boat model turns with yaw and its animated paddles sweep outside
        // the sized collision box. Hurt and bubble-column rotations are covered
        // by this orientation-independent model envelope.
        Aabb::new(
            at.subtract(Vec3::new(1.75, 0.75, 1.75)),
            at.add_vector(Vec3::new(1.75, 1.5, 1.75)),
        )
    }
}

impl RenderEntityBounds for Minecart {
    fn render_id(&self) -> u32 {
        self.id
    }

    fn render_bounds(&self, alpha: f64) -> Aabb {
        let at = self.previous.lerp(self.position, alpha);
        let offset = self.custom_offset.map_or_else(
            || {
                if self.kind == super::minecarts::CartKind::Hopper {
                    1.0
                } else {
                    6.0
                }
            },
            f64::from,
        );
        let display_shift = 0.75 * (offset - 8.0) / 16.0;
        let display_low = at.y + 0.375 - 0.4 + display_shift;
        let display_high = at.y + 0.375 + 0.4 + display_shift + 0.9;
        Aabb::new(
            Vec3::new(at.x - 1.25, (at.y - 0.5).min(display_low), at.z - 1.25),
            Vec3::new(at.x + 1.25, (at.y + 1.0).max(display_high), at.z + 1.25),
        )
    }
}

fn aabb_from_extents(bounds: ([f64; 3], [f64; 3])) -> Aabb {
    Aabb::new(
        Vec3::new(bounds.0[0], bounds.0[1], bounds.0[2]),
        Vec3::new(bounds.1[0], bounds.1[1], bounds.1[2]),
    )
}

impl RenderEntityBounds for Painting {
    fn render_id(&self) -> u32 {
        self.id
    }

    fn render_bounds(&self, _alpha: f64) -> Aabb {
        aabb_from_extents(self.bounds())
    }
}

impl RenderEntityBounds for ItemFrame {
    fn render_id(&self) -> u32 {
        self.id
    }

    fn render_bounds(&self, _alpha: f64) -> Aabb {
        let (lo, hi) = self.bounds();
        let step = self.step().map(f64::from);
        let center: [f64; 3] = std::array::from_fn(|axis| (lo[axis] + hi[axis]) * 0.5);
        // Maps fill a full block, while the held item projects toward the
        // camera from the frame plane. Include both in the occlusion envelope.
        let half: [f64; 3] = std::array::from_fn(|axis| if step[axis] != 0.0 { 0.75 } else { 0.5 });
        aabb_from_extents((
            std::array::from_fn(|axis| center[axis] - half[axis]),
            std::array::from_fn(|axis| center[axis] + half[axis]),
        ))
    }
}

impl RenderEntityBounds for Cushion {
    fn render_id(&self) -> u32 {
        self.id
    }

    fn render_bounds(&self, _alpha: f64) -> Aabb {
        aabb_from_extents(self.bounds())
    }
}

#[cfg(test)]
mod render_bounds_tests {
    use super::*;

    #[test]
    fn armor_stand_bounds_include_raised_equipment_space() {
        let mut stand = ArmorStand::new(1, Vec3::new(4.0, 64.0, -2.0), 90.0);
        stand.show_arms = true;
        stand.pose[2] = [180.0, 0.0, 45.0];
        stand.equipment[0] = Some(horizon_client::items::ItemStack {
            item: "minecraft:iron_sword".into(),
            count: 1,
            damage: 0,
            extra: None,
        });
        stand.equipment[5] = Some(horizon_client::items::ItemStack {
            item: "minecraft:banner".into(),
            count: 1,
            damage: 0,
            extra: None,
        });
        let bounds = stand.render_bounds(0.5);
        assert!(bounds.min_position().x <= 2.0);
        assert!(bounds.max_position().x >= 6.0);
        assert!(bounds.min_position().y <= 63.0);
        assert!(bounds.max_position().y >= 69.0);
        assert!(bounds.min_position().z <= -4.0);
        assert!(bounds.max_position().z >= 0.0);
    }

    #[test]
    fn boat_bounds_cover_the_rotated_paddle_sweep() {
        let boat = Boat::new(2, "oak".into(), Vec3::new(0.0, 10.0, 0.0), 90.0);
        let bounds = boat.render_bounds(0.5);
        assert!(bounds.min_position().x <= -1.75);
        assert!(bounds.max_position().x >= 1.75);
        assert!(bounds.min_position().y <= 9.25);
        assert!(bounds.max_position().y >= 11.5);
        assert!(bounds.min_position().z <= -1.75);
        assert!(bounds.max_position().z >= 1.75);
    }

    #[test]
    fn item_frame_bounds_include_the_full_map_and_held_item_depth() {
        let frame = ItemFrame {
            id: 3,
            pos: [0, 0, 0],
            facing: 5,
            glow: false,
            item: None,
            rotation: 0,
            drop_chance: 1.0,
            fixed: false,
            invisible: false,
            check_interval: 0,
            custom_name: None,
            random: horizon_core::random::LegacyRandom::new(0),
        };
        let bounds = frame.render_bounds(1.0);
        assert!(bounds.min_position().x <= -0.71875);
        assert!(bounds.max_position().x >= 0.78125);
        assert!(bounds.min_position().y <= 0.0);
        assert!(bounds.max_position().y >= 1.0);
        assert!(bounds.min_position().z <= 0.0);
        assert!(bounds.max_position().z >= 1.0);
    }

    #[test]
    fn minecart_bounds_follow_custom_display_offset() {
        let mut cart = Minecart::new(
            4,
            super::super::minecarts::CartKind::Chest,
            Vec3::new(0.0, 10.0, 0.0),
        );
        cart.custom_offset = Some(32);
        let bounds = cart.render_bounds(1.0);
        assert!(bounds.min_position().y <= 9.5);
        assert!(bounds.max_position().y >= 12.8);
    }
}

/// The fields every kind stores under the same names.
macro_rules! stored_motion {
    () => {
        fn id(&self) -> u32 {
            self.id
        }
        fn position(&self) -> Vec3 {
            self.position
        }
        fn position_mut(&mut self) -> &mut Vec3 {
            &mut self.position
        }
        fn velocity(&self) -> Vec3 {
            self.velocity
        }
        fn velocity_mut(&mut self) -> &mut Vec3 {
            &mut self.velocity
        }
    };
}

impl Game<'_> {
    /// Tick an item owned by the caller. The two results are ordinary survival
    /// and explicit consumption by a block-contact handler. `escape_speed`:
    /// the item is inside a block and moves without physics this tick, pushed
    /// out at this speed.
    pub(super) fn tick_item_with_block_contacts(
        &mut self,
        item: &mut ItemEntity,
        escape_speed: Option<f32>,
        entity_boxes: &[Aabb],
        client: bool,
        splash: &mut impl FnMut(horizon_client::entities::ItemSplash),
    ) -> (bool, bool) {
        let no_physics = escape_speed.is_some();
        let heights = item.push_by_fluid(&self.world, &self.res.motion().fluids);
        // Entity.baseTick splashes before moving, except on the first tick.
        item.water_entry(heights.0 > 0.0, item.ticks == 0, splash);
        item.lava_fall(heights.1 > 0.0);
        let step = item.tick_move_before_block_effects(
            &self.world,
            self.res.facts(),
            &self.collision,
            no_physics,
            escape_speed,
            heights,
            entity_boxes,
            Some(self.border.geometry()),
            Some(&self.res.motion().blocks),
        );
        item.check_fall(&step, |fall, from, moved| {
            self.vehicle_fall_reset(fall, from, moved)
        });
        if let Some(pos) = step.vertical_contact {
            let state = self.world.block(pos.x, pos.y, pos.z);
            let position = item.position;
            let mut discarded = false;
            self.native_item_after_vertical_collision(
                self.dim.id(),
                client,
                item.id,
                state,
                [pos.x, pos.y, pos.z],
                [position.x, position.y, position.z],
                &mut item.stack,
                &mut discarded,
            );
            if discarded || item.stack.count == 0 {
                return (false, true);
            }
        }
        item.finish_move_block_effects(
            &self.world,
            self.res.facts(),
            no_physics,
            Some(&self.res.motion().blocks),
            step,
        );
        item.push_by_bubble_column(
            &self.world,
            self.pack.assets.registry,
            self.res.facts(),
            &self.collision,
        );
        let alive = item.tick_settle(&self.world, self.res.facts(), no_physics, step);
        // ItemEntity.tick refreshes immersion and splashes on water entry.
        let (water, _) = item.push_by_fluid(&self.world, &self.res.motion().fluids);
        item.water_entry(water > 0.0, false, splash);
        (alive, false)
    }

    /// Keep new joins after the active batch, so they tick next time.
    pub(super) fn restore_item_batch(
        &mut self,
        mut items: horizon_client::join_seq::JoinList<ItemEntity>,
    ) {
        items.extend(std::mem::take(&mut self.dropped));
        self.dropped = items;
    }
}

/// The game's lists of entities, in the order systems walk them. Hands them to
/// `$then`, so one list drives the handles and every typed dispatch.
macro_rules! entity_lists {
    ($then:ident, $($args:tt)*) => {
        $crate::game::entity::$then! {
            ($($args)*)
            Mob(Mob) mobs: mobs,
            Item(ItemEntity) dropped: dropped,
            Boat(Boat) boats: boats,
            Cart(Minecart) carts: carts,
            ArmorStand(ArmorStand) armor_stands: armor_stands,
            Tnt(PrimedTnt) tnts: tnts,
            Falling(FallingBlock) falling: falling,
            Orb(ExperienceOrb) orbs: orbs,
            Projectile(Projectile) projectiles: creatures.projectiles,
            Rocket(Rocket) rockets: rockets,
            Trident(ThrownTrident) tridents: tridents,
        }
    };
}
pub(crate) use entity_lists;

macro_rules! define_entity_handles {
    (() $($kind:ident($ty:ty) $field:ident: $($list:ident).+,)*) => {
        /// Every list, borrowed apart from the rest of `Game`, for
        /// [`for_each_entity`] while the world is borrowed mutably.
        #[derive(Clone, Copy)]
        pub(super) struct EntityLists<'w> {
            $(pub(super) $field: &'w [$ty],)*
        }

        /// One entity, by list and index.
        #[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
        pub(super) enum EntityHandle {
            $($kind(usize),)*
        }

        /// Every list moved out of `Game`, for [`for_each_entity_mut`]: the
        /// world stays readable while the entities change.
        pub(super) struct TakenEntities {
            $(pub(super) $field: horizon_client::join_seq::JoinList<$ty>,)*
        }


        impl EntityLists<'_> {
            /// Visits every entity once, in the order they joined, whatever the
            /// kind: a merge of the lists, each already in join order, by a
            /// cursor per list. Static dispatch and nothing allocated. Checked:
            /// a list out of join order, or an entity that never joined, stops
            /// the walk with the entity at fault.
            #[allow(dead_code, reason = "the tick order work that reads this has not landed yet")]
            pub(super) fn walk_in_join_order(
                self,
                visit: impl FnMut(EntityHandle, JoinSeq),
            ) -> Result<(), JoinOrderError> {
                let lens = [$(self.$field.len()),*];
                merge_in_join_order(
                    lens,
                    |kind, i| {
                        let mut at = 0;
                        $(if kind == at {
                            return self.$field[i].join_seq();
                        } at += 1;)*
                        let _ = at;
                        unreachable!("kind {kind} is not a list")
                    },
                    |kind, i| {
                        let mut at = 0;
                        $(if kind == at {
                            return EntityHandle::$kind(i);
                        } at += 1;)*
                        let _ = at;
                        unreachable!("kind {kind} is not a list")
                    },
                    visit,
                )
            }
        }

        impl Game<'_> {
            /// Moves every list out; give it back with `restore_entities`.
            pub(super) fn take_entities(&mut self) -> TakenEntities {
                TakenEntities {
                    $($field: std::mem::take(&mut self$(.$list)+),)*
                }
            }
            pub(super) fn restore_entities(&mut self, taken: TakenEntities) {
                $(self$(.$list)+ = taken.$field;)*
            }
            /// Every entity's handle: lengths read now, nothing allocated, and
            /// no borrow of `Game` held, so the caller can change it mid-walk.
            pub(super) fn entity_handles(&self) -> impl Iterator<Item = EntityHandle> + use<> {
                std::iter::empty()
                    $(.chain((0..self$(.$list)+.len()).map(EntityHandle::$kind)))*
            }

            /// Takes the entity out of its list (`Entity.discard` before it ever ticked).
            pub(super) fn discard_entity(&mut self, handle: EntityHandle) {
                match handle {
                    $(EntityHandle::$kind(i) => {
                        self$(.$list)+.remove(i);
                    })*
                }
            }

            /// [`Self::entity_handles`], last of each list first, so the
            /// caller can remove the entity it is on.
            pub(super) fn entity_handles_rev(&self) -> impl Iterator<Item = EntityHandle> + use<> {
                std::iter::empty()
                    $(.chain((0..self$(.$list)+.len()).rev().map(EntityHandle::$kind)))*
            }
        }
    };
}
pub(crate) use define_entity_handles;

/// Why [`EntityLists::walk_in_join_order`] stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "the tick order work that reads this has not landed yet"
)]
pub(super) enum JoinOrderError {
    /// Never put in its list through the list's own `push`, so it has no
    /// join number.
    Unjoined(EntityHandle),
    /// Joined before an entity the walk had already visited: its list is out
    /// of join order.
    OutOfOrder(EntityHandle),
}

/// The merge behind the join-order walk, over `N` lists: `lens` are their
/// lengths, `seq(list, i)` the join number of entity `i` of a list, `handle`
/// names it. Each step takes the lowest next number among the `N` cursors
/// (a stack array; no allocation).
#[allow(
    dead_code,
    reason = "the tick order work that reads this has not landed yet"
)]
fn merge_in_join_order<const N: usize>(
    lens: [usize; N],
    seq: impl Fn(usize, usize) -> JoinSeq,
    handle: impl Fn(usize, usize) -> EntityHandle,
    mut visit: impl FnMut(EntityHandle, JoinSeq),
) -> Result<(), JoinOrderError> {
    let mut cursor = [0usize; N];
    let mut last = JoinSeq::UNJOINED;
    loop {
        let mut best: Option<(usize, JoinSeq)> = None;
        for list in 0..N {
            if cursor[list] < lens[list] {
                let s = seq(list, cursor[list]);
                if !s.is_joined() {
                    return Err(JoinOrderError::Unjoined(handle(list, cursor[list])));
                }
                if best.is_none_or(|(_, b)| s < b) {
                    best = Some((list, s));
                }
            }
        }
        let Some((list, s)) = best else {
            return Ok(());
        };
        let h = handle(list, cursor[list]);
        if s <= last {
            return Err(JoinOrderError::OutOfOrder(h));
        }
        last = s;
        cursor[list] += 1;
        visit(h, s);
    }
}

entity_lists!(define_entity_handles,);
horizon_client::impl_joined!(FallingBlock);

macro_rules! entity_borrow {
    (shared, $place:expr) => {
        &$place
    };
    (exclusive, $place:expr) => {
        &mut $place
    };
}
pub(crate) use entity_borrow;

macro_rules! match_entity {
    (($borrow:ident, $entity:ident, $game:expr, $handle:expr, $body:expr)
     $($kind:ident($ty:ty) $field:ident: $($list:ident).+,)*) => {
        match $handle {
            $($crate::game::entity::EntityHandle::$kind(i) => {
                let $entity = $crate::game::entity::entity_borrow!($borrow, $game$(.$list)+[i]);
                $body
            })*
        }
    };
}
pub(crate) use match_entity;

/// Runs `$body` with `$entity` bound to the handle's entity, as its own type:
/// one match, each arm compiled for its kind, so nothing goes through a vtable.
macro_rules! with_entity {
    ($game:expr, $handle:expr, $entity:ident => $body:expr) => {
        $crate::game::entity::entity_lists!(match_entity, shared, $entity, $game, $handle, $body)
    };
}
pub(super) use with_entity;

macro_rules! borrow_lists {
    (($game:expr) $($kind:ident($ty:ty) $field:ident: $($list:ident).+,)*) => {
        $crate::game::entity::EntityLists {
            $($field: &$game$(.$list)+,)*
        }
    };
}
pub(crate) use borrow_lists;

/// `$game`'s [`EntityLists`]; a macro so the borrow stays field by field.
macro_rules! entity_lists_of {
    ($game:expr) => {
        $crate::game::entity::entity_lists!(borrow_lists, $game)
    };
}
pub(super) use entity_lists_of;

macro_rules! walk_lists {
    (($lists:expr, $entity:ident, $body:block)
     $($kind:ident($ty:ty) $field:ident: $($list:ident).+,)*) => {
        $(for $entity in $lists.$field $body)*
    };
}
pub(crate) use walk_lists;

/// Runs `$body` for every entity, with `$entity` as a reference to its own
/// type: one loop per list, each compiled for its kind, so nothing allocates
/// or goes through a vtable and a check a kind answers with a constant folds
/// away. A kind added to `entity_lists!` joins every walk.
macro_rules! for_each_entity {
    ($lists:expr, $entity:ident => $body:block) => {
        $crate::game::entity::entity_lists!(walk_lists, $lists, $entity, $body)
    };
}
pub(super) use for_each_entity;

macro_rules! walk_lists_mut {
    (($lists:expr, $entity:ident, $body:block)
     $($kind:ident($ty:ty) $field:ident: $($list:ident).+,)*) => {
        $(for $entity in $lists.$field.iter_mut() $body)*
    };
}
pub(crate) use walk_lists_mut;

/// [`for_each_entity`] over [`TakenEntities`], with the entity mutable.
macro_rules! for_each_entity_mut {
    ($lists:expr, $entity:ident => $body:block) => {
        $crate::game::entity::entity_lists!(walk_lists_mut, $lists, $entity, $body)
    };
}
pub(super) use for_each_entity_mut;

/// [`with_entity`], with the entity mutable.
macro_rules! with_entity_mut {
    ($game:expr, $handle:expr, $entity:ident => $body:expr) => {
        $crate::game::entity::entity_lists!(match_entity, exclusive, $entity, $game, $handle, $body)
    };
}
pub(super) use with_entity_mut;

macro_rules! save_lists {
    (($game:expr, $keys:expr, $out:expr)
     $($kind:ident($ty:ty) $field:ident: $($list:ident).+,)*) => {
        $(for i in 0..$game$(.$list)+.len() {
            if let Some((key, tag)) = <$ty as $crate::game::entity::Entity>::save_nbt($game, i, $keys) {
                $out.entry(key).or_default().push((
                    $game$(.$list)+[i].join_seq(),
                    tag,
                ));
            }
        })*
    };
}
pub(crate) use save_lists;

/// Saves every entity of every list in `entity_lists!` into `$out` by chunk,
/// each with its join number (`$out` maps a chunk to `(JoinSeq, Tag)` pairs),
/// each through its own `Entity::save_nbt`: one loop per list, static dispatch,
/// nothing allocated beyond the tags. A kind added to the lists cannot be left
/// out, since `save_nbt` has no default.
macro_rules! save_entities_of_every_kind {
    ($game:expr, $keys:expr, $out:expr) => {
        $crate::game::entity::entity_lists!(save_lists, $game, $keys, $out)
    };
}

macro_rules! load_lists {
    (($game:expr, $kind:expr, $uuid:expr, $player:expr, $tag:expr)
     $($kind_name:ident($ty:ty) $field:ident: $($list:ident).+,)*) => {{
        let mut tag = $tag;
        $(match <$ty as $crate::game::entity::Entity>::load_nbt($game, $kind, $uuid, $player, tag) {
            Ok(()) => return Ok(()),
            Err(back) => tag = back,
        })*
        Err(tag)
    }};
}
pub(crate) use load_lists;

/// Loads a saved entity as the first kind in `entity_lists!` that claims its
/// type id, through that kind's own `Entity::load_nbt`: one test per kind,
/// static dispatch, nothing allocated (the compound moves through). Evaluates
/// to `Err(tag)` when no kind claims it, and `return`s `Ok` from the caller
/// otherwise. A kind added to the lists cannot be left out, since `load_nbt`
/// has no default.
macro_rules! load_entity_of_any_kind {
    ($game:expr, $kind:expr, $uuid:expr, $player:expr, $tag:expr) => {
        $crate::game::entity::entity_lists!(load_lists, $game, $kind, $uuid, $player, $tag)
    };
}

/// The type ids the minecart kind loads.
fn loads_as_cart(kind: &str) -> bool {
    super::minecarts::CartKind::from_id(kind).is_some()
}
/// The type id the primed TNT kind loads.
fn loads_as_tnt(kind: &str) -> bool {
    kind == "minecraft:tnt"
}

impl Game<'_> {
    /// The type ids the mob kind loads.
    fn loads_as_mob(&self, kind: &str) -> bool {
        self.res.mob_types().contains_key(kind)
    }
    /// The type ids the boat kind loads.
    fn loads_as_boat(&self, kind: &str) -> bool {
        super::boats::any_boat_wood(kind)
            .is_some_and(|(wood, chest)| self.boat_models.has(wood, chest))
    }
    /// Whether `load_listed` loads `kind` as a vehicle that takes its mob
    /// riders (`take_mob_passengers`): a mob, boat, minecart or primed TNT.
    pub(super) fn listed_kind_takes_riders(&self, kind: &str) -> bool {
        self.loads_as_mob(kind)
            || self.loads_as_boat(kind)
            || loads_as_cart(kind)
            || loads_as_tnt(kind)
    }
    /// Loads a saved entity as the first kind in `entity_lists!` that claims
    /// its type id: `Err` hands the compound back when none does.
    pub(super) fn load_listed(
        &mut self,
        kind: &str,
        uuid: Option<[i32; 4]>,
        player_uuid: Option<[i32; 4]>,
        tag: Tag,
    ) -> Result<(), Tag> {
        load_entity_of_any_kind!(self, kind, uuid, player_uuid, tag)
    }
    /// The ids of the entities that are not on the walk of `entity_lists!`.
    pub(super) fn entity_ids_off_walk(&self) -> impl Iterator<Item = u32> + '_ {
        self.cushions
            .iter()
            .map(|e| e.id)
            .chain(self.item_frames.iter().map(|e| e.id))
            .chain(self.paintings.iter().map(|e| e.id))
            .chain(self.others.iter().map(|e| e.id))
            .chain(self.breath_clouds.iter().map(|e| e.id))
            .chain(self.fangs.list.iter().map(|e| e.id))
            .chain(self.crystals.iter().map(|e| e.id))
            .chain(self.dragon.iter().map(|e| e.id))
            .chain(self.ominous_spawners.iter().map(|e| e.id))
    }
    /// Saves every entity of every kind that lies in `keys` into `out`, by chunk.
    pub(super) fn save_every_entity(
        &mut self,
        keys: &HashSet<ChunkKey>,
        out: &mut HashMap<ChunkKey, Vec<Tag>>,
    ) {
        // Saved by chunk in the order they joined, so a reload adds them in
        // that order (a chunk's entity list is in insertion order in vanilla).
        let mut staged: HashMap<ChunkKey, Vec<(JoinSeq, Tag)>> = HashMap::new();
        save_entities_of_every_kind!(self, keys, staged);
        for (key, mut tags) in staged {
            tags.sort_by_key(|(seq, _)| *seq);
            out.entry(key)
                .or_default()
                .extend(tags.into_iter().map(|(_, tag)| tag));
        }
    }
}

impl Entity for Mob {
    fn set_on_ground(&mut self, on_ground: bool) {
        self.on_ground = on_ground;
    }
    /// `Entity.move(MoverType.PISTON, ...)` ends in `setPos`, which `Shulker`
    /// overrides to snap to the block centre.
    fn move_by_piston(&mut self, movement: Vec3) {
        let moved = self.position.add_vector(movement);
        if self.species() == Species::Shulker {
            crate::mobs::shulker::client_set_pos(self, moved.x, moved.y, moved.z);
        } else {
            self.position = moved;
        }
    }
    fn rotation(&self) -> [f32; 2] {
        [self.yaw, self.head_pitch]
    }
    fn set_rotation(&mut self, [yaw, pitch]: [f32; 2]) {
        let previous = self.rotation();
        let yaw = if yaw.is_finite() { yaw } else { previous[0] };
        let pitch = if pitch.is_nan() { previous[1] } else { pitch };
        self.yaw = yaw;
        self.body_yaw = yaw;
        self.head_yaw = yaw;
        self.head_pitch = pitch.clamp(-90.0, 90.0);
        self.previous_yaw = self.yaw;
        self.previous_head_pitch = self.head_pitch;
    }
    fn native_type(&self) -> Cow<'static, str> {
        super::native_hooks::mob_kind(self).into()
    }
    fn save_nbt(
        game: &mut Game<'_>,
        index: usize,
        keys: &HashSet<ChunkKey>,
    ) -> Option<(ChunkKey, Tag)> {
        game.save_mob(index, keys)
    }
    fn load_nbt(
        game: &mut Game<'_>,
        kind: &str,
        _uuid: Option<[i32; 4]>,
        _player_uuid: Option<[i32; 4]>,
        tag: Tag,
    ) -> Result<(), Tag> {
        if !game.loads_as_mob(kind) {
            return Err(tag);
        }
        game.load_mob(tag, None);
        Ok(())
    }
    stored_motion!();
    fn portal_size(&self) -> Option<(f32, f32)> {
        Some((self.width(), self.height()))
    }
    fn sweeps_portals(&self) -> bool {
        true
    }
    fn can_use_portal(&self, game: &Game<'_>) -> bool {
        game.mob_can_use_portal(self)
    }
    /// `PersistentEntitySectionManager.Callback.onRemove`: discard stops
    /// section enumeration immediately, before the entity list is swept.
    fn selector_target(&self) -> Option<Target> {
        (!self.removed && self.death_time == 0).then_some(Target::Mob(self.id))
    }
    fn bounds(&self) -> Aabb {
        Mob::bounds(self)
    }
    fn on_ground(&self) -> bool {
        self.on_ground
    }
    fn get_step_height(&self) -> f32 {
        self.kind.step_height as f32
    }
    fn is_alive(&self) -> bool {
        !self.removed && self.alive()
    }
    fn is_removed(&self) -> bool {
        self.removed
    }
    fn is_living(&self) -> bool {
        true
    }
    /// `Bat.isIgnoringBlockTriggers`.
    fn is_ignoring_block_triggers(&self) -> bool {
        self.species() == Species::Bat
    }
    /// `Warden.ignoreExplosion`: while it digs or emerges.
    fn ignores_explosion(&self, blast: &Blast) -> bool {
        !self.alive()
            || blast.spared == Some(self.id)
            || (self.species() == Species::Warden
                && horizon_client::mobs::warden::digging_or_emerging(self))
    }
    /// `getEyeHeight` of the mob as it stands: a baby's, a slime's or a
    /// phantom's own scaled eye height, not the adult kind's. The blast's box
    /// is its real `bounds`, so it needs no override.
    fn explosion_eye_height(&self) -> f64 {
        self.eye_height()
    }
    /// `Entity.push` sets `needsSync`.
    fn push_from_explosion(&mut self, push: Vec3) {
        self.velocity = Vec3::new(
            self.velocity.x + push.x,
            self.velocity.y + push.y,
            self.velocity.z + push.z,
        );
        self.needs_sync = true;
    }
    fn is_supported_by(&self, pos: BlockPos) -> bool {
        self.supporting == Some([pos.x, pos.y, pos.z])
    }
    fn thunder_hit(&self) -> ThunderHit {
        if self.alive() {
            ThunderHit::Mob
        } else {
            ThunderHit::Untouched
        }
    }
    /// The kind's own size, as the strike has always read it.
    fn thunder_bounds(&self) -> Aabb {
        let (p, w) = (self.position, self.kind.width / 2.0);
        Aabb::new(
            Vec3::new(p.x - w, p.y, p.z - w),
            Vec3::new(p.x + w, p.y + self.kind.height, p.z + w),
        )
    }
}

impl Entity for ItemEntity {
    fn set_on_ground(&mut self, on_ground: bool) {
        self.on_ground = on_ground;
    }
    fn rotation(&self) -> [f32; 2] {
        self.rotation
    }
    fn set_rotation(&mut self, [yaw, pitch]: [f32; 2]) {
        let previous = self.rotation();
        let yaw = if yaw.is_finite() { yaw } else { previous[0] };
        let pitch = if pitch.is_nan() { previous[1] } else { pitch };
        self.rotation = [yaw, pitch.clamp(-90.0, 90.0)];
    }
    fn fluid_eye_y(&self) -> f64 {
        ItemEntity::fluid_eye_y(self)
    }
    fn native_type(&self) -> Cow<'static, str> {
        "minecraft:item".into()
    }
    fn save_nbt(
        game: &mut Game<'_>,
        index: usize,
        keys: &HashSet<ChunkKey>,
    ) -> Option<(ChunkKey, Tag)> {
        game.save_item(index, keys)
    }
    fn load_nbt(
        game: &mut Game<'_>,
        kind: &str,
        uuid: Option<[i32; 4]>,
        _player_uuid: Option<[i32; 4]>,
        tag: Tag,
    ) -> Result<(), Tag> {
        if !(kind == "minecraft:item") {
            return Err(tag);
        }
        game.load_item(tag, uuid);
        Ok(())
    }
    stored_motion!();
    fn portal_size(&self) -> Option<(f32, f32)> {
        Some((0.25, 0.25))
    }
    fn selector_target(&self) -> Option<Target> {
        Some(Target::Item(self.id))
    }
    fn thunder_hit(&self) -> ThunderHit {
        ThunderHit::Item
    }
    fn ignores_explosion(&self, blast: &Blast) -> bool {
        blast.items_ignore
    }
    fn explosion_eye_height(&self) -> f64 {
        0.2125
    }
    fn bounds(&self) -> Aabb {
        ItemEntity::bounds(self)
    }
    fn on_ground(&self) -> bool {
        self.on_ground
    }
    fn set_piston_on_ground(&mut self, on_ground: bool) {
        self.on_ground = on_ground;
    }
    fn piston_slime_bounce(&mut self, movement: Direction) {
        ItemEntity::piston_slime_bounce(self, movement);
    }
}

impl Entity for ExperienceOrb {
    fn set_on_ground(&mut self, on_ground: bool) {
        self.on_ground = on_ground;
    }
    fn rotation(&self) -> [f32; 2] {
        self.rotation
    }
    fn set_rotation(&mut self, [yaw, pitch]: [f32; 2]) {
        let previous = self.rotation();
        let yaw = if yaw.is_finite() { yaw } else { previous[0] };
        let pitch = if pitch.is_nan() { previous[1] } else { pitch };
        self.rotation = [yaw, pitch.clamp(-90.0, 90.0)];
    }
    fn ticks_after_projectiles(&self) -> bool {
        true
    }
    fn native_type(&self) -> Cow<'static, str> {
        "minecraft:experience_orb".into()
    }
    fn save_nbt(
        game: &mut Game<'_>,
        index: usize,
        keys: &HashSet<ChunkKey>,
    ) -> Option<(ChunkKey, Tag)> {
        game.save_orb(index, keys)
    }
    fn load_nbt(
        game: &mut Game<'_>,
        kind: &str,
        uuid: Option<[i32; 4]>,
        _player_uuid: Option<[i32; 4]>,
        tag: Tag,
    ) -> Result<(), Tag> {
        if !(kind == "minecraft:experience_orb") {
            return Err(tag);
        }
        game.load_orb(tag, uuid);
        Ok(())
    }
    stored_motion!();
    fn portal_size(&self) -> Option<(f32, f32)> {
        Some((0.5, 0.5))
    }
    fn selector_target(&self) -> Option<Target> {
        Some(Target::Orb(self.id))
    }
    fn thunder_hit(&self) -> ThunderHit {
        ThunderHit::Orb
    }
    fn explosion_eye_height(&self) -> f64 {
        f64::from(0.5f32 * 0.85f32)
    }
    fn bounds(&self) -> Aabb {
        ExperienceOrb::bounds(self)
    }
    fn on_ground(&self) -> bool {
        self.on_ground
    }
    fn set_piston_on_ground(&mut self, on_ground: bool) {
        self.on_ground = on_ground;
    }
    fn piston_slime_bounce(&mut self, movement: Direction) {
        self.velocity = piston_push::bounce_velocity(movement, self.velocity);
        self.on_ground = false;
    }
}

impl Entity for Boat {
    fn set_on_ground(&mut self, on_ground: bool) {
        self.on_ground = on_ground;
    }
    fn set_piston_on_ground(&mut self, on_ground: bool) {
        self.on_ground = on_ground;
    }
    fn rotation(&self) -> [f32; 2] {
        [self.yaw, self.pitch]
    }
    fn set_rotation(&mut self, [yaw, pitch]: [f32; 2]) {
        let previous = self.rotation();
        let yaw = if yaw.is_finite() { yaw } else { previous[0] };
        let pitch = if pitch.is_nan() { previous[1] } else { pitch };
        self.yaw = yaw;
        self.pitch = pitch.clamp(-90.0, 90.0);
    }
    fn native_type(&self) -> Cow<'static, str> {
        self.entity_type().into()
    }
    fn save_nbt(
        game: &mut Game<'_>,
        index: usize,
        keys: &HashSet<ChunkKey>,
    ) -> Option<(ChunkKey, Tag)> {
        game.save_boat(index, keys)
    }
    fn load_nbt(
        game: &mut Game<'_>,
        kind: &str,
        uuid: Option<[i32; 4]>,
        _player_uuid: Option<[i32; 4]>,
        tag: Tag,
    ) -> Result<(), Tag> {
        if !game.loads_as_boat(kind) {
            return Err(tag);
        }
        game.load_boat(tag, uuid, kind);
        Ok(())
    }
    stored_motion!();
    fn is_vehicle(&self) -> bool {
        true
    }
    /// `Boat.getEyeY`: the top of the 0.5625 box.
    fn fluid_eye_y(&self) -> f64 {
        self.position.y + super::boats::HEIGHT
    }
    fn portal_size(&self) -> Option<(f32, f32)> {
        Some((super::boats::WIDTH as f32, super::boats::HEIGHT as f32))
    }
    fn can_use_portal(&self, game: &Game<'_>) -> bool {
        game.vehicle_can_use_portal(self.id)
    }
    fn selector_target(&self) -> Option<Target> {
        Some(Target::Boat(self.id))
    }
    fn thunder_hit(&self) -> ThunderHit {
        ThunderHit::Vehicle
    }
    fn ignores_explosion(&self, blast: &Blast) -> bool {
        blast.vehicles_ignore
    }
    /// `EntityType.OAK_BOAT & co`: the full 0.5625.
    fn explosion_eye_height(&self) -> f64 {
        0.5625
    }
    fn bounds(&self) -> Aabb {
        Boat::bounds(self)
    }
    fn on_ground(&self) -> bool {
        self.on_ground
    }
}

impl Entity for Minecart {
    fn set_on_ground(&mut self, on_ground: bool) {
        self.on_ground = on_ground;
    }
    fn set_piston_on_ground(&mut self, on_ground: bool) {
        self.on_ground = on_ground;
    }
    fn rotation(&self) -> [f32; 2] {
        [self.yaw, self.pitch]
    }
    fn set_rotation(&mut self, [yaw, pitch]: [f32; 2]) {
        let previous = self.rotation();
        let yaw = if yaw.is_finite() { yaw } else { previous[0] };
        let pitch = if pitch.is_nan() { previous[1] } else { pitch };
        self.yaw = yaw;
        self.pitch = pitch.clamp(-90.0, 90.0);
    }
    fn native_type(&self) -> Cow<'static, str> {
        self.kind.id().into()
    }
    fn save_nbt(
        game: &mut Game<'_>,
        index: usize,
        keys: &HashSet<ChunkKey>,
    ) -> Option<(ChunkKey, Tag)> {
        game.save_cart(index, keys)
    }
    fn load_nbt(
        game: &mut Game<'_>,
        kind: &str,
        uuid: Option<[i32; 4]>,
        _player_uuid: Option<[i32; 4]>,
        tag: Tag,
    ) -> Result<(), Tag> {
        if !loads_as_cart(kind) {
            return Err(tag);
        }
        game.load_cart(tag, uuid, kind);
        Ok(())
    }
    stored_motion!();
    fn is_vehicle(&self) -> bool {
        true
    }
    fn portal_size(&self) -> Option<(f32, f32)> {
        Some((
            super::minecarts::WIDTH as f32,
            super::minecarts::HEIGHT as f32,
        ))
    }
    fn can_use_portal(&self, game: &Game<'_>) -> bool {
        game.vehicle_can_use_portal(self.id)
    }
    fn selector_target(&self) -> Option<Target> {
        Some(Target::Cart(self.id))
    }
    fn thunder_hit(&self) -> ThunderHit {
        ThunderHit::Vehicle
    }
    fn ignores_explosion(&self, blast: &Blast) -> bool {
        blast.vehicles_ignore
    }
    fn bounds(&self) -> Aabb {
        Minecart::bounds(self)
    }
    fn on_ground(&self) -> bool {
        self.on_ground
    }
    /// `tick_blocks` sweeps minecarts along with the players.
    fn sweeps_blocks_inside(&self) -> bool {
        false
    }
}

/// Width and height `EntityType.ARMOR_STAND` gives a stand.
fn armor_stand_size(stand: &ArmorStand) -> (f64, f64) {
    if stand.marker {
        (0.0, 0.0)
    } else if stand.small {
        (0.25, f64::from(0.9875f32))
    } else {
        (0.5, f64::from(1.975f32))
    }
}

impl Entity for ArmorStand {
    fn set_on_ground(&mut self, on_ground: bool) {
        self.on_ground = on_ground;
    }
    fn rotation(&self) -> [f32; 2] {
        [self.yaw, self.pitch]
    }
    fn set_rotation(&mut self, [yaw, pitch]: [f32; 2]) {
        let previous = self.rotation();
        let yaw = if yaw.is_finite() { yaw } else { previous[0] };
        let pitch = if pitch.is_nan() { previous[1] } else { pitch };
        self.yaw = yaw;
        self.previous_yaw = yaw;
        self.pitch = pitch.clamp(-90.0, 90.0);
    }
    fn fluid_eye_y(&self) -> f64 {
        ArmorStand::fluid_eye_y(self)
    }
    fn native_type(&self) -> Cow<'static, str> {
        "minecraft:armor_stand".into()
    }
    fn save_nbt(
        game: &mut Game<'_>,
        index: usize,
        keys: &HashSet<ChunkKey>,
    ) -> Option<(ChunkKey, Tag)> {
        game.save_armor_stand(index, keys)
    }
    fn load_nbt(
        game: &mut Game<'_>,
        kind: &str,
        uuid: Option<[i32; 4]>,
        _player_uuid: Option<[i32; 4]>,
        tag: Tag,
    ) -> Result<(), Tag> {
        if !(kind == "minecraft:armor_stand") {
            return Err(tag);
        }
        game.load_armor_stand(tag, uuid);
        Ok(())
    }
    stored_motion!();
    fn selector_target(&self) -> Option<Target> {
        Some(Target::Stand(self.id))
    }
    /// `ArmorStand.ignoreExplosion`: a visible stand feels only a burst that
    /// affects block-like entities (`items_ignore` is that flag, negated).
    fn ignores_explosion(&self, blast: &Blast) -> bool {
        self.invisible || blast.items_ignore
    }
    /// `EntityType.ARMOR_STAND`'s size, halved when small; a marker has none.
    fn explosion_box(&self) -> Aabb {
        let (w, h) = armor_stand_size(self);
        let half = w / 2.0;
        let p = self.position;
        Aabb::new(
            Vec3::new(p.x - half, p.y, p.z - half),
            Vec3::new(p.x + half, p.y + h, p.z + half),
        )
    }
    fn explosion_eye_height(&self) -> f64 {
        if self.marker {
            0.0
        } else if self.small {
            f64::from(0.9875f32)
        } else {
            f64::from(1.7775f32)
        }
    }
    /// A burst of wind pushes a stand that a blast only breaks.
    fn pushed_by_wind(&self) -> bool {
        true
    }
    fn bounds(&self) -> Aabb {
        ArmorStand::bounds(self)
    }
    fn on_ground(&self) -> bool {
        self.on_ground
    }
    /// A living entity's own flag too: keeping the old support flag would run
    /// Frost Walker while the stand is moving over open water.
    fn set_piston_on_ground(&mut self, on_ground: bool) {
        self.on_ground = on_ground;
    }
    fn is_living(&self) -> bool {
        true
    }
    /// `ArmorStand.isIgnoringBlockTriggers`.
    fn is_ignoring_block_triggers(&self) -> bool {
        self.marker
    }
    /// A NoGravity stand is not swept for the blocks it stands in (plates,
    /// fire): measured beside the real server, it never presses a plate nor
    /// catches fire, and does again once `NoGravity` is cleared.
    fn sweeps_blocks_inside(&self) -> bool {
        !self.no_gravity
    }
    fn pushed_by_explosion(&self) -> bool {
        false
    }
    /// `ArmorStand.getPistonPushReaction`: a marker is ignored.
    fn get_piston_push_reaction(&self) -> PushReaction {
        if self.marker {
            PushReaction::IgnoreEntity
        } else {
            PushReaction::PushPull
        }
    }
}

impl Entity for PrimedTnt {
    fn set_on_ground(&mut self, on_ground: bool) {
        self.on_ground = on_ground;
    }
    fn rotation(&self) -> [f32; 2] {
        self.rotation
    }
    fn set_rotation(&mut self, [yaw, pitch]: [f32; 2]) {
        let previous = self.rotation();
        let yaw = if yaw.is_finite() { yaw } else { previous[0] };
        let pitch = if pitch.is_nan() { previous[1] } else { pitch };
        self.rotation = [yaw, pitch.clamp(-90.0, 90.0)];
    }
    fn ticks_after_projectiles(&self) -> bool {
        true
    }
    fn native_type(&self) -> Cow<'static, str> {
        "minecraft:tnt".into()
    }
    fn save_nbt(
        game: &mut Game<'_>,
        index: usize,
        keys: &HashSet<ChunkKey>,
    ) -> Option<(ChunkKey, Tag)> {
        game.save_tnt(index, keys)
    }
    fn load_nbt(
        game: &mut Game<'_>,
        kind: &str,
        uuid: Option<[i32; 4]>,
        _player_uuid: Option<[i32; 4]>,
        tag: Tag,
    ) -> Result<(), Tag> {
        if !loads_as_tnt(kind) {
            return Err(tag);
        }
        game.load_tnt(tag, uuid);
        Ok(())
    }
    stored_motion!();
    /// `EntityTypes.TNT`'s explicit `eyeHeight(0.15F)`. Nothing vanilla reads
    /// for TNT depends on it (only `isEyeInFluid` does), but it is the type's.
    fn fluid_eye_y(&self) -> f64 {
        self.position.y + f64::from(0.15_f32)
    }
    /// `PrimedTnt` is 0.98 across.
    fn portal_size(&self) -> Option<(f32, f32)> {
        Some((0.98, 0.98))
    }
    fn can_use_portal(&self, game: &Game<'_>) -> bool {
        game.tnt_can_use_portal(self.id)
    }
    fn selector_target(&self) -> Option<Target> {
        Some(Target::Tnt(self.id))
    }
    /// Pushed from its position, not its eyes.
    fn explosion_eye_height(&self) -> f64 {
        0.0
    }
    fn bounds(&self) -> Aabb {
        PrimedTnt::bounds(self)
    }
    fn on_ground(&self) -> bool {
        self.on_ground
    }
    fn set_piston_on_ground(&mut self, on_ground: bool) {
        self.on_ground = on_ground;
    }
    fn piston_slime_bounce(&mut self, movement: Direction) {
        self.velocity = piston_push::bounce_velocity(movement, self.velocity);
        // Leave the ground so the next tick does not apply the on-ground
        // Y * -0.5 friction that cancels the launch.
        self.on_ground = false;
    }
}

impl Entity for FallingBlock {
    fn rotation(&self) -> [f32; 2] {
        self.rotation
    }
    fn set_rotation(&mut self, [yaw, pitch]: [f32; 2]) {
        let previous = self.rotation();
        let yaw = if yaw.is_finite() { yaw } else { previous[0] };
        let pitch = if pitch.is_nan() { previous[1] } else { pitch };
        self.rotation = [yaw, pitch.clamp(-90.0, 90.0)];
    }
    fn on_ground(&self) -> bool {
        self.on_ground
    }
    /// `Entity.move(MoverType.PISTON)` ends in `setOnGroundWithMovement`.
    fn set_piston_on_ground(&mut self, on_ground: bool) {
        self.on_ground = on_ground;
    }
    fn set_on_ground(&mut self, on_ground: bool) {
        self.on_ground = on_ground;
    }
    fn ticks_after_projectiles(&self) -> bool {
        true
    }
    fn native_type(&self) -> Cow<'static, str> {
        "minecraft:falling_block".into()
    }
    fn save_nbt(
        game: &mut Game<'_>,
        index: usize,
        keys: &HashSet<ChunkKey>,
    ) -> Option<(ChunkKey, Tag)> {
        game.save_falling(index, keys)
    }
    fn load_nbt(
        game: &mut Game<'_>,
        kind: &str,
        uuid: Option<[i32; 4]>,
        _player_uuid: Option<[i32; 4]>,
        tag: Tag,
    ) -> Result<(), Tag> {
        if !(kind == "minecraft:falling_block") {
            return Err(tag);
        }
        game.load_falling(tag, uuid);
        Ok(())
    }
    stored_motion!();
    fn selector_target(&self) -> Option<Target> {
        Some(Target::Falling(self.id))
    }
    fn bounds(&self) -> Aabb {
        horizon_client::falling_block::bounds(self.position)
    }
}

impl Entity for Projectile {
    fn rotation(&self) -> [f32; 2] {
        [self.yaw, self.pitch]
    }
    fn set_rotation(&mut self, [yaw, pitch]: [f32; 2]) {
        let previous = self.rotation();
        let yaw = if yaw.is_finite() { yaw } else { previous[0] };
        let pitch = if pitch.is_nan() { previous[1] } else { pitch };
        self.yaw = yaw;
        self.pitch = pitch.clamp(-90.0, 90.0);
        self.previous_yaw = self.yaw;
        self.previous_pitch = self.pitch;
    }
    fn on_ground(&self) -> bool {
        self.on_ground
    }
    /// `Entity.move(MoverType.PISTON)` ends in `setOnGroundWithMovement`.
    fn set_piston_on_ground(&mut self, on_ground: bool) {
        self.on_ground = on_ground;
    }
    fn set_on_ground(&mut self, on_ground: bool) {
        self.on_ground = on_ground;
    }
    fn native_type(&self) -> Cow<'static, str> {
        self.entity_type().into()
    }
    fn save_nbt(
        game: &mut Game<'_>,
        index: usize,
        keys: &HashSet<ChunkKey>,
    ) -> Option<(ChunkKey, Tag)> {
        game.save_projectile(index, keys)
    }
    fn load_nbt(
        game: &mut Game<'_>,
        kind: &str,
        uuid: Option<[i32; 4]>,
        player_uuid: Option<[i32; 4]>,
        tag: Tag,
    ) -> Result<(), Tag> {
        match kind {
            "minecraft:arrow" | "minecraft:spectral_arrow" => {
                game.load_arrow(tag, uuid, player_uuid)
            }
            "minecraft:llama_spit" | "minecraft:shulker_bullet" => {
                game.load_spit_or_bullet(tag, uuid, player_uuid)
            }
            "minecraft:fireball"
            | "minecraft:small_fireball"
            | "minecraft:dragon_fireball"
            | "minecraft:wind_charge"
            | "minecraft:breeze_wind_charge"
            | "minecraft:wither_skull" => game.load_fireball(tag, uuid, player_uuid),
            id if horizon_client::entity_nbt::throwable_kind(id).is_some() => {
                game.load_throwable(tag, uuid, player_uuid)
            }
            _ => return Err(tag),
        }
        Ok(())
    }
    stored_motion!();
    /// Arrows and the thrown items and fireballs a chunk saves.
    fn portal_size(&self) -> Option<(f32, f32)> {
        (super::nether_portal::is_arrow(self) || super::nether_portal::is_portal_projectile(self))
            .then(|| super::nether_portal::projectile_box(self))
    }
    fn portal_from(&self) -> Option<Vec3> {
        Some(self.previous)
    }
    fn portal_sweeps(&self) -> bool {
        !super::nether_portal::is_arrow(self) || !self.stuck || self.previous != self.position
    }
    fn selector_target(&self) -> Option<Target> {
        use horizon_client::projectiles::ProjectileKind as K;
        // The kinds /data can write (entity_nbt).
        let selectable = self.is_arrow()
            || horizon_client::entity_nbt::saved_with_chunk(self)
            || matches!(
                self.kind,
                K::LargeFireball { .. }
                    | K::SmallFireball
                    | K::DragonFireball
                    | K::WindCharge
                    | K::BreezeWindCharge
            );
        selectable.then_some(Target::Projectile(self.id))
    }
    /// `AbstractArrow.push` does nothing in the ground; a wind charge's motion
    /// stays 0.
    fn pushed_by_explosion(&self) -> bool {
        !self.stuck && !self.kind.ignores_blast()
    }
    fn explosion_eye_height(&self) -> f64 {
        self.kind.eye_height()
    }
    fn push_from_explosion(&mut self, push: Vec3) {
        self.velocity = Vec3::new(
            self.velocity.x + push.x,
            self.velocity.y + push.y,
            self.velocity.z + push.z,
        );
        self.needs_sync = true;
    }
    fn bounds(&self) -> Aabb {
        Projectile::bounds(self)
    }
    fn move_by_piston(&mut self, movement: Vec3) {
        Projectile::move_by_piston(self, movement);
    }
    fn piston_release_bounds(&self) -> Option<Aabb> {
        Projectile::piston_release_bounds(self)
    }
    fn release_after_piston(&mut self, no_block_collision: bool) {
        Projectile::release_after_piston(self, no_block_collision);
    }
    fn piston_slime_bounce(&mut self, movement: Direction) {
        Projectile::piston_slime_bounce(self, movement);
    }
}

impl Entity for Rocket {
    fn rotation(&self) -> [f32; 2] {
        self.rotation
    }
    fn set_rotation(&mut self, [yaw, pitch]: [f32; 2]) {
        let previous = self.rotation();
        let yaw = if yaw.is_finite() { yaw } else { previous[0] };
        let pitch = if pitch.is_nan() { previous[1] } else { pitch };
        self.rotation = [yaw, pitch.clamp(-90.0, 90.0)];
    }
    fn on_ground(&self) -> bool {
        self.on_ground
    }
    /// `Entity.move(MoverType.PISTON)` ends in `setOnGroundWithMovement`.
    fn set_piston_on_ground(&mut self, on_ground: bool) {
        self.on_ground = on_ground;
    }
    fn set_on_ground(&mut self, on_ground: bool) {
        self.on_ground = on_ground;
    }
    fn native_type(&self) -> Cow<'static, str> {
        "minecraft:firework_rocket".into()
    }
    fn save_nbt(
        game: &mut Game<'_>,
        index: usize,
        keys: &HashSet<ChunkKey>,
    ) -> Option<(ChunkKey, Tag)> {
        game.save_rocket(index, keys)
    }
    fn load_nbt(
        game: &mut Game<'_>,
        kind: &str,
        uuid: Option<[i32; 4]>,
        player_uuid: Option<[i32; 4]>,
        tag: Tag,
    ) -> Result<(), Tag> {
        if !(kind == "minecraft:firework_rocket") {
            return Err(tag);
        }
        game.load_rocket(tag, uuid, player_uuid);
        Ok(())
    }
    stored_motion!();
    /// A rocket that carries its owner stays.
    fn portal_size(&self) -> Option<(f32, f32)> {
        (self.attached_to.is_none() && !self.attached).then_some((0.25, 0.25))
    }
    fn portal_from(&self) -> Option<Vec3> {
        Some(self.previous)
    }
    fn selector_target(&self) -> Option<Target> {
        Some(Target::Rocket(self.id))
    }
    /// 0.25 wide, so 0.2125 to the eyes.
    fn explosion_eye_height(&self) -> f64 {
        0.2125
    }
    fn bounds(&self) -> Aabb {
        Rocket::bounds(self)
    }
    /// `FireworkRocketEntity.tick`: boosting a player skips the step.
    fn sweeps_blocks_inside(&self) -> bool {
        !self.attached
    }
}

impl Entity for ThrownTrident {
    fn rotation(&self) -> [f32; 2] {
        [self.yaw, self.pitch]
    }
    fn set_rotation(&mut self, [yaw, pitch]: [f32; 2]) {
        let previous = self.rotation();
        let yaw = if yaw.is_finite() { yaw } else { previous[0] };
        let pitch = if pitch.is_nan() { previous[1] } else { pitch };
        self.yaw = yaw;
        self.pitch = pitch.clamp(-90.0, 90.0);
    }
    fn on_ground(&self) -> bool {
        self.on_ground
    }
    /// `Entity.move(MoverType.PISTON)` ends in `setOnGroundWithMovement`.
    fn set_piston_on_ground(&mut self, on_ground: bool) {
        self.on_ground = on_ground;
    }
    fn set_on_ground(&mut self, on_ground: bool) {
        self.on_ground = on_ground;
    }
    fn native_type(&self) -> Cow<'static, str> {
        "minecraft:trident".into()
    }
    fn save_nbt(
        game: &mut Game<'_>,
        index: usize,
        keys: &HashSet<ChunkKey>,
    ) -> Option<(ChunkKey, Tag)> {
        game.save_trident(index, keys)
    }
    fn load_nbt(
        game: &mut Game<'_>,
        kind: &str,
        uuid: Option<[i32; 4]>,
        _player_uuid: Option<[i32; 4]>,
        tag: Tag,
    ) -> Result<(), Tag> {
        if !(kind == "minecraft:trident") {
            return Err(tag);
        }
        game.load_trident(tag, uuid);
        Ok(())
    }
    stored_motion!();
    fn portal_size(&self) -> Option<(f32, f32)> {
        Some((0.5, 0.5))
    }
    fn portal_from(&self) -> Option<Vec3> {
        Some(self.previous)
    }
    fn portal_sweeps(&self) -> bool {
        !self.in_ground || self.previous != self.position
    }
    fn selector_target(&self) -> Option<Target> {
        Some(Target::Trident(self.id))
    }
    /// Like any arrow (`AbstractArrow.push`) not pushed while in the ground.
    fn pushed_by_explosion(&self) -> bool {
        !self.in_ground
    }
    /// `EntityType.TRIDENT`: eyes at 0.13.
    fn explosion_eye_height(&self) -> f64 {
        f64::from(0.13_f32)
    }
    fn bounds(&self) -> Aabb {
        let p = self.position;
        Aabb::new(
            Vec3::new(p.x - 0.25, p.y, p.z - 0.25),
            Vec3::new(p.x + 0.25, p.y + 0.5, p.z + 0.25),
        )
    }
    fn move_by_piston(&mut self, movement: Vec3) {
        ThrownTrident::move_by_piston(self, movement);
    }
    fn piston_release_bounds(&self) -> Option<Aabb> {
        ThrownTrident::piston_release_bounds(self)
    }
    fn release_after_piston(&mut self, no_block_collision: bool) {
        ThrownTrident::release_after_piston(self, no_block_collision);
    }
    fn piston_slime_bounce(&mut self, movement: Direction) {
        ThrownTrident::piston_slime_bounce(self, movement);
    }
    /// `GameLevel::tick_blocks` sweeps tridents itself.
    fn sweeps_blocks_inside(&self) -> bool {
        false
    }
}

impl EntityRules for Rocket {}
impl EntityRules for ThrownTrident {}
impl EntityRules for Boat {
    fn is_no_gravity(&self) -> bool {
        self.no_gravity
    }
    /// `AbstractBoat.getDefaultGravity`.
    fn get_default_gravity(&self) -> f64 {
        0.04
    }
}
impl EntityRules for Minecart {
    fn is_no_gravity(&self) -> bool {
        self.no_gravity
    }
    /// `AbstractMinecart.getDefaultGravity`.
    fn get_default_gravity(&self) -> f64 {
        if self.in_water { 0.005 } else { 0.04 }
    }
}
impl EntityRules for PrimedTnt {
    fn is_no_gravity(&self) -> bool {
        self.no_gravity
    }
    /// `PrimedTnt.getDefaultGravity`.
    fn get_default_gravity(&self) -> f64 {
        0.04
    }
}
impl EntityRules for FallingBlock {
    fn is_no_gravity(&self) -> bool {
        self.no_gravity
    }
    /// `FallingBlockEntity.getDefaultGravity`.
    fn get_default_gravity(&self) -> f64 {
        0.04
    }
}

#[cfg(test)]
mod no_gravity_stand_tests {
    use super::Entity as _;
    use horizon_client::entities::ArmorStand;
    use horizon_core::geometry::Vec3;

    #[test]
    fn a_no_gravity_stand_is_not_swept_for_the_blocks_it_is_in() {
        let mut stand = ArmorStand::new(1, Vec3::new(0.5, 100.0, 0.5), 0.0);
        // A resting stand presses a plate (measured beside the real server)...
        assert!(stand.sweeps_blocks_inside() && stand.triggers_blocks());
        // ...a NoGravity one does not, nor does a marker.
        stand.no_gravity = true;
        assert!(!stand.sweeps_blocks_inside());
        stand.no_gravity = false;
        stand.marker = true;
        assert!(!stand.triggers_blocks());
    }
}

#[cfg(test)]
mod load_claim_tests {
    /// `Mob` is first in `entity_lists!` and claims every id in `mob_types`;
    /// the old load tried every other kind first. Both agree only while no
    /// mob species shares an id with another kind, which this checks.
    #[test]
    fn no_mob_species_is_an_id_another_kind_loads() {
        let others = [
            "minecraft:item",
            "minecraft:experience_orb",
            "minecraft:arrow",
            "minecraft:spectral_arrow",
            "minecraft:llama_spit",
            "minecraft:shulker_bullet",
            "minecraft:fireball",
            "minecraft:small_fireball",
            "minecraft:wind_charge",
            "minecraft:breeze_wind_charge",
            "minecraft:falling_block",
            "minecraft:trident",
            "minecraft:firework_rocket",
            "minecraft:tnt",
            "minecraft:armor_stand",
            "minecraft:ominous_item_spawner",
            "minecraft:ender_dragon",
            "minecraft:end_crystal",
            "minecraft:item_frame",
            "minecraft:glow_item_frame",
            "minecraft:cushion",
            "minecraft:painting",
        ];
        for def in horizon_client::mobs::species::ALL {
            let id = format!("minecraft:{}", def.name);
            assert!(!others.contains(&id.as_str()), "{id}");
            assert!(
                horizon_client::entity_nbt::throwable_kind(&id).is_none(),
                "{id}"
            );
            assert!(super::super::boats::any_boat_wood(&id).is_none(), "{id}");
            assert!(
                super::super::minecarts::CartKind::from_id(&id).is_none(),
                "{id}"
            );
        }
    }
}

#[cfg(test)]
mod trident_selector_tests {
    use super::Entity as _;
    use crate::game::{commands::Target, trident::ThrownTrident};
    use horizon_core::geometry::Vec3;

    /// `@e`, `/kill` and `/data` reach a thrown trident like any other entity
    /// (`EntityTypeTest.forClass(Entity.class)`: every entity in the level).
    #[test]
    fn a_thrown_trident_is_a_selector_target() {
        let t = ThrownTrident::remote(9, Vec3::new(0.5, 100.0, 0.5), 0.0, 0.0);
        assert_eq!(t.selector_target(), Some(Target::Trident(9)));
    }
}

#[cfg(test)]
mod join_order_tests {
    use super::{EntityHandle, EntityLists, JoinOrderError};
    use crate::game::{boats::Boat, fireworks::Rocket, trident::ThrownTrident};
    use horizon_client::entities::ArmorStand;
    use horizon_client::join_seq::JoinList;
    use horizon_core::geometry::Vec3;

    fn at() -> Vec3 {
        Vec3::new(0.5, 100.0, 0.5)
    }

    struct Lists {
        stands: JoinList<ArmorStand>,
        tridents: JoinList<ThrownTrident>,
        rockets: JoinList<Rocket>,
        boats: JoinList<Boat>,
    }

    impl Lists {
        fn view(&self) -> EntityLists<'_> {
            EntityLists {
                mobs: &[],
                dropped: &[],
                boats: &self.boats,
                carts: &[],
                armor_stands: &self.stands,
                tnts: &[],
                falling: &[],
                orbs: &[],
                projectiles: &[],
                rockets: &self.rockets,
                tridents: &self.tridents,
            }
        }
        fn walk(&self) -> Result<Vec<EntityHandle>, JoinOrderError> {
            let mut seen = Vec::new();
            self.view().walk_in_join_order(|h, _| seen.push(h))?;
            Ok(seen)
        }
    }

    /// Joins five kinds' entities interleaved, the way a level gets them.
    fn mixed() -> (Lists, Vec<EntityHandle>) {
        let mut l = Lists {
            stands: JoinList::default(),
            tridents: JoinList::default(),
            rockets: JoinList::default(),
            boats: JoinList::default(),
        };
        l.trident(1);
        l.rocket(2);
        l.stand(3);
        l.boat(4);
        l.trident(5);
        l.stand(6);
        l.rocket(7);
        let joined = vec![
            EntityHandle::Trident(0),
            EntityHandle::Rocket(0),
            EntityHandle::ArmorStand(0),
            EntityHandle::Boat(0),
            EntityHandle::Trident(1),
            EntityHandle::ArmorStand(1),
            EntityHandle::Rocket(1),
        ];
        (l, joined)
    }

    impl Lists {
        fn stand(&mut self, id: u32) {
            self.stands.push(ArmorStand::new(id, at(), 0.0));
        }
        fn trident(&mut self, id: u32) {
            self.tridents
                .push(ThrownTrident::remote(id, at(), 0.0, 0.0));
        }
        fn rocket(&mut self, id: u32) {
            self.rockets.push(Rocket::remote(id, at(), Vec3::ZERO));
        }
        fn boat(&mut self, id: u32) {
            self.boats.push(Boat::new(id, "oak".into(), at(), 0.0));
        }
    }

    /// The walk visits entities in the order they joined, not list by list.
    #[test]
    fn mixed_kinds_are_visited_in_join_order() {
        let (lists, joined) = mixed();
        assert_eq!(lists.walk().unwrap(), joined);
        // Walked list by list they would come in another order.
        assert_ne!(joined[0], EntityHandle::ArmorStand(0));
    }

    /// An entity taken out and put back (a tick takes its list) keeps its place.
    #[test]
    fn an_entity_put_back_keeps_its_join_number() {
        let (mut lists, joined) = mixed();
        let taken = std::mem::take(&mut lists.tridents);
        let before: Vec<_> = taken
            .iter()
            .map(horizon_client::join_seq::Joined::join_seq)
            .collect();
        let mut back = JoinList::default();
        for t in taken {
            back.push(t);
        }
        lists.tridents = back;
        let after: Vec<_> = lists
            .tridents
            .iter()
            .map(horizon_client::join_seq::Joined::join_seq)
            .collect();
        assert_eq!(before, after);
        assert_eq!(lists.walk().unwrap(), joined);
    }

    /// The walk is checked: a list out of join order, or an entity that never
    /// went through the list's `push`, is reported and not walked past.
    #[test]
    fn the_walk_reports_a_list_out_of_order_and_an_entity_that_never_joined() {
        let (mut lists, _) = mixed();
        lists.tridents.swap(0, 1);
        assert!(matches!(lists.walk(), Err(JoinOrderError::OutOfOrder(_))));
        let (mut lists, _) = mixed();
        Vec::push(&mut lists.stands, ArmorStand::new(9, at(), 0.0));
        assert_eq!(
            lists.walk(),
            Err(JoinOrderError::Unjoined(EntityHandle::ArmorStand(2)))
        );
    }
}

#[cfg(test)]
mod set_rotation_tests {
    use super::Entity as _;
    use horizon_client::entities::ExperienceOrb;

    /// `Entity.teleportSetPosition`: yaw as given, pitch clamped to -90..90, on
    /// every kind (the ones with no field of their own used to drop it).
    #[test]
    fn set_rotation_keeps_yaw_and_clamps_pitch_on_kinds_that_had_none() {
        let mut orb = ExperienceOrb {
            join: Default::default(),
            no_gravity: false,
            id: 0,
            position: horizon_core::geometry::Vec3::ZERO,
            previous: horizon_core::geometry::Vec3::ZERO,
            velocity: horizon_core::geometry::Vec3::ZERO,
            value: 1,
            age: 0,
            on_ground: false,
            count: 1,
            ticks: 0,
            health: 5,
            rotation: [0.0, 0.0],
            following: None,
        };
        orb.set_rotation([90.0, 10.0]);
        assert_eq!(orb.rotation(), [90.0, 10.0]);
        orb.set_rotation([0.0, 200.0]);
        assert_eq!(orb.rotation(), [0.0, 90.0]);
    }
}
