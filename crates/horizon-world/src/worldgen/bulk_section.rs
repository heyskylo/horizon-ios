//! Exclusive bulk access to resident sections, bypassing chunk heightmap writes.
use super::{
    feature_region::{RegionError, TerrainRegion},
    placement::{PlacementContext, PlacementQueryError},
    terrain::{TerrainBlockStates, TerrainError},
};
use crate::{heightmap::HeightmapType, registry::BlockStateId};
use horizon_core::position::{BlockPos, SectionPos};
use std::collections::BTreeMap;

pub struct TerrainBulkAccess<'a, 'r, B: TerrainBlockStates> {
    region: &'a mut TerrainRegion<'r, B>,
    min_section: i32,
    section_count: i32,
    air: BlockStateId,
    acquired: BTreeMap<i64, SectionPos>,
    last: Option<(i64, SectionPos)>,
}
impl<'a, 'r, B: TerrainBlockStates> TerrainBulkAccess<'a, 'r, B> {
    pub fn new(region: &'a mut TerrainRegion<'r, B>) -> Result<Self, RegionError> {
        let first = region.chunks().next().ok_or(RegionError::InvalidChunks)?;
        let min_section = first.min_y() >> 4;
        let section_count = first.height() / 16;
        let air = first
            .registry()
            .default_state("minecraft:air")
            .ok_or(RegionError::InvalidChunks)?;
        Ok(Self {
            region,
            min_section,
            section_count,
            air,
            acquired: BTreeMap::new(),
            last: None,
        })
    }
    fn resolve(&mut self, p: [i32; 3]) -> Result<Option<SectionPos>, RegionError> {
        let section = BlockPos::new(p[0], p[1], p[2]).section();
        let index = section.y.wrapping_sub(self.min_section);
        if index < 0 || index >= self.section_count {
            return Ok(None);
        }
        let key = section.pack();
        if let Some((last, section)) = self.last
            && last == key
        {
            return Ok(Some(section));
        }
        let cached = if let Some(cached) = self.acquired.get(&key) {
            *cached
        } else {
            // Missing chunks do not enter the cache. Preserve packed-key aliasing
            // at coordinates beyond the signed section-coordinate bit width.
            self.region.chunk(section.x, section.z)?;
            self.acquired.insert(key, section);
            section
        };
        self.last = Some((key, cached));
        Ok(Some(cached))
    }
    pub fn has_section(&mut self, position: [i32; 3]) -> Result<bool, RegionError> {
        Ok(self.resolve(position)?.is_some())
    }
    pub fn acquired_section_count(&self) -> usize {
        self.acquired.len()
    }
    /// Direct section reads retain cave/void air in otherwise all-air sections.
    /// Positions outside the vertical range return AIR without a chunk lookup.
    pub fn block(&mut self, position: [i32; 3]) -> Result<BlockStateId, RegionError> {
        let Some(section) = self.resolve(position)? else {
            return Ok(self.air);
        };
        let index = section.y.wrapping_sub(self.min_section) as usize;
        let p = BlockPos::new(position[0], position[1], position[2]);
        self.region
            .chunk(section.x, section.z)?
            .section(index)
            .ok_or(RegionError::InvalidChunks)?
            .get(p.section_index())
            .map_err(|e| TerrainError::from(e).into())
    }
    /// Raw section mutation: no chunk heightmap, POI, light or neighbor effects.
    /// The caller is responsible for checking the region's write permissions.
    pub fn set_block(
        &mut self,
        position: [i32; 3],
        state: BlockStateId,
    ) -> Result<Option<BlockStateId>, RegionError> {
        let Some(section) = self.resolve(position)? else {
            return Ok(None);
        };
        let y = section.y.wrapping_shl(4).wrapping_add(position[1] & 15);
        Ok(Some(
            self.region.chunk_mut(section.x, section.z)?.set_block_raw(
                position[0],
                y,
                position[2],
                state,
            )?,
        ))
    }
    pub fn world_block(&self, position: [i32; 3]) -> Result<BlockStateId, RegionError> {
        self.region.block(position)
    }
    pub fn height(
        &mut self,
        kind: HeightmapType,
        x: i32,
        z: i32,
    ) -> Result<i32, PlacementQueryError> {
        self.region.height(kind, x, z)
    }
}
