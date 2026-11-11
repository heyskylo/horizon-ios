//! Backpressure for asynchronous world saves.
use super::*;

const MAX_ASYNC_CHUNK_WRITES: usize = 512;

impl SaveDir {
    pub(super) fn encoding_for_save_later(
        &self,
        chunks: &[Arc<Chunk>],
        registry: &'static BlockRegistry,
        biomes: &Arc<Biomes>,
    ) -> Result<Arc<Encoding>> {
        if !chunks.is_empty() && self.active_chunk_writes() >= MAX_ASYNC_CHUNK_WRITES {
            self.flush()?;
        }
        Ok(Arc::new(Encoding {
            registry,
            biomes: biomes.clone(),
            tables: self.tables.get().cloned(),
            shape: self.shape,
        }))
    }
}
