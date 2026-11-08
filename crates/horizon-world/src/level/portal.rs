//! `PortalShape`: the obsidian frame search used by nether portal blocks.
use super::{LevelAccess, Sim, update::rel};
use crate::registry::BlockStateId;
use horizon_core::{geometry::Direction, position::BlockPos, random::RandomSource};

const MIN_WIDTH: i32 = 2;
const MAX_WIDTH: i32 = 21;
const MIN_HEIGHT: i32 = 3;
const MAX_HEIGHT: i32 = 21;

/// The result of `PortalShape.findAnyShape`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortalShape {
    pub x_axis: bool,
    pub right: Direction,
    pub bottom_left: BlockPos,
    pub width: i32,
    pub height: i32,
    pub portal_blocks: i32,
}

impl PortalShape {
    pub const fn is_valid(&self) -> bool {
        self.width >= MIN_WIDTH
            && self.width <= MAX_WIDTH
            && self.height >= MIN_HEIGHT
            && self.height <= MAX_HEIGHT
    }
    pub const fn is_complete(&self) -> bool {
        self.is_valid() && self.portal_blocks == self.width * self.height
    }
}

fn step(pos: BlockPos, dir: Direction, n: i32) -> BlockPos {
    let [dx, dy, dz] = dir.normal();
    pos.offset(dx * n, dy * n, dz * n)
}

impl<L: LevelAccess + ?Sized> Sim<'_, L> {
    fn portal_frame(&self, state: BlockStateId) -> bool {
        let b = self.b;
        b.tag("minecraft:nether_portal_frame")
            .is_some_and(|tag| b.in_set(state, tag))
    }
    /// `PortalShape.isEmpty`: air, fire or a portal block.
    fn portal_empty(&self, state: BlockStateId) -> bool {
        let b = self.b;
        b.is_air(state)
            || b.tag("minecraft:fire")
                .is_some_and(|tag| b.in_set(state, tag))
            || b.is_block(state, b.named.nether_portal)
    }
    fn portal_distance_until_edge_above_frame(&self, pos: BlockPos, dir: Direction) -> i32 {
        for width in 0..=MAX_WIDTH {
            let at = step(pos, dir, width);
            let state = self.state(at);
            if !self.portal_empty(state) {
                return if self.portal_frame(state) { width } else { 0 };
            }
            if !self.portal_frame(self.state(rel(at, Direction::Down))) {
                break;
            }
        }
        0
    }
    /// `PortalShape.findAnyShape`.
    pub fn find_portal_shape(&self, pos: BlockPos, x_axis: bool) -> PortalShape {
        let right = if x_axis {
            Direction::West
        } else {
            Direction::South
        };
        let mut shape = PortalShape {
            x_axis,
            right,
            bottom_left: pos,
            width: 0,
            height: 0,
            portal_blocks: 0,
        };
        // calculateBottomLeft
        let min_y = self.level.min_y().max(pos.y - MAX_HEIGHT);
        let mut start = pos;
        while start.y > min_y && self.portal_empty(self.state(rel(start, Direction::Down))) {
            start = rel(start, Direction::Down);
        }
        let left = right.opposite();
        let edge = self.portal_distance_until_edge_above_frame(start, left) - 1;
        if edge < 0 {
            return shape;
        }
        shape.bottom_left = step(start, left, edge);
        // calculateWidth
        let width = self.portal_distance_until_edge_above_frame(shape.bottom_left, right);
        if !(MIN_WIDTH..=MAX_WIDTH).contains(&width) {
            return shape;
        }
        shape.width = width;
        // calculateHeight: getDistanceUntilTop counts portal blocks as it goes.
        let mut height = MAX_HEIGHT;
        'rows: for h in 0..MAX_HEIGHT {
            let row = step(shape.bottom_left, Direction::Up, h);
            if !self.portal_frame(self.state(step(row, right, -1)))
                || !self.portal_frame(self.state(step(row, right, width)))
            {
                height = h;
                break;
            }
            for i in 0..width {
                let state = self.state(step(row, right, i));
                if !self.portal_empty(state) {
                    height = h;
                    break 'rows;
                }
                if self.b.is_block(state, self.b.named.nether_portal) {
                    shape.portal_blocks += 1;
                }
            }
        }
        let top = step(shape.bottom_left, Direction::Up, height);
        let has_top = (0..width).all(|i| self.portal_frame(self.state(step(top, right, i))));
        shape.height = if (MIN_HEIGHT..=MAX_HEIGHT).contains(&height) && has_top {
            height
        } else {
            0
        };
        shape
    }
    /// `PortalShape.findEmptyPortalShape`: a valid frame holding no portal
    /// block yet, on the preferred axis first.
    pub fn find_empty_portal_shape(&self, pos: BlockPos, x_axis: bool) -> Option<PortalShape> {
        [x_axis, !x_axis]
            .into_iter()
            .map(|axis| self.find_portal_shape(pos, axis))
            .find(|shape| shape.is_valid() && shape.portal_blocks == 0)
    }
    /// `PortalShape.createPortalBlocks`: fill the frame (flags 18).
    pub fn create_portal_blocks(&mut self, shape: &PortalShape) {
        let b = self.b;
        let base = b.default_of(b.named.nether_portal);
        let state = b
            .table
            .with_text(base, b.props.axis, if shape.x_axis { "x" } else { "z" })
            .unwrap_or(base);
        for h in 0..shape.height {
            for w in 0..shape.width {
                let at = step(step(shape.bottom_left, Direction::Up, h), shape.right, w);
                self.set_block(at, state, 18);
            }
        }
    }
    /// `BaseFireBlock.canBePlacedAt(level, pos, forwardDirection)`: air where
    /// fire would survive, or the inside of an empty portal frame (vertical
    /// directions pick the frame axis with the level random).
    pub fn fire_can_be_placed_at(&mut self, pos: BlockPos, forward: Direction) -> bool {
        if !self.b.is_air(self.state(pos)) {
            return false;
        }
        if self.can_survive(self.fire_state_for(pos), pos) {
            return true;
        }
        // isPortal
        if !matches!(
            self.level.level_key(),
            super::LevelKey::OVERWORLD | super::LevelKey::NETHER
        ) {
            return false;
        }
        for face in Direction::ALL {
            if self.portal_frame(self.state(rel(pos, face))) {
                // getCounterClockWise().getAxis(): north/south turn to X.
                let x_axis = match forward {
                    Direction::North | Direction::South => true,
                    Direction::West | Direction::East => false,
                    Direction::Up | Direction::Down => {
                        self.random().next_int_bound(2).unwrap_or(0) == 0
                    }
                };
                return self.find_empty_portal_shape(pos, x_axis).is_some();
            }
        }
        false
    }
}
