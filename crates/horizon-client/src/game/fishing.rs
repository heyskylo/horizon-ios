//! Fishing: `FishingRodItem.use` (cast / reel in), the `FishingHook` (flight,
//! bobbing, lure and bite timers, open water, hooking mobs, retrieving the
//! `gameplay/fishing` loot) and `FishingHookRenderer` (bobber and line).
use super::{Game, lan::LanHost, lan_mobs::guest_bounds, view_vector};
use horizon_client::advancements::EntityView;
use horizon_client::lan::{entity::States, hub::Hub};
use horizon_client::{
    blocks::Fluid,
    collisions::SceneCollisions,
    gizmos::Line,
    items::ItemStack,
    mesher::Vertex,
    particles::{ParticleOptions, ParticleType},
    pose::Pose,
    random_source,
};
use horizon_core::{
    geometry::{Aabb, Vec3, traverse_blocks, trig},
    position::BlockPos,
    random::{LegacyRandom, RandomSource},
};
use horizon_protocol::entity_data::{Value, serializer};
use std::{collections::HashMap, sync::Arc};

use super::lan_actions::GuestPlay;

#[cfg(test)]
#[path = "fishing_block_hit_tests.rs"]
mod block_hit_tests;
#[cfg(test)]
#[path = "fishing_medium_tests.rs"]
mod medium_tests;

/// The table a catch rolls where no mod's fishing medium names one.
const FISHING_TABLE: &str = "minecraft:gameplay/fishing";

/// `FishingHook.retrieve`: the table's named random sequence, or the level
/// random when a datapack table does not name one.
#[allow(clippy::too_many_arguments)]
fn fishing_loot(
    loot: &horizon_world::loot::LootData,
    table: &str,
    sequences: &mut horizon_client::random_sequences::RandomSequences,
    level_random: &mut LegacyRandom,
    world_seed: i64,
    luck: f32,
    biome: Option<&str>,
    open_water: bool,
) -> Vec<horizon_world::loot::LootStack> {
    match loot.random_sequence(table) {
        Some(sequence) => loot.roll(
            table,
            sequences.get(sequence, world_seed),
            luck,
            biome,
            Some(open_water),
        ),
        None => loot.roll(table, level_random, luck, biome, Some(open_water)),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Flying,
    Bobbing,
    Hooked(u32),
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum EntityCollision {
    Hooked,
    Deflected { first: bool },
}
pub(super) struct Hook {
    /// Its entity id, which a server's players track it under.
    id: u32,
    pub(super) position: Vec3,
    pub(super) previous: Vec3,
    velocity: Vec3,
    state: State,
    last_deflected_by: Option<u32>,
    needs_sync: bool,
    tick_count: i32,
    life: i32,
    on_ground: bool,
    nibble: i32,
    time_until_lured: i32,
    time_until_hooked: i32,
    fish_angle: f32,
    out_of_water: i32,
    open_water: bool,
    luck: i32,
    lure: i32,
    /// The fishing medium it last floated in outside water (`native_fishing_medium`): the
    /// table its catch rolls and the wait it draws. None in water.
    medium: Option<HookMedium>,
    /// The position a server's update put it heading for.
    lerp: HookLerp,
    pub(super) random: LegacyRandom,
}
/// What a hook keeps of the mod's fishing medium it floats in.
#[derive(Clone, Debug, Default, PartialEq)]
struct HookMedium {
    loot_table: Option<String>,
    lure_ticks: Option<(i32, i32)>,
}
/// The medium a mod answered, when the hook is in it: in the block of its
/// surface or below, as a hook is in water anywhere in a water block.
fn inside_medium(
    position: Vec3,
    medium: Option<horizon_client::mod_api::NativeFishingMedium>,
) -> Option<horizon_client::mod_api::NativeFishingMedium> {
    medium.filter(|m| m.surface.is_finite() && position.y < m.surface.floor() + 1.0)
}
/// `catchingFish`'s wait for a fish: vanilla's 100 to 600 ticks, or a medium's
/// own range, less the rod's Lure.
fn lure_wait(random: &mut LegacyRandom, range: Option<(i32, i32)>, lure: i32) -> i32 {
    match range {
        None => 100 + random.next_int_bound(501).unwrap_or(0) - lure,
        Some((min, max)) => {
            let min = min.max(1);
            let max = max.max(min);
            min + random.next_int_bound(max - min + 1).unwrap_or(0) - lure
        }
    }
}
/// A player a hook can catch (`canHitEntity`): their entity id, which
/// guest they are (none: the host) and their box.
pub(super) struct HookTarget {
    id: u32,
    conn: Option<u32>,
    bounds: Aabb,
}
const SIZE: f64 = 0.25;
fn bounds(p: Vec3) -> Aabb {
    let h = SIZE / 2.0;
    Aabb::new(
        Vec3::new(p.x - h, p.y, p.z - h),
        Vec3::new(p.x + h, p.y + SIZE, p.z + h),
    )
}
fn segment_entry(start: Vec3, end: Vec3, bounds: Aabb) -> Option<f64> {
    let min = bounds.min_position();
    let max = bounds.max_position();
    let start = [start.x, start.y, start.z];
    let min = [min.x, min.y, min.z];
    let max = [max.x, max.y, max.z];
    if (0..3).all(|axis| start[axis] >= min[axis] && start[axis] < max[axis]) {
        return None;
    }
    let delta = [end.x - start[0], end.y - start[1], end.z - start[2]];
    let mut entry: f64 = 0.0;
    let mut exit: f64 = 1.0;
    for axis in 0..3 {
        if delta[axis] == 0.0 {
            if start[axis] < min[axis] || start[axis] > max[axis] {
                return None;
            }
            continue;
        }
        let a = (min[axis] - start[axis]) / delta[axis];
        let b = (max[axis] - start[axis]) / delta[axis];
        entry = entry.max(a.min(b));
        exit = exit.min(a.max(b));
        if entry > exit {
            return None;
        }
    }
    (entry <= 1.0 && exit >= 0.0).then_some(entry)
}
/// `ProjectileUtil.computeMargin`: the fishing hook's target-box margin grows
/// to 0.3 blocks over its first eight ticks.
fn entity_hit_margin(tick_count: i32) -> f64 {
    f64::from((tick_count - 2).clamp(0, 6)) / 20.0
}
fn first_entity_hit(
    start: Vec3,
    end: Vec3,
    margin: f64,
    targets: impl IntoIterator<Item = (u32, Aabb)>,
) -> Option<u32> {
    let mut hit = None;
    let mut hit_at = f64::INFINITY;
    for (id, target_bounds) in targets {
        if let Some(at) = segment_entry(
            start,
            end,
            target_bounds.inflate(Vec3::new(margin, margin, margin)),
        ) && at < hit_at
        {
            hit = Some(id);
            hit_at = at;
        }
    }
    hit
}
fn pull_item(item: &mut horizon_client::entities::ItemEntity, delta: Vec3) {
    item.velocity = item.velocity.add_vector(delta);
    // The change is made outside ItemEntity::tick, so its velocity comparison
    // cannot request the immediate motion update for tracking clients.
    item.needs_sync = true;
}
/// `FishingHook.pullEntity`: add a tenth of the vector from the hook to its owner.
fn pull_hooked(velocity: Vec3, hook: Vec3, owner: Vec3) -> Vec3 {
    Vec3::new(
        velocity.x + (owner.x - hook.x) * 0.1,
        velocity.y + (owner.y - hook.y) * 0.1,
        velocity.z + (owner.z - hook.z) * 0.1,
    )
}
/// Minecarts a hook can catch. `AbstractMinecart.isPickable` is true while the
/// cart exists, and `Projectile.canHitEntity` skips the vehicle the owner rides.
fn minecart_hook_targets<'a>(
    carts: impl Iterator<Item = &'a super::minecarts::Minecart> + 'a,
    own_vehicle: Option<u32>,
) -> impl Iterator<Item = (u32, Aabb)> + 'a {
    carts
        .filter(move |c| Some(c.id) != own_vehicle)
        .map(|c| (c.id, c.bounds()))
}
impl Hook {
    fn hit_entity(&mut self, id: u32, deflects: bool) -> EntityCollision {
        if deflects {
            let first = self.last_deflected_by != Some(id);
            if first {
                self.velocity = self.velocity.scale(-0.5);
                self.last_deflected_by = Some(id);
                self.needs_sync = true;
            }
            return EntityCollision::Deflected { first };
        }
        self.state = State::Hooked(id);
        self.velocity = Vec3::ZERO;
        EntityCollision::Hooked
    }

    pub(super) fn bounds(&self) -> Aabb {
        bounds(self.position)
    }
    /// `FishingHook.tick` returns before `applyEffectsFromBlocks` while it
    /// holds an entity.
    pub(super) fn applies_block_effects(&self) -> bool {
        !matches!(self.state, State::Hooked(_))
    }
    /// `FishingHook.catchingFish`: the nibble timer, and DATA_BITING's plunge
    /// (`onSyncedDataUpdated` runs on the server too).
    pub(super) fn trigger_bite(&mut self, game_time: i64) {
        self.nibble = 20 + self.random.next_int_bound(21).unwrap_or(0);
        self.velocity.y = bite_impulse(&mut synchronized_random(self.id, game_time));
    }
}
/// `FishingHook.syncronizedRandom`, reseeded every tick from the hook's UUID and the game
/// time so the server and its clients draw alike. Horizon's hook has no UUID: its entity
/// id stands in. Kept off the level's random, whose draws must stay in vanilla's order.
fn synchronized_random(id: u32, game_time: i64) -> LegacyRandom {
    LegacyRandom::new(i64::from(id) ^ game_time)
}
/// `FishingHook.onSyncedDataUpdated` on DATA_BITING: the downward plunge,
/// `-0.4F * Mth.nextFloat(syncronizedRandom, 0.6F, 1.0F)`, in float as vanilla.
fn bite_impulse(r: &mut impl RandomSource) -> f64 {
    // Mth.nextFloat(random, min, max): `random.nextFloat() * (max - min) + min`.
    f64::from(-0.4f32 * (r.next_float() * (1.0f32 - 0.6f32) + 0.6f32))
}

/// `FishingHook` constructor's three independent triangle draws.
fn cast_velocity(y_sin: f32, y_cos: f32, dir_y: f32, random: &mut LegacyRandom) -> Vec3 {
    let v = Vec3::new(f64::from(-y_sin), f64::from(dir_y), f64::from(-y_cos));
    let len = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
    Vec3::new(
        v.x * (0.6 / len + random.triangle(0.5, 0.0103365)),
        v.y * (0.6 / len + random.triangle(0.5, 0.0103365)),
        v.z * (0.6 / len + random.triangle(0.5, 0.0103365)),
    )
}
/// `FishingHook.onHitBlock`: `getDeltaMovement().normalize().scale(hit.distanceTo(this))`,
/// where `HitResult.distanceTo` is the *squared* distance to the hook: the bobber
/// springs back a little off a wall instead of stopping dead.
fn rebound(velocity: Vec3, hit: Vec3, at: Vec3) -> Vec3 {
    velocity.normalize().scale(hit.distance_squared(at))
}
impl Game<'_> {
    /// Run real `catchingFish` states for the vanilla differential harness.
    pub(super) fn fishing_tick_reference(&mut self) -> Vec<serde_json::Value> {
        self.world.weather.raining = false;
        let mut rows = Vec::new();
        for seed in [0_i64, 1, 42, 123456789, -1] {
            for phase in 0..5 {
                let mut hook = Hook::remote(1, Vec3::new(0.5, 400.0, 0.5));
                hook.random = LegacyRandom::new(seed);
                hook.nibble = if phase == 4 { 2 } else { 0 };
                hook.time_until_lured = if phase == 1 { 2 } else { 0 };
                hook.time_until_hooked = match phase {
                    2 => 3,
                    3 => 1,
                    _ => 0,
                };
                hook.fish_angle = 12.5;
                self.catching_fish(&mut hook, [0, 400, 0]);
                rows.push(serde_json::json!({
                    "seed": seed, "phase": phase, "nibble": hook.nibble,
                    "lured": hook.time_until_lured, "hooked": hook.time_until_hooked,
                    "angle_bits": hook.fish_angle.to_bits() as i32, "next": hook.random.next_long(),
                }));
            }
        }
        rows
    }
    /// Exercise the actual rod-use path with a table without a named sequence.
    pub(super) fn fishing_fallback_reference(&mut self) -> Vec<serde_json::Value> {
        let rod = ItemStack {
            item: Arc::from("minecraft:fishing_rod"),
            count: 1,
            damage: 0,
            extra: None,
        };
        let mut rows = Vec::new();
        for seed in [0_i64, 1, 42, 123456789, -1] {
            let position = self.player.as_ref().expect("harness player").position;
            let mut hook = Hook::remote(1, position);
            hook.nibble = 10;
            hook.random = LegacyRandom::new(seed);
            self.hook = Some(hook);
            let item_start = self.dropped.len();
            let orb_start = self.orbs.len();
            self.world.random = LegacyRandom::new(seed);
            self.use_fishing_rod(&rod);
            let items: Vec<_> = self
                .dropped
                .iter()
                .skip(item_start)
                .map(|item| item.stack.item.to_string())
                .collect();
            let xp: Vec<_> = self
                .orbs
                .iter()
                .skip(orb_start)
                .map(|orb| orb.value)
                .collect();
            rows.push(serde_json::json!({
                "seed": seed, "level_next": self.world.random.next_long(), "items": items, "xp": xp,
            }));
        }
        rows
    }
    /// Exercise `FishingHook.tick` in a still-water block on the server.
    pub(super) fn fishing_bobbing_reference(&mut self) -> Vec<serde_json::Value> {
        let old_position = self.player.as_ref().unwrap().position;
        let block = [
            old_position.x.floor() as i32,
            300,
            old_position.z.floor() as i32,
        ];
        let old_block = self.world.block(block[0], block[1], block[2]);
        let water = self
            .pack
            .assets
            .registry
            .default_state("minecraft:water")
            .unwrap();
        self.level_set(block, water).unwrap();
        assert_eq!(
            self.world.block(block[0], block[1], block[2]),
            water,
            "bobbing fixture water not placed"
        );
        let x = f64::from(block[0]) + 0.5;
        let z = f64::from(block[2]) + 0.5;
        self.player.as_mut().unwrap().position = Vec3::new(x, 300.0, z - 2.0);
        let old_offhand = self.offhand.replace(ItemStack {
            item: Arc::from("minecraft:fishing_rod"),
            count: 1,
            damage: 0,
            extra: None,
        });
        let old_hook = self.hook.take();
        self.world.weather.raining = false;
        let mut rows = Vec::new();
        for seed in [0_i64, 1, 42, 123456789, -1] {
            let mut hook = Hook::remote(1, Vec3::new(x, 300.5, z));
            hook.state = State::Bobbing;
            hook.velocity = Vec3::new(0.1, 0.03, -0.2);
            hook.nibble = 0;
            hook.random = LegacyRandom::new(seed);
            self.hook = Some(hook);
            self.tick_fishing();
            let mut hook = self.hook.take().expect("bobbing hook removed");
            rows.push(serde_json::json!({
                "seed": seed,
                "vx": hook.velocity.x.to_bits() as i64, "vy": hook.velocity.y.to_bits() as i64,
                "vz": hook.velocity.z.to_bits() as i64, "nibble": hook.nibble, "next": hook.random.next_long(),
            }));
        }
        // The client must take the same block hit before moving as the server;
        // otherwise it lands farther away and slides back on the next update.
        let stone = self
            .pack
            .assets
            .registry
            .default_state("minecraft:stone")
            .unwrap();
        self.level_set(block, stone).unwrap();
        for (case, motion) in [
            Vec3::new(0.2, -0.4, 0.1),
            Vec3::new(-0.2, -0.4, 0.1),
            Vec3::new(0.0, -0.6, -0.2),
        ]
        .into_iter()
        .enumerate()
        {
            let mut state = (Vec3::new(x, 301.1, z), motion, State::Flying, false);
            let mut random = LegacyRandom::new(0);
            for tick in 0..5 {
                state = self.client_hook_step(1, None, state, &mut random);
                rows.push(serde_json::json!({
                    "case": case, "tick": tick,
                    "position": [state.0.x - f64::from(block[0]), state.0.y, state.0.z - f64::from(block[2])],
                    "motion": [state.1.x, state.1.y, state.1.z],
                }));
            }
        }
        self.hook = old_hook;
        self.offhand = old_offhand;
        self.player.as_mut().unwrap().position = old_position;
        self.level_set(block, old_block).unwrap();
        rows
    }
    fn holds_rod(&self) -> bool {
        self.held()
            .is_some_and(|s| &*s.item == "minecraft:fishing_rod")
            || self
                .offhand
                .as_ref()
                .is_some_and(|s| &*s.item == "minecraft:fishing_rod")
    }
    /// `FishingRodItem.use`.
    pub(super) fn use_fishing_rod(&mut self, stack: &ItemStack) -> bool {
        if &*stack.item != "minecraft:fishing_rod" {
            return false;
        }
        // The mods hear a server player's cast or reel-in first.
        if self.player.is_some()
            && let (Some(uuid), Some(name)) = (self.lan_acting_uuid, self.lan_acting_name.clone())
        {
            let state = match &self.hook {
                None => 0,
                Some(h) if matches!(h.state, State::Hooked(_)) => 4,
                Some(h) if h.nibble > 0 => 2,
                Some(_) => 3,
            };
            let mut cancel = false;
            self.native_player_fish(uuid, &name, state, &mut cancel);
            let hand = self.hand;
            self.native_player_fish_v2(uuid, &name, state, hand, &mut cancel);
            if cancel {
                return true;
            }
        }
        let Some(p) = self.player.as_ref() else {
            return false;
        };
        let at = [p.position.x, p.position.y, p.position.z];
        if self.hook.is_some() {
            let damage = self.retrieve_hook(stack);
            self.damage_held(damage);
            let pitch = 0.4 / (self.world.random.next_float() * 0.4 + 0.8);
            self.audio.play(
                "minecraft:entity.fishing_bobber.retrieve",
                Some(at),
                1.0,
                pitch,
            );
            // A rod that broke during retrieval is now an empty ItemStack,
            // whose use-effects component is absent.
            if let Some(rod) = self.held().cloned() {
                self.use_vibration(&rod, "minecraft:item_interact_finish");
            }
        } else {
            let pitch = 0.4 / (self.world.random.next_float() * 0.4 + 0.8);
            self.audio.play(
                "minecraft:entity.fishing_bobber.throw",
                Some(at),
                0.5,
                pitch,
            );
            // FishingHook(player, level, luck, lureSpeed)
            let lure = stack.enchantment_level("minecraft:lure") as i32 * 5 * 20;
            let luck = stack.enchantment_level("minecraft:luck_of_the_sea") as i32;
            let (yaw, pitch) = (self.yaw, self.pitch);
            let (y_sin, y_cos, dir_y) = cast_direction(yaw, pitch);
            let eye_y = p.position.y + f64::from(p.dimensions.eye_height);
            // snapTo(x, getEyeY(), z): the hook's feet start at the owner's eye.
            let start = Vec3::new(
                p.position.x - f64::from(y_sin) * 0.3,
                eye_y,
                p.position.z - f64::from(y_cos) * 0.3,
            );
            // The hook's own random (`Entity.random`).
            let mut random = random_source::entity();
            let v = cast_velocity(y_sin, y_cos, dir_y, &mut random);
            self.next_entity += 1;
            self.hook = Some(Hook {
                id: self.next_entity,
                position: start,
                previous: start,
                velocity: v,
                state: State::Flying,
                last_deflected_by: None,
                needs_sync: false,
                tick_count: 0,
                life: 0,
                on_ground: false,
                nibble: 0,
                time_until_lured: 0,
                time_until_hooked: 0,
                fish_angle: 0.0,
                out_of_water: 0,
                open_water: true,
                luck,
                lure,
                medium: None,
                lerp: HookLerp::default(),
                random,
            });
            // FishingRodItem.use counts a cast, not a reel-in.
            self.award_stat("used", "minecraft:fishing_rod", 1);
            self.use_vibration(stack, "minecraft:item_interact_start");
        }
        self.swing_arm();
        true
    }
    /// The boat the hook's owner rides (`Entity.isPassengerOfSameVehicle`).
    fn own_vehicle(&self) -> Option<u32> {
        match self.lan_acting {
            None => self.riding,
            Some(conn) => self.lan.as_ref().and_then(|h| {
                h.hub
                    .guests
                    .iter()
                    .find(|g| g.conn == conn)
                    .and_then(|g| g.vehicle)
            }),
        }
    }
    /// `FishingHook.retrieve`: the rod damage it costs.
    fn retrieve_hook(&mut self, rod: &ItemStack) -> u32 {
        let Some(mut hook) = self.hook.take() else {
            return 0;
        };
        let Some(p) = self.player.as_ref() else {
            return 0;
        };
        let owner = p.position;
        let mut damage = 0;
        if let State::Hooked(id) = hook.state {
            // pullEntity: 0.1 of the way to the owner.
            let d = Vec3::new(
                owner.x - hook.position.x,
                owner.y - hook.position.y,
                owner.z - hook.position.z,
            );
            let pulled = |v: Vec3| pull_hooked(v, hook.position, owner);
            let mut hooked_item = false;
            if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
                m.velocity = pulled(m.velocity);
            } else if let Some(item) = self.dropped.iter_mut().find(|i| i.id == id) {
                pull_item(item, Vec3::new(d.x * 0.1, d.y * 0.1, d.z * 0.1));
                hooked_item = true;
            } else if let Some(t) = self.tnts.iter_mut().find(|t| t.id != 0 && t.id == id) {
                t.velocity = pulled(t.velocity);
            } else if let Some(a) = self.armor_stands.iter_mut().find(|a| a.id == id) {
                a.velocity = pulled(a.velocity);
            } else if let Some(b) = self.boats.iter_mut().find(|b| b.id == id) {
                b.velocity = pulled(b.velocity);
            } else if let Some(c) = self.carts.iter_mut().find(|c| c.id == id) {
                // `AbstractMinecart` is pickable, so `pullEntity` moves it too.
                c.velocity = pulled(c.velocity);
            } else if self.creatures.hook_targets.iter().any(|t| t.id == id) {
                // A player: the hub moves them when the tick ends.
                self.creatures
                    .hook_pulls
                    .push((id, Vec3::new(d.x * 0.1, d.y * 0.1, d.z * 0.1)));
            }
            // CriteriaTriggers.FISHING_ROD_HOOKED with no items.
            let hooked = self
                .mobs
                .iter()
                .find(|m| m.id == id)
                .map(|m| self.mob_entity(m));
            if let Some(hooked) = hooked {
                self.trigger_fishing_rod_hooked(rod, &hooked, &[]);
            }
            // `hookedIn instanceof ItemEntity ? 3 : 5`.
            damage = if hooked_item { 3 } else { 5 };
        } else if hook.nibble > 0 {
            let biome = self.biome_name_at(hook.position);
            let Some(loot) = horizon_client::structure_blocks::loot_data(
                &self.options.resources,
                &self.pack_files(),
            ) else {
                return damage;
            };
            // `withLuck(this.luck + owner.getLuck())`: the rod's Luck of the Sea
            // plus the owner's luck attribute (the Luck and Bad Luck effects).
            let owner_luck = self
                .player
                .as_mut()
                .and_then(|p| p.attribute("minecraft:luck").ok())
                .unwrap_or(0.0) as f32;
            // FishingHook.retrieve calls LootTable.getRandomItems without an
            // explicit random source. LootContext uses the table's sequence,
            // or the level random if the table has no sequence.
            let luck = hook.luck as f32 + owner_luck;
            let table = hook
                .medium
                .as_ref()
                .and_then(|m| m.loot_table.as_deref())
                .unwrap_or(FISHING_TABLE);
            let items = fishing_loot(
                &loot,
                table,
                &mut self.rand_sequences,
                &mut self.world.random,
                self.level.seed,
                luck,
                biome.as_deref(),
                hook.open_water,
            );
            let mut caught = Vec::new();
            for stack in items {
                // FishingHook.retrieve: fish_caught for each catch in #minecraft:fishes.
                if self
                    .res
                    .items()
                    .item_tags
                    .get("minecraft:fishes")
                    .is_some_and(|t| t.contains(stack.item.as_str()))
                {
                    self.award_custom_stat("fish_caught", 1);
                }
                let mut item = ItemStack {
                    item: Arc::from(stack.item.as_str()),
                    count: stack.count.max(1) as u32,
                    damage: stack.damage.max(0) as u32,
                    extra: None,
                };
                for (id, level) in &stack.enchantments {
                    if &*item.item == "minecraft:enchanted_book" {
                        horizon_client::item_data::set_level(
                            &mut item.data_mut().stored_enchantments,
                            id,
                            (*level).max(0) as u32,
                        );
                    } else {
                        item.enchant(id, (*level).max(0) as u32);
                    }
                }
                if let Some(potion) = &stack.potion {
                    item = item.with_potion(potion);
                }
                caught.push(item.clone());
                let (dx, dy, dz) = (
                    owner.x - hook.position.x,
                    owner.y - hook.position.y,
                    owner.z - hook.position.z,
                );
                let v = [
                    dx * 0.1,
                    dy * 0.1 + (dx * dx + dy * dy + dz * dz).sqrt().sqrt() * 0.08,
                    dz * 0.1,
                ];
                self.spawn_item(
                    item,
                    [hook.position.x, hook.position.y, hook.position.z],
                    v,
                    0,
                );
                let xp = hook.random.next_int_bound(6).unwrap_or(0) as u32 + 1;
                // Vanilla constructs a single orb directly; ExperienceOrb.award
                // would split it and draw a merge id from the level random.
                self.add_orb([owner.x, owner.y + 0.5, owner.z + 0.5], xp);
            }
            // CriteriaTriggers.FISHING_ROD_HOOKED: the hook itself is the entity.
            let bobber = EntityView {
                kind: "minecraft:fishing_bobber".into(),
                position: [hook.position.x, hook.position.y, hook.position.z],
                on_ground: hook.on_ground,
                fishing_open_water: Some(hook.open_water),
                ..Default::default()
            };
            self.trigger_fishing_rod_hooked(rod, &bobber, &caught);
            damage = 1;
        }
        if hook.on_ground {
            damage = 2;
        }
        damage
    }
    pub(super) fn biome_name_at(&self, p: Vec3) -> Option<String> {
        let (x, y, z) = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        let c = self.world.chunks.get(&(x >> 4, z >> 4))?;
        Some(world_biomes!(self).name(c.biome(x, y, z)).to_owned())
    }
    fn water_height(&self, x: i32, y: i32, z: i32) -> Option<(f32, bool)> {
        let f = self.res.facts().get(self.world.block(x, y, z));
        if f.fluid != Fluid::Water {
            return None;
        }
        let above = self.res.facts().get(self.world.block(x, y + 1, z));
        let h = if above.fluid == Fluid::Water {
            1.0
        } else {
            f32::from(f.fluid_level) / 9.0
        };
        Some((h, f.fluid_level == 8 && !f.fluid_falling))
    }
    /// getOpenWaterTypeForBlock: 0 above water, 1 inside water, 2 invalid.
    fn open_water_type(&self, x: i32, y: i32, z: i32) -> u8 {
        let f = self.res.facts().get(self.world.block(x, y, z));
        if self.world.is_air(self.world.block(x, y, z)) || &*f.name == "minecraft:lily_pad" {
            return 0;
        }
        if f.fluid == Fluid::Water
            && f.fluid_level == 8
            && !f.fluid_falling
            && !f.full_collision
            && !f.legacy_solid
        {
            1
        } else {
            2
        }
    }
    /// calculateOpenWater over the 5x4x5 area around the hook.
    fn calculate_open_water(&self, b: [i32; 3]) -> bool {
        let mut previous = 2u8;
        for dy in -1..=2 {
            let mut layer = None;
            for dx in -2..=2 {
                for dz in -2..=2 {
                    let t = self.open_water_type(b[0] + dx, b[1] + dy, b[2] + dz);
                    layer = Some(match layer {
                        None => t,
                        Some(l) if l == t => l,
                        Some(_) => 2,
                    });
                }
            }
            let layer = layer.unwrap_or(2);
            match layer {
                0 if previous == 2 => return false,
                1 if previous == 0 => return false,
                2 => return false,
                _ => {}
            }
            previous = layer;
        }
        true
    }
    /// `FishingHook.catchingFish`.
    fn catching_fish(&mut self, h: &mut Hook, b: [i32; 3]) {
        let mut speed = 1;
        let above = [b[0], b[1] + 1, b[2]];
        let sky = self.world.light(above[0], above[1], above[2]) >> 4 == 15;
        // Level.isRainingAt: rain (not snow) reaches the position.
        if h.random.next_float() < 0.25
            && self.with_level(|b, level, _| {
                b.is_raining_at(level, BlockPos::new(above[0], above[1], above[2]))
            })
        {
            speed += 1;
        }
        if h.random.next_float() < 0.5 && !sky {
            speed -= 1;
        }
        let at = [h.position.x, h.position.y, h.position.z];
        if h.nibble > 0 {
            h.nibble -= 1;
            if h.nibble <= 0 {
                h.time_until_lured = 0;
                h.time_until_hooked = 0;
            }
        } else if h.time_until_hooked > 0 {
            h.time_until_hooked -= speed;
            if h.time_until_hooked > 0 {
                // The fish swimming up: a bubble now and then, and a fishing
                // particle to each side of its path.
                h.fish_angle += h.random.triangle(0.0, 9.188) as f32;
                let angle = h.fish_angle * (std::f64::consts::PI / 180.0) as f32;
                let (sin, cos) = (trig::sin(f64::from(angle)), trig::cos(f64::from(angle)));
                let distance = h.time_until_hooked as f32 * 0.1;
                let fish = [
                    h.position.x + f64::from(sin * distance),
                    f64::from(h.position.y.floor() as f32 + 1.0),
                    h.position.z + f64::from(cos * distance),
                ];
                if self.water_block(fish) {
                    if h.random.next_float() < 0.15 {
                        let spread = [f64::from(sin), 0.1, f64::from(cos)];
                        let at = [fish[0], fish[1] - f64::from(0.1f32), fish[2]];
                        self.send_particles(ParticleType::Bubble, at, spread, 0.0, 1);
                    }
                    let (x, z) = (sin * 0.04, cos * 0.04);
                    for side in [1.0f32, -1.0] {
                        let spread = [f64::from(z * side), 0.01, f64::from(-x * side)];
                        self.send_particles(ParticleType::Fishing, fish, spread, 1.0, 0);
                    }
                }
            } else {
                // The mods hear a server player's fish bite, and may call it
                // off: the wait starts over.
                if let (Some(uuid), Some(name)) =
                    (self.lan_acting_uuid, self.lan_acting_name.clone())
                {
                    let mut cancel = false;
                    self.native_player_fish(uuid, &name, 1, &mut cancel);
                    // A bite has no hand of its own: the one holding the rod, the main
                    // hand first, as `FishingHook.shouldStopFishing` looks.
                    let hand = if self
                        .stack_in(self.selected)
                        .is_some_and(|s| &*s.item == "minecraft:fishing_rod")
                    {
                        horizon_client::first_person::Hand::Main
                    } else {
                        horizon_client::first_person::Hand::Off
                    };
                    self.native_player_fish_v2(uuid, &name, 1, hand, &mut cancel);
                    if cancel {
                        h.time_until_lured = 0;
                        h.time_until_hooked = 0;
                        return;
                    }
                }
                let pitch = 1.0 + (h.random.next_float() - h.random.next_float()) * 0.4;
                self.audio.play(
                    "minecraft:entity.fishing_bobber.splash",
                    Some(at),
                    0.25,
                    pitch,
                );
                // The bite: bubbles and fishing particles around the bobber.
                let at = [h.position.x, h.position.y + 0.5, h.position.z];
                let count = (1.0 + SIZE as f32 * 20.0) as u32;
                let spread = [SIZE, 0.0, SIZE];
                for kind in [ParticleType::Bubble, ParticleType::Fishing] {
                    self.send_particles(kind, at, spread, f64::from(0.2f32), count);
                }
                h.trigger_bite(self.world.game_time);
            }
        } else if h.time_until_lured > 0 {
            h.time_until_lured -= speed;
            let mut tease = 0.15f32;
            if h.time_until_lured < 20 {
                tease += (20 - h.time_until_lured) as f32 * 0.05;
            } else if h.time_until_lured < 40 {
                tease += (40 - h.time_until_lured) as f32 * 0.02;
            } else if h.time_until_lured < 60 {
                tease += (60 - h.time_until_lured) as f32 * 0.01;
            }
            if h.random.next_float() < tease {
                // A splash where a fish is teasing the bait.
                let angle = (h.random.next_float() * 360.0) * (std::f64::consts::PI / 180.0) as f32;
                let distance = h.random.next_float() * 35.0 + 25.0;
                let fish = [
                    h.position.x + f64::from(trig::sin(f64::from(angle)) * distance) * 0.1,
                    f64::from(h.position.y.floor() as f32 + 1.0),
                    h.position.z + f64::from(trig::cos(f64::from(angle)) * distance) * 0.1,
                ];
                if self.water_block(fish) {
                    let count = 2 + h.random.next_int_bound(2).unwrap_or(0) as u32;
                    let spread = [f64::from(0.1f32), 0.0, f64::from(0.1f32)];
                    self.send_particles(ParticleType::Splash, fish, spread, 0.0, count);
                }
            }
            if h.time_until_lured <= 0 {
                h.fish_angle = h.random.next_float() * 360.0;
                h.time_until_hooked = 20 + h.random.next_int_bound(61).unwrap_or(0);
            }
        } else {
            let range = h.medium.as_ref().and_then(|m| m.lure_ticks);
            h.time_until_lured = lure_wait(&mut h.random, range, h.lure);
        }
    }
    /// `catchingFish`'s check below a fish: the block under it is water.
    fn water_block(&self, fish: [f64; 3]) -> bool {
        let state = self.world.block(
            fish[0].floor() as i32,
            (fish[1] - 1.0).floor() as i32,
            fish[2].floor() as i32,
        );
        &*self.res.facts().get(state).name == "minecraft:water"
    }
    /// `ServerLevel.sendParticles` of a simple particle.
    fn send_particles(
        &mut self,
        kind: ParticleType,
        at: [f64; 3],
        spread: [f64; 3],
        speed: f64,
        count: u32,
    ) {
        let options = ParticleOptions::simple(kind);
        self.particles
            .send_particles(&options, at, spread, speed, count);
        // The players a server serves are sent them too.
        self.lan_send_particles(super::lan_particles::QueuedParticle {
            dimension: self.dim.id(),
            options,
            state: None,
            at,
            spread,
            speed: [speed; 3],
            count: count as i32,
            randomization: 0,
        });
    }
    /// `FishingHook.tick`.
    pub(super) fn tick_fishing(&mut self) {
        let Some(mut h) = self.hook.take() else {
            return;
        };
        h.needs_sync = false;
        let Some(owner) = self.player.as_ref().map(|p| p.position) else {
            return;
        };
        // shouldStopFishing: a rod in either hand, within 32 blocks.
        let d2 = (h.position.x - owner.x).powi(2)
            + (h.position.y - owner.y).powi(2)
            + (h.position.z - owner.z).powi(2);
        if !self.holds_rod() || d2 > 1024.0 {
            return;
        }
        h.tick_count += 1;
        // The players it can catch, as they stand now (the host's own hook;
        // guests' hooks use the ones taken as their turns start).
        if self.lan_acting.is_none()
            && matches!(h.state, State::Flying | State::Hooked(_))
            && let Some(host) = self.lan.take()
        {
            self.refresh_hook_targets(&host);
            self.lan = Some(host);
        }
        h.previous = h.position;
        if h.on_ground {
            h.life += 1;
            if h.life >= 1200 {
                return;
            }
        } else {
            h.life = 0;
        }
        let b = [
            h.position.x.floor() as i32,
            h.position.y.floor() as i32,
            h.position.z.floor() as i32,
        ];
        let liquid = self.water_height(b[0], b[1], b[2]).map_or(0.0, |w| w.0);
        // Out of water, a mod may say the bobber floats here all the same.
        let medium = if liquid > 0.0 || matches!(h.state, State::Hooked(_)) {
            None
        } else {
            self.hook_medium(h.id, h.position, Some(owner))
        };
        match &medium {
            Some(m) => {
                h.medium = Some(HookMedium {
                    loot_table: m.loot_table.clone(),
                    lure_ticks: m.lure_ticks,
                })
            }
            None if liquid > 0.0 => h.medium = None,
            None => {}
        }
        let in_water = liquid > 0.0 || medium.is_some();
        // The block a flying bobber meets or rests on this tick (and the face), for
        // `native_fishing_hook_landed` once the tick is done.
        let rested = h.on_ground;
        let mut landed: Option<([i32; 3], [i32; 3])> = None;
        match h.state {
            State::Flying => {
                if in_water {
                    h.velocity =
                        Vec3::new(h.velocity.x * 0.3, h.velocity.y * 0.2, h.velocity.z * 0.3);
                    h.state = State::Bobbing;
                    self.hook = Some(h);
                    return;
                }
                // checkCollision: find the first block and entity on the movement
                // segment so a fast hook cannot tunnel or catch through a wall.
                let to = h.position.add_vector(h.velocity);
                let block_hit_at = self.hook_block_hit_at(h.position, to);
                let block_hit = block_hit_at.map(|hit| hit.location);
                let entity_end = block_hit.unwrap_or(to);
                let owner = self.lan_acting;
                let own_vehicle = self.own_vehicle();
                let hit = first_entity_hit(
                    h.position,
                    entity_end,
                    entity_hit_margin(h.tick_count),
                    self.mobs
                        .iter()
                        .filter(|m| m.alive())
                        .map(|m| (m.id, m.bounds()))
                        .chain(
                            self.creatures
                                .hook_targets
                                .iter()
                                .filter(|t| t.conn != owner)
                                .map(|t| (t.id, t.bounds)),
                        )
                        .chain(
                            self.armor_stands
                                .iter()
                                .filter(|a| a.health > 0.0 && !a.marker)
                                .map(|a| (a.id, a.bounds())),
                        )
                        .chain(
                            self.boats
                                .iter()
                                .filter(|b| Some(b.id) != own_vehicle)
                                .map(|b| (b.id, b.bounds())),
                        )
                        .chain(minecart_hook_targets(self.carts.iter(), own_vehicle))
                        .chain(self.dropped.iter().map(|i| (i.id, i.bounds())))
                        .chain(
                            self.tnts
                                .iter()
                                .filter(|t| t.id != 0)
                                .map(|t| (t.id, t.bounds())),
                        ),
                );
                if let Some(id) = hit {
                    let breeze = self
                        .mobs
                        .iter()
                        .find(|m| {
                            m.id == id && m.species() == horizon_client::mobs::Species::Breeze
                        })
                        .map(|m| [m.position.x, m.position.y, m.position.z]);
                    match h.hit_entity(id, breeze.is_some()) {
                        EntityCollision::Deflected { first } => {
                            if first {
                                self.audio.play(
                                    "minecraft:entity.breeze.deflect",
                                    breeze,
                                    1.0,
                                    1.0,
                                );
                            }
                        }
                        EntityCollision::Hooked => {
                            // Projectile.onHit on an entity: PROJECTILE_LAND.
                            self.projectile_event(
                                "minecraft:projectile_land",
                                Some(horizon_client::mobs::Who::Player),
                                [h.position.x, h.position.y, h.position.z],
                            );
                            self.hook = Some(h);
                            return;
                        }
                    }
                } else if let Some(hit) = block_hit {
                    landed = block_hit_at
                        .map(|at| ([at.block.x, at.block.y, at.block.z], at.direction.normal()));
                    // Projectile.onHit on a block: onHitBlock first, whose super call
                    // runs the block's onProjectileHit, then the rebound.
                    if let Some((pos, face)) = landed {
                        self.fishing_hook_block_hit(h.position, pos, hit, face);
                    }
                    // Then PROJECTILE_LAND.
                    self.projectile_event(
                        "minecraft:projectile_land",
                        Some(horizon_client::mobs::Who::Player),
                        [hit.x, hit.y, hit.z],
                    );
                    h.velocity = rebound(h.velocity, hit, h.position);
                }
            }
            State::Hooked(id) => {
                // updateHookedEntity: ride the hooked mob, player or item at 0.8 of its height.
                let hooked = self
                    .mobs
                    .iter()
                    .find(|m| m.id == id && m.alive())
                    .map(|m| (m.position, f64::from(m.height())))
                    .or_else(|| {
                        self.creatures
                            .hook_targets
                            .iter()
                            .find(|t| t.id == id)
                            .map(|t| {
                                let (lo, hi) = (t.bounds.min_position(), t.bounds.max_position());
                                (
                                    Vec3::new((lo.x + hi.x) / 2.0, lo.y, (lo.z + hi.z) / 2.0),
                                    hi.y - lo.y,
                                )
                            })
                    })
                    .or_else(|| {
                        self.armor_stands.iter().find(|a| a.id == id).map(|a| {
                            let b = a.bounds();
                            (a.position, b.max_position().y - b.min_position().y)
                        })
                    })
                    .or_else(|| {
                        self.boats
                            .iter()
                            .find(|b| b.id == id)
                            .map(|b| (b.position, super::boats::HEIGHT))
                    })
                    .or_else(|| {
                        self.carts
                            .iter()
                            .find(|c| c.id == id)
                            .map(|c| (c.position, super::minecarts::HEIGHT))
                    })
                    .or_else(|| {
                        self.dropped
                            .iter()
                            .find(|i| i.id == id)
                            .map(|i| (i.position, horizon_client::entities::ItemEntity::WIDTH))
                    })
                    .or_else(|| {
                        self.tnts
                            .iter()
                            .find(|t| t.id != 0 && t.id == id)
                            .map(|t| (t.position, super::tnt::PrimedTnt::SIZE))
                    });
                match hooked {
                    Some((at, height)) => h.position = Vec3::new(at.x, at.y + height * 0.8, at.z),
                    None => h.state = State::Flying,
                }
                self.hook = Some(h);
                return;
            }
            State::Bobbing => {
                let v = h.velocity;
                let mut force = match &medium {
                    Some(m) => h.position.y + v.y - m.surface,
                    None => h.position.y + v.y - f64::from(b[1]) - f64::from(liquid),
                };
                if force.abs() < 0.01 {
                    force += force.signum() * 0.1;
                }
                let r = f64::from(h.random.next_float());
                h.velocity = Vec3::new(v.x * 0.9, v.y - force * r * 0.2, v.z * 0.9);
                if h.nibble <= 0 && h.time_until_hooked <= 0 {
                    h.open_water = true;
                } else {
                    // A medium has no water around it to be open: it counts as open.
                    h.open_water = h.open_water
                        && h.out_of_water < 10
                        && (medium.is_some() || self.calculate_open_water(b));
                }
                if in_water {
                    h.out_of_water = (h.out_of_water - 1).max(0);
                    if h.nibble > 0 {
                        let mut sync_random = synchronized_random(h.id, self.world.game_time);
                        let dip = -0.1
                            * f64::from(sync_random.next_float())
                            * f64::from(sync_random.next_float());
                        h.velocity.y += dip;
                    }
                    self.catching_fish(&mut h, b);
                } else {
                    h.out_of_water = (h.out_of_water + 1).min(10);
                }
            }
        }
        if !in_water && !h.on_ground {
            h.velocity.y -= f64::from(0.03_f32);
        }
        let mut scene = SceneCollisions {
            world: &self.world,
            binding: self.collision,
            subject: None,
        };
        if let Ok(moved) = scene.resolve(bounds(h.position), h.velocity, 0.0, h.on_ground) {
            let horizontal =
                (moved.x - h.velocity.x).abs() > 1.0e-7 || (moved.z - h.velocity.z).abs() > 1.0e-7;
            h.on_ground = h.velocity.y < 0.0 && moved.y > h.velocity.y;
            h.position = Vec3::new(
                h.position.x + moved.x,
                h.position.y + moved.y,
                h.position.z + moved.z,
            );
            if h.state == State::Flying && (h.on_ground || horizontal) {
                h.velocity = Vec3::ZERO;
            }
        }
        self.entity_powder_snow_particles(h.previous, h.position, h.bounds(), h.velocity, false);
        h.velocity = Vec3::new(
            h.velocity.x * 0.92,
            h.velocity.y * 0.92,
            h.velocity.z * 0.92,
        );
        // A bobber lying on a block (on the ground as the tick began or ends) rests on
        // the block under its centre, on that block's top.
        if landed.is_none() && h.state == State::Flying && (rested || h.on_ground) {
            let below = [
                h.position.x.floor() as i32,
                (h.position.y - 1.0e-4).floor() as i32,
                h.position.z.floor() as i32,
            ];
            landed = Some((below, [0, 1, 0]));
        }
        let id = h.id;
        self.hook = Some(h);
        // The mods hear a server player's bobber on a block with the bobber in place, so
        // `native_remove_entity` can take it.
        if let (Some((block, face)), Some(player)) = (landed, self.lan_acting_uuid) {
            let dimension = self.dim.id();
            self.native_fishing_hook_landed(player, id, dimension, block, face);
        }
    }
    /// The players a hook can catch: everyone but spectators and the dead
    /// (`Projectile.canHitEntity`), the host and the guests in the level.
    pub(super) fn refresh_hook_targets(&mut self, host: &LanHost) {
        let mut targets = Vec::new();
        for g in &host.hub.guests {
            if host.hub.here(g) && g.loaded && !g.dead && !g.spectator() {
                targets.push(HookTarget {
                    id: g.entity_id as u32,
                    conn: Some(g.conn),
                    bounds: guest_bounds(g),
                });
            }
        }
        self.creatures.hook_targets = targets;
    }
    /// `FishingHook.pullEntity` on the players the hooks reeled in this tick
    /// pulled: their velocity moves, and a guest's reaches their client.
    pub(super) fn flush_hook_pulls(&mut self) {
        let pulls = std::mem::take(&mut self.creatures.hook_pulls);
        let Some(mut host) = self.lan.take() else {
            return;
        };
        for (id, delta) in pulls {
            let push = |g: &mut Self| {
                if let Some(p) = g.player.as_mut() {
                    p.velocity = p.velocity.add_vector(delta);
                }
            };
            if let Some(conn) = host
                .hub
                .guests
                .iter()
                .find(|g| g.entity_id as u32 == id)
                .map(|g| g.conn)
            {
                self.guest_hit_by(&mut host, conn, push);
            }
        }
        self.lan = Some(host);
    }
    /// `FishingRodCast`: the stack in the arm holding the rod
    /// (`FishingHookRenderer.getHoldingArm`) while the hook is out.
    pub(super) fn cast_rod(&self) -> Option<&ItemStack> {
        self.hook.as_ref()?;
        if self.main_hand_rod() {
            self.held()
        } else {
            self.offhand.as_ref()
        }
    }
    pub(super) fn main_hand_rod(&self) -> bool {
        self.held()
            .is_some_and(|s| &*s.item == "minecraft:fishing_rod")
    }
    /// The hook's position this frame.
    fn hook_position(&self, alpha: f64) -> Option<glam::DVec3> {
        let h = self.hook.as_ref()?;
        let lerp = |a: f64, b: f64| a + (b - a) * alpha;
        Some(glam::DVec3::new(
            lerp(h.previous.x, h.position.x),
            lerp(h.previous.y, h.position.y),
            lerp(h.previous.z, h.position.z),
        ))
    }
    /// FishingHookRenderer: the bobbers (this player's and the others' a
    /// server tracks), camera-facing half-block quads centred on the hooks.
    pub(super) fn fishing_geometry(&self, camera: glam::DVec3, alpha: f64, out: &mut Vec<Vertex>) {
        let hooks: Vec<glam::DVec3> = self
            .hook_position(alpha)
            .into_iter()
            .chain(self.remote_hook_ends(alpha).into_iter().map(|(h, _)| h))
            .collect();
        if hooks.is_empty() {
            return;
        }
        let Some(r) = self
            .gui
            .atlas
            .sprites
            .get("entity/fishing/fishing_hook")
            .copied()
        else {
            return;
        };
        let forward = glam::DVec3::from_array(view_vector(self.yaw, self.pitch));
        let right = forward.cross(glam::DVec3::Y).normalize_or_zero();
        let up = right.cross(forward).normalize_or_zero();
        let s = 0.25;
        let uv = [[r.u0, r.v1], [r.u1, r.v1], [r.u1, r.v0], [r.u0, r.v0]];
        for p in hooks {
            let light =
                self.world
                    .light(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
            let packed = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
            let corners = [
                p - right * s - up * s,
                p + right * s - up * s,
                p + right * s + up * s,
                p - right * s + up * s,
            ];
            for (v, uv) in corners.iter().zip(uv) {
                out.push(Vertex {
                    normal: [0; 4],
                    pos: (*v - camera).as_vec3().to_array(),
                    uv,
                    layer: 0,
                    color: 0xFFFF_FFFF,
                    light: packed,

                    glint_uv: [0.0, 0.0],
                });
            }
        }
    }
    /// FishingHookRenderer: each line, 16 black `lines` segments sagging
    /// from a quarter block above the hook to the rod in its owner's hand,
    /// `width` pixels wide (`Window.getAppropriateLineWidth`).
    pub(super) fn fishing_line(&self, alpha: f64, size: [u32; 2]) -> Vec<Line> {
        let own = self
            .hook_position(alpha)
            .zip(self.rod_hand_position(alpha, size));
        let width = horizon_client::gfx::outline_width(size[0]);
        let mut lines = Vec::new();
        for (hook, hand) in own.into_iter().chain(self.remote_hook_ends(alpha)) {
            let offset = (hand - (hook + glam::DVec3::new(0.0, 0.25, 0.0))).as_vec3();
            let point = |a: f32| {
                let y = offset.y * (a * a + a) * 0.5 + 0.25;
                hook + glam::Vec3::new(offset.x * a, y, offset.z * a).as_dvec3()
            };
            lines.extend((0..16).map(|i| Line {
                start: point(i as f32 / 16.0).to_array(),
                end: point((i + 1) as f32 / 16.0).to_array(),
                color: 0xFF00_0000,
                width,
            }));
        }
        lines
    }
    /// `FishingHookRenderer.getPlayerHandPos`: on the near plane in first
    /// person, beside the body otherwise.
    fn rod_hand_position(&self, alpha: f64, size: [u32; 2]) -> Option<glam::DVec3> {
        let p = self.player.as_ref()?;
        let right_arm = self.main_hand_rod() != self.settings.main_hand_left;
        let invert: f32 = if right_arm { 1.0 } else { -1.0 };
        let swing = trig::sin(f64::from(
            self.swing.animation(alpha as f32).sqrt() * std::f32::consts::PI,
        ));
        let feet = self.previous_position.lerp(p.position, alpha);
        let eye = glam::DVec3::new(feet.x, feet.y + f64::from(p.dimensions.eye_height), feet.z);
        if self.camera_type.first_person() {
            let fov = self.settings.fov as i32 as f32;
            // Camera.getNearPlane(fov).getPointOnPlane(invert * 0.525, -0.1).
            let aspect = f64::from(size[0]) / f64::from(size[1].max(1));
            let height =
                (f64::from(fov * (std::f64::consts::PI / 180.0) as f32) / 2.0).tan() * 0.05;
            let forward = glam::DVec3::from_array(view_vector(self.yaw, self.pitch));
            let left = -forward.cross(glam::DVec3::Y).normalize_or_zero();
            let up = forward.cross(left).normalize_or_zero();
            let point = forward * 0.05 + up * (height * -0.1)
                - left * (height * aspect * f64::from(invert * 0.525));
            let point = point * (960.0 / f64::from(fov));
            // Vec3.yRot(swing * 0.5), then xRot(-swing * 0.7).
            let (s, c) = (
                f64::from(trig::sin(f64::from(swing * 0.5))),
                f64::from(trig::cos(f64::from(swing * 0.5))),
            );
            let point = glam::DVec3::new(
                point.x * c + point.z * s,
                point.y,
                point.z * c - point.x * s,
            );
            let (s, c) = (
                f64::from(trig::sin(f64::from(-swing * 0.7))),
                f64::from(trig::cos(f64::from(-swing * 0.7))),
            );
            let point = glam::DVec3::new(
                point.x,
                point.y * c + point.z * s,
                point.z * c - point.y * s,
            );
            Some(eye + point)
        } else {
            let model = self.player_model.as_ref()?;
            let body =
                horizon_client::camera::lerp(alpha as f32, model.previous_body_yaw, model.body_yaw);
            let angle = f64::from(body * (std::f64::consts::PI / 180.0) as f32);
            let (sin, cos) = (f64::from(trig::sin(angle)), f64::from(trig::cos(angle)));
            let right = f64::from(invert) * 0.35;
            let crouch = if p.pose == Pose::Crouching {
                -0.1875
            } else {
                0.0
            };
            Some(
                eye + glam::DVec3::new(
                    -cos * right - sin * 0.8,
                    crouch - 0.45,
                    -sin * right + cos * 0.8,
                ),
            )
        }
    }
}
impl Game<'_> {
    /// The hooks drawn here, with their owners' ids (0 for this player's own).
    pub(super) fn hooks_status(&self) -> Vec<serde_json::Value> {
        let at = |p: Vec3| [p.x, p.y, p.z];
        let own = self.hook.iter().map(|h| (h.id, 0, h.position));
        let others = self
            .remote_hooks
            .iter()
            .map(|h| (h.id, h.owner, h.position));
        own.chain(others)
            .map(|(id, owner, p)| {
                serde_json::json!({"id": id, "kind": "minecraft:fishing_bobber", "owner": owner, "position": at(p)})
            })
            .collect()
    }
    /// The hook for `horizon-ctl status`.
    pub(super) fn hook_status(&self) -> Option<serde_json::Value> {
        let h = self.hook.as_ref()?;
        Some(serde_json::json!({
            "state": format!("{:?}", h.state),
            "nibble": h.nibble,
            "lured_in": h.time_until_lured,
            "hooked_in": h.time_until_hooked,
            "open_water": h.open_water,
            "loot_table": h.medium.as_ref().and_then(|m| m.loot_table.clone()),
        }))
    }
}

/// A hook a server tracks to this client (`FishingHook.recreateFromPacket`)
/// that another player cast: drawn where the server puts it, its line
/// running to that player's hand.
pub(super) struct RemoteHook {
    id: u32,
    owner: u32,
    pub(super) position: Vec3,
    pub(super) previous: Vec3,
    velocity: Vec3,
    state: State,
    on_ground: bool,
    lerp: HookLerp,
    pub(super) random: LegacyRandom,
}

/// The `onGround` of a server's update (`handleMoveEntity`), except that a
/// hook resting on the ground stays so while the update does not lift it: the
/// server's tracker can send the flag a tick behind the landing.
fn resting(was: bool, at: Vec3, target: [f64; 3], sent: bool) -> bool {
    sent || (was && target[1] <= at.y + 1.0e-3)
}

/// `LinearInterpolationHandler`: where a server's update has a hook heading
/// and the steps left (`DEFAULT_INTERPOLATION_STEPS` 3).
#[derive(Default)]
pub(super) struct HookLerp {
    target: Vec3,
    steps: u32,
}
impl HookLerp {
    /// `interpolateTo`: a new target restarts the steps, the same one does not.
    fn start(&mut self, target: [f64; 3]) {
        let target = Vec3::new(target[0], target[1], target[2]);
        if self.steps == 0 || self.target != target {
            self.target = target;
            self.steps = 3;
        }
    }
    /// `doInterpolate`: a step of the way there.
    fn step(&mut self, at: Vec3) -> Vec3 {
        if self.steps == 0 {
            return at;
        }
        let n = f64::from(self.steps);
        self.steps -= 1;
        Vec3::new(
            at.x + (self.target.x - at.x) / n,
            at.y + (self.target.y - at.y) / n,
            at.z + (self.target.z - at.z) / n,
        )
    }
    /// `applyPredictedMovement`: what the hook moved by itself carries the
    /// target along while it is heading there.
    fn moved(&mut self, by: Vec3) {
        if self.steps > 0 {
            self.target = Vec3::new(
                self.target.x + by.x,
                self.target.y + by.y,
                self.target.z + by.z,
            );
        }
    }
}

impl Hook {
    pub(super) fn id(&self) -> u32 {
        self.id
    }
    /// A server's update of this client's own hook (`handleMoveEntity` also
    /// sets `onGround`).
    pub(super) fn move_to(&mut self, target: [f64; 3], on_ground: bool) {
        self.on_ground = resting(self.on_ground, self.position, target, on_ground);
        self.lerp.start(target);
    }
}
impl RemoteHook {
    pub(super) fn id(&self) -> u32 {
        self.id
    }
    /// A server's update of another player's hook.
    pub(super) fn move_to(&mut self, target: [f64; 3], on_ground: bool) {
        self.on_ground = resting(self.on_ground, self.position, target, on_ground);
        self.lerp.start(target);
    }
}

impl Hook {
    /// This client's own hook as its server tracks it: drawn, never ticked here.
    fn remote(id: u32, at: Vec3) -> Self {
        Self {
            id,
            position: at,
            previous: at,
            velocity: Vec3::ZERO,
            state: State::Flying,
            last_deflected_by: None,
            needs_sync: false,
            tick_count: 0,
            life: 0,
            on_ground: false,
            nibble: 0,
            time_until_lured: 0,
            time_until_hooked: 0,
            fish_angle: 0.0,
            out_of_water: 0,
            open_water: true,
            luck: 0,
            lure: 0,
            medium: None,
            lerp: HookLerp::default(),
            random: random_source::entity(),
        }
    }
    /// `FishingHook`'s entity for the players tracking it: the owner's id in
    /// its add packet, DATA_HOOKED_ENTITY (the hooked id plus one) and
    /// DATA_BITING when they differ from their defaults.
    fn track(&self, owner: i32, out: &mut States) {
        let p = self.position;
        let e = out.add(
            self.id as i32,
            super::lan::entity_uuid(self.id),
            "minecraft:fishing_bobber",
        );
        e.position = [p.x, p.y, p.z];
        e.velocity = [self.velocity.x, self.velocity.y, self.velocity.z];
        e.on_ground = self.on_ground;
        e.needs_sync = self.needs_sync;
        e.data = owner;
        if let State::Hooked(id) = self.state {
            e.meta.push((8, serializer::INT, Value::Int(id as i32 + 1)));
        }
        if self.nibble > 0 {
            e.meta.push((9, serializer::BOOLEAN, Value::Bool(true)));
        }
    }
}

impl Game<'_> {
    /// The hooks guests track: each player's in the level.
    pub(super) fn lan_hook_states(
        &self,
        play: &HashMap<u32, GuestPlay>,
        hub: &Hub,
        out: &mut States,
    ) {
        for g in hub.guests.iter().filter(|g| hub.here(g)) {
            if let Some(h) = play.get(&g.conn).and_then(|p| p.hook.as_ref()) {
                h.track(g.entity_id, out);
            }
        }
    }

    /// `handleAddEntity` for a fishing bobber: this player's own becomes its
    /// hook (`Player.fishing`), another player's is drawn on its line to them.
    pub(super) fn spawn_remote_hook(&mut self, id: u32, at: Vec3, velocity: Vec3, owner: i32) {
        let own = self.remote.as_ref().map(|r| r.entity_id);
        if own == Some(owner) {
            let mut hook = Hook::remote(id, at);
            hook.velocity = velocity;
            self.hook = Some(hook);
        } else {
            self.remote_hooks.push(RemoteHook {
                id,
                owner: owner as u32,
                position: at,
                previous: at,
                velocity,
                state: State::Flying,
                on_ground: false,
                lerp: HookLerp::default(),
                random: random_source::entity(),
            });
        }
    }

    /// `FishingHook.onSyncedDataUpdated`: this player's hook is biting
    /// (DATA_BITING), which `status` shows as a nibble.
    pub(super) fn remote_hook_data(&mut self, id: u32, values: &[(u8, i32, Value)]) {
        // DATA_HOOKED_ENTITY (the hooked id plus one, 0 for none): a hook on a
        // client follows what it hooked (`FishingHook.onSyncedDataUpdated`).
        let time = self.world.game_time;
        let hooked = |value: &Value| match value {
            Value::Int(v) if *v > 0 => State::Hooked(*v as u32 - 1),
            _ => State::Flying,
        };
        if let Some(h) = self
            .hook
            .as_mut()
            .filter(|h| h.id == id && self.remote.is_some())
        {
            for (index, _, value) in values {
                match (index, value) {
                    (9, Value::Bool(biting)) => {
                        h.nibble = i32::from(*biting);
                        if *biting {
                            h.velocity.y = bite_impulse(&mut synchronized_random(h.id, time));
                        }
                    }
                    (8, value) => h.state = hooked(value),
                    _ => {}
                }
            }
        } else if let Some(h) = self.remote_hooks.iter_mut().find(|h| h.id == id) {
            for (index, _, value) in values {
                match (index, value) {
                    (9, Value::Bool(biting)) => {
                        if *biting {
                            h.velocity.y = bite_impulse(&mut synchronized_random(h.id, time));
                        }
                    }
                    (8, value) => h.state = hooked(value),
                    _ => {}
                }
            }
        }
    }

    /// `Entity.lerpMotion` for a hook a server tracks: the motion its own tick
    /// moves it on with between the tracker's updates.
    pub(super) fn remote_hook_motion(&mut self, id: u32, v: Vec3) -> bool {
        if self.remote.is_none() {
            return false;
        }
        if let Some(h) = self.hook.as_mut().filter(|h| h.id == id) {
            h.velocity = v;
        } else if let Some(h) = self.remote_hooks.iter_mut().find(|h| h.id == id) {
            h.velocity = v;
        } else {
            return false;
        }
        true
    }

    /// `FishingHook.tick` on a client: the interpolation step towards the
    /// server's last update, then its own movement between the tracker's
    /// updates (every five ticks), floating in water and riding what it
    /// hooked, as the server's does.
    pub(super) fn tick_remote_hooks(&mut self) {
        if self.remote.is_none() {
            return;
        }
        if let Some(mut h) = self.hook.take() {
            h.previous = h.position;
            h.position = h.lerp.step(h.position);
            let before = h.position;
            let owner = self.player.as_ref().map(|p| p.position);
            (h.position, h.velocity, h.state, h.on_ground) = self.client_hook_step(
                h.id,
                owner,
                (h.position, h.velocity, h.state, h.on_ground),
                &mut h.random,
            );
            h.lerp.moved(h.position.subtract(before));
            self.hook = Some(h);
        }
        let mut hooks = std::mem::take(&mut self.remote_hooks);
        for h in &mut hooks {
            h.previous = h.position;
            h.position = h.lerp.step(h.position);
            let before = h.position;
            let owner = self
                .mobs
                .iter()
                .find(|m| m.id == h.owner)
                .map(|m| m.position);
            (h.position, h.velocity, h.state, h.on_ground) = self.client_hook_step(
                h.id,
                owner,
                (h.position, h.velocity, h.state, h.on_ground),
                &mut h.random,
            );
            h.lerp.moved(h.position.subtract(before));
        }
        self.remote_hooks = hooks;
    }

    /// The fishing medium a mod says the hook at `position` floats in
    /// (`native_fishing_medium`), when it is in it. API @API:fishing_medium@.
    fn hook_medium(
        &mut self,
        id: u32,
        position: Vec3,
        owner: Option<Vec3>,
    ) -> Option<horizon_client::mod_api::NativeFishingMedium> {
        let mut medium = None;
        let dimension = self.dim.id();
        self.native_fishing_medium(
            dimension,
            id,
            [position.x, position.y, position.z],
            owner.map(|o| [o.x, o.y, o.z]),
            &mut medium,
        );
        inside_medium(position, medium)
    }
    /// `FishingHook.checkCollision` clips blocks on both sides of the connection.
    fn hook_block_hit(&self, from: Vec3, to: Vec3) -> Option<Vec3> {
        self.hook_block_hit_at(from, to).map(|hit| hit.location)
    }
    /// [`Self::hook_block_hit`] with the block and the face it meets.
    fn hook_block_hit_at(&self, from: Vec3, to: Vec3) -> Option<horizon_core::geometry::BlockHit> {
        let mut scene = SceneCollisions {
            world: &self.world,
            binding: self.collision,
            subject: None,
        };
        traverse_blocks(from, to, 1024, |p: BlockPos| {
            let state = self.world.block(p.x, p.y, p.z);
            if self.res.facts().is_air(state) {
                return None;
            }
            self.collision
                .collision_shape(&mut scene, state, [p.x, p.y, p.z])
                .ok()?
                .clip(from, to, p)
        })
        .ok()
        .flatten()
    }

    /// `FishingHook.tick`: one tick of movement without the fishing (the
    /// server's): position, motion, state and whether it rests on the ground.
    fn client_hook_step(
        &mut self,
        id: u32,
        owner: Option<Vec3>,
        (mut position, mut velocity, state, mut on_ground): (Vec3, Vec3, State, bool),
        random: &mut impl RandomSource,
    ) -> (Vec3, Vec3, State, bool) {
        let b = [
            position.x.floor() as i32,
            position.y.floor() as i32,
            position.z.floor() as i32,
        ];
        let liquid = self.water_height(b[0], b[1], b[2]).map_or(0.0, |w| w.0);
        // The same fishing medium the server's hook floats in, so it is drawn floating too.
        let medium = if liquid > 0.0 || matches!(state, State::Hooked(_)) {
            None
        } else {
            self.hook_medium(id, position, owner)
        };
        let in_water = liquid > 0.0 || medium.is_some();
        match state {
            State::Flying if in_water => {
                velocity = Vec3::new(velocity.x * 0.3, velocity.y * 0.2, velocity.z * 0.3);
                return (position, velocity, State::Bobbing, on_ground);
            }
            // A landed hook: the server stopped it dead (zero motion) and sends no
            // more than the odd update; it does not fall on from here.
            State::Flying if velocity == Vec3::ZERO && on_ground => {
                return (position, velocity, state, on_ground);
            }
            State::Flying => {
                if let Some(hit) = self.hook_block_hit(position, position.add_vector(velocity)) {
                    velocity = rebound(velocity, hit, position);
                }
            }
            State::Hooked(id) => {
                // updateHookedEntity: at 0.8 of the hooked entity's height.
                let hooked = if self
                    .remote
                    .as_ref()
                    .is_some_and(|r| r.entity_id as u32 == id)
                {
                    self.player
                        .as_ref()
                        .map(|p| (p.position, f64::from(p.dimensions.height)))
                } else {
                    self.mobs
                        .iter()
                        .find(|m| m.id == id && m.alive())
                        .map(|m| (m.position, f64::from(m.height())))
                        .or_else(|| {
                            self.carts
                                .iter()
                                .find(|c| c.id == id)
                                .map(|c| (c.position, super::minecarts::HEIGHT))
                        })
                        .or_else(|| {
                            self.dropped
                                .iter()
                                .find(|i| i.id == id)
                                .map(|i| (i.position, horizon_client::entities::ItemEntity::WIDTH))
                        })
                };
                return match hooked {
                    Some((at, height)) => (
                        Vec3::new(at.x, at.y + height * 0.8, at.z),
                        Vec3::ZERO,
                        state,
                        on_ground,
                    ),
                    None => (position, velocity, State::Flying, on_ground),
                };
            }
            State::Bobbing => {
                let mut force = match &medium {
                    Some(m) => position.y + velocity.y - m.surface,
                    None => position.y + velocity.y - f64::from(b[1]) - f64::from(liquid),
                };
                if force.abs() < 0.01 {
                    force += force.signum() * 0.1;
                }
                let r = f64::from(random.next_float());
                velocity = Vec3::new(
                    velocity.x * 0.9,
                    velocity.y - force * r * 0.2,
                    velocity.z * 0.9,
                );
            }
        }
        if !in_water && !on_ground {
            velocity.y -= f64::from(0.03_f32);
        }
        let mut scene = SceneCollisions {
            world: &self.world,
            binding: self.collision,
            subject: None,
        };
        if let Ok(moved) = scene.resolve(bounds(position), velocity, 0.0, on_ground) {
            let horizontal =
                (moved.x - velocity.x).abs() > 1.0e-7 || (moved.z - velocity.z).abs() > 1.0e-7;
            on_ground = velocity.y < 0.0 && moved.y > velocity.y;
            position = Vec3::new(
                position.x + moved.x,
                position.y + moved.y,
                position.z + moved.z,
            );
            if state == State::Flying && (on_ground || horizontal) {
                velocity = Vec3::ZERO;
            }
        }
        velocity = Vec3::new(velocity.x * 0.92, velocity.y * 0.92, velocity.z * 0.92);
        (position, velocity, state, on_ground)
    }

    /// Takes the server's fishing bobber `id` out of the level in place, for
    /// `native_remove_entity`: the bobber of the player in place, or another of this
    /// level's players' (out of reach during a guest's turn, when the hub is taken out).
    /// Its tracking removes it from the clients; the rod is left as when the bobber goes
    /// by itself. Whether it was there.
    pub(super) fn remove_server_hook(&mut self, id: u32) -> bool {
        if self.remote.is_some() || id == 0 {
            return false;
        }
        if self.hook.as_ref().is_some_and(|h| h.id == id) {
            self.hook = None;
            return true;
        }
        let Some(host) = self.lan.as_mut() else {
            return false;
        };
        let owner = host
            .hub
            .guests
            .iter()
            .filter(|g| host.hub.here(g))
            .map(|g| g.conn)
            .find(|conn| {
                host.play
                    .get(conn)
                    .and_then(|p| p.hook.as_ref())
                    .is_some_and(|h| h.id == id)
            });
        match owner.and_then(|conn| host.play.get_mut(&conn)) {
            Some(play) => {
                play.hook = None;
                true
            }
            None => false,
        }
    }

    pub(super) fn remove_remote_hook(&mut self, id: u32) {
        if self.remote.is_some() && self.hook.as_ref().is_some_and(|h| h.id == id) {
            self.hook = None;
        }
        self.remote_hooks.retain(|h| h.id != id);
    }

    pub(super) fn clear_remote_hooks(&mut self) {
        if self.remote.is_some() {
            self.hook = None;
        }
        self.remote_hooks.clear();
    }

    /// Other players' hooks: the bobber and its line to the owner's hand
    /// (`FishingHookRenderer.getPlayerHandPos` in third person), as
    /// (hook, hand) this frame.
    fn remote_hook_ends(&self, alpha: f64) -> Vec<(glam::DVec3, glam::DVec3)> {
        let lerp = |a: Vec3, b: Vec3| {
            glam::DVec3::new(
                a.x + (b.x - a.x) * alpha,
                a.y + (b.y - a.y) * alpha,
                a.z + (b.z - a.z) * alpha,
            )
        };
        self.remote_hooks
            .iter()
            .filter_map(|h| {
                let owner = self.mobs.iter().find(|m| m.id == h.owner)?;
                let feet = lerp(owner.previous, owner.position);
                let body = horizon_client::camera::lerp(
                    alpha as f32,
                    owner.previous_body_yaw,
                    owner.body_yaw,
                );
                let angle = f64::from(body * (std::f64::consts::PI / 180.0) as f32);
                let (sin, cos) = (f64::from(trig::sin(angle)), f64::from(trig::cos(angle)));
                // The right arm; a crouching owner's eyes are lower.
                let eye = owner.eye_y() - owner.position.y;
                let hand = feet
                    + glam::DVec3::new(
                        -cos * 0.35 - sin * 0.8,
                        eye - 0.45,
                        -sin * 0.35 + cos * 0.8,
                    );
                Some((lerp(h.previous, h.position), hand))
            })
            .collect()
    }
}

/// `FishingHook(Player, Level, int, int)`: `(ySin, yCos, y)` of the cast direction, using
/// `Mth.sin`/`Mth.cos` (the 65536-entry table), so straight down gives `xCos == -0.0`, y `-5`.
fn cast_direction(yaw: f32, pitch: f32) -> (f32, f32, f32) {
    let ya = f64::from(-yaw * trig::DEG_TO_RAD - std::f32::consts::PI);
    let pa = f64::from(-pitch * trig::DEG_TO_RAD);
    let (y_sin, y_cos) = (trig::sin(ya), trig::cos(ya));
    let (x_cos, x_sin) = (-trig::cos(pa), trig::sin(pa));
    (y_sin, y_cos, (-(x_sin / x_cos)).clamp(-5.0, 5.0))
}

#[cfg(test)]
mod tests {
    use super::{Hook, cast_direction, entity_hit_margin, first_entity_hit, pull_item, rebound};

    /// Compare the production loot selector to a real FishingHook.retrieve oracle.
    #[test]
    #[ignore = "requires prepared resources and the vanilla fishing reference"]
    fn fishing_retrieve_reference() {
        use horizon_core::random::{LegacyRandom, RandomSource};
        use horizon_world::loot::{LootData, PackOverlay};
        let resources = std::env::var_os("HORIZON_FISHING_RESOURCES").unwrap();
        let path = std::env::var_os("HORIZON_FISHING_REFERENCE").unwrap();
        let rows: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(rows.len(), 5, "missing vanilla retrieval cases");
        let loot = LootData::load(std::path::Path::new(&resources), &PackOverlay::NONE).unwrap();
        for row in rows {
            let seed = row["sequence_seed"].as_i64().unwrap();
            let mut sequences = horizon_client::random_sequences::RandomSequences::default();
            let mut level_random = LegacyRandom::new(1);
            let items = super::fishing_loot(
                &loot,
                super::FISHING_TABLE,
                &mut sequences,
                &mut level_random,
                seed,
                0.0,
                None,
                true,
            );
            let drops = row["drops"].as_array().unwrap();
            let expected_items: Vec<_> = drops.iter().filter(|d| d.get("item").is_some()).collect();
            assert_eq!(row["damage"].as_i64(), Some(1), "seed {seed}");
            assert!(
                !expected_items.is_empty(),
                "missing vanilla catch for seed {seed}"
            );
            assert_eq!(items.len(), expected_items.len(), "seed {seed}");
            let mut hook_random = LegacyRandom::new(row["hook_seed"].as_i64().unwrap());
            for (item, expected) in items.iter().zip(expected_items) {
                assert_eq!(item.item, expected["item"].as_str().unwrap(), "seed {seed}");
                assert_eq!(i64::from(item.count), expected["count"].as_i64().unwrap());
                let xp = hook_random.next_int_bound(6).unwrap() + 1;
                assert!(
                    drops
                        .iter()
                        .any(|d| d["xp"].as_i64() == Some(i64::from(xp)))
                );
            }
            assert_eq!(
                hook_random.next_long(),
                row["hook_next"].as_i64().unwrap(),
                "seed {seed}"
            );
            assert_eq!(
                sequences
                    .get("minecraft:gameplay/fishing", seed)
                    .next_long(),
                row["sequence_next"].as_i64().unwrap(),
                "seed {seed}"
            );
        }
        let casts: Vec<serde_json::Value> = serde_json::from_slice(
            &std::fs::read(std::env::var_os("HORIZON_FISHING_CAST_REFERENCE").unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(casts.len(), 5, "missing vanilla cast cases");
        for row in casts {
            let mut random = LegacyRandom::new(row["seed"].as_i64().unwrap());
            let (sin, cos, vertical) = super::cast_direction(
                row["yaw"].as_f64().unwrap() as f32,
                row["pitch"].as_f64().unwrap() as f32,
            );
            let velocity = super::cast_velocity(sin, cos, vertical, &mut random);
            for (name, value) in [
                ("x_bits", velocity.x),
                ("y_bits", velocity.y),
                ("z_bits", velocity.z),
            ] {
                assert_eq!(value.to_bits() as i64, row[name].as_i64().unwrap(), "{row}");
            }
            assert_eq!(random.next_long(), row["next"].as_i64().unwrap(), "{row}");
        }
    }
    #[test]
    fn a_breeze_deflects_the_hook_without_attaching_it() {
        use super::{EntityCollision, Hook, State};
        let mut hook = Hook::remote(1, Vec3::ZERO);
        hook.velocity = Vec3::new(2.0, -0.4, 1.0);
        assert_eq!(
            hook.hit_entity(10, true),
            EntityCollision::Deflected { first: true }
        );
        assert_eq!(hook.state, State::Flying);
        assert!(hook.needs_sync);
        assert_eq!(hook.velocity, Vec3::new(-1.0, 0.2, -0.5));
        assert_eq!(
            hook.hit_entity(10, true),
            EntityCollision::Deflected { first: false }
        );
        assert_eq!(hook.velocity, Vec3::new(-1.0, 0.2, -0.5));
        assert_eq!(
            hook.hit_entity(11, true),
            EntityCollision::Deflected { first: true }
        );
        assert_eq!(hook.velocity, Vec3::new(0.5, -0.1, 0.25));
    }

    #[test]
    fn ordinary_targets_still_hook_and_a_deflection_is_tracked_immediately() {
        use super::{EntityCollision, Hook, State};
        let mut hook = Hook::remote(1, Vec3::ZERO);
        hook.velocity = Vec3::new(2.0, 0.0, 0.0);
        hook.hit_entity(10, true);
        let mut states = horizon_client::lan::entity::States::default();
        hook.track(99, &mut states);
        let state = &states.as_slice()[0];
        assert!(state.needs_sync);
        assert_eq!(state.velocity, [-1.0, -0.0, -0.0]);
        assert!(!state.meta.iter().any(|entry| entry.0 == 8));
        assert_eq!(hook.hit_entity(20, false), EntityCollision::Hooked);
        assert_eq!(hook.state, State::Hooked(20));
        assert_eq!(hook.velocity, Vec3::ZERO);
    }

    #[test]
    #[ignore = "run by tools/test_breeze_hook_parity.py"]
    fn breeze_hook_parity_fixture() {
        use super::{EntityCollision, Hook, State};
        use std::io::Write;
        let input =
            std::fs::read_to_string(std::env::var("HORIZON_BREEZE_HOOK_INPUT").unwrap()).unwrap();
        let mut out =
            std::fs::File::create(std::env::var("HORIZON_BREEZE_HOOK_OUTPUT").unwrap()).unwrap();
        for line in input.lines() {
            let parts: Vec<_> = line.split_whitespace().collect();
            let mut hook = Hook::remote(1, Vec3::ZERO);
            hook.velocity = Vec3::new(
                parts[1].parse().unwrap(),
                parts[2].parse().unwrap(),
                parts[3].parse().unwrap(),
            );
            let mut trace = Vec::new();
            for target in &parts[4..] {
                hook.needs_sync = false;
                let response = hook.hit_entity(target.parse::<u32>().unwrap() + 10, true);
                let sounds = i32::from(matches!(
                    response,
                    EntityCollision::Deflected { first: true }
                ));
                let landed = i32::from(matches!(response, EntityCollision::Hooked));
                trace.push(format!(
                    "{:016x},{:016x},{:016x}:{sounds}:{landed}:{}:{}",
                    hook.velocity.x.to_bits(),
                    hook.velocity.y.to_bits(),
                    hook.velocity.z.to_bits(),
                    i32::from(hook.needs_sync),
                    i32::from(matches!(hook.state, State::Hooked(_)))
                ));
            }
            writeln!(out, "{}", trace.join(" ")).unwrap();
        }
    }

    use horizon_client::{entities::ItemEntity, items::ItemStack};
    use horizon_core::geometry::{Aabb, Vec3};

    #[test]
    fn casting_straight_down_aims_down_and_straight_up_aims_up() {
        assert_eq!(cast_direction(0.0, 90.0).2, -5.0);
        assert_eq!(cast_direction(123.0, 89.9).2, -5.0);
        assert_eq!(cast_direction(0.0, -90.0).2, 5.0);
        assert_eq!(cast_direction(0.0, 0.0).2, 0.0);
    }

    #[test]
    fn cast_direction_uses_the_vanilla_sine_table() {
        // pitch 45: y = -(sin(-a) / -cos(-a)) with table entries built independently.
        let a = f64::from(-45.0_f32 * std::f32::consts::PI / 180.0);
        let table = |i: i64| ((i & 65535) as f64 / 10430.378350470453).sin() as f32;
        let (s, c) = (
            table((a * 10430.378350470453) as i64),
            -table((a * 10430.378350470453 + 16384.0) as i64),
        );
        let (_, _, y) = cast_direction(0.0, 45.0);
        assert!((y - (-(s / c))).abs() < 1e-3, "{y}");
    }

    /// `HitResult.distanceTo` is squared: a hit 0.5 away rebounds at 0.25.
    #[test]
    fn a_block_hit_rebounds_by_the_squared_distance() {
        let v = Vec3::new(0.0, 0.0, -2.0);
        let hit = Vec3::new(0.0, 0.0, -0.5);
        assert_eq!(rebound(v, hit, Vec3::ZERO), Vec3::new(0.0, 0.0, -0.25));
    }

    #[test]
    fn reeling_an_item_requests_an_immediate_motion_update() {
        let mut item = ItemEntity {
            random: horizon_core::random::LegacyRandom::new(0),
            rotation: [0.0, 0.0],
            join: Default::default(),
            no_gravity: false,
            health: 5,
            fire_ticks: 0,
            id: 1,
            stack: ItemStack::new("minecraft:stone", 1),
            thrower: None,
            target: None,
            position: Vec3::ZERO,
            previous: Vec3::ZERO,
            velocity: Vec3::new(0.2, 0.0, -0.1),
            on_ground: true,
            vertical_collision: false,
            horizontal_collision: false,
            age: 0,
            pickup_delay: 0,
            bob_offset: 0.0,
            ticks: 0,
            needs_sync: false,
            stuck: Vec3::ZERO,
            was_touching_water: false,
        };

        pull_item(&mut item, Vec3::new(0.5, 0.1, -0.2));

        assert!((item.velocity.x - 0.7).abs() < 1.0e-12);
        assert!((item.velocity.y - 0.1).abs() < 1.0e-12);
        assert!((item.velocity.z + 0.3).abs() < 1.0e-12);
        assert!(item.needs_sync);
    }

    #[test]
    fn a_flying_hook_hits_a_small_target_between_ticks() {
        let target = Aabb::new(Vec3::new(0.45, -0.1, -0.1), Vec3::new(0.7, 0.15, 0.1));

        assert!(super::segment_entry(Vec3::ZERO, Vec3::new(0.8, 0.0, 0.0), target).is_some());
        assert!(super::segment_entry(Vec3::ZERO, Vec3::new(0.4, 0.0, 0.0), target).is_none());
    }

    #[test]
    fn a_flying_hook_uses_vanillas_growing_entity_hit_margin() {
        let start = Vec3::ZERO;
        let end = Vec3::new(1.0, 0.0, 0.0);
        let target = Aabb::new(Vec3::new(0.5, 0.31, -0.1), Vec3::new(0.7, 0.5, 0.1));

        assert_eq!(entity_hit_margin(2), 0.0);
        assert_eq!(entity_hit_margin(8), 0.3);
        assert_eq!(
            first_entity_hit(start, end, entity_hit_margin(8), [(1, target)]),
            None
        );
        let within_margin = Aabb::new(Vec3::new(0.5, 0.29, -0.1), Vec3::new(0.7, 0.5, 0.1));
        assert_eq!(
            first_entity_hit(start, end, entity_hit_margin(8), [(1, within_margin)]),
            Some(1)
        );
    }

    #[test]
    fn a_flying_hook_does_not_hit_a_target_it_starts_inside() {
        let target = Aabb::new(Vec3::new(-0.1, -0.1, -0.1), Vec3::new(0.7, 0.15, 0.1));

        assert_eq!(
            first_entity_hit(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), 0.0, [(1, target)]),
            None
        );
    }

    #[test]
    fn a_flying_hook_uses_the_first_target_along_its_path() {
        let start = Vec3::ZERO;
        let end = Vec3::new(1.0, 0.0, 0.0);
        let near = Aabb::new(Vec3::new(0.45, -0.1, -0.1), Vec3::new(0.7, 0.15, 0.1));
        let far = Aabb::new(Vec3::new(0.8, -0.1, -0.1), Vec3::new(0.95, 0.15, 0.1));

        assert_eq!(
            first_entity_hit(start, end, 0.0, [(2, far), (1, near)]),
            Some(1)
        );
    }

    #[test]
    fn bite_triggers_initial_downward_plunge_impulse() {
        for _ in 0..100 {
            let mut hook = Hook::remote(1, Vec3::ZERO);
            assert_eq!(hook.velocity.y, 0.0);
            hook.trigger_bite(0);
            assert!(
                hook.velocity.y < 0.0,
                "vertical velocity should be negative on bite, got {}",
                hook.velocity.y
            );
            assert!(
                hook.velocity.y <= -0.24 && hook.velocity.y >= -0.40,
                "vertical velocity {} not between -0.24 and -0.40",
                hook.velocity.y
            );
            assert!(
                hook.nibble >= 20 && hook.nibble <= 40,
                "nibble {} not in 20..=40",
                hook.nibble
            );
        }
    }

    /// A minecart is pickable (`AbstractMinecart.isPickable`), so the hook
    /// meets it. The cart the owner rides is the same vehicle and is skipped.
    #[test]
    fn a_fishing_hook_meets_a_minecart_but_not_the_one_it_rides() {
        use super::super::minecarts::{CartKind, Minecart};
        let cart = Minecart::new(7, CartKind::Rideable, Vec3::new(0.6, 0.0, 0.0));
        let ridden = Minecart::new(3, CartKind::Chest, Vec3::new(0.2, 0.0, 0.0));
        let boxes = super::minecart_hook_targets([&cart, &ridden].into_iter(), Some(3));
        assert_eq!(
            first_entity_hit(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), 0.0, boxes),
            Some(7)
        );
    }

    /// `FishingHook.pullEntity` scales the hook-to-owner vector by 0.1.
    #[test]
    fn reeling_pulls_a_hooked_minecart_a_tenth_of_the_way() {
        // Still cart at the origin, owner 10 east and 2 up: +1 x and +0.2 y,
        // on top of the cart's own 0.5.
        let pulled = super::pull_hooked(
            Vec3::new(0.5, 0.0, 0.0),
            Vec3::ZERO,
            Vec3::new(10.0, 2.0, 0.0),
        );
        assert_eq!(pulled, Vec3::new(1.5, 0.2, 0.0));
    }
}
