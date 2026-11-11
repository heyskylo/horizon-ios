//! `/item` (ItemCommands) and `if items`/`if slots`: addressing the item
//! slots of the player, mobs, armor stands, item frames and containers.
use super::{Arg, Context, Game, R, Target, err};
use horizon_client::{
    command::{CommandError, args::ItemInput},
    containers::BlockEntity,
    furnace::FurnaceCooking,
    items::ItemStack,
    scoreboard_cmd::{Holder, Tracker},
    text::Text,
};
use horizon_world::nbt::Tag;

mod contents;
use contents::Component;

/// One addressable slot.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::game) enum Slot {
    /// `inventory[0..36]`: hotbar then the main rows.
    Inv(usize),
    /// The selected hotbar slot as `weapon.mainhand` names it: the equipment's
    /// `setItemSlot`, where `hotbar.N` is the inventory's `setItem`.
    MainHand,
    /// `armor[0..4]` (head, chest, legs, feet).
    Armor(usize),
    Offhand,
    Ender(usize),
    /// A mob's equipment by name (`mainhand`, `offhand`, `head`, ...).
    Mob(u32, &'static str),
    /// An armor stand's equipment (`ArmorStand::SLOTS` index).
    Stand(u32, usize),
    Frame(u32),
    /// A chest or hopper minecart's slot (`ContainerEntity.getChestVehicleSlot`).
    Cart(u32, usize),
    Block([i32; 3], usize),
    /// A slot of another player (`for_player`): reached with their state in place.
    Guest(u32, Box<Slot>),
    /// A component collection for writes, or an indexed component item for reads.
    Contents(Box<Slot>, Component),
    ContentItem(Box<Slot>, Component, usize),
}
struct SlotCandidate {
    slot: Slot,
    filters: Vec<serde_json::Value>,
    limit: usize,
}
/// Whose slots a command addresses.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::game) enum Owner {
    Entity(Target),
    Block([i32; 3]),
}

/// The standard equipment slots with existing whole-stack storage.
fn mob_stack_slot(key: &str) -> Option<usize> {
    use horizon_client::mobs::finalize;
    Some(match key {
        "mainhand" => finalize::MAINHAND,
        "offhand" => finalize::OFFHAND,
        "head" => finalize::HEAD,
        "chest" => finalize::CHEST,
        "legs" => finalize::LEGS,
        "feet" => finalize::FEET,
        _ => return None,
    })
}

/// The player's slot names and the inventory index each addresses.
fn player_slot_names() -> Vec<(String, usize)> {
    let mut v = Vec::new();
    for i in 0..36 {
        v.push((format!("container.{i}"), i));
    }
    for i in 0..9 {
        v.push((format!("hotbar.{i}"), i));
    }
    for i in 0..27 {
        v.push((format!("inventory.{i}"), 9 + i));
    }
    v
}
/// `name` against a group: `container.*` matches every `container.N`.
fn matches_name(pattern: &str, name: &str) -> bool {
    match pattern.strip_suffix(".*") {
        Some(prefix) => name.starts_with(&format!("{prefix}.")),
        None => pattern == name,
    }
}

impl Game<'_> {
    fn owner_of(&mut self, ctx: &Context, prefix: &str, many: bool) -> R<Vec<Owner>> {
        let kind = ctx
            .literals
            .iter()
            .filter(|l| matches!(l.as_str(), "entity" | "block"))
            .nth(usize::from(prefix == "source" && ctx.has("from")))
            .cloned()
            .unwrap_or_default();
        if kind == "block" {
            let p = self.block_pos_arg(ctx, &format!("{prefix}Pos"))?;
            return Ok(vec![Owner::Block(p)]);
        }
        let targets = self.entities(ctx, prefix, false)?;
        let _ = many;
        Ok(targets.into_iter().map(Owner::Entity).collect())
    }
    /// The slots a name (or wildcard group) addresses on an owner.
    pub(in crate::game) fn slots_named(&mut self, owner: &Owner, name: &str) -> Vec<Slot> {
        if let Owner::Entity(Target::Guest(conn)) = *owner {
            let slots = self.for_player(Target::Guest(conn), |g| {
                g.slots_named(&Owner::Entity(Target::Player), name)
            });
            return slots
                .unwrap_or_default()
                .into_iter()
                .map(|s| Slot::Guest(conn, Box::new(s)))
                .collect();
        }
        let mut out = Vec::new();
        let mut add = |s: Slot| out.push(s);
        match owner {
            Owner::Block(p) => {
                let size = self.block_container_size(*p).unwrap_or(0);
                for i in 0..size {
                    if matches_name(name, &format!("container.{i}")) {
                        add(Slot::Block(*p, i));
                    }
                }
            }
            Owner::Entity(Target::Player) => {
                for (n, i) in player_slot_names() {
                    if matches_name(name, &n) {
                        add(Slot::Inv(i));
                    }
                }
                for (i, n) in ["armor.head", "armor.chest", "armor.legs", "armor.feet"]
                    .into_iter()
                    .enumerate()
                {
                    if matches_name(name, n) {
                        add(Slot::Armor(i));
                    }
                }
                if matches_name(name, "weapon") || matches_name(name, "weapon.mainhand") {
                    add(Slot::MainHand);
                }
                if matches_name(name, "weapon.offhand") {
                    add(Slot::Offhand);
                }
                for i in 0..27 {
                    if matches_name(name, &format!("enderchest.{i}")) {
                        add(Slot::Ender(i));
                    }
                }
            }
            Owner::Entity(Target::Mob(id)) => {
                for (n, key) in [
                    ("weapon", "mainhand"),
                    ("weapon.mainhand", "mainhand"),
                    ("weapon.offhand", "offhand"),
                    ("armor.head", "head"),
                    ("armor.chest", "chest"),
                    ("armor.legs", "legs"),
                    ("armor.feet", "feet"),
                    ("armor.body", "body"),
                    ("saddle", "saddle"),
                ] {
                    if matches_name(name, n) {
                        add(Slot::Mob(*id, key));
                    }
                }
            }
            Owner::Entity(Target::Stand(id)) => {
                for (i, key) in horizon_client::entities::ArmorStand::SLOTS
                    .iter()
                    .enumerate()
                {
                    let aliases: &[&str] = match *key {
                        "mainhand" => &["weapon", "weapon.mainhand"],
                        "offhand" => &["weapon.offhand"],
                        "feet" => &["armor.feet"],
                        "legs" => &["armor.legs"],
                        "chest" => &["armor.chest"],
                        _ => &["armor.head"],
                    };
                    if aliases.iter().any(|a| matches_name(name, a)) {
                        add(Slot::Stand(*id, i));
                    }
                }
            }
            Owner::Entity(Target::Frame(id)) => {
                if matches_name(name, "container.0") || matches_name(name, "contents") {
                    add(Slot::Frame(*id));
                }
            }
            // `getChestVehicleSlot` takes any index below `getContainerSize`.
            Owner::Entity(Target::Cart(id)) => {
                let size = self
                    .carts
                    .iter()
                    .find(|c| c.id == *id)
                    .map_or(0, |c| c.kind.slots());
                for i in 0..size {
                    if matches_name(name, &format!("container.{i}")) {
                        add(Slot::Cart(*id, i));
                    }
                }
            }
            Owner::Entity(_) => {}
        }
        out
    }
    /// `ItemCommands.getSlotsFromProvider`: evaluate inline sources against
    /// the accessor's live owner. Named ranges retain their existing adapter.
    fn command_slots(&mut self, owner: &Owner, raw: &str) -> Vec<Slot> {
        if raw.starts_with('{') || raw.starts_with('[') {
            let Ok((tag, _)) = horizon_world::nbt::parse_snbt(raw) else {
                return Vec::new();
            };
            self.inline_slots(owner, &horizon_client::scoreboard::tag_to_json(&tag))
        } else {
            self.slots_named(owner, raw)
        }
    }
    /// `SlotCollection.itemCopies` / `size`: transforms apply to the
    /// collection, before a command's item predicate or item modifier.
    fn inline_slots(&mut self, owner: &Owner, source: &serde_json::Value) -> Vec<Slot> {
        if let Some(terms) = source.as_array() {
            return terms
                .iter()
                .flat_map(|s| self.inline_slots(owner, s))
                .collect();
        }
        match source["type"]
            .as_str()
            .unwrap_or_default()
            .trim_start_matches("minecraft:")
        {
            "group" => source["terms"].as_array().map_or_else(Vec::new, |terms| {
                terms
                    .iter()
                    .flat_map(|s| self.inline_slots(owner, s))
                    .collect()
            }),
            "slot_range" => self.slots_named(owner, source["slots"].as_str().unwrap_or_default()),
            "contents" => {
                let Some(component) = source["component"].as_str().and_then(Component::named)
                else {
                    return Vec::new();
                };
                let parents = self.inline_slots(owner, &source["slot_source"]);
                parents
                    .into_iter()
                    .flat_map(|p| self.contents_slots(p, component))
                    .collect()
            }
            "limit_slots" => {
                let mut slots = self.inline_slots(owner, &source["slot_source"]);
                slots.truncate(source["limit"].as_u64().unwrap_or(0) as usize);
                slots
            }
            "filtered" => {
                let slots = self.inline_slots(owner, &source["slot_source"]);
                slots
                    .into_iter()
                    .filter(|slot| self.slot_filter(slot, &source["item_filter"]))
                    .collect()
            }
            _ => Vec::new(),
        }
    }
    fn slot_filter(&mut self, slot: &Slot, filter: &serde_json::Value) -> bool {
        let stack = self
            .slot_get(slot)
            .unwrap_or_else(|| ItemStack::new("minecraft:air", 0));
        self.loot_item_filter(filter, &stack)
    }
    /// `SlotCollection.modifySlots` / `replaceSlotItems`: filters inspect the
    /// live stack when visited. Limits share the command's TrackingSelector
    /// count across owners and group terms, and count selected empty slots
    /// only for replacement (not modification).
    fn command_slot_candidates(&mut self, owner: &Owner, raw: &str) -> Vec<SlotCandidate> {
        let source = horizon_world::nbt::parse_snbt(raw)
            .ok()
            .map(|(tag, _)| horizon_client::scoreboard::tag_to_json(&tag));
        if raw.starts_with('{') || raw.starts_with('[') {
            let mut out = Vec::new();
            if let Some(source) = source {
                self.slot_candidates(owner, &source, &[], usize::MAX, &mut out);
            }
            out
        } else {
            self.slots_named(owner, raw)
                .into_iter()
                .map(|slot| SlotCandidate {
                    slot,
                    filters: Vec::new(),
                    limit: usize::MAX,
                })
                .collect()
        }
    }
    /// `CompositeSlotSource.provide` and `TransformedSlotSource.provide`:
    /// carry each leaf's filters and selection bound into live writes.
    fn slot_candidates(
        &mut self,
        owner: &Owner,
        source: &serde_json::Value,
        filters: &[serde_json::Value],
        limit: usize,
        out: &mut Vec<SlotCandidate>,
    ) {
        if let Some(terms) = source.as_array() {
            for term in terms {
                self.slot_candidates(owner, term, filters, limit, out);
            }
            return;
        }
        match source["type"]
            .as_str()
            .unwrap_or_default()
            .trim_start_matches("minecraft:")
        {
            "group" => {
                if let Some(terms) = source["terms"].as_array() {
                    for term in terms {
                        self.slot_candidates(owner, term, filters, limit, out);
                    }
                }
            }
            "slot_range" => {
                for slot in self.slots_named(owner, source["slots"].as_str().unwrap_or_default()) {
                    out.push(SlotCandidate {
                        slot,
                        filters: filters.to_vec(),
                        limit,
                    });
                }
            }
            "contents" => {
                if let Some(component) = source["component"].as_str().and_then(Component::named) {
                    for parent in self.inline_slots(owner, &source["slot_source"]) {
                        out.push(SlotCandidate {
                            slot: Slot::Contents(Box::new(parent), component),
                            filters: filters.to_vec(),
                            limit,
                        });
                    }
                }
            }
            "filtered" => {
                let mut filters = filters.to_vec();
                filters.push(source["item_filter"].clone());
                self.slot_candidates(owner, &source["slot_source"], &filters, limit, out);
            }
            "limit_slots" => self.slot_candidates(
                owner,
                &source["slot_source"],
                filters,
                limit.min(source["limit"].as_u64().unwrap_or(0) as usize),
                out,
            ),
            _ => {}
        }
    }
    /// `SlotSelector.TrackingSelector.trySelectSlot`: count only slots that
    /// pass the predicate, before attempting the setter.
    fn select_slot(
        &mut self,
        candidate: &SlotCandidate,
        selected: &mut usize,
        nonempty: bool,
    ) -> bool {
        if *selected >= candidate.limit
            || (nonempty && self.slot_get(&candidate.slot).is_none())
            || !candidate
                .filters
                .iter()
                .all(|f| self.slot_filter(&candidate.slot, f))
        {
            return false;
        }
        *selected += 1;
        true
    }
    pub(in crate::game) fn slot_get(&mut self, s: &Slot) -> Option<ItemStack> {
        match s {
            Slot::Contents(..) => None,
            Slot::ContentItem(parent, component, index) => {
                let stack = self.slot_get(parent)?;
                component.items(&stack).get(*index).cloned().flatten()
            }
            Slot::Guest(conn, slot) => self
                .for_player(Target::Guest(*conn), |g| g.slot_get(slot))
                .flatten(),
            Slot::Inv(i) => self.inventory.get(*i).cloned().flatten(),
            Slot::MainHand => self.inventory.get(self.selected).cloned().flatten(),
            Slot::Armor(i) => self.armor.get(*i).cloned().flatten(),
            Slot::Offhand => self.offhand.clone(),
            Slot::Ender(i) => self.ender_items.get(*i).cloned().flatten(),
            Slot::Mob(id, key) => {
                let m = self.mobs.iter().find(|m| m.id == *id)?;
                if *key == "saddle" {
                    return m.saddle_item();
                }
                if let Some(slot) = mob_stack_slot(key) {
                    if let Some(stack) = horizon_client::mobs::finalize::stored_stack(m, slot) {
                        return Some(stack);
                    }
                    let worn = ["head", "chest", "legs", "feet"]
                        .iter()
                        .position(|name| name == key);
                    if let Some(stack) = worn.and_then(|i| m.extra.armor[i].clone()).filter(|s| {
                        horizon_client::mobs::finalize::item(m, slot).as_ref() == Some(&s.item)
                    }) {
                        return Some(stack);
                    }
                    return horizon_client::mobs::finalize::item_stack(m, slot);
                }
                let item = match *key {
                    "mainhand" => &m.extra.main_hand,
                    "offhand" => &m.extra.off_hand,
                    "head" => &m.extra.head,
                    "body" => &m.extra.body,
                    "chest" => &m.extra.chest,
                    "legs" => &m.extra.legs,
                    "feet" => &m.extra.feet,
                    _ => return None,
                };
                item.as_ref().map(|i| ItemStack {
                    item: i.clone(),
                    count: 1,
                    damage: 0,
                    extra: None,
                })
            }
            Slot::Stand(id, i) => self
                .armor_stands
                .iter()
                .find(|a| a.id == *id)?
                .equipment
                .get(*i)
                .cloned()
                .flatten(),
            Slot::Frame(id) => self.item_frames.iter().find(|f| f.id == *id)?.item.clone(),
            Slot::Cart(id, i) => self.cart_slots(*id)?.get(*i).cloned().flatten(),
            Slot::Block(p, i) => self
                .world
                .block_entities
                .get(&(p[0], p[1], p[2]))?
                .slots()
                .get(*i)
                .cloned()
                .flatten(),
        }
    }
    pub(in crate::game) fn slot_accepts(&self, s: &Slot, stack: Option<&ItemStack>) -> bool {
        let Slot::Block(p, i) = s else {
            return true;
        };
        let air = ItemStack {
            item: "minecraft:air".into(),
            count: 0,
            damage: 0,
            extra: None,
        };
        self.block_can_place(*p, *i, stack.unwrap_or(&air))
    }
    fn block_container_size(&self, p: [i32; 3]) -> Option<usize> {
        let size = match self.world.block_entities.get(&(p[0], p[1], p[2]))? {
            BlockEntity::Mod(m) if m.id.is_empty() => 0,
            BlockEntity::Mod(_) => self
                .world
                .block_entities
                .mod_container(self.world.block(p[0], p[1], p[2]).0)
                .map_or(0, |c| c.slots),
            entity => entity.slots().len(),
        };
        (size > 0).then_some(size)
    }
    fn block_slot_set(&mut self, p: [i32; 3], i: usize, stack: Option<ItemStack>) -> bool {
        let key = (p[0], p[1], p[2]);
        let Some(size) = self.block_container_size(p).filter(|&size| i < size) else {
            return false;
        };
        let book = stack.as_ref().is_none_or(|s| {
            self.items()
                .item_tags
                .get("minecraft:bookshelf_books")
                .is_some_and(|m| m.contains(&*s.item))
        });
        let recipes = world_recipes!(self);
        match self.world.block_entities.get_mut(&key) {
            Some(BlockEntity::Furnace(f)) if i == 0 => f.set_input(recipes, stack),
            Some(BlockEntity::Furnace(f)) => {
                f.items[i] = stack;
                self.world.modified.insert((p[0] >> 4, p[2] >> 4));
                return true;
            }
            Some(BlockEntity::ChiseledBookshelf(_)) if !book => return true,
            Some(BlockEntity::Jukebox(_)) => {
                self.jukebox_set_item(p, stack);
                return true;
            }
            Some(BlockEntity::Crafter(c)) => {
                c.disabled[i] = false;
                c.items[i] = stack;
            }
            Some(BlockEntity::Mod(m)) => {
                if m.items.len() < size {
                    m.items.resize(size, None);
                }
                m.items[i] = stack;
            }
            Some(entity) => entity.slots_mut()[i] = stack,
            None => return false,
        }
        self.container_changed(key);
        true
    }
    /// `LivingEntity.createEquipmentSlotAccess`: every slot but head, mainhand
    /// and offhand takes only an empty stack or one whose
    /// `getEquipmentSlotForItem` is that slot (a non-equippable item is mainhand).
    fn equipment_slot_accepts(&self, s: &Slot, stack: Option<&ItemStack>) -> bool {
        let name = match s {
            Slot::Guest(_, inner) => return self.equipment_slot_accepts(inner, stack),
            Slot::Armor(i @ 1..=3) => ["head", "chest", "legs", "feet"][*i],
            Slot::Mob(_, key) if matches!(*key, "chest" | "legs" | "feet" | "body" | "saddle") => {
                key
            }
            _ => return true,
        };
        // ItemStack.isEmpty: air or a count of 0.
        let Some(stack) = stack.filter(|s| &*s.item != "minecraft:air" && s.count > 0) else {
            return true;
        };
        // SulfurCube.getEquipmentSlotForItem: a swallowable item is a body item.
        if let Slot::Mob(id, "body") = s
            && self.mobs.iter().find(|m| m.id == *id).is_some_and(|m| {
                m.species() == horizon_client::mobs::Species::SulfurCube
                    && horizon_client::mobs::sulfur_cube::swallowable(m, &stack.item)
            })
        {
            return true;
        }
        self.items()
            .get(&stack.item)
            .is_some_and(|info| info.equip_slot.as_deref() == Some(name))
    }
    /// Put a stack (or nothing) in a slot; false when the slot cannot take it.
    pub(in crate::game) fn slot_set(&mut self, s: &Slot, stack: Option<ItemStack>) -> bool {
        match s {
            Slot::Contents(..) => false,
            Slot::ContentItem(parent, component, index) => {
                let Some(mut outer) = self.slot_get(parent) else {
                    return false;
                };
                let mut slots = component.items(&outer);
                let Some(slot) = slots.get_mut(*index) else {
                    return false;
                };
                *slot = stack;
                component.store(&mut outer, slots);
                self.slot_set(parent, Some(outer))
            }
            Slot::Guest(conn, slot) => self
                .for_player(Target::Guest(*conn), |g| g.slot_set(slot, stack))
                .unwrap_or(false),
            Slot::Inv(i) => match self.inventory.get_mut(*i) {
                Some(slot) => {
                    *slot = stack;
                    true
                }
                None => false,
            },
            // LivingEntity's equipment SlotAccess: setItemSlot, so onEquipItem.
            Slot::MainHand => match self.inventory.get_mut(self.selected) {
                Some(slot) => {
                    let old = std::mem::replace(slot, stack.clone());
                    self.player_on_equip("mainhand", old.as_ref(), stack.as_ref());
                    true
                }
                None => false,
            },
            Slot::Armor(i) => match self.armor.get_mut(*i) {
                Some(slot) => {
                    let old = std::mem::replace(slot, stack.clone());
                    let name = ["head", "chest", "legs", "feet"][*i];
                    self.player_on_equip(name, old.as_ref(), stack.as_ref());
                    self.apply_equipment();
                    true
                }
                None => false,
            },
            Slot::Offhand => {
                let old = std::mem::replace(&mut self.offhand, stack.clone());
                self.player_on_equip("offhand", old.as_ref(), stack.as_ref());
                true
            }
            Slot::Ender(i) => match self.ender_items.get_mut(*i) {
                Some(slot) => {
                    *slot = stack;
                    true
                }
                None => false,
            },
            Slot::Mob(id, key) => {
                let Some(index) = self.mobs.iter().position(|m| m.id == *id) else {
                    return false;
                };
                let m = &self.mobs[index];
                let first_tick = m.first_tick;
                let at = [m.position.x, m.position.y, m.position.z];
                let old = match *key {
                    "mainhand" => m
                        .extra
                        .main_hand_stack()
                        .cloned()
                        .or_else(|| m.extra.main_hand.as_ref().map(|i| ItemStack::new(i, 1))),
                    "offhand" => m.extra.off_hand.as_ref().map(|i| ItemStack::new(i, 1)),
                    "head" => m.extra.armor[0]
                        .clone()
                        .or_else(|| m.extra.head.as_ref().map(|i| ItemStack::new(i, 1))),
                    "chest" => m.extra.armor[1]
                        .clone()
                        .or_else(|| m.extra.chest.as_ref().map(|i| ItemStack::new(i, 1))),
                    "legs" => m.extra.armor[2]
                        .clone()
                        .or_else(|| m.extra.legs.as_ref().map(|i| ItemStack::new(i, 1))),
                    "feet" => m.extra.armor[3]
                        .clone()
                        .or_else(|| m.extra.feet.as_ref().map(|i| ItemStack::new(i, 1))),
                    "body" => m.extra.body.as_ref().map(|i| ItemStack::new(i, 1)),
                    "saddle" => m.saddle_item(),
                    _ => None,
                };
                let same = match (old.as_ref(), stack.as_ref()) {
                    (None, None) => true,
                    (Some(a), Some(b)) => a.same_item_same_components(b),
                    _ => false,
                };
                let equippable = stack.as_ref().is_some_and(|s| {
                    let data = s.data();
                    !data.is_some_and(|d| d.other("!minecraft:equippable").is_some())
                        && (data.is_some_and(|d| d.other("minecraft:equippable").is_some())
                            || self
                                .items()
                                .get(&s.item)
                                .is_some_and(|i| i.equip_slot.is_some()))
                });
                let m = &mut self.mobs[index];
                let item = stack.as_ref().map(|s| s.item.clone());
                // The worn armour stack goes with the id: a kept stack of the
                // previous piece would otherwise still be what the mob wears.
                let worn = match *key {
                    "head" => Some(0),
                    "chest" => Some(1),
                    "legs" => Some(2),
                    "feet" => Some(3),
                    _ => None,
                };
                if let Some(i) = worn {
                    m.extra.armor[i] = stack.clone();
                }
                if let Some(slot) = mob_stack_slot(key) {
                    if let Some(stack) = stack.as_ref() {
                        horizon_client::mobs::finalize::set_item_stack(m, slot, stack.clone());
                    } else {
                        horizon_client::mobs::finalize::set_slot_enchantments(m, slot, Vec::new());
                    }
                }
                match *key {
                    "mainhand" => m.extra.set_main_hand_stack(stack),
                    "offhand" => m.extra.off_hand = item,
                    "head" => m.extra.head = item,
                    "body" => m.extra.body = item,
                    "chest" => m.extra.chest = item,
                    "legs" => m.extra.legs = item,
                    "feet" => m.extra.feet = item,
                    "saddle" => m.set_saddle(stack),
                    _ => return false,
                }
                if !first_tick && !same && self.remote.is_none() {
                    self.mob_equip_event(*id, at, equippable);
                }
                true
            }
            Slot::Stand(id, i) => {
                // LivingEntity.onEquipItem: all stand slots emit; the new
                // stack's EQUIPPABLE presence chooses equip versus unequip.
                let equippable = stack.as_ref().is_some_and(|s| {
                    let data = s.data();
                    !data.is_some_and(|d| d.other("!minecraft:equippable").is_some())
                        && (data.is_some_and(|d| d.other("minecraft:equippable").is_some())
                            || self
                                .items()
                                .get(&s.item)
                                .is_some_and(|i| i.equip_slot.is_some()))
                });
                let Some(a) = self.armor_stands.iter_mut().find(|a| a.id == *id) else {
                    return false;
                };
                let Some(slot) = a.equipment.get_mut(*i) else {
                    return false;
                };
                let same = match (slot.as_ref(), stack.as_ref()) {
                    (None, None) => true,
                    (Some(old), Some(new)) => old.same_item_same_components(new),
                    _ => false,
                };
                let emit = !a.first_tick && !same;
                let at = a.position;
                *slot = stack;
                if emit && self.remote.is_none() {
                    self.post_game_event(
                        if equippable {
                            "minecraft:equip"
                        } else {
                            "minecraft:unequip"
                        },
                        [at.x, at.y, at.z],
                        horizon_client::sculk::Context {
                            source: Some(horizon_client::mobs::Who::Mob(*id)),
                            living: true,
                            ..Default::default()
                        },
                    );
                }
                true
            }
            Slot::Frame(id) => match self.item_frames.iter().position(|f| f.id == *id) {
                Some(i) => {
                    self.set_frame_item(i, stack);
                    true
                }
                None => false,
            },
            Slot::Cart(id, i) => match self.cart_slots(*id).and_then(|c| c.get_mut(*i)) {
                Some(slot) => {
                    *slot = stack;
                    true
                }
                None => false,
            },
            Slot::Block(p, i) => self.block_slot_set(*p, *i, stack),
        }
    }
    fn owner_holder(&self, o: &Owner) -> Holder {
        match o {
            Owner::Entity(t) => Holder {
                name: String::new(),
                text: self.target_name(*t),
                display: None,
            },
            Owner::Block(p) => Holder {
                name: String::new(),
                text: Text::literal(format!("{}, {}, {}", p[0], p[1], p[2])),
                display: None,
            },
        }
    }
    fn stack_name(&self, s: &ItemStack) -> Text {
        self.stack_display_name(s)
    }
    fn item_from_input(&self, ctx: &Context) -> R<Option<ItemStack>> {
        let Some(Arg::Item(ItemInput { id, components, .. })) = ctx.get("item").cloned() else {
            return Ok(None);
        };
        let (damage, extra) = match components
            .as_deref()
            .map(horizon_client::item_data::command_components)
        {
            Some(Ok(tag)) => horizon_client::item_data::decode_components(Some(&tag)),
            Some(Err(e)) => return Err(CommandError::new(Text::literal(e))),
            None => (0, None),
        };
        let count = Self::int(ctx, "count").unwrap_or(1).max(1) as u32;
        let stack = ItemStack {
            item: id.as_str().into(),
            count,
            damage,
            extra,
        };
        // ItemInput.createItemStack: a count above the stack's max (its
        // max_stack_size component, else the item's) is refused.
        let max_stack = self.stack_max_size(&stack);
        if count > max_stack {
            return Err(err("arguments.item.overstacked", &[&id, &max_stack]));
        }
        // ItemInput.createItemStack validates effective components after count.
        // Materialization currently discards removals, so inspect the validated
        // command patch itself without changing the stack or its saved patch.
        let components = self.item_validation_components(&id, components.as_deref())?;
        if components.get(&"minecraft:max_damage".to_owned()).is_some()
            && horizon_world::datafix::dynamic::as_int(
                components.get(&"minecraft:max_stack_size".to_owned()),
                1,
            ) > 1
        {
            return Err(err(
                "arguments.item.malformed",
                &[&"Item cannot be both damageable and stackable"],
            ));
        }
        Ok(Some(stack))
    }
    /// ItemInput.createItemStack: read-only view of the two prototype components used by this command
    /// validation, with all typed patch operations (including removals).
    fn item_validation_components(
        &self,
        id: &str,
        components: Option<&str>,
    ) -> R<horizon_world::component_patch::ComponentMap<String, Tag>> {
        use horizon_client::command::{Reader, args::read_identifier, snbt::parse_tag_argument};
        use horizon_world::component_patch::{ComponentMap, Patch, Prototype};
        let mut prototype = Vec::new();
        if let Some(item) = self.items().get(id) {
            for (key, value) in [
                ("minecraft:max_damage", item.max_damage),
                ("minecraft:max_stack_size", item.max_stack),
            ] {
                if item.components.iter().any(|component| component == key) {
                    prototype.push((key.to_owned(), Tag::Int(value as i32)));
                }
            }
        }
        let mut patch = Vec::new();
        if let Some(components) = components {
            // ItemInput has already validated IDs, values and duplicates. Reuse
            // its token/value parsers; do not derive removals from saved output.
            let mut reader = Reader::new(components);
            reader.expect('[')?;
            reader.skip_whitespace();
            while reader.can_read() && reader.peek() != ']' {
                let removed = reader.peek() == '!';
                if removed {
                    reader.skip();
                }
                let key = read_identifier(&mut reader)?;
                reader.skip_whitespace();
                let value = if removed {
                    None
                } else {
                    reader.expect('=')?;
                    reader.skip_whitespace();
                    Some(parse_tag_argument(&mut reader)?)
                };
                patch.push((key, value));
                reader.skip_whitespace();
                if reader.peek() == ']' {
                    break;
                }
                reader.expect(',')?;
                reader.skip_whitespace();
            }
            reader.expect(']')?;
        }
        Ok(ComponentMap::from_patch(
            Prototype::new(prototype),
            &Patch::new(patch),
        ))
    }
    /// One item modifier (a typed object, a list or a datapack reference)
    /// applied to a stack; `None` when the stack ends up empty.
    fn apply_item_modifier(
        &mut self,
        m: &serde_json::Value,
        stack: Option<ItemStack>,
        rng: &mut horizon_core::random::LegacyRandom,
        this: Option<&horizon_client::advancements::EntityView>,
        origin: [f64; 3],
        depth: u32,
    ) -> Option<ItemStack> {
        use horizon_core::random::RandomSource;
        use serde_json::Value;
        if depth > 16 {
            return stack;
        }
        let int = |v: &Value, rng: &mut horizon_core::random::LegacyRandom| -> i32 {
            if let Some(n) = v.as_f64() {
                return n as i32;
            }
            let min = v["min"].as_f64().unwrap_or(0.) as i32;
            let max = v["max"].as_f64().unwrap_or(0.) as i32;
            match v["type"].as_str().unwrap_or("minecraft:constant") {
                "minecraft:uniform" | "uniform" => rng
                    .next_int_between_inclusive(min, max.max(min))
                    .unwrap_or(min),
                _ => v["value"].as_f64().unwrap_or(0.) as i32,
            }
        };
        match m {
            Value::Array(list) => list.iter().fold(stack, |s, f| {
                self.apply_item_modifier(f, s, rng, this, origin, depth + 1)
            }),
            Value::String(id) => match self.fns.store.item_modifiers.get(id).cloned() {
                Some(f) => self.apply_item_modifier(&f, stack, rng, this, origin, depth + 1),
                None => stack,
            },
            Value::Object(_) => {
                let mut stack = stack?;
                // 26.3 item modifiers: a single `condition`, dispatched on `type`.
                if !m["condition"].is_null() && !self.conditions_hold(&m["condition"], this, origin)
                {
                    return Some(stack);
                }
                match m["type"].as_str().unwrap_or("") {
                    "minecraft:sequence" => {
                        #[allow(clippy::manual_try_fold)]
                        return m["functions"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .fold(Some(stack), |s, f| {
                                self.apply_item_modifier(f, s, rng, this, origin, depth + 1)
                            });
                    }
                    "minecraft:exploration_map" => {
                        if stack.count > 0 {
                            let map = horizon_world::loot::ExplorationMap::from_json(m);
                            // A failed command search keeps the input; pending loot's
                            // caller owns its separate filtered/drop policy.
                            self.materialize_exploration_map(&mut stack, &map, origin);
                        }
                    }
                    "minecraft:set_count" => {
                        let n = int(&m["count"], rng);
                        let base = if m["add"].as_bool() == Some(true) {
                            stack.count as i32
                        } else {
                            0
                        };
                        let max = self
                            .res
                            .items()
                            .get(&stack.item)
                            .map_or(64, |i| i.max_stack);
                        stack.count = (base + n).clamp(0, max as i32) as u32;
                    }
                    "minecraft:limit_count" => {
                        let min = m["limit"]["min"].as_i64().or(m["limit"].as_i64());
                        let max = m["limit"]["max"].as_i64().or(m["limit"].as_i64());
                        if let Some(min) = min {
                            stack.count = stack.count.max(min as u32);
                        }
                        if let Some(max) = max {
                            stack.count = stack.count.min(max as u32);
                        }
                    }
                    "minecraft:set_damage" => {
                        let max = self
                            .res
                            .items()
                            .get(&stack.item)
                            .map_or(0, |i| i.max_damage);
                        if max > 0 {
                            let f = match m["damage"].as_f64() {
                                Some(f) => f as f32,
                                None => {
                                    let lo = m["damage"]["min"].as_f64().unwrap_or(0.) as f32;
                                    let hi = m["damage"]["max"].as_f64().unwrap_or(0.) as f32;
                                    lo + rng.next_float() * (hi - lo)
                                }
                            };
                            let base = if m["add"].as_bool() == Some(true) {
                                1. - stack.damage as f32 / max as f32
                            } else {
                                0.
                            };
                            let frac = (base + f).clamp(0., 1.);
                            stack.damage = ((1. - frac) * max as f32).floor() as u32;
                        }
                    }
                    "minecraft:set_name" => {
                        if m["name"].is_null() {
                            return Some(stack);
                        }
                        if m["target"].as_str() == Some("item_name") {
                            let name = horizon_client::scoreboard::json_to_tag(&m["name"]);
                            edit_components(&mut stack, |c| {
                                c.insert("minecraft:item_name".into(), name);
                            });
                        } else {
                            // SetNameFunction.run preserves the component when no entity resolver is requested.
                            stack
                                .data_mut()
                                .set_loot_name("minecraft:custom_name", &m["name"]);
                        }
                    }
                    "minecraft:set_enchantments" => {
                        let add = m["add"].as_bool() == Some(true);
                        let d = stack.extra.get_or_insert_with(Default::default);
                        let target = if &*stack.item == "minecraft:enchanted_book" {
                            &mut d.stored_enchantments
                        } else {
                            &mut d.enchantments
                        };
                        for (id, level) in m["enchantments"].as_object().into_iter().flatten() {
                            let n = int(level, rng).clamp(0, 255) as u32;
                            let id: std::sync::Arc<str> = if id.contains(':') {
                                id.as_str().into()
                            } else {
                                format!("minecraft:{id}").into()
                            };
                            match target.iter_mut().find(|(e, _)| *e == id) {
                                Some(e) => e.1 = if add { e.1 + n } else { n },
                                None => target.push((id, n)),
                            }
                            target.retain(|(_, l)| *l > 0);
                        }
                    }
                    "minecraft:set_potion" => {
                        if let Some(p) = m["id"].as_str() {
                            stack.extra.get_or_insert_with(Default::default).potion =
                                Some(p.into());
                        }
                    }
                    "minecraft:set_lore" => {
                        let lines: Vec<Tag> = m["lore"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(horizon_client::scoreboard::json_to_tag)
                            .collect();
                        edit_components(&mut stack, |c| {
                            let old: Vec<Tag> = match c.get(&"minecraft:lore".into()) {
                                Some(Tag::List(l)) => l.clone(),
                                _ => Vec::new(),
                            };
                            let new = list_operation(m, old, lines);
                            c.insert("minecraft:lore".into(), Tag::List(new));
                        });
                    }
                    "minecraft:set_custom_data" => {
                        let tag = match &m["tag"] {
                            Value::String(snbt) => horizon_world::nbt::parse_snbt(snbt)
                                .map(|(t, _)| t)
                                .unwrap_or(Tag::Compound(Default::default())),
                            v => horizon_client::scoreboard::json_to_tag(v),
                        };
                        edit_components(&mut stack, |c| {
                            let mut data = match c.get(&"minecraft:custom_data".into()) {
                                Some(t @ Tag::Compound(_)) => t.clone(),
                                _ => Tag::Compound(Default::default()),
                            };
                            merge_compound(&mut data, &tag);
                            // Vanilla CustomData.update removes the component when the result is empty.
                            if matches!(&data, Tag::Compound(m) if m.is_empty()) {
                                c.remove(&"minecraft:custom_data".into());
                            } else {
                                c.insert("minecraft:custom_data".into(), data);
                            }
                        });
                    }
                    "minecraft:set_components" => {
                        let patch = m["components"].clone();
                        edit_components(&mut stack, |c| {
                            for (k, v) in patch.as_object().into_iter().flatten() {
                                let (remove, key) = match k.strip_prefix('!') {
                                    Some(rest) => (true, rest),
                                    None => (false, k.as_str()),
                                };
                                let key = if key.contains(':') {
                                    key.to_owned()
                                } else {
                                    format!("minecraft:{key}")
                                };
                                if remove {
                                    c.remove(&key.as_str().into());
                                } else {
                                    c.insert(
                                        key.as_str().into(),
                                        horizon_client::scoreboard::json_to_tag(v),
                                    );
                                }
                            }
                        });
                    }
                    "minecraft:toggle_tooltips" => {
                        let toggles = m["toggles"].clone();
                        edit_components(&mut stack, |c| {
                            let mut hidden: Vec<String> = c
                                .get(&"minecraft:tooltip_display".into())
                                .and_then(|d| d.get("hidden_components"))
                                .and_then(|l| match l {
                                    Tag::List(l) => Some(
                                        l.iter()
                                            .filter_map(|t| match t {
                                                Tag::String(s) => Some(s.to_string_lossy()),
                                                _ => None,
                                            })
                                            .collect(),
                                    ),
                                    _ => None,
                                })
                                .unwrap_or_default();
                            let hide_tooltip = c
                                .get(&"minecraft:tooltip_display".into())
                                .and_then(|d| d.get("hide_tooltip"))
                                .cloned();
                            for (k, shown) in toggles.as_object().into_iter().flatten() {
                                let key = if k.contains(':') {
                                    k.clone()
                                } else {
                                    format!("minecraft:{k}")
                                };
                                hidden.retain(|h| *h != key);
                                if shown.as_bool() == Some(false) {
                                    hidden.push(key);
                                }
                            }
                            let mut display: Vec<(&str, Tag)> = Vec::new();
                            if let Some(h) = hide_tooltip {
                                display.push(("hide_tooltip", h));
                            }
                            if !hidden.is_empty() {
                                display.push((
                                    "hidden_components",
                                    Tag::List(
                                        hidden
                                            .iter()
                                            .map(|h| Tag::String(h.as_str().into()))
                                            .collect(),
                                    ),
                                ));
                            }
                            c.insert("minecraft:tooltip_display".into(), Tag::compound(display));
                        });
                    }
                    "minecraft:filtered" => {
                        let pass = self.loot_item_filter(&m["item_filter"], &stack);
                        let branch = if pass { &m["on_pass"] } else { &m["on_fail"] };
                        if branch.is_null() {
                            return Some(stack);
                        }
                        return self.apply_item_modifier(
                            branch,
                            Some(stack),
                            rng,
                            this,
                            origin,
                            depth + 1,
                        );
                    }
                    "minecraft:discard" => stack.count = 0,
                    _ => {}
                }
                (stack.count > 0).then_some(stack)
            }
            _ => stack,
        }
    }
    /// `ItemStack.getMaxStackSize`: the max_stack_size component, else the
    /// item's.
    pub(in crate::game) fn stack_max_size(&self, s: &ItemStack) -> u32 {
        if let Some(Tag::Int(n)) = s.data().and_then(|d| d.other("minecraft:max_stack_size")) {
            return (*n).clamp(1, 99) as u32;
        }
        self.items().get(&s.item).map_or(64, |i| i.max_stack)
    }
    /// `ResourceOrIdArgument.getItemModifier`: an inline modifier (SNBT) or
    /// a registered one.
    pub(super) fn item_modifier_arg(&mut self, ctx: &Context, name: &str) -> R<serde_json::Value> {
        let raw = match ctx.get(name) {
            Some(Arg::Resource(s) | Arg::Str(s)) => s.clone(),
            _ => return Err(err("command.failed", &[])),
        };
        if raw.starts_with('{') || raw.starts_with('[') {
            let (tag, _) = horizon_world::nbt::parse_snbt(&raw)
                .map_err(|e| err("argument.nbt.invalid", &[&e]))?;
            return Ok(horizon_client::scoreboard::tag_to_json(&tag));
        }
        let id = if raw.contains(':') {
            raw
        } else {
            format!("minecraft:{raw}")
        };
        self.fns
            .store
            .item_modifiers
            .get(&id)
            .cloned()
            .ok_or_else(|| err("argument.item_modifier.unknown", &[&id]))
    }
    /// `/item modify (entity|block) <target> <slot> <modifier>`.
    pub(super) fn cmd_item_modify(&mut self, ctx: &Context) -> R<i32> {
        self.ensure_datapacks();
        let targets = self.owner_of(ctx, "target", true)?;
        let Some(Arg::Str(slot_name)) = ctx.get("slot").cloned() else {
            return Ok(0);
        };
        let modifier = self.item_modifier_arg(ctx, "modifier")?;
        let source = self.command_source.unwrap_or_else(|| self.player_source());
        let view = source.entity.map(|t| self.entity_view_asked(t, &modifier));
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as i64);
        let mut rng = horizon_core::random::LegacyRandom::new(seed);
        let mut tracker = Tracker::default();
        let mut selected = 0;
        for o in &targets {
            let slots = self.command_slot_candidates(o, &slot_name);
            if slots.is_empty() {
                if let Owner::Block(p) = o
                    && self.block_container_size(*p).is_none()
                {
                    return Err(err(
                        "commands.item.target.not_a_container",
                        &[&p[0], &p[1], &p[2]],
                    ));
                }
                continue;
            }
            let mut changed = 0;
            for candidate in &slots {
                if let Slot::Contents(parent, component) = &candidate.slot {
                    changed += self.modify_contents(
                        parent,
                        *component,
                        candidate,
                        &mut selected,
                        |g, current| {
                            g.apply_item_modifier(
                                &modifier,
                                Some(current),
                                &mut rng,
                                view.as_ref(),
                                source.pos,
                                0,
                            )
                            .map(|mut s| {
                                s.count = s.count.min(g.stack_max_size(&s));
                                s
                            })
                        },
                    );
                    continue;
                }
                if !self.select_slot(candidate, &mut selected, true) {
                    continue;
                }
                let slot = &candidate.slot;
                // SlotSelector.NON_EMPTY_SLOTS
                let Some(current) = self.slot_get(slot) else {
                    continue;
                };
                let new = self.apply_item_modifier(
                    &modifier,
                    Some(current.clone()),
                    &mut rng,
                    view.as_ref(),
                    source.pos,
                    0,
                );
                // CountingModifier: every slot set counts, changed or not;
                // ItemStack.limitSize to the (new) max stack size.
                let new = new.map(|mut s| {
                    let max = self.stack_max_size(&s);
                    s.count = s.count.min(max);
                    s
                });
                if self.equipment_slot_accepts(slot, new.as_ref()) && self.slot_set(slot, new) {
                    changed += 1;
                }
            }
            let h = self.owner_holder(o);
            tracker.track(&h, changed);
        }
        if selected == 0 {
            return Err(
                if slot_name.starts_with('{') || slot_name.starts_with('[') {
                    err("commands.item.target.no_such_slot.unnamed", &[])
                } else {
                    err("commands.item.target.no_such_slot", &[&slot_name])
                },
            );
        }
        let pos = match targets.first() {
            Some(Owner::Block(p)) => Some(*p),
            _ => None,
        };
        let (message, total) = tracker.message(
            false,
            Some(err("commands.item.target.failed", &[])),
            |h, count| {
                let n = Text::literal(count.to_string());
                match pos {
                    Some(p) => Text::translate(
                        "commands.item.block.modify.success",
                        &[
                            n,
                            Text::literal(p[0].to_string()),
                            Text::literal(p[1].to_string()),
                            Text::literal(p[2].to_string()),
                        ],
                    ),
                    None => Text::translate(
                        "commands.item.entity.modify.success.single",
                        &[n, h.text.clone()],
                    ),
                }
            },
            |c, _| {
                Text::translate(
                    "commands.item.entity.modify.success.multiple",
                    &[Text::literal(c.to_string())],
                )
            },
        )?;
        self.feedback(message);
        Ok(total)
    }
    /// `/item replace|fill|override <target> <slots> (with <item> [count] | from <source> <slots>)`.
    pub(super) fn cmd_item(&mut self, ctx: &Context) -> R<i32> {
        let mode = ctx
            .literals
            .iter()
            .find(|l| matches!(l.as_str(), "replace" | "fill" | "override"))
            .cloned()
            .unwrap_or_default();
        let targets = self.owner_of(ctx, "target", true)?;
        let Some(Arg::Str(slot_name)) = ctx.get("slots").cloned() else {
            return Ok(0);
        };
        // The items to place.
        let (items, known): (Vec<Option<ItemStack>>, Option<ItemStack>) = if ctx.has("with") {
            let stack = self.item_from_input(ctx)?;
            (vec![stack.clone()], stack)
        } else {
            let sources = self.owner_of(ctx, "source", true)?;
            let Some(Arg::Str(source_slots)) = ctx.get("sourceSlots").cloned() else {
                return Ok(0);
            };
            let mut found = Vec::new();
            for o in &sources {
                for s in self.command_slots(o, &source_slots) {
                    found.push(self.slot_get(&s));
                }
            }
            if found.is_empty() {
                return Err(match &sources[..] {
                    // ItemCommands.getContainer: a block entity that is no Container
                    // (a sign, a lectern, a beacon) is no source either.
                    [Owner::Block(p)] if self.block_container_size(*p).is_none() => err(
                        "commands.item.source.not_a_container",
                        &[&p[0], &p[1], &p[2]],
                    ),
                    _ if source_slots.starts_with('{') || source_slots.starts_with('[') => {
                        err("commands.item.source.no_such_slot.unnamed", &[])
                    }
                    _ => err("commands.item.source.no_such_slot", &[&source_slots]),
                });
            }
            // ItemCommands.applyModifier on each source stack.
            if ctx.get("modifier").is_some() {
                let modifier = self.item_modifier_arg(ctx, "modifier")?;
                let source = self.command_source.unwrap_or_else(|| self.player_source());
                let view = source.entity.map(|t| self.entity_view_asked(t, &modifier));
                let mut rng = horizon_core::random::LegacyRandom::new(0);
                found = found
                    .into_iter()
                    .map(|s| {
                        s.and_then(|s| {
                            self.apply_item_modifier(
                                &modifier,
                                Some(s),
                                &mut rng,
                                view.as_ref(),
                                source.pos,
                                0,
                            )
                        })
                    })
                    .collect();
            }
            // A source with nothing in the slot contributes air.
            (found, None)
        };
        let mut tracker = Tracker::default();
        let mut selected = 0;
        for o in &targets {
            let slots = self.command_slot_candidates(o, &slot_name);
            if slots.is_empty() {
                if let Owner::Block(p) = o
                    && self.block_container_size(*p).is_none()
                {
                    return Err(err(
                        "commands.item.target.not_a_container",
                        &[&p[0], &p[1], &p[2]],
                    ));
                }
                continue;
            }
            let mut changed = 0;
            let mut i = 0;
            for candidate in &slots {
                if let Slot::Contents(parent, component) = &candidate.slot {
                    changed += self.replace_contents(
                        parent,
                        *component,
                        candidate,
                        &mut selected,
                        &mut contents::Provider::new(&items, &mut i, &mode),
                    );
                    continue;
                }
                if !self.select_slot(candidate, &mut selected, false) {
                    continue;
                }
                let slot = &candidate.slot;
                // `fill` cycles through the items; `replace` stops when they
                // run out; `override` empties the rest.
                let item = match mode.as_str() {
                    "fill" => items.get(i % items.len()).cloned().flatten(),
                    "override" => items.get(i).cloned().flatten(),
                    _ => match items.get(i) {
                        Some(x) => x.clone(),
                        None => break,
                    },
                };
                i += 1;
                // ItemStack.isEmpty: air (or a count of 0) is an empty slot.
                // SlotAccess.set counts every slot it sets, the same item too.
                let item = item.filter(|s| &*s.item != "minecraft:air" && s.count > 0);
                if self.equipment_slot_accepts(slot, item.as_ref()) && self.slot_set(slot, item) {
                    changed += 1;
                }
            }
            let h = self.owner_holder(o);
            tracker.track(&h, changed);
        }
        if selected == 0 {
            return Err(
                if slot_name.starts_with('{') || slot_name.starts_with('[') {
                    err("commands.item.target.no_such_slot.unnamed", &[])
                } else {
                    err("commands.item.target.no_such_slot", &[&slot_name])
                },
            );
        }
        let block = matches!(targets.first(), Some(Owner::Block(_)));
        let pos = match targets.first() {
            Some(Owner::Block(p)) => *p,
            _ => [0; 3],
        };
        let zero = match &known {
            Some(k) => err(
                "commands.item.target.failed.known_item",
                // The ItemStack itself is the argument: ItemStack.toString.
                &[&format!("{} {}", k.count, k.item)],
            ),
            None => err("commands.item.target.failed", &[]),
        };
        let name = known.as_ref().map(|k| self.stack_name(k));
        let (message, total) = tracker.message(
            false,
            Some(zero),
            |h, count| {
                let n = Text::literal(count.to_string());
                match (&name, block) {
                    (Some(item), true) => Text::translate(
                        "commands.item.block.replace.success.known_item",
                        &[
                            n,
                            Text::literal(pos[0].to_string()),
                            Text::literal(pos[1].to_string()),
                            Text::literal(pos[2].to_string()),
                            item.clone(),
                        ],
                    ),
                    (None, true) => Text::translate(
                        "commands.item.block.replace.success",
                        &[
                            n,
                            Text::literal(pos[0].to_string()),
                            Text::literal(pos[1].to_string()),
                            Text::literal(pos[2].to_string()),
                        ],
                    ),
                    (Some(item), false) => Text::translate(
                        "commands.item.entity.replace.success.single.known_item",
                        &[n, h.text.clone(), item.clone()],
                    ),
                    (None, false) => Text::translate(
                        "commands.item.entity.replace.success.single",
                        &[n, h.text.clone()],
                    ),
                }
            },
            |c, _| match &name {
                Some(item) => Text::translate(
                    "commands.item.entity.replace.success.multiple.known_item",
                    &[Text::literal(c.to_string()), item.clone()],
                ),
                None => Text::translate(
                    "commands.item.entity.replace.success.multiple",
                    &[Text::literal(c.to_string())],
                ),
            },
        )?;
        self.feedback(message);
        Ok(total)
    }
    /// `if items` and `if slots`: how many items match / how many slots.
    pub(super) fn condition_items(&mut self, c: &Context) -> R<(bool, i32)> {
        let owners = self.owner_of(c, "source", true)?;
        let Some(Arg::Str(slot_name)) = c.get("slots").cloned() else {
            return Ok((false, 0));
        };
        // SlotSourceArgument keeps literal ranges ahead of registered holders.
        // Only condition reads resolve named definitions here: the item write
        // candidate adapters retain their existing supported scope.
        let registered = if slot_name.starts_with('{')
            || slot_name.starts_with('[')
            || horizon_client::command::args::slot_ranges()
                .iter()
                .any(|(name, _)| name == &slot_name)
        {
            None
        } else {
            let id = if slot_name.contains(':') {
                slot_name.clone()
            } else {
                format!("minecraft:{slot_name}")
            };
            self.fns.store.slot_sources.get(&id).cloned()
        };
        let predicate = match c.get("item_predicate") {
            Some(Arg::Item(p)) => Some(p.clone()),
            _ => None,
        };
        let mut count = 0;
        for o in owners {
            if let Owner::Block(p) = o
                && self.block_container_size(p).is_none()
            {
                return Err(err(
                    "commands.item.source.not_a_container",
                    &[&p[0], &p[1], &p[2]],
                ));
            }
            let slots = match &registered {
                Some(source) => self.inline_slots(&o, source),
                None => self.command_slots(&o, &slot_name),
            };
            if predicate.is_none() {
                count += slots.len() as i32;
                continue;
            }
            for s in slots {
                if let Some(stack) = self.slot_get(&s)
                    && predicate
                        .as_ref()
                        .is_some_and(|p| self.stack_matches(p, &stack))
                {
                    count += stack.count as i32;
                }
            }
        }
        Ok((count > 0, count))
    }
}

/// Edit a stack's components as their saved compound.
fn edit_components(stack: &mut ItemStack, f: impl FnOnce(&mut horizon_world::nbt::Compound)) {
    let mut c = match horizon_client::item_data::encode_components(stack) {
        Some(Tag::Compound(c)) => c,
        _ => horizon_world::nbt::Compound::new(),
    };
    f(&mut c);
    let tag = Tag::Compound(c);
    let (damage, extra) = horizon_client::item_data::decode_components(Some(&tag));
    stack.damage = damage;
    stack.extra = extra;
    stack.tidy();
}
/// `CompoundTag.merge`: nested compounds merge, other values replace.
fn merge_compound(into: &mut Tag, from: &Tag) {
    let (Tag::Compound(a), Tag::Compound(b)) = (into, from) else {
        return;
    };
    for (k, v) in b {
        match (a.get_mut(k), v) {
            (Some(existing @ Tag::Compound(_)), Tag::Compound(_)) => merge_compound(existing, v),
            _ => {
                a.insert(k.clone(), v.clone());
            }
        }
    }
}
/// `ListOperation.apply` (`mode`: replace_all, replace_section, insert,
/// append) with a 256-entry limit.
fn list_operation(m: &serde_json::Value, old: Vec<Tag>, new: Vec<Tag>) -> Vec<Tag> {
    let offset = m["offset"].as_u64().unwrap_or(0) as usize;
    let mut out = match m["mode"].as_str().unwrap_or("") {
        "replace_all" => new,
        "insert" => {
            let at = offset.min(old.len());
            let mut out = old[..at].to_vec();
            out.extend(new);
            out.extend_from_slice(&old[at..]);
            out
        }
        "replace_section" => {
            let at = offset.min(old.len());
            let size = m["size"].as_u64().map_or(new.len(), |s| s as usize);
            let mut out = old[..at].to_vec();
            out.extend(new);
            let end = (at + size).min(old.len());
            out.extend_from_slice(&old[end..]);
            out
        }
        _ => {
            let mut out = old;
            out.extend(new);
            out
        }
    };
    out.truncate(256);
    out
}

#[cfg(test)]
mod stand_equip_event_tests {
    use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
    use horizon_client::{chunk::Chunk, containers::BlockEntity, mobs::Who, sculk::Info};
    use horizon_core::position::ChunkPos;
    use std::sync::Arc;

    fn candidate(game: &Game<'_>) -> Option<Info> {
        let Some(BlockEntity::Sculk(sensor)) = game.world.block_entities.get(&(9, 200, 10)) else {
            panic!("actual sensor must be registered")
        };
        sensor.vibration.selector.current.map(|(info, _)| info)
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn commands_post_stand_equipment_events_after_its_first_tick() {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.game_dir =
            Some(std::env::temp_dir().join(format!("horizon-h130-unit-{}", std::process::id())));
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        game.set_allow_commands(true);
        let registry = game.pack.assets.registry;
        let sensor = registry.default_state("minecraft:sculk_sensor").unwrap();
        let mut chunk = Chunk::empty(0, 0);
        chunk.set(9, 200, 10, sensor.0);
        game.world.chunks.insert((0, 0), Arc::new(chunk));
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(0, 0))
            .unwrap();
        assert!(game.run_command("forceload add 0 0"));
        assert!(game.run_command("summon armor_stand 6.5 200 10.5 {UUID:[I;130,0,0,1],NoGravity:1b,attributes:[{id:\"minecraft:movement_speed\",base:0.1d}]}"));
        let id = game.armor_stands[0].id;
        assert_eq!(game.uuid_for(id), [130, 0, 0, 1]);
        assert!(
            game.armor_stands[0].speed_instance,
            "a loaded attribute is not firstTick"
        );
        game.sculk_prepare();
        game.sculk_sync_index();
        assert!(candidate(&game).is_none());
        let prefix = "item replace entity 00000082-0000-0000-0000-000000000001 ";
        for item in ["iron_helmet", "air"] {
            assert!(game.run_command(&format!("{prefix}armor.head with {item}")));
            assert!(candidate(&game).is_none(), "first-tick suppression");
        }
        game.tick_armor_stands().unwrap();
        assert!(
            game.run_command(
                "data merge entity 00000082-0000-0000-0000-000000000001 {ShowArms:1b}"
            )
        );
        for (slot, item, event) in [
            ("armor.head", "iron_helmet", Some("minecraft:equip")),
            ("armor.head", "iron_helmet", None),
            ("armor.head", "air", Some("minecraft:unequip")),
            ("armor.head", "air", None),
            ("weapon.mainhand", "stick", Some("minecraft:unequip")),
        ] {
            let Some(BlockEntity::Sculk(sensor)) = game.world.block_entities.get_mut(&(9, 200, 10))
            else {
                unreachable!()
            };
            sensor.vibration = Default::default();
            assert!(game.run_command(&format!("{prefix}{slot} with {item}")));
            let heard = candidate(&game);
            assert_eq!(heard.map(|info| info.event), event, "{slot} -> {item}");
            if let Some(info) = heard {
                assert_eq!(info.source, Some(Who::Mob(id)));
                assert_eq!(info.pos, [6.5, 200.0, 10.5]);
                assert!(info.owner.is_none());
            }
        }
    }
}

#[cfg(test)]
mod mob_equipment_command_tests {
    use crate::game::commands::{Target, data::DataRef};
    use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
    use horizon_client::chunk::Chunk;
    use horizon_core::position::ChunkPos;
    use std::sync::Arc;

    fn with_game(test: impl FnOnce(&mut Game<'_>)) {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.game_dir = Some(
            std::path::PathBuf::from(std::env::var_os("HORIZON_HOME").unwrap())
                .join(format!("h025-{:?}", std::thread::current().id())),
        );
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        game.set_allow_commands(true);

        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(0, 0))
            .unwrap();
        assert!(game.run_command("difficulty normal"));
        assert!(game.run_command("forceload add 0 0"));
        test(&mut game);
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn world_pack_enchantments_reach_actual_game_catalog() {
        use horizon_client::enchant::EffectContext;
        use horizon_core::random::LegacyRandom;
        with_game(|game| {
            let clock = game.world.game_time;
            game.world_root = std::path::PathBuf::from(std::env::var_os("HORIZON_HOME").unwrap())
                .join("enchantment-catalog-world");
            let pack = game.world_root.join("datapacks/catalog");
            let definitions = pack.join("data/wpenchant/enchantment");
            std::fs::create_dir_all(&definitions).unwrap();
            std::fs::write(
                pack.join("pack.mcmeta"),
                r#"{"pack":{"description":"World enchantment catalog fixture"}}"#,
            )
            .unwrap();
            let definition = serde_json::json!({
                "description": "World catalog loaded", "supported_items": "minecraft:diamond_sword",
                "weight": 1, "max_level": 1,
                "min_cost": {"base": 1, "per_level_above_first": 0},
                "max_cost": {"base": 1, "per_level_above_first": 0},
                "anvil_cost": 1, "slots": ["mainhand"],
                "effects": {"minecraft:damage": [{"effect": {"type": "minecraft:add", "value": 2.0}}]}
            });
            std::fs::write(
                definitions.join("loaded.json"),
                serde_json::to_vec(&definition).unwrap(),
            )
            .unwrap();
            std::fs::write(
                game.world_root.join("datapacks.json"),
                r#"{"enabled":["file/catalog"]}"#,
            )
            .unwrap();
            let functions = pack.join("data/wpenchant/function");
            std::fs::create_dir_all(&functions).unwrap();
            std::fs::write(functions.join("ready.mcfunction"), "return 1\n").unwrap();
            game.reload_datapacks(false);
            assert!(game.fns.store.functions.contains_key("wpenchant:ready"));
            game.reload_world_tables();
            let raw_present = game
                .pack_files()
                .kind("enchantment")
                .any(|(id, files)| id == "wpenchant:loaded" && !files.is_empty());
            assert!(
                raw_present,
                "pack function admitted, but enchantment raw file absent; actual catalog entry present={}",
                game.enchantments().get("wpenchant:loaded").is_some()
            );
            assert!(std::sync::Arc::ptr_eq(
                &game.items().enchantments,
                &game.stations().enchantments
            ));
            assert!(game.enchantments().get("minecraft:mending").is_some());
            assert!(game.enchantments().get("wpenchant:missing").is_none());
            let entry = game
                .enchantments()
                .get("wpenchant:loaded")
                .expect("the active Game catalog must adopt the loaded world-pack definition");
            assert_eq!(entry.description, "World catalog loaded");
            assert_eq!(entry.max_level, 1);
            assert!(game.enchantments().can_enchant(
                game.items(),
                "wpenchant:loaded",
                "minecraft:diamond_sword"
            ));
            assert!(!game.enchantments().can_enchant(
                game.items(),
                "wpenchant:loaded",
                "minecraft:stone"
            ));
            assert_eq!(
                game.enchantments().modify_value_with(
                    "wpenchant:loaded",
                    1,
                    "minecraft:damage",
                    &EffectContext::default(),
                    &mut LegacyRandom::new(0),
                    1.0
                ),
                3.0
            );
            assert_eq!(game.world.game_time, clock);
        });
    }

    fn actor(game: &mut Game<'_>, kind: &str, n: i32) -> (String, u32) {
        let uuid = format!("00000019-0000-0000-0000-{n:012x}");
        assert!(game.run_command(&format!("summon {kind} 4.5 200 5.5 {{UUID:[I;25,0,0,{n}],NoAI:1b,NoGravity:1b,Silent:1b,h025_raw:7,Tags:[h025]}}")), "{kind}/{n}: {:?}", game.ui.chat.recent_lines(3));
        let mob = game
            .mobs
            .iter()
            .find(|m| game.persist.uuid_of(m.id) == Some([25, 0, 0, n]))
            .unwrap();
        assert!(mob.no_ai);
        (uuid, mob.id)
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn body_commands_reject_wrong_items_without_changing_head() {
        with_game(|game| {
            for (n, kind, slot, item) in [
                (1, "horse", "armor.body", "iron_horse_armor"),
                (2, "wolf", "armor.body", "wolf_armor"),
                (3, "husk", "armor.head", "iron_helmet"),
            ] {
                let (uuid, id) = actor(game, kind, n);
                assert!(
                    game.run_command(&format!("item replace entity {uuid} {slot} with {item}"))
                );
                let mob = game.mobs.iter().find(|m| m.id == id).unwrap();
                let actual = if slot == "armor.body" {
                    &mob.extra.body
                } else {
                    &mob.extra.head
                };
                assert_eq!(
                    actual.as_deref(),
                    Some(format!("minecraft:{item}").as_str())
                );
            }
            for (n, kind) in [(4, "husk"), (5, "pig")] {
                let (uuid, id) = actor(game, kind, n);
                assert!(
                    game.run_command(&format!("item replace entity {uuid} armor.head with stone"))
                );
                assert!(
                    !game.run_command(&format!(
                        "item replace entity {uuid} armor.body with diamond_chestplate"
                    )),
                    "body must reject a chest item"
                );
                let mob = game.mobs.iter().find(|m| m.id == id).unwrap();
                assert!(mob.extra.body.is_none());
                assert_eq!(mob.extra.head.as_deref(), Some("minecraft:stone"));
                let saved = game.data_of(&DataRef::Entity(Target::Mob(id))).unwrap();
                assert!(saved.get("equipment").unwrap().get("body").is_none());
                assert_eq!(
                    saved
                        .get("equipment")
                        .unwrap()
                        .get("head")
                        .unwrap()
                        .get("id"),
                    Some(&horizon_world::nbt::Tag::String("minecraft:stone".into()))
                );
            }
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn saddle_commands_round_trip_existing_model_and_saved_equipment() {
        use super::Slot;
        use horizon_world::nbt::Tag;
        with_game(|game| {
            // Pig is the existing species saver; Husk needs the common projection.
            for (n, kind) in [(6, "pig"), (7, "husk")] {
                let (uuid, id) = actor(game, kind, n);
                assert!(game.slot_get(&Slot::Mob(id, "saddle")).is_none());
                assert!(
                    game.run_command(&format!("item replace entity {uuid} armor.head with stone"))
                );
                assert!(
                    game.run_command(&format!("item replace entity {uuid} saddle with saddle")),
                    "command must reach the existing saddle model"
                );
                assert_eq!(
                    game.slot_get(&Slot::Mob(id, "saddle"))
                        .unwrap()
                        .item
                        .as_ref(),
                    "minecraft:saddle"
                );
                let saved = game.data_of(&DataRef::Entity(Target::Mob(id))).unwrap();
                assert_eq!(
                    saved
                        .get("equipment")
                        .unwrap()
                        .get("saddle")
                        .unwrap()
                        .get("id"),
                    Some(&Tag::String("minecraft:saddle".into()))
                );
                assert_eq!(
                    saved
                        .get("equipment")
                        .unwrap()
                        .get("head")
                        .unwrap()
                        .get("id"),
                    Some(&Tag::String("minecraft:stone".into()))
                );
                assert_eq!(saved.get("h025_raw"), Some(&Tag::Int(7)));
                assert_eq!(saved.get("Silent"), Some(&Tag::Byte(1)));
                assert_eq!(
                    saved.get("Tags"),
                    Some(&Tag::List(vec![Tag::String("h025".into())]))
                );
                assert!(
                    !game.run_command(&format!("item replace entity {uuid} saddle with stick"))
                );
                assert_eq!(
                    game.slot_get(&Slot::Mob(id, "saddle"))
                        .unwrap()
                        .item
                        .as_ref(),
                    "minecraft:saddle"
                );
                assert!(game.run_command(&format!("item replace entity {uuid} saddle with air")));
                assert!(game.slot_get(&Slot::Mob(id, "saddle")).is_none());
            }
        });
    }
    /// `SulfurCube.getEquipmentSlotForItem`: a `#sulfur_cube_swallowable`
    /// item is a body item, so the body's `SlotAccess` takes it (#12038).
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_sulfur_cubes_body_takes_a_swallowable_block() {
        with_game(|game| {
            assert!(game.run_command("summon sulfur_cube 4.5 200 4.5 {NoAI:1b,Tags:[\"cube\"]}"));
            let body = |game: &Game<'_>| {
                let m = game
                    .mobs
                    .iter()
                    .find(|m| m.kind.id == "minecraft:sulfur_cube");
                m.and_then(|m| m.extra.body.clone())
            };
            let replace = "item replace entity @e[tag=cube,limit=1] armor.body with";
            assert!(game.run_command(&format!("{replace} white_wool")));
            assert_eq!(body(game).as_deref(), Some("minecraft:white_wool"));
            // An item outside the tag stays out, and the body stays on.
            assert!(!game.run_command(&format!("{replace} diamond")));
            assert_eq!(body(game).as_deref(), Some("minecraft:white_wool"));
            // An empty stack always fits: the body comes off.
            assert!(game.run_command(&format!("{replace} air")));
            assert_eq!(body(game), None);
        });
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn effective_item_composition_rejects_before_destination_mutation() {
        use super::Slot;
        with_game(|game| {
            assert!(game.run_command("setblock 4 200 4 chest"));
            let slot = Slot::Block([4, 200, 4], 0);
            for item in [
                "iron_helmet[max_stack_size=2]",
                "iron_helmet[max_stack_size=99]",
                "stone[max_damage=10,max_stack_size=2]",
            ] {
                assert!(game.run_command("item replace block 4 200 4 container.0 with emerald[custom_data={sentinel:86}] 4"));
                let before = game.slot_get(&slot).unwrap();
                assert!(
                    !game.run_command(&format!(
                        "item replace block 4 200 4 container.0 with {item}"
                    )),
                    "invalid effective composition must reject {item}"
                );
                assert_eq!(game.slot_get(&slot), Some(before));
            }
            for item in [
                "iron_helmet",
                "stone[max_stack_size=2]",
                "stone[max_stack_size=99]",
                "iron_helmet[!max_damage,max_stack_size=2]",
            ] {
                assert!(
                    game.run_command(&format!(
                        "item replace block 4 200 4 container.0 with {item}"
                    )),
                    "valid effective composition must accept {item}"
                );
            }
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn effective_item_validation_view_uses_loaded_prototypes_and_typed_removals() {
        use horizon_world::nbt::Tag;
        with_game(|game| {
            let damage = "minecraft:max_damage".to_owned();
            let size = "minecraft:max_stack_size".to_owned();
            let helmet = game.items().get("minecraft:iron_helmet").unwrap();
            assert!(helmet.components.contains(&damage));
            let plain = game
                .item_validation_components("minecraft:iron_helmet", None)
                .unwrap();
            assert_eq!(
                plain.get(&damage),
                Some(&Tag::Int(helmet.max_damage as i32))
            );
            assert_eq!(plain.get(&size), Some(&Tag::Int(helmet.max_stack as i32)));
            let removed = game
                .item_validation_components(
                    "minecraft:iron_helmet",
                    Some("[!max_damage,max_stack_size=2,custom_data={keep:86}]"),
                )
                .unwrap();
            assert!(removed.get(&damage).is_none());
            assert_eq!(removed.get(&size), Some(&Tag::Int(2)));
            assert!(removed.get(&"minecraft:custom_data".to_owned()).is_some());
            let fallback = game
                .item_validation_components("minecraft:iron_helmet", Some("[!max_stack_size]"))
                .unwrap();
            assert!(fallback.get(&damage).is_some());
            assert_eq!(
                horizon_world::datafix::dynamic::as_int(fallback.get(&size), 1),
                1
            );
            let stone = game
                .item_validation_components("minecraft:stone", None)
                .unwrap();
            assert!(stone.get(&damage).is_none());
            assert_eq!(
                stone.get(&size),
                Some(&Tag::Int(
                    game.items().get("minecraft:stone").unwrap().max_stack as i32
                ))
            );
        });
    }
}

#[cfg(test)]
mod mob_equipment_stack_tests {
    use crate::game::commands::{Target, data::DataRef};
    use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
    use horizon_client::{chunk::Chunk, mobs::finalize};
    use horizon_core::position::ChunkPos;
    use horizon_world::nbt::Tag;
    use std::sync::Arc;
    fn with_game(test: impl FnOnce(&mut Game<'_>)) {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.game_dir = Some(
            std::path::PathBuf::from(std::env::var_os("HORIZON_HOME").unwrap())
                .join(format!("h023-{:?}", std::thread::current().id())),
        );
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        game.set_allow_commands(true);

        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(0, 0))
            .unwrap();
        assert!(game.run_command("difficulty normal"));
        assert!(game.run_command("forceload add 0 0"));
        test(&mut game);
    }

    fn actor(game: &mut Game<'_>, kind: &str, n: i32) -> (String, u32) {
        let uuid = format!("00000017-0000-0000-0000-{n:012x}");
        assert!(game.run_command(&format!("summon {kind} 4.5 200 5.5 {{UUID:[I;23,0,0,{n}],NoAI:1b,NoGravity:1b,Silent:1b,h023_raw:7,Tags:[h023]}}")), "{kind}/{n}: {:?}", game.ui.chat.recent_lines(3));
        let mob = game
            .mobs
            .iter()
            .find(|m| game.persist.uuid_of(m.id) == Some([23, 0, 0, n]))
            .unwrap();
        assert!(mob.no_ai);
        (uuid, mob.id)
    }

    fn saved(game: &mut Game<'_>, id: u32) -> Tag {
        game.data_of(&DataRef::Entity(Target::Mob(id))).unwrap()
    }
    fn audit(stack: &horizon_client::items::ItemStack, value: i32) {
        assert_eq!(
            stack.custom_data().and_then(|c| c.get(&"audit".into())),
            Some(&Tag::Int(value))
        );
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn offhand_commands_adopt_and_copy_the_whole_stack() {
        use super::Slot;
        with_game(|game| {
            let (uuid, id) = actor(game, "husk", 1);
            assert!(game.run_command("setblock 4 200 4 chest"));
            assert!(game.slot_get(&Slot::Mob(id, "offhand")).is_none());
            assert!(game.run_command(&format!(
                "item replace entity {uuid} weapon.mainhand with stone"
            )));
            assert!(game.run_command(&format!(
                "item replace entity {uuid} weapon.offhand with stick[custom_data={{audit:7}}] 3"
            )));
            let mob = game.mobs.iter().find(|m| m.id == id).unwrap();
            assert_eq!(mob.extra.off_hand.as_deref(), Some("minecraft:stick"));
            let adopted = finalize::item_stack(mob, finalize::OFFHAND).unwrap();
            assert_eq!(
                adopted.count, 3,
                "actual command adoption must retain count"
            );
            audit(&adopted, 7);
            let tag = saved(game, id);
            assert_eq!(
                tag.get("equipment")
                    .unwrap()
                    .get("offhand")
                    .unwrap()
                    .get("count"),
                Some(&Tag::Int(3))
            );
            assert_eq!(
                tag.get("equipment")
                    .unwrap()
                    .get("mainhand")
                    .unwrap()
                    .get("id"),
                Some(&Tag::String("minecraft:stone".into()))
            );
            assert!(game.run_command(&format!(
                "item replace block 4 200 4 container.0 from entity {uuid} weapon.offhand"
            )));
            let copied = game.slot_get(&Slot::Block([4, 200, 4], 0)).unwrap();
            assert_eq!(copied.count, 3);
            audit(&copied, 7);
            assert!(game.run_command(&format!(
                "item replace entity {uuid} weapon.offhand with air"
            )));
            assert!(game.slot_get(&Slot::Mob(id, "offhand")).is_none());
            assert!(game.run_command(&format!(
                "item replace entity {uuid} weapon.offhand with stick"
            )));
            let plain = game.slot_get(&Slot::Mob(id, "offhand")).unwrap();
            assert_eq!(plain.count, 1);
            assert!(
                plain.custom_data().is_none(),
                "stale components must not return for the same id"
            );
        });
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn armor_command_copy_keeps_the_already_adopted_stack() {
        use super::Slot;
        with_game(|game| {
            let (uuid, id) = actor(game, "husk", 2);
            assert!(game.run_command("setblock 4 200 4 chest"));
            assert!(game.run_command(&format!("item replace entity {uuid} armor.head with iron_helmet[damage=3,custom_data={{audit:8}}]")));
            let mob = game.mobs.iter().find(|m| m.id == id).unwrap();
            let adopted = mob.extra.armor[0].as_ref().unwrap();
            assert_eq!(
                adopted.damage, 3,
                "existing armor state already adopts the full stack"
            );
            audit(adopted, 8);
            assert!(game.run_command(&format!(
                "item replace block 4 200 4 container.0 from entity {uuid} armor.head"
            )));
            let copied = game.slot_get(&Slot::Block([4, 200, 4], 0)).unwrap();
            assert_eq!(
                copied.damage, 3,
                "actual command copy must use the full stack"
            );
            audit(&copied, 8);
            let tag = saved(game, id);
            let components = tag
                .get("equipment")
                .unwrap()
                .get("head")
                .unwrap()
                .get("components")
                .unwrap();
            assert_eq!(components.get("minecraft:damage"), Some(&Tag::Int(3)));
            assert_eq!(
                components
                    .get("minecraft:custom_data")
                    .unwrap()
                    .get("audit"),
                Some(&Tag::Int(8))
            );
            assert!(game.run_command(&format!(
                "item replace entity {uuid} armor.head with iron_helmet"
            )));
            let plain = game.slot_get(&Slot::Mob(id, "head")).unwrap();
            assert_eq!(plain.damage, 0);
            assert!(plain.custom_data().is_none());
            assert!(game.run_command(&format!("item replace entity {uuid} armor.head with air")));
            assert!(game.slot_get(&Slot::Mob(id, "head")).is_none());
        });
    }
}

#[cfg(test)]
mod builtin_component_delivery_tests {
    use crate::game::{Game, lan::LanHost};
    use horizon_client::{
        items::ItemStack,
        lan::{Joined, NetConfig, Out, PlayerSender, guest::Guest, hub::HubConfig},
    };
    use horizon_core::position::ChunkPos;
    use horizon_protocol::{
        buf::{Codec, Reader, Writer},
        item::{Slot, component},
        packets::{configuration::ClientInformation, play},
    };
    use horizon_world::nbt::Tag;
    use std::sync::{Arc, atomic::AtomicI32, mpsc};

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn h283_actual_command_inventory_bridge_encodes_four_builtin_components() {
        let mut game = crate::game::native_machines::tests::server_game();
        game.build_registries();
        game.set_allow_commands(true);
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(0, 0))
            .unwrap();
        assert!(game.run_command("fill 0 199 0 15 199 15 stone"));
        assert!(game.world.loaded(8, 8));
        game.lan_host(
            crate::game::lan::LanSettings {
                port: 0,
                guest_commands: true,
                force_game_mode: false,
            },
            NetConfig {
                announce: false,
                ..NetConfig::loopback(0, "component-wire-test".into(), 2)
            },
            HubConfig {
                default_game_mode: 0,
                guest_commands: true,
                force_game_mode: false,
                hardcore: false,
                view_distance: 2,
                simulation_distance: 2,
                dimension: "minecraft:overworld".into(),
                allow_flight: false,
                seed: 0,
                max_players: 2,
                integrated: false,
                levels: Default::default(),
            },
        )
        .unwrap();
        let (tx, rx) = mpsc::channel();
        let mut guest = Guest::new(
            Joined {
                conn: 2,
                uuid: 283,
                name: "ComponentGuest".into(),
                properties: Vec::new(),
                info: ClientInformation::new(2),
                brand: "test".into(),
                sender: PlayerSender::new(tx, Arc::default()),
                latency: Arc::new(AtomicI32::new(0)),
                address: "127.0.0.1:25565".parse().unwrap(),
                expected: false,
                configured: Default::default(),
            },
            2,
            [8.5, 200.0, 8.5],
            2,
        );
        guest.dimension = "minecraft:overworld".into();
        guest.placed = true;
        guest.loaded = true;
        guest.selected = 1;
        guest.permission = 2;
        let mut host = game.lan.take().unwrap();
        host.hub.guests.push(guest);
        let clock = game.world.game_time;
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let registries: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("registries.json")).unwrap()).unwrap();
        let id = |registry: &str, name: &str| {
            registries[registry]["entries"][name]["protocol_id"]
                .as_i64()
                .unwrap() as i32
        };
        let command = |game: &mut Game<'_>, host: &mut LanHost, input: &str| {
            assert!(
                game.as_guest(host, 2, |g| g.run_command(input)).unwrap(),
                "{input}"
            );
        };
        let capture = |host: &mut LanHost| {
            while rx.try_recv().is_ok() {}
            host.hub.send_inventory(0);
            let mut packets = Vec::new();
            for out in rx.try_iter() {
                match out {
                    Out::Packet(body) => packets.push(body),
                    Out::Packets(bodies) => packets.extend(bodies),
                    Out::Disconnect(_) => panic!("unexpected disconnect"),
                }
            }
            let mut contents = Vec::new();
            for body in packets {
                let mut r = Reader::new(&body);
                let packet = r.varint().unwrap();
                if packet == horizon_protocol::ids::play::clientbound::CONTAINER_SET_CONTENT {
                    let content = play::ContainerSetContent::read(&mut r).unwrap();
                    assert_eq!(r.remaining(), 0, "complete enclosing content payload");
                    assert_eq!(content.container, 0);
                    assert_eq!(content.items.len(), 46);
                    assert!(content.carried.is_none());
                    contents.push(content);
                }
            }
            assert_eq!(contents.len(), 1, "one actual inventory content packet");
            contents.pop().unwrap().items[9].clone().unwrap()
        };
        command(
            &mut game,
            &mut host,
            "item replace entity @s inventory.0 with stone[minecraft:custom_name=\"H283 control\"] 3",
        );
        let control = capture(&mut host);
        assert_eq!(control.item, id("minecraft:item", "minecraft:stone"));
        assert_eq!(control.count, 3);
        assert!(control.component(component::CUSTOM_NAME).is_some());
        command(
            &mut game,
            &mut host,
            "item replace entity @s inventory.0 with gold_ingot",
        );
        let plain = capture(&mut host);
        assert!(
            plain
                .component(id(
                    "minecraft:data_component_type",
                    "minecraft:provides_trim_material"
                ))
                .is_none()
        );
        assert!(plain.removed.is_empty());
        let mut failures = Vec::new();
        for (name, item, value, expected) in [
            (
                "minecraft:provides_trim_material",
                "minecraft:diamond",
                "\"minecraft:gold\"",
                Tag::String("minecraft:gold".into()),
            ),
            (
                "minecraft:provides_banner_patterns",
                "minecraft:paper",
                "\"#minecraft:pattern_item/globe\"",
                Tag::String("#minecraft:pattern_item/globe".into()),
            ),
            (
                "minecraft:can_place_on",
                "minecraft:stone",
                "{blocks:\"minecraft:dirt\"}",
                Tag::compound([("blocks", Tag::String("minecraft:dirt".into()))]),
            ),
            (
                "minecraft:can_break",
                "minecraft:diamond_pickaxe",
                "{blocks:\"minecraft:stone\",state:{}}",
                Tag::compound([
                    ("blocks", Tag::String("minecraft:stone".into())),
                    ("state", Tag::compound(std::iter::empty::<(&str, Tag)>())),
                ]),
            ),
        ] {
            command(
                &mut game,
                &mut host,
                &format!("item replace entity @s inventory.0 with {item}[{name}={value}]"),
            );
            let source: ItemStack = host.hub.guest(2).unwrap().inventory[9].clone().unwrap();
            assert_eq!(source.item.as_ref(), item);
            assert_eq!(source.count, 1);
            assert_eq!(
                source.data().unwrap().other(name),
                Some(&expected),
                "actual registered command adoption"
            );
            assert!(source.wire_patch().is_none(), "no cached wire fallback");
            let received = capture(&mut host);
            assert_eq!(received.item, id("minecraft:item", item));
            assert_eq!(received.count, 1);
            assert!(received.removed.is_empty());
            let kind = id("minecraft:data_component_type", name);
            let mut w = Writer::new();
            Slot::write(&Some(received.clone()), &mut w);
            let mut r = Reader::new(&w.data);
            let reread = Slot::read(&mut r).unwrap().unwrap();
            assert_eq!(r.remaining(), 0, "complete nested slot payload");
            assert_eq!(reread.component(kind), received.component(kind));
            if received.component(kind).is_none() {
                failures.push((name, received));
            }
            assert_eq!(host.hub.guest(2).unwrap().uuid, 283);
            assert_eq!(host.hub.guest(2).unwrap().health, 20.0);
            assert_eq!(host.hub.guest(2).unwrap().position, [8.5, 200.0, 8.5]);
            assert_eq!(game.world.game_time, clock);
        }
        assert!(
            failures.is_empty(),
            "actual command -> network_stack -> full protocol packet missing builtin components: {failures:?}"
        );
    }
}

#[cfg(test)]
mod registered_slot_source_read_tests {
    use super::*;
    #[test]
    #[ignore = "needs prepared resources and isolated homes/cwd"]
    fn registered_first_three_reads_match_inline_and_keep_the_chest() {
        let mut game = crate::game::native_machines::tests::server_game();
        game.build_registries();
        game.set_allow_commands(true);
        let root = std::env::current_dir()
            .unwrap()
            .join("registered-slot-read-world");
        assert!(!root.exists());
        let pack = root.join("datapacks/parity");
        std::fs::create_dir_all(pack.join("data/parity/slot_source")).unwrap();
        let meta = r#"{"pack":{"description":"parity slots","pack_format":121,"min_format":[121,0],"max_format":[121,0]}}"#;
        let definition = r#"{"type":"minecraft:limit_slots","limit":3,"slot_source":{"type":"minecraft:slot_range","slots":"container.*"}}"#;
        std::fs::write(pack.join("pack.mcmeta"), meta).unwrap();
        std::fs::write(
            pack.join("data/parity/slot_source/first_three.json"),
            definition,
        )
        .unwrap();
        game.world_root = root;
        game.ensure_datapacks();
        assert_eq!(
            game.fns
                .pack_file("data/parity/slot_source/first_three.json")
                .unwrap(),
            definition.as_bytes()
        );
        assert!(
            game.ui
                .registries
                .datapacks_enabled
                .iter()
                .any(|p| p == "file/parity")
        );
        assert!(game.run_command("forceload add 0 0"));
        assert!(game.run_command("setblock 0 100 0 minecraft:chest{Items:[{Slot:0b,id:\"minecraft:stone\",count:5},{Slot:1b,id:\"minecraft:dirt\",count:2},{Slot:3b,id:\"minecraft:stone\",count:7},{Slot:4b,id:\"minecraft:shulker_box\",count:1,components:{\"minecraft:container\":[{slot:0,item:{id:\"minecraft:stone\",count:9}},{slot:2,item:{id:\"minecraft:dirt\",count:1}}]}}]}"));
        let before = game.block_entity_nbt([0, 100, 0]).unwrap();
        for (slot, item, count) in [
            (0, "minecraft:stone", 5),
            (1, "minecraft:dirt", 2),
            (3, "minecraft:stone", 7),
            (4, "minecraft:shulker_box", 1),
        ] {
            let actual = game.slot_get(&Slot::Block([0, 100, 0], slot)).unwrap();
            assert_eq!((&*actual.item, actual.count), (item, count));
        }
        assert!(game.run_command("scoreboard objectives add slotprobe dummy"));
        let clock = game.world.game_time;
        let inline =
            r#"{type:"limit_slots",limit:3,slot_source:{type:"slot_range",slots:"container.*"}}"#;
        let mut failures = Vec::new();
        for source in [inline, "parity:first_three"] {
            for (kind, suffix, count) in [
                ("items", " minecraft:stone", 5),
                ("items", " *", 7),
                ("slots", "", 3),
            ] {
                assert!(game.run_command("scoreboard players set #r slotprobe -99"));
                assert!(game.run_command("scoreboard players set #s slotprobe -99"));
                let command = format!(
                    "execute store result score #r slotprobe store success score #s slotprobe run execute if {kind} block 0 100 0 {source}{suffix}"
                );
                let ok = game.run_command(&command);
                let result = game.score.board.score("#r", "slotprobe").unwrap().value;
                let success = game.score.board.score("#s", "slotprobe").unwrap().value;
                println!("{command}: accepted={ok}, result={result}, success={success}");
                if (ok, result, success) != (true, count, 1) {
                    failures.push((command, ok, result, success));
                }
                assert_eq!(game.block_entity_nbt([0, 100, 0]).as_ref(), Some(&before));
                assert_eq!(game.world.game_time, clock);
            }
        }
        for name in ["#r", "#s"] {
            assert!(game.run_command(&format!("scoreboard players set {name} slotprobe -99")));
        }
        assert!(!game.run_command("execute store result score #r slotprobe store success score #s slotprobe run execute if slots block 0 100 0 parity:missing"));
        for name in ["#r", "#s"] {
            assert_eq!(
                game.score.board.score(name, "slotprobe").unwrap().value,
                -99
            );
        }
        assert_eq!(game.block_entity_nbt([0, 100, 0]).as_ref(), Some(&before));
        assert_eq!(game.world.game_time, clock);
        assert!(
            failures.is_empty(),
            "registered real-pack reads must match inline: {failures:?}"
        );
    }
}

#[cfg(test)]
mod registered_slot_source_preservation_tests {
    use super::*;

    fn scene(name: &str) -> (Game<'static>, std::path::PathBuf) {
        let mut game = crate::game::native_machines::tests::server_game();
        game.build_registries();
        game.set_allow_commands(true);
        let root = std::env::current_dir().unwrap().join(name);
        assert!(!root.exists());
        let mut high = std::path::PathBuf::new();
        for (pack, limit) in [("a-low", 1), ("parity", 3)] {
            let dir = root.join("datapacks").join(pack);
            std::fs::create_dir_all(dir.join("data/parity/slot_source")).unwrap();
            std::fs::write(dir.join("pack.mcmeta"), r#"{"pack":{"description":"slot snapshot","pack_format":121,"min_format":[121,0],"max_format":[121,0]}}"#).unwrap();
            let path = dir.join("data/parity/slot_source/first_three.json");
            std::fs::write(&path, serde_json::json!({"type":"minecraft:limit_slots","limit":limit,"slot_source":{"type":"minecraft:slot_range","slots":"container.*"}}).to_string()).unwrap();
            high = path;
        }
        game.world_root = root;
        game.ensure_datapacks();
        assert!(game.run_command("setblock 0 100 0 chest{Items:[{Slot:0b,id:\"stone\",count:5},{Slot:1b,id:\"dirt\",count:2},{Slot:3b,id:\"stone\",count:7}]}"));
        assert!(game.run_command("scoreboard objectives add slotprobe dummy"));
        (game, high)
    }

    fn count(game: &mut Game<'_>, expected: i32) {
        assert!(game.run_command("execute store result score #r slotprobe run execute if slots block 0 100 0 parity:first_three"));
        assert_eq!(
            game.score.board.score("#r", "slotprobe").unwrap().value,
            expected
        );
    }

    #[test]
    #[ignore = "needs prepared resources and isolated homes/cwd"]
    fn loaded_snapshot_changes_only_on_existing_reload_with_pack_precedence() {
        let (mut game, file) = scene("slot-source-snapshot-world");
        let before = game.block_entity_nbt([0, 100, 0]).unwrap();
        let clock = game.world.game_time;
        count(&mut game, 3); // Higher-priority pack overrides the same id.
        std::fs::write(&file, r#"{"type":"minecraft:limit_slots","limit":2,"slot_source":{"type":"minecraft:slot_range","slots":"container.*"}}"#).unwrap();
        count(&mut game, 3); // Disk edits do not change the loaded snapshot.
        game.reload_datapacks(true);
        count(&mut game, 2);
        std::fs::write(file, "{").unwrap();
        game.reload_datapacks(true);
        count(&mut game, 1); // Existing policy: invalid higher JSON is skipped.
        assert!(!game.run_command("execute if slots block 0 100 0 first_three"));
        assert_eq!(game.block_entity_nbt([0, 100, 0]).as_ref(), Some(&before));
        assert_eq!(game.world.game_time, clock);
    }

    #[test]
    #[ignore = "needs prepared resources and isolated homes/cwd"]
    fn admitted_item_arguments_still_fail_without_writing_or_player_fallback() {
        let (mut game, _) = scene("slot-source-unsupported-world");
        let before = game.block_entity_nbt([0, 100, 0]).unwrap();
        let clock = game.world.game_time;
        count(&mut game, 3);
        let player_was_present = game.player.is_some();
        println!("unsupported registered item routes: actual Player present={player_was_present}");
        let mut commands = vec![
            "item modify block 0 100 0 parity:first_three {type:\"minecraft:set_count\",count:1}"
                .to_owned(),
        ];
        for mode in ["replace", "fill", "override"] {
            commands.push(format!(
                "item {mode} block 0 100 0 parity:first_three with diamond"
            ));
            commands.push(format!(
                "item {mode} block 0 100 0 container.0 from block 0 100 0 parity:first_three"
            ));
        }
        for command in commands {
            let dispatcher = horizon_client::command::vanilla::game_dispatcher_with_features(
                &game.level.enabled_features,
            );
            assert!(
                dispatcher
                    .parse(&command, &game.ui.registries)
                    .executable()
                    .is_ok(),
                "registered argument must be admitted: {command}"
            );
            assert!(
                !game.run_command(&command),
                "unsupported registered write must fail: {command}"
            );
            assert_eq!(
                game.block_entity_nbt([0, 100, 0]).as_ref(),
                Some(&before),
                "{command}"
            );
            assert_eq!(game.player.is_some(), player_was_present);
            assert_eq!(game.world.game_time, clock);
        }
    }
}
