//! Nautili and zombie nautili (`AbstractNautilus`, `Nautilus`,
//! `ZombieNautilus`, `NautilusAi`, `ZombieNautilusAi`): smooth swimming,
//! drying out on land, taming with pufferfish, the saddle and armour, the
//! ridden swim with its dash, charging at pufferfish and at whoever hurt
//! them, the zombie's temperate and coral variants, and `NautilusModel`.
use super::{
    Mob, MobEvent, MobWorld, Species, Who,
    anim::{Poses, State},
    control::{LookKind, MoveKind, Operation},
    goals::{Breed, Cx, Goal, LOOK, LookAtPlayer, MOVE, Panic, Stroll, Tempt, goal_clone},
    keyframes::Animation,
    nav::NavKind,
};
use crate::blocks::Fluid;
use horizon_core::{geometry::Vec3, random::RandomSource};
use std::collections::HashMap;

// Nautilus state: `tame`/`owner`, the saddle, `head` the body armour,
// `variant` the zombie's variant (0 temperate, 1 warm), `anim_ticks` the
// dash cooldown and `flag2` dashing, `counter` the attack target cooldown,
// `counter2` the charge cooldown, `anger` the ticks left of `ANGRY_AT`
// (at `ai.last_hurt_by`), `ints[1]` the rider's breath effect given,
// `scratch[1]` a dash whose sound is still to play.
const DASH_COOLDOWN: i32 = 40;
const TIME_BETWEEN_ATTACKS: i32 = 80;
const ANGER_DURATION: i32 = 400;
const ATTACK_TARGET_COOLDOWN: &str = "minecraft:attack_target_cooldown";
/// `ZombieNautilusVariants`: the variant keys in index order.
pub const VARIANTS: [&str; 2] = ["temperate", "warm"];

pub fn is_nautilus(s: Species) -> bool {
    matches!(s, Species::Nautilus | Species::ZombieNautilus)
}
fn zombie(m: &Mob) -> bool {
    m.species() == Species::ZombieNautilus
}
pub fn register(m: &mut Mob) {
    // SmoothSwimmingMoveControl(85, 10, 0.011, 0, true), SmoothSwimmingLookControl(10).
    m.ai.mov.kind = MoveKind::SmoothSwimming {
        max_turn_x: 85.0,
        max_turn_y: 10.0,
        in_water: 0.011,
        outside: 0.0,
        gravity: true,
    };
    m.ai.look.kind = LookKind::SmoothSwimming(10.0);
    m.ai.nav.kind = NavKind::Water { breach: false };
    m.set_malus(super::path::PathType::Water, 0.0);
    let tempt = m.kind.tempt.clone();
    let stop = if m.is_baby() { 2.5 } else { 3.5 };
    let is_zombie = zombie(m);
    let g = &mut m.ai.goals;
    if is_zombie {
        // ZombieNautilusAi: no panic, no love, slower temptation and charge.
        let mut t = Tempt::new(0.9, tempt, false);
        t.stop_distance = stop;
        g.add(1, t);
        g.add(2, Charge::new(0.5));
        g.add(3, Stroll::swimming(1.0, 1));
        g.add(3, LookAtPlayer::new(6.0));
    } else {
        g.add(0, Panic::new(1.6));
        g.add(1, Breed::new(0.4));
        let mut t = Tempt::new(1.3, tempt, false);
        t.stop_distance = stop;
        g.add(2, t);
        g.add(3, Charge::new(0.6));
        g.add(4, Stroll::swimming(1.0, 1));
        g.add(4, LookAtPlayer::new(6.0));
    }
}
/// `finalizeSpawn`: `NautilusAi.initMemories` (2400 to 3600 ticks before it
/// first looks for prey) and the zombie's variant by biome.
pub fn finalize(m: &mut Mob, warm_ocean: bool) {
    m.extra.counter = 2400 + m.random.next_int_bound(1201).unwrap_or(0);
    m.extra.nautilus_attack_cooldown_ttl = Some(i64::MAX);
    if zombie(m) {
        m.extra.variant = u8::from(warm_ocean);
    }
}
/// `getControllingPassenger`: a player on a saddled nautilus.
pub fn controlled(m: &Mob) -> bool {
    m.rider.is_some() && m.extra.saddled
}
/// `AbstractNautilus.isFood`: taming items while wild, food otherwise.
pub fn is_food(m: &Mob, item: &str, taming: &[String]) -> bool {
    if !m.extra.tame && !m.is_baby() {
        taming.iter().any(|t| t == item)
    } else {
        m.kind.food.iter().any(|t| t == item)
    }
}
/// `TamableAnimal.feed`'s heal: twice the food's nutrition, or 1 for an item
/// without a food component (a fish bucket).
pub fn feed_heal(nutrition: Option<i32>) -> f32 {
    nutrition.map_or(1.0, |n| 2.0 * n as f32)
}
/// The ambient, hurt and death sounds by age and whether it is under water.
pub fn sounds(m: &Mob) -> [&'static str; 3] {
    let under = m.fluids.eye_in_water;
    match (zombie(m), m.is_baby(), under) {
        (true, _, true) => [
            "entity.zombie_nautilus.ambient",
            "entity.zombie_nautilus.hurt",
            "entity.zombie_nautilus.death",
        ],
        (true, _, false) => [
            "entity.zombie_nautilus.ambient_land",
            "entity.zombie_nautilus.hurt_land",
            "entity.zombie_nautilus.death_land",
        ],
        (false, true, true) => [
            "entity.baby_nautilus.ambient",
            "entity.baby_nautilus.hurt",
            "entity.baby_nautilus.death",
        ],
        (false, true, false) => [
            "entity.baby_nautilus.ambient_land",
            "entity.baby_nautilus.hurt_land",
            "entity.baby_nautilus.death_land",
        ],
        (false, false, true) => [
            "entity.nautilus.ambient",
            "entity.nautilus.hurt",
            "entity.nautilus.death",
        ],
        (false, false, false) => [
            "entity.nautilus.ambient_land",
            "entity.nautilus.hurt_land",
            "entity.nautilus.death_land",
        ],
    }
}
fn dash_sound(m: &Mob) -> &'static str {
    match (zombie(m), m.fluids.eye_in_water) {
        (true, true) => "entity.zombie_nautilus.dash",
        (true, false) => "entity.zombie_nautilus.dash_land",
        (false, true) => "entity.nautilus.dash",
        (false, false) => "entity.nautilus.dash_land",
    }
}
fn dash_ready_sound(m: &Mob) -> &'static str {
    match (zombie(m), m.fluids.eye_in_water) {
        (true, true) => "entity.zombie_nautilus.dash_ready",
        (true, false) => "entity.zombie_nautilus.dash_ready_land",
        (false, true) => "entity.nautilus.dash_ready",
        (false, false) => "entity.nautilus.dash_ready_land",
    }
}
/// `playEatingSound`.
pub fn eat_sound(m: &Mob) -> &'static str {
    match (zombie(m), m.is_baby()) {
        (true, _) => "entity.zombie_nautilus.eat",
        (false, true) => "entity.baby_nautilus.eat",
        (false, false) => "entity.nautilus.eat",
    }
}
/// `getEquipSound(SADDLE)`.
pub fn saddle_sound(m: &Mob) -> &'static str {
    if m.fluids.eye_in_water {
        "item.nautilus_saddle_underwater_equip"
    } else {
        "item.nautilus_saddle_equip"
    }
}
fn sound(m: &Mob, name: &'static str, events: &mut Vec<MobEvent>) {
    events.push(MobEvent::Sound(name, m.center(), 1.0, 1.0));
}
/// `AbstractNautilus.travelInWater`: its own speed, no fluid drag but 0.9.
pub fn travel(m: &mut Mob, w: &MobWorld, input: Vec3, events: &mut Vec<MobEvent>) -> bool {
    if !m.fluids.in_water {
        return false;
    }
    let speed = m.ai.speed;
    m.move_relative(speed, input);
    m.do_move(w, events);
    m.velocity = Vec3::new(m.velocity.x * 0.9, m.velocity.y * 0.9, m.velocity.z * 0.9);
    true
}
/// `tickRidden` and `getRiddenInput`: turn half way to the rider, swim along
/// their look, faster in water (`getRiddenSpeed`), and dash on the jump.
pub fn ridden(m: &mut Mob) {
    let Some(r) = m.rider else { return };
    let diff = super::living::wrap(r.yaw - m.yaw);
    m.yaw += diff * 0.5;
    m.head_pitch = r.pitch * 0.5;
    m.body_yaw = m.yaw;
    m.head_yaw = m.yaw;
    m.ai.nav.stop();
    m.ai.mov.operation = Operation::Wait;
    let (mut forward, mut up) = (0.0, 0.0);
    if r.forward != 0.0 {
        let mut f = r.pitch.to_radians().cos();
        let mut u = -r.pitch.to_radians().sin();
        if r.forward < 0.0 {
            f *= -0.5;
            u *= -0.5;
        }
        forward = f;
        up = u;
    }
    let factor = if m.fluids.in_water { 0.0325 } else { 0.02 };
    m.ai.speed = factor * m.movement_speed() as f32;
    m.ai.xxa = r.strafe;
    m.ai.yya = up;
    m.ai.zza = forward;
    if r.jump > 0.0 {
        if m.extra.anim_ticks <= 0 && !m.extra.flag2 {
            dash(m, r.jump);
        }
        if let Some(rider) = m.rider.as_mut() {
            rider.jump = 0.0;
        }
    }
}
/// `AbstractNautilus.onPlayerJump` and the momentum `handleStartJump` starts:
/// a saddled nautilus that is not in its dash cooldown keeps the released
/// jump bar's scale on the rider, and its next `tickRidden` dashes with it
/// (`executeRidersJump`). Returns whether the jump was taken.
pub fn on_player_jump(m: &mut Mob, data: i32) -> bool {
    // `if (isSaddled() && dashCooldown <= 0) playerJumpPendingScale = ...`
    if !m.extra.saddled || m.extra.anim_ticks > 0 {
        return false;
    }
    let Some(rider) = m.rider.as_mut() else {
        return false;
    };
    rider.jump = super::equine::jump_pending_scale(data);
    true
}
/// `executeRidersJump` (with `handleStartJump`'s sound): along the rider's
/// look, 1.2 in water and 0.5 on land.
fn dash(m: &mut Mob, amount: f32) {
    let Some(r) = m.rider else { return };
    let (yaw, pitch) = (r.yaw.to_radians(), r.pitch.to_radians());
    let look = Vec3::new(
        f64::from(-yaw.sin() * pitch.cos()),
        f64::from(-pitch.sin()),
        f64::from(yaw.cos() * pitch.cos()),
    );
    let power = if m.fluids.in_water { 1.2 } else { 0.5 };
    let k = f64::from(power * amount) * m.movement_speed();
    m.velocity = Vec3::new(
        m.velocity.x + look.x * k,
        m.velocity.y + look.y * k,
        m.velocity.z + look.z * k,
    );
    m.extra.anim_ticks = DASH_COOLDOWN;
    m.extra.flag2 = true;
    // The sound plays on its next tick.
    m.extra.scratch[1] = 1;
}
/// `AbstractNautilus.getNautilusRestrictionRadius`: 32 blocks, or 16 for a
/// baby or a saddled one.
fn restriction_radius(m: &Mob) -> i32 {
    if !m.is_baby() && !m.extra.saddled {
        32
    } else {
        16
    }
}
/// `AbstractNautilus.checkRestriction`: a tame nautilus that is neither
/// leashed nor ridden keeps its home where it is, within its radius.
fn check_restriction(m: &mut Mob) {
    if m.extra.leash.is_some() || m.rider.is_some() || !m.extra.tame {
        return;
    }
    let radius = restriction_radius(m);
    let at = [
        m.position.x.floor() as i32,
        m.position.y.floor() as i32,
        m.position.z.floor() as i32,
    ];
    let kept = m.ai.home.is_some_and(|(c, r)| {
        let d = [c[0] - at[0], c[1] - at[1], c[2] - at[2]];
        let sq = f64::from(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
        sq < f64::from(radius + 8).powi(2) && r == radius
    });
    if !kept {
        m.ai.home = Some((at, radius));
    }
}
/// `AbstractNautilus.tick` and the brains' memories: the rider's breath, the
/// dash clock, bubbles, and choosing whom to charge at
/// (`NautilusAi.findNearestValidAttackTarget`).
pub fn ai_step(
    m: &mut Mob,
    w: &MobWorld,
    level_random: &mut horizon_core::random::LegacyRandom,
    events: &mut Vec<MobEvent>,
) {
    // customServerAiStep: the home is kept before the rest of the step.
    check_restriction(m);
    if m.extra.scratch[1] != 0 {
        m.extra.scratch[1] = 0;
        sound(m, dash_sound(m), events);
    }
    // applyEffects: the rider breathes under water.
    if m.rider.is_some() {
        if m.extra.ints[1] == 0 || w.game_time % 40 == 0 {
            m.extra.ints[1] = 1;
            events.push(MobEvent::EffectPlayer {
                effect: "minecraft:breath_of_the_nautilus",
                ticks: 60,
                amplifier: 0,
                // AbstractNautilus.applyEffects: ambient, visible, showIcon.
                ambient: true,
            });
        }
    } else {
        m.extra.ints[1] = 0;
    }
    if m.extra.flag2 && m.extra.anim_ticks < 35 {
        m.extra.flag2 = false;
    }
    if m.extra.anim_ticks > 0 {
        m.extra.anim_ticks -= 1;
        if m.extra.anim_ticks == 0 {
            sound(m, dash_ready_sound(m), events);
        }
    }
    if m.fluids.in_water {
        bubbles(m, events);
    }
    // Brain's saved slot lifetime is independent of the existing attack counter.
    tick_attack_memory(m);
    // CountDownCooldownTicks and the ANGRY_AT expiry. Keep the legacy
    // counter/target policy, including after the saved slot expires.
    if m.extra.counter > 0 {
        m.extra.counter -= 1;
        if m.extra.nautilus_attack_cooldown_ttl.is_some() {
            // CountDownCooldownTicks.tick writes with Brain.setMemory (permanent).
            m.extra.nautilus_attack_cooldown_ttl = Some(i64::MAX);
        }
    } else {
        m.extra.nautilus_attack_cooldown_ttl = None;
    }
    if m.extra.counter2 > 0 {
        m.extra.counter2 -= 1;
    }
    if m.extra.anger > 0 {
        m.extra.anger -= 1;
    }
    if m.ai.target.is_none() && m.alive() && !m.no_ai {
        m.ai.target = find_target(m, w, level_random);
    }
}
/// `MemorySlot.tick`: expire before decrementing a finite saved lifetime.
/// The existing NoAI gate controls this class step; this does not replace
/// the legacy integer attack counter or its selection policy.
fn tick_attack_memory(m: &mut Mob) {
    if let Some(ttl) = m.extra.nautilus_attack_cooldown_ttl
        && ttl != i64::MAX
    {
        m.extra.nautilus_attack_cooldown_ttl = if ttl <= 0 { None } else { Some(ttl - 1) };
    }
}
/// `spawnBubbles`: more as it swims faster, from behind its mouth.
fn bubbles(m: &mut Mob, events: &mut Vec<MobEvent>) {
    let v = m.velocity;
    let speed = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
    let chance = (speed * 2.0).clamp(f64::from(0.15f32), 1.0);
    if f64::from(m.random.next_float()) >= chance {
        return;
    }
    let (yaw, pitch) = (
        m.yaw.to_radians(),
        m.head_pitch.clamp(-10.0, 10.0).to_radians(),
    );
    let dir = [
        f64::from(-yaw.sin() * pitch.cos()),
        f64::from(-pitch.sin()),
        f64::from(yaw.cos() * pitch.cos()),
    ];
    let spread = m.random.next_double() * 0.8 * (1.0 + speed);
    let velocity = [
        (f64::from(m.random.next_float()) - 0.5) * spread,
        (f64::from(m.random.next_float()) - 0.5) * spread,
        (f64::from(m.random.next_float()) - 0.5) * spread,
    ];
    events.push(MobEvent::ParticleAt {
        kind: "bubble",
        at: [
            m.position.x - dir[0] * 1.1,
            m.position.y - dir[1] + 0.25,
            m.position.z - dir[2] * 1.1,
        ],
        velocity,
    });
}
/// `AbstractNautilus.tick` on a client of a server: the bubbles of a nautilus
/// in the water, from the motion the server sent.
pub fn client_tick(m: &mut Mob, events: &mut Vec<MobEvent>) {
    if m.fluids.in_water {
        bubbles(m, events);
    }
}
fn in_water_at(w: &MobWorld, p: Vec3) -> bool {
    w.fluid(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32) == Fluid::Water
}
/// `AbstractNautilus.isAggravated`: the brain holds an `ANGRY_AT` or an
/// `ATTACK_TARGET` memory. Horizon keeps those as the anger timer (whose
/// target is `last_hurt_by`) and the current attack target.
pub fn aggravated(m: &Mob) -> bool {
    m.extra.anger > 0 || m.ai.target.is_some()
}
/// `NautilusAi.findNearestValidAttackTarget`: whoever it is angry at, else
/// now and then (a coin flip every 2400 to 3600 ticks) the nearest pufferfish.
fn find_target(
    m: &mut Mob,
    w: &MobWorld,
    level_random: &mut horizon_core::random::LegacyRandom,
) -> Option<Who> {
    if m.extra.in_love > 0 || !m.fluids.in_water || m.is_baby() || m.extra.tame {
        return None;
    }
    if m.extra.anger > 0
        && let Some(who) = m.ai.last_hurt_by
        && let Some(t) = w.target_info(who)
        && t.alive
        && !(t.is_player && t.creative)
        && in_water_at(w, t.position)
    {
        return Some(who);
    }
    if m.extra.counter > 0 {
        return None;
    }
    // The cooldown and the coin come from the level's random (`level.getRandom()`).
    m.extra.counter = 2400 + level_random.next_int_bound(1201).unwrap_or(0);
    m.extra.nautilus_attack_cooldown_ttl = Some(i64::MAX);
    if level_random.next_float() < 0.5 {
        return None;
    }
    // NEAREST_VISIBLE_LIVING_ENTITIES.findClosest(isHostileTarget): the
    // nearest #nautilus_hostiles in water that the nautilus can see
    // (`Sensor.isEntityTargetable`: line of sight).
    let mut near: Vec<(f64, u32)> = w
        .nearby(m.position, 16.0, |s| {
            s.species == Species::Pufferfish && in_water_at(w, s.position)
        })
        .map(|s| (s.position.distance_squared(m.position), s.id))
        // TargetingConditions.range(16): feet to feet.
        .filter(|&(d, _)| d <= 16.0 * 16.0)
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    near.into_iter()
        .map(|(_, id)| Who::Mob(id))
        .find(|&who| m.sees(w, who))
}
/// `hurtServer`: `NautilusAi.setAngerTarget` for 400 ticks.
pub fn after_hurt(m: &mut Mob, source: &super::Damage) {
    if source.attacker.is_some() {
        m.extra.anger = ANGER_DURATION + 1;
    }
}
/// `ChargeAttack(80, ..., speed, 2, 12, 11, dash)`: rush straight at the
/// target and ram it once.
#[derive(Clone, Debug)]
struct Charge {
    speed: f64,
    start: Vec3,
    velocity: Vec3,
}
impl Charge {
    fn new(speed: f64) -> Self {
        Self {
            speed,
            start: Vec3::ZERO,
            velocity: Vec3::ZERO,
        }
    }
    fn still_ok(&self, m: &Mob, cx: &Cx) -> bool {
        let Some(t) = m.ai.target.and_then(|who| cx.w.target_info(who)) else {
            return false;
        };
        if !t.alive || t.is_player && t.creative || m.extra.tame || m.extra.counter2 > 0 {
            return false;
        }
        if m.position.distance_squared(self.start) >= 12.0 * 12.0
            || t.position.distance_squared(m.position) >= 11.0 * 11.0
        {
            return false;
        }
        cx.w.line_of_sight(
            Vec3::new(m.position.x, m.eye_y(), m.position.z),
            Vec3::new(t.position.x, t.eye_y, t.position.z),
        )
    }
}
impl Goal for Charge {
    goal_clone!();
    fn flags(&self) -> u8 {
        MOVE | LOOK
    }
    fn can_use(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
        m.ai.target.is_some() && m.extra.counter2 <= 0 && m.extra.in_love <= 0
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        self.still_ok(m, cx)
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        self.start = m.position;
        let Some(t) = m.ai.target.and_then(|who| cx.w.target_info(who)) else {
            return;
        };
        let d = Vec3::new(
            t.position.x - m.position.x,
            t.position.y - m.position.y,
            t.position.z - m.position.z,
        );
        let len = d.length();
        self.velocity = if len < 1.0e-4 {
            Vec3::ZERO
        } else {
            Vec3::new(
                d.x / len * self.speed,
                d.y / len * self.speed,
                d.z / len * self.speed,
            )
        };
        if self.still_ok(m, cx) {
            let name = if zombie(m) {
                "entity.zombie_nautilus.dash"
            } else {
                "entity.nautilus.dash"
            };
            sound(m, name, cx.events);
        }
    }
    fn stop(&mut self, m: &mut Mob, _: &mut Cx) {
        m.extra.counter2 = TIME_BETWEEN_ATTACKS;
        m.ai.target = None;
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let Some(who) = m.ai.target else { return };
        let Some(t) = cx.w.target_info(who) else {
            return;
        };
        m.turn_to([t.position.x, t.eye_y, t.position.z], 360.0, 360.0);
        m.velocity = self.velocity;
        if !m.bounds().intersects(t.bounds) {
            return;
        }
        // dealDamageToTarget and dealKnockBack (causeExtraKnockback along its facing).
        let from = [m.position.x, m.position.y, m.position.z];
        let amount = m.attrs.attack_damage;
        cx.events.push(match who {
            Who::Player => MobEvent::AttackPlayer(amount, from),
            Who::Mob(id) => MobEvent::AttackMob {
                target: id,
                amount,
                source: super::Damage::mob(m.id, from),
            },
        });
        let level = |e: &str| {
            m.extra
                .effects
                .iter()
                .find(|x| x.0 == e)
                .map_or(0, |x| x.2 + 1)
        };
        let boost = 0.25 * (level("minecraft:speed") - level("minecraft:slowness")) as f32;
        let factor = (self.speed as f32 * m.movement_speed() as f32).clamp(0.2, 2.0) + boost;
        let power = f64::from(factor * 2.0);
        if power > 0.0 {
            // `causeExtraKnockback`: `LivingEntity.knockback` on the target, away
            // from (sin(yaw), -cos(yaw)): its resistance, the halving of its own
            // motion, the lift on the ground and the knockback hook all apply.
            let yaw = f64::from(m.yaw * (std::f32::consts::PI / 180.0));
            cx.events.push(MobEvent::Knockback {
                target: who,
                power,
                x: f64::from(horizon_core::geometry::trig::sin(yaw)),
                z: -f64::from(horizon_core::geometry::trig::cos(yaw)),
                source: super::Damage::mob(m.id, from),
                apply_blocking: None,
            });
            m.velocity = Vec3::new(m.velocity.x * 0.6, m.velocity.y, m.velocity.z * 0.6);
        }
        m.ai.target = None;
    }
}
/// The body texture: the baby's, the adult's, or the zombie's variant.
pub fn texture(m: &Mob) -> String {
    match (zombie(m), m.is_baby(), m.extra.variant) {
        (true, _, 1) => "entity/nautilus/zombie_nautilus_coral".into(),
        (true, _, _) => "entity/nautilus/zombie_nautilus".into(),
        (false, true, _) => "entity/nautilus/nautilus_baby".into(),
        (false, false, _) => "entity/nautilus/nautilus".into(),
    }
}
/// The body model: `ZombieNautilusRenderer` picks the coral model for the
/// warm variant.
pub fn model(m: &Mob) -> &'static str {
    match (zombie(m), m.is_baby(), m.extra.variant) {
        (true, _, 1) => "zombie_nautilus_coral#main",
        (true, _, _) => "zombie_nautilus#main",
        (false, true, _) if m.kind.layer("nautilus_baby#main").is_some() => "nautilus_baby#main",
        _ => "nautilus#main",
    }
}
/// The `NAUTILUS_BODY` equipment texture of a nautilus armour item.
pub fn armor_texture(item: &str) -> Option<String> {
    let material = item
        .strip_prefix("minecraft:")?
        .strip_suffix("_nautilus_armor")?;
    let material = if material == "golden" {
        "gold"
    } else {
        material
    };
    Some(format!("entity/equipment/nautilus_body/{material}"))
}
/// `NautilusModel.setupAnim` (and `ZombieNautilusCoralModel`'s corals,
/// hidden under armour).
pub fn anim(m: &Mob, s: &State, p: &mut Poses, animations: &HashMap<String, Animation>) {
    let deg = (std::f64::consts::PI / 180.0) as f32;
    let y = s.y_rot.clamp(-10.0, 10.0);
    let x = s.x_rot.clamp(-10.0, 10.0);
    p.yrot("body", y * deg);
    p.xrot("body", x * deg);
    if let Some(a) = animations.get("NautilusAnimation.SWIMMING") {
        a.apply_walk(s.pos + s.age / 5.0, s.speed + 0.2, 2.0, 3.0, p);
    }
    if m.extra.body.is_some() {
        p.visible("corals", false);
    }
}
/// `TamableAnimal.addAdditionalSaveData`: owner and ordered Sitting, plus
/// the saddle, the zombie's variant and the brain's attack cooldown.
pub fn to_nbt(m: &Mob, map: &mut horizon_world::nbt::Compound) {
    use horizon_world::nbt::Tag;
    if let Some(owner) = m.extra.owner {
        map.insert("Owner".into(), Tag::IntArray(owner.to_vec()));
    } else {
        map.remove(&"Owner".into());
    }
    map.insert("Sitting".into(), Tag::Byte(i8::from(m.extra.sitting)));
    if zombie(m) {
        let v = VARIANTS[usize::from(m.extra.variant).min(1)];
        map.insert(
            "variant".into(),
            Tag::String(format!("minecraft:{v}").as_str().into()),
        );
    }
    let mut equipment = match map.get(&"equipment".into()) {
        Some(Tag::Compound(c)) => c.clone(),
        _ => horizon_world::nbt::Compound::new(),
    };
    match m.saddle_item() {
        Some(saddle) => {
            equipment.insert("saddle".into(), crate::save::item_to_nbt(&saddle));
        }
        None => {
            equipment.remove(&"saddle".into());
        }
    }
    map.insert("equipment".into(), Tag::Compound(equipment));
    let memory = m.extra.nautilus_attack_cooldown_ttl.map(|ttl| {
        let mut fields = vec![("value", Tag::Int(m.extra.counter))];
        if ttl != i64::MAX {
            fields.push(("ttl", Tag::Long(ttl)));
        }
        (ATTACK_TARGET_COOLDOWN, Tag::compound(fields))
    });
    map.insert(
        "Brain".into(),
        Tag::compound([("memories", Tag::compound(memory))]),
    );
}
/// `TamableAnimal.readAdditionalSaveData`: Sitting is independent of Owner.
pub fn from_nbt(m: &mut Mob, tag: &horizon_world::nbt::Tag) {
    use horizon_world::nbt::Tag;
    m.extra.owner = crate::save::uuid_from_tag(tag.get("Owner"));
    m.extra.tame = m.extra.owner.is_some();
    m.extra.sitting = crate::entity_nbt::flag(tag, "Sitting");
    if let Some(Tag::String(v)) = tag.get("variant") {
        let v = v.to_string().unwrap_or_default();
        m.extra.variant = u8::from(v.trim_start_matches("minecraft:") == "warm");
    }
    let saddle = tag
        .get("equipment")
        .and_then(|e| e.get("saddle"))
        .and_then(crate::save::item_from_nbt);
    m.set_saddle(saddle);
    let memory = tag
        .get("Brain")
        .and_then(|b| b.get("memories"))
        .and_then(|b| b.get(ATTACK_TARGET_COOLDOWN));
    if let Some(Tag::Int(value)) = memory.and_then(|m| m.get("value")) {
        m.extra.counter = *value;
        m.extra.nautilus_attack_cooldown_ttl =
            Some(crate::save::number(memory.and_then(|m| m.get("ttl"))).unwrap_or(i64::MAX));
    } else {
        m.extra.counter = 0;
        m.extra.nautilus_attack_cooldown_ttl = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mobs::world::with_test_world;
    use std::sync::Arc;

    #[test]
    fn saved_sitting_is_independent_of_owner_and_raw_nbt_for_both_species() {
        use horizon_world::nbt::Tag;
        for (id, species) in [
            ("minecraft:nautilus", Species::Nautilus),
            ("minecraft:zombie_nautilus", Species::ZombieNautilus),
        ] {
            let mut kind = (*crate::mobs::sensor_scan_tests::kind()).clone();
            kind.id = id.into();
            kind.species = species;
            let mut m = Mob::new(1, Arc::new(kind), Vec3::ZERO, 0.0);
            for owner in [None, Some([1, 2, 3, 4])] {
                for sitting in [None, Some(false), Some(true)] {
                    let mut entries = Vec::new();
                    if let Some(owner) = owner {
                        entries.push(("Owner", Tag::IntArray(owner.to_vec())));
                    }
                    if let Some(sitting) = sitting {
                        entries.push(("Sitting", Tag::Byte(i8::from(sitting))));
                    }
                    let raw = Tag::compound(entries);
                    let expected = sitting.unwrap_or(false);
                    m.extra.sitting = !expected;
                    from_nbt(&mut m, &raw);
                    assert_eq!(m.extra.owner, owner);
                    assert_eq!(m.extra.sitting, expected, "{id} {owner:?} {sitting:?}");
                    // No raw compound: this must come from the modeled flag.
                    let saved =
                        crate::entity_nbt::mob_to_nbt(&m, [5, 6, 7, 8], None, None, |_| None);
                    assert_eq!(saved.get("Sitting"), Some(&Tag::Byte(i8::from(expected))));
                    // A stale raw flag must not override a later model change.
                    m.extra.sitting = !expected;
                    let saved =
                        crate::entity_nbt::mob_to_nbt(&m, [5, 6, 7, 8], Some(&raw), None, |_| None);
                    assert_eq!(saved.get("Sitting"), Some(&Tag::Byte(i8::from(!expected))));
                }
            }
        }
    }

    /// `ChargeAttack.dealKnockBack` is `causeExtraKnockback`: `LivingEntity.knockback`
    /// on the target, away from (sin(yaw), -cos(yaw)), not a raw push of its motion.
    #[test]
    fn a_charge_knocks_its_target_back_along_its_facing() {
        let mut events = Vec::new();
        let ran = with_test_world(&[], |w, types| {
            let mut w = w.clone();
            let at = Vec3::new(0.5, 64.0, 0.5);
            w.player = Some(crate::mobs::world::test_player(at));
            let mut m = Mob::new(
                1,
                Arc::new(types.types["minecraft:nautilus"].clone()),
                Vec3::new(0.5, 64.0, 0.2),
                0.0,
            );
            m.ai.target = Some(Who::Player);
            let mut goal = Charge::new(0.5);
            let mut lr = horizon_core::random::LegacyRandom::new(1);
            let mut cx = Cx {
                w: &w,
                lr: &mut lr,
                events: &mut events,
            };
            goal.tick(&mut m, &mut cx);
        });
        if !ran {
            return;
        }
        let knock = events.iter().find_map(|e| match e {
            MobEvent::Knockback {
                target,
                power,
                x,
                z,
                ..
            } => Some((*target, *power, *x, *z)),
            _ => None,
        });
        let (target, power, x, z) = knock.expect("the charge knocks its target back");
        assert_eq!(target, Who::Player);
        assert!(power > 0.0);
        // It turns to the target, which is straight ahead at +z (yaw 0): sin = 0, -cos = -1.
        assert!(x.abs() < 1e-3 && (z + 1.0).abs() < 1e-3, "{x} {z}");
        assert!(!events.iter().any(|e| matches!(e, MobEvent::Push { .. })));
    }

    /// A saddled nautilus with a rider looking south (`+z`), from the trip's
    /// own `MobType`, so no prepared resources are needed.
    fn ridden() -> Mob {
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        let k = Arc::make_mut(&mut kind);
        k.id = "minecraft:nautilus".into();
        k.species = Species::Nautilus;
        k.width = 0.9;
        k.height = 0.9;
        let mut m = Mob::new(7, kind, Vec3::new(1.5, 40.0, -2.5), 0.0);
        m.set_saddle(Some(crate::items::ItemStack::new("minecraft:saddle", 1)));
        m.rider = Some(super::super::equine::Rider {
            yaw: 0.0,
            pitch: 0.0,
            ..Default::default()
        });
        m
    }

    /// `AbstractNautilus.onPlayerJump` keeps the released bar's scale (0.4 to
    /// 1.0, none while the dash clock runs) and `tickRidden` spends it:
    /// `executeRidersJump` pushes along the rider's look at 0.5 on land,
    /// starts the 40-tick clock, raises DASH and queues the dash sound.
    #[test]
    fn the_dash_runs_off_the_scale_the_released_bar_left() {
        let mut m = ridden();
        assert!(on_player_jump(&mut m, 100));
        assert_eq!(m.rider.expect("the rider stays on").jump, 1.0);
        super::super::equine::ridden_step(&mut m);
        assert_eq!(
            m.rider.expect("the rider stays on").jump,
            0.0,
            "the scale is spent"
        );
        assert_eq!(m.extra.anim_ticks, DASH_COOLDOWN);
        assert!(m.extra.flag2, "DASH");
        assert_eq!(m.extra.scratch[1], 1, "the dash sound plays next tick");
        let k = 0.5 * m.movement_speed();
        assert!(
            (m.velocity.z - k).abs() < 1.0e-9 && m.velocity.x.abs() < 1.0e-9,
            "a dash along +z: {:?} against {k}",
            m.velocity
        );
        // A dash still in its cooldown takes nothing (`dashCooldown > 0`), and
        // neither does an unsaddled nautilus.
        assert!(!on_player_jump(&mut m, 50));
        m.extra.anim_ticks = 0;
        m.extra.saddled = false;
        assert!(!on_player_jump(&mut m, 50));
        assert_eq!(m.rider.expect("the rider stays on").jump, 0.0);
    }
}

#[cfg(test)]
mod hostile_target_tests {
    use super::*;
    use std::sync::Arc;

    /// A pool from x -8 to 8, with a stone wall at x = 3 when `wall`.
    fn pool(
        wall: bool,
    ) -> impl FnOnce(&mut crate::world::World, &horizon_world::registry::BlockRegistry) {
        move |world, registry| {
            let water = registry.default_state("minecraft:water").unwrap();
            let stone = registry.default_state("minecraft:stone").unwrap();
            for cx in -1..2 {
                for cz in -1..1 {
                    world
                        .chunks
                        .insert((cx, cz), Arc::new(crate::chunk::Chunk::empty(cx, cz)));
                }
            }
            for x in -8..=18 {
                for y in 60..=66 {
                    for z in -4..=4 {
                        let block = if wall && x == 3 { stone } else { water };
                        world.set(x, y, z, block).unwrap();
                    }
                }
            }
        }
    }

    /// `NautilusAi.findNearestValidAttackTarget`: past the cooldown and the
    /// level random's 1 in 2, the target is the closest #nautilus_hostiles in
    /// water among NEAREST_VISIBLE_LIVING_ENTITIES, so not a pufferfish behind
    /// stone. Each seed is a fresh level random.
    fn targets(wall: bool, fish_at: Vec3) -> Option<Vec<Option<Who>>> {
        let mut out = None;
        let mut fish = None;
        crate::mobs::world::with_built_test_world(
            &[],
            |_, _| {},
            |_, types| {
                let kind = Arc::new(types.types["minecraft:pufferfish"].clone());
                fish = Some(Mob::new(2, kind, fish_at, 0.0).seen(None));
            },
        );
        let fish = fish?;
        crate::mobs::world::with_built_test_world(&[fish], pool(wall), |w, types| {
            let kind = Arc::new(types.types["minecraft:nautilus"].clone());
            let picks = (0..16)
                .map(|seed| {
                    let mut m = Mob::new(1, kind.clone(), Vec3::new(0.5, 62.0, 0.5), 270.0);
                    m.fluids.in_water = true;
                    let mut level = horizon_core::random::LegacyRandom::new(seed);
                    find_target(&mut m, w, &mut level)
                })
                .collect();
            out = Some(picks);
        });
        out
    }

    #[test]
    fn a_nautilus_only_targets_a_pufferfish_it_sees() {
        let near = Vec3::new(6.5, 62.0, 0.5);
        // 16 out on x and 3 on z: inside the sensor's box, past range(16).
        let far = Vec3::new(16.5, 62.0, 3.5);
        let (Some(open), Some(walled), Some(beyond)) = (
            targets(false, near),
            targets(true, near),
            targets(false, far),
        ) else {
            return;
        };
        assert!(open.contains(&Some(Who::Mob(2))), "{open:?}");
        assert!(open.iter().all(|t| t.is_none() || *t == Some(Who::Mob(2))));
        assert!(walled.iter().all(Option::is_none), "{walled:?}");
        assert!(beyond.iter().all(Option::is_none), "{beyond:?}");
    }
}

#[cfg(test)]
mod cooldown_memory_tests {
    use super::*;
    use horizon_world::nbt::Tag;
    use std::sync::Arc;

    const MEMORY: &str = "minecraft:attack_target_cooldown";
    fn raw(id: &str, state: Option<(i32, Option<i64>)>, no_ai: bool) -> Tag {
        let memory = state.map(|(value, ttl)| {
            let mut entries = vec![("value", Tag::Int(value))];
            if let Some(ttl) = ttl {
                entries.push(("ttl", Tag::Long(ttl)));
            }
            Tag::compound(entries)
        });
        let memories = Tag::compound(memory.into_iter().map(|m| (MEMORY, m)));
        let mut fields = vec![
            ("id", Tag::String(id.into())),
            (
                "Pos",
                Tag::List(vec![
                    Tag::Double(4.5f64.to_bits()),
                    Tag::Double(3.0f64.to_bits()),
                    Tag::Double(4.5f64.to_bits()),
                ]),
            ),
            ("NoAI", Tag::Byte(i8::from(no_ai))),
            ("NoGravity", Tag::Byte(1)),
            ("Health", Tag::Float(10.0f32.to_bits())),
            ("PersistenceRequired", Tag::Byte(1)),
            ("Brain", Tag::compound([("memories", memories)])),
        ];
        if !no_ai {
            fields.push(("Owner", Tag::IntArray(vec![1, 2, 3, 4])));
        }
        Tag::compound(fields)
    }
    fn memory(tag: &Tag) -> Option<&Tag> {
        tag.get("Brain")?.get("memories")?.get(MEMORY)
    }
    fn expected(state: Option<(i32, Option<i64>)>) -> Option<Tag> {
        state.map(|(value, ttl)| {
            let mut entries = vec![("value", Tag::Int(value))];
            if let Some(ttl) = ttl.filter(|&ttl| ttl != i64::MAX) {
                entries.push(("ttl", Tag::Long(ttl)));
            }
            Tag::compound(entries)
        })
    }
    fn saved(m: &Mob, stale: Option<&Tag>) -> Tag {
        crate::entity_nbt::mob_to_nbt(m, [140, 1, 0, 7], stale, None, |_| None)
    }
    #[test]
    fn decoded_cooldown_presence_ttl_and_model_mutation_save_without_raw() {
        let mut failures = Vec::new();
        for (id, species) in [
            ("minecraft:nautilus", Species::Nautilus),
            ("minecraft:zombie_nautilus", Species::ZombieNautilus),
        ] {
            let mut kind = (*crate::mobs::sensor_scan_tests::kind()).clone();
            kind.id = id.into();
            kind.species = species;
            let types = std::collections::HashMap::from([(id.to_owned(), Arc::new(kind))]);
            for state in [
                None,
                Some((0, None)),
                Some((37, None)),
                Some((37, Some(5))),
                Some((37, Some(i64::MAX))),
            ] {
                let tag = raw(id, state, true);
                let mut m = crate::entity_nbt::mob_from_nbt(&tag, &types, 7, None)
                    .expect("actual class decode");
                assert_eq!(m.species(), species);
                assert!(m.no_ai && m.no_gravity && m.alive());
                assert_eq!(m.health, 10.0);
                assert_eq!(m.extra.counter, state.map_or(0, |s| s.0));
                for source in [None, Some(&tag)] {
                    let out = saved(&m, source);
                    if memory(&out).cloned() != expected(state) {
                        failures.push(format!(
                            "{id} {state:?} raw={} saved={:?}",
                            source.is_some(),
                            memory(&out)
                        ));
                    }
                }
                if memory(&saved(&m.clone(), None)).cloned() != expected(state) {
                    failures.push(format!("{id} cloned model lost memory presence/lifetime"));
                }
                if state.is_some() {
                    m.extra.counter = 41; // The live value changes; the raw compound still holds the old value.
                    let want = state.map(|(_, ttl)| (41, ttl));
                    if memory(&saved(&m, Some(&tag))).cloned() != expected(want) {
                        failures.push(format!("{id} stale raw value/TTL replaced modeled state"));
                    }
                }
                // A later valid empty memory map clears the class memory, not merely its value.
                from_nbt(&mut m, &raw(id, None, true));
                if memory(&saved(&m, Some(&tag))).is_some() {
                    failures.push(format!("{id} valid empty map retained stale memory"));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "saved nautilus cooldown state mismatches: {failures:?}"
        );
    }

    #[test]
    #[ignore = "needs actual prepared mob data; must execute, not skip"]
    fn actual_mob_ticks_retain_noai_lifetime_and_apply_active_memory_boundaries() {
        let root = std::path::PathBuf::from(
            std::env::var_os("HORIZON_TEST_RESOURCES").expect("actual prepared resources"),
        );
        let mut failures = Vec::new();
        let ran = crate::mobs::world::with_resource_test_world(
            &root,
            &[],
            true,
            true,
            |world, registry| {
                let water = registry.default_state("minecraft:water").unwrap();
                let stone = registry.default_state("minecraft:stone").unwrap();
                world
                    .chunks
                    .insert((0, 0), Arc::new(crate::chunk::Chunk::empty(0, 0)));
                for x in 0..16 {
                    for z in 0..16 {
                        world.set(x, 0, z, stone).unwrap();
                        for y in 1..6 {
                            world.set(x, y, z, water).unwrap();
                        }
                    }
                }
            },
            |world, types| {
                let types: std::collections::HashMap<_, _> = types
                    .types
                    .iter()
                    .map(|(id, kind)| (id.clone(), Arc::new(kind.clone())))
                    .collect();
                for id in ["minecraft:nautilus", "minecraft:zombie_nautilus"] {
                    for (no_ai, state) in [
                        (true, None),
                        (true, Some((0, None))),
                        (true, Some((37, None))),
                        (true, Some((37, Some(5)))),
                        (true, Some((37, Some(i64::MAX)))),
                        (false, Some((37, Some(5)))),
                        (false, Some((37, Some(1)))),
                        (false, Some((37, Some(0)))),
                    ] {
                        let tag = raw(id, state, no_ai);
                        let mut m = crate::entity_nbt::mob_from_nbt(&tag, &types, 7, None).unwrap();
                        m.attrs.speed = 0.0;
                        let mut level_random = horizon_core::random::LegacyRandom::new(140);
                        let mut events = Vec::new();
                        for elapsed in 0..=6 {
                            if elapsed > 0 {
                                let mut w = world.clone();
                                w.game_time += elapsed;
                                assert!(m.tick(&w, &mut level_random, &mut events));
                                assert!(m.fluids.in_water && m.fluids.eye_in_water);
                                assert!(m.ai.target.is_none(), "safe no-attack control");
                                assert!(
                                    !events.iter().any(|e| matches!(
                                        e,
                                        MobEvent::AttackPlayer(..) | MobEvent::AttackMob { .. }
                                    )),
                                    "no attack fixture"
                                );
                            }
                            assert_eq!(m.health, 10.0);
                            assert_eq!(m.no_ai, no_ai);
                            assert!(m.no_gravity);
                            let want = if no_ai || elapsed == 0 {
                                state
                            } else if state.unwrap().1 == Some(0) {
                                None
                            } else {
                                Some((37 - elapsed as i32, None))
                            };
                            if memory(&saved(&m, None)).cloned() != expected(want) {
                                failures.push(format!(
                                    "{id} NoAI={no_ai} {state:?} tick={elapsed}: {:?}",
                                    memory(&saved(&m, None))
                                ));
                            }
                        }
                    }
                }
            },
        );
        assert!(ran, "actual prepared world route must execute");
        assert!(
            failures.is_empty(),
            "nautilus cooldown tick/save mismatches: {failures:?}"
        );
    }
}

#[cfg(test)]
mod feed_heal_tests {
    use super::feed_heal;

    #[test]
    fn heals_twice_the_nutrition_of_vanilla_foods() {
        // Foods.java: cod 2, pufferfish 1, cooked cod 5, cooked salmon 6.
        assert_eq!(feed_heal(Some(2)), 4.0);
        assert_eq!(feed_heal(Some(1)), 2.0);
        assert_eq!(feed_heal(Some(5)), 10.0);
        assert_eq!(feed_heal(Some(6)), 12.0);
    }

    #[test]
    fn item_without_food_component_heals_one() {
        assert_eq!(feed_heal(None), 1.0);
    }
}
