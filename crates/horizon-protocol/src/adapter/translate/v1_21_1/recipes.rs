//! RecipeHolder/Ingredient packets before 1.21.2 introduced recipe displays.
//! The server's recipe keys remain authoritative; display ids are connection-local aliases.
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::{
    Error, Result,
    adapter::{
        ServerVersion,
        ids::{self, Way},
        pinned_ids::item,
        session::Session,
    },
    buf::{Codec, Reader, Writer},
    ids::play::{clientbound as cb, serverbound as sb},
    item::{Slot, Stack},
    packets::recipes::{
        HolderSet, PlaceGhostRecipe, PlaceRecipe, RecipeBookAdd, RecipeBookEntry, RecipeBookRemove,
        RecipeBookSeenRecipe, RecipeBookSettings, RecipeDisplay, SlotDisplay, TrimPattern,
    },
};

const MAX_RECIPES: usize = 65_536;

#[derive(Clone)]
struct Recipe {
    id: i32,
    display: Option<RecipeDisplay>,
    group: i32,
    category: i32,
    requirements: Vec<HolderSet>,
    notify: bool,
    properties: Vec<(&'static str, Vec<i32>)>,
    stonecutting: Option<(HolderSet, SlotDisplay)>,
}

#[derive(Clone, Default)]
pub(crate) struct Recipes {
    recipes: HashMap<String, Recipe>,
    keys: Vec<String>,
    display_ids: HashMap<String, i32>,
    groups: HashMap<(i32, String), i32>,
    unlocked: HashSet<String>,
    highlighted: HashSet<String>,
    received_book: bool,
}

#[derive(Clone)]
struct Ingredient(Vec<Stack>);
impl Ingredient {
    fn read(r: &mut Reader) -> Result<Self> {
        let n = r.count(MAX_RECIPES)?;
        let mut stacks = Vec::with_capacity(n.min(256));
        for _ in 0..n {
            if let Some(stack) = Slot::read(r)? {
                stacks.push(stack);
            }
        }
        Ok(Self(stacks))
    }
    fn items(&self) -> Vec<i32> {
        self.0
            .iter()
            .map(|s| s.item)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    fn requirement(&self) -> HolderSet {
        HolderSet::Items(self.items())
    }
    fn display(&self) -> SlotDisplay {
        let mut list: Vec<_> = self.0.iter().cloned().map(SlotDisplay::ItemStack).collect();
        match list.len() {
            0 => SlotDisplay::Empty,
            1 => list.remove(0),
            _ => SlotDisplay::Composite(list),
        }
    }
}

fn result(r: &mut Reader) -> Result<SlotDisplay> {
    Slot::read(r)?
        .map(SlotDisplay::ItemStack)
        .ok_or_else(|| Error::new("empty legacy recipe result"))
}

impl Recipes {
    fn id(&mut self, key: &str) -> Result<i32> {
        if let Some(id) = self.display_ids.get(key) {
            return Ok(*id);
        }
        if self.keys.len() >= MAX_RECIPES {
            return Err(Error::new("too many legacy recipe display ids"));
        }
        let id = self.keys.len() as i32;
        self.keys.push(key.to_owned());
        self.display_ids.insert(key.to_owned(), id);
        Ok(id)
    }
    fn group(&mut self, category: i32, group: String) -> Result<i32> {
        if group.is_empty() {
            return Ok(0);
        }
        let next = i32::try_from(self.groups.len() + 1)
            .map_err(|_| Error::new("too many recipe groups"))?;
        Ok(*self.groups.entry((category, group)).or_insert(next))
    }
    fn parse(
        &mut self,
        r: &mut Reader,
        serializer: i32,
        id: i32,
        trims: &HashMap<i32, i32>,
        width_first: bool,
    ) -> Result<Recipe> {
        // RecipeSerializer's 1.21/1.21.1 registration order from the vanilla report.
        let mut recipe = Recipe {
            id,
            display: None,
            group: 0,
            category: 3,
            requirements: Vec::new(),
            notify: true,
            properties: Vec::new(),
            stonecutting: None,
        };
        let mut ingredients = Vec::new();
        let mut group = String::new();
        match serializer {
            0 | 1 => {
                let dimensions = if serializer == 0 && width_first {
                    Some((r.varint()?, r.varint()?))
                } else {
                    None
                };
                group = r.string(32767)?;
                recipe.category = r.varint()?;
                if !(0..=3).contains(&recipe.category) {
                    return Err(Error::new("invalid legacy crafting category"));
                }
                let (width, height, count) = if serializer == 0 {
                    let (width, height) = match dimensions {
                        Some(size) => size,
                        None => (r.varint()?, r.varint()?),
                    };
                    if !(1..=3).contains(&width) || !(1..=3).contains(&height) {
                        return Err(Error::new("invalid legacy shaped recipe dimensions"));
                    }
                    (width, height, (width * height) as usize)
                } else {
                    (0, 0, r.count(9)?)
                };
                for _ in 0..count {
                    ingredients.push(Ingredient::read(r)?);
                }
                let result = result(r)?;
                let station = SlotDisplay::Item(item::CRAFTING_TABLE);
                recipe.display = Some(if serializer == 0 {
                    recipe.notify = r.bool()?;
                    RecipeDisplay::Shaped {
                        width,
                        height,
                        ingredients: ingredients.iter().map(Ingredient::display).collect(),
                        result,
                        station,
                    }
                } else {
                    RecipeDisplay::Shapeless {
                        ingredients: ingredients.iter().map(Ingredient::display).collect(),
                        result,
                        station,
                    }
                });
            }
            2..=14 | 22 => {
                // Vanilla's special crafting recipes have no fixed result/display in 1.21.2 either.
                recipe.category = r.varint()?;
                if !(0..=3).contains(&recipe.category) {
                    return Err(Error::new("invalid special recipe category"));
                }
                recipe.notify = false;
            }
            15..=18 => {
                group = r.string(32767)?;
                let category = r.varint()?;
                if !(0..=2).contains(&category) {
                    return Err(Error::new("invalid legacy cooking category"));
                }
                let ingredient = Ingredient::read(r)?;
                let result = result(r)?;
                let experience = r.f32()?;
                let duration = r.varint()?;
                let (station, property, book) = match serializer {
                    15 => (item::FURNACE, "furnace_input", 4 + category),
                    16 => (
                        item::BLAST_FURNACE,
                        "blast_furnace_input",
                        if category == 1 { 7 } else { 8 },
                    ),
                    17 => (item::SMOKER, "smoker_input", 9),
                    _ => (item::CAMPFIRE, "campfire_input", 12),
                };
                recipe.category = book;
                recipe.properties.push((property, ingredient.items()));
                recipe.display = Some(RecipeDisplay::Furnace {
                    ingredient: ingredient.display(),
                    fuel: SlotDisplay::AnyFuel,
                    result,
                    station: SlotDisplay::Item(station),
                    duration,
                    experience,
                });
                ingredients.push(ingredient);
            }
            19 => {
                group = r.string(32767)?;
                let ingredient = Ingredient::read(r)?;
                let result = result(r)?;
                recipe.category = 10;
                recipe.stonecutting = Some((ingredient.requirement(), result.clone()));
                recipe.display = Some(RecipeDisplay::Stonecutter {
                    input: ingredient.display(),
                    result,
                    station: SlotDisplay::Item(item::STONECUTTER),
                });
                ingredients.push(ingredient);
            }
            20 | 21 => {
                let template = Ingredient::read(r)?;
                let base = Ingredient::read(r)?;
                let addition = Ingredient::read(r)?;
                let result = if serializer == 20 {
                    result(r)?
                } else {
                    let mut patterns = BTreeSet::new();
                    for item in template.items() {
                        if let Some(pattern) = trims.get(&item) {
                            patterns.insert(*pattern);
                        }
                    }
                    // A template outside the synced/known set has no fabricated trim result.
                    let (base_display, addition_display) = (base.display(), addition.display());
                    // Every pattern copies both ingredients' displays: an old server's ingredient
                    // of 65 536 stacks made each 4-byte stack about 18 copies of 80-byte nodes.
                    let each = base_display
                        .weight()
                        .saturating_add(addition_display.weight());
                    if patterns.len().saturating_mul(each)
                        > crate::packets::recipes::MAX_TRIM_PREVIEW
                    {
                        return Err(Error::new("legacy trim preview too large"));
                    }
                    SlotDisplay::Composite(
                        patterns
                            .into_iter()
                            .map(|pattern| SlotDisplay::SmithingTrim {
                                base: Box::new(base_display.clone()),
                                material: Box::new(addition_display.clone()),
                                pattern: TrimPattern::Id(pattern),
                            })
                            .collect(),
                    )
                };
                recipe.category = 11;
                for (property, ingredient) in [
                    ("smithing_template", &template),
                    ("smithing_base", &base),
                    ("smithing_addition", &addition),
                ] {
                    recipe.properties.push((property, ingredient.items()));
                }
                recipe.display = Some(RecipeDisplay::Smithing {
                    template: template.display(),
                    base: base.display(),
                    addition: addition.display(),
                    result,
                    station: SlotDisplay::Item(item::SMITHING_TABLE),
                });
                ingredients.extend([template, base, addition]);
            }
            _ => {
                return Err(Error::new(format!(
                    "unknown legacy recipe serializer {serializer}"
                )));
            }
        }
        recipe.requirements = ingredients
            .iter()
            .filter(|i| !i.0.is_empty())
            .map(Ingredient::requirement)
            .collect();
        recipe.group = self.group(recipe.category, group)?;
        Ok(recipe)
    }
    fn entry(&self, key: &str, flags: i8) -> Option<RecipeBookEntry> {
        let r = self.recipes.get(key)?;
        Some(RecipeBookEntry {
            id: r.id,
            display: r.display.clone()?,
            group: r.group,
            category: r.category,
            requirements: Some(r.requirements.clone()),
            flags,
        })
    }
    fn book(&self, keys: &[String], replace: bool, added: bool) -> RecipeBookAdd {
        RecipeBookAdd {
            entries: keys
                .iter()
                .filter_map(|key| {
                    let recipe = self.recipes.get(key)?;
                    let flags = i8::from(added && recipe.notify)
                        | if self.highlighted.contains(key) { 2 } else { 0 };
                    self.entry(key, flags)
                })
                .collect(),
            replace,
        }
    }
    fn current_book(&self) -> RecipeBookAdd {
        let mut keys: Vec<_> = self.unlocked.iter().cloned().collect();
        keys.sort_unstable();
        self.book(&keys, true, false)
    }
    fn key(&self, id: i32) -> Option<&str> {
        let key = self.keys.get(usize::try_from(id).ok()?)?;
        self.recipes.get(key)?.display.as_ref()?;
        Some(key)
    }
    fn properties(&self) -> Vec<u8> {
        const NAMES: &[&str] = &[
            "smithing_addition",
            "smithing_base",
            "smithing_template",
            "furnace_input",
            "blast_furnace_input",
            "smoker_input",
            "campfire_input",
            "brewing_input",
            "brewing_reagent",
        ];
        let mut sets: BTreeMap<&str, BTreeSet<i32>> =
            NAMES.iter().map(|name| (*name, BTreeSet::new())).collect();
        let mut keys: Vec<_> = self.recipes.keys().collect();
        keys.sort_unstable();
        for recipe in self.recipes.values() {
            for (name, items) in &recipe.properties {
                sets.entry(name).or_default().extend(items);
            }
        }
        let mut w = Writer::new();
        w.varint(sets.len() as i32);
        for (name, items) in sets {
            w.string(&format!("minecraft:{name}"));
            w.varint(items.len() as i32);
            for item in items {
                w.varint(item);
            }
        }
        let stonecutting: Vec<_> = keys
            .into_iter()
            .filter_map(|key| self.recipes[key].stonecutting.as_ref())
            .collect();
        w.varint(stonecutting.len() as i32);
        for (ingredient, result) in stonecutting {
            ingredient.write(&mut w);
            result.write(&mut w);
        }
        w.data
    }
}

fn encode<T: Codec<T>>(value: &T) -> Vec<u8> {
    let mut w = Writer::new();
    T::write(value, &mut w);
    w.data
}
fn finish(r: &Reader) -> Result<()> {
    if r.remaining() == 0 {
        Ok(())
    } else {
        Err(Error::new("trailing legacy recipe data"))
    }
}

/// Returns canonical pinned bytes; callers must not decode these again with older registry ids.
pub(crate) fn clientbound(session: &Session, id: i32, payload: &[u8]) -> Result<Option<Vec<u8>>> {
    let mut r = Reader::new(payload);
    if id == cb::UPDATE_RECIPES {
        let trims = session.trim_patterns.lock().unwrap().clone();
        let mut state = session.legacy_recipes.lock().unwrap().clone();
        state.recipes.clear();
        {
            let _up = ids::renumbering(session.version.ids().unwrap(), Way::Up);
            let n = r.count(MAX_RECIPES)?;
            for _ in 0..n {
                let legacy_serializer = if ids::quirks().legacy_item_nbt {
                    let name = r.string(32767)?;
                    let names = if session.version.at_most(ServerVersion::V1_20_2) {
                        crate::adapter::v1_20_2_ids::RECIPE_SERIALIZER_NAMES
                    } else {
                        crate::adapter::v1_20_3_ids::RECIPE_SERIALIZER_NAMES
                    };
                    Some(
                        names
                            .iter()
                            .position(|candidate| *candidate == name)
                            .ok_or_else(|| Error::new("unknown legacy recipe serializer"))?
                            as i32,
                    )
                } else {
                    None
                };
                let key = r.string(32767)?;
                let serializer = match legacy_serializer {
                    Some(id) => id,
                    None => r.varint()?,
                };
                let display = state.id(&key)?;
                let recipe = state.parse(
                    &mut r,
                    serializer,
                    display,
                    &trims,
                    session.version.at_most(ServerVersion::V1_20_2),
                )?;
                if state.recipes.insert(key, recipe).is_some() {
                    return Err(Error::new("duplicate legacy recipe key"));
                }
            }
            finish(&r)?;
        }
        let result = state.properties();
        if state.received_book {
            session
                .incoming
                .lock()
                .unwrap()
                .push_back((cb::RECIPE_BOOK_ADD, encode(&state.current_book())));
        }
        *session.legacy_recipes.lock().unwrap() = state;
        return Ok(Some(result));
    }
    if id == cb::PLACE_GHOST_RECIPE {
        let container = i32::from(r.u8()?);
        let key = r.string(32767)?;
        finish(&r)?;
        let state = session.legacy_recipes.lock().unwrap();
        let display = state
            .recipes
            .get(&key)
            .and_then(|r| r.display.clone())
            .ok_or_else(|| Error::new("legacy ghost recipe has no known display"))?;
        return Ok(Some(encode(&PlaceGhostRecipe { container, display })));
    }
    Ok(None)
}

/// The old combined recipe packet becomes settings followed by a canonical add/remove packet.
pub(crate) fn recipe_book(session: &Session, payload: &[u8]) -> Result<(i32, Vec<u8>)> {
    let mut r = Reader::new(payload);
    let action = r.varint()?;
    if !(0..=2).contains(&action) {
        return Err(Error::new("invalid legacy recipe-book action"));
    }
    let settings = RecipeBookSettings::read(&mut r)?;
    let n = r.count(MAX_RECIPES)?;
    let mut keys = Vec::with_capacity(n.min(4096));
    for _ in 0..n {
        keys.push(r.string(32767)?);
    }
    let mut highlights = HashSet::new();
    if action == 0 {
        let n = r.count(MAX_RECIPES)?;
        for _ in 0..n {
            highlights.insert(r.string(32767)?);
        }
    }
    finish(&r)?;
    let mut state = session.legacy_recipes.lock().unwrap();
    if action == 1
        && state
            .unlocked
            .iter()
            .cloned()
            .chain(keys.iter().cloned())
            .collect::<HashSet<_>>()
            .len()
            > MAX_RECIPES
    {
        return Err(Error::new("too many unlocked legacy recipes"));
    }
    state.received_book = true;
    let packet = if action == 2 {
        let ids = keys
            .iter()
            .filter_map(|key| state.recipes.get(key))
            .filter(|r| r.display.is_some())
            .map(|r| r.id)
            .collect();
        for key in &keys {
            state.unlocked.remove(key);
            state.highlighted.remove(key);
        }
        (cb::RECIPE_BOOK_REMOVE, encode(&RecipeBookRemove { ids }))
    } else {
        if action == 0 {
            state.unlocked.clear();
            state.highlighted = highlights;
        } else {
            state.highlighted.extend(keys.iter().cloned());
        }
        state.unlocked.extend(keys.iter().cloned());
        (
            cb::RECIPE_BOOK_ADD,
            encode(&state.book(&keys, action == 0, action == 1)),
        )
    };
    session.incoming.lock().unwrap().push_back(packet);
    Ok((cb::RECIPE_BOOK_SETTINGS, encode(&settings)))
}

/// Call only for PLACE_RECIPE / RECIPE_BOOK_SEEN_RECIPE. `None` drops a stale display id.
pub(crate) fn serverbound(session: &Session, id: i32, payload: &[u8]) -> Result<Option<Vec<u8>>> {
    let mut r = Reader::new(payload);
    let (display, container, use_max) = match id {
        sb::PLACE_RECIPE => {
            let p = PlaceRecipe::read(&mut r)?;
            let container = i8::try_from(p.container)
                .map_err(|_| Error::new("legacy recipe container is out of range"))?;
            (p.recipe, Some(container), p.use_max)
        }
        sb::RECIPE_BOOK_SEEN_RECIPE => (RecipeBookSeenRecipe::read(&mut r)?.recipe, None, false),
        _ => return Ok(None),
    };
    finish(&r)?;
    let state = session.legacy_recipes.lock().unwrap();
    let Some(key) = state.key(display) else {
        return Ok(None);
    };
    let mut w = Writer::new();
    if let Some(container) = container {
        w.i8(container);
    }
    w.string(key);
    if container.is_some() {
        w.bool(use_max);
    }
    Ok(Some(w.data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::ServerVersion;

    // Independent 1.21.1 ClientboundUpdateRecipesPacket.STREAM_CODEC output. The fixture
    // contains one recipe for every one of the 23 vanilla RecipeSerializer registrations.
    const VANILLA_RECIPES: &str = "1713686f72697a6f6e5f776972653a736861706564000c686f72697a6f6e5f776972650302010101ab0600000101ab06000002e40200000016686f72697a6f6e5f776972653a73686170656c657373010c686f72697a6f6e5f776972650002010101000002011c0000012300000301000019686f72697a6f6e5f776972653a73746f6e6563757474696e67130c686f72697a6f6e5f77697265010101000002880200001f686f72697a6f6e5f776972653a736d697468696e675f7472616e73666f726d140101f50900000101e40600000101b006000001ec0600001a686f72697a6f6e5f776972653a736d697468696e675f7472696d150101f60900000101e40600000101af06000015686f72697a6f6e5f776972653a736d656c74696e670f0c686f72697a6f6e5f7769726501010140000001ab0600003f4000007815686f72697a6f6e5f776972653a626c617374696e67100c686f72697a6f6e5f7769726501010140000001ab0600003f4000007814686f72697a6f6e5f776972653a736d6f6b696e67110c686f72697a6f6e5f7769726501010140000001ab0600003f400000781d686f72697a6f6e5f776972653a63616d70666972655f636f6f6b696e67120c686f72697a6f6e5f7769726501010140000001ab0600003f4000007826686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f61726d6f72647965020329686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f626f6f6b636c6f6e696e67030328686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f6d6170636c6f6e696e6704032a686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f6d6170657874656e64696e6705032d686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f66697265776f726b5f726f636b657406032b686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f66697265776f726b5f73746172070330686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f66697265776f726b5f737461725f66616465080329686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f7469707065646172726f7709032d686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f62616e6e65726475706c69636174650a032e686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f736869656c646465636f726174696f6e0b0330686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f7368756c6b6572626f78636f6c6f72696e670c032c686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f737573706963696f7573737465770d0328686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f7265706169726974656d0e0323686f72697a6f6e5f776972653a6372616674696e675f6465636f72617465645f706f741603";

    fn bytes(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }
    fn setup() -> Session {
        let session = Session::new(ServerVersion::V1_21_1);
        session.trim_patterns.lock().unwrap().insert(1580, 7); // synced sentry pattern deliberately reordered
        clientbound(&session, cb::UPDATE_RECIPES, &bytes(VANILLA_RECIPES)).unwrap();
        session
    }
    fn old_book(action: i32, names: &[&str], highlights: &[&str]) -> Vec<u8> {
        let mut w = Writer::new();
        w.varint(action);
        RecipeBookSettings::write(
            &RecipeBookSettings {
                books: [(true, false), (false, true), (false, false), (true, true)],
            },
            &mut w,
        );
        w.varint(names.len() as i32);
        for name in names {
            w.string(&format!("horizon_wire:{name}"));
        }
        if action == 0 {
            w.varint(highlights.len() as i32);
            for name in highlights {
                w.string(&format!("horizon_wire:{name}"));
            }
        }
        w.data
    }
    fn next_book(session: &Session) -> RecipeBookAdd {
        let (id, payload) = session.next_incoming().unwrap();
        assert_eq!(id, cb::RECIPE_BOOK_ADD);
        RecipeBookAdd::read(&mut Reader::new(&payload)).unwrap()
    }

    #[test]
    fn a_trim_recipe_whose_previews_would_balloon_is_refused() {
        // One smithing_trim recipe (serializer 21) with the fixture's template and addition, and
        // a base ingredient of `stacks` netherite chestplates.
        let packet = |stacks: usize| {
            let mut hex =
                String::from("011a686f72697a6f6e5f776972653a736d697468696e675f7472696d15");
            hex += "0101f6090000";
            let mut w = Writer::new();
            w.varint(stacks as i32);
            hex += &w
                .data
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            hex += &"01e4060000".repeat(stacks);
            hex += "0101af060000";
            bytes(&hex)
        };
        let session = Session::new(ServerVersion::V1_21_1);
        session.trim_patterns.lock().unwrap().insert(1580, 7);
        assert!(clientbound(&session, cb::UPDATE_RECIPES, &packet(40)).is_ok());
        let err = clientbound(&session, cb::UPDATE_RECIPES, &packet(5000)).unwrap_err();
        assert!(err.to_string().contains("trim preview too large"), "{err}");
    }

    #[test]
    fn vanilla_recipe_serializers_become_pinned_displays_with_canonical_item_ids() {
        let session = setup();
        let state = session.legacy_recipes.lock().unwrap();
        assert_eq!(state.recipes.len(), 23);
        assert_eq!(
            state
                .recipes
                .values()
                .filter(|r| r.display.is_some())
                .count(),
            9
        );
        let shaped = &state.recipes["horizon_wire:shaped"];
        let Some(RecipeDisplay::Shaped {
            width,
            height,
            ingredients,
            result,
            station,
        }) = &shaped.display
        else {
            panic!("shaped")
        };
        assert_eq!((*width, *height), (2, 1));
        assert_eq!(ingredients.len(), 2);
        assert_eq!(*station, SlotDisplay::Item(405));
        let SlotDisplay::ItemStack(stack) = result else {
            panic!("result")
        };
        assert_eq!((stack.item, stack.count), (473, 2));
        assert_eq!(
            shaped.requirements,
            [HolderSet::Items(vec![1018]), HolderSet::Items(vec![1018])]
        );
        assert!(!shaped.notify);
        let Some(RecipeDisplay::Smithing { result, .. }) =
            &state.recipes["horizon_wire:smithing_trim"].display
        else {
            panic!("smithing")
        };
        let SlotDisplay::Composite(previews) = result else {
            panic!("trim previews")
        };
        let SlotDisplay::SmithingTrim { pattern, .. } = &previews[0] else {
            panic!("trim")
        };
        assert_eq!(*pattern, TrimPattern::Id(7));
        let Some(RecipeDisplay::Furnace {
            duration,
            experience,
            station,
            ..
        }) = &state.recipes["horizon_wire:blasting"].display
        else {
            panic!("blast")
        };
        assert_eq!(
            (*duration, *experience, station.clone()),
            (120, 0.75, SlotDisplay::Item(1508))
        );
        let properties = state.properties();
        let mut r = Reader::new(&properties);
        assert_eq!(r.varint().unwrap(), 9);
        let mut sets = HashMap::new();
        for _ in 0..9 {
            let name = r.string(32767).unwrap();
            let n = r.varint().unwrap();
            let list = (0..n).map(|_| r.varint().unwrap()).collect::<Vec<_>>();
            sets.insert(name, list);
        }
        assert_eq!(sets["minecraft:furnace_input"], [95]);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(HolderSet::read(&mut r).unwrap(), HolderSet::Items(vec![1]));
        let SlotDisplay::ItemStack(stack) = SlotDisplay::read(&mut r).unwrap() else {
            panic!("stonecutting")
        };
        assert_eq!((stack.item, stack.count), (355, 2));
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn init_add_remove_and_reload_preserve_keys_highlights_and_settings() {
        let session = setup();
        let names = [
            "shaped",
            "shapeless",
            "stonecutting",
            "smithing_transform",
            "smithing_trim",
            "smelting",
            "blasting",
            "smoking",
            "campfire_cooking",
            "crafting_special_armordye",
        ];
        let (id, payload) = recipe_book(
            &session,
            &old_book(0, &names, &["shaped", "crafting_special_armordye"]),
        )
        .unwrap();
        assert_eq!(id, cb::RECIPE_BOOK_SETTINGS);
        assert_eq!(
            RecipeBookSettings::read(&mut Reader::new(&payload))
                .unwrap()
                .books[0],
            (true, false)
        );
        let initial = next_book(&session);
        assert!(initial.replace);
        assert_eq!(initial.entries.len(), 9);
        assert_eq!(initial.entries[0].flags, 2);
        assert!(initial.entries[1..].iter().all(|r| r.flags == 0));
        let shaped_id = initial.entries[0].id;
        recipe_book(&session, &old_book(1, &["shaped", "shapeless"], &[])).unwrap();
        let added = next_book(&session);
        assert!(!added.replace);
        assert_eq!(added.entries[0].id, shaped_id);
        assert_eq!(added.entries[0].flags, 2);
        assert_eq!(added.entries[1].flags, 3);
        recipe_book(&session, &old_book(2, &["shaped"], &[])).unwrap();
        let (id, payload) = session.next_incoming().unwrap();
        assert_eq!(id, cb::RECIPE_BOOK_REMOVE);
        assert_eq!(
            RecipeBookRemove::read(&mut Reader::new(&payload))
                .unwrap()
                .ids,
            [shaped_id]
        );
        clientbound(&session, cb::UPDATE_RECIPES, &bytes(VANILLA_RECIPES)).unwrap();
        let reloaded = next_book(&session);
        assert!(reloaded.replace);
        assert_eq!(reloaded.entries.len(), 8);
        assert_eq!(
            session.legacy_recipes.lock().unwrap().recipes["horizon_wire:shaped"].id,
            shaped_id
        );
        let mut bad = Writer::new();
        bad.varint(1);
        bad.string("horizon_wire:bad");
        bad.varint(99);
        assert!(clientbound(&session, cb::UPDATE_RECIPES, &bad.data).is_err());
        assert_eq!(session.legacy_recipes.lock().unwrap().recipes.len(), 23);
    }

    #[test]
    fn outbound_selection_and_ghosts_follow_the_servers_recipe_key() {
        let session = setup();
        let id = session.legacy_recipes.lock().unwrap().recipes["horizon_wire:shaped"].id;
        let payload = encode(&PlaceRecipe {
            container: 11,
            recipe: id,
            use_max: true,
        });
        let old = serverbound(&session, sb::PLACE_RECIPE, &payload)
            .unwrap()
            .unwrap();
        let mut r = Reader::new(&old);
        assert_eq!(r.i8().unwrap(), 11);
        assert_eq!(r.string(32767).unwrap(), "horizon_wire:shaped");
        assert!(r.bool().unwrap());
        assert_eq!(r.remaining(), 0);
        let old = serverbound(
            &session,
            sb::RECIPE_BOOK_SEEN_RECIPE,
            &encode(&RecipeBookSeenRecipe { recipe: id }),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            Reader::new(&old).string(32767).unwrap(),
            "horizon_wire:shaped"
        );
        assert!(
            serverbound(
                &session,
                sb::RECIPE_BOOK_SEEN_RECIPE,
                &encode(&RecipeBookSeenRecipe { recipe: 65000 })
            )
            .unwrap()
            .is_none()
        );
        let mut ghost = Writer::new();
        ghost.i8(11);
        ghost.string("horizon_wire:shaped");
        let payload = clientbound(&session, cb::PLACE_GHOST_RECIPE, &ghost.data)
            .unwrap()
            .unwrap();
        let ghost = PlaceGhostRecipe::read(&mut Reader::new(&payload)).unwrap();
        assert_eq!(ghost.container, 11);
        assert!(matches!(
            ghost.display,
            RecipeDisplay::Shaped {
                width: 2,
                height: 1,
                ..
            }
        ));
        let mut empty = Writer::new();
        empty.varint(0);
        clientbound(&session, cb::UPDATE_RECIPES, &empty.data).unwrap();
        assert!(
            serverbound(
                &session,
                sb::RECIPE_BOOK_SEEN_RECIPE,
                &encode(&RecipeBookSeenRecipe { recipe: id })
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn a_book_before_declarations_is_filled_when_its_recipe_arrives() {
        let session = Session::new(ServerVersion::V1_21_1);
        recipe_book(&session, &old_book(0, &["shaped"], &["shaped"])).unwrap();
        assert!(next_book(&session).entries.is_empty());
        clientbound(&session, cb::UPDATE_RECIPES, &bytes(VANILLA_RECIPES)).unwrap();
        let book = next_book(&session);
        assert!(book.replace);
        assert_eq!(book.entries.len(), 1);
        assert_eq!(book.entries[0].flags, 2);
    }

    #[test]
    fn vanilla_1_20_5_and_1_20_6_recipe_codecs_reuse_displays_and_request_keys() {
        // Official 1.20.6 UpdateRecipes codec output, independently byte-identical on 1.20.5.
        // Includes all 23 serializers; registry ids differ from the 1.21.1 fixture above.
        let old = bytes(
            "1713686f72697a6f6e5f776972653a736861706564000c686f72697a6f6e5f776972650302010101aa0600000101aa06000002e40200000016686f72697a6f6e5f776972653a73686170656c657373010c686f72697a6f6e5f776972650002010101000002011c0000012300000301000019686f72697a6f6e5f776972653a73746f6e6563757474696e67130c686f72697a6f6e5f77697265010101000002880200001f686f72697a6f6e5f776972653a736d697468696e675f7472616e73666f726d140101f20900000101e40600000101af06000001ec0600001a686f72697a6f6e5f776972653a736d697468696e675f7472696d150101f30900000101e40600000101ae06000015686f72697a6f6e5f776972653a736d656c74696e670f0c686f72697a6f6e5f7769726501010140000001aa0600003f4000007815686f72697a6f6e5f776972653a626c617374696e67100c686f72697a6f6e5f7769726501010140000001aa0600003f4000007814686f72697a6f6e5f776972653a736d6f6b696e67110c686f72697a6f6e5f7769726501010140000001aa0600003f400000781d686f72697a6f6e5f776972653a63616d70666972655f636f6f6b696e67120c686f72697a6f6e5f7769726501010140000001aa0600003f4000007826686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f61726d6f72647965020329686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f626f6f6b636c6f6e696e67030328686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f6d6170636c6f6e696e6704032a686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f6d6170657874656e64696e6705032d686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f66697265776f726b5f726f636b657406032b686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f66697265776f726b5f73746172070330686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f66697265776f726b5f737461725f66616465080329686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f7469707065646172726f7709032d686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f62616e6e65726475706c69636174650a032e686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f736869656c646465636f726174696f6e0b0330686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f7368756c6b6572626f78636f6c6f72696e670c032c686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f737573706963696f7573737465770d0328686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f7265706169726974656d0e0323686f72697a6f6e5f776972653a6372616674696e675f6465636f72617465645f706f741603",
        );
        let session = Session::new(ServerVersion::V1_20_6);
        session.trim_patterns.lock().unwrap().insert(1580, 7);
        let properties = clientbound(&session, cb::UPDATE_RECIPES, &old)
            .unwrap()
            .unwrap();
        assert!(!properties.is_empty());
        {
            let state = session.legacy_recipes.lock().unwrap();
            assert_eq!(state.recipes.len(), 23);
            assert_eq!(
                state
                    .recipes
                    .values()
                    .filter(|r| r.display.is_some())
                    .count(),
                9
            );
            let Some(RecipeDisplay::Shaped { result, .. }) =
                &state.recipes["horizon_wire:shaped"].display
            else {
                panic!("shaped")
            };
            let SlotDisplay::ItemStack(stack) = result else {
                panic!("result")
            };
            assert_eq!((stack.item, stack.count), (473, 2));
            assert_eq!(
                state.recipes["horizon_wire:shaped"].requirements,
                [HolderSet::Items(vec![1018]), HolderSet::Items(vec![1018])]
            );
            let Some(RecipeDisplay::Smithing { result, .. }) =
                &state.recipes["horizon_wire:smithing_trim"].display
            else {
                panic!("smithing")
            };
            let SlotDisplay::Composite(previews) = result else {
                panic!("previews")
            };
            let SlotDisplay::SmithingTrim { pattern, .. } = &previews[0] else {
                panic!("trim")
            };
            assert_eq!(*pattern, TrimPattern::Id(7));
        }
        recipe_book(
            &session,
            &old_book(
                0,
                &["shaped", "smelting", "crafting_special_armordye"],
                &["shaped"],
            ),
        )
        .unwrap();
        let book = next_book(&session);
        assert!(book.replace);
        assert_eq!(book.entries.len(), 2);
        assert_eq!(book.entries[0].flags, 2);
        let id = book.entries[0].id;
        let selected = serverbound(
            &session,
            sb::PLACE_RECIPE,
            &encode(&PlaceRecipe {
                container: 3,
                recipe: id,
                use_max: true,
            }),
        )
        .unwrap()
        .unwrap();
        let mut r = Reader::new(&selected);
        assert_eq!(r.i8().unwrap(), 3);
        assert_eq!(r.string(32767).unwrap(), "horizon_wire:shaped");
        assert!(r.bool().unwrap());
        assert_eq!(r.remaining(), 0);
        let seen = serverbound(
            &session,
            sb::RECIPE_BOOK_SEEN_RECIPE,
            &encode(&RecipeBookSeenRecipe { recipe: id }),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            Reader::new(&seen).string(32767).unwrap(),
            "horizon_wire:shaped"
        );
    }
    #[test]
    fn vanilla_1_20_3_and_1_20_4_recipe_codecs_reuse_displays_and_request_keys() {
        // Official 1.20.4 UpdateRecipes codec output, independently byte-identical on 1.20.3.
        // All 23 serializer identifiers and NBT slots pass through the full recipe bridge.
        let old = bytes(
            "17196d696e6563726166743a6372616674696e675f73686170656413686f72697a6f6e5f776972653a7368617065640c686f72697a6f6e5f776972650302010101a70601000101a706010001e3020200001c6d696e6563726166743a6372616674696e675f73686170656c65737316686f72697a6f6e5f776972653a73686170656c6573730c686f72697a6f6e5f776972650002010101010002011c01000123010001010300166d696e6563726166743a73746f6e6563757474696e6719686f72697a6f6e5f776972653a73746f6e6563757474696e670c686f72697a6f6e5f77697265010101010001870202001c6d696e6563726166743a736d697468696e675f7472616e73666f726d1f686f72697a6f6e5f776972653a736d697468696e675f7472616e73666f726d0101e90901000101e106010a03000644616d61676500000000000101ac06010001e906010a03000644616d6167650000000000176d696e6563726166743a736d697468696e675f7472696d1a686f72697a6f6e5f776972653a736d697468696e675f7472696d0101ea0901000101e106010a03000644616d61676500000000000101ab060100126d696e6563726166743a736d656c74696e6715686f72697a6f6e5f776972653a736d656c74696e670c686f72697a6f6e5f7769726501010140010001a70601003f40000078126d696e6563726166743a626c617374696e6715686f72697a6f6e5f776972653a626c617374696e670c686f72697a6f6e5f7769726501010140010001a70601003f40000078116d696e6563726166743a736d6f6b696e6714686f72697a6f6e5f776972653a736d6f6b696e670c686f72697a6f6e5f7769726501010140010001a70601003f400000781a6d696e6563726166743a63616d70666972655f636f6f6b696e671d686f72697a6f6e5f776972653a63616d70666972655f636f6f6b696e670c686f72697a6f6e5f7769726501010140010001a70601003f40000078236d696e6563726166743a6372616674696e675f7370656369616c5f61726d6f7264796526686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f61726d6f7264796503266d696e6563726166743a6372616674696e675f7370656369616c5f626f6f6b636c6f6e696e6729686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f626f6f6b636c6f6e696e6703256d696e6563726166743a6372616674696e675f7370656369616c5f6d6170636c6f6e696e6728686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f6d6170636c6f6e696e6703276d696e6563726166743a6372616674696e675f7370656369616c5f6d6170657874656e64696e672a686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f6d6170657874656e64696e67032a6d696e6563726166743a6372616674696e675f7370656369616c5f66697265776f726b5f726f636b65742d686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f66697265776f726b5f726f636b657403286d696e6563726166743a6372616674696e675f7370656369616c5f66697265776f726b5f737461722b686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f66697265776f726b5f73746172032d6d696e6563726166743a6372616674696e675f7370656369616c5f66697265776f726b5f737461725f6661646530686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f66697265776f726b5f737461725f6661646503266d696e6563726166743a6372616674696e675f7370656369616c5f7469707065646172726f7729686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f7469707065646172726f77032a6d696e6563726166743a6372616674696e675f7370656369616c5f62616e6e65726475706c69636174652d686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f62616e6e65726475706c6963617465032b6d696e6563726166743a6372616674696e675f7370656369616c5f736869656c646465636f726174696f6e2e686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f736869656c646465636f726174696f6e032d6d696e6563726166743a6372616674696e675f7370656369616c5f7368756c6b6572626f78636f6c6f72696e6730686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f7368756c6b6572626f78636f6c6f72696e6703296d696e6563726166743a6372616674696e675f7370656369616c5f737573706963696f7573737465772c686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f737573706963696f75737374657703256d696e6563726166743a6372616674696e675f7370656369616c5f7265706169726974656d28686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f7265706169726974656d03206d696e6563726166743a6372616674696e675f6465636f72617465645f706f7423686f72697a6f6e5f776972653a6372616674696e675f6465636f72617465645f706f7403",
        );
        let session = Session::new(ServerVersion::V1_20_3);
        let _names = ids::legacy_registries(session.legacy_registries.lock().unwrap().clone());
        let _items = ids::legacy_items(session.legacy_items.clone());
        session.trim_patterns.lock().unwrap().insert(1580, 7);
        let properties = clientbound(&session, cb::UPDATE_RECIPES, &old)
            .unwrap()
            .unwrap();
        assert!(!properties.is_empty());
        {
            let state = session.legacy_recipes.lock().unwrap();
            assert_eq!(state.recipes.len(), 23);
            assert_eq!(
                state
                    .recipes
                    .values()
                    .filter(|r| r.display.is_some())
                    .count(),
                9
            );
            let Some(RecipeDisplay::Shaped { result, .. }) =
                &state.recipes["horizon_wire:shaped"].display
            else {
                panic!("shaped")
            };
            let SlotDisplay::ItemStack(stack) = result else {
                panic!("result")
            };
            assert_eq!((stack.item, stack.count), (473, 2));
            assert_eq!(
                state.recipes["horizon_wire:shaped"].requirements,
                [HolderSet::Items(vec![1018]), HolderSet::Items(vec![1018])]
            );
            let Some(RecipeDisplay::Smithing { result, .. }) =
                &state.recipes["horizon_wire:smithing_trim"].display
            else {
                panic!("smithing")
            };
            let SlotDisplay::Composite(previews) = result else {
                panic!("previews")
            };
            let SlotDisplay::SmithingTrim { pattern, .. } = &previews[0] else {
                panic!("trim")
            };
            assert_eq!(*pattern, TrimPattern::Id(7));
        }
        recipe_book(
            &session,
            &old_book(
                0,
                &["shaped", "smelting", "crafting_special_armordye"],
                &["shaped"],
            ),
        )
        .unwrap();
        let book = next_book(&session);
        assert!(book.replace);
        assert_eq!(book.entries.len(), 2);
        assert_eq!(book.entries[0].flags, 2);
        let id = book.entries[0].id;
        let selected = serverbound(
            &session,
            sb::PLACE_RECIPE,
            &encode(&PlaceRecipe {
                container: 3,
                recipe: id,
                use_max: true,
            }),
        )
        .unwrap()
        .unwrap();
        let mut r = Reader::new(&selected);
        assert_eq!(r.i8().unwrap(), 3);
        assert_eq!(r.string(32767).unwrap(), "horizon_wire:shaped");
        assert!(r.bool().unwrap());
        assert_eq!(r.remaining(), 0);
        let seen = serverbound(
            &session,
            sb::RECIPE_BOOK_SEEN_RECIPE,
            &encode(&RecipeBookSeenRecipe { recipe: id }),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            Reader::new(&seen).string(32767).unwrap(),
            "horizon_wire:shaped"
        );
    }

    #[test]
    fn vanilla_1_20_2_recipe_codecs_keep_width_first_and_reuse_request_keys() {
        // Independent official 1.20.2 UpdateRecipes output; shaped dimensions precede group/category.
        // All 23 serializer identifiers and NBT slots pass through the full recipe bridge.
        let old = bytes(
            "17196d696e6563726166743a6372616674696e675f73686170656413686f72697a6f6e5f776972653a73686170656402010c686f72697a6f6e5f776972650301018206010001018206010001ce020200001c6d696e6563726166743a6372616674696e675f73686170656c65737316686f72697a6f6e5f776972653a73686170656c6573730c686f72697a6f6e5f776972650002010101010002010f01000116010001010300166d696e6563726166743a73746f6e6563757474696e6719686f72697a6f6e5f776972653a73746f6e6563757474696e670c686f72697a6f6e5f77697265010101010001f20102001c6d696e6563726166743a736d697468696e675f7472616e73666f726d1f686f72697a6f6e5f776972653a736d697468696e675f7472616e73666f726d0101c20901000101bc06010a03000644616d616765000000000001018706010001c406010a03000644616d6167650000000000176d696e6563726166743a736d697468696e675f7472696d1a686f72697a6f6e5f776972653a736d697468696e675f7472696d0101c30901000101bc06010a03000644616d6167650000000000010186060100126d696e6563726166743a736d656c74696e6715686f72697a6f6e5f776972653a736d656c74696e670c686f72697a6f6e5f7769726501010133010001820601003f40000078126d696e6563726166743a626c617374696e6715686f72697a6f6e5f776972653a626c617374696e670c686f72697a6f6e5f7769726501010133010001820601003f40000078116d696e6563726166743a736d6f6b696e6714686f72697a6f6e5f776972653a736d6f6b696e670c686f72697a6f6e5f7769726501010133010001820601003f400000781a6d696e6563726166743a63616d70666972655f636f6f6b696e671d686f72697a6f6e5f776972653a63616d70666972655f636f6f6b696e670c686f72697a6f6e5f7769726501010133010001820601003f40000078236d696e6563726166743a6372616674696e675f7370656369616c5f61726d6f7264796526686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f61726d6f7264796503266d696e6563726166743a6372616674696e675f7370656369616c5f626f6f6b636c6f6e696e6729686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f626f6f6b636c6f6e696e6703256d696e6563726166743a6372616674696e675f7370656369616c5f6d6170636c6f6e696e6728686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f6d6170636c6f6e696e6703276d696e6563726166743a6372616674696e675f7370656369616c5f6d6170657874656e64696e672a686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f6d6170657874656e64696e67032a6d696e6563726166743a6372616674696e675f7370656369616c5f66697265776f726b5f726f636b65742d686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f66697265776f726b5f726f636b657403286d696e6563726166743a6372616674696e675f7370656369616c5f66697265776f726b5f737461722b686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f66697265776f726b5f73746172032d6d696e6563726166743a6372616674696e675f7370656369616c5f66697265776f726b5f737461725f6661646530686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f66697265776f726b5f737461725f6661646503266d696e6563726166743a6372616674696e675f7370656369616c5f7469707065646172726f7729686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f7469707065646172726f77032a6d696e6563726166743a6372616674696e675f7370656369616c5f62616e6e65726475706c69636174652d686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f62616e6e65726475706c6963617465032b6d696e6563726166743a6372616674696e675f7370656369616c5f736869656c646465636f726174696f6e2e686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f736869656c646465636f726174696f6e032d6d696e6563726166743a6372616674696e675f7370656369616c5f7368756c6b6572626f78636f6c6f72696e6730686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f7368756c6b6572626f78636f6c6f72696e6703296d696e6563726166743a6372616674696e675f7370656369616c5f737573706963696f7573737465772c686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f737573706963696f75737374657703256d696e6563726166743a6372616674696e675f7370656369616c5f7265706169726974656d28686f72697a6f6e5f776972653a6372616674696e675f7370656369616c5f7265706169726974656d03206d696e6563726166743a6372616674696e675f6465636f72617465645f706f7423686f72697a6f6e5f776972653a6372616674696e675f6465636f72617465645f706f7403",
        );
        let session = Session::new(ServerVersion::V1_20_2);
        let _names = ids::legacy_registries(session.legacy_registries.lock().unwrap().clone());
        let _items = ids::legacy_items(session.legacy_items.clone());
        session.trim_patterns.lock().unwrap().insert(1580, 7);
        let properties = clientbound(&session, cb::UPDATE_RECIPES, &old)
            .unwrap()
            .unwrap();
        assert!(!properties.is_empty());
        {
            let state = session.legacy_recipes.lock().unwrap();
            assert_eq!(state.recipes.len(), 23);
            assert_eq!(
                state
                    .recipes
                    .values()
                    .filter(|r| r.display.is_some())
                    .count(),
                9
            );
            let Some(RecipeDisplay::Shaped {
                width,
                height,
                result,
                ..
            }) = &state.recipes["horizon_wire:shaped"].display
            else {
                panic!("shaped")
            };
            assert_eq!((*width, *height), (2, 1));
            let SlotDisplay::ItemStack(stack) = result else {
                panic!("result")
            };
            assert_eq!((stack.item, stack.count), (473, 2));
            assert_eq!(
                state.recipes["horizon_wire:shaped"].requirements,
                [HolderSet::Items(vec![1018]), HolderSet::Items(vec![1018])]
            );
            let Some(RecipeDisplay::Smithing { result, .. }) =
                &state.recipes["horizon_wire:smithing_trim"].display
            else {
                panic!("smithing")
            };
            let SlotDisplay::Composite(previews) = result else {
                panic!("previews")
            };
            let SlotDisplay::SmithingTrim { pattern, .. } = &previews[0] else {
                panic!("trim")
            };
            assert_eq!(*pattern, TrimPattern::Id(7));
        }
        recipe_book(
            &session,
            &old_book(
                0,
                &["shaped", "smelting", "crafting_special_armordye"],
                &["shaped"],
            ),
        )
        .unwrap();
        let book = next_book(&session);
        assert!(book.replace);
        assert_eq!(book.entries.len(), 2);
        assert_eq!(book.entries[0].flags, 2);
        let id = book.entries[0].id;
        let selected = serverbound(
            &session,
            sb::PLACE_RECIPE,
            &encode(&PlaceRecipe {
                container: 3,
                recipe: id,
                use_max: true,
            }),
        )
        .unwrap()
        .unwrap();
        let mut r = Reader::new(&selected);
        assert_eq!(r.i8().unwrap(), 3);
        assert_eq!(r.string(32767).unwrap(), "horizon_wire:shaped");
        assert!(r.bool().unwrap());
        assert_eq!(r.remaining(), 0);
        let seen = serverbound(
            &session,
            sb::RECIPE_BOOK_SEEN_RECIPE,
            &encode(&RecipeBookSeenRecipe { recipe: id }),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            Reader::new(&seen).string(32767).unwrap(),
            "horizon_wire:shaped"
        );
    }
}
