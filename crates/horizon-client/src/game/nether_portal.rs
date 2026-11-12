//! Nether portals for the player and the other entities:
//! `Entity.setAsInsidePortal` / `handlePortal` with `PortalProcessor`,
//! `NetherPortalBlock.getPortalTransitionTime` and `getPortalDestination`
//! (`PortalForcer` through `horizon_world::level::portal_travel`), and
//! `LocalPlayer.handlePortalTransitionEffect`. Items and mobs also go through
//! End portals here (`EndPortalBlock.entityInside` / `getPortalDestination`);
//! the player's End portals are game/end.rs's.
//!
//! A server keeps one level per dimension. Entity portal processes and
//! cooldowns follow their level; shared `Arrival`s hold each traveller's
//! saved compound until its destination chunks are loaded.
use super::entity::{Entity as _, entity_lists_of, for_each_entity, with_entity};
use super::{Dim, Game, Result, Screen, level_flags};
use horizon_client::fast_hash::FxHashMap;
use horizon_client::nether_portal::{ENTITY_DELAY, PortalState};
use horizon_core::{
    geometry::{Aabb, Axis, Vec3},
    position::BlockPos,
    random::RandomSource,
};
use horizon_world::{
    dimension_type::DimensionFacts,
    level::portal_travel::{
        BorderBounds, FoundRectangle, closest_portal, collision_free_position, exit_position,
        largest_rectangle_around, relative_position,
    },
    nbt::Tag,
};
use std::collections::HashMap;

mod respawn_attributes;

/// `Player.getDimensionChangingDelay`.
pub(super) const PLAYER_DELAY: i32 = 10;

/// Where in its portal a traveller stood, and where its exit is to be found.
#[derive(Clone, Copy, Debug)]
pub(super) struct Plan {
    pub(super) dim: Dim,
    /// The border-clamped, scaled position (`approximateExitPos`).
    pub(super) approximate: BlockPos,
    entry_x_axis: bool,
    offset: Vec3,
    width: f32,
    height: f32,
    /// Through an End portal: `approximate` is the End's spawn point, or the
    /// Overworld's shared spawn.
    pub(super) end: bool,
}

impl Plan {
    /// `EndPortalBlock.getPortalDestination` for an entity other than a
    /// player in `from`: the End's spawn point, or out of the End the
    /// Overworld's shared spawn `world_spawn`.
    fn end_portal(from: Dim, world_spawn: BlockPos, width: f32, height: f32) -> Self {
        let (dim, approximate) = if from == Dim::End {
            (Dim::Overworld, world_spawn)
        } else {
            let [x, y, z] = super::end::END_SPAWN;
            (Dim::End, BlockPos::new(x, y, z))
        };
        Self {
            dim,
            approximate,
            entry_x_axis: true,
            offset: Vec3::ZERO,
            width,
            height,
            end: true,
        }
    }
}

/// An entity on its way to a dimension that isn't loaded.
#[derive(Clone, Debug)]
struct Arrival {
    plan: Plan,
    tag: Tag,
}

/// The nether portal state of the world (`Game::portals`).
#[derive(Default)]
pub(super) struct Portals {
    /// The player's.
    pub(super) player: PortalState,
    /// `LocalPlayer.portalProcess.isInsidePortalThisTick`.
    client_inside: bool,
    /// Whether the last tick's `handlePortalTransitionEffect` found the
    /// player inside a nether portal: it closes any screen opened meanwhile.
    client_was_inside: bool,
    /// Items and mobs, by entity id.
    pub(super) entities: HashMap<u32, PortalState>,
    /// Positions before this tick's moves, by entity id.
    pub(super) before: FxHashMap<u32, Vec3>,
    arrivals: Vec<Arrival>,
    /// The vehicles server players ride that went through a portal this
    /// tick, in their level, with their entry: they carry their riders
    /// (`tick_server_travel`).
    carrying: Vec<(Dim, u32, BlockPos)>,
    /// What a carrying minecart moved by when its portal was handled, at the
    /// start of its tick: its motion is what crosses (`arrive_vehicle`).
    pub(super) carried_motion: HashMap<u32, Vec3>,
    /// Where a carrying vehicle stood then: its plan is taken from there
    /// (`vehicle_plan`), as vanilla's is, not from where its rider's own tick
    /// has moved it on to since.
    pub(super) carried_at: HashMap<u32, Vec3>,
    loaded: bool,
    dirty: bool,
    /// Per-chunk portal palette masks, kept across ticks (portal_sections).
    pub(super) sections: horizon_client::portal_sections::PortalSections,
    /// The attributes a respawn hands from the old player to the new one.
    pub(super) respawn_attributes: respawn_attributes::RespawnAttributes,
}

fn rotate_y(v: Vec3, degrees: f32) -> Vec3 {
    // Vec3.yRot((float)Math.toRadians(degrees))
    let angle = f64::from(degrees.to_radians());
    let (cos, sin) = (
        f64::from(horizon_core::geometry::trig::cos(angle)),
        f64::from(horizon_core::geometry::trig::sin(angle)),
    );
    Vec3::new(v.x * cos + v.z * sin, v.y, v.z * cos - v.x * sin)
}
/// Whether a projectile is an arrow, which goes through a portal like the
/// other entities.
pub(super) fn is_arrow(p: &horizon_client::projectiles::Projectile) -> bool {
    matches!(
        p.kind,
        horizon_client::projectiles::ProjectileKind::Arrow { .. }
    )
}
/// The projectiles besides arrows that a portal sends on: the thrown items
/// and fireballs a chunk saves. A pearl has its own rules (it keeps its owner's
/// dimension) and a wind charge is not saved, so both stay.
pub(super) fn is_portal_projectile(p: &horizon_client::projectiles::Projectile) -> bool {
    use horizon_client::projectiles::ProjectileKind;
    !is_arrow(p)
        && !matches!(
            p.kind,
            ProjectileKind::EnderPearl
                | ProjectileKind::WindCharge
                | ProjectileKind::BreezeWindCharge
        )
        && horizon_client::entity_nbt::saved_with_chunk(p)
}
/// Width and height of a projectile that goes through a portal.
pub(super) fn projectile_box(p: &horizon_client::projectiles::Projectile) -> (f32, f32) {
    if is_arrow(p) {
        (
            horizon_client::arrows::WIDTH,
            horizon_client::arrows::HEIGHT,
        )
    } else {
        let size = p.kind.size() as f32;
        (size, size)
    }
}
/// `Entity.calculatePassengerTransition`: the riders under `tag`'s
/// `Passengers`, and theirs, keep their place on the vehicle: they move by
/// what it moved and turn by what it turned.
fn move_passengers(tag: &mut Tag, by: Vec3, turn: f32) {
    let Tag::Compound(c) = tag else {
        return;
    };
    let Some(Tag::List(riders)) = c.get_mut(&"Passengers".into()) else {
        return;
    };
    for rider in riders {
        if let Some(at) = horizon_client::entity_nbt::position(rider)
            && let Tag::Compound(r) = rider
        {
            r.insert(
                "Pos".into(),
                doubles([at.x + by.x, at.y + by.y, at.z + by.z]),
            );
            let [yaw, pitch] = horizon_client::entity_nbt::rotation(rider);
            if let Tag::Compound(r) = rider {
                r.insert(
                    "Rotation".into(),
                    Tag::List(vec![
                        Tag::Float((yaw + turn).to_bits()),
                        Tag::Float(pitch.to_bits()),
                    ]),
                );
            }
        }
        move_passengers(rider, by, turn);
    }
}
fn doubles(values: [f64; 3]) -> Tag {
    Tag::List(values.iter().map(|v| Tag::Double(v.to_bits())).collect())
}
/// Whether a `width` by `height` box moving from `from` to `to` meets the End
/// portal at `pos`: `Entity.collidedWithShapeMovingFrom` against its
/// entity-inside shape, y 6/16..12/16 of the block.
fn touches_end_portal(from: Vec3, to: Vec3, width: f32, height: f32, pos: BlockPos) -> bool {
    let half = f64::from(width) / 2.0;
    let start = Aabb::new(
        Vec3::new(from.x - half, from.y, from.z - half),
        Vec3::new(from.x + half, from.y + f64::from(height), from.z + half),
    );
    let (x, y, z) = (f64::from(pos.x), f64::from(pos.y), f64::from(pos.z));
    let shape = Aabb::new(
        Vec3::new(x, y + 0.375, z),
        Vec3::new(x + 1.0, y + 0.75, z + 1.0),
    );
    start.collided_along_vector(to.subtract(from), &[shape])
}
/// `PositionMoveRotation.calculateAbsolute` for an End portal's transition
/// of an entity facing `[yaw, pitch]`: into the End it faces west and keeps
/// its pitch (`Relative.X_ROT`); out of it both turn by the world spawn's
/// angles (`Relative.ROTATION`). The pitch stays within ±90.
fn end_portal_rotation(into_end: bool, [yaw, pitch]: [f32; 2], spawn: [f32; 2]) -> [f32; 2] {
    let (yaw, pitch) = if into_end {
        // Direction.WEST.toYRot().
        (90.0, pitch)
    } else {
        (yaw + spawn[0], pitch + spawn[1])
    };
    [yaw, pitch.clamp(-90.0, 90.0)]
}
/// A waiting entity as `horizon_portal_arrivals.dat` keeps it.
fn arrival_tag(a: &Arrival) -> Tag {
    let p = &a.plan;
    Tag::compound([
        ("dimension", Tag::String(p.dim.id().into())),
        ("x", Tag::Int(p.approximate.x)),
        ("y", Tag::Int(p.approximate.y)),
        ("z", Tag::Int(p.approximate.z)),
        ("x_axis", Tag::Int(i32::from(p.entry_x_axis))),
        ("right", Tag::Double(p.offset.x.to_bits())),
        ("up", Tag::Double(p.offset.y.to_bits())),
        ("forward", Tag::Double(p.offset.z.to_bits())),
        ("width", Tag::Float(p.width.to_bits())),
        ("height", Tag::Float(p.height.to_bits())),
        ("end", Tag::Int(i32::from(p.end))),
        ("entity", a.tag.clone()),
    ])
}
/// The waiting entity `arrival_tag` wrote.
fn arrival_from_tag(a: &Tag) -> Option<Arrival> {
    let int = |k: &str| match a.get(k) {
        Some(Tag::Int(v)) => Some(*v),
        _ => None,
    };
    let float = |k: &str| match a.get(k) {
        Some(Tag::Float(v)) => Some(f32::from_bits(*v)),
        _ => None,
    };
    let double = |k: &str| match a.get(k) {
        Some(Tag::Double(v)) => Some(f64::from_bits(*v)),
        _ => None,
    };
    let dim = Dim::from_id(&horizon_client::save::text(a.get("dimension"))?);
    let (Some(x), Some(y), Some(z), Some(tag)) = (int("x"), int("y"), int("z"), a.get("entity"))
    else {
        return None;
    };
    Some(Arrival {
        plan: Plan {
            dim,
            approximate: BlockPos::new(x, y, z),
            entry_x_axis: int("x_axis").unwrap_or(1) != 0,
            offset: Vec3::new(
                double("right").unwrap_or(0.5),
                double("up").unwrap_or(0.0),
                double("forward").unwrap_or(0.0),
            ),
            width: float("width").unwrap_or(0.25),
            height: float("height").unwrap_or(0.25),
            end: int("end").unwrap_or(0) != 0,
        },
        tag: tag.clone(),
    })
}

/// Keep an explicitly captured counter. Old archives without a positive counter
/// retain the historical fallback; their migration needs separate evidence.
fn preserve_arrival_cooldown(tag: &mut Tag) {
    if let Tag::Compound(fields) = tag
        && !matches!(fields.get(&"PortalCooldown".into()), Some(Tag::Int(delay)) if *delay > 0)
    {
        fields.insert("PortalCooldown".into(), Tag::Int(ENTITY_DELAY));
    }
}

impl Game<'_> {
    /// The world border's extent.
    pub(super) fn border_bounds(&self) -> BorderBounds {
        self.border_bounds_of(self.dim)
    }
    /// The extent of `dim`'s own border (`ServerLevel.getWorldBorder`), the level in place or a
    /// parked one.
    fn border_bounds_of(&self, dim: Dim) -> BorderBounds {
        let border = if dim == self.dim {
            &self.border
        } else {
            self.levels.parked_border(dim).unwrap_or(&self.border)
        };
        let [min_x, min_z, max_x, max_z] = border.bounds();
        BorderBounds {
            min_x,
            min_z,
            max_x,
            max_z,
        }
    }
    /// `ServerLevel.isAllowedToEnterPortal`.
    pub(super) fn may_enter(&self, target: Dim) -> bool {
        target != Dim::Nether || self.ui.rules.bool("allow_entering_nether_using_portals")
    }
    /// The plan of an entity of `width` by `height` at `at` going through the
    /// portal at `entry` (`getDimensionTransitionFromExit` and the scaled,
    /// clamped exit of `getPortalDestination`).
    pub(super) fn portal_plan(
        &self,
        entry: BlockPos,
        at: Vec3,
        width: f32,
        height: f32,
        reset_forward: bool,
    ) -> Plan {
        let target = if self.dim == Dim::Nether {
            Dim::Overworld
        } else {
            Dim::Nether
        };
        let state = self.world.block(entry.x, entry.y, entry.z);
        let b = self.res.behaviours();
        let (entry_x_axis, offset) = match b.portal_axis(state) {
            Some(x_axis) => {
                let area = largest_rectangle_around(
                    entry,
                    if x_axis { Axis::X } else { Axis::Z },
                    21,
                    Axis::Y,
                    21,
                    |p| self.world.block(p.x, p.y, p.z) == state,
                );
                let mut offset = relative_position(area, x_axis, at, width, height);
                if reset_forward {
                    offset.z = 0.0;
                }
                (x_axis, offset)
            }
            None => (true, Vec3::new(0.5, 0.0, 0.0)),
        };
        let scale = DimensionFacts::teleportation_scale(
            self.dimension_type(),
            self.dimension_type_of(target),
        );
        Plan {
            dim: target,
            // getPortalDestination clamps to the border of the level it leads to.
            approximate: self
                .border_bounds_of(target)
                .clamp(at.x * scale, at.y, at.z * scale),
            entry_x_axis,
            offset,
            width,
            height,
            end: false,
        }
    }
    /// The plan of an item or mob going through the portal block at `entry`:
    /// an End portal's fixed destination, else the Nether portal's.
    fn entity_plan(
        &self,
        entry: BlockPos,
        at: Vec3,
        width: f32,
        height: f32,
        reset_forward: bool,
    ) -> Plan {
        if !self.is_end_portal(self.world.block(entry.x, entry.y, entry.z)) {
            return self.portal_plan(entry, at, width, height, reset_forward);
        }
        let spawn = self.level.world_spawn.unwrap_or_else(|| {
            self.spawn.map_or([0, 64, 0], |s| {
                [s.x.floor() as i32, s.y.floor() as i32, s.z.floor() as i32]
            })
        });
        Plan::end_portal(
            self.dim,
            BlockPos::new(spawn[0], spawn[1], spawn[2]),
            width,
            height,
        )
    }
    fn is_end_portal(&self, state: horizon_world::registry::BlockStateId) -> bool {
        &*self.res.facts().get(state).name == "minecraft:end_portal"
    }
    /// Where an item or mob through an End portal lands once `plan`'s level
    /// has loaded around it: the bottom centre of the End's spawn point,
    /// above the platform built below it (`EndPlatformFeature.createEndPlatform`);
    /// out of the End, the top of the shared spawn's column
    /// (`Entity.adjustSpawnLocation`).
    fn end_portal_exit(&mut self, plan: &Plan) -> Vec3 {
        let p = plan.approximate;
        let centre = Vec3::new(f64::from(p.x) + 0.5, f64::from(p.y), f64::from(p.z) + 0.5);
        if plan.dim == Dim::End {
            // In the End the arrival only builds the platform: no spawn search.
            if let Err(e) = self.end_arrival(centre, &mut None) {
                log::error!("End platform: {e}");
            }
            return centre;
        }
        let y = self.motion_top(p.x, p.z);
        Vec3::new(centre.x, f64::from(y), centre.z)
    }
    /// The nether portal blocks of the chunks within `radius` of `center`
    /// (the points of interest `PortalForcer` searches): loaded chunks as
    /// they are, the others as saved; chunks never generated have none.
    fn portal_blocks(&self, center: BlockPos, radius: i32) -> Vec<BlockPos> {
        let b = self.res.behaviours();
        let chunk_radius = radius.div_euclid(16) + 1;
        let (cx, cz) = (center.x >> 4, center.z >> 4);
        let mut out = Vec::new();
        for z in cz - chunk_radius..=cz + chunk_radius {
            for x in cx - chunk_radius..=cx + chunk_radius {
                let saved;
                let chunk = match self.world.chunks.get(&(x, z)) {
                    Some(c) => &**c,
                    None => {
                        match self
                            .save
                            .load(x, z, self.pack.assets.registry, world_biomes!(self))
                        {
                            Some(c) => {
                                saved = c;
                                &saved
                            }
                            None => continue,
                        }
                    }
                };
                for (s, section) in chunk.sections.iter().enumerate() {
                    if !section.blocks.maybe_has(|v| {
                        b.portal_axis(horizon_world::registry::BlockStateId(v))
                            .is_some()
                    }) {
                        continue;
                    }
                    let base = self.world.bottom_y() + 16 * s as i32;
                    for y in base..base + 16 {
                        for lz in 0..16 {
                            for lx in 0..16 {
                                let (wx, wz) = (x * 16 + lx, z * 16 + lz);
                                if b.portal_axis(chunk.block(wx, y, wz)).is_some() {
                                    out.push(BlockPos::new(wx, y, wz));
                                }
                            }
                        }
                    }
                }
            }
        }
        out
    }
    /// `PortalForcer.findClosestPortalPosition` and the rectangle around it,
    /// for `plan` in the loaded dimension; builds nothing.
    pub(super) fn find_exit_portal(&mut self, plan: &Plan) -> Option<(BlockPos, FoundRectangle)> {
        let to_nether = plan.dim == Dim::Nether;
        let radius = if to_nether { 16 } else { 128 };
        let border = self.border_bounds();
        let portals = self.portal_blocks(plan.approximate, radius);
        let (min_section, max_section) = if to_nether { (0, 15) } else { (-4, 19) };
        let found = closest_portal(
            &portals,
            plan.approximate,
            to_nether,
            &border,
            min_section,
            max_section,
        )?;
        let state = self.world.block(found.x, found.y, found.z);
        let x_axis = self.res.behaviours().portal_axis(state).unwrap_or(true);
        let rectangle = largest_rectangle_around(
            found,
            if x_axis { Axis::X } else { Axis::Z },
            21,
            Axis::Y,
            21,
            |p| self.world.block(p.x, p.y, p.z) == state,
        );
        Some((found, rectangle))
    }
    /// The exit portal of `plan` in the loaded dimension: the closest one
    /// within reach, else one built by `PortalForcer.createPortal`.
    pub(super) fn nether_exit_portal(&mut self, plan: &Plan) -> Option<FoundRectangle> {
        let to_nether = plan.dim == Dim::Nether;
        let border = self.border_bounds();
        if let Some((found, rectangle)) = self.find_exit_portal(plan) {
            // NetherPortalBlock.getExitPortal: the ticket goes on the chunk
            // of the portal found.
            self.place_portal_ticket((found.x >> 4, found.z >> 4));
            return Some(rectangle);
        }
        let logical = if to_nether { 128 } else { 384 };
        let (approximate, x_axis) = (plan.approximate, plan.entry_x_axis);
        let created = self.with_level(|b, level, ticks| {
            b.create_portal(level, ticks, approximate, x_axis, &border, logical)
        });
        let _ = self.process_level_events();
        if let Some(exit) = created {
            let dim = if to_nether {
                "minecraft:the_nether"
            } else {
                "minecraft:overworld"
            };
            let corner = exit.min_corner;
            self.native_portal_create(dim, [corner.x, corner.y, corner.z]);
            // A portal built for the entity: the ticket goes on the chunk it
            // arrives in (`TeleportTransition.PLACE_PORTAL_TICKET`).
            let (at, _) = self.arrival(plan, exit);
            self.place_portal_ticket(Self::chunk_of(at));
        }
        created
    }
    /// `ChunkPos.containing`: the chunk a position is in.
    pub(super) fn chunk_of(at: Vec3) -> (i32, i32) {
        ((at.x.floor() as i32) >> 4, (at.z.floor() as i32) >> 4)
    }
    /// `Entity.placePortalTicket`: a `TicketType.PORTAL` ticket of radius 3
    /// on `chunk` in the level in place, which keeps the area running for 300
    /// ticks with no player in it (the entity that just came through ticks
    /// there, and goes back through the portal when its cooldown ends).
    /// Only a server holds levels with tickets of their own.
    pub(super) fn place_portal_ticket(&mut self, chunk: (i32, i32)) {
        if !self.options.execution_role.is_client() {
            self.tickets.add_portal(chunk);
        }
    }
    /// `NetherPortalBlock.createDimensionTransition`: the arrival position
    /// and the yaw turned by.
    pub(super) fn arrival(&self, plan: &Plan, exit: FoundRectangle) -> (Vec3, f32) {
        let b = self.res.behaviours();
        let corner = exit.min_corner;
        let exit_x_axis = b
            .portal_axis(self.world.block(corner.x, corner.y, corner.z))
            .unwrap_or(true);
        let (bottom, rotation) = exit_position(
            exit,
            exit_x_axis,
            plan.entry_x_axis,
            plan.offset,
            plan.width,
            plan.height,
        );
        let border = self.border_bounds();
        let at = collision_free_position(bottom, plan.width, plan.height, &border, |p| {
            b.collision_shape(self.world.block(p.x, p.y, p.z))
                .map(|s| s.moved(Vec3::new(f64::from(p.x), f64::from(p.y), f64::from(p.z))))
        });
        (at, rotation)
    }

    // ---- the player ----

    /// Whether the inventory key opens nothing: vanilla opens the screen,
    /// then `LocalPlayer.handlePortalTransitionEffect` closes it in the same
    /// tick while the player stands in a nether portal, so it is never drawn.
    /// The fading effect after leaving the portal closes nothing.
    pub(super) fn in_portal_transition(&self) -> bool {
        self.portals.client_was_inside
    }

    /// The same on a server's client: `Entity.canUsePortal` (alive, not a
    /// passenger), then only the client's `setAsInsidePortal`, which drives
    /// the confusion effect (`client_portal_effect`). On a server's client
    /// `portals.player` holds only the LocalPlayer's `portalCooldown`.
    pub(super) fn client_inside_nether_portal(&mut self) {
        let alive = self
            .player
            .as_ref()
            .is_some_and(|p| !p.health.dead_or_dying());
        if !alive || self.riding.is_some() {
            return;
        }
        if self.portals.player.cooldown > 0 {
            self.portals.player.cooldown = PLAYER_DELAY;
        } else {
            self.portals.client_inside = true;
        }
    }
    /// Where to wait for the destination of `plan` to load, once its level
    /// is open: the portal its points of interest lead to, or the
    /// approximate exit.
    pub(super) fn arrival_centre(&self, plan: &Plan) -> Vec3 {
        let to_nether = plan.dim == Dim::Nether;
        let portals = self.portal_blocks(plan.approximate, if to_nether { 16 } else { 128 });
        let (min_section, max_section) = if to_nether { (0, 15) } else { (-4, 19) };
        let border = self.border_bounds();
        let found = closest_portal(
            &portals,
            plan.approximate,
            to_nether,
            &border,
            min_section,
            max_section,
        );
        let centre = found.unwrap_or(plan.approximate);
        Vec3::new(
            f64::from(centre.x) + 0.5,
            f64::from(centre.y),
            f64::from(centre.z) + 0.5,
        )
    }
    /// `LocalPlayer.handlePortalTransitionEffect`: whether the confusion
    /// effect grows this tick. Entering a portal closes screens other than
    /// the chat and death screens and plays `block.portal.trigger` when the
    /// effect starts from nothing.
    pub(super) fn client_portal_effect(&mut self) -> bool {
        let inside = std::mem::take(&mut self.portals.client_inside);
        self.portals.client_was_inside = inside;
        if !inside {
            return false;
        }
        match self.screen {
            Screen::Playing | Screen::Chat | Screen::Dead => {}
            Screen::Inventory | Screen::Creative => {
                self.close_container();
                self.carried = None;
                self.screen = Screen::Playing;
                self.capture(true);
            }
            _ => {
                self.screen = Screen::Playing;
                self.capture(true);
            }
        }
        if self.screen_fx.portal == 0.0 {
            let pitch = self.world.random.next_float() * 0.4 + 0.8;
            self.audio
                .play("minecraft:block.portal.trigger", None, 0.25, pitch);
        }
        true
    }

    // ---- other entities ----

    /// Positions before the entities move this tick, and `Entity.handlePortal`
    /// at the start of their tick: an item or mob that stayed in a portal
    /// goes through (transition time 0) and waits for its dimension.
    pub(super) fn handle_entity_portals(&mut self) {
        self.handle_portals(false, false);
    }
    /// `Entity.handlePortal` of the boats and minecarts, which `tick_boats`
    /// and `tick_carts` move: it runs before them, from the position they
    /// ended the last tick at, as the start of a vehicle's own tick does.
    pub(super) fn handle_vehicle_portals(&mut self) {
        self.handle_portals(true, false);
    }
    fn handle_portals(&mut self, vehicles: bool, after_move: bool) {
        if self.remote.is_some() {
            return;
        }
        self.load_portal_arrivals();
        let mut leaving: Vec<(u32, BlockPos)> = Vec::new();
        // Positions before the entities move, and (only for an entity in a
        // portal, or cooling down from one, which has a state here) whether it
        // can use one: asking every entity on every tick is not worth it. The
        // vehicles' pass opens the tick and the other kinds' pass follows, so
        // each walk takes only its own group.
        let mut before = std::mem::take(&mut self.portals.before);
        if vehicles {
            before.clear();
        }
        let mut ids: Vec<(u32, bool, i32)> = Vec::new();
        // Portal handling runs only in entity-ticking chunks; retain frozen
        // states while their entities are outside the simulation distance.
        let entity_ticking = self.entity_ticking_area();
        let tracked = &self.portals.entities;
        let mut present = std::collections::HashSet::new();
        for_each_entity!(entity_lists_of!(self), entity => {
            let can_sweep = entity.sweeps_portals();
            let has_portal_state = tracked.contains_key(&entity.id());
            if has_portal_state {
                present.insert(entity.id());
            }
            // Entity.processPortalCooldown still runs for entities this
            // portal pass cannot send, such as an ender pearl.
            if entity.is_vehicle() == vehicles
                && entity.portal_after_move() == after_move
                && (can_sweep || has_portal_state)
            {
                let at = entity.position();
                if !entity_ticking(at.x.floor() as i32, at.z.floor() as i32) {
                    continue;
                }
                if can_sweep && entity.portal_from().is_none() {
                    before.insert(entity.id(), entity.position());
                }
                if has_portal_state {
                    ids.push((
                        entity.id(),
                        can_sweep && entity.can_use_portal(self),
                        entity.dimension_changing_delay(),
                    ));
                }
            }
        });
        self.portals.before = before;
        self.portals.entities.retain(|id, _| present.contains(id));
        for (id, allowed, delay) in ids {
            let Some(state) = self.portals.entities.get_mut(&id) else {
                continue;
            };
            if let Some(entry) = state.handle(allowed, 0, delay) {
                leaving.push((id, entry));
            }
            if state.cooldown == 0 && state.process.is_none() {
                self.portals.entities.remove(&id);
            }
        }
        for (id, entry) in leaving {
            // `Entity.teleportCrossDimension` carries the passengers: a
            // vehicle server players ride takes them along.
            if self.guest_rides(id) {
                if let Some(c) = self.carts.iter().find(|c| c.id == id) {
                    self.portals.carried_motion.insert(id, c.velocity);
                    self.portals.carried_at.insert(id, c.position);
                } else if let Some(b) = self.boats.iter().find(|b| b.id == id) {
                    self.portals.carried_at.insert(id, b.position);
                } else if let Some(m) = self.mobs.iter().find(|m| m.id == id) {
                    self.portals.carried_at.insert(id, m.position);
                }
                self.portals.carrying.push((self.dim, id, entry));
            } else {
                self.send_through_portal(id, entry);
            }
        }
    }
    /// `Entity.load`: the saved `PortalCooldown` (an entity that just came
    /// through stands in its exit portal for 300 ticks without going back).
    pub(super) fn restore_portal_cooldown(&mut self, id: u32, tag: &Tag) {
        if let Some(cooldown) = tag
            .get("PortalCooldown")
            .and_then(horizon_client::nbt_path::int_value)
            && cooldown != 0
        {
            self.portals.entities.insert(
                id,
                PortalState {
                    cooldown,
                    process: None,
                },
            );
        }
    }
    /// The live entity's `getDimensionChangingDelay`; unknown ids use Entity's default.
    pub(super) fn entity_portal_delay(&self, id: u32) -> i32 {
        self.entity_handles()
            .find_map(|handle| {
                with_entity!(self, handle, entity => {
                    (entity.id() == id).then(|| entity.dimension_changing_delay())
                })
            })
            .unwrap_or(ENTITY_DELAY)
    }
    /// `Entity.saveWithoutId` writes the live `PortalCooldown`: the countdown
    /// `Entity.processPortalCooldown` ticks down, not the value the entity was
    /// loaded with. Every entity that can stand in a portal takes it from the
    /// portal state, as the projectile writer does. An entity the portal
    /// state does not know is not on a cooldown, and writes 0.
    pub(super) fn save_portal_cooldown(&self, id: u32, tag: &mut Tag) {
        horizon_client::nether_portal::put_portal_cooldown(
            tag,
            horizon_client::nether_portal::portal_cooldown(&self.portals.entities, id),
        );
    }
    /// `Entity.saveWithoutId`: the modeled non-arrow projectile codecs
    /// share the live cooldown between command reads and chunk writes.
    pub(super) fn save_projectile_portal_cooldown(
        &self,
        projectile: &horizon_client::projectiles::Projectile,
        tag: &mut Tag,
    ) {
        if projectile.arrow.is_none()
            && let Tag::Compound(fields) = tag
        {
            let cooldown = self
                .portals
                .entities
                .get(&projectile.id)
                .map_or(0, |portal| portal.cooldown);
            fields.insert("PortalCooldown".into(), Tag::Int(cooldown));
        }
    }
    /// `Entity.setAsInsidePortal` on an entity that is still on its portal
    /// cooldown, whatever the portal: the cooldown restarts and it stays.
    pub(super) fn restart_portal_cooldown(&mut self, id: u32) -> bool {
        let delay = self.entity_portal_delay(id);
        match self.portals.entities.get_mut(&id) {
            Some(state) if state.cooldown > 0 => {
                state.cooldown = delay;
                true
            }
            _ => false,
        }
    }
    /// `Entity.setPortalCooldown` on an entity that has just spawned in a
    /// portal: it waits for its entity-specific cooldown without going through.
    pub(super) fn set_portal_cooldown(&mut self, id: u32) {
        let delay = self.entity_portal_delay(id);
        self.portals.entities.insert(
            id,
            PortalState {
                cooldown: delay,
                process: None,
            },
        );
    }
    /// `canUsePortal(false)` of a mob: what the mob itself decides
    /// (`mobs::mob_can_use_portal`), riding nothing, not asleep, and not
    /// carrying the players this side does.
    pub(super) fn mob_can_use_portal(&self, m: &horizon_client::mobs::Mob) -> bool {
        horizon_client::mobs::mob_can_use_portal(m)
            && m.extra.mount.is_none()
            && self.riding != Some(m.id)
            && (!self.guest_rides(m.id) || self.carries_riders())
    }
    /// `Entity.canUsePortal(false)` for a primed TNT: it carries no mob.
    pub(super) fn tnt_can_use_portal(&self, id: u32) -> bool {
        // Id 0: not tracked yet, nothing to key its portal state by.
        id != 0 && !self.mobs.iter().any(|m| m.extra.mount == Some(id))
    }
    /// `canUsePortal(false)` of a boat or minecart: it is nobody's passenger;
    /// the mobs riding it, and on a server the players, cross with it.
    pub(super) fn vehicle_can_use_portal(&self, id: u32) -> bool {
        // The window's player is moved by its own trip, not by the vehicle's.
        if self.riding == Some(id) {
            return false;
        }
        // Mob riders always go along; server players only on a server.
        !self.ridden_by_guest(id) || self.carries_riders()
    }
    /// Whether a vehicle can take the server players riding it through a
    /// portal: only a server moves players between its levels. Elsewhere a
    /// ridden vehicle stays, rather than leave its rider behind.
    fn carries_riders(&self) -> bool {
        self.options.dedicated.is_some()
    }
    /// The vehicles that went through a portal in `dim` with server players
    /// on them.
    pub(super) fn take_carrying(&mut self, dim: Dim) -> Vec<(u32, BlockPos)> {
        let (here, other) = std::mem::take(&mut self.portals.carrying)
            .into_iter()
            .partition(|(d, ..)| *d == dim);
        self.portals.carrying = other;
        here.into_iter().map(|(_, id, entry)| (id, entry)).collect()
    }
    /// The plan of vehicle `id` through the portal at `entry`: from its own
    /// position and size (`getPortalDestination(level, vehicle)`). A boat or
    /// minecart, like a mob, lands in the middle of the exit portal's depth
    /// (`AbstractBoat` and `AbstractMinecart.getRelativePortalPosition`).
    pub(super) fn vehicle_plan(&self, id: u32, entry: BlockPos) -> Option<Plan> {
        let at = |own: Vec3| self.portals.carried_at.get(&id).copied().unwrap_or(own);
        if let Some(m) = self.mobs.iter().find(|m| m.id == id) {
            return Some(self.entity_plan(entry, at(m.position), m.width(), m.height(), true));
        }
        if let Some(b) = self.boats.iter().find(|b| b.id == id) {
            let (width, height) = (super::boats::WIDTH as f32, super::boats::HEIGHT as f32);
            return Some(self.entity_plan(entry, at(b.position), width, height, true));
        }
        let c = self.carts.iter().find(|c| c.id == id)?;
        let (width, height) = (
            super::minecarts::WIDTH as f32,
            super::minecarts::HEIGHT as f32,
        );
        Some(self.entity_plan(entry, at(c.position), width, height, true))
    }
    /// `NetherPortalBlock.entityInside` and `EndPortalBlock.entityInside`
    /// for the items, mobs, empty boats and minecarts, and arrows that moved
    /// this tick: the portal blocks their box swept through (an End portal's
    /// transition time is 0 too).
    pub(super) fn find_entity_portals(&mut self) {
        self.find_portals(None);
        // AbstractArrow, ThrowableProjectile and AbstractHurtingProjectile apply
        // block effects before calling Entity.tick, so their refreshed cooldown
        // is decremented (or their new portal handled) in this same tick.
        self.handle_portals(false, true);
    }
    /// The same for the vehicles server players ride, once `tick_guests` has
    /// moved them: a ridden vehicle moves with its rider, after the other
    /// entities were swept, so without this pass its portal is found a tick
    /// late, from where it has already moved on to.
    pub(super) fn find_ridden_vehicle_portals(&mut self, ridden: &[u32]) {
        if !ridden.is_empty() {
            self.find_portals(Some(ridden));
        }
    }
    fn find_portals(&mut self, only: Option<&[u32]>) {
        if self.remote.is_some() {
            return;
        }
        let ridden = if only.is_none() {
            self.guest_vehicles()
        } else {
            Vec::new()
        };
        // A trident or rocket in flight gets its id here, before the walk.
        for t in self
            .tridents
            .iter_mut()
            .filter(|t| only.is_none() && (!t.in_ground || t.previous != t.position))
        {
            super::lan_entities::assign_entity_id(&mut self.next_entity, &mut t.id);
        }
        for r in self
            .rockets
            .iter_mut()
            .filter(|r| only.is_none() && r.attached_to.is_none() && !r.attached)
        {
            super::lan_entities::assign_entity_id(&mut self.next_entity, &mut r.id);
        }
        let mut inside: Vec<(u32, BlockPos, i32)> = Vec::new();
        let mut portal_sections = std::mem::take(&mut self.portals.sections);
        // No loaded section has a portal state in its palette (nearly always): no
        // sweep below can meet a portal block, so the walk would find nothing.
        if !self.level_may_hold_portal(&mut portal_sections) {
            self.portals.sections = portal_sections;
            return;
        }
        // Only the entities that tick (in entity-ticking chunks) move, so only
        // they can have swept into a portal.
        let entity_ticking = self.entity_ticking_area();
        // The first portal block each entity's box swept through since the
        // start of its tick (a stuck arrow that did not move enters none).
        for_each_entity!(entity_lists_of!(self), entity => {
            if !entity.sweeps_portals() {
                continue;
            }
            let (id, to) = (entity.id(), entity.position());
            if !entity_ticking(to.x.floor() as i32, to.z.floor() as i32) {
                continue;
            }
            if !entity.portal_sweeps() || !entity.can_use_portal(self) {
                continue;
            }
            let swept_later = match only {
                Some(ids) => !ids.contains(&id),
                None => ridden.contains(&id),
            };
            if swept_later {
                continue;
            }
            let from = entity
                .portal_from()
                .unwrap_or_else(|| self.portals.before.get(&id).copied().unwrap_or(to));
            let Some((width, height)) = entity.portal_size() else {
                continue;
            };
            if let Some(pos) = self.swept_portal(from, to, width, height, &mut portal_sections) {
                inside.push((id, pos, entity.dimension_changing_delay()));
            }
        });
        self.portals.sections = portal_sections;
        for (id, pos, delay) in inside {
            self.portals
                .entities
                .entry(id)
                .or_default()
                .set_inside(pos, delay);
        }
    }
    /// The first nether or End portal block an entity box swept through
    /// (`Entity.checkInsideBlocks`, whose first portal sets the entry).
    fn swept_portal(
        &self,
        from: Vec3,
        to: Vec3,
        width: f32,
        height: f32,
        portal_sections: &mut horizon_client::portal_sections::PortalSections,
    ) -> Option<BlockPos> {
        let half = f64::from(width) / 2.0;
        let bounds = horizon_core::geometry::Aabb::new(
            Vec3::new(to.x - half, to.y, to.z - half),
            Vec3::new(to.x + half, to.y + f64::from(height), to.z + half),
        )
        .deflate(Vec3::new(
            f64::from(1.0e-5_f32),
            f64::from(1.0e-5_f32),
            f64::from(1.0e-5_f32),
        ));
        // Every block the sweep visits lies in the box covering both ends.
        let reach = bounds
            .union_bounds(bounds.move_by(from.subtract(to)))
            .inflate(Vec3::new(1.0, 1.0, 1.0));
        if !self.may_hold_portal(reach, portal_sections) {
            return None;
        }
        let mut found = None;
        let _ = horizon_core::geometry::sweep_intersected_blocks(
            from,
            to,
            bounds,
            1_000_000,
            |p, _| {
                let state = self.world.block(p.x, p.y, p.z);
                if self.res.behaviours().portal_axis(state).is_some()
                    || (self.is_end_portal(state) && touches_end_portal(from, to, width, height, p))
                {
                    found = Some(p);
                    return false;
                }
                true
            },
        );
        found
    }
    /// Whether a section `reach` touches has a nether or End portal state in
    /// its palette: without one, no block there is a portal. Palette answers are
    /// kept per chunk across ticks (`portal_sections`); a box over many
    /// sections is not checked.
    fn may_hold_portal(
        &self,
        reach: horizon_core::geometry::Aabb,
        sections: &mut horizon_client::portal_sections::PortalSections,
    ) -> bool {
        use horizon_client::section_mask::SectionMask;
        let (min, max) = (reach.min_position(), reach.max_position());
        if !min.is_finite() || !max.is_finite() {
            return true;
        }
        let (min, max) = (BlockPos::containing(min), BlockPos::containing(max));
        let top = self.world.layout().sections() as i32 - 1;
        let (x0, x1) = (min.x >> 4, max.x >> 4);
        let (z0, z1) = (min.z >> 4, max.z >> 4);
        let (y0, y1) = (
            ((min.y - self.world.bottom_y()) >> 4).clamp(0, top),
            ((max.y - self.world.bottom_y()) >> 4).clamp(0, top),
        );
        let count = (i64::from(x1) - i64::from(x0) + 1)
            * (i64::from(y1) - i64::from(y0) + 1)
            * (i64::from(z1) - i64::from(z0) + 1);
        if count > 64 {
            return true;
        }
        let y_mask = SectionMask::below(y1 as usize + 1).from(y0 as usize);
        let chunks = &self.world.chunks;
        (x0..=x1).any(|cx| {
            (z0..=z1).any(|cz| {
                chunks.get(&(cx, cz)).is_some_and(|chunk| {
                    let portals = sections.get((cx, cz), chunk, |v| self.is_portal_state(v));
                    !(portals & y_mask).is_empty()
                })
            })
        })
    }
    /// [`Self::may_hold_portal`] for the whole level: whether any loaded section has
    /// a nether or End portal state in its palette.
    fn level_may_hold_portal(
        &self,
        sections: &mut horizon_client::portal_sections::PortalSections,
    ) -> bool {
        self.world.chunks.iter().any(|(&key, chunk)| {
            !sections
                .get(key, chunk, |v| self.is_portal_state(v))
                .is_empty()
        })
    }
    /// A nether or End portal state, by its id in a palette.
    fn is_portal_state(&self, v: u32) -> bool {
        let state = horizon_world::registry::BlockStateId(v);
        self.res.behaviours().portal_axis(state).is_some() || self.is_end_portal(state)
    }
    /// `Entity.teleportCrossDimension` away from the loaded dimension: the
    /// entity is saved and removed, and waits for its dimension to load.
    fn send_through_portal(&mut self, id: u32, entry: BlockPos) {
        let arrivals = self.portals.arrivals.len();
        let delay = self.entity_portal_delay(id);
        if let Some(i) = self.dropped.iter().position(|e| e.id == id) {
            let e = &self.dropped[i];
            let plan = self.entity_plan(entry, e.position, 0.25, 0.25, false);
            if !self.may_enter(plan.dim) {
                return;
            }
            let uuid = self.uuid_for(id);
            let tag = horizon_client::entity_nbt::item_entity_to_nbt_with_items(
                &self.dropped[i],
                uuid,
                self.persist_raw(id),
                self.items(),
            );
            self.remove_dropped(i);
            self.detach_entity(id);
            self.portals.arrivals.push(Arrival { plan, tag });
            self.portals.dirty = true;
        } else if let Some(i) = self.tnts.iter().position(|t| t.id == id) {
            let t = &self.tnts[i];
            let plan = self.entity_plan(entry, t.position, 0.98, 0.98, false);
            if !self.may_enter(plan.dim) {
                return;
            }
            let tag = self.tnt_nbt(i);
            self.tnts.remove(i);
            self.forget_entity(id);
            self.portals.arrivals.push(Arrival { plan, tag });
            self.portals.dirty = true;
        } else if let Some(i) = self.tridents.iter().position(|t| t.id == id) {
            let plan = self.entity_plan(entry, self.tridents[i].position, 0.5, 0.5, false);
            if !self.may_enter(plan.dim) {
                return;
            }
            let uuid = self.uuid_for(id);
            let tag =
                self.tridents[i].to_nbt(uuid, self.pack.assets.registry, self.persist_raw(id));
            self.tridents.remove(i);
            self.forget_entity(id);
            self.portals.arrivals.push(Arrival { plan, tag });
            self.portals.dirty = true;
        } else if let Some(i) = self.rockets.iter().position(|r| r.id == id) {
            let plan = self.entity_plan(entry, self.rockets[i].position, 0.25, 0.25, false);
            if !self.may_enter(plan.dim) {
                return;
            }
            let uuid = self.uuid_for(id);
            let tag = self.rockets[i].to_nbt(uuid, self.level.player_uuid, self.persist_raw(id));
            self.rockets.remove(i);
            self.forget_entity(id);
            self.portals.arrivals.push(Arrival { plan, tag });
            self.portals.dirty = true;
        } else if let Some(i) = self.orbs.iter().position(|o| o.id == id) {
            let plan = self.entity_plan(entry, self.orbs[i].position, 0.5, 0.5, false);
            if !self.may_enter(plan.dim) {
                return;
            }
            let uuid = self.uuid_for(id);
            let tag = horizon_client::entity_nbt::orb_to_nbt(&self.orbs[i], uuid);
            self.orbs.remove(i);
            self.forget_entity(id);
            self.portals.arrivals.push(Arrival { plan, tag });
            self.portals.dirty = true;
        } else if let Some(i) = self.mobs.iter().position(|m| m.id == id) {
            let m = &self.mobs[i];
            let plan = self.entity_plan(entry, m.position, m.width(), m.height(), true);
            if !self.may_enter(plan.dim) {
                return;
            }
            let mut tag = self.mob_tree_nbt(i);
            // Entity.removeAfterChangingDimensions drops the leash.
            if let Tag::Compound(c) = &mut tag {
                c.remove(&"leash".into());
            }
            let mut gone = vec![id];
            gone.extend(self.remove_mob_riders(id));
            self.mobs.retain(|m| m.id != id);
            for g in gone {
                self.detach_entity(g);
            }
            self.portals.arrivals.push(Arrival { plan, tag });
            self.portals.dirty = true;
        } else if let Some(i) = self.boats.iter().position(|b| b.id == id) {
            let b = &self.boats[i];
            let (width, height) = (super::boats::WIDTH as f32, super::boats::HEIGHT as f32);
            let plan = self.entity_plan(entry, b.position, width, height, true);
            if !self.may_enter(plan.dim) {
                return;
            }
            self.release_vehicle_container(id);
            let uuid = self.uuid_for(id);
            let b = &self.boats[i];
            let mut tag = b.to_nbt(uuid, self.persist_raw(id));
            // The mobs riding it cross with it and mount it again.
            self.put_mob_passengers(&mut tag, id);
            for rider in self.remove_mob_riders(id) {
                self.detach_entity(rider);
            }
            self.boats.remove(i);
            self.detach_entity(id);
            self.portals.arrivals.push(Arrival { plan, tag });
            self.portals.dirty = true;
        } else if let Some(i) = self.carts.iter().position(|c| c.id == id) {
            let c = &self.carts[i];
            let (width, height) = (
                super::minecarts::WIDTH as f32,
                super::minecarts::HEIGHT as f32,
            );
            let plan = self.entity_plan(entry, c.position, width, height, true);
            if !self.may_enter(plan.dim) {
                return;
            }
            self.release_vehicle_container(id);
            let uuid = self.uuid_for(id);
            let mut tag = self.carts[i].to_nbt(uuid, self.persist_raw(id));
            // `PositionMoveRotation.of(this)` takes the cart's `getKnownMovement`:
            // `OldMinecartBehavior` clamps it to 0.4 either way along x and z.
            let m = horizon_client::entity_nbt::motion(&tag);
            if let Tag::Compound(c) = &mut tag {
                c.insert(
                    "Motion".into(),
                    doubles([m.x.clamp(-0.4, 0.4), m.y, m.z.clamp(-0.4, 0.4)]),
                );
            }
            // The mobs riding it cross with it and mount it again.
            self.put_mob_passengers(&mut tag, id);
            for rider in self.remove_mob_riders(id) {
                self.detach_entity(rider);
            }
            self.carts.remove(i);
            self.detach_entity(id);
            self.portals.arrivals.push(Arrival { plan, tag });
            self.portals.dirty = true;
        } else if let Some(i) = self
            .creatures
            .projectiles
            .iter()
            .position(|p| p.id == id && (is_arrow(p) || is_portal_projectile(p)))
        {
            let p = &self.creatures.projectiles[i];
            let (width, height) = projectile_box(p);
            let plan = self.entity_plan(entry, p.position, width, height, false);
            if !self.may_enter(plan.dim) {
                return;
            }
            let uuid = self.uuid_for(id);
            let p = &self.creatures.projectiles[i];
            let in_block = p
                .arrow
                .as_ref()
                .and_then(|a| a.last_state)
                .map(|s| self.state_to_nbt(s));
            let raw = self.persist_raw(id);
            let Some(tag) =
                horizon_client::entity_nbt::arrow_to_nbt(p, uuid, p.owner_uuid, in_block, raw)
                    .or_else(|| {
                        horizon_client::entity_nbt::fireball_to_nbt(p, uuid, p.owner_uuid, raw)
                    })
                    .or_else(|| {
                        horizon_client::entity_nbt::throwable_to_nbt(p, uuid, p.owner_uuid, raw)
                    })
            else {
                return;
            };
            self.creatures.projectiles.remove(i);
            self.forget_entity(id);
            self.portals.arrivals.push(Arrival { plan, tag });
            self.portals.dirty = true;
        }
        if self.portals.arrivals.len() > arrivals {
            if let Some(arrival) = self.portals.arrivals.last_mut()
                && let Tag::Compound(tag) = &mut arrival.tag
            {
                tag.insert("PortalCooldown".into(), Tag::Int(delay));
            }
            self.native_entity_portal(self.dim.id(), id, [entry.x, entry.y, entry.z]);
        }
    }
    /// Removes the mobs riding `vehicle`, and those riding them, from the
    /// level: `Entity.teleportCrossDimension` takes the whole tree with its
    /// root. Their ids.
    fn remove_mob_riders(&mut self, vehicle: u32) -> Vec<u32> {
        let mut tree = vec![vehicle];
        let mut k = 0;
        while k < tree.len() {
            let v = tree[k];
            tree.extend(
                self.mobs
                    .iter()
                    .filter(|m| m.extra.mount == Some(v))
                    .map(|m| m.id),
            );
            k += 1;
        }
        tree.remove(0);
        self.mobs.retain(|m| !tree.contains(&m.id));
        tree
    }
    /// The chunks of the arrivals still waiting to land in `dim`, for the
    /// streaming `extra` areas that light them: vanilla's portal ticket
    /// (`Entity.teleport` adds `TicketType.PORTAL` at the destination) keeps
    /// the area loaded without a player.
    pub(super) fn arrival_chunks(&self, dim: Dim) -> Vec<(i32, i32)> {
        self.portals
            .arrivals
            .iter()
            .filter(|a| a.plan.dim == dim)
            .map(|a| (a.plan.approximate.x >> 4, a.plan.approximate.z >> 4))
            .collect()
    }
    /// `ServerLevel.addDuringTeleport`: the entities that arrive in the level in
    /// place join it now, before its entities tick, so a vehicle that crossed
    /// from an earlier level this server tick moves in its new level the same
    /// tick, and one that crossed from a later level stands there until the
    /// next. Only the portal arrivals join; the chunks' entities wait.
    pub(super) fn land_portal_arrivals(&mut self) {
        if self.remote.is_some() || self.portals.arrivals.is_empty() {
            return;
        }
        let waiting = std::mem::take(&mut self.world.loaded_entities);
        self.resolve_arrivals();
        self.spawn_loaded_entities();
        self.world.loaded_entities.extend(waiting);
    }
    /// Entities waiting for the loaded dimension whose exit area is loaded:
    /// their exit portal (found or built) or End destination, position,
    /// turned yaw and motion, and portal cooldown; they join the world with the chunk entities.
    pub(super) fn resolve_arrivals(&mut self) {
        if self.remote.is_some() {
            return;
        }
        let mut i = 0;
        while i < self.portals.arrivals.len() {
            let plan = self.portals.arrivals[i].plan;
            let (cx, cz) = (plan.approximate.x >> 4, plan.approximate.z >> 4);
            let ready = plan.dim == self.dim
                && (-1..=1).all(|dz| (-1..=1).all(|dx| self.streaming.is_lit(cx + dx, cz + dz)));
            if !ready {
                i += 1;
                continue;
            }
            let Arrival { plan, mut tag } = self.portals.arrivals.remove(i);
            self.portals.dirty = true;
            let motion = horizon_client::entity_nbt::motion(&tag);
            let [yaw, pitch] = horizon_client::entity_nbt::rotation(&tag);
            let was_yaw = yaw;
            let (at, motion, [yaw, pitch]) = if plan.end {
                // Relative.DELTA: the motion is kept.
                let at = self.end_portal_exit(&plan);
                // EndPortalBlock.getPortalDestination: PLACE_PORTAL_TICKET.
                self.place_portal_ticket(Self::chunk_of(at));
                let into_end = plan.dim == Dim::End;
                let turn =
                    end_portal_rotation(into_end, [yaw, pitch], self.level.world_spawn_angle);
                (at, motion, turn)
            } else {
                let Some(exit) = self.nether_exit_portal(&plan) else {
                    continue;
                };
                let (at, rotation) = self.arrival(&plan, exit);
                (at, rotate_y(motion, -rotation), [yaw + rotation, pitch])
            };
            let yaw_turn = yaw - was_yaw;
            if let Some(was) = horizon_client::entity_nbt::position(&tag) {
                move_passengers(&mut tag, at.subtract(was), yaw_turn);
            }
            if let Tag::Compound(c) = &mut tag {
                c.insert("Pos".into(), doubles([at.x, at.y, at.z]));
                c.insert("Motion".into(), doubles([motion.x, motion.y, motion.z]));
                c.insert(
                    "Rotation".into(),
                    Tag::List(vec![Tag::Float(yaw.to_bits()), Tag::Float(pitch.to_bits())]),
                );
            }
            preserve_arrival_cooldown(&mut tag);
            // ServerLevel.addDuringTeleport: a fresh add, not loaded from disk.
            super::persist::mark_fresh(&mut tag);
            let key = ((at.x.floor() as i32) >> 4, (at.z.floor() as i32) >> 4);
            match self
                .world
                .loaded_entities
                .iter_mut()
                .find(|(k, _)| *k == key)
            {
                Some((_, tags)) => tags.push(tag),
                None => self.world.loaded_entities.push((key, vec![tag])),
            }
        }
    }

    /// `FallingBlockEntity.tick`: block effects mark its portal, then
    /// `Entity.handlePortal` transfers it before the landing branch runs.
    pub(super) fn falling_through_portal(&mut self, f: &mut super::FallingBlock) -> Option<Dim> {
        if self.remote.is_some() {
            return None;
        }
        let mut sections = std::mem::take(&mut self.portals.sections);
        let entry = self.swept_portal(f.previous, f.position, 0.98, 0.98, &mut sections);
        self.portals.sections = sections;
        if let Some(entry) = entry {
            f.portal.set_inside(entry, ENTITY_DELAY);
        }
        let entry = f.portal.handle(true, 0, ENTITY_DELAY)?;
        let plan = self.entity_plan(entry, f.position, 0.98, 0.98, false);
        if !self.may_enter(plan.dim) {
            return None;
        }
        let uuid = f.saved.as_ref().map_or_else(
            || {
                if f.id == 0 {
                    horizon_client::save::random_uuid()
                } else {
                    self.uuid_for(f.id)
                }
            },
            |saved| saved.0,
        );
        let modelled = horizon_client::entity_nbt::falling_block_to_nbt(
            self.state_to_nbt(f.state),
            f.position,
            f.velocity,
            f.age,
            f.drop_item,
            f.damage,
            f.fall_distance,
            f.on_ground,
            f.rotation,
            uuid,
        );
        let mut tag = super::persist::with_raw(modelled, f.saved.as_ref().map(|s| &s.1));
        if let Tag::Compound(fields) = &mut tag {
            fields.insert("PortalCooldown".into(), Tag::Int(ENTITY_DELAY));
            if f.no_gravity {
                fields.insert("NoGravity".into(), Tag::Byte(1));
            } else {
                fields.remove(&"NoGravity".into());
            }
        }
        if f.id != 0 {
            self.forget_entity(f.id);
        }
        self.portals.arrivals.push(Arrival { plan, tag });
        self.portals.dirty = true;
        Some(plan.dim)
    }

    // ---- persistence ----

    fn arrivals_path(&self) -> std::path::PathBuf {
        self.world_root
            .join("data")
            .join("horizon_portal_arrivals.dat")
    }
    /// The waiting entities of the open world (read once).
    pub(super) fn load_portal_arrivals(&mut self) {
        if self.portals.loaded || self.remote.is_some() {
            return;
        }
        self.portals.loaded = true;
        let Some(root) = horizon_client::save::read_dat(&self.arrivals_path()) else {
            return;
        };
        let Some(Tag::List(list)) = root.get("arrivals") else {
            return;
        };
        self.portals
            .arrivals
            .extend(list.iter().filter_map(arrival_from_tag));
    }
    /// Write the waiting entities when they changed.
    pub(super) fn save_portal_arrivals(&mut self) {
        if !self.portals.dirty || self.remote.is_some() {
            return;
        }
        let list = self
            .portals
            .arrivals
            .iter()
            .map(|arrival| {
                let mut tag = arrival_tag(arrival);
                if let Tag::Compound(root) = &mut tag
                    && let Some(entity) = root.get_mut(&"entity".into())
                {
                    super::persist::write_attachments(entity, &self.entity_attachments);
                }
                tag
            })
            .collect();
        let root = Tag::compound([("arrivals", Tag::List(list))]);
        if horizon_client::save::write_dat(&self.arrivals_path(), root).is_ok() {
            self.portals.dirty = false;
        }
    }
}

impl Game<'_> {
    /// PortalShape: an obsidian frame around 2..=21 by 3..=21 empty blocks.
    pub(super) fn portal_shape(
        &self,
        start: [i32; 3],
        x_axis: bool,
    ) -> Option<([i32; 3], i32, i32)> {
        let name = |p: [i32; 3]| {
            self.res
                .facts()
                .get(self.world.block(p[0], p[1], p[2]))
                .name
                .clone()
        };
        let empty = |p: [i32; 3]| {
            let n = name(p);
            matches!(
                &*n,
                "minecraft:air"
                    | "minecraft:cave_air"
                    | "minecraft:fire"
                    | "minecraft:soul_fire"
                    | "minecraft:nether_portal"
            )
        };
        let frame = |p: [i32; 3]| &*name(p) == "minecraft:obsidian";
        let d = if x_axis { [1, 0, 0] } else { [0, 0, 1] };
        let mut bottom = start;
        let mut n = 0;
        while empty([bottom[0], bottom[1] - 1, bottom[2]]) && n < 21 {
            bottom[1] -= 1;
            n += 1;
        }
        if !frame([bottom[0], bottom[1] - 1, bottom[2]]) {
            return None;
        }
        let mut left = bottom;
        n = 0;
        while empty([left[0] - d[0], left[1], left[2] - d[2]]) && n < 21 {
            left = [left[0] - d[0], left[1], left[2] - d[2]];
            n += 1;
        }
        if !frame([left[0] - d[0], left[1], left[2] - d[2]]) {
            return None;
        }
        let mut width = 0;
        while width <= 21 {
            let p = [left[0] + d[0] * width, left[1], left[2] + d[2] * width];
            if !empty(p) || !frame([p[0], p[1] - 1, p[2]]) {
                break;
            }
            width += 1;
        }
        if !(2..=21).contains(&width)
            || !frame([left[0] + d[0] * width, left[1], left[2] + d[2] * width])
        {
            return None;
        }
        let mut height = 0;
        'rows: while height <= 21 {
            let y = left[1] + height;
            if !frame([left[0] - d[0], y, left[2] - d[2]])
                || !frame([left[0] + d[0] * width, y, left[2] + d[2] * width])
            {
                break;
            }
            for i in 0..width {
                if !empty([left[0] + d[0] * i, y, left[2] + d[2] * i]) {
                    break 'rows;
                }
            }
            height += 1;
        }
        let top_ok =
            (0..width).all(|i| frame([left[0] + d[0] * i, left[1] + height, left[2] + d[2] * i]));
        ((3..=21).contains(&height) && top_ok).then_some((left, width, height))
    }
    /// BaseFireBlock.onPlace: lighting fire inside a frame creates a portal,
    /// only in the Overworld and the Nether (`inPortalDimension`).
    pub(super) fn try_light_portal(&mut self, at: [i32; 3]) -> Result<bool> {
        if !matches!(self.dim, Dim::Overworld | Dim::Nether) {
            return Ok(false);
        }
        for x_axis in [true, false] {
            if let Some((origin, width, height)) = self.portal_shape(at, x_axis) {
                let d = if x_axis { [1, 0, 0] } else { [0, 0, 1] };
                let registry = &self.pack.assets.registry;
                let Some(default) = registry.default_state("minecraft:nether_portal") else {
                    return Ok(false);
                };
                let mut portal = registry.state(default).cloned().ok_or("portal state")?;
                portal
                    .properties
                    .insert("axis".into(), if x_axis { "x" } else { "z" }.into());
                let id = registry.resolve(&portal).unwrap_or(default);
                // PortalShape.createPortalBlocks uses BlockPos.betweenClosed:
                // X varies fastest, then Y, then Z.
                let first = BlockPos::new(origin[0], origin[1], origin[2]);
                let last = BlockPos::new(
                    origin[0] + d[0] * (width - 1),
                    origin[1] + height - 1,
                    origin[2] + d[2] * (width - 1),
                );
                for pos in BlockPos::between_closed(first, last).expect("portal size is bounded") {
                    self.level_set_with_flags(
                        [pos.x, pos.y, pos.z],
                        id,
                        level_flags::UPDATE_CLIENTS | level_flags::UPDATE_KNOWN_SHAPE,
                    )?;
                }
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_portals_lead_to_the_end_spawn_and_back_to_the_world_spawn() {
        let spawn = BlockPos::new(-7, 70, 12);
        for from in [Dim::Overworld, Dim::Nether] {
            let plan = Plan::end_portal(from, spawn, 0.9, 1.4);
            assert_eq!(plan.dim, Dim::End);
            assert_eq!(plan.approximate, BlockPos::new(100, 50, 0));
            assert!(plan.end);
        }
        let plan = Plan::end_portal(Dim::End, spawn, 0.25, 0.25);
        assert_eq!(plan.dim, Dim::Overworld);
        assert_eq!(plan.approximate, spawn);
    }

    #[test]
    fn the_end_portal_shape_takes_falling_items_and_standing_mobs() {
        let portal = BlockPos::new(0, 100, 3);
        let at = |y| Vec3::new(0.5, y, 3.5);
        // A dropped item falling through the slab, or in it.
        assert!(touches_end_portal(
            at(100.6),
            at(100.56),
            0.25,
            0.25,
            portal
        ));
        // One tick's fall across the whole slab, in it at neither end.
        assert!(touches_end_portal(at(101.0), at(99.9), 0.25, 0.25, portal));
        // Resting below the slab (on the floor) or above it.
        assert!(!touches_end_portal(
            at(100.0),
            at(100.0),
            0.25,
            0.25,
            portal
        ));
        assert!(!touches_end_portal(
            at(100.8),
            at(100.8),
            0.25,
            0.25,
            portal
        ));
        // A cow standing in the portal block.
        assert!(touches_end_portal(at(100.0), at(100.0), 0.9, 1.4, portal));
        // Beside it.
        let beside = Vec3::new(2.0, 100.0, 3.5);
        assert!(!touches_end_portal(beside, beside, 0.9, 1.4, portal));
    }

    #[test]
    fn end_portals_turn_entities_like_their_teleport_transition() {
        // Into the End: facing west, pitch kept.
        assert_eq!(
            end_portal_rotation(true, [-30.0, 20.0], [45.0, 10.0]),
            [90.0, 20.0]
        );
        // Out of it: turned by the world spawn's yaw and pitch, pitch clamped.
        assert_eq!(
            end_portal_rotation(false, [-30.0, 20.0], [45.0, 10.0]),
            [15.0, 30.0]
        );
        assert_eq!(
            end_portal_rotation(false, [0.0, 80.0], [0.0, 30.0]),
            [0.0, 90.0]
        );
    }

    #[test]
    fn legacy_arrivals_keep_explicit_positive_counters_without_type_migration() {
        for old in [None, Some(0), Some(-1), Some(5)] {
            let mut tag = Tag::compound([("id", Tag::String("minecraft:oak_boat".into()))]);
            if let (Some(old), Tag::Compound(fields)) = (old, &mut tag) {
                fields.insert("PortalCooldown".into(), Tag::Int(old));
            }
            preserve_arrival_cooldown(&mut tag);
            let arrival = Arrival {
                plan: Plan::end_portal(Dim::Overworld, BlockPos::new(0, 64, 0), 1.375, 0.5625),
                tag,
            };
            let restored = arrival_from_tag(&arrival_tag(&arrival)).unwrap();
            let expected = old.filter(|n| *n > 0).unwrap_or(ENTITY_DELAY);
            assert_eq!(
                restored.tag.get("PortalCooldown"),
                Some(&Tag::Int(expected))
            );
        }
    }

    #[test]
    fn waiting_arrivals_keep_their_end_destination_when_saved() {
        let tag = Tag::compound([("id", Tag::String("minecraft:cow".into()))]);
        for (from, dim) in [(Dim::Overworld, Dim::End), (Dim::End, Dim::Overworld)] {
            let arrival = Arrival {
                plan: Plan::end_portal(from, BlockPos::new(3, 64, -5), 0.9, 1.4),
                tag: tag.clone(),
            };
            let back = arrival_from_tag(&arrival_tag(&arrival)).unwrap();
            assert_eq!(back.plan.dim, dim);
            assert_eq!(back.plan.approximate, arrival.plan.approximate);
            assert!(back.plan.end);
            assert_eq!(back.tag, tag);
        }
        // Arrivals saved before End portals were Nether portal plans.
        let mut old = arrival_tag(&Arrival {
            plan: Plan::end_portal(Dim::End, BlockPos::new(0, 64, 0), 0.25, 0.25),
            tag,
        });
        if let Tag::Compound(c) = &mut old {
            c.remove(&"end".into());
        }
        assert!(!arrival_from_tag(&old).unwrap().plan.end);
    }
}

#[cfg(test)]
#[path = "portal_cooldown_tests.rs"]
mod cooldown_tests;
