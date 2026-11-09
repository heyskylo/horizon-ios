//! Biome writes for server mods: a helper on the level being ticked, as
//! `native_set_block` is, that sets biome cells (4x4x4 blocks) in loaded
//! chunks and sends the changed columns' biomes to the players who see them,
//! as `/fillbiome` does.
use super::super::Game;
use horizon_client::world::World;
use std::collections::{BTreeSet, HashMap};

/// Set `entries` (biome cell `[x, y, z]` in quarts, i.e. block coordinates
/// shifted right by 2, and the biome's table index) in `world`. Cells in
/// chunks not loaded, and cells above or below the column, are skipped; a
/// cell already holding the biome isn't a change. Returns how many cells
/// changed and the chunk columns they lie in (sorted by z, then x, the order
/// `/fillbiome` resends them in), marking those columns as modified.
fn write_cells(world: &mut World, entries: &[([i32; 3], u16)]) -> (usize, Vec<(i32, i32)>) {
    let bottom_quart = world.bottom_y() >> 2;
    let cells = world.layout().height() / 4;
    let mut changed = 0;
    let mut touched = BTreeSet::new();
    for &([qx, qy, qz], biome) in entries {
        let rel = qy - bottom_quart;
        if !(0..cells).contains(&rel) {
            continue;
        }
        let key = (qx >> 2, qz >> 2);
        let Some(chunk) = world.chunks.get_mut(&key) else {
            continue;
        };
        if chunk.biome(qx << 2, qy << 2, qz << 2) == biome {
            continue;
        }
        std::sync::Arc::make_mut(chunk).set_biome_cell(qx & 3, rel, qz & 3, biome);
        touched.insert((key.1, key.0));
        changed += 1;
    }
    let columns: Vec<(i32, i32)> = touched.into_iter().map(|(z, x)| (x, z)).collect();
    for &key in &columns {
        world.modified.insert(key);
        for sy in 0..world.layout().sections() as i32 {
            world.dirty_sections.insert((key.0, sy, key.1));
        }
    }
    (changed, columns)
}

/// The table index of each entry's biome id; an id the table doesn't have
/// refuses the whole call.
fn resolve_biomes(
    index: &HashMap<String, u16>,
    entries: &[([i32; 3], String)],
) -> Result<Vec<([i32; 3], u16)>, String> {
    entries
        .iter()
        .map(|(cell, id)| {
            index
                .get(id.as_str())
                .map(|&b| (*cell, b))
                .ok_or_else(|| format!("unknown biome {id:?}"))
        })
        .collect()
}

// Native mods use this; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// For a server mod: set the biome of each cell in `entries` (a cell
    /// `[x, y, z]` in quarts, a block coordinate shifted right by 2, and a
    /// biome id such as `minecraft:plains` or a data pack's own) in the
    /// authoritative level `dimension` (the one being ticked, as for
    /// `native_set_block`), and send the changed columns' biomes to the
    /// players who have them (`ChunkMap.resendBiomesForChunks`, as
    /// `/fillbiome` does). Cells in chunks not loaded, or outside the
    /// column's height, are left out. Returns the cells changed (a cell that
    /// already holds the biome isn't), or why nothing was: another dimension,
    /// a client of a server, or a biome id the level doesn't have (the whole
    /// call is refused, with nothing changed). API @API:set_biomes@.
    pub(in crate::game) fn native_set_biomes(
        &mut self,
        dimension: &str,
        entries: &[([i32; 3], String)],
    ) -> Result<usize, String> {
        if !self.native_level_open(dimension) {
            return Err(format!("{dimension} is not the level being ticked"));
        }
        let resolved = resolve_biomes(&world_biomes!(self).index, entries)?;
        let (changed, columns) = write_cells(&mut self.world, &resolved);
        if let Some(host) = self.lan.as_mut()
            && !columns.is_empty()
        {
            host.hub.resend_biomes(&self.world, &columns);
        }
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::chunk::Chunk;
    use horizon_world::registry::BlockStateId;
    use std::sync::Arc;

    fn world() -> World {
        let mut w = World::new(BlockStateId(0), 0);
        w.chunks.insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        w.chunks.insert((-1, 0), Arc::new(Chunk::empty(-1, 0)));
        w
    }

    #[test]
    fn a_biome_set_reads_back_and_marks_the_column() {
        let mut w = world();
        // quart (1, 3, 2) is blocks 4..8, 12..16, 8..12 of chunk (0, 0)
        let (n, columns) = write_cells(&mut w, &[([1, 3, 2], 7)]);
        assert_eq!((n, columns), (1, vec![(0, 0)]));
        let c = &w.chunks[&(0, 0)];
        assert_eq!(c.biome(4, 12, 8), 7);
        assert_eq!(c.biome(7, 15, 11), 7);
        assert_eq!(c.biome(8, 12, 8), 0, "the next cell stays");
        assert_eq!(c.biome(4, 16, 8), 0, "the cell above stays");
        assert!(w.modified.contains(&(0, 0)));
        // setting it again changes nothing
        assert_eq!(write_cells(&mut w, &[([1, 3, 2], 7)]).0, 0);
    }

    #[test]
    fn negative_cells_land_in_the_right_chunk_and_unloaded_ones_are_skipped() {
        let mut w = world();
        let below = w.bottom_y() >> 2;
        let (n, columns) = write_cells(
            &mut w,
            &[
                ([-1, 0, 0], 5),        // block x -4..0: chunk -1
                ([100, 0, 0], 5),       // chunk 25: not loaded
                ([0, below - 1, 0], 5), // under the column
            ],
        );
        assert_eq!((n, columns), (1, vec![(-1, 0)]));
        assert_eq!(w.chunks[&(-1, 0)].biome(-4, 0, 0), 5);
        assert_eq!(w.chunks[&(-1, 0)].biome(-5, 0, 0), 0);
    }

    #[test]
    fn an_unknown_biome_id_refuses_the_whole_call() {
        let index = HashMap::from([("minecraft:plains".to_string(), 1u16)]);
        let cell = |id: &str| ([0, 0, 0], id.to_string());
        assert_eq!(
            resolve_biomes(&index, &[cell("minecraft:plains")]),
            Ok(vec![([0, 0, 0], 1)])
        );
        let err =
            resolve_biomes(&index, &[cell("minecraft:plains"), cell("mymod:nope")]).unwrap_err();
        assert!(err.contains("mymod:nope"), "{err}");
    }
}
