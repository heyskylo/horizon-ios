//! Falling block entities (`FallingBlockEntity.tick`): one block's tick (`tick_one_falling`), its move
//! (`fall_step`) and the item entity it leaves as a drop (`drop_falling_block`, `Entity.spawnAtLocation`).
use super::*;

impl Game<'_> {
    /// Cache only model-dependent work, never an entity's seed, light or pose.
    pub(super) fn falling_model_vertices(
        &self,
        state: BlockStateId,
        start_pos: BlockPos,
        origin: glam::Vec3,
        light: u32,
        out: &mut Vec<Vertex>,
    ) {
        // `FallingBlockRenderer`: the model is picked by the block's starting position.
        let seed = horizon_core::random::position_seed(start_pos);
        let Ok(models) = self.pack.render.model_catalog.collect_seeded(state, seed) else {
            return;
        };
        // Baked models live at stable addresses until the pack is replaced.
        // Keep multipart order and duplicates: both affect the emitted vertices.
        let mut cache = self.falling_vertices.borrow_mut();
        let entries = cache.entry(state).or_default();
        let index = entries.iter().position(|(ptrs, _)| {
            ptrs.iter()
                .copied()
                .eq(models.iter().map(|(_, m)| *m as *const _ as usize))
        });
        let index = index.unwrap_or_else(|| {
            let mut vertices = Vec::new();
            self.block_model_vertices(state, seed, glam::Mat4::IDENTITY, 0, false, &mut vertices);
            // Preserve the original coordinates, including signed zero, rather
            // than composing an identity transform with the frame's translation.
            let positions = models.iter().flat_map(|(_, model)| {
                model
                    .faces
                    .iter()
                    .filter(|face| {
                        self.pack
                            .layer_names
                            .get(&face.layer)
                            .is_some_and(|name| self.gui.atlas.sprites.contains_key(name))
                    })
                    .flat_map(|face| face.quad.positions)
            });
            for (vertex, pos) in vertices.iter_mut().zip(positions) {
                vertex.pos = pos;
            }
            let ptrs = models
                .iter()
                .map(|(_, m)| *m as *const _ as usize)
                .collect();
            entries.push((ptrs, vertices));
            entries.len() - 1
        });
        let start = out.len();
        out.extend_from_slice(&entries[index].1);
        let matrix = glam::Mat4::from_translation(origin);
        for vertex in &mut out[start..] {
            vertex.pos = matrix
                .transform_point3(glam::Vec3::from_array(vertex.pos))
                .to_array();
            vertex.light = light;
        }
    }
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
    /// The end of a piston's `Entity.move(MoverType.PISTON, …)` for falling
    /// block `i` after a step of `moved`: `checkFallDamage` adds the drop and,
    /// on the ground, runs fallOn and posts HIT_GROUND after a fall, then
    /// forgets it, so its own next tick does not land again.
    pub(super) fn finish_falling_piston_move(&mut self, i: usize, moved: Vec3) {
        let mut f = self.falling[i].clone();
        if moved.y < 0.0 {
            f.fall_distance -= f64::from(moved.y as f32);
        }
        if f.on_ground {
            self.falling_block_impact(&mut f);
            if f.fall_distance > 0.0 {
                let bounds = horizon_client::falling_block::bounds(f.position);
                self.post_landing_event(f.position, bounds, false);
            }
            f.fall_distance = 0.0;
        }
        self.falling[i] = f;
    }
    pub(super) fn tick_one_falling(&mut self, i: usize, entities: &[Aabb]) -> Result<()> {
        let entity_drops = self.ui.rules.bool("entity_drops");
        let join = self.falling[i].join;
        let mut f = self.falling[i].clone();
        let mut step = self.fall_step(&mut f, entity_drops, entities)?;
        // FallingBlockEntity.tick: a block that comes down where a piston is
        // moving a block neither lands nor breaks (whatever its cancelDrop).
        // It slows like any landing and falls on, so the piston pushes it.
        if let FallStep::Landed([x, y, z]) = step
            && &*self.res.facts().get(self.world.block(x, y, z)).name == "minecraft:moving_piston"
        {
            f.velocity = horizon_client::entity_rules::scale_air_drag(
                f.velocity.multiply(Vec3::new(0.7, -0.5, 0.7)),
            );
            step = FallStep::Falling;
        }
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
        // Entity.checkFallDamage: each step down, narrowed to a float (a
        // falling block never runs baseTick, so it is never in water).
        if movement.y < 0.0 {
            f.fall_distance -= f64::from(movement.y as f32);
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
            // fallOn (causeFallDamage), then HIT_GROUND after a fall.
            self.falling_block_impact(f);
            if f.fall_distance > 0.0 {
                let bounds = horizon_client::falling_block::bounds(f.position);
                self.post_landing_event(f.position, bounds, false);
            }
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

#[cfg(test)]
mod geometry_tests {
    use super::*;

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn falling_model_cache_matches_direct_vertices() {
        let root = PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            Instant::now(),
            false,
            false,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = execution_role::ExecutionRole::IntegratedServer;
        let game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        for frame in 0..2 {
            for (name, pos, origin, light) in [
                (
                    "minecraft:sand",
                    BlockPos::new(1, 80, 3),
                    glam::Vec3::new(-3.25, 12.5, 0.0),
                    0x7010,
                ),
                (
                    "minecraft:sand",
                    BlockPos::new(9, 90, -4),
                    glam::Vec3::new(8.125, -0.0, -9.5),
                    0xf020,
                ),
                (
                    "minecraft:gravel",
                    BlockPos::new(-2, 70, 5),
                    glam::Vec3::new(-0.0, -17.75, 2.5),
                    0x5080,
                ),
            ] {
                let state = game.pack.assets.registry.default_state(name).unwrap();
                let seed = horizon_core::random::position_seed(pos);
                let mut expected = Vec::new();
                game.block_model_vertices(
                    state,
                    seed,
                    glam::Mat4::from_translation(origin),
                    light,
                    false,
                    &mut expected,
                );
                let mut actual = Vec::new();
                game.falling_model_vertices(state, pos, origin, light, &mut actual);
                let (_, tint) = game.block_face_sprite(state).unwrap();
                let tint = horizon_client::mobs::render::abgr(tint);
                for vertices in [&mut expected, &mut actual] {
                    for v in vertices {
                        v.color = horizon_client::hud_layout::multiply(v.color, tint);
                    }
                }
                assert!(!expected.is_empty());
                assert_eq!(actual.len(), expected.len());
                for (a, b) in actual.iter().zip(&expected) {
                    assert_eq!(
                        a.pos.map(f32::to_bits),
                        b.pos.map(f32::to_bits),
                        "frame {frame}"
                    );
                    assert_eq!(a.uv.map(f32::to_bits), b.uv.map(f32::to_bits));
                    assert_eq!(a.glint_uv.map(f32::to_bits), b.glint_uv.map(f32::to_bits));
                    assert_eq!(
                        (a.color, a.light, a.normal, a.layer),
                        (b.color, b.light, b.normal, b.layer)
                    );
                }
            }
        }
    }
}
