//! Guardians and elder guardians (`Guardian`, `ElderGuardian`): the laser
//! charging for 80 (60) ticks before it strikes, spikes that hurt melee
//! attackers while still, flopping on land, the swimming move control, and
//! the elder's mining fatigue curse every minute.
use super::{
    Damage, DamageKind, Mob, MobEvent, MobWorld, Species, Who,
    anim::{Poses, State},
    control::{MoveKind, Operation, rotlerp},
    goals::{
        Cx, Goal, LOOK, LookAtPlayer, MOVE, NearestTarget, RandomLookAround, Stroll, goal_clone,
    },
    nav::NavKind,
    path::PathType,
};
use horizon_core::geometry::trig::Mth;
use horizon_core::{geometry::Vec3, random::RandomSource};
use std::f32::consts::PI;

// Guardian state: `flag` an active beam target (DATA_ID_ATTACK_TARGET),
// `flag2` moving (DATA_ID_MOVING), `counter2` the client attack time.
// AnimState slots: tail animation, its speed, spikes animation.
pub const TAIL: usize = 0;
const TAIL_SPEED: usize = 1;
pub const SPIKES: usize = 2;
const EYE_X: usize = 3;
const EYE_Y: usize = 4;

pub fn is_elder(m: &Mob) -> bool {
    m.species() == Species::ElderGuardian
}
fn attack_duration(m: &Mob) -> i32 {
    if is_elder(m) { 60 } else { 80 }
}
pub fn register(m: &mut Mob) {
    m.ai.mov.kind = MoveKind::Custom;
    m.ai.nav.kind = NavKind::Water { breach: false };
    m.set_malus(PathType::Water, 0.0);
    let t = m.random.next_float();
    m.anim.v[TAIL] = t;
    m.anim.vo[TAIL] = t;
    let elder = is_elder(m);
    if elder {
        m.persistent = true;
    }
    let g = &mut m.ai.goals;
    g.add(4, GuardianAttack { attack_time: 0 });
    g.add(7, Stroll::new(1.0).interval(if elder { 400 } else { 80 }));
    g.add(8, LookAtPlayer::new(8.0));
    g.add(9, RandomLookAround::default());
    // GuardianAttackSelector: players, squid and axolotls beyond 3 blocks.
    m.ai.targets.add(
        1,
        NearestTarget::player(true).when(|m, w| {
            w.player
                .as_ref()
                .is_some_and(|p| p.position.distance_squared(m.position) > 9.0)
        }),
    );
    m.ai.targets.add(
        1,
        NearestTarget::species(
            &[Species::Squid, Species::GlowSquid, Species::Axolotl],
            true,
        )
        .beyond(9.0),
    );
}
/// `GuardianAttackGoal`.
#[derive(Clone, Debug)]
struct GuardianAttack {
    attack_time: i32,
}
impl Goal for GuardianAttack {
    goal_clone!();
    fn flags(&self) -> u8 {
        MOVE | LOOK
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        m.target_info(cx.w).is_some_and(|t| t.alive)
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        self.can_use(m, cx)
            && (is_elder(m)
                || m.target_info(cx.w)
                    .is_some_and(|t| t.position.distance_squared(m.position) > 9.0))
    }
    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        self.attack_time = -10;
        m.ai.nav.stop();
        if let Some(t) = m.target_info(cx.w) {
            m.look_at_speed([t.position.x, t.eye_y, t.position.z], 90.0, 90.0);
        }
    }
    fn stop(&mut self, m: &mut Mob, _cx: &mut Cx) {
        m.extra.flag = false;
        m.ai.target = None;
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let (Some(who), Some(t)) = (m.ai.target, m.target_info(cx.w)) else {
            return;
        };
        m.ai.nav.stop();
        m.look_at_speed([t.position.x, t.eye_y, t.position.z], 90.0, 90.0);
        if !m.sees(cx.w, who) {
            m.ai.target = None;
            return;
        }
        self.attack_time += 1;
        if self.attack_time == 0 {
            m.extra.flag = true;
            m.extra.counter2 = 0;
            // Entity event 21: the attack sound loop starts on the clients.
            if !m.silent {
                cx.events.push(MobEvent::EntityEvent {
                    mob: m.id,
                    event: 21,
                    at: [m.position.x, m.position.y, m.position.z],
                });
            }
        } else if self.attack_time >= attack_duration(m) {
            // Indirect magic (1, +2 on Hard, +2 for the elder), then
            // doHurtTarget.
            let magic = 1.0
                + if cx.w.difficulty.hard { 2.0 } else { 0.0 }
                + if is_elder(m) { 2.0 } else { 0.0 };
            let from = [m.position.x, m.position.y, m.position.z];
            match who {
                Who::Player => cx.events.push(MobEvent::DamagePlayer {
                    amount: magic,
                    from,
                    kind: "minecraft:indirect_magic",
                }),
                Who::Mob(id) => cx.events.push(MobEvent::AttackMob {
                    target: id,
                    amount: magic,
                    source: Damage {
                        kind: DamageKind::Magic,
                        attacker: Some(Who::Mob(m.id)),
                        from: Some(from),
                    },
                }),
            }
            super::ai::do_hurt_target(m, who, cx);
            m.ai.target = None;
        }
    }
}
/// `GuardianMoveControl`: swim straight at the wanted point with a wobble,
/// looking ahead.
pub fn move_control(m: &mut Mob) {
    let moving = m.ai.mov.operation == Operation::MoveTo && !m.ai.nav.is_done();
    if !moving {
        m.set_speed(0.0);
        m.extra.flag2 = false;
        return;
    }
    let [wx, wy, wz] = m.ai.mov.wanted;
    let d = Vec3::new(wx - m.position.x, wy - m.position.y, wz - m.position.z);
    let length = (d.x * d.x + d.y * d.y + d.z * d.z).sqrt();
    let (xd, yd, zd) = (d.x / length, d.y / length, d.z / length);
    let y_rot = (d.z.atan2(d.x).to_degrees() as f32) - 90.0;
    m.yaw = rotlerp(m.yaw, y_rot, 90.0);
    m.body_yaw = m.yaw;
    let target = (m.ai.mov.speed_modifier * m.movement_speed()) as f32;
    let speed = m.ai.speed + (target - m.ai.speed) * 0.125;
    m.set_speed(speed);
    let t = f64::from(m.age as i32 + m.id as i32);
    let push = (t * 0.5).sin() * 0.05;
    let (cos, sin) = (
        f64::from(m.yaw.to_radians().mcos()),
        f64::from(m.yaw.to_radians().msin()),
    );
    let y_push = (t * 0.75).sin() * 0.05;
    m.velocity = m.velocity.add_vector(Vec3::new(
        push * cos,
        y_push * (sin + cos) * 0.25 + f64::from(speed) * yd * 0.1,
        push * sin,
    ));
    let new_look = [
        m.position.x + xd * 2.0,
        m.eye_y() + yd / length,
        m.position.z + zd * 2.0,
    ];
    let old = if m.ai.look.cooldown > 0 {
        m.ai.look.wanted
    } else {
        new_look
    };
    let lerp = |a: f64, b: f64| a + (b - a) * 0.125;
    m.look_at_speed(
        [
            lerp(old[0], new_look[0]),
            lerp(old[1], new_look[1]),
            lerp(old[2], new_look[2]),
        ],
        10.0,
        40.0,
    );
    m.extra.flag2 = true;
}
/// `Guardian.travelInWater`; on land the usual travel.
pub fn travel(m: &mut Mob, w: &MobWorld, input: Vec3, events: &mut Vec<MobEvent>) -> bool {
    if !m.fluids.in_water {
        return false;
    }
    m.move_relative(0.1, input);
    m.do_move(w, events);
    m.velocity = m.velocity.scale(0.9);
    if !m.extra.flag2 && m.ai.target.is_none() {
        m.velocity.y -= 0.005;
    }
    true
}
/// `Guardian.aiStep`'s `clientSideTailAnimation` and
/// `clientSideSpikesAnimation` (the server's and a client's).
pub fn tail_and_spikes(m: &mut Mob) {
    let moving = m.extra.flag2;
    let in_water = m.fluids.in_water;
    // Tail and spikes (clientSideTailAnimation / clientSideSpikesAnimation).
    let a = &mut m.anim;
    let speed = &mut a.v[TAIL_SPEED];
    if !in_water {
        *speed = 2.0;
    } else if moving {
        if *speed < 0.5 {
            *speed = 4.0;
        } else {
            *speed += (0.5 - *speed) * 0.1;
        }
    } else {
        *speed += (0.125 - *speed) * 0.2;
    }
    a.v[TAIL] += a.v[TAIL_SPEED];
    if !in_water {
        a.v[SPIKES] = m.random.next_float();
    } else if moving {
        a.v[SPIKES] += (0.0 - a.v[SPIKES]) * 0.25;
    } else {
        a.v[SPIKES] += (1.0 - a.v[SPIKES]) * 0.06;
    }
}
/// `Guardian.aiStep`: the client animations, air, flopping on land, the
/// beam's clock, and the spikes' thorns.
pub fn ai_step(m: &mut Mob, w: &MobWorld, events: &mut Vec<MobEvent>) {
    if !m.alive() {
        return;
    }
    let moving = m.extra.flag2;
    let in_water = m.fluids.in_water;
    tail_and_spikes(m);
    client_tick(m);
    if in_water {
        m.air = 300;
    } else if m.on_ground {
        let r = &mut m.random;
        let (x, z) = (r.next_float() * 2.0 - 1.0, r.next_float() * 2.0 - 1.0);
        m.velocity = m
            .velocity
            .add_vector(Vec3::new(f64::from(x) * 0.4, 0.5, f64::from(z) * 0.4));
        m.yaw = m.random.next_float() * 360.0;
        m.on_ground = false;
        let flop = if is_elder(m) {
            "entity.elder_guardian.flop"
        } else {
            "entity.guardian.flop"
        };
        events.push(MobEvent::Sound(
            flop,
            [m.position.x, m.position.y, m.position.z],
            1.0,
            1.0,
        ));
    }
    if m.extra.flag {
        m.yaw = m.head_yaw;
    }
    // The eye follows the active target, else the player (the camera).
    let look = match m.target_info(w).filter(|_| m.extra.flag) {
        Some(t) => Some([t.position.x, t.eye_y, t.position.z]),
        None => w.player.as_ref().map(|p| p.eye),
    };
    if let Some(look) = look {
        look_at(m, look);
    }
    // Guardian.hurtServer: a still guardian's spikes hurt a melee attacker.
    if m.hurt_time == 10
        && !moving
        && let Some((d, _)) = m.last_damage
        && matches!(
            d.kind,
            DamageKind::PlayerAttack
                | DamageKind::MaceSmash
                | DamageKind::Spear
                | DamageKind::MobAttack
        )
    {
        let from = [m.position.x, m.position.y, m.position.z];
        match d.attacker {
            Some(Who::Player) => events.push(MobEvent::DamagePlayer {
                amount: 2.0,
                from,
                kind: "minecraft:thorns",
            }),
            Some(Who::Mob(id)) => events.push(MobEvent::AttackMob {
                target: id,
                amount: 2.0,
                source: Damage {
                    kind: DamageKind::Thorns,
                    attacker: Some(Who::Mob(m.id)),
                    from: Some(from),
                },
            }),
            None => {}
        }
    }
}
/// `GuardianModel.setupAnim`'s eye: it follows `look` (the attack target's
/// eye, else the camera's), from where the guardian faces.
pub fn look_at(m: &mut Mob, look: [f64; 3]) {
    m.anim.v[EYE_Y] = if look[1] - m.eye_y() > 0.0 { 0.0 } else { 1.0 };
    let yaw = m.head_yaw.to_radians();
    let pitch = m.head_pitch.to_radians();
    let view = [-yaw.msin() * pitch.mcos(), yaw.mcos() * pitch.mcos()];
    let d = [m.position.x - look[0], m.position.z - look[2]];
    let len = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-9);
    let (dx, dz) = ((d[0] / len) as f32, (d[1] / len) as f32);
    // Vec3.yRot(pi / 2) of the direction from the target.
    let side = [dz, -dx];
    let dot = view[0] * side[0] + view[1] * side[1];
    m.anim.v[EYE_X] = dot.abs().sqrt() * 2.0 * dot.signum();
}
/// `ElderGuardian.customServerAiStep`: every minute, mining fatigue III for
/// players within 50 blocks; a home within 16 blocks.
pub fn elder_server_ai_step(m: &mut Mob, cx: &mut Cx) {
    if (m.age as i64 + i64::from(m.id)) % 1200 == 0 {
        cx.events.push(MobEvent::ElderCurse {
            at: [m.position.x, m.position.y, m.position.z],
        });
    }
    if m.extra.home.is_none() {
        m.extra.home = Some([
            m.position.x.floor() as i32,
            m.position.y.floor() as i32,
            m.position.z.floor() as i32,
        ]);
    }
}
/// Guardian.aiStep's visual attack clock, also used by server clients.
/// Does not run attack goals, movement, damage or random draws.
pub fn client_tick(m: &mut Mob) {
    if m.extra.flag && m.extra.counter2 < attack_duration(m) {
        m.extra.counter2 += 1;
    }
}
/// The beam's clock for rendering: (attack time with the partial tick,
/// attack scale), while a target is locked.
pub fn beam(m: &Mob, partial: f32) -> Option<(f32, f32)> {
    let guardian = matches!(m.species(), Species::Guardian | Species::ElderGuardian);
    if !guardian || !m.extra.flag || m.ai.target.is_none() {
        return None;
    }
    let time = m.extra.counter2 as f32 + partial;
    Some((time, time / attack_duration(m) as f32))
}
/// `GuardianRenderer.renderBeam` from `eye` to `target` (world minus
/// camera): the four rotating sides and the end cap. The texture's V
/// repeats along the beam, so the sides are cut at each repeat.
pub fn beam_geometry(
    eye: glam::Vec3,
    target: glam::Vec3,
    time: f32,
    scale: f32,
    uv: [f32; 4],
    out: &mut Vec<crate::mesher::Vertex>,
) {
    let v = target - eye;
    let length = v.length() + 1.0;
    let dir = v.normalize_or_zero();
    let x_rot = dir.y.clamp(-1.0, 1.0).acos();
    let y_rot = std::f32::consts::FRAC_PI_2 - dir.z.atan2(dir.x);
    let pose = glam::Mat4::from_translation(eye)
        * glam::Mat4::from_rotation_y(y_rot)
        * glam::Mat4::from_rotation_x(x_rot);
    let rot = time * 0.05 * -1.5;
    let cs = scale * scale;
    let (r, g, b) = (
        64 + (cs * 191.0) as u32,
        32 + (cs * 191.0) as u32,
        128u32.saturating_sub((cs * 64.0) as u32),
    );
    let color = 0xFF00_0000 | (b.min(255) << 16) | (g.min(255) << 8) | r.min(255);
    let at = |a: f32, radius: f32| (a.mcos() * radius, a.msin() * radius);
    let quarter = std::f32::consts::FRAC_PI_4;
    let (wx, wz) = at(rot + PI, 0.2);
    let (ex, ez) = at(rot, 0.2);
    let (nx, nz) = at(rot + PI / 2.0, 0.2);
    let (sx, sz) = at(rot + PI * 1.5, 0.2);
    let (wnx, wnz) = at(rot + quarter * 3.0, 0.282);
    let (enx, enz) = at(rot + quarter, 0.282);
    let (wsx, wsz) = at(rot + quarter * 5.0, 0.282);
    let (esx, esz) = at(rot + quarter * 7.0, 0.282);
    let tex_off = (time * 0.5) % 1.0;
    let min_v = -1.0 + tex_off;
    let max_v = min_v + length * 2.5;
    let atlas = |u: f32, v: f32| [uv[0] + (uv[2] - uv[0]) * u, uv[1] + (uv[3] - uv[1]) * v];
    let mut push = |p: [f32; 3], t: [f32; 2]| {
        out.push(crate::mesher::Vertex {
            normal: [0; 4],
            pos: pose.transform_point3(glam::Vec3::from(p)).to_array(),
            uv: t,
            layer: 0,
            color,
            light: 240 | 240 << 8,

            glint_uv: [0.0, 0.0],
        });
    };
    // Sides: one quad per texture repeat.
    let mut k = min_v.floor();
    while k < max_v {
        let (v0, v1) = (min_v.max(k), max_v.min(k + 1.0));
        let (y0, y1) = (
            (v0 - min_v) / (max_v - min_v) * length,
            (v1 - min_v) / (max_v - min_v) * length,
        );
        let (t0, t1) = (v0 - k, v1 - k);
        for (a, c) in [((wx, wz), (ex, ez)), ((nx, nz), (sx, sz))] {
            push([a.0, y1, a.1], atlas(0.4999, t1));
            push([a.0, y0, a.1], atlas(0.4999, t0));
            push([c.0, y0, c.1], atlas(0.0, t0));
            push([c.0, y1, c.1], atlas(0.0, t1));
        }
        k += 1.0;
    }
    let base = if (time.floor() as i32) % 2 == 0 {
        0.5
    } else {
        0.0
    };
    push([wnx, length, wnz], atlas(0.5, base + 0.5));
    push([enx, length, enz], atlas(1.0, base + 0.5));
    push([esx, length, esz], atlas(1.0, base));
    push([wsx, length, wsz], atlas(0.5, base));
}
/// `GuardianModel.setupAnim`: head, spikes by withdrawal, the eye toward
/// what it looks at, and the tail.
pub fn anim(m: &Mob, s: &State, p: &mut Poses) {
    p.yrot("head", s.y_rot.to_radians());
    p.xrot("head", s.x_rot.to_radians());
    let spikes = m.anim.lerp(SPIKES, s.partial);
    let withdrawal = (1.0 - spikes) * 0.55;
    const X: [f32; 12] = [
        0.0, 0.0, 8.0, -8.0, -8.0, 8.0, 8.0, -8.0, 0.0, 0.0, 8.0, -8.0,
    ];
    const Y: [f32; 12] = [
        -8.0, -8.0, -8.0, -8.0, 0.0, 0.0, 0.0, 0.0, 8.0, 8.0, 8.0, 8.0,
    ];
    const Z: [f32; 12] = [
        8.0, -8.0, 0.0, 0.0, -8.0, -8.0, 8.0, 8.0, 8.0, -8.0, 0.0, 0.0,
    ];
    for i in 0..12 {
        let offset = 1.0 + (s.age * 1.5 + i as f32).mcos() * 0.01 - withdrawal;
        let part = p.get(&format!("spike{i}"));
        part[0] = X[i] * offset;
        part[1] = 16.0 + Y[i] * offset;
        part[2] = Z[i] * offset;
    }
    let e = p.get("eye");
    e[0] = m.anim.v[EYE_X];
    e[1] = m.anim.v[EYE_Y];
    let tail = m.anim.lerp(TAIL, s.partial);
    p.yrot("tail0", tail.msin() * PI * 0.05);
    p.yrot("tail1", tail.msin() * PI * 0.1);
    p.yrot("tail2", tail.msin() * PI * 0.15);
}
/// `Guardian.getAttackAnimationScale(0)`: how far the beam has charged. A
/// beam that ended counts from zero again.
pub fn attack_scale(m: &Mob) -> f32 {
    if m.extra.flag {
        m.extra.counter2 as f32 / attack_duration(m) as f32
    } else {
        0.0
    }
}
