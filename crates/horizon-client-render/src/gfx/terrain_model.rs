//! Opaque/cutout terrain-sheet native models, with ordinary culling and depth writes.
pub(super) fn pipelines(
    device: &wgpu::Device,
    layout: Option<&wgpu::PipelineLayout>,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    sprite_constants: &[(&str, f64)],
) -> [wgpu::RenderPipeline; 3] {
    std::array::from_fn(|i| {
        let mut constants = sprite_constants.to_vec();
        constants.push((
            "HDR_WRITE_LIGHT",
            f64::from(u8::from(format == super::LINEAR_FRAME)),
        ));
        constants.push(("NATIVE_MODEL_ALPHA_CUTOFF", [0.0, 0.1, 0.5][i]));
        // The ordinary Vertex ABI is unchanged. This material's private payload uses
        // normal's four bytes for a float z, and glint UV for float normal x/y.
        let attributes = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Uint32,3=>Unorm8x4,4=>Uint8x4,5=>Uint32,6=>Float32x2];
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(
                [
                    "Native model solid",
                    "Native model cutout",
                    "Native model cutout mipmapped",
                ][i],
            ),
            layout,
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_native_model"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &constants,
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
                entry_point: Some("fs_native_model"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &constants,
                    ..Default::default()
                },
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    })
}
