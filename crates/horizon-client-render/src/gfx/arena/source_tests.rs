//! Original sidecar lifetime controls, executed explicitly on an offscreen adapter.
use super::*;
use crate::mesher::{TerrainSource, TerrainVertex};

fn read(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    offset: u64,
    size: u64,
) -> Vec<u8> {
    let out = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, offset, &out, 0, size);
    queue.submit([encoder.finish()]);
    out.slice(..).map_async(wgpu::MapMode::Read, |r| r.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    out.slice(..).get_mapped_range().unwrap().to_vec()
}

#[test]
#[ignore = "offscreen GPU control; run explicitly with an available adapter"]
fn terrain_source_arena_gpu_grow_move_recycle_and_refusal() {
    let instance = crate::gfx::test_gpu::instance();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .expect("GPU control requires an adapter");
    eprintln!("TERRAIN_SOURCE_ARENA_ADAPTER {:?}", adapter.get_info());
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let mut arena = Arena::new(&device, 24, GRAIN, true);
    arena.page_limit = GRAIN * 2;
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut uploads = super::super::upload::Uploads::new(&device);
    let first = arena.alloc(&device, &mut encoder, 4).unwrap();
    assert!(
        arena.sources(first.0).is_none(),
        "default geometry allocates no source buffer"
    );
    let geometry = [TerrainVertex::new([2.25, 3.75, 4.5], [0.25, 0.75], 7, 0xff123456, 0x8070); 4];
    uploads.buffer(
        &mut encoder,
        arena.buffer(first.0),
        first.1 * 24,
        bytemuck::cast_slice(&geometry),
    );
    let source = TerrainSource::new(71, [2, 3, 4], 13, false);
    arena.write_sources(
        &device,
        &mut encoder,
        &mut uploads,
        first.0,
        first.1,
        4,
        &[source],
    );
    let second = arena.alloc(&device, &mut encoder, GRAIN).unwrap();
    assert_eq!(second, (first.0, GRAIN), "grew the first page");
    let third = arena.alloc(&device, &mut encoder, GRAIN).unwrap();
    assert_ne!(third.0, first.0, "move crosses pages");
    arena.copy(&mut encoder, first, third, 4);
    arena.release(first.0, first.1, 4);
    let reused = arena.alloc_existing(4).unwrap();
    assert_eq!(reused, first);
    arena.write_sources(
        &device,
        &mut encoder,
        &mut uploads,
        reused.0,
        reused.1,
        4,
        &[],
    );
    uploads.finish(&encoder);
    queue.submit([encoder.finish()]);
    assert_eq!(
        read(
            &device,
            &queue,
            arena.sources(third.0).unwrap(),
            third.1 / 4 * 8,
            8
        ),
        bytemuck::bytes_of(&source)
    );
    assert_eq!(
        read(
            &device,
            &queue,
            arena.sources(first.0).unwrap(),
            first.1 / 4 * 8,
            8
        ),
        [0; 8]
    );
    assert_eq!(
        read(&device, &queue, arena.buffer(third.0), third.1 * 24, 96),
        bytemuck::cast_slice::<_, u8>(&geometry)
    );

    // A real requested binding limit refuses metadata/table allocation. The native
    // vertex allocation still fits; this is a controlled refusal, not a real OOM.
    let limits = wgpu::Limits {
        max_storage_buffer_binding_size: 256,
        ..Default::default()
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: limits,
        ..Default::default()
    }))
    .unwrap();
    let mut arena = Arena::translucent(&device, 24, GRAIN);
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut uploads = super::super::upload::Uploads::new(&device);
    let at = arena.alloc(&device, &mut encoder, 4).unwrap();
    uploads.buffer(
        &mut encoder,
        arena.buffer(at.0),
        at.1 * 24,
        bytemuck::cast_slice(&geometry),
    );
    arena.write_sources(
        &device,
        &mut encoder,
        &mut uploads,
        at.0,
        at.1,
        4,
        &[source],
    );
    assert!(
        arena.sources(at.0).is_none(),
        "sidecar refused without losing vertices"
    );
    assert!(super::super::pack_post::block_ids(&device, &queue, &[0; 128], &[0, 1]).is_none());
    uploads.finish(&encoder);
    queue.submit([encoder.finish()]);
    assert_eq!(
        read(&device, &queue, arena.buffer(at.0), at.1 * 24, 96),
        bytemuck::cast_slice::<_, u8>(&geometry)
    );
}
