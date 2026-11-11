//! Chunk-batch selection: the normal view first, then a bounded scan through the saved far ring.
use super::*;

impl Hub {
    /// PlayerChunkSender.sendNextChunks' choice: ready chunks, closest first,
    /// as many as the batch quota the client's acknowledgements pace allows,
    /// with their places in the queue. What the view leaves of the quota goes
    /// to the far ring, as far as its brake lets it reach. A memory connection
    /// that has not loaded yet sends every ready view column in that one
    /// batch; the quota recorded for it covers them, so the tick after
    /// `PlayerLoaded` is capped again.
    pub(super) fn next_batch<'w>(
        &mut self,
        i: usize,
        world: &'w World,
        streaming: Option<&crate::streaming::Streaming>,
    ) -> Option<Batch<'w>> {
        let g = &mut self.guests[i];
        if self.ticks <= g.placed_at + 1 || g.unacknowledged >= g.max_unacknowledged {
            return None;
        }
        g.batch_quota = (g.batch_quota + g.chunks_per_tick).min(g.chunks_per_tick.max(1.0));
        if g.batch_quota < 1.0 || (g.chunk_queue.is_empty() && g.far_queue.is_empty()) {
            return None;
        }
        let budget = g.batch_quota.floor() as usize;
        // The integrated server's own player, still on the loading screen.
        // LAN guests and a client that has loaded keep the quota above.
        let burst = g.address == super::super::conn::MEMORY_ADDRESS && !g.loaded;
        let near_take = if burst { g.chunk_queue.len() } else { budget };
        // ChunkStatus.LIGHT needs its neighbours at INITIALIZE_LIGHT: a
        // column whose light layers would leave one out waits for it.
        let ready = |c: &Chunk| {
            c.lit
                && streaming.is_none_or(|streaming| {
                    (-1..=1).all(|dz| {
                        (-1..=1).all(|dx| {
                            let (x, z) = (c.x + dx, c.z + dz);
                            world.chunks.contains_key(&(x, z))
                                || streaming.proto_sections(x, z).is_some()
                        })
                    })
                })
        };
        let near: Vec<_> = g
            .chunk_queue
            .iter()
            .enumerate()
            .filter_map(|(idx, key)| Some((idx, &**world.chunks.get(key)?)))
            .filter(|(_, c)| ready(c))
            .take(near_take)
            .collect();
        let mut far = Vec::new();
        if let (Some(ring), Some((cx, cz))) = (self.far.as_mut(), g.center) {
            // What the view left of the vanilla quota. A first-view burst can take more than that.
            let mut left = budget.saturating_sub(near.len());
            let len = g.far_queue.len();
            let start = g.far_cursor % len.max(1);
            let mut looked = 0;
            for offset in 0..len.min(super::super::far::LOOKS) {
                if left == 0 {
                    break;
                }
                let idx = (start + offset) % len;
                let (x, z) = g.far_queue[idx];
                looked += 1;
                if !tracked(cx, cz, ring.limit(), x, z) {
                    continue;
                }
                let column = match world.chunks.get(&(x, z)) {
                    // Far columns carry all their own saved light layers. They need no
                    // neighbour views, including at the edge of the level's loading buffer.
                    Some(c) if c.lit => Column::Level(c),
                    _ => match ring.saved(x, z) {
                        super::super::far::Saved::Body(body) => Column::Saved(body),
                        super::super::far::Saved::Missing | super::super::far::Saved::Waiting => {
                            continue;
                        }
                    },
                };
                left -= 1;
                far.push((idx, column));
            }
            g.far_cursor = (start + looked) % len.max(1);
            // Removal after sending runs backwards through the queue, even when the scan wrapped.
            far.sort_unstable_by_key(|(idx, _)| *idx);
        }
        // `update_chunks` subtracts the batch. Covering the near columns keeps that from going
        // negative, so the next tick's `.min(chunks_per_tick)` cap holds again.
        if burst {
            g.batch_quota = g.batch_quota.max(near.len() as f32);
        }
        (!near.is_empty() || !far.is_empty()).then_some(Batch { near, far })
    }
}
