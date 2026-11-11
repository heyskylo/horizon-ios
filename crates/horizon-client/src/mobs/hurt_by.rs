//! The mob that last hurt a mob (`LivingEntity.lastHurtByMob`): its record in
//! `Ai`, its end in `baseTick`, and what a hit or a stop being angry does to it.
use super::ai::Ai;
use super::{Mob, MobWorld, Who};

// `LivingEntity.lastHurtByMob` is a lazy `EntityReference`: a record read back
// from `last_hurt_by_mob` stays the bare UUID (`Ai::last_hurt_by_ref`) until the
// game resolves it into `last_hurt_by`. `getLastHurtByMob` answers nothing for
// it and `baseTick` does not end it; its timestamp keeps aging.
impl Ai {
    /// `LivingEntity.setLastHurtByMob(who)`: the new record replaces a
    /// reference read back that was not resolved.
    pub fn hurt_by(&mut self, who: Who, tick: i32) {
        self.last_hurt_by = Some(who);
        self.last_hurt_by_ref = None;
        self.last_hurt_by_time = tick;
    }
    /// `LivingEntity.setLastHurtByMob(null)`, which also stamps the time.
    pub fn forget_hurt_by(&mut self, tick: i32) {
        self.last_hurt_by = None;
        self.last_hurt_by_ref = None;
        self.last_hurt_by_time = tick;
    }
}
/// `LivingEntity.baseTick`'s hurt timers: `hurtTime` and `invulnerableTime`
/// count down, and the mob that hurt it is forgotten once it is not alive or
/// after more than 100 ticks. One that is not in the level
/// (`getLastHurtByMob` is null) goes on.
pub(super) fn base_tick(m: &mut Mob, w: &MobWorld) {
    if m.hurt_time > 0 {
        m.hurt_time -= 1;
    }
    if m.invulnerable > 0 {
        m.invulnerable -= 1;
    }
    if let Some(who) = m.ai.last_hurt_by
        && let Some(by) = w.target_info(who)
        && (!by.alive || (m.age as i32).wrapping_sub(m.ai.last_hurt_by_time) > 100)
    {
        m.ai.forget_hurt_by(m.age as i32);
    }
}
