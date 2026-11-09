//! Falling block entities (`FallingBlockEntity.tick`): one block's tick (`tick_one_falling`), its move
//! (`fall_step`) and the item entity it leaves as a drop (`drop_falling_block`, `Entity.spawnAtLocation`).
use super::*;

impl Game<'_> {
    /// FallingBlockEntity.tick: fall with 0.04 gravity and 0.98F drag, then land.
    /// One falling block's `tick`: places or drops its block in its own tick,
    /// so a block falling onto one that just landed lands on top of it. `i` is
    /// its place in the list; the caller has checked it is ticking.
    /// `entities` are the boxes that stop its move (`Game::item_entity_collisions`).
    ///
    /// The block keeps its place in the list while it moves (nothing its move
    /// reads looks at the falling blocks): taking it out and putting it back
    /// shifted every block after it twice, for every falling block, every tick.
    /// One that lands, drops or goes leaves the list before its block is placed
    /// or dropped, as before; on an error the list keeps it as it was.
    pub(super) fn tick_one_falling(&mut self, i: usize, entities: &[Aabb]) -> Result<()> {
        let entity_drops = self.ui.rules.bool("entity_drops");
        let join = self.falling[i].join;
        let mut f = self.falling[i].clone();
        let step = self.fall_step(&mut f, entity_drops, entities)?;
        let slot = if self.falling.get(i).is_some_and(|e| e.join == join) {
            Some(i)
        } else {
            self.falling.iter().position(|e| e.join == join)
        };
        if matches!(step, FallStep::Falling) {
            if let Some(j) = slot {
                self.falling[j] = f;
            }
            return Ok(());
        }
        if let Some(j) = slot {
            self.falling.remove(j);
        }
        match step {
            FallStep::Falling | FallStep::Gone => {}
            FallStep::Dropped(at) => self.drop_falling_block(f.state, at),
            FallStep::Landed(pos) => self.finish_falling_block(f, pos),
        }
        Ok(())
    }
    /// One `FallingBlockEntity.tick` move of `f`.
    pub(super) fn fall_step(
        &mut self,
        f: &mut FallingBlock,
        entity_drops: bool,
        entities: &[Aabb],
    ) -> Result<FallStep> {
        if self.res.facts().is_air(f.state) {
            return Ok(FallStep::Gone);
        }
        f.previous = f.position;
        f.age = f.age.wrapping_add(1);
        f.velocity.y -= horizon_client::entity_rules::EntityRules::get_gravity(f);
        let mut wanted = f.velocity;
        // Entity.move: a cobweb it was in scales this move, which then
        // starts from rest.
        if f.stuck.length_squared() > 1.0e-7 {
            wanted = wanted.multiply(f.stuck);
            f.stuck = Vec3::ZERO;
            f.velocity = Vec3::ZERO;
        }
        let scene = collisions::SceneCollisions {
            world: &self.world,
            binding: self.collision,
            subject: Some(collisions::PowderSnowSubject {
                fall_distance: f.fall_distance,
                falling_block: true,
                ..Default::default()
            }),
        };
        // Entity.isDescending: only sneaking entities descend through
        // scaffolding. Falling blocks do not sneak, so descending is always
        // false (Entity.isDescending returns isShiftKeyDown, fixes #7553).
        let context = CollisionContext::for_entity(
            CollisionEntity {
                identity: EntityIdentity(0),
                kind: horizon_world::collision_context::EntityContextKind::Other,
                y: f.position.y,
                descending: false,
                main_hand: None,
            },
            false,
        );
        // On an error the caller keeps the block as it was before this tick.
        // Entity.collide: the world border's walls and the entities that
        // `canBeCollidedWith` it (a boat, a shulker) stop it too.
        let movement = collisions::EntityCollisions {
            blocks: scene,
            entities,
            border: Some((self.border.geometry(), [f.position.x, f.position.z])),
        }
        .resolve_with_context(
            horizon_client::falling_block::bounds(f.position),
            wanted,
            0.0,
            false,
            context,
        )?;
        let next = Vec3::new(
            f.position.x + movement.x,
            f.position.y + movement.y,
            f.position.z + movement.z,
        );
        let on_ground = wanted.y < 0.0 && movement.y != wanted.y;
        f.on_ground = on_ground;
        if next.y < f.position.y {
            f.fall_distance += f.position.y - next.y;
        }
        let from = f.position;
        f.position = next;
        // Entity.move stops it on the axes it collided on.
        if movement.x != wanted.x {
            f.velocity.x = 0.0;
        }
        if movement.y != wanted.y {
            f.velocity.y = 0.0;
        }
        if movement.z != wanted.z {
            f.velocity.z = 0.0;
        }
        if on_ground {
            self.falling_block_impact(f);
            f.fall_distance = 0.0;
        }
        // applyEffectsFromBlocks: WebBlock.entityInside makes it stuck
        // (Entity.makeStuckInBlock), BubbleColumnBlock.entityInside lifts or
        // drags it. Entity.move records the move unless it was blocked short
        // of a tiny one.
        let moved = movement.length_squared();
        let recorded = moved > 1.0e-7 || wanted.length_squared() - moved < 1.0e-7;
        let world = &self.world;
        let facts = self.res.facts();
        let (registry, collision) = (self.pack.assets.registry, self.collision);
        let mut tops = Vec::new();
        let stuck = horizon_client::falling_block::apply_effects_from_blocks(
            from,
            f.position,
            recorded.then_some(wanted),
            &mut f.velocity,
            &mut f.fall_distance,
            &mut tops,
            |p| {
                use horizon_client::falling_block::Inside;
                if &*facts.get(world.block(p.x, p.y, p.z)).name == "minecraft:cobweb" {
                    Inside::Cobweb
                } else if let Some(column) =
                    horizon_client::motion::bubble_column_at(world, registry, facts, collision, p)
                {
                    Inside::BubbleColumn(column)
                } else {
                    Inside::Nothing
                }
            },
        );
        if let Some(multiplier) = stuck {
            f.stuck = multiplier;
        }
        for pos in tops {
            self.bubble_column_particles(pos);
        }
        self.entity_powder_snow_particles(
            from,
            f.position,
            horizon_client::falling_block::bounds(f.position),
            f.velocity,
            false,
        );
        // FallingBlockEntity.tick handles its portal after the move's block
        // effects, before placing/dropping on the floor under an End portal.
        let teleported = self.falling_through_portal(f);
        // FallingBlockEntity.forceTickAfterTeleportToDuplicate: crossing
        // either way through the End deliberately keeps the source's final
        // placement/drop branch, even though the source entity was removed.
        if let Some(target) = teleported
            && self.dim != Dim::End
            && target != Dim::End
        {
            return Ok(FallStep::Gone);
        }
        let y = next.y.floor() as i32;
        let outside = y <= self.world.bottom_y() || y >= self.world.top_y();
        if !on_ground && (f.age > 100 && outside || f.age > 600) {
            return Ok(if f.drop_item && entity_drops {
                FallStep::Dropped(next)
            } else {
                FallStep::Gone
            });
        }
        if on_ground {
            return Ok(FallStep::Landed([
                f.position.x.floor() as i32,
                f.position.y.floor() as i32,
                f.position.z.floor() as i32,
            ]));
        }
        // Entity.scale(getAirDrag()): 0.98F, widened before it scales motion.
        f.velocity = horizon_client::entity_rules::scale_air_drag(f.velocity);
        Ok(if teleported.is_some() {
            FallStep::Gone
        } else {
            FallStep::Falling
        })
    }
    /// `Entity.spawnAtLocation`: an item entity where the block was.
    pub(super) fn drop_falling_block(&mut self, state: BlockStateId, at: Vec3) {
        // Entity.spawnAtLocation: an item entity where the block was.
        let item = self.res.facts().get(state).item.clone();
        if &*item != "minecraft:air" {
            // new ItemEntity(level, x, y, z, stack): the motion is from the
            // item's own random, not the level's.
            let stack = ItemStack {
                item,
                count: 1,
                damage: 0,
                extra: None,
            };
            self.spawn_item_own(stack, [at.x, at.y, at.z], 10);
        }
    }
}
