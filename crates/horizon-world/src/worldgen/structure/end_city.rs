//! End cities: EndCityStructure piece assembly (EndCityPieces section
//! generators) and EndCityPiece data markers.
use super::{
    BoundingBox, Mirror, Rotation, StructureResources, StructureTemplate,
    legacy::{LegacyPiece, PieceContext, Result, TemplateHandler, TemplatePiece, offset},
    processor::ProcessorList,
};
use crate::nbt::{Compound, Tag};
use horizon_core::{geometry::Direction, random::RandomSource};
use std::sync::Arc;

const MAX_GEN_DEPTH: i32 = 8;
const TOWER_BRIDGES: [(Rotation, [i32; 3]); 4] = [
    (Rotation::None, [1, -1, 0]),
    (Rotation::Clockwise90, [6, -1, 1]),
    (Rotation::Counterclockwise90, [0, -1, 5]),
    (Rotation::Clockwise180, [5, -1, 6]),
];
const FAT_TOWER_BRIDGES: [(Rotation, [i32; 3]); 4] = [
    (Rotation::None, [4, -1, 0]),
    (Rotation::Clockwise90, [12, -1, 4]),
    (Rotation::Counterclockwise90, [0, -1, 8]),
    (Rotation::Clockwise180, [8, -1, 12]),
];

/// A piece during assembly: template, position, settings and genDepth.
#[derive(Clone)]
struct Draft {
    name: &'static str,
    template: Arc<StructureTemplate>,
    position: [i32; 3],
    rotation: Rotation,
    overwrite: bool,
    depth: i32,
    bounding_box: BoundingBox,
}
#[derive(Clone, Copy)]
enum Section {
    HouseTower,
    Tower,
    TowerBridge,
    FatTower,
}
struct Assembly<'a, R> {
    resources: &'a StructureResources,
    random: &'a mut R,
    /// TOWER_BRIDGE_GENERATOR.shipCreated.
    ship: bool,
}

fn draft(
    resources: &StructureResources,
    name: &'static str,
    position: [i32; 3],
    rotation: Rotation,
    overwrite: bool,
) -> Option<Draft> {
    let template = resources.template(&format!("minecraft:end_city/{name}"))?;
    Some(Draft {
        name,
        bounding_box: template.bounding_box(position, rotation, [0; 3], Mirror::None),
        template,
        position,
        rotation,
        overwrite,
        depth: 0,
    })
}

impl<R: RandomSource> Assembly<'_, R> {
    fn int(&mut self, bound: i32) -> i32 {
        self.random.next_int_bound(bound).unwrap_or(0)
    }
    /// EndCityPieces.addPiece: the child is connected to the parent by
    /// StructureTemplate.calculateConnectedPosition(offset, ZERO).
    fn add(
        &self,
        parent: &Draft,
        offset: [i32; 3],
        name: &'static str,
        rotation: Rotation,
        overwrite: bool,
    ) -> Option<Draft> {
        let mut child = draft(self.resources, name, parent.position, rotation, overwrite)?;
        let a = StructureTemplate::transform(offset, Mirror::None, parent.rotation, [0; 3]);
        let b = StructureTemplate::transform([0; 3], Mirror::None, rotation, [0; 3]);
        let d: [i32; 3] = std::array::from_fn(|i| a[i] - b[i]);
        child.position = std::array::from_fn(|i| child.position[i] + d[i]);
        child.bounding_box = child.bounding_box.moved(d);
        Some(child)
    }
    fn push(
        &self,
        pieces: &mut Vec<Draft>,
        parent: &Draft,
        offset: [i32; 3],
        name: &'static str,
        rotation: Rotation,
        overwrite: bool,
    ) -> Option<Draft> {
        let child = self.add(parent, offset, name, rotation, overwrite)?;
        pieces.push(child.clone());
        Some(child)
    }
    /// EndCityPieces.recursiveChildren.
    fn children(
        &mut self,
        section: Section,
        depth: i32,
        parent: &Draft,
        offset: [i32; 3],
        pieces: &mut Vec<Draft>,
    ) -> Option<bool> {
        if depth > MAX_GEN_DEPTH {
            return Some(false);
        }
        let mut children = Vec::new();
        if self.generate(section, depth, parent, offset, &mut children)? {
            let tag = self.random.next_int();
            let mut collision = false;
            for child in &mut children {
                child.depth = tag;
                if let Some(other) = pieces
                    .iter()
                    .find(|p| p.bounding_box.intersects(&child.bounding_box))
                    && other.depth != parent.depth
                {
                    collision = true;
                    break;
                }
            }
            if !collision {
                pieces.extend(children);
                return Some(true);
            }
        }
        Some(false)
    }
    fn generate(
        &mut self,
        section: Section,
        depth: i32,
        parent: &Draft,
        offset: [i32; 3],
        pieces: &mut Vec<Draft>,
    ) -> Option<bool> {
        let rotation = parent.rotation;
        match section {
            Section::HouseTower => {
                if depth > MAX_GEN_DEPTH {
                    return Some(false);
                }
                let mut last = self.push(pieces, parent, offset, "base_floor", rotation, true)?;
                match self.int(3) {
                    0 => {
                        self.push(pieces, &last, [-1, 4, -1], "base_roof", rotation, true)?;
                    }
                    1 => {
                        last = self.push(
                            pieces,
                            &last,
                            [-1, 0, -1],
                            "second_floor_2",
                            rotation,
                            false,
                        )?;
                        last =
                            self.push(pieces, &last, [-1, 8, -1], "second_roof", rotation, false)?;
                        self.children(Section::Tower, depth + 1, &last, [0; 3], pieces)?;
                    }
                    _ => {
                        last = self.push(
                            pieces,
                            &last,
                            [-1, 0, -1],
                            "second_floor_2",
                            rotation,
                            false,
                        )?;
                        last = self.push(
                            pieces,
                            &last,
                            [-1, 4, -1],
                            "third_floor_2",
                            rotation,
                            false,
                        )?;
                        last =
                            self.push(pieces, &last, [-1, 8, -1], "third_roof", rotation, true)?;
                        self.children(Section::Tower, depth + 1, &last, [0; 3], pieces)?;
                    }
                }
                Some(true)
            }
            Section::Tower => {
                let x = 3 + self.int(2);
                let z = 3 + self.int(2);
                let mut last =
                    self.push(pieces, parent, [x, -3, z], "tower_base", rotation, true)?;
                last = self.push(pieces, &last, [0, 7, 0], "tower_piece", rotation, true)?;
                let mut bridge = (self.int(3) == 0).then(|| last.clone());
                let height = 1 + self.int(3);
                for i in 0..height {
                    last = self.push(pieces, &last, [0, 4, 0], "tower_piece", rotation, true)?;
                    if i < height - 1 && self.random.next_boolean() {
                        bridge = Some(last.clone());
                    }
                }
                if let Some(bridge) = bridge {
                    for (turn, offset) in TOWER_BRIDGES {
                        if self.random.next_boolean() {
                            let start = self.push(
                                pieces,
                                &bridge,
                                offset,
                                "bridge_end",
                                rotation.get_rotated(turn),
                                true,
                            )?;
                            self.children(Section::TowerBridge, depth + 1, &start, [0; 3], pieces)?;
                        }
                    }
                } else if depth != 7 {
                    return self.children(Section::FatTower, depth + 1, &last, [0; 3], pieces);
                }
                self.push(pieces, &last, [-1, 4, -1], "tower_top", rotation, true)?;
                Some(true)
            }
            Section::TowerBridge => {
                let length = self.int(4) + 1;
                let mut last =
                    self.push(pieces, parent, [0, 0, -4], "bridge_piece", rotation, true)?;
                if let Some(first) = pieces.last_mut() {
                    first.depth = -1;
                }
                last.depth = -1;
                let mut next_y = 0;
                for _ in 0..length {
                    if self.random.next_boolean() {
                        last = self.push(
                            pieces,
                            &last,
                            [0, next_y, -4],
                            "bridge_piece",
                            rotation,
                            true,
                        )?;
                        next_y = 0;
                    } else {
                        last = if self.random.next_boolean() {
                            self.push(
                                pieces,
                                &last,
                                [0, next_y, -4],
                                "bridge_steep_stairs",
                                rotation,
                                true,
                            )?
                        } else {
                            self.push(
                                pieces,
                                &last,
                                [0, next_y, -8],
                                "bridge_gentle_stairs",
                                rotation,
                                true,
                            )?
                        };
                        next_y = 4;
                    }
                }
                if !self.ship && self.int(10 - depth) == 0 {
                    let x = -8 + self.int(8);
                    let z = -70 + self.int(10);
                    self.push(pieces, &last, [x, next_y, z], "ship", rotation, true)?;
                    self.ship = true;
                } else if !self.children(
                    Section::HouseTower,
                    depth + 1,
                    &last,
                    [-3, next_y + 1, -11],
                    pieces,
                )? {
                    return Some(false);
                }
                self.push(
                    pieces,
                    &last,
                    [4, next_y, 0],
                    "bridge_end",
                    rotation.get_rotated(Rotation::Clockwise180),
                    true,
                )?;
                if let Some(end) = pieces.last_mut() {
                    end.depth = -1;
                }
                Some(true)
            }
            Section::FatTower => {
                let mut last = self.push(
                    pieces,
                    parent,
                    [-3, 4, -3],
                    "fat_tower_base",
                    rotation,
                    true,
                )?;
                last = self.push(pieces, &last, [0, 4, 0], "fat_tower_middle", rotation, true)?;
                let mut i = 0;
                while i < 2 && self.int(3) != 0 {
                    last =
                        self.push(pieces, &last, [0, 8, 0], "fat_tower_middle", rotation, true)?;
                    for (turn, offset) in FAT_TOWER_BRIDGES {
                        if self.random.next_boolean() {
                            let start = self.push(
                                pieces,
                                &last,
                                offset,
                                "bridge_end",
                                rotation.get_rotated(turn),
                                true,
                            )?;
                            self.children(Section::TowerBridge, depth + 1, &start, [0; 3], pieces)?;
                        }
                    }
                    i += 1;
                }
                self.push(pieces, &last, [-2, 8, -2], "fat_tower_top", rotation, true)?;
                Some(true)
            }
        }
    }
}

/// EndCityPieces.startHouseTower.
pub fn pieces(
    resources: &StructureResources,
    origin: [i32; 3],
    rotation: Rotation,
    random: &mut impl RandomSource,
) -> Option<Vec<EndCityPiece>> {
    let mut a = Assembly {
        resources,
        random,
        ship: false,
    };
    let mut pieces = Vec::new();
    let mut last = draft(resources, "base_floor", origin, rotation, true)?;
    pieces.push(last.clone());
    last = a.push(
        &mut pieces,
        &last,
        [-1, 0, -1],
        "second_floor_1",
        rotation,
        false,
    )?;
    last = a.push(
        &mut pieces,
        &last,
        [-1, 4, -1],
        "third_floor_1",
        rotation,
        false,
    )?;
    last = a.push(
        &mut pieces,
        &last,
        [-1, 8, -1],
        "third_roof",
        rotation,
        true,
    )?;
    a.children(Section::Tower, 1, &last, [0; 3], &mut pieces)?;
    Some(
        pieces
            .into_iter()
            .map(|d| {
                let ignore = if d.overwrite {
                    vec!["minecraft:structure_block".into()]
                } else {
                    vec!["minecraft:structure_block".into(), "minecraft:air".into()]
                };
                EndCityPiece(
                    TemplatePiece::new(
                        "minecraft:ecp",
                        &format!("minecraft:end_city/{}", d.name),
                        d.template,
                        d.position,
                        d.rotation,
                        Mirror::None,
                        [0; 3],
                        Arc::new(ProcessorList::default()),
                        true,
                        ignore,
                        TemplateHandler::Plain,
                    ),
                    d.depth,
                )
            })
            .collect(),
    )
}

/// EndCityPieces.EndCityPiece.
#[derive(Debug)]
/// The template piece and its genDepth (the collision tag of its batch).
pub struct EndCityPiece(pub(super) TemplatePiece, pub(super) i32);
impl LegacyPiece for EndCityPiece {
    fn bounding_box(&self) -> BoundingBox {
        self.0.bounding_box
    }
    /// `EndCityPiece`: the template piece's tag, `OW` for the pieces that
    /// overwrite air (only structure blocks ignored).
    fn save(&self) -> Option<crate::nbt::Compound> {
        let mut tag = self.0.saved();
        tag.insert("GD".into(), crate::nbt::Tag::Int(self.1));
        let overwrite = self.0.ignore.len() == 1;
        tag.insert("OW".into(), crate::nbt::Tag::Byte(i8::from(overwrite)));
        Some(tag)
    }
    fn type_name(&self) -> &'static str {
        "minecraft:ecp"
    }
    fn post_process(&self, c: &mut PieceContext<'_>) -> Result<()> {
        let Some(markers) = self.0.place_at(c, self.0.position, &[])? else {
            return Ok(());
        };
        for (marker, pos) in markers {
            if marker.starts_with("Sentry") {
                c.world.entity(super::legacy::spawn_nbt(
                    "minecraft:shulker",
                    pos,
                    false,
                    false,
                ))?;
            } else if marker.starts_with("Elytra") {
                // An item frame on the marker, facing the rotated south.
                let facing = self.0.rotation.rotate_direction(Direction::South);
                let mut nbt = super::legacy::spawn_nbt("minecraft:item_frame", pos, false, false);
                nbt.insert("block_pos".into(), Tag::IntArray(pos.to_vec()));
                let data = match facing {
                    Direction::North => 2,
                    Direction::South => 3,
                    Direction::West => 4,
                    _ => 5,
                };
                nbt.insert("Facing".into(), Tag::Byte(data));
                let mut item = Compound::new();
                item.insert("id".into(), Tag::String("minecraft:elytra".into()));
                item.insert("count".into(), Tag::Int(1));
                nbt.insert("Item".into(), Tag::Compound(item));
                c.world.entity(nbt)?;
            } else if marker.starts_with("Chest") {
                let chest = offset(pos, Direction::Down, 1);
                if c.chunk_box.is_inside(chest) {
                    c.loot_table(
                        chest,
                        "minecraft:chest",
                        "minecraft:chests/end_city_treasure",
                    )?;
                }
            }
        }
        Ok(())
    }
}
