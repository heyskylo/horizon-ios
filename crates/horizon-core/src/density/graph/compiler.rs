use super::*;

/// A compiler and cache-ID namespace for one immutable set of density inputs.
/// Share resulting samplers across workers, with a separate SamplerContext per
/// worker. Context cache cells must not be reused across compiler namespaces.
#[derive(Debug)]
pub struct DensityCompiler {
    graph: DensityGraph,
    samplers: HashMap<FunctionId, Arc<dyn DensitySampler>>,
    prepared: HashMap<FunctionId, FunctionId>,
    next_cache: usize,
    context: Option<seeded::SeededContext>,
    /// Distinguishes this compiler's function ids from other compilers'.
    instance: u64,
}
impl DensityCompiler {
    pub fn new(graph: DensityGraph) -> Self {
        static INSTANCES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        Self {
            instance: INSTANCES.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            graph,
            samplers: HashMap::new(),
            prepared: HashMap::new(),
            next_cache: 0,
            context: None,
        }
    }
    pub fn new_seeded(graph: DensityGraph, seed: i64, use_legacy_random: bool) -> Self {
        let mut compiler = Self::new(graph);
        compiler.context = Some(seeded::SeededContext::new(seed, use_legacy_random));
        compiler
    }
    pub fn graph(&self) -> &DensityGraph {
        &self.graph
    }
    pub fn graph_mut(&mut self) -> &mut DensityGraph {
        &mut self.graph
    }
    pub fn cache_count(&self) -> usize {
        self.next_cache
    }
    /// Resolve a registered noise using this compiler's fixed world seed.
    /// Ordinary names reuse their sampler; Nether biome compatibility names
    /// create a fresh sampler from their dedicated legacy seed on each call.
    pub fn get_noise_sampler(&self, noise: NoiseId) -> Result<Arc<dyn Noise>, CompileError> {
        self.context
            .as_ref()
            .ok_or(CompileError::MissingSeededContext)?
            .noise(&self.graph, noise)
    }
    pub fn get_sampler(&mut self, id: FunctionId) -> Result<Arc<dyn DensitySampler>, CompileError> {
        if let Some(sampler) = self.samplers.get(&id) {
            return Ok(sampler.clone());
        }
        let optimized = self.optimize(id)?;
        let sampler = self.compile_raw(optimized)?;
        self.samplers.insert(id, sampler.clone());
        Ok(sampler)
    }
    fn rewrite_children(
        &mut self,
        id: FunctionId,
        mut rewrite: impl FnMut(&mut Self, FunctionId) -> Result<FunctionId, CompileError>,
    ) -> Result<FunctionId, CompileError> {
        let Definition::Node(mut node) = self.graph.entry(id)?.definition.clone() else {
            return Ok(id);
        };
        if let GraphNode::Reference { input, .. } = node {
            let rewritten = rewrite(self, input)?;
            return Ok(if rewritten == input { id } else { rewritten });
        }
        for child in node.children_mut() {
            *child = rewrite(self, *child)?;
        }
        self.graph.insert(node)
    }
    fn optimize(&mut self, id: FunctionId) -> Result<FunctionId, CompileError> {
        let prepared = self.prepare(id)?;
        self.slice_uniform_axes(prepared, 7)
    }
    fn prepare(&mut self, mut id: FunctionId) -> Result<FunctionId, CompileError> {
        // Vanilla INLINE_REFERENCE unwraps one holder before detecting caches.
        // Nested holders still follow their own rewriteChildren identity rule.
        if let Definition::Node(GraphNode::Reference { input, .. }) =
            self.graph.entry(id)?.definition
        {
            id = input;
        }
        if let Definition::Node(GraphNode::Cache(input)) = self.graph.entry(id)?.definition.clone()
        {
            if let Some(prepared) = self.prepared.get(&input) {
                return Ok(*prepared);
            }
            let cache_id = self.next_cache;
            self.next_cache += 1;
            let optimized = self.optimize(input)?;
            let sampler = self.compile_raw(optimized)?;
            let sampler = Arc::new(CachingDensitySampler::new(cache_id, sampler)?);
            let range = self.graph.range(optimized)?;
            let axes = self.graph.domain_axes(optimized)?;
            let prepared = self.graph.bind(sampler, range, axes)?;
            self.prepared.insert(input, prepared);
            return Ok(prepared);
        }
        self.rewrite_children(id, Self::prepare)
    }
    fn slice_uniform_axes(
        &mut self,
        id: FunctionId,
        parent_axes: u8,
    ) -> Result<FunctionId, CompileError> {
        if matches!(
            self.graph.entry(id)?.definition,
            Definition::Node(GraphNode::Constant(_) | GraphNode::Gradient { .. })
        ) {
            return Ok(id);
        }
        let axes = self.graph.domain_axes(id)?;
        let mut result = self.rewrite_children(id, |compiler, child| {
            compiler.slice_uniform_axes(child, axes)
        })?;
        if axes == parent_axes {
            return Ok(result);
        }
        let mut existing = 0;
        let mut cursor = result;
        while let Definition::Node(GraphNode::Slice { axis, input, .. }) =
            self.graph.entry(cursor)?.definition
        {
            existing |= 1 << axis as u8;
            cursor = input;
        }
        let remove = parent_axes & !axes & !existing;
        // X then Z places these slices adjacent for the XZ sampler specialization.
        for axis in [Axis::X, Axis::Z, Axis::Y] {
            if remove & (1 << axis as u8) != 0 {
                result = self.graph.insert(GraphNode::Slice {
                    axis,
                    coordinate: 0,
                    input: result,
                })?;
            }
        }
        Ok(result)
    }
    fn constant(&self, id: FunctionId) -> Option<f32> {
        match self.graph.entries[id.0].definition {
            Definition::Node(GraphNode::Constant(value)) => Some(value),
            _ => None,
        }
    }
    /// Compile exactly the supplied definition, without rewriting. Cache nodes
    /// require get_sampler's preparation pass and are rejected here.
    pub fn compile_raw(&self, id: FunctionId) -> Result<Arc<dyn DensitySampler>, CompileError> {
        let node = match &self.graph.entry(id)?.definition {
            Definition::Bound { sampler, .. } => return Ok(sampler.clone()),
            Definition::Node(node) => node,
        };
        let sampler: Arc<dyn DensitySampler> = match *node {
            GraphNode::Constant(value) => Arc::new(ConstantSampler(value)),
            GraphNode::Reference { input, .. } => self.compile_raw(input)?,
            GraphNode::WorldField(field) => Arc::new(ContextBoundSampler::new(field)),
            GraphNode::BlendDensity(input) => Arc::new(BlendDensitySampler {
                input: self.compile_raw(input)?,
            }),
            GraphNode::EndIslands => self
                .context
                .as_ref()
                .ok_or(CompileError::MissingSeededContext)?
                .end_islands(),
            GraphNode::Noise {
                noise,
                xz_scale,
                y_scale,
                shift_x,
                shift_y,
                shift_z,
            } => {
                let noise = self
                    .context
                    .as_ref()
                    .ok_or(CompileError::MissingSeededContext)?
                    .noise(&self.graph, noise)?;
                let zero = |id: FunctionId| matches!(self.graph.entries[id.0].definition, Definition::Node(GraphNode::Constant(value)) if value.to_bits() == 0);
                if zero(shift_x) && zero(shift_y) && zero(shift_z) {
                    Arc::new(NoiseSampler {
                        noise,
                        xz_scale,
                        y_scale,
                    })
                } else {
                    // Compilation order differs from the XYZ sampler's sampling
                    // order: vanilla compiles X, then Z, then optional Y.
                    let x = self.compile_raw(shift_x)?;
                    let z = self.compile_raw(shift_z)?;
                    let y = if zero(shift_y) {
                        None
                    } else {
                        Some(self.compile_raw(shift_y)?)
                    };
                    Arc::new(ShiftedNoiseSampler {
                        source: NoiseSampler {
                            noise,
                            xz_scale,
                            y_scale,
                        },
                        shift_x: x,
                        shift_y: y,
                        shift_z: z,
                    })
                }
            }
            GraphNode::OffsetNoise { noise, kind } => compile_shift_noise(
                self.context
                    .as_ref()
                    .ok_or(CompileError::MissingSeededContext)?
                    .noise(&self.graph, noise)?,
                kind,
            ),
            GraphNode::BlendedNoise(definition) => self
                .context
                .as_ref()
                .ok_or(CompileError::MissingSeededContext)?
                .blended(definition)?,
            GraphNode::Spline(ref spline) => {
                let mut ids = HashMap::new();
                let mut coordinates = Vec::new();
                let compiled = spline.try_map_coordinates(&mut |id| {
                    if let Some(index) = ids.get(&id) {
                        return Ok::<usize, CompileError>(*index);
                    }
                    let sampler = self.compile_raw(id)?;
                    // Vanilla validates each unique coordinate range immediately
                    // after compiling it, even if sampling never selects it.
                    self.graph.range(id)?;
                    let index = coordinates.len();
                    coordinates.push(sampler);
                    ids.insert(id, index);
                    Ok(index)
                })?;
                Arc::new(SplineSampler::new(&compiled, &coordinates)?)
            }
            GraphNode::DistanceToPoint { point, metric } => {
                Arc::new(DistanceToPointSampler { point, metric })
            }
            GraphNode::IntervalSelect {
                input,
                ref thresholds,
                ref branches,
            } => {
                let input = self.compile_raw(input)?;
                let branches = branches
                    .iter()
                    .map(|id| self.compile_raw(*id))
                    .collect::<Result<Vec<_>, _>>()?;
                Arc::new(
                    IntervalSelectSampler::new(input, thresholds.to_vec(), branches)
                        .map_err(|_| CompileError::InvalidIntervals)?,
                )
            }
            GraphNode::Gradient {
                axis,
                tiling,
                from,
                to,
                low,
                high,
            } => Arc::new(GradientSampler::new(axis, tiling, from, to, low, high)),
            GraphNode::Unary { kind, input } => Arc::new(UnarySampler {
                input: self.compile_raw(input)?,
                operation: kind.operation(),
            }),
            GraphNode::Round {
                mode,
                input,
                multiple,
            } => {
                let input = self.compile_raw(input)?;
                let multiple = if self.constant(multiple) == Some(1.0) {
                    None
                } else {
                    Some(self.compile_raw(multiple)?)
                };
                Arc::new(RoundSampler {
                    mode,
                    input,
                    multiple,
                })
            }
            GraphNode::Power { base, exponent } => {
                // Both children compile before literal selection, even when one
                // compiled child will be discarded by the specialization.
                let compiled_base = self.compile_raw(base)?;
                let compiled_exponent = self.compile_raw(exponent)?;
                if let Some(value) = self.constant(base) {
                    Arc::new(UnarySampler {
                        input: compiled_exponent,
                        operation: UnaryOp::PowerBase(value as f64),
                    })
                } else if let Some(value) = self.constant(exponent) {
                    compile_constant_exponent(compiled_base, value)
                } else {
                    Arc::new(PowerSampler {
                        base: compiled_base,
                        exponent: compiled_exponent,
                    })
                }
            }
            GraphNode::Binary { kind, left, right } => {
                return self.compile_binary(kind, left, right);
            }
            GraphNode::Clamp { input, min, max } => Arc::new(UnarySampler {
                input: self.compile_raw(input)?,
                operation: UnaryOp::Clamp { min, max },
            }),
            GraphNode::Choice {
                input,
                min,
                max,
                inside,
                outside,
            } => {
                let input = self.compile_raw(input)?;
                if let (Some(inside), Some(outside)) =
                    (self.constant(inside), self.constant(outside))
                {
                    Arc::new(UnarySampler {
                        input,
                        operation: UnaryOp::ConstantRangeChoice {
                            min_inclusive: min,
                            max_exclusive: max,
                            inside,
                            outside,
                        },
                    })
                } else {
                    Arc::new(RangeChoiceSampler {
                        input,
                        min_inclusive: min,
                        max_exclusive: max,
                        when_in_range: self.compile_raw(inside)?,
                        when_out_of_range: self.compile_raw(outside)?,
                    })
                }
            }
            GraphNode::Lerp {
                alpha,
                first,
                second,
            } => {
                let alpha = self.compile_raw(alpha)?;
                let first_sampler = self.compile_raw(first)?;
                let second_sampler = self.compile_raw(second)?;
                let (first, second) = if let Some(value) = self.constant(first) {
                    (
                        DensityValue::Constant(value),
                        DensityValue::Sampler(second_sampler),
                    )
                } else if let Some(value) = self.constant(second) {
                    (
                        DensityValue::Sampler(first_sampler),
                        DensityValue::Constant(value),
                    )
                } else {
                    (
                        DensityValue::Sampler(first_sampler),
                        DensityValue::Sampler(second_sampler),
                    )
                };
                Arc::new(LerpSampler {
                    alpha,
                    first,
                    second,
                })
            }
            GraphNode::Slice {
                axis,
                coordinate,
                input,
            } => {
                let mut axes = match axis {
                    Axis::X => SliceAxes::X(coordinate),
                    Axis::Y => SliceAxes::Y(coordinate),
                    Axis::Z => SliceAxes::Z(coordinate),
                };
                let mut child = input;
                if let Definition::Node(GraphNode::Slice {
                    axis: inner_axis,
                    coordinate: inner_coordinate,
                    input: inner,
                }) = self.graph.entry(input)?.definition
                {
                    if axis == Axis::X && inner_axis == Axis::Z {
                        axes = SliceAxes::Xz {
                            x: coordinate,
                            z: inner_coordinate,
                        };
                        child = inner;
                    } else if axis == Axis::Z && inner_axis == Axis::X {
                        axes = SliceAxes::Xz {
                            x: inner_coordinate,
                            z: coordinate,
                        };
                        child = inner;
                    }
                }
                Arc::new(SliceSampler {
                    axes,
                    input: self.compile_raw(child)?,
                })
            }
            GraphNode::Cache(_) => return Err(CompileError::UnpreparedCache),
            GraphNode::Interpolated { input, xz, y } => Arc::new(
                InterpolatedSampler::new(self.compile_raw(input)?, xz, y)?
                    .with_identity(self.instance, id.0),
            ),
            GraphNode::Surface {
                density,
                upper,
                lower,
                height,
            } => Arc::new(compile_surface_search(
                self.compile_raw(density)?,
                self.compile_raw(upper)?,
                lower,
                height,
            )?),
        };
        Ok(sampler)
    }
    fn compile_binary(
        &self,
        kind: GraphBinary,
        left_id: FunctionId,
        right_id: FunctionId,
    ) -> Result<Arc<dyn DensitySampler>, CompileError> {
        let left = self.compile_raw(left_id)?;
        let right = self.compile_raw(right_id)?;
        let unary = |input, operation| {
            Arc::new(UnarySampler { input, operation }) as Arc<dyn DensitySampler>
        };
        let bounds = if matches!(kind, GraphBinary::Minimum | GraphBinary::Maximum) {
            let a = self.graph.range(left_id)?;
            let b = self.graph.range(right_id)?;
            if (kind == GraphBinary::Minimum && a.max() < b.min())
                || (kind == GraphBinary::Maximum && a.min() > b.max())
            {
                return Ok(left);
            }
            if (kind == GraphBinary::Minimum && b.max() < a.min())
                || (kind == GraphBinary::Maximum && b.min() > a.max())
            {
                return Ok(right);
            }
            Some(b)
        } else {
            None
        };
        if let Some(value) = self.constant(left_id) {
            return Ok(unary(
                right,
                match kind {
                    GraphBinary::Add => UnaryOp::Add(value),
                    GraphBinary::Subtract => UnaryOp::SubtractFrom(value),
                    GraphBinary::Multiply => UnaryOp::Multiply(value),
                    GraphBinary::Divide => UnaryOp::DivideInto(value),
                    GraphBinary::Minimum => UnaryOp::Minimum(value),
                    GraphBinary::Maximum => UnaryOp::Maximum(value),
                },
            ));
        }
        if let Some(value) = self.constant(right_id) {
            return Ok(unary(
                left,
                match kind {
                    GraphBinary::Add => UnaryOp::Add(value),
                    GraphBinary::Subtract => UnaryOp::Add(-value),
                    GraphBinary::Multiply => UnaryOp::Multiply(value),
                    GraphBinary::Divide => UnaryOp::Multiply(1.0 / value),
                    GraphBinary::Minimum => UnaryOp::Minimum(value),
                    GraphBinary::Maximum => UnaryOp::Maximum(value),
                },
            ));
        }
        let operation = match kind {
            GraphBinary::Add => BinaryOp::Add,
            GraphBinary::Subtract => BinaryOp::Subtract,
            GraphBinary::Multiply => BinaryOp::Multiply,
            GraphBinary::Divide => BinaryOp::Divide,
            GraphBinary::Minimum => BinaryOp::Minimum {
                right_min: bounds.unwrap().min(),
            },
            GraphBinary::Maximum => BinaryOp::Maximum {
                right_max: bounds.unwrap().max(),
            },
        };
        Ok(Arc::new(BinarySampler {
            left,
            right,
            operation,
        }))
    }
}
