//! Block particles: the sprite and tint of a block's faces and particle material, honey slides,
//! and the burst a destroyed block leaves (`ClientLevel.addDestroyBlockEffect`).
use super::*;

impl Game<'_> {
    /// The block's first up face (or any face) and its fixed tint, for block
    /// models drawn as entities (falling blocks, carried blocks).
    pub(super) fn block_face_sprite(&self, state: BlockStateId) -> Option<(String, u32)> {
        let pack = &self.pack;
        let models = pack
            .render
            .model_catalog
            .collect(state, horizon_core::position::BlockPos::new(0, 0, 0))
            .ok()?;
        let face = models
            .iter()
            .flat_map(|(_, m)| m.faces.iter())
            .find(|f| f.quad.facing == horizon_assets::model::Direction::Up)
            .or_else(|| models.iter().flat_map(|(_, m)| m.faces.iter()).next())?;
        let name = pack.layer_names.get(&face.layer)?.clone();
        let tint = if face.quad.tint_index >= 0 {
            let n = &self.res.facts().get(state).name;
            if n.contains("leaves") || n.contains("vine") {
                0x48B518
            } else {
                0x91BD59
            }
        } else {
            0xFFFFFF
        };
        Some((name, tint))
    }
    /// `TerrainParticle` at `pos`: the model's particle sprite and tint source
    /// 0's `colorAsTerrainParticle` (white when the block has none).
    /// `HoneyBlock.showSlideParticles` (entity event 53): five honey block
    /// particles at the entity.
    pub(super) fn honey_slide_particles(&mut self, at: [f64; 3]) {
        self.honey_entity_particles(at, 53);
    }
    /// `HoneyBlock.showParticles`: the crumb count of entity event 53 or 54.
    pub(super) fn honey_entity_particles(&mut self, at: [f64; 3], event: i8) {
        use horizon_client::particles::{ParticleOptions, ParticleType};
        let Some(count) = horizon_client::landing::honey_particle_count(event) else {
            return;
        };
        let Some(honey) = self
            .pack
            .assets
            .registry
            .default_state("minecraft:honey_block")
        else {
            return;
        };
        let block = [
            at[0].floor() as i32,
            at[1].floor() as i32,
            at[2].floor() as i32,
        ];
        if let Some((sprite, tint)) = self.particle_sprite(honey, block) {
            for _ in 0..count {
                self.particles.add(
                    &ParticleOptions::block(ParticleType::Block, sprite.clone(), tint),
                    at,
                    [0.0; 3],
                );
            }
        }
    }

    /// `BlockStateModelSet.getParticleMaterial`: the sprite of the model's
    /// `particle` texture, else its first face (entity-rendered blocks may
    /// have a particle texture without faces, and faces without one).
    pub(super) fn block_particle_sprite(
        pack: &PackResources,
        state: BlockStateId,
    ) -> Option<String> {
        let catalog = &pack.render.model_catalog;
        let layer = catalog.particle(state).or_else(|| {
            let models = catalog
                .collect(state, horizon_core::position::BlockPos::new(0, 0, 0))
                .ok()?;
            models
                .iter()
                .flat_map(|(_, m)| m.faces.iter())
                .next()
                .map(|f| f.layer)
        })?;
        pack.layer_names.get(&layer).cloned()
    }
    pub(super) fn particle_shape(&self, pos: [i32; 3], state: BlockStateId) -> Vec<[f64; 6]> {
        let f = self.res.facts().get(state);
        let offset = if f.outline_offset {
            self.pack
                .context
                .tables
                .offset(state, pos[0], pos[2])
                .unwrap_or([0.0; 3])
        } else {
            [0.0; 3]
        };
        f.outline
            .iter()
            .map(|b| std::array::from_fn(|i| b[i] + offset[i % 3]))
            .collect()
    }
    /// ClientLevel.addDestroyBlockEffect: a burst per outline box.
    pub(super) fn destroy_particles(&mut self, pos: [i32; 3], state: BlockStateId) {
        if self.settings.extras.hide_block_particles {
            return;
        }
        let Some((sprite, tint)) = self.particle_sprite(state, pos) else {
            return;
        };
        let boxes = self.particle_shape(pos, state);
        horizon_client::particles::destroy_positions(pos, &boxes, |p, v| {
            let particle = Particle::terrain(self.particles.random(), p, v, sprite.clone(), tint);
            self.particles.push(particle);
        });
    }
}
