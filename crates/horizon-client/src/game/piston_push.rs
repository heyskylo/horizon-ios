//! `PistonMovingBlockEntity.moveCollidedEntities` and `moveStuckEntities` for
//! the entities the game moves itself: the players and the mobs. The shulker
//! box lid's push (`ShulkerBoxBlockEntity.moveCollidedEntities`) rides along,
//! with `Entity.move(MoverType.SHULKER_BOX, …)` taking the same path.
//!
//! The geometry is in [`horizon_world::piston_push`]; this file supplies the
//! bodies and applies the movement the way `Entity.move(MoverType.PISTON, …)`
//! does: limited to 0.51 blocks per tick per axis (`Entity.limitPistonMovement`),
//! then run through the block collision scan, so a pushed entity stops at a
//! wall instead of passing through it.
//!
use super::{
    Game,
    entity::{Entity, EntityHandle, with_entity, with_entity_mut},
};
use horizon_client::collisions::{
    EntityCollisions, SceneCollisions, no_collision_empty, with_piston_noclip,
};
use horizon_client::motion::MotionWorld;
use horizon_core::geometry::{Aabb, Axis, Direction, ShapeOp, Vec3, border::BorderGeometry};
use horizon_core::position::BlockPos;
use horizon_world::{
    block_collision_scan::BlockCollisionWorld, collision_context::CollisionContext,
    level::data::PushReaction::IgnoreEntity, level::pistons::MovingPiston,
    piston_movement::PistonMovement, piston_push, world_movement::EntityMovementWorld,
};

/// What one piston asks for this tick: where it is, its state, and the progress
/// `moveCollidedEntities` runs at (before the tick stores it).
pub(super) type Push = (BlockPos, MovingPiston, f32);

/// What one shulker box asks for this tick (`moveCollidedEntities`): where it
/// is, its facing as a 3D data value, and the lid box it swept
/// (`Shulker.getProgressDeltaAabb`).
pub(super) type ShulkerPush = ([i32; 3], u8, [f64; 6]);

/// Which entity was pushed, so its storage can be reached after the world
/// borrow ends.
#[derive(Clone, Copy, Hash, PartialEq, Eq)]
enum Target {
    Player,
    Guest(u32),
    Entity(EntityHandle),
}

/// The first position of each moving piston in `block_ticks.moving_pistons`
/// and in `World::moving_pistons`, taken before a tick's pushes.
#[derive(Default)]
struct PistonIndex {
    ticked: horizon_client::fast_hash::FxHashMap<BlockPos, usize>,
    drawn: horizon_client::fast_hash::FxHashMap<[i32; 3], usize>,
}

/// `Entity.getStepHeight` of each body, read before the push.
struct StepHeights {
    player: f32,
    guests: std::collections::HashMap<u32, f32>,
}

/// What the push needs from one entity.
#[derive(Clone, Copy)]
struct Body {
    bounds: Aabb,
    position: Vec3,
    on_ground: bool,
    step_height: f32,
    /// Standing on the piston's own block (`Entity.isSupportedBy`).
    supported: bool,
}

impl Game<'_> {
    /// Push whatever this tick's moving pistons ran into, in the order the
    /// pistons ticked: vanilla pushes from each block entity's own tick, so a
    /// pushed entity collides with the moving blocks that ticked before it at
    /// their new progress and with the ones after it at their old one
    /// (`PistonMovingBlockEntity.getCollisionShape` reads `progress`). The
    /// collision view (`World::moving_pistons`, last synced before these
    /// ticks) takes each piston's new progress right after its own push: a
    /// TNT duper's TNT stops at the block that has not moved yet (#11399).
    pub(super) fn push_piston_entities(&mut self, pushes: Vec<Push>) {
        let mut accumulators = std::collections::HashMap::new();
        // Where each moving piston first sits in both lists, so a clock of a
        // thousand pistons doesn't scan them a thousand times a tick.
        let mut index = PistonIndex::default();
        for (i, (p, _)) in self.block_ticks.moving_pistons.iter().enumerate() {
            index.ticked.entry(*p).or_insert(i);
        }
        for (i, (p, _)) in self.world.moving_pistons.iter().enumerate() {
            index.drawn.entry(*p).or_insert(i);
        }
        let steps = self.push_step_heights();
        let targets = self.piston_push_targets(&pushes, &steps);
        for push in pushes {
            let pos = push.0;
            self.push_for_piston(push, &targets, &steps, &mut accumulators);
            self.advance_piston_collision(pos, &index);
        }
    }

    /// The bodies any of this tick's moving blocks can reach, in
    /// `push_targets` order. A moving block's box sits within a block of its
    /// position and sweeps at most one more, so every box a push tests (its
    /// search area, a honey block's sticky area) lies inside the block grown
    /// by 2. Only the pushed body moves during the pushes, so a body outside
    /// all of those at the start is never reached: its box is read once a
    /// tick instead of once per moving block.
    fn piston_push_targets(&self, pushes: &[Push], steps: &StepHeights) -> Vec<Target> {
        let Some(reach) = pushes
            .iter()
            .map(|(p, ..)| {
                let low = Vec3::new(f64::from(p.x), f64::from(p.y), f64::from(p.z));
                Aabb::new(
                    low.add_vector(Vec3::new(-2.0, -2.0, -2.0)),
                    low.add_vector(Vec3::new(3.0, 3.0, 3.0)),
                )
            })
            .reduce(|a, b| a.union_bounds(b))
        else {
            return Vec::new();
        };
        let at = pushes[0].0;
        self.push_targets()
            .filter(|&target| {
                self.push_body(target, at, steps)
                    .is_some_and(|body| body.bounds.intersects(reach))
            })
            .collect()
    }

    /// `moveEntityByPiston`'s `applyEffectsFromBlocks(previousPos, position)`
    /// for projectile `i`: a cobweb (`WebBlock.entityInside`) or powder snow
    /// (`PowderSnowBlock.entityInside`) its box met on the way makes it stuck
    /// (`makeStuckInBlock`), the last one met deciding. A push moves along one
    /// axis, so the boxes it started and ended in cover the blocks it crossed.
    fn projectile_piston_block_effects(&mut self, i: usize, from: Vec3) {
        let p = &self.creatures.projectiles[i];
        let end = p.aabb();
        let offset = from.subtract(p.position);
        let start = end.move_by(offset);
        let (lo, hi) = (
            Vec3::new(
                start.min_position().x.min(end.min_position().x) + 1.0e-5,
                start.min_position().y.min(end.min_position().y) + 1.0e-5,
                start.min_position().z.min(end.min_position().z) + 1.0e-5,
            ),
            Vec3::new(
                start.max_position().x.max(end.max_position().x) - 1.0e-5,
                start.max_position().y.max(end.max_position().y) - 1.0e-5,
                start.max_position().z.max(end.max_position().z) - 1.0e-5,
            ),
        );
        let mut stuck = None;
        for x in lo.x.floor() as i32..=hi.x.floor() as i32 {
            for y in lo.y.floor() as i32..=hi.y.floor() as i32 {
                for z in lo.z.floor() as i32..=hi.z.floor() as i32 {
                    let name = &self.res.facts().get(self.world.block(x, y, z)).name;
                    if let Some(s) = horizon_client::projectiles::stuck_speed_of(name) {
                        stuck = Some(s);
                    }
                }
            }
        }
        if let Some(stuck) = stuck {
            self.creatures.projectiles[i].stuck_speed = stuck;
        }
    }

    /// `moveEntityByPiston`'s `applyEffectsFromBlocks` for the entity-sensing
    /// blocks (pressure plates, tripwires, buttons, detector rails): the ones
    /// the pushed box covered, from `start` to `end`, sense it during the push
    /// (`entityInside`). So a plate a push carried an entity onto powers its
    /// neighbours before the piston pushes the next entity; the tick's
    /// entity-inside pass would only see it after the pistons. A push moves
    /// along one axis, so its two boxes cover the blocks it crossed.
    fn piston_block_triggers(&mut self, start: Aabb, end: Aabb) {
        let eps = 1.0e-5;
        let (a, b) = (start.min_position(), end.min_position());
        let lo = Vec3::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z));
        let (a, b) = (start.max_position(), end.max_position());
        let hi = Vec3::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z));
        let behaviours = self.res.behaviours();
        let mut sensing = Vec::new();
        for x in (lo.x + eps).floor() as i32..=(hi.x - eps).floor() as i32 {
            for y in (lo.y + eps).floor() as i32..=(hi.y - eps).floor() as i32 {
                for z in (lo.z + eps).floor() as i32..=(hi.z - eps).floor() as i32 {
                    if behaviours.senses_entities(self.world.block(x, y, z)) {
                        sensing.push(BlockPos::new(x, y, z));
                    }
                }
            }
        }
        if sensing.is_empty() {
            return;
        }
        self.with_level(|b, level, ticks| {
            for pos in sensing {
                b.entity_inside(level, ticks, pos);
            }
        });
    }

    /// The collision view of the moving piston at `pos` takes the progress
    /// its tick gave it. One no longer moving keeps its entry until
    /// `sync_moving_pistons`.
    fn advance_piston_collision(&mut self, pos: BlockPos, index: &PistonIndex) {
        let ticked = &self.block_ticks.moving_pistons;
        // The index holds while the pushes leave both lists as they were;
        // otherwise the first match is searched for, as before.
        let found = index
            .ticked
            .get(&pos)
            .filter(|&&i| ticked.get(i).is_some_and(|(p, _)| *p == pos))
            .copied()
            .or_else(|| ticked.iter().position(|(p, _)| *p == pos));
        let Some(state) = found.map(|i| ticked[i].1.shape_state()) else {
            return;
        };
        let at = [pos.x, pos.y, pos.z];
        let drawn = &mut self.world.moving_pistons;
        let slot = index
            .drawn
            .get(&at)
            .filter(|&&i| drawn.get(i).is_some_and(|(p, _)| *p == at))
            .copied()
            .or_else(|| drawn.iter().position(|(p, _)| *p == at));
        if let Some(i) = slot {
            drawn[i].1 = state;
        }
    }

    /// `ShulkerBoxBlockEntity.moveCollidedEntities` for the bodies this game
    /// moves itself: every entity the sweeping lid box meets moves out along
    /// the box's facing by the box's own size plus 0.01
    /// (`Entity.move(MoverType.SHULKER_BOX, …)`).
    pub(super) fn push_shulker_entities(&mut self, pushes: Vec<ShulkerPush>) {
        let steps = self.push_step_heights();
        for (pos, facing, lid) in pushes {
            self.push_for_shulker(pos, facing, lid, &steps);
        }
    }

    /// One box's push, a body at a time. The move resolves as a piston's does
    /// (`Entity.move` stops at the blocks and the world border), without a
    /// piston's per-tick travel limit or its noclip. The local player takes the
    /// client's own move instead (`shulker_push_player`).
    fn push_for_shulker(&mut self, pos: [i32; 3], facing: u8, lid: [f64; 6], steps: &StepHeights) {
        let (movement, area) = shulker_lid(facing, lid);
        let at = BlockPos::new(pos[0], pos[1], pos[2]);
        self.push_targets().for_each(|target| {
            let Some(body) = self.push_body(target, at, steps) else {
                return;
            };
            if !body.bounds.intersects(area) {
                return;
            }
            if target == Target::Player {
                self.shulker_push_player(movement);
                return;
            }
            let Ok(moved) = pushed_movement(
                SceneCollisions {
                    world: &self.world,
                    binding: self.collision,
                    subject: None,
                },
                self.border.geometry(),
                [body.position.x, body.position.z],
                body.bounds,
                movement,
                // `Entity.move(MoverType.SHULKER_BOX, ...)`: steps and reads the ground flag,
                body.step_height,
                body.on_ground,
                None,
            ) else {
                return;
            };
            self.set_pushed_on_ground(target, movement, moved);
            if moved.length_squared() > 0.0 {
                self.apply_push(target, moved);
            }
        });
    }

    /// `ShulkerBoxBlockEntity.moveCollidedEntities` for the player this game
    /// moves itself: the lid's pushes, applied to its own body. A client owns
    /// its player's position (the server thread's guest included), so the push
    /// is the client's own `Entity.move(MoverType.SHULKER_BOX, …)`.
    pub(super) fn push_shulker_local(&mut self, pushes: Vec<ShulkerPush>) {
        for (_, facing, lid) in pushes {
            let (movement, area) = shulker_lid(facing, lid);
            let Some(player) = self.player.as_ref() else {
                return;
            };
            if player.spectator || !player.bounds().intersects(area) {
                continue;
            }
            self.shulker_push_player(movement);
        }
    }

    /// `Entity.move(MoverType.SHULKER_BOX, …)` for the local player: an
    /// ordinary move, so the collision scan steps it and its end writes the
    /// ground state (`Entity.setOnGroundWithMovement`). A lid lifting the body
    /// off the ground leaves it airborne, and the next tick's gravity lands it
    /// on the lid instead of a stale ground flag holding it in place.
    fn shulker_push_player(&mut self, movement: Vec3) {
        let entities = self.player_entity_collisions();
        if let Some(player) = self.player.as_mut() {
            let _ = MotionWorld {
                entities: &entities,
                player,
                scene: &mut self.world,
                assets: &self.pack.assets,
                collision_binding: self.collision,
                resources: self.motion,
                health: &self.health_resources,
            }
            .shulker_box_push(movement);
        }
    }

    fn push_for_piston(
        &mut self,
        (pos, piston, new_progress): Push,
        targets: &[Target],
        steps: &StepHeights,
        accumulators: &mut std::collections::HashMap<Target, PistonMovement>,
    ) {
        let delta_progress = f64::from(new_progress - piston.progress);
        if delta_progress <= 0.0 {
            return;
        }
        let Some(bindings) = self.collision.pistons() else {
            return;
        };
        let Ok(state) = bindings.collision_related_state(piston.shape_state()) else {
            return;
        };
        let mut scene = SceneCollisions {
            world: &self.world,
            binding: self.collision,
            subject: None,
        };
        let Ok(shape) = self
            .collision
            .collision_shape(&mut scene, state, [pos.x, pos.y, pos.z])
        else {
            return;
        };
        if shape.is_empty() {
            return;
        }
        let movement = piston.movement_direction();
        let boxes: Vec<Aabb> = shape.boxes().to_vec();
        let facts = self.res.facts();
        let slime = &*facts.get(piston.moved_state).name == "minecraft:slime_block";
        let honey = &*facts.get(piston.moved_state).name == "minecraft:honey_block";
        // `moveStuckEntities` asks the moved state's own top for its box.
        let sticky_top = if honey {
            let mut scene = SceneCollisions {
                world: &self.world,
                binding: self.collision,
                subject: None,
            };
            self.collision
                .collision_shape(&mut scene, piston.moved_state, [pos.x, pos.y, pos.z])
                .ok()
                .and_then(|s| s.bounds())
                .map_or(0.0, |b| b.max_position().y)
        } else {
            0.0
        };

        let search = shape.bounds().map(|bounds| {
            piston_push::movement_search_area(
                piston_push::move_by_position_and_progress(pos, bounds, piston.shape_state()),
                movement,
                delta_progress,
            )
        });
        targets.iter().copied().for_each(|target| {
            let Some(body) = self.push_body(target, pos, steps) else {
                return;
            };
            // `level.getEntities(null, area)`: a body outside the search area
            // meets none of the boxes (each sweeps a slab inside it) and is
            // not on a slime block, so it is skipped before the box math.
            if search.is_some_and(|area| !area.intersects(body.bounds)) {
                return;
            }
            let amount = piston_push::collided_amount(
                &boxes,
                pos,
                piston.shape_state(),
                new_progress,
                movement,
                body.bounds,
            );
            if slime && search.is_some_and(|area| area.intersects(body.bounds)) {
                self.bounce(target, movement);
            }
            if amount > 0.0 {
                let step = piston_push::step_vector(movement);
                let before = body.bounds;
                self.move_pushed(
                    target,
                    before,
                    [body.position.x, body.position.z],
                    body.step_height,
                    body.on_ground,
                    step.scale(amount),
                    movement,
                    accumulators,
                );
                // `fixEntityWithinPistonBase`: a retracting source piston drags
                // an entity that is still inside its own block.
                if !piston.extending
                    && piston.source
                    && let Some(after) = self.push_body(target, pos, steps)
                    && let Some(fix) = piston_push::fix_within_piston_base(
                        pos,
                        after.bounds,
                        movement,
                        delta_progress,
                    )
                {
                    let back = piston_push::step_vector(movement.opposite());
                    self.move_pushed(
                        target,
                        after.bounds,
                        [after.position.x, after.position.z],
                        after.step_height,
                        after.on_ground,
                        back.scale(fix),
                        movement,
                        accumulators,
                    );
                }
            }
        });
        // Vanilla selects honey passengers after all collided entities have moved.
        if honey && movement.normal()[1] == 0 {
            let area = piston_push::sticky_area(pos, piston.shape_state(), sticky_top);
            for &target in targets {
                let Some(body) = self.push_body(target, pos, steps) else {
                    continue;
                };
                if piston_push::sticky_entity_selected(
                    area,
                    body.bounds,
                    horizon_world::level::data::PushReaction::PushPull,
                    body.on_ground,
                    body.supported,
                    body.position,
                ) {
                    self.move_pushed(
                        target,
                        body.bounds,
                        [body.position.x, body.position.z],
                        body.step_height,
                        body.on_ground,
                        piston_push::step_vector(movement).scale(delta_progress),
                        movement,
                        accumulators,
                    );
                }
            }
        }
    }

    /// `Block.pushEntitiesUp(old, new, level, pos)` for the block now at `pos`
    /// (`FarmlandBlock.turnToBaseBlock`, `PathBlock.turnToBaseBlock`): every body
    /// touching the collision that the new state adds over `old` is lifted
    /// onto it with `teleportRelative`, no collision scan.
    pub(super) fn push_entities_up(
        &mut self,
        pos: BlockPos,
        old: horizon_world::registry::BlockStateId,
    ) {
        let new = self.world.block(pos.x, pos.y, pos.z);
        if new == old {
            return;
        }
        let at = [pos.x, pos.y, pos.z];
        let shape = |state| {
            let mut scene = SceneCollisions {
                world: &self.world,
                binding: self.collision,
                subject: None,
            };
            self.collision.collision_shape(&mut scene, state, at).ok()
        };
        let (Some(old_shape), Some(new_shape)) = (shape(old), shape(new)) else {
            return;
        };
        let Ok(added) = old_shape.join_unoptimized(&new_shape, ShapeOp::ONLY_SECOND) else {
            return;
        };
        let added = added.moved(Vec3::new(
            f64::from(pos.x),
            f64::from(pos.y),
            f64::from(pos.z),
        ));
        let Some(bounds) = added.bounds() else {
            return;
        };
        let steps = self.push_step_heights();
        for target in self.push_targets() {
            let Some(body) = self.push_body(target, pos, &steps) else {
                continue;
            };
            if !body.bounds.intersects(bounds) {
                continue;
            }
            let dy = added.collide(Axis::Y, body.bounds.move_by(Vec3::new(0.0, 1.0, 0.0)), -1.0);
            self.apply_push(target, Vec3::new(0.0, 1.0 + dy, 0.0));
        }
    }

    /// `Entity.getStepHeight` for each body, read once: the attribute lookup
    /// needs the mutable body, and the push only needs the number.
    fn push_step_heights(&mut self) -> StepHeights {
        let player = self
            .player
            .as_mut()
            .and_then(|p| p.attribute("minecraft:step_height").ok())
            .map_or(0.6, |v| v as f32);
        let mut guests = std::collections::HashMap::new();
        if let Some(host) = self.lan.as_mut() {
            for g in &mut host.hub.guests {
                let step = g
                    .motion
                    .as_mut()
                    .and_then(|m| m.attribute("minecraft:step_height").ok())
                    .map_or(0.6, |v| v as f32);
                guests.insert(g.conn, step);
            }
        }
        StepHeights { player, guests }
    }

    /// The entities a moving block can run into: the window's player, the LAN
    /// guests in the level, then the mobs.
    fn push_targets(&self) -> impl Iterator<Item = Target> + use<> {
        let player = self.player.is_some().then_some(Target::Player);
        // Empty, and so unallocated, unless guests are here.
        let guests: Vec<Target> = self.lan.as_ref().map_or_else(Vec::new, |host| {
            host.hub
                .guests
                .iter()
                .filter(|g| host.hub.here(g) && g.loaded && !g.dead)
                .map(|g| Target::Guest(g.conn))
                .collect()
        });
        player
            .into_iter()
            .chain(guests)
            .chain(self.entity_handles().map(Target::Entity))
    }

    /// What the push needs from one entity, or `None` when it is not pushed:
    /// removed, dead, a spectator, or an armor-stand marker.
    fn push_body(&self, target: Target, pos: BlockPos, steps: &StepHeights) -> Option<Body> {
        match target {
            Target::Player => {
                let p = self.player.as_ref()?;
                if p.spectator {
                    return None;
                }
                Some(Body {
                    bounds: p.bounds(),
                    position: p.position,
                    on_ground: p.ground.on_ground,
                    step_height: steps.player,
                    supported: p.ground.supporting_block == Some(pos),
                })
            }
            Target::Guest(conn) => {
                let host = self.lan.as_ref()?;
                let g = host.hub.guests.iter().find(|g| g.conn == conn)?;
                if g.dead || g.game_mode == 3 {
                    return None;
                }
                let p = g.position;
                Some(Body {
                    // `EntityDimensions.makeBoundingBox`: the width is halved as a float.
                    bounds: g.dimensions().bounds(Vec3::new(p[0], p[1], p[2])),
                    position: Vec3::new(p[0], p[1], p[2]),
                    on_ground: g.on_ground,
                    step_height: steps.guests.get(&conn).copied().unwrap_or(0.6),
                    supported: g
                        .motion
                        .as_ref()
                        .is_some_and(|m| m.ground.supporting_block == Some(pos)),
                })
            }
            Target::Entity(handle) => with_entity!(self, handle, entity => {
                if !entity.is_alive() || entity.get_piston_push_reaction() == IgnoreEntity {
                    return None;
                }
                Some(Body {
                    bounds: Entity::bounds(entity),
                    position: entity.position(),
                    on_ground: entity.on_ground(),
                    step_height: entity.get_step_height(),
                    supported: entity.is_supported_by(pos),
                })
            }),
        }
    }

    /// `Entity.move(MoverType.PISTON, movement)` for one pushed entity: the
    /// per-tick piston limit, then the block and world border collision scan.
    fn move_pushed(
        &mut self,
        target: Target,
        before: Aabb,
        at: [f64; 2],
        step_height: f32,
        on_ground: bool,
        movement: Vec3,
        piston_direction: Direction,
        accumulators: &mut std::collections::HashMap<Target, PistonMovement>,
    ) {
        let Some(limited) = self.limit_piston(target, movement, accumulators) else {
            return;
        };
        // `limitPistonMovement` returns zero once the tick's budget is spent.
        if limited == Vec3::ZERO {
            // `Entity.move` stops there; `moveEntityByPiston` still applies
            // the block effects.
            if let Target::Entity(EntityHandle::Tnt(i)) = target {
                let from = self.tnts[i].position;
                self.tnt_piston_block_effects(i, from);
            }
            if let Target::Entity(EntityHandle::Projectile(i)) = target {
                let at = self.creatures.projectiles[i].position;
                self.projectile_piston_block_effects(i, at);
            }
            self.piston_block_triggers(before, before);
            self.release_after_block_move(target);
            return;
        }
        // `Entity.move`: a projectile a cobweb or powder snow holds stops dead
        // (`setDeltaMovement(Vec3.ZERO)`); a piston's push is not scaled.
        let projectile_from = match target {
            Target::Entity(EntityHandle::Projectile(i)) => {
                let p = &mut self.creatures.projectiles[i];
                if p.stuck_speed.length_squared() > 1.0e-7 {
                    p.stuck_speed = Vec3::ZERO;
                    p.velocity = Vec3::ZERO;
                }
                Some((i, p.position))
            }
            _ => None,
        };
        // Primed TNT: a `PISTON` move uses up a pending stuck multiplier
        // without scaling the push, and powder snow reads its fall.
        let tnt = match target {
            Target::Entity(EntityHandle::Tnt(i)) => {
                let t = &mut self.tnts[i];
                t.take_stuck();
                Some((
                    i,
                    t.position,
                    t.powder_snow_subject(),
                    t.collision_context(),
                ))
            }
            _ => None,
        };
        let resolved = with_piston_noclip(piston_direction, || {
            pushed_movement(
                SceneCollisions {
                    world: &self.world,
                    binding: self.collision,
                    subject: tnt.as_ref().map(|t| t.2),
                },
                self.border.geometry(),
                at,
                before,
                limited,
                step_height,
                on_ground,
                tnt.as_ref().map(|t| t.3),
            )
            .unwrap_or(Vec3::ZERO)
        });
        self.set_pushed_on_ground(target, limited, resolved);

        // Entity.move updates ground before its tiny-collision return. This
        // native slice covers only unoccupied server NoAI horizontal moves.
        if let Target::Entity(EntityHandle::Mob(i)) = target {
            let m = &self.mobs[i];
            let eligible = !self.options.execution_role.is_client()
                && self.remote.is_none()
                && limited.y == 0.0
                && m.no_ai
                && !m.extra.passenger
                && m.extra.mount.is_none()
                && m.rider.is_none()
                && m.extra.rider.is_none()
                && self.riding != Some(m.id)
                && self.lan.as_ref().is_none_or(|host| {
                    !host
                        .hub
                        .guests
                        .iter()
                        .any(|g| host.hub.here(g) && g.vehicle == Some(m.id))
                });
            if eligible {
                self.mobs[i].on_ground = limited.y < 0.0 && limited.y != resolved.y;
            }
        }
        // `Entity.move(MoverType.PISTON, …)` restitution, which runs whether or
        // not the step is applied: so a primed TNT in a chunk that does not tick
        // entities still loses its velocity into walls and ceilings and
        // bounces on slime (#12022).
        self.restitute_piston(target, limited, resolved);
        let moved = resolved.length_squared() > 1.0e-7
            || limited.length_squared() - resolved.length_squared() < 1.0e-7;
        if moved {
            self.apply_push(target, resolved);
        } else {
            // A move the collisions cut to nothing still ends in the check.
            self.release_after_block_move(target);
        }
        if let Some((i, from, ..)) = tnt {
            self.finish_tnt_piston_move(i, from, resolved);
        }
        if let Some((i, from)) = projectile_from {
            self.projectile_piston_block_effects(i, from);
        }
        let after = if moved {
            before.move_by(resolved)
        } else {
            before
        };
        self.piston_block_triggers(before, after);
    }

    /// `Entity.restituteMovementAfterCollisions` after a block's push, for a
    /// server entity (`canSimulateMovement`; a player's own client moves it):
    /// an axis the collision cut off stops (no entity bounces off walls), and
    /// a cut-off vertical push stops the fall or rise, except a fall onto
    /// slime fast enough to bounce. The bounce is of the entity's own
    /// velocity, scaled by the part of the push that was made. The ground
    /// flag is set by `set_pushed_on_ground`.
    fn restitute_piston(&mut self, target: Target, limited: Vec3, resolved: Vec3) {
        let Target::Entity(handle) = target else {
            return;
        };
        // `Mth.equal` horizontally, exact vertically, as `Entity.move` compares.
        let collided_x = (limited.x - resolved.x).abs() >= 1.0e-5;
        let collided_z = (limited.z - resolved.z).abs() >= 1.0e-5;
        let moved_vertically = limited.y.abs() > 0.0;
        let vertical = limited.y != resolved.y;
        if !(collided_x || collided_z || moved_vertically && vertical) {
            return;
        }
        let below = vertical && limited.y < 0.0;
        with_entity_mut!(self, handle, entity => {
            let pos = entity.position();
            // `getOnPosLegacy`: the block 0.2 under the feet.
            let on = self.world.block(
                pos.x.floor() as i32,
                (pos.y - 0.2).floor() as i32,
                pos.z.floor() as i32,
            );
            let slime = &*self.res.facts().get(on).name == "minecraft:slime_block";
            let velocity = entity.velocity_mut();
            if collided_x {
                velocity.x = 0.0;
            }
            if collided_z {
                velocity.z = 0.0;
            }
            if vertical {
                velocity.y = crate::entities::vertical_collision_velocity(
                    velocity.y, resolved.y, 0.04, below, slime,
                );
            }
        });
    }

    /// The end of a block's `Entity.move` push: a server entity's ground flag
    /// is set after every push, a sideways one and one the collision turns
    /// away included, on ground only when the push's downward part was blocked.
    fn set_pushed_on_ground(&mut self, target: Target, wanted: Vec3, moved: Vec3) {
        if let Target::Entity(handle) = target {
            let on_ground = wanted.y < 0.0 && wanted.y != moved.y;
            with_entity_mut!(self, handle, entity => entity.set_piston_on_ground(on_ground));
        }
    }

    /// `Entity.limitPistonMovement`: each axis may travel at most 0.51 blocks
    /// per game tick, and a move touches only its first non-zero axis.
    fn limit_piston(
        &mut self,
        target: Target,
        movement: Vec3,
        accumulators: &mut std::collections::HashMap<Target, PistonMovement>,
    ) -> Option<Vec3> {
        let time = self.world.game_time;
        let limit = |state: &mut PistonMovement| state.limit(movement, || Ok::<i64, ()>(time)).ok();
        match target {
            Target::Player => {
                let p = self.player.as_mut()?;
                limit(&mut p.piston)
            }
            Target::Guest(conn) => {
                let host = self.lan.as_mut()?;
                let g = host.hub.guests.iter_mut().find(|g| g.conn == conn)?;
                limit(&mut g.motion.as_deref_mut()?.piston)
            }
            Target::Entity(EntityHandle::Mob(i)) => limit(&mut self.mobs[i].piston),
            _ => limit(accumulators.entry(target).or_default()),
        }
    }

    fn apply_push(&mut self, target: Target, movement: Vec3) {
        match target {
            Target::Player => {
                if let Some(p) = self.player.as_mut() {
                    p.position = p.position.add_vector(movement);
                }
            }
            Target::Guest(conn) => {
                if let Some(host) = self.lan.as_mut()
                    && let Some(g) = host.hub.guests.iter_mut().find(|g| g.conn == conn)
                {
                    // The body and the hub's copy of its position move together:
                    // `leave_guest` writes the body back over the hub's.
                    for (slot, delta) in g.position.iter_mut().zip(movement.components()) {
                        *slot += delta;
                    }
                    if let Some(m) = g.motion.as_deref_mut() {
                        m.position = m.position.add_vector(movement);
                    }
                }
            }
            Target::Entity(handle) => {
                with_entity_mut!(self, handle, entity => entity.move_by_piston(movement));
                self.release_after_block_move(target);
            }
        }
    }

    /// `AbstractArrow.move` with a mover other than the arrow itself (a
    /// piston, a shulker lid): after `Entity.move`, whether or not it went
    /// anywhere, a stuck arrow or trident with nothing to collide with within
    /// 0.06 starts falling (`shouldFall`, `startFalling`). Its own tick only
    /// checks once the block at its position changes, a tick later.
    fn release_after_block_move(&mut self, target: Target) {
        let Target::Entity(handle) = target else {
            return;
        };
        let Some(bounds) = with_entity!(self, handle, entity => entity.piston_release_bounds())
        else {
            return;
        };
        let no_block_collision = {
            let mut scene = SceneCollisions {
                world: &self.world,
                binding: self.collision,
                subject: None,
            };
            no_collision_empty(&mut scene, bounds)
        };
        with_entity_mut!(self, handle, entity => {
            entity.release_after_piston(no_block_collision)
        });
    }

    /// `causeBounce`: a slime block sets the velocity along the movement axis.
    fn bounce(&mut self, target: Target, movement: Direction) {
        match target {
            Target::Player => {
                if let Some(p) = self.player.as_mut() {
                    p.velocity = piston_push::bounce_velocity(movement, p.velocity);
                }
            }
            Target::Guest(conn) => {
                if let Some(host) = self.lan.as_mut()
                    && let Some(guest) = host.hub.guests.iter_mut().find(|g| g.conn == conn)
                    && let Some(motion) = guest.motion.as_deref_mut()
                {
                    motion.velocity = piston_push::bounce_velocity(movement, motion.velocity);
                }
            }
            Target::Entity(handle) => {
                with_entity_mut!(self, handle, entity => Entity::piston_slime_bounce(entity, movement))
            }
        }
    }
}

/// The lid's push: the swept box in world space (`Shulker.
/// getProgressDeltaAabb`) and the movement out along the facing, the box's own
/// size plus 0.01 (`ShulkerBoxBlockEntity.moveCollidedEntities`).
fn shulker_lid(facing: u8, lid: [f64; 6]) -> (Vec3, Aabb) {
    let direction = Direction::ALL[usize::from(facing) % Direction::ALL.len()];
    let [x, y, z] = direction.normal();
    (
        Vec3::new(
            (lid[3] - lid[0] + 0.01) * f64::from(x),
            (lid[4] - lid[1] + 0.01) * f64::from(y),
            (lid[5] - lid[2] + 0.01) * f64::from(z),
        ),
        Aabb::new(
            Vec3::new(lid[0], lid[1], lid[2]),
            Vec3::new(lid[3], lid[4], lid[5]),
        ),
    )
}

/// `Entity.move(MoverType.PISTON, movement)` for a pushed body: the movement
/// it makes. The world's blocks stop it, and so does the world border:
/// `Entity.collectCollidersIgnoringWorldBorder` adds the border's walls when
/// the body is inside close to the border, as ordinary movement does.
fn pushed_movement<W>(
    blocks: W,
    border: &BorderGeometry,
    at: [f64; 2],
    before: Aabb,
    movement: Vec3,
    step_height: f32,
    on_ground: bool,
    context: Option<CollisionContext<'_>>,
) -> horizon_client::Result<Vec3>
where
    W: BlockCollisionWorld<Error = std::io::Error> + EntityMovementWorld<Error = std::io::Error>,
{
    // Without the entity's own context, the one `resolve` reads at its feet.
    let context = context.unwrap_or_else(|| CollisionContext::position(before.min_position().y));
    EntityCollisions::with_border(blocks, border, at).resolve_with_context(
        before,
        movement,
        step_height,
        on_ground,
        context,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::collisions::EmptyCollisions;

    /// A border 10 wide around the origin: its wall's inner face is at x = 5.
    fn border() -> BorderGeometry {
        BorderGeometry::new([0.0, 0.0], 10.0, 29_999_984).expect("a border")
    }

    /// `Entity.collectCollidersIgnoringWorldBorder`: the border's walls stop a
    /// pushed body that is inside close to the border.
    #[test]
    fn a_pushed_body_stops_at_the_world_border() {
        // A body 0.6 wide, 0.4 blocks short of the wall, pushed east into it.
        let before = Aabb::new(Vec3::new(4.0, 0.0, 0.0), Vec3::new(4.6, 1.8, 0.6));
        let border = border();
        let moved = pushed_movement(
            EmptyCollisions,
            &border,
            [4.3, 0.3],
            before,
            Vec3::new(0.51, 0.0, 0.0),
            0.0,
            false,
            None,
        )
        .expect("the push resolves");
        // The wall stops the body 0.4 blocks short of the whole push.
        assert!(
            (moved.x - 0.4).abs() < 1.0e-9,
            "the body was not stopped by the border: {moved:?}"
        );
    }

    /// Away from the border, the same push moves the whole way.
    #[test]
    fn a_pushed_body_moves_whole_away_from_the_border() {
        let before = Aabb::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.6, 1.8, 0.6));
        let border = border();
        let moved = pushed_movement(
            EmptyCollisions,
            &border,
            [0.3, 0.3],
            before,
            Vec3::new(0.51, 0.0, 0.0),
            0.0,
            false,
            None,
        )
        .expect("the push resolves");
        assert!(
            (moved.x - 0.51).abs() < 1.0e-9,
            "the body did not move the whole push: {moved:?}"
        );
    }
}

#[cfg(test)]
mod order_tests {
    use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
    use horizon_client::chunk::Chunk;
    use horizon_core::{geometry::Direction, position::BlockPos};
    use horizon_world::level::pistons::MovingPiston;
    use std::sync::Arc;

    /// A stone pushes a primed TNT east, into the cell a second stone is
    /// sliding north out of (half way there). The TNT collides with that
    /// stone at its progress when the push happens
    /// (`PistonMovingBlockEntity.getCollisionShape`): gone already when it
    /// ticked first, still in the way when it ticks second (#11399). A block
    /// moving the push's own way would not collide at all (`NOCLIP`).
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_pushed_entity_meets_the_other_moving_blocks_as_they_are_at_its_push() {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.game_dir = Some(
            std::env::temp_dir().join(format!("horizon-piston-order-unit-{}", std::process::id())),
        );
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        game.set_allow_commands(true);
        let registry = &game.pack.assets.registry;
        let stone = registry.default_state("minecraft:stone").unwrap();
        let moving = registry.default_state("minecraft:moving_piston").unwrap();
        let mut chunk = Chunk::empty(0, 0);
        for x in 0..16 {
            for z in 0..16 {
                chunk.set(x, 199, z, stone.0);
            }
        }
        chunk.set(5, 200, 5, moving.0);
        chunk.set(6, 200, 4, moving.0);
        game.world.chunks.insert((0, 0), Arc::new(chunk));
        let piston = |id, direction, progress| MovingPiston {
            moved_state: stone,
            direction,
            extending: true,
            source: false,
            progress,
            progress_o: progress,
            last_ticked: 0,
            death_ticks: 0,
            id,
        };
        let mut moved = Vec::new();
        for far_first in [true, false] {
            // The near stone moves east from x 4; the far one north from z 5,
            // half way: its box covers z 4.5 to 5.5, then z 4 to 5.
            let near = (BlockPos::new(5, 200, 5), piston(1, Direction::East, 0.0));
            let far = (BlockPos::new(6, 200, 4), piston(2, Direction::North, 0.5));
            game.block_ticks.moving_pistons = if far_first {
                vec![far, near]
            } else {
                vec![near, far]
            };
            game.sync_moving_pistons();
            game.tnts.clear();
            assert!(game.run_command("summon tnt 5.5 200 5.5 {fuse:80,Motion:[0.0d,0.0d,0.0d]}"));
            let before = game.tnts[0].position.x;
            // PistonMovingBlockEntity.tick of each, in order: half a block on.
            let pushes: Vec<_> = game
                .block_ticks
                .moving_pistons
                .iter()
                .map(|&(pos, old)| (pos, old, old.progress + 0.5))
                .collect();
            for (_, piston) in &mut game.block_ticks.moving_pistons {
                piston.progress += 0.5;
            }
            game.push_piston_entities(pushes);
            moved.push(game.tnts[0].position.x - before);
        }
        // The near stone's box reaches x 5.5 into a TNT from x 5.01: 0.49 plus
        // PUSH_OFFSET; the far stone's old box stops it at x 6.
        assert!(
            (moved[0] - 0.5).abs() < 1e-9,
            "far stone gone first: {}",
            moved[0]
        );
        assert!(moved[1] < 0.02, "far stone still in the way: {}", moved[1]);
    }

    /// A primed TNT pushed east by a piston into a stone wall loses its
    /// horizontal velocity, matching `Entity.move(MoverType.PISTON, …)`
    /// restitution even in a chunk that does not tick entities (#12022).
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_pushed_tnt_loses_velocity_into_a_wall() {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.game_dir = Some(std::env::temp_dir().join(format!(
            "horizon-piston-velocity-unit-{}",
            std::process::id()
        )));
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        game.set_allow_commands(true);
        let registry = &game.pack.assets.registry;
        let stone = registry.default_state("minecraft:stone").unwrap();
        let moving = registry.default_state("minecraft:moving_piston").unwrap();
        let mut chunk = Chunk::empty(0, 0);
        for x in 0..16 {
            for z in 0..16 {
                chunk.set(x, 199, z, stone.0);
            }
        }
        // Stone wall at x=6 blocks the TNT pushed east by the piston at x=5.
        chunk.set(6, 200, 5, stone.0);
        chunk.set(5, 200, 5, moving.0);
        game.world.chunks.insert((0, 0), Arc::new(chunk));
        let piston = MovingPiston {
            moved_state: stone,
            direction: Direction::East,
            extending: true,
            source: false,
            progress: 0.0,
            progress_o: 0.0,
            last_ticked: 0,
            death_ticks: 0,
            id: 1,
        };
        game.block_ticks.moving_pistons = vec![(BlockPos::new(5, 200, 5), piston)];
        game.sync_moving_pistons();
        game.tnts.clear();
        assert!(game.run_command("summon tnt 5.5 200 5.5 {fuse:80,Motion:[0.4d,0.0d,0.0d]}"));
        assert!(
            (game.tnts[0].velocity.x - 0.4).abs() < 1.0e-9,
            "TNT starts with vx 0.4, got {:?}",
            game.tnts[0].velocity
        );
        // Piston advances from progress 0 to 0.5 and pushes the TNT east.
        let pushes: Vec<_> = game
            .block_ticks
            .moving_pistons
            .iter()
            .map(|&(pos, old)| (pos, old, old.progress + 0.5))
            .collect();
        for (_, piston) in &mut game.block_ticks.moving_pistons {
            piston.progress += 0.5;
        }
        game.push_piston_entities(pushes);
        // The stone wall at x=6 stops the TNT; its horizontal velocity must be zeroed.
        assert!(
            game.tnts[0].velocity.x.abs() < 1.0e-9,
            "TNT velocity.x should be zeroed by the wall, got {:?}",
            game.tnts[0].velocity
        );
    }

    /// A primed TNT pushed down by a piston onto a slime block bounces up,
    /// matching vanilla's slime restitution (~+0.64) even in a lazy chunk (#12022).
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_pushed_tnt_bounces_on_slime() {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.game_dir = Some(
            std::env::temp_dir().join(format!("horizon-piston-slime-unit-{}", std::process::id())),
        );
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        game.set_allow_commands(true);
        let registry = &game.pack.assets.registry;
        let stone = registry.default_state("minecraft:stone").unwrap();
        let slime = registry.default_state("minecraft:slime_block").unwrap();
        let moving = registry.default_state("minecraft:moving_piston").unwrap();
        let mut chunk = Chunk::empty(0, 0);
        for x in 0..16 {
            for z in 0..16 {
                chunk.set(x, 199, z, stone.0);
            }
        }
        // Slime block at y=199 under the TNT pushed down by the piston at y=200.
        chunk.set(5, 199, 5, slime.0);
        chunk.set(5, 200, 5, moving.0);
        game.world.chunks.insert((0, 0), Arc::new(chunk));
        let piston = MovingPiston {
            moved_state: stone,
            direction: Direction::Down,
            extending: true,
            source: false,
            progress: 0.0,
            progress_o: 0.0,
            last_ticked: 0,
            death_ticks: 0,
            id: 1,
        };
        game.block_ticks.moving_pistons = vec![(BlockPos::new(5, 200, 5), piston)];
        game.sync_moving_pistons();
        game.tnts.clear();
        assert!(game.run_command("summon tnt 5.5 200.5 5.5 {fuse:80,Motion:[0.0d,-0.4d,0.0d]}"));
        assert!(
            (game.tnts[0].velocity.y - (-0.4)).abs() < 1.0e-9,
            "TNT starts with vy -0.4, got {:?}",
            game.tnts[0].velocity
        );
        // Piston advances from progress 0 to 0.5 and pushes the TNT down.
        let pushes: Vec<_> = game
            .block_ticks
            .moving_pistons
            .iter()
            .map(|&(pos, old)| (pos, old, old.progress + 0.5))
            .collect();
        for (_, piston) in &mut game.block_ticks.moving_pistons {
            piston.progress += 0.5;
        }
        game.push_piston_entities(pushes);
        // The slime block at y=199 bounces the TNT up; vertical velocity must be positive.
        assert!(
            game.tnts[0].velocity.y > 0.0,
            "TNT should bounce up on slime, got vy {:?}",
            game.tnts[0].velocity
        );
    }
}

#[cfg(test)]
mod h255_mob_piston_ground_tests {
    use super::*;
    use crate::game::{Audio, Options, Progress, Settings, load_resources};
    use horizon_client::{chunk::Chunk, motion::PlayerMotion};
    use horizon_core::{geometry::Vec3, position::ChunkPos, random::LegacyRandom};
    use std::sync::Arc;
    fn with_game(test: impl FnOnce(&mut Game<'_>)) {
        with_role_game(
            crate::game::execution_role::ExecutionRole::IntegratedServer,
            test,
        );
    }
    fn with_role_game(
        role: crate::game::execution_role::ExecutionRole,
        test: impl FnOnce(&mut Game<'_>),
    ) {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.game_dir = Some(
            std::path::PathBuf::from(std::env::var_os("HORIZON_HOME").unwrap())
                .join(format!("h255-{:?}", std::thread::current().id())),
        );
        options.execution_role = role;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        game.set_allow_commands(true);

        let mut center = Chunk::empty(0, 0);
        center.lit = true;
        game.world.chunks.insert((0, 0), Arc::new(center));
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(0, 0))
            .unwrap();
        assert!(game.run_command("difficulty normal"));
        assert!(game.run_command("forceload add 0 0"));
        for x in -1..=1 {
            for z in -1..=1 {
                if x == 0 && z == 0 {
                    continue;
                }
                let mut chunk = Chunk::empty(x, z);
                chunk.lit = true;
                game.world.chunks.insert((x, z), Arc::new(chunk));
                game.block_ticks
                    .add_empty_chunk(ChunkPos::new(x, z))
                    .unwrap();
            }
        }
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(2.5, 200.0, 2.5)).unwrap());
        game.creative = true;
        assert!(game.run_command("fill 0 199 0 31 199 31 stone"));
        game.world.random = LegacyRandom::new(255);
        game.update_chunk_tickets();
        assert!(crate::game::ticking_chunk(&game.world, 0, 0));
        assert!(game.tickets.levels().block_ticking(0, 0));
        test(&mut game);
    }
    fn mob_index(game: &Game<'_>, n: i32) -> usize {
        let id = game
            .persist
            .uuids
            .iter()
            .find_map(|(id, u)| (*u == [255, 1, 0, n]).then_some(*id))
            .unwrap();
        game.mobs.iter().position(|m| m.id == id).unwrap()
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn h255_real_honey_piston_lifecycle_clears_mob_ground_before_second_drag() {
        with_game(|game| {
            game.world.game_time = 0;
            for cmd in [
                "gamerule spawn_mobs false",
                "gamerule random_tick_speed 0",
                "gamerule advance_time false",
                "setblock 3 201 4 piston[facing=east]",
                "setblock 4 201 4 honey_block",
                "setblock 4 201 12 honey_block",
                "setblock 12 200 12 piston[facing=up]",
                "setblock 12 201 12 stone",
                "summon husk 4.5 202 4.5 {UUID:[I;255,1,0,1],Health:10f,NoAI:1b,NoGravity:1b,OnGround:1b,Motion:[0d,0d,0d]}",
                "summon husk 4.5 202 12.5 {UUID:[I;255,1,0,2],Health:10f,NoAI:1b,NoGravity:1b,OnGround:1b,Motion:[0d,0d,0d]}",
                "summon husk 12.5 202 12.5 {UUID:[I;255,1,0,3],Health:10f,NoAI:1b,NoGravity:1b,OnGround:0b,Motion:[0d,0d,0d]}",
            ] {
                assert!(game.run_command(cmd), "{cmd}");
            }
            let main = mob_index(game, 1);
            let idle = mob_index(game, 2);
            let vertical = mob_index(game, 3);
            let id = game.mobs[main].id;
            let age = game.mobs[main].age;
            assert!(game.mobs[main].on_ground);
            assert!(game.mobs[idle].on_ground);
            assert!(!game.mobs[vertical].on_ground);
            assert_eq!(game.mobs[main].position, Vec3::new(4.5, 202.0, 4.5));
            assert_eq!(
                game.res
                    .facts()
                    .get(game.world.block(4, 201, 4))
                    .name
                    .as_ref(),
                "minecraft:honey_block"
            );
            assert!(game.run_command("setblock 3 201 5 redstone_block"));
            assert!(game.run_command("setblock 13 200 12 redstone_block"));
            let mut failures = Vec::new();
            for tick in 1..=2 {
                game.world.game_time += 1;
                game.update_chunk_tickets();
                assert!(game.entity_ticking_area()(4, 4));
                game.tick_blocks().unwrap();
                game.tick_creature_entities().unwrap();
                // Real block-entity ticker generates and drains the piston pushes.
                game.tick_moving_pistons().unwrap();
                let p = game
                    .block_ticks
                    .moving_pistons
                    .iter()
                    .find(|(p, _)| *p == BlockPos::new(5, 201, 4))
                    .expect("real moving honey block");
                assert_eq!(p.1.progress, 0.5 * tick as f32);
                assert_eq!(
                    p.1.moved_state,
                    game.pack
                        .assets
                        .registry
                        .default_state("minecraft:honey_block")
                        .unwrap()
                );
                assert_eq!(game.mobs[main].id, id);
                assert_eq!(game.uuid_for(id), [255, 1, 0, 1]);
                assert_eq!(game.mobs[main].age, age + tick);
                assert!(game.mobs[main].no_ai);
                assert_eq!(game.mobs[main].health, 10.0);
                assert_eq!(game.mobs[main].velocity, Vec3::ZERO);
                if game.mobs[main].on_ground {
                    failures.push(format!("tick{tick}: actual Mob still grounded"));
                }
                if game.mobs[main].position != Vec3::new(5.0, 202.0, 4.5) {
                    failures.push(format!(
                        "tick{tick}: position {:?}",
                        game.mobs[main].position
                    ));
                }
                if game.mob_tree_nbt(main).get("OnGround")
                    != Some(&horizon_world::nbt::Tag::Byte(0))
                {
                    failures.push(format!("tick{tick}: saved ground is stale"));
                }
                assert_eq!(game.mobs[idle].position, Vec3::new(4.5, 202.0, 12.5));
                assert!(game.mobs[idle].on_ground);
                assert_eq!(
                    game.mobs[vertical].position,
                    Vec3::new(12.5, if tick == 1 { 202.51 } else { 203.01 }, 12.5)
                );
                assert!(!game.mobs[vertical].on_ground);
            }
            assert!(
                failures.is_empty(),
                "actual lifecycle failures: {failures:#?}"
            );
        });
    }
    // Added source/unit controls; these do not extend the Java live contract.
    fn added_mob(game: &mut Game<'_>, n: i32, kind: &str, no_ai: bool, x: f64) -> usize {
        let tame = if kind == "horse" { ",Tame:1b" } else { "" };
        assert!(game.run_command(&format!("summon {kind} {x} 202 4.5 {{UUID:[I;255,1,0,{n}],Health:10f,NoAI:{}b,NoGravity:1b,OnGround:1b,Motion:[0d,0d,0d]{tame}}}",i32::from(no_ai))));
        let i = mob_index(game, n);
        assert_eq!(game.mobs[i].no_ai, no_ai);
        assert!(game.mobs[i].on_ground);
        assert_eq!(game.uuid_for(game.mobs[i].id), [255, 1, 0, n]);
        i
    }
    fn added_push(game: &mut Game<'_>, i: usize, movement: Vec3) {
        let m = &game.mobs[i];
        let (bounds, at, step, ground) = (
            m.bounds(),
            [m.position.x, m.position.z],
            m.kind.step_height as f32,
            m.on_ground,
        );
        // Mob owns its actual piston budget; the map is for other entity kinds.
        game.move_pushed(
            Target::Entity(EntityHandle::Mob(i)),
            bounds,
            at,
            step,
            ground,
            movement,
            Direction::East,
            &mut std::collections::HashMap::new(),
        );
    }
    /// `AbstractArrow.move` after a push the collisions cut to nothing: a stuck
    /// arrow with nothing within 0.06 of it starts falling all the same.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_stuck_arrow_whose_push_a_wall_stops_falls_when_nothing_holds_it() {
        with_game(|game| {
            assert!(game.run_command("setblock 6 202 4 minecraft:stone"));
            game.creatures.projectiles.clear();
            assert!(game.run_command("summon minecraft:arrow 5.75 202.5 4.5 {NoGravity:0b}"));
            let i = game
                .creatures
                .projectiles
                .iter()
                .position(|p| p.arrow.is_some())
                .expect("the arrow");
            {
                let p = &mut game.creatures.projectiles[i];
                p.velocity = Vec3::ZERO;
                p.stuck = true;
                p.arrow.as_mut().unwrap().in_ground = true;
            }
            let p = &game.creatures.projectiles[i];
            let (bounds, at) = (p.bounds(), [p.position.x, p.position.z]);
            let before = p.position;
            game.move_pushed(
                Target::Entity(EntityHandle::Projectile(i)),
                bounds,
                at,
                0.0,
                false,
                Vec3::new(0.3, 0.0, 0.0),
                Direction::East,
                &mut std::collections::HashMap::new(),
            );
            let p = &game.creatures.projectiles[i];
            assert_eq!(p.position, before, "the stone stops the push");
            assert!(
                !p.arrow.as_ref().unwrap().in_ground,
                "nothing holds it, so it falls"
            );
        });
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn h255_added_no_request_and_exhausted_budget_keep_state() {
        with_game(|game| {
            let i = added_mob(game, 20, "horse", true, 8.5);
            added_push(game, i, Vec3::ZERO);
            assert!(game.mobs[i].on_ground);
            assert_eq!(game.mobs[i].position, Vec3::new(8.5, 202.0, 4.5));
            assert!(game.run_command("ride @s mount 000000ff-0000-0001-0000-000000000014"));
            assert_eq!(game.riding, Some(game.mobs[i].id));
            added_push(game, i, Vec3::new(0.51, 0.0, 0.0));
            assert!(game.mobs[i].on_ground, "occupied path remains excluded");
            assert_eq!(game.mobs[i].position, Vec3::new(9.01, 202.0, 4.5));
            assert!(game.run_command("ride @s dismount"));
            assert!(game.riding.is_none());
            assert!(game.mobs[i].rider.is_none());
            assert!(game.mobs[i].extra.rider.is_none());
            assert!(game.mobs[i].extra.mount.is_none());
            assert!(!game.mobs[i].extra.passenger);
            let before = game.mobs[i].position;
            added_push(game, i, Vec3::new(0.25, 0.0, 0.0));
            assert_eq!(
                game.mobs[i].position, before,
                "real same-tick piston budget is exhausted"
            );
            assert!(
                game.mobs[i].on_ground,
                "limitedZERO precedes the ground update"
            );
        });
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn h255_added_blocked_request_updates_ground_before_tiny_return() {
        with_game(|game| {
            assert!(game.run_command("fill 9 202 4 9 204 4 stone"));
            // Match the existing Mob::bounds_at f32 width boundary exactly.
            let width = game.res.mob_types().get("minecraft:husk").unwrap().width as f32;
            let x = 9.0 - f64::from(width) * 0.5;
            let i = added_mob(game, 21, "husk", true, x);
            assert_eq!(game.mobs[i].bounds().max_position().x, 9.0);
            let before = game.mobs[i].position;
            added_push(game, i, Vec3::new(0.25, 0.0, 0.0));
            assert_eq!(
                game.mobs[i].position, before,
                "actual loaded wall blocks the nonzero request"
            );
            assert!(
                !game.mobs[i].on_ground,
                "Entity.move ground ordering, source/unit-only coverage"
            );
        });
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn h255_added_client_role_ai_and_vertical_paths_keep_native_state() {
        use crate::game::execution_role::ExecutionRole as Role;
        for (role, no_ai, movement) in [
            (Role::Client, true, Vec3::new(0.25, 0.0, 0.0)),
            (Role::IntegratedServer, false, Vec3::new(0.25, 0.0, 0.0)),
            (Role::IntegratedServer, true, Vec3::new(0.0, 0.25, 0.0)),
        ] {
            with_role_game(role, |game| {
                let i = added_mob(game, 22, "husk", no_ai, 8.5);
                let before = game.mobs[i].position;
                added_push(game, i, movement);
                assert_eq!(game.mobs[i].position, before.add_vector(movement));
                assert!(
                    game.mobs[i].on_ground,
                    "excluded native path remains unchanged; not Java parity"
                );
            });
        }
    }
}
