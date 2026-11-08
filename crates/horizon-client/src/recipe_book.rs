//! The recipe book: what the book shows for each menu (`RecipeBookType`
//! tabs, `RecipeBookCategories`, groups), which recipes the player can craft
//! with the items at hand (`StackedItemContents.canCraft` and the biggest
//! craftable stack), where a recipe goes in a grid (`PlaceRecipeHelper`) and
//! the book's saved settings.
use crate::{
    crafting::{Ingredient, Placement, Recipe, Recipes},
    items::{ItemStack, Items},
};
use horizon_protocol::packets::recipes::{HolderSet, RecipeBookEntry, RecipeDisplay, SlotDisplay};
use horizon_world::loot::PackOverlay;
use horizon_world::nbt::Tag;
use std::collections::{BTreeSet, HashMap};

/// A native mod's book category for one custom recipe type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecipeBookCategory {
    /// A namespaced category id outside `minecraft:`.
    pub id: String,
    /// A custom type present in the current recipe table.
    pub recipe_type: String,
    /// A registered item id used as the category's icon.
    pub icon: String,
}

/// Validate declarations once per recipe table. The first valid declaration
/// owns its category id and recipe type; vanilla categories are never replaced.
pub fn mod_categories(
    declarations: Vec<RecipeBookCategory>,
    recipes: &Recipes,
    items: &Items,
) -> HashMap<String, RecipeBookCategory> {
    let valid_id = |id: &str| {
        id.split_once(':').is_some_and(|(namespace, path)| {
            !namespace.is_empty()
                && !path.is_empty()
                && namespace.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_.-".contains(&byte)
                })
                && path.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"/_.-".contains(&byte)
                })
        })
    };
    let mut categories = HashMap::new();
    let mut ids = BTreeSet::new();
    for category in declarations {
        let reason = if !valid_id(&category.id) || category.id.starts_with("minecraft:") {
            Some("invalid or reserved category id")
        } else if !valid_id(&category.recipe_type)
            || category.recipe_type.starts_with("minecraft:")
            || !recipes.custom.contains_key(&category.recipe_type)
        {
            Some("unknown or vanilla recipe type")
        } else if !valid_id(&category.icon)
            || category.icon == "minecraft:air"
            || items.get(&category.icon).is_none()
        {
            Some("missing or unknown icon item")
        } else if ids.contains(&category.id) || categories.contains_key(&category.recipe_type) {
            Some("duplicate category id or recipe type")
        } else {
            None
        };
        if let Some(reason) = reason {
            log::warn!("recipe book category {:?} skipped: {reason}", category);
            continue;
        }
        ids.insert(category.id.clone());
        categories.insert(category.recipe_type.clone(), category);
    }
    categories
}

/// `RecipeBookType`: the menus with a recipe book and their own settings.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum BookType {
    Crafting,
    Furnace,
    BlastFurnace,
    Smoker,
}
impl BookType {
    pub const ALL: [Self; 4] = [
        Self::Crafting,
        Self::Furnace,
        Self::BlastFurnace,
        Self::Smoker,
    ];
    pub fn index(self) -> usize {
        self as usize
    }
    /// `RecipeBookSettings` keys in the player's `recipeBook` compound.
    fn keys(self) -> (&'static str, &'static str) {
        match self {
            Self::Crafting => ("isGuiOpen", "isFilteringCraftable"),
            Self::Furnace => ("isFurnaceGuiOpen", "isFurnaceFilteringCraftable"),
            Self::BlastFurnace => (
                "isBlastingFurnaceGuiOpen",
                "isBlastingFurnaceFilteringCraftable",
            ),
            Self::Smoker => ("isSmokerGuiOpen", "isSmokerFilteringCraftable"),
        }
    }
    /// The tab buttons of the book's screen, search first.
    pub fn tabs(self) -> Vec<Tab> {
        use Category::*;
        let categories: &[Category] = match self {
            Self::Crafting => &[
                CraftingEquipment,
                CraftingBuilding,
                CraftingMisc,
                CraftingRedstone,
            ],
            Self::Furnace => &[FurnaceFood, FurnaceBlocks, FurnaceMisc],
            Self::BlastFurnace => &[BlastBlocks, BlastMisc],
            Self::Smoker => &[SmokerFood],
        };
        std::iter::once(Tab::Search)
            .chain(categories.iter().map(|c| Tab::Category(*c)))
            .collect()
    }
    /// The categories the search tab lists, in `SearchRecipeBookCategory` order.
    fn search_categories(self) -> &'static [Category] {
        use Category::*;
        match self {
            Self::Crafting => &[
                CraftingEquipment,
                CraftingBuilding,
                CraftingMisc,
                CraftingRedstone,
            ],
            Self::Furnace => &[FurnaceFood, FurnaceBlocks, FurnaceMisc],
            Self::BlastFurnace => &[BlastBlocks, BlastMisc],
            Self::Smoker => &[SmokerFood],
        }
    }
}

/// `RecipeBookCategories` of the four menus' recipes.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Category {
    CraftingBuilding,
    CraftingRedstone,
    CraftingEquipment,
    CraftingMisc,
    FurnaceFood,
    FurnaceBlocks,
    FurnaceMisc,
    BlastBlocks,
    BlastMisc,
    SmokerFood,
}
impl Category {
    /// The `RecipeBookCategories` registry path.
    pub fn name(self) -> &'static str {
        match self {
            Self::CraftingBuilding => "crafting_building_blocks",
            Self::CraftingRedstone => "crafting_redstone",
            Self::CraftingEquipment => "crafting_equipment",
            Self::CraftingMisc => "crafting_misc",
            Self::FurnaceFood => "furnace_food",
            Self::FurnaceBlocks => "furnace_blocks",
            Self::FurnaceMisc => "furnace_misc",
            Self::BlastBlocks => "blast_furnace_blocks",
            Self::BlastMisc => "blast_furnace_misc",
            Self::SmokerFood => "smoker_food",
        }
    }
    /// The category of a `minecraft:recipe_book_category` name; none for the
    /// ones this book has no tab for (stonecutter, smithing, campfire).
    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.strip_prefix("minecraft:").unwrap_or(name);
        [
            Self::CraftingBuilding,
            Self::CraftingRedstone,
            Self::CraftingEquipment,
            Self::CraftingMisc,
            Self::FurnaceFood,
            Self::FurnaceBlocks,
            Self::FurnaceMisc,
            Self::BlastBlocks,
            Self::BlastMisc,
            Self::SmokerFood,
        ]
        .into_iter()
        .find(|c| c.name() == name)
    }
}
/// A tab button: the search tab or one category.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Tab {
    Search,
    Category(Category),
}
impl Tab {
    /// The tab's icon items (`TabInfo`).
    pub fn icons(self) -> (&'static str, Option<&'static str>) {
        use Category::*;
        match self {
            Self::Search => ("minecraft:compass", None),
            Self::Category(c) => match c {
                CraftingEquipment => ("minecraft:iron_axe", Some("minecraft:golden_sword")),
                CraftingBuilding => ("minecraft:bricks", None),
                CraftingMisc => ("minecraft:lava_bucket", Some("minecraft:apple")),
                CraftingRedstone => ("minecraft:redstone", None),
                FurnaceFood | SmokerFood => ("minecraft:porkchop", None),
                FurnaceBlocks => ("minecraft:stone", None),
                FurnaceMisc => ("minecraft:lava_bucket", Some("minecraft:emerald")),
                BlastBlocks => ("minecraft:redstone_ore", None),
                BlastMisc => ("minecraft:iron_shovel", Some("minecraft:golden_leggings")),
            },
        }
    }
}

/// `RecipeDisplay`: how a recipe is laid out.
#[derive(Clone, Debug)]
pub enum Display {
    Shaped {
        width: usize,
        height: usize,
        cells: Vec<Option<Ingredient>>,
    },
    Shapeless(Vec<Ingredient>),
    /// A smelting-type recipe: its input; the fuel is any fuel.
    Furnace(Ingredient),
}
/// One recipe of the book (`RecipeDisplayEntry`).
#[derive(Clone, Debug)]
pub struct Entry {
    /// The recipe id, `minecraft:name`.
    pub id: String,
    /// `RecipeDisplayId`: the display's index among every recipe's displays
    /// (`Book::set_display_ids`; -1 until then).
    pub display_id: i32,
    pub category: Category,
    pub group: Option<String>,
    pub display: Display,
    pub result: ItemStack,
    /// Every stack its display can show as the result when that is more than
    /// `result` (a map copy of any map, an arrow of any potion): the search
    /// tree reads them all, the button shows the first.
    pub variants: Vec<ItemStack>,
    /// Ingredients shown as other stacks than their items (by index in
    /// `requirements`): the potions an imbued arrow's lingering potion is.
    pub stacks: Vec<(usize, Vec<ItemStack>)>,
}
impl Entry {
    /// Every stack the result display cycles through.
    pub fn results(&self) -> &[ItemStack] {
        if self.variants.is_empty() {
            std::slice::from_ref(&self.result)
        } else {
            &self.variants
        }
    }
    /// The ingredients that must be matched to craft it, in order.
    pub fn requirements(&self) -> Vec<&Ingredient> {
        match &self.display {
            Display::Shaped { cells, .. } => cells.iter().flatten().collect(),
            Display::Shapeless(list) => list.iter().collect(),
            Display::Furnace(i) => vec![i],
        }
    }
    /// `CraftingRecipeBookComponent.canDisplay` for a grid.
    pub fn fits(&self, grid_width: usize, grid_height: usize) -> bool {
        match &self.display {
            Display::Shaped { width, height, .. } => grid_width >= *width && grid_height >= *height,
            Display::Shapeless(list) => grid_width * grid_height >= list.len(),
            Display::Furnace(_) => true,
        }
    }
    /// The size the recipe takes in the grid (`PlaceRecipeHelper`).
    pub fn size_in(&self, grid_width: usize, grid_height: usize) -> (usize, usize) {
        match &self.display {
            Display::Shaped { width, height, .. } => (*width, *height),
            _ => (grid_width, grid_height),
        }
    }
    /// The ingredient of each grid cell the recipe fills, with its grid index
    /// (the ghost recipe and the placement use the same layout).
    pub fn grid_cells(&self, grid_width: usize, grid_height: usize) -> Vec<(Option<usize>, usize)> {
        let (w, h) = self.size_in(grid_width, grid_height);
        // One entry per recipe cell: the index of its ingredient, if any.
        let entries: Vec<Option<usize>> = match &self.display {
            Display::Shaped { cells, .. } => {
                let mut next = 0;
                cells
                    .iter()
                    .map(|c| {
                        c.as_ref().map(|_| {
                            next += 1;
                            next - 1
                        })
                    })
                    .collect()
            }
            Display::Shapeless(list) => (0..list.len()).map(Some).collect(),
            Display::Furnace(_) => vec![Some(0)],
        };
        place_cells(grid_width, grid_height, w, h, &entries)
    }
}

/// `PlaceRecipeHelper.placeRecipe`: the grid index each entry goes to; a
/// recipe smaller than half the grid is centred.
pub fn place_cells<T: Clone>(
    grid_width: usize,
    grid_height: usize,
    recipe_width: usize,
    recipe_height: usize,
    entries: &[T],
) -> Vec<(T, usize)> {
    let mut out = Vec::new();
    let mut iter = entries.iter().peekable();
    let mut index = 0usize;
    let mut y = 0;
    while y < grid_height {
        let center_rows = (recipe_height as f32) < grid_height as f32 / 2.0;
        let start_row = (grid_height as f32 / 2.0 - recipe_height as f32 / 2.0).floor() as usize;
        if center_rows && start_row > y {
            index += grid_width;
            y += 1;
        }
        for x in 0..grid_width {
            if iter.peek().is_none() {
                return out;
            }
            let center_cols = (recipe_width as f32) < grid_width as f32 / 2.0;
            let start_col = (grid_width as f32 / 2.0 - recipe_width as f32 / 2.0).floor() as usize;
            let mut total = recipe_width;
            let mut add = x < recipe_width;
            if center_cols {
                total = start_col + recipe_width;
                add = start_col <= x && x < start_col + recipe_width;
            }
            if add {
                if let Some(item) = iter.next() {
                    out.push((item.clone(), index));
                }
            } else if total == x {
                index += grid_width - x;
                break;
            }
            index += 1;
        }
        y += 1;
    }
    out
}

/// A group of recipes shown as one button (`RecipeCollection`).
#[derive(Clone, Debug, Default)]
pub struct Collection {
    /// Indices into `Book::entries`.
    pub entries: Vec<usize>,
}

/// Every recipe the menus' books can show.
#[derive(Default)]
pub struct Book {
    pub entries: Vec<Entry>,
    /// `Recipe.placementInfo` by recipe id: what the book's craftable test
    /// and the server's placement look for.
    pub placements: HashMap<String, Placement>,
    /// Item id -> registry position, to order ingredient choices.
    pub order: HashMap<String, usize>,
    /// The ingredients a server's entry asks for (`craftingRequirements`), by
    /// the entry's display id; none for an entry that sent none.
    pub server_requirements: HashMap<i32, Option<Vec<Ingredient>>>,
    /// The name each group of a server's entries goes by, by the server's group
    /// number (`1 + index`): the name of a book entry kept in that group, so
    /// that the entries taken from the packet join it.
    pub server_groups: HashMap<i32, String>,
}
pub(crate) fn crafting_category(name: &str) -> Category {
    match name {
        "building" => Category::CraftingBuilding,
        "redstone" => Category::CraftingRedstone,
        "equipment" => Category::CraftingEquipment,
        _ => Category::CraftingMisc,
    }
}
impl Book {
    pub fn build(recipes: &Recipes, items: &Items) -> Self {
        let mut entries = Vec::new();
        let group = |s: &str| (!s.is_empty()).then(|| s.to_owned());
        for (name, recipe) in &recipes.crafting {
            let (category, grp) = recipes.meta.get(name).cloned().unwrap_or_default();
            let (display, result) = match recipe {
                Recipe::Shaped {
                    width,
                    height,
                    cells,
                    result,
                } => (
                    Display::Shaped {
                        width: *width,
                        height: *height,
                        cells: cells.clone(),
                    },
                    result,
                ),
                Recipe::Shapeless {
                    ingredients,
                    result,
                } => (Display::Shapeless(ingredients.clone()), result),
                // Custom recipes are never learnable; the others show one
                // display per material count (TransmuteRecipe.display).
                Recipe::Special(special) => {
                    for shown in special.book_displays() {
                        entries.push(Entry {
                            id: crate::crafting::recipe_id(name),
                            display_id: -1,
                            category: crafting_category(&category),
                            group: group(&grp),
                            display: shown.display,
                            result: shown.result,
                            variants: shown.variants,
                            stacks: shown.stacks,
                        });
                    }
                    continue;
                }
            };
            entries.push(Entry {
                id: crate::crafting::recipe_id(name),
                display_id: -1,
                category: crafting_category(&category),
                group: group(&grp),
                display,
                result: result.create(),
                variants: Vec::new(),
                stacks: Vec::new(),
            });
        }
        for c in &recipes.cooking {
            let category = match (c.kind.as_str(), c.category.as_str()) {
                ("minecraft:smelting", "food") => Category::FurnaceFood,
                ("minecraft:smelting", "blocks") => Category::FurnaceBlocks,
                ("minecraft:smelting", _) => Category::FurnaceMisc,
                ("minecraft:blasting", "blocks") => Category::BlastBlocks,
                ("minecraft:blasting", _) => Category::BlastMisc,
                ("minecraft:smoking", _) => Category::SmokerFood,
                _ => continue,
            };
            entries.push(Entry {
                id: crate::crafting::recipe_id(&c.id),
                display_id: -1,
                category,
                group: group(&c.group),
                display: Display::Furnace(c.ingredient.clone()),
                result: c.result.clone(),
                variants: Vec::new(),
                stacks: Vec::new(),
            });
        }
        let order = items
            .order
            .iter()
            .enumerate()
            .map(|(i, id)| (id.to_string(), i))
            .collect();
        let placements = entries
            .iter()
            .filter_map(|e| Some((e.id.clone(), recipes.placement(&e.id)?)))
            .collect();
        Self {
            entries,
            placements,
            order,
            server_requirements: HashMap::new(),
            server_groups: HashMap::new(),
        }
    }
    /// `RecipeDisplayEntry.canCraft`: the recipe's placement ingredients
    /// (`craftingRequirements`) can all be found in `stock`.
    pub fn can_craft(&self, entry: &Entry, stock: &crate::menu::place::Stacked) -> bool {
        // A server's entry: the ingredients it sent, none meaning not craftable.
        if let Some(requirements) = self.server_requirements.get(&entry.display_id) {
            return requirements
                .as_ref()
                .is_some_and(|r| stock.can_craft(&r.iter().collect::<Vec<_>>()));
        }
        self.placements
            .get(&entry.id)
            .is_some_and(|p| stock.can_craft(&p.ingredients.iter().collect::<Vec<_>>()))
    }
    /// Numbers the entries as `RecipeManager.unpackRecipeInfo` numbers the
    /// displays: every recipe in id order, each with its displays.
    pub fn set_display_ids(&mut self, index: &RecipeIndex) {
        let mut previous: Option<(String, i32)> = None;
        for e in &mut self.entries {
            let next = match &previous {
                Some((id, last)) if *id == e.id => last + 1,
                _ => index.first_display(&e.id).unwrap_or(-1),
            };
            e.display_id = next;
            previous = Some((e.id.clone(), next));
        }
    }
    /// The entry of a display id.
    pub fn entry_of_display(&self, display: i32) -> Option<usize> {
        self.entries.iter().position(|e| e.display_id == display)
    }
    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }
    /// `ingredient`'s items as its display resolves them (the cycling order
    /// of ghost slots and the overlay).
    pub fn items_of(&self, ingredient: &Ingredient) -> Vec<String> {
        ingredient.order.clone()
    }
    /// `RecipeBookComponent.fillGhostRecipe` of a book type: the result and the
    /// ingredients faded into the menu's slots, in the order the component
    /// sets them. `grid` is the side of the crafting grid, `fuel_empty`
    /// whether the furnace's fuel slot is empty.
    pub fn ghost(
        &self,
        entry: &Entry,
        book_type: BookType,
        grid: usize,
        fuel_empty: bool,
        items: &crate::items::Items,
    ) -> Vec<GhostSlot> {
        let stacks = |ingredient: &Ingredient| -> Vec<ItemStack> {
            self.items_of(ingredient)
                .iter()
                .map(|i| ItemStack::new(i, 1))
                .collect()
        };
        let mut out = Vec::new();
        match book_type {
            BookType::Crafting => {
                out.push(GhostSlot {
                    target: GhostTarget::Result,
                    items: entry.results().to_vec(),
                });
                let requirements = entry.requirements();
                for (ingredient, index) in entry.grid_cells(grid, grid) {
                    let Some(i) = ingredient else { continue };
                    let list = match entry.stacks.iter().find(|s| s.0 == i) {
                        Some(shown) => shown.1.clone(),
                        None => stacks(requirements[i]),
                    };
                    if !list.is_empty() {
                        out.push(GhostSlot {
                            target: GhostTarget::Grid(index),
                            items: list,
                        });
                    }
                }
            }
            _ => {
                out.push(GhostSlot {
                    target: GhostTarget::Result,
                    items: entry.results().to_vec(),
                });
                if let Display::Furnace(ingredient) = &entry.display {
                    out.push(GhostSlot {
                        target: GhostTarget::Input,
                        items: stacks(ingredient),
                    });
                    if fuel_empty {
                        // `SlotDisplay.AnyFuel`: every item with a cooking fuel
                        // component, the registry's component lookup listing the
                        // items of one component value together, the values in
                        // the order of their first item.
                        let mut groups: Vec<(String, Vec<ItemStack>)> = Vec::new();
                        for item in &items.registry_order {
                            let Some(fuel) = items.get(item).and_then(|i| i.fuel.as_ref()) else {
                                continue;
                            };
                            let key = format!("{fuel:?}");
                            match groups.iter_mut().find(|g| g.0 == key) {
                                Some(group) => group.1.push(ItemStack::new(item, 1)),
                                None => groups.push((key, vec![ItemStack::new(item, 1)])),
                            }
                        }
                        let fuels = groups.into_iter().flat_map(|g| g.1).collect();
                        out.push(GhostSlot {
                            target: GhostTarget::Fuel,
                            items: fuels,
                        });
                    }
                }
            }
        }
        // `GhostSlots.setSlot` keeps only slots with something to show.
        out.retain(|g| !g.items.is_empty());
        out
    }
    /// The recipe-book search tree (`SessionSearchTrees.updateRecipes`, a
    /// `FullTextSearchTree`): without a colon the text must occur in a
    /// tooltip line of a result item of the collection; `namespace:path`
    /// needs the namespace in a result id and the path in an id or a line.
    pub fn collection_matches(
        &self,
        entries: &[usize],
        query: &str,
        tooltip: &dyn Fn(&ItemStack) -> Vec<String>,
    ) -> bool {
        let query = query.to_lowercase();
        let results: Vec<&ItemStack> = entries
            .iter()
            .flat_map(|&i| self.entries[i].results())
            .collect();
        let in_lines = |text: &str| {
            results
                .iter()
                .any(|r| tooltip(r).iter().any(|l| l.to_lowercase().contains(text)))
        };
        let Some((namespace, path)) = query.split_once(':') else {
            return in_lines(&query);
        };
        let (namespace, path) = (namespace.trim(), path.trim());
        let id = |r: &&ItemStack| {
            let (ns, p) = r.item.split_once(':').unwrap_or(("minecraft", &r.item));
            (ns.to_lowercase(), p.to_lowercase())
        };
        results.iter().any(|r| id(r).0.contains(namespace))
            && (results.iter().any(|r| id(r).1.contains(path)) || in_lines(path))
    }
}

/// Where a ghost recipe's stacks are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GhostTarget {
    Result,
    /// A crafting grid slot, row by row.
    Grid(usize),
    /// A furnace's input and fuel slots.
    Input,
    Fuel,
}
/// One slot of a ghost recipe and the stacks it cycles through.
#[derive(Clone, Debug)]
pub struct GhostSlot {
    pub target: GhostTarget,
    pub items: Vec<ItemStack>,
}

/// A `RecipeDisplayId` as a `HashMap` key: a record of one int hashes as it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DisplayKey(i32);
impl horizon_world::java_hash_map::JavaKey for DisplayKey {
    fn java_hash(&self) -> i32 {
        self.0
    }
    fn tie_break(&self, other: &Self) -> std::cmp::Ordering {
        if self.0 < other.0 {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    }
}

/// `ClientRecipeBook`: the displays the client was sent, in a `HashMap` whose
/// iteration order (the display ids' buckets, then arrival order) decides the
/// order of the book's buttons.
#[derive(Default)]
pub struct ClientBook {
    known: horizon_world::java_hash_map::JavaHashMap<DisplayKey, ()>,
    pub highlight: std::collections::HashSet<i32>,
}
impl ClientBook {
    pub fn add(&mut self, display: i32) {
        self.known.put(DisplayKey(display), ());
    }
    pub fn remove(&mut self, display: i32) {
        self.known.remove(&DisplayKey(display));
        self.highlight.remove(&display);
    }
    /// `clear` (a replacing `ClientboundRecipeBookAddPacket`): the table keeps its size.
    pub fn clear(&mut self) {
        self.known.clear();
        self.highlight.clear();
    }
    /// The known display ids in iteration order.
    pub fn displays(&self) -> Vec<i32> {
        self.known.entries().map(|(k, _)| k.0).collect()
    }
    /// `rebuildCollections` then `getCollection(tab)`: per category, a
    /// collection per entry without a group or per group, in the order the
    /// known displays iterate; the search tab lists its categories in turn.
    pub fn collections(&self, book: &Book, book_type: BookType, tab: Tab) -> Vec<Collection> {
        let by_display: HashMap<i32, usize> = book
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.display_id, i))
            .collect();
        let mut by_category: HashMap<Category, Vec<Collection>> = HashMap::new();
        let mut groups: HashMap<(Category, String), (Category, usize)> = HashMap::new();
        for display in self.displays() {
            let Some(&i) = by_display.get(&display) else {
                continue;
            };
            let e = &book.entries[i];
            let list = by_category.entry(e.category).or_default();
            match &e.group {
                None => list.push(Collection { entries: vec![i] }),
                Some(g) => match groups.get(&(e.category, g.clone())) {
                    Some(&(_, at)) => list[at].entries.push(i),
                    None => {
                        groups.insert((e.category, g.clone()), (e.category, list.len()));
                        list.push(Collection { entries: vec![i] });
                    }
                },
            }
        }
        let categories: Vec<Category> = match tab {
            Tab::Search => book_type.search_categories().to_vec(),
            Tab::Category(c) => vec![c],
        };
        categories
            .into_iter()
            .flat_map(|c| by_category.remove(&c).unwrap_or_default())
            .collect()
    }
}

/// How a server's packet names things: an item by its registry id, a recipe
/// book category by its id, and the items of a tag.
pub struct PacketNames<'a> {
    pub item: &'a dyn Fn(i32) -> Option<String>,
    pub category: &'a dyn Fn(i32) -> Option<String>,
    pub tag: &'a dyn Fn(&str) -> Vec<String>,
}
impl PacketNames<'_> {
    fn holder_set(&self, set: &HolderSet) -> Vec<String> {
        match set {
            HolderSet::Tag(tag) => (self.tag)(tag.strip_prefix('#').unwrap_or(tag)),
            HolderSet::Items(ids) => ids.iter().filter_map(|&id| (self.item)(id)).collect(),
        }
    }
    /// The stacks a slot display shows, in order (`SlotDisplay.resolveForStacks`):
    /// what a display builds from a component or a dye is not reproduced, it
    /// shows its source.
    fn stacks(&self, display: &SlotDisplay, out: &mut Vec<ItemStack>) {
        match display {
            SlotDisplay::Empty | SlotDisplay::AnyFuel => {}
            SlotDisplay::Item(id) => out.extend((self.item)(*id).map(|n| ItemStack::new(&n, 1))),
            SlotDisplay::ItemStack(stack) => out.extend(
                (self.item)(stack.item).map(|n| ItemStack::new(&n, stack.count.max(1) as u32)),
            ),
            SlotDisplay::Tag(set) => {
                out.extend(self.holder_set(set).iter().map(|n| ItemStack::new(n, 1)))
            }
            SlotDisplay::WithAnyPotion(source) | SlotDisplay::OnlyWithComponent(source, _) => {
                self.stacks(source, out)
            }
            SlotDisplay::Dyed { target, .. } => self.stacks(target, out),
            SlotDisplay::SmithingTrim { base, .. } => self.stacks(base, out),
            SlotDisplay::WithRemainder { input, .. } => self.stacks(input, out),
            SlotDisplay::Composite(list) => list.iter().for_each(|d| self.stacks(d, out)),
        }
    }
    /// A slot display as an ingredient: the items it shows, without repeats.
    fn ingredient(&self, display: &SlotDisplay) -> Ingredient {
        let mut stacks = Vec::new();
        self.stacks(display, &mut stacks);
        let mut items: Vec<String> = Vec::new();
        for stack in stacks {
            if !items.iter().any(|i| **i == *stack.item) {
                items.push(stack.item.to_string());
            }
        }
        Ingredient::of(items)
    }
}

/// What a server's entry comes to in this book.
pub enum FromPacket {
    /// The book's entry (its id is `server:<display id>`), with the ingredients
    /// its craftable filter checks.
    Shown(Box<Entry>, Option<Vec<Ingredient>>),
    /// This book has no place for it: a stonecutter or smithing display, a
    /// category without a tab, no result.
    Unshowable,
    /// Its category id is not one this client knows (a registry it lacks): not
    /// a judgement of the entry.
    Unknown,
}

/// An entry of a server's `ClientboundRecipeBookAddPacket` as this book's entry.
pub fn entry_from_packet(e: &RecipeBookEntry, names: &PacketNames) -> FromPacket {
    let Some(category_name) = (names.category)(e.category) else {
        return FromPacket::Unknown;
    };
    shown(e, names, &category_name).unwrap_or(FromPacket::Unshowable)
}
fn shown(e: &RecipeBookEntry, names: &PacketNames, category_name: &str) -> Option<FromPacket> {
    let category = Category::from_name(category_name)?;
    let (display, result) = match &e.display {
        RecipeDisplay::Shapeless {
            ingredients,
            result,
            ..
        } => (
            Display::Shapeless(ingredients.iter().map(|d| names.ingredient(d)).collect()),
            result,
        ),
        RecipeDisplay::Shaped {
            width,
            height,
            ingredients,
            result,
            ..
        } => (
            Display::Shaped {
                width: usize::try_from(*width).ok()?,
                height: usize::try_from(*height).ok()?,
                cells: ingredients
                    .iter()
                    .map(|d| (!matches!(d, SlotDisplay::Empty)).then(|| names.ingredient(d)))
                    .collect(),
            },
            result,
        ),
        RecipeDisplay::Furnace {
            ingredient, result, ..
        } => (Display::Furnace(names.ingredient(ingredient)), result),
        RecipeDisplay::Stonecutter { .. } | RecipeDisplay::Smithing { .. } => return None,
    };
    let mut results = Vec::new();
    names.stacks(result, &mut results);
    let first = results.first()?.clone();
    let variants = if results.len() > 1 {
        results
    } else {
        Vec::new()
    };
    let requirements = e.requirements.as_ref().map(|sets| {
        sets.iter()
            .map(|set| Ingredient::of(names.holder_set(set)))
            .collect()
    });
    let entry = Entry {
        id: format!("server:{}", e.id),
        display_id: e.id,
        category,
        group: (e.group > 0).then(|| format!("server:{}", e.group - 1)),
        display,
        result: first,
        variants,
        stacks: Vec::new(),
    };
    Some(FromPacket::Shown(Box::new(entry), requirements))
}

/// The items an ingredient accepts, sorted.
fn items_of(i: &Ingredient) -> Vec<&str> {
    let mut items: Vec<&str> = i.items.iter().map(String::as_str).collect();
    items.sort_unstable();
    items
}
/// The items each ingredient of an entry accepts, in any order of ingredients.
fn sorted_sets(e: &Entry) -> Vec<Vec<&str>> {
    let mut sets: Vec<Vec<&str>> = e.requirements().into_iter().map(items_of).collect();
    sets.sort_unstable();
    sets
}
/// Whether the book's own entry is the recipe a server's entry describes: the
/// same result, category and display. A shaped recipe must have the same size
/// and each cell, empty ones included, accepting the same items; a shapeless or
/// smelting one, the same ingredients in any order.
fn same_recipe(own: &Entry, server: &Entry) -> bool {
    if own.category != server.category || own.result.item != server.result.item {
        return false;
    }
    match (&own.display, &server.display) {
        (
            Display::Shaped {
                width: w,
                height: h,
                cells,
            },
            Display::Shaped {
                width: sw,
                height: sh,
                cells: scells,
            },
        ) => {
            (w, h) == (sw, sh)
                && cells.len() == scells.len()
                && cells
                    .iter()
                    .zip(scells)
                    .all(|(a, b)| a.as_ref().map(items_of) == b.as_ref().map(items_of))
        }
        (Display::Shapeless(_), Display::Shapeless(_))
        | (Display::Furnace(_), Display::Furnace(_)) => sorted_sets(own) == sorted_sets(server),
        _ => false,
    }
}

impl Book {
    /// Puts a server's entries over this book. The book numbers its own
    /// recipes the way a server of the same recipes does, but a server with
    /// a plugin's recipes, a datapack or another order numbers them
    /// otherwise: the client of vanilla keeps the entries the packet carries
    /// (`ClientRecipeBook.add`), so each is taken as it came. The book's own
    /// entry stands when it is the same recipe (`same_recipe`), so a server of
    /// this client's own recipes shows what it always did, and the entries
    /// taken join the groups of the ones kept. Where the server's entry cannot
    /// be shown here, the book's own entry of that number goes: it is another
    /// recipe. An entry of a category this client does not know changes nothing.
    pub fn use_server_entries<'a>(
        &mut self,
        entries: impl IntoIterator<Item = &'a RecipeBookEntry>,
        names: &PacketNames,
    ) {
        enum Act {
            Keep,
            Take(Box<Entry>, Option<Vec<Ingredient>>),
            Hide,
        }
        let mut acts = Vec::new();
        for e in entries {
            let at = self.entry_of_display(e.id);
            let act = match entry_from_packet(e, names) {
                FromPacket::Unknown => Act::Keep,
                FromPacket::Unshowable => Act::Hide,
                FromPacket::Shown(entry, requirements) => match at {
                    Some(i) if same_recipe(&self.entries[i], &entry) => {
                        // The kept entry names its server group: the entries
                        // already taken from it take that name too.
                        if e.group > 0
                            && let Some(name) = self.entries[i].group.clone()
                        {
                            let old = self.server_groups.insert(e.group, name.clone());
                            if let Some(old) =
                                old.filter(|old| *old != name && old.starts_with("server:"))
                            {
                                for taken in &mut self.entries {
                                    if taken.id.starts_with("server:")
                                        && taken.group.as_deref() == Some(old.as_str())
                                    {
                                        taken.group = Some(name.clone());
                                    }
                                }
                            }
                        }
                        if !self.entries[i].id.starts_with("server:") {
                            self.server_requirements.remove(&e.id);
                        }
                        Act::Keep
                    }
                    _ => Act::Take(entry, requirements),
                },
            };
            acts.push((e, at, act));
        }
        for (e, at, act) in acts {
            match act {
                Act::Keep => {}
                Act::Hide => {
                    if let Some(i) = at {
                        self.entries[i].display_id = -1;
                    }
                    self.server_requirements.remove(&e.id);
                }
                Act::Take(mut entry, requirements) => {
                    if e.group > 0 {
                        entry.group = Some(
                            self.server_groups
                                .entry(e.group)
                                .or_insert_with(|| format!("server:{}", e.group - 1))
                                .clone(),
                        );
                    }
                    match at {
                        Some(i) => self.entries[i] = *entry,
                        None => self.entries.push(*entry),
                    }
                    self.server_requirements.insert(e.id, requirements);
                }
            }
        }
    }
}

/// What the server knows of each recipe of the datapack: whether it is
/// special (`Recipe.isSpecial`: never learnable) and how many recipe book
/// displays it has (`RecipeManager.listDisplaysForRecipe`).
#[derive(Clone, Debug, Default)]
pub struct RecipeIndex {
    recipes: std::collections::BTreeMap<String, Known>,
}
#[derive(Clone, Debug)]
struct Known {
    special: bool,
    displays: usize,
    first_display: i32,
    /// `RecipeDisplay.craftingStation`: the workstation item of its displays.
    station: &'static str,
    /// `Recipe.showNotification`: unlocking it shows the `RecipeToast`.
    notify: bool,
    /// `result.id`, for the recipes the book doesn't list.
    result: Option<String>,
}
/// `RecipeDisplay.craftingStation` of a recipe type.
pub(crate) fn crafting_station(kind: &str) -> &'static str {
    match kind {
        "minecraft:smelting" => "minecraft:furnace",
        "minecraft:blasting" => "minecraft:blast_furnace",
        "minecraft:smoking" => "minecraft:smoker",
        "minecraft:campfire_cooking" => "minecraft:campfire",
        "minecraft:stonecutting" => "minecraft:stonecutter",
        "minecraft:brewing" => "minecraft:brewing_stand",
        "minecraft:smithing_transform" | "minecraft:smithing_trim" => "minecraft:smithing_table",
        _ => "minecraft:crafting_table",
    }
}
impl RecipeIndex {
    /// Reads vanilla and content namespaces' recipes under `root`, with
    /// the world's packs' recipe files over them (`overlay`).
    pub fn load(root: &std::path::Path, overlay: &PackOverlay) -> Self {
        let mut recipes = std::collections::BTreeMap::new();
        let mut insert = |id: String, v: &serde_json::Value| {
            let kind = v["type"].as_str().unwrap_or_default();
            let special = kind.starts_with("minecraft:crafting_special_")
                || kind == "minecraft:crafting_decorated_pot"
                // A brewing recipe has no recipe book display of its own
                // (`BrewingRecipe`), so it never takes display numbers.
                || kind == "minecraft:brewing";
            let displays = if special {
                0
            } else if kind == "minecraft:crafting_transmute" {
                // TransmuteRecipe.display: one per material count.
                match &v["material_count"] {
                    serde_json::Value::Object(o) => {
                        let min = o.get("min").and_then(|m| m.as_u64()).unwrap_or(1);
                        let max = o.get("max").and_then(|m| m.as_u64()).unwrap_or(8);
                        (max.saturating_sub(min) + 1) as usize
                    }
                    _ => 1,
                }
            } else {
                1
            };
            recipes.insert(
                id,
                Known {
                    special,
                    displays,
                    first_display: 0,
                    station: crafting_station(kind),
                    notify: v["show_notification"].as_bool().unwrap_or(true),
                    result: horizon_world::loot::recipe_result_id(&v["result"]).map(str::to_owned),
                },
            );
        };
        let dir = root.join("data/minecraft/recipe");
        // NeoForge's load conditions: the book lists only what loaded; a
        // recipe gated on an absent mod isn't in the manager either.
        let mods: Vec<String> = {
            let mut mods = horizon_items::content_namespaces::mod_ids(root);
            if !mods.iter().any(|m| m == "minecraft") {
                mods.push("minecraft".to_owned());
            }
            mods
        };
        let conditions_hold = |v: &serde_json::Value| {
            horizon_world::conditions::conditions_hold(v, &|id: &str| mods.iter().any(|m| m == id))
        };
        // The whole folder, `brewing/` under it too, in id order
        // (`SimpleJsonResourceReloadListener` walks it recursively): a
        // subfolder's recipe keeps it in its id, `minecraft:brewing/<name>`.
        // The packs' files replace first and the gate answers the merged
        // file, as the crafting loader's does: a false-conditioned pack
        // file leaves the lower one gone too.
        let folder = crate::json_folder::read(&dir).ok();
        let mut sources: Vec<(String, serde_json::Value)> = folder
            .iter()
            .flat_map(|folder| &folder.files)
            .filter_map(|file| {
                let v = serde_json::from_slice::<serde_json::Value>(file.bytes().ok()?).ok()?;
                Some((format!("minecraft:{}", file.name), v))
            })
            .collect();
        for namespace in crate::mod_content::namespaces(root) {
            if namespace == "minecraft" {
                continue;
            }
            if let Ok(folder) =
                crate::json_folder::read(&root.join(format!("data/{namespace}/recipe")))
            {
                sources.extend(folder.files.iter().filter_map(|file| {
                    let v = serde_json::from_slice::<serde_json::Value>(file.bytes().ok()?).ok()?;
                    Some((format!("{namespace}:{}", file.name), v))
                }));
            }
        }
        for (id, files) in overlay.kind("recipe") {
            match serde_json::from_slice::<serde_json::Value>(PackOverlay::top(files)) {
                Ok(v) => match sources.iter_mut().find(|(n, _)| n == id) {
                    Some(slot) => slot.1 = v,
                    None => sources.push((id.to_owned(), v)),
                },
                Err(e) => log::warn!("Horizon: pack recipe {id} not shown in the book: {e}"),
            }
        }
        let filters = horizon_items::recipe_filters::RecipeFilters::load(root, overlay);
        for (id, mut v) in sources {
            if conditions_hold(&v) && filters.apply(&id, &mut v) {
                insert(id, &v);
            }
        }
        // RecipeManager.unpackRecipeInfo numbers the displays in registration
        // order (path, then namespace), not in the order of the ids' text.
        let mut order: Vec<_> = recipes
            .iter_mut()
            .map(|(id, r)| (crate::crafting::registration_key(id), r))
            .collect();
        order.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        let mut next = 0;
        for (_, r) in order {
            r.first_display = next;
            next += r.displays as i32;
        }
        Self { recipes }
    }
    pub fn contains(&self, id: &str) -> bool {
        self.recipes.contains_key(id)
    }
    pub fn special(&self, id: &str) -> bool {
        self.recipes.get(id).is_some_and(|r| r.special)
    }
    pub fn displays(&self, id: &str) -> usize {
        self.recipes.get(id).map_or(0, |r| r.displays)
    }
    /// The crafting station shown on its displays' toast, when unlocking it
    /// shows one (`Recipe.showNotification`).
    pub fn toast_station(&self, id: &str) -> Option<&'static str> {
        self.recipes.get(id).filter(|r| r.notify).map(|r| r.station)
    }
    /// The result item id its file names (none for trims).
    pub fn result(&self, id: &str) -> Option<&str> {
        self.recipes.get(id).and_then(|r| r.result.as_deref())
    }
    /// Every recipe id, in id order.
    pub fn ids(&self) -> impl Iterator<Item = &String> {
        self.recipes.keys()
    }
    /// The display ids of a recipe (none for a special one).
    pub fn display_ids(&self, id: &str) -> std::ops::Range<i32> {
        self.recipes.get(id).map_or(0..0, |r| {
            r.first_display..r.first_display + r.displays as i32
        })
    }
    /// The recipe a display id belongs to (`RecipeManager.getRecipeFromDisplay`).
    pub fn recipe_of_display(&self, display: i32) -> Option<&str> {
        self.recipes
            .iter()
            .find(|(_, r)| {
                (r.first_display..r.first_display + r.displays as i32).contains(&display)
            })
            .map(|(id, _)| id.as_str())
    }
    pub fn first_display(&self, id: &str) -> Option<i32> {
        self.recipes
            .get(id)
            .filter(|r| r.displays > 0)
            .map(|r| r.first_display)
    }
}

/// The recipe ids a knowledge book names: its `minecraft:recipes` component
/// (`Recipe.KEY_CODEC.listOf()`, the namespace defaulting to minecraft).
fn knowledge_recipes(stack: &ItemStack) -> Vec<String> {
    let Some(Tag::List(ids)) = stack.data().and_then(|d| d.other("minecraft:recipes")) else {
        return Vec::new();
    };
    ids.iter()
        .filter_map(|t| match t {
            Tag::String(id) => Some(id.to_string_lossy()),
            _ => None,
        })
        .map(|id| {
            if id.contains(':') {
                id
            } else {
                format!("minecraft:{id}")
            }
        })
        .collect()
}

/// `ServerRecipeBook`: the player's `recipeBook` compound, the known recipes
/// and the ones still to highlight (`toBeDisplayed`) with the open and
/// filter settings of each `RecipeBookType`.
#[derive(Clone, Debug, Default)]
pub struct Settings {
    /// `isGuiOpen`-style flags per `BookType`.
    pub open: [bool; 4],
    pub filtering: [bool; 4],
    /// Known recipe ids (`recipes`).
    pub recipes: BTreeSet<String>,
    /// Recipes not yet shown in the book (`toBeDisplayed`): they animate once.
    pub to_be_displayed: BTreeSet<String>,
}
impl Settings {
    /// `ServerRecipeBook.addRecipes`: the displays added (the count
    /// `/recipe give` reports) and the recipes now known, in order.
    pub fn add_recipes(&mut self, index: &RecipeIndex, ids: &[String]) -> (usize, Vec<String>) {
        let mut displays = 0;
        let mut added = Vec::new();
        for id in ids {
            // The whole id: the crafting copy names a `minecraft:` recipe
            // bare, and every reader of the book (the network copy, the
            // advancements' JSON, the saved file) knows it qualified.
            let id = crate::crafting::qualified(id);
            if !self.recipes.contains(&id) && !index.special(&id) {
                self.recipes.insert(id.clone());
                self.to_be_displayed.insert(id.clone());
                displays += index.displays(&id);
                added.push(id);
            }
        }
        (displays, added)
    }
    /// `KnowledgeBookItem.use` for the stack in the acting hand: `Ok` is its
    /// `InteractionResult.SUCCESS`, with the recipes now known; `Err` is
    /// `FAIL`, with the recipe id vanilla logs (`Invalid recipe: {}`) or
    /// `None` for a book that names none. The book is used up first, as
    /// vanilla consumes it before either check (`ItemStack.consume(1, player)`;
    /// a player with infinite materials keeps it).
    pub fn use_knowledge_book(
        &mut self,
        index: &RecipeIndex,
        hand: &mut Option<ItemStack>,
        infinite: bool,
    ) -> Result<Vec<String>, Option<String>> {
        let recipes = hand.as_ref().map_or_else(Vec::new, knowledge_recipes);
        if !infinite && let Some(stack) = hand.as_mut() {
            stack.count -= 1;
            if stack.count == 0 {
                *hand = None;
            }
        }
        if recipes.is_empty() {
            return Err(None);
        }
        // `RecipeManager.byKey`: one id this server has no recipe for stops
        // the use, the recipes beside it unlearned.
        if let Some(i) = recipes.iter().position(|id| !index.contains(id)) {
            return Err(Some(recipes[i].clone()));
        }
        Ok(self.add_recipes(index, &recipes).1)
    }
    /// `ServerRecipeBook.removeRecipes`: the display ids removed, which its
    /// `ClientboundRecipeBookRemovePacket` lists (a transmute recipe has one
    /// per material count).
    pub fn remove_recipes(&mut self, index: &RecipeIndex, ids: &[String]) -> Vec<i32> {
        let mut displays = Vec::new();
        for id in ids {
            let id = crate::crafting::qualified(id);
            if self.recipes.remove(&id) {
                self.to_be_displayed.remove(&id);
                displays.extend(index.display_ids(&id));
            }
        }
        displays
    }
    /// `removeHighlight` (`ServerboundRecipeBookSeenRecipePacket`).
    pub fn remove_highlight(&mut self, id: &str) {
        self.to_be_displayed.remove(id);
    }
    pub fn knows(&self, id: &str) -> bool {
        self.recipes.contains(id)
    }
    /// The compound as saved, every recipe kept.
    pub fn read(tag: Option<&Tag>) -> Self {
        Self::read_valid(tag, &|_| true)
    }
    /// `ServerRecipeBook.loadUntrusted`: unknown recipes are dropped. Without
    /// both lists the compound does not decode and nothing is read.
    pub fn read_valid(tag: Option<&Tag>, valid: &dyn Fn(&str) -> bool) -> Self {
        let mut s = Self::default();
        let Some(tag) = tag else { return s };
        let ids = |key: &str| match tag.get(key) {
            Some(Tag::List(list)) => Some(
                list.iter()
                    .filter_map(|t| match t {
                        Tag::String(id) => Some(id.to_string_lossy()),
                        _ => None,
                    })
                    .map(|id| {
                        if id.contains(':') {
                            id
                        } else {
                            format!("minecraft:{id}")
                        }
                    })
                    .filter(|id| valid(id))
                    .collect::<BTreeSet<String>>(),
            ),
            _ => None,
        };
        let (Some(recipes), Some(to_be_displayed)) = (ids("recipes"), ids("toBeDisplayed")) else {
            return s;
        };
        let flag = |k: &str| matches!(tag.get(k), Some(Tag::Byte(b)) if *b != 0);
        for b in BookType::ALL {
            let (open, filter) = b.keys();
            s.open[b.index()] = flag(open);
            s.filtering[b.index()] = flag(filter);
        }
        s.recipes = recipes;
        s.to_be_displayed = to_be_displayed;
        s
    }
    /// The compound written to the player file (vanilla's lists follow an
    /// identity hash order; these are sorted).
    pub fn write(&self) -> Tag {
        let mut entries: Vec<(&str, Tag)> = Vec::new();
        for b in BookType::ALL {
            let (open, filter) = b.keys();
            // The codec omits settings at their default (false).
            if self.open[b.index()] {
                entries.push((open, Tag::Byte(1)));
            }
            if self.filtering[b.index()] {
                entries.push((filter, Tag::Byte(1)));
            }
        }
        let list = |ids: &BTreeSet<String>| {
            Tag::List(ids.iter().map(|i| Tag::String(i.as_str().into())).collect())
        };
        entries.push(("recipes", list(&self.recipes)));
        entries.push(("toBeDisplayed", list(&self.to_be_displayed)));
        Tag::compound(entries)
    }
}

#[cfg(test)]
mod tests {
    /// A book saved before #7186's fix holds bare names: reading fills the
    /// namespace in, so those recipes show in the book too.
    #[test]
    fn a_book_saved_with_bare_names_reads_qualified() {
        use horizon_world::nbt::{Compound, Tag};
        let list = || Tag::List(vec![Tag::String("folder_recipe".into())]);
        let mut compound = Compound::new();
        compound.insert("recipes".into(), list());
        compound.insert("toBeDisplayed".into(), list());
        let tag = Tag::Compound(compound);
        let read = Settings::read_valid(Some(&tag), &|_| true);
        assert_eq!(
            read.recipes,
            ["minecraft:folder_recipe".to_owned()].into_iter().collect()
        );
        assert!(read.to_be_displayed.contains("minecraft:folder_recipe"));
    }

    use super::*;

    #[test]
    fn mod_category_mapping_validates_ids_icons_and_conflicts() {
        let root =
            std::env::temp_dir().join(format!("horizon-book-categories-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("items.json"),
            r#"{"minecraft:stone":{},"mymod:crusher":{},"minecraft:air":{}}"#,
        )
        .unwrap();
        let items = Items::catalog(&root, &PackOverlay::NONE).unwrap();
        let recipes = Recipes::from_sources(
            &root,
            &items,
            &PackOverlay::NONE,
            vec![
                (
                    "mymod:crush".into(),
                    serde_json::json!({"type":"mymod:crusher"}),
                ),
                (
                    "mymod:mix".into(),
                    serde_json::json!({"type":"mymod:mixer"}),
                ),
            ],
        )
        .unwrap();
        let declare = |id: &str, recipe_type: &str, icon: &str| RecipeBookCategory {
            id: id.into(),
            recipe_type: recipe_type.into(),
            icon: icon.into(),
        };
        let crusher = declare("mymod:crushing", "mymod:crusher", "mymod:crusher");
        let mixer = declare("mymod:mixing", "mymod:mixer", "minecraft:stone");
        let mapped = mod_categories(
            vec![
                declare("mymod:crushing", "mymod:crusher", ""),
                crusher.clone(),
                declare("mymod:duplicate", "mymod:crusher", "minecraft:stone"),
                declare("mymod:crushing", "mymod:mixer", "minecraft:stone"),
                mixer.clone(),
            ],
            &recipes,
            &items,
        );
        assert_eq!(mapped.len(), 2);
        assert_eq!(mapped.get("mymod:crusher"), Some(&crusher));
        assert_eq!(mapped.get("mymod:mixer"), Some(&mixer));
        assert!(!mapped.contains_key("minecraft:crafting"));
        for invalid in [
            declare(
                "minecraft:crafting_misc",
                "mymod:crusher",
                "minecraft:stone",
            ),
            declare("bare", "mymod:crusher", "minecraft:stone"),
            declare("mymod:", "mymod:crusher", "minecraft:stone"),
            declare(":category", "mymod:crusher", "minecraft:stone"),
            declare("mymod:Bad", "mymod:crusher", "minecraft:stone"),
            declare("mymod:bad:id", "mymod:crusher", "minecraft:stone"),
            declare("mymod:crushing", "mymod:absent", "minecraft:stone"),
            declare("mymod:crushing", "minecraft:smelting", "minecraft:stone"),
            declare("mymod:crushing", "mymod:crusher", ""),
            declare("mymod:crushing", "mymod:crusher", "minecraft:absent"),
            declare("mymod:crushing", "mymod:crusher", "minecraft:air"),
        ] {
            assert!(
                mod_categories(vec![invalid.clone()], &recipes, &items).is_empty(),
                "{invalid:?}"
            );
        }
        assert!(mod_categories(Vec::new(), &recipes, &items).is_empty());
        let vanilla = Recipes::from_sources(&root, &items, &PackOverlay::NONE, Vec::new()).unwrap();
        assert!(mod_categories(vec![crusher], &vanilla, &items).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn ing(items: &[&str]) -> Ingredient {
        Ingredient::of(items.iter().map(|s| (*s).to_owned()).collect())
    }
    #[test]
    fn the_index_knows_each_recipes_toast_station_and_result() {
        let root = std::env::temp_dir().join(format!("horizon-toast-index-{}", std::process::id()));
        let dir = root.join("data/minecraft/recipe");
        std::fs::create_dir_all(&dir).unwrap();
        for (name, json) in [
            (
                "stick",
                r#"{"type":"minecraft:crafting_shaped","result":{"id":"minecraft:stick"}}"#,
            ),
            (
                "crafting_table",
                r#"{"type":"minecraft:crafting_shaped","show_notification":false,"result":{"id":"minecraft:crafting_table"}}"#,
            ),
            (
                "cooked_beef_from_campfire_cooking",
                r#"{"type":"minecraft:campfire_cooking","result":{"id":"minecraft:cooked_beef"}}"#,
            ),
            (
                "stone_slab_from_stonecutting",
                r#"{"type":"minecraft:stonecutting","result":{"id":"minecraft:stone_slab"}}"#,
            ),
            (
                "coast_trim",
                r#"{"type":"minecraft:smithing_trim","pattern":"minecraft:coast"}"#,
            ),
        ] {
            std::fs::write(dir.join(format!("{name}.json")), json).unwrap();
        }
        let index = RecipeIndex::load(&root, &PackOverlay::NONE);
        std::fs::remove_dir_all(&root).unwrap();
        let toast = |id: &str| (index.toast_station(id), index.result(id));
        assert_eq!(
            toast("minecraft:stick"),
            (Some("minecraft:crafting_table"), Some("minecraft:stick"))
        );
        // show_notification: false unlocks silently.
        assert_eq!(toast("minecraft:crafting_table").0, None);
        assert_eq!(
            toast("minecraft:cooked_beef_from_campfire_cooking"),
            (Some("minecraft:campfire"), Some("minecraft:cooked_beef"))
        );
        assert_eq!(
            toast("minecraft:stone_slab_from_stonecutting").0,
            Some("minecraft:stonecutter")
        );
        assert_eq!(
            toast("minecraft:coast_trim"),
            (Some("minecraft:smithing_table"), None)
        );
    }
    #[test]
    fn content_namespace_recipes_have_display_ids() {
        let root = std::env::temp_dir().join(format!(
            "horizon-mod-recipe-index-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let vanilla = root.join("data/minecraft/recipe");
        let content = root.join("data/recipe_pilot/recipe");
        std::fs::create_dir_all(&vanilla).unwrap();
        std::fs::create_dir_all(&content).unwrap();
        for marker in [
            horizon_items::content_namespaces::MARKER,
            horizon_items::content_namespaces::MOD_IDS_MARKER,
        ] {
            std::fs::write(root.join(marker), br#"["recipe_pilot"]"#).unwrap();
        }
        std::fs::write(
            vanilla.join("control.json"),
            br#"{"type":"minecraft:crafting_shapeless","result":{"id":"minecraft:stick"}}"#,
        )
        .unwrap();
        std::fs::write(
            content.join("flint_to_echo_shard.json"),
            br#"{"type":"minecraft:crafting_shapeless","ingredients":["minecraft:flint"],"result":{"id":"minecraft:echo_shard","count":2}}"#,
        )
        .unwrap();
        std::fs::write(
            content.join("gated.json"),
            br#"{"type":"minecraft:crafting_shapeless","neoforge:conditions":[{"type":"neoforge:mod_loaded","modid":"absent"}],"result":{"id":"minecraft:diamond"}}"#,
        )
        .unwrap();
        let index = RecipeIndex::load(&root, &PackOverlay::NONE);
        std::fs::remove_dir_all(&root).unwrap();
        let id = "recipe_pilot:flint_to_echo_shard";
        let display = index
            .first_display(id)
            .expect("content recipe has a display ID");
        assert!(display >= 0);
        assert_eq!(index.recipe_of_display(display), Some(id));
        assert_eq!(index.result(id), Some("minecraft:echo_shard"));
        assert!(index.first_display("minecraft:control").is_some());
        assert!(!index.contains("recipe_pilot:gated"));
    }

    #[test]
    fn small_recipes_are_centred_in_the_grid() {
        // One ingredient in a 3x3 grid goes to the middle cell.
        assert_eq!(place_cells(3, 3, 1, 1, &['a']), vec![('a', 4)]);
        // A 2x2 recipe fills the top-left corner.
        let cells: Vec<usize> = place_cells(3, 3, 2, 2, &['a', 'b', 'c', 'd'])
            .into_iter()
            .map(|(_, i)| i)
            .collect();
        assert_eq!(cells, vec![0, 1, 3, 4]);
        // A one-row recipe sits on the middle row.
        let row: Vec<usize> = place_cells(3, 3, 3, 1, &['a', 'b', 'c'])
            .into_iter()
            .map(|(_, i)| i)
            .collect();
        assert_eq!(row, vec![3, 4, 5]);
        // The 2x2 grid is never centred.
        let small: Vec<usize> = place_cells(2, 2, 1, 1, &['a'])
            .into_iter()
            .map(|(_, i)| i)
            .collect();
        assert_eq!(small, vec![0]);
    }
    #[test]
    fn collections_group_recipes_of_a_category() {
        let entry = |id: &str, display_id, group: Option<&str>, category| Entry {
            id: id.into(),
            display_id,
            category,
            group: group.map(str::to_owned),
            display: Display::Shapeless(vec![ing(&["minecraft:stick"])]),
            result: ItemStack::new("minecraft:stick", 1),
            variants: Vec::new(),
            stacks: Vec::new(),
        };
        let book = Book {
            entries: vec![
                entry("minecraft:a", 0, Some("planks"), Category::CraftingBuilding),
                entry("minecraft:b", 1, Some("planks"), Category::CraftingBuilding),
                entry("minecraft:c", 2, None, Category::CraftingBuilding),
                entry("minecraft:d", 3, Some("planks"), Category::CraftingMisc),
            ],
            placements: HashMap::new(),
            order: HashMap::new(),
            server_requirements: HashMap::new(),
            server_groups: HashMap::new(),
        };
        let mut client = ClientBook::default();
        for d in 0..4 {
            client.add(d);
        }
        let tab = Tab::Category(Category::CraftingBuilding);
        let c = client.collections(&book, BookType::Crafting, tab);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].entries, vec![0, 1]);
        // The search tab lists every category; the same group in another category is separate.
        let all = client.collections(&book, BookType::Crafting, Tab::Search);
        assert_eq!(all.len(), 3);
        // Displays the client was not sent are not listed.
        client.clear();
        client.add(2);
        assert_eq!(client.collections(&book, BookType::Crafting, tab).len(), 1);
    }
    #[test]
    fn display_ids_collide_in_a_small_table() {
        // A 16-bucket table: 17 and 1 share a bucket and keep arrival order.
        let mut client = ClientBook::default();
        for d in [17, 3, 1] {
            client.add(d);
        }
        assert_eq!(client.displays(), vec![17, 1, 3]);
    }
    #[test]
    fn search_reads_tooltip_lines_and_ids() {
        let book = Book {
            entries: vec![Entry {
                id: "minecraft:stick".into(),
                display_id: 0,
                category: Category::CraftingMisc,
                group: None,
                display: Display::Shapeless(Vec::new()),
                result: ItemStack::new("minecraft:stick", 4),
                variants: Vec::new(),
                stacks: Vec::new(),
            }],
            placements: HashMap::new(),
            order: HashMap::new(),
            server_requirements: HashMap::new(),
            server_groups: HashMap::new(),
        };
        let lines = |s: &ItemStack| vec![format!("Item {}", s.item), "Stick".to_owned()];
        assert!(book.collection_matches(&[0], "STI", &lines));
        // Plain text searches tooltips only, not the registry path.
        assert!(!book.collection_matches(&[0], "stick:", &lines));
        assert!(book.collection_matches(&[0], "minecraft:stic", &lines));
        assert!(!book.collection_matches(&[0], "other:stick", &lines));
    }
    #[test]
    fn settings_round_trip() {
        let mut s = Settings::default();
        s.open[BookType::Furnace.index()] = true;
        s.filtering[BookType::Crafting.index()] = true;
        s.recipes = ["minecraft:stick".to_owned()].into();
        let back = Settings::read(Some(&s.write()));
        assert_eq!(back.open, s.open);
        assert_eq!(back.filtering, s.filtering);
        assert_eq!(back.recipes, s.recipes);
    }
    #[test]
    fn a_knowledge_book_unlocks_its_recipes_and_is_used_up() {
        let root =
            std::env::temp_dir().join(format!("horizon-knowledge-book-{}", std::process::id()));
        let dir = root.join("data/minecraft/recipe");
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["stick", "crafting_table", "torch"] {
            std::fs::write(
                dir.join(format!("{name}.json")),
                format!(
                    r#"{{"type":"minecraft:crafting_shaped","result":{{"id":"minecraft:{name}"}}}}"#
                ),
            )
            .unwrap();
        }
        let index = RecipeIndex::load(&root, &PackOverlay::NONE);
        std::fs::remove_dir_all(&root).unwrap();
        let book = |recipes: &[&str], count: u32| {
            let ids = recipes.iter().map(|id| Tag::String((*id).into())).collect();
            let mut stack = ItemStack::new("minecraft:knowledge_book", count);
            stack
                .data_mut()
                .set_other("minecraft:recipes", Some(Tag::List(ids)));
            stack
        };

        // A player who knows no recipe, with a stack of two books naming two.
        let mut settings = Settings::default();
        let mut hand = Some(book(&["minecraft:stick", "minecraft:crafting_table"], 2));
        let unlocked = settings
            .use_knowledge_book(&index, &mut hand, false)
            .unwrap();
        assert_eq!(unlocked, ["minecraft:stick", "minecraft:crafting_table"]);
        assert!(settings.knows("minecraft:stick"));
        assert!(settings.knows("minecraft:crafting_table"));
        // One book used up, the second still in the hand.
        assert_eq!(hand.as_ref().map(|s| s.count), Some(1));

        // A creative player has infinite materials: the book stays, and a
        // recipe already known is still the success `awardRecipes` reports.
        let mut hand = Some(book(&["minecraft:stick"], 1));
        assert_eq!(
            settings.use_knowledge_book(&index, &mut hand, true),
            Ok(Vec::new())
        );
        assert_eq!(hand.as_ref().map(|s| s.count), Some(1));

        // One id this server has no recipe for (vanilla's "Invalid recipe: {}"):
        // the recipe beside it is not learned either, and the book is gone all
        // the same — vanilla consumes it before it looks the ids up.
        let mut hand = Some(book(&["minecraft:torch", "minecraft:no_such_recipe"], 1));
        assert_eq!(
            settings.use_knowledge_book(&index, &mut hand, false),
            Err(Some("minecraft:no_such_recipe".to_owned()))
        );
        assert!(!settings.knows("minecraft:torch"));
        assert_eq!(hand, None);

        // A book that names no recipe at all (a bare `/give`): nothing to
        // learn, and vanilla fails after using it up like any other.
        let mut hand = Some(ItemStack::new("minecraft:knowledge_book", 1));
        assert_eq!(
            settings.use_knowledge_book(&index, &mut hand, false),
            Err(None)
        );
        assert_eq!(hand, None);
    }

    /// `RecipeManager.unpackRecipeInfo` numbers the displays in registration
    /// order: the file's path (with `.json`) first, then its namespace.
    #[test]
    fn display_ids_follow_registration_order() {
        let root =
            std::env::temp_dir().join(format!("horizon-display-order-{}", std::process::id()));
        let dir = root.join("data/minecraft/recipe");
        std::fs::create_dir_all(&dir).unwrap();
        let recipe = r#"{"type":"minecraft:crafting_shapeless","result":{"id":"minecraft:stick"}}"#;
        for name in ["z_pick", "prefix", "prefix-early"] {
            std::fs::write(dir.join(format!("{name}.json")), recipe).unwrap();
        }
        let packs = [(
            "recipe/parity:a_pick".to_owned(),
            vec![recipe.as_bytes().to_vec()],
        )];
        let index = RecipeIndex::load(&root, &PackOverlay { files: &packs });
        std::fs::remove_dir_all(&root).unwrap();
        let ids = [
            "parity:a_pick",
            "minecraft:prefix-early",
            "minecraft:prefix",
            "minecraft:z_pick",
        ];
        assert_eq!(
            ids.map(|id| index.first_display(id)),
            [Some(0), Some(1), Some(2), Some(3)]
        );
        assert_eq!(index.recipe_of_display(0), Some("parity:a_pick"));
    }

    // ---- a server whose display ids are not this client's (#11705) ----

    use horizon_protocol::packets::recipes::RecipeBookAdd;

    /// The ids of the items the packet below names.
    const ITEMS: [&str; 5] = [
        "minecraft:tnt",
        "minecraft:piston",
        "minecraft:stick",
        "minecraft:diamond",
        "minecraft:gunpowder",
    ];
    fn item_name(id: i32) -> Option<String> {
        ITEMS
            .get(usize::try_from(id).ok()?)
            .map(|n| (*n).to_owned())
    }
    /// The server's category ids: 0 building, 1 redstone, 2 equipment, 3 misc,
    /// 4 stonecutter (a tab this book has none for); any other is unknown.
    fn category_name(id: i32) -> Option<String> {
        if id == 4 {
            return Some("minecraft:stonecutter".to_owned());
        }
        ["building_blocks", "redstone", "equipment", "misc"]
            .get(usize::try_from(id).ok()?)
            .map(|n| format!("minecraft:crafting_{n}"))
    }
    /// The book of this client: it numbers `tnt`, `piston`, `stick` 10, 11, 12.
    fn local_book() -> Book {
        let entry = |id: &str, display_id, category| Entry {
            id: id.into(),
            display_id,
            category,
            group: None,
            display: Display::Shapeless(vec![ing(&["minecraft:gunpowder"])]),
            result: ItemStack::new(id, 1),
            variants: Vec::new(),
            stacks: Vec::new(),
        };
        Book {
            entries: vec![
                entry("minecraft:tnt", 10, Category::CraftingRedstone),
                entry("minecraft:piston", 11, Category::CraftingRedstone),
                entry("minecraft:stick", 12, Category::CraftingMisc),
            ],
            placements: HashMap::new(),
            order: HashMap::new(),
            server_requirements: HashMap::new(),
            server_groups: HashMap::new(),
        }
    }
    /// A server entry: a shapeless recipe of one gunpowder for `item`.
    fn packet_entry(display: i32, item: i32, category: i32) -> RecipeBookEntry {
        RecipeBookEntry {
            id: display,
            display: RecipeDisplay::Shapeless {
                ingredients: vec![SlotDisplay::Item(4)],
                result: SlotDisplay::Item(item),
                station: SlotDisplay::Item(0),
            },
            group: 0,
            category,
            requirements: Some(vec![HolderSet::Items(vec![4])]),
            flags: 0,
        }
    }
    /// A server numbering by its own recipes: a plugin recipe at 5, then the
    /// vanilla ones far from where this client numbers them. Its category
    /// ids: 1 redstone, 3 misc.
    fn paper_packet() -> RecipeBookAdd {
        RecipeBookAdd {
            entries: vec![
                packet_entry(5, 3, 3),
                packet_entry(110, 0, 1),
                packet_entry(111, 1, 1),
                packet_entry(112, 2, 3),
            ],
            replace: true,
        }
    }
    /// A server whose TNT has the number this client gives the stick.
    fn colliding_packet() -> RecipeBookAdd {
        RecipeBookAdd {
            entries: vec![packet_entry(12, 0, 1)],
            replace: true,
        }
    }
    /// What a tab lists once the client has `packet`.
    fn tab_after(packet: &RecipeBookAdd, category: Category) -> Vec<String> {
        let mut book = local_book();
        let tags = |_: &str| Vec::new();
        book.use_server_entries(
            &packet.entries,
            &PacketNames {
                item: &item_name,
                category: &category_name,
                tag: &tags,
            },
        );
        let mut client = ClientBook::default();
        for e in &packet.entries {
            client.add(e.id);
        }
        let mut names: Vec<String> = client
            .collections(&book, BookType::Crafting, Tab::Category(category))
            .iter()
            .flat_map(|c| {
                c.entries
                    .iter()
                    .map(|&i| book.entries[i].result.item.to_string())
            })
            .collect();
        names.sort();
        names
    }
    #[test]
    fn the_book_lists_the_recipes_a_servers_packet_names() {
        let packet = paper_packet();
        assert_eq!(
            tab_after(&packet, Category::CraftingRedstone),
            ["minecraft:piston", "minecraft:tnt"]
        );
        assert_eq!(
            tab_after(&packet, Category::CraftingMisc),
            ["minecraft:diamond", "minecraft:stick"]
        );
    }
    #[test]
    fn a_number_this_client_gives_another_recipe_shows_the_servers() {
        let packet = colliding_packet();
        assert_eq!(
            tab_after(&packet, Category::CraftingRedstone),
            ["minecraft:tnt"]
        );
        assert!(tab_after(&packet, Category::CraftingMisc).is_empty());
    }

    fn names<'a>(tags: &'a dyn Fn(&str) -> Vec<String>) -> PacketNames<'a> {
        PacketNames {
            item: &item_name,
            category: &category_name,
            tag: tags,
        }
    }
    #[test]
    fn a_server_of_this_clients_recipes_keeps_the_books_own_entries() {
        // The same number, result, category and ingredient count: the entry
        // the client made stands, so its order and ghost stay as they were.
        let mut book = local_book();
        let before: Vec<String> = book.entries.iter().map(|e| e.id.clone()).collect();
        let tags = |_: &str| Vec::new();
        let packet = RecipeBookAdd {
            entries: vec![packet_entry(10, 0, 1), packet_entry(11, 1, 1)],
            replace: true,
        };
        book.use_server_entries(&packet.entries, &names(&tags));
        let after: Vec<String> = book.entries.iter().map(|e| e.id.clone()).collect();
        assert_eq!(after, before);
        assert!(book.server_requirements.is_empty());
    }
    #[test]
    fn an_entry_this_book_cannot_show_hides_the_recipe_that_has_its_number() {
        let mut book = local_book();
        let tags = |_: &str| Vec::new();
        let mut stonecutter = packet_entry(11, 1, 1);
        stonecutter.display = RecipeDisplay::Stonecutter {
            input: SlotDisplay::Item(4),
            result: SlotDisplay::Item(1),
            station: SlotDisplay::Item(0),
        };
        book.use_server_entries(&[stonecutter], &names(&tags));
        // Number 11 was the piston here: not on this server.
        assert_eq!(book.entry_of_display(11), None);
        assert_eq!(book.entry_of_display(10), Some(0));
    }
    #[test]
    fn the_craftable_filter_asks_the_servers_ingredients() {
        use crate::menu::place::Stacked;
        let mut book = local_book();
        let tags = |tag: &str| {
            if tag == "minecraft:planks" {
                vec!["minecraft:stick".to_owned()]
            } else {
                Vec::new()
            }
        };
        let mut asks_planks = packet_entry(110, 0, 1);
        asks_planks.requirements = Some(vec![HolderSet::Tag("minecraft:planks".into())]);
        let mut asks_nothing_known = packet_entry(111, 1, 1);
        asks_nothing_known.requirements = None;
        let mut asks_none = packet_entry(112, 2, 3);
        asks_none.requirements = Some(Vec::new());
        book.use_server_entries(&[asks_planks, asks_nothing_known, asks_none], &names(&tags));
        let stock = Stacked::default();
        let craft = |display| {
            let i = book.entry_of_display(display).unwrap();
            book.can_craft(&book.entries[i], &stock)
        };
        // Nothing in hand: the tag's item is missing, no list is never
        // craftable (`RecipeDisplayEntry.canCraft`), an empty list always is.
        assert!(!craft(110));
        assert!(!craft(111));
        assert!(craft(112));
    }
    #[test]
    fn a_servers_tag_ingredient_shows_the_items_the_client_knows_for_it() {
        let mut book = local_book();
        let tags = |tag: &str| {
            assert_eq!(tag, "minecraft:planks");
            vec!["minecraft:stick".to_owned(), "minecraft:diamond".to_owned()]
        };
        let mut e = packet_entry(110, 0, 1);
        e.display = RecipeDisplay::Shapeless {
            ingredients: vec![SlotDisplay::Tag(HolderSet::Tag("minecraft:planks".into()))],
            result: SlotDisplay::Item(0),
            station: SlotDisplay::Item(0),
        };
        book.use_server_entries(&[e], &names(&tags));
        let entry = &book.entries[book.entry_of_display(110).unwrap()];
        let items: Vec<&str> = entry.requirements()[0]
            .order
            .iter()
            .map(String::as_str)
            .collect();
        assert_eq!(items, ["minecraft:stick", "minecraft:diamond"]);
    }

    #[test]
    fn an_entry_that_asks_for_other_ingredients_is_not_the_books_recipe() {
        // The server's TNT takes diamonds where this client's takes gunpowder
        // (the stick of planks and the stick of bamboo, the iron ingot of any
        // ore): same result, same category, same number of ingredients.
        let mut book = local_book();
        let tags = |_: &str| Vec::new();
        let mut tnt = packet_entry(10, 0, 1);
        tnt.display = RecipeDisplay::Shapeless {
            ingredients: vec![SlotDisplay::Item(3)],
            result: SlotDisplay::Item(0),
            station: SlotDisplay::Item(0),
        };
        book.use_server_entries(&[tnt], &names(&tags));
        let entry = &book.entries[book.entry_of_display(10).unwrap()];
        assert_eq!(entry.id, "server:10");
        assert_eq!(entry.requirements()[0].order, ["minecraft:diamond"]);
    }
    #[test]
    fn the_entries_taken_join_the_group_of_the_ones_kept() {
        let mut book = local_book();
        book.entries[2].group = Some("planks".into());
        let tags = |_: &str| Vec::new();
        // The server's group 7: the stick this book has, and a recipe it lacks.
        let mut kept = packet_entry(12, 2, 3);
        kept.group = 7;
        let mut taken = packet_entry(50, 3, 3);
        taken.group = 7;
        // The taken one comes first in the packet.
        let packet = [taken, kept];
        book.use_server_entries(&packet, &names(&tags));
        let mut client = ClientBook::default();
        for e in &packet {
            client.add(e.id);
        }
        let misc = client.collections(
            &book,
            BookType::Crafting,
            Tab::Category(Category::CraftingMisc),
        );
        assert_eq!(misc.len(), 1);
        assert_eq!(misc[0].entries.len(), 2);
    }
    #[test]
    fn an_entry_of_a_category_this_client_lacks_changes_nothing() {
        // A registry the client does not have (or a server newer than it):
        // every entry is of an unknown category, and the book stays its own.
        let mut book = local_book();
        let no_category = |_: i32| None;
        let tags = |_: &str| Vec::new();
        let packet = [packet_entry(10, 0, 1), packet_entry(11, 1, 1)];
        book.use_server_entries(
            &packet,
            &PacketNames {
                item: &item_name,
                category: &no_category,
                tag: &tags,
            },
        );
        assert_eq!(book.entries.len(), 3);
        assert!(book.entries.iter().all(|e| !e.id.starts_with("server:")));
        assert_eq!(book.entry_of_display(11), Some(1));
        // A category id the registry names but a tab does not exist for is
        // another matter: that recipe is not this book's.
        let mut stonecutter = packet_entry(11, 1, 4);
        stonecutter.display = packet[1].display.clone();
        book.use_server_entries(&[stonecutter], &names(&tags));
        assert_eq!(book.entry_of_display(11), None);
    }

    /// A shaped server entry of `ingredients` (item ids, -1 an empty cell).
    fn shaped_entry(display: i32, width: i32, height: i32, cells: &[i32]) -> RecipeBookEntry {
        let mut e = packet_entry(display, 0, 1);
        e.display = RecipeDisplay::Shaped {
            width,
            height,
            ingredients: cells
                .iter()
                .map(|&c| {
                    if c < 0 {
                        SlotDisplay::Empty
                    } else {
                        SlotDisplay::Item(c)
                    }
                })
                .collect(),
            result: SlotDisplay::Item(0),
            station: SlotDisplay::Item(0),
        };
        e
    }
    fn shaped_local(width: usize, height: usize, cells: &[Option<&str>]) -> Book {
        let mut book = local_book();
        book.entries[0].display = Display::Shaped {
            width,
            height,
            cells: cells.iter().map(|c| c.map(|i| ing(&[i]))).collect(),
        };
        book
    }
    #[test]
    fn a_shaped_recipe_must_have_the_same_layout_to_be_the_books_own() {
        let tags = |_: &str| Vec::new();
        let gp = Some("minecraft:gunpowder");
        let sand = Some("minecraft:diamond");
        // This client's TNT: 3 wide, 2 high.
        let cells = [gp, sand, gp, sand, gp, sand];
        let kept = |packet: RecipeBookEntry| {
            let mut book = shaped_local(3, 2, &cells);
            book.use_server_entries(&[packet], &names(&tags));
            book.entries[0].id == "minecraft:tnt"
        };
        // The same layout: the book's entry stands.
        assert!(kept(shaped_entry(10, 3, 2, &[4, 3, 4, 3, 4, 3])));
        // 2 wide and 3 high, the same ingredients in the same order.
        assert!(!kept(shaped_entry(10, 2, 3, &[4, 3, 4, 3, 4, 3])));
        // The same size and ingredients with a cell moved (an empty one).
        assert!(!kept(shaped_entry(10, 3, 2, &[4, 3, 4, 3, -1, 4])));
        // The empty cell counts: a hole where this client has an ingredient.
        let mut holed = shaped_local(2, 1, &[gp, None]);
        holed.use_server_entries(&[shaped_entry(10, 2, 1, &[4, 3])], &names(&tags));
        assert_eq!(holed.entries[0].id, "server:10");
    }
    /// What the book lists in every tab, as a comparable whole.
    fn listing(book: &Book, client: &ClientBook) -> Vec<String> {
        let mut out = Vec::new();
        for category in [
            Category::CraftingBuilding,
            Category::CraftingRedstone,
            Category::CraftingEquipment,
            Category::CraftingMisc,
        ] {
            for c in client.collections(book, BookType::Crafting, Tab::Category(category)) {
                let mut one: Vec<String> = c
                    .entries
                    .iter()
                    .map(|&i| {
                        let e = &book.entries[i];
                        format!("{}:{}:{:?}", e.display_id, e.result.item, e.group)
                    })
                    .collect();
                one.sort();
                out.push(format!("{category:?} {one:?}"));
            }
        }
        out.sort();
        out
    }
    #[test]
    fn packets_one_after_another_end_as_one_rebuild_would() {
        let tags = |_: &str| Vec::new();
        let group = |mut e: RecipeBookEntry, g: i32| {
            e.group = g;
            e
        };
        // The server's group 7 holds the book's own stick (kept, "planks"),
        // and two recipes the book lacks, one in each packet, the kept one last.
        let first = vec![group(packet_entry(50, 3, 3), 7), packet_entry(110, 0, 1)];
        let second = vec![
            group(packet_entry(51, 4, 3), 7),
            group(packet_entry(12, 2, 3), 7),
        ];
        let make = || {
            let mut book = local_book();
            book.entries[2].group = Some("planks".into());
            book
        };
        let mut live = make();
        live.use_server_entries(&first, &names(&tags));
        live.use_server_entries(&second, &names(&tags));
        let mut rebuilt = make();
        let mut all: Vec<&RecipeBookEntry> = first.iter().chain(&second).collect();
        all.sort_by_key(|e| e.id);
        rebuilt.use_server_entries(all, &names(&tags));
        let mut client = ClientBook::default();
        for e in first.iter().chain(&second) {
            client.add(e.id);
        }
        assert_eq!(listing(&live, &client), listing(&rebuilt, &client));
        // And the misc tab holds the one group: three entries in one button.
        let misc = client.collections(
            &live,
            BookType::Crafting,
            Tab::Category(Category::CraftingMisc),
        );
        assert_eq!(misc.len(), 1);
        assert_eq!(misc[0].entries.len(), 3);
    }
    #[test]
    fn a_kept_entry_leaves_no_server_requirements_behind() {
        let mut book = local_book();
        let tags = |_: &str| Vec::new();
        // Taken once, with its own ingredients.
        let mut taken = packet_entry(10, 0, 1);
        taken.requirements = Some(vec![HolderSet::Items(vec![3])]);
        taken.display = RecipeDisplay::Shapeless {
            ingredients: vec![SlotDisplay::Item(3)],
            result: SlotDisplay::Item(0),
            station: SlotDisplay::Item(0),
        };
        book.use_server_entries(&[taken], &names(&tags));
        assert!(book.server_requirements.contains_key(&10));
        // Hidden by an entry this book cannot show: nothing left of it.
        let mut stonecutter = packet_entry(10, 0, 4);
        stonecutter.display = RecipeDisplay::Stonecutter {
            input: SlotDisplay::Item(4),
            result: SlotDisplay::Item(0),
            station: SlotDisplay::Item(0),
        };
        book.use_server_entries(&[stonecutter], &names(&tags));
        assert!(!book.server_requirements.contains_key(&10));
    }
}
