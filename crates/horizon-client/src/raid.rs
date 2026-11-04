//! Raids (`Raid` and `Raids`): the waves of raiders each difficulty sends,
//! where they gather around the village, the boss bar, victory, loss and the
//! celebration, the raid omen a player brings, and the per-dimension
//! `data/minecraft/raids.dat` saved data with vanilla's codec rules.
//!
//! The raid's members live in the game (each raider knows its raid and
//! wave); a raid reads them and the world through [`RaidLevel`] and asks the
//! game for its side effects with [`Action`]s, so the game and the
//! differential test (`tools/test_raid_parity.py`) run the same rules.
use horizon_core::{
    geometry::trig,
    random::{LegacyRandom, RandomSource},
};
use horizon_world::nbt::{JavaString, Tag};
use std::collections::BTreeMap;

pub const MAX_OMEN_LEVEL: i32 = 5;
/// `Raid.RAID_TIMEOUT_TICKS`.
const TIMEOUT: i64 = 48_000;
const PRE_RAID_TICKS: i32 = 300;
const POST_RAID_LIMIT: i32 = 40;
const CELEBRATION_TICKS: i32 = 600;
const MAX_NO_ACTION_TIME: i32 = 2400;
const OUTSIDE_RAID_BOUNDS_TIMEOUT: i32 = 30;
/// `Raids.getNearbyRaid(pos, 9216)`: within 96 blocks.
pub const VALID_RAID_RADIUS_SQR: f64 = 9216.0;
/// `RAID_REMOVAL_THRESHOLD_SQR`: a raider this far from the centre leaves.
const REMOVAL_THRESHOLD_SQR: f64 = 12544.0;
/// `HERO_OF_THE_VILLAGE_DURATION`.
pub const HERO_DURATION: i32 = 48_000;

/// `Mth.clamp(float)`: NaN passes through (`Math.min`).
fn clamp_f32(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if v.is_nan() || v > hi {
        if v.is_nan() { v } else { hi }
    } else {
        v
    }
}
/// `getHealthOfLivingRaiders`: a float sum from zero (Rust's empty float
/// sum is -0.0).
pub fn living_health(members: &[RaiderInfo]) -> f32 {
    members.iter().fold(0.0, |a, m| a + m.health)
}
/// `Vec3i.distSqr`.
fn dist_sqr(a: [i32; 3], b: [i32; 3]) -> f64 {
    (0..3)
        .map(|i| f64::from(a[i]) - f64::from(b[i]))
        .map(|d| d * d)
        .sum()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ongoing,
    Victory,
    Loss,
    Stopped,
}
impl Status {
    pub const ALL: [Self; 4] = [Self::Ongoing, Self::Victory, Self::Loss, Self::Stopped];
    pub fn name(self) -> &'static str {
        match self {
            Self::Ongoing => "ongoing",
            Self::Victory => "victory",
            Self::Loss => "loss",
            Self::Stopped => "stopped",
        }
    }
    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.name() == s)
    }
}

/// `Raid.RaiderType`, in spawn order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RaiderType {
    Vindicator,
    Evoker,
    Pillager,
    Witch,
    Ravager,
}
impl RaiderType {
    pub const ALL: [Self; 5] = [
        Self::Vindicator,
        Self::Evoker,
        Self::Pillager,
        Self::Witch,
        Self::Ravager,
    ];
    pub fn entity(self) -> &'static str {
        match self {
            Self::Vindicator => "minecraft:vindicator",
            Self::Evoker => "minecraft:evoker",
            Self::Pillager => "minecraft:pillager",
            Self::Witch => "minecraft:witch",
            Self::Ravager => "minecraft:ravager",
        }
    }
    /// A freshly created raider's health (its maximum).
    pub fn health(self) -> f32 {
        match self {
            Self::Witch => 26.0,
            Self::Ravager => 100.0,
            _ => 24.0,
        }
    }
    pub fn can_be_leader(self) -> bool {
        !matches!(self, Self::Witch | Self::Ravager)
    }
    /// `spawnsPerWaveBeforeBonus`, by wave number.
    fn spawns(self) -> [i32; 8] {
        match self {
            Self::Vindicator => [0, 0, 2, 0, 1, 4, 2, 5],
            Self::Evoker => [0, 0, 0, 0, 0, 1, 1, 2],
            Self::Pillager => [0, 4, 3, 3, 4, 4, 4, 2],
            Self::Witch => [0, 0, 0, 0, 3, 0, 0, 1],
            Self::Ravager => [0, 0, 0, 1, 0, 1, 0, 2],
        }
    }
}
/// `Raid.getNumGroups`: waves per difficulty (0 peaceful to 3 hard).
pub fn num_groups(difficulty: i32) -> i32 {
    match difficulty {
        1 => 3,
        2 => 5,
        3 => 7,
        _ => 0,
    }
}

/// One raider of a wave, with the raider riding it (ravagers only).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spawn {
    pub kind: RaiderType,
    pub leader: bool,
    pub rider: Option<RaiderType>,
}

/// A member of a raid (`groupRaiderMap`), as the game reports it.
#[derive(Clone, Debug, Default)]
pub struct RaiderInfo {
    pub id: u32,
    pub wave: i32,
    pub health: f32,
    /// `blockPosition()`.
    pub position: [i32; 3],
    pub tick_count: i32,
    pub no_action_time: i32,
    pub ticks_outside: i32,
    /// `isRemoved()` (killed, discarded or unloaded).
    pub removed: bool,
    /// The raider is in another dimension.
    pub other_dimension: bool,
    /// `level.getEntity(uuid)` finds it.
    pub in_level: bool,
    pub patrol_leader: bool,
}

/// `Raid.playSound` for one player. The horn is 13 blocks from them toward
/// the wave centre, heard when they are within 64 blocks or in the raid.
/// Standing on the centre leaves the horn there (`13 / 0` is not a point).
pub fn horn_at(player: [f64; 3], center_x: f64, center_z: f64, in_raid: bool) -> Option<[f64; 3]> {
    let dx = center_x - player[0];
    let dz = center_z - player[2];
    let distance = (dx * dx + dz * dz).sqrt();
    if distance > 64.0 && !in_raid {
        return None;
    }
    if distance == 0.0 {
        return Some(player);
    }
    let k = 13.0 / distance;
    Some([player[0] + k * dx, player[1], player[2] + k * dz])
}

/// The single player a raid sees (`level.players()`).
#[derive(Clone, Copy, Debug)]
pub struct RaidPlayer {
    pub position: [f64; 3],
    pub block: [i32; 3],
    pub alive: bool,
}

/// What a raid asks of the world.
pub trait RaidLevel {
    /// The world difficulty, 0 (peaceful) to 3 (hard).
    fn difficulty(&self) -> i32;
    /// `Level.hasChunkAt`.
    fn has_chunk_at(&self, pos: [i32; 3]) -> bool;
    /// `Level.hasChunksAt(x0, z0, x1, z1)`.
    fn has_chunks_at(&self, x0: i32, z0: i32, x1: i32, z1: i32) -> bool;
    /// `ServerLevel.sectionsToVillage` (0 to 7).
    fn sections_to_village(&self, pos: [i32; 3]) -> i32;
    /// `Heightmap.Types.WORLD_SURFACE` height of a column.
    fn surface_height(&self, x: i32, z: i32) -> i32;
    /// `ServerLevel.isPositionEntityTicking`.
    fn is_position_entity_ticking(&self, pos: [i32; 3]) -> bool;
    /// The ravager's placement accepts the position, or it is air on snow.
    fn ravager_can_spawn(&self, pos: [i32; 3]) -> bool;
    fn player(&self) -> Option<RaidPlayer>;
}
/// `ServerLevel.isVillage`.
pub fn is_village(level: &dyn RaidLevel, pos: [i32; 3]) -> bool {
    level.sections_to_village(pos) <= 1
}

/// What the game must do after a raid tick.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Spawn the raiders of a wave at `pos` (`spawnGroup`), in order, each
    /// joining the raid at `wave`; the leader gets the banner first.
    Spawn {
        pos: [i32; 3],
        wave: i32,
        spawns: Vec<Spawn>,
    },
    /// `playSound`: the raid horn with its seed, heard by the player when
    /// within 64 blocks or in the raid.
    Horn {
        pos: [i32; 3],
        seed: i64,
    },
    /// A raider leaves the raid (`removeFromRaid`: `setCurrentRaid(null)`).
    Remove {
        id: u32,
    },
    SetOutside {
        id: u32,
        ticks: i32,
    },
    /// The raid was won: heroes (in set order) get Hero of the Village.
    Victory {
        heroes: Vec<[i32; 4]>,
        amplifier: i32,
    },
}

/// The raid boss bar's title.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarName {
    Raid,
    Remaining(i32),
    Victory,
    Defeat,
}
/// The raid's `ServerBossEvent` (red, ten notches).
#[derive(Clone, Debug)]
pub struct Bar {
    pub name: BarName,
    pub progress: f32,
    pub visible: bool,
    /// The local player sees the bar (`getPlayers` holds them).
    pub player: bool,
}

#[derive(Clone, Debug)]
pub struct Raid {
    pub started: bool,
    pub active: bool,
    pub ticks_active: i64,
    pub omen_level: i32,
    pub groups_spawned: i32,
    pub cooldown_ticks: i32,
    pub post_raid_ticks: i32,
    pub total_health: f32,
    pub num_groups: i32,
    pub status: Status,
    pub center: [i32; 3],
    /// `heroesOfTheVillage`, in the order they joined the set.
    heroes: Vec<[i32; 4]>,
    pub bar: Bar,
    pub celebration_ticks: i32,
    pub wave_spawn_pos: Option<[i32; 3]>,
    /// `groupToLeaderMap`: wave to the leading raider's id.
    pub leaders: BTreeMap<i32, u32>,
    random: LegacyRandom,
}
impl Raid {
    /// `new Raid(center, difficulty)`.
    pub fn new(center: [i32; 3], difficulty: i32, seed: i64) -> Self {
        Self {
            started: false,
            active: true,
            ticks_active: 0,
            omen_level: 0,
            groups_spawned: 0,
            cooldown_ticks: PRE_RAID_TICKS,
            post_raid_ticks: 0,
            total_health: 0.0,
            num_groups: num_groups(difficulty),
            status: Status::Ongoing,
            center,
            heroes: Vec::new(),
            bar: Bar {
                name: BarName::Raid,
                progress: 0.0,
                visible: true,
                player: false,
            },
            celebration_ticks: 0,
            wave_spawn_pos: None,
            leaders: BTreeMap::new(),
            random: LegacyRandom::new(seed),
        }
    }
    /// Replaces the raid's random (vanilla's is unseeded).
    pub fn reseed(&mut self, seed: i64) {
        self.random = LegacyRandom::new(seed);
    }
    pub fn is_over(&self) -> bool {
        self.is_victory() || self.is_loss()
    }
    pub fn is_victory(&self) -> bool {
        self.status == Status::Victory
    }
    pub fn is_loss(&self) -> bool {
        self.status == Status::Loss
    }
    pub fn is_stopped(&self) -> bool {
        self.status == Status::Stopped
    }
    pub fn has_first_wave_spawned(&self) -> bool {
        self.groups_spawned > 0
    }
    /// `isBetweenWaves`: the next wave is counting down.
    pub fn is_between_waves(&self, raiders_alive: usize) -> bool {
        self.has_first_wave_spawned() && raiders_alive == 0 && self.cooldown_ticks > 0
    }
    pub fn stop(&mut self) {
        self.active = false;
        self.bar.player = false;
        self.status = Status::Stopped;
    }
    fn has_bonus_wave(&self) -> bool {
        self.omen_level > 1
    }
    fn is_final_wave(&self) -> bool {
        self.groups_spawned == self.num_groups
    }
    fn has_spawned_bonus_wave(&self) -> bool {
        self.groups_spawned > self.num_groups
    }
    fn has_more_waves(&self) -> bool {
        if self.has_bonus_wave() {
            !self.has_spawned_bonus_wave()
        } else {
            !self.is_final_wave()
        }
    }
    fn should_spawn_bonus_group(&self, alive: usize) -> bool {
        self.is_final_wave() && alive == 0 && self.has_bonus_wave()
    }
    fn should_spawn_group(&self, alive: usize) -> bool {
        self.cooldown_ticks == 0
            && (self.groups_spawned < self.num_groups || self.should_spawn_bonus_group(alive))
            && alive == 0
    }
    /// `Raid.absorbRaidOmen` for a player with Raid Omen at `amplifier`.
    /// True when this omen triggers the raid (no wave spawned yet: the
    /// `raid_trigger` stat and the `voluntary_exile` advancement).
    pub fn absorb_omen(&mut self, amplifier: i32) -> bool {
        self.omen_level = (self.omen_level + amplifier + 1).clamp(0, MAX_OMEN_LEVEL);
        !self.has_first_wave_spawned()
    }
    /// `Raid.getEnchantOdds`.
    pub fn enchant_odds(&self) -> f32 {
        match self.omen_level {
            2 => 0.1,
            3 => 0.25,
            4 => 0.5,
            5 => 0.75,
            _ => 0.0,
        }
    }
    /// `addHeroOfTheVillage`.
    pub fn add_hero(&mut self, uuid: [i32; 4]) {
        if !self.heroes.contains(&uuid) {
            self.heroes.push(uuid);
        }
    }
    /// The heroes in the order vanilla's `HashSet` iterates them.
    pub fn heroes(&self) -> Vec<[i32; 4]> {
        java_hash_set_order(&self.heroes, 16)
    }
    /// `setLeader`: the raider leading a wave (it wears the banner).
    pub fn set_leader(&mut self, wave: i32, id: u32) {
        self.leaders.insert(wave, id);
    }
    pub fn remove_leader(&mut self, wave: i32) {
        self.leaders.remove(&wave);
    }
    pub fn leader(&self, wave: i32) -> Option<u32> {
        self.leaders.get(&wave).copied()
    }
    /// `updateBossbar` with the health of the living raiders.
    pub fn update_bossbar(&mut self, living_health: f32) {
        self.bar.progress = clamp_f32(living_health / self.total_health, 0.0, 1.0);
    }
    /// `addWaveMob` (joining or loading): the raider's health counts toward
    /// the bar when `update_health`; `living_health` includes it.
    pub fn add_wave_mob(&mut self, health: f32, update_health: bool, living_health: f32) {
        if update_health {
            self.total_health += health;
        }
        self.update_bossbar(living_health);
    }
    /// `removeFromRaid` of a member; `living_health` is without it.
    pub fn remove_from_raid(&mut self, health: f32, from_total: bool, living_health: f32) {
        if from_total {
            self.total_health -= health;
        }
        self.update_bossbar(living_health);
    }

    /// `getDefaultNumSpawns` and `getPotentialBonusSpawns`.
    fn wave_count(&mut self, kind: RaiderType, wave: i32, difficulty: i32, bonus: bool) -> i32 {
        let index = if bonus { self.num_groups } else { wave } as usize;
        let base = kind.spawns()[index.min(7)];
        let easy = difficulty == 1;
        let normal = difficulty == 2;
        let extra = match kind {
            RaiderType::Vindicator | RaiderType::Pillager => {
                if easy {
                    self.random.next_int_bound(2).unwrap_or(0)
                } else if normal {
                    1
                } else {
                    2
                }
            }
            RaiderType::Evoker => return base,
            RaiderType::Witch => {
                if easy || wave <= 2 || wave == 4 {
                    return base;
                }
                1
            }
            RaiderType::Ravager => i32::from(!easy && bonus),
        };
        base + if extra > 0 {
            self.random.next_int_bound(extra + 1).unwrap_or(0)
        } else {
            0
        }
    }
    /// The raiders of wave `wave` (`spawnGroup`): per type the counted
    /// spawns, the first able raider leading, and ravagers carrying a
    /// pillager (wave 5) or an evoker then vindicators (wave 7 and later).
    pub fn compose_wave(&mut self, wave: i32, difficulty: i32, bonus: bool) -> Vec<Spawn> {
        let mut out = Vec::new();
        let mut leader_set = false;
        for kind in RaiderType::ALL {
            let count = self.wave_count(kind, wave, difficulty, bonus);
            let mut ravagers = 0;
            for _ in 0..count {
                let leader = !leader_set && kind.can_be_leader();
                leader_set |= leader;
                let mut rider = None;
                if kind == RaiderType::Ravager {
                    if wave == num_groups(2) {
                        rider = Some(RaiderType::Pillager);
                    } else if wave >= num_groups(3) {
                        rider = Some(if ravagers == 0 {
                            RaiderType::Evoker
                        } else {
                            RaiderType::Vindicator
                        });
                    }
                    ravagers += 1;
                }
                out.push(Spawn {
                    kind,
                    leader,
                    rider,
                });
            }
        }
        out
    }
    /// `Raid.findRandomSpawnPos`: a ring around the centre that closes in as
    /// the countdown runs down.
    fn find_spawn_pos(&mut self, level: &dyn RaidLevel, max_tries: i32) -> Option<[i32; 3]> {
        let seconds = self.cooldown_ticks / 20;
        let how_far = 0.22 * seconds as f32 - 0.24;
        let start = self.random.next_float() * (std::f32::consts::PI * 2.0);
        let far = how_far.floor() as i32;
        for i in 0..max_tries {
            let angle = start + std::f32::consts::PI * i as f32 / 8.0;
            let x = self.center[0]
                + (trig::cos(f64::from(angle)) * 32.0 * how_far).floor() as i32
                + self.random.next_int_bound(3).unwrap_or(0) * far;
            let z = self.center[2]
                + (trig::sin(f64::from(angle)) * 32.0 * how_far).floor() as i32
                + self.random.next_int_bound(3).unwrap_or(0) * far;
            let y = level.surface_height(x, z);
            if (y - self.center[1]).abs() > 96 {
                continue;
            }
            let pos = [x, y, z];
            if (!is_village(level, pos) || seconds <= 7)
                && level.has_chunks_at(x - 10, z - 10, x + 10, z + 10)
                && level.is_position_entity_ticking(pos)
                && level.ravager_can_spawn(pos)
            {
                return Some(pos);
            }
        }
        None
    }
    /// `moveRaidCenterToNearbyVillageSection`: the closest village section
    /// centre within two sections (first of equals in `SectionPos.cube` order).
    fn move_center_to_nearby_village(&mut self, level: &dyn RaidLevel) {
        let c = self.center;
        let (sx, sy, sz) = (c[0] >> 4, c[1] >> 4, c[2] >> 4);
        let mut best: Option<([i32; 3], f64)> = None;
        // SectionPos.cube: BetweenClosed over x fastest, then y, then z.
        for dz in -2..=2 {
            for dy in -2..=2 {
                for dx in -2..=2 {
                    let s = [sx + dx, sy + dy, sz + dz];
                    let center = [s[0] * 16 + 8, s[1] * 16 + 8, s[2] * 16 + 8];
                    if !is_village(level, center) {
                        continue;
                    }
                    let d = dist_sqr(center, c);
                    if best.is_none_or(|b| d < b.1) {
                        best = Some((center, d));
                    }
                }
            }
        }
        if let Some((center, _)) = best {
            self.center = center;
        }
    }
    /// The countdown's bar progress and the local player's membership
    /// (`updatePlayers`).
    fn update_players(&mut self, in_raid: bool) {
        self.bar.player = in_raid;
    }

    /// `Raid.tick`. `members` are the raid's raiders; `in_raid` answers
    /// whether the player stands in this raid (`getRaidAt(pos) == this`,
    /// with the raid's current centre and state).
    pub fn tick(
        &mut self,
        level: &dyn RaidLevel,
        members: &[RaiderInfo],
        in_raid: &dyn Fn(&Raid, [i32; 3]) -> bool,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        if self.is_stopped() {
            return actions;
        }
        let player_in = |raid: &Raid| {
            level
                .player()
                .is_some_and(|p| p.alive && in_raid(raid, p.block))
        };
        if self.status == Status::Ongoing {
            let mut members: Vec<RaiderInfo> = members.to_vec();
            let was_active = self.active;
            self.active = level.has_chunk_at(self.center);
            if level.difficulty() == 0 {
                self.stop();
                return actions;
            }
            if was_active != self.active {
                self.bar.visible = self.active;
            }
            if !self.active {
                return actions;
            }
            if !is_village(level, self.center) {
                self.move_center_to_nearby_village(level);
            }
            if !is_village(level, self.center) {
                if self.groups_spawned > 0 {
                    self.status = Status::Loss;
                } else {
                    self.stop();
                }
            }
            self.ticks_active += 1;
            if self.ticks_active >= TIMEOUT {
                self.stop();
                return actions;
            }
            let alive = members.len();
            if alive == 0 && self.has_more_waves() {
                if self.cooldown_ticks <= 0 {
                    if self.cooldown_ticks == 0 && self.groups_spawned > 0 {
                        self.cooldown_ticks = PRE_RAID_TICKS;
                        self.bar.name = BarName::Raid;
                        return actions;
                    }
                } else {
                    let cached = self.wave_spawn_pos.is_some();
                    let mut try_find = !cached && self.cooldown_ticks % 5 == 0;
                    if let Some(p) = self.wave_spawn_pos
                        && !level.is_position_entity_ticking(p)
                    {
                        try_find = true;
                    }
                    if try_find {
                        self.wave_spawn_pos = self.find_spawn_pos(level, 8);
                    }
                    if self.cooldown_ticks == PRE_RAID_TICKS || self.cooldown_ticks % 20 == 0 {
                        let p = player_in(self);
                        self.update_players(p);
                    }
                    self.cooldown_ticks -= 1;
                    self.bar.progress = clamp_f32(
                        (PRE_RAID_TICKS - self.cooldown_ticks) as f32 / 300.0,
                        0.0,
                        1.0,
                    );
                }
            }
            if self.ticks_active % 20 == 0 {
                let p = player_in(self);
                self.update_players(p);
                self.update_raiders(level, &mut members, &mut actions);
                self.bar.name = if alive > 0 && alive <= 2 {
                    BarName::Remaining(alive as i32)
                } else {
                    BarName::Raid
                };
            }
            let mut sound = false;
            let mut attempt = 0;
            while self.should_spawn_group(members.len()) {
                let pos = match self.wave_spawn_pos {
                    Some(p) => Some(p),
                    None => self.find_spawn_pos(level, 20),
                };
                if let Some(pos) = pos {
                    self.started = true;
                    let spawns = self.spawn_group(level, pos, &mut members);
                    actions.push(Action::Spawn {
                        pos,
                        wave: self.groups_spawned,
                        spawns,
                    });
                    if !sound {
                        actions.push(Action::Horn {
                            pos,
                            seed: self.random.next_long(),
                        });
                        sound = true;
                    }
                } else {
                    attempt += 1;
                }
                if attempt > 5 {
                    self.stop();
                    break;
                }
            }
            if self.started && !self.has_more_waves() && alive == 0 {
                if self.post_raid_ticks < POST_RAID_LIMIT {
                    self.post_raid_ticks += 1;
                } else {
                    self.status = Status::Victory;
                    actions.push(Action::Victory {
                        heroes: self.heroes(),
                        amplifier: self.omen_level - 1,
                    });
                }
            }
        } else if self.is_over() {
            self.celebration_ticks += 1;
            if self.celebration_ticks >= CELEBRATION_TICKS {
                self.stop();
                return actions;
            }
            if self.celebration_ticks % 20 == 0 {
                let p = player_in(self);
                self.update_players(p);
                self.bar.visible = true;
                if self.is_victory() {
                    self.bar.progress = 0.0;
                    self.bar.name = BarName::Victory;
                } else {
                    self.bar.name = BarName::Defeat;
                }
            }
        }
        actions
    }
    /// `spawnGroup`: the wave's raiders join at `pos` (each `addWaveMob`
    /// counting its health), then the wave counter moves on.
    fn spawn_group(
        &mut self,
        level: &dyn RaidLevel,
        pos: [i32; 3],
        members: &mut Vec<RaiderInfo>,
    ) -> Vec<Spawn> {
        let wave = self.groups_spawned + 1;
        self.total_health = 0.0;
        let bonus = self.should_spawn_bonus_group(members.len());
        let spawns = self.compose_wave(wave, level.difficulty(), bonus);
        let mut join = |raid: &mut Self, kind: RaiderType| {
            members.push(RaiderInfo {
                wave,
                health: kind.health(),
                position: pos,
                in_level: true,
                ..RaiderInfo::default()
            });
            let living = living_health(members);
            raid.add_wave_mob(kind.health(), true, living);
        };
        for s in &spawns {
            join(self, s.kind);
            if let Some(rider) = s.rider {
                join(self, rider);
            }
        }
        self.wave_spawn_pos = None;
        self.groups_spawned += 1;
        let living = living_health(members);
        self.update_bossbar(living);
        spawns
    }
    /// `Raid.updateRaiders`: members that are gone, too far or idle outside
    /// the village too long leave the raid.
    fn update_raiders(
        &mut self,
        level: &dyn RaidLevel,
        members: &mut Vec<RaiderInfo>,
        actions: &mut Vec<Action>,
    ) {
        let mut leaving = Vec::new();
        for r in members.iter_mut() {
            if r.removed
                || r.other_dimension
                || dist_sqr(self.center, r.position) >= REMOVAL_THRESHOLD_SQR
            {
                leaving.push(r.id);
            } else if r.tick_count > 600 {
                if !r.in_level {
                    leaving.push(r.id);
                }
                if !is_village(level, r.position) && r.no_action_time > MAX_NO_ACTION_TIME {
                    r.ticks_outside += 1;
                    actions.push(Action::SetOutside {
                        id: r.id,
                        ticks: r.ticks_outside,
                    });
                }
                if r.ticks_outside >= OUTSIDE_RAID_BOUNDS_TIMEOUT && !leaving.contains(&r.id) {
                    leaving.push(r.id);
                }
            }
        }
        for id in leaving {
            let Some(i) = members.iter().position(|m| m.id == id) else {
                continue;
            };
            let r = members.remove(i);
            let living = living_health(members);
            self.remove_from_raid(r.health, true, living);
            actions.push(Action::Remove { id });
            if r.patrol_leader {
                self.remove_leader(r.wave);
            }
        }
    }
}

// ------------------------------------------------------------------ Raids

/// `Raids`: the raids of a dimension by id, in vanilla's
/// `Int2ObjectOpenHashMap` (its iteration order is the tick and save order).
#[derive(Clone, Debug)]
pub struct Raids {
    pub map: BTreeMap<i32, Raid>,
    order: IntOpenHashOrder,
    pub next_id: i32,
    pub tick: i32,
    /// Changed since the last save.
    pub dirty: bool,
}
impl Default for Raids {
    /// `new Raids()` (dirty, so a fresh level writes its file).
    fn default() -> Self {
        Self {
            map: BTreeMap::new(),
            order: IntOpenHashOrder::new(),
            next_id: 1,
            tick: 0,
            dirty: true,
        }
    }
}
impl Raids {
    pub fn get(&self, id: i32) -> Option<&Raid> {
        self.map.get(&id)
    }
    pub fn get_mut(&mut self, id: i32) -> Option<&mut Raid> {
        self.map.get_mut(&id)
    }
    /// The raid ids in iteration order.
    pub fn ids(&self) -> Vec<i32> {
        self.order.iter()
    }
    fn insert(&mut self, id: i32, raid: Raid) {
        if self.map.insert(id, raid).is_none() {
            self.order.put(id);
        }
    }
    /// `Raids.getNearbyRaid(pos, 9216)`: the closest active raid.
    pub fn nearby(&self, pos: [i32; 3]) -> Option<(i32, &Raid)> {
        nearest(
            self.ids()
                .into_iter()
                .filter_map(|id| Some((id, self.map.get(&id)?))),
            pos,
        )
        .map(|id| (id, &self.map[&id]))
    }
    /// `Raids.createOrExtendRaid` once the caller found the village centre
    /// (`raid_center`): the raid at that position or a new one, then the
    /// player's Raid Omen at `omen_amplifier` absorbed. Returns the raid id
    /// and whether the omen triggered it.
    pub fn create_or_extend(
        &mut self,
        raid_center: [i32; 3],
        difficulty: i32,
        omen_amplifier: Option<i32>,
        seed: i64,
    ) -> (i32, bool) {
        let id = match self.nearby(raid_center) {
            Some((id, _)) => id,
            None => {
                self.next_id += 1;
                let id = self.next_id;
                self.insert(id, Raid::new(raid_center, difficulty, seed));
                id
            }
        };
        let mut triggered = false;
        if let Some(raid) = self.map.get_mut(&id)
            && (!raid.started || raid.omen_level < MAX_OMEN_LEVEL)
            && let Some(amp) = omen_amplifier
        {
            triggered = raid.absorb_omen(amp);
        }
        self.dirty = true;
        (id, triggered)
    }
    /// `Raids.tick`: each raid in map order (stopped ones removed, every
    /// raid stopped when the `raids` rule is off). `members` gives a raid's
    /// raiders. Returns the raids' actions.
    pub fn tick(
        &mut self,
        level: &dyn RaidLevel,
        rules_raids: bool,
        members: &dyn Fn(i32) -> Vec<RaiderInfo>,
    ) -> Vec<(i32, Action)> {
        self.tick += 1;
        let mut out = Vec::new();
        let map = &mut self.map;
        let mut removed_any = false;
        let ids = self.order.iter();
        let mut visit = |id: i32| -> bool {
            // The raids in map order as they stand when this one ticks.
            let snapshot: Vec<(i32, [i32; 3], bool)> = ids
                .iter()
                .filter_map(|i| map.get(i).map(|r| (*i, r.center, r.active)))
                .collect();
            let Some(raid) = map.get_mut(&id) else {
                return false;
            };
            if !rules_raids {
                raid.stop();
            }
            if raid.is_stopped() {
                removed_any = true;
                return true;
            }
            let order = &snapshot;
            let in_raid = |r: &Raid, pos: [i32; 3]| {
                let mut best: Option<(i32, f64)> = None;
                for (i, c, active) in order {
                    let (c, active) = if *i == id {
                        (r.center, r.active)
                    } else {
                        (*c, *active)
                    };
                    let d = dist_sqr(c, pos);
                    if active && d < best.map_or(VALID_RAID_RADIUS_SQR, |b| b.1) {
                        best = Some((*i, d));
                    }
                }
                best.is_some_and(|b| b.0 == id)
            };
            for a in raid.tick(level, &members(id), &in_raid) {
                out.push((id, a));
            }
            false
        };
        let gone = self.order.iterate_removing(&mut visit);
        for id in gone {
            map.remove(&id);
        }
        if removed_any || self.tick % 200 == 0 {
            self.dirty = true;
        }
        // Raid.tick marks the data dirty while a raid runs.
        if self
            .map
            .values()
            .any(|r| r.status == Status::Ongoing && r.active)
        {
            self.dirty = true;
        }
        out
    }

    /// `Raids.CODEC`: `{raids: [...], next_id, tick}` in the entry set's
    /// stream order.
    pub fn to_nbt(&self) -> Tag {
        let raids: Vec<Tag> = self
            .order
            .stream()
            .into_iter()
            .map(|id| {
                let mut tag = self.map[&id].to_nbt();
                if let Tag::Compound(map) = &mut tag {
                    map.insert("id".into(), Tag::Int(id));
                }
                tag
            })
            .collect();
        let mut tag = Tag::compound([
            ("next_id", Tag::Int(self.next_id)),
            ("tick", Tag::Int(self.tick)),
        ]);
        // optionalFieldOf("raids", List.of()) leaves the default out.
        if !raids.is_empty()
            && let Tag::Compound(map) = &mut tag
        {
            map.insert("raids".into(), Tag::List(raids));
        }
        tag
    }
    /// `Raids.CODEC.parse(...).resultOrPartial()`, or a fresh `Raids` when
    /// nothing decodes. Raids that fail to decode are dropped; a raid whose
    /// heroes partly fail keeps the others.
    pub fn from_nbt(tag: &Tag, seed: i64) -> Self {
        let Some(decoded) = decode_raids(tag) else {
            return Self::default();
        };
        let mut out = Self {
            dirty: false,
            next_id: decoded.next_id,
            tick: decoded.tick,
            ..Self::default()
        };
        for (i, (id, raid)) in decoded.raids.into_iter().enumerate() {
            let mut raid = raid;
            raid.random = LegacyRandom::new(seed.wrapping_add(i as i64));
            // Int2ObjectOpenHashMap.put: a repeated id replaces the raid.
            out.map.insert(id, raid);
            out.order.put(id);
        }
        out
    }
}
/// The id of the closest active raid within 96 blocks (`getNearbyRaid`).
fn nearest<'a>(raids: impl Iterator<Item = (i32, &'a Raid)>, pos: [i32; 3]) -> Option<i32> {
    let mut best: Option<(i32, f64)> = None;
    for (id, r) in raids {
        let d = dist_sqr(r.center, pos);
        if r.active && d < best.map_or(VALID_RAID_RADIUS_SQR, |b| b.1) {
            best = Some((id, d));
        }
    }
    best.map(|b| b.0)
}

/// `Raids.createOrExtendRaid`'s centre: the mean of the occupied village
/// points of interest within 64 blocks (`BlockPos.containing`), else the
/// omen's position.
pub fn raid_center(omen_pos: [i32; 3], village_pois: &[[i32; 3]]) -> [i32; 3] {
    let near: Vec<&[i32; 3]> = village_pois
        .iter()
        .filter(|p| {
            (0..3).all(|i| i == 1 || (p[i] - omen_pos[i]).abs() <= 64)
                && dist_sqr(**p, omen_pos) <= 4096.0
        })
        .collect();
    if near.is_empty() {
        return omen_pos;
    }
    let mut total = [0.0f64; 3];
    for p in &near {
        for i in 0..3 {
            total[i] += f64::from(p[i]);
        }
    }
    let scale = 1.0 / near.len() as f64;
    [
        (total[0] * scale).floor() as i32,
        (total[1] * scale).floor() as i32,
        (total[2] * scale).floor() as i32,
    ]
}

// ------------------------------------------------------------------ codec

impl Raid {
    /// `Raid.MAP_CODEC` fields.
    pub fn to_nbt(&self) -> Tag {
        Tag::compound([
            ("started", Tag::Byte(i8::from(self.started))),
            ("active", Tag::Byte(i8::from(self.active))),
            ("ticks_active", Tag::Long(self.ticks_active)),
            ("raid_omen_level", Tag::Int(self.omen_level)),
            ("groups_spawned", Tag::Int(self.groups_spawned)),
            ("cooldown_ticks", Tag::Int(self.cooldown_ticks)),
            ("post_raid_ticks", Tag::Int(self.post_raid_ticks)),
            ("total_health", Tag::Float(self.total_health.to_bits())),
            ("group_count", Tag::Int(self.num_groups)),
            ("status", Tag::String(self.status.name().into())),
            ("center", Tag::IntArray(self.center.to_vec())),
            (
                "heroes_of_the_village",
                Tag::List(
                    self.heroes()
                        .iter()
                        .map(|u| Tag::IntArray(u.to_vec()))
                        .collect(),
                ),
            ),
        ])
    }
    /// The private codec constructor: a loaded raid's bar is visible with
    /// full progress until the raid updates it.
    fn loaded(fields: RaidFields) -> Self {
        let mut raid = Self::new(fields.center, 0, 0);
        raid.started = fields.started;
        raid.active = fields.active;
        raid.ticks_active = fields.ticks_active;
        raid.omen_level = fields.omen_level;
        raid.groups_spawned = fields.groups_spawned;
        raid.cooldown_ticks = fields.cooldown_ticks;
        raid.post_raid_ticks = fields.post_raid_ticks;
        raid.total_health = fields.total_health;
        raid.num_groups = fields.num_groups;
        raid.status = fields.status;
        // Sets.newHashSet(list), then addAll into the raid's own set.
        let cap = java_table_size_for(java_hash_set_capacity(fields.heroes.len()));
        let mut unique: Vec<[i32; 4]> = Vec::new();
        for h in fields.heroes {
            if !unique.contains(&h) {
                unique.push(h);
            }
        }
        raid.heroes = java_hash_set_order(&unique, cap);
        raid.bar.progress = 1.0;
        raid
    }
}

struct RaidFields {
    started: bool,
    active: bool,
    ticks_active: i64,
    omen_level: i32,
    groups_spawned: i32,
    cooldown_ticks: i32,
    post_raid_ticks: i32,
    total_health: f32,
    num_groups: i32,
    status: Status,
    center: [i32; 3],
    heroes: Vec<[i32; 4]>,
}

/// A DataFixerUpper result: a value, and whether an error was reported
/// (`Some` with an error is a partial result).
type Decoded<T> = (Option<T>, bool);

/// `NbtOps.getNumberValue`: the numeric tags as Java numbers.
#[derive(Clone, Copy)]
enum Number {
    Long(i64),
    Float(f32),
    Double(f64),
}
fn number(tag: &Tag) -> Option<Number> {
    Some(match tag {
        Tag::Byte(v) => Number::Long(i64::from(*v)),
        Tag::Short(v) => Number::Long(i64::from(*v)),
        Tag::Int(v) => Number::Long(i64::from(*v)),
        Tag::Long(v) => Number::Long(*v),
        Tag::Float(b) => Number::Float(f32::from_bits(*b)),
        Tag::Double(b) => Number::Double(f64::from_bits(*b)),
        _ => return None,
    })
}
impl Number {
    /// `Number.intValue`: integers truncate, floating values convert like
    /// Java's `(int)` cast.
    fn int(self) -> i32 {
        match self {
            Self::Long(v) => v as i32,
            Self::Float(v) => v as i32,
            Self::Double(v) => v as i32,
        }
    }
    fn long(self) -> i64 {
        match self {
            Self::Long(v) => v,
            Self::Float(v) => v as i64,
            Self::Double(v) => v as i64,
        }
    }
    fn float(self) -> f32 {
        match self {
            Self::Long(v) => v as f32,
            Self::Float(v) => v,
            Self::Double(v) => v as f32,
        }
    }
    fn double(self) -> f64 {
        match self {
            Self::Long(v) => v as f64,
            Self::Float(v) => f64::from(v),
            Self::Double(v) => v,
        }
    }
}
/// `DynamicOps.getIntStream` through `NbtOps`: an int array, or a list or
/// array whose elements are all numbers.
fn int_stream(tag: &Tag) -> Option<Vec<i32>> {
    match tag {
        Tag::IntArray(v) => Some(v.clone()),
        Tag::ByteArray(v) => Some(v.iter().map(|b| i32::from(*b as i8)).collect()),
        Tag::LongArray(v) => Some(v.iter().map(|l| *l as i32).collect()),
        Tag::List(items) => items.iter().map(|t| number(t).map(Number::int)).collect(),
        _ => None,
    }
}
/// `ops.getList`: the elements of any collection tag.
fn collection(tag: &Tag) -> Option<Vec<Tag>> {
    match tag {
        Tag::List(items) => Some(items.clone()),
        Tag::ByteArray(v) => Some(v.iter().map(|b| Tag::Byte(*b as i8)).collect()),
        Tag::IntArray(v) => Some(v.iter().map(|i| Tag::Int(*i)).collect()),
        Tag::LongArray(v) => Some(v.iter().map(|i| Tag::Long(*i)).collect()),
        _ => None,
    }
}
fn field<'a>(tag: &'a Tag, key: &str) -> Option<&'a Tag> {
    match tag {
        Tag::Compound(map) => map.get(&JavaString::from(key)),
        _ => None,
    }
}
/// `Codec.list(UUIDUtil.CODEC)`: good entries kept, an error if any failed.
fn decode_uuids(tag: &Tag) -> Decoded<Vec<[i32; 4]>> {
    let Some(items) = collection(tag) else {
        return (None, true);
    };
    let mut out = Vec::new();
    let mut error = false;
    for item in &items {
        // Util.fixedSize: a longer stream is a partial result of its start.
        match int_stream(item) {
            Some(v) if v.len() >= 4 => {
                error |= v.len() > 4;
                out.push([v[0], v[1], v[2], v[3]]);
            }
            _ => error = true,
        }
    }
    (Some(out), error)
}
/// `Raid.MAP_CODEC` over a compound (missing or mistyped fields fail with
/// no partial result; a partial heroes list leaves a partial raid).
fn decode_raid(tag: &Tag) -> Decoded<RaidFields> {
    let num = |k: &str| field(tag, k).and_then(number);
    let (
        Some(started),
        Some(active),
        Some(ticks_active),
        Some(omen),
        Some(groups),
        Some(cooldown),
        Some(post),
        Some(health),
        Some(count),
    ) = (
        num("started"),
        num("active"),
        num("ticks_active"),
        num("raid_omen_level"),
        num("groups_spawned"),
        num("cooldown_ticks"),
        num("post_raid_ticks"),
        num("total_health"),
        num("group_count"),
    )
    else {
        return (None, true);
    };
    let status = match field(tag, "status") {
        Some(Tag::String(s)) => Status::from_name(&s.to_string_lossy()),
        _ => None,
    };
    // Util.fixedSize: a longer stream is a partial result of its start.
    let center = field(tag, "center")
        .and_then(int_stream)
        .filter(|v| v.len() >= 3);
    let (Some(status), Some(center)) = (status, center) else {
        return (None, true);
    };
    let Some(heroes_tag) = field(tag, "heroes_of_the_village") else {
        return (None, true);
    };
    let (heroes, error) = decode_uuids(heroes_tag);
    let Some(heroes) = heroes else {
        return (None, true);
    };
    let fields = RaidFields {
        started: started.double() != 0.0,
        active: active.double() != 0.0,
        ticks_active: ticks_active.long(),
        omen_level: omen.int(),
        groups_spawned: groups.int(),
        cooldown_ticks: cooldown.int(),
        post_raid_ticks: post.int(),
        total_health: health.float(),
        num_groups: count.int(),
        status,
        center: [center[0], center[1], center[2]],
        heroes,
    };
    (Some(fields), error)
}
struct DecodedRaids {
    raids: Vec<(i32, Raid)>,
    next_id: i32,
    tick: i32,
}
/// `Raids.CODEC` with `resultOrPartial`.
fn decode_raids(tag: &Tag) -> Option<DecodedRaids> {
    if !matches!(tag, Tag::Compound(_)) {
        return None;
    }
    let mut raids = Vec::new();
    if let Some(list) = field(tag, "raids") {
        // optionalFieldOf: a value that is no list decodes partially, to none.
        let items = collection(list).unwrap_or_default();
        for item in &items {
            let id = field(item, "id").and_then(number);
            let (raid, _) = if matches!(item, Tag::Compound(_)) {
                decode_raid(item)
            } else {
                (None, true)
            };
            if let (Some(id), Some(raid)) = (id, raid) {
                raids.push((id.int(), Raid::loaded(raid)));
            }
        }
    }
    let next_id = field(tag, "next_id").and_then(number)?.int();
    let tick = field(tag, "tick").and_then(number)?.int();
    Some(DecodedRaids {
        raids,
        next_id,
        tick,
    })
}

// ------------------------------------------------------------------ Java collections

/// `HashMap.hash` of a `UUID` built from its four ints.
fn uuid_hash(u: [i32; 4]) -> i32 {
    let most = (i64::from(u[0]) << 32) | (i64::from(u[1]) & 0xffff_ffff);
    let least = (i64::from(u[2]) << 32) | (i64::from(u[3]) & 0xffff_ffff);
    let hilo = most ^ least;
    let h = ((hilo >> 32) as i32) ^ (hilo as i32);
    h ^ ((h as u32) >> 16) as i32
}
/// `HashSet(Collection)`'s requested capacity (JDK 19+: `newHashMap(max(n, 12))`).
fn java_hash_set_capacity(n: usize) -> usize {
    (n.max(12) as f64 / 0.75).ceil() as usize
}
/// `HashMap.tableSizeFor`.
fn java_table_size_for(cap: usize) -> usize {
    cap.max(1).next_power_of_two()
}
/// The iteration order of a `HashSet<UUID>` whose table started at
/// `capacity` (doubling past three quarters full), given insertion order.
pub fn java_hash_set_order(items: &[[i32; 4]], capacity: usize) -> Vec<[i32; 4]> {
    let mut cap = capacity.max(1);
    while items.len() as f64 > cap as f64 * 0.75 {
        cap *= 2;
    }
    let mut keyed: Vec<(usize, usize)> = items
        .iter()
        .enumerate()
        .map(|(i, u)| ((uuid_hash(*u) as u32 as usize) & (cap - 1), i))
        .collect();
    keyed.sort();
    keyed.into_iter().map(|(_, i)| items[i]).collect()
}

/// `HashCommon.mix` for ints.
fn mix(x: i32) -> i32 {
    let h = x.wrapping_mul(0x9E37_79B9_u32 as i32);
    h ^ ((h as u32) >> 16) as i32
}
fn array_size(expected: usize) -> usize {
    ((expected as f64 / 0.75).ceil() as usize)
        .next_power_of_two()
        .max(2)
}
fn max_fill(n: usize) -> usize {
    ((n as f64 * 0.75).ceil() as usize).min(n - 1)
}
/// The key layout of a fastutil `Int2ObjectOpenHashMap` (default size and
/// load factor), for its iteration order.
#[derive(Clone, Debug)]
struct IntOpenHashOrder {
    key: Vec<i32>,
    null_key: bool,
    n: usize,
    min_n: usize,
    size: usize,
}
impl IntOpenHashOrder {
    fn new() -> Self {
        let n = array_size(16);
        Self {
            key: vec![0; n],
            null_key: false,
            n,
            min_n: n,
            size: 0,
        }
    }
    fn mask(&self) -> usize {
        self.n - 1
    }
    fn slot(&self, k: i32) -> usize {
        (mix(k) as u32 as usize) & self.mask()
    }
    fn find(&self, k: i32) -> Option<usize> {
        if k == 0 {
            return self.null_key.then_some(self.n);
        }
        let mut pos = self.slot(k);
        loop {
            match self.key[pos] {
                0 => return None,
                c if c == k => return Some(pos),
                _ => pos = (pos + 1) & self.mask(),
            }
        }
    }
    fn put(&mut self, k: i32) {
        if self.find(k).is_some() {
            return;
        }
        if k == 0 {
            self.null_key = true;
        } else {
            let mut pos = self.slot(k);
            while self.key[pos] != 0 {
                pos = (pos + 1) & self.mask();
            }
            self.key[pos] = k;
        }
        let before = self.size;
        self.size += 1;
        if before >= max_fill(self.n) {
            self.rehash(array_size(self.size + 1));
        }
    }
    fn rehash(&mut self, new_n: usize) {
        let mask = new_n - 1;
        let mut new_key = vec![0; new_n];
        let mut i = self.n;
        let real = self.size - usize::from(self.null_key);
        for _ in 0..real {
            loop {
                i -= 1;
                if self.key[i] != 0 {
                    break;
                }
            }
            let k = self.key[i];
            let mut pos = (mix(k) as u32 as usize) & mask;
            while new_key[pos] != 0 {
                pos = (pos + 1) & mask;
            }
            new_key[pos] = k;
        }
        self.n = new_n;
        self.key = new_key;
    }
    /// `shiftKeys` after the key at `pos` went; returns keys that wrapped
    /// from the start of the table to its end.
    fn shift_keys(&mut self, mut pos: usize) -> Vec<i32> {
        let mut wrapped = Vec::new();
        let mask = self.mask();
        loop {
            let last = pos;
            pos = (last + 1) & mask;
            let curr = loop {
                let curr = self.key[pos];
                if curr == 0 {
                    self.key[last] = 0;
                    return wrapped;
                }
                let slot = (mix(curr) as u32 as usize) & mask;
                let moves = if last <= pos {
                    last >= slot || slot > pos
                } else {
                    last >= slot && slot > pos
                };
                if moves {
                    break curr;
                }
                pos = (pos + 1) & mask;
            };
            if pos < last {
                wrapped.push(curr);
            }
            self.key[last] = curr;
        }
    }
    /// `remove(k)`, shrinking a sparse table.
    fn remove(&mut self, k: i32) {
        if k == 0 {
            if self.null_key {
                self.null_key = false;
                self.size -= 1;
            }
            return;
        }
        let Some(pos) = self.find(k) else { return };
        self.size -= 1;
        self.shift_keys(pos);
        if self.n > self.min_n && self.size < max_fill(self.n) / 4 && self.n > 16 {
            self.rehash(self.n / 2);
        }
    }
    /// The entry set's stream (`MapSpliterator`): the null key, then the
    /// table upward. `Raids.CODEC` encodes in this order.
    fn stream(&self) -> Vec<i32> {
        let mut out = Vec::new();
        if self.null_key {
            out.push(0);
        }
        out.extend(self.key[..self.n].iter().copied().filter(|k| *k != 0));
        out
    }
    /// The iterator (`MapIterator`): the null key, then the table downward.
    fn iter(&self) -> Vec<i32> {
        let mut out = Vec::new();
        if self.null_key {
            out.push(0);
        }
        for pos in (0..self.n).rev() {
            if self.key[pos] != 0 {
                out.push(self.key[pos]);
            }
        }
        out
    }
    /// A `values().iterator()` pass that removes the keys `visit` says to
    /// (the iterator's own removal, with wrapped entries); returns them.
    fn iterate_removing(&mut self, visit: &mut dyn FnMut(i32) -> bool) -> Vec<i32> {
        let mut removed = Vec::new();
        let mut pos = self.n as isize;
        let mut c = self.size;
        let mut must_null = self.null_key;
        let mut wrapped: Vec<i32> = Vec::new();
        while c > 0 {
            c -= 1;
            // nextEntry
            let (k, last): (i32, Option<usize>) = if must_null {
                must_null = false;
                (0, Some(self.n))
            } else {
                loop {
                    pos -= 1;
                    if pos < 0 {
                        let k = wrapped[(-pos - 1) as usize];
                        break (k, None);
                    }
                    if self.key[pos as usize] != 0 {
                        break (self.key[pos as usize], Some(pos as usize));
                    }
                }
            };
            if visit(k) {
                removed.push(k);
                match last {
                    Some(l) if l == self.n => {
                        self.null_key = false;
                        self.size -= 1;
                    }
                    Some(l) => {
                        wrapped.extend(self.shift_keys(l));
                        self.size -= 1;
                    }
                    None => self.remove(k),
                }
            }
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Level {
        difficulty: i32,
        village: bool,
        loaded: bool,
    }
    impl RaidLevel for Level {
        fn difficulty(&self) -> i32 {
            self.difficulty
        }
        fn has_chunk_at(&self, _: [i32; 3]) -> bool {
            self.loaded
        }
        fn has_chunks_at(&self, _: i32, _: i32, _: i32, _: i32) -> bool {
            true
        }
        fn sections_to_village(&self, _: [i32; 3]) -> i32 {
            if self.village { 0 } else { 7 }
        }
        fn surface_height(&self, _: i32, _: i32) -> i32 {
            64
        }
        fn is_position_entity_ticking(&self, _: [i32; 3]) -> bool {
            true
        }
        fn ravager_can_spawn(&self, _: [i32; 3]) -> bool {
            true
        }
        fn player(&self) -> Option<RaidPlayer> {
            None
        }
    }
    fn level(difficulty: i32) -> Level {
        Level {
            difficulty,
            village: true,
            loaded: true,
        }
    }
    fn nobody(_: &Raid, _: [i32; 3]) -> bool {
        false
    }
    fn count(spawns: &[Spawn], kind: RaiderType) -> usize {
        spawns.iter().filter(|s| s.kind == kind).count()
    }

    #[test]
    fn waves_per_difficulty() {
        assert_eq!(num_groups(0), 0);
        assert_eq!(num_groups(1), 3);
        assert_eq!(num_groups(2), 5);
        assert_eq!(num_groups(3), 7);
    }
    #[test]
    fn ravagers_carry_riders_by_wave() {
        let mut raid = Raid::new([0, 64, 0], 3, 7);
        let w5 = raid.compose_wave(5, 3, false);
        assert!(
            w5.iter()
                .filter(|s| s.kind == RaiderType::Ravager)
                .all(|s| s.rider == Some(RaiderType::Pillager))
        );
        let w7 = raid.compose_wave(7, 3, false);
        let riders: Vec<_> = w7
            .iter()
            .filter(|s| s.kind == RaiderType::Ravager)
            .map(|s| s.rider)
            .collect();
        assert_eq!(riders[0], Some(RaiderType::Evoker));
        assert!(
            riders[1..]
                .iter()
                .all(|r| *r == Some(RaiderType::Vindicator))
        );
        assert_eq!(count(&w7, RaiderType::Evoker), 2);
    }
    #[test]
    fn only_able_raiders_lead() {
        let mut raid = Raid::new([0, 64, 0], 2, 1);
        let w = raid.compose_wave(4, 2, false);
        assert_eq!(w.iter().filter(|s| s.leader).count(), 1);
        assert!(w.iter().find(|s| s.leader).unwrap().kind.can_be_leader());
    }
    #[test]
    fn omen_levels_stack_and_clamp() {
        let mut raid = Raid::new([0, 64, 0], 2, 1);
        assert!(raid.absorb_omen(0));
        assert_eq!(raid.omen_level, 1);
        raid.absorb_omen(4);
        assert_eq!(raid.omen_level, 5);
        assert_eq!(raid.enchant_odds(), 0.75);
    }
    #[test]
    fn a_raid_counts_down_then_spawns_a_wave() {
        let l = level(2);
        let mut raid = Raid::new([0, 64, 0], 2, 3);
        raid.absorb_omen(0);
        let mut spawned = None;
        for _ in 0..400 {
            for a in raid.tick(&l, &[], &nobody) {
                if let Action::Spawn { wave, spawns, .. } = a {
                    spawned = Some((wave, spawns));
                }
            }
            if spawned.is_some() {
                break;
            }
        }
        let (wave, spawns) = spawned.expect("a wave");
        assert_eq!(wave, 1);
        assert!(count(&spawns, RaiderType::Pillager) >= 4);
        assert_eq!(raid.bar.progress, 1.0);
        assert!(raid.started);
    }
    #[test]
    fn losing_the_village_after_a_wave_loses_the_raid() {
        let mut l = level(2);
        let mut raid = Raid::new([0, 64, 0], 2, 3);
        raid.groups_spawned = 1;
        l.village = false;
        raid.tick(&l, &[], &nobody);
        assert!(raid.is_loss());
        let mut fresh = Raid::new([0, 64, 0], 2, 3);
        fresh.tick(&l, &[], &nobody);
        assert!(fresh.is_stopped());
    }
    #[test]
    fn peaceful_stops_raids() {
        let mut raid = Raid::new([0, 64, 0], 2, 3);
        raid.tick(&level(0), &[], &nobody);
        assert!(raid.is_stopped());
    }
    /// 13/32 of the way from the origin to a centre 32 blocks east is 13.
    /// A player 32 blocks east of the centre hears it 13 blocks back, at 19.
    /// 80 blocks away is outside the 64 block reach.
    #[test]
    fn the_raid_horn_sits_thirteen_blocks_toward_the_wave() {
        assert_eq!(
            horn_at([0.0, 70.0, 0.0], 32.0, 0.0, false),
            Some([13.0, 70.0, 0.0])
        );
        assert_eq!(
            horn_at([32.0, 70.0, 0.0], 0.0, 0.0, false),
            Some([19.0, 70.0, 0.0])
        );
        assert_eq!(horn_at([80.0, 70.0, 0.0], 0.0, 0.0, false), None);
        assert_eq!(
            horn_at([80.0, 70.0, 0.0], 0.0, 0.0, true),
            Some([67.0, 70.0, 0.0])
        );
    }
    #[test]
    fn bar_progress_follows_java_clamp() {
        let mut raid = Raid::new([0, 64, 0], 2, 3);
        raid.update_bossbar(0.0);
        assert!(raid.bar.progress.is_nan());
        raid.total_health = 48.0;
        raid.update_bossbar(24.0);
        assert_eq!(raid.bar.progress, 0.5);
    }
    #[test]
    fn raids_round_trip_through_nbt() {
        let mut raids = Raids::default();
        let (id, triggered) = raids.create_or_extend([10, 64, 10], 2, Some(1), 5);
        assert!(triggered);
        assert_eq!(id, 2);
        raids.get_mut(id).unwrap().add_hero([1, 2, 3, 4]);
        let tag = raids.to_nbt();
        let back = Raids::from_nbt(&tag, 0);
        assert_eq!(back.next_id, 2);
        let r = back.get(id).unwrap();
        assert_eq!(r.omen_level, 2);
        assert_eq!(r.heroes(), vec![[1, 2, 3, 4]]);
        assert_eq!(back.to_nbt(), tag);
    }
    #[test]
    fn a_second_omen_extends_the_nearby_raid() {
        let mut raids = Raids::default();
        let (a, _) = raids.create_or_extend([0, 64, 0], 2, Some(0), 1);
        let (b, triggered) = raids.create_or_extend([20, 64, 0], 2, Some(0), 2);
        assert_eq!(a, b);
        assert!(triggered);
        assert_eq!(raids.get(a).unwrap().omen_level, 2);
    }
    #[test]
    fn fastutil_order_iterates_from_the_top_of_the_table() {
        let mut o = IntOpenHashOrder::new();
        for k in 2..40 {
            o.put(k);
        }
        let it = o.iter();
        assert_eq!(it.len(), 38);
        let mut sorted = it.clone();
        sorted.sort();
        assert_eq!(sorted, (2..40).collect::<Vec<_>>());
        let mut seen = Vec::new();
        let removed = o.iterate_removing(&mut |k| {
            seen.push(k);
            k % 3 == 0
        });
        assert_eq!(seen.len(), 38);
        assert_eq!(o.iter().len(), 38 - removed.len());
    }
    #[test]
    fn the_centre_is_the_mean_of_nearby_points() {
        assert_eq!(raid_center([0, 64, 0], &[]), [0, 64, 0]);
        assert_eq!(
            raid_center([0, 64, 0], &[[10, 64, 10], [11, 65, 10], [200, 64, 0]]),
            [10, 64, 10]
        );
    }
}
