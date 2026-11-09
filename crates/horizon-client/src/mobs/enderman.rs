//! Endermen: anger from being stared at, teleporting (randomly in daylight,
//! when hit by projectiles or water, and towards a distant target), freezing
//! while looked at, and picking up / placing `enderman_holdable` blocks.
use super::{
    Damage, Mob, MobEvent, MobWorld, Species, Who,
    anim::{Poses, State},
    goals::{
        Cx, Float, Goal, HurtByTarget, JUMP, LookAtPlayer, MOVE, MeleeAttack, NearestTarget,
        RandomLookAround, Stroll, TARGET, goal_clone, reduced,
    },
};
use crate::{
    blocks::Fluid,
    items::{ItemStack, Items},
    random_sequences::RandomSequences,
};
use horizon_core::{
    geometry::{Aabb, Vec3},
    random::RandomSource,
};
use horizon_world::registry::{BlockRegistry, BlockStateId};

pub fn register(m: &mut Mob) {
    let g = &mut m.ai.goals;
    g.add(0, Float);
    g.add(1, FreezeWhenLookedAt);
    g.add(2, MeleeAttack::new(1.0, false));
    let mut stroll = Stroll::avoiding_water(1.0);
    stroll.water_avoiding = Some(0.0);
    g.add(7, stroll);
    g.add(8, LookAtPlayer::new(8.0));
    g.add(8, RandomLookAround::default());
    g.add(10, LeaveBlock);
    g.add(11, TakeBlock);
    m.ai.targets.add(
        1,
        LookForPlayer {
            pending: false,
            aggro: 0,
            teleport: 0,
        },
    );
    m.ai.targets.add(2, HurtByTarget::new());
    m.ai.targets.add(
        3,
        NearestTarget::species(&[Species::Endermite], true).reach(),
    );
    m.set_malus(super::path::PathType::Water, -1.0);
}
/// Enderman.isBeingStaredBy: the player's look within a cone of 0.025 / distance
/// of the eyes, with line of sight, and no carved pumpkin.
pub fn stared_at(m: &Mob, w: &MobWorld) -> bool {
    let Some(p) = &w.player else { return false };
    if !p.alive || p.head.as_deref() == Some("minecraft:carved_pumpkin") {
        return false;
    }
    let look = p.look;
    let dir = [
        m.position.x - p.position.x,
        m.eye_y() - p.eye[1],
        m.position.z - p.position.z,
    ];
    let dist = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    if dist < 1.0e-6 {
        return false;
    }
    let dot = (look[0] * dir[0] + look[1] * dir[1] + look[2] * dir[2]) / dist;
    dot > 1.0 - 0.025 / dist
        && w.line_of_sight(
            Vec3::new(p.eye[0], p.eye[1], p.eye[2]),
            Vec3::new(m.position.x, m.eye_y(), m.position.z),
        )
}
/// `Enderman.teleport(x, y, z)`: never while riding something, then
/// `LivingEntity.randomTeleport(x, y, z, true, #enderman_does_not_teleport_to)`:
/// the first `#entities_can_teleport_to` block below the spot is the ground,
/// and the enderman lands at the spot's own height less the blocks it went
/// down (`checkPositionAndTeleport`), unless the ground or a block in its new
/// box is one it does not teleport to, the box collides or holds a liquid.
pub fn teleport_to(
    m: &mut Mob,
    w: &MobWorld,
    x: f64,
    y: f64,
    z: f64,
    events: &mut Vec<MobEvent>,
) -> bool {
    if m.extra.passenger {
        return false;
    }
    // WorldBorder.clampVec3ToBound keeps the target inside the border
    // (Mth.clamp, 1.0E-5F inside the far edges).
    let (x, z) = match w.border {
        Some(border) => {
            let [min_x, min_z, max_x, max_z] = border.bounds(0.0);
            let clamp = |v: f64, low: f64, high: f64| if v < low { low } else { v.min(high) };
            (
                clamp(x, min_x, max_x - f64::from(1.0e-5_f32)),
                clamp(z, min_z, max_z - f64::from(1.0e-5_f32)),
            )
        }
        None => (x, z),
    };
    let (bx, bz) = (x.floor() as i32, z.floor() as i32);
    if !w.world.loaded(bx, bz) {
        return false;
    }
    let refused = |name: &str| w.has_tag("minecraft:enderman_does_not_teleport_to", name);
    let mut by = y.floor() as i32;
    let mut ny = y;
    let ground = loop {
        if by <= w.world.bottom_y() {
            return false;
        }
        by -= 1;
        let name = w.block_name(bx, by, bz);
        if w.has_tag("minecraft:entities_can_teleport_to", name) {
            break name;
        }
        ny -= 1.0;
    };
    if refused(ground) {
        return false;
    }
    let half = f64::from(m.width()) / 2.0;
    let b = Aabb::new(
        Vec3::new(x - half, ny, z - half),
        Vec3::new(x + half, ny + f64::from(m.height()), z + half),
    );
    if !w.no_collision(b) {
        return false;
    }
    // LevelReader.containsAnyLiquid and findBlocksIn: the same closed range of
    // cells, from BlockPos.containing(min) to BlockPos.containing(max).
    let (lo, hi) = (b.min_position(), b.max_position());
    for cx in lo.x.floor() as i32..=hi.x.floor() as i32 {
        for cy in lo.y.floor() as i32..=hi.y.floor() as i32 {
            for cz in lo.z.floor() as i32..=hi.z.floor() as i32 {
                if w.fluid(cx, cy, cz) != Fluid::None || refused(w.block_name(cx, cy, cz)) {
                    return false;
                }
            }
        }
    }
    // Enderman.canRandomlyTeleportTo: not onto ground that holds water
    // (a waterlogged slab, leaves or roots).
    if w.fluid(bx, by, bz) == Fluid::Water {
        return false;
    }
    let old = m.position;
    let old_block = [
        m.position.x.floor() as i32,
        m.position.y.floor() as i32,
        m.position.z.floor() as i32,
    ];
    // Entity.teleportTo -> snapTo: the old position moves too, the motion stays.
    m.position = Vec3::new(x, ny, z);
    m.previous = m.position;
    m.ai.nav.stop();
    // `randomTeleport(..., true)` broadcasts entity event 46.
    events.push(MobEvent::EntityEvent {
        mob: m.id,
        event: 46,
        at: [m.position.x, m.position.y, m.position.z],
    });
    // Enderman.teleport: GameEvent.TELEPORT where it was, silent or not.
    events.push(MobEvent::GameEvent {
        event: "minecraft:teleport",
        at: [old.x, old.y, old.z],
        source: m.id,
        state: horizon_world::registry::BlockStateId(0),
    });
    if !m.silent {
        // Level event 2018: the portal trail, from the old block to the new
        // one, packed as `BlockUtil.clampedPackDifferenceInPosition` does.
        let new_block = [x.floor() as i32, ny.floor() as i32, z.floor() as i32];
        let packed = |k: usize| ((new_block[k] - old_block[k]).clamp(-127, 127) + 127) & 255;
        events.push(MobEvent::LevelEvent {
            id: 2018,
            pos: old_block,
            data: packed(0) << 16 | packed(1) << 8 | packed(2),
        });
        // Both sounds are at the new feet: `xo` was moved by snapTo, and
        // Entity.playSound plays at the position.
        let at = [m.position.x, m.position.y, m.position.z];
        events.push(MobEvent::Sound("entity.enderman.teleport", at, 1.0, 1.0));
        events.push(MobEvent::Sound("entity.enderman.teleport", at, 1.0, 1.0));
    }
    true
}
pub fn teleport_random(m: &mut Mob, w: &MobWorld, events: &mut Vec<MobEvent>) -> bool {
    if !m.alive() {
        return false;
    }
    let x = m.position.x + (m.random.next_double() - 0.5) * 64.0;
    let y = m.position.y + f64::from(m.random.next_int_bound(64).unwrap_or(32) - 32);
    let z = m.position.z + (m.random.next_double() - 0.5) * 64.0;
    teleport_to(m, w, x, y, z, events)
}
fn teleport_towards(
    m: &mut Mob,
    w: &MobWorld,
    at: Vec3,
    eye_y: f64,
    events: &mut Vec<MobEvent>,
) -> bool {
    let mut d = [
        m.position.x - at.x,
        m.position.y + f64::from(m.height()) * 0.5 - eye_y,
        m.position.z - at.z,
    ];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1.0e-9);
    d = d.map(|c| c / len);
    let x = m.position.x + (m.random.next_double() - 0.5) * 8.0 - d[0] * 16.0;
    let y = m.position.y + f64::from(m.random.next_int_bound(16).unwrap_or(8) - 8) - d[1] * 16.0;
    let z = m.position.z + (m.random.next_double() - 0.5) * 8.0 - d[2] * 16.0;
    teleport_to(m, w, x, y, z, events)
}
/// `Enderman.SPEED_MODIFIER_ATTACKING`, an ADD_VALUE modifier `setTarget`
/// adds while the enderman has a target.
pub fn attacking_speed(m: &Mob) -> f64 {
    if m.species() == Species::Enderman && m.ai.target.is_some() {
        f64::from(0.15f32)
    } else {
        0.0
    }
}
/// Enderman.customServerAiStep: in daylight, flee by teleporting. The End has a
/// fixed time, so `Level.isBrightOutside` is never true there.
pub fn custom_server_ai_step(m: &mut Mob, cx: &mut Cx) {
    if cx.w.daylight && !cx.w.dimension_type.has_fixed_time && m.age as i32 >= m.extra.counter + 600
    {
        let br = super::hostile::mob_brightness(m, cx.w);
        let (x, y, z) = (
            m.position.x.floor() as i32,
            m.position.y.floor() as i32,
            m.position.z.floor() as i32,
        );
        if br > 0.5 && cx.w.can_see_sky(x, y, z) && m.random.next_float() * 30.0 < (br - 0.4) * 2.0
        {
            m.ai.target = None;
            teleport_random(m, cx.w, cx.events);
        }
    }
    // Being angry (target set) makes the head "creepy" (render).
    m.extra.flag = m.ai.target.is_some();
}
/// Whether a hit teleports the enderman away instead of hurting it: any
/// projectile, unless it rides something (`!isPassenger()`).
pub fn dodges(m: &Mob, source: &Damage) -> bool {
    source.is_projectile() && !m.extra.passenger
}
/// Enderman.hurtServer: projectiles miss (teleport away), others may teleport.
pub fn on_damage(m: &mut Mob, source: &Damage) -> bool {
    if dodges(m, source) {
        // repeatedlyTryToTeleport is applied by the next AI step.
        m.extra.counter2 = 64;
        return false;
    }
    true
}
pub fn after_hurt(m: &mut Mob, source: &Damage) {
    // A thrown potion's hit (`indirect_magic` with the potion as the direct
    // entity, `Mob::damage_by`) and a projectile it dodges leave
    // `Enderman.hurtServer` before the random chance:
    // `repeatedlyTryToTeleport` replaces it.
    if !dodges(m, source) && source.kind != super::DamageKind::IndirectMagic {
        teleport_chance(m, source);
    }
}
/// `Enderman.hurtServer`'s `!(source.getEntity() instanceof LivingEntity) &&
/// random.nextInt(10) != 0`: a teleport after the hurt.
pub fn teleport_chance(m: &mut Mob, source: &Damage) {
    if source.attacker.is_none() && m.random.next_int_bound(10).unwrap_or(0) != 0 {
        m.extra.counter2 = 1;
    }
}
/// Enderman.aiStep: never jumps; the teleports a hurt left run.
pub fn ai_step(m: &mut Mob, w: &MobWorld, events: &mut Vec<MobEvent>) {
    m.ai.jumping = false;
    run_pending_teleports(m, w, events);
}
/// The teleports `hurtServer` asked for (`teleport()`, or
/// `repeatedlyTryToTeleport`: up to 64 tries).
pub fn run_pending_teleports(m: &mut Mob, w: &MobWorld, events: &mut Vec<MobEvent>) {
    if m.extra.counter2 > 0 {
        let tries = m.extra.counter2;
        m.extra.counter2 = 0;
        // repeatedlyTryToTeleport (64 tries) returns before any draw while riding;
        // a single `teleport()` still draws its target first.
        let tries = if tries > 1 && m.extra.passenger {
            0
        } else {
            tries
        };
        for _ in 0..tries {
            if teleport_random(m, w, events) {
                break;
            }
        }
    }
}
/// EndermanModel.setupAnim: half-speed clamped limbs, held-block arms, creepy head.
/// Humanoid riding pose runs first (`isPassenger`); only xRot is halved/clamped after.
pub fn anim(m: &Mob, s: &State, p: &mut Poses) {
    super::anim::humanoid_pub(p, s, m);
    for part in ["right_arm", "left_arm", "right_leg", "left_leg"] {
        let x = p.get(part)[3] * 0.5;
        p.xrot(part, x.clamp(-0.4, 0.4));
    }
    if m.extra.carried.is_some() {
        p.xrot("right_arm", -0.5);
        p.xrot("left_arm", -0.5);
        p.zrot("right_arm", 0.05);
        p.zrot("left_arm", -0.05);
    }
    if m.extra.flag {
        p.get("head")[1] -= 5.0;
        p.get("hat")[1] += 5.0;
    }
}
/// EndermanFreezeWhenLookedAt
#[derive(Clone, Debug)]
struct FreezeWhenLookedAt;
impl Goal for FreezeWhenLookedAt {
    goal_clone!();
    fn flags(&self) -> u8 {
        JUMP | MOVE
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if m.ai.target != Some(Who::Player) {
            return false;
        }
        let Some(p) = &cx.w.player else { return false };
        m.distance_sqr(p.position) <= 256.0 && stared_at(m, cx.w)
    }
    fn start(&mut self, m: &mut Mob, _: &mut Cx) {
        m.nav_stop();
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        if let Some(p) = &cx.w.player {
            let at = [p.position.x, p.eye[1], p.position.z];
            m.look_at(at);
        }
    }
}
/// EndermanLookForPlayerGoal: anger after being stared at for 5 ticks,
/// teleport away when stared at close up, or towards a far target.
#[derive(Clone, Debug)]
struct LookForPlayer {
    pending: bool,
    aggro: i32,
    teleport: i32,
}
impl Goal for LookForPlayer {
    goal_clone!();
    fn flags(&self) -> u8 {
        TARGET
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        let Some(p) = &cx.w.player else { return false };
        self.pending = p.alive
            && !p.creative
            && m.distance_sqr(p.position) <= m.attrs.follow_range.powi(2)
            && stared_at(m, cx.w);
        self.pending
    }
    fn start(&mut self, m: &mut Mob, _: &mut Cx) {
        self.aggro = reduced(5);
        self.teleport = 0;
        // setBeingStaredAt: play the stare sound.
        m.extra.counter = m.age as i32;
    }
    fn stop(&mut self, m: &mut Mob, _: &mut Cx) {
        self.pending = false;
        m.ai.target = None;
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        let Some(p) = cx.w.player.clone() else {
            return false;
        };
        if self.pending {
            if !stared_at(m, cx.w) {
                return false;
            }
            m.turn_to(p.eye, 10.0, 10.0);
            return true;
        }
        p.alive
            && !p.creative
            && m.ai.target == Some(Who::Player)
            && m.distance_sqr(p.position) <= m.attrs.follow_range.powi(2)
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let Some(p) = cx.w.player.clone() else { return };
        if self.pending {
            self.aggro -= 1;
            if self.aggro <= 0 {
                self.pending = false;
                m.ai.target = Some(Who::Player);
                cx.events.push(MobEvent::Sound(
                    "entity.enderman.stare",
                    m.center(),
                    2.5,
                    1.0,
                ));
            }
        } else if m.ai.target == Some(Who::Player) && !m.extra.passenger {
            if stared_at(m, cx.w) {
                if m.distance_sqr(p.position) < 16.0 {
                    teleport_random(m, cx.w, cx.events);
                }
                self.teleport = 0;
            } else {
                let far = m.distance_sqr(p.position) > 256.0;
                let t = self.teleport;
                if far {
                    self.teleport += 1;
                }
                if far
                    && t >= reduced(30)
                    && teleport_towards(m, cx.w, p.position, p.eye[1], cx.events)
                {
                    self.teleport = 0;
                }
            }
        }
    }
}
/// EndermanTakeBlockGoal
#[derive(Clone, Debug)]
struct TakeBlock;
impl Goal for TakeBlock {
    goal_clone!();
    fn flags(&self) -> u8 {
        0
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        // EndermanTakeBlockGoal.canUse: needs mobGriefing, checked before the draw.
        m.extra.carried.is_none()
            && cx.w.mob_griefing
            && m.random.next_int_bound(reduced(20)).unwrap_or(1) == 0
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let x = (m.position.x - 2.0 + m.random.next_double() * 4.0).floor() as i32;
        let y = (m.position.y + m.random.next_double() * 3.0).floor() as i32;
        let z = (m.position.z - 2.0 + m.random.next_double() * 4.0).floor() as i32;
        let state = cx.w.world.block(x, y, z);
        let name = cx.w.facts.get(state).name.to_string();
        if !cx.w.has_tag("minecraft:enderman_holdable", &name) {
            return;
        }
        // Reachable: the clip from the mob's column hits this block first.
        let from = Vec3::new(
            m.position.x.floor() + 0.5,
            f64::from(y) + 0.5,
            m.position.z.floor() + 0.5,
        );
        let to = Vec3::new(f64::from(x) + 0.5, f64::from(y) + 0.5, f64::from(z) + 0.5);
        let dx = (x - m.position.x.floor() as i32).abs() + (z - m.position.z.floor() as i32).abs();
        if dx > 0
            && !cx.w.clear_between(
                from,
                Vec3::new(
                    to.x - (to.x - from.x) * 0.3,
                    to.y,
                    to.z - (to.z - from.z) * 0.3,
                ),
                false,
            )
        {
            return;
        }
        cx.events.push(MobEvent::SetBlock {
            pos: [x, y, z],
            state: cx.w.world.air,
        });
        // level.gameEvent(BLOCK_DESTROY, pos, Context.of(enderman, blockState)).
        cx.events.push(MobEvent::GameEvent {
            event: "minecraft:block_destroy",
            at: [f64::from(x) + 0.5, f64::from(y) + 0.5, f64::from(z) + 0.5],
            source: m.id,
            state,
        });
        m.extra.carried = Some(state);
    }
}
/// EndermanLeaveBlockGoal
#[derive(Clone, Debug)]
struct LeaveBlock;
impl Goal for LeaveBlock {
    goal_clone!();
    fn flags(&self) -> u8 {
        0
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        // EndermanLeaveBlockGoal.canUse: needs mobGriefing, checked before the draw.
        m.extra.carried.is_some()
            && cx.w.mob_griefing
            && m.random.next_int_bound(reduced(2000)).unwrap_or(1) == 0
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let Some(carried) = m.extra.carried else {
            return;
        };
        let x = (m.position.x - 1.0 + m.random.next_double() * 2.0).floor() as i32;
        let y = (m.position.y + m.random.next_double() * 2.0).floor() as i32;
        let z = (m.position.z - 1.0 + m.random.next_double() * 2.0).floor() as i32;
        let area = Aabb::new(
            Vec3::new(f64::from(x), f64::from(y), f64::from(z)),
            Vec3::new(f64::from(x + 1), f64::from(y + 1), f64::from(z + 1)),
        );
        // Keep the per-mob occupancy view: later mobs have not moved yet.
        let occupants =
            cx.w.others_near(area)
                .filter(|s| s.id != m.id && !s.removed)
                .map(|s| s.bounds)
                .collect();
        // Game owns the mutable level, its scheduled ticks, RNG and shape hooks.
        // Defer the complete attempt, including failed candidates' shape updates.
        cx.events.push(MobEvent::EndermanPlaceBlock {
            mob: m.id,
            pos: [x, y, z],
            carried,
            occupants,
        });
    }
}

/// The tool `Enderman.dropCustomDeathLoot` rolls the carried block's drops
/// with: a diamond axe enchanted by `VanillaEnchantmentProviders.ENDERMAN_LOOT_DROP`
/// (`enchantment_provider/enderman_loot_drop.json`, a single Silk Touch I),
/// so the block itself comes back.
pub fn carried_block_tool() -> ItemStack {
    let mut axe = ItemStack::new("minecraft:diamond_axe", 1);
    axe.enchant("minecraft:silk_touch", 1);
    axe
}

/// `Enderman.dropCustomDeathLoot`: `BlockState.getDrops` of the carried
/// state with [`carried_block_tool`] and the enderman as `this` entity,
/// drawing from the block table's random sequence (`sequences`: the world's
/// sequences and seed), else `random` (the level random).
pub fn carried_block_drops(
    items: &Items,
    registry: &BlockRegistry,
    state: BlockStateId,
    random: &mut impl RandomSource,
    sequences: Option<(&mut RandomSequences, i64)>,
) -> Vec<ItemStack> {
    let tool = carried_block_tool();
    items
        .block_drops_exploded(
            registry,
            state,
            Some(&tool),
            &[],
            random,
            sequences,
            None,
            true,
        )
        .into_iter()
        .filter(|s| s.count > 0 && &*s.item != "minecraft:air")
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mobs::DamageKind;
    use std::sync::Arc;

    fn enderman() -> Mob {
        let mut kind = (*crate::mobs::sensor_scan_tests::kind()).clone();
        kind.species = Species::Enderman;
        Mob::new(1, Arc::new(kind), Vec3::ZERO, 0.0)
    }

    /// Enderman.hurtServer: a projectile makes a free enderman teleport away
    /// unhurt, but one that rides something (`isPassenger`) takes the hit.
    #[test]
    fn projectiles_hit_a_seated_enderman() {
        for kind in [DamageKind::Arrow, DamageKind::Thrown, DamageKind::Fireball] {
            let mut free = enderman();
            assert!(!on_damage(&mut free, &Damage::of(kind)), "{kind:?}");
            assert_eq!(free.extra.counter2, 64);
            // The arrow handler keeps the arrow flying when the enderman dodges.
            assert!(dodges(&free, &Damage::of(kind)), "{kind:?}");

            let mut seated = enderman();
            seated.extra.passenger = true;
            assert!(on_damage(&mut seated, &Damage::of(kind)), "{kind:?}");
            assert_eq!(seated.extra.counter2, 0, "no teleport while seated");
            assert!(!dodges(&seated, &Damage::of(kind)), "{kind:?}");
        }
    }

    /// Enderman.teleport(x, y, z) fails for a passenger (#4865: one in a boat
    /// played the teleport sound every tick and snapped back to its seat).
    /// `repeatedlyTryToTeleport` returns before drawing; `teleport()` still
    /// draws its three values first.
    #[test]
    fn a_seated_enderman_never_teleports() {
        let build = |world: &mut crate::world::World, registry: &BlockRegistry| {
            let stone = registry.default_state("minecraft:stone").unwrap();
            for cx in -2..2 {
                for cz in -2..2 {
                    world
                        .chunks
                        .insert((cx, cz), Arc::new(crate::chunk::Chunk::empty(cx, cz)));
                }
            }
            for x in -32..32 {
                for z in -32..32 {
                    world.set(x, -1, z, stone).unwrap();
                }
            }
        };
        crate::mobs::world::with_built_test_world(&[], build, |w, _| {
            let mut events = Vec::new();
            let mut free = enderman();
            assert!((0..64).any(|_| teleport_random(&mut free, w, &mut events)));
            assert!(
                events
                    .iter()
                    .any(|e| matches!(e, MobEvent::EntityEvent { event: 46, .. })),
                "entity event 46: {events:?}"
            );

            let mut seated = enderman();
            seated.extra.passenger = true;
            events.clear();
            assert!(!(0..64).any(|_| teleport_random(&mut seated, w, &mut events)));
            assert!(events.is_empty());
            assert_eq!(seated.position, Vec3::ZERO);

            for tries in [64, 1] {
                let mut seated = enderman();
                seated.extra.passenger = true;
                let mut expected = enderman().random;
                if tries == 1 {
                    expected.next_double();
                    expected.next_int_bound(64).unwrap();
                    expected.next_double();
                }
                seated.extra.counter2 = tries;
                ai_step(&mut seated, w, &mut events);
                assert!(events.is_empty(), "{tries}");
                assert_eq!(seated.random.next_int(), expected.next_int(), "{tries}");
            }
        });
    }

    /// LivingEntity.randomTeleport clamps the target into the world border
    /// (`WorldBorder.clampVec3ToBound`), so an enderman never lands outside
    /// it (#7080). A border of size 10 around the origin ends at x = 5.
    #[test]
    fn an_enderman_teleports_inside_the_world_border() {
        let build = |world: &mut crate::world::World, registry: &BlockRegistry| {
            let stone = registry.default_state("minecraft:stone").unwrap();
            for cx in -2..2 {
                for cz in -2..2 {
                    world
                        .chunks
                        .insert((cx, cz), Arc::new(crate::chunk::Chunk::empty(cx, cz)));
                }
            }
            for x in -32..32 {
                for z in -32..32 {
                    world.set(x, -1, z, stone).unwrap();
                }
            }
        };
        let border =
            horizon_core::geometry::border::BorderGeometry::new([0.0, 0.0], 10.0, 29_999_984)
                .unwrap();
        crate::mobs::world::with_built_test_world(&[], build, |w, _| {
            let mut free = enderman();
            assert!(teleport_to(&mut free, w, 20.5, 0.0, 0.5, &mut Vec::new()));
            assert_eq!(free.position, Vec3::new(20.5, 0.0, 0.5), "no border");

            let w = MobWorld {
                border: Some(&border),
                ..w.clone()
            };
            let mut bounded = enderman();
            assert!(teleport_to(
                &mut bounded,
                &w,
                20.5,
                0.0,
                -20.5,
                &mut Vec::new()
            ));
            // Mth.clamp to [min, max - 1.0E-5F] on x and z.
            let edge = 5.0 - f64::from(1.0e-5_f32);
            assert_eq!(bounded.position, Vec3::new(edge, 0.0, -5.0));
        });
    }

    /// Enderman.dropCustomDeathLoot (#3513): a killed enderman drops the
    /// block it carries, mined with a silk touch diamond axe, so a grass
    /// block comes back as a grass block (not dirt) and a nylium as itself.
    #[test]
    fn a_killed_enderman_drops_its_carried_block() {
        use crate::mobs::drops;
        use horizon_world::loot::{LootData, PackOverlay};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() || !root.join("data").exists() {
            return;
        }
        let assets = crate::assets::Assets::load(&root).unwrap();
        let registry = assets.registry;
        let facts = crate::blocks::BlockFacts::load(&root, registry).unwrap();
        let items = Items::load(&root, registry, &facts, &PackOverlay::NONE).unwrap();
        let loot = LootData::load(&root, &PackOverlay::NONE).unwrap();
        let death = |mob_drops| drops::Death {
            loot: &loot,
            damage: None,
            player: None,
            player_killed: false,
            luck: 0.0,
            vehicle: None,
            mob_drops,
            world_seed: 0,
        };
        let mut sequences = RandomSequences::default();
        let mut random = horizon_core::random::LegacyRandom::new(0);
        for block in [
            "minecraft:grass_block",
            "minecraft:crimson_nylium",
            "minecraft:tnt",
        ] {
            let mut m = enderman();
            m.extra.carried = registry.default_state(block);
            let (out, _) = drops::die(
                &mut m,
                &death(true),
                &mut sequences,
                &mut horizon_core::random::LegacyRandom::new(0),
            );
            let dropped: Vec<String> = out
                .into_iter()
                .flat_map(|d| match d {
                    drops::Drop::CarriedBlock(state) => carried_block_drops(
                        &items,
                        registry,
                        state,
                        &mut random,
                        Some((&mut sequences, 0)),
                    ),
                    _ => Vec::new(),
                })
                .map(|s| format!("{} {}", s.item, s.count))
                .collect();
            assert_eq!(dropped, [format!("{block} 1")], "{block}");
        }
        // doMobLoot off: no dropCustomDeathLoot, so no block either.
        let mut m = enderman();
        m.extra.carried = registry.default_state("minecraft:grass_block");
        let (out, _) = drops::die(
            &mut m,
            &death(false),
            &mut sequences,
            &mut horizon_core::random::LegacyRandom::new(0),
        );
        assert!(
            !out.iter()
                .any(|d| matches!(d, drops::Drop::CarriedBlock(_))),
            "{out:?}"
        );
    }
}
