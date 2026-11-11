//! Vanilla's `GoalSelector` (priorities, MOVE/LOOK/JUMP/TARGET control flags,
//! interruption) and the shared goals of `net.minecraft.world.entity.ai.goal`.
use super::{Mob, MobEvent, MobWorld, Seen, Species, Who, world::TargetInfo};
use horizon_core::{
    geometry::{Aabb, Vec3},
    jmath,
    random::{LegacyRandom, RandomSource},
};
use std::sync::Arc;

pub const MOVE: u8 = 1;
pub const LOOK: u8 = 2;
pub const JUMP: u8 = 4;
pub const TARGET: u8 = 8;

/// What goals may read and emit during a tick.
pub struct Cx<'a, 'w> {
    pub w: &'a MobWorld<'w>,
    /// The level's random (`level.getRandom()`), for the draws vanilla's
    /// behaviours make from it rather than from the mob's own.
    pub lr: &'a mut LegacyRandom,
    pub events: &'a mut Vec<MobEvent>,
}
pub trait Goal: std::fmt::Debug + Send + Sync {
    fn flags(&self) -> u8;
    /// A goal that draws its first start delay in its constructor (a schooling fish's
    /// `FollowFlockLeaderGoal`): the delay again, when the mob's random is seeded afterwards.
    fn set_start_delay(&mut self, _ticks: i32) {}
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool;
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        self.can_use(m, cx)
    }
    fn interruptable(&self) -> bool {
        true
    }
    fn start(&mut self, _m: &mut Mob, _cx: &mut Cx) {}
    fn stop(&mut self, _m: &mut Mob, _cx: &mut Cx) {}
    fn every_tick(&self) -> bool {
        false
    }
    fn tick(&mut self, _m: &mut Mob, _cx: &mut Cx) {}
    fn box_clone(&self) -> Box<dyn Goal>;
    /// The goal's name for debugging (`Goal.toString`).
    fn name(&self) -> &'static str {
        std::any::type_name::<Self>()
            .rsplit("::")
            .next()
            .unwrap_or("Goal")
    }
}
impl Clone for Box<dyn Goal> {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}
macro_rules! goal_clone {
    () => {
        fn box_clone(&self) -> Box<dyn crate::mobs::goals::Goal> {
            Box::new(self.clone())
        }
    };
}
pub(crate) use goal_clone;

/// Goal.reducedTickDelay
pub fn reduced(ticks: i32) -> i32 {
    (ticks + 1) / 2
}
fn adjusted(every_tick: bool, ticks: i32) -> i32 {
    if every_tick { ticks } else { reduced(ticks) }
}

#[derive(Clone, Debug)]
struct Wrapped {
    priority: i32,
    goal: Box<dyn Goal>,
    running: bool,
}
#[derive(Clone, Debug, Default)]
pub struct GoalSelector {
    goals: Vec<Wrapped>,
    locked: [Option<usize>; 4],
    pub disabled: u8,
}
impl GoalSelector {
    pub fn add(&mut self, priority: i32, goal: impl Goal + 'static) {
        self.goals.push(Wrapped {
            priority,
            goal: Box::new(goal),
            running: false,
        });
    }
    /// `Goal::set_start_delay` of the goal of this name.
    pub fn set_start_delay(&mut self, name: &str, ticks: i32) {
        for g in &mut self.goals {
            if g.goal.name() == name {
                g.goal.set_start_delay(ticks);
            }
        }
    }
    pub fn clear(&mut self) {
        self.goals.clear();
        self.locked = [None; 4];
    }
    pub fn running_names(&self) -> Vec<&'static str> {
        self.goals
            .iter()
            .filter(|g| g.running)
            .map(|g| g.goal.name())
            .collect()
    }
    /// Whether a goal of this name is registered, running or not.
    pub fn has(&self, name: &str) -> bool {
        self.goals.iter().any(|g| g.goal.name() == name)
    }
    pub fn is_running(&self, name: &str) -> bool {
        self.goals
            .iter()
            .any(|g| g.running && g.goal.name() == name)
    }
    fn stop(&mut self, i: usize, m: &mut Mob, cx: &mut Cx) {
        if self.goals[i].running {
            self.goals[i].running = false;
            self.goals[i].goal.stop(m, cx);
        }
    }
    /// GoalSelector.tick
    pub fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        for i in 0..self.goals.len() {
            if self.goals[i].running
                && (self.goals[i].goal.flags() & self.disabled != 0
                    || !self.goals[i].goal.can_continue(m, cx))
            {
                self.stop(i, m, cx);
            }
        }
        for slot in &mut self.locked {
            if slot.is_some_and(|i| !self.goals[i].running) {
                *slot = None;
            }
        }
        for i in 0..self.goals.len() {
            let flags = self.goals[i].goal.flags();
            if self.goals[i].running || flags & self.disabled != 0 {
                continue;
            }
            let replaceable =
                (0..4)
                    .filter(|b| flags & (1 << b) != 0)
                    .all(|b| match self.locked[b] {
                        None => true,
                        Some(j) => {
                            self.goals[j].goal.interruptable()
                                && self.goals[i].priority < self.goals[j].priority
                        }
                    });
            if !replaceable || !self.goals[i].goal.can_use(m, cx) {
                continue;
            }
            for b in 0..4 {
                if flags & (1 << b) != 0 {
                    if let Some(j) = self.locked[b] {
                        self.stop(j, m, cx);
                    }
                    self.locked[b] = Some(i);
                }
            }
            if !self.goals[i].running {
                self.goals[i].running = true;
                self.goals[i].goal.start(m, cx);
            }
        }
        self.tick_running(m, cx, true);
    }
    pub fn tick_running(&mut self, m: &mut Mob, cx: &mut Cx, force: bool) {
        for g in &mut self.goals {
            if g.running && (force || g.goal.every_tick()) {
                g.goal.tick(m, cx);
            }
        }
    }
    pub fn stop_all(&mut self, m: &mut Mob, cx: &mut Cx) {
        for i in 0..self.goals.len() {
            self.stop(i, m, cx);
        }
        self.locked = [None; 4];
    }
}

// --- random positions (DefaultRandomPos / LandRandomPos / RandomPos) -------

/// RandomPos.moveUpToAboveSolid: out of the solid blocks, then up to
/// `above` more blocks of air.
fn move_up_to_above_solid(w: &MobWorld, mut p: [i32; 3], above: i32) -> [i32; 3] {
    if !w.solid(p[0], p[1], p[2]) {
        return p;
    }
    p[1] += 1;
    while p[1] < w.world.top_y() && w.solid(p[0], p[1], p[2]) {
        p[1] += 1;
    }
    let first = p[1];
    while p[1] < w.world.top_y() && p[1] - first < above {
        p[1] += 1;
        if w.solid(p[0], p[1], p[2]) {
            p[1] -= 1;
            break;
        }
    }
    p
}
#[path = "home.rs"]
mod home;
pub use home::within_home;

impl Mob {
    /// PathfinderMob.getWalkTargetValue.
    pub fn walk_target_value(&self, w: &MobWorld, p: [i32; 3]) -> f32 {
        super::ai::walk_target_value(self, w, p)
    }
    fn random_direction(&mut self, h: i32, v: i32) -> [i32; 3] {
        let x = self.random.next_int_bound(2 * h + 1).unwrap_or(0) - h;
        let y = self.random.next_int_bound(2 * v + 1).unwrap_or(0) - v;
        let z = self.random.next_int_bound(2 * h + 1).unwrap_or(0) - h;
        [x, y, z]
    }
    #[allow(clippy::too_many_arguments)]
    fn random_direction_within(
        &mut self,
        min: f64,
        max: f64,
        v: i32,
        fly_height: i32,
        xd: f64,
        zd: f64,
        max_rad: f64,
    ) -> Option<[i32; 3]> {
        let center =
            horizon_core::geometry::trig::atan2(zd, xd) - f64::from(std::f32::consts::FRAC_PI_2);
        let yrad = center + f64::from(2.0 * self.random.next_float() - 1.0) * max_rad;
        let dist = (min + self.random.next_double().sqrt() * (max - min))
            * f64::from(std::f32::consts::SQRT_2);
        let xt = -dist * jmath::sin(yrad);
        let zt = dist * jmath::cos(yrad);
        if xt.abs() > max || zt.abs() > max {
            return None;
        }
        let yt = self.random.next_int_bound(2 * v + 1).unwrap_or(0) - v + fly_height;
        Some([xt.floor() as i32, yt, zt.floor() as i32])
    }
    /// RandomPos.generateRandomPosTowardDirection: a mob with a home leans
    /// half the distance back toward it.
    fn toward(&mut self, h: f64, d: [i32; 3]) -> [i32; 3] {
        let (mut xt, mut zt) = (f64::from(d[0]), f64::from(d[2]));
        if let Some((c, _)) = self.ai.home
            && h > 1.0
        {
            if self.position.x > f64::from(c[0]) {
                xt -= self.random.next_double() * h / 2.0;
            } else {
                xt += self.random.next_double() * h / 2.0;
            }
            if self.position.z > f64::from(c[2]) {
                zt -= self.random.next_double() * h / 2.0;
            } else {
                zt += self.random.next_double() * h / 2.0;
            }
        }
        [
            (xt + self.position.x).floor() as i32,
            (f64::from(d[1]) + self.position.y).floor() as i32,
            (zt + self.position.z).floor() as i32,
        ]
    }
    /// `GoalUtils.isRestricted(mobRestricted(mob, h), mob, pos)`: a mob
    /// near its home keeps the positions it picks within it.
    pub fn restricted(&self, h: f64, p: [i32; 3]) -> bool {
        let Some((c, r)) = self.ai.home else {
            return false;
        };
        let near = {
            let d = |i: usize, v: f64| f64::from(c[i]) + 0.5 - v;
            let (x, y, z) = (
                d(0, self.position.x),
                d(1, self.position.y),
                d(2, self.position.z),
            );
            let reach = f64::from(r) + h + 1.0;
            x * x + y * y + z * z < reach * reach
        };
        near && !within_home(c, r, p)
    }
    fn default_check(&self, w: &MobWorld, h: f64, p: [i32; 3]) -> bool {
        p[1] >= w.world.bottom_y()
            && p[1] < w.world.top_y()
            && !self.restricted(h, p)
            && self.stable_destination(w, p)
            && self.malus(w.path_type_static(p[0], p[1], p[2])) == 0.0
    }
    fn pick_best(
        &mut self,
        w: &MobWorld,
        mut supplier: impl FnMut(&mut Self) -> Option<[i32; 3]>,
    ) -> Option<[f64; 3]> {
        let mut best: Option<[i32; 3]> = None;
        let mut best_weight = f64::NEG_INFINITY;
        for _ in 0..10 {
            if let Some(p) = supplier(self) {
                let v = f64::from(self.walk_target_value(w, p));
                if v > best_weight {
                    best_weight = v;
                    best = Some(p);
                }
            }
        }
        best.map(|p| {
            [
                f64::from(p[0]) + 0.5,
                f64::from(p[1]),
                f64::from(p[2]) + 0.5,
            ]
        })
    }
    /// DefaultRandomPos.getPos
    pub fn default_random_pos(&mut self, w: &MobWorld, h: i32, v: i32) -> Option<[f64; 3]> {
        self.pick_best(w, |m| {
            let d = m.random_direction(h, v);
            let p = m.toward(f64::from(h), d);
            m.default_check(w, f64::from(h), p).then_some(p)
        })
    }
    /// DefaultRandomPos.getPosAway
    pub fn default_random_pos_away(
        &mut self,
        w: &MobWorld,
        h: i32,
        v: i32,
        away: [f64; 3],
    ) -> Option<[f64; 3]> {
        let (xd, zd) = (self.position.x - away[0], self.position.z - away[2]);
        self.pick_best(w, |m| {
            let d = m.random_direction_within(
                0.0,
                f64::from(h),
                v,
                0,
                xd,
                zd,
                std::f64::consts::FRAC_PI_2,
            )?;
            let p = m.toward(f64::from(h), d);
            m.default_check(w, f64::from(h), p).then_some(p)
        })
    }
    /// DefaultRandomPos.getPosTowards
    pub fn default_random_pos_towards(
        &mut self,
        w: &MobWorld,
        h: i32,
        v: i32,
        to: [f64; 3],
        max_rad: f64,
    ) -> Option<[f64; 3]> {
        let (xd, zd) = (to[0] - self.position.x, to[2] - self.position.z);
        self.pick_best(w, |m| {
            let d = m.random_direction_within(0.0, f64::from(h), v, 0, xd, zd, max_rad)?;
            let p = m.toward(f64::from(h), d);
            m.default_check(w, f64::from(h), p).then_some(p)
        })
    }
    fn land_up_out_of_solid(&self, w: &MobWorld, mut p: [i32; 3]) -> Option<[i32; 3]> {
        if w.solid(p[0], p[1], p[2]) {
            p[1] += 1;
            while p[1] < w.world.top_y() && w.solid(p[0], p[1], p[2]) {
                p[1] += 1;
            }
        }
        let water = w.fluid(p[0], p[1], p[2]) == crate::blocks::Fluid::Water;
        (!water && self.malus(w.path_type_static(p[0], p[1], p[2])) == 0.0).then_some(p)
    }
    /// `Entity.getViewVector(0)`: `calculateViewVector(xRotO, yHeadRotO)`.
    pub fn view_vector(&self) -> [f64; 3] {
        use horizon_core::geometry::trig::Mth;
        let to_rad = std::f32::consts::PI / 180.0;
        let x = self.previous_head_pitch * to_rad;
        // LivingEntity.getViewYRot(0): the head's yaw of the last tick.
        let y = -self.previous_head_yaw * to_rad;
        let (y_cos, y_sin, x_cos, x_sin) = (y.mcos(), y.msin(), x.mcos(), x.msin());
        [
            f64::from(y_sin * x_cos),
            f64::from(-x_sin),
            f64::from(y_cos * x_cos),
        ]
    }
    /// `Entity.getLookAngle`: `calculateViewVector(xRot, yRot)`, the facing
    /// of the body (`getHeadLookAngle` is the one with the head's yaw).
    pub fn look_angle(&self) -> [f64; 3] {
        use horizon_core::geometry::trig::Mth;
        let to_rad = std::f32::consts::PI / 180.0;
        let x = self.head_pitch * to_rad;
        let y = -self.yaw * to_rad;
        let (y_cos, y_sin, x_cos, x_sin) = (y.mcos(), y.msin(), x.mcos(), x.msin());
        [
            f64::from(y_sin * x_cos),
            f64::from(-x_sin),
            f64::from(y_cos * x_cos),
        ]
    }
    /// AirAndWaterRandomPos.getPos: within the radians of a direction, kept
    /// near home, moved up out of solid blocks and free of path maluses.
    pub fn air_and_water_random_pos(
        &mut self,
        w: &MobWorld,
        h: i32,
        v: i32,
        fly_height: i32,
        xd: f64,
        zd: f64,
    ) -> Option<[f64; 3]> {
        // Every caller passes `(float) (Math.PI / 2)`.
        let max_rad = f64::from(std::f32::consts::FRAC_PI_2);
        self.pick_best(w, |m| {
            m.air_and_water_pos(w, h, v, fly_height, xd, zd, max_rad)
        })
    }
    /// AirAndWaterRandomPos.generateRandomPos.
    #[allow(clippy::too_many_arguments)]
    fn air_and_water_pos(
        &mut self,
        w: &MobWorld,
        h: i32,
        v: i32,
        fly_height: i32,
        xd: f64,
        zd: f64,
        max_rad: f64,
    ) -> Option<[i32; 3]> {
        let d = self.random_direction_within(0.0, f64::from(h), v, fly_height, xd, zd, max_rad)?;
        let mut p = self.toward(f64::from(h), d);
        if p[1] < w.world.bottom_y() || p[1] >= w.world.top_y() || self.restricted(f64::from(h), p)
        {
            return None;
        }
        // RandomPos.moveUpOutOfSolid
        if w.solid(p[0], p[1], p[2]) {
            p[1] += 1;
            while p[1] < w.world.top_y() && w.solid(p[0], p[1], p[2]) {
                p[1] += 1;
            }
        }
        (self.malus(w.path_type_static(p[0], p[1], p[2])) == 0.0).then_some(p)
    }
    /// AirRandomPos.getPosTowards: an AirAndWaterRandomPos towards a point,
    /// out of water.
    pub fn air_random_pos_towards(
        &mut self,
        w: &MobWorld,
        h: i32,
        v: i32,
        fly_height: i32,
        to: [f64; 3],
        max_rad: f64,
    ) -> Option<[f64; 3]> {
        let (xd, zd) = (to[0] - self.position.x, to[2] - self.position.z);
        self.pick_best(w, |m| {
            let p = m.air_and_water_pos(w, h, v, fly_height, xd, zd, max_rad)?;
            (w.fluid(p[0], p[1], p[2]) != crate::blocks::Fluid::Water).then_some(p)
        })
    }
    /// HoverRandomPos.getPos: a stable spot within the radians of a
    /// direction, raised a random `hover_min..=hover_max` blocks above the
    /// solid ground it is in, out of water and free of path maluses.
    #[allow(clippy::too_many_arguments)]
    pub fn hover_random_pos(
        &mut self,
        w: &MobWorld,
        h: i32,
        v: i32,
        xd: f64,
        zd: f64,
        max_rad: f64,
        hover_max: i32,
        hover_min: i32,
    ) -> Option<[f64; 3]> {
        self.pick_best(w, |m| {
            let d = m.random_direction_within(0.0, f64::from(h), v, 0, xd, zd, max_rad)?;
            // LandRandomPos.generateRandomPosTowardDirection
            let p = m.toward(f64::from(h), d);
            if p[1] < w.world.bottom_y()
                || p[1] >= w.world.top_y()
                || m.restricted(f64::from(h), p)
                || !m.stable_destination(w, p)
            {
                return None;
            }
            let above = m
                .random
                .next_int_bound(hover_max - hover_min + 1)
                .unwrap_or(0)
                + hover_min;
            let p = move_up_to_above_solid(w, p, above);
            let water = w.fluid(p[0], p[1], p[2]) == crate::blocks::Fluid::Water;
            (!water && m.malus(w.path_type_static(p[0], p[1], p[2])) == 0.0).then_some(p)
        })
    }
    /// `WaterAvoidingRandomFlyingGoal.getPosition`: a hover spot ahead, else
    /// any air or water spot ahead.
    pub fn flying_stroll_pos(&mut self, w: &MobWorld) -> Option<[f64; 3]> {
        let [xd, _, zd] = self.view_vector();
        let max_rad = f64::from(std::f32::consts::FRAC_PI_2);
        self.hover_random_pos(w, 8, 7, xd, zd, max_rad, 3, 1)
            .or_else(|| self.air_and_water_random_pos(w, 8, 4, -2, xd, zd))
    }
    /// LandRandomPos.getPos
    pub fn land_random_pos(&mut self, w: &MobWorld, h: i32, v: i32) -> Option<[f64; 3]> {
        self.pick_best(w, |m| {
            let d = m.random_direction(h, v);
            let p = m.toward(f64::from(h), d);
            if p[1] < w.world.bottom_y()
                || p[1] >= w.world.top_y()
                || m.restricted(f64::from(h), p)
                || !m.stable_destination(w, p)
            {
                return None;
            }
            m.land_up_out_of_solid(w, p)
        })
    }
    /// LandRandomPos.getPos(mob, h, v, scorer): the best of ten by `score`.
    pub fn land_random_pos_scored(
        &mut self,
        w: &MobWorld,
        h: i32,
        v: i32,
        score: impl Fn([i32; 3]) -> f64,
    ) -> Option<[f64; 3]> {
        let mut best: Option<[i32; 3]> = None;
        let mut best_weight = f64::NEG_INFINITY;
        for _ in 0..10 {
            let d = self.random_direction(h, v);
            let p = self.toward(f64::from(h), d);
            if p[1] < w.world.bottom_y()
                || p[1] >= w.world.top_y()
                || self.restricted(f64::from(h), p)
                || !self.stable_destination(w, p)
            {
                continue;
            }
            if let Some(p) = self.land_up_out_of_solid(w, p) {
                let v = score(p);
                if v > best_weight {
                    best_weight = v;
                    best = Some(p);
                }
            }
        }
        best.map(|p| {
            [
                f64::from(p[0]) + 0.5,
                f64::from(p[1]),
                f64::from(p[2]) + 0.5,
            ]
        })
    }
    /// `LandRandomPos.getPosAway`: a spot away from `away`, the best of ten tries.
    pub fn land_random_pos_away(
        &mut self,
        w: &MobWorld,
        h: i32,
        v: i32,
        away: [f64; 3],
    ) -> Option<[f64; 3]> {
        self.land_random_pos_away_between(w, 0.0, f64::from(h), v, away)
    }
    /// `LandRandomPos.getPosAway(mob, minHorizontalDist, maxHorizontalDist, verticalDist, avoidPos)`.
    pub fn land_random_pos_away_between(
        &mut self,
        w: &MobWorld,
        min_h: f64,
        max_h: f64,
        v: i32,
        away: [f64; 3],
    ) -> Option<[f64; 3]> {
        let (mut xd, mut zd) = (self.position.x - away[0], self.position.z - away[2]);
        if xd == 0.0 && zd == 0.0 {
            xd = self.random.next_double() - 0.5;
            zd = self.random.next_double() - 0.5;
        }
        self.land_pos_in_direction(w, min_h, max_h, v, xd, zd)
    }
    /// `LandRandomPos.getPosTowards`: a spot within a quarter turn of the
    /// way to `to`, the best of ten tries.
    pub fn land_random_pos_towards(
        &mut self,
        w: &MobWorld,
        h: i32,
        v: i32,
        to: [f64; 3],
    ) -> Option<[f64; 3]> {
        let (xd, zd) = (to[0] - self.position.x, to[2] - self.position.z);
        self.land_pos_in_direction(w, 0.0, f64::from(h), v, xd, zd)
    }
    /// `LandRandomPos.getPosInDirection`: ten tries within a quarter turn
    /// (`(float) (Math.PI / 2)`) of the direction `(xd, zd)`.
    fn land_pos_in_direction(
        &mut self,
        w: &MobWorld,
        min_h: f64,
        max_h: f64,
        v: i32,
        xd: f64,
        zd: f64,
    ) -> Option<[f64; 3]> {
        self.pick_best(w, |m| {
            let d = m.random_direction_within(
                min_h,
                max_h,
                v,
                0,
                xd,
                zd,
                f64::from(std::f32::consts::FRAC_PI_2),
            )?;
            let p = m.toward(max_h, d);
            if p[1] < w.world.bottom_y()
                || p[1] >= w.world.top_y()
                || m.restricted(max_h, p)
                || !m.stable_destination(w, p)
            {
                return None;
            }
            m.land_up_out_of_solid(w, p)
        })
    }
    pub fn target_info(&self, w: &MobWorld) -> Option<TargetInfo> {
        self.ai.target.and_then(|t| w.target_info(t))
    }
    /// Sensing.hasLineOfSight (cached per tick in vanilla).
    pub fn sees(&mut self, w: &MobWorld, who: Who) -> bool {
        if let Some(v) = self.ai.seen_cache.iter().find(|(k, _)| *k == who) {
            return v.1;
        }
        let Some(t) = w.target_info(who) else {
            return false;
        };
        let eye = Vec3::new(self.position.x, self.eye_y(), self.position.z);
        let v = w.line_of_sight_for(
            Some(self.position.y),
            eye,
            Vec3::new(t.position.x, t.eye_y, t.position.z),
        );
        self.ai.seen_cache.push((who, v));
        v
    }
    /// Mob.isWithinMeleeAttackRange: the box inflated by sqrt(2.04) - 0.6, or
    /// for an active item with `attack_range` (the spears) by its
    /// `effectiveMaxRange` for a mob (`max_reach * mob_factor`), and not
    /// within its `effectiveMinRange` (`min_reach * mob_factor`).
    pub fn within_melee_range(&self, w: &MobWorld, t: &TargetInfo) -> bool {
        self.melee_reaches(w, t.hitbox)
    }
    /// [`Self::within_melee_range`] for a target's box (`getHitbox`).
    pub fn melee_reaches(&self, w: &MobWorld, target: Aabb) -> bool {
        // getVehicle().getBoundingBox() now: a mob vehicle's box as the
        // snapshot has it after its own tick, else the box positionRider
        // left for this same vehicle (a boat, a minecart, primed TNT).
        let vehicle = self.extra.mount.and_then(|v| {
            w.seen(v)
                .map(|s| s.bounds)
                .or_else(|| self.extra.seat.filter(|s| s.0 == v).map(|s| s.2))
        });
        self.melee_reaches_beside(vehicle, target)
    }
    /// [`Self::melee_reaches`] with the vehicle's box given.
    pub fn melee_reaches_beside(&self, vehicle: Option<Aabb>, target: Aabb) -> bool {
        let (max, min) = match self.active_attack_range() {
            Some((max, min)) => (f64::from(max), f64::from(min)),
            None => (2.04f32.sqrt() as f64 - 0.6, 0.0),
        };
        let reaches = |range: f64| self.attack_box(range, vehicle).intersects(target);
        reaches(max) && (min <= 0.0 || !reaches(min))
    }
    /// The active item's `attack_range` for a mob: `effectiveMaxRange` and
    /// `effectiveMinRange` (`max_reach` and `min_reach` times `mob_factor`).
    /// A patch on the held stack wins (`!minecraft:attack_range` removes it;
    /// the codec's defaults are 0, 3 and a factor of 1); else the spears'.
    fn active_attack_range(&self) -> Option<(f32, f32)> {
        let item = self.active_item()?;
        let patch = (self.extra.main_hand.as_deref() == Some(item))
            .then(|| self.extra.main_hand_stack().and_then(|s| s.data()))
            .flatten();
        if patch.is_some_and(|d| d.other("!minecraft:attack_range").is_some()) {
            return None;
        }
        if let Some(range) = patch.and_then(|d| d.other("minecraft:attack_range")) {
            let float = |key: &str, default: f32| match range.get(key) {
                Some(horizon_world::nbt::Tag::Float(bits)) => f32::from_bits(*bits),
                Some(horizon_world::nbt::Tag::Double(bits)) => f64::from_bits(*bits) as f32,
                Some(horizon_world::nbt::Tag::Int(v)) => *v as f32,
                _ => default,
            };
            let factor = float("mob_factor", 1.0);
            return Some((
                float("max_reach", 3.0) * factor,
                float("min_reach", 0.0) * factor,
            ));
        }
        crate::spear::Kinetic::of(item).map(|_| {
            let r = &crate::spear::REACH;
            (r.max * r.mob_factor, r.min * r.mob_factor)
        })
    }
    /// `LivingEntity.getActiveItem`: the item being used, else the main hand's.
    pub fn active_item(&self) -> Option<&str> {
        if super::spear_use::is_using_item(self) && self.extra.using_off_hand {
            self.extra.off_hand.as_deref()
        } else {
            self.extra.main_hand.as_deref()
        }
    }
    /// Whether the species is a `TamableAnimal` (not the horses and llamas,
    /// which are tamed another way).
    pub fn is_tamable_animal(&self) -> bool {
        matches!(
            self.species(),
            Species::Wolf
                | Species::Cat
                | Species::Parrot
                | Species::Nautilus
                | Species::ZombieNautilus
        )
    }
    /// `IronGolem.isPlayerCreated`: a golem a player built, which
    /// `IronGolem.canAttack` never lets attack a player.
    pub fn built_by_player(&self) -> bool {
        self.species() == Species::IronGolem && self.extra.player_created
    }
    /// `canAttack` for the player: a golem a player built never attacks
    /// players (`IronGolem.canAttack`), and a pet never attacks its owner
    /// (`TamableAnimal.canAttack`: `isOwnedBy`). In Peaceful nothing attacks a
    /// player (`LivingEntity.canAttack`, `TargetingConditions.test`: combat
    /// conditions fail when the difficulty is PEACEFUL).
    pub fn spares_players(&self, w: &MobWorld) -> bool {
        (w.difficulty.base == 0 && w.player.is_some())
            || self.built_by_player()
            || (self.is_tamable_animal()
                && self.extra.tame
                && w.player
                    .as_ref()
                    .is_some_and(|p| super::tame::owned_by(self.extra.owner, p.uuid)))
    }
    /// `canAttack` of any target: the player rules above, and
    /// `IronGolem.canAttack`'s refusal of creepers. The callers check that
    /// the target is alive and can be seen as an enemy.
    pub fn spares_target(&self, w: &MobWorld, who: Who) -> bool {
        // Dolphin.canAttack: a baby dolphin attacks nobody, so whatever hurt it
        // is never taken as its target.
        if self.species() == Species::Dolphin && self.is_baby() {
            return true;
        }
        match who {
            Who::Player => self.spares_players(w),
            Who::Mob(id) => {
                // `Mob.canAttack` refuses a Ghast for every mob.
                w.seen(id).is_some_and(|s| s.species == Species::Ghast)
                    || (self.species() == Species::IronGolem
                        && w.seen(id).is_some_and(|s| s.species == Species::Creeper))
            }
        }
    }
    /// TargetingConditions.forCombat().test for the player: alive, not
    /// creative, within range scaled by visibility, optionally visible.
    pub fn can_target_player(&mut self, w: &MobWorld, range: f64, check_sight: bool) -> bool {
        let Some(p) = &w.player else { return false };
        if self.spares_players(w) {
            return false;
        }
        // TargetingConditions.isCombat: never an ally.
        if p.creative || p.allies.contains(&self.id) {
            return false;
        }
        self.player_in_range(w, range, check_sight, true)
    }
    /// forCombat().ignoreInvisibilityTesting() for the player: the check for
    /// a target the mob already has, which ignores the visibility scaling.
    pub fn can_keep_target_player(&mut self, w: &MobWorld, range: f64) -> bool {
        let Some(p) = &w.player else { return false };
        if self.spares_players(w) {
            return false;
        }
        if p.creative || p.allies.contains(&self.id) {
            return false;
        }
        self.player_in_range(w, range, true, false)
    }
    /// TargetingConditions.forNonCombat().range(range).test for the player
    /// (seen by anyone, within range scaled by visibility, in sight).
    pub fn can_see_player(&mut self, w: &MobWorld, range: f64) -> bool {
        self.player_in_range(w, range, true, true)
    }
    /// `TargetingConditions.forNonCombat().range(range).ignoreLineOfSight()`
    /// for the player (still scaled by visibility).
    pub fn can_see_player_through_walls(&mut self, w: &MobWorld, range: f64) -> bool {
        self.player_in_range(w, range, false, true)
    }
    /// `scale` is `TargetingConditions.testInvisible`: scale the range by
    /// the player's visibility.
    fn player_in_range(
        &mut self,
        w: &MobWorld,
        range: f64,
        check_sight: bool,
        scale: bool,
    ) -> bool {
        let Some(p) = &w.player else { return false };
        if !p.alive || p.spectator {
            return false;
        }
        // LivingEntity.getVisibilityPercent, including worn mob_visibility.
        let visibility = if scale {
            p.visibility_percent(Some(self.kind.id.as_str()), w.items)
        } else {
            1.0
        };
        let dist = (range * visibility).max(2.0);
        if self.distance_sqr(p.position) > dist * dist {
            return false;
        }
        !check_sight || self.sees(w, Who::Player)
    }
}

// --- goals ---------------------------------------------------------------

/// FloatGoal: jump (swim up) while deeper than the jump threshold or in lava.
#[derive(Clone, Debug)]
pub struct Float;
impl Goal for Float {
    goal_clone!();
    fn flags(&self) -> u8 {
        JUMP
    }
    fn can_use(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
        m.floats()
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, m: &mut Mob, _: &mut Cx) {
        if m.random.next_float() < 0.8 {
            m.ai.jump = true;
        }
    }
}
/// `ClimbOnTopOfPowderSnowGoal`: a powder-snow walker standing in powder snow
/// jumps while the block above is powder snow or has no collision.
#[derive(Clone, Debug)]
pub struct ClimbOnTopOfPowderSnow;
impl Goal for ClimbOnTopOfPowderSnow {
    goal_clone!();
    fn flags(&self) -> u8 {
        JUMP
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        // wasInPowderSnow || isInPowderSnow: the freeze tick has read the
        // powder snow the mob's box touches before the goals run.
        if !super::ai::in_powder_snow(m, cx.w)
            || !cx
                .w
                .data
                .entity_has("minecraft:powder_snow_walkable_mobs", &m.kind.id)
        {
            return false;
        }
        // Mob.blockPosition().above()
        let above = [
            m.position.x.floor() as i32,
            m.position.y.floor() as i32 + 1,
            m.position.z.floor() as i32,
        ];
        cx.w.block_name(above[0], above[1], above[2]) == "minecraft:powder_snow"
            || cx.w.collision_bounds(above).is_none()
    }
    fn every_tick(&self) -> bool {
        true
    }
    /// `ClimbOnTopOfPowderSnowGoal.tick`: ask the mob to jump.
    fn tick(&mut self, m: &mut Mob, _: &mut Cx) {
        m.ai.jump = true;
    }
}
/// PanicGoal: flee to a random position after panic-causing damage (or
/// towards water while burning).
#[derive(Clone, Debug)]
pub struct Panic {
    pub speed: f64,
    /// Only the damage of `#panic_environmental_causes` (an armadillo).
    pub environmental: bool,
    /// `TurtlePanicGoal`: always look for water this far away, not only
    /// when on fire.
    pub water_range: Option<i32>,
    pos: [f64; 3],
}
impl Panic {
    pub fn new(speed: f64) -> Self {
        Self {
            speed,
            environmental: false,
            water_range: None,
            pos: [0.0; 3],
        }
    }
    /// `TurtlePanicGoal`.
    pub fn looking_for_water(mut self, range: i32) -> Self {
        self.water_range = Some(range);
        self
    }
}
pub fn should_panic(m: &Mob) -> bool {
    m.last_damage
        .is_some_and(|(d, t)| d.causes_panic() && i64::from(m.age) - t <= 40)
        || m.fire_ticks > 0 && !m.kind.flags.fire_immune
}
impl Goal for Panic {
    goal_clone!();
    fn flags(&self) -> u8 {
        MOVE
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        let wanted = if self.environmental {
            m.last_damage.is_some_and(|(d, t)| {
                super::armadillo::environmental(d.kind) && i64::from(m.age) - t <= 40
            }) || m.fire_ticks > 0 && !m.kind.flags.fire_immune
        } else {
            should_panic(m)
        };
        if !wanted {
            return false;
        }
        let range = self.water_range.or((m.fire_ticks > 0).then_some(5));
        if let Some(range) = range {
            let (bx, by, bz) = (
                m.position.x.floor() as i32,
                m.position.y.floor() as i32,
                m.position.z.floor() as i32,
            );
            if cx.w.data.state(cx.w.world.block(bx, by, bz)).top == 0.0 {
                // findBlocksInBoxByManhattanDistance(pos, range, 1) for water.
                let mut best: Option<([i32; 3], i32)> = None;
                for dy in -1i32..=1 {
                    for dx in -range..=range {
                        for dz in -range..=range {
                            let d = dx.abs() + dy.abs() + dz.abs();
                            if cx.w.fluid(bx + dx, by + dy, bz + dz) == crate::blocks::Fluid::Water
                                && best.is_none_or(|b| d < b.1)
                            {
                                best = Some(([bx + dx, by + dy, bz + dz], d));
                            }
                        }
                    }
                }
                if let Some((p, _)) = best {
                    self.pos = [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])];
                    return true;
                }
            }
        }
        match m.default_random_pos(cx.w, 5, 4) {
            Some(p) => {
                self.pos = p;
                true
            }
            None => false,
        }
    }
    fn can_continue(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
        !m.ai.nav.is_done()
    }
    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        m.nav_move_to(cx.w, self.pos, self.speed);
        m.ai.panicking = true;
    }
    fn stop(&mut self, m: &mut Mob, _: &mut Cx) {
        m.ai.panicking = false;
    }
}
/// RandomStrollGoal / WaterAvoidingRandomStrollGoal.
#[derive(Clone, Debug)]
pub struct Stroll {
    pub speed: f64,
    pub interval: i32,
    pub check_no_action: bool,
    /// WaterAvoidingRandomStrollGoal probability of a plain stroll.
    pub water_avoiding: Option<f32>,
    /// RandomSwimmingGoal: BehaviorUtils.getRandomSwimmablePos.
    pub swim: bool,
    /// WaterAvoidingRandomFlyingGoal: a hover spot, else an air or water pos.
    pub flying: bool,
    /// `Parrot.ParrotWanderGoal`: to a tree, else as `flying`.
    pub parrot: bool,
    pub force: bool,
    wanted: [f64; 3],
    /// A subclass's `getPosition` in place of the default pick
    /// (`GolemRandomStrollInVillageGoal`, `MoveBackToVillageGoal`).
    position: Option<fn(&mut Mob, &mut Cx) -> Option<[f64; 3]>>,
    /// A subclass's own `canUse` test, run before the interval's draw.
    gate: Option<fn(&Mob, &MobWorld) -> bool>,
    label: Option<&'static str>,
}
impl Stroll {
    /// `RandomStrollGoal.<init>` (interval 120, checks the no-action time).
    pub fn new(speed: f64) -> Self {
        Self {
            speed,
            interval: 120,
            check_no_action: true,
            water_avoiding: None,
            swim: false,
            flying: false,
            parrot: false,
            force: false,
            wanted: [0.0; 3],
            position: None,
            gate: None,
            label: None,
        }
    }
    /// The stroll's spot comes from `f` (an overridden `getPosition`).
    pub fn position(mut self, f: fn(&mut Mob, &mut Cx) -> Option<[f64; 3]>) -> Self {
        self.position = Some(f);
        self
    }
    /// The stroll may only start while `f` holds (an overridden `canUse`).
    pub fn gate(mut self, f: fn(&Mob, &MobWorld) -> bool) -> Self {
        self.gate = Some(f);
        self
    }
    /// The goal's class name, for debugging.
    pub fn named(mut self, label: &'static str) -> Self {
        self.label = Some(label);
        self
    }
    pub fn avoiding_water(speed: f64) -> Self {
        Self {
            water_avoiding: Some(0.001),
            ..Self::new(speed)
        }
    }
    pub fn interval(mut self, i: i32) -> Self {
        self.interval = i;
        self
    }
    /// WaterAvoidingRandomFlyingGoal (a nearby air position).
    pub fn flying(speed: f64) -> Self {
        Self {
            flying: true,
            ..Self::new(speed)
        }
    }
    /// `Parrot.ParrotWanderGoal(speed)`, a WaterAvoidingRandomFlyingGoal.
    pub fn parrot(speed: f64) -> Self {
        Self {
            water_avoiding: Some(0.001),
            parrot: true,
            ..Self::new(speed)
        }
    }
    /// RandomSwimmingGoal(speed, interval)
    pub fn swimming(speed: f64, interval: i32) -> Self {
        Self {
            swim: true,
            ..Self::new(speed).interval(interval)
        }
    }
}
impl Goal for Stroll {
    goal_clone!();
    fn name(&self) -> &'static str {
        if let Some(label) = self.label {
            label
        } else if self.parrot {
            "ParrotWander"
        } else if self.water_avoiding.is_some() {
            "Stroll"
        } else if self.swim {
            "RandomSwimming"
        } else {
            "RandomStroll"
        }
    }
    fn flags(&self) -> u8 {
        MOVE
    }
    /// RandomStrollGoal.canUse.
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if super::riders::mob_controlled(m, cx.w) {
            return false;
        }
        if self.gate.is_some_and(|gate| !gate(m, cx.w)) {
            return false;
        }
        if !self.force {
            if self.check_no_action && m.ai.no_action_time >= 100 {
                return false;
            }
            if m.random.next_int_bound(reduced(self.interval)).unwrap_or(1) != 0 {
                return false;
            }
        }
        let pos = match self.water_avoiding {
            _ if self.position.is_some() => self.position.and_then(|pick| pick(m, cx)),
            Some(_) if self.parrot => super::parrot::wander_pos(m, cx.w),
            Some(prob) => {
                if m.fluids.in_water {
                    m.land_random_pos(cx.w, 15, 7)
                        .or_else(|| m.default_random_pos(cx.w, 10, 7))
                } else if m.random.next_float() >= prob {
                    m.land_random_pos(cx.w, 10, 7)
                } else {
                    m.default_random_pos(cx.w, 10, 7)
                }
            }
            None if self.swim => {
                let mut p = m.default_random_pos(cx.w, 10, 7);
                let mut count = 0;
                while let Some(q) = p {
                    let s = cx.w.world.block(
                        q[0].floor() as i32,
                        q[1].floor() as i32,
                        q[2].floor() as i32,
                    );
                    if cx.w.data.state(s).flags & super::data::WATER != 0 || count >= 10 {
                        break;
                    }
                    count += 1;
                    p = m.default_random_pos(cx.w, 10, 7);
                }
                p
            }
            None if self.flying => m.flying_stroll_pos(cx.w),
            None => m.default_random_pos(cx.w, 10, 7),
        };
        let Some(p) = pos else { return false };
        self.wanted = p;
        self.force = false;
        true
    }
    /// RandomStrollGoal.canContinueToUse.
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        !m.ai.nav.is_done() && !super::riders::mob_controlled(m, cx.w)
    }
    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        m.nav_move_to(cx.w, self.wanted, self.speed);
    }
    fn stop(&mut self, m: &mut Mob, _: &mut Cx) {
        m.nav_stop();
    }
}
/// LookAtPlayerGoal (player targets only).
#[derive(Clone, Debug)]
pub struct LookAtPlayer {
    pub range: f32,
    pub probability: f32,
    pub only_horizontal: bool,
    /// `LookAtPlayerGoal(Mob.class, ...)`: the nearest mob, not the player.
    pub mobs: bool,
    /// `LookAtPlayerGoal(Strider.class, ...)`: only mobs of this species.
    pub only: Option<Species>,
    /// InteractGoal: LOOK and MOVE.
    pub interact: bool,
    look_at: Option<Who>,
    look_time: i32,
}
impl LookAtPlayer {
    pub fn new(range: f32) -> Self {
        Self {
            range,
            probability: 0.02,
            only_horizontal: false,
            mobs: false,
            only: None,
            interact: false,
            look_at: None,
            look_time: 0,
        }
    }
    /// `LookAtPlayerGoal(this, Mob.class, range)`.
    pub fn mobs(range: f32) -> Self {
        Self {
            mobs: true,
            ..Self::new(range)
        }
    }
    /// `LookAtPlayerGoal(this, <species class>, range)`.
    pub fn species(species: Species, range: f32) -> Self {
        Self {
            only: Some(species),
            ..Self::mobs(range)
        }
    }
    /// `InteractGoal(this, Player.class, range, probability)`.
    pub fn interact(range: f32, probability: f32) -> Self {
        Self {
            interact: true,
            ..Self::new(range).probability(probability)
        }
    }
    pub fn probability(mut self, p: f32) -> Self {
        self.probability = p;
        self
    }
    /// The goal's look target, alive, with its position and eye height.
    fn look_target(&self, w: &MobWorld) -> Option<(Vec3, f64)> {
        match self.look_at? {
            Who::Player => w
                .player
                .as_ref()
                .filter(|p| p.alive)
                .map(|p| (p.position, p.eye[1])),
            Who::Mob(id) => w
                .seen(id)
                .filter(|s| s.alive)
                .map(|s| (s.position, s.eye_y)),
        }
    }
}
impl Goal for LookAtPlayer {
    goal_clone!();
    fn flags(&self) -> u8 {
        if self.interact { LOOK | MOVE } else { LOOK }
    }
    fn name(&self) -> &'static str {
        if self.interact {
            "InteractGoal"
        } else {
            "LookAtPlayer"
        }
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if m.random.next_float() >= self.probability {
            return false;
        }
        self.look_at = None;
        let range = f64::from(self.range);
        if !self.mobs {
            // level.getNearestPlayer(forNonCombat().range(lookDistance)).
            if m.can_see_player(cx.w, range) {
                self.look_at = Some(Who::Player);
            }
            return self.look_at.is_some();
        }
        // level.getNearestEntity(mobs in the box inflated by (range, 3, range)).
        let area = m.bounds().inflate(Vec3::new(range, 3.0, range));
        let (id, only) = (m.id, self.only);
        let at = Vec3::new(m.position.x, m.eye_y(), m.position.z);
        let candidates: Vec<(u32, Vec3)> =
            cx.w.others_near(area)
                .filter(|s| s.id != id && s.alive && s.bounds.intersects(area))
                .filter(|s| only.is_none_or(|o| s.species == o))
                .map(|s| (s.id, s.position))
                .collect();
        let mut best: Option<(u32, f64)> = None;
        for (cid, pos) in candidates {
            let reach = range.max(2.0);
            if m.distance_sqr(pos) > reach * reach || !m.sees(cx.w, Who::Mob(cid)) {
                continue;
            }
            let d = pos.distance_squared(at);
            if best.is_none_or(|(_, b)| d < b) {
                best = Some((cid, d));
            }
        }
        self.look_at = best.map(|(cid, _)| Who::Mob(cid));
        self.look_at.is_some()
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        let Some((pos, _)) = self.look_target(cx.w) else {
            return false;
        };
        m.distance_sqr(pos) <= f64::from(self.range * self.range) && self.look_time > 0
    }
    fn start(&mut self, m: &mut Mob, _: &mut Cx) {
        self.look_time = reduced(40 + m.random.next_int_bound(40).unwrap_or(0));
    }
    fn stop(&mut self, _: &mut Mob, _: &mut Cx) {
        self.look_at = None;
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        if let Some((pos, eye)) = self.look_target(cx.w) {
            let y = if self.only_horizontal { m.eye_y() } else { eye };
            m.look_at([pos.x, y, pos.z]);
            self.look_time -= 1;
        }
    }
}
/// RandomLookAroundGoal.
#[derive(Clone, Debug, Default)]
pub struct RandomLookAround {
    rel: [f64; 2],
    time: i32,
}
impl Goal for RandomLookAround {
    goal_clone!();
    fn flags(&self) -> u8 {
        MOVE | LOOK
    }
    fn can_use(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
        m.random.next_float() < 0.02
    }
    fn can_continue(&mut self, _: &mut Mob, _: &mut Cx) -> bool {
        self.time >= 0
    }
    fn start(&mut self, m: &mut Mob, _: &mut Cx) {
        let r = std::f64::consts::TAU * m.random.next_double();
        self.rel = [jmath::cos(r), jmath::sin(r)];
        self.time = 20 + m.random.next_int_bound(20).unwrap_or(0);
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, m: &mut Mob, _: &mut Cx) {
        self.time -= 1;
        let at = [
            m.position.x + self.rel[0],
            m.eye_y(),
            m.position.z + self.rel[1],
        ];
        m.look_at(at);
    }
}
/// MeleeAttackGoal (and ZombieAttackGoal's raised arms when `zombie`).
#[derive(Clone, Debug)]
pub struct MeleeAttack {
    pub speed: f64,
    pub follow_unseen: bool,
    pub zombie: bool,
    /// `PolarBear.PolarBearMeleeAttackGoal`: rears up before each strike.
    pub polar_bear: bool,
    path_target: [f64; 3],
    recalc: i32,
    pub until_attack: i32,
    last_check: i64,
    raise_arm: i32,
    /// Ticks between attacks (20 unless set).
    pub interval: i32,
    /// Ticks between attacks for a baby, when they differ.
    pub baby_interval: Option<i32>,
    /// The sound played after each strike (a fox's bite).
    pub bite: Option<&'static str>,
}
impl MeleeAttack {
    pub fn new(speed: f64, follow_unseen: bool) -> Self {
        Self {
            speed,
            follow_unseen,
            zombie: false,
            polar_bear: false,
            path_target: [0.0; 3],
            recalc: 0,
            until_attack: 0,
            last_check: 0,
            raise_arm: 0,
            interval: 20,
            baby_interval: None,
            bite: None,
        }
    }
    pub fn zombie(mut self) -> Self {
        self.zombie = true;
        self
    }
    pub fn polar_bear(mut self) -> Self {
        self.polar_bear = true;
        self
    }
    pub fn interval(mut self, ticks: i32) -> Self {
        self.interval = ticks;
        self
    }
    /// `MeleeAttack.create(adult)` for adults, `create(baby)` for babies.
    pub fn intervals(mut self, adult: i32, baby: i32) -> Self {
        self.interval = adult;
        self.baby_interval = Some(baby);
        self
    }
    /// `FoxMeleeAttackGoal.checkAndPerformAttack`: the bite after each strike.
    pub fn bite(mut self, sound: &'static str) -> Self {
        self.bite = Some(sound);
        self
    }
}
impl Goal for MeleeAttack {
    goal_clone!();
    fn flags(&self) -> u8 {
        MOVE | LOOK
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if cx.w.game_time - self.last_check < 20 {
            return false;
        }
        self.last_check = cx.w.game_time;
        let Some(t) = m.target_info(cx.w) else {
            return false;
        };
        if !t.alive {
            return false;
        }
        let path = m.create_path_to_entity(cx.w, [t.position.x, t.position.y, t.position.z], 0);
        let ok = path.is_some() || m.within_melee_range(cx.w, &t);
        m.ai.pending_path = path;
        ok
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        let Some(t) = m.target_info(cx.w) else {
            return false;
        };
        if !t.alive {
            return false;
        }
        if !self.follow_unseen {
            return !m.ai.nav.is_done();
        }
        // `MeleeAttackGoal.canContinueToUse`'s follow branch re-checks
        // `isWithinHome(target.blockPosition())` and refuses a spectator or
        // creative player.
        let target_block = [
            t.position.x.floor() as i32,
            t.position.y.floor() as i32,
            t.position.z.floor() as i32,
        ];
        m.ai.home
            .is_none_or(|(center, radius)| within_home(center, radius, target_block))
            && !(t.is_player && (t.creative || cx.w.player.as_ref().is_some_and(|p| p.spectator)))
    }
    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        let path = m.ai.pending_path.take();
        m.nav_move_to_path(cx.w, path, self.speed);
        m.ai.aggressive = true;
        self.recalc = 0;
        self.until_attack = 0;
        self.raise_arm = 0;
    }
    fn stop(&mut self, m: &mut Mob, cx: &mut Cx) {
        if let Some(t) = m.target_info(cx.w)
            && t.is_player
            && t.creative
        {
            m.ai.target = None;
        }
        m.ai.aggressive = false;
        m.nav_stop();
        if self.polar_bear {
            super::polar_bear::set_standing(m, false);
        }
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let Some(who) = m.ai.target else { return };
        let Some(t) = cx.w.target_info(who) else {
            return;
        };
        m.look_at_speed([t.position.x, t.eye_y, t.position.z], 30.0, 30.0);
        self.recalc = (self.recalc - 1).max(0);
        let moved = (t.position.x - self.path_target[0]).powi(2)
            + (t.position.y - self.path_target[1]).powi(2)
            + (t.position.z - self.path_target[2]).powi(2)
            >= 1.0;
        if (self.follow_unseen || m.sees(cx.w, who))
            && self.recalc <= 0
            && (self.path_target == [0.0; 3] || moved || m.random.next_float() < 0.05)
        {
            self.path_target = [t.position.x, t.position.y, t.position.z];
            self.recalc = 4 + m.random.next_int_bound(7).unwrap_or(0);
            let d = m.distance_sqr(t.position);
            if d > 1024.0 {
                self.recalc += 10;
            } else if d > 256.0 {
                self.recalc += 5;
            }
            if !m.nav_move_to_entity(cx.w, self.path_target, 0, self.speed) {
                self.recalc += 15;
            }
            self.recalc = adjusted(true, self.recalc);
        }
        self.until_attack = (self.until_attack - 1).max(0);
        if self.polar_bear {
            let cooldown = adjusted(true, self.interval);
            super::polar_bear::check_and_perform_attack(
                &mut self.until_attack,
                cooldown,
                m,
                who,
                &t,
                cx,
            );
        } else if self.until_attack <= 0 && m.within_melee_range(cx.w, &t) && m.sees(cx.w, who) {
            let interval = match self.baby_interval {
                Some(baby) if m.is_baby() => baby,
                _ => self.interval,
            };
            self.until_attack = adjusted(true, interval);
            m.swing();
            super::ai::do_hurt_target(m, who, cx);
            if let Some(sound) = self.bite {
                let at = [m.position.x, m.position.y, m.position.z];
                cx.events.push(MobEvent::Sound(sound, at, 1.0, 1.0));
            }
        }
        if self.zombie {
            self.raise_arm += 1;
            m.ai.aggressive = self.raise_arm >= 5 && self.until_attack < adjusted(true, 20) / 2;
        }
    }
}
/// LeapAtTargetGoal.
#[derive(Clone, Debug)]
pub struct LeapAtTarget {
    pub yd: f32,
    target: [f64; 3],
}
impl LeapAtTarget {
    pub fn new(yd: f32) -> Self {
        Self {
            yd,
            target: [0.0; 3],
        }
    }
}
impl Goal for LeapAtTarget {
    goal_clone!();
    fn flags(&self) -> u8 {
        JUMP | MOVE
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        let Some(t) = m.target_info(cx.w) else {
            return false;
        };
        let d = m.distance_sqr(t.position);
        if !(4.0..=16.0).contains(&d) || !m.on_ground {
            return false;
        }
        self.target = [t.position.x, t.position.y, t.position.z];
        m.random.next_int_bound(reduced(5)).unwrap_or(1) == 0
    }
    fn can_continue(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
        !m.on_ground
    }
    fn start(&mut self, m: &mut Mob, _: &mut Cx) {
        let mut d = [
            self.target[0] - m.position.x,
            0.0,
            self.target[2] - m.position.z,
        ];
        let len_sq = d[0] * d[0] + d[2] * d[2];
        if len_sq > 1.0e-7 {
            let l = len_sq.sqrt();
            d = [
                d[0] / l * 0.4 + m.velocity.x * 0.2,
                0.0,
                d[2] / l * 0.4 + m.velocity.z * 0.2,
            ];
        }
        m.velocity = Vec3::new(d[0], f64::from(self.yd), d[2]);
    }
}
/// What a target goal hunts.
#[derive(Clone, Debug)]
pub enum Prey {
    Player,
    Species(Vec<Species>),
    /// Any mob matching a predicate on its snapshot.
    Filter(fn(&Seen) -> bool),
}
/// NearestAttackableTargetGoal (+ TargetGoal's continue rules).
#[derive(Clone, Debug)]
pub struct NearestTarget {
    pub prey: Prey,
    pub must_see: bool,
    pub must_reach: bool,
    pub interval: i32,
    /// Extra condition on the attacker, checked after the random interval.
    pub when: Option<fn(&Mob, &MobWorld) -> bool>,
    /// Mobs closer than this squared distance are not picked
    /// (`GuardianAttackSelector`: `distanceToSqr > 9`).
    pub beyond_sqr: Option<f64>,
    /// A condition checked before anything else, by subclasses overriding
    /// `canUse` (SpiderTargetGoal: only in the dark).
    pub gate: Option<fn(&Mob, &MobWorld) -> bool>,
    /// The vanilla class name of a subclass (`Goal::name`).
    pub label: Option<&'static str>,
    found: Option<Who>,
    target_goal: TargetState,
}
#[derive(Clone, Debug)]
pub struct TargetState {
    unseen: i32,
    pub unseen_memory: i32,
    /// `TargetGoal.getFollowDistance` as a fraction of the follow range.
    pub follow_scale: f64,
}
impl Default for TargetState {
    fn default() -> Self {
        Self {
            unseen: 0,
            unseen_memory: 0,
            follow_scale: 1.0,
        }
    }
}
impl NearestTarget {
    pub fn new(prey: Prey, must_see: bool) -> Self {
        Self {
            prey,
            must_see,
            must_reach: false,
            interval: reduced(10),
            when: None,
            beyond_sqr: None,
            gate: None,
            label: None,
            found: None,
            target_goal: TargetState {
                unseen_memory: 60,
                ..Default::default()
            },
        }
    }
    pub fn player(must_see: bool) -> Self {
        Self::new(Prey::Player, must_see)
    }
    pub fn species(list: &[Species], must_see: bool) -> Self {
        Self::new(Prey::Species(list.to_vec()), must_see)
    }
    pub fn interval(mut self, i: i32) -> Self {
        self.interval = reduced(i);
        self
    }
    /// `TargetGoal.setUnseenMemoryTicks`.
    pub fn unseen_memory(mut self, ticks: i32) -> Self {
        self.target_goal.unseen_memory = ticks;
        self
    }
    /// Scale the follow distance (llamas hunt wolves within a quarter of it).
    pub fn follow_scale(mut self, scale: f64) -> Self {
        self.target_goal.follow_scale = scale;
        self
    }
    pub fn reach(mut self) -> Self {
        self.must_reach = true;
        self
    }
    pub fn when(mut self, f: fn(&Mob, &MobWorld) -> bool) -> Self {
        self.when = Some(f);
        self
    }
    pub fn beyond(mut self, distance_sqr: f64) -> Self {
        self.beyond_sqr = Some(distance_sqr);
        self
    }
    pub fn gate(mut self, f: fn(&Mob, &MobWorld) -> bool) -> Self {
        self.gate = Some(f);
        self
    }
    /// A subclass's name, e.g. `NearestAttackableWitchTargetGoal`.
    pub fn named(mut self, label: &'static str) -> Self {
        self.label = Some(label);
        self
    }
}
/// `AbstractIllager.canAttack`: a baby `AbstractVillager` is never attacked.
fn illager_spares(attacker: Species, target: &Seen) -> bool {
    matches!(
        attacker,
        Species::Pillager | Species::Vindicator | Species::Evoker | Species::Illusioner
    ) && target.baby
        && matches!(target.species, Species::Villager | Species::WanderingTrader)
}
/// TargetGoal.canContinueToUse
pub fn target_continue(st: &mut TargetState, must_see: bool, m: &mut Mob, cx: &mut Cx) -> bool {
    let Some(who) = m.ai.target else { return false };
    if let Who::Mob(id) = who
        && cx
            .w
            .seen(id)
            .is_some_and(|s| illager_spares(m.species(), s))
    {
        return false;
    }
    let Some(t) = cx.w.target_info(who) else {
        return false;
    };
    if !t.alive
        || !t.can_be_seen_as_enemy
        || (t.is_player && t.creative)
        || m.spares_target(cx.w, who)
    {
        return false;
    }
    let follow = m.attrs.follow_range * st.follow_scale;
    if m.distance_sqr(t.position) > follow * follow {
        return false;
    }
    if must_see {
        if m.sees(cx.w, who) {
            st.unseen = 0;
        } else {
            st.unseen += 1;
            if st.unseen > reduced(st.unseen_memory) {
                return false;
            }
        }
    }
    true
}
impl Goal for NearestTarget {
    goal_clone!();
    fn name(&self) -> &'static str {
        self.label.unwrap_or("NearestTarget")
    }
    fn flags(&self) -> u8 {
        TARGET
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if let Some(f) = self.gate
            && !f(m, cx.w)
        {
            return false;
        }
        if self.interval > 0 && m.random.next_int_bound(self.interval).unwrap_or(1) != 0 {
            return false;
        }
        if let Some(f) = self.when
            && !f(m, cx.w)
        {
            return false;
        }
        let range = m.attrs.follow_range * self.target_goal.follow_scale;
        self.found = None;
        // NearestAttackableTargetGoal.findTarget: TargetingConditions.forCombat()
        // always checks the line of sight; mustSee and mustReach only matter
        // to TargetGoal.canContinueToUse and canAttack.
        match &self.prey {
            Prey::Player => {
                if m.can_target_player(cx.w, range, true) {
                    self.found = Some(Who::Player);
                }
            }
            prey => {
                let pos = Vec3::new(m.position.x, m.eye_y(), m.position.z);
                let id = m.id;
                let attacker = m.species();
                let candidates: Vec<(u32, Vec3)> =
                    cx.w.nearby(pos, range, move |s| {
                        s.id != id
                            && !s.playing_dead
                            && !illager_spares(attacker, s)
                            && match prey {
                                Prey::Species(list) => list.contains(&s.species),
                                Prey::Filter(f) => f(s),
                                Prey::Player => false,
                            }
                    })
                    .map(|s| (s.id, s.position))
                    .collect();
                for (cid, cpos) in candidates {
                    if m.distance_sqr(cpos) > range * range {
                        continue;
                    }
                    if self.beyond_sqr.is_some_and(|d| m.distance_sqr(cpos) <= d) {
                        continue;
                    }
                    if !m.sees(cx.w, Who::Mob(cid)) {
                        continue;
                    }
                    self.found = Some(Who::Mob(cid));
                    break;
                }
            }
        }
        self.found.is_some()
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        target_continue(&mut self.target_goal, self.must_see, m, cx)
    }
    fn start(&mut self, m: &mut Mob, _: &mut Cx) {
        m.ai.target = self.found;
        self.target_goal.unseen = 0;
    }
    fn stop(&mut self, m: &mut Mob, _: &mut Cx) {
        m.ai.target = None;
    }
}
/// HurtByTargetGoal (optionally alerting others of the same species).
#[derive(Clone, Debug)]
pub struct HurtByTarget {
    pub alert: bool,
    /// Species that should not be alerted.
    pub alert_except: Vec<Species>,
    /// Attackers ignored (e.g. zombified piglins ignore other zombified piglins).
    pub ignore: Vec<Species>,
    /// `Bee.BeeHurtByOtherGoal`: kept only while angry, and alerting others
    /// only when the attacker is in sight.
    pub bee: bool,
    /// Whom `alertOther` calls: a filter on the mob (`PandaHurtByTargetGoal`
    /// calls aggressive pandas only).
    pub alert_if: Option<fn(&Seen) -> bool>,
    /// `PolarBear.PolarBearHurtByTargetGoal`: a hurt cub only calls the
    /// adult bears and drops the target itself.
    pub polar_bear: bool,
    timestamp: i32,
    st: TargetState,
}
impl HurtByTarget {
    pub fn new() -> Self {
        Self {
            alert: false,
            alert_except: vec![],
            ignore: vec![],
            bee: false,
            alert_if: None,
            polar_bear: false,
            timestamp: 0,
            st: TargetState {
                unseen_memory: 300,
                ..Default::default()
            },
        }
    }
    pub fn alert(mut self) -> Self {
        self.alert = true;
        self
    }
    pub fn ignoring(mut self, s: &[Species]) -> Self {
        self.ignore = s.to_vec();
        self
    }
    pub fn bee(mut self) -> Self {
        self.bee = true;
        self
    }
    pub fn alert_if(mut self, f: fn(&Seen) -> bool) -> Self {
        self.alert_if = Some(f);
        self
    }
    pub fn polar_bear(mut self) -> Self {
        self.polar_bear = true;
        self
    }
}
impl Default for HurtByTarget {
    fn default() -> Self {
        Self::new()
    }
}
impl Goal for HurtByTarget {
    goal_clone!();
    fn name(&self) -> &'static str {
        if self.bee {
            "BeeHurtByOtherGoal"
        } else if self.polar_bear {
            "PolarBearHurtByTargetGoal"
        } else {
            "HurtByTarget"
        }
    }
    fn flags(&self) -> u8 {
        TARGET
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        let Some(by) = m.ai.last_hurt_by else {
            return false;
        };
        if m.ai.last_hurt_by_time == self.timestamp {
            return false;
        }
        // `HurtByTargetGoal.canUse`: with universal anger on, a player's hit
        // is left to the anger goals, never a direct retaliation.
        if by == Who::Player && cx.w.universal_anger {
            return false;
        }
        if let Who::Mob(id) = by
            && cx
                .w
                .seen(id)
                .is_some_and(|s| self.ignore.contains(&s.species))
        {
            return false;
        }
        let Some(t) = cx.w.target_info(by) else {
            return false;
        };
        t.alive
            && t.can_be_seen_as_enemy
            && !(t.is_player && t.creative)
            // HurtByTargetGoal.canUse ends with canAttack(lastHurtByMob).
            && !m.spares_target(cx.w, by)
            // TargetGoal.canAttack: Mob.isWithinHome(target.blockPosition()).
            && m.within_home_at(t.position)
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if self.bee && !super::bee::angry(m, cx.w) {
            return false;
        }
        target_continue(&mut self.st, true, m, cx)
    }
    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        m.ai.target = m.ai.last_hurt_by;
        self.timestamp = m.ai.last_hurt_by_time;
        self.st.unseen_memory = 300;
        self.st.unseen = 0;
        // PolarBearHurtByTargetGoal.start: a cub alerts the adults itself.
        let cub = self.polar_bear && m.is_baby();
        if (self.alert || cub)
            && let Some(by) = m.ai.target
        {
            let range = m.attrs.follow_range;
            let species = m.species();
            let id = m.id;
            let pos = m.position;
            // TamableAnimal: its owner (`getOwner`, none for a wild one) and
            // whether it is a pet at all.
            let tamable = m.is_tamable_animal();
            let owner = m.extra.tame.then_some(m.extra.owner).flatten();
            let player = cx.w.player.as_ref().map(|p| p.uuid);
            let ids: Vec<u32> =
                cx.w.others
                    .iter()
                    .filter(|s| {
                        s.id != id
                            && s.alive
                            && s.species == species
                            && s.target.is_none()
                            && (s.position.x - pos.x).abs() <= range + 1.0
                            && (s.position.y - pos.y).abs() <= 11.0
                            && (s.position.z - pos.z).abs() <= range + 1.0
                    })
                    .filter(|s| !self.alert_except.contains(&s.species))
                    .filter(|s| self.alert_if.is_none_or(|f| f(s)))
                    // PolarBearHurtByTargetGoal.alertOther: adults only.
                    .filter(|s| !(self.polar_bear && s.baby))
                    .filter(|s| Who::Mob(s.id) != by)
                    // HurtByTargetGoal.alertOthers: a pet is only called by a
                    // pet with the same owner.
                    .filter(|s| !tamable || s.tame.then_some(s.owner).flatten() == owner)
                    // TamableAnimal.considersEntityAsAlly: a pet tamed by the
                    // player who hurt it is allied with them, so it is not called.
                    .filter(|s| {
                        !(tamable
                            && by == Who::Player
                            && s.tame
                            && s.owner.is_some()
                            && s.owner == player)
                    })
                    .map(|s| s.id)
                    .collect();
            // Bee.BeeHurtByOtherGoal.alertOther: only with the attacker in sight.
            if self.bee && !ids.is_empty() && !m.sees(cx.w, by) {
                return;
            }
            for other in ids {
                cx.events.push(MobEvent::SetTarget {
                    mob: other,
                    target: by,
                });
            }
        }
        if cub {
            m.ai.target = None;
        }
    }
    fn stop(&mut self, m: &mut Mob, _: &mut Cx) {
        m.ai.target = None;
    }
}
/// What an AvoidEntityGoal flees from.
#[derive(Clone, Debug)]
pub enum Avoid {
    Player,
    Species(Vec<Species>),
}
/// AvoidEntityGoal.
#[derive(Clone, Debug)]
pub struct AvoidEntity {
    pub what: Avoid,
    pub max_dist: f32,
    pub walk: f64,
    pub sprint: f64,
    /// Extra condition on the avoider (e.g. untamed cats only).
    pub when: Option<fn(&Mob) -> bool>,
    /// Extra condition on the player or mob avoided (`AvoidEntityGoal`'s
    /// predicate: a sneaking player, a tame wolf).
    pub player_when: Option<fn(&Mob, &super::PlayerView) -> bool>,
    pub seen_when: Option<fn(&Seen) -> bool>,
    from: Vec3,
    /// The mob being avoided (`toAvoid`), when it is one.
    pub found: Option<u32>,
}
impl AvoidEntity {
    pub fn new(what: Avoid, max_dist: f32, walk: f64, sprint: f64) -> Self {
        Self {
            what,
            max_dist,
            walk,
            sprint,
            when: None,
            player_when: None,
            seen_when: None,
            from: Vec3::ZERO,
            found: None,
        }
    }
    pub fn when(mut self, f: fn(&Mob) -> bool) -> Self {
        self.when = Some(f);
        self
    }
    pub fn player_when(mut self, f: fn(&Mob, &super::PlayerView) -> bool) -> Self {
        self.player_when = Some(f);
        self
    }
    pub fn seen_when(mut self, f: fn(&Seen) -> bool) -> Self {
        self.seen_when = Some(f);
        self
    }
    /// The target the goal flees from, before the random retreat position and
    /// the path. `AvoidEntityGoal` finds it with `TargetingConditions.forCombat()`:
    /// for the player that is `canAttack` (`LivingEntity.canAttack` refuses a
    /// player in Peaceful), the range scaled by `getVisibilityPercent` with a
    /// floor of 2 and line of sight required; the species arm needs the line of
    /// sight too, because `checkLineOfSight` defaults to true. Without them an
    /// animal fled a player behind a wall, a sneaking or invisible one from the
    /// full range, and in Peaceful where vanilla animals ignore the player.
    fn avoid_target(&mut self, m: &mut Mob, cx: &mut Cx) -> Option<Vec3> {
        let max = f64::from(self.max_dist);
        if matches!(self.what, Avoid::Player) {
            let wanted = cx.w.player.as_ref().is_none_or(|p| {
                self.player_when.is_none_or(|f| f(m, p))
                    && (p.position.y - m.position.y).abs() <= max + 3.0
            });
            // `can_target_player` refuses a player in Peaceful (`spares_players`).
            if !wanted || !m.can_target_player(cx.w, max, true) {
                return None;
            }
            return cx.w.player.as_ref().map(|p| p.position);
        }
        let Avoid::Species(list) = &self.what else {
            return None;
        };
        let id = m.id;
        let y = m.position.y;
        let seen_when = self.seen_when;
        let mut hit = None;
        for s in cx.w.nearby(m.position, max, move |s| {
            s.id != id
                && list.contains(&s.species)
                && seen_when.is_none_or(|f| f(s))
                && (s.position.y - y).abs() <= 3.0 + max
        }) {
            if m.sees(cx.w, Who::Mob(s.id)) {
                hit = Some((s.id, s.position));
                break;
            }
        }
        self.found = hit.map(|h| h.0);
        hit.map(|h| h.1)
    }
}
impl Goal for AvoidEntity {
    goal_clone!();
    fn flags(&self) -> u8 {
        MOVE
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if let Some(f) = self.when
            && !f(m)
        {
            return false;
        }
        let max = f64::from(self.max_dist);
        let Some(from) = self.avoid_target(m, cx) else {
            return false;
        };
        // The player arm already applied the visibility-scaled range (floor 2);
        // the species arm only got a box test from `nearby`.
        if !matches!(self.what, Avoid::Player) && m.distance_sqr(from) > max * max {
            return false;
        }
        let Some(pos) = m.default_random_pos_away(cx.w, 16, 7, [from.x, from.y, from.z]) else {
            return false;
        };
        let to_pos =
            (from.x - pos[0]).powi(2) + (from.y - pos[1]).powi(2) + (from.z - pos[2]).powi(2);
        if to_pos < m.distance_sqr(from) {
            return false;
        }
        self.from = from;
        let path = m.create_path_to(
            cx.w,
            [
                pos[0].floor() as i32,
                pos[1].floor() as i32,
                pos[2].floor() as i32,
            ],
            0,
        );
        m.ai.pending_path = path;
        m.ai.pending_path.is_some()
    }
    fn can_continue(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
        !m.ai.nav.is_done()
    }
    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        let path = m.ai.pending_path.take();
        m.nav_move_to_path(cx.w, path, self.walk);
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let from = match &self.what {
            Avoid::Player => {
                cx.w.player
                    .as_ref()
                    .map(|p| p.position)
                    .unwrap_or(self.from)
            }
            Avoid::Species(_) => self.from,
        };
        m.ai.nav.speed_modifier = if m.distance_sqr(from) < 49.0 {
            self.sprint
        } else {
            self.walk
        };
    }
}
mod tempt;

pub use tempt::Tempt;
fn look_angles(look: [f64; 3]) -> (f64, f64) {
    let yaw = (-look[0]).atan2(look[2]).to_degrees();
    let pitch = (-look[1]).asin().to_degrees();
    (yaw, pitch)
}
/// BreedGoal: walk to a partner in love and breed after 60 ticks.
#[derive(Clone, Debug)]
pub struct Breed {
    pub speed: f64,
    partner: Option<u32>,
    love_time: i32,
}
impl Breed {
    pub fn new(speed: f64) -> Self {
        Self {
            speed,
            partner: None,
            love_time: 0,
        }
    }
}
impl Goal for Breed {
    goal_clone!();
    fn flags(&self) -> u8 {
        MOVE | LOOK
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if m.extra.in_love <= 0 {
            return false;
        }
        let species = m.species();
        // Wolf.canMate and Cat.canMate: both tame. The sitting pose sits on the
        // partner side only, and only for a wolf (`wolf.isInSittingPose()`), so
        // a tamed mob ordered to sit still walks off to breed.
        let tamable = matches!(species, Species::Wolf | Species::Cat);
        if tamable && !m.extra.tame {
            return false;
        }
        // AbstractHorse.canMate is false and Mule keeps it: mules never breed.
        if species == Species::Mule {
            return false;
        }
        // TurtleBreedGoal.canUse: not while carrying an egg.
        if species == Species::Turtle && super::turtle::has_egg(m) {
            return false;
        }
        let id = m.id;
        // Horses and donkeys breed together (AbstractHorse.canParent: tame).
        // Sniffer.canMate: calm states only.
        let sniffer = species == Species::Sniffer;
        if sniffer && !super::sniffer::can_mate_state(m.extra.variant) {
            return false;
        }
        let llama = super::llama::is_llama(species);
        let equine = matches!(species, Species::Horse | Species::Donkey) || llama;
        // Camel.canMate asks AbstractHorse.canParent of both sides: not a
        // vehicle or a passenger and at full health (a camel is always tame).
        let camel = species == Species::Camel;
        // AbstractHorse.canParent: tame, an adult, not a vehicle or passenger
        // and at full health.
        if (equine || camel)
            && (!m.extra.tame
                || m.is_baby()
                || m.rider.is_some()
                || m.extra.rider.is_some()
                || m.extra.mount.is_some()
                || m.health < m.attrs.max_health)
        {
            return false;
        }
        self.partner =
            cx.w.nearby(m.position, 8.0, move |s| {
                let same = s.species == species
                    || llama && super::llama::is_llama(s.species)
                    || !llama && equine && matches!(s.species, Species::Horse | Species::Donkey);
                s.id != id
                    && same
                    && s.in_love
                    && !s.baby
                    && (!tamable || s.tame)
                    // Wolf.canMate: a sitting wolf is no partner.
                    && !(species == Species::Wolf && s.sitting)
                    // AbstractHorse.canParent of the partner.
                    && (!(equine || camel) || s.tame && !s.ridden && s.health >= s.stats[0] as f32)
                    && (!sniffer || super::sniffer::can_mate_state(s.variant))
            })
            .find(|s| m.distance_sqr(s.position) <= 64.0)
            .map(|s| s.id);
        self.partner.is_some()
    }
    fn can_continue(&mut self, _: &mut Mob, cx: &mut Cx) -> bool {
        self.partner
            .and_then(|p| cx.w.seen(p))
            .is_some_and(|s| s.alive && s.in_love)
            && self.love_time < 60
    }
    fn stop(&mut self, _: &mut Mob, _: &mut Cx) {
        self.partner = None;
        self.love_time = 0;
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let Some(p) = self.partner.and_then(|p| cx.w.seen(p)).cloned() else {
            return;
        };
        let x = m.max_head_x_rot();
        m.look_at_speed([p.position.x, p.eye_y, p.position.z], 10.0, x);
        m.nav_move_to_entity(
            cx.w,
            [p.position.x, p.position.y, p.position.z],
            1,
            self.speed,
        );
        self.love_time += 1;
        if self.love_time >= reduced(60) && m.distance_sqr(p.position) < 9.0 {
            // Llama.getBreedOffspring and finalizeSpawnChildFromBreeding's
            // experience, from the parent's random: vanilla draws it only
            // inside the `mob_drops` check.
            let llama = super::llama::is_llama(m.species()).then(|| {
                let o = super::llama::breed_offspring(m, &p);
                if cx.w.mob_drops {
                    let xp = 1 + m.random.next_int_bound(7).unwrap_or(0);
                    cx.events.push(MobEvent::Experience {
                        at: [m.position.x, m.position.y, m.position.z],
                        amount: xp as u32,
                    });
                }
                Box::new(o)
            });
            m.extra.in_love = 0;
            m.extra.age = 6000;
            // TurtleBreedGoal.breed: the mother carries an egg, not a baby.
            if m.species() == Species::Turtle {
                m.extra.flag = true;
            }
            // Frog.spawnChildFromBreeding: IS_PREGNANT instead of a baby.
            if m.species() == Species::Frog {
                super::frog::set_pregnant(m, true);
            }
            cx.events.push(MobEvent::Breed {
                parent: m.id,
                partner: p.id,
                llama,
            });
        }
    }
}
/// FollowParentGoal.
#[derive(Clone, Debug)]
pub struct FollowParent {
    pub speed: f64,
    parent: Option<u32>,
    recalc: i32,
}
impl FollowParent {
    pub fn new(speed: f64) -> Self {
        Self {
            speed,
            parent: None,
            recalc: 0,
        }
    }
}
impl Goal for FollowParent {
    goal_clone!();
    fn flags(&self) -> u8 {
        0
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if m.extra.age >= 0 {
            return false;
        }
        let species = m.species();
        let pos = m.position;
        let mut best: Option<(u32, f64)> = None;
        // Every position in range puts the box's bottom face in this area.
        let area = Aabb::new(
            Vec3::new(pos.x - 9.0, pos.y - 5.0, pos.z - 9.0),
            Vec3::new(pos.x + 9.0, pos.y + 5.0, pos.z + 9.0),
        );
        for s in cx.w.others_near(area).filter(|s| {
            s.alive
                && s.species == species
                && !s.baby
                && (s.position.x - pos.x).abs() <= 8.0
                && (s.position.y - pos.y).abs() <= 4.0
                && (s.position.z - pos.z).abs() <= 8.0
        }) {
            let d = m.distance_sqr(s.position);
            if best.is_none_or(|b| d <= b.1) {
                best = Some((s.id, d));
            }
        }
        match best {
            Some((id, d)) if d >= 9.0 => {
                self.parent = Some(id);
                true
            }
            _ => false,
        }
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if m.extra.age >= 0 {
            return false;
        }
        let Some(p) = self.parent.and_then(|p| cx.w.seen(p)) else {
            return false;
        };
        let d = m.distance_sqr(p.position);
        p.alive && (9.0..=256.0).contains(&d)
    }
    fn start(&mut self, _: &mut Mob, _: &mut Cx) {
        self.recalc = 0;
    }
    fn stop(&mut self, _: &mut Mob, _: &mut Cx) {
        self.parent = None;
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        self.recalc -= 1;
        if self.recalc <= 0 {
            self.recalc = reduced(10);
            if let Some(p) = self.parent.and_then(|p| cx.w.seen(p)) {
                let at = [p.position.x, p.position.y, p.position.z];
                m.nav_move_to_entity(cx.w, at, 1, self.speed);
            }
        }
    }
}
/// `FollowMobGoal` `(mob, speed, stopDistance, areaSize)`: follows the first
/// visible mob of another class within `area` blocks and keeps `stop` away
/// from it (a parrot hovers by the other animals).
///
/// Left out: vanilla also backs away when the followed mob's look control
/// wants this mob's position; only another `FollowMobGoal` mob of a different
/// class would look at it, and parrots are the only ones that have the goal.
#[derive(Clone, Debug)]
pub struct FollowMob {
    speed: f64,
    stop: f64,
    area: f64,
    following: Option<u32>,
    recalc: i32,
    old_water: f32,
}
impl FollowMob {
    pub fn new(speed: f64, stop: f32, area: f32) -> Self {
        Self {
            speed,
            stop: f64::from(stop),
            area: f64::from(area),
            following: None,
            recalc: 0,
            old_water: 0.0,
        }
    }
}
impl Goal for FollowMob {
    fn name(&self) -> &'static str {
        "FollowMobGoal"
    }
    goal_clone!();
    fn flags(&self) -> u8 {
        MOVE | LOOK
    }
    /// `FollowMobGoal.canUse`.
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        let species = m.species();
        // `getEntitiesOfClass(Mob, getBoundingBox().inflate(areaSize))`.
        let area = m
            .bounds()
            .inflate(Vec3::new(self.area, self.area, self.area));
        let found = cx
            .w
            .others_near(area)
            .find(|s| {
                s.species != species && s.bounds.intersects(area) && !s.has_effect("invisibility")
            })
            .map(|s| s.id);
        self.following = found;
        found.is_some()
    }
    /// `FollowMobGoal.canContinueToUse`.
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        let Some(at) = self
            .following
            .and_then(|id| cx.w.seen(id))
            .map(|s| s.position)
        else {
            return false;
        };
        !m.ai.nav.is_done() && m.distance_sqr(at) > self.stop * self.stop
    }
    /// `FollowMobGoal.start`.
    fn start(&mut self, m: &mut Mob, _: &mut Cx) {
        self.recalc = 0;
        self.old_water = m.malus(super::path::PathType::Water);
        m.set_malus(super::path::PathType::Water, 0.0);
    }
    /// `FollowMobGoal.stop`.
    fn stop(&mut self, m: &mut Mob, _: &mut Cx) {
        self.following = None;
        m.nav_stop();
        m.set_malus(super::path::PathType::Water, self.old_water);
    }
    /// `FollowMobGoal.tick`.
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let Some((at, eye)) = self
            .following
            .and_then(|id| cx.w.seen(id))
            .map(|s| (s.position, s.eye_y))
        else {
            return;
        };
        if m.extra.leash.is_some() {
            return;
        }
        let max = m.max_head_x_rot();
        m.look_at_speed([at.x, eye, at.z], 10.0, max);
        self.recalc -= 1;
        if self.recalc <= 0 {
            self.recalc = reduced(10);
            let d = m.distance_sqr(at);
            if d > self.stop * self.stop {
                m.nav_move_to_entity(cx.w, [at.x, at.y, at.z], 1, self.speed);
            } else {
                m.nav_stop();
                // `distSqr <= stopDistance` (not squared, as vanilla writes it):
                // too close, so it moves away from the mob.
                if d <= self.stop {
                    let (dx, dz) = (at.x - m.position.x, at.z - m.position.z);
                    m.nav_move_to(
                        cx.w,
                        [m.position.x - dx, m.position.y, m.position.z - dz],
                        self.speed,
                    );
                }
            }
        }
    }
}
/// RestrictSunGoal: avoid sunlit nodes while it is bright outside.
#[derive(Clone, Debug)]
pub struct RestrictSun;
impl Goal for RestrictSun {
    goal_clone!();
    fn flags(&self) -> u8 {
        0
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        cx.w.daylight && m.extra.head.is_none()
    }
    fn start(&mut self, m: &mut Mob, _: &mut Cx) {
        m.ai.nav.avoid_sun = true;
    }
    fn stop(&mut self, m: &mut Mob, _: &mut Cx) {
        m.ai.nav.avoid_sun = false;
    }
}
/// `FleeSunGoal.getHidePos`: a spot within 10 blocks that the sky does not
/// see and the mob likes.
pub fn hide_pos(m: &mut Mob, w: &MobWorld) -> Option<[f64; 3]> {
    let (bx, by, bz) = (
        m.position.x.floor() as i32,
        m.position.y.floor() as i32,
        m.position.z.floor() as i32,
    );
    for _ in 0..10 {
        let p = [
            bx + m.random.next_int_bound(20).unwrap_or(0) - 10,
            by + m.random.next_int_bound(6).unwrap_or(0) - 3,
            bz + m.random.next_int_bound(20).unwrap_or(0) - 10,
        ];
        if !w.can_see_sky(p[0], p[1], p[2]) && m.walk_target_value(w, p) < 0.0 {
            return Some([
                f64::from(p[0]) + 0.5,
                f64::from(p[1]),
                f64::from(p[2]) + 0.5,
            ]);
        }
    }
    None
}
/// FleeSunGoal: find shade while burning.
#[derive(Clone, Debug)]
pub struct FleeSun {
    pub speed: f64,
    wanted: [f64; 3],
}
impl FleeSun {
    pub fn new(speed: f64) -> Self {
        Self {
            speed,
            wanted: [0.0; 3],
        }
    }
}
impl Goal for FleeSun {
    goal_clone!();
    fn flags(&self) -> u8 {
        MOVE
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if m.ai.target.is_some() || !cx.w.daylight || m.fire_ticks <= 0 || m.extra.head.is_some() {
            return false;
        }
        let (bx, by, bz) = (
            m.position.x.floor() as i32,
            m.position.y.floor() as i32,
            m.position.z.floor() as i32,
        );
        if !cx.w.can_see_sky(bx, by, bz) {
            return false;
        }
        match hide_pos(m, cx.w) {
            Some(at) => {
                self.wanted = at;
                true
            }
            None => false,
        }
    }
    fn can_continue(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
        !m.ai.nav.is_done()
    }
    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        m.nav_move_to(cx.w, self.wanted, self.speed);
    }
}
/// EatBlockGoal (sheep): eat short grass or turn grass blocks to dirt.
#[derive(Clone, Debug, Default)]
pub struct EatBlock {
    tick: i32,
}
impl Goal for EatBlock {
    goal_clone!();
    fn flags(&self) -> u8 {
        MOVE | LOOK | JUMP
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        let chance = reduced(if m.is_baby() { 50 } else { 1000 });
        if m.random.next_int_bound(chance).unwrap_or(1) != 0 {
            return false;
        }
        let (x, y, z) = (
            m.position.x.floor() as i32,
            m.position.y.floor() as i32,
            m.position.z.floor() as i32,
        );
        let here = cx.w.block_name(x, y, z).to_owned();
        cx.w.has_tag("minecraft:edible_for_sheep", &here)
            || cx.w.block_name(x, y - 1, z) == "minecraft:grass_block"
    }
    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        self.tick = reduced(40);
        m.extra.anim_ticks = 40;
        // EatBlockGoal.start: broadcastEntityEvent 10, so the head goes down
        // on every client.
        let at = [m.position.x, m.position.y, m.position.z];
        cx.events.push(MobEvent::EntityEvent {
            mob: m.id,
            event: 10,
            at,
        });
        m.nav_stop();
    }
    fn stop(&mut self, _: &mut Mob, _: &mut Cx) {
        self.tick = 0;
    }
    fn can_continue(&mut self, _: &mut Mob, _: &mut Cx) -> bool {
        self.tick > 0
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        self.tick = (self.tick - 1).max(0);
        if self.tick == reduced(4) {
            let (x, y, z) = (
                m.position.x.floor() as i32,
                m.position.y.floor() as i32,
                m.position.z.floor() as i32,
            );
            let here = cx.w.block_name(x, y, z).to_owned();
            // EatBlockGoal.tick: both halves of the world change sit behind
            // MOB_GRIEFING (`ifeq 89`, `ifeq 182` in the bytecode); `Mob.ate`
            // runs either way, so a sheep still sheds its wool and ages up.
            if cx.w.has_tag("minecraft:edible_for_sheep", &here) {
                if cx.w.mob_griefing {
                    cx.events.push(MobEvent::BreakBlock {
                        pos: [x, y, z],
                        drop: false,
                    });
                }
                super::ai::ate(m, cx.events);
            } else if cx.w.block_name(x, y - 1, z) == "minecraft:grass_block" {
                // The particles and the dirt `setBlock` are the same gate.
                if cx.w.mob_griefing {
                    cx.events.push(MobEvent::SetBlockName {
                        mob: Some(m.id),
                        pos: [x, y - 1, z],
                        name: "minecraft:dirt",
                        effect: true,
                        flags: horizon_world::level::flags::UPDATE_CLIENTS,
                    });
                }
                super::ai::ate(m, cx.events);
            }
        }
    }
}

/// BreathAirGoal: swim up to air when the air supply runs low.
#[derive(Clone, Debug, Default)]
pub struct BreathAir;
impl BreathAir {
    /// `BreathAirGoal.findAirPosition`: `BlockPos.neighborColumn` walks the
    /// center, north, east, south and west columns, each bottom to top.
    fn air_position(
        [x, y, z]: [i32; 3],
        mut gives_air: impl FnMut([i32; 3]) -> bool,
    ) -> Option<[i32; 3]> {
        for (dx, dz) in [(0, 0), (0, -1), (1, 0), (0, 1), (-1, 0)] {
            for yy in y..=y + 8 {
                let pos = [x + dx, yy, z + dz];
                if gives_air(pos) {
                    return Some(pos);
                }
            }
        }
        None
    }
    fn find_air(m: &mut Mob, cx: &mut Cx) {
        let (x, y, z) = (
            m.position.x.floor() as i32,
            m.position.y.floor() as i32,
            m.position.z.floor() as i32,
        );
        let d = Self::air_position([x, y, z], |[x, y, z]| {
            let s = cx.w.world.block(x, y, z);
            let f = cx.w.facts.get(s);
            (f.fluid == crate::blocks::Fluid::None || &*f.name == "minecraft:bubble_column")
                && cx.w.data.state(s).flags & super::data::LAND != 0
        })
        .unwrap_or([x, (m.position.y + 8.0).floor() as i32, z]);
        m.nav_move_to(
            cx.w,
            [f64::from(d[0]), f64::from(d[1] + 1), f64::from(d[2])],
            1.0,
        );
    }
}
impl Goal for BreathAir {
    goal_clone!();
    fn flags(&self) -> u8 {
        MOVE | LOOK
    }
    fn can_use(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
        m.air < 140
    }
    fn interruptable(&self) -> bool {
        false
    }
    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        m.nav_stop();
        Self::find_air(m, cx);
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        Self::find_air(m, cx);
        let input = Vec3::new(
            f64::from(m.ai.xxa),
            f64::from(m.ai.yya),
            f64::from(m.ai.zza),
        );
        m.move_relative(0.02, input);
        let mut ev = Vec::new();
        m.do_move(cx.w, &mut ev);
        cx.events.extend(ev);
    }
}
/// TryFindLiquidGoal (water): head for nearby water when stranded on land.
#[derive(Clone, Debug, Default)]
pub struct TryFindWater {
    wanted: [f64; 3],
}
impl Goal for TryFindWater {
    goal_clone!();
    fn flags(&self) -> u8 {
        MOVE
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if !m.on_ground || m.fluids.in_water {
            return false;
        }
        let (bx, by, bz) = (
            m.position.x.floor() as i32,
            m.position.y.floor() as i32,
            m.position.z.floor() as i32,
        );
        for _ in 0..10 {
            let p = [
                bx + m.random.next_int_bound(20).unwrap_or(0) - 10,
                by + 2 - m.random.next_int_bound(8).unwrap_or(0),
                bz + m.random.next_int_bound(20).unwrap_or(0) - 10,
            ];
            if cx.w.fluid(p[0], p[1], p[2]) == crate::blocks::Fluid::Water {
                self.wanted = [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])];
                return true;
            }
        }
        false
    }
    fn can_continue(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
        !m.ai.nav.is_done()
    }
    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        m.nav_move_to(cx.w, self.wanted, 1.0);
    }
}
/// The block a mob stands in (`blockPosition`).
fn block_of(m: &Mob) -> [i32; 3] {
    [
        m.position.x.floor() as i32,
        m.position.y.floor() as i32,
        m.position.z.floor() as i32,
    ]
}
/// `MoveToBlockGoal`'s shared state: search, walk to and stay at a block.
#[derive(Clone, Debug, Default)]
pub struct MoveToBlock {
    pub speed: f64,
    pub range: i32,
    pub vertical_range: i32,
    pub vertical_start: i32,
    pub next_start: i32,
    pub try_ticks: i32,
    pub max_stay: i32,
    pub reached: bool,
    pub block: [i32; 3],
}
impl MoveToBlock {
    pub fn new(speed: f64, range: i32, vertical_range: i32) -> Self {
        Self {
            speed,
            range,
            vertical_range,
            ..Default::default()
        }
    }
    /// `canUse` with a custom `nextStartTick` delay.
    pub fn can_use(
        &mut self,
        m: &mut Mob,
        w: &MobWorld,
        next: i32,
        valid: impl Fn(&MobWorld, [i32; 3]) -> bool,
    ) -> bool {
        if self.next_start > 0 {
            self.next_start -= 1;
            return false;
        }
        self.next_start = next;
        self.find_nearest(m, w, valid)
    }
    /// `MoveToBlockGoal.findNearestBlock`: rings outward, alternating heights.
    pub fn find_nearest(
        &mut self,
        m: &Mob,
        w: &MobWorld,
        valid: impl Fn(&MobWorld, [i32; 3]) -> bool,
    ) -> bool {
        match self.walk(block_of(m), |p| valid(w, p)) {
            Some(p) => {
                self.block = p;
                true
            }
            None => false,
        }
    }
    /// `findNearestBlock`'s walk out from `base`: the first cell `valid` takes.
    fn walk(&self, base: [i32; 3], mut valid: impl FnMut([i32; 3]) -> bool) -> Option<[i32; 3]> {
        let next = |v: i32| if v > 0 { -v } else { 1 - v };
        let mut y = self.vertical_start;
        while y <= self.vertical_range {
            for r in 0..self.range {
                let mut x = 0;
                while x <= r {
                    let mut z = if x < r && x > -r { r } else { 0 };
                    while z <= r {
                        let p = [base[0] + x, base[1] + y - 1, base[2] + z];
                        if valid(p) {
                            return Some(p);
                        }
                        z = next(z);
                    }
                    x = next(x);
                }
            }
            y = next(y);
        }
        None
    }
    /// The cells [`Self::walk`] can reach from `base`, as a box (inclusive), or
    /// `None` when it walks none.
    fn walk_bounds(&self, base: [i32; 3]) -> Option<([i32; 3], [i32; 3])> {
        if self.range <= 0 || self.vertical_start > self.vertical_range {
            return None;
        }
        // The heights go out from `vertical_start` both ways, past neither it
        // nor `vertical_range` by more than one, a block below.
        let reach = self.range - 1;
        let height = self
            .vertical_start
            .saturating_abs()
            .max(self.vertical_range.saturating_abs())
            .saturating_add(1);
        Some((
            [
                base[0].saturating_sub(reach),
                base[1].saturating_sub(height).saturating_sub(1),
                base[2].saturating_sub(reach),
            ],
            [
                base[0].saturating_add(reach),
                base[1].saturating_add(height),
                base[2].saturating_add(reach),
            ],
        ))
    }
    /// Whether [`Self::find_nearest`] may find a block whose state `matches`
    /// (its `valid` needs one): false when no section of the cells it walks
    /// holds such a state in its palette, so the walk would find nothing.
    pub fn may_find(
        &self,
        m: &Mob,
        w: &MobWorld,
        matches: impl FnMut(horizon_world::registry::BlockStateId) -> bool,
    ) -> bool {
        self.walk_bounds(block_of(m))
            .is_some_and(|(lo, hi)| w.world.may_hold(lo, hi, matches))
    }
    pub fn can_continue(&self, w: &MobWorld, valid: impl Fn(&MobWorld, [i32; 3]) -> bool) -> bool {
        self.try_ticks >= -self.max_stay && self.try_ticks <= 1200 && valid(w, self.block)
    }
    pub fn start(&mut self, m: &mut Mob, w: &MobWorld) {
        let b = self.block;
        m.nav_move_to(
            w,
            [
                f64::from(b[0]) + 0.5,
                f64::from(b[1] + 1),
                f64::from(b[2]) + 0.5,
            ],
            self.speed,
        );
        self.try_ticks = 0;
        let r = m.random.next_int_bound(1200).unwrap_or(0) + 1200;
        self.max_stay = m.random.next_int_bound(r).unwrap_or(0) + 1200;
    }
    /// `MoveToBlockGoal.tick` (accepted distance 1 from the block above).
    pub fn tick(&mut self, m: &mut Mob, w: &MobWorld) {
        self.tick_within(m, w, 1.0);
    }
    /// `MoveToBlockGoal.tick` with the goal's `acceptedDistance`.
    pub fn tick_within(&mut self, m: &mut Mob, w: &MobWorld, accepted: f64) {
        let t = [
            f64::from(self.block[0]) + 0.5,
            f64::from(self.block[1] + 1) + 0.5,
            f64::from(self.block[2]) + 0.5,
        ];
        let d2 = (m.position.x - t[0]).powi(2)
            + (m.position.y - t[1]).powi(2)
            + (m.position.z - t[2]).powi(2);
        if d2 >= accepted * accepted {
            self.reached = false;
            self.try_ticks += 1;
            if self.try_ticks % 40 == 0 {
                m.nav_move_to(w, [t[0], t[1] - 0.5, t[2]], self.speed);
            }
        } else {
            self.reached = true;
            self.try_ticks -= 1;
        }
    }
}

#[cfg(test)]
mod eat_block_tests {
    use super::*;
    use crate::mobs::world::with_built_test_world_rules;

    /// A sheep on a wide grass shelf, ticked long enough for its `EatBlockGoal`
    /// (1 in 1000 a tick) to fire several times: every event it pushed, with
    /// the `mob_griefing` game rule at `griefing` and `mob_drops` on. `tufts`
    /// puts short grass at the sheep's own feet, so the goal takes the
    /// `destroyBlock` half; without it the sheep stands on the grass and takes
    /// the grass-to-dirt half.
    fn sheep_events(griefing: bool, tufts: bool) -> Option<Vec<MobEvent>> {
        let mut out = None;
        let build = |world: &mut crate::world::World,
                     registry: &horizon_world::registry::BlockRegistry| {
            let grass = registry.default_state("minecraft:grass_block").unwrap();
            let tuft = registry.default_state("minecraft:short_grass").unwrap();
            for cx in -3..3 {
                for cz in -3..3 {
                    world
                        .chunks
                        .insert((cx, cz), Arc::new(crate::chunk::Chunk::empty(cx, cz)));
                }
            }
            for x in -8..=8 {
                for z in -8..=8 {
                    world.set(x, 63, z, grass).unwrap();
                    if tufts {
                        world.set(x, 64, z, tuft).unwrap();
                    }
                }
            }
        };
        with_built_test_world_rules(&[], griefing, true, build, |w, types| {
            let kind = Arc::new(types.types["minecraft:sheep"].clone());
            let mut m = Mob::new(1, kind, Vec3::new(0.5, 64.0, 0.5), 0.0);
            let mut random = LegacyRandom::new(0);
            let mut events = Vec::new();
            for _ in 0..6000 {
                crate::mobs::ai::tick(&mut m, w, &mut random, &mut events);
            }
            out = Some(events);
        });
        out
    }

    /// `EatBlockGoal.tick` puts the `levelEvent` + `setBlock` behind
    /// `MOB_GRIEFING` and leaves `Mob.ate` outside it, so with the rule off the
    /// goal still runs (`EntityEvent` 10) and still shears the coat, and the
    /// grass stays.
    #[test]
    fn the_eat_goal_turns_grass_to_dirt_only_with_mob_griefing() {
        let Some(on) = sheep_events(true, false) else {
            return;
        };
        let Some(off) = sheep_events(false, false) else {
            return;
        };
        let dirt = |events: &[MobEvent]| {
            events
                .iter()
                .filter(|e| matches!(e, MobEvent::SetBlockName { name, .. } if *name == "minecraft:dirt"))
                .count()
        };
        let ate = |events: &[MobEvent]| {
            events
                .iter()
                .filter(|e| matches!(e, MobEvent::EntityEvent { event: 10, .. }))
                .count()
        };
        assert!(dirt(&on) > 0, "mob_griefing on: {}", dirt(&on));
        assert_eq!(dirt(&off), 0, "mob_griefing off must leave the grass alone");
        assert!(ate(&off) > 0, "the goal still runs with the rule off");
    }

    /// The edible half of the same gate (`ifeq 89` skips
    /// `Level.destroyBlock(pos, false)`): short grass at the sheep's feet
    /// disappears with the rule on and stays with it off, while the goal eats
    /// either way.
    #[test]
    fn the_eat_goal_breaks_short_grass_only_with_mob_griefing() {
        let Some(on) = sheep_events(true, true) else {
            return;
        };
        let Some(off) = sheep_events(false, true) else {
            return;
        };
        let bitten = |events: &[MobEvent]| {
            events
                .iter()
                .filter(|e| matches!(e, MobEvent::BreakBlock { drop: false, .. }))
                .count()
        };
        let ate = |events: &[MobEvent]| {
            events
                .iter()
                .filter(|e| matches!(e, MobEvent::EntityEvent { event: 10, .. }))
                .count()
        };
        assert!(bitten(&on) > 0, "mob_griefing on: {}", bitten(&on));
        assert_eq!(
            bitten(&off),
            0,
            "mob_griefing off must leave the tufts alone"
        );
        assert!(ate(&off) > 0, "the goal still runs with the rule off");
    }
}

#[cfg(test)]
mod built_by_player_tests {
    use super::*;

    /// A mob of `species`, from a made-up type.
    fn mob(species: Species) -> Mob {
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        Arc::make_mut(&mut kind).species = species;
        Mob::new(1, kind, Vec3::ZERO, 0.0)
    }

    /// `IronGolem.canAttack`: only a golem a player built spares players, so
    /// the save restore drops that target (`Mob.asValidTarget`) while another
    /// mob's `PlayerCreated` byte means nothing.
    #[test]
    fn only_a_golem_spares_the_player_that_built_it() {
        let mut built = mob(Species::IronGolem);
        built.extra.player_created = true;
        assert!(built.built_by_player());
        assert!(!mob(Species::IronGolem).built_by_player());
        let mut zombie = mob(Species::Zombie);
        zombie.extra.player_created = true;
        assert!(!zombie.built_by_player());
    }
}

#[cfg(test)]
#[path = "goals_targeting_tests.rs"]
mod avoid_entity_targeting_tests;

#[cfg(test)]
mod peaceful_targets_tests {
    use super::*;
    use crate::mobs::world::{test_player, with_test_world};

    /// `LivingEntity.canAttack` / `TargetingConditions.test`: in Peaceful a
    /// hurt wolf neither takes its attacker (a player) as target nor picks one
    /// by sight, while at Normal it does.
    #[test]
    fn peaceful_mobs_never_target_the_player() {
        with_test_world(&[], |w, types| {
            let mut w = w.clone();
            w.player = Some(test_player(Vec3::new(2.5, 64.0, 0.5)));
            let kind = Arc::new(types.types["minecraft:wolf"].clone());
            let mut wolf = Mob::new(1, kind, Vec3::new(0.5, 64.0, 0.5), 0.0);
            wolf.ai.last_hurt_by = Some(Who::Player);
            wolf.ai.last_hurt_by_time = 5;
            let mut random = LegacyRandom::new(0);
            let mut events = Vec::new();
            let mut cx = Cx {
                w: &w,
                lr: &mut random,
                events: &mut events,
            };
            assert!(
                HurtByTarget::default().can_use(&mut wolf, &mut cx),
                "normal"
            );
            assert!(wolf.can_target_player(&w, 16.0, false), "normal");

            let mut peaceful = w.clone();
            peaceful.difficulty = crate::mobs::spawning::LocalDifficulty::new(0, 18000, 0, 0.0);
            let mut cx = Cx {
                w: &peaceful,
                lr: &mut random,
                events: &mut events,
            };
            assert!(
                !HurtByTarget::default().can_use(&mut wolf, &mut cx),
                "peaceful"
            );
            assert!(!wolf.can_target_player(&peaceful, 16.0, false), "peaceful");
            assert!(!wolf.can_keep_target_player(&peaceful, 16.0), "peaceful");
        });
    }

    /// `Mob.canAttack` refuses a Ghast, and `HurtByTargetGoal.canUse` ends
    /// with `canAttack(lastHurtByMob)`: a ghast's fireball is not retaliated.
    #[test]
    fn a_hurt_mob_does_not_retaliate_against_a_ghast() {
        let mut seen = None;
        with_test_world(&[], |_, types| {
            let mob = |id: u32, name: &str, at: Vec3| {
                Mob::new(id, Arc::new(types.types[name].clone()), at, 0.0)
            };
            let ghast = mob(2, "minecraft:ghast", Vec3::new(2.5, 64.0, 0.5));
            let zombie = mob(3, "minecraft:zombie", Vec3::new(3.5, 64.0, 0.5));
            seen = Some([ghast.seen(None), zombie.seen(None)]);
        });
        let Some(seen) = seen else { return };
        with_test_world(&seen, |w, types| {
            let kind = Arc::new(types.types["minecraft:cow"].clone());
            let mut m = Mob::new(1, kind, Vec3::new(0.5, 64.0, 0.5), 0.0);
            let mut lr = LegacyRandom::new(0);
            let mut events = Vec::new();
            let mut cx = Cx {
                w,
                lr: &mut lr,
                events: &mut events,
            };
            m.ai.last_hurt_by = Some(Who::Mob(2));
            m.ai.last_hurt_by_time = 5;
            assert!(
                !HurtByTarget::default().can_use(&mut m, &mut cx),
                "a ghast is not a valid attacker to retaliate against"
            );
            m.ai.last_hurt_by = Some(Who::Mob(3));
            m.ai.last_hurt_by_time = 6;
            assert!(
                HurtByTarget::default().can_use(&mut m, &mut cx),
                "a zombie attacker is retaliated against"
            );
        });
    }
}
#[cfg(test)]
mod move_to_block_tests {
    use super::MoveToBlock;

    /// Every cell the walk tries lies in the box `may_find` asks the
    /// palettes about, for the ranges the goals use and odd ones; and the
    /// walk stops at the first cell it is given.
    #[test]
    fn the_walk_stays_in_its_bounds() {
        for range in [-1, 0, 1, 2, 8, 24] {
            for vertical_range in [-2, 0, 1, 2, 3, 6] {
                for vertical_start in [-3, -1, 0, 1, 2] {
                    let goal = MoveToBlock {
                        range,
                        vertical_range,
                        vertical_start,
                        ..Default::default()
                    };
                    for base in [[0, 64, 0], [-17, -60, 31], [100, 319, -100]] {
                        let bounds = goal.walk_bounds(base);
                        let mut walked = 0;
                        let found = goal.walk(base, |p| {
                            walked += 1;
                            let (lo, hi) = bounds.expect("a walk with no bounds");
                            assert!((0..3).all(|a| lo[a] <= p[a] && p[a] <= hi[a]), "{p:?}");
                            false
                        });
                        assert_eq!(found, None);
                        assert_eq!(
                            walked > 0,
                            bounds.is_some(),
                            "{range} {vertical_range} {vertical_start}"
                        );
                    }
                }
            }
        }
        let goal = MoveToBlock::new(1.0, 24, 2);
        assert_eq!(goal.walk([5, 64, 5], |_| true), Some([5, 63, 5]));
    }
}

#[cfg(test)]
#[path = "breed_partner_range_tests.rs"]
mod breed_partner_range_tests;
#[cfg(test)]
#[path = "breed_xp_tests.rs"]
mod breed_xp_tests;

#[cfg(test)]
mod melee_range_tests {
    use super::*;

    fn mob_holding(item: &str, components: Option<&str>) -> Mob {
        let kind = (*crate::mobs::sensor_scan_tests::kind()).clone();
        let mut m = Mob::new(1, Arc::new(kind), Vec3::new(0.5, 64.0, 0.5), 0.0);
        let mut stack = crate::items::ItemStack::new(item, 1);
        if let Some(text) = components {
            let tag = horizon_world::nbt::parse_snbt(text).unwrap().0;
            (stack.damage, stack.extra) = crate::item_data::decode_components(Some(&tag));
        }
        m.extra.set_main_hand_stack(Some(stack));
        m
    }
    /// A one-block target whose near face is `gap` blocks east of the mob's box.
    fn target(m: &Mob, gap: f64) -> Aabb {
        let x = m.bounds().max_position().x + gap;
        Aabb::new(Vec3::new(x, 64.0, 0.0), Vec3::new(x + 1.0, 65.0, 1.0))
    }

    /// `Mob.isWithinMeleeAttackRange` with `getActiveItem().get(ATTACK_RANGE)`:
    /// no component reaches sqrt(2.04) - 0.6 (about 0.83); a spear 4.5 and 2.0
    /// times its mob factor 0.5; a patched component with the codec's
    /// defaults (`mob_factor` 1, `min_reach` 0); a removed one the default.
    #[test]
    fn the_active_items_attack_range_sets_the_reach() {
        let sword = mob_holding("minecraft:iron_sword", None);
        assert!(sword.melee_reaches_beside(None, target(&sword, 0.8)));
        assert!(!sword.melee_reaches_beside(None, target(&sword, 0.9)));

        let spear = mob_holding("minecraft:iron_spear", None);
        assert!(spear.melee_reaches_beside(None, target(&spear, 2.2)));
        assert!(!spear.melee_reaches_beside(None, target(&spear, 2.3)));
        assert!(
            !spear.melee_reaches_beside(None, target(&spear, 0.5)),
            "inside the minimum range"
        );
        assert!(spear.melee_reaches_beside(None, target(&spear, 1.1)));

        let long = mob_holding(
            "minecraft:iron_sword",
            Some(r#"{"minecraft:attack_range":{max_reach:6.0f}}"#),
        );
        assert!(long.melee_reaches_beside(None, target(&long, 5.9)));
        assert!(
            long.melee_reaches_beside(None, target(&long, 0.0)),
            "no minimum by default"
        );
        assert!(!long.melee_reaches_beside(None, target(&long, 6.1)));

        let blunt = mob_holding(
            "minecraft:iron_spear",
            Some(r#"{"!minecraft:attack_range":{}}"#),
        );
        assert!(blunt.melee_reaches_beside(None, target(&blunt, 0.5)));
        assert!(!blunt.melee_reaches_beside(None, target(&blunt, 2.0)));
    }

    /// `Mob.getAttackBoundingBox`: a rider's box widened on x and z to its
    /// vehicle's (a 1.4 wide horse under a 0.6 wide zombie) before the reach
    /// is added; `LivingEntity.getHitbox`: a rider is hit from its seat up.
    #[test]
    fn a_riders_attack_box_takes_its_vehicles_width() {
        let mut rider = mob_holding("minecraft:iron_sword", None);
        let at = rider.position;
        let horse = Aabb::new(
            Vec3::new(at.x - 0.7, at.y - 1.0, at.z - 0.7),
            Vec3::new(at.x + 0.7, at.y + 0.6, at.z + 0.7),
        );
        // Its near face 0.9 past the zombie's box: past the default reach of
        // the zombie alone (about 0.83), within it from the horse's side.
        let target = target(&rider, 0.9);
        assert!(!rider.melee_reaches_beside(None, target));
        rider.extra.mount = Some(9);
        rider.seated(9, 0.7, horse);
        assert!(rider.melee_reaches_beside(Some(horse), target));
        // Its own box keeps its height: the horse's lower box adds nothing.
        let below = Aabb::new(
            Vec3::new(at.x + 0.9, at.y - 1.0, at.z),
            Vec3::new(at.x + 1.1, at.y - 0.5, at.z + 1.0),
        );
        assert!(!rider.melee_reaches_beside(Some(horse), below));

        assert_eq!(rider.hitbox().min_position().y, at.y + 0.7);
        // A seat left from another vehicle is not this one's.
        rider.extra.mount = Some(10);
        assert_eq!(rider.hitbox().min_position().y, at.y);
        rider.extra.mount = None;
        assert_eq!(rider.hitbox().min_position().y, at.y);
    }
}

#[cfg(test)]
mod melee_continue_tests {
    use super::*;
    use crate::mobs::world::{test_player, with_test_world};
    use horizon_core::geometry::Vec3;
    use std::sync::Arc;

    /// Tick one `<MeleeAttack>` goal through the real selector.
    fn tick(goals: &mut GoalSelector, m: &mut Mob, world: &MobWorld) {
        let mut lr = LegacyRandom::new(0);
        let mut events = Vec::new();
        let mut cx = Cx {
            w: world,
            lr: &mut lr,
            events: &mut events,
        };
        goals.tick(m, &mut cx);
    }

    /// `MeleeAttackGoal.canContinueToUse`'s follow branch refuses a target
    /// outside the mob's home (`isWithinHome(target.blockPosition())`).
    #[test]
    fn a_chase_stops_outside_the_mobs_home() {
        let mut outcomes = None;
        let ran = with_test_world(&[], |w, types| {
            let kind = Arc::new(types.types["minecraft:iron_golem"].clone());
            let mut m = Mob::new(1, kind, Vec3::new(0.5, 64.0, 0.5), 0.0);
            m.ai.home = Some(([0, 64, 0], 8));
            m.ai.target = Some(Who::Player);
            let mut world = w.clone();
            world.player = Some(test_player(Vec3::new(0.5, 64.0, 0.5)));
            let mut goals = GoalSelector::default();
            goals.add(1, MeleeAttack::new(1.0, true));
            tick(&mut goals, &mut m, &world);
            let started = goals.is_running("MeleeAttack");
            world.player.as_mut().unwrap().position = Vec3::new(100.5, 64.0, 0.5);
            tick(&mut goals, &mut m, &world);
            outcomes = Some((started, goals.is_running("MeleeAttack")));
        });
        if !ran {
            return;
        }
        assert_eq!(
            outcomes,
            Some((true, false)),
            "the chase starts at home and stops when the target leaves its radius"
        );
    }

    /// The same branch refuses a spectator (`!(player.isSpectator() || player.isCreative())`).
    #[test]
    fn a_chase_stops_when_the_target_is_a_spectator() {
        let mut outcomes = None;
        let ran = with_test_world(&[], |w, types| {
            let kind = Arc::new(types.types["minecraft:iron_golem"].clone());
            let mut m = Mob::new(1, kind, Vec3::new(0.5, 64.0, 0.5), 0.0);
            m.ai.target = Some(Who::Player);
            let mut world = w.clone();
            world.player = Some(test_player(Vec3::new(0.5, 64.0, 0.5)));
            let mut goals = GoalSelector::default();
            goals.add(1, MeleeAttack::new(1.0, true));
            tick(&mut goals, &mut m, &world);
            let started = goals.is_running("MeleeAttack");
            world.player.as_mut().unwrap().spectator = true;
            tick(&mut goals, &mut m, &world);
            outcomes = Some((started, goals.is_running("MeleeAttack")));
        });
        if !ran {
            return;
        }
        assert_eq!(
            outcomes,
            Some((true, false)),
            "a mob chases a survival player and stops when they become a spectator"
        );
    }
}

#[cfg(test)]
mod breath_air_tests {
    use super::BreathAir;

    #[test]
    fn searches_neighbor_columns_in_vanilla_order() {
        let origin = [10, 20, 30];
        let air = [[10, 27, 29], [11, 21, 30], [10, 22, 31], [9, 23, 30]];
        assert_eq!(
            BreathAir::air_position(origin, |p| air.contains(&p)),
            Some([10, 27, 29])
        );
        assert_eq!(
            BreathAir::air_position(origin, |p| p == [10, 28, 30] || air.contains(&p)),
            Some([10, 28, 30])
        );
        for p in air {
            assert_eq!(BreathAir::air_position(origin, |q| q == p), Some(p));
        }
        // One block above the 9-block columns: not searched.
        assert_eq!(BreathAir::air_position(origin, |p| p == [10, 29, 30]), None);
    }
}

#[cfg(test)]
mod hurt_by_anger_tests {
    use super::*;
    use crate::mobs::world::{test_player, with_test_world};
    use horizon_core::geometry::Vec3;
    use std::sync::Arc;

    /// `HurtByTargetGoal.canUse` refuses a player when the `universal_anger`
    /// game rule is on: the anger target goals handle that hit instead.
    #[test]
    fn universal_anger_leaves_the_players_hit_to_the_anger_goals() {
        let mut outcomes = None;
        if !with_test_world(&[], |w, types| {
            let kind = Arc::new(types.types["minecraft:cow"].clone());
            let mut truth = Vec::new();
            for rule in [false, true] {
                let mut world = w.clone();
                world.player = Some(test_player(Vec3::new(2.5, 64.0, 0.5)));
                world.universal_anger = rule;
                let mut m = Mob::new(1, kind.clone(), Vec3::new(0.5, 64.0, 0.5), 0.0);
                m.ai.last_hurt_by = Some(Who::Player);
                m.ai.last_hurt_by_time = 1;
                m.ai.goals.add(1, HurtByTarget::new());
                let mut lr = LegacyRandom::new(0);
                let mut events = Vec::new();
                let mut cx = Cx {
                    w: &world,
                    lr: &mut lr,
                    events: &mut events,
                };
                let mut goals = std::mem::take(&mut m.ai.goals);
                goals.tick(&mut m, &mut cx);
                truth.push(goals.is_running("HurtByTarget"));
                m.ai.goals = goals;
            }
            outcomes = Some(truth);
        }) {
            return;
        }
        assert_eq!(
            outcomes,
            Some(vec![true, false]),
            "a hit is retaliated with the rule off and left to the anger goals with it on"
        );
    }
}
