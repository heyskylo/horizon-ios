//! One vertex buffer for every terrain section: sections own ranges of it, so
//! a frame binds it once and draws every section with indirect multi-draw.
//! Translucent quads can have an arena of their own (`Arena::translucent`),
//! with pages up to `LARGE_PAGE_BYTES`, and their sorted indices too
//! (`Arena::translucent_indices`): their back-to-front list then stays in one
//! page of each and draws in a few multi-draws.
use std::collections::BTreeMap;

/// Ranges are rounded up to this many vertices to limit fragmentation, and
/// start at multiples of it (`flat` names sections by these blocks).
pub const GRAIN: u64 = 256;
/// The largest page of the section arena, and of arenas shaders bind whole
/// as storage, in bytes.
pub const PAGE_BYTES: u64 = 256 << 20;
/// The largest page of an arena no shader binds as storage: under 2 GiB, where
/// DirectX 12 and OpenGL cap buffers (`i32::MAX`), with room for the bytes
/// wgpu-core adds to vertex buffers; the device's own limit still applies.
pub const LARGE_PAGE_BYTES: u64 = (1 << 31) - (1 << 20);

/// Free vertex ranges: start -> length, never adjacent.
#[derive(Debug, PartialEq)]
struct FreeList(BTreeMap<u64, u64>);

impl FreeList {
    fn new(capacity: u64) -> Self {
        Self(BTreeMap::from([(0, capacity)]))
    }
    /// First fit.
    fn take(&mut self, len: u64) -> Option<u64> {
        let (&start, &free) = self.0.iter().find(|(_, l)| **l >= len)?;
        self.0.remove(&start);
        if free > len {
            self.0.insert(start + len, free - len);
        }
        Some(start)
    }
    fn give(&mut self, start: u64, len: u64) {
        let (mut start, mut len) = (start, len);
        if let Some((&prev, &prev_len)) = self.0.range(..start).next_back()
            && prev + prev_len == start
        {
            self.0.remove(&prev);
            start = prev;
            len += prev_len;
        }
        if let Some(next_len) = self.0.remove(&(start + len)) {
            len += next_len;
        }
        self.0.insert(start, len);
    }
    /// `give` for many ranges: one merge of the sorted ranges into the list,
    /// where giving them one by one costs a few tree lookups each.
    fn give_all(&mut self, mut ranges: Vec<(u64, u64)>) {
        if ranges.len() < 64 {
            for (start, len) in ranges {
                self.give(start, len);
            }
            return;
        }
        ranges.sort_unstable();
        let mut free = std::mem::take(&mut self.0).into_iter().peekable();
        let mut given = ranges.into_iter().peekable();
        let mut merged: Vec<(u64, u64)> = Vec::new();
        loop {
            let next = match (free.peek(), given.peek()) {
                (Some(a), Some(b)) if a.0 <= b.0 => free.next(),
                (_, Some(_)) => given.next(),
                (Some(_), None) => free.next(),
                (None, None) => break,
            };
            let (start, len) = next.expect("peeked");
            match merged.last_mut() {
                Some(last) if last.0 + last.1 == start => last.1 += len,
                _ => merged.push((start, len)),
            }
        }
        // Sorted: the tree is built in one pass.
        self.0 = merged.into_iter().collect();
    }
}

/// One vertex buffer of the arena; only the newest page grows.
struct Page {
    buffer: wgpu::Buffer,
    /// Capacity in vertices.
    capacity: u64,
    free: FreeList,
    /// The buffer went to system memory (`Arena::create`'s fallback): meshes
    /// are never moved into it, and it is the first to empty.
    host: bool,
}

impl Page {
    /// Vertices in the page's free ranges.
    fn free_vertices(&self) -> u64 {
        self.free.0.values().sum()
    }
}

/// An arena with this many bytes in free ranges, at least half of its pages,
/// has a page worth emptying (`Arena::sparsest`).
const COMPACT_MIN_FREE: u64 = 128 << 20;

/// Terrain vertices in pages of at most `page_limit` vertices (devices cap
/// buffer sizes): a section's vertices sit in one page.
pub struct Arena {
    pages: Vec<Option<Page>>,
    stride: u64,
    page_limit: u64,
    initial_capacity: u64,
    usage: wgpu::BufferUsages,
    /// A page being emptied (`Arena::close`): nothing is placed in it.
    closing: Option<u32>,
    /// A page had to go to system memory (logged once).
    spilled: bool,
    /// Tests: GPU-local pages fail as if video memory were full.
    #[cfg(test)]
    video_memory_full: bool,
}

impl Arena {
    /// Vertices; `storage` also lets shaders read them as a storage buffer (a shader pack's
    /// terrain program reads the other corners of a vertex's quad for its normal).
    pub fn new(device: &wgpu::Device, stride: u64, capacity: u64, storage: bool) -> Self {
        let usage = if storage {
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE
        } else {
            wgpu::BufferUsages::VERTEX
        };
        Self::with_usage(device, stride, capacity, usage, PAGE_BYTES)
    }
    /// Sorted translucent indices; `storage` when compute passes read them.
    pub fn indices(device: &wgpu::Device, capacity: u64, storage: bool) -> Self {
        let usage = if storage {
            wgpu::BufferUsages::INDEX | wgpu::BufferUsages::STORAGE
        } else {
            wgpu::BufferUsages::INDEX
        };
        Self::with_usage(device, 4, capacity, usage, PAGE_BYTES)
    }
    /// Translucent quads' vertices, which no shader reads as storage: pages
    /// up to `LARGE_PAGE_BYTES`. The section arena's compaction never runs on
    /// it (`Renderer::compact_arena`): a page this large has no others to
    /// empty into, and the compaction moves sections' `page` ranges, not their
    /// `translucent_at`.
    pub fn translucent(device: &wgpu::Device, stride: u64, capacity: u64) -> Self {
        let usage = wgpu::BufferUsages::VERTEX;
        Self::with_usage(device, stride, capacity, usage, LARGE_PAGE_BYTES)
    }
    /// Sorted indices of the translucent arena's quads: pages up to
    /// `LARGE_PAGE_BYTES` as its own, so its draws keep to one index page too.
    pub fn translucent_indices(device: &wgpu::Device, capacity: u64) -> Self {
        let usage = wgpu::BufferUsages::INDEX;
        Self::with_usage(device, 4, capacity, usage, LARGE_PAGE_BYTES)
    }
    /// The sorted-index arena and, when `split`, the translucent arena
    /// (vertices of `stride`), both with large pages, which no shader binds;
    /// otherwise translucent quads stay in the section arena, and the indices
    /// in pages flat draws (`flat`) bind as storage.
    pub fn translucent_arenas(
        device: &wgpu::Device,
        stride: u64,
        flat: bool,
        split: bool,
    ) -> (Self, Option<Self>) {
        if split {
            let indices = Self::translucent_indices(device, 1 << 16);
            (indices, Some(Self::translucent(device, stride, 1 << 18)))
        } else {
            (Self::indices(device, 1 << 16, flat), None)
        }
    }
    fn with_usage(
        device: &wgpu::Device,
        stride: u64,
        capacity: u64,
        usage: wgpu::BufferUsages,
        page_bytes: u64,
    ) -> Self {
        // Pages of `page_bytes`, or less where the device's buffers (or the
        // storage bindings that read them whole) are smaller.
        let limits = device.limits();
        let mut bytes = limits.max_buffer_size.min(page_bytes);
        if usage.contains(wgpu::BufferUsages::STORAGE) {
            bytes = bytes.min(limits.max_storage_buffer_binding_size);
        }
        let page_limit = (bytes / stride) / GRAIN * GRAIN;
        let capacity = capacity.next_multiple_of(GRAIN).min(page_limit);
        Self {
            pages: Vec::new(),
            stride,
            page_limit,
            initial_capacity: capacity,
            usage,
            closing: None,
            spilled: false,
            #[cfg(test)]
            video_memory_full: false,
        }
    }
    /// A buffer of `capacity` vertices in video memory or, when that is full,
    /// `spill` allows it and so does the device, in system memory: Vulkan
    /// drivers fail an allocation rather than page like OpenGL ones, and
    /// wgpu's default error handler panics. `None` when neither has room.
    fn create(
        &mut self,
        device: &wgpu::Device,
        capacity: u64,
        spill: bool,
    ) -> Option<(wgpu::Buffer, bool)> {
        let usage = self.usage | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC;
        #[cfg(test)]
        let local = (!self.video_memory_full)
            .then(|| try_create(device, capacity * self.stride, usage))
            .flatten();
        #[cfg(not(test))]
        let local = try_create(device, capacity * self.stride, usage);
        let local = local.map(|buffer| (buffer, false));
        local.or_else(|| {
            // Mappable buffers are placed in host-visible memory, which the
            // allocator takes from system memory once video memory is full.
            if !spill
                || !device
                    .features()
                    .contains(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS)
            {
                return None;
            }
            let buffer = try_create(
                device,
                capacity * self.stride,
                usage | wgpu::BufferUsages::MAP_WRITE,
            )?;
            if !std::mem::replace(&mut self.spilled, true) {
                log::warn!(
                    "Horizon: video memory is full; more terrain is kept in system memory \
                     (lower the render distance if frames slow down)"
                );
            }
            Some((buffer, true))
        })
    }
    /// The rounded length a request of `vertices` occupies.
    fn span(vertices: u64) -> u64 {
        vertices.max(1).next_multiple_of(GRAIN)
    }
    /// Bytes of every page's buffer.
    pub fn bytes(&self) -> u64 {
        self.pages.iter().flatten().map(|p| p.capacity).sum::<u64>() * self.stride
    }
    /// The most vertices a page holds.
    pub fn shader_readable(&self) -> bool {
        self.usage.contains(wgpu::BufferUsages::STORAGE)
    }
    pub fn page_vertices(&self) -> u64 {
        self.page_limit
    }
    pub fn has_page(&self, page: u32) -> bool {
        self.pages.get(page as usize).is_some_and(Option::is_some)
    }
    /// Each live page's buffer, by page.
    pub fn page_buffers(&self) -> impl Iterator<Item = (u32, &wgpu::Buffer)> {
        let pages = self.pages.iter().enumerate();
        pages.filter_map(|(i, p)| Some((i as u32, &p.as_ref()?.buffer)))
    }
    pub fn buffer(&self, page: u32) -> &wgpu::Buffer {
        &self.pages[page as usize]
            .as_ref()
            .expect("live arena page")
            .buffer
    }
    /// A range of `vertices`: (page, first vertex). Grows the newest page
    /// (copying its contents on the GPU, in `encoder`) or starts a new one
    /// when nothing free is large enough; `None` when no memory is left for
    /// it. Writes to the arena belong in `encoder` too, so that those
    /// recorded before a grow reach the grown page.
    pub fn alloc(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        vertices: u64,
    ) -> Option<(u32, u64)> {
        let len = Self::span(vertices).min(self.page_limit);
        if let Some(found) = self.alloc_existing(len) {
            return Some(found);
        }
        let last = self.pages.len().saturating_sub(1);
        if let Some(page) = self.pages.last().and_then(Option::as_ref)
            && page.capacity < self.page_limit
            && self.closing != Some(last as u32)
        {
            let current = page.capacity;
            let grown = self.grown(current, len);
            if (grown - current >= len || grown == self.page_limit)
                && self.grow(device, encoder, last, grown)
                && let Some(start) = self.pages[last].as_mut().unwrap().free.take(len)
            {
                return Some((last as u32, start));
            }
        }
        let capacity = if self.pages.is_empty() {
            self.initial_capacity
        } else {
            1 << 20
        }
        .max(len)
        .min(self.page_limit);
        // A page just large enough when memory is short.
        let ((buffer, host), capacity) = match self.create(device, capacity, true) {
            Some(buffer) => (buffer, capacity),
            None if capacity > len => (self.create(device, len, true)?, len),
            None => return None,
        };
        let mut free = FreeList::new(capacity);
        let start = free.take(len).expect("a new page fits one section");
        let page = Page {
            buffer,
            capacity,
            free,
            host,
        };
        let index = self
            .pages
            .iter()
            .position(Option::is_none)
            .unwrap_or_else(|| {
                self.pages.push(None);
                self.pages.len() - 1
            });
        self.pages[index] = Some(page);
        Some((index as u32, start))
    }
    /// A range of `vertices` in a page that exists already and is not being
    /// emptied: never grows the arena.
    pub fn alloc_existing(&mut self, vertices: u64) -> Option<(u32, u64)> {
        let len = Self::span(vertices).min(self.page_limit);
        let closing = self.closing;
        self.pages
            .iter_mut()
            .enumerate()
            .filter(|(i, _)| closing != Some(*i as u32))
            .find_map(|(i, page)| Some((i as u32, page.as_mut()?.free.take(len)?)))
    }
    /// `alloc_existing` for a mesh moved off the closing page: only pages in
    /// video memory, so compaction never pushes terrain into system memory.
    pub fn alloc_moved(&mut self, vertices: u64) -> Option<(u32, u64)> {
        let len = Self::span(vertices).min(self.page_limit);
        let closing = self.closing;
        self.pages
            .iter_mut()
            .enumerate()
            .filter(|(i, p)| closing != Some(*i as u32) && p.as_ref().is_some_and(|p| !p.host))
            .find_map(|(i, page)| Some((i as u32, page.as_mut()?.free.take(len)?)))
    }
    /// The page to empty when much of the arena is free ranges (video memory
    /// that fragmentation keeps: a page goes back only once all of it is free):
    /// the one with the fewest vertices in use that the other pages' free
    /// ranges can take. `None` when the arena is dense enough or already
    /// emptying a page.
    pub fn sparsest(&self) -> Option<u32> {
        self.sparsest_from(COMPACT_MIN_FREE)
    }
    /// `sparsest` for an arena with at least `min_free` bytes in free ranges.
    fn sparsest_from(&self, min_free: u64) -> Option<u32> {
        if self.closing.is_some() {
            return None;
        }
        let pages = || {
            self.pages
                .iter()
                .enumerate()
                .filter_map(|(i, p)| Some((i, p.as_ref()?)))
        };
        let free: u64 = pages().map(|(_, p)| p.free_vertices()).sum();
        let capacity: u64 = pages().map(|(_, p)| p.capacity).sum();
        if free * self.stride < min_free || free * 2 < capacity {
            return None;
        }
        // Moves only go to video-memory pages (`alloc_moved`).
        let local_free: u64 = pages()
            .filter(|(_, p)| !p.host)
            .map(|(_, p)| p.free_vertices())
            .sum();
        pages()
            .filter_map(|(i, p)| {
                let used = p.capacity - p.free_vertices();
                let own = if p.host { 0 } else { p.free_vertices() };
                // The room elsewhere, less a quarter for first fit's gaps.
                let elsewhere = (local_free - own) / 4 * 3;
                // A page in system memory is emptied first.
                (used > 0 && used <= elsewhere).then_some((!p.host, used, i as u32))
            })
            .min()
            .map(|(_, _, i)| i)
    }
    /// Stops placing vertices in `page` until `reopen`.
    pub fn close(&mut self, page: u32) {
        self.closing = Some(page);
    }
    pub fn reopen(&mut self) {
        self.closing = None;
    }
    pub fn closing(&self) -> Option<u32> {
        self.closing
    }
    /// Records the copy of `vertices` from one range to another of other pages.
    pub fn copy(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        from: (u32, u64),
        to: (u32, u64),
        vertices: u64,
    ) {
        debug_assert_ne!(from.0, to.0);
        encoder.copy_buffer_to_buffer(
            self.buffer(from.0),
            from.1 * self.stride,
            self.buffer(to.0),
            to.1 * self.stride,
            vertices * self.stride,
        );
    }
    pub fn release(&mut self, page: u32, start: u64, vertices: u64) {
        let len = Self::span(vertices).min(self.page_limit);
        self.pages[page as usize]
            .as_mut()
            .expect("live arena page")
            .free
            .give(start, len);
    }
    /// `release` for many ranges (page, start, vertices) at once.
    pub fn release_all(&mut self, ranges: impl IntoIterator<Item = (u32, u64, u64)>) {
        let mut by_page: Vec<Vec<(u64, u64)>> = Vec::new();
        for (page, start, vertices) in ranges {
            let page = page as usize;
            if by_page.len() <= page {
                by_page.resize_with(page + 1, Vec::new);
            }
            by_page[page].push((start, Self::span(vertices).min(self.page_limit)));
        }
        for (page, ranges) in by_page.into_iter().enumerate() {
            if !ranges.is_empty() {
                self.pages[page]
                    .as_mut()
                    .expect("live arena page")
                    .free
                    .give_all(ranges);
            }
        }
    }
    /// The capacity the newest page grows to for a range of `len`: double, but
    /// by at most `PAGE_BYTES` past that size, so a large page is never left
    /// half empty (nor held twice during the copy) by hundreds of MiB.
    fn grown(&self, current: u64, len: u64) -> u64 {
        let step = current.min(PAGE_BYTES / self.stride);
        (current + step).max(current + len).min(self.page_limit)
    }
    /// Moves a page into a larger buffer; false (the page unchanged) when
    /// there is no memory for it. A page past `PAGE_BYTES` grows in video
    /// memory only: moving all of it to system memory would slow every frame
    /// that draws it, so a new page takes the range instead (and may spill).
    fn grow(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        page: usize,
        capacity: u64,
    ) -> bool {
        let capacity = capacity.next_multiple_of(GRAIN).min(self.page_limit);
        let spill = capacity * self.stride <= PAGE_BYTES;
        let Some((buffer, host)) = self.create(device, capacity, spill) else {
            return false;
        };
        let old = self.pages[page].as_mut().expect("live arena page");
        // Copies recorded for the old buffer run before this one.
        encoder.copy_buffer_to_buffer(&old.buffer, 0, &buffer, 0, old.capacity * self.stride);
        let previous = old.capacity;
        old.buffer = buffer;
        old.host = host;
        old.capacity = capacity;
        old.free.give(previous, capacity - previous);
        true
    }
    /// Reclaim after an upload batch, so replacing a section can reuse its range.
    pub fn trim(&mut self) {
        for page in &mut self.pages {
            if let Some(p) = page
                && p.free.0.len() == 1
                && p.free.0.get(&0) == Some(&p.capacity)
            {
                *page = None;
            }
        }
        if self.closing.is_some_and(|page| !self.has_page(page)) {
            self.closing = None;
        }
        // Keep live page indices stable; later allocations reuse interior holes.
        while self.pages.last().is_some_and(Option::is_none) {
            self.pages.pop();
        }
    }
    pub fn clear(&mut self) {
        self.pages.clear();
        self.closing = None;
    }
}

/// `create_buffer` with an out-of-memory failure returned instead of raised
/// (raised errors are fatal).
pub(super) fn try_create(
    device: &wgpu::Device,
    size: u64,
    usage: wgpu::BufferUsages,
) -> Option<wgpu::Buffer> {
    let scope = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Section buffer arena"),
        size,
        usage,
        mapped_at_creation: false,
    });
    // Native error scopes resolve at once.
    pollster::block_on(scope.pop()).is_none().then_some(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
        device_with(wgpu::Features::empty())
    }

    /// A device with those of `features` the adapter has.
    fn device_with(features: wgpu::Features) -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        eprintln!("arena GPU regression: {}", adapter.get_info().name);
        let required_features = adapter.features() & features;
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features,
            ..Default::default()
        }))
        .ok()
    }

    /// A device with the adapter's own limits, as the renderer opens it: page
    /// caps past the default 256 MiB buffer limit show there.
    fn device_at_limits() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .ok()
    }

    /// `device` and `device_at_limits`: the default limits bind 128 MiB of a storage
    /// buffer, half their 256 MiB buffers, so storage pages shrink to the binding there;
    /// the adapter's own let large pages past 256 MiB.
    fn devices() -> Vec<(wgpu::Device, wgpu::Queue)> {
        let devices = [device(), device_at_limits()];
        devices.into_iter().flatten().collect()
    }

    /// `Arena::alloc`, with the copies of a grow submitted at once.
    fn alloc(
        arena: &mut Arena,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vertices: u64,
    ) -> Option<(u32, u64)> {
        let mut encoder = device.create_command_encoder(&Default::default());
        let range = arena.alloc(device, &mut encoder, vertices);
        queue.submit([encoder.finish()]);
        range
    }

    #[test]
    fn writes_recorded_before_a_grow_reach_the_grown_page() {
        let Some((device, queue)) = device() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let mut arena = Arena::indices(&device, GRAIN, false);
        let mut uploads = super::super::upload::Uploads::new(&device);
        let mut encoder = device.create_command_encoder(&Default::default());
        let (page, start) = arena.alloc(&device, &mut encoder, GRAIN).unwrap();
        uploads.buffer(
            &mut encoder,
            arena.buffer(page),
            start * 4,
            &7u32.to_le_bytes(),
        );
        // The page is full: the next range grows it, in the same encoder.
        let (grown, next) = arena.alloc(&device, &mut encoder, GRAIN).unwrap();
        assert_eq!(grown, page);
        uploads.buffer(
            &mut encoder,
            arena.buffer(grown),
            next * 4,
            &9u32.to_le_bytes(),
        );
        uploads.finish(&encoder);
        queue.submit([encoder.finish()]);
        assert_eq!(read(&device, &queue, arena.buffer(page), start * 4), 7);
        assert_eq!(read(&device, &queue, arena.buffer(page), next * 4), 9);
    }

    #[test]
    fn a_sparse_page_empties_into_the_others_and_is_reclaimed() {
        let Some((device, queue)) = device() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let mut arena = Arena::new(&device, 4, GRAIN, false);
        arena.page_limit = 4 * GRAIN;
        // Eight ranges fill two pages, each holding its own number.
        let ranges: Vec<(u32, u64)> = (0..8)
            .map(|i| {
                let range = alloc(&mut arena, &device, &queue, GRAIN).unwrap();
                queue.write_buffer(
                    arena.buffer(range.0),
                    range.1 * 4,
                    &(100 + i as u32).to_le_bytes(),
                );
                range
            })
            .collect();
        assert_eq!(arena.pages.iter().flatten().count(), 2);
        let (first, second) = (ranges[0].0, ranges[7].0);
        assert_ne!(first, second);
        // Fragmented: one range live in the first page, two in the second.
        for (i, &(page, start)) in ranges.iter().enumerate() {
            if ![0, 6, 7].contains(&i) {
                arena.release(page, start, GRAIN);
            }
        }
        // Dense enough arenas, and ones without room elsewhere, are left alone.
        assert_eq!(arena.sparsest_from(u64::MAX), None);
        assert_eq!(arena.sparsest_from(0), Some(first));
        arena.close(first);
        assert_eq!(
            arena.sparsest_from(0),
            None,
            "one page is emptied at a time"
        );
        // New ranges avoid the closing page though it has room.
        let (page, extra) = alloc(&mut arena, &device, &queue, GRAIN).unwrap();
        assert_eq!(page, second);
        arena.release(page, extra, GRAIN);
        // Its live range moves, data included, and the page goes back.
        let (page, start) = ranges[0];
        let to = arena.alloc_existing(GRAIN).unwrap();
        assert_eq!(to.0, second);
        let mut encoder = device.create_command_encoder(&Default::default());
        arena.copy(&mut encoder, (page, start), to, GRAIN);
        queue.submit([encoder.finish()]);
        arena.release(page, start, GRAIN);
        assert_eq!(read(&device, &queue, arena.buffer(to.0), to.1 * 4), 100);
        // Emptied but not trimmed yet: it stays closed, so uploads can't refill it.
        assert_eq!(arena.closing(), Some(page));
        let refill = alloc(&mut arena, &device, &queue, GRAIN).unwrap();
        assert_ne!(refill.0, page);
        arena.release(refill.0, refill.1, GRAIN);
        let before = arena.bytes();
        arena.trim();
        assert_eq!(arena.bytes(), before - 4 * GRAIN * 4);
        assert_eq!(arena.closing(), None);
        // The others kept their ranges.
        for i in [6, 7] {
            let (page, start) = ranges[i];
            assert_eq!(
                read(&device, &queue, arena.buffer(page), start * 4),
                100 + i as u32
            );
        }
    }

    /// The renderer's half of that compaction (`move_off_page`) moves what each
    /// section holds in the section arena, data and all. Split, that is its
    /// solid quads alone, and nothing of a section without any, whose (0, 0)
    /// names no range of page 0; else solid and translucent quads together,
    /// `translucent_at` along.
    #[test]
    fn compaction_moves_what_sections_hold_in_the_section_arena() {
        use super::super::{SectionGpu, move_off_page};
        let Some((device, queue)) = device() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let stride = std::mem::size_of::<crate::mesher::TerrainVertex>() as u64;
        let section = |(page, start), translucent_at, solid, translucent| SectionGpu {
            page,
            start,
            translucent_at,
            solid,
            sides: crate::mesher::Sides::unsorted(solid),
            translucent,
            sort: super::super::translucency::SortState::new(Vec::new()),
            sorted_indices: None,
            flat_id: None,
            fading: None,
            leaving: None,
            animated_layers: Box::new([]),
        };
        for split in [true, false] {
            let mut arena = Arena::new(&device, stride, 4 * GRAIN, false);
            arena.page_limit = 4 * GRAIN;
            // Page 0: two sections' grains and two free ones; page 1: one taken.
            let (_, a) = alloc(&mut arena, &device, &queue, GRAIN).unwrap();
            let (_, b) = alloc(&mut arena, &device, &queue, GRAIN).unwrap();
            let (_, rest) = alloc(&mut arena, &device, &queue, 2 * GRAIN).unwrap();
            assert_eq!(alloc(&mut arena, &device, &queue, GRAIN), Some((1, 0)));
            arena.release(0, rest, 2 * GRAIN);
            // Split, a grain of solid quads each (the first's translucent ones, and
            // all of a water section's, in the translucent arena); else a grain of
            // solid and translucent quads, and one of translucent quads alone.
            let mut sections = if split {
                vec![
                    section((0, a), (0, 0), 64, 64),
                    section((0, 0), (0, GRAIN), 0, 64),
                    section((0, b), (0, 0), 64, 0),
                ]
            } else {
                vec![
                    section((0, a), (0, a + 128), 32, 32),
                    section((0, b), (0, b), 0, 64),
                ]
            };
            // A range's first and last vertex hold its section's number, and the
            // first translucent one of a mixed range that plus 100.
            let write = |(page, at): (u32, u64), value: usize| {
                let bytes = (value as u32).to_le_bytes();
                queue.write_buffer(arena.buffer(page), at * stride, &bytes);
            };
            for (i, s) in sections.iter().enumerate() {
                if let Some((page, start, len)) = s.vertex_ranges(split).0 {
                    write((page, start), i);
                    write((page, start + len - 1), i);
                    if !split && s.solid > 0 && s.translucent > 0 {
                        write(s.translucent_at, 100 + i);
                    }
                }
            }
            let before: Vec<_> = sections
                .iter()
                .map(|s| (s.page, s.start, s.translucent_at))
                .collect();
            arena.close(0);
            let mut encoder = device.create_command_encoder(&Default::default());
            let meshes = sections.iter_mut();
            assert!(move_off_page(
                &mut arena,
                &mut encoder,
                None,
                meshes,
                0,
                split,
                u64::MAX,
                None,
            ));
            queue.submit([encoder.finish()]);
            let word =
                |(page, at): (u32, u64)| read(&device, &queue, arena.buffer(page), at * stride);
            for (i, s) in sections.iter().enumerate() {
                let Some((page, start, len)) = s.vertex_ranges(split).0 else {
                    assert_eq!(
                        (s.page, s.start, s.translucent_at),
                        before[i],
                        "split {split}"
                    );
                    continue;
                };
                assert_eq!(page, 1, "split {split}");
                assert_eq!(word((page, start)), i as u32);
                assert_eq!(word((page, start + len - 1)), i as u32);
                if !split {
                    assert_eq!(s.translucent_at, (page, start + u64::from(s.solid) * 4));
                    if s.solid > 0 {
                        assert_eq!(word(s.translucent_at), 100 + i as u32);
                    }
                }
            }
            // Page 0 empties without a range given twice, and page 1 took a grain
            // per section moved: one is left.
            arena.trim();
            assert!(!arena.has_page(0), "split {split}");
            assert!(arena.alloc_existing(GRAIN).is_some(), "split {split}");
            assert_eq!(arena.alloc_existing(GRAIN), None, "split {split}");
        }
    }

    #[test]
    fn empty_pages_are_reclaimed_without_moving_live_ranges() {
        let Some((device, queue)) = device() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let mut arena = Arena::new(&device, 4, GRAIN, false);
        arena.page_limit = 2 * GRAIN;
        assert_eq!(arena.bytes(), 0);
        let (first, a) = alloc(&mut arena, &device, &queue, GRAIN).unwrap();
        let (_, b) = alloc(&mut arena, &device, &queue, GRAIN).unwrap();
        let (live, c) = alloc(&mut arena, &device, &queue, 2 * GRAIN).unwrap();
        let buffer = arena.buffer(live).clone();
        let data = [1u8, 2, 3, 4];
        queue.write_buffer(&buffer, c * 4, &data);
        assert_eq!(arena.bytes(), 4 * GRAIN * 4);
        arena.release(first, a, GRAIN);
        arena.trim();
        assert_eq!(arena.bytes(), 4 * GRAIN * 4);
        arena.release(first, b, GRAIN);
        arena.trim();
        assert_eq!(arena.bytes(), 2 * GRAIN * 4);
        assert_eq!(arena.buffer(live), &buffer);
        let (reused, d) = alloc(&mut arena, &device, &queue, GRAIN).unwrap();
        assert_eq!(reused, first);

        let (tx, rx) = std::sync::mpsc::channel();
        wgpu::util::DownloadBuffer::read_buffer(
            &device,
            &queue,
            &arena.buffer(live).slice(c * 4..c * 4 + 4),
            move |result| tx.send(result.unwrap().to_vec()).unwrap(),
        );
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        assert_eq!(rx.recv().unwrap(), data);
        arena.release(reused, d, GRAIN);
        arena.release(live, c, 2 * GRAIN);
        arena.trim();
        assert_eq!(arena.bytes(), 0);
        assert!(arena.pages.is_empty());
    }

    /// Every arena's pages stay within its cap and the device's limits: the
    /// whole-page storage bindings of flat draws and shader packs included.
    #[test]
    fn pages_stay_within_their_caps_and_the_device_limits() {
        let devices = devices();
        if devices.is_empty() {
            eprintln!("no GPU adapter: skipped");
            return;
        }
        let bytes = |a: &Arena| a.page_vertices() * a.stride;
        for (device, _) in &devices {
            let limits = device.limits();
            for storage in [false, true] {
                for arena in [
                    Arena::new(device, 24, GRAIN, storage),
                    Arena::indices(device, GRAIN, storage),
                ] {
                    assert!(bytes(&arena) <= PAGE_BYTES.min(limits.max_buffer_size));
                    let binding = limits.max_storage_buffer_binding_size;
                    assert!(!storage || bytes(&arena) <= binding);
                }
            }
            for large in [
                Arena::translucent(device, 24, GRAIN),
                Arena::translucent_indices(device, GRAIN),
            ] {
                assert!(bytes(&large) <= LARGE_PAGE_BYTES.min(limits.max_buffer_size));
                assert!(!large.usage.contains(wgpu::BufferUsages::STORAGE));
            }
        }
    }

    /// The translucent arena comes with the split layout alone, and only then
    /// are the index pages large. Otherwise, as while a shader pack's terrain
    /// draws flat (gfx's `translucent_split`), translucent quads keep to the
    /// section arena, and flat draws (`flat`) bind the index pages as storage.
    #[test]
    fn translucent_arenas_follow_the_layout() {
        let devices = devices();
        if devices.is_empty() {
            eprintln!("no GPU adapter: skipped");
            return;
        }
        let mut exercised = false;
        for (device, _) in &devices {
            let limits = device.limits();
            let large = limits.max_buffer_size > PAGE_BYTES;
            exercised |= large;
            for flat in [false, true] {
                for split in [false, true] {
                    let case = format!("large {large}, flat {flat}, split {split}");
                    let (indices, translucent) = Arena::translucent_arenas(device, 24, flat, split);
                    assert_eq!(translucent.is_some(), split, "{case}");
                    let bytes = indices.page_vertices() * 4;
                    let storage = indices.usage.contains(wgpu::BufferUsages::STORAGE);
                    assert_eq!(storage, flat && !split, "{case}");
                    assert!(!storage || bytes <= limits.max_storage_buffer_binding_size);
                    assert_eq!(bytes > PAGE_BYTES, split && large, "{case}");
                    if let Some(t) = translucent {
                        assert_eq!(t.page_vertices() * 24 > PAGE_BYTES, large, "{case}");
                    }
                }
            }
        }
        if !exercised {
            eprintln!("buffers end at PAGE_BYTES here: large pages not exercised");
        }
    }

    /// Pages double up to `PAGE_BYTES`, then grow by `PAGE_BYTES` at most.
    #[test]
    fn large_pages_grow_by_bounded_steps() {
        let Some((device, _)) = device() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let mut arena = Arena::translucent(&device, 24, GRAIN);
        // Exercise the policy past the test device's own buffer limit.
        arena.page_limit = LARGE_PAGE_BYTES / 24 / GRAIN * GRAIN;
        let step = PAGE_BYTES / 24;
        assert_eq!(arena.grown(1 << 18, GRAIN), 1 << 19);
        assert_eq!(arena.grown(step, GRAIN), 2 * step);
        assert_eq!(arena.grown(3 * step, GRAIN), 4 * step);
        assert_eq!(arena.grown(3 * step, step + GRAIN), 4 * step + GRAIN);
        assert_eq!(
            arena.grown(arena.page_limit - GRAIN, GRAIN),
            arena.page_limit
        );
    }

    #[test]
    fn clearing_an_arena_releases_buffers_and_can_allocate_again() {
        let Some((device, queue)) = device() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        for mut arena in [
            Arena::new(&device, 28, GRAIN, false),
            Arena::translucent(&device, 24, GRAIN),
            Arena::indices(&device, GRAIN, false),
            Arena::translucent_indices(&device, GRAIN),
        ] {
            alloc(&mut arena, &device, &queue, GRAIN).unwrap();
            assert_eq!(arena.bytes(), arena.stride * GRAIN);
            arena.clear();
            assert_eq!(arena.bytes(), 0);
            assert_eq!(alloc(&mut arena, &device, &queue, GRAIN), Some((0, 0)));
            assert_eq!(arena.bytes(), arena.stride * GRAIN);
        }
    }

    #[test]
    fn free_ranges_coalesce() {
        let mut free = FreeList::new(4 * GRAIN);
        let a = free.take(GRAIN).unwrap();
        let b = free.take(2 * GRAIN).unwrap();
        let c = free.take(GRAIN).unwrap();
        assert_eq!((a, b, c), (0, GRAIN, 3 * GRAIN));
        assert_eq!(free.take(1), None);
        free.give(a, GRAIN);
        free.give(c, GRAIN);
        // Two holes of one grain: a two-grain request does not fit.
        assert_eq!(free.take(2 * GRAIN), None);
        free.give(b, 2 * GRAIN);
        assert_eq!(free, FreeList::new(4 * GRAIN));
    }

    #[test]
    fn many_ranges_given_at_once_coalesce_as_one_by_one() {
        let fill = || {
            let mut free = FreeList::new(1 << 30);
            let taken: Vec<(u64, u64)> = (0..500u64)
                .map(|i| {
                    let len = GRAIN * (1 + i * 7 % 5);
                    (free.take(len).unwrap(), len)
                })
                .collect();
            for &(start, len) in taken.iter().step_by(3) {
                free.give(start, len);
            }
            (free, taken)
        };
        // Every other range of those still taken, in a scattered order.
        let (mut one_by_one, taken) = fill();
        let (mut at_once, _) = fill();
        let mut given: Vec<(u64, u64)> = (0..taken.len())
            .filter(|i| i % 3 != 0 && i % 2 == 0)
            .map(|i| taken[i])
            .collect();
        given.sort_by_key(|&(start, _)| start.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        assert!(given.len() >= 64);
        for &(start, len) in &given {
            one_by_one.give(start, len);
        }
        at_once.give_all(given);
        assert_eq!(at_once, one_by_one);
        // The rest too: everything is free again, in one range.
        let rest: Vec<(u64, u64)> = (0..taken.len())
            .filter(|i| i % 3 != 0 && i % 2 != 0)
            .map(|i| taken[i])
            .collect();
        at_once.give_all(rest);
        assert_eq!(at_once, FreeList::new(1 << 30));
    }

    #[test]
    fn first_fit_reuses_the_lowest_hole() {
        let mut free = FreeList::new(8 * GRAIN);
        let a = free.take(GRAIN).unwrap();
        let _b = free.take(GRAIN).unwrap();
        free.give(a, GRAIN);
        assert_eq!(free.take(GRAIN), Some(a));
    }

    fn read(device: &wgpu::Device, queue: &wgpu::Queue, buffer: &wgpu::Buffer, offset: u64) -> u32 {
        let (tx, rx) = std::sync::mpsc::channel();
        wgpu::util::DownloadBuffer::read_buffer(
            device,
            queue,
            &buffer.slice(offset..offset + 4),
            move |result| tx.send(result.unwrap().to_vec()).unwrap(),
        );
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        u32::from_le_bytes(rx.recv().unwrap()[..4].try_into().unwrap())
    }

    #[test]
    fn full_video_memory_spills_to_system_memory() {
        let Some((device, queue)) = device_with(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS) else {
            return;
        };
        if !device
            .features()
            .contains(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS)
        {
            return;
        }
        let mut arena = Arena::indices(&device, GRAIN, false);
        let (page, start) = alloc(&mut arena, &device, &queue, GRAIN).unwrap();
        queue.write_buffer(arena.buffer(page), start * 4, &7u32.to_le_bytes());
        arena.video_memory_full = true;
        // The full page can't grow in video memory: it moves to system
        // memory with its contents, and new ranges come from there.
        let (grown, next) = alloc(&mut arena, &device, &queue, GRAIN).unwrap();
        assert!(arena.spilled);
        assert_eq!(grown, page);
        assert!(
            arena
                .buffer(page)
                .usage()
                .contains(wgpu::BufferUsages::MAP_WRITE)
        );
        queue.write_buffer(arena.buffer(grown), next * 4, &9u32.to_le_bytes());
        assert_eq!(read(&device, &queue, arena.buffer(page), start * 4), 7);
        assert_eq!(read(&device, &queue, arena.buffer(grown), next * 4), 9);
        // Compaction never moves terrain into a page in system memory.
        assert!(arena.pages[page as usize].as_ref().unwrap().host);
        assert_eq!(arena.alloc_moved(GRAIN), None);
    }

    #[test]
    fn no_memory_left_is_not_an_error() {
        let Some((device, queue)) = device() else {
            return;
        };
        let mut arena = Arena::indices(&device, GRAIN, false);
        alloc(&mut arena, &device, &queue, GRAIN).unwrap();
        arena.video_memory_full = true;
        // Without mappable primary buffers nothing can spill: the request
        // fails, and the arena keeps what it has.
        assert_eq!(alloc(&mut arena, &device, &queue, GRAIN), None);
        assert_eq!(arena.bytes(), GRAIN * 4);
        arena.release(0, 0, GRAIN);
        assert_eq!(alloc(&mut arena, &device, &queue, GRAIN), Some((0, 0)));
    }
}
