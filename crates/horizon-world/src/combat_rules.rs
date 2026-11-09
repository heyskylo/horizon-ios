//! Native armor/toughness and enchantment-protection damage arithmetic.
fn clamp(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else if value.is_nan() || max.is_nan() {
        f32::NAN
    } else if value == 0.0 && max == 0.0 {
        f32::from_bits(value.to_bits() | max.to_bits())
    } else {
        value.min(max)
    }
}
pub trait ArmorWorld {
    type Error;
    type Weapon: Copy;
    type ServerLevel: Copy;
    fn weapon(&mut self) -> Result<Option<Self::Weapon>, Self::Error>;
    fn server_level(&mut self) -> Result<Option<Self::ServerLevel>, Self::Error>;
    /// Called only with a nonnull weapon and a server level. Implementations
    /// apply the weapon's actual armor-effectiveness enchantment components.
    fn modify_armor_effectiveness(
        &mut self,
        level: Self::ServerLevel,
        weapon: Self::Weapon,
        effectiveness: f32,
    ) -> Result<f32, Self::Error>;
}
pub fn after_armor<W: ArmorWorld>(
    world: &mut W,
    damage: f32,
    armor: f32,
    toughness: f32,
) -> Result<f32, W::Error> {
    let divisor = 2.0 + toughness / 4.0;
    let protection = clamp(armor - damage / divisor, armor * 0.2, 20.0);
    let mut effectiveness = protection / 25.0;
    if let Some(weapon) = world.weapon()?
        && let Some(level) = world.server_level()?
    {
        effectiveness = clamp(
            world.modify_armor_effectiveness(level, weapon, effectiveness)?,
            0.0,
            1.0,
        );
    }
    Ok(damage * (1.0 - effectiveness))
}
pub fn after_magic(damage: f32, protection: f32) -> f32 {
    let protection = clamp(protection, 0.0, 20.0);
    damage * (1.0 - protection / 25.0)
}
