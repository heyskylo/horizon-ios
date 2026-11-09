//! Item stacks: the V705 read hook (`ADD_NAMES`), `ItemStackComponentizationFix`
//! (1.20.5) and `ParticleUnflatteningFix`.
use super::dynamic::{
    as_bool, as_bool_opt, as_double, as_int, as_long, as_number, as_str, as_string, empty_map,
    int_list, is_empty_map, namespaced, remove, rename, set, stream, string,
};
use super::json;
use crate::nbt::{JavaString, Tag};

/// `V704.ITEM_TO_BLOCKENTITY`.
const ITEM_TO_BLOCK_ENTITY: &[(&str, &str)] = &[
    ("minecraft:furnace", "minecraft:furnace"),
    ("minecraft:lit_furnace", "minecraft:furnace"),
    ("minecraft:chest", "minecraft:chest"),
    ("minecraft:trapped_chest", "minecraft:chest"),
    ("minecraft:ender_chest", "minecraft:ender_chest"),
    ("minecraft:jukebox", "minecraft:jukebox"),
    ("minecraft:dispenser", "minecraft:dispenser"),
    ("minecraft:dropper", "minecraft:dropper"),
    ("minecraft:sign", "minecraft:sign"),
    ("minecraft:mob_spawner", "minecraft:mob_spawner"),
    ("minecraft:spawner", "minecraft:mob_spawner"),
    ("minecraft:noteblock", "minecraft:noteblock"),
    ("minecraft:brewing_stand", "minecraft:brewing_stand"),
    ("minecraft:enhanting_table", "minecraft:enchanting_table"),
    ("minecraft:command_block", "minecraft:command_block"),
    ("minecraft:beacon", "minecraft:beacon"),
    ("minecraft:skull", "minecraft:skull"),
    ("minecraft:daylight_detector", "minecraft:daylight_detector"),
    ("minecraft:hopper", "minecraft:hopper"),
    ("minecraft:banner", "minecraft:banner"),
    ("minecraft:flower_pot", "minecraft:flower_pot"),
    (
        "minecraft:repeating_command_block",
        "minecraft:command_block",
    ),
    ("minecraft:chain_command_block", "minecraft:command_block"),
    ("minecraft:shulker_box", "minecraft:shulker_box"),
    ("minecraft:white_shulker_box", "minecraft:shulker_box"),
    ("minecraft:orange_shulker_box", "minecraft:shulker_box"),
    ("minecraft:magenta_shulker_box", "minecraft:shulker_box"),
    ("minecraft:light_blue_shulker_box", "minecraft:shulker_box"),
    ("minecraft:yellow_shulker_box", "minecraft:shulker_box"),
    ("minecraft:lime_shulker_box", "minecraft:shulker_box"),
    ("minecraft:pink_shulker_box", "minecraft:shulker_box"),
    ("minecraft:gray_shulker_box", "minecraft:shulker_box"),
    ("minecraft:silver_shulker_box", "minecraft:shulker_box"),
    ("minecraft:cyan_shulker_box", "minecraft:shulker_box"),
    ("minecraft:purple_shulker_box", "minecraft:shulker_box"),
    ("minecraft:blue_shulker_box", "minecraft:shulker_box"),
    ("minecraft:brown_shulker_box", "minecraft:shulker_box"),
    ("minecraft:green_shulker_box", "minecraft:shulker_box"),
    ("minecraft:red_shulker_box", "minecraft:shulker_box"),
    ("minecraft:black_shulker_box", "minecraft:shulker_box"),
    ("minecraft:bed", "minecraft:bed"),
    ("minecraft:light_gray_shulker_box", "minecraft:shulker_box"),
    ("minecraft:white_banner", "minecraft:banner"),
    ("minecraft:orange_banner", "minecraft:banner"),
    ("minecraft:magenta_banner", "minecraft:banner"),
    ("minecraft:light_blue_banner", "minecraft:banner"),
    ("minecraft:yellow_banner", "minecraft:banner"),
    ("minecraft:lime_banner", "minecraft:banner"),
    ("minecraft:pink_banner", "minecraft:banner"),
    ("minecraft:gray_banner", "minecraft:banner"),
    ("minecraft:silver_banner", "minecraft:banner"),
    ("minecraft:light_gray_banner", "minecraft:banner"),
    ("minecraft:cyan_banner", "minecraft:banner"),
    ("minecraft:purple_banner", "minecraft:banner"),
    ("minecraft:blue_banner", "minecraft:banner"),
    ("minecraft:brown_banner", "minecraft:banner"),
    ("minecraft:green_banner", "minecraft:banner"),
    ("minecraft:red_banner", "minecraft:banner"),
    ("minecraft:black_banner", "minecraft:banner"),
    ("minecraft:standing_sign", "minecraft:sign"),
    ("minecraft:wall_sign", "minecraft:sign"),
    ("minecraft:piston_head", "minecraft:piston"),
    (
        "minecraft:daylight_detector_inverted",
        "minecraft:daylight_detector",
    ),
    ("minecraft:unpowered_comparator", "minecraft:comparator"),
    ("minecraft:powered_comparator", "minecraft:comparator"),
    ("minecraft:wall_banner", "minecraft:banner"),
    ("minecraft:standing_banner", "minecraft:banner"),
    ("minecraft:structure_block", "minecraft:structure_block"),
    ("minecraft:end_portal", "minecraft:end_portal"),
    ("minecraft:end_gateway", "minecraft:end_gateway"),
    ("minecraft:shield", "minecraft:banner"),
    ("minecraft:white_bed", "minecraft:bed"),
    ("minecraft:orange_bed", "minecraft:bed"),
    ("minecraft:magenta_bed", "minecraft:bed"),
    ("minecraft:light_blue_bed", "minecraft:bed"),
    ("minecraft:yellow_bed", "minecraft:bed"),
    ("minecraft:lime_bed", "minecraft:bed"),
    ("minecraft:pink_bed", "minecraft:bed"),
    ("minecraft:gray_bed", "minecraft:bed"),
    ("minecraft:silver_bed", "minecraft:bed"),
    ("minecraft:light_gray_bed", "minecraft:bed"),
    ("minecraft:cyan_bed", "minecraft:bed"),
    ("minecraft:purple_bed", "minecraft:bed"),
    ("minecraft:blue_bed", "minecraft:bed"),
    ("minecraft:brown_bed", "minecraft:bed"),
    ("minecraft:green_bed", "minecraft:bed"),
    ("minecraft:red_bed", "minecraft:bed"),
    ("minecraft:black_bed", "minecraft:bed"),
    ("minecraft:oak_sign", "minecraft:sign"),
    ("minecraft:spruce_sign", "minecraft:sign"),
    ("minecraft:birch_sign", "minecraft:sign"),
    ("minecraft:jungle_sign", "minecraft:sign"),
    ("minecraft:acacia_sign", "minecraft:sign"),
    ("minecraft:dark_oak_sign", "minecraft:sign"),
    ("minecraft:crimson_sign", "minecraft:sign"),
    ("minecraft:warped_sign", "minecraft:sign"),
    ("minecraft:skeleton_skull", "minecraft:skull"),
    ("minecraft:wither_skeleton_skull", "minecraft:skull"),
    ("minecraft:zombie_head", "minecraft:skull"),
    ("minecraft:player_head", "minecraft:skull"),
    ("minecraft:creeper_head", "minecraft:skull"),
    ("minecraft:dragon_head", "minecraft:skull"),
    ("minecraft:barrel", "minecraft:barrel"),
    ("minecraft:conduit", "minecraft:conduit"),
    ("minecraft:smoker", "minecraft:smoker"),
    ("minecraft:blast_furnace", "minecraft:blast_furnace"),
    ("minecraft:lectern", "minecraft:lectern"),
    ("minecraft:bell", "minecraft:bell"),
    ("minecraft:jigsaw", "minecraft:jigsaw"),
    ("minecraft:campfire", "minecraft:campfire"),
    ("minecraft:bee_nest", "minecraft:beehive"),
    ("minecraft:beehive", "minecraft:beehive"),
    ("minecraft:sculk_sensor", "minecraft:sculk_sensor"),
    ("minecraft:decorated_pot", "minecraft:decorated_pot"),
    ("minecraft:crafter", "minecraft:crafter"),
];

/// `V705.ITEM_TO_ENTITY`: spawn eggs and the items that place an entity.
fn item_to_entity(item: &str) -> Option<String> {
    if let Some(mob) = item
        .strip_suffix("_spawn_egg")
        .and_then(|m| m.strip_prefix("minecraft:"))
        && SPAWN_EGG_MOBS.contains(&mob)
    {
        return Some(format!("minecraft:{mob}"));
    }
    match item {
        "minecraft:armor_stand"
        | "minecraft:painting"
        | "minecraft:item_frame"
        | "minecraft:minecart"
        | "minecraft:chest_minecart"
        | "minecraft:furnace_minecart"
        | "minecraft:tnt_minecart"
        | "minecraft:hopper_minecart"
        | "minecraft:bamboo_raft"
        | "minecraft:bamboo_chest_raft" => Some(item.to_owned()),
        "minecraft:boat" => Some("minecraft:oak_boat".into()),
        _ => {
            let wood = item.strip_prefix("minecraft:")?;
            let wood = wood
                .strip_suffix("_chest_boat")
                .or_else(|| wood.strip_suffix("_boat"))?;
            BOAT_WOODS.contains(&wood).then(|| item.to_owned())
        }
    }
}
const BOAT_WOODS: [&str; 8] = [
    "oak", "spruce", "birch", "jungle", "acacia", "cherry", "dark_oak", "mangrove",
];
const SPAWN_EGG_MOBS: &[&str] = &[
    "armadillo",
    "allay",
    "axolotl",
    "bat",
    "bee",
    "blaze",
    "bogged",
    "breeze",
    "cat",
    "camel",
    "cave_spider",
    "chicken",
    "cod",
    "cow",
    "creeper",
    "dolphin",
    "donkey",
    "drowned",
    "elder_guardian",
    "ender_dragon",
    "enderman",
    "endermite",
    "evoker",
    "fox",
    "frog",
    "ghast",
    "glow_squid",
    "goat",
    "guardian",
    "hoglin",
    "horse",
    "husk",
    "iron_golem",
    "llama",
    "magma_cube",
    "mooshroom",
    "mule",
    "ocelot",
    "panda",
    "parrot",
    "phantom",
    "pig",
    "piglin",
    "piglin_brute",
    "pillager",
    "polar_bear",
    "pufferfish",
    "rabbit",
    "ravager",
    "salmon",
    "sheep",
    "shulker",
    "silverfish",
    "skeleton",
    "skeleton_horse",
    "slime",
    "sniffer",
    "snow_golem",
    "spider",
    "squid",
    "stray",
    "strider",
    "tadpole",
    "trader_llama",
    "tropical_fish",
    "turtle",
    "vex",
    "villager",
    "vindicator",
    "wandering_trader",
    "warden",
    "witch",
    "wither",
    "wither_skeleton",
    "wolf",
    "zoglin",
    "zombie",
    "zombie_horse",
    "zombie_villager",
    "zombified_piglin",
];

/// The read hook of item stacks in the schema at `k` (before 1.20.5): V99's tables below V704,
/// V704's block entities with V99's entities up to V705, V705's from then on.
pub(super) fn add_names_at(stack: &mut Tag, k: super::schema::Key) {
    use super::schema::key;
    if k >= key(705, 0) {
        return add_names(stack);
    }
    let item = namespaced(&as_str(stack.get("id")).unwrap_or_else(|| "minecraft:air".into()));
    let Some(tag) = stack.get_mut("tag") else {
        return;
    };
    if let Some(be) = tag.get_mut("BlockEntityTag")
        && item != "minecraft:air"
    {
        let found = if k >= key(704, 0) {
            ITEM_TO_BLOCK_ENTITY.iter().find(|(i, _)| *i == item)
        } else {
            super::tables::tables()
                .item_to_block_entity_v99
                .iter()
                .find(|(i, _)| *i == item)
        };
        if let Some((_, id)) = found {
            set(be, "id", string(id));
        }
    }
    if let Some(e) = tag.get_mut("EntityTag")
        && e.get("id").is_none()
    {
        let id = match item.as_str() {
            "minecraft:armor_stand" => Some("ArmorStand"),
            "minecraft:painting" => Some("Painting"),
            _ => None,
        };
        if let Some(id) = id {
            set(e, "id", string(id));
        }
    }
}

/// `V99.addNames` with V704/V705's tables: the read hook of item stacks before 1.20.5.
pub(super) fn add_names(stack: &mut Tag) {
    let item = namespaced(&as_str(stack.get("id")).unwrap_or_else(|| "minecraft:air".into()));
    let Some(tag) = stack.get_mut("tag") else {
        return;
    };
    if let Some(be) = tag.get_mut("BlockEntityTag")
        && item != "minecraft:air"
        && let Some((_, id)) = ITEM_TO_BLOCK_ENTITY.iter().find(|(i, _)| *i == item)
    {
        set(be, "id", string(id));
    }
    if let Some(e) = tag.get_mut("EntityTag")
        && e.get("id").is_none()
        && let Some(id) = item_to_entity(&item)
    {
        set(e, "id", string(&id));
    }
}

/// `ItemStackComponentizationFix.ItemStackData`.
struct Stack {
    item: String,
    count: i32,
    components: Tag,
    remainder: Tag,
    tag: Tag,
}

impl Stack {
    fn read(v: &Tag) -> Option<Self> {
        let item = as_str(v.get("id"))?;
        let count = as_number(v.get("Count")?)?.int_value();
        let mut remainder = v.clone();
        remove(&mut remainder, "id");
        remove(&mut remainder, "Count");
        let tag = super::dynamic::or_empty_map(remainder.get("tag"));
        remove(&mut remainder, "tag");
        Some(Self {
            item: namespaced(&item),
            count,
            components: empty_map(),
            remainder,
            tag,
        })
    }
    fn remove_tag(&mut self, key: &str) -> Option<Tag> {
        remove(&mut self.tag, key)
    }
    fn set_component(&mut self, ty: &str, value: Tag) {
        set(&mut self.components, ty, value);
    }
    fn set_component_opt(&mut self, ty: &str, value: Option<Tag>) {
        if let Some(v) = value {
            self.set_component(ty, v);
        }
    }
    fn move_tag_into(&mut self, from: &str, target: &mut Tag, to: &str) {
        if let Some(v) = self.remove_tag(from) {
            set(target, to, v);
        }
    }
    fn move_to_component_default(&mut self, key: &str, ty: &str, default: &Tag) {
        if let Some(v) = self.remove_tag(key)
            && v != *default
        {
            self.set_component(ty, v);
        }
    }
    fn move_to_component(&mut self, key: &str, ty: &str) {
        if let Some(v) = self.remove_tag(key) {
            self.set_component(ty, v);
        }
    }
    /// `fixSubTag`.
    fn fix_sub_tag(
        &mut self,
        key: &str,
        skip_missing: bool,
        f: impl FnOnce(&mut Self, Tag) -> Tag,
    ) {
        let value = self.tag.get(key).cloned();
        if skip_missing && value.is_none() {
            return;
        }
        let map = value.unwrap_or_else(empty_map);
        let map = f(self, map);
        if is_empty_map(&map) {
            remove(&mut self.tag, key);
        } else {
            set(&mut self.tag, key, map);
        }
    }
    fn is(&self, id: &str) -> bool {
        self.item == id
    }
    fn write(mut self) -> Tag {
        let mut result =
            Tag::compound([("id", string(&self.item)), ("count", Tag::Int(self.count))]);
        if !is_empty_map(&self.tag) {
            let tag = std::mem::replace(&mut self.tag, empty_map());
            self.set_component("minecraft:custom_data", tag);
        }
        if !is_empty_map(&self.components) {
            set(&mut result, "components", self.components);
        }
        // mergeRemainder: the remainder's fields under the stack's own.
        let mut out = self.remainder;
        if let (Tag::Compound(o), Tag::Compound(r)) = (&mut out, result) {
            o.extend(r);
            out
        } else {
            out
        }
    }
}

const BUCKETED_MOBS: [&str; 6] = [
    "minecraft:pufferfish_bucket",
    "minecraft:salmon_bucket",
    "minecraft:cod_bucket",
    "minecraft:tropical_fish_bucket",
    "minecraft:axolotl_bucket",
    "minecraft:tadpole_bucket",
];
const BUCKETED_MOB_TAGS: [&str; 10] = [
    "NoAI",
    "Silent",
    "NoGravity",
    "Glowing",
    "Invulnerable",
    "Health",
    "Age",
    "Variant",
    "HuntingCooldown",
    "BucketVariantTag",
];
const BOOLEAN_BLOCK_STATE_PROPERTIES: &[&str] = &[
    "attached",
    "bottom",
    "conditional",
    "disarmed",
    "drag",
    "enabled",
    "extended",
    "eye",
    "falling",
    "hanging",
    "has_bottle_0",
    "has_bottle_1",
    "has_bottle_2",
    "has_record",
    "has_book",
    "inverted",
    "in_wall",
    "lit",
    "locked",
    "occupied",
    "open",
    "persistent",
    "powered",
    "short",
    "signal_fire",
    "snowy",
    "triggered",
    "unstable",
    "waterlogged",
    "berries",
    "bloom",
    "shrieking",
    "can_summon",
    "up",
    "down",
    "north",
    "east",
    "south",
    "west",
    "slot_0_occupied",
    "slot_1_occupied",
    "slot_2_occupied",
    "slot_3_occupied",
    "slot_4_occupied",
    "slot_5_occupied",
    "cracked",
    "crafting",
];

/// The fix for one item stack (`writeFixAndRead` over ITEM_STACK).
pub(super) fn componentize(v: &mut Tag) {
    if let Some(mut s) = Stack::read(v) {
        fix_stack(&mut s);
        *v = s.write();
    }
}

fn fix_stack(s: &mut Stack) {
    let hide_flags = as_int(s.remove_tag("HideFlags").as_ref(), 0);
    s.move_to_component_default("Damage", "minecraft:damage", &Tag::Int(0));
    s.move_to_component_default("RepairCost", "minecraft:repair_cost", &Tag::Int(0));
    s.move_to_component("CustomModelData", "minecraft:custom_model_data");
    if let Some(b) = s.remove_tag("BlockStateTag") {
        s.set_component("minecraft:block_state", fix_block_state_tag(b));
    }
    s.move_to_component("EntityTag", "minecraft:entity_data");
    s.fix_sub_tag("BlockEntityTag", false, |s, be| {
        let id = namespaced(&as_string(be.get("id"), ""));
        let be = fix_block_entity_tag(s, be, &id);
        let mut without_id = be.clone();
        remove(&mut without_id, "id");
        if is_empty_map(&without_id) {
            without_id
        } else {
            be
        }
    });
    s.move_to_component("BlockEntityTag", "minecraft:block_entity_data");
    if as_bool(s.remove_tag("Unbreakable").as_ref(), false) {
        let mut c = empty_map();
        if hide_flags & 4 != 0 {
            set(&mut c, "show_in_tooltip", Tag::Byte(0));
        }
        s.set_component("minecraft:unbreakable", c);
    }
    fix_enchantments(
        s,
        "Enchantments",
        "minecraft:enchantments",
        hide_flags & 1 != 0,
    );
    if s.is("minecraft:enchanted_book") {
        fix_enchantments(
            s,
            "StoredEnchantments",
            "minecraft:stored_enchantments",
            hide_flags & 32 != 0,
        );
    }
    s.fix_sub_tag("display", false, |s, d| fix_display(s, d, hide_flags));
    fix_block_predicates(s, "CanDestroy", "minecraft:can_break", hide_flags & 8 != 0);
    fix_block_predicates(
        s,
        "CanPlaceOn",
        "minecraft:can_place_on",
        hide_flags & 16 != 0,
    );
    fix_attribute_modifiers(s, hide_flags);
    if let Some(mut trim) = s.remove_tag("Trim") {
        if hide_flags & 128 != 0 {
            set(&mut trim, "show_in_tooltip", Tag::Byte(0));
        }
        s.set_component("minecraft:trim", trim);
    }
    if hide_flags & 32 != 0 {
        s.set_component("minecraft:hide_additional_tooltip", empty_map());
    }
    if s.is("minecraft:crossbow") {
        s.remove_tag("Charged");
        s.move_to_component_default(
            "ChargedProjectiles",
            "minecraft:charged_projectiles",
            &Tag::List(vec![]),
        );
    }
    if s.is("minecraft:bundle") {
        s.move_to_component_default("Items", "minecraft:bundle_contents", &Tag::List(vec![]));
    }
    if s.is("minecraft:filled_map") {
        s.move_to_component("map", "minecraft:map_id");
        let decorations = stream(s.remove_tag("Decorations").as_ref());
        let mut map = crate::nbt::Compound::new();
        for d in decorations {
            let (id, value) = fix_map_decoration(&d);
            // Collectors.toMap(..., (first, second) -> first): the first stays.
            map.entry(id).or_insert(value);
        }
        if !map.is_empty() {
            s.set_component("minecraft:map_decorations", Tag::Compound(map));
        }
    }
    if matches!(
        s.item.as_str(),
        "minecraft:potion"
            | "minecraft:splash_potion"
            | "minecraft:lingering_potion"
            | "minecraft:tipped_arrow"
    ) {
        let mut c = empty_map();
        if let Some(p) = as_str(s.remove_tag("Potion").as_ref()).filter(|p| p != "minecraft:empty")
        {
            set(&mut c, "potion", string(&p));
        }
        s.move_tag_into("CustomPotionColor", &mut c, "custom_color");
        s.move_tag_into("custom_potion_effects", &mut c, "custom_effects");
        if !is_empty_map(&c) {
            s.set_component("minecraft:potion_contents", c);
        }
    }
    if s.is("minecraft:writable_book")
        && let Some(pages) = fix_book_pages(s)
    {
        s.set_component(
            "minecraft:writable_book_content",
            Tag::compound([("pages", pages)]),
        );
    }
    if s.is("minecraft:written_book") {
        let pages = fix_book_pages(s);
        let title = as_string(s.remove_tag("title").as_ref(), "");
        let filtered_title = as_str(s.remove_tag("filtered_title").as_ref());
        let mut c = empty_map();
        set(
            &mut c,
            "title",
            filtered_text(&title, filtered_title.as_deref()),
        );
        s.move_tag_into("author", &mut c, "author");
        s.move_tag_into("resolved", &mut c, "resolved");
        s.move_tag_into("generation", &mut c, "generation");
        if let Some(p) = pages {
            set(&mut c, "pages", p);
        }
        s.set_component("minecraft:written_book_content", c);
    }
    if s.is("minecraft:suspicious_stew") {
        s.move_to_component("effects", "minecraft:suspicious_stew_effects");
    }
    if s.is("minecraft:debug_stick") {
        s.move_to_component("DebugProperty", "minecraft:debug_stick_state");
    }
    if BUCKETED_MOBS.contains(&s.item.as_str()) {
        let mut data = empty_map();
        for k in BUCKETED_MOB_TAGS {
            s.move_tag_into(k, &mut data, k);
        }
        if !is_empty_map(&data) {
            s.set_component("minecraft:bucket_entity_data", data);
        }
    }
    if s.is("minecraft:goat_horn") {
        s.move_to_component("instrument", "minecraft:instrument");
    }
    if s.is("minecraft:knowledge_book") {
        s.move_to_component("Recipes", "minecraft:recipes");
    }
    if s.is("minecraft:compass") {
        fix_lodestone_tracker(s);
    }
    if s.is("minecraft:firework_rocket") {
        s.fix_sub_tag("Fireworks", true, |s, mut f| {
            let explosions: Vec<Tag> = stream(f.get("Explosions"))
                .into_iter()
                .map(fix_firework_explosion)
                .collect();
            let flight = as_int(f.get("Flight"), 0);
            s.set_component(
                "minecraft:fireworks",
                Tag::compound([
                    ("explosions", Tag::List(explosions)),
                    ("flight_duration", Tag::Byte(flight as i8)),
                ]),
            );
            remove(&mut f, "Explosions");
            remove(&mut f, "Flight");
            f
        });
    }
    if s.is("minecraft:firework_star") {
        s.fix_sub_tag("Explosion", true, |s, mut e| {
            s.set_component(
                "minecraft:firework_explosion",
                fix_firework_explosion(e.clone()),
            );
            for k in ["Type", "Colors", "FadeColors", "Trail", "Flicker"] {
                remove(&mut e, k);
            }
            e
        });
    }
    if s.is("minecraft:player_head")
        && let Some(owner) = s.remove_tag("SkullOwner")
    {
        s.set_component("minecraft:profile", fix_profile(&owner));
    }
}

fn fix_block_state_tag(v: Tag) -> Tag {
    let Tag::Compound(map) = v else {
        return v;
    };
    let mut out = crate::nbt::Compound::new();
    for (k, value) in map {
        let name = k.to_string_lossy();
        if BOOLEAN_BLOCK_STATE_PROPERTIES.contains(&name.as_str())
            && let Some(b) = as_bool_opt(Some(&value))
        {
            out.insert(k, string(if b { "true" } else { "false" }));
            continue;
        }
        let value = match as_number(&value) {
            Some(n) => string(&n.java_string()),
            None => value,
        };
        out.insert(k, value);
    }
    Tag::Compound(out)
}

fn is_strictly_valid_json(v: &Tag) -> bool {
    matches!(v, Tag::String(s) if json::parse_strict(&s.0).is_some())
}

fn fix_display(s: &mut Stack, mut d: Tag, hide_flags: i32) -> Tag {
    if let Some(name) = d.get("Name").filter(|n| is_strictly_valid_json(n)) {
        s.set_component("minecraft:custom_name", name.clone());
    }
    if d.get("Lore").is_some() {
        let lore = stream(d.get("Lore"))
            .into_iter()
            .filter(is_strictly_valid_json)
            .collect();
        s.set_component("minecraft:lore", Tag::List(lore));
    }
    let color = d.get("color").and_then(as_number).map(|n| n.int_value());
    let hide_dye = hide_flags & 64 != 0;
    if color.is_some() || hide_dye {
        let mut c = Tag::compound([("rgb", Tag::Int(color.unwrap_or(10_511_680)))]);
        if hide_dye {
            set(&mut c, "show_in_tooltip", Tag::Byte(0));
        }
        s.set_component("minecraft:dyed_color", c);
    }
    if let Some(loc) = as_str(d.get("LocName")) {
        s.set_component(
            "minecraft:item_name",
            Tag::String(JavaString(json::translatable_component_json(&loc))),
        );
    }
    if s.is("minecraft:filled_map") {
        let color = d.get("MapColor").cloned();
        s.set_component_opt("minecraft:map_color", color);
        remove(&mut d, "MapColor");
    }
    for k in ["Name", "Lore", "color", "LocName"] {
        remove(&mut d, k);
    }
    d
}

fn fix_block_entity_tag(s: &mut Stack, mut be: Tag, id: &str) -> Tag {
    let lock = be.get("Lock").cloned();
    s.set_component_opt("minecraft:lock", lock);
    remove(&mut be, "Lock");
    if let Some(loot) = be.get("LootTable").cloned() {
        let mut c = Tag::compound([("loot_table", loot)]);
        let seed = as_long(be.get("LootTableSeed"), 0);
        if seed != 0 {
            set(&mut c, "seed", Tag::Long(seed));
        }
        s.set_component("minecraft:container_loot", c);
        remove(&mut be, "LootTable");
        remove(&mut be, "LootTableSeed");
    }
    match id {
        "minecraft:skull" => {
            let v = be.get("note_block_sound").cloned();
            s.set_component_opt("minecraft:note_block_sound", v);
            remove(&mut be, "note_block_sound");
        }
        "minecraft:decorated_pot" => {
            let sherds = be.get("sherds").cloned();
            s.set_component_opt("minecraft:pot_decorations", sherds);
            if let Some(item) = be.get("item").cloned() {
                s.set_component(
                    "minecraft:container",
                    Tag::List(vec![Tag::compound([("slot", Tag::Int(0)), ("item", item)])]),
                );
            }
            remove(&mut be, "sherds");
            remove(&mut be, "item");
        }
        "minecraft:banner" => {
            let patterns = be.get("patterns").cloned();
            s.set_component_opt("minecraft:banner_patterns", patterns);
            if let Some(base) = be.get("Base").and_then(as_number) {
                s.set_component(
                    "minecraft:base_color",
                    string(super::fixes_a::dye_color(base.int_value())),
                );
            }
            remove(&mut be, "patterns");
            remove(&mut be, "Base");
        }
        "minecraft:shulker_box"
        | "minecraft:chest"
        | "minecraft:trapped_chest"
        | "minecraft:furnace"
        | "minecraft:ender_chest"
        | "minecraft:dispenser"
        | "minecraft:dropper"
        | "minecraft:brewing_stand"
        | "minecraft:hopper"
        | "minecraft:barrel"
        | "minecraft:smoker"
        | "minecraft:blast_furnace"
        | "minecraft:campfire"
        | "minecraft:chiseled_bookshelf"
        | "minecraft:crafter" => {
            let items: Vec<Tag> = stream(be.get("Items"))
                .into_iter()
                .map(|mut item| {
                    let slot = item
                        .get("Slot")
                        .and_then(as_number)
                        .map_or(0, |n| n.byte_value()) as u8;
                    remove(&mut item, "Slot");
                    Tag::compound([("slot", Tag::Int(i32::from(slot))), ("item", item)])
                })
                .collect();
            if !items.is_empty() {
                s.set_component("minecraft:container", Tag::List(items));
            }
            remove(&mut be, "Items");
        }
        "minecraft:beehive" => {
            let bees = be.get("bees").cloned();
            s.set_component_opt("minecraft:bees", bees);
            remove(&mut be, "bees");
        }
        _ => {}
    }
    be
}

fn fix_enchantments(s: &mut Stack, key: &str, ty: &str, hide: bool) {
    let raw = s.remove_tag(key);
    let mut enchantments: Vec<(String, i32)> = Vec::new();
    for e in stream(raw.as_ref()) {
        if let (Some(id), Some(level)) = (as_str(e.get("id")), e.get("lvl").and_then(as_number)) {
            let level = level.int_value().clamp(0, 255);
            if level > 0 {
                enchantments.push((id, level));
            }
        }
    }
    if !enchantments.is_empty() || hide {
        let mut levels = empty_map();
        for (id, level) in &enchantments {
            set(&mut levels, id, Tag::Int(*level));
        }
        let mut c = Tag::compound([("levels", levels)]);
        if hide {
            set(&mut c, "show_in_tooltip", Tag::Byte(0));
        }
        s.set_component(ty, c);
    }
    if raw.is_some() && enchantments.is_empty() {
        s.set_component("minecraft:enchantment_glint_override", Tag::Byte(1));
    }
}

fn fix_block_predicates(s: &mut Stack, tag: &str, component: &str, hide: bool) {
    let Some(old) = s.remove_tag(tag) else {
        return;
    };
    let predicates = stream(Some(&old))
        .into_iter()
        .map(|v| match as_str(Some(&v)) {
            Some(text) => fix_block_predicate(&text),
            None => v,
        })
        .collect();
    let mut c = Tag::compound([("predicates", Tag::List(predicates))]);
    if hide {
        set(&mut c, "show_in_tooltip", Tag::Byte(0));
    }
    s.set_component(component, c);
}

fn fix_block_predicate(text: &str) -> Tag {
    let start_props = text.find('[');
    let start_nbt = text.find('{');
    let mut end = text.len();
    if let Some(p) = start_props {
        end = p;
    }
    if let Some(n) = start_nbt {
        end = end.min(n);
    }
    let mut predicate = Tag::compound([("blocks", string(text[..end].trim()))]);
    let end_props = text.find(']');
    if let (Some(start), Some(stop)) = (start_props, end_props) {
        let mut props = empty_map();
        // Guava Splitter.on(','): String.substring throws when ] is before [.
        if start < stop {
            for property in text[start + 1..stop].split(',') {
                if let Some(eq) = property.find('=') {
                    set(
                        &mut props,
                        property[..eq].trim(),
                        string(property[eq + 1..].trim()),
                    );
                }
            }
        }
        set(&mut predicate, "state", props);
    }
    let end_nbt = text.find('}');
    if let (Some(start), Some(stop)) = (start_nbt, end_nbt)
        && start <= stop
    {
        set(&mut predicate, "nbt", string(&text[start..=stop]));
    }
    predicate
}

fn fix_attribute_modifiers(s: &mut Stack, hide_flags: i32) {
    let Some(field) = s.remove_tag("AttributeModifiers") else {
        return;
    };
    let modifiers = stream(Some(&field))
        .into_iter()
        .map(|m| fix_attribute_modifier(&m))
        .collect();
    let mut c = Tag::compound([("modifiers", Tag::List(modifiers))]);
    if hide_flags & 2 != 0 {
        set(&mut c, "show_in_tooltip", Tag::Byte(0));
    }
    s.set_component("minecraft:attribute_modifiers", c);
}

fn fix_attribute_modifier(input: &Tag) -> Tag {
    let mut r = Tag::compound([
        ("name", string("")),
        ("amount", Tag::Double(0f64.to_bits())),
        ("operation", string("add_value")),
    ]);
    super::dynamic::copy_field(input, "AttributeName", &mut r, "type");
    super::dynamic::copy_field(input, "Slot", &mut r, "slot");
    super::dynamic::copy_field(input, "UUID", &mut r, "uuid");
    super::dynamic::copy_field(input, "Name", &mut r, "name");
    super::dynamic::copy_field(input, "Amount", &mut r, "amount");
    if let Some(op) = input.get("Operation") {
        let name = match as_int(Some(op), 0) {
            1 => "add_multiplied_base",
            2 => "add_multiplied_total",
            _ => "add_value",
        };
        set(&mut r, "operation", string(name));
    }
    r
}

fn fix_map_decoration(d: &Tag) -> (JavaString, Tag) {
    let id = match d.get("id") {
        Some(Tag::String(s)) => s.clone(),
        // A non-string key fails the map; vanilla throws on it.
        // createMap throws on a key that is not a string; such a stack is left as it was.
        Some(_) => JavaString::default(),
        None => JavaString::default(),
    };
    let ty = match as_int(d.get("type"), 0) {
        1 => "frame",
        2 => "red_marker",
        3 => "blue_marker",
        4 => "target_x",
        5 => "target_point",
        6 => "player_off_map",
        7 => "player_off_limits",
        8 => "mansion",
        9 => "monument",
        10 => "banner_white",
        11 => "banner_orange",
        12 => "banner_magenta",
        13 => "banner_light_blue",
        14 => "banner_yellow",
        15 => "banner_lime",
        16 => "banner_pink",
        17 => "banner_gray",
        18 => "banner_light_gray",
        19 => "banner_cyan",
        20 => "banner_purple",
        21 => "banner_blue",
        22 => "banner_brown",
        23 => "banner_green",
        24 => "banner_red",
        25 => "banner_black",
        26 => "red_x",
        27 => "village_desert",
        28 => "village_plains",
        29 => "village_savanna",
        30 => "village_snowy",
        31 => "village_taiga",
        32 => "jungle_temple",
        33 => "swamp_hut",
        _ => "player",
    };
    let value = Tag::compound([
        ("type", string(ty)),
        ("x", Tag::Double(as_double(d.get("x"), 0.0).to_bits())),
        ("z", Tag::Double(as_double(d.get("z"), 0.0).to_bits())),
        (
            "rotation",
            Tag::Float((as_double(d.get("rot"), 0.0) as f32).to_bits()),
        ),
    ]);
    (id, value)
}

fn fix_book_pages(s: &mut Stack) -> Option<Tag> {
    let pages: Vec<String> = stream(s.remove_tag("pages").as_ref())
        .iter()
        .map(|p| as_string(Some(p), ""))
        .collect();
    let filtered: Vec<(String, String)> = match s.remove_tag("filtered_pages") {
        Some(Tag::Compound(c)) => c
            .iter()
            .map(|(k, v)| (k.to_string_lossy(), as_string(Some(v), "")))
            .collect(),
        _ => Vec::new(),
    };
    if pages.is_empty() {
        return None;
    }
    let fixed = pages
        .iter()
        .enumerate()
        .map(|(i, page)| {
            let f = filtered
                .iter()
                .find(|(k, _)| *k == i.to_string())
                .map(|(_, v)| v.as_str());
            filtered_text(page, f)
        })
        .collect();
    Some(Tag::List(fixed))
}

fn filtered_text(text: &str, filtered: Option<&str>) -> Tag {
    let mut t = Tag::compound([("raw", string(text))]);
    if let Some(f) = filtered {
        set(&mut t, "filtered", string(f));
    }
    t
}

fn fix_lodestone_tracker(s: &mut Stack) {
    let pos = s.remove_tag("LodestonePos");
    let dimension = s.remove_tag("LodestoneDimension");
    if pos.is_none() && dimension.is_none() {
        return;
    }
    let tracked = as_bool(s.remove_tag("LodestoneTracked").as_ref(), true);
    let mut c = empty_map();
    if let (Some(p), Some(d)) = (pos, dimension) {
        set(
            &mut c,
            "target",
            Tag::compound([("pos", p), ("dimension", d)]),
        );
    }
    if !tracked {
        set(&mut c, "tracked", Tag::Byte(0));
    }
    s.set_component("minecraft:lodestone_tracker", c);
}

fn fix_firework_explosion(mut e: Tag) -> Tag {
    let shape = match as_int(e.get("Type"), 0) {
        1 => "large_ball",
        2 => "star",
        3 => "creeper",
        4 => "burst",
        _ => "small_ball",
    };
    set(&mut e, "shape", string(shape));
    remove(&mut e, "Type");
    rename(&mut e, "Colors", "colors");
    rename(&mut e, "FadeColors", "fade_colors");
    rename(&mut e, "Trail", "has_trail");
    rename(&mut e, "Flicker", "has_twinkle");
    e
}

/// `ItemStackComponentizationFix.fixProfile`.
pub(super) fn fix_profile(v: &Tag) -> Tag {
    if let Some(name) = as_str(Some(v)) {
        return if valid_player_name(&name) {
            Tag::compound([("name", string(&name))])
        } else {
            empty_map()
        };
    }
    let name = as_string(v.get("Name"), "");
    let id = v.get("Id").cloned();
    let properties = fix_profile_properties(v.get("Properties"));
    let mut p = empty_map();
    if valid_player_name(&name) {
        set(&mut p, "name", string(&name));
    }
    if let Some(id) = id {
        set(&mut p, "id", id);
    }
    if let Some(props) = properties {
        set(&mut p, "properties", props);
    }
    p
}

fn valid_player_name(name: &str) -> bool {
    let units: Vec<u16> = name.encode_utf16().collect();
    units.len() <= 16 && units.iter().all(|&c| c > 32 && c < 127)
}

fn fix_profile_properties(v: Option<&Tag>) -> Option<Tag> {
    let Some(Tag::Compound(map)) = v else {
        return None;
    };
    // asMap collects into a HashMap: the property names in Java hash order.
    let mut entries: Vec<(String, Vec<(String, Option<String>)>)> = map
        .iter()
        .map(|(k, list)| {
            let props = stream(Some(list))
                .iter()
                .map(|p| (as_string(p.get("Value"), ""), as_str(p.get("Signature"))))
                .collect();
            (k.to_string_lossy(), props)
        })
        .collect();
    if entries.is_empty() {
        return None;
    }
    entries = java_hash_order(entries, |(k, _)| k.clone());
    let out = entries
        .into_iter()
        .flat_map(|(name, props)| {
            props.into_iter().map(move |(value, signature)| {
                let mut p = Tag::compound([("name", string(&name)), ("value", string(&value))]);
                if let Some(sig) = signature {
                    set(&mut p, "signature", string(&sig));
                }
                p
            })
        })
        .collect();
    Some(Tag::List(out))
}

/// Java `String.hashCode`.
pub(super) fn java_string_hash(s: &str) -> i32 {
    s.encode_utf16()
        .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(i32::from(c)))
}

/// Iteration order of a `java.util.HashMap`/`HashSet` built by inserting the
/// items in order (default capacity, string keys, no tree bins).
pub(super) fn java_hash_order<T>(items: Vec<T>, key: impl Fn(&T) -> String) -> Vec<T> {
    let mut capacity = 16usize;
    while items.len() as f64 > capacity as f64 * 0.75 {
        capacity *= 2;
    }
    let spread = |h: i32| (h ^ ((h as u32) >> 16) as i32) as u32 as usize;
    let mut buckets: Vec<Vec<T>> = (0..capacity).map(|_| Vec::new()).collect();
    for item in items {
        let index = spread(java_string_hash(&key(&item))) & (capacity - 1);
        buckets[index].push(item);
    }
    buckets.into_iter().flatten().collect()
}

// ------------------------------------------------------------------ particles

/// `ParticleUnflatteningFix.fix` of a particle description string.
pub(super) fn unflatten_particle(description: &str) -> Tag {
    let (first, rest) = match description.split_once(' ') {
        Some((a, b)) => (a, Some(b)),
        None => (description, None),
    };
    let id = namespaced(first);
    let mut result = Tag::compound([("type", string(&id))]);
    let Some(contents) = rest else {
        return result;
    };
    match id.as_str() {
        "minecraft:item" => {
            let mut stack = Tag::compound([("Count", Tag::Int(1))]);
            match contents.find('{') {
                None => set(&mut stack, "id", string(contents)),
                Some(i) => {
                    set(&mut stack, "id", string(&contents[..i]));
                    if let Ok((tag, used)) = crate::nbt::parse_snbt(&contents[i..])
                        && used == contents[i..].len()
                    {
                        set(&mut stack, "tag", tag);
                    }
                }
            }
            set(&mut result, "item", stack);
        }
        "minecraft:block"
        | "minecraft:block_marker"
        | "minecraft:falling_dust"
        | "minecraft:dust_pillar" => {
            let mut state = empty_map();
            match contents.find('[') {
                None => set(&mut state, "Name", string(&namespaced(contents))),
                Some(i) => {
                    set(&mut state, "Name", string(&namespaced(&contents[..i])));
                    let props = parse_block_properties(&contents[i..]);
                    if let Some(Tag::Compound(p)) = &props
                        && !p.is_empty()
                    {
                        set(&mut state, "Properties", props.unwrap_or_else(empty_map));
                    }
                }
            }
            set(&mut result, "block_state", state);
        }
        "minecraft:dust" => {
            let mut r = Reader::new(contents);
            if let Some(color) = r.vector()
                && r.expect(' ')
                && let Some(scale) = r.float()
            {
                set(&mut result, "color", color);
                set(&mut result, "scale", Tag::Float(scale.to_bits()));
            }
        }
        "minecraft:dust_color_transition" => {
            let mut r = Reader::new(contents);
            if let Some(from) = r.vector()
                && r.expect(' ')
                && let Some(scale) = r.float()
                && r.expect(' ')
                && let Some(to) = r.vector()
            {
                set(&mut result, "from_color", from);
                set(&mut result, "to_color", to);
                set(&mut result, "scale", Tag::Float(scale.to_bits()));
            }
        }
        "minecraft:sculk_charge" => {
            if let Some(roll) = Reader::new(contents).float() {
                set(&mut result, "roll", Tag::Float(roll.to_bits()));
            }
        }
        "minecraft:vibration" => {
            let mut r = Reader::new(contents);
            if let Some(x) = r.double()
                && r.expect(' ')
                && let Some(y) = r.double()
                && r.expect(' ')
                && let Some(z) = r.double()
                && r.expect(' ')
                && let Some(ticks) = r.int()
            {
                let pos = int_list(&[
                    (x as f32).floor() as i32,
                    (y as f32).floor() as i32,
                    (z as f32).floor() as i32,
                ]);
                set(
                    &mut result,
                    "destination",
                    Tag::compound([("type", string("minecraft:block")), ("pos", pos)]),
                );
                set(&mut result, "arrival_in_ticks", Tag::Int(ticks));
            }
        }
        "minecraft:shriek" => {
            if let Some(delay) = Reader::new(contents).int() {
                set(&mut result, "delay", Tag::Int(delay));
            }
        }
        _ => {}
    }
    result
}

/// `parseBlockProperties` with Brigadier's StringReader.
fn parse_block_properties(contents: &str) -> Option<Tag> {
    let mut r = Reader::new(contents);
    let mut out = empty_map();
    if !r.expect('[') {
        return Some(empty_map());
    }
    r.skip_ws();
    while r.peek().is_some_and(|c| c != ']') {
        r.skip_ws();
        let Some(key) = r.string() else {
            return Some(empty_map());
        };
        r.skip_ws();
        if !r.expect('=') {
            return Some(empty_map());
        }
        r.skip_ws();
        let Some(value) = r.string() else {
            return Some(empty_map());
        };
        r.skip_ws();
        set(&mut out, &key, string(&value));
        if r.peek().is_some() {
            if r.peek() != Some(',') {
                break;
            }
            r.pos += 1;
        }
    }
    if !r.expect(']') {
        return Some(empty_map());
    }
    Some(out)
}

/// The parts of Brigadier's `StringReader` the particle fix uses.
struct Reader {
    s: Vec<char>,
    pos: usize,
}
impl Reader {
    fn new(s: &str) -> Self {
        Self {
            s: s.chars().collect(),
            pos: 0,
        }
    }
    fn peek(&self) -> Option<char> {
        self.s.get(self.pos).copied()
    }
    fn expect(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
    }
    fn number_text(&mut self) -> String {
        let start = self.pos;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_digit() || c == '.' || c == '-')
        {
            self.pos += 1;
        }
        self.s[start..self.pos].iter().collect()
    }
    fn float(&mut self) -> Option<f32> {
        self.number_text().parse().ok()
    }
    fn double(&mut self) -> Option<f64> {
        self.number_text().parse().ok()
    }
    fn int(&mut self) -> Option<i32> {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '-') {
            self.pos += 1;
        }
        self.s[start..self.pos]
            .iter()
            .collect::<String>()
            .parse()
            .ok()
    }
    fn vector(&mut self) -> Option<Tag> {
        let x = self.float()?;
        if !self.expect(' ') {
            return None;
        }
        let y = self.float()?;
        if !self.expect(' ') {
            return None;
        }
        let z = self.float()?;
        Some(Tag::List(vec![
            Tag::Float(x.to_bits()),
            Tag::Float(y.to_bits()),
            Tag::Float(z.to_bits()),
        ]))
    }
    /// `StringReader.readString`: quoted or unquoted.
    fn string(&mut self) -> Option<String> {
        match self.peek() {
            Some(q @ ('"' | '\'')) => {
                self.pos += 1;
                let mut out = String::new();
                let mut escaped = false;
                while let Some(c) = self.peek() {
                    self.pos += 1;
                    if escaped {
                        if c != q && c != '\\' {
                            return None;
                        }
                        out.push(c);
                        escaped = false;
                    } else if c == '\\' {
                        escaped = true;
                    } else if c == q {
                        return Some(out);
                    } else {
                        out.push(c);
                    }
                }
                None
            }
            _ => {
                let start = self.pos;
                while self.peek().is_some_and(|c| {
                    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+')
                }) {
                    self.pos += 1;
                }
                Some(self.s[start..self.pos].iter().collect())
            }
        }
    }
}
