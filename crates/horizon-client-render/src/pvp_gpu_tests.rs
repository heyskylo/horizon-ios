use wgpu::util::DeviceExt;

/// Run the actual entity overlay function, including the zero-valued opaque black texel.
#[test]
#[ignore = "requires a Vulkan/Metal/DX12 adapter"]
fn custom_hit_colours_preserve_every_rgba_value() {
    let instance = crate::gfx::test_gpu::instance();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let mut cases = Vec::<[u32; 4]>::new();
    let mut expected = Vec::<[u32; 4]>::new();
    for custom in [false, true] {
        for colour in super::Colour::ALL {
            for opacity in [0, 30, 100] {
                let pvp = super::Pvp {
                    hit: super::Hit {
                        custom,
                        colour,
                        opacity,
                    },
                    ..Default::default()
                };
                let frame = pvp.frame();
                for overlay in [0, 1, 5, 8, 17, 31] {
                    cases.push([frame.hurt_texel, u32::from(frame.custom_hit), overlay, 0]);
                    expected.push(
                        match overlay {
                            1 | 8 if custom => {
                                let [_, r, g, b] = colour.argb().to_be_bytes();
                                [r, g, b, (255 - (u32::from(opacity) * 255 + 50) / 100) as u8]
                            }
                            1 | 8 => crate::overlay::HURT_TEXEL,
                            5 | 31 => [255, 255, 255, crate::overlay::white_alpha(15)],
                            17 => [255, 255, 255, crate::overlay::white_alpha(1)],
                            _ => [255; 4],
                        }
                        .map(u32::from),
                    );
                }
            }
        }
    }
    let entity = include_str!("entity.wgsl");
    let start = entity.find("fn overlay_texel(").unwrap();
    let end = entity[start..].find("\n}").unwrap() + start + 2;
    let source = format!(
        "{}\n{}\n{}",
        crate::overlay::wgsl_constants(),
        &entity[start..end],
        r#"
struct Globals { colored_count: vec4<u32> }
var<private> globals: Globals;
@group(0) @binding(0) var<storage, read> cases: array<vec4<u32>>;
@group(0) @binding(1) var<storage, read_write> results: array<vec4<u32>>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= arrayLength(&cases) { return; }
    let c = cases[id.x];
    globals.colored_count = vec4(0u, c.x, c.y, 0u);
    results[id.x] = vec4<u32>(round(overlay_texel(c.z) * 255.0));
}"#
    );
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Custom hit colours"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&cases),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let size = (cases.len() * 16) as u64;
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: input.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: output.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups((cases.len() as u32).div_ceil(64), 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, size);
    queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    readback.map_async(wgpu::MapMode::Read, .., move |r| tx.send(r).unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();
    let mapped = readback.get_mapped_range(..).unwrap();
    let actual: &[[u32; 4]] = bytemuck::cast_slice(&mapped);
    for (i, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(*actual, expected, "case {i}: {:?}", cases[i]);
    }
}
