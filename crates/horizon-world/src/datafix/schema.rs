//! Where DataFixerUpper finds each type inside the data, schema by schema
//! (`net.minecraft.util.datafix.schemas`), from the 1.20 schemas on.
//!
//! `walk` visits every value of the target type under a root of a given type,
//! children before parents (DataFixerUpper rewrites recursive types bottom-up).
//! Keys are `version * 10 + sub-version` as in `DataFixUtils.makeKey`, and a
//! fix with key K reads its input with the schema just below K.
use super::dynamic::{as_str, namespaced};
use crate::nbt::Tag;

#[path = "schema_flat.rs"]
pub(super) mod flat;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ty {
    Level,
    /// `LIGHTWEIGHT_LEVEL`: the level data apart from its typed fields (the same value).
    LightweightLevel,
    Player,
    Chunk,
    EntityChunk,
    PoiChunk,
    BlockEntity,
    EntityTree,
    Entity,
    ItemStack,
    DataComponents,
    TextComponent,
    VillagerTrade,
    /// `UNTAGGED_SPAWNER`: the spawner fields of a spawner block entity or minecart.
    UntaggedSpawner,
    Particle,
    BlockState,
    FlatBlockState,
    BlockName,
    ItemName,
    EntityName,
    Biome,
    Recipe,
    StructureFeature,
    GameEventName,
    Advancements,
    Stats,
    Hotbar,
    Objective,
    Team,
    WorldGenSettings,
    SavedCustomBossEvents,
    SavedMapData,
    SavedScoreboard,
    SavedWorldGenSettings,
    SavedStructureFeatureIndices,
    /// Saved data that has no typed fields (`DSL.remainder`).
    SavedRemainder,
    /// Each saved data kind still needs its own identity for fixes that target it.
    SavedEnderDragonFight,
    SavedGameRules,
    SavedTickets,
    SavedRaids,
    SavedRandomSequences,
    SavedWorldBorder,
    SavedWeather,
    SavedWorldClocks,
    SavedWanderingTrader,
    SavedScheduledEvents,
    SavedMapIndex,
    SavedCommandStorage,
    SavedStopwatches,
    Options,
    DebugProfile,
}

/// A schema key: `version * 10 + sub`.
pub type Key = u32;
pub const fn key(version: u32, sub: u32) -> Key {
    version * 10 + sub
}

pub struct Walker<'f> {
    /// The schema the data is read with.
    pub k: Key,
    pub target: Ty,
    pub f: &'f mut dyn FnMut(&mut Tag),
}

impl Walker<'_> {
    pub fn walk(&mut self, ty: Ty, v: &mut Tag) {
        self.children(ty, v);
        if ty == self.target {
            (self.f)(v);
        }
    }
    fn field(&mut self, ty: Ty, v: &mut Tag, name: &str) {
        if let Some(child) = v.get_mut(name) {
            self.walk(ty, child);
        }
    }
    fn list(&mut self, ty: Ty, v: &mut Tag, name: &str) {
        // A list of block names reads the numbers of an array tag and writes them as a list.
        if ty == Ty::BlockName
            && self.k < key(1451, 4)
            && let Some(array) = v.get_mut(name)
        {
            let numbers: Option<Vec<Tag>> = match array {
                Tag::IntArray(a) => Some(a.iter().map(|x| Tag::Int(*x)).collect()),
                Tag::ByteArray(a) => Some(a.iter().map(|x| Tag::Byte(*x as i8)).collect()),
                Tag::LongArray(a) => Some(a.iter().map(|x| Tag::Long(*x)).collect()),
                _ => None,
            };
            if let Some(numbers) = numbers {
                *array = Tag::List(numbers);
            }
        }
        if let Some(Tag::List(items)) = v.get_mut(name) {
            for item in items {
                self.walk(ty, item);
            }
        }
    }
    fn list_in(&mut self, ty: Ty, items: &mut Tag) {
        if let Tag::List(items) = items {
            for item in items {
                self.walk(ty, item);
            }
        }
    }
    /// `DSL.compoundList(type)`: every value of a map.
    fn values(&mut self, ty: Ty, v: &mut Tag, name: &str) {
        if let Some(Tag::Compound(c)) = v.get_mut(name) {
            for value in c.values_mut() {
                self.walk(ty, value);
            }
        }
    }
    /// A list of compounds, each with a typed field.
    fn list_of_field(&mut self, ty: Ty, v: &mut Tag, list: &str, field: &str) {
        if let Some(Tag::List(items)) = v.get_mut(list) {
            for item in items {
                self.field(ty, item, field);
            }
        }
    }
    /// `DSL.compoundList(keyType, valueType)` with typed string keys: keys are
    /// renamed in place by the callback (as a string tag).
    fn keys(&mut self, ty: Ty, v: &mut Tag, name: &str) {
        if ty != self.target {
            return;
        }
        if let Some(Tag::Compound(c)) = v.get_mut(name) {
            let entries: Vec<_> = std::mem::take(c).into_iter().collect();
            for (k, value) in entries {
                let mut key_tag = Tag::String(k.clone());
                (self.f)(&mut key_tag);
                let k = match key_tag {
                    Tag::String(s) => s,
                    _ => k,
                };
                c.insert(k, value);
            }
        }
    }

    fn children(&mut self, ty: Ty, v: &mut Tag) {
        let k = self.k;
        if k < flat::END && self.legacy_children(ty, v) {
            return;
        }
        match ty {
            Ty::Level => {
                if k < key(4771, 0)
                    && let Some(Tag::Compound(c)) = v.get_mut("CustomBossEvents")
                {
                    for e in c.values_mut() {
                        self.field(Ty::TextComponent, e, "Name");
                    }
                }
                self.walk(Ty::LightweightLevel, v);
            }
            Ty::Player => {
                if k >= key(4312, 0) {
                    self.equipment(v);
                }
                if let Some(rv) = v.get_mut("RootVehicle") {
                    self.field(Ty::EntityTree, rv, "Entity");
                }
                self.list(Ty::EntityTree, v, "ender_pearls");
                self.list(Ty::ItemStack, v, "Inventory");
                self.list(Ty::ItemStack, v, "EnderItems");
                self.field(Ty::EntityTree, v, "ShoulderEntityLeft");
                self.field(Ty::EntityTree, v, "ShoulderEntityRight");
                if let Some(book) = v.get_mut("recipeBook") {
                    self.list(Ty::Recipe, book, "recipes");
                    self.list(Ty::Recipe, book, "toBeDisplayed");
                }
            }
            Ty::Chunk if k < key(2842, 0) => {
                if let Some(level) = v.get_mut("Level") {
                    self.level_chunk(level);
                }
            }
            Ty::Chunk => {
                self.list(Ty::EntityTree, v, "entities");
                if let Some(Tag::List(items)) = v.get_mut("block_entities") {
                    for be in items {
                        // DSL.or(BLOCK_ENTITY, remainder): unknown ids stay as they are.
                        if block_entity_known(be, k) {
                            self.walk(Ty::BlockEntity, be);
                        }
                    }
                }
                self.list_of_field(Ty::BlockName, v, "block_ticks", "i");
                if let Some(Tag::List(sections)) = v.get_mut("sections") {
                    for s in sections {
                        if let Some(b) = s.get_mut("biomes") {
                            self.list(Ty::Biome, b, "palette");
                        }
                        if let Some(b) = s.get_mut("block_states") {
                            self.list(Ty::BlockState, b, "palette");
                        }
                    }
                }
                if let Some(s) = v.get_mut("structures") {
                    self.values(Ty::StructureFeature, s, "starts");
                }
            }
            Ty::EntityChunk => self.list(Ty::EntityTree, v, "Entities"),
            // What the schema it was read with did not know stays untyped (see `flat::UNTYPED`).
            Ty::EntityTree if v.get(flat::UNTYPED).is_some() => {}
            Ty::EntityTree => {
                self.list(Ty::EntityTree, v, "Passengers");
                self.walk(Ty::Entity, v);
            }
            Ty::Entity => self.entity(v),
            Ty::UntaggedSpawner => self.untagged_spawner(v),
            Ty::BlockEntity => self.block_entity(v),
            Ty::ItemStack => self.item_stack(v),
            Ty::DataComponents => self.components(v),
            Ty::TextComponent => self.text(v),
            Ty::VillagerTrade => {
                self.field(Ty::ItemStack, v, "buy");
                self.field(Ty::ItemStack, v, "buyB");
                self.field(Ty::ItemStack, v, "sell");
            }
            Ty::Particle => {
                if k >= key(3818, 4) {
                    self.field(Ty::ItemStack, v, "item");
                    self.field(Ty::BlockState, v, "block_state");
                }
            }
            Ty::Hotbar => {
                if let Tag::Compound(c) = v {
                    for row in c.values_mut() {
                        self.list_in(Ty::ItemStack, row);
                    }
                }
            }
            Ty::Stats => {
                if let Some(stats) = v.get_mut("stats") {
                    self.keys(Ty::BlockName, stats, "minecraft:mined");
                    for s in ["crafted", "used", "broken", "picked_up", "dropped"] {
                        self.keys(Ty::ItemName, stats, &format!("minecraft:{s}"));
                    }
                    self.keys(Ty::EntityName, stats, "minecraft:killed");
                    self.keys(Ty::EntityName, stats, "minecraft:killed_by");
                }
            }
            Ty::Advancements => {
                for (adv, kind) in [
                    ("minecraft:adventure/adventuring_time", Ty::Biome),
                    ("minecraft:adventure/kill_a_mob", Ty::EntityName),
                    ("minecraft:adventure/kill_all_mobs", Ty::EntityName),
                    ("minecraft:husbandry/bred_all_animals", Ty::EntityName),
                ] {
                    if let Some(a) = v.get_mut(adv) {
                        self.keys(kind, a, "criteria");
                    }
                }
            }
            Ty::Objective => {
                // The hook unpacked CriteriaName into CriteriaType {type, id}.
                if let Some(ct) = v.get_mut("CriteriaType") {
                    let kind = match as_str(ct.get("type")).as_deref() {
                        Some("minecraft:mined") => Some(Ty::BlockName),
                        Some(
                            "minecraft:crafted"
                            | "minecraft:used"
                            | "minecraft:broken"
                            | "minecraft:picked_up"
                            | "minecraft:dropped",
                        ) => Some(Ty::ItemName),
                        Some("minecraft:killed" | "minecraft:killed_by") => Some(Ty::EntityName),
                        _ => None,
                    };
                    if let Some(kind) = kind {
                        self.field(kind, ct, "id");
                    }
                }
                self.field(Ty::TextComponent, v, "DisplayName");
            }
            Ty::Team => {
                for f in ["MemberNamePrefix", "MemberNameSuffix", "DisplayName"] {
                    self.field(Ty::TextComponent, v, f);
                }
            }
            Ty::SavedCustomBossEvents => {
                if let Some(Tag::Compound(c)) = v.get_mut("data") {
                    for e in c.values_mut() {
                        self.field(Ty::TextComponent, e, "Name");
                    }
                }
            }
            Ty::SavedMapData => {
                if let Some(d) = v.get_mut("data") {
                    let name = if k >= key(3813, 0) { "name" } else { "Name" };
                    self.list_of_field(Ty::TextComponent, d, "banners", name);
                }
            }
            Ty::SavedScoreboard => {
                if let Some(d) = v.get_mut("data") {
                    self.list(Ty::Objective, d, "Objectives");
                    self.list(Ty::Team, d, "Teams");
                    self.list_of_field(Ty::TextComponent, d, "PlayerScores", "display");
                }
            }
            Ty::SavedWorldGenSettings => self.field(Ty::WorldGenSettings, v, "data"),
            Ty::SavedStructureFeatureIndices => {
                if let Some(d) = v.get_mut("data") {
                    self.values(Ty::StructureFeature, d, "Features");
                }
            }
            Ty::WorldGenSettings => self.world_gen(v),
            _ => {}
        }
    }

    fn equipment(&mut self, v: &mut Tag) {
        if self.k >= key(4301, 0) {
            if let Some(eq) = v.get_mut("equipment") {
                for slot in [
                    "mainhand", "offhand", "feet", "legs", "chest", "head", "body", "saddle",
                ] {
                    self.field(Ty::ItemStack, eq, slot);
                }
            }
        } else {
            self.list(Ty::ItemStack, v, "ArmorItems");
            self.list(Ty::ItemStack, v, "HandItems");
            self.field(Ty::ItemStack, v, "body_armor_item");
            self.field(Ty::ItemStack, v, "saddle");
        }
    }

    fn entity(&mut self, v: &mut Tag) {
        let k = self.k;
        self.equipment(v);
        self.field(Ty::TextComponent, v, "CustomName");
        let id = namespaced(&as_str(v.get("id")).unwrap_or_default());
        let id = id.as_str();
        let listener = |w: &mut Self, v: &mut Tag| {
            if let Some(l) = v.get_mut("listener")
                && let Some(e) = l.get_mut("event")
            {
                w.field(Ty::GameEventName, e, "game_event");
            }
        };
        match id {
            "minecraft:area_effect_cloud" => {
                let f = if k >= key(4420, 0) {
                    "custom_particle"
                } else {
                    "Particle"
                };
                self.field(Ty::Particle, v, f);
            }
            "minecraft:arrow" | "minecraft:spectral_arrow" => {
                self.field(Ty::BlockState, v, "inBlockState");
                if k >= key(3685, 0) {
                    self.field(Ty::ItemStack, v, "item");
                }
                if k >= key(3938, 0) {
                    self.field(Ty::ItemStack, v, "weapon");
                }
            }
            "minecraft:trident" => {
                self.field(Ty::BlockState, v, "inBlockState");
                let f = if k >= key(3685, 0) { "item" } else { "Trident" };
                self.field(Ty::ItemStack, v, f);
            }
            "minecraft:chest_minecart" | "minecraft:hopper_minecart" => {
                self.field(Ty::BlockState, v, "DisplayState");
                self.list(Ty::ItemStack, v, "Items");
            }
            "minecraft:minecart" | "minecraft:furnace_minecart" | "minecraft:tnt_minecart" => {
                self.field(Ty::BlockState, v, "DisplayState");
            }
            "minecraft:command_block_minecart" => {
                self.field(Ty::BlockState, v, "DisplayState");
                self.field(Ty::TextComponent, v, "LastOutput");
            }
            "minecraft:spawner_minecart" => {
                self.field(Ty::BlockState, v, "DisplayState");
                self.spawner(v);
            }
            "minecraft:donkey" | "minecraft:mule" => {
                self.list(Ty::ItemStack, v, "Items");
                if k < key(4300, 0) {
                    self.field(Ty::ItemStack, v, "SaddleItem");
                }
            }
            "minecraft:llama" | "minecraft:trader_llama" => {
                self.list(Ty::ItemStack, v, "Items");
                if k < key(4300, 0) {
                    self.field(Ty::ItemStack, v, "SaddleItem");
                }
                let split = if id == "minecraft:llama" {
                    key(3808, 1)
                } else {
                    key(3808, 2)
                };
                if k < split {
                    self.field(Ty::ItemStack, v, "DecorItem");
                }
            }
            "minecraft:horse" => {
                if k < key(4300, 0) {
                    self.field(Ty::ItemStack, v, "SaddleItem");
                }
                if k < key(3808, 0) {
                    self.field(Ty::ItemStack, v, "ArmorItem");
                }
            }
            "minecraft:skeleton_horse" | "minecraft:zombie_horse" => {
                if k < key(4300, 0) {
                    self.field(Ty::ItemStack, v, "SaddleItem");
                }
            }
            "minecraft:enderman" => self.field(Ty::BlockState, v, "carriedBlockState"),
            "minecraft:falling_block" => {
                self.field(Ty::BlockState, v, "BlockState");
                self.field(Ty::BlockEntity, v, "TileEntityData");
            }
            "minecraft:firework_rocket" => self.field(Ty::ItemStack, v, "FireworksItem"),
            "minecraft:item" | "minecraft:item_frame" | "minecraft:glow_item_frame" => {
                self.field(Ty::ItemStack, v, "Item");
            }
            "minecraft:potion" if k < key(4306, 0) => {
                // V2511_1 renamed the typed field.
                let field = if k < key(2511, 1) { "Potion" } else { "Item" };
                self.field(Ty::ItemStack, v, field);
            }
            "minecraft:splash_potion" | "minecraft:lingering_potion" if k >= key(4306, 0) => {
                self.field(Ty::ItemStack, v, "Item");
            }
            "minecraft:villager" | "minecraft:wandering_trader" => {
                self.list(Ty::ItemStack, v, "Inventory");
                if let Some(o) = v.get_mut("Offers") {
                    self.list(Ty::VillagerTrade, o, "Recipes");
                }
            }
            "minecraft:zombie_villager" => {
                if let Some(o) = v.get_mut("Offers") {
                    self.list(Ty::VillagerTrade, o, "Recipes");
                }
            }
            "minecraft:pillager" | "minecraft:piglin" => self.list(Ty::ItemStack, v, "Inventory"),
            "minecraft:allay" => {
                self.list(Ty::ItemStack, v, "Inventory");
                listener(self, v);
            }
            "minecraft:warden" => listener(self, v),
            "minecraft:tnt" if k >= key(3683, 0) => self.field(Ty::BlockState, v, "block_state"),
            "minecraft:chest_boat" if k < key(4067, 0) => self.list(Ty::ItemStack, v, "Items"),
            "minecraft:item_display" => self.field(Ty::ItemStack, v, "item"),
            "minecraft:block_display" => self.field(Ty::BlockState, v, "block_state"),
            "minecraft:text_display" => self.field(Ty::TextComponent, v, "text"),
            "minecraft:ominous_item_spawner" if k >= key(3825, 0) => {
                self.field(Ty::ItemStack, v, "item")
            }
            _ if k >= key(4067, 0)
                && (id.ends_with("_chest_boat") || id == "minecraft:bamboo_chest_raft") =>
            {
                self.list(Ty::ItemStack, v, "Items");
            }
            _ => {}
        }
    }

    /// The `Level` of a chunk before 1.18 (V99, V1451_1, V1466, V2832).
    fn level_chunk(&mut self, v: &mut Tag) {
        let k = self.k;
        self.list(Ty::EntityTree, v, "Entities");
        if let Some(Tag::List(items)) = v.get_mut("TileEntities") {
            for be in items {
                if block_entity_known(be, k) {
                    self.walk(Ty::BlockEntity, be);
                }
            }
        }
        self.list_of_field(Ty::BlockName, v, "TileTicks", "i");
        if let Some(Tag::List(sections)) = v.get_mut("Sections") {
            for s in sections {
                if k >= key(2832, 0) {
                    if let Some(b) = s.get_mut("biomes") {
                        self.list(Ty::Biome, b, "palette");
                    }
                    if let Some(b) = s.get_mut("block_states") {
                        self.list(Ty::BlockState, b, "palette");
                    }
                } else if k >= key(1451, 1) {
                    self.list(Ty::BlockState, s, "Palette");
                }
            }
        }
        if k >= key(1466, 0)
            && let Some(s) = v.get_mut("Structures")
        {
            self.values(Ty::StructureFeature, s, "Starts");
        }
    }

    fn spawner(&mut self, v: &mut Tag) {
        self.walk(Ty::UntaggedSpawner, v);
    }

    /// `UNTAGGED_SPAWNER` (V106, V2831).
    fn untagged_spawner(&mut self, v: &mut Tag) {
        if self.k < key(106, 0) {
            return;
        }
        if self.k < key(2831, 0) {
            self.list_of_field(Ty::EntityTree, v, "SpawnPotentials", "Entity");
            self.field(Ty::EntityTree, v, "SpawnData");
            return;
        }
        if let Some(Tag::List(items)) = v.get_mut("SpawnPotentials") {
            for p in items {
                if let Some(d) = p.get_mut("data") {
                    self.field(Ty::EntityTree, d, "entity");
                }
            }
        }
        if let Some(d) = v.get_mut("SpawnData") {
            self.field(Ty::EntityTree, d, "entity");
        }
    }

    fn block_entity(&mut self, v: &mut Tag) {
        let k = self.k;
        self.field(Ty::DataComponents, v, "components");
        let id = namespaced(&as_str(v.get("id")).unwrap_or_default());
        let named_inventory = |w: &mut Self, v: &mut Tag| {
            w.list(Ty::ItemStack, v, "Items");
            w.field(Ty::TextComponent, v, "CustomName");
        };
        match id.as_str() {
            "minecraft:furnace" | "minecraft:smoker" | "minecraft:blast_furnace" => {
                named_inventory(self, v);
                self.keys(Ty::Recipe, v, "RecipesUsed");
            }
            "minecraft:chest"
            | "minecraft:trapped_chest"
            | "minecraft:dispenser"
            | "minecraft:dropper"
            | "minecraft:hopper"
            | "minecraft:shulker_box"
            | "minecraft:brewing_stand"
            | "minecraft:barrel" => {
                named_inventory(self, v);
            }
            "minecraft:crafter" if k >= key(3682, 0) => named_inventory(self, v),
            "minecraft:jukebox" => self.field(Ty::ItemStack, v, "RecordItem"),
            "minecraft:flower_pot" if k < key(1451, 5) => self.field(Ty::ItemName, v, "Item"),
            "minecraft:sign" | "minecraft:hanging_sign" => {
                for side in ["front_text", "back_text"] {
                    if let Some(s) = v.get_mut(side) {
                        self.list(Ty::TextComponent, s, "messages");
                        self.list(Ty::TextComponent, s, "filtered_messages");
                    }
                }
            }
            "minecraft:mob_spawner" => self.spawner(v),
            "minecraft:piston" => self.field(Ty::BlockState, v, "blockState"),
            "minecraft:enchanting_table" | "minecraft:beacon" | "minecraft:banner" => {
                self.field(Ty::TextComponent, v, "CustomName");
            }
            "minecraft:skull" => self.field(Ty::TextComponent, v, "custom_name"),
            "minecraft:command_block" => {
                self.field(Ty::TextComponent, v, "CustomName");
                self.field(Ty::TextComponent, v, "LastOutput");
            }
            "minecraft:jigsaw" => self.field(Ty::FlatBlockState, v, "final_state"),
            "minecraft:campfire" | "minecraft:chiseled_bookshelf" => {
                self.list(Ty::ItemStack, v, "Items")
            }
            "minecraft:shelf" if k >= key(4533, 0) => self.list(Ty::ItemStack, v, "Items"),
            "minecraft:lectern" => self.field(Ty::ItemStack, v, "Book"),
            "minecraft:beehive" => {
                if k >= key(3818, 0) {
                    self.list_of_field(Ty::EntityTree, v, "bees", "entity_data");
                } else {
                    self.list_of_field(Ty::EntityTree, v, "Bees", "EntityData");
                }
            }
            "minecraft:sculk_sensor" | "minecraft:sculk_shrieker" => {
                if let Some(l) = v.get_mut("listener")
                    && let Some(e) = l.get_mut("event")
                {
                    self.field(Ty::GameEventName, e, "game_event");
                }
            }
            "minecraft:decorated_pot" => {
                if k >= key(4996, 1) {
                    if let Some(s) = v.get_mut("sherds") {
                        for side in ["back", "left", "right", "front"] {
                            self.field(Ty::ItemStack, s, side);
                        }
                    }
                } else if k >= key(3448, 0) {
                    self.list(Ty::ItemName, v, "sherds");
                } else {
                    self.list(Ty::ItemName, v, "shards");
                }
                self.field(Ty::ItemStack, v, "item");
            }
            "minecraft:brushable_block" | "minecraft:suspicious_sand" => {
                self.field(Ty::ItemStack, v, "item")
            }
            "minecraft:trial_spawner" if k >= key(3689, 0) => {
                if let Some(Tag::List(items)) = v.get_mut("spawn_potentials") {
                    for p in items {
                        if let Some(d) = p.get_mut("data") {
                            self.field(Ty::EntityTree, d, "entity");
                        }
                    }
                }
                if let Some(d) = v.get_mut("spawn_data") {
                    self.field(Ty::EntityTree, d, "entity");
                }
            }
            "minecraft:vault" if k >= key(3807, 0) => {
                if let Some(c) = v.get_mut("config") {
                    self.field(Ty::ItemStack, c, "key_item");
                }
                if let Some(c) = v.get_mut("server_data") {
                    self.list(Ty::ItemStack, c, "items_to_eject");
                }
                if let Some(c) = v.get_mut("shared_data") {
                    self.field(Ty::ItemStack, c, "display_item");
                }
            }
            "minecraft:test_instance_block" if k >= key(4302, 0) => {
                if let Some(d) = v.get_mut("data") {
                    self.field(Ty::TextComponent, d, "error_message");
                }
                self.list_of_field(Ty::TextComponent, v, "errors", "text");
            }
            _ => {}
        }
    }

    fn item_stack(&mut self, v: &mut Tag) {
        if self.k >= key(3818, 5) {
            self.field(Ty::ItemName, v, "id");
            self.field(Ty::DataComponents, v, "components");
            return;
        }
        self.field(Ty::ItemName, v, "id");
        let Some(tag) = v.get_mut("tag") else {
            return;
        };
        self.field(Ty::EntityTree, tag, "EntityTag");
        self.field(Ty::BlockEntity, tag, "BlockEntityTag");
        self.list(Ty::BlockName, tag, "CanDestroy");
        self.list(Ty::BlockName, tag, "CanPlaceOn");
        self.list(Ty::ItemStack, tag, "Items");
        self.list(Ty::ItemStack, tag, "ChargedProjectiles");
        self.list(Ty::TextComponent, tag, "pages");
        self.values(Ty::TextComponent, tag, "filtered_pages");
        if let Some(d) = tag.get_mut("display") {
            self.field(Ty::TextComponent, d, "Name");
            self.list(Ty::TextComponent, d, "Lore");
        }
    }

    fn components(&mut self, v: &mut Tag) {
        let k = self.k;
        if k < key(3818, 3) {
            return;
        }
        let Tag::Compound(_) = v else {
            return;
        };
        self.list_of_field(Ty::EntityTree, v, "minecraft:bees", "entity_data");
        self.field(Ty::BlockEntity, v, "minecraft:block_entity_data");
        self.list(Ty::ItemStack, v, "minecraft:bundle_contents");
        for c in ["minecraft:can_break", "minecraft:can_place_on"] {
            let Some(p) = v.get_mut(c) else {
                continue;
            };
            if k >= key(4307, 0) {
                // A predicate or a list of them.
                if let Tag::List(items) = p {
                    for item in items {
                        self.blocks_predicate(item);
                    }
                } else {
                    self.blocks_predicate(p);
                }
            } else if let Some(Tag::List(items)) = p.get_mut("predicates") {
                for item in items {
                    self.blocks_predicate(item);
                }
            }
        }
        self.list(Ty::ItemStack, v, "minecraft:charged_projectiles");
        self.list_of_field(Ty::ItemStack, v, "minecraft:container", "item");
        self.field(Ty::EntityTree, v, "minecraft:entity_data");
        if k >= key(4996, 0) {
            if let Some(p) = v.get_mut("minecraft:pot_decorations") {
                for side in ["back", "left", "right", "front"] {
                    self.field(Ty::ItemStack, p, side);
                }
            }
        } else {
            self.list(Ty::ItemName, v, "minecraft:pot_decorations");
        }
        if k < key(4059, 0) {
            if let Some(f) = v.get_mut("minecraft:food") {
                self.field(Ty::ItemStack, f, "using_converts_to");
            }
        } else {
            self.field(Ty::ItemStack, v, "minecraft:use_remainder");
            if let Some(e) = v.get_mut("minecraft:equippable")
                && let Some(a) = e.get_mut("allowed_entities")
            {
                if let Tag::List(_) = a {
                    self.list_in(Ty::EntityName, a);
                } else {
                    self.walk(Ty::EntityName, a);
                }
            }
            self.field(Ty::ItemStack, v, "minecraft:sulfur_cube_content");
        }
        self.field(Ty::TextComponent, v, "minecraft:custom_name");
        self.field(Ty::TextComponent, v, "minecraft:item_name");
        self.list(Ty::TextComponent, v, "minecraft:lore");
        if let Some(b) = v.get_mut("minecraft:written_book_content")
            && let Some(Tag::List(pages)) = b.get_mut("pages")
        {
            for page in pages {
                // DSL.or({raw, filtered}, TEXT_COMPONENT): any compound is the filterable form.
                if let Tag::Compound(_) = page {
                    self.field(Ty::TextComponent, page, "raw");
                    self.field(Ty::TextComponent, page, "filtered");
                } else {
                    self.walk(Ty::TextComponent, page);
                }
            }
        }
    }

    fn blocks_predicate(&mut self, p: &mut Tag) {
        if let Some(b) = p.get_mut("blocks") {
            if let Tag::List(_) = b {
                self.list_in(Ty::BlockName, b);
            } else {
                self.walk(Ty::BlockName, b);
            }
        }
    }

    fn text(&mut self, v: &mut Tag) {
        let k = self.k;
        if k < key(4290, 0) {
            return;
        }
        match v {
            Tag::List(_) => self.list_in(Ty::TextComponent, v),
            Tag::Compound(_) => {
                self.list(Ty::TextComponent, v, "extra");
                self.field(Ty::TextComponent, v, "separator");
                if k >= key(4292, 0) {
                    if let Some(h) = v.get_mut("hover_event") {
                        match as_str(h.get("action")).as_deref() {
                            Some("show_text") => self.field(Ty::TextComponent, h, "value"),
                            Some("show_item") => self.walk(Ty::ItemStack, h),
                            Some("show_entity") => {
                                self.field(Ty::EntityName, h, "id");
                                self.field(Ty::TextComponent, h, "name");
                            }
                            _ => {}
                        }
                    }
                } else if let Some(h) = v.get_mut("hoverEvent") {
                    match as_str(h.get("action")).as_deref() {
                        Some("show_text") => self.field(Ty::TextComponent, h, "contents"),
                        Some("show_item") => {
                            if let Some(c) = h.get_mut("contents") {
                                // DSL.or(ITEM_STACK, ITEM_NAME)
                                if let Tag::Compound(_) = c {
                                    self.walk(Ty::ItemStack, c);
                                } else {
                                    self.walk(Ty::ItemName, c);
                                }
                            }
                        }
                        Some("show_entity") => {
                            if let Some(c) = h.get_mut("contents") {
                                self.field(Ty::EntityName, c, "type");
                                self.field(Ty::TextComponent, c, "name");
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    /// `WORLD_GEN_SETTINGS` (V2832).
    fn world_gen(&mut self, v: &mut Tag) {
        if self.k < key(2551, 0) {
            return;
        }
        let Some(Tag::Compound(dims)) = v.get_mut("dimensions") else {
            return;
        };
        for dim in dims.values_mut() {
            let Some(g) = dim.get_mut("generator") else {
                continue;
            };
            match as_str(g.get("type")).as_deref() {
                Some("minecraft:flat") => {
                    if let Some(s) = g.get_mut("settings") {
                        self.field(Ty::Biome, s, "biome");
                        self.list_of_field(Ty::BlockName, s, "layers", "block");
                    }
                }
                Some("minecraft:noise") => {
                    if let Some(b) = g.get_mut("biome_source") {
                        match as_str(b.get("type")).as_deref() {
                            Some("minecraft:fixed") => self.field(Ty::Biome, b, "biome"),
                            Some("minecraft:checkerboard") => self.list(Ty::Biome, b, "biomes"),
                            _ => {}
                        }
                    }
                    if let Some(s) = g.get_mut("settings")
                        && let Tag::Compound(_) = s
                    {
                        self.field(Ty::BlockName, s, "default_block");
                        self.field(Ty::BlockName, s, "default_fluid");
                    }
                }
                _ => {}
            }
        }
    }
}

/// Block entity ids the schema at `k` knows (others are read as remainder).
pub fn block_entity_known(be: &Tag, k: Key) -> bool {
    let id = namespaced(&as_str(be.get("id")).unwrap_or_default());
    if k < flat::END {
        return flat::legacy_block_entity_known(&id, k);
    }
    let since = match id.as_str() {
        "minecraft:sculk_catalyst" => key(3076, 0),
        "minecraft:sculk_shrieker" => key(3078, 0),
        "minecraft:hanging_sign" => key(3202, 0),
        "minecraft:chiseled_bookshelf" => key(3204, 0),
        "minecraft:decorated_pot" => key(3327, 0),
        "minecraft:suspicious_sand" => return (key(3327, 0)..key(3438, 0)).contains(&k),
        "minecraft:crafter" => key(3682, 0),
        "minecraft:trial_spawner" => key(3689, 0),
        "minecraft:vault" => key(3807, 0),
        "minecraft:creaking_heart" => key(4071, 0),
        "minecraft:test_block" | "minecraft:test_instance_block" => key(4302, 0),
        "minecraft:copper_golem_statue" => key(4532, 0),
        "minecraft:shelf" => key(4533, 0),
        "minecraft:potent_sulfur" => key(4881, 0),
        "minecraft:bed" => return k < key(4885, 0),
        "minecraft:brushable_block" | "minecraft:calibrated_sculk_sensor" => key(3438, 0),
        _ => 0,
    };
    k >= since && BLOCK_ENTITIES.contains(&id.as_str())
}

/// Block entity types of the schemas from 1.20 on.
const BLOCK_ENTITIES: &[&str] = &[
    "minecraft:furnace",
    "minecraft:chest",
    "minecraft:trapped_chest",
    "minecraft:ender_chest",
    "minecraft:jukebox",
    "minecraft:dispenser",
    "minecraft:dropper",
    "minecraft:sign",
    "minecraft:mob_spawner",
    "minecraft:piston",
    "minecraft:brewing_stand",
    "minecraft:enchanting_table",
    "minecraft:end_portal",
    "minecraft:beacon",
    "minecraft:skull",
    "minecraft:daylight_detector",
    "minecraft:hopper",
    "minecraft:comparator",
    "minecraft:banner",
    "minecraft:structure_block",
    "minecraft:end_gateway",
    "minecraft:command_block",
    "minecraft:shulker_box",
    "minecraft:bed",
    "minecraft:conduit",
    "minecraft:barrel",
    "minecraft:smoker",
    "minecraft:blast_furnace",
    "minecraft:lectern",
    "minecraft:bell",
    "minecraft:jigsaw",
    "minecraft:campfire",
    "minecraft:beehive",
    "minecraft:sculk_sensor",
    "minecraft:sculk_catalyst",
    "minecraft:sculk_shrieker",
    "minecraft:hanging_sign",
    "minecraft:chiseled_bookshelf",
    "minecraft:decorated_pot",
    "minecraft:suspicious_sand",
    "minecraft:brushable_block",
    "minecraft:calibrated_sculk_sensor",
    "minecraft:crafter",
    "minecraft:trial_spawner",
    "minecraft:vault",
    "minecraft:creaking_heart",
    "minecraft:test_block",
    "minecraft:test_instance_block",
    "minecraft:copper_golem_statue",
    "minecraft:shelf",
    "minecraft:potent_sulfur",
];

/// Visit every value of `target` under a `root`-typed value read with schema `k`.
pub fn walk(root: Ty, v: &mut Tag, k: Key, target: Ty, f: &mut dyn FnMut(&mut Tag)) {
    Walker { k, target, f }.walk(root, v);
}
