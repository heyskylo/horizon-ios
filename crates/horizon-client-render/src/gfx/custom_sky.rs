//! OptiFine custom sky layers on the GPU: one texture per layer, kept with the renderer (its pack),
//! a static skybox mesh of the 3x2 grid's six faces, and one pipeline per blend mode. A frame
//! uploads one small uniform per drawn layer ([`crate::custom_sky::Draw`]) and draws 36 vertices
//! each, after the vanilla sky's disc and sunrise and before the sun, moon and stars.
use glam::Mat4;
use horizon_optifine::sky::{Blend, Face};
use wgpu::util::DeviceExt;

use crate::custom_sky::{CustomSky as Pack, Draw};

/// The blend modes in the shader's `mode` order (custom_sky.wgsl).
const MODES: [Blend; 9] = [
    Blend::Add,
    Blend::Subtract,
    Blend::Multiply,
    Blend::Dodge,
    Blend::Burn,
    Blend::Screen,
    Blend::Replace,
    Blend::Overlay,
    Blend::Alpha,
];

/// Layers drawn in a frame at most; a pack's later ones wait for a frame with fewer.
const MAX_DRAWS: usize = 32;
/// Dynamic uniform offsets are 256-byte aligned on every backend.
const STRIDE: u64 = 256;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
    clip_from_sky: [[f32; 4]; 4],
    brightness: f32,
    mode: u32,
    pad: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    uv: [f32; 2],
    cell: [f32; 4],
}

pub(super) struct CustomSky {
    pipelines: Vec<wgpu::RenderPipeline>,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    vertices: wgpu::Buffer,
    /// One bind group per layer texture of the pack.
    groups: Vec<wgpu::BindGroup>,
    /// This frame's layers: bind group and pipeline indices.
    frame: Vec<(u16, u8)>,
}

/// The fragment's output for each mode is in custom_sky.wgsl; these factors finish it.
fn blend(mode: Blend) -> wgpu::BlendComponent {
    use wgpu::BlendFactor as F;
    let (src_factor, dst_factor, operation) = match mode {
        Blend::Add => (F::One, F::One, wgpu::BlendOperation::Add),
        Blend::Subtract => (F::One, F::One, wgpu::BlendOperation::ReverseSubtract),
        Blend::Multiply => (F::Zero, F::Src, wgpu::BlendOperation::Add),
        // dst + dst * src: lightens.
        Blend::Dodge => (F::Dst, F::One, wgpu::BlendOperation::Add),
        // dst * (1 - src): darkens.
        Blend::Burn => (F::Zero, F::OneMinusSrc, wgpu::BlendOperation::Add),
        // src + dst * (1 - src) = 1 - (1 - src)(1 - dst).
        Blend::Screen => (F::One, F::OneMinusSrc, wgpu::BlendOperation::Add),
        Blend::Replace => (F::One, F::Zero, wgpu::BlendOperation::Add),
        // dst * src + src * dst.
        Blend::Overlay => (F::Dst, F::Src, wgpu::BlendOperation::Add),
        Blend::Alpha => (F::SrcAlpha, F::OneMinusSrcAlpha, wgpu::BlendOperation::Add),
    };
    wgpu::BlendComponent {
        src_factor,
        dst_factor,
        operation,
    }
}

/// The skybox's 36 vertices: two triangles per face, each corner with its cell's UV.
fn mesh() -> Vec<Vertex> {
    let mut out = Vec::with_capacity(36);
    for face in Face::ALL {
        let cell = face.cell();
        let [u0, v0, u1, v1] = cell;
        let uv = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
        let corners = face.corners();
        for i in [0, 1, 2, 0, 2, 3] {
            out.push(Vertex {
                position: corners[i],
                uv: uv[i],
                cell,
            });
        }
    }
    out
}

impl CustomSky {
    pub(super) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Custom sky bindings"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<Uniform>() as u64
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Custom sky shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("custom_sky.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Custom sky layout"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4];
        let pipelines = MODES
            .iter()
            .map(|&mode| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("Custom sky layer"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs_custom_sky"),
                        compilation_options: Default::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<Vertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &attributes,
                        })],
                    },
                    primitive: wgpu::PrimitiveState {
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: wgpu::TextureFormat::Depth32Float,
                        depth_write_enabled: Some(false),
                        depth_compare: Some(wgpu::CompareFunction::Always),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs_custom_sky"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            // The frame's alpha is HDR output's mark (hdr.wgsl): the sky
                            // under the layers keeps its own.
                            blend: Some(wgpu::BlendState {
                                color: blend(mode),
                                alpha: wgpu::BlendComponent {
                                    src_factor: wgpu::BlendFactor::Zero,
                                    dst_factor: wgpu::BlendFactor::One,
                                    operation: wgpu::BlendOperation::Add,
                                },
                            }),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            })
            .collect();
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Custom sky"),
            min_filter: wgpu::FilterMode::Linear,
            mag_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Custom sky layers"),
            size: STRIDE * MAX_DRAWS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Custom sky box"),
            contents: bytemuck::cast_slice(&mesh()),
            usage: wgpu::BufferUsages::VERTEX,
        });
        Self {
            pipelines,
            layout,
            sampler,
            uniform,
            vertices,
            groups: Vec::new(),
            frame: Vec::new(),
        }
    }

    /// The pack's layer textures, replacing any before.
    pub(super) fn set_pack(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, pack: &Pack) {
        let limit = device.limits().max_texture_dimension_2d;
        self.groups = pack
            .images
            .iter()
            .map(|image| {
                // Too large for this GPU: a 1x1 transparent texel, so the layer draws nothing.
                let (width, height, rgba) = if image.width <= limit && image.height <= limit {
                    (image.width, image.height, &image.rgba[..])
                } else {
                    log::warn!(
                        "custom sky texture of {}x{} too large",
                        image.width,
                        image.height
                    );
                    (1, 1, &[0u8; 4][..])
                };
                let texture = device.create_texture_with_data(
                    queue,
                    &wgpu::TextureDescriptor {
                        label: Some("Custom sky layer"),
                        size: wgpu::Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    },
                    wgpu::util::TextureDataOrder::LayerMajor,
                    rgba,
                );
                let view = texture.create_view(&Default::default());
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Custom sky layer"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &self.uniform,
                                offset: 0,
                                size: wgpu::BufferSize::new(std::mem::size_of::<Uniform>() as u64),
                            }),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                    ],
                })
            })
            .collect();
    }

    /// Upload this frame's layers; `clip` is clip space from camera-relative world.
    pub(super) fn prepare(
        &mut self,
        uploads: &mut super::upload::Uploads,
        encoder: &mut wgpu::CommandEncoder,
        clip: Mat4,
        draws: &[Draw],
    ) {
        self.frame.clear();
        for draw in draws.iter().take(MAX_DRAWS) {
            if usize::from(draw.image) >= self.groups.len() {
                continue;
            }
            let mode = MODES.iter().position(|m| *m == draw.blend).unwrap_or(0);
            let uniform = Uniform {
                clip_from_sky: (clip * draw.rotation).to_cols_array_2d(),
                brightness: draw.brightness.clamp(0.0, 1.0),
                mode: mode as u32,
                pad: [0; 2],
            };
            let offset = STRIDE * self.frame.len() as u64;
            uploads.buffer(encoder, &self.uniform, offset, bytemuck::bytes_of(&uniform));
            self.frame.push((draw.image, mode as u8));
        }
    }

    pub(super) fn is_drawing(&self) -> bool {
        !self.frame.is_empty()
    }

    /// The layers [`CustomSky::prepare`] uploaded. Leaves the pass's pipeline and bind group 0
    /// changed.
    pub(super) fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        for (i, &(image, mode)) in self.frame.iter().enumerate() {
            pass.set_pipeline(&self.pipelines[usize::from(mode)]);
            pass.set_bind_group(
                0,
                &self.groups[usize::from(image)],
                &[(STRIDE * i as u64) as u32],
            );
            pass.draw(0..36, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a blend leaves a sky texel of `dst` with when the shader outputs `src`
    /// (custom_sky.wgsl) for a layer texel `t` at brightness `b`, per channel.
    fn mix(mode: Blend, dst: f32, t: f32, a: f32, b: f32) -> f32 {
        let k = a * b;
        let src = match mode {
            Blend::Multiply => 1.0 + (t - 1.0) * k,
            Blend::Overlay => 0.5 + (t - 0.5) * k,
            Blend::Replace | Blend::Alpha => t,
            _ => t * k,
        };
        let c = blend(mode);
        let f = |factor: wgpu::BlendFactor| match factor {
            wgpu::BlendFactor::One => 1.0,
            wgpu::BlendFactor::Zero => 0.0,
            wgpu::BlendFactor::Src => src,
            wgpu::BlendFactor::OneMinusSrc => 1.0 - src,
            wgpu::BlendFactor::Dst => dst,
            wgpu::BlendFactor::SrcAlpha => k,
            wgpu::BlendFactor::OneMinusSrcAlpha => 1.0 - k,
            other => panic!("{other:?}"),
        };
        let (s, d) = (src * f(c.src_factor), dst * f(c.dst_factor));
        match c.operation {
            wgpu::BlendOperation::Add => s + d,
            wgpu::BlendOperation::ReverseSubtract => d - s,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn every_mode_gives_its_documented_result() {
        let close = |a: f32, b: f32| assert!((a - b).abs() < 1e-6, "{a} != {b}");
        // Full brightness and alpha, a sky texel of 0.4 under a layer texel of 0.8.
        let (d, t) = (0.4, 0.8);
        close(mix(Blend::Add, d, t, 1.0, 1.0), 1.2);
        close(mix(Blend::Subtract, d, t, 1.0, 1.0), -0.4);
        close(mix(Blend::Multiply, d, t, 1.0, 1.0), 0.32);
        close(mix(Blend::Screen, d, t, 1.0, 1.0), 1.0 - 0.6 * 0.2);
        close(mix(Blend::Replace, d, t, 1.0, 1.0), 0.8);
        close(mix(Blend::Overlay, d, t, 1.0, 1.0), 0.64);
        close(mix(Blend::Overlay, d, 0.2, 1.0, 1.0), 0.16);
        close(mix(Blend::Alpha, d, t, 0.25, 1.0), 0.4 * 0.75 + 0.8 * 0.25);
        assert!(mix(Blend::Dodge, d, t, 1.0, 1.0) > d);
        assert!(mix(Blend::Burn, d, t, 1.0, 1.0) < d);
        // Faded out, every mode but replace (drawn only when bright) leaves the sky alone.
        for mode in MODES {
            if mode != Blend::Replace {
                close(mix(mode, d, t, 1.0, 0.0), d);
            }
        }
        // Half faded: add adds half.
        close(mix(Blend::Add, d, t, 1.0, 0.5), 0.8);
    }

    #[test]
    fn the_shader_validates() {
        let module = naga::front::wgsl::parse_str(include_str!("custom_sky.wgsl")).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }

    #[test]
    fn the_box_covers_every_face_with_its_cell() {
        let mesh = mesh();
        assert_eq!(mesh.len(), 36);
        for v in &mesh {
            let [u0, v0, u1, v1] = v.cell;
            assert!((u0..=u1).contains(&v.uv[0]) && (v0..=v1).contains(&v.uv[1]));
            assert_eq!(v.position.iter().filter(|c| c.abs() == 1.0).count(), 3);
        }
    }
}
