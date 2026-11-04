//! GPU culling: the frame's near solid sections culled and their draws listed
//! by a compute pass, and drawn with one `multi_draw_indexed_indirect_count`
//! per arena page, where the device can draw a count the GPU wrote
//! (`MULTI_DRAW_INDIRECT_COUNT`: DX12, Vulkan 1.2 or
//! `VK_KHR_draw_indirect_count`). Walking those sections on the CPU, listing a
//! few draws per section by the sides it shows the eye and grouping them by
//! page costs the render thread time in proportion to the sections in view.
//!
//! Every resident section with solid quads has a record here, under an id it
//! keeps for as long as it stays (`SectionGpu::gpu_id`; resident slots move on
//! every removal). The pass keeps the sections whose column is within the
//! render distance (`streaming::in_view_distance`), that cave culling's graph
//! reaches and whose box is in the frustum, and draws all the solid quads of
//! each in one draw (the pipelines cull the faces turned away), with the origin
//! the walk would give it: `resident::origin`'s f32 bits, copied, never
//! computed on the GPU. Within the render distance the CPU's walk then lists
//! only the sections whose translucent quads it draws, that have no record, or
//! that fade (`Resident::walk`); past it, it lists every section as before. A
//! section whose instance carries a fade, fading in or out, has its record
//! flagged (`FADING`, gfx.rs `mark_fades`) until it is whole: the pass leaves
//! it out, and the CPU draws it with its fade.
//!
//! The graph is the one the walk reads that frame (`occlusion::Reach`). Its
//! search keeps the cells it reached as bits too, and `Occlusion` a generation
//! that changes whenever an answer of its `contains` may: the pass keeps a copy
//! of the bits, sent through the staging belt when the generation moves, and
//! the shader's `reached` answers as `contains` does, the sky band above a
//! clamped box included.
//!
//! The integer frustum test (`tables`, `rounded_box`) keeps every section the
//! walk's would and a thin shell more, alike on every backend; a frame whose
//! sums could leave i32 is drawn by the CPU (`Prepared::Cpu`).
//!
//! The pass `apply`s the records that changed, then `cull` keeps the sections
//! in view and counts them per page and block of ids, `scan` turns the counts
//! into where each block's draws start, and `scatter` writes every block's
//! draws there in id order. Appended with atomics alone, a page's draws would
//! come in the order the GPU happens to schedule them, and where faces of two
//! sections meet at exactly the same depth the later draw keeps the pixel, so
//! such pixels would flicker; in id order they differ from the CPU's
//! front-to-back order only there, and stay put.
//!
//! This is not `cull` (occlusion culling, `HORIZON_GPU_CULL`), which tests the
//! CPU's lists against phase 1's depth: frames drawn from here leave it off.
//!
//! wgpu checks a count draw only against the size of its buffers, never draw by
//! draw, so every draw here is in bounds by construction: its index count is
//! its section's own solid quads times 6, which the quad index buffer covers
//! (it grows before a larger mesh uploads); its base vertex starts its
//! section's own range of the page it is drawn from; its first instance is an
//! id below the table's end, which `origins` holds; and each page's count is
//! clamped to its region, which `scatter` never writes past. A region has room
//! for every record of its page, so in practice nothing is clamped.
use super::{
    Frustum, SectionKey,
    arena::{Arena, try_create},
    occlusion::Reach,
    pack_timing::PassTiming,
    resident,
    submit::Queue,
    upload::Uploads,
};
use glam::{DVec3, DVec4, Vec3};
use std::{cmp::Reverse, collections::BinaryHeap, time::Instant};

/// One section (`gpu_culling.wgsl`'s `Section`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct Record {
    /// Section x, y (from the level's bottom), z and arena page; -1: a free id.
    key: [i32; 4],
    /// The first vertex of its solid quads (a draw's base vertex, an i32: a
    /// page holds fewer vertices), its solid quads, its box (`rounded_box`),
    /// then its flags (`FADING`).
    a: [u32; 4],
}

/// In `Record::a[3]`: the CPU draws the section's solid quads while it fades
/// (gfx.rs `SectionGpu::gpu_fading`), and the pass leaves it out.
const FADING: u32 = 1;

impl Record {
    const FREE: Self = Self {
        key: [0, 0, 0, -1],
        a: [0; 4],
    };
    const BYTES: u64 = std::mem::size_of::<Self>() as u64;
    /// `quads` solid quads from vertex `start` of arena page `page`, within the
    /// box `boxed` (`rounded_box`).
    pub fn new(key: SectionKey, page: u32, start: u64, quads: u32, boxed: u32) -> Self {
        Self {
            key: [key.0, key.1, key.2, page as i32],
            a: [start as u32, quads, boxed, 0],
        }
    }
    /// The record, `FADING` when `on`.
    pub fn fading(mut self, on: bool) -> Self {
        if on {
            self.a[3] |= FADING;
        }
        self
    }
    fn page(&self) -> Option<u32> {
        u32::try_from(self.key[3]).ok()
    }
    fn is_fading(&self) -> bool {
        (self.a[3] & FADING) != 0
    }
}

/// `rounded_box` for bounds past what a box's codes hold, or not finite: no
/// frustum plane rejects it, so it is drawn wherever its column is within the
/// render distance.
const UNBOUNDED: u32 = 1 << 31;
/// What a box's ends, -8 to 23 blocks from its section's origin, add to make
/// their 5-bit codes.
const CODE_OFFSET: i32 = 8;

/// A record's box: its section's quad bounds (`SectionMesh::bounds`; `None` is
/// the whole cell) rounded out to whole blocks, each low end at most 16 and
/// each high end at least 0, so that along every axis the box reaches its cell.
/// A column the walk finds entirely inside the frustum then holds no box the
/// frustum test rejects, whatever its bounds. Packed as 5-bit codes: the low
/// ends along x, y and z in bits 0 to 14, the high ends in bits 15 to 29.
/// `UNBOUNDED` where an end is past -8 to 23 (never clamped inward) or the
/// bounds are not finite.
pub(super) fn rounded_box(bounds: Option<[Vec3; 2]>) -> u32 {
    let [min, max] = bounds.unwrap_or([Vec3::ZERO, Vec3::splat(16.0)]);
    let mut packed = 0;
    for axis in 0..3 {
        // `as` takes NaN to 0: tested first.
        if !(min[axis].is_finite() && max[axis].is_finite()) {
            return UNBOUNDED;
        }
        let low = (min[axis].floor() as i32).min(16);
        let high = (max[axis].ceil() as i32).max(0);
        if low < -CODE_OFFSET || high > 31 - CODE_OFFSET {
            return UNBOUNDED;
        }
        packed |= (((low + CODE_OFFSET) as u32) << (5 * axis))
            | (((high + CODE_OFFSET) as u32) << (15 + 5 * axis));
    }
    packed
}

/// A changed record, for `apply` (`gpu_culling.wgsl`'s `Update`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Update {
    /// Its id, and padding to the record's alignment.
    id: [u32; 4],
    record: Record,
}

impl Update {
    const BYTES: u64 = std::mem::size_of::<Self>() as u64;
}

/// `gpu_culling.wgsl`'s `Params`.
#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    /// Ids the pass tests, and the `Update`s `apply` writes.
    ids: u32,
    updates: u32,
    /// Arena pages the counts and regions cover (at least one), and where the
    /// regions start in `tables`, in words.
    pages: u32,
    regions: u32,
    /// Bits 3j, 3j + 1 and 3j + 2: frustum plane j's x, y and z are at least 0.
    signs: u32,
    /// The render distance in columns, and the camera's column.
    near: i32,
    cam_x: i32,
    cam_z: i32,
    /// The tables' first column: x, 0, z, 0.
    first: [i32; 4],
    /// Their entries along x, y and z, and 0.
    cells: [u32; 4],
    /// Each frustum plane's w term, its margin in (`tables`), then 0, 0.
    w: [[i32; 4]; 2],
    /// Cave culling's graph (`Reach`): from x, from z, radius, lowest section y.
    reach: [i32; 4],
    /// Its height in sections, and the level's (the sky band's top section).
    reach_height: i32,
    reach_sections: i32,
    /// `REACH`, `SKY`.
    flags: u32,
    pad: u32,
}

/// Counts before the pages' (the shader's `STATS`): the sections `cull` kept,
/// and the draws, fewer only where a region was short.
const STATS: u64 = 2;
/// In `Params::flags` (the shader's `REACH`): cave culling's graph is in
/// `reach`.
const REACH: u32 = 1;
/// In `Params::flags` (the shader's `SKY`): its search reached the box's top
/// layer, so the sky band above a clamped box is reached.
const SKY: u32 = 2;
/// Ids per block (the shader's `BLOCK`), and the workgroup of the passes over ids.
const WORKGROUP: u32 = 64;
/// Workgroups along x when there are more (the shader's `ROW`).
const ROW: u32 = 65535;
/// Changed records past which the whole table is written at once: their
/// updates would pass a megabyte, the staging belt's chunk (`upload::Uploads`).
const DIRTY_LIMIT: usize = (1 << 20) / Update::BYTES as usize;
/// The frustum test's unit: its terms are 256ths of a block.
const SCALE: f64 = 256.0;
/// f32's unit roundoff: one rounding of the walk's f32 frustum test is off by
/// at most this share of its result.
const UNIT: f64 = f32::EPSILON as f64 / 2.0;
/// The most a plane's terms may sum to in magnitude: below it no partial sum of
/// them, in any order, leaves i32.
const HEADROOM: f64 = (1 << 30) as f64;

/// Tests read the counts, draws and origins back.
const TEST_READ: wgpu::BufferUsages = if cfg!(test) {
    wgpu::BufferUsages::COPY_SRC
} else {
    wgpu::BufferUsages::empty()
};

/// What a frame's pass works from.
pub(super) struct FrameInputs<'a> {
    pub camera: DVec3,
    /// The level's lowest block y and its height in sections.
    pub min_y: i32,
    pub sections: i32,
    /// The render distance in columns, as the walk takes it.
    pub radius: i32,
    /// The frustum the walk culls with (a GPU frame has no water mirror).
    pub frustum: &'a Frustum,
    /// The cave-culling graph, while cave culling runs and has one
    /// (`Occlusion::reach`); `None`: every section counts as reached.
    pub reach: Option<Reach<'a>>,
}

/// What `GpuCulling::prepare` made of a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Prepared {
    /// The pass is ready: `dispatch` it, and `draw`.
    Gpu,
    /// The frame's frustum planes would leave the test's integers (`HEADROOM`),
    /// or its camera is not finite: the CPU draws it, and the records that
    /// changed are written by the next frame the GPU draws.
    Cpu,
    /// A buffer failed to grow.
    OutOfMemory,
}

/// The frame's `tables` before the regions, into `words`, and the parameters
/// the frustum test reads with them; `None` when the CPU draws the frame
/// (`Prepared::Cpu`): a sum of a plane's terms could leave i32 (`HEADROOM`), or
/// the camera is not finite.
///
/// The words: the origins' f32 bits along x (the columns within the render
/// distance), y (the level's sections) and z, the walk's own
/// `resident::origin(..).as_vec3()`; per plane, its terms for those origins
/// along x, y and z; per plane and axis, its terms for the 32 box codes. A term
/// is the plane's component times an origin or a box's end, in 256ths of a
/// block, rounded up: the product of two f32 values is exact in f64, so a
/// plane's terms for a section sum to at least 256 times its exact value at the
/// corner of the box `Frustum::intersects` looks at. Its w carries a margin over
/// what the walk's f32 test can round away, at most 6u |p| V + 2u |w| (u is
/// `UNIT`, V the largest corner along each axis): the corner `fl(o + b)`, the
/// products and sums of `intersects`, and the column top `fl(o + 16)` of
/// `contains`, each off by at most u of its size. So the GPU keeps every box the
/// walk keeps, and every box of a column the walk finds entirely inside the
/// frustum, as each box reaches its cell (`rounded_box`). A plane that is not
/// finite rejects nothing, as in `intersects`: its terms are 0, its w 1.
fn tables(input: &FrameInputs<'_>, words: &mut Vec<u32>) -> Option<Params> {
    words.clear();
    // Before the origins: an infinite camera's column would take
    // `resident::origin` past i32.
    if !input.camera.is_finite() {
        return None;
    }
    let radius = input.radius.max(0);
    let side = 2 * radius + 1;
    let height = input.sections.max(0);
    let (cx, cz) = resident::camera_column(input.camera);
    let (x0, z0) = (cx - radius, cz - radius);
    let origin = |key| resident::origin(key, input.min_y, input.camera).as_vec3();
    words.extend((x0..x0 + side).map(|x| origin((x, 0, 0)).x.to_bits()));
    words.extend((0..height).map(|y| origin((0, y, 0)).y.to_bits()));
    words.extend((z0..z0 + side).map(|z| origin((0, 0, z)).z.to_bits()));
    let (nx, ny) = (side as usize, height as usize);
    let axes = [0..nx, nx..nx + ny, nx + ny..words.len()];
    // Along each axis, the largest origin plus 24: more than any corner of a box
    // (its ends reach 23) or of a column's box.
    let reach = axes.clone().map(|axis| {
        let sizes = words[axis]
            .iter()
            .map(|&o| f64::from(f32::from_bits(o)).abs());
        sizes.fold(0.0, f64::max) + 24.0
    });
    let planes = input.frustum.planes.map(|p| {
        if p.is_finite() {
            p.as_dvec4()
        } else {
            DVec4::ZERO
        }
    });
    // Each plane's largest terms in magnitude, along each axis, which bound its
    // sums.
    let mut largest = [0.0; 6];
    for (j, p) in planes.iter().enumerate() {
        for (axis, origins) in axes.iter().enumerate() {
            let mut most: f64 = 0.0;
            for i in origins.clone() {
                let term = (SCALE * (p[axis] * f64::from(f32::from_bits(words[i])))).ceil();
                most = most.max(term.abs());
                words.push(term as i32 as u32);
            }
            largest[j] += most;
        }
    }
    let mut signs: u32 = 0;
    for (j, p) in planes.iter().enumerate() {
        for axis in 0..3 {
            // `intersects` looks at a box's high end where the plane's component
            // is at least 0 (alike in f64).
            signs |= u32::from(p[axis] >= 0.0) << (3 * j + axis);
            let mut most: f64 = 0.0;
            for code in 0..32 {
                let term = (SCALE * p[axis] * f64::from(code - CODE_OFFSET)).ceil();
                most = most.max(term.abs());
                words.push(term as i32 as u32);
            }
            largest[j] += most;
        }
    }
    let w = planes.map(|p| {
        let rounding: f64 = (0..3).map(|axis| p[axis].abs() * reach[axis]).sum();
        let margin = (SCALE * (6.0 * UNIT * rounding + 2.0 * UNIT * p.w.abs())).ceil() + 1.0;
        (SCALE * p.w).ceil() + margin
    });
    let fits = (0..6).all(|j| largest[j] + w[j].abs() <= HEADROOM);
    if !fits {
        return None;
    }
    let w = w.map(|w| w as i32);
    Some(Params {
        signs,
        near: radius,
        cam_x: cx,
        cam_z: cz,
        first: [x0, 0, z0, 0],
        cells: [side as u32, height as u32, side as u32, 0],
        w: [[w[0], w[1], w[2], w[3]], [w[4], w[5], 0, 0]],
        ..Default::default()
    })
}

/// Whether GPU culling is offered: where the device draws a count the GPU wrote
/// and runs compute shaders, where its multi-draws offset instance-rate origins
/// by each draw's first instance (`multi_draw`; Intel Gen11/Gen12's Vulkan
/// driver gets that wrong, `flat::is_broken_intel_vulkan`, tested here too for
/// when flat draws are off), where Translucent Batching is
/// (`translucent_batchable`), and where vanilla frames never draw flat (flat
/// draws off, or for a shader pack's terrain alone): flat draws copy the CPU's
/// lists.
pub(super) fn supported(
    info: &wgpu::AdapterInfo,
    features: wgpu::Features,
    downlevel: wgpu::DownlevelFlags,
    limits: &wgpu::Limits,
    multi_draw: bool,
    flat_min_sections: Option<usize>,
) -> bool {
    super::translucent_batchable(info, multi_draw)
        && flat_min_sections.is_none_or(|min| min == usize::MAX)
        && features.contains(wgpu::Features::MULTI_DRAW_INDIRECT_COUNT)
        && downlevel.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
        // The pass binds 8, the last cave culling's graph.
        && limits.max_storage_buffers_per_shader_stage >= 8
        && !super::flat::is_broken_intel_vulkan(info)
}

/// A buffer that is recreated larger when it must hold more.
struct Grown {
    buffer: wgpu::Buffer,
    bytes: u64,
    label: &'static str,
    usage: wgpu::BufferUsages,
}

impl Grown {
    fn new(
        device: &wgpu::Device,
        label: &'static str,
        bytes: u64,
        usage: wgpu::BufferUsages,
    ) -> Option<Self> {
        Some(Self {
            buffer: try_create(device, label, bytes, usage)?,
            bytes,
            label,
            usage,
        })
    }
    /// Room for `bytes`, in a power of two within the device's limits:
    /// `Some(true)` when the buffer was made anew (its contents lost), `None`
    /// past the limits or when the memory for it is missing.
    fn reserve(&mut self, device: &wgpu::Device, bytes: u64) -> Option<bool> {
        if bytes <= self.bytes {
            return Some(false);
        }
        let limits = device.limits();
        let mut limit = limits.max_buffer_size;
        if self.usage.contains(wgpu::BufferUsages::STORAGE) {
            limit = limit.min(limits.max_storage_buffer_binding_size);
        }
        if bytes > limit {
            return None;
        }
        let size = bytes.next_power_of_two().min(limit & !3);
        self.buffer = try_create(device, self.label, size, self.usage)?;
        self.bytes = size;
        Some(true)
    }
}

/// The section table as the CPU keeps it, by id, and what changed since
/// `GpuCulling::prepare` last wrote it.
#[derive(Default)]
pub(super) struct Records {
    records: Vec<Record>,
    /// Free ids below the table's end, a bit each (64 a word, lowest bit first).
    free: Vec<u64>,
    /// No free id is below `64 *` this.
    first_free: usize,
    /// Ids changed since, unless there were too many to list (`all_dirty`).
    dirty: Vec<u32>,
    all_dirty: bool,
    /// Per arena page, the records on it.
    page_sections: Vec<u32>,
    /// The records flagged `FADING`.
    fading: u32,
}

impl Records {
    /// A resident section with solid quads, `record` (`SectionGpu::record`):
    /// its id, the lowest free one or a new one.
    pub fn insert(&mut self, record: Record) -> u32 {
        debug_assert!(record.page().is_some(), "a record names its page");
        let id = match self.take_free() {
            Some(id) => id,
            None => {
                self.records.push(Record::FREE);
                self.records.len() as u32 - 1
            }
        };
        self.records[id as usize] = record;
        self.add_to(record.key[3] as u32);
        self.fading += u32::from(record.is_fading());
        self.mark(id);
        id
    }
    /// The section with `id` went.
    pub fn remove(&mut self, id: u32) {
        self.remove_all([id]);
    }
    /// The sections with `ids` went, a bit set for each (a teleport frees tens of
    /// thousands at once, `Renderer::remove_columns_at_once`). Each is marked, so
    /// the GPU has it free before the pass next culls: the pass keeps any record
    /// in view, and a stale one would draw quads that are gone or another
    /// section's.
    pub fn remove_all(&mut self, ids: impl IntoIterator<Item = u32>) {
        for id in ids {
            let page = self.records.get(id as usize).and_then(Record::page);
            debug_assert!(page.is_some(), "no record {id} to remove");
            let Some(page) = page else {
                continue;
            };
            self.page_sections[page as usize] -= 1;
            self.fading -= u32::from(self.records[id as usize].is_fading());
            self.records[id as usize] = Record::FREE;
            // shortcut: a teleport frees more than `DIRTY_LIMIT`, so `prepare` writes the
            // whole table (0.6-2.6 ms at render distance 64); upgrade to a list of freed ids
            // the pass reads, a word each, if those frames need it.
            self.mark(id);
            let word = id as usize / 64;
            if self.free.len() <= word {
                self.free.resize(word + 1, 0);
            }
            self.free[word] |= 1 << (id % 64);
            self.first_free = self.first_free.min(word);
        }
        // Free ids at the end leave the table, so the pass tests fewer.
        while self.records.last().is_some_and(|r| r.page().is_none()) {
            self.records.pop();
            let id = self.records.len();
            self.free[id / 64] &= !(1 << (id % 64));
        }
        self.free.truncate(self.records.len().div_ceil(64));
        self.first_free = self.first_free.min(self.free.len());
    }
    /// The lowest free id, taken.
    fn take_free(&mut self) -> Option<u32> {
        while let Some(&word) = self.free.get(self.first_free) {
            if word != 0 {
                self.free[self.first_free] = word & (word - 1);
                return Some((self.first_free * 64) as u32 + word.trailing_zeros());
            }
            self.first_free += 1;
        }
        None
    }
    /// The section with `id` moved to vertex `start` of page `page`, its quads
    /// with it (`move_off_page`).
    pub fn relocate(&mut self, id: u32, page: u32, start: u64) {
        let from = self.records.get(id as usize).and_then(Record::page);
        debug_assert!(from.is_some(), "no record {id} to move");
        let Some(from) = from else {
            return;
        };
        let record = &mut self.records[id as usize];
        record.key[3] = page as i32;
        record.a[0] = start as u32;
        self.page_sections[from as usize] -= 1;
        self.add_to(page);
        self.mark(id);
    }
    /// The section with `id` began to draw faded, or stopped (gfx.rs
    /// `SectionGpu::gpu_fading`): its record is `FADING` when `on`.
    pub fn set_fading(&mut self, id: u32, on: bool) {
        let page = self.records.get(id as usize).and_then(Record::page);
        debug_assert!(page.is_some(), "no record {id} to flag");
        if page.is_none() || self.records[id as usize].is_fading() == on {
            return;
        }
        self.records[id as usize].a[3] ^= FADING;
        if on {
            self.fading += 1;
        } else {
            self.fading -= 1;
        }
        self.mark(id);
    }
    /// Every section went.
    pub fn clear(&mut self) {
        self.records.clear();
        self.free.clear();
        self.first_free = 0;
        self.dirty = Vec::new();
        self.all_dirty = false;
        self.page_sections.clear();
        self.fading = 0;
    }
    /// The sections with a record.
    pub fn live(&self) -> usize {
        let free: u32 = self.free.iter().map(|word| word.count_ones()).sum();
        self.records.len() - free as usize
    }
    /// The sections whose record is `FADING`, which the CPU draws.
    pub fn fading(&self) -> u32 {
        self.fading
    }
    /// The record of the section with `id`; `None` for a free id.
    #[cfg(any(test, debug_assertions))]
    pub fn at(&self, id: u32) -> Option<&Record> {
        self.records.get(id as usize).filter(|r| r.page().is_some())
    }
    #[cfg(any(test, debug_assertions))]
    pub fn page_sections(&self) -> &[u32] {
        &self.page_sections
    }
    /// The table's end: the ids the pass tests.
    fn ids(&self) -> u32 {
        self.records.len() as u32
    }
    fn add_to(&mut self, page: u32) {
        let page = page as usize;
        if self.page_sections.len() <= page {
            self.page_sections.resize(page + 1, 0);
        }
        self.page_sections[page] += 1;
    }
    fn mark(&mut self, id: u32) {
        if self.all_dirty {
            return;
        }
        if self.dirty.len() >= DIRTY_LIMIT {
            self.all_dirty = true;
            self.dirty = Vec::new();
        } else {
            self.dirty.push(id);
        }
    }
}

/// The flagged sections by when their fade is over at the latest (gfx.rs
/// `SectionGpu::fade_end`), for gfx.rs `mark_fades` to look at them again
/// then, and the chunk fade-in time their flags were decided with: `None`
/// until a frame has decided them, just after GPU culling was switched on.
/// The ends of sections that went or were handed back are skipped as they come.
#[derive(Default)]
pub(super) struct Fades {
    due: BinaryHeap<Reverse<(Instant, SectionKey)>>,
    pub fade_ms: Option<u64>,
}

impl Fades {
    /// The section at `key` is looked at again at `end`.
    pub fn push(&mut self, end: Instant, key: SectionKey) {
        self.due.push(Reverse((end, key)));
    }
    /// The section of the earliest end at or before `now`, taken out.
    pub fn pop_due(&mut self, now: Instant) -> Option<SectionKey> {
        let &Reverse((end, key)) = self.due.peek()?;
        if end > now {
            return None;
        }
        self.due.pop();
        Some(key)
    }
    /// Every end goes; `fade_ms` stays.
    pub fn clear(&mut self) {
        self.due.clear();
    }
    /// The sections an end is waited for.
    #[cfg(debug_assertions)]
    pub fn pending(&self) -> impl Iterator<Item = SectionKey> {
        self.due.iter().map(|r| r.0.1)
    }
}

/// The pass, its buffers and the records it draws from: made when GPU culling
/// is switched on, dropped when it is switched off.
pub(super) struct GpuCulling {
    /// `gpu_culling.wgsl`'s entry points: apply, cull, scan, scatter.
    pipelines: [wgpu::ComputePipeline; 4],
    layout: wgpu::BindGroupLayout,
    params: wgpu::Buffer,
    sections: Grown,
    tables: Grown,
    updates: Grown,
    counts: Grown,
    draws: Grown,
    origins: Grown,
    runs: Grown,
    /// Cave culling's graph, its `Reach::bits`.
    reach: Grown,
    /// The generation `reach` holds (`Reach::generation`); `None` until it is
    /// written, or after it was made anew.
    reach_generation: Option<u64>,
    /// Dropped as soon as one of its buffers is made anew.
    group: Option<wgpu::BindGroup>,
    /// What `sections` holds once `prepare` writes it; a field of its own, so
    /// that the arena's compaction moves records while it borrows the
    /// renderer's other fields.
    pub records: Records,
    /// When the flagged records' fades end, a field of its own as `records`
    /// is: gfx.rs `mark_fades` takes both beside the renderer's sections.
    pub fades: Fades,
    /// This frame's pages with records: (page, first draw of its region, the
    /// region's size).
    pages: Vec<(u32, u32, u32)>,
    /// This frame's ids to test, records to `apply`, and pages the counts
    /// cover (at least one).
    ids: u32,
    n_updates: u32,
    pass_pages: u32,
    /// This frame's updates and table words, kept for their room.
    scratch: Vec<Update>,
    words: Vec<u32>,
}

impl GpuCulling {
    /// `None` when the device fails to make the pipelines or the memory for the
    /// buffers is missing (caught, not raised: raised errors are fatal).
    pub fn new(device: &wgpu::Device) -> Option<Self> {
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let storage =
            |binding, read_only| entry(binding, wgpu::BufferBindingType::Storage { read_only });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("GPU culling"),
            entries: &[
                entry(0, wgpu::BufferBindingType::Uniform),
                storage(1, false),
                storage(2, true),
                storage(3, true),
                storage(4, false),
                storage(5, false),
                storage(6, false),
                storage(7, false),
                storage(8, true),
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("GPU culling"),
            source: wgpu::ShaderSource::Wgsl(include_str!("gpu_culling.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("GPU culling"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipelines = ["apply", "cull", "scan", "scatter"].map(|entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("GPU culling"),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        });
        let internal = pollster::block_on(internal.pop());
        if let Some(e) = internal.or(pollster::block_on(validation.pop())) {
            log::warn!("Horizon renderer: GPU culling unavailable ({e})");
            return None;
        }
        use wgpu::BufferUsages as U;
        Some(Self {
            pipelines,
            layout,
            params: try_create(
                device,
                "GPU culling params",
                std::mem::size_of::<Params>() as u64,
                U::UNIFORM | U::COPY_DST,
            )?,
            sections: Grown::new(
                device,
                "GPU culling sections",
                1024 * Record::BYTES,
                U::STORAGE | U::COPY_DST,
            )?,
            tables: Grown::new(
                device,
                "GPU culling tables",
                1 << 14,
                U::STORAGE | U::COPY_DST,
            )?,
            updates: Grown::new(
                device,
                "GPU culling updates",
                1024 * Update::BYTES,
                U::STORAGE | U::COPY_DST,
            )?,
            counts: Grown::new(
                device,
                "GPU culling counts",
                (STATS + 64) * 4,
                U::STORAGE | U::INDIRECT | U::COPY_DST | TEST_READ,
            )?,
            draws: Grown::new(
                device,
                "GPU culling draws",
                4096 * 20,
                U::STORAGE | U::INDIRECT | TEST_READ,
            )?,
            origins: Grown::new(
                device,
                "GPU culling origins",
                1024 * 16,
                U::VERTEX | U::STORAGE | TEST_READ,
            )?,
            runs: Grown::new(device, "GPU culling runs", 1024 * 4, U::STORAGE)?,
            // Room for render distance 64's graph, 57.5 KB.
            reach: Grown::new(
                device,
                "GPU culling reach",
                1 << 16,
                U::STORAGE | U::COPY_DST,
            )?,
            reach_generation: None,
            group: None,
            records: Records::default(),
            fades: Fades::default(),
            pages: Vec::new(),
            ids: 0,
            n_updates: 0,
            pass_pages: 1,
            scratch: Vec::new(),
            words: Vec::new(),
        })
    }

    /// Records the frame's uploads into `encoder` and clears its counts, before
    /// the walk: the records that changed (through the queue when the whole
    /// table goes), the origins along each axis and the frustum planes' terms
    /// (`tables`), the pages' regions, cave culling's graph when its generation
    /// moved, and the parameters. Then `dispatch` and the world pass's `draw`.
    /// Unless the frame is `Prepared::Gpu`, nothing is written, and nothing of
    /// the frame is dispatched or drawn.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &Queue,
        uploads: &mut Uploads,
        encoder: &mut wgpu::CommandEncoder,
        input: &FrameInputs<'_>,
    ) -> Prepared {
        self.pages.clear();
        self.ids = 0;
        self.n_updates = 0;
        self.prepare_buffers(device, queue, uploads, encoder, input)
            .unwrap_or(Prepared::OutOfMemory)
    }

    fn prepare_buffers(
        &mut self,
        device: &wgpu::Device,
        queue: &Queue,
        uploads: &mut Uploads,
        encoder: &mut wgpu::CommandEncoder,
        input: &FrameInputs<'_>,
    ) -> Option<Prepared> {
        // Every buffer grows before anything is written. One made anew drops
        // the bind group at once, and a new section table needs every record:
        // a failure later, or a frame the CPU draws, keeps both for the next.
        let ids = self.records.ids();
        if self
            .sections
            .reserve(device, u64::from(ids) * Record::BYTES)?
        {
            self.group = None;
            self.records.all_dirty = true;
        }
        // The changed records, each once; ids past the end left with the
        // records they held.
        self.scratch.clear();
        if !self.records.all_dirty {
            let Records { records, dirty, .. } = &mut self.records;
            dirty.sort_unstable();
            dirty.dedup();
            let changed = dirty.iter().filter(|&&id| id < ids);
            self.scratch.extend(changed.map(|&id| Update {
                id: [id, 0, 0, 0],
                record: records[id as usize],
            }));
        }
        // The table words: the origins and the frustum planes' terms (`tables`),
        // then each page's region.
        let planes = tables(input, &mut self.words);
        let regions = self.words.len() as u32;
        let pass_pages = self.records.page_sections.len().max(1) as u32;
        // A region has room for a draw of every record on its page.
        let total: u32 = self.records.page_sections.iter().sum();
        // The statistics, each page's count, then each block's per page.
        let count_words = STATS + u64::from(pass_pages) * (1 + u64::from(ids.div_ceil(WORKGROUP)));
        for (buffer, bytes) in [
            (&mut self.updates, self.scratch.len() as u64 * Update::BYTES),
            (&mut self.origins, u64::from(ids) * 16),
            (&mut self.runs, u64::from(ids) * 4),
            (&mut self.tables, u64::from(regions + 2 * pass_pages) * 4),
            (&mut self.counts, count_words * 4),
            (&mut self.draws, u64::from(total.max(1)) * 20),
        ] {
            if buffer.reserve(device, bytes)? {
                self.group = None;
            }
        }
        // A copy of the graph made anew holds none.
        if let Some(r) = &input.reach
            && self.reach.reserve(device, r.bits.len() as u64 * 4)?
        {
            self.group = None;
            self.reach_generation = None;
        }
        // The planes past what the test's integers hold: the CPU draws the
        // frame, and the records that changed wait for the next.
        let Some(params) = planes else {
            return Some(Prepared::Cpu);
        };
        if self.records.all_dirty {
            // The whole table, after it grew or more records changed than
            // updates take (GPU culling switched on in a loaded world, a
            // teleport). Through the queue, which waits for the frame on the
            // submit thread once, rather than the belt, which would keep a
            // staging buffer this large for good.
            if ids > 0 {
                queue.write_buffer(
                    &self.sections.buffer,
                    0,
                    bytemuck::cast_slice(&self.records.records),
                );
            }
        } else {
            uploads.buffer(
                encoder,
                &self.updates.buffer,
                0,
                bytemuck::cast_slice(&self.scratch),
            );
        }
        self.records.dirty.clear();
        self.records.all_dirty = false;
        let (sizes, mut first) = (&self.records.page_sections, 0);
        for page in 0..pass_pages {
            let size = sizes.get(page as usize).copied().unwrap_or(0);
            self.words.extend([first, size]);
            if size > 0 {
                self.pages.push((page, first, size));
            }
            first += size;
        }
        uploads.buffer(
            encoder,
            &self.tables.buffer,
            0,
            bytemuck::cast_slice(&self.words),
        );
        let mut params = Params {
            ids,
            updates: self.scratch.len() as u32,
            pages: pass_pages,
            regions,
            ..params
        };
        if let Some(r) = &input.reach {
            if self.reach_generation != Some(r.generation) {
                // Through the belt: a search lands every few frames while the
                // camera moves or sections load, and a write through the queue
                // waits for the submit thread.
                uploads.buffer(encoder, &self.reach.buffer, 0, bytemuck::cast_slice(r.bits));
                self.reach_generation = Some(r.generation);
            }
            params.reach = [r.from_x, r.from_z, r.radius, r.y0];
            (params.reach_height, params.reach_sections) = (r.height, r.sections);
            params.flags |= REACH | if r.sky_reached { SKY } else { 0 };
        }
        uploads.buffer(encoder, &self.params, 0, bytemuck::bytes_of(&params));
        encoder.clear_buffer(&self.counts.buffer, 0, Some(count_words * 4));
        if self.group.is_none() {
            self.group = Some(self.bind(device));
        }
        self.ids = ids;
        self.n_updates = params.updates;
        self.pass_pages = pass_pages;
        Some(Prepared::Gpu)
    }

    fn bind(&self, device: &wgpu::Device) -> wgpu::BindGroup {
        fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry {
                binding,
                resource: buffer.as_entire_binding(),
            }
        }
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("GPU culling"),
            layout: &self.layout,
            entries: &[
                entry(0, &self.params),
                entry(1, &self.sections.buffer),
                entry(2, &self.tables.buffer),
                entry(3, &self.updates.buffer),
                entry(4, &self.counts.buffer),
                entry(5, &self.draws.buffer),
                entry(6, &self.origins.buffer),
                entry(7, &self.runs.buffer),
                entry(8, &self.reach.buffer),
            ],
        })
    }

    /// The pass, after `prepare`: the changed records into the table, then the
    /// draws of the sections it keeps.
    pub fn dispatch(&self, encoder: &mut wgpu::CommandEncoder, timing: Option<&mut PassTiming>) {
        let Some(group) = self.group.as_ref().filter(|_| self.ids > 0) else {
            return;
        };
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("GPU culling"),
            timestamp_writes: timing.and_then(|t| t.compute_writes("GPU culling")),
        });
        pass.set_bind_group(0, group, &[]);
        let [apply, cull, scan, scatter] = &self.pipelines;
        if self.n_updates > 0 {
            pass.set_pipeline(apply);
            pass.dispatch_workgroups(self.n_updates.div_ceil(WORKGROUP), 1, 1);
        }
        let blocks = self.ids.div_ceil(WORKGROUP);
        pass.set_pipeline(cull);
        pass.dispatch_workgroups(blocks.min(ROW), blocks.div_ceil(ROW), 1);
        pass.set_pipeline(scan);
        pass.dispatch_workgroups(self.pass_pages, 1, 1);
        pass.set_pipeline(scatter);
        pass.dispatch_workgroups(blocks.min(ROW), blocks.div_ceil(ROW), 1);
    }

    /// The kept sections' solid draws, a count draw per page, with the quad
    /// indices bound. Leaves the origins at vertex slot 1. `group`: the page's group 1 for
    /// pipelines that read the page themselves (`Flat::page_group`), else none.
    pub fn draw<'a>(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        arena: &Arena,
        group: impl Fn(u32) -> Option<&'a wgpu::BindGroup>,
    ) {
        if self.pages.is_empty() {
            return;
        }
        pass.set_vertex_buffer(1, self.origins.buffer.slice(..));
        for &(page, first, size) in &self.pages {
            pass.set_vertex_buffer(0, arena.buffer(page).slice(..));
            if let Some(group) = group(page) {
                pass.set_bind_group(1, group, &[]);
            }
            pass.multi_draw_indexed_indirect_count(
                &self.draws.buffer,
                u64::from(first) * 20,
                &self.counts.buffer,
                (STATS + u64::from(page)) * 4,
                size,
            );
        }
    }

    /// Draw calls `draw` makes.
    pub fn draw_calls(&self) -> usize {
        self.pages.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streaming::in_view_distance;
    use glam::{Mat4, Vec4};
    use std::collections::BTreeMap;

    /// xorshift64*.
    struct Rng(u64);
    impl Rng {
        fn bits(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
        fn below(&mut self, n: u32) -> u32 {
            (self.bits() >> 33) as u32 % n
        }
        fn range(&mut self, low: i32, high: i32) -> i32 {
            low + self.below((high - low + 1) as u32) as i32
        }
        fn unit(&mut self) -> f32 {
            (self.bits() >> 40) as f32 / (1u64 << 24) as f32
        }
    }

    /// Where a section's solid quads are: key, arena page, first vertex, quads,
    /// its box, and whether it fades (`FADING`).
    type Placed = (SectionKey, u32, u64, u32, u32, bool);

    /// `gpu_culling.wgsl`'s `near`, on the CPU.
    fn near(params: &Params, x: i32, z: i32) -> bool {
        let r = params.near;
        let gap = |d: i32| d.wrapping_abs().wrapping_sub(1).max(0);
        let gx = gap(x.wrapping_sub(params.cam_x));
        let gz = gap(z.wrapping_sub(params.cam_z));
        if gx >= r || gz >= r {
            return false;
        }
        gx.wrapping_mul(gx).wrapping_add(gz.wrapping_mul(gz)) < r.wrapping_mul(r)
    }

    /// `gpu_culling.wgsl`'s `in_frustum`, on the CPU and in its order: the box
    /// `b` of the section in cell `c` of the tables.
    fn in_frustum(params: &Params, words: &[u32], c: [u32; 3], b: u32) -> bool {
        if (b & UNBOUNDED) != 0 {
            return true;
        }
        let [nx, ny, nz, _] = params.cells;
        let n = nx + ny + nz;
        let term = |at: u32| words[at as usize] as i32;
        (0..6u32).all(|j| {
            let s = params.signs >> (3 * j);
            // The box's code at the corner the plane looks at, along `axis`.
            let code = |axis: u32| {
                let high = ((s >> axis) & 1) != 0;
                (b >> (5 * axis + if high { 15 } else { 0 })) & 31
            };
            let w = params.w[j as usize / 4][j as usize % 4];
            let (origin_terms, code_terms) = (n + j * n, 7 * n + 96 * j);
            let sum = w
                .wrapping_add(term(origin_terms + c[0]))
                .wrapping_add(term(origin_terms + nx + c[1]))
                .wrapping_add(term(origin_terms + nx + ny + c[2]))
                .wrapping_add(term(code_terms + code(0)))
                .wrapping_add(term(code_terms + 32 + code(1)))
                .wrapping_add(term(code_terms + 64 + code(2)));
            sum >= 0
        })
    }

    /// `gpu_culling.wgsl`'s `cull` on the CPU, the reference the pass is held
    /// to, but for cave culling's graph (its `reached` is `Reach::contains`):
    /// whether it draws `record` with the parameters and tables `tables` makes
    /// (and the pages `prepare` gives them).
    fn keeps(params: &Params, words: &[u32], record: &Record) -> bool {
        let (key, a) = (record.key, record.a);
        if key[3] < 0 || a[1] == 0 || (a[3] & FADING) != 0 || !near(params, key[0], key[2]) {
            return false;
        }
        let c: [u32; 3] = std::array::from_fn(|i| key[i].wrapping_sub(params.first[i]) as u32);
        (key[3] as u32) < params.pages
            && (0..3).all(|i| c[i] < params.cells[i])
            && in_frustum(params, words, c, a[2])
    }

    /// A frame of the overworld's 24 sections, cave culling off.
    fn inputs(camera: DVec3, radius: i32, frustum: &Frustum) -> FrameInputs<'_> {
        FrameInputs {
            camera,
            min_y: crate::chunk::MIN_Y,
            sections: 24,
            radius,
            frustum,
            reach: None,
        }
    }

    /// A frustum as `draw_frame` builds one, looking `yaw` and `pitch` out to
    /// `far`: a field of view and aspect at random, and a little view bobbing.
    fn looking(rng: &mut Rng, yaw: f32, pitch: f32, far: f32) -> Frustum {
        let fov = 30.0 + rng.unit() * 80.0;
        let aspect = 1.0 + rng.unit() * 1.4;
        let bob = Vec3::new(rng.unit() - 0.5, rng.unit() - 0.5, 0.0) * 0.2;
        let lens = super::super::world_projection(fov, aspect, far) * Mat4::from_translation(bob);
        Frustum {
            planes: super::super::frustum(lens * crate::camera::view_matrix(yaw, pitch)),
        }
    }

    /// `looking` anywhere, straight up and down among them, out to 64 to 8,192.
    fn view(rng: &mut Rng) -> Frustum {
        let yaw = rng.unit() * 360.0;
        let pitch = match rng.below(8) {
            0 => 90.0,
            1 => -90.0,
            _ => rng.unit() * 180.0 - 90.0,
        };
        let far = 64.0 + rng.unit() * 8128.0;
        looking(rng, yaw, pitch, far)
    }

    /// Bounds as `SectionMesh::compute_bounds` makes them, the quads' corners
    /// padded by `BOUNDS_PAD`, of every kind along each axis: inside the cell,
    /// overhanging it, a rotated model's reach, wholly outside it, flat; none
    /// (the cell); and now and then past what a box holds.
    fn bounds(rng: &mut Rng) -> Option<[Vec3; 2]> {
        if rng.below(10) == 0 {
            return None;
        }
        let pad = crate::mesher::SectionMesh::BOUNDS_PAD;
        let sixteenths = |rng: &mut Rng| rng.below(257) as f32 / 16.0;
        let (mut min, mut max) = (Vec3::ZERO, Vec3::ZERO);
        for axis in 0..3 {
            let (low, high) = match rng.below(12) {
                0 => (-1.0, 17.0),
                1 => (-5.5, 21.5),
                2 => (18.5, 19.5),
                3 => (-4.5, -3.5),
                4 => (0.0, 0.0),
                5 if rng.below(4) == 0 => (-9.0, 4.0),
                _ => {
                    let (a, b) = (sixteenths(rng), sixteenths(rng));
                    (a.min(b), a.max(b))
                }
            };
            (min[axis], max[axis]) = (low - pad, high + pad);
        }
        Some([min, max])
    }

    /// Ids, page counts, flags and changes against a plain list of what each id
    /// holds: the lowest free id comes back, flagged only if inserted so, free
    /// ids at the end leave, counts follow moves (to pages past the last too), a
    /// move keeps the box and the flag, a free is a change, a flag set changes
    /// its record only when it differs, the flagged ones are counted through
    /// inserts, flags, removals and clears, and more changes than updates take
    /// mark the whole table.
    #[test]
    fn records_keep_ids_page_counts_and_moves() {
        let mut records = Records::default();
        let a = records.insert(Record::new((0, 0, 0), 0, 0, 1, 7));
        let b = records.insert(Record::new((1, 0, 0), 0, 256, 2, 9));
        records.relocate(b, 5, 512);
        assert_eq!(records.page_sections(), [1, 0, 0, 0, 0, 1]);
        assert_eq!(records.at(b), Some(&Record::new((1, 0, 0), 5, 512, 2, 9)));
        records.remove(a);
        assert_eq!((records.ids(), records.live(), records.at(a)), (2, 1, None));
        assert_eq!(records.insert(Record::new((2, 0, 0), 1, 0, 3, 11)), a);
        records.remove(b);
        assert_eq!(
            (records.ids(), records.page_sections()),
            (1, &[0, 1, 0, 0, 0, 0][..])
        );

        // The free ids by their bits, which must be exactly the free records below the
        // table's end, none of them below `first_free`.
        let free_ids = |r: &Records| -> Vec<usize> {
            let set = |id: usize| {
                r.free
                    .get(id / 64)
                    .is_some_and(|&w| (w >> (id % 64)) & 1 == 1)
            };
            let span = r.records.len().max(r.free.len() * 64);
            let free = |id: usize| r.records.get(id).is_some_and(|x| x.page().is_none());
            assert!((0..span).all(|id| set(id) == free(id)), "free bits");
            assert!(
                r.free[..r.first_free.min(r.free.len())]
                    .iter()
                    .all(|&w| w == 0)
            );
            (0..span).filter(|&id| set(id)).collect()
        };
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        // What each id holds; `None`: a free id.
        let mut model: Vec<Option<Placed>> = vec![Some(((2, 0, 0), 1, 0, 3, 11, false))];
        for step in 0..4_000 {
            let live: Vec<u32> = (0..model.len() as u32)
                .filter(|&id| model[id as usize].is_some())
                .collect();
            let (page, start) = (
                rng.below(4 + step / 500),
                u64::from(rng.below(1 << 20)) * 256,
            );
            let dirty = records.dirty.len();
            match rng.below(6) {
                op if op < 2 || live.is_empty() => {
                    let key = (rng.range(-9, 9), rng.range(0, 23), rng.range(-9, 9));
                    let (quads, boxed) = (1 + rng.below(500), rng.below(1 << 30));
                    let fading = rng.below(4) == 0;
                    let lowest = model
                        .iter()
                        .position(Option::is_none)
                        .unwrap_or(model.len());
                    let record = Record::new(key, page, start, quads, boxed).fading(fading);
                    assert_eq!(records.insert(record) as usize, lowest);
                    if lowest == model.len() {
                        model.push(None);
                    }
                    model[lowest] = Some((key, page, start, quads, boxed, fading));
                }
                op => {
                    let id = live[rng.below(live.len() as u32) as usize];
                    let held = model[id as usize].as_mut().expect("a live id");
                    if op < 4 {
                        records.relocate(id, page, start);
                        (held.1, held.2) = (page, start);
                    } else if op == 4 {
                        let on = rng.below(2) == 0;
                        records.set_fading(id, on);
                        let changed = usize::from(held.5 != on);
                        assert_eq!(records.dirty.len(), dirty + changed, "step {step}");
                        held.5 = on;
                    } else {
                        records.remove(id);
                        // Marked: the GPU must have it free before the pass culls.
                        assert_eq!(records.dirty.last(), Some(&id), "step {step}");
                        model[id as usize] = None;
                        while model.last() == Some(&None) {
                            model.pop();
                        }
                    }
                }
            }
            assert_eq!(records.ids() as usize, model.len(), "step {step}");
            assert_eq!(records.live(), model.iter().flatten().count());
            let flagged = model.iter().flatten().filter(|held| held.5).count();
            assert_eq!(records.fading() as usize, flagged, "step {step}");
            let mut pages = vec![0; records.page_sections().len()];
            for (id, held) in model.iter().enumerate() {
                let record = held.map(|(key, page, start, quads, boxed, fading)| {
                    pages[page as usize] += 1;
                    Record::new(key, page, start, quads, boxed).fading(fading)
                });
                assert_eq!(
                    records.at(id as u32),
                    record.as_ref(),
                    "step {step}, id {id}"
                );
            }
            assert_eq!(records.page_sections(), pages, "step {step}");
            free_ids(&records);
        }

        // A few, then many at once, as a teleport drops them (onto free ids, past one
        // at the table's new end): the table removing each leaves.
        let (mut each, mut all) = (Records::default(), Records::default());
        for n in 0..1_000 {
            let (key, page, start) = ((n % 31, n % 24, n / 31), n as u32 % 3, n as u64 * 256);
            let record = Record::new(key, page, start, 1, 0).fading(n % 7 == 0);
            assert_eq!(each.insert(record), all.insert(record));
        }
        let gone: Vec<u32> = (0..1_000).filter(|id| id % 3 != 1 || *id >= 900).collect();
        let counts = |r: &Records| (r.ids(), r.live(), r.fading());
        for ids in [&[1u32, 4, 898][..], &gone[..]] {
            for &id in ids {
                each.remove(id);
            }
            all.remove_all(ids.iter().copied());
            assert_eq!(counts(&all), counts(&each));
            assert_eq!(all.page_sections(), each.page_sections());
            assert_eq!(free_ids(&all), free_ids(&each));
            assert_eq!(all.dirty, each.dirty);
            assert!((0..each.ids()).all(|id| all.at(id) == each.at(id)));
        }
        // The lowest free id comes back first, then the next (0 left with `gone`, 1 before).
        assert_eq!((all.take_free(), all.take_free()), (Some(0), Some(1)));
        assert_eq!(all.ids(), 896);
        // All of them: an empty table.
        let live: Vec<u32> = (0..all.ids()).filter(|&id| all.at(id).is_some()).collect();
        all.remove_all(live);
        assert_eq!((all.ids(), all.live(), all.free.len()), (0, 0, 0));
        assert!(all.page_sections().iter().all(|&n| n == 0));

        assert!(records.fading() > 0);
        records.clear();
        assert_eq!((records.ids(), records.live(), records.fading()), (0, 0, 0));
        assert!(records.page_sections().is_empty() && records.dirty.is_empty());
        for x in 0..DIRTY_LIMIT as i32 {
            records.insert(Record::new((x, 0, 0), 0, 0, 1, 0));
        }
        assert!(!records.all_dirty && records.dirty.len() == DIRTY_LIMIT);
        records.relocate(0, 1, 256);
        assert!(records.all_dirty && records.dirty.is_empty());
        records.clear();
        assert!(!records.all_dirty);
        assert_eq!(records.insert(Record::new((0, 0, 0), 0, 0, 1, 0)), 0);
    }

    /// A box holds the bounds it is made from, rounded out to whole blocks, and
    /// reaches its cell along every axis; bounds past what it holds, or not
    /// finite, are `UNBOUNDED`; no bounds are the cell.
    #[test]
    fn boxes_round_outward_and_meet_their_cell() {
        // Each axis's low and high ends.
        let ends = |packed: u32| -> [[i32; 2]; 3] {
            std::array::from_fn(|axis| {
                let code = |shift: usize| ((packed >> (shift + 5 * axis)) & 31) as i32;
                [code(0) - CODE_OFFSET, code(15) - CODE_OFFSET]
            })
        };
        assert_eq!(ends(rounded_box(None)), [[0, 16]; 3]);
        for (low, high, rounded) in [
            (-1.5, 17.5, Some([-2, 18])),
            (-6.0, 22.0, Some([-6, 22])),
            (18.0, 20.0, Some([16, 20])),
            (-5.0, -3.0, Some([-5, 0])),
            (-0.5, 0.5, Some([-1, 1])),
            (-8.0, 23.0, Some([-8, 23])),
            (-8.5, 4.0, None),
            (3.0, 23.25, None),
            (f32::NAN, 4.0, None),
            (0.0, f32::INFINITY, None),
            (f32::NEG_INFINITY, 0.0, None),
        ] {
            for axis in 0..3 {
                let mut bounds = [Vec3::new(1.0, 2.0, 3.0), Vec3::new(14.0, 15.0, 16.0)];
                (bounds[0][axis], bounds[1][axis]) = (low, high);
                let packed = rounded_box(Some(bounds));
                match rounded {
                    Some(rounded) => assert_eq!(ends(packed)[axis], rounded, "{low}..{high}"),
                    None => assert_eq!(packed, UNBOUNDED, "{low}..{high}"),
                }
            }
        }
    }

    /// `Params`, `Section` and `Update` as the shader lays them out, and the
    /// constants both sides use.
    #[test]
    fn params_and_records_match_the_shader() {
        let module = naga::front::wgsl::parse_str(include_str!("gpu_culling.wgsl")).unwrap();
        let mut layouter = naga::proc::Layouter::default();
        layouter.update(module.to_ctx()).unwrap();
        // A struct's size and its members' offsets.
        let layout = |name: &str| {
            let (handle, ty) = module
                .types
                .iter()
                .find(|(_, t)| t.name.as_deref() == Some(name))
                .unwrap_or_else(|| panic!("{name}"));
            let naga::TypeInner::Struct { members, .. } = &ty.inner else {
                panic!("{name} is a struct");
            };
            let members = members
                .iter()
                .map(|m| (m.name.clone().unwrap(), m.offset as usize));
            (layouter[handle].size as usize, members.collect::<Vec<_>>())
        };
        use std::mem::{offset_of, size_of};
        let (size, members) = layout("Params");
        assert_eq!(size, size_of::<Params>());
        for (name, offset) in [
            ("ids", offset_of!(Params, ids)),
            ("updates", offset_of!(Params, updates)),
            ("pages", offset_of!(Params, pages)),
            ("regions", offset_of!(Params, regions)),
            ("signs", offset_of!(Params, signs)),
            ("near", offset_of!(Params, near)),
            ("cam_x", offset_of!(Params, cam_x)),
            ("cam_z", offset_of!(Params, cam_z)),
            ("first", offset_of!(Params, first)),
            ("cells", offset_of!(Params, cells)),
            ("w", offset_of!(Params, w)),
            ("reach", offset_of!(Params, reach)),
            ("reach_height", offset_of!(Params, reach_height)),
            ("reach_sections", offset_of!(Params, reach_sections)),
            ("flags", offset_of!(Params, flags)),
            ("pad", offset_of!(Params, pad)),
        ] {
            assert!(members.contains(&(name.into(), offset)), "Params.{name}");
        }
        let named = |pairs: [(&str, usize); 2]| pairs.map(|(n, o)| (n.to_owned(), o)).to_vec();
        let key = [
            ("key", offset_of!(Record, key)),
            ("a", offset_of!(Record, a)),
        ];
        assert_eq!(layout("Section"), (size_of::<Record>(), named(key)));
        let id = [
            ("id", offset_of!(Update, id)),
            ("section", offset_of!(Update, record)),
        ];
        assert_eq!(layout("Update"), (size_of::<Update>(), named(id)));
        assert_eq!((size_of::<Record>(), size_of::<Update>()), (32, 48));
        assert_eq!(size_of::<Params>(), 128);
        let constant = |name: &str| {
            let (_, c) = module
                .constants
                .iter()
                .find(|(_, c)| c.name.as_deref() == Some(name))
                .unwrap_or_else(|| panic!("{name}"));
            match module.global_expressions[c.init] {
                naga::Expression::Literal(naga::Literal::U32(value)) => u64::from(value),
                ref e => panic!("{name}: {e:?}"),
            }
        };
        // A whole section's instance w: `section_instance`'s bare UV step.
        let step = super::super::section_instance([0.0; 3], 0, 1.0)[3];
        assert_eq!(step, 1.0 / crate::mesher::UV_STEPS);
        for (name, value) in [
            ("STEP_BITS", u64::from(step.to_bits())),
            ("ROW", u64::from(ROW)),
            ("BLOCK", u64::from(WORKGROUP)),
            ("STATS", STATS),
            ("UNBOUNDED", u64::from(UNBOUNDED)),
            ("REACH", u64::from(REACH)),
            ("SKY", u64::from(SKY)),
            ("FADING", u64::from(FADING)),
        ] {
            assert_eq!(constant(name), value, "{name}");
        }
    }

    /// The shader's `near` is `in_view_distance`, on every column out to 140
    /// from the camera's at every render distance to 130.
    #[test]
    fn the_shaders_near_is_in_view_distance() {
        let (cam_x, cam_z) = (-37, 1_234_567);
        for r in 0..=130 {
            let params = Params {
                near: r,
                cam_x,
                cam_z,
                ..Default::default()
            };
            for dx in -140..=140 {
                for dz in -140..=140 {
                    let (x, z) = (cam_x + dx, cam_z + dz);
                    assert_eq!(
                        near(&params, x, z),
                        in_view_distance((cam_x, cam_z), r, x, z),
                        "distance {r}, ({dx}, {dz})"
                    );
                }
            }
        }
    }

    /// A frame whose planes' sums could leave i32 is the CPU's (`tables` gives
    /// `None`): the camera 20 million blocks above or below the level, in any
    /// view, or not finite. Three million blocks out, they fit.
    #[test]
    fn planes_past_i32_take_the_cpu_path() {
        let mut rng = Rng(0x0F1E_2D3C_4B5A_6978);
        let mut words = Vec::new();
        for _ in 0..500 {
            let frustum = view(&mut rng);
            let x = f64::from(rng.range(-30_000_000, 30_000_000)) + f64::from(rng.unit());
            let z = f64::from(rng.range(-30_000_000, 30_000_000)) + f64::from(rng.unit());
            let radius = rng.range(2, 128);
            for (y, fits) in [
                (2.0e7, false),
                (-2.0e7, false),
                (3.0e6, true),
                (-3.0e6, true),
                (64.5, true),
            ] {
                let input = inputs(DVec3::new(x, y, z), radius, &frustum);
                assert_eq!(tables(&input, &mut words).is_some(), fits, "y {y}");
            }
            for camera in [
                DVec3::new(f64::NAN, 64.0, z),
                DVec3::new(f64::INFINITY, 64.0, z),
                DVec3::new(x, f64::INFINITY, z),
            ] {
                let input = inputs(camera, radius, &frustum);
                assert!(tables(&input, &mut words).is_none(), "{camera}");
            }
        }
    }

    /// The integer test keeps every section the walk keeps (`Resident::visible`,
    /// a GPU frame's camera alone), whatever its bounds: inside its cell,
    /// overhanging it, a rotated model's reach, wholly outside it along an axis,
    /// flat, none, or past what a box holds; in columns tested section by section
    /// and in short columns entirely inside the frustum; around cameras out to 30
    /// million blocks and 100,000 above and below the level, in views of every
    /// shape, most of them toward the sections, one with a plane that is not
    /// finite. It rejects a real share of the sections within the distance.
    #[test]
    fn the_integer_test_keeps_what_the_walk_keeps() {
        let mut rng = Rng(0x5DEE_CE66_D1CE_4E5B);
        let mut words = Vec::new();
        let (mut listed, mut near_sections, mut rejected, mut inside) = (0, 0, 0, 0);
        for case in 0..48 {
            let radius = 2 + case % 11;
            // Short columns of three sections, at the camera's height, in a view
            // along them: many lie entirely inside the frustum.
            let ahead = case % 4 == 3;
            let aimed = matches!(case % 8, 0 | 1 | 4 | 5);
            let wide = |rng: &mut Rng| {
                let blocks = if case % 3 == 0 { 30_000_000 } else { 2_000 };
                f64::from(rng.range(-blocks, blocks)) + f64::from(rng.unit())
            };
            let height = match case % 8 {
                0 => 1.0e5,
                4 => -1.0e5,
                _ if ahead => 8.0,
                _ => f64::from(rng.range(-2_100, 2_100)),
            };
            let camera = DVec3::new(
                wide(&mut rng),
                height + f64::from(rng.unit()),
                wide(&mut rng),
            );
            let (cx, cz) = resident::camera_column(camera);
            let mut frustum = if ahead {
                let (yaw, pitch) = (rng.unit() * 360.0, rng.unit() * 10.0 - 5.0);
                let lens = super::super::world_projection(70.0, 16.0 / 9.0, 512.0);
                Frustum {
                    planes: super::super::frustum(lens * crate::camera::view_matrix(yaw, pitch)),
                }
            } else if aimed {
                // Toward the sections, give or take 10 degrees, and out past them.
                let middle = DVec3::new(f64::from(cx * 16 + 8), 128.0, f64::from(cz * 16 + 8));
                let to = middle - camera;
                let yaw = (-to.x).atan2(to.z).to_degrees() as f32;
                let pitch = (-to.y / to.length()).asin().to_degrees() as f32;
                let turn = |rng: &mut Rng| (rng.unit() - 0.5) * 20.0;
                let yaw = yaw + turn(&mut rng);
                let pitch = (pitch + turn(&mut rng)).clamp(-90.0, 90.0);
                looking(&mut rng, yaw, pitch, to.length() as f32 + 400.0)
            } else {
                view(&mut rng)
            };
            if case == 9 {
                // Not finite: it rejects nothing, as in `intersects`.
                frustum.planes[3] = Vec4::NAN;
            }
            let rows = if ahead { 3..6 } else { 0..24 };
            // Each section's value is its record's box.
            let mut r = resident::Resident::default();
            let mut sections = Vec::new();
            for x in cx - radius - 2..=cx + radius + 2 {
                for z in cz - radius - 2..=cz + radius + 2 {
                    let (mut low, mut high) = (i32::MAX, i32::MIN);
                    for y in rows.clone() {
                        if rng.below(4) == 0 {
                            continue;
                        }
                        let b = bounds(&mut rng);
                        r.insert_with_bounds((x, y, z), rounded_box(b), b);
                        sections.push((x, y, z));
                        (low, high) = (low.min(y), high.max(y));
                    }
                    if low > high || !in_view_distance((cx, cz), radius, x, z) {
                        continue;
                    }
                    let min_y = crate::chunk::MIN_Y;
                    let min = resident::origin((x, low, z), min_y, camera).as_vec3();
                    let top = resident::origin((x, high, z), min_y, camera).as_vec3();
                    inside += usize::from(frustum.contains(min, top + Vec3::splat(16.0)));
                }
            }
            let mut params = tables(&inputs(camera, radius, &frustum), &mut words)
                .unwrap_or_else(|| panic!("case {case}: the planes fit"));
            params.pages = 1;
            let [nx, ny, nz, _] = params.cells.map(|n| n as usize);
            // Every term rounds up: at least the plane's component times its
            // origin or box end, which f64 holds exactly.
            let n = nx + ny + nz;
            let planes = frustum.planes.iter().enumerate();
            for (j, p) in planes.filter(|(_, p)| p.is_finite()) {
                for i in 0..n {
                    let axis = usize::from(i >= nx) + usize::from(i >= nx + ny);
                    let exact = SCALE * f64::from(p[axis]) * f64::from(f32::from_bits(words[i]));
                    assert!(
                        f64::from(words[n + j * n + i] as i32) >= exact,
                        "case {case}, plane {j}"
                    );
                }
                for k in 0..96 {
                    let exact =
                        SCALE * f64::from(p[k / 32]) * f64::from(k as i32 % 32 - CODE_OFFSET);
                    assert!(
                        f64::from(words[7 * n + 96 * j + k] as i32) >= exact,
                        "case {case}, plane {j}"
                    );
                }
            }
            let mut out = Vec::new();
            r.visible(camera, radius, &frustum, |_| true, |_, _| false, &mut out);
            for v in &out {
                let record = Record::new(v.key, 0, 0, 1, *r.get(v.slot));
                assert!(
                    keeps(&params, &words, &record),
                    "case {case}: {:?} in view",
                    v.key
                );
            }
            listed += out.len();
            for &key in &sections {
                if in_view_distance((cx, cz), radius, key.0, key.2) {
                    let record = Record::new(key, 0, 0, 1, *r.find(key).expect("resident"));
                    near_sections += 1;
                    rejected += usize::from(!keeps(&params, &words, &record));
                }
            }
        }
        assert!(listed > 5_000, "{listed} in view");
        assert!(
            rejected * 10 > near_sections && rejected < near_sections,
            "{rejected} of {near_sections} within the distance left out"
        );
        assert!(inside > 20, "{inside} columns entirely inside");
    }

    /// Where the walk's f32 test stops keeping a section, the integer test still
    /// keeps it. A plane through a box's corner, or a column box's, and the
    /// camera moved along its normal until `Frustum::intersects` (or `contains`)
    /// flips, by bisection: at the last camera it keeps, the integer test keeps
    /// the section (every section of the column), and the plane's exact value
    /// there is below 0 by no more than the margin covers. 100,000 blocks above
    /// the level and more, it is below by more than a 256th of a block
    /// somewhere: the margin is needed.
    #[test]
    fn the_margin_covers_production_rounding() {
        let mut rng = Rng(0x1405_7B7E_F767_814F);
        let min_y = crate::chunk::MIN_Y;
        let mut words = Vec::new();
        // The largest shortfall found at each height.
        let mut worst = [0.0f64; 4];
        for (h, lift) in [64.0, 1.0e4, 1.0e5, 1.0e6].into_iter().enumerate() {
            for trial in 0..400 {
                let base = DVec3::new(
                    f64::from(rng.range(-3_000, 3_000)) + f64::from(rng.unit()),
                    lift + f64::from(rng.unit()),
                    f64::from(rng.range(-3_000, 3_000)) + f64::from(rng.unit()),
                );
                let (cx, cz) = resident::camera_column(base);
                let (x, z) = (cx + rng.range(-1, 1), cz + rng.range(-1, 1));
                // Up to four sections of a column, each with whole-block bounds of
                // its own (`rounded_box` keeps them as they are).
                let low = rng.range(0, 20);
                let high = low + rng.range(0, 3);
                let mut rows: Vec<(i32, [Vec3; 2])> = (low..=high)
                    .map(|y| {
                        let [ex, ey, ez] = [0, 1, 2].map(|_| {
                            let start = rng.range(-8, 16);
                            [start, rng.range(start.max(0), 23)].map(|end| end as f32)
                        });
                        let min = Vec3::new(ex[0], ey[0], ez[0]);
                        (y, [min, Vec3::new(ex[1], ey[1], ez[1])])
                    })
                    .collect();
                // A normal leaning along y, where the origins are largest.
                let normal = loop {
                    let n = DVec3::new(
                        f64::from(rng.unit()) - 0.5,
                        f64::from(rng.unit()) - 0.5,
                        f64::from(rng.unit()) - 0.5,
                    );
                    if n.length() > 0.1 && n.y.abs() > 0.3 * n.length() {
                        break n.normalize();
                    }
                };
                let column = trial % 2 == 1;
                // The box the plane runs through. In a column, the section at the
                // column box's corner gets that cell's lowest corner along the plane
                // as its box: `keeps` there has only the margin to spare.
                let at = usize::from(column && normal.y < 0.0) * (rows.len() - 1);
                if column {
                    let c = Vec3::from_array(std::array::from_fn(|i| {
                        if normal[i] >= 0.0 { 0.0 } else { 16.0 }
                    }));
                    rows[at].1 = [c, c];
                }
                let origin = |camera: DVec3, y: i32| resident::origin((x, y, z), min_y, camera);
                // Along an axis the plane is at least 0 along, `intersects` looks at a
                // box's high end and `contains` at the column box's low end: on the
                // `[c, c]` box, the same corner.
                let (y, [min, max]) = rows[at];
                let o = origin(base, y);
                let corner = DVec3::from_array(std::array::from_fn(|i| {
                    o[i] + f64::from(if normal[i] >= 0.0 { max[i] } else { min[i] })
                }));
                let plane = normal.extend(-normal.dot(corner)).as_vec4();
                let plane = plane / plane.truncate().length();
                let frustum = Frustum {
                    planes: [plane, Vec4::W, Vec4::W, Vec4::W, Vec4::W, Vec4::W],
                };
                let walk_keeps = |t: f64| {
                    let camera = base + normal * t;
                    let o = origin(camera, low).as_vec3();
                    if column {
                        let top = origin(camera, high).as_vec3() + Vec3::splat(16.0);
                        frustum.contains(o, top)
                    } else {
                        let [min, max] = rows[0].1;
                        frustum.intersects(o + min, o + max)
                    }
                };
                let (mut kept, mut left) = (-4.0, 4.0);
                assert!(
                    walk_keeps(kept) && !walk_keeps(left),
                    "height {lift}, trial {trial}: the plane runs by"
                );
                for _ in 0..64 {
                    let t = 0.5 * (kept + left);
                    if walk_keeps(t) {
                        kept = t;
                    } else {
                        left = t;
                    }
                }
                let camera = base + normal * kept;
                let mut params = tables(&inputs(camera, 3, &frustum), &mut words)
                    .unwrap_or_else(|| panic!("height {lift}, trial {trial}: the planes fit"));
                params.pages = 1;
                let tested = if column { &rows[..] } else { &rows[..1] };
                for &(y, b) in tested {
                    let record = Record::new((x, y, z), 0, 0, 1, rounded_box(Some(b)));
                    assert!(
                        keeps(&params, &words, &record),
                        "height {lift}, trial {trial}: ({x}, {y}, {z}) kept"
                    );
                }
                // The plane's lowest exact value, from the f32 origins, at the corner
                // the test looks at of each tested box (in a column the `[c, c]` box's,
                // its cells' lowest), and the margin's bound on what the walk rounds.
                let p = plane.as_dvec4();
                let (mut value, mut most) = (f64::INFINITY, 0.0f64);
                for &(y, [min, max]) in tested {
                    let o = origin(camera, y).as_vec3();
                    let end = |i: usize| f64::from(if p[i] >= 0.0 { max[i] } else { min[i] });
                    let exact: f64 = (0..3).map(|i| p[i] * (f64::from(o[i]) + end(i))).sum();
                    let corners: f64 = (0..3)
                        .map(|i| p[i].abs() * (f64::from(o[i]).abs() + 24.0))
                        .sum();
                    value = value.min(exact + p.w);
                    most = most.max(6.0 * UNIT * corners + 2.0 * UNIT * p.w.abs());
                }
                let short = (-value).max(0.0);
                assert!(
                    short <= most,
                    "height {lift}, trial {trial}: {short} below 0, past {most}"
                );
                worst[h] = worst[h].max(short);
            }
        }
        assert!(worst[2].max(worst[3]) > 1.0 / SCALE, "{worst:?}");
    }

    /// A device of `backends` that draws counts with first instances, if this
    /// machine has one; on DirectX 12, `compiler` compiles its shaders. Not
    /// Intel's Gen11 and Gen12 Vulkan driver, which gets first instances wrong
    /// and is never offered GPU culling (`supported`).
    fn device(
        backends: wgpu::Backends,
        compiler: wgpu::Dx12Compiler,
        name: &str,
    ) -> Option<(wgpu::Device, wgpu::Queue)> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = backends;
        desc.backend_options.dx12.shader_compiler = compiler;
        let instance = wgpu::Instance::new(desc);
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        let needed =
            wgpu::Features::MULTI_DRAW_INDIRECT_COUNT | wgpu::Features::INDIRECT_FIRST_INSTANCE;
        if !adapter.features().contains(needed)
            || super::super::flat::is_broken_intel_vulkan(&adapter.get_info())
        {
            return None;
        }
        eprintln!("GPU culling on {} ({name})", adapter.get_info().name);
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: needed,
            ..Default::default()
        }))
        .ok()
    }

    /// `test` on each device here that draws counts with first instances:
    /// Vulkan, and DirectX 12 with each of its compilers. Players' DirectX 12
    /// compiles with FXC, part of Windows: wgpu takes DXC only where
    /// `dxcompiler.dll` loads, and the game ships none.
    fn on_each_device(test: impl Fn(&wgpu::Device, &Queue)) {
        let mut tested = false;
        for (backends, compiler, name) in [
            (wgpu::Backends::VULKAN, Default::default(), "Vulkan"),
            (
                wgpu::Backends::DX12,
                wgpu::Dx12Compiler::Fxc,
                "DirectX 12, FXC",
            ),
            (
                wgpu::Backends::DX12,
                wgpu::Dx12Compiler::default_dynamic_dxc(),
                "DirectX 12, DXC",
            ),
        ] {
            if let Some((device, queue)) = device(backends, compiler, name) {
                test(&device, &Queue::new(queue, false));
                tested = true;
            }
        }
        if !tested {
            eprintln!("no GPU with indirect count draws: skipped");
        }
    }

    fn read(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        buffer: &wgpu::Buffer,
        bytes: u64,
    ) -> Vec<u32> {
        let (tx, rx) = std::sync::mpsc::channel();
        wgpu::util::DownloadBuffer::read_buffer(
            device,
            queue,
            &buffer.slice(..bytes),
            move |result| tx.send(result.unwrap().to_vec()).unwrap(),
        );
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        bytemuck::pod_collect_to_vec(&rx.recv().unwrap())
    }

    /// The pass draws exactly what its CPU reference (`keeps`, and cave
    /// culling's `Reach::contains`) keeps: one draw of all the solid quads of
    /// each, from its page and first vertex, with the instance the CPU path
    /// gives a whole section, every page's draws in id order. One table serves
    /// every case, changed between them as sections come, go and move (ids
    /// reused, records written as updates and whole), cleared once as for
    /// another world, and kept through a frame the CPU draws; in frusta of every
    /// shape, one with a plane that is not finite, with boxes of every kind,
    /// and from cameras near the world's origin and 20 million blocks out. Most
    /// cases have a cave-culling graph, its box as wide as the walk's,
    /// narrower, or a column wider, full height or clamped, its sky band
    /// reached or not: the pass reads the copy it holds when a frame brings the
    /// same graph (once through a bind group made anew), sends a new one after
    /// a frame the CPU draws, and makes its copy anew for one too large for its
    /// room. A fifth of the sections fade, flagged anew before each case
    /// (`FADING`, sent as updates or with the whole table): the pass leaves
    /// them to the CPU.
    #[test]
    fn the_gpu_draws_what_the_integer_test_keeps() {
        on_each_device(draws_match);
    }

    fn draws_match(device: &wgpu::Device, queue: &Queue) {
        let mut rng = Rng(0x2545_F491_4F6C_DD1D);
        // The flags have their own sequence: the cases are otherwise as before.
        let mut flags = Rng(0x9E37_79B9_7F4A_7C15);
        let (min_y, height) = (-64, 24);
        let mut gpu = GpuCulling::new(device).expect("the pass");
        let mut uploads = Uploads::new(device);
        let mut held: BTreeMap<u32, Placed> = BTreeMap::new();
        // Fading sections the frustum and the graph keep, left to the CPU.
        let (mut camera, mut compared, mut culled, mut faded) = (DVec3::ZERO, 0, 0, 0);
        let mut words = Vec::new();
        // Cave culling's graph while a case has one (its generation, from x and
        // z, radius, height and whether its search reached the sky) and its bits.
        let (mut graph, mut bits) = (None, Vec::new());
        // Sections the frustum keeps with a graph, those it leaves out, and
        // those above its box it leaves out and keeps.
        let (mut graphed, mut unreached, mut band) = (0, 0, [0; 2]);
        for case in 0..24 {
            let radius = [2, 5, 9][case % 3];
            // Case 4 looks from case 3's camera, 20 million blocks out, while
            // every section moves and none comes: the whole table is written
            // though it keeps its room. Cases 11 and 18 look from the camera of
            // the case before, whose graph they keep.
            if !matches!(case, 4 | 11 | 18) {
                let far = |axis| if case % 4 == axis { 10_000.0 } else { 1.0 };
                camera = DVec3::new(
                    f64::from(rng.range(-2_000, 2_000)) * far(3) + f64::from(rng.unit()),
                    f64::from(rng.range(-60, 300)) + f64::from(rng.unit()),
                    f64::from(rng.range(-2_000, 2_000)) * far(1) + f64::from(rng.unit()),
                );
            }
            let mut frustum = view(&mut rng);
            if case == 9 {
                frustum.planes[rng.below(6) as usize] = Vec4::NAN;
            }
            let (cx, cz) = resident::camera_column(camera);
            // None in a quarter of the cases (cave culling off). Else around the
            // camera's column, as wide as the walk's search (the distance and 2)
            // or anything from 1 column to past the distance, full height or
            // clamped as over low ground, its sky band reached in half the cases,
            // three quarters of its cells reached. Cases 11 and 18 bring the
            // graph of the case before: the pass reads the copy it holds, 18
            // through a bind group made anew. Case 7's is new and comes first with
            // a frame the CPU draws; case 20's, 131 KB, is past the copy's room.
            if !matches!(case, 11 | 18) {
                let given = matches!(case, 4 | 7 | 10 | 17 | 20) || rng.below(4) != 0;
                graph = given.then(|| {
                    let span = match case {
                        20 => 100,
                        _ if rng.below(2) == 0 => radius + 2,
                        _ => rng.range(1, radius + 3),
                    };
                    let rows = if case % 4 < 2 && case != 20 {
                        rng.range(3, 8)
                    } else {
                        height + 2
                    };
                    let cells = ((2 * span + 1).pow(2) * rows) as usize;
                    bits.clear();
                    bits.extend(
                        (0..cells.div_ceil(32)).map(|_| rng.bits() as u32 | rng.bits() as u32),
                    );
                    let from = (cx + rng.range(-1, 1), cz + rng.range(-1, 1));
                    (rng.bits(), from, span, rows, case % 8 < 4)
                });
            }
            let reach = graph.map(|(generation, from, span, rows, sky_reached)| Reach {
                generation,
                from_x: from.0,
                from_z: from.1,
                radius: span,
                y0: -1,
                height: rows,
                sections: height,
                sky_reached,
                bits: &bits,
            });
            if case == 12 {
                // Another world.
                gpu.records.clear();
                held.clear();
            }
            // A third of the sections go and a sixth move, some to pages
            // past the last.
            let mut ids: Vec<u32> = held.keys().copied().collect();
            for _ in 0..ids.len() / 3 {
                let id = ids.swap_remove(rng.below(ids.len() as u32) as usize);
                gpu.records.remove(id);
                held.remove(&id);
            }
            let mut moves = ids.len() / 6;
            while !ids.is_empty() && (moves > 0 || (case == 4 && !gpu.records.all_dirty)) {
                moves = moves.saturating_sub(1);
                let id = ids[rng.below(ids.len() as u32) as usize];
                let (page, start) = (rng.below(6), u64::from(rng.below(40_000)) * 256);
                gpu.records.relocate(id, page, start);
                let placed = held.get_mut(&id).expect("a live id");
                (placed.1, placed.2) = (page, start);
            }
            // New sections around the camera, within the distance and past it.
            let place = |gpu: &mut GpuCulling, held: &mut BTreeMap<u32, Placed>, rng: &mut Rng| {
                let near = |rng: &mut Rng| rng.range(-radius - 3, radius + 3);
                let key = (cx + near(rng), rng.range(0, height - 1), cz + near(rng));
                let (page, start) = (rng.below(3), u64::from(rng.below(40_000)) * 256);
                let quads = 1 + rng.below(2_000);
                let boxed = rounded_box(bounds(rng));
                let id = gpu
                    .records
                    .insert(Record::new(key, page, start, quads, boxed));
                assert!(
                    held.insert(id, (key, page, start, quads, boxed, false))
                        .is_none(),
                    "id {id} twice"
                );
            };
            let new = if case == 4 { 0 } else { 3_000 };
            for _ in 0..new {
                place(&mut gpu, &mut held, &mut rng);
            }
            let room = gpu.sections.bytes;
            if case == 18 {
                // Past the table's room: it grows, and is written whole.
                while u64::from(gpu.records.ids()) * Record::BYTES <= room {
                    place(&mut gpu, &mut held, &mut rng);
                }
            }
            // A fifth of the sections fade: the flags that change go up as
            // updates, or with the whole table (cases 4 and 18), and through
            // case 7's frame the CPU draws.
            for (&id, placed) in held.iter_mut() {
                placed.5 = flags.below(5) == 0;
                gpu.records.set_fading(id, placed.5);
            }
            let whole = gpu.records.all_dirty;
            let mut encoder = device.create_command_encoder(&Default::default());
            let input = FrameInputs {
                camera,
                min_y,
                sections: height,
                radius,
                frustum: &frustum,
                reach,
            };
            if case == 7 {
                // Planes past the integers: the CPU draws that frame, nothing is
                // written, and the records that changed wait for the next.
                let up = FrameInputs {
                    camera: camera + DVec3::Y * 2.0e7,
                    ..input
                };
                let prepared = gpu.prepare(device, queue, &mut uploads, &mut encoder, &up);
                assert_eq!(prepared, Prepared::Cpu, "case 7");
                assert!(gpu.records.all_dirty || !gpu.records.dirty.is_empty());
            }
            let prepared = gpu.prepare(device, queue, &mut uploads, &mut encoder, &input);
            assert_eq!(prepared, Prepared::Gpu, "case {case}");
            match case {
                4 => assert!(
                    whole && gpu.sections.bytes == room,
                    "case 4: in place, whole"
                ),
                18 => assert!(gpu.sections.bytes > room, "case 18: the table grows"),
                20 => assert!(gpu.reach.bytes > 1 << 16, "case 20: the graph's copy grows"),
                _ => {}
            }
            let mut params = tables(&input, &mut words).expect("the planes fit");
            params.pages = gpu.pass_pages;
            let mut expected = Vec::new();
            for &(key, page, start, quads, boxed, fading) in held.values() {
                let record = Record::new(key, page, start, quads, boxed);
                let kept = keeps(&params, &words, &record.fading(fading));
                let reached = reach.is_none_or(|r| r.contains(key));
                // In view, but fading: the CPU's.
                faded += usize::from(fading && reached && keeps(&params, &words, &record));
                if kept && let Some(r) = reach {
                    graphed += 1;
                    unreached += usize::from(!reached);
                    // Above the box, over its columns: the sky band.
                    let over = (key.0 - r.from_x).abs() <= r.radius
                        && (key.2 - r.from_z).abs() <= r.radius;
                    if over && key.1 >= r.y0 + r.height {
                        band[usize::from(reached)] += 1;
                    }
                }
                if kept && reached {
                    let origin = resident::origin(key, min_y, camera).as_vec3();
                    let instance = super::super::section_instance(origin.to_array(), 0, 1.0);
                    expected.push((page, start as u32, quads * 6, instance.map(f32::to_bits)));
                } else if !fading && in_view_distance((cx, cz), radius, key.0, key.2) {
                    culled += 1;
                }
            }
            gpu.dispatch(&mut encoder, None);
            uploads.finish(&encoder);
            queue.submit([encoder.finish()]);
            let pages = u64::from(gpu.pass_pages);
            let counts = read(device, queue, &gpu.counts.buffer, (STATS + pages) * 4);
            let draws = read(device, queue, &gpu.draws.buffer, gpu.draws.bytes);
            let origins = read(device, queue, &gpu.origins.buffer, u64::from(gpu.ids) * 16);
            let mut got = Vec::new();
            for &(page, first, size) in &gpu.pages {
                let count = counts[(STATS + u64::from(page)) as usize];
                assert!(
                    count <= size,
                    "case {case}, page {page}: {count} draws, room {size}"
                );
                let mut last = None;
                for d in draws[first as usize * 5..][..count as usize * 5].chunks(5) {
                    // In id order, the same every frame.
                    assert!(
                        last < Some(d[4]),
                        "case {case}, page {page}: out of id order"
                    );
                    last = Some(d[4]);
                    assert_eq!([d[1], d[2]], [1, 0], "case {case}: one instance, index 0");
                    let instance = &origins[d[4] as usize * 4..][..4];
                    let instance: [u32; 4] = instance.try_into().unwrap();
                    got.push((page, d[3], d[0], instance));
                }
            }
            expected.sort_unstable();
            got.sort_unstable();
            assert_eq!(got.len(), expected.len(), "case {case}: draws");
            assert_eq!(got, expected, "case {case}");
            let kept = expected.len() as u32;
            assert_eq!(
                counts[..2],
                [kept, kept],
                "case {case}: sections kept, draws"
            );
            compared += expected.len();
        }
        // Not a comparison of empty lists, nor of a test that keeps everything.
        // The graphs leave out a real share of what the frustum keeps, and their
        // sky band keeps sections above a box in some cases and not in others.
        // The flags leave out about a fifth of what they would keep.
        assert!(compared > 2_000, "{compared} draws");
        assert!(culled > 2_000, "{culled} within the distance left out");
        assert!(faded * 10 > compared, "{faded} fading of {compared} drawn");
        assert!(
            unreached * 10 > graphed && unreached < graphed,
            "{unreached} of {graphed} in view unreached"
        );
        assert!(
            band[0] > 0 && band[1] > 0,
            "the sky band left out and kept {band:?}"
        );
    }

    /// `draw`'s count draws draw what the pass kept, page by page: each kept
    /// section's quads from its own range, with its own origin, which the
    /// draw's first instance picks out of the instance-rate origins (on DirectX
    /// 12 too, where no count draw's first instance reaches `instance_index`),
    /// and nothing past a page's count, though its region still holds the frame
    /// before's longer list. Nor the sections that went since, the table's last
    /// among them. Drawn through the terrain's vertex layouts, each quad writes
    /// its origin's bits to a pixel of its own.
    #[test]
    fn the_count_draws_draw_each_kept_section_with_its_origin() {
        on_each_device(draws_land);
    }

    /// `draws_land`'s shaders: a quad's corners put it on its pixel, which
    /// gets the bits of the origin it is drawn with.
    const ORIGINS: &str = "
struct Quad {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) origin: vec4<u32>,
}
@vertex fn vs(@location(0) position: vec3<f32>, @location(5) origin: vec4<f32>) -> Quad {
    return Quad(vec4<f32>(position.xy, 0.0, 1.0), bitcast<vec4<u32>>(origin));
}
@fragment fn fs(quad: Quad) -> @location(0) vec4<u32> {
    return quad.origin;
}
";

    fn draws_land(device: &wgpu::Device, queue: &Queue) {
        use crate::mesher::TerrainVertex;
        // Section i's quad q covers pixel (4 * (i % 16) + q, i / 16). The
        // sections take turns on pages 0, 1 and 3, a grain each: page 2 has none.
        const SECTIONS: u32 = 192;
        let (width, height) = (64, SECTIONS / 16);
        let stride = std::mem::size_of::<TerrainVertex>() as u64;
        let grain = super::super::arena::GRAIN;
        let mut arena = Arena::new(device, stride, grain, false);
        for _ in 0..4 {
            assert!(arena.open_page(device, u64::from(SECTIONS / 3) * grain));
        }
        let mut gpu = GpuCulling::new(device).expect("the pass");
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let (radius, min_y, levels) = (4, -64, 24);
        let corner = |x: u32, y: u32| TerrainVertex {
            pos: [
                x as f32 / width as f32 * 2.0 - 1.0,
                1.0 - y as f32 / height as f32 * 2.0,
                0.0,
            ],
            ..bytemuck::Zeroable::zeroed()
        };
        let mut sections = Vec::new();
        for i in 0..SECTIONS {
            let (page, start) = ([0, 1, 3][i as usize % 3], u64::from(i / 3) * grain);
            let quads = 1 + rng.below(4);
            let (x, y) = (4 * (i % 16), i / 16);
            let vertices: Vec<_> = (x..x + quads)
                .flat_map(|x| [(x, y), (x + 1, y), (x + 1, y + 1), (x, y + 1)])
                .map(|(x, y)| corner(x, y))
                .collect();
            let bytes = bytemuck::cast_slice(&vertices);
            queue.write_buffer(arena.buffer(page), start * stride, bytes);
            let near = |rng: &mut Rng| rng.range(-radius, radius);
            let key = (near(&mut rng), rng.range(0, levels - 1), near(&mut rng));
            let record = Record::new(key, page, start, quads, rounded_box(bounds(&mut rng)));
            gpu.records.insert(record);
            sections.push((key, quads, record));
        }
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(ORIGINS.into()),
        });
        let origins = wgpu::vertex_attr_array![5 => Float32x4];
        let buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: stride,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &super::super::TERRAIN_ATTRIBUTES,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: 16,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &origins,
            }),
        ];
        let format = wgpu::TextureFormat::Rgba32Uint;
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: None,
                compilation_options: Default::default(),
                buffers: &buffers,
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: None,
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let pixels = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(width * height) * 16,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let indices = super::super::Renderer::quad_indices(device, 4);
        let mut uploads = Uploads::new(device);
        // The first frame's planes reject nothing: it keeps every section within
        // the distance. The second, from another camera in the same column, keeps
        // those in its view: fewer.
        let all = Frustum {
            planes: [Vec4::W; 6],
        };
        let lens = super::super::world_projection(70.0, 16.0 / 9.0, 512.0);
        let ahead = Frustum {
            planes: super::super::frustum(lens * crate::camera::view_matrix(0.0, 0.0)),
        };
        let frames = [
            (DVec3::new(8.375, 70.625, 3.125), &all),
            (DVec3::new(2.75, -3.5, 13.875), &ahead),
        ];
        let (mut words, mut before) = (Vec::new(), usize::MAX);
        for (frame, (camera, frustum)) in frames.into_iter().enumerate() {
            if frame == 1 {
                // Every fifth section goes before the second frame, and the last 20,
                // which the table's end leaves: the GPU still holds what the first
                // frame drew of them.
                for (id, section) in sections.iter_mut().enumerate() {
                    if id % 5 == 0 || id >= SECTIONS as usize - 20 {
                        gpu.records.remove(id as u32);
                        section.2 = Record::FREE;
                    }
                }
            }
            let mut encoder = device.create_command_encoder(&Default::default());
            let input = FrameInputs {
                camera,
                min_y,
                sections: levels,
                radius,
                frustum,
                reach: None,
            };
            let prepared = gpu.prepare(device, queue, &mut uploads, &mut encoder, &input);
            assert_eq!(prepared, Prepared::Gpu, "frame {frame}");
            let mut params = tables(&input, &mut words).expect("the planes fit");
            params.pages = gpu.pass_pages;
            let mut expected = vec![[0; 4]; (width * height) as usize];
            let mut kept = 0;
            for (i, &(key, quads, record)) in sections.iter().enumerate() {
                if keeps(&params, &words, &record) {
                    let origin = resident::origin(key, min_y, camera).as_vec3();
                    let instance = super::super::section_instance(origin.to_array(), 0, 1.0);
                    let at = i / 16 * width as usize + 4 * (i % 16);
                    expected[at..at + quads as usize].fill(instance.map(f32::to_bits));
                    kept += 1;
                }
            }
            assert!(kept > 0 && kept < before, "frame {frame}: {kept} kept");
            before = kept;
            gpu.dispatch(&mut encoder, None);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
            gpu.draw(&mut pass, &arena, |_| None);
            drop(pass);
            encoder.copy_texture_to_buffer(
                target.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &pixels,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(width * 16),
                        rows_per_image: None,
                    },
                },
                target.size(),
            );
            uploads.finish(&encoder);
            queue.submit([encoder.finish()]);
            let got = read(device, queue, &pixels, pixels.size());
            let row = width as usize;
            for (p, (got, want)) in got.chunks(4).zip(&expected).enumerate() {
                let (i, q) = (p / row * 16 + p % row / 4, p % 4);
                assert_eq!(got, want, "frame {frame}, section {i}, quad {q}");
            }
        }
    }
}
