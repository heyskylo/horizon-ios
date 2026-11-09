use super::*;
use std::sync::Mutex;

#[test]
fn power_literal_selection_retains_signed_zeros_and_negative_fallback_behavior() {
    let signaling_base = UnarySampler {
        input: Arc::new(ConstantSampler(0.0)),
        operation: UnaryOp::PowerBase(f64::from_bits(0x7ff4_0000_2000_0000)),
    };
    assert_eq!(
        signaling_base.sample_value(&mut SamplerContext::default(), [0; 3]),
        Ok(1.0)
    );
    let mut graph = DensityGraph::new();
    let negative_zero = graph.insert(GraphNode::Constant(-0.0)).unwrap();
    let two = graph.insert(GraphNode::Constant(2.0)).unwrap();
    let half = graph.insert(GraphNode::Constant(0.5)).unwrap();
    let minus_four = graph.insert(GraphNode::Constant(-4.0)).unwrap();
    let bound_zero = graph
        .bind(
            Arc::new(ConstantSampler(-0.0)),
            Interval::exact(-0.0).unwrap(),
            7,
        )
        .unwrap();
    let bound_two = graph
        .bind(
            Arc::new(ConstantSampler(2.0)),
            Interval::exact(2.0).unwrap(),
            7,
        )
        .unwrap();
    let roots = [
        (negative_zero, half),
        (bound_zero, half),
        (two, minus_four),
        (bound_two, minus_four),
    ]
    .map(|(base, exponent)| graph.insert(GraphNode::Power { base, exponent }).unwrap());
    let cached_half = graph.insert(GraphNode::Cache(half)).unwrap();
    let invalid_raw = graph
        .insert(GraphNode::Power {
            base: two,
            exponent: cached_half,
        })
        .unwrap();
    let compiler = DensityCompiler::new(graph);
    assert_eq!(
        compiler.compile_raw(invalid_raw).unwrap_err(),
        CompileError::UnpreparedCache
    );
    for (root, expected) in roots.into_iter().zip([0.0_f32, -0.0, 0.0625, 0.0625]) {
        let value = compiler
            .compile_raw(root)
            .unwrap()
            .sample_value(&mut SamplerContext::default(), [0; 3])
            .unwrap();
        assert_eq!(value.to_bits(), expected.to_bits());
    }
    let sampler = PowerSampler {
        base: Arc::new(ConstantSampler(0.0)),
        exponent: Arc::new(FailingSampler),
    };
    assert_eq!(
        sampler.sample_value(&mut SamplerContext::default(), [0; 3]),
        Err(SampleError::ZeroGradientPeriod)
    );
    let mut output = [17.0];
    let volume = DensityVolume::new([1; 3], [0; 3], [1; 3]).unwrap();
    assert_eq!(
        sampler.sample_volume(&mut SamplerContext::default(), &mut output, volume),
        Err(SampleError::ZeroGradientPeriod)
    );
    assert_eq!(output, [0.0]);
}

#[test]
fn rounding_specializes_only_literal_one_and_preserves_input_on_multiple_failure() {
    let mut graph = DensityGraph::new();
    let input = graph.insert(GraphNode::Constant(0.5)).unwrap();
    let one = graph.insert(GraphNode::Constant(1.0)).unwrap();
    let bound = graph
        .bind(
            Arc::new(ConstantSampler(1.0)),
            Interval::exact(1.0).unwrap(),
            0,
        )
        .unwrap();
    let failing = graph
        .bind(Arc::new(FailingSampler), Interval::INFINITE, 7)
        .unwrap();
    let roots = [one, bound, failing].map(|multiple| {
        graph
            .insert(GraphNode::Round {
                mode: RoundMode::Round,
                input,
                multiple,
            })
            .unwrap()
    });
    let compiler = DensityCompiler::new(graph);
    let volume = DensityVolume::new([1; 3], [0; 3], [1; 3]).unwrap();
    let mut output = [17.0; 2];
    compiler
        .compile_raw(roots[0])
        .unwrap()
        .sample_volume(&mut SamplerContext::default(), &mut output, volume)
        .unwrap();
    assert_eq!(output, [1.0; 2]);
    assert_eq!(
        compiler.compile_raw(roots[1]).unwrap().sample_volume(
            &mut SamplerContext::default(),
            &mut output,
            volume
        ),
        Err(SampleError::ExactBufferSizeRequired)
    );
    assert_eq!(
        compiler.compile_raw(roots[2]).unwrap().sample_volume(
            &mut SamplerContext::default(),
            &mut output[..1],
            volume
        ),
        Err(SampleError::ZeroGradientPeriod)
    );
    assert_eq!(output[0], 0.5);
}

#[test]
fn end_islands_use_the_world_seed_independently_of_the_noise_algorithm() {
    let mut graph = DensityGraph::new();
    let root = graph.insert(GraphNode::EndIslands).unwrap();
    assert_eq!(graph.domain_axes(root), Ok(5));
    assert_eq!(
        graph.range(root).unwrap(),
        Interval::new(-0.84375, 0.5625).unwrap()
    );
    assert_eq!(
        DensityCompiler::new(graph.clone())
            .compile_raw(root)
            .unwrap_err(),
        CompileError::MissingSeededContext
    );
    let volume = DensityVolume::new([3, 4, 2], [-2049, -64, 4096], [17, 8, 31]).unwrap();
    let mut outputs = Vec::new();
    for legacy in [false, true] {
        let mut compiler = DensityCompiler::new_seeded(graph.clone(), i64::MIN, legacy);
        let sampler = compiler.get_sampler(root).unwrap();
        let mut output = vec![17.0; volume.size() + 1];
        sampler
            .sample_volume(&mut SamplerContext::default(), &mut output, volume)
            .unwrap();
        for column in output[..volume.size()].as_chunks::<4>().0 {
            assert!(column.iter().all(|&value| value == column[0]));
        }
        assert_eq!(output[volume.size()], 17.0);
        outputs.push(output);
    }
    assert_eq!(outputs[0], outputs[1]);
}

#[test]
fn registered_noises_share_ordinary_instances_and_recreate_nether_instances() {
    let mut builder = crate::noise::NormalNoise::builder();
    builder.set_base_octave(-3).set_octave_count(3).unwrap();
    let definition = builder.build().unwrap();
    let mut graph = DensityGraph::new();
    let ordinary = graph
        .register_noise("horizon:sample".to_owned(), definition.clone())
        .unwrap();
    assert_eq!(
        graph.register_noise("horizon:sample".to_owned(), definition.clone()),
        Err(CompileError::DuplicateNoise)
    );
    let nether = graph
        .register_noise("minecraft:nether/vegetation".to_owned(), definition)
        .unwrap();
    let unseeded = DensityCompiler::new(graph.clone());
    assert_eq!(
        unseeded.get_noise_sampler(ordinary).unwrap_err(),
        CompileError::MissingSeededContext
    );
    for legacy in [false, true] {
        let compiler = DensityCompiler::new_seeded(graph.clone(), i64::MAX, legacy);
        let a = compiler.get_noise_sampler(ordinary).unwrap();
        let b = compiler.get_noise_sampler(ordinary).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        let a = compiler.get_noise_sampler(nether).unwrap();
        let b = compiler.get_noise_sampler(nether).unwrap();
        assert!(!Arc::ptr_eq(&a, &b));
        assert_eq!(
            a.get3(-17.25, 64.0, 32.5).to_bits(),
            b.get3(-17.25, 64.0, 32.5).to_bits()
        );
    }
}

#[test]
fn noise_specialization_requires_literal_positive_zero_shifts() {
    let mut graph = DensityGraph::new();
    let noise = graph
        .register_noise(
            "horizon:empty".to_owned(),
            crate::noise::NormalNoise::builder()
                .set_octave_count(0)
                .unwrap()
                .build()
                .unwrap(),
        )
        .unwrap();
    let zero = graph.insert(GraphNode::Constant(0.0)).unwrap();
    let negative = graph.insert(GraphNode::Constant(-0.0)).unwrap();
    let roots = [zero, negative].map(|shift_x| {
        graph
            .insert(GraphNode::Noise {
                noise,
                xz_scale: 1.0,
                y_scale: 1.0,
                shift_x,
                shift_y: zero,
                shift_z: zero,
            })
            .unwrap()
    });
    let compiler = DensityCompiler::new_seeded(graph, 1, false);
    let volume = DensityVolume::new([1; 3], [0; 3], [1; 3]).unwrap();
    let mut buffer = [17.0; 2];
    compiler
        .compile_raw(roots[0])
        .unwrap()
        .sample_volume(&mut SamplerContext::default(), &mut buffer, volume)
        .unwrap();
    assert_eq!(buffer, [0.0; 2]);
    compiler
        .compile_raw(roots[1])
        .unwrap()
        .sample_volume(&mut SamplerContext::default(), &mut buffer, volume)
        .unwrap();
    assert_eq!(buffer[0].to_bits(), 0);
    assert_eq!(buffer[1].to_bits(), (-0.0_f32).to_bits());
}

#[test]
fn spline_range_early_return_does_not_skip_coordinate_validation_during_compilation() {
    let mut graph = DensityGraph::new();
    let unavailable = graph
        .bind(Arc::new(FailingSampler), Interval::NAI, 2)
        .unwrap();
    let invalid = graph.insert(GraphNode::Constant(f32::NAN)).unwrap();
    let nested = CubicSpline::multipoint(
        invalid,
        vec![SplinePoint {
            location: 0.0,
            value: CubicSpline::Constant(1.0),
            derivative: 0.0,
        }],
    )
    .unwrap();
    let root = graph
        .insert(GraphNode::Spline(
            CubicSpline::multipoint(
                unavailable,
                vec![SplinePoint {
                    location: 0.0,
                    value: nested,
                    derivative: 0.0,
                }],
            )
            .unwrap(),
        ))
        .unwrap();
    assert_eq!(graph.range(root), Ok(Interval::NAI));
    let compiler = DensityCompiler::new(graph);
    assert!(matches!(
        compiler.compile_raw(root),
        Err(CompileError::Range(_))
    ));
}

#[test]
fn spline_coordinates_share_bulk_evaluation_after_graph_rewriting() {
    let trace = Arc::new(VerticalTrace::default());
    let mut graph = DensityGraph::new();
    let coordinate = graph
        .bind(trace.clone(), Interval::new(-1.0, 1.0).unwrap(), 2)
        .unwrap();
    let inner = CubicSpline::multipoint(
        coordinate,
        vec![SplinePoint {
            location: 0.0,
            value: CubicSpline::Constant(3.0),
            derivative: 1.0,
        }],
    )
    .unwrap();
    let root = graph
        .insert(GraphNode::Spline(
            CubicSpline::multipoint(
                coordinate,
                vec![
                    SplinePoint {
                        location: -1.0,
                        value: inner.clone(),
                        derivative: 0.0,
                    },
                    SplinePoint {
                        location: 1.0,
                        value: inner,
                        derivative: 0.0,
                    },
                ],
            )
            .unwrap(),
        ))
        .unwrap();
    let mut compiler = DensityCompiler::new(graph);
    let sampler = compiler.get_sampler(root).unwrap();
    let volume = DensityVolume::new([3, 3, 2], [-1; 3], [1; 3]).unwrap();
    let mut output = vec![0.0; volume.size()];
    sampler
        .sample_volume(&mut SamplerContext::default(), &mut output, volume)
        .unwrap();
    assert_eq!(trace.0.lock().unwrap().len(), 1);
    for column in output.as_chunks::<3>().0 {
        assert_eq!(*column, [2.0, 3.0, 4.0]);
    }
}

#[test]
fn interval_threshold_equality_and_hashing_preserve_distinct_cache_inputs() {
    let mut graph = DensityGraph::new();
    let one = graph.insert(GraphNode::Constant(1.0)).unwrap();
    let zero = graph.insert(GraphNode::Constant(0.0)).unwrap();
    let shared_nan: Arc<[f32]> = Arc::from([f32::NAN]);
    let mut ids = Vec::new();
    for thresholds in [
        shared_nan.clone(),
        shared_nan,
        Arc::from([f32::NAN]),
        Arc::from([0.0]),
        Arc::from([-0.0]),
    ] {
        ids.push(
            graph
                .insert(GraphNode::IntervalSelect {
                    input: one,
                    thresholds,
                    branches: vec![zero, one],
                })
                .unwrap(),
        );
    }
    assert_eq!(ids[0], ids[1]);
    assert!(!graph.java_equals(ids[0], ids[2]).unwrap());
    assert!(graph.java_equals(ids[3], ids[4]).unwrap());
    assert_ne!(ids[3], ids[4]);
    let doubled_positive = graph
        .insert(GraphNode::Binary {
            kind: GraphBinary::Add,
            left: ids[3],
            right: ids[3],
        })
        .unwrap();
    let doubled_negative = graph
        .insert(GraphNode::Binary {
            kind: GraphBinary::Add,
            left: ids[4],
            right: ids[4],
        })
        .unwrap();
    assert_eq!(doubled_positive, doubled_negative);
    let caches = ids
        .into_iter()
        .map(|input| graph.insert(GraphNode::Cache(input)).unwrap())
        .collect::<Vec<_>>();
    let mut compiler = DensityCompiler::new(graph);
    for cache in caches {
        compiler.get_sampler(cache).unwrap();
    }
    assert_eq!(compiler.cache_count(), 4);
}

#[test]
fn structural_identity_preserves_java_float_keys_and_shared_cache_ids() {
    let mut graph = DensityGraph::new();
    let positive = graph.insert(GraphNode::Constant(0.0)).unwrap();
    let negative = graph.insert(GraphNode::Constant(-0.0)).unwrap();
    assert_ne!(positive, negative);
    let nan = graph.insert(GraphNode::Constant(f32::NAN)).unwrap();
    assert_eq!(
        nan,
        graph
            .insert(GraphNode::Constant(f32::from_bits(0xffa00001)))
            .unwrap()
    );
    let first = graph.insert(GraphNode::Cache(positive)).unwrap();
    let second = graph.insert(GraphNode::Cache(positive)).unwrap();
    assert_eq!(first, second);
    let root = graph
        .insert(GraphNode::Binary {
            kind: GraphBinary::Add,
            left: first,
            right: second,
        })
        .unwrap();
    let mut compiler = DensityCompiler::new(graph);
    assert_eq!(
        compiler.compile_raw(first).unwrap_err(),
        CompileError::UnpreparedCache
    );
    let sampler = compiler.get_sampler(root).unwrap();
    assert_eq!(compiler.cache_count(), 1);
    assert!(Arc::ptr_eq(&sampler, &compiler.get_sampler(root).unwrap()));
}

#[test]
fn failed_parent_preparation_retains_successful_child_and_consumes_new_id_on_retry() {
    let mut graph = DensityGraph::new();
    let good = graph.insert(GraphNode::Constant(1.0)).unwrap();
    let child = graph.insert(GraphNode::Cache(good)).unwrap();
    let bad = graph.insert(GraphNode::Constant(f32::NAN)).unwrap();
    let sum = graph
        .insert(GraphNode::Binary {
            kind: GraphBinary::Add,
            left: child,
            right: bad,
        })
        .unwrap();
    let root = graph.insert(GraphNode::Cache(sum)).unwrap();
    let mut compiler = DensityCompiler::new(graph);
    assert!(matches!(
        compiler.get_sampler(root),
        Err(CompileError::Range(_))
    ));
    assert_eq!(compiler.cache_count(), 2);
    assert!(matches!(
        compiler.get_sampler(root),
        Err(CompileError::Range(_))
    ));
    assert_eq!(compiler.cache_count(), 3);
    let sampler = compiler.get_sampler(child).unwrap();
    assert_eq!(compiler.cache_count(), 3);
    assert_eq!(
        sampler.sample_value(&mut SamplerContext::default(), [3; 3]),
        Ok(1.0)
    );
}

#[derive(Debug)]
struct FailingSampler;
impl DensitySampler for FailingSampler {
    fn sample_value(&self, _: &mut SamplerContext, _: [i32; 3]) -> Result<f32, SampleError> {
        Err(SampleError::ZeroGradientPeriod)
    }
}

#[test]
fn min_range_elimination_is_strict_and_does_not_skip_child_compilation() {
    let mut graph = DensityGraph::new();
    let one = graph.insert(GraphNode::Constant(1.0)).unwrap();
    let disjoint = graph
        .bind(
            Arc::new(FailingSampler),
            Interval::new(2.0, 3.0).unwrap(),
            7,
        )
        .unwrap();
    let touching = graph
        .bind(
            Arc::new(FailingSampler),
            Interval::new(1.0, 3.0).unwrap(),
            7,
        )
        .unwrap();
    let cached = graph.insert(GraphNode::Cache(disjoint)).unwrap();
    let roots: Vec<_> = [disjoint, touching, cached]
        .into_iter()
        .map(|right| {
            graph
                .insert(GraphNode::Binary {
                    kind: GraphBinary::Minimum,
                    left: one,
                    right,
                })
                .unwrap()
        })
        .collect();
    let compiler = DensityCompiler::new(graph);
    let volume = DensityVolume::new([1; 3], [0; 3], [1; 3]).unwrap();
    let mut context = SamplerContext::default();
    let mut output = [17.0];
    compiler
        .compile_raw(roots[0])
        .unwrap()
        .sample_volume(&mut context, &mut output, volume)
        .unwrap();
    assert_eq!(output, [1.0]);
    assert_eq!(
        compiler
            .compile_raw(roots[1])
            .unwrap()
            .sample_volume(&mut context, &mut output, volume),
        Err(SampleError::ZeroGradientPeriod)
    );
    assert_eq!(
        compiler.compile_raw(roots[2]).unwrap_err(),
        CompileError::UnpreparedCache
    );
}

#[derive(Debug, Default)]
struct VerticalTrace(Mutex<Vec<DensityVolume>>);
impl DensitySampler for VerticalTrace {
    fn sample_value(&self, _: &mut SamplerContext, position: [i32; 3]) -> Result<f32, SampleError> {
        Ok(position[1] as f32)
    }
    fn sample_volume(
        &self,
        _: &mut SamplerContext,
        buffer: &mut [f32],
        volume: DensityVolume,
    ) -> Result<(), SampleError> {
        self.0.lock().unwrap().push(volume);
        visit(volume, |index, position| {
            buffer[index] = position[1] as f32;
            Ok(())
        })
    }
}

#[test]
fn compiler_reduces_uniform_axes_and_preserves_strided_vertical_samples() {
    let trace = Arc::new(VerticalTrace::default());
    let mut graph = DensityGraph::new();
    let root = graph.bind(trace.clone(), Interval::INFINITE, 2).unwrap();
    let mut compiler = DensityCompiler::new(graph);
    let sampler = compiler.get_sampler(root).unwrap();
    let volume = DensityVolume::new([3, 4, 2], [-9, -7, 11], [2, 3, 5]).unwrap();
    let mut output = vec![17.0; volume.size() + 1];
    sampler
        .sample_volume(&mut SamplerContext::default(), &mut output, volume)
        .unwrap();
    assert_eq!(
        *trace.0.lock().unwrap(),
        vec![DensityVolume::new([1, 4, 1], [0, -7, 0], [2, 3, 5]).unwrap()]
    );
    for column in output[..volume.size()].as_chunks::<4>().0 {
        assert_eq!(*column, [-7.0, -4.0, -1.0, 2.0]);
    }
    assert_eq!(output[volume.size()], 17.0);
    assert_eq!(compiler.graph().domain_axes(root), Ok(2));
}

#[test]
fn positive_y_bounds_read_a_y_range_choice_with_a_nonpositive_fallback() {
    let mut graph = DensityGraph::new();
    let gradient = graph
        .insert(GraphNode::Gradient {
            axis: Axis::Y,
            tiling: TilingMode::ClampToEdge,
            from: -4064,
            to: 4062,
            low: -4064.0,
            high: 4062.0,
        })
        .unwrap();
    let y = graph
        .register_function("minecraft:y".into(), gradient)
        .unwrap();
    let noise_like = graph.insert(GraphNode::Constant(0.7)).unwrap();
    let minus_one = graph.insert(GraphNode::Constant(-1.0)).unwrap();
    let plus_one = graph.insert(GraphNode::Constant(1.0)).unwrap();
    let choice = |graph: &mut DensityGraph, input, min, max, outside| {
        graph
            .insert(GraphNode::Choice {
                input,
                min,
                max,
                inside: noise_like,
                outside,
            })
            .unwrap()
    };
    let vein = choice(&mut graph, y, 0.0, 50.0, minus_one);
    let named = graph
        .register_function("horizon:vein".into(), vein)
        .unwrap();
    assert_eq!(graph.positive_y_bounds(named), Ok(Some([0, 49])));
    let half = choice(&mut graph, y, -59.5, -8.0, minus_one);
    assert_eq!(graph.positive_y_bounds(half), Ok(Some([-59, -9])));
    // The clamped gradient stays in range below its first coordinate.
    let open = choice(&mut graph, y, f32::NEG_INFINITY, 10.0, minus_one);
    assert_eq!(graph.positive_y_bounds(open), Ok(Some([i32::MIN, 9])));
    let never = choice(&mut graph, y, 5000.0, 6000.0, minus_one);
    assert_eq!(
        graph.positive_y_bounds(never),
        Ok(Some([i32::MAX, i32::MIN]))
    );
    // A positive fallback, or an input other than y, gives no bounds.
    let positive = choice(&mut graph, y, 0.0, 50.0, plus_one);
    assert_eq!(graph.positive_y_bounds(positive), Ok(None));
    let not_y = choice(&mut graph, noise_like, 0.0, 50.0, minus_one);
    assert_eq!(graph.positive_y_bounds(not_y), Ok(None));
    assert_eq!(graph.positive_y_bounds(gradient), Ok(None));
}
