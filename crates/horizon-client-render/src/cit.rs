//! OptiFine's Custom Item Textures (and CIT Resewn's): the rules of the enabled packs'
//! `optifine/cit/**/*.properties`, read from the overlay resource root. The rules themselves are
//! `horizon_optifine::cit`'s.

pub use horizon_optifine::cit::{Hand, Kind, Rule};

use horizon_client_core::component::name_text;
use horizon_client_core::scoreboard::comp_text;
use horizon_items::item_data::{canon_tag, encode_components};
use horizon_items::items::ItemStack;
use horizon_optifine::cit::{ComponentTest, Source, ValueTest};
use horizon_optifine::{Properties, properties_files_in};
use horizon_world::nbt::{JavaString, Tag, parse_snbt};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::sync::Mutex;

/// Every rule under the root's `optifine/cit/`, `mcpatcher/cit/` and CIT Resewn's
/// `citresewn/cit/` folders, the one chosen first when several match first (`Rule::priority`).
/// The pack's global `cit.properties` is not a rule.
pub fn load_rules(root: &Path) -> Vec<Rule> {
    let tops = ["optifine", "mcpatcher", "citresewn"];
    let mut rules: Vec<Rule> = properties_files_in(root, &tops, "cit")
        .into_iter()
        .filter(|path| !path.ends_with("/cit.properties"))
        .filter_map(|path| {
            // Java reads `.properties` as Latin-1: a pack saved that way still loads.
            let bytes = fs::read(root.join(&path)).ok()?;
            let text = String::from_utf8(bytes)
                .unwrap_or_else(|e| e.into_bytes().iter().map(|&b| char::from(b)).collect());
            let rule = Rule::parse(&path, &Properties::parse_equals_first(&text), |p| {
                root.join(p).is_file()
            });
            if rule.is_none() {
                log::warn!("custom item textures: {path} is not a rule Horizon can read");
            }
            rule
        })
        .collect();
    rules.sort_by(Rule::priority);
    rules
}

/// The rules with an index by item and a cache of what each stack matched.
#[derive(Debug, Default)]
pub struct CitRules {
    pub rules: Vec<Rule>,
    /// Per kind (`Kind as usize`), the rules naming each item, in priority order.
    by_item: [HashMap<String, Vec<u32>>; 4],
    /// Enchantment rules without `items`, in priority order.
    any_item: Vec<u32>,
    /// Per rule, per component test: its literal read as SNBT and as a JSON text component.
    literals: Vec<Vec<Prepared>>,
    selected: Mutex<HashMap<u64, Option<u32>>>,
    enchanted: Mutex<HashMap<u64, Vec<u32>>>,
}

#[derive(Debug, Default)]
struct Prepared {
    snbt: Option<Tag>,
    json: Option<String>,
}

/// Cache entries kept before the cache starts over.
const CACHE_LIMIT: usize = 4096;

impl CitRules {
    /// The enabled packs' rules under the overlay resource root.
    pub fn load(root: &Path) -> Self {
        Self::new(load_rules(root))
    }

    /// `rules` in priority order, the winner first.
    pub fn new(rules: Vec<Rule>) -> Self {
        let mut by_item: [HashMap<String, Vec<u32>>; 4] = Default::default();
        let mut any_item = Vec::new();
        for (i, rule) in rules.iter().enumerate() {
            let i = i as u32;
            if rule.kind == Kind::Enchantment && rule.items.is_empty() {
                any_item.push(i);
            }
            for item in &rule.items {
                let list = by_item[rule.kind as usize].entry(item.clone()).or_default();
                if list.last() != Some(&i) {
                    list.push(i);
                }
            }
        }
        let literals = rules
            .iter()
            .map(|r| r.components.iter().map(prepare).collect())
            .collect();
        Self {
            rules,
            by_item,
            any_item,
            literals,
            ..Default::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The winning rule of `kind` (Item, Armor or Elytra) for this stack, as an index into `rules`.
    pub fn select(
        &self,
        kind: Kind,
        stack: &ItemStack,
        hand: Hand,
        max_damage: u32,
    ) -> Option<usize> {
        let candidates = self.by_item[kind as usize].get(&*stack.item)?;
        let comps = encode_components(stack);
        let key = cache_key((0, kind, hand), stack, max_damage, comps.as_ref());
        if let Some(hit) = lock(&self.selected).get(&key) {
            return hit.map(|i| i as usize);
        }
        let found = candidates
            .iter()
            .copied()
            .find(|&i| self.matches(i, stack, Some(hand), max_damage, comps.as_ref()));
        remember(&self.selected, key, found);
        found.map(|i| i as usize)
    }

    /// Every matching Enchantment rule, in priority order (layers are the glint PR's business).
    pub fn enchantments(&self, stack: &ItemStack, max_damage: u32) -> Vec<usize> {
        let named = self.by_item[Kind::Enchantment as usize]
            .get(&*stack.item)
            .map_or(&[][..], Vec::as_slice);
        if named.is_empty() && self.any_item.is_empty() {
            return Vec::new();
        }
        let comps = encode_components(stack);
        let key = cache_key(
            (1, Kind::Enchantment, Hand::Any),
            stack,
            max_damage,
            comps.as_ref(),
        );
        let hit = lock(&self.enchanted).get(&key).cloned();
        let found = hit.unwrap_or_else(|| {
            let mut all: Vec<u32> = named.iter().chain(&self.any_item).copied().collect();
            all.sort_unstable();
            all.retain(|&i| self.matches(i, stack, None, max_damage, comps.as_ref()));
            remember(&self.enchanted, key, all.clone());
            all
        });
        found.into_iter().map(|i| i as usize).collect()
    }

    /// Whether rule `i` matches; `hand` None skips the hand test (enchantment glints).
    fn matches(
        &self,
        i: u32,
        stack: &ItemStack,
        hand: Option<Hand>,
        max_damage: u32,
        comps: Option<&Tag>,
    ) -> bool {
        let rule = &self.rules[i as usize];
        if hand.is_some_and(|h| rule.hand != Hand::Any && rule.hand != h) {
            return false;
        }
        let count = i32::try_from(stack.count).unwrap_or(i32::MAX);
        if rule.stack_size.as_ref().is_some_and(|r| !r.contains(count)) {
            return false;
        }
        // CIT Resewn: only damageable items have damage.
        let damage = if max_damage > 0 { stack.damage } else { 0 };
        if rule
            .damage
            .as_ref()
            .is_some_and(|d| !d.matches(damage, max_damage))
        {
            return false;
        }
        let enchantments = stack.crafting_enchantments();
        let listed = |id: &str| rule.enchantments.iter().any(|e| e == id);
        if !rule.enchantments.is_empty() && !enchantments.iter().any(|(id, _)| listed(id)) {
            return false;
        }
        if let Some(levels) = &rule.enchantment_levels {
            let level_in_range = enchantments.iter().any(|(id, level)| {
                (rule.enchantments.is_empty() || listed(id))
                    && levels.contains(i32::try_from(*level).unwrap_or(i32::MAX))
            });
            if !level_in_range {
                return false;
            }
        }
        let literals = &self.literals[i as usize];
        rule.components
            .iter()
            .zip(literals)
            .all(|(test, lit)| component_test(test, lit, stack, comps) != test.negate)
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn remember<V>(cache: &Mutex<HashMap<u64, V>>, key: u64, value: V) {
    let mut cache = lock(cache);
    if cache.len() >= CACHE_LIMIT {
        cache.clear();
    }
    cache.insert(key, value);
}

/// What a cached answer depends on; `query` tells `select` from `enchantments`.
fn cache_key(query: (u8, Kind, Hand), stack: &ItemStack, max: u32, comps: Option<&Tag>) -> u64 {
    let mut h = DefaultHasher::new();
    (query.0, query.1 as u8, query.2 as u8, &*stack.item).hash(&mut h);
    (stack.count, stack.damage, max, comps).hash(&mut h);
    h.finish()
}

fn prepare(test: &ComponentTest) -> Prepared {
    let ValueTest::Literal(lit) = &test.value else {
        return Prepared::default();
    };
    let text = lit.text.trim();
    let snbt = match parse_snbt(text) {
        Ok((tag, read)) if read == text.len() && text.starts_with(['{', '[']) => Some(tag),
        _ => None,
    };
    let json = json_plain(text);
    Prepared { snbt, json }
}

/// The plain text of `s` read as a JSON text component.
fn json_plain(s: &str) -> Option<String> {
    if !s.starts_with(['{', '[', '"']) {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(s).ok()?;
    Some(comp_text(&value).plain())
}

/// Whether one test passes, before its `negate`.
fn component_test(
    test: &ComponentTest,
    lit: &Prepared,
    stack: &ItemStack,
    comps: Option<&Tag>,
) -> bool {
    let legacy;
    let (root, text_depth) = match &test.source {
        Source::Component(id) => {
            let depth = match id.as_str() {
                "minecraft:custom_name" | "minecraft:item_name" => Some(0),
                "minecraft:lore" => Some(1),
                _ => None,
            };
            (comps.and_then(|c| c.get(id)), depth)
        }
        Source::LegacyEnchantments { stored } => {
            let levels = if *stored {
                stack.stored_enchantments()
            } else {
                stack.enchantments()
            };
            legacy = Tag::List(
                levels
                    .iter()
                    .map(|(id, lvl)| {
                        let lvl = i16::try_from(*lvl).unwrap_or(i16::MAX);
                        Tag::compound([
                            ("id", Tag::String((&**id).into())),
                            ("lvl", Tag::Short(lvl)),
                        ])
                    })
                    .collect(),
            );
            (Some(&legacy), None)
        }
    };
    // Fields the component leaves out when 0, which pre-1.20.5 items always wrote.
    let defaulted;
    let root = match (&test.source, root, test.path.first()) {
        (Source::Component(id), Some(Tag::Compound(map)), Some(field))
            if matches!(
                (id.as_str(), field.as_str()),
                ("minecraft:written_book_content", "generation")
                    | ("minecraft:fireworks", "flight_duration")
            ) && !map.contains_key(&JavaString::from(field.as_str())) =>
        {
            let mut map = map.clone();
            map.insert(JavaString::from(field.as_str()), Tag::Int(0));
            defaulted = Tag::Compound(map);
            Some(&defaulted)
        }
        (_, root, _) => root,
    };
    let Some(root) = root else {
        return matches!(test.value, ValueTest::Exists(false));
    };
    if let ValueTest::Exists(want) = test.value {
        return walk(root, &test.path, 0, &mut |_, _| true) == want;
    }
    walk(root, &test.path, 0, &mut |tag, depth| {
        value_test(&test.value, lit, tag, text_depth == Some(depth))
    })
}

/// Visits what `path` reaches from `tag`, true once `visit` is.
fn walk(
    tag: &Tag,
    path: &[String],
    depth: usize,
    visit: &mut dyn FnMut(&Tag, usize) -> bool,
) -> bool {
    let Some((seg, rest)) = path.split_first() else {
        return visit(tag, depth);
    };
    let mut next = |t: &Tag| walk(t, rest, depth + 1, visit);
    let len = match tag {
        Tag::Compound(map) if seg == "*" => return map.values().any(&mut next),
        Tag::Compound(map) => return map.get(&JavaString::from(seg.as_str())).is_some_and(next),
        Tag::List(v) => v.len(),
        Tag::ByteArray(v) => v.len(),
        Tag::IntArray(v) => v.len(),
        Tag::LongArray(v) => v.len(),
        _ => return false,
    };
    let element = |i: usize| match tag {
        Tag::List(v) => Cow::Borrowed(&v[i]),
        Tag::ByteArray(v) => Cow::Owned(Tag::Byte(v[i] as i8)),
        Tag::IntArray(v) => Cow::Owned(Tag::Int(v[i])),
        Tag::LongArray(v) => Cow::Owned(Tag::Long(v[i])),
        _ => Cow::Owned(Tag::End),
    };
    if seg == "count" {
        return next(&Tag::Int(i32::try_from(len).unwrap_or(i32::MAX)));
    }
    if seg == "*" {
        return (0..len).any(|i| next(&element(i)));
    }
    seg.parse::<usize>()
        .ok()
        .filter(|&i| i < len)
        .is_some_and(|i| next(&element(i)))
}

fn int_value(tag: &Tag) -> Option<i64> {
    match *tag {
        Tag::Byte(v) => Some(v.into()),
        Tag::Short(v) => Some(v.into()),
        Tag::Int(v) => Some(v.into()),
        Tag::Long(v) => Some(v),
        _ => None,
    }
}

/// A number as Java's `String.valueOf` writes it (`3`, `3.0`).
fn decimal(tag: &Tag) -> Option<String> {
    let s = match *tag {
        Tag::Float(b) => f32::from_bits(b).to_string(),
        Tag::Double(b) => f64::from_bits(b).to_string(),
        _ => return int_value(tag).map(|v| v.to_string()),
    };
    Some(if s.contains(['.', 'e', 'N', 'i']) {
        s
    } else {
        s + ".0"
    })
}

/// `text` against a string, its JSON text, or the plain text of a text component.
fn text_matches(tag: &Tag, is_text: bool, text: impl Fn(&str) -> bool) -> bool {
    match tag {
        Tag::String(s) => {
            let s = s.to_string_lossy();
            text(&s)
                || (is_text && text(&name_text(tag).plain()))
                || json_plain(&s).is_some_and(|p| text(&p))
        }
        Tag::Compound(_) | Tag::List(_) => is_text && text(&name_text(tag).plain()),
        _ => false,
    }
}

fn value_test(test: &ValueTest, lit: &Prepared, tag: &Tag, is_text: bool) -> bool {
    match test {
        ValueTest::Exists(_) => true,
        ValueTest::Range(r) => {
            int_value(tag).is_some_and(|v| i32::try_from(v).is_ok_and(|v| r.contains(v)))
        }
        ValueTest::Raw(name) => name.matches(&canon_tag(tag)),
        ValueTest::Text(name) => match decimal(tag) {
            Some(n) => name.matches(&n),
            None => text_matches(tag, is_text, |s| name.matches(s)),
        },
        ValueTest::Literal(l) => match tag {
            Tag::Float(b) => l.text.trim().parse::<f32>().ok() == Some(f32::from_bits(*b)),
            Tag::Double(b) => l.float == Some(f64::from_bits(*b)),
            Tag::Compound(_) | Tag::List(_)
                if lit.snbt.as_ref().is_some_and(|e| contains(e, tag)) =>
            {
                true
            }
            _ => match int_value(tag) {
                Some(v) => l.int == Some(v),
                None => text_matches(tag, is_text, |s| {
                    s == l.text || lit.json.as_deref() == Some(s)
                }),
            },
        },
    }
}

/// `NbtUtils.compareNbt(expected, actual, true)`: every key and element of `expected` found in
/// `actual`.
fn contains(expected: &Tag, actual: &Tag) -> bool {
    match (expected, actual) {
        (Tag::Compound(e), Tag::Compound(a)) => e
            .iter()
            .all(|(k, v)| a.get(k).is_some_and(|a| contains(v, a))),
        (Tag::List(e), Tag::List(a)) if e.is_empty() => a.is_empty(),
        (Tag::List(e), Tag::List(a)) => e.iter().all(|e| a.iter().any(|a| contains(e, a))),
        _ => expected == actual,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Rules from (file stem, properties), sorted as `load_rules` sorts them.
    fn rules(files: &[(&str, &str)]) -> CitRules {
        let mut rules: Vec<Rule> = files
            .iter()
            .map(|(stem, text)| {
                let path = format!("assets/minecraft/optifine/cit/{stem}.properties");
                let text = format!("texture=x\n{text}");
                Rule::parse(&path, &Properties::parse_equals_first(&text), |_| true).unwrap()
            })
            .collect();
        rules.sort_by(Rule::priority);
        CitRules::new(rules)
    }

    fn stem(cit: &CitRules, i: Option<usize>) -> Option<&str> {
        let path = &cit.rules[i?].path;
        path.rsplit('/').next()?.strip_suffix(".properties")
    }

    fn item(cit: &CitRules, stack: ItemStack) -> Option<&str> {
        stem(cit, cit.select(Kind::Item, &stack, Hand::Main, 0))
    }

    fn new(id: &str) -> ItemStack {
        ItemStack::new(&format!("minecraft:{id}"), 1)
    }

    fn string(s: &str) -> Tag {
        Tag::String(s.into())
    }

    fn with(id: &str, component: &str, tag: Tag) -> ItemStack {
        let mut stack = new(id);
        let component = format!("minecraft:{component}");
        stack.data_mut().other.push((component, tag));
        stack
    }

    fn enchanted(id: &str, levels: &[(&str, u32)]) -> ItemStack {
        let mut stack = new(id);
        let levels = levels
            .iter()
            .map(|(e, l)| (Arc::from(format!("minecraft:{e}")), *l));
        stack.data_mut().enchantments = levels.collect();
        stack
    }

    #[test]
    fn heavier_rules_win_and_kinds_stay_apart() {
        let cit = rules(&[
            ("plain", "items=stick"),
            ("heavy", "items=stick\nweight=2\nstackSize=2-"),
            ("offhand", "items=stick\nweight=5\nhand=off"),
            ("armor", "type=armor\nitems=stick"),
            ("wings", "type=elytra"),
        ]);
        assert_eq!(item(&cit, new("stick")), Some("plain"));
        assert_eq!(
            item(&cit, ItemStack::new("minecraft:stick", 3)),
            Some("heavy")
        );
        let pick = |kind, id, hand, max| stem(&cit, cit.select(kind, &new(id), hand, max));
        assert_eq!(pick(Kind::Item, "stick", Hand::Off, 0), Some("offhand"));
        assert_eq!(item(&cit, new("stone")), None);
        assert_eq!(item(&cit, new("elytra")), None);
        assert_eq!(pick(Kind::Elytra, "elytra", Hand::Main, 432), Some("wings"));
        assert_eq!(pick(Kind::Armor, "stick", Hand::Main, 0), Some("armor"));
    }

    #[test]
    fn names_match_by_their_plain_text() {
        let cit = rules(&[
            ("glob", "items=stick\nnbt.display.Name=ipattern:*sword"),
            ("exact", "items=stone\ncomponents.custom_name=My Sword"),
            ("json", "items=dirt\nnbt.display.Name=[\"My \",\"Sword\"]"),
        ]);
        let named = |id: &str, name: Tag| {
            let mut stack = new(id);
            stack.data_mut().custom_name = Some(name);
            stack
        };
        let styled = Tag::compound([("text", string("My Sword")), ("color", string("red"))]);
        assert_eq!(item(&cit, named("stick", styled.clone())), Some("glob"));
        assert_eq!(item(&cit, named("stick", string("Blade"))), None);
        assert_eq!(item(&cit, new("stick")), None);
        assert_eq!(item(&cit, named("stone", styled.clone())), Some("exact"));
        assert_eq!(item(&cit, named("dirt", styled)), Some("json"));
    }

    #[test]
    fn lore_lines_and_list_counts() {
        let cit = rules(&[
            (
                "any_line",
                "items=stick\nnbt.display.Lore.*=ipattern:*legendary*",
            ),
            ("two_lines", "items=stone\ncomponents.lore.count=2"),
        ]);
        let bold = Tag::compound([("text", string("A LEGENDARY item")), ("bold", Tag::Byte(1))]);
        let lore = Tag::List(vec![string("Common"), bold]);
        assert_eq!(
            item(&cit, with("stick", "lore", lore.clone())),
            Some("any_line")
        );
        assert_eq!(item(&cit, with("stone", "lore", lore)), Some("two_lines"));
        let one = Tag::List(vec![string("A legendary item")]);
        assert_eq!(item(&cit, with("stone", "lore", one)), None);
    }

    #[test]
    fn custom_data_existence_and_negation() {
        let cit = rules(&[
            (
                "aote",
                "items=stick\nweight=2\nnbt.ExtraAttributes.id=ASPECT_OF_THE_END",
            ),
            (
                "not_hyp",
                "items=stick\nweight=1\ncomponent.custom_data.ExtraAttributes.id=!HYP",
            ),
            ("unnamed", "items=stick\nnbt.display.Name=exists:false"),
        ]);
        let data = |id: &str| {
            let attributes =
                Tag::compound([("ExtraAttributes", Tag::compound([("id", string(id))]))]);
            with("stick", "custom_data", attributes)
        };
        assert_eq!(item(&cit, data("ASPECT_OF_THE_END")), Some("aote"));
        assert_eq!(item(&cit, data("MIDAS")), Some("not_hyp"));
        assert_eq!(item(&cit, data("HYP")), Some("unnamed"));
        let mut named = data("HYP");
        named.data_mut().custom_name = Some(string("Hyperion"));
        assert_eq!(item(&cit, named), None);
    }

    #[test]
    fn legacy_enchantment_lists_and_model_data() {
        let cit = rules(&[
            (
                "sharp",
                "items=stick\nnbt.Enchantments.*.id=minecraft:sharpness\nnbt.Enchantments.0.lvl=5",
            ),
            ("cmd", "items=stone\nnbt.CustomModelData=3"),
            (
                "cmd_snbt",
                "items=dirt\ncomponents.custom_model_data={floats:[3.0f]}",
            ),
        ]);
        assert_eq!(
            item(&cit, enchanted("stick", &[("sharpness", 5)])),
            Some("sharp")
        );
        assert_eq!(item(&cit, enchanted("stick", &[("sharpness", 4)])), None);
        let model = |id: &str, f: f32| {
            let floats = Tag::List(vec![Tag::Float(f.to_bits()), Tag::Float(7f32.to_bits())]);
            with(id, "custom_model_data", Tag::compound([("floats", floats)]))
        };
        assert_eq!(item(&cit, model("stone", 3.0)), Some("cmd"));
        assert_eq!(item(&cit, model("stone", 3.5)), None);
        assert_eq!(item(&cit, model("dirt", 3.0)), Some("cmd_snbt"));
        assert_eq!(item(&cit, model("dirt", 2.0)), None);
    }

    #[test]
    fn fields_left_out_at_zero_read_as_zero() {
        let cit = rules(&[("zero", "items=firework_rocket\nnbt.Fireworks.Flight=0")]);
        let mut rocket = new("firework_rocket");
        rocket.data_mut().fireworks = Some((0, Vec::new()));
        assert_eq!(item(&cit, rocket.clone()), Some("zero"));
        rocket.data_mut().fireworks = Some((2, Vec::new()));
        assert_eq!(item(&cit, rocket), None);
    }

    #[test]
    fn damage_counts_only_for_damageable_items() {
        let cit = rules(&[
            ("worn", "items=diamond_sword\ndamage=50-100%"),
            ("new", "items=stick\ndamage=0"),
        ]);
        let damaged = |id: &str, damage: u32, max: u32| {
            let mut stack = new(id);
            stack.damage = damage;
            stem(&cit, cit.select(Kind::Item, &stack, Hand::Main, max))
        };
        // Half of 1561 is 780.5.
        assert_eq!(damaged("diamond_sword", 780, 1561), None);
        assert_eq!(damaged("diamond_sword", 781, 1561), Some("worn"));
        assert_eq!(damaged("stick", 5, 0), Some("new"));
    }

    #[test]
    fn enchantment_levels_and_rules_for_any_item() {
        let cit = rules(&[
            (
                "sharp3",
                "items=stick\nenchantments=sharpness\nenchantmentLevels=3-",
            ),
            ("any3", "items=stone\nenchantmentLevels=3-"),
            ("glint", "type=enchantment\nenchantments=sharpness"),
            (
                "sword_glint",
                "type=enchantment\nitems=diamond_sword\nweight=2",
            ),
        ]);
        let mixed = [("sharpness", 2), ("unbreaking", 3)];
        assert_eq!(item(&cit, enchanted("stick", &mixed)), None);
        assert_eq!(
            item(&cit, enchanted("stick", &[("sharpness", 4)])),
            Some("sharp3")
        );
        assert_eq!(item(&cit, enchanted("stone", &mixed)), Some("any3"));
        let glints = |stack: ItemStack| -> Vec<&str> {
            let found = cit.enchantments(&stack, 0).into_iter();
            found.map(|i| stem(&cit, Some(i)).unwrap()).collect()
        };
        let sharp = [("sharpness", 1)];
        assert_eq!(
            glints(enchanted("diamond_sword", &sharp)),
            ["sword_glint", "glint"]
        );
        assert_eq!(glints(enchanted("stick", &sharp)), ["glint"]);
        assert!(glints(new("stick")).is_empty());
        // Glints are never an item's texture.
        assert_eq!(item(&cit, enchanted("diamond_sword", &sharp)), None);
    }
}
