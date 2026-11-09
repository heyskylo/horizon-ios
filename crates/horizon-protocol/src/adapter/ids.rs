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
    /// The data components the older version has and the pinned one lacks: their older ids and names.
    pub dropped_components: &'static [(u16, &'static str)],
    /// EntityDataSerializers' registration order (not a registry).
    pub entity_data_serializer: Remap,
    /// What a client of the older version is shown in place of pinned entries it lacks.
    pub substitutes: Substitutes,
}

/// For a server sending to an older client: pinned entries the older version lacks, each with the pinned
/// entry that stands in for it there, as runs `(from, to, length)` in the pinned numbering (a stand-in is
/// one the older version has). An entry in no run has none: what carries it is dropped or emptied.
#[derive(Clone, Copy, Debug)]
pub struct Substitutes {
    pub block_state: &'static [(u16, u16, u16)],
    pub block: &'static [(u16, u16, u16)],
    pub item: &'static [(u16, u16, u16)],
    pub entity_type: &'static [(u16, u16, u16)],
    pub particle_type: &'static [(u16, u16, u16)],
    pub sound_event: &'static [(u16, u16, u16)],
}
impl Substitutes {
    pub const NONE: Self = Self {
        block_state: &[],
        block: &[],
        item: &[],
        entity_type: &[],
        particle_type: &[],
        sound_event: &[],
    };
}

/// The older version's id of the pinned `id`, or of the entry standing in for it there (`substitutes`).
pub fn down_or_substitute(remap: Remap, substitutes: &[(u16, u16, u16)], id: i32) -> Option<i32> {
    remap
        .down(id)
        .or_else(|| remap.down(lookup(substitutes, id)?))
}

/// Layout differences inside structures the pinned readers decode while ids are renumbered up.
#[derive(Clone, Copy, Debug, Default)]
pub struct Quirks {
    /// `SlotDisplay` type 6 holds a bare tag id (TagSlotDisplay) instead of a HolderSet (26.2).
    pub tag_slot_display_is_id: bool,
    /// `pot_decorations` is a list of up to four item ids, a brick for an undecorated side, instead of four
    /// optional item stack templates (26.2).
    pub pot_decorations_are_items: bool,
    /// `damage_type`, `instrument`, `provides_trim_material`, `jukebox_playable`, `chicken/variant` and
    /// `zombie_nautilus/variant` are EitherHolders (a flag, then an entry's holder or key) instead of
    /// holders, and `damage_resistant` a bare damage type tag instead of a HolderSet (1.21.11).
    pub either_holders_and_tag_resistance: bool,
    /// `use_remainder` holds an ItemStack and `container` a list of optional ones (a count first, 0 for
    /// none) instead of item stack templates (1.21.11).
    pub stacks_in_components: bool,
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
        pot_decorations_are_items: false,
        either_holders_and_tag_resistance: false,
        stacks_in_components: false,
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

/// Whether an older server's packet is being read on this thread.
pub fn reading_older() -> bool {
    matches!(ACTIVE.get(), Some((_, Way::Up)))
}

/// Reads and writes the pinned numbering on this thread until the guard drops: for values already
/// renumbered while an older server's packet is read.
pub fn pinned() -> Guard {
    Guard(ACTIVE.replace(None))
}

/// The older version's quirks while its packets are decoded on this thread; none otherwise.
pub fn quirks() -> Quirks {
    match ACTIVE.get() {
        Some((ids, Way::Up)) => ids.quirks,
        _ => Quirks::NONE,
    }
}

/// The older version's quirks while packets are encoded for it on this thread (an older client, or what is
/// sent to an older server); none otherwise. The writers then lay those structures out as it reads them.
pub fn quirks_written() -> Quirks {
    match ACTIVE.get() {
        Some((ids, Way::Down)) => ids.quirks,
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

/// An item id as read or about to be written on this thread. Written for an older client, an item it
/// lacks becomes its stand-in.
pub fn item(id: i32) -> i32 {
    match ACTIVE.get() {
        Some((ids, Way::Down)) => {
            down_or_substitute(ids.item, ids.substitutes.item, id).unwrap_or(-1)
        }
        _ => apply(id, |ids| ids.item),
    }
}
/// A data component type id as read or about to be written on this thread.
pub fn component(id: i32) -> i32 {
    apply(id, |ids| ids.data_component_type)
}
/// An argument parser id; its properties must be read using the pinned parser's layout.
pub fn command_argument_type(id: i32) -> i32 {
    apply(id, |ids| ids.command_argument_type)
}
/// The name of the data component an older server sent as `id` (its own number) that the pinned version
/// no longer has, while such a server's packet is read on this thread. The readers read its value past and
/// drop it.
pub fn dropped_component(id: i32) -> Option<&'static str> {
    match ACTIVE.get() {
        Some((ids, Way::Up)) => ids
            .dropped_components
            .iter()
            .find(|(old, _)| i32::from(*old) == id)
            .map(|(_, name)| *name),
        _ => None,
    }
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
/// An attribute id (the registry id, not the holder's id plus one) as read or about to be written on this
/// thread.
pub fn attribute(id: i32) -> i32 {
    apply(id, |ids| ids.attribute)
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
    fn every_attribute_of_1_21_1_and_1_20_6_has_its_pinned_counterpart() {
        // 1.21.2 dropped the generic./player./zombie. prefixes; matched by name alone, none mapped and every
        // update_attributes value was left out for these clients. 31 attributes in 1.21.1's report, 22 in 1.20.6's.
        for (ids, count) in [
            (&super::super::v1_21_1_ids::IDS, 31),
            (&super::super::v1_20_6_ids::IDS, 22),
        ] {
            for old in 0..count {
                let pinned = ids.attribute.up(old).unwrap();
                assert_eq!(ids.attribute.down(pinned), Some(old));
            }
        }
    }

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
    fn components_the_pinned_version_dropped_are_read_past() {
        use crate::buf::{Codec, Reader, Writer};
        use crate::item::{Component, Slot};
        let ids = crate::adapter::ServerVersion::V1_21_11.ids().unwrap();
        // A filled map (1104 in 1.21.11, 1238 in 26.3) as a 1.21.11 server sends it. Its patch adds
        // map_color (43, gone in 26.3), swing_animation (40, gone too), damage 5 (3 in both) and a
        // tooltip_display (18 in both) hiding dyed_color (42, 47 in 26.3) and swing_animation; it removes
        // map_color and potion_contents (49, 53 in 26.3). Ids read off both versions' registries reports.
        let sent = [
            1, 0xd0, 0x08, // count, item
            4, 2, // added, removed
            43, 0x00, 0xff, 0x00, 0x00, // map_color: a big-endian int
            40, 1, 6, // swing_animation: its type and duration
            3, 5, // damage
            18, 0, 2, 42, 40, // tooltip_display: shown, two hidden components
            43, 49, // removed
        ];
        let stack = {
            let _up = renumbering(ids, Way::Up);
            Slot::read(&mut Reader::new(&sent)).unwrap().unwrap()
        };
        assert_eq!((stack.item, stack.count), (1238, 1));
        assert_eq!(
            stack.added,
            [
                (3, Component::Int(5)),
                (
                    18,
                    Component::TooltipDisplay {
                        hide: false,
                        hidden: vec![47],
                    },
                ),
            ]
        );
        assert_eq!(stack.removed, [53]);
        // Written back to that server, the kept components take its numbers again.
        let mut w = Writer::new();
        {
            let _down = renumbering(ids, Way::Down);
            Slot::write(&Some(stack), &mut w);
        }
        assert_eq!(w.data, [1, 0xd0, 0x08, 2, 1, 3, 5, 18, 0, 1, 42, 49]);
    }

    #[test]
    fn components_whose_layout_changed_are_read_in_the_servers() {
        use crate::buf::{Codec, Reader};
        use crate::item::{Component, Slot};
        let ids = crate::adapter::ServerVersion::V1_21_11.ids().unwrap();
        let read = |sent: &[u8]| {
            let _up = renumbering(ids, Way::Up);
            Slot::read(&mut Reader::new(sent)).unwrap().unwrap()
        };
        // A decorated pot (319 in 1.21.11, 392 in 26.3) as a 1.21.11 server sends it: pot_decorations (72)
        // lists a brick (1025) and an angler sherd (1445, 1598 in 26.3); damage_resistant (27) a tag id;
        // jukebox_playable (62) a song by holder: the flag set, then the song's id plus one.
        let mut sent = vec![1, 0xbf, 0x02, 3, 0];
        sent.extend([72, 2, 0x81, 0x08, 0xa5, 0x0b]);
        sent.extend([27, 17]);
        sent.extend(b"minecraft:is_fire");
        sent.extend([62, 1, 3]);
        let stack = read(&sent);
        assert_eq!(stack.item, 392);
        let mut resistant = vec![0, 17];
        resistant.extend(b"minecraft:is_fire");
        assert_eq!(
            stack.added,
            [
                // Only the left side holds an item: one sherd by its pinned id, no components.
                (76, Component::Raw(vec![0, 1, 0xbe, 0x0c, 1, 0, 0, 0, 0])),
                (27, Component::Raw(resistant)),
                (66, Component::Raw(vec![3])),
            ]
        );
        // A song named by its key has no pinned holder: the component goes.
        let mut sent = vec![1, 0xbf, 0x02, 1, 0, 62, 0, 13];
        sent.extend(b"minecraft:cat");
        assert_eq!(read(&sent).added, []);
    }

    #[test]
    fn registry_entries_in_1_21_11_components_are_either_holders() {
        use crate::buf::{Codec, Reader};
        use crate::item::{Component, Slot, component as k};
        let ids = crate::adapter::ServerVersion::V1_21_11.ids().unwrap();
        let older = |kind: i32| {
            let id = ids.data_component_type.down(kind).unwrap();
            assert!((0..128).contains(&id));
            id as u8
        };
        let read = |sent: &[u8]| {
            let _up = renumbering(ids, Way::Up);
            let mut r = Reader::new(sent);
            let stack = Slot::read(&mut r).unwrap().unwrap();
            assert_eq!(r.remaining(), 0);
            stack
        };
        // Stone (1 in both) as a 1.21.11 server sends a stack whose instrument, provides_trim_material,
        // damage_type, chicken/variant and zombie_nautilus/variant are held: the flag set, then the
        // holder as 26.3 reads it (an id plus one, or a registry id).
        let mut sent = vec![1, 1, 5, 0];
        sent.extend([older(k::INSTRUMENT), 1, 2]);
        sent.extend([older(k::PROVIDES_TRIM_MATERIAL), 1, 5]);
        sent.extend([older(k::DAMAGE_TYPE), 1, 3]);
        sent.extend([older(k::CHICKEN_VARIANT), 1, 2]);
        sent.extend([older(k::ZOMBIE_NAUTILUS_VARIANT), 1, 0]);
        let stack = read(&sent);
        let kinds: Vec<i32> = stack.added.iter().map(|(kind, _)| *kind).collect();
        assert_eq!(
            kinds,
            [
                k::INSTRUMENT,
                k::PROVIDES_TRIM_MATERIAL,
                k::DAMAGE_TYPE,
                k::CHICKEN_VARIANT,
                k::ZOMBIE_NAUTILUS_VARIANT,
            ]
        );
        assert_eq!(stack.added[0].1, Component::Instrument(1));
        // An instrument named by its key has no pinned holder: the component goes, the rest reads on.
        let mut sent = vec![1, 1, 2, 0, older(k::INSTRUMENT), 0, 26];
        sent.extend(b"minecraft:ponder_goat_horn");
        sent.extend([older(k::CHICKEN_VARIANT), 1, 2]);
        let kinds: Vec<i32> = read(&sent).added.iter().map(|(kind, _)| *kind).collect();
        assert_eq!(kinds, [k::CHICKEN_VARIANT]);
    }

    #[test]
    fn items_inside_components_take_the_pinned_ids_and_layout() {
        use crate::buf::{Codec, Reader};
        use crate::item::{Component, Slot};
        let ids = crate::adapter::ServerVersion::V1_21_11.ids().unwrap();
        // A shulker box (581 in 1.21.11, 656 in 26.3) as a 1.21.11 server sends it. Its container (73, 77
        // in 26.3) lists ItemStacks, a count first: two diamonds (898, 1012 in 26.3) with a map_color (43,
        // gone in 26.3), then an empty slot. Its use_remainder (25 in both) is a bowl (892, 1006).
        let sent = [
            1, 0xc5, 0x04, 2, 0, // the box, two added components
            73, 2, 2, 0x82, 0x07, 1, 0, 43, 0, 0, 0, 5, 0, // container
            25, 1, 0xfc, 0x06, 0, 0, // use_remainder
        ];
        let stack = {
            let _up = renumbering(ids, Way::Up);
            Slot::read(&mut Reader::new(&sent)).unwrap().unwrap()
        };
        assert_eq!(stack.item, 656);
        // Templates, the item first: the diamonds without the color, then no item.
        assert_eq!(
            stack.added,
            [
                (77, Component::Raw(vec![2, 1, 0xf4, 0x07, 2, 0, 0, 0])),
                (25, Component::Raw(vec![0xee, 0x07, 1, 0, 0])),
            ]
        );
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

    #[test]
    fn modifiers_and_equippables_before_1_21_6_are_read_in_the_pinned_layout() {
        use crate::buf::{Codec, Reader, Writer};
        use crate::item::{Component, Slot, Stack, component as k};
        let ids = crate::adapter::ServerVersion::V1_21_5.ids().unwrap();
        let raw = |kind: i32| ids.data_component_type.down(kind).unwrap();
        // A stone as 1.21.5 sends it, with two attribute modifiers (holders: armor is 0 in 1.21.5, 1 in
        // 26.3, so the holder is its id plus one, 1 and 3; attack damage is 2 and 3 in 26.3, so 3 and 4)
        // and an equippable for the chest: no display, no shearing fields.
        let mut sent = Writer::new();
        sent.varint(1);
        sent.varint(1);
        sent.varint(2);
        sent.varint(0);
        sent.varint(raw(k::ATTRIBUTE_MODIFIERS));
        sent.varint(2);
        for (holder, id, amount, operation, slot) in [
            (1, "minecraft:test", 2.5, 0, 6),
            (3, "minecraft:other", -1.0, 1, 1),
        ] {
            sent.varint(holder);
            sent.string(id);
            sent.f64(amount);
            sent.varint(operation);
            sent.varint(slot);
        }
        sent.varint(raw(k::EQUIPPABLE));
        sent.varint(3);
        sent.varint(71);
        for b in [false, false, false, true, true, true, false] {
            sent.bool(b);
        }
        let sent = sent.data;
        let stack: Stack = {
            let _up = renumbering(ids, Way::Up);
            Slot::read(&mut Reader::new(&sent)).unwrap().unwrap()
        };
        assert_eq!(stack.item, 1);
        // The pinned layout: each entry with a default display, then the shears' sound by default.
        let mut modifiers = Writer::new();
        modifiers.varint(2);
        for (holder, id, amount, operation, slot) in [
            (2, "minecraft:test", 2.5, 0, 6),
            (4, "minecraft:other", -1.0, 1, 1),
        ] {
            modifiers.varint(holder);
            modifiers.string(id);
            modifiers.f64(amount);
            modifiers.varint(operation);
            modifiers.varint(slot);
            modifiers.varint(0);
        }
        let mut equippable = Writer::new();
        equippable.varint(3);
        equippable.varint(71);
        for b in [false, false, false, true, true, true, false, false] {
            equippable.bool(b);
        }
        equippable.varint(1448);
        assert_eq!(
            stack.added,
            [
                (k::ATTRIBUTE_MODIFIERS, Component::Raw(modifiers.data)),
                (k::EQUIPPABLE, Component::Raw(equippable.data)),
            ]
        );
        // Written for the 1.21.5 client again: the same bytes as it sent.
        let mut w = Writer::new();
        {
            let _down = renumbering(ids, Way::Down);
            Slot::write(&Some(stack), &mut w);
        }
        assert_eq!(w.data, sent);
    }

    #[test]
    fn modifiers_and_equippables_from_1_21_6_are_kept_as_they_are() {
        use crate::buf::{Codec, Reader, Writer};
        use crate::item::{Slot, component as k};
        use crate::text::literal;
        // The pinned layout (1.21.6 and later): a display after each entry's slot, and the shearing fields.
        // No quirk applies to this version, so the bytes pass through unchanged, and the ids are the
        // version's own.
        let ids = crate::adapter::ServerVersion::V1_21_6.ids().unwrap();
        let raw = |kind: i32| ids.data_component_type.down(kind).unwrap();
        let mut sent = Writer::new();
        sent.varint(1);
        sent.varint(1);
        sent.varint(2);
        sent.varint(0);
        sent.varint(raw(k::ATTRIBUTE_MODIFIERS));
        sent.varint(1);
        sent.varint(2);
        sent.string("minecraft:test");
        sent.f64(2.5);
        sent.varint(0);
        sent.varint(6);
        sent.varint(2);
        sent.nbt(&literal("Hi"));
        sent.varint(raw(k::EQUIPPABLE));
        sent.varint(3);
        sent.varint(71);
        for b in [false, false, false, true, true, true, false, false] {
            sent.bool(b);
        }
        sent.varint(1448);
        let sent = sent.data;
        let stack = {
            let _up = renumbering(ids, Way::Up);
            Slot::read(&mut Reader::new(&sent)).unwrap().unwrap()
        };
        assert_eq!(
            stack
                .added
                .iter()
                .map(|(kind, _)| *kind)
                .collect::<Vec<_>>(),
            [k::ATTRIBUTE_MODIFIERS, k::EQUIPPABLE]
        );
        let mut w = Writer::new();
        {
            let _down = renumbering(ids, Way::Down);
            Slot::write(&Some(stack), &mut w);
        }
        assert_eq!(w.data, sent);
    }
}
