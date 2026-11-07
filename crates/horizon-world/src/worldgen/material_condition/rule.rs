//! Compiled material rule composition, sharing the condition worker's context.
use super::{
    CachedNoise, Compiled, Context, Facts, MaterialCondition, MaterialConditions,
    MaterialSurfaceQueries, PossibleBiomes, density::MaterialDensitySources,
};
use crate::{registry::BlockStateId, worldgen::DecodeError};
use horizon_core::{
    density::DensitySampler,
    noise::Noise,
    position::BlockPos,
    random::{PositionalRandom, RandomSource},
};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub enum MaterialRule {
    Block(BlockStateId),
    Reference(Arc<MaterialRule>),
    /// Density roots must be compiled for the same world and sampler context.
    OreVein {
        ore: BlockStateId,
        raw_ore: BlockStateId,
        filler: BlockStateId,
        raw_ore_chance: f32,
        density: Arc<dyn DensitySampler>,
        richness: Arc<dyn DensitySampler>,
        filler_gap: Arc<dyn DensitySampler>,
        /// Inclusive y bounds outside which `density` is never positive, so
        /// the vein places nothing there (`DensityGraph::positive_y_bounds`);
        /// `[i32::MIN, i32::MAX]` when unknown.
        positive_y: [i32; 2],
    },
    Sequence(Vec<MaterialRule>),
    Condition {
        if_true: MaterialCondition,
        then_run: Box<MaterialRule>,
    },
    /// Vanilla calls this rule "bandlands"; it queries terracotta bands.
    Bandlands,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaterialRuleError {
    UnknownRule,
    UnknownCondition,
    MissingBiomeProvider,
    BiomeQueryFailed,
    MissingBiome,
    CachedConditionUnavailable,
    MissingBandProvider,
    MissingDensityProvider,
    Density(horizon_core::density::SampleError),
    InvalidBandIndex,
}
impl std::fmt::Display for MaterialRuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Self::Density(error) = self {
            return error.fmt(f);
        }
        f.write_str(match self {
            Self::Density(_) => unreachable!(),
            Self::MissingDensityProvider => "missing material density provider",
            Self::UnknownRule => "unknown material rule",
            Self::UnknownCondition => "unknown material condition",
            Self::MissingBiomeProvider => "missing material biome provider",
            Self::BiomeQueryFailed => "material biome lookup failed",
            Self::MissingBiome => "material position has no biome",
            Self::CachedConditionUnavailable => "condition was updated without a cached result",
            Self::MissingBandProvider => "missing material band provider",
            Self::InvalidBandIndex => "negative material band index",
        })
    }
}
impl std::error::Error for MaterialRuleError {}
impl From<horizon_core::density::SampleError> for MaterialRuleError {
    fn from(error: horizon_core::density::SampleError) -> Self {
        Self::Density(error)
    }
}

pub(super) enum CompiledRule<R: RandomSource> {
    Empty,
    Block(BlockStateId),
    OreVein {
        blocks: [BlockStateId; 3],
        raw_ore_chance: f32,
        densities: [usize; 3],
        random: R::Positional,
        positive_y: [i32; 2],
    },
    Sequence(Vec<Self>),
    Condition {
        condition: Compiled<R>,
        rule: Box<Self>,
    },
    Bandlands,
}
impl<R: RandomSource> CompiledRule<R> {
    fn compile<S: MaterialSurfaceQueries>(
        rule: MaterialRule,
        root: &R::Positional,
        context: &mut Context<S>,
        depth: usize,
        nodes: &mut usize,
    ) -> Result<Self, DecodeError> {
        *nodes += 1;
        if depth >= 256 || *nodes > 65536 {
            return Err(DecodeError::Field("material rule size"));
        }
        Ok(match rule {
            MaterialRule::Block(block) => Self::Block(block),
            MaterialRule::Reference(value) => {
                return Self::compile((*value).clone(), root, context, depth + 1, nodes);
            }
            MaterialRule::OreVein {
                ore,
                raw_ore,
                filler,
                raw_ore_chance,
                density,
                richness,
                filler_gap,
                positive_y,
            } => {
                let density = context.surface.bind_density(density, true)?;
                let richness = context.surface.bind_density(richness, true)?;
                let gap = context.surface.bind_density(filler_gap, false)?;
                let random = root.from_hash_of("minecraft:ore").fork_positional();
                Self::OreVein {
                    blocks: [ore, raw_ore, filler],
                    raw_ore_chance,
                    densities: [density, richness, gap],
                    random,
                    positive_y,
                }
            }
            MaterialRule::Bandlands => Self::Bandlands,
            MaterialRule::Condition { if_true, then_run } => {
                // Compile the predicate before the nested rule, regardless of
                // whether the predicate will match any subsequent position.
                let condition = Compiled::compile(if_true, root, context, depth + 1)?;
                let rule = Box::new(Self::compile(*then_run, root, context, depth + 1, nodes)?);
                // Compile both sides first: binding and errors must retain their order.
                match condition.constant_result() {
                    Some(false) => Self::Empty,
                    Some(true) => *rule,
                    None => Self::Condition { condition, rule },
                }
            }
            MaterialRule::Sequence(mut children) => {
                if children.len() == 1 {
                    return Self::compile(children.pop().unwrap(), root, context, depth + 1, nodes);
                }
                let mut result = Vec::with_capacity(children.len());
                for child in children {
                    let child = Self::compile(child, root, context, depth + 1, nodes)?;
                    if !matches!(child, Self::Empty) {
                        result.push(child);
                    }
                }
                match result.len() {
                    0 => Self::Empty,
                    1 => result.pop().unwrap(),
                    _ => Self::Sequence(result),
                }
            }
        })
    }
    fn apply<S: MaterialSurfaceQueries>(
        &mut self,
        position: BlockPos,
        context: &mut Context<S>,
    ) -> Result<Option<BlockStateId>, MaterialRuleError> {
        match self {
            Self::Empty => Ok(None),
            Self::Block(block) => Ok(Some(*block)),
            Self::OreVein {
                blocks,
                raw_ore_chance,
                densities,
                random,
                ..
            } => {
                // Densities use the context; randomness uses application coordinates.
                let density = context.surface.density(densities[0], context.position)?;
                if density <= 0.0 {
                    return Ok(None);
                }
                let mut random = random.at(position);
                if random.next_float() > density {
                    return Ok(None);
                }
                let richness = context.surface.density(densities[1], context.position)?;
                if random.next_float() < richness
                    && context.surface.density(densities[2], context.position)? < 0.0
                {
                    Ok(Some(if random.next_float() < *raw_ore_chance {
                        blocks[1]
                    } else {
                        blocks[0]
                    }))
                } else {
                    Ok(Some(blocks[2]))
                }
            }
            Self::Bandlands => context.surface.band(position).map(Some),
            Self::Condition { condition, rule } => {
                if condition.test(context)? {
                    rule.apply(position, context)
                } else {
                    Ok(None)
                }
            }
            Self::Sequence(children) => {
                for child in children {
                    if let Some(block) = child.apply(position, context)? {
                        return Ok(Some(block));
                    }
                }
                Ok(None)
            }
        }
    }
}
impl<R: RandomSource> CompiledRule<R>
where
    R::Positional: Clone,
{
    /// This rule where `facts` hold: conditions they decide are folded away,
    /// with what can no longer produce a block, and a sequence stops at its
    /// first child that always produces one. For every position the facts
    /// cover it gives the block the whole rule gives.
    fn specialize(&self, facts: Facts) -> Self {
        match self {
            Self::Empty => Self::Empty,
            Self::Block(block) => Self::Block(*block),
            Self::Bandlands => Self::Bandlands,
            Self::OreVein {
                blocks,
                raw_ore_chance,
                densities,
                random,
                positive_y: [low, high],
            } => {
                // A non-positive density returns before the vein's
                // positional random is drawn: nothing else is consumed.
                if low > high || facts.high < *low || facts.low > *high {
                    return Self::Empty;
                }
                Self::OreVein {
                    blocks: *blocks,
                    raw_ore_chance: *raw_ore_chance,
                    densities: *densities,
                    random: random.clone(),
                    positive_y: [*low, *high],
                }
            }
            Self::Condition { condition, rule } => match condition.decided(facts) {
                Some(false) => Self::Empty,
                Some(true) => rule.specialize(facts),
                None => match rule.specialize(facts) {
                    Self::Empty => Self::Empty,
                    rule => Self::Condition {
                        condition: condition.clone(),
                        rule: Box::new(rule),
                    },
                },
            },
            Self::Sequence(children) => {
                let mut result = Vec::new();
                for child in children {
                    let child = child.specialize(facts);
                    if matches!(child, Self::Empty) {
                        continue;
                    }
                    let last = child.always_produces();
                    result.push(child);
                    if last {
                        break;
                    }
                }
                match result.len() {
                    0 => Self::Empty,
                    1 => result.pop().unwrap(),
                    _ => Self::Sequence(result),
                }
            }
        }
    }
    /// This rule as `compile` makes it for a chunk with `possible` biomes
    /// (a copy without them): the biome conditions they decide are constant,
    /// and folded away as `compile` folds them.
    fn fold_biomes(&self, possible: Option<&PossibleBiomes>) -> Self {
        match self {
            Self::Empty => Self::Empty,
            Self::Block(block) => Self::Block(*block),
            Self::Bandlands => Self::Bandlands,
            Self::OreVein {
                blocks,
                raw_ore_chance,
                densities,
                random,
                positive_y,
            } => Self::OreVein {
                blocks: *blocks,
                raw_ore_chance: *raw_ore_chance,
                densities: *densities,
                random: random.clone(),
                positive_y: *positive_y,
            },
            Self::Condition { condition, rule } => {
                let condition = match possible {
                    Some(possible) => condition.fold_biomes(possible),
                    None => condition.clone(),
                };
                let rule = rule.fold_biomes(possible);
                match condition.constant_result() {
                    Some(false) => Self::Empty,
                    Some(true) => rule,
                    None => Self::Condition {
                        condition,
                        rule: Box::new(rule),
                    },
                }
            }
            Self::Sequence(children) => {
                let mut result: Vec<_> = children
                    .iter()
                    .map(|child| child.fold_biomes(possible))
                    .filter(|child| !matches!(child, Self::Empty))
                    .collect();
                match result.len() {
                    0 => Self::Empty,
                    1 => result.pop().unwrap(),
                    _ => Self::Sequence(result),
                }
            }
        }
    }
    /// Whether applying this rule never gives `None` (only an error).
    fn always_produces(&self) -> bool {
        match self {
            Self::Block(_) | Self::Bandlands => true,
            Self::Sequence(children) => children.iter().any(Self::always_produces),
            Self::Empty | Self::OreVein { .. } | Self::Condition { .. } => false,
        }
    }
    /// The first y of each band in which this rule specialises differently
    /// from the band below, unordered, and whether the preliminary surface is
    /// asked for.
    fn inputs(&self, starts: &mut Vec<i32>, surface: &mut bool) {
        match self {
            Self::Condition { condition, rule } => {
                condition.inputs(starts, surface);
                rule.inputs(starts, surface);
            }
            Self::Sequence(children) => {
                for child in children {
                    child.inputs(starts, surface);
                }
            }
            Self::OreVein {
                positive_y: [low, high],
                ..
            } if low <= high => {
                if *low != i32::MIN {
                    starts.push(*low);
                }
                if *high != i32::MAX {
                    starts.push(high + 1);
                }
            }
            _ => {}
        }
    }
}
impl MaterialRule {
    /// The highest y at which an ore vein of this rule can place a block
    /// (`MaterialRule::OreVein::positive_y`), `i32::MIN` without veins.
    pub fn ore_vein_top(&self) -> i32 {
        self.ore_vein_top_within(0, &mut 0)
    }
    fn ore_vein_top_within(&self, depth: usize, nodes: &mut usize) -> i32 {
        *nodes += 1;
        // Larger rules fail to compile (`CompiledRule::compile`); do not
        // bound them.
        if depth >= 256 || *nodes > 65536 {
            return i32::MAX;
        }
        match self {
            Self::Block(_) | Self::Bandlands => i32::MIN,
            Self::Reference(rule) => rule.ore_vein_top_within(depth + 1, nodes),
            Self::OreVein {
                positive_y: [low, high],
                ..
            } => {
                if low > high {
                    i32::MIN
                } else {
                    *high
                }
            }
            Self::Sequence(children) => {
                let mut top = i32::MIN;
                for child in children {
                    top = top.max(child.ore_vein_top_within(depth + 1, nodes));
                }
                top
            }
            Self::Condition { then_run, .. } => then_run.ore_vein_top_within(depth + 1, nodes),
        }
    }
}
/// A rule specialised (`CompiledRule::specialize`) for each band of y between
/// its vertical gradients' bounds and its ore veins' `positive_y` bounds, and
/// below and at or above the preliminary surface when the rule asks for it:
/// most blocks of a column, deep stone, then evaluate only the few rules that
/// can apply there. Each variant is made when first asked for.
pub struct SurfaceVariants {
    /// The rule the variants specialise.
    base: usize,
    /// The first y of each band after the first, ascending.
    starts: Vec<i32>,
    /// Per band (two per band, below then above the surface, when
    /// `surface`): the variant's rule index once made, `Some(None)` when it
    /// gives no block.
    rules: Vec<Option<Option<usize>>>,
    pub surface: bool,
}
impl SurfaceVariants {
    /// The band holding `y`.
    #[inline]
    pub fn band(&self, y: i32) -> usize {
        self.starts.partition_point(|&start| start <= y)
    }
    /// The inclusive y bounds of `band`.
    pub fn band_bounds(&self, band: usize) -> [i32; 2] {
        let low = if band == 0 {
            i32::MIN
        } else {
            self.starts[band - 1]
        };
        let high = self.starts.get(band).map_or(i32::MAX, |&next| next - 1);
        [low, high]
    }
}
impl<R: RandomSource, S: MaterialSurfaceQueries> MaterialConditions<R, S>
where
    R::Positional: Clone,
{
    /// The bands of the specialised variants of rule `index`
    /// (`SurfaceVariants`); `surface_variant` makes each.
    pub fn surface_variants(&mut self, index: usize) -> Result<SurfaceVariants, MaterialRuleError> {
        let rule = self
            .rules
            .get(index)
            .ok_or(MaterialRuleError::UnknownRule)?;
        let (mut starts, mut surface) = (Vec::new(), false);
        rule.inputs(&mut starts, &mut surface);
        starts.sort_unstable();
        starts.dedup();
        let sides = if surface { 2 } else { 1 };
        Ok(SurfaceVariants {
            base: index,
            rules: vec![None; (starts.len() + 1) * sides],
            starts,
            surface,
        })
    }
    /// The rule index of the variant for `band`, `above` the preliminary
    /// surface (ignored unless `variants.surface`), or `None` when it gives
    /// no block at any position of the band.
    #[inline]
    pub fn surface_variant(
        &mut self,
        variants: &mut SurfaceVariants,
        band: usize,
        above: bool,
    ) -> Option<usize> {
        let slot = if variants.surface {
            band * 2 + usize::from(above)
        } else {
            band
        };
        if let Some(rule) = variants.rules[slot] {
            return rule;
        }
        self.make_surface_variant(variants, band, above, slot)
    }
    #[cold]
    fn make_surface_variant(
        &mut self,
        variants: &mut SurfaceVariants,
        band: usize,
        above: bool,
        slot: usize,
    ) -> Option<usize> {
        let [low, high] = variants.band_bounds(band);
        let facts = Facts {
            low,
            high,
            above_surface: variants.surface.then_some(above),
        };
        let rule = match self.rules[variants.base].specialize(facts) {
            CompiledRule::Empty => None,
            rule => {
                self.rules.push(rule);
                Some(self.rules.len() - 1)
            }
        };
        variants.rules[slot] = Some(rule);
        rule
    }
}
/// A world's surface rule compiled once for every chunk of a generator
/// worker: the rule before any chunk's possible biomes fold it, the noises it
/// bound, and the densities (ore veins) it binds to each chunk's volume, in
/// the order it bound them.
pub struct CompiledSurfaceRule<R: RandomSource> {
    rule: CompiledRule<R>,
    noises: Vec<((String, bool), Arc<dyn Noise>)>,
    densities: Vec<(Arc<dyn DensitySampler>, bool)>,
}
/// The densities `CompiledRule::compile` binds for `rule`, in its order,
/// with whether it samples each over the volume now.
fn density_bindings(rule: &MaterialRule, out: &mut Vec<(Arc<dyn DensitySampler>, bool)>) {
    match rule {
        MaterialRule::Reference(value) => density_bindings(value, out),
        MaterialRule::OreVein {
            density,
            richness,
            filler_gap,
            ..
        } => out.extend([
            (Arc::clone(density), true),
            (Arc::clone(richness), true),
            (Arc::clone(filler_gap), false),
        ]),
        MaterialRule::Condition { then_run, .. } => density_bindings(then_run, out),
        MaterialRule::Sequence(children) => {
            for child in children {
                density_bindings(child, out);
            }
        }
        MaterialRule::Block(_) | MaterialRule::Bandlands => {}
    }
}
impl<R: RandomSource, S: MaterialSurfaceQueries> MaterialConditions<R, S>
where
    R::Positional: Clone,
{
    /// `add_rule`, compiling `rule` only the first time `shared` is used:
    /// later workers take the compiled rule and its noises, bind its densities
    /// to their own volume in the same order, and fold it with their own
    /// possible biomes as `compile` does. A worker that already holds rules or
    /// noises, or a possible biome the surface doesn't number, compiles as
    /// `add_rule` does.
    pub fn add_shared_rule(
        &mut self,
        rule: &MaterialRule,
        shared: &mut Option<CompiledSurfaceRule<R>>,
    ) -> Result<usize, DecodeError> {
        let fresh = self.rules.is_empty() && self.context.noises.is_empty();
        if !fresh || self.rules.len() >= 8192 {
            return self.add_rule(rule.clone());
        }
        let mut ids = Vec::new();
        if let Some(names) = &self.context.possible_biomes {
            for name in names {
                let Some(Some(id)) = self.context.surface.biome_index(name) else {
                    return self.add_rule(rule.clone());
                };
                let word = id as usize / 64;
                if ids.len() <= word {
                    ids.resize(word + 1, 0);
                }
                ids[word] |= 1 << (id % 64);
            }
        }
        match shared {
            Some(shared) => {
                for (key, sampler) in &shared.noises {
                    let is_3d = key.1;
                    let index = self.context.noises.len();
                    self.context.noises.push(CachedNoise {
                        sampler: Arc::clone(sampler),
                        is_3d,
                        revision: if is_3d {
                            self.context.block_revision
                        } else {
                            self.context.column_revision
                        }
                        .wrapping_sub(1),
                        value: 0.0,
                    });
                    self.context.noise_indices.insert(key.clone(), index);
                }
                for (sampler, eager) in &shared.densities {
                    self.context
                        .surface
                        .bind_density(Arc::clone(sampler), *eager)?;
                }
            }
            None => {
                let possible = self.context.possible_biomes.take();
                let compiled =
                    CompiledRule::compile(rule.clone(), &self.root, &mut self.context, 0, &mut 0);
                self.context.possible_biomes = possible;
                let compiled = compiled?;
                let mut noises: Vec<_> = self
                    .context
                    .noise_indices
                    .iter()
                    .map(|(key, &index)| (index, key.clone()))
                    .collect();
                noises.sort_unstable_by_key(|(index, _)| *index);
                let noises = noises
                    .into_iter()
                    .map(|(index, key)| (key, Arc::clone(&self.context.noises[index].sampler)))
                    .collect();
                let mut densities = Vec::new();
                density_bindings(rule, &mut densities);
                *shared = Some(CompiledSurfaceRule {
                    rule: compiled,
                    noises,
                    densities,
                });
            }
        }
        let shared = shared.as_ref().expect("compiled above");
        let possible = self
            .context
            .possible_biomes
            .as_ref()
            .map(|names| PossibleBiomes { names, ids });
        let folded = shared.rule.fold_biomes(possible.as_ref());
        let index = self.rules.len();
        self.rules.push(folded);
        Ok(index)
    }
}
impl<R: RandomSource, T: MaterialSurfaceQueries> MaterialConditions<R, MaterialDensitySources<T>> {
    /// Re-target the per-cell density bindings for a reused worker.
    pub fn retarget_surface_densities(&mut self, position: [i32; 3]) -> Result<(), DecodeError> {
        self.context.surface.retarget(position)
    }
}
impl<R: RandomSource, S: MaterialSurfaceQueries> MaterialConditions<R, S> {
    /// Compile all children in order, even after an unconditional block rule.
    /// Failed compilation never adds a partial rule to this worker.
    pub fn add_rule(&mut self, rule: MaterialRule) -> Result<usize, DecodeError> {
        if self.rules.len() >= 8192 {
            return Err(DecodeError::Field("material rule count"));
        }
        let compiled = CompiledRule::compile(rule, &self.root, &mut self.context, 0, &mut 0)?;
        let index = self.rules.len();
        self.rules.push(compiled);
        Ok(index)
    }
    /// Predicates read the worker's updated context. Position arguments are
    /// forwarded to block-producing rules independently, as in vanilla.
    pub fn try_apply(
        &mut self,
        index: usize,
        position: BlockPos,
    ) -> Result<Option<BlockStateId>, MaterialRuleError> {
        self.rules
            .get_mut(index)
            .ok_or(MaterialRuleError::UnknownRule)?
            .apply(position, &mut self.context)
    }
}
