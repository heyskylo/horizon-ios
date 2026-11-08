//! Hosting the open world on the local network ("Open to LAN"): starts the
//! server threads and the play-state hub, feeds it the world's entities each
//! tick and turns what the guests do into gameplay. A world that is not open
//! costs one `Option` check per tick.
mod event_timings;

use super::{Game, shield::OFFHAND_SLOT};
use event_timings::EventTimings;
use horizon_client::{
    Result,
    lan::{
        self, Lan, NetConfig,
        chunks::ChunkEncoder,
        data::{BIOME_REGISTRY, FrozenDamage, SyncData},
        entity::{EntityState, States},
        hub::{HostEvent, Hub, HubConfig, TickCtx},
        playerdata,
    },
    mobs::{Mob, Species, Who, leash::Holder},
    text::Text,
    world::World,
};
use horizon_protocol::{
    entity_data::{Value, serializer},
    packets::play::{self, Clientbound as C},
};
use horizon_world::block_collision::BlockCollisionRegistry;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};

/// What `sync_data` loaded: whether it was with data packs over the game's
/// data, and the registries and tags.
type Synced = (bool, std::result::Result<Arc<SyncData>, String>);

/// Only damage is bound eagerly for gameplay. Full wire data remains lazy
/// except when the entering world actually defines damage types.
#[derive(Default)]
struct SyncCache {
    data: Option<Synced>,
    damage: Option<FrozenDamage>,
    world_packs: bool,
}

static SYNC: Mutex<SyncCache> = Mutex::new(SyncCache {
    data: None,
    damage: None,
    world_packs: false,
});

/// Load wire data once, retaining this session's frozen damage definitions.
pub(super) fn sync_data(
    root: &std::path::Path,
    overlay: &horizon_world::loot::PackOverlay,
) -> std::result::Result<Arc<SyncData>, String> {
    let mut sync = SYNC.lock().unwrap_or_else(|e| e.into_inner());
    cached_sync_data(&mut sync, root, overlay)
}

fn cached_sync_data(
    sync: &mut SyncCache,
    root: &std::path::Path,
    overlay: &horizon_world::loot::PackOverlay,
) -> std::result::Result<Arc<SyncData>, String> {
    let packs = !overlay.files.is_empty();
    let load = || {
        match &sync.damage {
            Some(damage) => SyncData::load_with_frozen_damage(root, overlay, damage),
            None => SyncData::load(root, overlay),
        }
        .map(Arc::new)
    };
    // A helper started before world entry must not fill a pack world's
    // still-lazy cache with its vanilla overlay.
    if sync.data.is_none() && sync.world_packs && !packs {
        return load();
    }
    sync.data.get_or_insert_with(|| (packs, load())).1.clone()
}

/// Reuse a no-pack preload; unrelated packs only invalidate it, as on main.
/// Packs defining damage types seed the wire snapshot under the same lock
/// that helpers use, before reloads or guests can observe a partial state.
pub(super) fn open_world_sync_data(
    root: &std::path::Path,
    overlay: &horizon_world::loot::PackOverlay,
    damage: &horizon_world::damage_source::Catalog,
) -> std::result::Result<(), String> {
    let mut sync = SYNC.lock().unwrap_or_else(|e| e.into_inner());
    initialize_world_sync_data(&mut sync, root, overlay, damage)
}

fn initialize_world_sync_data(
    sync: &mut SyncCache,
    root: &std::path::Path,
    overlay: &horizon_world::loot::PackOverlay,
    damage: &horizon_world::damage_source::Catalog,
) -> std::result::Result<(), String> {
    sync.world_packs = !overlay.files.is_empty();
    if sync.world_packs {
        sync.data = None;
    }
    sync.damage = Some(FrozenDamage::new(damage, overlay));
    if overlay.kind("damage_type").next().is_some() {
        cached_sync_data(sync, root, overlay)?;
    }
    Ok(())
}

/// Let go of what `sync_data` loaded: a preload no world used
/// (game/world_loading.rs), or the previous session when another world opens.
pub(super) fn forget_sync_data() {
    *SYNC.lock().unwrap_or_else(|e| e.into_inner()) = SyncCache::default();
}

/// Without a hub yet, retain this world's frozen registries through `/reload`
/// too. World entry discards this snapshot before loading another world's.
pub(super) fn reload_cached_sync_data(
    root: &std::path::Path,
    overlay: &horizon_world::loot::PackOverlay,
) -> std::result::Result<(), String> {
    let kept = prepare_sync_reload(&mut SYNC.lock().unwrap_or_else(|e| e.into_inner()), overlay);
    if let Some(kept) = kept {
        reload_sync_data(root, overlay, &kept)?;
    }
    Ok(())
}

/// Keep lazy-cache admission aligned with the current packs even when no
/// complete wire snapshot has been loaded yet. Damage definitions stay frozen.
fn prepare_sync_reload(
    sync: &mut SyncCache,
    overlay: &horizon_world::loot::PackOverlay,
) -> Option<Arc<SyncData>> {
    sync.world_packs = !overlay.files.is_empty();
    sync.data
        .as_ref()
        .and_then(|(_, result)| result.as_ref().ok())
        .cloned()
}

/// `/reload` (MinecraftServer.reloadResources) under a running hub: the
/// tags, recipes and advancements read again with the world's packs, the
/// synchronized registries `kept` (the hub's) as they were, and that becomes
/// what `sync_data` gives.
pub(super) fn reload_sync_data(
    root: &std::path::Path,
    overlay: &horizon_world::loot::PackOverlay,
    kept: &SyncData,
) -> std::result::Result<Arc<SyncData>, String> {
    // Kept pack dialogs count as packs: a world opened without packs after
    // a `/reload` that dropped them must read its own registries again.
    let packs = !overlay.files.is_empty() || kept.has_pack_entries();
    let sync = SyncData::reload(root, overlay, kept).map(|mut sync| {
        sync.features.clone_from(&kept.features);
        Arc::new(sync)
    });
    SYNC.lock().unwrap_or_else(|e| e.into_inner()).data = Some((packs, sync.clone()));
    sync
}

impl Game<'_> {
    /// The commands' `minecraft:dialog` ids as the clients get them in the
    /// configuration phase (`sync`, the world's packs' dialogs among them):
    /// `/dialog show` parses an id against these and sends its index.
    pub(super) fn sync_dialog_ids(&mut self, sync: &SyncData) {
        let Some(names) = sync.dynamic.registries.get("minecraft:dialog") else {
            return;
        };
        let ids = names.iter().map(|(name, _)| name.clone()).collect();
        self.ui
            .registries
            .by_registry
            .entry("minecraft:dialog".to_owned())
            .or_default()
            .ids = ids;
    }
}

/// The registries and tags a server sends without data packs over them, read
/// on a helper thread while a server's world prepares: most worlds have no
/// packs, so it is what `sync_data` would load once the world is open.
pub(super) struct SyncPreload(std::thread::JoinHandle<std::result::Result<SyncData, String>>);

/// Start reading the server's data beside the world's preparation; `None`
/// when it is read already, or being read (`preload_world_data`).
pub(super) fn preload_sync_data(root: &std::path::Path) -> Option<SyncPreload> {
    if !SYNC
        .try_lock()
        .is_ok_and(|sync| sync.data.is_none() && sync.damage.is_none())
    {
        return None;
    }
    let root = root.to_owned();
    std::thread::Builder::new()
        .name("Server data preload".into())
        .spawn(move || SyncData::load(&root, &horizon_world::loot::PackOverlay::NONE))
        .map(SyncPreload)
        .ok()
}

impl SyncPreload {
    /// The world is open: without packs of its own (`overlay`), what was read
    /// becomes `sync_data`'s; with them, `sync_data` loads them as before.
    pub(super) fn adopt(self, overlay: &horizon_world::loot::PackOverlay) {
        if !overlay.files.is_empty() {
            return;
        }
        if let Ok(read) = self.0.join() {
            let mut sync = SYNC.lock().unwrap_or_else(|e| e.into_inner());
            if !sync.world_packs && sync.data.is_none() {
                sync.data = Some((false, read.map(Arc::new)));
            }
        }
    }
}

/// The settings of the open world (WorldOptionsScreen's multiplayer section).
#[derive(Clone, Copy, Debug)]
pub struct LanSettings {
    pub port: u16,
    pub guest_commands: bool,
    pub force_game_mode: bool,
}

pub(super) struct LanHost {
    pub hub: Hub,
    /// The mods heard that everyone still here leaves with the server.
    pub players_told_left: bool,
    pub settings: LanSettings,
    pub play: HashMap<u32, super::lan_actions::GuestPlay>,
    /// Blocks broken this tick (their sound is a level event for the guests).
    pub broken: Vec<[f64; 3]>,
    /// Hurt time, death and swing time (while swinging) of the mobs guests
    /// see; the players' hurt time by entity id.
    mob_state: HashMap<super::Dim, HashMap<u32, (i32, bool, Option<i32>)>>,
    /// Each placed guest's pose as the mods last heard it
    /// (`native_entity_pose_change`), by connection.
    pub poses: HashMap<u32, horizon_client::pose::Pose>,
    /// The guest each mob sees as its player, when not the host (game/lan_mobs.rs).
    /// Looked up per mob each tick: a fast hasher (never iterated).
    pub mob_players: horizon_client::fast_hash::FxHashMap<u32, u32>,
    /// An integrated server's owner (game/integrated.rs).
    pub owner: Option<Owner>,
    /// The players' locator bars, one waypoint manager per level by
    /// dimension id (game/lan_waypoints.rs), and the guests who are dead,
    /// whose respawn replaces their waypoint.
    pub waypoints: std::collections::HashMap<String, lan::waypoints::Waypoints>,
    pub waypoint_dead: std::collections::HashSet<u128>,
    /// The world border as the guests were last told of it (lan/border.rs).
    pub border_sent: Option<lan::border::Sent>,
    /// Guests' commands waiting for the hub to be back in `self.lan`, where
    /// a command can reach every player (game/lan_actions.rs).
    pub commands: Vec<super::lan_actions::GuestCommand>,
    /// Mods' quiet commands waiting to run a few a tick (game/lan_quiet.rs).
    pub quiet: std::collections::VecDeque<super::lan_quiet::QuietBatch>,
    /// Far terrain sampled for Horizon clients (game/lan_far.rs); None when
    /// `horizon-lod-distance` is 0.
    pub far: Option<horizon_client::lod_net::Service>,
    /// Native mods' commands and the level each needs, once asked
    /// (game/native_server.rs).
    pub native_commands: Option<HashMap<String, u8>>,
    /// The permission node of each native mod's command that has one
    /// (`NativeCommand::permission`), in registration order.
    pub native_command_nodes: Vec<(String, String)>,
    /// The usages behind them, as the mods registered them.
    pub native_usages: Vec<horizon_client::mod_api::NativeCommand>,
    /// The players a native mod leaves out of the sleep count when they
    /// aren't in bed (`native_sleep_ignored`).
    pub sleep_ignored: std::collections::HashSet<u128>,
    /// The entities the guests track, described again every tick
    /// (`lan_entities`) over last tick's states.
    pub(super) states: lan::entity::States,
    /// The entity id counter when this tick's states were taken for guests
    /// (`lan_note_joined`), None when no guest tracked them.
    pub(super) states_next_entity: Option<u32>,
    /// Last tick's states of the idle mobs and items (game/lan_idle.rs).
    pub(super) idle_states: super::lan_idle::IdleStates,
}

impl Game<'_> {
    /// `PlayerList.remove`: `player.awardStat(Stats.LEAVE_GAME)` before the
    /// player is saved, heard by native mods first. Once per guest, however
    /// often they are saved.
    pub(super) fn award_leave_game(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        player: u128,
        name: &str,
    ) {
        let Some(play) = host.play.get_mut(&conn) else {
            return;
        };
        if std::mem::replace(&mut play.left_game, true) {
            return;
        }
        let mut cancel = false;
        self.native_player_stat_award(
            player,
            name,
            "minecraft:custom",
            "minecraft:leave_game",
            1,
            &mut cancel,
        );
        if !cancel && let Some(play) = host.play.get_mut(&conn) {
            play.stats.custom("leave_game", 1);
        }
    }

    /// `PlayerList.removeAll`: every player who joined leaves with the world.
    pub(super) fn award_leave_game_to_all(&mut self, host: &mut LanHost) {
        let players: Vec<(u32, u128, String)> = host
            .hub
            .guests
            .iter()
            .filter(|g| g.placed)
            .map(|g| (g.conn, g.uuid, g.name.clone()))
            .collect();
        for (conn, player, name) in players {
            self.award_leave_game(host, conn, player, &name);
        }
    }
}

/// The player who owns an integrated server's world: a guest whose state is
/// the world's own player (`players/data/<singleplayer_uuid>.dat`), as in
/// single player.
pub(super) struct Owner {
    pub uuid: u128,
    /// The world's player as loaded, until the owner joins.
    pub saved: Option<Box<horizon_client::save::Level>>,
}

/// A stable UUID for an entity that has none.
pub(super) fn entity_uuid(id: u32) -> u128 {
    0x4856_5a4e_0000_0000_0000_0000_0000_0000u128 | u128::from(id)
}

/// The SynchedEntityData index of the baby flag: AgeableMob.DATA_BABY_ID and
/// Zombie.DATA_BABY_ID (Zoglin's too) are the first entry after Mob's (16);
/// Piglin's comes after AbstractPiglin's zombification flag (17). Every
/// AgeableMob has one, the happy ghast and the nautilus included (#6133).
pub(super) fn baby_index(species: Species) -> Option<u8> {
    use Species as S;
    match species {
        S::Piglin => Some(17),
        s if horizon_client::mobs::finalize::ageable(s)
            || horizon_client::mobs::doors::is_zombie(s)
            || s == S::Zoglin =>
        {
            Some(16)
        }
        _ => None,
    }
}

/// The variant and look entries of `mob_meta`'s mobs (registry holders, the
/// fish's, the bee's, the copper golem's weathering...), in a match of their
/// own: `mob_meta`'s arms hold these mobs' other entries.
fn mob_variant_meta(
    m: &Mob,
    hub: &Hub,
    meta: &mut Vec<(u8, i32, Value)>,
    defaults: &mut Vec<(u8, i32, Value)>,
) {
    let (kind, e) = (m.kind.id.as_str(), &m.extra);
    let id = |registry: &str, name: &str| hub.registry_id(registry, &format!("minecraft:{name}"));
    match kind {
        // Axolotl.DATA_VARIANT (after AgeableMob's two).
        "minecraft:axolotl" => {
            if e.variant != 0 {
                meta.push((18, serializer::INT, Value::Int(i32::from(e.variant))));
            }
        }
        // Frog.DATA_VARIANT_ID, Pig's, Cow's, Chicken's and ZombieNautilus's
        // (after the entries of their parents): registry holders.
        "minecraft:frog"
        | "minecraft:pig"
        | "minecraft:cow"
        | "minecraft:chicken"
        | "minecraft:zombie_nautilus" => {
            let (index, serializer, registry) = match kind {
                "minecraft:frog" => (18, serializer::FROG_VARIANT, "minecraft:frog_variant"),
                "minecraft:pig" => (19, serializer::PIG_VARIANT, "minecraft:pig_variant"),
                "minecraft:cow" => (18, serializer::COW_VARIANT, "minecraft:cow_variant"),
                "minecraft:chicken" => {
                    (18, serializer::CHICKEN_VARIANT, "minecraft:chicken_variant")
                }
                _ => (
                    21,
                    serializer::ZOMBIE_NAUTILUS_VARIANT,
                    "minecraft:zombie_nautilus_variant",
                ),
            };
            // Each registry's default (`define(DATA_VARIANT_ID, ... TEMPERATE)`)
            // stays out of the pairing data, as other default values do.
            let name = horizon_client::mobs::finalize::variant_name(m.species(), e.variant);
            let name = name.trim_start_matches("minecraft:");
            if let Some(v) = id(registry, name)
                && name != "temperate"
            {
                meta.push((index, serializer, Value::Id(v)));
            }
            if let Some(v) = id(registry, "temperate") {
                defaults.push((index, serializer, Value::Id(v)));
            }
        }
        // Rabbit.DATA_TYPE_ID (after AgeableMob's two) and
        // Parrot.DATA_VARIANT_ID (after TamableAnimal's two and its flags and
        // owner). The fox's type goes with its flags (#2538).
        "minecraft:rabbit" | "minecraft:parrot" => {
            let index = if kind == "minecraft:parrot" { 20 } else { 18 };
            if e.variant != 0 {
                meta.push((index, serializer::INT, Value::Int(i32::from(e.variant))));
            }
        }
        // Bee.DATA_FLAGS_ID (rolling 2, stung 4, nectar 8) and
        // DATA_ANGER_END_TIME: the texture shows the nectar and the anger.
        "minecraft:bee" => {
            let bee = horizon_client::mobs::bee::get(m);
            let flags = bee.map_or(0, |b| b.flags);
            if flags != 0 {
                meta.push((18, serializer::BYTE, Value::Byte(flags as i8)));
            }
            defaults.push((18, serializer::BYTE, Value::Byte(0)));
            let anger_end = bee.map_or(-1, |b| b.anger_end);
            if anger_end != -1 {
                meta.push((19, serializer::LONG, Value::Long(anger_end)));
            }
            defaults.push((19, serializer::LONG, Value::Long(-1)));
        }
        // AbstractFish.FROM_BUCKET takes 16: Salmon.DATA_TYPE (small 0,
        // medium 1, large 2; medium is the data's default, so always sent),
        // Pufferfish.PUFF_STATE and TropicalFish.DATA_ID_TYPE_VARIANT: the
        // pattern's packed id (size, then index << 8), base colour << 16 and
        // pattern colour << 24.
        "minecraft:salmon" => {
            meta.push((17, serializer::INT, Value::Int(i32::from(e.variant.min(2)))));
        }
        "minecraft:pufferfish" => {
            if e.size > 0 {
                meta.push((17, serializer::INT, Value::Int(e.size)));
            }
            defaults.push((17, serializer::INT, Value::Int(0)));
        }
        "minecraft:tropical_fish" => {
            let pattern = e.variant.min(11);
            let packed = i32::from(pattern >= 6)
                | i32::from(pattern % 6) << 8
                | i32::from(e.variant2 & 15) << 16
                | i32::from(e.variant3 & 15) << 24;
            if packed != 0 {
                meta.push((17, serializer::INT, Value::Int(packed)));
            }
        }
        // Tadpole.AGE_LOCKED (after AbstractFish's FROM_BUCKET).
        "minecraft:tadpole" => {
            if e.age_locked {
                meta.push((17, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((17, serializer::BOOLEAN, Value::Bool(false)));
        }
        // CopperGolem.DATA_WEATHER_STATE: unaffected 0 to oxidized 3, and it
        // goes back down when the golem is scraped.
        "minecraft:copper_golem" => {
            if e.variant != 0 {
                meta.push((
                    16,
                    serializer::WEATHERING_COPPER_STATE,
                    Value::Id(i32::from(e.variant.min(3))),
                ));
            }
            defaults.push((16, serializer::WEATHERING_COPPER_STATE, Value::Id(0)));
        }
        // Ghast.DATA_IS_CHARGING: the shooting texture.
        "minecraft:ghast" => {
            if e.flag {
                meta.push((16, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((16, serializer::BOOLEAN, Value::Bool(false)));
        }
        // WitherBoss.DATA_ID_INV (after the three head targets): the
        // invulnerable ticks, the blinking texture and the growing scale.
        "minecraft:wither" => {
            if e.counter > 0 {
                meta.push((19, serializer::INT, Value::Int(e.counter)));
            }
            defaults.push((19, serializer::INT, Value::Int(0)));
        }
        _ => {}
    }
}

/// A mob's synched data as ServerEntity sends it (only the values that differ
/// from the defaults the client constructs), with vanilla's entry indices:
/// Entity 0..=7, LivingEntity 8..=14, Mob 15, then the subclasses.
/// `defaults` gets the defaults of the entries that can go back to theirs.
pub(super) fn mob_meta(
    m: &Mob,
    hub: &Hub,
    player_entity: Option<i32>,
    game_time: i64,
    meta: &mut Vec<(u8, i32, Value)>,
    defaults: &mut Vec<(u8, i32, Value)>,
) {
    // The entries are the class's: a content mod's living type has its
    // template's.
    let kind = m.kind.class_id();
    // Entity.DATA_SHARED_FLAGS_ID: on fire 0x01, sprinting 0x08 (a dashing
    // cat or ocelot), swimming 0x10 (a drowned under water,
    // `Drowned.setSwimming`), invisible 0x20 (the effect's flag) and glowing
    // 0x40 (LivingEntity.updateGlowingStatus).
    let shared = i8::from(m.is_on_fire())
        | i8::from(m.extra.sprinting) << 3
        | i8::from(m.species() == Species::Drowned && m.extra.flag) << 4
        | i8::from(m.invisible || horizon_client::mobs::trader::is_invisible(m)) << 5
        | i8::from(m.is_currently_glowing()) << 6;
    if shared != 0 {
        meta.push((0, serializer::BYTE, Value::Byte(shared)));
    }
    // Entity.DATA_POSE: the pose the species keeps in its own fields (a
    // pose back at standing goes out by the entity's own default).
    let pose = horizon_client::mobs::sync::pose_id(m);
    if pose != 0 {
        meta.push((6, serializer::POSE, Value::Id(i32::from(pose))));
    }
    // LivingEntity.DATA_SLEEPING_POS_ID: the bed a sleeping villager lies in.
    if let Some(bed) = horizon_client::mobs::villager::sleeping_pos(m) {
        meta.push((14, serializer::OPTIONAL_BLOCK_POS, Value::OptPos(Some(bed))));
    }
    if let Some(name) = &m.custom_name {
        meta.push((
            2,
            serializer::OPTIONAL_COMPONENT,
            Value::OptText(Some(horizon_protocol::text::literal(name))),
        ));
        if m.name_visible {
            meta.push((3, serializer::BOOLEAN, Value::Bool(true)));
        }
    }
    // LivingEntity.DATA_LIVING_ENTITY_FLAGS: using an item 1, in the off
    // hand 2 (a skeleton draws its bow, a witch raises her potion).
    let using = i8::from(m.extra.using_item > 0) | i8::from(m.extra.using_off_hand) << 1;
    if using != 0 {
        meta.push((8, serializer::BYTE, Value::Byte(using)));
    }
    defaults.push((8, serializer::BYTE, Value::Byte(0)));
    // LivingEntity.DATA_HEALTH_ID: defined as 1, then set to the max health.
    let health = m.health.max(0.0);
    if health != 1.0 {
        meta.push((9, serializer::FLOAT, Value::Float(health)));
    }
    defaults.push((9, serializer::FLOAT, Value::Float(1.0)));
    // LivingEntity.DATA_EFFECT_PARTICLES and DATA_EFFECT_AMBIENCE_ID: the
    // swirls of its effects (the ones a mob has are visible, not ambient).
    meta.extend(
        hub.effect_meta(
            (m.extra.effects.iter())
                .filter(|e| !m.extra.hidden_effects.contains(&e.0))
                .map(|e| (e.0, m.extra.ambient_effects.contains(&e.0))),
        ),
    );
    defaults.push((10, serializer::PARTICLES, Value::Particles(Vec::new())));
    defaults.push((11, serializer::BOOLEAN, Value::Bool(false)));
    // Mob.DATA_MOB_FLAGS_ID: no AI 1, left-handed 2, aggressive 4 (what
    // draws a vindicator's axe, a skeleton's drawn bow, a piglin's raised
    // weapon).
    let flags =
        i8::from(m.no_ai) | i8::from(m.extra.left_handed) << 1 | i8::from(m.ai.aggressive) << 2;
    if flags != 0 {
        meta.push((15, serializer::BYTE, Value::Byte(flags)));
    }
    defaults.push((15, serializer::BYTE, Value::Byte(0)));
    // AgeableMob.DATA_BABY_ID (and Zombie's, Zoglin's, Piglin's).
    if let Some(index) = baby_index(m.species()) {
        if m.is_baby() {
            meta.push((index, serializer::BOOLEAN, Value::Bool(true)));
        }
        defaults.push((index, serializer::BOOLEAN, Value::Bool(false)));
        // AgeableMob.AGE_LOCKED, the entry after it (not Zombie's or Piglin's).
        if index == 16 && horizon_client::mobs::finalize::ageable(m.species()) {
            if m.extra.age_locked {
                meta.push((17, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((17, serializer::BOOLEAN, Value::Bool(false)));
        }
    }
    let e = &m.extra;
    let id = |registry: &str, name: &str| hub.registry_id(registry, &format!("minecraft:{name}"));
    // Pig.DATA_BOOST_TIME and Strider.DATA_BOOST_TIME (18, the first entry
    // after AgeableMob's two). The total `boost()` stored in `counter2`.
    // The default 0 is not registered: vanilla never writes it back, and the
    // rider's client finishes the timer `onSynced` started.
    if matches!(kind, "minecraft:pig" | "minecraft:strider") && e.counter2 > 0 {
        meta.push((18, serializer::INT, Value::Int(e.counter2)));
    }
    mob_variant_meta(m, hub, meta, defaults);
    mob_raider_meta(m, meta, defaults);
    match kind {
        "minecraft:guardian" | "minecraft:elder_guardian" => {
            // Guardian.DATA_ID_MOVING and DATA_ID_ATTACK_TARGET.
            if e.flag2 {
                meta.push((16, serializer::BOOLEAN, Value::Bool(true)));
            }
            let target = if e.flag {
                m.ai.target.and_then(|who| match who {
                    Who::Player => player_entity,
                    Who::Mob(id) => Some(id as i32),
                })
            } else {
                None
            };
            if let Some(target) = target {
                meta.push((17, serializer::INT, Value::Int(target)));
            }
            defaults.push((16, serializer::BOOLEAN, Value::Bool(false)));
            defaults.push((17, serializer::INT, Value::Int(0)));
        }
        // Bat.DATA_ID_FLAGS: resting in bit 0, flying by default.
        "minecraft:bat" => {
            if e.flag {
                meta.push((16, serializer::BYTE, Value::Byte(1)));
            }
            defaults.push((16, serializer::BYTE, Value::Byte(0)));
        }
        // SnowGolem.DATA_PUMPKIN_ID: 16 while the pumpkin is on (the default).
        "minecraft:snow_golem" => {
            if e.sheared {
                meta.push((16, serializer::BYTE, Value::Byte(0)));
            }
            defaults.push((16, serializer::BYTE, Value::Byte(16)));
        }
        // MushroomCow.DATA_TYPE (after AgeableMob's two): 0 red, 1 brown.
        "minecraft:mooshroom" => {
            if e.variant == 1 {
                meta.push((18, serializer::INT, Value::Int(1)));
            }
            defaults.push((18, serializer::INT, Value::Int(0)));
        }
        // AbstractPiglin.DATA_IMMUNE_TO_ZOMBIFICATION (16: with the dimension it
        // makes it shake) and Piglin.DATA_IS_DANCING (after the baby flag and
        // the crossbow charge).
        "minecraft:piglin" => {
            if e.flag2 {
                meta.push((16, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((16, serializer::BOOLEAN, Value::Bool(false)));
            if e.scratch[0] > 0 {
                meta.push((19, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((19, serializer::BOOLEAN, Value::Bool(false)));
        }
        // Sheep.DATA_WOOL_ID: the colour in the low nibble, 16 when sheared.
        "minecraft:sheep" => {
            let value = (e.color & 15) | if e.sheared { 16 } else { 0 };
            if value != 0 {
                meta.push((18, serializer::BYTE, Value::Byte(value as i8)));
            }
            defaults.push((18, serializer::BYTE, Value::Byte(0)));
        }
        // Fox.DATA_TYPE_ID and DATA_FLAGS_ID (after AgeableMob's two; sleeping 32, ...).
        "minecraft:fox" => {
            if e.variant != 0 {
                meta.push((18, serializer::INT, Value::Int(i32::from(e.variant))));
            }
            defaults.push((18, serializer::INT, Value::Int(0)));
            let flags = (e.ints[0] & 0xff) as u8 as i8;
            if flags != 0 {
                meta.push((19, serializer::BYTE, Value::Byte(flags)));
            }
            defaults.push((19, serializer::BYTE, Value::Byte(0)));
        }
        "minecraft:parrot" => {
            // TamableAnimal.DATA_FLAGS_ID: sitting 1, tame 4.
            let flags = i8::from(e.sitting) | if e.tame { 4 } else { 0 };
            if flags != 0 {
                meta.push((18, serializer::BYTE, Value::Byte(flags)));
            }
        }
        "minecraft:cat" | "minecraft:wolf" => {
            // TamableAnimal.DATA_FLAGS_ID: sitting 1, tame 4.
            let flags = i8::from(e.sitting) | if e.tame { 4 } else { 0 };
            if flags != 0 {
                meta.push((18, serializer::BYTE, Value::Byte(flags)));
            }
            let (index, registry, name) = if kind == "minecraft:cat" {
                let v = horizon_client::mobs::tame::CAT_VARIANTS[(e.variant as usize).min(10)];
                (20, "minecraft:cat_variant", v)
            } else {
                let v = horizon_client::mobs::tame::WOLF_VARIANTS[(e.variant as usize).min(8)].0;
                (23, "minecraft:wolf_variant", v)
            };
            let serializer = if index == 20 {
                serializer::CAT_VARIANT
            } else {
                serializer::WOLF_VARIANT
            };
            if let Some(v) = id(registry, name) {
                meta.push((index, serializer, Value::Id(v)));
            }
            // Cat.IS_LYING and RELAX_STATE_ONE.
            if kind == "minecraft:cat" {
                for (index, on) in [(21, e.flag), (22, e.relax_one)] {
                    if on {
                        meta.push((index, serializer::BOOLEAN, Value::Bool(true)));
                    }
                    defaults.push((index, serializer::BOOLEAN, Value::Bool(false)));
                }
            }
            // Wolf.DATA_COLLAR_COLOR and Cat's, red by default.
            let collar = if kind == "minecraft:cat" { 23 } else { 21 };
            let red = i32::from(horizon_client::mobs::tame::RED);
            if i32::from(e.color & 15) != red {
                meta.push((collar, serializer::INT, Value::Int(i32::from(e.color & 15))));
            }
            defaults.push((collar, serializer::INT, Value::Int(red)));
            // Wolf.DATA_ANGER_END_TIME: the game time the anger ends, -1 when calm.
            if kind == "minecraft:wolf" {
                // Wolf.DATA_INTERESTED_ID: BegGoal's flag, the head tilt.
                if e.flag {
                    meta.push((20, serializer::BOOLEAN, Value::Bool(true)));
                }
                defaults.push((20, serializer::BOOLEAN, Value::Bool(false)));
                if e.anger > 0 {
                    let end = game_time + i64::from(e.anger);
                    meta.push((22, serializer::LONG, Value::Long(end)));
                }
                defaults.push((22, serializer::LONG, Value::Long(-1)));
            }
        }
        "minecraft:horse"
        | "minecraft:donkey"
        | "minecraft:mule"
        | "minecraft:llama"
        | "minecraft:trader_llama"
        | "minecraft:skeleton_horse"
        | "minecraft:zombie_horse"
        | "minecraft:camel" => {
            // AbstractHorse.DATA_ID_FLAGS: tame 2, eating 16 (grazing), standing
            // 32 (rearing).
            let grazes = horizon_client::mobs::equine::grazes(m.species());
            let flags = if e.tame { 2 } else { 0 }
                | if grazes && e.flag { 16 } else { 0 }
                | if grazes && e.counter > 0 { 32 } else { 0 };
            if flags != 0 {
                meta.push((18, serializer::BYTE, Value::Byte(flags)));
            }
            defaults.push((18, serializer::BYTE, Value::Byte(0)));
            if kind == "minecraft:horse" {
                let v = i32::from(e.variant) | i32::from(e.variant2) << 8;
                if v != 0 {
                    meta.push((19, serializer::INT, Value::Int(v)));
                }
            } else if kind.ends_with("llama") {
                if e.chested {
                    meta.push((19, serializer::BOOLEAN, Value::Bool(true)));
                }
                meta.push((
                    20,
                    serializer::INT,
                    Value::Int(i32::from(e.variant2.max(1))),
                ));
                if e.variant != 0 {
                    meta.push((21, serializer::INT, Value::Int(i32::from(e.variant))));
                }
            } else if (kind == "minecraft:donkey" || kind == "minecraft:mule") && e.chested {
                meta.push((19, serializer::BOOLEAN, Value::Bool(true)));
            }
        }
        // HappyGhast.STAYS_STILL (after AgeableMob's two and IS_LEASH_HOLDER),
        // set while `serverStillTimeout > 0` (`syncStayStillFlag`).
        "minecraft:happy_ghast" => {
            // HappyGhast.IS_LEASH_HOLDER (`setLeashHolder` in `tick`).
            if horizon_client::mobs::happy_ghast::is_leash_holder(m) {
                meta.push((18, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((18, serializer::BOOLEAN, Value::Bool(false)));
            if horizon_client::mobs::happy_ghast::on_still_timeout(m) {
                meta.push((19, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((19, serializer::BOOLEAN, Value::Bool(false)));
        }
        // Villager.DATA_VILLAGER_DATA and ZombieVillager's (after Zombie's three).
        "minecraft:villager" | "minecraft:zombie_villager" => {
            use horizon_client::mobs::render::{PROFESSIONS, VILLAGER_TYPES};
            if kind == "minecraft:villager" {
                unhappy_counter(m, meta, defaults);
            } else {
                // ZombieVillager.DATA_CONVERTING_ID: the cure's shaking.
                if horizon_client::mobs::zombie_villager::is_converting(m) {
                    meta.push((19, serializer::BOOLEAN, Value::Bool(true)));
                }
                defaults.push((19, serializer::BOOLEAN, Value::Bool(false)));
            }
            let villager_type = VILLAGER_TYPES[e.variant as usize % VILLAGER_TYPES.len()];
            let profession = PROFESSIONS[e.variant2 as usize % PROFESSIONS.len()];
            if let (Some(t), Some(p)) = (
                id("minecraft:villager_type", villager_type),
                id("minecraft:villager_profession", profession),
            ) {
                let index = if kind == "minecraft:villager" { 19 } else { 20 };
                let level = i32::from(e.variant3.max(1));
                meta.push((
                    index,
                    serializer::VILLAGER_DATA,
                    Value::Ids(vec![t, p, level]),
                ));
            }
        }
        // Phantom.ID_SIZE (after Mob's flags).
        "minecraft:phantom" => {
            if e.size > 0 {
                meta.push((16, serializer::INT, Value::Int(e.size)));
            }
            defaults.push((16, serializer::INT, Value::Int(0)));
        }
        // AbstractCubeMob.ID_SIZE; SulfurCube also syncs FROM_BUCKET and
        // MAX_FUSE (clients tick the fuse for the swell / white flash).
        "minecraft:slime" | "minecraft:magma_cube" | "minecraft:sulfur_cube" => {
            if e.size > 1 {
                meta.push((18, serializer::INT, Value::Int(e.size)));
            }
            defaults.push((18, serializer::INT, Value::Int(1)));
            if kind == "minecraft:sulfur_cube" {
                let from_bucket = e.scratch[3] != 0;
                if from_bucket {
                    meta.push((19, serializer::BOOLEAN, Value::Bool(true)));
                }
                defaults.push((19, serializer::BOOLEAN, Value::Bool(false)));
                // SulfurCube.MAX_FUSE: set when primed; 0 means unprimed.
                let fuse = horizon_client::mobs::sulfur_cube::fuse(m);
                if fuse >= 0 {
                    meta.push((20, serializer::INT, Value::Int(fuse)));
                }
                defaults.push((20, serializer::INT, Value::Int(0)));
            }
        }
        // Creeper.DATA_SWELL_DIR, DATA_IS_POWERED and DATA_IS_IGNITED: the
        // guest swells the creeper from them. The direction always goes in,
        // as only changed entries are sent and it drops back to -1.
        "minecraft:creeper" => {
            meta.push((16, serializer::INT, Value::Int(e.counter)));
            if e.charged {
                meta.push((17, serializer::BOOLEAN, Value::Bool(true)));
            }
            if e.flag {
                meta.push((18, serializer::BOOLEAN, Value::Bool(true)));
            }
        }
        // Pillager.IS_CHARGING_CROSSBOW: the charge pose and the held
        // item's loading animation.
        "minecraft:pillager" => {
            if e.flag {
                meta.push((17, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((17, serializer::BOOLEAN, Value::Bool(false)));
        }
        // Panda: UNHAPPY_COUNTER, SNEEZE_COUNTER and EAT_COUNTER (after
        // AgeableMob's two), MAIN_GENE_ID, HIDDEN_GENE_ID and DATA_ID_FLAGS;
        // the client animates the states from them.
        "minecraft:panda" => {
            let (flags, counters) = horizon_client::mobs::panda::get(m)
                .map_or((0, [0; 3]), |s| (s.flags, [s.unhappy, s.sneeze, s.eat]));
            for (index, value) in (18..).zip(counters) {
                if value != 0 {
                    meta.push((index, serializer::INT, Value::Int(value)));
                }
                defaults.push((index, serializer::INT, Value::Int(0)));
            }
            for (index, gene) in [(21, e.variant), (22, e.variant2)] {
                if gene != 0 {
                    meta.push((index, serializer::BYTE, Value::Byte(gene as i8)));
                }
                defaults.push((index, serializer::BYTE, Value::Byte(0)));
            }
            if flags != 0 {
                meta.push((23, serializer::BYTE, Value::Byte(flags as i8)));
            }
            defaults.push((23, serializer::BYTE, Value::Byte(0)));
        }
        // PolarBear.DATA_STANDING_ID (after AgeableMob's two): the bear
        // rears up; the client animates it.
        "minecraft:polar_bear" => {
            if e.flag {
                meta.push((18, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((18, serializer::BOOLEAN, Value::Bool(false)));
        }
        // SpellcasterIllager.DATA_SPELL_CASTING_ID: the casting pose.
        "minecraft:evoker" | "minecraft:illusioner" => {
            if e.variant2 != 0 {
                meta.push((17, serializer::BYTE, Value::Byte(e.variant2 as i8)));
            }
            defaults.push((17, serializer::BYTE, Value::Byte(0)));
        }
        // Warden.CLIENT_ANGER_LEVEL: the heartbeat's pace.
        "minecraft:warden" => {
            let anger = e.warden.as_ref().map_or(0, |w| w.client_anger);
            if anger != 0 {
                meta.push((16, serializer::INT, Value::Int(anger)));
            }
            defaults.push((16, serializer::INT, Value::Int(0)));
        }
        // Sniffer.DATA_STATE (after AgeableMob's two).
        "minecraft:sniffer" => {
            if e.variant != 0 {
                meta.push((
                    18,
                    serializer::SNIFFER_STATE,
                    Value::Id(i32::from(e.variant)),
                ));
            }
            defaults.push((18, serializer::SNIFFER_STATE, Value::Id(0)));
        }
        // Shulker.DATA_ATTACH_FACE_ID, DATA_PEEK_ID and DATA_COLOR_ID (16
        // for none).
        "minecraft:shulker" => {
            if e.variant3 != 0 {
                meta.push((16, serializer::DIRECTION, Value::Id(i32::from(e.variant3))));
            }
            defaults.push((16, serializer::DIRECTION, Value::Id(0)));
            if e.counter != 0 {
                meta.push((17, serializer::BYTE, Value::Byte(e.counter as i8)));
            }
            defaults.push((17, serializer::BYTE, Value::Byte(0)));
            if e.color != 16 {
                meta.push((18, serializer::BYTE, Value::Byte(e.color as i8)));
            }
            defaults.push((18, serializer::BYTE, Value::Byte(16)));
        }
        // Vex.DATA_FLAGS_ID: charging in bit 0 (the arms and the glowing
        // texture).
        "minecraft:vex" => {
            if e.flag {
                meta.push((16, serializer::BYTE, Value::Byte(1)));
            }
            defaults.push((16, serializer::BYTE, Value::Byte(0)));
        }
        // GlowSquid.DATA_DARK_TICKS_REMAINING (after AgeableMob's two): the
        // dimming a hit gave, counted down by the server and the client alike.
        "minecraft:glow_squid" => {
            let ticks = horizon_client::mobs::aquatic::dark_ticks(m);
            if ticks != 0 {
                meta.push((18, serializer::INT, Value::Int(ticks)));
            }
            defaults.push((18, serializer::INT, Value::Int(0)));
        }
        // Ocelot.DATA_TRUSTING (after AgeableMob's two).
        "minecraft:ocelot" => {
            if e.trusting {
                meta.push((18, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((18, serializer::BOOLEAN, Value::Bool(false)));
        }
        // Mannequin.DATA_PROFILE, DATA_IMMOVABLE and DATA_DESCRIPTION (after Avatar's two):
        // `updateDescription` keeps the last as a component, or none when hidden.
        "minecraft:mannequin" => {
            if let Some(profile) = &m.profile {
                meta.push((
                    17,
                    serializer::RESOLVABLE_PROFILE,
                    Value::Profile(profile.clone()),
                ));
            }
            if e.immovable {
                meta.push((18, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((18, serializer::BOOLEAN, Value::Bool(false)));
            let default = horizon_client::mobs::MannequinDescription::Default.component();
            let shown = e.description.component();
            if shown != default {
                meta.push((19, serializer::OPTIONAL_COMPONENT, Value::OptText(shown)));
            }
            defaults.push((19, serializer::OPTIONAL_COMPONENT, Value::OptText(default)));
        }
        // Creaking.CAN_MOVE, IS_ACTIVE and IS_TEARING_DOWN.
        "minecraft:creaking" => {
            if e.flag2 {
                meta.push((16, serializer::BOOLEAN, Value::Bool(false)));
            }
            defaults.push((16, serializer::BOOLEAN, Value::Bool(true)));
            if e.flag {
                meta.push((17, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((17, serializer::BOOLEAN, Value::Bool(false)));
            if horizon_client::mobs::creaking::tearing_down(m) {
                meta.push((18, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((18, serializer::BOOLEAN, Value::Bool(false)));
        }
        // Allay.DATA_DANCING and DATA_CAN_DUPLICATE (the held item is its
        // main hand equipment).
        "minecraft:allay" => {
            if e.flag {
                meta.push((16, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((16, serializer::BOOLEAN, Value::Bool(false)));
            if e.egg_time > 0 {
                meta.push((17, serializer::BOOLEAN, Value::Bool(false)));
            }
            defaults.push((17, serializer::BOOLEAN, Value::Bool(true)));
        }
        // Axolotl.DATA_PLAYING_DEAD (after DATA_VARIANT).
        "minecraft:axolotl" => {
            if horizon_client::mobs::axolotl::is_playing_dead(m) {
                meta.push((19, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((19, serializer::BOOLEAN, Value::Bool(false)));
        }
        // CopperGolem.COPPER_GOLEM_STATE (after DATA_WEATHER_STATE).
        "minecraft:copper_golem" => {
            if e.variant3 != 0 {
                meta.push((
                    17,
                    serializer::COPPER_GOLEM_STATE,
                    Value::Id(i32::from(e.variant3)),
                ));
            }
            defaults.push((17, serializer::COPPER_GOLEM_STATE, Value::Id(0)));
        }
        // Armadillo.ARMADILLO_STATE (after AgeableMob's two).
        "minecraft:armadillo" => {
            let state = horizon_client::mobs::armadillo::state(m);
            if state != 0 {
                meta.push((18, serializer::ARMADILLO_STATE, Value::Id(i32::from(state))));
            }
            defaults.push((18, serializer::ARMADILLO_STATE, Value::Id(0)));
        }
        // Frog.DATA_TONGUE_TARGET_ID (after the variant): the entity id plus
        // one, 0 for none.
        "minecraft:frog" => {
            if let Some(target) = e.frog.tongue_target {
                let id = i32::try_from(target).map_or(0, |t| t + 1);
                meta.push((19, serializer::OPTIONAL_UNSIGNED_INT, Value::Id(id)));
            }
            defaults.push((19, serializer::OPTIONAL_UNSIGNED_INT, Value::Id(0)));
        }
        // Goat.DATA_HAS_LEFT_HORN and DATA_HAS_RIGHT_HORN (after
        // DATA_IS_SCREAMING_GOAT): both true by default.
        "minecraft:goat" => {
            for (index, lost) in [(19, e.lost_horns[0]), (20, e.lost_horns[1])] {
                if lost {
                    meta.push((index, serializer::BOOLEAN, Value::Bool(false)));
                }
                defaults.push((index, serializer::BOOLEAN, Value::Bool(true)));
            }
        }
        // Enderman.DATA_CARRY_STATE and DATA_CREEPY (angry: the open mouth).
        "minecraft:enderman" => {
            if let Some(state) = e.carried {
                meta.push((
                    16,
                    serializer::OPTIONAL_BLOCK_STATE,
                    Value::Id(state.0 as i32),
                ));
            }
            if e.flag {
                meta.push((17, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((17, serializer::BOOLEAN, Value::Bool(false)));
            defaults.push((16, serializer::OPTIONAL_BLOCK_STATE, Value::Id(0)));
        }
        // Zombie.DATA_DROWNED_CONVERSION_ID (after DATA_BABY_ID and
        // DATA_SPECIAL_TYPE_ID): the shaking of the conversion under water.
        "minecraft:zombie" | "minecraft:husk" => {
            if e.counter2 > 0 {
                meta.push((18, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((18, serializer::BOOLEAN, Value::Bool(false)));
        }
        // Skeleton.DATA_STRAY_CONVERSION_ID (the first entry after Mob's):
        // the shaking of the freezing conversion.
        "minecraft:skeleton" => {
            if e.counter2 > 0 {
                meta.push((16, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((16, serializer::BOOLEAN, Value::Bool(false)));
        }
        // Bogged.DATA_SHEARED (the first entry after Mob's): the mushrooms
        // on its head are gone.
        "minecraft:bogged" => {
            if e.sheared {
                meta.push((16, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((16, serializer::BOOLEAN, Value::Bool(false)));
        }
        // AbstractVillager.DATA_UNHAPPY_COUNTER (after AgeableMob's two).
        "minecraft:wandering_trader" => unhappy_counter(m, meta, defaults),
        // Hoglin.DATA_IMMUNE_TO_ZOMBIFICATION (after AgeableMob's two) and
        // AbstractPiglin's, the first entry of its subclasses (the piglin's is
        // in its own arm): with the dimension it makes them shake.
        "minecraft:hoglin" | "minecraft:piglin_brute" => {
            let index = if kind == "minecraft:hoglin" { 18 } else { 16 };
            if e.flag2 {
                meta.push((index, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((index, serializer::BOOLEAN, Value::Bool(false)));
        }
        // Blaze.DATA_FLAGS_ID: charged 1, which makes it burn.
        "minecraft:blaze" => {
            if e.flag {
                meta.push((16, serializer::BYTE, Value::Byte(1)));
            }
            defaults.push((16, serializer::BYTE, Value::Byte(0)));
        }
        // Turtle.HAS_EGG and LAYING_EGG (after AgeableMob's two).
        "minecraft:turtle" => {
            if e.flag {
                meta.push((18, serializer::BOOLEAN, Value::Bool(true)));
            }
            if e.flag2 {
                meta.push((19, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((18, serializer::BOOLEAN, Value::Bool(false)));
            defaults.push((19, serializer::BOOLEAN, Value::Bool(false)));
        }
        // WitherBoss.DATA_TARGET_B and DATA_TARGET_C (after DATA_TARGET_A, the
        // main target): the entities the side heads look at, by id.
        "minecraft:wither" => {
            for (k, index) in [(0, 17), (1, 18)] {
                let target = match e.ints[k] {
                    0 => None,
                    -1 => player_entity,
                    id => Some(id),
                };
                if let Some(target) = target {
                    meta.push((index, serializer::INT, Value::Int(target)));
                }
                defaults.push((index, serializer::INT, Value::Int(0)));
            }
        }
        _ => {}
    }
    // Camel.DASH and LAST_POSE_CHANGE_TICK (after AbstractHorse's flags).
    if horizon_client::mobs::camel::is_camel(m.species()) {
        if e.flag2 {
            meta.push((19, serializer::BOOLEAN, Value::Bool(true)));
        }
        defaults.push((19, serializer::BOOLEAN, Value::Bool(false)));
        let tick = horizon_client::mobs::camel::synced_pose_tick(m, game_time);
        if tick != 0 {
            meta.push((20, serializer::LONG, Value::Long(tick)));
        }
        defaults.push((20, serializer::LONG, Value::Long(0)));
    }
}

/// AbstractVillager.DATA_UNHAPPY_COUNTER (after AgeableMob's two): the head
/// shake of a villager saying no counts it down from 40.
fn unhappy_counter(
    m: &Mob,
    meta: &mut Vec<(u8, i32, Value)>,
    defaults: &mut Vec<(u8, i32, Value)>,
) {
    if m.extra.ints[0] > 0 {
        meta.push((18, serializer::INT, Value::Int(m.extra.ints[0])));
    }
    defaults.push((18, serializer::INT, Value::Int(0)));
}

/// Raider.IS_CELEBRATING: also list false so tracking sends the end of a celebration.
pub(super) fn mob_raider_meta(
    m: &Mob,
    meta: &mut Vec<(u8, i32, Value)>,
    defaults: &mut Vec<(u8, i32, Value)>,
) {
    use horizon_client::mobs::raider;
    if raider::is_raider(m.species()) {
        if raider::state(m).is_some_and(|state| state.celebrating) {
            meta.push((16, serializer::BOOLEAN, Value::Bool(true)));
        }
        defaults.push((16, serializer::BOOLEAN, Value::Bool(false)));
    }
}

/// What a client shakes a mob by: Entity.DATA_TICKS_FROZEN (7), whose full
/// count is `isFullyFrozen`, and Strider.DATA_SUFFOCATING (19, after
/// AgeableMob's two and DATA_BOOST_TIME).
pub(super) fn mob_freeze_meta(
    m: &Mob,
    meta: &mut Vec<(u8, i32, Value)>,
    defaults: &mut Vec<(u8, i32, Value)>,
) {
    // `setTicksFrozen` can go negative (a mob warming up), and a client reads
    // the count as is, so anything but the default 0 goes out.
    if m.ticks_frozen != 0 {
        meta.push((7, serializer::INT, Value::Int(m.ticks_frozen)));
    }
    defaults.push((7, serializer::INT, Value::Int(0)));
    // Every Entity tracks air, including changes back to its species' default.
    let max = m.max_air_supply();
    if m.air != max {
        meta.push((1, serializer::INT, Value::Int(m.air)));
    }
    defaults.push((1, serializer::INT, Value::Int(max)));
    if m.kind.id == "minecraft:strider" {
        if m.extra.flag {
            meta.push((19, serializer::BOOLEAN, Value::Bool(true)));
        }
        defaults.push((19, serializer::BOOLEAN, Value::Bool(false)));
    }
}

/// The equipment a mob shows (ClientboundSetEquipmentPacket slots: main hand,
/// off hand, feet, legs, chest, head, body, saddle).
fn mob_equipment(
    m: &Mob,
    hub: &mut Hub,
    out: &mut Vec<(u8, Option<horizon_protocol::item::Stack>)>,
) {
    for (slot, stack) in m.equipment_stacks().into_iter().enumerate() {
        if stack.is_some() {
            out.push((slot as u8, hub.stack(&stack)));
        }
    }
}

pub const HELP: &[&str] = &[
    "lan open [PORT] [commands] [force]  open the world to the network (PORT 0 picks one)",
    "lan close                      stop hosting",
    "lan status                     the hosting state and the guests",
];

/// With the game mode forced, the guests online take the default mode
/// (setting a mode does nothing when it is already theirs). A world open to
/// LAN is an integrated server, which leaves out the players who have game
/// master permission; a dedicated server does not.
fn enforce_guest_game_mode(
    host: &mut LanHost,
    world: &World,
    collision: &BlockCollisionRegistry<'_>,
) {
    if !host.hub.config.force_game_mode {
        return;
    }
    let mode = host.hub.config.default_game_mode;
    let spares_operators = host.hub.config.integrated;
    let stale: Vec<u32> = host
        .hub
        .guests
        .iter()
        .filter(|g| g.game_mode != mode && !(spares_operators && g.permission >= 2))
        .map(|g| g.conn)
        .collect();
    for conn in stale {
        host.hub.set_game_mode(conn, mode, world, Some(collision));
    }
}

impl Game<'_> {
    /// The level's default point, independently of the acting player's bed or spawnpoint.
    pub(super) fn world_spawn_position(&self) -> [f64; 3] {
        self.level
            .world_spawn
            .map_or([0.5, 80.0, 0.5], |[x, y, z]| {
                [f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5]
            })
    }

    /// MinecraftServer.setRespawnData: every placed player hears a changed world spawn.
    pub(super) fn lan_broadcast_world_spawn(&self) {
        if let Some(host) = &self.lan {
            for guest in host.hub.guests.iter().filter(|guest| guest.placed) {
                guest.send(&host.hub.default_spawn(
                    guest.uuid,
                    self.world_spawn_position(),
                    self.level.world_spawn_angle,
                ));
            }
        }
    }

    /// `horizon-ctl lan ...`.
    pub(super) fn lan_control(&mut self, words: &[&str]) -> Result<Option<serde_json::Value>> {
        use serde_json::json;
        if words[0] != "lan" {
            return Ok(None);
        }
        if let Some(server) = &self.integrated
            && words.get(1) == Some(&"status")
        {
            // The hub is the server thread's.
            let status = server
                .call(|g| g.lan_status())
                .ok_or(super::integrated::NO_ANSWER)?;
            return Ok(Some(status));
        }
        match words.get(1).copied() {
            Some("open") => {
                if self.title.is_some() {
                    return Err("open a world first".into());
                }
                let port = words.get(2).map_or(Ok(0), |p| p.parse::<u16>())?;
                self.lan_open(LanSettings {
                    port: if port == 0 {
                        lan::available_port()
                    } else {
                        port
                    },
                    guest_commands: words.contains(&"commands"),
                    force_game_mode: words.contains(&"force"),
                })?;
                Ok(Some(json!({ "ok": true, "port": self.lan_port() })))
            }
            Some("close") => {
                self.lan_close(true);
                Ok(Some(json!({ "ok": true })))
            }
            Some("status") => Ok(Some(self.lan_status())),
            _ => Err("lan open|close|status".into()),
        }
    }

    /// `horizon-ctl lan status`.
    fn lan_status(&self) -> serde_json::Value {
        use serde_json::json;
        match &self.lan {
            Some(h) if !self.serving() || self.published_port().is_some() => json!({
                "open": true,
                // The owner's profile name.
                "host": self.options.player_name(),
                "port": h.settings.port,
                // The server's own world: its finished chunks and how many it
                // has generated or loaded so far.
                "world": {
                    "chunks": self.world.chunks.len(),
                    "generated": self.streaming.generated_total,
                },
                "guests": h.hub.guests.iter().map(|g| json!({
                    "name": g.name,
                    "position": g.position,
                    "placed": g.placed,
                    "loaded": g.loaded,
                    "game_mode": g.game_mode,
                    // ItemCooldowns: the ticks the shield an axe disabled stays down.
                    "shield_cooldown": h.play.get(&g.conn).map_or(0, |p| p.shield_cooldown),
                    // The item the server has this guest using: its slot and ticks.
                    "using": h.play.get(&g.conn).and_then(|p| p.using())
                        .map(|(slot, ticks)| json!({ "slot": slot, "ticks": ticks })),
                    "sneaking": g.sneaking,
                    // The server's Elytra glide (shared flag 7).
                    "fall_flying": g.fall_flying,
                    "sleeping": g.bed,
                    "health": g.health,
                    // The server's body: its air supply and golden hearts.
                    "air": g.motion.as_ref().map(|m| m.air),
                    "absorption": g.motion.as_ref().map(|m| m.health.absorption()),
                    // FoodData: food level, saturation and exhaustion.
                    "food": [
                        f64::from(g.food),
                        f64::from(g.saturation),
                        g.motion.as_ref().map_or(0.0, |m| f64::from(m.health.food.exhaustion)),
                    ],
                    "chunks": g.sent_chunks.len(),
                    "tracked": g.tracker.len(),
                })).collect::<Vec<_>>(),
            }),
            _ => json!({ "open": false }),
        }
    }

    pub(super) fn lan_open(&mut self, settings: LanSettings) -> Result<()> {
        let port = if let Some(server) = &self.integrated {
            // ShareToLanScreen: IntegratedServer.publishServer.
            server
                .call(move |g| g.publish(settings))
                .ok_or(super::integrated::NO_ANSWER)??
        } else if self.serving() {
            return self.publish(settings).map(drop).map_err(Into::into);
        } else {
            return Err("no world of this game to open".into());
        };
        self.system_message(Text::translate(
            "menu.multiplayerOptions.publish.started.lan",
            &[Text::literal(port.to_string())],
        ));
        Ok(())
    }
    /// Start the network side and the play-state hub: for the LAN (with the
    /// host's own profile reserved) or for a dedicated server (no host).
    pub(super) fn lan_host(
        &mut self,
        settings: LanSettings,
        net: NetConfig,
        config: HubConfig,
    ) -> Result<()> {
        let mut sync = sync_data(&self.options.resources, &self.pack_files())?;
        let features = if self.level.enabled_features.is_empty() {
            vec!["minecraft:vanilla".to_owned()]
        } else {
            self.level.enabled_features.clone()
        };
        // A world whose packs change biomes sends its own biome registry.
        let packed = self.world_biomes.clone().filter(|b| b.has_packs());
        if packed.is_some()
            || sync.features.iter().any(|f| !features.contains(f))
            || features.iter().any(|f| !sync.features.contains(f))
        {
            // The shared resource preload has vanilla flags and vanilla biomes.
            // Feature flags and biomes belong to this world, not to the cached
            // resources or the next world opened.
            let mut world_sync = SyncData::load_with_kept(
                &self.options.resources,
                &self.pack_files(),
                packed.as_deref(),
                Some(sync.as_ref()),
            )?;
            world_sync.features = features;
            sync = Arc::new(world_sync);
        }
        self.sync_dialog_ids(&sync);
        // The table the world plays with (`world_biomes!`): a world with
        // biome data packs sends their ids past the built-in list, as the
        // encoder's `SectionCoder` expects.
        let biome_names: Vec<String> = world_biomes!(self)
            .list
            .iter()
            .map(|b| b.name.clone())
            .collect();
        let registry: Vec<String> = sync
            .dynamic
            .registries
            .get(BIOME_REGISTRY)
            .map(|v| v.iter().map(|(n, _)| n.clone()).collect())
            .unwrap_or_default();
        let mut encoder = ChunkEncoder::with_signs(
            self.res.facts().states.iter().map(|s| {
                (
                    s.name.clone(),
                    s.fluid_level > 0 || s.waterlogged,
                    s.sign_form(),
                )
            }),
            &sync.builtin,
            &biome_names,
            &registry,
        );
        encoder.tables = Some(
            self.save
                .tables
                .get_or_init(|| Arc::new(horizon_client::save::SaveTables::new(&self.pack.assets)))
                .clone(),
        );
        encoder.mod_update_tag = Some(|pos, entity| {
            let mut tag = None;
            Game::native_block_entity_update_tag(pos, entity, &mut tag);
            tag
        });
        let far = (net.lod_distance > 0)
            .then(|| horizon_client::lod_net::Service::new(net.lod_distance, biome_names.clone()));
        let lan = Lan::start_with(sync.clone(), net)?;
        let mut hub = Hub::new(lan, sync, encoder, config);
        hub.sync_attachments = !crate::mod_api::mods().is_empty();
        hub.flat_world = self.world_flat;
        hub.debug_world = self.world_debug;
        let port = hub.lan.port;
        self.world.block_log = Some(Vec::new());
        self.world.light_log = Some(Default::default());
        self.lan = Some(LanHost {
            players_told_left: false,
            hub,
            settings: LanSettings { port, ..settings },
            play: HashMap::new(),
            broken: Vec::new(),
            mob_state: HashMap::new(),
            poses: HashMap::new(),
            mob_players: Default::default(),
            owner: None,
            waypoints: Default::default(),
            waypoint_dead: Default::default(),
            border_sent: None,
            commands: Vec::new(),
            quiet: std::collections::VecDeque::new(),
            far,
            native_commands: None,
            native_command_nodes: Vec::new(),
            native_usages: Vec::new(),
            sleep_ignored: Default::default(),
            states: Default::default(),
            states_next_entity: None,
            idle_states: Default::default(),
        });
        self.audio.capture = Some(Vec::new());
        Ok(())
    }

    /// IntegratedServer.unpublishServer: the guests are sent away.
    pub(super) fn lan_close(&mut self, announce: bool) {
        if self.integrated.is_some() || self.serving() {
            let closed = match &self.integrated {
                Some(server) => server.call(|g| g.unpublish()).unwrap_or(false),
                None => self.unpublish(),
            };
            if closed {
                self.friends_lan_closed();
            }
            if closed && announce && self.integrated.is_some() {
                self.system_message(Text::translate(
                    "menu.multiplayerOptions.publish.stopped",
                    &[],
                ));
            }
            return;
        }
        self.friends_lan_closed();
        self.native_players_leaving();
        let Some(mut host) = self.lan.take() else {
            return;
        };
        self.award_leave_game_to_all(&mut host);
        self.save_leaving_guests(&mut host);
        host.hub.disconnect_all();
        // Give the network threads a moment to flush the disconnect packets.
        std::thread::sleep(std::time::Duration::from_millis(100));
        self.world.block_log = None;
        self.world.light_log = None;
        self.audio.capture = None;
        self.lan_events.clear();
        self.lan_songs.clear();
        self.lan_critical_hits.clear();
        self.lan_cushion_breaks.clear();
        self.streaming.extra.clear();
        if announce {
            self.system_message(Text::translate(
                "menu.multiplayerOptions.publish.stopped",
                &[],
            ));
        }
    }

    /// `PlayerList.removeAll`: every guest leaves in turn, the vehicle it
    /// alone rides with it (`PlayerList.remove`), and is saved.
    pub(super) fn save_leaving_guests(&mut self, host: &mut LanHost) {
        // Every open menu closes first, in its guest's level
        // (`ServerPlayer.disconnect` -> `doCloseContainer`), so the cursor,
        // the trade and the inputs are back in the file that is saved.
        let back = self.dim;
        let open: Vec<(u32, super::Dim)> = host
            .hub
            .guests
            .iter()
            .filter(|g| g.placed)
            .map(|g| (g.conn, super::Dim::from_id(&g.dimension)))
            .collect();
        for (conn, dim) in open {
            if self.enter_level(Some(host), dim).is_ok() {
                self.guest_close_all(host, conn);
            }
        }
        if let Err(e) = self.enter_level(Some(host), back) {
            log::error!("could not reopen {}: {e}", back.id());
        }
        for i in 0..host.hub.guests.len() {
            let mut g = host.hub.guests.remove(i);
            self.take_parent_vehicle(host, &mut g);
            self.save_guest(host, &mut g);
            host.hub.guests.insert(i, g);
        }
    }
    /// Save a guest when they leave or the world closes (after
    /// `take_parent_vehicle`, `PlayerList.save`): an integrated server's owner
    /// into the level (`/data`) and both its files (`playerdata::save_owner`),
    /// anyone else into their own file.
    pub(super) fn save_guest(
        &mut self,
        host: &LanHost,
        guest: &mut horizon_client::lan::guest::Guest,
    ) {
        self.guest_attachments_into(guest);
        // A player a proxy moved to another server (`player-release moved`) is saved
        // there now, not here.
        if guest.placed && !guest.released {
            let _ = self.finish_player_autosaves(true);
            if host.owner.as_ref().is_some_and(|o| o.uuid == guest.uuid) {
                playerdata::save_owner(guest, &mut self.level, &self.world_root);
            } else {
                self.player_autosaves.save_after(guest, &self.world_root);
            }
            let directory = playerdata::directory(&self.world_root);
            self.native_player_save_to_file(guest.uuid, &guest.name, &directory);
        }
        self.save_guest_records(host, guest);
    }
    /// The vehicles the other players in level `dim` ride, all but the
    /// guest `but` (`Entity.hasExactlyOnePlayerPassenger`); `host` adds the
    /// host's own player, in the level in place.
    pub(super) fn ridden_by_others(
        &self,
        guests: &[horizon_client::lan::guest::Guest],
        dim: &str,
        but: Option<u128>,
        host: bool,
    ) -> Vec<u32> {
        let mut others: Vec<u32> = guests
            .iter()
            .filter(|g| g.placed && g.dimension == dim && Some(g.uuid) != but)
            .filter_map(|g| g.vehicle)
            .collect();
        if host && super::Dim::from_id(dim) == self.dim {
            others.extend(self.riding);
        }
        others
    }
    /// `ServerPlayer.saveParentVehicle` of a player riding `vehicle` in level
    /// `dim`, put in place for it if need be; with `remove`, the vehicle tree
    /// leaves the world with the player (`PlayerList.remove`).
    fn parent_vehicle_in(
        &mut self,
        mut host: Option<&mut LanHost>,
        dim: &str,
        vehicle: Option<u32>,
        others: &[u32],
        remove: bool,
    ) -> Option<(horizon_world::nbt::Tag, [i32; 4])> {
        vehicle?;
        let back = self.dim;
        let theirs = super::Dim::from_id(dim);
        if theirs != back
            && let Err(e) = self.enter_level(host.as_deref_mut(), theirs)
        {
            log::error!("could not open {}: {e}", theirs.id());
            return None;
        }
        let found = if self.dim == theirs {
            self.parent_vehicle_tag(vehicle, others)
        } else {
            None
        };
        if remove && let Some((_, root, _)) = &found {
            self.remove_vehicle_tree(*root);
        }
        if self.dim != back
            && let Err(e) = self.enter_level(host, back)
        {
            log::error!("could not reopen {}: {e}", back.id());
        }
        found.map(|(tag, _, attach)| (tag, attach))
    }
    /// `PlayerList.remove` of a guest out of the hub: its file takes the
    /// vehicle it alone rides (`saveParentVehicle`'s RootVehicle, kept in
    /// `saved` as a file not yet ridden again), the vehicle tree leaves the
    /// world, and the guest stops riding (`unRide`).
    pub(super) fn take_parent_vehicle(
        &mut self,
        host: &mut LanHost,
        guest: &mut horizon_client::lan::guest::Guest,
    ) {
        let Some(vehicle) = guest.vehicle.take() else {
            return;
        };
        if !guest.placed {
            return;
        }
        let others = self.ridden_by_others(&host.hub.guests, &guest.dimension, None, true);
        let Some((tag, attach)) =
            self.parent_vehicle_in(Some(host), &guest.dimension, Some(vehicle), &others, true)
        else {
            return;
        };
        let saved = guest
            .saved
            .get_or_insert_with(|| horizon_world::nbt::Tag::Compound(Default::default()));
        if let horizon_world::nbt::Tag::Compound(p) = saved {
            p.insert("RootVehicle".into(), tag);
        }
        guest.saved_vehicle = Some(attach);
    }
    /// `ServerPlayer.loadAndSpawnParentVehicle`: a guest whose file names the
    /// vehicle it rode rides it again, once the vehicle is in the world.
    fn restore_guest_vehicles(&mut self, host: &mut LanHost) {
        let waiting: Vec<(usize, [i32; 4])> = host
            .hub
            .guests
            .iter()
            .enumerate()
            .filter_map(|(i, g)| Some((i, g.saved_vehicle?)))
            .filter(|(i, _)| {
                let g = &host.hub.guests[*i];
                let chunk = (
                    (g.position[0].floor() as i32).div_euclid(16),
                    (g.position[2].floor() as i32).div_euclid(16),
                );
                g.placed
                    && g.vehicle.is_none()
                    && g.dimension == self.dim.id()
                    && g.sent_chunks.contains(&chunk)
            })
            .collect();
        for (i, uuid) in waiting {
            let raw = host.hub.guests[i].saved.clone();
            let Some(id) = self.restore_saved_vehicle(uuid, raw.as_ref()) else {
                continue;
            };
            host.hub.seat_order += 1;
            let order = host.hub.seat_order;
            let g = &mut host.hub.guests[i];
            if let Some(horizon_world::nbt::Tag::Compound(p)) = &mut g.saved {
                p.remove(&"RootVehicle".into());
            }
            g.vehicle = Some(id);
            g.vehicle_order = order;
            g.saved_vehicle = None;
            // Pair the stationary restored entities before their passenger packet.
            g.moved = true;
        }
    }
    /// The file name of a player's advancements and statistics: an
    /// integrated server's owner keeps single player's.
    fn records_uuid(&self, host: &LanHost, uuid: u128) -> Option<[i32; 4]> {
        if host.owner.as_ref().is_some_and(|o| o.uuid == uuid) {
            self.level.player_uuid
        } else {
            Some(playerdata::uuid_ints(uuid))
        }
    }
    /// `PlayerAdvancements.save` and `ServerStatsCounter.save` of a network player.
    pub(super) fn save_guest_records(
        &self,
        host: &LanHost,
        guest: &horizon_client::lan::guest::Guest,
    ) {
        if !guest.placed || guest.released || guest.fake || self.adv.tree.list.is_empty() {
            return;
        }
        let Some(uuid) = self.records_uuid(host, guest.uuid) else {
            return;
        };
        let path = super::stats::player_json(&self.world_root, "advancements", uuid);
        let json = guest
            .advancements
            .to_json(&self.adv.tree, horizon_world::DATA_VERSION);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(&json) {
            let _ = std::fs::write(path, text);
        }
        if let Some(play) = host.play.get(&guest.conn) {
            play.stats
                .write(&super::stats::player_json(&self.world_root, "stats", uuid));
        }
    }
    /// The advancements and statistics files of a held arrival, before it is placed
    /// (`player-confirm`): each through a new file, synced and renamed into place
    /// (`save::write_durable`), so the reply's `ok` means both are on the disk whole.
    pub(super) fn write_arrival_records(
        &self,
        host: &LanHost,
        guest: &horizon_client::lan::guest::Guest,
    ) -> std::result::Result<(), String> {
        if self.adv.tree.list.is_empty() {
            return Ok(());
        }
        let Some(uuid) = self.records_uuid(host, guest.uuid) else {
            return Ok(());
        };
        let write = |dir: &str, json: &serde_json::Value| {
            let path = super::stats::player_json(&self.world_root, dir, uuid);
            let text = serde_json::to_string_pretty(json).map_err(|e| e.to_string())?;
            horizon_client::save::write_durable(&path, text.as_bytes())
                .map_err(|e| format!("{} is not saved: {e}", path.display()))
        };
        write(
            "advancements",
            &guest
                .advancements
                .to_json(&self.adv.tree, horizon_world::DATA_VERSION),
        )?;
        if let Some(play) = host.play.get(&guest.conn) {
            write("stats", &play.stats.to_json())?;
        }
        Ok(())
    }
    /// `PlayerAdvancements.load` and `ServerStatsCounter.parseLocal` of a
    /// network player who joined; every statistic goes out on the first
    /// request (`PlayerList.placeNewPlayer`'s `markAllDirty`).
    fn load_guest_records(&mut self, host: &mut LanHost, conn: u32) {
        let Some((uuid, name)) = host
            .hub
            .guest(conn)
            .and_then(|g| Some((self.records_uuid(host, g.uuid)?, g.name.clone())))
        else {
            return;
        };
        let tree = self.advancement_tree();
        let path = super::stats::player_json(&self.world_root, "advancements", uuid);
        let saved = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
        if let Some(g) = host.hub.guest_mut(conn)
            && let Some(json) = saved
        {
            let mut progress = horizon_client::advancements::Progress::from_json(&json);
            // PlayerAdvancements.applyFrom: advancements this server lacks are dropped.
            progress
                .criteria
                .retain(|id, _| tree.by_id.contains_key(id));
            g.advancements = progress;
            g.advancements_loaded = true;
        }
        // PlayerList.getPlayerStats: locateStatsFile moves an old name-based file first.
        let mut stats = super::stats::Stats::read(&super::stats::locate_player_json(
            &self.world_root,
            "stats",
            uuid,
            &name,
        ));
        stats.mark_all_dirty();
        host.play.entry(conn).or_default().stats = stats;
    }
    /// The integrated server's owner into the world's player, for `Level::write`.
    /// Returns the owner's uuid and name when they were stored.
    pub(super) fn store_owner(&mut self) -> Option<(u128, String)> {
        let uuid = self.lan.as_ref().and_then(|h| h.owner.as_ref())?.uuid;
        let (riding, dim, others) = self.lan.as_ref().and_then(|h| {
            let g = h.hub.guests.iter().find(|g| g.uuid == uuid && g.placed)?;
            let others = self.ridden_by_others(&h.hub.guests, &g.dimension, Some(uuid), true);
            Some((g.vehicle, g.dimension.clone(), others))
        })?;
        // The vehicle the owner rides (`ServerPlayer.saveParentVehicle`).
        let vehicle = self
            .parent_vehicle_in(None, &dim, riding, &others, false)
            .map(|(tag, _)| tag);
        let host = self.lan.as_ref()?;
        let mut stored = None;
        if let Some(g) = host.hub.guests.iter().find(|g| g.uuid == uuid && g.placed) {
            playerdata::store(g, &mut self.level, vehicle);
            self.save_guest_records(host, g);
            stored = Some(g.name.clone());
        } else if let Some(saved) = host.owner.as_ref().and_then(|o| o.saved.as_ref()) {
            self.level.position = saved.position;
        }
        stored.map(|name| (uuid, name))
    }

    /// The world's default game mode (`defaultgamemode`).
    pub(super) fn set_default_game_mode(&mut self, mode: i32) {
        self.level.game_mode = mode;
        self.level.creative = mode == 1;
        if let Some(host) = self.lan.as_mut() {
            host.hub.config.default_game_mode = mode;
            // `setWorldGameType` and `DefaultGameModeCommands.setMode`: a
            // world that forces its mode puts the guests online in it.
            enforce_guest_game_mode(host, &self.world, self.collision);
        }
    }

    /// Apply the default mode to connected players when forcing it is enabled.
    pub(super) fn set_force_game_mode(&mut self, force: bool) {
        if let Some(host) = self.lan.as_mut() {
            host.settings.force_game_mode = force;
            host.hub.config.force_game_mode = force;
            enforce_guest_game_mode(host, &self.world, self.collision);
        }
    }

    /// Update the live view distance used for player chunk tracking.
    pub(super) fn set_view_distance(&mut self, distance: i32) {
        if let Some(host) = self.lan.as_mut() {
            host.hub.set_view_distance(distance);
        }
    }

    /// Update the live spawn-protection radius.
    pub(super) fn set_spawn_protection(&mut self, radius: i32) {
        if let Some(host) = self.lan.as_mut() {
            host.hub.spawn_protection = radius;
        }
    }

    /// Update the live simulation distance sent to player clients.
    pub(super) fn set_simulation_distance(&mut self, distance: i32) {
        if let Some(host) = self.lan.as_mut() {
            host.hub.set_simulation_distance(distance);
        }
    }

    /// Update the live entity tracking range percentage.
    pub(super) fn set_entity_broadcast_range_percentage(&mut self, percentage: i32) {
        if let Some(host) = self.lan.as_mut() {
            host.hub.entity_range_percent = percentage;
        }
    }

    /// Update the live movement check's flight policy.
    pub(super) fn set_allow_flight(&mut self, allow: bool) {
        if let Some(host) = self.lan.as_mut() {
            host.hub.config.allow_flight = allow;
        }
    }

    /// Update whether status responses include online player names.
    pub(super) fn set_hide_online_players(&mut self, hide: bool) {
        if let Some(host) = self.lan.as_ref() {
            host.hub.lan.shared.set_hide_online_players(hide);
        }
    }

    /// Update whether the server answers connection status requests.
    pub(super) fn set_status_replies(&mut self, enable: bool) {
        if let Some(host) = self.lan.as_ref() {
            host.hub.lan.shared.set_enable_status(enable);
        }
    }

    /// New command access and game mode forcing settings of the open world.
    pub(super) fn lan_apply_settings(&mut self, guest_commands: bool, force: bool) {
        if let Some(server) = &self.integrated {
            server.call(move |g| g.lan_apply_settings(guest_commands, force));
            return;
        }
        // An integrated server gives guests commands only where the world
        // allows them (IntegratedServer.getCustomPermissionLevel).
        let effective = guest_commands && (!self.serving() || self.allow_commands());
        if let Some(host) = self.lan.as_mut() {
            host.settings.guest_commands = guest_commands;
            host.hub.config.guest_commands = effective;
        }
        self.set_force_game_mode(force);
        self.resend_commands_filtered();
    }

    pub(super) fn lan_port(&self) -> Option<u16> {
        if let Some(server) = &self.integrated {
            return server.published();
        }
        if self.serving() {
            return self.published_port();
        }
        self.lan.as_ref().map(|h| h.settings.port)
    }

    /// The entities guests track: mobs, items and the rest (game/lan_entities.rs).
    /// `mob_players` is the guest each mob sees as its player (game/lan_mobs.rs).
    /// Only the entities whose id is above `after` (0: all of them). With
    /// `kept`, idle mobs and items no client can pair with are described bare
    /// (game/lan_idle.rs).
    fn lan_entities(
        &mut self,
        mob_players: &horizon_client::fast_hash::FxHashMap<u32, u32>,
        hub: &mut Hub,
        out: &mut States,
        drain_minecart_steps: bool,
        after: u32,
        mut kept: Option<&mut super::lan_idle::IdlePass<'_>>,
    ) {
        out.clear();
        let mut speeds = super::lan_attributes::SpeedByType::default();
        // ChunkMap.tick sends the changes of an entity outside the
        // entity-ticking range only when it changed section or needs sync.
        let levels = self.tickets.levels();
        let idle_at = |p: horizon_core::geometry::Vec3| {
            !levels.in_entity_ticking_range((p.x.floor() as i32) >> 4, (p.z.floor() as i32) >> 4)
        };
        for m in self.mobs.iter().filter(|m| !m.removed && m.id > after) {
            // The lead's holder: the player the mob sees, another mob or a
            // fence knot (game/lan_entities.rs).
            let player_entity = || {
                (mob_players.get(&m.id))
                    .and_then(|conn| hub.guest(*conn))
                    .map(|g| g.entity_id)
            };
            let leash = m.extra.leash.and_then(|h| match h {
                Holder::Player => match m.extra.leash_player {
                    Some(u) => hub
                        .guests
                        .iter()
                        .find(|g| playerdata::uuid_ints(g.uuid) == u)
                        .map(|g| g.entity_id),
                    None => player_entity(),
                },
                Holder::Mob(id) | Holder::Boat(id) => Some(id as i32),
                Holder::Knot(p) => self.knot_ids.get(&p).map(|&id| id as i32),
            });
            let idle = idle_at(m.position);
            let place = super::lan_idle::Place {
                position: [m.position.x, m.position.y, m.position.z],
                rotation: (m.yaw, m.head_pitch, m.head_yaw),
                on_ground: m.on_ground,
                velocity: [m.velocity.x, m.velocity.y, m.velocity.z],
            };
            let kind = m.kind.id.as_str();
            let bare = idle
                && (kept.as_deref_mut()).is_some_and(|k| k.bare(hub, m.id as i32, kind, &place))
                && !m.needs_sync;
            let e = out.add(m.id as i32, entity_uuid(m.id), kind);
            e.position = place.position;
            (e.yaw, e.pitch, e.head_yaw) = place.rotation;
            e.on_ground = place.on_ground;
            e.velocity = place.velocity;
            e.needs_sync = m.needs_sync;
            e.sync_position = m.sync_position;
            e.stepped = true;
            (e.idle, e.bare) = (idle, bare);
            e.leash = leash;
            if bare {
                continue;
            }
            mob_meta(
                m,
                hub,
                player_entity(),
                self.world.game_time,
                &mut e.meta,
                &mut e.defaults,
            );
            // HappyGhast.aiStep: requiresPrecisePosition while on a still timeout.
            e.precise = m.species() == horizon_client::mobs::Species::HappyGhast
                && horizon_client::mobs::happy_ghast::on_still_timeout(m);
            mob_freeze_meta(m, &mut e.meta, &mut e.defaults);
            mob_equipment(m, hub, &mut e.equipment);
            self.mob_attributes(m, &mut speeds, &mut e.attributes);
        }
        for d in self.dropped.iter().filter(|d| d.id > after) {
            let idle = idle_at(d.position);
            let place = super::lan_idle::Place {
                position: [d.position.x, d.position.y, d.position.z],
                rotation: (0.0, 0.0, 0.0),
                on_ground: d.on_ground,
                velocity: [d.velocity.x, d.velocity.y, d.velocity.z],
            };
            let bare = idle
                && (kept.as_deref_mut())
                    .is_some_and(|k| k.bare(hub, d.id as i32, "minecraft:item", &place))
                && !d.needs_sync;
            let e = out.add(d.id as i32, entity_uuid(d.id), "minecraft:item");
            e.position = place.position;
            e.on_ground = place.on_ground;
            e.velocity = place.velocity;
            (e.idle, e.bare) = (idle, bare);
            if bare {
                continue;
            }
            let stack = hub.stack(&Some(d.stack.clone()));
            // `ServerEntity.getRequiresPrecisePosition` is false for an item:
            // `needs_sync` asks for its data again, not for an exact position.
            // The collision case is `full_precision_required` in the tracker.
            e.precise = false;
            e.needs_sync = d.needs_sync;
            e.vertical_collision = d.vertical_collision;
            e.horizontal_collision = d.horizontal_collision;
            // Entity.DATA_SHARED_FLAGS_ID: glowing 0x40 (the Glowing tag).
            if self.entity_glowing(d.id) {
                e.meta.push((0, serializer::BYTE, Value::Byte(0x40)));
            }
            e.meta.push((8, serializer::ITEM_STACK, Value::Item(stack)));
        }
        for o in self.orbs.iter().filter(|o| o.id > after) {
            let e = out.add(o.id as i32, entity_uuid(o.id), "minecraft:experience_orb");
            e.position = [o.position.x, o.position.y, o.position.z];
            e.idle = idle_at(o.position);
            e.on_ground = o.on_ground;
            e.velocity = [o.velocity.x, o.velocity.y, o.velocity.z];
            // ExperienceOrb.DATA_VALUE.
            e.meta
                .push((8, serializer::INT, Value::Int(o.value as i32)));
        }
        self.lan_object_states(hub, out, drain_minecart_steps, after);
    }

    /// Apply gameplay events produced while the hub handles guest packets.
    ///
    /// Both the pre-block packet phase and the normal LAN tick receive the
    /// same event stream. Keep their side effects in one place so joins,
    /// departures, chat, and queued guest commands cannot drift apart. None when a native hook's
    /// command closed the hub.
    fn lan_process_events(
        &mut self,
        mut host: LanHost,
        events: impl IntoIterator<Item = HostEvent>,
        entities: &[EntityState],
        spawn: [f64; 3],
    ) -> Option<LanHost> {
        // A slow tick's line says which event took it, and whose (`spans::note`): each event's
        // time is noted when the next one starts (an event's arm may `continue`), the last's
        // after the loop.
        let timed = horizon_client::spans::recording_here();
        let mut previous = None;
        let mut timings = EventTimings::new(horizon_client::spans::NOTE_MS);
        for event in events {
            if timed {
                note_event(&host, previous.take(), &mut timings);
                let detail = self.event_detail(&host, &event);
                previous = Some((Instant::now(), event_kind(&event), detail));
            }
            match event {
                event @ (HostEvent::Chat { .. } | HostEvent::Warn(_) | HostEvent::Error(_)) => {
                    self.lan_log_event(event);
                }
                HostEvent::Ban { uuid, name, reason } => {
                    self.anticheat_ban(&mut host.hub, uuid, &name, &reason);
                }
                HostEvent::ChatFiltered {
                    sender,
                    name,
                    place,
                    text,
                    count,
                    action,
                } => self.chat_filtered(&mut host, sender, &name, place, &text, count, action),
                HostEvent::PlayerChat { sender, text } => {
                    if let Some(g) = host.hub.guests.iter().find(|g| g.uuid == sender) {
                        if self.options.dedicated.is_some() {
                            super::dedicated::events::record(
                                "chat",
                                &g.name,
                                sender,
                                serde_json::json!({ "message": text }),
                            );
                        }
                        let conn = g.conn;
                        self.detect_rate_spam(&mut host, conn, false);
                    }
                }
                HostEvent::Joined(conn) => {
                    // The mods may refuse the login before the player is placed.
                    if let Some((uuid, name, permission)) = host
                        .hub
                        .guest(conn)
                        .map(|g| (g.uuid, g.name.clone(), g.permission))
                    {
                        let mut cancel = false;
                        let mut reason = None;
                        self.native_login_permission = Some((uuid, permission));
                        self.native_player_login(uuid, &name, &mut cancel, &mut reason);
                        self.native_login_permission = None;
                        if cancel {
                            let reason = match reason {
                                Some(text) => {
                                    horizon_client::lan::data::component_nbt(&text.component())
                                }
                                None => horizon_protocol::text::translatable(
                                    "multiplayer.disconnect.generic",
                                    &[],
                                ),
                            };
                            host.hub.kick(conn, reason);
                            continue;
                        }
                    }
                    // PlayerList.placeNewPlayer: the name the user cache knew,
                    // before it learns this one, for a renamed player's join line.
                    let mut renamed = None;
                    if self.options.dedicated.is_some()
                        && let Some((uuid, name)) =
                            host.hub.guest(conn).map(|g| (g.uuid, g.name.clone()))
                        && let Some(old) = super::dedicated::cached_user_by_id(uuid)
                    {
                        if !old.name.eq_ignore_ascii_case(&name) {
                            renamed = Some(old.name.clone());
                        }
                        host.hub.former_names.insert(uuid, old.name);
                    }
                    self.lan_guest_joined(&mut host, conn, entities, spawn);
                    if self.options.dedicated.is_some()
                        && let Some(g) = host.hub.guest(conn)
                    {
                        // PlayerList.placeNewPlayer, then the broadcast join message.
                        super::dedicated::player_joined(g.uuid, &g.name);
                        super::dedicated::events::record(
                            "join",
                            &g.name,
                            g.uuid,
                            serde_json::json!({ "address": g.address.ip().to_string() }),
                        );
                        let [x, y, z] = g.position;
                        super::dedicated::log(
                            "INFO",
                            &format!(
                                "{}[/{}] logged in with entity id {} at ({}, {}, {})",
                                g.name,
                                g.address,
                                g.entity_id,
                                super::dedicated::java_double(x),
                                super::dedicated::java_double(y),
                                super::dedicated::java_double(z)
                            ),
                        );
                        // A held player is not announced: it is not in the world yet.
                        if g.held {
                            super::dedicated::log(
                                "INFO",
                                &horizon_client::lan::arrivals::held_line(&g.name),
                            );
                        } else if let Some(old) = renamed {
                            let line = Text::translate(
                                "multiplayer.player.joined.renamed",
                                &[Text::literal(g.name.clone()), Text::literal(old)],
                            );
                            super::dedicated::log(
                                "INFO",
                                &format!("System chat: {}", line.plain()),
                            );
                        } else {
                            super::dedicated::log(
                                "INFO",
                                &format!("System chat: {} joined the game", g.name),
                            );
                        }
                    } else if self.player.is_some()
                        && let Some(g) = host.hub.guest(conn)
                        && g.placed
                    {
                        // The host's chat hears of the newcomer as the guests do.
                        let who = self.player_text(&g.name, g.uuid);
                        self.system_message(
                            Text::translate("multiplayer.player.joined", &[who])
                                .with_color(horizon_client::text::YELLOW),
                        );
                    }
                    if let Some((uuid, name)) = host
                        .hub
                        .guest(conn)
                        .filter(|g| g.placed)
                        .map(|g| (g.uuid, g.name.clone()))
                    {
                        self.lan = Some(host);
                        self.native_player_logged_in(uuid, &name);
                        // What the client said in its configuration (native_packets.rs).
                        self.configuration_heard(uuid, &name);
                        // A hook's command can close the hub.
                        host = self.lan.take()?;
                    }
                }
                HostEvent::Left(mut guest) => {
                    // The menu they had open closes, in the level they were in
                    // (`ServerPlayer.disconnect` -> `doCloseContainer`).
                    if guest.placed {
                        let back = self.dim;
                        let theirs = super::Dim::from_id(&guest.dimension);
                        if self.enter_level(Some(&mut host), theirs).is_ok() {
                            let conn = guest.conn;
                            let at = host.hub.guests.len();
                            host.hub.guests.push(*guest);
                            // Bukkit's PlayerQuitEvent: the mods hear them leave while
                            // they are still here.
                            if !host.players_told_left {
                                host = self.tell_mods_logging_out(host, conn)?;
                            }
                            self.guest_disconnected(&mut host, conn);
                            *guest = host.hub.guests.remove(at);
                        }
                        if let Err(e) = self.enter_level(Some(&mut host), back) {
                            log::error!("could not reopen {}: {e}", back.id());
                        }
                    }
                    self.lan_crafting_left(&mut host, &mut guest);
                    if guest.placed {
                        self.award_leave_game(&mut host, guest.conn, guest.uuid, &guest.name);
                    }
                    // PlayerList.remove: the vehicle it alone rides goes too.
                    self.take_parent_vehicle(&mut host, &mut guest);
                    self.save_guest(&host, &mut guest);
                    self.guest_attachments.remove(&guest.uuid);
                    host.play.remove(&guest.conn);
                    self.guest_combat.remove(&guest.conn);
                    // A held player was recorded as joined at its login: it leaves too.
                    if self.options.dedicated.is_some() && (guest.placed || guest.held) {
                        super::dedicated::events::record(
                            "leave",
                            &guest.name,
                            guest.uuid,
                            serde_json::json!({ "reason": guest.left_reason }),
                        );
                        if guest.held && !guest.placed {
                            // Never announced, so no "left the game" either.
                            super::dedicated::log(
                                "INFO",
                                &horizon_client::lan::arrivals::dropped_line(
                                    &guest.name,
                                    &guest.left_reason,
                                ),
                            );
                        } else {
                            super::dedicated::log(
                                "INFO",
                                &format!("{} lost connection: {}", guest.name, guest.left_reason),
                            );
                        }
                        // A player moved to another server without `announce` was not
                        // announced as leaving, so the line is not logged either; a held
                        // player was never announced at all.
                        if guest.placed && !guest.quiet_leave {
                            super::dedicated::log(
                                "INFO",
                                &format!("System chat: {} left the game", guest.name),
                            );
                        }
                    } else if self.player.is_some() && guest.placed && !guest.quiet_leave {
                        // ServerGamePacketListenerImpl.removePlayerFromWorld.
                        let who = self.player_text(&guest.name, guest.uuid);
                        self.system_message(
                            Text::translate("multiplayer.player.left", &[who])
                                .with_color(horizon_client::text::YELLOW),
                        );
                    }
                    if guest.placed && !host.players_told_left {
                        self.lan = Some(host);
                        self.native_player_logged_out(guest.uuid, &guest.name);
                        host = self.lan.take()?;
                    }
                }
                HostEvent::Flight { conn, flying } => {
                    self.lan_flight_toggled(&mut host, conn, flying);
                }
                HostEvent::Sneak { conn, sneaking } => {
                    if horizon_client::mod_api::mods().is_empty() {
                        continue;
                    }
                    if let Some(g) = host.hub.guest(conn) {
                        let (uuid, name) = (g.uuid, g.name.clone());
                        self.as_guest(&mut host, conn, |game| {
                            game.native_player_toggle_sneak(uuid, &name, sneaking);
                        });
                    }
                }
                HostEvent::Kicked {
                    conn,
                    player,
                    name,
                    reason,
                } => {
                    if horizon_client::mod_api::mods().is_empty() {
                        continue;
                    }
                    // The same poll can have removed the guest (their `Left`): the mods hear the
                    // kick anyway, without the guest's state swapped in.
                    if host.hub.guest(conn).is_some() {
                        self.as_guest(&mut host, conn, |game| {
                            game.native_player_kicked(player, &name, &reason);
                        });
                    } else {
                        self.native_player_kicked(player, &name, &reason);
                    }
                }
                HostEvent::Input { conn, flags } => {
                    if horizon_client::mod_api::mods().is_empty() {
                        continue;
                    }
                    if let Some(g) = host.hub.guest(conn) {
                        let (uuid, name) = (g.uuid, g.name.clone());
                        self.as_guest(&mut host, conn, |game| {
                            game.native_player_input(uuid, &name, flags);
                        });
                    }
                }
                HostEvent::MainHandChanged(conn, left) => {
                    if let Some(g) = host.hub.guest(conn) {
                        let (uuid, name) = (g.uuid, g.name.clone());
                        self.native_player_change_main_hand(uuid, &name, left);
                    }
                }
                HostEvent::LocaleChanged(conn, locale) => {
                    if let Some(g) = host.hub.guest(conn) {
                        let (uuid, name) = (g.uuid, g.name.clone());
                        self.native_player_locale_change(uuid, &name, &locale);
                    }
                }
                HostEvent::PlayerLoaded(conn) => {
                    if let Some(g) = host.hub.guest(conn) {
                        let (uuid, name) = (g.uuid, g.name.clone());
                        self.native_player_client_loaded_world(uuid, &name);
                    }
                }
                HostEvent::RecipeBookSettings {
                    conn,
                    book,
                    open,
                    filtering,
                } => {
                    if let Some(g) = host.hub.guest(conn) {
                        let (uuid, name) = (g.uuid, g.name.clone());
                        self.native_player_recipe_book_settings_change(
                            uuid, &name, book, open, filtering,
                        );
                    }
                }
                HostEvent::Packet(conn, packet) => {
                    self.lan_guest_packet(&mut host, conn, *packet);
                    if !host.commands.is_empty() {
                        self.lan = Some(host);
                        self.run_guest_commands();
                        host = self.lan.take().expect("the hub is back after the commands");
                    }
                }
                HostEvent::Jump { conn, from, yaw } => {
                    self.guest_jump(&mut host, conn, from, yaw);
                }
            }
        }
        note_event(&host, previous, &mut timings);
        for burst in timings.bursts() {
            horizon_client::spans::note(burst.ms, || describe_burst(&host, &burst));
        }
        Some(host)
    }

    /// Handle this level's player packets before scheduled block ticks
    /// (`ServerGamePacketListenerImpl` ahead of `ServerLevel`'s block phase).
    /// A sand place in the same tick as another sand's fall schedule still
    /// sees the support (#4510). `tick_lan` then syncs the level; a paused
    /// tick that never calls this still drains inside `tick_lan`.
    pub(super) fn tick_lan_packets(&mut self) {
        if self.level_pass.packets_drained {
            return;
        }
        let Some(mut host) = self.lan.take() else {
            return;
        };
        let holding = lan::Holding::start();
        // Last tick's tracked entities: enough for place collision and joins
        // before this tick moves anyone.
        let entities = std::mem::take(&mut host.states);
        let spawn = self.world_spawn_position();
        let events = {
            let clock_rate = self.network_clock_rate();
            let mut ctx = TickCtx {
                world: &mut self.world,
                entities: entities.as_slice(),
                next_entity: &mut self.next_entity,
                spawn,
                spawn_angle: self.level.world_spawn_angle,
                difficulty: i32::from(self.ui.difficulty),
                difficulty_locked: self.level.difficulty_locked,
                clock_rate,
                tick_rate: self.clock.rate(),
                tick_frozen: self.clock.frozen(),
                tick_steps: self.clock.step_state(),
                collision: Some(self.collision),
                border: &self.border,
                streaming: Some(&self.streaming),
            };
            host.hub.movement_checks = (
                self.ui.rules.bool("player_movement_check"),
                self.ui.rules.bool("elytra_movement_check"),
            );
            host.hub.mods_loaded = !horizon_client::mod_api::mods().is_empty();
            let pass = self.level_pass;
            let mut events = if pass.first {
                horizon_client::spans::time("lan_poll", || host.hub.poll(&mut ctx))
            } else {
                Vec::new()
            };
            events.extend(horizon_client::spans::time("lan_drain", || {
                host.hub.drain_packets(&mut ctx)
            }));
            events
        };
        let Some(back) = horizon_client::spans::time("lan_events", || {
            self.lan_process_events(host, events, entities.as_slice(), spawn)
        }) else {
            return;
        };
        host = back;
        if !host.quiet.is_empty() {
            self.lan = Some(host);
            horizon_client::spans::time("lan_quiet_commands", || self.run_quiet_commands());
            host = self
                .lan
                .take()
                .expect("the hub is back after the quiet commands");
        }
        if !host.hub.native_advancement_grants.is_empty() {
            self.lan = Some(host);
            horizon_client::spans::time("lan_advancement_grants", || {
                self.run_native_advancement_grants()
            });
            host = self
                .lan
                .take()
                .expect("the hub is back after the advancement grants");
        }
        if !host.hub.native_menu_events.is_empty() || !host.hub.native_menu_returns.is_empty() {
            self.lan = Some(host);
            horizon_client::spans::time("lan_menu_events", || self.run_native_menu_events());
            host = self
                .lan
                .take()
                .expect("the hub is back after the menu events");
        }
        if !self.native_moves.is_empty() {
            self.lan = Some(host);
            horizon_client::spans::time("lan_native_moves", || self.run_native_moves());
            host = self
                .lan
                .take()
                .expect("the hub is back after the mods' moves");
        }
        host.states = entities;
        self.level_pass.packets_drained = true;
        drop(holding);
        self.lan = Some(host);
    }

    /// The entity id counter, once the entities Horizon keeps without one
    /// have theirs, while guests track the level (None otherwise).
    fn lan_ids_so_far(&mut self) -> Option<i32> {
        if self.lan.as_ref().is_none_or(|h| h.hub.guests.is_empty()) {
            return None;
        }
        self.lan_assign_ids();
        Some(self.next_entity as i32)
    }

    /// ServerLevel.tick's ChunkMap.tick point, after the block ticks and
    /// before the entities': what joined the level up to here (a falling
    /// block, a dispensed arrow, the last tick's late joins) is paired and
    /// first updated in this tick's pass (`Entities::synced`).
    /// `ServerChunkCache.tick`'s broadcast, where ServerLevel.tick runs it:
    /// after the scheduled and random ticks, before the block events, so the
    /// players get the blocks as they are then. The block log's length after
    /// it: what the tick writes from there on goes out with the next tick's.
    pub(super) fn lan_broadcast_changes(&mut self) -> Option<usize> {
        if let Some(host) = self.lan.as_mut() {
            host.hub.broadcast_changes(&mut self.world);
        }
        self.world.block_log.as_ref().map(Vec::len)
    }

    pub(super) fn lan_mark_synced(&mut self) {
        if let Some(next) = self.lan_ids_so_far()
            && let Some(host) = self.lan.as_mut()
        {
            host.hub.synced = vec![(i32::MIN, next)];
        }
    }

    /// The start of a phase vanilla runs before ChunkMap.tick and Horizon
    /// among the entities' ticks (ServerChunkCache.tickChunks' spawning and
    /// thunder, the raids): the counter to give `lan_synced_since`.
    pub(super) fn lan_synced_from(&mut self) -> Option<i32> {
        self.lan_ids_so_far()
    }

    /// The end of such a phase: what joined during it is first updated in
    /// this tick's pass too.
    pub(super) fn lan_synced_since(&mut self, from: Option<i32>) {
        if let Some(from) = from
            && let Some(next) = self.lan_ids_so_far()
            && next > from
            && let Some(host) = self.lan.as_mut()
        {
            host.hub.synced.push((from, next));
        }
    }

    /// The end of the level's tick: the entities added since its states were
    /// taken for the guests (a falling block's drop, a spawner's mob, the
    /// host's snowball) are paired without another tracking tick
    /// (`Hub::note_joined`). Only they are
    /// described (their ids are above the counter the states were taken at),
    /// and nothing when the counter has not moved or no guest is in the level.
    pub(super) fn lan_note_joined(&mut self) {
        let Some(mut host) = self.lan.take() else {
            return;
        };
        host = self.lan_flush_tracking(host);
        host.states_next_entity = None;
        self.flush_attachment_sync(&mut host);
        self.lan = Some(host);
    }

    /// Deliver start/stop callbacks before sounds and attachment snapshots.
    /// A callback can spawn another entity, whose pairing is part of the same
    /// operation, as a nested ChunkMap.addEntity would be in vanilla.
    fn lan_flush_tracking(&mut self, mut host: LanHost) -> LanHost {
        loop {
            self.lan_pair_joined(&mut host);
            host.hub.collect_tracking();
            if host.hub.native_tracking.is_empty() {
                return host;
            }
            self.lan = Some(host);
            self.run_native_tracking();
            host = self
                .lan
                .take()
                .expect("the hub is back after the tracking events");
        }
    }

    /// Pair late arrivals before a sound flush as well as at the end of the
    /// tick. The counter prevents taking or draining an entity's state twice.
    pub(super) fn lan_pair_joined(&mut self, host: &mut LanHost) {
        if let Some(next) = host.states_next_entity
            && host.hub.guests.iter().any(|g| host.hub.here(g))
        {
            self.lan_assign_ids();
            if self.next_entity != next {
                let mut states = States::default();
                self.lan_entities(
                    &host.mob_players,
                    &mut host.hub,
                    &mut states,
                    false,
                    next,
                    None,
                );
                self.lan_hook_states(&host.play, &host.hub, &mut states);
                self.lan_entity_uuids(&mut states);
                host.hub.note_joined(states.as_slice());
            }
            host.states_next_entity = Some(self.next_entity);
        }
    }

    /// Run the hub for this tick and apply what the guests did. While the
    /// single player game is `paused` only the connections run
    /// (`IntegratedServer.tickPaused`): `ServerGamePacketListenerImpl.tick`
    /// skips `tickPlayer`, so no player's air, hunger or damage moves on.
    pub(super) fn tick_lan(&mut self, paused: bool) {
        // A frozen level does not reach tick_lan_packets through tick_world.
        // Apply its packets before taking the snapshot too.
        if !paused {
            self.tick_lan_packets();
        }
        let Some(mut host) = self.lan.take() else {
            if self.level_pass.running.is_none() {
                self.level_pass.packets_drained = false;
            }
            return;
        };
        // What the tick sends a guest goes to its connection at the end.
        let holding = lan::Holding::start();
        self.register_native_commands(&mut host);
        self.resend_native_trees(&mut host);
        // The host's own commands' (and command blocks') players moving
        // to another level, and their broadcastToAdmins lines.
        if !self.level_teleports.is_empty() {
            self.run_level_teleports(&mut host);
        }
        if !self.admin_feedback.is_empty() {
            self.send_admin_feedback(&mut host, None);
        }
        // An animal whose rider left the game (or the level) stops taking
        // its input.
        for m in &mut self.mobs {
            if m.rider.is_some()
                && self.riding != Some(m.id)
                && !host.hub.guests.iter().any(|g| g.vehicle == Some(m.id))
            {
                m.rider = None;
            }
        }
        // A guest whose file names the vehicle it rode rides it again.
        self.restore_guest_vehicles(&mut host);
        // Finish the players' mining, item use and pickups before describing
        // this tick to their clients. Otherwise a newly broken block and its
        // drops remain absent from the snapshot until the following tick.
        if !paused {
            horizon_client::spans::time("lan_guests", || self.tick_guests(&mut host));
            // The vehicles the guests just moved enter their portals now.
            let ridden: Vec<u32> = host.hub.guests.iter().filter_map(|g| g.vehicle).collect();
            self.find_ridden_vehicle_portals(&ridden);
        }
        // A guest's mount fired its `started_riding` with that guest alone in
        // place (the hub held by its action): now every player on the vehicle
        // hears it (`Entity.startRiding`).
        if !self.started_riding.is_empty() {
            self.lan = Some(host);
            self.flush_started_riding();
            host = self.lan.take().expect("the hub is back after the triggers");
        }
        {
            let has_guests = !host.hub.guests.is_empty();
            let mut states = std::mem::take(&mut host.states);
            if has_guests {
                self.lan_assign_ids();
                self.lan_riding(&host.hub.guests, &mut host.hub.riding);
                // Idle states are kept only when the players' moves are in:
                // the clients that could pair are known. The range scale is
                // the one the tracker gets below.
                let mut kept = if self.level_pass.packets_drained {
                    let percent = if host.hub.config.integrated {
                        (self.settings.entity_distance * 100.0).round() as i32
                    } else {
                        host.hub.entity_range_percent
                    };
                    host.idle_states.pass(&host.hub, percent)
                } else {
                    None
                };
                horizon_client::spans::time("lan_entity_states", || {
                    self.lan_entities(
                        &host.mob_players,
                        &mut host.hub,
                        &mut states,
                        true,
                        0,
                        kept.as_mut(),
                    );
                    if let Some(kept) = kept {
                        kept.finish();
                    }
                    self.lan_hook_states(&host.play, &host.hub, &mut states);
                    self.lan_entity_uuids(&mut states);
                });
                host.states_next_entity = Some(self.next_entity);
            } else {
                host.states_next_entity = None;
                states.clear();
                // Nobody can receive past movement. A returning viewer pairs
                // from the cart's current position/rotation, as AddEntity does.
                for cart in &mut self.carts {
                    cart.lerp_steps.clear();
                }
            }
            // ServerEntity.sendChanges clears needsSync once it has looked.
            // SteppedInterpolationTracker.updateTracking clears syncPosition.
            for m in &mut self.mobs {
                m.needs_sync = false;
                m.sync_position = false;
            }
            for p in &mut self.creatures.projectiles {
                p.needs_sync = false;
            }
            for d in &mut self.dropped {
                d.needs_sync = false;
            }
            // The step command reaches clients before the events its tick
            // generated, as ServerTickRateManager.updateStepTicks does.
            host.hub.sync_ticking(
                self.clock.rate(),
                self.clock.frozen(),
                self.clock.step_state(),
            );
            // `runBlockEvents`' broadcast precedes what the events change (held
            // for the next tick, `tick_world`), and follows the blocks changed
            // before them (`lan_broadcast_changes`): the guest re-runs a
            // piston's `triggerEvent` on the blocks as they were before the
            // move, a base the last tick restored included.
            for (pos, b0, b1, state) in std::mem::take(&mut self.lan_block_events) {
                host.hub.block_event(pos, b0, b1, state);
            }
            let entities = states.as_slice();
            self.lan_prepare_captured_sounds(&mut host, entities);
            // An entity that left the level since the last tick (a snowball
            // that hit) is dropped by the tracker below: its event goes out
            // first, as `Level.broadcastEntityEvent` precedes the discard.
            let (gone, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.lan_entity_events)
                .into_iter()
                .partition(|(id, ..)| !entities.iter().any(|e| e.id == *id));
            self.lan_entity_events = later;
            for (id, event, at) in gone {
                host.hub.entity_event(id, event, at);
            }
            host.hub.far_level(&self.save);
            let spawn = self.world_spawn_position();
            let events = {
                let clock_rate = self.network_clock_rate();
                let mut ctx = TickCtx {
                    world: &mut self.world,
                    entities,
                    next_entity: &mut self.next_entity,
                    spawn,
                    spawn_angle: self.level.world_spawn_angle,
                    difficulty: i32::from(self.ui.difficulty),
                    difficulty_locked: self.level.difficulty_locked,
                    clock_rate,
                    tick_rate: self.clock.rate(),
                    tick_frozen: self.clock.frozen(),
                    tick_steps: self.clock.step_state(),
                    collision: Some(self.collision),
                    border: &self.border,
                    streaming: Some(&self.streaming),
                };
                host.hub.movement_checks = (
                    self.ui.rules.bool("player_movement_check"),
                    self.ui.rules.bool("elytra_movement_check"),
                );
                // IntegratedServer.getScaledTrackingDistance: the Entity Distance
                // option scales what an integrated server or a LAN host tracks (a
                // dedicated one keeps its entity-broadcast-range-percentage).
                if host.hub.config.integrated {
                    host.hub.entity_range_percent =
                        (self.settings.entity_distance * 100.0).round() as i32;
                }
                host.hub
                    .set_immediate_respawn(self.ui.rules.bool("immediate_respawn"));
                host.hub
                    .set_reduced_debug_info(self.ui.rules.bool("reduced_debug_info"));
                host.hub
                    .set_limited_crafting(self.ui.rules.bool("limited_crafting"));
                // A server with several levels runs a pass for each
                // (`tick_server`): the network is read once, before the first.
                // Player packets may already have been drained before
                // scheduled block ticks (`tick_lan_packets`).
                host.hub.mods_loaded = !horizon_client::mod_api::mods().is_empty();
                let pass = self.level_pass;
                let mut events = if pass.first && !pass.packets_drained {
                    host.hub.poll(&mut ctx)
                } else {
                    Vec::new()
                };
                if !pass.packets_drained {
                    events.extend(host.hub.drain_packets(&mut ctx));
                }
                events.extend(horizon_client::spans::time("lan_tick_level", || {
                    host.hub.tick_level(&mut ctx)
                }));
                if pass.last {
                    horizon_client::spans::time("lan_end_tick", || host.hub.end_tick());
                }
                events
            };
            self.wake_woken_guests(&mut host);
            let processed = horizon_client::spans::time("lan_process_events", || {
                self.lan_process_events(host, events, entities, spawn)
            });
            let Some(back) = processed else {
                return;
            };
            host = back;
            host.states = states;
        }
        if !host.quiet.is_empty() {
            self.lan = Some(host);
            self.run_quiet_commands();
            host = self
                .lan
                .take()
                .expect("the hub is back after the quiet commands");
        }
        if self.native_net_waiting() {
            self.lan = Some(host);
            self.run_native_net();
            host = self
                .lan
                .take()
                .expect("the hub is back after the mod messages");
        }
        if !host.hub.native_advancement_grants.is_empty() {
            self.lan = Some(host);
            self.run_native_advancement_grants();
            host = self
                .lan
                .take()
                .expect("the hub is back after the advancement grants");
        }
        if !host.hub.native_menu_events.is_empty() || !host.hub.native_menu_returns.is_empty() {
            self.lan = Some(host);
            self.run_native_menu_events();
            host = self
                .lan
                .take()
                .expect("the hub is back after the menu events");
        }
        host = self.lan_flush_tracking(host);
        self.lan = Some(host);
        // A blast from a guest's attack, and a mace's wind burst.
        self.flush_lan_explosions();
        let _ = self.flush_wind_bursts();
        // A sweep attack's reach to the other players from a guest's attack.
        self.flush_sweeps();
        // A reeled-in hook pulling a player.
        self.flush_hook_pulls();
        // Particles a guest's turn sent (sweep attack, damage indicator, a hook's splashes).
        self.flush_lan_particles();
        let mut host = self
            .lan
            .take()
            .expect("the hub is back after the guest flushes");
        host = self.lan_flush_tracking(host);
        self.lan_flush_effects(&mut host);
        self.lan_living_flags(&mut host);
        self.lan_stuck(&mut host, paused);
        self.tick_server_travel(&mut host);
        self.lan_swings(&mut host);
        self.lan_update_streaming(&host);
        self.lan_guest_poses(&mut host);
        self.lan_update_waypoints(&mut host);
        self.lan_sync_scoreboard(&mut host);
        self.lan_sync_border(&mut host);
        host.hub.anticheat_ping();
        drop(holding);
        self.flush_attachment_sync(&mut host);
        self.flush_native_net(&mut host);
        self.flush_native_player_fx(&mut host);
        self.flush_far_terrain(&mut host);
        lan::flush_all(host.hub.guests.iter().map(|g| &g.sender));
        self.lan = Some(host);
        self.flush_native_sounds();
        // `tick_server` resets the pass after all dimensions; outside a pass
        // the packet gate is per-tick.
        if self.level_pass.running.is_none() {
            self.level_pass.packets_drained = false;
        }
    }

    fn lan_guest_joined(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        entities: &[EntityState],
        spawn: [f64; 3],
    ) {
        let default_mode = host.hub.config.default_game_mode;
        let force = host.hub.config.force_game_mode;
        let root = self.world_root.clone();
        let in_place = host.hub.config.dimension.clone();
        let owner = host.owner.as_mut();
        let Some(g) = host.hub.guest_mut(conn) else {
            return;
        };
        // PlayerList.loadPlayerData: the owner gets the world's player file,
        // everyone else their own.
        let known = match owner.filter(|o| o.uuid == g.uuid) {
            Some(o) => playerdata::load_owner(g, &root, o.saved.take().as_deref()),
            None => playerdata::load(g, &root),
        };
        self.guest_attachments
            .insert(g.uuid, std::mem::take(&mut g.attachments));
        // PlayerEvent.LoadFromFile: the player's data is read and applied,
        // the player not yet placed.
        self.native_player_load_from_file(g.uuid, &g.name, &playerdata::directory(&root));
        // PrepareSpawnTask: with no saved rotation the player faces the world
        // spawn's angle (`/setworldspawn ... <angle>`).
        playerdata::face_world_spawn(g, known, self.level.world_spawn_angle);
        if !known || force {
            g.game_mode = default_mode;
        }
        if !known {
            // ServerPlayer.adjustSpawnLocation: a player new to the world
            // stands where PlayerSpawnFinder puts them, not all on one spot
            // (it reads the loaded level, which holds the spawn when it is
            // the overworld).
            let feet = if self.dim == super::Dim::Overworld {
                self.server_player_spawn(spawn)
            } else {
                spawn
            };
            g.position = feet;
            g.previous_position = feet;
            // Ready.spawn snaps the newcomer to the respawn data's yaw and
            // pitch, which for a player without a bed or anchor is the world
            // spawn's angle (`setworldspawn <pos> <rotation>`).
            (g.yaw, g.pitch) = (
                self.level.world_spawn_angle[0],
                self.level.world_spawn_angle[1],
            );
            g.head_yaw = g.yaw;
            g.health = 20.0;
            g.food = 20;
            g.dimension = super::Dim::Overworld.id().to_owned();
        }
        // Spigot's PlayerSpawnLocationEvent: the mods may place them elsewhere.
        let (mut dimension, mut position, mut rotation) =
            (g.dimension.clone(), g.position, [g.yaw, g.pitch]);
        self.native_player_spawn_location(
            g.uuid,
            &g.name,
            &mut dimension,
            &mut position,
            &mut rotation,
        );
        if dimension != g.dimension && super::Dim::from_id(&dimension).id() == dimension {
            g.dimension = dimension;
        }
        if position != g.position && position.iter().all(|v| v.is_finite()) {
            g.position = position;
            g.previous_position = position;
        }
        if rotation != [g.yaw, g.pitch] && rotation.iter().all(|v| v.is_finite()) {
            (g.yaw, g.pitch) = (rotation[0], rotation[1]);
            g.head_yaw = g.yaw;
        }
        // A server places each player in their own level (`PlayerList.placeNewPlayer`).
        let back = self.dim;
        let level = super::Dim::from_id(&g.dimension);
        let elsewhere = level != back;
        if !elsewhere {
            g.dimension = in_place;
        }
        // A fake player (a mod's bot) has no advancements, statistics or command tree to load or
        // send.
        let fake = host.hub.guest(conn).is_some_and(|g| g.fake);
        if !fake {
            self.load_guest_records(host, conn);
            // The mods say which commands the newcomer's tree keeps.
            self.native_filter_commands(host, Some(conn));
        }
        let mut theirs = States::default();
        let entities = if elsewhere {
            if let Err(e) = self.enter_level(Some(host), level) {
                log::error!("could not open {}: {e}", level.id());
                return;
            }
            // A dedicated server has no host player.
            self.lan_entities(
                &host.mob_players,
                &mut host.hub,
                &mut theirs,
                false,
                0,
                None,
            );
            self.lan_hook_states(&host.play, &host.hub, &mut theirs);
            self.lan_entity_uuids(&mut theirs);
            theirs.as_slice()
        } else {
            entities
        };
        let clock_rate = self.network_clock_rate();
        let mut ctx = TickCtx {
            world: &mut self.world,
            entities,
            next_entity: &mut self.next_entity,
            spawn,
            spawn_angle: self.level.world_spawn_angle,
            difficulty: i32::from(self.ui.difficulty),
            difficulty_locked: self.level.difficulty_locked,
            clock_rate,
            tick_rate: self.clock.rate(),
            tick_frozen: self.clock.frozen(),
            tick_steps: self.clock.step_state(),
            collision: Some(self.collision),
            border: &self.border,
            streaming: Some(&self.streaming),
        };
        host.hub.place_new_player(conn, &mut ctx);
        if elsewhere && let Err(e) = self.enter_level(Some(host), back) {
            log::error!("could not reopen {}: {e}", back.id());
        }
    }

    /// Level events the game queued, for the guests (`process_level_events`).
    pub(super) fn lan_record_events(&mut self, events: &[horizon_world::level::LevelEvent]) {
        if self.audio.capture.is_none() {
            return;
        }
        // The acting guest's own break: `Block.playerWillDestroy` excludes the
        // player from the destroy events up to the broken block's. What a
        // break sets off after that (a torch popping off, a stalk falling) is
        // `destroyBlock` with no player, which the breaker hears and sees too.
        let mut own_break = self.lan_break_pos.is_some();
        for e in events {
            match e {
                horizon_world::level::LevelEvent::LevelEvent { id, pos, data } => {
                    let own = *id == 2001 && own_break;
                    if own && self.lan_break_pos == Some([pos.x, pos.y, pos.z]) {
                        own_break = false;
                    }
                    // The acting guest does not predict these, and the vanilla
                    // call names no excluded player (`levelEvent(id, pos, data)`
                    // passes null): the composter fill (1500), the egg's
                    // teleport, bone meal's growth, anvil result sounds, a
                    // finished brushing or an eye inserted in a portal frame,
                    // a zombie's infection and a finished cure (1026, 1027),
                    // and the digger's own crack and hit sound (2019, 2020).
                    let except = level_event_except(
                        *id,
                        self.lan_acting,
                        *id == 2001 && self.lan_break_pos.is_some() && !own,
                    );
                    self.lan_events
                        .push((*id, [pos.x, pos.y, pos.z], *data, except));
                }
                // `ServerLevel.runBlockEvents` broadcasts a handled block
                // event (`ClientboundBlockEventPacket`): the guest re-runs
                // the block's `triggerEvent` — a piston's `moveBlocks` is
                // what animates the move there. `state` is the block the
                // event named, read before it ran (the packet's `block`).
                horizon_world::level::LevelEvent::BlockEntityEvent { pos, state, a, b } => {
                    self.lan_block_events
                        .push(([pos.x, pos.y, pos.z], *a, *b, state.0));
                }
                _ => {}
            }
        }
    }

    /// `Level.blockEvent` for the guests (`ClientboundBlockEventPacket`).
    pub(super) fn lan_block_event(&mut self, pos: [i32; 3], b0: i32, b1: i32) {
        if self.audio.capture.is_some() {
            let state = self.world.block(pos[0], pos[1], pos[2]).0;
            self.lan_block_events.push((pos, b0, b1, state));
        }
    }
    /// `Level.broadcastEntityEvent` for the guests.
    pub(super) fn lan_entity_event(&mut self, id: i32, event: i8, at: [f64; 3]) {
        if self.audio.capture.is_some() {
            self.lan_entity_events.push((id, event, at));
        }
        // Event 16: zombie villagers play their cure sound on each client.
        if event == 16 && self.options.execution_role.is_client() {
            self.zombie_villager_cure_sound(id as u32);
        }
        // Event 63: a sniffer's digging sound loop starts (ClientPacketListener.handleEntityEvent 63).
        if event == 63 && self.options.execution_role.is_client() {
            self.start_sniffer_digging_sound(id as u32);
        }
        // Event 21: a window that simulates the world itself is its own
        // client, and hears the guardian's hum too.
        if event == 21 && self.options.execution_role.is_client() {
            self.start_guardian_sound(id as u32);
        }
        // Events 53 and 54: honey crumbs while sliding, and the landing burst
        // (`HoneyBlock.showSlideParticles` / `showJumpParticles`), drawn by the
        // client that runs the world.
        if self.options.execution_role.is_client()
            && horizon_client::landing::honey_particle_count(event).is_some()
        {
            self.honey_entity_particles(at, event);
        }
        // Event 2: a mob's spear hit felt; the same window hears its own mob's hit.
        if event == 2 && self.options.execution_role.is_client() {
            self.remote_kinetic_hit(id);
        }
        // Fox.handleEntityEvent 45: the same window draws the eating crumbs.
        if event == 45 && self.options.execution_role.is_client() {
            self.fox_eat_particles(id as u32);
        }
        // Entity event 35: a mob's death protection fired
        // (`LivingEntity.checkTotemDeathProtection`); the clients draw the
        // totem particles and play its sound.
        if event == 35 && self.options.execution_role.is_client() {
            self.remote_totem(id);
        }
    }
    /// `LivingEntity.onEquippedItemBroken` for the acting player (the guest whose turn it
    /// is, else the host): `level.broadcastEntityEvent` with `entityEventForEquipmentBreak`
    /// of the slot, which each client answers with `breakItem` (the sound and the crumbs,
    /// `remote_equipment_break`). Without a hub to tell, or in a window that runs the world
    /// itself, the sound and the crumbs are played here.
    pub(super) fn player_equipment_broke(
        &mut self,
        slot: horizon_world::player_inventory::EquipmentSlot,
        stack: &horizon_client::items::ItemStack,
    ) {
        use horizon_world::player_inventory::EquipmentSlot;
        let event = match slot {
            EquipmentSlot::MainHand => 47,
            EquipmentSlot::OffHand => 48,
            EquipmentSlot::Head => 49,
            EquipmentSlot::Chest => 50,
            EquipmentSlot::Legs => 51,
            EquipmentSlot::Feet => 52,
            EquipmentSlot::Body => 65,
            EquipmentSlot::Saddle => 68,
        };
        // The mods hear a server player's break (a creative player's gear takes no wear).
        if !self.creative
            && let (Some(uuid), Some(name)) = (self.lan_acting_uuid, self.lan_acting_name.clone())
        {
            self.native_player_destroy_item(uuid, &name, &stack.item, equipment_slot_name(slot));
        }
        let Some(at) = self
            .player
            .as_ref()
            .map(|p| [p.position.x, p.position.y, p.position.z])
        else {
            return;
        };
        let told = self.lan_acting.is_some();
        if told {
            self.guest_equipment_breaks.push((event, at));
        }
        if told && !self.options.execution_role.is_client() {
            return;
        }
        // LivingEntity.breakItem: the stack's break sound where it stands, then the crumbs.
        self.break_sound_at(stack, at);
        self.eat_particles(stack, 5);
    }
    /// `ServerPlayer.crit` (or `magicCrit` when `magic`): this window's
    /// emitter, and an animation for the attacker's client and observers once
    /// the hub is back in place.
    pub(super) fn critical_hit(
        &mut self,
        attacker: Option<u32>,
        id: i32,
        feet: [f64; 3],
        width: f64,
        height: f64,
        magic: bool,
    ) {
        use horizon_client::particles::{ParticleOptions, ParticleType};
        let particle = if magic {
            ParticleType::EnchantedHit
        } else {
            ParticleType::Crit
        };
        self.particles
            .tracking_emitter(ParticleOptions::simple(particle), feet, width, height);
        if self.audio.capture.is_some() {
            self.lan_critical_hits.push((attacker, id, magic));
        }
    }
    /// Send the tick's level events, sounds and mob animations.
    fn lan_flush_effects(&mut self, host: &mut LanHost) {
        let critical_hits = std::mem::take(&mut self.lan_critical_hits);
        let kinetic_hits = std::mem::take(&mut self.lan_kinetic_hits);
        for (state, at) in std::mem::take(&mut self.lan_cushion_breaks) {
            if let Some(id) = host
                .hub
                .registry_id("minecraft:particle_type", "minecraft:block")
            {
                host.hub.broadcast_near(
                    at,
                    32.0,
                    &C::LevelParticles(play::LevelParticles {
                        particle: horizon_protocol::entity_data::Particle {
                            id,
                            data: Some(horizon_protocol::entity_data::ParticleData::State(
                                state.0 as i32,
                            )),
                        },
                        override_limiter: false,
                        always_show: false,
                        pos: at,
                        dist: [0.25, 0.0625, 0.25],
                        max_speed: [0.05; 3],
                        count: 10,
                        randomization: 0,
                    }),
                    None,
                );
            }
        }
        for (kind, pos, data, acting) in std::mem::take(&mut self.lan_events) {
            if kind == 2001 {
                host.broken
                    .push([f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2])]);
            }
            host.hub.level_event(kind, pos, data, acting);
        }
        // Level events 1010 and 1011: the guests' parrots dance or stop.
        for (pos, song) in std::mem::take(&mut self.lan_songs) {
            match song {
                Some(name) => {
                    let id = host.hub.registry_id("minecraft:jukebox_song", &name);
                    host.hub.level_event(1010, pos, id.unwrap_or(0), None);
                }
                None => host.hub.level_event(1011, pos, 0, None),
            }
        }
        for (pos, b0, b1, state) in std::mem::take(&mut self.lan_block_events) {
            host.hub.block_event(pos, b0, b1, state);
        }
        for (id, event, at) in std::mem::take(&mut self.lan_entity_events) {
            host.hub.entity_event(id, event, at);
        }
        for (kind, pos, data) in std::mem::take(&mut self.lan_global_events) {
            host.hub.global_level_event(kind, pos, data);
        }
        self.lan_broadcast_effects(host);
        if host.hub.guests.is_empty() {
            host.mob_state.clear();
            return;
        }
        // Hurt and death animations of mobs.
        let mut seen = HashMap::new();
        for m in &self.mobs {
            let dead = !m.alive();
            let old = host
                .mob_state
                .get(&self.dim)
                .and_then(|s| s.get(&m.id))
                .copied();
            let at = [m.position.x, m.position.y, m.position.z];
            // LivingEntity.hurtServer: a hurt that restarted the hurt time.
            if m.hurt_time > old.map_or(0, |o| o.0)
                && !dead
                && let Some((source, _)) = m.last_damage
            {
                use horizon_client::mobs::{DamageKind as K, Who};
                let kind = super::creatures::source_damage_type(source);
                // The player a mob sees is the guest acting.
                let cause = source.attacker.and_then(|who| match who {
                    Who::Player => host
                        .mob_players
                        .get(&m.id)
                        .and_then(|conn| host.hub.guest(*conn))
                        .map(|g| g.entity_id),
                    Who::Mob(id) => Some(id as i32),
                });
                // A projectile is the direct entity of its hit: not tracked here.
                let melee = matches!(
                    source.kind,
                    K::PlayerAttack
                        | K::MaceSmash
                        | K::Spear
                        | K::MobAttack
                        | K::Sting
                        | K::SonicBoom
                );
                let direct = cause.filter(|_| melee);
                host.hub.damage_event(m.id as i32, kind, cause, direct, at);
            }
            if dead && !old.is_some_and(|o| o.1) {
                host.hub.entity_event(m.id as i32, 3, at);
            }
            // LivingEntity.swing on a server (a melee hit, a thrown potion):
            // ClientboundSwingAnimationPacket for the players around. A swing
            // started since the last flush, or restarted (its time went back).
            let swing = m.ai.swinging.then_some(m.ai.swing_time);
            if let Some(time) = swing
                && !dead
                && old.and_then(|o| o.2).is_none_or(|before| time < before)
            {
                host.hub.swing(m.id as i32, 0, at, None);
            }
            seen.insert(m.id, (m.hurt_time, dead, swing));
        }
        if let Some(d) = self.dragon.as_ref().filter(|d| d.id != 0) {
            let id = d.id;
            let dead = !d.alive();
            let old = host
                .mob_state
                .get(&self.dim)
                .and_then(|s| s.get(&id))
                .copied();
            let at = [d.position.x, d.position.y, d.position.z];
            if d.hurt_time > old.map_or(0, |o| o.0) && !dead {
                let (kind, cause, direct) =
                    d.last_source.unwrap_or(("minecraft:generic", None, None));
                host.hub.damage_event(id as i32, kind, cause, direct, at);
            }
            seen.insert(id, (d.hurt_time, dead, None));
        }
        host.mob_state.insert(self.dim, seen);
        // Level.broadcastEntityEvent(player, 2) of a spear's stab.
        for attacker in kinetic_hits {
            let hit = attacker
                .and_then(|conn| host.hub.guest(conn))
                .map(|g| (g.entity_id, g.position));
            if let Some((id, at)) = hit {
                host.hub.entity_event(id, 2, at);
            }
        }
        for (attacker, target, magic) in critical_hits {
            let Some(attacker) =
                attacker.and_then(|conn| host.hub.guest(conn).map(|g| g.entity_id))
            else {
                continue;
            };
            host.hub.critical_hit(attacker, target, magic);
        }
    }

    /// Keep the chunks around every guest loaded.
    fn lan_update_streaming(&mut self, host: &LanHost) {
        // DistanceManager.PlayerTicketTracker: every chunk within the server's
        // view distance (Chebyshev) of a player holds a PLAYER_LOADING ticket
        // at the entity-ticking level (31), whatever the player asked for;
        // levels rise by one a chunk, so FULL (33) reaches two chunks past
        // it. An extra area finishes its chunks out to its radius + 1. That
        // covers what the player tracks (`ChunkTrackingView`, within its view
        // distance + 1) and those chunks' lit neighbours.
        let radius = host.hub.config.view_distance + 1;
        let rule = self.ui.rules.bool("spectators_generate_chunks");
        // The held players are sent the level, so their chunks are kept as the placed ones'.
        self.streaming.extra = host
            .hub
            .loading_chunks(|g| super::game_mode::loads_chunks(rule, g.spectator()))
            .into_iter()
            .map(|chunk| (chunk, radius))
            // Fake players load a small area.
            .chain(
                host.hub
                    .fake_loading_areas(|g| super::game_mode::loads_chunks(rule, g.spectator())),
            )
            .chain(
                self.arrival_chunks(self.dim)
                    .into_iter()
                    // An arrival keeps only its landing spot lit enough to
                    // resolve (the 3x3 `resolve_arrivals` checks), where it
                    // then ticks without a player.
                    .map(|chunk| (chunk, 1)),
            )
            .chain(self.portal_ticket_areas())
            .collect();
    }

    /// The bed at `head` as a sleeping model lies in it (`ints[1]`,
    /// `villager::sleep_pose`): its `facing`.
    /// `Entity.setPose` for the server's players: the mods hear each placed
    /// guest's change of pose. Nothing is kept while no mod is loaded.
    fn lan_guest_poses(&mut self, host: &mut LanHost) {
        if horizon_client::mod_api::mods().is_empty() {
            host.poses.clear();
            return;
        }
        host.poses
            .retain(|conn, _| host.hub.guest(*conn).is_some_and(|g| g.placed));
        let mut changed = Vec::new();
        for g in host.hub.guests.iter().filter(|g| g.placed) {
            let to = g.pose();
            let from = host.poses.insert(g.conn, to).unwrap_or_default();
            if from != to {
                changed.push((g.uuid, g.name.clone(), from, to));
            }
        }
        for (uuid, name, from, to) in changed {
            self.native_entity_pose_change(uuid, &name, from.name(), to.name());
        }
    }

    pub(super) fn bed_facing(&self, head: [i32; 3]) -> i32 {
        let state = self.world.block(head[0], head[1], head[2]);
        let facing = self
            .pack
            .assets
            .registry
            .state(state)
            .and_then(|s| s.properties.get("facing"))
            .map(String::as_str);
        horizon_client::mobs::villager::bed_facing_code(facing)
    }
}

impl Game<'_> {
    /// A hub's chat and log lines: a dedicated server logs them
    /// (MinecraftServer.logChatMessage, `[Not Secure]` for unsigned or
    /// expired messages); a LAN host shows the chat.
    pub(super) fn lan_log_event(&mut self, event: HostEvent) {
        let dedicated = self.options.dedicated.is_some();
        match event {
            HostEvent::Chat { line, secure } if dedicated => {
                let tag = if secure { "" } else { "[Not Secure] " };
                super::dedicated::log("INFO", &format!("{tag}{line}"));
            }
            // Player chat (and `/say`'s or `/me`'s, which are chat messages
            // too): the host's Chat option decides, as `ChatListener` does
            // for the guests' packets. Command feedback is a system message
            // and takes the system-message permission instead.
            HostEvent::Chat { line, .. } => self.player_message(Text::literal(line)),
            HostEvent::Warn(line) if dedicated => super::dedicated::log("WARN", &line),
            HostEvent::Warn(line) => log::warn!("{line}"),
            HostEvent::Error(line) if dedicated => super::dedicated::log("ERROR", &line),
            HostEvent::Error(line) => log::error!("{line}"),
            _ => {}
        }
    }
}

/// `LivingEntity.DATA_LIVING_ENTITY_FLAGS` for an item in use: 1, and 2 when
/// it is the off hand's. A stop is 0 here; vanilla leaves bit 1 set, so its
/// stop of an off-hand use is 2. Both mean "not using" (bit 0 is clear), and
/// nothing reads bit 1 without bit 0.
pub(super) fn living_flags(using: Option<(usize, u32)>) -> i8 {
    using.map_or(0, |(slot, _)| if slot == OFFHAND_SLOT { 3 } else { 1 })
}

/// The name a mod hears for an equipment slot (the armor stand hook's names, plus
/// the two slots a mount's gear uses).
fn equipment_slot_name(slot: horizon_world::player_inventory::EquipmentSlot) -> &'static str {
    use horizon_world::player_inventory::EquipmentSlot;
    match slot {
        EquipmentSlot::MainHand => "mainhand",
        EquipmentSlot::OffHand => "offhand",
        EquipmentSlot::Head => "head",
        EquipmentSlot::Chest => "chest",
        EquipmentSlot::Legs => "legs",
        EquipmentSlot::Feet => "feet",
        EquipmentSlot::Body => "body",
        EquipmentSlot::Saddle => "saddle",
    }
}

/// What a hub event is, whose connection, and the name of a player who left (no longer in the
/// hub once it is handled): for a slow tick's notes.
type EventKind = (&'static str, Option<u32>, Option<String>);

fn event_kind(event: &HostEvent) -> EventKind {
    match event {
        HostEvent::Joined(c) => ("join", Some(*c), None),
        HostEvent::Left(g) => ("leave", None, Some(g.name.clone())),
        HostEvent::Packet(c, p) => (p.name(), Some(*c), None),
        HostEvent::Jump { conn, .. } => ("jump", Some(*conn), None),
        HostEvent::MainHandChanged(c, _) => ("main hand", Some(*c), None),
        HostEvent::LocaleChanged(c, _) => ("locale", Some(*c), None),
        HostEvent::PlayerLoaded(c) => ("loaded", Some(*c), None),
        HostEvent::RecipeBookSettings { conn, .. } => ("recipe book", Some(*conn), None),
        HostEvent::Sneak { conn, .. } => ("sneak", Some(*conn), None),
        HostEvent::Input { conn, .. } => ("input", Some(*conn), None),
        HostEvent::Kicked { name, .. } => ("kick", None, Some(name.clone())),
        HostEvent::Flight { conn, .. } => ("flight", Some(*conn), None),
        HostEvent::Chat { .. } => ("chat line", None, None),
        HostEvent::PlayerChat { .. } => ("chat", None, None),
        HostEvent::ChatFiltered { name, .. } => ("filtered chat", None, Some(name.clone())),
        HostEvent::Warn(_) => ("warning", None, None),
        HostEvent::Error(_) => ("error", None, None),
        HostEvent::Ban { name, .. } => ("ban", None, Some(name.clone())),
    }
}

/// Who `ServerLevel.levelEvent` leaves out of the broadcast.
///
/// `LevelAccessor.levelEvent(int, BlockPos, int)` passes a null source, so the
/// acting player is included. `ComposterBlock.useItemOn` (and the hopper input
/// container) fill that way: event 1500, which the client turns into
/// `handleFill` (the fill particles and `playLocalSound`). A call that names
/// the player still excludes them. `collateral_break` is a later 2001 from
/// `destroyBlock` with no player, after the breaker's own `playerWillDestroy`.
fn level_event_except(id: i32, acting: Option<u32>, collateral_break: bool) -> Option<u32> {
    if matches!(
        id,
        2015 | 1500
            | 1501
            | 1503
            | 1505
            | 1026
            | 1027
            | 1029
            | 1030
            | 1045
            | 1052
            | 2019
            | 2020
            | 3000
            | 3008
    ) || collateral_break
    {
        None
    } else {
        acting
    }
}

/// Notes how long the event that started at `started` took (`spans::note`), with what it
/// was about when [`Game::event_detail`] said.
fn note_event(
    host: &LanHost,
    started: Option<(Instant, EventKind, Option<UseDetail>)>,
    timings: &mut EventTimings<(Option<u32>, Option<String>), UseDetail>,
) {
    let Some((started, kind, detail)) = started else {
        return;
    };
    let ms = started.elapsed().as_secs_f32() * 1000.0;
    horizon_client::spans::note(ms, || describe_event(host, &kind, detail.as_ref()));
    let (name, conn, player) = kind;
    timings.add(name, (conn, player), detail, ms);
}

/// The player an event came from: its name, else its connection.
fn who(host: &LanHost, conn: Option<u32>, name: Option<String>) -> Option<String> {
    name.or_else(|| conn.and_then(|c| host.hub.guest(c)).map(|g| g.name.clone()))
        .or_else(|| conn.map(|c| format!("connection {c}")))
}

/// A burst's note: `213 x UseItemOn under 10 ms each (…); most from Steve: 140 of them, 710.0 ms`.
fn describe_burst(
    host: &LanHost,
    burst: &event_timings::Burst<(Option<u32>, Option<String>), UseDetail>,
) -> String {
    let mut line = format!(
        "{} x {} under {} ms each",
        burst.count,
        burst.kind,
        horizon_client::spans::NOTE_MS
    );
    if let Some(d) = &burst.detail {
        line.push_str(&format!(" ({d})"));
    }
    if let Some(((conn, name), count, ms)) = &burst.top
        && let Some(who) = who(host, *conn, name.clone())
    {
        line.push_str(&format!("; most from {who}: {count} of them, {ms:.1} ms"));
    }
    line
}

fn describe_event(
    host: &LanHost,
    (kind, conn, name): &EventKind,
    detail: Option<&UseDetail>,
) -> String {
    let line = match who(host, *conn, name.clone()) {
        Some(who) => format!("{kind} of {who}"),
        None => (*kind).to_owned(),
    };
    match detail {
        Some(d) => format!("{line} ({d})"),
        None => line,
    }
}

/// What a use packet was about, taken before it ran (the use may consume the item or change
/// the block): the item in the hand it names and, on a block, that block, where, and the face.
#[derive(PartialEq)]
struct UseDetail {
    item: Option<Arc<str>>,
    block: Option<(Arc<str>, [i32; 3], i32)>,
}

impl std::fmt::Display for UseDetail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.item.as_deref().unwrap_or("empty hand"))?;
        if let Some((block, [x, y, z], face)) = &self.block {
            let face = ["down", "up", "north", "south", "west", "east"]
                .get(*face as usize)
                .copied()
                .unwrap_or("?");
            write!(f, " on {block} at {x} {y} {z} {face}")?;
        }
        Ok(())
    }
}

impl Game<'_> {
    /// For a slow tick's note (#8669): the item a `UseItemOn` or `UseItem` uses and the block
    /// it aims at. None for other events.
    fn event_detail(&self, host: &LanHost, event: &HostEvent) -> Option<UseDetail> {
        let HostEvent::Packet(conn, packet) = event else {
            return None;
        };
        let (hand, block) = match &**packet {
            play::Serverbound::UseItemOn(p) => (p.hand, Some((p.pos, p.face))),
            play::Serverbound::UseItem(p) => (p.hand, None),
            _ => return None,
        };
        let guest = host.hub.guest(*conn)?;
        let held = if hand == 1 {
            guest.offhand.as_ref()
        } else {
            guest.inventory.get(guest.selected).and_then(Option::as_ref)
        };
        Some(UseDetail {
            item: held.map(|s| Arc::clone(&s.item)),
            block: block.map(|(pos, face)| (self.block_name(pos), pos, face)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{EventTimings, OFFHAND_SLOT, equipment_slot_name, living_flags, mob_freeze_meta};

    #[test]
    fn a_burst_of_short_events_still_has_a_slow_tick_note() {
        use horizon_client::spans;
        // Thread-local recording avoids changing instrumentation on other test threads.
        spans::record_here(true);
        spans::take_notes();
        let mut timings = EventTimings::new(spans::NOTE_MS);
        for _ in 0..200 {
            spans::note(5.0, || "UseItemOn of fixture player".into());
            timings.add("UseItemOn", "fixture player", None::<()>, 5.0);
        }
        assert!(
            spans::take_notes().is_empty(),
            "every individual event is below NOTE_MS"
        );
        for burst in timings.bursts() {
            spans::note(burst.ms, || format!("{} x {}", burst.count, burst.kind));
        }
        let notes = spans::take_notes();
        spans::record_here(false);
        assert_eq!(notes, vec![(1000.0, "200 x UseItemOn".into())]);
    }
    use horizon_client::mobs::Mob;
    use horizon_protocol::entity_data::{Value, serializer};

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn guest_climbable_glide() {
        use crate::game::{Audio, Options, Progress, Settings, load_resources};
        use horizon_client::lan::{Joined, NetConfig, PlayerSender, guest::Guest};
        use horizon_client::motion::MotionWorld;
        use std::sync::{Arc, atomic::AtomicI32, mpsc};

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
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let res = Box::leak(Box::new(loaded.resources));
        let mut game = super::Game::new(
            res,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        let render = game.pack.assets.reload(&root).unwrap();
        game.world =
            horizon_client::world::World::test_scene(&game.pack.assets, &render, 2).unwrap();

        let sync = Arc::new(horizon_client::lan::data::SyncData::empty());
        let net = NetConfig {
            listen: false,
            ..NetConfig::loopback(0, String::new(), 2)
        };
        let lan = horizon_client::lan::Lan::start_with(sync.clone(), net).unwrap();
        let encoder = horizon_client::lan::chunks::ChunkEncoder::new(
            [] as [(Arc<str>, bool); 0],
            &horizon_protocol::registry::Builtin::default(),
            &[],
            &[],
        );
        let config = horizon_client::lan::hub::HubConfig {
            default_game_mode: 0,
            guest_commands: false,
            force_game_mode: false,
            hardcore: false,
            view_distance: 10,
            simulation_distance: 10,
            dimension: "minecraft:overworld".into(),
            allow_flight: false,
            seed: 0,
            max_players: 20,
            integrated: true,
            levels: Default::default(),
        };
        let mut hub = horizon_client::lan::hub::Hub::new(lan, sync, encoder, config);
        let (tx, _rx) = mpsc::channel();
        let mut guest = Guest::new(
            Joined {
                conn: 1,
                uuid: 1,
                name: "Test".into(),
                properties: Vec::new(),
                info: horizon_protocol::packets::configuration::ClientInformation::new(8),
                brand: String::new(),
                sender: PlayerSender::new(tx, Arc::default()),
                latency: Arc::new(AtomicI32::new(0)),
                address: "127.0.0.1:1".parse().unwrap(),
                expected: false,
                configured: Default::default(),
            },
            1,
            [0.5, 4.0, 0.5],
            8,
        );
        guest.placed = true;
        guest.dimension = "minecraft:overworld".into();
        hub.guests.push(guest);
        let mut host = super::LanHost {
            hub,
            players_told_left: false,
            settings: super::LanSettings {
                port: 0,
                guest_commands: false,
                force_game_mode: false,
            },
            play: Default::default(),
            broken: Default::default(),
            mob_state: Default::default(),
            poses: Default::default(),
            mob_players: Default::default(),
            owner: None,
            waypoints: Default::default(),
            waypoint_dead: Default::default(),
            border_sent: None,
            commands: Default::default(),
            quiet: Default::default(),
            far: None,
            native_commands: None,
            native_command_nodes: Vec::new(),
            native_usages: Vec::new(),
            sleep_ignored: Default::default(),
            states: Default::default(),
            states_next_entity: None,
            idle_states: Default::default(),
        };
        for (block, expected) in [
            ("minecraft:ladder", false),
            ("minecraft:scaffolding", false),
            ("minecraft:vine", true),
        ] {
            let state = game.pack.assets.registry.default_state(block).unwrap();
            game.world.set(0, 4, 0, state).unwrap();
            host.hub.guests[0].fall_flying = true;
            game.as_guest(&mut host, 1, |game| {
                let player = game.player.as_mut().unwrap();
                MotionWorld {
                    entities: &[],
                    player,
                    scene: &mut game.world,
                    assets: &game.pack.assets,
                    collision_binding: game.collision,
                    resources: game.motion,
                    health: &game.health_resources,
                }
                .server_travel()
                .unwrap();
            })
            .unwrap();
            assert_eq!(host.hub.guests[0].fall_flying, expected, "{block}");
        }
    }

    #[test]
    #[ignore = "requires prepared vanilla resources"]
    fn datapack_damage_world_open_reuses_the_no_pack_preload() {
        use super::{SyncCache, SyncData, cached_sync_data, initialize_world_sync_data};
        use horizon_world::loot::PackOverlay;
        use std::{path::Path, sync::Arc};
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        let preloaded = Arc::new(SyncData::load(&root, &PackOverlay::NONE).unwrap());
        let mut cache = SyncCache {
            data: Some((false, Ok(preloaded.clone()))),
            ..Default::default()
        };
        let damage =
            horizon_world::damage_source::Catalog::load(&root, &PackOverlay::NONE).unwrap();
        // A missing root makes an accidental second disk read fail, rather
        // than merely comparing equal snapshots loaded twice.
        let absent = root.join("no-such-resources");
        initialize_world_sync_data(&mut cache, &absent, &PackOverlay::NONE, &damage).unwrap();
        let opened = cached_sync_data(&mut cache, &absent, &PackOverlay::NONE).unwrap();
        assert!(Arc::ptr_eq(&preloaded, &opened));
        assert!(Arc::ptr_eq(
            &opened,
            &cached_sync_data(&mut cache, &absent, &PackOverlay::NONE).unwrap()
        ));
    }

    #[test]
    #[ignore = "requires prepared vanilla resources"]
    fn datapack_damage_world_open_replaces_preload_atomically() {
        use super::{SyncCache, SyncData, cached_sync_data, initialize_world_sync_data};
        use horizon_world::loot::PackOverlay;
        use std::{
            path::Path,
            sync::{Arc, Barrier, Mutex},
        };
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        let preloaded = Arc::new(SyncData::load(&root, &PackOverlay::NONE).unwrap());
        let cache = Arc::new(Mutex::new(SyncCache {
            data: Some((false, Ok(preloaded.clone()))),
            ..Default::default()
        }));
        let barrier = Arc::new(Barrier::new(2));
        let worker = {
            let cache = cache.clone();
            let barrier = barrier.clone();
            let root = root.clone();
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..32 {
                    cached_sync_data(&mut cache.lock().unwrap(), &root, &PackOverlay::NONE)
                        .unwrap();
                }
            })
        };
        let files = vec![(
            "damage_type/demo:world_heat".into(),
            vec![br#"{"message_id":"world_heat","scaling":"never","exhaustion":0.25}"#.to_vec()],
        )];
        barrier.wait();
        let overlay = PackOverlay { files: &files };
        let damage = horizon_world::damage_source::Catalog::load(&root, &overlay).unwrap();
        initialize_world_sync_data(&mut cache.lock().unwrap(), &root, &overlay, &damage).unwrap();
        let opened = cached_sync_data(&mut cache.lock().unwrap(), &root, &overlay).unwrap();
        worker.join().unwrap();
        assert!(!Arc::ptr_eq(&preloaded, &opened));
        let id = opened
            .id("minecraft:damage_type", "demo:world_heat")
            .unwrap();
        let later =
            cached_sync_data(&mut cache.lock().unwrap(), &root, &PackOverlay::NONE).unwrap();
        assert!(
            Arc::ptr_eq(&opened, &later),
            "a late helper or disabled pack cannot replace a frozen world"
        );
        let packet = later.registry_packets(true, &horizon_client::chunk::LevelShapes::VANILLA);
        let packet = packet
            .iter()
            .find(|p| p.registry == "minecraft:damage_type")
            .unwrap();
        assert_eq!(packet.entries[id as usize].id, "demo:world_heat");
        assert!(packet.entries[id as usize].data.is_some());
    }

    #[test]
    #[ignore = "requires prepared vanilla resources"]
    fn datapack_damage_unrelated_packs_do_not_load_sync_data_at_open() {
        use super::{SyncCache, SyncData, initialize_world_sync_data};
        use horizon_world::{damage_source::Catalog, loot::PackOverlay};
        use std::{path::Path, sync::Arc};
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        let damage = Catalog::load(&root, &PackOverlay::NONE).unwrap();
        let preloaded = Arc::new(SyncData::load(&root, &PackOverlay::NONE).unwrap());
        // A recipe-only pack and a damage-tag-only pack must retain main's
        // lazy wire-data path. An absent root proves no full load happens.
        for key in ["recipe/demo:test", "tags/minecraft:damage_type/is_fire"] {
            let files = vec![(key.into(), vec![br#"{"values":[]}"#.to_vec()])];
            let mut cache = SyncCache {
                data: Some((false, Ok(preloaded.clone()))),
                ..Default::default()
            };
            initialize_world_sync_data(
                &mut cache,
                &root.join("absent"),
                &PackOverlay { files: &files },
                &damage,
            )
            .unwrap();
            assert!(cache.data.is_none(), "{key} must not eagerly load SyncData");
            assert!(cache.damage.is_some());
        }
        let mut empty = SyncCache::default();
        initialize_world_sync_data(
            &mut empty,
            &root.join("absent"),
            &PackOverlay::NONE,
            &damage,
        )
        .unwrap();
        assert!(
            empty.data.is_none(),
            "no-pack worlds need no synchronous fallback load either"
        );
    }

    #[test]
    #[ignore = "requires prepared vanilla resources"]
    fn datapack_damage_lazy_wire_snapshot_survives_reload_before_first_hub() {
        use super::{SyncCache, cached_sync_data, initialize_world_sync_data, prepare_sync_reload};
        use horizon_world::{damage_source::Catalog, loot::PackOverlay};
        use std::path::Path;
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        let damage = Catalog::load(&root, &PackOverlay::NONE).unwrap();
        let initial = vec![(
            "dialog/demo:before".into(),
            vec![br#"{"type":"minecraft:notice"}"#.to_vec()],
        )];
        let mut cache = SyncCache::default();
        initialize_world_sync_data(&mut cache, &root, &PackOverlay { files: &initial }, &damage)
            .unwrap();
        assert!(cache.data.is_none());
        // A late vanilla helper cannot install its overlay over this world.
        cached_sync_data(&mut cache, &root, &PackOverlay::NONE).unwrap();
        assert!(cache.data.is_none());
        let changed = vec![
            (
                "damage_type/demo:too_late".into(),
                vec![br#"{"message_id":"too_late","scaling":"never","exhaustion":0.5}"#.to_vec()],
            ),
            (
                "damage_type/minecraft:fall".into(),
                vec![br#"{"message_id":"changed","scaling":"never","exhaustion":0.9}"#.to_vec()],
            ),
            (
                "dialog/demo:after".into(),
                vec![br#"{"type":"minecraft:notice"}"#.to_vec()],
            ),
            (
                "tags/demo:damage_type/reloaded".into(),
                vec![
                    br#"{"values":["minecraft:fall",{"id":"demo:too_late","required":false}]}"#
                        .to_vec(),
                ],
            ),
        ];
        let overlay = PackOverlay { files: &changed };
        assert!(prepare_sync_reload(&mut cache, &overlay).is_none());
        let health = damage.reload_tags(&root, &overlay).unwrap();
        let wire = cached_sync_data(&mut cache, &root, &overlay).unwrap();
        assert!(wire.id("minecraft:damage_type", "demo:too_late").is_none());
        assert!(health.id("demo:too_late").is_none());
        assert!(wire.id("minecraft:dialog", "demo:before").is_none());
        assert!(
            wire.id("minecraft:dialog", "demo:after").is_some(),
            "unrelated registries stay lazy"
        );
        let fall = wire.id("minecraft:damage_type", "minecraft:fall").unwrap();
        let packets = wire.registry_packets(false, &horizon_client::chunk::LevelShapes::VANILLA);
        let packet = packets
            .iter()
            .find(|p| p.registry == "minecraft:damage_type")
            .unwrap();
        let definition = health
            .definition(health.id("minecraft:fall").unwrap())
            .unwrap();
        assert_eq!(
            packet.entries[fall as usize].data.as_ref(),
            Some(&definition.to_nbt())
        );
        let tags = wire
            .tags
            .registries
            .iter()
            .find(|r| r.registry == "minecraft:damage_type")
            .unwrap();
        assert_eq!(
            tags.tags
                .iter()
                .find(|t| t.name == "demo:reloaded")
                .unwrap()
                .ids,
            vec![fall]
        );
        assert_eq!(
            health.tag_members("demo:reloaded").collect::<Vec<_>>(),
            vec!["minecraft:fall"]
        );
        // The next world's initial load can register the previously-late type.
        let next = Catalog::load(&root, &overlay).unwrap();
        let mut next_cache = SyncCache::default();
        initialize_world_sync_data(&mut next_cache, &root, &overlay, &next).unwrap();
        assert!(
            cached_sync_data(&mut next_cache, &root, &overlay)
                .unwrap()
                .id("minecraft:damage_type", "demo:too_late")
                .is_some()
        );
    }

    #[test]
    #[ignore = "requires prepared vanilla resources"]
    fn datapack_damage_lazy_wire_load_caches_after_all_packs_are_removed() {
        use super::{SyncCache, cached_sync_data, initialize_world_sync_data, prepare_sync_reload};
        use horizon_world::{damage_source::Catalog, loot::PackOverlay};
        use std::{path::Path, sync::Arc};
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        let damage = Catalog::load(&root, &PackOverlay::NONE).unwrap();
        let files = vec![(
            "recipe/demo:unrelated".into(),
            vec![br#"{"type":"minecraft:crafting_special_mapcloning"}"#.to_vec()],
        )];
        let mut cache = SyncCache::default();
        initialize_world_sync_data(&mut cache, &root, &PackOverlay { files: &files }, &damage)
            .unwrap();
        assert!(cache.data.is_none());
        assert!(prepare_sync_reload(&mut cache, &PackOverlay::NONE).is_none());
        let opened = cached_sync_data(&mut cache, &root, &PackOverlay::NONE).unwrap();
        // An accidental uncached load would fail on the next request rather
        // than merely reconstructing an equal registry from the same files.
        let reused = cached_sync_data(
            &mut cache,
            &root.join("no-such-resources"),
            &PackOverlay::NONE,
        )
        .unwrap();
        assert!(Arc::ptr_eq(&opened, &reused));
    }

    fn mob_of(kind: &str) -> Option<Mob> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        let types = horizon_client::mobs::MobTypes::load(&root).ok()?;
        let t = std::sync::Arc::new(types.types.get(kind)?.clone());
        Some(Mob::new(
            1,
            t,
            horizon_core::geometry::Vec3::new(0.0, 64.0, 0.0),
            0.0,
        ))
    }

    fn freeze_meta(m: &Mob) -> (Vec<(u8, i32, Value)>, Vec<(u8, i32, Value)>) {
        let (mut meta, mut defaults) = (Vec::new(), Vec::new());
        mob_freeze_meta(m, &mut meta, &mut defaults);
        (meta, defaults)
    }

    /// Entity.setTicksFrozen can go negative; index 7 must carry it. Index 1
    /// (air supply) goes out for every mob, with the species' own default.
    #[test]
    fn negative_frozen_ticks_and_axolotl_air_are_synced() {
        let Some(mut cow) = mob_of("minecraft:cow") else {
            return;
        };
        cow.ticks_frozen = -3;
        let (meta, _) = freeze_meta(&cow);
        assert!(meta.contains(&(7, serializer::INT, Value::Int(-3))));
        cow.air = 100;
        assert!(
            freeze_meta(&cow)
                .0
                .contains(&(1, serializer::INT, Value::Int(100)))
        );

        let Some(mut axolotl) = mob_of("minecraft:axolotl") else {
            return;
        };
        axolotl.air = 5000;
        let (meta, defaults) = freeze_meta(&axolotl);
        assert!(meta.contains(&(1, serializer::INT, Value::Int(5000))));
        assert!(defaults.contains(&(1, serializer::INT, Value::Int(6000))));
        axolotl.air = 6000;
        assert!(freeze_meta(&axolotl).0.iter().all(|e| e.0 != 1));
    }

    /// Raider.IS_CELEBRATING goes out through the tracker's own metadata
    /// (`mob_meta`), not only through the helper: false is listed as a
    /// default so the end of a celebration is sent, and true is sent while
    /// the raider celebrates.
    #[test]
    fn a_celebrating_raider_is_sent_by_mob_meta() {
        let Some(mut pillager) = mob_of("minecraft:pillager") else {
            return;
        };
        let hub = meta_hub("RaiderMetaTest");
        let meta_of = |m: &Mob| {
            let (mut meta, mut defaults) = (Vec::new(), Vec::new());
            super::mob_meta(m, &hub, None, 0, &mut meta, &mut defaults);
            (meta, defaults)
        };

        let (meta, defaults) = meta_of(&pillager);
        assert!(defaults.contains(&(16, serializer::BOOLEAN, Value::Bool(false))));
        assert!(meta.iter().all(|e| e.0 != 16), "{meta:?}");
        horizon_client::mobs::raider::state_mut(&mut pillager).celebrating = true;
        let (meta, _) = meta_of(&pillager);
        assert!(
            meta.contains(&(16, serializer::BOOLEAN, Value::Bool(true))),
            "{meta:?}"
        );
    }

    /// A content mod's living type sends the entity data of its template's
    /// class (`Zombie.DATA_BABY_ID`, the villager's unhappy counter), entry
    /// for entry what its template sends.
    #[test]
    fn a_mod_living_type_sends_its_templates_entity_data() {
        let types = super::super::mods::living_mod_types();
        let hub = meta_hub("ModMobMetaTest");
        let meta_of = |id: &str| {
            let at = horizon_core::geometry::Vec3::new(0.5, 64.0, 0.5);
            let mut m = Mob::new(1, types[id].clone(), at, 0.0);
            m.extra.age = -24000;
            // The same health: the ghoul's own maximum is 40, the zombie's 20.
            m.health = 10.0;
            let (mut meta, mut defaults) = (Vec::new(), Vec::new());
            super::mob_meta(&m, &hub, None, 0, &mut meta, &mut defaults);
            (meta, defaults)
        };
        for (template, id) in [
            ("minecraft:zombie", "ruby:ghoul"),
            ("minecraft:villager", "ruby:settler"),
            ("minecraft:wandering_trader", "ruby:merchant"),
        ] {
            assert_eq!(
                meta_of(id),
                meta_of(template),
                "{id} sends what {template} sends"
            );
        }
    }

    /// A hub with no players and empty registries, for `mob_meta`.
    fn meta_hub(name: &str) -> horizon_client::lan::hub::Hub {
        use horizon_client::lan::{Lan, NetConfig, chunks::ChunkEncoder, data, hub};
        let sync = std::sync::Arc::new(data::SyncData::empty());
        let config = NetConfig {
            announce: false,
            ..NetConfig::loopback(0, name.into(), 2)
        };
        let lan = Lan::start_with(sync.clone(), config).unwrap();
        let encoder = ChunkEncoder::new(
            [] as [(std::sync::Arc<str>, bool); 0],
            &horizon_protocol::registry::Builtin::default(),
            &[],
            &[],
        );
        let hub_config = hub::HubConfig {
            default_game_mode: 0,
            guest_commands: false,
            force_game_mode: false,
            hardcore: false,
            view_distance: 10,
            simulation_distance: 10,
            dimension: "minecraft:overworld".into(),
            allow_flight: false,
            seed: 0,
            max_players: 20,
            integrated: false,
            levels: Default::default(),
        };
        hub::Hub::new(lan, sync, encoder, hub_config)
    }

    /// `LivingEntity.startUsingItem` sets flag 1 for the use and flag 2 for
    /// the off hand, so an off-hand use is 3, not 2. A reader takes bit 0 as
    /// `isUsingItem` and bit 1 as `getUsedItemHand`.
    #[test]
    fn an_off_hand_use_flags_both_the_use_and_the_hand() {
        assert_eq!(living_flags(None), 0);
        assert_eq!(living_flags(Some((0, 12))), 1);
        assert_eq!(living_flags(Some((OFFHAND_SLOT, 12))), 3);
    }

    /// `ComposterBlock.useItemOn` calls `levelEvent(1500, pos, data)`, the
    /// null-source overload, so the player who filled it still receives
    /// `handleFill`. `Block.playerWillDestroy`'s 2001 names that player.
    #[test]
    fn the_composter_fill_reaches_the_player_who_filled_it() {
        let actor = Some(4);
        assert_eq!(super::level_event_except(1500, actor, false), None);
        assert_eq!(super::level_event_except(2001, actor, false), actor);
        assert_eq!(super::level_event_except(2001, actor, true), None);
    }

    /// The names a mod matches on for a broken item's slot (a contract).
    #[test]
    fn a_broken_items_slot_is_named_for_mods() {
        use horizon_world::player_inventory::EquipmentSlot::*;
        let named: Vec<_> = [MainHand, OffHand, Head, Chest, Legs, Feet, Body, Saddle]
            .map(equipment_slot_name)
            .to_vec();
        assert_eq!(
            named,
            [
                "mainhand", "offhand", "head", "chest", "legs", "feet", "body", "saddle"
            ]
        );
    }
}

#[cfg(test)]
mod world_spawn_sync_tests {
    use super::*;
    use horizon_client::lan::{Joined, Out, PlayerSender, guest::Guest};
    use horizon_protocol::packets::play::{self, Clientbound as C};
    use std::sync::{Arc, atomic::AtomicI32, mpsc};

    fn host(game: &mut Game<'_>) -> mpsc::Receiver<Out> {
        game.lan_host(
            LanSettings {
                port: 0,
                guest_commands: true,
                force_game_mode: false,
            },
            NetConfig {
                listen: false,
                announce: false,
                ..NetConfig::loopback(0, String::new(), 2)
            },
            HubConfig {
                default_game_mode: 0,
                guest_commands: true,
                force_game_mode: false,
                hardcore: false,
                view_distance: 2,
                simulation_distance: 2,
                dimension: "minecraft:overworld".into(),
                allow_flight: false,
                seed: 0,
                max_players: 20,
                integrated: true,
                levels: Default::default(),
            },
        )
        .unwrap();
        let (tx, rx) = mpsc::channel();
        let mut guest = Guest::new(
            Joined {
                conn: 1,
                uuid: 1,
                name: "SpawnTest".into(),
                properties: Vec::new(),
                info: horizon_protocol::packets::configuration::ClientInformation::new(2),
                brand: String::new(),
                sender: PlayerSender::new(tx, Arc::default()),
                latency: Arc::new(AtomicI32::new(0)),
                address: "127.0.0.1:1".parse().unwrap(),
                expected: false,
                configured: Default::default(),
            },
            1,
            [0.5, 80.0, 0.5],
            2,
        );
        guest.placed = true;
        guest.dimension = "minecraft:the_nether".into();
        game.lan.as_mut().unwrap().hub.guests.push(guest);
        rx
    }

    fn spawns(rx: &mpsc::Receiver<Out>) -> Vec<play::SetDefaultSpawnPosition> {
        rx.try_iter()
            .flat_map(|out| match out {
                Out::Packet(body) => vec![body],
                Out::Packets(bodies) => bodies,
                Out::Disconnect(_) => panic!("unexpected disconnect"),
            })
            .filter_map(|body| {
                let mut r = horizon_protocol::buf::Reader::new(&body);
                let id = r.varint().unwrap();
                match C::decode(id, r.rest()).unwrap() {
                    C::SetDefaultSpawnPosition(p) => Some(p),
                    _ => None,
                }
            })
            .collect()
    }

    #[test]
    #[ignore = "needs prepared resources and scratch homes/cwd"]
    fn join_and_respawn_contexts_use_world_spawn_not_a_personal_spawn() {
        let mut game = super::super::native_machines::tests::server_game();
        game.level.world_spawn = Some([100, 140, -20]);
        game.spawn = Some(horizon_core::geometry::Vec3::new(96.5, 145.0, -31.5));
        let ctx = game.server_ctx();
        let block = ctx.spawn.map(|v| v.floor() as i32);
        println!(
            "world-spawn\tcontext\tminecraft:overworld\t{}\t{}\t{}",
            block[0], block[1], block[2]
        );
        assert_eq!(ctx.spawn, [100.5, 140.0, -19.5]);
        game.level.world_spawn_angle = [45.0, -10.0];
        assert_eq!(game.server_ctx().spawn_angle, [45.0, -10.0]);
    }

    #[test]
    #[ignore = "needs prepared resources and scratch homes/cwd"]
    fn setworldspawn_reaches_guests_in_other_dimensions_and_keeps_compass_overrides() {
        let mut game = super::super::native_machines::tests::server_game();
        game.build_registries();
        game.level.world_spawn = None;
        let personal = horizon_core::geometry::Vec3::new(96.5, 145.0, -31.5);
        game.spawn = Some(personal);
        for (x, z) in [(6, -2), (-1, 0)] {
            game.world
                .chunks
                .insert((x, z), Arc::new(horizon_client::chunk::Chunk::empty(x, z)));
        }
        let rx = host(&mut game);
        // Materializing the effective fallback does not change the packet data.
        game.as_console(|game| assert!(game.run_command("setworldspawn 0 80 0")));
        assert!(spawns(&rx).is_empty());
        game.as_console(|game| assert!(game.run_command("setworldspawn 100 140 -20")));
        assert_eq!(
            game.spawn,
            Some(personal),
            "a personal respawn point stays personal"
        );
        let sent = spawns(&rx);
        assert_eq!(
            sent.len(),
            1,
            "a placed Nether guest must hear the world spawn change"
        );
        assert_eq!(sent[0].pos.dimension, "minecraft:overworld");
        assert_eq!(sent[0].pos.pos, [100, 140, -20]);
        println!(
            "world-spawn\tchanged\t{}\t{}\t{}\t{}",
            sent[0].pos.dimension, sent[0].pos.pos[0], sent[0].pos.pos[1], sent[0].pos.pos[2]
        );
        game.as_console(|game| assert!(game.run_command("setworldspawn 100 140 -20")));
        assert!(
            spawns(&rx).is_empty(),
            "unchanged respawn data is not broadcast again"
        );
        let target = play::GlobalPos {
            dimension: "minecraft:the_end".into(),
            pos: [9, 70, -4],
        };
        game.lan
            .as_mut()
            .unwrap()
            .hub
            .compass_targets
            .insert(1, target.clone());
        game.as_console(|game| assert!(game.run_command("setworldspawn -7 90 12")));
        let sent = spawns(&rx);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].pos.dimension, target.dimension);
        assert_eq!(sent[0].pos.pos, target.pos);
        println!(
            "world-spawn\toverride\t{}\t{}\t{}\t{}",
            sent[0].pos.dimension, sent[0].pos.pos[0], sent[0].pos.pos[1], sent[0].pos.pos[2]
        );
        game.lan.as_mut().unwrap().hub.compass_targets.clear();
        // Rotation alone is a changed RespawnData, even at the same block.
        game.as_console(|game| assert!(game.run_command("setworldspawn -7 90 12 45 -10")));
        let sent = spawns(&rx);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].pos.pos, [-7, 90, 12]);
        assert_eq!([sent[0].yaw, sent[0].pitch], [45.0, -10.0]);
        println!(
            "world-spawn\tangle\t{}\t{}\t{}\t{}\t{:.1}\t{:.1}",
            sent[0].pos.dimension,
            sent[0].pos.pos[0],
            sent[0].pos.pos[1],
            sent[0].pos.pos[2],
            sent[0].yaw,
            sent[0].pitch
        );
        game.as_console(|game| assert!(game.run_command("setworldspawn -7 90 12 45 -10")));
        assert!(spawns(&rx).is_empty());
        // RespawnData.of wraps yaw and clamps pitch before storing or comparing.
        game.as_console(|game| assert!(game.run_command("setworldspawn -7 90 12 405 100")));
        let sent = spawns(&rx);
        assert_eq!(sent.len(), 1);
        assert_eq!(game.level.world_spawn_angle, [45.0, 90.0]);
        assert_eq!([sent[0].yaw, sent[0].pitch], [45.0, 90.0]);
        println!(
            "world-spawn\tnormalized\t{}\t{}\t{}\t{}\t{:.1}\t{:.1}",
            sent[0].pos.dimension,
            sent[0].pos.pos[0],
            sent[0].pos.pos[1],
            sent[0].pos.pos[2],
            sent[0].yaw,
            sent[0].pitch
        );
        for command in [
            "setworldspawn -7 90 12 45 90",
            "setworldspawn -7 90 12 405 100",
        ] {
            game.as_console(|game| assert!(game.run_command(command)));
            assert!(
                spawns(&rx).is_empty(),
                "equivalent normalized rotations are unchanged"
            );
        }
        // Clamp raw pitch: wrapping -270 first would incorrectly produce +90.
        game.as_console(|game| assert!(game.run_command("setworldspawn -7 90 12 405 -270")));
        let sent = spawns(&rx);
        assert_eq!(sent.len(), 1);
        assert_eq!(game.level.world_spawn_angle, [45.0, -90.0]);
        assert_eq!([sent[0].yaw, sent[0].pitch], [45.0, -90.0]);
        println!(
            "world-spawn\tclamped\t{}\t{}\t{}\t{}\t{:.1}\t{:.1}",
            sent[0].pos.dimension,
            sent[0].pos.pos[0],
            sent[0].pos.pos[1],
            sent[0].pos.pos[2],
            sent[0].yaw,
            sent[0].pitch
        );
    }

    #[test]
    #[ignore = "needs prepared resources and scratch homes/cwd"]
    fn guest_respawn_without_a_known_world_spawn_never_advertises_the_hosts_personal_point() {
        let mut game = super::super::native_machines::tests::server_game();
        game.level.world_spawn = None;
        game.level.world_spawn_angle = [45.0, -10.0];
        let host_personal = horizon_core::geometry::Vec3::new(96.5, 145.0, -31.5);
        let guest_personal = [5.5, 81.0, 5.5];
        game.spawn = Some(host_personal);
        let rx = host(&mut game);
        let mut host = game.lan.take().unwrap();
        let guest = &mut host.hub.guests[0];
        guest.dimension = "minecraft:overworld".into();
        guest.spawn = Some(guest_personal);
        guest.health = 0.0;
        guest.dead = true;
        guest.loaded = true;
        game.guest_respawn(&mut host, 1);
        let guest = &host.hub.guests[0];
        assert_eq!(
            guest.position, guest_personal,
            "the guest's landing stays personal"
        );
        assert!(!guest.dead);
        assert_eq!(game.spawn, Some(host_personal));
        let sent = spawns(&rx);
        assert_eq!(sent.len(), 2, "respawn and level info both send world data");
        for point in &sent {
            assert_eq!(point.pos.dimension, "minecraft:overworld");
            assert_eq!(
                point.pos.pos,
                [0, 80, 0],
                "the host's personal point is not world data"
            );
            assert_eq!([point.yaw, point.pitch], [45.0, -10.0]);
        }
        println!(
            "world-spawn\trespawn\t{}\t{}\t{}\t{}\t{:.1}\t{:.1}",
            sent[0].pos.dimension,
            sent[0].pos.pos[0],
            sent[0].pos.pos[1],
            sent[0].pos.pos[2],
            sent[0].yaw,
            sent[0].pitch
        );
    }
}
