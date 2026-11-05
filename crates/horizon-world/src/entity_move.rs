//! Entity.move ordering around collision, support, landing and movement effects.
use crate::{movement_emission::MovementEmission, registry::BlockStateId};
use horizon_core::{geometry::Vec3, position::BlockPos};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoverType {
    SelfMovement,
    Player,
    Piston,
    ShulkerBox,
    Shulker,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct CollisionFlags {
    pub horizontal: bool,
    pub vertical: bool,
    pub below: bool,
    pub minor: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct MovementRecord {
    pub from: Vec3,
    pub to: Vec3,
    pub original: Option<Vec3>,
}
#[derive(Clone, Debug, Default)]
pub struct MovementHistory {
    records: VecDeque<MovementRecord>,
}
impl MovementHistory {
    pub fn records(&self) -> &VecDeque<MovementRecord> {
        &self.records
    }
    pub fn add(&mut self, record: MovementRecord) {
        if self.records.len() >= 100 {
            let first = self.records.pop_front().unwrap();
            let second = self.records.pop_front().unwrap();
            self.records.push_front(MovementRecord {
                from: first.from,
                to: second.to,
                original: None,
            });
        }
        self.records.push_back(record);
    }
    pub fn clear(&mut self) {
        self.records.clear();
    }
}
/// The provider retains entity fields so callbacks can mutate them. Hooks below
/// are native phase boundaries; bind collision, support, landing, restitution
/// and emission hooks to their corresponding world modules.
pub trait EntityMoveWorld {
    type Error;
    fn no_physics(&self) -> bool;
    fn position(&self) -> Vec3;
    fn set_position(&mut self, position: Vec3) -> Result<(), Self::Error>;
    fn record_movement(&mut self, mover: MoverType, movement: Vec3) -> Result<(), Self::Error>;
    fn collision_flags(&self) -> CollisionFlags;
    fn set_collision_flags(&mut self, flags: CollisionFlags);
    fn limit_piston_movement(&mut self, movement: Vec3) -> Result<Vec3, Self::Error>;
    fn stuck_multiplier(&self) -> Vec3;
    fn set_stuck_multiplier(&mut self, value: Vec3);
    fn velocity(&mut self) -> Result<Vec3, Self::Error>;
    fn set_velocity(&mut self, value: Vec3) -> Result<(), Self::Error>;
    fn back_off_edge(&mut self, movement: Vec3, mover: MoverType) -> Result<Vec3, Self::Error>;
    fn collide(&mut self, movement: Vec3) -> Result<Vec3, Self::Error>;
    fn fall_distance(&self) -> f64;
    /// FALLDAMAGE_RESETTING blocks and WATER fluids, entity collision context.
    /// Return true for a non-MISS hit.
    fn fall_reset_clip(&mut self, from: Vec3, to: Vec3) -> Result<bool, Self::Error>;
    fn reset_fall_distance(&mut self) -> Result<(), Self::Error>;
    fn movement_history(&mut self) -> &mut MovementHistory;
    fn locally_authoritative(&mut self) -> Result<bool, Self::Error>;
    fn set_on_ground_with_movement(
        &mut self,
        below: bool,
        horizontal: bool,
        movement: Vec3,
    ) -> Result<(), Self::Error>;
    fn minor_horizontal_collision(&mut self, movement: Vec3) -> Result<bool, Self::Error>;
    fn legacy_on_position(&mut self) -> Result<BlockPos, Self::Error>;
    fn block_state(&mut self, p: BlockPos) -> Result<BlockStateId, Self::Error>;
    fn on_ground(&mut self) -> Result<bool, Self::Error>;
    fn check_fall_damage(
        &mut self,
        dy: f64,
        on_ground: bool,
        state: BlockStateId,
        p: BlockPos,
    ) -> Result<(), Self::Error>;
    fn removed(&mut self) -> Result<bool, Self::Error>;
    fn can_simulate(&mut self) -> Result<bool, Self::Error>;
    fn restitute(
        &mut self,
        state: BlockStateId,
        collided_x: bool,
        collided_z: bool,
        resolved: Vec3,
    ) -> Result<(), Self::Error>;
    fn client_side(&mut self) -> Result<bool, Self::Error>;
    fn movement_emission(&mut self) -> Result<MovementEmission, Self::Error>;
    fn passenger(&mut self) -> Result<bool, Self::Error>;
    fn apply_movement_emission(
        &mut self,
        emission: MovementEmission,
        resolved: Vec3,
        p: BlockPos,
        state: BlockStateId,
    ) -> Result<(), Self::Error>;
    fn block_speed_factor(&mut self) -> Result<f32, Self::Error>;
}
fn java_zero(v: Vec3) -> bool {
    v.x.to_bits() == 0 && v.y.to_bits() == 0 && v.z.to_bits() == 0
}
fn different(a: f64, b: f64) -> bool {
    let delta = (b - a).abs();
    delta >= f64::from(1.0e-5f32) || delta.is_nan()
}
/// Release behavior; native IDE-only invalid-simulation assertions and profiler
/// callbacks do not form part of this gameplay transition.
pub fn move_entity<W: EntityMoveWorld>(
    world: &mut W,
    mover: MoverType,
    mut movement: Vec3,
) -> Result<(), W::Error> {
    if world.no_physics() {
        world.set_position(world.position().add_vector(movement))?;
        world.record_movement(mover, movement)?;
        world.set_collision_flags(CollisionFlags::default());
        return Ok(());
    }
    if mover == MoverType::Piston {
        movement = world.limit_piston_movement(movement)?;
        if java_zero(movement) {
            return Ok(());
        }
    }
    let stuck = world.stuck_multiplier();
    if stuck.length_squared() > 1.0e-7 {
        if mover != MoverType::Piston {
            movement = movement.multiply(stuck);
        }
        world.set_stuck_multiplier(Vec3::ZERO);
        world.set_velocity(Vec3::ZERO)?;
    }
    movement = world.back_off_edge(movement, mover)?;
    let resolved = world.collide(movement)?;
    let length = resolved.length_squared();
    if length > 1.0e-7 || movement.length_squared() - length < 1.0e-7 {
        if world.fall_distance() != 0.0 && length >= 1.0 {
            let distance = resolved.length().min(8.0);
            let from = world.position();
            let to = from.add_vector(resolved.normalize().scale(distance));
            if world.fall_reset_clip(from, to)? {
                world.reset_fall_distance()?;
            }
        }
        let from = world.position();
        let to = from.add_vector(resolved);
        world.movement_history().add(MovementRecord {
            from,
            to,
            original: Some(movement),
        });
        world.set_position(to)?;
        world.record_movement(mover, resolved)?;
    }
    let x = different(movement.x, resolved.x);
    let z = different(movement.z, resolved.z);
    let mut flags = world.collision_flags();
    flags.horizontal = x || z;
    world.set_collision_flags(flags);
    let nonzero_y = movement.y.abs() > 0.0;
    if nonzero_y || world.locally_authoritative()? {
        flags = world.collision_flags();
        flags.vertical = movement.y != resolved.y;
        flags.below = flags.vertical && movement.y < 0.0;
        world.set_collision_flags(flags);
        world.set_on_ground_with_movement(flags.below, flags.horizontal, resolved)?;
    }
    let minor = if world.collision_flags().horizontal {
        world.minor_horizontal_collision(resolved)?
    } else {
        false
    };
    flags = world.collision_flags();
    flags.minor = minor;
    world.set_collision_flags(flags);
    let p = world.legacy_on_position()?;
    let state = world.block_state(p)?;
    if world.locally_authoritative()? {
        let grounded = world.on_ground()?;
        world.check_fall_damage(resolved.y, grounded, state, p)?;
    }
    if world.removed()? {
        return Ok(());
    }
    if world.can_simulate()?
        && (nonzero_y && world.collision_flags().vertical || world.collision_flags().horizontal)
    {
        world.restitute(state, x, z, resolved)?;
    }
    if !world.client_side()? || world.locally_authoritative()? {
        let emission = world.movement_emission()?;
        if emission.anything() && !world.passenger()? {
            world.apply_movement_emission(emission, resolved, p, state)?;
        }
    }
    let factor = world.block_speed_factor()?;
    let velocity = scale_by_block_speed(world.velocity()?, factor);
    world.set_velocity(velocity)
}
/// The last step of `Entity.move`: the horizontal velocity is multiplied by
/// `getBlockSpeedFactor()` (soul sand, honey), the vertical one is untouched.
/// Every kind of entity ends its move here, so the player and the mobs share
/// this one definition.
pub fn scale_by_block_speed(velocity: Vec3, factor: f32) -> Vec3 {
    let factor = f64::from(factor);
    velocity.multiply(Vec3::new(factor, 1.0, factor))
}
