//! Player difficulty/ability gates and server PvP dispatch before living hurt.
use crate::{
    food::Difficulty,
    living_hurt::{self, HurtState, HurtWorld},
};
pub trait PlayerHurtWorld: HurtWorld {
    fn ability_invulnerable(&self) -> bool;
    fn bypasses_invulnerability(
        &mut self,
        s: &mut HurtState<Self::Source>,
    ) -> Result<bool, Self::Error>;
    fn remove_shoulders(&mut self, s: &mut HurtState<Self::Source>) -> Result<(), Self::Error>;
    fn scales_with_difficulty(
        &mut self,
        s: &mut HurtState<Self::Source>,
    ) -> Result<bool, Self::Error>;
    fn difficulty(&mut self, s: &mut HurtState<Self::Source>) -> Result<Difficulty, Self::Error>;
}
fn min(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == 0.0 && b == 0.0 {
        f32::from_bits(a.to_bits() | b.to_bits())
    } else {
        a.min(b)
    }
}
pub fn player_hurt_server<W: PlayerHurtWorld>(
    s: &mut HurtState<W::Source>,
    w: &mut W,
    mut damage: f32,
) -> Result<bool, W::Error> {
    if w.invulnerable(s)? {
        return Ok(false);
    }
    if w.ability_invulnerable() && !w.bypasses_invulnerability(s)? {
        return Ok(false);
    }
    s.no_action_time = 0;
    if w.dead_or_dying(s)? {
        return Ok(false);
    }
    w.remove_shoulders(s)?;
    if w.scales_with_difficulty(s)? {
        if w.difficulty(s)? == Difficulty::Peaceful {
            damage = 0.0;
        }
        if w.difficulty(s)? == Difficulty::Easy {
            damage = min(damage / 2.0 + 1.0, damage);
        }
        if w.difficulty(s)? == Difficulty::Hard {
            damage = damage * 3.0 / 2.0;
        }
    }
    if damage == 0.0 {
        return Ok(false);
    }
    living_hurt::hurt_server(s, w, damage)
}
#[derive(Clone, Copy, Debug)]
pub enum CausingEntity<P, A> {
    Other,
    Player(P),
    Arrow(A),
}
pub trait ServerPlayerHurtWorld: PlayerHurtWorld {
    type Player: Copy;
    type Arrow: Copy;
    fn causing_entity(
        &mut self,
        s: &mut HurtState<Self::Source>,
    ) -> Result<CausingEntity<Self::Player, Self::Arrow>, Self::Error>;
    fn arrow_player_owner(
        &mut self,
        s: &mut HurtState<Self::Source>,
        arrow: Self::Arrow,
    ) -> Result<Option<Self::Player>, Self::Error>;
    fn can_harm_player(
        &mut self,
        s: &mut HurtState<Self::Source>,
        player: Self::Player,
    ) -> Result<bool, Self::Error>;
}
pub fn server_player_hurt_server<W: ServerPlayerHurtWorld>(
    s: &mut HurtState<W::Source>,
    w: &mut W,
    damage: f32,
) -> Result<bool, W::Error> {
    if w.invulnerable(s)? {
        return Ok(false);
    }
    match w.causing_entity(s)? {
        CausingEntity::Player(p) => {
            if !w.can_harm_player(s, p)? {
                return Ok(false);
            }
        }
        CausingEntity::Arrow(a) => {
            if let Some(p) = w.arrow_player_owner(s, a)?
                && !w.can_harm_player(s, p)?
            {
                return Ok(false);
            }
        }
        CausingEntity::Other => {}
    }
    player_hurt_server(s, w, damage)
}
