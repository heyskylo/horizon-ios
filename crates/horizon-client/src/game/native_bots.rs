//! Server-side player bots for mods (docs/native-mods.md, "Server hooks for plugin ports: player
//! bots"): a mod that drives a server player itself (a fake client sending its own moves) asks
//! where the player's movement code would take them in one tick, and which block shapes are in
//! a box. Nothing runs unless a mod calls it, and neither changes the player or the level.
use super::Game;
use horizon_client::collisions::SceneCollisions;
use horizon_client::mod_api::{NativePlayerInput, NativeSimulatedMove};
use horizon_client::motion::{MotionWorld, PlayerMotion};
use horizon_client::sprint::Keys;
use horizon_core::geometry::{Aabb, Vec3};
use horizon_world::block_collision_scan::BlockCollisionScan;
use horizon_world::collision_context::CollisionContext;

/// The most block cells `native_collision_boxes` looks at: a box of about 100 blocks a side.
const COLLISION_CELLS: usize = 1_000_000;

/// The keys a bot's input presses: the direction flags by sign (sprint and the server's input
/// packet read them), and the exact move vector when it isn't a key's (a stick's).
fn input_keys(input: &NativePlayerInput) -> Option<Keys> {
    if !(input.forward.is_finite() && input.strafe.is_finite()) {
        return None;
    }
    let forward = input.forward.clamp(-1.0, 1.0);
    let strafe = input.strafe.clamp(-1.0, 1.0);
    let key = |v: f32| v == 0.0 || v.abs() == 1.0;
    Some(Keys {
        forward: forward > 0.0,
        backward: forward < 0.0,
        left: strafe > 0.0,
        right: strafe < 0.0,
        jump: input.jump,
        shift: input.sneak,
        sprint: input.sprint,
        analog: (!key(forward) || !key(strafe)).then_some([strafe, forward]),
    })
}

/// `LocalPlayer.aiStep`'s sprint: it needs a forward impulse (`hasForwardImpulse`).
fn sprints(input: &NativePlayerInput) -> bool {
    input.sprint && input.forward > 1.0e-5
}

/// A box from `[min_x, min_y, min_z, max_x, max_y, max_z]`, when its corners are numbers in order.
fn aabb_of(b: [f64; 6]) -> Option<Aabb> {
    (b.iter().all(|v| v.is_finite()) && b[0] <= b[3] && b[1] <= b[4] && b[2] <= b[5])
        .then(|| Aabb::new(Vec3::new(b[0], b[1], b[2]), Vec3::new(b[3], b[4], b[5])))
}

fn corners(b: Aabb) -> [f64; 6] {
    let (lo, hi) = (b.min_position(), b.max_position());
    [lo.x, lo.y, lo.z, hi.x, hi.y, hi.z]
}

// Native mods use these; nothing in Horizon does.
impl Game<'_> {
    /// One tick of the server player `uuid`'s own movement from `input`, as their client would
    /// play it (`LocalPlayer.aiStep` and `Player.travel`): walking and strafing (`forward` and
    /// `strafe`, -1..1), jumping, sneaking (slower, and held at edges), sprinting (with a
    /// forward input only), looking `yaw` and `pitch`; gravity, friction, block speed factors,
    /// stepping up, swimming and climbing as the server's movement prediction replays a client
    /// tick. It starts from where the player is now, with `input.velocity` when given (the
    /// velocity the last call returned: the server doesn't keep a bot's own), else the server's
    /// copy. Nothing is changed: the player stays where they are until the mod moves them (a
    /// move packet from its bot, `native_teleport_player`), and the level is only read.
    ///
    /// None off a server, for a player not here (another level than the one in place, a
    /// guest out of reach inside another player's hook, where the hub is taken out), dead, a
    /// spectator or riding, an input that is not a number, and when the movement code fails.
    /// API @API:player_bots@.
    pub(super) fn native_simulate_player_move(
        &mut self,
        uuid: u128,
        input: NativePlayerInput,
    ) -> Option<NativeSimulatedMove> {
        if !self.native_authoritative() || !(input.yaw.is_finite() && input.pitch.is_finite()) {
            return None;
        }
        if input
            .velocity
            .is_some_and(|v| !v.iter().all(|c| c.is_finite()))
        {
            return None;
        }
        let keys = input_keys(&input)?;
        let mut body = self.native_bot_body(uuid)?;
        body.yaw = input.yaw;
        body.pitch = input.pitch.clamp(-90.0, 90.0);
        if let Some([x, y, z]) = input.velocity {
            body.velocity = Vec3::new(x, y, z);
        }
        // Game.sync_player_body's part that depends on the blocks, as the replay does.
        body.nudge = super::closest_space_nudge(&body, &self.world, self.res.facts());
        MotionWorld {
            entities: &[],
            player: &mut body,
            scene: &mut self.world,
            assets: &self.pack.assets,
            collision_binding: self.collision,
            resources: self.motion,
            health: &self.health_resources,
        }
        .replayed_tick(keys, sprints(&input))
        .ok()?;
        let (p, v) = (body.position, body.velocity);
        Some(NativeSimulatedMove {
            position: [p.x, p.y, p.z],
            velocity: [v.x, v.y, v.z],
            on_ground: body.ground.on_ground,
            horizontal_collision: body.collision.horizontal,
            sprinting: body.sprinting,
        })
    }

    /// A copy of the body of server player `uuid` in the level in place, as it stands now: the
    /// acting player's swapped in, or a guest's as `lan_actions` swaps it in.
    fn native_bot_body(&self, uuid: u128) -> Option<PlayerMotion> {
        if self.lan_acting_uuid == Some(uuid) {
            self.lan_acting?;
            let p = self.player.as_ref()?;
            return (p.health.health() > 0.0 && !p.spectator && self.riding.is_none())
                .then(|| p.clone());
        }
        if self.lan_acting.is_some() {
            return None;
        }
        let g = self.lan.as_ref()?.hub.guests.iter().find(|g| {
            g.placed
                && !g.dead
                && g.uuid == uuid
                && g.dimension == self.dim.id()
                && g.vehicle.is_none()
                && !g.spectator()
        })?;
        let mut body = g.motion.as_deref()?.clone();
        body.position = Vec3::new(g.position[0], g.position[1], g.position[2]);
        body.ground.on_ground = g.on_ground;
        body.yaw = g.yaw;
        body.pitch = g.pitch;
        body.sprinting = g.sprinting;
        body.may_fly = g.creative();
        body.flying = g.flying;
        body.flying_speed = g.flying_speed;
        body.fall_flying = g.fall_flying;
        Some(body)
    }

    /// The block collision boxes in `aabb` (`[min_x, min_y, min_z, max_x, max_y, max_z]`) of the
    /// level `dimension`, as `Level.getBlockCollisions(null, aabb)` gives them: each block's
    /// collision shape that shares volume with the box, as world-space boxes (a stair gives two,
    /// a fence its post), under the empty collision context (no entity: scaffolding and powder
    /// snow as for nothing standing on them). Entities and the world border are not in it. For a
    /// bot's own collision checks and pathfinding. Empty off a server, for another level than the
    /// one in place, a box that isn't a box (not numbers, or min past max), and one over about a
    /// million blocks. API @API:player_bots@.
    pub(super) fn native_collision_boxes(&self, dimension: &str, aabb: [f64; 6]) -> Vec<[f64; 6]> {
        let Some(bounds) = aabb_of(aabb) else {
            return Vec::new();
        };
        if !self.native_authoritative() || dimension != self.dim.id() {
            return Vec::new();
        }
        let mut scene = SceneCollisions {
            world: &self.world,
            binding: self.collision,
            subject: None,
        };
        let Ok(scan) = BlockCollisionScan::new(
            &mut scene,
            CollisionContext::empty(false),
            bounds,
            false,
            COLLISION_CELLS,
        ) else {
            return Vec::new();
        };
        let mut boxes = Vec::new();
        for hit in scan {
            let Ok(hit) = hit else {
                return Vec::new();
            };
            boxes.extend(hit.shape.boxes().iter().copied().map(corners));
        }
        boxes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(forward: f32, strafe: f32) -> NativePlayerInput {
        NativePlayerInput {
            forward,
            strafe,
            ..Default::default()
        }
    }

    #[test]
    fn keys_follow_the_signs() {
        let k = input_keys(&input(1.0, -1.0)).unwrap();
        assert!(k.forward && !k.backward && k.right && !k.left);
        assert!(
            k.analog.is_none(),
            "full keys use the keyboard's own vector"
        );
        let k = input_keys(&input(-1.0, 0.0)).unwrap();
        assert!(k.backward && !k.forward && !k.left && !k.right);
    }

    #[test]
    fn partial_input_is_a_stick() {
        let k = input_keys(&input(0.5, 0.0)).unwrap();
        assert!(k.forward);
        assert_eq!(k.analog, Some([0.0, 0.5]));
        let k = input_keys(&input(3.0, -7.0)).unwrap();
        assert!(k.analog.is_none(), "clamped to full keys");
    }

    #[test]
    fn non_numbers_are_refused() {
        assert!(input_keys(&input(f32::NAN, 0.0)).is_none());
        assert!(input_keys(&input(0.0, f32::INFINITY)).is_none());
    }

    #[test]
    fn sprint_needs_forward() {
        let mut i = input(0.0, 1.0);
        i.sprint = true;
        assert!(!sprints(&i));
        i.forward = -1.0;
        assert!(!sprints(&i));
        i.forward = 1.0;
        assert!(sprints(&i));
        i.sprint = false;
        assert!(!sprints(&i));
    }

    #[test]
    fn boxes_round_trip_and_bad_boxes_are_refused() {
        let b = [0.0, 1.0, 2.0, 3.0, 4.5, 6.0];
        assert_eq!(corners(aabb_of(b).unwrap()), b);
        assert!(aabb_of([1.0, 0.0, 0.0, 0.0, 1.0, 1.0]).is_none());
        assert!(aabb_of([0.0, f64::NAN, 0.0, 1.0, 1.0, 1.0]).is_none());
    }
}
