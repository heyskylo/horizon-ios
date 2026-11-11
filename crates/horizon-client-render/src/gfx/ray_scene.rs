//! The terrain as acceleration structures, with ray tracing on (#13780 step 2, after the #1180
//! spike; `ray_query`): one BLAS per section's solid quads within `reach` chunks of the camera,
//! compacted once built, and one TLAS of them, built again only when a BLAS changes or the camera
//! enters another chunk column (its instances sit relative to that column). Nothing traces them
//! yet. Without ray queries on the device there is no scene and nothing here runs.
//!
//! The section arena keeps its compact layout: a section about to get a BLAS has its solid quads
//! written out as vertices first (`Unpacker`, `rt_unpack.wgsl`), as the solid draws expand them.

use glam::DVec3;

use crate::fast_hash::FxHashMap;
use crate::gfx::{Renderer, SectionGpu, SectionKey, arena::Arena, resident::Resident};
use crate::mesher::TerrainVertex;

/// BLAS builds per frame at most: a world that just loaded builds over a few frames.
const BUILDS_PER_FRAME: usize = 512;
/// Chunks around the camera the TLAS spans, unless `HORIZON_RT_CHUNKS` says otherwise. The
/// #1180 spike's TLAS over every section cost 2-7 ms of CPU a frame at render distance 32.
const REACH: i32 = 8;

/// The renderer's side: the calls gfx.rs makes, each one line there.
impl Renderer {
    /// Section `key` got a new mesh, or lost it.
    pub(in crate::gfx) fn ray_mark(&mut self, key: SectionKey) {
        if let Some(rt) = &mut self.ray_scene {
            rt.mark(key);
        }
    }

    /// Every section is gone (a new world, a new dimension).
    pub(in crate::gfx) fn ray_clear(&mut self) {
        if let Some(rt) = &mut self.ray_scene {
            rt.clear();
        }
    }

    /// The frame's BLAS builds and TLAS, recorded in `encoder` before the world pass.
    pub(in crate::gfx) fn ray_prepare(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        camera: DVec3,
        title: bool,
    ) {
        if let Some(rt) = &mut self.ray_scene
            && !title
        {
            crate::spans::time("ray scene", || {
                rt.prepare(
                    &self.device,
                    &self.queue,
                    encoder,
                    &self.sections,
                    &self.arena,
                    &self.indices,
                    camera,
                )
            });
        }
    }
}

/// The terrain's acceleration structures.
pub(in crate::gfx) struct RayScene {
    /// Each section's BLAS, while it has solid quads within reach.
    blas: FxHashMap<SectionKey, wgpu::Blas>,
    /// Sections meshed again or emptied since their BLAS was built.
    dirty: Vec<SectionKey>,
    tlas: wgpu::Tlas,
    /// The camera's chunk column when the TLAS was built: its instances are placed relative to
    /// that column's corner, so moving inside it needs no new TLAS.
    anchor: Option<(i32, i32)>,
    /// Chunks around the anchor the TLAS spans.
    reach: i32,
    /// BLAS built this frame, to measure for compaction once submitted.
    to_compact: Vec<(SectionKey, wgpu::Blas)>,
    /// BLAS whose compacted size the GPU is measuring.
    compacting: Vec<(SectionKey, wgpu::Blas)>,
    /// Sections in the arena's compact layout, written out as vertices for their BLAS.
    unpacker: Unpacker,
}

impl RayScene {
    /// The scene, when the device has ray queries (`ray_query`).
    pub(in crate::gfx) fn new(device: &wgpu::Device) -> Option<Self> {
        if !super::on(device) {
            return None;
        }
        let reach = std::env::var("HORIZON_RT_CHUNKS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(REACH);
        Some(Self {
            blas: FxHashMap::default(),
            dirty: Vec::new(),
            tlas: create_tlas(device, 1024),
            anchor: None,
            reach,
            to_compact: Vec::new(),
            compacting: Vec::new(),
            unpacker: Unpacker::new(device, 1 << 20),
        })
    }

    fn mark(&mut self, key: SectionKey) {
        self.dirty.push(key);
    }

    fn clear(&mut self) {
        self.blas.clear();
        self.dirty.clear();
        self.to_compact.clear();
        self.compacting.clear();
        self.anchor = None;
    }

    /// Compacts BLAS built on earlier frames: built with room to spare, a compacted copy (made
    /// once the GPU has measured it) takes about a quarter. Whether a BLAS was replaced.
    fn compact(&mut self, queue: &wgpu::Queue) -> bool {
        let mut changed = false;
        let blas = &mut self.blas;
        self.compacting.retain(|(key, b)| {
            if blas.get(key) != Some(b) {
                return false;
            }
            if !b.ready_for_compaction() {
                return true;
            }
            blas.insert(*key, queue.compact_blas(b));
            changed = true;
            false
        });
        // Built and submitted last frame: the GPU can now measure them.
        for (key, b) in std::mem::take(&mut self.to_compact) {
            if self.blas.get(&key) == Some(&b) {
                b.prepare_compaction_async(|_| {});
                self.compacting.push((key, b));
            }
        }
        changed
    }

    /// Builds the BLAS of sections marked since the last frame (at most `BUILDS_PER_FRAME`, and
    /// as many as the unpacker has room for), and the TLAS again when a BLAS changed or the
    /// camera entered another chunk column.
    #[allow(clippy::too_many_arguments)]
    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        sections: &Resident<SectionGpu>,
        arena: &Arena,
        indices: &wgpu::Buffer,
        camera: DVec3,
    ) {
        let column = crate::gfx::resident::camera_column(camera);
        // Only sections within `reach` get a BLAS (at render distance 32 all of them took 1.3 GB
        // of video memory); those past `reach + 2` lose theirs, so walking along a chunk border
        // doesn't build the same ones over and over.
        let keep = self.reach + 2;
        let mut changed = self.compact(queue);
        if self.anchor != Some(column) {
            let before = self.blas.len();
            self.blas.retain(|k, _| within(k, column, keep));
            changed |= self.blas.len() != before;
            for (key, s, _) in sections.iter() {
                if within(&key, column, self.reach) && s.solid > 0 && !self.blas.contains_key(&key)
                {
                    self.dirty.push(key);
                }
            }
        }
        let take = self.dirty.len().min(BUILDS_PER_FRAME);
        let mut builds: Vec<Build<'_>> = Vec::new();
        let mut deferred = Vec::new();
        // Vertices the unpacker writes this frame.
        let mut unpacked = 0;
        for key in self.dirty.drain(..take) {
            if builds.iter().any(|b| b.key == key) {
                continue;
            }
            match sections
                .find(key)
                .filter(|s| s.solid > 0 && within(&key, column, self.reach))
            {
                Some(s) => {
                    let at = if s.packed {
                        let vertices = u64::from(s.solid) * 4;
                        if unpacked + vertices > self.unpacker.capacity {
                            if unpacked > 0 {
                                // The rest waits for the next frame's room.
                                deferred.push(key);
                                continue;
                            }
                            self.unpacker.grow(device, vertices);
                        }
                        unpacked += vertices;
                        Some(unpacked - vertices)
                    } else {
                        None
                    };
                    let (size, blas) = new_blas(device, s.solid);
                    builds.push(Build {
                        key,
                        s,
                        size,
                        blas,
                        unpacked: at,
                    });
                }
                None => changed |= self.blas.remove(&key).is_some(),
            }
        }
        self.dirty.extend(deferred);
        if builds.is_empty() && !changed && self.anchor == Some(column) {
            return;
        }
        self.unpacker.run(device, encoder, arena, &builds);
        for b in &builds {
            self.blas.insert(b.key, b.blas.clone());
            self.to_compact.push((b.key, b.blas.clone()));
        }
        let near = |k: &SectionKey| within(k, column, self.reach);
        let count = self.blas.keys().filter(|k| near(k)).count();
        if count > self.tlas.get().len() {
            self.tlas = create_tlas(device, (count as u32).next_power_of_two());
        }
        let min_y = sections.min_y();
        let mut n = 0;
        for (key, blas) in self.blas.iter().filter(|(k, _)| near(k)) {
            let o = instance_origin(*key, min_y, column);
            self.tlas[n] = Some(wgpu::TlasInstance::new(
                blas,
                [
                    1.0, 0.0, 0.0, o[0], 0.0, 1.0, 0.0, o[1], 0.0, 0.0, 1.0, o[2],
                ],
                0,
                0xff,
            ));
            n += 1;
        }
        for i in n..self.tlas.get().len() {
            if self.tlas[i].is_none() {
                break;
            }
            self.tlas[i] = None;
        }
        let entries = build_entries(&builds, arena, &self.unpacker, indices);
        encoder.build_acceleration_structures(entries.iter(), std::iter::once(&self.tlas));
        self.anchor = Some(column);
    }
}

/// Whether section `key` is within `reach` chunks of column `column`, along x and z.
fn within(key: &SectionKey, column: (i32, i32), reach: i32) -> bool {
    (key.0 - column.0).abs() <= reach && (key.2 - column.1).abs() <= reach
}

/// A BLAS to build this frame.
struct Build<'a> {
    key: SectionKey,
    s: &'a SectionGpu,
    size: wgpu::BlasTriangleGeometrySizeDescriptor,
    blas: wgpu::Blas,
    /// Its first vertex in the unpacker's buffer, for a section in the compact layout.
    unpacked: Option<u64>,
}

impl Build<'_> {
    /// The buffer holding its solid quads as vertices, and its first vertex there.
    fn vertices<'b>(&self, arena: &'b Arena, unpacker: &'b Unpacker) -> (&'b wgpu::Buffer, u64) {
        match self.unpacked {
            Some(at) => (&unpacker.out, at),
            None => (arena.buffer(self.s.page), self.s.start),
        }
    }
}

/// `rt_unpack.wgsl`'s `RtJob`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct RtJob {
    base: u32,
    quads: u32,
    out: u32,
    pad: u32,
}

/// Writes the solid quads of sections in the arena's compact layout out as vertices, for their
/// BLAS (BLAS builds read vertices; the records are only for the draws to expand). The buffer
/// is reused every frame: a frame takes only the sections it has room for.
struct Unpacker {
    pipeline: wgpu::ComputePipeline,
    out: wgpu::Buffer,
    /// Vertices `out` holds.
    capacity: u64,
}

impl Unpacker {
    fn new(device: &wgpu::Device, capacity: u64) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Ray tracing: unpack sections"),
            source: wgpu::ShaderSource::Wgsl(unpack_shader().into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Ray tracing: unpack sections"),
            layout: None,
            module: &module,
            entry_point: Some("cs_rt_unpack"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self {
            pipeline,
            out: unpacked_buffer(device, capacity),
            capacity,
        }
    }

    /// Room for at least `vertices` (one section larger than the buffer).
    fn grow(&mut self, device: &wgpu::Device, vertices: u64) {
        self.capacity = vertices.next_power_of_two();
        self.out = unpacked_buffer(device, self.capacity);
    }

    /// Writes out the builds' sections that are in the compact layout, one dispatch per arena
    /// page.
    fn run(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        arena: &Arena,
        builds: &[Build<'_>],
    ) {
        let mut pages: Vec<u32> = builds
            .iter()
            .filter(|b| b.unpacked.is_some())
            .map(|b| b.s.page)
            .collect();
        pages.sort_unstable();
        pages.dedup();
        if pages.is_empty() {
            return;
        }
        let empty = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Ray tracing: unpack sections"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &empty, &[]);
        for page in pages {
            let jobs: Vec<RtJob> = builds
                .iter()
                .filter_map(|b| {
                    let at = b.unpacked.filter(|_| b.s.page == page)?;
                    Some(RtJob {
                        base: b.s.solid_base(),
                        quads: b.s.solid,
                        out: at as u32,
                        pad: 0,
                    })
                })
                .collect();
            let jobs_buffer = wgpu::util::DeviceExt::create_buffer_init(
                device,
                &wgpu::util::BufferInitDescriptor {
                    label: Some("Ray tracing: unpack jobs"),
                    contents: bytemuck::cast_slice(&jobs),
                    usage: wgpu::BufferUsages::STORAGE,
                },
            );
            let arena_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.pipeline.get_bind_group_layout(1),
                entries: &[wgpu::BindGroupEntry {
                    binding: 19,
                    resource: arena.buffer(page).as_entire_binding(),
                }],
            });
            let out_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.pipeline.get_bind_group_layout(2),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: jobs_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: self.out.as_entire_binding(),
                    },
                ],
            });
            let most = jobs.iter().map(|j| j.quads * 4).max().unwrap_or(0);
            pass.set_bind_group(1, &arena_group, &[]);
            pass.set_bind_group(2, &out_group, &[]);
            pass.dispatch_workgroups(most.div_ceil(64), jobs.len() as u32, 1);
        }
    }
}

/// terrain.wgsl with the unpacking entry point after it, which reads sections as its solid
/// draws do (`section_corner`).
pub(in crate::gfx) fn unpack_shader() -> String {
    crate::gfx::terrain_shader() + include_str!("../rt_unpack.wgsl")
}

fn unpacked_buffer(device: &wgpu::Device, vertices: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Ray tracing: unpacked sections"),
        size: vertices * std::mem::size_of::<TerrainVertex>() as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::BLAS_INPUT,
        mapped_at_creation: false,
    })
}

fn create_tlas(device: &wgpu::Device, capacity: u32) -> wgpu::Tlas {
    device.create_tlas(&wgpu::CreateTlasDescriptor {
        label: Some("Terrain sections"),
        max_instances: capacity,
        flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE,
        update_mode: wgpu::AccelerationStructureUpdateMode::Build,
    })
}

/// A new BLAS for `solid` quads, and its size.
fn new_blas(
    device: &wgpu::Device,
    solid: u32,
) -> (wgpu::BlasTriangleGeometrySizeDescriptor, wgpu::Blas) {
    let size = wgpu::BlasTriangleGeometrySizeDescriptor {
        vertex_format: wgpu::VertexFormat::Float32x3,
        vertex_count: solid * 4,
        index_format: Some(wgpu::IndexFormat::Uint32),
        index_count: Some(solid * 6),
        flags: wgpu::AccelerationStructureGeometryFlags::OPAQUE,
    };
    let blas = device.create_blas(
        &wgpu::CreateBlasDescriptor {
            label: None,
            flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE
                | wgpu::AccelerationStructureFlags::ALLOW_COMPACTION,
            update_mode: wgpu::AccelerationStructureUpdateMode::Build,
        },
        wgpu::BlasGeometrySizeDescriptors::Triangles {
            descriptors: vec![size.clone()],
        },
    );
    (size, blas)
}

/// Build entries reading each section's solid vertices (`pos` is `TerrainVertex`'s first field)
/// with the shared quad indices.
fn build_entries<'a>(
    builds: &'a [Build<'a>],
    arena: &'a Arena,
    unpacker: &'a Unpacker,
    indices: &'a wgpu::Buffer,
) -> Vec<wgpu::BlasBuildEntry<'a>> {
    let stride = std::mem::size_of::<TerrainVertex>() as u64;
    builds
        .iter()
        .map(|b| {
            let (source, first) = b.vertices(arena, unpacker);
            wgpu::BlasBuildEntry {
                blas: &b.blas,
                geometry: wgpu::BlasGeometries::TriangleGeometries(vec![
                    wgpu::BlasTriangleGeometry {
                        size: &b.size,
                        vertex_buffer: source,
                        first_vertex: first as u32,
                        vertex_stride: stride,
                        index_buffer: Some(indices),
                        first_index: Some(0),
                        transform_buffer: None,
                        transform_buffer_offset: None,
                    },
                ]),
            }
        })
        .collect()
}

/// Where section `key` sits relative to the corner of chunk column `anchor`.
fn instance_origin(key: SectionKey, min_y: i32, anchor: (i32, i32)) -> [f32; 3] {
    [
        ((key.0 - anchor.0) * 16) as f32,
        (min_y + key.1 * 16) as f32,
        ((key.2 - anchor.1) * 16) as f32,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instances_sit_relative_to_the_anchor_column() {
        // Section 5 of column (3, -2), the world starting at y = -64, seen from column (2, -2).
        assert_eq!(instance_origin((3, 5, -2), -64, (2, -2)), [16.0, 16.0, 0.0]);
        assert_eq!(instance_origin((2, 0, -2), -64, (2, -2)), [0.0, -64.0, 0.0]);
    }

    #[test]
    fn the_unpack_shader_validates_with_terrain() {
        let source = unpack_shader();
        let module = naga::front::wgsl::parse_str(&source).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }

    /// The off path is main's: a device opened without ray queries (every device without
    /// `HORIZON_RAY_TRACING`, and every plain build) gets no scene, and its buffers no usage.
    #[test]
    fn without_ray_queries_there_is_no_scene() {
        let instance = crate::gfx::test_gpu::instance();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            return;
        };
        let Ok((device, _)) = pollster::block_on(adapter.request_device(&Default::default()))
        else {
            return;
        };
        assert!(!super::super::on(&device));
        assert!(RayScene::new(&device).is_none());
        assert!(super::super::blas_input(&device).is_empty());
    }
}
