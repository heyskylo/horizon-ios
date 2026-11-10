//! `LivingEntity` movement for mobs: `aiStep` input decay and jumping,
//! `travel` in air, water and lava (friction-scaled acceleration, gravity,
//! fluid slowdown and falling adjustment), fluid immersion heights, fall
//! damage, fire and the walk animation.
use super::{Mob, MobEvent, MobWorld};
use crate::{
    blocks::Fluid,
    collisions::{PowderSnowSubject, SceneCollisions},
};
use horizon_core::{
    geometry::{Aabb, Vec3, trig},
    position::BlockPos,
    random::RandomSource,
};
use horizon_world::{
    block_landing::LandingEntityKind,
    entity_fluid::{CurrentAccumulator, CurrentTarget, ModFluidImmersion},
    fluid::{FluidKind, FluidState},
    fluid_flow::{FluidFlowWorld, flow_at},
    movement_restitution::{RestitutionInput, RestitutionWorld, restitute_movement},
    registry::BlockStateId,
};

/// How many block particles a landing throws up
/// (`(int)(min(0.2F + power / 15, 2.5) * 150)`), none when the fall is within
/// `safe_fall_distance` (`power = max(floor(fall + 1.0E-6 - safe), 0)`).
fn landing_dust_count(fall_distance: f64, safe_fall_distance: f64) -> Option<i32> {
    let power = f64::from(((fall_distance + 1.0e-6 - safe_fall_distance).floor() as i32).max(0));
    (power > 0.0).then(|| (150.0 * (f64::from(0.2_f32) + power / 15.0).min(2.5)) as i32)
}
/// Where the dust starts: at the mob, or, when its own block is not the one it
/// landed on (`landing`), on that block's face toward the mob.
fn landing_dust_at(position: Vec3, landing: [i32; 3]) -> Vec3 {
    if landing[0] == position.x.floor() as i32 && landing[2] == position.z.floor() as i32 {
        return position;
    }
    let dx = position.x - f64::from(landing[0]) - 0.5;
    let dz = position.z - f64::from(landing[2]) - 0.5;
    let scale = dx.abs().max(dz.abs());
    Vec3::new(
        f64::from(landing[0]) + 0.5 + dx / scale * 0.5,
        position.y,
        f64::from(landing[2]) + 0.5 + dz / scale * 0.5,
    )
}

/// `Entity.moveDist`, `flyDist`, `nextStep` and `Parrot.nextFlap`.
#[derive(Clone, Copy, Debug)]
pub struct Steps {
    pub move_dist: f32,
    pub fly_dist: f32,
    pub next_step: f32,
    pub next_flap: f32,
    /// `AbstractHorse.gallopSoundCounter`.
    pub gallop: i32,
}
impl Default for Steps {
    fn default() -> Self {
        Self {
            move_dist: 0.0,
            fly_dist: 0.0,
            next_step: 1.0,
            next_flap: 1.0,
            gallop: 0,
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Fluids {
    pub water: f64,
    pub lava: f64,
    pub in_water: bool,
    pub in_lava: bool,
    pub eye_in_water: bool,
    /// The mod fluid the mob is deepest in (`isInFluidType`): not water to the
    /// game, but the mob moves in it as in water, with its numbers.
    pub modded: Option<ModFluidImmersion>,
}
/// `FluidFlowWorld` over the mob's scene: the neighbours a flow vector reads.
struct FlowScene<'a, 'w>(&'a MobWorld<'w>);
impl FluidFlowWorld for FlowScene<'_, '_> {
    type Error = std::convert::Infallible;
    type Block = BlockStateId;
    fn fluid(&mut self, p: BlockPos) -> Result<FluidState, Self::Error> {
        Ok(self.0.fluid_tables.fluid(self.0.world.block(p.x, p.y, p.z)))
    }
    fn block(&mut self, p: BlockPos) -> Result<BlockStateId, Self::Error> {
        Ok(self.0.world.block(p.x, p.y, p.z))
    }
    fn blocks_fluid_flow(&self, s: BlockStateId) -> bool {
        self.0.fluid_tables.blocks_flow(s)
    }
    fn is_ice_class(&self, s: BlockStateId) -> bool {
        self.0.fluid_tables.ice(s)
    }
    fn is_face_sturdy(
        &mut self,
        s: BlockStateId,
        _: BlockPos,
        d: horizon_core::geometry::Direction,
    ) -> Result<bool, Self::Error> {
        Ok(self.0.fluid_tables.sturdy(s, d))
    }
}
/// The mob's velocity, for `CurrentAccumulator::apply_to`.
struct Pushed<'a>(&'a mut Vec3);
impl CurrentTarget for Pushed<'_> {
    type Error = std::convert::Infallible;
    fn is_player_class(&self) -> bool {
        false
    }
    fn velocity(&mut self) -> Result<Vec3, Self::Error> {
        Ok(*self.0)
    }
    fn add_delta_movement(&mut self, delta: Vec3) -> Result<(), Self::Error> {
        let sum = Vec3::new(self.0.x + delta.x, self.0.y + delta.y, self.0.z + delta.z);
        if delta.is_finite() && sum.is_finite() {
            *self.0 = sum;
        }
        Ok(())
    }
}
pub fn wrap(a: f32) -> f32 {
    let mut a = a % 360.0;
    if a >= 180.0 {
        a -= 360.0;
    }
    if a < -180.0 {
        a += 360.0;
    }
    a
}
impl MobWorld<'_> {
    /// FluidState.getHeight: a full block under the same fluid, else amount / 9.
    pub fn fluid_height(&self, x: i32, y: i32, z: i32) -> (Fluid, f64) {
        let f = self.facts.get(self.world.block(x, y, z));
        if f.fluid == Fluid::None {
            return (Fluid::None, 0.0);
        }
        let above = self.facts.get(self.world.block(x, y + 1, z));
        if above.fluid == f.fluid {
            return (f.fluid, 1.0);
        }
        (f.fluid, f64::from(f.fluid_level) / 9.0)
    }
    /// Entity.updateFluidHeightAndDoFluidPushing heights over the deflated box.
    pub fn fluids(&self, bounds: Aabb, eye_y: f64) -> Fluids {
        let b = bounds.deflate(Vec3::new(0.001, 0.001, 0.001));
        let (lo, hi) = (b.min_position(), b.max_position());
        let mut out = Fluids::default();
        // The eye column below, in the box but for a box thinner than the
        // deflation.
        let (ex, ez) = (
            ((lo.x + hi.x) / 2.0).floor() as i32,
            ((lo.z + hi.z) / 2.0).floor() as i32,
        );
        // Most boxes are in sections without a fluid: no cell to read.
        let cells = (
            [
                (lo.x.floor() as i32).min(ex),
                lo.y.floor() as i32,
                (lo.z.floor() as i32).min(ez),
            ],
            [
                (hi.x.ceil() as i32 - 1).max(ex),
                hi.y.ceil() as i32 - 1,
                (hi.z.ceil() as i32 - 1).max(ez),
            ],
        );
        if !self.world.may_hold(cells.0, cells.1, |s| {
            self.facts.get(s).fluid != Fluid::None
                || !self.fluid_tables.fluid(s).kind().is_vanilla()
        }) {
            return out;
        }
        // A second pass over the box, only when a content root loaded mod fluids.
        if horizon_world::fluid::mod_fluids_loaded() {
            out.modded = self.mod_fluid(lo, hi, (ex, ez), eye_y);
        }
        for x in lo.x.floor() as i32..=(hi.x.ceil() as i32 - 1) {
            for y in lo.y.floor() as i32..=(hi.y.ceil() as i32 - 1) {
                for z in lo.z.floor() as i32..=(hi.z.ceil() as i32 - 1) {
                    let (fluid, h) = self.fluid_height(x, y, z);
                    if fluid == Fluid::None {
                        continue;
                    }
                    let top = f64::from(y) + h;
                    if top >= lo.y {
                        let depth = top - lo.y;
                        match fluid {
                            Fluid::Water => {
                                out.water = out.water.max(depth);
                                out.in_water = true;
                            }
                            Fluid::Lava => {
                                out.lava = out.lava.max(depth);
                                out.in_lava = true;
                            }
                            Fluid::None => {}
                        }
                    }
                }
            }
        }
        // EntityFluidInteraction.update: the eyes are in a fluid block of
        // the box in the entity's column, between its bottom and its height
        // for the camera.
        for y in lo.y.floor() as i32..=(hi.y.ceil() as i32 - 1) {
            let (fluid, h) = self.fluid_height(ex, y, ez);
            if fluid != Fluid::Water || f64::from(y) + h < lo.y || eye_y < f64::from(y) {
                continue;
            }
            let state = self.world.block(ex, y, ez);
            let source = self
                .registry
                .state(state)
                .and_then(|s| s.properties.get("level"))
                .is_none_or(|l| l == "0");
            let above = self.facts.get(self.world.block(ex, y + 1, ez));
            let camera = if source && above.sturdy_full & 1 != 0 {
                1.0
            } else {
                h
            };
            if eye_y <= f64::from(y) + camera {
                out.eye_in_water = true;
            }
        }
        out
    }
    /// The mod fluid in a cell and its surface: a full block under the same
    /// fluid, else its amount / 9 (`FluidState.getHeight`). A mod liquid block
    /// holds its fluid only in the fluid table, not in the block facts.
    fn mod_fluid_at(&self, x: i32, y: i32, z: i32) -> Option<(FluidKind, f64)> {
        let state = self.fluid_tables.fluid(self.world.block(x, y, z));
        if state.kind().is_vanilla() {
            return None;
        }
        let fluid = state.kind().source();
        let above = self.fluid_tables.fluid(self.world.block(x, y + 1, z));
        let h = if above.kind().source() == fluid {
            1.0
        } else {
            f64::from(state.amount()) / 9.0
        };
        Some((fluid, f64::from(y) + h))
    }
    /// The mod fluid deepest over the box (`lo`..`hi`), and whether the eye
    /// (in column `eye`) is in it: a mod liquid block holds its fluid only in
    /// the fluid table.
    fn mod_fluid(
        &self,
        lo: Vec3,
        hi: Vec3,
        eye: (i32, i32),
        eye_y: f64,
    ) -> Option<ModFluidImmersion> {
        let at = |x, y, z| self.mod_fluid_at(x, y, z);
        let (y0, y1) = (lo.y.floor() as i32, hi.y.ceil() as i32 - 1);
        let mut deepest: Option<ModFluidImmersion> = None;
        for x in lo.x.floor() as i32..=(hi.x.ceil() as i32 - 1) {
            for y in y0..=y1 {
                for z in lo.z.floor() as i32..=(hi.z.ceil() as i32 - 1) {
                    if let Some((fluid, top)) = at(x, y, z)
                        && top >= lo.y
                        && deepest.is_none_or(|m| top - lo.y > m.height)
                    {
                        deepest = Some(ModFluidImmersion {
                            fluid,
                            height: top - lo.y,
                            eyes_inside: false,
                        });
                    }
                }
            }
        }
        let mut m = deepest?;
        m.eyes_inside = (y0..=y1).any(|y| {
            at(eye.0, y, eye.1).is_some_and(|(fluid, top)| {
                fluid == m.fluid && top >= lo.y && eye_y >= f64::from(y) && eye_y <= top
            })
        });
        Some(m)
    }
    /// Entity.updateFluidHeightAndDoFluidPushing's currents over the deflated
    /// box: the flow of every water and lava block it touches, summed (a thin
    /// fluid layer scales its flow down), as (water, lava); `modded` sums the
    /// flow of that mod fluid instead.
    fn fluid_currents(
        &self,
        bounds: Aabb,
        modded: Option<FluidKind>,
    ) -> (CurrentAccumulator, CurrentAccumulator) {
        let base_y = bounds.min_position().y;
        let b = bounds.deflate(Vec3::new(0.001, 0.001, 0.001));
        let (lo, hi) = (b.min_position(), b.max_position());
        let mut scene = FlowScene(self);
        let (mut water, mut lava) = (CurrentAccumulator::default(), CurrentAccumulator::default());
        for x in lo.x.floor() as i32..=(hi.x.ceil() as i32 - 1) {
            for y in lo.y.floor() as i32..=(hi.y.ceil() as i32 - 1) {
                for z in lo.z.floor() as i32..=(hi.z.ceil() as i32 - 1) {
                    let p = BlockPos::new(x, y, z);
                    let state = self.fluid_tables.fluid(self.world.block(x, y, z));
                    let kind = state.kind();
                    let current = if modded.is_some_and(|m| kind.source() == m) {
                        &mut water
                    } else if modded.is_some() || !kind.is_vanilla() {
                        continue;
                    } else if kind.source() == FluidKind::Water {
                        &mut water
                    } else if kind.source() == FluidKind::Lava {
                        &mut lava
                    } else {
                        continue;
                    };
                    let modded_top = if kind.is_vanilla() {
                        None
                    } else {
                        self.mod_fluid_at(x, y, z)
                    };
                    let surface = match modded_top {
                        Some((_, top)) => top,
                        None => f64::from(y) + self.fluid_height(x, y, z).1,
                    };
                    if surface < lo.y {
                        continue;
                    }
                    current.height = current.height.max(surface - base_y);
                    let mut flow = flow_at(&mut scene, state, p).unwrap_or(Vec3::ZERO);
                    if current.height < 0.4 {
                        flow = flow.scale(current.height);
                    }
                    current.accumulate(flow);
                }
            }
        }
        (water, lava)
    }
    /// `Entity.updateFluidInteraction`'s pushes for an entity fluids push
    /// (`isPushedByFluid`) with the box `bounds`: its `velocity` after the
    /// water's and the lava's currents (`EntityFluidInteraction.applyCurrentTo`).
    pub fn pushed_by_currents(&self, bounds: Aabb, mut velocity: Vec3) -> Vec3 {
        let (water, lava) = self.fluid_currents(bounds, None);
        let lava_scale = if self.dimension_type.fast_lava {
            0.007
        } else {
            0.0023333333333333335
        };
        let mut pushed = Pushed(&mut velocity);
        if water.height > 0.0 {
            let _ = water.apply_to(&mut pushed, 0.014);
        }
        if lava.height > 0.0 {
            let _ = lava.apply_to(&mut pushed, lava_scale);
        }
        velocity
    }
    /// `BlockState.getCollisionShape(level, pos)` (the empty collision
    /// context) as boxes in the world.
    pub fn block_collision_boxes(&self, x: i32, y: i32, z: i32) -> Vec<Aabb> {
        let mut scene = SceneCollisions {
            world: self.world,
            binding: self.collision,
            subject: None,
        };
        let state = self.world.block(x, y, z);
        let at = horizon_core::position::BlockPos::new(x, y, z);
        let context = horizon_world::collision_context::CollisionContext::empty(false);
        let offset = Vec3::new(f64::from(x), f64::from(y), f64::from(z));
        self.collision
            .collision_shape_with_context(&mut scene, state, at, &context)
            .map(|shape| shape.boxes().iter().map(|b| b.move_by(offset)).collect())
            .unwrap_or_default()
    }
    /// `Level.findSupportingBlock`: of the blocks whose collision shape
    /// meets `area`, the one whose centre is nearest `from` (ties: the
    /// greatest by `Vec3i.compareTo`, y then z then x).
    ///
    /// Like `BlockCollisions`' cursor, a block whose shape stays inside its
    /// own cell (not `hasLargeCollisionShape`, not a moving piston) is only
    /// shaped when that cell meets `area`: elsewhere it cannot. The pick is
    /// a total order, so the column-by-column visit finds the same block.
    pub fn supporting_block(&self, area: Aabb, from: Vec3) -> Option<[i32; 3]> {
        use horizon_core::geometry::Axis;
        let range = |axis| {
            (area.min(axis) - 1.0e-7).floor() as i32 - 1
                ..=(area.max(axis) + 1.0e-7).floor() as i32 + 1
        };
        // Aabb.intersects of the unit cell at `c` along one axis.
        let meets = |axis, c: i32| {
            let c = f64::from(c);
            c < area.max(axis) && c + 1.0 > area.min(axis)
        };
        // Where no section of the cursor holds a block reaching out of its
        // cell, only the cells meeting `area` can hold the support.
        let (rx, ry, rz) = (range(Axis::X), range(Axis::Y), range(Axis::Z));
        let outer = self.world.may_hold(
            [*rx.start(), *ry.start(), *rz.start()],
            [*rx.end(), *ry.end(), *rz.end()],
            |s| self.collision.reaches_out(s),
        );
        let mut best: Option<(f64, [i32; 3])> = None;
        for x in rx {
            for z in rz.clone() {
                let column = meets(Axis::X, x) && meets(Axis::Z, z);
                if !outer && !column {
                    continue;
                }
                let Some(chunk) = self.world.chunks.get(&(x >> 4, z >> 4)) else {
                    continue;
                };
                for y in ry.clone() {
                    if !outer && !meets(Axis::Y, y) {
                        continue;
                    }
                    let state = chunk.block(x & 15, y, z & 15);
                    let Ok(block) = self.collision.collision_block(state) else {
                        continue;
                    };
                    if !(block.large_collision_shape
                        || block.moving_piston
                        || (column && meets(Axis::Y, y)))
                    {
                        continue;
                    }
                    if !self
                        .block_collision_boxes(x, y, z)
                        .iter()
                        .any(|b| b.intersects(area))
                    {
                        continue;
                    }
                    let d = (f64::from(x) + 0.5 - from.x).powi(2)
                        + (f64::from(y) + 0.5 - from.y).powi(2)
                        + (f64::from(z) + 0.5 - from.z).powi(2);
                    let greater = |b: [i32; 3]| (y, z, x) > (b[1], b[2], b[0]);
                    if best.is_none_or(|(bd, b)| d < bd || d == bd && greater(b)) {
                        best = Some((d, [x, y, z]));
                    }
                }
            }
        }
        best.map(|(_, p)| p)
    }
    pub fn no_collision(&self, b: Aabb) -> bool {
        let mut scene = SceneCollisions {
            world: self.world,
            binding: self.collision,
            subject: None,
        };
        scene.unobstructed(b).unwrap_or(false)
    }
    /// `LevelReader.containsAnyLiquid`: the blocks from the box's minimum
    /// corner to the block holding its maximum corner.
    pub fn contains_any_liquid(&self, b: Aabb) -> bool {
        let (lo, hi) = (b.min_position(), b.max_position());
        let (x0, y0, z0) = (
            lo.x.floor() as i32,
            lo.y.floor() as i32,
            lo.z.floor() as i32,
        );
        let (x1, y1, z1) = (
            hi.x.floor() as i32,
            hi.y.floor() as i32,
            hi.z.floor() as i32,
        );
        (x0..=x1).any(|x| {
            (y0..=y1).any(|y| {
                (z0..=z1).any(|z| self.facts.get(self.world.block(x, y, z)).fluid != Fluid::None)
            })
        })
    }
}
impl Mob {
    /// `Entity.refreshDimensions` after growing from `old_w` by `old_h`:
    /// `fudgePositionAfterSizeChange` moves the mob to the free spot nearest
    /// its old centre where the bigger box fits (`findFreePosition`), or
    /// failing that, one that fits at the old height.
    pub fn fudge_position_after_size_change(&mut self, w: &MobWorld, old_w: f64, old_h: f64) {
        let (new_w, new_h, _) = self.dims();
        let small = new_w <= 4.0 && new_h <= 4.0;
        if self.age == 0 || self.no_physics() || !small || !(new_w > old_w || new_h > old_h) {
            return;
        }
        let center = Vec3::new(
            self.position.x,
            self.position.y + old_h / 2.0,
            self.position.z,
        );
        let dw = (new_w - old_w).max(0.0) + 1.0e-6;
        let dh = (new_h - old_h).max(0.0) + 1.0e-6;
        let at = |allowed: Aabb, size_y: f64| free_position(w, allowed, center, new_w, size_y);
        if let Some(p) = at(Aabb::of_size(center, Vec3::new(dw, dh, dw)), new_h) {
            self.position = Vec3::new(p.x, p.y - new_h / 2.0, p.z);
            return;
        }
        if new_w > old_w
            && new_h > old_h
            && let Some(p) = at(Aabb::of_size(center, Vec3::new(dw, 1.0e-6, dw)), old_h)
        {
            self.position = Vec3::new(p.x, p.y - old_h / 2.0 + 1.0e-6, p.z);
        }
    }
    fn no_physics(&self) -> bool {
        false
    }
    pub fn eye_y(&self) -> f64 {
        self.position.y + self.eye_height()
    }
    /// `LivingEntity.onClimbable` (`Spider.onClimbable`: the climbing flag):
    /// the block it stands in is `#minecraft:climbable` (vines, ladders,
    /// scaffolding...), or an open trapdoor over a ladder facing the same way
    /// (`trapdoorUsableAsLadder`). A mob gliding (`isFallFlying`) through a
    /// `#minecraft:can_glide_through` block (vines) is not on it: that test
    /// comes first.
    pub fn on_climbable(&self, w: &MobWorld) -> bool {
        if self.kind.flags.climber {
            return self.climbing;
        }
        // `AbstractHorse.onClimbable`, `Ghast.onClimbable`, `Phantom.onClimbable`
        // and `HappyGhast.onClimbable` are a plain false: no horse, donkey,
        // mule, undead horse, llama, trader llama, camel or camel husk (nor
        // those flying monsters) is ever on a climbable block, whatever it
        // stands in.
        if super::equine::is_equine(self.species())
            || matches!(
                self.species(),
                super::Species::Ghast | super::Species::Phantom | super::Species::HappyGhast
            )
        {
            return false;
        }
        let (x, y, z) = (
            self.position.x.floor() as i32,
            self.position.y.floor() as i32,
            self.position.z.floor() as i32,
        );
        let state = w.world.block(x, y, z);
        let facts = w.facts.get(state);
        if self.extra.fall_flying && w.has_tag("minecraft:can_glide_through", &facts.name) {
            return false;
        }
        if w.has_tag("minecraft:climbable", &facts.name) {
            return true;
        }
        facts.has_class("TrapDoorBlock")
            && w.prop(x, y, z, "open") == Some("true")
            && w.block_name(x, y - 1, z) == "minecraft:ladder"
            && w.prop(x, y - 1, z, "facing") == w.prop(x, y, z, "facing")
    }
    /// `LivingEntity.getScale`: the `minecraft:scale` the server sent on this
    /// living entity (`extra.model_scale`, 0 when none came), through the
    /// species' `sanitizeScale` override. Every box, eye height, seat and
    /// shadow radius takes it.
    pub fn model_scale(&self) -> f32 {
        let scale = if self.extra.model_scale > 0.0 {
            self.extra.model_scale
        } else {
            1.0
        };
        match self.species() {
            // HappyGhast.sanitizeScale: min(scale, 1), it never grows.
            super::Species::HappyGhast => scale.min(1.0),
            // Shulker.sanitizeScale: min(scale, 3).
            super::Species::Shulker => scale.min(3.0),
            _ => scale,
        }
    }
    /// Entity dimensions: the type's, a class's BABY_DIMENSIONS, or the
    /// slime size scaling.
    /// `EntityDimensions` (float width, height and eye height) widened to double.
    pub fn dims(&self) -> (f64, f64, f64) {
        // LivingEntity.SLEEPING_DIMENSIONS.
        if super::villager::sleeping(self) {
            return (f64::from(0.2_f32), f64::from(0.2_f32), f64::from(0.2_f32));
        }
        self.awake_dims()
    }
    /// `dims` without the sleeping box: a sleeping villager's
    /// `getDimensions(Pose.STANDING)`, whose eye height lays it in the bed
    /// (`LivingEntityRenderer.submit`).
    pub fn awake_dims(&self) -> (f64, f64, f64) {
        self.awake_dims_in(self.extra.pose == super::sync::LONG_JUMPING)
    }
    /// `Entity.getDimensions(Pose)` of the standing pose, or of
    /// `Pose.LONG_JUMPING` (what `LongJumpUtil` checks an arc with).
    pub fn awake_dims_in(&self, long_jumping: bool) -> (f64, f64, f64) {
        let k = &self.kind;
        // `LivingEntity.getDimensions` is `getDefaultDimensions(pose).scale(
        // getScale())` on every living entity: the species' box (a player's or
        // mannequin's follows its pose, Avatar.POSES) times its
        // `minecraft:scale` (`getScale`), width, height and eye height alike.
        let scale = self.model_scale();
        if matches!(&*k.id, "minecraft:player" | "minecraft:mannequin") {
            let d = self.extra.avatar_pose.dimensions(scale);
            return (
                f64::from(d.width),
                f64::from(d.height),
                f64::from(d.eye_height),
            );
        }
        let (mut w, mut h, mut e) = (k.width as f32, k.height as f32, k.eye_height as f32);
        // Sniffer.DIGGING_DIMENSIONS: 0.4 lower, its eyes at 0.81.
        if k.species == super::Species::Sniffer && super::sniffer::digging(self) {
            (h, e) = (h - 0.4, 0.81);
        }
        if self.is_baby() {
            match super::species::def(k.species).baby {
                super::species::Baby::Abs(bw, bh, be) => {
                    (w, h, e) = (bw as f32, bh as f32, be as f32)
                }
                super::species::Baby::Scale(s, eye) => {
                    let s = s as f32;
                    (w, h, e) = (w * s, h * s, eye.map_or(e * s, |x| x as f32))
                }
                super::species::Baby::None => {}
            }
        }
        // Goat.getDefaultDimensions: a long jump scales the box by 0.7.
        if long_jumping && k.species == super::Species::Goat {
            (w, h, e) = (w * 0.7_f32, h * 0.7_f32, e * 0.7_f32);
        }
        // Camel.getDefaultDimensions: a sitting camel is lower.
        if super::camel::is_camel(k.species) && super::camel::sitting(self) {
            (w, h, e) = super::camel::sitting_dims(self.is_baby());
        }
        // Warden.getDefaultDimensions: a metre tall while digging or emerging.
        if self.kind.species == super::Species::Warden && super::warden::digging_or_emerging(self) {
            h = 1.0;
            e = 0.85;
        }
        if self.kind.species == super::Species::Pufferfish {
            let s = super::aquatic::puff_scale(self.extra.size) as f32;
            (w, h, e) = (w * s, h * s, e * s);
        } else if self.kind.species == super::Species::Phantom {
            // Phantom.getDefaultDimensions: 1 + 0.15 per size.
            let s = 1.0 + 0.15 * self.extra.size as f32;
            (w, h, e) = (w * s, h * s, e * s);
        } else if self.extra.size > 0 {
            let s = self.extra.size as f32;
            (w, h, e) = (w * s, h * s, e * s);
        }
        // Shulker.getDefaultDimensions: the last scale, over everything
        // above, is the lid opening upward on a shell attached to the floor.
        if k.species == super::Species::Shulker
            && let Some(s) = super::shulker::peek_height_scale(self)
        {
            (h, e) = (h * s, e * s);
        }
        // Salmon.getDefaultDimensions: the variant's bounding box scale (small
        // 0.5, medium 1.0, large 1.5) over the whole fish box.
        if k.species == super::Species::Salmon {
            let s = super::aquatic::salmon_scale(self.extra.variant);
            (w, h, e) = (w * s, h * s, e * s);
        }
        // PolarBear.getDefaultDimensions: the rear-up animation scales the box
        // by (1, 1 + clientSideStandAnimation / 6), so a bear at full height is
        // twice as tall, with its eyes twice as high and the same width.
        if k.species == super::Species::PolarBear {
            let f = super::polar_bear::stand_box_scale(self);
            (h, e) = (h * f, e * f);
        }
        // `EntityDimensions.scale(getScale())`: width and height by the scale,
        // the eye height by its height factor (the same one).
        (
            f64::from(w * scale),
            f64::from(h * scale),
            f64::from(e * scale),
        )
    }
    pub fn eye_height(&self) -> f64 {
        self.dims().2
    }
    /// `EntityRenderer.getPackedLightCoords` reads the block
    /// `Entity.getLightProbePosition(partialTick)` is in, and a living entity
    /// leaves that as `getEyePosition`: the eye height over
    /// `getPosition(partialTick)`, the position the frame draws, not this
    /// tick's. A mob walking across a cell boundary is drawn lit by the cell it
    /// is drawn in.
    pub fn light_probe(&self, partial: f64) -> [i32; 3] {
        eye_probe_cell(self.previous, self.position, self.eye_height(), partial)
    }
    pub fn width(&self) -> f32 {
        self.dims().0 as f32
    }
    pub fn height(&self) -> f32 {
        self.dims().1 as f32
    }
    /// `AgeableMob.isBaby`: `canBeABaby() && getAge() < 0`. The age alone is
    /// not enough — a frog, a parrot, a zombie nautilus, a camel husk, a slime,
    /// a magma cube, a piglin brute, a tadpole, or anything that is not an
    /// `AgeableMob` at all is an adult whatever `Age` says, so its box, voice,
    /// breeding, growth, drops and goals stay an adult's.
    pub fn is_baby(&self) -> bool {
        self.extra.age < 0 && super::species::can_be_a_baby(self.kind.species)
    }
    pub fn fluid_jump_threshold(&self) -> f64 {
        // Breeze.getFluidJumpThreshold: its eye height.
        if self.species() == super::Species::Breeze {
            return self.eye_height();
        }
        if self.eye_height() < 0.4 { 0.0 } else { 0.4 }
    }
    pub fn is_in_liquid(&self) -> bool {
        self.fluids.in_water || self.fluids.in_lava
    }
    /// The mod fluid the mob is in when it `canSwim`: it rises and floats in it.
    pub fn swim_fluid(&self) -> Option<ModFluidImmersion> {
        self.fluids
            .modded
            .filter(|m| m.fluid.info().entity.can_swim)
    }
    /// In a mod fluid that `canExtinguish`.
    pub fn in_extinguishing_fluid(&self) -> bool {
        self.fluids
            .modded
            .is_some_and(|m| m.fluid.info().entity.can_extinguish)
    }
    /// The eyes in a mod fluid that `canDrown`.
    pub fn eye_in_drowning_fluid(&self) -> bool {
        self.fluids
            .modded
            .is_some_and(|m| m.eyes_inside && m.fluid.info().entity.can_drown)
    }
    /// `FloatGoal.canUse` / the brains' `Swim`: deeper than the jump threshold
    /// in water or a mod fluid that `canSwim`, or in lava.
    pub fn floats(&self) -> bool {
        let threshold = self.fluid_jump_threshold();
        self.fluids.water > threshold
            || self.fluids.in_lava
            || self.swim_fluid().is_some_and(|m| m.height > threshold)
    }
    /// `Entity.isPushedByFluid`: water animals, turtles, axolotls, frogs and
    /// nautiluses keep their own course.
    fn pushed_by_fluid(&self) -> bool {
        use super::Species as S;
        !matches!(
            self.species(),
            S::Tadpole
                | S::Cod
                | S::Salmon
                | S::TropicalFish
                | S::Pufferfish
                | S::Squid
                | S::GlowSquid
                | S::Dolphin
                | S::Turtle
                | S::Axolotl
                | S::Frog
                | S::Nautilus
                | S::ZombieNautilus
        )
    }
    fn affected_by_fluids(&self) -> bool {
        true
    }
    /// Entity.moveRelative: input rotated by yaw and scaled to `speed`.
    pub fn move_relative(&mut self, speed: f32, input: Vec3) {
        let len_sq = input.x * input.x + input.y * input.y + input.z * input.z;
        if len_sq < 1.0e-7 {
            // `getInputVector` is `Vec3.ZERO`, still added to the motion: a -0.0 becomes 0.0.
            self.velocity = Vec3::new(
                self.velocity.x + 0.0,
                self.velocity.y + 0.0,
                self.velocity.z + 0.0,
            );
            return;
        }
        let v = if len_sq > 1.0 {
            // Vec3.normalize
            let l = len_sq.sqrt();
            if l < f64::from(1.0e-5_f32) {
                Vec3::ZERO
            } else {
                Vec3::new(input.x / l, input.y / l, input.z / l)
            }
        } else {
            input
        };
        let v = Vec3::new(
            v.x * f64::from(speed),
            v.y * f64::from(speed),
            v.z * f64::from(speed),
        );
        let angle = f64::from(self.yaw * trig::DEG_TO_RAD);
        let (s, c) = (f64::from(trig::sin(angle)), f64::from(trig::cos(angle)));
        self.velocity = Vec3::new(
            self.velocity.x + (v.x * c - v.z * s),
            self.velocity.y + v.y,
            self.velocity.z + (v.z * c + v.x * s),
        );
    }
    /// Entity.move with block collisions and step-up; updates collision flags
    /// and fall distance (Entity.checkFallDamage).
    /// `EntityGetter.getEntityCollisions(this, box.expandTowards(movement))`:
    /// the boxes of the mobs whose `canBeCollidedWith(this)` holds (a living
    /// shulker, a happy ghast held still), but not this mob's vehicle or a
    /// fellow passenger (`Entity.canCollideWith`).
    fn entity_collisions(&self, w: &MobWorld, movement: Vec3) -> Vec<Aabb> {
        let area = self.bounds().expand_towards(movement);
        if area.average_size() < 1.0e-7 {
            return Vec::new();
        }
        let area = area.inflate(Vec3::new(1.0e-7, 1.0e-7, 1.0e-7));
        let mover = self.species();
        let blocks = |s: &&super::Seen| {
            s.id != self.id
                && s.blocks_mob(mover)
                && s.bounds.intersects(area)
                && !self.passenger_of_same_vehicle(s.id, s.vehicle)
        };
        match w.others_solid {
            Some(slots) => slots
                .iter()
                .map(|&i| &w.others[i as usize])
                .filter(blocks)
                .map(|s| s.bounds)
                .collect(),
            None => w.others.iter().filter(blocks).map(|s| s.bounds).collect(),
        }
    }
    /// `Entity.isPassengerOfSameVehicle` toward the entity `id` riding
    /// `vehicle`: one rides the other, or both ride the same one.
    fn passenger_of_same_vehicle(&self, id: u32, vehicle: Option<u32>) -> bool {
        self.extra.mount == Some(id)
            || vehicle == Some(self.id)
            || (self.extra.mount.is_some() && vehicle == self.extra.mount)
    }
    pub fn do_move(&mut self, w: &MobWorld, events: &mut Vec<MobEvent>) {
        let mut wanted = self.velocity;
        // Entity.move: a block the mob is stuck in (sweet berry bush) scales
        // this move once and stops it.
        if self.stuck_multiplier.length_squared() > 1.0e-7 {
            wanted = wanted.multiply(self.stuck_multiplier);
            self.stuck_multiplier = Vec3::ZERO;
            self.velocity = Vec3::ZERO;
        }
        let step = self.kind.step_height as f32;
        let scene = SceneCollisions {
            world: w.world,
            binding: w.collision,
            subject: self.kind.flags.lava_walker.then(|| PowderSnowSubject {
                fall_distance: self.fall_distance,
                lava_walker: true,
                ..Default::default()
            }),
        };
        // Entity.collide: the world border's walls stop the mob too, and so
        // do the entities it may not pass through.
        let border = w.border.map(|b| (b, [self.position.x, self.position.z]));
        let entities = self.entity_collisions(w, wanted);
        let moved = scene
            .resolve_in_border_with(
                self.bounds(),
                wanted,
                step,
                self.on_ground,
                border,
                &entities,
            )
            .unwrap_or(Vec3::ZERO);
        let length = moved.x * moved.x + moved.y * moved.y + moved.z * moved.z;
        let wanted_length = wanted.x * wanted.x + wanted.y * wanted.y + wanted.z * wanted.z;
        if length > 1.0e-7 || wanted_length - length < 1.0e-7 {
            // A move of a block or more through water or a reset block (a
            // vine, a ladder) resets the fall before the move's drop counts.
            if self.fall_distance != 0.0 && length >= 1.0 && fall_reset_hit(w, self.position, moved)
            {
                self.fall_distance = 0.0;
            }
            self.position = Vec3::new(
                self.position.x + moved.x,
                self.position.y + moved.y,
                self.position.z + moved.z,
            );
        }
        // !Mth.equal(delta, movement)
        let differs = |a: f64, b: f64| (b - a).abs() >= f64::from(1.0e-5_f32);
        let x_hit = differs(wanted.x, moved.x);
        let z_hit = differs(wanted.z, moved.z);
        self.horizontal_collision = x_hit || z_hit;
        // A server's mobs are authoritative (`isLocalInstanceAuthoritative`):
        // the vertical flags update even without vertical movement.
        let vertical = wanted.y != moved.y;
        self.vertical_collision_below = vertical && wanted.y < 0.0;
        self.on_ground = self.vertical_collision_below;
        // Entity.setOnGroundWithMovement's checkSupportingBlock, before
        // checkFallDamage reads the landing block (getOnPosLegacy).
        self.supporting = self.supporting_block(w, moved);
        self.species_check_fall_damage(w, moved, events);
        self.restitute_movement(w.world, w.movement, wanted, moved);
        self.movement_emission(w, moved, events);
        // Entity.move's last step: soul sand, honey and the like slow the
        // horizontal speed of whatever the mob stands in or on.
        let factor = self.block_speed_factor(w);
        self.velocity = horizon_world::entity_move::scale_by_block_speed(self.velocity, factor);
    }
    /// `Entity.restituteMovementAfterCollisions`, shared by travel and piston pushes.
    pub fn restitute_movement(
        &mut self,
        world: &crate::world::World,
        movement: &horizon_world::movement_properties::BoundMovementProperties,
        wanted: Vec3,
        moved: Vec3,
    ) {
        let differs = |a: f64, b: f64| (b - a).abs() >= f64::from(1.0e-5_f32);
        let x_hit = differs(wanted.x, moved.x);
        let z_hit = differs(wanted.z, moved.z);
        let vertical = wanted.y != moved.y;
        if wanted.y.abs() > 0.0 && vertical || x_hit || z_hit {
            let on = (
                self.position.x.floor() as i32,
                (self.position.y - f64::from(0.2_f32)).floor() as i32,
                self.position.z.floor() as i32,
            );
            let input = RestitutionInput {
                block: world.block(on.0, on.1, on.2),
                collided_x: x_hit,
                collided_z: z_hit,
                vertical_collision: vertical,
                collision_below: vertical && wanted.y < 0.0,
                living: true,
                resolved_movement: moved,
                fall_distance: self.fall_distance,
            };
            // Unknown block states have no bounce; the provider cannot fail.
            let _ = restitute_movement(
                &mut MobRestitution {
                    mob: self,
                    world,
                    movement,
                },
                input,
            );
        }
    }
    /// `Entity.getBlockSpeedFactor` at the mob's feet.
    fn block_speed_factor(&self, w: &MobWorld) -> f32 {
        crate::motion::block_speed_factor_at(
            w.world,
            w.movement,
            self.position,
            self.supporting.map(|[x, y, z]| BlockPos::new(x, y, z)),
        )
    }
    /// `Entity.applyEffectsFromBlocks` as `LivingEntity.aiStep` runs it, after
    /// `travel` has moved the mob and applied the drag of its fluid (not inside
    /// `Entity.move`): bubble columns push the mob, so the push is not slowed
    /// by that tick's drag, and the server shows the surface of one it is at the
    /// top of.
    pub fn apply_effects_from_blocks(&mut self, w: &MobWorld, events: &mut Vec<MobEvent>) {
        let mut tops = Vec::new();
        if crate::motion::bubble_column_push_over(
            w.world,
            w.registry,
            w.facts,
            w.collision,
            self.bounds(),
            &mut self.velocity,
            |pos| tops.push(pos),
        ) {
            self.fall_distance = 0.0;
        }
        for pos in tops {
            events.push(MobEvent::BubbleColumnParticles {
                pos: [pos.x, pos.y, pos.z],
            });
        }
    }
    /// `checkFallDamage` as the species has it: Strider's lava breaks any fall
    /// (the fluid state waits for the next baseTick); Bat's and Bee's do nothing.
    fn species_check_fall_damage(&mut self, w: &MobWorld, moved: Vec3, events: &mut Vec<MobEvent>) {
        if self.kind.flags.lava_walker && self.fluids.in_lava {
            self.fall_distance = 0.0;
        } else if !matches!(self.kind.species, super::Species::Bat | super::Species::Bee) {
            self.check_fall_damage(w, moved, events);
        }
    }
    /// `handleMoveVehicle` for a mount its rider's client moves, after the move:
    /// `setOnGroundWithMovement` with the client's ground (`checkSupportingBlock`),
    /// then `doCheckFallDamage` with the client's move `delta`. The server's
    /// `Entity.move` counted no fall: it isn't the mount's authority.
    pub fn client_moved_fall(
        &mut self,
        w: &MobWorld,
        delta: Vec3,
        on_ground: bool,
        events: &mut Vec<MobEvent>,
    ) {
        self.on_ground = on_ground;
        self.supporting = self.supporting_block(w, delta);
        self.species_check_fall_damage(w, delta, events);
    }
    /// LivingEntity.checkFallDamage: the fluid state refreshed out of water
    /// (touching water resets the fall), then Entity.checkFallDamage.
    fn check_fall_damage(&mut self, w: &MobWorld, moved: Vec3, events: &mut Vec<MobEvent>) {
        if !self.fluids.in_water {
            self.update_fluids(w, events);
        }
        // Strider.checkFallDamage: in lava the fall is reset and the base never
        // runs, so a strider that drops into a lava sea lands with no damage and
        // no dust, and walks out of it owing nothing.
        if self.species() == super::Species::Strider && self.fluids.in_lava {
            self.fall_distance = 0.0;
            return;
        }
        if !self.fluids.in_water && moved.y < 0.0 {
            self.fall_distance -= f64::from(moved.y as f32);
        }
        if self.on_ground {
            if self.fall_distance > 0.0 {
                self.landing_dust(w, events);
                self.fall_on_block(w, events);
            }
            self.fall_distance = 0.0;
        }
    }
    /// The block particles `LivingEntity.checkFallDamage` sends up from the
    /// block a mob lands on, by the power of the fall.
    fn landing_dust(&self, w: &MobWorld, events: &mut Vec<MobEvent>) {
        let pos = self.block_on_legacy(w);
        let state = w.world.block(pos[0], pos[1], pos[2]);
        let Some(count) = landing_dust_count(self.fall_distance, self.kind.safe_fall_distance)
        else {
            return;
        };
        if w.facts.is_air(state) {
            return;
        }
        let at = landing_dust_at(self.position, pos);
        events.push(MobEvent::LandingDust {
            state,
            at: [at.x, at.y, at.z],
            count,
            // `sendParticles(..., 0.15)`: the crumbs fly up a little.
            speed: f64::from(0.15_f32),
        });
    }
    /// Entity.updateFluidInteraction: the fluid state, then touching water
    /// resets the fall and entering it (after the first tick) splashes.
    pub(super) fn update_fluids(&mut self, w: &MobWorld, events: &mut Vec<MobEvent>) {
        let was_touching_water = self.fluids.in_water;
        self.fluids = w.fluids(self.bounds(), self.eye_y());
        // EntityFluidInteraction.applyCurrentTo: the flow pushes the mob.
        if (self.fluids.in_water || self.fluids.in_lava) && self.pushed_by_fluid() {
            self.velocity = w.pushed_by_currents(self.bounds(), self.velocity);
        }
        // A mod fluid pushes by its `motionScale` when it `canPushEntity`, and
        // scales the fall distance by its `fallDistanceModifier`.
        if let Some(m) = self.fluids.modded {
            let numbers = m.fluid.info().entity;
            if numbers.can_push && self.pushed_by_fluid() {
                let (current, _) = w.fluid_currents(self.bounds(), Some(m.fluid));
                let _ = current.apply_to(&mut Pushed(&mut self.velocity), numbers.motion_scale);
            }
            self.fall_distance *= f64::from(numbers.fall_distance_modifier);
        }
        if self.fluids.in_water {
            self.fall_distance = 0.0;
            if !was_touching_water && !self.first_tick {
                // The splash is the mob's own: Horizon's mobs carry no
                // controlling passenger here.
                let _ = horizon_world::water_splash::water_splash(
                    &mut Splash { mob: self, events },
                    1_000_000,
                );
            }
        }
    }
    /// Entity.applyMovementEmissionAndPlaySound: walked distance, footsteps
    /// and swimming sounds.
    fn movement_emission(&mut self, w: &MobWorld, moved: Vec3, events: &mut Vec<MobEvent>) {
        if !super::ai::emits_movement(self) || self.extra.mount.is_some() {
            return;
        }
        let full = (moved.length() * f64::from(0.6_f32)) as f32;
        let horizontal =
            ((moved.x * moved.x + moved.z * moved.z).sqrt() * f64::from(0.6_f32)) as f32;
        let supporting = self.supporting;
        let on = self.supported_on_pos(w, supporting, 1.0e-5);
        let state = w.world.block(on[0], on[1], on[2]);
        let climbable = w.facts.get(state).name.as_ref() == "minecraft:powder_snow"
            || w.has_tag("minecraft:climbable", &w.facts.get(state).name);
        let steps = &mut self.ai.steps;
        steps.move_dist += if climbable { full } else { horizontal };
        steps.fly_dist += full;
        if steps.move_dist > steps.next_step && !w.facts.is_air(state) {
            let legacy = self.supported_on_pos(w, supporting, 0.2);
            let legacy_state = w.world.block(legacy[0], legacy[1], legacy[2]);
            let produced = !w.facts.is_air(legacy_state)
                && (self.on_ground || climbable)
                && !self.fluids_swimming();
            if produced {
                // `getMovementEmission().emitsSounds()`: the footfall is played
                // only for a type whose emission is `ALL`, while the step game
                // event below is sent by `EVENTS` types too.
                let heard = super::ai::movement_sounds(self);
                if super::equine::hoof_steps(self.species()) {
                    if heard && !self.silent {
                        super::equine::play_step(self, w, legacy, legacy_state, events);
                    }
                } else if let Some((sound, volume, pitch)) =
                    super::ai::step_sound(self, w, legacy, legacy_state)
                    && heard
                    && !self.silent
                {
                    let at = [self.position.x, self.position.y, self.position.z];
                    events.push(MobEvent::Sound(sound, at, volume, pitch));
                }
                // Entity.vibrationAndSoundEffectsFromBlock: the step's game
                // event, with the supporting block's state when the effect
                // block is another.
                events.push(MobEvent::GameEvent {
                    event: "minecraft:step",
                    at: [self.position.x, self.position.y, self.position.z],
                    source: self.id,
                    state: if legacy == on { legacy_state } else { state },
                });
                self.ai.steps.next_step = self.next_step();
            } else if self.fluids.in_water {
                self.ai.steps.next_step = self.next_step();
                // Entity.waterSwimSound, and SkeletonHorse.playSwimSound's
                // mapping of the volume it works out.
                let v = self.velocity;
                let speed =
                    (v.x * v.x * f64::from(0.2_f32) + v.y * v.y + v.z * v.z * f64::from(0.2_f32))
                        .sqrt() as f32;
                let volume = super::equine::swim_volume(self, 1.0_f32.min(speed * 0.35));
                if !super::ai::movement_sounds(self) {
                    return;
                }
                let pitch = 1.0 + (self.random.next_float() - self.random.next_float()) * 0.4;
                // Entity.playSwimSound asks getSwimSound for its sound even
                // when the mob is silent, and a skeleton horse counts there.
                let sound = super::ai::swim_sound(self);
                if !self.silent {
                    let at = [self.position.x, self.position.y, self.position.z];
                    events.push(MobEvent::Sound(sound, at, volume, pitch));
                }
            }
        } else if w.facts.is_air(state)
            && self.species() == super::Species::Bee
            && !self.on_ground
            && self.age.is_multiple_of(2)
        {
            // Entity.processFlappingMovement: a flying bee beats its wings on
            // every other tick (Bee.isFlapping, TICKS_PER_FLAP is 2).
            events.push(MobEvent::GameEvent {
                event: "minecraft:flap",
                at: [self.position.x, self.position.y, self.position.z],
                source: self.id,
                state: horizon_world::registry::BlockStateId(0),
            });
        } else if w.facts.is_air(state) {
            // Entity.processFlappingMovement: onFlap runs before the game
            // event. Silent birds still emit it, independently of sound.
            let flapped = match self.species() {
                super::Species::Chicken => super::farm::chicken_process_flapping(self),
                super::Species::Parrot => super::parrot::process_flapping(self, events),
                _ => false,
            };
            if flapped {
                events.push(MobEvent::GameEvent {
                    event: "minecraft:flap",
                    at: [self.position.x, self.position.y, self.position.z],
                    source: self.id,
                    state: horizon_world::registry::BlockStateId(0),
                });
            }
        }
    }
    /// `Entity.nextStep`: the next whole metre walked; a warden's steps come
    /// every 0.55, a strider's every 0.6, a turtle's every 0.15.
    fn next_step(&self) -> f32 {
        let walked = self.ai.steps.move_dist;
        if self.kind.species == super::Species::Warden {
            walked + 0.55
        } else if self.kind.species == super::Species::Strider {
            walked + 0.6
        } else if self.kind.species == super::Species::Turtle {
            walked + 0.15
        } else {
            (walked as i32).wrapping_add(1) as f32
        }
    }
    /// Entity.isSwimming (mobs never swim in the player sense).
    fn fluids_swimming(&self) -> bool {
        false
    }
    /// `Entity.checkSupportingBlock` after a move: on the ground, the block
    /// under the feet (`mainSupportingBlockPos`), looked for again where
    /// the mob came from if none is under it now.
    fn supporting_block(&self, w: &MobWorld, moved: Vec3) -> Option<[i32; 3]> {
        if !self.on_ground {
            return None;
        }
        let b = self.bounds();
        use horizon_core::geometry::Axis;
        let area = Aabb::new(
            Vec3::new(b.min(Axis::X), b.min(Axis::Y) - 1.0e-6, b.min(Axis::Z)),
            Vec3::new(b.max(Axis::X), b.min(Axis::Y), b.max(Axis::Z)),
        );
        w.supporting_block(area, self.position).or_else(|| {
            let back = Vec3::new(-moved.x, 0.0, -moved.z);
            let shifted = Aabb::new(
                Vec3::new(
                    area.min(Axis::X) + back.x,
                    area.min(Axis::Y),
                    area.min(Axis::Z) + back.z,
                ),
                Vec3::new(
                    area.max(Axis::X) + back.x,
                    area.max(Axis::Y),
                    area.max(Axis::Z) + back.z,
                ),
            );
            w.supporting_block(shifted, self.position)
        })
    }
    /// `Entity.getBlockStateOnLegacy`'s position: 0.2 below the feet, or the
    /// supporting block.
    pub(super) fn block_on_legacy(&self, w: &MobWorld) -> [i32; 3] {
        self.supported_on_pos(w, self.supporting, 0.2)
    }
    /// `Entity.getOnPos(offset)` with a supporting block: its column (its
    /// own position for fences, walls and gates up to half a block down).
    fn supported_on_pos(
        &self,
        w: &MobWorld,
        supporting: Option<[i32; 3]>,
        offset: f32,
    ) -> [i32; 3] {
        let Some(p) = supporting else {
            return self.on_pos(offset);
        };
        if offset <= 1.0e-5 {
            return p;
        }
        let name = &w.facts.get(w.world.block(p[0], p[1], p[2])).name;
        let tall = [
            "minecraft:fences",
            "minecraft:walls",
            "minecraft:fence_gates",
        ]
        .iter()
        .any(|t| w.has_tag(t, name));
        if offset <= 0.5 && tall {
            p
        } else {
            [
                p[0],
                (self.position.y - f64::from(offset)).floor() as i32,
                p[2],
            ]
        }
    }
    /// Entity.getOnPos(offset) without a supporting block record.
    pub fn on_pos(&self, offset: f32) -> [i32; 3] {
        [
            self.position.x.floor() as i32,
            (self.position.y - f64::from(offset)).floor() as i32,
            self.position.z.floor() as i32,
        ]
    }
    /// `Block.fallOn` of the block under the mob (`Entity.checkFallDamage`):
    /// the block's own rule (hay, honey, slime, powder snow, a bed, a
    /// stalagmite, farmland, a turtle egg) decides how much of the fall hurts.
    fn fall_on_block(&mut self, w: &MobWorld, events: &mut Vec<MobEvent>) {
        let pos = self.block_on_legacy(w);
        let state = w.world.block(pos[0], pos[1], pos[2]);
        let species = self.species();
        let entity = if super::doors::is_zombie(species) {
            LandingEntityKind::Zombie
        } else if species == super::Species::Turtle {
            LandingEntityKind::Turtle
        } else if species == super::Species::Bat {
            LandingEntityKind::Bat
        } else {
            LandingEntityKind::Living
        };
        let distance = self.fall_distance;
        let result = w.landing.fall_on(
            &mut super::landing::MobLanding {
                mob: self,
                w,
                events,
                pos,
            },
            state,
            BlockPos::new(pos[0], pos[1], pos[2]),
            entity,
            distance,
        );
        // A block outside the landing registry falls back to the plain fall.
        if result.is_err() {
            self.cause_fall_damage_at(
                distance,
                1.0,
                events,
                Some((w, pos)),
                super::DamageKind::Fall,
            );
        }
    }
    fn mod_block_fall(
        &self,
        contact: Option<(&MobWorld, [i32; 3])>,
        distance: f64,
        damage: f32,
    ) -> f32 {
        contact.map_or(damage, |(w, p)| {
            crate::block_contact::fall(
                crate::block_contact::Contact {
                    entity: self.id as i32,
                    kind: &self.kind.id,
                    block: w.block_name(p[0], p[1], p[2]),
                    state: w.world.block(p[0], p[1], p[2]),
                    position: horizon_core::position::BlockPos::new(p[0], p[1], p[2]),
                },
                distance,
                damage,
            )
        })
    }
    /// `LivingEntity.causeFallDamage`: a fall of `distance` blocks at `modifier`
    /// (its own landing, or a vehicle's passed on by `propagateFallToPassengers`)
    /// brings the species' sounds and reactions, the damage, and the fall
    /// handed to whoever rides it. `AbstractHorse.causeFallDamage` hurts the
    /// horse first and passes the fall on only when it was hurt.
    pub fn cause_fall_damage_from(
        &mut self,
        distance: f64,
        modifier: f32,
        events: &mut Vec<MobEvent>,
    ) {
        self.cause_fall_damage_at(distance, modifier, events, None, super::DamageKind::Fall);
    }
    /// True when the fall hurt (`causeFallDamage`'s result).
    pub(super) fn cause_fall_damage_at(
        &mut self,
        distance: f64,
        modifier: f32,
        events: &mut Vec<MobEvent>,
        contact: Option<(&MobWorld, [i32; 3])>,
        source: super::DamageKind,
    ) -> bool {
        // Breeze.causeFallDamage: a landing from higher than three blocks.
        if self.species() == super::Species::Breeze && distance > 3.0 {
            let at = [self.position.x, self.position.y, self.position.z];
            events.push(MobEvent::Sound("entity.breeze.land", at, 1.0, 1.0));
        }
        // Creeper.causeFallDamage: the fall swells the fuse, up to five ticks
        // short of exploding.
        if self.species() == super::Species::Creeper {
            self.swell = (self.swell + (distance * 1.5) as i32).min(self.max_swell - 5);
        }
        // AbstractHorse.causeFallDamage (llamas override it): a landing of more
        // than a block thuds, and the horse's fall makes no generic fall sound.
        let thud =
            super::equine::is_equine(self.species()) && !super::llama::is_llama(self.species());
        if thud && distance > 1.0 && !self.silent {
            let at = [self.position.x, self.position.y, self.position.z];
            let sound = if self.is_baby() {
                "entity.baby_horse.land"
            } else {
                "entity.horse.land"
            };
            events.push(MobEvent::Sound(sound, at, 0.4, 1.0));
        }
        // AbstractHorse.causeFallDamage hurts the horse first and passes the fall
        // on to its riders only when it was hurt.
        let horse = super::equine::is_equine(self.species());
        // `Block.fallOn` is told the fall as it was before the mods' changes.
        let landed = distance;
        let mut distance = distance;
        let mut modifier = modifier;
        if !horse {
            let mut cancel = false;
            crate::landing::native_living_fall(
                self.id,
                &self.kind.id,
                &mut distance,
                &mut modifier,
                &mut cancel,
            );
            if cancel {
                self.mod_block_fall(contact, landed, 0.0);
                return false;
            }
        }
        // Entity.causeFallDamage: nothing happens to a fall immune entity, nor
        // to those on it.
        if self.kind.flags.fall_immune {
            self.mod_block_fall(contact, landed, 0.0);
            return false;
        }
        let power = distance + 1.0e-6 - self.kind.safe_fall_distance;
        let mut damage = (power * f64::from(modifier) * self.kind.fall_damage_multiplier).floor();
        // Frog.calculateFallDamage: five points less than the base damage;
        // Goat.calculateFallDamage: ten, so a long jump down a ledge is safe.
        match self.species() {
            super::Species::Frog => damage -= 5.0,
            super::Species::Goat => damage -= 10.0,
            _ => {}
        }
        let damage = f64::from(self.mod_block_fall(contact, landed, damage as f32));
        // LivingEntity.causeFallDamage lets the riders fall first.
        if !horse {
            self.fall_to_passengers(distance, modifier, events);
        }
        if damage > 0.0 {
            // Llama.causeFallDamage: shorter damaging falls play the block
            // sound but do not hurt the llama or its riders.
            if super::llama::is_llama(self.species()) && distance < 6.0 {
                self.block_fall_sound(contact, events);
                return true;
            }
            let at = [self.position.x, self.position.y, self.position.z];
            // Monster.getFallSounds: the hostile pair.
            let monster = super::ai::is_monster(self.species());
            let sound = match (monster, damage > 4.0) {
                (true, true) => "entity.hostile.big_fall",
                (true, false) => "entity.hostile.small_fall",
                (false, true) => "entity.generic.big_fall",
                (false, false) => "entity.generic.small_fall",
            };
            // Horses thud and llamas make no generic fall sound.
            if !horse {
                events.push(MobEvent::Sound(sound, at, 1.0, 1.0));
                self.block_fall_sound(contact, events);
            }
            self.damage(damage as f32, super::Damage::of(source));
            if horse {
                self.fall_to_passengers(distance, modifier, events);
                self.block_fall_sound(contact, events);
            }
            return true;
        }
        false
    }
    /// `LivingEntity.playBlockFallSound`: the block 0.2 below the feet plays
    /// its fall sound at half its volume and three quarters of its pitch,
    /// unless the mob is silent or the block is air.
    fn block_fall_sound(&self, contact: Option<(&MobWorld, [i32; 3])>, events: &mut Vec<MobEvent>) {
        if self.silent {
            return;
        }
        if let Some((w, _)) = contact {
            let pos = self.on_pos(0.2);
            let state = w.world.block(pos[0], pos[1], pos[2]);
            if !w.facts.is_air(state) {
                let sounds = &w.facts.get(state).sounds;
                let at = [self.position.x, self.position.y, self.position.z];
                events.push(MobEvent::SoundId(
                    sounds.fall.to_string(),
                    at,
                    sounds.volume * 0.5,
                    sounds.pitch * 0.75,
                ));
            }
        }
    }
    /// `Entity.propagateFallToPassengers`: the game gives the fall to each
    /// passenger (`causeFallDamage`), a mob or a player, for this mob is
    /// borrowed here.
    fn fall_to_passengers(&self, distance: f64, modifier: f32, events: &mut Vec<MobEvent>) {
        if self.rider.is_some() || self.extra.rider.is_some() {
            events.push(MobEvent::FallToPassengers {
                mob: self.id,
                distance,
                modifier,
            });
        }
    }
    /// LivingEntity.getFrictionInfluencedSpeed.
    fn friction_speed(&self, friction: f32) -> f32 {
        if self.on_ground {
            if friction > 0.6 {
                self.ai.speed * (0.216_000_02 / (friction * friction * friction))
            } else {
                self.ai.speed
            }
        } else {
            self.flying_speed()
        }
    }
    /// `LivingEntity.getFlyingSpeed`: a mount its player steers
    /// (`getControllingPassenger() instanceof Player`) keeps a tenth of its
    /// speed in the air (a horse's jump); anything else 0.02.
    fn flying_speed(&self) -> f32 {
        if super::equine::controlled(self) {
            self.ai.speed * 0.1
        } else {
            0.02
        }
    }
    /// `LivingEntity.getEffectiveGravity`: the gravity, floored to 0.01 while a
    /// Slow Falling mob is on its way down.
    pub fn effective_gravity(&self) -> f64 {
        let gravity = self.gravity();
        if self.velocity.y <= 0.0 && self.has_effect("minecraft:slow_falling") && !gravity.is_nan()
        {
            gravity.min(0.01)
        } else {
            gravity
        }
    }
    /// Entity.getGravity: none while isNoGravity.
    pub fn gravity(&self) -> f64 {
        if self.no_gravity {
            0.0
        } else {
            self.kind.gravity
        }
    }
    /// LivingEntity.travel.
    pub fn travel(&mut self, w: &MobWorld, input: Vec3, events: &mut Vec<MobEvent>) {
        if super::ai::custom_travel(self, w, input, events) {
            return;
        }
        if self.kind.flags.flying {
            return self.travel_flying(w, input, events);
        }
        // shouldTravelInFluid: not where the fluid at its feet holds it up.
        let (x, y, z) = (
            self.position.x.floor() as i32,
            self.position.y.floor() as i32,
            self.position.z.floor() as i32,
        );
        let stands_on_fluid = self.kind.flags.lava_walker && w.fluid(x, y, z) == Fluid::Lava;
        let in_fluid = self.is_in_liquid() || self.fluids.modded.is_some();
        if in_fluid && self.affected_by_fluids() && !stands_on_fluid {
            self.travel_in_fluid(w, input, events);
        } else if self.extra.fall_flying {
            self.travel_fall_flying(w, input, events);
        } else {
            self.travel_in_air(w, input, events);
        }
    }
    /// `LivingEntity.travelFallFlying`: use the mob's existing authoritative move
    /// and damage routes, with the shared `updateFallFlyingMovement` arithmetic.
    fn travel_fall_flying(&mut self, w: &MobWorld, input: Vec3, events: &mut Vec<MobEvent>) {
        if self.on_climbable(w) {
            self.travel_in_air(w, input, events);
            self.extra.fall_flying = false;
            return;
        }
        let before = (self.velocity.x * self.velocity.x + self.velocity.z * self.velocity.z).sqrt();
        self.velocity = crate::motion::fall_flying_movement(
            self.velocity,
            self.head_pitch,
            self.yaw,
            self.effective_gravity(),
        );
        self.do_move(w, events);
        if self.horizontal_collision {
            let damage = ((before
                - (self.velocity.x * self.velocity.x + self.velocity.z * self.velocity.z).sqrt())
                * 10.0
                - 3.0) as f32;
            if damage > 0.0 {
                let monster = super::ai::is_monster(self.species());
                let sound = match (monster, damage as i32 > 4) {
                    (true, true) => "entity.hostile.big_fall",
                    (true, false) => "entity.hostile.small_fall",
                    (false, true) => "entity.generic.big_fall",
                    (false, false) => "entity.generic.small_fall",
                };
                if !self.silent {
                    events.push(MobEvent::Sound(
                        sound,
                        [self.position.x, self.position.y, self.position.z],
                        1.0,
                        1.0,
                    ));
                }
                self.damage(damage, super::Damage::of(super::DamageKind::FlyIntoWall));
            }
        }
    }
    /// LivingEntity.travelFlying: no gravity, `0.91F` drag in the air,
    /// `0.8F` in water and 0.5 in lava. A happy ghast swims as fast as it
    /// flies (`HappyGhast.travel`); the others at 0.02.
    fn travel_flying(&mut self, w: &MobWorld, input: Vec3, events: &mut Vec<MobEvent>) {
        let fluid_speed = if self.species() == super::Species::HappyGhast {
            self.kind.air_speed
        } else {
            0.02
        };
        self.travel_flying_speeds(
            w,
            input,
            events,
            fluid_speed,
            fluid_speed,
            self.kind.air_speed,
        );
    }
    /// `LivingEntity.travelFlying(input, waterSpeed, lavaSpeed, airSpeed)`:
    /// the speeds vary per mob (an allay flies at the speed its move control
    /// set, `Allay.travel`).
    pub fn travel_flying_speeds(
        &mut self,
        w: &MobWorld,
        input: Vec3,
        events: &mut Vec<MobEvent>,
        water_speed: f32,
        lava_speed: f32,
        air_speed: f32,
    ) {
        let (speed, drag) = if self.fluids.in_water {
            (water_speed, f64::from(0.8_f32))
        } else if self.fluids.in_lava {
            (lava_speed, 0.5)
        } else {
            (air_speed, f64::from(0.91_f32))
        };
        self.move_relative(speed, input);
        self.do_move(w, events);
        self.velocity = Vec3::new(
            self.velocity.x * drag,
            self.velocity.y * drag,
            self.velocity.z * drag,
        );
    }
    fn block_friction(&self, w: &MobWorld) -> f32 {
        // Entity.getBlockPosBelowThatAffectsMyMovement: 0.500001 below.
        let x = self.position.x.floor() as i32;
        let z = self.position.z.floor() as i32;
        let y = (self.position.y - 0.500_001).floor() as i32;
        w.facts.get(w.world.block(x, y, z)).friction
    }
    /// `getAttributeValue(BOUNCINESS)`.
    fn entity_bounciness(&self) -> f64 {
        super::sulfur_cube::physics(self).map_or(0.0, |a| f64::from(a.bounciness.clamp(0.0, 1.0)))
    }
    /// `LivingEntity.computeModifiedFriction`.
    fn modified_friction(friction: f32, modifier: f32) -> f32 {
        (1.0 - (1.0 - friction) * modifier).clamp(0.0, 1.0)
    }
    /// `LivingEntity.getAirDrag`: what vertical air drag a bounce applies.
    fn air_drag(&self) -> f32 {
        let base = if self.kind.flags.omnidirectional {
            0.91
        } else {
            0.98
        };
        Self::modified_friction(base, self.air_drag_modifier())
    }
    /// `getAttributeValue(AIR_DRAG_MODIFIER)`.
    fn air_drag_modifier(&self) -> f32 {
        super::sulfur_cube::physics(self).map_or(1.0, |a| a.air_drag_modifier.clamp(0.0, 2048.0))
    }
    /// `LivingEntity.handleOnClimbable` for a mob, which is never a `Player`
    /// that holds still on a ladder: the move across clamped to 0.15 either
    /// way and the fall to 0.15, both the float 0.15F (`Mth.clamp`,
    /// `Math.max`, which keep a NaN).
    fn climbable_velocity(v: Vec3) -> Vec3 {
        let limit = f64::from(0.15f32);
        let y = if v.y.is_nan() { v.y } else { v.y.max(-limit) };
        Vec3::new(v.x.clamp(-limit, limit), y, v.z.clamp(-limit, limit))
    }
    fn travel_in_air(&mut self, w: &MobWorld, input: Vec3, events: &mut Vec<MobEvent>) {
        let friction = if self.on_ground {
            // FRICTION_MODIFIER scales the block's slipperiness.
            let modifier = super::sulfur_cube::physics(self)
                .map_or(1.0, |a| a.friction_modifier.clamp(0.0, 2048.0));
            Self::modified_friction(self.block_friction(w), modifier)
        } else {
            1.0
        };
        let speed = self.friction_speed(friction);
        self.move_relative(speed, input);
        // LivingEntity.handleRelativeFrictionAndCalculateMovement:
        // handleOnClimbable, the move, then the climb up a wall.
        if self.on_climbable(w) {
            self.fall_distance = 0.0;
            self.velocity = Self::climbable_velocity(self.velocity);
        }
        self.do_move(w, events);
        let mut movement = self.velocity;
        if (self.horizontal_collision || self.ai.jumping) && self.on_climbable(w) {
            movement.y = 0.2;
        }
        let mut y = movement.y;
        // LivingEntity.travelInAir: levitation lifts by its amplifier.
        if let Some(&(_, _, amp)) = self
            .extra
            .effects
            .iter()
            .find(|e| e.0 == "minecraft:levitation")
        {
            y += (0.05 * f64::from(amp + 1) - movement.y) * 0.2;
        } else {
            y -= self.effective_gravity();
        }
        // LivingEntity.shouldDiscardFriction (a breeze's, a frog's or a goat's
        // long jump).
        if super::breeze::discards_friction(self)
            || self.extra.frog.discard_friction
            || self.extra.goat.discard_friction
        {
            self.velocity = Vec3::new(movement.x, y, movement.z);
            return;
        }
        let air_drag_modifier = self.air_drag_modifier();
        let drag = Self::modified_friction(0.91, air_drag_modifier);
        let f = friction * drag;
        let vertical = if self.kind.flags.omnidirectional {
            drag
        } else {
            Self::modified_friction(0.98, air_drag_modifier)
        };
        self.velocity = Vec3::new(
            movement.x * f64::from(f),
            y * f64::from(vertical),
            movement.z * f64::from(f),
        );
    }
    fn travel_in_fluid(&mut self, w: &MobWorld, input: Vec3, events: &mut Vec<MobEvent>) {
        let falling = self.velocity.y <= 0.0;
        let old_y = self.position.y;
        let gravity = self.effective_gravity();
        // A mod fluid moves the mob as water (NeoForge's `FluidType.move` left
        // to the default).
        if self.fluids.in_water || !self.fluids.in_lava && self.fluids.modded.is_some() {
            let mut slow = if self.extra.sprinting {
                0.9
            } else {
                self.kind.water_slowdown
            };
            let mut speed = 0.02f32;
            let mut efficiency = self.kind.water_efficiency as f32;
            if !self.on_ground {
                efficiency *= 0.5;
            }
            if efficiency > 0.0 {
                slow += (0.546_000_06 - slow) * efficiency;
                speed += (self.ai.speed - speed) * efficiency;
            }
            self.move_relative(speed, input);
            self.do_move(w, events);
            let mut m = self.velocity;
            // LivingEntity.travelInWater: `horizontalCollision && onClimbable()`,
            // a ladder or scaffolding under water as well as a climbing spider.
            if self.horizontal_collision && self.on_climbable(w) {
                m.y = 0.2;
            }
            m = Vec3::new(
                m.x * f64::from(slow),
                m.y * f64::from(0.8_f32),
                m.z * f64::from(slow),
            );
            self.velocity = fluid_falling(gravity, falling, self.extra.sprinting, m);
        } else {
            self.move_relative(0.02, input);
            self.do_move(w, events);
            if self.fluids.lava <= self.fluid_jump_threshold() {
                self.velocity = Vec3::new(
                    self.velocity.x * 0.5,
                    self.velocity.y * f64::from(0.8_f32),
                    self.velocity.z * 0.5,
                );
                self.velocity =
                    fluid_falling(gravity, falling, self.extra.sprinting, self.velocity);
            } else {
                self.velocity = Vec3::new(
                    self.velocity.x * 0.5,
                    self.velocity.y * 0.5,
                    self.velocity.z * 0.5,
                );
            }
            if gravity != 0.0 {
                self.velocity.y -= gravity / 4.0;
            }
        }
        // jumpOutOfFluid: Entity.isFree wants the box dry and clear of the
        // entities a mob collides with (canBeCollidedWith: living shulkers).
        let m = self.velocity;
        let ahead = self.bounds().move_by(Vec3::new(
            m.x,
            m.y + f64::from(0.6_f32) - self.position.y + old_y,
            m.z,
        ));
        let wide = ahead.inflate(Vec3::new(1.0e-7, 1.0e-7, 1.0e-7));
        if self.horizontal_collision
            && w.no_collision(ahead)
            && !w.contains_any_liquid(ahead)
            && !w.others_near(wide).any(|s| {
                s.id != self.id
                    && s.alive
                    && s.species == super::Species::Shulker
                    && s.bounds.intersects(wide)
            })
        {
            self.velocity.y = f64::from(0.3_f32);
        }
        // `LivingEntity.travelInFluid` ends with
        // `floatInLiquidWhileRidden(FluidTags.ENTITY_FLOATABLE)`: a mount with
        // a passenger that is in `minecraft:can_float_while_ridden` and deeper
        // in the water than its jump threshold rises 0.04 a tick, which is
        // what keeps a ridden horse and its rider afloat.
        if (self.rider.is_some() || self.extra.rider.is_some())
            && w.data
                .entity_has("minecraft:can_float_while_ridden", &self.kind.id)
            && self.fluids.water > self.fluid_jump_threshold()
        {
            self.velocity.y += f64::from(0.04_f32);
        }
    }
    /// The jump part of LivingEntity.aiStep.
    pub fn handle_jumping(&mut self, w: &MobWorld) {
        if self.ai.jumping && self.affected_by_fluids() {
            // `jumpInFluid` also rises in a mod fluid that `canSwim`.
            let swim = self.swim_fluid().map_or(0.0, |m| m.height);
            let (fluid_height, in_water_with_height) = if self.fluids.in_lava {
                (self.fluids.lava, false)
            } else {
                let height = self.fluids.water.max(swim);
                (height, (self.fluids.in_water || swim > 0.0) && height > 0.0)
            };
            let threshold = self.fluid_jump_threshold();
            if !in_water_with_height || self.on_ground && fluid_height <= threshold {
                if !self.fluids.in_lava || self.on_ground && self.fluids.lava <= threshold {
                    if (self.on_ground || in_water_with_height && fluid_height <= threshold)
                        && self.ai.no_jump_delay == 0
                    {
                        self.jump_from_ground(w);
                        self.ai.no_jump_delay = 10;
                    }
                } else {
                    self.jump_in_liquid(true);
                }
            } else {
                self.jump_in_liquid(false);
            }
        } else {
            self.ai.no_jump_delay = 0;
        }
    }
    /// Mob.jumpInLiquid: floaters rise 0.04 per tick, others 0.3; bees 0.01.
    /// A magma cube in lava rises to `0.22 + size * 0.05` instead
    /// (`MagmaCube.jumpInLiquid`).
    fn jump_in_liquid(&mut self, lava: bool) {
        if lava && self.species() == super::Species::MagmaCube {
            self.velocity.y = f64::from(0.22_f32 + self.extra.size as f32 * 0.05);
            self.needs_sync = true;
            return;
        }
        self.velocity.y += if self.species() == super::Species::Bee {
            0.01
        } else if self.ai.nav.can_float {
            // LivingEntity.jumpInLiquid adds 0.04F.
            f64::from(0.04_f32)
        } else {
            0.3
        };
    }
    pub fn jump_from_ground(&mut self, w: &MobWorld) {
        // LivingEntity.jumpFromGround (and the rabbit's and cubes' own,
        // which call it or do the same): needsSync.
        self.needs_sync = true;
        if self.species() == super::Species::Rabbit {
            super::rabbit::jump(self, w);
            crate::motion::native_living_jump(self.id, &self.kind.id);
            return;
        }
        if matches!(
            self.species(),
            super::Species::Slime | super::Species::MagmaCube | super::Species::SulfurCube
        ) {
            super::cube::jump(self, w);
            crate::motion::native_living_jump(self.id, &self.kind.id);
            return;
        }
        let power = self.jump_power(w);
        if power > 1.0e-5 {
            self.velocity.y = f64::from(power).max(self.velocity.y);
            // A sprint jump (a dashing cat or ocelot) adds 0.2 along its yaw.
            if self.extra.sprinting {
                let angle = f64::from(self.yaw * trig::DEG_TO_RAD);
                self.velocity.x += f64::from(-trig::sin(angle)) * 0.2;
                self.velocity.z += f64::from(trig::cos(angle)) * 0.2;
            }
        }
        // Sniffer.jumpFromGround: a hop the move control asked for, taken from
        // nearly standing still, carries a tenth of a block forward.
        if self.species() == super::Species::Sniffer
            && self.ai.mov.speed_modifier > 0.0
            && self.velocity.horizontal_distance_squared() < 0.01
        {
            self.move_relative(0.1, Vec3::new(0.0, 0.0, 1.0));
        }
        crate::motion::native_living_jump(self.id, &self.kind.id);
    }
    /// `LivingEntity.getJumpPower`: the jump power with a multiplier of 1.0.
    pub fn jump_power(&self, w: &MobWorld) -> f32 {
        self.jump_power_with(w, 1.0)
    }
    /// `LivingEntity.getJumpPower(float multiplier)`
    pub fn jump_power_with(&self, w: &MobWorld, multiplier: f32) -> f32 {
        self.attrs.jump_strength as f32 * multiplier * self.block_jump_factor(w)
            + self.jump_boost_power()
    }
    /// `LivingEntity.getJumpBoostPower`: amplifier + 1 times 0.1, or 0.0 without the effect.
    pub fn jump_boost_power(&self) -> f32 {
        self.extra
            .effects
            .iter()
            .find(|e| e.0 == "minecraft:jump_boost")
            .map_or(0.0, |e| 0.1 * (e.2 + 1) as f32)
    }
    /// `Entity.getBlockJumpFactor`: the block the mob stands in, or the one
    /// below that affects movement when the current block has a neutral jump factor of 1.0.
    pub fn block_jump_factor(&self, w: &MobWorld) -> f32 {
        let properties = w.movement;
        let current = w.world.block(
            self.position.x.floor() as i32,
            self.position.y.floor() as i32,
            self.position.z.floor() as i32,
        );
        let below = self.on_pos(0.500_001);
        let lower = w.world.block(below[0], below[1], below[2]);
        let current = properties
            .get(current)
            .map_or(1.0, |p| p.coefficients.jump_factor);
        let lower = properties
            .get(lower)
            .map_or(1.0, |p| p.coefficients.jump_factor);
        if current == 1.0 { lower } else { current }
    }
    /// LivingEntity.updateWalkAnimation (client calculateEntityAnimation).
    pub fn update_walk_animation(&mut self) {
        let dy = if self.kind.flags.omnidirectional {
            self.position.y - self.previous.y
        } else {
            0.0
        };
        let dx = self.position.x - self.previous.x;
        let dz = self.position.z - self.previous.z;
        let distance = (dx * dx + dy * dy + dz * dz).sqrt() as f32;
        self.previous_limb_speed = self.limb_speed;
        if self.alive() {
            let (target, factor) = match self.species() {
                // Creaking.updateWalkAnimation walks the model faster.
                super::Species::Creaking => ((distance * 25.0).min(3.0), 0.4),
                super::Species::Frog => (super::frog::walk_target(self, distance), 0.4),
                s if super::camel::is_camel(s) => super::camel::walk_target(self, distance),
                _ => ((distance * 4.0).min(1.0), 0.4),
            };
            self.limb_speed += (target - self.limb_speed) * factor;
            self.limb_position += self.limb_speed;
        } else {
            self.limb_speed = 0.0;
        }
    }
}
/// LivingEntity.getFluidFallingAdjustedMovement.
fn fluid_falling(gravity: f64, falling: bool, sprinting: bool, m: Vec3) -> Vec3 {
    if gravity != 0.0 && !sprinting {
        let y = if falling && (m.y - 0.005).abs() >= 0.003 && (m.y - gravity / 16.0).abs() < 0.003 {
            -0.003
        } else {
            m.y - gravity / 16.0
        };
        Vec3::new(m.x, y, m.z)
    } else {
        m
    }
}

/// `CollisionGetter.findFreePosition`: within `allowed`, the point nearest
/// `preferred` at which a box of the size, centred there, meets no block.
fn free_position(
    w: &MobWorld,
    allowed: Aabb,
    preferred: Vec3,
    size_x: f64,
    size_y: f64,
) -> Option<Vec3> {
    use horizon_core::geometry::{ShapeOp, VoxelShape};
    let allowed_shape = VoxelShape::from_box(allowed).ok()?;
    if allowed_shape.is_empty() {
        return None;
    }
    let half = Vec3::new(size_x / 2.0, size_y / 2.0, size_x / 2.0);
    let area = allowed.inflate(half);
    let (lo, hi) = (area.min_position(), area.max_position());
    let mut collisions = VoxelShape::empty();
    // getBlockCollisions: the block shapes that reach into the area.
    for x in (lo.x - 1.0e-7).floor() as i32 - 1..=(hi.x + 1.0e-7).floor() as i32 + 1 {
        for y in (lo.y - 1.0e-7).floor() as i32 - 1..=(hi.y + 1.0e-7).floor() as i32 + 1 {
            for z in (lo.z - 1.0e-7).floor() as i32 - 1..=(hi.z + 1.0e-7).floor() as i32 + 1 {
                let boxes = w.block_collision_boxes(x, y, z);
                let touches = boxes.iter().any(|b| {
                    let (a, c) = (b.min_position(), b.max_position());
                    a.x < hi.x && c.x > lo.x && a.y < hi.y && c.y > lo.y && a.z < hi.z && c.z > lo.z
                });
                if !touches {
                    continue;
                }
                for b in boxes {
                    let grown = VoxelShape::from_box(b.inflate(half)).ok()?;
                    collisions = collisions.join(&grown, ShapeOp::OR).ok()?;
                }
            }
        }
    }
    let free = allowed_shape.join(&collisions, ShapeOp::ONLY_FIRST).ok()?;
    free.closest_point(preferred)
}

/// `Entity.doWaterSplashEffect` for a mob: its own random, its splash sound
/// (`getSwimSplashSound`), particles for the client and the SPLASH game event.
struct Splash<'a> {
    mob: &'a mut Mob,
    events: &'a mut Vec<MobEvent>,
}
impl horizon_world::water_splash::WaterSplashWorld for Splash<'_> {
    type Error = std::convert::Infallible;
    type Entity = ();
    type Sound = &'static str;
    fn this_entity(&self) {}
    fn controlling_passenger(&mut self) -> Result<Option<()>, Self::Error> {
        Ok(None)
    }
    fn velocity(&mut self, _: ()) -> Result<Vec3, Self::Error> {
        Ok(self.mob.velocity)
    }
    fn splash_sound(&mut self, _high_speed: bool) -> Result<&'static str, Self::Error> {
        use super::Species;
        // Entity.getSwimSplashSound and getSwimHighSpeedSplashSound: the
        // overrides are Monster's, the hoglin's, the dolphin's and the axolotl's.
        let species = self.mob.species();
        Ok(match species {
            Species::Dolphin => "entity.dolphin.splash",
            Species::Axolotl => "entity.axolotl.splash",
            _ if species == Species::Hoglin || super::ai::is_monster(species) => {
                "entity.hostile.splash"
            }
            _ => "entity.generic.splash",
        })
    }
    fn random_float(&mut self) -> Result<f32, Self::Error> {
        Ok(self.mob.random.next_float())
    }
    fn random_double(&mut self) -> Result<f64, Self::Error> {
        Ok(self.mob.random.next_double())
    }
    fn play_sound(
        &mut self,
        sound: &'static str,
        volume: f32,
        pitch: f32,
    ) -> Result<(), Self::Error> {
        if !self.mob.silent {
            let at = [
                self.mob.position.x,
                self.mob.position.y,
                self.mob.position.z,
            ];
            self.events.push(MobEvent::Sound(sound, at, volume, pitch));
        }
        Ok(())
    }
    fn position(&self) -> Vec3 {
        self.mob.position
    }
    fn width(&self) -> f32 {
        self.mob.width()
    }
    fn particle(
        &mut self,
        kind: horizon_world::water_splash::SplashParticle,
        position: Vec3,
        velocity: Vec3,
    ) -> Result<(), Self::Error> {
        self.events.push(MobEvent::ParticleAt {
            kind: match kind {
                horizon_world::water_splash::SplashParticle::Bubble => "bubble",
                horizon_world::water_splash::SplashParticle::Splash => "splash",
            },
            at: [position.x, position.y, position.z],
            velocity: [velocity.x, velocity.y, velocity.z],
        });
        Ok(())
    }
    fn splash_event(&mut self) -> Result<(), Self::Error> {
        self.events.push(MobEvent::GameEvent {
            event: "minecraft:splash",
            at: [
                self.mob.position.x,
                self.mob.position.y,
                self.mob.position.z,
            ],
            source: self.mob.id,
            state: horizon_world::registry::BlockStateId(0),
        });
        Ok(())
    }
}

/// `Entity.getEyePosition(partialTick)` as the cell the renderer lights an
/// entity from (`Entity.getLightProbePosition` is that position and
/// `EntityRenderer.getPackedLightCoords` takes `BlockPos.containing` of it):
/// `Mth.lerp(partialTick, xo, x)` on each axis, plus `getEyeHeight()` on y.
pub fn eye_probe_cell(previous: Vec3, position: Vec3, eye_height: f64, partial: f64) -> [i32; 3] {
    let at = previous.lerp(position, partial);
    [
        at.x.floor() as i32,
        (at.y + eye_height).floor() as i32,
        at.z.floor() as i32,
    ]
}

/// `Entity.doWaterSplashEffect` for a mob this window is drawing.
/// `Level.addParticle` runs on the client. `ClientLevel.playSeededSound`
/// ignores `Entity.playSound` (`except` is null), and the SPLASH game event
/// stays on the server, so only the bubble and splash particles are kept.
pub fn client_water_splash(mob: &mut Mob, out: &mut Vec<MobEvent>) {
    let mut events = Vec::new();
    let _ = horizon_world::water_splash::water_splash(
        &mut Splash {
            mob,
            events: &mut events,
        },
        1_000_000,
    );
    out.extend(
        events
            .into_iter()
            .filter(|event| matches!(event, MobEvent::ParticleAt { .. })),
    );
}

/// `Entity.restituteMovementAfterCollisions`' native inputs, bound for a mob:
/// its own bounciness attribute, the block's coefficients and the pose of a
/// mob that never sneaks. The phases themselves are
/// `movement_restitution::restitute_movement`, the same port the player and
/// dropped items use.
struct MobRestitution<'a> {
    mob: &'a mut Mob,
    world: &'a crate::world::World,
    movement: &'a horizon_world::movement_properties::BoundMovementProperties,
}
impl<'a> MobRestitution<'a> {
    /// The block's movement data: its coefficients and its bounce tag.
    fn movement(
        &self,
        state: BlockStateId,
    ) -> Result<horizon_world::movement_properties::MovementBlock, ()> {
        self.movement.get(state).map_err(|_| ())
    }
}
impl RestitutionWorld for MobRestitution<'_> {
    // The block data cannot fail here: a state the registry does not know
    // reads as a block with no bounce, never as an error.
    type Error = ();
    /// The entity's own suppression is its sneak pose, and a mob never
    /// sneaks; the block's tag is `block_suppresses_bounce` below.
    fn suppressing_bounce(&mut self) -> Result<bool, ()> {
        Ok(false)
    }
    /// `getAttributeValue(BOUNCINESS)`: a sulfur cube's.
    fn entity_bounciness(&mut self) -> Result<f64, ()> {
        Ok(self.mob.entity_bounciness())
    }
    fn velocity(&mut self) -> Result<Vec3, ()> {
        Ok(self.mob.velocity)
    }
    /// `living_motion::effective_gravity`: the gravity attribute, floored to
    /// 0.01 while a Slow Falling mob is on its way down.
    fn effective_gravity(&mut self) -> Result<f64, ()> {
        Ok(self.mob.effective_gravity())
    }
    fn block_suppresses_bounce(&mut self, state: BlockStateId) -> Result<bool, ()> {
        Ok(self.movement(state)?.suppresses_bounce)
    }
    /// `Entity.getBlockBounciness`: the restitution of the block under the
    /// entity, so a mob landing on a slime block or a bed bounces too.
    fn block_bounce_restitution(&mut self, state: BlockStateId) -> Result<f32, ()> {
        Ok(self.movement(state)?.coefficients.bounce_restitution)
    }
    /// `LivingEntity.getAirDrag`.
    fn air_drag(&mut self) -> Result<f32, ()> {
        Ok(self.mob.air_drag())
    }
    /// `Entity.getBlockPosBelowThatAffectsMyMovement`: `Entity.getOnPos(0.500001F)`,
    /// which keeps the supporting fence gate's or wall's column. The offset is
    /// over 0.5, so the fence case never applies.
    fn movement_support_position(&mut self) -> Result<BlockPos, ()> {
        let position = self.mob.position;
        let offset = f64::from(horizon_world::movement_support::MOVEMENT_SUPPORT_OFFSET);
        let Some(support) = self.mob.supporting else {
            return Ok(BlockPos::new(
                position.x.floor() as i32,
                (position.y - offset).floor() as i32,
                position.z.floor() as i32,
            ));
        };
        let state = self.world.block(support[0], support[1], support[2]);
        let keeps_column = self
            .movement(state)
            .is_ok_and(|b| b.wall || b.coefficients.fence_gate);
        Ok(if keeps_column {
            BlockPos::new(support[0], support[1], support[2])
        } else {
            BlockPos::new(support[0], (position.y - offset).floor() as i32, support[2])
        })
    }
    /// `BlockLandingRegistry.bounce_on`'s only case is the shelf mushroom's
    /// bounce sound, which no path plays yet (`landing.rs` binds it for the
    /// player too and the game's sound code resolves neither).
    fn bounce_on(&mut self, _: BlockStateId, _: BlockPos, _: f64) -> Result<(), ()> {
        Ok(())
    }
    fn bounce_event(&mut self) -> Result<(), ()> {
        Ok(())
    }
    /// `Entity.syncPosition`: the tracker keeps where the mob bounced as a
    /// step of its next move (`SteppedInterpolationTracker.updateTracking`).
    fn mark_position_for_sync(&mut self) {
        self.mob.sync_position = true;
    }
    fn set_velocity(&mut self, velocity: Vec3) -> Result<(), ()> {
        self.mob.velocity = velocity;
        Ok(())
    }
}

#[cfg(test)]
#[path = "living_climbable_tests.rs"]
mod climbable_tests;
#[cfg(test)]
#[path = "living_tests.rs"]
mod tests;
/// `Entity.move`'s fall-reset ray for a mob, with the world's
/// `minecraft:fall_damage_resetting` blocks.
fn fall_reset_hit(w: &MobWorld, from: Vec3, moved: Vec3) -> bool {
    let reset = w.block_tags.get("minecraft:fall_damage_resetting");
    crate::entities::fall_reset_hit(
        w.world,
        w.fluid_tables,
        |p| {
            let name = &w.facts.get(w.world.block(p.x, p.y, p.z)).name;
            reset.is_some_and(|t| t.contains(&**name))
        },
        from,
        moved,
    )
}
