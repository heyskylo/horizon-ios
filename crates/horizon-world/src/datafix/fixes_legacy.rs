//! Fixes of 1.8 to 1.12 (keys 100 to 1125, DataVersion up to 1343): what worlds
//! saved by those versions need before the flattening ([`super::fixes_flat`]).
//! The options fixes of this range (505, 816, 1344) are client settings.
use super::dynamic::{
    as_bool, as_float, as_int, as_int_stream, as_number, as_str, boolean, empty_map, is_list,
    namespaced, remove, set, stream, string,
};
use super::schema::flat::entity_tree_known;
use super::schema::{Key, Ty, block_entity_known, key};
use super::tables as t;
use super::{Cx, FixFn, drop_typed, items, mark_read, reread, reread_as};
use crate::nbt::{JavaString, Tag};

pub(super) fn register(out: &mut Vec<(Key, FixFn)>) {
    out.push((key(100, 0), equipment_to_armor_and_hands));
    out.push((key(101, 0), |cx| {
        cx.entity("Villager", |e| set(e, "CanPickUpLoot", boolean(true)))
    }));
    out.push((key(102, 0), item_ids));
    out.push((key(102, 0), item_potions));
    out.push((key(105, 0), spawn_egg_entities));
    out.push((key(106, 0), spawner_entity_identifiers));
    out.push((key(107, 0), minecart_identifiers));
    out.push((key(108, 0), entity_string_uuid));
    out.push((key(109, 0), entity_health));
    out.push((key(110, 0), horse_saddle));
    out.push((key(111, 0), painting_item_frame_direction));
    out.push((key(113, 0), redundant_chance_tags));
    out.push((key(135, 0), riding_to_passengers));
    out.push((key(143, 0), |cx| {
        cx.entity("TippedArrow", |e| set(e, "id", string("Arrow")))
    }));
    out.push((key(147, 0), armor_stand_silent));
    out.push((key(165, 0), sign_text_strict_json));
    out.push((key(165, 0), written_book_pages_strict_json));
    out.push((key(502, 0), |cx| {
        cx.rename(Ty::ItemName, |name| {
            (namespaced(name) == "minecraft:cooked_fished").then_some("minecraft:cooked_fish")
        })
    }));
    out.push((key(502, 0), zombie_villager_type));
    out.push((key(700, 0), elder_guardian_split));
    out.push((key(701, 0), skeleton_split));
    out.push((key(702, 0), zombie_split));
    out.push((key(703, 0), horse_split));
    out.push((key(704, 0), block_entity_ids));
    out.push((key(705, 0), entity_ids));
    out.push((key(804, 0), item_banner_color));
    out.push((key(806, 0), item_water_potion));
    out.push((key(808, 1), shulker_color));
    out.push((key(813, 0), item_shulker_box_color));
    out.push((key(813, 0), |cx| {
        cx.block_entity("minecraft:shulker_box", |be| {
            remove(be, "Color");
        })
    }));
    out.push((key(820, 0), |cx| {
        cx.rename(Ty::ItemName, |name| {
            (namespaced(name) == "minecraft:totem").then_some("minecraft:totem_of_undying")
        })
    }));
    out.push((key(1022, 0), |cx| {
        let k = cx.k;
        cx.each(Ty::Player, |p| reread_as(Ty::Player, k, p))
    }));
    out.push((key(1125, 0), bed_block_entities));
    out.push((key(1125, 0), |cx| {
        cx.item(
            |id| id == "minecraft:bed",
            |s| {
                if as_int(s.get("Damage"), 0) == 0 {
                    set(s, "Damage", Tag::Short(14));
                }
            },
        )
    }));
}

/// `Number.shortValue` of a numeric field, or the default (`Dynamic.asShort`).
fn as_short(tag: Option<&Tag>, default: i16) -> i16 {
    tag.and_then(as_number)
        .map_or(default, |n| n.int_value() as i16)
}

fn lookup_int(table: &'static [(i32, &'static str)], k: i32) -> Option<&'static str> {
    table.iter().find(|(i, _)| *i == k).map(|(_, v)| *v)
}

fn lookup_str(table: &'static [(&'static str, &'static str)], k: &str) -> Option<&'static str> {
    table.iter().find(|(i, _)| *i == k).map(|(_, v)| *v)
}

fn float_list(values: &[f32]) -> Tag {
    Tag::List(values.iter().map(|v| Tag::Float(v.to_bits())).collect())
}

// ------------------------------------------------------------------ 100

/// `EntityEquipmentToArmorAndHandFix`: the `Equipment` list becomes hand and armor items.
fn equipment_to_armor_and_hands(cx: &mut Cx) {
    cx.each(Ty::Entity, |e| {
        // The drop chances come first.
        let chances: Option<Vec<f32>> =
            e.get("DropChances").filter(|c| is_list(Some(c))).map(|c| {
                stream(Some(c))
                    .iter()
                    .map(|v| as_float(Some(v), 0.0))
                    .collect()
            });
        remove(e, "DropChances");
        if let Some(mut chances) = chances {
            chances.extend([0.0; 5]);
            if e.get("HandDropChances").is_none() {
                set(e, "HandDropChances", float_list(&[chances[0], 0.0]));
            }
            if e.get("ArmorDropChances").is_none() {
                set(e, "ArmorDropChances", float_list(&chances[1..5]));
            }
        }
        // The equipment list reads only if every element is a compound.
        let items: Vec<Tag> = match e.get("Equipment") {
            Some(Tag::List(items)) if items.iter().all(|i| matches!(i, Tag::Compound(_))) => {
                items.clone()
            }
            _ => Vec::new(),
        };
        if !items.is_empty() {
            set(
                e,
                "HandItems",
                Tag::List(vec![items[0].clone(), empty_map()]),
            );
        }
        if items.len() > 1 {
            let mut armor = vec![empty_map(); 4];
            let end = items.len().min(5);
            armor[..end - 1].clone_from_slice(&items[1..end]);
            set(e, "ArmorItems", Tag::List(armor));
        }
        // `Equipment` is no field of the new type: the map it was read from gives it back.
        drop_typed(e, "Equipment");
    });
}

// ------------------------------------------------------------------ 102

/// `ItemIdFix`: numeric item ids become names.
fn item_ids(cx: &mut Cx) {
    cx.each(Ty::ItemStack, |s| {
        let new = match s.get("id") {
            Some(Tag::String(id)) => namespaced(&id.to_string_lossy()),
            Some(other) => match as_number(other) {
                Some(n) => lookup_int(t::tables().item_ids, n.int_value())
                    .unwrap_or("minecraft:air")
                    .to_owned(),
                None => return,
            },
            None => return,
        };
        set(s, "id", string(&new));
    });
}

/// `ItemPotionFix`: potion damage values become potion names.
fn item_potions(cx: &mut Cx) {
    cx.item(
        |id| id == "minecraft:potion",
        |s| {
            if !matches!(s.get("tag"), Some(Tag::Compound(_))) {
                return;
            }
            let damage = i32::from(as_short(s.get("Damage"), 0));
            if let Some(tag) = s.get_mut("tag")
                && !matches!(tag.get("Potion"), Some(Tag::String(_)))
            {
                let potion =
                    lookup_int(t::tables().item_potions, damage & 127).unwrap_or("minecraft:water");
                set(tag, "Potion", string(potion));
                if damage & 16384 == 16384 {
                    set(s, "id", string("minecraft:splash_potion"));
                }
            }
            if damage != 0 {
                set(s, "Damage", Tag::Short(0));
            }
        },
    );
}

// ------------------------------------------------------------------ 105

/// `ItemSpawnEggFix`: the damage of a spawn egg becomes the entity it spawns.
fn spawn_egg_entities(cx: &mut Cx) {
    let (k, input) = (cx.k, cx.k - 1);
    cx.item(
        |id| id == "minecraft:spawn_egg",
        |s| {
            let damage = i32::from(as_short(s.get("Damage"), 0));
            if let Some(entity) = lookup_int(t::tables().spawn_egg_entities, damage & 0xFF) {
                // The entity tree is typed only if its id is one of the schema's.
                let typed = s
                    .get("tag")
                    .and_then(|tag| tag.get("EntityTag"))
                    .filter(|e| entity_tree_known(e, input))
                    .cloned();
                let old_id = typed.as_ref().and_then(|e| as_str(e.get("id")));
                if old_id.as_deref() != Some(entity) {
                    if !matches!(s.get("tag"), Some(Tag::Compound(_))) {
                        set(s, "tag", empty_map());
                    }
                    let mut entity_tag = typed.unwrap_or_else(empty_map);
                    set(&mut entity_tag, "id", string(entity));
                    reread_as(Ty::EntityTree, k, &mut entity_tag);
                    if let Some(tag) = s.get_mut("tag") {
                        set(tag, "EntityTag", entity_tag);
                    }
                }
            }
            if damage != 0 {
                set(s, "Damage", Tag::Short(0));
            }
        },
    );
}

// ------------------------------------------------------------------ 106

/// `MobSpawnerEntityIdentifiersFix`: the spawned entity is a tree of its own.
fn spawner_entity_identifiers(cx: &mut Cx) {
    let k = cx.k;
    cx.each(Ty::UntaggedSpawner, |sp| {
        if !matches!(sp, Tag::Compound(_)) {
            return;
        }
        if let Some(Tag::String(entity)) = sp.get("EntityId") {
            let id = if entity.0.is_empty() {
                string("Pig")
            } else {
                Tag::String(entity.clone())
            };
            let mut data = sp.get("SpawnData").cloned().unwrap_or_else(empty_map);
            if matches!(data, Tag::Compound(_)) {
                set(&mut data, "id", id);
            }
            set(sp, "SpawnData", data);
            remove(sp, "EntityId");
        }
        if let Some(Tag::List(potentials)) = sp.get_mut("SpawnPotentials") {
            for potential in potentials {
                if let Some(Tag::String(entity)) = potential.get("Type") {
                    let entity = entity.clone();
                    let mut data = potential
                        .get("Properties")
                        .cloned()
                        .unwrap_or_else(empty_map);
                    if matches!(data, Tag::Compound(_)) {
                        set(&mut data, "id", Tag::String(entity));
                    }
                    set(potential, "Entity", data);
                    remove(potential, "Type");
                    remove(potential, "Properties");
                }
            }
        }
        reread_as(Ty::UntaggedSpawner, k, sp);
    });
}

// ------------------------------------------------------------------ 107

/// `EntityMinecartIdentifiersFix`: a minecart's type becomes its entity id.
fn minecart_identifiers(cx: &mut Cx) {
    let k = cx.k;
    cx.entity("Minecart", |e| {
        let name = match as_int(e.get("Type"), 0) {
            1 => "MinecartChest",
            2 => "MinecartFurnace",
            _ => "MinecartRideable",
        };
        set(e, "id", string(name));
        remove(e, "Type");
        reread(k, e);
    });
}

// ------------------------------------------------------------------ 108

/// `UUID.fromString`.
fn parse_uuid(s: &str) -> Option<(i64, i64)> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 5 {
        return None;
    }
    let part = |i: usize| i64::from_str_radix(parts[i], 16).ok();
    let mut most = part(0)? & 0xFFFF_FFFF;
    most = (most << 16) | (part(1)? & 0xFFFF);
    most = (most << 16) | (part(2)? & 0xFFFF);
    let mut least = part(3)? & 0xFFFF;
    least = (least << 48) | (part(4)? & 0xFFFF_FFFF_FFFF);
    Some((most, least))
}

/// `EntityStringUuidFix`: the string UUID becomes a most/least pair.
fn entity_string_uuid(cx: &mut Cx) {
    cx.each(Ty::Entity, |e| {
        if let Some(uuid) = as_str(e.get("UUID"))
            && let Some((most, least)) = parse_uuid(&uuid)
        {
            remove(e, "UUID");
            set(e, "UUIDMost", Tag::Long(most));
            set(e, "UUIDLeast", Tag::Long(least));
        }
    });
}

// ------------------------------------------------------------------ 109

/// `EntityHealthFix`: the health is a float.
fn entity_health(cx: &mut Cx) {
    cx.each(Ty::Entity, |e| {
        let heal_f = e.get("HealF").and_then(as_number);
        let health = e.get("Health").and_then(as_number);
        let value = if let Some(n) = heal_f {
            remove(e, "HealF");
            n.float_value()
        } else if let Some(n) = health {
            n.float_value()
        } else {
            return;
        };
        set(e, "Health", Tag::Float(value.to_bits()));
    });
}

// ------------------------------------------------------------------ 110

/// `EntityHorseSaddleFix`: a saddled horse has a saddle item.
fn horse_saddle(cx: &mut Cx) {
    cx.entity("EntityHorse", |e| {
        if !matches!(e.get("SaddleItem"), Some(Tag::Compound(_))) && as_bool(e.get("Saddle"), false)
        {
            // `Saddle` stays: `Dynamic.remove` was not kept by the fix.
            set(
                e,
                "SaddleItem",
                Tag::compound([
                    ("id", string("minecraft:saddle")),
                    ("Count", Tag::Byte(1)),
                    ("Damage", Tag::Short(0)),
                ]),
            );
        }
    });
}

// ------------------------------------------------------------------ 111

/// `EntityPaintingItemFrameDirectionFix`.
fn painting_item_frame_direction(cx: &mut Cx) {
    const DIRECTIONS: [[i32; 3]; 4] = [[0, 0, 1], [-1, 0, 0], [0, 0, -1], [1, 0, 0]];
    for (id, item_frame) in [("Painting", false), ("ItemFrame", true)] {
        cx.entity(id, |e| {
            if e.get("Facing").and_then(as_number).is_some() {
                return;
            }
            let byte = |tag: Option<&Tag>| tag.and_then(as_number).map_or(0, |n| n.byte_value());
            let direction;
            if e.get("Direction").and_then(as_number).is_some() {
                direction = i32::from(byte(e.get("Direction"))) % 4;
                let Some(steps) = usize::try_from(direction).ok().map(|d| DIRECTIONS[d]) else {
                    return;
                };
                for (field, step) in ["TileX", "TileY", "TileZ"].into_iter().zip(steps) {
                    let value = as_int(e.get(field), 0).wrapping_add(step);
                    set(e, field, Tag::Int(value));
                }
                remove(e, "Direction");
                if item_frame && e.get("ItemRotation").and_then(as_number).is_some() {
                    let rotation = i32::from(byte(e.get("ItemRotation"))) * 2;
                    set(e, "ItemRotation", Tag::Byte(rotation as i8));
                }
            } else {
                direction = i32::from(byte(e.get("Dir"))) % 4;
                remove(e, "Dir");
            }
            set(e, "Facing", Tag::Byte(direction as i8));
        });
    }
}

// ------------------------------------------------------------------ 113

/// `EntityRedundantChanceTagsFix`: drop chances that are all zero are not saved.
fn redundant_chance_tags(cx: &mut Cx) {
    let is_zero_list = |tag: Option<&Tag>, size: usize| {
        let Some(tag) = tag.filter(|t| is_list(Some(t))) else {
            return false;
        };
        let values = stream(Some(tag));
        values.len() == size
            && values
                .iter()
                .all(|v| as_number(v).is_some_and(|n| n.float_value() == 0.0))
    };
    cx.each(Ty::Entity, |e| {
        if is_zero_list(e.get("HandDropChances"), 2) {
            remove(e, "HandDropChances");
        }
        if is_zero_list(e.get("ArmorDropChances"), 4) {
            remove(e, "ArmorDropChances");
        }
    });
}

// ------------------------------------------------------------------ 135

/// `EntityRidingToPassengersFix`: the vehicle is the root and carries its riders.
fn riding_to_passengers(cx: &mut Cx) {
    let (k, input) = (cx.k, cx.k - 1);
    cx.each(Ty::EntityTree, |tree| {
        if !entity_tree_known(tree, input) {
            return;
        }
        // The vehicle was fixed first: it has its passengers already.
        let Some(vehicle) = tree.get("Riding").filter(|v| entity_tree_known(v, input)) else {
            return;
        };
        let vehicle = vehicle.clone();
        let mut rider = tree.clone();
        remove(&mut rider, "Riding");
        // `Riding` is no field of the new type: the map the rider was read from gives it back.
        drop_typed(&mut rider, "Riding");
        *tree = add_passenger_to_top(vehicle, rider);
    });
    cx.each(Ty::Player, |p| reread_as(Ty::Player, k, p));
}

/// `EntityRidingToPassengersFix.addPassengerToTop`: the rider goes to the end of the chain.
fn add_passenger_to_top(mut root: Tag, passenger: Tag) -> Tag {
    let first = match root.get("Passengers") {
        Some(Tag::List(passengers)) => passengers.first().cloned(),
        _ => None,
    };
    let passenger = match first {
        Some(first) => add_passenger_to_top(first, passenger),
        None => passenger,
    };
    set(&mut root, "Passengers", Tag::List(vec![passenger]));
    root
}

// ------------------------------------------------------------------ 147

/// `EntityArmorStandSilentFix`.
fn armor_stand_silent(cx: &mut Cx) {
    cx.entity("ArmorStand", |e| {
        if as_bool(e.get("Silent"), false) && !as_bool(e.get("Marker"), false) {
            remove(e, "Silent");
        }
    });
}

// ------------------------------------------------------------------ 165

/// `LegacyComponentDataFixUtils.rewriteFromLenient`.
fn rewrite_from_lenient(text: &[u16]) -> Vec<u16> {
    use super::json::{self, Json};
    let empty = || json::text_component_json(&[]);
    if text.is_empty() || text == "null".encode_utf16().collect::<Vec<_>>().as_slice() {
        return empty();
    }
    let (first, last) = (text[0], text[text.len() - 1]);
    let (quote, open_brace, close_brace, open_bracket, close_bracket) = (
        u16::from(b'"'),
        u16::from(b'{'),
        u16::from(b'}'),
        u16::from(b'['),
        u16::from(b']'),
    );
    let wrapped = (first == quote && last == quote)
        || (first == open_brace && last == close_brace)
        || (first == open_bracket && last == close_bracket);
    if wrapped && let Some(parsed) = json::parse_lenient(text) {
        return match parsed {
            Json::Str(s) => json::text_component_json(&s),
            Json::Num(n) => json::text_component_json(&n.encode_utf16().collect::<Vec<_>>()),
            Json::Bool(b) => json::text_component_json(
                &(if b { "true" } else { "false" })
                    .encode_utf16()
                    .collect::<Vec<_>>(),
            ),
            other => json::to_stable_string(&other),
        };
    }
    json::text_component_json(text)
}

/// `SignTextStrictJsonFix`: the lines of a sign are strict JSON.
fn sign_text_strict_json(cx: &mut Cx) {
    cx.block_entity("Sign", |be| {
        for line in ["Text1", "Text2", "Text3", "Text4"] {
            if let Some(Tag::String(text)) = be.get(line) {
                let fixed = rewrite_from_lenient(&text.0);
                set(be, line, Tag::String(JavaString(fixed)));
            }
        }
    });
}

/// `WrittenBookPagesStrictJsonFix`: the pages of a written book are strict JSON.
fn written_book_pages_strict_json(cx: &mut Cx) {
    cx.item(
        |id| id == "minecraft:written_book",
        |s| {
            let Some(Tag::List(pages)) = s.get_mut("tag").and_then(|t| t.get_mut("pages")) else {
                return;
            };
            if pages.iter().all(|p| matches!(p, Tag::String(_))) {
                for page in pages {
                    if let Tag::String(text) = page {
                        *text = JavaString(rewrite_from_lenient(&text.0));
                    }
                }
            }
        },
    );
}

// ------------------------------------------------------------------ 502

/// `EntityZombieVillagerTypeFix`: `IsVillager` becomes a zombie type.
fn zombie_villager_type(cx: &mut Cx) {
    cx.entity("Zombie", |e| {
        if !as_bool(e.get("IsVillager"), false) {
            return;
        }
        if e.get("ZombieType").is_none() {
            let profession = as_int(e.get("VillagerProfession"), -1);
            // Vanilla picks a random profession for an invalid one: nothing to match there.
            let profession = if (0..6).contains(&profession) {
                profession
            } else {
                0
            };
            set(e, "ZombieType", Tag::Int(profession));
        }
        remove(e, "IsVillager");
    });
}

// ------------------------------------------------------------------ 700 to 703

/// `EntityElderGuardianSplitFix`.
fn elder_guardian_split(cx: &mut Cx) {
    cx.entity("Guardian", |e| {
        if as_bool(e.get("Elder"), false) {
            set(e, "id", string("ElderGuardian"));
        }
    });
}

/// `EntitySkeletonSplitFix`.
fn skeleton_split(cx: &mut Cx) {
    cx.entity("Skeleton", |e| match as_int(e.get("SkeletonType"), 0) {
        1 => set(e, "id", string("WitherSkeleton")),
        2 => set(e, "id", string("Stray")),
        _ => {}
    });
}

/// `EntityZombieSplitFix`.
fn zombie_split(cx: &mut Cx) {
    let k = cx.k;
    cx.entity("Zombie", |e| {
        let zombie_type = as_int(e.get("ZombieType"), 0);
        match zombie_type {
            1..=5 => {
                set(e, "id", string("ZombieVillager"));
                set(e, "Profession", Tag::Int(zombie_type - 1));
                reread(k, e);
            }
            6 => set(e, "id", string("Husk")),
            _ => {}
        }
        remove(e, "ZombieType");
    });
}

/// `EntityHorseSplitFix`.
fn horse_split(cx: &mut Cx) {
    let k = cx.k;
    cx.entity("EntityHorse", |e| {
        let name = match as_int(e.get("Type"), 0) {
            1 => "Donkey",
            2 => "Mule",
            3 => "ZombieHorse",
            4 => "SkeletonHorse",
            _ => "Horse",
        };
        set(e, "id", string(name));
        remove(e, "Type");
        reread(k, e);
    });
}

// ------------------------------------------------------------------ 704, 705

/// `BlockEntityIdFix`: block entity ids are namespaced. The item stacks are read
/// again first, with the hook of the new schema.
fn block_entity_ids(cx: &mut Cx) {
    let k = cx.k;
    cx.each(Ty::ItemStack, |s| items::add_names_at(s, k));
    cx.each(Ty::BlockEntity, |be| {
        if let Some(id) = as_str(be.get("id"))
            && let Some(new) = lookup_str(t::tables().block_entity_ids, &id)
        {
            set(be, "id", string(new));
        }
    });
}

/// `EntityIdFix`: entity ids are namespaced (the item stacks are read again first).
fn entity_ids(cx: &mut Cx) {
    let k = cx.k;
    cx.each(Ty::ItemStack, |s| items::add_names_at(s, k));
    cx.each(Ty::Entity, |e| {
        if let Some(id) = as_str(e.get("id"))
            && let Some(new) = lookup_str(t::tables().entity_ids, &id)
        {
            set(e, "id", string(new));
        }
    });
}

// ------------------------------------------------------------------ 804 to 813

/// `ItemBannerColorFix`: a banner item's damage is its base colour.
fn item_banner_color(cx: &mut Cx) {
    let input = cx.k - 1;
    cx.item(
        |id| id == "minecraft:banner",
        |s| {
            let Some(base) = s
                .get("tag")
                .and_then(|tag| tag.get("BlockEntityTag"))
                .filter(|be| block_entity_known(be, input))
                .and_then(|be| be.get("Base"))
                .and_then(as_number)
            else {
                return;
            };
            set(s, "Damage", Tag::Short((base.int_value() & 15) as i16));
            let pick_marker = Tag::compound([("Lore", Tag::List(vec![string("(+NBT")]))]);
            if s.get("tag")
                .and_then(|tag| tag.get("display"))
                .is_some_and(|display| *display == pick_marker)
            {
                return;
            }
            if let Some(be) = s
                .get_mut("tag")
                .and_then(|tag| tag.get_mut("BlockEntityTag"))
            {
                remove(be, "Base");
            }
        },
    );
}

/// `ItemWaterPotionFix`: potions without a type are water.
fn item_water_potion(cx: &mut Cx) {
    cx.item(
        |id| {
            matches!(
                id,
                "minecraft:potion"
                    | "minecraft:splash_potion"
                    | "minecraft:lingering_potion"
                    | "minecraft:tipped_arrow"
            )
        },
        |s| {
            if !matches!(s.get("tag"), Some(Tag::Compound(_))) {
                set(s, "tag", empty_map());
            }
            if let Some(tag) = s.get_mut("tag")
                && !matches!(tag.get("Potion"), Some(Tag::String(_)))
            {
                set(tag, "Potion", string("minecraft:water"));
            }
        },
    );
}

/// `EntityShulkerColorFix`: shulkers of before 1.11 are purple.
fn shulker_color(cx: &mut Cx) {
    cx.entity("minecraft:shulker", |e| {
        if e.get("Color").is_none() {
            set(e, "Color", Tag::Byte(10));
        }
    });
}

const SHULKER_BOX_COLORS: [&str; 16] = [
    "minecraft:white_shulker_box",
    "minecraft:orange_shulker_box",
    "minecraft:magenta_shulker_box",
    "minecraft:light_blue_shulker_box",
    "minecraft:yellow_shulker_box",
    "minecraft:lime_shulker_box",
    "minecraft:pink_shulker_box",
    "minecraft:gray_shulker_box",
    "minecraft:silver_shulker_box",
    "minecraft:cyan_shulker_box",
    "minecraft:purple_shulker_box",
    "minecraft:blue_shulker_box",
    "minecraft:brown_shulker_box",
    "minecraft:green_shulker_box",
    "minecraft:red_shulker_box",
    "minecraft:black_shulker_box",
];

/// `ItemShulkerBoxColorFix`: the colour of a shulker box item is in its name.
fn item_shulker_box_color(cx: &mut Cx) {
    let input = cx.k - 1;
    cx.item(
        |id| id == "minecraft:shulker_box",
        |s| {
            let Some(be) = s
                .get("tag")
                .and_then(|tag| tag.get("BlockEntityTag"))
                .filter(|be| block_entity_known(be, input))
            else {
                return;
            };
            // `Color` stays in the block entity: the fix does not keep `Dynamic.remove`.
            let color = as_int(be.get("Color"), 0);
            let name = SHULKER_BOX_COLORS[color.rem_euclid(16) as usize];
            set(s, "id", string(name));
        },
    );
}

// ------------------------------------------------------------------ 1125

/// `ChunkBedBlockEntityInjecterFix`: a bed block gets the block entity 1.12 gave it.
fn bed_block_entities(cx: &mut Cx) {
    cx.each(Ty::Chunk, |chunk| {
        let Some(level) = chunk.get_mut("Level") else {
            return;
        };
        let chunk_x = as_int(level.get("xPos"), 0);
        let chunk_z = as_int(level.get("zPos"), 0);
        let mut beds = Vec::new();
        if let Some(Tag::List(sections)) = level.get("Sections") {
            for section in sections {
                let y = as_int(section.get("Y"), 0);
                for (p, block) in as_int_stream(section.get("Blocks"))
                    .unwrap_or_default()
                    .into_iter()
                    .enumerate()
                {
                    if (block & 0xFF) << 4 == 416 {
                        let (x, yy, z) = (p as i32 & 15, p as i32 >> 8 & 15, p as i32 >> 4 & 15);
                        let mut bed = Tag::compound([
                            ("id", string("minecraft:bed")),
                            ("x", Tag::Int(x + (chunk_x << 4))),
                            ("y", Tag::Int(yy + (y << 4))),
                            ("z", Tag::Int(z + (chunk_z << 4))),
                            ("color", Tag::Short(14)),
                        ]);
                        mark_read(&mut bed);
                        beds.push(bed);
                    }
                }
            }
        }
        if beds.is_empty() {
            return;
        }
        if !matches!(level.get("TileEntities"), Some(Tag::List(_))) {
            set(level, "TileEntities", Tag::List(Vec::new()));
        }
        if let Some(Tag::List(entities)) = level.get_mut("TileEntities") {
            entities.extend(beds);
        }
    });
}
