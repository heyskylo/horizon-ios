//! Sorted still-fluid material: ordinary back culling, alpha blend and color/depth writes.
pub(super) const SAMPLER: wgpu::SamplerDescriptor<'static> = wgpu::SamplerDescriptor {
    label: Some("Still fluid (nearest texels, linear mipmaps)"),
    address_mode_u: wgpu::AddressMode::ClampToEdge,
    address_mode_v: wgpu::AddressMode::ClampToEdge,
    address_mode_w: wgpu::AddressMode::ClampToEdge,
    mag_filter: wgpu::FilterMode::Nearest,
    min_filter: wgpu::FilterMode::Nearest,
    mipmap_filter: wgpu::MipmapFilterMode::Linear,
    lod_min_clamp: 0.0,
    lod_max_clamp: 32.0,
    compare: None,
    anisotropy_clamp: 1,
    border_color: None,
};

pub(super) fn pipeline(
    device: &wgpu::Device,
    layout: Option<&wgpu::PipelineLayout>,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    constants: &[(&str, f64)],
) -> wgpu::RenderPipeline {
    let attributes = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Uint32,3=>Unorm8x4,4=>Uint8x4,5=>Snorm8x4,6=>Float32x2];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Still fluid"),
        layout,
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_native_fluid"),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants,
                ..Default::default()
            },
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
            entry_point: Some("fs_native_fluid"),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants,
                ..Default::default()
            },
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
