//! A loot table's odds, worked out rather than rolled: what each item's
//! chance is to come from one roll of the table and how many come, for a
//! recipe viewer that lists a fishing spot's or a sieve's outputs
//! (`native_loot_odds`).
//!
//! It follows `LootTable.getRandomItemsRaw` as [`LootData::roll`] does:
//! each pool whose condition holds rolls `rolls` times, each roll picks one
//! of the pool's valid entries by weight (luck 0, so `quality` and
//! `bonus_rolls` count for nothing), and the entry's item modifiers set its
//! count. Chances come from `random_chance` (and the unenchanted chances of
//! `random_chance_with_enchanted_bonus` and `table_bonus`). A `match_tool`
//! is judged with the tool given; with none, and for conditions on the
//! world (weather, a killer, a place), it is taken as met and named in the
//! item's `conditions`. Conditions are taken as independent of each other.
use super::{LootData, int_value};
use serde_json::Value;

/// One item a table can drop and its odds.
#[derive(Clone, Debug, PartialEq)]
pub struct LootOdds {
    pub item: String,
    /// The chance at least one comes from one roll of the table.
    pub chance: f64,
    /// The fewest one drop gives (at least 1) and the most the table gives
    /// at once.
    pub min: u32,
    pub max: u32,
    /// Conditions on its way that couldn't be judged, taken as met.
    pub conditions: Vec<String>,
}

/// The chance a condition holds, and what was taken as met to say so.
#[derive(Clone, Debug, Default)]
struct Chance {
    p: f64,
    notes: Vec<String>,
}
impl Chance {
    fn sure() -> Self {
        Self {
            p: 1.0,
            notes: Vec::new(),
        }
    }
    fn never() -> Self {
        Self {
            p: 0.0,
            notes: Vec::new(),
        }
    }
    fn assumed(note: String) -> Self {
        Self {
            p: 1.0,
            notes: vec![note],
        }
    }
}

/// An entry a roll can pick, with the chance its conditions let it.
struct Choice<'v> {
    entry: &'v Value,
    /// A tag entry's item when the tag is expanded.
    item: Option<String>,
    notes: Vec<String>,
}

/// The ways entries can expand: each one's chance, the choices valid then
/// (indexes) and whether the expansion succeeded.
type Sets = Vec<(f64, Vec<usize>, bool)>;

/// The most ways kept for one pool; past that the unlikeliest are dropped
/// (their chance shared out over the rest).
const MOST_WAYS: usize = 4096;

/// What one picked entry gives: per item, the chance it gives some, and
/// the count range when it does.
#[derive(Clone, Debug)]
struct Gives {
    item: String,
    chance: f64,
    min: u32,
    max: u32,
    notes: Vec<String>,
}

/// A count range and the chance it is 0 or less (no stack).
#[derive(Clone, Copy, Debug)]
struct Count {
    min: i64,
    max: i64,
    zero: f64,
}

impl LootData {
    /// The odds of table `id`, or None when there is no such table. `tool`
    /// is the item in hand that `match_tool` conditions are judged with.
    pub fn odds(&self, id: &str, tool: Option<&str>) -> Option<Vec<LootOdds>> {
        let table = self.tables.get(id)?;
        let mut judge = Judge {
            data: self,
            tool,
            visited: Vec::new(),
        };
        let gives = judge.table(id, table);
        Some(
            gives
                .into_iter()
                .map(|g| LootOdds {
                    item: g.item,
                    chance: g.chance.clamp(0.0, 1.0),
                    min: g.min,
                    max: g.max,
                    conditions: g.notes,
                })
                .collect(),
        )
    }
}

struct Judge<'d> {
    data: &'d LootData,
    tool: Option<&'d str>,
    /// Tables being worked out, so a table naming itself stops.
    visited: Vec<String>,
}

impl Judge<'_> {
    /// Every item one roll of `table` can give, merged across its pools.
    fn table(&mut self, name: &str, table: &Value) -> Vec<Gives> {
        if self.visited.iter().any(|v| v == name) || self.visited.len() > 16 {
            return Vec::new();
        }
        self.visited.push(name.to_owned());
        let table_modifier = &table["modifier"];
        let mut all: Vec<Gives> = Vec::new();
        for pool in table["pools"].as_array().into_iter().flatten() {
            let held = self.condition(&pool["condition"]);
            let held = and(held, self.condition(&pool["conditions"]));
            if held.p <= 0.0 {
                continue;
            }
            for g in self.pool(pool, &[&pool["modifier"], table_modifier]) {
                let g = Gives {
                    chance: g.chance * held.p,
                    notes: merge_notes(&held.notes, &g.notes),
                    ..g
                };
                merge_independent(&mut all, g);
            }
        }
        self.visited.pop();
        all
    }

    /// One pool's items: its rolls, each picking one valid entry by weight.
    fn pool(&mut self, pool: &Value, modifiers: &[&Value]) -> Vec<Gives> {
        let mut choices = Vec::new();
        // Which entries are valid, and how likely each such set is.
        let mut sets: Sets = vec![(1.0, Vec::new(), true)];
        for entry in pool["entries"].as_array().into_iter().flatten() {
            let one = self.expand(entry, &[], &mut choices);
            sets = product(&sets, &one);
        }
        let picked = picked(&choices, &sets);
        // Per roll: each item's chance, and its count range.
        let mut per_roll: Vec<Gives> = Vec::new();
        for (choice, chance) in choices.iter().zip(picked) {
            if chance <= 0.0 {
                continue;
            }
            for g in self.create(choice, modifiers) {
                let g = Gives {
                    chance: g.chance * chance,
                    notes: merge_notes(&choice.notes, &g.notes),
                    ..g
                };
                // One roll picks one entry: the entries' chances add up.
                match per_roll.iter_mut().find(|o| o.item == g.item) {
                    Some(o) => {
                        o.chance += g.chance;
                        o.min = o.min.min(g.min);
                        o.max = o.max.max(g.max);
                        o.notes = merge_notes(&o.notes, &g.notes);
                    }
                    None => per_roll.push(g),
                }
            }
        }
        let rolls = distribution(&pool["rolls"]);
        let most = rolls.iter().map(|(n, _)| *n).max().unwrap_or(0);
        per_roll
            .into_iter()
            .map(|g| {
                let q = g.chance.clamp(0.0, 1.0);
                let chance = rolls
                    .iter()
                    .map(|&(n, p)| p * (1.0 - (1.0 - q).powi(n as i32)))
                    .sum();
                Gives {
                    chance,
                    max: g.max.saturating_mul(most),
                    ..g
                }
            })
            .filter(|g| g.chance > 0.0 && g.max > 0)
            .collect()
    }

    /// `LootPoolEntryContainer.expand`: the ways an entry can expand, each
    /// with its chance, the choices it adds (indexes into `out`) and whether
    /// it expanded, which alternatives and sequences go by.
    fn expand<'v>(
        &mut self,
        entry: &'v Value,
        notes: &[String],
        out: &mut Vec<Choice<'v>>,
    ) -> Sets {
        let held = and(
            self.condition(&entry["condition"]),
            self.condition(&entry["conditions"]),
        );
        if held.p <= 0.0 {
            return vec![(1.0, Vec::new(), false)];
        }
        let notes = merge_notes(notes, &held.notes);
        let children = || entry["children"].as_array().into_iter().flatten();
        let weighty = int_value(&entry["weight"]).unwrap_or(1) > 0;
        let leaf = |out: &mut Vec<Choice<'v>>, item: Option<String>| -> Vec<usize> {
            if !weighty {
                return Vec::new();
            }
            out.push(Choice {
                entry,
                item,
                notes: notes.clone(),
            });
            vec![out.len() - 1]
        };
        let inner: Sets = match entry["type"].as_str().unwrap_or("") {
            "minecraft:alternatives" => {
                // The first child that expands; the next only when it didn't.
                let mut done: Sets = Vec::new();
                let mut open: Sets = vec![(1.0, Vec::new(), false)];
                for child in children() {
                    let ways = self.expand(child, &notes, out);
                    // What came before failed: whether the child expands says.
                    let before: Sets = open.iter().map(|(p, s, _)| (*p, s.clone(), true)).collect();
                    let next = product(&before, &ways);
                    open = Vec::new();
                    for (p, set, ok) in next {
                        if ok {
                            done.push((p, set, true));
                        } else {
                            open.push((p, set, false));
                        }
                    }
                    if open.is_empty() {
                        break;
                    }
                }
                done.extend(open);
                done
            }
            "minecraft:group" => {
                let mut ways: Sets = vec![(1.0, Vec::new(), true)];
                for child in children() {
                    let child = self.expand(child, &notes, out);
                    ways = product(&ways, &child)
                        .into_iter()
                        .map(|(p, set, _)| (p, set, true))
                        .collect();
                }
                ways
            }
            "minecraft:sequence" => {
                let mut done: Sets = Vec::new();
                let mut open: Sets = vec![(1.0, Vec::new(), true)];
                for child in children() {
                    let ways = self.expand(child, &notes, out);
                    let next = product(&open, &ways);
                    open = Vec::new();
                    for (p, set, ok) in next {
                        if ok {
                            open.push((p, set, true));
                        } else {
                            done.push((p, set, false));
                        }
                    }
                }
                done.extend(open);
                done
            }
            "minecraft:tag" if entry["expand"].as_bool().unwrap_or(false) => {
                let mut set = Vec::new();
                for item in self.data.holder_set("item", &entry["items"]) {
                    set.extend(leaf(out, Some(item)));
                }
                vec![(1.0, set, true)]
            }
            "minecraft:item" | "minecraft:empty" | "minecraft:loot_table" | "minecraft:tag" => {
                vec![(1.0, leaf(out, None), true)]
            }
            _ => vec![(1.0, Vec::new(), false)],
        };
        let mut ways: Sets = inner
            .into_iter()
            .map(|(p, set, ok)| (p * held.p, set, ok))
            .collect();
        if held.p < 1.0 {
            ways.push((1.0 - held.p, Vec::new(), false));
        }
        ways
    }

    /// What a picked entry gives, its modifiers' counts applied.
    fn create(&mut self, choice: &Choice<'_>, modifiers: &[&Value]) -> Vec<Gives> {
        let entry = choice.entry;
        let mut modifiers_here = vec![&entry["modifier"]];
        modifiers_here.extend_from_slice(modifiers);
        let one = |judge: &mut Self, item: &str| -> Option<Gives> {
            if item == "minecraft:air" || item.is_empty() {
                return None;
            }
            let mut count = Count {
                min: 1,
                max: 1,
                zero: 0.0,
            };
            for m in &modifiers_here {
                count = judge.modify(m, count);
            }
            (count.max > 0).then(|| Gives {
                item: item.to_owned(),
                chance: 1.0 - count.zero,
                min: count.min.max(1) as u32,
                max: count.max.min(u32::MAX as i64) as u32,
                notes: Vec::new(),
            })
        };
        match entry["type"].as_str().unwrap_or("") {
            "minecraft:item" => one(self, entry["name"].as_str().unwrap_or(""))
                .into_iter()
                .collect(),
            "minecraft:tag" => match &choice.item {
                Some(item) => one(self, item).into_iter().collect(),
                None => self
                    .data
                    .holder_set("item", &entry["items"])
                    .iter()
                    .filter_map(|item| one(self, item))
                    .collect(),
            },
            "minecraft:loot_table" => {
                let gives = match &entry["value"] {
                    Value::String(name) => match self.data.tables.get(name) {
                        Some(table) => self.table(name, table),
                        None => Vec::new(),
                    },
                    inline @ Value::Object(_) => self.table("<inline>", inline),
                    _ => Vec::new(),
                };
                // The entry's own modifiers change the nested table's stacks.
                gives
                    .into_iter()
                    .map(|g| {
                        let mut count = Count {
                            min: g.min as i64,
                            max: g.max as i64,
                            zero: 0.0,
                        };
                        for m in &modifiers_here {
                            count = self.modify(m, count);
                        }
                        Gives {
                            chance: g.chance * (1.0 - count.zero),
                            min: count.min.max(1) as u32,
                            max: count.max.max(0) as u32,
                            ..g
                        }
                    })
                    .filter(|g| g.max > 0)
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// An item modifier's effect on the count (a typed object, a list or
    /// a registry reference); others leave it.
    fn modify(&mut self, m: &Value, count: Count) -> Count {
        match m {
            Value::Array(list) => list.iter().fold(count, |c, f| self.modify(f, c)),
            Value::String(id) => match self.data.modifiers.get(id) {
                Some(f) => self.modify(f, count),
                None => count,
            },
            Value::Object(_) => {
                let held = and(
                    self.condition(&m["condition"]),
                    self.condition(&m["conditions"]),
                );
                if held.p <= 0.0 {
                    return count;
                }
                let changed = self.run(m, count);
                if held.p >= 1.0 {
                    return changed;
                }
                // Sometimes: either count may come.
                Count {
                    min: count.min.min(changed.min),
                    max: count.max.max(changed.max),
                    zero: count.zero * (1.0 - held.p) + changed.zero * held.p,
                }
            }
            _ => count,
        }
    }

    fn run(&mut self, f: &Value, count: Count) -> Count {
        let kind = f["type"]
            .as_str()
            .or_else(|| f["function"].as_str())
            .unwrap_or("");
        match kind {
            "minecraft:sequence" => f["functions"]
                .as_array()
                .into_iter()
                .flatten()
                .fold(count, |c, g| self.modify(g, c)),
            "minecraft:set_count" => {
                let (lo, hi, dist) = int_range(&f["count"]);
                if f["add"].as_bool() == Some(true) {
                    let min = count.min + lo;
                    let max = count.max + hi;
                    // Only the sure part of the zero chance carries over.
                    let zero = if count.min == count.max {
                        dist.iter()
                            .filter(|(v, _)| count.min + v <= 0)
                            .map(|(_, p)| p)
                            .sum()
                    } else {
                        count.zero
                    };
                    Count { min, max, zero }
                } else {
                    Count {
                        min: lo,
                        max: hi,
                        zero: dist.iter().filter(|(v, _)| *v <= 0).map(|(_, p)| p).sum(),
                    }
                }
            }
            "minecraft:limit_count" => {
                let limit = &f["limit"];
                let lo = int_value(&limit["min"]);
                let hi = int_value(&limit["max"]);
                let clamp = |v: i64| {
                    let v = lo.map_or(v, |l| v.max(l));
                    hi.map_or(v, |h| v.min(h))
                };
                Count {
                    min: clamp(count.min),
                    max: clamp(count.max),
                    zero: if lo.is_some_and(|l| l > 0) {
                        0.0
                    } else {
                        count.zero
                    },
                }
            }
            // ApplyBonusCount with no enchantment: only the binomial
            // formula adds anything (`extra` tries).
            "minecraft:apply_bonus"
                if f["formula"].as_str() == Some("minecraft:binomial_with_bonus_count") =>
            {
                let extra = int_value(&f["parameters"]["extra"]).unwrap_or(0).max(0);
                Count {
                    max: count.max + extra,
                    ..count
                }
            }
            _ => count,
        }
    }

    /// The chance a condition holds (null holds).
    fn condition(&mut self, c: &Value) -> Chance {
        match c {
            Value::Null => Chance::sure(),
            Value::Array(list) => list
                .iter()
                .fold(Chance::sure(), |acc, c| and(acc, self.condition(c))),
            Value::String(id) => match self.data.predicates.get(id) {
                Some(p) => self.condition(p),
                // An unknown reference fails, as when rolling.
                None => Chance::never(),
            },
            Value::Object(_) => self.condition_object(c),
            _ => Chance::never(),
        }
    }

    fn condition_object(&mut self, c: &Value) -> Chance {
        let kind = c["type"]
            .as_str()
            .or_else(|| c["condition"].as_str())
            .unwrap_or("");
        let terms = || c["terms"].as_array().into_iter().flatten();
        match kind {
            "minecraft:inverted" => {
                let inner = self.condition(&c["term"]);
                if inner.notes.is_empty() {
                    Chance {
                        p: 1.0 - inner.p,
                        notes: Vec::new(),
                    }
                } else {
                    // Taken as met either way: say the opposite.
                    Chance {
                        p: 1.0,
                        notes: inner.notes.iter().map(|n| negate(n)).collect(),
                    }
                }
            }
            "minecraft:all_of" => {
                let mut acc = Chance::sure();
                for t in terms() {
                    acc = and(acc, self.condition(t));
                }
                acc
            }
            "minecraft:any_of" => {
                let mut none = 1.0;
                let mut notes = Vec::new();
                let mut sure = false;
                for t in terms() {
                    let t = self.condition(t);
                    if t.p >= 1.0 && t.notes.is_empty() {
                        sure = true;
                    }
                    none *= 1.0 - t.p;
                    notes = merge_notes(&notes, &t.notes);
                }
                Chance {
                    p: 1.0 - none,
                    notes: if sure { Vec::new() } else { notes },
                }
            }
            "minecraft:random_chance" => Chance {
                p: number(&c["chance"]).clamp(0.0, 1.0),
                notes: Vec::new(),
            },
            "minecraft:random_chance_with_enchanted_bonus" => Chance {
                p: number(&c["unenchanted_chance"]).clamp(0.0, 1.0),
                notes: Vec::new(),
            },
            "minecraft:table_bonus" => Chance {
                p: c["chances"]
                    .as_array()
                    .and_then(|a| a.first())
                    .map_or(0.0, number)
                    .clamp(0.0, 1.0),
                notes: Vec::new(),
            },
            "minecraft:survives_explosion" => Chance::sure(),
            "minecraft:match_tool" => self.match_tool(&c["predicate"]),
            k => Chance::assumed(describe(k, c)),
        }
    }

    /// `MatchTool` with the tool given, else taken as met.
    fn match_tool(&self, predicate: &Value) -> Chance {
        let Some(tool) = self.tool else {
            return Chance::assumed(match predicate["items"] {
                Value::Null => "needs a certain tool".to_owned(),
                ref items => format!("needs {}", names(items)),
            });
        };
        let items_ok = match &predicate["items"] {
            Value::Null => true,
            set => self.data.item_in(tool, set),
        };
        // A plain tool has no enchantments, components or custom data.
        let only_items = predicate
            .as_object()
            .is_none_or(|o| o.keys().all(|k| k == "items" || k == "count"));
        if items_ok && only_items {
            Chance::sure()
        } else {
            Chance::never()
        }
    }
}

/// Both ways at once (independent): every pair, chances multiplied, sets
/// joined, succeeding when both did; equal sets merged.
fn product(a: &Sets, b: &Sets) -> Sets {
    let mut out: Sets = Vec::new();
    for (pa, sa, oa) in a {
        for (pb, sb, ob) in b {
            let p = pa * pb;
            if p <= 0.0 {
                continue;
            }
            let mut set = sa.clone();
            set.extend(sb.iter().copied());
            let ok = *oa && *ob;
            match out.iter_mut().find(|(_, s, o)| *s == set && *o == ok) {
                Some(way) => way.0 += p,
                None => out.push((p, set, ok)),
            }
        }
    }
    if out.len() > MOST_WAYS {
        out.sort_by(|x, y| y.0.total_cmp(&x.0));
        out.truncate(MOST_WAYS);
        let kept: f64 = out.iter().map(|w| w.0).sum();
        if kept > 0.0 {
            for way in &mut out {
                way.0 /= kept;
            }
        }
    }
    out
}

/// Each choice's chance to be the one a roll picks: by weight among the
/// valid ones, over the ways the pool's entries can expand.
fn picked(choices: &[Choice<'_>], sets: &Sets) -> Vec<f64> {
    let weight = |i: usize| int_value(&choices[i].entry["weight"]).unwrap_or(1).max(0) as f64;
    let mut out = vec![0.0; choices.len()];
    for (p, set, _) in sets {
        let total: f64 = set.iter().map(|&i| weight(i)).sum();
        if total <= 0.0 {
            continue;
        }
        for &i in set {
            out[i] += p * weight(i) / total;
        }
    }
    out
}

/// One item's chance from several pools: at least one of them gives it.
fn merge_independent(all: &mut Vec<Gives>, g: Gives) {
    match all.iter_mut().find(|o| o.item == g.item) {
        Some(o) => {
            o.chance = 1.0 - (1.0 - o.chance) * (1.0 - g.chance);
            o.min = o.min.min(g.min);
            o.max = o.max.saturating_add(g.max);
            o.notes = merge_notes(&o.notes, &g.notes);
        }
        None => all.push(g),
    }
}

fn and(a: Chance, b: Chance) -> Chance {
    Chance {
        p: a.p * b.p,
        notes: merge_notes(&a.notes, &b.notes),
    }
}

fn merge_notes(a: &[String], b: &[String]) -> Vec<String> {
    let mut out = a.to_vec();
    for n in b {
        if !out.contains(n) {
            out.push(n.clone());
        }
    }
    out
}

fn negate(note: &str) -> String {
    if let Some(rest) = note.strip_prefix("needs ") {
        format!("without {rest}")
    } else if let Some(rest) = note.strip_prefix("without ") {
        format!("needs {rest}")
    } else {
        format!("not: {note}")
    }
}

/// A condition this can't judge, in a few words.
fn describe(kind: &str, c: &Value) -> String {
    match kind {
        "minecraft:killed_by_player" => "killed by a player".to_owned(),
        "minecraft:weather_check" => "in certain weather".to_owned(),
        "minecraft:time_check" => "at certain times".to_owned(),
        "minecraft:location_check" => match &c["predicate"]["biomes"] {
            Value::Null => "in certain places".to_owned(),
            biomes => format!("in {}", names(biomes)),
        },
        "minecraft:entity_properties" => "for certain entities".to_owned(),
        "minecraft:damage_source_properties" => "from certain damage".to_owned(),
        "minecraft:block_state_property" => "from certain block states".to_owned(),
        "minecraft:enchantment_active_check" => "with an enchantment active".to_owned(),
        "minecraft:value_check" => "when a value is in range".to_owned(),
        k => k.rsplit(':').next().unwrap_or(k).replace('_', " "),
    }
}

/// An item set as words: `minecraft:brush` -> `brush`, `#minecraft:axes` ->
/// `axes`, a list joined with "or".
fn names(set: &Value) -> String {
    let short = |s: &str| {
        s.trim_start_matches('#')
            .rsplit(':')
            .next()
            .unwrap_or(s)
            .replace('_', " ")
    };
    match set {
        Value::String(s) => format!("a {}", short(s)),
        Value::Array(a) => a
            .iter()
            .filter_map(Value::as_str)
            .map(|s| format!("a {}", short(s)))
            .collect::<Vec<_>>()
            .join(" or "),
        _ => "a certain tool".to_owned(),
    }
}

/// A float number provider's value; a range's middle.
fn number(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::Object(_) => match v["type"].as_str().unwrap_or("") {
            "minecraft:constant" => number(&v["value"]),
            "minecraft:uniform" => (number(&v["min"]) + number(&v["max"])) / 2.0,
            "minecraft:binomial" => number(&v["n"]) * number(&v["p"]),
            _ => 0.0,
        },
        _ => 0.0,
    }
}

/// An int number provider's range and distribution (as `LootData::roll`
/// draws it: a uniform's ends both included).
fn int_range(v: &Value) -> (i64, i64, Vec<(i64, f64)>) {
    let dist = int_distribution(v);
    let lo = dist.iter().map(|(v, _)| *v).min().unwrap_or(0);
    let hi = dist.iter().map(|(v, _)| *v).max().unwrap_or(0);
    (lo, hi, dist)
}

fn int_distribution(v: &Value) -> Vec<(i64, f64)> {
    match v {
        Value::Number(_) => vec![(int_value(v).unwrap_or(0), 1.0)],
        Value::Object(_) => match v["type"].as_str().unwrap_or("") {
            "minecraft:constant" => int_distribution(&v["value"]),
            "minecraft:uniform" => {
                let lo = number(&v["min"]).floor() as i64;
                let hi = number(&v["max"]).floor() as i64;
                if hi <= lo {
                    return vec![(lo, 1.0)];
                }
                let n = (hi - lo + 1) as f64;
                (lo..=hi).map(|k| (k, 1.0 / n)).collect()
            }
            "minecraft:binomial" => {
                let n = number(&v["n"]).max(0.0) as i64;
                let p = number(&v["p"]).clamp(0.0, 1.0);
                (0..=n)
                    .map(|k| {
                        (
                            k,
                            binomial(n, k) * p.powi(k as i32) * (1.0 - p).powi((n - k) as i32),
                        )
                    })
                    .collect()
            }
            _ => vec![(0, 1.0)],
        },
        _ => vec![(0, 1.0)],
    }
}

fn binomial(n: i64, k: i64) -> f64 {
    (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64)
}

/// A pool's roll counts and their chances.
fn distribution(v: &Value) -> Vec<(u32, f64)> {
    int_distribution(v)
        .into_iter()
        .map(|(n, p)| (n.max(0) as u32, p))
        .collect()
}

#[cfg(test)]
#[path = "odds_tests.rs"]
mod tests;
