//! Shulkers (`Shulker`): fixed to a block face, peeking out of their shell,
//! firing homing bullets at players within 20 blocks, teleporting when
//! their face is lost or when badly hurt, and shrugging off arrows while
//! closed.
use super::{
    Damage, DamageKind, Mob, MobEvent, MobWorld, Species, Who,
    anim::{Poses, State},
    control::LookKind,
    goals::{
        Cx, Goal, HurtByTarget, LOOK, LookAtPlayer, MOVE, NearestTarget, RandomLookAround,
        goal_clone, reduced,
    },
};
use crate::projectiles::{Projectile, ProjectileWorld, STEPS};
use horizon_core::geometry::trig::Mth;
use horizon_core::{
    geometry::{Aabb, Direction, ShapeOp, Vec3, VoxelShape},
    random::{LegacyRandom, RandomSource},
};
use std::f32::consts::PI;

// Shulker state: `variant3` the attach face (3D data value), `counter` the
// raw peek (0..100), `color` the shell colour (16 none). AnimState slot 0
// the current peek amount. On the client, `scratch[0]` is
// `clientSideTeleportInterpolation` and `scratch[1..3]` is
// `clientOldAttachPosition`.
pub const PEEK: usize = 0;
pub const NO_COLOR: u8 = 16;
/// `Direction.DOWN`'s 3D data value.
const DOWN: u8 = 0;

/// `Shulker.startRiding`: a shulker that boards a vehicle puts its attach face
/// back to `DOWN`, so it rides lid up whatever block face it was stuck to.
/// The client half clears the teleport slide (`clientOldAttachPosition` and
/// `clientSideTeleportInterpolation`).
pub fn on_start_riding(m: &mut Mob) {
    if m.species() == Species::Shulker {
        m.extra.variant3 = DOWN;
        m.extra.scratch[0] = 0;
    }
}

/// `Shulker.getDefaultDimensions`: a shell attached to the block below it
/// (attach face `DOWN`, its lid opening upward) grows `1 + currentPeekAmount`
/// tall as the lid opens. Width is
/// untouched, and so is every other attach face.
pub fn peek_height_scale(m: &Mob) -> Option<f32> {
    let peek = m.anim.v[PEEK];
    (m.extra.variant3 == DOWN && peek > 0.0).then_some(1.0 + peek)
}

pub fn register(m: &mut Mob) {
    m.no_gravity = true;
    m.yaw = 0.0;
    m.body_yaw = 0.0;
    m.previous_body_yaw = 0.0;
    if m.extra.color == 0 && m.extra.variant == 0 {
        m.extra.color = NO_COLOR;
    }
    let g = &mut m.ai.goals;
    g.add(1, LookAtPlayer::new(8.0).probability(0.02));
    g.add(4, ShulkerAttack { attack_time: 0 });
    g.add(7, Peek { time: 0 });
    g.add(8, RandomLookAround::default());
    // `ShulkerLookControl`: the head turns in the attach face's plane.
    m.ai.look.kind = LookKind::Shulker;
    // `HurtByTargetGoal(this, Shulker.class).setAlertOthers()`: damage from
    // another shulker draws no retaliation (`toIgnoreDamage`), but a hurt
    // shulker alerts every targetless shulker around.
    m.ai.targets
        .add(1, HurtByTarget::new().alert().ignoring(&[Species::Shulker]));
    // ShulkerNearestAttackGoal: the search box is only 4 deep along the
    // attach axis.
    m.ai.targets.add(
        2,
        NearestTarget::player(true).gate(peaceful_off).when(|m, w| {
            let Some(p) = &w.player else { return false };
            let d = p.position.subtract(m.position);
            let axis = match m.extra.variant3 {
                0 | 1 => d.y,
                2 | 3 => d.z,
                _ => d.x,
            };
            axis.abs() <= 4.0 + 0.5
        }),
    );
}
fn face_step(face: u8) -> [i32; 3] {
    STEPS[face.min(5) as usize]
}
/// `Direction.getOpposite` for a 3D data value.
fn opposite(face: u8) -> u8 {
    face.min(5) ^ 1
}
fn block_pos(m: &Mob) -> [i32; 3] {
    [
        m.position.x.floor() as i32,
        m.position.y.floor() as i32,
        m.position.z.floor() as i32,
    ]
}
/// `Shulker.getProgressDeltaAabb`: the shell's box of side `size` from `pos`
/// (the bottom centre), grown `from`..`to` along `direction`.
fn progress_aabb(direction: u8, from: f64, to: f64, pos: Vec3, size: f32) -> Aabb {
    let s = face_step(direction).map(f64::from);
    let size = f64::from(size);
    let (max, min) = (from.max(to), from.min(to));
    let half = size * 0.5;
    Aabb::new(Vec3::new(-half, 0.0, -half), Vec3::new(half, size, half))
        .expand_towards(Vec3::new(
            s[0] * max * size,
            s[1] * max * size,
            s[2] * max * size,
        ))
        .contract(Vec3::new(
            -s[0] * (1.0 + min) * size,
            -s[1] * (1.0 + min) * size,
            -s[2] * (1.0 + min) * size,
        ))
        .move_by(pos)
}
/// `Shulker.makeBoundingBox`: the shell's box at `position`, as wide as the
/// shulker's scale and grown along the lid's open direction by the lid's
/// lift (`getProgressAabb` up to `getPhysicalPeek(currentPeekAmount)`). The
/// `1 + currentPeekAmount` height of [`peek_height_scale`] is only the
/// entity's dimensions (its eye height), not this box.
pub fn bounds_at(m: &Mob, position: Vec3) -> Aabb {
    let lift = f64::from(physical_peek(m.anim.v[PEEK]));
    progress_aabb(
        opposite(m.extra.variant3),
        -1.0,
        lift,
        position,
        m.model_scale(),
    )
}
/// Where `ShulkerBullet(level, shulker, ...)` starts: the centre of the
/// shulker's box (`getBoundingBox().getCenter()`), which `Shulker.makeBoundingBox`
/// grows toward the open side by the lid's physical peek, so an open shulker
/// on a floor fires from the top of its block, not its middle. The attack goal
/// runs before `Shulker.tick` eases the peek, so the box is the one the peek
/// built last tick, as here.
pub fn bullet_origin(m: &Mob) -> Vec3 {
    bounds_at(m, m.position).center()
}
/// Block queries shared by attachment and teleport candidates. Entity overlap
/// is checked separately, using the nearby-entity index from MobWorld.
struct BlockSpace<'w, 'r> {
    world: &'w crate::world::World,
    registry: &'r horizon_world::registry::BlockRegistry,
    facts: &'w crate::blocks::BlockFacts,
    binding: &'w horizon_world::block_collision::BlockCollisionRegistry<'r>,
    powder_walkable: bool,
}
impl<'w, 'r> BlockSpace<'w, 'r> {
    fn context(m: &Mob) -> horizon_world::collision_context::CollisionContext<'_> {
        use horizon_world::{
            collision_context::{CollisionContext, CollisionEntity, CollisionItem},
            entity_obstruction::EntityIdentity,
        };
        CollisionContext::for_entity(
            CollisionEntity {
                kind: horizon_world::collision_context::EntityContextKind::Living,
                identity: EntityIdentity(u64::from(m.id)),
                // A teleport candidate is queried before setPos changes the entity.
                y: m.position.y,
                descending: false,
                main_hand: Some(CollisionItem {
                    item: m.extra.main_hand.as_deref().unwrap_or("minecraft:air"),
                    count: m
                        .extra
                        .main_hand_stack()
                        .map_or(i32::from(m.extra.main_hand.is_some()), |s| {
                            s.count.min(i32::MAX as u32) as i32
                        }),
                }),
            },
            false,
        )
    }
    fn scene(&self, m: &Mob) -> crate::collisions::SceneCollisions<'_, 'r> {
        crate::collisions::SceneCollisions {
            world: self.world,
            binding: self.binding,
            subject: Some(crate::collisions::PowderSnowSubject {
                fall_distance: m.fall_distance,
                leather_boots: m.extra.feet.as_deref() == Some("minecraft:leather_boots")
                    && m.extra.armor[3].as_ref().is_none_or(|s| s.count > 0),
                walkable_type: self.powder_walkable,
                ..Default::default()
            }),
        }
    }
    fn no_collision(&self, m: &Mob, bounds: Aabb) -> bool {
        use horizon_world::{
            entity_obstruction::EntityIdentity,
            world_movement::{CollisionCheckRequest, no_world_collision},
        };
        no_world_collision(
            &mut self.scene(m),
            CollisionCheckRequest {
                entity: EntityIdentity(u64::from(m.id)),
                context: Self::context(m),
                bounds,
                candidate_budget: 1_000_000,
            },
        )
        .unwrap_or(false)
    }
    fn can_stay_at(&self, m: &Mob, p: [i32; 3], face: u8) -> bool {
        // isPositionBlocked: anything but air blocks, except a moving piston
        // being driven through the shulker's own cell.
        if !self.facts.is_air(self.world.block(p[0], p[1], p[2]))
            && (p != block_pos(m)
                || self
                    .registry
                    .state(self.world.block(p[0], p[1], p[2]))
                    .map_or("", |s| s.name.as_str())
                    != "minecraft:moving_piston")
        {
            return false;
        }
        // loadedAndEntityCanStandOnFace uses the entity-context collision shape,
        // not the block's generic support shape (notably different on soul sand).
        let s = face_step(face);
        let support_pos =
            horizon_core::position::BlockPos::new(p[0] + s[0], p[1] + s[1], p[2] + s[2]);
        let support = self
            .world
            .block(support_pos.x, support_pos.y, support_pos.z);
        let open = opposite(face);
        let Ok(shape) = self.binding.collision_shape_with_context(
            &mut self.scene(m),
            support,
            support_pos,
            &Self::context(m),
        ) else {
            return false;
        };
        // Block.isFaceFull. A full cube, what shulkers cling to almost always
        // (canStayAt runs every tick), is full on every face: skip the shape ops.
        let full = shape.is_block()
            || shape
                .face_shape(Direction::ALL[open as usize])
                .is_ok_and(|face| {
                    VoxelShape::block()
                        .join_is_not_empty(&face, ShapeOp::XOR)
                        .is_ok_and(|differs| !differs)
                });
        full && self.no_collision(m, attachment_box(p, face, m.model_scale()))
    }
}
fn block_space<'w, 'r>(m: &Mob, w: &'w MobWorld<'r>) -> BlockSpace<'w, 'r> {
    BlockSpace {
        world: w.world,
        registry: w.registry,
        facts: w.facts,
        binding: w.collision,
        powder_walkable: w
            .data
            .entity_has("minecraft:powder_snow_walkable_mobs", &m.kind.id),
    }
}
fn entities_clear(m: &Mob, w: &MobWorld, bounds: Aabb) -> bool {
    let grown = bounds.inflate(Vec3::new(1.0e-7, 1.0e-7, 1.0e-7));
    !w.others_near(grown).any(|s| {
        s.id != m.id
            && s.alive
            && (s.species == Species::Shulker || (s.species == Species::HappyGhast && !s.baby))
            && s.bounds.intersects(grown)
    })
}
fn no_collision(m: &Mob, w: &MobWorld, bounds: Aabb) -> bool {
    block_space(m, w).no_collision(m, bounds) && entities_clear(m, w, bounds)
}
/// `canStayAt`'s `getProgressAabb(getScale(), ...)`: the shell of `size`
/// fully opened from face `face` of block `p`.
fn attachment_box(p: [i32; 3], face: u8, size: f32) -> Aabb {
    progress_aabb(
        opposite(face),
        -1.0,
        1.0,
        Vec3::new(
            f64::from(p[0]) + 0.5,
            f64::from(p[1]),
            f64::from(p[2]) + 0.5,
        ),
        size,
    )
    .deflate(Vec3::new(1.0e-6, 1.0e-6, 1.0e-6))
}
/// Shulker.canStayAt, including its separately indexed collidable entities.
fn can_stay_at(m: &Mob, w: &MobWorld, p: [i32; 3], face: u8) -> bool {
    block_space(m, w).can_stay_at(m, p, face)
        && entities_clear(m, w, attachment_box(p, face, m.model_scale()))
}
/// `findAttachableSurface`: the first face (down, up, north, south, west,
/// east) to cling to.
fn attachable(m: &Mob, w: &MobWorld, p: [i32; 3]) -> Option<u8> {
    (0..6).find(|&f| can_stay_at(m, w, p, f))
}
/// `BlockUtil.packDifferenceInPosition` within 8 blocks of `pos`.
fn pack_diff(pos: [i32; 3], to: [i32; 3]) -> i32 {
    ((to[0] - pos[0] + 8) & 0xFF) << 16
        | ((to[1] - pos[1] + 8) & 0xFF) << 8
        | (to[2] - pos[2] + 8) & 0xFF
}
/// `Shulker.teleportSomewhere`: up to five tries within 8 blocks; never
/// without AI.
fn teleport(m: &mut Mob, w: &MobWorld, events: &mut Vec<MobEvent>) -> bool {
    if m.no_ai || !m.alive() {
        return false;
    }
    let old = block_pos(m);
    for _ in 0..5 {
        let r = |m: &mut Mob| m.random.next_int_bound(17).unwrap_or(8) - 8;
        let t = [old[0] + r(m), old[1] + r(m), old[2] + r(m)];
        let unit = Aabb::new(
            Vec3::new(f64::from(t[0]), f64::from(t[1]), f64::from(t[2])),
            Vec3::new(
                f64::from(t[0]) + 1.0,
                f64::from(t[1]) + 1.0,
                f64::from(t[2]) + 1.0,
            ),
        );
        if t[1] <= w.world.bottom_y()
            || !w.facts.is_air(w.world.block(t[0], t[1], t[2]))
            // WorldBorder.isWithinBounds(BlockPos): the block's corner, no margin.
            || w.border
                .is_some_and(|b| !b.is_within_bounds(f64::from(t[0]), f64::from(t[2]), 0.0))
            || !no_collision(m, w, unit.deflate(Vec3::new(1.0e-6, 1.0e-6, 1.0e-6)))
        {
            continue;
        }
        let Some(face) = attachable(m, w, t) else {
            continue;
        };
        // `#minecraft:shulker_does_not_teleport_to` (bedrock) is never clung to.
        let s = face_step(face);
        if w.has_tag(
            "minecraft:shulker_does_not_teleport_to",
            w.block_name(t[0] + s[0], t[1] + s[1], t[2] + s[2]),
        ) {
            continue;
        }
        // unRide: whoever rides it gets off where it stands, and it gets
        // off its own vehicle (it lands at the target below).
        events.push(MobEvent::EjectPassengers {
            mob: m.id,
            from: m.position,
        });
        super::riders::stop_riding(m, None);
        m.extra.variant3 = face;
        events.push(MobEvent::Sound(
            "entity.shulker.teleport",
            [m.position.x, m.position.y, m.position.z],
            1.0,
            1.0,
        ));
        m.position = Vec3::new(
            f64::from(t[0]) + 0.5,
            f64::from(t[1]),
            f64::from(t[2]) + 0.5,
        );
        m.previous = m.position;
        // `level.gameEvent(GameEvent.TELEPORT, oldPos, Context.of(this))`:
        // the BlockPos overload uses `Vec3.atCenterOf(oldPos)`, between the
        // sound and the level event.
        events.push(MobEvent::GameEvent {
            event: "minecraft:teleport",
            at: [
                f64::from(old[0]) + 0.5,
                f64::from(old[1]) + 0.5,
                f64::from(old[2]) + 0.5,
            ],
            source: m.id,
            state: horizon_world::registry::BlockStateId(0),
        });
        // Level.levelEvent(2016, oldPos, packedDiff): the teleport's
        // particles at both ends, seen by the players around.
        events.push(MobEvent::LevelEvent {
            id: 2016,
            pos: old,
            data: pack_diff(old, t),
        });
        m.needs_sync = true;
        m.extra.counter = 0;
        m.ai.target = None;
        return true;
    }
    false
}
/// `Shulker.setRawPeekAmount`: the shell opens or closes with a sound.
fn set_peek(m: &mut Mob, amount: i32, events: &mut Vec<MobEvent>) {
    let sound = if amount == 0 {
        "entity.shulker.close"
    } else {
        "entity.shulker.open"
    };
    events.push(MobEvent::Sound(
        sound,
        [m.position.x, m.position.y, m.position.z],
        1.0,
        1.0,
    ));
    // Shulker.setRawPeekAmount: CONTAINER_CLOSE as the lid shuts, CONTAINER_OPEN as it opens.
    events.push(MobEvent::GameEvent {
        event: if amount == 0 {
            "minecraft:container_close"
        } else {
            "minecraft:container_open"
        },
        at: [m.position.x, m.position.y, m.position.z],
        source: m.id,
        state: horizon_world::registry::BlockStateId(0),
    });
    m.extra.counter = amount;
}
/// `Shulker.updatePeekAmount`/`updatePeekAmountO`: the eased peek.
fn ease_peek(m: &mut Mob) -> (f32, f32) {
    let old = m.anim.v[PEEK];
    let target = m.extra.counter as f32 * 0.01;
    // `Mth.clamp(float, float, float)` tolerates `max < min` (a negative or
    // oversized raw `Peek`), where `f32::clamp` would panic.
    let mth_clamp = |v: f32, lo: f32, hi: f32| if v < lo { lo } else { v.min(hi) };
    let new = if old > target {
        mth_clamp(old - 0.05, target, 1.0)
    } else {
        mth_clamp(old + 0.05, 0.0, target)
    };
    m.anim.v[PEEK] = new;
    (old, new)
}
/// `Entity.load` on a live shell (`/data merge`): `readAdditionalSaveData`
/// reads only the raw `Peek`, the lid's target, so `currentPeekAmount` keeps
/// its eased value from `old` and the lid eases toward the new target. (A
/// shell loaded or summoned fresh still starts with its lid at the raw
/// `Peek` here, where vanilla's starts shut and eases open.)
pub fn keep_eased_peek(slot: &mut Mob, old: &Mob) {
    slot.anim.v[PEEK] = old.anim.v[PEEK];
    slot.anim.vo[PEEK] = old.anim.vo[PEEK];
}
/// `Shulker.tick` additions: stay still and square on its block, then
/// [`shell_tick`].
pub fn tick(m: &mut Mob, w: &MobWorld, events: &mut Vec<MobEvent>) {
    m.velocity = Vec3::ZERO;
    m.body_yaw = 0.0;
    m.yaw = 0.0;
    m.head_pitch = 0.0;
    shell_tick(m, w, events);
}
/// `Shulker.tick`'s own part, which runs with or without AI: a shell that
/// rides nothing keeps a face (or teleports), and the lid eases toward its
/// raw peek, pushing whatever it opens into.
pub fn shell_tick(m: &mut Mob, w: &MobWorld, events: &mut Vec<MobEvent>) {
    let p = block_pos(m);
    if m.extra.mount.is_none() && !can_stay_at(m, w, p, m.extra.variant3) {
        match attachable(m, w, p) {
            Some(f) => m.extra.variant3 = f,
            None => {
                teleport(m, w, events);
            }
        }
    }
    let (old, new) = ease_peek(m);
    if new != old {
        on_peek_change(m, w, old, new, events);
    }
}
/// `Shulker.getPhysicalPeek`: the lid's lift for a peek amount.
fn physical_peek(amount: f32) -> f32 {
    0.5 - ((0.5 + amount) * PI).msin() * 0.5
}
/// `Shulker.onPeekAmountChange`'s push: the step out of the lid's way (the
/// lift gained, in float, times the scale) and the box the lid swept, while
/// it opens.
fn peek_push(m: &Mob, old: f32, new: f32) -> Option<([f64; 3], Aabb)> {
    let dir = opposite(m.extra.variant3);
    let s = face_step(dir);
    let size = m.model_scale();
    let (old_p, new_p) = (physical_peek(old), physical_peek(new));
    let push = (new_p - old_p) * size;
    if push <= 0.0 {
        return None;
    }
    let delta = s.map(|step| f64::from(push * step as f32));
    let area = progress_aabb(dir, f64::from(old_p), f64::from(new_p), m.position, size);
    Some((delta, area))
}
/// `Shulker.onPeekAmountChange`: an opening shell pushes out of its way
/// every entity that can be collided with (except other shulkers): the
/// player and the mobs here, the armor stands through
/// [`MobEvent::LidPush`].
fn on_peek_change(m: &Mob, w: &MobWorld, old: f32, new: f32, events: &mut Vec<MobEvent>) {
    let Some((delta, area)) = peek_push(m, old, new) else {
        return;
    };
    events.push(MobEvent::LidPush {
        shulker: m.id,
        area,
        delta,
    });
    if let Some(p) = &w.player
        && p.alive
        && !p.spectator
        && p.bounds.intersects(area)
    {
        events.push(MobEvent::Shift {
            target: Who::Player,
            delta,
        });
    }
    for seen in w.others_near(area) {
        if seen.id == m.id
            || !seen.alive
            || seen.species == Species::Shulker
            || !seen.bounds.intersects(area)
        {
            continue;
        }
        events.push(MobEvent::Shift {
            target: Who::Mob(seen.id),
            delta,
        });
    }
}
/// `Shulker.tick`'s `onPeekAmountChange` on a client that is not the server,
/// after `client_tick` eased the lid: the client moves its own player, so a
/// lid opening into `player` (the local player's box, alive and not a
/// spectator) pushes it out of the way.
pub fn client_push(m: &Mob, player: Aabb, events: &mut Vec<MobEvent>) {
    let (old, new) = (m.anim.vo[PEEK], m.anim.v[PEEK]);
    if new == old {
        return;
    }
    if let Some((delta, area)) = peek_push(m, old, new)
        && player.intersects(area)
    {
        events.push(MobEvent::Shift {
            target: Who::Player,
            delta,
        });
    }
}
/// `ShulkerLookControl.getYRotD`: the look yaw in the open direction's
/// plane (`None`: the target is on the head's own axis).
pub fn look_yaw(m: &Mob, at: [f64; 3]) -> Option<f32> {
    let open = opposite(m.extra.variant3);
    // `attachFace.getRotation().transform(FORWARD)`: the model's forward
    // rotated as the renderer does (`rotations` uses the same quats).
    let forward = face_quat(m.extra.variant3) * glam::Vec3::Z;
    let s = face_step(open).map(|v| v as f32);
    let right = glam::Vec3::from_array(s).cross(forward);
    let out = glam::Vec3::new(
        (at[0] - m.position.x) as f32,
        (at[1] - m.eye_y()) as f32,
        (at[2] - m.position.z) as f32,
    );
    let (delta_right, delta_forward) = (right.dot(out), forward.dot(out));
    if delta_right.abs() <= 1.0e-5 && delta_forward.abs() <= 1.0e-5 {
        return None;
    }
    Some(horizon_core::geometry::trig::atan2_degrees(
        f64::from(-delta_right),
        f64::from(delta_forward),
    ))
}
/// `Shulker.tick` on a client that is not the server: the teleport slide
/// counts down (`setPos` armed it when the move packet arrived), the lid
/// eases to the peek it was sent, and the body stays square
/// (`ShulkerBodyRotationControl`).
pub fn client_tick(m: &mut Mob) {
    if m.extra.scratch[0] > 0 {
        m.extra.scratch[0] -= 1;
    }
    m.anim.vo = m.anim.v;
    ease_peek(m);
    m.body_yaw = 0.0;
}
/// `Shulker.setPos`. A shulker's interpolation handler is `NO_OP`, so a move
/// packet lands here instead of being stepped. Not a passenger: the position
/// snaps to the block centre. A block change after the first tick closes the
/// lid, and, off a vehicle and other than toward the block the current slide
/// remembers, starts the 6-tick slide from the block it was in before the
/// packet and snaps `previous` so ordinary interpolation does not also run.
pub fn client_set_pos(m: &mut Mob, x: f64, y: f64, z: f64) {
    let old = block_pos(m);
    m.position = if m.extra.passenger {
        Vec3::new(x, y, z)
    } else {
        Vec3::new(x.floor() + 0.5, (y + 0.5).floor(), z.floor() + 0.5)
    };
    // `setPos` sees `tickCount == 0` and starts nothing.
    if m.age == 0 {
        return;
    }
    let pos = block_pos(m);
    if pos == old {
        return;
    }
    m.extra.counter = 0;
    if m.extra.passenger {
        return;
    }
    let remembered = [m.extra.scratch[1], m.extra.scratch[2], m.extra.scratch[3]];
    if m.extra.scratch[0] > 0 && pos == remembered {
        return;
    }
    m.extra.scratch[1] = old[0];
    m.extra.scratch[2] = old[1];
    m.extra.scratch[3] = old[2];
    m.extra.scratch[0] = 6;
    m.previous = m.position;
}
/// `Shulker.getRenderPosition`: while the slide has ticks left, the model is
/// pulled back toward the block it left by `((ticks - partial) / 6)^2`,
/// times `getScale`. Vanilla's `null` is no offset.
pub fn render_offset(m: &Mob, alpha: f32) -> glam::DVec3 {
    let ticks = m.extra.scratch[0];
    if m.species() != Species::Shulker || ticks <= 0 {
        return glam::DVec3::ZERO;
    }
    let mut scale = (f64::from(ticks) - f64::from(alpha)) / 6.0;
    scale *= scale;
    scale *= f64::from(m.model_scale());
    let current = block_pos(m);
    let old = [m.extra.scratch[1], m.extra.scratch[2], m.extra.scratch[3]];
    glam::DVec3::new(
        -f64::from(current[0] - old[0]) * scale,
        -f64::from(current[1] - old[1]) * scale,
        -f64::from(current[2] - old[2]) * scale,
    )
}
/// `Shulker.hurtServer` before the damage: arrows bounce off a closed shell.
pub fn accepts(m: &Mob, source: &Damage) -> bool {
    !(m.extra.counter == 0 && source.kind == DamageKind::Arrow)
}
/// `Shulker.hurtServer` after the damage: below half health, a one in four
/// chance to teleport away.
pub fn after_hurt(m: &mut Mob) {
    if m.health < m.attrs.max_health * 0.5 && m.random.next_int_bound(4).unwrap_or(1) == 0 {
        // The teleport needs the world, which `Mob::damage` has not: whoever
        // dealt the hurt runs it right after (`hurt_teleport`,
        // `run_pending_teleports`).
        m.extra.flag = true;
    }
}
/// `Shulker.hurtServer` after a shulker bullet's damage: unless the
/// low-health teleport was drawn instead (`else if`), `hitByShulkerBullet`
/// runs right after the hurt (`run_pending_teleports`). Whether the shell is
/// open is read here, at hurt time.
pub fn hit_by_bullet(m: &mut Mob) {
    if m.species() == Species::Shulker && !m.extra.flag && m.extra.counter != 0 {
        m.extra.flag2 = true;
    }
}
/// Whether a hurt left `Shulker.hurtServer` a teleport to make.
pub fn teleport_pending(m: &Mob) -> bool {
    m.species() == Species::Shulker && (m.extra.flag || m.extra.flag2)
}
/// `Shulker.hurtServer`'s low-health `teleportSomewhere()`, inside the hurt
/// `after_hurt` drew it for (a hurt within the shulker's own tick).
pub fn hurt_teleport(m: &mut Mob, w: &MobWorld, events: &mut Vec<MobEvent>) {
    if m.species() == Species::Shulker && m.extra.flag {
        m.extra.flag = false;
        teleport(m, w, events);
    }
}
/// The teleport `Shulker.hurtServer` makes inside a hurt: the low-health
/// `teleportSomewhere()` (`after_hurt`), else a bullet's
/// `hitByShulkerBullet()` (`hit_by_bullet`).
pub fn run_pending_teleports(
    m: &mut Mob,
    w: &MobWorld,
    level_random: &mut LegacyRandom,
    events: &mut Vec<MobEvent>,
) {
    hurt_teleport(m, w, events);
    if m.extra.flag2 {
        m.extra.flag2 = false;
        hit_by_shulker_bullet(m, w, level_random, events);
    }
}
/// What a hurt left that no one ran right after it (a hurt the game layer
/// does not settle at once).
pub fn ai_step(
    m: &mut Mob,
    w: &MobWorld,
    level_random: &mut LegacyRandom,
    events: &mut Vec<MobEvent>,
) {
    run_pending_teleports(m, w, level_random, events);
}
/// `Shulker.hitByShulkerBullet` (only queued by an open shulker): it
/// teleports, and leaves a new one of its colour behind unless shulkers
/// crowd the place (one in five fewer chances for every other shulker
/// within 8 blocks).
fn hit_by_shulker_bullet(
    m: &mut Mob,
    w: &MobWorld,
    level_random: &mut LegacyRandom,
    events: &mut Vec<MobEvent>,
) {
    let old_position = m.position;
    let old_box = m.bounds();
    if !teleport(m, w, events) {
        return;
    }
    let area = old_box.inflate(Vec3::new(8.0, 8.0, 8.0));
    // The shulker itself stands within reach of its old box, so it counts.
    let others = w
        .others_near(area)
        .filter(|s| {
            s.alive && s.species == Species::Shulker && s.id != m.id && s.bounds.intersects(area)
        })
        .count();
    let failure_chance = others as f32 / 5.0;
    // The level's random, not the shulker's.
    if level_random.next_float() < failure_chance {
        return;
    }
    let mut baby = Mob::new(0, m.kind.clone(), old_position, 0.0);
    baby.extra.color = m.extra.color;
    events.push(MobEvent::Spawn(Box::new(baby)));
}
/// A ProjectileWorld over the mob world, for aiming a new bullet.
struct Probe<'a, 'w> {
    w: &'a MobWorld<'w>,
    targets: Vec<(Who, Aabb)>,
}
impl ProjectileWorld for Probe<'_, '_> {
    fn clip(&self, _: Vec3, _: Vec3) -> Option<(Vec3, [i32; 3], [i32; 3])> {
        None
    }
    fn in_water(&self, _: Aabb) -> bool {
        false
    }
    fn targets(&self) -> &[(Who, Aabb)] {
        &self.targets
    }
    fn empty(&self, p: [i32; 3]) -> bool {
        self.w.facts.is_air(self.w.world.block(p[0], p[1], p[2]))
    }
    fn can_stand_on(&self, p: [i32; 3], entity_y: f64) -> bool {
        crate::collisions::projectile_can_stand_on(
            self.w.world,
            self.w.collision,
            horizon_core::position::BlockPos::new(p[0], p[1], p[2]),
            entity_y,
        )
    }
}
/// `level().getDifficulty() != Difficulty.PEACEFUL`, which the shulker's
/// attack goal and target goal check.
fn peaceful_off(_m: &Mob, w: &MobWorld) -> bool {
    w.difficulty.effective != 0.0
}
/// `ShulkerAttackGoal`: open up and fire a bullet every one to five and a
/// half seconds at a target within 20 blocks.
#[derive(Clone, Debug)]
struct ShulkerAttack {
    attack_time: i32,
}
impl Goal for ShulkerAttack {
    goal_clone!();
    fn flags(&self) -> u8 {
        MOVE | LOOK
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        m.target_info(cx.w).is_some_and(|t| t.alive) && peaceful_off(m, cx.w)
    }
    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        self.attack_time = 20;
        set_peek(m, 100, cx.events);
    }
    fn stop(&mut self, m: &mut Mob, cx: &mut Cx) {
        set_peek(m, 0, cx.events);
    }
    /// `Shulker.ShulkerAttackGoal.tick`: the bullet starts at the centre of
    /// the shulker's box ([`bullet_origin`]).
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        if !peaceful_off(m, cx.w) {
            return;
        }
        self.attack_time -= 1;
        let (Some(who), Some(t)) = (m.ai.target, m.target_info(cx.w)) else {
            return;
        };
        m.look_at_speed([t.position.x, t.eye_y, t.position.z], 180.0, 180.0);
        if t.position.distance_squared(m.position) >= 400.0 {
            m.ai.target = None;
            return;
        }
        if self.attack_time > 0 {
            return;
        }
        self.attack_time = 20 + m.random.next_int_bound(10).unwrap_or(0) * 20 / 2;
        let probe = Probe {
            w: cx.w,
            targets: vec![(who, t.bounds)],
        };
        let axis = match m.extra.variant3 {
            0 | 1 => 1,
            2 | 3 => 2,
            _ => 0,
        };
        let seed = m.random.next_long() as u64 | 1;
        let bullet =
            Projectile::shulker_bullet(&probe, bullet_origin(m), Who::Mob(m.id), who, axis, seed);
        cx.events.push(MobEvent::Projectile(bullet));
        let pitch = (m.random.next_float() - m.random.next_float()) * 0.2 + 1.0;
        cx.events.push(MobEvent::Sound(
            "entity.shulker.shoot",
            [m.position.x, m.position.y, m.position.z],
            2.0,
            pitch,
        ));
    }
}
/// `ShulkerPeekGoal`: idle peeks of one to three seconds.
#[derive(Clone, Debug)]
struct Peek {
    time: i32,
}
impl Goal for Peek {
    goal_clone!();
    fn flags(&self) -> u8 {
        0
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        m.ai.target.is_none()
            && m.random.next_int_bound(reduced(40)).unwrap_or(1) == 0
            && can_stay_at(m, cx.w, block_pos(m), m.extra.variant3)
    }
    fn can_continue(&mut self, m: &mut Mob, _cx: &mut Cx) -> bool {
        m.ai.target.is_none() && self.time > 0
    }
    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        self.time = reduced(20 * (1 + m.random.next_int_bound(3).unwrap_or(0)));
        set_peek(m, 30, cx.events);
    }
    fn stop(&mut self, m: &mut Mob, cx: &mut Cx) {
        if m.ai.target.is_none() {
            set_peek(m, 0, cx.events);
        }
    }
    fn tick(&mut self, _m: &mut Mob, _cx: &mut Cx) {
        self.time -= 1;
    }
}
/// The shell's texture: plain or dyed.
pub fn texture(m: &Mob) -> String {
    const COLORS: [&str; 16] = [
        "white",
        "orange",
        "magenta",
        "light_blue",
        "yellow",
        "lime",
        "pink",
        "gray",
        "light_gray",
        "cyan",
        "purple",
        "blue",
        "brown",
        "green",
        "red",
        "black",
    ];
    // Shulker.getColor: the saved byte is signed; 16 and above are undyed, and
    // DyeColor.byId sends a negative id to white (OutOfBoundsStrategy.ZERO).
    match m.extra.color as i8 {
        16.. => "entity/shulker/shulker".into(),
        c => format!("entity/shulker/shulker_{}", COLORS[c.max(0) as usize]),
    }
}
/// `attachFace.getOpposite().getRotation()` by attach face: the open
/// direction's rotation (`Direction.getRotation` of `face ^ 1`).
fn face_quat(face: u8) -> glam::Quat {
    match face.min(5) {
        // Attached below: upright.
        0 => glam::Quat::IDENTITY,
        1 => glam::Quat::from_rotation_x(PI),
        2 => glam::Quat::from_rotation_x(PI / 2.0),
        3 => glam::Quat::from_euler(glam::EulerRot::XYZ, PI / 2.0, 0.0, PI),
        4 => glam::Quat::from_euler(glam::EulerRot::XYZ, PI / 2.0, 0.0, -PI / 2.0),
        _ => glam::Quat::from_euler(glam::EulerRot::XYZ, PI / 2.0, 0.0, PI / 2.0),
    }
}
/// `ShulkerRenderer.setupRotations`: turned onto its attach face around the
/// block's centre (after a half turn of the body).
pub fn rotations(m: &Mob) -> glam::Mat4 {
    let q = face_quat(m.extra.variant3);
    glam::Mat4::from_rotation_y(PI)
        * glam::Mat4::from_translation(glam::Vec3::new(0.0, 0.5, 0.0))
        * glam::Mat4::from_quat(q)
        * glam::Mat4::from_translation(glam::Vec3::new(0.0, -0.5, 0.0))
}
/// `ShulkerModel.setupAnim`: the lid rises and twists with the peek, the
/// head turns.
pub fn anim(m: &Mob, s: &State, p: &mut Poses) {
    let peek = m.anim.lerp(PEEK, s.partial);
    let bs = (0.5 + peek) * PI;
    let q = -1.0 + bs.msin();
    let extra = if bs > PI {
        (s.age * 0.1).msin() * 0.7
    } else {
        0.0
    };
    let lid = p.get("lid");
    lid[1] = 16.0 + bs.msin() * 8.0 + extra;
    lid[4] = if peek > 0.3 {
        q * q * q * q * PI * 0.125
    } else {
        0.0
    };
    p.xrot("head", s.x_rot.to_radians());
    p.yrot("head", (s.y_rot - 180.0).to_radians());
}

#[cfg(test)]
#[path = "shulker_space_tests.rs"]
mod space_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mobs::sensor_scan_tests::kind;
    use std::sync::Arc;

    /// `Shulker.setPos`: a piston's push lands on the block centre, the y
    /// rounded by `floor(y + 0.5)`, not left at the pushed offset.
    #[test]
    fn a_pushed_position_snaps_to_the_block_centre() {
        let mut m = shulker();
        m.position = Vec3::new(0.5, 4.0, 0.5);
        client_set_pos(&mut m, 0.5, 3.49, 0.5);
        assert_eq!(m.position, Vec3::new(0.5, 3.0, 0.5));
        client_set_pos(&mut m, 2.9, 3.0, -0.2);
        assert_eq!(m.position, Vec3::new(2.5, 3.0, -0.5));
    }

    /// `Mth.clamp` saturates when the target is below the floor: a negative
    /// raw `Peek` eases the lid there once and then holds, no panic.
    #[test]
    fn a_negative_peek_target_does_not_panic() {
        let mut m = shulker();
        m.extra.counter = -1;
        for _ in 0..3 {
            ease_peek(&mut m);
        }
        assert_eq!(m.anim.v[PEEK], -0.01);
    }

    /// Shulker.setRawPeekAmount: the lid opening posts CONTAINER_OPEN and shutting
    /// CONTAINER_CLOSE, for the sculk sensors and wardens nearby.
    #[test]
    fn the_lid_posts_container_open_and_close() {
        let posted = |events: &[MobEvent]| -> Vec<&'static str> {
            events
                .iter()
                .filter_map(|e| match e {
                    MobEvent::GameEvent { event, .. } => Some(*event),
                    _ => None,
                })
                .collect()
        };
        let mut m = shulker();
        let mut events = Vec::new();
        set_peek(&mut m, 30, &mut events);
        assert_eq!(posted(&events), ["minecraft:container_open"]);
        let mut events = Vec::new();
        set_peek(&mut m, 0, &mut events);
        assert_eq!(posted(&events), ["minecraft:container_close"]);
    }

    /// A shulker fixed to a block face.
    pub(super) fn shulker() -> Mob {
        let mut kind = kind();
        let k = Arc::make_mut(&mut kind);
        k.id = "minecraft:shulker".into();
        k.species = Species::Shulker;
        k.max_head_y_rot = 180.0;
        // EntityTypes.SHULKER: sized(1.0F, 1.0F).eyeHeight(0.5F).
        k.width = 1.0;
        k.height = 1.0;
        k.eye_height = 0.5;
        let mut m = Mob::new(1, kind, Vec3::ZERO, 0.0);
        m.extra.variant3 = 2;
        m
    }

    /// A box query over many mobs (a boat's) tests their positions against
    /// `BoxReach` first. It has to reach an open lid on every face:
    /// `makeBoundingBox` grows the shell a block out from its position,
    /// sideways or below it too.
    #[test]
    fn box_reach_reaches_an_open_lid_on_every_face() {
        for face in 0..6u8 {
            let mut m = shulker();
            m.position = Vec3::new(0.5, 4.0, 0.5);
            m.extra.variant3 = face;
            m.anim.v[PEEK] = 1.0;
            let reach = crate::mobs::BoxReach::of(std::slice::from_ref(&m));
            // A small box near the lid's far end, 1.4 out from the shell's
            // centre, where only the open lid reaches.
            let s = face_step(opposite(face)).map(f64::from);
            let tip = Vec3::new(0.5 + s[0] * 1.4, 4.5 + s[1] * 1.4, 0.5 + s[2] * 1.4);
            let area = Aabb::new(
                Vec3::new(tip.x - 0.05, tip.y - 0.05, tip.z - 0.05),
                Vec3::new(tip.x + 0.05, tip.y + 0.05, tip.z + 0.05),
            );
            assert!(
                m.bounds().intersects(area),
                "face {face}: the lid reaches {tip:?}"
            );
            assert!(
                reach.may_meet(&m, area),
                "face {face}: the reach misses the lid"
            );
        }
    }

    /// `Shulker.hurtServer` returns false for an `AbstractArrow` while
    /// `isClosed` (raw peek 0). An open shell takes the same arrow.
    #[test]
    fn a_closed_shell_refuses_an_arrow() {
        let source = Damage::of(DamageKind::Arrow);
        let mut closed = shulker();
        closed.extra.counter = 0;
        let health = closed.health;
        assert!(
            !closed.damage_projectile(2.0, source, None, [0.0, 1.0]),
            "a closed shell rejects the arrow"
        );
        assert_eq!(closed.health, health);
        let mut open = shulker();
        open.extra.counter = 30;
        assert!(open.damage_projectile(2.0, source, None, [0.0, 1.0]));
        assert!(open.health < health);
    }

    #[test]
    fn a_move_packet_starts_the_teleport_slide() {
        // Shulker.createInterpolationHandler is NO_OP, so the packet's end
        // position is setPos. The slide is armed there, from the block before
        // the packet, and client_tick only counts it down.
        let mut m = shulker();
        m.age = 10;
        m.position = Vec3::new(0.5, 0.0, 0.5);
        m.previous = m.position;
        client_set_pos(&mut m, 3.2, 1.0, 0.4);
        assert_eq!(m.position, Vec3::new(3.5, 1.0, 0.5));
        assert_eq!(m.previous, m.position);
        assert_eq!(m.extra.scratch[0], 6);
        assert_eq!(render_offset(&m, 0.0), glam::DVec3::new(-3.0, -1.0, 0.0));
        client_tick(&mut m);
        assert_eq!(m.extra.scratch[0], 5);
        // A tick with no new packet does not look at previous and re-arm.
        m.previous = Vec3::new(80.0, 0.0, 0.0);
        client_tick(&mut m);
        assert_eq!(m.extra.scratch[0], 4);
    }

    #[test]
    fn a_riding_shulker_closes_its_lid_but_does_not_slide() {
        // Shulker.setPos on a passenger: the exact position, the peek set to
        // 0 on a block change, and no clientSideTeleportInterpolation.
        let mut m = shulker();
        m.age = 10;
        m.extra.passenger = true;
        m.extra.counter = 30;
        m.position = Vec3::new(0.5, 0.0, 0.5);
        client_set_pos(&mut m, 2.25, 0.4, 0.5);
        assert_eq!(m.position, Vec3::new(2.25, 0.4, 0.5));
        assert_eq!(m.extra.counter, 0);
        assert_eq!(m.extra.scratch[0], 0);
        assert_eq!(render_offset(&m, 0.0), glam::DVec3::ZERO);
    }

    #[test]
    fn the_body_stays_square_while_the_head_turns() {
        // Shulker.recreateFromPacket sets the body rotation to 0 and
        // Shulker.ShulkerBodyRotationControl.clientTick is empty, so the
        // shulker's body never turns: it stays square to the block it is
        // fixed to and only its head turns (#1661).
        let mut m = shulker();
        m.yaw = 90.0;
        m.head_yaw = 90.0;
        // It moved, so a body that follows the mob would turn with it.
        m.previous = Vec3::new(1.0, 0.0, 0.0);
        m.body_rotation_tick();
        assert_eq!(
            m.body_yaw, 0.0,
            "the shulker's body stays square to the block"
        );
        assert_eq!(m.previous_body_yaw, 0.0);
        // A mob that moves turns its body to where it goes.
        let mut other = Mob::new(2, kind(), Vec3::ZERO, 0.0);
        other.yaw = 90.0;
        other.previous = Vec3::new(1.0, 0.0, 0.0);
        other.body_rotation_tick();
        assert_eq!(other.body_yaw, 90.0);
    }

    #[test]
    fn the_head_turns_to_its_target() {
        // ShulkerLookControl.clampHeadRotationToBody does nothing, so the
        // head is not clamped to the square body: it turns to the target.
        let mut m = shulker();
        m.ai.look.kind = LookKind::Shulker;
        for _ in 0..20 {
            // A goal keeps the look up to date every tick.
            m.ai.look.set_look_at([5.0, 0.0, 0.0], 10.0, 10.0);
            m.look_control_tick();
        }
        assert!(
            m.head_yaw.abs() > 45.0,
            "the head turns to the target, not back to the square body"
        );
        assert_eq!(m.body_yaw, 0.0);
    }

    #[test]
    fn an_open_shell_on_the_floor_stands_taller() {
        // Shulker.getDefaultDimensions: a shell on Direction.DOWN (attach
        // face 0) with currentPeekAmount p is `scale(1, 1 + p)` of its
        // 1 x 1 x 0.5 box — the width is untouched, the height and the eyes
        // grow. A client eases the peek 0.05 a tick towards the synced raw
        // peek, 100 while ShulkerAttackGoal holds the lid open.
        let mut m = shulker();
        m.extra.variant3 = DOWN;
        m.extra.counter = 100;
        for _ in 0..10 {
            client_tick(&mut m);
        }
        assert!((m.anim.v[PEEK] - 0.5).abs() < 1e-6, "{}", m.anim.v[PEEK]);
        let (w, h, e) = m.dims();
        assert!(
            (w - 1.0).abs() < 1e-9 && (h - 1.5).abs() < 1e-5 && (e - 0.75).abs() < 1e-5,
            "a half open shell is 1 x 1.5 with its eyes at 0.75: {w} x {h} x {e}"
        );
        // The culling (and the eye height) use `height`; the collision box
        // is `bounds_at` (`Shulker.makeBoundingBox`), not built from it.
        assert!((f64::from(m.height()) - 1.5).abs() < 1e-5, "{}", m.height());
    }

    #[test]
    fn only_a_shell_on_the_floor_grows() {
        // Every other attach face keeps the plain 1 x 1 x 0.5 box, however
        // far the lid opens.
        for face in [1u8, 2, 3, 4, 5] {
            let mut m = shulker();
            m.extra.variant3 = face;
            m.extra.counter = 100;
            for _ in 0..20 {
                client_tick(&mut m);
            }
            assert_eq!(m.anim.v[PEEK], 1.0);
            let (w, h, e) = m.dims();
            assert!(
                (w - 1.0).abs() < 1e-9 && (h - 1.0).abs() < 1e-9 && (e - 0.5).abs() < 1e-9,
                "a shell on face {face} stays 1 x 1: {w} x {h} x {e}"
            );
        }
        // A closed one on the floor does not grow either.
        let mut m = shulker();
        m.extra.variant3 = DOWN;
        let (_, h, _) = m.dims();
        assert!((h - 1.0).abs() < 1e-9, "{h}");
    }
}

#[cfg(test)]
mod saved_bytes_tests {
    use super::*;

    /// `Shulker.readAdditionalSaveData`: `AttachFace` is
    /// `Direction.LEGACY_ID_CODEC` (`from3DDataValue` wraps the id through
    /// `abs(id % 6)`), while `Peek` and `Color` are raw `getByteOr` bytes. The
    /// load clamped all three to their setters' ranges instead.
    #[test]
    fn a_shulker_reads_its_face_and_lid_bytes_like_vanilla() {
        use crate::entity_nbt::mob_from_nbt;
        use horizon_world::nbt::Tag;
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        let k = std::sync::Arc::make_mut(&mut kind);
        k.id = "minecraft:shulker".into();
        k.species = Species::Shulker;
        let types = [("minecraft:shulker".into(), kind)].into();
        let tag = Tag::compound([
            ("id", Tag::String("minecraft:shulker".into())),
            (
                "Pos",
                Tag::List(vec![
                    Tag::Double(0.0f64.to_bits()),
                    Tag::Double(64.0f64.to_bits()),
                    Tag::Double(0.0f64.to_bits()),
                ]),
            ),
            ("AttachFace", Tag::Byte(7)),
            ("Peek", Tag::Byte(-1)),
            ("Color", Tag::Byte(-1)),
        ]);
        let m = mob_from_nbt(&tag, &types, 1, None).unwrap();
        assert_eq!(m.extra.variant3, 1, "abs(7 % 6) is up");
        assert_eq!(m.extra.counter, -1, "the peek byte is kept");
        assert_eq!(m.extra.color, 255, "the color byte is kept");
        assert_eq!(
            texture(&m),
            "entity/shulker/shulker_white",
            "DyeColor.byId(-1)"
        );
    }
}
