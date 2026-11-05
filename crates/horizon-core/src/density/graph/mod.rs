//! Immutable density definitions and the first integrated compiler operations.
//! Seeded noise registration is supported; serialized codecs and remaining
//! function kinds are pending.
use super::*;
use std::collections::HashMap;
mod compiler;
mod seeded;
#[cfg(test)]
mod tests;
pub use compiler::DensityCompiler;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FunctionId(usize);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NoiseId(usize);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HolderId(usize);

#[derive(Clone, Copy, Debug)]
pub enum GraphUnary {
    Abs,
    Square,
    Cube,
    HalfNegative,
    QuarterNegative,
    Reciprocal,
    Negate,
    Squeeze,
    Sign,
    Log,
    Sqrt,
}
impl GraphUnary {
    fn operation(self) -> UnaryOp {
        match self {
            Self::Abs => UnaryOp::Abs,
            Self::Square => UnaryOp::Square,
            Self::Cube => UnaryOp::Cube,
            Self::HalfNegative => UnaryOp::LeakyRelu(0.5),
            Self::QuarterNegative => UnaryOp::LeakyRelu(0.25),
            Self::Reciprocal => UnaryOp::Reciprocal,
            Self::Negate => UnaryOp::Negate,
            Self::Squeeze => UnaryOp::Squeeze,
            Self::Sign => UnaryOp::Sign,
            Self::Log => UnaryOp::Log,
            Self::Sqrt => UnaryOp::Sqrt,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphBinary {
    Add,
    Subtract,
    Multiply,
    Divide,
    Minimum,
    Maximum,
}

/// References belong to the graph that creates them. New nodes may refer only
/// to existing immutable nodes, keeping the definition graph acyclic.
#[derive(Clone, Debug)]
pub enum GraphNode {
    Constant(f32),
    Spline(CubicSpline<FunctionId>),
    Noise {
        noise: NoiseId,
        xz_scale: f64,
        y_scale: f64,
        shift_x: FunctionId,
        shift_y: FunctionId,
        shift_z: FunctionId,
    },
    OffsetNoise {
        noise: NoiseId,
        kind: ShiftNoiseKind,
    },
    BlendedNoise(BlendedNoise),
    EndIslands,
    WorldField(WorldField),
    BlendDensity(FunctionId),
    /// A direct holder compares by value; a registry holder by identity.
    Reference {
        input: FunctionId,
        holder: Option<HolderId>,
    },
    Gradient {
        axis: Axis,
        tiling: TilingMode,
        from: i32,
        to: i32,
        low: f32,
        high: f32,
    },
    Unary {
        kind: GraphUnary,
        input: FunctionId,
    },
    Binary {
        kind: GraphBinary,
        left: FunctionId,
        right: FunctionId,
    },
    Round {
        mode: RoundMode,
        input: FunctionId,
        multiple: FunctionId,
    },
    Power {
        base: FunctionId,
        exponent: FunctionId,
    },
    Clamp {
        input: FunctionId,
        min: f32,
        max: f32,
    },
    Choice {
        input: FunctionId,
        min: f32,
        max: f32,
        inside: FunctionId,
        outside: FunctionId,
    },
    Lerp {
        alpha: FunctionId,
        first: FunctionId,
        second: FunctionId,
    },
    Slice {
        axis: Axis,
        coordinate: i32,
        input: FunctionId,
    },
    Cache(FunctionId),
    Interpolated {
        input: FunctionId,
        xz: i32,
        y: i32,
    },
    Surface {
        density: FunctionId,
        upper: FunctionId,
        lower: i32,
        height: i32,
    },
    IntervalSelect {
        input: FunctionId,
        thresholds: Arc<[f32]>,
        branches: Vec<FunctionId>,
    },
    DistanceToPoint {
        point: [i32; 3],
        metric: DistanceMetric,
    },
}
impl GraphNode {
    fn children_mut(&mut self) -> Vec<&mut FunctionId> {
        match self {
            Self::Spline(spline) => spline.coordinates_mut(),
            Self::Constant(_)
            | Self::Gradient { .. }
            | Self::DistanceToPoint { .. }
            | Self::OffsetNoise { .. }
            | Self::BlendedNoise(_)
            | Self::EndIslands
            | Self::WorldField(_) => vec![],
            Self::Noise {
                shift_x,
                shift_y,
                shift_z,
                ..
            } => vec![shift_x, shift_y, shift_z],
            Self::Reference { input, .. }
            | Self::Unary { input, .. }
            | Self::Clamp { input, .. }
            | Self::Slice { input, .. }
            | Self::Cache(input)
            | Self::BlendDensity(input)
            | Self::Interpolated { input, .. } => vec![input],
            Self::Binary { left, right, .. } => vec![left, right],
            Self::Round {
                input, multiple, ..
            } => vec![input, multiple],
            Self::Power { base, exponent } => vec![base, exponent],
            Self::Choice {
                input,
                inside,
                outside,
                ..
            } => vec![input, inside, outside],
            Self::Lerp {
                alpha,
                first,
                second,
            } => vec![alpha, first, second],
            Self::Surface { density, upper, .. } => vec![density, upper],
            Self::IntervalSelect {
                input, branches, ..
            } => std::iter::once(input).chain(branches.iter_mut()).collect(),
        }
    }
    fn key(&self) -> Vec<u64> {
        let f = |v: f32| u64::from(if v.is_nan() { 0x7fc0_0000 } else { v.to_bits() });
        let i = |v: i32| u64::from(v as u32);
        let d = |v: f64| {
            if v.is_nan() {
                0x7ff8_0000_0000_0000
            } else {
                v.to_bits()
            }
        };
        let id = |v: FunctionId| v.0 as u64;
        match *self {
            Self::Reference { input, holder } => match holder {
                Some(holder) => vec![22, 1, holder.0 as u64],
                None => vec![22, 0, id(input)],
            },
            Self::EndIslands => vec![17],
            Self::WorldField(field) => vec![20, field as u64],
            Self::BlendDensity(input) => vec![21, id(input)],
            Self::Power { base, exponent } => vec![19, id(base), id(exponent)],
            Self::Round {
                mode,
                input,
                multiple,
            } => vec![18, mode as u64, id(input), id(multiple)],
            Self::Noise {
                noise,
                xz_scale,
                y_scale,
                shift_x,
                shift_y,
                shift_z,
            } => vec![
                14,
                noise.0 as u64,
                d(xz_scale),
                d(y_scale),
                id(shift_x),
                id(shift_y),
                id(shift_z),
            ],
            Self::OffsetNoise { noise, kind } => vec![15, noise.0 as u64, kind as u64],
            Self::BlendedNoise(v) => vec![
                16,
                d(v.xz_scale),
                d(v.y_scale),
                d(v.xz_factor),
                d(v.y_factor),
                d(v.smear_scale_multiplier),
            ],
            Self::Spline(ref spline) => {
                fn append(spline: &CubicSpline<FunctionId>, key: &mut Vec<u64>) {
                    let bits =
                        |v: f32| u64::from(if v.is_nan() { 0x7fc0_0000 } else { v.to_bits() });
                    match spline {
                        CubicSpline::Constant(value) => key.extend([0, bits(*value)]),
                        CubicSpline::Multipoint(node) => {
                            key.extend([1, node.coordinate.0 as u64, node.points.len() as u64]);
                            for point in &node.points {
                                key.extend([bits(point.location), bits(point.derivative)]);
                                append(&point.value, key);
                            }
                        }
                    }
                }
                let mut key = vec![13];
                append(spline, &mut key);
                key
            }
            Self::Constant(v) => vec![0, f(v)],
            Self::Gradient {
                axis,
                tiling,
                from,
                to,
                low,
                high,
            } => vec![
                1,
                axis as u64,
                tiling as u64,
                i(from),
                i(to),
                f(low),
                f(high),
            ],
            Self::Unary { kind, input } => vec![2, kind as u64, id(input)],
            Self::Binary { kind, left, right } => vec![3, kind as u64, id(left), id(right)],
            Self::Clamp { input, min, max } => vec![4, id(input), f(min), f(max)],
            Self::Choice {
                input,
                min,
                max,
                inside,
                outside,
            } => vec![5, id(input), f(min), f(max), id(inside), id(outside)],
            Self::Lerp {
                alpha,
                first,
                second,
            } => vec![6, id(alpha), id(first), id(second)],
            Self::Slice {
                axis,
                coordinate,
                input,
            } => vec![7, axis as u64, i(coordinate), id(input)],
            Self::Cache(input) => vec![8, id(input)],
            Self::Interpolated { input, xz, y } => vec![9, id(input), i(xz), i(y)],
            Self::Surface {
                density,
                upper,
                lower,
                height,
            } => vec![10, id(density), id(upper), i(lower), i(height)],
            Self::IntervalSelect {
                input,
                ref thresholds,
                ref branches,
            } => {
                let mut key = vec![11, id(input), thresholds.len() as u64];
                key.push(if thresholds.iter().any(|v| v.is_nan()) {
                    Arc::as_ptr(thresholds) as *const f32 as usize as u64
                } else {
                    0
                });
                key.extend(thresholds.iter().map(|v| if *v == 0.0 { 0 } else { f(*v) }));
                key.extend(branches.iter().copied().map(id));
                key
            }
            Self::DistanceToPoint { point, metric } => {
                vec![12, i(point[0]), i(point[1]), i(point[2]), metric as u64]
            }
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompileError {
    UnknownFunction,
    NestingTooDeep,
    InvalidAxes,
    UnpreparedCache,
    InvalidIntervals,
    UnknownNoise,
    DuplicateNoise,
    DuplicateFunction,
    UnknownHolder,
    MissingSeededContext,
    PoisonedNoiseCache,
    Noise(crate::noise::NoiseBuildError),
    Spline(SplineError),
    Range(IntervalError),
    Sample(SampleError),
}
impl From<IntervalError> for CompileError {
    fn from(e: IntervalError) -> Self {
        Self::Range(e)
    }
}
impl From<SampleError> for CompileError {
    fn from(e: SampleError) -> Self {
        Self::Sample(e)
    }
}
impl From<SplineError> for CompileError {
    fn from(e: SplineError) -> Self {
        Self::Spline(e)
    }
}
impl From<crate::noise::NoiseBuildError> for CompileError {
    fn from(e: crate::noise::NoiseBuildError) -> Self {
        Self::Noise(e)
    }
}
impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "density graph compilation failed: {self:?}")
    }
}
impl std::error::Error for CompileError {}

#[derive(Clone, Debug)]
enum Definition {
    Node(GraphNode),
    Bound {
        sampler: Arc<dyn DensitySampler>,
        range: Interval,
        axes: u8,
    },
}
#[derive(Clone, Debug)]
struct Entry {
    definition: Definition,
    depth: usize,
    equality_id: FunctionId,
    hash_variant: bool,
}
#[derive(Clone, Debug, Default)]
pub struct DensityGraph {
    entries: Vec<Entry>,
    interned: HashMap<(Vec<u64>, bool), FunctionId>,
    equal: HashMap<Vec<u64>, FunctionId>,
    noises: Vec<seeded::NoiseEntry>,
    noise_names: HashMap<String, NoiseId>,
    holders: Vec<FunctionId>,
    function_names: HashMap<String, FunctionId>,
}
impl DensityGraph {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn insert(&mut self, node: GraphNode) -> Result<FunctionId, CompileError> {
        if let GraphNode::Reference {
            input,
            holder: Some(holder),
        } = node
        {
            if self.holders.get(holder.0) != Some(&input) {
                return Err(CompileError::UnknownHolder);
            }
        }
        if let GraphNode::Noise { noise, .. } | GraphNode::OffsetNoise { noise, .. } = node {
            self.noise_entry(noise)?;
        }
        if let GraphNode::IntervalSelect {
            thresholds,
            branches,
            ..
        } = &node
        {
            if branches.len() != thresholds.len().saturating_add(1) || branches.len() > 256 {
                return Err(CompileError::InvalidIntervals);
            }
        }
        // Fastutil threshold lists compare zeros numerically but hash their
        // signed bits. Among equal definitions, the only possible hash delta
        // is 0x80000000. Java record/list hashes use odd (31) multipliers, so
        // these deltas XOR through children and cancel in pairs. Keep equality
        // classes separate from this hash variant to reproduce Java map reuse,
        // including equal parents whose differing child hashes cancel.
        let mut hash_variant = match &node {
            GraphNode::IntervalSelect { thresholds, .. } => {
                thresholds
                    .iter()
                    .filter(|v| v.to_bits() == 0x8000_0000)
                    .count()
                    % 2
                    != 0
            }
            _ => false,
        };
        let mut canonical = node.clone();
        let mut depth = if let GraphNode::Spline(spline) = &node {
            fn spline_depth(
                spline: &CubicSpline<FunctionId>,
                graph: &DensityGraph,
            ) -> Result<usize, CompileError> {
                Ok(match spline {
                    CubicSpline::Constant(_) => 0,
                    CubicSpline::Multipoint(node) => {
                        let mut depth = graph.entry(node.coordinate)?.depth;
                        for point in &node.points {
                            depth = depth.max(spline_depth(&point.value, graph)?);
                        }
                        depth + 1
                    }
                })
            }
            spline_depth(spline, self)? + 1
        } else {
            1
        };
        for child in canonical.children_mut() {
            let entry = self.entry(*child)?;
            depth = depth.max(entry.depth + 1);
            // Registry references hash their holder identity, not its value.
            if !matches!(
                node,
                GraphNode::Reference {
                    holder: Some(_),
                    ..
                }
            ) {
                hash_variant ^= entry.hash_variant;
            }
            *child = entry.equality_id;
        }
        let key = canonical.key();
        if let Some(id) = self.interned.get(&(key.clone(), hash_variant)) {
            return Ok(*id);
        }
        if depth > 256 {
            return Err(CompileError::NestingTooDeep);
        }
        let id = FunctionId(self.entries.len());
        let equality_id = *self.equal.entry(key.clone()).or_insert(id);
        self.entries.push(Entry {
            definition: Definition::Node(node),
            depth,
            equality_id,
            hash_variant,
        });
        self.interned.insert((key, hash_variant), id);
        Ok(id)
    }
    /// Bind a precompiled input with its declared range and axis dependency
    /// mask (X=1, Y=2, Z=4). Each binding has distinct function identity.
    pub fn bind(
        &mut self,
        sampler: Arc<dyn DensitySampler>,
        range: Interval,
        axes: u8,
    ) -> Result<FunctionId, CompileError> {
        if axes & !7 != 0 {
            return Err(CompileError::InvalidAxes);
        }
        let id = FunctionId(self.entries.len());
        self.entries.push(Entry {
            definition: Definition::Bound {
                sampler,
                range,
                axes,
            },
            depth: 1,
            equality_id: id,
            hash_variant: false,
        });
        Ok(id)
    }
    fn entry(&self, id: FunctionId) -> Result<&Entry, CompileError> {
        self.entries.get(id.0).ok_or(CompileError::UnknownFunction)
    }
    /// Register a noise under its canonical namespaced registry identifier.
    /// Definitions and names are immutable after registration.
    pub fn register_noise(
        &mut self,
        name: String,
        definition: crate::noise::NormalNoise,
    ) -> Result<NoiseId, CompileError> {
        if self.noise_names.contains_key(&name) {
            return Err(CompileError::DuplicateNoise);
        }
        let id = NoiseId(self.noises.len());
        self.noise_names.insert(name.clone(), id);
        self.noises.push(seeded::NoiseEntry { name, definition });
        Ok(id)
    }
    /// Register an immutable named density value and return its holder wrapper.
    /// Names must already be canonical namespaced registry identifiers.
    pub fn register_function(
        &mut self,
        name: String,
        input: FunctionId,
    ) -> Result<FunctionId, CompileError> {
        if self.function_names.contains_key(&name) {
            return Err(CompileError::DuplicateFunction);
        }
        self.entry(input)?;
        let holder = HolderId(self.holders.len());
        self.holders.push(input);
        let result = self.insert(GraphNode::Reference {
            input,
            holder: Some(holder),
        });
        match result {
            Ok(id) => {
                self.function_names.insert(name, id);
                Ok(id)
            }
            Err(error) => {
                self.holders.pop();
                Err(error)
            }
        }
    }
    pub fn function(&self, name: &str) -> Option<FunctionId> {
        self.function_names.get(name).copied()
    }
    fn noise_entry(&self, id: NoiseId) -> Result<&seeded::NoiseEntry, CompileError> {
        self.noises.get(id.0).ok_or(CompileError::UnknownNoise)
    }
    /// Vanilla definition equality, separate from interned identity. Interval
    /// threshold lists can compare equal despite having different Java hashes.
    pub fn java_equals(&self, left: FunctionId, right: FunctionId) -> Result<bool, CompileError> {
        Ok(self.entry(left)?.equality_id == self.entry(right)?.equality_id)
    }
    pub fn domain_axes(&self, id: FunctionId) -> Result<u8, CompileError> {
        let mut node = match &self.entry(id)?.definition {
            Definition::Bound { axes, .. } => return Ok(*axes),
            Definition::Node(node) => node.clone(),
        };
        let mask = match node {
            GraphNode::EndIslands => return Ok(5),
            GraphNode::WorldField(field) => return Ok(field.domain_axes()),
            GraphNode::OffsetNoise { kind, .. } => {
                return Ok(if kind == ShiftNoiseKind::Shift { 7 } else { 5 });
            }
            GraphNode::BlendedNoise(_) => return Ok(7),
            GraphNode::Noise {
                xz_scale, y_scale, ..
            } => {
                let mut axes = if xz_scale == 0.0 { 0 } else { 5 };
                if y_scale != 0.0 {
                    axes |= 2;
                }
                for child in node.children_mut() {
                    axes |= self.domain_axes(*child)?;
                }
                return Ok(axes);
            }
            GraphNode::Constant(_) => return Ok(0),
            GraphNode::Gradient { axis, .. } => return Ok(1 << axis as u8),
            GraphNode::DistanceToPoint { .. } => return Ok(7),
            GraphNode::Slice { axis, .. } => !(1 << axis as u8),
            GraphNode::Surface { .. } => !2,
            _ => 7,
        };
        let mut axes = 0;
        for child in node.children_mut() {
            axes |= self.domain_axes(*child)?;
        }
        Ok(axes & mask)
    }
    pub fn range(&self, id: FunctionId) -> Result<Interval, CompileError> {
        let node = match &self.entry(id)?.definition {
            Definition::Bound { range, .. } => return Ok(*range),
            Definition::Node(node) => node,
        };
        Ok(match *node {
            GraphNode::EndIslands => Interval::new(-0.84375, 0.5625)?,
            GraphNode::WorldField(field) => field.range(),
            GraphNode::Noise { noise, .. } => {
                let [min, max] = self.noise_entry(noise)?.definition.range();
                Interval::new(min, max)?
            }
            GraphNode::OffsetNoise { noise, .. } => {
                let [min, max] = self.noise_entry(noise)?.definition.range();
                Interval::new(min, max)?.multiply_range(Interval::exact(4.0)?)
            }
            GraphNode::BlendedNoise(definition) => definition.range()?,
            GraphNode::Spline(ref spline) => spline.try_range(&mut |id| self.range(id))?,
            GraphNode::Constant(v) => Interval::exact(v)?,
            GraphNode::Gradient { low, high, .. } => Interval::encapsulating_values(low, high),
            GraphNode::DistanceToPoint { .. } => Interval::new(0.0, f32::INFINITY)?,
            GraphNode::IntervalSelect { ref branches, .. } => {
                let ranges = branches
                    .iter()
                    .map(|id| self.range(*id))
                    .collect::<Result<Vec<_>, _>>()?;
                Interval::encapsulating(&ranges)?
            }
            GraphNode::Unary { kind, input } => {
                let range = self.range(input)?;
                match kind {
                    GraphUnary::Abs => range.abs(),
                    GraphUnary::Square => range.square(),
                    GraphUnary::Reciprocal => range.reciprocal(),
                    GraphUnary::Sign => range.sign(),
                    GraphUnary::Negate => Interval::exact(0.0)?.subtract_range(range),
                    GraphUnary::Log => range.log()?,
                    GraphUnary::Sqrt => range.power(Interval::exact(0.5)?),
                    _ => range.map_monotonic(|v| kind.operation().apply(v, false))?,
                }
            }
            GraphNode::Binary { kind, left, right } => {
                let left = self.range(left)?;
                let right = self.range(right)?;
                match kind {
                    GraphBinary::Add => left.add_range(right),
                    GraphBinary::Subtract => left.subtract_range(right),
                    GraphBinary::Multiply => left.multiply_range(right),
                    GraphBinary::Divide => left.divide_range(right),
                    GraphBinary::Minimum => left.minimum(right),
                    GraphBinary::Maximum => left.maximum(right),
                }
            }
            GraphNode::Round {
                mode,
                input,
                multiple,
            } => {
                let multiple = self.range(multiple)?;
                self.range(input)?
                    .divide_range(multiple)
                    .map_monotonic(|v| mode.integer(v))?
                    .multiply_range(multiple)
            }
            GraphNode::Power { base, exponent } => self.range(base)?.power(self.range(exponent)?),
            GraphNode::Clamp { input, min, max } => self.range(input)?.clamp(min, max)?,
            GraphNode::Choice {
                inside, outside, ..
            } => Interval::encapsulating(&[self.range(inside)?, self.range(outside)?])?,
            GraphNode::Lerp {
                alpha,
                first,
                second,
            } => self
                .range(alpha)?
                .lerp_ranges(self.range(first)?, self.range(second)?),
            GraphNode::Slice { input, .. }
            | GraphNode::Cache(input)
            | GraphNode::BlendDensity(input)
            | GraphNode::Reference { input, .. }
            | GraphNode::Interpolated { input, .. } => self.range(input)?,
            GraphNode::Surface { upper, lower, .. } => Interval::new(
                lower as f32,
                super::arithmetic::java_max(lower as f32, self.range(upper)?.max()),
            )?,
        })
    }
    /// Inclusive `y` bounds outside which `id` is never positive, at any x
    /// and z, read from its shape: through registry references, a range
    /// choice whose input is a clamped `y` gradient and whose out-of-range
    /// branch is a constant `<= 0` (as vanilla's ore vein densities are).
    /// A side the gradient stays in range on is unbounded (`i32::MIN` or
    /// `i32::MAX`); `low > high` when the input is never in range. `None` for
    /// any other shape.
    pub fn positive_y_bounds(&self, id: FunctionId) -> Result<Option<[i32; 2]>, CompileError> {
        let Some(&GraphNode::Choice {
            input,
            min,
            max,
            outside,
            ..
        }) = self.resolve_references(id)?
        else {
            return Ok(None);
        };
        if !matches!(self.resolve_references(outside)?, Some(&GraphNode::Constant(value)) if value <= 0.0)
        {
            return Ok(None);
        }
        let Some(&GraphNode::Gradient {
            axis: Axis::Y,
            tiling: TilingMode::ClampToEdge,
            from,
            to,
            low,
            high,
        }) = self.resolve_references(input)?
        else {
            return Ok(None);
        };
        let (first, last) = (from.min(to), from.max(to));
        if i64::from(last) - i64::from(first) > 1 << 16 {
            return Ok(None);
        }
        // The compiled sampler's own arithmetic, at every y it varies over;
        // beyond them the clamped value is that of the nearer end.
        let gradient = GradientSampler::new(Axis::Y, TilingMode::ClampToEdge, from, to, low, high);
        let inside = |y| -> Result<bool, CompileError> {
            let value = gradient.compute(y)?;
            Ok(value >= min && value < max)
        };
        let mut bounds = [i32::MAX, i32::MIN];
        for y in first..=last {
            if inside(y)? {
                bounds[0] = bounds[0].min(y);
                bounds[1] = y;
            }
        }
        if inside(first)? {
            bounds[0] = i32::MIN;
        }
        if inside(last)? {
            bounds[1] = i32::MAX;
        }
        Ok(Some(bounds))
    }
    /// The node `id` names, past registry and direct references; `None` for
    /// a bound sampler.
    fn resolve_references(&self, mut id: FunctionId) -> Result<Option<&GraphNode>, CompileError> {
        loop {
            match &self.entry(id)?.definition {
                Definition::Node(GraphNode::Reference { input, .. }) => id = *input,
                Definition::Node(node) => return Ok(Some(node)),
                Definition::Bound { .. } => return Ok(None),
            }
        }
    }
}
