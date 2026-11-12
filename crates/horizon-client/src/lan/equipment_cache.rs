//! The converted equipment of one guest. A slot keeps the stack it was converted from; the
//! stack is converted again only when the slot holds a different one, compared in full.

use crate::items::ItemStack;
use horizon_protocol::item::Stack;

/// Equipment slots: held, offhand, then the armor from head to feet.
pub const SLOTS: usize = 6;

/// The wire slot of each equipment slot, in the order the equipment is listed. Armor is stored
/// head..feet; the wire slots are feet(2) legs(3) chest(4) head(5).
const WIRE_SLOTS: [u8; SLOTS] = [0, 1, 5, 4, 3, 2];

#[derive(Default)]
struct Slot {
    source: Option<ItemStack>,
    converted: Option<Stack>,
}

#[derive(Default)]
pub struct EquipmentCache {
    slots: [Slot; SLOTS],
}

impl EquipmentCache {
    /// Whether `source` differs from the stack slot `index` was last converted from.
    pub fn is_stale(&self, index: usize, source: Option<&ItemStack>) -> bool {
        self.slots[index].source.as_ref() != source
    }

    /// Records the stack slot `index` was converted from, and the conversion.
    pub fn store(&mut self, index: usize, source: Option<ItemStack>, converted: Option<Stack>) {
        self.slots[index] = Slot { source, converted };
    }

    /// `(wire slot, stack)` for each equipment slot that holds an item.
    pub fn equipment(&self) -> Vec<(u8, Option<Stack>)> {
        WIRE_SLOTS
            .iter()
            .zip(&self.slots)
            .filter(|(_, slot)| slot.source.is_some())
            .map(|(&wire, slot)| (wire, slot.converted.clone()))
            .collect()
    }
}
