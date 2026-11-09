//! NPCs for native server mods (docs/native-mods.md, "Server hooks for plugin
//! ports: NPCs"), what an NPC plugin (Citizens, Sentinel) or a stage plugin
//! needs from the server: spawn into any loaded level, find an entity by its
//! UUID, and drive a mob as vanilla's goals do. They act where the world is
//! simulated, never on a client's copy, and reach any level the server has
//! loaded by its `dimension` (`minecraft:the_nether`), not only the one in
//! place: that level is put in place for the call, as `execute in` does, and
//! the one that was is put back. A level nobody opened is not opened.
use super::{Game, commands::Target, creatures::seen};
use horizon_client::mobs::{Mob, MobWorld, Seen, Who, ai::do_hurt_target, goals::Cx};
use horizon_world::nbt::Compound;

// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// The entity with `uuid` in any loaded level (Bukkit's
    /// `Server.getEntity(UUID)`, `ServerLevel.getEntity(UUID)` over every
    /// level): its dimension, its id there and its type (`minecraft:zombie`).
    /// A server's player is found too (`minecraft:player`, in its level). None
    /// on a client, for no such entity, and for a player while a guest's
    /// packet is handled (the hub is taken out then): call it from a tick or
    /// console hook. API @API:npc_entity_by_uuid@.
    pub(super) fn native_entity_by_uuid(&mut self, uuid: u128) -> Option<(String, u32, String)> {
        if self.remote.is_some() || self.options.execution_role.is_client() {
            return None;
        }
        let player = self.lan.as_ref().and_then(|h| {
            let g = h.hub.guests.iter().find(|g| g.placed && g.uuid == uuid)?;
            Some((g.dimension.clone(), u32::try_from(g.entity_id).ok()?))
        });
        if let Some((dimension, id)) = player {
            return Some((dimension, id, "minecraft:player".into()));
        }
        let words = horizon_client::lan::playerdata::uuid_ints(uuid);
        let dims: Vec<&'static str> = std::iter::once(self.dim)
            .chain(self.levels.parked_dims())
            .map(|d| d.id())
            .collect();
        for dimension in dims {
            let found = self.npc_in_level(dimension, |g| {
                // The UUIDs the level remembers, checked against its entities: a
                // remembered id may belong to an entity that is gone.
                let id = g
                    .persist
                    .uuids
                    .iter()
                    .find(|(_, w)| **w == words)
                    .map(|(id, _)| *id)?;
                let t = g.level_entity(i32::try_from(id).ok()?)?;
                if let Target::Mob(m) = t
                    && !g.mobs.iter().any(|x| x.id == m && !x.removed)
                {
                    return None;
                }
                // The mod API has no natural fence knots (#13036).
                if let Target::Knot(_) = t {
                    return None;
                }
                Some((id, g.target_type(t)))
            });
            if let Some(Some((id, kind))) = found {
                return Some((dimension.to_owned(), id, kind));
            }
        }
        None
    }
    /// Merges `nbt` into entity `id`'s data in the loaded level `dimension`,
    /// as `/execute in <dimension> run data merge entity` does: what
    /// `native_entity_merge_nbt` does to the level in place, for any loaded
    /// level (with `native_entity_by_uuid`, an entity found by its UUID).
    /// False on a client, for a level not loaded, no such entity, or what
    /// `native_entity_merge_nbt` refuses (a player, a kind Horizon can't
    /// reload yet, data that changes nothing). API @API:npc_entity_by_uuid@.
    pub(super) fn native_merge_entity_nbt(
        &mut self,
        dimension: &str,
        id: u32,
        nbt: Compound,
    ) -> bool {
        self.npc_in_level(dimension, |g| {
            g.native_entity(id)
                .is_some_and(|e| g.native_entity_merge_nbt(e, nbt))
        })
        .unwrap_or(false)
    }
    /// Turns entity `id` of the loaded level `dimension` to `yaw` and `pitch`
    /// in degrees, as `/rotate` does (`Entity.forceSetRotation`: `setYRot`,
    /// `setXRot` clamped to -90..90, its body and head with it), without
    /// moving it; `head_yaw` then turns a mob's head alone
    /// (`setYHeadRot`). For a mob, a mannequin, an armor stand, a boat, a
    /// minecart, evoker fangs and the entities Horizon keeps without
    /// simulating them (displays, interactions). A mob whose AI runs turns
    /// back by its own look and body control on its next tick: face a point
    /// with `native_mob_look_at` instead, or spawn it with `no_ai`. False on
    /// a client, for a level not loaded, no such entity (a player is turned
    /// by teleporting them), another kind, or an angle that isn't finite.
    /// API @API:npc_entity_rotation@.
    pub(super) fn native_set_entity_rotation(
        &mut self,
        dimension: &str,
        id: u32,
        yaw: f32,
        pitch: f32,
        head_yaw: Option<f32>,
    ) -> bool {
        if !yaw.is_finite() || !pitch.is_finite() || head_yaw.is_some_and(|h| !h.is_finite()) {
            return false;
        }
        self.npc_in_level(dimension, |g| {
            let Some(t) = i32::try_from(id).ok().and_then(|id| g.level_entity(id)) else {
                return false;
            };
            let turns = match t {
                Target::Mob(id) => g.mobs.iter().any(|m| m.id == id && !m.removed),
                Target::Other(_)
                | Target::Stand(_)
                | Target::Boat(_)
                | Target::Cart(_)
                | Target::Fangs(_) => true,
                _ => false,
            };
            if !turns {
                return false;
            }
            g.set_rotation(t, [yaw, pitch]);
            if let (Target::Mob(id), Some(head)) = (t, head_yaw)
                && let Some(m) = g.mobs.iter_mut().find(|m| m.id == id)
            {
                m.head_yaw = head;
                m.previous_head_yaw = head;
            }
            true
        })
        .unwrap_or(false)
    }
    /// Walks mob `mob` of the loaded level `dimension` towards `to` with the
    /// engine's pathfinding, as `PathNavigation.moveTo(x, y, z, reach,
    /// speed)` does: a path to the block holding `to` (a walker's onto the
    /// ground below it), done within `reach` blocks, followed at `speed`
    /// times the mob's movement speed. True when a path was found and the
    /// mob is on its way; false for no path (within its follow range, as
    /// vanilla's), a level not loaded, no such living mob, or a value that
    /// isn't finite. The path goes round walls and through open doors as the
    /// mob's own navigation allows. The mob's goals may take the navigation
    /// over, as they do from vanilla's callers: call it again (an NPC
    /// library's navigator does so each tick) or give the mob goals of its
    /// own (`native_register_goals`). A mob with `no_ai` keeps the path but
    /// doesn't walk it. API @API:npc_mob_navigation@.
    pub(super) fn native_mob_navigate(
        &mut self,
        dimension: &str,
        mob: u32,
        to: [f64; 3],
        reach: i32,
        speed: f64,
    ) -> bool {
        if !to.into_iter().all(f64::is_finite) || !speed.is_finite() || reach < 0 {
            return false;
        }
        let at = to.map(|v| v.floor() as i32);
        self.npc_drive(dimension, mob, |m, w| {
            let path = m.create_path_to(w, at, reach);
            m.nav_move_to_path(w, path, speed)
        })
        .unwrap_or(false)
    }
    /// The walkable path mob `mob` of the loaded level `dimension` would take
    /// to `to` (`PathNavigation.createPath`, the engine's own pathfinder for
    /// that mob's kind), without moving it: the block positions from the
    /// mob's own block on, in walking order, and whether the path reaches
    /// `to` within `reach` blocks. When `to` can't be reached, the path ends at
    /// the nearest node vanilla reached, with `reached` false. None for no path
    /// at all (beyond the follow range, as vanilla's), a level not loaded, no
    /// such living mob, a negative `reach` or a value that isn't finite. Nothing
    /// is set on the mob: pass the nodes to a mob's own walk or to your NPC's
    /// movement yourself. API @API:npc_mob_path@.
    pub(super) fn native_mob_path(
        &mut self,
        dimension: &str,
        mob: u32,
        to: [f64; 3],
        reach: i32,
    ) -> Option<(Vec<[i32; 3]>, bool)> {
        if !to.into_iter().all(f64::is_finite) || reach < 0 {
            return None;
        }
        let at = to.map(|v| v.floor() as i32);
        self.npc_drive(dimension, mob, |m, w| {
            m.create_path_to(w, at, reach).map(|path| {
                let nodes = path.nodes.iter().map(|n| [n.x, n.y, n.z]).collect();
                (nodes, path.reached)
            })
        })
        .flatten()
    }
    /// Strikes lightning at `pos` in the loaded level `dimension`, as `/summon
    /// lightning_bolt` does; with `visual_only` only the bolt is shown, with no
    /// fire, damage or sound-of-strike event (`EntityType.LIGHTNING_BOLT`'s
    /// `visualOnly`). False for a non-finite position and a level not loaded or
    /// a client. API @API:living_helpers@.
    pub(super) fn native_lightning_bolt(
        &mut self,
        dimension: &str,
        pos: [f64; 3],
        visual_only: bool,
    ) -> bool {
        if !pos.into_iter().all(f64::is_finite) {
            return false;
        }
        self.npc_in_level(dimension, |g| g.spawn_bolt(pos, visual_only))
            .is_some()
    }
    /// Walks mob `mob` towards where entity `target` (a mob, a player or any
    /// other entity of the same level) stands now, as
    /// `PathNavigation.moveTo(entity, speed)` does (done within a block). A
    /// path to where it is now: to follow it, call it again as it moves
    /// (vanilla's follow and attack goals ask every few ticks). False as
    /// `native_mob_navigate`, or for no such target in that level. API
    /// @API:npc_mob_navigation@.
    pub(super) fn native_mob_follow(
        &mut self,
        dimension: &str,
        mob: u32,
        target: u32,
        speed: f64,
    ) -> bool {
        if !speed.is_finite() || mob == target {
            return false;
        }
        self.npc_in_level(dimension, |g| {
            let Some(at) = g.npc_entity_position(target) else {
                return false;
            };
            g.npc_drive(dimension, mob, |m, w| m.nav_move_to_entity(w, at, 1, speed))
                .unwrap_or(false)
        })
        .unwrap_or(false)
    }
    /// Stops mob `mob`'s walk (`PathNavigation.stop`): false for a level not
    /// loaded or no such living mob. API @API:npc_mob_navigation@.
    pub(super) fn native_mob_stop(&mut self, dimension: &str, mob: u32) -> bool {
        self.npc_drive(dimension, mob, |m, _| m.nav_stop())
            .is_some()
    }
    /// Whether mob `mob` is walking a path (`PathNavigation.isInProgress`):
    /// false once it arrived, got stuck or was stopped, and for a level not
    /// loaded or no such living mob. API @API:npc_mob_navigation@.
    pub(super) fn native_mob_navigating(&mut self, dimension: &str, mob: u32) -> bool {
        self.npc_in_level(dimension, |g| {
            g.mobs
                .iter()
                .any(|m| m.id == mob && !m.removed && m.alive() && m.ai.nav.is_in_progress())
        })
        .unwrap_or(false)
    }
    /// Turns mob `mob`'s head towards `at` at its own head speeds, as
    /// `LookControl.setLookAt(x, y, z)` does: the head turns on the mob's
    /// next tick and the body follows it, as for vanilla's look goals. It
    /// holds for that tick only, so call it each tick to keep looking; a mob
    /// with `no_ai` doesn't turn (`native_set_entity_rotation` turns it).
    /// False for a level not loaded, no such living mob or a point that isn't
    /// finite. API @API:npc_mob_navigation@.
    pub(super) fn native_mob_look_at(&mut self, dimension: &str, mob: u32, at: [f64; 3]) -> bool {
        if !at.into_iter().all(f64::is_finite) {
            return false;
        }
        self.npc_drive(dimension, mob, |m, _| m.look_at(at))
            .is_some()
    }
    /// Mob `mob` strikes entity `target` of its level with its own attack, as
    /// `Mob.doHurtTarget` does: its attack damage, held weapon and
    /// enchantments, knockback, the species' extras (a cave spider's poison,
    /// a husk's hunger) and its attack sound, through the damage the game
    /// deals any mob's hit. `target` is a living mob or a server's player in
    /// the same level, by entity id. No reach or sight check and no swing,
    /// as in vanilla, where the attack goal checks and swings first. True
    /// when the blow was dealt (whether it hurt is the damage hooks' to
    /// tell); false for a level not loaded, no such living mob or target, the
    /// mob itself, or a player while a guest's packet is handled (the hub is
    /// taken out then). API @API:npc_mob_attack@.
    pub(super) fn native_mob_attack(&mut self, dimension: &str, mob: u32, target: u32) -> bool {
        if mob == target {
            return false;
        }
        self.npc_in_level(dimension, |g| g.npc_mob_attack(mob, target))
            .unwrap_or(false)
    }
    /// `native_mob_attack` in the level in place.
    fn npc_mob_attack(&mut self, mob: u32, target: u32) -> bool {
        let live = |m: &Mob, id: u32| m.id == id && !m.removed && m.alive();
        let Some(i) = self.mobs.iter().position(|m| live(m, mob)) else {
            return false;
        };
        let guest = self.lan.as_ref().and_then(|h| {
            h.hub
                .guests
                .iter()
                .find(|g| {
                    h.hub.here(g) && g.loaded && !g.dead && u32::try_from(g.entity_id) == Ok(target)
                })
                .map(|g| g.conn)
        });
        // The mob code sees one player (`Who::Player`): the guest hit, as in a
        // mob's tick that sees them.
        let (who, player) = match guest {
            Some(conn) => match self.lan_guest_view(conn) {
                Some(view) => (Who::Player, Some(view)),
                None => return false,
            },
            None if self.mobs.iter().any(|m| live(m, target)) => (Who::Mob(target), None),
            None => return false,
        };
        let uuid = self.level.player_uuid;
        let snapshot: Vec<Seen> = self.mobs.iter().map(|m| seen(m, uuid)).collect();
        let mut events = Vec::new();
        let mut random = self.world.random.clone();
        let mut mobs = std::mem::take(&mut self.mobs);
        {
            let mut w = self.mob_world_base(&snapshot);
            if player.is_some() {
                w.player = player;
            }
            let mut cx = Cx {
                w: &w,
                lr: &mut random,
                events: &mut events,
            };
            do_hurt_target(&mut mobs[i], who, &mut cx);
        }
        self.world.random = random;
        self.mobs = mobs;
        // What acts on the player goes to the guest, the rest as a tick's events.
        let (to_guest, events): (Vec<_>, Vec<_>) = events
            .into_iter()
            .partition(|e| guest.is_some() && super::lan_mobs::acts_on_player(e));
        if let Err(e) = self.handle_mob_events(events) {
            log::warn!("mob {mob} attacking {target}: {e}");
        }
        if let Some(conn) = guest {
            self.lan_guest_mob_events(to_guest.into_iter().map(|e| (conn, mob, e)).collect());
        }
        true
    }
    /// Runs `f` on the living mob `mob` of the loaded level `dimension`, with
    /// the world its goals see. The mob is out of the level's list meanwhile,
    /// as in its own tick.
    fn npc_drive<R>(
        &mut self,
        dimension: &str,
        mob: u32,
        f: impl FnOnce(&mut Mob, &MobWorld<'_>) -> R,
    ) -> Option<R> {
        self.npc_in_level(dimension, |g| {
            let i = g
                .mobs
                .iter()
                .position(|m| m.id == mob && !m.removed && m.alive())?;
            let mut mobs = std::mem::take(&mut g.mobs);
            let out = f(&mut mobs[i], &g.mob_world_base(&[]));
            g.mobs = mobs;
            Some(out)
        })
        .flatten()
    }
    /// Where entity `id` of the level in place stands: a server's player in
    /// it, or any entity of its lists.
    fn npc_entity_position(&self, id: u32) -> Option<[f64; 3]> {
        let player = self.lan.as_ref().and_then(|h| {
            h.hub
                .guests
                .iter()
                .find(|g| h.hub.here(g) && u32::try_from(g.entity_id) == Ok(id))
                .map(|g| g.position)
        });
        if player.is_some() {
            return player;
        }
        let t = self.level_entity(i32::try_from(id).ok()?)?;
        if let Target::Mob(m) = t
            && !self.mobs.iter().any(|x| x.id == m && !x.removed)
        {
            return None;
        }
        // The mod API has no natural fence knots (#13036).
        if let Target::Knot(_) = t {
            return None;
        }
        self.target_position(t)
    }
}

impl Game<'_> {
    /// Runs `f` with the loaded level `dimension` in place, then puts back
    /// the level that was (`execute in <dimension> run ...`): None on a
    /// client, or for a dimension the server hasn't loaded. The level in place
    /// runs `f` as it is. Not a helper of its own (a closure isn't one): the
    /// helpers that reach any loaded level go through it.
    pub(super) fn npc_in_level<R>(
        &mut self,
        dimension: &str,
        f: impl FnOnce(&mut Self) -> R,
    ) -> Option<R> {
        if self.remote.is_some() || self.options.execution_role.is_client() {
            return None;
        }
        if dimension == self.dim.id() {
            return Some(f(self));
        }
        let dim = self
            .levels
            .parked_dims()
            .into_iter()
            .find(|d| d.id() == dimension)?;
        let back = self.enter_command_level(dim)?;
        let out = f(self);
        self.enter_command_level(back);
        Some(out)
    }
}
