//! Fixes of 1.12.2 to 1.13.2 (keys 1450 to 1624, DataVersion 1343 to 1631): the
//! flattening and what came with it. The options fixes of this range (1344, 1446)
//! are client settings, not world data.
use std::collections::HashMap;
use std::sync::OnceLock;

use super::block_state_data as bsd;
use super::chunk_leaves;
use super::dynamic::{
    as_int, as_number, as_str, empty_map, is_list, namespaced, remove, set, stream, string,
};
use super::schema::{Key, Ty, key};
use super::tables as t;
use super::{Cx, FixFn, items, mark_read, reread, reread_as};
use crate::nbt::{Compound, JavaString, Tag};

pub(super) fn register(out: &mut Vec<(Key, FixFn)>) {
    out.push((key(1450, 0), |cx| {
        cx.each(Ty::BlockState, |s| *s = bsd::upgrade_block_state_tag(s))
    }));
    out.push((key(1451, 1), chunk_palettes));
    out.push((key(1451, 2), piston_block_state));
    out.push((key(1451, 3), entity_block_state));
    out.push((key(1451, 3), map_id));
    out.push((key(1451, 4), block_name_flattening));
    out.push((key(1451, 4), item_flattening));
    out.push((key(1451, 5), remove_block_entity_tags));
    out.push((key(1451, 5), spawn_eggs));
    out.push((key(1451, 5), |cx| {
        cx.entity("minecraft:wolf", |e| {
            if let Some(c) = e.get("CollarColor") {
                let color = 15 - as_int(Some(c), 0);
                set(e, "CollarColor", Tag::Byte(color as i8));
            }
        })
    }));
    out.push((key(1451, 5), banner_color));
    out.push((key(1451, 5), flat_generator_info));
    out.push((key(1451, 6), stats_counters));
    out.push((key(1451, 6), jukebox_record));
    out.push((key(1451, 7), villager_trades));
    out.push((key(1456, 0), |cx| {
        cx.entity("minecraft:item_frame", |e| {
            let facing = match as_int(e.get("Facing"), 0) as i8 {
                0 => 3,
                1 => 4,
                3 => 5,
                _ => 2,
            };
            set(e, "Facing", Tag::Byte(facing));
        })
    }));
    out.push((key(1458, 0), entity_custom_name));
    out.push((key(1458, 0), item_custom_name));
    out.push((key(1458, 0), block_entity_custom_name));
    out.push((key(1460, 0), painting_motive));
    out.push((key(1466, 0), chunk_to_protochunk));
    out.push((key(1474, 0), |cx| {
        cx.entity("minecraft:shulker", |e| {
            if as_int(e.get("Color"), 0) == 10 {
                set(e, "Color", Tag::Byte(16));
            }
        })
    }));
    out.push((key(1474, 0), |cx| {
        legacy_block_rename(
            cx,
            &only("minecraft:purple_shulker_box", "minecraft:shulker_box"),
        )
    }));
    out.push((key(1474, 0), |cx| {
        item_rename(
            cx,
            &only("minecraft:purple_shulker_box", "minecraft:shulker_box"),
        )
    }));
    out.push((key(1475, 0), |cx| {
        legacy_block_rename(
            cx,
            &table(&[
                ("minecraft:flowing_water", "minecraft:water"),
                ("minecraft:flowing_lava", "minecraft:lava"),
            ]),
        )
    }));
    out.push((key(1480, 0), |cx| {
        legacy_block_rename(cx, &table(t::tables().coral))
    }));
    out.push((key(1480, 0), |cx| {
        item_rename(cx, &table(t::tables().coral))
    }));
    out.push((key(1483, 0), |cx| {
        entity_rename(cx, &only("minecraft:puffer_fish", "minecraft:pufferfish"))
    }));
    out.push((key(1483, 0), |cx| {
        item_rename(
            cx,
            &only(
                "minecraft:puffer_fish_spawn_egg",
                "minecraft:pufferfish_spawn_egg",
            ),
        )
    }));
    const SEAGRASS: &[(&str, &str)] = &[
        ("minecraft:sea_grass", "minecraft:seagrass"),
        ("minecraft:tall_sea_grass", "minecraft:tall_seagrass"),
    ];
    out.push((key(1484, 0), |cx| item_rename(cx, &table(SEAGRASS))));
    out.push((key(1484, 0), |cx| legacy_block_rename(cx, &table(SEAGRASS))));
    out.push((key(1484, 0), heightmap_renaming));
    out.push((key(1486, 0), |cx| {
        entity_rename(cx, &table(t::tables().cod_salmon))
    }));
    out.push((key(1486, 0), |cx| {
        item_rename(cx, &table(t::tables().cod_salmon_eggs))
    }));
    const PRISMARINE: &[(&str, &str)] = &[
        (
            "minecraft:prismarine_bricks_slab",
            "minecraft:prismarine_brick_slab",
        ),
        (
            "minecraft:prismarine_bricks_stairs",
            "minecraft:prismarine_brick_stairs",
        ),
    ];
    out.push((key(1487, 0), |cx| item_rename(cx, &table(PRISMARINE))));
    out.push((key(1487, 0), |cx| {
        legacy_block_rename(cx, &table(PRISMARINE))
    }));
    out.push((key(1488, 0), |cx| {
        legacy_block_rename(
            cx,
            &table(&[
                ("minecraft:kelp_top", "minecraft:kelp"),
                ("minecraft:kelp", "minecraft:kelp_plant"),
            ]),
        )
    }));
    out.push((key(1488, 0), |cx| {
        item_rename(cx, &only("minecraft:kelp_top", "minecraft:kelp"))
    }));
    out.push((key(1488, 0), command_block_custom_name));
    out.push((key(1488, 0), command_block_minecart_custom_name));
    out.push((key(1488, 0), igloo_metadata_removal));
    out.push((key(1490, 0), |cx| {
        legacy_block_rename(cx, &only("minecraft:melon_block", "minecraft:melon"))
    }));
    out.push((key(1490, 0), |cx| {
        item_rename(
            cx,
            &table(&[
                ("minecraft:melon_block", "minecraft:melon"),
                ("minecraft:melon", "minecraft:melon_slice"),
                (
                    "minecraft:speckled_melon",
                    "minecraft:glistering_melon_slice",
                ),
            ]),
        )
    }));
    out.push((key(1492, 0), structure_template_rename));
    out.push((key(1494, 0), enchantment_names));
    out.push((key(1496, 0), |cx| {
        cx.each(Ty::Chunk, chunk_leaves::leaves_fix)
    }));
    out.push((key(1500, 0), |cx| {
        cx.block_entity("DUMMY", |be| set(be, "keepPacked", Tag::Byte(1)))
    }));
    out.push((key(1501, 0), advancements_rename));
    out.push((key(1502, 0), |cx| {
        rename_names(cx, Ty::Recipe, &table(t::tables().recipes))
    }));
    out.push((key(1506, 0), level_generator_options));
    out.push((key(1510, 0), |cx| {
        legacy_block_rename(cx, &table(t::tables().the_renamening_blocks))
    }));
    out.push((key(1510, 0), |cx| item_rename(cx, &the_renamening_items)));
    out.push((key(1510, 0), |cx| {
        rename_names(cx, Ty::Recipe, &table(t::tables().recipes_renamening))
    }));
    out.push((key(1510, 0), |cx| {
        entity_rename(cx, &the_renamening_entities)
    }));
    out.push((key(1510, 0), swim_stats_rename));
    out.push((key(1514, 0), |cx| {
        display_name_to_component(cx, Ty::Objective)
    }));
    out.push((key(1514, 0), |cx| display_name_to_component(cx, Ty::Team)));
    out.push((key(1514, 0), objective_render_type));
    out.push((key(1515, 0), |cx| {
        legacy_block_rename(cx, &table(t::tables().coral_fans))
    }));
    out.push((key(1624, 0), |cx| {
        cx.each(Ty::Chunk, chunk_leaves::trapped_chest_fix)
    }));
}

// ------------------------------------------------------------------ helpers

fn lookup(table: &'static [(&'static str, &'static str)], k: &str) -> Option<&'static str> {
    table.iter().find(|(from, _)| *from == k).map(|(_, to)| *to)
}

/// `createRenamer(map)`: the id namespaced, looked up in a table.
fn table(entries: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    move |id| lookup(entries, &namespaced(id)).map(str::to_owned)
}

/// `createRenamer(from, to)`.
fn only(from: &'static str, to: &'static str) -> impl Fn(&str) -> Option<String> {
    move |id| (namespaced(id) == from).then(|| to.to_owned())
}

fn rename_names(cx: &mut Cx, ty: Ty, f: &dyn Fn(&str) -> Option<String>) {
    cx.each(ty, |v| {
        if let Some(name) = as_str(Some(v))
            && let Some(new) = f(&name)
        {
            *v = string(&new);
        }
    });
}

/// `ItemRenameFix`.
fn item_rename(cx: &mut Cx, f: &dyn Fn(&str) -> Option<String>) {
    rename_names(cx, Ty::ItemName, f);
}

/// `LegacyBlockRenameFix`: block names, block states and flat block states.
fn legacy_block_rename(cx: &mut Cx, f: &dyn Fn(&str) -> Option<String>) {
    rename_names(cx, Ty::BlockName, f);
    cx.each(Ty::BlockState, |s| {
        if let Some(name) = as_str(s.get("Name"))
            && let Some(new) = f(&name)
        {
            set(s, "Name", string(&new));
        }
    });
    cx.each(Ty::FlatBlockState, |s| {
        let Some(text) = as_str(Some(s)) else {
            return;
        };
        let mut end = text.len();
        if let Some(p) = text.find('[').filter(|p| *p > 0) {
            end = p;
        }
        if let Some(n) = text.find('{').filter(|n| *n > 0) {
            end = end.min(n);
        }
        if let Some(new) = f(&text[..end]) {
            *s = string(&format!("{new}{}", &text[end..]));
        }
    });
}

/// `SimplestEntityRenameFix`: entity ids and entity names.
fn entity_rename(cx: &mut Cx, f: &dyn Fn(&str) -> Option<String>) {
    cx.each(Ty::Entity, |e| {
        if let Some(id) = as_str(e.get("id"))
            && let Some(new) = f(&namespaced(&id))
        {
            set(e, "id", string(&new));
        }
    });
    rename_names(cx, Ty::EntityName, f);
}

fn the_renamening_entities(name: &str) -> Option<String> {
    let name = namespaced(name);
    let name = match name.strip_prefix("minecraft:bred_") {
        Some(rest) => format!("minecraft:{rest}"),
        None => name,
    };
    Some(lookup(t::tables().the_renamening_entities, &name).map_or(name.clone(), str::to_owned))
}

fn the_renamening_items(name: &str) -> Option<String> {
    let name = namespaced(name);
    if let Some(new) = lookup(t::tables().the_renamening_blocks, &name) {
        return Some(new.to_owned());
    }
    Some(
        match name.as_str() {
            "minecraft:clownfish" => "minecraft:tropical_fish",
            "minecraft:chorus_fruit_popped" => "minecraft:popped_chorus_fruit",
            "minecraft:evocation_illager_spawn_egg" => "minecraft:evoker_spawn_egg",
            "minecraft:vindication_illager_spawn_egg" => "minecraft:vindicator_spawn_egg",
            _ => return None,
        }
        .to_owned(),
    )
}

/// `LegacyComponentDataFixUtils.createTextComponentJson` of a string tag's text.
fn text_json_units(text: &JavaString) -> Tag {
    Tag::String(JavaString(super::json::text_component_json(&text.0)))
}

// ------------------------------------------------------------------ 1451.1

fn chunk_palettes(cx: &mut Cx) {
    let k = cx.k;
    // The chunk is written with the old type and read with the new one: the numeric
    // block names (`DSL.intType()`) come out as ints, whatever tag they were.
    cx.each(Ty::BlockName, |v| {
        if let Some(n) = as_number(v) {
            *v = Tag::Int(n.int_value());
        }
    });
    cx.each(Ty::Chunk, |c| {
        super::chunk_flatten::fix(c);
        reread_as(Ty::Chunk, k, c);
    });
}

// ------------------------------------------------------------------ 1451.2

fn piston_block_state(cx: &mut Cx) {
    cx.block_entity("minecraft:piston", |be| {
        let block = as_int(be.get("blockId"), 0);
        remove(be, "blockId");
        let data = as_int(be.get("blockData"), 0) & 15;
        remove(be, "blockData");
        // The new value is built from this remainder: it remembers it.
        mark_read(be);
        set(be, "blockState", bsd::get_tag(block << 4 | data));
    });
}

// ------------------------------------------------------------------ 1451.3

fn block_id(name: &str) -> i32 {
    t::tables()
        .block_ids
        .iter()
        .find(|(n, _)| *n == name)
        .map_or(0, |(_, id)| *id)
}

/// The block id of a `BLOCK_NAME` value (`Either<Integer, String>`).
fn block_name_id(tile: &Tag) -> Option<i32> {
    match tile {
        Tag::String(s) => Some(block_id(&namespaced(&s.to_string_lossy()))),
        other => as_number(other).map(|n| n.int_value()),
    }
}

/// `EntityBlockStateFix.updateBlockToBlockState`. The typed old field stays in
/// the remainder it was read from.
fn block_to_state(e: &mut Tag, old: &str, data_name: &str, new: &str) {
    let block = e.get(old).and_then(block_name_id);
    let data = as_int(e.get(data_name), 0) & 15;
    if let Some(block) = block {
        set(e, new, bsd::get_tag(block << 4 | data));
    }
    remove(e, data_name);
}

fn entity_block_state(cx: &mut Cx) {
    cx.each(Ty::Entity, |e| {
        let Some(id) = as_str(e.get("id")) else {
            return;
        };
        match namespaced(&id).as_str() {
            "minecraft:falling_block" => {
                let block = match e.get("Block") {
                    Some(tile) => block_name_id(tile).unwrap_or(0),
                    None => match e.get("TileID").and_then(as_number) {
                        Some(n) => n.int_value(),
                        None => as_int(e.get("Tile"), 0) as i8 as u8 as i32,
                    },
                };
                let data = as_int(e.get("Data"), 0) & 15;
                set(e, "BlockState", bsd::get_tag(block << 4 | data));
                for f in ["Data", "TileID", "Tile"] {
                    remove(e, f);
                }
            }
            "minecraft:enderman" => {
                block_to_state(e, "carried", "carriedData", "carriedBlockState")
            }
            "minecraft:arrow" | "minecraft:spectral_arrow" => {
                block_to_state(e, "inTile", "inData", "inBlockState")
            }
            "minecraft:commandblock_minecart"
            | "minecraft:minecart"
            | "minecraft:chest_minecart"
            | "minecraft:furnace_minecart"
            | "minecraft:tnt_minecart"
            | "minecraft:hopper_minecart"
            | "minecraft:spawner_minecart" => {
                block_to_state(e, "DisplayTile", "DisplayData", "DisplayState")
            }
            _ => {}
        }
    });
}

fn map_id(cx: &mut Cx) {
    cx.item(
        |id| id == "minecraft:filled_map",
        |s| {
            let damage = as_int(s.get("Damage"), 0);
            if !matches!(s.get("tag"), Some(Tag::Compound(_))) {
                set(s, "tag", empty_map());
            }
            if let Some(tag) = s.get_mut("tag") {
                set(tag, "map", Tag::Int(damage));
            }
        },
    );
}

// ------------------------------------------------------------------ 1451.4

fn block_name_flattening(cx: &mut Cx) {
    cx.each(Ty::BlockName, |v| {
        let name = match &*v {
            Tag::String(s) => bsd::upgrade_block_name(&namespaced(&s.to_string_lossy())),
            other => match as_number(other) {
                Some(n) => bsd::upgrade_block_id(n.int_value()),
                None => return,
            },
        };
        *v = string(&name);
    });
}

/// `ItemStackTheFlatteningFix.updateItem`.
fn update_item(name: &str, data: i32) -> Option<&'static str> {
    static MAP: OnceLock<(HashMap<&'static str, &'static str>, Vec<&'static str>)> =
        OnceLock::new();
    let (map, ids) = MAP.get_or_init(|| {
        let map: HashMap<_, _> = t::tables().item_flattening.iter().copied().collect();
        let mut ids: Vec<&str> = t::tables()
            .item_flattening
            .iter()
            .map(|(k, _)| &k[..k.find('.').expect("dot")])
            .collect();
        ids.sort_unstable();
        ids.dedup();
        (map, ids)
    });
    if ids.binary_search(&name).is_err() {
        return None;
    }
    map.get(format!("{name}.{data}").as_str())
        .or_else(|| map.get(format!("{name}.0").as_str()))
        .copied()
}

fn item_flattening(cx: &mut Cx) {
    cx.each(Ty::ItemStack, |s| {
        let Some(id) = as_str(s.get("id")) else {
            return;
        };
        let id = namespaced(&id);
        let data = as_int(s.get("Damage"), 0);
        if let Some(new) = update_item(&id, data) {
            set(s, "id", string(new));
        }
        if t::tables().item_damage_ids.contains(&id.as_str()) {
            if !matches!(s.get("tag"), Some(Tag::Compound(_))) {
                set(s, "tag", empty_map());
            }
            if let Some(tag) = s.get_mut("tag") {
                set(tag, "Damage", Tag::Int(data));
            }
        }
        remove(s, "Damage");
    });
}

// ------------------------------------------------------------------ 1451.5

const DROPPED_BLOCK_ENTITIES: [&str; 2] = ["minecraft:noteblock", "minecraft:flower_pot"];

fn dropped_block_entity(tag: Option<&Tag>) -> bool {
    tag.and_then(|be| as_str(be.get("id")))
        .is_some_and(|id| DROPPED_BLOCK_ENTITIES.contains(&namespaced(&id).as_str()))
}

/// `RemoveBlockEntityTagFix` (legacy data structure).
fn remove_block_entity_tags(cx: &mut Cx) {
    cx.each(Ty::ItemStack, |s| {
        if let Some(tag) = s.get_mut("tag")
            && dropped_block_entity(tag.get("BlockEntityTag"))
        {
            remove(tag, "BlockEntityTag");
        }
    });
    cx.entity("minecraft:falling_block", |e| {
        if dropped_block_entity(e.get("TileEntityData")) {
            remove(e, "TileEntityData");
        }
    });
}

fn spawn_eggs(cx: &mut Cx) {
    let input = cx.k - 1;
    cx.item(
        |id| id == "minecraft:spawn_egg",
        |s| {
            // `getOrCreateTyped`: an EntityTag that is missing, or does not read as an entity
            // of the schema, is a default entity of it (the first of its table: a wither).
            let entity_id = s
                .get("tag")
                .and_then(|t| t.get("EntityTag"))
                .filter(|e| super::schema::flat::entity_tree_known(e, input))
                .and_then(|e| as_str(e.get("id")))
                .map(|id| namespaced(&id))
                .unwrap_or_else(|| "minecraft:wither".to_owned());
            let egg =
                lookup(t::tables().spawn_eggs, &entity_id).unwrap_or("minecraft:pig_spawn_egg");
            set(s, "id", string(egg));
        },
    );
}

fn banner_color(cx: &mut Cx) {
    cx.block_entity("minecraft:banner", |be| {
        if let Some(base) = be.get("Base") {
            let color = 15 - as_int(Some(base), 0);
            set(be, "Base", Tag::Int(color));
        }
        if is_list(be.get("Patterns")) {
            let patterns: Vec<Tag> = stream(be.get("Patterns"))
                .into_iter()
                .map(|mut p| {
                    if let Some(c) = p.get("Color") {
                        let color = 15 - as_int(Some(c), 0);
                        set(&mut p, "Color", Tag::Int(color));
                    }
                    p
                })
                .collect();
            set(be, "Patterns", Tag::List(patterns));
        }
    });
}

/// Guava `Splitter.on(sep).limit(n)` over a string (empty parts kept).
fn split_limit(s: &str, sep: char, limit: usize) -> Vec<&str> {
    s.splitn(limit, sep).collect()
}

/// `NumberUtils.toInt(s, default)`.
fn to_int(s: &str, default: i32) -> i32 {
    s.parse::<i32>().unwrap_or(default)
}

const DEFAULT_FLAT: &str = "minecraft:bedrock,2*minecraft:dirt,minecraft:grass_block;1;village";

/// `LevelFlatGeneratorInfoFix.fixString`.
fn flat_generator_string(options: &str) -> String {
    if options.is_empty() {
        return DEFAULT_FLAT.to_owned();
    }
    let mut parts = split_limit(options, ';', 5).into_iter();
    let first = parts.next().unwrap_or("");
    let rest: Vec<&str> = parts.collect();
    let (version, layer_info, rest) = match rest.split_first() {
        Some((layers, rest)) => (to_int(first, 0), *layers, rest.to_vec()),
        None => (0, first, Vec::new()),
    };
    if !(0..=3).contains(&version) {
        return DEFAULT_FLAT.to_owned();
    }
    let layers: Vec<String> = layer_info
        .split(',')
        .map(|layer| {
            let amount: Vec<&str> = if version < 3 {
                split_limit(layer, 'x', 2)
            } else {
                split_limit(layer, '*', 2)
            };
            let (height, layer_type) = if amount.len() == 2 {
                (to_int(amount[0], 0), amount[1])
            } else {
                (1, amount[0])
            };
            let block_parts = split_limit(layer_type, ':', 3);
            let name_index = usize::from(block_parts[0] == "minecraft");
            let block_string = block_parts.get(name_index).copied().unwrap_or("");
            let block = if version == 3 {
                block_id(&format!("minecraft:{block_string}"))
            } else {
                to_int(block_string, 0)
            };
            let data_index = name_index + 1;
            let data = block_parts.get(data_index).map_or(0, |d| to_int(d, 0));
            let tag = bsd::get_tag(block << 4 | data);
            let name = as_str(tag.get("Name")).unwrap_or_default();
            format!(
                "{}{name}",
                if height == 1 {
                    String::new()
                } else {
                    format!("{height}*")
                }
            )
        })
        .collect();
    let mut result = layers.join(",");
    for part in rest {
        result.push(';');
        result.push_str(part);
    }
    result
}

fn flat_generator_info(cx: &mut Cx) {
    cx.each(Ty::Level, |l| {
        if !as_str(l.get("generatorName")).is_some_and(|n| n.eq_ignore_ascii_case("flat")) {
            return;
        }
        if let Some(options) = as_str(l.get("generatorOptions")) {
            set(
                l,
                "generatorOptions",
                string(&flat_generator_string(&options)),
            );
        }
    });
}

// ------------------------------------------------------------------ 1451.6

/// `V1451_6.packNamespacedWithDot`.
fn pack_namespaced_with_dot(location: &str) -> String {
    match super::identifier(location) {
        Some((ns, path)) => format!("{ns}.{path}"),
        None => location.to_owned(),
    }
}

/// `StatsCounterFix.unpackLegacyKey`: the stat type and its key.
fn unpack_legacy_key(k: &str) -> Option<(String, String)> {
    if t::tables().stats_skip.contains(&k) {
        return None;
    }
    if let Some(custom) = lookup(t::tables().stats_custom, k) {
        return Some(("minecraft:custom".to_owned(), custom.to_owned()));
    }
    // StringUtils.ordinalIndexOf(key, ".", 2)
    let mut dots = k.match_indices('.');
    let split = dots.nth(1)?.0;
    let prefix = &k[..split];
    let rest = k[split + 1..].replace('.', ":");
    if prefix == "stat.mineBlock" {
        return Some(("minecraft:mined".to_owned(), bsd::upgrade_block_name(&rest)));
    }
    if let Some(item_key) = lookup(t::tables().stats_item_keys, prefix) {
        let new = update_item(&rest, 0).map_or(rest.clone(), str::to_owned);
        return Some((item_key.to_owned(), new));
    }
    if let Some(entity_key) = lookup(t::tables().stats_entity_keys, prefix) {
        let new = lookup(t::tables().stats_entities, &rest).map_or(rest.clone(), str::to_owned);
        return Some((entity_key.to_owned(), new));
    }
    None
}

fn stats_counters(cx: &mut Cx) {
    cx.each(Ty::Stats, |stats| {
        let mut new: Vec<(String, Compound)> = Vec::new();
        if let Tag::Compound(c) = &*stats {
            for (k, v) in c {
                if as_number(v).is_none() {
                    continue;
                }
                let Some((ty, ty_key)) = unpack_legacy_key(&k.to_string_lossy()) else {
                    continue;
                };
                let slot = match new.iter().position(|(t, _)| *t == ty) {
                    Some(i) => i,
                    None => {
                        new.push((ty, Compound::new()));
                        new.len() - 1
                    }
                };
                new[slot].1.insert(super::dynamic::key(&ty_key), v.clone());
            }
        }
        let map: Compound = new
            .into_iter()
            .map(|(ty, entries)| (super::dynamic::key(&ty), Tag::Compound(entries)))
            .collect();
        *stats = Tag::compound([("stats", Tag::Compound(map))]);
    });
    cx.each(Ty::Objective, |o| {
        // The output schema reads the objective again: its read hook unpacks the name.
        remove(o, "CriteriaType");
        if let Some(name) = as_str(o.get("CriteriaName")) {
            let new = if t::tables()
                .special_objective_criteria
                .contains(&name.as_str())
            {
                name
            } else {
                match unpack_legacy_key(&name) {
                    None => "dummy".to_owned(),
                    Some((ty, ty_key)) => format!(
                        "{}:{}",
                        pack_namespaced_with_dot(&ty),
                        pack_namespaced_with_dot(&ty_key)
                    ),
                }
            };
            set(o, "CriteriaName", string(&new));
        }
        super::fixes_a::unpack_objective(o);
    });
}

fn jukebox_record(cx: &mut Cx) {
    cx.block_entity("minecraft:jukebox", |be| {
        let record = as_int(be.get("Record"), 0);
        if record <= 0 {
            return;
        }
        let name = t::tables()
            .item_ids
            .iter()
            .find(|(id, _)| *id == record)
            .map_or("minecraft:air", |(_, n)| *n);
        if let Some(id) = update_item(name, 0) {
            // `tag.remove("Record")` is not kept: Dynamics are immutable.
            let mut stack = Tag::compound([("id", string(id)), ("Count", Tag::Byte(1))]);
            items::add_names(&mut stack);
            set(be, "RecordItem", stack);
        }
    });
}

// ------------------------------------------------------------------ 1451.7

fn villager_trades(cx: &mut Cx) {
    cx.each(Ty::VillagerTrade, |trade| {
        for f in ["buy", "buyB", "sell"] {
            if let Some(stack) = trade.get_mut(f)
                && as_str(stack.get("id")).is_some_and(|id| id == "minecraft:carved_pumpkin")
            {
                set(stack, "id", string("minecraft:pumpkin"));
            }
        }
    });
}

// ------------------------------------------------------------------ 1458

fn entity_custom_name(cx: &mut Cx) {
    let k = cx.k;
    cx.each(Ty::Entity, |e| {
        let Some(Tag::String(name)) = e.get("CustomName").cloned() else {
            return;
        };
        if name.0.is_empty() {
            remove(e, "CustomName");
            reread(k, e);
            return;
        }
        let id = as_str(e.get("id")).unwrap_or_default();
        let component = if id == "minecraft:commandblock_minecart" {
            Tag::String(name)
        } else {
            text_json_units(&name)
        };
        set(e, "CustomName", component);
    });
}

fn item_custom_name(cx: &mut Cx) {
    cx.each(Ty::ItemStack, |s| {
        if let Some(display) = s.get_mut("tag").and_then(|t| t.get_mut("display"))
            && let Some(Tag::String(name)) = display.get("Name").cloned()
        {
            set(display, "Name", text_json_units(&name));
        }
    });
}

const NAMEABLE_BLOCK_ENTITIES: [&str; 11] = [
    "minecraft:beacon",
    "minecraft:banner",
    "minecraft:brewing_stand",
    "minecraft:chest",
    "minecraft:trapped_chest",
    "minecraft:dispenser",
    "minecraft:dropper",
    "minecraft:enchanting_table",
    "minecraft:furnace",
    "minecraft:hopper",
    "minecraft:shulker_box",
];

/// `BlockEntityCustomNameToComponentFix.fixTagCustomName`.
fn fix_tag_custom_name(tag: &mut Tag) {
    let name = match tag.get("CustomName") {
        Some(Tag::String(s)) => s.clone(),
        _ => JavaString(Vec::new()),
    };
    if name.0.is_empty() {
        remove(tag, "CustomName");
    } else {
        set(tag, "CustomName", text_json_units(&name));
    }
}

fn block_entity_custom_name(cx: &mut Cx) {
    let k = cx.k;
    cx.each(Ty::BlockEntity, |be| {
        if let Some(id) = as_str(be.get("id"))
            && !NAMEABLE_BLOCK_ENTITIES.contains(&namespaced(&id).as_str())
        {
            return;
        }
        fix_tag_custom_name(be);
        reread_as(Ty::BlockEntity, k, be);
    });
}

// ------------------------------------------------------------------ 1460

fn painting_motive(cx: &mut Cx) {
    cx.entity("minecraft:painting", |e| {
        if let Some(motive) = as_str(e.get("Motive")) {
            let lower = motive.to_lowercase();
            let mapped = match lower.as_str() {
                "donkeykong" => "donkey_kong",
                "burningskull" => "burning_skull",
                "skullandroses" => "skull_and_roses",
                other => other,
            };
            set(e, "Motive", string(&namespaced(mapped)));
        }
    });
}

// ------------------------------------------------------------------ 1466

fn chunk_to_protochunk(cx: &mut Cx) {
    let k = cx.k;
    cx.each(Ty::Chunk, |chunk| {
        if let Some(level) = chunk.get_mut("Level") {
            fix_protochunk_level(level);
        }
        reread_as(Ty::Chunk, k, chunk);
    });
}

fn fix_protochunk_level(tag: &mut Tag) {
    let terrain_populated = super::dynamic::as_bool(tag.get("TerrainPopulated"), false);
    let light_populated = match tag.get("LightPopulated").and_then(as_number) {
        None => true,
        Some(_) => super::dynamic::as_bool(tag.get("LightPopulated"), false),
    };
    let status = if terrain_populated {
        if light_populated {
            "mobs_spawned"
        } else {
            "decorated"
        }
    } else {
        "carved"
    };
    // repackBiomes
    if let Some(buffer) = super::dynamic::as_byte_buffer(tag.get("Biomes")) {
        let biomes: Vec<i32> = (0..256)
            .map(|i| buffer.get(i).map_or(0, |b| i32::from(*b)))
            .collect();
        set(tag, "Biomes", Tag::IntArray(biomes));
    }
    // repackTicks
    if is_list(tag.get("TileTicks")) {
        let mut to_be_ticked: Vec<Vec<Tag>> = vec![Vec::new(); 16];
        for tick in stream(tag.get("TileTicks")) {
            let x = as_int(tick.get("x"), 0);
            let y = as_int(tick.get("y"), 0);
            let z = as_int(tick.get("z"), 0);
            let packed = (x & 15 | (y & 15) << 4 | (z & 15) << 8) as i16;
            if let Some(list) = usize::try_from(y >> 4)
                .ok()
                .and_then(|i| to_be_ticked.get_mut(i))
            {
                list.push(Tag::Short(packed));
            }
        }
        remove(tag, "TileTicks");
        set(
            tag,
            "ToBeTicked",
            Tag::List(to_be_ticked.into_iter().map(Tag::List).collect()),
        );
    }
    set(tag, "Status", string(status));
    set(tag, "hasLegacyStructureData", Tag::Byte(1));
}

// ------------------------------------------------------------------ 1484

fn heightmap_renaming(cx: &mut Cx) {
    cx.each(Ty::Chunk, |chunk| {
        let Some(level) = chunk.get_mut("Level") else {
            return;
        };
        let Some(heightmaps) = level.get_mut("Heightmaps") else {
            return;
        };
        let mut moves: Vec<(&str, &[&str])> = vec![
            ("LIQUID", &["WORLD_SURFACE_WG"]),
            ("SOLID", &["OCEAN_FLOOR_WG", "OCEAN_FLOOR"]),
            ("LIGHT", &["LIGHT_BLOCKING"]),
            ("RAIN", &["MOTION_BLOCKING", "MOTION_BLOCKING_NO_LEAVES"]),
        ];
        for (from, to) in moves.drain(..) {
            if let Some(value) = remove(heightmaps, from) {
                for name in to {
                    set(heightmaps, name, value.clone());
                }
            }
        }
    });
}

// ------------------------------------------------------------------ 1488

fn command_block_custom_name(cx: &mut Cx) {
    let k = cx.k;
    cx.block_entity("minecraft:command_block", |be| {
        fix_tag_custom_name(be);
        reread_as(Ty::BlockEntity, k, be);
    });
}

fn command_block_minecart_custom_name(cx: &mut Cx) {
    cx.entity("minecraft:commandblock_minecart", |e| {
        if let Some(Tag::String(name)) = e.get("CustomName").cloned() {
            set(e, "CustomName", text_json_units(&name));
        }
    });
}

fn is_igloo_piece(tag: &Tag) -> bool {
    as_str(tag.get("id")).unwrap_or_default() == "Iglu"
}

fn igloo_metadata_removal(cx: &mut Cx) {
    cx.each(Ty::StructureFeature, |feature| {
        let only_igloo = is_list(feature.get("Children"))
            && stream(feature.get("Children")).iter().all(is_igloo_piece);
        if only_igloo {
            set(feature, "id", string("Igloo"));
            remove(feature, "Children");
        } else if is_list(feature.get("Children")) {
            let kept: Vec<Tag> = stream(feature.get("Children"))
                .into_iter()
                .filter(|c| !is_igloo_piece(c))
                .collect();
            set(feature, "Children", Tag::List(kept));
        }
    });
}

// ------------------------------------------------------------------ 1492

fn structure_template_rename(cx: &mut Cx) {
    cx.each(Ty::StructureFeature, |structure| {
        if structure.get("Children").is_none() {
            return;
        }
        let id = as_str(structure.get("id")).unwrap_or_default();
        let rename = t::tables()
            .structure_template_renames
            .iter()
            .find(|(s, _, _)| *s == id);
        let children: Vec<Tag> = stream(structure.get("Children"))
            .into_iter()
            .map(|mut child| {
                if let Some((_, child_id, templates)) = rename
                    && as_str(child.get("id")).unwrap_or_default() == *child_id
                {
                    let template = as_str(child.get("Template")).unwrap_or_default();
                    let new = templates
                        .iter()
                        .find(|(from, _)| *from == template)
                        .map_or(template.as_str(), |(_, to)| *to);
                    set(&mut child, "Template", string(new));
                }
                child
            })
            .collect();
        set(structure, "Children", Tag::List(children));
    });
}

// ------------------------------------------------------------------ 1494

fn enchantment_names(cx: &mut Cx) {
    let name = |e: &Tag| {
        let id = as_int(e.get("id"), 0);
        t::tables()
            .enchantments
            .iter()
            .find(|(i, _)| *i == id)
            .map_or("null", |(_, n)| *n)
    };
    cx.each(Ty::ItemStack, |s| {
        let Some(tag) = s.get_mut("tag") else {
            return;
        };
        if is_list(tag.get("ench")) {
            let list: Vec<Tag> = stream(tag.get("ench"))
                .into_iter()
                .map(|mut e| {
                    let n = name(&e);
                    set(&mut e, "id", string(n));
                    e
                })
                .collect();
            remove(tag, "ench");
            set(tag, "Enchantments", Tag::List(list));
        }
        if is_list(tag.get("StoredEnchantments")) {
            let list: Vec<Tag> = stream(tag.get("StoredEnchantments"))
                .into_iter()
                .map(|mut e| {
                    let n = name(&e);
                    set(&mut e, "id", string(n));
                    e
                })
                .collect();
            set(tag, "StoredEnchantments", Tag::List(list));
        }
    });
}

// ------------------------------------------------------------------ 1501

fn advancements_rename(cx: &mut Cx) {
    cx.each(Ty::Advancements, |adv| {
        let Tag::Compound(c) = adv else {
            return;
        };
        let entries: Vec<_> = std::mem::take(c).into_iter().collect();
        for (k, v) in entries {
            let name = k.to_string_lossy();
            let renamed = lookup(t::tables().advancements, &name).map_or(k, super::dynamic::key);
            c.insert(renamed, v);
        }
    });
}

// ------------------------------------------------------------------ 1506

/// Java `String.split(regex)` with a literal separator and a limit of 0 (trailing
/// empty strings removed) or a positive limit.
fn java_split<'a>(s: &'a str, sep: &str, limit: usize) -> Vec<&'a str> {
    let mut parts: Vec<&str> = if limit > 0 {
        s.splitn(limit, sep).collect()
    } else {
        s.split(sep).collect()
    };
    if limit == 0 {
        while parts.len() > 1 && parts.last().is_some_and(|p| p.is_empty()) {
            parts.pop();
        }
        if parts.len() == 1 && parts[0].is_empty() && !s.is_empty() {
            parts.clear();
        }
    }
    parts
}

fn layers_info(input: &str) -> Vec<(i32, String)> {
    let mut result = Vec::new();
    for depth in java_split(input, ",", 0) {
        let parts = java_split(depth, "*", 2);
        let height = if parts.len() == 2 {
            match parts[0].parse::<i32>() {
                Ok(h) => h,
                Err(_) => return Vec::new(),
            }
        } else {
            1
        };
        result.push((height, parts[parts.len() - 1].to_owned()));
    }
    result
}

/// `LevelDataGeneratorOptionsFix.convert`.
fn flat_options(flat: &str) -> Tag {
    let mut parts = flat.split(';');
    let mut biome = "minecraft:plains".to_owned();
    let mut structures: Vec<(String, Vec<(String, String)>)> = Vec::new();
    let layers: Vec<(i32, String)>;
    let first = parts.next();
    if !flat.is_empty() && first.is_some() {
        layers = layers_info(first.unwrap_or(""));
        if !layers.is_empty() {
            if let Some(b) = parts.next() {
                biome = lookup(t::tables().generator_biomes, b)
                    .unwrap_or("minecraft:plains")
                    .to_owned();
            }
            if let Some(list) = parts.next() {
                for structure in java_split(&list.to_lowercase(), ",", 0) {
                    let separated = java_split(structure, "(", 2);
                    if separated.is_empty() || separated[0].is_empty() {
                        continue;
                    }
                    let mut options = Vec::new();
                    if separated.len() > 1 && separated[1].ends_with(')') && separated[1].len() > 1
                    {
                        let inner = &separated[1][..separated[1].len() - 1];
                        for part in java_split(inner, " ", 0) {
                            let split = java_split(part, "=", 2);
                            if split.len() == 2 {
                                options.push((split[0].to_owned(), split[1].to_owned()));
                            }
                        }
                    }
                    structures.retain(|(n, _)| n != separated[0]);
                    structures.push((separated[0].to_owned(), options));
                }
            } else {
                structures.push(("village".to_owned(), Vec::new()));
            }
        }
    } else {
        layers = vec![
            (1, "minecraft:bedrock".to_owned()),
            (2, "minecraft:dirt".to_owned()),
            (1, "minecraft:grass_block".to_owned()),
        ];
        structures.push(("village".to_owned(), Vec::new()));
    }
    let layers = Tag::List(
        layers
            .into_iter()
            .map(|(h, b)| Tag::compound([("height", Tag::Int(h)), ("block", string(&b))]))
            .collect(),
    );
    let structures: Compound = structures
        .into_iter()
        .map(|(name, options)| {
            let options: Compound = options
                .into_iter()
                .map(|(k, v)| (super::dynamic::key(&k), string(&v)))
                .collect();
            (
                super::dynamic::key(&name.to_lowercase()),
                Tag::Compound(options),
            )
        })
        .collect();
    Tag::compound([
        ("layers", layers),
        ("biome", string(&biome)),
        ("structures", Tag::Compound(structures)),
    ])
}

fn level_generator_options(cx: &mut Cx) {
    let k = cx.k;
    cx.each(Ty::Level, |level| {
        let options = as_str(level.get("generatorOptions"));
        let name = as_str(level.get("generatorName")).unwrap_or_default();
        if name.eq_ignore_ascii_case("flat") {
            let converted = flat_options(options.as_deref().unwrap_or(""));
            set(level, "generatorOptions", converted);
        } else if name.eq_ignore_ascii_case("buffet")
            && let Some(options) = options
        {
            let units: Vec<u16> = options.encode_utf16().collect();
            if let Some(json) = super::json::parse_lenient(&units)
                && let Some(tag) = super::json::to_nbt(&json)
            {
                set(level, "generatorOptions", tag);
            }
        }
        let _ = k;
    });
}

// ------------------------------------------------------------------ 1510, 1514

fn swim_stats_rename(cx: &mut Cx) {
    const RENAMES: &[(&str, &str)] = &[
        ("minecraft:swim_one_cm", "minecraft:walk_on_water_one_cm"),
        ("minecraft:dive_one_cm", "minecraft:walk_under_water_one_cm"),
    ];
    cx.each(Ty::Stats, |stats| {
        if let Some(custom) = stats
            .get_mut("stats")
            .and_then(|s| s.get_mut("minecraft:custom"))
            && let Tag::Compound(c) = custom
        {
            let entries: Vec<_> = std::mem::take(c).into_iter().collect();
            for (k, v) in entries {
                let renamed = lookup(RENAMES, &k.to_string_lossy()).map_or(k, super::dynamic::key);
                c.insert(renamed, v);
            }
        }
    });
    cx.each(Ty::Objective, |o| {
        if let Some(ct) = o.get_mut("CriteriaType")
            && as_str(ct.get("type")).as_deref() == Some("minecraft:custom")
            && let Some(id) = as_str(ct.get("id"))
            && let Some(new) = lookup(RENAMES, &id)
        {
            set(ct, "id", string(new));
        }
    });
}

fn display_name_to_component(cx: &mut Cx, ty: Ty) {
    cx.each(ty, |v| {
        if let Some(Tag::String(name)) = v.get("DisplayName").cloned() {
            set(v, "DisplayName", text_json_units(&name));
        }
    });
}

fn objective_render_type(cx: &mut Cx) {
    cx.each(Ty::Objective, |o| {
        if as_str(o.get("RenderType")).is_none() {
            let criteria = as_str(o.get("CriteriaName")).unwrap_or_default();
            let render = if criteria == "health" {
                "hearts"
            } else {
                "integer"
            };
            set(o, "RenderType", string(render));
        }
    });
}
