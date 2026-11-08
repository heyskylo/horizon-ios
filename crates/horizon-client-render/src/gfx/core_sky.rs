//! A resource pack's `core/sky` and `core/stars` shaders: the overworld's sky disc and stars drawn
//! with the pack's GLSL (`SkyRenderer.renderSkyDisc`, `renderStars`), as a pack that adds a haze
//! to the horizon or tints the night sky expects. Without one the sky draws natively (`fs_sky` in
//! terrain.wgsl, stars.rs).

use glam::Mat4;
use horizon_shaders::{CoreProgram, Glue};
use wgpu::util::DeviceExt;

/// `DefaultVertexFormat.POSITION`: the disc's and the stars' vertices.
pub(crate) const GLUE: Glue = Glue {
    inputs: &[("Position", "vec3")],
    prelude: "layout(location = 0) in vec3 hz_position;",
    setup: "Position = hz_position;",
    // 26.3's ProjMat already has wgpu's reversed 0 to 1 depth (Projection: zZeroToOne).
    finish: "",
    vertex_id: "int(gl_VertexIndex)",
};

/// Each uniform block's offset in the one buffer (`core_shader::BLOCKS` order).
const BLOCK_OFFSETS: [u64; 4] = [0, 256, 512, 768];
const BLOCK_SIZES: [u64; 4] = [48, 64, 160, 48];

/// `SkyRenderer.buildSkyDisc(16)`: a fan around the point 16 above the camera, out to 512 every
/// 45 degrees, as a triangle list.
pub(crate) fn disc_triangles() -> Vec<[f32; 3]> {
    let rim: Vec<[f32; 3]> = (-180..=180)
        .step_by(45)
        .map(|i: i32| {
            let a = (i as f32).to_radians();
            [512.0 * a.cos(), 16.0, 512.0 * a.sin()]
        })
        .collect();
    rim.windows(2)
        .flat_map(|edge| [[0.0, 16.0, 0.0], edge[0], edge[1]])
        .collect()
}

/// One of the sky's pack pipelines with its vertices and uniforms.
struct Part {
    pipeline: wgpu::RenderPipeline,
    group: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    vertices: wgpu::Buffer,
    count: u32,
}

impl Part {
    fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        program: &CoreProgram,
        name: &str,
        blend: Option<wgpu::BlendState>,
        triangles: &[[f32; 3]],
    ) -> Option<Self> {
        // A pack's shader is untrusted: the backend failing to compile it is an internal error.
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
        let module = |stage, module: &naga::Module| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&format!("Resource pack {name} ({stage})")),
                source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(module.clone())),
            })
        };
        let vertex = module("vertex", &program.vertex);
        let fragment = module("fragment", &program.fragment);
        // Vanilla's SKY and STARS pipelines have no samplers: a pack that reads one is not used.
        let entries: Vec<wgpu::BindGroupLayoutEntry> = (0..4)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(BLOCK_SIZES[binding as usize]),
                },
                count: None,
            })
            .collect();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(name),
            entries: &entries,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(name),
            size: 1024,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let resources: Vec<wgpu::BindGroupEntry> = (0..4)
            .map(|i| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &uniform,
                    offset: BLOCK_OFFSETS[i],
                    size: wgpu::BufferSize::new(BLOCK_SIZES[i]),
                }),
            })
            .collect();
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(name),
            layout: &layout,
            entries: &resources,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(name),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(name),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &vertex,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 12,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3],
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            // The sky is drawn first, behind everything: no depth test, no depth write.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &fragment,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let internal = pollster::block_on(internal.pop());
        if let Some(e) = internal.or(pollster::block_on(validation.pop())) {
            log::warn!("the resource pack's {name} shader is not used: {e}");
            return None;
        }
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(name),
            contents: bytemuck::cast_slice(triangles),
            usage: wgpu::BufferUsages::VERTEX,
        });
        Some(Self {
            pipeline,
            group,
            uniform,
            vertices,
            count: triangles.len() as u32,
        })
    }

    fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.draw(0..self.count, 0..1);
    }
}

/// The packs' sky disc and stars pipelines, each present when a pack replaces that shader.
pub(super) struct CoreSky {
    disc: Option<Part>,
    stars: Option<Part>,
}

impl CoreSky {
    /// `None` when neither shader is replaced or neither builds.
    pub(super) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        disc: Option<&CoreProgram>,
        stars: Option<&CoreProgram>,
    ) -> Option<Self> {
        let disc = disc.and_then(|program| {
            // RenderPipelines.SKY: ColorTargetState.DEFAULT, no blending.
            Part::new(device, format, program, "sky", None, &disc_triangles())
        });
        let stars = stars.and_then(|program| {
            // RenderPipelines.STARS: BlendFunction.OVERLAY (src alpha and one; alpha one, zero).
            let overlay = wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::SrcAlpha,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::Zero,
                    operation: wgpu::BlendOperation::Add,
                },
            };
            let triangles = super::stars::star_triangles();
            Part::new(device, format, program, "stars", Some(overlay), &triangles)
        });
        (disc.is_some() || stars.is_some()).then_some(Self { disc, stars })
    }

    pub(super) fn draws_disc(&self) -> bool {
        self.disc.is_some()
    }

    pub(super) fn draws_stars(&self) -> bool {
        self.stars.is_some()
    }

    /// Uploads the disc's and the stars' blocks: `sky` and `stars` from [`uniforms`].
    pub(super) fn prepare(
        &self,
        uploads: &mut super::upload::Uploads,
        encoder: &mut wgpu::CommandEncoder,
        sky: &[u8],
        stars: Option<&[u8]>,
    ) {
        if let Some(disc) = &self.disc {
            uploads.buffer(encoder, &disc.uniform, 0, sky);
        }
        if let (Some(part), Some(data)) = (&self.stars, stars) {
            uploads.buffer(encoder, &part.uniform, 0, data);
        }
    }

    /// The disc, in place of the native sky colour. The caller sets its own group 0 again after.
    pub(super) fn draw_disc(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(disc) = &self.disc {
            disc.draw(pass);
        }
    }

    /// The stars, in place of the native ones. The caller sets its own group 0 again after.
    pub(super) fn draw_stars(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(stars) = &self.stars {
            stars.draw(pass);
        }
    }
}

/// The std140 blocks for one sky draw: `Globals`, `Projection` (`projection`, far enough for the
/// disc's 512-block rim), `DynamicTransforms` (`model_view`: the camera's rotation, times the
/// stars' rotation for them; `color`: the sky colour, or the star brightness in every channel)
/// and `Fog` (`fog_color`, `fog`: environmental start and end, render distance start and end,
/// sky end, clouds end).
pub(super) fn uniforms(
    globals: [u8; 48],
    projection: Mat4,
    model_view: Mat4,
    color: [f32; 4],
    fog_color: [f32; 4],
    fog: [f32; 6],
) -> Vec<u8> {
    let mut data = vec![0f32; 256];
    data[64..80].copy_from_slice(&projection.to_cols_array());
    let transforms = 128;
    data[transforms..transforms + 16].copy_from_slice(&model_view.to_cols_array());
    data[transforms + 16..transforms + 32].copy_from_slice(&Mat4::IDENTITY.to_cols_array());
    data[transforms + 32..transforms + 36].copy_from_slice(&color);
    data[192..196].copy_from_slice(&fog_color);
    data[196..202].copy_from_slice(&fog);
    let mut bytes: Vec<u8> = bytemuck::cast_slice(&data).to_vec();
    bytes[..48].copy_from_slice(&globals);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `buildSkyDisc(16)`: eight triangles from the point above the camera to the rim, which
    /// starts and ends at x -512 (cos 180°) and passes through z -512, x 512 and z 512.
    #[test]
    fn the_disc_is_vanillas_fan_as_triangles() {
        let triangles = disc_triangles();
        assert_eq!(triangles.len(), 24);
        assert!(triangles.chunks(3).all(|t| t[0] == [0.0, 16.0, 0.0]));
        let rim: Vec<[f32; 3]> = triangles.chunks(3).map(|t| t[1]).collect();
        let near = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3);
        assert!(near(rim[0], [-512.0, 16.0, 0.0]));
        assert!(near(rim[2], [0.0, 16.0, -512.0]));
        assert!(near(rim[4], [512.0, 16.0, 0.0]));
        assert!(near(rim[6], [0.0, 16.0, 512.0]));
        assert!(near(triangles[23], [-512.0, 16.0, 0.0]));
    }

    /// The prepared resources (`HORIZON_RESOURCES`, or `.local/horizon-client/<version>`), where
    /// vanilla's sky shader and its includes are.
    fn resources() -> std::path::PathBuf {
        std::env::var_os("HORIZON_RESOURCES")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../.local/horizon-client")
                    .join(horizon_core::VANILLA_VERSION)
            })
    }

    /// Vanilla's `sky.vsh` with a pack's fragment shader that paints `ColorModulator` (the sky
    /// colour) where the disc is: looking at the horizon, the top half of the target is the
    /// disc, the bottom half stays clear.
    #[test]
    fn a_pack_sky_shader_paints_the_disc_above_the_horizon() {
        let root = resources();
        let Ok(vertex) =
            std::fs::read_to_string(root.join("assets/minecraft/shaders/core/sky.vsh"))
        else {
            eprintln!("skipping: no prepared resources at {}", root.display());
            return;
        };
        let include = |id: &str| {
            let (namespace, path) = id.split_once(':')?;
            std::fs::read_to_string(root.join(format!("assets/{namespace}/{path}"))).ok()
        };
        let fragment = "#version 330\n#include <minecraft:dynamictransforms.glsl>\n\
            layout(location = 0) out vec4 fragColor;\n\
            void main() {\n    fragColor = ColorModulator;\n}\n";
        let program = horizon_shaders::translate_core(&vertex, fragment, &[], &include, &GLUE)
            .expect("the sky shaders translate");
        let instance = crate::gfx::test_gpu::instance();
        let Some((device, queue)) =
            pollster::block_on(instance.request_adapter(&Default::default()))
                .ok()
                .and_then(|adapter| {
                    pollster::block_on(adapter.request_device(&Default::default())).ok()
                })
        else {
            eprintln!("no GPU: skipping");
            return;
        };
        const SIZE: u32 = 64;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let sky = CoreSky::new(&device, format, Some(&program), None).expect("the sky pipeline");
        let projection = crate::gfx::world_projection(90.0, 1.0, 2048.0);
        let data = uniforms(
            [0; 48],
            projection,
            Mat4::IDENTITY,
            [0.25, 0.5, 1.0, 1.0],
            [0.0; 4],
            [1e6; 6],
        );
        queue.write_buffer(&sky.disc.as_ref().unwrap().uniform, 0, &data);
        let target = |format, usage| {
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
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | usage,
                view_formats: &[],
            })
        };
        let color = target(format, wgpu::TextureUsages::COPY_SRC);
        let depth = target(
            wgpu::TextureFormat::Depth32Float,
            wgpu::TextureUsages::empty(),
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
            sky.draw_disc(&mut pass);
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
        let at = |x: usize, y: usize| pixels[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4].to_vec();
        // Row 0 is the top: the disc, in the sky colour; the bottom row is below the horizon.
        let top = at(32, 4);
        assert!(
            top.iter()
                .zip([64u8, 128, 255, 255])
                .all(|(&p, w)| p.abs_diff(w) <= 1),
            "{top:?}"
        );
        assert_eq!(at(32, 60), vec![0, 0, 0, 0]);
    }
}
