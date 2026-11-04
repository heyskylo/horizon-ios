//! Integer coordinates and vanilla's packed position formats.
mod iter;
pub use iter::{BlockBoxIter, BlockIterationTooLarge};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// Values outside the signed 26/12/26-bit ranges are truncated as in vanilla.
    pub const fn pack(self) -> i64 {
        (((self.x as u64 & 0x3ff_ffff) << 38)
            | ((self.z as u64 & 0x3ff_ffff) << 12)
            | (self.y as u64 & 0xfff)) as i64
    }

    pub const fn unpack(value: i64) -> Self {
        Self::new(
            (value >> 38) as i32,
            (value << 52 >> 52) as i32,
            (value << 26 >> 38) as i32,
        )
    }

    #[inline]
    pub const fn offset(self, dx: i32, dy: i32, dz: i32) -> Self {
        Self::new(
            self.x.wrapping_add(dx),
            self.y.wrapping_add(dy),
            self.z.wrapping_add(dz),
        )
    }

    pub const fn section(self) -> SectionPos {
        // Arithmetic shift floors negative coordinates; integer / 16 does not.
        SectionPos::new(self.x >> 4, self.y >> 4, self.z >> 4)
    }

    #[inline]
    pub const fn chunk(self) -> ChunkPos {
        ChunkPos::new(self.x >> 4, self.z >> 4)
    }

    /// Section update packet coordinate: X in bits 8..11, Z in 4..7, Y in 0..3.
    pub const fn section_relative(self) -> u16 {
        (((self.x & 15) << 8) | ((self.z & 15) << 4) | (self.y & 15)) as u16
    }

    /// Block-state container index. This is a different layout from packets.
    pub const fn section_index(self) -> usize {
        (((self.y & 15) << 8) | ((self.z & 15) << 4) | (self.x & 15)) as usize
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SectionPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl SectionPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    pub const fn pack(self) -> i64 {
        (((self.x as u64 & 0x3f_ffff) << 42)
            | ((self.z as u64 & 0x3f_ffff) << 20)
            | (self.y as u64 & 0xf_ffff)) as i64
    }

    pub const fn unpack(value: i64) -> Self {
        Self::new(
            (value >> 42) as i32,
            (value << 44 >> 44) as i32,
            (value << 22 >> 42) as i32,
        )
    }

    pub const fn relative_to_block(self, relative: u16) -> BlockPos {
        BlockPos::new(
            self.x
                .wrapping_shl(4)
                .wrapping_add(((relative >> 8) & 15) as i32),
            self.y.wrapping_shl(4).wrapping_add((relative & 15) as i32),
            self.z
                .wrapping_shl(4)
                .wrapping_add(((relative >> 4) & 15) as i32),
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ChunkPos {
    pub x: i32,
    pub z: i32,
}

impl ChunkPos {
    pub const fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    pub const fn pack(self) -> i64 {
        ((self.x as u32 as u64) | ((self.z as u32 as u64) << 32)) as i64
    }

    pub const fn unpack(value: i64) -> Self {
        Self::new(value as i32, (value >> 32) as i32)
    }

    pub const fn region(self) -> Self {
        Self::new(self.x >> 5, self.z >> 5)
    }

    pub const fn region_local(self) -> (u8, u8) {
        ((self.x & 31) as u8, (self.z & 31) as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_boundaries_use_floor_division() {
        for (block, section, local) in [
            (-17, -2, 15),
            (-16, -1, 0),
            (-1, -1, 15),
            (0, 0, 0),
            (16, 1, 0),
        ] {
            let p = BlockPos::new(block, block, block);
            assert_eq!(p.section(), SectionPos::new(section, section, section));
            assert_eq!(p.section_relative(), local * 0x111);
            assert_eq!(p.section().relative_to_block(p.section_relative()), p);
        }
        assert_eq!(ChunkPos::new(-1, -33).region(), ChunkPos::new(-1, -2));
        assert_eq!(ChunkPos::new(-1, -33).region_local(), (31, 31));
    }

    #[test]
    fn coordinate_formats_are_distinct_and_bijective_in_range() {
        let mut packet_indices = [false; 4096];
        let mut storage_indices = [false; 4096];
        for y in 0..16 {
            for z in 0..16 {
                for x in 0..16 {
                    let p = BlockPos::new(x, y, z);
                    packet_indices[p.section_relative() as usize] = true;
                    storage_indices[p.section_index()] = true;
                    assert_eq!(
                        SectionPos::default().relative_to_block(p.section_relative()),
                        p
                    );
                }
            }
        }
        assert!(packet_indices.into_iter().all(|x| x));
        assert!(storage_indices.into_iter().all(|x| x));
        assert_ne!(
            BlockPos::new(1, 2, 3).section_relative() as usize,
            BlockPos::new(1, 2, 3).section_index()
        );
    }

    #[test]
    fn packed_signed_limits_and_java_overflow() {
        let block = BlockPos::new(-33_554_432, -2048, 33_554_431);
        assert_eq!(BlockPos::unpack(block.pack()), block);
        let section = SectionPos::new(-2_097_152, -524_288, 2_097_151);
        assert_eq!(SectionPos::unpack(section.pack()), section);
        let chunk = ChunkPos::new(i32::MIN, i32::MAX);
        assert_eq!(ChunkPos::unpack(chunk.pack()), chunk);
        assert_eq!(BlockPos::new(i32::MAX, 0, 0).offset(1, 0, 0).x, i32::MIN);
    }
}
