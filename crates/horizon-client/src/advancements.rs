//! Advancements: the tree loaded from `data/*/advancement`, the player's
//! progress (`PlayerAdvancements`) and the predicates criteria test
//! (`EntityPredicate`, `ItemPredicate`, `LocationPredicate`, the loot
//! conditions advancements use).
use horizon_world::lazy_json::LazyJson;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    Task,
    Goal,
    Challenge,
}
#[derive(Clone, Debug)]
pub struct Display {
    pub icon: String,
    pub title: Value,
    pub description: Value,
    pub frame: Frame,
    pub background: Option<String>,
    pub show_toast: bool,
    pub announce: bool,
    pub hidden: bool,
}
#[derive(Clone, Debug)]
pub struct Criterion {
    pub name: String,
    pub trigger: String,
    /// Read on first test: ~1,300 recipe advancements' criteria, most never
    /// tested in a game, took ~4 MB as `Value`s.
    pub conditions: LazyJson,
}
#[derive(Clone, Debug, Default)]
pub struct Rewards {
    pub recipes: Vec<String>,
    pub experience: i32,
    pub loot: Vec<String>,
    pub function: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Advancement {
    pub id: String,
    pub parent: Option<String>,
    pub display: Option<Display>,
    pub criteria: Vec<Criterion>,
    pub requirements: Vec<Vec<String>>,
    pub rewards: Rewards,
}
/// Every advancement, indexed by id and by trigger.
#[derive(Default)]
pub struct Tree {
    pub list: Vec<Advancement>,
    pub by_id: HashMap<String, usize>,
    /// trigger id -> (advancement index, criterion index).
    pub by_trigger: HashMap<String, Vec<(usize, usize)>>,
    /// `recipe_unlocked` criteria by the recipe they name (`index_recipes`);
    /// those naming none in `recipes_any`.
    by_recipe: HashMap<String, Vec<(usize, usize)>>,
    recipes_any: Vec<(usize, usize)>,
    /// The triggers with a criterion that may test what the player looks at
    /// (`index_looking_at`).
    looking_at_triggers: BTreeSet<String>,
    /// The triggers with a criterion that may test an entity's `nbt`
    /// (`index_nbt`).
    nbt_triggers: BTreeSet<String>,
    /// What the criteria of each trigger may ask of the player's level,
    /// statistics, advancements and recipes (`index_player_facts`).
    player_fact_triggers: HashMap<String, FactsAsked>,
}
/// Which of `PlayerPredicate`'s level, `stats`, `advancements` and `recipes`
/// a trigger's criteria may test: only those are filled in its player view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FactsAsked {
    pub level: bool,
    pub stats: bool,
    pub advancements: bool,
    pub recipes: bool,
}
impl FactsAsked {
    pub const ALL: Self = Self {
        level: true,
        stats: true,
        advancements: true,
        recipes: true,
    };
    /// Asked by either.
    pub fn or(self, other: Self) -> Self {
        Self {
            level: self.level || other.level,
            stats: self.stats || other.stats,
            advancements: self.advancements || other.advancements,
            recipes: self.recipes || other.recipes,
        }
    }
}
/// What `v` (a predicate, a list of conditions or an item modifier) may ask
/// of the player's level, statistics, advancements and recipes: the fields
/// of its player predicates, and all four for a `minecraft:reference` to a
/// predicate file. Walked without reading anything else, for the commands
/// that test a predicate on the player.
pub fn facts_asked_in(v: &Value) -> FactsAsked {
    let mut asked = FactsAsked::default();
    let mut todo = vec![v];
    while let Some(v) = todo.pop() {
        match v {
            Value::Array(list) => todo.extend(list),
            Value::Object(o) => {
                // A predicate file, or (`function`) an item modifier file.
                let kind =
                    (o.get("condition").or_else(|| o.get("type"))).or_else(|| o.get("function"));
                if kind
                    .and_then(Value::as_str)
                    .is_some_and(|k| full(k) == "minecraft:reference")
                {
                    return FactsAsked::ALL;
                }
                for (key, value) in o {
                    if full(key) == "minecraft:type_specific/player"
                        && let Some(p) = value.as_object()
                    {
                        asked = asked.or(FactsAsked {
                            level: p.contains_key("level"),
                            stats: p.contains_key("stats"),
                            advancements: p.contains_key("advancements"),
                            recipes: p.contains_key("recipes"),
                        });
                    }
                    todo.push(value);
                }
            }
            _ => {}
        }
    }
    asked
}
fn frame(v: &Value) -> Frame {
    match v.as_str() {
        Some("goal") => Frame::Goal,
        Some("challenge") => Frame::Challenge,
        _ => Frame::Task,
    }
}
impl Tree {
    /// `PlayerAdvancements.checkForAutomaticTriggers`: the advancements that
    /// have no criteria at all, so nothing can ever grant them a criterion and
    /// vanilla hands them over (and grants their rewards) as the player's
    /// progress loads.
    pub fn automatic(&self) -> Vec<usize> {
        (0..self.list.len())
            .filter(|&i| self.list[i].criteria.is_empty())
            .collect()
    }

    /// [`Tree::load`], shared while anyone holds it: a server's advancement
    /// packets (`ServerAdvancements`) and its game's criteria read the same
    /// files, and each kept its own copy (7 MB). A world whose data packs bring
    /// advancements gets its own tree ([`Tree::load_with`]).
    pub fn shared(data: &Path, overlay: &horizon_world::loot::PackOverlay) -> Arc<Tree> {
        if overlay.kind("advancement").next().is_some() {
            return Arc::new(Tree::load_with(data, overlay));
        }
        static TREES: Mutex<Vec<(PathBuf, Weak<Tree>)>> = Mutex::new(Vec::new());
        // Held while loading: a second reader waits for the first one's tree.
        let mut trees = TREES.lock().unwrap_or_else(|e| e.into_inner());
        trees.retain(|(_, tree)| tree.strong_count() > 0);
        if let Some(tree) = trees
            .iter()
            .find(|(path, _)| path == data)
            .and_then(|(_, tree)| tree.upgrade())
        {
            return tree;
        }
        let tree = Arc::new(Tree::load(data));
        trees.push((data.to_path_buf(), Arc::downgrade(&tree)));
        tree
    }
    /// Loads `<data>/<namespace>/advancement/**/*.json`.
    pub fn load(data: &Path) -> Tree {
        Self::load_with(data, &horizon_world::loot::PackOverlay::NONE)
    }
    /// The tree with a world's data packs' advancement files over the game's
    /// own: the top pack's file for each id replaces it.
    pub fn load_with(data: &Path, overlay: &horizon_world::loot::PackOverlay) -> Tree {
        let mut tree = Tree::default();
        let Ok(namespaces) = std::fs::read_dir(data) else {
            return tree;
        };
        // One JSON per id, the packs' over the game's own. Through `json_folder`: from
        // the prepared data file, and read once while a load holds the folder.
        let mut json: BTreeMap<String, Value> = BTreeMap::new();
        for ns in namespaces.flatten() {
            let Ok(folder) = horizon_world::json_folder::read(&ns.path().join("advancement"))
            else {
                continue;
            };
            let ns = ns.file_name().to_string_lossy().into_owned();
            for file in &folder.files {
                if let Ok(v) = serde_json::from_slice::<Value>(file.bytes().unwrap_or_default()) {
                    json.insert(format!("{ns}:{}", file.name), v);
                }
            }
        }
        for (id, files) in overlay.kind("advancement") {
            match serde_json::from_slice::<Value>(horizon_world::loot::PackOverlay::top(files)) {
                Ok(v) => {
                    json.insert(id.to_owned(), v);
                }
                Err(e) => log::warn!("Horizon: pack advancement {id} not loaded: {e}"),
            }
        }
        for (id, v) in json {
            if let Some(a) = parse(&id, &v) {
                let index = tree.list.len();
                for (ci, c) in a.criteria.iter().enumerate() {
                    tree.by_trigger
                        .entry(c.trigger.clone())
                        .or_default()
                        .push((index, ci));
                }
                tree.by_id.insert(id, index);
                tree.list.push(a);
            }
        }
        tree.index_recipes();
        tree.index_looking_at();
        tree.index_nbt();
        tree.index_player_facts();
        tree
    }
    /// Index the `recipe_unlocked` criteria by recipe: a grant that unlocks
    /// twenty recipes tested every one of the ~1,300 recipe advancements'
    /// criteria for each (several ms in one tick).
    pub fn index_recipes(&mut self) {
        self.by_recipe.clear();
        self.recipes_any.clear();
        for &(ai, ci) in self
            .by_trigger
            .get("minecraft:recipe_unlocked")
            .into_iter()
            .flatten()
        {
            // `read`: the index alone keeps none of the criteria read.
            match self.list[ai].criteria[ci].conditions.read()["recipes"].as_str() {
                Some(r) => self
                    .by_recipe
                    .entry(full(r).into_owned())
                    .or_default()
                    .push((ai, ci)),
                None => self.recipes_any.push((ai, ci)),
            }
        }
    }
    /// Index the triggers whose criteria name `looking_at`, or a
    /// `minecraft:reference` to a predicate file that might: the text is
    /// searched, nothing is read.
    pub fn index_looking_at(&mut self) {
        let reference = ["\"minecraft:reference\"", "\"reference\""];
        self.looking_at_triggers = (self.list.iter().flat_map(|a| &a.criteria))
            .filter(|c| {
                let json = c.conditions.json();
                json.contains("\"looking_at\"") || reference.iter().any(|r| json.contains(r))
            })
            .map(|c| c.trigger.clone())
            .collect();
    }
    /// Whether a criterion of `trigger` may test what the player looks at.
    pub fn may_look(&self, trigger: &str) -> bool {
        self.looking_at_triggers.contains(trigger)
    }
    /// Index the triggers whose criteria name `nbt`, or a
    /// `minecraft:reference` to a predicate file that might: the text is
    /// searched, nothing is read.
    pub fn index_nbt(&mut self) {
        let keys = [
            "\"nbt\"",
            "\"minecraft:nbt\"",
            "\"minecraft:reference\"",
            "\"reference\"",
        ];
        self.nbt_triggers = (self.list.iter().flat_map(|a| &a.criteria))
            .filter(|c| {
                let json = c.conditions.json();
                keys.iter().any(|k| json.contains(k))
            })
            .map(|c| c.trigger.clone())
            .collect();
    }
    /// Whether a criterion of `trigger` may test an entity's `nbt`.
    pub fn may_test_nbt(&self, trigger: &str) -> bool {
        self.nbt_triggers.contains(trigger)
    }
    /// Whether any criterion may test an entity's `nbt`.
    pub fn tests_nbt(&self) -> bool {
        !self.nbt_triggers.is_empty()
    }
    /// Index what each trigger's criteria may ask of the player's level,
    /// statistics, advancements and recipes: a player predicate naming the
    /// field, or a `minecraft:reference` to a predicate file that might (all
    /// four). The text is searched, nothing is read.
    pub fn index_player_facts(&mut self) {
        let reference = ["\"minecraft:reference\"", "\"reference\""];
        let mut out: HashMap<String, FactsAsked> = HashMap::new();
        for c in self.list.iter().flat_map(|a| &a.criteria) {
            let json = c.conditions.json();
            let asked = if reference.iter().any(|r| json.contains(r)) {
                FactsAsked::ALL
            } else if json.contains("type_specific/player") {
                FactsAsked {
                    level: json.contains("\"level\""),
                    stats: json.contains("\"stats\""),
                    advancements: json.contains("\"advancements\""),
                    recipes: json.contains("\"recipes\""),
                }
            } else {
                continue;
            };
            if asked == FactsAsked::default() {
                continue;
            }
            let e = out.entry(c.trigger.clone()).or_default();
            *e = e.or(asked);
        }
        self.player_fact_triggers = out;
    }
    /// What a criterion of `trigger` may ask of the player's level,
    /// statistics, advancements and recipes.
    pub fn player_facts_asked(&self, trigger: &str) -> FactsAsked {
        self.player_fact_triggers
            .get(trigger)
            .copied()
            .unwrap_or_default()
    }
    /// The `recipe_unlocked` criteria that could pass for `recipe`, in tree
    /// order (the others name another recipe).
    pub fn recipe_criteria(&self, recipe: &str) -> Vec<(usize, usize)> {
        let mut out = self.recipes_any.clone();
        out.extend(self.by_recipe.get(recipe).into_iter().flatten());
        out.sort_unstable();
        out
    }
    pub fn find_id(&self, id: &str) -> Option<usize> {
        self.by_id.get(id).copied().or_else(|| {
            if let Some(rest) = id.strip_prefix("minecraft:") {
                self.by_id.get(rest).copied()
            } else {
                self.by_id.get(full(id).as_ref()).copied()
            }
        })
    }
    pub fn get(&self, id: &str) -> Option<&Advancement> {
        self.find_id(id).map(|i| &self.list[i])
    }
}

/// The `inventory_changed` criteria by the items they need. A criterion whose
/// item predicates include one limited to some items (an id, a `#tag`, a
/// list) can only pass with one of those in the inventory, so an inventory
/// change tests only the criteria of the items it holds, plus those that
/// need no particular item. The same answers as testing them all: the ~1,300
/// recipe advancements each have one, and testing every one of them cost a
/// player 1–4 ms on every tick its inventory changed.
#[derive(Default)]
pub struct InventoryIndex {
    by_item: HashMap<String, Vec<(usize, usize)>>,
    any: Vec<(usize, usize)>,
}
impl InventoryIndex {
    /// `tag` gives a `#tag`'s item ids (none for a tag that doesn't exist,
    /// which matches nothing).
    pub fn build<'t>(tree: &Tree, tag: impl Fn(&str) -> Vec<&'t str>) -> Self {
        fn ids<'t>(v: &Value, tag: &impl Fn(&str) -> Vec<&'t str>, out: &mut Vec<String>) {
            match v {
                Value::String(s) => match s.strip_prefix('#') {
                    Some(t) => out.extend(tag(&full(t)).into_iter().map(str::to_owned)),
                    None => out.push(full(s).into_owned()),
                },
                Value::Array(a) => a.iter().for_each(|x| ids(x, tag, out)),
                _ => {}
            }
        }
        let mut index = Self::default();
        let Some(list) = tree.by_trigger.get("minecraft:inventory_changed") else {
            return index;
        };
        for &(ai, ci) in list {
            let conditions = tree.list[ai].criteria[ci].conditions.read();
            let items = &conditions["items"];
            // `id_matches`: only a string or a list limits the items.
            let limited = items.as_array().and_then(|a| {
                a.iter()
                    .map(|p| &p["items"])
                    .find(|v| v.is_string() || v.is_array())
            });
            match limited {
                Some(v) => {
                    let mut out = Vec::new();
                    ids(v, &tag, &mut out);
                    out.sort();
                    out.dedup();
                    for id in out {
                        index.by_item.entry(id).or_default().push((ai, ci));
                    }
                }
                None => index.any.push((ai, ci)),
            }
        }
        index
    }
    /// The criteria an inventory holding these item ids could pass, in
    /// tree order.
    pub fn candidates<'a>(&self, held: impl IntoIterator<Item = &'a str>) -> Vec<(usize, usize)> {
        let mut out = self.any.clone();
        for id in held {
            if let Some(list) = self.by_item.get(id) {
                out.extend_from_slice(list);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}
fn parse(id: &str, v: &Value) -> Option<Advancement> {
    let criteria: Vec<Criterion> = v["criteria"]
        .as_object()?
        .iter()
        .map(|(name, c)| Criterion {
            name: name.clone(),
            trigger: c["trigger"]
                .as_str()
                .map_or(String::new(), |t| full(t).into_owned()),
            conditions: LazyJson::new(&c["conditions"]),
        })
        .collect();
    let requirements = match v["requirements"].as_array() {
        Some(r) => r
            .iter()
            .map(|group| {
                group
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|s| s.as_str().map(str::to_owned))
                    .collect()
            })
            .collect(),
        // AdvancementRequirements.allOf: every criterion.
        None => criteria.iter().map(|c| vec![c.name.clone()]).collect(),
    };
    let d = &v["display"];
    let display = d.is_object().then(|| Display {
        icon: d["icon"]["id"]
            .as_str()
            .unwrap_or("minecraft:air")
            .to_owned(),
        title: d["title"].clone(),
        description: d["description"].clone(),
        frame: frame(&d["frame"]),
        background: d["background"].as_str().map(str::to_owned),
        show_toast: d["show_toast"].as_bool().unwrap_or(true),
        announce: d["announce_to_chat"].as_bool().unwrap_or(true),
        hidden: d["hidden"].as_bool().unwrap_or(false),
    });
    let r = &v["rewards"];
    let strings = |v: &Value| -> Vec<String> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s.as_str().map(str::to_owned))
            .collect()
    };
    Some(Advancement {
        id: id.to_owned(),
        parent: v["parent"].as_str().map(|p| full(p).into_owned()),
        display,
        criteria,
        requirements,
        rewards: Rewards {
            recipes: strings(&r["recipes"]),
            experience: r["experience"].as_i64().unwrap_or(0) as i32,
            loot: strings(&r["loot"]),
            function: r["function"].as_str().map(|id| full(id).into_owned()),
        },
    })
}

/// `CriterionProgress`'s date as saved, "yyyy-MM-dd HH:mm:ss Z", in UTC.
pub fn date_text(unix_secs: i64) -> String {
    let days = unix_secs.div_euclid(86400);
    let rem = unix_secs.rem_euclid(86400);
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} +0000",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// A saved date back to milliseconds since the epoch (the network form of
/// `CriterionProgress`), the zone offset applied.
pub fn date_millis(text: &str) -> Option<i64> {
    let (date, rest) = text.trim().split_once(' ')?;
    let (time, zone) = rest.split_once(' ').unwrap_or((rest, "+0000"));
    let mut d = date.split('-').map(|v| v.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.split(':').map(|v| v.parse::<i64>().ok());
    let (h, min, sec) = (t.next()??, t.next()??, t.next()??);
    let sign = if zone.starts_with('-') { -1 } else { 1 };
    let zone = zone.trim_start_matches(['+', '-']);
    let offset =
        zone.get(0..2)?.parse::<i64>().ok()? * 3600 + zone.get(2..4)?.parse::<i64>().ok()? * 60;
    // Days from civil (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((days * 86400 + h * 3600 + min * 60 + sec - sign * offset) * 1000)
}

/// The player's progress: advancement -> criterion -> obtained date.
#[derive(Default, Clone)]
pub struct Progress {
    pub criteria: BTreeMap<String, BTreeMap<String, String>>,
}
impl Progress {
    pub fn has(&self, adv: &str, criterion: &str) -> bool {
        self.criteria
            .get(adv)
            .is_some_and(|m| m.contains_key(criterion))
    }
    /// `AdvancementProgress.isDone`.
    pub fn done(&self, a: &Advancement) -> bool {
        !a.requirements.is_empty()
            && a.requirements
                .iter()
                .all(|group| group.iter().any(|c| self.has(&a.id, c)))
    }
    /// Completed requirement groups and their count (the screen's tooltip).
    pub fn counts(&self, a: &Advancement) -> (usize, usize) {
        let done = a
            .requirements
            .iter()
            .filter(|g| g.iter().any(|c| self.has(&a.id, c)))
            .count();
        (done, a.requirements.len())
    }
    pub fn grant(&mut self, adv: &str, criterion: &str, date: &str) -> bool {
        let m = self.criteria.entry(adv.to_owned()).or_default();
        if m.contains_key(criterion) {
            return false;
        }
        m.insert(criterion.to_owned(), date.to_owned());
        true
    }
    pub fn revoke(&mut self, adv: &str, criterion: &str) -> bool {
        self.criteria
            .get_mut(adv)
            .is_some_and(|m| m.remove(criterion).is_some())
    }
    pub fn from_json(v: &Value) -> Progress {
        let mut p = Progress::default();
        if let Some(o) = v.as_object() {
            for (adv, entry) in o {
                if adv == "DataVersion" {
                    continue;
                }
                if let Some(c) = entry["criteria"].as_object() {
                    let m = p.criteria.entry(adv.clone()).or_default();
                    for (k, d) in c {
                        m.insert(k.clone(), d.as_str().unwrap_or("").to_owned());
                    }
                }
            }
        }
        p
    }
    pub fn to_json(&self, tree: &Tree, data_version: i32) -> Value {
        let mut o = serde_json::Map::new();
        for (adv, m) in &self.criteria {
            if m.is_empty() {
                continue;
            }
            let done = tree.get(adv).is_some_and(|a| self.done(a));
            o.insert(
                adv.clone(),
                serde_json::json!({ "criteria": m, "done": done }),
            );
        }
        o.insert("DataVersion".into(), data_version.into());
        Value::Object(o)
    }
}

/// What predicates may ask about the world.
pub trait World {
    fn dimension(&self) -> &str;
    /// Effective rain/thunder booleans, including the level's dimension rules.
    fn weather(&self) -> (bool, bool);
    fn biome(&self, pos: [i32; 3]) -> String;
    fn structures(&self, pos: [i32; 3]) -> Vec<String>;
    /// Block name and properties.
    fn block(&self, pos: [i32; 3]) -> (String, BTreeMap<String, String>);
    fn fluid(&self, pos: [i32; 3]) -> Option<String>;
    fn can_see_sky(&self, pos: [i32; 3]) -> bool;
    fn light(&self, pos: [i32; 3]) -> i32;
    fn smokey(&self, pos: [i32; 3]) -> bool;
    fn in_tag(&self, registry: &str, tag: &str, id: &str) -> bool;
    /// The block entity's `saveWithFullMetadata` at `pos`, `None` without one.
    fn block_entity_nbt(&self, _pos: [i32; 3]) -> Option<horizon_world::nbt::Tag> {
        None
    }
    /// Read-only contents of a loaded named predicate, when this context
    /// supplies a predicate registry. Other contexts keep references absent.
    fn predicate(&self, _id: &str) -> Option<&Value> {
        None
    }
}
/// An item stack as predicates see it.
#[derive(Clone, Debug, Default)]
pub struct ItemView {
    pub id: String,
    pub count: u32,
    pub damage: u32,
    pub enchantments: Vec<(String, u32)>,
    pub potion: Option<String>,
    pub custom_name: bool,
    /// Typed snapshot for supported mob equipment views. `None` leaves this
    /// family unprojected and unsupported; an empty compound is actual absence.
    pub custom_data: Option<horizon_world::nbt::Tag>,
}
/// Query snapshot of supported known movement, in blocks/tick, and actual fall distance.
#[derive(Clone, Copy, Debug)]
pub struct MovementView {
    pub per_tick: [f64; 3],
    pub fall_distance: f64,
}

/// An entity as predicates see it.
#[derive(Clone, Debug, Default)]
pub struct EntityView {
    /// None is unprojected/unsupported, not a fabricated stationary entity.
    pub movement: Option<MovementView>,
    pub kind: String,
    pub position: [f64; 3],
    pub on_fire: bool,
    pub sneaking: bool,
    pub sprinting: bool,
    pub swimming: bool,
    pub flying: bool,
    pub baby: bool,
    pub on_ground: bool,
    /// mainhand, offhand, head, chest, legs, feet, body.
    pub equipment: [Option<ItemView>; 7],
    pub vehicle: Option<Box<EntityView>>,
    /// `Entity.getPassengers`, for `PassengerPredicate`.
    pub passengers: Vec<EntityView>,
    /// Player: what it looks at within 100 blocks (`PlayerPredicate.lookingAt`).
    pub looking_at: Option<Box<EntityView>>,
    pub effects: Vec<(String, i32, i32)>,
    /// Player: game mode name.
    pub gamemode: Option<String>,
    /// Player: the sneak input is held (`Input.shift`, what `input.sneak`
    /// asks; not the sneaking pose).
    pub input_sneak: bool,
    /// Player: its level, statistics, advancements and recipes, as far as
    /// they were filled.
    pub player_facts: PlayerFacts,
    /// Lightning: entities it struck.
    pub struck: Vec<EntityView>,
    /// Lightning: blocks it set on fire (`LightningBolt.blocksSetOnFire`).
    pub blocks_set_on_fire: i32,
    pub fishing_open_water: Option<bool>,
    pub variant: Option<String>,
    /// The data components the entity reports (`Entity.get`): id and value.
    pub components: Vec<(String, String)>,
    /// `Entity.get(CUSTOM_NAME)`: the live Text component, preserving styles
    /// and siblings rather than treating its contents as a registry id.
    pub custom_name: Option<Value>,
    /// The entity as saved (`NbtPredicate.getEntityTagToCompare`), for
    /// `minecraft:nbt`; built only when a criterion may ask (`Tree::tests_nbt`).
    pub nbt: Option<horizon_world::nbt::Tag>,
    /// `Entity.getTeam().getName()` (a tamed pet answers its owner's team);
    /// `None` without a team or where the identity is not bound.
    pub team: Option<String>,
    /// Existing objective scores for the bound command source holder. `None`
    /// means this context did not bind a scoreboard holder; an empty map is a
    /// real holder without scores. Named predicates snapshot current values.
    pub scores: Option<BTreeMap<String, i32>>,
}
/// What `PlayerPredicate` reads of a player beyond its entity. Each record
/// is filled only when a criterion may ask for it (`Tree::player_facts_asked`),
/// the food always (two numbers); a predicate that asks for a part left
/// `None` fails.
#[derive(Clone, Default)]
pub struct PlayerFacts {
    /// `Player.experienceLevel`.
    pub level: Option<i32>,
    /// `Player.getFoodData`: `FoodData.getFoodLevel` and
    /// `FoodData.getSaturationLevel`.
    pub food: Option<(i32, f32)>,
    /// `ServerStatsCounter`: stat type -> stat id -> value, both namespaced.
    pub stats: Option<BTreeMap<String, BTreeMap<String, i32>>>,
    /// `PlayerAdvancements`: the loaded advancements and the player's
    /// progress in them.
    pub advancements: Option<(Arc<Tree>, Progress)>,
    /// `ServerRecipeBook`: the known recipe ids.
    pub recipes: Option<BTreeSet<String>>,
}
impl std::fmt::Debug for PlayerFacts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerFacts")
            .field("level", &self.level)
            .field("food", &self.food)
            .field("stats", &self.stats)
            .field(
                "advancements",
                &self.advancements.as_ref().map(|(_, p)| p.criteria.len()),
            )
            .field("recipes", &self.recipes.as_ref().map(BTreeSet::len))
            .finish()
    }
}
fn range_f(v: &Value, x: f64) -> bool {
    match v {
        Value::Null => true,
        Value::Number(n) => n.as_f64().is_some_and(|n| (n - x).abs() < 1e-9),
        Value::Object(o) => {
            o.get("min").and_then(Value::as_f64).is_none_or(|m| x >= m)
                && o.get("max").and_then(Value::as_f64).is_none_or(|m| x <= m)
        }
        _ => false,
    }
}
pub fn range_i(v: &Value, x: i64) -> bool {
    range_f(v, x as f64)
}
/// `Identifier.parse`: an id written without a namespace is a `minecraft:` one.
pub fn full(id: &str) -> std::borrow::Cow<'_, str> {
    if id.contains(':') {
        id.into()
    } else {
        format!("minecraft:{id}").into()
    }
}
/// The entry of a map keyed by registry ids (`Identifier.parse` on the key),
/// such as an item predicate's `components` or `predicates`.
fn keyed<'a>(map: &'a Value, id: &str) -> Option<&'a Value> {
    map.as_object()?
        .iter()
        .find(|(k, _)| full(k) == id)
        .map(|(_, v)| v)
}
/// A registry id, `#tag`, or list matched against one id.
pub fn id_matches(v: &Value, registry: &str, id: &str, w: &dyn World) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => match s.strip_prefix('#') {
            Some(tag) => w.in_tag(registry, &full(tag), id),
            None => full(s) == id,
        },
        Value::Array(a) => a.iter().any(|x| id_matches(x, registry, id, w)),
        _ => false,
    }
}
/// `PotionsPredicate`: the potion in a `HolderSet`, plus a `CollectionPredicate`
/// over the potion's effects (`contains`, `count`, `size`).
pub fn potions(v: &Value, potion: Option<&str>, w: &dyn World) -> bool {
    if v.is_null() {
        return true;
    }
    if !v["potions"].is_null() && !potion.is_some_and(|p| id_matches(&v["potions"], "potion", p, w))
    {
        return false;
    }
    let c = &v["effects"];
    if c.is_null() {
        return true;
    }
    let effects: Vec<(String, i32, i32)> = potion
        .and_then(crate::brewing::potion)
        .map(|d| {
            d.effects
                .iter()
                .map(|(id, dur, amp)| (format!("minecraft:{id}"), *amp, *dur))
                .collect()
        })
        .unwrap_or_default();
    // `MobEffectsPredicate.test`: the one listed effect, amplifier and duration.
    let test = |pred: &Value, e: &(String, i32, i32)| {
        pred.as_object().is_some_and(|m| {
            m.iter()
                .find(|(k, _)| full(k) == e.0)
                .is_some_and(|(_, want)| {
                    range_i(&want["amplifier"], i64::from(e.1))
                        && range_i(&want["duration"], i64::from(e.2))
                        && want["ambient"].as_bool().is_none_or(|b| !b)
                        && want["visible"].as_bool().is_none_or(|b| b)
                })
        })
    };
    c["contains"]
        .as_array()
        .is_none_or(|l| l.iter().all(|p| effects.iter().any(|e| test(p, e))))
        && c["count"].as_array().is_none_or(|l| {
            l.iter().all(|entry| {
                let n = effects.iter().filter(|e| test(&entry["test"], e)).count();
                range_i(&entry["count"], n as i64)
            })
        })
        && range_i(&c["size"], effects.len() as i64)
}
/// `ItemPredicate`.
pub fn item(v: &Value, it: &ItemView, w: &dyn World) -> bool {
    if v.is_null() {
        return true;
    }
    if !id_matches(&v["items"], "item", &it.id, w) {
        return false;
    }
    if !range_i(&v["count"], i64::from(it.count)) {
        return false;
    }
    // Data components: exact matches on the few the data uses.
    let comps = &v["components"];
    if let Some(p) = keyed(comps, "minecraft:potion_contents").and_then(Value::as_str)
        && !it.potion.as_deref().is_some_and(|h| full(h) == full(p))
    {
        return false;
    }
    if let Some(d) = keyed(comps, "minecraft:damage").and_then(Value::as_u64)
        && u64::from(it.damage) != d
    {
        return false;
    }
    let preds = &v["predicates"];
    if let Some(list) = keyed(preds, "minecraft:enchantments").and_then(Value::as_array) {
        for e in list {
            let ok = it.enchantments.iter().any(|(id, lvl)| {
                id_matches(&e["enchantments"], "enchantment", id, w)
                    && range_i(&e["levels"], i64::from(*lvl))
            });
            if !ok {
                return false;
            }
        }
    }
    if let Some(d) = keyed(preds, "minecraft:damage").and_then(Value::as_object) {
        let damage = Value::Object(d.clone());
        if !range_i(&damage["damage"], i64::from(it.damage)) {
            return false;
        }
    }
    // `CustomDataPredicate.matches`: typed subset matching of a projected
    // stack snapshot. Unprojected consumers keep their unsupported behavior.
    if let Some(pattern) = keyed(preds, "minecraft:custom_data").and_then(Value::as_str)
        && let Some(actual) = it.custom_data.as_ref()
    {
        let Ok(pattern @ horizon_world::nbt::Tag::Compound(_)) =
            crate::command::snbt::parse_fully(pattern)
        else {
            return false;
        };
        if !crate::nbt_path::compare_nbt(&pattern, Some(actual)) {
            return false;
        }
    }
    true
}
/// `LockCode.canUnlock`: a spectator opens any container; otherwise the
/// player's main hand must match the lock's `ItemPredicate`. An empty hand
/// holds nothing to match (`getMainHandItem` is `ItemStack.EMPTY`), while
/// `LockCode.NO_LOCK`'s empty predicate takes any stack.
pub fn can_unlock(
    lock: &Value,
    spectator: bool,
    main_hand: Option<&ItemView>,
    w: &dyn World,
) -> bool {
    let empty = ItemView::default();
    spectator || item(lock, main_hand.unwrap_or(&empty), w)
}
/// `BlockPredicate`.
pub fn block(v: &Value, pos: [i32; 3], w: &dyn World) -> bool {
    if v.is_null() {
        return true;
    }
    let (name, props) = w.block(pos);
    if !id_matches(&v["blocks"], "block", &name, w) {
        return false;
    }
    if let Some(state) = v["state"].as_object() {
        for (k, want) in state {
            let have = props.get(k).cloned().unwrap_or_default();
            let ok = match want {
                Value::String(s) => have == *s,
                Value::Bool(b) => have == b.to_string(),
                Value::Number(n) => have == n.to_string(),
                Value::Object(_) => have.parse::<f64>().is_ok_and(|h| range_f(want, h)),
                _ => false,
            };
            if !ok {
                return false;
            }
        }
    }
    // `matchesBlockEntity`: the block entity's data holds the pattern
    // (`NbtPredicate.matches`, `NbtUtils.compareNbt`); no block entity fails.
    if let Some(nbt) = v["nbt"].as_str() {
        let Ok((pattern, _)) = horizon_world::nbt::parse_snbt(nbt) else {
            return false;
        };
        if !crate::nbt_path::compare_nbt(&pattern, w.block_entity_nbt(pos).as_ref()) {
            return false;
        }
    }
    true
}
/// `LocationPredicate`.
pub fn location(v: &Value, p: [f64; 3], w: &dyn World) -> bool {
    if v.is_null() {
        return true;
    }
    let pos = [
        p[0].floor() as i32,
        p[1].floor() as i32,
        p[2].floor() as i32,
    ];
    let posv = &v["position"];
    if !range_f(&posv["x"], p[0]) || !range_f(&posv["y"], p[1]) || !range_f(&posv["z"], p[2]) {
        return false;
    }
    if let Some(d) = v["dimension"].as_str()
        && full(d) != w.dimension()
    {
        return false;
    }
    if !v["biomes"].is_null() && !id_matches(&v["biomes"], "worldgen/biome", &w.biome(pos), w) {
        return false;
    }
    if !v["structures"].is_null() {
        let here = w.structures(pos);
        if !here
            .iter()
            .any(|s| id_matches(&v["structures"], "worldgen/structure", s, w))
        {
            return false;
        }
    }
    if let Some(b) = v["can_see_sky"].as_bool()
        && b != w.can_see_sky(pos)
    {
        return false;
    }
    if let Some(b) = v["smokey"].as_bool()
        && b != w.smokey(pos)
    {
        return false;
    }
    if !v["light"].is_null() && !range_i(&v["light"]["light"], i64::from(w.light(pos))) {
        return false;
    }
    if !v["block"].is_null() && !block(&v["block"], pos, w) {
        return false;
    }
    if !v["fluid"].is_null() {
        let Some(f) = w.fluid(pos) else {
            return false;
        };
        if !id_matches(&v["fluid"]["fluids"], "fluid", &f, w) {
            return false;
        }
    }
    true
}
/// `DataComponentExactPredicate.test` on an entity: every expected
/// component equals what the entity reports (absent ones never match).
fn components(expected: &Value, entity: &EntityView) -> bool {
    expected.as_object().is_some_and(|o| {
        o.iter().all(|(id, want)| {
            let id = full(id);
            if id == "minecraft:custom_name" {
                return entity.custom_name.as_ref().is_some_and(|have| {
                    crate::scoreboard::canonical_component(have)
                        == crate::scoreboard::canonical_component(want)
                });
            }
            want.as_str().is_some_and(|want| {
                entity
                    .components
                    .iter()
                    .any(|(k, v)| full(k) == id && full(v) == full(want))
            })
        })
    })
}
/// `DistancePredicate.matches` between two points.
pub fn distance(v: &Value, a: [f64; 3], b: [f64; 3]) -> bool {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    range_f(&v["x"], d[0].abs())
        && range_f(&v["y"], d[1].abs())
        && range_f(&v["z"], d[2].abs())
        && range_f(&v["horizontal"], (d[0] * d[0] + d[2] * d[2]).sqrt())
        && range_f(
            &v["absolute"],
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt(),
        )
}
/// `MinMaxBounds.Doubles.matches` and `matchesSqr`, only for movement predicates.
fn movement_range(v: &Value, value: f64, squared: bool) -> bool {
    let (mut min, mut max) = match v {
        Value::Null => (None, None),
        Value::Number(n) => {
            let Some(n) = n.as_f64() else { return false };
            (Some(n), Some(n))
        }
        Value::Object(o) => {
            let read = |key| match o.get(key) {
                None => Some(None),
                Some(n) => n.as_f64().map(Some),
            };
            let (Some(min), Some(max)) = (read("min"), read("max")) else {
                return false;
            };
            (min, max)
        }
        _ => return false,
    };
    if squared {
        min = min.map(|n| n * n);
        max = max.map(|n| n * n);
    }
    if min.is_some_and(|n| n > value) {
        return false;
    }
    max.is_none_or(|n| n >= value)
}

/// `MovementPredicate.matches`: known movement is scaled by 20 before the ordered tests.
fn movement(p: &Value, m: MovementView) -> bool {
    if !p.is_object() {
        return false;
    }
    let [x, y, z] = m.per_tick.map(|v| v * 20.0);
    movement_range(&p["x"], x, false)
        && movement_range(&p["y"], y, false)
        && movement_range(&p["z"], z, false)
        && movement_range(&p["speed"], x * x + y * y + z * z, true)
        && movement_range(&p["horizontal_speed"], x * x + z * z, true)
        && movement_range(&p["vertical_speed"], y.abs(), false)
        && movement_range(&p["fall_distance"], m.fall_distance, false)
}

/// `EntityPredicate` (26.x keyed sub-predicates).
pub fn entity(v: &Value, e: &EntityView, w: &dyn World, origin: [f64; 3]) -> bool {
    if v.is_null() {
        return true;
    }
    let Some(o) = v.as_object() else {
        return false;
    };
    for (key, p) in o {
        // `EntityPredicate.CODEC`: the keys are sub-predicate type ids.
        let ok = match &*full(key) {
            "minecraft:entity_type" => id_matches(p, "entity_type", &e.kind, w),
            "minecraft:location" => location(p, e.position, w),
            "minecraft:stepping_on" => {
                location(p, [e.position[0], e.position[1] - 0.5, e.position[2]], w)
            }
            "minecraft:distance" => distance(p, e.position, origin),
            "minecraft:movement" => e.movement.is_some_and(|m| movement(p, m)),
            "minecraft:flags" => {
                let f = |k: &str, have: bool| p[k].as_bool().is_none_or(|want| want == have);
                f("is_on_fire", e.on_fire)
                    && f("is_sneaking", e.sneaking)
                    && f("is_sprinting", e.sprinting)
                    && f("is_swimming", e.swimming)
                    && f("is_flying", e.flying)
                    && f("is_baby", e.baby)
                    && f("is_on_ground", e.on_ground)
            }
            "minecraft:equipment" => {
                let slots = [
                    "mainhand", "offhand", "head", "chest", "legs", "feet", "body",
                ];
                slots.iter().enumerate().all(|(i, s)| {
                    p[*s].is_null()
                        || e.equipment[i]
                            .as_ref()
                            .is_some_and(|it| item(&p[*s], it, w))
                })
            }
            "minecraft:vehicle" => e
                .vehicle
                .as_ref()
                .is_some_and(|veh| entity(p, veh, w, origin)),
            // PassengerPredicate.matches: any passenger matches the nested predicate.
            "minecraft:passenger" => e
                .passengers
                .iter()
                .any(|passenger| entity(p, passenger, w, origin)),
            "minecraft:effects" => p.as_object().is_some_and(|m| {
                m.iter().all(|(id, want)| {
                    e.effects.iter().any(|(eid, amp, dur)| {
                        *eid == full(id)
                            && range_i(&want["amplifier"], i64::from(*amp))
                            && range_i(&want["duration"], i64::from(*dur))
                    })
                })
            }),
            // `EntityNbtPredicate.matches`: the entity's compound holds the
            // pattern (`NbtUtils.compareNbt`); no compound known fails.
            "minecraft:nbt" => p.as_str().is_some_and(|s| {
                horizon_world::nbt::parse_snbt(s).is_ok_and(|(pattern, _)| {
                    crate::nbt_path::compare_nbt(&pattern, e.nbt.as_ref())
                })
            }),
            "minecraft:type_specific/player" => {
                // `PlayerPredicate.matches`: `instanceof ServerPlayer` first,
                // then the game mode, what it looks at and its `input`.
                e.kind == "minecraft:player"
                    && p["gamemode"].as_array().is_none_or(|modes| {
                        e.gamemode
                            .as_deref()
                            .is_some_and(|g| modes.iter().any(|m| m.as_str() == Some(g)))
                    })
                    && (p["looking_at"].is_null()
                        || e.looking_at
                            .as_ref()
                            .is_some_and(|t| entity(&p["looking_at"], t, w, origin)))
                    && (p["input"].is_null() || input(&p["input"], e))
                    && player_facts(p, &e.player_facts)
            }
            "minecraft:type_specific/lightning" => {
                range_i(&p["blocks_set_on_fire"], i64::from(e.blocks_set_on_fire))
                    && (p["entity_struck"].is_null()
                        || e.struck
                            .iter()
                            .any(|s| entity(&p["entity_struck"], s, w, origin)))
            }
            "minecraft:type_specific/fishing_hook" => p["in_open_water"]
                .as_bool()
                .is_none_or(|want| e.fishing_open_water == Some(want)),
            "minecraft:components" => components(p, e),
            // `TeamPredicate.matches`: a non-null effective team whose name
            // equals the string exactly.
            "minecraft:team" => {
                matches!((p.as_str(), &e.team), (Some(want), Some(have)) if want == have)
            }
            "minecraft:predicates" => true,
            _ => false,
        };
        if !ok {
            return false;
        }
    }
    true
}
/// The rest of `PlayerPredicate.matches` (expected from it): the experience
/// level in the `level` bounds; the food (`FoodPredicate.matches`); each
/// `stats` entry's value (0 when never counted) in its bounds; each listed
/// recipe known or not as asked; each listed advancement loaded, and done or
/// not, or each named criterion of it granted or not. A field asked of facts
/// that weren't filled fails.
fn player_facts(p: &Value, f: &PlayerFacts) -> bool {
    let level = &p["level"];
    if !level.is_null() && !f.level.is_some_and(|l| range_i(level, i64::from(l))) {
        return false;
    }
    // `FoodPredicate.matches`: the food level in `level`, then the
    // saturation (a float, compared as a double) in `saturation`.
    let food = &p["food"];
    if !food.is_null() {
        let Some((level, saturation)) = f.food else {
            return false;
        };
        if !range_i(&food["level"], i64::from(level))
            || !range_f(&food["saturation"], f64::from(saturation))
        {
            return false;
        }
    }
    let stats = &p["stats"];
    if !stats.is_null() {
        let Some(list) = stats.as_array() else {
            return false;
        };
        // `StatMatcher.matches`: the counter's value for the type's stat.
        let ok = list.is_empty()
            || f.stats.as_ref().is_some_and(|counts| {
                list.iter().all(|m| {
                    let (Some(ty), Some(stat)) = (m["type"].as_str(), m["stat"].as_str()) else {
                        return false;
                    };
                    let value = counts
                        .get(&*full(ty))
                        .and_then(|of| of.get(&*full(stat)))
                        .copied()
                        .unwrap_or(0);
                    range_i(&m["value"], i64::from(value))
                })
            });
        if !ok {
            return false;
        }
    }
    let recipes = &p["recipes"];
    if !recipes.is_null() {
        let Some(map) = recipes.as_object() else {
            return false;
        };
        let ok = map.is_empty()
            || f.recipes.as_ref().is_some_and(|known| {
                map.iter().all(|(id, want)| {
                    want.as_bool()
                        .is_some_and(|want| known.contains(&*full(id)) == want)
                })
            });
        if !ok {
            return false;
        }
    }
    let advancements = &p["advancements"];
    if !advancements.is_null() {
        let Some(map) = advancements.as_object() else {
            return false;
        };
        let ok = map.is_empty()
            || f.advancements.as_ref().is_some_and(|(tree, progress)| {
                map.iter().all(|(id, want)| {
                    // An advancement that isn't loaded matches nothing.
                    let Some(a) = tree.get(&full(id)) else {
                        return false;
                    };
                    match want {
                        // `AdvancementDonePredicate`.
                        Value::Bool(want) => progress.done(a) == *want,
                        // `AdvancementCriterionsPredicate`: a criterion the
                        // advancement doesn't have matches nothing.
                        Value::Object(criteria) => criteria.iter().all(|(name, want)| {
                            want.as_bool().is_some_and(|want| {
                                a.criteria.iter().any(|c| c.name == *name)
                                    && progress.has(&a.id, name) == want
                            })
                        }),
                        _ => false,
                    }
                })
            });
        if !ok {
            return false;
        }
    }
    true
}
/// `InputPredicate.matches`: the player's held inputs. The sneak input is
/// what the game holds; the other inputs have no source here yet and count
/// as unconstrained.
fn input(p: &Value, e: &EntityView) -> bool {
    p["sneak"]
        .as_bool()
        .is_none_or(|want| want == e.input_sneak)
}
/// `KilledByArrowTrigger.TriggerInstance.matches`: the weapon, each listed
/// victim condition matched by a distinct victim, and the count of entity types.
pub fn killed_by_arrow(
    c: &Value,
    me: &EntityView,
    victims: &[EntityView],
    weapon: Option<&ItemView>,
    w: &dyn World,
) -> bool {
    fn ctx<'a>(e: &'a EntityView, me: &EntityView) -> Entities<'a> {
        Entities {
            this: Some(e),
            origin: me.position,
            tool: None,
            block_pos: None,
        }
    }
    if !conditions(&c["player"], &ctx(me, me), w) {
        return false;
    }
    if !c["fired_from_weapon"].is_null()
        && !weapon.is_some_and(|it| item(&c["fired_from_weapon"], it, w))
    {
        return false;
    }
    let mut left: Vec<&EntityView> = victims.iter().collect();
    for want in c["victims"].as_array().into_iter().flatten() {
        let Some(i) = left.iter().position(|e| conditions(want, &ctx(e, me), w)) else {
            return false;
        };
        left.remove(i);
    }
    let kinds: BTreeSet<&str> = victims.iter().map(|e| e.kind.as_str()).collect();
    range_i(&c["unique_entity_types"], kinds.len() as i64)
}
/// `KilledTrigger.TriggerInstance.matches`: the player, the other entity
/// (the one killed, or the killer for `entity_killed_player`) and the
/// killing blow (none: any `killing_blow` fails).
pub fn killed(
    c: &Value,
    me: &EntityView,
    target: &EntityView,
    blow: Option<&DamageView>,
    w: &dyn World,
) -> bool {
    let pctx = Entities {
        this: Some(me),
        origin: me.position,
        tool: None,
        block_pos: None,
    };
    let ectx = Entities {
        this: Some(target),
        ..pctx
    };
    let blow_ok = c["killing_blow"].is_null()
        || blow.is_some_and(|d| damage_source(&c["killing_blow"], d, w, me.position));
    conditions(&c["player"], &pctx, w) && conditions(&c["entity"], &ectx, w) && blow_ok
}
/// A blow recorded by its damage type and causing entity, as `killing_blow`
/// sees it: the projectile those types name is the direct entity, at `at`;
/// otherwise the causing entity dealt it itself (`DamageSource.isDirect`).
pub fn recorded_blow(kind: String, source: Option<EntityView>, at: [f64; 3]) -> DamageView {
    let projectile = match kind.as_str() {
        "minecraft:arrow" => Some("minecraft:arrow"),
        "minecraft:fireball" | "minecraft:small_fireball" => Some("minecraft:fireball"),
        "minecraft:wither_skull" => Some("minecraft:wither_skull"),
        "minecraft:fireworks" => Some("minecraft:firework_rocket"),
        _ => None,
    };
    DamageView {
        kind,
        is_direct: projectile.is_none(),
        direct: match projectile {
            Some(kind) => Some(EntityView {
                kind: kind.into(),
                position: at,
                ..Default::default()
            }),
            None => source.clone(),
        },
        source,
        ..Default::default()
    }
}
/// A damage event as `DamagePredicate` sees it.
#[derive(Clone, Debug, Default)]
pub struct DamageView {
    /// The damage type id (`minecraft:arrow`).
    pub kind: String,
    /// `DamageSource.getDirectEntity` and `getEntity`.
    pub direct: Option<EntityView>,
    pub source: Option<EntityView>,
    /// `DamageSource.isDirect`: the direct entity is the causing one.
    pub is_direct: bool,
    /// The damage before and after the shield took its share.
    pub original: f32,
    pub actual: f32,
    pub blocked: bool,
}
/// `DamagePredicate`, with its `DamageSourcePredicate` as `type`.
pub fn damage(v: &Value, d: &DamageView, w: &dyn World, origin: [f64; 3]) -> bool {
    if v.is_null() {
        return true;
    }
    let by = |key: &str, e: &Option<EntityView>| {
        v[key].is_null() || e.as_ref().is_some_and(|e| entity(&v[key], e, w, origin))
    };
    if !range_f(&v["dealt"], f64::from(d.original))
        || !range_f(&v["taken"], f64::from(d.actual))
        || !by("source_entity", &d.source)
        || v["blocked"].as_bool().is_some_and(|b| b != d.blocked)
    {
        return false;
    }
    damage_source(&v["type"], d, w, origin)
}
/// `DamageSourcePredicate`: the damage type's tags and the entities behind it.
pub fn damage_source(ty: &Value, d: &DamageView, w: &dyn World, origin: [f64; 3]) -> bool {
    if ty.is_null() {
        return true;
    }
    let tags_ok = ty["tags"].as_array().into_iter().flatten().all(|t| {
        // `TagPredicate`: a `HolderSet`, like `id_matches`.
        id_matches(&t["id"], "damage_type", &d.kind, w) == t["expected"].as_bool().unwrap_or(true)
    });
    let direct_ok = ty["direct_entity"].is_null()
        || d.direct
            .as_ref()
            .is_some_and(|e| entity(&ty["direct_entity"], e, w, origin));
    let source_ok = ty["source_entity"].is_null()
        || d.source
            .as_ref()
            .is_some_and(|e| entity(&ty["source_entity"], e, w, origin));
    tags_ok && direct_ok && source_ok && ty["is_direct"].as_bool().is_none_or(|b| b == d.is_direct)
}
/// The entities a loot-style condition list may refer to.
pub struct Entities<'a> {
    pub this: Option<&'a EntityView>,
    pub origin: [f64; 3],
    pub tool: Option<&'a ItemView>,
    pub block_pos: Option<[i32; 3]>,
}
/// Conditions shared by item/entity interactions and thrown-item pickups.
pub fn item_entity(
    v: &Value,
    player: &EntityView,
    target: &EntityView,
    stack: &ItemView,
    w: &dyn World,
) -> bool {
    let pctx = Entities {
        this: Some(player),
        origin: player.position,
        tool: None,
        block_pos: None,
    };
    let ectx = Entities {
        this: Some(target),
        ..pctx
    };
    conditions(&v["player"], &pctx, w)
        && conditions(&v["entity"], &ectx, w)
        && item(&v["item"], stack, w)
}

/// `RecipeCraftedTrigger.TriggerInstance.matches`: each ingredient predicate
/// takes a different one of the stacks used, in order.
pub fn recipe_ingredients(v: &Value, used: &[ItemView], w: &dyn World) -> bool {
    let Some(preds) = v.as_array() else {
        return true;
    };
    let mut remaining: Vec<&ItemView> = used.iter().collect();
    preds.iter().all(|p| {
        let Some(i) = remaining.iter().position(|it| item(p, it, w)) else {
            return false;
        };
        remaining.remove(i);
        true
    })
}

/// A `ContextAwarePredicate`: a list of loot conditions (or one condition).
/// Invalid references are distinct from false: inversion must not accept them.
pub fn conditions(v: &Value, ctx: &Entities, w: &dyn World) -> bool {
    references_valid(v, w) && conditions_with(v, ctx, &mut |v, ctx| condition_leaf(v, ctx, w))
}

/// Every named predicate `v` reaches is loaded, and none reaches itself.
pub fn references_valid(v: &Value, w: &dyn World) -> bool {
    predicate_references(v, w, &mut BTreeSet::new()).is_ok()
}

enum InvalidReference {
    Missing,
    Cycle,
}

/// `Validatable.validateHolder`'s active-name rule, as a defensive check on
/// this evaluation context. Check every typed reference before boolean
/// short-circuiting; this does not implement registry/startup admission.
fn predicate_references(
    v: &Value,
    w: &dyn World,
    active: &mut BTreeSet<String>,
) -> Result<(), InvalidReference> {
    match v {
        Value::String(id) => {
            let id = full(id).into_owned();
            if !active.insert(id.clone()) {
                return Err(InvalidReference::Cycle);
            }
            let result = w
                .predicate(&id)
                .ok_or(InvalidReference::Missing)
                .and_then(|v| predicate_references(v, w, active));
            active.remove(&id);
            result
        }
        Value::Array(list) => list
            .iter()
            .try_for_each(|v| predicate_references(v, w, active)),
        Value::Object(o) => {
            let kind = o
                .get("condition")
                .or_else(|| o.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("");
            match &*full(kind) {
                "minecraft:inverted" => predicate_references(&v["term"], w, active),
                "minecraft:all_of" | "minecraft:any_of" => v["terms"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .try_for_each(|v| predicate_references(v, w, active)),
                _ => Ok(()),
            }
        }
        _ => Ok(()),
    }
}

/// The existing condition traversal with a caller-owned leaf context.
/// Leaves run synchronously, in order, with normal boolean short-circuiting.
pub fn conditions_with(
    v: &Value,
    ctx: &Entities,
    test: &mut dyn FnMut(&Value, &Entities) -> bool,
) -> bool {
    match v {
        Value::Null => true,
        Value::Array(list) => list.iter().all(|c| conditions_with(c, ctx, test)),
        Value::Object(o) if o.contains_key("condition") || o.contains_key("type") => {
            let ty = o
                .get("condition")
                .or_else(|| o.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("");
            match &*full(ty) {
                "minecraft:inverted" => !conditions_with(&v["term"], ctx, test),
                "minecraft:all_of" => v["terms"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .all(|t| conditions_with(t, ctx, test)),
                "minecraft:any_of" => v["terms"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|t| conditions_with(t, ctx, test)),
                _ => test(v, ctx),
            }
        }
        _ => test(v, ctx),
    }
}

/// `LootItemRandomChanceCondition.test`: evaluate a literal float chance,
/// then always draw once, including at zero and one. Other provider forms
/// and additional fields are not supported by this bounded context.
pub fn literal_random_chance(v: &Value, draw: &mut dyn FnMut() -> f32) -> Option<bool> {
    let o = v.as_object()?;
    let ty = o.get("condition").or_else(|| o.get("type"))?.as_str()?;
    if full(ty) != "minecraft:random_chance"
        || o.keys()
            .any(|k| !matches!(k.as_str(), "condition" | "type" | "chance"))
    {
        return None;
    }
    let chance = o.get("chance")?.as_f64()? as f32;
    Some(draw() < chance)
}

fn condition_leaf(v: &Value, ctx: &Entities, w: &dyn World) -> bool {
    match v {
        Value::Object(o) if o.contains_key("condition") || o.contains_key("type") => {
            let ty = o
                .get("condition")
                .or_else(|| o.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("");
            match &*full(ty) {
                // EntityHasScoreCondition.test/hasScore: missing objective or
                // score fails; this bounded path supports constant integer ranges.
                "minecraft:entity_scores" if v["entity"].as_str() == Some("this") => {
                    let scores = ctx.this.and_then(|e| e.scores.as_ref());
                    v["scores"].as_object().is_some_and(|tests| {
                        scores.is_some_and(|scores| {
                            tests.iter().all(|(objective, range)| {
                                // Do not treat unsupported number providers as
                                // absent min/max bounds in the shared range helper.
                                let numeric = range.as_i64().is_some()
                                    || range.as_object().is_some_and(|bounds| {
                                        bounds.iter().all(|(key, value)| {
                                            matches!(key.as_str(), "min" | "max")
                                                && value.as_i64().is_some()
                                        })
                                    });
                                numeric
                                    && scores
                                        .get(objective)
                                        .is_some_and(|score| range_i(range, i64::from(*score)))
                            })
                        })
                    })
                }
                "minecraft:entity_properties" => ctx
                    .this
                    .is_some_and(|e| entity(&v["predicate"], e, w, ctx.origin)),
                "minecraft:location_check" => {
                    let base = ctx.block_pos.map_or(ctx.origin, |b| {
                        [
                            f64::from(b[0]) + 0.5,
                            f64::from(b[1]) + 0.5,
                            f64::from(b[2]) + 0.5,
                        ]
                    });
                    let off = [
                        v["offsetX"].as_f64().unwrap_or(0.0),
                        v["offsetY"].as_f64().unwrap_or(0.0),
                        v["offsetZ"].as_f64().unwrap_or(0.0),
                    ];
                    location(
                        &v["predicate"],
                        [base[0] + off[0], base[1] + off[1], base[2] + off[2]],
                        w,
                    )
                }
                "minecraft:match_block" => ctx.block_pos.is_some_and(|b| block(v, b, w)),
                "minecraft:block_state_property" => ctx.block_pos.is_some_and(|b| {
                    let mut pred = serde_json::Map::new();
                    pred.insert("blocks".into(), v["block"].clone());
                    if !v["properties"].is_null() {
                        pred.insert("state".into(), v["properties"].clone());
                    }
                    block(&Value::Object(pred), b, w)
                }),
                "minecraft:match_tool" => ctx.tool.is_some_and(|t| item(&v["predicate"], t, w)),
                "minecraft:weather_check" => {
                    // WeatherCheck.test: absent fields impose no constraint.
                    let (rain, thunder) = w.weather();
                    v["raining"].as_bool().is_none_or(|r| r == rain)
                        && v["thundering"].as_bool().is_none_or(|t| t == thunder)
                }
                _ => false,
            }
        }
        // An inline entity predicate (old-style).
        Value::Object(_) => ctx.this.is_some_and(|e| entity(v, e, w, ctx.origin)),
        // A named predicate reference: its loaded definition, walked like any other.
        Value::String(id) => w
            .predicate(&full(id))
            .is_some_and(|v| conditions_with(v, ctx, &mut |v, ctx| condition_leaf(v, ctx, w))),
        _ => false,
    }
}

/// One node of `TreeNodePosition`, the Walker layout of a display tree.
struct Place {
    adv: usize,
    parent: Option<usize>,
    previous: Option<usize>,
    child_index: i32,
    children: Vec<usize>,
    ancestor: usize,
    thread: Option<usize>,
    x: i32,
    y: f32,
    modifier: f32,
    change: f32,
    shift: f32,
}
struct Layout<'a> {
    tree: &'a Tree,
    kids: &'a [Vec<usize>],
    nodes: Vec<Place>,
}
impl Layout<'_> {
    fn build(
        &mut self,
        adv: usize,
        parent: Option<usize>,
        previous: Option<usize>,
        child_index: i32,
        depth: i32,
    ) -> usize {
        let me = self.nodes.len();
        self.nodes.push(Place {
            adv,
            parent,
            previous,
            child_index,
            children: Vec::new(),
            ancestor: me,
            thread: None,
            x: depth,
            y: -1.0,
            modifier: 0.0,
            change: 0.0,
            shift: 0.0,
        });
        let mut prev = None;
        for &c in &self.kids[adv] {
            prev = self.add_child(me, c, prev);
        }
        me
    }
    fn add_child(&mut self, me: usize, adv: usize, mut prev: Option<usize>) -> Option<usize> {
        if self.tree.list[adv].display.is_some() {
            let index = self.nodes[me].children.len() as i32 + 1;
            let depth = self.nodes[me].x + 1;
            let c = self.build(adv, Some(me), prev, index, depth);
            self.nodes[me].children.push(c);
            Some(c)
        } else {
            for &g in &self.kids[adv] {
                prev = self.add_child(me, g, prev);
            }
            prev
        }
    }
    fn first_walk(&mut self, v: usize) {
        let children = self.nodes[v].children.clone();
        let previous = self.nodes[v].previous;
        if children.is_empty() {
            self.nodes[v].y = previous.map_or(0.0, |p| self.nodes[p].y + 1.0);
            return;
        }
        let mut default_ancestor: Option<usize> = None;
        for &c in &children {
            self.first_walk(c);
            default_ancestor = Some(self.apportion(c, default_ancestor.unwrap_or(c)));
        }
        self.execute_shifts(v);
        let mid = (self.nodes[children[0]].y + self.nodes[children[children.len() - 1]].y) / 2.0;
        if let Some(p) = previous {
            self.nodes[v].y = self.nodes[p].y + 1.0;
            self.nodes[v].modifier = self.nodes[v].y - mid;
        } else {
            self.nodes[v].y = mid;
        }
    }
    fn second_walk(&mut self, v: usize, mod_sum: f32, depth: i32, mut min: f32) -> f32 {
        self.nodes[v].y += mod_sum;
        self.nodes[v].x = depth;
        min = min.min(self.nodes[v].y);
        let m = self.nodes[v].modifier;
        for c in self.nodes[v].children.clone() {
            min = self.second_walk(c, mod_sum + m, depth + 1, min);
        }
        min
    }
    fn third_walk(&mut self, v: usize, offset: f32) {
        self.nodes[v].y += offset;
        for c in self.nodes[v].children.clone() {
            self.third_walk(c, offset);
        }
    }
    fn execute_shifts(&mut self, v: usize) {
        let (mut shift, mut change) = (0.0, 0.0);
        for c in self.nodes[v].children.clone().into_iter().rev() {
            let n = &mut self.nodes[c];
            n.y += shift;
            n.modifier += shift;
            change += n.change;
            shift += n.shift + change;
        }
    }
    fn previous_or_thread(&self, v: usize) -> Option<usize> {
        self.nodes[v]
            .thread
            .or_else(|| self.nodes[v].children.first().copied())
    }
    fn next_or_thread(&self, v: usize) -> Option<usize> {
        self.nodes[v]
            .thread
            .or_else(|| self.nodes[v].children.last().copied())
    }
    fn apportion(&mut self, v: usize, mut default_ancestor: usize) -> usize {
        let Some(previous) = self.nodes[v].previous else {
            return default_ancestor;
        };
        let parent = self.nodes[v].parent.unwrap_or(v);
        let (mut vir, mut vor, mut vil) = (v, v, previous);
        let mut vol = self.nodes[parent].children[0];
        let mut sir = self.nodes[v].modifier;
        let mut sor = sir;
        let mut sil = self.nodes[vil].modifier;
        let mut sol = self.nodes[vol].modifier;
        while let (Some(l), Some(r)) = (self.next_or_thread(vil), self.previous_or_thread(vir)) {
            vil = l;
            vir = r;
            vol = self.previous_or_thread(vol).unwrap_or(vol);
            vor = self.next_or_thread(vor).unwrap_or(vor);
            self.nodes[vor].ancestor = v;
            let shift = self.nodes[vil].y + sil - (self.nodes[vir].y + sir) + 1.0;
            if shift > 0.0 {
                let a = self.ancestor_of(vil, v, default_ancestor);
                self.move_subtree(a, v, shift);
                sir += shift;
                sor += shift;
            }
            sil += self.nodes[vil].modifier;
            sir += self.nodes[vir].modifier;
            sol += self.nodes[vol].modifier;
            sor += self.nodes[vor].modifier;
        }
        if let Some(l) = self.next_or_thread(vil)
            && self.next_or_thread(vor).is_none()
        {
            self.nodes[vor].thread = Some(l);
            self.nodes[vor].modifier += sil - sor;
        } else {
            if let Some(r) = self.previous_or_thread(vir)
                && self.previous_or_thread(vol).is_none()
            {
                self.nodes[vol].thread = Some(r);
                self.nodes[vol].modifier += sir - sol;
            }
            default_ancestor = v;
        }
        default_ancestor
    }
    fn move_subtree(&mut self, me: usize, right: usize, shift: f32) {
        let subtrees = (self.nodes[right].child_index - self.nodes[me].child_index) as f32;
        if subtrees != 0.0 {
            self.nodes[right].change -= shift / subtrees;
            self.nodes[me].change += shift / subtrees;
        }
        let r = &mut self.nodes[right];
        r.shift += shift;
        r.y += shift;
        r.modifier += shift;
    }
    fn ancestor_of(&self, me: usize, other: usize, default_ancestor: usize) -> usize {
        let a = self.nodes[me].ancestor;
        let siblings = self.nodes[other]
            .parent
            .map(|p| &self.nodes[p].children[..])
            .unwrap_or(&[]);
        if siblings.contains(&a) {
            a
        } else {
            default_ancestor
        }
    }
}
impl Tree {
    /// Each advancement's children in `AdvancementTree.addAll` order: the
    /// sorted list, a parent listed after its child inserting it a pass later.
    pub fn children(&self) -> Vec<Vec<usize>> {
        let mut kids = vec![Vec::new(); self.list.len()];
        let mut inserted = vec![false; self.list.len()];
        let mut pending: Vec<usize> = (0..self.list.len()).collect();
        while !pending.is_empty() {
            let before = pending.len();
            pending.retain(|&i| {
                let parent = match &self.list[i].parent {
                    None => None,
                    Some(p) => match self.find_id(p) {
                        Some(p) if inserted[p] => Some(p),
                        _ => return true,
                    },
                };
                if let Some(p) = parent {
                    kids[p].push(i);
                }
                inserted[i] = true;
                false
            });
            if pending.len() == before {
                break;
            }
        }
        kids
    }
    /// `AdvancementVisibilityEvaluator`: done advancements, their
    /// ancestors, and unhidden ones within two levels of a done one.
    pub fn visible(&self, kids: &[Vec<usize>], done: impl Fn(usize) -> bool) -> Vec<bool> {
        #[derive(Clone, Copy, PartialEq)]
        enum Rule {
            Show,
            Hide,
            Same,
        }
        fn walk(
            tree: &Tree,
            kids: &[Vec<usize>],
            done: &dyn Fn(usize) -> bool,
            i: usize,
            stack: &mut Vec<Rule>,
            out: &mut [bool],
        ) -> bool {
            let me = done(i);
            let rule = match &tree.list[i].display {
                None => Rule::Hide,
                Some(_) if me => Rule::Show,
                Some(d) if d.hidden => Rule::Hide,
                Some(_) => Rule::Same,
            };
            let mut any = me;
            stack.push(rule);
            for &c in &kids[i] {
                any |= walk(tree, kids, done, c, stack, out);
            }
            let near = stack
                .iter()
                .rev()
                .take(3)
                .find(|r| **r != Rule::Same)
                .is_some_and(|r| *r == Rule::Show);
            stack.pop();
            out[i] = any || near;
            any
        }
        let mut out = vec![false; self.list.len()];
        for (i, a) in self.list.iter().enumerate() {
            if a.parent.is_none() {
                let mut stack = vec![Rule::Same; 3];
                walk(self, kids, &done, i, &mut stack, &mut out);
            }
        }
        out
    }
    /// `AdvancementTree.repositionNodes`: (column, row) of every displayed
    /// advancement under a displayed root.
    pub fn layout(&self) -> Vec<Option<(i32, f32)>> {
        let kids = self.children();
        let mut out = vec![None; self.list.len()];
        for (i, a) in self.list.iter().enumerate() {
            if a.parent.is_some() || a.display.is_none() {
                continue;
            }
            let mut l = Layout {
                tree: self,
                kids: &kids,
                nodes: Vec::new(),
            };
            let root = l.build(i, None, None, 1, 0);
            l.first_walk(root);
            let y = l.nodes[root].y;
            let min = l.second_walk(root, 0.0, 0, y);
            if min < 0.0 {
                l.third_walk(root, -min);
            }
            for n in &l.nodes {
                out[n.adv] = Some((n.x, n.y));
            }
        }
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn advancement_reward_function_is_optional_and_namespaced() {
        for (raw, expected) in [
            ("parity:reward", "parity:reward"),
            ("reward", "minecraft:reward"),
        ] {
            let input = serde_json::json!({
                "criteria": {"never": {"trigger": "minecraft:impossible"}},
                "rewards": {"function": raw, "experience": 7, "recipes": ["minecraft:bread"]}
            });
            let advancement = parse("parity:test", &input).unwrap();
            assert_eq!(advancement.rewards.function.as_deref(), Some(expected));
            assert_eq!(advancement.rewards.experience, 7);
            assert_eq!(advancement.rewards.recipes, ["minecraft:bread"]);
        }
        let without = parse(
            "parity:empty",
            &serde_json::json!({
                "criteria": {"never": {"trigger": "minecraft:impossible"}}
            }),
        )
        .unwrap();
        assert!(without.rewards.function.is_none());
    }

    /// The criteria an inventory change and a recipe unlock test, on vanilla's tree:
    /// `cargo test -p horizon-client --lib -- --ignored --nocapture index_on_vanilla_tree`
    /// with HORIZON_RESOURCES set to the prepared resources.
    #[test]
    #[ignore = "needs the prepared resources"]
    fn index_on_vanilla_tree() {
        let Some(root) = std::env::var_os("HORIZON_RESOURCES").map(std::path::PathBuf::from) else {
            return;
        };
        let data = root.join("data");
        let t = std::time::Instant::now();
        let tree = Tree::load(&data);
        let load = t.elapsed();
        let tags = data.join("minecraft/tags/item");
        let tag_items: HashMap<String, Vec<String>> = std::fs::read_dir(&tags)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let v: Value =
                    serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok()?;
                let name = format!("minecraft:{}", e.path().file_stem()?.to_string_lossy());
                let values = v["values"]
                    .as_array()?
                    .iter()
                    .filter_map(|x| x.as_str().map(str::to_owned));
                Some((name, values.collect()))
            })
            .collect();
        let t = std::time::Instant::now();
        let index = InventoryIndex::build(&tree, |tag| {
            tag_items
                .get(tag)
                .map(|v| v.iter().map(String::as_str).collect())
                .unwrap_or_default()
        });
        let build = t.elapsed();
        let all = tree
            .by_trigger
            .get("minecraft:inventory_changed")
            .map_or(0, Vec::len);
        let held = [
            "minecraft:iron_ingot",
            "minecraft:oak_log",
            "minecraft:cobblestone",
            "minecraft:diamond_pickaxe",
            "minecraft:bread",
            "minecraft:torch",
            "minecraft:oak_planks",
            "minecraft:stick",
            "minecraft:coal",
        ];
        let t = std::time::Instant::now();
        let mut candidates = 0;
        for _ in 0..1000 {
            candidates = index.candidates(held).len();
        }
        let per_change = t.elapsed() / 1000;
        let recipes = tree
            .by_trigger
            .get("minecraft:recipe_unlocked")
            .map_or(0, Vec::len);
        let per_recipe = tree.recipe_criteria("minecraft:iron_pickaxe").len();
        eprintln!(
            "tree {load:?}; inventory_changed: {all} criteria, index built in {build:?}, \
             a 9-stack inventory tests {candidates} (looked up in {per_change:?}); \
             recipe_unlocked: {recipes} criteria, iron_pickaxe tests {per_recipe}"
        );
        assert!(candidates < all / 10);
    }
    struct Flat;
    impl World for Flat {
        fn dimension(&self) -> &str {
            "minecraft:overworld"
        }
        fn weather(&self) -> (bool, bool) {
            (false, false)
        }
        fn biome(&self, _: [i32; 3]) -> String {
            "minecraft:plains".into()
        }
        fn structures(&self, _: [i32; 3]) -> Vec<String> {
            vec!["minecraft:village_plains".into()]
        }
        fn block(&self, _: [i32; 3]) -> (String, BTreeMap<String, String>) {
            ("minecraft:stone".into(), BTreeMap::new())
        }
        fn fluid(&self, _: [i32; 3]) -> Option<String> {
            None
        }
        fn can_see_sky(&self, _: [i32; 3]) -> bool {
            true
        }
        fn light(&self, _: [i32; 3]) -> i32 {
            15
        }
        fn smokey(&self, _: [i32; 3]) -> bool {
            false
        }
        fn in_tag(&self, _: &str, tag: &str, id: &str) -> bool {
            (tag == "minecraft:stone_tool_materials" && id == "minecraft:cobblestone")
                || (tag == "minecraft:sulfur_cube_archetype/explosive" && id == "minecraft:tnt")
                || (tag == "minecraft:is_projectile"
                    && matches!(id, "minecraft:fireball" | "minecraft:arrow"))
        }
    }
    /// `BlockPredicate.matchesBlockEntity`: an `nbt` test needs a block
    /// entity whose `saveWithFullMetadata` contains the pattern
    /// (`NbtPredicate.matches`), so it never matches a plain block.
    #[test]
    fn a_block_predicates_nbt_needs_the_block_entitys_data() {
        let w = Flat;
        let stone = serde_json::json!({"blocks": "minecraft:stone", "nbt": "{}"});
        assert!(!block(&stone, [0, 0, 0], &w), "stone has no block entity");
        assert!(block(
            &serde_json::json!({"blocks": "minecraft:stone"}),
            [0, 0, 0],
            &w
        ));
        /// An unpowered beacon: its saved data has `Levels:0`.
        struct Beacon;
        impl World for Beacon {
            fn dimension(&self) -> &str {
                Flat.dimension()
            }
            fn weather(&self) -> (bool, bool) {
                Flat.weather()
            }
            fn biome(&self, p: [i32; 3]) -> String {
                Flat.biome(p)
            }
            fn structures(&self, p: [i32; 3]) -> Vec<String> {
                Flat.structures(p)
            }
            fn block(&self, _: [i32; 3]) -> (String, BTreeMap<String, String>) {
                ("minecraft:beacon".into(), BTreeMap::new())
            }
            fn fluid(&self, _: [i32; 3]) -> Option<String> {
                None
            }
            fn can_see_sky(&self, _: [i32; 3]) -> bool {
                true
            }
            fn light(&self, _: [i32; 3]) -> i32 {
                15
            }
            fn smokey(&self, _: [i32; 3]) -> bool {
                false
            }
            fn in_tag(&self, _: &str, _: &str, _: &str) -> bool {
                false
            }
            fn block_entity_nbt(&self, _: [i32; 3]) -> Option<horizon_world::nbt::Tag> {
                horizon_world::nbt::parse_snbt(r#"{id:"minecraft:beacon",x:0,y:0,z:0,Levels:0}"#)
                    .ok()
                    .map(|(t, _)| t)
            }
        }
        let beacon = |nbt: &str| serde_json::json!({"blocks": "minecraft:beacon", "nbt": nbt});
        assert!(block(&beacon("{Levels:0}"), [0, 0, 0], &Beacon));
        assert!(block(&beacon("{}"), [0, 0, 0], &Beacon));
        assert!(!block(&beacon("{Levels:4}"), [0, 0, 0], &Beacon));
        assert!(
            !block(&beacon("{Levels:0b}"), [0, 0, 0], &Beacon),
            "an int, not a byte"
        );
    }
    #[test]
    fn weather_conditions_leave_absent_fields_unconstrained_and_compose() {
        let ctx = Entities {
            this: None,
            origin: [0.0; 3],
            tool: None,
            block_pos: None,
        };
        for (mut predicate, expected) in [
            (serde_json::json!({}), true),
            (serde_json::json!({"raining": false}), true),
            (serde_json::json!({"raining": true}), false),
            (serde_json::json!({"thundering": false}), true),
            (serde_json::json!({"thundering": true}), false),
            (
                serde_json::json!({"raining": false, "thundering": false}),
                true,
            ),
            (
                serde_json::json!({"raining": false, "thundering": true}),
                false,
            ),
        ] {
            predicate["type"] = "minecraft:weather_check".into();
            assert_eq!(conditions(&predicate, &ctx, &Flat), expected, "{predicate}");
        }
        let dry = serde_json::json!({"type": "minecraft:weather_check", "raining": false});
        let wet = serde_json::json!({"type": "minecraft:inverted", "term": dry});
        assert!(!conditions(&wet, &ctx, &Flat));
        assert!(conditions(
            &serde_json::json!({"type": "minecraft:all_of", "terms": [dry]}),
            &ctx,
            &Flat
        ));
        assert!(!conditions(
            &serde_json::json!({"type": "minecraft:any_of", "terms": [wet]}),
            &ctx,
            &Flat
        ));
    }

    /// A `recipe_crafted` criterion names `minecraft:x`; the craft hands
    /// the recipe book and the trigger the same whole id (`award_crafted`
    /// qualifies). The bare name the crafting copy uses never matched —
    /// the second half of #7186.
    #[test]
    fn a_recipe_crafted_criterion_matches_the_whole_id_only() {
        let w = Flat;
        let criterion = serde_json::json!({"recipes": "minecraft:folder_recipe"});
        assert!(
            !id_matches(&criterion["recipes"], "recipe", "folder_recipe", &w),
            "the crafting copy's bare name never matched the criterion"
        );
        assert!(
            id_matches(
                &criterion["recipes"],
                "recipe",
                &crate::crafting::qualified("folder_recipe"),
                &w
            ),
            "the whole id award_crafted passes matches"
        );
    }

    /// `LockCode.canUnlock` for a chest with
    /// `lock:{items:"minecraft:gold_ingot"}`: a gold ingot opens it, an empty
    /// hand or another item does not, and a spectator always does.
    #[test]
    fn a_lock_takes_its_item_and_any_spectator() {
        let lock = serde_json::json!({"items": "minecraft:gold_ingot"});
        let gold = ItemView {
            id: "minecraft:gold_ingot".into(),
            count: 1,
            ..Default::default()
        };
        let stick = ItemView {
            id: "minecraft:stick".into(),
            count: 1,
            ..Default::default()
        };
        assert!(can_unlock(&lock, false, Some(&gold), &Flat));
        assert!(!can_unlock(&lock, false, Some(&stick), &Flat));
        assert!(!can_unlock(&lock, false, None, &Flat));
        assert!(can_unlock(&lock, true, None, &Flat));
        assert!(can_unlock(&lock, true, Some(&stick), &Flat));
        // `LockCode.NO_LOCK` is the empty predicate: it takes any stack, and
        // `unlocksWith(EMPTY)` too.
        let none = serde_json::json!({});
        assert!(can_unlock(&none, false, None, &Flat));
    }
    /// `ItemPredicate`'s `components` and `predicates` are keyed by registry
    /// ids, so a key without a namespace is a `minecraft:` one. An unread key
    /// used to be skipped, which let every stack pass.
    #[test]
    fn item_predicate_reads_bare_component_keys() {
        let potion = ItemView {
            id: "minecraft:potion".into(),
            count: 1,
            potion: Some("minecraft:swiftness".into()),
            ..Default::default()
        };
        let sword = ItemView {
            id: "minecraft:iron_sword".into(),
            count: 1,
            damage: 3,
            enchantments: vec![("minecraft:sharpness".into(), 2)],
            ..Default::default()
        };
        let cases = [
            (
                serde_json::json!({"components": {"potion_contents": "minecraft:swiftness"}}),
                serde_json::json!({"components": {"potion_contents": "minecraft:healing"}}),
                &potion,
            ),
            (
                serde_json::json!({"components": {"damage": 3}}),
                serde_json::json!({"components": {"damage": 4}}),
                &sword,
            ),
            (
                serde_json::json!({"predicates": {"enchantments": [
                    {"enchantments": "sharpness", "levels": 2}
                ]}}),
                serde_json::json!({"predicates": {"enchantments": [
                    {"enchantments": "sharpness", "levels": {"min": 3}}
                ]}}),
                &sword,
            ),
            (
                serde_json::json!({"predicates": {"damage": {"damage": {"max": 3}}}}),
                serde_json::json!({"predicates": {"damage": {"damage": {"min": 4}}}}),
                &sword,
            ),
        ];
        for (yes, no, stack) in cases {
            assert!(item(&yes, stack, &Flat), "{yes} should match");
            assert!(!item(&no, stack, &Flat), "{no} should not match");
        }
        // The potion is a registry id too.
        let bare = serde_json::json!({"components": {"minecraft:potion_contents": "swiftness"}});
        assert!(item(&bare, &potion, &Flat));
    }
    /// `PassengerPredicate.matches`: ride_a_boat_with_a_goat's vehicle holds a goat.
    #[test]
    fn passenger_predicate_needs_a_matching_passenger() {
        let want = serde_json::json!({
            "minecraft:entity_type": "minecraft:oak_boat",
            "minecraft:passenger": {"minecraft:entity_type": "minecraft:goat"}
        });
        let entity = |kind: &str| EntityView {
            kind: kind.into(),
            ..Default::default()
        };
        let mut boat = entity("minecraft:oak_boat");
        assert!(!super::entity(&want, &boat, &Flat, [0.0; 3]));
        boat.passengers = vec![entity("minecraft:player")];
        assert!(!super::entity(&want, &boat, &Flat, [0.0; 3]));
        boat.passengers.push(entity("minecraft:goat"));
        assert!(super::entity(&want, &boat, &Flat, [0.0; 3]));
    }
    /// `EntityNbtPredicate.matches`: the entity's saved compound holds the
    /// pattern (`NbtUtils.compareNbt(…, true)`); without one it fails.
    #[test]
    fn an_entity_predicates_nbt_tests_the_saved_compound() {
        let snbt = |s: &str| horizon_world::nbt::parse_snbt(s).unwrap().0;
        let view = |kind: &str, nbt: &str| EntityView {
            kind: kind.into(),
            nbt: Some(snbt(nbt)),
            ..Default::default()
        };
        let check = |v: serde_json::Value, e: &EntityView| super::entity(&v, e, &Flat, [0.0; 3]);
        let nbt = |s: &str| serde_json::json!({"minecraft:nbt": s});
        let axolotl = view("minecraft:axolotl", "{Variant:1,FromBucket:1b}");
        assert!(check(nbt("{Variant:1}"), &axolotl));
        assert!(check(nbt("{}"), &axolotl));
        assert!(!check(nbt("{Variant:0}"), &axolotl));
        assert!(
            !check(nbt("{FromBucket:1}"), &axolotl),
            "a byte, not an int"
        );
        let pig = serde_json::json!({"minecraft:entity_type": "minecraft:pig",
            "minecraft:nbt": "{Variant:1}"});
        assert!(
            !check(pig, &axolotl),
            "the other sub-predicates still apply"
        );
        let villager = view(
            "minecraft:villager",
            r#"{VillagerData:{level:2,profession:"minecraft:farmer",type:"minecraft:taiga"}}"#,
        );
        assert!(check(
            nbt(r#"{VillagerData:{profession:"minecraft:farmer"}}"#),
            &villager
        ));
        assert!(!check(
            nbt(r#"{VillagerData:{profession:"minecraft:librarian"}}"#),
            &villager
        ));
        let player = view(
            "minecraft:player",
            r#"{Inventory:[{Slot:0b,id:"minecraft:spyglass",count:1},{Slot:3b,id:"minecraft:stick",count:2}],SelectedItem:{id:"minecraft:spyglass",count:1}}"#,
        );
        assert!(check(
            nbt(r#"{Inventory:[{id:"minecraft:stick"}]}"#),
            &player
        ));
        assert!(!check(
            nbt(r#"{Inventory:[{id:"minecraft:stick",count:3}]}"#),
            &player
        ));
        assert!(
            !check(nbt("{Inventory:[]}"), &player),
            "an empty list wants an empty one"
        );
        assert!(check(
            nbt(r#"{SelectedItem:{id:"minecraft:spyglass"}}"#),
            &player
        ));
        let bare = EntityView {
            kind: "minecraft:axolotl".into(),
            ..Default::default()
        };
        assert!(!check(nbt("{}"), &bare), "no compound known");
        assert!(!check(nbt("{Variant:"), &axolotl), "unparsable");
        assert!(!check(serde_json::json!({"minecraft:nbt": 3}), &axolotl));
    }
    /// Return to Sender's `killing_blow`: only the ghast's own fireball, as a
    /// projectile, kills it for the advancement; a sword or an arrow does not.
    #[test]
    fn killing_blow_needs_the_fireball() {
        let blow = serde_json::json!({
            "direct_entity": {"minecraft:entity_type": "minecraft:fireball"},
            "tags": [{"expected": true, "id": "#minecraft:is_projectile"}]
        });
        let entity = |kind: &str| EntityView {
            kind: kind.into(),
            ..Default::default()
        };
        let player = entity("minecraft:player");
        let fireball = DamageView {
            kind: "minecraft:fireball".into(),
            direct: Some(entity("minecraft:fireball")),
            source: Some(player.clone()),
            ..Default::default()
        };
        assert!(damage_source(&blow, &fireball, &Flat, [0.0; 3]));
        let sword = DamageView {
            kind: "minecraft:player_attack".into(),
            direct: Some(player.clone()),
            source: Some(player.clone()),
            is_direct: true,
            ..Default::default()
        };
        assert!(!damage_source(&blow, &sword, &Flat, [0.0; 3]));
        let arrow = DamageView {
            kind: "minecraft:arrow".into(),
            direct: Some(entity("minecraft:arrow")),
            source: Some(player),
            ..Default::default()
        };
        assert!(!damage_source(&blow, &arrow, &Flat, [0.0; 3]));
        assert!(damage_source(&Value::Null, &sword, &Flat, [0.0; 3]));
    }
    fn mob(kind: &str) -> EntityView {
        EntityView {
            kind: kind.into(),
            ..Default::default()
        }
    }
    /// `TeamPredicate.matches`: the effective team's name, exactly.
    #[test]
    fn team_predicate_needs_the_exact_team_name() {
        let team = |v: &str| serde_json::json!({ v: "audit_team" });
        let member = EntityView {
            team: Some("audit_team".into()),
            ..mob("minecraft:zombie")
        };
        for key in ["team", "minecraft:team"] {
            assert!(entity(&team(key), &member, &Flat, [0.0; 3]));
        }
        let other = EntityView {
            team: Some("Audit_Team".into()),
            ..mob("minecraft:zombie")
        };
        assert!(!entity(&team("team"), &other, &Flat, [0.0; 3]));
        assert!(!entity(
            &team("team"),
            &mob("minecraft:zombie"),
            &Flat,
            [0.0; 3]
        ));
    }
    /// `entity_killed_player` by a mob's melee hit: the mob is the killer and
    /// the blow's causing and direct entity.
    #[test]
    fn entity_killed_player_by_a_mob_names_the_mob() {
        let c = serde_json::json!({
            "entity": {"minecraft:entity_type": "minecraft:polar_bear"},
            "killing_blow": {"source_entity": {"minecraft:entity_type": "minecraft:polar_bear"}}
        });
        let me = mob("minecraft:player");
        let bear = mob("minecraft:polar_bear");
        let blow = recorded_blow("minecraft:mob_attack".into(), Some(bear.clone()), [0.0; 3]);
        assert!(blow.is_direct);
        assert_eq!(
            blow.direct.as_ref().map(|e| e.kind.as_str()),
            Some("minecraft:polar_bear")
        );
        assert!(killed(&c, &me, &bear, Some(&blow), &Flat));
        let panda = mob("minecraft:panda");
        let blow = recorded_blow("minecraft:mob_attack".into(), Some(panda.clone()), [0.0; 3]);
        assert!(!killed(&c, &me, &panda, Some(&blow), &Flat));
    }
    /// `entity_killed_player` by a skeleton's arrow: the shooter is the
    /// killer and the blow's causing entity, the arrow its direct one.
    #[test]
    fn entity_killed_player_by_an_arrow_names_the_shooter() {
        let c = serde_json::json!({
            "entity": {"minecraft:entity_type": "minecraft:skeleton"},
            "killing_blow": {
                "direct_entity": {"minecraft:entity_type": "minecraft:arrow"},
                "source_entity": {"minecraft:entity_type": "minecraft:skeleton"},
                "is_direct": false
            }
        });
        let me = mob("minecraft:player");
        let skeleton = mob("minecraft:skeleton");
        let blow = recorded_blow(
            "minecraft:arrow".into(),
            Some(skeleton.clone()),
            [1.0, 2.0, 3.0],
        );
        assert!(!blow.is_direct);
        assert_eq!(
            blow.direct.as_ref().map(|e| e.position),
            Some([1.0, 2.0, 3.0])
        );
        assert!(killed(&c, &me, &skeleton, Some(&blow), &Flat));
        // Shot point-blank, not with an arrow: the direct entity is the skeleton.
        let melee = recorded_blow(
            "minecraft:mob_attack".into(),
            Some(skeleton.clone()),
            [0.0; 3],
        );
        assert!(!killed(&c, &me, &skeleton, Some(&melee), &Flat));
    }
    /// A blow with no causing entity fails a `source_entity` test, and with
    /// no blow at all any `killing_blow` fails.
    #[test]
    fn a_blow_with_no_killer_fails_its_entity_tests() {
        let c = serde_json::json!({
            "killing_blow": {"source_entity": {"minecraft:entity_type": "minecraft:zombie"}}
        });
        let me = mob("minecraft:player");
        let zombie = mob("minecraft:zombie");
        let fall = recorded_blow("minecraft:fall".into(), None, [0.0; 3]);
        assert!(fall.direct.is_none() && fall.source.is_none());
        assert!(!killed(&c, &me, &zombie, Some(&fall), &Flat));
        assert!(!killed(&c, &me, &zombie, None, &Flat));
        assert!(killed(&serde_json::json!({}), &me, &zombie, None, &Flat));
    }
    #[test]
    fn uh_oh_accepts_either_tnt_event_only_for_adult_sulfur_cubes() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../.local/horizon-client/26.3/data/minecraft/advancement/husbandry/uh_oh.json",
        );
        if !path.exists() {
            return;
        }
        let json = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let advancement = parse("minecraft:husbandry/uh_oh", &json).unwrap();
        assert_eq!(advancement.criteria.len(), 2);
        let player = EntityView {
            kind: "minecraft:player".into(),
            ..Default::default()
        };
        let mut target = EntityView {
            kind: "minecraft:sulfur_cube".into(),
            ..Default::default()
        };
        let mut stack = ItemView {
            id: "minecraft:tnt".into(),
            count: 1,
            ..Default::default()
        };
        for criterion in &advancement.criteria {
            assert!(item_entity(
                &criterion.conditions,
                &player,
                &target,
                &stack,
                &Flat
            ));
            let mut progress = Progress::default();
            progress.grant(&advancement.id, &criterion.name, "test");
            assert!(
                progress.done(&advancement),
                "either criterion must award Uh Oh"
            );
            target.baby = true;
            assert!(!item_entity(
                &criterion.conditions,
                &player,
                &target,
                &stack,
                &Flat
            ));
            target.baby = false;
            target.kind = "minecraft:pig".into();
            assert!(!item_entity(
                &criterion.conditions,
                &player,
                &target,
                &stack,
                &Flat
            ));
            target.kind = "minecraft:sulfur_cube".into();
            stack.id = "minecraft:stone".into();
            assert!(!item_entity(
                &criterion.conditions,
                &player,
                &target,
                &stack,
                &Flat
            ));
            stack.id = "minecraft:tnt".into();
        }
    }
    /// Only the criteria the held items could pass are tested; those that
    /// need no particular item always are.
    #[test]
    fn inventory_index_keeps_the_criteria_the_items_could_pass() {
        let criteria = [
            serde_json::json!({"items": [{"items": "minecraft:diamond"}]}),
            serde_json::json!({"items": [{"items": "#minecraft:planks"}]}),
            serde_json::json!({"items": [{"count": {"min": 2}}]}),
            serde_json::json!({"items": [{"count": 1}, {"items": ["minecraft:stick", "minecraft:apple"]}]}),
            serde_json::json!({"slots": {"empty": 0}}),
            serde_json::json!({"items": [{"items": "#minecraft:no_such_tag"}]}),
        ];
        let mut tree = Tree::default();
        for (i, conditions) in criteria.into_iter().enumerate() {
            tree.list.push(Advancement {
                id: format!("test:{i}"),
                parent: None,
                display: None,
                criteria: vec![Criterion {
                    name: "c".into(),
                    trigger: "minecraft:inventory_changed".into(),
                    conditions: LazyJson::new(&conditions),
                }],
                requirements: vec![vec!["c".into()]],
                rewards: Rewards::default(),
            });
            tree.by_trigger
                .entry("minecraft:inventory_changed".into())
                .or_default()
                .push((i, 0));
        }
        let index = InventoryIndex::build(&tree, |tag| match tag {
            "minecraft:planks" => vec!["minecraft:oak_planks", "minecraft:birch_planks"],
            _ => Vec::new(),
        });
        let found = |held: &[&str]| -> Vec<usize> {
            index
                .candidates(held.iter().copied())
                .into_iter()
                .map(|(ai, _)| ai)
                .collect()
        };
        assert_eq!(found(&[]), [2, 4]);
        assert_eq!(found(&["minecraft:dirt"]), [2, 4]);
        assert_eq!(
            found(&["minecraft:oak_planks", "minecraft:diamond"]),
            [0, 1, 2, 4]
        );
        assert_eq!(found(&["minecraft:apple", "minecraft:apple"]), [2, 3, 4]);
    }
    /// A recipe unlocked tests only the criteria that name it.
    #[test]
    fn recipe_index_keeps_the_criteria_of_the_recipe() {
        let mut tree = Tree::default();
        for (i, conditions) in [
            serde_json::json!({"recipes": "minecraft:oak_planks"}),
            serde_json::json!({"recipes": "minecraft:stick"}),
            serde_json::json!({}),
            serde_json::json!({"recipes": "minecraft:oak_planks"}),
        ]
        .into_iter()
        .enumerate()
        {
            tree.list.push(Advancement {
                id: format!("test:{i}"),
                parent: None,
                display: None,
                criteria: vec![Criterion {
                    name: "c".into(),
                    trigger: "minecraft:recipe_unlocked".into(),
                    conditions: LazyJson::new(&conditions),
                }],
                requirements: vec![vec!["c".into()]],
                rewards: Rewards::default(),
            });
            tree.by_trigger
                .entry("minecraft:recipe_unlocked".into())
                .or_default()
                .push((i, 0));
        }
        tree.index_recipes();
        assert_eq!(
            tree.recipe_criteria("minecraft:oak_planks"),
            [(0, 0), (2, 0), (3, 0)]
        );
        assert_eq!(tree.recipe_criteria("minecraft:stick"), [(1, 0), (2, 0)]);
        assert_eq!(tree.recipe_criteria("minecraft:torch"), [(2, 0)]);
    }
    #[test]
    fn bucket_axolotl_block_advancements_require_the_placed_block() {
        let ctx = Entities {
            this: None,
            origin: [0.0; 3],
            tool: None,
            block_pos: Some([0; 3]),
        };
        let stone = serde_json::json!({"type":"minecraft:match_block","blocks":"minecraft:stone"});
        let wheat = serde_json::json!({"type":"minecraft:match_block","blocks":"minecraft:wheat"});
        assert!(conditions(&stone, &ctx, &Flat));
        assert!(!conditions(&wheat, &ctx, &Flat));
        let absent = Entities {
            block_pos: None,
            ..ctx
        };
        assert!(!conditions(&stone, &absent, &Flat));
    }
    /// Data pack ids, tags and registry keys written without a namespace are
    /// `minecraft:` ones (`Identifier.parse`), wherever a predicate reads one.
    #[test]
    fn bare_ids_and_tags_mean_minecraft() {
        let w = Flat;
        let player = EntityView {
            kind: "minecraft:player".into(),
            position: [0.5, 64.0, 0.5],
            effects: vec![("minecraft:speed".into(), 0, 100)],
            ..Default::default()
        };
        let stone = ItemView {
            id: "minecraft:cobblestone".into(),
            count: 1,
            ..Default::default()
        };
        let ctx = Entities {
            this: Some(&player),
            origin: player.position,
            tool: None,
            block_pos: None,
        };
        let arrow = DamageView {
            kind: "minecraft:arrow".into(),
            ..Default::default()
        };
        let o = [0.0; 3];
        let j = |s: &str| serde_json::from_str::<Value>(s).unwrap();
        let cases = [
            (
                "item id",
                id_matches(&j(r#""cobblestone""#), "item", &stone.id, &w),
            ),
            (
                "item tag",
                id_matches(&j(r##""#stone_tool_materials""##), "item", &stone.id, &w),
            ),
            (
                "item list",
                id_matches(&j(r#"["dirt", "cobblestone"]"#), "item", &stone.id, &w),
            ),
            (
                "item predicate",
                item(&j(r#"{"items": "cobblestone"}"#), &stone, &w),
            ),
            (
                "dimension",
                location(&j(r#"{"dimension": "overworld"}"#), player.position, &w),
            ),
            (
                "biome",
                location(&j(r#"{"biomes": "plains"}"#), player.position, &w),
            ),
            (
                "entity type",
                entity(&j(r#"{"minecraft:entity_type": "player"}"#), &player, &w, o),
            ),
            (
                "sub-predicate key",
                entity(&j(r#"{"entity_type": "minecraft:player"}"#), &player, &w, o),
            ),
            (
                "effect key",
                entity(
                    &j(r#"{"minecraft:effects": {"speed": {}}}"#),
                    &player,
                    &w,
                    o,
                ),
            ),
            (
                "damage tag",
                damage_source(
                    &j(r##"{"tags": [{"id": "#is_projectile", "expected": true}]}"##),
                    &arrow,
                    &w,
                    o,
                ),
            ),
            (
                "condition type",
                conditions(
                    &j(r#"{"type": "entity_properties", "entity": "this",
                        "predicate": {"minecraft:entity_type": "minecraft:player"}}"#),
                    &ctx,
                    &w,
                ),
            ),
            // `TagPredicate.id` is a holder set: one damage type is that type.
            (
                "damage type",
                damage_source(&j(r#"{"tags": [{"id": "arrow"}]}"#), &arrow, &w, o),
            ),
            // Near misses stay misses.
            (
                "other item",
                !id_matches(&j(r#"["dirt", "stone"]"#), "item", &stone.id, &w),
            ),
            (
                "other dimension",
                !location(&j(r#"{"dimension": "the_nether"}"#), player.position, &w),
            ),
            (
                "other effect",
                !entity(&j(r#"{"effects": {"slowness": {}}}"#), &player, &w, o),
            ),
            (
                "other damage type",
                !damage_source(&j(r#"{"tags": [{"id": "fireball"}]}"#), &arrow, &w, o),
            ),
            (
                "the type key is no sub-predicate",
                !entity(&j(r#"{"type": "player"}"#), &player, &w, o),
            ),
        ];
        let failed: Vec<&str> = cases.iter().filter(|c| !c.1).map(|c| c.0).collect();
        assert!(
            failed.is_empty(),
            "bare ids not read as minecraft: {failed:?}"
        );
    }
    /// A bare trigger, item or recipe id reaches the indexes as `minecraft:`.
    #[test]
    fn bare_ids_reach_the_trigger_and_item_indexes() {
        let a = parse(
            "test:a",
            &serde_json::json!({"criteria": {
                "items": {"trigger": "inventory_changed",
                    "conditions": {"items": [{"items": ["cobblestone", "#planks"]}]}},
                "recipe": {"trigger": "recipe_unlocked", "conditions": {"recipes": "stick"}}
            }}),
        )
        .unwrap();
        let mut tree = Tree::default();
        for (ci, c) in a.criteria.iter().enumerate() {
            tree.by_trigger
                .entry(c.trigger.clone())
                .or_default()
                .push((0, ci));
        }
        tree.list.push(a);
        tree.index_recipes();
        let index = InventoryIndex::build(&tree, |tag| match tag {
            "minecraft:planks" => vec!["minecraft:oak_planks"],
            _ => Vec::new(),
        });
        let mut failed = Vec::new();
        if !tree.by_trigger.contains_key("minecraft:inventory_changed") {
            failed.push("trigger");
        }
        if index.candidates(["minecraft:cobblestone"]).is_empty() {
            failed.push("item index id");
        }
        if index.candidates(["minecraft:oak_planks"]).is_empty() {
            failed.push("item index tag");
        }
        if tree.recipe_criteria("minecraft:stick").is_empty() {
            failed.push("recipe index");
        }
        assert!(
            failed.is_empty(),
            "bare ids not read as minecraft: {failed:?}"
        );
    }
    #[test]
    fn predicates_match_tags_ranges_and_locations() {
        let w = Flat;
        let stone = ItemView {
            id: "minecraft:cobblestone".into(),
            count: 3,
            ..Default::default()
        };
        assert!(item(
            &serde_json::json!({"items": "#minecraft:stone_tool_materials"}),
            &stone,
            &w
        ));
        assert!(!item(
            &serde_json::json!({"items": "minecraft:dirt"}),
            &stone,
            &w
        ));
        assert!(item(&serde_json::json!({"count": {"min": 2}}), &stone, &w));
        let player = EntityView {
            kind: "minecraft:player".into(),
            position: [0.5, 64.0, 0.5],
            ..Default::default()
        };
        let cond = serde_json::json!({
            "type": "minecraft:entity_properties",
            "entity": "this",
            "predicate": {"minecraft:location": {"structures": "minecraft:village_plains"}}
        });
        let ctx = Entities {
            this: Some(&player),
            origin: player.position,
            tool: None,
            block_pos: None,
        };
        assert!(conditions(&cond, &ctx, &w));
        let mut p = Progress::default();
        let a = parse(
            "minecraft:t",
            &serde_json::json!({"criteria": {"a": {"trigger": "minecraft:tick"}, "b": {"trigger": "minecraft:tick"}},
                "requirements": [["a", "b"]]}),
        )
        .unwrap();
        assert!(!p.done(&a));
        p.grant("minecraft:t", "b", "now");
        assert!(p.done(&a));
    }
    /// `AdvancementRequirements.test` answers false with no requirement groups, so an advancement
    /// without criteria is never done by `isDone`; `PlayerAdvancements.checkForAutomaticTriggers`
    /// still grants its rewards as progress loads, and `Tree::automatic` is the list it walks.
    #[test]
    fn an_advancement_without_criteria_is_not_done_but_granted_automatically() {
        let empty = parse("minecraft:free", &serde_json::json!({"criteria": {}})).unwrap();
        assert!(!Progress::default().done(&empty), "no groups: not done");
        let waiting = parse(
            "minecraft:waits",
            &serde_json::json!({"criteria": {"a": {"trigger": "minecraft:tick"}}}),
        )
        .unwrap();
        assert!(!Progress::default().done(&waiting));
        let tree = tree_of(&[
            (
                "minecraft:waits",
                serde_json::json!({"criteria": {"a": {"trigger": "minecraft:tick"}}}),
            ),
            ("minecraft:free", serde_json::json!({"criteria": {}})),
        ]);
        assert_eq!(tree.automatic(), vec![1]);
    }
    #[test]
    fn layout_spreads_children_like_tree_node_position() {
        let mut tree = Tree::default();
        let shown = Display {
            icon: String::new(),
            title: Value::Null,
            description: Value::Null,
            frame: Frame::Task,
            background: None,
            show_toast: true,
            announce: true,
            hidden: false,
        };
        // Sorted ids; "b" names a parent that sorts after it.
        for (id, parent) in [
            ("a", Some("r")),
            ("b", Some("e")),
            ("c", Some("a")),
            ("d", Some("a")),
            ("e", Some("r")),
            ("r", None),
        ] {
            tree.by_id.insert(id.into(), tree.list.len());
            tree.list.push(Advancement {
                id: id.into(),
                parent: parent.map(Into::into),
                display: Some(shown.clone()),
                criteria: Vec::new(),
                requirements: Vec::new(),
                rewards: Rewards::default(),
            });
        }
        let kids = tree.children();
        assert_eq!(kids[5], vec![0, 4]);
        let l = tree.layout();
        assert_eq!(l[5], Some((0, 1.25)));
        assert_eq!(l[0], Some((1, 0.5)));
        assert_eq!(l[2], Some((2, 0.0)));
        assert_eq!(l[3], Some((2, 1.0)));
        assert_eq!(l[4], Some((1, 2.0)));
        assert_eq!(l[1], Some((2, 2.0)));
    }
    /// A tree of the given advancements, indexed as `load_with` does.
    fn tree_of(list: &[(&str, Value)]) -> Tree {
        let mut tree = Tree::default();
        for (id, v) in list {
            let a = parse(id, v).expect("an advancement");
            for (ci, c) in a.criteria.iter().enumerate() {
                (tree.by_trigger.entry(c.trigger.clone()).or_default()).push((tree.list.len(), ci));
            }
            tree.by_id.insert((*id).into(), tree.list.len());
            tree.list.push(a);
        }
        tree.index_player_facts();
        tree
    }
    /// Whether the player `facts` match `{"type_specific/player": p}`.
    fn player_matches(p: Value, facts: &PlayerFacts) -> bool {
        let me = EntityView {
            kind: "minecraft:player".into(),
            player_facts: facts.clone(),
            ..EntityView::default()
        };
        entity(&json!({"type_specific/player": p}), &me, &Flat, [0.0; 3])
    }
    #[test]
    fn player_level_is_tested_in_its_bounds() {
        let five = PlayerFacts {
            level: Some(5),
            ..PlayerFacts::default()
        };
        for (level, expected) in [
            (json!({"min": 5}), true),
            (json!({"min": 6}), false),
            (json!({"max": 4}), false),
            (json!({"min": 1, "max": 5}), true),
            (json!(5), true),
            (json!(6), false),
        ] {
            assert_eq!(
                player_matches(json!({"level": level}), &five),
                expected,
                "{level}"
            );
        }
        // Not filled: a level asked fails, none asked passes.
        assert!(!player_matches(
            json!({"level": {"min": 0}}),
            &PlayerFacts::default()
        ));
        assert!(player_matches(json!({}), &PlayerFacts::default()));
    }
    #[test]
    fn player_food_tests_level_and_saturation_in_their_bounds() {
        let fed = PlayerFacts {
            food: Some((17, 3.0)),
            ..PlayerFacts::default()
        };
        for (food, expected) in [
            (json!({}), true),
            (json!({"level": {"min": 17}}), true),
            (json!({"level": {"min": 18}}), false),
            (json!({"level": {"max": 16}}), false),
            (json!({"level": 17}), true),
            (json!({"saturation": {"min": 3.0}}), true),
            (json!({"saturation": {"min": 3.5}}), false),
            (json!({"saturation": {"max": 2.9}}), false),
            (
                json!({"level": {"min": 17}, "saturation": {"max": 3.0}}),
                true,
            ),
            (
                json!({"level": {"min": 17}, "saturation": {"min": 3.5}}),
                false,
            ),
            (
                json!({"level": {"min": 18}, "saturation": {"min": 3.0}}),
                false,
            ),
        ] {
            assert_eq!(
                player_matches(json!({"food": food}), &fed),
                expected,
                "{food}"
            );
        }
        // The saturation float widens to a double: 1.2f is 1.2000000476837158.
        let widened = PlayerFacts {
            food: Some((20, 1.2)),
            ..PlayerFacts::default()
        };
        assert!(player_matches(
            json!({"food": {"saturation": {"min": 1.2}}}),
            &widened
        ));
        assert!(!player_matches(
            json!({"food": {"saturation": {"max": 1.2}}}),
            &widened
        ));
        // No food known: food asked fails, even unbounded.
        assert!(!player_matches(
            json!({"food": {}}),
            &PlayerFacts::default()
        ));
    }
    #[test]
    fn player_stats_match_each_counter_by_type_and_id() {
        let mut counts: BTreeMap<String, BTreeMap<String, i32>> = BTreeMap::new();
        (counts.entry("minecraft:custom".into()).or_default())
            .insert("minecraft:walk_one_cm".into(), 1000);
        (counts.entry("minecraft:mined".into()).or_default()).insert("minecraft:stone".into(), 3);
        let facts = PlayerFacts {
            stats: Some(counts),
            ..PlayerFacts::default()
        };
        let stat =
            |ty: &str, id: &str, value: Value| json!({"type": ty, "stat": id, "value": value});
        for (stats, expected) in [
            (
                vec![stat(
                    "minecraft:custom",
                    "minecraft:walk_one_cm",
                    json!({"min": 1000}),
                )],
                true,
            ),
            (
                vec![stat(
                    "minecraft:custom",
                    "minecraft:walk_one_cm",
                    json!({"min": 1001}),
                )],
                false,
            ),
            (
                vec![stat(
                    "minecraft:custom",
                    "minecraft:walk_one_cm",
                    json!({"max": 999}),
                )],
                false,
            ),
            // Bare ids are `minecraft:` ones.
            (
                vec![stat("custom", "walk_one_cm", json!({"min": 1000}))],
                true,
            ),
            // The same id under another type is another counter: never counted, 0.
            (
                vec![stat(
                    "minecraft:mined",
                    "minecraft:walk_one_cm",
                    json!({"min": 1}),
                )],
                false,
            ),
            (
                vec![stat(
                    "minecraft:mined",
                    "minecraft:walk_one_cm",
                    json!({"max": 0}),
                )],
                true,
            ),
            (
                vec![stat("minecraft:killed", "minecraft:zombie", json!(0))],
                true,
            ),
            // Every entry must match.
            (
                vec![
                    stat("minecraft:mined", "minecraft:stone", json!({"min": 3})),
                    stat(
                        "minecraft:custom",
                        "minecraft:walk_one_cm",
                        json!({"min": 5000}),
                    ),
                ],
                false,
            ),
            (
                vec![
                    stat("minecraft:mined", "minecraft:stone", json!({"min": 3})),
                    stat(
                        "minecraft:custom",
                        "minecraft:walk_one_cm",
                        json!({"min": 500}),
                    ),
                ],
                true,
            ),
        ] {
            let p = json!({"stats": stats});
            assert_eq!(player_matches(p.clone(), &facts), expected, "{p}");
        }
        let walked = json!({"stats": [stat("custom", "walk_one_cm", json!({"min": 0}))]});
        assert!(!player_matches(walked, &PlayerFacts::default()));
    }
    #[test]
    fn player_recipes_are_known_or_not_as_asked() {
        let facts = PlayerFacts {
            recipes: Some(["minecraft:iron_pickaxe".to_owned()].into()),
            ..PlayerFacts::default()
        };
        for (recipes, expected) in [
            (json!({"minecraft:iron_pickaxe": true}), true),
            (json!({"iron_pickaxe": true}), true),
            (json!({"minecraft:iron_pickaxe": false}), false),
            (json!({"minecraft:diamond_pickaxe": false}), true),
            (json!({"minecraft:diamond_pickaxe": true}), false),
            (
                json!({"minecraft:iron_pickaxe": true, "minecraft:diamond_pickaxe": true}),
                false,
            ),
        ] {
            assert_eq!(
                player_matches(json!({"recipes": recipes}), &facts),
                expected,
                "{recipes}"
            );
        }
        let unknown = json!({"recipes": {"minecraft:diamond_pickaxe": false}});
        assert!(!player_matches(unknown, &PlayerFacts::default()));
    }
    #[test]
    fn player_advancements_are_done_or_have_their_criteria_as_asked() {
        let tick = json!({"trigger": "minecraft:tick"});
        let tree = Arc::new(tree_of(&[(
            "pack:a",
            json!({"criteria": {"x": tick, "y": tick}}),
        )]));
        let mut progress = Progress::default();
        progress.grant("pack:a", "x", "2026-11-04 12:00:00 +0000");
        let half = PlayerFacts {
            advancements: Some((tree.clone(), progress.clone())),
            ..PlayerFacts::default()
        };
        for (advancements, expected) in [
            (json!({"pack:a": true}), false),
            (json!({"pack:a": false}), true),
            (json!({"pack:a": {"x": true}}), true),
            (json!({"pack:a": {"x": false}}), false),
            (json!({"pack:a": {"y": true}}), false),
            (json!({"pack:a": {"y": false}}), true),
            (json!({"pack:a": {"x": true, "y": true}}), false),
            // A criterion the advancement doesn't have, or an advancement
            // that isn't loaded, matches nothing.
            (json!({"pack:a": {"z": false}}), false),
            (json!({"pack:missing": false}), false),
        ] {
            assert_eq!(
                player_matches(json!({"advancements": advancements}), &half),
                expected,
                "{advancements}"
            );
        }
        progress.grant("pack:a", "y", "2026-11-04 12:00:01 +0000");
        let done = PlayerFacts {
            advancements: Some((tree, progress)),
            ..PlayerFacts::default()
        };
        assert!(player_matches(
            json!({"advancements": {"pack:a": true}}),
            &done
        ));
        assert!(!player_matches(
            json!({"advancements": {"pack:a": false}}),
            &done
        ));
        let asked = json!({"advancements": {"pack:a": false}});
        assert!(!player_matches(asked, &PlayerFacts::default()));
    }
    #[test]
    fn player_facts_are_asked_by_the_triggers_that_name_them() {
        let player = |p: Value| {
            json!({"player": [{"condition": "minecraft:entity_properties", "entity": "this",
                "predicate": {"minecraft:type_specific/player": p}}]})
        };
        let tree = tree_of(&[
            (
                "pack:walk",
                json!({"criteria": {"c": {"trigger": "minecraft:tick",
                    "conditions": player(json!({"stats": [{"type": "minecraft:custom",
                        "stat": "minecraft:walk_one_cm", "value": {"min": 1}}]}))}}}),
            ),
            (
                "pack:milestone",
                json!({"criteria": {"c": {"trigger": "minecraft:location",
                    "conditions": player(json!({"advancements": {"pack:walk": true},
                        "level": {"min": 30}}))}}}),
            ),
            (
                "pack:ref",
                json!({"criteria": {"c": {"trigger": "minecraft:inventory_changed",
                    "conditions": {"player": [{"condition": "minecraft:reference",
                        "name": "pack:p"}]}}}}),
            ),
            (
                "pack:recipe",
                json!({"criteria": {"c": {"trigger": "minecraft:recipe_unlocked",
                    "conditions": {"recipe": "minecraft:iron_pickaxe"}},
                    "d": {"trigger": "minecraft:using_item",
                    "conditions": player(json!({"gamemode": ["survival"]}))}}}),
            ),
        ]);
        let none = FactsAsked::default();
        assert_eq!(
            tree.player_facts_asked("minecraft:tick"),
            FactsAsked {
                stats: true,
                ..none
            }
        );
        assert_eq!(
            tree.player_facts_asked("minecraft:location"),
            FactsAsked {
                advancements: true,
                level: true,
                ..none
            }
        );
        assert_eq!(
            tree.player_facts_asked("minecraft:inventory_changed"),
            FactsAsked::ALL
        );
        assert_eq!(tree.player_facts_asked("minecraft:recipe_unlocked"), none);
        assert_eq!(tree.player_facts_asked("minecraft:using_item"), none);
    }
    #[test]
    fn a_command_predicate_asks_only_the_player_facts_it_names() {
        let player = |p: Value| {
            json!({"condition": "minecraft:entity_properties", "entity": "this",
                "predicate": {"type_specific/player": p}})
        };
        let none = FactsAsked::default();
        for (v, expected) in [
            // Nothing of the player's records: nothing copied.
            (
                json!({"condition": "minecraft:entity_properties", "entity": "this",
                "predicate": {"flags": {"is_sneaking": true}}}),
                none,
            ),
            (player(json!({"gamemode": ["survival"]})), none),
            (
                json!({"condition": "minecraft:random_chance", "chance": 0.5}),
                none,
            ),
            // A field named elsewhere than in a player predicate.
            (
                json!({"function": "minecraft:enchant_with_levels", "levels": 30,
                "conditions": [{"condition": "minecraft:inverted",
                "term": {"condition": "minecraft:entity_properties", "entity": "this",
                "predicate": {"minecraft:effects": {"minecraft:speed": {}}}}}]}),
                none,
            ),
            (
                player(json!({"level": {"min": 3}})),
                FactsAsked {
                    level: true,
                    ..none
                },
            ),
            // Nested in a list, under `inverted`, in an item modifier.
            (
                json!([{"function": "minecraft:set_count", "count": 2, "conditions": [
                    {"condition": "minecraft:inverted", "term": player(json!({
                        "stats": [], "recipes": {"minecraft:bread": true}}))}]}]),
                FactsAsked {
                    stats: true,
                    recipes: true,
                    ..none
                },
            ),
            (
                json!({"condition": "minecraft:all_of", "terms": [player(json!(
                    {"advancements": {"minecraft:story/root": true}}))]}),
                FactsAsked {
                    advancements: true,
                    ..none
                },
            ),
            // A predicate file could ask anything.
            (
                json!({"condition": "reference", "name": "pack:p"}),
                FactsAsked::ALL,
            ),
            (
                json!([{"function": "minecraft:reference", "name": "pack:m"}]),
                FactsAsked::ALL,
            ),
        ] {
            assert_eq!(facts_asked_in(&v), expected, "{v}");
        }
    }
    #[test]
    fn saved_dates_read_back_to_their_instant() {
        for secs in [0, 951_782_400, 1_792_216_800, 4_107_542_399] {
            assert_eq!(date_millis(&date_text(secs)), Some(secs * 1000));
        }
        assert_eq!(
            date_millis("2026-10-17 07:30:00 +0200"),
            date_millis("2026-10-17 05:30:00 +0000")
        );
        assert_eq!(date_millis("not a date"), None);
    }

    #[test]
    fn simulate_progression_advancements() {
        let mut tree = Tree::default();
        let advs = [
            ("minecraft:story/root", None),
            ("minecraft:story/mine_stone", Some("story/root")),
            (
                "minecraft:story/upgrade_tools",
                Some("minecraft:story/mine_stone"),
            ),
            ("minecraft:story/smelt_iron", Some("story/upgrade_tools")),
            (
                "minecraft:story/obtain_armor",
                Some("minecraft:story/smelt_iron"),
            ),
            (
                "minecraft:story/lava_bucket",
                Some("minecraft:story/smelt_iron"),
            ),
            (
                "minecraft:story/iron_tools",
                Some("minecraft:story/smelt_iron"),
            ),
            (
                "minecraft:story/deflect_arrow",
                Some("minecraft:story/obtain_armor"),
            ),
            (
                "minecraft:story/form_obsidian",
                Some("minecraft:story/lava_bucket"),
            ),
            (
                "minecraft:story/mine_diamond",
                Some("minecraft:story/iron_tools"),
            ),
            (
                "minecraft:story/enter_the_nether",
                Some("minecraft:story/form_obsidian"),
            ),
            (
                "minecraft:story/shiny_gear",
                Some("minecraft:story/mine_diamond"),
            ),
            (
                "minecraft:story/enchant_item",
                Some("minecraft:story/mine_diamond"),
            ),
            (
                "minecraft:story/cure_zombie_villager",
                Some("minecraft:story/enter_the_nether"),
            ),
            (
                "minecraft:story/follow_ender_eye",
                Some("minecraft:story/enter_the_nether"),
            ),
            (
                "minecraft:story/enter_the_end",
                Some("minecraft:story/follow_ender_eye"),
            ),
        ];
        for (id, parent) in advs {
            let a = parse(
                id,
                &serde_json::json!({
                    "parent": parent,
                    "criteria": {"crit": {"trigger": "minecraft:tick"}},
                    "display": {"icon": {"id": "minecraft:stone"}, "title": "t", "description": "d"}
                }),
            )
            .unwrap();
            tree.by_id.insert(id.to_string(), tree.list.len());
            tree.list.push(a);
        }
        for i in 16..124 {
            let id = format!("minecraft:custom/{i}");
            let a = parse(
                &id,
                &serde_json::json!({
                    "criteria": {"crit": {"trigger": "minecraft:tick"}},
                    "display": {"icon": {"id": "minecraft:stone"}, "title": "t", "description": "d"}
                }),
            )
            .unwrap();
            tree.by_id.insert(id, tree.list.len());
            tree.list.push(a);
        }

        fn get_list(tree: &Tree, target: Option<&str>, mode: &str) -> Vec<usize> {
            match target {
                None => (0..tree.list.len()).collect(),
                Some(id) => {
                    let i = *tree.by_id.get(id).unwrap();
                    let mut out = Vec::new();
                    if mode == "until" || mode == "through" {
                        let mut p = tree.list[i].parent.as_ref();
                        while let Some(&pi) = p.and_then(|pid| tree.by_id.get(pid)) {
                            out.push(pi);
                            p = tree.list[pi].parent.as_ref();
                        }
                    }
                    out.push(i);
                    if mode == "from" || mode == "through" {
                        let kids = tree.children();
                        fn add(kids: &[Vec<usize>], i: usize, out: &mut Vec<usize>) {
                            for &c in &kids[i] {
                                out.push(c);
                                add(kids, c, out);
                            }
                        }
                        add(&kids, i, &mut out);
                    }
                    out
                }
            }
        }

        fn grant(tree: &Tree, prog: &mut Progress, list: &[usize]) -> i32 {
            let mut count = 0;
            for &i in list {
                let a = &tree.list[i];
                if prog.done(a) {
                    continue;
                }
                for c in &a.criteria {
                    prog.grant(&a.id, &c.name, "now");
                }
                count += 1;
            }
            count
        }

        fn revoke(tree: &Tree, prog: &mut Progress, list: &[usize]) -> i32 {
            let mut count = 0;
            for &i in list {
                let a = &tree.list[i];
                let mut any = false;
                for c in &a.criteria {
                    any |= prog.revoke(&a.id, &c.name);
                }
                if any {
                    count += 1;
                }
            }
            count
        }

        let mut p = Progress::default();
        let until = get_list(&tree, Some("minecraft:story/smelt_iron"), "until");
        assert_eq!(grant(&tree, &mut p, &until), 4);

        let from = get_list(&tree, Some("minecraft:story/root"), "from");
        assert_eq!(grant(&tree, &mut p, &from), 12);

        let through = get_list(&tree, Some("minecraft:story/iron_tools"), "through");
        assert_eq!(revoke(&tree, &mut p, &through), 8);

        let all = get_list(&tree, None, "everything");
        let granted_everything = grant(&tree, &mut p, &all);
        assert_eq!(granted_everything, 116);
    }
    #[test]
    fn random_chance_literal_matches_official_seeded_float_and_continuation() {
        use horizon_core::random::{LegacyRandom, RandomSource};
        let ctx = Entities {
            this: None,
            origin: [0.0; 3],
            tool: None,
            block_pos: None,
        };
        for line in include_str!("../../../docs/parity/random-chance-seeded.jsonl").lines() {
            let row: Value = serde_json::from_str(line).unwrap();
            let mut random = LegacyRandom::new(row["seed"].as_i64().unwrap());
            let result = conditions_with(&row["input"], &ctx, &mut |v, ctx| {
                literal_random_chance(v, &mut || random.next_float())
                    .unwrap_or_else(|| conditions(v, ctx, &Flat))
            });
            assert_eq!(result, row["result"].as_bool().unwrap(), "{row}");
            let next: Vec<i64> = (0..4).map(|_| random.next_long()).collect();
            let expected: Vec<i64> = row["continuation"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_i64().unwrap())
                .collect();
            assert_eq!(next, expected, "{row}");
        }
    }
    #[test]
    fn existing_no_random_context_and_traversal_keep_their_behavior() {
        let ctx = Entities {
            this: None,
            origin: [0.0; 3],
            tool: None,
            block_pos: None,
        };
        let z = json!({"type":"minecraft:random_chance","chance":0});
        let o = json!({"type":"minecraft:random_chance","chance":1});
        for (v, expected) in [
            (Value::Null, true),
            (json!([]), true),
            (z.clone(), false),
            (o.clone(), false),
            (json!({"type":"inverted","term":o}), true),
            (
                json!({"type":"all_of","terms":[z.clone(),Value::Null]}),
                false,
            ),
            (
                json!({"type":"any_of","terms":[z.clone(),Value::Null]}),
                true,
            ),
            (json!("h122:unsupported_reference"), false),
        ] {
            assert_eq!(conditions(&v, &ctx, &Flat), expected, "{v}");
        }
        let mut seen = vec![];
        let v =
            json!({"type":"any_of","terms":[{"type":"all_of","terms":[false,true]},true,false]});
        assert!(conditions_with(&v, &ctx, &mut |v, _| {
            seen.push(v.as_bool().unwrap());
            v.as_bool().unwrap()
        }));
        assert_eq!(seen, vec![false, true]);
    }
    #[test]
    fn unsupported_chance_shapes_keep_prior_false_without_a_draw() {
        let ctx = Entities {
            this: None,
            origin: [0.0; 3],
            tool: None,
            block_pos: None,
        };
        let mut draws = 0;
        for v in [
            json!({"type":"random_chance","chance":{"type":"minecraft:uniform","min":0,"max":1}}),
            json!({"type":"random_chance"}),
            json!({"type":"random_chance","chance":"1"}),
            json!({"type":"random_chance","chance":1,"unsupported":true}),
        ] {
            assert_eq!(
                literal_random_chance(&v, &mut || {
                    draws += 1;
                    0.0
                }),
                None
            );
            assert!(!conditions(&v, &ctx, &Flat));
        }
        assert_eq!(draws, 0);
    }

    #[cfg(test)]
    mod movement_model_tests {
        use super::*;
        use serde_json::json;

        #[test]
        fn added_model_movement_units_exact_bounds_and_squared_bounds() {
            // Source-derived model proof; no moving Java/runtime claim.
            let m = MovementView {
                per_tick: [3.0 / 20.0, -4.0 / 20.0, 0.0],
                fall_distance: 7.25,
            };
            assert_eq!(m.per_tick.map(|n| n * 20.0), [3.0, -4.0, 0.0]);
            for p in [
                json!({"x":3.0,"y":-4.0,"z":0.0}),
                json!({"x":{"min":3.0,"max":3.0}}),
                json!({"speed":5.0,"horizontal_speed":3.0,"vertical_speed":4.0,"fall_distance":7.25}),
                json!({"speed":{"min":5.0,"max":5.0}}),
                // Doubles boundsSqr maps each bound through Mth.square, including its sign.
                json!({"speed":-5.0,"horizontal_speed":{"max":-3.0}}),
                json!({}),
            ] {
                assert!(movement(&p, m), "{p}");
            }
            for p in [
                json!({"x":3.0000000001}),
                json!({"x":{"min":3.0000000001}}),
                json!({"y":{"max":-4.0000000001}}),
                json!({"speed":{"max":4.9999999999}}),
                json!({"horizontal_speed":{"min":-4.0}}),
                json!({"vertical_speed":-4.0}),
                json!({"fall_distance":{"max":7.2499999999}}),
            ] {
                assert!(!movement(&p, m), "{p}");
            }
            assert!(!movement_range(&json!(3.0000000001), 3.0, false));
            assert!(
                range_f(&json!(3.0000000001), 3.0),
                "existing global tolerance is untouched"
            );
        }

        #[test]
        fn added_model_unprojected_is_distinct_from_actual_stationary() {
            let p = json!({"minecraft:movement":{}});
            let view = EntityView {
                kind: "minecraft:pig".into(),
                ..Default::default()
            };
            // Unprojected is explicitly unsupported, not claimed Java-correct.
            assert!(!entity(&p, &view, &super::Flat, [0.0; 3]));
            let view = EntityView {
                movement: Some(MovementView {
                    per_tick: [0.0; 3],
                    fall_distance: 0.0,
                }),
                ..view
            };
            assert!(entity(&p, &view, &super::Flat, [0.0; 3]));
        }
    }
}
