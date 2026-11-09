//! Light worked out at once on the calling thread, for a caller that wrote
//! sections without a block edit, and the light of a `Job::Light` taken into
//! its chunk.
use super::Streaming;
use crate::{
    chunk::Light,
    light::{LightChanges, apply_lighting, apply_relighting, light_chunk},
    mesher::borrow,
    world::World,
};
use std::sync::Arc;

impl Streaming {
    /// Takes a lit column's light into its chunk (`Done::Lit`), `snapshot` being
    /// the edit sequence the blocks it was lit from had reached.
    pub(super) fn apply_lit(
        &mut self,
        world: &mut World,
        x: i32,
        z: i32,
        sections: Vec<Light>,
        snapshot: u64,
    ) {
        if let Some(chunk) = world.chunks.get_mut(&(x, z)) {
            let chunk = Arc::make_mut(chunk);
            // Keep blocks edited since the snapshot; take only light.
            // A chunk relit (a neighbour loaded or changed) remeshes
            // only around the light that changed, as vanilla's light
            // updates dirty only their sections (#270).
            let first = self.lit.insert((x, z));
            let mut changed = LightChanges::default();
            if first {
                apply_lighting(chunk, sections);
            } else {
                apply_relighting(chunk, sections, &mut changed);
            }
            // Saved unlit (at the edge of what was loaded), or its light
            // changed: saved again.
            if (first && !chunk.light_loaded) || !changed.is_empty() {
                world.modified.insert((x, z));
            }
            chunk.lit = true;
            if first {
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        for sy in 0..world.layout().sections() as i32 {
                            world.dirty_sections.insert((x + dx, sy, z + dz));
                        }
                    }
                }
            } else {
                Self::mark_light_changes(world, &changed);
            }
            self.replay_light_edits(world, x, z, snapshot);
        }
    }
    /// Lights `columns` again now, whole, from the blocks of each one's 3x3 as
    /// the light jobs `World::edit_runs` queues through `dirty_light` do, but
    /// at once and on this thread: for a caller that wrote sections without
    /// going through an edit and needs the light before it sends them. Columns
    /// not loaded, or without all eight neighbours loaded, are left out (a
    /// light job needs the region). A job already out for a column is
    /// dropped, so its older result is not taken after this one. The light
    /// that changed is logged for the players as any relight's is
    /// (`light_log`), and the sections remeshed. How many columns were lit.
    pub fn relight_now(&mut self, world: &mut World, columns: &[(i32, i32)]) -> usize {
        let mut lit = 0;
        for &(x, z) in columns {
            let Some(region) = Self::region(world, x, z) else {
                continue;
            };
            let sections = light_chunk(&borrow(&region), &self.light_tables, self.sky_light);
            self.lighting.remove(&(x, z));
            self.light_snapshots.remove(&(x, z));
            world.dirty_light.remove(&(x, z));
            self.apply_lit(world, x, z, sections, u64::MAX);
            lit += 1;
        }
        lit
    }
}
