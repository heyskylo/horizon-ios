//! Mobs in the game loop: ticking with entity snapshots, natural and
//! chunk-generation spawning (`NaturalSpawner`), mob events (attacks,
//! breeding, conversions, drops), projectiles, player interactions
//! (feeding, shearing, milking, spawn eggs, throwing) and rendering.
mod pufferfish_player;

use super::{
    Dim, Game, Hand, InteractTarget, entity_culling::EntityCullDecision,
    native_mob_draw::NativePlayerRender, native_projectiles::Impact, view_vector,
};
use horizon_assets::item_model::DisplayContext;
use horizon_client::actions::Action;
use horizon_client::{
    Result,
    arrows::EntityHit,
    blocks::Fluid,
    collisions::SceneCollisions,
    crafting::Recipes,
    graphics::EntityCullingMode,
    health::{HealthEvent, HurtSource, Hurting, Knock, knockback_canceled},
    items::ItemStack,
    mesher::Vertex,
    mobs::{
        Damage, DamageKind, Mob, MobEvent, MobWorld, PlayerView, Seen, SpawnSetup, Species, Who,
        ai::{eating_sound, feed_animal, takes_food},
        farm::Climate,
        spawning::{self, BiomeFacts},
    },
    mod_api::NativeLootDrops,
    particles::{ParticleOptions, ParticleType},
    projectiles::{Hit, Projectile, ProjectileKind, ProjectileWorld, clip_including_border},
};
use horizon_core::{
    geometry::{Aabb, Direction, Vec3, traverse_blocks, trig::Mth},
    position::BlockPos,
    random::RandomSource,
};
use std::sync::Arc;

/// `AbstractArrow.onHitEntity` when `hurtOrSimulate` returned false.
/// An enderman that teleported (`projectileReceivesSideEffectsOnHit` is
/// false) leaves the arrow in flight. Every other refusal, including
/// `Shulker.hurtServer` while `isClosed()`, is
/// `deflect(ProjectileDeflection.REVERSE, entity, owner, false, 0.2)`.
fn arrow_miss(dodged: bool) -> EntityHit {
    if dodged {
        EntityHit::Dodged
    } else {
        EntityHit::Refused
    }
}
/// The world impact sound after successful arrow damage, including self-shots.
/// `AbstractArrow.onHitEntity`'s damage: the crit bonus comes from the
/// arrow's own random.
fn arrow_damage(arrow: &mut Projectile, fallback: &mut impl RandomSource) -> f32 {
    let mut state = arrow.arrow.take();
    let damage = match state.as_mut() {
        Some(a) => arrow.arrow_damage(&mut a.random),
        None => arrow.arrow_damage(fallback),
    };
    arrow.arrow = state;
    damage
}

/// A sound a native mod queued, sent when the host is free.
pub(super) struct NativeSound {
    pub dimension: &'static str,
    pub sound: String,
    pub at: [f64; 3],
    pub source: i32,
    pub volume: f32,
    pub pitch: f32,
}

/// Mob-system state owned by the game.
#[derive(Default)]
pub(super) struct Creatures {
    pub projectiles: horizon_client::join_seq::JoinList<Projectile>,
    /// The entities a family tells the mods ticked, kept between ticks
    /// (game/native_ticks.rs).
    pub tick_buffer: Vec<(u32, super::native_ticks::TickKind)>,
    /// Wind bursts from a guest's turn, waiting for it to end (game/wind.rs).
    pub wind_bursts: Vec<super::wind::Burst>,
    /// The wind charge whose burst is being handled, by entity id: which
    /// entities have already ticked in vanilla's spawn-ordered tick list
    /// (game/wind.rs).
    pub wind_source: Option<horizon_client::join_seq::JoinSeq>,
    /// Wind pushes held until the entity's own tick has run, by entity id
    /// (game/wind.rs `apply_late_wind_pushes`).
    pub late_wind_pushes: Vec<(u32, crate::game::Vec3)>,
    /// The players a fishing hook can catch, taken as the hooks tick, and
    /// the pulls on them waiting for the hub (game/fishing.rs).
    pub hook_targets: Vec<super::fishing::HookTarget>,
    pub hook_pulls: Vec<(u32, horizon_core::geometry::Vec3)>,
    /// Mace smash knockback on the other players from a guest's turn, waiting
    /// for it to end (game/mace.rs).
    pub smash_pushes: Vec<super::mace::SmashPush>,
    /// Sweep attacks from a guest's turn, waiting for it to end
    /// (game/lan_pvp.rs).
    pub sweeps: Vec<super::lan_pvp::Sweep>,
    /// Explosion packets from a guest's turn, waiting for it to end
    /// (centre, small, radius, blocks hit, the players it pushed;
    /// game/lan_particles.rs).
    pub lan_explosions: Vec<([f64; 3], bool, f32, usize, Vec<(u32, [f64; 3])>)>,
    /// Ticks left of the elder guardian's apparition (MobAppearanceParticle).
    pub elder_apparition: i32,
    /// The snapshot's buffer between ticks, empty: ~400 bytes a mob, so
    /// thousands of mobs would allocate and free megabytes every tick.
    seen_buf: Vec<Seen>,
    /// Per-tick scratch retained between creature ticks.
    event_buf: Vec<MobEvent>,
    /// The targets the mobs had before their ticks, kept between ticks
    /// (empty) as `seen_buf` is: `native_living_change_target` hears a change.
    target_buf: Vec<Option<Who>>,
    /// Pickable projectile hitboxes reused while ticking pearls and arrows.
    pickable_projectile_targets: Vec<(Who, Aabb)>,
    /// The mobs whose own tick ran in the last `tick_creature_entities`:
    /// `ServerLevel.tick` ticks only the entities in entity-ticking range, so
    /// what the passes after the loop do for a mob's `aiStep`, goals or brain
    /// (pickups, item searches) is for these alone.
    pub ticked: horizon_client::fast_hash::FxHashSet<u32>,
    /// The wandering trader spawner's clock.
    pub trader: super::wandering_trader::Spawner,
    /// When each player last put a parrot on a shoulder (`timeEntitySatOnShoulder`).
    pub shoulder_time: std::collections::HashMap<[i32; 4], i64>,
    /// The cat spawner's countdown (`CatSpawner.nextTick`).
    pub cat_tick: i32,
    /// Native sounds waiting for the server host to be available.
    pub lan_sounds: Vec<NativeSound>,
    /// Particles from a guest's turn, waiting for it to end.
    pub lan_particles: Vec<super::lan_particles::QueuedParticle>,
}
/// Whether `p`'s next step goes from inside the world border (`bounds`: [minX, minZ, maxX,
/// maxZ]) to outside it, as `clip_including_border` would stop it.
fn border_crossing(p: &Projectile, bounds: Option<[f64; 4]>) -> bool {
    let Some([min_x, min_z, max_x, max_z]) = bounds else {
        return false;
    };
    let within = |x: f64, z: f64| x >= min_x && x < max_x && z >= min_z && z < max_z;
    let (at, v) = (p.position, p.velocity);
    within(at.x, at.z) && !within(at.x + v.x, at.z + v.z)
}

pub(super) struct ProjWorld<'a, 'w> {
    pub w: &'a MobWorld<'w>,
    pub targets: Vec<(Who, Aabb)>,
    /// The world border's [minX, minZ, maxX, maxZ], which stops the ray as a
    /// block would (`clipIncludingBorder`); None: no border.
    pub border: Option<[f64; 4]>,
}
impl ProjectileWorld for ProjWorld<'_, '_> {
    fn block_contacts(&self, entity: i32, kind: &str, from: Vec3, to: Vec3, bounds: Aabb) {
        horizon_client::block_contact::swept_inside(entity, kind, from, to, bounds, |p| {
            let state = self.w.world.block(p.x, p.y, p.z);
            (state, &self.w.facts.get(state).name)
        });
    }
    fn clip(&self, from: Vec3, to: Vec3) -> Option<(Vec3, [i32; 3], [i32; 3])> {
        let mut scene = SceneCollisions {
            world: self.w.world,
            binding: self.w.collision,
            subject: None,
        };
        let context = horizon_world::collision_context::CollisionContext::position(from.y);
        let r = traverse_blocks(from, to, 256, |p: BlockPos| {
            let state = self.w.world.block(p.x, p.y, p.z);
            if self.w.facts.is_air(state) {
                return None;
            }
            // ClipContext hands CollisionContext.of(projectile): scaffolding
            // is open for a projectile below it.
            let shape = self
                .w
                .collision
                .collision_shape_with_context(&mut scene, state, p, &context)
                .ok()?;
            shape.clip(from, to, p)
        });
        let hit = r.ok().flatten().map(|hit| {
            (
                hit.location,
                [hit.block.x, hit.block.y, hit.block.z],
                hit.direction.normal(),
            )
        });
        match self.border {
            Some(bounds) => clip_including_border(hit, from, to, bounds),
            None => hit,
        }
    }
    /// Entity.updateFluidInteraction over the projectile's box, not the
    /// block at its feet (#3072).
    fn in_water(&self, bounds: Aabb) -> bool {
        self.w.fluids(bounds, bounds.max_position().y).in_water
    }
    fn targets(&self) -> &[(Who, Aabb)] {
        &self.targets
    }
    /// `BaseFireBlock.entityInside` for a thrown projectile: fire and soul
    /// fire set it alight for 160 ticks.
    fn ignites(&self, p: [i32; 3]) -> bool {
        let state = self.w.world.block(p[0], p[1], p[2]);
        matches!(
            &*self.w.facts.get(state).name,
            "minecraft:fire" | "minecraft:soul_fire"
        )
    }
    fn empty(&self, p: [i32; 3]) -> bool {
        self.w.facts.is_air(self.w.world.block(p[0], p[1], p[2]))
    }
    /// `WebBlock.entityInside` and `PowderSnowBlock.entityInside`.
    fn stuck_speed(&self, p: [i32; 3]) -> Option<Vec3> {
        horizon_client::projectiles::stuck_speed_of(
            &self.w.facts.get(self.w.world.block(p[0], p[1], p[2])).name,
        )
    }
    fn bubble_column(&self, p: [i32; 3]) -> Option<(bool, bool)> {
        horizon_client::motion::bubble_column_at(
            self.w.world,
            self.w.registry,
            self.w.facts,
            self.w.collision,
            BlockPos::new(p[0], p[1], p[2]),
        )
        .map(|c| (c.drag, c.surface))
    }
    fn can_stand_on(&self, p: [i32; 3], entity_y: f64) -> bool {
        horizon_client::collisions::projectile_can_stand_on(
            self.w.world,
            self.w.collision,
            BlockPos::new(p[0], p[1], p[2]),
            entity_y,
        )
    }
}

/// `Projectile.isPickable`: a projectile in `#minecraft:redirectable_projectile`
/// (a ghast's fireball and both wind charges) is pickable, so another
/// projectile's `ProjectileUtil.getEntityHitResult` can strike it
/// (`Projectile.onHit` then redirects it). `PICKABLE_PROJECTILE_IDS + j` is
/// `projectiles[j]` during `tick_projectiles`, below the dragon's reserved ids.
const PICKABLE_PROJECTILE_IDS: u32 = super::dragon::SPECIAL - 0x10000;

/// The index of the projectile a `pickable_projectile_hitboxes` id stands for.
pub(super) fn pickable_projectile_hit(id: u32) -> Option<usize> {
    (PICKABLE_PROJECTILE_IDS..super::dragon::SPECIAL)
        .contains(&id)
        .then(|| (id - PICKABLE_PROJECTILE_IDS) as usize)
}

fn pickable_projectile_hitboxes_into(
    projectiles: &[Projectile],
    alive: &[bool],
    skip: usize,
    out: &mut Vec<(Who, Aabb)>,
) {
    out.clear();
    out.extend(
        projectiles
            .iter()
            .enumerate()
            .filter(|(j, q)| *j != skip && alive[*j] && super::deflect::redirectable(q))
            .map(|(j, q)| (Who::Mob(PICKABLE_PROJECTILE_IDS + j as u32), q.aabb())),
    );
}

#[cfg(test)]
fn pickable_projectile_hitboxes(
    projectiles: &[Projectile],
    alive: &[bool],
    skip: usize,
) -> Vec<(Who, Aabb)> {
    let mut out = Vec::new();
    pickable_projectile_hitboxes_into(projectiles, alive, skip, &mut out);
    out
}

/// `FlameFeatureRenderer`: fire quads stacked up a burning entity's box,
/// turned to the camera around Y, at full block light. `at` is the entity's
/// feet relative to the camera.
fn flame_geometry(
    atlas: &horizon_client::hud::HudAtlas,
    at: glam::Vec3,
    width: f32,
    height: f32,
    camera_yaw: f32,
    light: u8,
    out: &mut Vec<Vertex>,
) {
    let (Some(fire0), Some(fire1)) = (
        atlas.sprites.get("block/fire_0").copied(),
        atlas.sprites.get("block/fire_1").copied(),
    ) else {
        return;
    };
    let s = width * 1.4;
    let mut h = height / s;
    // Mth.rotationAroundAxis(Y, camera.orientation): the camera's turn about Y.
    let turn = glam::Mat3::from_rotation_y(std::f32::consts::PI - camera_yaw.to_radians());
    let lift = 0.3 - (h as i32) as f32 * 0.02;
    let packed = (u32::from(light >> 4) * 16) | ((15 * 16) << 8);
    let (mut r, mut yo, mut zo, mut ss) = (0.5f32, 0.0f32, 0.0f32, 0);
    while h > 0.0 {
        let tex = if ss % 2 == 0 { fire0 } else { fire1 };
        let (mut u0, v0, mut u1, v1) = (tex.u0, tex.v0, tex.u1, tex.v1);
        if ss / 2 % 2 == 0 {
            std::mem::swap(&mut u0, &mut u1);
        }
        for (x, y, uv) in [
            (-r, -yo, [u1, v1]),
            (r, -yo, [u0, v1]),
            (r, 1.4 - yo, [u0, v0]),
            (-r, 1.4 - yo, [u1, v0]),
        ] {
            let p = at + turn * glam::Vec3::new(x, y, zo + lift) * s;
            out.push(Vertex {
                normal: [0; 4],
                pos: p.to_array(),
                uv,
                // HDR output: the flames shine like fire (entity.wgsl fs_hdr_mark).
                layer: u32::from(horizon_client::mesher::INCANDESCENT) << 16,
                color: 0xFFFF_FFFF,
                light: packed,

                glint_uv: [0.0, 0.0],
            });
        }
        h -= 0.45;
        yo -= 0.45;
        r *= 0.9;
        zo -= 0.03;
        ss += 1;
    }
}
pub(super) fn seen(m: &Mob, player: Option<[i32; 4]>) -> Seen {
    m.seen(player)
}
/// A mob's target as `native_living_change_target` names it: a player's UUID
/// or a mob's id, both None for no target; None for one it cannot name.
/// `Who::Player` is the guest the mob sees (`seen`, a connection) or, with
/// none, the host's own player (`host`); a guest's entity (`guest_ids`, by
/// connection) is that guest.
fn target_named(
    target: Option<Who>,
    seen: Option<u32>,
    guests: &super::lan_mobs::GuestViews,
    guest_ids: &[(u32, u32)],
    host: Option<&PlayerView>,
) -> Option<(Option<u128>, Option<u32>)> {
    let uuid = |v: &PlayerView| horizon_client::lan::playerdata::ints_uuid(v.uuid);
    let guest = |conn: u32| guests.iter().find(|g| g.0 == conn).map(|g| uuid(&g.1));
    match target {
        None => Some((None, None)),
        Some(Who::Player) => {
            let player = match seen {
                Some(conn) => guest(conn),
                None => host.map(uuid),
            };
            Some((Some(player?), None))
        }
        Some(Who::Mob(id)) => match guest_ids.iter().find(|g| g.1 == id) {
            Some(&(conn, _)) => Some((Some(guest(conn)?), None)),
            None => Some((None, Some(id))),
        },
    }
}
/// Whether `e`, from a mob's tick, is a hit it deals `target`: a melee hit
/// (with its effect), a sting, a sonic boom.
fn hits_target(e: &MobEvent, want: Who) -> bool {
    match (e, want) {
        (
            MobEvent::AttackPlayer(..)
            | MobEvent::AttackPlayerWithEffect { .. }
            | MobEvent::StingPlayer { .. },
            Who::Player,
        ) => true,
        (
            MobEvent::AttackMob { target, .. }
            | MobEvent::DamageMob { target, .. }
            | MobEvent::AttackMobWithEffect { target, .. },
            Who::Mob(id),
        ) => *target == id,
        (MobEvent::SonicBoom { target, .. }, _) => *target == want,
        _ => false,
    }
}
/// Whether a mob a packet named may take the click: `mob_hit` skips a mob in
/// its death animation, the one the player rides (it shares the player's root
/// vehicle) and the camera entity the pick starts from, so a packet that names
/// one of those does nothing rather than reaching the feed, shear, leash or
/// name paths the pick path refuses.
pub(super) fn named_mob_pickable(
    alive: bool,
    id: u32,
    riding: Option<u32>,
    camera: Option<u32>,
) -> bool {
    alive && Some(id) != riding && Some(id) != camera
}

/// What an entity drew on the GPU path: the skin frame's mark before its draws, and their
/// camera-relative bounds (`cull_skinned`).
pub(super) type Skinned = Option<((usize, usize, u32), Option<(glam::Vec3, glam::Vec3)>)>;

/// What a block's `onProjectileHit` reads of the projectile that struck it:
/// a thrown projectile ([`ProjectileHitter::of`]) or a fishing bobber
/// (`FishingHook.onHitBlock` calls `Projectile.onHitBlock` too).
#[derive(Clone, Copy, Debug)]
pub(super) struct ProjectileHitter {
    /// `Projectile.getOwner`.
    pub(super) owner: Option<Who>,
    /// On a server, the guest (by connection) who owns it.
    pub(super) guest: Option<u32>,
    /// An `AbstractArrow` (a target block resets after 20 ticks, not 8).
    pub(super) arrow: bool,
    /// Its entity type, for `target_hit`'s projectile predicate.
    pub(super) kind: &'static str,
    pub(super) position: Vec3,
}
impl ProjectileHitter {
    pub(super) fn of(p: &Projectile) -> Self {
        Self {
            owner: p.owner,
            guest: p.guest,
            arrow: p.arrow.is_some(),
            kind: p.entity_type(),
            position: p.position,
        }
    }
}

/// How a new mob's yaw is chosen, as its vanilla spawn path chooses it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum SpawnYaw {
    /// `level.random.nextFloat() * 360`, unwrapped.
    Level,
    /// `EntityType.create(level, .., pos, reason, ..)` (spawn eggs, a
    /// dispenser's eggs, mob buckets): `snapTo(.., Mth.wrapDegrees(level.random
    /// .nextFloat() * 360), 0)`.
    Created,
    /// A yaw that takes no draw: `SummonCommand.createEntity` loads the mob
    /// from its compound (0 without `Rotation`), and
    /// `SpawnEggItem.spawnOffspringFromSpawnEgg` snaps the baby to 0.
    Fixed(f32),
}

impl Game<'_> {
    /// A sulfur cube's attribute map (`LivingEntity.attributes`), from its
    /// type's defaults, before its first tick adopts the body's archetype
    /// modifiers. A map already there (commands, armor) is kept.
    fn init_sulfur_attribute_maps(&mut self) {
        for m in &mut self.mobs {
            if m.species() == horizon_client::mobs::Species::SulfurCube
                && m.extra.armor_attributes.is_none()
            {
                m.extra.armor_attributes = self.motion.attributes.create(&m.kind.id).map(Box::new);
            }
        }
    }
}
impl Game<'_> {
    pub(super) fn player_view(&self) -> Option<PlayerView> {
        let p = self.player.as_ref()?;
        let w = f64::from(p.dimensions.width) / 2.0;
        let bounds = Aabb::new(
            Vec3::new(p.position.x - w, p.position.y, p.position.z - w),
            Vec3::new(
                p.position.x + w,
                p.position.y + f64::from(p.dimensions.height),
                p.position.z + w,
            ),
        );
        Some(PlayerView {
            position: p.position,
            bounds,
            eye: [
                p.position.x,
                p.position.y + f64::from(p.dimensions.eye_height),
                p.position.z,
            ],
            alive: !p.health.dead_or_dying(),
            // EntitySelector.NO_CREATIVE_OR_SPECTATOR.
            creative: self.creative || self.spectator(),
            sneaking: self.keys.held(Action::Sneak),
            swimming: p.swimming,
            look: view_vector(self.yaw, self.pitch),
            main_hand: self.held().map(|s| s.item.clone()),
            off_hand: self.offhand.as_ref().map(|s| s.item.clone()),
            head: self.armor[0].as_ref().map(|s| s.item.clone()),
            chest: self.armor[1].as_ref().map(|s| s.item.clone()),
            legs: self.armor[2].as_ref().map(|s| s.item.clone()),
            feet: self.armor[3].as_ref().map(|s| s.item.clone()),
            armor_cover: self.armor.iter().filter(|a| a.is_some()).count() as f32 / 4.0,
            invisible: self.potions.effects.get("invisibility").is_some(),
            spectator: self.spectator(),
            gold_armor: self.wearing_gold(),
            hurt_by: self.player_hurt_by(),
            last_hurt: self.combat.last_hurt_mob.map(|(id, at)| (Who::Mob(id), at)),
            velocity: p.velocity,
            yaw: self.yaw,
            sprinting: p.sprinting,
            riding: self.riding.is_some(),
            hero: self.potions.effects.get("hero_of_the_village").is_some(),
            effects: self.potions.effects.list.iter().map(|e| e.id).collect(),
            uuid: self.level.player_uuid.unwrap_or_default(),
            allies: std::sync::Arc::new(self.player_allies()),
            remote: false,
            sleeping: self.sleeping.is_some(),
            sleep_timer: self.sleep_timer,
            passenger: self.riding.is_some(),
            riding_vehicle: self.riding,
            vehicle: self.riding.and_then(|id| {
                if let Some(b) = self.boats.iter().find(|b| b.id == id) {
                    // A boat's box: 1.375 wide and 0.5625 high around its position.
                    let w = 0.6875;
                    return Some(horizon_client::mobs::Vehicle {
                        nautilus: false,
                        bounds: Aabb::new(
                            Vec3::new(b.position.x - w, b.position.y, b.position.z - w),
                            Vec3::new(b.position.x + w, b.position.y + 0.5625, b.position.z + w),
                        ),
                        velocity: b.velocity,
                    });
                }
                let m = self.mobs.iter().find(|m| m.id == id)?;
                matches!(m.species(), Species::Nautilus | Species::ZombieNautilus).then(|| {
                    horizon_client::mobs::Vehicle {
                        nautilus: true,
                        bounds: m.bounds(),
                        velocity: m.velocity,
                    }
                })
            }),
        })
    }
    /// Biome facts at a position for finalizeSpawn and variant rules.
    pub(super) fn biome_facts(&self, x: i32, y: i32, z: i32) -> BiomeFacts {
        let Some(chunk) = self.world.chunks.get(&(x >> 4, z >> 4)) else {
            return BiomeFacts::default();
        };
        let b = world_biomes!(self).get(chunk.biome(x, y, z));
        let tags = self.res.mob_data();
        let has = |t: &str| tags.biome_has(t, &b.name);
        let climate = if has("minecraft:spawns_warm_variant_farm_animals") {
            Climate::Warm
        } else if has("minecraft:spawns_cold_variant_farm_animals") {
            Climate::Cold
        } else {
            Climate::Temperate
        };
        BiomeFacts {
            coral_nautilus: has("minecraft:spawns_coral_variant_zombie_nautilus"),
            climate,
            temperature: b.temperature,
            snowy: has("minecraft:spawns_snow_foxes")
                || b.name.contains("snow")
                || b.name.contains("frozen"),
            snow_fox: has("minecraft:spawns_snow_foxes"),
            swamp: b.name.contains("swamp"),
            ocean: has("minecraft:is_ocean"),
            mushroom: b.name == "minecraft:mushroom_fields",
            desert: b.name == "minecraft:desert",
            jungle: has("minecraft:is_jungle"),
            wolf: horizon_client::mobs::tame::wolf_variant_for(&b.name, &has),
            full_moon: (self.world.day_time / 24000).rem_euclid(8) == 0,
            frog: if has("minecraft:spawns_warm_variant_frogs") {
                1
            } else if has("minecraft:spawns_cold_variant_frogs") {
                2
            } else {
                0
            },
        }
    }
    /// Create a mob of `kind` at a position, finalized like a natural spawn.
    pub(super) fn spawn_mob(&mut self, kind: &str, at: [f64; 3], persistent: bool) -> Option<u32> {
        self.spawn_mob_for(kind, at, persistent, false)
    }
    /// Create a mob of `kind` at a position without `finalizeSpawn`'s rolls,
    /// matching vanilla `EntityType.create(level, reason)`.
    pub(super) fn spawn_mob_raw(
        &mut self,
        kind: &str,
        at: [f64; 3],
        persistent: bool,
    ) -> Option<u32> {
        self.spawn_mob_internal(kind, at, persistent, false, false, false, SpawnYaw::Level)
    }
    /// `SummonCommand` and `SpawnEggItem`: vanilla spawns the mob, but it
    /// never calls `Mob.setPersistenceRequired` for it, so the mob is not
    /// persistent: the despawn rule holds it like any other mob, and only
    /// its NBT can make it persistent.
    pub(super) fn spawn_mob_spawned(
        &mut self,
        kind: &str,
        at: [f64; 3],
        yaw: SpawnYaw,
    ) -> Option<u32> {
        // Ender Dragons have their own entity implementation rather than a
        // `Mob` entry.  Spawn eggs still go through `SpawnEggItem.spawnMob`,
        // so route this special entity through the same spawned-entity path.
        if matches!(kind, "ender_dragon" | "minecraft:ender_dragon") {
            return self.spawn_dragon_spawned(at);
        }
        // The spawn is not a natural one (no jockeys, the patrol leader roll
        // stays), but the mob is not persistent.
        let id = self.spawn_mob_reason_yaw(kind, at, true, false, false, yaw)?;
        if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
            m.persistent = false;
        }
        Some(id)
    }
    /// `spawn_mob` for a patrol or raid (`EntitySpawnReason.PATROL` / `EVENT`),
    /// which never rolls a patrol leader.
    pub(super) fn spawn_mob_for(
        &mut self,
        kind: &str,
        at: [f64; 3],
        persistent: bool,
        scripted: bool,
    ) -> Option<u32> {
        self.spawn_mob_reason(kind, at, persistent, scripted, !persistent)
    }
    /// `spawn_mob_for` with the spawn reason's NATURAL flag explicit: only a
    /// natural spawn rolls the jockey mounts of zombie horses, husks and
    /// drowned (`reinforcement` and spawner spawns pass `false`).
    pub(super) fn spawn_mob_reason(
        &mut self,
        kind: &str,
        at: [f64; 3],
        persistent: bool,
        scripted: bool,
        natural: bool,
    ) -> Option<u32> {
        self.spawn_mob_internal(
            kind,
            at,
            persistent,
            scripted,
            natural,
            true,
            SpawnYaw::Level,
        )
    }
    /// `spawn_mob_reason` with the new mob's yaw chosen as its vanilla path does.
    pub(super) fn spawn_mob_reason_yaw(
        &mut self,
        kind: &str,
        at: [f64; 3],
        persistent: bool,
        scripted: bool,
        natural: bool,
        yaw: SpawnYaw,
    ) -> Option<u32> {
        self.spawn_mob_internal(kind, at, persistent, scripted, natural, true, yaw)
    }
    fn spawn_mob_internal(
        &mut self,
        kind: &str,
        at: [f64; 3],
        persistent: bool,
        scripted: bool,
        natural: bool,
        finalize: bool,
        yaw: SpawnYaw,
    ) -> Option<u32> {
        let kind = if kind.contains(':') {
            kind.to_owned()
        } else {
            format!("minecraft:{kind}")
        };
        let t = self.res.mob_types().get(&kind).cloned()?;
        self.next_entity += 1;
        let yaw = match yaw {
            SpawnYaw::Level => self.world.random.next_float() * 360.0,
            SpawnYaw::Created => {
                horizon_client::camera::wrap_degrees(self.world.random.next_float() * 360.0)
            }
            SpawnYaw::Fixed(yaw) => yaw,
        };
        // The mob's own random (Entity.random): its UUID first, then LivingEntity's
        // yaw draw, apart from the spawn yaw above, which comes from the level's.
        let (own, uuid) = horizon_client::mobs::living_random();
        self.persist.uuids.insert(self.next_entity, uuid);
        let position = Vec3::new(at[0], at[1], at[2]);
        let mut mob = Mob::with_random(self.next_entity, t, position, yaw, own);
        mob.persistent = persistent;
        if finalize {
            let facts = self.biome_facts(
                at[0].floor() as i32,
                at[1].floor() as i32,
                at[2].floor() as i32,
            );
            let difficulty = self.local_difficulty(at);
            spawning::finalize(&mut mob, &facts, &difficulty);
            if !scripted {
                let reason = if persistent {
                    horizon_client::mobs::raider::Reason::Other
                } else {
                    horizon_client::mobs::raider::Reason::Natural
                };
                horizon_client::mobs::raider::finalize(&mut mob, reason);
            }
        }
        let id = mob.id;
        self.mobs.push(mob);
        self.native_mobs_joined(self.mobs.len() - 1, false);
        if !self.mobs.iter().any(|m| m.id == id) {
            return None;
        }
        if finalize {
            self.enchant_spawn_equipment(id);
            self.halloween_spawn_head(id);
            if !persistent && !scripted {
                self.spawn_jockeys(id, natural);
            }
        }
        Some(id)
    }
    /// `Mob.burnUndead` for a mob in daylight with an item on its sun
    /// protection slot (the head, or the body for a zombie horse or
    /// nautilus): `hurtAndBreak(random.nextInt(2))` on a damageable item, and
    /// when it breaks the slot empties and `onEquippedItemBroken` broadcasts
    /// the break event.
    fn wear_sun_protection(&mut self, id: u32) {
        use horizon_world::player_inventory::EquipmentSlot;
        let Some(m) = self.mobs.iter().find(|m| m.id == id) else {
            return;
        };
        let body = horizon_client::mobs::drops::body_slot_species(m.species());
        let slot = if body {
            EquipmentSlot::Body
        } else {
            EquipmentSlot::Head
        };
        let Some(mut stack) = m.item_by_slot(slot) else {
            return;
        };
        if !horizon_client::stations::is_damageable(self.res.items(), &stack) {
            return;
        }
        let at = [m.position.x, m.position.y, m.position.z];
        let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) else {
            return;
        };
        let amount = m.random.next_int_bound(2).unwrap_or(0) as u32;
        if amount == 0 {
            return;
        }
        let loss = self.durability_loss(&stack, amount);
        if loss == 0 {
            return;
        }
        let max = self
            .res
            .items()
            .get(&stack.item)
            .map_or(0, |i| i.max_damage);
        stack.damage += loss;
        let broke = stack.damage >= max;
        let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) else {
            return;
        };
        match (body, broke) {
            (true, false) => m.extra.body_damage = stack.damage as i32,
            (true, true) => {
                m.extra.body = None;
                m.extra.body_damage = 0;
            }
            (false, false) => m.extra.armor[0] = Some(stack),
            (false, true) => {
                m.extra.head = None;
                m.extra.armor[0] = None;
            }
        }
        if broke {
            // LivingEntity.entityEventForEquipmentBreak: head 49, body 65.
            self.lan_entity_event(id as i32, if body { 65 } else { 49 }, at);
        }
    }
    /// `LivingEntity.updateFallFlying`'s wear, `ItemStack.hurtAndBreak(1, this,
    /// slot)` on the glider `MobEvent::GliderWear` picked: Unbreaking (from the
    /// level's random, `Enchantment.modifyItemFilteredCount`) may save it, and
    /// the rest goes on the stack where it is worn. A glider is only picked
    /// while its next point of wear will not break it (`canGlideUsing`), so
    /// this wear never breaks one.
    fn wear_mob_glider(&mut self, id: u32, slot: horizon_world::player_inventory::EquipmentSlot) {
        use horizon_client::mobs::finalize;
        use horizon_world::player_inventory::EquipmentSlot as S;
        let Some(stack) = self
            .mobs
            .iter()
            .find(|m| m.id == id)
            .and_then(|m| finalize::equipment_stack(m, slot))
        else {
            return;
        };
        if !horizon_client::stations::is_damageable(self.res.items(), &stack) {
            return;
        }
        let loss = self.durability_loss(&stack, 1);
        if loss == 0 {
            return;
        }
        let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) else {
            return;
        };
        let damage = stack.damage + loss;
        match slot {
            S::Body => m.extra.body_damage = damage as i32,
            S::Saddle => m.set_saddle(Some(ItemStack { damage, ..stack })),
            _ => finalize::set_slot_damage(m, slot as usize, damage),
        }
    }
    /// `LivingEntity.hurtArmor` -> `LivingEntity.doHurtEquipment`, from the hit
    /// `MobEvent::ArmorHurt` reported. Only `Horse` overrides `hurtArmor` for a
    /// mob, and its override asks for the BODY slot alone, so the body piece is
    /// the only one this wears: it loses `max(1, damage / 4)` durability,
    /// Unbreaking applied, and at its maximum the slot empties and the break
    /// event goes out (`LivingEntity.entityEventForEquipmentBreak`: 65 for the
    /// body — 49 to 52 are the four armour slots, which only a player's own
    /// pieces reach). A fire hit leaves a fire-resistant piece alone
    /// (`ItemStack.canBeHurtBy`).
    fn hurt_mob_armor(&mut self, id: u32, damage: f32, kind: horizon_client::mobs::DamageKind) {
        use horizon_world::player_inventory::EquipmentSlot;
        if damage <= 0.0 {
            return;
        }
        let Some(m) = self.mobs.iter().find(|m| m.id == id) else {
            return;
        };
        let at = [m.position.x, m.position.y, m.position.z];
        let amount = ((damage / 4.0) as u32).max(1);
        let name = damage_type(kind, false);
        let fire = self
            .health_resources
            .damage
            .id(name)
            .is_some_and(|d| self.health_resources.damage.in_tag(d, "minecraft:is_fire"));
        // A piece's maximum, or none when it may not lose durability at all.
        // `doHurtEquipment` starts from the item's `equippable` component
        // (`Equippable equippable = item.get(DataComponents.EQUIPPABLE)`), so a
        // damageable item with no such component — a sword put in a horse's body
        // slot by `/item replace` — never wears; `equip_damage_on_hurt` alone
        // cannot say that, since it defaults to true when the component is absent.
        fn wearable(me: &Game<'_>, stack: &ItemStack, fire: bool, name: &str) -> Option<u32> {
            let info = me.res.items().get(&stack.item)?;
            if info.equip_slot.is_none()
                || !info.equip_damage_on_hurt
                || info.max_damage == 0
                || !horizon_client::stations::is_damageable(me.res.items(), stack)
                || (fire && me.res.items().resists_fire(stack, name))
            {
                return None;
            }
            Some(info.max_damage)
        }
        let Some(body) = self
            .mobs
            .iter()
            .find(|m| m.id == id)
            .and_then(|m| m.item_by_slot(EquipmentSlot::Body))
        else {
            return;
        };
        let Some(max) = wearable(self, &body, fire, name) else {
            return;
        };
        let loss = self.durability_loss(&body, amount);
        if loss == 0 {
            return;
        }
        let lost = body.damage + loss;
        let broke = {
            let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) else {
                return;
            };
            if lost >= max {
                m.extra.body = None;
                m.extra.body_damage = 0;
                true
            } else {
                m.extra.body_damage = lost as i32;
                false
            }
        };
        if broke {
            // LivingEntity.onEquippedItemBroken through the level's broadcast.
            let event =
                horizon_client::mobs::item_break::equipment_break_event(EquipmentSlot::Body);
            self.lan_entity_event(id as i32, event, at);
        }
    }
    /// What a converted frog gets once its tadpole has grown up: the spawn
    /// finalization of a conversion, then persistence.
    fn finalize_converted(&mut self, id: u32) {
        let Some(at) = self.mobs.iter().find(|m| m.id == id).map(|m| m.position) else {
            return;
        };
        let facts = self.biome_facts(
            at.x.floor() as i32,
            at.y.floor() as i32,
            at.z.floor() as i32,
        );
        let difficulty = self.local_difficulty([at.x, at.y, at.z]);
        if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
            spawning::finalize(m, &facts, &difficulty);
            m.persistent = true;
        }
    }
    /// `Mob.convertTo(type, ConversionParams.single(...))` for a mob already
    /// removed: a new mob of `kind`, made without spawn finalization, takes
    /// its place with what `conversion::single` carries over, and the old
    /// mob's riders move onto it. A zombie that comes out of it rolls
    /// `Zombie.handleAttributes(special, CONVERSION)` (the
    /// `postConversionStep` of its `ConversionTracker`).
    pub(super) fn convert_to(
        &mut self,
        from: &Mob,
        kind: &str,
        params: horizon_client::mobs::conversion::Params,
    ) -> Option<u32> {
        let t = self.res.mob_types().get(kind).cloned()?;
        // LivingConversionEvent.Pre: the caller already marked the mob removed,
        // so a canceled conversion puts it back as it was.
        let mut cancel = false;
        let from_kind = super::native_hooks::mob_kind(from);
        self.native_living_conversion_pre(from.id, &from_kind, kind, &mut cancel);
        if cancel {
            match self.mobs.iter_mut().find(|m| m.id == from.id) {
                Some(kept) => kept.removed = false,
                None => {
                    let mut kept = from.clone();
                    kept.removed = false;
                    self.mobs.push(kept);
                }
            }
            return None;
        }
        self.next_entity += 1;
        let mut mob = Mob::new(self.next_entity, t, from.position, from.yaw);
        horizon_client::mobs::conversion::single(from, &mut mob, params);
        if matches!(from.species(), Species::Zombie | Species::Husk) {
            let at = [from.position.x, from.position.y, from.position.z];
            let special = self.local_difficulty(at).special();
            horizon_client::mobs::finalize::zombie_attributes(
                &mut mob,
                special,
                horizon_client::mobs::finalize::Reason::Conversion,
            );
        }
        let id = mob.id;
        // ConversionType.SINGLE: the passengers get off and board the new mob.
        for rider in self.mobs.iter_mut() {
            if rider.extra.mount == Some(from.id) {
                rider.extra.mount = Some(id);
            }
        }
        if let Some(raw) = horizon_client::entity_nbt::conversion_tags(self.persist_raw(from.id)) {
            self.persist.raw.insert(id, raw);
        }
        self.mobs.push(mob);
        self.native_mobs_joined(self.mobs.len() - 1, false);
        let transformed_kind = self
            .mobs
            .last()
            .expect("converted mob was just inserted")
            .kind
            .id
            .clone();
        self.native_entity_transform(from.id, &from.kind.id, id, &transformed_kind);
        self.native_living_conversion_post(from.id, &from_kind, id, &transformed_kind);
        Some(id)
    }
    /// Default position for a summon command (3 blocks in front of the player).
    pub(super) fn default_summon_position(&self) -> Result<[f64; 3]> {
        let p = self.player.as_ref().ok_or("no player yet")?;
        let d = view_vector(self.yaw, 0.0);
        Ok([
            p.position.x + d[0] * 3.0,
            p.position.y,
            p.position.z + d[2] * 3.0,
        ])
    }
    /// The `summon` command: spawn a mob in front of the player or at a position.
    pub(super) fn summon(&mut self, kind: &str, at: Option<[f64; 3]>, baby: bool) -> Result<u32> {
        let at = match at {
            Some(a) => a,
            None => self.default_summon_position()?,
        };
        let id = self
            .spawn_mob_spawned(kind, at, SpawnYaw::Created)
            .ok_or_else(|| format!("unknown or unsupported mob {kind}"))?;
        if baby && let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
            m.extra.age = if m.species() == Species::Sniffer {
                horizon_client::mobs::sniffer::BABY_START_AGE
            } else {
                -24000
            };
        }
        self.spawn_jockeys(id, false);
        Ok(id)
    }
    /// The `mobs` command: nearby mobs with position, health and running goals.
    pub(super) fn control_mobs(&self, radius: f64) -> serde_json::Value {
        let center = self.player.as_ref().map_or(Vec3::ZERO, |p| p.position);
        self.control_mobs_at(radius, center)
    }
    pub(super) fn control_mobs_at(&self, radius: f64, center: Vec3) -> serde_json::Value {
        let list: Vec<serde_json::Value> = self
            .mobs
            .iter()
            .filter(|m| m.distance_sqr(center) <= radius * radius)
            .map(|m| {
                let mut state = serde_json::json!({
                    "id": m.id,
                    "kind": m.kind.id,
                    "pos": [m.position.x, m.position.y, m.position.z],
                    "health": m.health,
                    "baby": m.is_baby(),
                    "target": m.ai.target.map(|t| format!("{t:?}")),
                    "goals": m.ai.goals.running_names(),
                    "held": m.extra.main_hand.as_deref(),
                    "mount": m.extra.mount,
                    "rider": m.extra.rider,
                    // A player's input this tick, and whether it steers.
                    "ridden": m.rider.map(|r| r.steer),
                    "controlled": horizon_client::mobs::equine::controlled(m),
                    "saddled": m.extra.saddled,
                    "enchants": horizon_client::mobs::gear::gear(m).map(|g| g.slots.iter().map(|s| s.iter().map(|(e, l)| format!("{e}:{l}")).collect::<Vec<_>>()).collect::<Vec<_>>()),
                    "ints": m.extra.ints,
                    "pois": [m.extra.home, m.extra.bed, m.extra.meeting],
                    "profession": m.extra.variant2,
                    "targets": m.ai.targets.running_names(),
                    "path": m.ai.nav.path.as_ref().map(|p| p.nodes.len() - p.next.min(p.nodes.len())),
                    "on_ground": m.on_ground,
                    "variant": m.extra.variant,
                    "color": m.extra.color,
                    "sheared": m.extra.sheared,
                    "burning": m.fire_ticks > 0,
                    "size": m.extra.size,
                    "leash": m.extra.leash.map(|h| format!("{h:?}")),
                    "tame": m.extra.tame,
                    "sitting": m.extra.sitting,
                    "temper": if horizon_client::mobs::equine::is_equine(m.species()) {
                        horizon_client::mobs::equine::temper(m)
                    } else { i32::from(m.extra.variant3) },
                    "chested": m.extra.chested,
                });
                // Set apart: one json! with every key goes past the default macro recursion
                // limit, and horizon-client can't raise it (#8169).
                state["head"] = m.extra.head.as_deref().into();
                state["body"] = m.extra.body.as_deref().into();
                state["warden"] = serde_json::json!(horizon_client::mobs::warden::describe(m));
                state["villager"] = serde_json::json!(horizon_client::mobs::villager::ext(m).map(|e| serde_json::json!({
                    "food_level": e.food_level,
                    "inventory": e.inventory.iter().map(|s| format!("{}x{}", s.count, s.item)).collect::<Vec<_>>(),
                    "gossips": e.gossips.len(),
                    "last_slept": e.last_slept,
                    "raid": e.raid,
                })));
                state["raider"] = serde_json::json!(horizon_client::mobs::raider::state(m).map(|s| serde_json::json!({
                    "raid": s.raid_id,
                    "wave": s.wave,
                    "leader": s.patrol_leader,
                    "patrolling": s.patrolling,
                    "patrol_target": s.patrol_target,
                    "vehicle": m.extra.mount,
                })));
                // Drawn seated (Entity.isPassenger).
                state["passenger"] = m.extra.passenger.into();
                state["guardian_beam"] = serde_json::json!(horizon_client::mobs::guardian::beam(m, 0.0));
                state
            })
            .collect();
        serde_json::json!({ "count": self.mobs.len(), "near": list, "projectiles": self.creatures.projectiles.len() })
    }
    /// Highest motion-blocking (no leaves) block + 1 (Heightmap MOTION_BLOCKING_NO_LEAVES).
    pub(super) fn motion_top(&self, x: i32, z: i32) -> i32 {
        let top = if self.dim == Dim::Nether { 127 } else { 319 };
        for y in (self.world.bottom_y()..=top).rev() {
            let f = self.res.facts().get(self.world.block(x, y, z));
            if (f.full_collision || f.fluid != Fluid::None) && !f.name.ends_with("leaves") {
                return y + 1;
            }
        }
        self.world.bottom_y()
    }
    pub(super) fn mob_world_base<'s>(&'s self, others: &'s [Seen]) -> MobWorld<'s> {
        let (rain, thunder) = self.weather_levels(1.0);
        let weather = self.can_have_weather();
        let player = self.player_view();
        MobWorld {
            world: &self.world,
            registry: self.pack.assets.registry,
            facts: self.res.facts(),
            fluid_tables: &self.motion.fluids,
            movement: &self.motion.blocks,
            collision: self.collision,
            suffocation: self.mob_suffocation(),
            data: self.res.mob_data(),
            controlling_player_movement: None,
            nearest_player: player.as_ref().filter(|p| !p.spectator).map(|p| p.position),
            player,
            others,
            dragon: self.dragon_target(),
            leash_boats: &self.leash_boats,
            others_grid: None,
            others_index: None,
            others_villagers: None,
            others_solid: None,
            daylight: self.dim != Dim::Nether && self.sky_darken() < 4,
            // day.json: false from tick 12542, true from 23460 (a step track).
            monsters_burn: self.dimension_type().has_timeline("minecraft:day")
                && !(12542..23460).contains(&self.world.day_time.rem_euclid(24000)),
            game_time: self.world.game_time,
            day_time: self.world.day_time,
            raining: weather && rain > 0.2,
            thundering: weather && thunder > 0.9,
            level: self.level_key(),
            block_tags: &self.items().block_tags,
            landing: &self.motion.landing,
            biomes: world_biomes!(self),
            mob_griefing: self.ui.rules.bool("mob_griefing"),
            universal_anger: self.ui.rules.bool("universal_anger"),
            mob_drops: self.ui.rules.bool("mob_drops"),
            max_entity_cramming: self.ui.rules.int("max_entity_cramming"),
            dimension_type: self.dimension_type(),
            world_seed: self.level.seed,
            sea_level: if self.dim == Dim::Nether { 32 } else { 63 },
            teams: std::sync::Arc::new(self.mob_teams()),
            items: Some(self.items()),
            lid_openers: &self.lid_openers,
            mod_effects: if self.remote.is_none() {
                Some(self)
            } else {
                None
            },
            // A warden's away suspects are UUIDs: on the server side, where the
            // entity UUIDs live, they resolve back to the mobs that are here.
            who_lookup: if self.remote.is_none() {
                Some(self)
            } else {
                None
            },
            border: Some(self.border.geometry()),
            // The first player in the level: a server has no player of its own.
            difficulty: self.local_difficulty(
                self.level_players_iter()
                    .next()
                    .map_or([0.0; 3], |p| [p.position.x, p.position.y, p.position.z]),
            ),
        }
    }
    /// What the generator finalizes this level's structure mobs with.
    pub(super) fn share_spawn_conditions(&self) {
        if let Some(terrain) = self.end.locator() {
            terrain.set_spawn_conditions(self.difficulty_id() as u8, self.world.day_time);
        }
    }
    /// `ServerLevel.getCurrentDifficultyAt`.
    pub(super) fn local_difficulty(&self, at: [f64; 3]) -> spawning::LocalDifficulty {
        use horizon_world::food::Difficulty;
        let base = match self.world.health_rules.difficulty {
            Difficulty::Peaceful => 0,
            Difficulty::Easy => 1,
            Difficulty::Normal => 2,
            Difficulty::Hard => 3,
        };
        let key = ((at[0].floor() as i32) >> 4, (at[2].floor() as i32) >> 4);
        let (inhabited, moon) = if self.world.chunks.contains_key(&key) {
            let phase = self.world.day_time.div_euclid(24000).rem_euclid(8) as usize;
            (
                self.world.inhabited.get(&key).copied().unwrap_or(0),
                spawning::MOON_BRIGHTNESS[phase],
            )
        } else {
            (0, 0.0)
        };
        spawning::LocalDifficulty::new(base, self.world.day_time, inhabited, moon)
    }
    /// Generation's animals for the chunks generated since, once lit: chunk
    /// generation places them whether the level ticks or not.
    pub(super) fn spawn_fresh_chunks(&mut self) {
        if !self.world.chunks.has_fresh() {
            return;
        }
        let fresh: Vec<(i32, i32)> = self
            .world
            .chunks
            .fresh_keys_ordered()
            .into_iter()
            .filter(|k| self.world.chunks.get(k).is_some_and(|c| c.fresh && c.lit))
            .collect();
        for key in fresh {
            if let Some(c) = self.world.chunks.get_mut(&key) {
                Arc::make_mut(c).fresh = false;
            }
            self.world.chunks.remove_fresh(&key);
            self.spawn_generation_creatures(key.0, key.1);
        }
    }
    /// Advance every mob one tick, then apply their events.
    pub(super) fn tick_creatures(&mut self) -> Result<()> {
        // Hurts dealt since the last creature tick (explosions, commands, the
        // systems after it) teleport an enderman before it ticks, not in its AI step.
        self.settle_hurt_teleports()?;
        self.attack_ticks = self.attack_ticks.saturating_add(1);
        self.refresh_gear_knockback_resistance();
        self.tick_leads();
        // Vanilla's spawning and raids run before ChunkMap.tick.
        let from = self.lan_synced_from();
        horizon_client::spans::time("spawn_natural", || self.spawn_natural());
        // ServerChunkCache.tickChunks: the custom spawners tick only with spawn_mobs.
        if self.ui.rules.bool("spawn_mobs") {
            self.tick_wandering_trader();
            self.tick_patrol_spawner();
            self.tick_cat_spawner();
            self.tick_village_siege();
        }
        self.tick_raids();
        self.lan_synced_since(from);
        let ticked =
            horizon_client::spans::time("tick_creature_entities", || self.tick_creature_entities());
        self.dismount_underwater_mob_riders();
        ticked
    }
    /// `DistanceManager.inEntityTickingRange` in an entity-ticking chunk
    /// (block coordinates), from every player's tickets (`update_chunk_tickets`).
    pub(super) fn entity_ticking_area(&self) -> impl Fn(i32, i32) -> bool + use<> {
        let levels = self.tickets.levels();
        move |x, z| levels.entity_ticking_at(x, z)
    }
    /// The entity phase of `tick_creatures`: every mob's tick, deaths, and the
    /// projectiles and effects that follow from them.
    pub(super) fn tick_creature_entities(&mut self) -> Result<()> {
        // Its steps as spans for `horizon-report` (nothing is read while spans are off).
        let mut laps = horizon_client::spans::laps();
        // LivingEntity.die runs when the killing blow lands: mobs killed
        // since the last tick (by the player, explosions...) die first.
        let killed: Vec<Mob> = self
            .mobs
            .iter()
            .filter(|m| !m.alive() && !m.removed && !self.died.contains(&m.id))
            .cloned()
            .collect();
        for m in killed {
            self.died.insert(m.id);
            self.mob_died(&m);
        }
        // Their victims have been looted and mourned: no charged creeper is
        // waiting any more.
        self.combat.charged_exploders.clear();
        self.combat.exploded.clear();
        self.combat.exploded_kinds.clear();
        // EntityTickEvent.Pre for the mobs about to tick, before the loop holds them.
        let mut round = self.native_tick_round();
        if round.listening() {
            let ticking = self.entity_ticking_area();
            let mut i = 0;
            while let Some(m) = self.mobs.get(i) {
                let (x, z) = (m.position.x.floor() as i32, m.position.z.floor() as i32);
                if self.world.loaded(x, z) && ticking(x, z) {
                    let (id, kind) = (m.id, super::native_ticks::TickKind::Mob(m.kind.clone()));
                    self.native_tick_pre_batch(&mut round, id, kind);
                }
                i += 1;
            }
        }
        self.init_sulfur_attribute_maps();
        let uuid = self.level.player_uuid;
        let level_id = self.dim.id();
        // LivingEntity.isPushable reads onClimbable live; a mob's cached one
        // is only refreshed by its own push step, so one that has not ticked
        // yet (the first tick after a load, a fresh spawn) or has moved since
        // would be pushed out of a ladder or scaffolding. Refreshed here, from
        // where each mob stands now, before the snapshot others push it by.
        let climbing: Vec<bool> = {
            let w = self.mob_world_base(&[]);
            self.mobs.iter().map(|m| m.on_climbable(&w)).collect()
        };
        for (m, climbing) in self.mobs.iter_mut().zip(climbing) {
            m.on_climbable = climbing;
        }
        let mut snapshot = std::mem::take(&mut self.creatures.seen_buf);
        snapshot.extend(self.mobs.iter().map(|m| seen(m, uuid)));
        let mut events = std::mem::take(&mut self.creatures.event_buf);
        events.clear();
        // SulfurCube.primeWhenOnPoweredPosition runs before super.tick adopts
        // the body: the cubes whose adopted body explodes, as this tick starts.
        let primable = self.sulfur_cubes_adopted_explosive();
        let mut mobs = std::mem::take(&mut self.mobs);
        // Each mob's target as the mods last heard it, by its place in `mobs`
        // (`native_living_change_target`).
        let mut targets = std::mem::take(&mut self.creatures.target_buf);
        targets.clear();
        targets.extend(mobs.iter().map(|m| m.ai.target));
        let mut dead = Vec::new();
        let mut corpses = Vec::new();
        let mut died = std::mem::take(&mut self.died);
        let mut ticked = std::mem::take(&mut self.creatures.ticked);
        ticked.clear();
        let ticking = self.entity_ticking_area();
        // With guests on, each mob sees its nearest player (game/lan_mobs.rs).
        let mut guests = self.lan_guest_views();
        // What stands in a guest's slot while a mob holds that guest's view for
        // its tick (see `lent` below).
        let mut spare = guests.first().map(|g| g.1.clone());
        let players = self.level_players();
        let nearest = super::natural_spawning::NearestPlayers::new(&players);
        let mut bindings = match self.lan.as_mut() {
            Some(host) if !guests.is_empty() => std::mem::take(&mut host.mob_players),
            _ => Default::default(),
        };
        // The guests' entity ids by connection, for the shulker bullets fired
        // at the guest a mob sees.
        let guest_ids: Vec<(u32, u32)> = match self.lan.as_ref() {
            Some(host) if !guests.is_empty() => host
                .hub
                .guests
                .iter()
                .map(|g| (g.conn, g.entity_id as u32))
                .collect(),
            _ => Vec::new(),
        };
        let mut guest_events = Vec::new();
        // The block writes since the last tick: mobs re-path around them.
        let path_writes = std::mem::take(self.world.path_writes.get_or_insert_with(Vec::new));
        // The mobs whose bodies go this tick: (id, feet, width, height, split).
        // 🤖 Agent: the last field is the grown slime, magma cube or sulfur
        // cube the body leaves behind (`AbstractCubeMob.remove`), captured as
        // the body goes because it has left the list by the time it splits.
        let mut gone = Vec::new();
        let level_random;
        {
            // The level random, lent to the mobs (brain behaviours draw from
            // it) and taken back after their ticks.
            let mut scratch = self.world.random.clone();
            let mut pushes = horizon_client::mobs::PendingPushes::default();
            let mut keep = Vec::with_capacity(mobs.len());
            let mut grid =
                horizon_client::mobs::seen_grid::SeenGrid::new(snapshot.iter().map(|s| s.bounds));
            // All but the snapshot is the same for every mob of the tick, so
            // build it once: the player view and the teams scan every UUID.
            self.leash_boats = self
                .boats
                .iter()
                .map(super::boat_leads::boat_state)
                .collect();
            let mut base = self.mob_world_base(&[]);
            let path_changes = horizon_client::mobs::collision_changes(&base, &path_writes);
            // Each mob keeps its snapshot slot all tick; the first slot of an
            // id answers, as a scan would.
            let mut index = horizon_client::fast_hash::FxHashMap::<u32, u32>::default();
            let mut villager_slots = Vec::new();
            let mut solid_slots = Vec::new();
            for (i, s) in snapshot.iter().enumerate() {
                index.entry(s.id).or_insert(i as u32);
                if s.species == horizon_client::mobs::Species::Villager {
                    villager_slots.push(i as u32);
                }
                if s.may_block_mobs() {
                    solid_slots.push(i as u32);
                }
            }
            laps.lap("creature_prepare");
            let mut cached_difficulty: Option<((i32, i32), spawning::LocalDifficulty)> = None;
            for i in 0..mobs.len() {
                let m = &mut mobs[i];
                let (bx, bz) = (m.position.x.floor() as i32, m.position.z.floor() as i32);
                let seen_guest = if guests.is_empty() {
                    None
                } else {
                    let bound = bindings.get(&m.id).copied();
                    let conn = super::lan_mobs::player_for(m, base.player.as_ref(), &guests, bound);
                    match conn {
                        Some(conn) => {
                            bindings.insert(m.id, conn);
                        }
                        None => {
                            bindings.remove(&m.id);
                        }
                    }
                    conn
                };
                if !base.world.loaded(bx, bz) {
                    keep.push(!m.hostile() || m.persistent);
                    continue;
                }
                let nearest_player = nearest
                    .nearest(m.position.x, m.position.y, m.position.z)
                    .map(|p| p.position);
                // ServerLevel.tick: every loaded entity checks despawning, but
                // only those in entity-ticking chunks (within the simulation
                // distance) tick. The despawn check reads the nearest player
                // and the difficulty, not the player the mob sees.
                if !ticking(bx, bz) {
                    let w = MobWorld {
                        others: &snapshot,
                        others_grid: Some(&grid),
                        others_index: Some(&index),
                        others_villagers: Some(&villager_slots),
                        others_solid: Some(&solid_slots),
                        nearest_player,
                        player: None,
                        teams: std::sync::Arc::clone(&base.teams),
                        ..base
                    };
                    let kept = horizon_client::mobs::ai::despawn_check(m, &w);
                    if !kept {
                        died.remove(&m.id);
                        snapshot[i].alive = false;
                    }
                    keep.push(kept);
                    continue;
                }
                let controlling_player_movement = horizon_client::mobs::equine::controlled(m)
                    .then(|| {
                        base.player
                            .as_ref()
                            .filter(|p| p.riding_vehicle == Some(m.id))
                            .map(|p| p.velocity)
                            .or_else(|| {
                                guests
                                    .iter()
                                    .find(|(_, p)| p.riding_vehicle == Some(m.id))
                                    .map(|(_, p)| p.velocity)
                            })
                    })
                    .flatten();
                // A mod canceled its tick: it stays as it is.
                if round.canceled(m.id) {
                    keep.push(true);
                    continue;
                }
                // getCurrentDifficultyAt(blockPosition()): the mob's own chunk,
                // not the local player's (a server thread has no player).
                let chunk_key = (bx >> 4, bz >> 4);
                let difficulty = match cached_difficulty {
                    Some((key, difficulty)) if key == chunk_key => difficulty,
                    _ => {
                        let difficulty =
                            self.local_difficulty([m.position.x, m.position.y, m.position.z]);
                        cached_difficulty = Some((chunk_key, difficulty));
                        difficulty
                    }
                };
                horizon_client::mobs::apply_mob_pushes(m, &mut pushes);
                horizon_client::mobs::blocks_changed(m, &path_changes);
                let from = events.len();
                // The guest's view is lent to the mob for its tick, not copied:
                // the tick only reads it (`&MobWorld`), and it goes back to the
                // guest's slot straight after.
                let lent = seen_guest.and_then(|conn| guests.iter().position(|g| g.0 == conn));
                let active_player = match (seen_guest, lent) {
                    (None, _) => base.player.take(),
                    (Some(_), Some(k)) => spare
                        .take()
                        .map(|stand_in| std::mem::replace(&mut guests[k].1, stand_in)),
                    (Some(_), None) => None,
                };
                let mut w = MobWorld {
                    others: &snapshot,
                    others_grid: Some(&grid),
                    others_index: Some(&index),
                    others_villagers: Some(&villager_slots),
                    others_solid: Some(&solid_slots),
                    difficulty,
                    nearest_player,
                    player: active_player,
                    teams: std::sync::Arc::clone(&base.teams),
                    ..base
                };
                w.controlling_player_movement = controlling_player_movement;
                ticked.insert(mobs[i].id);
                // `horizon-report`'s cost by mob type (tick_profile.rs).
                let sampled = super::tick_profile::start().map(|t| (t, mobs[i].kind.clone()));
                let kept =
                    horizon_client::mobs::riders::tick(&mut mobs, i, &w, &mut scratch, &mut events);
                if let Some((started, kind)) = sampled {
                    super::tick_profile::add(level_id, &kind, started);
                }
                let m = &mut mobs[i];
                if seen_guest.is_none() {
                    base.player = w.player;
                } else if let Some(k) = lent {
                    spare = w.player.map(|v| std::mem::replace(&mut guests[k].1, v));
                }
                // Its tick changed its target: the mods may keep the one it had
                // (`native_living_change_target`), and then what the tick did to
                // the new one is not dealt.
                if let Some(&before) = targets.get(i)
                    && m.ai.target != before
                {
                    let named = target_named(
                        m.ai.target,
                        seen_guest,
                        &guests,
                        &guest_ids,
                        base.player.as_ref(),
                    );
                    if let Some((player, mob)) = named {
                        let kind = super::native_hooks::mob_kind(m);
                        let mut cancel = false;
                        Self::native_living_change_target(
                            level_id,
                            m.id,
                            &kind,
                            player,
                            mob,
                            &mut cancel,
                        );
                        if cancel {
                            if let Some(new) = m.ai.target {
                                let mut k = from;
                                while k < events.len() {
                                    if hits_target(&events[k], new) {
                                        events.remove(k);
                                    } else {
                                        k += 1;
                                    }
                                }
                            }
                            m.ai.target = before;
                        }
                    }
                    targets[i] = m.ai.target;
                }
                // A killed mob's body is removed: its effects that act on
                // death run (`LivingEntity.remove` with KILLED).
                if !kept && !m.alive() && !m.removed && !m.extra.effects.is_empty() {
                    corpses.push(m.clone());
                }
                horizon_client::mobs::take_mob_pushes(&mut events, from, &mut pushes);
                // LivingEntity.tickDeath: after twenty ticks the body is removed.
                if !kept && !m.removed && !m.alive() && m.death_time >= 20 {
                    let (width, height) = (f64::from(m.width()), f64::from(m.height()));
                    // 🤖 Agent: `AbstractCubeMob.remove` splits a grown cube into
                    // its smaller halves when it is removed, twenty ticks after
                    // the death. The parent leaves the list before the split
                    // runs, so its size, position, kind and persistence are
                    // captured here, as they are at the removal.
                    let split = if matches!(
                        m.species(),
                        Species::Slime | Species::MagmaCube | Species::SulfurCube
                    ) && m.extra.size > 1
                    {
                        Some(m.clone())
                    } else {
                        None
                    };
                    gone.push((m.id, m.position, width, height, split));
                }
                if let Some(conn) = seen_guest {
                    // ShulkerBullet.finalTarget is the guest's entity: the
                    // bullet homes on it while the projectiles tick, where
                    // `Who::Player` is the host's own player.
                    if let Some(&(_, id)) = guest_ids.iter().find(|g| g.0 == conn) {
                        for e in &mut events[from..] {
                            if let MobEvent::Projectile(p) = e
                                && let ProjectileKind::ShulkerBullet { target, .. } = &mut p.kind
                                && *target == Some(Who::Player)
                            {
                                *target = Some(Who::Mob(id));
                            }
                        }
                    }
                    let mut k = from;
                    while k < events.len() {
                        if super::lan_mobs::acts_on_player(&events[k]) {
                            guest_events.push((conn, m.id, events.remove(k)));
                        } else {
                            k += 1;
                        }
                    }
                }
                // Killed during its own tick (drowning, burning, falling...).
                if !m.alive() && !m.removed && died.insert(m.id) {
                    dead.push(m.clone());
                }
                if !kept {
                    died.remove(&m.id);
                }
                keep.push(kept);
                // ServerLevel ticks entities one after another: the ones
                // after this mob see it as it is now.
                snapshot[i] = seen(m, uuid);
                snapshot[i].alive &= kept;
                grid.update(i, snapshot[i].bounds);
                horizon_client::mobs::apply_brain_events(&mut mobs, &mut events, from);
            }
            // Targets changed after the mob's own tick (another mob's alert):
            // heard now, and a cancel puts the old one back before its next tick.
            for (i, m) in mobs.iter_mut().enumerate() {
                let Some(&before) = targets.get(i) else {
                    break;
                };
                if m.ai.target == before {
                    continue;
                }
                let seen_guest = if guests.is_empty() {
                    None
                } else {
                    bindings.get(&m.id).copied()
                };
                let named = target_named(
                    m.ai.target,
                    seen_guest,
                    &guests,
                    &guest_ids,
                    base.player.as_ref(),
                );
                if let Some((player, mob)) = named {
                    let kind = super::native_hooks::mob_kind(m);
                    let mut cancel = false;
                    Self::native_living_change_target(
                        level_id,
                        m.id,
                        &kind,
                        player,
                        mob,
                        &mut cancel,
                    );
                    if cancel {
                        m.ai.target = before;
                    }
                }
            }
            laps.lap("creature_loop");
            let mut flags = keep.into_iter();
            mobs.retain(|_| flags.next().unwrap_or(true));
            for m in &mut mobs {
                horizon_client::mobs::apply_mob_pushes(m, &mut pushes);
            }
            level_random = scratch;
        }
        self.world.random = level_random;
        if let Some(host) = self.lan.as_mut()
            && !guests.is_empty()
        {
            // A set: a scan of every mob for each binding is quadratic, and a
            // server's loaded mobs run into the thousands.
            let live: horizon_client::fast_hash::FxHashSet<u32> =
                mobs.iter().map(|m| m.id).collect();
            bindings.retain(|id, _| live.contains(id));
            host.mob_players = bindings;
        }
        // The mobs added during the tick (into the emptied `self.mobs`) come
        // first, as `append` put them; with none, the ticked list goes back
        // as it is instead of being copied into a new one, every mob a tick.
        if self.mobs.is_empty() {
            self.mobs = mobs;
        } else {
            self.mobs.append(&mut mobs);
        }
        self.died = died;
        snapshot.clear();
        self.creatures.seen_buf = snapshot;
        self.creatures.ticked = ticked;
        targets.clear();
        self.creatures.target_buf = targets;
        self.native_tick_post_batch(round);
        // ... and entity event 60 goes out as it is: the poof.
        // 🤖 Agent: the split follows the poof, as `LivingEntity.tickDeath`
        // broadcasts event 60 before it calls `remove`.
        for (id, at, width, height, split) in gone {
            let at = [at.x, at.y, at.z];
            self.lan_entity_event(id as i32, 60, at);
            self.death_poof(at, width, height);
            if let Some(m) = split {
                self.split_dead_cube(&m);
            }
        }
        laps.lap("creature_retain");
        self.sync_riders();
        self.tick_skeleton_traps();
        self.tick_apparition();
        self.guardian_bubbles();
        self.villager_reputation_tick();
        self.villager_item_sense();
        self.villager_pickups();
        laps.lap("creature_villagers");
        for m in dead {
            self.mob_died(&m);
        }
        for mut m in corpses {
            let names: Vec<&'static str> = m
                .extra
                .effects
                .iter()
                .map(|e| e.0.trim_start_matches("minecraft:"))
                .collect();
            let bounds = m.bounds();
            self.effects_on_death(&names, bounds, false, Some(m.id), Some(&mut m.random));
        }
        let event_result = self.handle_mob_events_reusable(&mut events);
        self.creatures.event_buf = events;
        event_result?;
        self.lan_guest_mob_events(guest_events);
        laps.lap("creature_events");
        self.ordered_before(super::ordered_tick::Batch::Projectiles)?;
        self.tick_projectiles()?;
        self.tick_fangs();
        laps.lap("creature_projectiles");
        self.tick_allays();
        self.tick_pandas();
        self.tick_foxes();
        self.tick_dolphins();
        self.tick_mob_looting();
        self.tick_breeze_effects();
        self.tick_sniffer_effects();
        self.piglin_pickups();
        self.sulfur_cube_pickups();
        self.sulfur_cube_power_priming(&primable);
        laps.lap("creature_systems");
        // Hurt and death sounds of damage dealt outside the mob ticks.
        let mut queued = Vec::new();
        for m in &mut self.mobs {
            queued.append(&mut m.outbox);
        }
        self.handle_mob_events(queued)?;
        // The enderman teleports those hurts asked for (`Enderman.hurtServer`).
        self.settle_hurt_teleports()?;
        laps.lap("creature_outbox");
        Ok(())
    }
    /// `LivingEntity.makePoofParticles` (entity event 60) about a mob's box.
    pub(super) fn death_poof(&mut self, at: [f64; 3], width: f64, height: f64) {
        let options = horizon_client::particles::ParticleOptions::simple(
            horizon_client::particles::ParticleType::Poof,
        );
        for _ in 0..20 {
            let r = self.particles.level_random();
            let xa = r.next_gaussian() * 0.02;
            let ya = r.next_gaussian() * 0.02;
            let za = r.next_gaussian() * 0.02;
            let x = at[0] + width * (2.0 * r.next_double() - 1.0) - xa * 10.0;
            let y = at[1] + height * r.next_double() - ya * 10.0;
            let z = at[2] + width * (2.0 * r.next_double() - 1.0) - za * 10.0;
            self.particles.add(&options, [x, y, z], [xa, ya, za]);
        }
    }
    /// `LivingDeathEvent` for a mob whose health ran out: whether a native
    /// mod canceled the death. A canceled mob stays alive at 1 health.
    fn mob_death_canceled(&mut self, m: &Mob) -> bool {
        if horizon_client::mod_api::mods().is_empty() {
            return false;
        }
        let (damage_type, attacker) = match m.last_damage {
            Some((d, _)) => (source_damage_type(d), d.attacker),
            None => ("minecraft:generic", None),
        };
        let attacker = match attacker {
            Some(Who::Mob(id)) => id,
            Some(Who::Player) => {
                let conn = self.last_hurt_by_player.get(&m.id).copied().flatten();
                let guests = self.lan.as_ref().map(|host| host.hub.guests.as_slice());
                let guest = guests.and_then(|all| all.iter().find(|g| Some(g.conn) == conn));
                guest.map_or(0, |g| g.entity_id as u32)
            }
            None => 0,
        };
        let kind = super::native_hooks::mob_kind(m);
        let mut cancel = false;
        self.native_living_death(m.id, &kind, damage_type, attacker, &mut cancel);
        if cancel {
            self.died.remove(&m.id);
            if let Some(alive) = self.mobs.iter_mut().find(|o| o.id == m.id) {
                alive.health = 1.0;
                alive.death_time = 0;
            }
        }
        cancel
    }
    /// LivingEntity.die / dropAllDeathLoot.
    pub(super) fn mob_died(&mut self, m: &Mob) {
        if self.mob_death_canceled(m) {
            return;
        }
        // LivingEntity.die: a sleeper gets up, and its bed is free again.
        if let Some(bed) = horizon_client::mobs::villager::sleeping_pos(m) {
            self.wake_villager(m.id);
            // The body may be gone already.
            self.set_bed_occupied(bed, false);
        }
        let killer = self.last_hurt_by_player.remove(&m.id);
        // TamableAnimal.die, before the rest of LivingEntity.die.
        self.tell_owner_pet_died(m, killer);
        self.love_cause.remove(&m.id);
        let by_player = killer.is_some() || m.ai.last_hurt_by_player > 0;
        let mut dying = m.clone();
        // LivingEntity.die: the death is heard, and a sculk catalyst nearby
        // takes the experience (before the loot is dropped). It takes what
        // would drop, whoever killed the mob.
        self.sculk_mob_died(m);
        let mut catalyst_xp = 0;
        let consumed =
            self.catalyst_takes_experience([m.position.x, m.position.y, m.position.z], |_| {
                catalyst_xp = if horizon_client::mobs::drops::drops_experience(&dying) {
                    horizon_client::mobs::drops::base_experience(&mut dying).max(0) as u32
                } else {
                    0
                };
                catalyst_xp
            });
        // CatalystListener: with experience to take, the player who last hurt
        // the mob gets KILL_MOB_NEAR_SCULK_CATALYST.
        if consumed && catalyst_xp > 0 {
            let who = match killer {
                Some(Some(conn)) => Some(super::commands::Target::Guest(conn)),
                Some(None) if self.lan_acting.is_none() => Some(super::commands::Target::Player),
                _ => None,
            };
            if let Some(who) = who {
                self.for_player(who, |g| {
                    // CatalystListener: trigger(player, killedEntity, damageSource).
                    let view = g.mob_entity(m);
                    let blow = g.last_blow(m);
                    g.trigger_killed_as(
                        "minecraft:kill_mob_near_sculk_catalyst",
                        &view,
                        blow.as_ref(),
                    );
                });
            }
        }
        if by_player {
            // A guest's kill is theirs: their statistics, scores and
            // advancements (LivingEntity.die credits lastHurtByPlayer).
            let guest = killer.flatten().filter(|_| self.lan_acting.is_none());
            let credited = guest.is_some_and(|conn| {
                let Some(mut host) = self.lan.take() else {
                    return false;
                };
                let done = self
                    .as_guest(&mut host, conn, |g| g.credit_kill(m))
                    .is_some();
                self.lan = Some(host);
                done
            });
            if !credited {
                self.credit_kill(m);
            }
        }
        // The body keeps its scores until it is removed (`ServerLevel.
        // EntityCallbacks.onDestroyed` → `Scoreboard.entityRemoved`, run by
        // `score_destroyed_entities`): LivingEntity.die does not reset them.
        // Raider.die's hero and Villager.tellWitnessesThatIWasMurdered go to
        // the causing entity, not the credited player: a tame wolf's kill is
        // neither its owner's heroism nor a murder by them.
        let slain_by_player = by_player && !killed_by_mob(m);
        self.raider_died(m, slain_by_player, killer.flatten());
        self.villager_murdered(m, slain_by_player, killer.flatten());
        let pos = [m.position.x, m.position.y + 0.5, m.position.z];
        let killer_entity = self.killer_loot_entity(killer.flatten());
        // LAST_DAMAGE_PLAYER only while the player is still in the world and
        // the hurt memory lasts (LivingEntity.dropFromLootTable). Experience
        // and equipment use the timer alone (dropExperience / playerKilled):
        // a guest who has left still counts.
        let player = killer
            .filter(|_| m.ai.last_hurt_by_player > 0)
            .and_then(|conn| self.killer_uuid(conn));
        if let Some(player) = player.filter(|_| m.extra.leash.is_some()) {
            self.lead_killers.insert(m.id, (player, self.dim));
        }
        self.drop_death_loot(
            &mut dying,
            player,
            m.ai.last_hurt_by_player > 0,
            killer_entity,
            consumed,
            pos,
        );
        self.create_wither_rose(m);
        if m.species() == Species::CopperGolem {
            // CopperGolem.dropEquipment: the preserved equipment too.
            self.golem_drop_equipment(&mut dying);
        }
    }
    /// 🤖 Agent: `AbstractCubeMob.remove`'s split: a grown cube mob leaves its
    /// smaller halves behind when its body goes, twenty ticks after the death
    /// (`LivingEntity.tickDeath`), not when it dies.
    fn split_dead_cube(&mut self, m: &Mob) {
        if m.species() == Species::SulfurCube {
            self.split_sulfur_cube(m);
            return;
        }
        // 🤖 Agent: AbstractCubeMob.getSplitCount: two to four smaller slimes.
        let size = m.extra.size;
        if size <= 1 {
            return;
        }
        let mut n = 2 + self.world.random.next_int_bound(3).unwrap_or(0);
        let mut cancel = false;
        let mut count = n as u32;
        let kind = super::native_hooks::mob_kind(m);
        self.native_mob_split(m.id, &kind, size, size / 2, &mut count, &mut cancel);
        n = if cancel { 0 } else { count.min(64) as _ };
        for k in 0..n {
            let off = |kk: i32| (f64::from(kk % 2) - 0.5) * f64::from(size) / 4.0;
            let at = [
                m.position.x + off(k),
                m.position.y + 0.5,
                m.position.z + off(k / 2),
            ];
            if let Some(id) = self.spawn_mob_raw(&m.kind.id, at, m.persistent)
                && let Some(c) = self.mobs.iter_mut().find(|c| c.id == id)
            {
                horizon_client::mobs::cube::set_size(c, size / 2);
            }
        }
    }
    /// The killer's side of a mob's death: `ServerPlayer.awardKillScore`'s
    /// statistics and scores, and the `player_killed_entity` trigger.
    fn credit_kill(&mut self, m: &Mob) {
        // Player.killedEntity: the killed statistic goes to the causing
        // entity, so a kill by a mob (a tame wolf credited to its owner)
        // leaves it out; ServerPlayer.awardKillScore counts the rest.
        if !killed_by_mob(m) {
            self.award_stat("killed", &m.kind.id, 1);
        }
        self.award_custom_stat("mob_kills", 1);
        self.score_player_killed(super::commands::Target::Mob(m.id));
        let view = self.mob_entity(m);
        let blow = self.last_blow(m);
        self.trigger_killed(&view, blow.as_ref());
    }
    /// The damage source a mob died to (`die`'s), its direct entity included when the mob
    /// recorded one ([`Mob::last_direct`]).
    fn last_blow(&self, m: &Mob) -> Option<horizon_client::advancements::DamageView> {
        let (d, _) = m.last_damage?;
        let mut view = self.damage_view(d);
        if let Some(kind) = m.last_direct {
            view.direct = Some(horizon_client::advancements::EntityView {
                kind: kind.into(),
                position: d.from.unwrap_or_default(),
                ..Default::default()
            });
            view.is_direct = false;
        }
        Some(view)
    }
    /// A mob's damage source as advancement predicates see it: its type, the
    /// causing entity, and the projectile when one dealt it.
    pub(super) fn damage_view(&self, d: Damage) -> horizon_client::advancements::DamageView {
        let source = match d.attacker {
            // Its callers are KilledTrigger's: the source as their killing_blow sees it.
            Some(Who::Player) => Some(self.player_entity_for_any(&[
                "minecraft:player_killed_entity",
                "minecraft:kill_mob_near_sculk_catalyst",
            ])),
            Some(Who::Mob(id)) => self
                .mobs
                .iter()
                .find(|o| o.id == id)
                .map(|o| self.mob_entity(o)),
            None => None,
        };
        let projectile = match d.kind {
            DamageKind::Arrow => Some("minecraft:arrow"),
            DamageKind::Fireball => Some("minecraft:fireball"),
            DamageKind::WitherSkull => Some("minecraft:wither_skull"),
            DamageKind::Fireworks => Some("minecraft:firework_rocket"),
            _ => None,
        };
        let direct = match projectile {
            Some(kind) => Some(horizon_client::advancements::EntityView {
                kind: kind.into(),
                position: d.from.unwrap_or_default(),
                ..Default::default()
            }),
            None => source.clone(),
        };
        horizon_client::advancements::DamageView {
            kind: source_damage_type(d).to_owned(),
            // DamageSource.isDirect: the causing entity is the direct one.
            is_direct: projectile.is_none(),
            direct,
            source,
            ..Default::default()
        }
    }
    /// `dropAllDeathLoot` through `mobs::drops`: the entity loot table from the
    /// world's random sequence, equipment by drop chance, species extras and
    /// the experience orbs, spawned in order.
    /// The killing player as a loot entity: their worn and held items, read
    /// from the guest who struck the last blow when a server's player did
    /// (on the server thread that is every player, the window's too), so
    /// Looting on their weapon counts.
    fn killer_loot_entity(&mut self, guest: Option<u32>) -> horizon_world::loot::LootEntity {
        let own = |g: &Self| {
            let mut e = horizon_world::loot::LootEntity::new("minecraft:player");
            let slots = [
                ("mainhand", g.stack_in(g.selected)),
                ("offhand", g.stack_in(super::shield::OFFHAND_SLOT)),
                ("head", g.armor[0].as_ref()),
                ("chest", g.armor[1].as_ref()),
                ("legs", g.armor[2].as_ref()),
                ("feet", g.armor[3].as_ref()),
            ];
            for (slot, stack) in slots {
                if let Some(stack) = stack {
                    e.equipment.push((slot.into(), to_loot(stack)));
                }
            }
            e
        };
        if let Some(conn) = guest.filter(|_| self.lan_acting.is_none())
            && let Some(mut host) = self.lan.take()
        {
            let entity = self.as_guest(&mut host, conn, |g| own(g));
            self.lan = Some(host);
            if let Some(entity) = entity {
                return entity;
            }
        }
        own(self)
    }
    /// The UUID of the player `last_hurt_by_player` keeps: the guest on
    /// connection `conn` (None once they left), else the player in place
    /// (None on a server, which has none of its own).
    fn killer_uuid(&mut self, conn: Option<u32>) -> Option<u128> {
        match conn {
            // During that guest's own turn, the hub is out of `lan`.
            Some(conn) if self.lan_acting != Some(conn) => {
                Some(self.lan.as_ref()?.hub.guest(conn)?.uuid)
            }
            Some(_) => Some(self.native_acting_id()),
            None if self.lan_acting.is_none() && self.player.is_some() => {
                Some(self.native_acting_id())
            }
            None => None,
        }
    }
    fn drop_death_loot(
        &mut self,
        m: &mut Mob,
        player_uuid: Option<u128>,
        player_killed: bool,
        killer: horizon_world::loot::LootEntity,
        consumed: bool,
        pos: [f64; 3],
    ) {
        use horizon_client::mobs::{DamageKind, Who, drops};
        use horizon_world::loot::{LootDamage, LootEntity};
        let Some(loot) = horizon_client::structure_blocks::loot_data(
            &self.options.resources,
            &self.pack_files(),
        ) else {
            return;
        };
        let player = || killer.clone();
        let damage = m.last_damage.map(|(d, _)| {
            let source = match d.attacker {
                Some(Who::Player) => Some(player()),
                Some(Who::Mob(id)) => self
                    .mobs
                    .iter()
                    .find(|o| o.id == id)
                    .map(|o| drops::loot_entity(o, None))
                    // A charged creeper's blast kills after the creeper left the list.
                    .or_else(|| {
                        self.combat
                            .charged_exploders
                            .contains(&id)
                            .then(|| LootEntity {
                                powered: true,
                                ..LootEntity::new("minecraft:creeper")
                            })
                    }),
                None => None,
            };
            let direct = match d.kind {
                DamageKind::Arrow => Some(LootEntity {
                    kind: "minecraft:arrow".into(),
                    ..LootEntity::default()
                }),
                DamageKind::Fireball => Some(LootEntity {
                    kind: "minecraft:fireball".into(),
                    ..LootEntity::default()
                }),
                DamageKind::WitherSkull => Some(LootEntity {
                    kind: "minecraft:wither_skull".into(),
                    ..LootEntity::default()
                }),
                DamageKind::Fireworks => Some(LootEntity {
                    kind: "minecraft:firework_rocket".into(),
                    ..LootEntity::default()
                }),
                _ => source.clone(),
            };
            LootDamage {
                kind: source_damage_type(d).into(),
                source,
                direct,
            }
        });
        let vehicle = m
            .extra
            .mount
            .and_then(|id| self.mobs.iter().find(|o| o.id == id))
            .map(|o| LootEntity::new(&o.kind.id));
        let death = drops::Death {
            loot: &loot,
            damage,
            player: player_uuid.is_some().then(player),
            player_killed,
            luck: 0.0,
            vehicle,
            mob_drops: self.ui.rules.bool("mob_drops"),
            world_seed: self.level.seed,
        };
        // LootContextParams.ORIGIN of a death: the mob's position.
        let place = self.loot_place_at([m.position.x, m.position.y, m.position.z], None);
        let (items, skulls) = horizon_world::loot::with_native_loot_place(place, || {
            drops::die(m, &death, &mut self.rand_sequences, &mut self.world.random)
        });
        if skulls && let Some(Who::Mob(id)) = m.last_damage.and_then(|(d, _)| d.attacker) {
            // Creeper.droppedSkulls: one head per charged explosion.
            self.combat.charged_exploders.retain(|c| *c != id);
            if let Some(creeper) = self.mobs.iter_mut().find(|o| o.id == id) {
                creeper.extra.charged = false;
            }
        }
        let raider = horizon_client::mobs::raider::is_raider(m.species());
        // The item drops come before the carried block and the orbs, so the
        // mods get them all at once and they still spawn in that order.
        let mut stacks = Vec::new();
        let mut rest = Vec::new();
        for d in items {
            match d {
                drops::Drop::Item(stack) => stacks.push(
                    if raider && stack.item == horizon_client::mobs::raider::BANNER {
                        horizon_client::mobs::raider::ominous_banner()
                    } else {
                        from_loot(&stack)
                    },
                ),
                drops::Drop::Stack(stack) => stacks.push(stack),
                other => rest.push(other),
            }
        }
        let kind = super::native_hooks::mob_kind(m);
        let table = drops::death_loot_table(m, death.mob_drops).filter(|t| loot.has_table(t));
        let context = NativeLootDrops {
            table: table.as_deref().unwrap_or_default(),
            dimension: self.dim.id(),
            origin: [m.position.x, m.position.y, m.position.z],
            block: None,
            entity: Some((m.id, &kind)),
            player: player_uuid,
            tool: None,
            explosion: None,
            used: false,
        };
        self.native_loot_drops(&context, &mut stacks);
        stacks.retain(|stack| stack.count > 0 && &*stack.item != "minecraft:air");
        for item in stacks {
            // Every death drop is its own ItemEntity, and the drop entity's
            // own random (not the level's) gives it the constructor's
            // horizontal push (its motion otherwise stays [0, 0.2, 0] and
            // the drops land stacked at the mob's centre, without disturbing
            // the level random that the orbs' merge ids and block ticks read
            // next).
            self.spawn_item_own(item, pos, 10);
        }
        // LivingEntity.dropExperience: a mod may change the mob's experience
        // before the orbs are split and spawned (the orbs stay last).
        if rest.iter().any(|d| matches!(d, drops::Drop::Orb(_))) {
            let mut xp: i32 = rest
                .iter()
                .filter_map(|d| match d {
                    drops::Drop::Orb(value) => Some(*value),
                    _ => None,
                })
                .sum();
            let before = xp;
            self.native_living_experience_drop(m.id, &kind, &mut xp);
            if xp != before {
                rest.retain(|d| !matches!(d, drops::Drop::Orb(_)));
                while xp > 0 {
                    let value = drops::orb_value(xp);
                    xp -= value;
                    rest.push(drops::Drop::Orb(value));
                }
            }
        }
        for d in rest {
            match d {
                drops::Drop::CarriedBlock(state) => {
                    // Enderman.dropCustomDeathLoot: spawnAtLocation for each
                    // of the carried block's silk-touch drops.
                    let stacks = horizon_client::mobs::enderman::carried_block_drops(
                        world_items!(self),
                        self.pack.assets.registry,
                        state,
                        &mut self.world.random,
                        Some((&mut self.rand_sequences, self.level.seed)),
                    );
                    for item in stacks {
                        self.spawn_item_own(item, pos, 10);
                    }
                }
                drops::Drop::Orb(value) if !consumed => self.spawn_orbs(pos, value as u32),
                drops::Drop::Item(_) | drops::Drop::Stack(_) | drops::Drop::Orb(_) => {}
            }
        }
    }
    /// `Mob.doHurtTarget` and the other melee callers of post-attack effects.
    fn mob_attacks_mob(&mut self, target: u32, amount: f32, source: Damage) -> bool {
        let hurt = self.mob_hits_mob(target, amount, source);
        if hurt {
            self.mob_post_attack(
                target,
                source.attacker,
                None,
                Some(damage_type(source.kind, false)),
            );
        }
        hurt
    }
    /// Melee or ranged damage to another mob; whether the hit landed.
    pub(super) fn mob_hits_mob(&mut self, target: u32, amount: f32, mut source: Damage) -> bool {
        if source.attacker_species.is_none()
            && let Some(Who::Mob(id)) = source.attacker
        {
            source.attacker_species = self.mobs.iter().find(|m| m.id == id).map(|m| m.species());
        }
        let mut killed = false;
        let mut hurt = false;
        let mut infested = Vec::new();
        let aim = self.attacker_aim(source.attacker);
        // So `living_knockback_v2` sees who hit and what they hold.
        let _attacker = self.knockback_scope_for(source.attacker);
        if let Some(t) = self.mobs.iter_mut().find(|m| m.id == target) {
            let hit = t.damage_aimed(amount, source, aim);
            hurt = hit;
            if hit {
                killed = !t.alive();
                // The hurt or death sound the mob raised.
                let center = t.center();
                for event in std::mem::take(&mut t.outbox) {
                    match event {
                        MobEvent::Sound(s, _, volume, pitch) => {
                            self.audio
                                .play(&format!("minecraft:{s}"), Some(center), volume, pitch);
                        }
                        MobEvent::Infested { at, velocities } => infested.push((at, velocities)),
                        other => {
                            // Left for the end of the tick.
                            t.outbox.push(other);
                        }
                    }
                }
            }
        }
        if hurt {
            self.resolve_player_responsible(target, source);
        }
        for (at, velocities) in infested {
            self.spawn_infested(at, velocities);
        }
        if hurt
            && let Some(Who::Mob(attacker)) = source.attacker
            && let Some(m) = self
                .mobs
                .iter()
                .find(|m| m.id == attacker && m.species() == Species::Axolotl)
        {
            self.audio.play(
                "minecraft:entity.axolotl.attack",
                Some([m.position.x, m.position.y, m.position.z]),
                1.0,
                1.0,
            );
        }
        if killed && let Some(Who::Mob(killer)) = source.attacker {
            self.zombie_killed(killer, target);
        }
        hurt
    }
    /// `LivingEntity.resolvePlayerResponsibleForDamage` after `source` hurt a
    /// mob: a tame wolf's hit is its owner's (`setLastHurtByPlayer(uuid, 100)`),
    /// so the kill drops experience and player-only loot and the owner, when
    /// in the world, gets the credit (`getKillCredit`); a tame wolf without an
    /// owner clears it. A player's own hits are recorded where they land.
    pub(super) fn resolve_player_responsible(&mut self, victim: u32, source: Damage) {
        let Some(Who::Mob(attacker)) = source.attacker else {
            return;
        };
        let Some(owner) = self
            .mobs
            .iter()
            .find(|m| m.id == attacker && m.species() == Species::Wolf && m.extra.tame)
            .map(|m| m.extra.owner)
        else {
            return;
        };
        let player = owner.and_then(|uuid| self.player_with_uuid(uuid));
        let Some(m) = self.mobs.iter_mut().find(|m| m.id == victim) else {
            return;
        };
        m.ai.last_hurt_by_player = if owner.is_some() { 100 } else { 0 };
        match player {
            Some(who) => self.last_hurt_by_player.insert(victim, who),
            None => self.last_hurt_by_player.remove(&victim),
        };
    }
    /// The player in the world with `uuid` (`EntityReference.getPlayer`), as
    /// `last_hurt_by_player` keeps them: None for the player in place, else the
    /// guest's connection.
    fn player_with_uuid(&self, uuid: [i32; 4]) -> Option<Option<u32>> {
        if self.player.is_some()
            && horizon_client::mobs::tame::owned_by(Some(uuid), self.acting_uuid())
        {
            return Some(self.lan_acting);
        }
        let uuid = horizon_client::lan::playerdata::ints_uuid(uuid);
        self.lan
            .iter()
            .flat_map(|h| h.hub.guests.iter())
            .find(|g| g.placed && g.uuid == uuid)
            .map(|g| Some(g.conn))
    }
    /// `LivingEntity.dropFromGiftLootTable`: the gift table from its random
    /// sequence with the mob as `this` (a chicken's egg depends on its
    /// variant); the loot data of the game's resources decides, and without it
    /// the items' table engine (no entity) rolls it. Whether anything dropped.
    fn drop_gift_loot(&mut self, mob: u32, table: &'static str, at: [f64; 3]) -> bool {
        use horizon_client::mobs::drops;
        use horizon_world::loot::EntityLootParams;
        let loot = horizon_client::structure_blocks::loot_data(
            &self.options.resources,
            &self.pack_files(),
        );
        let this = self
            .mobs
            .iter()
            .find(|m| m.id == mob)
            .map(|m| drops::loot_entity(m, None));
        let stacks = match (loot, this) {
            (Some(loot), Some(this)) if loot.has_table(table) => {
                let params = EntityLootParams {
                    this,
                    ..EntityLootParams::default()
                };
                let seed = self.level.seed;
                match loot.random_sequence(table) {
                    Some(id) => {
                        let random = self.rand_sequences.get(id, seed);
                        loot.roll_entity(table, random, &params)
                            .iter()
                            .filter(|s| !s.is_empty())
                            .map(from_loot)
                            .collect()
                    }
                    None => Vec::new(),
                }
            }
            _ => self
                .res
                .items()
                .loot_table(table, false, None, &mut self.world.random),
        };
        let dropped = !stacks.is_empty();
        for d in stacks {
            // The ItemEntity constructor's motion, from the entity's own
            // random (not the level's).
            self.spawn_item_own(d, at, 10);
        }
        dropped
    }
    /// Adds a projectile, giving it an id from the entity counter
    /// (`Entity.id`, the same counter as the rest) when it has none.
    pub(super) fn add_projectile(&mut self, mut p: horizon_client::projectiles::Projectile) {
        if p.id == 0 {
            self.next_entity += 1;
            p.id = self.next_entity;
        }
        // Projectile.setOwner: the acting player's or the mob's UUID, which
        // its save keeps even once the owner is gone.
        if p.owner_uuid.is_none() {
            p.owner_uuid = match p.owner {
                Some(Who::Player) => Some(self.acting_player_uuid()),
                Some(Who::Mob(id)) => Some(self.uuid_for(id)),
                None => None,
            };
        }
        let player = (p.owner == Some(Who::Player))
            .then(|| horizon_client::lan::playerdata::ints_uuid(self.acting_player_uuid()));
        let name = if player.is_some() {
            self.acting_player_name().to_owned()
        } else {
            String::new()
        };
        let (id, kind) = (p.id, p.entity_type());
        if let Some(a) = p.arrow.as_mut()
            && !p.stuck
        {
            a.spawn_pos = Some(p.position);
            a.spawn_velocity = Some(p.velocity);
            a.spawn_rot = Some((p.pitch, p.yaw));
        }
        self.creatures.projectiles.push(p);
        self.native_projectile_launch(player, &name, id, kind);
    }
    /// `MushroomCow.shear`'s `sendParticles(EXPLOSION, x, getY(0.5), z, 1, 0, 0, 0, 0)`:
    /// one puff over a sheared mooshroom, half its `height` up, shown here and to
    /// the players within range.
    pub(super) fn mooshroom_shear_puff(&mut self, at: [f64; 3], height: f64) {
        let at = [at[0], at[1] + 0.5 * height, at[2]];
        let options = ParticleOptions::simple(ParticleType::Explosion);
        self.particles
            .send_particles(&options, at, [0.0; 3], 0.0, 1);
        self.lan_level_particles(None, &options, at, [0.0; 3], 0.0, 1, None);
    }
    /// `Entity.sendBubbleColumnParticles` at the top of the column block `pos`: two
    /// splashes and two bubbles over it, which the level random places, shown here
    /// and to the players within range.
    pub(super) fn bubble_column_particles(&mut self, pos: BlockPos) {
        for _ in 0..2 {
            let r = &mut self.world.random;
            let splash = [
                f64::from(pos.x) + r.next_double(),
                f64::from(pos.y + 1),
                f64::from(pos.z) + r.next_double(),
            ];
            let bubble = [
                f64::from(pos.x) + r.next_double(),
                f64::from(pos.y + 1),
                f64::from(pos.z) + r.next_double(),
            ];
            let splash_options = ParticleOptions::simple(ParticleType::Splash);
            self.particles
                .send_particles(&splash_options, splash, [0.0; 3], 1.0, 1);
            self.lan_level_particles(None, &splash_options, splash, [0.0; 3], 1.0, 1, None);
            let bubble_options = ParticleOptions::simple(ParticleType::Bubble);
            self.particles
                .send_particles(&bubble_options, bubble, [0.0, 0.01, 0.0], 0.2, 1);
            self.lan_level_particles(
                None,
                &bubble_options,
                bubble,
                [0.0, 0.01, 0.0],
                0.2,
                1,
                None,
            );
        }
    }
    /// A mob's projectile enters the level.
    fn launch_mob_projectile(&mut self, mut p: Projectile) {
        self.next_entity += 1;
        p.id = self.next_entity;
        let (owner, at) = (p.owner, [p.position.x, p.position.y, p.position.z]);
        // LlamaSpit.recreateFromPacket: the spit's burst on the client.
        if p.kind == ProjectileKind::LlamaSpit {
            let v = p.velocity;
            self.particles.llama_spit_burst(at, [v.x, v.y, v.z]);
        }
        // Ordinary arrows post from ArrowState's first tick, including loaded arrows.
        // Keep the existing immediate post for the other arrow-state kind (tridents).
        let immediate = p.arrow.is_some() && !p.is_arrow();
        self.add_projectile(p);
        if immediate {
            self.projectile_event("minecraft:projectile_shoot", owner, at);
        }
    }
    pub(super) fn handle_mob_events(&mut self, mut events: Vec<MobEvent>) -> Result<()> {
        self.handle_mob_events_reusable(&mut events)
    }

    pub(super) fn handle_mob_events_reusable(&mut self, events: &mut Vec<MobEvent>) -> Result<()> {
        for e in events.drain(..) {
            match e {
                MobEvent::BubbleColumnParticles { pos } => {
                    self.bubble_column_particles(BlockPos::new(pos[0], pos[1], pos[2]))
                }
                MobEvent::LandingDust {
                    state,
                    at,
                    count,
                    speed,
                } => self.landing_dust(None, state, Vec3::new(at[0], at[1], at[2]), count, speed),
                MobEvent::SquidCycle { mob, at } => self.lan_entity_event(mob as i32, 19, at),
                MobEvent::RabbitHop { mob, at } => self.lan_entity_event(mob as i32, 1, at),
                MobEvent::Drown { mob, at } => {
                    self.lan_entity_event(mob as i32, super::combat::DROWN_EVENT, at);
                    let v = self
                        .mobs
                        .iter()
                        .find(|m| m.id == mob)
                        .map_or([0.0; 3], |m| [m.velocity.x, m.velocity.y, m.velocity.z]);
                    self.drown_particles(at, v);
                }
                MobEvent::PowderSnowParticle { at, velocity } => {
                    self.snowflake_particle(at, velocity);
                }
                MobEvent::SunProtectionWear { mob } => self.wear_sun_protection(mob),
                MobEvent::ArmorHurt { mob, amount, kind } => self.hurt_mob_armor(mob, amount, kind),
                MobEvent::GliderWear { mob, slot } => self.wear_mob_glider(mob, slot),
                MobEvent::EntityEvent { mob, event, at } => {
                    // Ravager.roar (event 69 ends it): with mob_griefing on,
                    // ROAR_TARGET_WITH_GRIEFING also takes the armor stands.
                    if event == 69
                        && self.ui.rules.bool("mob_griefing")
                        && let Some(area) = self
                            .mobs
                            .iter()
                            .find(|m| m.id == mob && m.species() == Species::Ravager)
                            .map(|m| m.bounds().inflate(Vec3::new(4.0, 4.0, 4.0)))
                    {
                        self.roar_pushes_armor_stands(area, at);
                    }
                    self.lan_entity_event(mob as i32, event, at)
                }
                MobEvent::LevelParticles {
                    kind,
                    at,
                    spread,
                    speed,
                    count,
                } => {
                    if let Some(ty) = horizon_client::particles::ParticleType::from_name(kind) {
                        let options = horizon_client::particles::ParticleOptions::simple(ty);
                        self.particles
                            .send_particles(&options, at, spread, speed, count);
                        self.lan_level_particles(None, &options, at, spread, speed, count, None);
                    }
                }
                // The LAN packet has no item for a particle yet: the host's window only.
                MobEvent::ItemParticles {
                    item,
                    at,
                    spread,
                    speed,
                    count,
                } => {
                    let options =
                        self.item_particle(&horizon_client::items::ItemStack::new(item, 1));
                    self.particles
                        .send_particles(&options, at, spread, speed, count);
                }
                MobEvent::EntitySound {
                    mob,
                    name,
                    at,
                    volume,
                    pitch,
                    source,
                } => {
                    let silent = self
                        .mobs
                        .iter()
                        .find(|m| m.id == mob)
                        .is_some_and(|m| m.silent);
                    let entity = horizon_client::sound_engine::SoundEntity::new(mob, at, silent);
                    self.audio.play_bound(
                        &format!("minecraft:{name}"),
                        source,
                        volume,
                        pitch,
                        entity,
                        None,
                    );
                }
                MobEvent::Sound(name, at, volume, pitch) => {
                    self.audio
                        .play(&format!("minecraft:{name}"), Some(at), volume, pitch)
                }
                MobEvent::LocalSound(name, at, volume, pitch) => {
                    // Level.playLocalSound: play locally without broadcasting.
                    self.audio
                        .play_unshared(&format!("minecraft:{name}"), Some(at), volume, pitch)
                }
                MobEvent::LevelEventSound {
                    sound,
                    at,
                    volume,
                    spread,
                } => {
                    // WitherBoss.customServerAiStep: the spawn roar is global
                    // level event 1023 at the block position.
                    if sound == "entity.wither.spawn" {
                        let pos = at.map(|v| v.floor() as i32);
                        self.global_level_event(1023, pos, 0);
                    } else {
                        // The client's own level random: nothing drawn from the server's.
                        let mut r = horizon_core::random::LegacyRandom::new(
                            self.world.game_time.wrapping_mul(341_873_128_712),
                        );
                        let pitch = (r.next_float() - r.next_float()) * spread + 1.0;
                        self.audio
                            .play(&format!("minecraft:{sound}"), Some(at), volume, pitch)
                    }
                }
                MobEvent::SoundId(name, at, volume, pitch) => {
                    self.audio.play(&name, Some(at), volume, pitch)
                }
                MobEvent::GameEvent {
                    event, at, source, ..
                } if event == "minecraft:flap"
                    && self.mobs.iter().any(|m| {
                        m.id == source && matches!(m.species(), Species::Chicken | Species::Parrot)
                    }) =>
                {
                    let ctx = self.who_context(Some(Who::Mob(source)), None);
                    self.post_game_event(event, at, ctx);
                }
                // A mob's own `gameEvent(..)` calls: Armadillo.rollUp/rollOut's
                // ENTITY_ACTION, Creeper.tick's PRIME_FUSE, Mob.ate's EAT, the
                // shulker's lid (Shulker.setRawPeekAmount), an evoker's vexes
                // (ENTITY_PLACE), Enderman.teleport's TELEPORT,
                // LivingEntity.onEquipItem's EQUIP/UNEQUIP.
                MobEvent::GameEvent {
                    event, at, source, ..
                } if matches!(
                    event,
                    "minecraft:entity_action"
                        | "minecraft:prime_fuse"
                        | "minecraft:eat"
                        | "minecraft:container_open"
                        | "minecraft:container_close"
                        | "minecraft:entity_place"
                        | "minecraft:teleport"
                        | "minecraft:equip"
                        | "minecraft:unequip"
                ) =>
                {
                    let ctx = self.who_context(Some(Who::Mob(source)), None);
                    self.post_game_event(event, at, ctx);
                }
                MobEvent::GameEvent {
                    event: event @ ("minecraft:block_place" | "minecraft:block_destroy"),
                    at,
                    source,
                    state,
                } => {
                    let ctx = self.who_context(Some(Who::Mob(source)), Some(state));
                    self.post_game_event(event, at, ctx);
                }
                // `FarmlandBlock.turnToBaseBlock` and `TurtleEggBlock.decreaseEggs`:
                // the landing's own block change posts its game event, from the
                // mob that landed (`Context.of(entity, state)`) unless the block
                // destroyed itself under it (`Context.of(state)`).
                MobEvent::LandingGameEvent {
                    event,
                    at,
                    source,
                    state,
                } => {
                    let who = source.map(Who::Mob);
                    self.post_block_game_event(event, at, who, Some(state));
                }
                // The sculk layer tracks steps and bee/bat flaps itself
                // (`entity_motion_events`, `tick_mob_sculk_events`). Birds
                // above use their movement's flyDist/nextFlap threshold.
                MobEvent::GameEvent { .. } => {}
                MobEvent::AttackPlayer(amount, from) => {
                    self.mob_hits_player(amount, from, "minecraft:mob_attack");
                }
                // doHurtTarget: the effect only when hurtServer took damage.
                // Bee.doHurtTarget: a sting that hurt leaves the stinger in the
                // body, then the poison.
                MobEvent::StingPlayer {
                    amount,
                    from,
                    poison_ticks,
                } => {
                    if self.mob_hits_player(amount, from, "minecraft:mob_attack") {
                        self.combat.stuck.add_stinger();
                        if poison_ticks > 0 {
                            self.give_player_effect("minecraft:poison", poison_ticks, 0);
                        }
                    }
                }
                MobEvent::AttackPlayerWithEffect {
                    amount,
                    from,
                    effect,
                    ticks,
                    amplifier,
                } => {
                    if self.mob_hits_player(amount, from, "minecraft:mob_attack") {
                        self.give_player_effect(effect, ticks, amplifier);
                    }
                }
                MobEvent::DamagePlayer { amount, from, kind } => {
                    self.mob_hits_player(amount, from, kind);
                }
                MobEvent::ElderCurse { at, silent } => self.elder_curse(at, silent),
                MobEvent::SetVelocity { target, velocity } => {
                    let v = Vec3::new(velocity[0], velocity[1], velocity[2]);
                    match target {
                        Who::Player => {
                            if let Some(p) = self.player.as_mut() {
                                p.velocity = v;
                            }
                        }
                        Who::Mob(id) => {
                            if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
                                m.velocity = v;
                            }
                        }
                    }
                }
                MobEvent::RemoveKilled { target } => {
                    // The bite that killed it ran `LivingEntity.die` (loot, such
                    // as a magma cube's froglight, and experience) before
                    // `eatEntity` removes it: a removed mob never reaches the
                    // next tick's deaths, so it dies here first.
                    if let Some(m) = self
                        .mobs
                        .iter()
                        .find(|m| m.id == target && !m.alive() && !m.removed)
                        .cloned()
                    {
                        if self.died.insert(target) {
                            self.mob_died(&m);
                        }
                        if let Some(m) = self.mobs.iter_mut().find(|m| m.id == target) {
                            m.removed = true;
                        }
                    }
                }
                MobEvent::Push { target, velocity } => {
                    let v = Vec3::new(velocity[0], velocity[1], velocity[2]);
                    match target {
                        Who::Player => {
                            if let Some(p) = self.player.as_mut() {
                                p.velocity = p.velocity.add_vector(v);
                            }
                        }
                        Who::Mob(id) => {
                            if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
                                m.velocity = m.velocity.add_vector(v);
                            }
                        }
                    }
                }
                MobEvent::Knockback {
                    target,
                    power,
                    x,
                    z,
                    source,
                } => match target {
                    // A player's source names no melee attacker, as its damage's doesn't.
                    Who::Player => {
                        let _attacker = self.knockback_scope(match source.attacker {
                            Some(Who::Mob(id)) => id as i32,
                            _ => 0,
                        });
                        let source = HurtSource {
                            damage_type: source_damage_type(source),
                            causing: 0,
                            direct: 0,
                        };
                        self.knock_back_player(power, x, z, source);
                    }
                    Who::Mob(id) => {
                        if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
                            m.knockback(power, x, z, &source);
                        }
                    }
                },
                MobEvent::Shift { target, delta } => {
                    let d = Vec3::new(delta[0], delta[1], delta[2]);
                    match target {
                        Who::Player => {
                            if let Some(p) = self.player.as_mut() {
                                p.position = p.position.add_vector(d);
                            }
                        }
                        Who::Mob(id) => {
                            if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
                                m.position = m.position.add_vector(d);
                            }
                        }
                    }
                }
                MobEvent::LidPush {
                    shulker,
                    area,
                    delta,
                } => self.lid_pushes_armor_stands(shulker, area, delta),
                MobEvent::Wololo { mob } => {
                    if let Some(m) = self.mobs.iter_mut().find(|m| m.id == mob) {
                        // DyeColor.RED.
                        m.extra.color = 14;
                    }
                }
                MobEvent::Fangs {
                    at,
                    yaw,
                    delay,
                    owner,
                } => self.add_fangs(at, yaw, delay, owner),
                MobEvent::Shoot(..) => {}
                MobEvent::ExplosionSource {
                    mob,
                    charged,
                    name,
                    in_water,
                    kind,
                } => {
                    self.combat.explosion_source = Some(mob);
                    self.combat.explosion_in_water = in_water;
                    // The creeper causes the blast's damage, but it is out of
                    // the mob list already: its name comes with the event.
                    self.combat.exploded.push((mob, name));
                    self.combat.exploded_kinds.push((mob, kind));
                    if charged {
                        self.combat.charged_exploders.push(mob);
                    }
                }
                MobEvent::LingeringCloud {
                    at,
                    effects,
                    hidden,
                    ambient,
                    color,
                } => {
                    self.spawn_creeper_cloud(at, effects, hidden, ambient, color);
                }
                MobEvent::Explode(at, radius, species, silent) => {
                    // The exploding mob causes the damage (Creeper.explodeCreeper).
                    let noted = self.combat.explosion_source.is_some();
                    let source_mob = self.combat.explosion_source.or_else(|| {
                        self.mobs
                            .iter()
                            .filter(|m| {
                                (m.position.x - at[0]).powi(2) + (m.position.z - at[2]).powi(2)
                                    < 1.0
                            })
                            .map(|m| m.id)
                            .next()
                    });
                    self.combat.explosion_source = source_mob;
                    self.combat.explosion_source_named = noted;
                    self.combat.explosion_cause = match species {
                        Species::Creeper => Some("minecraft:creeper"),
                        Species::Wither => Some("minecraft:wither"),
                        Species::SulfurCube => Some("minecraft:sulfur_cube"),
                        _ => None,
                    }
                    .map(|kind| horizon_client::advancements::EntityView {
                        kind: kind.into(),
                        position: at,
                        ..Default::default()
                    });
                    // Only a creeper or a wither empties the hives it blows up.
                    // The creeper is already gone from the list, so the event
                    // carries its kind. A sulfur cube's blast is TNT's.
                    let mut source =
                        super::explosion::ExplosionSource::default().with_silence(silent);
                    if let Some(id) = source_mob
                        && matches!(species, Species::Creeper | Species::Wither)
                    {
                        let kind = match species {
                            Species::Creeper => "minecraft:creeper",
                            _ => "minecraft:wither",
                        };
                        source = source.with_entity(id, kind);
                    }
                    let result = match species {
                        Species::Creeper | Species::Wither => {
                            self.with_hive_emptying_blast(|g| g.explode(at, radius, source))
                        }
                        Species::SulfurCube => self.explode_sulfur_cube(at, radius, source),
                        _ => self.explode(at, radius, source),
                    };
                    self.combat.explosion_source = None;
                    self.combat.explosion_source_named = false;
                    self.combat.explosion_in_water = false;
                    self.combat.explosion_cause = None;
                    result?
                }
                MobEvent::AttackMob {
                    target,
                    amount,
                    source,
                } => {
                    self.mob_attacks_mob(target, amount, source);
                }
                MobEvent::DamageMob {
                    target,
                    amount,
                    source,
                } => {
                    self.mob_hits_mob(target, amount, source);
                }
                MobEvent::AttackMobWithEffect {
                    target,
                    amount,
                    source,
                    effect,
                    ticks,
                    amplifier,
                } => {
                    // CaveSpider, Husk and WitherSkeleton.doHurtTarget: the
                    // effect sits inside the branch where the hit landed, and
                    // `LivingEntity.addEffect` takes a mob as well as a player.
                    if self.mob_attacks_mob(target, amount, source)
                        && let Some(t) = self.mobs.iter_mut().find(|m| m.id == target)
                    {
                        t.add_effect(effect, ticks, amplifier);
                    }
                }
                MobEvent::CubeHurtPlayer { cube, amount, from } => {
                    // AbstractCubeMob.dealDamage: the attack sound (and its
                    // two pitch draws, made even for a silent cube, whose
                    // Entity.playSound is then skipped) only follows a hit
                    // that landed.
                    if self.hurt_player_from(
                        amount,
                        Some(from),
                        "minecraft:mob_attack",
                        Some(Some(cube)),
                    ) && let Some(m) = self.mobs.iter_mut().find(|m| m.id == cube)
                    {
                        let pitch = (m.random.next_float() - m.random.next_float()) * 0.2 + 1.0;
                        let at = m.center();
                        if !m.silent {
                            self.handle_mob_events(vec![MobEvent::Sound(
                                "entity.slime.attack",
                                at,
                                1.0,
                                pitch,
                            )])?;
                        }
                    }
                }
                MobEvent::CubeHurtMob {
                    cube,
                    target,
                    amount,
                    source,
                } => {
                    // AbstractCubeMob.dealDamage: the attack sound (and its
                    // two pitch draws, made even for a silent cube, whose
                    // Entity.playSound is then skipped) only follows a hit
                    // that landed.
                    if self.mob_hits_mob(target, amount, source)
                        && let Some(m) = self.mobs.iter_mut().find(|m| m.id == cube)
                    {
                        let pitch = (m.random.next_float() - m.random.next_float()) * 0.2 + 1.0;
                        let at = m.center();
                        if !m.silent {
                            self.handle_mob_events(vec![MobEvent::Sound(
                                "entity.slime.attack",
                                at,
                                1.0,
                                pitch,
                            )])?;
                        }
                        self.mob_post_attack(
                            target,
                            source.attacker,
                            None,
                            Some(super::creatures::damage_type(source.kind, false)),
                        );
                    }
                }
                MobEvent::SonicBoom {
                    target,
                    attacker,
                    from,
                    direction,
                } => {
                    // SonicBoom: the push only follows a hit that landed, scaled
                    // by the target's KNOCKBACK_RESISTANCE.
                    let (hurt, resistance) = match target {
                        Who::Player => {
                            let hurt = self.mob_hits_player(10.0, from, "minecraft:sonic_boom");
                            let resistance = self.player.as_mut().map_or(0.0, |p| {
                                p.attribute("minecraft:knockback_resistance").unwrap_or(0.0)
                            });
                            (hurt, resistance)
                        }
                        Who::Mob(id) => {
                            let source = Damage {
                                kind: DamageKind::SonicBoom,
                                attacker: Some(Who::Mob(attacker)),
                                from: Some(from),
                                attacker_species: Some(Species::Warden),
                            };
                            let hurt = self.mob_hits_mob(id, 10.0, source);
                            let resistance = self
                                .mobs
                                .iter()
                                .find(|m| m.id == id)
                                .map_or(0.0, |m| self.mob_knockback_resistance(m));
                            (hurt, resistance)
                        }
                    };
                    if hurt {
                        let horizontal = 2.5 * (1.0 - resistance);
                        let vertical = 0.5 * (1.0 - resistance);
                        let velocity = [
                            direction[0] * horizontal,
                            direction[1] * vertical,
                            direction[2] * horizontal,
                        ];
                        self.handle_mob_events(vec![MobEvent::Push { target, velocity }])?;
                    }
                }
                MobEvent::FinishAdmiring { mob } => self.finish_admiring(mob),
                MobEvent::Hiss { mob } => {
                    if let Some(m) = self.mobs.iter().find(|m| m.id == mob) {
                        let at = m.center();
                        let sound = horizon_client::mobs::finalize::variant_sound(m, "hiss")
                            .unwrap_or("entity.cat.hiss");
                        self.audio
                            .play(&format!("minecraft:{sound}"), Some(at), 1.0, 1.0);
                    }
                }
                MobEvent::Tamed { .. }
                | MobEvent::Buck { .. }
                | MobEvent::StabDismount { .. }
                | MobEvent::EjectPassengers { .. } => self.mount_event(&e),
                MobEvent::IronGolemHit {
                    target,
                    amount,
                    source,
                } => {
                    // IronGolem.doHurtTarget: the toss only follows a hit that
                    // landed, scaled by the target's KNOCKBACK_RESISTANCE.
                    let hurt = match target {
                        Who::Player => {
                            let from = source.from.unwrap_or_default();
                            self.mob_hits_player(amount, from, "minecraft:mob_attack")
                        }
                        Who::Mob(id) => self.mob_hits_mob(id, amount, source),
                    };
                    if hurt {
                        let resistance = match target {
                            Who::Player => self.player.as_mut().map_or(0.0, |p| {
                                p.attribute("minecraft:knockback_resistance").unwrap_or(0.0)
                            }),
                            Who::Mob(id) => self
                                .mobs
                                .iter()
                                .find(|m| m.id == id)
                                .map_or(0.0, |m| self.mob_knockback_resistance(m)),
                        };
                        let up = f64::from(0.4f32) * (1.0 - resistance).max(0.0);
                        match target {
                            Who::Player => {
                                if let Some(p) = self.player.as_mut() {
                                    p.velocity.y += up;
                                }
                            }
                            Who::Mob(id) => {
                                if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
                                    m.velocity.y += up;
                                }
                            }
                        }
                        if let Who::Mob(id) = target {
                            self.mob_post_attack(
                                id,
                                source.attacker,
                                None,
                                Some("minecraft:mob_attack"),
                            );
                        }
                    }
                }
                MobEvent::Bonemeal { pos } => {
                    let at = BlockPos::new(pos[0], pos[1], pos[2]);
                    if self.with_level(|b, level, ticks| b.grow_crop(level, ticks, at)) {
                        self.level_events
                            .push(horizon_world::level::LevelEvent::LevelEvent {
                                id: 1505,
                                pos: at,
                                data: 15,
                            });
                        let _ = self.process_level_events();
                    }
                }
                MobEvent::RingBell { bell } => {
                    let pos = BlockPos::new(bell[0], bell[1], bell[2]);
                    let rang = self
                        .with_level(|b, level, ticks| b.bell_attempt_to_ring(level, ticks, pos));
                    if rang {
                        let _ = self.process_level_events();
                    }
                }
                MobEvent::ChestOpen { chest, open, mob } => self.golem_chest_open(chest, open, mob),
                MobEvent::ChestSlot { chest, slot, stack } => {
                    self.golem_chest_slot(chest, slot, stack)
                }
                MobEvent::BecomeStatue { mob } => self.golem_become_statue(mob),
                MobEvent::EnterHive { mob, hive } => self.bee_enter_hive(mob, hive),
                MobEvent::ParrotToShoulder { mob } => self.parrot_to_shoulder(mob),
                MobEvent::Infested { at, velocities } => self.spawn_infested(at, velocities),
                MobEvent::Gift { target, item } => {
                    if let Some(m) = self.mobs.iter_mut().find(|m| m.id == target)
                        && m.saddle_item().is_none()
                    {
                        m.set_saddle(Some(ItemStack::new(item, 1)));
                    }
                }
                MobEvent::HurtSelf { mob, kind, amount } => {
                    if let Some(m) = self.mobs.iter_mut().find(|m| m.id == mob) {
                        m.damage(amount, horizon_client::mobs::Damage::of(kind));
                    }
                }
                MobEvent::SetTarget { mob, target } => {
                    if let Some(m) = self.mobs.iter_mut().find(|m| m.id == mob) {
                        m.ai.target = Some(target);
                    }
                }
                MobEvent::HoglinAttackTarget { mob, target } => {
                    let places: Vec<(u32, horizon_core::geometry::Vec3)> = self
                        .mobs
                        .iter()
                        .filter(|m| m.alive())
                        .map(|m| (m.id, m.position))
                        .collect();
                    let position = |who: Who| match who {
                        Who::Mob(id) => places.iter().find(|(i, _)| *i == id).map(|(_, p)| *p),
                        Who::Player => None,
                    };
                    if let Some(m) = self.mobs.iter_mut().find(|m| m.id == mob) {
                        horizon_client::mobs::piglin::receive_attack_target(m, target, position);
                    }
                }
                MobEvent::HuntHoglin { mob, target, ticks } => {
                    if let Some(m) = self.mobs.iter_mut().find(|m| m.id == mob) {
                        m.extra.ints[0] = ticks;
                        if m.ai.target.is_none() {
                            m.ai.target = Some(Who::Mob(target));
                        }
                    }
                }
                MobEvent::Projectile(p) => self.launch_mob_projectile(p),
                MobEvent::BowShot {
                    mob,
                    weapon,
                    mut arrow,
                } => {
                    self.mob_bow_ammo(mob, &weapon, &mut arrow);
                    self.launch_mob_projectile(arrow);
                }
                MobEvent::CrossbowShot { ammo, mut arrow } => {
                    if &*ammo.item == "minecraft:firework_rocket" {
                        // `CrossbowItem.createProjectile`: a rocket, from 0.15 under the eyes.
                        let mut at = arrow.position;
                        at.y -= f64::from(0.15f32) - 0.1;
                        self.launch_angled_rocket(at, arrow.velocity, &ammo, arrow.owner);
                    } else {
                        Self::arrow_from_ammo(&ammo, &mut arrow);
                        self.launch_mob_projectile(arrow);
                    }
                }
                MobEvent::CrossbowLoad {
                    mob,
                    weapon,
                    at,
                    pitch,
                } => {
                    if self.mob_crossbow_ammo(mob, &weapon) {
                        self.audio.play(
                            "minecraft:item.crossbow.loading_end",
                            Some(at),
                            1.0,
                            pitch,
                        );
                    }
                }
                MobEvent::Spawn(mut m) => {
                    self.next_entity += 1;
                    m.id = self.next_entity;
                    self.mobs.push(*m);
                    self.native_mobs_joined(self.mobs.len() - 1, false);
                }
                MobEvent::SpawnKind { kind, at, setup } => {
                    let id = if setup.skip_finalize {
                        self.spawn_mob_raw(&kind, at, setup.persistent)
                    } else {
                        self.spawn_mob(&kind, at, setup.persistent)
                    };
                    if let Some(id) = id {
                        self.apply_setup(id, &setup);
                    }
                }
                MobEvent::Convert { from, kind, params } => {
                    let id = self.convert_to(&from, kind, params);
                    // `Piglin.finishConversion` and `Hoglin.finishConversion`:
                    // the converted mob is nauseous for 10 seconds.
                    if matches!(kind, "minecraft:zombified_piglin" | "minecraft:zoglin")
                        && let Some(m) = self.mobs.iter_mut().find(|m| Some(m.id) == id)
                    {
                        m.add_effect("minecraft:nausea", 200, 0);
                    }
                    // `Tadpole.ageUp`: the frog is finalized for the
                    // conversion and made persistent.
                    if kind == "minecraft:frog"
                        && let Some(id) = id
                    {
                        self.finalize_converted(id);
                    }
                }
                MobEvent::ThrowItem {
                    mob,
                    stack,
                    at,
                    motion,
                    pickup_delay,
                } => {
                    // `thrownItem.setThrower(the mob)`.
                    self.spawn_item(stack, at, motion, pickup_delay);
                    let thrower = self.mob_uuid(mob);
                    if let Some(thrown) = self.just_spawned_item() {
                        thrown.thrower = Some(thrower);
                    }
                }
                MobEvent::DropStack { mob, stack, at } => {
                    self.native_entity_drop_item(mob, &stack.item, stack.count as i32, at);
                    self.spawn_item_own(stack, at, 10);
                }
                MobEvent::DropItem {
                    mob,
                    item,
                    count,
                    at,
                } => {
                    self.native_entity_drop_item(mob, &item, count as i32, at);
                    // The ItemEntity constructor's motion, from the entity's
                    // own random (not the level's), so spawnAtLocation drops
                    // (a fox's berries, a panda's held food, a goat's horn)
                    // land apart.
                    self.spawn_item_own(
                        ItemStack {
                            item,
                            count,
                            damage: 0,
                            extra: None,
                        },
                        at,
                        10,
                    );
                }
                MobEvent::DropGoatHorn {
                    mob,
                    screaming,
                    at,
                    motion,
                } => {
                    let instrument =
                        horizon_client::mobs::goat::create_horn(self.mob_uuid(mob), screaming);
                    let mut horn = ItemStack::new("minecraft:goat_horn", 1);
                    horn.data_mut().set_other(
                        "minecraft:instrument",
                        Some(horizon_world::nbt::Tag::String(instrument.into())),
                    );
                    // Goat.dropHorn's ItemEntity constructor leaves pickupDelay at
                    // 0 (no setDefaultPickUpDelay).
                    self.spawn_item(horn, at, motion, 0);
                }
                MobEvent::DropGift { table, at } => {
                    let stacks =
                        self.res
                            .items()
                            .loot_table(table, false, None, &mut self.world.random);
                    for stack in stacks {
                        // The ItemEntity constructor's motion, from the
                        // entity's own random (not the level's).
                        self.spawn_item_own(stack, at, 10);
                    }
                }
                // `Level.setBlockAndUpdate`: neighbour and shape updates, so a crop
                // above trampled farmland breaks and drops.
                MobEvent::SetBlock { pos, state } => self.level_set(pos, state)?,
                MobEvent::EndermanPlaceBlock {
                    mob,
                    pos,
                    carried,
                    occupants,
                } => {
                    self.enderman_place_block(mob, pos, carried, &occupants)?;
                }
                MobEvent::CropRaided {
                    pos,
                    state,
                    old,
                    mob,
                } => {
                    self.level_set_with_flags(
                        pos,
                        state,
                        horizon_world::level::flags::UPDATE_CLIENTS,
                    )?;
                    if let Some(old) = old {
                        self.post_block_game_event(
                            "minecraft:block_change",
                            pos,
                            Some(Who::Mob(mob)),
                            None,
                        );
                        self.level_event_at(2001, pos, old.0 as i32);
                    }
                }
                MobEvent::CrackTurtleEgg { pos } => self.crack_turtle_egg(pos)?,
                MobEvent::BedOccupied { pos, occupied } => self.set_bed_occupied(pos, occupied),
                MobEvent::SetDoorOpen { pos, open } => {
                    let pos = BlockPos::new(pos[0], pos[1], pos[2]);
                    self.with_level(|b, level, ticks| b.set_door_open(level, ticks, pos, open));
                    self.process_level_events()?;
                }
                MobEvent::BreakDoor { mob, pos } => {
                    // BreakDoorGoal.tick: removeBlock, then level events 1021
                    // and 2001 with the state that is there now.
                    let kind = self
                        .mobs
                        .iter()
                        .find(|m| m.id == mob)
                        .map(super::native_hooks::mob_kind)
                        .unwrap_or_default();
                    let air = self.world.air;
                    let at = BlockPos::new(pos[0], pos[1], pos[2]);
                    self.with_level(|b, level, ticks| b.set_block(level, ticks, at, air, 3));
                    self.process_level_events()?;
                    self.level_event_at(1021, pos, 0);
                    let left = self.world.block(pos[0], pos[1], pos[2]);
                    self.level_event_at(2001, pos, left.0 as i32);
                    self.native_entity_break_door(mob, &kind, pos);
                }
                MobEvent::DestroyProgress { mob, pos, progress } => {
                    self.server_destroy_block_progress(mob as i32, pos, progress);
                }
                MobEvent::LevelEvent { id, pos, data } => self.level_event_at(id, pos, data),
                MobEvent::SetBlockName {
                    mob,
                    pos,
                    name,
                    effect,
                    flags,
                } => {
                    if effect {
                        // EatBlockGoal passes the block's default state constant
                        // (Blocks.GRASS_BLOCK.defaultBlockState()), not the live state
                        // (which could be snowy).
                        let old = self.world.block(pos[0], pos[1], pos[2]);
                        let state = self
                            .pack
                            .assets
                            .registry
                            .default_state(&self.res.facts().get(old).name)
                            .unwrap_or(old);
                        self.level_event_at(2001, pos, state.0 as i32);
                    }
                    if let Some(state) = self.pack.assets.registry.default_state(name) {
                        let old_name = self
                            .res
                            .facts()
                            .get(self.world.block(pos[0], pos[1], pos[2]))
                            .name
                            .clone();
                        let mut cancel = false;
                        if let Some(mob) = mob
                            && let Some(kind) = self
                                .mobs
                                .iter()
                                .find(|m| m.id == mob)
                                .map(|m| m.kind.id.clone())
                        {
                            self.native_entity_change_block_v3(
                                mob,
                                &kind,
                                pos,
                                &old_name,
                                name,
                                &mut cancel,
                            );
                        }
                        if !cancel {
                            self.level_set_with_flags(pos, state, flags)?;
                            if let Some(mob) = mob
                                && let Some(m) = self.mobs.iter().find(|m| m.id == mob)
                            {
                                let kind = m.kind.id.clone();
                                self.native_entity_change_block(mob, &kind, pos, &old_name, name);
                            }
                        }
                    }
                }
                MobEvent::Infest {
                    pos,
                    infested,
                    poof,
                } => {
                    let registry = &self.pack.assets.registry;
                    let old = self.world.block(pos[0], pos[1], pos[2]);
                    if let Some(mut state) = registry.state(old).cloned() {
                        let (from, to) = if infested {
                            ("minecraft:", "minecraft:infested_")
                        } else {
                            ("minecraft:infested_", "minecraft:")
                        };
                        state.name = state.name.replacen(from, to, 1);
                        if let Ok(id) = registry.resolve(&state) {
                            self.level_set(pos, id)?;
                        }
                    }
                    if let Some(mob) = poof {
                        self.mob_poof(mob);
                    }
                }
                MobEvent::BreakBlock { pos, drop } => {
                    let old = self.world.block(pos[0], pos[1], pos[2]);
                    // InfestedBlock.spawnAfterBreak: its silverfish comes out
                    // when blocks drop.
                    if drop
                        && self
                            .res
                            .facts()
                            .get(old)
                            .name
                            .starts_with("minecraft:infested_")
                        && self.ui.rules.bool("block_drops")
                    {
                        let at = [
                            f64::from(pos[0]) + 0.5,
                            f64::from(pos[1]),
                            f64::from(pos[2]) + 0.5,
                        ];
                        if let Some(id) = self.spawn_mob_raw("minecraft:silverfish", at, false) {
                            self.mob_poof(id);
                        }
                    }
                    // Block.spawnDestroyParticles: fire breaks leave nothing.
                    if !self.res.facts().get(old).has_class("BaseFireBlock") {
                        self.level_event_at(2001, pos, old.0 as i32);
                    }
                    if drop {
                        let mut drops = world_items!(self).block_drops_in(
                            self.pack.assets.registry,
                            old,
                            None,
                            &mut self.world.random,
                            Some((&mut self.rand_sequences, self.level.seed)),
                        );
                        self.native_block_drops(pos, old, None, None, None, &mut drops);
                        for d in drops {
                            self.pop_resource(pos, d);
                        }
                    }
                    // Level.destroyBlock: setBlock(fluidState.createLegacyBlock(), 3),
                    // so water stays water and a waterlogged block leaves its water.
                    let b = self.res.behaviours();
                    let left = b.legacy_block(b.fluid(old));
                    if left != old {
                        self.level_set(pos, left)?;
                    }
                }
                MobEvent::Particles { kind, at, count } => {
                    // Mob particle bursts (hearts, smoke, splashes) around a point.
                    if let Some(ty) = horizon_client::particles::ParticleType::from_name(kind) {
                        if kind.starts_with("item_") {
                            self.ensure_item_particle_sprites();
                        }
                        let options = horizon_client::particles::ParticleOptions::simple(ty);
                        if ty == horizon_client::particles::ParticleType::SonicBoom {
                            self.lan_sonic_boom(&options, at);
                        }
                        // Count 0: one particle exactly there, at rest.
                        if count == 0 {
                            self.particles.add(&options, at, [0.0; 3]);
                        }
                        for _ in 0..count {
                            let r = &mut self.world.random;
                            let p = [
                                at[0] + (f64::from(r.next_float()) - 0.5),
                                at[1] + (f64::from(r.next_float()) - 0.5) * 0.5,
                                at[2] + (f64::from(r.next_float()) - 0.5),
                            ];
                            let v = [
                                r.next_gaussian() * 0.02,
                                r.next_gaussian() * 0.02,
                                r.next_gaussian() * 0.02,
                            ];
                            self.particles.add(&options, p, v);
                        }
                    }
                }
                MobEvent::AroundSelf {
                    mob,
                    event,
                    kind,
                    feet,
                    width,
                    height,
                    burst,
                } => {
                    self.lan_entity_event(mob as i32, event, feet);
                    self.particles_around(kind, feet, [width, height], burst)
                }
                MobEvent::ParticleAt { kind, at, velocity } => {
                    if let Some(ty) = horizon_client::particles::ParticleType::from_name(kind) {
                        let options = horizon_client::particles::ParticleOptions::simple(ty);
                        self.particles.add(&options, at, velocity);
                    }
                }
                MobEvent::ParticleColored {
                    at,
                    color: [r, g, b],
                } => {
                    let options = horizon_client::particles::ParticleOptions::colored(
                        horizon_client::particles::ParticleType::EntityEffect,
                        [r, g, b, 1.0],
                    );
                    self.particles.add(&options, at, [0.0; 3]);
                }
                MobEvent::Experience { at, amount } => self.spawn_breeding_orb(at, amount),
                MobEvent::AxolotlSupportPlayer { mob } => {
                    let previous = self
                        .potions
                        .effects
                        .get("regeneration")
                        .map_or(0, |e| e.duration);
                    if (0..=2399).contains(&previous) {
                        self.potions
                            .effects
                            .add(horizon_client::effects::EffectInstance::new(
                                "regeneration",
                                (previous + 100).min(2400),
                                0,
                            ));
                    }
                    self.potions
                        .effects
                        .list
                        .retain(|e| e.id != "mining_fatigue");
                    let source = self
                        .mobs
                        .iter()
                        .find(|m| m.id == mob)
                        .map(|m| self.mob_entity(m));
                    self.trigger_effects_changed_by(source);
                }
                MobEvent::DolphinTreasure { mob } => {
                    // ServerLevel.findNearestMapStructure(#dolphin_located, 50).
                    let Some(at) = self.mobs.iter().find(|m| m.id == mob).map(|m| {
                        [
                            m.position.x.floor() as i32,
                            m.position.y.floor() as i32,
                            m.position.z.floor() as i32,
                        ]
                    }) else {
                        continue;
                    };
                    let names = self.structure_tag("minecraft:dolphin_located");
                    let found = self
                        .end
                        .locator()
                        .filter(|_| !names.is_empty())
                        .and_then(|t| t.locate(&names, at, 50));
                    if let Some(m) = self.mobs.iter_mut().find(|m| m.id == mob) {
                        m.extra.home = found.map(|(_, p)| p);
                    }
                }
                MobEvent::EffectPlayer {
                    effect,
                    ticks,
                    amplifier,
                    ambient,
                } => {
                    self.give_mob_effect(effect, ticks, amplifier, ambient);
                }
                MobEvent::EffectMob {
                    target,
                    effect,
                    ticks,
                    amplifier,
                } => {
                    if let Some(t) = self.mobs.iter_mut().find(|m| m.id == target) {
                        t.add_effect(effect, ticks, amplifier);
                    }
                }
                MobEvent::Ignite { target, seconds } => match target {
                    Who::Player => {
                        if !self.creative
                            && let Some(p) = self.player.as_mut()
                        {
                            p.ignite_for_ticks(seconds * 20)?;
                        }
                    }
                    Who::Mob(id) => {
                        if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
                            m.ignite_for_ticks(seconds * 20);
                        }
                    }
                },
                MobEvent::Breed {
                    parent,
                    partner,
                    llama,
                } => {
                    let mob_count = self.mobs.len();
                    self.breed(parent, partner, llama);
                    let child = self.mobs.get(mob_count).map(|m| m.id);
                    self.native_entity_breed(parent, partner, child);
                }
                MobEvent::LockGaze { mob, partner } => {
                    if let Some(m) = self.mobs.iter_mut().find(|m| m.id == mob) {
                        horizon_client::mobs::animal_brain::lock_gaze(m, partner);
                    }
                }
                MobEvent::BreedTarget { mob, partner } => {
                    if let Some(m) = self.mobs.iter_mut().find(|m| m.id == mob) {
                        horizon_client::mobs::animal_brain::set_breed_partner(m, partner);
                    }
                }
                MobEvent::Retreat { mob, from, at } => {
                    if let Some(m) = self.mobs.iter_mut().find(|m| m.id == mob) {
                        horizon_client::mobs::piglin::receive_retreat(m, from, at);
                    }
                }
                MobEvent::StartRiding { rider, vehicle } => {
                    if horizon_client::mobs::riders::start_riding(&mut self.mobs, rider, vehicle) {
                        // sendEventAndTriggers: the server players already on it.
                        self.note_started_riding(vehicle);
                    }
                }
                // Applied after the fish's tick (`apply_brain_events`).
                MobEvent::StartFollowing { .. }
                | MobEvent::SchoolSize { .. }
                | MobEvent::PufferfishSting { .. } => {}
                MobEvent::LeashBreak { mob, drop, sound } => self.break_leash(mob, drop, sound),
                MobEvent::NotifyLeashHolder { holder } => {
                    if let Some(m) = self.mobs.iter_mut().find(|m| m.id == holder) {
                        horizon_client::mobs::happy_ghast::notify_leash_holder(m);
                    }
                }
                MobEvent::Vibration {
                    from,
                    to,
                    ticks,
                    source,
                } => self.mob_vibration_particle(source, from, to, ticks),
                MobEvent::DigParticles { at } => self.warden_dig_particles(at),
                MobEvent::BlockBreakEffect { pos, block } => {
                    // Turtle.aiStep broadcasts LevelEvent 2001, not the
                    // particles themselves: the event carries the particles and
                    // the sand's break sound, and reaches the clients the same
                    // way a broken block's does.
                    let state = self.world.block(block[0], block[1], block[2]);
                    self.level_event_at(2001, pos, state.0 as i32);
                }
                MobEvent::Darkness { at, radius } => self.apply_darkness(at, radius),
                MobEvent::CreakingHurt {
                    home,
                    by_player,
                    id,
                } => self.creaking_hurt(home, id, by_player),
                MobEvent::CreakingTearDown { center, spread } => {
                    self.creaking_tear_down(center, spread)
                }
                MobEvent::CureZombieVillager { mob } => self.cure_zombie_villager(mob),
                MobEvent::Reinforcements { mob, target } => self.call_reinforcements(mob, target),
                MobEvent::VillagerMeet { mob, other } => self.villager_meet(mob, other),
                MobEvent::VillagerTrade { mob, player } => self.villager_traded_by(mob, player),
                MobEvent::VillagerOffers { mob } => {
                    if let Some(i) = self.mobs.iter().position(|m| m.id == mob) {
                        self.ensure_offers(i);
                    }
                }
                MobEvent::VillagerBirth {
                    parent,
                    partner,
                    child,
                } => self.villager_birth(parent, partner, child),
                MobEvent::WorkAtComposter { mob, pos } => self.villager_work_at_composter(mob, pos),
                MobEvent::GolemCheck { mob, needed } => self.villager_golem_check(mob, needed),
                MobEvent::Firework { at, color, flight } => {
                    self.villager_firework(at, color, flight)
                }
                MobEvent::ThrowGift { table, from, to } => self.throw_gift(&table, from, to),
                MobEvent::PatrolTarget { mob, target } => {
                    if let Some(m) = self.mobs.iter_mut().find(|m| m.id == mob) {
                        horizon_client::mobs::raider::set_patrol_target(m, target);
                    }
                }
                MobEvent::RaiderTarget {
                    mob,
                    target,
                    aggressive,
                } => {
                    if let Some(m) = self.mobs.iter_mut().find(|m| m.id == mob) {
                        m.ai.target = Some(target);
                        m.ai.aggressive |= aggressive;
                    }
                }
                MobEvent::PandaSneezed { mob } => self.panda_sneezed(mob),
                MobEvent::FallToPassengers {
                    mob,
                    distance,
                    modifier,
                } => self.fall_to_passengers(mob, distance, modifier),
                MobEvent::ShedScute { mob, at, pitch } => {
                    // The mob already checked `shouldDropLoot` (a baby, `mob_drops`).
                    let table = "minecraft:gameplay/armadillo_shed";
                    let drops = self.sequence_loot(table, table, None);
                    for d in drops {
                        self.spawn_at_location(Vec3::new(at[0], at[1], at[2]), 0.0, d);
                    }
                    self.audio.play(
                        "minecraft:entity.armadillo.scute_drop",
                        Some(at),
                        1.0,
                        pitch,
                    );
                    let ctx = self.who_context(Some(Who::Mob(mob)), None);
                    self.post_game_event("minecraft:entity_place", at, ctx);
                }
                MobEvent::LayEgg { mob, at, pitch } => {
                    // Chicken.aiStep: `dropFromGiftLootTable` says whether
                    // anything dropped; only then the sound and the game event.
                    if self.drop_gift_loot(mob, "minecraft:gameplay/chicken_lay", at) {
                        self.audio
                            .play("minecraft:entity.chicken.egg", Some(at), 1.0, pitch);
                        let ctx = self.who_context(Some(Who::Mob(mob)), None);
                        self.post_game_event("minecraft:entity_place", at, ctx);
                    }
                }
                MobEvent::GiftLoot { mob, table, at } => {
                    // Turtle.ageBoundaryReached and Panda.afterSneeze: only
                    // with mob loot on.
                    if matches!(
                        table,
                        "minecraft:gameplay/turtle_grow" | "minecraft:gameplay/panda_sneeze"
                    ) && !self.ui.rules.bool("mob_drops")
                    {
                        continue;
                    }
                    self.drop_gift_loot(mob, table, at);
                }
            }
        }
        Ok(())
    }
    /// `TurtleEggBlock.decreaseEggs` after an entity's step or landing won its
    /// roll (`destroyEgg`): the break sound and the remaining eggs (a stack of
    /// one is destroyed after it).
    pub(super) fn crack_turtle_egg(&mut self, pos: [i32; 3]) -> Result<()> {
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let eggs = self
            .pack
            .assets
            .registry
            .state(state)
            .filter(|s| s.name == "minecraft:turtle_egg")
            .and_then(|s| s.properties.get("eggs"))
            .and_then(|e| e.parse::<i32>().ok());
        if let Some(eggs) = eggs {
            let at = BlockPos::new(pos[0], pos[1], pos[2]);
            self.with_level(|b, level, ticks| {
                b.turtle_egg_destroyed_by_player(level, ticks, at, state);
                if eggs <= 1 {
                    b.destroy_block(level, ticks, at, false);
                }
            });
            self.process_level_events()?;
        }
        Ok(())
    }
    /// `TurtleEggBlock.randomTick` at the last hatch stage: one baby turtle
    /// per egg, a day from grown, with the egg as its home (the sound, the
    /// crumbs and the block's removal came from the level).
    pub(super) fn hatch_turtles(
        &mut self,
        pos: [i32; 3],
        state: horizon_world::registry::BlockStateId,
    ) {
        let eggs = self
            .pack
            .assets
            .registry
            .state(state)
            .and_then(|s| s.properties.get("eggs"))
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(1);
        for i in 0..eggs {
            let at = [
                f64::from(pos[0]) + 0.3 + f64::from(i) * 0.2,
                f64::from(pos[1]),
                f64::from(pos[2]) + 0.3,
            ];
            if let Some(id) = self.spawn_mob_raw("minecraft:turtle", at, true)
                && let Some(m) = self.mobs.iter_mut().find(|m| m.id == id)
            {
                m.extra.age = -24000;
                m.extra.home = Some(pos);
                m.yaw = 0.0;
                m.body_yaw = 0.0;
                m.head_yaw = 0.0;
            }
        }
    }
    fn apply_setup(&mut self, id: u32, setup: &SpawnSetup) {
        let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) else {
            return;
        };
        if setup.baby {
            m.extra.age = -24000;
        }
        if setup.nausea {
            m.add_effect("minecraft:nausea", 200, 0);
        }
        if setup.size > 0 {
            m.extra.size = setup.size;
        }
        m.yaw = setup.yaw;
        m.body_yaw = setup.yaw;
        m.head_yaw = setup.yaw;
        m.velocity = Vec3::new(setup.velocity[0], setup.velocity[1], setup.velocity[2]);
        if let Some(h) = setup.health {
            m.health = h;
        }
        if let Some(v) = setup.variant {
            m.extra.variant = v;
        }
        if let Some(owner) = setup.owner_mob {
            m.extra.counter2 = owner as i32;
        }
        if let Some(home) = setup.home {
            m.extra.home = Some(home);
        }
        if let Some(life) = setup.life {
            m.extra.egg_time = life;
        }
    }
    /// `Animal.finalizeSpawnChildFromBreeding`'s experience orb: only when the
    /// `mob_drops` game rule is on, and one `new ExperienceOrb` of the whole
    /// value (not split or merged as `ExperienceOrb.award` does).
    fn spawn_breeding_orb(&mut self, at: [f64; 3], amount: u32) {
        if self.ui.rules.bool("mob_drops") {
            self.add_orb(at, amount);
        }
    }
    /// `ServerPlayer.tellNeutralMobsThatIDied`: the mobs within 32 blocks
    /// across and 10 up and down get `NeutralMob.playerDied`, when the
    /// `forgive_dead_players` rule is on. A mob's `Who::Player` is the guest
    /// it is bound to in `bindings` (game/lan_mobs.rs), else the host's
    /// player, so only the mobs whose binding is `conn` (`None` for the
    /// host's player) hear it.
    pub(super) fn tell_neutral_mobs_player_died(
        &mut self,
        feet: [f64; 3],
        conn: Option<u32>,
        bindings: &horizon_client::fast_hash::FxHashMap<u32, u32>,
    ) {
        // A client of someone else's world has the server's mobs only to show.
        if self.remote.is_some() || !self.ui.rules.bool("forgive_dead_players") {
            return;
        }
        let block = feet.map(f64::floor);
        let area = Aabb::new(
            Vec3::new(block[0], block[1], block[2]),
            Vec3::new(block[0] + 1.0, block[1] + 1.0, block[2] + 1.0),
        )
        .inflate(Vec3::new(32.0, 10.0, 32.0));
        for m in self.mobs.iter_mut() {
            if bindings.get(&m.id).copied() == conn && m.bounds().intersects(area) {
                horizon_client::mobs::ai::forgive_dead_player(m);
            }
        }
    }
    /// Animal.spawnChildFromBreeding
    fn breed(
        &mut self,
        parent: u32,
        partner: u32,
        llama: Option<Box<horizon_client::mobs::llama::Offspring>>,
    ) {
        let Some(p) = self.mobs.iter().find(|m| m.id == parent).cloned() else {
            return;
        };
        // BabyEntitySpawnEvent: not for an egg (turtle, frog, sniffer).
        if !matches!(
            p.species(),
            Species::Turtle | Species::Frog | Species::Sniffer
        ) && !horizon_client::mod_api::mods().is_empty()
        {
            let mule = matches!(
                (
                    p.species(),
                    self.mobs
                        .iter()
                        .find(|m| m.id == partner)
                        .map(|m| m.species())
                ),
                (Species::Horse, Some(Species::Donkey)) | (Species::Donkey, Some(Species::Horse))
            );
            let kind = if mule {
                "minecraft:mule".to_owned()
            } else {
                super::native_hooks::mob_kind(&p)
            };
            let conn = self
                .love_cause
                .get(&parent)
                .or(self.love_cause.get(&partner))
                .copied();
            let player = conn.and_then(|c| self.killer_uuid(c)).unwrap_or(0);
            let mut cancel = false;
            self.native_baby_entity_spawn(parent, partner, player, &kind, &mut cancel);
            if cancel {
                breeding_credit(&mut self.love_cause, parent, partner);
                if let Some(q) = self.mobs.iter_mut().find(|m| m.id == partner) {
                    q.extra.age = 6000;
                    q.extra.in_love = 0;
                }
                return;
            }
        }
        // Animal.finalizeSpawnChildFromBreeding: the player who fed the parent,
        // or else the one who fed the partner.
        if let Some(cause) = breeding_credit(&mut self.love_cause, parent, partner) {
            let parent = self.mob_entity(&p);
            let partner_view = self
                .mobs
                .iter()
                .find(|m| m.id == partner)
                .map(|m| self.mob_entity(m))
                .unwrap_or_else(|| parent.clone());
            let mut child = parent.clone();
            child.baby = true;
            // Not born yet: none of the parent's saved data is its own.
            child.nbt = None;
            // TurtleBreedGoal.breed triggers it without a child.
            let turtle = matches!(p.species(), Species::Turtle | Species::Frog);
            if matches!(
                (
                    p.species(),
                    self.mobs
                        .iter()
                        .find(|m| m.id == partner)
                        .map(|m| m.species())
                ),
                (Species::Horse, Some(Species::Donkey)) | (Species::Donkey, Some(Species::Horse))
            ) {
                child.kind = "minecraft:mule".into();
            }
            let who = cause.map_or(
                super::commands::Target::Player,
                super::commands::Target::Guest,
            );
            self.for_player(who, |g| {
                g.award_custom_stat("animals_bred", 1);
                g.trigger_bred(&parent, &partner_view, (!turtle).then_some(&child));
            });
        }
        if let Some(q) = self.mobs.iter_mut().find(|m| m.id == partner) {
            q.extra.age = 6000;
            q.extra.in_love = 0;
        }
        let q = self.mobs.iter().find(|m| m.id == partner).cloned();
        // Animal.finalizeSpawnChildFromBreeding: entity event 18 for the parent.
        self.mob_event_particles(parent, 18);
        // Sniffer.spawnChildFromBreeding: an egg instead of a baby.
        if p.species() == Species::Sniffer {
            self.lay_sniffer_egg(&p);
            return;
        }
        // TurtleBreedGoal.breed: the goal gave the mother her egg; only the
        // experience is left, and only with mob loot on.
        if matches!(p.species(), Species::Turtle | Species::Frog) {
            // One `new ExperienceOrb` of 1 to 7 from the mother's random (not
            // split up as `ExperienceOrb.award` does).
            if self.ui.rules.bool("mob_drops")
                && let Some(m) = self.mobs.iter_mut().find(|m| m.id == parent)
            {
                let xp = 1 + m.random.next_int_bound(7).unwrap_or(0) as u32;
                self.add_orb([p.position.x, p.position.y, p.position.z], xp);
            }
            return;
        }
        // (Mooshroom draws its own coin in getOffspringVariant.)
        let coin = p.species() != Species::Mooshroom
            && self.world.random.next_int_bound(2).unwrap_or(0) == 0;
        // Axolotl.getBreedOffspring: useRareVariant (one in 1200) makes it blue,
        // else either parent's variant. The draws come from the mother's random.
        let axolotl = if p.species() == Species::Axolotl {
            let other = q.as_ref().map_or(p.extra.variant, |q| q.extra.variant);
            self.mobs.iter_mut().find(|m| m.id == parent).map(|m| {
                if m.random.next_int_bound(1200).unwrap_or(1) == 0 {
                    // Axolotl.Variant.getRareSpawnVariant: the one blue, still a draw.
                    let _ = m.random.next_int_bound(1);
                    4
                } else if m.random.next_boolean() {
                    p.extra.variant
                } else {
                    other
                }
            })
        } else {
            None
        };
        // MushroomCow.getOffspringVariant, from the mother's random: the other
        // colour 1 time in 1024 when both parents share one, otherwise either
        // parent's colour by a coin flip.
        let mooshroom = if p.species() == Species::Mooshroom {
            let mate = q.as_ref().map_or(p.extra.variant, |q| q.extra.variant);
            let own = p.extra.variant;
            self.mobs.iter_mut().find(|m| m.id == parent).map(|m| {
                if own == mate && m.random.next_int_bound(1024).unwrap_or(1) == 0 {
                    if own == 1 { 0 } else { 1 }
                } else if m.random.next_boolean() {
                    own
                } else {
                    mate
                }
            })
        } else {
            None
        };
        // A horse and a donkey have a mule.
        let kind = match (p.species(), q.as_ref().map(|q| q.species())) {
            (Species::Horse, Some(Species::Donkey)) | (Species::Donkey, Some(Species::Horse)) => {
                "minecraft:mule".to_owned()
            }
            _ => p.kind.id.clone(),
        };
        if let Some(id) =
            self.spawn_mob_raw(&kind, [p.position.x, p.position.y, p.position.z], true)
            && let Some(child) = self.mobs.iter_mut().find(|m| m.id == id)
        {
            child.extra.age = -24000;
            // No Mob.finalizeSpawn either: no follow range bonus, right-handed.
            spawning::bred_offspring(child);
            // getBreedOffspring creates the baby without finalizeSpawn: it keeps
            // the default (`classic`) sound variant.
            child.extra.sound_variant = None;
            // Fox.getBreedOffspring runs no finalizeSpawn: nothing in the
            // baby fox's mouth.
            if p.species() == Species::Fox {
                child.extra.main_hand = None;
            }
            // Fox.FoxBreedGoal.breed: the baby trusts the players who fed its
            // parents into love.
            if p.species() == Species::Fox {
                horizon_client::mobs::fox::trust_breeders(
                    child,
                    p.extra.love_cause,
                    q.as_ref().and_then(|q| q.extra.love_cause),
                );
            }
            // Goat.getBreedOffspring: GoatAi.initMemories, then the baby
            // screams if the parent a coin toss picks does, or on a 2% roll.
            if p.species() == Species::Goat {
                child.extra.lost_horns = [false; 2];
                horizon_client::mobs::finalize::goat_init_memories(child, &mut self.world.random);
                let pick_parent = self.world.random.next_boolean();
                let parent_screams = if pick_parent {
                    p.extra.screaming
                } else {
                    q.as_ref().unwrap_or(&p).extra.screaming
                };
                child.extra.screaming = parent_screams || self.world.random.next_double() < 0.02;
            }
            // A llama's baby was drawn in the parent's tick, with the
            // experience.
            if let Some(o) = llama {
                horizon_client::mobs::llama::apply_offspring(child, &o);
                return;
            }
            if horizon_client::mobs::equine::is_equine(p.species()) {
                let other = q.clone().unwrap_or_else(|| p.clone());
                let mut r = self.world.random.clone();
                horizon_client::mobs::equine::inherit(child, &p, &other, &mut r);
                if horizon_client::mobs::llama::is_llama(p.species()) {
                    horizon_client::mobs::llama::offspring(child, &p, &other, &mut r);
                }
                // Horse.getBreedOffspring: coat from either parent or new.
                if p.species() == Species::Horse && other.species() == Species::Horse {
                    let c = r.next_int_bound(9).unwrap_or(0);
                    child.extra.variant = if c < 4 {
                        p.extra.variant
                    } else if c < 8 {
                        other.extra.variant
                    } else {
                        r.next_int_bound(7).unwrap_or(0) as u8
                    };
                    let mk = r.next_int_bound(5).unwrap_or(0);
                    child.extra.variant2 = if mk < 2 {
                        p.extra.variant2
                    } else if mk < 4 {
                        other.extra.variant2
                    } else {
                        r.next_int_bound(5).unwrap_or(0) as u8
                    };
                }
                self.world.random = r;
                self.breeding_orb(parent, [p.position.x, p.position.y, p.position.z]);
                return;
            }
            if p.species() == Species::Cat {
                // Cat.getBreedOffspring.
                let other = q.as_ref().unwrap_or(&p);
                child.extra.variant = if coin {
                    p.extra.variant
                } else {
                    other.extra.variant
                };
                if p.extra.tame {
                    child.extra.tame = true;
                    child.extra.owner = p.extra.owner;
                    // The mix needs all of `self`, so it waits until `child`'s
                    // borrow of the mob list is over.
                    let mixed = dye_mix(
                        world_recipes!(self),
                        &mut self.world.random,
                        p.extra.color,
                        other.extra.color,
                    );
                    if let Some(c) = self.mobs.iter_mut().find(|m| m.id == id) {
                        c.extra.color = mixed;
                    }
                }
                self.breeding_orb(parent, [p.position.x, p.position.y, p.position.z]);
                return;
            }
            if p.species() == Species::Wolf {
                // Wolf.getBreedOffspring: a parent's variant; tame parents
                // give a tame baby with their collar colours mixed.
                let other = q.as_ref().unwrap_or(&p);
                child.extra.variant = if coin {
                    p.extra.variant
                } else {
                    other.extra.variant
                };
                if p.extra.tame {
                    horizon_client::mobs::tame::set_tame(child, true);
                    child.extra.owner = p.extra.owner;
                    // The mix needs all of `self`, so it waits until `child`'s
                    // borrow of the mob list is over.
                    let mixed = dye_mix(
                        world_recipes!(self),
                        &mut self.world.random,
                        p.extra.color,
                        other.extra.color,
                    );
                    if let Some(c) = self.mobs.iter_mut().find(|m| m.id == id) {
                        c.extra.color = mixed;
                    }
                }
                self.breeding_orb(parent, [p.position.x, p.position.y, p.position.z]);
                return;
            }
            // Panda.getBreedOffspring: genes from both parents.
            if p.species() == Species::Panda {
                horizon_client::mobs::panda::inherit(child, &p, q.as_ref().unwrap_or(&p));
                self.breeding_orb(parent, [p.position.x, p.position.y, p.position.z]);
                return;
            }
            // Axolotl.getBreedOffspring: the drawn variant, and the baby is persistent.
            if let Some(variant) = axolotl {
                child.extra.variant = variant;
                child.persistent = true;
                self.breeding_orb(parent, [p.position.x, p.position.y, p.position.z]);
                return;
            }
            // Nautilus.getBreedOffspring: a tame parent's baby is tame too.
            if p.species() == Species::Nautilus && p.extra.tame {
                child.extra.tame = true;
                child.extra.owner = p.extra.owner;
                child.persistent = true;
            }
            // Offspring inherit a parent's variant (pigs/cows/chickens) or mixed wool.
            child.extra.variant = p.extra.variant;
            // Fox, Pig, Cow and Chicken.getBreedOffspring: either parent's variant.
            if matches!(
                p.species(),
                Species::Fox | Species::Pig | Species::Cow | Species::Chicken
            ) && !coin
            {
                child.extra.variant = q.as_ref().map_or(p.extra.variant, |q| q.extra.variant);
            }
            if let Some(variant) = mooshroom {
                child.extra.variant = variant;
            }
            // Sheep.getBreedOffspring: the lamb's wool mixes both parents' dyes.
            if p.species() == Species::Sheep {
                let other = q.as_ref().unwrap_or(&p);
                let mixed = dye_mix(
                    world_recipes!(self),
                    &mut self.world.random,
                    p.extra.color,
                    other.extra.color,
                );
                if let Some(c) = self.mobs.iter_mut().find(|m| m.id == id) {
                    c.extra.color = mixed;
                }
            }
        }
        self.breeding_orb(parent, [p.position.x, p.position.y, p.position.z]);
    }
    /// `Animal.finalizeSpawnChildFromBreeding`'s experience, from the breeding
    /// parent's own random, and only when the `mob_drops` game rule is on: one
    /// fresh orb of 1 to 7 (#11388).
    fn breeding_orb(&mut self, parent: u32, at: [f64; 3]) {
        if self.ui.rules.bool("mob_drops")
            && let Some(m) = self.mobs.iter_mut().find(|m| m.id == parent)
        {
            let xp = 1 + m.random.next_int_bound(7).unwrap_or(0) as u32;
            self.add_orb(at, xp);
        }
    }
    /// `MobEffectUtil.addEffectToPlayersAround` for an elder guardian:
    /// mining fatigue III for five minutes unless a stronger or longer one
    /// is on, with the curse sound and the elder's apparition.
    fn elder_curse(&mut self, at: [f64; 3], silent: bool) {
        let mut cursed = Vec::new();
        self.for_each_player(|g| {
            if g.elder_curse_player(at, silent) {
                cursed.push(g.lan_acting);
            }
        });
        // A guest's client shows the apparition and plays the curse itself
        // (ClientboundGameEventPacket.GUARDIAN_ELDER_EFFECT), the sound only
        // for 1: `isSilent() ? 0.0F : 1.0F`.
        if let Some(host) = self.lan.as_ref() {
            for conn in cursed.into_iter().flatten() {
                host.hub.send_to(
                    conn,
                    &horizon_protocol::packets::play::Clientbound::GameEvent(
                        horizon_protocol::packets::play::GameEvent {
                            event: horizon_client::lan::hub::game_event::GUARDIAN_ELDER_EFFECT,
                            param: crate::mobs::guardian::elder_curse_param(silent),
                        },
                    ),
                );
            }
        }
    }
    /// The curse on the player in place; whether it took.
    fn elder_curse_player(&mut self, at: [f64; 3], silent: bool) -> bool {
        if self.creative || self.spectator() {
            return false;
        }
        let Some(p) = self.player.as_ref() else {
            return false;
        };
        let d = p.position.subtract(Vec3::new(at[0], at[1], at[2]));
        if d.x * d.x + d.y * d.y + d.z * d.z >= 2500.0 || p.health.dead_or_dying() {
            return false;
        }
        // The player's effects are kept by their bare name. A curse as strong
        // that won't end within the next minute stays (endsWithin(1199)).
        let active = self.potions.effects.get("mining_fatigue");
        if active.is_some_and(|e| e.amplifier >= 2 && !e.ends_within(1199)) {
            return false;
        }
        let at = [p.position.x, p.position.y, p.position.z];
        self.give_player_effect("minecraft:mining_fatigue", 6000, 2);
        if self.lan_acting.is_none() {
            // ClientPacketListener.handleGameEvent: the apparition always, the
            // curse sound unless the elder is silent.
            if crate::mobs::guardian::elder_curse_param(silent) == 1.0 {
                self.audio
                    .play("minecraft:entity.elder_guardian.curse", Some(at), 1.0, 1.0);
            }
            self.creatures.elder_apparition = super::mob_blocks::APPARITION_TICKS;
        }
        true
    }
    /// Damage of a given type to the player from a point.
    pub(super) fn mob_hits_player_kind(&mut self, amount: f32, from: [f64; 3], kind: &str) {
        self.mob_hits_player(amount, from, kind);
    }
    /// Damage whose causing mob is known (`cause`, or none): not guessed
    /// from the mob nearest to `from`.
    pub(super) fn mob_hits_player_caused(
        &mut self,
        amount: f32,
        from: [f64; 3],
        kind: &str,
        cause: Option<u32>,
    ) {
        self.hurt_player_from(amount, Some(from), kind, Some(cause));
    }
    pub(super) fn mob_hits_player(
        &mut self,
        amount: f32,
        from: [f64; 3],
        damage_type: &str,
    ) -> bool {
        self.hurt_player_from(amount, Some(from), damage_type, None)
    }
    /// `hurtServer` on the player. `from` is the source's position (none: no
    /// direction), `known`: the causing mob when it is given (`/damage`),
    /// else the melee attacker is the mob nearest to `from`.
    pub(super) fn hurt_player_from(
        &mut self,
        amount: f32,
        from: Option<[f64; 3]>,
        damage_type: &str,
        known: Option<Option<u32>>,
    ) -> bool {
        let post_attack_if_alive = std::mem::take(&mut self.combat.post_attack_if_alive);
        let rocket = std::mem::take(&mut self.combat.rocket_hit);
        let melee = self.combat.melee_mob.take();
        // ServerPlayer.canHarmPlayer refused another player's projectile.
        if self.creative || self.combat.refuse_player_hit {
            self.combat.forget_noted(false);
            return false;
        }
        // A projectile entity dealt it: `#is_projectile`, or a firework
        // rocket (its `fireworks` type is not in the tag).
        let projectile = rocket
            || world_stations!(self)
                .enchantments
                .damage_tags
                .get("minecraft:is_projectile")
                .is_some_and(|t| t.contains(damage_type));
        let blocking = self.player_blocking(from, damage_type, false);
        let raised = blocking.is_some();
        // Melee: the attacker stands at `from` (another player's hit has no mob behind it).
        let attacker = match known {
            Some(known) => known,
            None if melee.is_some() => melee,
            None => from
                .filter(|_| !projectile && damage_type != "minecraft:player_attack")
                .and_then(|from| {
                    self.mobs
                        .iter()
                        .filter(|m| m.alive())
                        .min_by(|a, b| {
                            let d = |m: &horizon_client::mobs::Mob| {
                                (m.position.x - from[0]).powi(2) + (m.position.z - from[2]).powi(2)
                            };
                            d(a).total_cmp(&d(b))
                        })
                        .map(|m| m.id)
                }),
        };
        // The causing entity for the death message.
        let owner = self.combat.projectile_owner.take();
        let _attacker = self.knockback_scope(
            attacker
                .or(owner)
                .map_or_else(horizon_client::health::knockback_attacker, |id| id as i32),
        );
        if let Some(a) = attacker.or(owner) {
            self.note_attacker(damage_type, a);
        }
        if let Some((id, name)) = self.combat.projectile_attacker.take() {
            self.note_named_attacker(damage_type, id, name);
        }
        // The projectile is the direct entity: named when it has no owner.
        if let Some(name) = self.combat.projectile_direct.take() {
            self.note_direct_entity(damage_type, name);
        }
        // DamageSource.scalesWithDifficulty: a mob (or its projectile's
        // owner) caused the hit, not a player.
        let living_cause = attacker.or(owner).is_some();
        // The native damage hook's causing entity: the one given, or the
        // owner of the projectile that hit (`projectile_owner` outlives a
        // projectile's other hits), not the guessed melee attacker.
        let owner = owner.filter(|_| projectile);
        let causing = known.flatten().or(owner).map_or(0, |id| id as i32);
        let mut outcome = horizon_client::health::BlockOutcome::default();
        // The hit's own push, held back until after the shield's (`hurtServer`
        // runs `applyItemBlocking` before `dealDefaultKnockback`).
        let mut default_push: Option<([f64; 2], HurtSource<'_>)> = None;
        let events_before = self.player.as_ref().map_or(0, |p| p.events.len());
        let mut resolved_damage = None;
        let damage_outcome = if let Some(id) = self.health_resources.damage.id(damage_type) {
            // Every source that reaches the armor boundary wears it first:
            // `getDamageAfterArmorAbsorb` calls `hurtArmor` before it reads
            // the armor value, so a piece that breaks on this hit takes no
            // part in the reduction of the hit that broke it (#11696). The
            // hit's own advancement keeps the helmet phase's reported amount.
            let damages_helmet = self
                .health_resources
                .damage
                .in_tag(id, "minecraft:damages_helmet");
            self.hurt_player_equipment(id, amount, blocking, living_cause, causing)
                .map(|(outcome, actual)| {
                    if damages_helmet {
                        resolved_damage = Some(actual);
                    }
                    outcome
                })
        } else {
            Ok(Default::default())
        };
        if let Ok(o) = damage_outcome
            && let Some(player) = self.player.as_mut()
            && {
                outcome = o;
                o.hurt
            }
        {
            // LivingEntity.hurtServer's dealDefaultKnockback: not inside the
            // hurt cooldown (a stronger hit there only adds its difference).
            if o.knockback {
                // No source position: a random direction.
                let (xd, zd) = from.map_or((0.0, 0.0), |from| {
                    (from[0] - player.position.x, from[2] - player.position.z)
                });
                // dealDefaultKnockback's `indicateDamage`, unless blocked.
                if o.blocked <= 0.0 {
                    player.health.indicate_damage(xd, zd, player.yaw);
                }
                let source = HurtSource {
                    damage_type,
                    causing,
                    direct: 0,
                };
                default_push = Some(([xd, zd], source));
            }
            // The thorns hurt the entity that caused the hit, if there is one.
            // Thorns' own hit is no attack, so nothing answers it
            // (`doPostAttackEffects` runs from the attacks, not `hurtServer`).
            let killed = self.player.as_ref().is_some_and(|p| {
                super::wither::killed(true, p.health.dead_or_dying(), p.health.totem_used)
            });
            if let Some(a) = attacker.or(owner)
                && !(post_attack_if_alive && killed)
                && damage_type != "minecraft:thorns"
            {
                self.thorns(a);
            }
        }
        // LivingEntity.playSecondaryHurtSound: a thorns hit that landed whole
        // sounds where the player stands, for everyone around.
        if let Some(p) = self.player.as_ref()
            && p.events.get(events_before..).is_some_and(|new| {
                new.iter().any(|e| {
                    matches!(
                        e,
                        horizon_client::motion::MotionEvent::Health(HealthEvent::Phase(
                            "thorns-sound"
                        ))
                    )
                })
            })
        {
            let at = [p.position.x, p.position.y, p.position.z];
            self.audio
                .play("minecraft:enchant.thorns.hit", Some(at), 1.0, 1.0);
        }
        // LivingEntity.hurtServer: ENTITY_HURT_PLAYER, unless it returned
        // early (invulnerable, or inside the hurt cooldown).
        // A hit a native mod canceled records no damage (hurtServer never ran).
        self.combat.forget_noted(outcome.canceled);
        if outcome.hurt || outcome.sound {
            let source = attacker
                .or(owner)
                .and_then(|id| self.mob_index.get(&self.mobs, id))
                .map(|m| self.mob_entity(m));
            self.trigger_entity_hurt_player(&horizon_client::advancements::DamageView {
                kind: damage_type.to_owned(),
                // Arrows and the like name no direct entity here; a
                // rocket's burst names the rocket.
                direct: if rocket {
                    Some(horizon_client::advancements::EntityView {
                        kind: "minecraft:firework_rocket".to_owned(),
                        position: from.unwrap_or_default(),
                        ..Default::default()
                    })
                } else {
                    source.clone().filter(|_| !projectile)
                },
                is_direct: !projectile,
                source,
                original: amount,
                actual: resolved_damage.unwrap_or(amount - outcome.blocked),
                blocked: outcome.blocked > 0.0,
            });
        }
        if raised {
            let source = HurtSource {
                damage_type,
                causing,
                direct: 0,
            };
            self.after_block(outcome, (attacker, projectile), source);
        }
        // `LivingEntity.dealDefaultKnockback`: after the item's block push, so
        // a half-blocked hit pushes the defender back the way vanilla does.
        if let Some(([xd, zd], source)) = default_push {
            self.knock_back_player(f64::from(0.4_f32), xd, zd, source);
            if let Some(p) = self.player.as_mut()
                && let Some(r) = p.hurt_log.last_mut()
            {
                r.knockback = Some([p.velocity.x, p.velocity.y, p.velocity.z]);
            }
        }
        outcome.hurt
    }
    /// `LivingEntity.knockback` on the player: pushed `power` away from the
    /// direction (`xd`, `zd`), less its knockback resistance, and up only from
    /// the ground.
    /// The native knockback hook runs before resistance is applied, with
    /// `source`, the hit's.
    pub(super) fn knock_back_player(
        &mut self,
        power: f64,
        xd: f64,
        zd: f64,
        source: HurtSource<'_>,
    ) {
        let credit = match self.player_hurt_by() {
            Some((Who::Mob(id), _)) => id as i32,
            _ => 0,
        };
        let _attacker = self.knockback_scope(match horizon_client::health::knockback_attacker() {
            0 => credit,
            attacker => attacker,
        });
        let legacy = self.legacy_rules();
        if let Some(p) = self.player.as_mut() {
            knock_back(p, &mut self.world.random, power, (xd, zd), source, legacy);
        }
    }
    /// A status effect from a mob, arrow or splash potion (`addEffect`).
    pub(super) fn give_player_effect(&mut self, effect: &'static str, ticks: i32, amplifier: i32) {
        self.give_mob_effect(effect, ticks, amplifier, false);
    }
    /// A mob's effect on the player. Creative and spectator players shrug off
    /// the damaging effects here, but the nautilus's ambient Breath is plain
    /// `addEffect` and lands in every game mode.
    fn give_mob_effect(&mut self, effect: &'static str, ticks: i32, amplifier: i32, ambient: bool) {
        let immune = self.creative || self.spectator();
        if let Some(instance) = mob_effect_instance(effect, ticks, amplifier, ambient, immune) {
            self.potions.effects.add(instance);
        }
    }
    /// `WitherRoseBlock.entityInside` for the player: on a server level that
    /// isn't peaceful, `getBeeInteractionEffect` (Wither, 40 ticks) unless the
    /// player is invulnerable to wither damage (creative, spectator). A
    /// server's player gets it from the server.
    pub(super) fn wither_rose_contact(&mut self) {
        if self.remote.is_some() || self.ui.difficulty == 0 {
            return;
        }
        self.give_player_effect("minecraft:wither", 40, 0);
    }
    pub(super) fn hurt_player_by(&mut self, damage_type: &str, amount: f32) {
        let mut canceled = false;
        if !self.creative
            && let Some(id) = self.health_resources.damage.id(damage_type)
            && let Ok((o, _)) = self.hurt_player_equipment(id, amount, None, false, 0)
        {
            canceled = o.canceled;
        }
        self.combat.forget_noted(canceled);
    }
    /// `Player.aiStep` touching arrows in its box grown by (1, 0.5, 1):
    /// `AbstractArrow.playerTouch`, then `Player.take`'s pickup sound at the
    /// arrow (`ClientPacketListener.handleTakeItemEntity`).
    pub(super) fn touch_arrows(&mut self) {
        use horizon_client::arrows::Touch;
        let Some(p) = self.player.as_ref() else {
            return;
        };
        if p.health.dead_or_dying() || self.spectator() {
            return;
        }
        let w = f64::from(p.dimensions.width) / 2.0;
        let reach = Aabb::new(
            Vec3::new(p.position.x - w, p.position.y, p.position.z - w),
            Vec3::new(
                p.position.x + w,
                p.position.y + f64::from(p.dimensions.height),
                p.position.z + w,
            ),
        )
        .inflate(Vec3::new(1.0, 0.5, 1.0));
        // Player.hasInfiniteMaterials.
        let creative = self.creative;
        let mut projectiles = std::mem::take(&mut self.creatures.projectiles);
        let mut picked = Vec::new();
        projectiles.retain(|shot| {
            let Some(a) = shot.arrow.as_ref().filter(|a| !a.removed) else {
                return true;
            };
            let at = shot.position;
            let bounds = Aabb::new(
                Vec3::new(at.x - 0.25, at.y, at.z - 0.25),
                Vec3::new(at.x + 0.25, at.y + 0.5, at.z + 0.25),
            );
            if !bounds.intersects(reach) {
                return true;
            }
            let taken = match a.player_touch(creative) {
                Touch::Ignore => false,
                Touch::Take => true,
                // Inventory.add: a creative player takes what does not fit.
                Touch::Give(stack) => self.give(stack).is_none() || creative,
            };
            if taken {
                let r = &mut self.world.random;
                let pitch = (r.next_float() - r.next_float()) * 1.4 + 2.0;
                self.audio.play(
                    "minecraft:entity.item.pickup",
                    Some([at.x, at.y, at.z]),
                    0.2,
                    pitch,
                );
                picked.push(shot.clone());
            }
            !taken
        });
        self.creatures.projectiles = projectiles;
        for shot in picked {
            self.add_pickup(
                super::pickups::Taken::Arrow(Box::new(shot)),
                super::pickups::Collector::Player,
            );
        }
    }

    fn tick_projectiles(&mut self) -> Result<()> {
        if self.creatures.projectiles.is_empty() {
            self.flush_lan_explosions();
            return self.flush_wind_bursts();
        }
        let mut projectiles = std::mem::take(&mut self.creatures.projectiles);
        let ticking = self.entity_ticking_area();
        let mut hits = Vec::new();
        let mut deflections: Vec<u32> = Vec::new();
        let mut arrow_events = Vec::new();
        let mut shoot_events = Vec::new();
        let mut trails = Vec::new();
        let mut bubble_tops = Vec::new();
        let mut snow_contacts = Vec::new();
        let mut keep_flags = vec![true; projectiles.len()];
        let mut winds = std::mem::take(&mut self.creatures.pickable_projectile_targets);
        // The impacts the mods may cancel (`native_projectile_impact`), on a server.
        let hear = self.native_hears_impacts();
        let mut impacts: Vec<Impact> = Vec::new();
        {
            let targets = self.projectile_targets();
            let breezes: Vec<u32> = self
                .mobs
                .iter()
                .filter(|m| m.alive() && m.species() == Species::Breeze)
                .map(|m| m.id)
                .collect();
            // Drawn only when a breeze could deflect a projectile: an empty
            // level must not move the level random every tick.
            let jitter = if breezes.is_empty() {
                0.0
            } else {
                self.world.random.next_float()
            };
            let w = self.mob_world_base(&[]);
            let rain = self.arrow_rain();
            let scene = self.arrow_scene(&rain);
            // Mobs' projectiles also hit the guests (game/lan_mobs.rs).
            let guests = self.lan_projectile_targets();
            let has_guests = !guests.is_empty();
            let targets_guests = if has_guests {
                let mut targets_guests = targets.clone();
                targets_guests.extend(guests);
                targets_guests
            } else {
                Vec::new()
            };
            let border = Some(self.border.bounds());
            let dimension = self.dim.id();
            let pw = ProjWorld {
                w: &w,
                targets,
                border,
            };
            let pw_guests = ProjWorld {
                w: &w,
                targets: targets_guests,
                border,
            };
            // Every guest, not only the ones a projectile can hit: a dead or
            // spectating guest's pearl or arrow still resolves to them.
            let guest_entities = self.lan_guest_entities();
            // ThrownEnderpearl.tick: a dead player's pearl is discarded in
            // place of its tick while ender_pearls_vanish_on_death is on.
            let vanish = self.ui.rules.bool("ender_pearls_vanish_on_death");
            let host_dead = self.player_view().is_some_and(|p| !p.alive);
            let dead_guests: Vec<u32> = self.lan.as_ref().map_or_else(Vec::new, |h| {
                h.hub
                    .guests
                    .iter()
                    .filter(|g| g.dead)
                    .map(|g| g.conn)
                    .collect()
            });
            // `EntityTickList.forEach`: a projectile that joined during this
            // tick's entity pass (a mob's shot) first ticks on the next one; a
            // player's or a dispenser's, which join before the pass, tick now.
            let through = self.ticked_entities_through;
            for i in 0..projectiles.len() {
                if projectiles[i].id > through {
                    continue;
                }
                // ServerLevel.tick: losing an entity-ticking ticket must not
                // advance projectile flags or motion while its chunk unloads.
                let at = projectiles[i].position;
                if !ticking(at.x.floor() as i32, at.z.floor() as i32) {
                    projectiles[i].previous = at;
                    continue;
                }
                // A ghast's fireball or a wind charge is pickable, so a pearl, an
                // arrow or a trident can strike it (`Projectile.isPickable`,
                // `Projectile.onHit`).
                winds.clear();
                if matches!(projectiles[i].kind, ProjectileKind::EnderPearl)
                    || projectiles[i].arrow.is_some()
                {
                    pickable_projectile_hitboxes_into(&projectiles, &keep_flags, i, &mut winds);
                }
                let p = &mut projectiles[i];
                if vanish
                    && matches!(p.kind, ProjectileKind::EnderPearl)
                    && p.owner == Some(Who::Player)
                    && p.guest.map_or(host_dead, |c| dead_guests.contains(&c))
                {
                    keep_flags[i] = false;
                    continue;
                }
                // MC-310476: the mods may let a thrown projectile through the world border.
                let through = p.arrow.is_none() && !p.stuck && border_crossing(p, border) && {
                    let (at, v) = (p.position, p.velocity);
                    let (mut pos, mut velocity) = ([at.x, at.y, at.z], [v.x, v.y, v.z]);
                    let mut collide = true;
                    let kind = p.kind.id();
                    let (vel, c) = (&mut velocity, &mut collide);
                    Self::native_projectile_world_border(dimension, p.id, kind, &mut pos, vel, c);
                    if pos.iter().chain(&velocity).all(|v| v.is_finite())
                        && (pos != [at.x, at.y, at.z] || velocity != [v.x, v.y, v.z])
                    {
                        p.position = Vec3::new(pos[0], pos[1], pos[2]);
                        p.velocity = Vec3::new(velocity[0], velocity[1], velocity[2]);
                        p.needs_sync = true;
                    }
                    !collide
                };
                // The projectile as it was, to fly on if a mod cancels its impact.
                let before = (hear && !p.stuck).then(|| p.clone());
                let mob_shot = !matches!(p.owner, Some(Who::Player));
                // A player's arrows, snowballs and eggs meet every player; a
                // guest's flies from that guest (Projectile.getOwner), not
                // from the host, until it has left them.
                let pvp = !mob_shot && super::lan_pvp::meets_players(&p.kind);
                let shooter = p
                    .guest
                    .filter(|_| pvp)
                    .and_then(|c| guest_entities.iter().find(|(g, _)| *g == c))
                    .map(|&(_, id)| Who::Mob(id));
                if shooter.is_some() {
                    p.owner = shooter;
                }
                let use_guests = (mob_shot || pvp) && has_guests;
                let (keep, hit) = match (p.arrow.is_some(), use_guests, winds.is_empty()) {
                    (true, false, true) => horizon_client::arrows::tick(p, &scene, &pw.targets),
                    (true, true, true) => {
                        horizon_client::arrows::tick(p, &scene, &pw_guests.targets)
                    }
                    (true, with_guests, false) => {
                        let mut targets = if with_guests {
                            pw_guests.targets.clone()
                        } else {
                            pw.targets.clone()
                        };
                        targets.extend_from_slice(&winds);
                        horizon_client::arrows::tick(p, &scene, &targets)
                    }
                    // Through the border: this tick's ray does not stop on it.
                    (false, with_guests, true) if through => p.tick(&ProjWorld {
                        w: pw.w,
                        targets: if with_guests {
                            pw_guests.targets.clone()
                        } else {
                            pw.targets.clone()
                        },
                        border: None,
                    }),
                    (false, false, true) => p.tick(&pw),
                    (false, true, true) => p.tick(&pw_guests),
                    (false, pvp_shot, false) => {
                        let mut targets = if pvp_shot {
                            pw_guests.targets.clone()
                        } else {
                            pw.targets.clone()
                        };
                        targets.extend_from_slice(&winds);
                        p.tick(&ProjWorld {
                            w: if pvp_shot { pw_guests.w } else { pw.w },
                            targets,
                            border: border.filter(|_| !through),
                        })
                    }
                };
                if (p.is_thrown() && !matches!(p.kind, ProjectileKind::LlamaSpit))
                    || p.is_arrow() && p.arrow.as_ref().is_none_or(|a| !a.no_physics)
                {
                    for block in horizon_client::powder_snow::contacts(
                        p.previous,
                        p.position,
                        p.bounds(),
                        false,
                        |at| w.block_name(at.x, at.y, at.z) == "minecraft:powder_snow",
                    ) {
                        snow_contacts.push((block, p.position, p.velocity));
                    }
                }
                if shooter.is_some() {
                    p.owner = Some(Who::Player);
                }
                if let Some(at) = p.take_shoot_event() {
                    shoot_events.push((p.owner, p.guest, [at.x, at.y, at.z]));
                }
                // AbstractHurtingProjectile.tick ends with createParticleTrail.
                if super::remote_projectiles::has_smoke_trail(&p.kind) {
                    trails.push(p.position);
                }
                // Projectile.onAboveBubbleColumn's particles, from the tick's
                // bubble column pushes (`Projectile::tick`).
                bubble_tops.extend(
                    p.bubble_surfaces
                        .drain(..)
                        .map(|[x, y, z]| BlockPos::new(x, y, z)),
                );
                let events_from = arrow_events.len();
                let ordinary_arrow = p.is_arrow();
                if let Some(a) = p.arrow.as_mut() {
                    arrow_events.extend(a.events.drain(..).map(|e| {
                        if ordinary_arrow && e == horizon_client::arrows::ArrowEvent::ShootEvent {
                            shoot_events.push((p.owner, p.guest, p.position.components()));
                        }
                        // Retain even the no-op marker: native impact ranges index this list.
                        (p.position, e)
                    }));
                }
                let wind = matches!(
                    p.kind,
                    ProjectileKind::WindCharge | ProjectileKind::BreezeWindCharge
                );
                // Entity.deflection: a breeze turns every projectile but wind charges away.
                if let Hit::Entity {
                    who: Who::Mob(id), ..
                } = hit
                    && !wind
                    && breezes.contains(&id)
                {
                    super::wind::reverse(p, jitter);
                    if p.arrow.is_some() {
                        horizon_client::arrows::finish_entity_hit(p, &scene, EntityHit::Deflected);
                        let a = p.arrow.as_mut().expect("an arrow");
                        arrow_events.extend(a.events.drain(..).map(|e| {
                            if ordinary_arrow && e == horizon_client::arrows::ArrowEvent::ShootEvent
                            {
                                shoot_events.push((p.owner, p.guest, p.position.components()));
                            }
                            (p.position, e)
                        }));
                    }
                    deflections.push(id);
                    continue;
                }
                // AbstractWindCharge.tick: out of the world's top it bursts.
                if wind && keep && p.position.y > 350.0 {
                    hits.push((
                        p.clone(),
                        Hit::Block {
                            pos: [0; 3],
                            at: p.position,
                            face: [0; 3],
                        },
                    ));
                    keep_flags[i] = false;
                    continue;
                }
                if hit != Hit::None {
                    if let Some(before) = before {
                        impacts.push(Impact {
                            index: i,
                            before,
                            hit,
                            hit_slot: hits.len(),
                            events: events_from..arrow_events.len(),
                        });
                    }
                    hits.push((p.clone(), hit));
                }
                keep_flags[i] = keep;
                // Projectile.onHit: the redirectable projectile it hit (a ghast's
                // fireball or a wind charge) turns before its own tick.
                if let Hit::Entity {
                    who: Who::Mob(id), ..
                } = hit
                    && let Some(j) = pickable_projectile_hit(id)
                {
                    let (owner, owner_uuid, guest) = (p.owner, p.owner_uuid, p.guest);
                    let look = self.owner_look(owner, guest);
                    let wind = &mut projectiles[j];
                    super::deflect::aim_deflect(wind, look, owner, owner_uuid, guest);
                }
            }
        }
        self.creatures.pickable_projectile_targets = winds;
        for (block, position, movement) in snow_contacts {
            self.powder_snow_particle(block, position, movement);
        }
        let listed = keep_flags.clone();
        let mut idx = 0;
        projectiles.retain(|_| {
            let k = keep_flags[idx];
            idx += 1;
            k
        });
        if !impacts.is_empty() {
            self.native_settle_impacts(
                &mut projectiles,
                &listed,
                impacts,
                &mut hits,
                &mut arrow_events,
            );
        }
        for (owner, guest, at) in shoot_events {
            self.projectile_shoot_event(owner, guest, at);
        }
        self.smoke_trails(trails);
        self.dragon_breath_trails();
        for pos in bubble_tops {
            self.bubble_column_particles(pos);
        }
        for id in deflections {
            if let Some(index) = self.mobs.iter().position(|m| m.id == id) {
                self.play_mob_bound_sound(
                    index,
                    "minecraft:entity.breeze.deflect",
                    5,
                    1.0,
                    1.0,
                    false,
                );
            }
        }
        for (at, event) in arrow_events {
            self.arrow_event(at, event);
        }
        self.creatures.projectiles.append(&mut projectiles);
        for (p, hit) in hits {
            // A player's arrow on another player lands as the victim's hurt.
            if let Some(victim) = self.projectile_pvp_victim(&p, hit) {
                self.projectile_hits_player(&p, hit, victim)?;
                continue;
            }
            // A guest's projectile lands as theirs: a pearl takes them, what
            // it hurts answers to them. One hitting the host (`Who::Player`
            // here) stays the host's hit.
            match p.guest {
                Some(conn)
                    if !matches!(
                        hit,
                        Hit::Entity {
                            who: Who::Player,
                            ..
                        }
                    ) =>
                {
                    self.guest_projectile_hit(conn, &p, hit)?;
                }
                _ => self.projectile_hit(&p, hit)?,
            }
        }
        self.flush_lan_explosions();
        self.flush_wind_bursts()
    }
    /// `LivingEntity.hurtServer`'s `setLastHurtByPlayer` for a projectile a
    /// player owns (a fireball they turned back): the kill is theirs, so a
    /// guest's (the host on its server thread too) reaches their statistics
    /// and `player_killed_entity`, as the arrow's hit does.
    fn player_hurt_mob(&mut self, p: &Projectile, id: u32) {
        if p.owner == Some(Who::Player) {
            self.last_hurt_by_player.insert(id, self.lan_acting);
        }
    }
    /// `LivingEntity.hurtServer` for a snowball, egg or pearl (`kind`) that hurt the mob `id`,
    /// 0 damage included: the hit is its owner's, and a player's fires PLAYER_HURT_ENTITY.
    fn thrown_hurt_mob(&mut self, p: &Projectile, id: u32, kind: &str, amount: f32) {
        self.player_hurt_mob(p, id);
        if p.owner == Some(Who::Player)
            && let Some(m) = self.mobs.iter().find(|m| m.id == id)
        {
            let victim = self.mob_entity(m);
            let direct = horizon_client::advancements::EntityView {
                kind: kind.into(),
                position: [p.position.x, p.position.y, p.position.z],
                ..Default::default()
            };
            self.player_hit(&victim, "minecraft:thrown", Some(direct), amount);
        }
    }
    /// `projectile_hit` for a projectile the guest `conn` shot, run as them;
    /// as the host's when they have left.
    fn guest_projectile_hit(&mut self, conn: u32, p: &Projectile, hit: Hit) -> Result<()> {
        let Some(mut host) = self.lan.take() else {
            return self.projectile_hit(p, hit);
        };
        let result = self.as_guest(&mut host, conn, |g| g.projectile_hit(p, hit));
        // The mob hit now answers to the guest (LivingEntity.lastHurtByPlayer).
        if result.is_some()
            && let Hit::Entity {
                who: Who::Mob(id), ..
            } = hit
            && self.mobs.iter().any(|m| m.id == id)
        {
            host.mob_players.insert(id, conn);
        }
        self.lan = Some(host);
        match result {
            Some(r) => r,
            None => self.projectile_hit(p, hit),
        }
    }
    /// `EnchantmentHelper.doPostAttackEffects` after the player's shot `p`
    /// (its `direct` entity, a hit of `damage_type`) hurt the mob `who`: the
    /// victim's own post-attack enchantments answer the player (Thorns).
    pub(super) fn shot_post_attack(
        &mut self,
        p: &Projectile,
        who: Who,
        direct: &str,
        damage_type: &str,
    ) {
        if let Who::Mob(id) = who {
            self.mob_post_attack(id, p.owner, Some(direct), Some(damage_type));
        }
    }
    /// What an arrow or a thrown trident hits an entity of type `victim`
    /// for (`AbstractArrow.onHitEntity`, `ThrownTrident.onHitEntity`): a
    /// trident's 8 through `EnchantmentHelper.modifyDamage` of the trident
    /// (Impaling's `minecraft:damage` on `#sensitive_to_impaling`).
    fn shot_damage(&mut self, shot: &mut Projectile, victim: Option<&str>) -> f32 {
        if !matches!(shot.kind, ProjectileKind::Trident) {
            return arrow_damage(shot, &mut self.world.random) + shot.bonus.damage;
        }
        let Some(trident) = shot.arrow.as_ref().map(|a| a.pickup_item.clone()) else {
            return 8.0;
        };
        let ctx = horizon_client::enchant::EffectContext {
            this: victim,
            attacker: (shot.owner == Some(Who::Player)).then_some("minecraft:player"),
            direct_attacker: Some("minecraft:trident"),
            damage_type: Some("minecraft:trident"),
            items: Some(world_items!(self)),
            ..Default::default()
        };
        world_stations!(self).enchantments.modify_value(
            Some(&trident),
            "minecraft:damage",
            &ctx,
            &mut self.world.random,
            8.0,
        )
    }
    /// A projectile's hurt of `amount` on the boat or minecart `id`
    /// (`VehicleEntity.hurtServer`): even 0 shakes it (`Snowball`, `ThrownEgg`,
    /// `ThrownEnderpearl` `.onHitEntity`), 5 from a small fireball destroys it.
    fn hurt_vehicle(&mut self, p: &Projectile, id: u32, amount: f32) {
        let creative = p.owner == Some(Who::Player) && self.creative;
        if let Some(i) = self.boats.iter().position(|b| b.id == id) {
            self.hurt_boat(i, amount, creative);
        } else if let Some(i) = self.carts.iter().position(|c| c.id == id) {
            self.hurt_cart(i, amount, creative, false);
        }
    }
    /// `BellBlock.onProjectileHit`: a projectile that strikes the bell proper
    /// rings it, and a player's shot counts as the player's ring.
    fn bell_projectile_hit(
        &mut self,
        p: &ProjectileHitter,
        pos: [i32; 3],
        at: Vec3,
        face: [i32; 3],
    ) {
        let Some(face) = Direction::ALL.into_iter().find(|d| d.normal() == face) else {
            return;
        };
        let block = BlockPos::new(pos[0], pos[1], pos[2]);
        let rang = self
            .with_level(|b, level, ticks| b.bell_projectile_hit(level, ticks, block, face, at.y));
        if rang {
            let _ = self.process_level_events();
            if p.owner == Some(Who::Player) {
                self.award_custom_stat("bell_ring", 1);
            }
        }
    }
    /// `AmethystBlock.onProjectileHit`: a projectile that strikes an amethyst
    /// block rings `block.amethyst_block.chime` there, pitched by the level random.
    fn amethyst_projectile_hit(&mut self, pos: [i32; 3]) {
        let state = self.world.block(pos[0], pos[1], pos[2]);
        if &*self.res.facts().get(state).name != "minecraft:amethyst_block" {
            return;
        }
        let pitch = 0.5 + self.world.random.next_float() * 1.2;
        self.level_events
            .push(horizon_world::level::LevelEvent::Sound {
                pos: BlockPos::new(pos[0], pos[1], pos[2]),
                sound: "minecraft:block.amethyst_block.chime".into(),
                volume: 1.0,
                pitch,
            });
        let _ = self.process_level_events();
    }
    /// `onProjectileHit` of chorus flowers, decorated pots and speleothems
    /// (pointed dripstone, sulfur spikes): an impact projectile
    /// (`#impact_projectiles`) breaks the block when `Projectile.mayInteract`
    /// and `Projectile.mayBreak` allow it, which takes the
    /// `projectiles_can_break_blocks` rule and a shooter that may break blocks
    /// (`canBreakBlockInAdventureMode`: a resolved mob's arrow needs its
    /// actual projectile predicate and mob_griefing). Other owner/kind paths
    /// retain their existing policy. A pot is cracked first (flags 260).
    fn projectile_breaks_block(&mut self, p: &Projectile, pos: [i32; 3]) {
        use horizon_client::command::args::GameMode;
        let impact = matches!(
            p.kind,
            ProjectileKind::Arrow { .. }
                | ProjectileKind::Trident
                | ProjectileKind::Snowball
                | ProjectileKind::Thrown(_)
                | ProjectileKind::Egg
                | ProjectileKind::SmallFireball
                | ProjectileKind::LargeFireball { .. }
                | ProjectileKind::WitherSkull { .. }
                | ProjectileKind::WindCharge
                | ProjectileKind::BreezeWindCharge
        );
        use horizon_client::arrows::OwnerLookup;
        let shooter_may_break = match p.owner {
            Some(Who::Player) => self.mode != GameMode::Adventure,
            Some(Who::Mob(_)) => {
                matches!(p.kind, ProjectileKind::Arrow { .. })
                    && self.ui.rules.bool("mob_griefing")
                    && p.arrow
                        .as_ref()
                        .and_then(|a| a.can_break.as_ref())
                        .is_some_and(|predicate| {
                            super::game_mode::projectile_predicate_supported(predicate)
                                && self.adventure_predicate_at(predicate, pos)
                        })
            }
            None => !p.owner_uuid.is_some_and(|u| self.non_player_entity(u)),
        };
        if !impact || !shooter_may_break || !self.ui.rules.bool("projectiles_can_break_blocks") {
            return;
        }
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let at = BlockPos::new(pos[0], pos[1], pos[2]);
        match &*self.res.facts().get(state).name {
            "minecraft:chorus_flower"
                if matches!(
                    p.kind,
                    ProjectileKind::Arrow { .. } | ProjectileKind::Snowball
                ) =>
            {
                // `ChorusFlowerBlock.onProjectileHit` supplies the projectile,
                // even when it has no owner, as the direct block's loot entity.
                let _ = self.destroy_block_dropping_with_breaker(pos, true);
                return;
            }
            "minecraft:chorus_flower" => {}
            // SpeleothemBlock.onProjectileHit (pointed dripstone and sulfur spikes): a
            // thrown trident faster than 0.6 breaks it.
            "minecraft:pointed_dripstone" | "minecraft:sulfur_spike"
                if matches!(p.kind, ProjectileKind::Trident)
                    && p.arrow.as_ref().is_some_and(|a| a.hit_speed > 0.6) => {}
            "minecraft:decorated_pot" => {
                let registry = &self.pack.assets.registry;
                let Some(mut pot) = registry.state(state).cloned() else {
                    return;
                };
                pot.properties.insert("cracked".into(), "true".into());
                let Ok(cracked) = registry.resolve(&pot) else {
                    return;
                };
                self.with_level(|b, level, ticks| b.set_block(level, ticks, at, cracked, 260));
            }
            _ => return,
        }
        let _ = self.destroy_block_dropping(pos);
    }
    /// What an arrow's tick asked for: its particles and sounds.
    pub(super) fn arrow_event(&mut self, at: Vec3, event: horizon_client::arrows::ArrowEvent) {
        use horizon_client::arrows::{ArrowEvent, ArrowParticle, ArrowSound, BlockReaction};
        use horizon_client::particles::{ParticleOptions, ParticleType};
        let kind = |k| match k {
            ArrowParticle::Bubble => ParticleType::Bubble,
            ArrowParticle::Crit => ParticleType::Crit,
            ArrowParticle::Splash => ParticleType::Splash,
        };
        match event {
            ArrowEvent::Particle {
                kind: k,
                position,
                velocity,
            } => self.particles.add(
                &ParticleOptions::simple(kind(k)),
                [position.x, position.y, position.z],
                [velocity.x, velocity.y, velocity.z],
            ),
            // Arrow.tick: `broadcastEntityEvent(this, 0)`. The window and the
            // guests burst the particles themselves (`remote_arrow_puff`).
            ArrowEvent::PotionPuff { id, .. } => {
                self.lan_entity_event(id as i32, 0, [at.x, at.y, at.z]);
            }
            ArrowEvent::Sound {
                sound,
                volume,
                pitch,
            } => {
                let name = match sound {
                    ArrowSound::Splash => "minecraft:entity.generic.splash",
                    ArrowSound::HitGround => "minecraft:entity.arrow.hit",
                    ArrowSound::CrossbowHit => "minecraft:item.crossbow.hit",
                    ArrowSound::Extinguish => "minecraft:entity.generic.extinguish_fire",
                    ArrowSound::TridentHitGround => "minecraft:item.trident.hit_ground",
                    ArrowSound::TridentHit => "minecraft:item.trident.hit",
                };
                // Each one is the projectile's own `Entity.playSound`, on `getSoundSource()`:
                // NEUTRAL, which neither AbstractArrow nor ThrownTrident overrides. The
                // name-based guess would put `entity.arrow.*` and `item.*` on PLAYERS.
                self.audio
                    .play_on(6, name, Some([at.x, at.y, at.z]), volume, pitch);
            }
            ArrowEvent::BubbleColumnParticles { pos } => self.bubble_column_particles(pos),
            ArrowEvent::Block {
                pos,
                reaction,
                owner,
            } => match reaction {
                BlockReaction::PrimeTnt => {
                    // `TntBlock.onProjectileHit`: the arrow's owner lights it.
                    if self.tnt_prime_allowed(pos) {
                        self.spawn_primed_tnt(pos, false, owner, None);
                        self.with_level(|b, level, ticks| b.remove_block(level, ticks, pos, false));
                        let _ = self.process_level_events();
                    }
                }
                BlockReaction::Light(lit) => {
                    let _ = self.level_set([pos.x, pos.y, pos.z], lit);
                }
            },
            ArrowEvent::SplashEvent => {
                self.projectile_event("minecraft:splash", None, [at.x, at.y, at.z]);
            }
            // finish_arrow_hit drops the pickup, which it holds.
            ArrowEvent::ShootEvent | ArrowEvent::Drop => {}
        }
    }
    /// `Level.isRainingAt` for arrows.
    /// What a projectile can hit: the mobs, the player, the dragon's parts and
    /// the End crystals (under reserved ids, game/dragon.rs).
    /// The boats and minecarts a projectile can strike.
    pub(super) fn vehicle_targets(&self) -> Vec<(Who, Aabb)> {
        let mut targets: Vec<(Who, Aabb)> = self
            .boats
            .iter()
            .map(|b| (Who::Mob(b.id), b.bounds()))
            .collect();
        targets.extend(self.carts.iter().map(|c| (Who::Mob(c.id), c.bounds())));
        targets
    }
    pub(super) fn projectile_targets(&self) -> Vec<(Who, Aabb)> {
        let mut targets: Vec<(Who, Aabb)> = self
            .mobs
            .iter()
            .filter(|m| m.alive())
            .map(|m| (Who::Mob(m.id), m.bounds()))
            .collect();
        if let Some(p) = self.player_view()
            && p.alive
        {
            targets.push((Who::Player, p.bounds));
        }
        targets.extend(
            self.special_targets()
                .into_iter()
                .map(|(id, b)| (Who::Mob(id), b)),
        );
        // Boats and minecarts are pickable for every projectile
        // (`VehicleEntity.isPickable`), so every kind shares this one list.
        targets.extend(self.vehicle_targets());
        targets
    }
    pub(super) fn arrow_rain(&self) -> impl Fn(BlockPos) -> bool + '_ {
        move |p: BlockPos| self.raining_at(p)
    }
    /// The level as arrows see it.
    pub(super) fn arrow_scene<'s>(
        &'s self,
        rain: &'s dyn Fn(BlockPos) -> bool,
    ) -> horizon_client::arrows::ArrowScene<'s, 's> {
        horizon_client::arrows::ArrowScene {
            world: &self.world,
            registry: self.pack.assets.registry,
            facts: self.res.facts(),
            collision: self.collision,
            fluids: &self.motion.fluids,
            raining: rain,
            mob_griefing: self.ui.rules.bool("mob_griefing"),
            tnt_explodes: self.ui.rules.bool("tnt_explodes"),
            owners: self,
        }
    }
    /// Dispatch with the projectile's actual owner context, including a guest shooter.
    fn projectile_shoot_event(&mut self, owner: Option<Who>, guest: Option<u32>, at: [f64; 3]) {
        if let Some(conn) = guest.filter(|conn| self.lan_acting != Some(*conn)) {
            let dispatched = if let Some(mut host) = self.lan.take() {
                let result = self.as_guest(&mut host, conn, |g| {
                    g.projectile_event("minecraft:projectile_shoot", owner, at);
                });
                self.lan = Some(host);
                result.is_some()
            } else {
                false
            };
            // An owner who left cannot resolve to the host. The event
            // still occurs, with no resolved owner (Projectile.getOwner).
            if !dispatched {
                self.projectile_event("minecraft:projectile_shoot", None, at);
            }
        } else {
            self.projectile_event("minecraft:projectile_shoot", owner, at);
        }
    }
    /// The rest of the tick of an arrow that hit an entity
    /// (`arrows::finish_entity_hit`): its sounds and drop, and back among the
    /// projectiles if it flies on.
    fn finish_arrow_hit(&mut self, mut arrow: Projectile, outcome: EntityHit) {
        let keep = {
            let rain = self.arrow_rain();
            horizon_client::arrows::finish_entity_hit(&mut arrow, &self.arrow_scene(&rain), outcome)
        };
        let Some(a) = arrow.arrow.as_mut() else {
            return;
        };
        let (events, item) = (std::mem::take(&mut a.events), a.pickup_item.clone());
        for event in events {
            if event == horizon_client::arrows::ArrowEvent::Drop {
                self.spawn_at_location(arrow.position, f64::from(0.1f32), item.clone());
            } else if arrow.is_arrow() && event == horizon_client::arrows::ArrowEvent::ShootEvent {
                self.projectile_shoot_event(arrow.owner, arrow.guest, arrow.position.components());
            } else {
                self.arrow_event(arrow.position, event);
            }
        }
        if keep {
            self.add_projectile(arrow);
        }
    }
    /// `TargetBlock.onProjectileHit`: a target block takes its power from
    /// where the projectile landed and resets after 20 ticks for an arrow, 8
    /// for any other projectile. A player's shot counts as a target hit.
    fn target_block_hit(&mut self, p: &ProjectileHitter, pos: [i32; 3], at: Vec3, face: [i32; 3]) {
        let block = BlockPos::new(pos[0], pos[1], pos[2]);
        let arrow = p.arrow;
        let location = [at.x, at.y, at.z];
        let hit = self.with_level(|b, level, ticks| {
            b.target_projectile_hit(level, ticks, block, location, face, arrow)
        });
        if let Some(strength) = hit {
            let _ = self.process_level_events();
            self.native_target_hit(self.dim.id(), pos, strength);
            if p.owner == Some(Who::Player) && (p.guest.is_none() || p.guest == self.lan_acting) {
                self.award_custom_stat("target_hit", 1);
                let projectile = horizon_client::advancements::EntityView {
                    kind: p.kind.to_owned(),
                    position: [p.position.x, p.position.y, p.position.z],
                    ..Default::default()
                };
                self.trigger_target_hit(&projectile, strength);
            }
        }
    }
    /// `Projectile.onHitBlock`, which a fishing bobber's override calls before it
    /// rebounds: the struck block's `onProjectileHit` for the acting player's
    /// bobber at `position`, which hit
    /// `pos` on `face` at `at`. A bell rings, a target block is powered (8
    /// ticks: a bobber is no arrow), amethyst chimes and a big dripleaf tilts.
    /// The bobber is no `#impact_projectiles` and never burns, so the blocks it
    /// breaks or lights take no part.
    pub(super) fn fishing_hook_block_hit(
        &mut self,
        position: Vec3,
        pos: [i32; 3],
        at: Vec3,
        face: [i32; 3],
    ) {
        let hook = ProjectileHitter {
            owner: Some(Who::Player),
            guest: self.lan_acting,
            arrow: false,
            kind: "minecraft:fishing_bobber",
            position,
        };
        self.bell_projectile_hit(&hook, pos, at, face);
        self.amethyst_projectile_hit(pos);
        if self.with_level(|b, level, ticks| {
            b.big_dripleaf_projectile_hit(level, ticks, BlockPos::new(pos[0], pos[1], pos[2]))
        }) {
            let _ = self.process_level_events();
        }
        self.target_block_hit(&hook, pos, at, face);
    }
    pub(super) fn projectile_hit(&mut self, p: &Projectile, hit: Hit) -> Result<()> {
        let result = self.projectile_hit_inner(p, hit);
        // LivingEntity.actuallyHurt credits the shooter (a ServerPlayer) with the absorbed damage.
        self.settle_dealt_absorbed(p.owner == Some(Who::Player));
        result?;
        // The hit's hurt teleports an enderman or a shulker now (`hurtServer`).
        self.settle_hurt_teleports()
    }
    fn projectile_hit_inner(&mut self, p: &Projectile, hit: Hit) -> Result<()> {
        let _attacker = self.knockback_scope_for(p.owner);
        // Projectile.onHitBlock: BlockState.onProjectileHit, for every
        // projectile (a wind charge out of the world's top has no face).
        if let Hit::Block { pos, at, face } = hit
            && face != [0; 3]
        {
            self.bell_projectile_hit(&ProjectileHitter::of(p), pos, at, face);
            self.amethyst_projectile_hit(pos);
            if self.with_level(|b, level, ticks| {
                b.big_dripleaf_projectile_hit(level, ticks, BlockPos::new(pos[0], pos[1], pos[2]))
            }) {
                self.process_level_events()?;
            }
            self.projectile_breaks_block(p, pos);
        }
        self.combat.projectile_owner = match p.owner {
            Some(Who::Mob(id)) => Some(id),
            _ => None,
        };
        self.combat.projectile_direct = match hit {
            Hit::Entity {
                who: Who::Player, ..
            } => Some(self.target_name(super::commands::Target::Projectile(p.id))),
            _ => None,
        };
        let at = match hit {
            Hit::Entity { at, .. } | Hit::Block { at, .. } => at,
            Hit::None => return Ok(()),
        };
        // Projectile.onHitBlock: BlockState.onProjectileHit, for every
        // projectile (a wind charge out of the world's top has no face).
        if let Hit::Block { pos, face, .. } = hit
            && face != [0; 3]
        {
            self.target_block_hit(&ProjectileHitter::of(p), pos, at, face);
        }
        if let Hit::Entity {
            who: Who::Mob(id),
            at,
        } = hit
            && self.lan_projectile_hit(p, id, at)
        {
            return Ok(());
        }
        if matches!(
            p.kind,
            ProjectileKind::WindCharge | ProjectileKind::BreezeWindCharge
        ) {
            return self.wind_charge_hit(p, hit);
        }
        // A wither skull's hit on a part or crystal goes through
        // `wither_skull_hit` below, as on any entity, and then explodes.
        if let Hit::Entity {
            who: Who::Mob(id), ..
        } = hit
            && id >= super::dragon::SPECIAL
            && !matches!(
                p.kind,
                ProjectileKind::WitherSkull { .. } | ProjectileKind::DragonFireball
            )
        {
            let by_player = p.owner == Some(Who::Player);
            if p.arrow.is_some() {
                // AbstractArrow.onHitEntity: spent if the part or crystal took
                // it, else turned back (a perched dragon sets it alight first).
                let mut arrow = p.clone();
                let amount = self.shot_damage(&mut arrow, None);
                let outcome = match self.hit_special(
                    id,
                    amount,
                    by_player,
                    horizon_client::mobs::DamageKind::Arrow,
                    Some(&p.kind),
                ) {
                    super::dragon::Hurt::Landed { .. } => EntityHit::Hurt,
                    super::dragon::Hurt::Refused => EntityHit::Refused,
                    super::dragon::Hurt::Shrugged => {
                        if let Some(a) = arrow.arrow.as_mut() {
                            a.ignite_for_seconds(1.0);
                        }
                        EntityHit::Refused
                    }
                };
                self.finish_arrow_hit(arrow, outcome);
                return Ok(());
            }
            let amount = match p.kind {
                ProjectileKind::SmallFireball => 5.0,
                ProjectileKind::LargeFireball { .. } => 6.0,
                ProjectileKind::Thrown(t) => t.damage,
                _ => 0.0,
            };
            let kind = match p.kind {
                ProjectileKind::SmallFireball | ProjectileKind::LargeFireball { .. } => {
                    horizon_client::mobs::DamageKind::Fireball
                }
                _ => horizon_client::mobs::DamageKind::Thrown,
            };
            self.hit_special(id, amount, by_player, kind, Some(&p.kind));
            // ThrowableProjectile.onHit goes on after onHitEntity: a pearl
            // still takes its owner to where it broke (into the crystal's
            // blast), an egg still hatches, a snowball still breaks. A large
            // fireball still explodes (LargeFireball.onHit), whatever the
            // part or crystal made of its hit (both refuse fire), and the
            // blast takes the crystal.
            if !matches!(
                p.kind,
                ProjectileKind::EnderPearl
                    | ProjectileKind::Egg
                    | ProjectileKind::Snowball
                    | ProjectileKind::Thrown(_)
                    | ProjectileKind::LargeFireball { .. }
            ) {
                return Ok(());
            }
        }
        if p.arrow.is_some()
            && let Hit::Entity {
                who: Who::Mob(id), ..
            } = hit
            && let Some(i) = self.carts.iter().position(|c| c.id == id)
        {
            // AbstractArrow.onHitEntity on a minecart.
            let mut arrow = p.clone();
            let amount = self.shot_damage(&mut arrow, None);
            let burning = p.arrow.as_ref().is_some_and(|a| a.on_fire());
            let creative = p.owner == Some(Who::Player) && self.creative;
            self.arrow_hits_cart(i, amount, p.velocity, burning, creative);
            self.finish_arrow_hit(arrow, EntityHit::Hurt);
            return Ok(());
        }
        if p.arrow.is_some()
            && let Hit::Entity {
                who: Who::Mob(id), ..
            } = hit
            && let Some(i) = self.boats.iter().position(|b| b.id == id)
        {
            // AbstractArrow.onHitEntity on a boat: VehicleEntity.hurtServer
            // takes the arrow's damage, and the arrow is spent.
            let mut arrow = p.clone();
            let amount = self.shot_damage(&mut arrow, None);
            let creative = p.owner == Some(Who::Player) && self.creative;
            self.hurt_boat(i, amount, creative);
            self.finish_arrow_hit(arrow, EntityHit::Hurt);
            return Ok(());
        }
        let atp = [at.x, at.y, at.z];
        // Projectile.onHit: the landing is heard.
        self.projectile_event("minecraft:projectile_land", p.owner, atp);
        let source_kind = match p.kind {
            ProjectileKind::Arrow { .. } | ProjectileKind::Trident => DamageKind::Arrow,
            ProjectileKind::SmallFireball | ProjectileKind::LargeFireball { .. } => {
                DamageKind::Fireball
            }
            _ => DamageKind::Thrown,
        };
        let attacker_species = match p.owner {
            Some(Who::Mob(id)) => self.mobs.iter().find(|m| m.id == id).map(|m| m.species()),
            _ => None,
        };
        let source = Damage {
            kind: source_kind,
            attacker: p.owner,
            from: Some([p.previous.x, p.previous.y, p.previous.z]),
            attacker_species,
        };
        let aim = self.attacker_aim(source.attacker);
        match (&p.kind, hit) {
            // ThrownTrident.onHitEntity: 8 damage (Impaling on aquatic mobs),
            // dealt whether or not the hurt lands, then it bounces off.
            (ProjectileKind::Trident, Hit::Entity { who, .. }) => {
                let mut trident = p.clone();
                let victim = match who {
                    Who::Mob(id) => self
                        .mobs
                        .iter()
                        .find(|m| m.id == id)
                        .map(|m| m.kind.id.clone()),
                    Who::Player => Some("minecraft:player".to_owned()),
                };
                let amount = self.shot_damage(&mut trident, victim.as_deref());
                let hurt = match who {
                    Who::Player => self.mob_hits_player(
                        amount,
                        [p.previous.x, p.previous.y, p.previous.z],
                        "minecraft:trident",
                    ),
                    Who::Mob(id) => self.mobs.iter_mut().find(|m| m.id == id).is_some_and(|m| {
                        m.damage_projectile(amount, source, aim, [p.velocity.x, p.velocity.z])
                    }),
                };
                let outcome = if hurt {
                    self.shot_post_attack(p, who, "minecraft:trident", "minecraft:trident");
                    EntityHit::Hurt
                } else {
                    EntityHit::Refused
                };
                self.burning_arrow_primes(p, who);
                self.finish_arrow_hit(trident, outcome);
            }
            // AbstractArrow.onHitBlock plays its sound itself (ArrowEvent).
            (ProjectileKind::Trident, Hit::Block { .. }) => {}
            // AbstractArrow.onHitEntity on a ghast's fireball or a wind charge:
            // `hurtOrSimulate` fails (`AbstractHurtingProjectile.hurtServer` is
            // false; the projectile turned in `Projectile.onHit`, which already
            // ran), so the arrow takes `deflect(REVERSE, 0.2)` instead of vanishing.
            (
                ProjectileKind::Arrow { .. },
                Hit::Entity {
                    who: Who::Mob(id), ..
                },
            ) if pickable_projectile_hit(id).is_some() => {
                let mut arrow = p.clone();
                arrow_damage(&mut arrow, &mut self.world.random);
                self.finish_arrow_hit(arrow, EntityHit::Refused);
            }
            (ProjectileKind::Arrow { .. }, Hit::Entity { who, .. }) => {
                let mut arrow = p.clone();
                let amount = arrow_damage(&mut arrow, &mut self.world.random) + p.bonus.damage;
                let player_fire = self.player.as_ref().map(|pl| pl.fire_ticks);
                // AbstractArrow.onHitEntity saves the fire, ignites, then puts
                // it back when hurtOrSimulate fails and the target still takes
                // a projectile's side effects.
                let mob_fire = match who {
                    Who::Mob(id) => self.mobs.iter().find(|m| m.id == id).map(|m| m.fire_ticks),
                    Who::Player => None,
                };
                let mut dodged = false;
                // AbstractArrow.onHitEntity: a burning arrow sets its target alight.
                if p.arrow.as_ref().is_some_and(|a| a.on_fire()) {
                    self.handle_mob_events(vec![MobEvent::Ignite {
                        target: who,
                        seconds: 5,
                    }])?;
                }
                let hurt = match who {
                    Who::Player => {
                        let hurt = self.mob_hits_player(
                            amount,
                            [p.previous.x, p.previous.y, p.previous.z],
                            "minecraft:arrow",
                        );
                        // AbstractArrow.onHitEntity: a hit that hurt leaves a
                        // non-piercing arrow stuck in the body.
                        if hurt && p.arrow.as_ref().is_some_and(|a| a.pierce == 0) {
                            self.combat.stuck.add_arrow();
                        }
                        hurt
                    }
                    Who::Mob(id) => {
                        let hurt = self.mobs.iter_mut().find(|m| m.id == id).is_some_and(|m| {
                            m.damage_projectile(amount, source, aim, [p.velocity.x, p.velocity.z])
                        });
                        dodged = !hurt
                            && self.mobs.iter().find(|m| m.id == id).is_some_and(|m| {
                                m.species() == Species::Enderman
                                    && horizon_client::mobs::enderman::dodges(m, &source)
                            });
                        if p.owner == Some(Who::Player) {
                            self.last_hurt_by_player.insert(id, self.lan_acting);
                            if hurt && let Some(m) = self.mobs.iter().find(|m| m.id == id) {
                                // LivingEntity.hurtServer: PLAYER_HURT_ENTITY.
                                let victim = self.mob_entity(m);
                                let spectral = p.arrow.as_ref().is_some_and(|a| {
                                    &*a.pickup_item.item == "minecraft:spectral_arrow"
                                });
                                let direct = horizon_client::advancements::EntityView {
                                    kind: if spectral {
                                        "minecraft:spectral_arrow"
                                    } else {
                                        "minecraft:arrow"
                                    }
                                    .into(),
                                    position: [p.position.x, p.position.y, p.position.z],
                                    ..Default::default()
                                };
                                self.player_hit(&victim, "minecraft:arrow", Some(direct), amount);
                            }
                            // AbstractArrow.onHitEntity: KILLED_BY_ARROW for a kill
                            // (piercing arrows are not modelled).
                            if hurt
                                && let Some(m) = self.mobs.iter().find(|m| m.id == id)
                                && !m.alive()
                            {
                                let victim = self.mob_entity(m);
                                let weapon =
                                    p.arrow.as_ref().and_then(|a| a.fired_from_weapon.as_ref());
                                self.trigger_killed_by_arrow(&[victim], weapon);
                            }
                        }
                        hurt
                    }
                };
                self.burning_arrow_primes(p, who);
                // doKnockback: Punch pushes along the flight, less the target's
                // knockback resistance.
                let flight = (p.velocity.x * p.velocity.x + p.velocity.z * p.velocity.z).sqrt();
                let resistance = match who {
                    Who::Player => self
                        .player
                        .as_mut()
                        .and_then(|pl| pl.attribute("minecraft:knockback_resistance").ok())
                        .unwrap_or(0.0),
                    Who::Mob(id) => self
                        .mobs
                        .iter()
                        .find(|m| m.id == id)
                        .map_or(0.0, |m| m.knockback_resistance()),
                };
                let knockback = if !hurt {
                    0.0
                } else if let Some(weapon) =
                    p.arrow.as_ref().and_then(|a| a.fired_from_weapon.as_ref())
                {
                    let victim = match who {
                        Who::Player => Some("minecraft:player"),
                        Who::Mob(id) => self
                            .mobs
                            .iter()
                            .find(|m| m.id == id)
                            .map(|m| m.kind.id.as_str()),
                    };
                    let owner = match p.owner {
                        Some(Who::Player) => Some("minecraft:player"),
                        Some(Who::Mob(id)) => self
                            .mobs
                            .iter()
                            .find(|m| m.id == id)
                            .map(|m| m.kind.id.as_str()),
                        None => None,
                    };
                    let arrow_type = if p
                        .arrow
                        .as_ref()
                        .is_some_and(|a| &*a.pickup_item.item == "minecraft:spectral_arrow")
                    {
                        "minecraft:spectral_arrow"
                    } else {
                        "minecraft:arrow"
                    };
                    let ctx = horizon_client::enchant::EffectContext {
                        this: victim,
                        attacker: owner,
                        direct_attacker: Some(arrow_type),
                        damage_type: Some("minecraft:arrow"),
                        items: Some(self.res.items()),
                        ..Default::default()
                    };
                    self.res.stations().enchantments.modify_value(
                        Some(weapon),
                        "minecraft:knockback",
                        &ctx,
                        &mut self.world.random,
                        0.0,
                    )
                } else {
                    p.bonus.punch as f32
                };
                let push = f64::from(knockback) * 0.6 * (1.0 - resistance).max(0.0);
                if hurt && push > 0.0 && flight > 1e-6 {
                    let k = push / flight;
                    self.handle_mob_events(vec![MobEvent::Push {
                        target: who,
                        velocity: [p.velocity.x * k, 0.1, p.velocity.z * k],
                    }])?;
                }
                if hurt {
                    let spectral = p
                        .arrow
                        .as_ref()
                        .is_some_and(|a| &*a.pickup_item.item == "minecraft:spectral_arrow");
                    let direct = if spectral {
                        "minecraft:spectral_arrow"
                    } else {
                        "minecraft:arrow"
                    };
                    self.shot_post_attack(p, who, direct, "minecraft:arrow");
                    self.arrow_post_hurt_effects(p, who);
                }
                match (hurt, who) {
                    (true, _) => self.finish_arrow_hit(arrow, EntityHit::Hurt),
                    // What the player took no damage from (a raised shield)
                    // keeps its fire as it was and turns the arrow back.
                    (false, Who::Player) => {
                        if let (Some(pl), Some(ticks)) = (self.player.as_mut(), player_fire) {
                            pl.set_remaining_fire_ticks(ticks);
                        }
                        self.finish_arrow_hit(arrow, EntityHit::Refused);
                    }
                    // EnderMan.hurtServer teleported away: the arrow flies on.
                    (false, Who::Mob(_)) if dodged => {
                        self.finish_arrow_hit(arrow, arrow_miss(true));
                    }
                    // hurtOrSimulate returned false and
                    // projectileReceivesSideEffectsOnHit(false) is the default
                    // (Shulker.hurtServer on a closed shell, invulnerability).
                    // The fire the arrow just set goes back, then
                    // deflect(REVERSE, entity, owner, false, 0.2). The hit
                    // sound plays only when the hurt lands.
                    (false, Who::Mob(id)) => {
                        if let (Some(m), Some(ticks)) =
                            (self.mobs.iter_mut().find(|m| m.id == id), mob_fire)
                        {
                            m.fire_ticks = ticks;
                        }
                        self.finish_arrow_hit(arrow, arrow_miss(false));
                    }
                }
            }
            // AbstractArrow.onHitBlock plays its sound itself (ArrowEvent).
            (ProjectileKind::Arrow { .. }, Hit::Block { .. }) => {}
            (ProjectileKind::Snowball, hit) => {
                if let Hit::Entity {
                    who: Who::Mob(id), ..
                } = hit
                    && let Some(m) = self.mobs.iter_mut().find(|m| m.id == id)
                {
                    // Snowball.onHitEntity: 3 damage to blazes, 0 otherwise.
                    let amount = if m.species() == Species::Blaze {
                        3.0
                    } else {
                        0.0
                    };
                    if m.damage_projectile(amount, source, aim, [p.velocity.x, p.velocity.z]) {
                        self.thrown_hurt_mob(p, id, "minecraft:snowball", amount);
                    }
                } else if let Hit::Entity {
                    who: Who::Mob(id), ..
                } = hit
                {
                    // Snowball.onHitEntity: 0 damage, which still shakes a
                    // boat or minecart (VehicleEntity.hurtServer).
                    self.hurt_vehicle(p, id, 0.0);
                }
                self.thrown_break_event(p.id, atp);
                self.item_burst(atp, "minecraft:snowball");
            }
            // a mod's copy of Snowball: its damage to anything it hits.
            (ProjectileKind::Thrown(t), hit) => {
                match hit {
                    Hit::Entity {
                        who: Who::Mob(id), ..
                    } => {
                        let push = [p.velocity.x, p.velocity.z];
                        let hurt = self
                            .mobs
                            .iter_mut()
                            .find(|m| m.id == id)
                            .is_some_and(|m| m.damage_projectile(t.damage, source, aim, push));
                        if hurt {
                            self.player_hurt_mob(p, id);
                        }
                        // LivingEntity.hurtServer: PLAYER_HURT_ENTITY.
                        if hurt
                            && p.owner == Some(Who::Player)
                            && let Some(m) = self.mobs.iter().find(|m| m.id == id)
                        {
                            let victim = self.mob_entity(m);
                            let direct = horizon_client::advancements::EntityView {
                                kind: t.id.into(),
                                position: [p.position.x, p.position.y, p.position.z],
                                ..Default::default()
                            };
                            self.player_hit(&victim, "minecraft:thrown", Some(direct), t.damage);
                        }
                    }
                    // Player.hurtServer takes no 0 damage.
                    Hit::Entity {
                        who: Who::Player, ..
                    } if t.damage > 0.0 => {
                        let from = [p.previous.x, p.previous.y, p.previous.z];
                        self.mob_hits_player(t.damage, from, "minecraft:thrown");
                    }
                    _ => {}
                }
                self.thrown_break_event(p.id, atp);
                self.item_burst(atp, t.item);
            }
            (ProjectileKind::Egg, hit) => {
                if let Hit::Entity {
                    who: Who::Mob(id), ..
                } = hit
                    && let Some(m) = self.mobs.iter_mut().find(|m| m.id == id)
                {
                    if m.damage_projectile(0.0, source, aim, [p.velocity.x, p.velocity.z]) {
                        self.thrown_hurt_mob(p, id, "minecraft:egg", 0.0);
                    }
                } else if let Hit::Entity {
                    who: Who::Mob(id), ..
                } = hit
                {
                    // ThrownEgg.onHitEntity: the same 0 damage.
                    self.hurt_vehicle(p, id, 0.0);
                }
                // ThrownEgg.onHit: 1/8 chance of a chick, 1/32 of four.
                if self.world.random.next_int_bound(8).unwrap_or(1) == 0 {
                    let n = if self.world.random.next_int_bound(32).unwrap_or(1) == 0 {
                        4
                    } else {
                        1
                    };
                    for _ in 0..n {
                        if let Some(id) = self.spawn_mob_raw("minecraft:chicken", atp, true)
                            && let Some(c) = self.mobs.iter_mut().find(|m| m.id == id)
                        {
                            c.extra.age = -24000;
                        }
                    }
                }
                self.thrown_break_event(p.id, atp);
                self.item_burst(atp, "minecraft:egg");
            }
            (ProjectileKind::EnderPearl, hit) => {
                if let Hit::Entity {
                    who: Who::Mob(id), ..
                } = hit
                    && let Some(m) = self.mobs.iter_mut().find(|m| m.id == id)
                {
                    if m.damage_projectile(0.0, source, aim, [p.velocity.x, p.velocity.z]) {
                        self.thrown_hurt_mob(p, id, "minecraft:ender_pearl", 0.0);
                    }
                } else if let Hit::Entity {
                    who: Who::Mob(id), ..
                } = hit
                {
                    // ThrownEnderpearl.onHitEntity: the same 0 damage.
                    self.hurt_vehicle(p, id, 0.0);
                }
                // To where the pearl started its last tick (oldPosition).
                let mut to = p.previous;
                let mut damage = 5.0;
                let mut cancel = false;
                // EntityTeleportEvent.EnderPearl: a server player's pearl.
                if p.owner == Some(Who::Player)
                    && let Some(from) = self.player.as_ref().map(|pl| pl.position)
                    && let (Some(uuid), Some(name)) =
                        (self.lan_acting_uuid, self.lan_acting_name.clone())
                {
                    let mut target = [to.x, to.y, to.z];
                    self.native_ender_pearl_teleport(
                        uuid,
                        &name,
                        p.id,
                        [from.x, from.y, from.z],
                        &mut target,
                        &mut damage,
                        &mut cancel,
                    );
                    // A target that is not a number keeps the pearl's.
                    if target.iter().all(|v| v.is_finite()) {
                        to = Vec3::new(target[0], target[1], target[2]);
                    }
                }
                // ThrownEnderpearl.isAllowedToTeleportOwner: an owner who is
                // dead or asleep cannot take the pearl, so it is discarded
                // where it lands (no teleport and no 5 damage).
                let taken = self.player.as_ref().is_some_and(|pl| {
                    horizon_client::projectiles::owner_can_take_pearl(
                        !pl.health.dead_or_dying(),
                        self.sleeping.is_some(),
                    )
                });
                if p.owner == Some(Who::Player) && taken && !cancel {
                    // ThrownEnderpearl: 5% endermite where the player was
                    // (when monsters spawn), teleport, 5 damage.
                    if self.world.random.next_float() < 0.05
                        && self.ui.rules.bool("spawn_monsters")
                        && self.ui.difficulty != 0
                        && let Some(pl) = self.player.as_ref()
                    {
                        let from = pl.position;
                        self.spawn_mob_raw("minecraft:endermite", [from.x, from.y, from.z], false);
                    }
                    // ServerPlayer.teleport: removeVehicle before the placement.
                    self.stop_riding();
                    if let Some(pl) = self.player.as_mut() {
                        pl.position = to;
                        pl.velocity = Vec3::ZERO;
                        pl.fall_distance = 0.0;
                        pl.impulse.reset();
                        pl.teleported = true;
                    }
                    if self.lan_acting.is_some() {
                        horizon_client::lan::hub::teleport_cause(Some("ender_pearl"));
                    }
                    self.previous_position = to;
                    if damage > 0.0 {
                        self.hurt_player_by("minecraft:ender_pearl", damage);
                    }
                    self.audio.play(
                        "minecraft:entity.player.teleport",
                        Some([to.x, to.y, to.z]),
                        1.0,
                        1.0,
                    );
                }
            }
            (ProjectileKind::SmallFireball, hit) => match hit {
                Hit::Entity { who, .. } => match who {
                    Who::Player => {
                        if !self.creative {
                            // SmallFireball.onHitEntity: remember the fire, ignite,
                            // and restore it when the hurt is refused (a raised shield).
                            let remaining = self.player.as_ref().map(|p| p.fire_ticks);
                            if let Some(p) = self.player.as_mut() {
                                p.ignite_for_ticks(100)?;
                            }
                            let hurt = self.mob_hits_player(
                                5.0,
                                [p.previous.x, p.previous.y, p.previous.z],
                                "minecraft:fireball",
                            );
                            if !hurt
                                && let Some(remaining) = remaining
                                && let Some(pl) = self.player.as_mut()
                            {
                                pl.set_remaining_fire_ticks(remaining);
                            }
                        }
                    }
                    Who::Mob(id) => {
                        let hurt = self.mobs.iter_mut().find(|m| m.id == id).is_some_and(|m| {
                            let remaining = m.fire_ticks;
                            if !m.kind.flags.fire_immune {
                                m.ignite_for_ticks(100);
                            }
                            let hurt =
                                m.damage_projectile(5.0, source, aim, [p.velocity.x, p.velocity.z]);
                            if !hurt {
                                m.fire_ticks = remaining;
                            }
                            hurt
                        });
                        if hurt {
                            self.player_hurt_mob(p, id);
                            self.shot_post_attack(
                                p,
                                who,
                                "minecraft:small_fireball",
                                "minecraft:fireball",
                            );
                        } else if !self.mobs.iter().any(|m| m.id == id) {
                            // SmallFireball.onHitEntity: 5 damage to a boat or minecart.
                            self.hurt_vehicle(p, id, 5.0);
                        }
                    }
                },
                Hit::Block { pos, face, .. } => {
                    // SmallFireball.onHitBlock: a mob's only with mobGriefing;
                    // BaseFireBlock.getState placed with updates (the fire
                    // ticks, ages and goes out where it can't stay).
                    let fire = [pos[0] + face[0], pos[1] + face[1], pos[2] + face[2]];
                    let mob = matches!(p.owner, Some(Who::Mob(_)));
                    if (!mob || self.ui.rules.bool("mob_griefing"))
                        && self
                            .res
                            .facts()
                            .is_air(self.world.block(fire[0], fire[1], fire[2]))
                    {
                        let at = BlockPos::new(fire[0], fire[1], fire[2]);
                        let state =
                            self.with_level(|b, level, ticks| b.fire_state_for(level, ticks, at));
                        self.level_set(fire, state)?;
                    }
                }
                Hit::None => {}
            },
            (ProjectileKind::LargeFireball { power }, hit) => {
                // Fireball explosions are caused by the shooter. A direct hit
                // on the player takes `projectile_owner`, so read the owner
                // from the fireball.
                let owner = match p.owner {
                    Some(Who::Mob(id)) => Some(id),
                    _ => None,
                };
                if let Hit::Entity { who, .. } = hit {
                    match who {
                        Who::Player => {
                            self.mob_hits_player(
                                6.0,
                                [p.previous.x, p.previous.y, p.previous.z],
                                "minecraft:fireball",
                            );
                        }
                        Who::Mob(id) => {
                            if self.mobs.iter_mut().find(|m| m.id == id).is_some_and(|m| {
                                m.damage_projectile(6.0, source, aim, [p.velocity.x, p.velocity.z])
                            }) {
                                self.player_hurt_mob(p, id);
                            }
                            self.shot_post_attack(
                                p,
                                who,
                                "minecraft:fireball",
                                "minecraft:fireball",
                            );
                        }
                    }
                }
                self.combat.explosion_owner = owner;
                self.combat.explosion_in_water = p.touching_water;
                // The blast's source names the owner, so a dragon it kills
                // remembers the player who shot it.
                self.set_blast_by_player(p.owner == Some(Who::Player));
                // LargeFireball.onHit: both fire and MOB block destruction
                // follow mob_griefing; other mob explosions keep their default.
                let fire = self.ui.rules.bool("mob_griefing");
                let result = self.explode_mob(
                    atp,
                    *power as f32,
                    fire,
                    super::explosion::ExplosionSource::default()
                        .with_silence(self.entity_silent(p.id))
                        .with_entity(p.id, "minecraft:fireball"),
                );
                self.set_blast_by_player(false);
                self.combat.explosion_owner = None;
                self.combat.explosion_in_water = false;
                result?;
            }
            // AbstractThrownPotion.onHit, then onHitAsPotion from the stack it
            // flies as: the splash happens where the hit was, a lingering
            // potion's cloud sits at the mob it hit (ThrownLingeringPotion),
            // and the potion that lands names the hurt it deals.
            (kind @ (ProjectileKind::Potion(..) | ProjectileKind::ExperienceBottle), hit) => {
                // A bottle flies as the bottle item (`getDefaultItem`).
                let bottle;
                let stack = match kind {
                    ProjectileKind::Potion(stack, _) => stack,
                    _ => {
                        bottle = Box::new(ItemStack::new("minecraft:experience_bottle", 1));
                        &bottle
                    }
                };
                let block = match hit {
                    Hit::Block { pos, face, .. } => Some((
                        pos,
                        Direction::approximate_nearest(Vec3::new(
                            f64::from(face[0]),
                            f64::from(face[1]),
                            f64::from(face[2]),
                        )),
                    )),
                    _ => None,
                };
                let mob = match hit {
                    Hit::Entity {
                        who: Who::Player, ..
                    } => self.player.as_ref().map(|pv| pv.position),
                    Hit::Entity {
                        who: Who::Mob(id), ..
                    } => self.mobs.iter().find(|m| m.id == id).map(|m| m.position),
                    _ => None,
                };
                let owner = self.potion_owner(p.owner);
                let item = stack.item.to_string();
                let mut cancel = false;
                if stack.item.as_ref() != "minecraft:experience_bottle" {
                    self.native_potion_splash_v2(&item, [at.x, at.y, at.z], &mut cancel);
                }
                if !cancel {
                    self.splash(
                        stack,
                        at,
                        mob,
                        block,
                        p.age,
                        &owner,
                        self.entity_silent(p.id),
                        p.velocity,
                    );
                    if stack.item.as_ref() != "minecraft:experience_bottle" {
                        self.native_potion_splash(&item, [at.x, at.y, at.z]);
                    }
                }
            }
            // LlamaSpit.onHitEntity hurts only when the owner is a LivingEntity.
            (ProjectileKind::LlamaSpit, Hit::Entity { .. }) if p.owner.is_none() => {}
            (ProjectileKind::LlamaSpit, Hit::Entity { who, .. }) => match who {
                Who::Player => {
                    self.mob_hits_player(
                        1.0,
                        [p.previous.x, p.previous.y, p.previous.z],
                        "minecraft:spit",
                    );
                }
                Who::Mob(id) => {
                    if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id)
                        && m.damage_projectile(1.0, source, aim, [p.velocity.x, p.velocity.z])
                    {
                        self.shot_post_attack(p, who, "minecraft:llama_spit", "minecraft:spit");
                    }
                }
            },
            // ShulkerBullet.onHitEntity: 4 damage, then levitation for 10 s.
            (ProjectileKind::ShulkerBullet { .. }, Hit::Entity { who, .. }) => match who {
                Who::Player => {
                    // Only a hit that hurt (`wasHurt`) brings the levitation.
                    if !self.creative
                        && self.mob_hits_player(
                            4.0,
                            [p.previous.x, p.previous.y, p.previous.z],
                            "minecraft:mob_projectile",
                        )
                    {
                        self.give_player_effect("minecraft:levitation", 200, 0);
                    }
                }
                Who::Mob(id) => {
                    let hurt = self.mobs.iter_mut().find(|m| m.id == id).is_some_and(|m| {
                        let hurt =
                            m.damage_projectile(4.0, source, aim, [p.velocity.x, p.velocity.z]);
                        if hurt {
                            // Shulker.hurtServer: a shulker hit by a bullet.
                            horizon_client::mobs::shulker::hit_by_bullet(m);
                        }
                        hurt
                    });
                    if hurt {
                        // The hurt's teleport (`teleportSomewhere`,
                        // `hitByShulkerBullet`) comes before the levitation.
                        self.settle_hurt_teleports()?;
                        self.shot_post_attack(
                            p,
                            who,
                            "minecraft:shulker_bullet",
                            "minecraft:mob_projectile",
                        );
                        if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
                            m.add_effect("minecraft:levitation", 200, 0);
                        }
                    }
                }
            },
            // DragonFireball.onHit (its own owner's hit never gets here): the
            // dragon's breath where it burst.
            (ProjectileKind::DragonFireball, _) => self.dragon_fireball_on_hit(at),
            (ProjectileKind::WitherSkull { .. }, hit) => {
                let who = match hit {
                    Hit::Entity { who, .. } => Some(who),
                    _ => None,
                };
                self.wither_skull_hit(p, who, atp)?;
            }
            (ProjectileKind::ShulkerBullet { .. }, Hit::Block { .. }) => {
                self.audio
                    .play("minecraft:entity.shulker_bullet.hit", Some(atp), 1.0, 1.0);
                if let Some(ty) = horizon_client::particles::ParticleType::from_name("explosion") {
                    let options = horizon_client::particles::ParticleOptions::simple(ty);
                    for _ in 0..2 {
                        let r = &mut self.world.random;
                        let at = [
                            atp[0] + r.next_gaussian() * 0.2,
                            atp[1] + r.next_gaussian() * 0.2,
                            atp[2] + r.next_gaussian() * 0.2,
                        ];
                        self.particles.add(&options, at, [0.0; 3]);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
    /// `Snowball`/`ThrownEgg.onHit`: entity event 3 for the guests, before the
    /// projectile is discarded (the clients lose it when the tracker next runs).
    fn thrown_break_event(&mut self, id: u32, at: [f64; 3]) {
        if id != 0 {
            self.lan_entity_event(id as i32, 3, at);
        }
    }
    /// `Snowball`/`ThrownEgg.handleEntityEvent(3)`: eight crumbs of the
    /// thrown item where it broke, at rest for a snowball, scattered by the
    /// client entity's random (the particle engine's here) for an egg.
    pub(super) fn item_burst(&mut self, at: [f64; 3], item: &str) {
        let options = self.item_particle(&horizon_client::items::ItemStack::new(item, 1));
        let egg = item == "minecraft:egg";
        for _ in 0..8 {
            let v = if egg {
                let r = self.particles.random();
                [
                    (f64::from(r.next_float()) - 0.5) * 0.08,
                    (f64::from(r.next_float()) - 0.5) * 0.08,
                    (f64::from(r.next_float()) - 0.5) * 0.08,
                ]
            } else {
                [0.0; 3]
            };
            self.particles.add(&options, at, v);
        }
    }
    /// `AgeableMob.mobInteract`: whether a click with `item` on `m` gets to
    /// the golden dandelion check (`canUseGoldenDandelion`, which a tadpole
    /// calls with `isBaby` true). A panda that is scared or on its back and an
    /// armadillo in its shell answer in their own `mobInteract` first.
    pub(super) fn age_lock_takes(&self, m: &Mob, item: &str) -> bool {
        let species = m.species();
        let baby = if species == Species::Tadpole {
            true
        } else if horizon_client::mobs::finalize::ageable(species) {
            m.is_baby()
                && !(species == Species::Panda
                    && (horizon_client::mobs::panda::scared(m)
                        || horizon_client::mobs::panda::is_on_back(m)))
                && !(species == Species::Armadillo && horizon_client::mobs::armadillo::scared(m))
        } else {
            false
        };
        let cannot = self
            .res
            .mob_data()
            .entity_has("minecraft:cannot_be_age_locked", &m.kind.id);
        horizon_client::mobs::ai::can_use_golden_dandelion(
            item,
            baby,
            m.extra.age_lock_particle_timer,
            cannot,
        )
    }
    /// `AgeableMob.setAgeLocked`: a golden dandelion on mob `index` flips its
    /// age lock (a tadpole's or an ageable mob's own `setAgeLockedData`) and is
    /// used up; a locked mob stays (`setPersistenceRequired`), and the lock's
    /// use or unuse sound plays at its block for everyone near.
    pub(super) fn set_age_locked(&mut self, index: usize) {
        let m = &mut self.mobs[index];
        if m.species() == Species::Tadpole {
            horizon_client::mobs::aquatic::set_age_locked_data(m);
        } else {
            horizon_client::mobs::ai::set_age_locked_data(m);
        }
        self.consume_held();
        let m = &mut self.mobs[index];
        let locked = m.extra.age_locked;
        if locked {
            m.persistent = true;
        }
        // Level.playSound(null, blockPosition, ..): the middle of the block.
        let at = [
            m.position.x.floor() + 0.5,
            m.position.y.floor() + 0.5,
            m.position.z.floor() + 0.5,
        ];
        let chunk = (
            m.position.x.floor() as i32 >> 4,
            m.position.z.floor() as i32 >> 4,
        );
        self.world.modified.insert(chunk);
        let sound = if locked {
            "minecraft:item.golden_dandelion.use"
        } else {
            "minecraft:item.golden_dandelion.unuse"
        };
        self.audio.play(sound, Some(at), 1.0, 1.0);
    }
    /// The mob a normal interaction can reach: a packet's exact validated ID,
    /// or the crosshair's mob before its blocking shape.
    pub(super) fn mob_interaction_target(&self, target: Option<InteractTarget>) -> Option<usize> {
        let (index, distance, _) = (match target {
            // The id the packet named, under the filters the crosshair pick
            // applies (`mob_hit`): a mob in its death animation is not clicked
            // by either path, so naming one in a packet must not reach the
            // feed, shear, leash or name paths either.
            Some(t) => Self::named_entity(&self.mobs, t.id, |m| m.id)
                .filter(|&i| {
                    let m = &self.mobs[i];
                    named_mob_pickable(m.alive(), m.id, self.riding, self.remote_camera_id())
                })
                .map(|i| (i, 0.0, [0.0; 3])),
            None => self.mob_hit(),
        })?;
        // Only a client's own pick is stopped by a block: the range check passed.
        if target.is_none()
            && let Some(hit) = self.pick_unfiltered()
            && let Some(eye) = self.eye_position()
        {
            let d = ((hit.point[0] - eye[0]).powi(2)
                + (hit.point[1] - eye[1]).powi(2)
                + (hit.point[2] - eye[2]).powi(2))
            .sqrt();
            if d < distance {
                return None;
            }
        }
        Some(index)
    }
    /// Right click on a mob (Mob.interact / mobInteract), on the mob under the
    /// crosshair or on the one a guest's interact packet named: returns true
    /// when consumed.
    pub(super) fn interact_mob(&mut self, target: Option<InteractTarget>) -> Result<bool> {
        let Some(index) = self.mob_interaction_target(target) else {
            return Ok(false);
        };
        // PlayerInteractTrigger: the name tag as it was before it named the mob.
        let used = self.held().cloned();
        if self.name_mob(index) {
            if let Some(stack) = &used {
                let target = self.mob_entity(&self.mobs[index]);
                self.trigger_item_entity("minecraft:player_interacted_with_entity", stack, &target);
            }
            return Ok(true);
        }
        if self
            .held()
            .is_some_and(|stack| &*stack.item == "minecraft:shears")
        {
            let mob = &self.mobs[index];
            let shearable = match mob.species() {
                Species::Sheep => !mob.extra.sheared && !mob.is_baby(),
                Species::Mooshroom => !mob.is_baby(),
                Species::SnowGolem | Species::Bogged => mob.alive() && !mob.extra.sheared,
                _ => false,
            };
            if shearable
                && let (Some(player), Some(name)) =
                    (self.lan_acting_uuid, self.lan_acting_name.clone())
            {
                let mut cancel = false;
                self.native_player_shear_entity(
                    player,
                    &name,
                    mob.id as i32,
                    self.hand == Hand::Off,
                    &mut cancel,
                );
                if cancel {
                    return Ok(true);
                }
            }
        }
        if self.spawn_egg_offspring(index) {
            self.swing_arm();
            return Ok(true);
        }
        {
            let item = self.held().map(|h| h.item.to_string()).unwrap_or_default();
            let used = self.held().cloned();
            if self.interact_leash(index, &item) {
                if let Some(stack) = &used {
                    let target = self.mob_entity(&self.mobs[index]);
                    self.trigger_item_entity(
                        "minecraft:player_interacted_with_entity",
                        stack,
                        &target,
                    );
                }
                self.swing_arm();
                return Ok(true);
            }
        }
        let held = self.held().cloned();
        let item = held
            .as_ref()
            .map(|h| h.item.to_string())
            .unwrap_or_default();
        // Bucketable.bucketMobPickup: a water bucket scoops up a fish, a
        // tadpole or an axolotl, with the data its bucket carries.
        if item == "minecraft:water_bucket"
            && self.mobs[index].alive()
            && let Some((bucket, sound)) = bucketable(self.mobs[index].species())
        {
            let mut filled = ItemStack::new(bucket, 1);
            filled.extra = Some(Box::new(horizon_client::entity_nbt::bucket_components(
                &self.mobs[index],
            )));
            self.audio
                .play(sound, Some(self.mobs[index].center()), 1.0, 1.0);
            self.trigger_item("minecraft:filled_bucket", &filled);
            // PlayerInteractTrigger: the water bucket as it was, and the mob picked up.
            if let Some(stack) = &held {
                let target = self.mob_entity(&self.mobs[index]);
                self.trigger_item_entity("minecraft:player_interacted_with_entity", stack, &target);
            }
            let species = bucket.trim_end_matches("_bucket").to_owned();
            let id = self.mobs[index].id as i32;
            // Leashable.dropLeash, then discard.
            self.break_leash(id as u32, true, false);
            self.mobs[index].removed = true;
            if self.creative {
                if let Some(rest) = self.give(filled) {
                    self.spawn_item(rest, self.eye_position().unwrap_or([0.0; 3]), [0.0; 3], 40);
                }
            } else {
                *self.held_mut() = Some(filled);
            }
            if let (Some(player), Some(name)) = (self.lan_acting_uuid, self.lan_acting_name.clone())
            {
                self.native_player_bucket_entity(player, &name, id, &species);
            }
            self.swing_arm();
            return Ok(true);
        }
        // AgeableMob.mobInteract: a golden dandelion on a baby freezes its
        // growth, or lets it grow again.
        if self.mobs[index].alive() && self.age_lock_takes(&self.mobs[index], &item) {
            let entity_id = self.mobs[index].id;
            self.set_age_locked(index);
            if let (Some(player), Some(name)) = (self.lan_acting_uuid, self.lan_acting_name.clone())
            {
                self.native_player_toggle_entity_age_lock(
                    player,
                    &name,
                    entity_id,
                    self.mobs[index].extra.age_locked,
                );
            }
            if let Some(stack) = &held {
                let target = self.mob_entity(&self.mobs[index]);
                self.trigger_item_entity("minecraft:player_interacted_with_entity", stack, &target);
            }
            self.swing_arm();
            return Ok(true);
        }
        let creative = self.creative;
        // The hand's slot (Player.interactOn passes the hand).
        let selected = self.held_slot();
        // Panda.mobInteract: bamboo, or a click on a panda lying on its back.
        if self.mobs[index].species() == Species::Panda && self.mobs[index].alive() {
            let taken = self.interact_panda(index, &item);
            if taken {
                self.swing_arm();
            }
            return Ok(taken);
        }
        if self.interact_zombie_villager(index, &item) {
            return Ok(true);
        }
        if self.interact_trader(index, &item)? {
            self.swing_arm();
            return Ok(true);
        }
        if !self.mobs[index].kind.food.contains(&item)
            && self.interact_sheep_dye(index, self.hand, false)
        {
            // PlayerInteractTrigger: the dye as it was before it coloured the sheep.
            if let Some(stack) = &held {
                let target = self.mob_entity(&self.mobs[index]);
                self.trigger_item_entity("minecraft:player_interacted_with_entity", stack, &target);
            }
            self.swing_arm();
            return Ok(true);
        }
        if self.interact_allay(index, held.as_ref()) {
            self.swing_arm();
            return Ok(true);
        }
        if self.interact_sulfur_cube(index, &item) {
            if let Some(stack) = &held {
                let target = self.mob_entity(&self.mobs[index]);
                self.trigger_item_entity("minecraft:player_interacted_with_entity", stack, &target);
            }
            self.swing_arm();
            return Ok(true);
        }
        if self.interact_armadillo(index, &item) {
            if let Some(stack) = &held {
                let target = self.mob_entity(&self.mobs[index]);
                self.trigger_item_entity("minecraft:player_interacted_with_entity", stack, &target);
            }
            self.swing_arm();
            return Ok(true);
        }
        if self.interact_copper_golem(index, &item) {
            self.swing_arm();
            return Ok(true);
        }
        if self.interact_shear_mob(index, &item) {
            self.swing_arm();
            return Ok(true);
        }
        if let Some(sheared) = self.shear_saddle(index, &item) {
            if sheared {
                self.swing_arm();
            }
            return Ok(sheared);
        }
        // AbstractHorse.mobInteract: feed, equip or ride.
        if self.interact_mount(index, &item) {
            self.swing_arm();
            return Ok(true);
        }
        // PiglinAi.mobInteract: hand over something gold to admire.
        if self.mobs[index].alive() && self.give_piglin(index, &item) {
            self.swing_arm();
            if !creative && let Some(stack) = self.stack_in_mut(selected).as_mut() {
                stack.count -= 1;
                if stack.count == 0 {
                    *self.stack_in_mut(selected) = None;
                }
            }
            return Ok(true);
        }
        // Creeper.mobInteract: any item in #creeper_igniters, damaged when it
        // is damageable and used up otherwise.
        let creeper_igniter = self
            .res
            .items()
            .item_tags
            .get("minecraft:creeper_igniters")
            .is_some_and(|t| t.contains(&*item));
        let fish = self
            .res
            .items()
            .item_tags
            .get("minecraft:fishes")
            .is_some_and(|t| t.contains(&*item));
        let damageable = self
            .res
            .items()
            .get(&item)
            .is_some_and(|i| i.max_damage > 0);
        // Wolf.mobInteract: the owner's #wolf_collar_dyes item only ever dyes the
        // collar; one of the colour the collar already has does nothing.
        let wolf_collar_dye = self
            .res
            .items()
            .item_tags
            .get("minecraft:wolf_collar_dyes")
            .is_some_and(|t| t.contains(&*item));
        let parrot_poisonous = self
            .res
            .items()
            .item_tags
            .get("minecraft:parrot_poisonous_food")
            .is_some_and(|t| t.contains(&*item));
        let player_at = self.eye_position();
        let player_feet = self
            .player
            .as_ref()
            .map(|p| [p.position.x, p.position.y, p.position.z]);
        let acting = self.acting_player_uuid();
        let sneaking =
            self.keys.held(Action::Sneak) || self.keys.raw(&winit::keyboard::KeyCode::ShiftRight);
        let held_damage = held.as_ref().map_or(0, |h| h.damage as i32);
        let held_extra = held.as_ref().and_then(|h| h.extra.clone());
        let m = &mut self.mobs[index];
        if !m.alive() {
            return Ok(false);
        }
        let species = m.species();
        let center = m.center();
        // Wolf armor the owner takes off with shears or leaves a dropped stack of.
        let mut armor_drop: Option<ItemStack> = None;
        let mut equip_event: Option<bool> = None;
        let mut consume = false;
        let mut give: Option<&str> = None;
        let mut sound: Option<&str> = None;
        // Sounds played as vanilla plays them: (event, volume, pitch) at the feet.
        let mut eat: Option<(&'static str, f32, f32)> = None;
        let feet = [m.position.x, m.position.y, m.position.z];
        let mut handled = true;
        let mut ignited_creeper = false;
        let mut drops: Vec<(String, u32)> = Vec::new();
        // The shearing table rolled, for the mods (none for a mooshroom).
        let mut drops_table = String::new();
        let mob = (m.id, super::native_hooks::mob_kind(m));
        // `Sheep.shear`'s drops add a spread on top of the ItemEntity's own
        // motion; `MushroomCow.shear`'s do not.
        let mut wool = false;
        let is_food = m.kind.food.contains(&item);
        let baby_turtle = species == Species::Turtle && m.is_baby();
        // MushroomCow.getEffectsFromItemStack: the item's block supplies them.
        let flower = if species == Species::Mooshroom {
            horizon_client::flowers::stew_effect(&item)
        } else {
            None
        };
        let mut give_stew: Option<Vec<(Arc<str>, i32)>> = None;
        // `Some(true)` for the four spell particles, `Some(false)` for the two
        // smoke puffs of a cow that already holds effects.
        let mut stew_particles: Option<bool> = None;
        // `MushroomCow.shear`: where, and how tall, the mooshroom that was sheared stood.
        let mut shear_puff: Option<([f64; 3], f64)> = None;
        // `Ocelot.mobInteract`: whether the fish won its trust.
        let mut ocelot_trust: Option<bool> = None;
        match species {
            Species::Cow | Species::Mooshroom if item == "minecraft:bucket" && !m.is_baby() => {
                give = Some("minecraft:milk_bucket");
                consume = true;
                sound = Some("entity.cow.milk");
            }
            // Goat.mobInteract: a bucket on an adult gives milk.
            Species::Goat if item == "minecraft:bucket" && !m.is_baby() => {
                give = Some("minecraft:milk_bucket");
                consume = true;
                sound = Some(if m.extra.screaming {
                    "entity.goat.screaming.milk"
                } else {
                    "entity.goat.milk"
                });
            }
            // MushroomCow.mobInteract: a bowl gives back what a flower left as
            // a suspicious stew, or plain stew when it holds no effects.
            Species::Mooshroom if item == "minecraft:bowl" && !m.is_baby() => {
                consume = true;
                match m.extra.stew_effects.take() {
                    Some(effects) => {
                        give = Some("minecraft:suspicious_stew");
                        give_stew = Some(effects);
                        // `playSound(.., 1.0F, 1.0F)` at the feet.
                        eat = Some(("entity.mooshroom.suspicious_milk", 1.0, 1.0));
                    }
                    None => {
                        give = Some("minecraft:mushroom_stew");
                        sound = Some("entity.mooshroom.milk");
                    }
                }
            }
            Species::Mooshroom if item == "minecraft:shears" && !m.is_baby() => {
                // MushroomCow.shear: becomes a cow and drops five mushrooms.
                let mushroom = if m.extra.variant == 1 {
                    "minecraft:brown_mushroom"
                } else {
                    "minecraft:red_mushroom"
                };
                drops.push((mushroom.into(), 5));
                shear_puff = Some((
                    [m.position.x, m.position.y, m.position.z],
                    f64::from(m.height()),
                ));
                self.audio.play_bound(
                    "minecraft:entity.mooshroom.shear",
                    7,
                    1.0,
                    1.0,
                    horizon_client::sound_engine::SoundEntity::new(m.id, feet, m.silent),
                    None,
                );
                m.removed = true;
                let setup = SpawnSetup {
                    yaw: m.yaw,
                    persistent: true,
                    skip_finalize: true,
                    ..Default::default()
                };
                let pos = [m.position.x, m.position.y, m.position.z];
                self.handle_mob_events(vec![MobEvent::SpawnKind {
                    kind: "minecraft:cow".into(),
                    at: pos,
                    setup,
                }])?;
                self.damage_held_tool();
            }
            // MushroomCow.mobInteract: a brown adult takes the flower's effects
            // unless it holds some already, when it only puffs smoke.
            Species::Mooshroom if m.extra.variant == 1 && !m.is_baby() && flower.is_some() => {
                if m.extra.stew_effects.is_some() {
                    stew_particles = Some(false);
                } else {
                    let (effect, ticks) = flower.expect("the guard checked");
                    m.extra.stew_effects = Some(vec![(Arc::from(effect), ticks)]);
                    consume = true;
                    eat = Some(("entity.mooshroom.eat", 2.0, 1.0));
                    stew_particles = Some(true);
                }
            }
            Species::Sheep if item == "minecraft:shears" && !m.extra.sheared && !m.is_baby() => {
                self.audio.play_bound(
                    "minecraft:entity.sheep.shear",
                    7,
                    1.0,
                    1.0,
                    horizon_client::sound_engine::SoundEntity::new(m.id, feet, m.silent),
                    None,
                );
                m.extra.sheared = true;
                let color = horizon_client::mobs::render::DYE_NAMES[m.extra.color as usize & 15];
                drops_table = format!("minecraft:shearing/sheep/{color}");
                let stacks = world_items!(self).loot_table(
                    &drops_table,
                    false,
                    None,
                    &mut self.world.random,
                );
                drops.extend(stacks.into_iter().map(|s| (s.item.to_string(), s.count)));
                wool = true;
                self.damage_held_tool();
            }
            // Entity.interact: shears take a tamed wolf's armor off for its owner.
            Species::Wolf
                if item == "minecraft:shears"
                    && m.extra.tame
                    && m.extra.owner == Some(acting)
                    && m.extra.body.is_some()
                    && !sneaking =>
            {
                let mut stack = ItemStack::new("minecraft:wolf_armor", 1);
                stack.damage = m.extra.body_damage.max(0) as u32;
                stack.extra = m.extra.body_extra.take();
                armor_drop = Some(stack);
                m.extra.body = None;
                m.extra.body_damage = 0;
                eat = Some(("item.armor.unequip_wolf", 1.0, 1.0));
                equip_event = Some(false);
                self.damage_held_tool();
            }
            // Wolf.mobInteract: heal, dye the collar or sit/stand when tame;
            // a bone tames a wild one a third of the time.
            Species::Wolf if m.extra.tame && is_food && m.health < m.attrs.max_health => {
                let heal = self
                    .res
                    .items()
                    .get(&item)
                    .and_then(|i| i.food.as_ref())
                    .map_or(2.0, |f| f.0 as f32 * 2.0);
                // TamableAnimal.feed(.., 2.0F, 2.0F): heals twice the nutrition,
                // then playEatingSound, which wolves leave empty.
                m.heal(heal);
                consume = true;
            }
            // Wolf.mobInteract: the owner puts wolf armor on a tamed adult.
            Species::Wolf
                if item == "minecraft:wolf_armor"
                    && m.extra.tame
                    && m.extra.owner == Some(acting)
                    && m.extra.body.is_none()
                    && !m.is_baby() =>
            {
                m.extra.body = Some(item.as_str().into());
                m.extra.body_damage = held_damage;
                m.extra.body_extra = held_extra;
                consume = true;
                eat = Some(("item.armor.equip_wolf", 1.0, 1.0));
                equip_event = Some(true);
            }
            // Wolf.mobInteract: an armadillo scute repairs a sitting wolf's
            // armor by an eighth of its durability.
            Species::Wolf
                if item == "minecraft:armadillo_scute"
                    && m.extra.tame
                    && m.extra.owner == Some(acting)
                    && m.extra.sitting
                    && m.extra.body.is_some()
                    && m.extra.body_damage > 0 =>
            {
                m.extra.body_damage = (m.extra.body_damage
                    - (horizon_client::mobs::WOLF_ARMOR_DURABILITY as f32 * 0.125) as i32)
                    .max(0);
                consume = true;
                eat = Some(("item.wolf_armor.repair", 1.0, 1.0));
            }
            Species::Wolf
                if m.extra.tame
                    && m.extra.owner == Some(acting)
                    && item.ends_with("_dye")
                    && horizon_client::mobs::render::DYE_NAMES
                        .iter()
                        .position(|d| {
                            item.trim_start_matches("minecraft:")
                                .trim_end_matches("_dye")
                                == *d
                        })
                        .is_some_and(|c| c as u8 != m.extra.color) =>
            {
                let name = item
                    .trim_start_matches("minecraft:")
                    .trim_end_matches("_dye");
                if let Some(c) = horizon_client::mobs::render::DYE_NAMES
                    .iter()
                    .position(|d| *d == name)
                {
                    m.extra.color = c as u8;
                }
                consume = true;
            }
            // Wolf.mobInteract: Animal.mobInteract first breeds an adult or
            // grows a baby with the food; only when it did not does the owner
            // make the pet sit or stand.
            Species::Wolf
                if m.extra.tame
                    && m.extra.owner == Some(acting)
                    && !wolf_collar_dye
                    && !(is_food && takes_food(m)) =>
            {
                m.extra.sitting = !m.extra.sitting;
                m.ai.jumping = false;
                m.nav_stop();
                m.ai.target = None;
                self.world.modified.insert((
                    m.position.x.floor() as i32 >> 4,
                    m.position.z.floor() as i32 >> 4,
                ));
            }
            Species::Wolf
                if !m.extra.tame
                    && item == "minecraft:bone"
                    && !horizon_client::mobs::tame::angry(m) =>
            {
                consume = true;
                let tamed = m.random.next_int_bound(3).unwrap_or(1) == 0;
                if tamed {
                    horizon_client::mobs::tame::set_tame(m, true);
                    m.extra.owner = Some(acting);
                    m.persistent = true;
                    m.nav_stop();
                    m.ai.target = None;
                    m.extra.sitting = true;
                }
                // TamableAnimal.spawnTamingParticles (entity event 7 or 6).
                self.mob_event_particles(self.mobs[index].id, if tamed { 7 } else { 6 });
                if tamed {
                    let view = self.mob_entity(&self.mobs[index]);
                    self.trigger_entity("minecraft:tame_animal", "entity", &view);
                }
                let m = &self.mobs[index];
                self.world.modified.insert((
                    m.position.x.floor() as i32 >> 4,
                    m.position.z.floor() as i32 >> 4,
                ));
            }
            // Cat.mobInteract: dye, heal or sit when tame; fish may tame a wild one.
            Species::Cat
                if m.extra.tame
                    && m.extra.owner == Some(acting)
                    && item.ends_with("_dye")
                    && horizon_client::mobs::render::DYE_NAMES
                        .iter()
                        .position(|d| {
                            item.trim_start_matches("minecraft:")
                                .trim_end_matches("_dye")
                                == *d
                        })
                        .is_some_and(|c| c as u8 != m.extra.color) =>
            {
                let name = item
                    .trim_start_matches("minecraft:")
                    .trim_end_matches("_dye");
                if let Some(c) = horizon_client::mobs::render::DYE_NAMES
                    .iter()
                    .position(|d| *d == name)
                {
                    m.extra.color = c as u8;
                }
                m.persistent = true;
                consume = true;
            }
            Species::Cat
                if m.extra.tame
                    && m.extra.owner == Some(acting)
                    && is_food
                    && m.health < m.attrs.max_health =>
            {
                let heal = self
                    .res
                    .items()
                    .get(&item)
                    .and_then(|i| i.food.as_ref())
                    .map_or(1.0, |f| f.0 as f32);
                // TamableAnimal.feed.
                m.heal(heal);
                consume = true;
                eat = eating_sound(m, &mut self.world.random);
            }
            // Cat.mobInteract: Animal.mobInteract first breeds an adult or
            // grows a baby with the food; only when it did not does the owner
            // make the pet sit or stand.
            Species::Cat
                if m.extra.tame && m.extra.owner == Some(acting) && !(is_food && takes_food(m)) =>
            {
                m.extra.sitting = !m.extra.sitting;
                self.world.modified.insert((
                    m.position.x.floor() as i32 >> 4,
                    m.position.z.floor() as i32 >> 4,
                ));
            }
            // Parrot.mobInteract: the owner makes a tame parrot on the ground
            // sit or stand (poisonous food is handled apart).
            Species::Parrot
                if m.extra.tame
                    && m.extra.owner == Some(acting)
                    && m.on_ground
                    && !parrot_poisonous =>
            {
                m.extra.sitting = !m.extra.sitting;
                self.world.modified.insert((
                    m.position.x.floor() as i32 >> 4,
                    m.position.z.floor() as i32 >> 4,
                ));
            }
            Species::Cat if !m.extra.tame && is_food => {
                consume = true;
                m.persistent = true;
                let tamed = m.random.next_int_bound(3).unwrap_or(1) == 0;
                if tamed {
                    m.extra.tame = true;
                    m.extra.owner = Some(acting);
                    m.extra.sitting = true;
                }
                eat = eating_sound(m, &mut self.world.random);
                // TamableAnimal.spawnTamingParticles (entity event 7 or 6).
                self.mob_event_particles(self.mobs[index].id, if tamed { 7 } else { 6 });
                if tamed {
                    let view = self.mob_entity(&self.mobs[index]);
                    self.trigger_entity("minecraft:tame_animal", "entity", &view);
                }
                let m = &self.mobs[index];
                self.world.modified.insert((
                    m.position.x.floor() as i32 >> 4,
                    m.position.z.floor() as i32 >> 4,
                ));
            }
            // IronGolem.mobInteract: an iron ingot repairs 25 health.
            Species::IronGolem
                if item == "minecraft:iron_ingot" && m.health < m.attrs.max_health =>
            {
                m.heal(25.0);
                consume = true;
                sound = Some("entity.iron_golem.repair");
            }
            Species::Creeper if creeper_igniter => {
                m.extra.flag = true;
                ignited_creeper = true;
                // FIRECHARGE_USE for a fire charge, else FLINTANDSTEEL_USE,
                // at the creeper's feet with a pitch from its own random.
                let event = if item == "minecraft:fire_charge" {
                    "item.firecharge.use"
                } else {
                    "item.flintandsteel.use"
                };
                eat = Some((event, 1.0, m.random.next_float() * 0.4 + 0.8));
                if damageable {
                    self.damage_held_tool();
                } else {
                    consume = true;
                }
            }
            // Parrot.mobInteract: #parrot_poisonous_food (a cookie) poisons the
            // parrot for 45 s and then hurts it with a player attack of
            // Float.MAX_VALUE, which an invulnerable one shrugs off.
            Species::Parrot
                if self
                    .res
                    .items()
                    .item_tags
                    .get("minecraft:parrot_poisonous_food")
                    .is_some_and(|t| t.contains(&*item)) =>
            {
                consume = true;
                m.add_effect("minecraft:poison", 900, 0);
                let id = m.id;
                let hurt = m.damage(
                    f32::MAX,
                    horizon_client::mobs::Damage::player(player_at.unwrap_or(center)),
                );
                // The feeder gets the kill, as for a normal player attack.
                if hurt {
                    self.last_hurt_by_player.insert(id, self.lan_acting);
                }
            }
            // Parrot.mobInteract: parrot food may tame a wild one (one time in
            // ten); a parrot never takes it as Animal food (isFood is false).
            Species::Parrot if !m.extra.tame && is_food => {
                consume = true;
                if !m.silent {
                    let pitch = 1.0 + (m.random.next_float() - m.random.next_float()) * 0.2;
                    eat = Some(("entity.parrot.eat", 1.0, pitch));
                }
                let tamed = m.random.next_int_bound(10).unwrap_or(1) == 0;
                if tamed {
                    m.extra.tame = true;
                    m.extra.owner = Some(acting);
                }
                // TamableAnimal.spawnTamingParticles (entity event 7 or 6).
                self.mob_event_particles(self.mobs[index].id, if tamed { 7 } else { 6 });
                if tamed {
                    let view = self.mob_entity(&self.mobs[index]);
                    self.trigger_entity("minecraft:tame_animal", "entity", &view);
                }
            }
            // Dolphin.mobInteract: a fish makes a baby grow by a tenth and an
            // adult remember it (`gotFish`), to swim to a treasure.
            Species::Dolphin if fish => {
                eat = Some(("entity.dolphin.eat", 1.0, 1.0));
                if horizon_client::mobs::ai::can_age_up(m) {
                    let secs = ((-m.extra.age / 20) as f32 * 0.1) as i32;
                    horizon_client::mobs::ai::age_up(m, secs, true);
                } else {
                    m.extra.flag = true;
                }
                consume = true;
            }
            // Tadpole.feed: frog food ages it by a tenth of the time left to
            // grow into a frog (it grows on its next tick once past 24000).
            Species::Tadpole if is_food && !m.extra.age_locked => {
                consume = true;
                let left = 24_000 - m.extra.age;
                m.extra.age += ((left / 20) as f32 * 0.1) as i32 * 20;
                // One happy villager particle at rest about the body.
                self.particles_around_mob(
                    self.mobs[index].id,
                    "happy_villager",
                    horizon_client::particles::Burst::HORSE_EATING,
                );
            }
            // Bee.mobInteract: a wither rose or an open eyeblossom is eaten for
            // its `getBeeInteractionEffect` instead of breeding the bee.
            Species::Bee
                if is_food
                    && matches!(
                        &*item,
                        "minecraft:wither_rose" | "minecraft:open_eyeblossom"
                    ) =>
            {
                if &*item == "minecraft:wither_rose" {
                    m.add_effect("minecraft:wither", 40, 0);
                } else {
                    m.add_effect("minecraft:poison", 25, 0);
                }
                consume = true;
            }
            // Ocelot.mobInteract: raw fish from a hand within 3 blocks, while it comes for the
            // food (`temptGoal.isRunning`), is used up; one time in three it trusts the player
            // (entity event 41, hearts), otherwise smoke (event 40). Anything else is
            // `Animal.mobInteract`.
            Species::Ocelot
                if is_food
                    && !m.extra.trusting
                    && m.ai.goals.is_running("OcelotTemptGoal")
                    && player_feet.is_some_and(|p| {
                        let d = [
                            p[0] - m.position.x,
                            p[1] - m.position.y,
                            p[2] - m.position.z,
                        ];
                        d[0] * d[0] + d[1] * d[1] + d[2] * d[2] < 9.0
                    }) =>
            {
                consume = true;
                let trusts = m.random.next_int_bound(3).unwrap_or(1) == 0;
                if trusts {
                    m.extra.trusting = true;
                }
                ocelot_trust = Some(trusts);
            }
            // HappyGhast.canFallInLove: never, so only a ghastling takes food.
            _ if is_food
                && m.kind.flags.animal
                && species != Species::Parrot
                && (species != Species::HappyGhast || m.is_baby()) =>
            {
                // Animal.mobInteract: breed adults, speed up babies, then
                // playEatingSound (silent unless the species overrides it).
                match feed_animal(m, &mut self.world.random, Some(acting)) {
                    Some(sound) => {
                        consume = true;
                        eat = sound;
                        // Axolotl.usePlayerItem: a tropical fish bucket is
                        // emptied into a water bucket in its place.
                        if species == Species::Axolotl && &*item == "minecraft:tropical_fish_bucket"
                        {
                            give = Some("minecraft:water_bucket");
                        }
                        // Animal.setInLove: entity event 18 (a baby that
                        // only grew is not in love).
                        if self.mobs[index].extra.in_love > 0 {
                            let id = self.mobs[index].id;
                            self.love_cause.insert(id, self.lan_acting);
                            let kind = self.mobs[index].kind.id.clone();
                            self.native_entity_enter_love_mode(id, &kind, self.lan_acting_uuid);
                            self.mob_event_particles(id, 18);
                        }
                    }
                    // Animal.mobInteract still consumes the client click when
                    // an adult cannot breed, without swinging or taking food.
                    None => return Ok(true),
                }
            }
            _ => handled = false,
        }
        if !handled {
            return Ok(false);
        }
        if ignited_creeper {
            self.native_creeper_ignite(
                self.mobs[index].id,
                horizon_client::lan::playerdata::ints_uuid(acting),
                &item,
            );
        }
        if let Some(trusts) = ocelot_trust {
            // `spawnTrustingParticles` on the clients: entity event 41 (hearts) or 40 (smoke).
            self.mob_event_particles(self.mobs[index].id, if trusts { 41 } else { 40 });
        }
        if let Some(fresh) = stew_particles {
            // `SpellParticleOption.create(EFFECT, -1, 1)` is the plain white
            // effect particle; the smoke is the already-fed answer.
            let ty = if fresh {
                ParticleType::Effect
            } else {
                ParticleType::Smoke
            };
            let options = ParticleOptions::simple(ty);
            let at = [feet[0], feet[1] + 0.5, feet[2]];
            let (spread, count) = ([0.5, 0.0, 0.5], if fresh { 4 } else { 2 });
            // shortcut: scalar speed models DEFAULT randomization; 26.3 sends
            // ALTERNATIVE_WITH_SPEED with a per-axis maximum speed.
            self.particles
                .send_particles(&options, at, spread, 0.2, count);
            self.lan_send_particles(super::lan_particles::QueuedParticle {
                dimension: self.dim.id(),
                options,
                state: None,
                at,
                spread,
                speed: [0.2; 3],
                count: count as i32,
                randomization: 0,
            });
        }
        // PlayerInteractTrigger: the item used on the mob, once the interaction took.
        if let Some(stack) = &held {
            let target = self.mob_entity(&self.mobs[index]);
            self.trigger_item_entity("minecraft:player_interacted_with_entity", stack, &target);
        }
        // Turtle.ageBoundaryReached: food that finishes a baby's growth.
        if baby_turtle && !self.mobs[index].is_baby() {
            self.handle_mob_events(vec![MobEvent::GiftLoot {
                mob: self.mobs[index].id,
                table: "minecraft:gameplay/turtle_grow",
                at: feet,
            }])?;
        }
        self.swing_arm();
        if let Some(s) = sound {
            let pitch =
                (self.world.random.next_float() - self.world.random.next_float()) * 0.2 + 1.0;
            self.audio
                .play(&format!("minecraft:{s}"), Some(center), 1.0, pitch);
        }
        if let Some((s, volume, pitch)) = eat {
            let event = format!("minecraft:{s}");
            if matches!(
                self.mobs[index].species(),
                Species::Frog | Species::Goat | Species::Sniffer
            ) {
                self.play_mob_bound_sound(index, &event, 6, volume, pitch, false);
            } else {
                self.audio.play(&event, Some(feet), volume, pitch);
            }
        }
        if consume
            && !creative
            && let Some(stack) = self.stack_in_mut(selected).as_mut()
        {
            stack.count -= 1;
            if stack.count == 0 {
                *self.stack_in_mut(selected) = None;
            }
        }
        if let Some(g) = give {
            let mut stack = ItemStack {
                item: g.into(),
                count: 1,
                damage: 0,
                extra: None,
            };
            if let Some(effects) = give_stew {
                stack.extra = Some(Box::new(horizon_client::item_data::StackData {
                    stew_effects: effects,
                    ..Default::default()
                }));
            }
            if self.stack_in(selected).is_none() {
                *self.stack_in_mut(selected) = Some(stack);
            } else if let Some(rest) = self.give(stack) {
                let at = self.eye_position().unwrap_or(center);
                self.spawn_item(rest, at, [0.0, 0.0, 0.0], 40);
            }
        }
        if let Some(stack) = armor_drop {
            // Mob.shearItem: PLAYER_SHEARED_EQUIPMENT with the armor and the wolf.
            let target = self.mob_entity(&self.mobs[index]);
            self.trigger_item_entity("minecraft:player_sheared_equipment", &stack, &target);
            self.spawn_item(stack, [center[0], center[1] + 0.5, center[2]], [0.0; 3], 10);
        }
        if let Some(equip) = equip_event {
            let id = self.mobs[index].id;
            let pos = self.mobs[index].position;
            self.mob_equip_event(id, [pos.x, pos.y, pos.z], equip);
        }
        if let Some((at, height)) = shear_puff {
            self.mooshroom_shear_puff(at, height);
        }
        let mut drops: Vec<ItemStack> = drops
            .into_iter()
            .map(|(item, count)| ItemStack::new(&item, count))
            .collect();
        if !drops.is_empty() {
            self.native_use_drops(
                &drops_table,
                feet,
                None,
                Some((mob.0, &mob.1)),
                held.as_ref(),
                &mut drops,
            );
        }
        for stack in drops {
            for _ in 0..stack.count {
                // `spawnAtLocation(stack, 1.0)`: an ItemEntity a block above
                // the feet that starts with the constructor's own motion, and
                // `setDefaultPickUpDelay`. Sheep.shear adds its spread on top;
                // MushroomCow.shear builds the entity by hand instead, so its
                // mushrooms keep the constructor's motion and no pickup delay.
                let mut spread = [0.0; 3];
                if wool {
                    spread[0] = (self.world.random.next_float() - self.world.random.next_float())
                        as f64
                        * 0.1;
                    spread[1] = self.world.random.next_float() as f64 * 0.05;
                    spread[2] = (self.world.random.next_float() - self.world.random.next_float())
                        as f64
                        * 0.1;
                }
                self.spawn_item_own(
                    ItemStack {
                        count: 1,
                        ..stack.clone()
                    },
                    [feet[0], feet[1] + 1.0, feet[2]],
                    if wool { 10 } else { 0 },
                );
                if let Some(e) = self.just_spawned_item() {
                    e.velocity = e
                        .velocity
                        .add_vector(Vec3::new(spread[0], spread[1], spread[2]));
                }
            }
        }
        Ok(true)
    }
    /// `SnowGolem.mobInteract` and `Bogged.mobInteract` with shears: the
    /// pumpkin or the mushrooms come off (the shearing loot table), the shear
    /// game event is posted and the tool takes one damage.
    fn interact_shear_mob(&mut self, index: usize, item: &str) -> bool {
        let m = &self.mobs[index];
        if item != "minecraft:shears" || !m.alive() || m.extra.sheared {
            return false;
        }
        let (sound, table, height) = match m.species() {
            Species::SnowGolem => (
                "minecraft:entity.snow_golem.shear",
                "minecraft:shearing/snow_golem",
                m.eye_height(),
            ),
            Species::Bogged => (
                "minecraft:entity.bogged.shear",
                "minecraft:shearing/bogged",
                f64::from(m.height()),
            ),
            _ => return false,
        };
        let at = m.position;
        let mob = (m.id, super::native_hooks::mob_kind(m));
        let tool = self.held().cloned();
        self.play_mob_bound_sound(index, sound, 7, 1.0, 1.0, false);
        self.mobs[index].extra.sheared = true;
        let mut stacks = self.sequence_loot(table, table, tool.as_ref());
        let origin = [at.x, at.y, at.z];
        self.native_use_drops(
            table,
            origin,
            None,
            Some((mob.0, &mob.1)),
            tool.as_ref(),
            &mut stacks,
        );
        for stack in stacks {
            self.spawn_at_location(at, height, stack);
        }
        let ctx = self.who_context(Some(horizon_client::mobs::Who::Player), None);
        self.post_game_event("minecraft:shear", [at.x, at.y, at.z], ctx);
        self.damage_held_tool();
        true
    }
    /// `Armadillo.mobInteract`: a brush brushes a scute off an adult
    /// (`brushOffScute`) and loses 16 durability.
    fn interact_armadillo(&mut self, index: usize, item: &str) -> bool {
        let m = &self.mobs[index];
        if item != "minecraft:brush"
            || m.species() != Species::Armadillo
            || !m.alive()
            || m.is_baby()
        {
            return false;
        }
        let (id, at) = (m.id, m.position);
        let feet = [at.x, at.y, at.z];
        let tool = self.held().cloned();
        let table = "minecraft:brush/armadillo";
        let mut stacks = self.sequence_loot(table, table, tool.as_ref());
        let kind = super::native_hooks::mob_kind(&self.mobs[index]);
        self.native_use_drops(
            table,
            feet,
            None,
            Some((id, &kind)),
            tool.as_ref(),
            &mut stacks,
        );
        for d in stacks {
            self.spawn_at_location(at, 0.0, d);
        }
        self.audio
            .play("minecraft:entity.armadillo.brush", Some(feet), 1.0, 1.0);
        let ctx = self.who_context(Some(Who::Mob(id)), None);
        self.post_game_event("minecraft:entity_interact", feet, ctx);
        // ItemStack.hurtAndBreak(16).
        if !self.creative
            && let Some(stack) = tool
        {
            let max = self
                .res
                .items()
                .get(&stack.item)
                .map_or(0, |i| i.max_damage);
            if max > 0 {
                self.damage_held(16);
            }
        }
        true
    }
    /// `ItemStack.hurtAndBreak(1, ...)` on the held tool: Unbreaking, break sound and stat.
    pub(super) fn damage_held_tool(&mut self) {
        self.damage_held(1);
    }
    /// Throwables and spawn eggs (SnowballItem/EggItem/EnderpearlItem/SpawnEggItem.use).
    pub(super) fn throw_item(&mut self, stack: &ItemStack) -> Result<bool> {
        let item = &*stack.item;
        let kind = match item {
            "minecraft:snowball" => ProjectileKind::Snowball,
            "minecraft:egg" | "minecraft:blue_egg" | "minecraft:brown_egg" => ProjectileKind::Egg,
            "minecraft:ender_pearl" => ProjectileKind::EnderPearl,
            "minecraft:wind_charge" => {
                let thrown = self.throw_wind_charge()?;
                if thrown {
                    self.apply_use_cooldown(stack);
                }
                return Ok(thrown);
            }
            _ if item.ends_with("_spawn_egg") => return self.use_spawn_egg(stack),
            _ => match horizon_client::projectiles::thrown_by_item(item) {
                Some(t) => ProjectileKind::Thrown(t),
                None => return Ok(false),
            },
        };
        let Some(eye) = self.eye_position() else {
            return Ok(false);
        };
        // The mods, before the projectile exists: a canceled throw is a
        // failed use, and the guest's client gets back the item it took.
        if let (Some(player), Some(name)) = (self.lan_acting_uuid, self.lan_acting_name.clone()) {
            let mut cancel = false;
            self.native_player_launch_projectile_v2(player, &name, stack, &mut cancel);
            if cancel {
                self.use_failed = true;
                self.guest_inventory_resend = true;
                return Ok(true);
            }
        }
        let Some(p) = self.player.as_ref() else {
            return Ok(false);
        };
        // Projectile.shootFromRotation adds getKnownMovement: what the thrower
        // last moved, not a velocity the server kept for a guest.
        let on_ground = p.ground.on_ground;
        let velocity = self.known_player_movement();
        let start = Vec3::new(eye[0], eye[1] - 0.1, eye[2]);
        let mut proj = Projectile::new(kind.clone(), start, Some(Who::Player));
        proj.shoot_from_rotation(
            &mut self.world.random,
            self.pitch,
            self.yaw,
            0.0,
            1.5,
            1.0,
            velocity,
            on_ground,
        );
        self.add_projectile(proj);
        // SnowballItem.use / EggItem.use / EnderpearlItem.use: awardStat(ITEM_USED).
        self.award_stat("used", item, 1);
        // ItemStack.use's applyAfterUseComponentSideEffects: the pearl's
        // second is the `use_cooldown` component, as for any thrown item.
        self.apply_use_cooldown(stack);
        let sound = match kind {
            ProjectileKind::Snowball | ProjectileKind::Thrown(_) => {
                "minecraft:entity.snowball.throw"
            }
            ProjectileKind::Egg => "minecraft:entity.egg.throw",
            _ => "minecraft:entity.ender_pearl.throw",
        };
        let pitch = 0.4 / (self.world.random.next_float() * 0.4 + 0.8);
        self.audio.play(sound, Some(eye), 0.5, pitch);
        self.swing_arm();
        if !self.creative
            && let Some(s) = self.held_mut().as_mut()
        {
            s.count -= 1;
            if s.count == 0 {
                *self.held_mut() = None;
            }
        }
        if kind == ProjectileKind::Egg
            && let (Some(player), Some(name)) = (self.lan_acting_uuid, self.lan_acting_name.clone())
        {
            self.native_player_egg_throw(player, &name);
        }
        Ok(true)
    }
    /// `Mob.checkAndHandleImportantInteractions` with a spawn egg of mob
    /// `index`'s own kind (`SpawnEggItem.spawnOffspringFromSpawnEgg`): a baby
    /// at the mob's feet, taking the parent's variant as `getBreedOffspring`
    /// does, and one egg used. False (PASS) when the mob has no baby form.
    fn spawn_egg_offspring(&mut self, index: usize) -> bool {
        let Some(stack) = self.held().cloned() else {
            return false;
        };
        let parent = &self.mobs[index];
        if !parent.alive()
            || !horizon_client::mobs::finalize::spawn_egg_is_for(&stack.item, &parent.kind.id)
            || !horizon_client::mobs::species::can_be_a_baby(parent.species())
            // WanderingTrader.getBreedOffspring returns null: no offspring, the
            // click carries on to trading.
            || parent.species() == Species::WanderingTrader
        {
            return false;
        }
        let (kind, at) = (
            parent.kind.id.clone(),
            [parent.position.x, parent.position.y, parent.position.z],
        );
        let (species, variant, color) =
            (parent.species(), parent.extra.variant, parent.extra.color);
        let target = self.mob_entity(&self.mobs[index]);
        // SpawnEggItem.spawnOffspringFromSpawnEgg: `type.create(level, SPAWN_ITEM_USE)`
        // creates the baby without finalizeSpawn, and `snapTo(pos, 0, 0)` faces
        // it south without a level-random draw.
        let Some(id) =
            self.spawn_mob_internal(&kind, at, false, false, false, false, SpawnYaw::Fixed(0.0))
        else {
            return false;
        };
        if let Some(child) = self.mobs.iter_mut().find(|m| m.id == id) {
            horizon_client::mobs::finalize::spawn_egg_offspring(child, species, variant, color);
        }
        // `Mob.checkAndHandleImportantInteractions` then calls
        // `onOffspringSpawnedFromEgg`: `Fox`'s makes the baby trust the player
        // who used the egg (a fox egg used on a block trusts nobody).
        if species == Species::Fox {
            let spawner = self.acting_player_uuid();
            if let Some(child) = self.mobs.iter_mut().find(|m| m.id == id) {
                horizon_client::mobs::fox::add_trusted_entity(child, spawner);
            }
        }
        self.trigger_item_entity("minecraft:player_interacted_with_entity", &stack, &target);
        self.consume_held();
        true
    }
    /// `SpawnEggItem.use`: the SOURCE_ONLY ray must hit a liquid block.
    pub(super) fn spawn_egg_fluid_hit(&self) -> Option<super::Hit> {
        let hit = self.bucket_hit()?;
        matches!(
            &*self.block_name(hit.pos),
            "minecraft:water" | "minecraft:lava"
        )
        .then_some(hit)
    }
    /// Shared peaceful-mode eligibility for spawn eggs.
    pub(super) fn spawn_egg_allowed(&self, item: &str) -> bool {
        let kind = item.trim_end_matches("_spawn_egg");
        self.ui.difficulty != 0
            || self
                .res
                .mob_types()
                .get(kind)
                .is_none_or(|t| horizon_client::mobs::spawn_rules::allowed_in_peaceful(t.species))
    }
    /// `SpawnEggItem.useOn` for a block, or `use` for a fluid source.
    fn use_spawn_egg(&mut self, stack: &ItemStack) -> Result<bool> {
        let fluid = matches!(self.guest_use, Some(super::GuestUse::Item(_)));
        let Some(hit) = (if fluid {
            self.spawn_egg_fluid_hit()
        } else {
            self.pick()
        }) else {
            return Ok(false);
        };
        let kind = stack.item.trim_end_matches("_spawn_egg").to_owned();
        if !self.spawn_egg_allowed(&stack.item) {
            self.use_failed = true;
            return Ok(true);
        }
        if fluid
            && !self.may_build()
            && !self.can_place_on_in_adventure([
                hit.pos[0] - hit.face[0],
                hit.pos[1] - hit.face[1],
                hit.pos[2] - hit.face[2],
            ])
        {
            self.use_failed = true;
            return Ok(true);
        }
        // SpawnEggItem.useOn on a spawner sets its mob (and an op's egg data).
        let (spawner_id, data) = horizon_client::block_entity_item::spawn_egg_on_spawner(
            stack,
            self.acting_player_is_op(),
        );
        if let Some(done) = self.use_spawn_egg_on_spawner(hit.pos, &spawner_id, data.as_ref()) {
            if done {
                self.swing_arm();
                self.award_stat("used", &stack.item, 1);
                self.consume_held();
            }
            return Ok(true);
        }
        // useOn: a clicked block without a collision shape (vines, grass)
        // takes the mob itself, any other the one on the clicked face.
        let p = if fluid || self.collision_empty(hit.pos) {
            hit.pos
        } else {
            [
                hit.pos[0] + hit.face[0],
                hit.pos[1] + hit.face[1],
                hit.pos[2] + hit.face[2],
            ]
        };
        let at = [
            f64::from(p[0]) + 0.5,
            f64::from(p[1]),
            f64::from(p[2]) + 0.5,
        ];
        // SpawnEggItem.spawnMob: `EntityType.spawn`.
        let Some(id) = self.spawn_mob_spawned(&kind, at, SpawnYaw::Created) else {
            return Ok(false);
        };
        // SpawnEggItem.spawnMob: `EntityType.create` finalizes the mob, then
        // `PostSpawnProcessor` puts the egg's components on it.
        if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
            horizon_client::entity_nbt::apply_item_components(m, stack);
        }
        self.apply_spawn_egg_entity_data(id, stack);
        self.post_block_game_event("minecraft:entity_place", p, Some(Who::Player), None);
        self.award_stat("used", &stack.item, 1);
        self.swing_arm();
        if !self.creative
            && let Some(s) = self.held_mut().as_mut()
        {
            s.count -= 1;
            if s.count == 0 {
                *self.held_mut() = None;
            }
        }
        Ok(true)
    }
    /// `EntityType.updateCustomEntityTag`: the egg's `minecraft:entity_data` on
    /// the mob it spawned, once the guards let it through, as
    /// `TypedEntityData.loadInto`: the mob's own compound, the data merged over
    /// it, loaded back into the same mob (`/data merge entity`'s loader).
    pub(super) fn apply_spawn_egg_entity_data(&mut self, id: u32, stack: &ItemStack) {
        let Some(i) = self.mobs.iter().position(|m| m.id == id) else {
            return;
        };
        let kind = self.mobs[i].kind.id.clone();
        let op = self.acting_player_is_op();
        let Some(data) = horizon_client::block_entity_item::spawn_egg_entity_data(stack, &kind, op)
        else {
            return;
        };
        let own = self.mob_tree_nbt(i);
        let tag = horizon_client::data_cmd::merged(&own, data);
        // Nothing to report: the mob's own compound always loads back.
        let _ = self.apply_entity_nbt(super::commands::Target::Mob(id), tag);
    }
    /// A beam target's centre (half its height), interpolated.
    fn beam_target(&self, who: Option<Who>, alpha: f64) -> Option<glam::DVec3> {
        match who? {
            Who::Player => {
                let p = self.player.as_ref()?;
                let (a, b) = (self.previous_position, p.position);
                Some(glam::DVec3::new(
                    a.x + (b.x - a.x) * alpha,
                    a.y + (b.y - a.y) * alpha + f64::from(p.dimensions.height) * 0.5,
                    a.z + (b.z - a.z) * alpha,
                ))
            }
            Who::Mob(id) => {
                let m = self.mobs.iter().find(|m| m.id == id)?;
                Some(glam::DVec3::new(
                    m.previous.x + (m.position.x - m.previous.x) * alpha,
                    m.previous.y
                        + (m.position.y - m.previous.y) * alpha
                        + f64::from(m.height()) * 0.5,
                    m.previous.z + (m.position.z - m.previous.z) * alpha,
                ))
            }
        }
    }
    /// `Guardian.aiStep` on a client: two bubbles a tick trailing a guardian
    /// that swims along, and bubbles up the beam to its target. The server
    /// never draws them, so the mob's own random stays untouched.
    pub(super) fn guardian_bubbles(&mut self) {
        use horizon_client::mobs::guardian;
        use horizon_client::particles::{ParticleOptions, ParticleType};
        let mut swimming = Vec::new();
        let mut beams = Vec::new();
        for m in &self.mobs {
            if !m.alive() || !matches!(m.species(), Species::Guardian | Species::ElderGuardian) {
                continue;
            }
            let at = [m.position.x, m.position.y, m.position.z];
            if m.extra.flag2 && m.fluids.in_water {
                let (width, height, _) = m.dims();
                swimming.push((at, width, height, m.view_vector()));
            }
            if let Some((_, scale)) = guardian::beam(m, 0.0)
                && let Some(t) = self.beam_target(m.ai.target, 1.0)
            {
                beams.push(([at[0], m.eye_y(), at[2]], [t.x, t.y, t.z], f64::from(scale)));
            }
        }
        let bubble = ParticleOptions::simple(ParticleType::Bubble);
        for (at, width, height, view) in swimming {
            for _ in 0..2 {
                let (x, y, z) = {
                    let r = self.particles.random();
                    (r.next_double(), r.next_double(), r.next_double())
                };
                let p = [
                    at[0] + width * (2.0 * x - 1.0) * 0.5 - view[0] * 1.5,
                    at[1] + height * y - view[1] * 1.5,
                    at[2] + width * (2.0 * z - 1.0) * 0.5 - view[2] * 1.5,
                ];
                self.particles.add(&bubble, p, [0.0; 3]);
            }
        }
        for (eye, target, scale) in beams {
            let d = [target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]];
            let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if length <= 0.0 {
                continue;
            }
            let mut dist = self.particles.random().next_double();
            while dist < length {
                let r = self.particles.random();
                dist += 1.8 - scale + r.next_double() * (1.7 - scale);
                let f = dist / length;
                let p = [eye[0] + d[0] * f, eye[1] + d[1] * f, eye[2] + d[2] * f];
                self.particles.add(&bubble, p, [0.0; 3]);
            }
        }
    }
    /// Mob models and projectiles for the entity pass.
    /// The renderer's models for the GPU path (`mobs::skin`) this frame, if it
    /// poses entities: a new frame of draws, the store restarted when the
    /// renderer's no longer matches. Once per frame, before any entity draws.
    pub(super) fn begin_skinning(&self) {
        let renderer = self.renderer.as_ref().filter(|r| r.skins_entities());
        match renderer.and_then(|r| r.skinned_models()) {
            Some(stored) => {
                let mut models = self.skin_models.borrow_mut();
                // Reuse helps CPU-bound crowds, but regressed TAA frame tails in
                // windowed measurements. Keep the original arithmetic with TAA on.
                models.set_rotation_cache_enabled(!self.settings.horizon.taa);
                models.begin(&mut self.skin_frame.borrow_mut(), stored);
                models.tagged = renderer.is_some_and(|r| r.skins_tagged());
            }
            None => self.skin_frame.borrow_mut().clear_draws(),
        }
    }

    /// Whether this frame's models can be posed on the GPU (`begin_skinning`).
    pub(super) fn skinning(&self) -> bool {
        self.renderer
            .as_ref()
            .is_some_and(|r| r.skins_entities() && r.skinned_models().is_some())
    }

    /// Culls one entity's vertices from `entity_start`, counting what it drew
    /// on the GPU path by the corners of `skinned`'s bounds; a culled entity's
    /// posed draws since `skinned`'s mark are taken back.
    pub(super) fn cull_skinned(
        &self,
        out: &mut Vec<Vertex>,
        entity_start: usize,
        skinned: Skinned,
    ) {
        let corners = skinned.and_then(|(_, bounds)| bounds);
        if out.len() == entity_start {
            if let Some((lo, hi)) = corners
                && self.cull_entity_camera_bounds(lo, hi)
                && let Some((mark, _)) = skinned
            {
                if self.pack_shadow_casters.0 {
                    let mut frame = self.skin_frame.borrow_mut();
                    let end = frame.vertices as usize;
                    horizon_client::gfx::pack_draw::EntitySpan::hide(
                        &mut frame.spans,
                        mark.2 as usize,
                        end,
                    );
                } else {
                    self.skin_frame.borrow_mut().truncate(mark);
                }
            }
            return;
        }
        if let Some((lo, hi)) = corners {
            out.extend([lo, hi].map(|pos| Vertex {
                pos: pos.to_array(),
                ..bytemuck::Zeroable::zeroed()
            }));
        }
        let len = out.len();
        let hidden = self.cull_entity_vertices(out, entity_start);
        if out.len() < len {
            if let Some((mark, _)) = skinned {
                self.skin_frame.borrow_mut().truncate(mark);
            }
        } else if corners.is_some() {
            if let Some((mark, _)) = skinned {
                // Previous GPU-only entities can leave zero-length CPU spans at
                // this same offset. Use this query's result, not their metadata.
                if hidden {
                    let mut skin = self.skin_frame.borrow_mut();
                    let end = skin.vertices as usize;
                    horizon_client::gfx::pack_draw::EntitySpan::hide(
                        &mut skin.spans,
                        mark.2 as usize,
                        end,
                    );
                }
            }
            out.truncate(len - 2);
            // The temporary bounds are not real geometry. Keep the culling end marker
            // at the true CPU end before the next entity appends a span there.
            for span in self.entity_spans.borrow_mut().iter_mut().rev() {
                if span.start as usize <= out.len() {
                    break;
                }
                span.start = out.len() as u32;
            }
        }
    }

    pub(super) fn mob_geometry(&self, camera: glam::DVec3, alpha: f64, out: &mut Vec<Vertex>) {
        // The coarse mob bounds pay off only in dense async scenes; otherwise,
        // keep the existing exact mesh-culling path.
        if self.settings.horizon.entity_culling == EntityCullingMode::Asynchronous
            && self
                .async_occlusion_culler
                .as_ref()
                .is_some_and(|culler| culler.camera_is_stationary())
            && self.settings.horizon.entity_culling_precheck != 0
            && self.mobs.len() >= self.settings.horizon.entity_culling_precheck as usize
            && !self.pack_shadow_casters.0
            && horizon_client::mod_api::mods().is_empty()
        {
            self.mob_geometry_inner::<true>(camera, alpha, out);
        } else {
            self.mob_geometry_inner::<false>(camera, alpha, out);
        }
    }

    fn mob_geometry_inner<const PRECHECK: bool>(
        &self,
        camera: glam::DVec3,
        alpha: f64,
        out: &mut Vec<Vertex>,
    ) {
        let atlas = &self.gui.atlas;
        let lookup = |name: &str| atlas.sprites.get(name).map(|r| [r.u0, r.v0, r.u1, r.v1]);
        let scale = self.view_scale();
        self.rendered_mobs.set(0);
        let skinning = self.skinning();
        // What custom entity model animations read of the frame.
        horizon_client::mobs::cem_anim::set_frame(horizon_client::mobs::cem_anim::Frame {
            time: self.world.game_time as f64 + alpha,
            day_time: self.world.day_time as f64 + alpha,
            dimension: match self.dim {
                Dim::Overworld => 0.0,
                Dim::Nether => -1.0,
                Dim::End => 1.0,
            },
            player: self.player.as_ref().map_or(camera.to_array(), |p| {
                [p.position.x, p.position.y, p.position.z]
            }),
            player_rot: [self.pitch, self.yaw],
            paused: self.pause,
            first_person: self.camera_type.first_person(),
            third_person_reversed: self.camera_type
                == super::perspective::CameraType::ThirdPersonFront,
            ..Default::default()
        });
        for mob in &self.mobs {
            // LevelRenderer: the entity the view looks out of is not drawn
            // in first person.
            if self.hides_as_camera(mob.id) {
                continue;
            }
            let is_player = &*mob.kind.id == "minecraft:player";
            let d = glam::DVec3::new(mob.position.x, mob.position.y, mob.position.z) - camera;
            // Entity.shouldRenderAtSqrDistance: 64 times the bounding box's
            // mean edge, scaled by the view scale. RemotePlayer's box counts
            // ten times.
            let (w, h, _) = mob.dims();
            let edge = (w + h + w) / 3.0 * if is_player { 10.0 } else { 1.0 };
            let range = edge * 64.0 * scale;
            if d.length_squared() >= range * range {
                continue;
            }
            let beam = horizon_client::mobs::guardian::beam(mob, alpha as f32);
            // EntityRenderer.shouldRender: the renderer's interpolated culling box,
            // grown by 0.5 (GuardianRenderer also draws while its beam is out).
            if let Some(view) = &self.view_frustum
                && beam.is_none()
            {
                let (lo, hi) = super::entity_culling::cull_box(mob, alpha);
                let (lo, hi) = (
                    (lo - camera - glam::DVec3::splat(0.5)).as_vec3(),
                    (hi - camera + glam::DVec3::splat(0.5)).as_vec3(),
                );
                if !view.intersects(lo, hi) {
                    continue;
                }
            }
            let outline = self.mob_outline(mob);
            // Entity.DATA_SHARED_FLAGS_ID invisible: only the name tag draws (and
            // the outline of a glowing mob, LivingEntityRenderer.getRenderType).
            // A player keeps its armour, elytra and held items
            // (`LivingEntityRenderer.submit` skips only the body).
            let layers_only = mob.invisible && outline.is_none() && is_player;
            if mob.invisible && outline.is_none() && !layers_only {
                continue;
            }
            let culling = if PRECHECK
                && outline.is_none()
                && !is_player
                && mob.kind.id.starts_with("minecraft:")
                && [
                    &mob.extra.head,
                    &mob.extra.chest,
                    &mob.extra.legs,
                    &mob.extra.feet,
                    &mob.extra.main_hand,
                    &mob.extra.off_hand,
                ]
                .iter()
                .all(|item| item.is_none())
                && beam.is_none()
            {
                self.entity_cull_decision(mob, alpha)
            } else {
                EntityCullDecision::CheckVertices
            };
            if culling.hides_entity() {
                continue;
            }
            let entity_start = out.len();
            self.mark_motion(entity_start, (1 << 32) | u64::from(mob.id));
            self.mark_entity(entity_start, &mob.kind.id);
            self.rendered_mobs.set(self.rendered_mobs.get() + 1);
            // `EntityRenderer.getPackedLightCoords`: the block the mob's light
            // probe (its eye) is in, at the position this frame draws rather
            // than this tick's.
            let probe = mob.light_probe(alpha);
            let light = self.world.light(probe[0], probe[1], probe[2]);
            let mut packed = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
            // EntityRenderer.getBlockLightLevel: a burning mob has 15 block
            // light, and so do the Allay, Blaze, Magma Cube, Vex and Wither
            // renderers' overrides; the sky light stays the level's.
            if mob.fire_ticks > 0
                || matches!(
                    mob.species(),
                    Species::Allay
                        | Species::Blaze
                        | Species::MagmaCube
                        | Species::Vex
                        | Species::Wither
                )
            {
                packed = (packed & 0xFF) | ((15 * 16) << 8);
            }
            // GlowSquidRenderer.getBlockLightLevel: the body brightens as the
            // dark ticks run out (the block light is the second byte).
            if mob.species() == Species::GlowSquid {
                let lit = (packed >> 12) & 15;
                let block = horizon_client::mobs::aquatic::glow_squid_block_light(mob, lit);
                packed = (packed & 0xFF) | ((block * 16) << 8);
            }
            // A native mod's own model in place of the vanilla one
            // (`native_mob_submit`), before any of the vanilla model's parts
            // (a sulfur cube's swallowed block) are drawn.
            // A player's own hook (`native_player_submit`) is asked for the
            // other players in view.
            let who = is_player
                .then(|| self.remote_player_name(mob.id))
                .flatten()
                .map(|name| NativePlayerRender { name, local: false });
            let modded = if layers_only {
                false
            } else if let Some(who) = who {
                self.mod_player_geometry(mob, who, alpha as f32, camera, packed, out)
            } else {
                !is_player && self.mod_mob_geometry(mob, alpha as f32, camera, packed, out)
            };
            if modded {
                if let Some(rgb) = outline {
                    self.mark_outline(&out[entity_start..], rgb);
                    if mob.invisible {
                        out.truncate(entity_start);
                        continue;
                    }
                }
                if outline.is_none() {
                    self.cull_entity_vertices(out, entity_start);
                }
                continue;
            }
            if mob.species() == Species::SulfurCube
                && !horizon_client::mobs::trader::is_invisible(mob)
            {
                self.sulfur_cube_body_geometry(
                    mob,
                    alpha as f32,
                    horizon_client::mobs::render::base_transform(mob, alpha as f32, camera),
                    packed,
                    out,
                );
            }
            // Other players on a server: PlayerRenderer's HumanoidArmorLayer.
            let armor = if is_player {
                self.worn_armor_layers(mob)
            } else {
                Vec::new()
            };
            let body = out.len();
            // The GPU path: an ordinary mob, no layers the CPU path changes after drawing them
            // (a player's, the HDR tiers below, the sulfur cube's shell). A glowing mob's
            // silhouette is posed too (`SkinFrame::outline_since`), unless it is invisible:
            // then only the outline draws.
            let silhouette_on_gpu = outline.is_none() || !mob.invisible;
            let posable = skinning
                && !is_player
                && !layers_only
                && silhouette_on_gpu
                && armor.is_empty()
                && !matches!(
                    mob.species(),
                    Species::Blaze | Species::MagmaCube | Species::GlowSquid | Species::SulfurCube
                );
            let mut skinned = None;
            if posable {
                let mut models = self.skin_models.borrow_mut();
                let mut frame = self.skin_frame.borrow_mut();
                let mark = frame.mark();
                let mut skin = horizon_client::mobs::skin::Skin::new(&mut models, &mut frame);
                horizon_client::mobs::render::render_skinned(
                    mob,
                    alpha as f32,
                    camera,
                    &lookup,
                    packed,
                    self.res.mob_data(),
                    out,
                    &mut skin,
                );
                skinned = Some((mark, skin.bounds));
            } else {
                let render = if layers_only {
                    horizon_client::mobs::render::render_layers
                } else {
                    horizon_client::mobs::render::render_with
                };
                render(
                    mob,
                    alpha as f32,
                    camera,
                    &lookup,
                    packed,
                    self.res.mob_data(),
                    &armor,
                    out,
                );
            }
            // HDR output: mobs that are light shine by their kind, as blocks do (entity.wgsl
            // fs_hdr_mark); a tier's window lifts only their bright texels.
            if let Some(tier) = match mob.species() {
                Species::Blaze => Some(horizon_client::mesher::BRIGHT),
                Species::MagmaCube | Species::GlowSquid => Some(horizon_client::mesher::LOW),
                _ => None,
            } {
                for v in &mut out[body..] {
                    v.layer |= u32::from(tier) << 16;
                }
            }
            // AvatarRenderer.shouldRenderLayers: a spectator draws no layers.
            let layers = horizon_client::mobs::render::layers_shown(mob);
            if is_player && layers {
                // PlayerRenderer's CapeLayer, WingsLayer, ParrotOnShoulderLayer
                // and SpinAttackEffectLayer. Only the cape hides with the body;
                // the others have no `isInvisible` check.
                let chest = mob.extra.chest.as_deref();
                if !layers_only {
                    self.player_cape_geometry(mob, chest, camera, alpha as f32, packed, out);
                }
                if let Some((_, name)) = self
                    .remote_player_names()
                    .into_iter()
                    .find(|(id, _)| *id == mob.id)
                {
                    self.player_ears_geometry(mob, &name, camera, alpha as f32, packed, out);
                }
                self.player_elytra_geometry(mob, chest, camera, alpha as f32, packed, out);
                self.shoulder_parrots_geometry(mob, camera, alpha as f32, packed, out);
                self.spin_attack_geometry(mob, camera, alpha as f32, packed, out);
                self.stuck_in_body_geometry(mob, mob.id as i32, camera, alpha as f32, packed, out);
            }
            // Cosmetics (game/cosmetics.rs), posed on the GPU with the mob's own jobs: culled
            // with it.
            if !layers_only
                && super::cosmetics::enabled()
                && (is_player || &*mob.kind.id == "minecraft:mannequin")
            {
                let mark = self.skin_frame.borrow().mark();
                let base = horizon_client::mobs::render::base_transform(mob, alpha as f32, camera);
                let gpu = skinning;
                if let Some((lo, hi)) = self.cosmetics_geometry(
                    mob,
                    false,
                    base,
                    Some(camera),
                    alpha as f32,
                    packed,
                    gpu,
                    out,
                ) {
                    skinned = Some(match skinned {
                        Some((first, Some((l, h)))) => (first, Some((l.min(lo), h.max(hi)))),
                        Some((first, None)) => (first, Some((lo, hi))),
                        None => (mark, Some((lo, hi))),
                    });
                }
            }
            // CustomHeadLayer (a pumpkin, skull or head item) included.
            if layers {
                // Held items join the GPU path's draws, their silhouette too.
                let skin = (skinning && silhouette_on_gpu).then_some(&mut skinned);
                self.mob_block_geometry(mob, camera, alpha, packed, skin, out);
                // `native_mob_layer`: a native mod's layer, over the vanilla ones.
                self.mod_mob_layer_geometry(mob, who, alpha as f32, camera, packed, out);
            }
            if let Some(rgb) = outline {
                self.mark_outline(&out[entity_start..], rgb);
                if let Some(((first, _, _), _)) = skinned {
                    let color = horizon_client::mobs::render::abgr(rgb);
                    self.skin_frame.borrow_mut().outline_since(first, color);
                }
                if mob.invisible {
                    out.truncate(entity_start);
                    continue;
                }
            }
            // Glowing outlines and a guardian's beam remain visible independently
            // of whether the body is hidden behind terrain.
            if outline.is_none() && culling.checks_vertices() && beam.is_none() {
                // What the GPU draws of the mob counts by its bounds' corners.
                self.cull_skinned(out, entity_start, skinned);
            }
            // GuardianRenderer: the laser to the locked target.
            if let Some((time, scale)) = beam
                && let Some(target) = self.beam_target(mob.ai.target, alpha)
                && let Some(uv) = lookup("entity/guardian/guardian_beam")
            {
                let a = alpha;
                let eye = glam::DVec3::new(
                    mob.previous.x + (mob.position.x - mob.previous.x) * a,
                    mob.previous.y + (mob.position.y - mob.previous.y) * a + mob.eye_height(),
                    mob.previous.z + (mob.position.z - mob.previous.z) * a,
                ) - camera;
                horizon_client::mobs::guardian::beam_geometry(
                    eye.as_vec3(),
                    (target - camera).as_vec3(),
                    time,
                    scale,
                    uv,
                    out,
                );
            }
        }
        // What follows isn't a tracked mob: close the last one's span.
        self.mark_motion(out.len(), 0);
        self.elder_apparition_geometry(alpha, out);
        self.spawner_geometry(camera, alpha, out);
        // Projectiles: the baked vanilla arrow model, the rest as item billboards.
        let (right, up) = thrown_item_axes(self.camera_type, self.yaw, self.pitch);
        let live = self.creatures.projectiles.iter().map(|a| {
            let at = [
                a.previous.x + (a.position.x - a.previous.x) * alpha,
                a.previous.y + (a.position.y - a.previous.y) * alpha,
                a.previous.z + (a.position.z - a.previous.z) * alpha,
            ];
            (a, at)
        });
        // ItemPickupParticle: a picked-up arrow flying to its collector.
        let taken = self.pickups.live.iter().filter_map(|p| match &p.taken {
            super::pickups::Taken::Arrow(a) => Some((&**a, super::pickups::position(p, alpha))),
            _ => None,
        });
        // A glowing projectile's outline: what it drew up to the next one.
        let mut glowing = None;
        for (a, at) in live.chain(taken) {
            if let Some((start, rgb)) = glowing.take() {
                self.mark_outline(&out[start..], rgb);
            }
            if a.id != 0
                && let Some(rgb) = self.thing_outline(a.id, a.kind.id())
            {
                glowing = Some((out.len(), rgb));
            }
            let p = glam::DVec3::from_array(at) - camera;
            let c = p.as_vec3();
            // EntityRenderer.getPackedLightCoords: the block its light probe
            // (the eye) is in, at the position this frame draws rather than
            // this tick's, and not the one under its feet.
            let light = self.world.light(
                at[0].floor() as i32,
                a.light_probe_y(alpha).floor() as i32,
                at[2].floor() as i32,
            );
            let mut packed = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
            // EntityRenderer.getBlockLightLevel: a shulker bullet and anything
            // on fire are drawn at 15 block light (the fireball renderers'
            // own override); the sky light stays the level's.
            if let Some(block) = a.block_light_override() {
                packed = (packed & 0xFF) | (u32::from(block) * 16) << 8;
            }
            if a.is_arrow() {
                let Some(model) = self.arrow_model.as_ref() else {
                    continue;
                };
                // SpectralArrowRenderer, and TippableArrowRenderer's `getColor() > 0`.
                let texture = if a.is_spectral_arrow() {
                    "entity/projectiles/arrow_spectral"
                } else if a.arrow.as_ref().is_some_and(|s| s.effect_color > 0) {
                    "entity/projectiles/arrow_tipped"
                } else {
                    "entity/projectiles/arrow"
                };
                let Some(r) = atlas.sprites.get(texture).copied() else {
                    continue;
                };
                // ArrowRenderer: `Mth.lerp(partialTick, state.yRotO, state.yRot)`
                // and the same for the pitch, so a turning arrow is drawn
                // between the two ticks it turned over.
                let alpha32 = alpha as f32;
                let yaw = a.previous_yaw + alpha32 * (a.yaw - a.previous_yaw);
                let pitch = a.previous_pitch + alpha32 * (a.pitch - a.previous_pitch);
                let m = glam::Mat4::from_translation(c)
                    * glam::Mat4::from_rotation_y((yaw - 90.0).to_radians())
                    * glam::Mat4::from_rotation_z(pitch.to_radians());
                // ArrowRenderer: `state.shake = shakeTime - partialTicks`; ArrowModel.setupAnim
                // rolls the root while the shake timer runs.
                let shake = a
                    .arrow
                    .as_ref()
                    .map_or(0.0, |s| s.shake as f32 - alpha as f32);
                let roll = if shake > 0.0 {
                    -(shake * 3.0).msin() * shake * (std::f32::consts::PI / 180.0)
                } else {
                    0.0
                };
                let pose_for = |part: &horizon_client::mobs::Part| {
                    let mut pose = part.pose;
                    if std::ptr::eq(part, model) {
                        pose[5] += roll;
                    }
                    pose
                };
                horizon_client::mobs::render_model(
                    model,
                    m,
                    &pose_for,
                    [r.u0, r.v0, r.u1, r.v1],
                    packed,
                    0,
                    out,
                );
                if a.arrow.as_ref().is_some_and(|s| s.on_fire()) {
                    flame_geometry(atlas, c, 0.5, 0.5, self.yaw, light, out);
                }
                continue;
            }
            // ThrownTridentRenderer.
            if matches!(a.kind, ProjectileKind::Trident) {
                let Some(model) = self.trident_model.as_ref() else {
                    continue;
                };
                let Some(r) = atlas.sprites.get("entity/trident/trident").copied() else {
                    continue;
                };
                let m = glam::Mat4::from_translation(c)
                    * glam::Mat4::from_rotation_y((a.yaw - 90.0).to_radians())
                    * glam::Mat4::from_rotation_z((a.pitch + 90.0).to_radians());
                let poses = horizon_client::mobs::anim::Poses::new(model);
                horizon_client::mobs::render::draw(
                    model,
                    m,
                    &poses,
                    [r.u0, r.v0, r.u1, r.v1],
                    0xFFFF_FFFF,
                    packed,
                    0,
                    out,
                );
                continue;
            }
            // WindChargeRenderer.
            if matches!(
                a.kind,
                ProjectileKind::WindCharge | ProjectileKind::BreezeWindCharge
            ) {
                self.wind_charge_geometry(a, c, packed, alpha as f32, out);
                continue;
            }
            // ShulkerBulletRenderer: the tumbling bullet model, then a larger
            // translucent copy of it (colour 0x26FFFFFF); always 15 block light.
            if matches!(a.kind, ProjectileKind::ShulkerBullet { .. }) {
                let model = &self.entity_models.shulker_bullet;
                if let (Some(model), Some(r)) =
                    (model, atlas.sprites.get("entity/shulker/spark").copied())
                {
                    use horizon_core::geometry::trig;
                    let tc = a.age as f32 + alpha as f32;
                    let mut poses = horizon_client::mobs::anim::Poses::new(model);
                    poses.yrot("main", a.yaw.to_radians());
                    poses.xrot("main", a.pitch.to_radians());
                    let light = (packed & 0xFF) | ((15 * 16) << 8);
                    let base = glam::Mat4::from_translation(c)
                        * glam::Mat4::from_translation(glam::Vec3::new(0.0, 0.15, 0.0))
                        * glam::Mat4::from_rotation_y(
                            (trig::sin(f64::from(tc * 0.1)) * 180.0).to_radians(),
                        )
                        * glam::Mat4::from_rotation_x(
                            (trig::cos(f64::from(tc * 0.1)) * 180.0).to_radians(),
                        )
                        * glam::Mat4::from_rotation_z(
                            (trig::sin(f64::from(tc * 0.15)) * 360.0).to_radians(),
                        )
                        * glam::Mat4::from_scale(glam::Vec3::new(-0.5, -0.5, 0.5));
                    let uv = [r.u0, r.v0, r.u1, r.v1];
                    horizon_client::mobs::render::draw(
                        model,
                        base,
                        &poses,
                        uv,
                        0xFFFF_FFFF,
                        light,
                        0,
                        out,
                    );
                    // The translucent shell blends over the opaque core
                    // (`RenderTypes.entityTranslucent`).
                    let shell = out.len();
                    horizon_client::mobs::render::draw(
                        model,
                        base * glam::Mat4::from_scale(glam::Vec3::splat(1.5)),
                        &poses,
                        uv,
                        0x26FF_FFFF,
                        light,
                        0,
                        out,
                    );
                    horizon_client::gfx::mark_oit(&mut out[shell..]);
                }
                continue;
            }
            // WitherSkullRenderer: the skull model facing its flight.
            if let Some(dangerous) = super::wither::skull_kind(a) {
                let model = &self.entity_models.wither_skull;
                let tex = if dangerous {
                    "entity/wither/wither_invulnerable"
                } else {
                    "entity/wither/wither"
                };
                if let (Some(model), Some(r)) = (model, atlas.sprites.get(tex).copied()) {
                    let mut poses = horizon_client::mobs::anim::Poses::new(model);
                    poses.yrot("head", a.yaw.to_radians());
                    poses.xrot("head", a.pitch.to_radians());
                    let base = glam::Mat4::from_translation(c)
                        * glam::Mat4::from_scale(glam::Vec3::new(-1.0, -1.0, 1.0));
                    horizon_client::mobs::render::draw(
                        model,
                        base,
                        &poses,
                        [r.u0, r.v0, r.u1, r.v1],
                        0xFFFF_FFFF,
                        packed,
                        0,
                        out,
                    );
                }
                continue;
            }
            if let Some(stack) = a.drawn_item() {
                let owner = super::item_draw::owner_at(at, 0.0, a.kind.id());
                let state = self.resolve_item(&stack, DisplayContext::Ground, Some(&owner), 0);
                let pose = glam::Mat4::from_translation(c)
                    * glam::Mat4::from_quat(self.camera_orientation());
                self.submit_item_world(&state, pose, packed, out);
                continue;
            }
            // `Entity.displayFireAnimation`: a thrown item that flew through a
            // fire block burns (`ThrowableProjectile.tick`'s
            // `applyEffectsFromBlocks`), and ThrownItemRenderer draws the flame
            // over its box as any other entity renderer does.
            if a.on_fire() {
                let size = a.kind.size() as f32;
                flame_geometry(atlas, c, size, size, self.yaw, light, out);
            }
            // `ThrownItemRenderer` for a fireball: the item `Fireball.getItem`
            // returns (a fire charge until `setItem` gave it another), its model
            // at the renderer's scale for the kind. `packed` is already 15, the
            // renderer's full bright.
            if let Some(stack) = a.item.as_ref() {
                let owner = super::item_draw::owner_at(at, 0.0, a.kind.id());
                let state = self.resolve_item(stack, DisplayContext::Ground, Some(&owner), 0);
                let pose = glam::Mat4::from_translation(c)
                    * glam::Mat4::from_quat(self.camera_orientation())
                    * glam::Mat4::from_scale(glam::Vec3::splat(a.sprite_scale()));
                self.submit_item_world(&state, pose, packed, out);
                continue;
            }
            let Some(name) = a.item_sprite() else {
                continue;
            };
            let Some(r) = atlas.sprites.get(name).copied() else {
                continue;
            };
            let lift = is_item_billboard(&a.kind);
            let quad = thrown_item_quad(c, right, up, a.sprite_scale(), lift);
            for (v, uv) in quad
                .iter()
                .zip([[r.u0, r.v1], [r.u1, r.v1], [r.u1, r.v0], [r.u0, r.v0]])
            {
                out.push(Vertex {
                    normal: [0; 4],
                    pos: v.to_array(),
                    uv,
                    layer: 0,
                    color: 0xFFFF_FFFF,
                    light: packed,

                    glint_uv: [0.0, 0.0],
                });
            }
        }
        if let Some((start, rgb)) = glowing {
            self.mark_outline(&out[start..], rgb);
        }
    }
}
/// `ThrownItemRenderer.submit` turns the item by `CameraRenderState.orientation`
/// (`Camera.setRotation`): the billboard's right and up axes are the camera's.
/// They stay continuous at pitch ±90, and in third-person front view they are
/// the turned-round camera's, not the player's.
fn thrown_item_axes(
    camera_type: super::perspective::CameraType,
    yaw: f32,
    pitch: f32,
) -> (glam::Vec3, glam::Vec3) {
    let orientation = super::camera_rotation(camera_type, yaw, pitch);
    (orientation * glam::Vec3::X, orientation * glam::Vec3::Y)
}
/// The four corners of the item billboard, in the order its UVs are laid out.
/// `ThrownItemRenderer` only scales and billboards it; the lift is
/// `item/generated`'s ground display transform, `translation [0, 2, 0]` of 16
/// units = 0.125 blocks, which is `0.125 * scale` == `0.5 * s`.
fn thrown_item_quad(
    at: glam::Vec3,
    right: glam::Vec3,
    up: glam::Vec3,
    scale: f32,
    lift: bool,
) -> [glam::Vec3; 4] {
    let s = 0.25 * scale;
    let rise = if lift { 0.5 * s } else { 0.0 };
    let centre = at + up * rise;
    [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
        .map(|k| centre + right * k.0 * s + up * k.1 * s)
}
/// Whether vanilla draws the kind as `ThrownItemRenderer`'s item billboard, the
/// only ones that take that lift. Llama spit shares this quad but
/// `LlamaSpitRenderer` is a model, with its own offset, and a fireball's item
/// is drawn as the item model that carries the lift itself.
fn is_item_billboard(kind: &ProjectileKind) -> bool {
    matches!(
        kind,
        ProjectileKind::Snowball | ProjectileKind::Egg | ProjectileKind::EnderPearl
    )
}
/// `Bucketable.getBucketItemStack` and `getPickupSound` of a mob that a water
/// bucket picks up.
fn bucketable(species: Species) -> Option<(&'static str, &'static str)> {
    let fish = "minecraft:item.bucket.fill_fish";
    Some(match species {
        Species::Cod => ("minecraft:cod_bucket", fish),
        Species::Salmon => ("minecraft:salmon_bucket", fish),
        Species::Pufferfish => ("minecraft:pufferfish_bucket", fish),
        Species::TropicalFish => ("minecraft:tropical_fish_bucket", fish),
        Species::Tadpole => (
            "minecraft:tadpole_bucket",
            "minecraft:item.bucket.fill_tadpole",
        ),
        Species::Axolotl => (
            "minecraft:axolotl_bucket",
            "minecraft:item.bucket.fill_axolotl",
        ),
        _ => return None,
    })
}
/// The damage type id of a mob's damage source (`DamageSources.explosion`: a
/// blast with a causing entity is `player_explosion` whoever it is, an
/// exploding creeper too).
pub(super) fn source_damage_type(d: Damage) -> &'static str {
    damage_type(d.kind, d.attacker.is_some())
}
/// Whether the causing entity deals `kind` itself, so it is the source's
/// direct entity too: a melee hit (`DamageSources.mobAttack`, `playerAttack`,
/// a mace's or a spear's), a sting, a sonic boom, thorns.
pub(super) fn melee(kind: horizon_client::mobs::DamageKind) -> bool {
    use horizon_client::mobs::DamageKind as K;
    matches!(
        kind,
        K::PlayerAttack
            | K::MaceSmash
            | K::Spear
            | K::MobAttack
            | K::Sting
            | K::SonicBoom
            | K::Thorns
    )
}
/// The damage type id of a Horizon damage kind. `caused`: the source has a
/// causing entity (only a blast asks).
pub(super) fn damage_type(kind: horizon_client::mobs::DamageKind, caused: bool) -> &'static str {
    use horizon_client::mobs::DamageKind as K;
    match kind {
        K::Named(named) => named.name,
        K::PlayerAttack => "minecraft:player_attack",
        K::MaceSmash => "minecraft:mace_smash",
        K::Spear => "minecraft:spear",
        K::MobAttack => "minecraft:mob_attack",
        K::Arrow => "minecraft:arrow",
        K::Thrown => "minecraft:thrown",
        K::Fireball => "minecraft:fireball",
        K::Explosion if caused => "minecraft:player_explosion",
        K::Explosion => "minecraft:explosion",
        K::Fall => "minecraft:fall",
        K::FlyIntoWall => "minecraft:fly_into_wall",
        K::FallingBlock => "minecraft:falling_block",
        K::FallingStalactite => "minecraft:falling_stalactite",
        K::OnFire => "minecraft:on_fire",
        K::InFire => "minecraft:in_fire",
        K::Campfire => "minecraft:campfire",
        K::Lava => "minecraft:lava",
        K::Drown => "minecraft:drown",
        K::DryOut => "minecraft:dry_out",
        K::Magic => "minecraft:magic",
        K::IndirectMagic => "minecraft:indirect_magic",
        K::Sting => "minecraft:sting",
        K::Cactus => "minecraft:cactus",
        K::SweetBerryBush => "minecraft:sweet_berry_bush",
        K::Generic => "minecraft:generic",
        K::InWall => "minecraft:in_wall",
        K::Thorns => "minecraft:thorns",
        K::SonicBoom => "minecraft:sonic_boom",
        K::Freeze => "minecraft:freeze",
        K::OutOfWorld => "minecraft:out_of_world",
        K::Cramming => "minecraft:cramming",
        K::FallingAnvil => "minecraft:falling_anvil",
        K::Fireworks => "minecraft:fireworks",
        K::GenericKill => "minecraft:generic_kill",
        K::LightningBolt => "minecraft:lightning_bolt",
        K::OutsideBorder => "minecraft:outside_border",
        K::Spit => "minecraft:spit",
        K::Stalagmite => "minecraft:stalagmite",
        K::Starve => "minecraft:starve",
        K::HotFloor => "minecraft:hot_floor",
        K::SulfurCubeHot => "minecraft:sulfur_cube_hot",
        K::WindCharge => "minecraft:wind_charge",
        K::Wither => "minecraft:wither",
        K::WitherSkull => "minecraft:wither_skull",
    }
}
/// A Horizon item stack as a loot stack (id, count, damage, enchantments,
/// and its custom data as SNBT for `minecraft:custom_data` predicates).
fn to_loot(stack: &ItemStack) -> horizon_world::loot::LootStack {
    let components = stack
        .custom_data()
        .map(|data| {
            let snbt =
                horizon_world::snbt_text::to_snbt(&horizon_world::nbt::Tag::Compound(data.clone()));
            (
                "minecraft:custom_data".to_owned(),
                serde_json::Value::String(snbt),
            )
        })
        .into_iter()
        .collect();
    horizon_world::loot::LootStack {
        item: stack.item.to_string(),
        count: stack.count as i32,
        damage: stack.damage as i32,
        enchantments: stack
            .enchantments()
            .iter()
            .map(|(e, l)| (e.to_string(), *l as i32))
            .collect(),
        potion: None,
        map: false,
        exploration: None,
        ominous_amplifier: None,
        stew_effects: Vec::new(),
        names: Vec::new(),
        components,
    }
}
/// A generated loot stack as a Horizon item stack.
pub(super) fn from_loot(stack: &horizon_world::loot::LootStack) -> ItemStack {
    let mut item = ItemStack {
        item: Arc::from(stack.item.as_str()),
        count: stack.count.max(1) as u32,
        damage: stack.damage.max(0) as u32,
        extra: None,
    };
    for (id, level) in &stack.enchantments {
        item.enchant(id, (*level).max(0) as u32);
    }
    if let Some(potion) = &stack.potion {
        item = item.with_potion(potion);
    }
    if let Some(amplifier) = stack.ominous_amplifier.filter(|a| *a > 0) {
        item.data_mut().ominous_amplifier = Some(amplifier as u8);
    }
    for (id, text) in &stack.names {
        item.data_mut().set_loot_name(id, text);
    }
    for (id, value) in &stack.components {
        item.data_mut().set_loot_component(id, value);
    }
    item
}
impl Game<'_> {
    /// The rest of `NetherPortalBlock.randomTick` after a won roll: with a
    /// player close enough for spawning, a zombified piglin spawns on the block
    /// under the portal column when it is a valid spawn.
    pub(super) fn portal_spawn(&mut self, pos: [i32; 3]) {
        // ChunkMap.anyPlayerCloseEnoughForSpawning: a non-spectator within 128
        // blocks of the chunk centre.
        let centre = [
            f64::from((pos[0] >> 4) * 16 + 8),
            f64::from((pos[2] >> 4) * 16 + 8),
        ];
        let close = self.level_players().iter().any(|p| {
            let (dx, dz) = (p.position.x - centre[0], p.position.z - centre[1]);
            !p.spectator && dx * dx + dz * dz < 16384.0
        });
        if !close {
            return;
        }
        let mut y = pos[1];
        while &*self
            .res
            .facts()
            .get(self.world.block(pos[0], y, pos[2]))
            .name
            == "minecraft:nether_portal"
        {
            y -= 1;
        }
        // isValidSpawn: a sturdy top face (portal frames emit no light).
        let below = self.res.facts().get(self.world.block(pos[0], y, pos[2]));
        if below.sturdy_full & (1 << 1) == 0 {
            return;
        }
        let at = [
            f64::from(pos[0]) + 0.5,
            f64::from(y + 1),
            f64::from(pos[2]) + 0.5,
        ];
        // setPortalCooldown: the new piglin does not travel at once.
        if let Some(id) = self.spawn_mob("minecraft:zombified_piglin", at, false) {
            self.set_portal_cooldown(id);
        }
    }
}
/// `LivingEntity.knockback` on `p`: [`Game::knock_back_player`]; 1.8.9's
/// `EntityLivingBase.knockBack` under `legacy` rules.
fn knock_back(
    p: &mut horizon_client::motion::PlayerMotion,
    random: &mut horizon_core::random::LegacyRandom,
    power: f64,
    (xd, zd): (f64, f64),
    source: HurtSource<'_>,
    legacy: bool,
) {
    let resistance = p.attribute("minecraft:knockback_resistance").unwrap_or(0.0);
    // The native hook runs first, on the raw strength (`LivingKnockBackEvent`), then the
    // resistance scales what it left.
    let mut knock = Knock {
        strength: power,
        x: xd,
        z: zd,
    };
    if knockback_canceled(Hurting::Player(p.entity_id, source), &mut knock) {
        return;
    }
    let Knock {
        strength: power,
        x: mut xd,
        z: mut zd,
    } = knock;
    if legacy && horizon_client::legacy_combat::resists(resistance, random.next_double()) {
        return;
    }
    let power = if legacy {
        power
    } else {
        power * (1.0 - resistance)
    };
    if power <= 0.0 {
        return;
    }
    while xd * xd + zd * zd < f64::from(1.0e-5_f32) {
        xd = (random.next_double() - random.next_double()) * 0.01;
        zd = (random.next_double() - random.next_double()) * 0.01;
    }
    if legacy {
        let v = p.velocity;
        let [x, y, z] = horizon_client::legacy_combat::knockback([v.x, v.y, v.z], power, (xd, zd));
        p.velocity = Vec3::new(x, y, z);
        return;
    }
    let l = (xd * xd + zd * zd).sqrt();
    let v = p.velocity;
    p.velocity = Vec3::new(
        v.x / 2.0 - xd / l * power,
        if p.ground.on_ground {
            (v.y / 2.0 + power).min(0.4)
        } else {
            v.y
        },
        v.z / 2.0 - zd / l * power,
    );
}

/// The instance a mob effect gives the player; `None` for a creative or
/// spectator player (`immune`) unless it is ambient (the nautilus's Breath).
fn mob_effect_instance(
    effect: &'static str,
    ticks: i32,
    amplifier: i32,
    ambient: bool,
    immune: bool,
) -> Option<horizon_client::effects::EffectInstance> {
    if immune && !ambient {
        return None;
    }
    let id = effect.strip_prefix("minecraft:").unwrap_or(effect);
    let mut instance = horizon_client::effects::EffectInstance::new(id, ticks, amplifier);
    instance.ambient = ambient;
    Some(instance)
}

#[cfg(test)]
mod local_sound_event_tests {
    use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
    use horizon_client::mobs::MobEvent;

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn local_mob_events_play_without_entering_the_shared_capture() {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.game_dir =
            Some(std::env::temp_dir().join(format!("horizon-h069-unit-{}", std::process::id())));
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.audio.capture = Some(Vec::new());
        game.handle_mob_events(vec![
            MobEvent::Sound("entity.cow.hurt", [1.0, 100.0, 2.0], 0.5, 1.0),
            MobEvent::LocalSound("entity.breeze.whirl", [3.0, 100.0, 4.0], 0.8, 0.7),
            MobEvent::Sound("entity.zombie.hurt", [5.0, 100.0, 6.0], 1.0, 0.9),
        ])
        .unwrap();
        // A positive LocalSound must still take the playback path. An absent
        // producer cannot satisfy this actual Game adapter regression.
        assert_eq!(
            game.audio.heard.get("minecraft:entity.breeze.whirl"),
            Some(&1)
        );
        assert_eq!(game.audio.heard.get("minecraft:entity.cow.hurt"), Some(&1));
        assert_eq!(
            game.audio.heard.get("minecraft:entity.zombie.hurt"),
            Some(&1)
        );
        let shared = game.audio.capture.as_ref().unwrap();
        assert_eq!(shared.len(), 2, "LocalSound must not become a shared sound");
        assert_eq!(shared[0].event, "minecraft:entity.cow.hurt");
        assert_eq!(shared[1].event, "minecraft:entity.zombie.hurt");
        assert_eq!(
            (
                shared[0].position,
                shared[0].volume,
                shared[0].pitch,
                shared[0].source
            ),
            ([1.0, 100.0, 2.0], 0.5, 1.0, 6)
        );
        assert_eq!(
            (
                shared[1].position,
                shared[1].volume,
                shared[1].pitch,
                shared[1].source
            ),
            ([5.0, 100.0, 6.0], 1.0, 0.9, 5)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::perspective::CameraType;
    use super::mob_effect_instance;
    use super::thrown_item_axes;
    use super::thrown_item_quad;
    use super::{ProjectileKind, is_item_billboard};

    /// The component-wise lowest and highest corner of the quad.
    fn bounds(corners: &[glam::Vec3; 4]) -> (glam::Vec3, glam::Vec3) {
        (
            corners.iter().copied().reduce(glam::Vec3::min).unwrap(),
            corners.iter().copied().reduce(glam::Vec3::max).unwrap(),
        )
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn ender_pearl_clears_impulse_before_next_fall() {
        use super::{Game, Hit, Projectile, Vec3, Who};
        use crate::game::{Audio, Options, Progress, Settings, load_resources};
        use horizon_client::motion::{MotionWorld, PlayerMotion};

        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        let mut player = PlayerMotion::new(game.motion, Vec3::new(0.0, 64.0, 0.0)).unwrap();
        player
            .impulse
            .set_ignore_fall_damage(true, Some(player.position));
        player.fall_distance = 4.0;
        game.player = Some(player);
        let destination = Vec3::new(0.0, 80.0, 0.0);
        let mut pearl = Projectile::new(ProjectileKind::EnderPearl, destination, Some(Who::Player));
        pearl.previous = destination;
        game.projectile_hit(
            &pearl,
            Hit::Block {
                pos: [0, 80, 0],
                at: destination,
                face: [0; 3],
            },
        )
        .unwrap();
        let player = game.player.as_mut().unwrap();
        assert_eq!(player.position, destination);
        assert_eq!(player.fall_distance, 0.0);
        let damaged = MotionWorld {
            entities: &[],
            player,
            scene: &mut game.world,
            assets: &game.pack.assets,
            collision_binding: game.collision,
            resources: game.motion,
            health: &game.health_resources,
        }
        .cause_mount_fall(10.0, 1.0)
        .unwrap();
        assert!(
            damaged,
            "a ten-block fall after the teleport must cause damage"
        );
    }

    #[test]
    fn nautilus_breath_lands_in_creative_as_ambient() {
        // AbstractNautilus.applyEffects: MobEffectInstance(BREATH_OF_THE_NAUTILUS,
        // 60, 0, ambient true, visible true, showIcon true), no game-mode gate.
        let e = mob_effect_instance("minecraft:breath_of_the_nautilus", 60, 0, true, true)
            .expect("applies to a creative player");
        assert_eq!(
            (e.id, e.duration, e.amplifier),
            ("breath_of_the_nautilus", 60, 0)
        );
        assert!(e.ambient && e.visible);
        // Survival gets the same ambient instance.
        let e =
            mob_effect_instance("minecraft:breath_of_the_nautilus", 60, 0, true, false).unwrap();
        assert!(e.ambient);
    }

    #[test]
    fn other_mob_effects_stay_blocked_in_creative() {
        assert!(mob_effect_instance("minecraft:poison", 140, 0, false, true).is_none());
        let e = mob_effect_instance("minecraft:poison", 140, 0, false, false).unwrap();
        assert!(!e.ambient);
    }

    #[test]
    fn thrown_item_billboard_lifts_by_the_generated_ground_transform() {
        // The kinds vanilla draws with `ThrownItemRenderer` as a billboard: the
        // only lifted ones. A fireball hands `getItem` to the item renderer
        // instead, which carries the ground transform itself.
        assert!(is_item_billboard(&ProjectileKind::Snowball));
        assert!(is_item_billboard(&ProjectileKind::Egg));
        assert!(is_item_billboard(&ProjectileKind::EnderPearl));
        assert!(!is_item_billboard(&ProjectileKind::SmallFireball));
        let large = ProjectileKind::LargeFireball { power: 1 };
        assert!(!is_item_billboard(&large));
        // A projectile at y = 100 with right = +X, up = +Y: the sprite sits
        // 0.125 blocks above the entity (2 of `item/generated`'s 16 units) and
        // stays 0.5 blocks wide, so the quad's y runs 99.875 .. 100.375.
        let at = glam::Vec3::Y * 100.0;
        let (right, up) = (glam::Vec3::X, glam::Vec3::Y);
        let lifted = is_item_billboard(&ProjectileKind::Snowball);
        let (lo, hi) = bounds(&thrown_item_quad(at, right, up, 1.0, lifted));
        assert_eq!((lo.y, hi.y), (99.875, 100.375));
        assert_eq!((lo.x, hi.x), (-0.25, 0.25));
        // Llama spit shares the quad here but is a model in vanilla
        // (`LlamaSpitRenderer`): no lift at all.
        let lifted = is_item_billboard(&ProjectileKind::LlamaSpit);
        assert!(!lifted);
        let (lo, hi) = bounds(&thrown_item_quad(at, right, up, 1.0, lifted));
        assert_eq!((lo.y, hi.y), (99.75, 100.25));
        // The rise follows the billboard's own up axis, not world up.
        let (right, up) = (glam::Vec3::Z, glam::Vec3::X);
        let lifted = is_item_billboard(&ProjectileKind::Egg);
        let (lo, hi) = bounds(&thrown_item_quad(at, right, up, 1.0, lifted));
        assert_eq!((lo.x, hi.x), (-0.125, 0.375));
        assert_eq!((lo.y, hi.y), (100.0, 100.0));
    }

    #[test]
    fn thrown_item_billboard_does_not_flip_at_straight_up_or_down() {
        for yaw in [0.0f32, 37.5, 90.0, 180.0, -135.0] {
            for (edge, near) in [(-90.0f32, -89.9f32), (90.0, 89.9)] {
                let (right_edge, up_edge) = thrown_item_axes(CameraType::FirstPerson, yaw, edge);
                let (right_near, up_near) = thrown_item_axes(CameraType::FirstPerson, yaw, near);
                assert!(
                    right_edge.dot(right_near) > 0.99 && up_edge.dot(up_near) > 0.99,
                    "yaw {yaw}, pitch {edge}: {right_edge}/{up_edge} vs {right_near}/{up_near} at {near}"
                );
            }
        }
    }

    #[test]
    fn thrown_item_billboard_faces_the_front_camera() {
        // Camera.setup turns the front camera round (yaw + 180, -pitch): its
        // right is the player's left.
        let (right, up) = thrown_item_axes(CameraType::FirstPerson, 0.0, 0.0);
        assert!(right.abs_diff_eq(glam::Vec3::NEG_X, 1e-5), "{right}");
        assert!(up.abs_diff_eq(glam::Vec3::Y, 1e-5), "{up}");
        let (right, up) = thrown_item_axes(CameraType::ThirdPersonFront, 0.0, 0.0);
        assert!(right.abs_diff_eq(glam::Vec3::X, 1e-5), "{right}");
        assert!(up.abs_diff_eq(glam::Vec3::Y, 1e-5), "{up}");
        let (right, _) = thrown_item_axes(CameraType::ThirdPersonBack, 0.0, 0.0);
        assert!(right.abs_diff_eq(glam::Vec3::NEG_X, 1e-5), "{right}");
    }
}

/// Whether the killing blow's causing entity (`DamageSource.getEntity`) was a
/// mob, such as a tame wolf whose owner gets the kill credit.
fn killed_by_mob(m: &Mob) -> bool {
    m.last_damage
        .is_some_and(|(d, _)| matches!(d.attacker, Some(Who::Mob(_))))
}

/// `Animal.finalizeSpawnChildFromBreeding`'s player: the one who fed the
/// parent, or else the one who fed the partner (`getLoveCause`). `Some(None)`
/// is the host, `Some(Some(conn))` a guest. Both animals are spent.
/// `DyeColor.getMixedColor`: the colour of the crafting recipe that mixes
/// the two dyes, else one of the two picked at random.
fn dye_mix(recipes: &Recipes, random: &mut impl RandomSource, a: u8, b: u8) -> u8 {
    if let Some(mixed) = dye_color_mix_in_recipes(recipes, a, b) {
        return mixed;
    }
    if random.next_boolean() { a } else { b }
}

/// `DyeColor.findColorMixInRecipes`: the dye colour a two-dye crafting recipe
/// makes, when its result is a dye. The two dyes are the grid's first row
/// (`CraftingInput.of(2, 1, ...)`) and the recipe manager answers for it; the
/// result's `_dye` item names the colour.
fn dye_color_mix_in_recipes(recipes: &Recipes, a: u8, b: u8) -> Option<u8> {
    let dye = |c: u8| {
        let name = horizon_client::mobs::render::DYE_NAMES[usize::from(c) & 15];
        Some(ItemStack::new(&format!("minecraft:{name}_dye"), 1))
    };
    let grid = [dye(a), dye(b), None, None];
    let (_, result) = recipes.find(&grid, 2)?;
    let name = result
        .item
        .strip_prefix("minecraft:")?
        .strip_suffix("_dye")?;
    let index = horizon_client::mobs::render::DYE_NAMES
        .iter()
        .position(|n| *n == name)?;
    u8::try_from(index).ok()
}

fn breeding_credit(
    love_cause: &mut std::collections::HashMap<u32, Option<u32>>,
    parent: u32,
    partner: u32,
) -> Option<Option<u32>> {
    let from_parent = love_cause.remove(&parent);
    let from_partner = love_cause.remove(&partner);
    from_parent.or(from_partner)
}

#[cfg(test)]
mod shulker_arrow_tests {
    use super::*;
    use horizon_client::arrows::{ArrowEvent, deflect_reverse};
    use horizon_core::random::LegacyRandom;

    /// `AbstractArrow.onHitEntity` after `hurtOrSimulate` fails and the target
    /// still takes a projectile's side effects: `deflect(REVERSE, …, 0.2)`,
    /// and no hit sound. A closed shulker's refusal is that miss
    /// (`shulker::tests`). An enderman's teleport is `Dodged`, not this.
    #[test]
    fn a_refused_arrow_reverses_without_a_hit_sound() {
        assert_eq!(arrow_miss(false), EntityHit::Refused);
        assert_eq!(arrow_miss(true), EntityHit::Dodged);

        let mut arrow = Projectile::new(
            ProjectileKind::Arrow {
                base_damage: 2.0,
                critical: false,
                effect: None,
            },
            Vec3::new(0.0, 1.0, 0.0),
            Some(Who::Player),
        )
        .with_arrow_random(LegacyRandom::new(7));
        arrow.velocity = Vec3::new(3.0, -1.0, 2.0);
        arrow.yaw = 15.0;
        let mut oracle = LegacyRandom::new(7);
        let expect_yaw = 15.0 + 170.0 + oracle.next_float() * 20.0;
        let mut state = arrow.arrow.take().expect("an arrow");
        deflect_reverse(&mut arrow.velocity, &mut arrow.yaw, &mut state);
        // power 0.2, and REVERSE multiplies by -power * 0.5.
        assert!((arrow.velocity.x - -0.3).abs() < 1e-6);
        assert!((arrow.velocity.y - 0.1).abs() < 1e-6);
        assert!((arrow.velocity.z - -0.2).abs() < 1e-6);
        assert!((arrow.yaw - expect_yaw).abs() < 1e-4);
        assert!(!state.removed, "the reversed arrow stays in the world");
        assert!(
            state
                .events
                .iter()
                .all(|e| !matches!(e, ArrowEvent::Sound { .. })),
            "the deflect branch plays no hit sound"
        );
    }
}

#[cfg(test)]
mod breeding_tests {
    use super::*;

    #[test]
    fn the_player_who_fed_either_animal_is_credited() {
        let mut love = std::collections::HashMap::from([(1, Some(7)), (2, Some(9))]);
        // The parent's feeder first, and both animals are spent.
        assert_eq!(breeding_credit(&mut love, 1, 2), Some(Some(7)));
        assert!(love.is_empty());
        // Only the partner was fed: still credited.
        let mut love = std::collections::HashMap::from([(2, None)]);
        assert_eq!(breeding_credit(&mut love, 1, 2), Some(None));
        // Neither was fed by a player (a bred pair loaded from a save).
        assert_eq!(breeding_credit(&mut love, 1, 2), None);
    }
}

#[cfg(test)]
mod damage_type_tests {
    use super::*;

    #[test]
    fn named_damage_keeps_its_registry_id_with_or_without_an_attacker() {
        let named = horizon_client::mobs::NamedDamage::new(
            "example:fan_fire",
            horizon_client::mobs::NamedDamage::IS_FIRE,
        );
        for attacker in [None, Some(Who::Mob(7)), Some(Who::Player)] {
            assert_eq!(
                source_damage_type(Damage {
                    kind: DamageKind::Named(named),
                    attacker,
                    from: None,
                    attacker_species: None,
                }),
                "example:fan_fire"
            );
        }
    }

    /// `DamageSources.explosion`: a blast with a causing entity is
    /// `player_explosion` whoever caused it, so an exploding creeper's too.
    #[test]
    fn a_creeper_blast_is_a_player_explosion() {
        let blast = |attacker| Damage {
            kind: DamageKind::Explosion,
            attacker,
            from: Some([0.0; 3]),
            attacker_species: None,
        };
        let creeper = blast(Some(Who::Mob(7)));
        assert_eq!(source_damage_type(creeper), "minecraft:player_explosion");
        // No causing entity.
        assert_eq!(source_damage_type(blast(None)), "minecraft:explosion");
        // The other kinds do not ask.
        let bite = Damage {
            kind: DamageKind::MobAttack,
            ..creeper
        };
        assert_eq!(source_damage_type(bite), "minecraft:mob_attack");
    }
}

#[cfg(test)]
mod pearl_tests {
    use super::*;

    /// A pearl's water test is `Entity.isInWater`: `wasTouchingWater` from the
    /// last `updateFluidInteraction`, water whose surface reaches its 0.25
    /// box (#3072), not the block at its feet.
    #[test]
    fn a_pearl_feels_the_water_its_box_touches() {
        // The throwable air drag is the float 0.99 widened to a double.
        let air = f64::from(0.99f32);
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("entity-types.json").exists()
            || !root.join("movement-properties.nbt").exists()
        {
            return;
        }
        let assets = horizon_client::assets::Assets::load(&root).unwrap();
        let render = assets.reload(&root).unwrap();
        let registry = assets.registry;
        let facts = horizon_client::blocks::BlockFacts::load(&root, registry).unwrap();
        let motion = horizon_client::motion::MotionResources::load(&root, &assets).unwrap();
        let data = horizon_client::mobs::MobData::load(&root, &facts);
        let biomes = horizon_client::biome::Biomes::load(&root).unwrap();
        let items = horizon_client::items::Items::load(
            &root,
            registry,
            &facts,
            &horizon_world::loot::PackOverlay::NONE,
        )
        .unwrap();
        let collision =
            std::sync::Arc::new(horizon_client::collisions::load(&root, registry).unwrap());
        let suffocation =
            horizon_client::mobs::suffocation::bind(collision.clone(), &motion.causes_suffocation)
                .unwrap();
        let mut world = horizon_client::world::World::test_scene(&assets, &render, 3).unwrap();
        // A pool of still water one block deep at y = 4, from x = 20 on (clear of the
        // scene's pyramid): its
        // surface is at 4 + 8/9.
        let water = registry.default_state("minecraft:water").unwrap();
        for x in 20..=40 {
            for z in 0..=2 {
                world.set(x, 4, z, water).unwrap();
            }
        }
        let lid_openers = std::collections::HashMap::new();
        let w = MobWorld {
            teams: Default::default(),
            world: &world,
            registry,
            facts: &facts,
            fluid_tables: &motion.fluids,
            movement: &motion.blocks,
            collision: &collision,
            suffocation: &suffocation,
            data: &data,
            player: None,
            controlling_player_movement: None,
            nearest_player: None,
            others: &[],
            dragon: None,
            leash_boats: &[],
            who_lookup: None,
            others_grid: None,
            others_index: None,
            others_villagers: None,
            others_solid: None,
            daylight: true,
            monsters_burn: false,
            game_time: 1000,
            day_time: 1000,
            raining: false,
            thundering: false,
            level: horizon_core::LevelKey::OVERWORLD,
            block_tags: &items.block_tags,
            landing: &motion.landing,
            biomes: &biomes,
            mob_griefing: true,
            universal_anger: false,
            mob_drops: true,
            max_entity_cramming: 24,
            difficulty: spawning::LocalDifficulty::new(2, 1000, 0, 0.0),
            dimension_type: horizon_world::dimension_type::DimensionFacts::overworld(),
            world_seed: 0,
            sea_level: 63,
            items: Some(&items),
            lid_openers: &lid_openers,
            mod_effects: None,
            border: None,
        };
        let pw = ProjWorld {
            w: &w,
            targets: Vec::new(),
            border: None,
        };
        let pearl = |x: f64, y: f64, velocity: Vec3| {
            let mut p = Projectile::new(ProjectileKind::EnderPearl, Vec3::new(x, y, 1.5), None);
            p.velocity = velocity;
            p
        };
        // Skimming the pool: its feet are in the water block, its box above
        // the surface. ThrowableProjectile.applyInertia keeps the air drag.
        let mut p = pearl(25.5, 4.95, Vec3::new(0.3, 0.0, 0.0));
        p.tick(&pw);
        assert!(p.position.y > 4.9, "the pearl dipped: {p:?}");
        p.tick(&pw);
        assert!(
            (p.velocity.x - 0.3 * air * air).abs() < 1e-9,
            "a pearl above the surface slowed as in water: {p:?}"
        );
        // At the pool's edge: its feet over the dry block at x = 19, a side
        // of its box in the water at x = 20. The water drag after the first
        // tick, which still reads `wasTouchingWater` as false.
        let mut p = pearl(19.95, 4.3, Vec3::ZERO);
        p.tick(&pw);
        assert!(
            (p.velocity.y - (-0.03 * air)).abs() < 1e-9,
            "the first tick has the air drag: {p:?}"
        );
        p.tick(&pw);
        assert!(
            (p.velocity.y - (-0.03 * air - 0.03) * 0.8).abs() < 1e-9,
            "a pearl whose box is in the water kept the air drag: {p:?}"
        );
        // Thrown under water, a pearl moves with the air drag on its first
        // tick and the water drag after it.
        let mut p = pearl(30.5, 4.2, Vec3::ZERO);
        p.tick(&pw);
        assert!(
            (p.velocity.y - (-0.03 * air)).abs() < 1e-9,
            "the first tick under water has the air drag: {p:?}"
        );
        p.tick(&pw);
        assert!(
            (p.velocity.y - (-0.03 * air - 0.03) * 0.8).abs() < 1e-9,
            "the second tick under water has the water drag: {p:?}"
        );
    }
}

#[cfg(test)]
mod knockback_tests {
    use super::*;
    use horizon_client::health::KNOCKBACK;

    /// What the hook of these tests saw: the player, its source (as damage
    /// type, causing, direct) and the push.
    type Pushes = std::sync::Mutex<Vec<(i32, (String, i32, i32), Knock)>>;
    static PUSHES: Pushes = std::sync::Mutex::new(Vec::new());

    /// Sets the native knockback hook of these tests (global: it stays set for
    /// the process) for players 9201 to 9203: it records the push, cancels
    /// 9201's and doubles 9202's strength and turns its direction around.
    fn hook(pushed: Hurting<'_>, knock: &mut Knock) -> bool {
        let Hurting::Player(entity @ 9201..=9203, s) = pushed else {
            return false;
        };
        let source = (s.damage_type.to_owned(), s.causing, s.direct);
        PUSHES.lock().unwrap().push((entity, source, *knock));
        match entity {
            9201 => return true,
            9202 => {
                knock.strength *= 2.0;
                knock.z = -knock.z;
            }
            _ => {}
        }
        false
    }

    /// `LivingEntity.knockback` on a player: the hook runs before the
    /// knockback resistance and the motion changes; a mod's strength or
    /// direction pushes, a cancel pushes nothing, and without a mod in the way
    /// the push is vanilla's. Needs the prepared data in
    /// `.local/horizon-client/26.3`; skipped without it.
    #[test]
    fn the_native_knockback_hook_changes_or_cancels_a_players_push() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("movement-properties.nbt").exists() {
            return;
        }
        let installed = *KNOCKBACK.get_or_init(|| hook);
        assert!(std::ptr::fn_addr_eq(
            installed,
            hook as fn(Hurting<'_>, &mut Knock) -> bool
        ));
        let assets = horizon_client::assets::Assets::load(&root).unwrap();
        let resources = horizon_client::motion::MotionResources::load(&root, &assets).unwrap();
        let mut random = horizon_core::random::LegacyRandom::new(1);
        let hit = HurtSource {
            damage_type: "minecraft:player_attack",
            causing: 5,
            direct: 5,
        };
        let mut push = |entity, resistance: f64| {
            let mut p = horizon_client::motion::PlayerMotion::new(&resources, Vec3::ZERO).unwrap();
            p.entity_id = entity;
            p.ground.on_ground = true;
            if resistance != 0.0 {
                let attribute = "minecraft:knockback_resistance";
                let set = p.set_modifier(attribute, "horizon:test", resistance, "add_value", true);
                set.unwrap();
            }
            knock_back(&mut p, &mut random, 0.4, (0.0, 3.0), hit, false);
            (p.velocity.x, p.velocity.y, p.velocity.z)
        };
        // No mod in the way: vanilla's push, away from z = 3.
        assert_eq!(push(9200, 0.0), (0.0, 0.4, -0.4));
        // Canceled: no motion. The hook saw the push and the hit.
        assert_eq!(push(9201, 0.0), (0.0, 0.0, 0.0));
        let made = Knock {
            strength: 0.4,
            x: 0.0,
            z: 3.0,
        };
        let named = ("minecraft:player_attack".to_owned(), 5, 5);
        let seen = |entity| -> Vec<_> {
            let seen = PUSHES.lock().unwrap();
            seen.iter().filter(|p| p.0 == entity).cloned().collect()
        };
        assert_eq!(seen(9201), [(9201, named, made)]);
        // Twice the strength, direction turned around.
        assert_eq!(push(9202, 0.0), (0.0, 0.4, 0.8));
        // Before the resistance (`LivingKnockBackEvent`): a 0.5 resistance still hands the hook
        // the raw strength, and the push that follows is halved.
        assert_eq!(push(9203, 0.5), (0.0, 0.2, -0.2));
        assert_eq!(seen(9203)[0].2.strength, 0.4);
    }
}

#[cfg(test)]
mod dye_mix_tests {
    use super::*;

    /// `DyeColor.getMixedColor` for the offspring a sheep, wolf or cat breed
    /// gets: the colour of the crafting recipe that mixes its parents' two
    /// dyes (vanilla's `crafting_shapeless` rows: red + yellow makes orange,
    /// blue + green cyan, blue + red purple), and a pair no recipe mixes
    /// keeps one of the two.
    #[test]
    fn two_dyes_mix_by_their_crafting_recipe() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("data/minecraft/recipe").is_dir() {
            return;
        }
        let registry = horizon_world::level::data::load_registry(&root).unwrap();
        let facts = crate::blocks::BlockFacts::load(&root, &registry).unwrap();
        let overlay = horizon_world::loot::PackOverlay::NONE;
        let items = crate::items::Items::load(&root, &registry, &facts, &overlay).unwrap();
        let recipes = Recipes::load(&root, &items, &overlay).unwrap();
        let dye = |name: &str| {
            u8::try_from(
                horizon_client::mobs::render::DYE_NAMES
                    .iter()
                    .position(|n| *n == name)
                    .unwrap_or_else(|| panic!("{name} is not a dye colour")),
            )
            .unwrap()
        };
        let mut random = horizon_core::random::LegacyRandom::new(1);
        for (first, second, mixed) in [
            ("red", "yellow", "orange"),
            ("blue", "green", "cyan"),
            ("blue", "red", "purple"),
            ("green", "white", "lime"),
        ] {
            let (a, b, made) = (dye(first), dye(second), dye(mixed));
            assert_eq!(
                dye_color_mix_in_recipes(&recipes, a, b),
                Some(made),
                "{first} + {second} makes {mixed}"
            );
            // The recipe answers, so the coin never has to.
            assert_eq!(dye_mix(&recipes, &mut random, a, b), made);
        }
        // Two dyes that no recipe mixes keep one of the two colours.
        let (orange, cyan) = (dye("orange"), dye("cyan"));
        assert_eq!(dye_color_mix_in_recipes(&recipes, orange, cyan), None);
        let kept = dye_mix(&recipes, &mut random, orange, cyan);
        assert!(kept == orange || kept == cyan, "kept {kept}");
    }
}

impl horizon_client::arrows::OwnerLookup for Game<'_> {
    fn non_player_entity(&self, uuid: [i32; 4]) -> bool {
        let found = self.target_by_uuid(horizon_client::lan::playerdata::ints_uuid(uuid));
        !matches!(
            found,
            None | Some(super::commands::Target::Player | super::commands::Target::Guest(_))
        )
    }
}

#[cfg(test)]
mod conversion_tag_tests {
    use super::*;
    use crate::game::{Audio, Options, Progress, Settings, commands::Target, load_resources};
    use horizon_world::nbt::Tag;

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn converted_mobs_keep_command_tags_at_the_new_id_and_on_save() {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        for (number, (from, to)) in [
            ("husk", "zombie"),
            ("zombie", "drowned"),
            ("skeleton", "stray"),
        ]
        .into_iter()
        .enumerate()
        {
            let uuid = [10374, 1, 0, number as i32];
            let tags = Tag::List(vec![
                Tag::String("subject".into()),
                Tag::String(from.into()),
            ]);
            let raw = Tag::compound([
                (
                    "id",
                    Tag::String(format!("minecraft:{from}").as_str().into()),
                ),
                ("UUID", Tag::IntArray(uuid.to_vec())),
                (
                    "Pos",
                    Tag::List(vec![
                        Tag::Double(0.5_f64.to_bits()),
                        Tag::Double(70.0_f64.to_bits()),
                        Tag::Double(0.5_f64.to_bits()),
                    ]),
                ),
                ("NoAI", Tag::Byte(1)),
                ("NoGravity", Tag::Byte(1)),
                ("Tags", tags.clone()),
                ("horizon:old_entity_only", Tag::Byte(1)),
            ]);
            let old_id = game.load_mob(raw, None).unwrap();
            assert_eq!(game.tags_of(Target::Mob(old_id)), ["subject", from]);
            let source = game.mobs.iter().find(|m| m.id == old_id).unwrap().clone();
            let new_id = game
                .convert_to(
                    &source,
                    &format!("minecraft:{to}"),
                    horizon_client::mobs::conversion::Params::KEEP_ALL,
                )
                .unwrap();
            assert_ne!(old_id, new_id);
            assert_eq!(
                game.tags_of(Target::Mob(new_id)),
                ["subject", from],
                "converted ID must keep tags"
            );
            let index = game.mobs.iter().position(|m| m.id == new_id).unwrap();
            let saved = game.mob_tree_nbt(index);
            assert_eq!(saved.get("Tags"), Some(&tags));
            assert_eq!(
                horizon_client::entity_nbt::kind(&saved).as_deref(),
                Some(format!("minecraft:{to}").as_str())
            );
            assert_ne!(horizon_client::entity_nbt::uuid(&saved), Some(uuid));
            assert!(saved.get("horizon:old_entity_only").is_none());
            game.mobs.remove(index);
            let reloaded = game.load_mob(saved, None).unwrap();
            assert_eq!(game.tags_of(Target::Mob(reloaded)), ["subject", from]);
        }
    }
}

#[cfg(test)]
mod pickable_projectile_tests {
    use super::{pickable_projectile_hit, pickable_projectile_hitboxes};
    use horizon_client::mobs::Who;
    use horizon_client::projectiles::{Hit, Projectile, ProjectileKind, ProjectileWorld};
    use horizon_core::geometry::{Aabb, Vec3};

    /// Open air with nothing in it but `.0`.
    struct Open(Vec<(Who, Aabb)>);
    impl ProjectileWorld for Open {
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

    /// Which of `list` another projectile (the one at 0) can strike.
    fn struck(list: &[Projectile]) -> Vec<usize> {
        let alive = vec![true; list.len()];
        pickable_projectile_hitboxes(list, &alive, 0)
            .iter()
            .map(|(who, _)| match who {
                Who::Mob(id) => pickable_projectile_hit(*id).expect("a pickable projectile id"),
                Who::Player => panic!("not a projectile"),
            })
            .collect()
    }

    /// `Projectile.isPickable` is `#minecraft:redirectable_projectile`: a ghast's
    /// fireball is in it, as both wind charges are, so another projectile's hit
    /// test can strike it. A blaze's small fireball is not, and a projectile
    /// that is stuck is no target.
    #[test]
    fn a_ghast_fireball_is_pickable_like_a_wind_charge() {
        let at = Vec3::new(0.0, 100.0, 0.0);
        let fired = |kind| Projectile::fireball(kind, at, None, [1.0, 0.0, 0.0]);
        let mut stuck = fired(ProjectileKind::LargeFireball { power: 1 });
        stuck.stuck = true;
        let list = vec![
            Projectile::new(ProjectileKind::EnderPearl, at, Some(Who::Player)),
            fired(ProjectileKind::LargeFireball { power: 1 }),
            fired(ProjectileKind::WindCharge),
            fired(ProjectileKind::BreezeWindCharge),
            fired(ProjectileKind::SmallFireball),
            stuck,
        ];
        assert_eq!(struck(&list), vec![1, 2, 3]);
    }

    /// A pearl thrown through a ghast's fireball strikes it (`Projectile.onHit`
    /// then turns the fireball), where it used to fly on through.
    #[test]
    fn a_pearl_thrown_through_a_ghast_fireball_strikes_it() {
        let mut pearl = Projectile::new(
            ProjectileKind::EnderPearl,
            Vec3::new(0.0, 100.0, 0.0),
            Some(Who::Player),
        );
        pearl.velocity = Vec3::new(1.5, 0.0, 0.0);
        let fireball = Projectile::fireball(
            ProjectileKind::LargeFireball { power: 1 },
            Vec3::new(1.0, 99.5, 0.0),
            None,
            [0.0, 0.0, 1.0],
        );
        let list = vec![pearl, fireball];
        let alive = vec![true; list.len()];
        let world = Open(pickable_projectile_hitboxes(&list, &alive, 0));
        let mut pearl = list[0].clone();
        let (keep, hit) = pearl.tick(&world);
        assert!(!keep, "the pearl is spent on the hit");
        match hit {
            Hit::Entity {
                who: Who::Mob(id), ..
            } => assert_eq!(
                pickable_projectile_hit(id),
                Some(1),
                "it struck the fireball"
            ),
            other => panic!("the pearl flew through the fireball: {other:?}"),
        }
    }
}

#[cfg(test)]
#[path = "breeding_orb_tests.rs"]
mod breeding_orb_tests;

#[cfg(test)]
mod chorus_projectile_drop_tests {
    use super::*;
    use crate::game::{Audio, Options, Progress, Settings, load_resources};
    use horizon_client::{chunk::Chunk, motion::PlayerMotion};
    use horizon_core::position::ChunkPos;
    use std::sync::Arc;
    fn with_game(test: impl FnOnce(&mut Game<'_>)) {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.game_dir = Some(
            std::path::PathBuf::from(std::env::var_os("HORIZON_HOME").unwrap())
                .join(format!("h116-{:?}", std::thread::current().id())),
        );
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        game.set_allow_commands(true);

        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(0, 0))
            .unwrap();
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(2.5, 200.0, 2.5)).unwrap());
        game.creative = true;
        assert!(game.run_command("forceload add 0 0"));
        assert!(game.run_command("tick freeze"));
        assert!(game.run_command("fill 0 198 0 15 199 15 stone"));
        test(&mut game);
    }

    fn source(game: &mut Game<'_>, x: i32, z: i32, block: &str) {
        assert!(game.world.chunks.contains_key(&(0, 0)));
        assert!(game.run_command(&format!("setblock {x} 199 {z} end_stone strict")));
        assert!(game.run_command(&format!("setblock {x} 200 {z} {block} strict")));
        assert!(game.run_command(&format!("execute if block {x} 199 {z} end_stone")));
    }
    fn launch(game: &mut Game<'_>, kind: &str, n: i32, x: i32, z: i32, owner: bool) -> u32 {
        let own = if owner { ",Owner:[I;116,0,0,99]" } else { "" };
        assert!(game.run_command(&format!("summon {kind} {x}.5 202.5 {z}.5 {{UUID:[I;116,0,0,{n}],NoGravity:1b,Motion:[0d,-2d,0d]{own}}}")));
        let p = game
            .creatures
            .projectiles
            .iter()
            .find(|p| game.persist.uuid_of(p.id) == Some([116, 0, 0, n]))
            .unwrap();
        assert_eq!(p.kind.id(), format!("minecraft:{kind}"));
        assert_eq!(p.position, Vec3::new(x as f64 + 0.5, 202.5, z as f64 + 0.5));
        assert_eq!(p.velocity, Vec3::new(0.0, -2.0, 0.0));
        assert!(p.no_gravity);
        assert_eq!(p.owner_uuid, owner.then_some([116, 0, 0, 99]));
        if owner {
            assert!(matches!(p.owner, Some(Who::Mob(_))));
        }
        p.id
    }
    fn dropped(game: &Game<'_>, item: &str) {
        assert_eq!(game.dropped.len(), 1, "actual unfiltered item collection");
        assert_eq!(&*game.dropped[0].stack.item, item);
        assert_eq!(game.dropped[0].stack.count, 1);
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_registered_arrow_collision_keeps_the_actual_projectile_for_chorus_loot() {
        with_game(|game| {
            source(game, 4, 4, "chorus_flower");
            assert!(game.dropped.is_empty());
            let id = launch(game, "arrow", 1, 4, 4, false);
            game.tick_projectiles().unwrap();
            assert!(
                game.run_command("execute if block 4 200 4 air"),
                "actual collision broke the source"
            );
            assert!(
                game.creatures
                    .projectiles
                    .iter()
                    .find(|p| p.id == id)
                    .unwrap()
                    .arrow
                    .as_ref()
                    .unwrap()
                    .in_ground
            );
            dropped(game, "minecraft:chorus_flower");
        });
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_registered_snowball_collision_keeps_the_actual_projectile_for_chorus_loot() {
        with_game(|game| {
            source(game, 4, 4, "chorus_flower");
            let id = launch(game, "snowball", 2, 4, 4, false);
            game.tick_projectiles().unwrap();
            assert!(game.run_command("execute if block 4 200 4 air"));
            assert!(!game.creatures.projectiles.iter().any(|p| p.id == id));
            dropped(game, "minecraft:chorus_flower");
        });
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn denied_hits_other_blocks_and_direct_destroy_keep_their_drop_contracts() {
        with_game(|game| {
            source(game, 4, 4, "chorus_flower");
            assert!(game.run_command("gamerule projectiles_can_break_blocks false"));
            launch(game, "arrow", 3, 4, 4, false);
            game.tick_projectiles().unwrap();
            assert!(game.run_command("execute if block 4 200 4 chorus_flower"));
            assert!(game.dropped.is_empty());
            assert!(game.run_command("gamerule projectiles_can_break_blocks true"));
            assert!(game.ui.rules.bool("mob_griefing"));
            assert!(game.run_command("gamerule mob_griefing"));
            assert!(game.run_command(
                "summon pig 12.5 200 12.5 {UUID:[I;116,0,0,99],NoAI:1b,NoGravity:1b,Health:10f}"
            ));
            source(game, 8, 4, "chorus_flower");
            launch(game, "arrow", 4, 8, 4, true);
            game.tick_projectiles().unwrap();
            assert!(game.run_command("execute if block 8 200 4 chorus_flower"));
            assert!(game.dropped.is_empty());
            source(game, 12, 4, "chorus_flower");
            launch(game, "ender_pearl", 5, 12, 4, false);
            game.tick_projectiles().unwrap();
            assert!(game.run_command("execute if block 12 200 4 chorus_flower"));
            assert!(game.dropped.is_empty());
            source(game, 4, 8, "stone");
            launch(game, "arrow", 6, 4, 8, false);
            game.tick_projectiles().unwrap();
            assert!(game.run_command("execute if block 4 200 8 stone"));
            assert!(game.dropped.is_empty());
            assert!(game.run_command("gamerule block_drops false"));
            source(game, 8, 8, "chorus_flower");
            launch(game, "snowball", 7, 8, 8, false);
            game.tick_projectiles().unwrap();
            assert!(game.run_command("execute if block 8 200 8 air"));
            assert!(game.dropped.is_empty());
            assert!(game.run_command("gamerule block_drops true"));
            source(game, 4, 12, "chorus_flower");
            assert!(game.run_command("setblock 4 200 12 air destroy"));
            assert!(game.dropped.is_empty());
            source(game, 8, 12, "dirt");
            assert!(game.run_command("setblock 8 200 12 air destroy"));
            dropped(game, "minecraft:dirt");
        });
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn default_drop_rng_continues_identically_with_table_sequence_or_level_fallback() {
        for named in [false, true] {
            with_game(|game| {
                let root =
                    std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
                let mut table: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(root.join("data/minecraft/loot_table/blocks/gravel.json"))
                        .unwrap(),
                )
                .unwrap();
                if !named {
                    table.as_object_mut().unwrap().remove("random_sequence");
                }
                let files = vec![(
                    "loot_table/minecraft:blocks/gravel".to_owned(),
                    vec![serde_json::to_vec(&table).unwrap()],
                )];
                let registry = game.pack.assets.registry;
                // The level callback requires its catalog for the world lifetime.
                // These two fixture catalogs live only for this test process.
                let items: &'static horizon_client::items::Items = Box::leak(Box::new(
                    horizon_client::items::Items::load(
                        &root,
                        registry,
                        game.res.facts(),
                        &horizon_world::loot::PackOverlay { files: &files },
                    )
                    .unwrap(),
                ));
                let state = registry.default_state("minecraft:gravel").unwrap();
                assert_eq!(items.block_loot_sequence(registry, state).is_some(), named);
                source(game, 4, 4, "gravel");
                game.level.seed = 116;
                game.world.random = horizon_core::random::LegacyRandom::new(116);
                game.rand_sequences = Default::default();
                game.with_level(|b, level, ticks| {
                    level.items = items;
                    assert!(b.destroy_block(level, ticks, BlockPos::new(4, 200, 4), true));
                });
                game.process_level_events().unwrap();
                assert_eq!(game.dropped.len(), 1);
                let pool = game.rand_sequences.to_nbt();
                let sequence_words = match pool
                    .get("sequences")
                    .and_then(|v| v.get("minecraft:blocks/gravel"))
                    .and_then(|v| v.get("source"))
                {
                    Some(horizon_world::nbt::Tag::LongArray(v)) => v.clone(),
                    _ => vec![],
                };
                assert_eq!(
                    !sequence_words.is_empty(),
                    named,
                    "actual selected random source"
                );
                let mut random = game.world.random.clone();
                let continuation: Vec<i64> = (0..4).map(|_| random.next_long()).collect();
                let drops: Vec<_> = game
                    .dropped
                    .iter()
                    .map(|d| (d.stack.item.to_string(), d.stack.count))
                    .collect();
                println!(
                    "h116-continuation {}",
                    serde_json::json!({"named":named,"level_continuation":continuation,"sequence_words":sequence_words,"drops":drops})
                );
            });
        }
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn breaker_context_is_target_only_and_does_not_escape_its_level_call() {
        with_game(|game| {
            source(game, 4, 4, "chorus_flower");
            source(game, 8, 4, "chorus_flower");
            source(game, 12, 4, "chorus_flower");
            let target = BlockPos::new(4, 200, 4);
            let neighbor = BlockPos::new(8, 200, 4);
            let state = game.world.block(4, 200, 4);
            // Actual loaded states and callback/evaluator; this is an isolation
            // unit, beside the registered projectile collision regressions.
            game.with_level(|_, level, _| {
                assert!(level.drop_breaker_at.is_none());
                level.drop_breaker_at = Some(target);
                level.drop_resources_now(neighbor, state);
                level.drop_resources_now(target, state);
            });
            dropped(game, "minecraft:chorus_flower");
            game.with_level(|_, level, _| assert!(level.drop_breaker_at.is_none()));
            for fail in [false, true] {
                let outcome: std::result::Result<(), ()> = game.with_level(|_, level, _| {
                    level.drop_breaker_at = Some(target);
                    if fail {
                        return Err(());
                    }
                    Ok(())
                });
                assert_eq!(outcome.is_err(), fail);
                game.with_level(|_, level, _| assert!(level.drop_breaker_at.is_none()));
            }
            // The same position on a later no-entity destruction cannot reuse
            // the former callback's presence. Nor can its neighbor.
            game.destroy_block_dropping([4, 200, 4]).unwrap();
            game.destroy_block_dropping([8, 200, 4]).unwrap();
            dropped(game, "minecraft:chorus_flower");
            let kept = game.with_level(|b, level, ticks| {
                assert!(level.drop_breaker_at.is_none());
                level.drop_tool = Some(((12, 200, 4), None));
                level.drop_into = Some(Vec::new());
                assert!(b.destroy_block(level, ticks, BlockPos::new(12, 200, 4), true));
                level.drop_into.take().unwrap()
            });
            assert!(kept.is_empty(), "machine's no-entity loot stays empty");
            dropped(game, "minecraft:chorus_flower");
        });
    }
}

#[cfg(test)]
mod loaded_arrow_shot_tests {
    use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
    use horizon_client::{
        chunk::Chunk,
        mobs::Who,
        motion::PlayerMotion,
        projectiles::{Projectile, ProjectileKind},
    };
    use horizon_core::{
        geometry::Vec3,
        position::ChunkPos,
        random::{LegacyRandom, RandomSource},
    };
    use std::sync::Arc;
    fn with_game(test: impl FnOnce(&mut Game<'_>)) {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.game_dir = Some(
            std::path::PathBuf::from(std::env::var_os("HORIZON_HOME").unwrap())
                .join(format!("h114-{:?}", std::thread::current().id())),
        );
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        game.set_allow_commands(true);

        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(0, 0))
            .unwrap();
        assert!(game.run_command("difficulty normal"));
        assert!(game.run_command("forceload add 0 0"));
        for (x, z) in [(0, 1), (1, 0), (1, 1)] {
            game.world
                .chunks
                .insert((x, z), Arc::new(Chunk::empty(x, z)));
            game.block_ticks
                .add_empty_chunk(ChunkPos::new(x, z))
                .unwrap();
        }
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(2.5, 200.0, 2.5)).unwrap());
        game.creative = true;
        assert!(game.run_command("fill 0 199 0 31 199 31 stone"));
        assert!(
            game.run_command("summon pig 13.5 200 2.5 {UUID:[I;114,0,0,99],NoAI:1b,NoGravity:1b}")
        );
        game.world.random = LegacyRandom::new(114);
        test(&mut game);
    }
    fn start_events() {
        crate::game::sculk::TEST_POSTED_EVENTS.with(|v| *v.borrow_mut() = Some(Vec::new()));
    }
    fn take_shots() -> Vec<(&'static str, [f64; 3], Option<Who>)> {
        crate::game::sculk::TEST_POSTED_EVENTS.with(|v| {
            std::mem::take(v.borrow_mut().as_mut().unwrap())
                .into_iter()
                .filter(|e| e.0 == "minecraft:projectile_shoot")
                .collect()
        })
    }
    fn stop_events() {
        crate::game::sculk::TEST_POSTED_EVENTS.with(|v| *v.borrow_mut() = None);
    }
    fn owner(game: &Game<'_>) -> u32 {
        game.mobs
            .iter()
            .find(|m| game.persist.uuid_of(m.id) == Some([114, 0, 0, 99]))
            .unwrap()
            .id
    }
    #[test]
    #[ignore = "prepared resources; actual registered load, Game tick and posted events"]
    fn loaded_arrow_first_tick_dispatches_once_with_its_real_owner() {
        with_game(|game| {
            let owner = owner(game);
            // Already-shot is an independent silent control before the old failure.
            for (n, flag) in [(1, Some(1)), (2, None), (3, Some(0))] {
                let extra = flag
                    .map(|f| format!(",HasBeenShot:{f}b"))
                    .unwrap_or_default();
                assert!(game.run_command(&format!("summon arrow 6.5 201.5 10.5 {{UUID:[I;114,0,0,{n}],NoGravity:1b,Motion:[0d,0d,0d],Owner:[I;114,0,0,99]{extra}}}")));
                let id = game
                    .creatures
                    .projectiles
                    .iter()
                    .find(|p| game.persist.uuid_of(p.id) == Some([114, 0, 0, n]))
                    .unwrap()
                    .id;
                let p = game
                    .creatures
                    .projectiles
                    .iter()
                    .find(|p| p.id == id)
                    .unwrap();
                assert_eq!(p.owner_uuid, Some([114, 0, 0, 99]));
                assert_eq!(p.arrow.as_ref().unwrap().has_been_shot, flag == Some(1));
                assert!(p.no_gravity);
                assert_eq!(p.velocity, Vec3::ZERO);
                start_events();
                game.tick_projectiles().unwrap();
                let p = game
                    .creatures
                    .projectiles
                    .iter()
                    .find(|p| p.id == id)
                    .unwrap();
                assert!(p.arrow.as_ref().unwrap().has_been_shot);
                assert_eq!(p.position, Vec3::new(6.5, 201.5, 10.5));
                assert_eq!(p.velocity, Vec3::ZERO);
                let first = take_shots();
                game.tick_projectiles().unwrap();
                let second = take_shots();
                stop_events();
                assert!(second.is_empty(), "no repeated shot on tick2");
                let expected = if flag == Some(1) {
                    Vec::new()
                } else {
                    vec![(
                        "minecraft:projectile_shoot",
                        [6.5, 201.5, 10.5],
                        Some(Who::Mob(owner)),
                    )]
                };
                assert_eq!(first, expected, "actual first tick, input={flag:?}");
                assert!(game.run_command(&format!(
                    "kill {}",
                    horizon_protocol::crypto::uuid_string(
                        horizon_client::lan::playerdata::ints_uuid([114, 0, 0, n])
                    )
                )));
            }
        });
    }
    fn natural(kind: &str) {
        with_game(|game| {
            let shooter = owner(game);
            match kind {
                "bow"=>{
                    assert!(game.run_command("item replace entity @s weapon.mainhand with bow"));
                    assert!(game.run_command("item replace entity @s hotbar.1 with arrow 4"));
                },
                "crossbow"=>assert!(game.run_command("item replace entity @s weapon.mainhand with crossbow[charged_projectiles=[{id:\"minecraft:arrow\",count:1}]]")),
                _=>{}
            }
            let clock = game.world.game_time;
            start_events();
            match kind {
                "bow" => {
                    let bow = game.held().unwrap().clone();
                    game.release_bow(&bow, 20);
                }
                "crossbow" => {
                    let bow = game.held().unwrap().clone();
                    assert!(game.use_crossbow(&bow));
                }
                "mob" => {
                    let mut p = Projectile::new(
                        ProjectileKind::Arrow {
                            base_damage: 2.0,
                            critical: false,
                            effect: None,
                        },
                        Vec3::new(6.5, 201.5, 6.5),
                        Some(Who::Mob(shooter)),
                    );
                    p.velocity = Vec3::new(0.0, 0.0, 1.0);
                    game.launch_mob_projectile(p);
                }
                _ => unreachable!(),
            }
            assert_eq!(game.creatures.projectiles.len(), 1, "real launch {kind}");
            let p = &game.creatures.projectiles[0];
            let id = p.id;
            assert!(id > 0);
            assert!(
                !p.arrow.as_ref().unwrap().has_been_shot,
                "saved pre-tick state"
            );
            let launch = take_shots();
            game.tick_projectiles().unwrap();
            let p = game
                .creatures
                .projectiles
                .iter()
                .find(|p| p.id == id)
                .unwrap();
            assert!(p.arrow.as_ref().unwrap().has_been_shot);
            let at = [p.position.x, p.position.y, p.position.z];
            let first = take_shots();
            game.tick_projectiles().unwrap();
            let second = take_shots();
            stop_events();
            assert_eq!(
                game.world.game_time, clock,
                "fixture directly invokes actual projectile ticks; runtime clocks independently admitted"
            );
            println!("H114_RNG {kind} {}", game.world.random.next_long());
            assert_eq!(
                [launch.len(), first.len(), second.len()],
                [0, 1, 0],
                "actual launch/tick1/tick2 counts {kind}"
            );
            assert_eq!(
                first[0],
                (
                    "minecraft:projectile_shoot",
                    at,
                    Some(if kind == "mob" {
                        Who::Mob(shooter)
                    } else {
                        Who::Player
                    })
                )
            );
        });
    }
    #[test]
    #[ignore = "prepared resources; actual bow release and Game ticks"]
    fn bow_launch_uses_only_the_first_tick_event() {
        natural("bow")
    }
    #[test]
    #[ignore = "prepared resources; actual charged crossbow use and Game ticks"]
    fn crossbow_launch_uses_only_the_first_tick_event() {
        natural("crossbow")
    }
    #[test]
    #[ignore = "prepared resources; actual Mob launch adapter and Game ticks"]
    fn mob_launch_uses_only_the_first_tick_event() {
        natural("mob")
    }
    #[test]
    #[ignore = "prepared resources; non-arrow launch posts remain unchanged"]
    fn non_arrow_rocket_and_mob_trident_keep_their_immediate_posts() {
        with_game(|game| {
            assert!(game.run_command("item replace entity @s weapon.mainhand with crossbow[charged_projectiles=[{id:\"minecraft:firework_rocket\",count:1}]]"));
            start_events();
            let bow = game.held().unwrap().clone();
            assert!(game.use_crossbow(&bow));
            assert_eq!(game.rockets.len(), 1);
            assert_eq!(take_shots().len(), 1);
            game.tick_projectiles().unwrap();
            assert!(take_shots().is_empty());
            stop_events();
            let mut p = Projectile::new(
                ProjectileKind::Trident,
                Vec3::new(6.5, 201.5, 6.5),
                Some(Who::Mob(owner(game))),
            );
            p.velocity = Vec3::new(0.0, 0.0, 1.0);
            start_events();
            game.launch_mob_projectile(p);
            assert_eq!(take_shots().len(), 1);
            game.tick_projectiles().unwrap();
            assert!(take_shots().is_empty());
            stop_events();
        });
    }
    #[test]
    #[ignore = "prepared resources; real loaded owner and finish-hit source"]
    fn finish_hit_dispatches_once_and_the_next_tick_does_not_repeat_it() {
        with_game(|game| {
            assert!(game.run_command("summon arrow 6.5 201.5 10.5 {UUID:[I;114,0,0,20],NoGravity:1b,Motion:[0d,0d,0d],Owner:[I;114,0,0,99]}"));
            let mut arrow = game.creatures.projectiles.pop().unwrap();
            let owner = owner(game);
            assert_eq!(arrow.owner_uuid, Some([114, 0, 0, 99]));
            // Resolve through the actual registered owner, as tick_projectiles does.
            arrow.owner = game.who_by_uuid(arrow.owner_uuid.unwrap());
            assert_eq!(arrow.owner, Some(Who::Mob(owner)));
            assert!(!arrow.arrow.as_ref().unwrap().has_been_shot);
            let id = arrow.id;
            start_events();
            game.finish_arrow_hit(arrow, super::EntityHit::Deflected);
            let first = take_shots();
            assert!(
                game.creatures
                    .projectiles
                    .iter()
                    .find(|p| p.id == id)
                    .unwrap()
                    .arrow
                    .as_ref()
                    .unwrap()
                    .has_been_shot
            );
            game.tick_projectiles().unwrap();
            assert!(take_shots().is_empty());
            stop_events();
            assert_eq!(
                first,
                vec![(
                    "minecraft:projectile_shoot",
                    [6.5, 201.5, 10.5],
                    Some(Who::Mob(owner))
                )]
            );
        });
    }
    #[test]
    #[ignore = "prepared resources; registered Guest context and finish-hit source"]
    fn finish_hit_uses_registered_guest_context_and_restores_the_host() {
        use horizon_client::lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig};
        use horizon_protocol::packets::configuration::ClientInformation;
        use std::sync::{atomic::AtomicI32, mpsc};
        with_game(|game| {
            game.lan_host(
                crate::game::lan::LanSettings {
                    port: 0,
                    guest_commands: true,
                    force_game_mode: false,
                },
                NetConfig {
                    announce: false,
                    ..NetConfig::loopback(0, "arrow-test".into(), 2)
                },
                HubConfig {
                    default_game_mode: 1,
                    guest_commands: true,
                    force_game_mode: false,
                    hardcore: false,
                    view_distance: 2,
                    simulation_distance: 2,
                    dimension: "minecraft:overworld".into(),
                    allow_flight: false,
                    seed: 0,
                    max_players: 2,
                    integrated: false,
                    levels: Default::default(),
                },
            )
            .unwrap();
            let (tx, _rx) = mpsc::channel();
            let mut guest = Guest::new(
                Joined {
                    conn: 2,
                    uuid: 114,
                    name: "ArrowGuest".into(),
                    properties: Vec::new(),
                    info: ClientInformation::new(2),
                    brand: "test".into(),
                    sender: PlayerSender::new(tx, Arc::default()),
                    latency: Arc::new(AtomicI32::new(0)),
                    address: "127.0.0.1:25565".parse().unwrap(),
                    expected: false,
                    configured: Default::default(),
                },
                2,
                [6.5, 200.0, 6.5],
                2,
            );
            guest.dimension = "minecraft:overworld".into();
            guest.placed = true;
            guest.loaded = true;
            guest.permission = 4;
            let mut host = game.lan.take().unwrap();
            host.hub.guests.push(guest);
            let host_uuid = game.acting_player_uuid();
            game.as_guest(&mut host,2,|g| {
                assert_eq!(g.lan_acting,Some(2));
                assert_eq!(g.acting_player_uuid(),[0,0,0,114]);
                assert!(g.run_command("item replace entity @s weapon.mainhand with crossbow[charged_projectiles=[{id:\"minecraft:arrow\",count:1}]]"));
                let bow=g.held().unwrap().clone();assert!(g.use_crossbow(&bow));
            }).unwrap();
            game.lan = Some(host);
            assert_eq!(game.acting_player_uuid(), host_uuid);
            assert_eq!(game.lan_acting, None);
            let arrow = game.creatures.projectiles.pop().unwrap();
            assert_eq!(arrow.guest, Some(2));
            assert_eq!(arrow.owner, Some(Who::Player));
            assert_eq!(arrow.owner_uuid, Some([0, 0, 0, 114]));
            assert!(!arrow.arrow.as_ref().unwrap().has_been_shot);
            let at = [arrow.position.x, arrow.position.y, arrow.position.z];
            let id = arrow.id;
            start_events();
            game.finish_arrow_hit(arrow, super::EntityHit::Deflected);
            let first = take_shots();
            assert_eq!(game.acting_player_uuid(), host_uuid);
            assert_eq!(game.lan_acting, None);
            assert_eq!(game.lan.as_ref().unwrap().hub.guests[0].uuid, 114);
            assert!(
                game.creatures
                    .projectiles
                    .iter()
                    .find(|p| p.id == id)
                    .unwrap()
                    .arrow
                    .as_ref()
                    .unwrap()
                    .has_been_shot
            );
            game.tick_projectiles().unwrap();
            assert!(take_shots().is_empty());
            stop_events();
            assert_eq!(
                first,
                vec![("minecraft:projectile_shoot", at, Some(Who::Player))]
            );
        });
    }
}

#[cfg(test)]
mod loaded_mob_arrow_permission_tests {
    use super::*;
    use crate::game::native_machines::tests::server_game;
    use horizon_client::{entity_nbt, save};
    use horizon_core::position::BlockPos;
    use horizon_world::nbt::Tag;

    fn parse(text: &str) -> Tag {
        let (tag, used) = horizon_world::nbt::parse_snbt(text).unwrap();
        assert_eq!(used, text.len());
        tag
    }

    #[test]
    #[ignore = "prepared resources; actual registered summon, collision, permission and drops"]
    fn loaded_mob_arrow_predicate_survives_model_and_controls_actual_block_hit() {
        let mut game = server_game();
        game.build_registries();
        game.set_allow_commands(true);
        game.block_ticks
            .add_empty_chunk(BlockPos::new(0, 0, 0).chunk())
            .unwrap();
        assert!(game.run_command("forceload add 0 0"));
        assert!(game.run_command("fill 0 199 0 15 199 15 stone"));
        assert!(game.world.loaded(3, 1));
        assert!(game.ui.rules.bool("projectiles_can_break_blocks"));
        assert!(game.ui.rules.bool("block_drops"));
        assert!(game.run_command(
            "summon pig 14.5 200 14.5 {UUID:[I;197,0,0,1],NoAI:1b,NoGravity:1b,Health:10f}"
        ));
        let owner = game
            .mobs
            .iter()
            .find(|m| game.persist.uuid_of(m.id) == Some([197, 0, 0, 1]))
            .unwrap()
            .id;
        assert_eq!(
            game.mobs.iter().find(|m| m.id == owner).unwrap().health,
            10.0
        );
        let clock = game.world.game_time;
        let expected = parse("{blocks:\"minecraft:chorus_flower\"}");
        let stale = parse("{can_break:{blocks:\"minecraft:stone\"},marker:37}");
        let mut missing = Vec::new();
        for (n, predicate, griefing, breaks) in [
            (1, Some("chorus_flower"), true, true),
            (2, Some("stone"), true, false),
            (3, None, true, false),
            (4, Some("chorus_flower"), false, false),
            (5, Some("stone"), false, false),
            (6, None, false, false),
        ] {
            let x = if n <= 3 { 3 } else { 11 };
            let z = 1 + 4 * ((n - 1) % 3);
            if game.ui.rules.bool("mob_griefing") != griefing {
                assert!(game.run_command(&format!("gamerule mob_griefing {griefing}")));
            }
            assert_eq!(game.ui.rules.bool("mob_griefing"), griefing);
            assert!(game.run_command(&format!("setblock {x} 199 {z} end_stone")));
            assert!(game.run_command(&format!("setblock {x} 200 {z} chorus_flower[age=0]")));
            let field = predicate.map_or(String::new(), |p| {
                format!(",can_break:{{blocks:\"minecraft:{p}\"}}")
            });
            assert!(game.run_command(&format!("summon arrow {} 200.5 {} {{UUID:[I;197,0,{n},2],NoGravity:1b,HasBeenShot:1b,Motion:[2d,0d,0d],Owner:[I;197,0,0,1],item:{{id:\"minecraft:arrow\",count:1}}{field}}}", f64::from(x)-1.5, f64::from(z)+0.5)));
            let p = game
                .creatures
                .projectiles
                .iter()
                .find(|p| game.persist.uuid_of(p.id) == Some([197, 0, n, 2]))
                .unwrap()
                .clone();
            assert_eq!(p.owner_uuid, Some([197, 0, 0, 1]));
            assert_eq!(
                p.owner,
                Some(Who::Mob(owner)),
                "actual resolved loaded Mob, not raw Owner alone"
            );
            assert!(matches!(p.kind, ProjectileKind::Arrow { .. }));
            assert!(p.no_gravity);
            assert_eq!(p.velocity, Vec3::new(2.0, 0.0, 0.0));
            assert!(!p.arrow.as_ref().unwrap().in_ground);
            game.persist.raw.remove(&p.id);
            for (route, raw) in [("no-raw", None), ("stale-raw", Some(&stale))] {
                let saved =
                    entity_nbt::arrow_to_nbt(&p, [197, 0, n, 2], p.owner_uuid, None, raw).unwrap();
                let want = predicate.map(|id| parse(&format!("{{blocks:\"minecraft:{id}\"}}")));
                if saved.get("can_break") != want.as_ref() {
                    missing.push(format!(
                        "{n} cloned loaded model {route}: {:?} != {want:?}",
                        saved.get("can_break")
                    ));
                }
                if raw.is_some() {
                    assert_eq!(save::number(saved.get("marker")), Some(37));
                }
            }
            if n == 1 {
                assert_eq!(game.persist.uuid_of(p.id), Some([197, 0, n, 2]));
                // The expected value is the actual admitted input, never an injected model field.
                assert_eq!(expected, parse("{blocks:\"minecraft:chorus_flower\"}"));
            }
            let drops_before = game.dropped.len();
            game.tick_projectiles().unwrap();
            let after = game
                .creatures
                .projectiles
                .iter()
                .find(|a| a.id == p.id)
                .unwrap();
            assert!(
                after.arrow.as_ref().unwrap().in_ground,
                "actual air-to-ground block collision"
            );
            let block = &*game.res.facts().get(game.world.block(x, 200, z)).name;
            let want = if breaks {
                "minecraft:air"
            } else {
                "minecraft:chorus_flower"
            };
            if block != want {
                missing.push(format!("{n} actual hit block {block} != {want}"));
            }
            let dropped: Vec<_> = game
                .dropped
                .iter()
                .skip(drops_before)
                .map(|e| (&*e.stack.item, e.stack.count, e.position, e.velocity))
                .collect();
            println!("actual collision {n}: block={block}, unfiltered new drops={dropped:?}");
            if !breaks {
                assert!(dropped.is_empty(), "denied hits cannot create drops");
            }
        }
        assert!(game.run_command("setblock 13 200 12 dirt"));
        let before = game.dropped.len();
        game.destroy_block_dropping([13, 200, 12]).unwrap();
        assert_eq!(
            &*game.res.facts().get(game.world.block(13, 200, 12)).name,
            "minecraft:air"
        );
        let direct: Vec<_> = game.dropped.iter().skip(before).collect();
        assert_eq!(
            direct.len(),
            1,
            "independent actual destruction/drop callback"
        );
        assert_eq!(&*direct[0].stack.item, "minecraft:dirt");
        assert_eq!(direct[0].stack.count, 1);
        assert_eq!(
            game.world.game_time, clock,
            "one projectile step per scene; no fabricated world ticks"
        );
        assert!(
            missing.is_empty(),
            "actual loaded Mob arrow model/permission mismatches: {missing:?}"
        );
    }
}

#[cfg(test)]
mod h169_glide_travel_tests {
    use super::*;
    use crate::game::{Audio, Options, Progress, Settings, load_resources};
    use horizon_client::{chunk::Chunk, motion::PlayerMotion};
    use horizon_core::{geometry::Vec3, position::ChunkPos, random::LegacyRandom};
    use std::sync::Arc;
    fn with_game(test: impl FnOnce(&mut Game<'_>)) {
        with_role_game(
            crate::game::execution_role::ExecutionRole::IntegratedServer,
            test,
        );
    }
    fn with_role_game(
        role: crate::game::execution_role::ExecutionRole,
        test: impl FnOnce(&mut Game<'_>),
    ) {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.game_dir = Some(
            std::path::PathBuf::from(std::env::var_os("HORIZON_HOME").unwrap())
                .join(format!("h169-{:?}", std::thread::current().id())),
        );
        options.execution_role = role;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        game.set_allow_commands(true);

        let mut center = Chunk::empty(0, 0);
        center.lit = true;
        game.world.chunks.insert((0, 0), Arc::new(center));
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(0, 0))
            .unwrap();
        assert!(game.run_command("difficulty normal"));
        assert!(game.run_command("forceload add 0 0"));
        for x in -1..=1 {
            for z in -1..=1 {
                if x == 0 && z == 0 {
                    continue;
                }
                let mut chunk = Chunk::empty(x, z);
                chunk.lit = true;
                game.world.chunks.insert((x, z), Arc::new(chunk));
                game.block_ticks
                    .add_empty_chunk(ChunkPos::new(x, z))
                    .unwrap();
            }
        }
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(2.5, 200.0, 2.5)).unwrap());
        game.creative = true;
        assert!(game.run_command("fill 0 199 0 31 199 31 stone"));
        game.world.random = LegacyRandom::new(169);
        game.update_chunk_tickets();
        assert!(crate::game::ticking_chunk(&game.world, 0, 0));
        assert!(game.tickets.levels().block_ticking(0, 0));
        test(&mut game);
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn h169_registered_eligible_husk_actually_travels_as_a_glider() {
        with_game(|game| {
            game.world.game_time = 0;
            for (n, no_ai, glide, x) in [(1, 0, 1, 4.5), (2, 1, 1, 8.5), (3, 0, 0, 12.5)] {
                assert!(game.run_command(&format!(
                    "summon husk {x} 300 8.5 {{UUID:[I;169,0,0,{n}],NoAI:{no_ai}b,PersistenceRequired:1b,FallFlying:{glide}b,Rotation:[0f,0f],Motion:[0d,0d,0d],equipment:{{chest:{{id:\"minecraft:elytra\",count:1}}}}}}"
                )));
            }
            let indexes: Vec<_> = (1..=3)
                .map(|n| {
                    let id = game
                        .persist
                        .uuids
                        .iter()
                        .find_map(|(id, u)| (*u == [169, 0, 0, n]).then_some(*id))
                        .unwrap();
                    game.mobs.iter().position(|m| m.id == id).unwrap()
                })
                .collect();
            let main = indexes[0];
            let id = game.mobs[main].id;
            assert!(game.mobs[main].extra.fall_flying);
            assert!(!game.mobs[main].no_ai && !game.mobs[main].on_ground);
            let stack = horizon_client::mobs::finalize::equipment_stack(
                &game.mobs[main],
                horizon_world::player_inventory::EquipmentSlot::Chest,
            )
            .unwrap();
            assert!(horizon_client::item_sync::can_glide_using(
                &stack,
                "chest",
                game.items()
            ));
            game.world.game_time += 1;
            game.update_chunk_tickets();
            assert!(game.entity_ticking_area()(4, 8));
            game.tick_creature_entities().unwrap();
            assert_eq!(game.mobs[main].id, id);
            assert_eq!(game.uuid_for(id), [169, 0, 0, 1]);
            assert!(
                game.mobs[main].extra.fall_flying,
                "real modeled flag remains eligible after the tick"
            );
            assert_eq!(
                game.mobs[indexes[1]].position,
                Vec3::new(8.5, 300.0, 8.5),
                "NoAI is a stationary control"
            );
            assert!(!game.mobs[indexes[2]].extra.fall_flying);
            assert_eq!(game.world.game_time, 1);
            // Independent fresh Java first-tick values, not native helper output.
            assert_eq!(
                game.mobs[main].position,
                Vec3::new(4.5, 299.9823599996567, 8.501782000017165)
            );
            assert_eq!(
                game.mobs[main].velocity,
                Vec3::new(0.0, -0.017640000343322755, 0.0017820000171661376)
            );
        });
    }

    // Added source/unit control, not part of the frozen dry-air Java runtime pair.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn h169_added_actual_wall_collision_uses_mob_damage_source() {
        with_game(|game| {
            assert!(game.run_command("fill 6 298 7 6 303 10 stone"));
            assert!(game.run_command(r#"summon husk 4.5 300 8.5 {UUID:[I;169,0,0,4],PersistenceRequired:1b,FallFlying:1b,Rotation:[-90f,0f],Motion:[2d,0d,0d],equipment:{chest:{id:"minecraft:elytra",count:1}}}"#));
            let id = game
                .persist
                .uuids
                .iter()
                .find_map(|(id, u)| (*u == [169, 0, 0, 4]).then_some(*id))
                .unwrap();
            let index = game.mobs.iter().position(|m| m.id == id).unwrap();
            assert!(
                game.mobs[index].kind.armor > 0.0,
                "real Husk armor is the bypass control"
            );
            assert_eq!(game.mobs[index].health, 20.0);
            game.world.game_time += 1;
            game.update_chunk_tickets();
            game.tick_creature_entities().unwrap();
            let m = &game.mobs[index];
            assert!(m.horizontal_collision, "actual wall collision");
            assert!(m.position.x > 4.5 && m.position.x < 6.0);
            assert_eq!(m.velocity.x, 0.0, "resolved collision, no damage knockback");
            assert_eq!(m.health, 3.0, "(2 - 0) * 10 - 3, bypassing the real armor");
            let damage = m.last_damage.unwrap().0;
            assert_eq!(damage.kind, horizon_client::mobs::DamageKind::FlyIntoWall);
            assert!(damage.bypasses_armor() && damage.no_knockback());
            assert!(!damage.is_fire() && !damage.is_projectile());
            assert_eq!(source_damage_type(damage), "minecraft:fly_into_wall");
            assert_eq!(game.uuid_for(id), [169, 0, 0, 4]);
        });
    }
}
