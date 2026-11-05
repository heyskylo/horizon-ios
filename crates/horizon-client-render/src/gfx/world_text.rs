//! NORMAL world text: alpha blended, back-face culled, ordinary depth writes.
pub(super) fn pipeline(
    device: &wgpu::Device,
    layout: Option<&wgpu::PipelineLayout>,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let attributes = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Uint32,3=>Unorm8x4,4=>Uint8x4,5=>Snorm8x4,6=>Float32x2];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Normal world text"),
        layout,
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_entity"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<super::Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attributes,
            })],
        },
        primitive: wgpu::PrimitiveState {
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_entity"),
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::util::DeviceExt;
    #[test]
    #[ignore = "requires a Vulkan/Metal/DX12 adapter"]
    fn world_text_blends_culls_and_obeys_world_depth() {
        let instance = crate::gfx::test_gpu::instance();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        // Use the production pipeline/vertex layout with a pass-through shader
        // to isolate its actual depth, winding and blend behavior from lighting.
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(
                r#"
struct Out { @builtin(position) pos:vec4<f32>, @location(0) color:vec4<f32> }
@vertex fn vs_entity(@location(0) pos:vec3<f32>, @location(3) color:vec4<f32>)->Out {
 return Out(vec4(pos,1.0),color);
}
@fragment fn fs_entity(v:Out)->@location(0) vec4<f32> { return v.color; }
"#
                .into(),
            ),
        });
        let pipeline = pipeline(&device, None, &shader, wgpu::TextureFormat::Rgba8Unorm);
        let texture = |format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 4,
                    height: 1,
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
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let depth = texture(
            wgpu::TextureFormat::Depth32Float,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut quad = |pixel: u32, z: f32, color: [u8; 4], back: bool| {
            let x = -1.0 + pixel as f32 * 0.5;
            let first = vertices.len() as u32;
            for pos in [
                [x, -1.0, z],
                [x + 0.5, -1.0, z],
                [x + 0.5, 1.0, z],
                [x, 1.0, z],
            ] {
                vertices.push(super::super::Vertex {
                    pos,
                    uv: [0.0; 2],
                    color: u32::from_le_bytes(color),
                    layer: 0,
                    light: 0,
                    normal: [0; 4],
                    glint_uv: [0.0; 2],
                });
            }
            for i in if back {
                [0, 2, 1, 0, 3, 2]
            } else {
                [0, 1, 2, 0, 2, 3]
            } {
                indices.push(first + i);
            }
        };
        quad(0, 0.6, [255, 0, 0, 255], false);
        // A hair behind: the entity model pipeline's 128-ulp depth bias
        // would incorrectly put the green quad in front of the red surface.
        quad(0, 0.599_999, [0, 255, 0, 255], false);
        quad(1, 0.8, [0, 255, 0, 255], false);
        quad(1, 0.7, [255, 0, 0, 255], false);
        quad(2, 0.5, [255, 0, 0, 128], false);
        quad(3, 0.5, [255, 255, 0, 255], true);
        let buffer = |data: &[u8], usage| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: data,
                usage,
            })
        };
        let vertex = buffer(bytemuck::cast_slice(&vertices), wgpu::BufferUsages::VERTEX);
        let index = buffer(bytemuck::cast_slice(&indices), wgpu::BufferUsages::INDEX);
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let color = color.create_view(&Default::default());
            let depth = depth.create_view(&Default::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLUE),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            pass.set_vertex_buffer(0, vertex.slice(..));
            pass.set_index_buffer(index.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..indices.len() as u32, 0, 0..1);
        }
        encoder.copy_texture_to_buffer(
            color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: None,
                },
            },
            color.size(),
        );
        queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback.map_async(wgpu::MapMode::Read, .., move |result| {
            tx.send(result).unwrap()
        });
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        rx.recv().unwrap().unwrap();
        let pixels = readback.get_mapped_range(..).unwrap();
        assert_eq!(
            &pixels[..16],
            &[
                255, 0, 0, 255, 0, 255, 0, 255, 128, 0, 127, 255, 0, 0, 255, 255
            ]
        );
    }
}
