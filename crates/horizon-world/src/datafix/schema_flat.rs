//! The schemas of before 1.14 (keys below 1800): V99 to V1510 as far as the fixes
//! of those versions look. [`Walker::legacy_children`] answers for the types whose
//! shape differs from the general code of [`super`]; the other types fall through.
use super::{Key, Ty, Walker, as_str, key, namespaced};
use crate::nbt::Tag;
use std::cell::Cell;
use std::collections::HashSet;
use std::sync::OnceLock;

/// DataFixerUpper reads the data once, with the schema of its version: an entity tree
/// of an id that schema does not register stays untyped for good, even when a later
/// schema registers the id. Such a tree carries this key until the data is written.
pub(in crate::datafix) const UNTYPED: &str = "\u{0}untyped";

thread_local! {
    /// Set while the data is read with its own schema: the walkers mark what is untyped.
    static MARKING: Cell<bool> = const { Cell::new(false) };
}

/// Mark the untyped entity trees of data read with the schema at `k`.
pub(in crate::datafix) fn mark_untyped(root: Ty, data: &mut Tag, k: Key) {
    if k >= END {
        return;
    }
    MARKING.with(|m| m.set(true));
    super::walk(root, data, k, Ty::BlockEntity, &mut |_| {});
    MARKING.with(|m| m.set(false));
}

/// Remove the untyped marks of `v` (it is read again).
pub(in crate::datafix) fn strip_untyped(v: &mut Tag) {
    match v {
        Tag::Compound(c) => {
            c.remove(&crate::datafix::dynamic::key(UNTYPED));
            c.values_mut().for_each(strip_untyped);
        }
        Tag::List(items) => items.iter_mut().for_each(strip_untyped),
        _ => {}
    }
}

fn mark(tree: &mut Tag) {
    if let Tag::Compound(_) = tree {
        crate::datafix::dynamic::set(tree, UNTYPED, Tag::Byte(1));
    }
}

/// Whether every tree of a list reads as typed (a list reads only if all its elements do);
/// while the data is read, the trees of a list that does not are marked.
pub(in crate::datafix) fn all_trees_typed(trees: &mut [Tag], k: Key) -> bool {
    let ok = trees.iter().all(|t| entity_tree_known(t, k));
    if !ok && MARKING.with(Cell::get) {
        trees.iter_mut().for_each(mark);
    }
    ok
}

/// The schemas below this key are described here.
pub(in crate::datafix) const END: Key = key(1800, 0);

impl Walker<'_> {
    /// `children` for a schema below [`END`]; false when the general code applies.
    pub(super) fn legacy_children(&mut self, ty: Ty, v: &mut Tag) -> bool {
        let k = self.k;
        match ty {
            Ty::Entity => {
                if k < key(705, 0) {
                    self.legacy_entity_old(v);
                } else {
                    self.legacy_entity(v);
                }
                true
            }
            // An entity of an unknown id does not read as a typed value: nothing in it is fixed.
            Ty::EntityTree => {
                if entity_tree_known(v, k) {
                    if k < key(135, 0) {
                        // The vehicle this entity rides is a tree of its own.
                        if let Some(vehicle) = v.get_mut("Riding") {
                            if entity_tree_known(vehicle, k) {
                                self.walk(Ty::EntityTree, vehicle);
                            } else if MARKING.with(Cell::get) {
                                mark(vehicle);
                            }
                        }
                    } else if let Some(Tag::List(trees)) = v.get_mut("Passengers")
                        && all_trees_typed(trees, k)
                    {
                        for tree in trees {
                            self.walk(Ty::EntityTree, tree);
                        }
                    }
                    self.walk(Ty::Entity, v);
                } else if MARKING.with(Cell::get) {
                    mark(v);
                }
                true
            }
            Ty::UntaggedSpawner if k >= key(106, 0) => {
                self.legacy_untagged_spawner(v);
                true
            }
            Ty::BlockEntity if k < key(704, 0) => {
                self.legacy_block_entity_old(v);
                true
            }
            Ty::Chunk => {
                if let Some(level) = v.get_mut("Level") {
                    self.legacy_level_chunk(level);
                }
                true
            }
            // STATS is a plain map until V1451_6 turns it into typed counters.
            Ty::Stats if k < key(1451, 6) => true,
            Ty::Objective if k < key(1451, 6) => {
                self.field(Ty::TextComponent, v, "DisplayName");
                true
            }
            _ => false,
        }
    }

    /// The `Level` of a chunk (V99 to V1510).
    fn legacy_level_chunk(&mut self, v: &mut Tag) {
        let k = self.k;
        // A list of entity trees reads only if every entity of it is known.
        if let Some(Tag::List(trees)) = v.get_mut("Entities")
            && all_trees_typed(trees, k)
        {
            for tree in trees {
                self.walk(Ty::EntityTree, tree);
            }
        }
        if let Some(Tag::List(items)) = v.get_mut("TileEntities") {
            for be in items {
                if super::block_entity_known(be, k) {
                    self.walk(Ty::BlockEntity, be);
                }
            }
        }
        self.list_of_field(Ty::BlockName, v, "TileTicks", "i");
        if k >= key(1451, 1)
            && let Some(Tag::List(sections)) = v.get_mut("Sections")
        {
            for s in sections {
                self.list(Ty::BlockState, s, "Palette");
            }
        }
        if k >= key(1466, 0)
            && let Some(s) = v.get_mut("Structures")
        {
            self.values(Ty::StructureFeature, s, "Starts");
        }
    }

    /// `UNTAGGED_SPAWNER` of V106 to V2830: the potential entities are a list that
    /// reads only if all of them do.
    fn legacy_untagged_spawner(&mut self, v: &mut Tag) {
        let k = self.k;
        if let Some(Tag::List(potentials)) = v.get_mut("SpawnPotentials") {
            let mut trees: Vec<Option<&mut Tag>> =
                potentials.iter_mut().map(|p| p.get_mut("Entity")).collect();
            let ok = trees
                .iter()
                .all(|t| t.as_deref().is_some_and(|t| entity_tree_known(t, k)));
            if ok {
                for tree in trees.iter_mut().flatten() {
                    self.walk(Ty::EntityTree, tree);
                }
            } else if MARKING.with(Cell::get) {
                trees.iter_mut().flatten().for_each(|t| mark(t));
            }
        }
        self.field(Ty::EntityTree, v, "SpawnData");
    }

    /// `ENTITY` of the schemas V99 to V703 (CamelCase ids).
    fn legacy_entity_old(&mut self, v: &mut Tag) {
        let k = self.k;
        if k < key(100, 0) {
            self.list(Ty::ItemStack, v, "Equipment");
        } else {
            self.list(Ty::ItemStack, v, "ArmorItems");
            self.list(Ty::ItemStack, v, "HandItems");
        }
        let id = as_str(v.get("id")).unwrap_or_default();
        match id.as_str() {
            "Item" | "ItemFrame" => self.field(Ty::ItemStack, v, "Item"),
            "ThrownEgg" | "Snowball" | "Fireball" | "SmallFireball" | "ThrownEnderpearl"
            | "ThrownExpBottle" | "WitherSkull" | "Arrow" | "TippedArrow" | "SpectralArrow" => {
                self.field(Ty::BlockName, v, "inTile")
            }
            "ThrownPotion" => {
                self.field(Ty::BlockName, v, "inTile");
                self.field(Ty::ItemStack, v, "Potion");
            }
            "FallingSand" => {
                self.field(Ty::BlockName, v, "Block");
                self.field(Ty::BlockEntity, v, "TileEntityData");
            }
            "FireworksRocketEntity" => self.field(Ty::ItemStack, v, "FireworksItem"),
            "Minecart" | "MinecartChest" | "MinecartHopper" => {
                self.field(Ty::BlockName, v, "DisplayTile");
                self.list(Ty::ItemStack, v, "Items");
            }
            "MinecartRideable" | "MinecartFurnace" | "MinecartTNT" => {
                self.field(Ty::BlockName, v, "DisplayTile")
            }
            "MinecartSpawner" => {
                self.field(Ty::BlockName, v, "DisplayTile");
                self.spawner(v);
            }
            "MinecartCommandBlock" => {
                self.field(Ty::BlockName, v, "DisplayTile");
                self.field(Ty::TextComponent, v, "LastOutput");
            }
            "Enderman" => self.field(Ty::BlockName, v, "carried"),
            "EntityHorse" => {
                self.list(Ty::ItemStack, v, "Items");
                self.field(Ty::ItemStack, v, "ArmorItem");
                self.field(Ty::ItemStack, v, "SaddleItem");
            }
            "Horse" => {
                self.field(Ty::ItemStack, v, "ArmorItem");
                self.field(Ty::ItemStack, v, "SaddleItem");
            }
            "Donkey" | "Mule" => {
                self.list(Ty::ItemStack, v, "Items");
                self.field(Ty::ItemStack, v, "SaddleItem");
            }
            "ZombieHorse" | "SkeletonHorse" => self.field(Ty::ItemStack, v, "SaddleItem"),
            "Villager" => {
                self.list(Ty::ItemStack, v, "Inventory");
                if let Some(o) = v.get_mut("Offers") {
                    self.list(Ty::VillagerTrade, o, "Recipes");
                }
            }
            "ZombieVillager" => {
                if let Some(o) = v.get_mut("Offers") {
                    self.list(Ty::VillagerTrade, o, "Recipes");
                }
            }
            "AreaEffectCloud" => self.field(Ty::Particle, v, "Particle"),
            _ => {}
        }
    }

    /// `BLOCK_ENTITY` of the schemas V99 to V703 (CamelCase ids).
    fn legacy_block_entity_old(&mut self, v: &mut Tag) {
        let id = as_str(v.get("id")).unwrap_or_default();
        match id.as_str() {
            "Furnace" | "Chest" | "Trap" | "Dropper" | "Cauldron" | "Hopper" => {
                self.list(Ty::ItemStack, v, "Items")
            }
            "RecordPlayer" => self.field(Ty::ItemStack, v, "RecordItem"),
            "Sign" => {
                for line in ["Text1", "Text2", "Text3", "Text4"] {
                    self.field(Ty::TextComponent, v, line);
                }
            }
            "MobSpawner" => self.spawner(v),
            "FlowerPot" => self.field(Ty::ItemName, v, "Item"),
            "Control" => self.field(Ty::TextComponent, v, "LastOutput"),
            "Banner" => self.field(Ty::TextComponent, v, "CustomName"),
            _ => {}
        }
    }

    /// `ENTITY` of the schemas V705 to V1510 (the ids are `minecraft:...` since V704).
    fn legacy_entity(&mut self, v: &mut Tag) {
        let k = self.k;
        self.list(Ty::ItemStack, v, "ArmorItems");
        self.list(Ty::ItemStack, v, "HandItems");
        self.field(Ty::TextComponent, v, "CustomName");
        let id = namespaced(&as_str(v.get("id")).unwrap_or_default());
        // V1451_3 flattens the blocks of entities; V1510 renames some entities.
        let blocks_as_names = k < key(1451, 3);
        let renamed = k >= key(1510, 0);
        let block = |w: &mut Self, v: &mut Tag, old: &str, new: &str| {
            if blocks_as_names {
                w.field(Ty::BlockName, v, old);
            } else {
                w.field(Ty::BlockState, v, new);
            }
        };
        match id.as_str() {
            "minecraft:area_effect_cloud" => self.field(Ty::Particle, v, "Particle"),
            "minecraft:arrow" | "minecraft:spectral_arrow" => {
                block(self, v, "inTile", "inBlockState")
            }
            "minecraft:egg"
            | "minecraft:ender_pearl"
            | "minecraft:fireball"
            | "minecraft:small_fireball"
            | "minecraft:snowball"
            | "minecraft:wither_skull" => {
                if blocks_as_names {
                    self.field(Ty::BlockName, v, "inTile");
                }
            }
            "minecraft:xp_bottle" if !renamed => {
                if blocks_as_names {
                    self.field(Ty::BlockName, v, "inTile");
                }
            }
            "minecraft:potion" => {
                self.field(Ty::ItemStack, v, "Potion");
                if blocks_as_names {
                    self.field(Ty::BlockName, v, "inTile");
                }
            }
            "minecraft:trident" => {
                self.field(Ty::BlockState, v, "inBlockState");
                self.field(Ty::ItemStack, v, "Trident");
            }
            "minecraft:chest_minecart" | "minecraft:hopper_minecart" => {
                block(self, v, "DisplayTile", "DisplayState");
                self.list(Ty::ItemStack, v, "Items");
            }
            "minecraft:minecart" | "minecraft:furnace_minecart" | "minecraft:tnt_minecart" => {
                block(self, v, "DisplayTile", "DisplayState");
            }
            "minecraft:commandblock_minecart" if !renamed => {
                block(self, v, "DisplayTile", "DisplayState");
                self.field(Ty::TextComponent, v, "LastOutput");
            }
            "minecraft:command_block_minecart" if renamed => {
                block(self, v, "DisplayTile", "DisplayState");
                self.field(Ty::TextComponent, v, "LastOutput");
            }
            "minecraft:spawner_minecart" => {
                block(self, v, "DisplayTile", "DisplayState");
                self.spawner(v);
            }
            "minecraft:enderman" => block(self, v, "carried", "carriedBlockState"),
            "minecraft:falling_block" => {
                block(self, v, "Block", "BlockState");
                self.field(Ty::BlockEntity, v, "TileEntityData");
            }
            "minecraft:fireworks_rocket" if !renamed => {
                self.field(Ty::ItemStack, v, "FireworksItem")
            }
            "minecraft:firework_rocket" if renamed => self.field(Ty::ItemStack, v, "FireworksItem"),
            "minecraft:item" | "minecraft:item_frame" => self.field(Ty::ItemStack, v, "Item"),
            "minecraft:horse" => {
                self.field(Ty::ItemStack, v, "ArmorItem");
                self.field(Ty::ItemStack, v, "SaddleItem");
            }
            "minecraft:donkey" | "minecraft:mule" => {
                self.list(Ty::ItemStack, v, "Items");
                self.field(Ty::ItemStack, v, "SaddleItem");
            }
            "minecraft:llama" => {
                self.list(Ty::ItemStack, v, "Items");
                self.field(Ty::ItemStack, v, "SaddleItem");
                self.field(Ty::ItemStack, v, "DecorItem");
            }
            "minecraft:skeleton_horse" | "minecraft:zombie_horse" => {
                self.field(Ty::ItemStack, v, "SaddleItem");
            }
            "minecraft:villager" => {
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
            _ => {}
        }
    }
}

/// Whether the block entity id is registered in the schema at `k` (before 1.14).
pub(super) fn legacy_block_entity_known(id: &str, k: Key) -> bool {
    if k < key(704, 0) {
        return crate::datafix::tables::tables()
            .block_entities_v99
            .contains(&id);
    }
    match id {
        "minecraft:furnace"
        | "minecraft:chest"
        | "minecraft:ender_chest"
        | "minecraft:jukebox"
        | "minecraft:dispenser"
        | "minecraft:dropper"
        | "minecraft:sign"
        | "minecraft:mob_spawner"
        | "minecraft:piston"
        | "minecraft:brewing_stand"
        | "minecraft:enchanting_table"
        | "minecraft:end_portal"
        | "minecraft:beacon"
        | "minecraft:skull"
        | "minecraft:daylight_detector"
        | "minecraft:hopper"
        | "minecraft:comparator"
        | "minecraft:banner"
        | "minecraft:structure_block"
        | "minecraft:end_gateway"
        | "minecraft:command_block" => true,
        // V808 adds the shulker box.
        "minecraft:shulker_box" => k >= key(808, 0),
        // Registered by V1125 and dropped by V1451_5.
        "minecraft:bed" => k >= key(1125, 0),
        "minecraft:noteblock" | "minecraft:flower_pot" => k < key(1451, 5),
        "minecraft:trapped_chest" => k >= key(1451, 0),
        "DUMMY" => k >= key(1466, 0),
        "minecraft:conduit" => k >= key(1481, 0),
        _ => false,
    }
}

/// Whether `id` is an entity registered by the schema at `k` (V705 to V1510).
pub(in crate::datafix) fn legacy_entity_known(id: &str, k: Key) -> bool {
    if k < key(705, 0) {
        return old_entity_known(id, k);
    }
    static ERAS: OnceLock<[HashSet<String>; 5]> = OnceLock::new();
    let eras = ERAS.get_or_init(|| {
        let mut set: HashSet<String> = crate::datafix::tables::tables()
            .entities_v1460
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let rename = |set: &mut HashSet<String>, from: &str, to: &str| {
            if set.remove(from) {
                set.insert(to.to_owned());
            }
        };
        let base = set.clone();
        // V1470
        for id in [
            "turtle",
            "cod_mob",
            "tropical_fish",
            "salmon_mob",
            "puffer_fish",
            "phantom",
            "dolphin",
            "drowned",
            "trident",
        ] {
            set.insert(format!("minecraft:{id}"));
        }
        let v1470 = set.clone();
        rename(&mut set, "minecraft:puffer_fish", "minecraft:pufferfish");
        let v1483 = set.clone();
        rename(&mut set, "minecraft:cod_mob", "minecraft:cod");
        rename(&mut set, "minecraft:salmon_mob", "minecraft:salmon");
        let v1486 = set.clone();
        for (from, to) in [
            ("commandblock_minecart", "command_block_minecart"),
            ("ender_crystal", "end_crystal"),
            ("snowman", "snow_golem"),
            ("evocation_illager", "evoker"),
            ("evocation_fangs", "evoker_fangs"),
            ("illusion_illager", "illusioner"),
            ("vindication_illager", "vindicator"),
            ("villager_golem", "iron_golem"),
            ("xp_orb", "experience_orb"),
            ("xp_bottle", "experience_bottle"),
            ("eye_of_ender_signal", "eye_of_ender"),
            ("fireworks_rocket", "firework_rocket"),
        ] {
            rename(
                &mut set,
                &format!("minecraft:{from}"),
                &format!("minecraft:{to}"),
            );
        }
        [base, v1470, v1483, v1486, set]
    });
    let era = if k < key(1470, 0) {
        0
    } else if k < key(1483, 0) {
        1
    } else if k < key(1486, 0) {
        2
    } else if k < key(1510, 0) {
        3
    } else {
        4
    };
    eras[era].contains(id)
}

/// Whether the entity tree's own entity reads as a typed value at `k`.
pub(in crate::datafix) fn entity_tree_known(tree: &Tag, k: Key) -> bool {
    tree.get(UNTYPED).is_none()
        && legacy_entity_known(&namespaced(&as_str(tree.get("id")).unwrap_or_default()), k)
}

/// Whether `id` is an entity registered by the schemas V99 to V703 (CamelCase ids).
fn old_entity_known(id: &str, k: Key) -> bool {
    match id {
        "Minecart" => k < key(107, 0),
        "TippedArrow" => k < key(143, 0),
        "PolarBear" => k >= key(501, 0),
        "ElderGuardian" => k >= key(700, 0),
        "WitherSkeleton" | "Stray" => k >= key(701, 0),
        "ZombieVillager" | "Husk" => k >= key(702, 0),
        "Horse" | "Donkey" | "Mule" | "ZombieHorse" | "SkeletonHorse" => k >= key(703, 0),
        "EntityHorse" => k < key(703, 0),
        _ => crate::datafix::tables::tables().entities_v99.contains(&id),
    }
}
