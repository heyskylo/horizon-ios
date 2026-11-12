//! What the streamer's queues hold, for `horizon-ctl memory` and captures.
use super::{Queued, Streaming};

/// What the streamer's work queues hold, each as (entries, estimated bytes).
#[derive(Debug, Default)]
pub struct QueueCensus {
    /// Jobs queued and chunks generating.
    pub generation: (usize, usize),
    /// Chunks being lit and the edit log their jobs replay.
    pub light: (usize, usize),
}

impl Streaming {
    /// The queues' lengths and the bytes of their tables, from capacities. For `horizon-ctl memory`.
    pub fn queue_census(&self) -> QueueCensus {
        use std::mem::size_of;
        let table = |capacity: usize, entry: usize| capacity * (entry + 1);
        let queued = {
            let queue = self.shared.queue.lock().unwrap();
            (queue.len(), queue.capacity() * size_of::<Queued>())
        };
        QueueCensus {
            generation: (
                queued.0 + self.generating.len(),
                queued.1 + table(self.generating.capacity(), size_of::<(i32, i32)>()),
            ),
            light: (
                self.lighting.len(),
                table(self.lighting.capacity(), size_of::<((i32, i32), u64)>())
                    + table(
                        self.light_snapshots.capacity(),
                        size_of::<((i32, i32), u64)>(),
                    )
                    + self.light_log.capacity() * size_of::<(u64, (i32, i32, i32))>()
                    + self.light_scratch.capacity() * size_of::<(i64, i32, i32)>(),
            ),
        }
    }
}
