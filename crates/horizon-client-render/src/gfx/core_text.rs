//! A resource pack's `core/text` on world text, as vanilla 26.3's `TEXT_POLYGON_OFFSET` and
//! `TEXT_SEE_THROUGH` draw text displays, name tags and their backgrounds.

use crate::core_shader::{CoreProgram, Glue};
use std::sync::Arc;

/// The entity vertices' light bytes are sky then block; vanilla's `UV2` is block then sky.
pub(crate) const WORLD_GLUE: Glue = Glue {
    inputs: &[
        ("Position", "vec3"),
        ("Color", "vec4"),
        ("UV0", "vec2"),
        ("UV2", "ivec2"),
    ],
    prelude: "layout(location = 0) in vec3 hz_pos;\n\
              layout(location = 1) in vec2 hz_uv;\n\
              layout(location = 3) in vec4 hz_color;\n\
              layout(location = 4) in uvec4 hz_light;",
    setup: "Position = hz_pos; Color = hz_color; UV0 = hz_uv; UV2 = ivec2(hz_light.y, hz_light.x);",
    finish: "",
    vertex_id: "int(gl_VertexIndex)",
};

pub(crate) const SEE_THROUGH_GLUE: Glue = Glue {
    inputs: &[("Position", "vec3"), ("Color", "vec4"), ("UV0", "vec2")],
    prelude: "layout(location = 0) in vec3 hz_pos;\n\
              layout(location = 1) in vec2 hz_uv;\n\
              layout(location = 3) in vec4 hz_color;",
    setup: "Position = hz_pos; Color = hz_color; UV0 = hz_uv;",
    finish: "",
    vertex_id: "int(gl_VertexIndex)",
};

#[derive(Default)]
pub struct TextShaders {
    pub gui: Option<Arc<CoreProgram>>,
    pub world: Option<Arc<CoreProgram>>,
    pub see_through: Option<Arc<CoreProgram>>,
}

#[derive(Clone, Copy)]
pub(super) struct PackBlocks {
    pub(super) globals: [u8; 48],
    pub(super) fog: [f32; 6],
}

impl super::Renderer {
    pub fn set_world_text_shaders(&mut self, shaders: &TextShaders) {
        self.core_text = CoreText::new(
            &self.device,
            self.output.frame.format,
            shaders.world.as_deref(),
            shaders.see_through.as_deref(),
            &self.entity_atlas,
            (&self.lightmap_view, &self.lightmap_sampler),
        );
    }

    pub(super) fn pack_blocks(
        &self,
        frame: &super::Frame,
        fog: [f32; 4],
        ends: [f32; 4],
    ) -> PackBlocks {
        let globals = crate::post_chain::Globals {
            glint_alpha: frame.glint_alpha,
            ..frame.post_effects.globals
        };
        let size = [self.config.width, self.config.height];
        PackBlocks {
            globals: globals.bytes(size, frame.post_effects.menu_blur_radius as i32),
            fog: [fog[0], fog[1], fog[2], fog[3], ends[0], ends[1]],
        }
    }

    pub(super) fn write_core_text(
        &self,
        pack: &PackBlocks,
        view: &super::View,
        pose: glam::Mat4,
        jitter: glam::Mat4,
        fog_color: [f32; 4],
    ) {
        let Some(core) = &self.core_text else {
            return;
        };
        let data =
            super::core_lines::uniforms(pack.globals, view, pose, jitter, 1.0, fog_color, pack.fog);
        self.queue.write_buffer(&core.uniform, 0, &data);
    }

    pub(super) fn bind_world_text(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        allowed: bool,
        see_through: bool,
    ) {
        let pack = self
            .core_text
            .as_ref()
            .filter(|_| allowed)
            .and_then(|core| {
                let pipeline = if see_through {
                    core.see_through.as_ref()
                } else {
                    core.polygon_offset.as_ref()
                };
                Some((pipeline?, &core.group))
            });
        match pack {
            Some((pipeline, group)) => {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, group, &[]);
            }
            None => pass.set_bind_group(0, &self.entity_group, &[]),
        }
    }
}

const BLOCK_OFFSETS: [u64; 4] = [0, 256, 512, 768];
const BLOCK_SIZES: [u64; 4] = [48, 64, 160, 48];

pub(super) struct CoreText {
    pub(super) polygon_offset: Option<wgpu::RenderPipeline>,
    pub(super) see_through: Option<wgpu::RenderPipeline>,
    pub(super) group: wgpu::BindGroup,
    uniform: wgpu::Buffer,
}

impl CoreText {
    pub(super) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        world: Option<&CoreProgram>,
        see_through: Option<&CoreProgram>,
        atlas: &wgpu::TextureView,
        lightmap: (&wgpu::TextureView, &wgpu::Sampler),
    ) -> Option<Self> {
        if world.is_none() && see_through.is_none() {
            return None;
        }
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
        let buffer = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(BLOCK_SIZES[binding as usize]),
            },
            count: None,
        };
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Resource pack world text"),
            entries: &[
                buffer(0),
                buffer(1),
                buffer(2),
                buffer(3),
                texture(4),
                sampler(5),
                texture(8),
                sampler(9),
            ],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Resource pack world text uniforms"),
            size: 1024,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let glyphs = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Resource pack world text glyphs"),
            ..Default::default()
        });
        let mut resources: Vec<wgpu::BindGroupEntry> = (0..4)
            .map(|i| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &uniform,
                    offset: BLOCK_OFFSETS[i],
                    size: wgpu::BufferSize::new(BLOCK_SIZES[i]),
                }),
            })
            .collect();
        resources.extend([
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(atlas),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::Sampler(&glyphs),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: wgpu::BindingResource::TextureView(lightmap.0),
            },
            wgpu::BindGroupEntry {
                binding: 9,
                resource: wgpu::BindingResource::Sampler(lightmap.1),
            },
        ]);
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Resource pack world text"),
            layout: &layout,
            entries: &resources,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Resource pack world text"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let attributes = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Uint32,3=>Unorm8x4,4=>Uint8x4,5=>Snorm8x4,6=>Float32x2];
        let buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<crate::mesher::Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attributes,
        })];
        let pipeline = |label, program: &CoreProgram, depth: wgpu::DepthStencilState| {
            let module = |module: &naga::Module| {
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(label),
                    source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(module.clone())),
                })
            };
            let (vertex, fragment) = (module(&program.vertex), module(&program.fragment));
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &vertex,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    buffers: &buffers,
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: Some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: Some(depth),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &fragment,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let depth = |write, compare, bias| wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(write),
            depth_compare: Some(compare),
            stencil: Default::default(),
            bias,
        };
        let offset = wgpu::DepthBiasState {
            constant: 10,
            slope_scale: 1.0,
            clamp: 0.0,
        };
        let polygon_offset = world.map(|program| {
            pipeline(
                "Resource pack polygon offset text",
                program,
                depth(true, wgpu::CompareFunction::GreaterEqual, offset),
            )
        });
        let see_through = see_through.map(|program| {
            pipeline(
                "Resource pack see-through text",
                program,
                depth(false, wgpu::CompareFunction::Always, Default::default()),
            )
        });
        let internal = pollster::block_on(internal.pop());
        if let Some(e) = internal.or(pollster::block_on(validation.pop())) {
            log::warn!("the resource pack's world text shader is not used: {e}");
            return None;
        }
        Some(Self {
            polygon_offset,
            see_through,
            group,
            uniform,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = crate::gfx::test_gpu::instance();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        pollster::block_on(adapter.request_device(&Default::default())).ok()
    }

    #[test]
    fn a_pack_text_shader_reads_the_light_as_vanilla_does() {
        let Some((device, queue)) = device() else {
            eprintln!("no GPU: skipping");
            return;
        };
        let vsh = "#version 330\n#extension GL_ARB_separate_shader_objects : require\n\
            layout(location = 0) in vec3 Position;\nlayout(location = 1) in vec4 Color;\n\
            layout(location = 2) in vec2 UV0;\nlayout(location = 3) in ivec2 UV2;\n\
            layout(location = 0) flat out ivec2 light;\n\
            void main() { gl_Position = vec4(Position, 1.0); light = UV2; }\n";
        let fsh = "#version 330\n#extension GL_ARB_separate_shader_objects : require\n\
            layout(location = 0) flat in ivec2 light;\nlayout(location = 0) out vec4 fragColor;\n\
            void main() { fragColor = vec4(vec2(light) / 255.0, 0.0, 1.0); }\n";
        let program =
            crate::core_shader::translate_core(vsh, fsh, &[], &|_| None, &WORLD_GLUE).unwrap();
        const SIZE: u32 = 64;
        let texture = |format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: SIZE,
                    height: SIZE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let binding = wgpu::TextureUsages::TEXTURE_BINDING;
        let atlas =
            texture(wgpu::TextureFormat::Rgba8Unorm, binding).create_view(&Default::default());
        let sampler = device.create_sampler(&Default::default());
        let text = CoreText::new(
            &device,
            wgpu::TextureFormat::Rgba8Unorm,
            Some(&program),
            None,
            &atlas,
            (&atlas, &sampler),
        )
        .expect("the pack's pipeline");
        let target = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let color = texture(
            wgpu::TextureFormat::Rgba8Unorm,
            target | wgpu::TextureUsages::COPY_SRC,
        );
        let depth = texture(wgpu::TextureFormat::Depth32Float, target);
        let light = 0x30 | (0xC0 << 8);
        let corner = |x: f32, y: f32| crate::mesher::Vertex {
            pos: [x, y, 0.5],
            uv: [0.0; 2],
            layer: 0,
            color: u32::MAX,
            light,
            normal: [0; 4],
            glint_uv: [0.0; 2],
        };
        let quad = [
            corner(-1.0, -1.0),
            corner(1.0, -1.0),
            corner(1.0, 1.0),
            corner(-1.0, -1.0),
            corner(1.0, 1.0),
            corner(-1.0, 1.0),
        ];
        let vertices = wgpu::util::DeviceExt::create_buffer_init(
            &device,
            &wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&quad),
                usage: wgpu::BufferUsages::VERTEX,
            },
        );
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(SIZE * SIZE * 4),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let color_view = color.create_view(&Default::default());
            let depth_view = depth.create_view(&Default::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(text.polygon_offset.as_ref().unwrap());
            pass.set_bind_group(0, &text.group, &[]);
            pass.set_vertex_buffer(0, vertices.slice(..));
            pass.draw(0..6, 0..1);
        }
        encoder.copy_texture_to_buffer(
            color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(SIZE * 4),
                    rows_per_image: Some(SIZE),
                },
            },
            color.size(),
        );
        queue.submit([encoder.finish()]);
        read.slice(..)
            .map_async(wgpu::MapMode::Read, |r| r.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let pixels = read.slice(..).get_mapped_range().unwrap().to_vec();
        let middle = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
        assert_eq!(
            &pixels[middle..middle + 4],
            &[0xC0, 0x30, 0, 0xFF],
            "UV2 is (block, sky)"
        );
    }

    #[test]
    #[ignore = "requires prepared resources in HORIZON_TEST_RESOURCES or .local/horizon-client/26.3, and a GPU"]
    fn vanillas_text_shader_makes_both_world_text_pipelines() {
        let root = std::env::var_os("HORIZON_TEST_RESOURCES")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../.local/horizon-client")
                    .join(horizon_core::VANILLA_VERSION)
            });
        let read = |path: &str| std::fs::read_to_string(root.join(path)).ok();
        let vertex = read("assets/minecraft/shaders/core/text.vsh").expect("vanilla's text.vsh");
        let fragment = read("assets/minecraft/shaders/core/text.fsh").expect("vanilla's text.fsh");
        let include = |id: &str| {
            let (namespace, path) = id.split_once(':')?;
            read(&format!("assets/{namespace}/{path}"))
        };
        let translate = |defines: &[&str], glue: &Glue| {
            crate::core_shader::translate_core(&vertex, &fragment, defines, &include, glue)
                .expect("vanilla's text shader translates")
        };
        let world = translate(&[], &WORLD_GLUE);
        let see_through = translate(&["IS_SEE_THROUGH"], &SEE_THROUGH_GLUE);
        let (device, _) = device().expect("a GPU");
        let texture = || {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size: wgpu::Extent3d {
                        width: 4,
                        height: 4,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let (atlas, lightmap) = (texture(), texture());
        let sampler = device.create_sampler(&Default::default());
        let text = CoreText::new(
            &device,
            wgpu::TextureFormat::Rgba8Unorm,
            Some(&world),
            Some(&see_through),
            &atlas,
            (&lightmap, &sampler),
        )
        .expect("the pack's world text pipelines");
        assert!(text.polygon_offset.is_some() && text.see_through.is_some());
    }
}
