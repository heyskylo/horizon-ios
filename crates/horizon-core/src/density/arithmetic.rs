use super::*;

pub(super) fn java_power(base: f64, exponent: f64) -> f64 {
    // Native libm can propagate a signaling NaN base even for exponent zero;
    // Java returns one for every base in this case.
    if exponent == 0.0 {
        1.0
    } else if exponent.is_nan() || (base.abs() == 1.0 && exponent.is_infinite()) {
        f64::NAN
    } else {
        crate::jmath::pow(base, exponent)
    }
}

pub(super) fn java_min(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == 0.0 && b == 0.0 {
        f32::from_bits(a.to_bits() | b.to_bits())
    } else if a <= b {
        a
    } else {
        b
    }
}
pub(super) fn java_max(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == 0.0 && b == 0.0 {
        f32::from_bits(a.to_bits() & b.to_bits())
    } else if a >= b {
        a
    } else {
        b
    }
}
fn clamp(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else {
        java_min(value, max)
    }
}

/// Selected compiled unary samplers, including constant-operand specializations.
/// Graph compilation is implemented separately.
#[derive(Clone, Copy, Debug)]
pub enum UnaryOp {
    Abs,
    Negate,
    Square,
    Cube,
    Sqrt,
    Log,
    PowerBase(f64),
    PowerExponent(f64),
    Reciprocal,
    Sign,
    Squeeze,
    LeakyRelu(f32),
    Add(f32),
    SubtractFrom(f32),
    Multiply(f32),
    DivideInto(f32),
    Minimum(f32),
    Maximum(f32),
    ConstantRangeChoice {
        min_inclusive: f32,
        max_exclusive: f32,
        inside: f32,
        outside: f32,
    },
    Clamp {
        min: f32,
        max: f32,
    },
}
// One expression definition for scalar dispatch and specialized bulk loops.
macro_rules! unary_apply {
    ($operation:expr, $value:expr, $bulk:expr) => {{
        let value = $value;
        let bulk = $bulk;
        match $operation {
            Self::Abs => value.abs(),
            Self::Negate => -value,
            Self::Square => value * value,
            Self::Cube => value * value * value,
            Self::Sqrt => (value as f64).sqrt() as f32,
            Self::Log => crate::jmath::log(value as f64) as f32,
            Self::PowerBase(base) => java_power(base, value as f64) as f32,
            Self::PowerExponent(exponent) => java_power(value as f64, exponent) as f32,
            Self::Reciprocal => 1.0 / value,
            Self::Sign => {
                if value == 0.0 || value.is_nan() {
                    value
                } else {
                    value.signum()
                }
            }
            Self::Squeeze => {
                let v = clamp(value, -1.0, 1.0);
                v / 2.0 - v * v * v / 24.0
            }
            Self::LeakyRelu(factor) => {
                if value > 0.0 {
                    value
                } else {
                    value * factor
                }
            }
            Self::Add(right) => value + right,
            Self::SubtractFrom(left) => left - value,
            Self::Multiply(right) => value * right,
            Self::DivideInto(left) => left / value,
            Self::Minimum(right) => {
                if bulk {
                    if right < value { right } else { value }
                } else {
                    java_min(value, right)
                }
            }
            Self::Maximum(right) => {
                if bulk {
                    if right > value { right } else { value }
                } else {
                    java_max(value, right)
                }
            }
            Self::Clamp { min, max } => clamp(value, min, max),
            Self::ConstantRangeChoice {
                min_inclusive,
                max_exclusive,
                inside,
                outside,
            } => {
                if value >= min_inclusive && value < max_exclusive {
                    inside
                } else {
                    outside
                }
            }
        }
    }};
}
impl UnaryOp {
    pub(super) fn apply(self, value: f32, bulk: bool) -> f32 {
        unary_apply!(self, value, bulk)
    }
    /// Dispatch once per volume so each loop has one fixed arithmetic operation.
    /// Keep the scalar expression shared, including the bulk min/max behavior.
    fn apply_bulk(self, buffer: &mut [f32]) {
        // Specialization can reverse which NaN operand supplies the payload.
        // Retain the original dispatch for non-finite constant operands.
        let non_finite = match self {
            Self::PowerBase(value) | Self::PowerExponent(value) => !value.is_finite(),
            Self::LeakyRelu(value)
            | Self::Add(value)
            | Self::SubtractFrom(value)
            | Self::Multiply(value)
            | Self::DivideInto(value)
            | Self::Minimum(value)
            | Self::Maximum(value) => !value.is_finite(),
            Self::Clamp { min, max } => !min.is_finite() || !max.is_finite(),
            Self::ConstantRangeChoice {
                min_inclusive,
                max_exclusive,
                inside,
                outside,
            } => {
                !min_inclusive.is_finite()
                    || !max_exclusive.is_finite()
                    || !inside.is_finite()
                    || !outside.is_finite()
            }
            _ => false,
        };
        if non_finite {
            for value in buffer {
                *value = self.apply(*value, true);
            }
            return;
        }
        macro_rules! apply {
            ($operation:expr) => {
                for value in buffer {
                    *value = unary_apply!($operation, *value, true);
                }
            };
        }
        match self {
            Self::Abs => apply!(Self::Abs),
            Self::Negate => apply!(Self::Negate),
            Self::Square => apply!(Self::Square),
            Self::Cube => apply!(Self::Cube),
            Self::Sqrt => apply!(Self::Sqrt),
            Self::Log => apply!(Self::Log),
            Self::PowerBase(base) => apply!(Self::PowerBase(base)),
            Self::PowerExponent(exponent) => apply!(Self::PowerExponent(exponent)),
            Self::Reciprocal => apply!(Self::Reciprocal),
            Self::Sign => apply!(Self::Sign),
            Self::Squeeze => apply!(Self::Squeeze),
            Self::LeakyRelu(factor) => apply!(Self::LeakyRelu(factor)),
            Self::Add(right) => apply!(Self::Add(right)),
            Self::SubtractFrom(left) => apply!(Self::SubtractFrom(left)),
            Self::Multiply(right) => apply!(Self::Multiply(right)),
            Self::DivideInto(left) => apply!(Self::DivideInto(left)),
            Self::Minimum(right) => apply!(Self::Minimum(right)),
            Self::Maximum(right) => apply!(Self::Maximum(right)),
            Self::ConstantRangeChoice {
                min_inclusive,
                max_exclusive,
                inside,
                outside,
            } => apply!(Self::ConstantRangeChoice {
                min_inclusive,
                max_exclusive,
                inside,
                outside
            }),
            Self::Clamp { min, max } => apply!(Self::Clamp { min, max }),
        }
    }
}
#[derive(Clone, Debug)]
pub struct UnarySampler {
    pub input: Arc<dyn DensitySampler>,
    pub operation: UnaryOp,
}
impl DensitySampler for UnarySampler {
    fn pointwise_inputs(&self) -> Option<Vec<&Arc<dyn DensitySampler>>> {
        Some(vec![&self.input])
    }
    fn sample_value(
        &self,
        context: &mut SamplerContext,
        position: [i32; 3],
    ) -> Result<f32, SampleError> {
        Ok(self
            .operation
            .apply(self.input.sample_value(context, position)?, false))
    }
    fn sample_volume(
        &self,
        context: &mut SamplerContext,
        buffer: &mut [f32],
        volume: DensityVolume,
    ) -> Result<(), SampleError> {
        check_buffer(buffer, volume)?;
        self.input.sample_volume(context, buffer, volume)?;
        self.operation.apply_bulk(buffer);
        Ok(())
    }
}

/// Range bounds are the values baked into vanilla's compiled min/max samplers.
/// Computing those bounds from a density graph is a separate compiler step.
#[derive(Clone, Copy, Debug)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Minimum { right_min: f32 },
    Maximum { right_max: f32 },
}
impl BinaryOp {
    fn apply(self, left: f32, right: f32, bulk: bool) -> f32 {
        match self {
            Self::Add => left + right,
            Self::Subtract => left - right,
            Self::Multiply => left * right,
            Self::Divide => left / right,
            Self::Minimum { .. } => {
                if bulk {
                    if right < left { right } else { left }
                } else {
                    java_min(left, right)
                }
            }
            Self::Maximum { .. } => {
                if bulk {
                    if right > left { right } else { left }
                } else {
                    java_max(left, right)
                }
            }
        }
    }
    fn apply_bulk(self, buffer: &mut [f32], right: &[f32]) {
        macro_rules! apply {
            ($operation:expr) => {
                for (left, &right) in buffer.iter_mut().zip(right) {
                    *left = ($operation)(*left, right);
                }
            };
        }
        match self {
            Self::Add => apply!(|left, right| left + right),
            Self::Subtract => apply!(|left, right| left - right),
            Self::Multiply => apply!(|left, right| left * right),
            Self::Divide => apply!(|left, right| left / right),
            Self::Minimum { .. } => apply!(|left, right| if right < left { right } else { left }),
            Self::Maximum { .. } => apply!(|left, right| if right > left { right } else { left }),
        }
    }
}
#[derive(Clone, Debug)]
pub struct BinarySampler {
    pub left: Arc<dyn DensitySampler>,
    pub right: Arc<dyn DensitySampler>,
    pub operation: BinaryOp,
}
impl DensitySampler for BinarySampler {
    fn pointwise_inputs(&self) -> Option<Vec<&Arc<dyn DensitySampler>>> {
        Some(vec![&self.left, &self.right])
    }
    fn sample_value(
        &self,
        context: &mut SamplerContext,
        position: [i32; 3],
    ) -> Result<f32, SampleError> {
        let left = self.left.sample_value(context, position)?;
        match self.operation {
            BinaryOp::Multiply | BinaryOp::Divide if left == 0.0 => return Ok(0.0),
            BinaryOp::Minimum { right_min } if left <= right_min => return Ok(left),
            BinaryOp::Maximum { right_max } if left >= right_max => return Ok(left),
            _ => {}
        }
        Ok(self
            .operation
            .apply(left, self.right.sample_value(context, position)?, false))
    }
    fn sample_volume(
        &self,
        context: &mut SamplerContext,
        buffer: &mut [f32],
        volume: DensityVolume,
    ) -> Result<(), SampleError> {
        check_exact_buffer(buffer, volume)?;
        // Vanilla reads a volume-sized temporary through every output index.
        // Restrict this API to the valid equal-size case before invoking children.
        self.left.sample_volume(context, buffer, volume)?;
        context.with_buffer(volume.size(), |context, right| {
            self.right.sample_volume(context, right, volume)?;
            self.operation.apply_bulk(buffer, right);
            Ok(())
        })
    }
}

#[cfg(test)]
mod bulk_tests {
    use super::*;

    #[inline(never)]
    fn dispatched_reference(operation: UnaryOp, value: f32) -> f32 {
        operation.apply(value, true)
    }

    #[test]
    fn fixed_operation_loops_preserve_float_bits() {
        let mut operations = vec![
            UnaryOp::Abs,
            UnaryOp::Negate,
            UnaryOp::Square,
            UnaryOp::Cube,
            UnaryOp::Sqrt,
            UnaryOp::Log,
            UnaryOp::Reciprocal,
            UnaryOp::Sign,
            UnaryOp::Squeeze,
        ];
        for factor in [0.0, -0.0, 0.5, -1.0, f32::INFINITY, f32::NAN] {
            operations.extend([
                UnaryOp::LeakyRelu(factor),
                UnaryOp::Add(factor),
                UnaryOp::SubtractFrom(factor),
                UnaryOp::Multiply(factor),
                UnaryOp::DivideInto(factor),
                UnaryOp::Minimum(factor),
                UnaryOp::Maximum(factor),
                UnaryOp::Clamp {
                    min: -factor,
                    max: factor,
                },
                UnaryOp::ConstantRangeChoice {
                    min_inclusive: -factor,
                    max_exclusive: factor,
                    inside: -0.0,
                    outside: 17.0,
                },
            ]);
        }
        for exponent in [0.0, -0.0, 0.5, -1.0, 2.0, 3.0, f64::INFINITY, f64::NAN] {
            operations.extend([
                UnaryOp::PowerBase(exponent),
                UnaryOp::PowerExponent(exponent),
            ]);
        }
        let mut values: Vec<f32> = [
            0,
            0x8000_0000,
            1,
            0x8000_0001,
            0x007f_ffff,
            0x0080_0000,
            0x3f00_0000,
            0xbf00_0000,
            0x3f80_0000,
            0xbf80_0000,
            0x7f7f_ffff,
            0xff7f_ffff,
            0x7f80_0000,
            0xff80_0000,
            0x7fc0_0000,
            0x7fa0_0001,
            0xffc0_1234,
        ]
        .into_iter()
        .map(f32::from_bits)
        .collect();
        let mut bits = 0x484f_5249u32;
        for _ in 0..4096 {
            bits ^= bits << 13;
            bits ^= bits >> 17;
            bits ^= bits << 5;
            values.push(f32::from_bits(bits));
        }
        for operation in operations {
            for length in [0, 1, 3, 16, 257, values.len()] {
                let mut actual = values[..length].to_vec();
                operation.apply_bulk(&mut actual);
                for (i, (&input, actual)) in values.iter().zip(actual).enumerate() {
                    let expected = dispatched_reference(operation, input);
                    assert_eq!(
                        actual.to_bits(),
                        expected.to_bits(),
                        "{operation:?}, index {i}, input {input:?}"
                    );
                }
            }
        }
    }

    #[inline(never)]
    fn dispatched_binary_reference(operation: BinaryOp, left: f32, right: f32) -> f32 {
        operation.apply(left, right, true)
    }

    #[test]
    fn fixed_binary_loops_preserve_float_bits() {
        let edges = [
            0,
            0x8000_0000,
            1,
            0x8000_0001,
            0x007f_ffff,
            0x0080_0000,
            0x3f80_0000,
            0xbf80_0000,
            0x7f7f_ffff,
            0xff7f_ffff,
            0x7f80_0000,
            0xff80_0000,
            0x7fc0_0000,
            0x7fa0_0001,
            0xffc0_1234,
        ];
        let mut left = Vec::new();
        let mut right = Vec::new();
        for a in edges {
            for b in edges {
                left.push(f32::from_bits(a));
                right.push(f32::from_bits(b));
            }
        }
        let mut bits = 0x484f_5249u32;
        for _ in 0..8192 {
            bits ^= bits << 13;
            bits ^= bits >> 17;
            bits ^= bits << 5;
            left.push(f32::from_bits(bits));
            bits ^= bits << 13;
            bits ^= bits >> 17;
            bits ^= bits << 5;
            right.push(f32::from_bits(bits));
        }
        for operation in [
            BinaryOp::Add,
            BinaryOp::Subtract,
            BinaryOp::Multiply,
            BinaryOp::Divide,
            BinaryOp::Minimum {
                right_min: f32::NEG_INFINITY,
            },
            BinaryOp::Maximum {
                right_max: f32::INFINITY,
            },
        ] {
            for length in [0, 1, 3, 16, 257, left.len()] {
                let mut actual = left[..length].to_vec();
                operation.apply_bulk(&mut actual, &right[..length]);
                for (i, actual) in actual.into_iter().enumerate() {
                    let expected = dispatched_binary_reference(operation, left[i], right[i]);
                    assert_eq!(
                        actual.to_bits(),
                        expected.to_bits(),
                        "{operation:?}, index {i}"
                    );
                }
            }
        }
    }
}
