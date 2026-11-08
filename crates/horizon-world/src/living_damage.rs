//! Inner living/player damage application after the outer hurt gates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageStat {
    Absorbed,
    DealtAbsorbed,
    Taken,
}
pub trait DamageWorld {
    type Error;
    type Attacker: Copy;
    fn invulnerable(&mut self) -> Result<bool, Self::Error>;
    fn after_armor(&mut self, damage: f32) -> Result<f32, Self::Error>;
    fn after_magic(&mut self, damage: f32) -> Result<f32, Self::Error>;
    fn absorption(&mut self) -> Result<f32, Self::Error>;
    fn set_absorption(&mut self, value: f32) -> Result<(), Self::Error>;
    fn server_player_attacker(&mut self) -> Result<Option<Self::Attacker>, Self::Error>;
    fn award_attacker(
        &mut self,
        attacker: Self::Attacker,
        stat: DamageStat,
        amount: i32,
    ) -> Result<(), Self::Error>;
    fn award_self(&mut self, stat: DamageStat, amount: i32) -> Result<(), Self::Error>;
    fn source_food_exhaustion(&mut self) -> Result<f32, Self::Error>;
    fn cause_food_exhaustion(&mut self, amount: f32) -> Result<(), Self::Error>;
    fn record_damage(&mut self, amount: f32) -> Result<(), Self::Error>;
    fn health(&mut self) -> Result<f32, Self::Error>;
    fn set_health(&mut self, value: f32) -> Result<(), Self::Error>;
    fn damage_event(&mut self) -> Result<(), Self::Error>;
    /// Horizon's hook point, not vanilla's: what is about to come off the
    /// health once armour, enchantments, effects and absorption took their
    /// part. It may change `damage`. The default does nothing.
    fn damage_pre(&mut self, _damage: &mut f32) -> Result<(), Self::Error> {
        Ok(())
    }
    /// Horizon's hook point: the hit is done, and `damage` came off the
    /// health (0 when nothing did). The default does nothing.
    fn damage_post(&mut self, _damage: f32) -> Result<(), Self::Error> {
        Ok(())
    }
}
pub(crate) fn positive(value: f32) -> f32 {
    if value.is_nan() {
        value
    } else if value <= 0.0 {
        0.0
    } else {
        value
    }
}
pub(crate) fn stat_amount(value: f32) -> i32 {
    let value = value * 10.0;
    // Java Math.round(float), including negative ties and integer saturation.
    let bits = value.to_bits() as i32;
    let shift = 149 - ((bits >> 23) & 0xff);
    if (0..32).contains(&shift) {
        let mut significand = (bits & 0x7f_ffff) | 0x80_0000;
        if bits < 0 {
            significand = -significand;
        }
        ((significand >> shift) + 1) >> 1
    } else {
        value as i32
    }
}
pub(crate) const STAT_LIMIT: f32 = 3.402_823_5e37;
pub fn actually_hurt<W: DamageWorld>(
    world: &mut W,
    damage: f32,
    player: bool,
) -> Result<(), W::Error> {
    if world.invulnerable()? {
        return Ok(());
    }
    actually_hurt_after_immunity_check(world, damage, player)
}
/// Caller must have performed this hit's native invulnerability query.
pub(crate) fn actually_hurt_after_immunity_check<W: DamageWorld>(
    world: &mut W,
    damage: f32,
    player: bool,
) -> Result<(), W::Error> {
    let damage = world.after_armor(damage)?;
    actually_hurt_after_armor(world, damage, player)
}
/// Resume an inner hit after synchronous equipment wear and armor reduction.
pub(crate) fn actually_hurt_after_armor<W: DamageWorld>(
    world: &mut W,
    damage: f32,
    player: bool,
) -> Result<(), W::Error> {
    let damage = world.after_magic(damage)?;
    let mut remaining = positive(damage - world.absorption()?);
    let absorption = world.absorption()? - (damage - remaining);
    world.set_absorption(absorption)?;
    let absorbed = damage - remaining;
    if absorbed > 0.0 && absorbed < STAT_LIMIT {
        if player {
            world.award_self(DamageStat::Absorbed, stat_amount(absorbed))?;
        } else if let Some(attacker) = world.server_player_attacker()? {
            world.award_attacker(attacker, DamageStat::DealtAbsorbed, stat_amount(absorbed))?;
        }
    }
    world.damage_pre(&mut remaining)?;
    if remaining == 0.0 {
        return world.damage_post(0.0);
    }
    if player {
        let exhaustion = world.source_food_exhaustion()?;
        world.cause_food_exhaustion(exhaustion)?;
    }
    world.record_damage(remaining)?;
    let health = world.health()?;
    world.set_health(health - remaining)?;
    if player {
        if remaining < STAT_LIMIT {
            world.award_self(DamageStat::Taken, stat_amount(remaining))?;
        }
    } else {
        let absorption = world.absorption()?;
        world.set_absorption(absorption - remaining)?;
    }
    world.damage_event()?;
    world.damage_post(remaining)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;

    /// A hit whose armour and magic take nothing, and what the hook points
    /// were handed; `set` is what Pre turns the amount into.
    #[derive(Default)]
    struct World {
        health: f32,
        absorption: f32,
        set: Option<f32>,
        recorded: Vec<f32>,
        pre: Vec<f32>,
        post: Vec<f32>,
    }
    impl DamageWorld for World {
        type Error = Infallible;
        type Attacker = ();
        fn invulnerable(&mut self) -> Result<bool, Infallible> {
            Ok(false)
        }
        fn after_armor(&mut self, damage: f32) -> Result<f32, Infallible> {
            Ok(damage)
        }
        fn after_magic(&mut self, damage: f32) -> Result<f32, Infallible> {
            Ok(damage)
        }
        fn absorption(&mut self) -> Result<f32, Infallible> {
            Ok(self.absorption)
        }
        fn set_absorption(&mut self, value: f32) -> Result<(), Infallible> {
            self.absorption = value;
            Ok(())
        }
        fn server_player_attacker(&mut self) -> Result<Option<()>, Infallible> {
            Ok(None)
        }
        fn award_attacker(&mut self, _: (), _: DamageStat, _: i32) -> Result<(), Infallible> {
            Ok(())
        }
        fn award_self(&mut self, _: DamageStat, _: i32) -> Result<(), Infallible> {
            Ok(())
        }
        fn source_food_exhaustion(&mut self) -> Result<f32, Infallible> {
            Ok(0.0)
        }
        fn cause_food_exhaustion(&mut self, _: f32) -> Result<(), Infallible> {
            Ok(())
        }
        fn record_damage(&mut self, amount: f32) -> Result<(), Infallible> {
            self.recorded.push(amount);
            Ok(())
        }
        fn health(&mut self) -> Result<f32, Infallible> {
            Ok(self.health)
        }
        fn set_health(&mut self, value: f32) -> Result<(), Infallible> {
            self.health = value;
            Ok(())
        }
        fn damage_event(&mut self) -> Result<(), Infallible> {
            Ok(())
        }
        fn damage_pre(&mut self, damage: &mut f32) -> Result<(), Infallible> {
            self.pre.push(*damage);
            if let Some(set) = self.set {
                *damage = set;
            }
            Ok(())
        }
        fn damage_post(&mut self, damage: f32) -> Result<(), Infallible> {
            self.post.push(damage);
            Ok(())
        }
    }

    fn hit(damage: f32, absorption: f32, set: Option<f32>) -> World {
        let mut world = World {
            health: 20.0,
            absorption,
            set,
            ..World::default()
        };
        let Ok(()) = actually_hurt(&mut world, damage, true);
        world
    }

    #[test]
    fn the_hook_points_see_and_change_what_the_health_takes() {
        // Absorption takes 4 of 10: Pre is handed the other 6 and makes them 3.
        let w = hit(10.0, 4.0, Some(3.0));
        assert_eq!((w.pre, w.post), (vec![6.0], vec![3.0]));
        assert_eq!((w.health, w.absorption, w.recorded), (17.0, 0.0, vec![3.0]));
        // Absorption takes it all: Post still hears, with 0, and nothing is
        // recorded.
        let w = hit(3.0, 4.0, None);
        assert_eq!((w.pre, w.post), (vec![0.0], vec![0.0]));
        assert_eq!((w.health, w.absorption, w.recorded.len()), (20.0, 1.0, 0));
        // Pre can make such a hit hurt, or a hurting one take nothing.
        let w = hit(3.0, 4.0, Some(2.0));
        assert_eq!((w.post, w.health, w.recorded), (vec![2.0], 18.0, vec![2.0]));
        let w = hit(10.0, 0.0, Some(0.0));
        assert_eq!((w.post, w.health, w.recorded.len()), (vec![0.0], 20.0, 0));
    }
}
