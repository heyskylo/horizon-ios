//! Living item-blocking dispatch around direction and component reductions.
use crate::view_direction;
use horizon_core::geometry::Vec3;
pub trait ItemBlockingWorld {
    type Error;
    type Item: Copy;
    type Component: Copy;
    type Entity: Copy;
    type Level: Copy;
    type Hand: Copy;
    fn blocking_item(&mut self) -> Result<Option<Self::Item>, Self::Error>;
    fn component(&mut self, item: Self::Item) -> Option<Self::Component>;
    fn bypasses_component(&mut self, component: Self::Component) -> Result<bool, Self::Error>;
    fn direct_entity(&mut self) -> Result<Option<Self::Entity>, Self::Error>;
    fn is_arrow(&self, entity: Self::Entity) -> bool;
    fn pierce_level(&mut self, arrow: Self::Entity) -> Result<i8, Self::Error>;
    fn source_position(&mut self) -> Result<Option<Vec3>, Self::Error>;
    fn head_yaw(&mut self) -> Result<f32, Self::Error>;
    fn position(&mut self) -> Result<Vec3, Self::Error>;
    fn resolve(
        &mut self,
        component: Self::Component,
        damage: f32,
        angle: f64,
    ) -> Result<f32, Self::Error>;
    fn level(&mut self) -> Result<Self::Level, Self::Error>;
    fn used_hand(&mut self) -> Result<Self::Hand, Self::Error>;
    fn hurt_blocking_item(
        &mut self,
        component: Self::Component,
        level: Self::Level,
        item: Self::Item,
        hand: Self::Hand,
        blocked: f32,
    ) -> Result<(), Self::Error>;
    fn source_is_projectile(&mut self) -> Result<bool, Self::Error>;
    fn is_living(&self, entity: Self::Entity) -> bool;
    /// Uses the original server-level argument, not the later virtual level.
    fn block_using_item(
        &mut self,
        attacker: Self::Entity,
        damage: f32,
        fully_blocked: bool,
    ) -> Result<(), Self::Error>;
}
pub fn apply<W: ItemBlockingWorld>(world: &mut W, damage: f32) -> Result<f32, W::Error> {
    if damage <= 0.0 {
        return Ok(0.0);
    }
    let Some(item) = world.blocking_item()? else {
        return Ok(0.0);
    };
    let Some(component) = world.component(item) else {
        return Ok(0.0);
    };
    if world.bypasses_component(component)? {
        return Ok(0.0);
    }
    if let Some(entity) = world.direct_entity()?
        && world.is_arrow(entity)
        && world.pierce_level(entity)? > 0
    {
        return Ok(0.0);
    }
    let angle = if let Some(source) = world.source_position()? {
        let yaw = world.head_yaw()?;
        let position = world.position()?;
        view_direction::blocking_angle(source, position, yaw)
    } else {
        view_direction::BLOCKING_FALLBACK_ANGLE
    };
    let blocked = world.resolve(component, damage, angle)?;
    let level = world.level()?;
    let hand = world.used_hand()?;
    world.hurt_blocking_item(component, level, item, hand, blocked)?;
    if blocked > 0.0
        && !world.source_is_projectile()?
        && let Some(entity) = world.direct_entity()?
        && world.is_living(entity)
    {
        world.block_using_item(entity, damage, blocked >= damage)?;
    }
    Ok(blocked)
}
