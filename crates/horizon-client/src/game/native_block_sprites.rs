//! Where blocks' textures are in the block sprite arrays, for a client mod drawing blocks with the player's own
//! textures (resource packs included) in its own pass: with `WorldDrawn::block_sprites`, the arrays themselves.
use super::Game;
use horizon_client::block_sprites::{BlockSprites, Lookup, SpriteRef};
use horizon_core::geometry::Direction;
use horizon_world::registry::BlockStateId;

#[allow(dead_code)]
impl Game<'_> {
    /// For a client mod: what each block (`minecraft:grass_block`, `oak_log[axis=x]`; properties left out take the
    /// block's default) draws on each of its six sides, as the mesher bakes its model: each face's sprite (array and
    /// layer, as `WorldDrawn::block_sprites` cuts them), where it lies on the side, how its texture runs across it,
    /// its tint source and whether it is cut out, translucent or animated; its particle sprite; whether it is a plain
    /// cube. None for a block the game does not know. The model is the state's first random variant. API
    /// 380.
    pub(super) fn native_block_sprites(&self, blocks: &[&str]) -> Vec<Option<BlockSprites>> {
        let lookup = self.native_sprite_lookup();
        blocks.iter().map(|block| lookup.block(block)).collect()
    }
    /// For a client mod: each texture's sprite by name (`minecraft:block/stone`, `block/stone`), none for one no
    /// model uses. API 380.
    pub(super) fn native_texture_sprites(&self, textures: &[&str]) -> Vec<Option<SpriteRef>> {
        let lookup = self.native_sprite_lookup();
        textures
            .iter()
            .map(|texture| lookup.texture(texture))
            .collect()
    }
    /// For a client mod drawing a block entity: the texture block state `state`'s model shows on its `side`
    /// (`block/oak_planks`: vanilla's without their namespace, a mod's with it), by the name `native_draw_cube` and
    /// `native_draw_quad` take, and that face's
    /// tint index (-1 for none): a copycat block drawing a box of its material's textures with `native_draw_cube`,
    /// kept with `BlockEntityDraw::keep`. The face culled against that side comes first, else the first facing it;
    /// None when the model has no face that way or isn't baked. API @API:block_face_sprite@.
    pub(super) fn native_block_face_sprite(
        &self,
        state: BlockStateId,
        side: Direction,
    ) -> Option<(String, i32)> {
        let wanted = horizon_assets::model::Direction::ALL[side as usize];
        let models = self
            .pack
            .render
            .model_catalog
            .collect_seeded(state, 42)
            .ok()?;
        let faces = || models.iter().flat_map(|(_, model)| &model.faces);
        let face = faces()
            .find(|f| f.quad.cull == Some(wanted))
            .or_else(|| faces().find(|f| f.quad.facing == wanted))?;
        let name = self.pack.layer_names.get(&face.layer)?.clone();
        Some((name, face.quad.tint_index))
    }
    /// For a client mod: changes each time the resources load (a resource pack turned on or off can move every
    /// sprite): ask `native_block_sprites` again then. API 380.
    pub(super) fn native_block_sprites_version(&self) -> u64 {
        self.pack.context.render.generation
    }
    fn native_sprite_lookup(&self) -> Lookup<'_> {
        // (no renderer, no arrays: every layer counts as array 0's)
        let split = self
            .renderer
            .as_ref()
            .map_or(u32::MAX, |r| r.sprite_split());
        Lookup::new(&self.pack.context, split)
    }
}
