use super::*;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplineError {
    EmptyPoints,
    NestingTooDeep,
    MissingCoordinate,
    NanConstant,
}
impl std::fmt::Display for SplineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NanConstant => "a compiled constant spline cannot be NaN",
            Self::EmptyPoints => "a multipoint spline requires at least one point",
            Self::NestingTooDeep => "spline nesting exceeds 256 levels",
            Self::MissingCoordinate => "spline coordinate has no density sampler",
        })
    }
}
impl std::error::Error for SplineError {}

#[derive(Clone, Debug)]
pub struct SplinePoint<C = usize> {
    pub location: f32,
    pub value: CubicSpline<C>,
    pub derivative: f32,
}
#[derive(Clone, Debug)]
pub struct MultipointSpline<C = usize> {
    pub(super) coordinate: C,
    pub(super) points: Vec<SplinePoint<C>>,
    pub(super) depth: usize,
}
/// An immutable spline definition. Coordinate indices are resolved against a
/// sampler table at compilation. Point order is retained, including NaNs and
/// equal locations; codec ordering constraints belong to serialized definitions.
#[derive(Clone, Debug)]
pub enum CubicSpline<C = usize> {
    Constant(f32),
    Multipoint(Arc<MultipointSpline<C>>),
}
impl<C: Copy> CubicSpline<C> {
    pub fn multipoint(coordinate: C, points: Vec<SplinePoint<C>>) -> Result<Self, SplineError> {
        let depth = points
            .iter()
            .map(|p| match &p.value {
                Self::Constant(_) => 0,
                Self::Multipoint(node) => node.depth,
            })
            .max()
            .ok_or(SplineError::EmptyPoints)?
            + 1;
        if depth > 256 {
            return Err(SplineError::NestingTooDeep);
        }
        Ok(Self::Multipoint(Arc::new(MultipointSpline {
            coordinate,
            points,
            depth,
        })))
    }
    fn sample(
        &self,
        coordinate: &mut impl FnMut(C) -> Result<f32, SampleError>,
    ) -> Result<f32, SampleError> {
        let node = match self {
            Self::Constant(value) => return Ok(*value),
            Self::Multipoint(node) => node,
        };
        let input = coordinate(node.coordinate)?;
        // Vanilla binarySearch finds the first strictly greater location. Keep
        // this search even for manually supplied unsorted or NaN locations.
        let mut first = 0;
        let mut remaining = node.points.len();
        while remaining != 0 {
            let half = remaining / 2;
            let middle = first + half;
            if input < node.points[middle].location {
                remaining = half;
            } else {
                first = middle + 1;
                remaining -= half + 1;
            }
        }
        if first == 0 || first == node.points.len() {
            let endpoint = if first == 0 { 0 } else { first - 1 };
            let point = &node.points[endpoint];
            let value = point.value.sample(coordinate)?;
            return Ok(if point.derivative == 0.0 {
                value
            } else {
                value + point.derivative * (input - point.location)
            });
        }
        let left = &node.points[first - 1];
        let right = &node.points[first];
        let width = right.location - left.location;
        let alpha = (input - left.location) / width;
        let left_value = left.value.sample(coordinate)?;
        let right_value = right.value.sample(coordinate)?;
        let left_correction = left.derivative * width - (right_value - left_value);
        let right_correction = -right.derivative * width + (right_value - left_value);
        let linear = left_value + alpha * (right_value - left_value);
        let correction = left_correction + alpha * (right_correction - left_correction);
        Ok(linear + (alpha * (1.0 - alpha)) * correction)
    }
    /// Vanilla's conservative range analysis, with coordinate ranges supplied
    /// by the enclosing graph. Child ranges are evaluated in reference order.
    pub fn range(
        &self,
        coordinate: &mut impl FnMut(C) -> Interval,
    ) -> Result<Interval, IntervalError> {
        self.try_range(&mut |id| Ok(coordinate(id)))
    }
    /// Fallible coordinate range lookup, preserving early returns and error order.
    pub fn try_range<E: From<IntervalError>>(
        &self,
        coordinate: &mut impl FnMut(C) -> Result<Interval, E>,
    ) -> Result<Interval, E> {
        use super::arithmetic::{java_max, java_min};
        let node = match self {
            Self::Constant(value) => return Interval::exact(*value).map_err(E::from),
            Self::Multipoint(node) => node,
        };
        let input = coordinate(node.coordinate)?;
        if input.is_nai() {
            return Ok(input);
        }
        let mut minimum = f32::INFINITY;
        let mut maximum = f32::NEG_INFINITY;
        let first = &node.points[0];
        let last = node.points.last().unwrap();
        let extend = |point: &SplinePoint<C>, input: f32, value: f32| {
            if point.derivative == 0.0 {
                value
            } else {
                value + point.derivative * (input - point.location)
            }
        };
        if input.min() < first.location {
            let value = first.value.try_range(coordinate)?;
            let low = extend(first, input.min(), value.min());
            let high = extend(first, input.min(), value.max());
            minimum = java_min(minimum, java_min(low, high));
            maximum = java_max(maximum, java_max(low, high));
        }
        if input.max() > last.location {
            let value = last.value.try_range(coordinate)?;
            let low = extend(last, input.max(), value.min());
            let high = extend(last, input.max(), value.max());
            minimum = java_min(minimum, java_min(low, high));
            maximum = java_max(maximum, java_max(low, high));
        }
        let ranges = node
            .points
            .iter()
            .map(|point| point.value.try_range(coordinate))
            .collect::<Result<Vec<_>, _>>()?;
        for range in &ranges {
            minimum = java_min(minimum, range.min());
            maximum = java_max(maximum, range.max());
        }
        for (points, ranges) in node.points.windows(2).zip(ranges.windows(2)) {
            let width = points[1].location - points[0].location;
            let left = ranges[0];
            let right = ranges[1];
            let derivative_left = points[0].derivative;
            let derivative_right = points[1].derivative;
            if derivative_left == 0.0 && derivative_right == 0.0 {
                continue;
            }
            let tangent_left = derivative_left * width;
            let tangent_right = derivative_right * width;
            let value_min = java_min(left.min(), right.min());
            let value_max = java_max(left.max(), right.max());
            let low_left = tangent_left - right.max() + left.min();
            let high_left = tangent_left - right.min() + left.max();
            let low_right = -tangent_right + right.min() - left.max();
            let high_right = -tangent_right + right.max() - left.min();
            minimum = java_min(minimum, value_min + 0.25 * java_min(low_left, low_right));
            maximum = java_max(maximum, value_max + 0.25 * java_max(high_left, high_right));
        }
        Interval::new(minimum, maximum).map_err(E::from)
    }
    pub(super) fn coordinates_mut(&mut self) -> Vec<&mut C> {
        match self {
            Self::Constant(_) => Vec::new(),
            Self::Multipoint(node) => {
                let node = Arc::make_mut(node);
                let mut coordinates = vec![&mut node.coordinate];
                for point in &mut node.points {
                    coordinates.extend(point.value.coordinates_mut());
                }
                coordinates
            }
        }
    }
    pub(super) fn try_map_coordinates<D: Copy, E>(
        &self,
        map: &mut impl FnMut(C) -> Result<D, E>,
    ) -> Result<CubicSpline<D>, E> {
        Ok(match self {
            Self::Constant(value) => CubicSpline::Constant(*value),
            Self::Multipoint(node) => {
                let coordinate = map(node.coordinate)?;
                let points = node
                    .points
                    .iter()
                    .map(|point| {
                        Ok(SplinePoint {
                            location: point.location,
                            value: point.value.try_map_coordinates(map)?,
                            derivative: point.derivative,
                        })
                    })
                    .collect::<Result<Vec<_>, E>>()?;
                CubicSpline::Multipoint(Arc::new(MultipointSpline {
                    coordinate,
                    points,
                    depth: node.depth,
                }))
            }
        })
    }
}
impl CubicSpline {
    fn compile(
        &self,
        inputs: &[Arc<dyn DensitySampler>],
        ids: &mut HashMap<usize, usize>,
        coordinates: &mut Vec<Arc<dyn DensitySampler>>,
    ) -> Result<Self, SplineError> {
        match self {
            Self::Constant(value) => Ok(Self::Constant(*value)),
            Self::Multipoint(node) => {
                let coordinate = if let Some(index) = ids.get(&node.coordinate) {
                    *index
                } else {
                    let input = inputs
                        .get(node.coordinate)
                        .ok_or(SplineError::MissingCoordinate)?;
                    let index = coordinates.len();
                    coordinates.push(input.clone());
                    ids.insert(node.coordinate, index);
                    index
                };
                let points = node
                    .points
                    .iter()
                    .map(|point| {
                        Ok(SplinePoint {
                            location: point.location,
                            derivative: point.derivative,
                            value: point.value.compile(inputs, ids, coordinates)?,
                        })
                    })
                    .collect::<Result<Vec<_>, SplineError>>()?;
                Self::multipoint(coordinate, points)
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct SplineSampler {
    spline: CubicSpline,
    coordinates: Vec<Arc<dyn DensitySampler>>,
}
impl SplineSampler {
    /// Equal coordinate IDs share evaluation results. Structural function
    /// identity and deduplication are supplied by the density graph compiler.
    pub fn new(
        spline: &CubicSpline,
        inputs: &[Arc<dyn DensitySampler>],
    ) -> Result<Self, SplineError> {
        if matches!(spline, CubicSpline::Constant(value) if value.is_nan()) {
            return Err(SplineError::NanConstant);
        }
        let mut coordinates = Vec::new();
        let spline = spline.compile(inputs, &mut HashMap::new(), &mut coordinates)?;
        Ok(Self {
            spline,
            coordinates,
        })
    }
    pub fn coordinate_count(&self) -> usize {
        self.coordinates.len()
    }
}
impl DensitySampler for SplineSampler {
    fn pointwise_inputs(&self) -> Option<Vec<&Arc<dyn DensitySampler>>> {
        Some(self.coordinates.iter().collect())
    }
    fn sample_value(
        &self,
        context: &mut SamplerContext,
        position: [i32; 3],
    ) -> Result<f32, SampleError> {
        context.with_buffer(self.coordinates.len(), |context, values| {
            values.fill(f32::NAN);
            self.spline.sample(&mut |id| {
                if values[id].is_nan() {
                    values[id] = self.coordinates[id].sample_value(context, position)?;
                }
                Ok(values[id])
            })
        })
    }
    fn sample_volume(
        &self,
        context: &mut SamplerContext,
        output: &mut [f32],
        volume: DensityVolume,
    ) -> Result<(), SampleError> {
        check_buffer(output, volume)?;
        if let CubicSpline::Constant(value) = self.spline {
            output.fill(value);
            return Ok(());
        }
        check_exact_buffer(output, volume)?;
        let mut buffers: Vec<Option<Vec<f32>>> =
            (0..self.coordinates.len()).map(|_| None).collect();
        let result = (|| {
            for (index, value) in output.iter_mut().enumerate() {
                *value = self.spline.sample(&mut |id| {
                    if buffers[id].is_none() {
                        let mut buffer = context.scratch.pop().unwrap_or_default();
                        buffer.resize(volume.size(), 0.0);
                        if let Err(error) =
                            self.coordinates[id].sample_volume(context, &mut buffer, volume)
                        {
                            context.scratch.push(buffer);
                            return Err(error);
                        }
                        buffers[id] = Some(buffer);
                    }
                    Ok(buffers[id].as_ref().unwrap()[index])
                })?;
            }
            Ok(())
        })();
        // Release in coordinate-ID order, also after a later lazy child fails.
        context.scratch.extend(buffers.into_iter().flatten());
        result
    }
}
