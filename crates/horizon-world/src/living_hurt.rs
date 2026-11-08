//! Outer living damage dispatch, cooldown writes and post-hit service ordering.
use crate::living_damage::{STAT_LIMIT, stat_amount};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HurtTag {
    Fire,
    Freezing,
    DamagesHelmet,
    BypassesCooldown,
    NoImpact,
    NoKnockback,
}
#[derive(Clone, Debug)]
pub struct HurtState<S> {
    pub no_action_time: i32,
    pub cooldown: i32,
    pub last_hurt: f32,
    pub hurt_duration: i32,
    pub hurt_time: i32,
    pub last_source: Option<S>,
    pub last_stamp: i64,
}
/// One invocation retains the same incoming source and server level. Services
/// receive live state so reentrant callbacks preserve native write ordering.
pub trait HurtWorld {
    type Error;
    type Source: Copy;
    type UseItem: Copy;
    type Blocking: Copy;
    type Sound: Copy;
    fn source(&self) -> Self::Source;
    fn is_server_player(&self) -> bool;
    fn invulnerable(&mut self, s: &mut HurtState<Self::Source>) -> Result<bool, Self::Error>;
    fn dead_or_dying(&mut self, s: &mut HurtState<Self::Source>) -> Result<bool, Self::Error>;
    fn tag(&mut self, s: &mut HurtState<Self::Source>, tag: HurtTag) -> Result<bool, Self::Error>;
    fn fire_resistance(&mut self, s: &mut HurtState<Self::Source>) -> Result<bool, Self::Error>;
    /// The hit once the guards above passed: it may change `damage`, and
    /// true refuses the hit. The default does nothing.
    fn hurt_start(
        &mut self,
        _s: &mut HurtState<Self::Source>,
        _damage: &mut f32,
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }
    fn sleeping(&mut self, s: &mut HurtState<Self::Source>) -> Result<bool, Self::Error>;
    fn stop_sleeping(&mut self, s: &mut HurtState<Self::Source>) -> Result<(), Self::Error>;
    fn use_item(&mut self, s: &mut HurtState<Self::Source>) -> Result<Self::UseItem, Self::Error>;
    fn apply_blocking(
        &mut self,
        s: &mut HurtState<Self::Source>,
        damage: f32,
    ) -> Result<f32, Self::Error>;
    fn freeze_hurts_extra(&mut self, s: &mut HurtState<Self::Source>) -> Result<bool, Self::Error>;
    fn helmet_empty(&mut self, s: &mut HurtState<Self::Source>) -> Result<bool, Self::Error>;
    fn hurt_helmet(
        &mut self,
        s: &mut HurtState<Self::Source>,
        damage: f32,
    ) -> Result<(), Self::Error>;
    fn actually_hurt(
        &mut self,
        s: &mut HurtState<Self::Source>,
        damage: f32,
    ) -> Result<(), Self::Error>;
    fn resolve_mob(&mut self, s: &mut HurtState<Self::Source>) -> Result<(), Self::Error>;
    fn resolve_player(&mut self, s: &mut HurtState<Self::Source>) -> Result<(), Self::Error>;
    fn blocking_component(&mut self, item: Self::UseItem) -> Option<Self::Blocking>;
    fn on_blocked(
        &mut self,
        s: &mut HurtState<Self::Source>,
        block: Self::Blocking,
    ) -> Result<(), Self::Error>;
    fn broadcast_damage(&mut self, s: &mut HurtState<Self::Source>) -> Result<(), Self::Error>;
    fn mark_hurt(&mut self, s: &mut HurtState<Self::Source>) -> Result<(), Self::Error>;
    fn knockback(
        &mut self,
        s: &mut HurtState<Self::Source>,
        damage: f32,
        blocked: bool,
    ) -> Result<(), Self::Error>;
    fn death_protection(&mut self, s: &mut HurtState<Self::Source>) -> Result<bool, Self::Error>;
    fn death_sound(&mut self, s: &mut HurtState<Self::Source>) -> Result<Self::Sound, Self::Error>;
    fn make_sound(
        &mut self,
        s: &mut HurtState<Self::Source>,
        sound: Self::Sound,
    ) -> Result<(), Self::Error>;
    fn secondary_sound(&mut self, s: &mut HurtState<Self::Source>) -> Result<(), Self::Error>;
    fn die(&mut self, s: &mut HurtState<Self::Source>) -> Result<(), Self::Error>;
    fn hurt_sound(&mut self, s: &mut HurtState<Self::Source>) -> Result<(), Self::Error>;
    fn game_time(&mut self, s: &mut HurtState<Self::Source>) -> Result<i64, Self::Error>;
    fn effects_on_hurt(
        &mut self,
        s: &mut HurtState<Self::Source>,
        damage: f32,
    ) -> Result<(), Self::Error>;
    fn player_hurt_criterion(
        &mut self,
        s: &mut HurtState<Self::Source>,
        original: f32,
        damage: f32,
        blocked: bool,
    ) -> Result<(), Self::Error>;
    fn award_blocked(
        &mut self,
        s: &mut HurtState<Self::Source>,
        amount: i32,
    ) -> Result<(), Self::Error>;
    /// Looks up the attacker, then dispatches its criterion if a server player.
    fn attacker_criterion(
        &mut self,
        s: &mut HurtState<Self::Source>,
        original: f32,
        damage: f32,
        blocked: bool,
    ) -> Result<(), Self::Error>;
}
/// Outer-hit values retained while equipment services run with their owner.
pub struct PreparedHurt<I> {
    pub damage: f32,
    original: f32,
    item: I,
    blocked_amount: f32,
}
/// The cooldown decision is made before inner equipment wear and retained even
/// if a durability callback changes the player's cooldown.
pub struct PendingHurt<I> {
    pub damage: f32,
    prepared: PreparedHurt<I>,
    fresh: bool,
}
pub fn prepare_hurt_server<W: HurtWorld>(
    s: &mut HurtState<W::Source>,
    w: &mut W,
    mut damage: f32,
) -> Result<Option<PreparedHurt<W::UseItem>>, W::Error> {
    if w.invulnerable(s)? || w.dead_or_dying(s)? {
        return Ok(None);
    }
    if w.tag(s, HurtTag::Fire)? && w.fire_resistance(s)? {
        return Ok(None);
    }
    if w.hurt_start(s, &mut damage)? {
        return Ok(None);
    }
    if w.sleeping(s)? {
        w.stop_sleeping(s)?;
    }
    s.no_action_time = 0;
    if damage < 0.0 {
        damage = 0.0;
    }
    let original = damage;
    let item = w.use_item(s)?;
    let blocked_amount = w.apply_blocking(s, damage)?;
    damage -= blocked_amount;
    if w.tag(s, HurtTag::Freezing)? && w.freeze_hurts_extra(s)? {
        damage *= 5.0;
    }
    Ok(Some(PreparedHurt {
        damage,
        original,
        item,
        blocked_amount,
    }))
}
pub fn hurt_helmet<W: HurtWorld>(
    s: &mut HurtState<W::Source>,
    w: &mut W,
    prepared: &mut PreparedHurt<W::UseItem>,
) -> Result<(), W::Error> {
    if w.tag(s, HurtTag::DamagesHelmet)? && !w.helmet_empty(s)? {
        w.hurt_helmet(s, prepared.damage)?;
        prepared.damage *= 0.75;
    }
    Ok(())
}
pub fn select_hurt_server<W: HurtWorld>(
    s: &mut HurtState<W::Source>,
    w: &mut W,
    mut prepared: PreparedHurt<W::UseItem>,
) -> Result<Option<PendingHurt<W::UseItem>>, W::Error> {
    if !prepared.damage.is_finite() {
        prepared.damage = f32::MAX;
    }
    let fresh = !(s.cooldown as f32 > 10.0 && !w.tag(s, HurtTag::BypassesCooldown)?);
    let damage = if fresh {
        s.last_hurt = prepared.damage;
        s.cooldown = 20;
        prepared.damage
    } else {
        if prepared.damage <= s.last_hurt {
            return Ok(None);
        }
        prepared.damage - s.last_hurt
    };
    Ok(Some(PendingHurt {
        damage,
        prepared,
        fresh,
    }))
}
/// Complete an outer hit after its selected inner hit has run.
pub fn finish_hurt_server<W: HurtWorld>(
    s: &mut HurtState<W::Source>,
    w: &mut W,
    pending: PendingHurt<W::UseItem>,
) -> Result<bool, W::Error> {
    let PendingHurt {
        prepared, fresh, ..
    } = pending;
    let PreparedHurt {
        damage,
        original,
        item,
        blocked_amount,
    } = prepared;
    let blocked = blocked_amount > 0.0;
    if fresh {
        s.hurt_duration = 10;
        s.hurt_time = s.hurt_duration;
    } else {
        s.last_hurt = damage;
    }
    w.resolve_mob(s)?;
    w.resolve_player(s)?;
    if fresh {
        let component = w.blocking_component(item);
        if let Some(component) = component.filter(|_| blocked) {
            w.on_blocked(s, component)?;
        } else {
            w.broadcast_damage(s)?;
        }
        if !w.tag(s, HurtTag::NoImpact)? && (!blocked || damage > 0.0) {
            w.mark_hurt(s)?;
        }
        let fully_blocked = blocked && damage <= 0.0;
        if !w.tag(s, HurtTag::NoKnockback)? && !fully_blocked {
            w.knockback(s, damage, blocked)?;
        }
    }
    if w.dead_or_dying(s)? {
        if !w.death_protection(s)? {
            if fresh {
                let sound = w.death_sound(s)?;
                w.make_sound(s, sound)?;
                w.secondary_sound(s)?;
            }
            w.die(s)?;
        }
    } else if fresh {
        w.hurt_sound(s)?;
        w.secondary_sound(s)?;
    }
    let accepted = !blocked || damage > 0.0;
    if accepted {
        s.last_source = Some(w.source());
        s.last_stamp = w.game_time(s)?;
        w.effects_on_hurt(s, damage)?;
    }
    if w.is_server_player() {
        w.player_hurt_criterion(s, original, damage, blocked)?;
        if blocked_amount > 0.0 && blocked_amount < STAT_LIMIT {
            w.award_blocked(s, stat_amount(blocked_amount))?;
        }
    }
    w.attacker_criterion(s, original, damage, blocked)?;
    Ok(accepted)
}

pub fn hurt_server<W: HurtWorld>(
    s: &mut HurtState<W::Source>,
    w: &mut W,
    damage: f32,
) -> Result<bool, W::Error> {
    let Some(mut prepared) = prepare_hurt_server(s, w, damage)? else {
        return Ok(false);
    };
    hurt_helmet(s, w, &mut prepared)?;
    let Some(pending) = select_hurt_server(s, w, prepared)? else {
        return Ok(false);
    };
    w.actually_hurt(s, pending.damage)?;
    finish_hurt_server(s, w, pending)
}
