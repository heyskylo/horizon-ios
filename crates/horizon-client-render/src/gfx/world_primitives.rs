//! Native mods' world primitives (`native_world_primitives`, docs/native-mods.md): lines, boxes,
//! beams and text a mod hands the game each frame, drawn here with Horizon's own pipelines at
//! the end of the world pass, after the entities and before the first-person hand. They draw
//! into the world's targets, so they take the world's reversed depth, its draw resolution and
//! TAA's jitter; a mod never touches wgpu for them.

use super::Vertex;
use glam::Mat4;

/// A frame's primitives, camera-relative, as the game tessellated them: quads of 4 vertices
/// from the shared atlas (plain colours on its white texel, text on its glyphs, beams on the
/// beacon beam sprite) and lines as `gizmos::camera_relative` gives them. `*_through` ones are
/// seen through walls: no depth test.
#[derive(Clone, Copy, Debug, Default)]
pub struct Batch<'a> {
    pub quads: &'a [Vertex],
    pub quads_through: &'a [Vertex],
    pub lines: &'a [[f32; 8]],
    pub lines_through: &'a [[f32; 8]],
}

impl Batch<'_> {
    pub fn is_empty(&self) -> bool {
        self.quads.is_empty()
            && self.quads_through.is_empty()
            && self.lines.is_empty()
            && self.lines_through.is_empty()
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    viewport: [f32; 4],
}

/// The pipelines and buffers that draw a [`Batch`].
pub struct WorldPrimitives {
    /// Depth tested, then seen through walls.
    quad_pipelines: [wgpu::RenderPipeline; 2],
    line_pipelines: [wgpu::RenderPipeline; 2],
    layout: wgpu::BindGroupLayout,
    globals: wgpu::Buffer,
    sampler: wgpu::Sampler,
    quads: wgpu::Buffer,
    lines: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_quads: u32,
    group: Option<wgpu::BindGroup>,
    /// Quads depth tested, quads through walls, lines depth tested, lines through walls.
    counts: [u32; 4],
}

/// Writes bytes into a buffer as part of the frame (the renderer's staging uploads, or the
/// queue in a test).
pub type Upload<'u> = &'u mut dyn FnMut(&wgpu::Buffer, &[u8]);

impl WorldPrimitives {
    /// Pipelines drawing into `color_format` over a reversed Depth32Float depth.
    pub fn new(device: &wgpu::Device, color_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("World primitives"),
            source: wgpu::ShaderSource::Wgsl(include_str!("world_primitives.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("World primitives"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("World primitives"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let quad_attributes =
            wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Uint32,3=>Unorm8x4,4=>Uint32];
        let line_attributes =
            wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Unorm8x4,3=>Float32];
        let quad_buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &quad_attributes,
        })];
        let line_buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: 32,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &line_attributes,
        })];
        let pipeline = |label: &str,
                        entry: [&str; 2],
                        buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
                        topology,
                        through: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(entry[0]),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState {
                    topology,
                    // Seen from inside or behind as well: a box around the camera, a beam.
                    cull_mode: None,
                    ..Default::default()
                },
                // Blended over what is drawn, writing no depth: a translucent box doesn't hide
                // the primitives behind it. Reversed depth: nearer is greater.
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(if through {
                        wgpu::CompareFunction::Always
                    } else {
                        wgpu::CompareFunction::GreaterEqual
                    }),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry[1]),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let quads = ["vs_quad", "fs_quad"];
        let lines = ["vs_line", "fs_line"];
        let list = wgpu::PrimitiveTopology::TriangleList;
        let strip = wgpu::PrimitiveTopology::TriangleStrip;
        let quad_pipelines = [
            pipeline("World primitives", quads, &quad_buffers, list, false),
            pipeline(
                "World primitives through walls",
                quads,
                &quad_buffers,
                list,
                true,
            ),
        ];
        let line_pipelines = [
            pipeline("World primitive lines", lines, &line_buffers, strip, false),
            pipeline(
                "World primitive lines through walls",
                lines,
                &line_buffers,
                strip,
                true,
            ),
        ];
        let buffer = |label, size, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        Self {
            quad_pipelines,
            line_pipelines,
            globals: buffer(
                "World primitive globals",
                std::mem::size_of::<Globals>() as u64,
                wgpu::BufferUsages::UNIFORM,
            ),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("World primitives"),
                ..Default::default()
            }),
            layout,
            quads: buffer("World primitive quads", 0, wgpu::BufferUsages::VERTEX),
            lines: buffer("World primitive lines", 0, wgpu::BufferUsages::VERTEX),
            indices: buffer("World primitive indices", 0, wgpu::BufferUsages::INDEX),
            index_quads: 0,
            group: None,
            counts: [0; 4],
        }
    }

    /// Uploads this frame's `batch`, drawn with `view_proj` (camera-relative world to clip,
    /// jittered as the world is) into a frame `viewport` pixels big, sampling `atlas`. Whether
    /// there is anything to draw.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        upload: Upload<'_>,
        batch: &Batch<'_>,
        view_proj: Mat4,
        viewport: [u32; 2],
        atlas: &wgpu::TextureView,
    ) -> bool {
        let quads = [batch.quads, batch.quads_through].map(|q| (q.len() / 4) as u32);
        let lines = [batch.lines, batch.lines_through].map(|l| l.len() as u32);
        self.counts = [quads[0], quads[1], lines[0], lines[1]];
        if batch.is_empty() {
            return false;
        }
        let grow = |buffer: &mut wgpu::Buffer, bytes: u64| {
            if buffer.size() < bytes {
                *buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("World primitives"),
                    size: bytes.next_power_of_two(),
                    usage: buffer.usage(),
                    mapped_at_creation: false,
                });
            }
        };
        let total_quads = quads[0] + quads[1];
        if total_quads > 0 {
            let vertices: Vec<Vertex> = [batch.quads, batch.quads_through]
                .into_iter()
                .flat_map(|q| &q[..q.len() / 4 * 4])
                .copied()
                .collect();
            let bytes: &[u8] = bytemuck::cast_slice(&vertices);
            grow(&mut self.quads, bytes.len() as u64);
            upload(&self.quads, bytes);
            if total_quads > self.index_quads {
                self.index_quads = total_quads.next_power_of_two();
                let indices: Vec<u32> = (0..self.index_quads)
                    .flat_map(|q| [0, 1, 2, 0, 2, 3].map(|i| q * 4 + i))
                    .collect();
                let bytes: &[u8] = bytemuck::cast_slice(&indices);
                grow(&mut self.indices, bytes.len() as u64);
                upload(&self.indices, bytes);
            }
        }
        if lines[0] + lines[1] > 0 {
            let all: Vec<[f32; 8]> = batch
                .lines
                .iter()
                .chain(batch.lines_through)
                .copied()
                .collect();
            let bytes: &[u8] = bytemuck::cast_slice(&all);
            grow(&mut self.lines, bytes.len() as u64);
            upload(&self.lines, bytes);
        }
        let globals = Globals {
            view_proj: view_proj.to_cols_array_2d(),
            viewport: [viewport[0] as f32, viewport[1] as f32, 0.0, 0.0],
        };
        upload(&self.globals, bytemuck::bytes_of(&globals));
        self.group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("World primitives"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.globals.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(atlas),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        }));
        true
    }

    /// Draws what [`Self::prepare`] uploaded into a pass over the world's colour and depth:
    /// the depth-tested quads and lines, then those seen through walls over everything.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(group) = &self.group else {
            return;
        };
        let [quads, quads_through, lines, lines_through] = self.counts;
        pass.set_bind_group(0, group, &[]);
        for (through, (quad_range, line_range)) in [
            (0..quads, 0..lines),
            (quads..quads + quads_through, lines..lines + lines_through),
        ]
        .into_iter()
        .enumerate()
        {
            if !quad_range.is_empty() {
                pass.set_pipeline(&self.quad_pipelines[through]);
                pass.set_vertex_buffer(0, self.quads.slice(..));
                pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(quad_range.start * 6..quad_range.end * 6, 0, 0..1);
            }
            if !line_range.is_empty() {
                pass.set_pipeline(&self.line_pipelines[through]);
                pass.set_vertex_buffer(0, self.lines.slice(..));
                pass.draw(0..4, line_range);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    const SIZE: u32 = 64;
    /// RGBA, as `Vertex::color` holds it.
    const RED: u32 = 0xFF00_00FF;
    const GREEN: u32 = 0xFF00_FF00;
    const BLUE: u32 = 0xFFFF_0000;
    /// ARGB, as a line carries it.
    const MAGENTA_ARGB: u32 = 0xFFFF_00FF;

    /// A camera-relative quad of plain colour facing the camera, `z` blocks ahead of it, over
    /// the pixels `x0..x1`, `y0..y1` of the frame (y down).
    fn quad(view_proj: Mat4, [x0, y0, x1, y1]: [f32; 4], z: f32, rgba: u32) -> [Vertex; 4] {
        let depth = view_proj.project_point3(Vec3::new(0.0, 0.0, -z)).z;
        let inverse = view_proj.inverse();
        let at = |x: f32, y: f32| {
            let ndc = Vec3::new(
                x / SIZE as f32 * 2.0 - 1.0,
                1.0 - y / SIZE as f32 * 2.0,
                depth,
            );
            inverse.project_point3(ndc).to_array()
        };
        [at(x0, y0), at(x0, y1), at(x1, y1), at(x1, y0)].map(|pos| Vertex {
            pos,
            uv: [0.5, 0.5],
            layer: 0,
            color: rgba,
            light: 0,
            normal: [0; 4],
            glint_uv: [0.0, 0.0],
        })
    }

    /// A horizontal line, 4 pixels wide, over the pixels `x0..x1` of row `y`, `z` blocks ahead.
    fn line(view_proj: Mat4, [x0, x1]: [f32; 2], y: f32, z: f32, argb: u32) -> [f32; 8] {
        let q = quad(view_proj, [x0, y, x1, y], z, 0);
        let [a, b] = [q[0].pos, q[3].pos];
        [
            a[0],
            a[1],
            a[2],
            b[0],
            b[1],
            b[2],
            f32::from_bits(argb),
            4.0,
        ]
    }

    /// A wall 5 blocks ahead fills the depth (reversed: nearer is greater). Behind it, a
    /// depth-tested quad and line stay hidden while the same seen through walls show; a
    /// depth-tested quad in front of the wall shows.
    #[test]
    fn depth_tested_primitives_hide_behind_walls_and_see_through_ones_do_not() {
        let instance = crate::gfx::test_gpu::instance();
        let Some((device, queue)) =
            pollster::block_on(instance.request_adapter(&Default::default()))
                .ok()
                .and_then(|adapter| {
                    pollster::block_on(adapter.request_device(&Default::default())).ok()
                })
        else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let view_proj = super::super::world_projection(70.0, 1.0, 512.0);
        let wall = view_proj.project_point3(Vec3::new(0.0, 0.0, -5.0)).z;
        // Reversed depth: 10 blocks away is less than the wall, 2 blocks away more.
        assert!(view_proj.project_point3(Vec3::new(0.0, 0.0, -10.0)).z < wall);
        assert!(view_proj.project_point3(Vec3::new(0.0, 0.0, -2.0)).z > wall);

        let format = wgpu::TextureFormat::Rgba8Unorm;
        let texture = |size: u32, format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: size,
                    height: size,
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
        let color = texture(
            SIZE,
            format,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let depth = texture(
            SIZE,
            wgpu::TextureFormat::Depth32Float,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        // The atlas: one white texel, what plain colours sample.
        let atlas = texture(
            1,
            format,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        queue.write_texture(
            atlas.as_image_copy(),
            &[255; 4],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: None,
            },
            atlas.size(),
        );

        let (half, full) = (SIZE as f32 / 2.0, SIZE as f32);
        // Top left: behind the wall, depth tested. Top right: behind it, through walls.
        // Bottom left: in front of it, depth tested. Bottom right: lines behind it, the
        // depth-tested one along row 40, the one through walls along row 56.
        let quads = [
            quad(view_proj, [0.0, 0.0, half, half], 10.0, RED),
            quad(view_proj, [0.0, half, half, full], 2.0, BLUE),
        ]
        .concat();
        let quads_through = quad(view_proj, [half, 0.0, full, half], 10.0, GREEN);
        let lines = [line(view_proj, [half, full], 40.0, 10.0, MAGENTA_ARGB)];
        let lines_through = [line(view_proj, [half, full], 56.0, 10.0, MAGENTA_ARGB)];
        let batch = Batch {
            quads: &quads,
            quads_through: &quads_through,
            lines: &lines,
            lines_through: &lines_through,
        };

        let mut primitives = WorldPrimitives::new(&device, format);
        let atlas_view = atlas.create_view(&Default::default());
        assert!(primitives.prepare(
            &device,
            &mut |buffer, bytes| queue.write_buffer(buffer, 0, bytes),
            &batch,
            view_proj,
            [SIZE, SIZE],
            &atlas_view,
        ));
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
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wall),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            primitives.draw(&mut pass);
        }
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(SIZE * SIZE * 4),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(SIZE * 4),
                    rows_per_image: None,
                },
            },
            color.size(),
        );
        queue.submit([encoder.finish()]);
        readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let texels = readback.slice(..).get_mapped_range().unwrap().to_vec();
        let at = |x: u32, y: u32| {
            let i = ((y * SIZE + x) * 4) as usize;
            [texels[i], texels[i + 1], texels[i + 2], texels[i + 3]]
        };
        let black = [0, 0, 0, 255];
        assert_eq!(
            at(16, 16),
            black,
            "a depth-tested quad behind the wall is hidden"
        );
        assert_eq!(
            at(48, 16),
            [0, 255, 0, 255],
            "a quad through walls shows behind it"
        );
        assert_eq!(
            at(16, 48),
            [0, 0, 255, 255],
            "a depth-tested quad before it shows"
        );
        assert_eq!(
            at(48, 40),
            black,
            "a depth-tested line behind the wall is hidden"
        );
        assert_eq!(
            at(48, 56),
            [255, 0, 255, 255],
            "a line through walls shows behind it"
        );
        // The line is its width (4 pixels) thick, not more.
        assert_eq!(at(48, 52), black);
    }
}
