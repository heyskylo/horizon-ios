//! Primed TNT (`PrimedTnt`): 80-tick fuse, gravity and drag, explosion of
//! power 4, `TntRenderer` swell and white flash, and `TntBlock.useItemOn`
//! (flint and steel / fire charge).
use super::{Game, entity::Entity};
use horizon_client::entity_rules::EntityRules;
use horizon_client::lan::playerdata::uuid_ints;
use horizon_client::{
    Result, collisions::SceneCollisions, items::ItemStack, mesher::Vertex, mobs::Who, text::Text,
};
use horizon_core::{
    geometry::{Aabb, Vec3},
    position::BlockPos,
    random::RandomSource,
};
use horizon_world::level::flags as level_flags;

#[derive(Clone, Debug)]
pub(super) struct PrimedTnt {
    /// `Entity.yRot` and `xRot` (`/tp`, `/data get ... Rotation`).
    pub rotation: [f32; 2],
    /// When it joined its list (`horizon_client::join_seq`); set by the list.
    pub(super) join: horizon_client::join_seq::JoinSeq,
    /// The entity id a server tracks it by (0 until it is tracked).
    pub id: u32,
    pub position: Vec3,
    pub previous: Vec3,
    pub velocity: Vec3,
    pub fuse: i32,
    pub on_ground: bool,
    pub owner: Option<Who>,
    pub guest: Option<u32>,
    /// `Entity.noGravity` (`NoGravity` in its save).
    pub no_gravity: bool,
    /// `PrimedTnt.explosionPower`: 4 by default, saved as `explosion_power`
    /// and clamped to 0..128 when read back.
    pub power: f32,
    /// PrimedTnt.DATA_BLOCK_STATE_ID; None is the default TNT state.
    pub block_state: Option<horizon_world::registry::BlockStateId>,
}
impl PrimedTnt {
    /// The live state encoded for each existing save view.
    pub(super) fn block_state_nbt(
        &self,
        registry: &horizon_world::registry::BlockRegistry,
    ) -> horizon_world::nbt::Tag {
        self.block_state.map_or_else(
            || horizon_world::nbt::Tag::String("minecraft:tnt".into()),
            |state| horizon_client::entity_nbt::state_to_nbt(registry, state),
        )
    }

    pub(super) const SIZE: f64 = 0.98;
    pub(super) fn bounds(&self) -> Aabb {
        let h = Self::SIZE / 2.0;
        Aabb::new(
            Vec3::new(self.position.x - h, self.position.y, self.position.z - h),
            Vec3::new(
                self.position.x + h,
                self.position.y + Self::SIZE,
                self.position.z + h,
            ),
        )
    }
    pub(super) fn tick(
        &mut self,
        world: &horizon_client::world::World,
        collision: &horizon_world::block_collision::BlockCollisionRegistry<'_>,
        assets: &horizon_client::assets::Assets,
        facts: &horizon_client::blocks::BlockFacts,
        fluids: &horizon_client::motion::FluidResources,
        border: Option<&horizon_core::geometry::border::BorderGeometry>,
        blocks: Option<&horizon_world::movement_properties::BoundMovementProperties>,
    ) -> Result<Vec<BlockPos>> {
        self.tick_among(world, collision, assets, facts, fluids, border, blocks, &[])
    }
    /// [`Self::tick`], its move also stopped by `entities`: the boxes of the
    /// entities that `canBeCollidedWith` it (`EntityGetter.getEntityCollisions`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn tick_among(
        &mut self,
        world: &horizon_client::world::World,
        collision: &horizon_world::block_collision::BlockCollisionRegistry<'_>,
        assets: &horizon_client::assets::Assets,
        facts: &horizon_client::blocks::BlockFacts,
        fluids: &horizon_client::motion::FluidResources,
        border: Option<&horizon_core::geometry::border::BorderGeometry>,
        blocks: Option<&horizon_world::movement_properties::BoundMovementProperties>,
        entities: &[Aabb],
    ) -> Result<Vec<BlockPos>> {
        self.previous = self.position;
        self.velocity.y -= self.get_gravity();
        let scene = SceneCollisions {
            world,
            binding: collision,
            subject: None,
        };
        let wanted = self.velocity;
        // Entity.collide: the world border's walls and the entities that
        // `canBeCollidedWith` it (a boat, a shulker) stop it too.
        let border = border.map(|b| (b, [self.position.x, self.position.z]));
        let moved = scene
            .resolve_in_border_with(self.bounds(), wanted, 0.0, self.on_ground, border, entities)
            .unwrap_or(Vec3::ZERO);
        if horizon_client::entities::move_applies(wanted, moved) {
            self.position = Vec3::new(
                self.position.x + moved.x,
                self.position.y + moved.y,
                self.position.z + moved.z,
            );
        }
        self.on_ground = wanted.y < 0.0 && moved.y != wanted.y;
        if moved.x != wanted.x {
            self.velocity.x = 0.0;
        }
        if moved.z != wanted.z {
            self.velocity.z = 0.0;
        }
        if moved.y != wanted.y {
            // Entity.restituteMovementAfterCollisions: slime bounces it.
            let below = world.block(
                self.position.x.floor() as i32,
                (self.position.y - f64::from(1.0e-5_f32)).floor() as i32,
                self.position.z.floor() as i32,
            );
            self.velocity.y = horizon_client::entities::vertical_collision_velocity(
                wanted.y,
                moved.y,
                0.04,
                wanted.y < 0.0,
                &*facts.get(below).name == "minecraft:slime_block",
            );
        }
        // Entity.move's last step: soul sand and honey slow the TNT's
        // horizontal speed. `blocks` is None only for the isolated unit tests.
        if let Some(blocks) = blocks {
            let factor =
                horizon_client::motion::block_speed_factor_at(world, blocks, self.position, None);
            self.velocity = horizon_world::entity_move::scale_by_block_speed(self.velocity, factor);
        }
        // Entity.applyEffectsFromBlocks: a bubble column lifts or drags it, and the
        // columns it is at the top of send their surface particles.
        let mut tops = Vec::new();
        self.push_by_bubble_column_over(world, assets.registry, facts, collision, |pos| {
            tops.push(pos)
        });
        if self.on_ground {
            // Entity.applyEffectsFromBlocks -> SlimeBlock.stepOn.
            let under = world.block(
                self.position.x.floor() as i32,
                (self.position.y - f64::from(0.2_f32)).floor() as i32,
                self.position.z.floor() as i32,
            );
            if &*facts.get(under).name == "minecraft:slime_block"
                && let Some(scale) = horizon_client::entities::slime_step_on_scale(self.velocity.y)
            {
                self.velocity.x *= scale;
                self.velocity.z *= scale;
            }
        }
        self.velocity = Vec3::new(
            self.velocity.x * 0.98,
            self.velocity.y * 0.98,
            self.velocity.z * 0.98,
        );
        if self.on_ground {
            self.velocity = Vec3::new(
                self.velocity.x * 0.7,
                self.velocity.y * -0.5,
                self.velocity.z * 0.7,
            );
        }
        self.fuse -= 1;
        if self.fuse > 0 {
            // PrimedTnt updates immersion after moving and decrementing its fuse.
            self.push_by_fluid(world, fluids);
        }
        Ok(tops)
    }
}

impl Game<'_> {
    /// Asks the mods whether the TNT at `pos` may be primed
    /// (`native_tnt_primed`); the acting guest, if any, is its player.
    pub(super) fn tnt_prime_allowed(&mut self, pos: BlockPos) -> bool {
        let mut cancel = false;
        self.native_tnt_primed(self.lan_acting_uuid, [pos.x, pos.y, pos.z], &mut cancel);
        // `TntBlock.prime` leaves the block alone with `tnt_explodes` off, for
        // redstone, fire and an unstable block breaking alike.
        !cancel && self.ui.rules.bool("tnt_explodes")
    }
    /// Primes TNT a level event announced, or, when the mods refuse, puts
    /// back the block the fire or signal had already taken out.
    pub(super) fn prime_tnt_event(&mut self, pos: BlockPos, rot: f64) {
        if self.tnt_prime_allowed(pos) {
            self.add_primed_tnt(pos, rot, 80, None, None);
            return;
        }
        // Refused: the level queued the fuse sound and the game event right
        // after the prime; they are dropped as they come up.
        self.tnt_refused = Some(pos);
        let Some(tnt) = self.pack.assets.registry.default_state("minecraft:tnt") else {
            return;
        };
        if self.world.block(pos.x, pos.y, pos.z) == self.world.air {
            let flags = level_flags::UPDATE_CLIENTS | level_flags::UPDATE_SKIP_ON_PLACE;
            self.with_level(|b, level, ticks| b.set_block(level, ticks, pos, tnt, flags));
        }
    }
    /// `new PrimedTnt(level, x + 0.5, y, z + 0.5, owner)`; `short` applies
    /// `wasExploded`'s random short fuse. Plays `entity.tnt.primed` for primes.
    pub(super) fn spawn_primed_tnt(
        &mut self,
        pos: BlockPos,
        short: bool,
        owner: Option<Who>,
        guest: Option<u32>,
    ) {
        // PrimedTnt: the fuse is lit.
        let who = self.sculk_actor();
        self.post_block_game_event("minecraft:prime_fuse", [pos.x, pos.y, pos.z], who, None);
        let r = &mut self.world.random;
        let rot = r.next_double() * f64::from(std::f32::consts::TAU);
        let fuse = if short {
            // PrimedTnt.getRandomShortFuse
            r.next_int_bound(20).unwrap_or(0) + 10
        } else {
            self.audio.play(
                "minecraft:entity.tnt.primed",
                Some([
                    f64::from(pos.x) + 0.5,
                    f64::from(pos.y),
                    f64::from(pos.z) + 0.5,
                ]),
                1.0,
                1.0,
            );
            80
        };
        self.add_primed_tnt(pos, rot, fuse, owner, guest);
    }
    /// Adds primed TNT at the block's bottom centre, pushed along `rot`.
    pub(super) fn add_primed_tnt(
        &mut self,
        pos: BlockPos,
        rot: f64,
        fuse: i32,
        owner: Option<Who>,
        guest: Option<u32>,
    ) {
        let position = Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y),
            f64::from(pos.z) + 0.5,
        );
        // An id from the entity counter at once, so a command can name it.
        self.next_entity += 1;
        self.tnts.push(PrimedTnt {
            rotation: [0.0, 0.0],
            join: Default::default(),
            no_gravity: false,
            id: self.next_entity,
            position,
            previous: position,
            velocity: Vec3::new(-rot.sin() * 0.02, f64::from(0.2f32), -rot.cos() * 0.02),
            fuse,
            on_ground: false,
            owner,
            guest,
            // PrimedTnt's constructor: 4 when nothing sets it.
            power: 4.0,
            block_state: None,
        });
    }
    /// `PrimedTnt.tick` for every primed TNT, in order; an expired fuse
    /// explodes immediately (later TNT in the list feel its knockback this
    /// tick). TNT primed by an explosion starts ticking next tick.
    /// One primed TNT's `tick`: it moves, and explodes when its fuse is out.
    /// `i` is its place in the list; the caller has checked it is ticking.
    /// `entities` are the boxes that stop its move (`Game::item_entity_collisions`).
    pub(super) fn tick_one_tnt(&mut self, i: usize, entities: &[Aabb]) -> Result<()> {
        let t = &mut self.tnts[i];
        let tops = t.tick_among(
            &self.world,
            self.collision,
            &self.pack.assets,
            self.res.facts(),
            &self.res.motion().fluids,
            Some(self.border.geometry()),
            Some(&self.res.motion().blocks),
            entities,
        )?;
        let snow = (t.previous, t.position, t.bounds(), t.velocity);
        self.entity_powder_snow_particles(snow.0, snow.1, snow.2, snow.3, false);
        for pos in tops {
            self.bubble_column_particles(pos);
        }
        let t = &mut self.tnts[i];
        if t.fuse <= 0 {
            let center = [
                t.position.x,
                t.position.y + PrimedTnt::SIZE * 0.0625,
                t.position.z,
            ];
            let bounds = t.bounds();
            let (id, owner, guest, power) = (t.id, t.owner, t.guest, t.power);
            let (owner, guest) = match owner {
                Some(_) => (owner, guest),
                None => self
                    .loaded_tnt_owner(id)
                    .map_or((None, None), |(w, g)| (Some(w), g)),
            };
            let in_water = self.bounds_in_water(bounds);
            self.tnts.remove(i);
            // PrimedTnt.tick: with `tnt_explodes` off it goes away without exploding.
            if !self.ui.rules.bool("tnt_explodes") {
                return Ok(());
            }
            // ExplosionInteraction.TNT: the tnt_explosion_drop_decay rule.
            let interaction = self.destroy_type("tnt_explosion_drop_decay");
            self.combat.explosion_in_water = in_water;
            // `Explosion.getIndirectSourceEntity`: a player-lit TNT names
            // its igniter, which `blast_attacker` and the damage type read.
            self.combat.blast_guest = guest;
            self.combat.blast_owner = tnt_blast_owner(
                owner,
                || {
                    self.lan
                        .as_ref()
                        .and_then(|host| self.player_attacker(host, guest))
                },
                |id| self.mob_display_name(id),
            );
            let result = self
                .with_hive_emptying_blast(|g| g.server_explode(center, power, interaction, false));
            self.combat.blast_owner = None;
            self.combat.blast_guest = None;
            self.combat.explosion_in_water = false;
            result?;
        }
        Ok(())
    }
    /// The saved compound of primed TNT `i`: chunk saves, `/data get`, portals.
    pub(super) fn tnt_nbt(&mut self, i: usize) -> horizon_world::nbt::Tag {
        let id = self.tnts[i].id;
        let uuid = self.uuid_for(id);
        let owner = self.tnt_owner_uuid(self.tnts[i].owner, self.tnts[i].guest);
        let t = &self.tnts[i];
        horizon_client::entity_nbt::tnt_to_nbt(
            t.position,
            t.velocity,
            t.on_ground,
            t.fuse,
            t.power,
            t.block_state_nbt(self.pack.assets.registry),
            t.rotation,
            uuid,
            owner,
            self.persist.raw.get(&id),
        )
    }
    /// `EntityReference.of(owner)`: the UUID of the player or mob that lit it.
    fn tnt_owner_uuid(&mut self, owner: Option<Who>, guest: Option<u32>) -> Option<[i32; 4]> {
        match owner? {
            Who::Player => match guest {
                Some(conn) => self
                    .lan
                    .as_ref()?
                    .hub
                    .guest(conn)
                    .map(|g| uuid_ints(g.uuid)),
                None => self.level.player_uuid,
            },
            Who::Mob(id) if self.mobs.iter().any(|m| m.id == id) => Some(self.uuid_for(id)),
            Who::Mob(id) => self.persist.uuids.get(&id).copied(),
        }
    }
    /// `PrimedTnt.getOwner` for TNT loaded with an `owner`: looked up when
    /// asked, so a player who joined or a mob loaded since is found.
    fn loaded_tnt_owner(&self, id: u32) -> Option<(Who, Option<u32>)> {
        let uuid = horizon_client::entity_nbt::tnt_owner(self.persist.raw.get(&id)?)?;
        let guest = self.lan.as_ref().and_then(|host| {
            let mut guests = host.hub.guests.iter();
            guests.find(|g| uuid_ints(g.uuid) == uuid).map(|g| g.conn)
        });
        if guest.is_some() {
            return Some((Who::Player, guest));
        }
        if self.level.player_uuid == Some(uuid) {
            return Some((Who::Player, None));
        }
        self.mob_by_uuid(uuid).map(|m| (Who::Mob(m), None))
    }
    /// `TntBlock.useItemOn`: flint and steel or a fire charge primes TNT.
    pub(super) fn ignite_tnt(&mut self, stack: &ItemStack) -> Result<bool> {
        let item = &*stack.item;
        if item != "minecraft:flint_and_steel" && item != "minecraft:fire_charge" {
            return Ok(false);
        }
        let Some(hit) = self.pick() else {
            return Ok(false);
        };
        let state = self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]);
        if &*self.res.facts().get(state).name != "minecraft:tnt" {
            return Ok(false);
        }
        let pos = BlockPos::new(hit.pos[0], hit.pos[1], hit.pos[2]);
        if !self.ui.rules.bool("tnt_explodes") {
            // TntBlock.useItemOn: the rule is off, the acting player's overlay
            // says so, and PASS lets the item's own useOn light a fire there.
            let message = Text::translate("block.minecraft.tnt.disabled", &[]);
            self.send_overlay_message(message);
            return Ok(false);
        }
        if !self.tnt_prime_allowed(pos) {
            return Ok(true);
        }
        let guest = self.lan_acting;
        self.spawn_primed_tnt(pos, false, Some(Who::Player), guest);
        let air = self.world.air;
        self.with_level(|b, level, ticks| {
            b.set_block(level, ticks, pos, air, level_flags::UPDATE_ALL_IMMEDIATE)
        });
        self.process_level_events()?;
        self.swing_arm();
        if item == "minecraft:flint_and_steel" {
            self.damage_held(1);
        } else {
            self.consume_held();
        }
        self.award_stat("used", item, 1);
        Ok(true)
    }
    /// `TntRenderer`: the block centred on the entity, swelling over the last
    /// 10 ticks and flashing white every 5 ticks.
    pub(super) fn tnt_geometry(&self, camera: glam::DVec3, alpha: f64, out: &mut Vec<Vertex>) {
        if self.tnts.is_empty() {
            return;
        }
        let Some(state) = self.pack.assets.registry.default_state("minecraft:tnt") else {
            return;
        };
        // Face sprites from the block model: up, down, north, south, west, east.
        let Ok(models) = self
            .pack
            .render
            .model_catalog
            .collect(state, BlockPos::new(0, 0, 0))
        else {
            return;
        };
        let mut sprites = [None; 6];
        for face in models.iter().flat_map(|(_, m)| m.faces.iter()) {
            use horizon_assets::model::Direction as D;
            let index = match face.quad.facing {
                D::Up => 0,
                D::Down => 1,
                D::North => 2,
                D::South => 3,
                D::West => 4,
                D::East => 5,
            };
            if sprites[index].is_none() {
                sprites[index] = self
                    .pack
                    .layer_names
                    .get(&face.layer)
                    .and_then(|name| self.gui.atlas.sprites.get(name).copied());
            }
        }
        let range = super::entity_culling::default_range(
            glam::DVec3::splat(PrimedTnt::SIZE),
            self.view_scale(),
        );
        for t in &self.tnts {
            // EntityRenderer.shouldRender.
            if !self.standing_entity_in_view(
                camera,
                super::entity_culling::at(t.previous),
                super::entity_culling::at(t.position),
                alpha,
                PrimedTnt::SIZE,
                PrimedTnt::SIZE,
                range,
                0.0,
            ) {
                continue;
            }
            let fuse = t.fuse as f32 - alpha as f32 + 1.0;
            let scale = if fuse < 10.0 {
                let g = (1.0 - fuse / 10.0).clamp(0.0, 1.0);
                1.0 + g * g * g * g * 0.3
            } else {
                1.0
            };
            let lit = fuse >= 0.0 && (fuse / 5.0) as i32 % 2 == 0;
            let base = glam::DVec3::new(
                t.previous.x + (t.position.x - t.previous.x) * alpha,
                t.previous.y + (t.position.y - t.previous.y) * alpha + 0.5,
                t.previous.z + (t.position.z - t.previous.z) * alpha,
            ) - camera;
            let o = base.as_vec3();
            let light = self.world.light(
                t.position.x.floor() as i32,
                (t.position.y + 0.5).floor() as i32,
                t.position.z.floor() as i32,
            );
            let packed = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
            let v = |x: f32, y: f32, z: f32| o + glam::Vec3::new(x - 0.5, y - 0.5, z - 0.5) * scale;
            let faces = [
                (
                    [v(0., 1., 1.), v(1., 1., 1.), v(1., 1., 0.), v(0., 1., 0.)],
                    1.0,
                ),
                (
                    [v(0., 0., 0.), v(1., 0., 0.), v(1., 0., 1.), v(0., 0., 1.)],
                    0.5,
                ),
                (
                    [v(1., 0., 0.), v(0., 0., 0.), v(0., 1., 0.), v(1., 1., 0.)],
                    0.8,
                ),
                (
                    [v(0., 0., 1.), v(1., 0., 1.), v(1., 1., 1.), v(0., 1., 1.)],
                    0.8,
                ),
                (
                    [v(0., 0., 0.), v(0., 0., 1.), v(0., 1., 1.), v(0., 1., 0.)],
                    0.6,
                ),
                (
                    [v(1., 0., 1.), v(1., 0., 0.), v(1., 1., 0.), v(1., 1., 1.)],
                    0.6,
                ),
            ];
            for ((corners, shade), sprite) in faces.into_iter().zip(sprites) {
                let Some(r) = sprite else { continue };
                let c = (255.0 * shade) as u32;
                let color = 0xFF00_0000 | c << 16 | c << 8 | c;
                for (p, uv) in
                    corners
                        .iter()
                        .zip([[r.u0, r.v1], [r.u1, r.v1], [r.u1, r.v0], [r.u0, r.v0]])
                {
                    // Overlay 5: OverlayTexture white flash.
                    out.push(Vertex {
                        normal: [0; 4],
                        pos: p.to_array(),
                        uv,
                        layer: if lit { 5 } else { 0 },
                        color,
                        light: packed,

                        glint_uv: [0.0, 0.0],
                    });
                }
            }
        }
    }
}

impl Game<'_> {
    /// `positionRider` for the mobs riding primed TNT: the passenger attachment
    /// (the top of its 0.98 box), less the rider's own vehicle attachment.
    pub(super) fn seat_tnt_mobs(&mut self, riders: &super::passengers::Riders) {
        for i in 0..self.tnts.len() {
            let (id, pos, prev) = (
                self.tnts[i].id,
                self.tnts[i].position,
                self.tnts[i].previous,
            );
            let Some(js) = riders.get(&id).filter(|_| id != 0) else {
                continue;
            };
            let tnt_box = self.tnts[i].bounds();
            for &j in js {
                let m = &mut self.mobs[j];
                // Primed TNT's box is 0.98 wide.
                let offset = horizon_client::mobs::riders::vehicle_attachment(m, 0.98);
                m.seated(id, offset, tnt_box);
                m.position = tnt_seat(pos, offset);
                m.previous = tnt_seat(prev, offset);
                m.velocity = Vec3::ZERO;
                m.fall_distance = 0.0;
                m.on_ground = false;
                m.extra.passenger = true;
                m.ai.nav.stop();
            }
        }
    }
}

/// `PrimedTnt.getOwner` as the blast's causing entity: the player, or a mob
/// that is still in the level (null once it is gone).
fn tnt_blast_owner(
    owner: Option<Who>,
    player: impl FnOnce() -> Option<(u32, Text)>,
    mob_name: impl Fn(u32) -> Option<Text>,
) -> Option<(u32, Text)> {
    match owner? {
        Who::Player => player(),
        Who::Mob(id) => mob_name(id).map(|name| (id, name)),
    }
}

fn tnt_seat(tnt: Vec3, rider_offset: f64) -> Vec3 {
    Vec3::new(tnt.x, tnt.y + PrimedTnt::SIZE - rider_offset, tnt.z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_block_state_follows_the_model_instead_of_raw_input() {
        use horizon_world::{
            nbt::Tag,
            registry::{BlockRegistry, BlockStateId},
        };
        let unstable = Tag::compound([
            ("id", Tag::String("minecraft:tnt".into())),
            (
                "properties",
                Tag::compound([("unstable", Tag::String("true".into()))]),
            ),
        ]);
        let registry = BlockRegistry::from_nbt(&Tag::compound([
            ("DataVersion", Tag::Int(horizon_world::DATA_VERSION)),
            (
                "states",
                Tag::List(vec![
                    Tag::String("minecraft:tnt".into()),
                    Tag::String("minecraft:stone".into()),
                    unstable.clone(),
                ]),
            ),
            (
                "defaults",
                Tag::compound([
                    ("minecraft:tnt", Tag::Int(0)),
                    ("minecraft:stone", Tag::Int(1)),
                ]),
            ),
        ]))
        .unwrap();
        let mut tnt = PrimedTnt {
            rotation: [0.0, 0.0],
            join: Default::default(),
            id: 1,
            position: Vec3::ZERO,
            previous: Vec3::ZERO,
            velocity: Vec3::ZERO,
            fuse: 400,
            on_ground: false,
            owner: None,
            guest: None,
            no_gravity: true,
            power: 4.0,
            block_state: Some(BlockStateId(1)),
        };
        let raw = Tag::compound([("block_state", Tag::String("audit_missing:block".into()))]);
        for (state, expected) in [
            (Some(BlockStateId(1)), Tag::String("minecraft:stone".into())),
            (Some(BlockStateId(2)), unstable),
            (None, Tag::String("minecraft:tnt".into())),
        ] {
            tnt.block_state = state;
            let saved = horizon_client::entity_nbt::tnt_to_nbt(
                tnt.position,
                tnt.velocity,
                tnt.on_ground,
                tnt.fuse,
                tnt.power,
                tnt.block_state_nbt(&registry),
                [0.0, 0.0],
                [221, 0, 0, 1],
                None,
                Some(&raw),
            );
            assert_eq!(saved.get("block_state"), Some(&expected));
        }
    }

    #[test]
    fn a_tnt_lit_by_a_skeletons_arrow_names_the_skeleton_as_its_cause() {
        let name = || Text::translate("entity.minecraft.skeleton", &[]);
        let by_mob = tnt_blast_owner(Some(Who::Mob(7)), || None, |id| (id == 7).then(name));
        assert_eq!(by_mob, Some((7, name())));
        // The skeleton died before the blast: `getOwner` is null.
        assert_eq!(tnt_blast_owner(Some(Who::Mob(7)), || None, |_| None), None);
        let player = Text::literal("Steve");
        let by_player = tnt_blast_owner(Some(Who::Player), || Some((1, player.clone())), |_| None);
        assert_eq!(by_player, Some((1, player)));
        assert_eq!(tnt_blast_owner(None, || None, |_| None), None);
    }

    #[test]
    fn a_rider_sits_on_top_of_the_tnt_less_its_own_attachment() {
        // 64 + 0.98 (the box's top) - 0.14 (a rider's vehicle attachment).
        let seat = tnt_seat(Vec3::new(1.5, 64.0, -2.5), 0.14);
        assert_eq!((seat.x, seat.z), (1.5, -2.5));
        assert!((seat.y - 64.84).abs() < 1e-9);
    }

    #[test]
    fn a_piston_slime_bounce_launches_primed_tnt() {
        use horizon_core::geometry::Direction;
        use horizon_world::piston_push::bounce_velocity;
        let mut tnt = PrimedTnt {
            rotation: [0.0, 0.0],
            join: Default::default(),
            no_gravity: false,
            id: 0,
            position: Vec3::new(0.5, 1.0, 0.5),
            previous: Vec3::new(0.5, 1.0, 0.5),
            velocity: Vec3::ZERO,
            fuse: 80,
            on_ground: true,
            owner: None,
            guest: None,
            power: 4.0,
            block_state: None,
        };
        tnt.velocity = bounce_velocity(Direction::Up, tnt.velocity);
        tnt.on_ground = false;
        assert_eq!(tnt.velocity.y, 1.0);
        assert!(!tnt.on_ground);
    }

    /// `Entity.collectColliders` (#7080): the world border's wall stops primed
    /// TNT. A border of size 10 around (20, 20) has its east wall at x = 25;
    /// the TNT starts at x = 24 on the test scene's flat ground, 1.5 east.
    #[test]
    fn the_world_border_stops_primed_tnt() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("movement-properties.nbt").exists() {
            return;
        }
        let assets = horizon_client::assets::Assets::load(&root).unwrap();
        let render = assets.reload(&root).unwrap();
        let facts = horizon_client::blocks::BlockFacts::load(&root, assets.registry).unwrap();
        let fluids = horizon_client::motion::FluidResources::new(&assets, &facts).unwrap();
        let collision = horizon_client::collisions::load(&root, assets.registry).unwrap();
        let world = horizon_client::world::World::test_scene(&assets, &render, 3).unwrap();
        let border =
            horizon_core::geometry::border::BorderGeometry::new([20.0, 20.0], 10.0, 29_999_984)
                .unwrap();
        for bordered in [false, true] {
            let at = Vec3::new(24.0, 4.0, 20.5);
            let mut tnt = PrimedTnt {
                rotation: [0.0, 0.0],
                join: Default::default(),
                no_gravity: false,
                id: 0,
                position: at,
                previous: at,
                velocity: Vec3::new(1.5, 0.0, 0.0),
                fuse: 80,
                on_ground: true,
                owner: None,
                guest: None,
                power: 4.0,
                block_state: None,
            };
            let border = bordered.then_some(&border);
            tnt.tick(&world, &collision, &assets, &facts, &fluids, border, None)
                .unwrap();
            let east = tnt.bounds().max_position().x;
            if bordered {
                assert!(
                    (east - 25.0).abs() < 1.0e-7,
                    "stops on the wall, got {east}"
                );
            } else {
                assert!(east > 25.0, "no border: goes on, got {east}");
            }
        }
    }

    /// `Entity.move`'s last step, `getBlockSpeedFactor`: primed TNT sliding
    /// over soul sand or honey keeps 40% of the horizontal speed it keeps over
    /// stone. Skips quietly without the vanilla data (CI has none).
    #[test]
    fn primed_tnt_on_soul_sand_or_honey_is_slowed_by_the_move() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("movement-properties.nbt").exists() {
            return;
        }
        let assets = horizon_client::assets::Assets::load(&root).unwrap();
        let render = assets.reload(&root).unwrap();
        let facts = horizon_client::blocks::BlockFacts::load(&root, assets.registry).unwrap();
        let fluids = horizon_client::motion::FluidResources::new(&assets, &facts).unwrap();
        let collision = horizon_client::collisions::load(&root, assets.registry).unwrap();
        let motion = horizon_client::motion::MotionResources::load(&root, &assets).unwrap();
        let mut after = Vec::new();
        // The tops of the blocks: stone 1, soul sand 7/8, honey 15/16.
        for (block, top) in [
            ("minecraft:stone", 4.0),
            ("minecraft:soul_sand", 3.875),
            ("minecraft:honey_block", 3.9375),
        ] {
            let mut world = horizon_client::world::World::test_scene(&assets, &render, 3).unwrap();
            let id = assets.registry.default_state(block).unwrap();
            for x in 18..=23 {
                for z in 19..=21 {
                    world.set(x, 3, z, id).unwrap();
                }
            }
            let at = Vec3::new(19.5, top, 20.5);
            let mut tnt = PrimedTnt {
                rotation: [0.0, 0.0],
                join: Default::default(),
                no_gravity: false,
                id: 0,
                position: at,
                previous: at,
                velocity: Vec3::new(0.2, 0.0, 0.0),
                fuse: 80,
                on_ground: true,
                owner: None,
                guest: None,
                power: 4.0,
                block_state: None,
            };
            tnt.tick(
                &world,
                &collision,
                &assets,
                &facts,
                &fluids,
                None,
                Some(&motion.blocks),
            )
            .unwrap();
            after.push(tnt.velocity.x);
        }
        let (stone, soul, honey) = (after[0], after[1], after[2]);
        assert!(stone > 0.0);
        assert!(
            (soul - stone * f64::from(0.4_f32)).abs() < 1.0e-9,
            "soul sand {soul} vs stone {stone}"
        );
        assert!(
            (honey - stone * f64::from(0.4_f32)).abs() < 1.0e-9,
            "honey {honey} vs stone {stone}"
        );
    }

    #[test]
    fn primed_tnt_follows_water_currents_after_moving() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("movement-properties.nbt").exists() {
            return;
        }
        let assets = horizon_client::assets::Assets::load(&root).unwrap();
        let render = assets.reload(&root).unwrap();
        let facts = horizon_client::blocks::BlockFacts::load(&root, assets.registry).unwrap();
        let fluids = horizon_client::motion::FluidResources::new(&assets, &facts).unwrap();
        let collision = horizon_client::collisions::load(&root, assets.registry).unwrap();
        let mut world = horizon_client::world::World::test_scene(&assets, &render, 3).unwrap();
        let stone = assets.registry.default_state("minecraft:stone").unwrap();
        let water = assets.registry.default_state("minecraft:water").unwrap();
        let mut state = assets.registry.state(water).unwrap().clone();
        state.properties.insert("level".into(), "1".into());
        let flowing = assets.registry.resolve(&state).unwrap();
        for x in 19..=22 {
            for z in 19..=21 {
                world.set(x, 3, z, stone).unwrap();
            }
        }
        world.set(20, 4, 20, water).unwrap();
        world.set(21, 4, 20, flowing).unwrap();
        let at = Vec3::new(20.5, 4.0, 20.5);
        let mut tnt = PrimedTnt {
            rotation: [0.0, 0.0],
            join: Default::default(),
            no_gravity: false,
            id: 0,
            position: at,
            previous: at,
            velocity: Vec3::ZERO,
            fuse: 80,
            on_ground: false,
            owner: None,
            guest: None,
            power: 4.0,
            block_state: None,
        };
        tnt.tick(&world, &collision, &assets, &facts, &fluids, None, None)
            .unwrap();
        assert_eq!(
            tnt.position.x, at.x,
            "current is applied after motion, not before"
        );
        assert!(
            (tnt.velocity.x - 0.014).abs() < 1e-12,
            "water must push primed TNT east: {:?}",
            tnt.velocity
        );
        assert_eq!(tnt.fuse, 79);
        tnt.tick(&world, &collision, &assets, &facts, &fluids, None, None)
            .unwrap();
        assert!((tnt.position.x - at.x - 0.014).abs() < 1e-12);
        assert!(tnt.on_ground);
        tnt.position = at;
        tnt.velocity = Vec3::ZERO;
        tnt.fuse = 1;
        tnt.tick(&world, &collision, &assets, &facts, &fluids, None, None)
            .unwrap();
        assert_eq!(tnt.fuse, 0);
        assert_eq!(
            tnt.velocity.x, 0.0,
            "expired TNT must not receive a current"
        );
        world.set(21, 4, 20, water).unwrap();
        tnt.position = at;
        tnt.velocity = Vec3::ZERO;
        tnt.fuse = 80;
        tnt.tick(&world, &collision, &assets, &facts, &fluids, None, None)
            .unwrap();
        assert_eq!(
            tnt.velocity.x, 0.0,
            "still water must not add horizontal motion"
        );
        world
            .set(
                20,
                3,
                20,
                assets.registry.default_state("minecraft:air").unwrap(),
            )
            .unwrap();
        tnt.on_ground = false;
        tnt.velocity = Vec3::ZERO;
        tnt.tick(&world, &collision, &assets, &facts, &fluids, None, None)
            .unwrap();
        assert!(
            tnt.position.y < at.y && tnt.velocity.y < 0.0,
            "TNT sinks; water does not add buoyancy"
        );
    }

    #[test]
    fn a_soul_sand_bubble_column_lifts_primed_tnt() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("movement-properties.nbt").exists() {
            return;
        }
        let assets = horizon_client::assets::Assets::load(&root).unwrap();
        let render = assets.reload(&root).unwrap();
        let facts = horizon_client::blocks::BlockFacts::load(&root, assets.registry).unwrap();
        let fluids = horizon_client::motion::FluidResources::new(&assets, &facts).unwrap();
        let collision = horizon_client::collisions::load(&root, assets.registry).unwrap();
        let mut world = horizon_client::world::World::test_scene(&assets, &render, 3).unwrap();
        let water = assets.registry.default_state("minecraft:water").unwrap();
        let soul_sand = assets
            .registry
            .default_state("minecraft:soul_sand")
            .unwrap();
        // Soul sand makes an up-pushing column (`drag=false`); the block's
        // default state is the down-dragging one.
        let drag_down = assets
            .registry
            .default_state("minecraft:bubble_column")
            .unwrap();
        let mut up = assets.registry.state(drag_down).unwrap().clone();
        up.properties.insert("drag".into(), "false".into());
        let column = assets.registry.resolve(&up).unwrap();
        for y in 3..=8 {
            world.set(20, y, 20, column).unwrap();
        }
        world.set(20, 2, 20, soul_sand).unwrap();
        world.set(20, 9, 20, water).unwrap();
        // BubbleColumnBlock.entityInside: the columns its box touches lift it
        // (Entity.handleOnInsideBubbleColumn / handleOnAboveBubbleColumn).
        let at = Vec3::new(20.5, 6.0, 20.5);
        let mut tnt = PrimedTnt {
            rotation: [0.0, 0.0],
            join: Default::default(),
            no_gravity: false,
            id: 0,
            position: at,
            previous: at,
            velocity: Vec3::ZERO,
            fuse: 80,
            on_ground: false,
            owner: None,
            guest: None,
            power: 4.0,
            block_state: None,
        };
        for _ in 0..10 {
            tnt.tick(&world, &collision, &assets, &facts, &fluids, None, None)
                .unwrap();
        }
        assert!(
            tnt.velocity.y > 0.0,
            "soul sand lifts primed TNT, got {:?}",
            tnt.velocity
        );
        assert!(
            tnt.position.y > at.y,
            "the column carries it up, got {}",
            tnt.position.y
        );
        // Without the column the same TNT only falls.
        for y in 3..=8 {
            world.set(20, y, 20, water).unwrap();
        }
        tnt.position = at;
        tnt.velocity = Vec3::ZERO;
        tnt.on_ground = false;
        for _ in 0..4 {
            tnt.tick(&world, &collision, &assets, &facts, &fluids, None, None)
                .unwrap();
        }
        assert!(
            tnt.position.y < at.y,
            "water alone does not lift it, got {}",
            tnt.position.y
        );
    }

    /// `Entity.onAboveBubbleColumn` sends the surface particles only from the top of
    /// a column: `tick` names the column block it is at the top of, and none while
    /// the column has water above it.
    #[test]
    fn primed_tnt_reports_the_surface_of_a_bubble_column() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("movement-properties.nbt").exists() {
            return;
        }
        let assets = horizon_client::assets::Assets::load(&root).unwrap();
        let render = assets.reload(&root).unwrap();
        let facts = horizon_client::blocks::BlockFacts::load(&root, assets.registry).unwrap();
        let fluids = horizon_client::motion::FluidResources::new(&assets, &facts).unwrap();
        let collision = horizon_client::collisions::load(&root, assets.registry).unwrap();
        let mut world = horizon_client::world::World::test_scene(&assets, &render, 3).unwrap();
        let water = assets.registry.default_state("minecraft:water").unwrap();
        let soul_sand = assets
            .registry
            .default_state("minecraft:soul_sand")
            .unwrap();
        let default = assets
            .registry
            .default_state("minecraft:bubble_column")
            .unwrap();
        let mut up = assets.registry.state(default).unwrap().clone();
        up.properties.insert("drag".into(), "false".into());
        let column = assets.registry.resolve(&up).unwrap();
        for y in 3..=8 {
            world.set(20, y, 20, column).unwrap();
        }
        world.set(20, 2, 20, soul_sand).unwrap();
        let at = Vec3::new(20.5, 6.0, 20.5);
        let primed = || PrimedTnt {
            rotation: [0.0, 0.0],
            join: Default::default(),
            no_gravity: false,
            id: 0,
            position: at,
            previous: at,
            velocity: Vec3::ZERO,
            fuse: 80,
            on_ground: false,
            owner: None,
            guest: None,
            power: 4.0,
            block_state: None,
        };
        let mut tops = Vec::new();
        let mut tnt = primed();
        for _ in 0..30 {
            tops.extend(
                tnt.tick(&world, &collision, &assets, &facts, &fluids, None, None)
                    .unwrap(),
            );
        }
        assert!(!tops.is_empty(), "it reaches the top of the column");
        assert!(
            tops.iter().all(|p| *p == BlockPos::new(20, 8, 20)),
            "only the surface block, got {tops:?}"
        );
        // Water above the top block: it is no longer the surface.
        world.set(20, 9, 20, water).unwrap();
        let mut tnt = primed();
        for _ in 0..10 {
            assert!(
                tnt.tick(&world, &collision, &assets, &facts, &fluids, None, None)
                    .unwrap()
                    .is_empty()
            );
        }
    }

    /// A server's game on the prepared resources with the chunk at the origin
    /// loaded, as `native_machines`' tests make one; None without them.
    fn server_game() -> Option<Game<'static>> {
        use crate::game::*;
        let root = std::env::var_os("HORIZON_TEST_RESOURCES")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../.local/horizon-client")
                    .join(horizon_core::VANILLA_VERSION)
            });
        if !root.join("movement-properties.nbt").exists() {
            return None;
        }
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
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let res: &'static _ = Box::leak(Box::new(loaded.resources));
        let mut game = Game::new(
            res,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.options.dedicated = Some(Vec::new());
        game.world.chunks.insert(
            (0, 0),
            std::sync::Arc::new(horizon_client::chunk::Chunk::empty(0, 0)),
        );
        game.streaming.forced.insert((0, 0));
        Some(game)
    }

    #[test]
    fn loaded_tnt_fuse_uses_numeric_short_and_saves_without_raw() {
        use horizon_world::nbt::Tag;
        let Some(mut game) = server_game() else {
            return;
        };
        let cases = [
            (None, 80),
            (Some(Tag::Short(80)), 80),
            (Some(Tag::Short(2)), 2),
            (Some(Tag::Short(-1)), -1),
            (Some(Tag::Byte(-2)), -2),
            (Some(Tag::Int(32768)), -32768),
            (Some(Tag::Long(65535)), -1),
            (Some(Tag::Long(i64::MAX)), -1),
            (Some(Tag::Float((-1.5_f32).to_bits())), -2),
            (Some(Tag::Double((-1.5_f64).to_bits())), -2),
            (Some(Tag::Double(1.5_f64.to_bits())), 1),
            (Some(Tag::Double(32768.75_f64.to_bits())), -32768),
            (Some(Tag::String("wrong_type".into())), 80),
        ];
        for (value, expected) in cases {
            let mut tag = Tag::compound([
                (
                    "Pos",
                    Tag::List(vec![
                        Tag::Double(4.5_f64.to_bits()),
                        Tag::Double(200_f64.to_bits()),
                        Tag::Double(4.5_f64.to_bits()),
                    ]),
                ),
                (
                    "Motion",
                    Tag::List(vec![
                        Tag::Double(0.125_f64.to_bits()),
                        Tag::Double(0),
                        Tag::Double(0),
                    ]),
                ),
                ("NoGravity", Tag::Byte(1)),
                ("block_state", Tag::String("minecraft:stone".into())),
                ("explosion_power", Tag::Float(2.5_f32.to_bits())),
                ("owner", Tag::IntArray(vec![5, 6, 7, 8])),
                ("CustomName", Tag::String("Fuse control".into())),
                ("Tags", Tag::List(vec![Tag::String("h090".into())])),
            ]);
            if let (Some(value), Tag::Compound(fields)) = (value, &mut tag) {
                fields.insert("fuse".into(), value);
            }
            let i = game.tnts.len();
            game.load_tnt(tag.clone(), Some([90, 0, 0, i as i32 + 1]));
            let t = &game.tnts[i];
            assert_eq!(t.fuse, expected, "{tag:?}");
            assert_eq!(t.velocity, Vec3::new(0.125, 0.0, 0.0));
            assert!(t.no_gravity);
            assert_eq!(t.power, 2.5);
            let state = t.block_state_nbt(game.pack.assets.registry);
            assert_eq!(state, Tag::String("minecraft:stone".into()));
            let saved = crate::entity_nbt::tnt_to_nbt(
                t.position,
                t.velocity,
                t.on_ground,
                t.fuse,
                t.power,
                state,
                t.rotation,
                [90, 0, 0, i as i32 + 1],
                None,
                None,
            );
            assert_eq!(saved.get("fuse"), Some(&Tag::Short(expected as i16)));
            let saved = game.tnt_nbt(i);
            for field in [
                "owner",
                "CustomName",
                "Tags",
                "Motion",
                "block_state",
                "explosion_power",
            ] {
                assert_eq!(saved.get(field), tag.get(field), "{field}");
            }
        }
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn live_data_commands_adopt_into_the_existing_tnt() {
        use crate::game::commands::Target;
        use horizon_world::nbt::Tag;
        let mut game = server_game().expect("prepared Game resources are required");
        game.build_registries();
        game.set_allow_commands(true);
        game.block_ticks
            .add_empty_chunk(horizon_core::position::ChunkPos::new(0, 0))
            .unwrap();
        assert!(game.streaming.forced.contains(&(0, 0)));
        // The existing unit helper supplies a resident column. The paired
        // runtime harness separately admits actual ticket/streaming readiness.
        assert!(game.world.chunks.contains_key(&(0, 0)));
        assert!(game.run_command(
            "summon pig 8.5 200 5.5 {UUID:[I;89,0,0,3],Health:10f,NoAI:1b,NoGravity:1b}"
        ));
        assert!(game.run_command("summon tnt 5.5 200 5.5 {UUID:[I;89,0,0,1],fuse:40s,Motion:[0d,0d,0d],NoGravity:1b,explosion_power:2f,owner:[I;89,0,0,3],Tags:[keep],Silent:1b}"));
        let pig = game
            .mobs
            .iter()
            .find(|m| game.persist.uuid_of(m.id) == Some([89, 0, 0, 3]))
            .unwrap()
            .id;
        assert_eq!(
            game.mobs.iter().find(|m| m.id == pig).unwrap().kind.id,
            "minecraft:pig"
        );
        assert_eq!(game.tnts.len(), 1);
        let id = game.tnts[0].id;
        assert_eq!(game.persist.uuid_of(id), Some([89, 0, 0, 1]));
        assert_eq!(game.tnts[0].fuse, 40);
        assert_eq!(game.tnts[0].velocity, Vec3::ZERO);
        assert!(game.tnts[0].no_gravity);
        // Preserve a live owner referencing an actually registered mob.
        game.tnts[0].owner = Some(Who::Mob(pig));
        let before = game.tnts[0].clone();
        let next_id = game.next_entity;
        let clock = game.world.game_time;
        assert!(before.join.is_joined());
        let reference = Target::Tnt(id);
        let saved_before = game.entity_nbt(reference).unwrap();
        assert_eq!(saved_before.get("NoGravity"), Some(&Tag::Byte(1)));
        assert_eq!(
            saved_before.get("owner"),
            Some(&Tag::IntArray(vec![89, 0, 0, 3]))
        );
        assert!(
            !game.run_command("data merge entity 00000059-0000-0000-0000-000000000001 {fuse:40s}")
        );
        assert!(game.run_command("data merge entity 00000059-0000-0000-0000-000000000001 {fuse:60s,Motion:[0.125d,0d,-0.25d],Tags:[adopted]}"), "valid TNT data must reach the existing live instance");
        assert_eq!(
            game.next_entity, next_id,
            "adoption must not spawn another entity"
        );
        assert_eq!(game.tnts.len(), 1);
        let live = game.tnts.iter().find(|t| t.id == id).unwrap();
        assert_eq!(live.fuse, 60, "the actual modeled counter changes");
        assert_eq!(live.velocity, Vec3::new(0.125, 0.0, -0.25));
        assert_eq!(live.position, before.position);
        assert_eq!(live.previous, live.position);
        assert_eq!(live.rotation, before.rotation);
        assert_eq!(live.join, before.join);
        assert_eq!(live.owner, before.owner);
        assert_eq!(live.guest, before.guest);
        assert_eq!(live.power, before.power);
        assert_eq!(live.no_gravity, before.no_gravity);
        assert_eq!(game.world.game_time, clock);
        assert_eq!(game.persist.uuid_of(id), Some([89, 0, 0, 1]));
        let saved = game.entity_nbt(reference).unwrap();
        assert_eq!(saved.get("fuse"), Some(&Tag::Short(60)));
        assert_eq!(
            saved.get("Tags"),
            Some(&Tag::List(vec![Tag::String("adopted".into())]))
        );
        for field in [
            "UUID",
            "Pos",
            "NoGravity",
            "Rotation",
            "owner",
            "block_state",
            "explosion_power",
            "Silent",
        ] {
            assert!(
                saved_before.get(field).is_some(),
                "positive saved {field} control"
            );
            assert_eq!(
                saved.get(field),
                saved_before.get(field),
                "retained {field}"
            );
        }
    }

    /// #10063: TNT lit between level ticks (flint and steel, a command) has its
    /// first tick in the next level tick, so its fuse reads 79 after it, and it
    /// explodes on the 80th (`PrimedTnt.tick`). Horizon held every TNT that
    /// joined during a tick back one more: the late pass compared its join
    /// against an `announced` already raised past it.
    #[test]
    fn lit_tnt_ticks_on_the_next_level_tick_and_explodes_on_the_80th() {
        let Some(mut game) = server_game() else {
            return;
        };
        game.add_primed_tnt(BlockPos::new(8, 64, 8), 0.0, 80, None, None);
        // Hang it in the empty chunk, so it neither falls out nor lands.
        game.tnts[0].no_gravity = true;
        game.tnts[0].velocity = Vec3::ZERO;
        game.tick_world(true).unwrap();
        assert_eq!(game.tnts.len(), 1);
        assert_eq!(
            game.tnts[0].fuse, 79,
            "its first tick is the next level tick"
        );
        let mut exploded_on = None;
        for tick in 2..=90 {
            game.tick_world(true).unwrap();
            if game.tnts.is_empty() {
                exploded_on = Some(tick);
                break;
            }
        }
        assert_eq!(exploded_on, Some(80), "explodes on the 80th tick");
    }
    /// TNT a blast primes joins while the entities tick (vanilla's
    /// `EntityTickList` loop) and waits for the next tick. The fix for #10063
    /// must not tick it in the tick it joined.
    #[test]
    fn tnt_primed_by_a_blast_waits_for_the_next_tick() {
        let Some(mut game) = server_game() else {
            return;
        };
        let block = game
            .pack
            .assets
            .registry
            .default_state("minecraft:tnt")
            .unwrap();
        game.world.set(9, 64, 8, block).unwrap();
        game.add_primed_tnt(BlockPos::new(8, 64, 8), 0.0, 1, None, None);
        game.tnts[0].no_gravity = true;
        game.tnts[0].velocity = Vec3::ZERO;
        let first = game.tnts[0].id;
        for _ in 0..3 {
            game.tick_world(true).unwrap();
            if game.tnts.iter().all(|t| t.id != first) {
                break;
            }
        }
        assert!(
            game.tnts.iter().all(|t| t.id != first),
            "the first TNT went off"
        );
        assert_eq!(game.tnts.len(), 1, "its blast primed the TNT block");
        let primed = &game.tnts[0];
        assert_eq!(
            primed.position, primed.previous,
            "not ticked in the tick the blast primed it"
        );
        game.tick_world(true).unwrap();
        let primed = &game.tnts[0];
        assert_ne!(primed.position, primed.previous, "ticked on the next tick");
    }
}
horizon_client::impl_joined!(PrimedTnt);

#[cfg(test)]
mod entity_collision_tests {
    use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
    use horizon_client::chunk::Chunk;
    use horizon_core::position::ChunkPos;
    use std::sync::Arc;

    /// `Entity.collide` for primed TNT, a falling block and an experience orb:
    /// a boat holds the TNT and the orb up, a shulker the sand, as they hold
    /// up a dropped item (`canBeCollidedWith`).
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn tnt_sand_and_orbs_land_on_boats_and_shulkers() {
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
            std::env::temp_dir().join(format!("horizon-tnt-boat-unit-{}", std::process::id())),
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
        game.ui.difficulty = 2;
        let registry = &game.pack.assets.registry;
        let stone = registry.default_state("minecraft:stone").unwrap();
        let sand = registry.default_state("minecraft:sand").unwrap();
        let mut chunk = Chunk::empty(0, 0);
        for x in 0..16 {
            for z in 0..16 {
                chunk.set(x, 199, z, stone.0);
            }
        }
        game.world.chunks.insert((0, 0), Arc::new(chunk));
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(0, 0))
            .unwrap();
        assert!(game.run_command("forceload add 0 0"));
        assert!(game.run_command("summon oak_boat 4.5 200 4.5"));
        assert!(game.run_command("summon oak_boat 12.5 200 12.5"));
        assert!(game.run_command("summon shulker 8.5 200 8.5 {NoAI:1b}"));
        assert!(game.run_command("summon tnt 4.5 202 4.5 {fuse:80}"));
        assert!(game.run_command("summon tnt 4.5 202 12.5 {fuse:80}"));
        assert!(game.run_command(
            "summon falling_block 8.5 203 8.5 {BlockState:{Name:\"minecraft:sand\"},Time:1}"
        ));
        assert!(game.run_command("summon experience_orb 12.5 202 12.5 {Value:1}"));
        for _ in 0..40 {
            game.tick_world(true).unwrap();
        }
        let boat_top = game.boats[0].bounds().max_position().y;
        let tnt_at = |z: f64| {
            game.tnts
                .iter()
                .find(|t| (t.position.z - z).abs() < 0.5)
                .map(|t| t.position.y)
                .unwrap()
        };
        assert_eq!(tnt_at(4.5), boat_top, "the TNT rests on the boat");
        assert_eq!(tnt_at(12.5), 200.0, "the other TNT lands on the stone");
        assert_eq!(
            game.orbs[0].position.y, boat_top,
            "the orb rests on the boat"
        );
        // FallingBlockEntity.tick: it stops on the shulker, but the air
        // under it would let it fall on (`wouldContinueFalling`), so it breaks
        // into an item there instead of passing through to the stone.
        assert_ne!(
            game.world.block(8, 200, 8),
            sand,
            "the sand does not fall through"
        );
        assert_ne!(
            game.world.block(8, 201, 8),
            sand,
            "nor is it placed over air"
        );
        assert!(
            game.dropped
                .iter()
                .any(|i| &*i.stack.item == "minecraft:sand"),
            "the sand breaks into an item"
        );
    }
}
