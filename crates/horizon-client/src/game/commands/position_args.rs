//! Block position argument validation for command execution.
use super::{Context, Game, R, err};

impl Game<'_> {
    /// BlockPosArgument.getLoadedBlockPos.
    pub(in crate::game) fn block_pos_arg(&self, ctx: &Context, name: &str) -> R<[i32; 3]> {
        let c = self
            .coords(ctx, name)
            .ok_or_else(|| err("argument.pos.missing.int", &[]))?;
        // BlockPos.containing(getPosition)
        let p = self.resolve_coords(&c).map(|v| v.floor() as i32);
        let dimension = self.command_source.map_or(self.dim, |s| s.dimension);
        if dimension != self.dim || !self.world.chunks.contains_key(&(p[0] >> 4, p[2] >> 4)) {
            return Err(err("argument.pos.unloaded", &[]));
        }
        if p[1] < self.world.bottom_y() || p[1] >= self.world.top_y() {
            return Err(err("argument.pos.outofworld", &[]));
        }
        Ok(p)
    }
    /// BlockPosArgument.getSpawnablePos: no loaded-chunk or build-height check,
    /// only `Level.isInSpawnableBounds` (upper bounds exclusive).
    pub(in crate::game) fn spawnable_pos_arg(&self, ctx: &Context, name: &str) -> R<[i32; 3]> {
        let c = self
            .coords(ctx, name)
            .ok_or_else(|| err("argument.pos.missing.int", &[]))?;
        let p = self.resolve_coords(&c).map(|v| v.floor() as i32);
        if !(-30_000_000..30_000_000).contains(&p[0])
            || !(-30_000_000..30_000_000).contains(&p[2])
            || !(-20_000_000..20_000_000).contains(&p[1])
        {
            return Err(err("argument.pos.outofbounds", &[]));
        }
        Ok(p)
    }
}
