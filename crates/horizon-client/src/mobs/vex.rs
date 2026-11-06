//! Vexes (`Vex`): summoned by evokers, flying through walls around their
//! bound origin, charging at their target, and wasting away after their
//! limited life.
use super::{
    Damage, DamageKind, Mob, MobEvent, Species, Who,
    anim::{Poses, State},
    control::Operation,
    goals::{
        Cx, Float, Goal, HurtByTarget, LookAtPlayer, MOVE, NearestTarget, goal_clone, reduced,
    },
};
use horizon_core::geometry::trig::Mth;
use horizon_core::{geometry::Vec3, random::RandomSource};
use std::f32::consts::PI;

// Vex state: `flag` charging, `counter2` the owner's mob id (0 none),
// `home` the bound origin, `egg_time` the life left (-1 unlimited).

pub fn register(m: &mut Mob) {
    m.no_gravity = true;
    m.ai.mov.kind = super::control::MoveKind::Custom;
    m.extra.set_main_hand(Some("minecraft:iron_sword".into()));
    if m.extra.egg_time == 0 {
        m.extra.egg_time = -1;
    }
    let g = &mut m.ai.goals;
    g.add(0, Float);
    g.add(4, Charge);
    g.add(8, RandomMove);
    g.add(9, LookAtPlayer::new(3.0).probability(1.0));
    g.add(10, LookAtPlayer::mobs(8.0));
    // `HurtByTargetGoal(this, Raider.class)`: raiders are ignored, vexes are not.
    let mut hurt = HurtByTarget::new().alert();
    hurt.ignore = vec![
        Species::Pillager,
        Species::Vindicator,
        Species::Evoker,
        Species::Illusioner,
        Species::Witch,
        Species::Ravager,
    ];
    m.ai.targets.add(1, hurt);
    m.ai.targets.add(2, CopyOwnerTarget);
    m.ai.targets.add(3, NearestTarget::player(true));
}
fn has_wanted(m: &Mob) -> bool {
    m.ai.mov.operation == Operation::MoveTo
}
fn set_wanted(m: &mut Mob, at: [f64; 3], speed: f64) {
    m.ai.mov.wanted = at;
    m.ai.mov.speed_modifier = speed;
    m.ai.mov.operation = Operation::MoveTo;
}
/// `VexChargeAttackGoal`.
#[derive(Clone, Debug)]
struct Charge;
impl Goal for Charge {
    goal_clone!();
    fn flags(&self) -> u8 {
        MOVE
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        let Some(t) = m.target_info(cx.w).filter(|t| t.alive) else {
            return false;
        };
        !has_wanted(m)
            && m.random.next_int_bound(reduced(7)).unwrap_or(1) == 0
            && m.distance_sqr(t.position) > 4.0
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        has_wanted(m) && m.extra.flag && m.target_info(cx.w).is_some_and(|t| t.alive)
    }
    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        if let Some(t) = m.target_info(cx.w) {
            set_wanted(m, [t.position.x, t.eye_y, t.position.z], 1.0);
        }
        m.extra.flag = true;
        cx.events.push(MobEvent::Sound(
            "entity.vex.charge",
            [m.position.x, m.position.y, m.position.z],
            1.0,
            1.0,
        ));
    }
    fn stop(&mut self, m: &mut Mob, _cx: &mut Cx) {
        m.extra.flag = false;
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let (Some(who), Some(t)) = (m.ai.target, m.target_info(cx.w)) else {
            return;
        };
        if m.bounds().intersects(t.bounds) {
            super::ai::do_hurt_target(m, who, cx);
            m.extra.flag = false;
        } else if m.distance_sqr(t.position) < 9.0 {
            set_wanted(m, [t.position.x, t.eye_y, t.position.z], 1.0);
        }
    }
}
/// `VexRandomMoveGoal`: drift to an empty block near the bound origin.
#[derive(Clone, Debug)]
struct RandomMove;
impl Goal for RandomMove {
    goal_clone!();
    fn flags(&self) -> u8 {
        MOVE
    }
    fn can_use(&mut self, m: &mut Mob, _cx: &mut Cx) -> bool {
        !has_wanted(m) && m.random.next_int_bound(reduced(7)).unwrap_or(1) == 0
    }
    fn can_continue(&mut self, _m: &mut Mob, _cx: &mut Cx) -> bool {
        false
    }
    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        // Ticked once: the goal never continues.
        let origin = m.extra.home.unwrap_or([
            m.position.x.floor() as i32,
            m.position.y.floor() as i32,
            m.position.z.floor() as i32,
        ]);
        for _ in 0..3 {
            let r = &mut m.random;
            let p = [
                origin[0] + r.next_int_bound(15).unwrap_or(7) - 7,
                origin[1] + r.next_int_bound(11).unwrap_or(5) - 5,
                origin[2] + r.next_int_bound(15).unwrap_or(7) - 7,
            ];
            if cx.w.facts.is_air(cx.w.world.block(p[0], p[1], p[2])) {
                let at = [
                    f64::from(p[0]) + 0.5,
                    f64::from(p[1]) + 0.5,
                    f64::from(p[2]) + 0.5,
                ];
                set_wanted(m, at, 0.25);
                if m.ai.target.is_none() {
                    m.look_at_speed(at, 180.0, 20.0);
                }
                break;
            }
        }
    }
}
/// `Vex.getOwner`: the mob the vex is bound to. `counter2` holds its id while
/// the evoker that summoned it is in the level; a vex loaded from a save knows
/// only the UUID it was saved with, and vanilla's `EntityReference.getEntity`
/// asks the level for that entity every time it is called
/// (`MobWorld::who_by_uuid`), so an owner that loaded later is found too.
pub fn owner(m: &Mob, lookup: Option<&dyn super::WhoLookup>) -> Option<u32> {
    if m.extra.counter2 != 0 {
        return Some(m.extra.counter2 as u32);
    }
    match lookup?.who_by_uuid(m.extra.owner?)? {
        Who::Mob(id) => Some(id),
        // `Vex.setOwner` takes a `Mob`: a UUID naming the player is no owner.
        Who::Player => None,
    }
}
/// `VexCopyOwnerTargetGoal`: the evoker's target.
#[derive(Clone, Debug)]
struct CopyOwnerTarget;
impl Goal for CopyOwnerTarget {
    goal_clone!();
    fn flags(&self) -> u8 {
        super::goals::TARGET
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        let seen = owner(m, cx.w.who_lookup).and_then(|owner| {
            cx.w.others
                .iter()
                .find(|s| s.id == owner)
                .and_then(|s| s.target)
        });
        seen.is_some_and(|t| t != Who::Mob(m.id))
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        m.target_info(cx.w).is_some_and(|t| t.alive)
    }
    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        m.ai.target = owner(m, cx.w.who_lookup).and_then(|owner| {
            cx.w.others
                .iter()
                .find(|s| s.id == owner)
                .and_then(|s| s.target)
        });
    }
}
/// `VexMoveControl`: accelerate straight at the wanted point.
pub fn move_control(m: &mut Mob) {
    if m.ai.mov.operation != Operation::MoveTo {
        return;
    }
    let [wx, wy, wz] = m.ai.mov.wanted;
    let d = Vec3::new(wx - m.position.x, wy - m.position.y, wz - m.position.z);
    let len = (d.x * d.x + d.y * d.y + d.z * d.z).sqrt();
    let b = m.bounds();
    let (lo, hi) = (b.min_position(), b.max_position());
    let size = ((hi.x - lo.x) + (hi.y - lo.y) + (hi.z - lo.z)) / 3.0;
    if len < size {
        m.ai.mov.operation = Operation::Wait;
        m.velocity = m.velocity.scale(0.5);
        return;
    }
    m.velocity = m
        .velocity
        .add_vector(d.scale(m.ai.mov.speed_modifier * 0.05 / len));
    let yaw = match m.extra.point.filter(|_| m.ai.target.is_some()) {
        Some(t) => -((t[0] - m.position.x).atan2(t[2] - m.position.z) as f32).to_degrees(),
        None => -(m.velocity.x.atan2(m.velocity.z) as f32).to_degrees(),
    };
    m.yaw = yaw;
    m.body_yaw = yaw;
}
/// `Vex` travel with `noPhysics`: straight through blocks, air friction.
pub fn travel(m: &mut Mob) -> bool {
    m.position = m.position.add_vector(m.velocity);
    m.velocity = Vec3::new(
        m.velocity.x * 0.91,
        m.velocity.y * 0.98,
        m.velocity.z * 0.91,
    );
    m.on_ground = false;
    true
}
/// `Vex.tick`: the limited life costs a heart a second once spent.
pub fn ai_step(m: &mut Mob, w: &super::MobWorld) {
    m.extra.point = m
        .target_info(w)
        .map(|t| [t.position.x, t.position.y, t.position.z]);
    if m.extra.egg_time > 0 {
        m.extra.egg_time -= 1;
        if m.extra.egg_time == 0 {
            m.extra.egg_time = 20;
            m.damage(1.0, Damage::of(DamageKind::Generic));
        }
    }
}
/// `VexModel.setupAnim`.
pub fn anim(m: &Mob, s: &State, p: &mut Poses) {
    p.yrot("head", s.y_rot.to_radians());
    p.xrot("head", s.x_rot.to_radians());
    let bob = (s.age * 5.5 * horizon_core::geometry::trig::DEG_TO_RAD).mcos() * 0.1;
    p.zrot("right_arm", PI / 5.0 + bob);
    p.zrot("left_arm", -(PI / 5.0 + bob));
    if m.extra.flag {
        p.xrot("body", 0.0);
        // setArmsCharging
        let (right, left) = (m.extra.main_hand.is_some(), m.extra.off_hand.is_some());
        if !right && !left {
            p.xrot("right_arm", -1.221_730_5);
            p.yrot("right_arm", PI / 12.0);
            p.zrot("right_arm", -0.471_238_88 - bob);
            p.xrot("left_arm", -1.221_730_5);
            p.yrot("left_arm", -PI / 12.0);
            p.zrot("left_arm", 0.471_238_88 + bob);
        } else {
            let sword = (std::f64::consts::PI * 7.0 / 6.0) as f32;
            if right {
                p.xrot("right_arm", sword);
                p.yrot("right_arm", PI / 12.0);
                p.zrot("right_arm", -0.471_238_88 - bob);
            }
            if left {
                p.xrot("left_arm", sword);
                p.yrot("left_arm", -PI / 12.0);
                p.zrot("left_arm", 0.471_238_88 + bob);
            }
        }
    } else {
        p.xrot("body", PI / 20.0);
    }
    let deg = horizon_core::geometry::trig::DEG_TO_RAD;
    let wing = 1.099_557_4 + (s.age * 45.836_624 * deg).mcos() * deg * 16.2;
    p.yrot("left_wing", wing);
    p.yrot("right_wing", -wing);
    p.xrot("left_wing", 0.471_238_88);
    p.zrot("left_wing", -0.471_238_88);
    p.xrot("right_wing", 0.471_238_88);
    p.zrot("right_wing", 0.471_238_88);
}
/// The vex's texture: brighter while charging.
pub fn texture(m: &Mob) -> &'static str {
    if m.extra.flag {
        "entity/illager/vex_charging"
    } else {
        "entity/illager/vex"
    }
}

#[cfg(test)]
mod owner_save_tests {
    use super::*;
    use crate::entity_nbt::{mob_from_nbt, mob_to_nbt};
    use crate::mobs::WhoLookup;
    use horizon_world::nbt::Tag;
    use std::collections::HashMap;
    use std::sync::Arc;

    /// A vex with the id `EVOKER` in the level: what `Vex.setOwner` records
    /// when an evoker summons it.
    const EVOKER: u32 = 7;

    fn vex() -> Mob {
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        let ty = Arc::make_mut(&mut kind);
        ty.id = "minecraft:vex".into();
        ty.species = Species::Vex;
        Mob::new(1, kind, Vec3::ZERO, 0.0)
    }

    /// `Level.getEntity(uuid)` for the test: `Game` answers this with
    /// `Game::who_by_uuid`.
    struct Level(HashMap<[i32; 4], Who>);
    impl WhoLookup for Level {
        fn who_by_uuid(&self, uuid: [i32; 4]) -> Option<Who> {
            self.0.get(&uuid).copied()
        }
    }

    /// `Vex.addAdditionalSaveData` stores its owner reference's UUID
    /// (`EntityReference.store`, nothing at all without an owner) and
    /// `readAdditionalSaveData` reads it back (`EntityReference.read`), so a
    /// summoned vex keeps the evoker whose target it copies across a chunk
    /// save. Horizon models the owner as the owner's mob id and wrote neither,
    /// so a reloaded vex had no owner to copy a target from.
    #[test]
    fn a_vexs_owner_uuid_survives_the_chunk_save() {
        let evoker_uuid = [3, 1, 4, 1];
        let mut summoned = vex();
        assert_eq!(owner(&summoned, None), None, "a vex nobody summoned");
        summoned.extra.counter2 = EVOKER as i32;
        assert_eq!(owner(&summoned, None), Some(EVOKER), "the evoker's id");
        let uuid_of = |id: u32| (id == EVOKER).then_some(evoker_uuid);
        let saved = mob_to_nbt(&summoned, [0; 4], None, None, uuid_of);
        assert_eq!(
            saved.get("owner"),
            Some(&Tag::IntArray(evoker_uuid.to_vec()))
        );
        // The loaded vex knows the UUID: no id survives a save, and its owner
        // is found again only through the level (`EntityReference.getEntity`).
        let types = [("minecraft:vex".into(), summoned.kind.clone())].into();
        let back = mob_from_nbt(&saved, &types, 2, None).unwrap();
        assert_eq!(back.extra.owner, Some(evoker_uuid));
        assert_eq!(back.extra.counter2, 0);
        let level = Level(HashMap::from([(evoker_uuid, Who::Mob(42))]));
        assert_eq!(owner(&back, Some(&level)), Some(42), "the evoker, found");
        // An owner the level does not have, and a UUID naming the player, are
        // no owner: `Vex.setOwner` takes a `Mob`.
        assert_eq!(owner(&back, Some(&Level(HashMap::new()))), None);
        let player = Level(HashMap::from([(evoker_uuid, Who::Player)]));
        assert_eq!(owner(&back, Some(&player)), None);
        assert_eq!(owner(&back, None), None, "a level with no UUIDs");
        // A save read again keeps the reference even when nothing in the level
        // answers for it, as `EntityReference` does.
        let again = mob_to_nbt(&back, [0; 4], None, None, |_| None);
        assert_eq!(
            again.get("owner"),
            Some(&Tag::IntArray(evoker_uuid.to_vec()))
        );
    }

    /// `EntityReference.store` writes nothing for a vex with no owner, and a
    /// save that names none loads with none - neither the `owner` key a loaded
    /// compound carried nor an id left over may stand.
    #[test]
    fn a_vex_with_no_owner_saves_no_owner_key() {
        let mut loose = vex();
        let stale = Tag::compound([
            ("id", Tag::String("minecraft:vex".into())),
            (
                "Pos",
                Tag::List(vec![
                    Tag::Double(0.5_f64.to_bits()),
                    Tag::Double(64.0_f64.to_bits()),
                    Tag::Double(0.5_f64.to_bits()),
                ]),
            ),
            ("owner", Tag::IntArray(vec![9, 9, 9, 9])),
        ]);
        let types = [("minecraft:vex".into(), loose.kind.clone())].into();
        let loaded = mob_from_nbt(&stale, &types, 3, None).unwrap();
        assert_eq!(loaded.extra.owner, Some([9, 9, 9, 9]));
        loose.extra.owner = None;
        loose.extra.counter2 = 0;
        let saved = mob_to_nbt(&loose, [0; 4], Some(&stale), None, |_| None);
        assert_eq!(saved.get("owner"), None, "no owner, no key");
    }
}
