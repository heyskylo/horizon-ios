//! Boats and the entities around them: what a boat runs into
//! (`AbstractBoat.canVehicleCollide`), the pushes between a boat and what
//! touches it (`AbstractBoat.push`, `LivingEntity.pushEntities`), the mobs
//! it picks up (the end of `AbstractBoat.tick`), where they sit
//! (`positionRider`, `clampRotation`) and how they get out.
//! Saving them under the boat's `Passengers` lives in `persist.rs`.
use super::{Game, boats::Boat, lan_mobs::guest_bounds};
use horizon_client::{
    camera::wrap_degrees,
    fast_hash::FxHashMap,
    join_seq::ListStamp,
    mobs::{BoxReach, Mob, Species, Who, drops::is_animal, equine, riders, seen_grid::SeenGrid},
    scoreboard::pushable_by,
};
use horizon_core::geometry::{Aabb, Vec3};
use std::cell::{Cell, RefCell};

/// `Entity.boardingCooldown` once a mob leaves its boat.
pub(super) const BOARDING_COOLDOWN: i32 = 60;
/// `Player.DEFAULT_VEHICLE_ATTACHMENT`: how far below the seat a player's
/// feet are.
const PLAYER_RIDING_OFFSET: f64 = 0.6;

/// How many box queries the walk over every mob answers, once a write to the mob list has
/// made the grid stale, before a new grid is built: about what building one costs (with
/// 25,000 mobs, six to eight walks). It doubles with each grid of a stretch, so vehicles
/// that each write a mob (a boat pushing the pig beside it) never pay more for grids than
/// for their walks.
const WALKS_BEFORE_GRID: u32 = 8;

/// What a stretch of vehicle ticks knows about the mobs, so a boat or a minecart reads the
/// mobs near it and not every mob of the level ([`Game::mobs_meeting`]).
#[derive(Default)]
pub(super) struct VehicleMobs {
    /// The mobs' [`BoxReach`] with the mob count it was worked out for; None outside a stretch.
    reach: Cell<Option<(usize, BoxReach)>>,
    grid: RefCell<MobGrid>,
    /// The last query's candidates, kept for their allocation.
    near: Cell<Vec<u32>>,
    /// Each ridden vehicle's mobs by slot, for the mob list as it stood ([`Game::vehicle_riders`]).
    riders: RefCell<Option<(ListStamp, FxHashMap<u32, Vec<usize>>)>>,
}
#[derive(Default)]
struct MobGrid {
    /// The mobs' boxes by column and the list they were read from: they hold until that
    /// list's next write ([`ListStamp`]), which every push, pick-up, death and join is.
    built: Option<(ListStamp, SeenGrid)>,
    /// The queries walked since the grid went stale, and how many are before the next grid.
    walked: u32,
    patience: u32,
}

/// `Entity.canRide` for a mob species: `Warden` and `WitherBoss` override it
/// to return false, so no vehicle ever takes them aboard.
pub(super) fn species_can_ride(species: Species) -> bool {
    !matches!(species, Species::Warden | Species::Wither)
}

/// `Entity.isVehicle`: a player or a mob rides it.
fn carries(m: &Mob) -> bool {
    m.rider.is_some() || m.extra.rider.is_some()
}
/// `isPushable`, as `pushableBy` and `canVehicleCollide` ask it:
/// `Mob::pushable` plus the shulkers it leaves out (they take no push, but
/// count), less a ridden horse (`AbstractHorse.isPushable`).
fn pushable(m: &Mob) -> bool {
    !m.removed
        && (m.pushable() || m.species() == Species::Shulker && m.alive())
        && !(equine::is_equine(m.species()) && carries(m))
}
/// Whether `Entity.push` moves the mob: it is pushable and carries no one.
fn moved_by_push(m: &Mob) -> bool {
    !m.removed && m.pushable() && !carries(m)
}
/// `Entity.push(Entity)`: the push `other` gets from `this` (which takes the
/// opposite one), or None when they stand on nearly the same spot.
pub(super) fn push_impulse(this: Vec3, other: Vec3) -> Option<Vec3> {
    let (mut xa, mut za) = (other.x - this.x, other.z - this.z);
    let mut dd = xa.abs().max(za.abs());
    if dd < f64::from(0.01_f32) {
        return None;
    }
    dd = dd.sqrt();
    xa /= dd;
    za /= dd;
    let pow = (1.0 / dd).min(1.0);
    let k = f64::from(0.05_f32);
    Some(Vec3::new(xa * pow * k, 0.0, za * pow * k))
}
/// `AbstractBoat.push`: another boat is pushed while it is lower than this
/// boat's top, anything else while its feet are no higher than the boat's.
pub(super) fn boat_pushes(boat: Aabb, other: Aabb, other_is_boat: bool) -> bool {
    if other_is_boat {
        other.min_position().y < boat.max_position().y
    } else {
        other.min_position().y <= boat.min_position().y
    }
}

impl Game<'_> {
    /// Starts or ends a stretch of vehicle ticks (`tick_boats`, `tick_carts`): while one runs,
    /// a vehicle's box query ([`Self::mobs_meeting`]) reads the mobs near it from a grid of
    /// their boxes, or tests the mobs' positions against their [`BoxReach`] first. A mob
    /// joining or leaving works the reach out again, and so does [`Self::vehicle_picked_up`].
    /// The first query that needs one works it out (the reach's count is one no list has), so
    /// a level without boats or carts never walks its mobs for them.
    pub(super) fn vehicles_ticking(&self, on: bool) {
        let reach = on.then(|| (usize::MAX, BoxReach::default()));
        self.vehicle_mobs.reach.set(reach);
        // The boats' grid serves the carts ticking right after them; an older one goes.
        let mut grid = self.vehicle_mobs.grid.borrow_mut();
        let stamp = self.mobs.stamp();
        let built = grid.built.take().filter(|(s, _)| *s == stamp);
        *grid = MobGrid {
            built,
            ..Default::default()
        };
    }
    /// A vehicle picked a mob up: a shulker turns its lid up (`Shulker.startRiding`), which
    /// changes its box, so the next query works the [`BoxReach`] out again.
    pub(super) fn vehicle_picked_up(&self) {
        if let Some((_, reach)) = self.vehicle_mobs.reach.get() {
            self.vehicle_mobs.reach.set(Some((usize::MAX, reach)));
        }
    }
    /// The mobs' [`BoxReach`] while vehicles tick, none otherwise.
    pub(super) fn vehicle_mob_reach(&self) -> Option<BoxReach> {
        let (count, reach) = self.vehicle_mobs.reach.get()?;
        if count == self.mobs.len() {
            return Some(reach);
        }
        let reach = BoxReach::of(&self.mobs);
        self.vehicle_mobs.reach.set(Some((self.mobs.len(), reach)));
        Some(reach)
    }
    /// The slots, ascending, of the mobs whose vehicle is `id`, whatever their state. While
    /// vehicles tick, one walk over the mobs answers for every vehicle until the mob list is
    /// next written: each boat asks for its passengers and each cart on rails whether it is
    /// ridden, and each one walked every mob of the level.
    fn vehicle_riders(&self, id: u32) -> Vec<usize> {
        let rides = |v: u32| move |&j: &usize| self.mobs[j].extra.mount == Some(v);
        if !self.vehicle_mobs_indexed() {
            return (0..self.mobs.len()).filter(rides(id)).collect();
        }
        let stamp = self.mobs.stamp();
        let mut riders = self.vehicle_mobs.riders.borrow_mut();
        if riders.as_ref().is_none_or(|(s, _)| *s != stamp) {
            let mut by_vehicle = FxHashMap::<u32, Vec<usize>>::default();
            for (j, m) in self.mobs.iter().enumerate() {
                if let Some(v) = m.extra.mount {
                    by_vehicle.entry(v).or_default().push(j);
                }
            }
            *riders = Some((stamp, by_vehicle));
        }
        let of = riders
            .as_ref()
            .and_then(|(_, by_vehicle)| by_vehicle.get(&id));
        of.cloned().unwrap_or_default()
    }
    /// Whether a mob (one not removed) rides vehicle `id`: `Entity.isVehicle` for a cart.
    pub(super) fn mob_rides(&self, id: u32) -> bool {
        let riders = self.vehicle_riders(id);
        riders.iter().any(|&j| !self.mobs[j].removed)
    }
    /// Whether what a stretch of vehicle ticks reads from the mob list may stand in for a walk
    /// over it: a stretch runs, and no native mod is loaded (a hook can move a mob unseen, as
    /// for the carts' `CartIndex`).
    fn vehicle_mobs_indexed(&self) -> bool {
        self.vehicle_mobs.reach.get().is_some() && horizon_client::mod_api::mods().is_empty()
    }
    /// The slots, ascending, of the mobs whose box may meet `area` into `out` (a superset: the
    /// caller still tests each box), from a grid of the mobs' boxes that the first query of a
    /// stretch of vehicle ticks builds. False: walk them all. That is outside a stretch or with
    /// native mods loaded ([`Self::vehicle_mobs_indexed`]), when the grid cannot say, and for
    /// a while after a write to the mob list ([`MobGrid`]).
    fn vehicle_mobs_near(&self, area: Aabb, out: &mut Vec<u32>) -> bool {
        if !self.vehicle_mobs_indexed() {
            return false;
        }
        let stamp = self.mobs.stamp();
        let mut grid = self.vehicle_mobs.grid.borrow_mut();
        if grid.built.as_ref().is_none_or(|(s, _)| *s != stamp) {
            if grid.walked < grid.patience {
                grid.walked += 1;
                return false;
            }
            grid.built = Some((stamp, SeenGrid::new(self.mobs.iter().map(Mob::bounds))));
            grid.walked = 0;
            grid.patience = (grid.patience * 2).max(WALKS_BEFORE_GRID);
        }
        let built = grid.built.as_ref();
        built.is_some_and(|(_, cells)| cells.candidates(area, out))
    }
    /// A vehicle's box query: `each` gets every mob whose box meets `area`, in slot order,
    /// with its slot and its box. While vehicles tick it reads the mobs near the area
    /// ([`Self::vehicle_mobs_near`]) or, walking them all, tests each position against the
    /// [`BoxReach`] before working the box out. The mobs and their order are those of a walk
    /// that tests every box.
    pub(super) fn mobs_meeting(&self, area: Aabb, mut each: impl FnMut(usize, &Mob, Aabb)) {
        let mut near = self.vehicle_mobs.near.take();
        if self.vehicle_mobs_near(area, &mut near) {
            for &j in &near {
                let m = &self.mobs[j as usize];
                let b = m.bounds();
                if b.intersects(area) {
                    each(j as usize, m, b);
                }
            }
        } else {
            let reach = self.vehicle_mob_reach();
            for (j, m) in self.mobs.iter().enumerate() {
                if reach.is_some_and(|r| !r.may_meet(m, area)) {
                    continue;
                }
                let b = m.bounds();
                if b.intersects(area) {
                    each(j, m, b);
                }
            }
        }
        self.vehicle_mobs.near.set(near);
    }
    /// The slots, ascending, of the mobs whose box may meet a cart, read from the mobs' grid
    /// once per cart. None when the mobs are the fewer (asking the carts' grid once per mob
    /// is then the shorter way) or the grid does not answer.
    pub(super) fn mobs_near_carts(&self) -> Option<Vec<u32>> {
        if self.mobs.len() <= self.carts.len() {
            return None;
        }
        let (mut slots, mut near) = (Vec::new(), Vec::new());
        for c in self.carts.iter() {
            if !self.vehicle_mobs_near(c.bounds(), &mut near) {
                return None;
            }
            slots.extend_from_slice(&near);
        }
        slots.sort_unstable();
        slots.dedup();
        Some(slots)
    }
    /// Whether mob `m`, whose box `reach` bounds, may touch a cart: the carts' grid (`cells`)
    /// names the carts near it, or every cart when the grid cannot (a superset either way).
    pub(super) fn mob_may_meet_cart(
        &self,
        reach: BoxReach,
        m: &Mob,
        cells: &SeenGrid,
        near: &mut Vec<u32>,
    ) -> bool {
        if !cells.candidates(reach.around(m.position), near) {
            near.clear();
            near.extend(0..self.carts.len() as u32);
        }
        near.iter()
            .any(|&k| reach.may_meet(m, self.carts[k as usize].bounds()))
    }
    /// The players in boat `id`: the window's player (or the server player
    /// acting as it) and the server's other players.
    pub(super) fn boat_players(&self, id: u32) -> usize {
        let guests = self.lan.as_ref().map_or(0, |h| {
            h.hub
                .guests
                .iter()
                .filter(|g| Some(g.conn) != self.lan_acting && g.vehicle == Some(id))
                .count()
        });
        usize::from(self.riding == Some(id)) + guests
    }
    /// The mobs in boat `id`, in their seat order after the players
    /// (`Entity.addPassenger` puts a player first).
    pub(super) fn boat_mobs(&self, id: u32) -> Vec<usize> {
        let mut riders = self.vehicle_riders(id);
        riders.retain(|&j| !self.mobs[j].removed && self.mobs[j].alive());
        riders
    }
    /// `getPassengers().size()` of boat `id`.
    pub(super) fn boat_passengers(&self, id: u32) -> usize {
        self.boat_players(id) + self.boat_mobs(id).len()
    }
    /// `AbstractBoat.canAddPassenger`: a seat is free and the boat is not
    /// under water.
    pub(super) fn boat_has_room(&self, i: usize) -> bool {
        let b = &self.boats[i];
        self.boat_passengers(b.id) < b.max_passengers() && !b.under_water()
    }
    /// Where the player in boat `i` sits: in the middle alone, in front of a
    /// mob.
    pub(super) fn player_boat_seat(&self, i: usize) -> Vec3 {
        let b = &self.boats[i];
        let count = self.boat_players(b.id).max(1) + self.boat_mobs(b.id).len();
        b.seat(0, count, false, PLAYER_RIDING_OFFSET, false)
    }
    /// `getRootVehicle` of mob `j`: the mob or boat at the bottom of what it
    /// rides, None when it rides nothing.
    pub(super) fn mob_root_vehicle(&self, mut j: usize) -> Option<u32> {
        let mut root = None;
        for _ in 0..8 {
            let Some(v) = self.mobs[j].extra.mount else {
                break;
            };
            root = Some(v);
            match self.mobs.iter().position(|m| m.id == v) {
                Some(k) => j = k,
                None => break,
            }
        }
        root
    }
    /// The players outside boat `id` that push and block it (living, not
    /// spectators), with the window player's team.
    fn players_outside(&self, id: u32) -> Vec<(Aabb, bool)> {
        let mut out = Vec::new();
        if let Some(p) = self.player.as_ref()
            && self.riding != Some(id)
            && !self.spectator()
            && !p.health.dead_or_dying()
        {
            out.push((p.bounds(), true));
        }
        if let Some(host) = self.lan.as_ref() {
            for g in host.hub.guests.iter().filter(|g| {
                Some(g.conn) != self.lan_acting
                    && host.hub.here(g)
                    && g.loaded
                    && !g.dead
                    && !g.spectator()
                    && g.vehicle != Some(id)
            }) {
                out.push((guest_bounds(g), false));
            }
        }
        out
    }
    /// `getEntityCollisions` of boat `id` within `area`, with
    /// `AbstractBoat.canVehicleCollide`: the other boats, minecarts,
    /// shulkers and pushable mobs and players, but not its own passengers.
    pub(super) fn boat_obstacles(&self, id: u32, area: Aabb) -> Vec<Aabb> {
        let mut out = Vec::new();
        self.mobs_meeting(area, |j, m, b| {
            if pushable(m) && (m.extra.mount.is_none() || self.mob_root_vehicle(j) != Some(id)) {
                out.push(b);
            }
        });
        out.extend(
            self.boats
                .iter()
                .filter(|b| b.id != id)
                .map(Boat::bounds)
                .chain(self.carts.iter().map(|c| c.bounds()))
                .chain(self.players_outside(id).into_iter().map(|(bb, _)| bb))
                .filter(|bb| bb.intersects(area)),
        );
        out
    }
    /// What boat `b` may run into this tick.
    pub(super) fn boat_move_obstacles(&self, b: &Boat) -> Vec<Aabb> {
        let reach = b.velocity.length() + 1.0;
        self.boat_obstacles(b.id, b.bounds().inflate(Vec3::new(reach, reach, reach)))
    }
    /// `noEntityCollision(boat, box)` for a new boat (`BoatItem.use`).
    pub(super) fn boat_entity_collision(&self, bb: Aabb) -> bool {
        // EntityGetter.getEntityCollisions: the box grown by 1e-7.
        let area = bb.inflate(Vec3::new(1.0e-7, 1.0e-7, 1.0e-7));
        !self.boat_obstacles(u32::MAX, area).is_empty()
    }

    /// The end of `AbstractBoat.tick` on the server for boat `i`: each
    /// entity it touches gets in (a mob, while no player steers) or is
    /// pushed apart from it, and `LivingEntity.pushEntities` of each mob and
    /// player touching it pushes the two apart once more. Only the window's
    /// own player is moved here: a server's players are pushed by their
    /// clients, which tick the boats near them (`remote_pushes`).
    pub(super) fn boat_contacts(&mut self, i: usize) {
        let (id, bb, max) = {
            let b = &self.boats[i];
            (b.id, b.bounds(), b.max_passengers())
        };
        let area = bb.inflate(Vec3::new(
            f64::from(0.2_f32),
            f64::from(-0.01_f32),
            f64::from(0.2_f32),
        ));
        let mut count = self.boat_passengers(id);
        // getControllingPassenger is a player: one rides it (players sit first).
        let pick_up = self.boat_players(id) == 0;
        // canAddPassenger, which startRiding asks after the checks below.
        let room = !self.boats[i].under_water();
        // The mobs near the two boxes: the teams are only worked out when something
        // touches the boat.
        let mut near = Vec::new();
        self.mobs_meeting(area.union_bounds(bb), |j, _, mb| {
            if mb.intersects(area) || mb.intersects(bb) {
                near.push(j);
            }
        });
        let outside = self.players_outside(id);
        let touching = !near.is_empty()
            || outside
                .iter()
                .any(|(pb, _)| pb.intersects(area) || pb.intersects(bb));
        // (mob, touched by the boat's sweep, its own pushEntities reaches the boat)
        let teams = if touching {
            self.mob_teams()
        } else {
            Default::default()
        };
        let mobs: Vec<(u32, bool, bool)> = near
            .into_iter()
            .filter_map(|j| {
                let m = &self.mobs[j];
                if !pushable(m) || m.extra.mount.is_some() && self.mob_root_vehicle(j) == Some(id) {
                    return None;
                }
                let mb = m.bounds();
                let team = teams.of(Who::Mob(m.id));
                let touched = mb.intersects(area) && pushable_by(None, team, true);
                // Bats push nothing (Bat.pushEntities).
                let pushes = m.species() != Species::Bat
                    && mb.intersects(bb)
                    && pushable_by(team, None, true);
                (touched || pushes).then_some((m.id, touched, pushes))
            })
            .collect();
        let player_team = teams.of(Who::Player);
        let players: Vec<(Aabb, bool, bool, bool)> = outside
            .into_iter()
            .map(|(pb, own)| {
                let team = if own { player_team } else { None };
                (
                    pb,
                    own,
                    pb.intersects(area) && pushable_by(None, team, true),
                    pb.intersects(bb) && pushable_by(team, None, true),
                )
            })
            .collect();
        drop(teams);
        for (mob, touched, pushes) in mobs {
            // By id: a mod's mount hook may have removed mobs since the list
            // was made, which moves the others' indices.
            let Some(j) = self.mobs.iter().position(|m| m.id == mob) else {
                continue;
            };
            if touched {
                let m = &self.mobs[j];
                let boards = pick_up
                    && count < max
                    && m.extra.mount.is_none()
                    && m.width() < super::boats::WIDTH as f32
                    && !self
                        .res
                        .mob_data()
                        .entity_has("minecraft:cannot_be_pushed_onto_boats", &m.kind.id);
                if boards {
                    // startRiding fails quietly (no push) while the mob's
                    // boarding cooldown runs (Entity.canRide) or the boat
                    // is under water, or when a mod cancels it
                    // (EntityMountEvent).
                    let ready = m.extra.boarding_cooldown <= 0 && species_can_ride(m.species());
                    if room
                        && ready
                        && self.native_mob_may_ride(mob, id, true)
                        && let Some(m) = self.mobs.iter_mut().find(|m| m.id == mob)
                    {
                        horizon_client::mobs::shulker::on_start_riding(m);
                        m.extra.mount = Some(id);
                        m.extra.passenger = true;
                        m.ai.nav.stop();
                        count += 1;
                        // Mob.startRiding drops its lead.
                        self.break_leash(mob, true, false);
                        self.note_started_riding(id);
                        self.vehicle_picked_up();
                    }
                    continue;
                }
                self.boat_push_mob(i, j, count);
            }
            if pushes {
                self.boat_push_mob(i, j, count);
            }
        }
        for (pb, own, touched, pushes) in players {
            let Some(v) = push_impulse(self.boats[i].position, pb.center())
                .filter(|_| boat_pushes(bb, pb, false))
            else {
                continue;
            };
            for _ in 0..usize::from(touched) + usize::from(pushes) {
                if count == 0 {
                    let b = &mut self.boats[i];
                    b.velocity = b.velocity.subtract(v);
                }
                if own && let Some(p) = self.player.as_mut() {
                    p.velocity = p.velocity.add_vector(v);
                }
            }
        }
        for k in 0..self.boats.len() {
            let ob = self.boats[k].bounds();
            if k == i || !ob.intersects(area) || !boat_pushes(bb, ob, true) {
                continue;
            }
            let Some(v) = push_impulse(self.boats[i].position, self.boats[k].position) else {
                continue;
            };
            if count == 0 {
                self.boats[i].velocity = self.boats[i].velocity.subtract(v);
            }
            if self.boat_passengers(self.boats[k].id) == 0 {
                self.boats[k].velocity = self.boats[k].velocity.add_vector(v);
            }
        }
        let guest_carts = self.guest_vehicles();
        for c in 0..self.carts.len() {
            let cb = self.carts[c].bounds();
            if !cb.intersects(area) || !boat_pushes(bb, cb, false) {
                continue;
            }
            let Some(v) = push_impulse(self.boats[i].position, self.carts[c].position) else {
                continue;
            };
            if count == 0 {
                self.boats[i].velocity = self.boats[i].velocity.subtract(v);
            }
            let cid = self.carts[c].id;
            let ridden = self.riding == Some(cid)
                || guest_carts.contains(&cid)
                || self.mobs.iter().any(|m| m.extra.mount == Some(cid));
            if !ridden {
                self.carts[c].velocity = self.carts[c].velocity.add_vector(v);
            }
        }
    }
    /// `AbstractBoat.push(mob)` from boat `i`, which has `count` passengers.
    fn boat_push_mob(&mut self, i: usize, j: usize, count: usize) {
        let (b, m) = (&self.boats[i], &self.mobs[j]);
        if !boat_pushes(b.bounds(), m.bounds(), false) {
            return;
        }
        let Some(v) = push_impulse(b.position, m.position) else {
            return;
        };
        if count == 0 {
            let b = &mut self.boats[i];
            b.velocity = b.velocity.subtract(v);
        }
        let m = &mut self.mobs[j];
        if moved_by_push(m) {
            m.velocity = m.velocity.add_vector(v);
            m.needs_sync = true;
        }
    }

    /// `positionRider` and `clampRotation` for the mobs in boats: each sits
    /// in its seat facing the way the boat does, an animal in a full boat
    /// sideways, its head kept within its reach of its body.
    pub(super) fn seat_boat_mobs(&mut self, riding: &super::passengers::Riders) {
        for i in 0..self.boats.len() {
            let id = self.boats[i].id;
            // `boat_mobs`, from the riders found in one pass.
            let mobs: Vec<usize> = riding
                .get(&id)
                .into_iter()
                .flatten()
                .copied()
                .filter(|&j| !self.mobs[j].removed && self.mobs[j].alive())
                .collect();
            if mobs.is_empty() {
                continue;
            }
            let players = self.boat_players(id);
            let count = players + mobs.len();
            let b = &self.boats[i];
            let full = count == b.max_passengers();
            let delta = b.delta_rotation;
            let boat_box = b.bounds();
            let ridden = horizon_client::motion::RiddenBoat {
                under_water: b.under_water(),
                max_y: boat_box.max_position().y,
            };
            let seats: Vec<_> = mobs
                .iter()
                .enumerate()
                .map(|(n, &j)| {
                    let m = &self.mobs[j];
                    let animal = is_animal(m.species());
                    // Another player's too, on a server's client.
                    // AbstractBoat's box is 1.375 wide.
                    let offset = riders::vehicle_attachment(m, 1.375);
                    let index = players + n;
                    // calculatePassengerBodyYRot.
                    let turn = match (animal && full, m.id.is_multiple_of(2)) {
                        (false, _) => 0.0,
                        (true, true) => 90.0,
                        (true, false) => 270.0,
                    };
                    let turns = !self
                        .res
                        .mob_data()
                        .entity_has("minecraft:can_turn_in_boats", &m.kind.id);
                    (
                        j,
                        offset,
                        b.seat(index, count, animal, offset, false),
                        b.seat(index, count, animal, offset, true),
                        turns.then_some((b.yaw + turn, b.previous_yaw + turn)),
                    )
                })
                .collect();
            for (j, offset, at, previous, facing) in seats {
                let m = &mut self.mobs[j];
                m.seated(id, offset, boat_box);
                m.fluids.boat = Some((id, ridden));
                m.position = at;
                m.previous = previous;
                m.velocity = Vec3::ZERO;
                m.fall_distance = 0.0;
                m.on_ground = false;
                m.extra.passenger = true;
                m.ai.nav.stop();
                let Some((body, previous_body)) = facing else {
                    continue;
                };
                m.head_yaw += delta;
                m.body_yaw = body;
                m.previous_body_yaw = previous_body;
                m.yaw = body;
                m.previous_yaw = previous_body;
                // Mob.clampHeadRotationToBody.
                let limit = m.kind.max_head_y_rot;
                let d = wrap_degrees(body - m.head_yaw);
                m.head_yaw += d - d.clamp(-limit, limit);
            }
        }
    }
    /// `ejectPassengers` for the mobs in boat `i`: each gets out where
    /// `getDismountLocationForPassenger` puts it (beside the boat when it
    /// broke: `LivingEntity.dismountVehicle`) and can't board for a while.
    pub(super) fn eject_boat_mobs(&mut self, i: usize, broken: bool) {
        let b = &self.boats[i];
        let (id, y) = (b.id, b.position.y);
        for j in self.boat_mobs(id) {
            let m = &self.mobs[j];
            let at = if broken {
                Vec3::new(m.position.x, m.position.y.max(y), m.position.z)
            } else {
                self.boat_dismount_target(i, f64::from(m.width()), m.yaw)
            };
            let m = &mut self.mobs[j];
            m.extra.mount = None;
            m.extra.passenger = false;
            m.extra.boarding_cooldown = BOARDING_COOLDOWN;
            m.position = at;
            m.previous = at;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_is_capped_and_scaled() {
        // One block apart on x: the full 0.05 along x.
        let v = push_impulse(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)).unwrap();
        assert!((v.x - f64::from(0.05_f32)).abs() < 1e-12 && v.z == 0.0);
        // Almost on the same spot: no push.
        assert!(push_impulse(Vec3::ZERO, Vec3::new(0.005, 0.0, 0.0)).is_none());
    }

    #[test]
    fn warden_and_wither_never_board() {
        assert!(!species_can_ride(Species::Warden));
        assert!(!species_can_ride(Species::Wither));
        assert!(species_can_ride(Species::Cow));
    }

    #[test]
    fn boats_push_by_height() {
        let boat = Aabb::new(Vec3::new(0.0, 64.0, 0.0), Vec3::new(1.0, 64.5625, 1.0));
        let at = |y: f64| Aabb::new(Vec3::new(0.0, y, 0.0), Vec3::new(1.0, y + 1.0, 1.0));
        assert!(boat_pushes(boat, at(64.0), false));
        assert!(!boat_pushes(boat, at(64.1), false));
        assert!(boat_pushes(boat, at(64.5), true));
        assert!(!boat_pushes(boat, at(64.5625), true));
    }

    /// A mob pushes the cart its box touches while a mob far from every cart pushes none and
    /// is not pushed, whichever grid finds them: the carts' (four carts in four of its columns,
    /// asked once per mob while the mobs are the fewer) or, in a crowd, the mobs' (asked once
    /// per cart).
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn only_the_mob_touching_a_cart_pushes_it() {
        use super::super::minecarts::{CartKind, Minecart};
        let pushed = [0, 12].map(|crowd: u32| {
            let mut game = super::super::native_machines::tests::server_game();
            let pig = game.res.mob_types().get("minecraft:pig").unwrap().clone();
            for (i, (x, z)) in [(0.5, 0.5), (20.5, 0.5), (0.5, 20.5), (20.5, 20.5)]
                .into_iter()
                .enumerate()
            {
                let at = Vec3::new(x, 64.0, z);
                game.carts
                    .push(Minecart::new(100 + i as u32, CartKind::Chest, at));
            }
            // 0.5 west of the second cart (boxes 0.9 and 0.98 wide), in the middle of them
            // all, and the crowd well away.
            let spots = [(20.0, 0.5), (10.5, 10.5)].into_iter();
            let crowd = (0..crowd).map(|i| (40.5 + 3.0 * f64::from(i), 40.5));
            for (i, (x, z)) in spots.chain(crowd).enumerate() {
                let at = Vec3::new(x, 64.0, z);
                game.mobs.push(Mob::new(1 + i as u32, pig.clone(), at, 0.0));
            }
            game.tick_carts();
            let velocity = |id| game.mobs.iter().find(|m| m.id == id).unwrap().velocity;
            assert!(velocity(1).x < 0.0, "pushed back west: {:?}", velocity(1));
            let idle = game.mobs.iter().filter(|m| m.velocity == Vec3::ZERO);
            assert_eq!(idle.count(), game.mobs.len() - 1);
            velocity(1)
        });
        assert_eq!(pushed[0], pushed[1]);
    }

    /// A vehicle's box query during a stretch of vehicle ticks names the mobs a walk over
    /// every box names, in the same order, also right after a mob moved into the box (a
    /// push or a pick-up writes the mob list in the middle of a stretch).
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_vehicles_box_query_matches_a_walk_over_every_mob() {
        let mut game = super::super::native_machines::tests::server_game();
        let pig = game.res.mob_types().get("minecraft:pig").unwrap().clone();
        // A small LCG keeps the scene the same on every run.
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = |range: f64| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            (seed >> 11) as f64 / (1u64 << 53) as f64 * range
        };
        for id in 1..=400 {
            let at = Vec3::new(next(48.0), 64.0 + next(3.0), next(48.0));
            game.mobs.push(Mob::new(id, pig.clone(), at, 0.0));
        }
        let mut met = 0;
        for round in 0..300 {
            // Three stretches, each with two writes to the mob list.
            if round % 100 == 0 {
                game.vehicles_ticking(true);
            }
            let corner = Vec3::new(next(46.0), 63.0 + next(3.0), next(46.0));
            let size = Vec3::new(0.5 + next(2.5), 0.5 + next(1.5), 0.5 + next(2.5));
            let area = Aabb::new(corner, corner.add_vector(size));
            if round % 40 == 20 {
                let j = round % game.mobs.len();
                game.mobs[j].position = area.center();
            }
            let walked: Vec<Aabb> = game
                .mobs
                .iter()
                .map(Mob::bounds)
                .filter(|b| b.intersects(area))
                .collect();
            met += walked.len();
            assert_eq!(game.boat_obstacles(u32::MAX, area), walked, "round {round}");
            if round % 100 == 99 {
                // The last comparisons ran against the grid: it stands, for the list as it is.
                let fresh = |(stamp, _): &(ListStamp, SeenGrid)| *stamp == game.mobs.stamp();
                assert!(
                    game.vehicle_mobs
                        .grid
                        .borrow()
                        .built
                        .as_ref()
                        .is_some_and(fresh)
                );
                game.vehicles_ticking(false);
            }
        }
        assert!(met > 100, "the boxes met {met} mobs");
    }

    /// Who rides a vehicle follows the mob list through a stretch of vehicle ticks: a mob that
    /// boards counts from then on, one removed no longer does, and a dead one still makes
    /// its cart a vehicle but is no passenger of a boat.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_vehicle_knows_its_mob_riders_as_soon_as_they_board() {
        let mut game = super::super::native_machines::tests::server_game();
        let pig = game.res.mob_types().get("minecraft:pig").unwrap().clone();
        for id in 1..=4 {
            game.mobs.push(Mob::new(id, pig.clone(), Vec3::ZERO, 0.0));
        }
        let riders =
            |game: &Game| [100, 101, 102].map(|id| (game.mob_rides(id), game.boat_mobs(id)));
        game.mobs[0].extra.mount = Some(100);
        game.vehicles_ticking(true);
        let none = (false, vec![]);
        assert_eq!(riders(&game), [(true, vec![0]), none.clone(), none.clone()]);
        game.mobs[0].removed = true;
        game.mobs[1].extra.mount = Some(101);
        game.mobs[3].extra.mount = Some(102);
        game.mobs[3].health = 0.0;
        let after = [none, (true, vec![1]), (true, vec![])];
        assert_eq!(riders(&game), after);
        game.vehicles_ticking(false);
        assert_eq!(riders(&game), after);
    }
}
