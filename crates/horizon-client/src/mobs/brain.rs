//! Pieces of vanilla's `Brain` that species implemented as brains share:
//! memories that expire, behaviour durations, walk targets and
//! `MoveToTargetSink`.
use super::{Mob, MobWorld, Who, goals::Cx};
use horizon_core::{
    geometry::Vec3,
    random::{LegacyRandom, RandomSource},
};

/// A memory with an expiry (`Brain.setMemoryWithExpiry`): present for the
/// tick it was set and `ttl` more ticks. `Ttl::default()` is absent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ttl(i32);
impl Ttl {
    pub fn set(&mut self, ttl: i32) {
        self.0 = ttl + 1;
    }
    pub fn erase(&mut self) {
        self.0 = 0;
    }
    pub fn present(self) -> bool {
        self.0 > 0
    }
    /// `Brain.forgetOutdatedMemories`, once at the start of each tick.
    pub fn tick(&mut self) {
        if self.0 > 0 {
            self.0 -= 1;
        }
    }
}
/// `Behavior`'s bookkeeping in the brains ported behaviour for behaviour
/// (villager, animal, monster, warden): running, and until when.
#[derive(Clone, Copy, Debug)]
pub struct Timed {
    pub running: bool,
    pub end: i64,
    pub min: i32,
    pub max: i32,
}
impl Timed {
    pub fn new(min: i32, max: i32) -> Self {
        Self {
            running: false,
            end: 0,
            min,
            max,
        }
    }
    /// `Behavior.tryStart`'s duration draw from the level random, in Java's
    /// int arithmetic (made even when it cannot vary).
    pub fn begin(&mut self, lr: &mut LegacyRandom, now: i64) {
        self.running = true;
        let bound = self.max.wrapping_add(1).wrapping_sub(self.min);
        let d = self.min.wrapping_add(lr.next_int_bound(bound).unwrap_or(0));
        self.end = now + i64::from(d);
    }
    /// `Behavior.timedOut`.
    pub fn timed_out(&self, now: i64) -> bool {
        now > self.end
    }
}
/// `NearestLivingEntitySensor`'s scan: the living entities (the player
/// among them) whose boxes meet the follow range around `m`, nearest first.
pub fn living_near(m: &Mob, w: &MobWorld) -> Vec<Who> {
    let r = m.attrs.follow_range;
    let area = m.bounds().inflate(Vec3::new(r, r, r));
    let mut found: Vec<(Who, f64)> = Vec::new();
    for o in w.others_near(area) {
        if o.id != m.id && o.alive && o.bounds.intersects(area) {
            found.push((Who::Mob(o.id), m.distance_sqr(o.position)));
        }
    }
    if let Some(p) = &w.player
        && p.alive
        && p.bounds.intersects(area)
    {
        found.push((Who::Player, m.distance_sqr(p.position)));
    }
    found.sort_by(|a, b| a.1.total_cmp(&b.1));
    found.into_iter().map(|(w, _)| w).collect()
}
/// A random duration in `min..=max` (`Behavior`'s end time).
pub fn duration(m: &mut Mob, min: i32, max: i32) -> i32 {
    if max <= min {
        return min;
    }
    min + m.random.next_int_bound(max - min + 1).unwrap_or(0)
}
/// `WalkTarget`: where to go, how fast and how close counts as there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WalkTarget {
    pub pos: [i32; 3],
    pub speed: f64,
    pub close: i32,
}
impl WalkTarget {
    pub fn new(pos: [i32; 3], speed: f64, close: i32) -> Self {
        Self { pos, speed, close }
    }
    /// `MoveToTargetSink.reachedTarget`: Manhattan distance of blocks.
    pub fn reached(&self, m: &Mob) -> bool {
        let here = [
            m.position.x.floor() as i32,
            m.position.y.floor() as i32,
            m.position.z.floor() as i32,
        ];
        let d = (self.pos[0] - here[0]).abs()
            + (self.pos[1] - here[1]).abs()
            + (self.pos[2] - here[2]).abs();
        d <= self.close
    }
}
/// `MoveToTargetSink(min, max)`: walks to the walk target and gives up on it
/// after a random time.
#[derive(Clone, Debug)]
pub struct Sink {
    min: i32,
    max: i32,
    until: Option<i32>,
}
impl Sink {
    pub fn new(min: i32, max: i32) -> Self {
        Self {
            min,
            max,
            until: None,
        }
    }
    pub fn running(&self) -> bool {
        self.until.is_some()
    }
    /// `stop`: forget the walk target and the path.
    pub fn stop(&mut self, m: &mut Mob, walk: &mut Option<WalkTarget>) {
        if self.until.take().is_some() {
            m.nav_stop();
        }
        *walk = None;
    }
    /// One tick of `checkExtraStartConditions`, `canStillUse`, `tick` and
    /// `stop` for the walk target.
    pub fn tick(&mut self, m: &mut Mob, cx: &mut Cx, walk: &mut Option<WalkTarget>) {
        let now = m.age as i32;
        if let Some(until) = self.until {
            let still = !m.ai.nav.is_done() && walk.is_some_and(|w| !w.reached(m));
            if now > until || !still {
                self.stop(m, walk);
            }
            return;
        }
        let Some(target) = *walk else { return };
        if target.reached(m) {
            *walk = None;
            return;
        }
        let at = [
            f64::from(target.pos[0]) + 0.5,
            f64::from(target.pos[1]),
            f64::from(target.pos[2]) + 0.5,
        ];
        let mut moving = m.nav_move_to(cx.w, at, target.speed);
        if !moving {
            // A partial step toward it (`DefaultRandomPos.getPosTowards`).
            moving = m
                .default_random_pos_towards(cx.w, 10, 7, at, std::f64::consts::FRAC_PI_2)
                .is_some_and(|p| m.nav_move_to(cx.w, p, target.speed));
        }
        if moving {
            self.until = Some(now + duration(m, self.min, self.max));
        } else {
            *walk = None;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_memory_lasts_its_expiry_and_the_tick_it_was_set() {
        let mut t = Ttl::default();
        assert!(!t.present());
        t.set(2);
        let mut ticks = 0;
        while t.present() {
            ticks += 1;
            t.tick();
        }
        assert_eq!(ticks, 3);
    }
}

use super::Mob;
use horizon_world::nbt::{Compound, Tag};

/// `finalizeSpawn`'s memories (`m.extra.memories`) as `Brain.Packed` writes
/// them: a `Unit` empty compound, a boolean, an integer, or a global position
/// for `home`, with the remaining TTL when it is finite.
pub fn put_brain(map: &mut Compound, m: &Mob, dimension: &str) {
    if m.extra.memories.is_empty() {
        return;
    }
    let mut memories = match map.get(&"Brain".into()).and_then(|b| b.get("memories")) {
        Some(Tag::Compound(c)) => c.clone(),
        _ => Compound::new(),
    };
    for &(name, value, ttl) in &m.extra.memories {
        let value = match name {
            "hunted_recently" => Tag::Byte(value as i8),
            // `Unit` memories save an empty compound.
            "is_pregnant" => Tag::Compound(Compound::new()),
            "home" => match m.extra.home {
                Some(p) => Tag::compound([
                    ("dimension", Tag::String(dimension.into())),
                    ("pos", Tag::IntArray(p.to_vec())),
                ]),
                None => continue,
            },
            _ => Tag::Int(value as i32),
        };
        let mut memory = vec![("value", value)];
        if ttl != i64::MAX {
            memory.push(("ttl", Tag::Long(ttl)));
        }
        memories.insert(
            format!("minecraft:{name}").as_str().into(),
            Tag::compound(memory),
        );
    }
    map.insert(
        "Brain".into(),
        Tag::compound([("memories", Tag::Compound(memories))]),
    );
}

/// `Frog`'s brain memory `IS_PREGNANT`, which `Brain.Packed` writes beside the
/// other memories and `mob_from_nbt` reads back into the frog's memories. The
/// memory is a `Unit`: its value is an empty compound, with no TTL.
pub fn write_frog(m: &Mob, map: &mut Compound) {
    let brain = map
        .entry("Brain".into())
        .or_insert_with(|| Tag::Compound(Compound::new()));
    if !matches!(brain, Tag::Compound(_)) {
        *brain = Tag::Compound(Compound::new());
    }
    let Tag::Compound(brain) = brain else { return };
    let memories = brain
        .entry("memories".into())
        .or_insert_with(|| Tag::Compound(Compound::new()));
    if !matches!(memories, Tag::Compound(_)) {
        *memories = Tag::Compound(Compound::new());
    }
    let Tag::Compound(memories) = memories else {
        return;
    };
    if super::frog::is_pregnant(m) {
        memories.insert(
            "minecraft:is_pregnant".into(),
            Tag::compound([("value", Tag::Compound(Compound::new()))]),
        );
    } else {
        memories.remove(&"minecraft:is_pregnant".into());
    }
}
