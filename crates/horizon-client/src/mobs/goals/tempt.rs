//! `TemptGoal`: a mob follows a player holding its food.
use super::*;

/// TemptGoal: follow a player holding one of the items.
#[derive(Clone, Debug)]
pub struct Tempt {
    pub speed: f64,
    pub items: Arc<[String]>,
    pub can_scare: bool,
    pub stop_distance: f64,
    p: [f64; 3],
    rot: [f64; 2],
    calm_down: i32,
}

impl Goal for Tempt {
    goal_clone!();
    fn flags(&self) -> u8 {
        MOVE | LOOK
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if self.calm_down > 0 {
            self.calm_down -= 1;
            return false;
        }
        let Some(p) = &cx.w.player else { return false };
        // Attributes.TEMPT_RANGE default 10, scaled by the player's
        // visibility (`TargetingConditions.forNonCombat`, floor 2).
        let range = (10.0 * p.visibility_percent(Some(m.kind.id.as_str()), cx.w.items)).max(2.0);
        p.alive
            && !p.spectator
            && (self.wants(&p.main_hand) || self.wants(&p.off_hand))
            && m.distance_sqr(p.position) <= range * range
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if self.can_scare
            && let Some(p) = &cx.w.player
        {
            if m.distance_sqr(p.position) < 36.0 {
                let moved = (p.position.x - self.p[0]).powi(2)
                    + (p.position.y - self.p[1]).powi(2)
                    + (p.position.z - self.p[2]).powi(2);
                if moved > 0.010_000_000_000_000_002 {
                    return false;
                }
                let (yaw, pitch) = look_angles(p.look);
                if (pitch - self.rot[0]).abs() > 5.0 || (yaw - self.rot[1]).abs() > 5.0 {
                    return false;
                }
            } else {
                self.p = [p.position.x, p.position.y, p.position.z];
            }
            let (yaw, pitch) = look_angles(p.look);
            self.rot = [pitch, yaw];
        }
        self.can_use(m, cx)
    }
    fn start(&mut self, _: &mut Mob, cx: &mut Cx) {
        if let Some(p) = &cx.w.player {
            self.p = [p.position.x, p.position.y, p.position.z];
        }
    }
    fn stop(&mut self, m: &mut Mob, _: &mut Cx) {
        m.nav_stop();
        self.calm_down = reduced(100);
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let Some(p) = cx.w.player.clone() else { return };
        let (y, x) = (m.kind.max_head_y_rot + 20.0, m.max_head_x_rot());
        m.look_at_speed(p.eye, y, x);
        if m.distance_sqr(p.position) < self.stop_distance * self.stop_distance {
            m.nav_stop();
        } else {
            m.nav_move_to_entity(
                cx.w,
                [p.position.x, p.position.y, p.position.z],
                1,
                self.speed,
            );
        }
    }
}

impl Tempt {
    pub fn new(speed: f64, items: Arc<[String]>, can_scare: bool) -> Self {
        Self {
            speed,
            items,
            can_scare,
            stop_distance: 2.5,
            p: [0.0; 3],
            rot: [0.0; 2],
            calm_down: 0,
        }
    }
    fn wants(&self, item: &Option<Arc<str>>) -> bool {
        item.as_ref()
            .is_some_and(|i| self.items.iter().any(|t| t.as_str() == &**i))
    }
}
