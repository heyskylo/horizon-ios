//! Result rules of the workstation menus, ported from vanilla's
//! `EnchantmentMenu`, `AnvilMenu`, `GrindstoneMenu`, `SmithingMenu` (with
//! `SmithingTransformRecipe` / `SmithingTrimRecipe`) and `StonecutterMenu`.
//! Rendering and clicks live in the game; this module is pure item logic.
use crate::{
    crafting::Ingredient,
    enchant::Enchantments,
    item_data::{self, Levels},
    items::{ItemStack, Items},
};
use horizon_core::random::{LegacyRandom, RandomSource};
use horizon_world::loot::PackOverlay;
use serde_json::Value;
use std::{fs, path::Path, sync::Arc};

/// Everything the workstation menus read, loaded once with the resources.
pub struct StationData {
    /// The same registry `Items` carries (`Items::enchantments`).
    pub enchantments: std::sync::Arc<Enchantments>,
    pub recipes: StationRecipes,
    pub brewing: crate::brewing::Brewing,
    pub trades: crate::merchant::Trades,
    pub banners: crate::banner::Banners,
    /// Every recipe's learnability and display count (the server recipe book).
    pub recipe_index: crate::recipe_book::RecipeIndex,
}
impl StationData {
    pub fn load(root: &Path, items: &Items, overlay: &PackOverlay) -> Self {
        let defaults = horizon_world::item_defaults::shared(root).ok();
        Self {
            enchantments: std::sync::Arc::clone(&items.enchantments),
            recipes: StationRecipes::load_with_defaults(root, items, overlay, defaults.as_deref()),
            brewing: crate::brewing::Brewing::load(root, items, overlay),
            trades: crate::merchant::Trades::load(root, overlay),
            banners: crate::banner::Banners::load_with_defaults(root, defaults.as_deref()),
            recipe_index: crate::recipe_book::RecipeIndex::load(root, overlay),
        }
    }
}

// ---------------------------------------------------------------- enchanting

/// `EnchantingTableBlock.BOOKSHELF_OFFSETS`: the 5x5 ring two blocks out, two high.
pub fn bookshelf_offsets() -> Vec<[i32; 3]> {
    let mut v = Vec::new();
    for x in -2i32..=2 {
        for z in -2i32..=2 {
            if x.abs() == 2 || z.abs() == 2 {
                for y in 0..=1 {
                    v.push([x, y, z]);
                }
            }
        }
    }
    v
}
/// The three offers of `EnchantmentMenu.slotsChanged`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EnchantOffers {
    pub costs: [i32; 3],
    /// Shown clue: enchantment and level (`enchantClue` / `levelClue`).
    pub clues: [Option<(Arc<str>, u32)>; 3],
}
/// `EnchantmentMenu.getEnchantmentList`.
pub fn enchantment_list(
    e: &Enchantments,
    items: &Items,
    seed: i32,
    slot: usize,
    cost: i32,
    item: &str,
) -> Levels {
    let mut random = LegacyRandom::new(i64::from(seed.wrapping_add(slot as i32)));
    let pool = e.tag("minecraft:in_enchanting_table").to_vec();
    let mut list = e.select(items, &mut random, item, cost, &pool);
    if item == "minecraft:book" && list.len() > 1 {
        let i = random.next_int_bound(list.len() as i32).unwrap_or(0) as usize;
        list.remove(i);
    }
    list
}
/// `EnchantmentMenu.slotsChanged` for an enchantable item.
pub fn enchant_offers(
    e: &Enchantments,
    items: &Items,
    seed: i32,
    bookcases: i32,
    stack: Option<&ItemStack>,
) -> EnchantOffers {
    let mut offers = EnchantOffers::default();
    let Some(stack) = stack else { return offers };
    // ItemStack.isEnchantable: enchantable component and no enchantments yet.
    if e.enchantable(&stack.item) == 0 || !stack.enchantments().is_empty() {
        return offers;
    }
    let mut random = LegacyRandom::new(i64::from(seed));
    for i in 0..3 {
        offers.costs[i] = e.enchantment_cost(&mut random, i, bookcases, &stack.item);
        if offers.costs[i] < i as i32 + 1 {
            offers.costs[i] = 0;
        }
    }
    for i in 0..3 {
        offers.clues[i] = enchant_clue(e, items, seed, i, offers.costs[i], stack);
    }
    offers
}
/// `EnchantmentMenu.slotsChanged`'s clue for offer `slot` at `cost`: one
/// enchantment of the list that cost would give, or None for no offer.
pub fn enchant_clue(
    e: &Enchantments,
    items: &Items,
    seed: i32,
    slot: usize,
    cost: i32,
    stack: &ItemStack,
) -> Option<(Arc<str>, u32)> {
    if cost <= 0 {
        return None;
    }
    // getEnchantmentList reseeds the menu random; the clue draws continue from it.
    let mut list_random = LegacyRandom::new(i64::from(seed.wrapping_add(slot as i32)));
    let pool = e.tag("minecraft:in_enchanting_table").to_vec();
    let mut list = e.select(items, &mut list_random, &stack.item, cost, &pool);
    if &*stack.item == "minecraft:book" && list.len() > 1 {
        let r = list_random.next_int_bound(list.len() as i32).unwrap_or(0) as usize;
        list.remove(r);
    }
    if list.is_empty() {
        return None;
    }
    let pick = list_random.next_int_bound(list.len() as i32).unwrap_or(0) as usize;
    Some(list[pick].clone())
}
/// `EnchantmentMenu.clickMenuButton` result: the enchanted item, or None.
pub fn apply_enchantment(
    e: &Enchantments,
    items: &Items,
    seed: i32,
    slot: usize,
    cost: i32,
    stack: &ItemStack,
) -> Option<ItemStack> {
    let list = enchantment_list(e, items, seed, slot, cost, &stack.item);
    if list.is_empty() {
        return None;
    }
    let mut out = if &*stack.item == "minecraft:book" {
        stack.transmute("minecraft:enchanted_book", stack.count)
    } else {
        stack.clone()
    };
    for (id, level) in list {
        out.enchant(&id, level);
    }
    Some(out)
}

// ---------------------------------------------------------------- anvil

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnvilResult {
    pub result: Option<ItemStack>,
    /// Level cost (the `cost` data slot).
    pub cost: i32,
    /// Material items consumed from the second slot on take.
    pub repair_item_count_cost: u32,
    pub only_renaming: bool,
}
/// `DataComponents.MAX_DAMAGE` of a stack: its patch, else the item default
/// (0 when the component is absent or removed).
fn max_damage(items: &Items, stack: &ItemStack) -> i32 {
    let data = stack.data();
    if data.is_some_and(|d| d.other("!minecraft:max_damage").is_some()) {
        return 0;
    }
    match data.and_then(|d| d.other("minecraft:max_damage")) {
        Some(tag) => component_int(tag).filter(|v| *v >= 1).unwrap_or(0),
        None => items.get(&stack.item).map_or(0, |i| i.max_damage as i32),
    }
}
fn component_int(tag: &horizon_world::nbt::Tag) -> Option<i32> {
    use horizon_world::nbt::Tag;
    match tag {
        Tag::Byte(v) => Some(i32::from(*v)),
        Tag::Short(v) => Some(i32::from(*v)),
        Tag::Int(v) => Some(*v),
        _ => None,
    }
}
/// An item holder set in a component: one id or tag, or a list of them.
fn component_items(tag: &horizon_world::nbt::Tag) -> Vec<String> {
    use horizon_world::nbt::Tag;
    match tag {
        Tag::String(s) => vec![s.to_string_lossy().to_owned()],
        Tag::List(list) => list
            .iter()
            .filter_map(|item| match item {
                Tag::String(s) => Some(s.to_string_lossy().to_owned()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}
/// `ItemStack.get(DataComponents.REPAIRABLE)`: the stack's component, else
/// the item's default; a removed component leaves none.
fn repairable(e: &Enchantments, stack: &ItemStack) -> Vec<String> {
    let data = stack.data();
    if data.is_some_and(|d| d.other("!minecraft:repairable").is_some()) {
        return Vec::new();
    }
    if let Some(tag) = data.and_then(|d| d.other("minecraft:repairable")) {
        return tag.get("items").map(component_items).unwrap_or_default();
    }
    e.props(&stack.item)
        .map(|p| p.repairable.clone())
        .unwrap_or_default()
}
/// `ItemStack.getDamageValue`: the damage clamped to `[0, max_damage]`.
fn damage_value(items: &Items, stack: &ItemStack) -> i32 {
    (stack.damage.min(i32::MAX as u32) as i32).clamp(0, max_damage(items, stack).max(0))
}
/// `ItemStack.isDamageableItem`: max damage and damage components present,
/// and not `minecraft:unbreakable`.
pub fn is_damageable(items: &Items, stack: &ItemStack) -> bool {
    max_damage(items, stack) > 0
        && stack.data().is_none_or(|d| {
            d.other("minecraft:unbreakable").is_none() && d.other("!minecraft:damage").is_none()
        })
}
/// `ItemStack.setDamageValue`: stored clamped to `[0, max_damage]`.
fn set_damage_value(items: &Items, stack: &mut ItemStack, damage: i32) {
    stack.damage = damage.clamp(0, max_damage(items, stack).max(0)) as u32;
}
/// `ItemStack.isValidRepairItem`: `REPAIRABLE` from the stack's components,
/// not only the item's default.
pub fn is_valid_repair_item(
    e: &Enchantments,
    items: &Items,
    stack: &ItemStack,
    material: &ItemStack,
) -> bool {
    Enchantments::item_in(items, &repairable(e, stack), &material.item)
}
/// `stack.has(STORED_ENCHANTMENTS)`: the enchanted book's default component
/// unless removed, or an explicitly set one, even when empty.
fn has_stored_enchantments(stack: &ItemStack) -> bool {
    let data = stack.data();
    if data.is_some_and(|d| d.other("!minecraft:stored_enchantments").is_some()) {
        return false;
    }
    &*stack.item == "minecraft:enchanted_book"
        || !stack.stored_enchantments().is_empty()
        || data.is_some_and(|d| d.other("minecraft:stored_enchantments").is_some())
}
/// `AnvilMenu.calculateIncreasedRepairCost`.
pub fn increased_repair_cost(cost: u32) -> u32 {
    (u64::from(cost) * 2 + 1).min(i32::MAX as u64) as u32
}
/// `ItemStack.getHoverName`: the custom name, a potion's or tipped arrow's
/// contents, a written book's title, the `item_name` component, else the
/// item's or head owner's name.
pub fn hover_name(names: &crate::creative::Names, stack: &ItemStack) -> String {
    if let Some(name) = stack.custom_name() {
        return name;
    }
    if let Some(key) = crate::brewing::contents_name_key(stack) {
        return names
            .display_get(&key)
            .unwrap_or_else(|| names.display_item(&stack.item));
    }
    if let Some(title) = stack
        .data()
        .and_then(|d| d.written.as_ref())
        .map(|w| w.title.as_str())
        .filter(|t| !t.trim().is_empty())
    {
        return title.to_owned();
    }
    if let Some(name) = stack.item_name() {
        return name;
    }
    if stack.data().is_some_and(|d| d.ominous_banner) {
        return names
            .display_get("block.minecraft.ominous_banner")
            .unwrap_or_else(|| "Ominous Banner".to_owned());
    }
    if let Some(head) = crate::item_tooltip::player_head_name(stack) {
        return head.title(&|key| names.display_get(key));
    }
    names.display_item(&stack.item)
}
/// `AnvilMenu.createResult`. `name` is the rename box text (None = untouched).
pub fn anvil_result(
    e: &Enchantments,
    items: &Items,
    input: Option<&ItemStack>,
    addition: Option<&ItemStack>,
    name: Option<&str>,
    hover_name: &str,
    creative: bool,
) -> AnvilResult {
    let fail = AnvilResult::default();
    let Some(input) = input else { return fail };
    // EnchantmentHelper.canStoreEnchantments: enchantable or enchanted book... every
    // item with an enchantments component, which is all of them in 26.3.
    let mut result = input.clone();
    let mut enchantments: Levels = input.crafting_enchantments().to_vec();
    let mut price: i64 = 0;
    let tax: i64 =
        i64::from(input.repair_cost()) + i64::from(addition.map_or(0, ItemStack::repair_cost));
    let mut repair_item_count_cost = 0;
    let mut naming_cost = 0;
    if let Some(addition) = addition {
        // addition.has(STORED_ENCHANTMENTS): an enchanted book (its default
        // component), or any item given stored enchantments.
        let using_book = has_stored_enchantments(addition);
        if is_damageable(items, &result) && is_valid_repair_item(e, items, input, addition) {
            let max = max_damage(items, &result);
            let mut repair = damage_value(items, &result).min(max / 4);
            if repair <= 0 {
                return fail;
            }
            let mut count = 0;
            while repair > 0 && count < addition.count {
                let damage = damage_value(items, &result) - repair;
                set_damage_value(items, &mut result, damage);
                price += 1;
                repair = damage_value(items, &result).min(max / 4);
                count += 1;
            }
            repair_item_count_cost = count;
        } else {
            if !using_book && (result.item != addition.item || !is_damageable(items, &result)) {
                return fail;
            }
            if is_damageable(items, &result) && !using_book {
                let max = max_damage(items, &result);
                let remaining1 = max_damage(items, input) - damage_value(items, input);
                let remaining2 = max_damage(items, addition) - damage_value(items, addition);
                let additional = remaining2 + max * 12 / 100;
                let remaining = remaining1 + additional;
                let damage = (max - remaining).max(0);
                if damage < damage_value(items, &result) {
                    set_damage_value(items, &mut result, damage);
                    price += 2;
                }
            }
            let mut any_compatible = false;
            let mut any_incompatible = false;
            for (id, level) in addition.crafting_enchantments() {
                let current = enchantments
                    .iter()
                    .find(|(x, _)| x == id)
                    .map_or(0, |(_, l)| *l);
                let mut level = if current == *level {
                    level + 1
                } else {
                    (*level).max(current)
                };
                let Some(def) = e.get(id) else { continue };
                let mut compatible = e.can_enchant(items, id, &input.item);
                if creative || &*input.item == "minecraft:enchanted_book" {
                    compatible = true;
                }
                for (other, _) in &enchantments {
                    if other != id && !e.compatible(id, other) {
                        compatible = false;
                        price += 1;
                    }
                }
                if !compatible {
                    any_incompatible = true;
                } else {
                    any_compatible = true;
                    level = level.min(def.max_level);
                    item_data::set_level(&mut enchantments, id, level);
                    let mut fee = def.anvil_cost as i64;
                    if using_book {
                        fee = (fee / 2).max(1);
                    }
                    price += fee * i64::from(level);
                    if input.count > 1 {
                        price = 40;
                    }
                }
            }
            if any_incompatible && !any_compatible {
                return fail;
            }
        }
    }
    match name {
        Some(n) if !n.trim().is_empty() => {
            if n != hover_name {
                naming_cost = 1;
                price += 1;
                result.data_mut().custom_name = Some(horizon_world::nbt::Tag::String(n.into()));
            }
        }
        _ => {
            if input.custom_name().is_some() {
                naming_cost = 1;
                price += 1;
                result.data_mut().custom_name = None;
            }
        }
    }
    let mut cost = if price <= 0 {
        0
    } else {
        (tax + price).clamp(0, i64::from(i32::MAX)) as i32
    };
    let mut out = (price > 0).then_some(result);
    let mut only_renaming = false;
    if naming_cost == price && naming_cost > 0 {
        if cost >= 40 {
            cost = 39;
        }
        only_renaming = true;
    }
    if cost >= 40 && !creative {
        out = None;
    }
    if let Some(r) = out.as_mut() {
        let mut base = r
            .repair_cost()
            .max(addition.map_or(0, ItemStack::repair_cost));
        if naming_cost != price || naming_cost == 0 {
            base = increased_repair_cost(base);
        }
        r.data_mut().repair_cost = base;
        r.set_crafting_enchantments(enchantments);
        r.tidy();
    }
    AnvilResult {
        result: out,
        cost,
        repair_item_count_cost,
        only_renaming,
    }
}
impl AnvilResult {
    /// NeoForge's `CommonHooks.onAnvilUpdate` once the mods answered
    /// `AnvilUpdateEvent` about this result (vanilla's): a cancel leaves no
    /// result and both costs at 0. Otherwise their output, their level cost
    /// through `AnvilMenu.setCost` (a negative one is 0) and their material
    /// cost, where 0 or less is vanilla's 0 (`onTake` takes the whole right
    /// stack, unless the anvil only renames). `only_renaming` stays vanilla's.
    pub fn answer_update(
        &mut self,
        canceled: bool,
        output: Option<ItemStack>,
        xp_cost: i32,
        material_cost: i32,
    ) {
        if canceled {
            self.result = None;
            self.cost = 0;
            self.repair_item_count_cost = 0;
            return;
        }
        self.result = output;
        self.cost = xp_cost.max(0);
        self.repair_item_count_cost = u32::try_from(material_cost).unwrap_or(0);
    }
}

// ---------------------------------------------------------------- grindstone

/// `GrindstoneMenu` slot filter: damageable or enchanted.
pub fn grindstone_accepts(items: &Items, stack: &ItemStack) -> bool {
    is_damageable(items, stack) || stack.has_any_enchantments()
}
fn remove_non_curses(e: &Enchantments, mut item: ItemStack) -> ItemStack {
    let kept: Levels = item
        .crafting_enchantments()
        .iter()
        .filter(|(id, _)| e.is_curse(id))
        .cloned()
        .collect();
    let n = kept.len();
    item.set_crafting_enchantments(kept);
    if &*item.item == "minecraft:enchanted_book" && item.stored_enchantments().is_empty() {
        item = item.transmute("minecraft:book", item.count);
    }
    let mut cost = 0;
    for _ in 0..n {
        cost = increased_repair_cost(cost);
    }
    item.data_mut().repair_cost = cost;
    item.tidy();
    item
}
/// `GrindstoneMenu.computeResult`.
pub fn grindstone_result(
    e: &Enchantments,
    items: &Items,
    input: Option<&ItemStack>,
    additional: Option<&ItemStack>,
) -> Option<ItemStack> {
    if input.is_none() && additional.is_none() {
        return None;
    }
    if input.is_some_and(|s| s.count > 1) || additional.is_some_and(|s| s.count > 1) {
        return None;
    }
    match (input, additional) {
        (Some(a), Some(b)) => {
            if a.item != b.item {
                return None;
            }
            let durability = max_damage(items, a).max(max_damage(items, b));
            let remaining = (max_damage(items, a) - damage_value(items, a))
                + (max_damage(items, b) - damage_value(items, b))
                + durability * 5 / 100;
            let mut count = 1;
            if !is_damageable(items, a) {
                let stack_max = items.get(&a.item).map_or(64, |i| i.max_stack);
                if stack_max < 2 || a != b {
                    return None;
                }
                count = 2;
            }
            let mut out = ItemStack { count, ..a.clone() };
            if is_damageable(items, &out) {
                // GrindstoneMenu.mergeItems writes the larger MAX_DAMAGE first. The
                // patch drops a value equal to the item's own (PatchedDataComponentMap.set).
                let default = items.get(&out.item).map_or(0, |i| i.max_damage as i32);
                let data = out.data_mut();
                data.set_other("!minecraft:max_damage", None);
                data.set_other(
                    "minecraft:max_damage",
                    (durability != default).then_some(horizon_world::nbt::Tag::Int(durability)),
                );
                set_damage_value(items, &mut out, durability - remaining);
            }
            // mergeEnchantsFrom
            let mut levels = out.crafting_enchantments().to_vec();
            for (id, l) in b.crafting_enchantments() {
                if !e.is_curse(id) || !levels.iter().any(|(x, _)| x == id) {
                    item_data::upgrade(&mut levels, id, *l);
                }
            }
            out.set_crafting_enchantments(levels);
            Some(remove_non_curses(e, out))
        }
        (Some(s), None) | (None, Some(s)) => s
            .has_any_enchantments()
            .then(|| remove_non_curses(e, s.clone())),
        (None, None) => None,
    }
}
/// The grindstone result slot's `getExperienceAmount` before its roll: the
/// minimum costs of the inputs' non-curse enchantments.
pub fn grindstone_xp_base(
    e: &Enchantments,
    input: Option<&ItemStack>,
    additional: Option<&ItemStack>,
) -> i32 {
    let from = |s: Option<&ItemStack>| -> i32 {
        s.map_or(0, |s| {
            s.crafting_enchantments()
                .iter()
                .filter(|(id, _)| !e.is_curse(id))
                .map(|(id, l)| e.get(id).map_or(0, |d| d.min_cost.calculate(*l)))
                .sum()
        })
    };
    from(input) + from(additional)
}
/// `getExperienceAmount`'s roll of the sum: `half + nextInt(half)`.
pub fn grindstone_xp_roll(amount: i32, random: &mut LegacyRandom) -> u32 {
    if amount > 0 {
        let half = (f64::from(amount) / 2.0).ceil() as i32;
        (half + random.next_int_bound(half).unwrap_or(0)) as u32
    } else {
        0
    }
}
/// Experience from the grindstone result slot's `getExperienceAmount`.
pub fn grindstone_experience(
    e: &Enchantments,
    random: &mut LegacyRandom,
    input: Option<&ItemStack>,
    additional: Option<&ItemStack>,
) -> u32 {
    grindstone_xp_roll(grindstone_xp_base(e, input, additional), random)
}

// ---------------------------------------------------------------- smithing & stonecutting

#[derive(Clone, Debug)]
pub enum Smithing {
    Transform {
        id: String,
        template: Option<Ingredient>,
        base: Ingredient,
        addition: Option<Ingredient>,
        result: SmithingResult,
    },
    Trim {
        id: String,
        template: Ingredient,
        base: Ingredient,
        addition: Ingredient,
        pattern: crate::item_data::trim::Holder<crate::item_data::trim::Pattern>,
    },
}
/// A smithing transform's `ItemStackTemplate`, retaining the patch as well as
/// its display stack so explicit zeroes and removals override the input.
#[derive(Clone, Debug)]
pub struct SmithingResult {
    stack: ItemStack,
    patch: horizon_world::nbt::Tag,
    defaults: Value,
}
impl SmithingResult {
    fn parse(v: &Value, defaults: &horizon_world::item_defaults::Defaults) -> Option<Self> {
        let item = horizon_world::loot::recipe_result_id(v)?;
        if item == "minecraft:air" {
            return None;
        }
        let count = crate::crafting::template_count(v)?;
        let patch =
            crate::scoreboard::json_to_tag(v.get("components").unwrap_or(&serde_json::json!({})));
        if !matches!(patch, horizon_world::nbt::Tag::Compound(_)) {
            return None;
        }
        let (damage, extra) = item_data::decode_components(Some(&patch));
        Some(Self {
            stack: ItemStack {
                item: Arc::from(item),
                count,
                damage,
                extra,
            },
            patch,
            defaults: defaults.get(item)?.clone(),
        })
    }
    /// `ItemStackTemplate.create`, used by the native recipe display export.
    pub fn create(&self) -> Option<ItemStack> {
        self.assemble(&ItemStack::new("minecraft:air", 0))
    }
    /// `TransmuteRecipe.createWithOriginalComponents` and `ItemStackTemplate.apply`.
    fn assemble(&self, base: &ItemStack) -> Option<ItemStack> {
        use horizon_world::nbt::Tag;
        let mut patch = match item_data::encode_components(base) {
            Some(Tag::Compound(map)) => map,
            _ => Default::default(),
        };
        if let Tag::Compound(over) = &self.patch {
            for (key, value) in over {
                let key = key.to_string_lossy();
                if let Some(id) = key.strip_prefix('!') {
                    patch.remove(&horizon_world::nbt::JavaString::from(id));
                    if self.defaults.get(id).is_some() {
                        patch.insert(key.as_str().into(), value.clone());
                    }
                } else {
                    patch.remove(&horizon_world::nbt::JavaString::from(
                        format!("!{key}").as_str(),
                    ));
                    patch.insert(key.as_str().into(), value.clone());
                }
            }
        }
        // Patches are relative to the result item's defaults, as on vanilla's
        // PatchedDataComponentMap; don't retain an explicit default as a patch.
        patch.retain(|key, value| {
            let key = key.to_string_lossy();
            if let Some(id) = key.strip_prefix('!') {
                self.defaults.get(id).is_some()
            } else {
                self.defaults
                    .get(key.as_str())
                    .is_none_or(|default| crate::scoreboard::json_to_tag(default) != *value)
            }
        });
        let components = Tag::Compound(patch);
        let number = |id: &str, fallback: i64| {
            if components.get(&format!("!{id}")).is_some() {
                return fallback;
            }
            match components.get(id) {
                Some(Tag::Int(n)) => i64::from(*n),
                _ => self.defaults[id].as_i64().unwrap_or(fallback),
            }
        };
        let max_stack = number("minecraft:max_stack_size", 1);
        let damageable = components.get("!minecraft:max_damage").is_none()
            && (components.get("minecraft:max_damage").is_some()
                || self.defaults.get("minecraft:max_damage").is_some());
        if i64::from(self.stack.count) > max_stack || (damageable && max_stack > 1) {
            return None;
        }
        let (damage, extra) = item_data::decode_components(Some(&components));
        Some(ItemStack {
            item: self.stack.item.clone(),
            count: self.stack.count,
            damage,
            extra,
        })
    }
}
#[derive(Clone, Debug)]
pub struct Stonecutting {
    pub id: String,
    pub input: Ingredient,
    pub result: ItemStack,
}
#[derive(Default)]
pub struct StationRecipes {
    pub smithing: Vec<Smithing>,
    /// Sorted by recipe id, the registry order vanilla lists them in.
    pub stonecutting: Vec<Stonecutting>,
    /// `trim_material` registry: id -> (asset name, description).
    pub trim_materials: Vec<(String, String)>,
}
fn ingredient(v: &Value, items: &Items) -> Ingredient {
    Ingredient::parse(v, items).unwrap_or_else(|| Ingredient::of(Vec::new()))
}
impl StationRecipes {
    pub fn load(root: &Path, items: &Items, overlay: &PackOverlay) -> Self {
        Self::load_with_defaults(root, items, overlay, None)
    }
    pub(crate) fn load_with_defaults(
        root: &Path,
        items: &Items,
        overlay: &PackOverlay,
        defaults: Option<&horizon_world::item_defaults::Defaults>,
    ) -> Self {
        // Vanilla's recipes, then the content mods' (docs/modpacks.md).
        let mut files = Vec::new();
        let namespaces =
            std::iter::once("minecraft".to_owned()).chain(crate::mod_content::namespaces(root));
        // NeoForge's load conditions: a file whose don't hold is silently
        // absent (`mod_loaded` asks which mods' data is in this root).
        let mods: Vec<String> = {
            let mut mods = horizon_items::content_namespaces::mod_ids(root);
            if !mods.iter().any(|m| m == "minecraft") {
                mods.push("minecraft".to_owned());
            }
            mods
        };
        let conditions_hold = |v: &Value| {
            horizon_world::conditions::conditions_hold(v, &|id: &str| mods.iter().any(|m| m == id))
        };
        for namespace in namespaces {
            let dir = root.join(format!("data/{namespace}/recipe"));
            if let Ok(folder) = crate::json_folder::read(&dir) {
                files.push((namespace, folder));
            }
        }
        let mut sources: Vec<(String, Value)> = files
            .iter()
            .flat_map(|(namespace, folder)| folder.files.iter().map(move |f| (namespace, f)))
            .filter_map(|(namespace, file)| {
                let v = serde_json::from_slice::<Value>(file.bytes().ok()?).ok()?;
                Some((format!("{namespace}:{}", file.name), v))
            })
            .collect();
        // The world's packs' recipes over the game's own: the top pack's
        // file for each id replaces it (`SimpleJsonResourceReloadListener`).
        // The gate answers the merged file, as the crafting loader's does:
        // a false-conditioned pack file leaves the lower one gone too.
        for (id, files) in overlay.kind("recipe") {
            let Ok(v) = serde_json::from_slice::<Value>(PackOverlay::top(files)) else {
                continue;
            };
            match sources.iter_mut().find(|(n, _)| n == id) {
                Some(slot) => slot.1 = v,
                None => sources.push((id.to_owned(), v)),
            }
        }
        // The data's recipe filters, over the final set.
        let filters = horizon_items::recipe_filters::RecipeFilters::load(root, overlay);
        sources.retain_mut(|(id, v)| conditions_hold(v) && filters.apply(id, v));
        // Registration order, not the game's files before the packs': the
        // stonecutter lists its recipes in it and smithing takes the first match.
        sources.sort_by_cached_key(|(id, _)| crate::crafting::registration_key(id));
        let owned_defaults;
        let defaults = match defaults {
            Some(defaults) => defaults,
            None => {
                owned_defaults = horizon_world::item_defaults::shared(root).unwrap_or_default();
                &owned_defaults
            }
        };
        let mut smithing = Vec::new();
        let mut stonecutting = Vec::new();
        for (id, v) in sources {
            match v["type"].as_str().unwrap_or("") {
                "minecraft:smithing_transform" => {
                    // Only omitted optional ingredients mean an empty slot;
                    // malformed present ingredients must not become optional.
                    let parse_optional = |key: &str| match v.get(key) {
                        None => Some(None),
                        Some(value) => Ingredient::parse(value, items).map(Some),
                    };
                    if let (Some(template), Some(base), Some(addition), Some(result)) = (
                        parse_optional("template"),
                        Ingredient::parse(&v["base"], items),
                        parse_optional("addition"),
                        SmithingResult::parse(&v["result"], defaults),
                    ) {
                        smithing.push(Smithing::Transform {
                            id,
                            template,
                            base,
                            addition,
                            result,
                        });
                    }
                }
                "minecraft:smithing_trim" => {
                    // TrimPattern.CODEC: one holder, named or inline; a missing
                    // or malformed pattern fails the recipe.
                    let pattern = crate::scoreboard::json_to_tag(&v["pattern"]);
                    if let Some(pattern) = crate::item_data::trim::pattern_from_nbt(&pattern) {
                        smithing.push(Smithing::Trim {
                            id,
                            template: ingredient(&v["template"], items),
                            base: ingredient(&v["base"], items),
                            addition: ingredient(&v["addition"], items),
                            pattern,
                        });
                    }
                }
                "minecraft:stonecutting" => {
                    // A result count outside 1..=99 fails vanilla's codec: no recipe.
                    if let Some(count) = crate::crafting::template_count(&v["result"]) {
                        stonecutting.push(Stonecutting {
                            id,
                            input: ingredient(&v["ingredient"], items),
                            result: ItemStack::new(
                                horizon_world::loot::recipe_result_id(&v["result"])
                                    .unwrap_or("minecraft:air"),
                                count,
                            ),
                        });
                    }
                }
                _ => {}
            }
        }
        let mut trim_materials = Vec::new();
        let mut tm: Vec<_> = fs::read_dir(root.join("data/minecraft/trim_material"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect();
        tm.sort();
        for p in tm {
            if let Ok(bytes) = fs::read(&p)
                && let Ok(v) = serde_json::from_slice::<Value>(&bytes)
            {
                let id = format!(
                    "minecraft:{}",
                    p.file_stem().and_then(|s| s.to_str()).unwrap_or("")
                );
                trim_materials.push((
                    id,
                    v["description"]["translate"]
                        .as_str()
                        .unwrap_or("")
                        .to_owned(),
                ));
            }
        }
        Self {
            smithing,
            stonecutting,
            trim_materials,
        }
    }
    /// Recipes whose input accepts `stack`, in display order.
    pub fn stonecutter(&self, stack: &ItemStack) -> Vec<&Stonecutting> {
        self.stonecutting
            .iter()
            .filter(|r| r.input.test(stack))
            .collect()
    }
    fn slot_ok(&self, slot: usize, stack: &ItemStack) -> bool {
        self.smithing.iter().any(|r| {
            let (t, b, a) = match r {
                Smithing::Transform {
                    template,
                    base,
                    addition,
                    ..
                } => (template.as_ref(), base, addition.as_ref()),
                Smithing::Trim {
                    template,
                    base,
                    addition,
                    ..
                } => (Some(template), base, Some(addition)),
            };
            [t, Some(b), a][slot].is_some_and(|ingredient| ingredient.test(stack))
        })
    }
    /// `RecipePropertySet` SMITHING_TEMPLATE / BASE / ADDITION.
    pub fn smithing_accepts(&self, slot: usize, stack: &ItemStack) -> bool {
        slot < 3 && self.slot_ok(slot, stack)
    }
    /// `SmithingMenu.createResult`.
    pub fn smithing_result(
        &self,
        e: &Enchantments,
        template: Option<&ItemStack>,
        base: Option<&ItemStack>,
        addition: Option<&ItemStack>,
    ) -> Option<ItemStack> {
        self.smithing_recipe(e, template, base, addition)
            .map(|(_, result)| result)
    }
    /// `SmithingMenu.createResult` with the id of the recipe that made the result.
    pub fn smithing_recipe(
        &self,
        e: &Enchantments,
        template: Option<&ItemStack>,
        base: Option<&ItemStack>,
        addition: Option<&ItemStack>,
    ) -> Option<(&str, ItemStack)> {
        let nonempty = |stack: &ItemStack| stack.count > 0 && &*stack.item != "minecraft:air";
        let base = base.filter(|s| nonempty(s))?;
        let optional_matches = |ingredient: &Option<Ingredient>, stack: Option<&ItemStack>| match (
            ingredient,
            stack.filter(|s| nonempty(s)),
        ) {
            (None, None) => true,
            (Some(ingredient), Some(stack)) => ingredient.test(stack),
            _ => false,
        };
        for r in &self.smithing {
            match r {
                Smithing::Transform {
                    id,
                    template: t,
                    base: b,
                    addition: a,
                    result,
                } => {
                    if optional_matches(t, template)
                        && b.test(base)
                        && optional_matches(a, addition)
                    {
                        return result.assemble(base).map(|result| (id.as_str(), result));
                    }
                }
                Smithing::Trim {
                    id,
                    template: t,
                    base: b,
                    addition: a,
                    pattern,
                    ..
                } => {
                    if let (Some(template), Some(addition)) = (template, addition)
                        && t.test(template)
                        && b.test(base)
                        && a.test(addition)
                    {
                        return apply_trim(e, base, addition, pattern).map(|r| (id.as_str(), r));
                    }
                }
            }
        }
        None
    }
    /// `SmithingTrimDemoSlotDisplay.resolveForFirstStack` of a trim recipe:
    /// a random base trimmed with a random material (`pick(n)` draws below n).
    pub fn trim_demo(
        &self,
        e: &Enchantments,
        id: &str,
        pick: &mut dyn FnMut(usize) -> usize,
    ) -> Option<ItemStack> {
        let (base, addition, pattern) = self.smithing.iter().find_map(|r| match r {
            Smithing::Trim {
                id: i,
                base,
                addition,
                pattern,
                ..
            } if i == id => Some((&base.order, &addition.order, pattern)),
            _ => None,
        })?;
        if base.is_empty() || addition.is_empty() {
            return None;
        }
        // applyDemoTransformation: up to 256 draws, the first that isn't empty.
        (0..256).find_map(|_| {
            let base = ItemStack::new(&base[pick(base.len())], 1);
            let material = ItemStack::new(&addition[pick(addition.len())], 1);
            apply_trim(e, &base, &material, pattern)
        })
    }
}
/// `SmithingTrimRecipe.applyTrim`: none when the material provides no trim
/// or the base already has this one.
fn apply_trim(
    e: &Enchantments,
    base: &ItemStack,
    material: &ItemStack,
    pattern: &crate::item_data::trim::Holder<crate::item_data::trim::Pattern>,
) -> Option<ItemStack> {
    let material = e.props(&material.item)?.trim_material.clone()?;
    let trim: crate::item_data::Trim = (material.as_str().into(), pattern.clone());
    if base.data().and_then(|d| d.trim.as_ref()) == Some(&trim) {
        return None;
    }
    let mut out = ItemStack {
        count: 1,
        ..base.clone()
    };
    out.data_mut().trim = Some(trim);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enchant::tests::{sample, sample_items};
    use horizon_world::nbt::Tag;

    fn sword(ench: &[(&str, u32)]) -> ItemStack {
        let mut s = ItemStack::new("minecraft:diamond_sword", 1);
        for (id, l) in ench {
            s.enchant(id, *l);
        }
        s
    }
    fn book(ench: &[(&str, u32)]) -> ItemStack {
        let mut s = ItemStack::new("minecraft:enchanted_book", 1);
        for (id, l) in ench {
            s.enchant(id, *l);
        }
        s
    }

    fn worn(item: &str, max: i32, damage: u32) -> ItemStack {
        let mut s = ItemStack::new(item, 1);
        s.damage = damage;
        s.data_mut().set_other(
            "minecraft:max_damage",
            Some(horizon_world::nbt::Tag::Int(max)),
        );
        s
    }

    /// Anvil: `ItemStack.isValidRepairItem` reads the stack's `REPAIRABLE`
    /// component, so a patched list repairs where the item's default does not.
    #[test]
    fn anvil_reads_the_stacks_repairable_component() {
        let e = sample();
        let items = sample_items();
        let mut sword = worn("minecraft:diamond_sword", 100, 20);
        sword.data_mut().set_other(
            "minecraft:repairable",
            Some(Tag::compound([(
                "items",
                Tag::List(vec![Tag::String("minecraft:stick".into())]),
            )])),
        );
        let r = anvil_result(
            &e,
            &items,
            Some(&sword),
            Some(&ItemStack::new("minecraft:stick", 1)),
            None,
            "",
            false,
        );
        let out = r.result.expect("the stick repairs the sword");
        // 20 damage of a 100 max is under the 25 one material restores.
        assert_eq!(out.damage, 0);
        assert_eq!(r.cost, 1);
        assert_eq!(r.repair_item_count_cost, 1);
        assert_eq!(out.repair_cost(), 1);
    }

    /// Grindstone: the larger MAX_DAMAGE is written and the damage follows it.
    #[test]
    fn grindstone_merge_uses_component_max_damage() {
        let e = sample();
        let items = sample_items();
        let a = worn("minecraft:diamond_sword", 100, 90);
        let b = worn("minecraft:diamond_sword", 200, 190);
        let out = grindstone_result(&e, &items, Some(&a), Some(&b)).unwrap();
        assert_eq!(
            out.data().unwrap().other("minecraft:max_damage"),
            Some(&horizon_world::nbt::Tag::Int(200))
        );
        // remaining 10 + 10 + 200 * 5 / 100 = 30 -> damage 170.
        assert_eq!(out.damage, 170);
    }

    /// Grindstone: a merged MAX_DAMAGE equal to the item's own is no patch, so the
    /// result saves without it (vanilla: two worn diamond pickaxes give a plain one).
    #[test]
    fn grindstone_merge_at_the_default_max_damage_writes_no_component() {
        let root = std::env::temp_dir().join(format!("horizon-grind-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("items.json"),
            r#"{"minecraft:diamond_sword":{"minecraft:max_stack_size":1,"minecraft:max_damage":1561}}"#,
        )
        .unwrap();
        let items = Items::catalog(&root, &PackOverlay::NONE).unwrap();
        let e = sample();
        let mut a = ItemStack::new("minecraft:diamond_sword", 1);
        a.damage = 819;
        let mut b = ItemStack::new("minecraft:diamond_sword", 1);
        b.damage = 602;
        let out = grindstone_result(&e, &items, Some(&a), Some(&b)).unwrap();
        assert_eq!(
            out.data().and_then(|d| d.other("minecraft:max_damage")),
            None
        );
        // remaining 742 + 959 + 78 = 1779 > 1561 -> damage 0.
        assert_eq!(out.damage, 0);
        // A larger patched MAX_DAMAGE on the second stack still wins and is written.
        let b = worn("minecraft:diamond_sword", 2000, 1990);
        let out = grindstone_result(&e, &items, Some(&a), Some(&b)).unwrap();
        assert_eq!(
            out.data().unwrap().other("minecraft:max_damage"),
            Some(&horizon_world::nbt::Tag::Int(2000))
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// Anvil: merging two damaged items reads each stack's own max damage.
    #[test]
    fn anvil_merge_uses_component_max_damage() {
        let e = sample();
        let items = sample_items();
        let a = worn("minecraft:diamond_sword", 100, 50);
        let b = worn("minecraft:diamond_sword", 100, 50);
        let r = anvil_result(&e, &items, Some(&a), Some(&b), None, "", false);
        // remaining 50 + 50 + 12 = 112 -> damage 0.
        assert_eq!(r.result.unwrap().damage, 0);
        assert_eq!(r.cost, 2);
    }

    /// `has(STORED_ENCHANTMENTS)` is presence: an empty component counts, an
    /// enchanted book counts by default.
    #[test]
    fn stored_enchantments_presence() {
        let mut stick = ItemStack::new("minecraft:stick", 1);
        assert!(!has_stored_enchantments(&stick));
        stick.data_mut().set_other(
            "minecraft:stored_enchantments",
            Some(horizon_world::nbt::Tag::compound(Vec::<(&str, _)>::new())),
        );
        assert!(has_stored_enchantments(&stick));
        let mut book = ItemStack::new("minecraft:enchanted_book", 1);
        assert!(has_stored_enchantments(&book));
        book.data_mut().set_other(
            "!minecraft:stored_enchantments",
            Some(horizon_world::nbt::Tag::compound(Vec::<(&str, _)>::new())),
        );
        assert!(!has_stored_enchantments(&book));
    }

    #[test]
    fn bookshelf_ring_has_32_positions() {
        let o = bookshelf_offsets();
        assert_eq!(o.len(), 32);
        assert!(o.contains(&[-2, 0, -2]) && o.contains(&[2, 1, 0]) && !o.contains(&[1, 0, 1]));
    }

    #[test]
    fn enchant_offers_are_deterministic_per_seed() {
        let e = sample();
        let items = sample_items();
        let s = ItemStack::new("minecraft:diamond_sword", 1);
        let a = enchant_offers(&e, &items, 1234, 15, Some(&s));
        let b = enchant_offers(&e, &items, 1234, 15, Some(&s));
        assert_eq!(a, b);
        assert_eq!(a.costs[2], 30, "{a:?}");
        assert!(a.costs[0] >= 1 && a.costs[0] <= a.costs[2]);
        // The clue is the first offer's random pick from the same list.
        for slot in 0..3 {
            let list = enchantment_list(
                &e,
                &items,
                1234,
                slot,
                a.costs[slot],
                "minecraft:diamond_sword",
            );
            let clue = a.clues[slot].clone().unwrap();
            assert!(list.contains(&clue));
            let applied = apply_enchantment(&e, &items, 1234, slot, a.costs[slot], &s).unwrap();
            assert_eq!(applied.enchantments(), &list[..]);
        }
        // Already enchanted items are not enchantable.
        assert_eq!(
            enchant_offers(
                &e,
                &items,
                1234,
                15,
                Some(&sword(&[("minecraft:sharpness", 1)]))
            )
            .costs,
            [0, 0, 0]
        );
        let books =
            apply_enchantment(&e, &items, 99, 2, 30, &ItemStack::new("minecraft:book", 1)).unwrap();
        assert_eq!(&*books.item, "minecraft:enchanted_book");
        assert!(!books.stored_enchantments().is_empty());
    }

    /// The mods' answer to `AnvilUpdateEvent` replaces vanilla's result as
    /// `CommonHooks.onAnvilUpdate` sets it: a cancel empties the result and
    /// both costs, `AnvilMenu.setCost` keeps the level cost at 0 or more, and
    /// a material cost below 1 is vanilla's 0. The rename flag stays vanilla's.
    #[test]
    fn an_anvil_update_answer_replaces_vanillas_result() {
        let vanilla = AnvilResult {
            result: Some(ItemStack::new("minecraft:diamond_sword", 1)),
            cost: 3,
            repair_item_count_cost: 2,
            only_renaming: true,
        };
        let emerald = Some(ItemStack::new("minecraft:emerald", 1));
        let mut r = vanilla.clone();
        r.answer_update(false, emerald.clone(), 7, 4);
        assert_eq!(
            r,
            AnvilResult {
                result: emerald.clone(),
                cost: 7,
                repair_item_count_cost: 4,
                only_renaming: true,
            }
        );
        let mut r = vanilla.clone();
        r.answer_update(false, emerald.clone(), -5, -1);
        assert_eq!((r.cost, r.repair_item_count_cost), (0, 0));
        // Unchanged values give vanilla's result back.
        let mut r = vanilla.clone();
        r.answer_update(false, vanilla.result.clone(), 3, 2);
        assert_eq!(r, vanilla);
        let mut r = vanilla.clone();
        r.answer_update(true, emerald, 7, 4);
        assert_eq!(
            r,
            AnvilResult {
                result: None,
                cost: 0,
                repair_item_count_cost: 0,
                only_renaming: true,
            }
        );
    }

    #[test]
    fn anvil_combines_and_charges_like_vanilla() {
        let e = sample();
        let items = sample_items();
        // Sharpness II + Sharpness II book = Sharpness III; book fee 1/2 -> max(1, 0) = 1 per level.
        let r = anvil_result(
            &e,
            &items,
            Some(&sword(&[("minecraft:sharpness", 2)])),
            Some(&book(&[("minecraft:sharpness", 2)])),
            None,
            "Diamond Sword",
            false,
        );
        let out = r.result.unwrap();
        assert_eq!(out.enchantment_level("minecraft:sharpness"), 3);
        assert_eq!(r.cost, 3);
        assert_eq!(out.repair_cost(), 1);
        // Incompatible only: no result.
        let r = anvil_result(
            &e,
            &items,
            Some(&sword(&[("minecraft:sharpness", 1)])),
            Some(&book(&[("minecraft:smite", 1)])),
            None,
            "Diamond Sword",
            false,
        );
        assert!(r.result.is_none());
        // Max level clamps: V + V stays V.
        let r = anvil_result(
            &e,
            &items,
            Some(&sword(&[("minecraft:sharpness", 5)])),
            Some(&book(&[("minecraft:sharpness", 5)])),
            None,
            "x",
            false,
        );
        assert_eq!(
            r.result.unwrap().enchantment_level("minecraft:sharpness"),
            5
        );
        assert_eq!(r.cost, 5);
    }

    #[test]
    fn anvil_rename_and_prior_work() {
        let e = sample();
        let items = sample_items();
        let mut s = sword(&[]);
        s.data_mut().repair_cost = 7;
        let r = anvil_result(
            &e,
            &items,
            Some(&s),
            None,
            Some("Excalibur"),
            "Diamond Sword",
            false,
        );
        assert_eq!(r.cost, 8);
        assert!(r.only_renaming);
        let out = r.result.unwrap();
        assert_eq!(out.custom_name().as_deref(), Some("Excalibur"));
        // Renaming alone does not raise the penalty.
        assert_eq!(out.repair_cost(), 7);
        // Same name: nothing to do.
        assert!(
            anvil_result(
                &e,
                &items,
                Some(&s),
                None,
                Some("Diamond Sword"),
                "Diamond Sword",
                false
            )
            .result
            .is_none()
        );
        // Too expensive at 40+ unless creative; rename-only caps at 39.
        s.data_mut().repair_cost = 63;
        let r = anvil_result(
            &e,
            &items,
            Some(&s),
            None,
            Some("A"),
            "Diamond Sword",
            false,
        );
        assert_eq!(r.cost, 39);
        assert!(r.result.is_some());
        let r = anvil_result(
            &e,
            &items,
            Some(&s),
            Some(&book(&[("minecraft:unbreaking", 1)])),
            None,
            "Diamond Sword",
            false,
        );
        assert!(r.cost >= 40 && r.result.is_none());
        assert!(
            anvil_result(
                &e,
                &items,
                Some(&s),
                Some(&book(&[("minecraft:unbreaking", 1)])),
                None,
                "Diamond Sword",
                true
            )
            .result
            .is_some()
        );
        assert_eq!(increased_repair_cost(0), 1);
        assert_eq!(increased_repair_cost(3), 7);
    }

    /// Checks against the prepared vanilla resources when they are present.
    #[test]
    fn prepared_vanilla_data() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("items.json").exists() {
            return;
        }
        let items = Items::tags_only(&root, &PackOverlay::NONE).unwrap();
        let data = StationData::load(&root, &items, &PackOverlay::NONE);
        let e = &data.enchantments;
        assert_eq!(e.list.len(), 43);
        assert_eq!(
            &*e.tag("minecraft:in_enchanting_table")[0],
            "minecraft:protection"
        );
        assert!(!e.compatible("minecraft:fortune", "minecraft:silk_touch"));
        assert!(e.is_curse("minecraft:binding_curse"));
        assert_eq!(e.full_name("minecraft:sharpness", 5).0, "Sharpness V");
        assert_eq!(e.enchantable("minecraft:golden_sword"), 22);
        // Brewing: water + nether wart = awkward.
        assert_eq!(data.brewing.recipes.len(), 279);
        let water = ItemStack::new("minecraft:potion", 1).with_potion("minecraft:water");
        let r = data
            .brewing
            .find(&water, &ItemStack::new("minecraft:nether_wart", 1))
            .unwrap();
        assert_eq!(r.output.potion(), Some("minecraft:awkward"));
        assert!(
            data.brewing
                .is_reagent(&ItemStack::new("minecraft:blaze_powder", 1))
        );
        // Smithing: netherite upgrade keeps enchantments.
        let mut sword = ItemStack::new("minecraft:diamond_sword", 1);
        sword.enchant("minecraft:sharpness", 2);
        let out = data
            .recipes
            .smithing_result(
                e,
                Some(&ItemStack::new(
                    "minecraft:netherite_upgrade_smithing_template",
                    1,
                )),
                Some(&sword),
                Some(&ItemStack::new("minecraft:netherite_ingot", 1)),
            )
            .unwrap();
        assert_eq!(
            (&*out.item, out.enchantment_level("minecraft:sharpness")),
            ("minecraft:netherite_sword", 2)
        );
        // Anvil: a patched REPAIRABLE list repairs with an item the sword's
        // default does not accept.
        let mut repairable = ItemStack::new("minecraft:diamond_sword", 1);
        repairable.damage = 100;
        repairable.data_mut().set_other(
            "minecraft:repairable",
            Some(Tag::compound([(
                "items",
                Tag::List(vec![Tag::String("minecraft:stick".into())]),
            )])),
        );
        let r = anvil_result(
            e,
            &items,
            Some(&repairable),
            Some(&ItemStack::new("minecraft:stick", 1)),
            None,
            "Diamond Sword",
            false,
        );
        assert_eq!(r.cost, 1);
        assert_eq!(r.result.unwrap().damage, 0);
        assert!(
            data.recipes
                .stonecutter(&ItemStack::new("minecraft:stone", 1))
                .len()
                >= 10
        );
        // Trades: every profession level set yields offers.
        let ctx = crate::merchant::TradeContext {
            items: &items,
            enchantments: e,
            variant: "minecraft:plains",
            origin: [0.0; 3],
            maps: None,
        };
        for p in crate::merchant::PROFESSIONS {
            let set = crate::merchant::Trades::set_for(p, 1);
            let mut random = crate::merchant::sequence_random(
                12345,
                &format!(
                    "minecraft:trade_set/{}",
                    set.trim_start_matches("minecraft:")
                ),
            );
            assert!(
                !data
                    .trades
                    .offers_from_set(&ctx, &set, &mut random)
                    .is_empty(),
                "{p}"
            );
        }
    }

    #[test]
    fn grindstone_strips_non_curses() {
        let e = sample();
        let items = sample_items();
        let s = sword(&[("minecraft:sharpness", 3)]);
        let out = grindstone_result(&e, &items, Some(&s), None).unwrap();
        assert!(out.enchantments().is_empty());
        assert_eq!(out.repair_cost(), 0);
        assert!(out.extra.is_none());
        let b =
            grindstone_result(&e, &items, None, Some(&book(&[("minecraft:mending", 1)]))).unwrap();
        assert_eq!(&*b.item, "minecraft:book");
        assert!(grindstone_result(&e, &items, Some(&sword(&[])), None).is_none());
        let mut r = LegacyRandom::new(5);
        // min cost of Sharpness III = 23; ceil(23/2)=12 -> 12..23.
        let xp = grindstone_experience(&e, &mut r, Some(&s), None);
        assert!((12..24).contains(&xp));
    }
    /// Grindstone: `ItemStack.matches` compares the component patch, so an
    /// explicit `damage: 0` on an item whose prototype has no damage component
    /// keeps the two stacks from merging.
    #[test]
    fn grindstone_keeps_an_explicit_zero_damage_apart() {
        let e = sample();
        let items = sample_items();
        let zero = item_data::parse_stack(
            r#"{id:"minecraft:book",count:1,components:{"minecraft:damage":0,"minecraft:enchantments":{"minecraft:sharpness":1}}}"#,
        )
        .unwrap()
        .unwrap();
        let plain = item_data::parse_stack(
            r#"{id:"minecraft:book",count:1,components:{"minecraft:enchantments":{"minecraft:sharpness":1}}}"#,
        )
        .unwrap()
        .unwrap();
        assert!(grindstone_result(&e, &items, Some(&zero), Some(&plain)).is_none());
        let out = grindstone_result(&e, &items, Some(&zero), Some(&zero.clone())).unwrap();
        assert_eq!(out.count, 2);
        assert!(!out.has_any_enchantments());
    }

    #[test]
    fn smithing_transform_overlay_matches_optional_inputs_and_takes_template_result() {
        use crate::{banner::Banners, brewing::Brewing, crafting::Recipes, creative::Names, menu};
        let root = std::env::temp_dir().join(format!("horizon-smithing-{}", std::process::id()));
        fs::create_dir_all(root.join("data/minecraft/recipe")).unwrap();
        fs::write(
            root.join("items.json"),
            r#"{
            "minecraft:stick":{"minecraft:max_stack_size":64},
            "minecraft:diamond":{"minecraft:max_stack_size":64},
            "minecraft:emerald":{"minecraft:max_stack_size":64},
            "minecraft:diamond_sword":{"minecraft:max_stack_size":1,"minecraft:max_damage":1561}
        }"#,
        )
        .unwrap();
        let items = Items::catalog(&root, &PackOverlay::NONE).unwrap();
        let recipe = serde_json::json!({"type":"minecraft:smithing_transform",
            "base":"minecraft:stick", "addition":"minecraft:diamond",
            "result":{"id":"minecraft:emerald","count":3,
                "components":{"minecraft:custom_name":"Upgraded","minecraft:repair_cost":0}}});
        let files = vec![(
            "recipe/test:upgrade".to_owned(),
            vec![serde_json::to_vec(&recipe).unwrap()],
        )];
        let overlay = PackOverlay { files: &files };
        let stations = StationRecipes::load(&root, &items, &overlay);
        let e = sample();
        let mut base = ItemStack::new("minecraft:stick", 2);
        base.data_mut()
            .set_loot_name("minecraft:custom_name", &serde_json::json!("Original"));
        base.data_mut().repair_cost = 7;
        base.data_mut()
            .set_loot_component("minecraft:custom_data", &serde_json::json!({"kept":1}));
        let addition = ItemStack::new("minecraft:diamond", 2);
        let result = stations
            .smithing_result(&e, None, Some(&base), Some(&addition))
            .expect("omitted template accepts an empty slot");
        assert_eq!(
            (&*result.item, result.count, result.repair_cost()),
            ("minecraft:emerald", 3, 0)
        );
        assert_eq!(
            result.data().unwrap().custom_name,
            Some(horizon_world::nbt::Tag::String("Upgraded".into()))
        );
        assert!(
            result
                .data()
                .unwrap()
                .other("minecraft:custom_data")
                .is_some()
        );
        assert!(
            stations
                .smithing_result(&e, Some(&addition), Some(&base), Some(&addition))
                .is_none()
        );
        assert!(
            stations
                .smithing_result(&e, None, Some(&base), None)
                .is_none()
        );
        let recipes = Recipes::load(&root, &items, &PackOverlay::NONE).unwrap();
        let brewing = Brewing::load(&root, &items, &PackOverlay::NONE);
        let banners = Banners::load(&root);
        let names = Names::default();
        let env = menu::Env {
            items: &items,
            recipes: &recipes,
            brewing: &brewing,
            banners: &banners,
            station_recipes: &stations,
            enchantments: &e,
            names: &names,
            trade: None,
            maps: None,
            limited_crafting: false,
            known_recipes: None,
        };
        let mut menu = menu::Menu::new(menu::Kind::Smithing);
        menu.set_slot(&env, 1, Some(base));
        menu.set_slot(&env, 2, Some(addition));
        assert_eq!(menu.slot(3), Some(&result));
        menu.click(&env, 3, 0, menu::Input::Pickup).unwrap();
        assert_eq!(menu.carried.as_ref(), Some(&result));
        assert!(menu.slot(0).is_none());
        assert_eq!(menu.slot(1).unwrap().count, 1);
        assert_eq!(menu.slot(2).unwrap().count, 1);
        assert_eq!(menu.crafted[0].0, "test:upgrade");
        for result in [
            serde_json::json!({"id":"minecraft:emerald","count":0}),
            serde_json::json!({"id":"minecraft:emerald","count":100}),
            serde_json::json!({"id":"minecraft:diamond_sword","count":2}),
            serde_json::json!({"id":"minecraft:emerald","components":{"minecraft:max_damage":10}}),
        ] {
            let mut invalid = recipe.clone();
            invalid["result"] = result;
            let files = vec![(
                "recipe/test:invalid".to_owned(),
                vec![serde_json::to_vec(&invalid).unwrap()],
            )];
            let stations = StationRecipes::load(&root, &items, &PackOverlay { files: &files });
            assert!(
                stations
                    .smithing_result(&e, None, menu.slot(1), menu.slot(2))
                    .is_none(),
                "{invalid}"
            );
        }
        for key in ["template", "base", "addition"] {
            let mut invalid = recipe.clone();
            invalid[key] = Value::Null;
            let files = vec![(
                "recipe/test:invalid".to_owned(),
                vec![serde_json::to_vec(&invalid).unwrap()],
            )];
            let stations = StationRecipes::load(&root, &items, &PackOverlay { files: &files });
            assert!(
                stations.smithing.is_empty(),
                "malformed {key} must not become optional"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn smithing_trim_reads_an_inline_pattern() {
        let root = std::env::temp_dir().join(format!("horizon-trim-{}", std::process::id()));
        fs::create_dir_all(root.join("data/minecraft/recipe")).unwrap();
        fs::write(
            root.join("items.json"),
            r#"{"minecraft:gold_ingot":{"minecraft:max_stack_size":64,"minecraft:provides_trim_material":"minecraft:gold"}}"#,
        )
        .unwrap();
        fs::write(
            root.join("data/minecraft/recipe/coast.json"),
            serde_json::json!({
                "type": "minecraft:smithing_trim",
                "template": "minecraft:coast_armor_trim_smithing_template",
                "base": "minecraft:diamond_chestplate",
                "addition": "minecraft:gold_ingot",
                "pattern": {
                    "asset_id": "minecraft:coast",
                    "description": { "text": "Coast" },
                    "decal": false
                }
            })
            .to_string(),
        )
        .unwrap();
        let items = Items::tags_only(&root, &PackOverlay::NONE).unwrap();
        let recipes = StationRecipes::load(&root, &items, &PackOverlay::NONE);
        assert_eq!(recipes.smithing.len(), 1);
        let out = recipes
            .smithing_result(
                &items.enchantments,
                Some(&ItemStack::new(
                    "minecraft:coast_armor_trim_smithing_template",
                    1,
                )),
                Some(&ItemStack::new("minecraft:diamond_chestplate", 1)),
                Some(&ItemStack::new("minecraft:gold_ingot", 1)),
            )
            .expect("the inline pattern applies");
        let trim = out.data().and_then(|d| d.trim.as_ref()).unwrap();
        assert_eq!(
            trim.1,
            crate::item_data::trim::Holder::Direct(crate::item_data::trim::Pattern {
                asset_id: "minecraft:coast".into(),
                description: Tag::String("Coast".into()),
                decal: false,
            })
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn smithing_ingredient_parses_holders() {
        let items = sample_items();
        let a = ingredient(&serde_json::json!("minecraft:diamond_sword"), &items);
        assert_eq!(a.order, vec!["minecraft:diamond_sword"]);
        let b = ingredient(
            &serde_json::json!(["minecraft:diamond_sword", "minecraft:diamond_axe"]),
            &items,
        );
        assert_eq!(
            b.order,
            vec!["minecraft:diamond_sword", "minecraft:diamond_axe"]
        );
    }
    /// A world pack's stonecutting recipe takes its place in registration
    /// order (path, then namespace), not a place after the game's own.
    #[test]
    fn stonecutter_lists_recipes_in_registration_order() {
        let root =
            std::env::temp_dir().join(format!("horizon-stonecutter-order-{}", std::process::id()));
        let dir = root.join("data/minecraft/recipe");
        fs::create_dir_all(&dir).unwrap();
        let cut = |result: &str| {
            format!(
                r#"{{"type":"minecraft:stonecutting","ingredient":"minecraft:stone","result":{{"id":"{result}"}}}}"#
            )
        };
        fs::write(dir.join("z_cut.json"), cut("minecraft:stone_slab")).unwrap();
        fs::write(dir.join("b_cut.json"), cut("minecraft:stone_bricks")).unwrap();
        let packs = [(
            "recipe/pack:a_cut".to_owned(),
            vec![cut("minecraft:stone_stairs").into_bytes()],
        )];
        let recipes = StationRecipes::load(&root, &sample_items(), &PackOverlay { files: &packs });
        let _ = fs::remove_dir_all(&root);
        let listed: Vec<_> = recipes
            .stonecutter(&ItemStack::new("minecraft:stone", 1))
            .iter()
            .map(|r| (r.id.as_str(), &*r.result.item))
            .collect();
        assert_eq!(
            listed,
            [
                ("pack:a_cut", "minecraft:stone_stairs"),
                ("minecraft:b_cut", "minecraft:stone_bricks"),
                ("minecraft:z_cut", "minecraft:stone_slab"),
            ]
        );
    }
}
