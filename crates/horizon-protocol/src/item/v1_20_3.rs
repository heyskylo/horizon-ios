//! Before components: FriendlyByteBuf's present flag, item id, byte count and unnamed NBT.
//! Saved NBT upgrades use Horizon's vanilla datafix implementation; values stored here are pinned.
use super::*;
use crate::Error;
use horizon_world::datafix::{self, dynamic as d, schema::Ty};
use std::collections::{HashMap, VecDeque};

const CACHE_ENTRIES: usize = 1024;
const CACHE_BYTES: usize = 8 * 1024 * 1024;
const CACHE_ENTRY_BYTES: usize = 256 * 1024;
#[derive(Debug)]
struct Original {
    tag: Option<Tag>,
    bytes: usize,
}
/// Original NBT for unchanged stacks. Two old encodings that normalize to the same value are
/// ambiguous: neither original is reused. Every session owns its cache; nothing is persisted.
#[derive(Debug, Default)]
pub(crate) struct Cache {
    entries: HashMap<Vec<u8>, Original>,
    order: VecDeque<Vec<u8>>,
    bytes: usize,
}
impl Cache {
    fn key(stack: &Stack) -> Option<Vec<u8>> {
        if stack.item < 0 {
            return None;
        }
        let _pinned = ids::unrenumbered();
        let mut value = stack.clone();
        value.count = 1;
        value.wire = WirePatch::default();
        value.added.sort_by_key(|(kind, _)| *kind);
        value.removed.sort_unstable();
        let mut w = Writer::new();
        Slot::write(&Some(value), &mut w);
        (w.data.len() <= CACHE_ENTRY_BYTES).then_some(w.data)
    }
    fn remember(&mut self, stack: &Stack, tag: Tag) {
        let Some(key) = Self::key(stack) else {
            return;
        };
        if let Some(original) = self.entries.get_mut(&key) {
            if original.tag.as_ref().is_some_and(|old| *old != tag) {
                self.bytes -= original.bytes;
                original.tag = None;
                original.bytes = key.len() * 2;
                self.bytes += original.bytes;
            }
            return;
        }
        let mut w = Writer::new();
        w.nbt(&tag);
        // Count both copies of the FIFO key, conservatively, in the serialized byte budget.
        let bytes = key.len() * 2 + w.data.len();
        if bytes > CACHE_ENTRY_BYTES {
            return;
        }
        while self.entries.len() >= CACHE_ENTRIES || self.bytes + bytes > CACHE_BYTES {
            let Some(old) = self.order.pop_front() else {
                break;
            };
            if let Some(old) = self.entries.remove(&old) {
                self.bytes -= old.bytes;
            }
        }
        self.bytes += bytes;
        self.order.push_back(key.clone());
        self.entries.insert(
            key,
            Original {
                tag: Some(tag),
                bytes,
            },
        );
    }
    fn original(&self, stack: &Stack) -> Option<Tag> {
        self.entries.get(&Self::key(stack)?)?.tag.clone()
    }
}

const DYES: &[&str] = &[
    "white",
    "orange",
    "magenta",
    "light_blue",
    "yellow",
    "lime",
    "pink",
    "gray",
    "light_gray",
    "cyan",
    "purple",
    "blue",
    "brown",
    "green",
    "red",
    "black",
];
const SLOTS: &[&str] = &[
    "any", "mainhand", "offhand", "hand", "feet", "legs", "chest", "head", "armor", "body",
];
const KINDS: &[(&str, i32)] = &[
    ("custom_data", 0),
    ("max_stack_size", 1),
    ("max_damage", 2),
    ("damage", 3),
    ("unbreakable", 4),
    ("custom_name", 6),
    ("item_name", 9),
    ("lore", 11),
    ("enchantments", 13),
    ("can_place_on", 14),
    ("can_break", 15),
    ("attribute_modifiers", 16),
    ("custom_model_data", 17),
    ("tooltip_display", 18),
    ("repair_cost", 19),
    ("enchantment_glint_override", 21),
    ("stored_enchantments", 45),
    ("dyed_color", 47),
    ("map_id", 48),
    ("map_decorations", 49),
    ("charged_projectiles", 51),
    ("bundle_contents", 52),
    ("potion_contents", 53),
    ("suspicious_stew_effects", 55),
    ("writable_book_content", 56),
    ("written_book_content", 57),
    ("trim", 58),
    ("debug_stick_state", 59),
    ("entity_data", 60),
    ("bucket_entity_data", 61),
    ("block_entity_data", 62),
    ("instrument", 63),
    ("recipes", 68),
    ("lodestone_tracker", 69),
    ("firework_explosion", 70),
    ("fireworks", 71),
    ("profile", 72),
    ("banner_patterns", 74),
    ("base_color", 75),
    ("pot_decorations", 76),
    ("container", 77),
    ("block_state", 78),
    ("bees", 79),
    ("lock", 81),
    ("container_loot", 82),
];
pub(super) fn reading() -> bool {
    matches!(ids::active(),Some((ids,ids::Way::Up)) if ids.quirks.legacy_item_nbt)
}
pub(super) fn writing() -> bool {
    matches!(ids::active(),Some((ids,ids::Way::Down)) if ids.quirks.legacy_item_nbt)
}
fn component_id(name: &str) -> Option<i32> {
    let name = name.strip_prefix("minecraft:")?;
    KINDS.iter().find(|(n, _)| *n == name).map(|(_, id)| *id)
}
fn text(value: Option<&Tag>) -> Option<String> {
    d::as_str(value)
}
fn number(value: Option<&Tag>, default: i32) -> i32 {
    d::as_int(value, default)
}
fn list(value: Option<&Tag>) -> &[Tag] {
    match value {
        Some(Tag::List(values)) => values,
        _ => &[],
    }
}
fn int_array(value: Option<&Tag>) -> Vec<i32> {
    match value {
        Some(Tag::IntArray(values)) => values.clone(),
        _ => list(value).iter().map(|v| number(Some(v), 0)).collect(),
    }
}
fn registry_id(registry: &str, name: &str) -> Result<i32> {
    ids::legacy_registry_id(registry, name)
        .ok_or_else(|| Error::new(format!("legacy item entry missing from {registry}")))
}
fn registry_name(registry: &str, id: i32) -> Result<String> {
    ids::legacy_registry_name(registry, id)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| Error::new(format!("legacy item id missing from {registry}")))
}
fn id_tag(registry: &str, value: Option<&Tag>) -> Result<i32> {
    registry_id(
        registry,
        &text(value).ok_or_else(|| Error::new("missing legacy item identifier"))?,
    )
}
fn raw_nbt(value: &Tag) -> Component {
    let mut w = Writer::new();
    w.nbt(value);
    Component::Raw(w.data)
}
pub(super) fn read_stack(r: &mut Reader) -> Result<Option<Stack>> {
    if !r.bool()? {
        return Ok(None);
    }
    let item = ids::item(r.varint()?);
    let count = i32::from(r.i8()?);
    let tag = r.nbt()?;
    if !matches!(tag, Tag::End | Tag::Compound(_)) {
        return Err(Error::new("legacy item tag is not a compound"));
    }
    if count <= 0 || item < 0 {
        return Ok(None);
    }
    let mut saved = Tag::compound([
        ("id", d::string(&registry_name("minecraft:item", item)?)),
        ("Count", Tag::Byte(count as i8)),
    ]);
    if !matches!(tag, Tag::End) {
        d::set(&mut saved, "tag", tag.clone());
    }
    let from = ids::quirks().legacy_item_data_version;
    if from <= 0 {
        return Err(Error::new("missing legacy item data version"));
    }
    datafix::update(Ty::ItemStack, &mut saved, from, datafix::CURRENT);
    let stack = from_tag(&saved, Some((item, count)))?;
    if let Some(cache) = ids::legacy_item_cache() {
        cache.lock().unwrap().remember(&stack, tag);
    }
    Ok(Some(stack))
}

fn from_tag(saved: &Tag, known: Option<(i32, i32)>) -> Result<Stack> {
    let _nested = super::Nested::enter()?;
    let _pinned = ids::unrenumbered();
    let (item, count) = match known {
        Some(values) => values,
        None => (
            id_tag("minecraft:item", saved.get("id"))?,
            number(saved.get("count"), 1),
        ),
    };
    let mut stack = Stack::new(item, count);
    let Some(Tag::Compound(components)) = saved.get("components") else {
        return Ok(stack);
    };
    for (name, value) in components {
        let name = name.to_string_lossy();
        if let Some(removed) = name.strip_prefix('!') {
            if let Some(kind) = component_id(removed) {
                stack.removed.push(kind);
            }
            continue;
        }
        let Some(kind) = component_id(&name) else {
            continue;
        };
        let value = match kind {
            0 => Component::Nbt(value.clone()),
            1..=3 | 19 | 48 => Component::Int(number(Some(value), 0)),
            4 => Component::Unit,
            6 | 9 => Component::Text(text_component(value)),
            11 => Component::Lore(list(Some(value)).iter().map(text_component).collect()),
            13 | 45 => {
                let levels = value.get("levels").unwrap_or(value);
                let mut out = Vec::new();
                if let Tag::Compound(levels) = levels {
                    for (name, level) in levels {
                        if let Some(id) = ids::legacy_registry_id(
                            "minecraft:enchantment",
                            &name.to_string_lossy(),
                        ) {
                            out.push((id, number(Some(level), 0)));
                        }
                    }
                }
                Component::Enchantments(out)
            }
            14 | 15 => predicates(value)?,
            16 => attributes(value)?,
            17 => Component::CustomModelData {
                floats: list(value.get("floats"))
                    .iter()
                    .map(|v| d::as_number(v).map_or(0.0, |v| v.float_value()))
                    .collect(),
                flags: list(value.get("flags"))
                    .iter()
                    .map(|v| d::as_bool(Some(v), false))
                    .collect(),
                strings: list(value.get("strings"))
                    .iter()
                    .filter_map(|v| text(Some(v)))
                    .collect(),
                colors: int_array(value.get("colors")),
            },
            18 => Component::TooltipDisplay {
                hide: d::as_bool(value.get("hide_tooltip"), false),
                hidden: list(value.get("hidden_components"))
                    .iter()
                    .filter_map(|v| component_id(&text(Some(v))?))
                    .collect(),
            },
            21 => Component::Bool(d::as_bool(Some(value), false)),
            47 => Component::Int(number(Some(value), 0)),
            49 => {
                let mut value = value.clone();
                if let Tag::Compound(entries) = &mut value {
                    for entry in entries.values_mut() {
                        if let Some(name) = text(entry.get("type")) {
                            d::set(entry, "type", d::string(&d::namespaced(&name)));
                        }
                    }
                }
                raw_nbt(&value)
            }
            59 | 61 | 68 | 81 | 82 => raw_nbt(value),
            51 | 52 => Component::Stacks(
                list(Some(value))
                    .iter()
                    .map(|v| from_tag(v, None))
                    .collect::<Result<_>>()?,
            ),
            53 => potion(value)?,
            55 => Component::StewEffects(
                list(Some(value))
                    .iter()
                    .map(|v| {
                        Ok((
                            id_tag("minecraft:mob_effect", v.get("id"))?,
                            number(v.get("duration"), 160),
                        ))
                    })
                    .collect::<Result<_>>()?,
            ),
            56 => {
                let pages = list(value.get("pages"));
                let filtered = pages.iter().any(|p| p.get("filtered").is_some());
                Component::WritableBook {
                    pages: pages
                        .iter()
                        .map(|p| text(p.get("raw").or(Some(p))).unwrap_or_default())
                        .collect(),
                    filtered: if filtered {
                        pages.iter().map(|p| text(p.get("filtered"))).collect()
                    } else {
                        Vec::new()
                    },
                }
            }
            57 => {
                let title = value.get("title").unwrap_or(&Tag::End);
                let pages = list(value.get("pages"));
                let filtered = pages.iter().any(|p| p.get("filtered").is_some());
                Component::WrittenBook(WrittenBook {
                    title: text(title.get("raw").or(Some(title))).unwrap_or_default(),
                    title_filtered: text(title.get("filtered")),
                    author: text(value.get("author")).unwrap_or_default(),
                    generation: number(value.get("generation"), 0),
                    resolved: d::as_bool(value.get("resolved"), false),
                    pages: pages
                        .iter()
                        .map(|p| text_component(p.get("raw").unwrap_or(p)))
                        .collect(),
                    pages_filtered: if filtered {
                        pages
                            .iter()
                            .map(|p| p.get("filtered").map(text_component))
                            .collect()
                    } else {
                        Vec::new()
                    },
                })
            }
            58 => Component::Trim {
                material: id_tag("minecraft:trim_material", value.get("material"))?,
                pattern: id_tag("minecraft:trim_pattern", value.get("pattern"))?,
            },
            60 => typed_data(value, "minecraft:entity_type")?,
            62 => typed_data(value, "minecraft:block_entity_type")?,
            63 => Component::Instrument(id_tag("minecraft:instrument", Some(value))?),
            69 => {
                let target = if let Some(target) = value.get("target") {
                    let coords = int_array(target.get("pos"));
                    if coords.len() == 3 {
                        Some((
                            text(target.get("dimension"))
                                .unwrap_or_else(|| "minecraft:overworld".into()),
                            [coords[0], coords[1], coords[2]],
                        ))
                    } else {
                        None
                    }
                } else {
                    None
                };
                Component::LodestoneTracker {
                    target,
                    tracked: d::as_bool(value.get("tracked"), true),
                }
            }
            70 => Component::FireworkExplosion(explosion(value)),
            71 => Component::Fireworks {
                flight: number(value.get("flight_duration"), 0),
                explosions: list(value.get("explosions"))
                    .iter()
                    .map(explosion)
                    .collect(),
            },
            72 => Component::Profile(profile(value)),
            74 => Component::BannerPatterns(
                list(Some(value))
                    .iter()
                    .map(|v| {
                        Ok((
                            id_tag("minecraft:banner_pattern", v.get("pattern"))?,
                            dye(v.get("color")),
                        ))
                    })
                    .collect::<Result<_>>()?,
            ),
            75 => Component::Int(dye(Some(value))),
            76 => decorations(value)?,
            77 => container(value)?,
            78 => {
                let mut w = Writer::new();
                let Tag::Compound(map) = value else {
                    return Err(Error::new("block state is not a map"));
                };
                w.varint(map.len() as i32);
                for (k, v) in map {
                    w.string(&k.to_string_lossy());
                    w.string(&text(Some(v)).unwrap_or_default());
                }
                Component::Raw(w.data)
            }
            79 => bees(value)?,
            _ => continue,
        };
        stack.added.push((kind, value));
    }
    Ok(stack)
}

fn dye(value: Option<&Tag>) -> i32 {
    text(value)
        .and_then(|s| DYES.iter().position(|v| *v == s))
        .map_or(0, |i| i as i32)
}
fn text_component(value: &Tag) -> Tag {
    match value {
        Tag::Compound(map) if map.len() == 1 => {
            value.get("text").cloned().unwrap_or_else(|| value.clone())
        }
        Tag::List(values) if values.len() == 1 => text_component(&values[0]),
        _ => value.clone(),
    }
}
fn explosion(value: &Tag) -> FireworkExplosion {
    FireworkExplosion {
        shape: text(value.get("shape"))
            .and_then(|s| {
                ["small_ball", "large_ball", "star", "creeper", "burst"]
                    .iter()
                    .position(|v| *v == s)
            })
            .unwrap_or(0) as i32,
        colors: int_array(value.get("colors")),
        fade: int_array(value.get("fade_colors")),
        trail: d::as_bool(value.get("has_trail"), false),
        twinkle: d::as_bool(value.get("has_twinkle"), false),
    }
}
fn uuid(value: Option<&Tag>) -> Option<u128> {
    let parts = int_array(value);
    (parts.len() == 4).then(|| {
        parts
            .iter()
            .fold(0u128, |n, part| (n << 32) | u128::from(*part as u32))
    })
}
fn profile(value: &Tag) -> Profile {
    Profile {
        name: text(value.get("name")),
        id: uuid(value.get("id")),
        skin: SkinPatch::default(),
        properties: list(value.get("properties"))
            .iter()
            .map(|v| {
                (
                    text(v.get("name")).unwrap_or_default(),
                    text(v.get("value")).unwrap_or_default(),
                    text(v.get("signature")),
                )
            })
            .collect(),
    }
}
fn typed_data(value: &Tag, registry: &str) -> Result<Component> {
    let mut data = value.clone();
    let id = id_tag(registry, data.get("id"))?;
    d::remove(&mut data, "id");
    let mut w = Writer::new();
    w.varint(id);
    w.nbt(&data);
    Ok(Component::Raw(w.data))
}
fn container(value: &Tag) -> Result<Component> {
    let mut contents = Vec::new();
    for entry in list(Some(value)) {
        let slot = number(entry.get("slot"), -1);
        if (0..256).contains(&slot)
            && let Some(item) = entry.get("item")
        {
            contents.push((slot as usize, from_tag(item, None)?));
        }
    }
    let mut w = Writer::new();
    let n = contents.iter().map(|(slot, _)| slot + 1).max().unwrap_or(0);
    w.varint(n as i32);
    for slot in 0..n {
        let item = contents.iter().find(|(i, _)| *i == slot);
        w.bool(item.is_some());
        if let Some((_, item)) = item {
            Template::write(item, &mut w);
        }
    }
    Ok(Component::Raw(w.data))
}
fn decorations(value: &Tag) -> Result<Component> {
    let mut w = Writer::new();
    for side in ["back", "left", "right", "front"] {
        let entry = value.get(side);
        w.bool(entry.is_some());
        if let Some(entry) = entry {
            let item = if let Tag::String(_) = entry {
                Stack::new(id_tag("minecraft:item", Some(entry))?, 1)
            } else {
                from_tag(entry, None)?
            };
            Template::write(&item, &mut w);
        }
    }
    Ok(Component::Raw(w.data))
}

fn attributes(value: &Tag) -> Result<Component> {
    let entries = list(value.get("modifiers").or(Some(value)));
    let mut w = Writer::new();
    w.varint(entries.len() as i32);
    for entry in entries {
        w.varint(id_tag("minecraft:attribute", entry.get("type"))?);
        w.string(&text(entry.get("id")).ok_or_else(|| Error::new("attribute modifier has no id"))?);
        w.f64(d::as_double(entry.get("amount"), 0.0));
        w.varint(match text(entry.get("operation")).as_deref() {
            Some("add_multiplied_base") => 1,
            Some("add_multiplied_total") => 2,
            _ => 0,
        });
        w.varint(
            text(entry.get("slot"))
                .and_then(|s| SLOTS.iter().position(|v| *v == s))
                .unwrap_or(0) as i32,
        );
        w.varint(0);
    }
    Ok(Component::Raw(w.data))
}
fn predicates(value: &Tag) -> Result<Component> {
    let predicates = list(value.get("predicates").or(Some(value)));
    let mut w = Writer::new();
    w.varint(predicates.len() as i32);
    for predicate in predicates {
        let blocks = predicate.get("blocks");
        w.bool(blocks.is_some());
        if let Some(blocks) = blocks {
            if let Some(tag) =
                text(Some(blocks)).and_then(|s| s.strip_prefix('#').map(str::to_owned))
            {
                w.varint(0);
                w.string(&tag);
            } else {
                let names = if matches!(blocks, Tag::String(_)) {
                    vec![blocks]
                } else {
                    list(Some(blocks)).iter().collect()
                };
                w.varint(names.len() as i32 + 1);
                for name in names {
                    w.varint(id_tag("minecraft:block", Some(name))?);
                }
            }
        }
        let properties = predicate.get("state");
        w.bool(properties.is_some());
        if let Some(Tag::Compound(properties)) = properties {
            w.varint(properties.len() as i32);
            for (name, matcher) in properties {
                w.string(&name.to_string_lossy());
                let exact = text(Some(matcher));
                w.bool(exact.is_some());
                if let Some(exact) = exact {
                    w.string(&exact);
                } else {
                    for field in ["min", "max"] {
                        let bound = text(matcher.get(field));
                        w.bool(bound.is_some());
                        if let Some(bound) = bound {
                            w.string(&bound);
                        }
                    }
                }
            }
        } else if properties.is_some() {
            return Err(Error::new("invalid predicate properties"));
        }
        let nbt = predicate.get("nbt");
        w.bool(nbt.is_some());
        if let Some(nbt) = nbt {
            let parsed;
            let nbt = if let Some(snbt) = text(Some(nbt)) {
                let (value, used) = horizon_world::nbt::parse_snbt(&snbt).map_err(Error::new)?;
                if used != snbt.len() {
                    return Err(Error::new("trailing predicate SNBT"));
                }
                parsed = value;
                &parsed
            } else {
                nbt
            };
            w.nbt(nbt);
        }
        w.varint(0);
        w.varint(0); // No component matchers existed in legacy predicates.
    }
    Ok(Component::Raw(w.data))
}
fn effect(value: &Tag, w: &mut Writer) -> Result<()> {
    w.varint(id_tag("minecraft:mob_effect", value.get("id"))?);
    let mut current = value;
    loop {
        w.varint(number(current.get("amplifier"), 0).clamp(0, 255));
        w.varint(number(current.get("duration"), 0));
        w.bool(d::as_bool(current.get("ambient"), false));
        let particles = d::as_bool(current.get("show_particles"), true);
        w.bool(particles);
        w.bool(d::as_bool(current.get("show_icon"), particles));
        let hidden = current.get("hidden_effect");
        w.bool(hidden.is_some());
        let Some(hidden) = hidden else {
            break;
        };
        current = hidden;
    }
    Ok(())
}
fn potion(value: &Tag) -> Result<Component> {
    let potion = value
        .get("potion")
        .map(|v| id_tag("minecraft:potion", Some(v)))
        .transpose()?;
    let color = value.get("custom_color").map(|v| number(Some(v), 0));
    let effects = list(value.get("custom_effects"));
    let name = text(value.get("custom_name"));
    let mut w = Writer::new();
    w.varint(effects.len() as i32);
    for entry in effects {
        effect(entry, &mut w)?;
    }
    w.bool(name.is_some());
    if let Some(name) = &name {
        w.string(name);
    }
    Ok(Component::Potion {
        potion,
        color,
        rest: if effects.is_empty() && name.is_none() {
            Vec::new()
        } else {
            w.data
        },
    })
}

pub(super) fn write_stack(stack: Option<&Stack>, w: &mut Writer) {
    let Some(stack) = stack else {
        w.bool(false);
        return;
    };
    let item = ids::item(stack.item);
    if item < 0 || !(1..=127).contains(&stack.count) {
        w.bool(false);
        return;
    }
    let cached = ids::legacy_item_cache().and_then(|cache| cache.lock().unwrap().original(stack));
    let from_cache = cached.is_some();
    let tag = if let Some(tag) = cached {
        tag
    } else {
        let Ok(tag) = legacy_tag(stack) else {
            w.bool(false);
            return;
        };
        tag
    };
    w.bool(true);
    w.varint(item);
    w.i8(stack.count as i8);
    if !from_cache && matches!(&tag, Tag::Compound(map) if map.is_empty()) {
        w.nbt(&Tag::End);
    } else {
        w.nbt(&tag);
    }
}
fn set_sub(tag: &mut Tag, outer: &str, key: &str, value: Tag) {
    if !matches!(tag.get(outer), Some(Tag::Compound(_))) {
        d::set(tag, outer, d::empty_map());
    }
    d::set(tag.get_mut(outer).unwrap(), key, value);
}
fn json_text(value: &Tag) -> Tag {
    fn fix(value: &mut datafix::json::Json) {
        use datafix::json::Json;
        match value {
            Json::Obj(entries) => {
                for (name, value) in entries {
                    let name = String::from_utf16_lossy(name);
                    if matches!(
                        name.as_str(),
                        "bold" | "italic" | "underlined" | "strikethrough" | "obfuscated"
                    ) {
                        if let Json::Num(n) = value {
                            *value = Json::Bool(n != "0");
                        }
                    } else {
                        fix(value);
                    }
                }
            }
            Json::Arr(entries) => entries.iter_mut().for_each(fix),
            _ => {}
        }
    }
    let mut json = datafix::json::from_nbt(value);
    fix(&mut json);
    Tag::String(horizon_world::nbt::JavaString(
        datafix::json::to_stable_string(&json),
    ))
}
fn uuid_tag(uuid: u128) -> Tag {
    Tag::IntArray((0..4).map(|i| (uuid >> (96 - i * 32)) as i32).collect())
}
fn legacy_saved(stack: &Stack) -> Result<Tag> {
    let tag = legacy_tag(stack)?;
    let mut result = Tag::compound([
        (
            "id",
            d::string(&registry_name("minecraft:item", stack.item)?),
        ),
        ("Count", Tag::Byte(stack.count as i8)),
    ]);
    if !matches!(&tag, Tag::Compound(m) if m.is_empty()) {
        d::set(&mut result, "tag", tag);
    }
    Ok(result)
}
fn legacy_tag(stack: &Stack) -> Result<Tag> {
    let _pinned = ids::unrenumbered();
    let mut tag = match stack.component(component::CUSTOM_DATA) {
        Some(Component::Nbt(tag @ Tag::Compound(_))) => tag.clone(),
        _ => d::empty_map(),
    };
    let mut hide = 0;
    for (kind, value) in &stack.added {
        match (*kind, value) {
            (3, Component::Int(value)) => d::set(&mut tag, "Damage", Tag::Int(*value)),
            (19, Component::Int(value)) => d::set(&mut tag, "RepairCost", Tag::Int(*value)),
            (4, Component::Unit) => d::set(&mut tag, "Unbreakable", Tag::Byte(1)),
            (6 | 9, Component::Text(value)) => {
                set_sub(&mut tag, "display", "Name", json_text(value))
            }
            (11, Component::Lore(values)) => set_sub(
                &mut tag,
                "display",
                "Lore",
                Tag::List(values.iter().map(json_text).collect()),
            ),
            (13 | 45, Component::Enchantments(values)) => {
                let values = values
                    .iter()
                    .map(|(id, level)| {
                        Ok(Tag::compound([
                            (
                                "id",
                                d::string(&registry_name("minecraft:enchantment", *id)?),
                            ),
                            ("lvl", Tag::Short(*level as i16)),
                        ]))
                    })
                    .collect::<Result<Vec<_>>>()?;
                d::set(
                    &mut tag,
                    if *kind == 13 {
                        "Enchantments"
                    } else {
                        "StoredEnchantments"
                    },
                    Tag::List(values),
                );
            }
            (17, Component::CustomModelData { floats, .. }) => {
                if let Some(value) = floats.first() {
                    d::set(&mut tag, "CustomModelData", Tag::Int(*value as i32));
                }
            }
            (18, Component::TooltipDisplay { hide: all, hidden }) => {
                if *all {
                    hide |= 255;
                }
                for kind in hidden {
                    hide |= match kind {
                        13 => 1,
                        16 => 2,
                        4 => 4,
                        15 => 8,
                        14 => 16,
                        45 => 32,
                        47 => 64,
                        58 => 128,
                        k if v1_21_4::ADDITIONAL.contains(k) => 32,
                        _ => 0,
                    };
                }
            }
            (21, Component::Bool(true)) => {
                if stack.component(component::ENCHANTMENTS).is_none() {
                    d::set(&mut tag, "Enchantments", Tag::List(Vec::new()));
                }
            }
            (47, Component::Int(value)) => set_sub(&mut tag, "display", "color", Tag::Int(*value)),
            (48, Component::Int(value)) => d::set(&mut tag, "map", Tag::Int(*value)),
            (51 | 52, Component::Stacks(values)) => {
                d::set(
                    &mut tag,
                    if *kind == 51 {
                        "ChargedProjectiles"
                    } else {
                        "Items"
                    },
                    Tag::List(values.iter().map(legacy_saved).collect::<Result<_>>()?),
                );
                if *kind == 51 {
                    d::set(&mut tag, "Charged", Tag::Byte(i8::from(!values.is_empty())));
                }
            }
            (
                53,
                Component::Potion {
                    potion,
                    color,
                    rest,
                },
            ) => {
                if let Some(id) = potion {
                    d::set(
                        &mut tag,
                        "Potion",
                        d::string(&registry_name("minecraft:potion", *id)?),
                    );
                }
                if let Some(color) = color {
                    d::set(&mut tag, "CustomPotionColor", Tag::Int(*color));
                }
                if !rest.is_empty() {
                    let mut r = Reader::new(rest);
                    let n = r.count(65536)?;
                    let effects = (0..n)
                        .map(|_| legacy_effect(&mut r))
                        .collect::<Result<Vec<_>>>()?;
                    if !effects.is_empty() {
                        d::set(&mut tag, "custom_potion_effects", Tag::List(effects));
                    }
                    if r.bool()? {
                        r.string(32767)?;
                    } // New custom potion names have no old field.
                }
            }
            (55, Component::StewEffects(values)) => d::set(
                &mut tag,
                "effects",
                Tag::List(
                    values
                        .iter()
                        .map(|(id, duration)| {
                            Ok(Tag::compound([
                                (
                                    "id",
                                    d::string(&registry_name("minecraft:mob_effect", *id)?),
                                ),
                                ("duration", Tag::Int(*duration)),
                            ]))
                        })
                        .collect::<Result<_>>()?,
                ),
            ),
            (56, Component::WritableBook { pages, filtered }) => {
                d::set(
                    &mut tag,
                    "pages",
                    Tag::List(pages.iter().map(|s| d::string(s)).collect()),
                );
                let mut filtered_pages = d::empty_map();
                for (i, value) in filtered.iter().enumerate() {
                    if let Some(value) = value {
                        d::set(&mut filtered_pages, &i.to_string(), d::string(value));
                    }
                }
                if !matches!(&filtered_pages,Tag::Compound(m) if m.is_empty()) {
                    d::set(&mut tag, "filtered_pages", filtered_pages);
                }
            }
            (57, Component::WrittenBook(book)) => {
                d::set(&mut tag, "title", d::string(&book.title));
                d::set(&mut tag, "author", d::string(&book.author));
                d::set(&mut tag, "generation", Tag::Int(book.generation));
                d::set(&mut tag, "resolved", Tag::Byte(i8::from(book.resolved)));
                if let Some(filtered) = &book.title_filtered {
                    d::set(&mut tag, "filtered_title", d::string(filtered));
                }
                d::set(
                    &mut tag,
                    "pages",
                    Tag::List(book.pages.iter().map(json_text).collect()),
                );
                let mut filtered_pages = d::empty_map();
                for (i, value) in book.pages_filtered.iter().enumerate() {
                    if let Some(value) = value {
                        d::set(&mut filtered_pages, &i.to_string(), json_text(value));
                    }
                }
                if !matches!(&filtered_pages,Tag::Compound(m) if m.is_empty()) {
                    d::set(&mut tag, "filtered_pages", filtered_pages);
                }
            }
            (58, Component::Trim { material, pattern }) => d::set(
                &mut tag,
                "Trim",
                Tag::compound([
                    (
                        "material",
                        d::string(&registry_name("minecraft:trim_material", *material)?),
                    ),
                    (
                        "pattern",
                        d::string(&registry_name("minecraft:trim_pattern", *pattern)?),
                    ),
                ]),
            ),
            (63, Component::Instrument(id)) => d::set(
                &mut tag,
                "instrument",
                d::string(&registry_name("minecraft:instrument", *id)?),
            ),
            (69, Component::LodestoneTracker { target, tracked }) => {
                if let Some((dimension, [x, y, z])) = target {
                    d::set(&mut tag, "LodestoneDimension", d::string(dimension));
                    d::set(
                        &mut tag,
                        "LodestonePos",
                        Tag::compound([
                            ("X", Tag::Int(*x)),
                            ("Y", Tag::Int(*y)),
                            ("Z", Tag::Int(*z)),
                        ]),
                    );
                }
                d::set(&mut tag, "LodestoneTracked", Tag::Byte(i8::from(*tracked)));
            }
            (70, Component::FireworkExplosion(value)) => {
                d::set(&mut tag, "Explosion", legacy_explosion(value))
            }
            (71, Component::Fireworks { flight, .. }) => {
                set_sub(&mut tag, "Fireworks", "Flight", Tag::Byte(*flight as i8))
            }
            (72, Component::Profile(profile)) => {
                d::set(&mut tag, "SkullOwner", legacy_profile(profile))
            }
            (74, Component::BannerPatterns(layers)) => {
                let layers = layers
                    .iter()
                    .map(|(id, color)| {
                        Ok(Tag::compound([
                            (
                                "Pattern",
                                d::string(banner_code(&registry_name(
                                    "minecraft:banner_pattern",
                                    *id,
                                )?)),
                            ),
                            ("Color", Tag::Int(*color)),
                        ]))
                    })
                    .collect::<Result<Vec<_>>>()?;
                set_sub(&mut tag, "BlockEntityTag", "Patterns", Tag::List(layers));
            }
            (75, Component::Int(color)) => {
                set_sub(&mut tag, "BlockEntityTag", "Base", Tag::Int(*color))
            }
            (kind, Component::Raw(bytes)) => legacy_raw(&mut tag, kind, bytes)?,
            _ => {}
        }
        if let (71, Component::Fireworks { explosions, .. }) = (*kind, value)
            && !explosions.is_empty()
        {
            set_sub(
                &mut tag,
                "Fireworks",
                "Explosions",
                Tag::List(explosions.iter().map(legacy_explosion).collect()),
            );
        }
    }
    if hide != 0 {
        d::set(&mut tag, "HideFlags", Tag::Int(hide));
    }
    Ok(tag)
}
fn legacy_explosion(value: &FireworkExplosion) -> Tag {
    let mut out = Tag::compound([
        ("Type", Tag::Byte(value.shape as i8)),
        ("Colors", Tag::IntArray(value.colors.clone())),
    ]);
    if !value.fade.is_empty() {
        d::set(&mut out, "FadeColors", Tag::IntArray(value.fade.clone()));
    }
    if value.trail {
        d::set(&mut out, "Trail", Tag::Byte(1));
    }
    if value.twinkle {
        d::set(&mut out, "Flicker", Tag::Byte(1));
    }
    out
}
fn legacy_profile(profile: &Profile) -> Tag {
    let mut tag = d::empty_map();
    if let Some(name) = &profile.name {
        d::set(&mut tag, "Name", d::string(name));
    }
    if let Some(id) = profile.id {
        d::set(&mut tag, "Id", uuid_tag(id));
    }
    let mut properties = d::empty_map();
    for (name, value, signature) in &profile.properties {
        let mut entry = Tag::compound([("Value", d::string(value))]);
        if let Some(signature) = signature {
            d::set(&mut entry, "Signature", d::string(signature));
        }
        if properties.get(name).is_none() {
            d::set(&mut properties, name, Tag::List(Vec::new()));
        }
        if let Some(Tag::List(values)) = properties.get_mut(name) {
            values.push(entry);
        }
    }
    if !profile.properties.is_empty() {
        d::set(&mut tag, "Properties", properties);
    }
    tag
}
fn legacy_effect(r: &mut Reader) -> Result<Tag> {
    let id = r.varint()?;
    let mut details = Vec::new();
    loop {
        let amplifier = r.varint()?;
        let duration = r.varint()?;
        let ambient = r.bool()?;
        let particles = r.bool()?;
        let icon = r.bool()?;
        details.push(Tag::compound([
            ("amplifier", Tag::Byte(amplifier as i8)),
            ("duration", Tag::Int(duration)),
            ("ambient", Tag::Byte(i8::from(ambient))),
            ("show_particles", Tag::Byte(i8::from(particles))),
            ("show_icon", Tag::Byte(i8::from(icon))),
        ]));
        if !r.bool()? {
            break;
        }
    }
    let mut result = details.pop().unwrap();
    while let Some(mut outer) = details.pop() {
        d::set(&mut outer, "hidden_effect", result);
        result = outer;
    }
    d::set(
        &mut result,
        "id",
        d::string(&registry_name("minecraft:mob_effect", id)?),
    );
    Ok(result)
}

fn bees(value: &Tag) -> Result<Component> {
    let entries = list(Some(value));
    let mut w = Writer::new();
    w.varint(entries.len() as i32);
    for entry in entries {
        let data = entry
            .get("entity_data")
            .ok_or_else(|| Error::new("bee has no entity data"))?;
        let Component::Raw(data) = typed_data(data, "minecraft:entity_type")? else {
            unreachable!();
        };
        w.bytes(&data);
        w.varint(number(entry.get("ticks_in_hive"), 0));
        w.varint(number(entry.get("min_ticks_in_hive"), 0));
    }
    Ok(Component::Raw(w.data))
}
fn legacy_raw(tag: &mut Tag, kind: i32, bytes: &[u8]) -> Result<()> {
    let mut r = Reader::new(bytes);
    match kind {
        49 => {
            const ICONS: &[&str] = &[
                "player",
                "frame",
                "red_marker",
                "blue_marker",
                "target_x",
                "target_point",
                "player_off_map",
                "player_off_limits",
                "mansion",
                "monument",
                "banner_white",
                "banner_orange",
                "banner_magenta",
                "banner_light_blue",
                "banner_yellow",
                "banner_lime",
                "banner_pink",
                "banner_gray",
                "banner_light_gray",
                "banner_cyan",
                "banner_purple",
                "banner_blue",
                "banner_brown",
                "banner_green",
                "banner_red",
                "banner_black",
                "red_x",
                "village_desert",
                "village_plains",
                "village_savanna",
                "village_snowy",
                "village_taiga",
                "jungle_temple",
                "swamp_hut",
            ];
            let mut entries = Vec::new();
            if let Tag::Compound(values) = r.nbt()? {
                for (id, value) in values {
                    let ty = text(value.get("type")).and_then(|n| {
                        ICONS
                            .iter()
                            .position(|v| *v == n.strip_prefix("minecraft:").unwrap_or(&n))
                    });
                    if let Some(ty) = ty {
                        entries.push(Tag::compound([
                            ("id", Tag::String(id)),
                            ("type", Tag::Byte(ty as i8)),
                            (
                                "x",
                                Tag::Double(d::as_double(value.get("x"), 0.0).to_bits()),
                            ),
                            (
                                "z",
                                Tag::Double(d::as_double(value.get("z"), 0.0).to_bits()),
                            ),
                            (
                                "rot",
                                Tag::Double(d::as_double(value.get("rotation"), 0.0).to_bits()),
                            ),
                        ]));
                    }
                }
            }
            d::set(tag, "Decorations", Tag::List(entries));
        }
        14 | 15 => d::set(
            tag,
            if kind == 14 {
                "CanPlaceOn"
            } else {
                "CanDestroy"
            },
            Tag::List(
                legacy_predicates(&mut r)?
                    .iter()
                    .map(|s| d::string(s))
                    .collect(),
            ),
        ),
        16 => d::set(
            tag,
            "AttributeModifiers",
            Tag::List(legacy_attributes(&mut r)?),
        ),
        59 => d::set(tag, "DebugProperty", r.nbt()?),
        60 | 62 => {
            let id = r.varint()?;
            let mut data = r.nbt()?;
            if let Some(name) = data.get("CustomName").cloned() {
                d::set(&mut data, "CustomName", json_text(&name));
            }
            d::set(
                &mut data,
                "id",
                d::string(&registry_name(
                    if kind == 60 {
                        "minecraft:entity_type"
                    } else {
                        "minecraft:block_entity_type"
                    },
                    id,
                )?),
            );
            d::set(
                tag,
                if kind == 60 {
                    "EntityTag"
                } else {
                    "BlockEntityTag"
                },
                data,
            );
        }
        61 => {
            if let Tag::Compound(data) = r.nbt()? {
                for (key, value) in data {
                    d::set(tag, &key.to_string_lossy(), value);
                }
            }
        }
        68 => d::set(tag, "Recipes", r.nbt()?),
        76 => {
            let mut sherds = Vec::new();
            for _ in 0..4 {
                sherds.push(if r.bool()? {
                    d::string(&registry_name(
                        "minecraft:item",
                        Template::read(&mut r)?.item,
                    )?)
                } else {
                    d::string("minecraft:brick")
                });
            }
            set_sub(tag, "BlockEntityTag", "sherds", Tag::List(sherds));
        }
        77 => {
            let mut items = Vec::new();
            for slot in 0..r.count(256)? {
                if r.bool()? {
                    let mut item = legacy_saved(&Template::read(&mut r)?)?;
                    d::set(&mut item, "Slot", Tag::Byte(slot as i8));
                    items.push(item);
                }
            }
            set_sub(tag, "BlockEntityTag", "Items", Tag::List(items));
        }
        78 => {
            let mut state = d::empty_map();
            for _ in 0..r.count(65536)? {
                let key = r.string(32767)?;
                d::set(&mut state, &key, d::string(&r.string(32767)?));
            }
            d::set(tag, "BlockStateTag", state);
        }
        79 => {
            let mut bees = Vec::new();
            for _ in 0..r.count(65536)? {
                let id = r.varint()?;
                let mut data = r.nbt()?;
                if let Some(name) = data.get("CustomName").cloned() {
                    d::set(&mut data, "CustomName", json_text(&name));
                }
                d::set(
                    &mut data,
                    "id",
                    d::string(&registry_name("minecraft:entity_type", id)?),
                );
                bees.push(Tag::compound([
                    ("EntityData", data),
                    ("TicksInHive", Tag::Int(r.varint()?)),
                    ("MinOccupationTicks", Tag::Int(r.varint()?)),
                ]));
            }
            set_sub(tag, "BlockEntityTag", "Bees", Tag::List(bees));
        }
        81 => {
            let value = r.nbt()?;
            if let Some(Tag::String(value)) = value
                .get("components")
                .and_then(|c| c.get("minecraft:custom_name"))
                && let Some(datafix::json::Json::Str(name)) = datafix::json::parse_strict(&value.0)
            {
                set_sub(
                    tag,
                    "BlockEntityTag",
                    "Lock",
                    Tag::String(horizon_world::nbt::JavaString(name)),
                );
            }
        }
        82 => {
            let value = r.nbt()?;
            if let Some(table) = value.get("loot_table") {
                set_sub(tag, "BlockEntityTag", "LootTable", table.clone());
            }
            if let Some(seed) = value.get("seed") {
                set_sub(tag, "BlockEntityTag", "LootTableSeed", seed.clone());
            }
        }
        // New components without an old NBT equivalent cannot be represented by this wire version.
        _ => {}
    }
    Ok(())
}
fn legacy_attributes(r: &mut Reader) -> Result<Vec<Tag>> {
    let mut attributes = Vec::new();
    for _ in 0..r.count(65536)? {
        let attribute = registry_name("minecraft:attribute", r.varint()?)?;
        let id = r.string(32767)?;
        let amount = r.f64()?;
        let operation = r.varint()?;
        let slot = r.varint()?;
        match r.varint()? {
            0 | 1 => {}
            2 => {
                r.nbt()?;
            }
            _ => return Err(Error::new("unknown modifier display")),
        }
        let mut value = Tag::compound([
            ("AttributeName", d::string(&attribute)),
            ("Name", d::string(v1_20_5::modifier_name(&id))),
            ("UUID", uuid_tag(v1_20_5::modifier_uuid(&id))),
            ("Amount", Tag::Double(amount.to_bits())),
            ("Operation", Tag::Int(operation)),
        ]);
        if let Some(name) = usize::try_from(slot).ok().and_then(|i| SLOTS.get(i))
            && slot != 0
        {
            d::set(&mut value, "Slot", d::string(name));
        }
        attributes.push(value);
    }
    Ok(attributes)
}
fn legacy_predicates(r: &mut Reader) -> Result<Vec<String>> {
    let mut result = Vec::new();
    for _ in 0..r.count(65536)? {
        let mut blocks = Vec::new();
        if r.bool()? {
            let n = r.varint()?;
            if n == 0 {
                blocks.push(format!("#{}", r.string(32767)?));
            } else {
                let count =
                    usize::try_from(n - 1).map_err(|_| Error::new("negative holder set"))?;
                if count > r.remaining() {
                    return Err(Error::new("holder set past end"));
                }
                for _ in 0..count {
                    blocks.push(registry_name("minecraft:block", r.varint()?)?);
                }
            }
        }
        let mut states = Vec::new();
        let mut representable = true;
        if r.bool()? {
            for _ in 0..r.count(65536)? {
                let key = r.string(32767)?;
                if r.bool()? {
                    states.push(format!("{key}={}", r.string(32767)?));
                } else {
                    for _ in 0..2 {
                        if r.bool()? {
                            r.string(32767)?;
                        }
                    }
                    representable = false;
                }
            }
        }
        let nbt = if r.bool()? {
            horizon_world::snbt_text::to_snbt(&r.nbt()?)
        } else {
            String::new()
        };
        // Consume modern matchers even though this older syntax cannot express them.
        let start = r.position();
        skip::component_matchers(r)?;
        if r.since(start) != [0, 0] {
            representable = false;
        }
        if representable {
            let state = if states.is_empty() {
                String::new()
            } else {
                format!("[{}]", states.join(","))
            };
            for block in blocks {
                result.push(format!("{block}{state}{nbt}"));
            }
        }
    }
    Ok(result)
}
fn banner_code(name: &str) -> &str {
    match name.strip_prefix("minecraft:").unwrap_or(name) {
        "base" => "b",
        "square_bottom_left" => "bl",
        "square_bottom_right" => "br",
        "square_top_left" => "tl",
        "square_top_right" => "tr",
        "stripe_bottom" => "bs",
        "stripe_top" => "ts",
        "stripe_left" => "ls",
        "stripe_right" => "rs",
        "stripe_center" => "cs",
        "stripe_middle" => "ms",
        "stripe_downright" => "drs",
        "stripe_downleft" => "dls",
        "small_stripes" => "ss",
        "cross" => "cr",
        "straight_cross" => "sc",
        "triangle_bottom" => "bt",
        "triangle_top" => "tt",
        "triangles_bottom" => "bts",
        "triangles_top" => "tts",
        "diagonal_left" => "ld",
        "diagonal_up_right" => "rd",
        "diagonal_up_left" => "lud",
        "diagonal_right" => "rud",
        "circle" => "mc",
        "rhombus" => "mr",
        "half_vertical" => "vh",
        "half_horizontal" => "hh",
        "half_vertical_right" => "vhr",
        "half_horizontal_bottom" => "hhb",
        "border" => "bo",
        "curly_border" => "cbo",
        "gradient" => "gra",
        "gradient_up" => "gru",
        "bricks" => "bri",
        "globe" => "glb",
        "creeper" => "cre",
        "skull" => "sku",
        "flower" => "flo",
        "mojang" => "moj",
        "piglin" => "pig",
        _ => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_nbt_cache_preserves_json_and_opaque_entity_fields_and_rejects_collisions() {
        let mut stack = Stack::new(1, 64);
        stack.added.push((
            component::CUSTOM_NAME,
            Component::Text(Tag::compound([("text", d::string("Fixture"))])),
        ));
        let first = Tag::compound([
            (
                "display",
                Tag::compound([("Name", d::string("{ \"text\" : \"Fixture\" }"))]),
            ),
            (
                "EntityTag",
                Tag::compound([("HorizonFixture", Tag::Int(42))]),
            ),
        ]);
        let mut cache = Cache::default();
        cache.remember(&stack, first.clone());
        stack.count = 1;
        stack.added.reverse();
        assert_eq!(cache.original(&stack), Some(first.clone()));
        stack.added.push((component::DAMAGE, Component::Int(7)));
        assert_eq!(cache.original(&stack), None);
        stack.added.pop();
        let second = Tag::compound([(
            "display",
            Tag::compound([("Name", d::string("{\"text\":\"Fixture\"}"))]),
        )]);
        cache.remember(&stack, second);
        assert_eq!(cache.original(&stack), None); // Two old tags share one semantic key.
        cache.remember(&stack, first);
        assert_eq!(cache.original(&stack), None); // An ambiguous key stays ambiguous.
    }

    #[test]
    fn original_nbt_cache_obeys_entry_size_total_size_and_fifo_bounds() {
        let mut cache = Cache::default();
        for item in 0..=CACHE_ENTRIES {
            cache.remember(&Stack::new(item as i32, 1), Tag::End);
        }
        assert_eq!(cache.entries.len(), CACHE_ENTRIES);
        assert_eq!(cache.original(&Stack::new(0, 1)), None);
        assert_eq!(
            cache.original(&Stack::new(CACHE_ENTRIES as i32, 1)),
            Some(Tag::End)
        );
        let mut cache = Cache::default();
        let oversized = Tag::compound([("data", Tag::ByteArray(vec![0; CACHE_ENTRY_BYTES]))]);
        cache.remember(&Stack::new(1, 1), oversized);
        assert!(cache.entries.is_empty());
        let large = Tag::compound([("data", Tag::ByteArray(vec![0; 200 * 1024]))]);
        for item in 0..64 {
            cache.remember(&Stack::new(item, 1), large.clone());
        }
        assert!(cache.bytes <= CACHE_BYTES);
        assert!(cache.entries.len() < 64);
        assert_eq!(cache.original(&Stack::new(0, 1)), None);
        assert_eq!(cache.original(&Stack::new(63, 1)), Some(large));
    }
    fn bytes(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }
    fn context() -> ids::LegacyRegistries {
        let mut out = ids::LegacyRegistries::new();
        type Entry = (i32, &'static str, &'static str);
        let registries: &[(&str, &[Entry])] = &[
            (
                "minecraft:item",
                &[
                    (1, "minecraft:stone", "minecraft:stone"),
                    (163, "minecraft:oak_log", "minecraft:oak_log"),
                    (392, "minecraft:decorated_pot", "minecraft:decorated_pot"),
                    (404, "minecraft:chest", "minecraft:chest"),
                    (656, "minecraft:shulker_box", "minecraft:shulker_box"),
                    (1009, "minecraft:arrow", "minecraft:arrow"),
                    (1012, "minecraft:diamond", "minecraft:diamond"),
                    (1050, "minecraft:diamond_sword", "minecraft:diamond_sword"),
                    (
                        1052,
                        "minecraft:diamond_pickaxe",
                        "minecraft:diamond_pickaxe",
                    ),
                    (1060, "minecraft:stick", "minecraft:stick"),
                    (
                        1069,
                        "minecraft:leather_chestplate",
                        "minecraft:leather_chestplate",
                    ),
                    (
                        1085,
                        "minecraft:diamond_chestplate",
                        "minecraft:diamond_chestplate",
                    ),
                    (1142, "minecraft:brick", "minecraft:brick"),
                    (1151, "minecraft:compass", "minecraft:compass"),
                    (1153, "minecraft:bundle", "minecraft:bundle"),
                    (1238, "minecraft:filled_map", "minecraft:filled_map"),
                    (1271, "minecraft:potion", "minecraft:potion"),
                    (1282, "minecraft:pig_spawn_egg", "minecraft:pig_spawn_egg"),
                    (1371, "minecraft:writable_book", "minecraft:writable_book"),
                    (1372, "minecraft:written_book", "minecraft:written_book"),
                    (1386, "minecraft:player_head", "minecraft:player_head"),
                    (
                        1393,
                        "minecraft:firework_rocket",
                        "minecraft:firework_rocket",
                    ),
                    (1394, "minecraft:firework_star", "minecraft:firework_star"),
                    (1395, "minecraft:enchanted_book", "minecraft:enchanted_book"),
                    (1417, "minecraft:white_banner", "minecraft:white_banner"),
                    (1491, "minecraft:crossbow", "minecraft:crossbow"),
                    (
                        1492,
                        "minecraft:suspicious_stew",
                        "minecraft:suspicious_stew",
                    ),
                    (1504, "minecraft:goat_horn", "minecraft:goat_horn"),
                    (1532, "minecraft:beehive", "minecraft:beehive"),
                    (
                        1598,
                        "minecraft:angler_pottery_sherd",
                        "minecraft:angler_pottery_sherd",
                    ),
                ],
            ),
            (
                "minecraft:block",
                &[
                    (1, "minecraft:stone", "minecraft:stone"),
                    (51, "minecraft:oak_log", "minecraft:oak_log"),
                    (245, "minecraft:chest", "minecraft:chest"),
                    (512, "minecraft:player_head", "minecraft:player_head"),
                    (617, "minecraft:white_banner", "minecraft:white_banner"),
                    (735, "minecraft:shulker_box", "minecraft:shulker_box"),
                    (1002, "minecraft:beehive", "minecraft:beehive"),
                    (1273, "minecraft:decorated_pot", "minecraft:decorated_pot"),
                ],
            ),
            (
                "minecraft:attribute",
                &[(
                    3,
                    "minecraft:generic.attack_damage",
                    "minecraft:attack_damage",
                )],
            ),
            (
                "minecraft:entity_type",
                &[
                    (6, "minecraft:arrow", "minecraft:arrow"),
                    (11, "minecraft:bee", "minecraft:bee"),
                    (54, "minecraft:firework_rocket", "minecraft:firework_rocket"),
                    (101, "minecraft:pig", "minecraft:pig"),
                ],
            ),
            (
                "minecraft:block_entity_type",
                &[
                    (1, "minecraft:chest", "minecraft:chest"),
                    (24, "minecraft:shulker_box", "minecraft:shulker_box"),
                    (33, "minecraft:beehive", "minecraft:beehive"),
                    (41, "minecraft:decorated_pot", "minecraft:decorated_pot"),
                ],
            ),
            (
                "minecraft:potion",
                &[
                    (25, "minecraft:strong_healing", "minecraft:strong_healing"),
                    (28, "minecraft:poison", "minecraft:poison"),
                ],
            ),
            (
                "minecraft:mob_effect",
                &[
                    (0, "minecraft:speed", "minecraft:speed"),
                    (18, "minecraft:poison", "minecraft:poison"),
                ],
            ),
            (
                "minecraft:trim_pattern",
                &[(1, "minecraft:coast", "minecraft:coast")],
            ),
            (
                "minecraft:enchantment",
                &[
                    (23, "minecraft:mending", "minecraft:mending"),
                    (33, "minecraft:sharpness", "minecraft:sharpness"),
                ],
            ),
            (
                "minecraft:banner_pattern",
                &[
                    (31, "minecraft:stripe_bottom", "minecraft:stripe_bottom"),
                    (38, "minecraft:stripe_top", "minecraft:stripe_top"),
                ],
            ),
            (
                "minecraft:instrument",
                &[(
                    4,
                    "minecraft:ponder_goat_horn",
                    "minecraft:ponder_goat_horn",
                )],
            ),
            (
                "minecraft:trim_material",
                &[
                    (2, "minecraft:diamond", "minecraft:diamond"),
                    (4, "minecraft:gold", "minecraft:gold"),
                ],
            ),
        ];
        for &(registry, values) in registries {
            let mut table = crate::registry::Ids::default();
            for &(id, old, current) in values {
                table
                    .names
                    .resize(table.names.len().max(id as usize + 1), String::new());
                table.names[id as usize] = old.into();
                table.ids.insert(old.into(), id);
                table.ids.insert(current.into(), id);
            }
            out.insert(registry.into(), table);
        }
        out
    }
    fn context_764() -> ids::LegacyRegistries {
        let mut out = context();
        for (registry, id, old, current) in [
            (
                "minecraft:item",
                238,
                "minecraft:grass",
                "minecraft:short_grass",
            ),
            (
                "minecraft:block",
                140,
                "minecraft:grass",
                "minecraft:short_grass",
            ),
            (
                "minecraft:item",
                1444,
                "minecraft:tipped_arrow",
                "minecraft:tipped_arrow",
            ),
            (
                "minecraft:entity_type",
                136,
                "minecraft:tnt",
                "minecraft:tnt",
            ),
        ] {
            let table = out.get_mut(registry).unwrap();
            table
                .names
                .resize(table.names.len().max(id as usize + 1), String::new());
            table.names[id as usize] = old.into();
            table.ids.insert(old.into(), id);
            table.ids.insert(current.into(), id);
        }
        out
    }
    fn normalized(mut stack: Option<Stack>) -> Option<Stack> {
        fn normalize(stack: &mut Stack) {
            stack.added.sort_by_key(|(kind, _)| *kind);
            stack.removed.sort_unstable();
            stack.wire = WirePatch::default();
            for (kind, value) in &mut stack.added {
                match value {
                    Component::Stacks(stacks) => stacks.iter_mut().for_each(normalize),
                    Component::Enchantments(levels) => levels.sort_unstable(),
                    Component::TooltipDisplay { hidden, .. } => hidden.sort_unstable(),
                    Component::Raw(data)
                        if matches!(
                            *kind,
                            49 | 59 | 60 | 61 | 62 | 68 | 76 | 77 | 79 | 81 | 82
                        ) =>
                    {
                        let mut r = Reader::new(data);
                        let mut w = Writer::new();
                        match *kind {
                            60 | 62 => {
                                w.varint(r.varint().unwrap());
                                w.nbt(&r.nbt().unwrap());
                            }
                            76 | 77 => {
                                let n = if *kind == 76 {
                                    4
                                } else {
                                    let n = r.varint().unwrap();
                                    w.varint(n);
                                    n
                                };
                                for _ in 0..n {
                                    let present = r.bool().unwrap();
                                    w.bool(present);
                                    if present {
                                        let mut item = Template::read(&mut r).unwrap();
                                        normalize(&mut item);
                                        Template::write(&item, &mut w);
                                    }
                                }
                            }
                            79 => {
                                let n = r.varint().unwrap();
                                w.varint(n);
                                for _ in 0..n {
                                    w.varint(r.varint().unwrap());
                                    w.nbt(&r.nbt().unwrap());
                                    w.varint(r.varint().unwrap());
                                    w.varint(r.varint().unwrap());
                                }
                            }
                            _ => w.nbt(&r.nbt().unwrap()),
                        }
                        assert_eq!(r.remaining(), 0);
                        *data = w.data;
                    }
                    _ => {}
                }
            }
        }
        if let Some(stack) = &mut stack {
            normalize(stack);
        }
        stack
    }
    fn legacy_parts(data: &[u8]) -> Option<(i32, i8, Tag)> {
        let mut r = Reader::new(data);
        let result = if r.bool().unwrap() {
            Some((r.varint().unwrap(), r.i8().unwrap(), r.nbt().unwrap()))
        } else {
            None
        };
        assert_eq!(r.remaining(), 0);
        result
    }
    #[test]
    fn legacy_count_boundaries_consume_nbt_and_unmapped_items_encode_empty() {
        use crate::adapter::{ServerVersion, ids::Way};
        for count in [0, 255] {
            let data = [1, 1, count, 0, 42];
            let mut r = Reader::new(&data);
            let _up = ids::renumbering(ServerVersion::V1_20_3.ids().unwrap(), Way::Up);
            assert_eq!(Slot::read(&mut r).unwrap(), None);
            assert_eq!(r.varint().unwrap(), 42);
            assert_eq!(r.remaining(), 0);
        }
        for stack in [Stack::new(i32::MAX, 1), Stack::new(1, 128)] {
            let mut w = Writer::new();
            let _down = ids::renumbering(ServerVersion::V1_20_3.ids().unwrap(), Way::Down);
            Slot::write(&Some(stack), &mut w);
            assert_eq!(w.data, [0]);
        }
    }
    #[test]
    fn modified_stack_reconstructs_nbt_without_reusing_the_original() {
        use crate::adapter::{ServerVersion, ids::Way};
        let _registries = ids::legacy_registries(Arc::new(context()));
        let _cache = ids::legacy_items(Arc::new(std::sync::Mutex::new(Cache::default())));
        let mut old = Writer::new();
        old.bool(true);
        old.varint(1);
        old.i8(1);
        old.nbt(&Tag::compound([("HorizonFixture", Tag::Int(42))]));
        let mut stack = {
            let _up = ids::renumbering(ServerVersion::V1_20_3.ids().unwrap(), Way::Up);
            Slot::read(&mut Reader::new(&old.data)).unwrap().unwrap()
        };
        stack.added.push((component::DAMAGE, Component::Int(7)));
        let mut w = Writer::new();
        {
            let _down = ids::renumbering(ServerVersion::V1_20_3.ids().unwrap(), Way::Down);
            Slot::write(&Some(stack), &mut w);
        }
        let (_, _, tag) = legacy_parts(&w.data).unwrap();
        assert_eq!(tag.get("Damage"), Some(&Tag::Int(7)));
        assert_eq!(tag.get("HorizonFixture"), Some(&Tag::Int(42)));
    }
    #[test]
    fn modified_764_grass_nested_stacks_and_predicates_keep_the_old_names() {
        use crate::adapter::{ServerVersion, ids::Way};
        let _registries = ids::legacy_registries(Arc::new(context_764()));
        let _cache = ids::legacy_items(Arc::new(std::sync::Mutex::new(Cache::default())));
        let read = |wire: &[u8]| {
            let _up = ids::renumbering(ServerVersion::V1_20_2.ids().unwrap(), Way::Up);
            Slot::read(&mut Reader::new(wire)).unwrap().unwrap()
        };
        let write = |stack: &Stack| {
            let _down = ids::renumbering(ServerVersion::V1_20_2.ids().unwrap(), Way::Down);
            let mut w = Writer::new();
            Slot::write(&Some(stack.clone()), &mut w);
            w.data
        };
        let mut grass = read(&bytes("01ad010100"));
        grass.added.push((component::DAMAGE, Component::Int(7)));
        let wire = write(&grass);
        let (id, _, tag) = legacy_parts(&wire).unwrap();
        assert_eq!(id, 173); // The old grass item id, rather than the pinned short_grass id.
        assert_eq!(tag.get("Damage"), Some(&Tag::Int(7)));
        assert_eq!(normalized(Some(read(&wire))), normalized(Some(grass)));

        let mut bundle = read(&bytes(
            "01fa06010a0900054974656d730a000000010800026964000f6d696e6563726166743a6772617373010005436f756e74020000",
        ));
        let Component::Stacks(items) =
            &mut bundle.added.iter_mut().find(|(id, _)| *id == 52).unwrap().1
        else {
            panic!("bundle")
        };
        items[0].count = 3;
        let wire = write(&bundle);
        let (_, _, tag) = legacy_parts(&wire).unwrap();
        let Tag::List(items) = tag.get("Items").unwrap() else {
            panic!("items")
        };
        assert_eq!(items[0].get("id"), Some(&d::string("minecraft:grass")));
        assert_eq!(items[0].get("Count"), Some(&Tag::Byte(3)));
        assert_eq!(normalized(Some(read(&wire))), normalized(Some(bundle)));

        let mut pickaxe = read(&bytes(
            "019f06010a03000644616d6167650000000009000a43616e506c6163654f6e0800000001000f6d696e6563726166743a677261737309000a43616e44657374726f790800000001000f6d696e6563726166743a677261737300",
        ));
        pickaxe.added.push((component::DAMAGE, Component::Int(7)));
        let wire = write(&pickaxe);
        let (_, _, tag) = legacy_parts(&wire).unwrap();
        for field in ["CanDestroy", "CanPlaceOn"] {
            assert_eq!(
                tag.get(field),
                Some(&Tag::List(vec![d::string("minecraft:grass")]))
            );
        }
        assert_eq!(normalized(Some(read(&wire))), normalized(Some(pickaxe)));
    }

    #[test]
    fn modified_764_projectile_entity_data_exposes_the_reverse_schema_limit() {
        use crate::adapter::{ServerVersion, ids::Way};
        let _registries = ids::legacy_registries(Arc::new(context_764()));
        let cache = Arc::new(std::sync::Mutex::new(Cache::default()));
        let _cache = ids::legacy_items(cache.clone());
        let old = bytes(
            "0101010a0a0009456e74697479546167080006506f74696f6e00106d696e6563726166743a706f69736f6e0800026964000f6d696e6563726166743a6172726f770000",
        );
        let mut stack = {
            let _up = ids::renumbering(ServerVersion::V1_20_2.ids().unwrap(), Way::Up);
            Slot::read(&mut Reader::new(&old)).unwrap().unwrap()
        };
        assert!(cache.lock().unwrap().original(&stack).is_some());
        let Component::Raw(data) = &mut stack.added.iter_mut().find(|(id, _)| *id == 60).unwrap().1
        else {
            panic!("entity data")
        };
        let mut r = Reader::new(data);
        let kind = r.varint().unwrap();
        let mut entity = r.nbt().unwrap();
        d::set(entity.get_mut("item").unwrap(), "count", Tag::Int(2));
        let mut w = Writer::new();
        w.varint(kind);
        w.nbt(&entity);
        *data = w.data;
        assert_eq!(cache.lock().unwrap().original(&stack), None);
        let mut w = Writer::new();
        {
            let _down = ids::renumbering(ServerVersion::V1_20_2.ids().unwrap(), Way::Down);
            Slot::write(&Some(stack), &mut w);
        }
        let (_, _, tag) = legacy_parts(&w.data).unwrap();
        let entity = tag.get("EntityTag").unwrap();
        assert_eq!(entity.get("id"), Some(&d::string("minecraft:arrow")));
        assert_eq!(entity.get("item").unwrap().get("count"), Some(&Tag::Int(2)));
        // Semantic fallback retains the edited raw data, but does not fabricate an old Potion.
        // 764 reads Potion instead of the modern item field: this is not gameplay parity.
        assert!(entity.get("Potion").is_none());
    }

    fn check_legacy_fixture(
        version: crate::adapter::ServerVersion,
        label: &str,
        old: &str,
        pinned: &str,
        semantic_downgrade: bool,
    ) {
        use crate::adapter::ids::Way;
        let old = bytes(old);
        let pinned = bytes(pinned);
        let mut r = Reader::new(&old);
        let stack = {
            let _up = ids::renumbering(version.ids().unwrap(), Way::Up);
            Slot::read(&mut r).unwrap_or_else(|e| panic!("{label}: {e}"))
        };
        assert_eq!(r.remaining(), 0, "{label}");
        let expected =
            Slot::read(&mut Reader::new(&pinned)).unwrap_or_else(|e| panic!("{label} pinned: {e}"));
        assert_eq!(normalized(stack.clone()), normalized(expected), "{label}");
        if semantic_downgrade {
            let mut w = Writer::new();
            {
                let _down = ids::renumbering(version.ids().unwrap(), Way::Down);
                Slot::write(&stack, &mut w);
            }
            let returned = {
                let _up = ids::renumbering(version.ids().unwrap(), Way::Up);
                Slot::read(&mut Reader::new(&w.data))
                    .unwrap_or_else(|e| panic!("{label} returned: {e}"))
            };
            assert_eq!(
                normalized(returned),
                normalized(stack),
                "{label} semantic downgrade"
            );
        }
        let _cache = ids::legacy_items(Arc::new(std::sync::Mutex::new(Cache::default())));
        let cached = {
            let _up = ids::renumbering(version.ids().unwrap(), Way::Up);
            Slot::read(&mut Reader::new(&old)).unwrap()
        };
        let mut w = Writer::new();
        {
            let _down = ids::renumbering(version.ids().unwrap(), Way::Down);
            DelimitedSlot::write(&cached, &mut w);
        }
        assert_eq!(
            legacy_parts(&w.data),
            legacy_parts(&old),
            "{label} original tag identity"
        );
    }
    #[test]
    fn vanilla_legacy_slots_match_current_datafix_and_component_encoder() {
        use crate::adapter::ServerVersion;
        let _registries = ids::legacy_registries(Arc::new(context()));
        // Actual FriendlyByteBuf.writeItem output is identical in 1.20.3/1.20.4. The pinned
        // fixture is vanilla26.3 DataFixer.update(ITEM_STACK,3698,CURRENT), parsed by its
        // DataComponentPatch.CODEC and encoded by its real STREAM_CODEC. Test registry ids
        // use that oracle's vanilla registries; connections instead retain their own ids.
        for (label, old, pinned) in [
            ("empty", "00", "00"),
            ("plain", "01010100", "01010000"),
            ("count64", "01014000", "40010000"),
            ("count127", "01017f00", "7f010000"),
            ("zero", "00", "00"),
            ("negative", "00", "00"),
            (
                "ordinary",
                "01c206010a03000a526570616972436f73740000000301000b556e627265616b61626c650103000f437573746f6d4d6f64656c446174610000000703000644616d616765000000110a0007646973706c61790900044c6f72650800000001000f7b2274657874223a224c6f7265227d0800044e616d6500257b2274657874223a22486f72697a6f6e46697874757265222c22626f6c64223a747275657d000a000e486f72697a6f6e4669787475726503000576616c75650000002a0000",
                "019a080700110140e000000000000b010800044c6f7265031104060a08000474657874000e486f72697a6f6e46697874757265010004626f6c640100000a0a000e486f72697a6f6e4669787475726503000576616c75650000002a00001303",
            ),
            (
                "enchants",
                "01d308010a09001253746f726564456e6368616e746d656e74730a000000010200036c766c0001080002696400116d696e6563726166743a6d656e64696e670003000948696465466c6167730000002109000c456e6368616e746d656e74730a000000010200036c766c0003080002696400136d696e6563726166743a73686172706e6573730000",
                "01f30a03002d0117011200020d2d0d012103",
            ),
            (
                "potion",
                "01e307010a090015637573746f6d5f706f74696f6e5f656666656374730a000000010300086475726174696f6e00000028010009616d706c696669657202010007616d6269656e740101000973686f775f69636f6e00080002696400106d696e6563726166743a706f69736f6e01000e73686f775f7061727469636c65730000080006506f74696f6e00186d696e6563726166743a7374726f6e675f6865616c696e67030011437573746f6d506f74696f6e436f6c6f720001e24000",
                "01f7090100350119010001e240011202280100000000",
            ),
            (
                "attributes",
                "01cc06010a03000948696465466c616773000000020900124174747269627574654d6f646966696572730a00000001080004536c6f7400086d61696e68616e6408000d4174747269627574654e616d65001f6d696e6563726166743a67656e657269632e61747461636b5f64616d6167650300094f7065726174696f6e000000000b00045555494400000004cb3f55d3645c4f38a4979c13a33db5cf060006416d6f756e743ff80000000000000800044e616d65000f576561706f6e206d6f6469666965720000",
                "01a4080200120001101001031c6d696e6563726166743a626173655f61747461636b5f64616d6167653ff8000000000000000100",
            ),
            (
                "predicate",
                "01c406010a03000644616d6167650000000003000948696465466c6167730000001809000a43616e506c6163654f6e0800000001000f6d696e6563726166743a73746f6e6509000a43616e44657374726f790800000003000f6d696e6563726166743a73746f6e65000f236d696e6563726166743a6c6f677300196d696e6563726166743a6f616b5f6c6f675b617869733d795d00",
                "019c0803000e01010201000000001200020e0f0f030102010000000001000e6d696e6563726166743a6c6f67730000000001023301010461786973010179000000",
            ),
            (
                "profile",
                "01ca08010a0a000a536b756c6c4f776e65720a000a50726f7065727469657309000874657874757265730a0000000108000556616c75650007666978747572650800095369676e6174757265000973796e74686574696300000b0002496400000004000000000000000000000000000000010800044e616d65000e486f72697a6f6e466978747572650000",
                "01ea0a01004801000000000000000000000000000000010e486f72697a6f6e46697874757265010874657874757265730766697874757265010973796e74686574696300000000",
            ),
            (
                "fireworks",
                "01d108030a0a000946697265776f726b73010006466c696768740209000a4578706c6f73696f6e730a0000000101000454797065010b0006436f6c6f72730000000200ff0000000000ff010005547261696c010b000a46616465436f6c6f7273000000010000ff00010007466c69636b657201000000",
                "03f10a0100470201010200ff0000000000ff010000ff000101",
            ),
            (
                "firework_star",
                "01d208010a0a00094578706c6f73696f6e01000454797065020b0006436f6c6f72730000000100ff0000010005547261696c010000",
                "01f20a010046020100ff0000000100",
            ),
            (
                "written_book",
                "01be08010a03000a67656e65726174696f6e0000000109000570616765730800000001001e7b2274657874223a2250616765222c22636f6c6f72223a22676f6c64227d0800057469746c65000746697874757265080006617574686f72000e486f72697a6f6e466978747572650a000e66696c74657265645f70616765730800013000137b2274657874223a2246696c7465726564227d000100087265736f6c7665640100",
                "01dc0a0100390746697874757265000e486f72697a6f6e4669787475726501010a080005636f6c6f720004676f6c6408000474657874000450616765000108000846696c746572656401",
            ),
            (
                "writable_book",
                "01bd08010a090005706167657308000000020005466972737400065365636f6e640a000e66696c74657265645f706167657308000131000846696c74657265640000",
                "01db0a0100380205466972737400065365636f6e64010846696c7465726564",
            ),
            (
                "crossbow",
                "019e09010a03000644616d616765000000000900124368617267656450726f6a656374696c65730a000000020800026964000f6d696e6563726166743a6172726f77010005436f756e740100080002696400196d696e6563726166743a66697265776f726b5f726f636b6574010005436f756e74010a00037461670a000946697265776f726b73010006466c6967687402000000010007436861726765640100",
                "01d30b01003302f107010000f10a010100470200",
            ),
            (
                "bundle",
                "019f07010a0900054974656d730a000000020800026964000f6d696e6563726166743a73746f6e65010005436f756e740200080002696400176d696e6563726166743a6469616d6f6e645f73776f7264010005436f756e74010a000374616703000644616d61676500000009000000",
                "01810901003402010200009a080101000309",
            ),
            (
                "container",
                "018904010a0a000e426c6f636b456e7469747954616708000a437573746f6d4e616d65000e7b2274657874223a22426f78227d0900054974656d730a00000001010004536c6f7402080002696400116d696e6563726166743a6469616d6f6e64010005436f756e740300080002696400156d696e6563726166743a7368756c6b65725f626f780000",
                "01900502004d03000001f4070300003e180a0a000a437573746f6d4e616d65080004746578740003426f780000",
            ),
            (
                "block_state",
                "018301010a0a000d426c6f636b5374617465546167080004617869730001790000",
                "01a30101004e0104617869730179",
            ),
            (
                "compass",
                "019d07010a0800124c6f646573746f6e6544696d656e73696f6e00136d696e6563726166743a6f766572776f726c640a000c4c6f646573746f6e65506f73030001580000000a03000159000000400300015afffffffe000100104c6f646573746f6e65547261636b65640000",
                "01ff0801004501136d696e6563726166743a6f766572776f726c64000002bfffffe04000",
            ),
            (
                "goat_horn",
                "01a709010a08000a696e737472756d656e74001a6d696e6563726166743a706f6e6465725f676f61745f686f726e00",
                "01e00b01003f05",
            ),
            (
                "banner",
                "01e608010a03000948696465466c616773000000200a000e426c6f636b456e746974795461670900085061747465726e730a000000020800075061747465726e00026273030005436f6c6f720000000e000800075061747465726e00027473030005436f6c6f720000000b000000",
                "01890b02004a02200e270b1200014a",
            ),
            (
                "dyed",
                "01d606010a03000644616d6167650000000003000948696465466c616773000000400a0007646973706c6179030005636f6c6f7200ff00000000",
                "01ad0802001200012f2f00ff0000",
            ),
            ("map", "01d307010a0300036d61700000001100", "01d60901003011"),
            (
                "stew",
                "019f09010a090007656666656374730a000000010300086475726174696f6e000000140800026964000f6d696e6563726166743a73706565640000",
                "01d40b010037010014",
            ),
            (
                "pot",
                "019f02010a0a000e426c6f636b456e746974795461670900067368657264730800000004000f6d696e6563726166743a627269636b001e6d696e6563726166743a616e676c65725f706f74746572795f7368657264000f6d696e6563726166743a627269636b000f6d696e6563726166743a627269636b0000",
                "01880301004c01f60801000001be0c01000001f60801000001f608010000",
            ),
            (
                "trim",
                "01e206010a03000644616d616765000000000a00045472696d0800086d6174657269616c000e6d696e6563726166743a676f6c640800077061747465726e000f6d696e6563726166743a636f6173740003000948696465466c6167730000008000",
                "01bd0802003a05021200013a",
            ),
            ("empty_tag", "0101010a00", "01010000"),
            (
                "entity",
                "019408010a0a0009456e7469747954616708000a437573746f6d4e616d6500197b2274657874223a22486f72697a6f6e46697874757265227d0100044e6f41490103000e486f72697a6f6e46697874757265000000070000",
                "01820a01003c650a0a000a437573746f6d4e616d6508000474657874000e486f72697a6f6e466978747572650003000e486f72697a6f6e46697874757265000000070100044e6f41490100",
            ),
            (
                "predicate_nbt",
                "01c406010a03000644616d6167650000000009000a43616e44657374726f79080000000100226d696e6563726166743a73746f6e657b486f72697a6f6e466978747572653a31627d00",
                "019c0801000f0101020100010a01000e486f72697a6f6e4669787475726501000000",
            ),
            (
                "bees",
                "01bb09010a0a000e426c6f636b456e74697479546167090004426565730a000000010a000a456e746974794461746108000a437573746f6d4e616d6500197b2274657874223a22486f72697a6f6e46697874757265227d0800026964000d6d696e6563726166743a6265650003000b5469636b73496e486976650000000a0300124d696e4f636375706174696f6e5469636b7300000258000000",
                "01fc0b01004f010b0a0a000a437573746f6d4e616d6508000474657874000e486f72697a6f6e4669787475726500000ad804",
            ),
            (
                "lock_loot",
                "01aa02010a0a000e426c6f636b456e746974795461670800094c6f6f745461626c65001f6d696e6563726166743a6368657374732f73696d706c655f64756e67656f6e0800044c6f636b00034b657904000d4c6f6f745461626c6553656564000000000000007b0000",
                "0194030200510a0a000a636f6d706f6e656e74730800156d696e6563726166743a637573746f6d5f6e616d650005224b6579220000520a04000473656564000000000000007b08000a6c6f6f745f7461626c65001f6d696e6563726166743a6368657374732f73696d706c655f64756e67656f6e00",
            ),
            (
                "map_decor",
                "01d307010a09000b4465636f726174696f6e730a000000010600017840240000000000000600017ac00000000000000008000269640007666978747572650100047479706500060003726f7440568000000000000000",
                "01d6090100310a0a000766697874757265050008726f746174696f6e42b400000600017840240000000000000600017ac0000000000000000800047479706500106d696e6563726166743a706c617965720000",
            ),
        ] {
            check_legacy_fixture(ServerVersion::V1_20_3, label, old, pinned, true);
        }
    }

    #[test]
    fn vanilla_1_20_2_slots_match_datafix_3578_and_current_component_encoder() {
        use crate::adapter::ServerVersion;
        let _registries = ids::legacy_registries(Arc::new(context_764()));
        // Independent official 1.20.2 writeItem/readItem output. Expected values use the
        // current vanilla DFU from DataVersion3578, then its component codec and encoder.
        // The extra grass, predicate and projectile cases distinguish the older fix range.
        for (label, old, pinned) in [
            ("empty", "00", "00"),
            ("plain", "01010100", "01010000"),
            ("count64", "01014000", "40010000"),
            ("count127", "01017f00", "7f010000"),
            ("zero", "00", "00"),
            ("negative", "00", "00"),
            (
                "ordinary",
                "019d06010a03000a526570616972436f73740000000301000b556e627265616b61626c650103000f437573746f6d4d6f64656c44617461000000070a0007646973706c61790900044c6f72650800000001000f7b2274657874223a224c6f7265227d0800044e616d6500257b2274657874223a22486f72697a6f6e46697874757265222c22626f6c64223a747275657d000a000e486f72697a6f6e4669787475726503000576616c75650000002a0003000644616d6167650000001100",
                "019a080700110140e000000000000b010800044c6f7265031104060a08000474657874000e486f72697a6f6e46697874757265010004626f6c640100000a0a000e486f72697a6f6e4669787475726503000576616c75650000002a00001303",
            ),
            (
                "enchants",
                "01ac08010a03000948696465466c6167730000002109000c456e6368616e746d656e74730a000000010200036c766c0003080002696400136d696e6563726166743a73686172706e6573730009001253746f726564456e6368616e746d656e74730a000000010200036c766c0001080002696400116d696e6563726166743a6d656e64696e670000",
                "01f30a03002d0117011200020d2d0d012103",
            ),
            (
                "potion",
                "01bd07010a090015637573746f6d5f706f74696f6e5f656666656374730a000000010300086475726174696f6e0000002801000973686f775f69636f6e00010009616d706c696669657202010007616d6269656e7401080002696400106d696e6563726166743a706f69736f6e01000e73686f775f7061727469636c65730000030011437573746f6d506f74696f6e436f6c6f720001e240080006506f74696f6e00186d696e6563726166743a7374726f6e675f6865616c696e6700",
                "01f7090100350119010001e240011202280100000000",
            ),
            (
                "attributes",
                "01a706010a03000948696465466c616773000000020900124174747269627574654d6f646966696572730a00000001060006416d6f756e743ff8000000000000080004536c6f7400086d61696e68616e6408000d4174747269627574654e616d65001f6d696e6563726166743a67656e657269632e61747461636b5f64616d6167650300094f7065726174696f6e000000000b00045555494400000004cb3f55d3645c4f38a4979c13a33db5cf0800044e616d65000f576561706f6e206d6f6469666965720000",
                "01a4080200120001101001031c6d696e6563726166743a626173655f61747461636b5f64616d6167653ff8000000000000000100",
            ),
            (
                "predicate",
                "019f06010a03000948696465466c6167730000001803000644616d6167650000000009000a43616e506c6163654f6e0800000001000f6d696e6563726166743a73746f6e6509000a43616e44657374726f790800000003000f6d696e6563726166743a73746f6e65000f236d696e6563726166743a6c6f677300196d696e6563726166743a6f616b5f6c6f675b617869733d795d00",
                "019c0803000e01010201000000001200020e0f0f030102010000000001000e6d696e6563726166743a6c6f67730000000001023301010461786973010179000000",
            ),
            (
                "profile",
                "01a308010a0a000a536b756c6c4f776e65720b0002496400000004000000000000000000000000000000010a000a50726f7065727469657309000874657874757265730a000000010800095369676e6174757265000973796e74686574696308000556616c756500076669787475726500000800044e616d65000e486f72697a6f6e466978747572650000",
                "01ea0a01004801000000000000000000000000000000010e486f72697a6f6e46697874757265010874657874757265730766697874757265010973796e74686574696300000000",
            ),
            (
                "fireworks",
                "01aa08030a0a000946697265776f726b73010006466c696768740209000a4578706c6f73696f6e730a0000000101000454797065010b0006436f6c6f72730000000200ff0000000000ff010005547261696c010b000a46616465436f6c6f7273000000010000ff00010007466c69636b657201000000",
                "03f10a0100470201010200ff0000000000ff010000ff000101",
            ),
            (
                "firework_star",
                "01ab08010a0a00094578706c6f73696f6e01000454797065020b0006436f6c6f72730000000100ff0000010005547261696c010000",
                "01f20a010046020100ff0000000100",
            ),
            (
                "written_book",
                "019708010a03000a67656e65726174696f6e0000000109000570616765730800000001001e7b2274657874223a2250616765222c22636f6c6f72223a22676f6c64227d080006617574686f72000e486f72697a6f6e466978747572650a000e66696c74657265645f70616765730800013000137b2274657874223a2246696c7465726564227d000800057469746c650007466978747572650100087265736f6c7665640100",
                "01dc0a0100390746697874757265000e486f72697a6f6e4669787475726501010a080005636f6c6f720004676f6c6408000474657874000450616765000108000846696c746572656401",
            ),
            (
                "writable_book",
                "019608010a090005706167657308000000020005466972737400065365636f6e640a000e66696c74657265645f706167657308000131000846696c74657265640000",
                "01db0a0100380205466972737400065365636f6e64010846696c7465726564",
            ),
            (
                "crossbow",
                "01f708010a0900124368617267656450726f6a656374696c65730a000000020800026964000f6d696e6563726166743a6172726f77010005436f756e740100080002696400196d696e6563726166743a66697265776f726b5f726f636b6574010005436f756e74010a00037461670a000946697265776f726b73010006466c6967687402000000010007436861726765640103000644616d6167650000000000",
                "01d30b01003302f107010000f10a010100470200",
            ),
            (
                "bundle",
                "01fa06010a0900054974656d730a000000020800026964000f6d696e6563726166743a73746f6e65010005436f756e740200080002696400176d696e6563726166743a6469616d6f6e645f73776f7264010005436f756e74010a000374616703000644616d61676500000009000000",
                "01810901003402010200009a080101000309",
            ),
            (
                "container",
                "01f403010a0a000e426c6f636b456e7469747954616708000a437573746f6d4e616d65000e7b2274657874223a22426f78227d0900054974656d730a00000001010004536c6f7402080002696400116d696e6563726166743a6469616d6f6e64010005436f756e740300080002696400156d696e6563726166743a7368756c6b65725f626f780000",
                "01900502004d03000001f4070300003e180a0a000a437573746f6d4e616d65080004746578740003426f780000",
            ),
            (
                "block_state",
                "016e010a0a000d426c6f636b5374617465546167080004617869730001790000",
                "01a30101004e0104617869730179",
            ),
            (
                "compass",
                "01f806010a0800124c6f646573746f6e6544696d656e73696f6e00136d696e6563726166743a6f766572776f726c640a000c4c6f646573746f6e65506f73030001580000000a03000159000000400300015afffffffe000100104c6f646573746f6e65547261636b65640000",
                "01ff0801004501136d696e6563726166743a6f766572776f726c64000002bfffffe04000",
            ),
            (
                "goat_horn",
                "018009010a08000a696e737472756d656e74001a6d696e6563726166743a706f6e6465725f676f61745f686f726e00",
                "01e00b01003f05",
            ),
            (
                "banner",
                "01bf08010a03000948696465466c616773000000200a000e426c6f636b456e746974795461670900085061747465726e730a000000020800075061747465726e00026273030005436f6c6f720000000e000800075061747465726e00027473030005436f6c6f720000000b000000",
                "01890b02004a02200e270b1200014a",
            ),
            (
                "dyed",
                "01b106010a03000948696465466c616773000000400a0007646973706c6179030005636f6c6f7200ff00000003000644616d6167650000000000",
                "01ad0802001200012f2f00ff0000",
            ),
            ("map", "01ad07010a0300036d61700000001100", "01d60901003011"),
            (
                "stew",
                "01f808010a090007656666656374730a000000010300086475726174696f6e000000140800026964000f6d696e6563726166743a73706565640000",
                "01d40b010037010014",
            ),
            (
                "pot",
                "018a02010a0a000e426c6f636b456e746974795461670900067368657264730800000004000f6d696e6563726166743a627269636b001e6d696e6563726166743a616e676c65725f706f74746572795f7368657264000f6d696e6563726166743a627269636b000f6d696e6563726166743a627269636b0000",
                "01880301004c01f60801000001be0c01000001f60801000001f608010000",
            ),
            (
                "trim",
                "01bd06010a0a00045472696d0800086d6174657269616c000e6d696e6563726166743a676f6c640800077061747465726e000f6d696e6563726166743a636f6173740003000948696465466c6167730000008003000644616d6167650000000000",
                "01bd0802003a05021200013a",
            ),
            ("empty_tag", "0101010a00", "01010000"),
            (
                "entity",
                "01ed07010a0a0009456e7469747954616708000a437573746f6d4e616d6500197b2274657874223a22486f72697a6f6e46697874757265227d03000e486f72697a6f6e46697874757265000000070100044e6f4149010000",
                "01820a01003c650a0a000a437573746f6d4e616d6508000474657874000e486f72697a6f6e466978747572650003000e486f72697a6f6e46697874757265000000070100044e6f41490100",
            ),
            (
                "predicate_nbt",
                "019f06010a03000644616d6167650000000009000a43616e44657374726f79080000000100226d696e6563726166743a73746f6e657b486f72697a6f6e466978747572653a31627d00",
                "019c0801000f0101020100010a01000e486f72697a6f6e4669787475726501000000",
            ),
            (
                "bees",
                "019409010a0a000e426c6f636b456e74697479546167090004426565730a000000010300124d696e4f636375706174696f6e5469636b730000025803000b5469636b73496e486976650000000a0a000a456e746974794461746108000a437573746f6d4e616d6500197b2274657874223a22486f72697a6f6e46697874757265227d0800026964000d6d696e6563726166743a62656500000000",
                "01fc0b01004f010b0a0a000a437573746f6d4e616d6508000474657874000e486f72697a6f6e4669787475726500000ad804",
            ),
            (
                "lock_loot",
                "019502010a0a000e426c6f636b456e746974795461670800094c6f6f745461626c65001f6d696e6563726166743a6368657374732f73696d706c655f64756e67656f6e0800044c6f636b00034b657904000d4c6f6f745461626c6553656564000000000000007b0000",
                "0194030200510a0a000a636f6d706f6e656e74730800156d696e6563726166743a637573746f6d5f6e616d650005224b6579220000520a04000473656564000000000000007b08000a6c6f6f745f7461626c65001f6d696e6563726166743a6368657374732f73696d706c655f64756e67656f6e00",
            ),
            (
                "map_decor",
                "01ad07010a09000b4465636f726174696f6e730a00000001060003726f7440568000000000000600017840240000000000000600017ac000000000000000080002696400076669787475726501000474797065000000",
                "01d6090100310a0a000766697874757265050008726f746174696f6e42b400000600017840240000000000000600017ac0000000000000000800047479706500106d696e6563726166743a706c617965720000",
            ),
            ("grass", "01ad010100", "01ee010000"),
            (
                "nested_grass",
                "01fa06010a0900054974656d730a000000010800026964000f6d696e6563726166743a6772617373010005436f756e74020000",
                "01810901003401ee01020000",
            ),
            (
                "predicate_grass",
                "019f06010a03000644616d6167650000000009000a43616e506c6163654f6e0800000001000f6d696e6563726166743a677261737309000a43616e44657374726f790800000001000f6d696e6563726166743a677261737300",
                "019c0802000e0101028c01000000000f0101028c0100000000",
            ),
            (
                "tnt_entity",
                "0101010a0a0009456e746974795461670200044675736500280800026964000d6d696e6563726166743a746e740000",
                "010101003c88010a0200044675736500280200046675736500280a000b626c6f636b5f73746174650800026964000d6d696e6563726166743a746e740000",
            ),
            (
                "arrow_entity",
                "0101010a0a0009456e74697479546167080006506f74696f6e00106d696e6563726166743a706f69736f6e0800026964000f6d696e6563726166743a6172726f770000",
                "010101003c060a0a00046974656d0a000a636f6d706f6e656e74730a00196d696e6563726166743a706f74696f6e5f636f6e74656e7473080006706f74696f6e00106d696e6563726166743a706f69736f6e0000030005636f756e7400000001080002696400166d696e6563726166743a7469707065645f6172726f770000",
            ),
        ] {
            check_legacy_fixture(
                ServerVersion::V1_20_2,
                label,
                old,
                pinned,
                // A raw entity component has no general reverse schema conversion.
                label != "arrow_entity",
            );
        }
    }
}
