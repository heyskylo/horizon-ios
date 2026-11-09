//! Item stack components beyond count and damage: enchantments, stored
//! (book) enchantments, `repair_cost`, `custom_name`, `potion_contents` and
//! `trim`, with their NBT form in vanilla's `components` compound.
use crate::items::ItemStack;
use horizon_world::nbt::{JavaString, Tag};
use std::sync::Arc;

/// Enchantment id and level, in insertion order.
pub type Levels = Vec<(Arc<str>, u32)>;
pub mod trim;
pub use trim::Trim;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StackData {
    /// `minecraft:enchantments`.
    pub enchantments: Levels,
    /// `minecraft:stored_enchantments` (enchanted books).
    pub stored_enchantments: Levels,
    /// `minecraft:repair_cost` (anvil prior-work penalty).
    pub repair_cost: u32,
    /// An explicit `minecraft:damage` of zero. The damage is outside
    /// `StackData`, and a non-damageable item's prototype has no such
    /// component, so its presence is part of the stack's components.
    pub explicit_zero_damage: bool,
    /// The original `minecraft:custom_name` component; plain text is derived
    /// when gameplay needs it, so styled names have one source of truth.
    pub custom_name: Option<Tag>,
    /// `minecraft:potion_contents.potion`.
    pub potion: Option<Arc<str>>,
    /// The rest of `minecraft:potion_contents` (`custom_color`, `custom_effects`,
    /// `custom_name`), kept as saved.
    pub potion_extra: Vec<(String, Tag)>,
    /// `minecraft:trim`: (material, pattern).
    pub trim: Option<Trim>,
    /// `minecraft:container` (shulker boxes): the non-empty slots.
    pub container: Vec<(u8, ItemStack)>,
    /// `minecraft:charged_projectiles` (a loaded crossbow).
    pub charged: Vec<ItemStack>,
    /// `minecraft:fireworks`: flight duration and explosions (None: the
    /// item's default).
    pub fireworks: Option<(u8, Vec<crate::firework::FireworkExplosion>)>,
    /// `minecraft:map_id` (filled maps).
    pub map_id: Option<i32>,
    /// `minecraft:writable_book_content` pages.
    pub writable_pages: Vec<String>,
    /// `minecraft:written_book_content`.
    pub written: Option<WrittenBook>,
    /// `minecraft:banner_patterns` (banners and shields).
    pub banner_patterns: Vec<crate::banner_layer::BannerLayer>,
    /// `minecraft:base_color` (a shield's dye).
    pub base_color: Option<Arc<str>>,
    /// The raid captain's banner: `minecraft:banner_patterns` with the
    /// ominous layers, its item name, rarity and hidden tooltip.
    pub ominous_banner: bool,
    /// `minecraft:ominous_bottle_amplifier` (None: the item's default, 0).
    pub ominous_amplifier: Option<u8>,
    /// `minecraft:dyed_color` (leather armour, wolf armour, ... as RGB).
    pub dyed_color: Option<i32>,
    /// `minecraft:firework_explosion` (a firework star).
    pub firework_explosion: Option<crate::firework::FireworkExplosion>,
    /// `minecraft:pot_decorations`: back, left, right, front sherds.
    pub pot_decorations: Option<Box<[Option<ItemStack>; 4]>>,
    /// `minecraft:bundle_contents`.
    pub bundle_contents: Vec<ItemStack>,
    /// `BundleContents.selectedItem`: runtime only, never saved or sent.
    pub bundle_selected: BundleSelected,
    /// `minecraft:suspicious_stew_effects`: (effect id, duration).
    pub stew_effects: Vec<(Arc<str>, i32)>,
    /// `minecraft:map_post_processing`: 0 lock, 1 scale.
    pub map_post_processing: Option<u8>,
    /// Components Horizon does not model (lore, custom data, item name,
    /// max stack size...), kept as their saved values in key order.
    pub other: Vec<(String, Tag)>,
    /// What the server sent for this stack, for sending it back unchanged.
    pub wire: Wire,
}
/// The component patch a server sent for a stack, as the creative slot packet
/// writes it, and the stack Horizon decoded from it, to tell whether anything
/// changed since. Like vanilla's `BundleContents.selectedItem` it is left out
/// when stacks are compared.
#[derive(Clone, Debug, Default)]
pub struct Wire(Option<Arc<(ItemStack, Arc<[u8]>)>>);
impl Wire {
    pub fn new(decoded: ItemStack, patch: Arc<[u8]>) -> Self {
        Self(Some(Arc::new((decoded, patch))))
    }
    /// The server's patch, whether or not the stack changed since.
    pub fn patch(&self) -> Option<&[u8]> {
        self.0.as_deref().map(|(_, patch)| &**patch)
    }
}
impl PartialEq for Wire {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
/// A bundle's selected item index (-1: none). Like vanilla's
/// `BundleContents.equals` it is left out when stacks are compared.
#[derive(Clone, Copy, Debug)]
pub struct BundleSelected(pub i32);
impl Default for BundleSelected {
    fn default() -> Self {
        Self(-1)
    }
}
impl PartialEq for BundleSelected {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
/// The components `components_tag` writes from modelled fields.
const MODELLED: &[&str] = &[
    "minecraft:banner_patterns",
    "minecraft:base_color",
    "minecraft:bundle_contents",
    "minecraft:charged_projectiles",
    "minecraft:container",
    "minecraft:custom_name",
    "minecraft:damage",
    "minecraft:dyed_color",
    "minecraft:enchantments",
    "minecraft:firework_explosion",
    "minecraft:fireworks",
    "minecraft:map_id",
    "minecraft:map_post_processing",
    "minecraft:ominous_bottle_amplifier",
    "minecraft:pot_decorations",
    "minecraft:potion_contents",
    "minecraft:repair_cost",
    "minecraft:stored_enchantments",
    "minecraft:suspicious_stew_effects",
    "minecraft:trim",
    "minecraft:writable_book_content",
    "minecraft:written_book_content",
];
impl StackData {
    pub fn custom_name_plain(&self) -> Option<String> {
        self.custom_name.as_ref().and_then(read_text)
    }

    /// A preserved component's saved value.
    pub fn other(&self, id: &str) -> Option<&Tag> {
        if id == "minecraft:custom_name" {
            return self.custom_name.as_ref();
        }
        self.other.iter().find(|(k, _)| k == id).map(|(_, v)| v)
    }
    /// `SetNameFunction.run`: retain a loot name as its original component.
    pub fn set_loot_name(&mut self, id: &str, text: &serde_json::Value) {
        let tag = horizon_client_core::scoreboard::json_to_tag(text);
        self.set_other(id, Some(tag));
    }
    /// `SetComponentsFunction`: one component of a loot patch, in its JSON
    /// form. A component Horizon models is read into its field as a saved
    /// stack's is, so a rolled potion's contents are the effects it gives;
    /// the rest keep their saved components. A null value removes the
    /// component.
    pub fn set_loot_component(&mut self, id: &str, value: &serde_json::Value) {
        let tag = (!value.is_null()).then(|| horizon_client_core::scoreboard::json_to_tag(value));
        match id {
            // `set_banner_pattern` (and `set_components`) write the layers a
            // banner or shield wears; the renderer and the tooltip read them
            // from `banner_patterns`, not from a tag in `other`.
            "minecraft:banner_patterns" => {
                self.banner_patterns = crate::banner_layer::layers_from_nbt(tag.as_ref());
            }
            // `set_book_cover` (and `set_components`) write the book's title,
            // author, generation and pages as one component: the fields the
            // tooltip and the reading screen use come back out of it.
            "minecraft:written_book_content" => {
                self.written = tag.as_ref().map(written_book_from_tag);
            }
            "minecraft:trim" => {
                self.trim = tag.as_ref().and_then(trim::from_nbt);
            }
            // The stack's damage is outside its data; a name keeps its own reading.
            "minecraft:damage" | "minecraft:custom_name" => self.set_other(id, tag),
            _ if MODELLED.contains(&id) => {
                let mut saved = match components_tag(0, Some(self)) {
                    Some(Tag::Compound(c)) => c,
                    _ => Default::default(),
                };
                saved.remove(&JavaString::from(id));
                if let Some(tag) = tag {
                    saved.insert(JavaString::from(id), tag);
                }
                // A damage set before stays where it was: reading would take it.
                let damage = self.other("minecraft:damage").cloned();
                let (_, read) = decode_components(Some(&Tag::Compound(saved)));
                *self = read.map_or_else(Default::default, |data| *data);
                self.set_other("minecraft:damage", damage);
            }
            _ => self.set_other(id, tag),
        }
    }
    /// Set (or with None remove) a preserved component.
    pub fn set_other(&mut self, id: &str, value: Option<Tag>) {
        self.other.retain(|(k, _)| k != id);
        if id == "minecraft:custom_name" {
            self.custom_name = value
                .filter(|tag| read_text(tag).is_some())
                .map(collapsed_text);
            return;
        }
        if let Some(v) = value {
            self.other.push((id.to_owned(), v));
            self.other.sort_by(|a, b| a.0.cmp(&b.0));
        }
    }
    /// `DebugStickState.properties().get(block)`: the property selected for
    /// `block` in `minecraft:debug_stick_state`.
    pub fn debug_stick_property(&self, block: &str) -> Option<String> {
        let Tag::Compound(map) = self.other("minecraft:debug_stick_state")? else {
            return None;
        };
        match map.get(&JavaString::from(block))? {
            Tag::String(property) => Some(property.to_string_lossy()),
            _ => None,
        }
    }
    /// `DebugStickState.withProperty(block, property)`.
    pub fn set_debug_stick_property(&mut self, block: &str, property: &str) {
        let mut map = match self.other("minecraft:debug_stick_state") {
            Some(Tag::Compound(map)) => map.clone(),
            _ => Default::default(),
        };
        map.insert(
            JavaString::from(block),
            Tag::String(JavaString::from(property)),
        );
        self.set_other("minecraft:debug_stick_state", Some(Tag::Compound(map)));
    }
}
/// `Raid.getBannerComponentPatch`: the ominous banner's pattern layers.
pub const OMINOUS_LAYERS: [(&str, &str); 8] = [
    ("rhombus", "cyan"),
    ("stripe_bottom", "light_gray"),
    ("stripe_center", "gray"),
    ("border", "light_gray"),
    ("stripe_middle", "black"),
    ("half_horizontal", "light_gray"),
    ("circle", "light_gray"),
    ("border", "black"),
];

/// The raid captain banner's default tooltip setting, stored by vanilla as a
/// component but represented by `StackData::ominous_banner` in Horizon.
fn ominous_tooltip_display() -> Tag {
    Tag::compound([(
        "hidden_components",
        Tag::List(vec![Tag::String("minecraft:banner_patterns".into())]),
    )])
}

/// `WrittenBookContent` with the pages as plain text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WrittenBook {
    pub title: String,
    pub author: String,
    pub generation: u8,
    pub pages: Vec<String>,
    /// Each page's component when it is not a plain string, so that its
    /// click events survive; empty or one per page.
    pub page_tags: Vec<Tag>,
}
impl StackData {
    pub fn is_empty(&self) -> bool {
        self.wire.0.is_none() && *self == Self::default()
    }
}

/// `BlockItem.onDestroyed` (`ItemStack.onDestroyed` for a container item): the
/// stack's `container` is emptied and the stacks it held come back in slot
/// order â€” `ItemUtils.onContainerDestroyed` drops each where the entity lay.
pub fn destroyed_container(stack: &mut ItemStack) -> Vec<ItemStack> {
    let Some(data) = stack.extra.as_deref_mut() else {
        return Vec::new();
    };
    if data.container.is_empty() {
        return Vec::new();
    }
    // `set(CONTAINER, ItemContainerContents.EMPTY)`: no slot left filled.
    let mut held = std::mem::take(&mut data.container);
    stack.tidy();
    // `nonEmptyItemCopyStream` walks the 256 slots in order.
    held.sort_by_key(|(slot, _)| *slot);
    held.into_iter().map(|(_, stack)| stack).collect()
}

/// `NbtOps.getBooleanValue`, used by boolean component codecs: every numeric tag
/// is true unless its double value is zero. In particular, fractions stay true.
pub fn component_bool(value: &Tag) -> Option<bool> {
    Some(match value {
        Tag::Byte(v) => *v != 0,
        Tag::Short(v) => *v != 0,
        Tag::Int(v) => *v != 0,
        Tag::Long(v) => *v != 0,
        Tag::Float(bits) => f32::from_bits(*bits) != 0.0,
        Tag::Double(bits) => f64::from_bits(*bits) != 0.0,
        _ => return None,
    })
}

impl ItemStack {
    pub fn new(item: &str, count: u32) -> Self {
        Self {
            item: Arc::from(item),
            count,
            damage: 0,
            extra: None,
        }
    }
    /// The stack `ItemStack.CODEC` reads from `tag` (`id`, `count`,
    /// `components`), as `item_nbt::item_from_nbt` does; None for air or an
    /// id that is no valid identifier. The id is not looked up in a registry.
    /// API @API:item_from_snbt@.
    pub fn from_nbt(tag: &horizon_world::nbt::Compound) -> Option<Self> {
        crate::item_nbt::item_from_nbt(&Tag::Compound(tag.clone()))
    }
    pub fn data(&self) -> Option<&StackData> {
        self.extra.as_deref()
    }
    /// The patch the server sent for this stack, while the stack is still what
    /// it decoded to (its count aside); None once anything else changed.
    pub fn wire_patch(&self) -> Option<Arc<[u8]>> {
        let (decoded, patch) = &**self.data()?.wire.0.as_ref()?;
        let none = StackData::default();
        (decoded.item == self.item
            && decoded.damage == self.damage
            && decoded.data().unwrap_or(&none) == self.data().unwrap_or(&none))
        .then(|| patch.clone())
    }
    pub fn data_mut(&mut self) -> &mut StackData {
        self.extra.get_or_insert_with(Box::default)
    }
    /// Drops an all-default component box so equal stacks compare equal.
    pub fn tidy(&mut self) {
        if self.extra.as_ref().is_some_and(|d| d.is_empty()) {
            self.extra = None;
        }
    }
    pub fn enchantments(&self) -> &[(Arc<str>, u32)] {
        self.data().map_or(&[], |d| &d.enchantments)
    }
    pub fn stored_enchantments(&self) -> &[(Arc<str>, u32)] {
        self.data().map_or(&[], |d| &d.stored_enchantments)
    }
    /// `EnchantmentHelper.getItemEnchantmentLevel` (0 when absent).
    pub fn enchantment_level(&self, id: &str) -> u32 {
        self.enchantments()
            .iter()
            .find(|(e, _)| &**e == id)
            .map_or(0, |(_, l)| *l)
    }
    /// The component an anvil/grindstone works on: stored for books.
    pub fn crafting_enchantments(&self) -> &[(Arc<str>, u32)] {
        if &*self.item == "minecraft:enchanted_book" {
            self.stored_enchantments()
        } else {
            self.enchantments()
        }
    }
    pub fn set_crafting_enchantments(&mut self, levels: Levels) {
        if &*self.item == "minecraft:enchanted_book" {
            self.data_mut().stored_enchantments = levels;
        } else {
            self.data_mut().enchantments = levels;
        }
        self.tidy();
    }
    /// `ItemStack.enchant`: upgrade to the higher level. Vanilla's
    /// `EnchantmentHelper.updateEnchantments` returns EMPTY when the component
    /// is absent, so a removal patch makes this a no-op.
    pub fn enchant(&mut self, id: &str, level: u32) {
        let book = &*self.item == "minecraft:enchanted_book";
        let removal = if book {
            "!minecraft:stored_enchantments"
        } else {
            "!minecraft:enchantments"
        };
        if self
            .data()
            .is_some_and(|data| data.other(removal).is_some())
        {
            return;
        }
        let data = self.data_mut();
        let list = if book {
            &mut data.stored_enchantments
        } else {
            &mut data.enchantments
        };
        upgrade(list, id, level);
    }
    pub fn has_any_enchantments(&self) -> bool {
        !self.enchantments().is_empty() || !self.stored_enchantments().is_empty()
    }
    /// `ItemStack.hasFoil`: an explicit `minecraft:enchantment_glint_override`
    /// component wins, then the item's default override (`item_glint`), then a
    /// non-empty `minecraft:enchantments` component (`Item.isFoil` is
    /// `ItemStack.isEnchanted`; stored book enchantments do not count).
    pub fn has_foil(&self, item_glint: bool) -> bool {
        if let Some(flag) = self
            .data()
            .and_then(|d| d.other("minecraft:enchantment_glint_override"))
            .and_then(component_bool)
        {
            return flag;
        }
        item_glint || !self.enchantments().is_empty()
    }
    pub fn repair_cost(&self) -> u32 {
        self.data().map_or(0, |d| d.repair_cost)
    }
    pub fn custom_name(&self) -> Option<String> {
        self.data()?.custom_name_plain()
    }
    /// Non-empty slots of a `minecraft:container` component.
    pub fn container(&self) -> &[(u8, ItemStack)] {
        self.data().map_or(&[], |d| &d.container)
    }
    /// The `minecraft:custom_data` compound if present.
    pub fn custom_data(&self) -> Option<&horizon_world::nbt::Compound> {
        match self.data()?.other("minecraft:custom_data")? {
            Tag::Compound(map) => Some(map),
            _ => None,
        }
    }
    /// `ResolvableProfile.name`: a bare profile string or its `name` field.
    pub fn profile_name(&self) -> Option<String> {
        let profile = self.data()?.other("minecraft:profile")?;
        string(profile).or_else(|| profile.get("name").and_then(string))
    }
    /// `minecraft:item_name` in the current language (`getItemName`'s
    /// component). It stays in the patch as loaded, so a translatable name
    /// saves and reaches clients unchanged; the ominous banner's is modelled
    /// by `ominous_banner` instead.
    pub fn item_name(&self) -> Option<String> {
        let name = self.data()?.other("minecraft:item_name")?;
        Some(horizon_client_core::component::name_text(name).plain())
    }
    pub fn potion(&self) -> Option<&str> {
        self.data().and_then(|d| d.potion.as_deref())
    }
    /// `PotionContents.customEffects` as saved: (id, duration, amplifier). Both
    /// default to 0 like `MobEffectInstance.CODEC`.
    pub fn custom_potion_effects(&self) -> Vec<(String, i32, i32)> {
        let Some((_, Tag::List(list))) = self
            .data()
            .and_then(|d| d.potion_extra.iter().find(|(k, _)| k == "custom_effects"))
        else {
            return Vec::new();
        };
        list.iter()
            .filter_map(|e| {
                Some((
                    string(e.get("id")?)?,
                    e.get("duration").and_then(int).unwrap_or(0) as i32,
                    e.get("amplifier").and_then(int).unwrap_or(0).clamp(0, 255) as i32,
                ))
            })
            .collect()
    }
    /// `DataComponents.POTION_DURATION_SCALE` with the item's default: 0.125
    /// for a tipped arrow, 0.25 for a lingering potion, else 1.
    pub fn potion_duration_scale(&self) -> f32 {
        match self
            .data()
            .and_then(|d| d.other("minecraft:potion_duration_scale"))
        {
            Some(Tag::Float(bits)) => f32::from_bits(*bits),
            Some(Tag::Double(bits)) => f64::from_bits(*bits) as f32,
            Some(t) if int(t).is_some() => int(t).unwrap_or(1) as f32,
            _ => match &*self.item {
                "minecraft:tipped_arrow" => 0.125,
                "minecraft:lingering_potion" => 0.25,
                _ => 1.0,
            },
        }
    }
    pub fn with_potion(mut self, potion: &str) -> Self {
        self.data_mut().potion = Some(Arc::from(potion));
        self
    }
    /// `ItemStack.isSameItemSameComponents`.
    pub fn same_item_same_components(&self, other: &Self) -> bool {
        self.item == other.item && self.damage == other.damage && self.data() == other.data()
    }
    /// `ItemStack.transmuteCopy`: another item keeping every component.
    pub fn transmute(&self, item: &str, count: u32) -> Self {
        let mut stack = Self {
            item: Arc::from(item),
            count,
            ..self.clone()
        };
        stack.tidy();
        stack
    }
}
/// `ItemEnchantments.Mutable.upgrade`.
pub fn upgrade(list: &mut Levels, id: &str, level: u32) {
    if level == 0 {
        return;
    }
    match list.iter_mut().find(|(e, _)| &**e == id) {
        Some((_, l)) => *l = (*l).max(level),
        None => list.push((Arc::from(id), level)),
    }
}
/// `ItemEnchantments.Mutable.set` (level 0 removes).
pub fn set_level(list: &mut Levels, id: &str, level: u32) {
    if level == 0 {
        list.retain(|(e, _)| &**e != id);
    } else {
        match list.iter_mut().find(|(e, _)| &**e == id) {
            Some((_, l)) => *l = level,
            None => list.push((Arc::from(id), level)),
        }
    }
}

fn levels_tag(levels: &Levels) -> Tag {
    Tag::compound(levels.iter().map(|(id, l)| (&**id, Tag::Int(*l as i32))))
}
fn int(t: &Tag) -> Option<i64> {
    match t {
        Tag::Byte(v) => Some(i64::from(*v)),
        Tag::Short(v) => Some(i64::from(*v)),
        Tag::Int(v) => Some(i64::from(*v)),
        Tag::Long(v) => Some(*v),
        _ => None,
    }
}
fn string(t: &Tag) -> Option<String> {
    match t {
        Tag::String(s) => Some(s.to_string_lossy()),
        _ => None,
    }
}
/// The persistent `ItemEnchantments` codec uses integer levels in 1..=255.
/// `TagValueInput.read` keeps the partial map when an entry fails decoding.
fn read_levels(t: Option<&Tag>) -> Levels {
    let Some(Tag::Compound(map)) = t else {
        return Vec::new();
    };
    // Pre-1.21.5 worlds wrap the map in {levels: {...}}.
    let map = match map.get(&JavaString::from("levels")) {
        Some(Tag::Compound(inner)) => inner,
        _ => map,
    };
    // The keys are enchantment Identifiers: `knockback` is `minecraft:knockback`.
    map.iter()
        .filter_map(|(k, v)| {
            let level = horizon_world::datafix::dynamic::as_number(v)?.int_value();
            if !(1..=255).contains(&level) {
                return None;
            }
            let id = horizon_world::datafix::dynamic::namespaced(&k.to_string_lossy());
            Some((Arc::from(id), level as u32))
        })
        .collect()
}
/// A text component as stored by vanilla: a plain string or `{"text": ...}`.
/// A `Filterable<String/Component>`: the value itself or `{raw: value}`.
/// `ComponentSerialization.CODEC`'s short form: a plain text component with
/// no style and no siblings is written as its string.
fn plain_component(t: &Tag) -> Tag {
    if let Tag::Compound(c) = t
        && let Some(text @ Tag::String(_)) = c.get(&"text".into())
        && c.iter().all(|(k, v)| {
            k.to_string_lossy() == "text"
                || (k.to_string_lossy() == "type"
                    && matches!(v, Tag::String(s) if s.to_string_lossy() == "text"))
        })
    {
        return text.clone();
    }
    t.clone()
}
/// `WrittenBookContent` from its component: the title and author (a
/// `Filterable`'s raw text), the generation and the pages, whose component
/// tags are kept apart from their plain text for click events.
fn written_book_from_tag(b: &Tag) -> WrittenBook {
    WrittenBook {
        title: b.get("title").and_then(filterable_text).unwrap_or_default(),
        author: b.get("author").and_then(string).unwrap_or_default(),
        generation: b.get("generation").and_then(int).unwrap_or(0).clamp(0, 3) as u8,
        pages: match b.get("pages") {
            Some(Tag::List(pages)) => pages.iter().filter_map(filterable_text).collect(),
            _ => Vec::new(),
        },
        page_tags: match b.get("pages") {
            Some(Tag::List(pages)) => pages
                .iter()
                .map(|p| plain_component(p.get("raw").unwrap_or(p)))
                .collect(),
            _ => Vec::new(),
        },
    }
}

fn filterable_text(t: &Tag) -> Option<String> {
    match t.get("raw") {
        Some(raw) => read_text(raw),
        None => read_text(t),
    }
}
fn read_text(t: &Tag) -> Option<String> {
    match t {
        Tag::String(s) => Some(s.to_string_lossy()),
        // A component's root `text` may be empty with the visible words in
        // `extra` (every legacy display name ViaVersion produces looks like
        // this), so flatten the whole component instead of reading one key.
        // ComponentSerialization.CODEC also takes a list (its first element
        // styling the rest), as servers behind ViaVersion send book pages.
        Tag::Compound(_) | Tag::List(_) => {
            Some(horizon_client_core::component::name_text(t).plain())
        }
        _ => None,
    }
}

/// `Component.tryCollapseToString`: `ComponentSerialization.CODEC` writes
/// plain text with no style and no siblings as a bare string.
fn collapsed_text(tag: Tag) -> Tag {
    if let Tag::Compound(map) = &tag
        && let Some(Tag::String(text)) = tag.get("text")
        && map
            .iter()
            .all(|(key, value)| match key.to_string_lossy().as_str() {
                "text" => true,
                "type" => matches!(value, Tag::String(s) if s.to_string_lossy() == "text"),
                "extra" => matches!(value, Tag::List(l) if l.is_empty()),
                _ => false,
            })
    {
        return Tag::String(text.clone());
    }
    tag
}

/// A `FireworkExplosion` as vanilla's codec writes it (defaults omitted).
fn explosion_tag(e: &crate::firework::FireworkExplosion) -> Tag {
    let colors = |c: &[u32]| Tag::List(c.iter().map(|c| Tag::Int(*c as i32)).collect());
    let mut fields = vec![("shape", Tag::String(e.shape.name().into()))];
    if !e.colors.is_empty() {
        fields.push(("colors", colors(&e.colors)));
    }
    if !e.fade.is_empty() {
        fields.push(("fade_colors", colors(&e.fade)));
    }
    if e.trail {
        fields.push(("has_trail", Tag::Byte(1)));
    }
    if e.twinkle {
        fields.push(("has_twinkle", Tag::Byte(1)));
    }
    Tag::compound(fields)
}
fn explosion_from(e: &Tag) -> crate::firework::FireworkExplosion {
    let ints = |t: Option<&Tag>| match t {
        Some(Tag::IntArray(v)) => v.iter().map(|c| *c as u32).collect(),
        Some(Tag::List(v)) => v.iter().filter_map(int).map(|c| c as u32).collect(),
        _ => Vec::new(),
    };
    crate::firework::FireworkExplosion {
        shape: crate::firework::FireworkShape::from_name(
            e.get("shape")
                .and_then(string)
                .as_deref()
                .map_or("small_ball", |s| s.strip_prefix("minecraft:").unwrap_or(s)),
        ),
        colors: ints(e.get("colors")),
        fade: ints(e.get("fade_colors")),
        trail: e.get("has_trail").and_then(int).unwrap_or(0) != 0,
        twinkle: e.get("has_twinkle").and_then(int).unwrap_or(0) != 0,
    }
}
/// An `ItemStackTemplate`: `{id, count?, components?}`.
fn template_tag(stack: &ItemStack) -> Tag {
    let mut entries = vec![("id", Tag::String((&*stack.item).into()))];
    if stack.count != 1 {
        entries.push(("count", Tag::Int(stack.count as i32)));
    }
    if let Some(components) = encode_components(stack) {
        entries.push(("components", components));
    }
    Tag::compound(entries)
}
/// The pot prototype's decorations: a brick on every side.
pub fn all_bricks(sides: &[Option<ItemStack>; 4]) -> bool {
    sides.iter().all(|s| {
        s.as_ref()
            .is_some_and(|s| &*s.item == "minecraft:brick" && s.count == 1 && s.extra.is_none())
    })
}
fn template_from(tag: &Tag) -> Option<ItemStack> {
    match tag {
        Tag::String(id) => Some(ItemStack::new(&id.to_string_lossy(), 1)),
        _ => {
            let id = tag.get("id").and_then(string)?;
            let (damage, extra) = decode_components(tag.get("components"));
            Some(ItemStack {
                item: Arc::from(id.as_str()),
                count: tag.get("count").and_then(int).unwrap_or(1).max(1) as u32,
                damage,
                extra,
            })
        }
    }
}
/// The stack a `show_item` hover event holds (`HoverEvent.ShowItem`): its
/// `{id, count?, components?}` object.
pub fn stack_from_hover(event: &serde_json::Value) -> Option<ItemStack> {
    template_from(&horizon_client_core::scoreboard::json_to_tag(event))
}
/// Sorted-key, type-annotated text of a tag (the differential harnesses
/// print the same form from the vanilla side).
pub fn canon_tag(tag: &Tag) -> String {
    fn list(out: &mut String, prefix: &str, items: impl Iterator<Item = String>) {
        out.push_str(prefix);
        out.push_str(&items.collect::<Vec<_>>().join(","));
        out.push(']');
    }
    fn go(tag: &Tag, out: &mut String) {
        use std::fmt::Write as _;
        match tag {
            Tag::End => {}
            Tag::Byte(v) => write!(out, "{v}b").unwrap(),
            Tag::Short(v) => write!(out, "{v}s").unwrap(),
            Tag::Int(v) => write!(out, "{v}").unwrap(),
            Tag::Long(v) => write!(out, "{v}L").unwrap(),
            Tag::Float(v) => write!(out, "{}f", f32::from_bits(*v)).unwrap(),
            Tag::Double(v) => write!(out, "{}d", f64::from_bits(*v)).unwrap(),
            Tag::String(s) => write!(
                out,
                "\"{}\"",
                s.to_string_lossy()
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
            )
            .unwrap(),
            Tag::ByteArray(v) => list(out, "[B;", v.iter().map(|b| (*b as i8).to_string())),
            Tag::IntArray(v) => list(out, "[I;", v.iter().map(i32::to_string)),
            Tag::LongArray(v) => list(out, "[L;", v.iter().map(i64::to_string)),
            Tag::List(items) => {
                out.push('[');
                for (i, t) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    go(t, out);
                }
                out.push(']');
            }
            Tag::Compound(map) => {
                let mut keys: Vec<(String, &Tag)> =
                    map.iter().map(|(k, v)| (k.to_string_lossy(), v)).collect();
                keys.sort_by(|a, b| a.0.cmp(&b.0));
                out.push('{');
                for (i, (k, v)) in keys.into_iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&k);
                    out.push(':');
                    go(v, out);
                }
                out.push('}');
            }
        }
    }
    let mut out = String::new();
    go(tag, &mut out);
    out
}
/// `canon_tag` of a stack as `ItemStack.CODEC` writes it; "-" for none.
pub fn canon_stack(stack: Option<&ItemStack>) -> String {
    stack.map_or_else(
        || "-".to_owned(),
        |s| canon_tag(&crate::item_nbt::item_to_nbt(s)),
    )
}
/// The stack of an SNBT `ItemStack.CODEC` compound; None for "-".
pub fn parse_stack(snbt: &str) -> Result<Option<ItemStack>, String> {
    if snbt == "-" {
        return Ok(None);
    }
    let (tag, _) = horizon_world::nbt::parse_snbt(snbt)?;
    Ok(crate::item_nbt::item_from_nbt(&tag))
}
/// `MapDecorations.Entry.CODEC`: a bare type ID uses the default resource namespace.
/// Keep preserved entry keys and numeric tags intact; this is not registry validation.
fn canonical_other_component(id: &str, tag: &Tag) -> Tag {
    let mut value = tag.clone();
    if id == "minecraft:map_decorations"
        && let Tag::Compound(entries) = &mut value
    {
        for entry in entries.values_mut() {
            if let Some(Tag::String(kind)) = entry.get_mut("type") {
                let text = kind.to_string_lossy();
                if !text.is_empty()
                    && !text.contains(':')
                    && let Ok(id) = horizon_world::identifier::Identifier::parse(&text)
                {
                    *kind = id.to_string().as_str().into();
                }
            }
        }
    }
    value
}
/// The saved `DataComponentPatch.CODEC` compound, or None when default.
/// A filled map's empty decorations equal its prototype (`Items.mapProperties`).
pub fn encode_components(stack: &ItemStack) -> Option<Tag> {
    let mut components = components_tag(stack.damage, stack.data())?;
    // MobEffectInstance.Details' codec always saves the resolved icon flag.
    // An omitted icon follows show_particles, whose default is true.
    if let Some(Tag::List(effects)) = components
        .get_mut("minecraft:potion_contents")
        .and_then(|potion| potion.get_mut("custom_effects"))
    {
        for effect in effects {
            let shown = effect.get("show_particles").and_then(int).unwrap_or(1) != 0;
            if let Tag::Compound(fields) = effect {
                fields
                    .entry(JavaString::from("show_icon"))
                    .or_insert(Tag::Byte(i8::from(shown)));
            }
        }
    }
    // LodestoneTracker.CODEC: optionalFieldOf("tracked", true).
    if let Some(Tag::Compound(tracker)) = components.get_mut("minecraft:lodestone_tracker")
        && tracker.get(&JavaString::from("tracked")) == Some(&Tag::Byte(1))
    {
        tracker.remove(&JavaString::from("tracked"));
    }
    if &*stack.item == "minecraft:filled_map"
        && matches!(components.get("minecraft:map_decorations"), Some(Tag::Compound(entries)) if entries.is_empty())
        && let Tag::Compound(entries) = &mut components
    {
        entries.remove(&JavaString::from("minecraft:map_decorations"));
        if entries.is_empty() {
            return None;
        }
    }
    // Items.ENCHANTED_BOOK has empty stored enchantments in its prototype.
    // Keep the live presence marker; only the saved component patch elides it.
    if &*stack.item == "minecraft:enchanted_book"
        && matches!(components.get("minecraft:stored_enchantments"), Some(Tag::Compound(entries)) if entries.is_empty())
        && let Tag::Compound(entries) = &mut components
    {
        entries.remove(&JavaString::from("minecraft:stored_enchantments"));
        if entries.is_empty() {
            return None;
        }
    }
    // Items.BEEHIVE/BEE_NEST use this whole block-state map as their prototype.
    // Empty maps and maps with extra properties are distinct, nondefault patches.
    if matches!(&*stack.item, "minecraft:beehive" | "minecraft:bee_nest")
        && matches!(components.get("minecraft:block_state"), Some(Tag::Compound(state))
            if state.len() == 1
                && matches!(state.get(&JavaString::from("honey_level")),
                    Some(Tag::String(level)) if level.to_string_lossy() == "0"))
        && let Tag::Compound(entries) = &mut components
    {
        entries.remove(&JavaString::from("minecraft:block_state"));
        if entries.is_empty() {
            return None;
        }
    }
    Some(components)
}
/// Saved patch with the loaded item's prototype available.
/// Context-free callers and the live `components_tag` view stay unchanged.
pub fn encode_components_with_items(stack: &ItemStack, items: &crate::items::Items) -> Option<Tag> {
    let mut components = encode_components(stack)?;
    // PatchedDataComponentMap.set removes values equal to the prototype.
    if let Some(prototype) = items.get(&stack.item)
        && prototype
            .components
            .iter()
            .any(|id| id == "minecraft:max_stack_size")
        && matches!(components.get("minecraft:max_stack_size"), Some(Tag::Int(size))
            if *size > 0 && *size as u32 == prototype.max_stack)
        && let Tag::Compound(entries) = &mut components
    {
        entries.remove(&JavaString::from("minecraft:max_stack_size"));
        if entries.is_empty() {
            return None;
        }
    }
    Some(components)
}
/// `encode_components` from the damage value and extra components.
pub fn components_tag(damage: u32, data: Option<&StackData>) -> Option<Tag> {
    let mut entries: Vec<(&str, Tag)> = Vec::new();
    if damage > 0 {
        entries.push(("minecraft:damage", Tag::Int(damage as i32)));
    }
    if let Some(d) = data {
        if !d.enchantments.is_empty() {
            entries.push(("minecraft:enchantments", levels_tag(&d.enchantments)));
        }
        if !d.stored_enchantments.is_empty() {
            entries.push((
                "minecraft:stored_enchantments",
                levels_tag(&d.stored_enchantments),
            ));
        }
        if d.repair_cost > 0 {
            entries.push(("minecraft:repair_cost", Tag::Int(d.repair_cost as i32)));
        }
        if let Some(name) = &d.custom_name {
            entries.push(("minecraft:custom_name", name.clone()));
        }
        if d.potion.is_some() || !d.potion_extra.is_empty() {
            let potion = d
                .potion
                .as_ref()
                .map(|p| ("potion", Tag::String((&**p).into())));
            let extra = d.potion_extra.iter().map(|(k, v)| (k.as_str(), v.clone()));
            entries.push((
                "minecraft:potion_contents",
                Tag::compound(potion.into_iter().chain(extra)),
            ));
        }
        if let Some(trim) = &d.trim {
            entries.push(("minecraft:trim", trim::to_nbt(trim)));
        }
        if let Some(id) = d.map_id {
            entries.push(("minecraft:map_id", Tag::Int(id)));
        }
        if !d.banner_patterns.is_empty() {
            entries.push((
                "minecraft:banner_patterns",
                crate::banner_layer::layers_to_nbt(&d.banner_patterns),
            ));
        }
        if let Some(color) = &d.base_color {
            entries.push(("minecraft:base_color", Tag::String((&**color).into())));
        }
        // Filterable values are saved as {raw: ..}.
        let raw = |t: Tag| Tag::compound([("raw", t)]);
        if !d.writable_pages.is_empty() {
            let pages = d
                .writable_pages
                .iter()
                .map(|p| raw(Tag::String(p.as_str().into())))
                .collect();
            entries.push((
                "minecraft:writable_book_content",
                Tag::compound([("pages", Tag::List(pages))]),
            ));
        }
        if let Some(w) = &d.written {
            let pages = if w.page_tags.len() == w.pages.len() {
                w.page_tags.iter().cloned().map(raw).collect()
            } else {
                w.pages
                    .iter()
                    .map(|p| raw(Tag::String(p.as_str().into())))
                    .collect()
            };
            entries.push((
                "minecraft:written_book_content",
                Tag::compound({
                    let mut fields = vec![
                        ("title", raw(Tag::String(w.title.as_str().into()))),
                        ("author", Tag::String(w.author.as_str().into())),
                    ];
                    // `optionalFieldOf("pages", List.of())`: no pages, no field.
                    if !w.pages.is_empty() {
                        fields.push(("pages", Tag::List(pages)));
                    }
                    if w.generation != 0 {
                        fields.push(("generation", Tag::Int(i32::from(w.generation))));
                    }
                    fields
                }),
            ));
        }
        if let Some((flight, explosions)) = &d.fireworks {
            let mut fields = Vec::new();
            if *flight != 0 {
                fields.push(("flight_duration", Tag::Byte(*flight as i8)));
            }
            if !explosions.is_empty() {
                fields.push((
                    "explosions",
                    Tag::List(explosions.iter().map(explosion_tag).collect()),
                ));
            }
            entries.push(("minecraft:fireworks", Tag::compound(fields)));
        }
        if let Some(e) = &d.firework_explosion {
            entries.push(("minecraft:firework_explosion", explosion_tag(e)));
        }
        if let Some(color) = d.dyed_color {
            entries.push(("minecraft:dyed_color", Tag::Int(color)));
        }
        if let Some(sides) = &d.pot_decorations {
            let mut sherds = Vec::new();
            for (name, side) in ["back", "left", "right", "front"].iter().zip(sides.iter()) {
                if let Some(stack) = side {
                    sherds.push((*name, template_tag(stack)));
                }
            }
            entries.push(("minecraft:pot_decorations", Tag::compound(sherds)));
        }
        if !d.bundle_contents.is_empty() {
            entries.push((
                "minecraft:bundle_contents",
                Tag::List(d.bundle_contents.iter().map(template_tag).collect()),
            ));
        }
        if !d.stew_effects.is_empty() {
            entries.push((
                "minecraft:suspicious_stew_effects",
                Tag::List(
                    d.stew_effects
                        .iter()
                        .map(|(id, duration)| {
                            // The codec leaves out the default duration (160).
                            let mut e = vec![("id", Tag::String((&**id).into()))];
                            if *duration != 160 {
                                e.push(("duration", Tag::Int(*duration)));
                            }
                            Tag::compound(e)
                        })
                        .collect(),
                ),
            ));
        }
        // minecraft:map_post_processing is transient: ItemStack.CODEC never
        // saves it (only the network carries it).
        if !d.charged.is_empty() {
            entries.push((
                "minecraft:charged_projectiles",
                Tag::List(d.charged.iter().map(template_tag).collect()),
            ));
        }
        if d.ominous_banner {
            let layer = |pattern: &str, color: &str| {
                Tag::compound([
                    (
                        "pattern",
                        Tag::String(format!("minecraft:{pattern}").as_str().into()),
                    ),
                    ("color", Tag::String(color.into())),
                ])
            };
            entries.push((
                "minecraft:banner_patterns",
                Tag::List(OMINOUS_LAYERS.iter().map(|(p, c)| layer(p, c)).collect()),
            ));
            if d.other("minecraft:tooltip_display").is_none() {
                entries.push(("minecraft:tooltip_display", ominous_tooltip_display()));
            }
            entries.push((
                "minecraft:item_name",
                Tag::compound([(
                    "translate",
                    Tag::String("block.minecraft.ominous_banner".into()),
                )]),
            ));
            entries.push(("minecraft:rarity", Tag::String("uncommon".into())));
        }
        if let Some(a) = d.ominous_amplifier {
            entries.push(("minecraft:ominous_bottle_amplifier", Tag::Int(i32::from(a))));
        }
        if !d.container.is_empty() {
            let slots = d
                .container
                .iter()
                .map(|(slot, stack)| {
                    Tag::compound([
                        ("slot", Tag::Int(i32::from(*slot))),
                        ("item", template_tag(stack)),
                    ])
                })
                .collect();
            entries.push(("minecraft:container", Tag::List(slots)));
        }
    }
    let mut out: horizon_world::nbt::Compound = entries
        .into_iter()
        .map(|(k, v)| (JavaString::from(k), v))
        .collect();
    if let Some(d) = data {
        for (k, v) in &d.other {
            out.entry(JavaString::from(k.as_str()))
                .or_insert_with(|| canonical_other_component(k, v));
        }
    }
    (!out.is_empty()).then_some(Tag::Compound(out))
}
/// The ASCII bytes of an NBT key, in `buf`. Every component ID is ASCII; a key
/// with any other unit cannot be one of them and needs no comparison.
fn ascii_key<'a>(key: &JavaString, buf: &'a mut [u8; 64]) -> Option<&'a str> {
    let units = &key.0;
    if units.len() > buf.len() {
        return None;
    }
    for (slot, &unit) in buf.iter_mut().zip(units) {
        if unit > 0x7f {
            return None;
        }
        *slot = unit as u8;
    }
    std::str::from_utf8(&buf[..units.len()]).ok()
}
/// The saved tags of the component IDs `decode_components` models, found in one
/// pass over the compound. Reading them one at a time built a key and walked the
/// tree for every ID, present or not, and most items carry one or two.
#[derive(Default)]
struct Seen<'a> {
    damage: Option<&'a Tag>,
    potion_contents: Option<&'a Tag>,
    enchantments: Option<&'a Tag>,
    stored_enchantments: Option<&'a Tag>,
    repair_cost: Option<&'a Tag>,
    custom_name: Option<&'a Tag>,
    map_id: Option<&'a Tag>,
    banner_patterns: Option<&'a Tag>,
    base_color: Option<&'a Tag>,
    writable_book_content: Option<&'a Tag>,
    written_book_content: Option<&'a Tag>,
    fireworks: Option<&'a Tag>,
    firework_explosion: Option<&'a Tag>,
    dyed_color: Option<&'a Tag>,
    pot_decorations: Option<&'a Tag>,
    bundle_contents: Option<&'a Tag>,
    suspicious_stew: Option<&'a Tag>,
    map_post_processing: Option<&'a Tag>,
    charged_projectiles: Option<&'a Tag>,
    container: Option<&'a Tag>,
    item_name: Option<&'a Tag>,
    amplifier: Option<&'a Tag>,
    trim: Option<&'a Tag>,
}
impl<'a> Seen<'a> {
    fn read(c: &'a Tag) -> Self {
        let Tag::Compound(map) = c else {
            return Self::default();
        };
        let mut seen = Self::default();
        let mut buf = [0u8; 64];
        for (key, value) in map {
            match ascii_key(key, &mut buf) {
                Some("minecraft:damage") => seen.damage = Some(value),
                Some("minecraft:potion_contents") => seen.potion_contents = Some(value),
                Some("minecraft:enchantments") => seen.enchantments = Some(value),
                Some("minecraft:stored_enchantments") => seen.stored_enchantments = Some(value),
                Some("minecraft:repair_cost") => seen.repair_cost = Some(value),
                Some("minecraft:custom_name") => seen.custom_name = Some(value),
                Some("minecraft:map_id") => seen.map_id = Some(value),
                Some("minecraft:banner_patterns") => seen.banner_patterns = Some(value),
                Some("minecraft:base_color") => seen.base_color = Some(value),
                Some("minecraft:writable_book_content") => seen.writable_book_content = Some(value),
                Some("minecraft:written_book_content") => seen.written_book_content = Some(value),
                Some("minecraft:fireworks") => seen.fireworks = Some(value),
                Some("minecraft:firework_explosion") => seen.firework_explosion = Some(value),
                Some("minecraft:dyed_color") => seen.dyed_color = Some(value),
                Some("minecraft:pot_decorations") => seen.pot_decorations = Some(value),
                Some("minecraft:bundle_contents") => seen.bundle_contents = Some(value),
                Some("minecraft:suspicious_stew_effects") => seen.suspicious_stew = Some(value),
                Some("minecraft:map_post_processing") => seen.map_post_processing = Some(value),
                Some("minecraft:charged_projectiles") => seen.charged_projectiles = Some(value),
                Some("minecraft:container") => seen.container = Some(value),
                Some("minecraft:item_name") => seen.item_name = Some(value),
                Some("minecraft:ominous_bottle_amplifier") => seen.amplifier = Some(value),
                Some("minecraft:trim") => seen.trim = Some(value),
                _ => {}
            }
        }
        seen
    }
}
/// Damage and extra components from a stack's `components` compound.
pub fn decode_components(components: Option<&Tag>) -> (u32, Option<Box<StackData>>) {
    let Some(c) = components else {
        return (0, None);
    };
    let seen = Seen::read(c);
    // NbtOps.getNumberValue supplies a boxed number; Codec.INT truncates its value.
    let damage = horizon_world::datafix::dynamic::as_int(seen.damage, 0).max(0) as u32;
    let potion = match seen.potion_contents {
        Some(Tag::String(s)) => Some(Arc::from(s.to_string_lossy().as_str())),
        Some(t) => t
            .get("potion")
            .and_then(string)
            .map(|s| Arc::from(s.as_str())),
        None => None,
    };
    let potion_extra = match seen.potion_contents {
        Some(Tag::Compound(map)) => map
            .iter()
            .filter(|(k, _)| k.to_string_lossy() != "potion")
            .map(|(k, v)| (k.to_string_lossy(), v.clone()))
            .collect(),
        _ => Vec::new(),
    };
    let mut data = StackData {
        enchantments: read_levels(seen.enchantments),
        stored_enchantments: read_levels(seen.stored_enchantments),
        repair_cost: seen.repair_cost.and_then(int).unwrap_or(0).max(0) as u32,
        explicit_zero_damage: damage == 0 && seen.damage.is_some(),
        custom_name: seen
            .custom_name
            .filter(|tag| read_text(tag).is_some())
            .cloned()
            .map(collapsed_text),
        potion,
        potion_extra,
        map_id: seen.map_id.and_then(int).map(|v| v as i32),
        banner_patterns: crate::banner_layer::layers_from_nbt(seen.banner_patterns),
        base_color: seen
            .base_color
            .and_then(string)
            .map(|s| Arc::from(s.as_str())),
        writable_pages: match seen.writable_book_content.and_then(|b| b.get("pages")) {
            Some(Tag::List(pages)) => pages.iter().filter_map(filterable_text).collect(),
            _ => Vec::new(),
        },
        written: seen.written_book_content.map(written_book_from_tag),
        fireworks: seen
            .fireworks
            .map(|f| {
                let flight = f
                    .get("flight_duration")
                    .and_then(int)
                    .unwrap_or(0)
                    .clamp(0, 255) as u8;
                let explosions = match f.get("explosions") {
                    Some(Tag::List(list)) => list.iter().map(explosion_from).collect(),
                    _ => Vec::new(),
                };
                (flight, explosions)
            })
            .filter(|(flight, explosions)| !(*flight == 1 && explosions.is_empty())),
        firework_explosion: seen.firework_explosion.map(explosion_from),
        dyed_color: seen.dyed_color.and_then(int).map(|v| v as i32),
        pot_decorations: seen
            .pot_decorations
            .map(|p| {
                let side = |name: &str| p.get(name).and_then(template_from);
                Box::new([side("back"), side("left"), side("right"), side("front")])
            })
            .filter(|sides| !all_bricks(sides)),
        bundle_contents: match seen.bundle_contents {
            Some(Tag::List(list)) => list.iter().filter_map(template_from).collect(),
            _ => Vec::new(),
        },
        stew_effects: match seen.suspicious_stew {
            Some(Tag::List(list)) => list
                .iter()
                .filter_map(|e| {
                    Some((
                        Arc::from(e.get("id").and_then(string)?.as_str()),
                        e.get("duration").and_then(int).unwrap_or(160) as i32,
                    ))
                })
                .collect(),
            _ => Vec::new(),
        },
        map_post_processing: match seen.map_post_processing {
            Some(Tag::String(s)) => Some(u8::from(s.to_string_lossy() != "lock")),
            Some(t) => int(t).map(|v| v.clamp(0, 1) as u8),
            None => None,
        },
        charged: match seen.charged_projectiles {
            Some(Tag::List(list)) => list
                .iter()
                .filter_map(crate::item_nbt::item_from_nbt)
                .collect(),
            _ => Vec::new(),
        },
        container: match seen.container {
            Some(Tag::List(list)) => list
                .iter()
                .filter_map(|e| {
                    Some((
                        e.get("slot").and_then(int)?.clamp(0, 255) as u8,
                        template_from(e.get("item")?)?,
                    ))
                })
                .collect(),
            _ => Vec::new(),
        },
        ominous_banner: seen
            .item_name
            .and_then(|n| n.get("translate"))
            .and_then(string)
            .as_deref()
            == Some("block.minecraft.ominous_banner")
            && matches!(
                seen.banner_patterns,
                Some(Tag::List(layers)) if layers.len() == OMINOUS_LAYERS.len()),
        ominous_amplifier: seen.amplifier.and_then(int).map(|v| v.clamp(0, 4) as u8),
        trim: seen.trim.and_then(trim::from_nbt),
        bundle_selected: BundleSelected::default(),
        other: Vec::new(),
        wire: Wire::default(),
    };
    if let Tag::Compound(map) = c {
        for (k, v) in map {
            let key = k.to_string_lossy();
            let ominous_part = data.ominous_banner
                && (matches!(key.as_str(), "minecraft:item_name" | "minecraft:rarity")
                    || (key == "minecraft:tooltip_display" && v == &ominous_tooltip_display()));
            // An explicitly empty stored_enchantments still counts as present
            // (`ItemStack.has`); the list is modelled, so keep a marker.
            if key == "minecraft:stored_enchantments"
                && data.stored_enchantments.is_empty()
                && matches!(v, Tag::Compound(_))
            {
                data.other
                    .push((key, Tag::compound(Vec::<(&str, Tag)>::new())));
            } else if !MODELLED.contains(&key.as_str()) && !ominous_part {
                let value = canonical_other_component(&key, v);
                data.other.push((key, value));
            }
        }
        data.other.sort_by(|a, b| a.0.cmp(&b.0));
    }
    (damage, (!data.is_empty()).then(|| Box::new(data)))
}

/// The `[id=value,...]` component list of an item argument (`ItemParser`)
/// as a `components` compound, retaining `!id` removal patches.
pub fn command_components(text: &str) -> Result<Tag, String> {
    let inner = text
        .strip_prefix('[')
        .and_then(|t| t.strip_suffix(']'))
        .ok_or("expected '['")?;
    let mut entries = horizon_world::nbt::Compound::new();
    let mut rest = inner.trim_start();
    while !rest.is_empty() {
        let end = rest.find(['=', ',']).unwrap_or(rest.len());
        let key = rest[..end].trim();
        rest = &rest[end..];
        if let Some(value) = rest.strip_prefix('=') {
            let (tag, used) = horizon_world::nbt::parse_snbt(value)?;
            let id = if key.contains(':') {
                key.to_owned()
            } else {
                format!("minecraft:{key}")
            };
            entries.insert(JavaString::from(id.as_str()), tag);
            rest = &value[used..];
        } else if let Some(key) = key.strip_prefix('!') {
            let id = if key.contains(':') {
                format!("!{key}")
            } else {
                format!("!minecraft:{key}")
            };
            entries.insert(
                JavaString::from(id.as_str()),
                Tag::compound([] as [(&str, Tag); 0]),
            );
        } else {
            return Err("expected '='".to_string());
        }
        rest = rest.trim_start();
        rest = rest.strip_prefix(',').unwrap_or(rest).trim_start();
    }
    Ok(Tag::Compound(entries))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::banner_layer::BannerLayer;

    /// `set_banner_pattern` (and `set_components`) hand a banner's layers over
    /// as a component; the renderer and the tooltip read `banner_patterns`, so
    /// they have to come out of it.
    #[test]
    fn a_loot_banner_patterns_component_dresses_the_banner() {
        let mut stack = ItemStack::new("minecraft:white_banner", 1);
        stack.data_mut().set_loot_component(
            "minecraft:banner_patterns",
            &serde_json::json!([
                {"pattern": "minecraft:stripe_bottom", "color": "red"},
                {"pattern": "minecraft:border", "color": "blue"},
            ]),
        );
        let layers = &stack.data().expect("a component box").banner_patterns;
        assert_eq!(
            layers,
            &[
                BannerLayer::new("minecraft:stripe_bottom", "red"),
                BannerLayer::new("minecraft:border", "blue"),
            ]
        );
        // A null component takes them off again.
        stack
            .data_mut()
            .set_loot_component("minecraft:banner_patterns", &serde_json::Value::Null);
        assert!(stack.data().is_none_or(|d| d.banner_patterns.is_empty()));
    }

    /// `set_book_cover` (and `set_components`) hand a written book's content
    /// over as a component; the fields the tooltip and the reading screen read
    /// have to come out of it, not just sit in `other` unread.
    #[test]
    fn a_loot_written_book_component_fills_the_book() {
        let mut stack = ItemStack::new("minecraft:written_book", 1);
        stack.data_mut().set_loot_component(
            "minecraft:written_book_content",
            &serde_json::json!({
                "title": {"raw": "A Guide"},
                "author": "Cartographer",
                "generation": 2,
                "pages": [{"raw": "page one"}, {"raw": "page two"}],
            }),
        );
        let book = stack
            .data()
            .and_then(|d| d.written.as_ref())
            .expect("content");
        assert_eq!(book.title, "A Guide");
        assert_eq!(book.author, "Cartographer");
        assert_eq!(book.generation, 2);
        assert_eq!(book.pages, ["page one", "page two"]);
        // A null component is no component.
        stack
            .data_mut()
            .set_loot_component("minecraft:written_book_content", &serde_json::Value::Null);
        assert!(stack.data().is_none_or(|d| d.written.is_none()));
    }

    #[test]
    fn an_empty_enchanted_book_patch_is_elided_only_when_saved() {
        let stack = parse_stack(r#"{id:"minecraft:enchanted_book",count:1,components:{"minecraft:stored_enchantments":{}}}"#)
            .unwrap().unwrap();
        let live = components_tag(stack.damage, stack.data());
        assert!(
            live.as_ref()
                .unwrap()
                .get("minecraft:stored_enchantments")
                .is_some()
        );
        assert_eq!(
            crate::item_nbt::item_to_nbt(&stack),
            horizon_world::nbt::parse_snbt(r#"{id:"minecraft:enchanted_book",count:1}"#)
                .unwrap()
                .0,
        );
        assert_eq!(
            components_tag(stack.damage, stack.data()),
            live,
            "saving must not remove live presence"
        );
    }

    #[test]
    fn nondefault_stored_enchantment_patches_are_preserved() {
        for (item, patch) in [
            ("minecraft:book", r#"{"minecraft:stored_enchantments":{}}"#),
            ("minecraft:stone", r#"{"minecraft:stored_enchantments":{}}"#),
            (
                "minecraft:enchanted_book",
                r#"{"minecraft:stored_enchantments":{"minecraft:sharpness":1}}"#,
            ),
            (
                "minecraft:enchanted_book",
                r#"{"!minecraft:stored_enchantments":{}}"#,
            ),
        ] {
            let expected = horizon_world::nbt::parse_snbt(patch).unwrap().0;
            let stack = parse_stack(&format!("{{id:'{item}',count:1,components:{patch}}}"))
                .unwrap()
                .unwrap();
            assert_eq!(encode_components(&stack), Some(expected), "{item}: {patch}");
        }
    }

    #[test]
    fn an_empty_book_patch_keeps_unrelated_components() {
        let stack = parse_stack(r#"{id:"minecraft:enchanted_book",count:1,components:{"minecraft:stored_enchantments":{},"minecraft:custom_data":{keep:7}}}"#).unwrap().unwrap();
        let expected = horizon_world::nbt::parse_snbt(r#"{"minecraft:custom_data":{keep:7}}"#)
            .unwrap()
            .0;
        assert_eq!(encode_components(&stack), Some(expected));
    }

    #[test]
    fn inline_trim_is_modeled_and_saved_without_raw_fallback() {
        let source = horizon_world::nbt::parse_snbt(r#"{id:"minecraft:diamond_chestplate",count:1,components:{"minecraft:custom_data":{keep:284},"minecraft:trim":{material:{palette_id:"minecraft:gold",description:"Inline material"},pattern:{asset_id:"minecraft:coast",description:"Inline pattern",decal:0b}}}}"#).unwrap().0;
        let decoded = crate::item_nbt::item_from_nbt(&source).unwrap();
        let data = decoded.data().unwrap();
        assert!(
            data.trim.is_some(),
            "inline trim must be adopted into the actual model"
        );
        assert!(data.other("minecraft:trim").is_none(), "not a raw fallback");
        let mut fresh = ItemStack::new("minecraft:diamond_chestplate", 1);
        fresh.data_mut().trim = data.trim.clone();
        fresh.data_mut().set_other(
            "minecraft:custom_data",
            Some(Tag::compound([("keep", Tag::Int(284))])),
        );
        for stack in [&decoded, &fresh] {
            assert_eq!(
                components_tag(0, stack.data()),
                source.get("components").cloned()
            );
            assert_eq!(crate::item_nbt::item_to_nbt(stack), source);
        }
    }

    fn check_lodestone_saved_default(decoded: bool) {
        const TRACKER: &str = "minecraft:lodestone_tracker";
        const CUSTOM: &str = "minecraft:custom_data";
        let target = Tag::compound([
            ("dimension", Tag::String("minecraft:overworld".into())),
            ("pos", Tag::IntArray(vec![12, 200, 12])),
        ]);
        let custom = Tag::compound([("keep", Tag::Int(17))]);
        assert_eq!(
            encode_components(&ItemStack::new("minecraft:compass", 1)),
            None
        );
        let mut only_custom = ItemStack::new("minecraft:compass", 1);
        only_custom
            .data_mut()
            .set_other(CUSTOM, Some(custom.clone()));
        assert_eq!(
            encode_components(&only_custom),
            Some(Tag::compound([(CUSTOM, custom.clone())]))
        );
        // The valid false and absent controls precede the old true-default failure.
        for (tracked, has_target) in [
            (Some(false), true),
            (None, true),
            (Some(true), true),
            (Some(true), false),
        ] {
            let mut fields = Vec::new();
            let mut expected_fields = Vec::new();
            if has_target {
                fields.push(("target", target.clone()));
                expected_fields.push(("target", target.clone()));
            }
            if let Some(value) = tracked {
                fields.push(("tracked", Tag::Byte(i8::from(value))));
                if !value {
                    expected_fields.push(("tracked", Tag::Byte(0)));
                }
            }
            let source = Tag::compound(fields);
            let input = Tag::compound([(TRACKER, source.clone()), (CUSTOM, custom.clone())]);
            let expected = Tag::compound([
                (TRACKER, Tag::compound(expected_fields)),
                (CUSTOM, custom.clone()),
            ]);
            let stack = if decoded {
                let (damage, extra) = decode_components(Some(&input));
                ItemStack {
                    damage,
                    extra,
                    ..ItemStack::new("minecraft:compass", 1)
                }
            } else {
                let mut fresh = ItemStack::new("minecraft:compass", 1);
                assert!(fresh.extra.is_none());
                fresh.data_mut().set_other(TRACKER, Some(source.clone()));
                fresh.data_mut().set_other(CUSTOM, Some(custom.clone()));
                fresh
            };
            {
                assert_eq!(stack.data().unwrap().other(TRACKER), Some(&source));
                assert_eq!(encode_components(&stack), Some(expected.clone()));
                assert_eq!(components_tag(0, stack.data()), Some(input.clone()));
                let saved = crate::item_nbt::item_to_nbt(&stack);
                assert_eq!(saved.get("components"), Some(&expected));
                assert_eq!(
                    saved.get("id"),
                    Some(&Tag::String("minecraft:compass".into()))
                );
                assert_eq!(saved.get("count"), Some(&Tag::Int(1)));
                assert_eq!(stack.data().unwrap().other(TRACKER), Some(&source));
            }
        }
        let mut targetless = ItemStack::new("minecraft:compass", 1);
        targetless
            .data_mut()
            .set_other(TRACKER, Some(Tag::compound([("tracked", Tag::Byte(1))])));
        assert_eq!(
            encode_components(&targetless),
            Some(Tag::compound([(
                TRACKER,
                Tag::Compound(Default::default())
            )]))
        );
    }

    #[test]
    fn lodestone_tracker_saved_default_decoded() {
        check_lodestone_saved_default(true);
    }

    #[test]
    fn lodestone_tracker_saved_default_fresh_model() {
        check_lodestone_saved_default(false);
    }

    #[test]
    fn a_stack_reads_from_its_compound_as_from_snbt() {
        let snbt = r#"{id:"diamond_sword",count:2,components:{"minecraft:damage":5}}"#;
        let Tag::Compound(compound) = horizon_world::nbt::parse_snbt(snbt).unwrap().0 else {
            panic!("a compound");
        };
        let stack = ItemStack::from_nbt(&compound).unwrap();
        assert_eq!(&*stack.item, "minecraft:diamond_sword");
        assert_eq!((stack.count, stack.damage), (2, 5));
        assert_eq!(parse_stack(snbt).unwrap(), Some(stack));
        let Tag::Compound(air) = horizon_world::nbt::parse_snbt(r#"{id:"air"}"#).unwrap().0 else {
            panic!("a compound");
        };
        assert_eq!(ItemStack::from_nbt(&air), None);
    }

    #[test]
    fn numeric_damage_decodes_and_saves_as_a_modelled_int() {
        let keep = Tag::compound([("keep", Tag::Int(87))]);
        for (raw, expected) in [
            (None, 0),
            (Some(Tag::Byte(1)), 1),
            (Some(Tag::Short(1)), 1),
            (Some(Tag::Int(1)), 1),
            (Some(Tag::Long(4_294_967_297)), 1),
            (Some(Tag::Float(1.5_f32.to_bits())), 1),
            (Some(Tag::Double(2.75_f64.to_bits())), 2),
            (Some(Tag::Float((-0.5_f32).to_bits())), 0),
            (
                Some(Tag::Float(2_147_483_648_f32.to_bits())),
                i32::MAX as u32,
            ),
            (Some(Tag::Double(1.0e20_f64.to_bits())), i32::MAX as u32),
        ] {
            let mut entries = vec![("minecraft:custom_data", keep.clone())];
            if let Some(raw) = &raw {
                entries.push(("minecraft:damage", raw.clone()));
            }
            let (damage, extra) = decode_components(Some(&Tag::compound(entries)));
            let loaded = ItemStack {
                damage,
                extra,
                ..ItemStack::new("minecraft:diamond_pickaxe", 1)
            };
            assert_eq!(loaded.damage, expected, "{raw:?}");
            assert!(loaded.wire_patch().is_none());
            assert!(loaded.data().unwrap().other("minecraft:damage").is_none());
            let saved = crate::item_nbt::item_to_nbt(&loaded);
            assert_eq!(
                saved
                    .get("components")
                    .and_then(|c| c.get("minecraft:damage")),
                (expected > 0).then_some(&Tag::Int(expected as i32)),
                "{raw:?}"
            );
            assert_eq!(
                saved
                    .get("components")
                    .unwrap()
                    .get("minecraft:custom_data"),
                Some(&keep)
            );
            // No retained source or wire patch: only modelled damage is saved.
            let mut fresh = ItemStack::new("minecraft:diamond_pickaxe", 1);
            fresh.damage = expected;
            fresh
                .data_mut()
                .set_other("minecraft:custom_data", Some(keep.clone()));
            assert_eq!(crate::item_nbt::item_to_nbt(&fresh), saved);
        }
    }

    #[test]
    fn potion_custom_effect_icons_save_the_codec_default() {
        let parse = |text: &str| horizon_world::nbt::parse_snbt(text).unwrap().0;
        for (flags, icon) in [
            ("", 1),
            (",show_icon:0b", 0),
            (",show_icon:1b", 1),
            (",show_particles:0b", 0),
            (",show_particles:0b,show_icon:0b", 0),
            (",show_particles:0b,show_icon:1b,ambient:1b", 1),
        ] {
            let contents = |suffix: &str| {
                format!(
                    r#"{{potion:"minecraft:water",custom_color:123456,custom_effects:[{{id:"minecraft:speed",duration:100,amplifier:2{suffix}}}]}}"#
                )
            };
            let input = parse(&format!(
                r#"{{"minecraft:potion_contents":{},"minecraft:custom_data":{{keep:7}}}}"#,
                contents(flags)
            ));
            let expected = parse(&format!(
                r#"{{"minecraft:potion_contents":{},"minecraft:custom_data":{{keep:7}}}}"#,
                contents(&format!("{flags},show_icon:{icon}b"))
            ));
            let (damage, extra) = decode_components(Some(&input));
            let decoded = ItemStack {
                damage,
                extra,
                ..ItemStack::new("minecraft:potion", 1)
            };
            assert_eq!(
                encode_components(&decoded),
                Some(expected.clone()),
                "decoded {flags}"
            );
            // Build a model directly, without decoded input or a retained raw patch.
            let mut fresh = ItemStack::new("minecraft:potion", 1);
            fresh.data_mut().potion = Some(Arc::from("minecraft:water"));
            let potion = input.get("minecraft:potion_contents").unwrap();
            for name in ["custom_color", "custom_effects"] {
                fresh
                    .data_mut()
                    .potion_extra
                    .push((name.into(), potion.get(name).unwrap().clone()));
            }
            fresh.data_mut().set_other(
                "minecraft:custom_data",
                input.get("minecraft:custom_data").cloned(),
            );
            assert_eq!(encode_components(&fresh), Some(expected), "modeled {flags}");
        }
        assert!(encode_components(&ItemStack::new("minecraft:potion", 1)).is_none());
        let mut base = ItemStack::new("minecraft:potion", 1);
        base.data_mut().potion = Some(Arc::from("minecraft:water"));
        assert_eq!(
            encode_components(&base),
            Some(parse(
                r#"{"minecraft:potion_contents":{potion:"minecraft:water"}}"#
            ))
        );
    }

    #[test]
    fn map_decoration_types_round_trip_with_default_namespaces() {
        let parse = |text| horizon_world::nbt::parse_snbt(text).unwrap().0;
        let source = parse(
            r#"{"minecraft:map_decorations":{a:{type:"minecraft:mansion",x:10.5d,z:-20.0d,rotation:90.0f},"a+b":{type:"red_x",x:-1.25d,z:2.75d,rotation:-45.5f}},"minecraft:custom_data":{keep:7}}"#,
        );
        let expected = parse(
            r#"{"minecraft:map_decorations":{a:{type:"minecraft:mansion",x:10.5d,z:-20.0d,rotation:90.0f},"a+b":{type:"minecraft:red_x",x:-1.25d,z:2.75d,rotation:-45.5f}},"minecraft:custom_data":{keep:7}}"#,
        );
        assert!(decode_components(None).1.is_none());
        for input in [&source, &expected] {
            let (damage, extra) = decode_components(Some(input));
            assert_eq!(
                extra.as_ref().unwrap().other("minecraft:map_decorations"),
                expected.get("minecraft:map_decorations")
            );
            let loaded = ItemStack {
                damage,
                extra,
                ..ItemStack::new("minecraft:filled_map", 1)
            };
            assert_eq!(encode_components(&loaded), Some(expected.clone()));
            // A fresh model has no original NBT or retained component source.
            let mut fresh = ItemStack::new("minecraft:filled_map", 1);
            assert!(fresh.extra.is_none());
            for id in ["minecraft:map_decorations", "minecraft:custom_data"] {
                fresh.data_mut().set_other(id, input.get(id).cloned());
            }
            assert_eq!(encode_components(&fresh), Some(expected.clone()));
            assert!(fresh.data().unwrap().map_id.is_none());
        }
    }

    #[test]
    fn empty_map_decoration_patch_uses_filled_map_prototype() {
        let parse = |text| horizon_world::nbt::parse_snbt(text).unwrap().0;
        let empty = parse(r#"{"minecraft:map_decorations":{}}"#);
        assert!(decode_components(None).1.is_none());
        assert!(encode_components(&ItemStack::new("minecraft:filled_map", 1)).is_none());
        let mut fresh = ItemStack::new("minecraft:filled_map", 1);
        fresh
            .data_mut()
            .other
            .push(("minecraft:map_decorations".into(), parse("{}")));
        // The value codec has no item prototype; only the saved patch elides it.
        assert_eq!(components_tag(0, fresh.data()), Some(empty.clone()));
        assert!(encode_components(&fresh).is_none());
        let (damage, extra) = decode_components(Some(&empty));
        let loaded = ItemStack {
            damage,
            extra,
            ..ItemStack::new("minecraft:filled_map", 1)
        };
        let saved = crate::item_nbt::item_to_nbt(&loaded);
        assert_eq!(saved, parse(r#"{id:"minecraft:filled_map",count:1}"#));
        let back = crate::item_nbt::item_from_nbt(&saved).unwrap();
        assert!(encode_components(&back).is_none());
        let keep = parse(r#"{"minecraft:map_decorations":{},"minecraft:custom_data":{keep:7}}"#);
        let (damage, extra) = decode_components(Some(&keep));
        let stack = ItemStack {
            damage,
            extra,
            ..ItemStack::new("minecraft:filled_map", 1)
        };
        assert_eq!(
            encode_components(&stack),
            Some(parse(r#"{"minecraft:custom_data":{keep:7}}"#))
        );
    }

    #[test]
    fn nondefault_map_decoration_patches_round_trip() {
        let parse = |text| horizon_world::nbt::parse_snbt(text).unwrap().0;
        let nonempty = parse(
            r#"{"minecraft:map_decorations":{"a+b":{type:"minecraft:red_x",x:-1.25d,z:2.75d,rotation:-45.5f}},"minecraft:custom_data":{keep:7}}"#,
        );
        let removed =
            parse(r#"{"!minecraft:map_decorations":{},"minecraft:custom_data":{keep:7}}"#);
        let empty = parse(r#"{"minecraft:map_decorations":{}}"#);
        for (item, components) in [
            ("minecraft:filled_map", &nonempty),
            ("minecraft:stone", &nonempty),
            ("minecraft:filled_map", &removed),
            ("minecraft:stone", &empty),
            ("minecraft:map", &empty),
        ] {
            let (damage, extra) = decode_components(Some(components));
            let stack = ItemStack {
                damage,
                extra,
                ..ItemStack::new(item, 1)
            };
            assert_eq!(
                encode_components(&stack),
                Some(components.clone()),
                "{item}"
            );
            let back =
                crate::item_nbt::item_from_nbt(&crate::item_nbt::item_to_nbt(&stack)).unwrap();
            assert_eq!(encode_components(&back), Some(components.clone()), "{item}");
        }
    }

    fn check_hive_saved_patch(decoded: bool) {
        let parse = |text| horizon_world::nbt::parse_snbt(text).unwrap().0;
        let zero = parse(r#"{"minecraft:block_state":{honey_level:"0"}}"#);
        let keep = parse(
            r#"{"minecraft:block_state":{honey_level:"0"},"minecraft:custom_data":{keep:34}}"#,
        );
        let custom = parse(r#"{"minecraft:custom_data":{keep:34}}"#);
        let one = parse(r#"{"minecraft:block_state":{honey_level:"1"}}"#);
        let empty = parse(r#"{"minecraft:block_state":{}}"#);
        let extra = parse(r#"{"minecraft:block_state":{honey_level:"0",facing:"south"}}"#);
        let removed = parse(r#"{"!minecraft:block_state":{},"minecraft:custom_data":{keep:34}}"#);
        for item in ["minecraft:beehive", "minecraft:bee_nest", "minecraft:stone"] {
            assert!(encode_components(&ItemStack::new(item, 1)).is_none());
            for source in [&zero, &keep, &one, &empty, &extra, &removed] {
                let mut stack = ItemStack::new(item, 1);
                if decoded {
                    (stack.damage, stack.extra) = decode_components(Some(source));
                } else {
                    // A fresh model, without retained source NBT.
                    assert!(stack.extra.is_none());
                    let Tag::Compound(entries) = source else {
                        unreachable!()
                    };
                    for (id, value) in entries {
                        stack
                            .data_mut()
                            .set_other(&id.to_string_lossy(), Some(value.clone()));
                    }
                }
                let before = stack.data().cloned();
                assert_eq!(
                    components_tag(stack.damage, stack.data()),
                    Some(source.clone())
                );
                let saved = encode_components(&stack);
                // Saved patch canonicalization must not change live/unsaved values.
                assert_eq!(stack.data(), before.as_ref());
                assert_eq!(
                    components_tag(stack.damage, stack.data()),
                    Some(source.clone())
                );
                let expected = if item != "minecraft:stone" && source == &zero {
                    None
                } else if item != "minecraft:stone" && source == &keep {
                    Some(custom.clone())
                } else {
                    Some(source.clone())
                };
                assert_eq!(saved, expected, "{item}, decoded={decoded}, {source:?}");
                let tag = crate::item_nbt::item_to_nbt(&stack);
                assert_eq!(tag.get("components"), expected.as_ref());
                let back = crate::item_nbt::item_from_nbt(&tag).unwrap();
                assert_eq!(encode_components(&back), expected);
            }
        }
    }

    fn check_max_stack_saved_prototype(decoded: bool) {
        use crate::items::Items;
        use horizon_world::loot::PackOverlay;
        let root =
            std::env::temp_dir().join(format!("h088-prototypes-{}-{decoded}", std::process::id()));
        std::fs::create_dir_all(root.join("data/minecraft/loot_table")).unwrap();
        std::fs::write(root.join("items.json"), r#"{"minecraft:iron_helmet":{"minecraft:max_stack_size":1},"minecraft:stone":{"minecraft:max_stack_size":64},"minecraft:ender_pearl":{"minecraft:max_stack_size":16},"audit:changed":{"minecraft:max_stack_size":7},"audit:missing":{}}"#).unwrap();
        let items = Items::catalog(&root, &PackOverlay::NONE).unwrap();
        let parse = |text| horizon_world::nbt::parse_snbt(text).unwrap().0;
        for (id, prototype, has_default) in [
            ("minecraft:iron_helmet", 1, true),
            ("minecraft:stone", 64, true),
            ("minecraft:ender_pearl", 16, true),
            ("audit:changed", 7, true),
            ("audit:missing", 64, false),
            ("audit:unknown", 7, false),
        ] {
            assert!(encode_components_with_items(&ItemStack::new(id, 1), &items).is_none());
            for (key, value) in [
                ("minecraft:max_stack_size", Tag::Int(prototype)),
                ("minecraft:max_stack_size", Tag::Int(3)),
                ("!minecraft:max_stack_size", parse("{}")),
            ] {
                let input = Tag::compound([
                    (key, value.clone()),
                    ("minecraft:custom_data", parse("{keep:88}")),
                ]);
                let mut stack = ItemStack::new(id, 1);
                if decoded {
                    (stack.damage, stack.extra) = decode_components(Some(&input));
                } else {
                    assert!(stack.extra.is_none());
                    stack.data_mut().set_other(key, Some(value.clone()));
                    stack
                        .data_mut()
                        .set_other("minecraft:custom_data", Some(parse("{keep:88}")));
                }
                let before = stack.clone();
                assert_eq!(
                    components_tag(stack.damage, stack.data()),
                    Some(input.clone())
                );
                assert_eq!(
                    encode_components(&stack),
                    Some(input.clone()),
                    "context-free API stays unchanged"
                );
                let default = key == "minecraft:max_stack_size"
                    && value == Tag::Int(prototype)
                    && has_default;
                let expected = if default {
                    Some(parse(r#"{"minecraft:custom_data":{keep:88}}"#))
                } else {
                    Some(input.clone())
                };
                assert_eq!(
                    encode_components_with_items(&stack, &items),
                    expected,
                    "{id}, decoded={decoded}"
                );
                assert_eq!(
                    crate::item_nbt::item_to_nbt_with_items(&stack, &items).get("components"),
                    expected.as_ref()
                );
                assert_eq!(stack, before, "no live stack mutation");
                assert_eq!(components_tag(stack.damage, stack.data()), Some(input));
            }
            // Removing a sole default must also omit the empty components compound.
            let mut only = ItemStack::new(id, 1);
            only.data_mut()
                .set_other("minecraft:max_stack_size", Some(Tag::Int(prototype)));
            assert_eq!(
                encode_components_with_items(&only, &items).is_none(),
                has_default
            );
        }
        // Reload the same known item's prototype through the real catalog loader.
        std::fs::write(
            root.join("items.json"),
            r#"{"minecraft:stone":{"minecraft:max_stack_size":7}}"#,
        )
        .unwrap();
        let changed = Items::catalog(&root, &PackOverlay::NONE).unwrap();
        assert_eq!(items.get("minecraft:stone").unwrap().max_stack, 64);
        assert_eq!(changed.get("minecraft:stone").unwrap().max_stack, 7);
        for size in [7, 64] {
            let mut stack = ItemStack::new("minecraft:stone", 1);
            stack
                .data_mut()
                .set_other("minecraft:max_stack_size", Some(Tag::Int(size)));
            assert_eq!(
                encode_components_with_items(&stack, &items).is_none(),
                size == 64
            );
            assert_eq!(
                encode_components_with_items(&stack, &changed).is_none(),
                size == 7
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn decoded_max_stack_saved_patch_uses_loaded_prototype() {
        check_max_stack_saved_prototype(true);
    }
    #[test]
    fn modeled_max_stack_saved_patch_uses_loaded_prototype() {
        check_max_stack_saved_prototype(false);
    }
    #[test]
    fn decoded_hive_saved_patch_uses_whole_prototype() {
        check_hive_saved_patch(true);
    }

    #[test]
    fn fresh_hive_saved_patch_uses_whole_prototype() {
        check_hive_saved_patch(false);
    }

    #[test]
    fn an_absent_enchantments_component_is_not_enchantable() {
        let mut stack = ItemStack::new("minecraft:diamond_sword", 1);
        stack.data_mut().set_other(
            "!minecraft:enchantments",
            Some(horizon_world::nbt::Tag::compound(Vec::<(
                &str,
                horizon_world::nbt::Tag,
            )>::new())),
        );
        stack.enchant("minecraft:sharpness", 1);
        assert!(stack.enchantments().is_empty());
        let mut book = ItemStack::new("minecraft:enchanted_book", 1);
        book.data_mut().set_other(
            "!minecraft:stored_enchantments",
            Some(horizon_world::nbt::Tag::compound(Vec::<(
                &str,
                horizon_world::nbt::Tag,
            )>::new())),
        );
        book.enchant("minecraft:sharpness", 1);
        assert!(book.stored_enchantments().is_empty());
    }

    #[test]
    fn components_round_trip() {
        let mut stack = ItemStack::new("minecraft:diamond_sword", 1);
        stack.damage = 12;
        stack.enchant("minecraft:sharpness", 3);
        stack.enchant("minecraft:unbreaking", 2);
        stack.data_mut().repair_cost = 3;
        stack.data_mut().custom_name = Some(Tag::String("Blade".into()));
        let tag = encode_components(&stack).expect("components");
        let (damage, extra) = decode_components(Some(&tag));
        assert_eq!(damage, 12);
        let back = ItemStack {
            damage,
            extra,
            ..ItemStack::new("minecraft:diamond_sword", 1)
        };
        assert_eq!(back.enchantment_level("minecraft:sharpness"), 3);
        assert_eq!(back.enchantment_level("minecraft:unbreaking"), 2);
        assert_eq!(back.repair_cost(), 3);
        assert_eq!(back.custom_name().as_deref(), Some("Blade"));
    }

    #[test]
    fn enchantment_ids_without_a_namespace_are_minecraft_ones() {
        let levels = Tag::compound([
            ("knockback", Tag::Int(2)),
            ("minecraft:sharpness", Tag::Int(1)),
        ]);
        let components = Tag::compound([("minecraft:enchantments", levels)]);
        let (_, extra) = decode_components(Some(&components));
        let stack = ItemStack {
            extra,
            ..ItemStack::new("minecraft:wooden_sword", 1)
        };
        assert_eq!(stack.enchantment_level("minecraft:knockback"), 2);
        assert_eq!(stack.enchantment_level("minecraft:sharpness"), 1);
    }

    #[test]
    fn invalid_enchantment_levels_leave_the_other_entries_intact() {
        for component in ["minecraft:enchantments", "minecraft:stored_enchantments"] {
            for level in [-1, 0, 256, i32::MAX] {
                let components = Tag::compound([
                    ("minecraft:damage", Tag::Int(12)),
                    (
                        component,
                        Tag::compound([("looting", Tag::Int(level)), ("sharpness", Tag::Int(3))]),
                    ),
                ]);
                let (damage, extra) = decode_components(Some(&components));
                let data = extra.unwrap();
                let levels = if component == "minecraft:enchantments" {
                    data.enchantments
                } else {
                    data.stored_enchantments
                };
                assert_eq!(damage, 12);
                assert_eq!(levels, vec![(Arc::from("minecraft:sharpness"), 3)]);
            }
        }
    }

    #[test]
    fn custom_name_reads_the_extra_spans() {
        let name = Tag::compound([
            (
                "extra",
                Tag::List(vec![
                    Tag::compound([("text", Tag::String("Games ".into()))]),
                    Tag::compound([("text", Tag::String("Right click".into()))]),
                ]),
            ),
            ("text", Tag::String("".into())),
        ]);
        let components = Tag::compound([("minecraft:custom_name", name)]);
        let (_, extra) = decode_components(Some(&components));
        assert_eq!(
            extra.and_then(|d| d.custom_name_plain()).as_deref(),
            Some("Games Right click")
        );
    }

    #[test]
    fn plain_custom_name_is_saved_as_a_string() {
        // Component.tryCollapseToString: `{text:"Cool"}` is written "Cool";
        // a style or a sibling keeps the compound.
        let decode = |name: Tag| {
            let components = Tag::compound([("minecraft:custom_name", name)]);
            decode_components(Some(&components)).1.unwrap().custom_name
        };
        let plain = Tag::compound([("text", Tag::String("Cool".into()))]);
        assert_eq!(decode(plain), Some(Tag::String("Cool".into())));
        let styled = Tag::compound([
            ("text", Tag::String("Cool".into())),
            ("color", Tag::String("red".into())),
        ]);
        assert_eq!(decode(styled.clone()), Some(styled));
        let mut data = StackData::default();
        data.set_other(
            "minecraft:custom_name",
            Some(Tag::compound([("text", Tag::String("Loot".into()))])),
        );
        assert_eq!(data.custom_name, Some(Tag::String("Loot".into())));
    }

    #[test]
    fn rich_custom_name_is_the_single_saved_value_after_rename() {
        let name = Tag::compound([
            ("text", Tag::String("".into())),
            (
                "extra",
                Tag::List(vec![
                    Tag::compound([
                        ("text", Tag::String("A".into())),
                        ("color", Tag::String("#ff0000".into())),
                    ]),
                    Tag::compound([
                        ("text", Tag::String("B".into())),
                        ("color", Tag::String("#0000ff".into())),
                    ]),
                ]),
            ),
        ]);
        let components = Tag::compound([("minecraft:custom_name", name.clone())]);
        let (_, extra) = decode_components(Some(&components));
        let mut stack = ItemStack {
            extra,
            ..ItemStack::new("minecraft:stick", 1)
        };
        assert_eq!(stack.custom_name().as_deref(), Some("AB"));
        assert_eq!(encode_components(&stack), Some(components));
        stack
            .data_mut()
            .set_other("minecraft:custom_name", Some(Tag::String("Changed".into())));
        assert_eq!(stack.custom_name().as_deref(), Some("Changed"));
        assert_eq!(
            encode_components(&stack)
                .unwrap()
                .get("minecraft:custom_name"),
            Some(&Tag::String("Changed".into()))
        );
    }

    #[test]
    fn loot_name_preserves_plain_and_styled_components() {
        let mut stack = ItemStack::new("minecraft:stone", 1);
        stack
            .data_mut()
            .set_loot_name("minecraft:custom_name", &serde_json::json!("Rock"));
        assert_eq!(
            encode_components(&stack)
                .unwrap()
                .get("minecraft:custom_name"),
            Some(&Tag::String("Rock".into()))
        );
        let styled = serde_json::json!({"text": "Rock", "color": "red"});
        stack
            .data_mut()
            .set_loot_name("minecraft:custom_name", &styled);
        assert_eq!(
            encode_components(&stack)
                .unwrap()
                .get("minecraft:custom_name"),
            Some(&horizon_client_core::scoreboard::json_to_tag(&styled))
        );
    }

    #[test]
    fn loot_component_keeps_a_styled_custom_name() {
        let name = serde_json::json!({"text":"Prize","color":"#ff8800"});
        let mut stack = ItemStack::new("minecraft:stick", 1);
        stack
            .data_mut()
            .set_loot_component("minecraft:custom_name", &name);
        assert_eq!(stack.custom_name().as_deref(), Some("Prize"));
        let expected = horizon_client_core::scoreboard::json_to_tag(&name);
        assert_eq!(
            encode_components(&stack)
                .unwrap()
                .get("minecraft:custom_name"),
            Some(&expected)
        );
        stack
            .data_mut()
            .set_loot_component("minecraft:custom_name", &serde_json::Value::Null);
        assert!(stack.custom_name().is_none());
    }

    /// A chest's `set_components` patch: a potion's contents land where
    /// drinking and splashing read them, not only in its saved form, and the
    /// components Horizon keeps as saved stay so.
    #[test]
    fn a_loot_component_horizon_models_lands_in_its_field() {
        use serde_json::json;
        let mut swift = ItemStack::new("minecraft:splash_potion", 1);
        swift.data_mut().set_loot_component(
            "minecraft:potion_contents",
            &json!({"potion": "minecraft:swiftness"}),
        );
        assert_eq!(swift.potion(), Some("minecraft:swiftness"));
        let mut custom = ItemStack::new("minecraft:potion", 1);
        let data = custom.data_mut();
        data.set_loot_component("minecraft:custom_data", &json!({"page": 3}));
        data.set_loot_component("minecraft:damage", &json!(5));
        let effects = json!({"custom_effects": [{"id": "minecraft:resistance", "amplifier": 1, "duration": 1800}]});
        data.set_loot_component("minecraft:potion_contents", &effects);
        assert_eq!(
            custom.custom_potion_effects(),
            [("minecraft:resistance".to_owned(), 1800, 1)]
        );
        let data = custom.data().unwrap();
        assert!(data.other("minecraft:custom_data").is_some());
        assert!(
            data.other("minecraft:damage").is_some(),
            "a damage set before stays"
        );
        // Saved as before: the patch's contents, with the codec's icon flag.
        let saved = encode_components(&custom).unwrap();
        assert_eq!(
            horizon_client_core::scoreboard::tag_to_json(
                saved.get("minecraft:potion_contents").unwrap()
            ),
            json!({"custom_effects": [{"id": "minecraft:resistance", "amplifier": 1, "duration": 1800, "show_icon": true}]})
        );
        // A null value takes the potion away again.
        swift
            .data_mut()
            .set_loot_component("minecraft:potion_contents", &serde_json::Value::Null);
        assert_eq!(swift.potion(), None);
    }

    #[test]
    fn written_book_pages_may_be_component_lists() {
        // A page as Hypixel sends it through ViaVersion: a list, not a compound.
        let page = Tag::List(vec![
            Tag::compound([("text", Tag::String("".into()))]),
            Tag::compound([("text", Tag::String("Please read ".into()))]),
            Tag::compound([("text", Tag::String("the rules".into()))]),
        ]);
        let book = Tag::compound([
            (
                "title",
                Tag::compound([("raw", Tag::String("Rules".into()))]),
            ),
            ("author", Tag::String("Server".into())),
            (
                "pages",
                Tag::List(vec![
                    page,
                    Tag::compound([("raw", Tag::String("Two".into()))]),
                ]),
            ),
        ]);
        let components = Tag::compound([("minecraft:written_book_content", book)]);
        let (_, extra) = decode_components(Some(&components));
        let pages = extra.and_then(|d| d.written.clone()).map(|w| w.pages);
        assert_eq!(
            pages,
            Some(vec!["Please read the rules".to_owned(), "Two".to_owned()])
        );
    }

    #[test]
    fn banner_layers_and_shield_base_round_trip() {
        let mut stack = ItemStack::new("minecraft:shield", 1);
        stack.data_mut().base_color = Some(Arc::from("red"));
        stack.data_mut().banner_patterns = vec![
            crate::banner_layer::BannerLayer::new("minecraft:flower", "black"),
            crate::banner_layer::BannerLayer::new("minecraft:border", "white"),
        ];
        let tag = encode_components(&stack).expect("components");
        let (_, extra) = decode_components(Some(&tag));
        let extra = extra.expect("data");
        assert_eq!(extra.base_color.as_deref(), Some("red"));
        assert_eq!(extra.banner_patterns, stack.data().unwrap().banner_patterns);
    }

    #[test]
    fn ominous_items_round_trip() {
        let mut banner = ItemStack::new("minecraft:white_banner", 1);
        banner.data_mut().ominous_banner = true;
        let tag = encode_components(&banner).unwrap();
        assert!(tag.get("minecraft:banner_patterns").is_some());
        let (_, back) = decode_components(Some(&tag));
        let back = back.unwrap();
        assert!(back.ominous_banner);
        assert!(back.other("minecraft:tooltip_display").is_none());

        banner.data_mut().set_other(
            "minecraft:tooltip_display",
            Some(Tag::Compound(Default::default())),
        );
        let (_, custom_display) = decode_components(encode_components(&banner).as_ref());
        assert_eq!(
            custom_display.unwrap().other("minecraft:tooltip_display"),
            Some(&Tag::Compound(Default::default()))
        );
        let mut bottle = ItemStack::new("minecraft:ominous_bottle", 1);
        bottle.data_mut().ominous_amplifier = Some(3);
        let (_, back) = decode_components(encode_components(&bottle).as_ref());
        assert_eq!(back.unwrap().ominous_amplifier, Some(3));
    }
    #[test]
    fn plain_stacks_have_no_components() {
        let stack = ItemStack::new("minecraft:stone", 64);
        assert!(encode_components(&stack).is_none());
        let mut named = stack.clone();
        named.data_mut();
        named.tidy();
        assert_eq!(named, stack);
    }

    #[test]
    fn books_store_enchantments() {
        let mut book = ItemStack::new("minecraft:enchanted_book", 1);
        book.enchant("minecraft:mending", 1);
        assert!(book.enchantments().is_empty());
        assert_eq!(book.stored_enchantments().len(), 1);
        upgrade(
            &mut book.data_mut().stored_enchantments,
            "minecraft:mending",
            1,
        );
        assert_eq!(book.crafting_enchantments()[0].1, 1);
        let book = ItemStack::new("minecraft:book", 1).transmute("minecraft:enchanted_book", 1);
        assert!(book.extra.is_none());
    }
    #[test]
    fn command_components_decode_fireworks() {
        let tag = command_components(
            "[fireworks={flight_duration:2,explosions:[{shape:large_ball,colors:[I;16711680],has_twinkle:true}]},damage=3]",
        )
        .unwrap();
        let (damage, data) = decode_components(Some(&tag));
        assert_eq!(damage, 3);
        let (flight, explosions) = data.unwrap().fireworks.unwrap();
        assert_eq!(flight, 2);
        assert_eq!(
            explosions[0].shape,
            crate::firework::FireworkShape::LargeBall
        );
        assert_eq!(explosions[0].colors, vec![0xFF0000]);
        assert!(explosions[0].twinkle && !explosions[0].trail);
    }

    #[test]
    fn command_components_rejects_bare_component_without_equals() {
        assert_eq!(
            command_components("[custom_data]"),
            Err("expected '='".to_string())
        );
        assert_eq!(
            command_components("[!tool,custom_data]"),
            Err("expected '='".to_string())
        );
    }

    #[test]
    fn debug_stick_state_round_trips() {
        let mut stack = ItemStack::new("minecraft:debug_stick", 1);
        stack
            .data_mut()
            .set_debug_stick_property("minecraft:oak_stairs", "facing");
        stack
            .data_mut()
            .set_debug_stick_property("minecraft:oak_stairs", "half");
        stack
            .data_mut()
            .set_debug_stick_property("minecraft:stone", "axis");

        let tag = encode_components(&stack).expect("debug stick components");
        let (_, extra) = decode_components(Some(&tag));
        let data = extra.expect("debug stick state");
        assert_eq!(
            data.debug_stick_property("minecraft:oak_stairs").as_deref(),
            Some("half")
        );
        assert_eq!(
            data.debug_stick_property("minecraft:stone").as_deref(),
            Some("axis")
        );
        assert_eq!(data.debug_stick_property("minecraft:dirt"), None);
    }

    /// The modelled component IDs, as `decode_components` read them before the
    /// single pass: one `BTreeMap` lookup per ID, whether or not the item
    /// carries it.
    fn baseline_decode_components(components: Option<&Tag>) -> (u32, Option<Box<StackData>>) {
        let Some(c) = components else {
            return (0, None);
        };
        let damage =
            horizon_world::datafix::dynamic::as_int(c.get("minecraft:damage"), 0).max(0) as u32;
        let potion = match c.get("minecraft:potion_contents") {
            Some(Tag::String(s)) => Some(Arc::from(s.to_string_lossy().as_str())),
            Some(t) => t
                .get("potion")
                .and_then(string)
                .map(|s| Arc::from(s.as_str())),
            None => None,
        };
        let potion_extra = match c.get("minecraft:potion_contents") {
            Some(Tag::Compound(map)) => map
                .iter()
                .filter(|(k, _)| k.to_string_lossy() != "potion")
                .map(|(k, v)| (k.to_string_lossy(), v.clone()))
                .collect(),
            _ => Vec::new(),
        };
        let mut data = StackData {
            enchantments: read_levels(c.get("minecraft:enchantments")),
            stored_enchantments: read_levels(c.get("minecraft:stored_enchantments")),
            repair_cost: c
                .get("minecraft:repair_cost")
                .and_then(int)
                .unwrap_or(0)
                .max(0) as u32,
            explicit_zero_damage: damage == 0 && c.get("minecraft:damage").is_some(),
            custom_name: c
                .get("minecraft:custom_name")
                .filter(|tag| read_text(tag).is_some())
                .cloned()
                .map(collapsed_text),
            potion,
            potion_extra,
            map_id: c.get("minecraft:map_id").and_then(int).map(|v| v as i32),
            banner_patterns: crate::banner_layer::layers_from_nbt(
                c.get("minecraft:banner_patterns"),
            ),
            base_color: c
                .get("minecraft:base_color")
                .and_then(string)
                .map(|s| Arc::from(s.as_str())),
            writable_pages: match c
                .get("minecraft:writable_book_content")
                .and_then(|b| b.get("pages"))
            {
                Some(Tag::List(pages)) => pages.iter().filter_map(filterable_text).collect(),
                _ => Vec::new(),
            },
            written: c
                .get("minecraft:written_book_content")
                .map(written_book_from_tag),
            fireworks: c
                .get("minecraft:fireworks")
                .map(|f| {
                    let flight = f
                        .get("flight_duration")
                        .and_then(int)
                        .unwrap_or(0)
                        .clamp(0, 255) as u8;
                    let explosions = match f.get("explosions") {
                        Some(Tag::List(list)) => list.iter().map(explosion_from).collect(),
                        _ => Vec::new(),
                    };
                    (flight, explosions)
                })
                .filter(|(flight, explosions)| !(*flight == 1 && explosions.is_empty())),
            firework_explosion: c.get("minecraft:firework_explosion").map(explosion_from),
            dyed_color: c
                .get("minecraft:dyed_color")
                .and_then(int)
                .map(|v| v as i32),
            pot_decorations: c
                .get("minecraft:pot_decorations")
                .map(|p| {
                    let side = |name: &str| p.get(name).and_then(template_from);
                    Box::new([side("back"), side("left"), side("right"), side("front")])
                })
                .filter(|sides| !all_bricks(sides)),
            bundle_contents: match c.get("minecraft:bundle_contents") {
                Some(Tag::List(list)) => list.iter().filter_map(template_from).collect(),
                _ => Vec::new(),
            },
            stew_effects: match c.get("minecraft:suspicious_stew_effects") {
                Some(Tag::List(list)) => list
                    .iter()
                    .filter_map(|e| {
                        Some((
                            Arc::from(e.get("id").and_then(string)?.as_str()),
                            e.get("duration").and_then(int).unwrap_or(160) as i32,
                        ))
                    })
                    .collect(),
                _ => Vec::new(),
            },
            map_post_processing: match c.get("minecraft:map_post_processing") {
                Some(Tag::String(s)) => Some(u8::from(s.to_string_lossy() != "lock")),
                Some(t) => int(t).map(|v| v.clamp(0, 1) as u8),
                None => None,
            },
            charged: match c.get("minecraft:charged_projectiles") {
                Some(Tag::List(list)) => list
                    .iter()
                    .filter_map(crate::item_nbt::item_from_nbt)
                    .collect(),
                _ => Vec::new(),
            },
            container: match c.get("minecraft:container") {
                Some(Tag::List(list)) => list
                    .iter()
                    .filter_map(|e| {
                        Some((
                            e.get("slot").and_then(int)?.clamp(0, 255) as u8,
                            template_from(e.get("item")?)?,
                        ))
                    })
                    .collect(),
                _ => Vec::new(),
            },
            ominous_banner: c
                .get("minecraft:item_name")
                .and_then(|n| n.get("translate"))
                .and_then(string)
                .as_deref()
                == Some("block.minecraft.ominous_banner")
                && matches!(
                    c.get("minecraft:banner_patterns"),
                    Some(Tag::List(layers)) if layers.len() == OMINOUS_LAYERS.len()),
            ominous_amplifier: c
                .get("minecraft:ominous_bottle_amplifier")
                .and_then(int)
                .map(|v| v.clamp(0, 4) as u8),
            trim: c.get("minecraft:trim").and_then(trim::from_nbt),
            bundle_selected: BundleSelected::default(),
            other: Vec::new(),
            wire: Wire::default(),
        };
        if let Tag::Compound(map) = c {
            for (k, v) in map {
                let key = k.to_string_lossy();
                let ominous_part = data.ominous_banner
                    && (matches!(key.as_str(), "minecraft:item_name" | "minecraft:rarity")
                        || (key == "minecraft:tooltip_display" && v == &ominous_tooltip_display()));
                if key == "minecraft:stored_enchantments"
                    && data.stored_enchantments.is_empty()
                    && matches!(v, Tag::Compound(_))
                {
                    data.other
                        .push((key, Tag::compound(Vec::<(&str, Tag)>::new())));
                } else if !MODELLED.contains(&key.as_str()) && !ominous_part {
                    let value = canonical_other_component(&key, v);
                    data.other.push((key, value));
                }
            }
            data.other.sort_by(|a, b| a.0.cmp(&b.0));
        }
        (damage, (!data.is_empty()).then(|| Box::new(data)))
    }
    /// Components as saved: none, one kept ID, every modelled ID, and IDs the
    /// model keeps in `other`, one of them with a non-ASCII unit.
    fn component_cases() -> Vec<(&'static str, Tag)> {
        let parse = |text: &str| horizon_world::nbt::parse_snbt(text).unwrap().0;
        vec![
            ("not a compound", Tag::String("minecraft:damage".into())),
            ("empty", Tag::compound(Vec::<(&str, Tag)>::new())),
            (
                "one kept id",
                parse(r#"{"minecraft:custom_data":{fixture:"save batch"}}"#),
            ),
            (
                "every modelled id",
                parse(
                    r#"{"minecraft:damage": 7,
                        "minecraft:potion_contents": {potion:"minecraft:water",custom_color:255,custom_effects:[{id:"minecraft:speed",duration:100,amplifier:1}]},
                        "minecraft:enchantments": {levels:{"minecraft:sharpness":3}},
                        "minecraft:stored_enchantments": {levels:{"minecraft:mending":1}},
                        "minecraft:repair_cost": 4,
                        "minecraft:custom_name": {text:"Blade",color:"red"},
                        "minecraft:map_id": 12,
                        "minecraft:banner_patterns": [{pattern:"minecraft:border",color:"blue"}],
                        "minecraft:base_color": "red",
                        "minecraft:writable_book_content": {pages:[{raw:"one"},{raw:"two"}]},
                        "minecraft:written_book_content": {title:{raw:"T"},author:"A",generation:1,pages:[{raw:"p"}]},
                        "minecraft:fireworks": {flight_duration:2,explosions:[{shape:"large_ball",colors:[I;16711680],has_twinkle:true}]},
                        "minecraft:firework_explosion": {shape:"star",colors:[I;255]},
                        "minecraft:dyed_color": 16711680,
                        "minecraft:pot_decorations": {back:{id:"minecraft:brick"},front:{id:"minecraft:brick"}},
                        "minecraft:bundle_contents": [{id:"minecraft:stone",count:2}],
                        "minecraft:suspicious_stew_effects": [{id:"minecraft:poison",duration:100}],
                        "minecraft:map_post_processing": "lock",
                        "minecraft:charged_projectiles": [{id:"minecraft:arrow",count:1}],
                        "minecraft:container": [{slot:0,item:{id:"minecraft:stone",count:3}}],
                        "minecraft:ominous_bottle_amplifier": 2,
                        "minecraft:trim": {material:"minecraft:gold",pattern:"minecraft:coast"},
                        "minecraft:item_name": {translate:"block.minecraft.ominous_banner"},
                        "minecraft:rarity": "uncommon",
                        "minecraft:tooltip_display": {hide_tooltip:true}
                    }"#,
                ),
            ),
            (
                "unknown and non-ASCII ids",
                parse(
                    r#"{"minecraft:custom_data":{keep:1},"minecraft:future_component":"x","minecraft:damage\u00e9":3}"#,
                ),
            ),
        ]
    }
    /// One pass over the compound decodes every shape the per-ID lookups did,
    /// including IDs kept in `other` and keys that are not ASCII.
    #[test]
    fn one_pass_decode_matches_the_per_id_lookups() {
        for (name, tag) in component_cases() {
            assert_eq!(
                decode_components(Some(&tag)),
                baseline_decode_components(Some(&tag)),
                "{name}"
            );
        }
        assert_eq!(decode_components(None), baseline_decode_components(None));
    }
    /// `decode_components` against the per-ID lookups, over the cases above. 25
    /// measured rounds alternate order and report the median microseconds per
    /// call.
    #[test]
    #[ignore = "microbenchmark; run with --ignored --nocapture"]
    fn benchmark_decode_components() {
        use std::{hint::black_box, time::Instant};
        for (name, tag) in component_cases() {
            let reps = 10_000;
            let mut times = [Vec::new(), Vec::new()];
            for round in 0..25 {
                for optimized in [round % 2 == 0, round % 2 != 0] {
                    let started = Instant::now();
                    for _ in 0..reps {
                        if optimized {
                            black_box(decode_components(black_box(Some(&tag))));
                        } else {
                            black_box(baseline_decode_components(black_box(Some(&tag))));
                        }
                    }
                    if round > 0 {
                        times[usize::from(optimized)]
                            .push(started.elapsed().as_secs_f64() * 1e6 / f64::from(reps));
                    }
                }
            }
            for t in &mut times {
                t.sort_by(f64::total_cmp);
            }
            let (base, new) = (times[0][times[0].len() / 2], times[1][times[1].len() / 2]);
            println!(
                "decode {name}: baseline {base:.3} us, new {new:.3} us, {:.2}x",
                base / new
            );
        }
    }
}

#[cfg(test)]
mod charged_projectile_template_tests {
    use super::*;

    #[test]
    fn charged_templates_omit_default_count_and_keep_components() {
        for count in [1, 2] {
            let mut projectile = ItemStack::new("minecraft:arrow", count);
            let marker = Tag::compound([("audit", Tag::Int(7))]);
            projectile
                .data_mut()
                .other
                .push(("minecraft:custom_data".into(), marker.clone()));
            let mut outer = ItemStack::new("minecraft:crossbow", 1);
            outer.data_mut().charged.push(projectile.clone());
            outer.data_mut().other.push((
                "minecraft:custom_data".into(),
                Tag::compound([("audit", Tag::Int(9))]),
            ));
            let encoded = encode_components(&outer).unwrap();
            let Tag::List(list) = encoded.get("minecraft:charged_projectiles").unwrap() else {
                panic!("projectile list")
            };
            assert_eq!(list.len(), 1);
            assert_eq!(
                list[0].get("count"),
                (count != 1).then_some(Tag::Int(count as i32)).as_ref()
            );
            assert_eq!(
                list[0]
                    .get("components")
                    .unwrap()
                    .get("minecraft:custom_data"),
                Some(&marker)
            );
            assert_eq!(
                encoded.get("minecraft:custom_data"),
                Some(&Tag::compound([("audit", Tag::Int(9))]))
            );
            // Generic ItemStack/inventory encoding retains its required count.
            assert_eq!(
                crate::item_nbt::item_to_nbt(&projectile).get("count"),
                Some(&Tag::Int(count as i32))
            );
            let (_, decoded) = decode_components(Some(&encoded));
            let restored = decoded.unwrap();
            assert_eq!(restored.charged[0].count, count);
            assert_eq!(
                restored.charged[0]
                    .data()
                    .unwrap()
                    .other("minecraft:custom_data"),
                Some(&marker)
            );
        }
    }
}
