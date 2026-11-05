//! Double-precision simulation geometry for the pinned Java Edition target.
//!
//! Operation order and Java floating-point edge cases are deliberate. These are
//! simulation coordinates; rendering may use a separate camera-relative format.

pub mod border;
mod movement;
mod shape;
mod sweep;
mod traversal;
pub mod trig;
use crate::position::BlockPos;
pub use movement::{
    block_floor_height, candidate_step_heights, entity_collision_bounds, resolve_shape_movement,
    resolve_step_movement, try_resolve_step_movement,
};
pub use shape::{ShapeError, ShapeOp, VoxelShape};
pub use sweep::sweep_intersected_blocks;
pub use traversal::{TraversalError, traverse_blocks};

pub const COLLISION_EPSILON: f64 = 1.0e-7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub const ALL: [Self; 3] = [Self::X, Self::Y, Self::Z];
    pub fn step_order(movement: Vec3) -> [Self; 3] {
        if movement.x.abs() < movement.z.abs() {
            [Self::Y, Self::Z, Self::X]
        } else {
            [Self::Y, Self::X, Self::Z]
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Direction {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Direction {
    pub const ALL: [Self; 6] = [
        Self::Down,
        Self::Up,
        Self::North,
        Self::South,
        Self::West,
        Self::East,
    ];
    pub const fn opposite(self) -> Self {
        match self {
            Self::Down => Self::Up,
            Self::Up => Self::Down,
            Self::North => Self::South,
            Self::South => Self::North,
            Self::West => Self::East,
            Self::East => Self::West,
        }
    }
    pub const fn normal(self) -> [i32; 3] {
        match self {
            Self::Down => [0, -1, 0],
            Self::Up => [0, 1, 0],
            Self::North => [0, 0, -1],
            Self::South => [0, 0, 1],
            Self::West => [-1, 0, 0],
            Self::East => [1, 0, 0],
        }
    }
    pub fn approximate_nearest(vector: Vec3) -> Self {
        let [x, y, z] = vector.components().map(|v| v as f32);
        let mut best = f32::from_bits(1);
        let mut result = Self::North;
        for direction in Self::ALL {
            let [nx, ny, nz] = direction.normal().map(|v| v as f32);
            let dot = x * nx + y * ny + z * nz;
            if dot > best {
                best = dot;
                result = direction;
            }
        }
        result
    }
}

/// Component equality follows Java Double.compare, including NaNs and -0.0.
#[derive(Clone, Copy, Debug, Default)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl PartialEq for Vec3 {
    fn eq(&self, other: &Self) -> bool {
        self.components()
            .into_iter()
            .zip(other.components())
            .all(|(a, b)| canonical_bits(a) == canonical_bits(b))
    }
}
impl Vec3 {
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);
    #[inline]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
    #[inline]
    pub const fn components(self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }
    #[inline]
    pub fn component(self, axis: Axis) -> f64 {
        self.components()[axis as usize]
    }
    #[inline]
    pub fn from_block(pos: BlockPos) -> Self {
        Self::new(pos.x as f64, pos.y as f64, pos.z as f64)
    }
    #[inline]
    pub fn add_vector(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
    #[inline]
    pub fn subtract(self, other: Self) -> Self {
        self.add_vector(Self::new(-other.x, -other.y, -other.z))
    }
    #[inline]
    pub fn vector_to(self, other: Self) -> Self {
        Self::new(other.x - self.x, other.y - self.y, other.z - self.z)
    }
    #[inline]
    pub fn multiply(self, other: Self) -> Self {
        Self::new(self.x * other.x, self.y * other.y, self.z * other.z)
    }
    #[inline]
    pub fn scale(self, scale: f64) -> Self {
        self.multiply(Self::new(scale, scale, scale))
    }
    #[inline]
    pub fn reverse(self) -> Self {
        self.scale(-1.0)
    }
    #[inline]
    pub fn horizontal(self) -> Self {
        Self::new(self.x, 0.0, self.z)
    }
    #[inline]
    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }
    #[inline]
    pub fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }
    #[inline]
    pub fn length_squared(self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }
    #[inline]
    pub fn length(self) -> f64 {
        self.length_squared().sqrt()
    }
    #[inline]
    pub fn horizontal_distance_squared(self) -> f64 {
        self.x * self.x + self.z * self.z
    }
    #[inline]
    pub fn horizontal_distance(self) -> f64 {
        self.horizontal_distance_squared().sqrt()
    }
    #[inline]
    pub fn distance_squared(self, other: Self) -> f64 {
        let x = self.x - other.x;
        let y = self.y - other.y;
        let z = self.z - other.z;
        x * x + y * y + z * z
    }
    #[inline]
    pub fn distance(self, other: Self) -> f64 {
        self.distance_squared(other).sqrt()
    }
    pub fn normalize(self) -> Self {
        let length = self.length();
        if length < 9.999999747378752e-6 {
            Self::ZERO
        } else {
            Self::new(self.x / length, self.y / length, self.z / length)
        }
    }
    pub fn lerp(self, other: Self, amount: f64) -> Self {
        Self::new(
            lerp(amount, self.x, other.x),
            lerp(amount, self.y, other.y),
            lerp(amount, self.z, other.z),
        )
    }
    pub fn projected_on(self, other: Self) -> Self {
        if other.length_squared() == 0.0 {
            other
        } else {
            other
                .scale(self.dot(other))
                .scale(1.0 / other.length_squared())
        }
    }
    #[inline]
    pub fn closer_than(self, other: Self, distance: f64) -> bool {
        self.distance_squared(other) < distance * distance
    }
    #[inline]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
    #[inline]
    pub fn with_component(self, axis: Axis, value: f64) -> Self {
        let mut coordinates = self.components();
        coordinates[axis as usize] = value;
        Self::new(coordinates[0], coordinates[1], coordinates[2])
    }
    pub fn align(self, axes: [bool; 3]) -> Self {
        let coordinates = std::array::from_fn::<_, 3, _>(|i| {
            if axes[i] {
                floor_i32(self.components()[i]) as f64
            } else {
                self.components()[i]
            }
        });
        Self::new(coordinates[0], coordinates[1], coordinates[2])
    }
    pub fn relative(self, direction: Direction, distance: f64) -> Self {
        let normal = direction.normal();
        Self::new(
            self.x + distance * normal[0] as f64,
            self.y + distance * normal[1] as f64,
            self.z + distance * normal[2] as f64,
        )
    }
    pub fn rotate_x(self, angle: f32) -> Self {
        let c = trig::cos(angle as f64) as f64;
        let s = trig::sin(angle as f64) as f64;
        Self::new(self.x, self.y * c + self.z * s, self.z * c - self.y * s)
    }
    pub fn rotate_y(self, angle: f32) -> Self {
        let c = trig::cos(angle as f64) as f64;
        let s = trig::sin(angle as f64) as f64;
        Self::new(self.x * c + self.z * s, self.y, self.z * c - self.x * s)
    }
    pub fn rotate_z(self, angle: f32) -> Self {
        let c = trig::cos(angle as f64) as f64;
        let s = trig::sin(angle as f64) as f64;
        Self::new(self.x * c + self.y * s, self.y * c - self.x * s, self.z)
    }
    pub fn rotate_clockwise_90(self) -> Self {
        Self::new(-self.z, self.y, self.x)
    }
    pub fn direction_from_rotation(pitch: f32, yaw: f32) -> Self {
        let yaw_radians = -yaw * trig::DEG_TO_RAD - std::f32::consts::PI;
        let cy = trig::cos(yaw_radians as f64);
        let sy = trig::sin(yaw_radians as f64);
        let pitch_radians = -pitch * trig::DEG_TO_RAD;
        let cp = -trig::cos(pitch_radians as f64);
        let sp = trig::sin(pitch_radians as f64);
        Self::new((sy * cp) as f64, sp as f64, (cy * cp) as f64)
    }
    /// Return vanilla's (pitch, yaw) float pair in degrees.
    pub fn rotation(self) -> [f32; 2] {
        let yaw = ((-self.x).atan2(self.z) as f32) * trig::RAD_TO_DEG;
        let pitch = ((-self.y / self.length()).asin() as f32) * trig::RAD_TO_DEG;
        [pitch, yaw]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    min: Vec3,
    max: Vec3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockHit {
    pub location: Vec3,
    pub direction: Direction,
    pub block: BlockPos,
    pub inside: bool,
}

impl Aabb {
    #[inline]
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self {
            min: Vec3::new(
                java_min(min.x, max.x),
                java_min(min.y, max.y),
                java_min(min.z, max.z),
            ),
            max: Vec3::new(
                java_max(min.x, max.x),
                java_max(min.y, max.y),
                java_max(min.z, max.z),
            ),
        }
    }
    #[inline]
    pub fn block(pos: BlockPos) -> Self {
        Self::new(Vec3::from_block(pos), Vec3::from_block(pos.offset(1, 1, 1)))
    }
    #[inline]
    pub fn unit_cube(lower: Vec3) -> Self {
        Self::new(lower, lower.add_vector(Vec3::new(1.0, 1.0, 1.0)))
    }
    pub fn of_size(center: Vec3, size: Vec3) -> Self {
        Self::new(
            Vec3::new(
                center.x - size.x / 2.0,
                center.y - size.y / 2.0,
                center.z - size.z / 2.0,
            ),
            Vec3::new(
                center.x + size.x / 2.0,
                center.y + size.y / 2.0,
                center.z + size.z / 2.0,
            ),
        )
    }
    pub fn encapsulating_blocks(a: BlockPos, b: BlockPos) -> Self {
        Self::new(
            Vec3::from_block(BlockPos::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z))),
            Vec3::from_block(BlockPos::new(
                a.x.max(b.x).wrapping_add(1),
                a.y.max(b.y).wrapping_add(1),
                a.z.max(b.z).wrapping_add(1),
            )),
        )
    }
    #[inline]
    pub const fn min_position(self) -> Vec3 {
        self.min
    }
    #[inline]
    pub const fn max_position(self) -> Vec3 {
        self.max
    }
    #[inline]
    pub fn min(self, axis: Axis) -> f64 {
        self.min.component(axis)
    }
    #[inline]
    pub fn max(self, axis: Axis) -> f64 {
        self.max.component(axis)
    }
    #[inline]
    pub fn with_min(self, axis: Axis, value: f64) -> Self {
        let mut a = self.min.components();
        a[axis as usize] = value;
        Self::new(Vec3::new(a[0], a[1], a[2]), self.max)
    }
    #[inline]
    pub fn with_max(self, axis: Axis, value: f64) -> Self {
        let mut a = self.max.components();
        a[axis as usize] = value;
        Self::new(self.min, Vec3::new(a[0], a[1], a[2]))
    }
    #[inline]
    pub fn size(self) -> Vec3 {
        self.min.vector_to(self.max)
    }
    pub fn average_size(self) -> f64 {
        let v = self.size();
        (v.x + v.y + v.z) / 3.0
    }
    #[inline]
    pub fn center(self) -> Vec3 {
        self.min.lerp(self.max, 0.5)
    }
    #[inline]
    pub fn bottom_center(self) -> Vec3 {
        let v = self.center();
        Vec3::new(v.x, self.min.y, v.z)
    }
    #[inline]
    pub fn move_by(self, offset: Vec3) -> Self {
        Self::new(self.min.add_vector(offset), self.max.add_vector(offset))
    }
    #[inline]
    pub fn inflate(self, amount: Vec3) -> Self {
        Self::new(
            Vec3::new(
                self.min.x - amount.x,
                self.min.y - amount.y,
                self.min.z - amount.z,
            ),
            self.max.add_vector(amount),
        )
    }
    #[inline]
    pub fn deflate(self, amount: Vec3) -> Self {
        self.inflate(amount.reverse())
    }
    #[inline]
    pub fn expand_towards(self, amount: Vec3) -> Self {
        self.change_extent(amount, false)
    }
    #[inline]
    pub fn contract(self, amount: Vec3) -> Self {
        self.change_extent(amount, true)
    }
    #[inline]
    fn change_extent(self, amount: Vec3, contract: bool) -> Self {
        let mut min = self.min.components();
        let mut max = self.max.components();
        for (i, v) in amount.components().into_iter().enumerate() {
            if v < 0.0 {
                min[i] = if contract { min[i] - v } else { min[i] + v };
            } else if v > 0.0 {
                max[i] = if contract { max[i] - v } else { max[i] + v };
            }
        }
        Self::new(
            Vec3::new(min[0], min[1], min[2]),
            Vec3::new(max[0], max[1], max[2]),
        )
    }
    #[inline]
    pub fn intersect(self, other: Self) -> Self {
        Self::new(
            Vec3::new(
                java_max(self.min.x, other.min.x),
                java_max(self.min.y, other.min.y),
                java_max(self.min.z, other.min.z),
            ),
            Vec3::new(
                java_min(self.max.x, other.max.x),
                java_min(self.max.y, other.max.y),
                java_min(self.max.z, other.max.z),
            ),
        )
    }
    #[inline]
    pub fn union_bounds(self, other: Self) -> Self {
        Self::new(
            Vec3::new(
                java_min(self.min.x, other.min.x),
                java_min(self.min.y, other.min.y),
                java_min(self.min.z, other.min.z),
            ),
            Vec3::new(
                java_max(self.max.x, other.max.x),
                java_max(self.max.y, other.max.y),
                java_max(self.max.z, other.max.z),
            ),
        )
    }
    /// `AABB.intersects(minX, minY, minZ, maxX, maxY, maxZ)`: `intersects`
    /// with a box given by its corners, `min` below `max` on every axis.
    #[inline]
    pub fn intersects_corners(self, min: Vec3, max: Vec3) -> bool {
        self.min.x < max.x
            && self.max.x > min.x
            && self.min.y < max.y
            && self.max.y > min.y
            && self.min.z < max.z
            && self.max.z > min.z
    }
    #[inline]
    pub fn intersects(self, other: Self) -> bool {
        self.min.x < other.max.x
            && self.max.x > other.min.x
            && self.min.y < other.max.y
            && self.max.y > other.min.y
            && self.min.z < other.max.z
            && self.max.z > other.min.z
    }
    #[inline]
    pub fn contains(self, point: Vec3) -> bool {
        point.x >= self.min.x
            && point.x < self.max.x
            && point.y >= self.min.y
            && point.y < self.max.y
            && point.z >= self.min.z
            && point.z < self.max.z
    }
    #[inline]
    pub fn has_nan(self) -> bool {
        self.min
            .components()
            .into_iter()
            .chain(self.max.components())
            .any(f64::is_nan)
    }
    #[inline]
    pub fn distance_squared_to_point(self, point: Vec3) -> f64 {
        let distances = Axis::ALL.map(|a| {
            java_max(
                java_max(
                    self.min(a) - point.component(a),
                    point.component(a) - self.max(a),
                ),
                0.0,
            )
        });
        distances[0] * distances[0] + distances[1] * distances[1] + distances[2] * distances[2]
    }
    pub fn distance_squared_to_box(self, other: Self) -> f64 {
        let distances = Axis::ALL.map(|a| {
            java_max(
                java_max(self.min(a) - other.max(a), other.min(a) - self.max(a)),
                0.0,
            )
        });
        distances[0] * distances[0] + distances[1] * distances[1] + distances[2] * distances[2]
    }
    pub fn next_deflated(self) -> Self {
        let lo = self.min.components().map(next_up);
        let hi = self.max.components().map(next_down);
        Self::new(
            Vec3::new(lo[0], lo[1], lo[2]),
            Vec3::new(hi[0], hi[1], hi[2]),
        )
    }
    fn clip_direction(self, start: Vec3, delta: Vec3, best: &mut f64) -> Option<Direction> {
        let mut result = None;
        for axis in Axis::ALL {
            let i = axis as usize;
            let j = (i + 1) % 3;
            let k = (i + 2) % 3;
            let ds = delta.components();
            let origins = start.components();
            let (plane, face) = if ds[i] > COLLISION_EPSILON {
                (
                    self.min(axis),
                    [Direction::West, Direction::Down, Direction::North][i],
                )
            } else if ds[i] < -COLLISION_EPSILON {
                (
                    self.max(axis),
                    [Direction::East, Direction::Up, Direction::South][i],
                )
            } else {
                continue;
            };
            let t = (plane - origins[i]) / ds[i];
            let u = origins[j] + t * ds[j];
            let v = origins[k] + t * ds[k];
            if 0.0 < t
                && t < *best
                && self.min.components()[j] - COLLISION_EPSILON < u
                && u < self.max.components()[j] + COLLISION_EPSILON
                && self.min.components()[k] - COLLISION_EPSILON < v
                && v < self.max.components()[k] + COLLISION_EPSILON
            {
                *best = t;
                result = Some(face);
            }
        }
        result
    }
    pub fn clip(self, start: Vec3, end: Vec3) -> Option<Vec3> {
        let delta = start.vector_to(end);
        let mut best = 1.0;
        self.clip_direction(start, delta, &mut best)
            .map(|_| start.add_vector(delta.scale(best)))
    }
    pub fn clip_boxes(
        boxes: impl IntoIterator<Item = Self>,
        start: Vec3,
        end: Vec3,
        block: BlockPos,
    ) -> Option<BlockHit> {
        let delta = start.vector_to(end);
        let mut best = 1.0;
        let mut direction = None;
        for aabb in boxes {
            if let Some(face) = aabb
                .move_by(Vec3::from_block(block))
                .clip_direction(start, delta, &mut best)
            {
                direction = Some(face);
            }
        }
        direction.map(|face| BlockHit {
            location: start.add_vector(delta.scale(best)),
            direction: face,
            block,
            inside: false,
        })
    }
    pub fn collided_along_vector(self, movement: Vec3, boxes: &[Self]) -> bool {
        let start = self.center();
        let end = start.add_vector(movement);
        let amount = self.size().scale(0.5).subtract(Vec3::new(
            COLLISION_EPSILON,
            COLLISION_EPSILON,
            COLLISION_EPSILON,
        ));
        boxes.iter().any(|b| {
            let b = b.inflate(amount);
            b.contains(end) || b.contains(start) || b.clip(start, end).is_some()
        })
    }
}

#[inline]
pub(crate) fn java_min(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        a
    } else if b.is_nan() {
        b
    } else if a == 0.0 && b == 0.0 {
        f64::from_bits(a.to_bits() | b.to_bits())
    } else if a <= b {
        a
    } else {
        b
    }
}
#[inline]
pub(crate) fn java_max(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        a
    } else if b.is_nan() {
        b
    } else if a == 0.0 && b == 0.0 {
        f64::from_bits(a.to_bits() & b.to_bits())
    } else if a >= b {
        a
    } else {
        b
    }
}
#[inline]
pub(crate) fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else {
        java_min(value, max)
    }
}
/// `Mth.floor`, which is `(int) Math.floor(v)`: truncate, then step down for
/// negative fractions. A truncating conversion and a compare, where
/// `f64::floor` is a libm call on baseline x86-64. Both casts saturate (NaN is
/// 0), so the step down saturates too: below `i32::MIN` stays `i32::MIN`.
#[inline]
pub(crate) fn floor_i32(value: f64) -> i32 {
    let truncated = value as i32;
    if value < f64::from(truncated) {
        truncated.saturating_sub(1)
    } else {
        truncated
    }
}
fn lerp(amount: f64, start: f64, end: f64) -> f64 {
    start + amount * (end - start)
}
fn canonical_bits(value: f64) -> u64 {
    if value.is_nan() {
        0x7ff8_0000_0000_0000
    } else {
        value.to_bits()
    }
}
fn next_up(value: f64) -> f64 {
    if value.is_nan() || value == f64::INFINITY {
        value
    } else if value == 0.0 {
        f64::from_bits(1)
    } else {
        f64::from_bits(if value > 0.0 {
            value.to_bits() + 1
        } else {
            value.to_bits() - 1
        })
    }
}
fn next_down(value: f64) -> f64 {
    -next_up(-value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_i32_is_mths_floor() {
        for v in [
            0.0,
            -0.0,
            0.5,
            -0.5,
            1.0,
            -1.0,
            2.999,
            -2.999,
            1e-300,
            -1e-300,
            16_777_215.5,
            -16_777_215.5,
            // Past the i32 range `(int) Math.floor` saturates (vanilla 26.3's
            // BlockCollisions and BlockPos.containing rely on it).
            2_147_483_647.5,
            2_147_483_648.0,
            -2_147_483_648.0,
            -2_147_483_648.5,
            -2_147_483_649.0,
            3e9,
            -3e9,
            f64::MAX,
            f64::MIN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            assert_eq!(floor_i32(v), v.floor() as i32, "{v}");
        }
        assert_eq!(floor_i32(f64::NAN), 0);
        assert_eq!(floor_i32(-3e9), i32::MIN);
        assert_eq!(floor_i32(-2_147_483_648.5), i32::MIN);
    }
    #[test]
    fn ray_faces_and_half_open_bounds() {
        let b = Aabb::block(BlockPos::new(0, 0, 0));
        assert!(b.contains(Vec3::ZERO));
        assert!(!b.contains(Vec3::new(1.0, 0.0, 0.0)));
        assert_eq!(
            b.clip(Vec3::new(-1.0, 0.5, 0.5), Vec3::new(2.0, 0.5, 0.5)),
            Some(Vec3::new(0.0, 0.5, 0.5))
        );
        assert!(
            b.clip(Vec3::new(0.5, 0.5, 0.5), Vec3::new(2.0, 0.5, 0.5))
                .is_none()
        );
        assert!(
            b.clip(Vec3::new(-1.0, 0.5, 0.5), Vec3::new(0.0, 0.5, 0.5))
                .is_none()
        );
    }
    #[test]
    fn numerical_boundaries() {
        assert_eq!(java_min(0.0, -0.0).to_bits(), (-0.0f64).to_bits());
        assert_eq!(java_max(0.0, -0.0).to_bits(), 0.0f64.to_bits());
        assert_eq!(Vec3::new(-0.0, 0.0, 0.0).normalize(), Vec3::ZERO);
        assert_ne!(Vec3::new(-0.0, 0.0, 0.0), Vec3::ZERO);
        let b = Aabb::block(BlockPos::new(i32::MAX, 0, 0));
        assert_eq!(b.min(Axis::X), i32::MIN as f64);
        assert_eq!(b.max(Axis::X), i32::MAX as f64);
        assert!(Aabb::new(Vec3::new(f64::NAN, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0)).has_nan());
    }
}
