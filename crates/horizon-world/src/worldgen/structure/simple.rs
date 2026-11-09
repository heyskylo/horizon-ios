//! Small piece-based structures: buried treasure, igloo, swamp hut, shipwreck.
use super::{
    BoundingBox, Mirror, Rotation, StructureResources, StructureTemplate,
    legacy::{
        LegacyPiece, PieceContext, PieceFrame, Result, TemplateHandler, TemplatePiece,
        create_chest, make_bounding_box, offset, random_horizontal,
    },
    processor::ProcessorList,
};
use crate::heightmap::HeightmapType;
use horizon_core::{geometry::Direction, random::RandomSource};
use std::sync::{Arc, Mutex};

fn empty_processors() -> Arc<ProcessorList> {
    Arc::new(ProcessorList::default())
}

#[derive(Debug)]
pub struct BuriedTreasurePiece {
    pub position: [i32; 3],
    /// Where the chest went: the box vanilla saves once placed.
    pub found: Mutex<Option<[i32; 3]>>,
}
impl LegacyPiece for BuriedTreasurePiece {
    fn bounding_box(&self) -> BoundingBox {
        BoundingBox::new(self.position, self.position)
    }
    fn save(&self) -> Option<crate::nbt::Compound> {
        let at = self.found.lock().unwrap().unwrap_or(self.position);
        Some(super::legacy::piece_tag(
            "minecraft:btp",
            BoundingBox::new(at, at),
            None,
            0,
        ))
    }
    fn type_name(&self) -> &'static str {
        "minecraft:btp"
    }
    fn post_process(&self, c: &mut PieceContext<'_>) -> Result<()> {
        let [x, _, z] = self.position;
        let mut pos = [x, c.world.height(HeightmapType::OceanFloorWg, x, z)?, z];
        let min = c.world.min_y()?;
        while pos[1] > min {
            let current = c.world.block(pos)?;
            let below = c.world.block(offset(pos, Direction::Down, 1))?;
            if matches!(
                c.name(below),
                "minecraft:sandstone"
                    | "minecraft:stone"
                    | "minecraft:andesite"
                    | "minecraft:granite"
                    | "minecraft:diorite"
            ) {
                let soft = if !c.is_air(current) && !c.is_liquid(current) {
                    current
                } else {
                    c.state("sand")
                };
                for d in Direction::ALL {
                    let relative = offset(pos, d, 1);
                    let state = c.world.block(relative)?;
                    if c.is_air(state) || c.is_liquid(state) {
                        let under = c.world.block(offset(relative, Direction::Down, 1))?;
                        if (c.is_air(under) || c.is_liquid(under)) && d != Direction::Up {
                            c.world.set(relative, below, 3)?;
                        } else {
                            c.world.set(relative, soft, 3)?;
                        }
                    }
                }
                *self.found.lock().unwrap() = Some(pos);
                create_chest(c, pos, "minecraft:chests/buried_treasure", None)?;
                return Ok(());
            }
            pos[1] -= 1;
        }
        Ok(())
    }
}

pub const IGLOO_TOP: &str = "minecraft:igloo/top";
const IGLOO_MIDDLE: &str = "minecraft:igloo/middle";
const IGLOO_BOTTOM: &str = "minecraft:igloo/bottom";
pub(super) fn igloo_pivot(name: &str) -> [i32; 3] {
    match name {
        IGLOO_TOP => [3, 5, 5],
        IGLOO_MIDDLE => [1, 3, 1],
        _ => [3, 6, 7],
    }
}
fn igloo_offset(name: &str) -> [i32; 3] {
    match name {
        IGLOO_TOP => [0, 0, 0],
        IGLOO_MIDDLE => [2, -3, 4],
        _ => [0, -3, -2],
    }
}
/// IglooPieces.addPieces.
pub fn igloo_pieces(
    resources: &StructureResources,
    position: [i32; 3],
    rotation: Rotation,
    random: &mut impl RandomSource,
) -> Option<Vec<TemplatePiece>> {
    let piece = |name: &str, depth: i32| -> Option<TemplatePiece> {
        let o = igloo_offset(name);
        Some(TemplatePiece::new(
            "minecraft:iglu",
            name,
            resources.template(name)?,
            [
                position[0] + o[0],
                position[1] + o[1] - depth,
                position[2] + o[2],
            ],
            rotation,
            Mirror::None,
            igloo_pivot(name),
            empty_processors(),
            false,
            vec!["minecraft:structure_block".into()],
            TemplateHandler::Igloo,
        ))
    };
    let mut pieces = Vec::new();
    if random.next_double() < 0.5 {
        let depth = random.next_int_bound(8).unwrap_or(0) + 4;
        pieces.push(piece(IGLOO_BOTTOM, depth * 3)?);
        for i in 0..depth - 1 {
            pieces.push(piece(IGLOO_MIDDLE, i * 3)?);
        }
    }
    pieces.push(piece(IGLOO_TOP, 0)?);
    Some(pieces)
}

const SHIPWRECK_BEACHED: [&str; 11] = [
    "with_mast",
    "sideways_full",
    "sideways_fronthalf",
    "sideways_backhalf",
    "rightsideup_full",
    "rightsideup_fronthalf",
    "rightsideup_backhalf",
    "with_mast_degraded",
    "rightsideup_full_degraded",
    "rightsideup_fronthalf_degraded",
    "rightsideup_backhalf_degraded",
];
const SHIPWRECK_OCEAN: [&str; 20] = [
    "with_mast",
    "upsidedown_full",
    "upsidedown_fronthalf",
    "upsidedown_backhalf",
    "sideways_full",
    "sideways_fronthalf",
    "sideways_backhalf",
    "rightsideup_full",
    "rightsideup_fronthalf",
    "rightsideup_backhalf",
    "with_mast_degraded",
    "upsidedown_full_degraded",
    "upsidedown_fronthalf_degraded",
    "upsidedown_backhalf_degraded",
    "sideways_full_degraded",
    "sideways_fronthalf_degraded",
    "sideways_backhalf_degraded",
    "rightsideup_full_degraded",
    "rightsideup_fronthalf_degraded",
    "rightsideup_backhalf_degraded",
];
/// ShipwreckPieces.addRandomPiece.
pub fn shipwreck_piece(
    resources: &StructureResources,
    position: [i32; 3],
    rotation: Rotation,
    random: &mut impl RandomSource,
    beached: bool,
) -> Option<TemplatePiece> {
    let names: &[&str] = if beached {
        &SHIPWRECK_BEACHED
    } else {
        &SHIPWRECK_OCEAN
    };
    let name = format!(
        "minecraft:shipwreck/{}",
        names[random.next_int_bound(names.len() as i32).ok()? as usize]
    );
    Some(TemplatePiece::new(
        "minecraft:shipwreck",
        &name,
        resources.template(&name)?,
        position,
        rotation,
        Mirror::None,
        [4, 0, 15],
        empty_processors(),
        true,
        vec!["minecraft:structure_block".into(), "minecraft:air".into()],
        TemplateHandler::Shipwreck { beached },
    ))
}
pub fn shipwreck_too_big(template: &StructureTemplate) -> bool {
    template.size[0] > 32 || template.size[1] > 32
}

impl TemplatePiece {
    /// `TemplateStructurePiece.addAdditionalSaveData` and its subclasses',
    /// with the height a shipwreck settled on once placed.
    pub fn saved(&self) -> crate::nbt::Compound {
        use crate::nbt::Tag;
        let flag = |b: bool| Tag::Byte(i8::from(b));
        let adjusted = match self.handler {
            TemplateHandler::Shipwreck { .. }
            | TemplateHandler::OceanRuin { .. }
            | TemplateHandler::Igloo => *self.state.lock().unwrap(),
            _ => None,
        };
        let dy = adjusted.map_or(0, |y| y - self.position[1]);
        let template_dy = if matches!(self.handler, TemplateHandler::Igloo) {
            0
        } else {
            dy
        };
        let mut tag = super::legacy::piece_tag(
            self.type_name,
            self.bounding_box.moved([0, dy, 0]),
            Some(Direction::North),
            self.gen_depth,
        );
        // Mansion and End City pieces keep the name inside their folder.
        let name = self
            .template_name
            .strip_prefix("minecraft:woodland_mansion/")
            .or_else(|| self.template_name.strip_prefix("minecraft:end_city/"))
            .unwrap_or(&self.template_name);
        let rotation = || Tag::String(self.rotation.saved_name().into());
        let mut entries = vec![
            ("TPX", Tag::Int(self.position[0])),
            ("TPY", Tag::Int(self.position[1] + template_dy)),
            ("TPZ", Tag::Int(self.position[2])),
            ("Template", Tag::String(name.into())),
        ];
        match self.handler {
            TemplateHandler::Igloo => entries.push(("Rot", rotation())),
            TemplateHandler::Shipwreck { beached } => entries.extend([
                ("isBeached", flag(beached)),
                ("Rot", rotation()),
                ("height_adjusted", flag(adjusted.is_some())),
            ]),
            TemplateHandler::OceanRuin {
                warm,
                large,
                integrity,
            } => entries.extend([
                ("Rot", rotation()),
                ("Integrity", Tag::Float(integrity.to_bits())),
                (
                    "BiomeType",
                    Tag::String(if warm { "WARM" } else { "COLD" }.into()),
                ),
                ("IsLarge", flag(large)),
            ]),
            TemplateHandler::RuinedPortal(props) => {
                entries.push(("Rotation", rotation()));
                entries.push(("Mirror", Tag::String(self.mirror.saved_name().into())));
                entries.extend(props.save());
            }
            TemplateHandler::Mansion => entries.extend([
                ("Rot", rotation()),
                ("Mi", Tag::String(self.mirror.saved_name().into())),
            ]),
            TemplateHandler::Plain => entries.push(("Rot", rotation())),
        }
        for (key, value) in entries {
            tag.insert(key.into(), value);
        }
        tag
    }
}
impl LegacyPiece for TemplatePiece {
    fn bounding_box(&self) -> BoundingBox {
        self.bounding_box
    }
    fn save(&self) -> Option<crate::nbt::Compound> {
        Some(self.saved())
    }
    fn after_place(&self, c: &mut PieceContext<'_>, pieces: &[BoundingBox]) -> Result<()> {
        match self.handler {
            TemplateHandler::Mansion => super::mansion::after_place(c, pieces),
            _ => Ok(()),
        }
    }
    fn type_name(&self) -> &'static str {
        self.type_name
    }
    fn post_process(&self, c: &mut PieceContext<'_>) -> Result<()> {
        match self.handler {
            TemplateHandler::Plain => {
                self.place_at(c, self.position, &[])?;
            }
            TemplateHandler::Igloo => {
                let o = igloo_offset(&self.template_name);
                let entrance_relative = StructureTemplate::transform(
                    [3 - o[0], 0, -o[2]],
                    Mirror::None,
                    self.rotation,
                    self.pivot,
                );
                let entrance = [
                    self.position[0] + entrance_relative[0],
                    0,
                    self.position[2] + entrance_relative[2],
                ];
                let height =
                    c.world
                        .height(HeightmapType::WorldSurfaceWg, entrance[0], entrance[2])?;
                let position = [
                    self.position[0],
                    self.position[1] + height - 90 - 1,
                    self.position[2],
                ];
                // The box stays moved; templatePosition goes back after placing.
                *self.state.lock().unwrap() = Some(position[1]);
                if let Some(markers) = self.place_at(c, position, &[])? {
                    for (marker, pos) in markers {
                        if marker == "chest" {
                            c.world.set(pos, c.state("air"), 3)?;
                            let below = offset(pos, Direction::Down, 1);
                            if c.is_block(below, "minecraft:chest")? {
                                c.loot_table(
                                    below,
                                    "minecraft:chest",
                                    "minecraft:chests/igloo_chest",
                                )?;
                            }
                        }
                    }
                }
                if self.template_name == IGLOO_TOP {
                    let r = StructureTemplate::transform(
                        [3, 0, 5],
                        Mirror::None,
                        self.rotation,
                        self.pivot,
                    );
                    let trapdoor = [position[0] + r[0], position[1] + r[1], position[2] + r[2]];
                    let below = c.world.block(offset(trapdoor, Direction::Down, 1))?;
                    if !c.is_air(below) && c.name(below) != "minecraft:ladder" {
                        c.world.set(trapdoor, c.state("snow_block"), 3)?;
                    }
                }
            }
            TemplateHandler::Shipwreck { beached } => {
                let adjusted = *self.state.lock().unwrap();
                let y = match adjusted {
                    Some(y) => y,
                    None if shipwreck_too_big(&self.template) => self.position[1],
                    None => {
                        let kind = if beached {
                            HeightmapType::WorldSurfaceWg
                        } else {
                            HeightmapType::OceanFloorWg
                        };
                        let size = self.template.size;
                        let mut min = c.world.max_y()? + 1;
                        let mut mean = 0;
                        let base = size[0] * size[2];
                        if base == 0 {
                            mean = c.world.height(kind, self.position[0], self.position[2])?;
                        } else {
                            for x in self.position[0]..self.position[0] + size[0] {
                                for z in self.position[2]..self.position[2] + size[2] {
                                    let h = c.world.height(kind, x, z)?;
                                    mean += h;
                                    min = min.min(h);
                                }
                            }
                            mean /= base;
                        }
                        let y = if beached {
                            min - size[1] / 2 - c.random.int(3)
                        } else {
                            mean
                        };
                        *self.state.lock().unwrap() = Some(y);
                        y
                    }
                };
                let position = [self.position[0], y, self.position[2]];
                if let Some(markers) = self.place_at(c, position, &[])? {
                    for (marker, pos) in markers {
                        let table = match marker.as_str() {
                            "map_chest" => "minecraft:chests/shipwreck_map",
                            "treasure_chest" => "minecraft:chests/shipwreck_treasure",
                            "supply_chest" => "minecraft:chests/shipwreck_supply",
                            _ => continue,
                        };
                        let below = offset(pos, Direction::Down, 1);
                        let state = c.world.block(below)?;
                        if c.resources.transforms.loot_container(state) {
                            let kind = c.name(state).to_owned();
                            c.loot_table(below, &kind, table)?;
                        }
                    }
                }
            }
            TemplateHandler::OceanRuin { .. } => {
                super::ocean_ruin::post_process(self, c)?;
            }
            TemplateHandler::RuinedPortal(_) => {
                super::ruined_portal::post_process(self, c)?;
            }
            TemplateHandler::Mansion => {
                super::mansion::post_process(self, c)?;
            }
        }
        Ok(())
    }
}

/// ScatteredFeaturePiece: an oriented box whose height is fixed on first use.
#[derive(Debug)]
pub struct Scattered {
    pub frame: Mutex<PieceFrame>,
    pub width: i32,
    pub height: i32,
    pub depth: i32,
    pub height_position: Mutex<i32>,
}
impl Scattered {
    pub fn new(
        west: i32,
        floor: i32,
        north: i32,
        width: i32,
        height: i32,
        depth: i32,
        direction: Direction,
    ) -> Self {
        Self {
            frame: Mutex::new(PieceFrame::new(
                make_bounding_box(west, floor, north, direction, width, height, depth),
                Some(direction),
            )),
            width,
            height,
            depth,
            height_position: Mutex::new(-1),
        }
    }
    pub fn frame(&self) -> PieceFrame {
        *self.frame.lock().unwrap()
    }
    /// `ScatteredFeaturePiece`'s tag: the box and the height it settled on.
    pub fn saved(&self, id: &str) -> crate::nbt::Compound {
        use crate::nbt::Tag;
        let f = self.frame();
        let mut tag = super::legacy::piece_tag(id, f.bounding_box, f.orientation, 0);
        for (key, value) in [
            ("Width", self.width),
            ("Height", self.height),
            ("Depth", self.depth),
            ("HPos", *self.height_position.lock().unwrap()),
        ] {
            tag.insert(key.into(), Tag::Int(value));
        }
        tag
    }
    /// updateAverageGroundHeight over the part of the box inside the chunk.
    pub fn average_ground(&self, c: &mut PieceContext<'_>, offset: i32) -> Result<bool> {
        if *self.height_position.lock().unwrap() >= 0 {
            return Ok(true);
        }
        let b = self.frame().bounding_box;
        let (mut total, mut count) = (0, 0);
        for z in b.min[2]..=b.max[2] {
            for x in b.min[0]..=b.max[0] {
                if c.chunk_box.is_inside([x, 64, z]) {
                    total += c
                        .world
                        .height(HeightmapType::MotionBlockingNoLeaves, x, z)?;
                    count += 1;
                }
            }
        }
        if count == 0 {
            return Ok(false);
        }
        let h = total / count;
        *self.height_position.lock().unwrap() = h;
        let mut frame = self.frame.lock().unwrap();
        frame.bounding_box =
            frame
                .bounding_box
                .moved([0, h - frame.bounding_box.min[1] + offset, 0]);
        Ok(true)
    }
    /// updateHeightPositionToLowestGroundHeight over the whole box.
    pub fn lowest_ground(&self, c: &mut PieceContext<'_>, offset: i32) -> Result<bool> {
        if *self.height_position.lock().unwrap() >= 0 {
            return Ok(true);
        }
        let b = self.frame().bounding_box;
        let mut lowest = c.world.max_y()? + 1;
        for z in b.min[2]..=b.max[2] {
            for x in b.min[0]..=b.max[0] {
                lowest = lowest.min(
                    c.world
                        .height(HeightmapType::MotionBlockingNoLeaves, x, z)?,
                );
            }
        }
        *self.height_position.lock().unwrap() = lowest;
        let mut frame = self.frame.lock().unwrap();
        frame.bounding_box =
            frame
                .bounding_box
                .moved([0, lowest - frame.bounding_box.min[1] + offset, 0]);
        Ok(true)
    }
}

#[derive(Debug)]
pub struct SwampHutPiece {
    pub base: Scattered,
    /// spawnedWitch and spawnedCat: both spawn in the same chunk.
    spawned: Mutex<bool>,
}
impl SwampHutPiece {
    pub fn new(random: &mut impl RandomSource, west: i32, north: i32) -> Self {
        let direction = random_horizontal(random);
        Self {
            base: Scattered::new(west, 64, north, 7, 7, 9, direction),
            spawned: Mutex::new(false),
        }
    }
}
impl LegacyPiece for SwampHutPiece {
    fn bounding_box(&self) -> BoundingBox {
        self.base.frame().bounding_box
    }
    fn save(&self) -> Option<crate::nbt::Compound> {
        let mut tag = self.base.saved("minecraft:tesh");
        let spawned = crate::nbt::Tag::Byte(i8::from(*self.spawned.lock().unwrap()));
        tag.insert("Witch".into(), spawned.clone());
        tag.insert("Cat".into(), spawned);
        Some(tag)
    }
    fn type_name(&self) -> &'static str {
        "minecraft:tesh"
    }
    fn post_process(&self, c: &mut PieceContext<'_>) -> Result<()> {
        if !self.base.average_ground(c, 0)? {
            return Ok(());
        }
        let f = self.base.frame();
        let planks = c.state("spruce_planks");
        let log = c.state("oak_log");
        for (a, b) in [
            ([1, 1, 1], [5, 1, 7]),
            ([1, 4, 2], [5, 4, 7]),
            ([2, 1, 0], [4, 1, 0]),
            ([2, 2, 2], [3, 3, 2]),
            ([1, 2, 3], [1, 3, 6]),
            ([5, 2, 3], [5, 3, 6]),
            ([2, 2, 7], [4, 3, 7]),
        ] {
            f.generate_box(c, a, b, planks, planks, false)?;
        }
        for (a, b) in [
            ([1, 0, 2], [1, 3, 2]),
            ([5, 0, 2], [5, 3, 2]),
            ([1, 0, 7], [1, 3, 7]),
            ([5, 0, 7], [5, 3, 7]),
        ] {
            f.generate_box(c, a, b, log, log, false)?;
        }
        let fence = c.state("oak_fence");
        let air = c.state("air");
        f.place(c, fence, 2, 3, 2)?;
        f.place(c, fence, 3, 3, 7)?;
        f.place(c, air, 1, 3, 4)?;
        f.place(c, air, 5, 3, 4)?;
        f.place(c, air, 5, 3, 5)?;
        f.place(c, c.state("potted_red_mushroom"), 1, 3, 5)?;
        f.place(c, c.state("crafting_table"), 3, 2, 6)?;
        f.place(c, c.state("cauldron"), 4, 2, 6)?;
        f.place(c, fence, 1, 2, 1)?;
        f.place(c, fence, 5, 2, 1)?;
        let stairs = |c: &PieceContext<'_>, facing: &str, shape: &str| {
            c.state_with("spruce_stairs", &[("facing", facing), ("shape", shape)])
        };
        let north = stairs(c, "north", "straight");
        let east = stairs(c, "east", "straight");
        let west = stairs(c, "west", "straight");
        let south = stairs(c, "south", "straight");
        f.generate_box(c, [0, 4, 1], [6, 4, 1], north, north, false)?;
        f.generate_box(c, [0, 4, 2], [0, 4, 7], east, east, false)?;
        f.generate_box(c, [6, 4, 2], [6, 4, 7], west, west, false)?;
        f.generate_box(c, [0, 4, 8], [6, 4, 8], south, south, false)?;
        f.place(c, stairs(c, "north", "outer_right"), 0, 4, 1)?;
        f.place(c, stairs(c, "north", "outer_left"), 6, 4, 1)?;
        f.place(c, stairs(c, "south", "outer_left"), 0, 4, 8)?;
        f.place(c, stairs(c, "south", "outer_right"), 6, 4, 8)?;
        for z in [2, 7] {
            for x in [1, 5] {
                f.fill_column_down(c, log, x, -1, z)?;
            }
        }
        // The witch, then spawnCat, both at (2, 2, 5).
        let pos = f.world_pos(2, 2, 5);
        if c.chunk_box.is_inside(pos) {
            for id in ["minecraft:witch", "minecraft:cat"] {
                c.world
                    .entity(super::legacy::spawn_nbt(id, pos, true, true))?;
            }
            *self.spawned.lock().unwrap() = true;
        }
        Ok(())
    }
}
