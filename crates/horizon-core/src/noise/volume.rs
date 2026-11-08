use crate::geometry::Axis;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DensityError {
    NonpositiveDimensions,
    NonpositiveSteps,
    VolumeTooLarge,
    BufferTooShort,
}
impl std::fmt::Display for DensityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NonpositiveDimensions => "density dimensions must be positive",
            Self::NonpositiveSteps => "density coordinate steps must be positive",
            Self::VolumeTooLarge => "density volume exceeds the Java array index domain",
            Self::BufferTooShort => "density buffer is smaller than its volume",
        })
    }
}
impl std::error::Error for DensityError {}

/// A bounded strided density volume. The flat layout is Z, X, Y (Y innermost).
/// Block coordinate multiplication and addition retain Java's i32 wrapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DensityVolume {
    sizes: [usize; 3],
    minimum: [i32; 3],
    steps: [i32; 3],
    size: usize,
}
impl DensityVolume {
    pub fn new(sizes: [i32; 3], minimum: [i32; 3], steps: [i32; 3]) -> Result<Self, DensityError> {
        if sizes.iter().any(|v| *v <= 0) {
            return Err(DensityError::NonpositiveDimensions);
        }
        if steps.iter().any(|v| *v <= 0) {
            return Err(DensityError::NonpositiveSteps);
        }
        let sizes = sizes.map(|v| v as usize);
        let size = sizes
            .into_iter()
            .try_fold(1_usize, |n, v| n.checked_mul(v))
            .filter(|n| *n <= i32::MAX as usize)
            .ok_or(DensityError::VolumeTooLarge)?;
        Ok(Self {
            sizes,
            minimum,
            steps,
            size,
        })
    }
    pub fn sizes(self) -> [usize; 3] {
        self.sizes
    }
    pub fn minimum(self) -> [i32; 3] {
        self.minimum
    }
    pub fn steps(self) -> [i32; 3] {
        self.steps
    }
    pub fn size(self) -> usize {
        self.size
    }
    pub fn maximum(self) -> [i32; 3] {
        std::array::from_fn(|axis| self.coordinate(axis, self.sizes[axis]).wrapping_sub(1))
    }
    pub fn index(self, position: [usize; 3]) -> Option<usize> {
        if (0..3).any(|i| position[i] >= self.sizes[i]) {
            return None;
        }
        Some(position[1] + (position[0] + position[2] * self.sizes[0]) * self.sizes[1])
    }
    pub fn block(self, axis: Axis, index: usize) -> Option<i32> {
        let axis = axis as usize;
        (index < self.sizes[axis]).then(|| self.coordinate(axis, index))
    }
    /// Find an exact lattice sample, retaining Java's wrapped subtraction and
    /// extent multiplication. Coordinates between samples are cache misses.
    pub fn index_of_block(self, position: [i32; 3]) -> Option<usize> {
        let relative: [i32; 3] = std::array::from_fn(|i| position[i].wrapping_sub(self.minimum[i]));
        for (axis, value) in relative.iter().enumerate() {
            let extent = (self.sizes[axis] as i32).wrapping_mul(self.steps[axis]);
            if *value < 0 || *value >= extent || *value % self.steps[axis] != 0 {
                return None;
            }
        }
        self.index(std::array::from_fn(|i| {
            (relative[i] / self.steps[i]) as usize
        }))
    }
    #[inline]
    pub(crate) fn coordinate(self, axis: usize, index: usize) -> i32 {
        self.minimum[axis].wrapping_add((index as i32).wrapping_mul(self.steps[axis]))
    }
}
