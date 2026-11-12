//! Fixes of 1.15 and 1.16 (keys 2100 to 2571): what worlds saved by 1.14.4 to
//! 1.16.5 need before the fixes of [`super::fixes_cliffs`].
use super::dynamic::{
    as_bool, as_double, as_int, as_long, as_long_stream, as_number, as_str, ceillog2, namespaced,
    remove, set, stream, string,
};
use super::fixes_a::attributes_rename_legacy;
use super::fixes_b::advancements_rename;
use super::schema::{Key, Ty, key};
use super::{Cx, FixFn, id_is, renamer};
use crate::nbt::{Compound, JavaString, Tag};

pub(super) fn register(out: &mut Vec<(Key, FixFn)>) {
    out.push((key(1800, 0), |cx| {
        cx.rename(
            Ty::ItemName,
            renamer(&[
                ("minecraft:cactus_green", "minecraft:green_dye"),
                ("minecraft:rose_red", "minecraft:red_dye"),
                ("minecraft:dandelion_yellow", "minecraft:yellow_dye"),
            ]),
        )
    }));
    out.push((key(1802, 0), |cx| {
        super::fixes_a::block_rename(
            cx,
            &[
                ("minecraft:stone_slab", "minecraft:smooth_stone_slab"),
                ("minecraft:sign", "minecraft:oak_sign"),
                ("minecraft:wall_sign", "minecraft:oak_wall_sign"),
            ],
        )
    }));
    out.push((key(1802, 0), |cx| {
        cx.rename(
            Ty::ItemName,
            renamer(&[
                ("minecraft:stone_slab", "minecraft:smooth_stone_slab"),
                ("minecraft:sign", "minecraft:oak_sign"),
            ]),
        )
    }));
    out.push((key(1803, 0), item_lore));
    out.push((key(1904, 0), cat_split));
    out.push((key(1905, 0), |cx| {
        level_status(cx, |status| {
            (status == "postprocessed").then_some("fullchunk")
        })
    }));
    out.push((key(1911, 0), |cx| {
        level_status(cx, |status| {
            Some(match status {
                "structure_references" | "biomes" => "empty",
                "base" => "surface",
                "carved" => "carvers",
                "liquid_carved" => "liquid_carvers",
                "decorated" => "features",
                "lighted" => "light",
                "mobs_spawned" => "spawn",
                "finalized" => "heightmaps",
                "fullchunk" => "full",
                _ => "empty",
            })
        })
    }));
    out.push((key(1914, 0), |cx| {
        cx.block_entity("minecraft:chest", |be| {
            if as_str(be.get("LootTable")).as_deref() == Some("minecraft:chests/village_blacksmith")
            {
                set(
                    be,
                    "LootTable",
                    string("minecraft:chests/village/village_weaponsmith"),
                );
            }
        })
    }));
    out.push((key(1917, 0), |cx| {
        cx.entity("minecraft:cat", |e| {
            if as_int(e.get("CatType"), 0) == 9 {
                set(e, "CatType", Tag::Int(10));
            }
        })
    }));
    out.push((key(1918, 0), |cx| villager_data(cx, "minecraft:villager")));
    out.push((key(1918, 0), |cx| {
        villager_data(cx, "minecraft:zombie_villager")
    }));
    out.push((key(1920, 0), new_village));
    out.push((key(1925, 0), |cx| {
        cx.each(Ty::SavedMapIndex, |root| {
            let old = std::mem::replace(root, Tag::End);
            *root = Tag::compound([("data", old)]);
        })
    }));
    out.push((key(1928, 0), |cx| {
        entity_rename(cx, "minecraft:illager_beast", "minecraft:ravager")
    }));
    out.push((key(1928, 0), |cx| {
        cx.rename(
            Ty::ItemName,
            renamer(&[(
                "minecraft:illager_beast_spawn_egg",
                "minecraft:ravager_spawn_egg",
            )]),
        )
    }));
    out.push((key(1946, 0), reorganize_poi));
    out.push((key(1948, 0), ominous_banner_item));
    out.push((key(1953, 0), ominous_banner_block_entity));
    out.push((key(1955, 0), villager_level_and_xp));
    out.push((key(1955, 0), zombie_villager_xp));
    out.push((key(1961, 0), |cx| {
        cx.each(Ty::Chunk, |c| {
            if let Some(level) = c.get_mut("Level") {
                remove(level, "isLightOn");
            }
        })
    }));
    out.push((key(1963, 0), |cx| {
        cx.entity("minecraft:villager", |e| {
            map_elements_filter(e, "Gossips", |g| {
                as_str(g.get("Type")).as_deref() != Some("golem")
            });
        })
    }));
    out.push((key(2100, 0), |cx| {
        cx.rename(
            Ty::Recipe,
            renamer(&[("minecraft:sugar", "minecraft:sugar_from_sugar_cane")]),
        )
    }));
    out.push((key(2100, 0), |cx| {
        advancements_rename_table(
            cx,
            &[(
                "minecraft:recipes/misc/sugar",
                "minecraft:recipes/misc/sugar_from_sugar_cane",
            )],
        )
    }));
    out.push((key(2202, 0), chunk_biomes));
    out.push((key(2209, 0), |cx| {
        let t: &[(&str, &'static str)] = &[("minecraft:bee_hive", "minecraft:beehive")];
        cx.rename(Ty::ItemName, renamer(t));
        poi_type_rename(cx, t);
    }));
    out.push((key(2209, 0), |cx| {
        super::fixes_a::block_rename(cx, &[("minecraft:bee_hive", "minecraft:beehive")])
    }));
    out.push((key(2211, 0), structure_reference_count));
    out.push((key(2218, 0), force_poi_rebuild));
    out.push((key(2501, 0), furnace_recipes));
    out.push((key(2503, 0), wall_property));
    out.push((key(2503, 0), |cx| {
        advancements_rename_table(
            cx,
            &[(
                "minecraft:recipes/misc/composter",
                "minecraft:recipes/decorations/composter",
            )],
        )
    }));
    out.push((key(2505, 0), |cx| memory_expiry(cx, "minecraft:villager")));
    out.push((key(2508, 0), |cx| cx.rename(Ty::ItemName, renamer(FUNGUS))));
    out.push((key(2508, 0), |cx| super::fixes_a::block_rename(cx, FUNGUS)));
    out.push((key(2509, 0), |cx| {
        entity_rename(cx, "minecraft:zombie_pigman", "minecraft:zombified_piglin")
    }));
    out.push((key(2509, 0), |cx| {
        cx.rename(
            Ty::ItemName,
            renamer(&[(
                "minecraft:zombie_pigman_spawn_egg",
                "minecraft:zombified_piglin_spawn_egg",
            )]),
        )
    }));
    out.push((key(2511, 0), projectile_owner));
    // `SplashPotionItemFieldRenameFix` casts the thrown potion to the type whose
    // item stack field is `Item`: the typed `Potion` value takes its place, and
    // writing the entity gives `Potion` back from the map it was read from.
    out.push((key(2511, 1), |cx| {
        cx.entity("minecraft:potion", |e| {
            if let Some(item) = e.get("Potion").cloned() {
                set(e, "Item", item);
            }
            super::drop_typed(e, "Potion");
        })
    }));
    out.push((key(2514, 0), entity_uuids));
    out.push((key(2514, 0), block_entity_uuids));
    out.push((key(2514, 0), player_uuids));
    out.push((key(2514, 0), level_uuids));
    out.push((key(2514, 0), raids_uuids));
    out.push((key(2514, 0), item_stack_uuids));
    out.push((key(2516, 0), |cx| gossip_uuids(cx, "minecraft:villager")));
    out.push((key(2516, 0), |cx| {
        gossip_uuids(cx, "minecraft:zombie_villager")
    }));
    out.push((key(2518, 0), jigsaw_properties));
    out.push((key(2518, 0), jigsaw_rotation));
    out.push((key(2523, 0), |cx| {
        attributes_rename_legacy(cx, &|name| {
            ATTRIBUTES
                .iter()
                .find(|(from, _)| *from == name)
                .map(|(_, to)| *to)
        })
    }));
    out.push((key(2527, 0), bit_storage_align));
    out.push((key(2528, 0), |cx| {
        cx.rename(
            Ty::ItemName,
            renamer(&[
                ("minecraft:soul_fire_torch", "minecraft:soul_torch"),
                ("minecraft:soul_fire_lantern", "minecraft:soul_lantern"),
            ]),
        )
    }));
    out.push((key(2528, 0), |cx| {
        super::fixes_a::block_rename(
            cx,
            &[
                ("minecraft:soul_fire_torch", "minecraft:soul_torch"),
                (
                    "minecraft:soul_fire_wall_torch",
                    "minecraft:soul_wall_torch",
                ),
                ("minecraft:soul_fire_lantern", "minecraft:soul_lantern"),
            ],
        )
    }));
    out.push((key(2529, 0), strider_gravity));
    out.push((key(2531, 0), redstone_wire));
    out.push((key(2533, 0), villager_follow_range));
    out.push((key(2535, 0), shulker_rotation));
    out.push((key(2537, 0), legacy_dimension_id));
    out.push((key(2538, 0), level_legacy_world_gen_settings));
    out.push((key(2550, 0), world_gen_settings));
    out.push((key(2552, 0), |cx| {
        cx.rename(
            Ty::Biome,
            renamer(&[("minecraft:nether", "minecraft:nether_wastes")]),
        )
    }));
    out.push((key(2553, 0), |cx| cx.rename(Ty::Biome, renamer(BIOMES))));
}

const FUNGUS: &[(&str, &str)] = &[
    ("minecraft:warped_fungi", "minecraft:warped_fungus"),
    ("minecraft:crimson_fungi", "minecraft:crimson_fungus"),
];

/// `AdvancementsRenameFix` with a `createRenamer` table.
fn advancements_rename_table(cx: &mut Cx, table: &'static [(&str, &str)]) {
    advancements_rename(cx, &|id| {
        let id = namespaced(id);
        table
            .iter()
            .find(|(from, _)| *from == id)
            .map(|(_, to)| (*to).to_owned())
    });
}

/// `SimplestEntityRenameFix`: the id of the entities and the entity names.
fn entity_rename(cx: &mut Cx, from: &str, to: &'static str) {
    cx.each(Ty::Entity, |e| {
        if id_is(e, from) {
            set(e, "id", string(to));
        }
    });
    cx.rename(Ty::EntityName, |name| {
        (namespaced(name) == from).then_some(to)
    });
}

/// `PoiTypeRenameFix`.
fn poi_type_rename(cx: &mut Cx, table: &[(&str, &'static str)]) {
    let r = renamer(table);
    cx.each(Ty::PoiChunk, |poi| {
        let Some(Tag::Compound(sections)) = poi.get_mut("Sections") else {
            return;
        };
        for section in sections.values_mut() {
            let Some(Tag::List(records)) = section.get_mut("Records") else {
                continue;
            };
            for record in records {
                if let Some(t) = as_str(record.get("type"))
                    && let Some(new) = r(&t)
                {
                    set(record, "type", string(new));
                }
            }
        }
    });
}

/// `ChunkBiomeFix`: the 256 biomes of a column become 1024 (a 4x4x64 grid).
fn chunk_biomes(cx: &mut Cx) {
    cx.each(Ty::Chunk, |c| {
        let Some(level) = c.get_mut("Level") else {
            return;
        };
        let Some(old) = super::dynamic::as_int_stream(level.get("Biomes")) else {
            return;
        };
        if old.len() != 256 {
            return;
        }
        let mut new = vec![0i32; 1024];
        for z in 0..4 {
            for x in 0..4 {
                let (ox, oz) = ((x << 2) + 2, (z << 2) + 2);
                new[z << 2 | x] = old[oz << 4 | ox];
            }
        }
        for y in 1..64 {
            new.copy_within(0..16, y * 16);
        }
        set(level, "Biomes", Tag::IntArray(new));
    });
}

/// `StructureReferenceCountFix`.
fn structure_reference_count(cx: &mut Cx) {
    cx.each(Ty::StructureFeature, |s| {
        if let Some(r) = s.get("references") {
            let n = as_number(r)
                .map(|n| n.int_value())
                .filter(|n| *n > 0)
                .unwrap_or(1);
            set(s, "references", Tag::Int(n));
        }
    });
}

/// `ForcePoiRebuild`: every POI section loses its `Valid` flag.
fn force_poi_rebuild(cx: &mut Cx) {
    cx.each(Ty::PoiChunk, |poi| {
        if let Some(Tag::Compound(sections)) = poi.get_mut("Sections") {
            for section in sections.values_mut() {
                remove(section, "Valid");
            }
        }
    });
}

/// `FurnaceRecipeFix`: `RecipeLocation<i>`/`RecipeAmount<i>` pairs become the
/// `RecipesUsed` map.
fn furnace_recipes(cx: &mut Cx) {
    cx.each(Ty::BlockEntity, |be| {
        if ![
            "minecraft:furnace",
            "minecraft:blast_furnace",
            "minecraft:smoker",
        ]
        .iter()
        .any(|id| id_is(be, id))
        {
            return;
        }
        let size = as_int(be.get("RecipesUsedSize"), 0);
        remove(be, "RecipesUsedSize");
        let mut used = Compound::new();
        for i in 0..size {
            let location = remove(be, &format!("RecipeLocation{i}"));
            let amount = remove(be, &format!("RecipeAmount{i}"));
            let amount = as_int(amount.as_ref(), 0);
            if amount > 0
                && let Some(location) = location.as_ref().and_then(|l| as_str(Some(l)))
            {
                used.insert(
                    JavaString::from(namespaced(&location).as_str()),
                    Tag::Int(amount),
                );
            }
        }
        set(be, "RecipesUsed", Tag::Compound(used));
    });
}

const WALLS: [&str; 14] = [
    "minecraft:andesite_wall",
    "minecraft:brick_wall",
    "minecraft:cobblestone_wall",
    "minecraft:diorite_wall",
    "minecraft:end_stone_brick_wall",
    "minecraft:granite_wall",
    "minecraft:mossy_cobblestone_wall",
    "minecraft:mossy_stone_brick_wall",
    "minecraft:nether_brick_wall",
    "minecraft:prismarine_wall",
    "minecraft:red_nether_brick_wall",
    "minecraft:red_sandstone_wall",
    "minecraft:sandstone_wall",
    "minecraft:stone_brick_wall",
];

/// `WallPropertyFix`: `true`/`false` sides become `low`/`none`.
fn wall_property(cx: &mut Cx) {
    cx.each(Ty::BlockState, |state| {
        let Some(name) = as_str(state.get("Name")) else {
            return;
        };
        if !WALLS.contains(&name.as_str()) {
            return;
        }
        if let Some(props) = state.get_mut("Properties") {
            for side in ["east", "west", "north", "south"] {
                if let Some(v) = props.get(side)
                    && let Some(s) = as_str(Some(v))
                {
                    let new = if s == "true" { "low" } else { "none" };
                    set(props, side, string(new));
                }
            }
        }
    });
}

/// `MemoryExpiryDataFix`: every memory of a brain is wrapped as `{value: ...}`.
fn memory_expiry(cx: &mut Cx, entity: &str) {
    cx.entity(entity, |e| {
        let Some(brain) = e.get_mut("Brain") else {
            return;
        };
        if let Some(Tag::Compound(memories)) = brain.get_mut("memories") {
            for value in memories.values_mut() {
                let inner = std::mem::replace(value, Tag::End);
                *value = Tag::compound([("value", inner)]);
            }
        }
    });
}

/// `EntityProjectileOwnerFix`: the owner of a projectile as a UUID int array.
fn projectile_owner(cx: &mut Cx) {
    let uuid = |most: i64, least: i64| -> Option<Tag> {
        (most != 0 && least != 0).then(|| {
            Tag::IntArray(vec![
                (most >> 32) as i32,
                most as i32,
                (least >> 32) as i32,
                least as i32,
            ])
        })
    };
    cx.each(Ty::Entity, |e| {
        let Some(id) = as_str(e.get("id")).map(|i| namespaced(&i)) else {
            return;
        };
        match id.as_str() {
            "minecraft:egg"
            | "minecraft:ender_pearl"
            | "minecraft:experience_bottle"
            | "minecraft:snowball"
            | "minecraft:potion" => {
                let (m, l) = e
                    .get("owner")
                    .map_or((0, 0), |o| (as_long(o.get("M"), 0), as_long(o.get("L"), 0)));
                if let Some(u) = uuid(m, l) {
                    set(e, "OwnerUUID", u);
                }
                remove(e, "owner");
            }
            "minecraft:llama_spit" => {
                let (m, l) = e.get("Owner").map_or((0, 0), |o| {
                    (
                        as_long(o.get("OwnerUUIDMost"), 0),
                        as_long(o.get("OwnerUUIDLeast"), 0),
                    )
                });
                if let Some(u) = uuid(m, l) {
                    set(e, "OwnerUUID", u);
                }
                remove(e, "Owner");
            }
            "minecraft:arrow" | "minecraft:spectral_arrow" | "minecraft:trident" => {
                let m = as_long(e.get("OwnerUUIDMost"), 0);
                let l = as_long(e.get("OwnerUUIDLeast"), 0);
                if let Some(u) = uuid(m, l) {
                    set(e, "OwnerUUID", u);
                }
                remove(e, "OwnerUUIDMost");
                remove(e, "OwnerUUIDLeast");
            }
            _ => {}
        }
    });
}

// ------------------------------------------------------------------ 2514, 2516

fn uuid_tag(most: i64, least: i64) -> Tag {
    Tag::IntArray(vec![
        (most >> 32) as i32,
        most as i32,
        (least >> 32) as i32,
        least as i32,
    ])
}

/// `AbstractUUIDFix.createUUIDFromLongs`.
fn uuid_from_longs(tag: &Tag, most: &str, least: &str) -> Option<Tag> {
    let m = as_long(tag.get(most), 0);
    let l = as_long(tag.get(least), 0);
    (m != 0 && l != 0).then(|| uuid_tag(m, l))
}

/// `AbstractUUIDFix.replaceUUIDLeastMost`.
fn replace_least_most(tag: &mut Tag, old: &str, new: &str) {
    let (most, least) = (format!("{old}Most"), format!("{old}Least"));
    if let Some(uuid) = uuid_from_longs(tag, &most, &least) {
        remove(tag, &most);
        remove(tag, &least);
        set(tag, new, uuid);
    }
}

/// `AbstractUUIDFix.replaceUUIDMLTag`.
fn replace_ml(tag: &mut Tag, old: &str, new: &str) {
    if let Some(uuid) = tag.get(old).and_then(|t| uuid_from_longs(t, "M", "L")) {
        remove(tag, old);
        set(tag, new, uuid);
    }
}

/// `UUID.fromString`, which takes five hexadecimal groups of any length.
fn parse_uuid(s: &str) -> Option<(i64, i64)> {
    if s.len() > 36 {
        return None;
    }
    let mut parts = [0i64; 5];
    let mut count = 0;
    for part in s.split('-') {
        if count == 5 || part.is_empty() || !part.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        parts[count] = i64::from_str_radix(part, 16).ok()?;
        count += 1;
    }
    (count == 5).then(|| {
        let most = (parts[0] & 0xffff_ffff) << 32 | (parts[1] & 0xffff) << 16 | (parts[2] & 0xffff);
        let least = (parts[3] & 0xffff) << 48 | (parts[4] & 0xffff_ffff_ffff);
        (most, least)
    })
}

/// `AbstractUUIDFix.replaceUUIDString`.
fn replace_uuid_string(tag: &mut Tag, old: &str, new: &str) -> bool {
    let Some((m, l)) = as_str(tag.get(old)).as_deref().and_then(parse_uuid) else {
        return false;
    };
    remove(tag, old);
    set(tag, new, uuid_tag(m, l));
    true
}

/// A field that is present becomes the list of its elements mapped by `f`.
fn map_elements(tag: &mut Tag, field: &str, mut f: impl FnMut(Tag) -> Tag) {
    if let Some(v) = tag.get_mut(field) {
        *v = Tag::List(stream(Some(v)).into_iter().map(&mut f).collect());
    }
}

/// `EntityUUIDFix.updateLivingEntity`.
fn living_entity_uuids(tag: &mut Tag) {
    map_elements(tag, "Attributes", |mut attribute| {
        map_elements(&mut attribute, "Modifiers", |mut modifier| {
            replace_least_most(&mut modifier, "UUID", "UUID");
            modifier
        });
        attribute
    });
}

fn mob_uuids(tag: &mut Tag) {
    living_entity_uuids(tag);
    if let Some(leash) = tag.get_mut("Leash") {
        replace_least_most(leash, "UUID", "UUID");
    }
}

fn animal_uuids(tag: &mut Tag) {
    mob_uuids(tag);
    replace_least_most(tag, "LoveCause", "LoveCause");
}

const ABSTRACT_HORSES: &[&str] = &[
    "minecraft:donkey",
    "minecraft:horse",
    "minecraft:llama",
    "minecraft:mule",
    "minecraft:skeleton_horse",
    "minecraft:trader_llama",
    "minecraft:zombie_horse",
];
const TAMEABLE_ANIMALS: &[&str] = &["minecraft:cat", "minecraft:parrot", "minecraft:wolf"];
const ANIMALS: &[&str] = &[
    "minecraft:bee",
    "minecraft:chicken",
    "minecraft:cow",
    "minecraft:fox",
    "minecraft:mooshroom",
    "minecraft:ocelot",
    "minecraft:panda",
    "minecraft:pig",
    "minecraft:polar_bear",
    "minecraft:rabbit",
    "minecraft:sheep",
    "minecraft:turtle",
    "minecraft:hoglin",
];
const MOBS: &[&str] = &[
    "minecraft:bat",
    "minecraft:blaze",
    "minecraft:cave_spider",
    "minecraft:cod",
    "minecraft:creeper",
    "minecraft:dolphin",
    "minecraft:drowned",
    "minecraft:elder_guardian",
    "minecraft:ender_dragon",
    "minecraft:enderman",
    "minecraft:endermite",
    "minecraft:evoker",
    "minecraft:ghast",
    "minecraft:giant",
    "minecraft:guardian",
    "minecraft:husk",
    "minecraft:illusioner",
    "minecraft:magma_cube",
    "minecraft:pufferfish",
    "minecraft:zombified_piglin",
    "minecraft:salmon",
    "minecraft:shulker",
    "minecraft:silverfish",
    "minecraft:skeleton",
    "minecraft:slime",
    "minecraft:snow_golem",
    "minecraft:spider",
    "minecraft:squid",
    "minecraft:stray",
    "minecraft:tropical_fish",
    "minecraft:vex",
    "minecraft:villager",
    "minecraft:iron_golem",
    "minecraft:vindicator",
    "minecraft:pillager",
    "minecraft:wandering_trader",
    "minecraft:witch",
    "minecraft:wither",
    "minecraft:wither_skeleton",
    "minecraft:zombie",
    "minecraft:zombie_villager",
    "minecraft:phantom",
    "minecraft:ravager",
    "minecraft:piglin",
];
const PROJECTILES: &[&str] = &[
    "minecraft:arrow",
    "minecraft:dragon_fireball",
    "minecraft:firework_rocket",
    "minecraft:fireball",
    "minecraft:llama_spit",
    "minecraft:small_fireball",
    "minecraft:snowball",
    "minecraft:spectral_arrow",
    "minecraft:egg",
    "minecraft:ender_pearl",
    "minecraft:experience_bottle",
    "minecraft:potion",
    "minecraft:trident",
    "minecraft:wither_skull",
];

/// `EntityUUIDFix`.
fn entity_uuids(cx: &mut Cx) {
    cx.each(Ty::Entity, |e| {
        let Some(id) = as_str(e.get("id")).map(|i| namespaced(&i)) else {
            // Without an id no choice matches; the UUID is still fixed.
            replace_least_most(e, "UUID", "UUID");
            return;
        };
        let id = id.as_str();
        replace_least_most(e, "UUID", "UUID");
        if ABSTRACT_HORSES.contains(&id) || TAMEABLE_ANIMALS.contains(&id) {
            animal_uuids(e);
            replace_uuid_string(e, "OwnerUUID", "Owner");
        }
        if ANIMALS.contains(&id) {
            animal_uuids(e);
        }
        if MOBS.contains(&id) {
            mob_uuids(e);
        }
        if id == "minecraft:armor_stand" {
            living_entity_uuids(e);
        }
        if PROJECTILES.contains(&id)
            && let Some(owner) = remove(e, "OwnerUUID")
        {
            set(e, "Owner", owner);
        }
        match id {
            "minecraft:bee" | "minecraft:zombified_piglin" => {
                replace_uuid_string(e, "HurtBy", "HurtBy");
            }
            "minecraft:fox" => {
                if e.get("TrustedUUIDs").is_some() {
                    let trusted: Vec<Tag> = stream(e.get("TrustedUUIDs"))
                        .into_iter()
                        .map(|t| uuid_from_longs(&t, "M", "L").unwrap_or(t))
                        .collect();
                    remove(e, "TrustedUUIDs");
                    set(e, "Trusted", Tag::List(trusted));
                }
            }
            "minecraft:item" => {
                replace_ml(e, "Owner", "Owner");
                replace_ml(e, "Thrower", "Thrower");
            }
            "minecraft:shulker_bullet" => {
                replace_ml(e, "Owner", "Owner");
                replace_ml(e, "Target", "Target");
            }
            "minecraft:area_effect_cloud" | "minecraft:evoker_fangs" => {
                replace_least_most(e, "OwnerUUID", "Owner");
            }
            "minecraft:zombie_villager" => {
                replace_least_most(e, "ConversionPlayer", "ConversionPlayer")
            }
            "minecraft:piglin" => {
                if let Some(angry) = e
                    .get_mut("Brain")
                    .and_then(|b| b.get_mut("memories"))
                    .and_then(|m| m.get_mut("minecraft:angry_at"))
                {
                    replace_uuid_string(angry, "value", "value");
                }
            }
            _ => {}
        }
    });
}

/// `BlockEntityUUIDFix`.
fn block_entity_uuids(cx: &mut Cx) {
    cx.each(Ty::BlockEntity, |be| {
        if id_is(be, "minecraft:conduit") {
            replace_ml(be, "target_uuid", "Target");
        }
        if id_is(be, "minecraft:skull")
            && let Some(mut owner) = remove(be, "Owner")
        {
            replace_uuid_string(&mut owner, "Id", "Id");
            set(be, "SkullOwner", owner);
        }
    });
}

/// `PlayerUUIDFix`.
fn player_uuids(cx: &mut Cx) {
    cx.each(Ty::Player, |p| {
        if let Some(vehicle) = p.get_mut("RootVehicle") {
            replace_least_most(vehicle, "Attach", "Attach");
        }
        living_entity_uuids(p);
        replace_least_most(p, "UUID", "UUID");
    });
}

/// `LevelUUIDFix`.
fn level_uuids(cx: &mut Cx) {
    cx.each(Ty::Level, |level| {
        if let Some(Tag::Compound(dimensions)) = level.get_mut("DimensionData") {
            for dimension in dimensions.values_mut() {
                if let Some(fight) = dimension.get_mut("DragonFight") {
                    replace_least_most(fight, "DragonUUID", "Dragon");
                }
            }
        }
        replace_uuid_string(level, "WanderingTraderId", "WanderingTraderId");
        if let Some(Tag::Compound(events)) = level.get_mut("CustomBossEvents") {
            for event in events.values_mut() {
                map_elements(event, "Players", |player| {
                    uuid_from_longs(&player, "M", "L").unwrap_or(player)
                });
            }
        }
    });
}

/// `SavedDataUUIDFix` (raids).
fn raids_uuids(cx: &mut Cx) {
    cx.each(Ty::SavedRaids, |saved| {
        let Some(data) = saved.get_mut("data") else {
            return;
        };
        map_elements(data, "Raids", |mut raid| {
            map_elements(&mut raid, "HeroesOfTheVillage", |hero| {
                uuid_from_longs(&hero, "UUIDMost", "UUIDLeast").unwrap_or(hero)
            });
            raid
        });
    });
}

/// `ItemStackUUIDFix`.
fn item_stack_uuids(cx: &mut Cx) {
    cx.each(Ty::ItemStack, |stack| {
        let head =
            as_str(stack.get("id")).is_some_and(|id| namespaced(&id) == "minecraft:player_head");
        let Some(tag) = stack.get_mut("tag") else {
            return;
        };
        map_elements(tag, "AttributeModifiers", |mut modifier| {
            replace_least_most(&mut modifier, "UUID", "UUID");
            modifier
        });
        if head && let Some(owner) = tag.get_mut("SkullOwner") {
            replace_uuid_string(owner, "Id", "Id");
        }
    });
}

/// `GossipUUIDFix`.
fn gossip_uuids(cx: &mut Cx, entity: &str) {
    cx.entity(entity, |e| {
        if let Some(gossips) = e.get_mut("Gossips")
            && crate::datafix::dynamic::is_list(Some(gossips))
        {
            *gossips = Tag::List(
                stream(Some(gossips))
                    .into_iter()
                    .map(|mut g| {
                        replace_least_most(&mut g, "Target", "Target");
                        g
                    })
                    .collect(),
            );
        }
    });
}

// ------------------------------------------------------------------ 2518 to 2535

/// `JigsawPropertiesFix`.
fn jigsaw_properties(cx: &mut Cx) {
    cx.block_entity("minecraft:jigsaw", |be| {
        let name = super::dynamic::as_string(be.get("attachement_type"), "minecraft:empty");
        let pool = super::dynamic::as_string(be.get("target_pool"), "minecraft:empty");
        set(be, "name", string(&name));
        set(be, "target", string(&name));
        remove(be, "attachement_type");
        set(be, "pool", string(&pool));
        remove(be, "target_pool");
    });
}

/// `LegacyJigsawRotationFix`.
fn jigsaw_rotation(cx: &mut Cx) {
    cx.each(Ty::BlockState, |state| {
        if as_str(state.get("Name")).is_none_or(|n| namespaced(&n) != "minecraft:jigsaw") {
            return;
        }
        if let Some(props) = state.get_mut("Properties") {
            let facing = super::dynamic::as_string(props.get("facing"), "north");
            let orientation = match facing.as_str() {
                "down" => "down_south",
                "up" => "up_north",
                "north" => "north_up",
                "south" => "south_up",
                "west" => "west_up",
                "east" => "east_up",
                other => other,
            };
            remove(props, "facing");
            set(props, "orientation", string(orientation));
        }
    });
}

const ATTRIBUTES: &[(&str, &str)] = &[
    ("generic.maxHealth", "minecraft:generic.max_health"),
    ("Max Health", "minecraft:generic.max_health"),
    (
        "zombie.spawnReinforcements",
        "minecraft:zombie.spawn_reinforcements",
    ),
    (
        "Spawn Reinforcements Chance",
        "minecraft:zombie.spawn_reinforcements",
    ),
    ("horse.jumpStrength", "minecraft:horse.jump_strength"),
    ("Jump Strength", "minecraft:horse.jump_strength"),
    ("generic.followRange", "minecraft:generic.follow_range"),
    ("Follow Range", "minecraft:generic.follow_range"),
    (
        "generic.knockbackResistance",
        "minecraft:generic.knockback_resistance",
    ),
    (
        "Knockback Resistance",
        "minecraft:generic.knockback_resistance",
    ),
    ("generic.movementSpeed", "minecraft:generic.movement_speed"),
    ("Movement Speed", "minecraft:generic.movement_speed"),
    ("generic.flyingSpeed", "minecraft:generic.flying_speed"),
    ("Flying Speed", "minecraft:generic.flying_speed"),
    ("generic.attackDamage", "minecraft:generic.attack_damage"),
    (
        "generic.attackKnockback",
        "minecraft:generic.attack_knockback",
    ),
    ("generic.attackSpeed", "minecraft:generic.attack_speed"),
    (
        "generic.armorToughness",
        "minecraft:generic.armor_toughness",
    ),
];

/// `BitStorageAlignFix.addPadding`: values stored without crossing longs.
pub(super) fn add_padding(size: usize, bits: usize, data: &[i64]) -> Vec<i64> {
    let length = data.len();
    if length == 0 || bits == 0 || bits >= 64 {
        return Vec::new();
    }
    let mask = (1i64 << bits) - 1;
    let per_long = 64 / bits;
    let required = size.div_ceil(per_long);
    let mut result = vec![0i64; required];
    let mut out_index = 0;
    let mut out_start = 0;
    let mut out_data = 0i64;
    let mut current_index = 0;
    let mut current = data[0];
    let mut next = if length > 1 { data[1] } else { 0 };
    for index in 0..size {
        let position = index * bits;
        let start_data = position >> 6;
        let end_data = ((index + 1) * bits - 1) >> 6;
        let start_bit = position ^ (start_data << 6);
        if start_data != current_index {
            current = next;
            next = if start_data + 1 < length {
                data[start_data + 1]
            } else {
                0
            };
            current_index = start_data;
        }
        let value = if start_data == end_data {
            ((current as u64) >> start_bit) as i64 & mask
        } else {
            let shift = 64 - start_bit;
            ((((current as u64) >> start_bit) as i64) | next.wrapping_shl(shift as u32)) & mask
        };
        let out_end = out_start + bits;
        if out_end >= 64 {
            result[out_index] = out_data;
            out_index += 1;
            out_data = value;
            out_start = bits;
        } else {
            out_data |= value << out_start;
            out_start = out_end;
        }
    }
    if out_data != 0 {
        result[out_index] = out_data;
    }
    result
}

fn align_storage(tag: &mut Tag, field: &str, size: usize, bits: usize) {
    if let Some(v) = tag.get_mut(field) {
        let input = as_long_stream(Some(v)).unwrap_or_default();
        *v = Tag::LongArray(add_padding(size, bits, &input));
    }
}

/// `BitStorageAlignFix`: block states and heightmaps no longer cross longs.
fn bit_storage_align(cx: &mut Cx) {
    cx.each(Ty::Chunk, |c| {
        let Some(level) = c.get_mut("Level") else {
            return;
        };
        if let Some(Tag::List(sections)) = level.get_mut("Sections") {
            for section in sections {
                let Some(Tag::List(palette)) = section.get("Palette") else {
                    continue;
                };
                let bits = 4.max(ceillog2(palette.len())) as usize;
                if bits & (bits - 1) != 0 {
                    align_storage(section, "BlockStates", 4096, bits);
                }
            }
        }
        if let Some(Tag::Compound(heightmaps)) = level.get_mut("Heightmaps") {
            for heightmap in heightmaps.values_mut() {
                let input = as_long_stream(Some(heightmap)).unwrap_or_default();
                *heightmap = Tag::LongArray(add_padding(256, 9, &input));
            }
        }
    });
}

/// `StriderGravityFix`.
fn strider_gravity(cx: &mut Cx) {
    cx.entity("minecraft:strider", |e| {
        if as_bool(e.get("NoGravity"), false) {
            set(e, "NoGravity", Tag::Byte(0));
        }
    });
}

/// `RedstoneWireConnectionsFix`.
fn redstone_wire(cx: &mut Cx) {
    cx.each(Ty::BlockState, |state| {
        if as_str(state.get("Name")).as_deref() != Some("minecraft:redstone_wire") {
            return;
        }
        let Some(props) = state.get_mut("Properties") else {
            return;
        };
        let side = |props: &Tag, name: &str| super::dynamic::as_string(props.get(name), "none");
        let (east, west, north, south) = (
            side(props, "east"),
            side(props, "west"),
            side(props, "north"),
            side(props, "south"),
        );
        let connected = |s: &str| s != "none";
        let east_west = connected(&east) || connected(&west);
        let north_south = connected(&north) || connected(&south);
        let fixed = [
            (
                "east",
                if !connected(&east) && !north_south {
                    "side"
                } else {
                    &east
                },
            ),
            (
                "west",
                if !connected(&west) && !north_south {
                    "side"
                } else {
                    &west
                },
            ),
            (
                "north",
                if !connected(&north) && !east_west {
                    "side"
                } else {
                    &north
                },
            ),
            (
                "south",
                if !connected(&south) && !east_west {
                    "side"
                } else {
                    &south
                },
            ),
        ];
        for (name, value) in fixed {
            if props.get(name).is_some() {
                set(props, name, string(value));
            }
        }
    });
}

/// `VillagerFollowRangeFix`.
fn villager_follow_range(cx: &mut Cx) {
    cx.entity("minecraft:villager", |e| {
        map_elements(e, "Attributes", |mut attribute| {
            if super::dynamic::as_string(attribute.get("Name"), "") == "generic.follow_range"
                && as_double(attribute.get("Base"), 0.0) == 16.0
            {
                set(&mut attribute, "Base", Tag::Double(48.0f64.to_bits()));
            }
            attribute
        });
    });
}

/// `EntityShulkerRotationFix`.
fn shulker_rotation(cx: &mut Cx) {
    cx.entity("minecraft:shulker", |e| {
        let mut rotation: Vec<f64> = stream(e.get("Rotation"))
            .iter()
            .map(|d| as_double(Some(d), 180.0))
            .collect();
        if !rotation.is_empty() {
            rotation[0] -= 180.0;
            set(
                e,
                "Rotation",
                Tag::List(
                    rotation
                        .into_iter()
                        .map(|d| Tag::Double(d.to_bits()))
                        .collect(),
                ),
            );
        }
    });
}

// ------------------------------------------------------------------ 2537 to 2553

/// `LegacyDimensionIdFix`: numeric dimensions of players and maps become names.
fn legacy_dimension_id(cx: &mut Cx) {
    let fix = |v: &mut Tag, field: &str| {
        if let Some(id) = v.get(field)
            && let Some(n) = as_number(id)
        {
            let name = match n.int_value() {
                -1 => "minecraft:the_nether",
                1 => "minecraft:the_end",
                _ => "minecraft:overworld",
            };
            set(v, field, string(name));
        }
    };
    cx.each(Ty::Player, |p| fix(p, "Dimension"));
    cx.each(Ty::SavedMapData, |m| {
        if let Some(data) = m.get_mut("data") {
            fix(data, "dimension");
        }
    });
}

/// `LevelLegacyWorldGenSettingsFix`: the generator settings move into `WorldGenSettings`.
fn level_legacy_world_gen_settings(cx: &mut Cx) {
    cx.each(Ty::Level, |data| {
        let mut settings = data
            .get("WorldGenSettings")
            .cloned()
            .unwrap_or_else(super::dynamic::empty_map);
        for key in [
            "RandomSeed",
            "generatorName",
            "generatorOptions",
            "generatorVersion",
            "legacy_custom_options",
            "MapFeatures",
            "BonusChest",
        ] {
            if let Some(old) = remove(data, key) {
                set(&mut settings, key, old);
            }
        }
        set(data, "WorldGenSettings", settings);
    });
}

fn map_of(entries: impl IntoIterator<Item = (&'static str, Tag)>) -> Tag {
    Tag::compound(entries)
}

fn noise_generator(seed: i64, settings: Tag, biome_source: Tag) -> Tag {
    map_of([
        ("type", string("minecraft:noise")),
        ("biome_source", biome_source),
        ("seed", Tag::Long(seed)),
        ("settings", settings),
    ])
}

fn vanilla_biome_source(seed: i64, legacy_biome_init_layer: bool, large_biomes: bool) -> Tag {
    let mut source = map_of([
        ("type", string("minecraft:vanilla_layered")),
        ("seed", Tag::Long(seed)),
        ("large_biomes", Tag::Byte(i8::from(large_biomes))),
    ]);
    if legacy_biome_init_layer {
        set(&mut source, "legacy_biome_init_layer", Tag::Byte(1));
    }
    source
}

fn default_overworld(seed: i64) -> Tag {
    noise_generator(
        seed,
        string("minecraft:overworld"),
        vanilla_biome_source(seed, false, false),
    )
}

/// `WorldGenSettingsFix.vanillaLevels`.
pub(super) fn vanilla_levels(seed: i64, overworld: Tag, caves: bool) -> Tag {
    let overworld_type = if caves {
        "minecraft:overworld_caves"
    } else {
        "minecraft:overworld"
    };
    map_of([
        (
            "minecraft:overworld",
            map_of([("type", string(overworld_type)), ("generator", overworld)]),
        ),
        (
            "minecraft:the_nether",
            map_of([
                ("type", string("minecraft:the_nether")),
                (
                    "generator",
                    noise_generator(
                        seed,
                        string("minecraft:nether"),
                        map_of([
                            ("type", string("minecraft:multi_noise")),
                            ("seed", Tag::Long(seed)),
                            ("preset", string("minecraft:nether")),
                        ]),
                    ),
                ),
            ]),
        ),
        (
            "minecraft:the_end",
            map_of([
                ("type", string("minecraft:the_end")),
                (
                    "generator",
                    noise_generator(
                        seed,
                        string("minecraft:end"),
                        map_of([
                            ("type", string("minecraft:the_end")),
                            ("seed", Tag::Long(seed)),
                        ]),
                    ),
                ),
            ]),
        ),
    ])
}

/// The default `StructureFeatureConfiguration` of a structure: spacing, separation, salt.
fn structure_default(name: &str) -> (i32, i32, i32) {
    match name {
        "minecraft:village" => (32, 8, 10387312),
        "minecraft:desert_pyramid" => (32, 8, 14357617),
        "minecraft:igloo" => (32, 8, 14357618),
        "minecraft:jungle_pyramid" => (32, 8, 14357619),
        "minecraft:swamp_hut" => (32, 8, 14357620),
        "minecraft:pillager_outpost" => (32, 8, 165745296),
        "minecraft:monument" => (32, 5, 10387313),
        "minecraft:endcity" => (20, 11, 10387313),
        _ => (80, 20, 10387319), // minecraft:mansion
    }
}

/// `NumberUtils.toInt(s, default)` with a minimum.
fn option_int(value: &str, default: i32, min: i32) -> i32 {
    value.parse::<i32>().unwrap_or(default).max(min)
}

fn set_spacing(
    config: &mut Vec<(&'static str, (i32, i32, i32))>,
    structure: &'static str,
    value: &str,
    min: i32,
) {
    let current = config
        .iter()
        .find(|(n, _)| *n == structure)
        .map_or_else(|| structure_default(structure), |(_, c)| *c);
    let spacing = option_int(value, current.0, min);
    let updated = (spacing, current.1, current.2);
    match config.iter_mut().find(|(n, _)| *n == structure) {
        Some(entry) => entry.1 = updated,
        None => config.push((structure, updated)),
    }
}

/// `WorldGenSettingsFix.fixFlatStructures`.
fn fix_flat_structures(settings: Option<&Tag>) -> Tag {
    let (mut distance, mut spread, mut count) = (32, 3, 128);
    let mut has_stronghold = false;
    let mut config: Vec<(&'static str, (i32, i32, i32))> = Vec::new();
    if settings.is_none() {
        has_stronghold = true;
        config.push(("minecraft:village", structure_default("minecraft:village")));
    }
    if let Some(Tag::Compound(structures)) = settings.and_then(|s| s.get("structures")) {
        for (structure_key, options) in structures {
            let Tag::Compound(options) = options else {
                continue;
            };
            let structure = structure_key.to_string_lossy();
            for (option_key, option_value) in options {
                let option = option_key.to_string_lossy();
                let value = as_str(Some(option_value)).unwrap_or_default();
                if structure == "stronghold" {
                    has_stronghold = true;
                    match option.as_str() {
                        "distance" => distance = option_int(&value, distance, 1),
                        "spread" => spread = option_int(&value, spread, 1),
                        "count" => count = option_int(&value, count, 1),
                        _ => {}
                    }
                } else {
                    match (option.as_str(), structure.as_str()) {
                        ("distance", "village") => {
                            set_spacing(&mut config, "minecraft:village", &value, 9)
                        }
                        ("distance", "biome_1") => {
                            for s in [
                                "minecraft:desert_pyramid",
                                "minecraft:igloo",
                                "minecraft:jungle_pyramid",
                                "minecraft:swamp_hut",
                                "minecraft:pillager_outpost",
                            ] {
                                set_spacing(&mut config, s, &value, 9);
                            }
                        }
                        ("distance", "endcity") => {
                            set_spacing(&mut config, "minecraft:endcity", &value, 1)
                        }
                        ("distance", "mansion") => {
                            set_spacing(&mut config, "minecraft:mansion", &value, 1)
                        }
                        ("separation", "oceanmonument") => {
                            let current = config
                                .iter()
                                .find(|(n, _)| *n == "minecraft:monument")
                                .map_or_else(
                                    || structure_default("minecraft:monument"),
                                    |(_, c)| *c,
                                );
                            let spacing = option_int(&value, current.1, 1);
                            let updated = (spacing, current.1, current.2);
                            match config.iter_mut().find(|(n, _)| *n == "minecraft:monument") {
                                Some(entry) => entry.1 = updated,
                                None => config.push(("minecraft:monument", updated)),
                            }
                        }
                        ("spacing", "oceanmonument") => {
                            set_spacing(&mut config, "minecraft:monument", &value, 1)
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    let mut structures = Compound::new();
    for (name, (spacing, separation, salt)) in config {
        structures.insert(
            JavaString::from(name),
            map_of([
                ("spacing", Tag::Int(spacing)),
                ("separation", Tag::Int(separation)),
                ("salt", Tag::Int(salt)),
            ]),
        );
    }
    let mut out = map_of([("structures", Tag::Compound(structures))]);
    if has_stronghold {
        set(
            &mut out,
            "stronghold",
            map_of([
                ("distance", Tag::Int(distance)),
                ("spread", Tag::Int(spread)),
                ("count", Tag::Int(count)),
            ]),
        );
    }
    out
}

/// `WorldGenSettingsFix`: the legacy generator settings become dimensions.
fn world_gen_settings(cx: &mut Cx) {
    cx.each(Ty::WorldGenSettings, |input| {
        let seed = as_long(input.get("RandomSeed"), 0);
        let name = as_str(input.get("generatorName")).map(|n| n.to_lowercase());
        let legacy_custom_options = as_str(input.get("legacy_custom_options")).or_else(|| {
            (name.as_deref() == Some("customized"))
                .then(|| as_str(input.get("generatorOptions")))
                .flatten()
        });
        let mut caves = false;
        let generator = match name.as_deref() {
            None | Some("customized") => default_overworld(seed),
            Some("flat") => {
                let settings = input.get("generatorOptions");
                let layers = settings
                    .and_then(|s| s.get("layers"))
                    .cloned()
                    .unwrap_or_else(|| {
                        Tag::List(
                            [
                                (1, "minecraft:bedrock"),
                                (2, "minecraft:dirt"),
                                (1, "minecraft:grass_block"),
                            ]
                            .into_iter()
                            .map(|(h, b)| map_of([("height", Tag::Int(h)), ("block", string(b))]))
                            .collect(),
                        )
                    });
                map_of([
                    ("type", string("minecraft:flat")),
                    (
                        "settings",
                        map_of([
                            ("structures", fix_flat_structures(settings)),
                            ("layers", layers),
                            (
                                "biome",
                                string(&super::dynamic::as_string(
                                    settings.and_then(|s| s.get("biome")),
                                    "minecraft:plains",
                                )),
                            ),
                        ]),
                    ),
                ])
            }
            Some("debug_all_block_states") => map_of([("type", string("minecraft:debug"))]),
            Some("buffet") => {
                let settings = input.get("generatorOptions");
                let kind = settings
                    .and_then(|s| s.get("chunk_generator"))
                    .and_then(|g| as_str(g.get("type")));
                let noise_settings = match kind.as_deref() {
                    Some("minecraft:caves") => {
                        caves = true;
                        "minecraft:caves"
                    }
                    Some("minecraft:floating_islands") => "minecraft:floating_islands",
                    _ => "minecraft:overworld",
                };
                let source = settings
                    .and_then(|s| s.get("biome_source"))
                    .cloned()
                    .unwrap_or_else(|| map_of([("type", string("minecraft:fixed"))]));
                let fixed = if as_str(source.get("type")).as_deref() == Some("minecraft:fixed") {
                    let biome = stream(source.get("options").and_then(|o| o.get("biomes")))
                        .first()
                        .and_then(|b| as_str(Some(b)))
                        .unwrap_or_else(|| "minecraft:ocean".to_owned());
                    let mut fixed = source;
                    remove(&mut fixed, "options");
                    set(&mut fixed, "biome", string(&biome));
                    fixed
                } else {
                    source
                };
                noise_generator(seed, string(noise_settings), fixed)
            }
            Some(other) => {
                let normal = other == "default";
                let legacy = other == "default_1_1"
                    || normal && as_int(input.get("generatorVersion"), 0) == 0;
                noise_generator(
                    seed,
                    string(if other == "amplified" {
                        "minecraft:amplified"
                    } else {
                        "minecraft:overworld"
                    }),
                    vanilla_biome_source(seed, legacy, other == "largebiomes"),
                )
            }
        };
        let mut out = map_of([
            ("seed", Tag::Long(seed)),
            (
                "generate_features",
                Tag::Byte(i8::from(as_bool(input.get("MapFeatures"), true))),
            ),
            (
                "bonus_chest",
                Tag::Byte(i8::from(as_bool(input.get("BonusChest"), false))),
            ),
            ("dimensions", vanilla_levels(seed, generator, caves)),
        ]);
        if let Some(options) = legacy_custom_options {
            set(&mut out, "legacy_custom_options", string(&options));
        }
        *input = out;
    });
}

/// `BiomeFix.BIOMES`.
const BIOMES: &[(&str, &str)] = &[
    ("minecraft:extreme_hills", "minecraft:mountains"),
    ("minecraft:swampland", "minecraft:swamp"),
    ("minecraft:hell", "minecraft:nether_wastes"),
    ("minecraft:sky", "minecraft:the_end"),
    ("minecraft:ice_flats", "minecraft:snowy_tundra"),
    ("minecraft:ice_mountains", "minecraft:snowy_mountains"),
    ("minecraft:mushroom_island", "minecraft:mushroom_fields"),
    (
        "minecraft:mushroom_island_shore",
        "minecraft:mushroom_field_shore",
    ),
    ("minecraft:beaches", "minecraft:beach"),
    ("minecraft:forest_hills", "minecraft:wooded_hills"),
    ("minecraft:smaller_extreme_hills", "minecraft:mountain_edge"),
    ("minecraft:stone_beach", "minecraft:stone_shore"),
    ("minecraft:cold_beach", "minecraft:snowy_beach"),
    ("minecraft:roofed_forest", "minecraft:dark_forest"),
    ("minecraft:taiga_cold", "minecraft:snowy_taiga"),
    ("minecraft:taiga_cold_hills", "minecraft:snowy_taiga_hills"),
    ("minecraft:redwood_taiga", "minecraft:giant_tree_taiga"),
    (
        "minecraft:redwood_taiga_hills",
        "minecraft:giant_tree_taiga_hills",
    ),
    (
        "minecraft:extreme_hills_with_trees",
        "minecraft:wooded_mountains",
    ),
    ("minecraft:savanna_rock", "minecraft:savanna_plateau"),
    ("minecraft:mesa", "minecraft:badlands"),
    ("minecraft:mesa_rock", "minecraft:wooded_badlands_plateau"),
    ("minecraft:mesa_clear_rock", "minecraft:badlands_plateau"),
    ("minecraft:sky_island_low", "minecraft:small_end_islands"),
    ("minecraft:sky_island_medium", "minecraft:end_midlands"),
    ("minecraft:sky_island_high", "minecraft:end_highlands"),
    ("minecraft:sky_island_barren", "minecraft:end_barrens"),
    ("minecraft:void", "minecraft:the_void"),
    ("minecraft:mutated_plains", "minecraft:sunflower_plains"),
    ("minecraft:mutated_desert", "minecraft:desert_lakes"),
    (
        "minecraft:mutated_extreme_hills",
        "minecraft:gravelly_mountains",
    ),
    ("minecraft:mutated_forest", "minecraft:flower_forest"),
    ("minecraft:mutated_taiga", "minecraft:taiga_mountains"),
    ("minecraft:mutated_swampland", "minecraft:swamp_hills"),
    ("minecraft:mutated_ice_flats", "minecraft:ice_spikes"),
    ("minecraft:mutated_jungle", "minecraft:modified_jungle"),
    (
        "minecraft:mutated_jungle_edge",
        "minecraft:modified_jungle_edge",
    ),
    (
        "minecraft:mutated_birch_forest",
        "minecraft:tall_birch_forest",
    ),
    (
        "minecraft:mutated_birch_forest_hills",
        "minecraft:tall_birch_hills",
    ),
    (
        "minecraft:mutated_roofed_forest",
        "minecraft:dark_forest_hills",
    ),
    (
        "minecraft:mutated_taiga_cold",
        "minecraft:snowy_taiga_mountains",
    ),
    (
        "minecraft:mutated_redwood_taiga",
        "minecraft:giant_spruce_taiga",
    ),
    (
        "minecraft:mutated_redwood_taiga_hills",
        "minecraft:giant_spruce_taiga_hills",
    ),
    (
        "minecraft:mutated_extreme_hills_with_trees",
        "minecraft:modified_gravelly_mountains",
    ),
    ("minecraft:mutated_savanna", "minecraft:shattered_savanna"),
    (
        "minecraft:mutated_savanna_rock",
        "minecraft:shattered_savanna_plateau",
    ),
    ("minecraft:mutated_mesa", "minecraft:eroded_badlands"),
    (
        "minecraft:mutated_mesa_rock",
        "minecraft:modified_wooded_badlands_plateau",
    ),
    (
        "minecraft:mutated_mesa_clear_rock",
        "minecraft:modified_badlands_plateau",
    ),
    ("minecraft:warm_deep_ocean", "minecraft:deep_warm_ocean"),
    (
        "minecraft:lukewarm_deep_ocean",
        "minecraft:deep_lukewarm_ocean",
    ),
    ("minecraft:cold_deep_ocean", "minecraft:deep_cold_ocean"),
    ("minecraft:frozen_deep_ocean", "minecraft:deep_frozen_ocean"),
];

// ------------------------------------------------------------------ 1800 to 1963

/// `ItemLoreFix`: the lore lines become JSON text components.
fn item_lore(cx: &mut Cx) {
    cx.each(Ty::ItemStack, |stack| {
        let Some(lore) = stack
            .get_mut("tag")
            .and_then(|t| t.get_mut("display"))
            .and_then(|d| d.get_mut("Lore"))
        else {
            return;
        };
        if let Tag::List(lines) = lore {
            for line in lines {
                if let Tag::String(text) = line {
                    *line = Tag::String(JavaString(super::json::text_component_json(&text.0)));
                }
            }
        }
    });
}

/// `EntityCatSplitFix`: tame ocelots become cats. (The vanilla fix sets
/// `Trusting` on a copy it then drops.)
fn cat_split(cx: &mut Cx) {
    cx.each(Ty::Entity, |e| {
        if !id_is(e, "minecraft:ocelot") {
            return;
        }
        let kind = as_int(e.get("CatType"), 0);
        if kind > 0 && kind < 4 {
            set(e, "CatType", Tag::Int(kind));
            let owner = super::dynamic::as_string(e.get("OwnerUUID"), "");
            set(e, "OwnerUUID", string(&owner));
            set(e, "id", string("minecraft:cat"));
        }
    });
}

/// The `Status` of a chunk, mapped (`ChunkStatusFix`, `ChunkStatusFix2`).
fn level_status(cx: &mut Cx, f: impl Fn(&str) -> Option<&'static str>) {
    cx.each(Ty::Chunk, |c| {
        let Some(level) = c.get_mut("Level") else {
            return;
        };
        let status = super::dynamic::as_string(level.get("Status"), "empty");
        if let Some(new) = f(&status)
            && new != status
        {
            set(level, "Status", string(new));
        }
    });
}

/// A list field filtered by `keep`; a present field that is no list becomes empty.
fn map_elements_filter(tag: &mut Tag, field: &str, keep: impl Fn(&Tag) -> bool) {
    if let Some(v) = tag.get_mut(field) {
        *v = Tag::List(stream(Some(v)).into_iter().filter(|e| keep(e)).collect());
    }
}

/// `VillagerDataFix`: profession and career become `VillagerData`.
fn villager_data(cx: &mut Cx, entity: &str) {
    cx.entity(entity, |e| {
        let profession = as_int(e.get("Profession"), 0);
        let career = as_int(e.get("Career"), 0);
        let profession = match (profession, career) {
            (0, 2) => "minecraft:fisherman",
            (0, 3) => "minecraft:shepherd",
            (0, 4) => "minecraft:fletcher",
            (0, _) => "minecraft:farmer",
            (1, 2) => "minecraft:cartographer",
            (1, _) => "minecraft:librarian",
            (2, _) => "minecraft:cleric",
            (3, 2) => "minecraft:weaponsmith",
            (3, 3) => "minecraft:toolsmith",
            (3, _) => "minecraft:armorer",
            (4, 2) => "minecraft:leatherworker",
            (4, _) => "minecraft:butcher",
            (5, _) => "minecraft:nitwit",
            _ => "minecraft:none",
        };
        let level = e.get("CareerLevel").cloned().unwrap_or(Tag::Int(1));
        remove(e, "Profession");
        remove(e, "Career");
        remove(e, "CareerLevel");
        set(
            e,
            "VillagerData",
            Tag::compound([
                ("type", string("minecraft:plains")),
                ("profession", string(profession)),
                ("level", level),
            ]),
        );
    });
}

/// `NewVillageFix`: the new village structure takes the old one's name.
fn new_village(cx: &mut Cx) {
    cx.each(Ty::Chunk, |c| {
        let Some(structures) = c.get_mut("Level").and_then(|l| l.get_mut("Structures")) else {
            return;
        };
        if let Some(Tag::Compound(starts)) = structures.get_mut("Starts") {
            starts.remove(&JavaString::from("Village"));
            if let Some(v) = starts.remove(&JavaString::from("New_Village")) {
                starts.insert(JavaString::from("Village"), v);
            }
        }
        if let Some(references) = structures.get_mut("References") {
            if let Some(v) = remove(references, "New_Village") {
                set(references, "Village", v);
            }
            remove(references, "Village");
        }
    });
    cx.each(Ty::StructureFeature, |s| {
        if let Some(id) = s.get("id") {
            let id = as_str(Some(id)).unwrap_or_default();
            if namespaced(&id) == "minecraft:new_village" {
                set(s, "id", string("minecraft:village"));
            }
        }
    });
}

/// `ReorganizePoi`: the sections `0` to `15` move below `Sections`.
fn reorganize_poi(cx: &mut Cx) {
    cx.each(Ty::PoiChunk, |poi| {
        let mut sections = Compound::new();
        for i in 0..16 {
            let name = i.to_string();
            if let Some(records) = remove(poi, &name) {
                sections.insert(
                    JavaString::from(name.as_str()),
                    Tag::compound([("Records", records)]),
                );
            }
        }
        set(poi, "Sections", Tag::Compound(sections));
    });
}

const ILLAGER_BANNER: &str = "\"translate\":\"block.minecraft.illager_banner\"";
const OMINOUS_BANNER: &str = "\"translate\":\"block.minecraft.ominous_banner\"";

/// `OminousBannerRenameFix`.
fn ominous_banner_item(cx: &mut Cx) {
    cx.item(
        |id| id == "minecraft:white_banner",
        |stack| {
            if let Some(name) = stack
                .get_mut("tag")
                .and_then(|t| t.get_mut("display"))
                .and_then(|d| d.get_mut("Name"))
                && let Some(text) = as_str(Some(name))
            {
                *name = string(&text.replace(ILLAGER_BANNER, OMINOUS_BANNER));
            }
        },
    );
}

/// `OminousBannerBlockEntityRenameFix`.
fn ominous_banner_block_entity(cx: &mut Cx) {
    cx.block_entity("minecraft:banner", |be| {
        if let Some(name) = be.get_mut("CustomName")
            && let Some(text) = as_str(Some(name))
        {
            *name = string(&text.replace(ILLAGER_BANNER, OMINOUS_BANNER));
        }
    });
}

const LEVEL_XP_THRESHOLDS: [i32; 5] = [0, 10, 50, 100, 150];

fn min_xp_per_level(level: i32) -> i32 {
    LEVEL_XP_THRESHOLDS[(level - 1).clamp(0, 4) as usize]
}

/// `VillagerRebuildLevelAndXpFix`.
fn villager_level_and_xp(cx: &mut Cx) {
    cx.entity("minecraft:villager", |e| {
        let mut level = as_int(e.get("VillagerData").and_then(|d| d.get("level")), 0);
        if level == 0 || level == 1 {
            let offers = e
                .get("Offers")
                .and_then(|o| o.get("Recipes"))
                .map_or(0, |r| stream(Some(r)).len() as i32);
            level = (offers / 2).clamp(1, 5);
            if level > 1
                && let Some(data) = e.get_mut("VillagerData")
            {
                set(data, "level", Tag::Int(level));
            }
        }
        if e.get("Xp").and_then(as_number).is_none() {
            set(e, "Xp", Tag::Int(min_xp_per_level(level)));
        }
    });
}

/// `ZombieVillagerRebuildXpFix`.
fn zombie_villager_xp(cx: &mut Cx) {
    cx.entity("minecraft:zombie_villager", |e| {
        if e.get("Xp").and_then(as_number).is_none() {
            let level = as_int(e.get("VillagerData").and_then(|d| d.get("level")), 1);
            set(e, "Xp", Tag::Int(min_xp_per_level(level)));
        }
    });
}
