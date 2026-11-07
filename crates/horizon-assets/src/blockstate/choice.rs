//! Prepared model choices with vanilla random draw consumption.
use super::{BlockStateDefinition, Variant, VariantSet, bind::SelectedModels};
use crate::json::{Result, error};
use horizon_core::random::RandomSource;

#[derive(Clone, Debug)]
enum Choice<T> {
    Single(T),
    Weighted {
        variants: Vec<T>,
        ends: Vec<i32>,
        total: i32,
    },
}
impl Choice<Variant> {
    fn new(value: &VariantSet) -> Result<Self> {
        match value {
            VariantSet::Single(value) => Ok(Self::Single(value.clone())),
            VariantSet::Weighted(entries) => {
                let mut total = 0i32;
                let mut ends = Vec::with_capacity(entries.len());
                let mut variants = Vec::with_capacity(entries.len());
                for (variant, weight) in entries {
                    if *weight <= 0 {
                        return Err(error("nonpositive model weight"));
                    }
                    total = total
                        .checked_add(*weight)
                        .ok_or_else(|| error("model weight overflow"))?;
                    ends.push(total);
                    variants.push(variant.clone());
                }
                if total == 0 {
                    return Err(error("empty weighted model"));
                }
                Ok(Self::Weighted {
                    variants,
                    ends,
                    total,
                })
            }
        }
    }
}
impl<T> Choice<T> {
    fn map<U>(&self, f: &mut impl FnMut(&T) -> U) -> Choice<U> {
        match self {
            Self::Single(value) => Choice::Single(f(value)),
            Self::Weighted {
                variants,
                ends,
                total,
            } => Choice::Weighted {
                variants: variants.iter().map(&mut *f).collect(),
                ends: ends.clone(),
                total: *total,
            },
        }
    }
    fn select(&self, random: &mut impl RandomSource) -> &T {
        match self {
            Self::Single(value) => value,
            Self::Weighted {
                variants,
                ends,
                total,
            } => {
                // A singleton weighted list still consumes the bounded draw.
                let index = random
                    .next_int_bound(*total)
                    .expect("validated positive model weight");
                &variants[ends.partition_point(|end| *end <= index)]
            }
        }
    }
}
/// Compile once per definition and share across its bound block states. Prefix
/// sums avoid rebuilding weight totals or scanning all entries during selection.
/// `map` swaps each variant for what it stands for (its baked model).
#[derive(Clone, Debug)]
pub struct CompiledChoices<T = Variant> {
    variants: Vec<Choice<T>>,
    multipart: Vec<Choice<T>>,
}
impl CompiledChoices {
    pub fn new(definition: &BlockStateDefinition) -> Result<Self> {
        Ok(Self {
            variants: definition
                .variants
                .iter()
                .flatten()
                .map(|(_, v)| Choice::new(v))
                .collect::<Result<_>>()?,
            multipart: definition
                .multipart
                .iter()
                .flatten()
                .map(|s| Choice::new(&s.variant))
                .collect::<Result<_>>()?,
        })
    }
}
impl<T> CompiledChoices<T> {
    /// The same choices, with `f` of each variant in its place.
    pub fn map<U>(&self, mut f: impl FnMut(&T) -> U) -> CompiledChoices<U> {
        CompiledChoices {
            variants: self.variants.iter().map(|c| c.map(&mut f)).collect(),
            multipart: self.multipart.iter().map(|c| c.map(&mut f)).collect(),
        }
    }
    /// A variant that needs no random selection and produces exactly one model.
    pub fn single_variant(&self, selected: &SelectedModels) -> Option<&T> {
        let SelectedModels::Variant(index) = selected else {
            return None;
        };
        match self.variants.get(*index)? {
            Choice::Single(value) => Some(value),
            Choice::Weighted { .. } => None,
        }
    }
    /// The variant `particleMaterial` comes from: the first entry of a weighted
    /// list (`WeightedVariants`) and, for a multipart root, of its first
    /// selector whether or not that selector applies (`MultiPartModel`).
    pub fn particle_variant(&self, selected: &SelectedModels) -> Option<&T> {
        let choice = match selected {
            SelectedModels::Variant(index) => self.variants.get(*index)?,
            SelectedModels::Multipart(_) => self.multipart.first()?,
        };
        Some(match choice {
            Choice::Single(value) => value,
            Choice::Weighted { variants, .. } => variants.first()?,
        })
    }
    /// Append selected variants without allocating intermediate part lists.
    pub fn collect<'a>(
        &'a self,
        selected: &SelectedModels,
        random: &mut impl RandomSource,
        output: &mut Vec<&'a T>,
    ) -> Result<()> {
        self.for_each(selected, random, |v| output.push(v))
    }
    /// Each selected variant, in order. A multipart root consumes nextLong
    /// even for an empty selected subset, then resets the same random source
    /// to that seed before each selected part.
    pub fn for_each<'a>(
        &'a self,
        selected: &SelectedModels,
        random: &mut impl RandomSource,
        mut f: impl FnMut(&'a T),
    ) -> Result<()> {
        match selected {
            SelectedModels::Variant(index) => {
                let choice = self
                    .variants
                    .get(*index)
                    .ok_or_else(|| error("invalid variant selection"))?;
                f(choice.select(random));
            }
            SelectedModels::Multipart(indices) => {
                if indices.iter().any(|index| *index >= self.multipart.len()) {
                    return Err(error("invalid multipart selection"));
                }
                let seed = random.next_long();
                for index in indices {
                    random.set_seed(seed);
                    f(self.multipart[*index].select(random));
                }
            }
        }
        Ok(())
    }
}
