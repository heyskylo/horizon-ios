//! StructureTemplate: NBT loading, palettes, transforms and bounding boxes.
use super::{BoundingBox, Mirror, Rotation, StateTransforms};
use crate::{
    chunk::BlockState,
    nbt::{self, Compound, Tag},
    registry::{BlockRegistry, BlockStateId},
};
use std::sync::{Arc, OnceLock};

#[derive(Clone, Debug, PartialEq)]
pub struct BlockInfo {
    pub pos: [i32; 3],
    pub state: BlockStateId,
    /// Boxed: nearly no block has any, and a template holds thousands of these for the session.
    pub nbt: Option<Box<Compound>>,
}
#[derive(Clone, Debug)]
pub struct JigsawInfo {
    pub info: BlockInfo,
    pub rollable: bool,
    pub name: String,
    pub pool: String,
    pub target: String,
    pub placement_priority: i32,
    pub selection_priority: i32,
}
#[derive(Debug)]
pub struct Palette {
    blocks: Vec<BlockInfo>,
    jigsaws: OnceLock<Vec<JigsawInfo>>,
}
impl Palette {
    pub fn blocks(&self) -> &[BlockInfo] {
        &self.blocks
    }
}
/// StructureTemplate.StructureEntityInfo: an entity saved with the template.
#[derive(Clone, Debug)]
pub struct EntityInfo {
    pub pos: [f64; 3],
    pub block_pos: [i32; 3],
    pub nbt: Compound,
}
#[derive(Debug)]
pub struct StructureTemplate {
    /// The id it was loaded under (`StructureResources::template`); empty
    /// for a template read from elsewhere.
    pub location: Arc<str>,
    pub size: [i32; 3],
    palettes: Vec<Palette>,
    pub entities: Vec<EntityInfo>,
}

/// `NumericTag.intValue`, used by `CompoundTag.getIntOr` and `ListTag.getIntOr`.
/// Float tags floor before Java's saturating cast; long tags keep the low bits.
fn int_value(tag: Option<&Tag>) -> i32 {
    match tag {
        Some(Tag::Byte(v)) => i32::from(*v),
        Some(Tag::Short(v)) => i32::from(*v),
        Some(Tag::Int(v)) => *v,
        Some(Tag::Long(v)) => *v as i32,
        Some(Tag::Float(v)) => f32::from_bits(*v).floor() as i32,
        Some(Tag::Double(v)) => f64::from_bits(*v).floor() as i32,
        _ => 0,
    }
}

fn int_list(tag: Option<&Tag>) -> [i32; 3] {
    let mut out = [0; 3];
    if let Some(Tag::List(values)) = tag {
        for (slot, value) in out.iter_mut().zip(values) {
            *slot = int_value(Some(value));
        }
    }
    out
}
fn string(tag: Option<&Tag>) -> Option<String> {
    match tag {
        Some(Tag::String(s)) => Some(s.to_string_lossy()),
        _ => None,
    }
}
fn get<'a>(compound: &'a Compound, key: &str) -> Option<&'a Tag> {
    compound.get(&crate::nbt::JavaString::from(key))
}

/// One `blocks` entry before its palette resolves the `state` index: what
/// [`StructureTemplate::from_nbt`] and [`StructureTemplate::from_bytes`]
/// both collect.
struct RawBlock {
    pos: [i32; 3],
    state: i32,
    nbt: Option<Compound>,
}

/// A `blocks` element out of a `Tag` tree; non-compounds are skipped.
fn raw_block(tag: &Tag) -> Option<RawBlock> {
    let Tag::Compound(block) = tag else {
        return None;
    };
    Some(RawBlock {
        pos: int_list(get(block, "pos")),
        state: int_value(get(block, "state")),
        nbt: match get(block, "nbt") {
            Some(Tag::Compound(c)) => Some(c.clone()),
            _ => None,
        },
    })
}

/// `StructureTemplate.loadPalette`: the palette's block states in order and the template's
/// shared block list, as full blocks, then other blocks, then block
/// entities, each sorted by y, x, z.
fn load_palette(
    states: &[Tag],
    blocks: &[RawBlock],
    registry: &BlockRegistry,
    transforms: &StateTransforms,
) -> Palette {
    let states: Vec<BlockStateId> = states.iter().map(|t| read_state(registry, t)).collect();
    let air = registry.default_state("minecraft:air").unwrap();
    let mut lists: [Vec<BlockInfo>; 3] = Default::default();
    for block in blocks {
        let state = usize::try_from(block.state)
            .ok()
            .and_then(|i| states.get(i))
            .copied()
            .unwrap_or(air);
        let class = if block.nbt.is_some() {
            2
        } else if transforms.full_block(state) {
            0
        } else {
            1
        };
        lists[class].push(BlockInfo {
            pos: block.pos,
            state,
            nbt: block.nbt.clone().map(Box::new),
        });
    }
    let mut result = Vec::with_capacity(blocks.len());
    for mut list in lists {
        list.sort_by_key(|b| (b.pos[1], b.pos[0], b.pos[2]));
        result.extend(list);
    }
    Palette {
        blocks: result,
        jigsaws: OnceLock::new(),
    }
}

/// `StructureTemplate.load`'s entity loop: only entities with a compound `nbt` are kept.
fn entities_of(list: &[Tag]) -> Vec<EntityInfo> {
    let mut entities = Vec::new();
    for e in list {
        let Tag::Compound(e) = e else { continue };
        let Some(Tag::Compound(nbt)) = get(e, "nbt") else {
            continue;
        };
        let mut pos = [0.0; 3];
        if let Some(Tag::List(p)) = get(e, "pos") {
            for (slot, v) in pos.iter_mut().zip(p) {
                *slot = crate::datafix::dynamic::as_double(Some(v), 0.0);
            }
        }
        entities.push(EntityInfo {
            pos,
            block_pos: int_list(get(e, "blockPos")),
            nbt: nbt.clone(),
        });
    }
    entities
}
/// NbtUtils.readBlockState: unknown blocks read as air; missing or invalid
/// properties keep the block's defaults.
pub(super) fn read_state(registry: &BlockRegistry, tag: &Tag) -> BlockStateId {
    let air = registry.default_state("minecraft:air").expect("air");
    let Tag::Compound(compound) = tag else {
        return air;
    };
    // 26.3 writes `id`/`properties`; older templates use `Name`/`Properties`.
    let Some(name) = string(get(compound, "id")).or_else(|| string(get(compound, "Name"))) else {
        return air;
    };
    let Some(default) = registry.default_state(&name) else {
        return air;
    };
    let mut state = registry.state(default).unwrap().clone();
    if let Some(Tag::Compound(properties)) =
        get(compound, "properties").or_else(|| get(compound, "Properties"))
    {
        for (key, value) in properties {
            let (Tag::String(value), key) = (value, key.to_string_lossy()) else {
                continue;
            };
            let value = value.to_string_lossy();
            if let Some(slot) = state.properties.get_mut(&key) {
                let previous = std::mem::replace(slot, value);
                if registry.id(&state).is_none() {
                    *state.properties.get_mut(&key).unwrap() = previous;
                }
            }
        }
    }
    registry.id(&state).unwrap_or(default)
}
/// Parse `name[key=value,...]` (BlockStateParser without NBT); None if invalid.
pub(super) fn parse_state_string(registry: &BlockRegistry, text: &str) -> Option<BlockStateId> {
    let text = text.split('{').next().unwrap_or(text).trim();
    let (name, properties) = match text.split_once('[') {
        Some((name, rest)) => (name, rest.strip_suffix(']')?),
        None => (text, ""),
    };
    let name = if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    };
    let mut descriptor = BlockState {
        name,
        properties: Default::default(),
    };
    for pair in properties.split(',').filter(|p| !p.trim().is_empty()) {
        let (k, v) = pair.split_once('=')?;
        descriptor
            .properties
            .insert(k.trim().to_owned(), v.trim().to_owned());
    }
    registry.resolve(&descriptor).ok()
}

/// `StructureTemplate.fillFromWorld`'s result written by `StructureTemplate.save`:
/// the blocks (positions relative to the box) sorted full blocks, then other
/// blocks, then block entities, each by y, x, z, the palette in first-use
/// order, the entities and the size, with the data version. The author is not
/// part of the file.
pub fn save_nbt(
    registry: &BlockRegistry,
    transforms: &StateTransforms,
    size: [i32; 3],
    blocks: Vec<BlockInfo>,
    entities: &[EntityInfo],
    data_version: i32,
) -> Tag {
    let mut lists: [Vec<BlockInfo>; 3] = Default::default();
    for block in blocks {
        let class = if block.nbt.is_some() {
            2
        } else if transforms.full_block(block.state) {
            0
        } else {
            1
        };
        lists[class].push(block);
    }
    let mut ordered = Vec::new();
    for mut list in lists {
        list.sort_by_key(|b| (b.pos[1], b.pos[0], b.pos[2]));
        ordered.extend(list);
    }
    let int_list = |v: [i32; 3]| Tag::List(v.iter().map(|i| Tag::Int(*i)).collect());
    let mut palette: Vec<BlockStateId> = Vec::new();
    let mut block_list = Vec::with_capacity(ordered.len());
    for block in &ordered {
        let index = palette
            .iter()
            .position(|s| *s == block.state)
            .unwrap_or_else(|| {
                palette.push(block.state);
                palette.len() - 1
            });
        let mut tag = Compound::new();
        tag.insert("pos".into(), int_list(block.pos));
        tag.insert("state".into(), Tag::Int(index as i32));
        if let Some(nbt) = &block.nbt {
            tag.insert("nbt".into(), Tag::Compound((**nbt).clone()));
        }
        block_list.push(Tag::Compound(tag));
    }
    // NbtUtils.writeBlockState: `id` and every property as a string.
    let palette_list = palette
        .iter()
        .map(|id| {
            let mut tag = Compound::new();
            if let Some(state) = registry.state(*id) {
                tag.insert("id".into(), Tag::String(state.name.as_str().into()));
                if !state.properties.is_empty() {
                    let mut properties = Compound::new();
                    for (k, v) in &state.properties {
                        properties.insert(k.as_str().into(), Tag::String(v.as_str().into()));
                    }
                    tag.insert("properties".into(), Tag::Compound(properties));
                }
            }
            Tag::Compound(tag)
        })
        .collect();
    let entity_list = entities
        .iter()
        .map(|e| {
            let mut tag = Compound::new();
            tag.insert(
                "pos".into(),
                Tag::List(e.pos.iter().map(|v| Tag::Double(v.to_bits())).collect()),
            );
            tag.insert("blockPos".into(), int_list(e.block_pos));
            tag.insert("nbt".into(), Tag::Compound(e.nbt.clone()));
            Tag::Compound(tag)
        })
        .collect();
    let mut root = Compound::new();
    root.insert("blocks".into(), Tag::List(block_list));
    root.insert("palette".into(), Tag::List(palette_list));
    root.insert("entities".into(), Tag::List(entity_list));
    root.insert("size".into(), int_list(size));
    root.insert("DataVersion".into(), Tag::Int(data_version));
    Tag::Compound(root)
}

impl StructureTemplate {
    /// A new, never-loaded template: zero size and no palettes.
    pub fn empty() -> Self {
        Self {
            location: "".into(),
            size: [0; 3],
            palettes: Vec::new(),
            entities: Vec::new(),
        }
    }
    /// StructureTemplate.load. Blocks are ordered as vanilla orders them:
    /// full collision blocks, then other blocks, then block entities, each
    /// sorted by Y, X, Z.
    pub fn from_nbt(tag: &Tag, registry: &BlockRegistry, transforms: &StateTransforms) -> Self {
        let empty = Compound::new();
        let root = match tag {
            Tag::Compound(c) => c,
            _ => &empty,
        };
        let size = int_list(get(root, "size"));
        let blocks: Vec<RawBlock> = match get(root, "blocks") {
            Some(Tag::List(list)) => list.iter().filter_map(raw_block).collect(),
            _ => Vec::new(),
        };
        let mut palettes = Vec::new();
        match get(root, "palettes") {
            Some(Tag::List(lists)) => {
                for list in lists {
                    let states: &[Tag] = match list {
                        Tag::List(p) => p,
                        _ => &[],
                    };
                    palettes.push(load_palette(states, &blocks, registry, transforms));
                }
            }
            _ => {
                let states: &[Tag] = match get(root, "palette") {
                    Some(Tag::List(p)) => p,
                    _ => &[],
                };
                palettes.push(load_palette(states, &blocks, registry, transforms));
            }
        }
        let entities = match get(root, "entities") {
            Some(Tag::List(list)) => entities_of(list),
            _ => Vec::new(),
        };
        Self {
            location: "".into(),
            size,
            palettes,
            entities,
        }
    }
    /// `StructureTemplate.load` without the `Tag` tree: the decompressed
    /// file is read key by key ([`nbt::Cursor`]) and `blocks` streams straight
    /// into [`RawBlock`]s, one pass over them serving every palette. Bytes the
    /// tree read would refuse are an error here too, so callers fall back to
    /// [`empty`](Self::empty) in the same cases.
    pub fn from_bytes(
        bytes: &[u8],
        limits: nbt::Limits,
        registry: &BlockRegistry,
        transforms: &StateTransforms,
    ) -> Result<Self, nbt::Error> {
        let Some(mut cursor) = nbt::Cursor::file(bytes, limits)? else {
            // Not a compound: the tree read sees an empty root compound.
            return Ok(Self::from_nbt(&Tag::End, registry, transforms));
        };
        let mut size = [0; 3];
        let mut blocks = Vec::new();
        let mut palettes: Option<Vec<Vec<Tag>>> = None;
        let mut palette: Option<Vec<Tag>> = None;
        let mut entities = Vec::new();
        // A later key of the same name replaces an earlier one, as the tree's
        // compound map keeps only the last; a `palettes` that is not a list
        // falls back to `palette` in both readers.
        while let Some((key, id)) = cursor.next_key()? {
            if key.is("size") {
                size = int_list_stream(&mut cursor, id, 1)?;
            } else if key.is("blocks") {
                blocks = blocks_stream(&mut cursor, id, 1)?;
            } else if key.is("palettes") {
                palettes = list_stream(&mut cursor, id, 1)?.map(|lists| {
                    lists
                        .into_iter()
                        .map(|list| match list {
                            Tag::List(entries) => entries,
                            _ => Vec::new(),
                        })
                        .collect()
                });
            } else if key.is("palette") {
                palette = list_stream(&mut cursor, id, 1)?;
            } else if key.is("entities") {
                entities =
                    list_stream(&mut cursor, id, 1)?.map_or_else(Vec::new, |l| entities_of(&l));
            } else {
                // `DataVersion`, `author`, ...: read (and checked), then dropped.
                cursor.tag(id, 1)?;
            }
        }
        let mut palettes_out = Vec::new();
        match palettes {
            Some(lists) => {
                for states in &lists {
                    palettes_out.push(load_palette(states, &blocks, registry, transforms));
                }
            }
            None => {
                let states = palette.unwrap_or_default();
                palettes_out.push(load_palette(&states, &blocks, registry, transforms));
            }
        }
        Ok(Self {
            location: "".into(),
            size,
            palettes: palettes_out,
            entities,
        })
    }
    /// StructureTemplate.transform(Vec3, ...): entity positions.
    pub fn transform_vec(
        pos: [f64; 3],
        mirror: Mirror,
        rotation: Rotation,
        pivot: [i32; 3],
    ) -> [f64; 3] {
        let [mut x, y, mut z] = pos;
        match mirror {
            Mirror::LeftRight => z = 1.0 - z,
            Mirror::FrontBack => x = 1.0 - x,
            Mirror::None => {}
        }
        let (px, pz) = (f64::from(pivot[0]), f64::from(pivot[2]));
        match rotation {
            Rotation::Counterclockwise90 => [px - pz + z, y, px + pz + 1.0 - x],
            Rotation::Clockwise90 => [px + pz + 1.0 - z, y, pz - px + x],
            Rotation::Clockwise180 => [px + px + 1.0 - x, y, pz + pz + 1.0 - z],
            Rotation::None => [x, y, z],
        }
    }
    pub fn palettes(&self) -> &[Palette] {
        &self.palettes
    }
    pub fn size(&self, rotation: Rotation) -> [i32; 3] {
        match rotation {
            Rotation::Clockwise90 | Rotation::Counterclockwise90 => {
                [self.size[2], self.size[1], self.size[0]]
            }
            _ => self.size,
        }
    }
    pub fn transform(
        pos: [i32; 3],
        mirror: Mirror,
        rotation: Rotation,
        pivot: [i32; 3],
    ) -> [i32; 3] {
        let [mut x, y, mut z] = pos;
        match mirror {
            Mirror::LeftRight => z = z.wrapping_neg(),
            Mirror::FrontBack => x = x.wrapping_neg(),
            Mirror::None => {}
        }
        let (px, pz) = (pivot[0], pivot[2]);
        match rotation {
            Rotation::Counterclockwise90 => [px - pz + z, y, px + pz - x],
            Rotation::Clockwise90 => [px + pz - z, y, pz - px + x],
            Rotation::Clockwise180 => [px + px - x, y, pz + pz - z],
            Rotation::None => [x, y, z],
        }
    }
    pub fn zero_position_with_transform(
        zero: [i32; 3],
        mirror: Mirror,
        rotation: Rotation,
        size_x: i32,
        size_z: i32,
    ) -> [i32; 3] {
        let (sx, sz) = (size_x - 1, size_z - 1);
        let mx = if mirror == Mirror::FrontBack { sx } else { 0 };
        let mz = if mirror == Mirror::LeftRight { sz } else { 0 };
        let d = match rotation {
            Rotation::Counterclockwise90 => [mz, 0, sx - mx],
            Rotation::Clockwise90 => [sz - mz, 0, mx],
            Rotation::Clockwise180 => [sx - mx, 0, sz - mz],
            Rotation::None => [mx, 0, mz],
        };
        [zero[0] + d[0], zero[1], zero[2] + d[2]]
    }
    pub fn bounding_box(
        &self,
        position: [i32; 3],
        rotation: Rotation,
        pivot: [i32; 3],
        mirror: Mirror,
    ) -> BoundingBox {
        let delta = [self.size[0] - 1, self.size[1] - 1, self.size[2] - 1];
        let a = Self::transform([0; 3], mirror, rotation, pivot);
        let b = Self::transform(delta, mirror, rotation, pivot);
        BoundingBox::from_corners(a, b).moved(position)
    }
    /// The palette selected by StructurePlaceSettings.getRandomPalette.
    pub fn palette_index(&self, random: &mut impl horizon_core::random::RandomSource) -> usize {
        random
            .next_int_bound(self.palettes.len().max(1) as i32)
            .unwrap_or(0) as usize
    }
    /// Jigsaw blocks of a palette, parsed from their block-entity data.
    pub fn jigsaws(
        &self,
        palette: usize,
        jigsaw_states: &dyn Fn(BlockStateId) -> Option<bool>,
    ) -> &[JigsawInfo] {
        let Some(palette) = self.palettes.get(palette) else {
            return &[];
        };
        palette.jigsaws.get_or_init(|| {
            palette
                .blocks
                .iter()
                .filter_map(|info| {
                    let horizontal = jigsaw_states(info.state)?;
                    let nbt = info.nbt.clone().unwrap_or_default();
                    let joint = string(get(&nbt, "joint"));
                    let text = |key: &str, default: &str| {
                        string(get(&nbt, key)).unwrap_or_else(|| default.to_owned())
                    };
                    let int = |key: &str| match get(&nbt, key) {
                        Some(Tag::Int(v)) => *v,
                        _ => 0,
                    };
                    Some(JigsawInfo {
                        rollable: match joint.as_deref() {
                            Some("rollable") => true,
                            Some("aligned") => false,
                            _ => !horizontal,
                        },
                        name: text("name", "minecraft:empty"),
                        pool: text("pool", "minecraft:empty"),
                        target: text("target", "minecraft:empty"),
                        placement_priority: int("placement_priority"),
                        selection_priority: int("selection_priority"),
                        info: info.clone(),
                    })
                })
                .collect()
        })
    }
}

/// [`int_list`] out of the stream: a list's first three numeric elements.
fn int_list_stream(cursor: &mut nbt::Cursor, id: u8, depth: usize) -> Result<[i32; 3], nbt::Error> {
    let mut out = [0; 3];
    if id != 9 {
        cursor.tag(id, depth)?;
        return Ok(out);
    }
    let (element_id, len) = cursor.list(depth)?;
    for slot in 0..len {
        let value = cursor.element(element_id, depth + 1)?;
        if let Some(slot) = out.get_mut(slot) {
            *slot = int_value(Some(&value));
        }
    }
    Ok(out)
}

/// The `blocks` list: [`raw_block`] per element, without the tree.
fn blocks_stream(
    cursor: &mut nbt::Cursor,
    id: u8,
    depth: usize,
) -> Result<Vec<RawBlock>, nbt::Error> {
    if id != 9 {
        cursor.tag(id, depth)?;
        return Ok(Vec::new());
    }
    let (element_id, len) = cursor.list(depth)?;
    let mut blocks = Vec::new();
    for _ in 0..len {
        // Only a compound can be a block; a mixed list's other members are
        // read and dropped, as `from_nbt` skips them.
        if element_id == 10 {
            if let Some(block) = block_stream(cursor, depth + 1)? {
                blocks.push(block);
            }
        } else {
            cursor.element(element_id, depth + 1)?;
        }
    }
    Ok(blocks)
}

/// One element of `blocks`, read field by field; a later field of the same
/// name wins, as the compound map keeps only the last. A compound whose only
/// (distinct) member is empty-named is the member itself, as the list read
/// unwraps it.
fn block_stream(cursor: &mut nbt::Cursor, depth: usize) -> Result<Option<RawBlock>, nbt::Error> {
    cursor.compound(depth)?;
    let mut block = RawBlock {
        pos: [0; 3],
        state: 0,
        nbt: None,
    };
    let mut only_empty = true;
    let mut wrapped = None;
    while let Some((key, id)) = cursor.next_key()? {
        if key.is_empty() {
            wrapped = Some(cursor.tag(id, depth + 1)?);
        } else {
            only_empty = false;
            if key.is("pos") {
                block.pos = int_list_stream(cursor, id, depth + 1)?;
            } else if key.is("state") {
                block.state = int_value(Some(&cursor.tag(id, depth + 1)?));
            } else if key.is("nbt") {
                block.nbt = match cursor.tag(id, depth + 1)? {
                    Tag::Compound(c) => Some(c),
                    _ => None,
                };
            } else {
                cursor.tag(id, depth + 1)?;
            }
        }
    }
    match wrapped {
        Some(tag) if only_empty => Ok(raw_block(&tag)),
        _ => Ok(Some(block)),
    }
}

/// A list's elements as tags (`None` when the value is not a list): the
/// `palettes`, `palette` and `entities` keys, which are small.
fn list_stream(
    cursor: &mut nbt::Cursor,
    id: u8,
    depth: usize,
) -> Result<Option<Vec<Tag>>, nbt::Error> {
    if id != 9 {
        cursor.tag(id, depth)?;
        return Ok(None);
    }
    let (element_id, len) = cursor.list(depth)?;
    let mut elements = Vec::new();
    for _ in 0..len {
        elements.push(cursor.element(element_id, depth + 1)?);
    }
    Ok(Some(elements))
}

#[cfg(test)]
mod save_tests {
    use super::*;
    use horizon_core::version::DATA_VERSION;

    fn registry() -> BlockRegistry {
        let state = |name: &str| Tag::compound([("id", Tag::String(name.into()))]);
        let tag = Tag::compound([
            ("DataVersion", Tag::Int(DATA_VERSION)),
            (
                "states",
                Tag::List(vec![
                    state("minecraft:air"),
                    state("minecraft:stone"),
                    state("minecraft:torch"),
                ]),
            ),
            (
                "defaults",
                Tag::compound([
                    ("minecraft:air", Tag::Int(0)),
                    ("minecraft:stone", Tag::Int(1)),
                    ("minecraft:torch", Tag::Int(2)),
                ]),
            ),
        ]);
        BlockRegistry::from_nbt(&tag).unwrap()
    }

    #[test]
    fn a_data_pack_template_is_found_and_others_stay_empty() {
        let registry: &'static BlockRegistry = Box::leak(Box::new(registry()));
        let transforms = transforms();
        let tag = save_nbt(
            registry,
            &transforms,
            [3, 1, 2],
            vec![BlockInfo {
                pos: [0, 0, 0],
                state: BlockStateId(1),
                nbt: None,
            }],
            &[],
            DATA_VERSION,
        );
        let named = crate::nbt::NamedTag {
            name: "".into(),
            tag,
        };
        let raw = crate::nbt::write_named(&named, crate::nbt::Limits::templates()).unwrap();
        let file = crate::compression::Compression::Gzip.encode(&raw).unwrap();
        let resources = super::super::StructureResources::new(
            registry,
            transforms,
            std::path::Path::new("/nonexistent"),
            Default::default(),
            &Default::default(),
        )
        .with_pack_templates([("p:room/a".to_owned(), file.into())].into());
        assert_eq!(resources.template("p:room/a").unwrap().size, [3, 1, 2]);
        assert_eq!(resources.template("p:room/b").unwrap().size, [0, 0, 0]);
    }

    #[test]
    fn save_orders_like_fill_from_world_and_reads_back() {
        let registry = registry();
        let transforms = transforms();
        let block = |pos: [i32; 3], state: u32, nbt: bool| BlockInfo {
            pos,
            state: BlockStateId(state),
            nbt: nbt.then(|| {
                let mut c = Compound::new();
                c.insert("id".into(), Tag::String("minecraft:chest".into()));
                Box::new(c)
            }),
        };
        let blocks = vec![
            block([0, 1, 0], 2, false),
            block([1, 0, 0], 1, false),
            block([0, 0, 0], 1, false),
            block([0, 0, 1], 2, true),
        ];
        let tag = save_nbt(&registry, &transforms, [2, 2, 2], blocks, &[], DATA_VERSION);
        assert_eq!(tag.get("DataVersion"), Some(&Tag::Int(DATA_VERSION)));
        assert_eq!(
            tag.get("size"),
            Some(&Tag::List(vec![Tag::Int(2), Tag::Int(2), Tag::Int(2)]))
        );
        let Some(Tag::List(palette)) = tag.get("palette") else {
            panic!("palette")
        };
        // First use order: the stone full blocks come first.
        assert_eq!(palette.len(), 2);
        assert_eq!(
            palette[0].get("id"),
            Some(&Tag::String("minecraft:stone".into()))
        );
        let read = StructureTemplate::from_nbt(&tag, &registry, &transforms);
        assert_eq!(read.size, [2, 2, 2]);
        let blocks = read.palettes()[0].blocks();
        let order: Vec<_> = blocks.iter().map(|b| b.pos).collect();
        // Full blocks by y, x, z; then other blocks; block entities last.
        assert_eq!(order, [[0, 0, 0], [1, 0, 0], [0, 1, 0], [0, 0, 1]]);
        assert!(blocks[3].nbt.is_some());
    }

    fn transforms() -> StateTransforms {
        StateTransforms::from_json(
            r#"{"rotate":[[0,0,0],[1,1,1],[2,2,2]],"mirror":[[0,0],[1,1],[2,2]],
                "full_block":[false,true,false],"loot_containers":[],"block_entities":{}}"#,
            3,
        )
        .unwrap()
    }

    /// The same template out of the `Tag` tree and out of the file bytes,
    /// compared field by field: size, every palette's blocks (position,
    /// state, block-entity data), the entities, and the jigsaw data derived
    /// from the block entities.
    fn both_ways(tag: &Tag) -> (StructureTemplate, StructureTemplate) {
        let registry: &'static BlockRegistry = Box::leak(Box::new(registry()));
        let transforms = transforms();
        let named = crate::nbt::NamedTag {
            name: "".into(),
            tag: tag.clone(),
        };
        let raw = crate::nbt::write_named(&named, crate::nbt::Limits::templates()).unwrap();
        let limits = crate::nbt::Limits::templates();
        let tree = StructureTemplate::from_nbt(tag, registry, &transforms);
        let streamed = StructureTemplate::from_bytes(&raw, limits, registry, &transforms).unwrap();
        assert_eq!(format!("{tree:?}"), format!("{streamed:?}"));
        for palette in 0..tree.palettes().len() {
            assert_eq!(
                format!("{:?}", tree.jigsaws(palette, &|_| Some(true))),
                format!("{:?}", streamed.jigsaws(palette, &|_| Some(true)))
            );
        }
        (tree, streamed)
    }

    /// A template exercising both palettes (`palettes`), block-entity data, a
    /// state index past the palette, a mixed list's non-compound member and an
    /// entity, as vanilla's `StructureTemplate.save` cannot write all of them.
    fn hand_built() -> Tag {
        let entry = |id: &str, properties: &[(&str, &str)]| {
            let mut palette = Compound::new();
            palette.insert("id".into(), Tag::String(id.into()));
            if !properties.is_empty() {
                let mut map = Compound::new();
                for (key, value) in properties {
                    map.insert((*key).into(), Tag::String((*value).into()));
                }
                palette.insert("properties".into(), Tag::Compound(map));
            }
            Tag::Compound(palette)
        };
        let chest = || {
            let mut nbt = Compound::new();
            nbt.insert("id".into(), Tag::String("minecraft:chest".into()));
            nbt.insert(
                "LootTable".into(),
                Tag::String("minecraft:chests/simple".into()),
            );
            Tag::Compound(nbt)
        };
        let block = |pos: [i32; 3], state: i32, nbt: Option<Tag>| {
            let mut block = Compound::new();
            block.insert(
                "pos".into(),
                Tag::List(pos.iter().map(|v| Tag::Int(*v)).collect()),
            );
            block.insert("state".into(), Tag::Int(state));
            if let Some(nbt) = nbt {
                block.insert("nbt".into(), nbt);
            }
            Tag::Compound(block)
        };
        let entity = Tag::compound([
            (
                "pos",
                Tag::List(vec![
                    Tag::Double(0.5f64.to_bits()),
                    Tag::Double(1.5f64.to_bits()),
                    Tag::Double(2.5f64.to_bits()),
                ]),
            ),
            (
                "blockPos",
                Tag::List(vec![Tag::Int(0), Tag::Int(1), Tag::Int(2)]),
            ),
            ("nbt", chest()),
        ]);
        Tag::compound([
            ("DataVersion", Tag::Int(DATA_VERSION)),
            (
                "size",
                Tag::List(vec![Tag::Int(3), Tag::Int(2), Tag::Int(4)]),
            ),
            (
                "palettes",
                Tag::List(vec![
                    Tag::List(vec![
                        entry("minecraft:stone", &[]),
                        entry("minecraft:oak_stairs", &[("facing", "east")]),
                    ]),
                    Tag::List(vec![entry("minecraft:oak_stairs", &[("facing", "west")])]),
                ]),
            ),
            (
                "blocks",
                Tag::List(vec![
                    block([2, 1, 3], 1, Some(chest())),
                    block([0, 0, 0], 0, None),
                    block([1, 0, 2], 1, None),
                    // Past the palette: air; no `state` or a non-`Int` one:
                    // palette entry 0. A mixed list's other member: written
                    // wrapped, read and skipped by both.
                    block([2, 0, 1], 9, None),
                    Tag::compound([(
                        "pos",
                        Tag::List(vec![Tag::Int(0), Tag::Int(1), Tag::Int(0)]),
                    )]),
                    Tag::compound([
                        (
                            "pos",
                            Tag::List(vec![Tag::Int(0), Tag::Int(1), Tag::Int(1)]),
                        ),
                        ("state", Tag::String("minecraft:stone".into())),
                    ]),
                    Tag::Int(7),
                ]),
            ),
            ("entities", Tag::List(vec![entity])),
        ])
    }

    #[test]
    fn a_hand_built_template_streams_out_of_its_bytes_to_the_same_fields() {
        let (tree, streamed) = both_ways(&hand_built());
        assert_eq!(streamed.size, [3, 2, 4]);
        assert_eq!(streamed.palettes().len(), 2);
        assert_eq!(streamed.palettes()[0].blocks().len(), 6);
        assert_eq!(streamed.palettes()[1].blocks().len(), 6);
        assert_eq!(streamed.entities.len(), 1);
        // Full blocks (those without a state too) first, the block entity last.
        assert_eq!(tree.palettes()[0].blocks()[5].pos, [2, 1, 3]);
        assert!(tree.palettes()[0].blocks()[5].nbt.is_some());
        assert_eq!(streamed.palettes()[0].blocks()[5].pos, [2, 1, 3]);
    }

    #[test]
    fn single_and_missing_palettes_stream_like_the_tree() {
        let block = Tag::compound([
            (
                "pos",
                Tag::List(vec![Tag::Int(1), Tag::Int(0), Tag::Int(0)]),
            ),
            ("state", Tag::Int(0)),
        ]);
        let single = Tag::compound([
            (
                "size",
                Tag::List(vec![Tag::Int(2), Tag::Int(1), Tag::Int(1)]),
            ),
            (
                "palette",
                Tag::List(vec![Tag::compound([(
                    "id",
                    Tag::String("minecraft:stone".into()),
                )])]),
            ),
            ("blocks", Tag::List(vec![block.clone()])),
        ]);
        let (_, streamed) = both_ways(&single);
        assert_eq!(streamed.size, [2, 1, 1]);
        assert_eq!(streamed.palettes()[0].blocks()[0].state, BlockStateId(1));
        // No palettes and no size: one palette of air, size zero; a root that
        // is not a compound is the empty root compound, so the same.
        let bare = Tag::compound([("blocks", Tag::List(vec![block]))]);
        let (_, streamed) = both_ways(&bare);
        assert_eq!(streamed.size, [0, 0, 0]);
        assert_eq!(streamed.palettes().len(), 1);
        assert_eq!(streamed.palettes()[0].blocks().len(), 1);
        let (tree, streamed) = both_ways(&Tag::Int(5));
        assert_eq!(format!("{tree:?}"), format!("{streamed:?}"));
        assert_eq!(streamed.palettes().len(), 1);
    }

    /// A streamed read must refuse exactly the bytes a tree read refuses: a
    /// truncated or altered file is accepted by one decoder iff by the other.
    #[test]
    fn streamed_and_tree_reads_refuse_the_same_bytes() {
        let registry: &'static BlockRegistry = Box::leak(Box::new(registry()));
        let transforms = transforms();
        let mut nbt = Compound::new();
        nbt.insert("id".into(), Tag::String("minecraft:chest".into()));
        let block = Tag::compound([
            (
                "pos",
                Tag::List(vec![Tag::Int(1), Tag::Int(2), Tag::Int(3)]),
            ),
            ("state", Tag::Int(1)),
            ("nbt", Tag::Compound(nbt)),
        ]);
        let tag = Tag::compound([
            (
                "size",
                Tag::List(vec![Tag::Int(4), Tag::Int(1), Tag::Int(4)]),
            ),
            (
                "palette",
                Tag::List(vec![Tag::compound([(
                    "id",
                    Tag::String("minecraft:stone".into()),
                )])]),
            ),
            ("blocks", Tag::List(vec![block])),
            (
                "entities",
                Tag::List(vec![Tag::compound([(
                    "pos",
                    Tag::List(vec![Tag::Double(0.5f64.to_bits())]),
                )])]),
            ),
        ]);
        let limits = crate::nbt::Limits::templates();
        let mut checked = 0;
        for tag in [tag, hand_built()] {
            let named = crate::nbt::NamedTag {
                name: "".into(),
                tag,
            };
            let raw = crate::nbt::write_named(&named, limits).unwrap();
            let mut cases: Vec<Vec<u8>> = (0..raw.len()).map(|end| raw[..end].to_vec()).collect();
            for i in 0..raw.len() {
                let mut altered = raw.clone();
                altered[i] ^= 0x80;
                cases.push(altered);
            }
            for bytes in cases {
                let tree = crate::nbt::read_named(&bytes, limits);
                let streamed = StructureTemplate::from_bytes(&bytes, limits, registry, &transforms);
                assert_eq!(tree.is_ok(), streamed.is_ok(), "{} bytes", bytes.len());
                if let (Ok((tree, _)), Ok(streamed)) = (tree, streamed) {
                    let tree = StructureTemplate::from_nbt(&tree.tag, registry, &transforms);
                    assert_eq!(format!("{tree:?}"), format!("{streamed:?}"));
                }
                checked += 1;
            }
        }
        assert!(checked > 400);
    }

    #[test]
    fn streamed_limits_and_duplicate_encoded_keys_match_the_tree() {
        let registry = registry();
        let transforms = transforms();
        // Two spellings of "size", the second using Java modified UTF-8's
        // accepted overlong encoding. The last decoded key must win.
        let bytes = [
            10, 0, 0, 9, 0, 4, b's', b'i', b'z', b'e', 3, 0, 0, 0, 1, 0, 0, 0, 7, 9, 0, 5, 0xc1,
            0xb3, b'i', b'z', b'e', 3, 0, 0, 0, 1, 0, 0, 0, 9, 0,
        ];
        for raw in [&[0][..], &bytes[..]] {
            for max_nodes in 0..8 {
                for max_depth in 0..4 {
                    for max_bytes in 0..=raw.len() {
                        let limits = nbt::Limits {
                            max_nodes,
                            max_depth,
                            max_bytes,
                        };
                        let tree = nbt::read_named(raw, limits);
                        let streamed =
                            StructureTemplate::from_bytes(raw, limits, &registry, &transforms);
                        assert_eq!(tree.is_ok(), streamed.is_ok(), "{limits:?}");
                        if let (Ok((tree, _)), Ok(streamed)) = (tree, streamed) {
                            let tree =
                                StructureTemplate::from_nbt(&tree.tag, &registry, &transforms);
                            assert_eq!(format!("{tree:?}"), format!("{streamed:?}"));
                        }
                    }
                }
            }
        }
        assert_eq!(
            StructureTemplate::from_bytes(&bytes, nbt::Limits::templates(), &registry, &transforms)
                .unwrap()
                .size,
            [9, 0, 0]
        );
    }
}
