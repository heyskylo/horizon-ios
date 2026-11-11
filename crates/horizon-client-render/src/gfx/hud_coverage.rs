//! HUD coverage beside its unchanged colour and alpha, only on the float HDR lane.

pub(crate) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
/// The HUD's own target on the layered frame (`Hdr::hud`): 8-bit, so each HUD draw clips at
/// white and rounds as on vanilla's frame. The coverage pipelines are the only ones that draw
/// on it.
pub(crate) const HUD_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

pub(crate) fn supported(
    attachments: u32,
    flags: wgpu::DownlevelFlags,
    format: wgpu::TextureFormatFeatures,
) -> bool {
    attachments >= 2
        && flags.contains(wgpu::DownlevelFlags::INDEPENDENT_BLEND)
        && format
            .allowed_usages
            .contains(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING)
        && format
            .flags
            .contains(wgpu::TextureFormatFeatureFlags::BLENDABLE)
}

pub(crate) fn attachment(view: &wgpu::TextureView) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        resolve_target: None,
        depth_slice: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Load,
            store: wgpu::StoreOp::Store,
        },
    }
}

pub(crate) fn clear(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
    let mut color = attachment(view);
    color.ops.load = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("HUD coverage reset"),
        color_attachments: &[Some(color)],
        ..Default::default()
    });
}

pub(crate) struct Pipeline {
    plain: wgpu::RenderPipeline,
    coverage: Option<wgpu::RenderPipeline>,
}

impl Pipeline {
    /// `None` when `enabled` and the shader can't be given a coverage output (a resource
    /// pack's, which the caller then doesn't use).
    pub(crate) fn new(
        device: &wgpu::Device,
        desc: &wgpu::RenderPipelineDescriptor<'_>,
        source: wgpu::ShaderSource<'_>,
        enabled: bool,
    ) -> Option<Self> {
        let coverage = if enabled {
            Some(pipeline(device, desc, source)?)
        } else {
            None
        };
        Some(Self {
            plain: crate::gfx::render_pipeline(device, desc),
            coverage,
        })
    }

    pub(crate) fn get(&self, coverage: bool) -> &wgpu::RenderPipeline {
        if coverage {
            self.coverage.as_ref().expect("HUD coverage pipeline")
        } else {
            &self.plain
        }
    }
}

/// Keep the primary output and all its blend factors, including destination alpha, on the
/// HUD's target (`HUD_FORMAT`). Wrap the final shader result, so pack text's alpha, early
/// returns and discard all apply to coverage.
/// `None` for a fragment shader without a colour output at location 0.
pub(super) fn pipeline(
    device: &wgpu::Device,
    desc: &wgpu::RenderPipelineDescriptor<'_>,
    source: wgpu::ShaderSource<'_>,
) -> Option<wgpu::RenderPipeline> {
    let mut desc = desc.clone();
    let fragment = desc.fragment.as_mut()?;
    let mut module = match source {
        wgpu::ShaderSource::Wgsl(s) => naga::front::wgsl::parse_str(&s).ok()?,
        wgpu::ShaderSource::Naga(m) => m.into_owned(),
        _ => return None,
    };
    let mut primary = fragment.targets.first()?.clone()?;
    primary.format = HUD_FORMAT;
    let blend = primary.blend.unwrap_or(wgpu::BlendState::REPLACE).color;
    let additive = matches!(
        (blend.src_factor, blend.dst_factor),
        (
            wgpu::BlendFactor::One
                | wgpu::BlendFactor::Src
                | wgpu::BlendFactor::SrcAlpha
                | wgpu::BlendFactor::Zero,
            wgpu::BlendFactor::One
        )
    ) && blend.operation == wgpu::BlendOperation::Add;
    wrap(&mut module, fragment.entry_point?, blend)?;
    let shader = crate::gfx::shader_module(
        device,
        wgpu::ShaderModuleDescriptor {
            label: desc.label,
            source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(module)),
        },
    );
    let over = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrc,
        operation: wgpu::BlendOperation::Add,
    };
    let targets = [
        Some(primary),
        Some(wgpu::ColorTargetState {
            format: FORMAT,
            blend: Some(wgpu::BlendState {
                color: over,
                alpha: over,
            }),
            write_mask: if additive {
                wgpu::ColorWrites::empty()
            } else {
                wgpu::ColorWrites::RED
            },
        }),
    ];
    fragment.module = &shader;
    fragment.targets = &targets;
    Some(crate::gfx::render_pipeline(device, &desc))
}

fn wrap(module: &mut naga::Module, name: &str, blend: wgpu::BlendComponent) -> Option<()> {
    use naga::{Expression as E, Statement as S, TypeInner as T};
    use wgpu::BlendFactor as B;
    let span = naga::Span::UNDEFINED;
    let entry = module.entry_points.iter_mut().find(|e| e.name == name)?;
    // Checked before anything is taken apart: a resource pack's shader may have no colour.
    let result = entry.function.result.clone()?;
    if let T::Struct { members, .. } = &module.types[result.ty].inner {
        members
            .iter()
            .find(|m| matches!(m.binding, Some(naga::Binding::Location { location: 0, .. })))?;
    } else if !matches!(
        result.binding,
        Some(naga::Binding::Location { location: 0, .. })
    ) {
        return None;
    }
    let mut original = std::mem::take(&mut entry.function);
    entry.function.arguments = original.arguments.clone();
    for arg in &mut original.arguments {
        arg.binding = None;
    }
    original.result.as_mut().unwrap().binding = None;
    original.name = Some("hud_color".into());
    let function = module.functions.append(original, span);
    let f = &mut entry.function;
    let args = (0..f.arguments.len())
        .map(|i| f.expressions.append(E::FunctionArgument(i as u32), span))
        .collect();
    let call = f.expressions.append(E::CallResult(function), span);
    f.body.push(
        S::Call {
            function,
            arguments: args,
            result: Some(call),
        },
        span,
    );
    let one = f
        .expressions
        .append(E::Literal(naga::Literal::F32(1.0)), span);
    let start = f.expressions.len();
    let (mut members, mut values, color, offset) = match &module.types[result.ty].inner {
        T::Struct {
            members,
            span: size,
        } => {
            // HUD pipelines have one target, so a pack's other colour outputs went nowhere;
            // left in, one at location 1 would collide with the coverage.
            let (members, values): (Vec<_>, Vec<_>) = members
                .iter()
                .enumerate()
                .filter(|(_, m)| {
                    !matches!(m.binding, Some(naga::Binding::Location { location, .. }) if location != 0)
                })
                .map(|(i, m)| {
                    let value = f.expressions.append(
                        E::AccessIndex {
                            base: call,
                            index: i as u32,
                        },
                        span,
                    );
                    (m.clone(), value)
                })
                .unzip();
            let color = members
                .iter()
                .position(|m| {
                    matches!(m.binding, Some(naga::Binding::Location { location: 0, .. }))
                })
                .unwrap();
            let color = values[color];
            (members, values, color, *size)
        }
        _ => (
            vec![naga::StructMember {
                name: Some("color".into()),
                ty: result.ty,
                binding: result.binding,
                offset: 0,
            }],
            vec![call],
            call,
            16,
        ),
    };
    let mut component = |index| {
        f.expressions
            .append(E::AccessIndex { base: color, index }, span)
    };
    let [r, g, b, a] = [component(0), component(1), component(2), component(3)];
    let mut math = |fun, arg, arg1| {
        f.expressions.append(
            E::Math {
                fun,
                arg,
                arg1: Some(arg1),
                arg2: None,
                arg3: None,
            },
            span,
        )
    };
    let low = math(naga::MathFunction::Min, r, g);
    let low = math(naga::MathFunction::Min, low, b);
    let high = math(naga::MathFunction::Max, r, g);
    let high = math(naga::MathFunction::Max, high, b);
    // A scalar cannot reproduce RGB-dependent replacement or inversion: surviving pixels
    // cover fully, so the later composite cannot leak world light through them.
    let c = if blend.operation != wgpu::BlendOperation::Add {
        one
    } else {
        match (blend.src_factor, blend.dst_factor) {
            (B::SrcAlpha | B::One, B::OneMinusSrcAlpha) => a,
            (B::Zero, B::OneMinusSrc) => high,
            // Multiplying retains src.rgb (or 1 - alpha + src.rgb) of the backdrop.
            // Use the greatest channel darkening, as for the credits vignette above.
            (B::Zero, B::Src)
            | (B::Dst, B::Zero)
            | (B::Dst, B::OneMinusSrcAlpha)
            | (B::Dst, B::Src) => {
                let top = if blend.dst_factor == B::OneMinusSrcAlpha {
                    a
                } else {
                    one
                };
                let factor = if blend.dst_factor == B::Src && blend.src_factor == B::Dst {
                    f.expressions.append(
                        E::Binary {
                            op: naga::BinaryOperator::Add,
                            left: low,
                            right: low,
                        },
                        span,
                    )
                } else {
                    low
                };
                f.expressions.append(
                    E::Binary {
                        op: naga::BinaryOperator::Subtract,
                        left: top,
                        right: factor,
                    },
                    span,
                )
            }
            _ => one,
        }
    };
    let c = f.expressions.append(
        E::Math {
            fun: naga::MathFunction::Saturate,
            arg: c,
            arg1: None,
            arg2: None,
            arg3: None,
        },
        span,
    );
    let scalar = module.types.insert(
        naga::Type {
            name: None,
            inner: T::Scalar(naga::Scalar::F32),
        },
        span,
    );
    members.push(naga::StructMember {
        name: Some("coverage".into()),
        ty: scalar,
        binding: Some(naga::Binding::Location {
            location: 1,
            interpolation: None,
            sampling: None,
            blend_src: None,
            per_primitive: false,
        }),
        offset,
    });
    let ty = module.types.insert(
        naga::Type {
            name: Some("HudCoverage".into()),
            inner: T::Struct {
                members,
                span: (offset + 4).next_multiple_of(16),
            },
        },
        span,
    );
    values.push(c);
    let value = f.expressions.append(
        E::Compose {
            ty,
            components: values,
        },
        span,
    );
    f.result = Some(naga::FunctionResult { ty, binding: None });
    f.body.push(S::Emit(f.expressions.range_from(start)), span);
    f.body.push(S::Return { value: Some(value) }, span);
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_requires_two_attachments_and_independent_r8_blending() {
        let flags = wgpu::DownlevelFlags::INDEPENDENT_BLEND;
        let format = wgpu::TextureFormatFeatures {
            allowed_usages: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING,
            flags: wgpu::TextureFormatFeatureFlags::BLENDABLE,
        };
        assert!(supported(2, flags, format));
        assert!(!supported(1, flags, format));
        assert!(!supported(2, wgpu::DownlevelFlags::empty(), format));
        assert!(!supported(
            2,
            flags,
            wgpu::TextureFormatFeatures {
                flags: wgpu::TextureFormatFeatureFlags::empty(),
                ..format
            }
        ));
        assert!(!supported(
            2,
            flags,
            wgpu::TextureFormatFeatures {
                allowed_usages: wgpu::TextureUsages::TEXTURE_BINDING,
                ..format
            }
        ));
        assert!(!supported(
            2,
            flags,
            wgpu::TextureFormatFeatures {
                allowed_usages: wgpu::TextureUsages::RENDER_ATTACHMENT,
                ..format
            }
        ));
    }

    #[test]
    fn disabled_coverage_builds_only_the_original_pipeline() {
        let instance = crate::gfx::test_gpu::instance();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            return;
        };
        let Ok((device, _)) = pollster::block_on(adapter.request_device(&Default::default()))
        else {
            return;
        };
        let shader = crate::gfx::shader_module(
            &device,
            wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(
                    "@vertex fn vs() -> @builtin(position) vec4<f32> { return vec4(0.0); }".into(),
                ),
            },
        );
        let desc = wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        };
        // The coverage source must not even be parsed outside the lane.
        let pipeline = Pipeline::new(
            &device,
            &desc,
            wgpu::ShaderSource::Wgsl("invalid".into()),
            false,
        )
        .unwrap();
        assert!(pipeline.coverage.is_none());
    }

    #[test]
    fn coverage_wraps_the_debug_crosshair_without_changing_world_entry_points() {
        let mut module = naga::front::wgsl::parse_str(&crate::gfx::terrain_shader()).unwrap();
        wrap(
            &mut module,
            "fs_debug_line",
            wgpu::BlendState::ALPHA_BLENDING.color,
        )
        .unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }

    #[test]
    fn coverage_replaces_a_colour_output_the_hud_has_no_target_for() {
        let mut module = naga::front::wgsl::parse_str(
            "struct Out { @location(0) color: vec4<f32>, @location(1) unused: vec4<f32> }
             @fragment fn fs() -> Out { return Out(vec4(0.5), vec4(1.0)); }",
        )
        .unwrap();
        wrap(&mut module, "fs", wgpu::BlendState::ALPHA_BLENDING.color).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
        let result = module.entry_points[0].function.result.as_ref().unwrap();
        let naga::TypeInner::Struct { members, .. } = &module.types[result.ty].inner else {
            panic!("the wrapped entry returns a struct");
        };
        let names: Vec<_> = members.iter().map(|m| m.name.as_deref().unwrap()).collect();
        assert_eq!(names, ["color", "coverage"]);
    }
}
