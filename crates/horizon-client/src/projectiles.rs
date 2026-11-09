//! Projectiles fired by mobs and thrown by the player: arrows
//! (`AbstractArrow`, gravity 0.05), throwables (`ThrowableProjectile`:
//! snowballs, eggs, ender pearls, gravity 0.03) and fireballs
//! (`AbstractHurtingProjectile`: no gravity, 0.1 acceleration, 0.95 inertia).
//! The game applies the effects of a hit.
use crate::entity_rules::EntityRules;
use crate::mobs::Who;
use horizon_core::{
    geometry::{Aabb, Direction, Vec3},
    position::BlockPos,
    random::RandomSource,
};

/// `AbstractArrow.startFalling`'s per-axis random velocity damping.
pub fn start_falling_velocity(
    velocity: Vec3,
    random: &mut horizon_core::random::LegacyRandom,
) -> Vec3 {
    let x = f64::from(random.next_float() * 0.2);
    let y = f64::from(random.next_float() * 0.2);
    let z = f64::from(random.next_float() * 0.2);
    Vec3::new(velocity.x * x, velocity.y * y, velocity.z * z)
}

/// a content mod's thrown item projectile, a copy of `Snowball` that deals
/// `damage` and flies as its `item`.
#[derive(Debug, PartialEq)]
pub struct ThrownType {
    /// `<mod id>:<path>`.
    pub id: &'static str,
    pub item: &'static str,
    pub damage: f32,
}
/// the thrown types content mods declare, in registry order.
static THROWN_TYPES: std::sync::OnceLock<Vec<ThrownType>> = std::sync::OnceLock::new();
/// registers the content mods' thrown types, `(id, item, damage)`, a list
/// set once per process (false when it already was).
pub fn register_thrown_types(types: impl IntoIterator<Item = (String, String, f32)>) -> bool {
    if THROWN_TYPES.get().is_some() {
        return false;
    }
    let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    let types = types.into_iter().map(|(id, item, damage)| ThrownType {
        id: leak(id),
        item: leak(item),
        damage,
    });
    THROWN_TYPES.set(types.collect()).is_ok()
}
/// the content mods' thrown types, in registry order.
pub fn thrown_types() -> &'static [ThrownType] {
    THROWN_TYPES.get().map_or(&[], Vec::as_slice)
}
/// the lib's tests' one mod type, `ruby:bolt` thrown from `ruby:gem`: the
/// registry is set once per process, so every test registers the same.
#[cfg(test)]
pub(crate) fn register_test_types() {
    let _ = register_thrown_types([("ruby:bolt".into(), "ruby:gem".into(), 2.0)]);
}
/// the mod type with entity type `id`; out of line, as vanilla's never are.
#[cold]
#[inline(never)]
pub fn thrown_type(id: &str) -> Option<&'static ThrownType> {
    thrown_types().iter().find(|t| t.id == id)
}
/// the mod type `item` throws (the first in registry order).
#[cold]
#[inline(never)]
pub fn thrown_by_item(item: &str) -> Option<&'static ThrownType> {
    thrown_types().iter().find(|t| t.item == item)
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProjectileKind {
    /// Arrow with its base damage, critical flag and tipped/stray/bogged effect.
    Arrow {
        base_damage: f64,
        critical: bool,
        effect: Option<(&'static str, i32)>,
    },
    /// `ThrownTrident` of a mob (a drowned's): flies and sticks with
    /// `AbstractArrow`'s tick; the trident itself is the arrow state's
    /// `pickup_item`.
    Trident,
    Snowball,
    Egg,
    EnderPearl,
    SmallFireball,
    LargeFireball {
        power: i32,
    },
    /// `DragonFireball`: an `AbstractHurtingProjectile` that bursts into
    /// dragon's breath, does not burn and trails dragon breath.
    DragonFireball,
    LlamaSpit,
    /// A player's `WindCharge` and a breeze's `BreezeWindCharge`: straight
    /// flight that bursts on whatever it meets.
    WindCharge,
    BreezeWindCharge,
    /// `ShulkerBullet`: homing on `target` in axis-aligned steps; one with
    /// no target (a summoned one) falls like any projectile.
    ShulkerBullet {
        target: Option<Who>,
        /// The current step direction (3D data value), if any.
        dir: Option<u8>,
        steps: i32,
        delta: [f64; 3],
        seed: u64,
    },
    /// `WitherSkull`: blue (dangerous) skulls keep their speed better.
    WitherSkull {
        dangerous: bool,
    },
    /// `ThrownSplashPotion` or `ThrownLingeringPotion`: the potion item it
    /// flies as (`ThrowableItemProjectile.getItem`). What it splashes is that
    /// stack's `potion_contents`, so a witch's potion and a summoned one are
    /// the same thing; its entity type is the stack's item.
    ///
    /// The `bool` is whether its entity type is `ThrownLingeringPotion`: the
    /// type is the entity's own and `/summon` can pair either with any `Item`,
    /// so it is kept next to the stack (a thrown potion's follows its item).
    Potion(Box<crate::items::ItemStack>, bool),
    /// `ThrownExperienceBottle`: its own entity type, gravity 0.07.
    ExperienceBottle,
    /// a content mod's type.
    Thrown(&'static ThrownType),
}
impl ProjectileKind {
    /// A thrown potion flying as `stack`: a lingering item makes the lingering
    /// entity type, any other the splash one.
    pub fn potion(stack: crate::items::ItemStack) -> Self {
        let lingering = &*stack.item == "minecraft:lingering_potion";
        Self::Potion(Box::new(stack), lingering)
    }
    /// Wind charges take no push from an explosion (vanilla leaves their motion at 0).
    pub fn ignores_blast(&self) -> bool {
        matches!(self, Self::WindCharge | Self::BreezeWindCharge)
    }
    /// Whether the kind is a `Fireball` proper (`LargeFireball`,
    /// `SmallFireball`): the projectiles that carry an `Item`
    /// (`Fireball.DATA_ITEM_STACK`) and are drawn as it. A `DragonFireball`
    /// and a `WitherSkull` extend the hurting projectile only.
    pub fn is_item_fireball(&self) -> bool {
        matches!(self, Self::SmallFireball | Self::LargeFireball { .. })
    }
    /// `EntityType.Builder.eyeHeight`: 0.13 for arrows and tridents, 0 for
    /// wind charges, else the default 0.85 of the box height.
    pub fn eye_height(&self) -> f64 {
        match self {
            Self::Arrow { .. } | Self::Trident => f64::from(0.13_f32),
            Self::WindCharge | Self::BreezeWindCharge => 0.0,
            _ => f64::from(self.size() as f32 * 0.85_f32),
        }
    }
    /// The width and height of its entity type's `sized(...)`.
    pub fn size(&self) -> f64 {
        match self {
            Self::Arrow { .. } | Self::Trident => 0.5,
            Self::Snowball
            | Self::Egg
            | Self::EnderPearl
            | Self::LlamaSpit
            | Self::Potion(..)
            | Self::ExperienceBottle
            | Self::Thrown(_) => 0.25,
            Self::SmallFireball
            | Self::WindCharge
            | Self::BreezeWindCharge
            | Self::ShulkerBullet { .. }
            | Self::WitherSkull { .. } => 0.3125,
            Self::LargeFireball { .. } | Self::DragonFireball => 1.0,
        }
    }
    /// Its entity type.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Arrow { .. } => "minecraft:arrow",
            Self::Trident => "minecraft:trident",
            Self::Snowball => "minecraft:snowball",
            Self::Egg => "minecraft:egg",
            Self::EnderPearl => "minecraft:ender_pearl",
            Self::SmallFireball => "minecraft:small_fireball",
            Self::LargeFireball { .. } => "minecraft:fireball",
            Self::DragonFireball => "minecraft:dragon_fireball",
            Self::LlamaSpit => "minecraft:llama_spit",
            Self::WindCharge => "minecraft:wind_charge",
            Self::BreezeWindCharge => "minecraft:breeze_wind_charge",
            Self::ShulkerBullet { .. } => "minecraft:shulker_bullet",
            Self::WitherSkull { .. } => "minecraft:wither_skull",
            // `ThrownSplashPotion`/`ThrownLingeringPotion`: the item names it.
            Self::Potion(_, true) => "minecraft:lingering_potion",
            Self::Potion(_, false) => "minecraft:splash_potion",
            Self::ExperienceBottle => "minecraft:experience_bottle",
            Self::Thrown(t) => t.id,
        }
    }
}
/// `ThrownItemRenderer`'s sprite for a thrown potion.
fn potion_sprite(item: &str) -> &'static str {
    if item == "minecraft:lingering_potion" {
        "item/lingering_potion"
    } else {
        "item/splash_potion"
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Projectile {
    /// When it joined its list ([`crate::join_seq`]); set by the list, not by the spawn site.
    pub join: crate::join_seq::JoinSeq,
    /// The entity id a server tracks it by (0 until it is tracked).
    pub id: u32,
    pub kind: ProjectileKind,
    pub owner: Option<Who>,
    /// `Projectile.owner`'s UUID, kept while the owner is away (a guest who
    /// left, a mob not loaded): what its save writes as `Owner`.
    pub owner_uuid: Option<[i32; 4]>,
    /// On a server, the guest (by connection) who shot it: `Who::Player`
    /// then means them, not the host.
    pub guest: Option<u32>,
    pub position: Vec3,
    pub previous: Vec3,
    pub velocity: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    /// `Entity.yRotO`/`xRotO`: the rotation the last tick ended with, which
    /// the renderer interpolates from (`setOldPosAndRot`).
    pub previous_yaw: f32,
    pub previous_pitch: f32,
    pub age: u32,
    pub stuck: bool,
    /// `Entity.stuckSpeedMultiplier`: set by a cobweb or powder snow its box
    /// passed through on a block's push (`makeStuckInBlock`); the next push
    /// (`Entity.move`) then stops it dead. Zero: not stuck.
    pub stuck_speed: Vec3,
    /// Whether the projectile has left its owner's box (Projectile.leftOwner).
    pub left_owner: bool,
    /// Non-arrow Projectile.hasBeenShot, independent of entity age.
    pub has_been_shot: bool,
    shoot_event: Option<Vec3>,
    /// The shooter's bow enchantments (Power, Punch, Flame, Piercing).
    pub bonus: crate::mobs::gear::ArrowBonus,
    /// Arrows' `AbstractArrow` state (ticked by `arrows::tick`).
    pub arrow: Option<Box<crate::arrows::ArrowState>>,
    /// `AbstractHurtingProjectile.accelerationPower`.
    pub acceleration: f64,
    /// `Entity.needsSync`: a server tells its clients about the change this
    /// tick instead of at the next update interval.
    pub needs_sync: bool,
    /// The bubble column tops this tick's `onAboveBubbleColumn` pushes came
    /// from, for their particles (`sendBubbleColumnParticles`); the caller
    /// drains them.
    pub bubble_surfaces: Vec<[i32; 3]>,
    /// `Entity.wasTouchingWater`: what the last tick's
    /// `updateFluidInteraction` found around the box after the move, read by
    /// the next tick's inertia (false until the first tick has moved it).
    pub touching_water: bool,
    /// `Entity.noGravity` (`NoGravity` in its save): `getGravity` is 0.
    pub no_gravity: bool,
    /// `Entity.onGround` (`OnGround` in its save). A projectile moves by
    /// `setPos`, never `Entity.move`, so it keeps what it was loaded with.
    pub on_ground: bool,
    /// `Entity.remainingFireTicks`: a thrown projectile that flew through a
    /// fire block burns for 160 ticks, with the flame drawn over it.
    pub fire_ticks: i32,
    /// `Entity.DATA_AIR_SUPPLY_ID`, saved as a short but tracked as an int.
    pub air_supply: i32,
    /// `Entity.hasVisualFire`: an optional saved flag independent of burning.
    pub has_visual_fire: bool,
    /// `Entity.fallDistance` (`fall_distance` in its save): a projectile sets
    /// its position without `Entity.move`, so it keeps what it was given.
    pub fall_distance: f64,
    /// `Entity.DATA_SHARED_FLAGS_ID`'s fire bit, set by the server's base tick.
    pub synced_fire: bool,
    /// `Fireball.DATA_ITEM_STACK`: the item a fireball is drawn as, a fire
    /// charge until [`Projectile::set_item`] gives it another. None for the
    /// kinds that are no `Fireball` (a wither skull, a wind charge).
    pub item: Option<crate::items::ItemStack>,
}
/// `ThrownEnderpearl.isAllowedToTeleportOwner(owner, level)` for an owner in
/// the pearl's own dimension: vanilla reaches
/// `LivingEntity.isAlive() && !LivingEntity.isSleeping()` (bytecode 26-45) and
/// returns false otherwise, so a pearl whose owner died or went to bed is
/// discarded where it lands: no teleport, no 5 damage, no endermite roll.
pub fn owner_can_take_pearl(alive: bool, sleeping: bool) -> bool {
    alive && !sleeping
}
/// What a projectile struck this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    None,
    Block {
        pos: [i32; 3],
        at: Vec3,
        face: [i32; 3],
    },
    Entity {
        who: Who,
        at: Vec3,
    },
}
/// The world a projectile flies through: block ray clipping, water, and
/// the boxes of entities it could hit.
/// The speed multiplier `makeStuckInBlock` gives an entity in block `name`
/// (`WebBlock.entityInside`, `PowderSnowBlock.entityInside`), if any.
pub fn stuck_speed_of(name: &str) -> Option<Vec3> {
    match name {
        "minecraft:cobweb" => Some(Vec3::new(0.25, f64::from(0.05_f32), 0.25)),
        "minecraft:powder_snow" => Some(Vec3::new(f64::from(0.9_f32), 1.5, f64::from(0.9_f32))),
        _ => None,
    }
}
pub trait ProjectileWorld {
    fn clip(&self, from: Vec3, to: Vec3) -> Option<(Vec3, [i32; 3], [i32; 3])>;
    /// `Entity.isInWater` as `updateFluidInteraction` sets it: some water
    /// block's surface reaches the bottom of the box (deflated by 0.001).
    fn in_water(&self, bounds: Aabb) -> bool;
    fn targets(&self) -> &[(Who, Aabb)];
    /// Server-only mod-block contacts along the completed movement.
    fn block_contacts(&self, _entity: i32, _kind: &str, _from: Vec3, _to: Vec3, _bounds: Aabb) {}
    /// `Level.isEmptyBlock`.
    fn empty(&self, _p: [i32; 3]) -> bool {
        true
    }
    /// `Level.loadedAndEntityCanStandOn(p, projectile)` with the projectile's
    /// feet at `entity_y`: the top face of the block's collision shape is
    /// full (`Block.isFaceFull`), as on a top slab, not only a full cube.
    fn can_stand_on(&self, _p: [i32; 3], _entity_y: f64) -> bool {
        false
    }
    /// The block at `p` is a `BaseFireBlock` (fire, soul fire) whose
    /// `entityInside` ignites an entity in it for 160 ticks
    /// (`BaseFireBlock.fireIgnite`).
    fn ignites(&self, _p: [i32; 3]) -> bool {
        false
    }
    /// `BubbleColumnBlock.entityInside`'s view of the block at `p`: a bubble
    /// column's `drag`, and whether nothing is above it (its surface, where
    /// it throws harder); None for any other block.
    fn bubble_column(&self, _p: [i32; 3]) -> Option<(bool, bool)> {
        None
    }
    /// `entityInside`'s `makeStuckInBlock` for the block at `p`: a cobweb's
    /// or powder snow's speed multiplier, None for any other block.
    fn stuck_speed(&self, _p: [i32; 3]) -> Option<Vec3> {
        None
    }
}
/// `CollisionGetter.clipIncludingBorder` over a block clip's result: a ray
/// from inside the world border whose hit (or end, on a miss) lies outside it
/// stops on the border instead, at `WorldBorder.clampVec3ToBound` of that
/// point, facing `Direction.getApproximateNearest` of the travel. `bounds` is
/// the border's [minX, minZ, maxX, maxZ].
pub fn clip_including_border(
    hit: Option<(Vec3, [i32; 3], [i32; 3])>,
    from: Vec3,
    to: Vec3,
    bounds: [f64; 4],
) -> Option<(Vec3, [i32; 3], [i32; 3])> {
    let [min_x, min_z, max_x, max_z] = bounds;
    // WorldBorder.isWithinBounds(x, z): the far edges are outside.
    let within = |p: Vec3| p.x >= min_x && p.x < max_x && p.z >= min_z && p.z < max_z;
    let end = hit.map_or(to, |h| h.0);
    if !within(from) || within(end) {
        return hit;
    }
    // Mth.clamp, up to 1.0E-5F inside the far edges.
    let clamp = |v: f64, low: f64, high: f64| if v < low { low } else { v.min(high) };
    let inset = f64::from(1.0e-5_f32);
    let at = Vec3::new(
        clamp(end.x, min_x, max_x - inset),
        end.y,
        clamp(end.z, min_z, max_z - inset),
    );
    let face =
        Direction::approximate_nearest(Vec3::new(end.x - from.x, end.y - from.y, end.z - from.z));
    let pos = BlockPos::containing(at);
    Some((at, [pos.x, pos.y, pos.z], face.normal()))
}
/// Direction steps by 3D data value: down, up, north, south, west, east.
pub const STEPS: [[i32; 3]; 6] = [
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
    [-1, 0, 0],
    [1, 0, 0],
];
fn axis_of(d: u8) -> u8 {
    match d {
        0 | 1 => 1,
        2 | 3 => 2,
        _ => 0,
    }
}
impl crate::entity_rules::EntityRules for Projectile {
    fn is_no_gravity(&self) -> bool {
        self.no_gravity
    }
    /// Each projectile class's `getDefaultGravity`.
    fn get_default_gravity(&self) -> f64 {
        match &self.kind {
            ProjectileKind::Arrow { .. } | ProjectileKind::Trident => 0.05,
            ProjectileKind::SmallFireball
            | ProjectileKind::LargeFireball { .. }
            | ProjectileKind::DragonFireball
            | ProjectileKind::WitherSkull { .. } => 0.0,
            ProjectileKind::LlamaSpit => 0.06,
            // `ThrownExperienceBottle.getDefaultGravity` is 0.07.
            ProjectileKind::ExperienceBottle => 0.07,
            ProjectileKind::Potion { .. } => 0.05,
            ProjectileKind::ShulkerBullet { .. } => 0.04,
            _ => 0.03,
        }
    }
}
/// `AbstractHurtingProjectile.applyInertia` / `ThrowableProjectile.applyInertia`:
/// four bubble particles at `position - movement * 0.25`, moving with `movement`.
pub fn water_bubble_at(position: [f64; 3], movement: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    (
        [
            position[0] - movement[0] * 0.25,
            position[1] - movement[1] * 0.25,
            position[2] - movement[2] * 0.25,
        ],
        movement,
    )
}
impl Projectile {
    /// `Entity.getBoundingBox`: its type's square box standing on its position,
    /// a wind charge's 0.15 lower (`AbstractWindCharge.makeBoundingBox`).
    pub fn bounds(&self) -> Aabb {
        let (p, half) = (self.position, self.kind.size() / 2.0);
        let height = self.kind.size();
        let bottom = match self.kind {
            ProjectileKind::WindCharge | ProjectileKind::BreezeWindCharge => {
                p.y - f64::from(0.15_f32)
            }
            _ => p.y,
        };
        Aabb::new(
            Vec3::new(p.x - half, bottom, p.z - half),
            Vec3::new(p.x + half, bottom + height, p.z + half),
        )
    }
    pub fn new(kind: ProjectileKind, position: Vec3, owner: Option<Who>) -> Self {
        let kind_is_arrow = matches!(kind, ProjectileKind::Arrow { .. } | ProjectileKind::Trident);
        // `Fireball.defineSynchedData`: DATA_ITEM_STACK starts as a fire charge.
        let item = kind
            .is_item_fireball()
            .then(|| crate::items::ItemStack::new("minecraft:fire_charge", 1));
        // `AbstractWindCharge`'s constructors set it to 0.
        let acceleration = if matches!(
            kind,
            ProjectileKind::WindCharge | ProjectileKind::BreezeWindCharge
        ) {
            0.0
        } else {
            0.1
        };
        Self {
            join: Default::default(),
            id: 0,
            kind,
            owner,
            owner_uuid: None,
            guest: None,
            position,
            previous: position,
            velocity: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            previous_yaw: 0.0,
            previous_pitch: 0.0,
            age: 0,
            stuck: false,
            stuck_speed: Vec3::ZERO,
            left_owner: false,
            has_been_shot: false,
            shoot_event: None,
            bonus: Default::default(),
            acceleration,
            needs_sync: false,
            bubble_surfaces: Vec::new(),
            touching_water: false,
            no_gravity: false,
            on_ground: false,
            fire_ticks: 0,
            air_supply: 300,
            has_visual_fire: false,
            fall_distance: 0.0,
            synced_fire: false,
            item,
            arrow: kind_is_arrow.then(|| {
                // The arrow's own random (`Entity.random`).
                Box::new(crate::arrows::ArrowState::new(
                    crate::random_source::entity(),
                ))
            }),
        }
    }
    /// Entity box (`EntityType.sized` cube of `kind.size()`), as `bounds`.
    pub fn aabb(&self) -> Aabb {
        self.bounds()
    }
    /// The arrow's own random from now on (`Entity.random`).
    pub fn with_arrow_random(mut self, random: horizon_core::random::LegacyRandom) -> Self {
        if let Some(a) = self.arrow.as_mut() {
            a.random = random;
        }
        self
    }
    /// The shooter's bow enchantments; Flame spawns the arrow burning for 100
    /// seconds (`projectile_spawned` ignite), Piercing sets its pierce level.
    pub fn set_bonus(&mut self, bonus: crate::mobs::gear::ArrowBonus) {
        if let Some(a) = self.arrow.as_mut() {
            if bonus.flame {
                a.ignite_for_seconds(100.0);
            }
            a.pierce = bonus.pierce.min(127) as u8;
        }
        self.bonus = bonus;
    }
    /// `AbstractArrow.isCritArrow`.
    pub fn critical(&self) -> bool {
        matches!(self.kind, ProjectileKind::Arrow { critical: true, .. })
    }
    pub fn set_critical(&mut self, value: bool) {
        if let ProjectileKind::Arrow { critical, .. } = &mut self.kind {
            *critical = value;
        }
    }
    /// `AbstractArrow.tick`: critical streaks and submerged bubble trails.
    pub fn arrow_trail(
        &self,
        water: bool,
    ) -> Vec<(crate::particles::ParticleType, [f64; 3], [f64; 3])> {
        use crate::particles::ParticleType;
        let mut trail = Vec::new();
        if self.stuck || !self.is_arrow() {
            return trail;
        }
        let (p, v) = (self.position, self.velocity);
        if water {
            let drag = f64::from(0.6f32);
            let speed = [v.x * drag, v.y * drag, v.z * drag];
            let at = [
                p.x - speed[0] * 0.25,
                p.y - speed[1] * 0.25,
                p.z - speed[2] * 0.25,
            ];
            for _ in 0..4 {
                trail.push((ParticleType::Bubble, at, speed));
            }
        }
        if matches!(self.kind, ProjectileKind::Arrow { critical: true, .. }) {
            for i in 0..4 {
                let t = i as f64 / 4.0;
                trail.push((
                    ParticleType::Crit,
                    [p.x + v.x * t, p.y + v.y * t, p.z + v.z * t],
                    [-v.x, -v.y + 0.2, -v.z],
                ));
            }
        }
        trail
    }
    pub fn is_arrow(&self) -> bool {
        matches!(self.kind, ProjectileKind::Arrow { .. })
    }
    /// A `ThrowableProjectile`: snowballs, eggs, ender pearls, potions, a
    /// content mod's thrown item and a llama's spit. They are the projectiles
    /// that apply the effects of the blocks they fly through
    /// (`ThrowableProjectile.tick` calls `applyEffectsFromBlocks`), which an
    /// `AbstractHurtingProjectile` — a fireball, a wind charge, a skull — does
    /// not.
    pub fn is_thrown(&self) -> bool {
        matches!(
            self.kind,
            ProjectileKind::Snowball
                | ProjectileKind::Egg
                | ProjectileKind::EnderPearl
                | ProjectileKind::LlamaSpit
                | ProjectileKind::Potion(..)
                | ProjectileKind::Thrown(_)
        )
    }
    /// `Entity.isOnFire`. `WitherSkull.isOnFire` always returns false.
    pub fn on_fire(&self) -> bool {
        !matches!(self.kind, ProjectileKind::WitherSkull { .. })
            && (self.fire_ticks > 0 || self.synced_fire)
    }
    /// `Entity.igniteForTicks`: the longer fire wins.
    pub fn ignite_for_ticks(&mut self, ticks: i32) {
        if self.fire_ticks < ticks {
            self.fire_ticks = ticks;
        }
    }
    /// `Entity.clearFire`: it counts from below zero, which `on_fire` reads as out.
    pub fn clear_fire(&mut self) {
        self.fire_ticks = self.fire_ticks.min(0);
    }
    /// `Entity.baseTick` counts down active fire after block effects. The two
    /// item fireballs then renew their own flame in `AbstractHurtingProjectile.tick`.
    fn fire_tick(&mut self, w: &impl ProjectileWorld, client_side: bool) {
        if !(self.is_thrown()
            || self.kind == ProjectileKind::ExperienceBottle
            || self.is_fireball())
        {
            return;
        }
        let previous_fire_ticks = self.fire_ticks;
        if self.in_fire(w) {
            // BaseFireBlock.fireIgnite: a non-player counts up to zero before
            // it can catch fire again.
            if self.fire_ticks < 0 {
                self.fire_ticks += 1;
            }
            if self.fire_ticks >= 0 {
                self.ignite_for_ticks(160);
            }
        }
        if w.in_water(self.bounds()) {
            self.clear_fire();
        }
        let sync_fire = !client_side && self.kind != ProjectileKind::LlamaSpit;
        let burning =
            self.fire_ticks > 0 && !matches!(self.kind, ProjectileKind::WitherSkull { .. });
        if sync_fire && !burning && self.fire_ticks <= previous_fire_ticks {
            // Entity.applyEffectsFromBlocks resets fire immunity when the entity
            // is not burning and no block ignited it. A skull always reports
            // isOnFire=false, even when its saved remainingFireTicks is positive.
            self.fire_ticks = 0;
        }
        if self.fire_ticks > 0 {
            self.fire_ticks -= 1;
        }
        if sync_fire {
            // Entity.setSharedFlagOnFire runs before the fireball's own ignition.
            self.synced_fire = self.fire_ticks > 0 || self.has_visual_fire;
        }
        if self.kind.is_item_fireball() {
            self.ignite_for_ticks(20);
        }
    }
    /// `Entity.checkInsideBlocks`: the blocks under the box, deflated by 1e-7
    /// so a box sitting exactly on a block boundary is not inside it.
    fn in_fire(&self, w: &impl ProjectileWorld) -> bool {
        let b = self.bounds().deflate(Vec3::new(1.0e-7, 1.0e-7, 1.0e-7));
        let (lo, hi) = (b.min_position(), b.max_position());
        for x in (lo.x).floor() as i32..=(hi.x).floor() as i32 {
            for y in (lo.y).floor() as i32..=(hi.y).floor() as i32 {
                for z in (lo.z).floor() as i32..=(hi.z).floor() as i32 {
                    if w.ignites([x, y, z]) {
                        return true;
                    }
                }
            }
        }
        false
    }
    /// Whether it is a `SpectralArrow` (its own entity type): a spectral
    /// arrow's pickup item, or the glowing effect a dispenser shoots with.
    pub fn is_spectral_arrow(&self) -> bool {
        let ProjectileKind::Arrow { effect, .. } = &self.kind else {
            return false;
        };
        effect.is_some_and(|(id, _)| id == "minecraft:glowing")
            || self
                .arrow
                .as_ref()
                .is_some_and(|a| a.spectral || &*a.pickup_item.item == "minecraft:spectral_arrow")
    }
    /// Its entity type id (`minecraft:spectral_arrow` for a spectral arrow,
    /// else its kind's id).
    pub fn entity_type(&self) -> &'static str {
        if self.is_spectral_arrow() {
            "minecraft:spectral_arrow"
        } else {
            self.kind.id()
        }
    }
    /// `Arrow.ID_EFFECT_COLOR` (`updateColor`): the opaque colour of the
    /// potion the arrow carries, from its tipped pickup item or the effect a
    /// stray, bogged or parched adds; -1 for none.
    pub fn arrow_effect_color(&self) -> i32 {
        let ProjectileKind::Arrow { effect, .. } = &self.kind else {
            return -1;
        };
        if self.is_spectral_arrow() {
            return -1;
        }
        let rgb = match (effect, &self.arrow) {
            (Some((id, _)), _) => crate::brewing::effect(id).map(|e| e.shown_color()),
            (None, Some(a)) if &*a.pickup_item.item == "minecraft:tipped_arrow" => a
                .pickup_item
                .potion()
                .map(|p| crate::brewing::potion_color(Some(p))),
            _ => None,
        };
        rgb.map_or(-1, |rgb| (rgb | 0xFF00_0000) as i32)
    }
    pub fn is_fireball(&self) -> bool {
        matches!(
            self.kind,
            ProjectileKind::SmallFireball
                | ProjectileKind::LargeFireball { .. }
                | ProjectileKind::DragonFireball
                | ProjectileKind::WitherSkull { .. }
        )
    }
    /// `EntityRenderer.getBlockLightLevel` for a projectile: the renderer's
    /// own override wins (`ShulkerBulletRenderer` returns 15, as the fireball
    /// renderers do), and the base class draws anything `isOnFire` at 15
    /// whatever the level says. `None` keeps the level's block light; the sky
    /// light is never overridden.
    pub fn block_light_override(&self) -> Option<u8> {
        (self.is_fireball()
            || self.on_fire()
            || matches!(self.kind, ProjectileKind::ShulkerBullet { .. }))
        .then_some(15)
    }
    /// `EntityRenderer.getPackedLightCoords` lights an entity from the block
    /// its `getLightProbePosition` is in, which is `getEyePosition`: the eye
    /// height of its box over `getPosition(partialTick)`, so the frame's
    /// position rather than this tick's, and not the cell its feet rest in.
    pub fn light_probe_y(&self, partial: f64) -> f64 {
        self.previous.y + (self.position.y - self.previous.y) * partial + self.kind.eye_height()
    }
    pub fn water_bubbles(&self) -> Option<([f64; 3], [f64; 3])> {
        if !self.touching_water {
            return None;
        }
        // ThrowableProjectile.tick applies gravity, then applyInertia reads
        // that movement. AbstractHurtingProjectile.applyInertia reads the
        // movement it already has: fireballs, wither skulls and wind charges
        // have no gravity step before the bubbles.
        let hurting = matches!(
            self.kind,
            ProjectileKind::SmallFireball
                | ProjectileKind::LargeFireball { .. }
                | ProjectileKind::DragonFireball
                | ProjectileKind::WitherSkull { .. }
                | ProjectileKind::WindCharge
                | ProjectileKind::BreezeWindCharge
        );
        let throwable = matches!(
            self.kind,
            ProjectileKind::Snowball
                | ProjectileKind::Egg
                | ProjectileKind::EnderPearl
                | ProjectileKind::Potion(..)
                | ProjectileKind::ExperienceBottle
                | ProjectileKind::Thrown(_)
        );
        let movement = if hurting {
            [self.velocity.x, self.velocity.y, self.velocity.z]
        } else if throwable {
            [
                self.velocity.x,
                self.velocity.y - self.get_gravity(),
                self.velocity.z,
            ]
        } else {
            return None;
        };
        let at = [self.position.x, self.position.y, self.position.z];
        Some(water_bubble_at(at, movement))
    }
    /// Projectile.shoot: normalized direction plus triangular noise, times speed.
    pub fn shoot(
        &mut self,
        random: &mut impl RandomSource,
        dir: [f64; 3],
        speed: f32,
        uncertainty: f32,
    ) {
        let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2])
            .sqrt()
            .max(1.0e-9);
        let spread = 0.017_227_5 * f64::from(uncertainty);
        let v = [
            (dir[0] / len + random.triangle(0.0, spread)) * f64::from(speed),
            (dir[1] / len + random.triangle(0.0, spread)) * f64::from(speed),
            (dir[2] / len + random.triangle(0.0, spread)) * f64::from(speed),
        ];
        self.velocity = Vec3::new(v[0], v[1], v[2]);
        self.update_rotation();
    }
    /// Projectile.shootFromRotation for player throws.
    pub fn shoot_from_rotation(
        &mut self,
        random: &mut impl RandomSource,
        pitch: f32,
        yaw: f32,
        offset: f32,
        speed: f32,
        uncertainty: f32,
        source_velocity: Vec3,
        source_on_ground: bool,
    ) {
        let (p, y, o) = (
            pitch.to_radians(),
            yaw.to_radians(),
            (pitch + offset).to_radians(),
        );
        let dir = [
            f64::from(-y.sin() * p.cos()),
            f64::from(-o.sin()),
            f64::from(y.cos() * p.cos()),
        ];
        self.shoot(random, dir, speed, uncertainty);
        self.velocity = Vec3::new(
            self.velocity.x + source_velocity.x,
            self.velocity.y
                + if source_on_ground {
                    0.0
                } else {
                    source_velocity.y
                },
            self.velocity.z + source_velocity.z,
        );
    }
    /// AbstractHurtingProjectile: movement along the direction at the acceleration power.
    pub fn fireball(
        kind: ProjectileKind,
        position: Vec3,
        owner: Option<Who>,
        dir: [f64; 3],
    ) -> Self {
        let mut p = Self::new(kind, position, owner);
        let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2])
            .sqrt()
            .max(1.0e-9);
        p.velocity = Vec3::new(dir[0] / len * 0.1, dir[1] / len * 0.1, dir[2] / len * 0.1);
        p.update_rotation();
        p
    }
    /// `Fireball.setItem`: the item a fireball is drawn as. An empty stack
    /// puts the default fire charge back, anything else is kept as a single
    /// one (`copyWithCount(1)`); a kind that is no `Fireball` has no item.
    pub fn set_item(&mut self, stack: crate::items::ItemStack) {
        if !self.kind.is_item_fireball() {
            return;
        }
        let mut stack = if stack.count == 0 || &*stack.item == "minecraft:air" {
            crate::items::ItemStack::new("minecraft:fire_charge", 1)
        } else {
            stack
        };
        stack.count = 1;
        self.item = Some(stack);
    }
    fn update_rotation(&mut self) {
        let v = self.velocity;
        let sd = (v.x * v.x + v.z * v.z).sqrt();
        self.yaw = v.x.atan2(v.z).to_degrees() as f32;
        self.pitch = v.y.atan2(sd).to_degrees() as f32;
        // `Projectile.shoot` ends with `yRotO = getYRot(); xRotO = getXRot()`:
        // a freshly shot projectile interpolates from the rotation it was
        // aimed along, not from where it was created.
        self.previous_yaw = self.yaw;
        self.previous_pitch = self.pitch;
    }
    /// `Entity.setOldPosAndRot`: where the renderer interpolates from.
    pub fn set_old_pos_and_rot(&mut self) {
        self.previous = self.position;
        self.previous_yaw = self.yaw;
        self.previous_pitch = self.pitch;
    }
    /// `Entity.move(PISTON)`: the position only; `release_after_piston` follows
    /// (`AbstractArrow.move`).
    pub fn move_by_piston(&mut self, movement: Vec3) {
        self.position = self.position.add_vector(movement);
        self.needs_sync = true;
    }
    pub fn piston_release_bounds(&self) -> Option<Aabb> {
        let arrow = self.arrow.as_ref()?;
        if !arrow.in_ground || arrow.no_physics {
            return None;
        }
        Some(Aabb::new(self.position, self.position).inflate(Vec3::new(0.06, 0.06, 0.06)))
    }
    pub fn release_after_piston(&mut self, no_block_collision: bool) {
        let Some(arrow) = self.arrow.as_mut() else {
            return;
        };
        if arrow.in_ground && !arrow.no_physics && no_block_collision {
            self.velocity = start_falling_velocity(self.velocity, &mut arrow.random);
            arrow.in_ground = false;
            arrow.life = 0;
            self.stuck = false;
            self.needs_sync = true;
        }
    }
    /// `PistonMovingBlockEntity.moveCollidedEntities` on a slime block: the
    /// velocity along the axis. The piston's move after it (`AbstractArrow.move`)
    /// is what takes a stuck arrow out, with `startFalling`'s random damping.
    pub fn piston_slime_bounce(&mut self, movement: Direction) {
        self.velocity = horizon_world::piston_push::bounce_velocity(movement, self.velocity);
        self.needs_sync = true;
    }
    /// `ShulkerBullet.<init>(level, owner, target, invalidStartAxis)`: a
    /// shulker's bullet at `at`, the centre of its owner's box
    /// (`owner.getBoundingBox().getCenter()`), toward `target`, first leaving
    /// along an axis other than the one it is attached on.
    pub fn shulker_bullet(
        w: &impl ProjectileWorld,
        at: Vec3,
        owner: Who,
        target: Who,
        attach_axis: u8,
        seed: u64,
    ) -> Self {
        let mut p = Self::new(
            ProjectileKind::ShulkerBullet {
                target: Some(target),
                dir: Some(1),
                steps: 0,
                delta: [0.0; 3],
                seed,
            },
            at,
            Some(owner),
        );
        p.select_next_move(w, Some(attach_axis));
        p
    }
    fn bullet_random(&mut self, bound: i32) -> i32 {
        let ProjectileKind::ShulkerBullet { seed, .. } = &mut self.kind else {
            return 0;
        };
        // xorshift64*: the bullet's own random stream.
        *seed ^= *seed >> 12;
        *seed ^= *seed << 25;
        *seed ^= *seed >> 27;
        let v = seed.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33;
        (v % bound.max(1) as u64) as i32
    }
    fn block_pos(&self) -> [i32; 3] {
        [
            self.position.x.floor() as i32,
            self.position.y.floor() as i32,
            self.position.z.floor() as i32,
        ]
    }
    fn target_box(&self, w: &impl ProjectileWorld) -> Option<Aabb> {
        let ProjectileKind::ShulkerBullet {
            target: Some(target),
            ..
        } = self.kind
        else {
            return None;
        };
        w.targets()
            .iter()
            .find(|(who, _)| *who == target)
            .map(|(_, b)| *b)
    }
    /// `ShulkerBullet.selectNextMoveDirection`.
    fn select_next_move(&mut self, w: &impl ProjectileWorld, excluded: Option<u8>) {
        let (target, y_offset) = match self.target_box(w) {
            Some(b) => {
                let (lo, hi) = (b.min_position(), b.max_position());
                let half = (hi.y - lo.y) * 0.5;
                let c = [(lo.x + hi.x) / 2.0, lo.y + half, (lo.z + hi.z) / 2.0];
                (c.map(|v| v.floor() as i32), half)
            }
            None => {
                let p = self.block_pos();
                ([p[0], p[1] - 1, p[2]], 0.5)
            }
        };
        let mut goal = [
            f64::from(target[0]) + 0.5,
            f64::from(target[1]) + y_offset,
            f64::from(target[2]) + 0.5,
        ];
        let mut selection = None;
        let centre = [
            f64::from(target[0]) + 0.5,
            f64::from(target[1]) + 0.5,
            f64::from(target[2]) + 0.5,
        ];
        let d2 = (0..3)
            .map(|i| ([self.position.x, self.position.y, self.position.z][i] - centre[i]).powi(2))
            .sum::<f64>();
        if d2 >= 4.0 {
            let cur = self.block_pos();
            let mut options = Vec::new();
            let step = |d: u8| {
                let s = STEPS[d as usize];
                [cur[0] + s[0], cur[1] + s[1], cur[2] + s[2]]
            };
            for (axis, (lo, hi)) in [(0u8, (4u8, 5u8)), (1, (0, 1)), (2, (2, 3))] {
                if excluded == Some(axis) {
                    continue;
                }
                let i = axis as usize;
                if cur[i] < target[i] && w.empty(step(hi)) {
                    options.push(hi);
                } else if cur[i] > target[i] && w.empty(step(lo)) {
                    options.push(lo);
                }
            }
            let mut pick = self.bullet_random(6) as u8;
            if options.is_empty() {
                let mut i = 5;
                while !w.empty(step(pick)) && i > 0 {
                    pick = self.bullet_random(6) as u8;
                    i -= 1;
                }
            } else {
                pick = options[self.bullet_random(options.len() as i32) as usize];
            }
            let s = STEPS[pick as usize];
            goal = [
                self.position.x + f64::from(s[0]),
                self.position.y + f64::from(s[1]),
                self.position.z + f64::from(s[2]),
            ];
            selection = Some(pick);
        }
        let d = [
            goal[0] - self.position.x,
            goal[1] - self.position.y,
            goal[2] - self.position.z,
        ];
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let delta = if len == 0.0 {
            [0.0; 3]
        } else {
            d.map(|v| v / len * 0.15)
        };
        let steps_next = 10 + self.bullet_random(5) * 10;
        if let ProjectileKind::ShulkerBullet {
            dir,
            steps,
            delta: dl,
            ..
        } = &mut self.kind
        {
            *dir = selection;
            *dl = delta;
            *steps = steps_next;
        }
    }
    /// `ShulkerBullet.tick`.
    fn tick_bullet(&mut self, w: &impl ProjectileWorld, impact: bool) -> (bool, Hit) {
        let target = self.target_box(w);
        let ProjectileKind::ShulkerBullet { delta, .. } = &mut self.kind else {
            return (false, Hit::None);
        };
        if target.is_some() {
            for d in delta.iter_mut() {
                *d = (*d * 1.025).clamp(-1.0, 1.0);
            }
            let dl = *delta;
            let v = self.velocity;
            self.velocity = Vec3::new(
                v.x + (dl[0] - v.x) * 0.2,
                v.y + (dl[1] - v.y) * 0.2,
                v.z + (dl[2] - v.z) * 0.2,
            );
        } else {
            // `Entity.applyGravity`: `getGravity` is 0 under NoGravity.
            self.velocity.y -= crate::entity_rules::EntityRules::get_gravity(self);
        }
        let from = self.position;
        let to = from.add_vector(self.velocity);
        let block = w.clip(from, to);
        let end = block.map_or(to, |b| b.0);
        let margin = self.hit_margin();
        let mut best: Option<(Who, Vec3, f64)> = None;
        for (who, b) in w.targets() {
            if Some(*who) == self.owner && !self.left_owner {
                continue;
            }
            if let Some(at) = b.inflate(Vec3::new(margin, margin, margin)).clip(from, end) {
                let d = (at.x - from.x).powi(2) + (at.y - from.y).powi(2) + (at.z - from.z).powi(2);
                if best.is_none_or(|bb| d < bb.2) {
                    best = Some((*who, at, d));
                }
            }
        }
        if !self.left_owner {
            let owner_box = w
                .targets()
                .iter()
                .find(|(who, _)| Some(*who) == self.owner)
                .map(|(_, b)| *b);
            self.left_owner =
                owner_box.is_none_or(|b| !b.inflate(Vec3::new(1.0, 1.0, 1.0)).contains(from));
        }
        if impact && let Some((who, at, _)) = best {
            self.position = at;
            return (false, Hit::Entity { who, at });
        }
        if impact && let Some((at, pos, face)) = block {
            self.position = at;
            return (false, Hit::Block { pos, at, face });
        }
        self.position = to;
        self.update_rotation();
        if let Some(b) = target {
            let (dir, steps) = match self.kind {
                ProjectileKind::ShulkerBullet { dir, steps, .. } => (dir, steps),
                _ => (None, 0),
            };
            if steps > 0 {
                let left = steps - 1;
                if let ProjectileKind::ShulkerBullet { steps, .. } = &mut self.kind {
                    *steps = left;
                }
                if left == 0 {
                    self.select_next_move(w, dir.map(axis_of));
                }
            }
            let dir = match self.kind {
                ProjectileKind::ShulkerBullet { dir, .. } => dir,
                _ => None,
            };
            if let Some(d) = dir {
                let p = self.block_pos();
                let s = STEPS[d as usize];
                let axis = axis_of(d);
                let (lo, hi) = (b.min_position(), b.max_position());
                let t = [
                    ((lo.x + hi.x) / 2.0).floor() as i32,
                    lo.y.floor() as i32,
                    ((lo.z + hi.z) / 2.0).floor() as i32,
                ];
                if w.can_stand_on([p[0] + s[0], p[1] + s[1], p[2] + s[2]], self.position.y)
                    || p[axis as usize] == t[axis as usize]
                {
                    self.select_next_move(w, Some(axis));
                }
            }
        }
        (self.position.y > -128.0, Hit::None)
    }
    /// AbstractHurtingProjectile.applyInertia, on both server and client.
    pub fn apply_hurting_inertia(&mut self, water: bool) {
        // WitherSkull.getInertia: 0.73 for dangerous skulls.
        // Vanilla returns float inertia and widens it before scaling double motion.
        let inertia = f64::from(match self.kind {
            ProjectileKind::WitherSkull { dangerous: true } => 0.73_f32,
            // AbstractWindCharge.getInertia and getLiquidInertia.
            ProjectileKind::WindCharge | ProjectileKind::BreezeWindCharge => 1.0_f32,
            _ if water => 0.8_f32,
            _ => 0.95_f32,
        });
        let v = self.velocity;
        let len = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt().max(1.0e-9);
        self.velocity = Vec3::new(
            (v.x + v.x / len * self.acceleration) * inertia,
            (v.y + v.y / len * self.acceleration) * inertia,
            (v.z + v.z / len * self.acceleration) * inertia,
        );
    }

    /// One tick: returns whether to keep the projectile and what it hit.
    pub fn tick(&mut self, w: &impl ProjectileWorld) -> (bool, Hit) {
        self.tick_with(w, true, false)
    }
    /// One tick of a server's projectile on its client: [`tick`](Self::tick),
    /// whose bubble columns push it on the client too, so a pearl held in a
    /// stasis chamber hovers between the server's updates instead of sinking
    /// and snapping back.
    pub fn tick_remote(&mut self, w: &impl ProjectileWorld) -> (bool, Hit) {
        self.tick_with(w, true, true)
    }
    /// The projectiles whose tick applies the effects of the blocks they moved
    /// through (`applyEffectsFromBlocks`), bubble columns among them:
    /// `ThrowableProjectile`, `AbstractHurtingProjectile` and `ShulkerBullet`.
    /// Arrows push themselves (`arrows::tick`); llama spit applies none.
    fn applies_block_effects(&self) -> bool {
        self.is_throwable_projectile()
            || matches!(
                self.kind,
                ProjectileKind::SmallFireball
                    | ProjectileKind::LargeFireball { .. }
                    | ProjectileKind::WitherSkull { .. }
                    | ProjectileKind::WindCharge
                    | ProjectileKind::BreezeWindCharge
                    | ProjectileKind::ShulkerBullet { .. }
            )
    }
    /// A `ThrowableProjectile`: the thrown kinds but llama spit, a plain
    /// `Projectile` with its own tick.
    fn is_throwable_projectile(&self) -> bool {
        self.is_thrown() && self.kind != ProjectileKind::LlamaSpit
    }
    /// `ThrowableProjectile.handleFirstTickBubbleColumn`: a thrown item that
    /// spawns inside a bubble column is pushed by every column block in its box
    /// on its first tick (`findBlocksIn`, the block positions of both corners),
    /// before it has moved through anything.
    fn first_tick_bubble_column(&mut self, w: &impl ProjectileWorld) {
        let b = self.bounds();
        let (lo, hi) = (b.min_position(), b.max_position());
        for x in lo.x.floor() as i32..=hi.x.floor() as i32 {
            for y in lo.y.floor() as i32..=hi.y.floor() as i32 {
                for z in lo.z.floor() as i32..=hi.z.floor() as i32 {
                    if let Some((drag, surface)) = w.bubble_column([x, y, z]) {
                        self.bubble_column_push(drag, surface, [x, y, z]);
                    }
                }
            }
        }
    }
    /// `Projectile.onAboveBubbleColumn` / `onInsideBubbleColumn`: unlike
    /// `Entity`'s, a projectile's pushes are not clamped.
    fn bubble_column_push(&mut self, drag: bool, surface: bool, at: [i32; 3]) {
        if surface {
            self.bubble_surfaces.push(at);
        }
        self.velocity.y += match (drag, surface) {
            (true, _) => -0.03,
            (false, true) => 0.1,
            (false, false) => 0.06,
        };
    }
    /// `applyEffectsFromBlocks`' bubble columns (`Entity.checkInsideBlocks`):
    /// the blocks the box swept through from `from`, each once, pushing when
    /// the move was long or the final box (deflated by 1e-5F) is inside it.
    fn bubble_columns_along(&mut self, w: &impl ProjectileWorld, from: Vec3) {
        let to = self.position;
        let eps = f64::from(1.0e-5_f32);
        let deflated = self.bounds().deflate(Vec3::new(eps, eps, eps));
        let far = from.distance_squared(to) > 0.999_990_000_000_252_6f64 * 0.999_990_000_000_252_6;
        let mut cells = Vec::new();
        let mut used = 0;
        let _ = horizon_core::geometry::sweep_intersected_blocks(
            from,
            to,
            deflated,
            1_000_000,
            |p, step| {
                if step >= 16 {
                    return false;
                }
                used = step;
                cells.push(p);
                true
            },
        );
        // A sweep that used all 16 steps checks the end position once more.
        if used + 1 >= 16 {
            let _ = horizon_core::geometry::sweep_intersected_blocks(
                to,
                to,
                deflated,
                1_000_000,
                |p, step| {
                    if step >= 1 {
                        return false;
                    }
                    cells.push(p);
                    true
                },
            );
        }
        // Each block once, in the sweep's order: a handful of cells, no set.
        for (i, &p) in cells.iter().enumerate() {
            if cells[..i].contains(&p) {
                continue;
            }
            let Some((drag, surface)) = w.bubble_column([p.x, p.y, p.z]) else {
                continue;
            };
            let cell = Aabb::new(
                Vec3::new(f64::from(p.x), f64::from(p.y), f64::from(p.z)),
                Vec3::new(f64::from(p.x + 1), f64::from(p.y + 1), f64::from(p.z + 1)),
            );
            if far || deflated.intersects(cell) {
                self.bubble_column_push(drag, surface, [p.x, p.y, p.z]);
            }
        }
    }
    /// One tick that meets nothing: what `tick` would have done had its hit been
    /// canceled (`ProjectileImpactEvent`). The projectile goes the whole way of its
    /// motion and flies on, with no hit to act on.
    pub fn tick_through(&mut self, w: &impl ProjectileWorld) -> (bool, Hit) {
        self.tick_with(w, false, false)
    }
    fn tick_with(
        &mut self,
        w: &impl ProjectileWorld,
        impact: bool,
        client_side: bool,
    ) -> (bool, Hit) {
        // LlamaSpit and ShulkerBullet call Projectile.tick before moving;
        // ThrowableProjectile and AbstractHurtingProjectile call it afterwards.
        let base_first = matches!(
            self.kind,
            ProjectileKind::LlamaSpit | ProjectileKind::ShulkerBullet { .. }
        );
        if base_first {
            self.tick_shoot_event();
        }
        self.bubble_surfaces.clear();
        // `ThrowableProjectile.tick` begins with `handleFirstTickBubbleColumn`,
        // before its gravity and inertia, so the push takes the tick's drag
        // with it. A server simulates it too, not only a client.
        if self.age == 0 && self.is_throwable_projectile() {
            self.first_tick_bubble_column(w);
        }
        let from = self.position;
        let result = self.tick_inner(w, impact);
        if self.applies_block_effects() {
            self.bubble_columns_along(w, from);
        }
        w.block_contacts(
            self.id as i32,
            self.kind.id(),
            from,
            self.position,
            self.bounds(),
        );
        self.fire_tick(w, client_side);
        self.stuck_tick(w, from);
        if !base_first {
            self.tick_shoot_event();
        }
        result
    }
    /// `applyEffectsFromBlocks` after the tick's move, for its cobwebs and
    /// powder snow (`makeStuckInBlock`): the last such block its box (deflated
    /// by 1e-5, `checkInsideBlocks`) met between `from` and where it is now
    /// sets `stuck_speed`, which the next block's push (`Entity.move`) spends.
    fn stuck_tick(&mut self, w: &impl ProjectileWorld, from: Vec3) {
        let end = self.bounds();
        let start = end.move_by(from.subtract(self.position));
        let lo = Vec3::new(
            start.min_position().x.min(end.min_position().x) + 1.0e-5,
            start.min_position().y.min(end.min_position().y) + 1.0e-5,
            start.min_position().z.min(end.min_position().z) + 1.0e-5,
        );
        let hi = Vec3::new(
            start.max_position().x.max(end.max_position().x) - 1.0e-5,
            start.max_position().y.max(end.max_position().y) - 1.0e-5,
            start.max_position().z.max(end.max_position().z) - 1.0e-5,
        );
        for x in lo.x.floor() as i32..=hi.x.floor() as i32 {
            for y in lo.y.floor() as i32..=hi.y.floor() as i32 {
                for z in lo.z.floor() as i32..=hi.z.floor() as i32 {
                    if let Some(stuck) = w.stuck_speed([x, y, z]) {
                        self.stuck_speed = stuck;
                    }
                }
            }
        }
    }
    /// The first-tick notification branch of vanilla's base projectile tick.
    /// Arrows own this state in ArrowState and follow their separate tick path.
    fn tick_shoot_event(&mut self) {
        if self.arrow.is_none() && !self.has_been_shot {
            self.has_been_shot = true;
            self.shoot_event = Some(self.position);
        }
    }
    /// Drain the first-tick event for dispatch in the owner's game context.
    pub fn take_shoot_event(&mut self) -> Option<Vec3> {
        self.shoot_event.take()
    }
    /// `ProjectileUtil.computeMargin`: how far the boxes of the entities on
    /// its way grow, from nothing for its first two ticks up to 0.3 at its
    /// eighth (`tickCount`, which `age` counts, is raised before the tick).
    fn hit_margin(&self) -> f64 {
        f64::from(((self.age as i32 - 2) as f32 / 20.0).clamp(0.0, 0.3))
    }
    fn tick_inner(&mut self, w: &impl ProjectileWorld, impact: bool) -> (bool, Hit) {
        // `Entity.setOldPosAndRot` (the previous position and rotation the
        // renderer interpolates from), kept across the `impact` flag's arrival
        // from #6279.
        self.set_old_pos_and_rot();
        self.age += 1;
        if matches!(self.kind, ProjectileKind::ShulkerBullet { .. }) {
            return self.tick_bullet(w, impact);
        }
        // ThrowableProjectile and AbstractHurtingProjectile.applyInertia read
        // `isInWater` before their move and `Projectile.tick`'s base tick;
        // LlamaSpit.tick runs that base tick first, at the spit's box here.
        let water = if self.kind == ProjectileKind::LlamaSpit {
            w.in_water(self.bounds())
        } else {
            self.touching_water
        };
        match self.kind {
            ProjectileKind::SmallFireball
            | ProjectileKind::LargeFireball { .. }
            | ProjectileKind::DragonFireball
            | ProjectileKind::WitherSkull { .. }
            | ProjectileKind::WindCharge
            | ProjectileKind::BreezeWindCharge => {
                self.apply_hurting_inertia(water);
            }
            // Arrows and tridents tick with `arrows::tick`.
            ProjectileKind::Arrow { .. } | ProjectileKind::Trident => {}
            // LlamaSpit.tick: gone in water; drag and gravity follow the move.
            ProjectileKind::LlamaSpit => {
                if water {
                    return (false, Hit::None);
                }
            }
            _ => {
                // ThrowableProjectile: gravity, then inertia. Its drag is a
                // float in vanilla (0.8F in water, `getAirDrag` 0.99F), so
                // widens to the double that scales the motion.
                self.velocity.y -= self.get_gravity();
                let inertia = f64::from(if water { 0.8_f32 } else { 0.99_f32 });
                self.velocity = Vec3::new(
                    self.velocity.x * inertia,
                    self.velocity.y * inertia,
                    self.velocity.z * inertia,
                );
            }
        }
        let from = self.position;
        let to = Vec3::new(
            from.x + self.velocity.x,
            from.y + self.velocity.y,
            from.z + self.velocity.z,
        );
        let block = w.clip(from, to);
        let end = block.map_or(to, |b| b.0);
        // ProjectileUtil.getEntityHitResult: boxes inflated by `computeMargin` along the path.
        let margin = self.hit_margin();
        let mut best: Option<(Who, Vec3, f64)> = None;
        for (who, b) in w.targets() {
            if Some(*who) == self.owner && !self.left_owner {
                continue;
            }
            let inflated = b.inflate(Vec3::new(margin, margin, margin));
            if let Some(at) = inflated.clip(from, end) {
                let d = (at.x - from.x).powi(2) + (at.y - from.y).powi(2) + (at.z - from.z).powi(2);
                if best.is_none_or(|bb| d < bb.2) {
                    best = Some((*who, at, d));
                }
            }
        }
        if !self.left_owner {
            // Leave the owner once no target box (the owner's) contains us.
            let owner_box = w
                .targets()
                .iter()
                .find(|(who, _)| Some(*who) == self.owner)
                .map(|(_, b)| *b);
            self.left_owner = owner_box
                .is_none_or(|b| !b.inflate(Vec3::new(1.0, 1.0, 1.0)).contains(self.position));
        }
        // DragonFireball.onHit does nothing for its own owner (`ownedBy`): the
        // selected hit is not replaced by what lies behind it, nothing happens,
        // and the fireball keeps its motion. AbstractHurtingProjectile.tick
        // still moves it to the hit's location, as for any hit that is not a
        // MISS, so it stops there this tick.
        if impact
            && self.kind == ProjectileKind::DragonFireball
            && let Some((who, at, _)) = best
            && self.owner == Some(who)
        {
            self.position = at;
            self.touching_water = w.in_water(self.bounds());
            let alive = self.position.y > -128.0 && self.age < 1200;
            return (alive, Hit::None);
        }
        // LlamaSpit never discards on an entity: `onHitEntity` only hurts, so
        // the spit flies on through what it hit (its block hit discards).
        let spit = self.kind == ProjectileKind::LlamaSpit;
        let spit_hit = best
            .filter(|_| impact && spit)
            .map_or(Hit::None, |(who, at, _)| Hit::Entity { who, at });
        if impact
            && !spit
            && let Some((who, at, _)) = best
        {
            self.position = at;
            return (false, Hit::Entity { who, at });
        }
        if impact
            && !(spit && best.is_some())
            && let Some((at, pos, face)) = block
        {
            self.position = at;
            return (false, Hit::Block { pos, at, face });
        }
        if self.kind == ProjectileKind::LlamaSpit {
            // The 0.25 box touching anything but air discards the spit.
            let h = 0.125;
            let touches = [-h, h].iter().any(|dx| {
                [-h, h].iter().any(|dy| {
                    [-h, h].iter().any(|dz| {
                        !w.empty([
                            (from.x + dx).floor() as i32,
                            (from.y + dy + h).floor() as i32,
                            (from.z + dz).floor() as i32,
                        ])
                    })
                })
            });
            if touches {
                return (false, Hit::None);
            }
            self.position = to;
            // LlamaSpit.getAirDrag is 0.99F, widened before it scales motion.
            let drag = f64::from(0.99_f32);
            self.velocity = Vec3::new(
                self.velocity.x * drag,
                self.velocity.y * drag - self.get_gravity(),
                self.velocity.z * drag,
            );
            self.update_rotation();
            return (self.position.y > -128.0 && self.age < 6000, spit_hit);
        }
        self.position = to;
        if !self.is_fireball() {
            self.update_rotation();
        }
        // Entity.baseTick's updateFluidInteraction, after the move.
        self.touching_water = w.in_water(self.bounds());
        let alive =
            self.position.y > -128.0 && self.age < if self.is_fireball() { 1200 } else { 6000 };
        (alive, Hit::None)
    }
    /// AbstractArrow.onHitEntity damage: ceil(speed * base) plus crit bonus.
    pub fn arrow_damage(&self, random: &mut impl RandomSource) -> f32 {
        let ProjectileKind::Arrow {
            base_damage,
            critical,
            ..
        } = self.kind
        else {
            return 0.0;
        };
        let v = self.velocity;
        let speed = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
        let mut damage = (speed * base_damage).clamp(0.0, f64::from(i32::MAX)).ceil() as i32;
        if critical {
            let bonus = random.next_int_bound(damage / 2 + 2).unwrap_or(0);
            damage = damage.saturating_add(bonus);
        }
        damage as f32
    }
    /// the item a thrown potion or a mod's type is drawn as with its model
    /// (`ThrownItemRenderer`): the potion stack itself, or the type's item.
    pub fn drawn_item(&self) -> Option<crate::items::ItemStack> {
        match &self.kind {
            ProjectileKind::Potion(stack, _) => Some((**stack).clone()),
            ProjectileKind::ExperienceBottle => Some(crate::items::ItemStack::new(
                "minecraft:experience_bottle",
                1,
            )),
            ProjectileKind::Thrown(t) => Some(crate::items::ItemStack::new(t.item, 1)),
            _ => None,
        }
    }
    /// The item sprite drawn for thrown projectiles (ThrownItemRenderer).
    pub fn item_sprite(&self) -> Option<&'static str> {
        Some(match &self.kind {
            ProjectileKind::Snowball => "item/snowball",
            ProjectileKind::Egg => "item/egg",
            ProjectileKind::EnderPearl => "item/ender_pearl",
            ProjectileKind::SmallFireball | ProjectileKind::LargeFireball { .. } => {
                "item/fire_charge"
            }
            ProjectileKind::Potion(stack, _) => potion_sprite(&stack.item),
            ProjectileKind::ExperienceBottle => "item/experience_bottle",
            ProjectileKind::LlamaSpit => "entity/llama/llama_spit",
            // Drawn as `WindChargeModel` and `ShulkerBulletModel` by the game,
            // a dragon fireball as `DragonFireballRenderer`'s billboard.
            ProjectileKind::DragonFireball
            | ProjectileKind::WindCharge
            | ProjectileKind::BreezeWindCharge
            | ProjectileKind::ShulkerBullet { .. } => return None,
            ProjectileKind::WitherSkull { dangerous: false } => "entity/wither/wither",
            ProjectileKind::WitherSkull { dangerous: true } => "entity/wither/wither_invulnerable",
            // Drawn as their models by the game.
            ProjectileKind::Arrow { .. } | ProjectileKind::Trident | ProjectileKind::Thrown(_) => {
                return None;
            }
        })
    }
    /// `ThrownItemRenderer`'s scale for the kind: 3 for a large fireball,
    /// 0.75 for a small one, 1 for the rest.
    pub fn sprite_scale(&self) -> f32 {
        match self.kind {
            ProjectileKind::LargeFireball { .. } => 3.0,
            ProjectileKind::SmallFireball => 0.75,
            _ => 1.0,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use horizon_core::random::LegacyRandom;

    struct Empty(Vec<(Who, Aabb)>);
    impl ProjectileWorld for Empty {
        fn clip(&self, _: Vec3, _: Vec3) -> Option<(Vec3, [i32; 3], [i32; 3])> {
            None
        }
        fn in_water(&self, _: Aabb) -> bool {
            false
        }
        fn targets(&self) -> &[(Who, Aabb)] {
            &self.0
        }
    }
    /// Water filled with one kind of bubble column, as in a stasis chamber.
    struct Column {
        drag: bool,
        surface: bool,
    }
    impl ProjectileWorld for Column {
        fn clip(&self, _: Vec3, _: Vec3) -> Option<(Vec3, [i32; 3], [i32; 3])> {
            None
        }
        fn in_water(&self, _: Aabb) -> bool {
            true
        }
        fn targets(&self) -> &[(Who, Aabb)] {
            &[]
        }
        fn bubble_column(&self, _: [i32; 3]) -> Option<(bool, bool)> {
            Some((self.drag, self.surface))
        }
    }
    /// A pearl resting in the water of a column, past its first tick.
    fn resting_pearl() -> Projectile {
        let mut p = Projectile::new(ProjectileKind::EnderPearl, Vec3::new(0.5, 100.2, 0.5), None);
        p.age = 5;
        p.touching_water = true;
        p
    }
    #[test]
    fn a_pearl_in_a_bubble_column_is_pushed_after_it_moves() {
        // ThrowableProjectile.tick: gravity 0.03, the water drag 0.8F, the
        // move, then applyEffectsFromBlocks' BubbleColumnBlock.entityInside:
        // Projectile.onInsideBubbleColumn adds 0.06 (-0.03 dragging down) and
        // onAboveBubbleColumn 0.1 at the surface, with no clamp.
        let drag = f64::from(0.8_f32);
        for (column, push) in [
            (
                Column {
                    drag: false,
                    surface: false,
                },
                0.06,
            ),
            (
                Column {
                    drag: false,
                    surface: true,
                },
                0.1,
            ),
            (
                Column {
                    drag: true,
                    surface: false,
                },
                -0.03,
            ),
            (
                Column {
                    drag: true,
                    surface: true,
                },
                -0.03,
            ),
        ] {
            let mut p = resting_pearl();
            p.tick_remote(&column);
            assert_eq!(p.position.y, 100.2 + -0.03 * drag, "the move comes first");
            assert_eq!(
                p.velocity.y,
                -0.03 * drag + push,
                "drag {} surface {}",
                column.drag,
                column.surface
            );
        }
    }
    #[test]
    fn a_new_pearl_takes_its_first_bubble_column_push_before_gravity() {
        // ThrowableProjectile.handleFirstTickBubbleColumn on the first tick,
        // before gravity, then the air drag 0.99F (not yet touching water),
        // then the push after the move.
        let mut p = Projectile::new(ProjectileKind::EnderPearl, Vec3::new(0.5, 100.2, 0.5), None);
        p.tick_remote(&Column {
            drag: false,
            surface: false,
        });
        let air = f64::from(0.99_f32);
        assert_eq!(p.velocity.y, (0.06 - 0.03) * air + 0.06);
    }
    /// `ThrowableProjectile.handleFirstTickBubbleColumn` on a server: the tick
    /// a snowball is shot in pushes it by the column it spawned in, before its
    /// gravity and the air drag, and only on that first tick. The client path
    /// (`tick_remote`) had it and the host's did not, so a throwable in a bubble
    /// column rose for a guest and sank for the host. Every tick then ends with
    /// `applyEffectsFromBlocks`, whose columns push it again after the move.
    #[test]
    fn a_throwable_spawned_in_a_bubble_column_is_pushed_on_its_first_tick() {
        let air = f64::from(0.99_f32);
        for (column, push) in [
            (
                Column {
                    drag: false,
                    surface: false,
                },
                0.06,
            ),
            (
                Column {
                    drag: false,
                    surface: true,
                },
                0.1,
            ),
            (
                Column {
                    drag: true,
                    surface: false,
                },
                -0.03,
            ),
        ] {
            let mut p = Projectile::new(ProjectileKind::Snowball, Vec3::new(0.5, 100.5, 0.5), None);
            p.tick(&column);
            assert_eq!(
                p.velocity.y,
                (push - 0.03) * air + push,
                "drag {} surface {}",
                column.drag,
                column.surface
            );
            p.velocity = Vec3::ZERO;
            p.tick(&column);
            // The box is in the column's water after the first move, so the
            // drag is `getWaterDrag`'s 0.8F; the first tick's own scan is not
            // run again, only the push of the blocks the move went through.
            assert_eq!(
                p.velocity.y,
                -0.03 * f64::from(0.8_f32) + push,
                "drag {} surface {}",
                column.drag,
                column.surface
            );
        }
        // A kind that is no `ThrowableProjectile` has no first-tick scan: a
        // hurting projectile is pushed only by the blocks its move went through
        // (`AbstractHurtingProjectile.tick`'s `applyEffectsFromBlocks`): its
        // 1-block box spans two column blocks.
        let mut p = Projectile::new(
            ProjectileKind::LargeFireball { power: 1 },
            Vec3::new(0.5, 100.5, 0.5),
            None,
        );
        p.tick(&Column {
            drag: false,
            surface: false,
        });
        assert_eq!(p.velocity.y, 0.12);
    }
    #[test]
    fn a_pearl_outside_any_bubble_column_only_falls() {
        let mut p = resting_pearl();
        p.tick_remote(&Empty(vec![]));
        assert_eq!(p.velocity.y, -0.03 * f64::from(0.8_f32));
    }
    #[test]
    fn piston_move_releases_a_stuck_arrow_with_start_falling_velocity() {
        let mut p = Projectile::new(
            ProjectileKind::Arrow {
                base_damage: 2.0,
                critical: false,
                effect: None,
            },
            Vec3::new(0.5, 100.0, 0.5),
            None,
        );
        p.velocity = Vec3::new(1.0, -2.0, 3.0);
        p.stuck = true;
        p.arrow.as_mut().unwrap().in_ground = true;
        let mut random = p.arrow.as_ref().unwrap().random.clone();
        let expected = start_falling_velocity(p.velocity, &mut random);

        p.move_by_piston(Vec3::new(0.5, 0.0, 0.0));
        assert!(p.piston_release_bounds().is_some());
        p.release_after_piston(false);
        assert!(p.arrow.as_ref().unwrap().in_ground);
        assert_eq!(p.velocity, Vec3::new(1.0, -2.0, 3.0));

        p.release_after_piston(true);
        let arrow = p.arrow.as_ref().unwrap();
        assert!(!arrow.in_ground);
        assert_eq!(p.velocity, expected);
        assert!(!p.stuck);
        assert!(p.needs_sync);
        assert!(p.piston_release_bounds().is_none());
    }

    #[test]
    fn first_tick_shoot_event_uses_base_tick_position_and_saved_flag() {
        let w = Empty(vec![]);
        for kind in [ProjectileKind::Snowball, ProjectileKind::LlamaSpit] {
            let start = Vec3::new(0.0, 100.0, 0.0);
            let mut p = Projectile::new(kind.clone(), start, None);
            p.velocity = Vec3::new(1.0, 0.0, 0.0);
            p.tick(&w);
            let expected = if kind == ProjectileKind::LlamaSpit {
                start
            } else {
                p.position
            };
            assert!(p.has_been_shot);
            assert_eq!(p.take_shoot_event(), Some(expected));
            assert_eq!(p.take_shoot_event(), None);
            p.tick(&w);
            assert_eq!(p.take_shoot_event(), None);
            let mut saved = Projectile::new(kind, start, None);
            saved.has_been_shot = true;
            saved.tick(&w);
            assert_eq!(saved.take_shoot_event(), None);
        }
    }

    #[test]
    fn snowball_falls_with_throwable_gravity_and_drag() {
        let mut p = Projectile::new(ProjectileKind::Snowball, Vec3::new(0.0, 100.0, 0.0), None);
        p.velocity = Vec3::new(1.0, 0.0, 0.0);
        let w = Empty(vec![]);
        p.tick(&w);
        // ThrowableProjectile: gravity before inertia, then move.
        // `getAirDrag` is 0.99F, widened to double: 0.9900000095367432.
        assert_eq!(p.velocity.y.to_bits(), 0xbf9e_69ad_47ae_147b);
        assert_eq!(p.position.x.to_bits(), 0x3fef_ae14_8000_0000);
    }
    /// One fire block in the cell (0, 99, 0), open air around it: a throwable
    /// inside that cell is set alight, and one that has left it burns down.
    struct Fire {
        targets: Vec<(Who, Aabb)>,
        water: bool,
    }
    impl ProjectileWorld for Fire {
        fn clip(&self, _: Vec3, _: Vec3) -> Option<(Vec3, [i32; 3], [i32; 3])> {
            None
        }
        fn in_water(&self, _: Aabb) -> bool {
            self.water
        }
        fn targets(&self) -> &[(Who, Aabb)] {
            &self.targets
        }
        fn ignites(&self, p: [i32; 3]) -> bool {
            p == [0, 99, 0]
        }
    }
    /// `ThrowableProjectile.tick` calls `applyEffectsFromBlocks`, so a snowball
    /// thrown through a fire block burns for `BaseFireBlock.fireIgnite`'s 160
    /// ticks and is drawn with the flame (`Entity.displayFireAnimation`). The
    /// port had no fire at all on a thrown projectile: `fire_ticks` stayed 0
    /// however long it sat in the flames (#7456).
    #[test]
    fn a_snowball_in_a_fire_block_burns_and_the_fire_counts_down() {
        let mut p = Projectile::new(ProjectileKind::Snowball, Vec3::new(0.5, 99.5, 0.5), None);
        // Falling slowly: it stays inside (0, 99, 0) for this tick.
        p.velocity = Vec3::ZERO;
        let w = Fire {
            targets: vec![],
            water: false,
        };
        assert!(!p.on_fire());
        p.tick(&w);
        // Ignited for 160, then `Entity.baseTick`'s countdown for this tick.
        assert_eq!(p.fire_ticks, 159);
        assert!(p.on_fire());
        // Out of the flames, the rest of the 160 ticks burn down.
        let air = Empty(vec![]);
        p.position = Vec3::new(0.5, 120.0, 0.5);
        p.tick(&air);
        assert_eq!(p.fire_ticks, 158);
        for _ in 0..158 {
            p.tick(&air);
        }
        assert_eq!(p.fire_ticks, 0, "the fire runs out");
        assert!(!p.on_fire());
        // Water puts it out at once: `Entity.baseTick`'s `isInWaterRainOrBubble`.
        let mut wet = Projectile::new(ProjectileKind::Snowball, Vec3::new(0.5, 99.5, 0.5), None);
        wet.velocity = Vec3::ZERO;
        wet.tick(&w);
        assert_eq!(wet.fire_ticks, 159);
        wet.tick(&Fire {
            targets: vec![],
            water: true,
        });
        assert_eq!(wet.fire_ticks, 0, "water puts the snowball out");
    }
    /// `AbstractHurtingProjectile.tick` renews the fire after the base
    /// countdown, without shortening a longer fire.
    #[test]
    fn a_fireball_renews_its_fire_after_the_base_countdown() {
        let mut p = Projectile::new(
            ProjectileKind::SmallFireball,
            Vec3::new(0.5, 99.5, 0.5),
            None,
        );
        let air = Empty(vec![]);
        p.tick(&air);
        assert_eq!(p.fire_ticks, 20);
        assert!(
            !p.synced_fire,
            "the first renewal comes after the shared fire bit"
        );
        p.tick(&air);
        assert_eq!(p.fire_ticks, 20);
        assert!(p.synced_fire);
        p.tick(&Fire {
            targets: vec![],
            water: false,
        });
        assert_eq!(p.fire_ticks, 159);
        p.tick(&air);
        assert_eq!(p.fire_ticks, 158);
    }

    #[test]
    fn visual_fire_stays_on_when_a_pearls_fire_ticks_expire() {
        let mut server = Projectile::new(ProjectileKind::EnderPearl, Vec3::ZERO, None);
        server.fire_ticks = 1;
        server.has_visual_fire = true;
        let air = Empty(vec![]);
        server.tick(&air);
        assert_eq!(server.fire_ticks, 0);
        assert!(server.synced_fire);

        let mut client = Projectile::new(ProjectileKind::EnderPearl, Vec3::ZERO, None);
        client.synced_fire = server.synced_fire;
        client.tick_remote(&air);
        assert!(client.on_fire());
    }

    #[test]
    fn throwables_in_water_leave_bubbles_behind_them() {
        let mut p = Projectile::new(ProjectileKind::Snowball, Vec3::new(0.5, 60.0, 0.5), None);
        p.velocity = Vec3::new(1.0, 0.5, -2.0);
        assert_eq!(p.water_bubbles(), None);
        p.touching_water = true;
        let (at, speed) = p.water_bubbles().expect("a snowball in water");
        let expected_at = [0.25, 60.0 - 0.47 * 0.25, 1.0];
        let expected_speed = [1.0, 0.47, -2.0];
        for i in 0..3 {
            assert!((at[i] - expected_at[i]).abs() < 1e-12, "{at:?}");
            assert!((speed[i] - expected_speed[i]).abs() < 1e-12, "{speed:?}");
        }
        let mut bottle = Projectile::new(
            ProjectileKind::ExperienceBottle,
            Vec3::new(0.5, 60.0, 0.5),
            None,
        );
        bottle.touching_water = true;
        let (_, speed) = bottle.water_bubbles().expect("a bottle in water");
        assert!((speed[1] + 0.07).abs() < 1e-12, "{speed:?}");
        for kind in [
            ProjectileKind::Arrow {
                base_damage: 2.0,
                critical: false,
                effect: None,
            },
            ProjectileKind::LlamaSpit,
        ] {
            let mut other = Projectile::new(kind, Vec3::new(0.5, 60.0, 0.5), None);
            other.touching_water = true;
            assert_eq!(other.water_bubbles(), None);
        }
    }
    /// `AbstractHurtingProjectile.applyInertia`: four bubbles on the motion
    /// the projectile already has, with no gravity taken off first.
    #[test]
    fn hurting_projectiles_in_water_bubble_along_their_motion() {
        let expected_at = [0.25, 59.875, 1.0];
        let expected_speed = [1.0, 0.5, -2.0];
        for kind in [
            ProjectileKind::SmallFireball,
            ProjectileKind::LargeFireball { power: 1 },
            ProjectileKind::WitherSkull { dangerous: true },
            ProjectileKind::WindCharge,
            ProjectileKind::BreezeWindCharge,
        ] {
            let label = format!("{kind:?}");
            let mut p = Projectile::new(kind, Vec3::new(0.5, 60.0, 0.5), None);
            p.velocity = Vec3::new(1.0, 0.5, -2.0);
            assert_eq!(p.water_bubbles(), None, "{label} in air leaves no trail");
            p.touching_water = true;
            let (at, speed) = p.water_bubbles().expect("bubbles in water");
            for i in 0..3 {
                assert!((at[i] - expected_at[i]).abs() < 1e-12, "{label} {at:?}");
                assert!(
                    (speed[i] - expected_speed[i]).abs() < 1e-12,
                    "{label} {speed:?}"
                );
            }
        }
    }
    // Controls added for the DragonFireball owner-hit adapter
    // (`DragonFireball.onHit` ignores its own owner, `ownedBy`): a fireball
    // that has left its owner and flies through the owner's box, with another
    // mob and a wall at x = 1 behind it.
    fn dragon_fireball_through(owner: Who) -> (Projectile, (bool, Hit)) {
        let mut p = Projectile::new(
            ProjectileKind::DragonFireball,
            Vec3::new(0.0, 100.5, 0.5),
            Some(owner),
        );
        p.velocity = Vec3::new(0.5, 0.0, 0.0);
        p.left_owner = true;
        let boxed = |x: f64| Aabb::new(Vec3::new(x, 100.0, 0.0), Vec3::new(x + 0.2, 101.0, 1.0));
        let w = Wall(vec![(Who::Mob(7), boxed(0.4)), (Who::Mob(8), boxed(0.8))]);
        let result = p.tick(&w);
        (p, result)
    }

    #[test]
    fn control_a_dragon_fireball_stops_at_its_owner_and_flies_on() {
        let (p, (keep, hit)) = dragon_fireball_through(Who::Mob(7));
        assert!(keep, "no discard");
        assert_eq!(hit, Hit::None, "no impact");
        // AbstractHurtingProjectile.tick: the position is the owner hit's
        // location (the owner's box, at x = 0.4, less the margin), not the free
        // step of (0.5 + 0.1) × 0.95F; the motion is kept for the next tick.
        assert!(
            p.position.x > 0.0 && p.position.x <= 0.4,
            "{:?}",
            p.position
        );
        assert_eq!((p.position.y, p.position.z), (100.5, 0.5));
        assert!(p.velocity.x > 0.5, "{:?}", p.velocity);
    }

    #[test]
    fn control_a_dragon_fireball_hits_an_entity_that_is_not_its_owner() {
        let (_, (keep, hit)) = dragon_fireball_through(Who::Mob(9));
        assert!(!keep);
        assert!(
            matches!(
                hit,
                Hit::Entity {
                    who: Who::Mob(7),
                    ..
                }
            ),
            "{hit:?}"
        );
    }

    #[test]
    fn control_other_kinds_still_hit_their_owner_once_they_left_it() {
        let mut p = Projectile::new(
            ProjectileKind::LargeFireball { power: 1 },
            Vec3::new(0.0, 100.5, 0.5),
            Some(Who::Mob(7)),
        );
        p.velocity = Vec3::new(0.5, 0.0, 0.0);
        p.left_owner = true;
        let boxed = Aabb::new(Vec3::new(0.4, 100.0, 0.0), Vec3::new(0.6, 101.0, 1.0));
        let (keep, hit) = p.tick(&Wall(vec![(Who::Mob(7), boxed)]));
        assert!(!keep);
        assert!(matches!(
            hit,
            Hit::Entity {
                who: Who::Mob(7),
                ..
            }
        ));
    }

    /// A wall of blocks across x = 1.
    struct Wall(Vec<(Who, Aabb)>);
    impl ProjectileWorld for Wall {
        fn clip(&self, from: Vec3, to: Vec3) -> Option<(Vec3, [i32; 3], [i32; 3])> {
            (from.x < 1.0 && to.x >= 1.0)
                .then(|| (Vec3::new(1.0, from.y, from.z), [1, 100, 0], [-1, 0, 0]))
        }
        fn in_water(&self, _: Aabb) -> bool {
            false
        }
        fn targets(&self) -> &[(Who, Aabb)] {
            &self.0
        }
    }
    /// Open air inside a border from x, z = -4.5 to 5.5 (`/worldborder set
    /// 10` centred on 0.5, 0.5).
    const BORDER: [f64; 4] = [-4.5, -4.5, 5.5, 5.5];
    /// `WorldBorder.clampVec3ToBound`'s far-edge inset, the float 1.0E-5F as
    /// the class file holds it.
    const INSET: f64 = 9.999999747378752E-6;
    struct Bordered(Vec<(Who, Aabb)>);
    impl ProjectileWorld for Bordered {
        fn clip(&self, from: Vec3, to: Vec3) -> Option<(Vec3, [i32; 3], [i32; 3])> {
            clip_including_border(None, from, to, BORDER)
        }
        fn in_water(&self, _: Aabb) -> bool {
            false
        }
        fn targets(&self) -> &[(Who, Aabb)] {
            &self.0
        }
    }
    #[test]
    fn a_pearl_thrown_at_the_world_border_stops_on_it() {
        let mut p = Projectile::new(
            ProjectileKind::EnderPearl,
            Vec3::new(3.5, 100.0, 0.5),
            Some(Who::Player),
        );
        (p.left_owner, p.velocity) = (true, Vec3::new(3.0, 0.0, 0.0));
        let (keep, hit) = p.tick(&Bordered(vec![]));
        // It would end at x = 3.5 + 3 * 0.99F = 6.47, past maxX 5.5: the hit
        // is on the border, 1.0E-5F inside it, facing east (+x), at the
        // height it reached (100 - 0.03 * 0.99F).
        let at = Vec3::new(5.5 - INSET, 100.0 - 0.03 * f64::from(0.99_f32), 0.5);
        assert!(!keep, "ThrownEnderpearl.onHit discards the pearl");
        let Hit::Block {
            pos,
            at: hit_at,
            face,
        } = hit
        else {
            panic!("no border hit: {hit:?}");
        };
        assert!((hit_at.x - at.x).abs() < 1e-12, "{hit_at:?}");
        assert!((hit_at.y - at.y).abs() < 1e-9, "{hit_at:?}");
        assert_eq!(hit_at.z, 0.5);
        assert_eq!((pos, face), ([5, 99, 0], [1, 0, 0]));
        assert_eq!(p.position, hit_at, "the pearl stays where it hit");
        // It teleports its owner to where it started that tick (oldPosition),
        // inside the border.
        assert_eq!(p.previous, Vec3::new(3.5, 100.0, 0.5));
    }
    #[test]
    fn the_border_clamps_each_axis_and_faces_the_travel() {
        // Out across the near edge of z: minZ is kept as is, facing north.
        let from = Vec3::new(0.5, 70.0, -4.0);
        let hit = clip_including_border(None, from, Vec3::new(0.7, 70.0, -6.0), BORDER);
        assert_eq!(
            hit,
            Some((Vec3::new(0.7, 70.0, -4.5), [0, 70, -5], [0, 0, -1]))
        );
        // Ending on maxX itself is outside (isWithinBounds: x < maxX).
        let hit = clip_including_border(None, from, Vec3::new(5.5, 70.0, -4.0), BORDER)
            .expect("a border hit");
        assert_eq!(hit.0.x, 5.5 - INSET);
        assert_eq!(hit.2, [1, 0, 0]);
    }
    #[test]
    fn the_border_leaves_inner_hits_and_rays_from_outside_alone() {
        let inside = Vec3::new(0.5, 70.0, 0.5);
        // A miss that stays inside.
        assert_eq!(
            clip_including_border(None, inside, Vec3::new(5.0, 70.0, 5.0), BORDER),
            None
        );
        // A block struck before the border.
        let block = Some((Vec3::new(2.0, 70.0, 0.5), [2, 70, 0], [-1, 0, 0]));
        assert_eq!(
            clip_including_border(block, inside, Vec3::new(9.0, 70.0, 0.5), BORDER),
            block
        );
        // A ray that starts outside the border is not stopped by it.
        let outside = Vec3::new(7.0, 70.0, 0.5);
        assert_eq!(
            clip_including_border(None, outside, Vec3::new(9.0, 70.0, 0.5), BORDER),
            None
        );
    }
    fn thrown(kind: ProjectileKind) -> Projectile {
        let mut p = Projectile::new(kind, Vec3::new(0.0, 100.0, 0.0), Some(Who::Player));
        (p.left_owner, p.velocity) = (true, Vec3::new(1.5, 0.0, 0.0));
        p
    }
    #[test]
    fn a_tick_through_a_wall_flies_the_whole_way_and_hits_nothing() {
        for kind in [
            ProjectileKind::Snowball,
            ProjectileKind::SmallFireball,
            ProjectileKind::LlamaSpit,
        ] {
            let wall = Wall(Vec::new());
            let mut hit = thrown(kind.clone());
            let (keep, hits) = hit.tick(&wall);
            assert!(
                !keep
                    && matches!(
                        hits,
                        Hit::Block {
                            pos: [1, 100, 0],
                            ..
                        }
                    )
            );
            assert_eq!(hit.position.x, 1.0);
            // The same tick, canceled: on, as in open air.
            let mut through = thrown(kind.clone());
            let mut open = thrown(kind);
            let (kept, none) = through.tick_through(&wall);
            assert_eq!(none, Hit::None);
            let _ = open.tick(&Empty(Vec::new()));
            assert_eq!(
                (through.position, through.velocity),
                (open.position, open.velocity)
            );
            assert!(kept);
        }
    }
    #[test]
    fn a_tick_through_an_entity_does_not_hit_it() {
        let pig = Aabb::new(Vec3::new(1.0, 99.5, -0.5), Vec3::new(2.0, 100.5, 0.5));
        let world = Empty(vec![(Who::Mob(7), pig)]);
        let mut hit = thrown(ProjectileKind::EnderPearl);
        assert!(matches!(
            hit.tick(&world),
            (
                false,
                Hit::Entity {
                    who: Who::Mob(7),
                    ..
                }
            )
        ));
        let mut through = thrown(ProjectileKind::EnderPearl);
        let (keep, none) = through.tick_through(&world);
        assert!(keep && none == Hit::None);
        // Gravity and drag of the tick, then the whole of its motion.
        assert!((through.position.x - 1.5 * f64::from(0.99_f32)).abs() < 1e-9);
    }
    #[test]
    fn a_snowball_with_no_gravity_stays_where_it_was_summoned() {
        // Entity.getGravity is 0 with NoGravity: at rest, it never moves.
        let start = Vec3::new(0.5, 100.0, 0.5);
        let mut p = Projectile::new(ProjectileKind::Snowball, start, None);
        p.no_gravity = true;
        let w = Empty(vec![]);
        for _ in 0..20 {
            p.tick(&w);
        }
        assert_eq!((p.position, p.velocity), (start, Vec3::ZERO));
    }
    #[test]
    fn a_mod_thrown_type_flies_tick_for_tick_as_a_snowball() {
        static BOLT: ThrownType = ThrownType {
            id: "ruby:bolt",
            item: "ruby:gem",
            damage: 2.0,
        };
        let start = Vec3::new(0.0, 100.0, 0.0);
        let mut snowball = Projectile::new(ProjectileKind::Snowball, start, None);
        let mut bolt = Projectile::new(ProjectileKind::Thrown(&BOLT), start, None);
        for p in [&mut snowball, &mut bolt] {
            p.velocity = Vec3::new(0.3, 0.4, -0.2);
        }
        let air = Empty(Vec::new());
        for _ in 0..40 {
            assert_eq!(snowball.tick(&air), bolt.tick(&air));
            assert_eq!(
                (snowball.position, snowball.velocity, snowball.bounds()),
                (bolt.position, bolt.velocity, bolt.bounds())
            );
        }
        assert_eq!(bolt.kind.id(), "ruby:bolt");
        assert_eq!(bolt.drawn_item().unwrap().item.as_ref(), "ruby:gem");
        // thrown at a mob, it hits it and is gone, as a snowball is.
        let mut bolt = Projectile::new(ProjectileKind::Thrown(&BOLT), start, Some(Who::Player));
        (bolt.left_owner, bolt.velocity) = (true, Vec3::new(1.5, 0.0, 0.0));
        let pig = Aabb::new(Vec3::new(1.0, 99.5, -0.5), Vec3::new(2.0, 100.5, 0.5));
        let (keep, hit) = bolt.tick(&Empty(vec![(Who::Mob(7), pig)]));
        assert!(!keep);
        assert!(matches!(
            hit,
            Hit::Entity {
                who: Who::Mob(7),
                ..
            }
        ));
    }

    /// the registry is set once per process: every test registers the same.
    #[test]
    fn mod_thrown_types_are_found_by_id_and_item_once_registered() {
        register_test_types();
        assert!(!register_thrown_types([(
            "ruby:late".into(),
            "ruby:x".into(),
            1.0
        )]));
        let bolt = thrown_type("ruby:bolt").unwrap();
        assert_eq!((bolt.item, bolt.damage), ("ruby:gem", 2.0));
        assert_eq!(thrown_by_item("ruby:gem"), Some(bolt));
        assert!(
            thrown_type("ruby:late").is_none() && thrown_by_item("minecraft:snowball").is_none()
        );
        // its template's tracking settings, not the builder defaults.
        let settings = crate::lan::tracking::settings;
        assert_eq!(settings("ruby:bolt"), settings("minecraft:snowball"));
        assert_eq!(settings("ruby:late"), (5, 3, true));
    }

    /// `ShulkerBullet.tick` with no target: `applyGravity` (0.04, none under
    /// NoGravity), then the move, no drag. 20 ticks from y 110 fall 8.4.
    #[test]
    fn a_shulker_bullet_without_a_target_falls_unless_no_gravity() {
        let bullet = || {
            Projectile::new(
                ProjectileKind::ShulkerBullet {
                    target: None,
                    dir: None,
                    steps: 0,
                    delta: [0.0; 3],
                    seed: 0,
                },
                Vec3::new(0.5, 110.0, 0.5),
                None,
            )
        };
        let w = Empty(vec![]);
        let mut falling = bullet();
        let mut held = bullet();
        held.no_gravity = true;
        for _ in 0..20 {
            falling.tick(&w);
            held.tick(&w);
        }
        assert!(
            (falling.position.y - 101.6).abs() < 1e-6,
            "{}",
            falling.position.y
        );
        assert_eq!(held.position, Vec3::new(0.5, 110.0, 0.5));
    }
    /// `ShulkerBullet.tick`: a bullet turns off its axis before the block
    /// ahead when it could stand on it (`loadedAndEntityCanStandOn`, asked
    /// at the bullet's own height), and flies on toward one it could not.
    #[test]
    fn a_shulker_bullet_turns_before_a_block_it_could_stand_on() {
        struct Ahead(Vec<(Who, Aabb)>, bool);
        impl ProjectileWorld for Ahead {
            fn clip(&self, _: Vec3, _: Vec3) -> Option<(Vec3, [i32; 3], [i32; 3])> {
                None
            }
            fn in_water(&self, _: Aabb) -> bool {
                false
            }
            fn targets(&self) -> &[(Who, Aabb)] {
                &self.0
            }
            fn can_stand_on(&self, p: [i32; 3], entity_y: f64) -> bool {
                self.1 && p == [1, 64, 0] && (64.0..65.0).contains(&entity_y)
            }
        }
        // East and above: with the X axis ruled out, up is the only way on.
        let target = Aabb::new(Vec3::new(20.0, 70.0, 0.0), Vec3::new(21.0, 71.0, 1.0));
        for standable in [false, true] {
            let mut p = Projectile::new(
                ProjectileKind::ShulkerBullet {
                    target: Some(Who::Mob(2)),
                    dir: Some(5),
                    steps: 30,
                    delta: [0.15, 0.0, 0.0],
                    seed: 1,
                },
                Vec3::new(0.5, 64.5, 0.5),
                None,
            );
            p.tick(&Ahead(vec![(Who::Mob(2), target)], standable));
            let ProjectileKind::ShulkerBullet { dir, .. } = p.kind else {
                unreachable!()
            };
            assert_eq!(dir, Some(if standable { 1 } else { 5 }), "{standable}");
        }
    }
    #[test]
    fn llama_spit_moves_first_then_drags_and_falls() {
        let mut p = Projectile::new(ProjectileKind::LlamaSpit, Vec3::new(0.0, 100.0, 0.0), None);
        p.velocity = Vec3::new(1.0, 0.0, 0.0);
        let w = Empty(vec![]);
        let (keep, _) = p.tick(&w);
        assert!(keep);
        assert!((p.position.x - 1.0).abs() < 1e-9);
        assert_eq!(p.velocity.x.to_bits(), 0x3fef_ae14_8000_0000);
        assert!((p.velocity.y + 0.06).abs() < 1e-9);
    }
    #[test]
    fn arrow_damage_is_speed_times_base() {
        let mut p = Projectile::new(
            ProjectileKind::Arrow {
                base_damage: 2.0,
                critical: false,
                effect: None,
            },
            Vec3::new(0.0, 100.0, 0.0),
            None,
        );
        p.velocity = Vec3::new(3.0, 0.0, 0.0);
        let mut r = LegacyRandom::new(1);
        // ceil(3 * 2) = 6 for a non-critical arrow.
        assert_eq!(p.arrow_damage(&mut r), 6.0);
    }
    #[test]
    fn fireball_accelerates_along_its_direction() {
        let mut p = Projectile::fireball(
            ProjectileKind::SmallFireball,
            Vec3::new(0.0, 100.0, 0.0),
            None,
            [1.0, 0.0, 0.0],
        );
        let w = Empty(vec![]);
        p.tick(&w);
        // Fresh Java 26.3: (0.1 + 0.1) * (double) 0.95f.
        assert_eq!(p.velocity.x, 0.189_999_997_615_814_2);
    }

    /// `Fireball.setItem` with `Fireball.defineSynchedData`'s default and
    /// `getItem`: a fireball is drawn as a fire charge until it is given
    /// another item, and it keeps a single one of it. A kind that is no
    /// `Fireball` (a wither skull, a snowball) has no item to set.
    #[test]
    fn a_fireball_holds_one_of_the_item_it_is_given() {
        let held = |p: &Projectile| {
            p.item
                .as_ref()
                .map(|i| (&*i.item, i.count))
                .map(|(i, n)| (i.to_string(), n))
        };
        for kind in [
            ProjectileKind::LargeFireball { power: 1 },
            ProjectileKind::SmallFireball,
        ] {
            let mut p = Projectile::new(kind, Vec3::new(0.0, 100.0, 0.0), None);
            assert_eq!(
                held(&p),
                Some(("minecraft:fire_charge".to_owned(), 1)),
                "the default"
            );
            p.set_item(crate::items::ItemStack::new("minecraft:diamond", 5));
            assert_eq!(
                held(&p),
                Some(("minecraft:diamond".to_owned(), 1)),
                "copyWithCount(1)"
            );
            // `setItem` refuses an empty stack, either way of being empty, and
            // puts the default fire charge back.
            for empty in [
                crate::items::ItemStack::new("minecraft:air", 1),
                crate::items::ItemStack::new("minecraft:diamond", 0),
            ] {
                p.set_item(empty);
                assert_eq!(held(&p), Some(("minecraft:fire_charge".to_owned(), 1)));
            }
        }
        for kind in [
            ProjectileKind::WitherSkull { dangerous: false },
            ProjectileKind::Snowball,
        ] {
            let mut p = Projectile::new(kind, Vec3::new(0.0, 100.0, 0.0), None);
            assert_eq!(p.item, None);
            p.set_item(crate::items::ItemStack::new("minecraft:diamond", 1));
            assert_eq!(p.item, None, "no Fireball, no item");
        }
    }
    /// `ThrownItemRenderer`'s scale per entity type, which the item model a
    /// fireball is drawn as is scaled by: 3 for a fireball, 0.75 for a small
    /// one, 1 for the other kinds and their own renderers.
    #[test]
    fn a_fireball_draws_at_thrice_or_three_quarters() {
        let scale = |kind: ProjectileKind| {
            Projectile::new(kind, Vec3::new(0.0, 100.0, 0.0), None).sprite_scale()
        };
        assert_eq!(scale(ProjectileKind::LargeFireball { power: 1 }), 3.0);
        assert_eq!(scale(ProjectileKind::SmallFireball), 0.75);
        assert_eq!(scale(ProjectileKind::Snowball), 1.0);
    }
    #[test]
    fn fireball_inertia_preserves_custom_acceleration() {
        let mut p = Projectile::fireball(
            ProjectileKind::SmallFireball,
            Vec3::new(0.0, 100.0, 0.0),
            None,
            [1.0, 0.0, 0.0],
        );
        // A redirected projectile can carry a different acceleration power.
        p.acceleration = 0.2;
        p.tick(&Empty(vec![]));
        assert_eq!(p.velocity.x, 0.284_999_996_423_721_37);
        assert_eq!(p.position.x, 0.284_999_996_423_721_37);
    }

    #[test]
    fn hurting_inertia_widens_water_and_dangerous_skull_floats() {
        for (kind, water, expected) in [
            (ProjectileKind::SmallFireball, true, 0.160_000_002_384_185_8),
            (
                ProjectileKind::WitherSkull { dangerous: true },
                false,
                0.146_000_003_814_697_27,
            ),
        ] {
            let mut p = Projectile::new(kind, Vec3::ZERO, None);
            p.velocity = Vec3::new(0.1, 0.0, 0.0);
            p.acceleration = 0.1;
            p.apply_hurting_inertia(water);
            assert_eq!(p.velocity, Vec3::new(expected, 0.0, 0.0));
        }
    }

    #[test]
    fn a_wind_charge_holds_its_speed_until_a_deflection_gives_it_power() {
        let mut p = Projectile::new(ProjectileKind::WindCharge, Vec3::ZERO, None);
        p.velocity = Vec3::new(0.0, 1.5, 0.0);
        p.apply_hurting_inertia(false);
        assert_eq!(p.velocity, Vec3::new(0.0, 1.5, 0.0));
        // Inertia 1 in water too, and the power adds along the movement.
        p.acceleration = 0.1;
        p.apply_hurting_inertia(true);
        assert!((p.velocity.y - 1.6).abs() < 1e-12, "{p:?}");
    }

    #[test]
    fn ender_pearl_hits_a_wind_charge_box() {
        // Through the charge's box itself: a pearl on its first tick has no
        // margin (`ProjectileUtil.computeMargin`) to reach one it passes near.
        let mut pearl = Projectile::new(
            ProjectileKind::EnderPearl,
            Vec3::new(0.0, 1.1, 0.0),
            Some(Who::Player),
        );
        pearl.left_owner = true;
        pearl.velocity = Vec3::new(2.0, 0.0, 0.0);
        let wind = Projectile::new(
            ProjectileKind::WindCharge,
            Vec3::new(0.6, 1.0, 0.0),
            Some(Who::Player),
        );
        let w = Empty(vec![(Who::Mob(1), wind.aabb())]);
        let (keep, hit) = pearl.tick(&w);
        assert!(!keep, "ThrownEnderpearl.onHit discards the pearl");
        assert!(
            matches!(
                hit,
                Hit::Entity {
                    who: Who::Mob(1),
                    ..
                }
            ),
            "pearl collides with a pickable wind charge"
        );
    }
}

#[cfg(test)]
mod trail_tests {
    use super::*;
    use crate::particles::ParticleType;
    fn arrow(critical: bool) -> Projectile {
        let mut p = Projectile::new(
            ProjectileKind::Arrow {
                base_damage: 2.0,
                critical,
                effect: None,
            },
            Vec3::new(1.0, 2.0, 3.0),
            None,
        );
        p.velocity = Vec3::new(4.0, 2.0, -1.0);
        p
    }
    #[test]
    fn critical_arrow_emits_four_native_streak_samples() {
        let trail = arrow(true).arrow_trail(false);
        assert_eq!(trail.len(), 4);
        for (i, (ty, pos, speed)) in trail.iter().enumerate() {
            assert_eq!(*ty, ParticleType::Crit);
            assert_eq!(
                *pos,
                [1.0 + i as f64, 2.0 + i as f64 * 0.5, 3.0 - i as f64 * 0.25]
            );
            assert_eq!(*speed, [-4.0, -1.8, 1.0]);
        }
    }
    #[test]
    fn submerged_arrow_emits_four_bubbles_after_water_inertia() {
        let trail = arrow(false).arrow_trail(true);
        assert_eq!(trail.len(), 4);
        let drag = f64::from(0.6f32);
        for (ty, pos, speed) in trail {
            assert_eq!(ty, ParticleType::Bubble);
            assert_eq!(speed, [4.0 * drag, 2.0 * drag, -drag]);
            assert_eq!(pos, [1.0 - drag, 2.0 - drag * 0.5, 3.0 + drag * 0.25]);
        }
    }
    #[test]
    fn ordinary_and_stuck_arrows_do_not_emit_critical_trails() {
        assert!(arrow(false).arrow_trail(false).is_empty());
        let mut p = arrow(true);
        p.stuck = true;
        assert!(p.arrow_trail(true).is_empty());
    }
}
#[cfg(test)]
mod pearl_owner_tests {
    use super::*;

    /// `ThrownEnderpearl.isAllowedToTeleportOwner` in the pearl's own
    /// dimension: only a living, awake owner takes the pearl. Vanilla's
    /// `onHit` discards the pearl (and skips the 5 damage) otherwise.
    #[test]
    fn a_dead_or_sleeping_owner_cannot_take_its_pearl() {
        assert!(owner_can_take_pearl(true, false), "alive and awake");
        assert!(!owner_can_take_pearl(false, false), "a dead owner");
        assert!(!owner_can_take_pearl(true, true), "a sleeping owner");
        assert!(!owner_can_take_pearl(false, true), "dead in bed");
    }
}
#[cfg(test)]
mod render_light_tests {
    use super::*;

    fn shot(kind: ProjectileKind) -> Projectile {
        Projectile::new(kind, Vec3::new(0.5, 64.0, 0.5), None)
    }

    /// `EntityRenderer.getBlockLightLevel` and the two overrides: a shulker
    /// bullet and a fireball are 15 wherever they fly, anything on fire is 15
    /// by the base class, and an ordinary arrow keeps the level's light.
    #[test]
    fn only_the_full_bright_projectiles_override_the_block_light() {
        let bullet = shot(ProjectileKind::ShulkerBullet {
            target: None,
            dir: None,
            steps: 0,
            delta: [0.0; 3],
            seed: 0,
        });
        assert_eq!(bullet.block_light_override(), Some(15));
        assert_eq!(
            shot(ProjectileKind::SmallFireball).block_light_override(),
            Some(15)
        );
        assert_eq!(
            shot(ProjectileKind::WitherSkull { dangerous: false }).block_light_override(),
            Some(15)
        );
        let mut arrow = shot(ProjectileKind::Arrow {
            base_damage: 2.0,
            critical: false,
            effect: None,
        });
        assert_eq!(arrow.block_light_override(), None);
        // `Entity.isOnFire` on any projectile is 15, whatever its kind.
        arrow.ignite_for_ticks(100);
        assert_eq!(arrow.block_light_override(), Some(15));
        let mut snowball = shot(ProjectileKind::Snowball);
        assert_eq!(snowball.block_light_override(), None);
        snowball.ignite_for_ticks(1);
        assert_eq!(snowball.block_light_override(), Some(15));
    }

    /// `Entity.getLightProbePosition` is the eye position, so an arrow lying
    /// in the top eighth of a block is lit by the block above it.
    #[test]
    fn a_projectile_is_lit_from_the_block_its_eye_is_in() {
        let mut arrow = shot(ProjectileKind::Arrow {
            base_damage: 2.0,
            critical: false,
            effect: None,
        });
        arrow.position = Vec3::new(0.5, 64.9, 0.5);
        assert_eq!(arrow.light_probe_y(1.0).floor(), 65.0);
        assert_eq!(arrow.position.y.floor(), 64.0);
        // A wind charge's entity type declares no eye height, so it keeps its feet.
        let mut wind = shot(ProjectileKind::WindCharge);
        wind.position = Vec3::new(0.5, 64.9, 0.5);
        assert_eq!(wind.light_probe_y(1.0), 64.9);
        // A snowball's box is a quarter block: the eye is 0.2125 up.
        let mut ball = shot(ProjectileKind::Snowball);
        ball.position = Vec3::new(0.5, 64.8, 0.5);
        assert!((ball.light_probe_y(1.0) - 64.8 - 0.85 * 0.25).abs() < 1e-6);
    }

    /// `getEyePosition(partialTick)` reads `getPosition(partialTick)`: an arrow
    /// falling from 64 to 65 is probed at 64.63 for the first half of the frame
    /// (the cell at 64) and only reaches the cell at 65 when the frame ends.
    /// Horizon read the tick's own position, so the light popped a frame early.
    #[test]
    fn a_projectile_probe_follows_the_frame() {
        let mut arrow = shot(ProjectileKind::Arrow {
            base_damage: 2.0,
            critical: false,
            effect: None,
        });
        arrow.previous = Vec3::new(0.5, 64.0, 0.5);
        arrow.position = Vec3::new(0.5, 65.0, 0.5);
        assert!((arrow.light_probe_y(0.0) - 64.13).abs() < 1e-6);
        assert_eq!(arrow.light_probe_y(0.0).floor(), 64.0);
        assert_eq!(arrow.light_probe_y(0.5).floor(), 64.0);
        assert!((arrow.light_probe_y(0.5) - 64.63).abs() < 1e-6);
        assert_eq!(arrow.light_probe_y(1.0).floor(), 65.0);
        // The old probe, this tick's position, was 65.0 for every frame.
        assert_eq!((arrow.position.y + arrow.kind.eye_height()).floor(), 65.0);
    }

    #[test]
    fn projectile_entity_type_parity() {
        let arrow = shot(ProjectileKind::Arrow {
            base_damage: 2.0,
            critical: false,
            effect: None,
        });
        assert_eq!(arrow.entity_type(), "minecraft:arrow");
        let spectral = shot(ProjectileKind::Arrow {
            base_damage: 2.0,
            critical: false,
            effect: Some(("minecraft:glowing", 200)),
        });
        assert_eq!(spectral.entity_type(), "minecraft:spectral_arrow");
        assert_eq!(
            shot(ProjectileKind::SmallFireball).entity_type(),
            "minecraft:small_fireball"
        );
    }
}
crate::impl_joined!(Projectile);
