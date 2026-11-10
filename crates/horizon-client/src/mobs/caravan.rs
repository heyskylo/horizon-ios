//! Llama caravans and the trader's llamas: `LlamaFollowCaravanGoal` (a
//! llama joins the nearest line of llamas that starts at a leashed one and
//! follows the llama ahead), `TraderLlamaDefendWanderingTraderGoal` and the
//! trader llama's despawn.
use super::{
    Mob, MobWorld, Seen, Species, Who,
    goals::{Cx, Goal, TARGET, TargetState, goal_clone, reduced, target_continue},
    llama::is_llama,
};
use horizon_core::geometry::Vec3;

const CARAVAN_LIMIT: i32 = 8;

/// The id plus one of the llama this one follows, 0 when not in a caravan
/// (`Llama.caravanHead`, kept in `ints[1]`).
pub fn head_of(m: &Mob) -> u32 {
    if is_llama(m.species()) {
        m.extra.ints[1].max(0) as u32
    } else {
        0
    }
}
fn in_caravan(m: &Mob) -> bool {
    head_of(m) != 0
}
fn is_leashed_seen(s: &Seen) -> bool {
    s.leash.is_some()
}
/// Whether another llama follows `s` (`hasCaravanTail`).
fn has_tail(cx: &Cx, s: &Seen) -> bool {
    cx.w.others
        .iter()
        .any(|o| is_llama(o.species) && o.alive && o.caravan == s.id + 1)
}
/// `firstIsLeashed`: the line leads back to a leashed llama in at most
/// eight links.
fn first_is_leashed(cx: &Cx, caravan: u32, leashed: bool, counter: i32) -> bool {
    if counter > CARAVAN_LIMIT {
        return false;
    }
    let _ = leashed;
    if caravan == 0 {
        return false;
    }
    let Some(head) = cx.w.seen(caravan - 1) else {
        return false;
    };
    if is_leashed_seen(head) {
        true
    } else {
        first_is_leashed(cx, head.caravan, false, counter + 1)
    }
}
/// `LlamaFollowCaravanGoal`.
#[derive(Clone, Debug)]
pub struct FollowCaravan {
    speed: f64,
    dist_check: i32,
}
impl FollowCaravan {
    pub fn new(speed: f64) -> Self {
        Self {
            speed,
            dist_check: 0,
        }
    }
}
impl Goal for FollowCaravan {
    goal_clone!();
    fn name(&self) -> &'static str {
        "LlamaFollowCaravanGoal"
    }
    fn flags(&self) -> u8 {
        super::goals::MOVE
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if m.extra.leash.is_some() || in_caravan(m) {
            return false;
        }
        let area = m.bounds().inflate(Vec3::new(9.0, 4.0, 9.0));
        let mine = m.position;
        let id = m.id;
        let candidates: Vec<&Seen> = cx
            .w
            .others
            .iter()
            .filter(|s| s.id != id && is_llama(s.species) && s.alive && s.bounds.intersects(area))
            .collect();
        let mut closest: Option<&Seen> = None;
        let mut best = f64::MAX;
        for c in &candidates {
            if c.caravan != 0 && !has_tail(cx, c) {
                let d = mine.distance_squared(c.position);
                if d <= best {
                    best = d;
                    closest = Some(c);
                }
            }
        }
        if closest.is_none() {
            for c in &candidates {
                if is_leashed_seen(c) && !has_tail(cx, c) {
                    let d = mine.distance_squared(c.position);
                    if d <= best {
                        best = d;
                        closest = Some(c);
                    }
                }
            }
        }
        let Some(head) = closest else {
            return false;
        };
        if best < 4.0 {
            return false;
        }
        if !is_leashed_seen(head) && !first_is_leashed(cx, head.caravan, false, 1) {
            return false;
        }
        m.extra.ints[1] = head.id as i32 + 1;
        true
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        let caravan = head_of(m);
        if caravan == 0 {
            return false;
        }
        let Some(head) = cx.w.seen(caravan - 1).filter(|h| h.alive) else {
            return false;
        };
        if !first_is_leashed(cx, caravan, false, 0) {
            return false;
        }
        // Two llamas that joined the same one in the same tick: the one with
        // the higher id leaves and looks for another place.
        if cx
            .w
            .others
            .iter()
            .any(|o| o.id < m.id && is_llama(o.species) && o.alive && o.caravan == caravan)
        {
            return false;
        }
        let dist_sqr = m.position.distance_squared(head.position);
        if dist_sqr > 676.0 {
            if self.speed <= 3.0 {
                self.speed *= 1.2;
                self.dist_check = reduced(40);
                return true;
            }
            if self.dist_check == 0 {
                return false;
            }
        }
        if self.dist_check > 0 {
            self.dist_check -= 1;
        }
        true
    }
    fn stop(&mut self, m: &mut Mob, _: &mut Cx) {
        m.extra.ints[1] = 0;
        // A double 2.1 here, while the llama built it with 2.1F.
        self.speed = 2.1;
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let caravan = head_of(m);
        if caravan == 0 {
            return;
        }
        // A llama leashed to a fence knot does not walk to the one ahead.
        if matches!(m.extra.leash, Some(super::leash::Holder::Knot(_))) {
            return;
        }
        let Some(head) = cx.w.seen(caravan - 1) else {
            return;
        };
        let to = Vec3::new(
            head.position.x - m.position.x,
            head.position.y - m.position.y,
            head.position.z - m.position.z,
        );
        let distance = to.length();
        let delta = to.normalize().scale((distance - 2.0).max(0.0));
        m.nav_move_to(
            cx.w,
            [
                m.position.x + delta.x,
                m.position.y + delta.y,
                m.position.z + delta.z,
            ],
            self.speed,
        );
    }
}
/// `TraderLlamaDefendWanderingTraderGoal`: attack whoever last hurt the
/// wandering trader holding this llama's lead.
#[derive(Clone, Debug, Default)]
pub struct DefendTrader {
    attacker: Option<Who>,
    timestamp: i32,
    st: TargetState,
}
impl Goal for DefendTrader {
    goal_clone!();
    fn flags(&self) -> u8 {
        TARGET
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        let Some(super::leash::Holder::Mob(id)) = m.extra.leash else {
            return false;
        };
        let Some(owner) =
            cx.w.seen(id)
                .filter(|s| s.species == Species::WanderingTrader)
        else {
            return false;
        };
        let Some((by, stamp)) = owner.hurt_by else {
            return false;
        };
        if stamp == self.timestamp {
            return false;
        }
        let Some(t) = cx.w.target_info(by) else {
            return false;
        };
        if !t.alive
            || t.is_player && (t.creative || cx.w.player.as_ref().is_some_and(|p| p.spectator))
        {
            return false;
        }
        let follow = m.attrs.follow_range;
        if m.distance_sqr(t.position) > follow * follow || !m.sees(cx.w, by) {
            return false;
        }
        self.attacker = Some(by);
        true
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        target_continue(&mut self.st, false, m, cx)
    }
    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        m.ai.target = self.attacker;
        if let Some(super::leash::Holder::Mob(id)) = m.extra.leash
            && let Some(owner) = cx.w.seen(id)
            && let Some((_, stamp)) = owner.hurt_by
        {
            self.timestamp = stamp;
        }
    }
    fn stop(&mut self, m: &mut Mob, _: &mut Cx) {
        m.ai.target = None;
    }
}
/// `TraderLlama.maybeDespawn`: counts down its own delay, or follows the
/// trader's when led by one, unless tamed, led by something else, ridden,
/// age-locked or kept.
pub fn trader_llama_tick(m: &mut Mob, w: &MobWorld) {
    let trader = match m.extra.leash {
        Some(super::leash::Holder::Mob(id)) => w
            .seen(id)
            .filter(|s| s.species == Species::WanderingTrader)
            .map(|s| s.delay),
        _ => None,
    };
    let leashed_other = m.extra.leash.is_some() && trader.is_none();
    if m.extra.tame || leashed_other || m.rider.is_some() || m.extra.age_locked || m.persistent {
        return;
    }
    m.extra.ints[0] = trader.unwrap_or(m.extra.ints[0]) - 1;
    if m.extra.ints[0] <= 0 {
        super::leash::remove(m);
        m.removed = true;
    }
}
