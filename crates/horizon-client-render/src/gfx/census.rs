//! What the renderer holds, for status, soak tests and memory reports (`horizon-ctl memory`,
//! `scene_replay --memory`): resident sections and far tiles, and the bytes of its largest
//! GPU resources and of its per-section bookkeeping.

use std::collections::HashMap;

use super::{
    Renderer, SectionGpu, SectionKey, TerrainVertex, arena, flat, lod, texture_bytes, visbuffer,
};

impl Renderer {
    /// Sections with GPU buffers, for soak tests watching residency.
    pub fn resident_sections(&self) -> usize {
        self.sections.len()
    }
    /// Quads of the far terrain tiles with GPU buffers.
    pub fn lod_quads(&self) -> u64 {
        self.lods
            .values()
            .map(|tile| u64::from(tile.mesh.solid + tile.mesh.translucent))
            .sum()
    }
    /// Tiles and quads of the far terrain tiles with GPU buffers, by level.
    pub fn lod_levels(&self) -> Vec<[u64; 2]> {
        let mut levels = Vec::new();
        for (key, tile) in &self.lods {
            let level = usize::from(key.level);
            if levels.len() <= level {
                levels.resize(level + 1, [0, 0]);
            }
            levels[level][0] += 1;
            levels[level][1] += u64::from(tile.mesh.solid + tile.mesh.translucent);
        }
        levels
    }
    /// Whether solid quads go to the arena in the compact layout
    /// (`packed_quads`), and whether far tiles' own records are on
    /// (`lod::packed::far_packing`).
    pub fn packed_layout(&self) -> [bool; 2] {
        [self.packed_quads, crate::lod::packed::far_packing()]
    }
    /// Bytes of the arena the resident chunk sections hold, and of the one far
    /// tiles hold: the same split `gpu_census` reports, for `status`, so the
    /// compact layouts can be seen working.
    pub fn vertex_bytes_split(&self) -> [u64; 2] {
        let stride = std::mem::size_of::<TerrainVertex>() as u64;
        let used = |s: &SectionGpu| (s.solid_slots + u64::from(s.translucent) * 4) * stride;
        [
            self.sections.values().map(used).sum(),
            self.lods.values().map(|tile| used(&tile.mesh)).sum(),
        ]
    }
    /// Bytes of the section vertex arenas' buffers, and of the vertices
    /// resident sections hold in them.
    pub fn section_vertex_bytes(&self) -> [u64; 2] {
        let stride = std::mem::size_of::<TerrainVertex>() as u64;
        let vertices: u64 = self
            .sections
            .values()
            .chain(self.lods.values().map(|tile| &*tile.mesh))
            .map(|s| s.solid_slots + u64::from(s.translucent) * 4)
            .sum();
        let translucent = self.translucent_arena.as_ref();
        let arenas = self.arena.bytes() + translucent.map_or(0, arena::Arena::bytes);
        [arenas, vertices * stride]
    }
    /// Bytes of the renderer's largest GPU resources, by name (`scene_replay --memory`):
    /// the terrain arenas (their pages, and what resident sections and far tiles use of
    /// them), the quad indices, the block sprites with their mips, the LabPBR maps, flat
    /// draws' buffers and the visibility buffer's.
    pub fn gpu_census(&self) -> Vec<(&'static str, u64)> {
        let stride = std::mem::size_of::<TerrainVertex>() as u64;
        let used = |s: &SectionGpu| (s.solid_slots + u64::from(s.translucent) * 4) * stride;
        let textures = |t: &[wgpu::Texture]| t.iter().map(texture_bytes).sum::<u64>();
        let translucent = self.translucent_arena.as_ref();
        vec![
            ("section_arena_pages", self.arena.bytes()),
            (
                "translucent_arena_pages",
                translucent.map_or(0, arena::Arena::bytes),
            ),
            ("sorted_index_pages", self.sorted_indices.bytes()),
            (
                "near_section_vertices",
                self.sections.values().map(used).sum(),
            ),
            (
                "held_section_vertices",
                self.hold
                    .sections()
                    .filter_map(|held| held.gpu.as_ref())
                    .map(used)
                    .sum(),
            ),
            (
                "far_tile_vertices",
                self.lods.values().map(|tile| used(&tile.mesh)).sum(),
            ),
            ("quad_indices", self.indices.size()),
            ("block_sprites", textures(&self.sprite_textures)),
            (
                "pbr_maps",
                self.pbr_maps
                    .as_ref()
                    .map_or(0, |maps| maps.iter().flatten().map(texture_bytes).sum()),
            ),
            (
                "flat_draws",
                self.flat.as_ref().map_or(0, flat::Flat::bytes),
            ),
            (
                "visibility_buffer",
                self.visbuf.as_ref().map_or(0, visbuffer::Visbuffer::bytes),
            ),
        ]
    }
    /// Device memory the backend's allocator holds, by the label of what holds
    /// it, largest first, with its allocated and reserved totals (Vulkan and
    /// DirectX 12; none on Metal and OpenGL) (`horizon-ctl memory`).
    pub fn gpu_memory_by_label(&self) -> Option<(u64, u64, Vec<(String, u64)>)> {
        let report = self.device.generate_allocator_report()?;
        let mut by_label: HashMap<String, u64> = HashMap::new();
        for a in report.allocations {
            *by_label.entry(a.name).or_default() += a.size;
        }
        let mut labels: Vec<_> = by_label.into_iter().collect();
        labels.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Some((
            report.total_allocated_bytes,
            report.total_reserved_bytes,
            labels,
        ))
    }
    /// Heap bytes of the renderer's per-section bookkeeping (`horizon-ctl memory`):
    /// resident sections, far tiles, block entities and light sources by section.
    pub fn cpu_census(&self) -> [(&'static str, usize); 4] {
        use std::mem::size_of;
        let per_section = |map_len: usize, lists: usize| {
            map_len * (size_of::<SectionKey>() + size_of::<Vec<([i32; 3], u32)>>() + 1)
                + lists * size_of::<([i32; 3], u32)>()
        };
        let lists = |map: &crate::fast_hash::FxHashMap<SectionKey, Vec<([i32; 3], u32)>>| {
            per_section(
                map.capacity(),
                map.values().map(Vec::capacity).sum::<usize>(),
            )
        };
        let animated: usize = self
            .sections
            .values()
            .map(|s| s.animated_layers.len() * size_of::<u16>())
            .sum();
        [
            ("resident_sections", self.sections.heap_bytes() + animated),
            (
                "far_tiles",
                self.lods.capacity()
                    * (size_of::<crate::lod::TileKey>() + size_of::<lod::Tile>() + 1),
            ),
            ("block_entities", lists(&self.block_entities)),
            ("light_sources", lists(&self.light_blocks)),
        ]
    }
}
