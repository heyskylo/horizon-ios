//! Registry ids an older server numbers differently: the tables `tools/generate_registry_id_adapter.py`
//! writes, and the per-thread context the item and particle readers consult while a packet from such a
//! server is decoded (an id there decides how the bytes after it read, so it must be renumbered as it is
//! read, not afterwards).
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// How one registry's ids differ between an older version and the pinned one.
#[derive(Clone, Copy, Debug)]
pub enum Remap {
    /// Numbered alike.
    Same,
    /// Runs `(from, to, length)`: ids `from..from + length` map to `to..to + length`. `up` goes from the older
    /// version to the pinned one, `down` back. An id in no run has no entry on the other side.
    Runs {
        up: &'static [(u16, u16, u16)],
        down: &'static [(u16, u16, u16)],
    },
}

fn lookup(runs: &[(u16, u16, u16)], id: i32) -> Option<i32> {
    let id = u16::try_from(id).ok()?;
    let i = runs
        .partition_point(|&(from, _, _)| from <= id)
        .checked_sub(1)?;
    let (from, to, length) = runs[i];
    (id - from < length).then(|| i32::from(to + (id - from)))
}

impl Remap {
    /// The pinned id of the older version's `id`; `None` when the pinned version has no such entry.
    pub fn up(self, id: i32) -> Option<i32> {
        match self {
            Self::Same => Some(id),
            Self::Runs { up, .. } => lookup(up, id),
        }
    }
    /// The older version's id of the pinned `id`; `None` when the older version has no such entry.
    pub fn down(self, id: i32) -> Option<i32> {
        match self {
            Self::Same => Some(id),
            Self::Runs { down, .. } => lookup(down, id),
        }
    }
}

/// Block states and every registry the protocol sends by id, for one older version.
#[derive(Clone, Copy, Debug)]
pub struct Ids {
    /// How the older version lays out what the shared readers decode.
    pub quirks: Quirks,
    /// How many block states the older version and the pinned one have.
    pub states: (u32, u32),
    pub block_state: Remap,
    pub item: Remap,
    pub entity_type: Remap,
    pub particle_type: Remap,
    pub sound_event: Remap,
    pub data_component_type: Remap,
    pub command_argument_type: Remap,
    pub custom_stat: Remap,
    pub map_decoration_type: Remap,
    pub block: Remap,
    pub block_entity_type: Remap,
    pub menu: Remap,
    pub mob_effect: Remap,
    pub attribute: Remap,
    pub stat_type: Remap,
    pub fluid: Remap,
    pub game_event: Remap,
    pub recipe_serializer: Remap,
    pub slot_display: Remap,
    pub recipe_display: Remap,
    pub recipe_book_category: Remap,
    pub villager_type: Remap,
    pub villager_profession: Remap,
    pub position_source_type: Remap,
    pub consume_effect_type: Remap,
    pub enchantment_effect_component_type: Remap,
    /// EntityDataSerializers' registration order (not a registry).
    pub entity_data_serializer: Remap,
}

/// Layout differences inside structures the pinned readers decode while ids are renumbered up.
#[derive(Clone, Copy, Debug, Default)]
pub struct Quirks {
    /// `SlotDisplay` type 6 holds a bare tag id (TagSlotDisplay) instead of a HolderSet (26.2).
    pub tag_slot_display_is_id: bool,
    /// Before 1.21.9, player shoulder parrots were compound tags (serializer 16), not optional variants.
    pub shoulder_parrots_as_nbt: bool,
    /// Before 1.21.6, attribute modifiers had no display and equippables no shearing fields.
    pub components_before_1_21_6: bool,
    /// The 1.21.4 component patch has per-component tooltip flags and older nested codecs.
    pub components_before_1_21_5: bool,
    /// These releases use count-first ItemStack.STREAM_CODEC where the pinned version uses templates.
    pub count_first_templates: bool,
    /// Optional name/UUID and properties, without the pinned profile tag or skin patch.
    pub optional_profile_fields: bool,
    /// Before 1.21.4, custom model data was one VarInt instead of four lists.
    pub integer_custom_model_data: bool,
    /// Before 1.21.4, trail options left the lifetime to the client particle provider.
    pub trail_without_duration: bool,
    /// Before 1.21.2, food combined consumption effects and several packet/component layouts differed.
    pub components_before_1_21_2: bool,
    /// Before 1.21, food had no remainder and attribute modifiers used UUIDs and names.
    pub components_before_1_21: bool,
    /// 1.21 through 1.21.4 encoded wolf variants as registry-or-inline holders.
    pub wolf_variant_holder: bool,
    /// Before 1.20.5 an item stack carried a single legacy NBT tag instead of a component patch.
    pub legacy_item_nbt: bool,
    /// Saved-item data version at the legacy wire boundary (zero when components are native).
    pub legacy_item_data_version: i32,
    /// Before 1.20.3, component fields inside entity data use UTF JSON rather than NBT.
    pub components_as_json: bool,
    /// Before 1.20.3, vibration position sources start with a resource identifier.
    pub vibration_source_identifier: bool,
    /// Through 1.21.8 these particle types carried no options at all.
    pub simple_spell_particles: bool,
}
impl Quirks {
    pub const NONE: Self = Self {
        tag_slot_display_is_id: false,
        shoulder_parrots_as_nbt: false,
        components_before_1_21_6: false,
        components_before_1_21_5: false,
        count_first_templates: false,
        optional_profile_fields: false,
        integer_custom_model_data: false,
        trail_without_duration: false,
        components_before_1_21_2: false,
        components_before_1_21: false,
        wolf_variant_holder: false,
        legacy_item_nbt: false,
        legacy_item_data_version: 0,
        components_as_json: false,
        vibration_source_identifier: false,
        simple_spell_particles: false,
    };
}

/// The older version's quirks while its packets are decoded on this thread; none otherwise.
pub fn quirks() -> Quirks {
    match ACTIVE.get() {
        Some((ids, Way::Up)) => ids.quirks,
        _ => Quirks::NONE,
    }
}

impl Ids {
    /// Bits per entry of a chunk section's global block palette: the older version's, then the pinned one's
    /// (`Mth.ceillog2` of the state count).
    pub fn state_bits(&self) -> (u8, u8) {
        let bits = |n: u32| (u32::BITS - n.saturating_sub(1).leading_zeros()) as u8;
        (bits(self.states.0), bits(self.states.1))
    }
}

/// Which way ids are being renumbered on this thread.
#[derive(Clone, Copy, Debug)]
pub enum Way {
    /// Decoding what an older server sent: its ids become the pinned version's.
    Up,
    /// Encoding for an older server: the pinned ids become its own.
    Down,
}

thread_local! {
    static ACTIVE: Cell<Option<(&'static Ids, Way)>> = const { Cell::new(None) };
    static TRIM_PATTERNS: RefCell<HashMap<i32,i32>> = RefCell::new(HashMap::new());
    static LEGACY_REGISTRIES: RefCell<Option<Arc<LegacyRegistries>>> = const { RefCell::new(None) };
    static LEGACY_ITEMS: RefCell<Option<Arc<Mutex<crate::item::v1_20_3::Cache>>>> = const { RefCell::new(None) };
}

pub(crate) fn legacy_items(cache: Arc<Mutex<crate::item::v1_20_3::Cache>>) -> LegacyItemGuard {
    LegacyItemGuard(LEGACY_ITEMS.replace(Some(cache)))
}
pub(crate) fn legacy_item_cache() -> Option<Arc<Mutex<crate::item::v1_20_3::Cache>>> {
    LEGACY_ITEMS.with_borrow(Clone::clone)
}
pub(crate) struct LegacyItemGuard(Option<Arc<Mutex<crate::item::v1_20_3::Cache>>>);
impl Drop for LegacyItemGuard {
    fn drop(&mut self) {
        LEGACY_ITEMS.replace(self.0.take());
    }
}

/// Name lookups for legacy NBT. Fixed registries use pinned ids; synchronized registries keep the
/// connection's ids. Reverse names retain old spellings, with current names accepted as aliases.
pub(crate) type LegacyRegistries = HashMap<String, crate::registry::Ids>;

pub(crate) fn legacy_registries(registries: Arc<LegacyRegistries>) -> LegacyRegistryGuard {
    LegacyRegistryGuard(LEGACY_REGISTRIES.replace(Some(registries)))
}

pub(crate) struct LegacyRegistryGuard(Option<Arc<LegacyRegistries>>);
impl Drop for LegacyRegistryGuard {
    fn drop(&mut self) {
        LEGACY_REGISTRIES.replace(self.0.take());
    }
}

pub(crate) fn legacy_registry_id(registry: &str, name: &str) -> Option<i32> {
    LEGACY_REGISTRIES.with_borrow(|registries| registries.as_ref()?.get(registry)?.id(name))
}

pub(crate) fn legacy_registry_name(registry: &str, id: i32) -> Option<String> {
    LEGACY_REGISTRIES.with_borrow(|registries| {
        registries
            .as_ref()?
            .get(registry)?
            .name(id)
            .map(str::to_owned)
    })
}

/// Connection-local trim ids while decoding the old smithing preview's template displays.
pub(crate) fn trim_patterns(patterns: HashMap<i32, i32>) -> TrimGuard {
    TrimGuard(TRIM_PATTERNS.replace(patterns))
}
pub(crate) struct TrimGuard(HashMap<i32, i32>);
impl Drop for TrimGuard {
    fn drop(&mut self) {
        TRIM_PATTERNS.replace(std::mem::take(&mut self.0));
    }
}
pub(crate) fn trim_pattern(item: i32) -> Option<i32> {
    TRIM_PATTERNS.with_borrow(|patterns| patterns.get(&item).copied())
}

/// Renumber ids `way` on this thread until the guard drops. Readers and writers that pick a layout by an id
/// (item stacks, data components, particles) consult it through [`item`], [`component`] and [`particle`].
pub fn renumbering(ids: &'static Ids, way: Way) -> Guard {
    Guard(ACTIVE.replace(Some((ids, way))))
}
/// Decode a stored pinned value before encoding its nested values for an older server.
pub(crate) fn unrenumbered() -> Guard {
    Guard(ACTIVE.replace(None))
}

/// Restores the previous context when dropped.
pub struct Guard(Option<(&'static Ids, Way)>);
impl Drop for Guard {
    fn drop(&mut self) {
        ACTIVE.set(self.0);
    }
}

fn apply(id: i32, pick: impl Fn(&Ids) -> Remap) -> i32 {
    match ACTIVE.get() {
        None => id,
        // An id the other side lacks becomes -1, which every reader turns away or reads as nothing.
        Some((ids, Way::Up)) => pick(ids).up(id).unwrap_or(-1),
        Some((ids, Way::Down)) => pick(ids).down(id).unwrap_or(-1),
    }
}

/// An item id as read or about to be written on this thread.
pub fn item(id: i32) -> i32 {
    apply(id, |ids| ids.item)
}
/// A data component type id as read or about to be written on this thread.
pub fn component(id: i32) -> i32 {
    apply(id, |ids| ids.data_component_type)
}
/// An argument parser id; its properties must be read using the pinned parser's layout.
pub fn command_argument_type(id: i32) -> i32 {
    apply(id, |ids| ids.command_argument_type)
}
/// A particle type id as read or about to be written on this thread.
pub fn particle(id: i32) -> i32 {
    apply(id, |ids| ids.particle_type)
}
/// A block state id as read or about to be written on this thread.
pub fn block_state(id: i32) -> i32 {
    apply(id, |ids| ids.block_state)
}
/// An entity type id as read or about to be written on this thread.
pub fn entity_type(id: i32) -> i32 {
    apply(id, |ids| ids.entity_type)
}
/// A sound event id as read or about to be written on this thread.
pub fn sound_event(id: i32) -> i32 {
    apply(id, |ids| ids.sound_event)
}
/// An entity data serializer id as read on this thread.
pub fn entity_data_serializer(id: i32) -> i32 {
    apply(id, |ids| ids.entity_data_serializer)
}
/// A slot display type id as read on this thread.
pub fn slot_display(id: i32) -> i32 {
    apply(id, |ids| ids.slot_display)
}
/// The ids on this thread now, if any.
pub fn active() -> Option<(&'static Ids, Way)> {
    ACTIVE.get()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_map_inside_and_nothing_outside() {
        let runs = Remap::Runs {
            up: &[(0, 0, 3), (3, 5, 2)],
            down: &[(0, 0, 3), (5, 3, 2)],
        };
        assert_eq!(runs.up(2), Some(2));
        assert_eq!(runs.up(3), Some(5));
        assert_eq!(runs.up(4), Some(6));
        assert_eq!(runs.up(5), None);
        assert_eq!(runs.up(-1), None);
        assert_eq!(runs.down(3), None);
        assert_eq!(runs.down(6), Some(4));
    }

    #[test]
    fn the_context_lasts_while_its_guard_does() {
        static IDS: Ids = super::super::v26_2_ids::IDS;
        assert_eq!(item(926), 926);
        {
            // diamond: 926 in 26.2, 1012 in 26.3.
            let _up = renumbering(&IDS, Way::Up);
            assert_eq!(item(926), 1012);
            {
                let _down = renumbering(&IDS, Way::Down);
                assert_eq!(item(1012), 926);
            }
            assert_eq!(item(926), 1012);
        }
        assert_eq!(item(926), 926);
    }

    #[test]
    fn the_26_2_tables_match_the_reports() {
        // Read off both versions' registries and blocks reports by hand.
        let ids = crate::adapter::ServerVersion::V26_2.ids().unwrap();
        assert_eq!(ids.item.up(926), Some(1012)); // diamond
        assert_eq!(ids.item.up(1), Some(1)); // stone
        assert_eq!(ids.block_state.up(13330), Some(15177)); // oak_slab type=top waterlogged=true
        assert_eq!(ids.block_state.up(3987), Some(5543)); // chest, first state
        assert_eq!(ids.block_state.up(8), Some(8)); // grass_block
        assert_eq!(ids.state_bits(), (15, 16));
    }

    #[test]
    fn an_item_stack_is_read_and_written_in_the_servers_numbering() {
        use crate::buf::{Codec, Reader, Writer};
        use crate::item::{Slot, Stack};
        let ids = crate::adapter::ServerVersion::V26_2.ids().unwrap();
        // Three diamonds as a 26.2 server sends them: item 926, no components.
        let sent = [3, 0x9e, 0x07, 0, 0];
        let stack = {
            let _up = renumbering(ids, Way::Up);
            Slot::read(&mut Reader::new(&sent)).unwrap().unwrap()
        };
        assert_eq!((stack.item, stack.count), (1012, 3));
        let mut w = Writer::new();
        {
            let _down = renumbering(ids, Way::Down);
            Slot::write(&Some(Stack::new(1012, 3)), &mut w);
        }
        assert_eq!(w.data, sent);
    }
}
