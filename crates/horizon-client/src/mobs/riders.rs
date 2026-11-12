//! Mobs riding mobs (`Entity.getPassengers`): where a rider sits on its
//! vehicle (`EntityAttachments`), the rider steering the vehicle toward its
//! target (`Mob.getControllingPassenger`, whose navigation and move control
//! the rider's goals use), and the saved jockey fields. The game links the
//! two (`game/passengers.rs`) and spawns the jockeys.
use super::{
    Mob, MobWorld, Seen, Species,
    goals::{JUMP, LOOK, MOVE, TARGET},
};
use horizon_core::geometry::{Aabb, Vec3};
use horizon_world::nbt::{Compound, Tag};

/// The goal flags a controlling passenger takes from its vehicle.
const CONTROLLED: u8 = MOVE | JUMP | LOOK;
/// `Entity.removePassenger`: a passenger that got off cannot board for 60 ticks.
const BOARDING_COOLDOWN: i32 = 60;
/// `Piglin.startRiding`: how many baby piglins stack on one hoglin.
const MAX_PASSENGERS_ON_ONE_HOGLIN: i32 = 3;

/// `LivingEntity.getScale`: the `minecraft:scale` the server sent on any
/// living entity, 1 when unset (`Mob::model_scale`).
pub(super) fn scale(m: &Mob) -> f64 {
    f64::from(m.model_scale())
}
/// Players and mannequins (`Avatar`).
fn avatar(m: &Mob) -> bool {
    matches!(&*m.kind.id, "minecraft:player" | "minecraft:mannequin")
}
/// `EntityType` passenger attachment of a vehicle (before rotation): where a
/// rider's attachment point sits relative to the vehicle's feet, in its
/// scale (`getDimensions` scales the attachments with the box).
pub fn passenger_attachment(v: &Mob) -> [f64; 3] {
    let [x, y, z] = base_attachment(v);
    let s = scale(v);
    // AbstractHorse.getPassengerAttachmentPoint: a rearing horse slides it,
    // after the scaled attachment (`rear_offset` scales with age itself).
    let rear = super::equine::rear_offset(v);
    [x * s + rear[0], y * s + rear[1], z * s + rear[2]]
}
/// An `EntityType` attachment point: the builder takes floats, so the double
/// the position math sees is the float's widening (`Vec3(x, y, z)` of
/// `0.86875F` is `0.8687499761581421`).
fn pt(x: f32, y: f32, z: f32) -> [f64; 3] {
    [f64::from(x), f64::from(y), f64::from(z)]
}
/// `EntityTypes` `passengerAttachments(y)` with one height.
fn up(y: f32) -> [f64; 3] {
    pt(0.0, y, 0.0)
}
/// `EntityType.HAPPY_GHAST` `passengerAttachments`: four seats at the corners
/// of the back (front, left, back, right), picked by the rider's place in
/// `getPassengers` (`EntityAttachments.get` clamps the index to the last).
pub fn happy_ghast_attachment(index: usize) -> [f64; 3] {
    const SEATS: [[f32; 3]; 4] = [
        [0.0, 4.0, 1.7],
        [-1.7, 4.0, 0.0],
        [0.0, 4.0, -1.7],
        [1.7, 4.0, 0.0],
    ];
    let [x, y, z] = SEATS[index.min(SEATS.len() - 1)];
    pt(x, y, z)
}
/// `Pig.getDefaultDimensions` and other species' age-specific passenger attachments.
fn base_attachment(v: &Mob) -> [f64; 3] {
    let baby = v.is_baby();
    // Players and mannequins have no `passengerAttachments`: their box top.
    if avatar(v) {
        return [0.0, f64::from(v.height()) / scale(v), 0.0];
    }
    match v.species() {
        Species::Chicken => pt(0.0, 0.7, -0.1),
        Species::Spider => up(0.765),
        Species::Ravager => pt(0.0, 2.2625, -0.0625),
        Species::Horse => up(1.44375),
        Species::Donkey => up(1.1125),
        Species::Mule => up(1.2125),
        Species::SkeletonHorse | Species::ZombieHorse => up(1.31875),
        Species::Pig if baby => up(0.5),
        Species::Pig => up(0.86875),
        // `Hoglin.BABY_DIMENSIONS` and `EntityType.HOGLIN`.
        Species::Hoglin if baby => up(0.875),
        Species::Hoglin => up(1.49375),
        Species::Zoglin if !baby => up(1.49375),
        Species::Nautilus | Species::ZombieNautilus => up(1.1375),
        Species::Camel | Species::CamelHusk => super::camel::passenger_attachment(v),
        // The rest of the `passengerAttachments(...)` in `EntityTypes`. The
        // humanoids' babies carry their own dimensions (the default below).
        Species::Cat => up(0.5125),
        Species::Cow | Species::Mooshroom => up(1.36875),
        Species::Sheep => up(1.2375),
        Species::Goat => up(1.1125),
        Species::Ocelot => up(0.6375),
        Species::Parrot => up(0.4625),
        Species::Phantom => up(0.3375),
        Species::Vex => up(0.7375),
        Species::Guardian => up(0.975),
        Species::ElderGuardian => up(2.350625),
        Species::Endermite | Species::Silverfish => up(0.2375),
        Species::Enderman => up(2.80625),
        Species::Ghast => up(4.0625),
        Species::HappyGhast => happy_ghast_attachment(0),
        Species::Warden => up(3.15),
        Species::Sniffer => up(2.09375),
        Species::Fox => pt(0.0, 0.6375, -0.25),
        Species::Frog => pt(0.0, 0.375, -0.25),
        Species::Llama | Species::TraderLlama => pt(0.0, 1.37, -0.3),
        Species::Turtle => pt(0.0, 0.55625, -0.25),
        Species::Wolf => pt(0.0, 0.81875, -0.0625),
        Species::Witch => up(2.2625),
        Species::Evoker | Species::Illusioner | Species::Pillager | Species::Vindicator => up(2.0),
        Species::ZombifiedPiglin if !baby => up(2.0),
        Species::Zombie | Species::Drowned | Species::Piglin | Species::PiglinBrute if !baby => {
            up(2.0125)
        }
        Species::Husk if !baby => up(2.075),
        Species::ZombieVillager if !baby => up(2.125),
        // The default attachments: the top of the bounding box, before
        // `passenger_attachment`'s scale — `height()` carries
        // `minecraft:scale` since `getDimensions` grew every living entity's
        // box, so the box's own height divides it out again.
        _ => [0.0, f64::from(v.height()) / scale(v), 0.0],
    }
}
/// The rider's own `VEHICLE` attachment height (`ridingOffset`): how far
/// below the vehicle's attachment point its feet hang, in its scale.
/// `vehicle_width` is the vehicle's `getBbWidth`, which a spider's own rule
/// reads.
pub fn vehicle_attachment(r: &Mob, vehicle_width: f32) -> f64 {
    unscaled_vehicle_attachment(r, vehicle_width) * scale(r)
}
fn unscaled_vehicle_attachment(r: &Mob, vehicle_width: f32) -> f64 {
    use Species as S;
    // Avatar.DEFAULT_VEHICLE_ATTACHMENT.
    if avatar(r) {
        return f64::from(0.6_f32);
    }
    // `Spider.getVehicleAttachmentPoint` and `CaveSpider.getVehicleAttachmentPoint`:
    // a vehicle no wider than the rider takes the spider's fixed seat, and a
    // wider one the type's default. A cave spider's super call reaches the
    // spider's rule, which then sees the same width and takes the default too.
    if vehicle_width <= r.width() {
        match r.species() {
            S::CaveSpider => return 0.21875,
            S::Spider => return 0.3125,
            _ => {}
        }
    }
    match r.species() {
        S::Zombie | S::Husk | S::Drowned if r.is_baby() => 0.1875,
        S::ZombieVillager if r.is_baby() => 0.125,
        // `Piglin.BABY_DIMENSIONS`' vehicle attachment.
        S::Piglin if r.is_baby() => 0.1875,
        S::Zombie
        | S::Husk
        | S::Drowned
        | S::ZombieVillager
        | S::ZombifiedPiglin
        | S::Piglin
        | S::PiglinBrute
        | S::Skeleton
        | S::Stray
        | S::Bogged
        | S::Parched => f64::from(0.7_f32),
        S::WitherSkeleton => f64::from(0.875_f32),
        // `ridingOffset(0.5F)` and kin: a negative seat offset lifts the rider.
        S::Ghast | S::HappyGhast => f64::from(-0.5_f32),
        S::Allay | S::Vex => f64::from(-0.04_f32),
        S::Phantom => f64::from(0.125_f32),
        S::Giant => f64::from(3.75_f32),
        S::Pillager | S::Vindicator | S::Evoker | S::Illusioner => f64::from(0.6_f32),
        _ => 0.0,
    }
}
/// `Vec3.yRot(-yaw)` of an attachment point.
fn rotated([x, y, z]: [f64; 3], yaw: f32) -> [f64; 3] {
    let a = -f64::from(yaw).to_radians();
    let (sin, cos) = (a.sin(), a.cos());
    [x * cos + z * sin, y, z * cos - x * sin]
}
/// `Entity.positionRider`: the vehicle's position plus its rotated passenger
/// attachment, less the rider's own vehicle attachment.
pub fn seat(attachment: [f64; 3], rider_offset: f64, at: Vec3, yaw: f32) -> Vec3 {
    let [x, y, z] = rotated(attachment, yaw);
    Vec3::new(at.x + x, at.y + y - rider_offset, at.z + z)
}

/// `Entity.startRiding`: the link it makes, `rider` on `vehicle` as its first
/// passenger (`addPassenger`).
pub fn link<M: super::MobSlot>(mobs: &mut [M], rider: u32, vehicle: u32) {
    if rider == vehicle {
        return;
    }
    if let Some(m) = mobs
        .iter_mut()
        .map(super::MobSlot::mob_mut)
        .find(|m| m.id == rider)
    {
        super::shulker::on_start_riding(m);
        m.extra.mount = Some(vehicle);
        m.extra.passenger = true;
    }
    if let Some(m) = mobs
        .iter_mut()
        .map(super::MobSlot::mob_mut)
        .find(|m| m.id == vehicle)
        && m.extra.rider.is_none()
    {
        m.extra.rider = Some(rider);
    }
}
/// `canAddPassenger` with `aboard` passengers on `v`: a camel takes another
/// while it carries at most two (`Camel`: `getPassengers().size() <= 2`, a baby
/// too), a happy ghast up to four, anything else only when empty (`Entity`).
pub fn can_add_passenger(v: &Mob, aboard: usize) -> bool {
    match v.species() {
        s if super::camel::is_camel(s) => aboard <= 2,
        Species::HappyGhast => aboard < 4,
        _ => aboard == 0,
    }
}
/// `Piglin.getTopPassenger`: down the chain of first passengers from
/// `vehicle`, stopping at the `counter`th seat.
fn top_passenger<M: super::MobSlot>(mobs: &[M], vehicle: u32, counter: i32) -> u32 {
    let first = mobs
        .iter()
        .map(super::MobSlot::mob)
        .find(|m| m.id == vehicle)
        .and_then(|m| m.extra.rider);
    match first {
        Some(passenger) if counter != 1 => top_passenger(mobs, passenger, counter - 1),
        _ => vehicle,
    }
}
/// `Piglin.startRiding` over `Entity.startRiding` for two mobs: a baby piglin
/// that boards a hoglin sits on the top of up to three piglins stacked on it.
/// Not the vehicle it is on already, nor one it carries itself, and only
/// after its boarding cooldown, onto a vehicle with no passenger
/// (`canRide`, `canAddPassenger`). Returns whether it boarded.
pub fn start_riding<M: super::MobSlot>(mobs: &mut [M], rider: u32, vehicle: u32) -> bool {
    // `Shulker.startRiding` puts its attach face back before the ride is
    // decided, so a shulker that is refused still ends up lid up.
    if let Some(m) = mobs
        .iter_mut()
        .map(super::MobSlot::mob_mut)
        .find(|m| m.id == rider)
    {
        super::shulker::on_start_riding(m);
    }
    let Some(r) = mobs.iter().map(super::MobSlot::mob).find(|m| m.id == rider) else {
        return false;
    };
    let on_hoglin = mobs
        .iter()
        .map(super::MobSlot::mob)
        .any(|m| m.id == vehicle && m.species() == Species::Hoglin);
    let vehicle = if r.species() == Species::Piglin && r.is_baby() && on_hoglin {
        top_passenger(mobs, vehicle, MAX_PASSENGERS_ON_ONE_HOGLIN)
    } else {
        vehicle
    };
    if rider == vehicle || r.extra.mount == Some(vehicle) {
        return false;
    }
    // Nothing that rides the rider (through any number of vehicles) can be
    // its vehicle.
    let mut up = Some(vehicle);
    while let Some(v) = up {
        up = mobs
            .iter()
            .map(super::MobSlot::mob)
            .find(|m| m.id == v)
            .and_then(|m| m.extra.mount);
        if up == Some(rider) {
            return false;
        }
    }
    let Some(v) = mobs
        .iter()
        .map(super::MobSlot::mob)
        .find(|m| m.id == vehicle)
    else {
        return false;
    };
    let count = usize::from(v.rider.is_some())
        + mobs
            .iter()
            .map(super::MobSlot::mob)
            .filter(|m| m.extra.mount == Some(vehicle))
            .count()
            .max(usize::from(v.extra.rider.is_some()));
    if r.extra.boarding_cooldown > 0 || !can_add_passenger(v, count) {
        return false;
    }
    link(mobs, rider, vehicle);
    true
}
/// `LivingEntity.stopRiding` for the mob itself: it leaves its vehicle (which
/// forgets it when the riders are next synced), cannot board again for 60
/// ticks (`Entity.removePassenger`) and is set down at `at`, where
/// `dismountVehicle` puts it (`dismountTo`), or stays where it is.
pub fn stop_riding(m: &mut Mob, at: Option<Vec3>) {
    if m.extra.mount.take().is_none() {
        return;
    }
    m.extra.passenger = false;
    m.extra.boarding_cooldown = BOARDING_COOLDOWN;
    if let Some(at) = at {
        m.position = at;
        m.previous = at;
    }
}
/// `Animal.getDismountLocationForPassenger` for a standing passenger of
/// `width` by `height`: on the floor of the first block around the
/// vehicle's, going right, left, behind right, behind left, ahead right,
/// ahead left, behind and ahead of its facing, where the passenger's box
/// fits (`DismountHelper.canDismountTo`, without the world border); else on
/// top of the vehicle. A piglin is no `Animal`: it keeps
/// `Entity.getDismountLocationForPassenger`, on top of it.
pub fn dismount_location(w: &MobWorld, vehicle: &Seen, width: f32, height: f32) -> Vec3 {
    // `Entity.getDismountLocationForPassenger`.
    let on_top = Vec3::new(
        vehicle.position.x,
        vehicle.bounds.max_position().y,
        vehicle.position.z,
    );
    if vehicle.species == Species::Piglin {
        return on_top;
    }
    let (bx, by, bz) = (
        vehicle.position.x.floor() as i32,
        vehicle.position.y.floor() as i32,
        vehicle.position.z.floor() as i32,
    );
    // `Direction.fromYRot` of the vehicle's yaw: south, west, north, east.
    let facing = (f64::from(vehicle.yaw) / 90.0 + 0.5).floor() as i32;
    let (fx, fz) = [(0, 1), (-1, 0), (0, -1), (1, 0)][(facing & 3) as usize];
    // `DismountHelper.offsetsForDirection`; `right` is `Direction.getClockWise`.
    let (rx, rz) = (-fz, fx);
    let offsets = [
        (rx, rz),
        (-rx, -rz),
        (rx - fx, rz - fz),
        (-rx - fx, -rz - fz),
        (fx + rx, fz + rz),
        (fx - rx, fz - rz),
        (-fx, -fz),
        (fx, fz),
    ];
    // `EntityDimensions.makeBoundingBox`: the half width is a float.
    let half = f64::from(width / 2.0);
    for (dx, dz) in offsets {
        let (x, z) = (bx + dx, bz + dz);
        let floor = w.block_floor_height(x, by, z);
        // `DismountHelper.isBlockFloorValid`.
        if floor.is_infinite() || floor >= 1.0 {
            continue;
        }
        // `Vec3.upFromBottomCenterOf`.
        let at = Vec3::new(
            f64::from(x) + 0.5,
            f64::from(by) + floor,
            f64::from(z) + 0.5,
        );
        let bounds = Aabb::new(
            Vec3::new(at.x - half, at.y, at.z - half),
            Vec3::new(at.x + half, at.y + f64::from(height), at.z + half),
        );
        if w.no_collision(bounds) {
            return at;
        }
    }
    on_top
}

/// Ranged riders keep this far from their target instead of closing in.
const RANGED_DISTANCE: f64 = 8.0;
fn ranged(s: Species) -> bool {
    matches!(
        s,
        Species::Skeleton
            | Species::Stray
            | Species::Bogged
            | Species::Parched
            | Species::Pillager
            | Species::Witch
            | Species::Piglin
    )
}
/// `Mob.getControllingPassenger`: NoAI belongs to the vehicle, not its rider.
fn controller<'a>(m: &Mob, w: &'a MobWorld) -> Option<&'a Seen> {
    if m.no_ai {
        return None;
    }
    m.extra.rider.and_then(|id| w.seen(id)).filter(|r| {
        r.alive
            && !w.data.entity_has(
                "minecraft:non_controlling_rider",
                &format!("minecraft:{}", super::species::def(r.species).name),
            )
    })
}
/// Vehicle goal flags are independent of whether the rider has a combat target.
/// Ravager.updateControlFlags permits raiders, unlike Mob.updateControlFlags.
pub fn drive(m: &mut Mob, w: &MobWorld) {
    let rider = controller(m, w);
    let disabled = rider
        .is_some_and(|r| m.species() != Species::Ravager || !super::raider::is_raider(r.species));
    let flags = CONTROLLED
        | if m.species() == Species::Ravager {
            TARGET
        } else {
            0
        };
    if disabled {
        m.ai.goals.disabled |= flags;
    } else {
        m.ai.goals.disabled &= !flags;
    }
    // Keep the existing combat steering and ranged stopping distance.
    let Some((seen, t)) = rider.and_then(|r| {
        r.target
            .and_then(|target| w.target_info(target))
            .filter(|t| t.alive && !(t.is_player && t.creative))
            .map(|t| (r, t))
    }) else {
        return;
    };
    m.look_at_speed([t.position.x, t.eye_y, t.position.z], 30.0, 30.0);
    let close =
        ranged(seen.species) && m.distance_sqr(t.position) < RANGED_DISTANCE * RANGED_DISTANCE;
    if close {
        m.nav_stop();
    } else if m.ai.nav.is_done() || (m.age + m.id).is_multiple_of(10) {
        m.nav_move_to_entity(w, [t.position.x, t.position.y, t.position.z], 0, 1.0);
    }
}
pub fn mob_controlled(m: &Mob, w: &MobWorld) -> bool {
    controller(m, w).is_some()
}
/// `Mob.getControllingPassenger` is `rider`: the vehicle's first passenger (a
/// player aboard sits first, `Entity.addPassenger`), the vehicle not NoAI,
/// and a rider able to steer (`Entity.canControlVehicle`: not tagged
/// `non_controlling_rider`). The overrides in `AbstractHorse`, `Pig`,
/// `Strider`, `HappyGhast` and `AbstractNautilus` fall back to it for a mob.
pub fn controls_vehicle(
    vehicle: &Mob,
    rider: &Mob,
    player_aboard: bool,
    non_controlling: bool,
) -> bool {
    !vehicle.no_ai && !player_aboard && !non_controlling && vehicle.extra.rider == Some(rider.id)
}
/// Whether a mob rider takes its mob vehicle's `yBodyRot` each tick, as
/// 26.3 does in `AbstractHorse.positionRider` and `Chicken.positionRider`
/// (any living rider), and in `AbstractSkeleton.rideTick` and
/// `Drowned.rideTick` for a rider steering a `PathfinderMob`
/// (`getControlledVehicle`: the vehicle's `getControllingPassenger`, which
/// `controlling` says). Any other rider's body turns on its own
/// (`BodyRotationControl`).
pub fn takes_vehicle_body(vehicle: &Mob, rider: &Mob, controlling: bool) -> bool {
    let steers = matches!(
        rider.species(),
        Species::Skeleton
            | Species::Stray
            | Species::Bogged
            | Species::Parched
            | Species::WitherSkeleton
            | Species::Drowned
    );
    // Bat, Ghast and Phantom are the Mobs that are not PathfinderMobs (the
    // ender dragon is no `Mob` here).
    let pathfinder = !matches!(
        vehicle.species(),
        Species::Bat | Species::Ghast | Species::Phantom
    );
    super::equine::is_equine(vehicle.species())
        || vehicle.species() == Species::Chicken
        || (steers && controlling && pathfinder)
}
/// `LivingEntityRenderer.solveBodyRot` for the rider of a living vehicle:
/// the body drawn from the vehicle's, at most 85 degrees off the head, and
/// past 50 degrees turned a fifth of the way back toward it.
pub fn solve_body_rot(head: f32, vehicle_body: f32) -> f32 {
    let diff = super::living::wrap(head - vehicle_body).clamp(-85.0, 85.0);
    let mut body = head - diff;
    if diff.abs() > 50.0 {
        body += diff * 0.2;
    }
    body
}

/// Tick a passenger with Mob.getNavigation's recursive vehicle redirection.
/// Navigation is lent, not copied: goal start/stop/path progress all address
/// the same object the vehicle ticks. The pathfinder sees the vehicle body.
pub fn tick<M: super::MobSlot>(
    mobs: &mut [M],
    index: usize,
    w: &MobWorld,
    random: &mut horizon_core::random::LegacyRandom,
    events: &mut Vec<super::MobEvent>,
) -> bool {
    let mut owner = index;
    if !mobs[index].mob().no_ai {
        for _ in 0..mobs.len() {
            let Some(vehicle) = mobs[owner]
                .mob()
                .extra
                .mount
                .and_then(|id| mobs.iter().position(|m| m.mob().id == id))
            else {
                break;
            };
            if vehicle == index
                || controller(mobs[vehicle].mob(), w).is_none_or(|r| r.id != mobs[owner].mob().id)
            {
                break;
            }
            owner = vehicle;
        }
    }
    if owner == index {
        return mobs[index].mob_mut().tick(w, random, events);
    }
    let (rider, vehicle) = if index < owner {
        let (left, right) = mobs.split_at_mut(owner);
        (left[index].mob_mut(), right[0].mob_mut())
    } else {
        let (left, right) = mobs.split_at_mut(index);
        (right[0].mob_mut(), left[owner].mob_mut())
    };
    let body = vehicle.path_mob();
    std::mem::swap(&mut rider.ai.nav, &mut vehicle.ai.nav);
    rider.ai.navigation_redirect = Some(Box::new(super::nav::Redirect {
        own: std::mem::take(&mut vehicle.ai.nav),
        body,
        follow_range: vehicle.attrs.follow_range as f32,
        passenger: vehicle.extra.mount.is_some(),
        in_liquid: vehicle.is_in_liquid(),
    }));
    let kept = rider.tick(w, random, events);
    let redirect = rider
        .ai
        .navigation_redirect
        .take()
        .expect("navigation loan lasts for the tick");
    vehicle.ai.nav = std::mem::replace(&mut rider.ai.nav, redirect.own);
    kept
}
/// `Mob.requiresCustomPersistence`: a passenger never despawns.
pub fn keeps_alive(m: &Mob) -> bool {
    m.extra.mount.is_some()
}

/// `IsChickenJockey`, `SkeletonTrap` and `SkeletonTrapTime`.
pub fn write_nbt(m: &Mob, map: &mut Compound) {
    match m.species() {
        Species::Chicken => {
            map.insert(
                "IsChickenJockey".into(),
                Tag::Byte(i8::from(m.extra.jockey)),
            );
        }
        Species::SkeletonHorse => {
            map.insert("SkeletonTrap".into(), Tag::Byte(i8::from(m.extra.trap)));
            map.insert("SkeletonTrapTime".into(), Tag::Int(m.extra.trap_time));
        }
        _ => {}
    }
}
pub fn read_nbt(m: &mut Mob, tag: &Tag) {
    let int = |key: &str| match tag.get(key) {
        Some(Tag::Int(v)) => *v,
        Some(Tag::Byte(v)) => i32::from(*v),
        Some(Tag::Short(v)) => i32::from(*v),
        _ => 0,
    };
    match m.species() {
        Species::Chicken => m.extra.jockey = int("IsChickenJockey") != 0,
        Species::SkeletonHorse => {
            m.extra.trap = int("SkeletonTrap") != 0;
            m.extra.trap_time = int("SkeletonTrapTime");
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachments_turn_with_the_vehicle() {
        // A ravager's seat sits a sixteenth behind its centre.
        let behind = [0.0, 2.2625, -0.0625];
        let [x, y, z] = rotated(behind, 0.0);
        assert!(x.abs() < 1e-9 && (z + 0.0625).abs() < 1e-9 && y == 2.2625);
        // Facing west (yaw 90), behind is to the east.
        let [x, _, z] = rotated(behind, 90.0);
        assert!((x - 0.0625).abs() < 1e-9 && z.abs() < 1e-9);
    }
    fn of(id: u32, species: Species) -> Mob {
        let mut kind = (*crate::mobs::sensor_scan_tests::kind()).clone();
        kind.species = species;
        Mob::new(id, std::sync::Arc::new(kind), Vec3::ZERO, 0.0)
    }
    /// 26.3 bytecode (`javap -c`): `AbstractHorse.positionRider` and
    /// `Chicken.positionRider` set every living rider's `yBodyRot` to the
    /// vehicle's; `AbstractSkeleton.rideTick` and `Drowned.rideTick` take it
    /// from a `PathfinderMob` they control; no other vehicle or rider does.
    #[test]
    fn only_horses_chickens_and_steering_skeletons_take_the_vehicles_body() {
        let takes = |vehicle, rider, controlling| {
            takes_vehicle_body(&of(1, vehicle), &of(2, rider), controlling)
        };
        for vehicle in [
            Species::Horse,
            Species::SkeletonHorse,
            Species::Camel,
            Species::Llama,
            Species::Chicken,
        ] {
            assert!(takes(vehicle, Species::Zombie, false), "{vehicle:?}");
        }
        assert!(takes(Species::Spider, Species::Skeleton, true));
        assert!(takes(Species::Spider, Species::Drowned, true));
        assert!(!takes(Species::Spider, Species::Skeleton, false));
        // Not a PathfinderMob.
        assert!(!takes(Species::Ghast, Species::Skeleton, true));
        // Their own BodyRotationControl turns these.
        assert!(!takes(Species::Pig, Species::Zombie, true));
        assert!(!takes(Species::Ravager, Species::Pillager, true));
        assert!(!takes(Species::Hoglin, Species::Piglin, true));
        assert!(!takes(Species::Strider, Species::ZombifiedPiglin, true));
    }
    /// `Mob.getControllingPassenger` (26.3 bytecode): the first passenger,
    /// and only while no player is aboard, the vehicle has AI and the rider
    /// can steer.
    #[test]
    fn a_skeleton_steers_its_spider_only_as_its_first_passenger() {
        let mut spider = of(1, Species::Spider);
        let skeleton = of(2, Species::Skeleton);
        spider.extra.rider = Some(2);
        assert!(controls_vehicle(&spider, &skeleton, false, false));
        assert!(!controls_vehicle(&spider, &skeleton, true, false));
        assert!(!controls_vehicle(&spider, &skeleton, false, true));
        spider.extra.rider = Some(3);
        assert!(!controls_vehicle(&spider, &skeleton, false, false));
        spider.extra.rider = Some(2);
        spider.no_ai = true;
        assert!(!controls_vehicle(&spider, &skeleton, false, false));
    }
    /// `LivingEntityRenderer.extractRenderState` draws a rider of a living
    /// vehicle from `solveBodyRot` (the vehicle's body, interpolated with
    /// `rotLerp`), and anything else from its own body.
    #[test]
    fn a_rider_of_a_living_vehicle_is_drawn_from_the_vehicles_body() {
        let mut m = of(2, Species::Pillager);
        m.body_yaw = 0.0;
        m.previous_body_yaw = 0.0;
        m.head_yaw = 100.0;
        m.previous_head_yaw = 100.0;
        m.extra.mount = Some(7);
        m.extra.vehicle_body = Some((7, 30.0, 50.0));
        let close = |a: f32, b: f32| (a - b).abs() < 1e-4;
        // Halfway, the vehicle's body is at 40: the head is 60 off, so the
        // body is 85-clamped to 40 and turned a fifth of 60 toward the head.
        assert!(close(super::super::anim::body_rot(&m, 0.5), 52.0));
        assert!(close(super::super::anim::state(&m, 0.5).y_rot, 48.0));
        // Off that vehicle (or onto a boat), its own body again.
        m.extra.mount = Some(8);
        assert!(close(super::super::anim::body_rot(&m, 0.5), 0.0));
        m.extra.mount = None;
        assert!(close(super::super::anim::body_rot(&m, 0.5), 0.0));
    }
    fn npc(scale: f32) -> Mob {
        let mut kind = (*crate::mobs::sensor_scan_tests::kind()).clone();
        kind.id = "minecraft:player".into();
        let mut m = Mob::new(1, std::sync::Arc::new(kind), Vec3::ZERO, 0.0);
        m.extra.model_scale = scale;
        m
    }
    /// An NPC plugin's enlarged fake player: the name it carries sits on the
    /// top of its scaled box, and on a seat it hangs `Avatar`'s 0.6 lower, scaled too.
    #[test]
    fn a_scaled_player_carries_and_sits_in_its_scale() {
        let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
        assert!(close(passenger_attachment(&npc(0.0))[1], 1.8));
        assert!(close(passenger_attachment(&npc(1.5))[1], 2.7));
        assert!(close(vehicle_attachment(&npc(0.0), 1.375), 0.6));
        assert!(close(vehicle_attachment(&npc(1.5), 1.375), 0.9));
        // On a seat at y 55 (a text display, whose attachment is its feet):
        // a 1.5 NPC's feet at 54.1, where the server puts it.
        let seat = seat(
            [0.0; 3],
            vehicle_attachment(&npc(1.5), 1.375),
            Vec3::new(0.0, 55.0, 0.0),
            0.0,
        );
        assert!(close(seat.y, 54.1));
    }
    /// A scaled vehicle that is not an avatar: `remote_mob_attributes` puts
    /// `minecraft:scale` on any living entity, so the seat rides at the scaled
    /// box height on a cow as on a fake player.
    #[test]
    fn a_scaled_vehicle_seat_stays_scaled() {
        let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
        let mut cow = Mob::new(1, crate::mobs::sensor_scan_tests::kind(), Vec3::ZERO, 0.0);
        // `EntityTypes.COW`: passengerAttachments(1.36875F).
        assert!(close(passenger_attachment(&cow)[1], 1.36875));
        cow.extra.model_scale = 2.0;
        assert!(close(passenger_attachment(&cow)[1], 2.7375));
    }
    /// The attachments are `EntityTypes` floats widened to double, and a
    /// zombie's seat (2.0125) is above its box (1.95).
    #[test]
    fn seats_are_the_entity_type_floats() {
        let close = |a: f64, b: f64| (a - b).abs() < 1e-12;
        assert!(close(up(0.86875)[1], 0.868_749_976_158_142_1));
        assert!(close(f64::from(0.7_f32), 0.699_999_988_079_071));
        assert!(close(
            up(2.0125)[1] - f64::from(0.7_f32),
            1.312_500_059_604_645
        ));
    }
    /// `Pig.getDefaultDimensions`: BABY_DIMENSIONS overrides PASSENGER.
    #[test]
    fn a_baby_pig_uses_its_own_passenger_attachment() {
        let mut kind = (*crate::mobs::sensor_scan_tests::kind()).clone();
        kind.id = "minecraft:pig".into();
        kind.species = Species::Pig;
        let kind = std::sync::Arc::new(kind);
        let mut vehicle = Mob::new(1, kind.clone(), Vec3::new(4.5, 200.0, 4.5), 0.0);
        let rider = Mob::new(2, kind, Vec3::ZERO, 0.0);
        for (scale, baby_y, adult_y) in [
            (0.0, 0.5, 0.868_749_976_158_142_1),
            (1.0, 0.5, 0.868_749_976_158_142_1),
            (2.0, 1.0, 1.737_499_952_316_284_2),
        ] {
            vehicle.extra.model_scale = scale;
            for (age, expected) in [(-24000, baby_y), (-1, baby_y), (0, adult_y), (42, adult_y)] {
                vehicle.extra.age = age;
                let attachment = passenger_attachment(&vehicle);
                assert_eq!(attachment, [0.0, expected, 0.0], "age={age}, scale={scale}");
                let offset = vehicle_attachment(&rider, vehicle.width());
                assert_eq!(offset, 0.0);
                assert_eq!(
                    seat(attachment, offset, vehicle.position, vehicle.yaw),
                    Vec3::new(4.5, 200.0 + expected, 4.5)
                );
            }
        }
    }

    #[test]
    fn archers_keep_their_distance_and_brawlers_close_in() {
        assert!(ranged(Species::Skeleton) && ranged(Species::Pillager));
        assert!(!ranged(Species::Zombie) && !ranged(Species::Vindicator));
    }
    /// `Spider.getVehicleAttachmentPoint` and
    /// `CaveSpider.getVehicleAttachmentPoint`: a vehicle no wider than the
    /// spider takes the spider's fixed seat — 0.3125 of its scale, and a cave
    /// spider's 0.21875 — and a wider one the type's default.
    #[test]
    fn a_spider_sits_by_its_own_rule_on_a_narrow_vehicle() {
        // World-gated: without the extracted data the body never runs.
        let _ = crate::mobs::world::with_test_world(&[], |_, types| {
            let new = |kind: &str| {
                Mob::new(
                    1,
                    std::sync::Arc::new(types.types[kind].clone()),
                    Vec3::ZERO,
                    0.0,
                )
            };
            let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
            let chicken = 0.4_f32;
            // Wider than either spider (the boat's 1.375 is not).
            let wide = 2.0_f32;
            let spider = new("minecraft:spider");
            let cave = new("minecraft:cave_spider");
            assert!(spider.width() > chicken && cave.width() > chicken);
            assert!(spider.width() < wide && cave.width() < wide);
            assert!(close(vehicle_attachment(&spider, chicken), 0.3125));
            assert!(close(vehicle_attachment(&cave, chicken), 0.21875));
            assert!(close(vehicle_attachment(&spider, wide), 0.0));
            assert!(close(vehicle_attachment(&cave, wide), 0.0));
            let mut big = new("minecraft:cave_spider");
            big.extra.model_scale = 2.0;
            assert!(close(vehicle_attachment(&big, chicken), 0.4375));
        });
    }

    /// `Shulker.startRiding`: `setAttachFace(Direction.DOWN)` runs before
    /// `Entity.startRiding` decides, so a shulker that boards a vehicle sits
    /// lid up whatever face it was stuck to - and one whose board is refused
    /// still ends up lid up. No other rider's face data moves.
    #[test]
    fn a_shulker_that_boards_sits_lid_up() {
        let mob = |id: u32, name: &str, species: Species, face: u8| {
            let mut kind = (*crate::mobs::sensor_scan_tests::kind()).clone();
            kind.id = name.into();
            kind.species = species;
            let mut m = Mob::new(id, std::sync::Arc::new(kind), Vec3::ZERO, 0.0);
            m.extra.variant3 = face;
            m
        };
        // Attached to the block to its east (face 5) and saddled by nobody.
        let mut mobs = [
            mob(1, "minecraft:shulker", Species::Shulker, 5),
            mob(2, "minecraft:pig", Species::Pig, 0),
        ];
        assert!(start_riding(&mut mobs, 1, 2), "the shulker boards");
        assert_eq!(mobs[0].extra.variant3, 0, "riding lid up");
        assert_eq!(mobs[1].extra.rider, Some(1));

        // A vehicle that already carries someone refuses the board, but the
        // face has already gone back to `DOWN`.
        let mut refused = [
            mob(1, "minecraft:shulker", Species::Shulker, 3),
            mob(2, "minecraft:pig", Species::Pig, 0),
            mob(9, "minecraft:zombie", Species::Zombie, 0),
        ];
        refused[1].extra.rider = Some(9);
        assert!(!start_riding(&mut refused, 1, 2));
        assert_eq!(refused[0].extra.variant3, 0);
        assert_eq!(refused[0].extra.mount, None);

        // `Entity.startRiding` for anything else leaves the data alone.
        let mut others = [
            mob(1, "minecraft:cow", Species::Cow, 4),
            mob(2, "minecraft:pig", Species::Pig, 0),
        ];
        assert!(start_riding(&mut others, 1, 2));
        assert_eq!(others[0].extra.variant3, 4);

        // A camel takes riders while it carries at most two: the first stays
        // extra.rider, and a fourth is refused.
        let mut camel_ride = [
            mob(1, "minecraft:husk", Species::Husk, 0),
            mob(2, "minecraft:parched", Species::Parched, 0),
            mob(3, "minecraft:zombie", Species::Zombie, 0),
            mob(4, "minecraft:husk", Species::Husk, 0),
            mob(10, "minecraft:camel_husk", Species::CamelHusk, 0),
        ];
        assert!(start_riding(&mut camel_ride, 1, 10), "first rider boards");
        assert_eq!(camel_ride[4].extra.rider, Some(1), "first rider is driver");
        assert!(start_riding(&mut camel_ride, 2, 10), "second rider boards");
        assert_eq!(camel_ride[1].extra.mount, Some(10));
        assert_eq!(camel_ride[4].extra.rider, Some(1), "driver stays");
        assert!(start_riding(&mut camel_ride, 3, 10), "a third still fits");
        assert!(!start_riding(&mut camel_ride, 4, 10), "a fourth is refused");
        assert_eq!(camel_ride[3].extra.mount, None);

        // Any other mob carries one.
        let mut single = [
            mob(1, "minecraft:zombie", Species::Zombie, 0),
            mob(2, "minecraft:husk", Species::Husk, 0),
            mob(10, "minecraft:chicken", Species::Chicken, 0),
        ];
        assert!(start_riding(&mut single, 1, 10));
        assert!(!start_riding(&mut single, 2, 10));
    }
}

#[cfg(test)]
mod happy_ghast_seat_tests {
    use super::happy_ghast_attachment;

    #[test]
    fn passengers_take_separate_seats() {
        assert_eq!(happy_ghast_attachment(0), [0.0, 4.0, f64::from(1.7_f32)]);
        let seats: Vec<_> = (0..4).map(happy_ghast_attachment).collect();
        for (i, a) in seats.iter().enumerate() {
            assert!(seats[i + 1..].iter().all(|b| a != b));
        }
        assert_eq!(happy_ghast_attachment(9), seats[3]);
    }
}
