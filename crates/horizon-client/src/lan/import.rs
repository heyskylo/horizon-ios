//! `player-import`: a player's record, as `player-export` made it on another
//! server (or the same one), applied to the held player that has just logged in.
//!
//! The record can be applied whole or in parts. A part left out keeps what the
//! held player has: the state this server loaded for it when it logged in (its own
//! file, or a new player's defaults). The parts are named as the operator of a proxy
//! names them, and each is a set of the guest's fields:
//!
//! | part | what it sets |
//! | --- | --- |
//! | `inventory` | the inventory, armour, off hand and the selected slot |
//! | `ender_chest` | the ender chest |
//! | `experience` | the level, the progress, the total and the seed |
//! | `health` | health and absorption, food, saturation, exhaustion and its timer, and the body's air, fire and freezing |
//! | `effects` | the active effects |
//! | `game_mode` | the game mode, the flight and its speed |
//! | `position` | where it stands, how it looks, whether it is on the ground, its fall and motion, the vehicle it rode |
//! | `spawn_point` | the respawn point |
//! | `recipe_book` | the recipe book |
//! | `advancements` | the advancements and the statistics (the caller applies them: they need the game's tree and counters) |
//!
//! What the record holds that no part names (the compound's keys Horizon does not
//! model) is taken when every part is.
use super::{
    guest::{Guest, SavedBody},
    hub::Hub,
    playerdata,
};
use crate::save::Level;
use horizon_world::nbt::{Limits, Tag, read_named};
use serde_json::Value;

/// The parts of a record, in the order the replies list them.
pub const PARTS: [&str; 10] = [
    "inventory",
    "ender_chest",
    "experience",
    "health",
    "effects",
    "game_mode",
    "position",
    "spawn_point",
    "recipe_book",
    "advancements",
];

/// What an import did, for the caller to finish.
#[derive(Debug, PartialEq)]
pub struct Imported {
    /// The parts that were applied, in [`PARTS`] order.
    pub parts: Vec<&'static str>,
    /// The advancements and the statistics to give the player, when the advancements part was
    /// asked for: they need the game's tree and its counters.
    pub advancements: Option<Value>,
    pub stats: Option<Value>,
    /// Whether the mods' attachments came (a whole import): the game's own copy of them is then
    /// the record's.
    pub attachments: bool,
    /// Whether the other players are told the player has arrived (when it is placed).
    pub announce: bool,
    /// Whether the request asks the player to be let into the world now. The import never places
    /// it itself: the game writes the player's file first and places it then, as for
    /// `player-confirm`. Until then it stays held, and `player-drop` removes it.
    pub place: bool,
    /// The record's hand-over.
    pub handover: u128,
}

/// What a request of `player-import` asks: the record, the parts (all by default)
/// and whether the world is told (yes by default).
struct Request<'a> {
    record: &'a Value,
    parts: Vec<&'static str>,
    announce: bool,
    place: bool,
}

fn read_request(request: &Value) -> Result<Request<'_>, String> {
    let record = request
        .get("record")
        .filter(|r| r.is_object())
        .ok_or("the request needs a record")?;
    let parts = match request.get("parts") {
        None | Some(Value::Null) => PARTS.to_vec(),
        Some(Value::Array(names)) => {
            let mut parts = Vec::new();
            for name in names {
                let name = name.as_str().ok_or("a part is a name")?;
                let part = PARTS
                    .iter()
                    .find(|p| **p == name)
                    .ok_or_else(|| format!("{name:?} is not a part of a record"))?;
                if !parts.contains(part) {
                    parts.push(*part);
                }
            }
            // Always in the order of PARTS, whatever order they were asked in.
            PARTS.into_iter().filter(|p| parts.contains(p)).collect()
        }
        Some(_) => return Err("parts is a list of names".into()),
    };
    let announce = match request.get("announce") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("announce is true or false".into()),
    };
    let place = match request.get("place") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("place is true or false".into()),
    };
    Ok(Request {
        record,
        parts,
        announce,
        place,
    })
}

/// Reads the player compound and the hand-over id out of a record, after checking the record
/// is one this server can take for `uuid`.
fn read_compound(record: &Value, uuid: u128) -> Result<(Tag, u128), String> {
    match record.get("version").and_then(Value::as_i64) {
        Some(super::arrivals::RECORD_VERSION) => {}
        Some(other) => return Err(format!("record version {other} is not understood")),
        None => return Err("the record has no version".into()),
    }
    let theirs = record.get("data_version").and_then(Value::as_i64);
    if theirs != Some(i64::from(horizon_world::DATA_VERSION)) {
        return Err(format!(
            "the record is of data version {theirs:?}, this server's is {}",
            horizon_world::DATA_VERSION
        ));
    }
    let named = record.get("uuid").and_then(Value::as_str);
    if named.and_then(super::arrivals::parse_uuid) != Some(uuid) {
        return Err("the record is another player's".into());
    }
    let Some(handover) = record
        .get("handover")
        .and_then(Value::as_str)
        .and_then(super::arrivals::parse_handover)
    else {
        return Err("the record has no hand-over id: export it with freeze".into());
    };
    let text = record
        .get("player")
        .and_then(Value::as_str)
        .ok_or("the record has no player compound")?;
    let bytes = crate::skins::base64_decode(text).ok_or("the player compound is not base64")?;
    let (named, _) = read_named(&bytes, Limits::saved())
        .map_err(|e| format!("the player compound is not NBT: {e}"))?;
    match named.tag {
        tag @ Tag::Compound(_) => Ok((tag, handover)),
        _ => Err("the player compound is not a compound".into()),
    }
}

/// The farthest a position may be from the origin on x and z, and on y: what vanilla's
/// `Entity.load` clamps a saved position to (`Mth.clamp(x, -3.0000512E7, 3.0000512E7)`, y
/// to +-2.0E7).
const MAX_XZ: f64 = 3.000_051_2e7;
const MAX_Y: f64 = 2.0e7;

/// Refuses a compound whose values no player can have. `Entity.load` throws on a position or
/// rotation that is not finite; a record comes from another machine, so it is checked rather
/// than clamped, and a health of zero or less (a dead player) is refused too.
fn check_values(level: &Level) -> Result<(), String> {
    let finite = |what: &str, values: &[f64]| {
        if values.iter().all(|v| v.is_finite()) {
            Ok(())
        } else {
            Err(format!("the record's {what} is not a number"))
        }
    };
    if let Some(p) = level.position {
        finite("position", &p)?;
        if p[0].abs() > MAX_XZ || p[2].abs() > MAX_XZ || p[1].abs() > MAX_Y {
            return Err("the record's position is out of the world".into());
        }
    }
    finite(
        "rotation",
        &[f64::from(level.rotation[0]), f64::from(level.rotation[1])],
    )?;
    finite("motion", &level.motion)?;
    finite(
        "health",
        &[
            f64::from(level.health),
            f64::from(level.absorption),
            f64::from(level.saturation),
            f64::from(level.xp_progress),
            level.fall_distance,
        ],
    )?;
    if level.health <= 0.0 {
        return Err("the record's player is dead".into());
    }
    // The bounds vanilla's own data keeps (26.3's bytecode): `max_health` is at most 1024
    // (`Attributes.MAX_HEALTH`) and absorption at most 2048 (`MAX_ABSORPTION`), an effect's
    // amplifier is 0 to 255 (`MobEffectInstance` clamps it), and food exhaustion is at most 40
    // (`FoodData.addExhaustion`). A stack's count needs nothing here: `item_from_nbt` takes one out of
    // `ItemStack`'s `intRange(1, 99)` as 1.
    if level.health > 1024.0 || !(0.0..=2048.0).contains(&level.absorption) {
        return Err("the record's health is past what a player can have".into());
    }
    if !(0.0..=40.0).contains(&level.food_exhaustion) {
        return Err("the record's food exhaustion is past what a player can have".into());
    }
    if level
        .active_effects
        .iter()
        .any(|(_, _, amplifier, _, _)| !(0..=255).contains(amplifier))
    {
        return Err("the record holds an effect no player can have".into());
    }
    Ok(())
}

/// What the held player has now, for the parts that are not imported.
struct Kept {
    inventory: Option<(
        Vec<Option<crate::items::ItemStack>>,
        [Option<crate::items::ItemStack>; 4],
        Option<crate::items::ItemStack>,
        usize,
    )>,
    ender: Option<Vec<Option<crate::items::ItemStack>>>,
    experience: Option<(i32, f32, i32, i32)>,
    health: Option<(f32, Option<f32>, i32, f32)>,
    effects: Option<crate::effects::PlayerEffects>,
    game_mode: Option<(i32, bool, f32)>,
    position: Option<([f64; 3], [f64; 3], f32, f32, f32, bool, Option<[i32; 4]>)>,
    spawn: Option<(Option<[f64; 3]>, Option<(String, [i32; 3], f32, bool)>)>,
    recipes: Option<crate::recipe_book::Settings>,
    /// The body's values: its fall goes with the position, its air and fire with the
    /// health.
    body: Option<SavedBody>,
    other: Option<(
        Option<Tag>,
        Option<(String, [i32; 3])>,
        bool,
        Vec<String>,
        horizon_world::attachments::Attachments,
    )>,
    dimension: String,
}

impl Kept {
    fn of(g: &Guest, parts: &[&str]) -> Self {
        let has = |p: &str| parts.contains(&p);
        let all = parts.len() == PARTS.len();
        Kept {
            inventory: (!has("inventory")).then(|| {
                (
                    g.inventory.clone(),
                    g.armor.clone(),
                    g.offhand.clone(),
                    g.selected,
                )
            }),
            ender: (!has("ender_chest")).then(|| g.ender_items.clone()),
            experience: (!has("experience")).then_some((
                g.xp_level,
                g.xp_progress,
                g.xp_total,
                g.xp_seed,
            )),
            health: (!has("health")).then_some((
                g.health,
                g.saved_absorption,
                g.food,
                g.saturation,
            )),
            effects: (!has("effects")).then(|| g.effects.clone()),
            game_mode: (!has("game_mode")).then_some((g.game_mode, g.flying, g.flying_speed)),
            position: (!has("position")).then_some((
                g.position,
                g.previous_position,
                g.yaw,
                g.pitch,
                g.head_yaw,
                g.on_ground,
                g.saved_vehicle,
            )),
            spawn: (!has("spawn_point")).then(|| (g.spawn, g.respawn_elsewhere.clone())),
            recipes: (!has("recipe_book")).then(|| g.recipe_book.clone()),
            body: g.loaded_body,
            other: (!all).then(|| {
                (
                    g.saved.clone(),
                    g.last_death.clone(),
                    g.seen_credits,
                    g.post_effects.clone(),
                    g.attachments.clone(),
                )
            }),
            dimension: g.dimension.clone(),
        }
    }

    /// Puts back what the import must not have changed.
    fn restore(self, g: &mut Guest, parts: &[&str]) {
        let has = |p: &str| parts.contains(&p);
        if let Some((inventory, armor, offhand, selected)) = self.inventory {
            (g.inventory, g.armor, g.offhand, g.selected) = (inventory, armor, offhand, selected);
        }
        if let Some(ender) = self.ender {
            g.ender_items = ender;
        }
        if let Some((level, progress, total, seed)) = self.experience {
            (g.xp_level, g.xp_progress, g.xp_total, g.xp_seed) = (level, progress, total, seed);
        }
        if let Some((health, absorption, food, saturation)) = self.health {
            (g.health, g.saved_absorption, g.food, g.saturation) =
                (health, absorption, food, saturation);
        }
        if let Some(effects) = self.effects {
            g.effects = effects;
        }
        if let Some((mode, flying, speed)) = self.game_mode {
            (g.game_mode, g.flying, g.flying_speed) = (mode, flying, speed);
        }
        if let Some((position, previous, yaw, pitch, head, ground, vehicle)) = self.position {
            (g.position, g.previous_position) = (position, previous);
            (g.yaw, g.pitch, g.head_yaw, g.on_ground) = (yaw, pitch, head, ground);
            // The vehicle the player rode goes with where it stands.
            g.saved_vehicle = vehicle;
        }
        if let Some((spawn, elsewhere)) = self.spawn {
            (g.spawn, g.respawn_elsewhere) = (spawn, elsewhere);
        }
        if let Some(recipes) = self.recipes {
            g.recipe_book = recipes;
        }
        // The body mixes the two: its fall and motion go with the position, its air, fire,
        // freezing and food exhaustion and timer with the health. What a part does not take
        // is put back value by value.
        match (self.body, g.loaded_body.as_mut()) {
            (Some(mine), Some(imported)) => {
                if !has("position") {
                    imported.fall_distance = mine.fall_distance;
                    imported.velocity = mine.velocity;
                }
                if !has("health") {
                    imported.fire_ticks = mine.fire_ticks;
                    imported.air = mine.air;
                    imported.ticks_frozen = mine.ticks_frozen;
                    imported.food_exhaustion = mine.food_exhaustion;
                    imported.food_tick_timer = mine.food_tick_timer;
                }
            }
            (Some(mine), None) => g.loaded_body = Some(mine),
            (None, _) => {}
        }
        if let Some((saved, death, credits, post, attachments)) = self.other {
            (
                g.saved,
                g.last_death,
                g.seen_credits,
                g.post_effects,
                g.attachments,
            ) = (saved, death, credits, post, attachments);
        }
        // The client is in one dimension, and only a login can change it.
        g.dimension = self.dimension;
    }
}

/// Applies the request to the held player `uuid` and tells its client, leaving it held
/// (`Hub::confirm_arrival` places it). The caller gives the player its advancements and
/// statistics (`Imported::advancements`, `Imported::stats`) and its attachments.
///
/// Nothing changes when it refuses: the record is read and checked first, and the
/// guest is touched only once everything is known to be well.
pub fn import_player(hub: &mut Hub, uuid: u128, request: &Value) -> Result<Imported, String> {
    let request = read_request(request)?;
    let (tag, handover) = read_compound(request.record, uuid)?;
    let Some(i) = hub
        .guests
        .iter()
        .position(|g| g.uuid == uuid && g.held && !g.placed)
    else {
        return Err("no player with that unique id is held".into());
    };
    if !hub.guests[i].left_reason.is_empty() {
        return Err("that player is leaving".into());
    }
    if hub.guests[i]
        .arrival_handover
        .is_some_and(|theirs| theirs != handover)
    {
        return Err("that player's record of another hand-over was imported".into());
    }
    let mut level = Level::default();
    level.read_player(&tag);
    check_values(&level)?;
    // What `playerdata::load` gives `apply` too: the whole compound, for the keys Horizon
    // does not model, the mods' attachments and the body's values.
    level.player_extra = Some(tag);
    // A position in another dimension than the client's cannot be taken: the client
    // would have to change its world, and only a login does that.
    if request.parts.contains(&"position") {
        let theirs = playerdata_dimension(&level);
        if theirs != hub.guests[i].dimension {
            return Err(format!(
                "the record is in {theirs}, the held player in {}: the position cannot be taken",
                hub.guests[i].dimension
            ));
        }
    }
    let kept = Kept::of(&hub.guests[i], &request.parts);
    // `apply` fills the slots a record holds and leaves the others as they are, which is right
    // for a player loaded into an empty guest and wrong here: the held player holds what its
    // own file gave it on this server, and an imported inventory REPLACES that, a slot the
    // record leaves empty being empty afterwards.
    {
        let g = &mut hub.guests[i];
        if request.parts.contains(&"inventory") {
            g.inventory.iter_mut().for_each(|slot| *slot = None);
            g.armor = Default::default();
            g.offhand = None;
        }
        if request.parts.contains(&"ender_chest") {
            g.ender_items.iter_mut().for_each(|slot| *slot = None);
        }
    }
    playerdata::apply(&mut hub.guests[i], &level);
    kept.restore(&mut hub.guests[i], &request.parts);
    // Everything the import changed reaches the client now, so that a proxy sees the
    // player as it will be; the player stays held until it is placed or dropped.
    hub.send_arrival_state(i);
    let g = &mut hub.guests[i];
    g.imported = true;
    g.arrival_announce = request.announce;
    g.arrival_handover = Some(handover);
    let records = |key: &str| {
        request
            .parts
            .contains(&"advancements")
            .then(|| request.record.get(key).cloned())
            .flatten()
            .filter(|a| !a.is_null())
    };
    Ok(Imported {
        advancements: records("advancements"),
        stats: records("stats"),
        attachments: request.parts.len() == PARTS.len(),
        parts: request.parts,
        announce: request.announce,
        place: request.place,
        handover,
    })
}

/// The dimension a record names, the way `playerdata::apply` places it.
fn playerdata_dimension(level: &Level) -> String {
    match level.dimension.as_str() {
        d @ ("minecraft:the_nether" | "minecraft:the_end") => d.to_owned(),
        _ => "minecraft:overworld".to_owned(),
    }
}
