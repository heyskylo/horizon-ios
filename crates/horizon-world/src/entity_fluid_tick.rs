//! Entity water-entry effects and current forces around immersion updates.
use crate::entity_fluid::{
    CurrentTarget, EntityFluidInteraction, FluidInteractionError, FluidInteractionWorld, FluidTagId,
};
type WorldError<W> = <W as FluidInteractionWorld>::Error;
pub trait EntityFluidTickWorld:
    FluidInteractionWorld + CurrentTarget<Error = WorldError<Self>>
{
    fn is_pushed_by_fluid(&mut self) -> Result<bool, WorldError<Self>>;
    fn reset_fall_distance(&mut self) -> Result<(), WorldError<Self>>;
    fn was_touching_water(&self) -> bool;
    fn first_tick(&self) -> bool;
    fn water_splash(&mut self) -> Result<(), WorldError<Self>>;
    fn set_was_touching_water(&mut self, value: bool);
    fn fast_lava(&mut self) -> Result<bool, WorldError<Self>>;
    /// Multiplies the fall distance by a mod fluid's `fallDistanceModifier`;
    /// an entity that keeps none only resets it (water's 0).
    fn scale_fall_distance(&mut self, by: f32) -> Result<(), WorldError<Self>> {
        if by == 0.0 {
            self.reset_fall_distance()
        } else {
            Ok(())
        }
    }
}
/// The push predicate is queried twice: before resetting immersion state and
/// after water-entry effects. Water/lava membership is captured before effects.
pub fn update<W: EntityFluidTickWorld>(
    interaction: &mut EntityFluidInteraction,
    world: &mut W,
    water: FluidTagId,
    lava: FluidTagId,
    budget: usize,
) -> Result<bool, FluidInteractionError<WorldError<W>>> {
    let ignore = !world
        .is_pushed_by_fluid()
        .map_err(FluidInteractionError::World)?;
    let changed = interaction.update(world, ignore, budget)?;
    let in_water = interaction.is_in_fluid(world, water);
    let in_lava = interaction.is_in_fluid(world, lava);
    if in_water {
        world
            .reset_fall_distance()
            .map_err(FluidInteractionError::World)?;
        if !world.was_touching_water() && !world.first_tick() {
            world.water_splash().map_err(FluidInteractionError::World)?;
        }
    }
    for m in interaction.mod_fluids().filter(|m| m.height > 0.0) {
        world
            .scale_fall_distance(m.fluid.info().entity.fall_distance_modifier)
            .map_err(FluidInteractionError::World)?;
    }
    world.set_was_touching_water(in_water);
    if world
        .is_pushed_by_fluid()
        .map_err(FluidInteractionError::World)?
    {
        if in_water {
            interaction
                .apply_current(world, water, 0.014)
                .map_err(FluidInteractionError::World)?;
        }
        if in_lava {
            let speed = if world.fast_lava().map_err(FluidInteractionError::World)? {
                0.007
            } else {
                0.0023333333333333335
            };
            interaction
                .apply_current(world, lava, speed)
                .map_err(FluidInteractionError::World)?;
        }
        interaction
            .apply_mod_currents(world)
            .map_err(FluidInteractionError::World)?;
    }
    Ok(changed)
}
