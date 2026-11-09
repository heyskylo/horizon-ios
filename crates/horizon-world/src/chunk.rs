//! Saved section palettes and storage-level chunk access. Changes here do not
//! run block behavior, lighting, heightmaps, neighbor updates or scheduled ticks.
pub use crate::block_state_properties::BlockProperties;
use crate::{
    DATA_VERSION,
    compression::invalid,
    nbt::{Compound, JavaString, NamedTag, Tag},
    registry::{BlockRegistry, BlockStateId},
    tick_data::{self, TickKind},
};
use horizon_core::{
    position::{BlockPos, ChunkPos},
    storage::PackedStorage,
    ticks::{ChunkTicks, SavedTick, TickTypeId},
};
use std::{collections::HashMap, io};

fn key(value: &str) -> JavaString {
    JavaString::from(value)
}
fn bits_for_palette(len: usize, min: u8) -> u8 {
    if len <= 1 {
        0
    } else {
        ((usize::BITS - (len - 1).leading_zeros()) as u8).max(min)
    }
}

#[derive(Clone, Debug)]
pub struct PalettedContainer {
    palette: Vec<Tag>,
    storage: Option<PackedStorage>,
    entries: usize,
    min_bits: u8,
    extra: Compound,
}
impl PalettedContainer {
    pub fn uniform(value: Tag, entries: usize, min_bits: u8) -> io::Result<Self> {
        if entries == 0 || !(1..=32).contains(&min_bits) {
            return Err(invalid("invalid palette dimensions"));
        }
        if matches!(value, Tag::End) {
            return Err(invalid("End cannot be a palette entry"));
        }
        Ok(Self {
            palette: vec![value],
            storage: None,
            entries,
            min_bits,
            extra: Compound::new(),
        })
    }

    pub fn from_nbt(tag: &Tag, entries: usize, min_bits: u8) -> io::Result<Self> {
        let Tag::Compound(map) = tag else {
            return Err(invalid("palette container is not a compound"));
        };
        let Some(Tag::List(palette)) = map.get(&key("palette")) else {
            return Err(invalid("missing palette list"));
        };
        let first = palette.first().ok_or_else(|| invalid("empty palette"))?;
        let mut result = Self::uniform(first.clone(), entries, min_bits)?;
        if palette.len() > i32::MAX as usize || palette.iter().any(|t| matches!(t, Tag::End)) {
            return Err(invalid("invalid palette"));
        }
        result.palette = palette.clone();
        let bits = bits_for_palette(palette.len(), min_bits);
        if bits > 0 {
            let Some(Tag::LongArray(words)) = map.get(&key("data")) else {
                return Err(invalid("nonuniform palette is missing data"));
            };
            let storage =
                PackedStorage::from_words(bits, entries, words.iter().map(|&w| w as u64).collect())
                    .map_err(|e| invalid(e.to_string()))?;
            for index in 0..entries {
                if storage.get(index).unwrap() as usize >= palette.len() {
                    return Err(invalid("palette index outside palette"));
                }
            }
            result.storage = Some(storage);
        }
        result.extra = map.clone();
        result.extra.remove(&key("palette"));
        result.extra.remove(&key("data"));
        Ok(result)
    }

    pub fn bits(&self) -> u8 {
        self.storage.as_ref().map_or(0, PackedStorage::bits)
    }
    pub fn palette_len(&self) -> usize {
        self.palette.len()
    }
    pub fn get(&self, index: usize) -> io::Result<&Tag> {
        if index >= self.entries {
            return Err(invalid("section index out of bounds"));
        }
        let palette_index = self.storage.as_ref().map_or(0, |s| s.get(index).unwrap()) as usize;
        Ok(&self.palette[palette_index])
    }

    pub fn set(&mut self, index: usize, value: Tag) -> io::Result<Tag> {
        let old = self.get(index)?.clone();
        if matches!(value, Tag::End) {
            return Err(invalid("End cannot be a palette entry"));
        }
        let id = if let Some(id) = self.palette.iter().position(|v| *v == value) {
            id
        } else {
            if self.palette.len() >= i32::MAX as usize {
                return Err(invalid("palette too large"));
            }
            let next_bits = bits_for_palette(self.palette.len() + 1, self.min_bits);
            if next_bits != self.bits() {
                let mut next = PackedStorage::new(next_bits, self.entries)
                    .map_err(|e| invalid(e.to_string()))?;
                if let Some(previous) = &self.storage {
                    for i in 0..self.entries {
                        next.set(i, previous.get(i).unwrap()).unwrap();
                    }
                }
                self.storage = Some(next);
            }
            self.palette.push(value);
            self.palette.len() - 1
        };
        if let Some(storage) = &mut self.storage {
            storage
                .set(index, id as u32)
                .map_err(|e| invalid(e.to_string()))?;
        }
        Ok(old)
    }

    /// Rebuild a minimal palette in first-use order, preserving every entry.
    pub fn compact(&mut self) -> io::Result<()> {
        let values = (0..self.entries)
            .map(|i| self.get(i).cloned())
            .collect::<io::Result<Vec<_>>>()?;
        let mut compact = Self::uniform(values[0].clone(), self.entries, self.min_bits)?;
        for (i, value) in values.into_iter().enumerate() {
            compact.set(i, value)?;
        }
        compact.extra = std::mem::take(&mut self.extra);
        *self = compact;
        Ok(())
    }

    /// `PalettedContainer.pack`: the cells re-encoded into a fresh palette in
    /// first-use order, so a slot no cell holds any more is not written and the
    /// bit count is the one for the entries that remain. Saving the palette as
    /// it is would keep a state a block edit replaced and pack wider than
    /// vanilla writes for the same cells.
    pub fn to_nbt(&self) -> Tag {
        let mut map = self.extra.clone();
        let mut palette: Vec<&Tag> = Vec::with_capacity(self.palette.len());
        let mut seen: HashMap<&Tag, u32> = HashMap::with_capacity(self.palette.len());
        let mut ids = Vec::with_capacity(self.entries);
        for index in 0..self.entries {
            let id = self.storage.as_ref().map_or(0, |s| s.get(index).unwrap()) as usize;
            let value = &self.palette[id];
            ids.push(*seen.entry(value).or_insert_with(|| {
                palette.push(value);
                palette.len() as u32 - 1
            }));
        }
        let bits = bits_for_palette(palette.len(), self.min_bits);
        map.insert(
            key("palette"),
            Tag::List(palette.into_iter().cloned().collect()),
        );
        if bits > 0 {
            let mut storage = PackedStorage::new(bits, self.entries).unwrap();
            for (index, id) in ids.into_iter().enumerate() {
                storage.set(index, id).unwrap();
            }
            map.insert(
                key("data"),
                Tag::LongArray(storage.words().iter().map(|&v| v as i64).collect()),
            );
        }
        Tag::Compound(map)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BlockState {
    pub name: String,
    pub properties: BlockProperties,
}
impl BlockState {
    pub fn to_nbt(&self) -> Tag {
        let mut map = Compound::new();
        map.insert(key("id"), Tag::String(key(&self.name)));
        if !self.properties.is_empty() {
            map.insert(
                key("properties"),
                Tag::Compound(
                    self.properties
                        .iter()
                        .map(|(k, v)| (key(k), Tag::String(key(v))))
                        .collect(),
                ),
            );
        }
        Tag::Compound(map)
    }
    pub fn from_nbt(tag: &Tag) -> io::Result<Self> {
        if let Tag::String(name) = tag {
            return Ok(Self {
                name: name.to_string().map_err(|e| invalid(e.to_string()))?,
                properties: BlockProperties::new(),
            });
        }
        let Some(Tag::String(name)) = tag.get("id") else {
            return Err(invalid("block state is missing id"));
        };
        let mut properties = Vec::new();
        if let Some(tag) = tag.get("properties") {
            let Tag::Compound(map) = tag else {
                return Err(invalid("block properties are not a compound"));
            };
            for (k, v) in map {
                let Tag::String(v) = v else {
                    return Err(invalid("block property is not a string"));
                };
                properties.push((
                    k.to_string().map_err(|e| invalid(e.to_string()))?,
                    v.to_string().map_err(|e| invalid(e.to_string()))?,
                ));
            }
        }
        Ok(Self {
            name: name.to_string().map_err(|e| invalid(e.to_string()))?,
            properties: properties.into_iter().collect(),
        })
    }
}

/// `SerializableChunkData.postLoadChunk`: the entities a saved full chunk keeps in its own
/// `entities` list, which `ServerLevel.addLegacyChunkEntities` adds as the chunk loads. Only a
/// chunk last saved before 1.17 gave entities files of their own (`entities/`) has any; saving a
/// full chunk writes no such list.
pub fn legacy_entities(root: &Tag) -> Vec<Tag> {
    match root.get("entities") {
        Some(Tag::List(entities)) => entities.clone(),
        _ => Vec::new(),
    }
}

#[derive(Clone, Debug)]
pub struct ChunkData {
    root: NamedTag,
    position: ChunkPos,
}
impl ChunkData {
    pub fn new(root: NamedTag) -> io::Result<Self> {
        let Some(Tag::Int(x)) = root.tag.get("xPos") else {
            return Err(invalid("chunk is missing xPos"));
        };
        let Some(Tag::Int(z)) = root.tag.get("zPos") else {
            return Err(invalid("chunk is missing zPos"));
        };
        let position = ChunkPos::new(*x, *z);
        let Some(Tag::Int(version)) = root.tag.get("DataVersion") else {
            return Err(invalid("chunk is missing DataVersion"));
        };
        if *version != DATA_VERSION {
            return Err(invalid(format!(
                "expected DataVersion {DATA_VERSION}, got {version}; data fixing is not implemented"
            )));
        }
        let Some(Tag::List(sections)) = root.tag.get("sections") else {
            return Err(invalid("chunk is missing sections"));
        };
        let mut seen = std::collections::BTreeSet::new();
        for section in sections {
            let Some(Tag::Byte(y)) = section.get("Y") else {
                return Err(invalid("section is missing byte Y"));
            };
            if !seen.insert(*y) {
                return Err(invalid("duplicate section Y"));
            }
        }
        Ok(Self { root, position })
    }
    pub fn position(&self) -> ChunkPos {
        self.position
    }
    pub fn into_nbt(self) -> NamedTag {
        self.root
    }
    pub fn root(&self) -> &NamedTag {
        &self.root
    }

    pub fn saved_ticks(
        &self,
        kind: TickKind,
        resolve: impl FnMut(&str) -> Option<TickTypeId>,
    ) -> io::Result<ChunkTicks> {
        let Some(tag) = self.root.tag.get(kind.field()) else {
            return Ok(ChunkTicks::default());
        };
        let mut ticks = tick_data::read_list(tag, resolve)?;
        ticks.retain(|tick| tick.pos.chunk() == self.position);
        Ok(ChunkTicks::from_saved(ticks))
    }

    pub fn set_saved_ticks(
        &mut self,
        kind: TickKind,
        ticks: &[SavedTick],
        name: impl FnMut(TickTypeId) -> Option<String>,
    ) -> io::Result<()> {
        if ticks.iter().any(|tick| tick.pos.chunk() != self.position) {
            return Err(invalid("saved tick is outside this chunk"));
        }
        let tag = tick_data::write_list(ticks, name)?;
        let Tag::Compound(map) = &mut self.root.tag else {
            unreachable!()
        };
        map.insert(key(kind.field()), tag);
        Ok(())
    }

    fn section_index(&self, block: BlockPos) -> io::Result<Option<usize>> {
        if block.chunk() != self.position {
            return Err(invalid("block is outside this chunk"));
        }
        let section_y = i8::try_from(block.y >> 4)
            .map_err(|_| invalid("block height outside saved section range"))?;
        let Tag::List(sections) = self.root.tag.get("sections").unwrap() else {
            unreachable!()
        };
        Ok(sections
            .iter()
            .position(|s| s.get("Y") == Some(&Tag::Byte(section_y))))
    }

    pub fn block(&self, block: BlockPos) -> io::Result<Option<BlockState>> {
        let Some(index) = self.section_index(block)? else {
            return Ok(None);
        };
        let Tag::List(sections) = self.root.tag.get("sections").unwrap() else {
            unreachable!()
        };
        let Some(container) = sections[index].get("block_states") else {
            return Ok(None);
        };
        let palette = PalettedContainer::from_nbt(container, 4096, 4)?;
        Ok(Some(BlockState::from_nbt(
            palette.get(block.section_index())?,
        )?))
    }

    /// Storage-only mutation. The caller must arrange simulation-derived updates
    /// before treating this as a gameplay block placement.
    pub fn set_block_data(&mut self, block: BlockPos, state: BlockState) -> io::Result<BlockState> {
        let index = self
            .section_index(block)?
            .ok_or_else(|| invalid("section is not present"))?;
        let Tag::List(sections) = self.root.tag.get_mut("sections").unwrap() else {
            unreachable!()
        };
        let container = sections[index]
            .get_mut("block_states")
            .ok_or_else(|| invalid("section has no block-state data"))?;
        let mut palette = PalettedContainer::from_nbt(container, 4096, 4)?;
        let old = BlockState::from_nbt(palette.get(block.section_index())?)?;
        palette.set(block.section_index(), state.to_nbt())?;
        *container = palette.to_nbt();
        Ok(old)
    }
}

/// Resident section: 4,096 blocks stored as numeric palette entries. Uniform
/// sections have no word array; property strings live in the shared registry.
/// Palettes are almost always tiny, so a local id is a scan of that vector
/// (the resident container does the same) with the last lookup cached: a run
/// of one state costs a compare, and no section allocates a hash table.
fn id_in(
    palette: &[BlockStateId],
    counts: &[u16],
    last: &mut Option<(BlockStateId, u32)>,
    state: BlockStateId,
) -> Option<u32> {
    if let Some((seen, id)) = *last
        && seen == state
        && counts[id as usize] != 0
    {
        return Some(id);
    }
    // A dead slot is still on `free`; reviving it without removing it there
    // would let the next new state overwrite it. Only live slots count.
    let id = palette
        .iter()
        .zip(counts)
        .position(|(&v, &count)| count != 0 && v == state)? as u32;
    *last = Some((state, id));
    Some(id)
}
#[cold]
fn out_of_bounds() -> io::Error {
    invalid("section index out of bounds")
}
pub struct BlockSection<'r> {
    palette: Vec<BlockStateId>,
    last: Option<(BlockStateId, u32)>,
    counts: Vec<u16>,
    free: Vec<u32>,
    storage: Option<PackedStorage>,
    registry: &'r BlockRegistry,
    extra: Compound,
}
impl<'r> BlockSection<'r> {
    /// The section's heap bytes: its packed storage, and its palette, counts
    /// and free list (`horizon-ctl memory`; extra NBT not counted).
    pub fn heap_bytes(&self) -> [usize; 2] {
        [
            self.storage.as_ref().map_or(0, PackedStorage::heap_bytes),
            self.palette.capacity() * std::mem::size_of::<BlockStateId>()
                + self.counts.capacity() * std::mem::size_of::<u16>()
                + self.free.capacity() * std::mem::size_of::<u32>(),
        ]
    }
    /// Bits per block of its packed storage, 0 for a single-state section.
    pub fn storage_bits(&self) -> u8 {
        self.storage.as_ref().map_or(0, PackedStorage::bits)
    }
    pub fn registry(&self) -> &'r BlockRegistry {
        self.registry
    }
    pub fn uniform(state: BlockStateId, registry: &'r BlockRegistry) -> io::Result<Self> {
        if registry.state(state).is_none() {
            return Err(invalid("state ID outside registry"));
        }
        Ok(Self {
            palette: vec![state],
            last: Some((state, 0)),
            counts: vec![4096],
            free: Vec::new(),
            storage: None,
            registry,
            extra: Compound::new(),
        })
    }
    pub fn from_nbt(tag: &Tag, registry: &'r BlockRegistry) -> io::Result<Self> {
        let container = PalettedContainer::from_nbt(tag, 4096, 4)?;
        let resolved = container
            .palette
            .iter()
            .map(|tag| registry.resolve(&BlockState::from_nbt(tag)?))
            .collect::<io::Result<Vec<_>>>()?;
        let first_index = container.storage.as_ref().map_or(0, |s| s.get(0).unwrap()) as usize;
        let mut section = Self::uniform(resolved[first_index], registry)?;
        for index in 0..4096 {
            let id = container
                .storage
                .as_ref()
                .map_or(0, |s| s.get(index).unwrap()) as usize;
            section.set(index, resolved[id])?;
        }
        section.extra = container.extra;
        Ok(section)
    }
    /// Test current distinct states without unpacking all 4,096 cells. The
    /// predicate should be pure; unused palette slots are intentionally skipped.
    pub fn any_state(&self, mut predicate: impl FnMut(BlockStateId) -> bool) -> bool {
        self.palette
            .iter()
            .zip(&self.counts)
            .any(|(&state, &count)| count != 0 && predicate(state))
    }
    pub fn bits(&self) -> u8 {
        self.storage.as_ref().map_or(0, PackedStorage::bits)
    }
    #[inline]
    pub fn get(&self, index: usize) -> io::Result<BlockStateId> {
        if index >= 4096 {
            return Err(out_of_bounds());
        }
        Ok(self.state(index))
    }
    /// Every state in index order, as [`state`](Self::state) reads them one by
    /// one, unpacked word by word.
    pub(crate) fn fill_states(&self, out: &mut [u32; 4096]) {
        match &self.storage {
            None => out.fill(self.palette[0].0),
            Some(storage) => {
                storage.unpack_into(out);
                for v in out.iter_mut() {
                    *v = self.palette[*v as usize].0;
                }
            }
        }
    }
    /// The state at `index`, which must be below 4096 (a `local_index`).
    #[inline]
    pub(crate) fn state(&self, index: usize) -> BlockStateId {
        debug_assert!(index < 4096);
        let id = self.storage.as_ref().map_or(0, |s| s.read(index));
        self.palette[id as usize]
    }
    /// The state of every block, when the section stores only one (no
    /// word array).
    #[inline]
    pub(crate) fn uniform_state(&self) -> Option<BlockStateId> {
        self.storage.is_none().then(|| self.palette[0])
    }
    pub fn set(&mut self, index: usize, state: BlockStateId) -> io::Result<BlockStateId> {
        if index >= 4096 {
            return Err(invalid("section index out of bounds"));
        }
        if state.0 as usize >= self.registry.len() {
            return Err(invalid("state ID outside registry"));
        }
        // A uniform section has one palette slot, all 4,096 blocks and no free
        // slots, so a write either matches it or grows it to two states.
        let Some(storage) = self.storage.as_ref() else {
            debug_assert!(self.palette.len() == 1 && self.counts == [4096] && self.free.is_empty());
            let old = self.palette[0];
            if old == state {
                return Ok(old);
            }
            let bits = bits_for_palette(2, 4);
            let mut storage = PackedStorage::new(bits, 4096).map_err(|e| invalid(e.to_string()))?;
            storage.write(index, 1);
            self.storage = Some(storage);
            self.palette.push(state);
            self.counts[0] = 4095;
            self.counts.push(1);
            self.last = Some((state, 1));
            return Ok(old);
        };
        // The slot at `index` is the old state's id, so no palette lookup is
        // needed for it; only the new state has to be resolved. Reading and
        // writing that slot share one position computation; growing the palette
        // re-packs the storage, which looks the position up again.
        let mut slot = storage.slot_of(index);
        let old_id = storage.read_at(slot);
        let old = self.palette[old_id as usize];
        if old == state {
            return Ok(old);
        }
        let id = match id_in(&self.palette, &self.counts, &mut self.last, state) {
            Some(id) => id,
            None if self.counts[old_id as usize] == 1 => {
                // Replacing the last occurrence can reuse its slot without repacking.
                self.palette[old_id as usize] = state;
                self.last = Some((state, old_id));
                return Ok(old);
            }
            None => {
                if let Some(id) = self.free.pop() {
                    self.palette[id as usize] = state;
                    self.last = Some((state, id));
                    id
                } else {
                    let id = self.palette.len() as u32;
                    let next_bits = bits_for_palette(self.palette.len() + 1, 4);
                    if next_bits != self.bits() {
                        let mut storage = PackedStorage::new(next_bits, 4096)
                            .map_err(|e| invalid(e.to_string()))?;
                        for i in 0..4096 {
                            storage.write(i, self.storage.as_ref().unwrap().read(i));
                        }
                        slot = storage.slot_of(index);
                        self.storage = Some(storage);
                    }
                    self.palette.push(state);
                    self.counts.push(0);
                    self.last = Some((state, id));
                    id
                }
            }
        };
        self.storage.as_mut().unwrap().write_at(slot, id);
        // Both counts are read once and written back once: the two checks below
        // use the values this call produced.
        let old_count = self.counts[old_id as usize] - 1;
        self.counts[old_id as usize] = old_count;
        let new_count = self.counts[id as usize] + 1;
        self.counts[id as usize] = new_count;
        if old_count == 0 {
            self.free.push(old_id);
            if self.last == Some((old, old_id)) {
                self.last = None;
            }
        }
        if new_count == 4096 {
            self.palette = vec![state];
            self.last = Some((state, 0));
            self.counts = vec![4096];
            self.free.clear();
            self.storage = None;
        }
        Ok(old)
    }
    /// Write `len` consecutive values at (`x`, `y_start..y_start+len`, `z`) with
    /// `state`, all inside this section. Resolves the palette id once, then writes
    /// the rest directly; returns how many previous blocks were `air` so the
    /// caller can update its non-air count. The final palette and counts match
    /// `len` calls to `set` in the same top-down order.
    pub fn fill_run(
        &mut self,
        x: i32,
        y_start: i32,
        z: i32,
        len: i32,
        state: BlockStateId,
        air: BlockStateId,
    ) -> io::Result<u16> {
        if !(0..16).contains(&x)
            || !(0..16).contains(&z)
            || len <= 0
            || y_start < 0
            || y_start + len > 16
        {
            return Err(invalid("section fill outside its bounds"));
        }
        let index = |y: i32| ((y as usize) << 8) | ((z as usize) << 4) | x as usize;
        let top = y_start + len - 1;
        let mut added = 0u16;
        // The first write takes the general path: it may add the state to the
        // palette, allocate storage, or find the section already uniform.
        let old = self.set(index(top), state)?;
        if old == air {
            added += 1;
        }
        let Some(id) = id_in(&self.palette, &self.counts, &mut self.last, state) else {
            return Ok(added);
        };
        // Ids are unique among live palette slots, so a cell already holds
        // `state` exactly when it holds `id`, and held `air` exactly when it
        // held `air_id` (written as a sentinel: no slot is that large).
        let air_id = id_in(&self.palette, &self.counts, &mut self.last, air).unwrap_or(u32::MAX);
        let head = index(top);
        // A run in one column moves 256 entries per Y. While a word holds a
        // whole number of those steps (every width generation produces: 1, 2, 4,
        // 8, 16 and 32 bits per entry) the bit shift stays put, so the run walks
        // its words directly. Other widths step the shift as well and go one
        // `set` per block: the palette edits and non-air counts are the same,
        // and the run's state is already in the palette by now.
        const STRIDE: usize = 1 << 8;
        let fixed = match self.storage.as_ref() {
            // No word array: every block already is what the first write wrote.
            None => return Ok(added),
            Some(storage) => storage.fixed_shift(STRIDE),
        };
        if !fixed {
            // A cell that already held the run's state is not counted, so a run
            // of air over air adds nothing here; the top block above is counted
            // either way.
            for y in (y_start..top).rev() {
                if self.set(index(y), state)? == air && state != air {
                    added += 1;
                }
            }
            return Ok(added);
        }
        let counts = &mut self.counts;
        let free = &mut self.free;
        let last = &mut self.last;
        let palette = &self.palette;
        // `counts[id]` is read nowhere in the loop, so its shares are added up.
        let mut gained = 0u16;
        let mut run = self
            .storage
            .as_mut()
            .expect("the section is not uniform")
            .word_run(head, STRIDE, id);
        // Counted down rather than iterated over the Y range: the range's own
        // bookkeeping was a sixth of the loop's instructions.
        let mut left = top - y_start;
        while left > 0 {
            left -= 1;
            let old_id = run.write_back();
            if old_id == id {
                continue;
            }
            gained += 1;
            let old_count = counts[old_id as usize] - 1;
            counts[old_id as usize] = old_count;
            if old_id == air_id {
                added += 1;
            }
            if old_count == 0 {
                free.push(old_id);
                if *last == Some((palette[old_id as usize], old_id)) {
                    *last = None;
                }
            }
        }
        counts[id as usize] += gained;
        if self.counts[id as usize] == 4096 {
            self.palette = vec![state];
            self.last = Some((state, 0));
            self.counts = vec![4096];
            self.free.clear();
            self.storage = None;
        }
        Ok(added)
    }
    pub fn compact(&mut self) -> io::Result<()> {
        let values: Vec<_> = (0..4096).map(|i| self.get(i).unwrap()).collect();
        self.palette.clear();
        self.last = None;
        self.counts.clear();
        self.free.clear();
        for state in &values {
            if let Some(id) = id_in(&self.palette, &self.counts, &mut self.last, *state) {
                self.counts[id as usize] += 1;
            } else {
                self.palette.push(*state);
                self.counts.push(1);
                self.last = Some((*state, self.palette.len() as u32 - 1));
            }
        }
        let bits = bits_for_palette(self.palette.len(), 4);
        self.storage = if bits == 0 {
            None
        } else {
            let mut storage = PackedStorage::new(bits, 4096).map_err(|e| invalid(e.to_string()))?;
            let mut last = None;
            for (i, state) in values.iter().enumerate() {
                let id = id_in(&self.palette, &self.counts, &mut last, *state).unwrap();
                storage.set(i, id).unwrap();
            }
            Some(storage)
        };
        Ok(())
    }
    pub fn to_nbt(&self) -> io::Result<Tag> {
        let mut map = self.extra.clone();
        let palette = self
            .palette
            .iter()
            .map(|id| {
                self.registry
                    .state(*id)
                    .map(BlockState::to_nbt)
                    .ok_or_else(|| invalid("state ID outside registry"))
            })
            .collect::<io::Result<Vec<_>>>()?;
        map.insert(key("palette"), Tag::List(palette));
        if let Some(storage) = &self.storage {
            map.insert(
                key("data"),
                Tag::LongArray(storage.words().iter().map(|&w| w as i64).collect()),
            );
        }
        Ok(Tag::Compound(map))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn saved(palette: &PalettedContainer) -> (usize, Option<usize>) {
        let Tag::Compound(map) = palette.to_nbt() else {
            unreachable!()
        };
        let entries = match map.get(&key("palette")) {
            Some(Tag::List(entries)) => entries.len(),
            other => panic!("palette is {other:?}"),
        };
        let words = match map.get(&key("data")) {
            Some(Tag::LongArray(words)) => Some(words.len()),
            None => None,
            other => panic!("data is {other:?}"),
        };
        (entries, words)
    }

    /// `PalettedContainer.pack` re-encodes the cells into a fresh palette, so a
    /// state no cell holds any more is not written and the bit count comes from
    /// the states that remain: `getConfigurationForPaletteSize(17)` is
    /// `SimpleBitStorage(5, 4096)` (342 longs), `(16)` is `(4, 4096)` (256).
    #[test]
    fn a_state_no_cell_holds_is_not_saved() {
        let mut palette = PalettedContainer::uniform(Tag::Int(0), 4096, 4).unwrap();
        for index in 0..16 {
            palette.set(index, Tag::Int(index as i32 + 1)).unwrap();
        }
        assert_eq!((palette.palette_len(), palette.bits()), (17, 5));
        palette.set(15, Tag::Int(0)).unwrap();

        let Tag::Compound(map) = palette.to_nbt() else {
            unreachable!()
        };
        let Some(Tag::LongArray(words)) = map.get(&key("data")) else {
            panic!("no data")
        };
        assert_eq!(words.len(), 256);
        let Some(Tag::List(entries)) = map.get(&key("palette")) else {
            panic!("no palette")
        };
        // First use over the cells: state 1 at index 0 … state 15 at index 14,
        // then air everywhere from index 15 on.
        let first_use = (1..=15)
            .map(Tag::Int)
            .chain([Tag::Int(0)])
            .collect::<Vec<_>>();
        assert_eq!(entries, &first_use);
        let loaded = PalettedContainer::from_nbt(&palette.to_nbt(), 4096, 4).unwrap();
        for index in 0..4096 {
            let want = if index <= 14 {
                Tag::Int(index as i32 + 1)
            } else {
                Tag::Int(0)
            };
            assert_eq!(loaded.get(index).unwrap(), &want);
        }
    }

    /// `pack` writes no `data` once one state is all that is left: a palette of
    /// one is configuration bits 0, `ZeroBitStorage`, and empty `values`.
    #[test]
    fn a_section_that_became_uniform_saves_no_data() {
        let mut palette = PalettedContainer::uniform(Tag::Int(0), 4096, 4).unwrap();
        palette.set(4000, Tag::Int(9)).unwrap();
        assert_eq!(saved(&palette), (2, Some(256)));
        palette.set(4000, Tag::Int(0)).unwrap();
        assert_eq!(saved(&palette), (1, None));
    }

    #[test]
    fn palette_grows_across_word_widths_and_compacts_to_uniform() {
        let mut palette = PalettedContainer::uniform(Tag::Int(0), 4096, 4).unwrap();
        for i in 0..4096 {
            palette.set(i, Tag::Int(i as i32)).unwrap();
        }
        assert_eq!(palette.bits(), 12);
        let mut loaded = PalettedContainer::from_nbt(&palette.to_nbt(), 4096, 4).unwrap();
        for i in 0..4096 {
            assert_eq!(loaded.get(i).unwrap(), &Tag::Int(i as i32));
            loaded.set(i, Tag::Int(7)).unwrap();
        }
        loaded.compact().unwrap();
        assert_eq!(loaded.bits(), 0);
        assert_eq!(loaded.palette_len(), 1);
        assert!(loaded.to_nbt().get("data").is_none());
        assert_eq!(loaded.get(4095).unwrap(), &Tag::Int(7));
    }
    #[test]
    fn invalid_palette_data_fails_before_access() {
        let mut tag = Tag::compound([("palette", Tag::List(vec![Tag::Int(0), Tag::Int(1)]))]);
        assert!(PalettedContainer::from_nbt(&tag, 4096, 4).is_err());
        let Tag::Compound(map) = &mut tag else {
            unreachable!()
        };
        map.insert(key("data"), Tag::LongArray(vec![-1; 256]));
        assert!(PalettedContainer::from_nbt(&tag, 4096, 4).is_err());
    }

    /// The pre-optimization `get`, kept to compare against in the benchmark.
    fn baseline_get(section: &BlockSection<'_>, index: usize) -> io::Result<BlockStateId> {
        if index >= 4096 {
            return Err(invalid("section index out of bounds"));
        }
        let id = section
            .storage
            .as_ref()
            .map_or(0, |s| s.get(index).unwrap());
        Ok(section.palette[id as usize])
    }
    /// The pre-optimization `set`, kept to compare against in the benchmark.
    fn baseline_set(
        section: &mut BlockSection<'_>,
        index: usize,
        state: BlockStateId,
    ) -> io::Result<BlockStateId> {
        let old = baseline_get(section, index)?;
        if state.0 as usize >= section.registry.len() {
            return Err(invalid("state ID outside registry"));
        }
        if old == state {
            return Ok(old);
        }
        // Dead slots stay in `palette` but are on the free list: the old map
        // removed them, so only live entries count for either lookup.
        let old_id = section
            .palette
            .iter()
            .enumerate()
            .find(|&(id, &s)| s == old && section.counts[id] != 0)
            .map(|(id, _)| id as u32)
            .unwrap();
        let known = section
            .palette
            .iter()
            .enumerate()
            .find(|&(id, &s)| s == state && section.counts[id] != 0)
            .map(|(id, _)| id as u32);
        if section.counts[old_id as usize] == 1 && known.is_none() {
            section.palette[old_id as usize] = state;
            return Ok(old);
        }
        let id = if let Some(id) = known {
            id
        } else if let Some(id) = section.free.pop() {
            section.palette[id as usize] = state;
            id
        } else {
            let id = section.palette.len() as u32;
            let next_bits = bits_for_palette(section.palette.len() + 1, 4);
            if next_bits != section.bits() {
                let mut storage =
                    PackedStorage::new(next_bits, 4096).map_err(|e| invalid(e.to_string()))?;
                if let Some(old) = &section.storage {
                    for i in 0..4096 {
                        storage.set(i, old.get(i).unwrap()).unwrap();
                    }
                }
                section.storage = Some(storage);
            }
            section.palette.push(state);
            section.counts.push(0);
            id
        };
        section
            .storage
            .as_mut()
            .unwrap()
            .set(index, id)
            .map_err(|e| invalid(e.to_string()))?;
        section.counts[old_id as usize] -= 1;
        section.counts[id as usize] += 1;
        if section.counts[old_id as usize] == 0 {
            section.free.push(old_id);
        }
        if section.counts[id as usize] == 4096 {
            section.palette = vec![state];
            section.counts = vec![4096];
            section.free.clear();
            section.storage = None;
        }
        Ok(old)
    }

    /// `main`'s `fill_run`, unchanged except that it writes through `set`
    /// (checked against `baseline_set` above) instead of that file's own copy.
    fn reference_fill_run(
        section: &mut BlockSection<'_>,
        x: i32,
        y_start: i32,
        z: i32,
        len: i32,
        state: BlockStateId,
        air: BlockStateId,
    ) -> io::Result<u16> {
        if !(0..16).contains(&x)
            || !(0..16).contains(&z)
            || len <= 0
            || y_start < 0
            || y_start + len > 16
        {
            return Err(invalid("section fill outside its bounds"));
        }
        let index = |y: i32| ((y as usize) << 8) | ((z as usize) << 4) | x as usize;
        let top = y_start + len - 1;
        let mut added = 0u16;
        let old = section.set(index(top), state)?;
        if old == air {
            added += 1;
        }
        let Some(id) = id_in(&section.palette, &section.counts, &mut section.last, state) else {
            return Ok(added);
        };
        let Some(storage) = section.storage.as_mut() else {
            return Ok(added);
        };
        for y in (y_start..top).rev() {
            let i = index(y);
            let old_id = storage.read(i);
            let old_state = section.palette[old_id as usize];
            if old_state == state {
                continue;
            }
            storage.write(i, id);
            section.counts[old_id as usize] -= 1;
            section.counts[id as usize] += 1;
            if old_state == air {
                added += 1;
            }
            if section.counts[old_id as usize] == 0 {
                section.free.push(old_id);
                if section.last == Some((old_state, old_id)) {
                    section.last = None;
                }
            }
        }
        if section.counts[id as usize] == 4096 {
            section.palette = vec![state];
            section.last = Some((state, 0));
            section.counts = vec![4096];
            section.free.clear();
            section.storage = None;
        }
        Ok(added)
    }

    /// Palette order, counts, free slots, width and every packed word.
    fn assert_fields(section: &BlockSection<'_>, reference: &BlockSection<'_>, what: &str) {
        assert_eq!(section.palette, reference.palette, "{what}: palette");
        assert_eq!(section.counts, reference.counts, "{what}: counts");
        assert_eq!(section.free, reference.free, "{what}: free slots");
        assert_eq!(section.bits(), reference.bits(), "{what}: bits");
        assert_eq!(
            section.storage.as_ref().map(|s| s.words()),
            reference.storage.as_ref().map(|s| s.words()),
            "{what}: packed words"
        );
    }

    /// Every block, as `get` reads it.
    fn assert_blocks(section: &BlockSection<'_>, reference: &BlockSection<'_>, what: &str) {
        for index in 0..4096 {
            assert_eq!(
                section.state(index),
                reference.state(index),
                "{what}: block {index}"
            );
        }
    }

    /// `set` and `fill_run` against `main`'s implementations, over every width
    /// the palette can pick: 1 state, 2..4, 5..16, 17..32, 33..64, 65..128,
    /// 129..256 and the global ids past that (4 bits to 12). Runs cover whole
    /// sections, single blocks and every length in between.
    #[test]
    fn fill_run_and_set_match_reference_across_palette_widths() {
        let registry = benchmark_registry(4100);
        let air = BlockStateId(0);
        for &(distinct, seed) in &[
            (1u64, 11u64),
            (2, 12),
            (3, 13),
            (4, 14),
            (5, 15),
            (16, 16),
            (17, 17),
            (31, 18),
            (32, 19),
            (33, 20),
            (64, 21),
            (65, 22),
            (128, 23),
            (129, 24),
            (200, 25),
            (256, 26),
            (257, 27),
            (400, 28),
            (4096, 29),
        ] {
            let mut section = BlockSection::uniform(air, &registry).unwrap();
            let mut reference = BlockSection::uniform(air, &registry).unwrap();
            let mut rng = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
            let mut next = || {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                rng
            };
            for step in 0..600u32 {
                let state = BlockStateId((next() % distinct) as u32);
                if step % 3 == 0 {
                    let index = (next() % 4096) as usize;
                    let what = format!("distinct {distinct} step {step} set {index}");
                    assert_eq!(
                        section.set(index, state).unwrap(),
                        reference.set(index, state).unwrap(),
                        "{what}"
                    );
                    assert_fields(&section, &reference, &what);
                    continue;
                }
                let x = (next() % 16) as i32;
                let z = (next() % 16) as i32;
                let y_start = (next() % 16) as i32;
                let len = 1 + (next() % (16 - y_start as u64)) as i32;
                // Every section width, and fills that land on the section top
                // and bottom.
                let len = if step % 7 == 0 { 16 - y_start } else { len };
                let what = format!("distinct {distinct} step {step} run {x},{y_start},{z}+{len}");
                assert_eq!(
                    section.fill_run(x, y_start, z, len, state, air).unwrap(),
                    reference_fill_run(&mut reference, x, y_start, z, len, state, air).unwrap(),
                    "{what}: air blocks added"
                );
                assert_fields(&section, &reference, &what);
                if step % 16 == 0 {
                    assert_blocks(&section, &reference, &what);
                }
            }
            // One state over the whole section collapses it to a uniform one.
            let state = BlockStateId((next() % distinct) as u32);
            for x in 0..16 {
                for z in 0..16 {
                    assert_eq!(
                        section.fill_run(x, 0, z, 16, state, air).unwrap(),
                        reference_fill_run(&mut reference, x, 0, z, 16, state, air).unwrap()
                    );
                }
            }
            assert_fields(&section, &reference, "collapsed");
            assert_blocks(&section, &reference, "collapsed");
            assert_eq!(section.bits(), 0, "a uniform section has no words");
        }
    }

    fn benchmark_registry(states: usize) -> BlockRegistry {
        let list: Vec<Tag> = (0..states)
            .map(|i| {
                Tag::compound([("id", Tag::String(format!("minecraft:b{i}").as_str().into()))])
            })
            .collect();
        let defaults: std::collections::BTreeMap<_, _> = (0..states)
            .map(|i| {
                (
                    JavaString::from(format!("minecraft:b{i}").as_str()),
                    Tag::Int(i as i32),
                )
            })
            .collect();
        let tag = Tag::compound([
            ("DataVersion", Tag::Int(crate::DATA_VERSION)),
            ("states", Tag::List(list)),
            ("defaults", Tag::Compound(defaults)),
        ]);
        BlockRegistry::from_nbt(&tag).unwrap()
    }

    /// `fill_states` reads what `state` reads at every index, from a uniform
    /// section (no storage) through every palette width.
    #[test]
    fn fill_states_matches_state() {
        let registry = benchmark_registry(64);
        for &(distinct, seed) in &[(1u64, 1u64), (2, 2), (16, 3), (17, 4), (64, 6)] {
            let mut section = BlockSection::uniform(BlockStateId(0), &registry).unwrap();
            let mut rng = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
            let mut out = [0u32; 4096];
            for round in 0..4 {
                section.fill_states(&mut out);
                for (index, &value) in out.iter().enumerate() {
                    assert_eq!(value, section.state(index).0, "round {round} index {index}");
                }
                for _ in 0..2_000 {
                    rng ^= rng << 13;
                    rng ^= rng >> 7;
                    rng ^= rng << 17;
                    let index = (rng % 4096) as usize;
                    let value = ((rng >> 20) % distinct) as u32;
                    section.set(index, BlockStateId(value)).unwrap();
                }
            }
        }
    }

    /// The optimized `set`/`get` must agree with the pre-optimization ones on
    /// every field, across growth, collapse and reclaimed palette slots.
    #[test]
    fn set_matches_baseline_under_random_edits() {
        let registry = benchmark_registry(64);
        for &(distinct, seed) in &[(1u64, 1u64), (2, 2), (16, 3), (17, 4), (24, 5), (64, 6)] {
            let mut base = BlockSection::uniform(BlockStateId(0), &registry).unwrap();
            let mut fast = BlockSection::uniform(BlockStateId(0), &registry).unwrap();
            let mut rng = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
            let mut next = || {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                rng
            };
            for step in 0..50_000u32 {
                if step % 5_000 == 4_999 {
                    let value = (next() % distinct) as u32;
                    for index in 0..4096 {
                        assert_eq!(
                            baseline_set(&mut base, index, BlockStateId(value)).unwrap(),
                            fast.set(index, BlockStateId(value)).unwrap()
                        );
                    }
                } else {
                    let index = (next() % 4096) as usize;
                    let value = (next() % distinct) as u32;
                    assert_eq!(
                        baseline_set(&mut base, index, BlockStateId(value)).unwrap(),
                        fast.set(index, BlockStateId(value)).unwrap()
                    );
                }
                assert_eq!(base.bits(), fast.bits());
            }
            assert_eq!(base.palette, fast.palette);
            assert_eq!(base.counts, fast.counts);
            assert_eq!(base.free, fast.free);
            for index in 0..4096 {
                assert_eq!(
                    baseline_get(&base, index).unwrap(),
                    fast.get(index).unwrap(),
                    "distinct {distinct} index {index}"
                );
            }
            assert_eq!(base.to_nbt().unwrap(), fast.to_nbt().unwrap());
        }
    }

    /// A lookup must not revive a freed palette slot: the next new state pops
    /// that slot off the free list and would silently overwrite it.
    #[test]
    fn lookups_do_not_revive_freed_palette_slots() {
        let registry = benchmark_registry(8);
        let mut section = BlockSection::uniform(BlockStateId(0), &registry).unwrap();
        for (index, state) in [(3, 3), (0, 1), (0, 0), (1, 1), (2, 2)] {
            section.set(index, BlockStateId(state)).unwrap();
        }
        assert_eq!(section.get(1).unwrap(), BlockStateId(1));
        assert_eq!(section.get(2).unwrap(), BlockStateId(2));
        assert_eq!(section.get(3).unwrap(), BlockStateId(3));
    }

    /// `set` and `get` against the pre-optimization implementations, on the same
    /// edit stream. The result is checked before timing; 51 measured rounds
    /// alternate order and report the median and best nanosecond-per-operation.
    #[test]
    #[ignore = "microbenchmark; run with --ignored --nocapture"]
    fn benchmark_section_get_set() {
        use std::{hint::black_box, time::Instant};
        const OPS: usize = 50_000;
        const ROUNDS: u32 = 51;
        let registry = benchmark_registry(32);
        let mut rng = 0x1234_5678_9abc_def1u64;
        let mut next = || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };
        let ops: Vec<(usize, u32)> = (0..OPS)
            .map(|_| ((next() % 4096) as usize, (next() % 24) as u32))
            .collect();
        let make = || BlockSection::uniform(BlockStateId(0), &registry).unwrap();

        let mut base = make();
        let mut fast = make();
        for &(index, value) in &ops {
            assert_eq!(
                baseline_set(&mut base, index, BlockStateId(value)).unwrap(),
                fast.set(index, BlockStateId(value)).unwrap()
            );
        }
        assert_eq!(base.bits(), fast.bits());
        assert_eq!(base.palette, fast.palette);
        assert_eq!(base.counts, fast.counts);
        assert_eq!(base.free, fast.free);
        assert_eq!(base.to_nbt().unwrap(), fast.to_nbt().unwrap());
        for i in 0..4096 {
            assert_eq!(baseline_get(&base, i).unwrap(), fast.get(i).unwrap());
        }

        let mut times = [Vec::new(), Vec::new()];
        for round in 0..ROUNDS {
            for optimized in [round % 2 == 0, round % 2 != 0] {
                let mut section = make();
                let started = Instant::now();
                for &(index, value) in &ops {
                    if optimized {
                        section
                            .set(black_box(index), black_box(BlockStateId(value)))
                            .unwrap();
                    } else {
                        baseline_set(
                            &mut section,
                            black_box(index),
                            black_box(BlockStateId(value)),
                        )
                        .unwrap();
                    }
                }
                black_box(&section);
                if round > 0 {
                    times[usize::from(optimized)]
                        .push(started.elapsed().as_secs_f64() * 1e9 / OPS as f64);
                }
            }
        }
        for values in &mut times {
            values.sort_by(f64::total_cmp);
        }
        let median = |v: &[f64]| v[v.len() / 2];
        println!(
            "section set median: baseline {:.3} ns/op optimized {:.3} ns/op speedup {:.3}x",
            median(&times[0]),
            median(&times[1]),
            median(&times[0]) / median(&times[1])
        );
        println!(
            "section set best:   baseline {:.3} ns/op optimized {:.3} ns/op speedup {:.3}x",
            times[0][0],
            times[1][0],
            times[0][0] / times[1][0]
        );

        let mut get_times = [Vec::new(), Vec::new()];
        for round in 0..ROUNDS {
            for optimized in [round % 2 == 0, round % 2 != 0] {
                let started = Instant::now();
                let mut acc = 0u32;
                for &(index, _) in &ops {
                    let state = if optimized {
                        fast.get(black_box(index)).unwrap()
                    } else {
                        baseline_get(&base, black_box(index)).unwrap()
                    };
                    acc ^= state.0;
                }
                black_box(acc);
                if round > 0 {
                    get_times[usize::from(optimized)]
                        .push(started.elapsed().as_secs_f64() * 1e9 / OPS as f64);
                }
            }
        }
        for values in &mut get_times {
            values.sort_by(f64::total_cmp);
        }
        println!(
            "section get median: baseline {:.3} ns/op optimized {:.3} ns/op speedup {:.3}x",
            median(&get_times[0]),
            median(&get_times[1]),
            median(&get_times[0]) / median(&get_times[1])
        );
        println!(
            "section get best:   baseline {:.3} ns/op optimized {:.3} ns/op speedup {:.3}x",
            get_times[0][0],
            get_times[1][0],
            get_times[0][0] / get_times[1][0]
        );
    }
}
