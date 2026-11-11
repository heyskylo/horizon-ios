//! Mineshafts: MineshaftStructure and MineshaftPieces.
use super::{
    BoundingBox,
    legacy::{LegacyPiece, PieceContext, PieceFrame, Result, solid_render},
};
use crate::datafix::dynamic::as_number;
use crate::nbt::Tag;
use crate::worldgen::biome::BiomeId;
use horizon_core::{geometry::Direction, random::RandomSource};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

/// MineshaftStructure.Type: wood, planks, fence, and the blocks pieces keep.
#[derive(Clone, Copy, Debug)]
pub struct Wood {
    log: &'static str,
    planks: &'static str,
    fence: &'static str,
    keep: &'static [&'static str],
}
pub const NORMAL: Wood = Wood {
    log: "minecraft:oak_log",
    planks: "minecraft:oak_planks",
    fence: "minecraft:oak_fence",
    keep: &[
        "minecraft:oak_planks",
        "minecraft:oak_log",
        "minecraft:oak_fence",
        "minecraft:iron_chain",
    ],
};
pub const MESA: Wood = Wood {
    log: "minecraft:dark_oak_log",
    planks: "minecraft:dark_oak_planks",
    fence: "minecraft:dark_oak_fence",
    keep: &[
        "minecraft:dark_oak_planks",
        "minecraft:dark_oak_log",
        "minecraft:dark_oak_fence",
        "minecraft:iron_chain",
    ],
};

#[derive(Debug)]
enum Kind {
    Room(Vec<BoundingBox>),
    Corridor {
        rails: bool,
        spider: bool,
        sections: i32,
        placed_spider: Mutex<bool>,
    },
    Crossing {
        direction: Direction,
        two_floored: bool,
    },
    Stairs,
}
#[derive(Debug)]
pub struct MineshaftPiece {
    kind: Kind,
    frame: PieceFrame,
    depth: i32,
    wood: Wood,
    blocking: Arc<BTreeSet<BiomeId>>,
}

fn int(r: &mut impl RandomSource, bound: i32) -> i32 {
    r.next_int_bound(bound).unwrap_or(0)
}
fn bb(a: [i32; 3], b: [i32; 3], at: [i32; 3]) -> BoundingBox {
    BoundingBox::new(a, b).moved(at)
}

struct Builder {
    pieces: Vec<MineshaftPiece>,
}
impl Builder {
    fn collides(&self, b: &BoundingBox) -> bool {
        self.pieces
            .iter()
            .any(|p| p.frame.bounding_box.intersects(b))
    }
    fn piece(
        &self,
        kind: Kind,
        b: BoundingBox,
        o: Option<Direction>,
        depth: i32,
    ) -> MineshaftPiece {
        let start = &self.pieces[0];
        let mut frame = PieceFrame::new(b, o);
        frame.keep = start.wood.keep;
        MineshaftPiece {
            kind,
            frame,
            depth,
            wood: start.wood,
            blocking: start.blocking.clone(),
        }
    }
    /// MineshaftPieces.createRandomShaftPiece.
    fn create(
        &self,
        r: &mut impl RandomSource,
        at: [i32; 3],
        d: Direction,
        depth: i32,
    ) -> Option<MineshaftPiece> {
        let selection = int(r, 100);
        if selection >= 80 {
            let y1 = if int(r, 4) == 0 { 6 } else { 2 };
            let b = match d {
                Direction::South => bb([-1, 0, 0], [3, y1, 4], at),
                Direction::West => bb([-4, 0, -1], [0, y1, 3], at),
                Direction::East => bb([0, 0, -1], [4, y1, 3], at),
                _ => bb([-1, 0, -4], [3, y1, 0], at),
            };
            if self.collides(&b) {
                return None;
            }
            let kind = Kind::Crossing {
                direction: d,
                two_floored: b.span()[1] > 3,
            };
            Some(self.piece(kind, b, None, depth))
        } else if selection >= 70 {
            let b = match d {
                Direction::South => bb([0, -5, 0], [2, 2, 8], at),
                Direction::West => bb([-8, -5, 0], [0, 2, 2], at),
                Direction::East => bb([0, -5, 0], [8, 2, 2], at),
                _ => bb([0, -5, -8], [2, 2, 0], at),
            };
            (!self.collides(&b)).then(|| self.piece(Kind::Stairs, b, Some(d), depth))
        } else {
            let mut length = int(r, 3) + 2;
            while length > 0 {
                let l = length * 5 - 1;
                let b = match d {
                    Direction::South => bb([0, 0, 0], [2, 2, l], at),
                    Direction::West => bb([-l, 0, 0], [0, 2, 2], at),
                    Direction::East => bb([0, 0, 0], [l, 2, 2], at),
                    _ => bb([0, 0, -l], [2, 2, 0], at),
                };
                if !self.collides(&b) {
                    let rails = int(r, 3) == 0;
                    let spider = !rails && int(r, 23) == 0;
                    let axis = if matches!(d, Direction::North | Direction::South) {
                        2
                    } else {
                        0
                    };
                    let kind = Kind::Corridor {
                        rails,
                        spider,
                        sections: b.span()[axis] / 5,
                        placed_spider: Mutex::new(false),
                    };
                    return Some(self.piece(kind, b, Some(d), depth));
                }
                length -= 1;
            }
            None
        }
    }
    /// MineshaftPieces.generateAndAddPiece; returns the new piece's box.
    fn add(
        &mut self,
        r: &mut impl RandomSource,
        at: [i32; 3],
        d: Direction,
        depth: i32,
    ) -> Option<BoundingBox> {
        if depth > 8 {
            return None;
        }
        let start = self.pieces[0].frame.bounding_box;
        if (at[0] - start.min[0]).abs() > 80 || (at[2] - start.min[2]).abs() > 80 {
            return None;
        }
        let piece = self.create(r, at, d, depth + 1)?;
        let b = piece.frame.bounding_box;
        self.pieces.push(piece);
        self.add_children(r, self.pieces.len() - 1);
        Some(b)
    }
    fn add_children(&mut self, r: &mut impl RandomSource, index: usize) {
        let p = &self.pieces[index];
        let (b, depth, orientation) = (p.frame.bounding_box, p.depth, p.frame.orientation);
        let ([x0, y0, z0], [x1, _y1, z1]) = (b.min, b.max);
        use Direction::*;
        match &p.kind {
            Kind::Room(_) => {
                let height = (b.span()[1] - 4).max(1);
                let [xs, _, zs] = b.span();
                for side in 0..4 {
                    let span = if side < 2 { xs } else { zs };
                    let mut pos = 0;
                    while pos < span {
                        pos += int(r, span);
                        if pos + 3 > span {
                            break;
                        }
                        let y = y0 + int(r, height) + 1;
                        let (at, d) = match side {
                            0 => ([x0 + pos, y, z0 - 1], North),
                            1 => ([x0 + pos, y, z1 + 1], South),
                            2 => ([x0 - 1, y, z0 + pos], West),
                            _ => ([x1 + 1, y, z0 + pos], East),
                        };
                        if let Some(c) = self.add(r, at, d, depth) {
                            let entrance = match side {
                                0 => BoundingBox::new(
                                    [c.min[0], c.min[1], z0],
                                    [c.max[0], c.max[1], z0 + 1],
                                ),
                                1 => BoundingBox::new(
                                    [c.min[0], c.min[1], z1 - 1],
                                    [c.max[0], c.max[1], z1],
                                ),
                                2 => BoundingBox::new(
                                    [x0, c.min[1], c.min[2]],
                                    [x0 + 1, c.max[1], c.max[2]],
                                ),
                                _ => BoundingBox::new(
                                    [x1 - 1, c.min[1], c.min[2]],
                                    [x1, c.max[1], c.max[2]],
                                ),
                            };
                            if let Kind::Room(entrances) = &mut self.pieces[index].kind {
                                entrances.push(entrance);
                            }
                        }
                        pos += 4;
                    }
                }
            }
            Kind::Corridor { .. } => {
                let end = int(r, 4);
                let o = orientation.unwrap_or(North);
                let (forward, left, right) = match o {
                    South => (
                        [x0, z1 + 1],
                        (West, [x0 - 1, z1 - 3]),
                        (East, [x1 + 1, z1 - 3]),
                    ),
                    West => ([x0 - 1, z0], (North, [x0, z0 - 1]), (South, [x0, z1 + 1])),
                    East => (
                        [x1 + 1, z0],
                        (North, [x1 - 3, z0 - 1]),
                        (South, [x1 - 3, z1 + 1]),
                    ),
                    _ => ([x0, z0 - 1], (West, [x0 - 1, z0]), (East, [x1 + 1, z0])),
                };
                let (d, [x, z]) = match end {
                    0 | 1 => (o, forward),
                    2 => left,
                    _ => right,
                };
                let y = y0 - 1 + int(r, 3);
                self.add(r, [x, y, z], d, depth);
                if depth < 8 {
                    if o != North && o != South {
                        let mut x = x0 + 3;
                        while x + 3 <= x1 {
                            match int(r, 5) {
                                0 => {
                                    self.add(r, [x, y0, z0 - 1], North, depth + 1);
                                }
                                1 => {
                                    self.add(r, [x, y0, z1 + 1], South, depth + 1);
                                }
                                _ => {}
                            }
                            x += 5;
                        }
                    } else {
                        let mut z = z0 + 3;
                        while z + 3 <= z1 {
                            match int(r, 5) {
                                0 => {
                                    self.add(r, [x0 - 1, y0, z], West, depth + 1);
                                }
                                1 => {
                                    self.add(r, [x1 + 1, y0, z], East, depth + 1);
                                }
                                _ => {}
                            }
                            z += 5;
                        }
                    }
                }
            }
            Kind::Crossing {
                direction,
                two_floored,
            } => {
                let two = *two_floored;
                let n = ([x0 + 1, y0, z0 - 1], North);
                let s = ([x0 + 1, y0, z1 + 1], South);
                let w = ([x0 - 1, y0, z0 + 1], West);
                let e = ([x1 + 1, y0, z0 + 1], East);
                let order = match direction {
                    South => [s, w, e],
                    West => [n, s, w],
                    East => [n, s, e],
                    _ => [n, w, e],
                };
                for (at, d) in order {
                    self.add(r, at, d, depth);
                }
                if two {
                    for (at, d) in [n, w, e, s] {
                        if r.next_boolean() {
                            self.add(r, [at[0], y0 + 4, at[2]], d, depth);
                        }
                    }
                }
            }
            Kind::Stairs => {
                let (at, d) = match orientation.unwrap_or(North) {
                    South => ([x0, y0, z1 + 1], South),
                    West => ([x0 - 1, y0, z0], West),
                    East => ([x1 + 1, y0, z0], East),
                    _ => ([x0, y0, z0 - 1], North),
                };
                self.add(r, at, d, depth);
            }
        }
    }
}

/// MineshaftStructure.findGenerationPoint: the pieces and the start position.
pub fn generate(
    r: &mut impl RandomSource,
    chunk: [i32; 2],
    mesa: bool,
    blocking: Arc<BTreeSet<BiomeId>>,
    sea_level: i32,
    min_y: i32,
    surface: &mut dyn FnMut(i32, i32) -> i32,
) -> ([i32; 3], Vec<MineshaftPiece>) {
    r.next_double();
    let (west, north) = (chunk[0] * 16 + 2, chunk[1] * 16 + 2);
    let room = BoundingBox::new(
        [west, 50, north],
        [west + 7 + int(r, 6), 54 + int(r, 6), north + 7 + int(r, 6)],
    );
    let wood = if mesa { MESA } else { NORMAL };
    let mut frame = PieceFrame::new(room, None);
    frame.keep = wood.keep;
    let mut builder = Builder {
        pieces: vec![MineshaftPiece {
            kind: Kind::Room(Vec::new()),
            frame,
            depth: 0,
            wood,
            blocking,
        }],
    };
    builder.add_children(r, 0);
    let mut all = builder.pieces[0].frame.bounding_box;
    for p in &builder.pieces[1..] {
        all.encapsulate(&p.frame.bounding_box);
    }
    let dy = if mesa {
        let center = all.center();
        let height = surface(center[0], center[2]);
        let target = if height <= sea_level {
            sea_level
        } else {
            int(r, height - sea_level + 1) + sea_level
        };
        target - center[1]
    } else {
        let max = sea_level - 10;
        let mut y1 = all.span()[1] + min_y + 1;
        if y1 < max {
            y1 += int(r, max - y1);
        }
        y1 - all.max[1]
    };
    for p in &mut builder.pieces {
        p.frame.bounding_box = p.frame.bounding_box.moved([0, dy, 0]);
        if let Kind::Room(entrances) = &mut p.kind {
            for e in entrances {
                *e = e.moved([0, dy, 0]);
            }
        }
    }
    ([chunk[0] * 16 + 8, 50 + dy, chunk[1] * 16], builder.pieces)
}

fn replaceable(name: &str) -> bool {
    matches!(
        name,
        "minecraft:air"
            | "minecraft:cave_air"
            | "minecraft:void_air"
            | "minecraft:water"
            | "minecraft:lava"
            | "minecraft:glow_lichen"
            | "minecraft:seagrass"
            | "minecraft:tall_seagrass"
    )
}
fn falling(name: &str) -> bool {
    matches!(
        name,
        "minecraft:sand"
            | "minecraft:red_sand"
            | "minecraft:gravel"
            | "minecraft:anvil"
            | "minecraft:chipped_anvil"
            | "minecraft:damaged_anvil"
            | "minecraft:dragon_egg"
    ) || name.ends_with("_concrete_powder")
}

impl MineshaftPiece {
    fn invalid(&self, c: &mut PieceContext<'_>) -> Result<bool> {
        let (b, k) = (self.frame.bounding_box, c.chunk_box);
        let lo: [i32; 3] = std::array::from_fn(|i| (b.min[i] - 1).max(k.min[i]));
        let hi: [i32; 3] = std::array::from_fn(|i| (b.max[i] + 1).min(k.max[i]));
        let center = std::array::from_fn(|i| (lo[i] + hi[i]) / 2);
        if self.blocking.contains(&c.world.biome(center)?) {
            return Ok(true);
        }
        let liquid = |c: &mut PieceContext<'_>, p: [i32; 3]| -> Result<bool> {
            let s = c.world.block(p)?;
            Ok(c.is_liquid(s))
        };
        for x in lo[0]..=hi[0] {
            for z in lo[2]..=hi[2] {
                if liquid(c, [x, lo[1], z])? || liquid(c, [x, hi[1], z])? {
                    return Ok(true);
                }
            }
        }
        for x in lo[0]..=hi[0] {
            for y in lo[1]..=hi[1] {
                if liquid(c, [x, y, lo[2]])? || liquid(c, [x, y, hi[2]])? {
                    return Ok(true);
                }
            }
        }
        for z in lo[2]..=hi[2] {
            for y in lo[1]..=hi[1] {
                if liquid(c, [lo[0], y, z])? || liquid(c, [hi[0], y, z])? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
    fn set_planks(&self, c: &mut PieceContext<'_>, x: i32, y: i32, z: i32) -> Result<()> {
        if self.frame.is_interior(c, x, y, z)? {
            let pos = self.frame.world_pos(x, y, z);
            if !c.world.face_sturdy(pos, Direction::Up)? {
                c.world.set(pos, c.state(self.wood.planks), 2)?;
            }
        }
        Ok(())
    }
    fn air_box(&self, c: &mut PieceContext<'_>, a: [i32; 3], b: [i32; 3]) -> Result<()> {
        let air = c.state("cave_air");
        self.frame.generate_box(c, a, b, air, air, false)
    }
    fn room(&self, c: &mut PieceContext<'_>, entrances: &[BoundingBox]) -> Result<()> {
        let b = self.frame.bounding_box;
        self.air_box(
            c,
            [b.min[0], b.min[1] + 1, b.min[2]],
            [b.max[0], (b.min[1] + 3).min(b.max[1]), b.max[2]],
        )?;
        for e in entrances {
            self.air_box(c, [e.min[0], e.max[1] - 2, e.min[2]], e.max)?;
        }
        let air = c.state("cave_air");
        self.frame
            .upper_half_sphere(c, [b.min[0], b.min[1] + 4, b.min[2]], b.max, air, false)
    }
    fn crossing(&self, c: &mut PieceContext<'_>, two_floored: bool) -> Result<()> {
        let ([x0, y0, z0], [x1, y1, z1]) =
            (self.frame.bounding_box.min, self.frame.bounding_box.max);
        if two_floored {
            self.air_box(c, [x0 + 1, y0, z0], [x1 - 1, y0 + 2, z1])?;
            self.air_box(c, [x0, y0, z0 + 1], [x1, y0 + 2, z1 - 1])?;
            self.air_box(c, [x0 + 1, y1 - 2, z0], [x1 - 1, y1, z1])?;
            self.air_box(c, [x0, y1 - 2, z0 + 1], [x1, y1, z1 - 1])?;
            self.air_box(c, [x0 + 1, y0 + 3, z0 + 1], [x1 - 1, y0 + 3, z1 - 1])?;
        } else {
            self.air_box(c, [x0 + 1, y0, z0], [x1 - 1, y1, z1])?;
            self.air_box(c, [x0, y0, z0 + 1], [x1, y1, z1 - 1])?;
        }
        let planks = c.state(self.wood.planks);
        let air = c.state("cave_air");
        for (x, z) in [
            (x0 + 1, z0 + 1),
            (x0 + 1, z1 - 1),
            (x1 - 1, z0 + 1),
            (x1 - 1, z1 - 1),
        ] {
            let above = self.frame.get(c, x, y1 + 1, z)?;
            if !c.is_air(above) {
                self.frame
                    .generate_box(c, [x, y0, z], [x, y1, z], planks, air, false)?;
            }
        }
        for x in x0..=x1 {
            for z in z0..=z1 {
                self.set_planks(c, x, y0 - 1, z)?;
            }
        }
        Ok(())
    }
    fn stairs(&self, c: &mut PieceContext<'_>) -> Result<()> {
        self.air_box(c, [0, 5, 0], [2, 7, 1])?;
        self.air_box(c, [0, 0, 7], [2, 2, 8])?;
        for i in 0..5 {
            self.air_box(c, [0, 5 - i - i32::from(i < 4), 2 + i], [2, 7 - i, 2 + i])?;
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn corridor(
        &self,
        c: &mut PieceContext<'_>,
        rails: bool,
        spider: bool,
        sections: i32,
        placed_spider: &Mutex<bool>,
    ) -> Result<()> {
        let f = self.frame;
        let length = sections * 5 - 1;
        let air = c.state("cave_air");
        let web = c.state("cobweb");
        f.generate_box(c, [0, 0, 0], [2, 1, length], air, air, false)?;
        f.generate_maybe_box(c, 0.8, [0, 2, 0], [2, 2, length], air, air, false, false)?;
        if spider {
            f.generate_maybe_box(c, 0.6, [0, 0, 0], [2, 1, length], web, air, false, true)?;
        }
        for section in 0..sections {
            let z = 2 + section * 5;
            self.support(c, z)?;
            for (p, x, dz) in [
                (0.1, 0, -1),
                (0.1, 2, -1),
                (0.1, 0, 1),
                (0.1, 2, 1),
                (0.05, 0, -2),
                (0.05, 2, -2),
                (0.05, 0, 2),
                (0.05, 2, 2),
            ] {
                if f.is_interior(c, x, 2, z + dz)?
                    && c.random.float() < p
                    && self.sturdy_neighbours(c, [x, 2, z + dz])?
                {
                    f.place(c, web, x, 2, z + dz)?;
                }
            }
            if c.random.int(100) == 0 {
                self.minecart_chest(c, 2, 0, z - 1)?;
            }
            if c.random.int(100) == 0 {
                self.minecart_chest(c, 0, 0, z + 1)?;
            }
            if spider && !*placed_spider.lock().unwrap() {
                let nz = z - 1 + c.random.int(3);
                let pos = f.world_pos(1, 0, nz);
                if c.chunk_box.is_inside(pos) && f.is_interior(c, 1, 0, nz)? {
                    *placed_spider.lock().unwrap() = true;
                    c.spawner(pos, "minecraft:cave_spider")?;
                }
            }
        }
        for x in 0..=2 {
            for z in 0..=length {
                self.set_planks(c, x, -1, z)?;
            }
        }
        self.double_support(c, 2)?;
        if sections > 1 {
            self.double_support(c, length - 2)?;
        }
        if rails {
            let rail = c.state_with("rail", &[("shape", "north_south")]);
            for z in 0..=length {
                let floor = f.get(c, 1, -1, z)?;
                if !c.is_air(floor) && solid_render(c, floor) {
                    let p = if f.is_interior(c, 1, 0, z)? { 0.7 } else { 0.9 };
                    f.maybe_block(c, p, 1, 0, z, rail)?;
                }
            }
        }
        Ok(())
    }
    /// MineShaftCorridor.createChest: a rail with a chest minecart.
    fn minecart_chest(&self, c: &mut PieceContext<'_>, x: i32, y: i32, z: i32) -> Result<()> {
        let pos = self.frame.world_pos(x, y, z);
        if !c.chunk_box.is_inside(pos) {
            return Ok(());
        }
        let here = c.world.block(pos)?;
        let below = c.world.block([pos[0], pos[1] - 1, pos[2]])?;
        if c.is_air(here) && !c.is_air(below) {
            let shape = if c.random.boolean() {
                "north_south"
            } else {
                "east_west"
            };
            let rail = c.state_with("rail", &[("shape", shape)]);
            self.frame.place(c, rail, x, y, z)?;
            // A chest minecart with the mineshaft loot table.
            let seed = c.random.long();
            let mut nbt = super::legacy::spawn_nbt("minecraft:chest_minecart", pos, false, false);
            // As the proto chunk saves the new cart: not ticked, not burning.
            nbt.insert("HasTicked".into(), Tag::Byte(1));
            nbt.insert("Fire".into(), Tag::Short(0));
            let p = [
                f64::from(pos[0]) + 0.5,
                f64::from(pos[1]) + 0.5,
                f64::from(pos[2]) + 0.5,
            ];
            nbt.insert(
                "Pos".into(),
                Tag::List(p.iter().map(|v| Tag::Double(v.to_bits())).collect()),
            );
            nbt.insert(
                "LootTable".into(),
                Tag::String("minecraft:chests/abandoned_mineshaft".into()),
            );
            nbt.insert("LootTableSeed".into(), Tag::Long(seed));
            c.world.entity(nbt)?;
        }
        Ok(())
    }
    /// MineShaftCorridor.placeSupport(x0 0, y0 0, z, y1 2, x1 2).
    fn support(&self, c: &mut PieceContext<'_>, z: i32) -> Result<()> {
        let f = self.frame;
        for x in 0..=2 {
            let above = f.get(c, x, 3, z)?;
            if c.is_air(above) {
                return Ok(());
            }
        }
        let air = c.state("cave_air");
        let planks = c.state(self.wood.planks);
        let west = c.state_with(self.wood.fence, &[("west", "true")]);
        let east = c.state_with(self.wood.fence, &[("east", "true")]);
        f.generate_box(c, [0, 0, z], [0, 1, z], west, air, false)?;
        f.generate_box(c, [2, 0, z], [2, 1, z], east, air, false)?;
        if c.random.int(4) == 0 {
            f.generate_box(c, [0, 2, z], [0, 2, z], planks, air, false)?;
            f.generate_box(c, [2, 2, z], [2, 2, z], planks, air, false)?;
        } else {
            f.generate_box(c, [0, 2, z], [2, 2, z], planks, air, false)?;
            let south = c.state_with("wall_torch", &[("facing", "south")]);
            let north = c.state_with("wall_torch", &[("facing", "north")]);
            f.maybe_block(c, 0.05, 1, 2, z - 1, south)?;
            f.maybe_block(c, 0.05, 1, 2, z + 1, north)?;
        }
        Ok(())
    }
    fn sturdy_neighbours(&self, c: &mut PieceContext<'_>, [x, y, z]: [i32; 3]) -> Result<bool> {
        let pos = self.frame.world_pos(x, y, z);
        let mut count = 0;
        for d in Direction::ALL {
            let n = super::legacy::offset(pos, d, 1);
            if c.chunk_box.is_inside(n) && c.world.face_sturdy(n, d.opposite())? {
                count += 1;
                if count >= 2 {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
    fn double_support(&self, c: &mut PieceContext<'_>, z: i32) -> Result<()> {
        for x in [0, 2] {
            let s = self.frame.get(c, x, -1, z)?;
            if c.name(s) == self.wood.planks {
                self.pillar_or_chain(c, x, -1, z)?;
            }
        }
        Ok(())
    }
    /// MineShaftCorridor.fillPillarDownOrChainUp.
    fn pillar_or_chain(&self, c: &mut PieceContext<'_>, x: i32, y: i32, z: i32) -> Result<()> {
        let [px, wy, pz] = self.frame.world_pos(x, y, z);
        if !c.chunk_box.is_inside([px, wy, pz]) {
            return Ok(());
        }
        let (min, max) = (c.world.min_y()?, c.world.max_y()?);
        let (mut below, mut above) = (true, true);
        let mut distance = 1;
        while below || above {
            if below {
                let p = [px, wy - distance, pz];
                let s = c.world.block(p)?;
                let name = c.name(s).to_owned();
                let empty = replaceable(&name) && name != "minecraft:lava";
                if !empty && c.world.face_sturdy(p, Direction::Up)? {
                    let log = c.state(self.wood.log);
                    for py in wy - distance + 1..wy {
                        c.world.set([px, py, pz], log, 2)?;
                    }
                    return Ok(());
                }
                below = distance <= 20 && empty && p[1] > min + 1;
            }
            if above {
                let p = [px, wy + distance, pz];
                let s = c.world.block(p)?;
                let name = c.name(s).to_owned();
                let empty = replaceable(&name);
                if !empty && c.world.center_sturdy(p, Direction::Down)? && !falling(&name) {
                    c.world.set([px, wy + 1, pz], c.state(self.wood.fence), 2)?;
                    let chain = c.state("iron_chain");
                    for py in wy + 2..wy + distance {
                        c.world.set([px, py, pz], chain, 2)?;
                    }
                    return Ok(());
                }
                above = distance <= 50 && empty && p[1] < max;
            }
            distance += 1;
        }
        Ok(())
    }

    /// `MineshaftPieces.MineShaftPiece(StructurePieceType, CompoundTag)` and
    /// the subclasses' tag constructors, which only read their own fields: the
    /// box is rebuilt by `saved_legacy::read_frame`.
    pub(crate) fn from_saved(
        id: &str,
        tag: &Tag,
        frame: PieceFrame,
        depth: i32,
        blocking: Arc<BTreeSet<BiomeId>>,
    ) -> Option<Self> {
        // MineshaftStructure.Type.byId: everything but MESA (1) is NORMAL.
        let wood = if super::saved_legacy::int(tag, "MST") == 1 {
            MESA
        } else {
            NORMAL
        };
        let int = |key| super::saved_legacy::int(tag, key);
        let flag = |key| super::saved_legacy::flag(tag, key);
        let kind = match id {
            "msroom" => {
                // A failed codec read defaults to no entrances, as does a missing tag.
                let entrances = tag
                    .get("Entrances")
                    .and_then(|tag| match tag {
                        Tag::List(items) => items
                            .iter()
                            .map(super::saved_legacy::read_box)
                            .collect::<Option<Vec<_>>>(),
                        _ => None,
                    })
                    .unwrap_or_default();
                Kind::Room(entrances)
            }
            "mscorridor" => Kind::Corridor {
                rails: flag("hr"),
                spider: flag("sc"),
                placed_spider: Mutex::new(flag("hps")),
                sections: int("Num"),
            },
            "mscrossing" => Kind::Crossing {
                two_floored: flag("tf"),
                // `read("D", LEGACY_ID_CODEC_2D).orElse(SOUTH)`.
                direction: match tag.get("D").and_then(as_number) {
                    Some(n) => super::saved_legacy::direction_2d(i32::from(n.byte_value())),
                    None => Direction::South,
                },
            },
            "msstairs" => Kind::Stairs,
            _ => return None,
        };
        let mut frame = frame;
        frame.keep = wood.keep;
        Some(MineshaftPiece {
            kind,
            frame,
            depth,
            wood,
            blocking,
        })
    }

    /// One piece of each kind and wood, with distinct field values, for the
    /// saved-piece round trip.
    #[cfg(test)]
    pub(crate) fn saved_test_pieces(blocking: Arc<BTreeSet<BiomeId>>) -> Vec<Box<dyn LegacyPiece>> {
        let mut out = Vec::new();
        for (i, wood) in [NORMAL, MESA].into_iter().enumerate() {
            let kinds = [
                Kind::Room(vec![
                    BoundingBox::new([1, 2, 3], [4, 5, 6]),
                    BoundingBox::new([-1, -2, -3], [0, 0, 0]),
                ]),
                Kind::Corridor {
                    rails: i == 0,
                    spider: i == 1,
                    sections: 3 + i as i32,
                    placed_spider: Mutex::new(i == 1),
                },
                Kind::Crossing {
                    direction: [Direction::North, Direction::East][i],
                    two_floored: i == 1,
                },
                Kind::Stairs,
            ];
            for (j, kind) in kinds.into_iter().enumerate() {
                let orientation = (j != 0).then(|| [Direction::South, Direction::West][j % 2]);
                let mut frame = PieceFrame::new(
                    BoundingBox::new([10 * i as i32 + j as i32, 50, -20], [13, 53, -15]),
                    orientation,
                );
                frame.keep = wood.keep;
                let piece: Box<dyn LegacyPiece> = Box::new(MineshaftPiece {
                    kind,
                    frame,
                    depth: j as i32,
                    wood,
                    blocking: blocking.clone(),
                });
                out.push(piece);
            }
        }
        out
    }
}

impl LegacyPiece for MineshaftPiece {
    fn bounding_box(&self) -> BoundingBox {
        self.frame.bounding_box
    }
    fn save(&self) -> Option<crate::nbt::Compound> {
        use crate::nbt::Tag;
        let flag = |b: bool| Tag::Byte(i8::from(b));
        let mut tag = super::legacy::piece_tag(
            self.type_name(),
            self.frame.bounding_box,
            self.frame.orientation,
            self.depth,
        );
        // MineshaftStructure.Type's ordinal: NORMAL, MESA.
        let mesa = self.wood.log == MESA.log;
        let mut entries = vec![("MST", Tag::Int(i32::from(mesa)))];
        match &self.kind {
            Kind::Room(entrances) => entries.push((
                "Entrances",
                Tag::List(
                    entrances
                        .iter()
                        .map(|b| {
                            Tag::IntArray(vec![
                                b.min[0], b.min[1], b.min[2], b.max[0], b.max[1], b.max[2],
                            ])
                        })
                        .collect(),
                ),
            )),
            Kind::Corridor {
                rails,
                spider,
                sections,
                placed_spider,
            } => entries.extend([
                ("hr", flag(*rails)),
                ("sc", flag(*spider)),
                ("hps", flag(*placed_spider.lock().unwrap())),
                ("Num", Tag::Int(*sections)),
            ]),
            Kind::Crossing {
                direction,
                two_floored,
            } => {
                let d = crate::level::data_2d(*direction) as i8;
                entries.extend([("tf", flag(*two_floored)), ("D", Tag::Byte(d))]);
            }
            Kind::Stairs => {}
        }
        for (key, value) in entries {
            tag.insert(key.into(), value);
        }
        Some(tag)
    }
    fn type_name(&self) -> &'static str {
        match self.kind {
            Kind::Room(_) => "minecraft:msroom",
            Kind::Corridor { .. } => "minecraft:mscorridor",
            Kind::Crossing { .. } => "minecraft:mscrossing",
            Kind::Stairs => "minecraft:msstairs",
        }
    }
    fn post_process(&self, c: &mut PieceContext<'_>) -> Result<()> {
        if self.invalid(c)? {
            return Ok(());
        }
        match &self.kind {
            Kind::Room(entrances) => self.room(c, entrances),
            Kind::Crossing { two_floored, .. } => self.crossing(c, *two_floored),
            Kind::Stairs => self.stairs(c),
            Kind::Corridor {
                rails,
                spider,
                sections,
                placed_spider,
            } => self.corridor(c, *rails, *spider, *sections, placed_spider),
        }
    }
}
