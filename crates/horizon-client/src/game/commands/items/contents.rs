//! ContentsSlotSource's live component collections; mutable until written back.
use super::{Game, ItemStack, Slot, SlotCandidate};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::game) enum Component {
    Container,
    Bundle,
    Charged,
}
pub(super) struct Provider<'a> {
    items: &'a [Option<ItemStack>],
    position: &'a mut usize,
    mode: &'a str,
}
impl<'a> Provider<'a> {
    pub(super) fn new(
        items: &'a [Option<ItemStack>],
        position: &'a mut usize,
        mode: &'a str,
    ) -> Self {
        Self {
            items,
            position,
            mode,
        }
    }
}
impl Component {
    pub(super) fn named(name: &str) -> Option<Self> {
        match name.strip_prefix("minecraft:").unwrap_or(name) {
            "container" => Some(Self::Container),
            "bundle_contents" => Some(Self::Bundle),
            "charged_projectiles" => Some(Self::Charged),
            _ => None,
        }
    }
    pub(super) fn items(self, stack: &ItemStack) -> Vec<Option<ItemStack>> {
        let Some(data) = stack.data() else {
            return Vec::new();
        };
        match self {
            Self::Container => {
                let size = data
                    .container
                    .iter()
                    .map(|(i, _)| usize::from(*i) + 1)
                    .max()
                    .unwrap_or(0);
                let mut out = vec![None; size];
                for (i, item) in &data.container {
                    out[usize::from(*i)] = Some(item.clone());
                }
                out
            }
            Self::Bundle => data.bundle_contents.iter().cloned().map(Some).collect(),
            Self::Charged => data.charged.iter().cloned().map(Some).collect(),
        }
    }
    /// ComponentSlotCollection writes a single normalized component back to
    /// its live parent; all unrelated parent components remain modeled.
    pub(super) fn store(self, stack: &mut ItemStack, mut slots: Vec<Option<ItemStack>>) {
        for slot in &mut slots {
            if slot
                .as_ref()
                .is_some_and(|s| s.count == 0 || &*s.item == "minecraft:air")
            {
                *slot = None;
            }
        }
        if self == Self::Bundle {
            // BundleContents.Mutable.mergeIdenticalStacks: merge into a later
            // matching entry, preserving the order of the surviving entries.
            let mut i = 0;
            while i < slots.len() {
                let Some(item) = slots[i].as_ref() else {
                    slots.remove(i);
                    continue;
                };
                if let Some(j) = (i + 1..slots.len()).find(|&j| {
                    slots[j]
                        .as_ref()
                        .is_some_and(|s| s.same_item_same_components(item))
                }) {
                    let count = item.count;
                    slots[j].as_mut().unwrap().count += count;
                    slots.remove(i);
                } else {
                    i += 1;
                }
            }
        }
        let data = stack.data_mut();
        match self {
            Self::Container => {
                data.container = slots
                    .into_iter()
                    .enumerate()
                    .filter_map(|(i, s)| s.map(|s| (i as u8, s)))
                    .collect()
            }
            Self::Bundle => data.bundle_contents = slots.into_iter().flatten().collect(),
            Self::Charged => data.charged = slots.into_iter().flatten().collect(),
        }
        stack.tidy();
    }
}

impl Game<'_> {
    pub(super) fn contents_slots(&mut self, parent: Slot, component: Component) -> Vec<Slot> {
        let size = self
            .slot_get(&parent)
            .map_or(0, |s| component.items(&s).len());
        (0..size)
            .map(|i| Slot::ContentItem(Box::new(parent.clone()), component, i))
            .collect()
    }
    fn select_contents_item(
        &mut self,
        candidate: &SlotCandidate,
        selected: &mut usize,
        stack: Option<&ItemStack>,
        nonempty: bool,
    ) -> bool {
        if *selected >= candidate.limit || (nonempty && stack.is_none()) {
            return false;
        }
        let air = ItemStack::new("minecraft:air", 0);
        if !candidate
            .filters
            .iter()
            .all(|f| self.loot_item_filter(f, stack.unwrap_or(&air)))
        {
            return false;
        }
        *selected += 1;
        true
    }
    fn contents_can_grow(&self, component: Component, slots: &[Option<ItemStack>]) -> bool {
        match component {
            Component::Container => slots.len() < 256,
            Component::Charged => slots.len() < 1024,
            Component::Bundle => !horizon_client::bundle::Contents::from_items(
                self.res.items(),
                slots.iter().flatten().cloned().collect(),
            )
            .is_full(),
        }
    }
    fn contents_accepts(
        &self,
        component: Component,
        slots: &[Option<ItemStack>],
        index: usize,
        item: Option<&ItemStack>,
    ) -> bool {
        if component != Component::Bundle {
            return true;
        }
        let Some(item) = item else { return true };
        if !horizon_client::bundle::can_be_in_bundle(item) {
            return false;
        }
        let rest = slots
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != index)
            .filter_map(|(_, s)| s.clone())
            .collect();
        horizon_client::bundle::Contents::from_items(self.res.items(), rest)
            .room_for(self.res.items(), item)
            >= item.count
    }
    /// ComponentSlotCollection.modifySlots: visit occupied slots only, then
    /// commit the component through the same live parent slot.
    pub(super) fn modify_contents(
        &mut self,
        parent: &Slot,
        component: Component,
        candidate: &SlotCandidate,
        selected: &mut usize,
        mut modifier: impl FnMut(&mut Self, ItemStack) -> Option<ItemStack>,
    ) -> i32 {
        let Some(mut stack) = self.slot_get(parent) else {
            return 0;
        };
        let mut slots = component.items(&stack);
        let mut changed = 0;
        for i in 0..slots.len() {
            if !self.select_contents_item(candidate, selected, slots[i].as_ref(), true) {
                continue;
            }
            let Some(current) = slots[i].clone() else {
                continue;
            };
            let next =
                modifier(self, current).filter(|s| s.count > 0 && &*s.item != "minecraft:air");
            if self.contents_accepts(component, &slots, i, next.as_ref()) {
                slots[i] = next;
                changed += 1;
            }
        }
        component.store(&mut stack, slots);
        if self.slot_set(parent, Some(stack)) {
            changed
        } else {
            0
        }
    }
    /// GrowableMutableContainer.replaceSlotItems: consume one provider shared
    /// by the owner's source terms, including new component slots.
    pub(super) fn replace_contents(
        &mut self,
        parent: &Slot,
        component: Component,
        candidate: &SlotCandidate,
        selected: &mut usize,
        provider: &mut Provider<'_>,
    ) -> i32 {
        let items = provider.items;
        let position = &mut *provider.position;
        let mode = provider.mode;
        let original = self.slot_get(parent);
        let mut stack = original
            .clone()
            .unwrap_or_else(|| ItemStack::new("minecraft:air", 0));
        let mut slots = component.items(&stack);
        let mut changed = 0;
        for i in 0..slots.len() {
            if !has_next(items, *position, mode) {
                break;
            }
            if !self.select_contents_item(candidate, selected, slots[i].as_ref(), false) {
                continue;
            }
            let next = next_item(items, position, mode);
            if self.contents_accepts(component, &slots, i, next.as_ref()) {
                slots[i] = next;
                changed += 1;
            }
        }
        while has_next(items, *position, mode) && self.contents_can_grow(component, &slots) {
            if !self.select_contents_item(candidate, selected, None, false) {
                break;
            }
            if component != Component::Container && !find_nonempty(items, position, mode) {
                break;
            }
            let next = peek_item(items, *position, mode);
            if !self.contents_accepts(component, &slots, slots.len(), next.as_ref()) {
                break;
            }
            slots.push(next_item(items, position, mode));
            changed += 1;
        }
        // ItemStack.EMPTY cannot gain components, though its virtual mutable
        // collection can still report replacements.
        if original.is_none() {
            return changed;
        }
        component.store(&mut stack, slots);
        if self.slot_set(parent, Some(stack)) {
            changed
        } else {
            0
        }
    }
}
fn has_next(items: &[Option<ItemStack>], position: usize, mode: &str) -> bool {
    !items.is_empty() && (mode != "replace" || position < items.len())
}
fn peek_item(items: &[Option<ItemStack>], position: usize, mode: &str) -> Option<ItemStack> {
    let index = if mode == "fill" {
        position % items.len()
    } else {
        position
    };
    items
        .get(index)
        .cloned()
        .flatten()
        .filter(|s| s.count > 0 && &*s.item != "minecraft:air")
}
fn next_item(items: &[Option<ItemStack>], position: &mut usize, mode: &str) -> Option<ItemStack> {
    let next = peek_item(items, *position, mode);
    *position += 1;
    next
}
fn find_nonempty(items: &[Option<ItemStack>], position: &mut usize, mode: &str) -> bool {
    // ArrayItemProvider.findNext changes position only when it finds a match.
    // A failed component append must leave air available to later source terms.
    for offset in 0..items.len() {
        let next = *position + offset;
        if mode != "fill" && next >= items.len() {
            return false;
        }
        if peek_item(items, next, mode).is_some() {
            *position = next;
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_world::nbt::Tag;

    #[test]
    fn failed_component_growth_keeps_air_for_the_next_source_term() {
        let items = [None];
        let mut position = 0;
        assert!(!find_nonempty(&items, &mut position, "replace"));
        assert_eq!(position, 0);
        assert!(has_next(&items, position, "replace"));
        assert!(next_item(&items, &mut position, "replace").is_none());
        assert_eq!(position, 1);
    }

    #[test]
    fn component_storage_keeps_sparse_positions_and_the_live_parent() {
        let mut parent = ItemStack::new("minecraft:stone", 1);
        parent.data_mut().other.push((
            "minecraft:custom_data".into(),
            Tag::compound([("audit", Tag::Int(7))]),
        ));
        let mut slots = vec![None; 6];
        slots[0] = Some(ItemStack::new("minecraft:diamond", 1));
        slots[5] = Some(ItemStack::new("minecraft:dirt", 7));
        Component::Container.store(&mut parent, slots.clone());
        assert_eq!((&*parent.item, parent.count), ("minecraft:stone", 1));
        assert_eq!(
            parent.data().unwrap().other("minecraft:custom_data"),
            Some(&Tag::compound([("audit", Tag::Int(7))]))
        );
        assert_eq!(Component::Container.items(&parent), slots);
    }

    #[test]
    fn bundle_writeback_merges_identical_entries_but_keeps_distinct_components() {
        let mut parent = ItemStack::new("minecraft:bundle", 1);
        Component::Bundle.store(
            &mut parent,
            vec![Some(ItemStack::new("minecraft:gold_ingot", 1)); 64],
        );
        assert_eq!(
            parent.data().unwrap().bundle_contents,
            vec![ItemStack::new("minecraft:gold_ingot", 64)]
        );
        let plain = ItemStack::new("minecraft:gold_ingot", 1);
        let mut marked = plain.clone();
        marked.data_mut().other.push((
            "minecraft:custom_data".into(),
            Tag::compound([("audit", Tag::Int(7))]),
        ));
        Component::Bundle.store(&mut parent, vec![Some(plain.clone()), Some(marked.clone())]);
        assert_eq!(parent.data().unwrap().bundle_contents, vec![plain, marked]);
    }
}
