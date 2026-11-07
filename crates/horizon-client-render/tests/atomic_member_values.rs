//! An ordinary read of a struct or buffer-block member that an atomic function also uses gets
//! the member's current value (#9081), on a host GPU through the translated program.

use horizon_shaders::{Macros, Pack, preprocess, translate_compute};

/// Runs compute program `source` once (one work group), with `probe_buffer` (set 4, binding 2)
/// starting at `start`, and returns `probe_result.result` (set 4, binding 1).
fn run(device: &wgpu::Device, queue: &wgpu::Queue, source: &str, start: [u32; 2]) -> u32 {
    use std::io::Write;
    use wgpu::util::DeviceExt;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file(
        "shaders/deferred.csh",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(source.as_bytes()).unwrap();
    let pack =
        Pack::from_zip_bytes("atomic-values.zip", zip.finish().unwrap().into_inner()).unwrap();
    let program = translate_compute(&preprocess(&pack, "deferred.csh", &Macros::standard()))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(program.module)),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let storage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC;
    let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(program.interface.uniform_size.max(48)),
        usage: wgpu::BufferUsages::UNIFORM,
        mapped_at_creation: false,
    });
    let result = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: &[0; 4],
        usage: storage,
    });
    let probe: Vec<u8> = start.iter().flat_map(|v| v.to_le_bytes()).collect();
    let probe = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: &probe,
        usage: storage,
    });
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 4,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let groups: Vec<wgpu::BindGroup> = (0..5)
        .map(|set| {
            let entries: Vec<wgpu::BindGroupEntry> = match set {
                0 => vec![wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                }],
                4 if source.contains("probe_buffer") => vec![
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: result.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: probe.as_entire_binding(),
                    },
                ],
                4 => vec![wgpu::BindGroupEntry {
                    binding: 1,
                    resource: result.as_entire_binding(),
                }],
                _ => Vec::new(),
            };
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(set),
                entries: &entries,
            })
        })
        .collect();
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        for (set, group) in groups.iter().enumerate() {
            pass.set_bind_group(set as u32, group, &[]);
        }
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&result, 0, &read, 0, 4);
    queue.submit([encoder.finish()]);
    read.slice(..)
        .map_async(wgpu::MapMode::Read, |r| r.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let bytes = read.slice(..).get_mapped_range().unwrap();
    u32::from_le_bytes(bytes[..4].try_into().unwrap())
}

#[test]
#[ignore = "requires a host GPU; run with --ignored --nocapture"]
fn reads_of_atomic_members_see_the_atomic_updates() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    eprintln!("Atomic member GPU: {}", adapter.get_info().name);
    // Pack buffers sit in bind group 4, as in the game.
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits {
            max_bind_groups: 5,
            ..Default::default()
        },
        ..Default::default()
    }))
    .unwrap();
    let result = "layout(std430, binding = 1) buffer probe_result { uint result; };\n";
    // Eight invocations each add 1 to a zeroed shared member, then read it: 8.
    for (host, member) in [
        ("shared struct { uint other; uint count; } sh;", "sh.count"),
        (
            "struct Counter { uint count; uint other; };\nshared Counter counters[2];",
            "counters[1].count",
        ),
    ] {
        for (read, expected) in [
            ("result = M * 10u + 1u;", 81),
            ("uint r = M; result = r;", 8),
            ("result = uint(float(M) * 0.5);", 4),
            ("result = min(M, 5u);", 5),
        ] {
            let source = format!(
                "#version 460 core\nlayout(local_size_x = 8) in;\n{host}\n{result}\
                 void main() {{\n    {member} = 0u;\n    barrier();\n    \
                 atomicAdd({member}, 1u);\n    barrier();\n    {}\n}}\n",
                read.replace('M', member)
            );
            assert_eq!(
                run(&device, &queue, &source, [0, 0]),
                expected,
                "{member}: {read}"
            );
        }
    }
    // One invocation adds 2 to a buffer member that starts at 5, then reads it: 7.
    for (read, expected) in [
        ("result = pc.count * 10u + 1u;", 71),
        ("uint r = pc.count; result = r;", 7),
        ("result = uint(int(pc.count) - 10 + 100);", 97),
    ] {
        let source = format!(
            "#version 460 core\nlayout(local_size_x = 1) in;\n\
             layout(std430, binding = 2) buffer probe_buffer {{ uint count; uint other; }} pc;\n\
             {result}void main() {{\n    atomicAdd(pc.count, 2u);\n    {read}\n}}\n"
        );
        assert_eq!(run(&device, &queue, &source, [5, 0]), expected, "{read}");
    }
}
