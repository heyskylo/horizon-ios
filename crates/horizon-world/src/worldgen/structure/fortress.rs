//! Nether fortresses: NetherFortressStructure and NetherFortressPieces.
use super::{
    BoundingBox,
    legacy::{LegacyPiece, PieceContext, PieceFrame, Result, make_bounding_box, random_horizontal},
    stronghold::orient_box,
};
use crate::nbt::Tag;
use crate::registry::BlockStateId;
use horizon_core::{
    geometry::Direction,
    random::{LegacyRandom, RandomSource},
};
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    BridgeStraight,
    BridgeCrossing,
    RoomCrossing,
    StairsRoom,
    MonsterThrone,
    CastleEntrance,
    SmallCorridor,
    SmallCorridorCrossing,
    SmallCorridorRightTurn,
    SmallCorridorLeftTurn,
    CorridorStairs,
    CorridorTBalcony,
    StalkRoom,
    BridgeEndFiller,
    Start,
}
/// PieceWeight: kind, weight, max place count, allow in row.
type Weight = (Kind, i32, i32, bool);
const BRIDGE_WEIGHTS: [Weight; 6] = [
    (Kind::BridgeStraight, 30, 0, true),
    (Kind::BridgeCrossing, 10, 4, false),
    (Kind::RoomCrossing, 10, 4, false),
    (Kind::StairsRoom, 10, 3, false),
    (Kind::MonsterThrone, 5, 2, false),
    (Kind::CastleEntrance, 5, 1, false),
];
const CASTLE_WEIGHTS: [Weight; 7] = [
    (Kind::SmallCorridor, 25, 0, true),
    (Kind::SmallCorridorCrossing, 15, 5, false),
    (Kind::SmallCorridorRightTurn, 5, 10, false),
    (Kind::SmallCorridorLeftTurn, 5, 10, false),
    (Kind::CorridorStairs, 10, 3, true),
    (Kind::CorridorTBalcony, 7, 2, false),
    (Kind::StalkRoom, 5, 2, false),
];

#[derive(Debug)]
pub struct FortressPiece {
    kind: Kind,
    frame: PieceFrame,
    depth: i32,
    /// BridgeEndFiller.selfSeed.
    seed: i32,
    /// Corridor turns: isNeedingChest; MonsterThrone: hasPlacedSpawner.
    flag: Mutex<bool>,
}

fn int(r: &mut impl RandomSource, bound: i32) -> i32 {
    r.next_int_bound(bound).unwrap_or(0)
}

struct Builder {
    pieces: Vec<FortressPiece>,
    /// (weight, place count) for the bridge and castle lists.
    bridge: Vec<(Weight, i32)>,
    castle: Vec<(Weight, i32)>,
    previous: Option<Kind>,
    pending: Vec<usize>,
}
impl Builder {
    fn collides(&self, b: &BoundingBox) -> bool {
        self.pieces
            .iter()
            .any(|p| p.frame.bounding_box.intersects(b))
    }
    fn piece(kind: Kind, b: BoundingBox, d: Direction, depth: i32) -> FortressPiece {
        FortressPiece {
            kind,
            frame: PieceFrame::new(b, Some(d)),
            depth,
            seed: 0,
            flag: Mutex::new(false),
        }
    }
    /// findAndCreateBridgePieceFactory / BridgeEndFiller.createPiece.
    fn create(
        &self,
        kind: Kind,
        r: &mut impl RandomSource,
        foot: [i32; 3],
        d: Direction,
        depth: i32,
    ) -> Option<FortressPiece> {
        let (offset, size) = match kind {
            Kind::BridgeStraight => ([-1, -3, 0], [5, 10, 19]),
            Kind::BridgeCrossing => ([-8, -3, 0], [19, 10, 19]),
            Kind::RoomCrossing => ([-2, 0, 0], [7, 9, 7]),
            Kind::StairsRoom => ([-2, 0, 0], [7, 11, 7]),
            Kind::MonsterThrone => ([-2, 0, 0], [7, 8, 9]),
            Kind::CastleEntrance | Kind::StalkRoom => ([-5, -3, 0], [13, 14, 13]),
            Kind::SmallCorridor
            | Kind::SmallCorridorCrossing
            | Kind::SmallCorridorRightTurn
            | Kind::SmallCorridorLeftTurn => ([-1, 0, 0], [5, 7, 5]),
            Kind::CorridorStairs => ([-1, -7, 0], [5, 14, 10]),
            Kind::CorridorTBalcony => ([-3, 0, 0], [9, 7, 9]),
            Kind::BridgeEndFiller => ([-1, -3, 0], [5, 10, 8]),
            Kind::Start => return None,
        };
        let b = orient_box(foot, offset, size, d);
        if b.min[1] <= 10 || self.collides(&b) {
            return None;
        }
        let mut piece = Self::piece(kind, b, d, depth);
        match kind {
            Kind::BridgeEndFiller => piece.seed = r.next_int(),
            Kind::SmallCorridorRightTurn | Kind::SmallCorridorLeftTurn => {
                piece.flag = Mutex::new(int(r, 3) == 0)
            }
            _ => {}
        }
        Some(piece)
    }
    /// NetherBridgePiece.generatePiece.
    #[allow(clippy::too_many_arguments)]
    fn generate_piece(
        &mut self,
        castle: bool,
        r: &mut impl RandomSource,
        foot: [i32; 3],
        d: Direction,
        depth: i32,
    ) -> Option<FortressPiece> {
        let list = if castle { &self.castle } else { &self.bridge };
        let any = list.iter().any(|(w, n)| w.2 > 0 && *n < w.2);
        let total: i32 = list.iter().map(|(w, _)| w.1).sum();
        let total = if any { total } else { -1 };
        if total > 0 && depth <= 30 {
            for _ in 0..5 {
                let mut selection = int(r, total);
                let len = if castle {
                    self.castle.len()
                } else {
                    self.bridge.len()
                };
                for i in 0..len {
                    let ((kind, weight, max, allow_in_row), count) = if castle {
                        self.castle[i]
                    } else {
                        self.bridge[i]
                    };
                    selection -= weight;
                    if selection < 0 {
                        let place = max == 0 || count < max;
                        if !place || (self.previous == Some(kind) && !allow_in_row) {
                            break;
                        }
                        if let Some(piece) = self.create(kind, r, foot, d, depth) {
                            let list = if castle {
                                &mut self.castle
                            } else {
                                &mut self.bridge
                            };
                            list[i].1 += 1;
                            self.previous = Some(kind);
                            if max != 0 && count + 1 >= max {
                                list.remove(i);
                            }
                            return Some(piece);
                        }
                    }
                }
            }
        }
        self.create(Kind::BridgeEndFiller, r, foot, d, depth)
    }
    /// NetherBridgePiece.generateAndAddPiece.
    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        r: &mut impl RandomSource,
        foot: [i32; 3],
        d: Direction,
        depth: i32,
        castle: bool,
    ) {
        let start = self.pieces[0].frame.bounding_box;
        if (foot[0] - start.min[0]).abs() <= 112 && (foot[2] - start.min[2]).abs() <= 112 {
            if let Some(piece) = self.generate_piece(castle, r, foot, d, depth + 1) {
                self.pieces.push(piece);
                self.pending.push(self.pieces.len() - 1);
            }
        } else {
            // The end filler is created (drawing its seed) but never added.
            let _ = self.create(Kind::BridgeEndFiller, r, foot, d, depth);
        }
    }
    fn forward(
        &mut self,
        r: &mut impl RandomSource,
        i: usize,
        x_off: i32,
        y_off: i32,
        castle: bool,
    ) {
        let p = &self.pieces[i];
        let (b, depth) = (p.frame.bounding_box, p.depth);
        let Some(o) = p.frame.orientation else { return };
        let y = b.min[1] + y_off;
        let foot = match o {
            Direction::North => [b.min[0] + x_off, y, b.min[2] - 1],
            Direction::South => [b.min[0] + x_off, y, b.max[2] + 1],
            Direction::West => [b.min[0] - 1, y, b.min[2] + x_off],
            Direction::East => [b.max[0] + 1, y, b.min[2] + x_off],
            _ => return,
        };
        self.add(r, foot, o, depth, castle);
    }
    #[allow(clippy::too_many_arguments)]
    fn side(
        &mut self,
        r: &mut impl RandomSource,
        i: usize,
        right: bool,
        y_off: i32,
        z_off: i32,
        castle: bool,
    ) {
        let p = &self.pieces[i];
        let (b, depth) = (p.frame.bounding_box, p.depth);
        let Some(o) = p.frame.orientation else { return };
        let y = b.min[1] + y_off;
        let (foot, d) = match (o, right) {
            (Direction::North | Direction::South, false) => {
                ([b.min[0] - 1, y, b.min[2] + z_off], Direction::West)
            }
            (Direction::North | Direction::South, true) => {
                ([b.max[0] + 1, y, b.min[2] + z_off], Direction::East)
            }
            (Direction::West | Direction::East, false) => {
                ([b.min[0] + z_off, y, b.min[2] - 1], Direction::North)
            }
            (Direction::West | Direction::East, true) => {
                ([b.min[0] + z_off, y, b.max[2] + 1], Direction::South)
            }
            _ => return,
        };
        self.add(r, foot, d, depth, castle);
    }
    fn add_children(&mut self, r: &mut impl RandomSource, i: usize) {
        let p = &self.pieces[i];
        let (kind, o) = (p.kind, p.frame.orientation);
        match kind {
            Kind::Start | Kind::BridgeCrossing => {
                self.forward(r, i, 8, 3, false);
                self.side(r, i, false, 3, 8, false);
                self.side(r, i, true, 3, 8, false);
            }
            Kind::BridgeStraight => self.forward(r, i, 1, 3, false),
            Kind::CorridorStairs | Kind::SmallCorridor => self.forward(r, i, 1, 0, true),
            Kind::CorridorTBalcony => {
                let z_off = if matches!(o, Some(Direction::West | Direction::North)) {
                    5
                } else {
                    1
                };
                let castle = int(r, 8) > 0;
                self.side(r, i, false, 0, z_off, castle);
                let castle = int(r, 8) > 0;
                self.side(r, i, true, 0, z_off, castle);
            }
            Kind::CastleEntrance => self.forward(r, i, 5, 3, true),
            Kind::SmallCorridorCrossing => {
                self.forward(r, i, 1, 0, true);
                self.side(r, i, false, 0, 1, true);
                self.side(r, i, true, 0, 1, true);
            }
            Kind::SmallCorridorLeftTurn => self.side(r, i, false, 0, 1, true),
            Kind::SmallCorridorRightTurn => self.side(r, i, true, 0, 1, true),
            Kind::StalkRoom => {
                self.forward(r, i, 5, 3, true);
                self.forward(r, i, 5, 11, true);
            }
            Kind::RoomCrossing => {
                self.forward(r, i, 2, 0, false);
                self.side(r, i, false, 0, 2, false);
                self.side(r, i, true, 0, 2, false);
            }
            Kind::StairsRoom => self.side(r, i, true, 6, 2, false),
            Kind::MonsterThrone | Kind::BridgeEndFiller => {}
        }
    }
}

/// NetherFortressStructure.generatePieces.
pub fn generate(random: &mut impl RandomSource, chunk: [i32; 2]) -> Vec<FortressPiece> {
    let d = random_horizontal(random);
    let b = make_bounding_box(chunk[0] * 16 + 2, 64, chunk[1] * 16 + 2, d, 19, 10, 19);
    let mut g = Builder {
        pieces: vec![Builder::piece(Kind::Start, b, d, 0)],
        bridge: BRIDGE_WEIGHTS.iter().map(|w| (*w, 0)).collect(),
        castle: CASTLE_WEIGHTS.iter().map(|w| (*w, 0)).collect(),
        previous: None,
        pending: Vec::new(),
    };
    g.add_children(random, 0);
    while !g.pending.is_empty() {
        let index = int(random, g.pending.len() as i32) as usize;
        let next = g.pending.remove(index);
        g.add_children(random, next);
    }
    // StructurePiecesBuilder.moveInsideHeights(random, 48, 70).
    let mut all = g.pieces[0].frame.bounding_box;
    for p in &g.pieces[1..] {
        all.encapsulate(&p.frame.bounding_box);
    }
    let span = 70 - 48 + 1 - all.span()[1];
    let y = if span > 1 { 48 + int(random, span) } else { 48 };
    let dy = y - all.min[1];
    for p in &mut g.pieces {
        p.frame.bounding_box = p.frame.bounding_box.moved([0, dy, 0]);
    }
    g.pieces
}

/// Block states used by the fortress pieces.
struct Palette {
    bricks: BlockStateId,
    air: BlockStateId,
    fence: BlockStateId,
    ns: BlockStateId,
    we: BlockStateId,
    nse: BlockStateId,
    nsw: BlockStateId,
}
fn fence(c: &PieceContext<'_>, sides: &[&str]) -> BlockStateId {
    let props: Vec<(&str, &str)> = sides.iter().map(|s| (*s, "true")).collect();
    c.state_with("nether_brick_fence", &props)
}
impl Palette {
    fn new(c: &PieceContext<'_>) -> Self {
        Self {
            bricks: c.state("nether_bricks"),
            air: c.state("air"),
            fence: c.state("nether_brick_fence"),
            ns: fence(c, &["north", "south"]),
            we: fence(c, &["west", "east"]),
            nse: fence(c, &["north", "south", "east"]),
            nsw: fence(c, &["north", "south", "west"]),
        }
    }
}

impl FortressPiece {
    fn bx(
        &self,
        c: &mut PieceContext<'_>,
        [x0, y0, z0, x1, y1, z1]: [i32; 6],
        s: BlockStateId,
    ) -> Result<()> {
        self.frame
            .generate_box(c, [x0, y0, z0], [x1, y1, z1], s, s, false)
    }
    fn put(&self, c: &mut PieceContext<'_>, s: BlockStateId, x: i32, y: i32, z: i32) -> Result<()> {
        self.frame.place(c, s, x, y, z)
    }
    fn column(&self, c: &mut PieceContext<'_>, p: &Palette, x: i32, z: i32) -> Result<()> {
        self.frame.fill_column_down(c, p.bricks, x, -1, z)
    }
    fn columns(
        &self,
        c: &mut PieceContext<'_>,
        p: &Palette,
        xs: [i32; 2],
        zs: [i32; 2],
    ) -> Result<()> {
        for x in xs[0]..=xs[1] {
            for z in zs[0]..=zs[1] {
                self.column(c, p, x, z)?;
            }
        }
        Ok(())
    }
    fn bridge_crossing(&self, c: &mut PieceContext<'_>, p: &Palette) -> Result<()> {
        let b = p.bricks;
        self.bx(c, [7, 3, 0, 11, 4, 18], b)?;
        self.bx(c, [0, 3, 7, 18, 4, 11], b)?;
        self.bx(c, [8, 5, 0, 10, 7, 18], p.air)?;
        self.bx(c, [0, 5, 8, 18, 7, 10], p.air)?;
        for r in [
            [7, 5, 0, 7, 5, 7],
            [7, 5, 11, 7, 5, 18],
            [11, 5, 0, 11, 5, 7],
            [11, 5, 11, 11, 5, 18],
            [0, 5, 7, 7, 5, 7],
            [11, 5, 7, 18, 5, 7],
            [0, 5, 11, 7, 5, 11],
            [11, 5, 11, 18, 5, 11],
            [7, 2, 0, 11, 2, 5],
            [7, 2, 13, 11, 2, 18],
            [7, 0, 0, 11, 1, 3],
            [7, 0, 15, 11, 1, 18],
        ] {
            self.bx(c, r, b)?;
        }
        for x in 7..=11 {
            for z in 0..=2 {
                self.column(c, p, x, z)?;
                self.column(c, p, x, 18 - z)?;
            }
        }
        for r in [
            [0, 2, 7, 5, 2, 11],
            [13, 2, 7, 18, 2, 11],
            [0, 0, 7, 3, 1, 11],
            [15, 0, 7, 18, 1, 11],
        ] {
            self.bx(c, r, b)?;
        }
        for x in 0..=2 {
            for z in 7..=11 {
                self.column(c, p, x, z)?;
                self.column(c, p, 18 - x, z)?;
            }
        }
        Ok(())
    }
    fn end_filler(&self, c: &mut PieceContext<'_>, p: &Palette) -> Result<()> {
        let mut r = LegacyRandom::new(i64::from(self.seed));
        let b = p.bricks;
        for x in 0..=4 {
            for y in 3..=4 {
                let z = int(&mut r, 8);
                self.bx(c, [x, y, 0, x, y, z], b)?;
            }
        }
        let z = int(&mut r, 8);
        self.bx(c, [0, 5, 0, 0, 5, z], b)?;
        let z = int(&mut r, 8);
        self.bx(c, [4, 5, 0, 4, 5, z], b)?;
        for x in 0..=4 {
            let z = int(&mut r, 5);
            self.bx(c, [x, 2, 0, x, 2, z], b)?;
        }
        for x in 0..=4 {
            for y in 0..=1 {
                let z = int(&mut r, 3);
                self.bx(c, [x, y, 0, x, y, z], b)?;
            }
        }
        Ok(())
    }
    fn bridge_straight(&self, c: &mut PieceContext<'_>, p: &Palette) -> Result<()> {
        let b = p.bricks;
        self.bx(c, [0, 3, 0, 4, 4, 18], b)?;
        self.bx(c, [1, 5, 0, 3, 7, 18], p.air)?;
        for r in [
            [0, 5, 0, 0, 5, 18],
            [4, 5, 0, 4, 5, 18],
            [0, 2, 0, 4, 2, 5],
            [0, 2, 13, 4, 2, 18],
            [0, 0, 0, 4, 1, 3],
            [0, 0, 15, 4, 1, 18],
        ] {
            self.bx(c, r, b)?;
        }
        for x in 0..=4 {
            for z in 0..=2 {
                self.column(c, p, x, z)?;
                self.column(c, p, x, 18 - z)?;
            }
        }
        for r in [
            [0, 1, 1, 0, 4, 1],
            [0, 3, 4, 0, 4, 4],
            [0, 3, 14, 0, 4, 14],
            [0, 1, 17, 0, 4, 17],
        ] {
            self.bx(c, r, p.nse)?;
        }
        for r in [
            [4, 1, 1, 4, 4, 1],
            [4, 3, 4, 4, 4, 4],
            [4, 3, 14, 4, 4, 14],
            [4, 1, 17, 4, 4, 17],
        ] {
            self.bx(c, r, p.nsw)?;
        }
        Ok(())
    }
    fn corridor_stairs(&self, c: &mut PieceContext<'_>, p: &Palette) -> Result<()> {
        let stairs = c.state_with("nether_brick_stairs", &[("facing", "south")]);
        let b = p.bricks;
        for step in 0..=9 {
            let floor = (7 - step).max(1);
            let roof = (floor + 5).max(14 - step).min(13);
            let z = step;
            self.bx(c, [0, 0, z, 4, floor, z], b)?;
            self.bx(c, [1, floor + 1, z, 3, roof - 1, z], p.air)?;
            if step <= 6 {
                for x in 1..=3 {
                    self.put(c, stairs, x, floor + 1, z)?;
                }
            }
            self.bx(c, [0, roof, z, 4, roof, z], b)?;
            self.bx(c, [0, floor + 1, z, 0, roof - 1, z], b)?;
            self.bx(c, [4, floor + 1, z, 4, roof - 1, z], b)?;
            if step & 1 == 0 {
                self.bx(c, [0, floor + 2, z, 0, floor + 3, z], p.ns)?;
                self.bx(c, [4, floor + 2, z, 4, floor + 3, z], p.ns)?;
            }
            for x in 0..=4 {
                self.column(c, p, x, z)?;
            }
        }
        Ok(())
    }
    fn t_balcony(&self, c: &mut PieceContext<'_>, p: &Palette) -> Result<()> {
        let b = p.bricks;
        self.bx(c, [0, 0, 0, 8, 1, 8], b)?;
        self.bx(c, [0, 2, 0, 8, 5, 8], p.air)?;
        self.bx(c, [0, 6, 0, 8, 6, 5], b)?;
        self.bx(c, [0, 2, 0, 2, 5, 0], b)?;
        self.bx(c, [6, 2, 0, 8, 5, 0], b)?;
        self.bx(c, [1, 3, 0, 1, 4, 0], p.we)?;
        self.bx(c, [7, 3, 0, 7, 4, 0], p.we)?;
        self.bx(c, [0, 2, 4, 8, 2, 8], b)?;
        self.bx(c, [1, 1, 4, 2, 2, 4], p.air)?;
        self.bx(c, [6, 1, 4, 7, 2, 4], p.air)?;
        self.bx(c, [1, 3, 8, 7, 3, 8], p.we)?;
        self.put(c, fence(c, &["east", "south"]), 0, 3, 8)?;
        self.put(c, fence(c, &["west", "south"]), 8, 3, 8)?;
        self.bx(c, [0, 3, 6, 0, 3, 7], p.ns)?;
        self.bx(c, [8, 3, 6, 8, 3, 7], p.ns)?;
        self.bx(c, [0, 3, 4, 0, 5, 5], b)?;
        self.bx(c, [8, 3, 4, 8, 5, 5], b)?;
        self.bx(c, [1, 3, 5, 2, 5, 5], b)?;
        self.bx(c, [6, 3, 5, 7, 5, 5], b)?;
        self.bx(c, [1, 4, 5, 1, 5, 5], p.we)?;
        self.bx(c, [7, 4, 5, 7, 5, 5], p.we)?;
        for z in 0..=5 {
            for x in 0..=8 {
                self.column(c, p, x, z)?;
            }
        }
        Ok(())
    }
    /// The walls, roof and battlements shared by CastleEntrance and CastleStalkRoom.
    fn castle_hall(&self, c: &mut PieceContext<'_>, p: &Palette) -> Result<()> {
        let b = p.bricks;
        self.bx(c, [0, 3, 0, 12, 4, 12], b)?;
        self.bx(c, [0, 5, 0, 12, 13, 12], p.air)?;
        for r in [
            [0, 5, 0, 1, 12, 12],
            [11, 5, 0, 12, 12, 12],
            [2, 5, 11, 4, 12, 12],
            [8, 5, 11, 10, 12, 12],
            [5, 9, 11, 7, 12, 12],
            [2, 5, 0, 4, 12, 1],
            [8, 5, 0, 10, 12, 1],
            [5, 9, 0, 7, 12, 1],
            [2, 11, 2, 10, 12, 10],
        ] {
            self.bx(c, r, b)?;
        }
        Ok(())
    }
    fn battlements(&self, c: &mut PieceContext<'_>, p: &Palette) -> Result<()> {
        let b = p.bricks;
        for i in (1..=11).step_by(2) {
            self.bx(c, [i, 10, 0, i, 11, 0], p.we)?;
            self.bx(c, [i, 10, 12, i, 11, 12], p.we)?;
            self.bx(c, [0, 10, i, 0, 11, i], p.ns)?;
            self.bx(c, [12, 10, i, 12, 11, i], p.ns)?;
            self.put(c, b, i, 13, 0)?;
            self.put(c, b, i, 13, 12)?;
            self.put(c, b, 0, 13, i)?;
            self.put(c, b, 12, 13, i)?;
            if i != 11 {
                self.put(c, p.we, i + 1, 13, 0)?;
                self.put(c, p.we, i + 1, 13, 12)?;
                self.put(c, p.ns, 0, 13, i + 1)?;
                self.put(c, p.ns, 12, 13, i + 1)?;
            }
        }
        self.put(c, fence(c, &["north", "east"]), 0, 13, 0)?;
        self.put(c, fence(c, &["south", "east"]), 0, 13, 12)?;
        self.put(c, fence(c, &["south", "west"]), 12, 13, 12)?;
        self.put(c, fence(c, &["north", "west"]), 12, 13, 0)?;
        Ok(())
    }
    /// The floor and supports shared by CastleEntrance and CastleStalkRoom.
    fn castle_floor(&self, c: &mut PieceContext<'_>, p: &Palette) -> Result<()> {
        let b = p.bricks;
        for r in [
            [4, 2, 0, 8, 2, 12],
            [0, 2, 4, 12, 2, 8],
            [4, 0, 0, 8, 1, 3],
            [4, 0, 9, 8, 1, 12],
            [0, 0, 4, 3, 1, 8],
            [9, 0, 4, 12, 1, 8],
        ] {
            self.bx(c, r, b)?;
        }
        for x in 4..=8 {
            for z in 0..=2 {
                self.column(c, p, x, z)?;
                self.column(c, p, x, 12 - z)?;
            }
        }
        for x in 0..=2 {
            for z in 4..=8 {
                self.column(c, p, x, z)?;
                self.column(c, p, 12 - x, z)?;
            }
        }
        Ok(())
    }
    fn castle_entrance(&self, c: &mut PieceContext<'_>, p: &Palette) -> Result<()> {
        self.castle_hall(c, p)?;
        self.bx(c, [5, 8, 0, 7, 8, 0], p.fence)?;
        self.battlements(c, p)?;
        for z in (3..=9).step_by(2) {
            self.bx(c, [1, 7, z, 1, 8, z], p.nsw)?;
            self.bx(c, [11, 7, z, 11, 8, z], p.nse)?;
        }
        self.castle_floor(c, p)?;
        self.bx(c, [5, 5, 5, 7, 5, 7], p.bricks)?;
        self.bx(c, [6, 1, 6, 6, 4, 6], p.air)?;
        self.put(c, p.bricks, 6, 0, 6)?;
        // The lava source's scheduled tick has no generation-time effect.
        self.put(c, c.state("lava"), 6, 5, 6)
    }
    fn stalk_room(&self, c: &mut PieceContext<'_>, p: &Palette) -> Result<()> {
        let b = p.bricks;
        self.castle_hall(c, p)?;
        self.battlements(c, p)?;
        for z in (3..=9).step_by(2) {
            self.bx(c, [1, 7, z, 1, 8, z], p.nsw)?;
            self.bx(c, [11, 7, z, 11, 8, z], p.nse)?;
        }
        let stairs = c.state_with("nether_brick_stairs", &[("facing", "north")]);
        for i in 0..=6 {
            let z = i + 4;
            for x in 5..=7 {
                self.put(c, stairs, x, 5 + i, z)?;
            }
            if (5..=8).contains(&z) {
                self.bx(c, [5, 5, z, 7, i + 4, z], b)?;
            } else if (9..=10).contains(&z) {
                self.bx(c, [5, 8, z, 7, i + 4, z], b)?;
            }
            if i >= 1 {
                self.bx(c, [5, 6 + i, z, 7, 9 + i, z], p.air)?;
            }
        }
        for x in 5..=7 {
            self.put(c, stairs, x, 12, 11)?;
        }
        self.bx(c, [5, 6, 7, 5, 7, 7], p.nse)?;
        self.bx(c, [7, 6, 7, 7, 7, 7], p.nsw)?;
        self.bx(c, [5, 13, 12, 7, 13, 12], p.air)?;
        for r in [
            [2, 5, 2, 3, 5, 3],
            [2, 5, 9, 3, 5, 10],
            [2, 5, 4, 2, 5, 8],
            [9, 5, 2, 10, 5, 3],
            [9, 5, 9, 10, 5, 10],
            [10, 5, 4, 10, 5, 8],
        ] {
            self.bx(c, r, b)?;
        }
        let east = c.state_with("nether_brick_stairs", &[("facing", "east")]);
        let west = c.state_with("nether_brick_stairs", &[("facing", "west")]);
        for z in [2, 3, 9, 10] {
            self.put(c, west, 4, 5, z)?;
        }
        for z in [2, 3, 9, 10] {
            self.put(c, east, 8, 5, z)?;
        }
        let soul_sand = c.state("soul_sand");
        let wart = c.state("nether_wart");
        self.bx(c, [3, 4, 4, 4, 4, 8], soul_sand)?;
        self.bx(c, [8, 4, 4, 9, 4, 8], soul_sand)?;
        self.bx(c, [3, 5, 4, 4, 5, 8], wart)?;
        self.bx(c, [8, 5, 4, 9, 5, 8], wart)?;
        self.castle_floor(c, p)
    }
    fn corridor_floor(&self, c: &mut PieceContext<'_>, p: &Palette) -> Result<()> {
        self.bx(c, [0, 0, 0, 4, 1, 4], p.bricks)?;
        self.bx(c, [0, 2, 0, 4, 5, 4], p.air)
    }
    fn corridor_roof(&self, c: &mut PieceContext<'_>, p: &Palette) -> Result<()> {
        self.bx(c, [0, 6, 0, 4, 6, 4], p.bricks)?;
        self.columns(c, p, [0, 4], [0, 4])
    }
    fn corridor_crossing(&self, c: &mut PieceContext<'_>, p: &Palette) -> Result<()> {
        self.corridor_floor(c, p)?;
        for r in [
            [0, 2, 0, 0, 5, 0],
            [4, 2, 0, 4, 5, 0],
            [0, 2, 4, 0, 5, 4],
            [4, 2, 4, 4, 5, 4],
        ] {
            self.bx(c, r, p.bricks)?;
        }
        self.corridor_roof(c, p)
    }
    fn corridor_chest(&self, c: &mut PieceContext<'_>, [x, y, z]: [i32; 3]) -> Result<()> {
        let mut needed = self.flag.lock().unwrap();
        if *needed && c.chunk_box.is_inside(self.frame.world_pos(x, y, z)) {
            *needed = false;
            self.frame
                .create_chest(c, x, y, z, "minecraft:chests/nether_bridge")?;
        }
        Ok(())
    }
    fn corridor_left(&self, c: &mut PieceContext<'_>, p: &Palette) -> Result<()> {
        self.corridor_floor(c, p)?;
        let b = p.bricks;
        self.bx(c, [4, 2, 0, 4, 5, 4], b)?;
        self.bx(c, [4, 3, 1, 4, 4, 1], p.ns)?;
        self.bx(c, [4, 3, 3, 4, 4, 3], p.ns)?;
        self.bx(c, [0, 2, 0, 0, 5, 0], b)?;
        self.bx(c, [0, 2, 4, 3, 5, 4], b)?;
        self.bx(c, [1, 3, 4, 1, 4, 4], p.we)?;
        self.bx(c, [3, 3, 4, 3, 4, 4], p.we)?;
        self.corridor_chest(c, [3, 2, 3])?;
        self.corridor_roof(c, p)
    }
    fn corridor(&self, c: &mut PieceContext<'_>, p: &Palette) -> Result<()> {
        self.corridor_floor(c, p)?;
        let b = p.bricks;
        self.bx(c, [0, 2, 0, 0, 5, 4], b)?;
        self.bx(c, [4, 2, 0, 4, 5, 4], b)?;
        for r in [
            [0, 3, 1, 0, 4, 1],
            [0, 3, 3, 0, 4, 3],
            [4, 3, 1, 4, 4, 1],
            [4, 3, 3, 4, 4, 3],
        ] {
            self.bx(c, r, p.ns)?;
        }
        self.corridor_roof(c, p)
    }
    fn corridor_right(&self, c: &mut PieceContext<'_>, p: &Palette) -> Result<()> {
        self.corridor_floor(c, p)?;
        let b = p.bricks;
        self.bx(c, [0, 2, 0, 0, 5, 4], b)?;
        self.bx(c, [0, 3, 1, 0, 4, 1], p.ns)?;
        self.bx(c, [0, 3, 3, 0, 4, 3], p.ns)?;
        self.bx(c, [4, 2, 0, 4, 5, 0], b)?;
        self.bx(c, [1, 2, 4, 4, 5, 4], b)?;
        self.bx(c, [1, 3, 4, 1, 4, 4], p.we)?;
        self.bx(c, [3, 3, 4, 3, 4, 4], p.we)?;
        self.corridor_chest(c, [1, 2, 3])?;
        self.corridor_roof(c, p)
    }
    fn monster_throne(&self, c: &mut PieceContext<'_>, p: &Palette) -> Result<()> {
        let b = p.bricks;
        self.bx(c, [0, 2, 0, 6, 7, 7], p.air)?;
        for r in [
            [1, 0, 0, 5, 1, 7],
            [1, 2, 1, 5, 2, 7],
            [1, 3, 2, 5, 3, 7],
            [1, 4, 3, 5, 4, 7],
            [1, 2, 0, 1, 4, 2],
            [5, 2, 0, 5, 4, 2],
            [1, 5, 2, 1, 5, 3],
            [5, 5, 2, 5, 5, 3],
            [0, 5, 3, 0, 5, 8],
            [6, 5, 3, 6, 5, 8],
            [1, 5, 8, 5, 5, 8],
        ] {
            self.bx(c, r, b)?;
        }
        let west = fence(c, &["west"]);
        let east = fence(c, &["east"]);
        self.put(c, west, 1, 6, 3)?;
        self.put(c, east, 5, 6, 3)?;
        self.put(c, fence(c, &["east", "north"]), 0, 6, 3)?;
        self.put(c, fence(c, &["west", "north"]), 6, 6, 3)?;
        self.bx(c, [0, 6, 4, 0, 6, 7], p.ns)?;
        self.bx(c, [6, 6, 4, 6, 6, 7], p.ns)?;
        self.put(c, fence(c, &["east", "south"]), 0, 6, 8)?;
        self.put(c, fence(c, &["west", "south"]), 6, 6, 8)?;
        self.bx(c, [1, 6, 8, 5, 6, 8], p.we)?;
        self.put(c, east, 1, 7, 8)?;
        self.bx(c, [2, 7, 8, 4, 7, 8], p.we)?;
        self.put(c, west, 5, 7, 8)?;
        self.put(c, east, 2, 8, 8)?;
        self.put(c, p.we, 3, 8, 8)?;
        self.put(c, west, 4, 8, 8)?;
        {
            let mut placed = self.flag.lock().unwrap();
            if !*placed {
                let pos = self.frame.world_pos(3, 5, 5);
                if c.chunk_box.is_inside(pos) {
                    *placed = true;
                    c.spawner(pos, "minecraft:blaze")?;
                }
            }
        }
        self.columns(c, p, [0, 6], [0, 6])
    }
    fn room_crossing(&self, c: &mut PieceContext<'_>, p: &Palette) -> Result<()> {
        let b = p.bricks;
        self.bx(c, [0, 0, 0, 6, 1, 6], b)?;
        self.bx(c, [0, 2, 0, 6, 7, 6], p.air)?;
        for r in [
            [0, 2, 0, 1, 6, 0],
            [0, 2, 6, 1, 6, 6],
            [5, 2, 0, 6, 6, 0],
            [5, 2, 6, 6, 6, 6],
            [0, 2, 0, 0, 6, 1],
            [0, 2, 5, 0, 6, 6],
            [6, 2, 0, 6, 6, 1],
            [6, 2, 5, 6, 6, 6],
        ] {
            self.bx(c, r, b)?;
        }
        self.bx(c, [2, 6, 0, 4, 6, 0], b)?;
        self.bx(c, [2, 5, 0, 4, 5, 0], p.we)?;
        self.bx(c, [2, 6, 6, 4, 6, 6], b)?;
        self.bx(c, [2, 5, 6, 4, 5, 6], p.we)?;
        self.bx(c, [0, 6, 2, 0, 6, 4], b)?;
        self.bx(c, [0, 5, 2, 0, 5, 4], p.ns)?;
        self.bx(c, [6, 6, 2, 6, 6, 4], b)?;
        self.bx(c, [6, 5, 2, 6, 5, 4], p.ns)?;
        self.columns(c, p, [0, 6], [0, 6])
    }
    fn stairs_room(&self, c: &mut PieceContext<'_>, p: &Palette) -> Result<()> {
        let b = p.bricks;
        self.bx(c, [0, 0, 0, 6, 1, 6], b)?;
        self.bx(c, [0, 2, 0, 6, 10, 6], p.air)?;
        for r in [
            [0, 2, 0, 1, 8, 0],
            [5, 2, 0, 6, 8, 0],
            [0, 2, 1, 0, 8, 6],
            [6, 2, 1, 6, 8, 6],
            [1, 2, 6, 5, 8, 6],
        ] {
            self.bx(c, r, b)?;
        }
        self.bx(c, [0, 3, 2, 0, 5, 4], p.ns)?;
        self.bx(c, [6, 3, 2, 6, 5, 2], p.ns)?;
        self.bx(c, [6, 3, 4, 6, 5, 4], p.ns)?;
        self.put(c, b, 5, 2, 5)?;
        for r in [
            [4, 2, 5, 4, 3, 5],
            [3, 2, 5, 3, 4, 5],
            [2, 2, 5, 2, 5, 5],
            [1, 2, 5, 1, 6, 5],
            [1, 7, 1, 5, 7, 4],
        ] {
            self.bx(c, r, b)?;
        }
        self.bx(c, [6, 8, 2, 6, 8, 4], p.air)?;
        self.bx(c, [2, 6, 0, 4, 8, 0], b)?;
        self.bx(c, [2, 5, 0, 4, 5, 0], p.we)?;
        self.columns(c, p, [0, 6], [0, 6])
    }

    /// `NetherFortressPieces.NetherBridgePiece(StructurePieceType, CompoundTag)`
    /// and the subclasses' tag constructors. `nebcr` is what vanilla saves a
    /// generated start as (its `StartPiece` is built through the
    /// `BridgeCrossing` constructor) and the registry reads it back as a plain
    /// `BridgeCrossing`; only the tag constructor writes `nestart`, which
    /// Horizon's own writer never does, so a piece read from `nestart` saves
    /// back as `nebcr`.
    pub(crate) fn from_saved(id: &str, tag: &Tag, frame: PieceFrame, depth: i32) -> Option<Self> {
        let kind = match id {
            "nebcr" => Kind::BridgeCrossing,
            "nebef" => Kind::BridgeEndFiller,
            "nebs" => Kind::BridgeStraight,
            "neccs" => Kind::CorridorStairs,
            "nectb" => Kind::CorridorTBalcony,
            "nece" => Kind::CastleEntrance,
            "nescsc" => Kind::SmallCorridorCrossing,
            "nesclt" => Kind::SmallCorridorLeftTurn,
            "nesc" => Kind::SmallCorridor,
            "nescrt" => Kind::SmallCorridorRightTurn,
            "necsr" => Kind::StalkRoom,
            "nemt" => Kind::MonsterThrone,
            "nerc" => Kind::RoomCrossing,
            "nesr" => Kind::StairsRoom,
            "nestart" => Kind::Start,
            _ => return None,
        };
        let int = |key| super::saved_legacy::int(tag, key);
        let flag = |key| super::saved_legacy::flag(tag, key);
        Some(FortressPiece {
            kind,
            frame,
            depth,
            seed: if kind == Kind::BridgeEndFiller {
                int("Seed")
            } else {
                0
            },
            flag: Mutex::new(
                (matches!(
                    kind,
                    Kind::SmallCorridorLeftTurn | Kind::SmallCorridorRightTurn
                ) && flag("Chest"))
                    || (kind == Kind::MonsterThrone && flag("Mob")),
            ),
        })
    }

    /// One piece of each kind, with distinct field values, for the saved-piece
    /// round trip.
    #[cfg(test)]
    pub(crate) fn saved_test_pieces() -> Vec<Box<dyn LegacyPiece>> {
        const KINDS: [Kind; 15] = [
            Kind::BridgeStraight,
            Kind::BridgeCrossing,
            Kind::RoomCrossing,
            Kind::StairsRoom,
            Kind::MonsterThrone,
            Kind::CastleEntrance,
            Kind::SmallCorridor,
            Kind::SmallCorridorCrossing,
            Kind::SmallCorridorRightTurn,
            Kind::SmallCorridorLeftTurn,
            Kind::CorridorStairs,
            Kind::CorridorTBalcony,
            Kind::StalkRoom,
            Kind::BridgeEndFiller,
            Kind::Start,
        ];
        KINDS
            .into_iter()
            .enumerate()
            .map(|(i, kind)| -> Box<dyn LegacyPiece> {
                let i = i as i32;
                let orientation =
                    [Direction::North, Direction::South, Direction::West][i as usize % 3];
                Box::new(FortressPiece {
                    kind,
                    frame: PieceFrame::new(
                        BoundingBox::new([i * 7 - 40, 60 + i, -20], [i * 7 - 30, 70 + i, -5]),
                        Some(orientation),
                    ),
                    depth: 8 - i,
                    seed: if kind == Kind::BridgeEndFiller {
                        -1234567
                    } else {
                        0
                    },
                    // Only the turns and the throne save their flag.
                    flag: Mutex::new(
                        matches!(
                            kind,
                            Kind::SmallCorridorLeftTurn
                                | Kind::SmallCorridorRightTurn
                                | Kind::MonsterThrone
                        ) && i % 2 == 0,
                    ),
                })
            })
            .collect()
    }
}

impl LegacyPiece for FortressPiece {
    fn bounding_box(&self) -> BoundingBox {
        self.frame.bounding_box
    }
    fn save(&self) -> Option<crate::nbt::Compound> {
        use crate::nbt::Tag;
        let mut tag = super::legacy::piece_tag(
            self.type_name(),
            self.frame.bounding_box,
            self.frame.orientation,
            self.depth,
        );
        let flag = Tag::Byte(i8::from(*self.flag.lock().unwrap()));
        match self.kind {
            Kind::BridgeEndFiller => {
                tag.insert("Seed".into(), Tag::Int(self.seed));
            }
            Kind::SmallCorridorLeftTurn | Kind::SmallCorridorRightTurn => {
                tag.insert("Chest".into(), flag);
            }
            Kind::MonsterThrone => {
                tag.insert("Mob".into(), flag);
            }
            _ => {}
        }
        Some(tag)
    }
    fn type_name(&self) -> &'static str {
        match self.kind {
            // StartPiece is built through the BridgeCrossing constructor.
            Kind::Start | Kind::BridgeCrossing => "minecraft:nebcr",
            Kind::BridgeEndFiller => "minecraft:nebef",
            Kind::BridgeStraight => "minecraft:nebs",
            Kind::CorridorStairs => "minecraft:neccs",
            Kind::CorridorTBalcony => "minecraft:nectb",
            Kind::CastleEntrance => "minecraft:nece",
            Kind::SmallCorridorCrossing => "minecraft:nescsc",
            Kind::SmallCorridorLeftTurn => "minecraft:nesclt",
            Kind::SmallCorridor => "minecraft:nesc",
            Kind::SmallCorridorRightTurn => "minecraft:nescrt",
            Kind::StalkRoom => "minecraft:necsr",
            Kind::MonsterThrone => "minecraft:nemt",
            Kind::RoomCrossing => "minecraft:nerc",
            Kind::StairsRoom => "minecraft:nesr",
        }
    }
    fn post_process(&self, c: &mut PieceContext<'_>) -> Result<()> {
        let p = Palette::new(c);
        match self.kind {
            Kind::Start | Kind::BridgeCrossing => self.bridge_crossing(c, &p),
            Kind::BridgeEndFiller => self.end_filler(c, &p),
            Kind::BridgeStraight => self.bridge_straight(c, &p),
            Kind::CorridorStairs => self.corridor_stairs(c, &p),
            Kind::CorridorTBalcony => self.t_balcony(c, &p),
            Kind::CastleEntrance => self.castle_entrance(c, &p),
            Kind::SmallCorridorCrossing => self.corridor_crossing(c, &p),
            Kind::SmallCorridorLeftTurn => self.corridor_left(c, &p),
            Kind::SmallCorridor => self.corridor(c, &p),
            Kind::SmallCorridorRightTurn => self.corridor_right(c, &p),
            Kind::StalkRoom => self.stalk_room(c, &p),
            Kind::MonsterThrone => self.monster_throne(c, &p),
            Kind::RoomCrossing => self.room_crossing(c, &p),
            Kind::StairsRoom => self.stairs_room(c, &p),
        }
    }
}
