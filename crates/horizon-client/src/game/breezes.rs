//! Breeze particles (`Breeze.tick`): block dust at its feet while it stands,
//! a burst of twenty while it slides and a short trail along a long jump.
use super::{BlockStateId, Game};
use horizon_client::{
    blocks::RenderShape,
    mobs::{Mob, Species, breeze},
    particles::{ParticleOptions, ParticleType},
};

impl Game<'_> {
    pub(super) fn tick_breeze_effects(&mut self) {
        let Some(camera) = self.player.as_ref().map(|p| p.position) else {
            return;
        };
        for i in 0..self.mobs.len() {
            let m = &self.mobs[i];
            if m.species() != Species::Breeze
                || !m.alive()
                || m.position.distance_squared(camera) > 32.0 * 32.0
            {
                continue;
            }
            let pose = breeze::pose(m);
            let (amount, jump_trail) = match pose {
                breeze::SLIDING => (20, false),
                breeze::LONG_JUMPING => (3, true),
                _ => (1, false),
            };
            if jump_trail {
                self.mobs[i].extra.scratch[1] += 1;
                if self.mobs[i].extra.scratch[1] > 5 {
                    continue;
                }
            } else {
                self.mobs[i].extra.scratch[1] = 0;
            }
            let (block, state) = self.breeze_ground(&self.mobs[i]);
            // `emitGroundParticles` skips an invisible block; the jump trail
            // (`emitJumpTrailParticles`) does not ask.
            if !jump_trail && self.res.facts().get(state).render_shape == RenderShape::Invisible {
                continue;
            }
            let Some((sprite, tint)) = self.particle_sprite(state, block) else {
                continue;
            };
            let m = &self.mobs[i];
            let at = if jump_trail {
                [
                    m.position.x + m.velocity.x,
                    m.position.y + m.velocity.y + f64::from(0.1_f32),
                    m.position.z + m.velocity.z,
                ]
            } else {
                [m.position.x, m.position.y, m.position.z]
            };
            // `ClientLevel.addParticle` with no speed: each crumb's own
            // generator draws its lifetime, its little push and the quarter of
            // the block's sprite it shows, never the level's random.
            let options = ParticleOptions::block(ParticleType::Block, sprite, tint);
            for _ in 0..amount {
                self.particles.add(&options, at, [0.0; 3]);
            }
        }
    }

    /// `Breeze.emitGroundParticles`'s block: `getInBlockState`, else
    /// `getBlockStateOn`, which is `Entity.getOnPos(1.0E-5F)` — the supporting
    /// block the feet rest on, or the cell just below them. A fence, a wall and
    /// a fence gate have a 1.5-block collision, so a breeze on one has its feet
    /// half a block above that block's own cell, where the cell below them is
    /// air and only the supporting block has the material.
    pub(super) fn breeze_ground(&self, m: &Mob) -> ([i32; 3], BlockStateId) {
        let feet = [
            m.position.x.floor() as i32,
            m.position.y.floor() as i32,
            m.position.z.floor() as i32,
        ];
        let in_block = self.world.block(feet[0], feet[1], feet[2]);
        if !self.res.facts().is_air(in_block) {
            return (feet, in_block);
        }
        let on_ground = m
            .on_ground
            .then(|| self.supporting_block(m.position, m.bounds()));
        let at = on_ground.flatten().map_or(
            [
                feet[0],
                (m.position.y - f64::from(1.0e-5_f32)).floor() as i32,
                feet[2],
            ],
            |p| [p.x, p.y, p.z],
        );
        (at, self.world.block(at[0], at[1], at[2]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{Audio, Options, Progress, Settings, load_resources};
    use horizon_client::chunk::Chunk;
    use horizon_client::mobs::Mob;
    use horizon_core::geometry::Vec3;
    use horizon_core::random::LegacyRandom;
    use std::sync::Arc;

    /// A game on the prepared resources with an empty chunk at (0, 0), the
    /// window's player (the camera the dust needs) and one breeze standing
    /// with its feet at `feet`.
    fn game_with_breeze(feet: Vec3) -> Game<'static> {
        let root = std::path::PathBuf::from(
            std::env::var_os("HORIZON_TEST_RESOURCES")
                .expect("set HORIZON_TEST_RESOURCES to prepared resources"),
        );
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            false,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let res: &'static _ = Box::leak(Box::new(loaded.resources));
        let mut game = Game::new(
            res,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        // Eight blocks south of the breeze: inside the 32 blocks the dust needs.
        game.player = Some(
            horizon_client::motion::PlayerMotion::new(game.motion, Vec3::new(8.5, 66.0, 16.5))
                .unwrap(),
        );
        game.particles.camera = [8.5, 66.0, 16.5];
        let kind = game.res.mob_types()["minecraft:breeze"].clone();
        game.mobs.push(Mob::new(13001, kind, feet, 0.0));
        game
    }

    /// A breeze standing on a fence rests its feet at 64 + 1.5, where its own
    /// cell is air: `Breeze.emitGroundParticles` asks `getBlockStateOn` for the
    /// supporting block, so the crumbs are the fence's. Every one of them is
    /// made by the particle's own generator (`Particle.random`), so the
    /// client level's random is where it was before the tick.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn the_dust_of_a_breeze_on_a_fence_keeps_the_client_level_random() {
        let mut game = game_with_breeze(Vec3::new(8.5, 65.5, 8.5));
        let registry = &game.pack.assets.registry;
        let stone = registry.default_state("minecraft:stone").unwrap();
        let fence = registry.default_state("minecraft:oak_fence").unwrap();
        game.world.set(8, 63, 8, stone).unwrap();
        game.world.set(8, 64, 8, fence).unwrap();
        game.mobs[0].on_ground = true;
        assert_eq!(
            game.breeze_ground(&game.mobs[0]),
            ([8, 64, 8], fence),
            "the feet's own cell is air; the fence is what holds them up"
        );
        game.world.random = LegacyRandom::new(1234);
        let level = game.world.random.state();

        game.tick_breeze_effects();

        assert_eq!(
            game.particles.made().get("block").copied().unwrap_or(0),
            1,
            "vanilla emits one crumb of the fence the breeze stands on"
        );
        assert_eq!(
            game.world.random.state(),
            level,
            "the crumb's constructor draws belong to the particle, not the level"
        );
    }

    /// The control: a breeze standing on a full block's top also crumbles it,
    /// and its constructor's draws still leave the client level's random alone.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn the_dust_of_a_breeze_on_a_full_block_keeps_the_client_level_random() {
        let mut game = game_with_breeze(Vec3::new(8.5, 64.0, 8.5));
        let stone = game
            .pack
            .assets
            .registry
            .default_state("minecraft:stone")
            .unwrap();
        game.world.set(8, 63, 8, stone).unwrap();
        game.mobs[0].on_ground = true;
        game.world.random = LegacyRandom::new(1234);
        let level = game.world.random.state();

        game.tick_breeze_effects();

        assert_eq!(
            game.world.random.state(),
            level,
            "the crumb's constructor draws belong to the particle, not the level"
        );
        assert_eq!(game.particles.made().get("block").copied().unwrap_or(0), 1);
    }

    /// `Entity.getOnPos(1.0E-5F)` keeps a fence's, a wall's or a fence gate's
    /// own cell while the feet rest on its 1.5-block collision; on a full block
    /// it is the block below the feet. Reading 0.2 below the feet, as the dust
    /// used to, lands in air on all three.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn the_ground_of_a_breeze_is_the_fence_wall_or_gate_it_stands_on() {
        let mut game = game_with_breeze(Vec3::new(8.5, 65.5, 8.5));
        let registry = &game.pack.assets.registry;
        let stone = registry.default_state("minecraft:stone").unwrap();
        game.world.set(8, 63, 8, stone).unwrap();
        game.mobs[0].on_ground = true;
        for name in [
            "minecraft:oak_fence",
            "minecraft:cobblestone_wall",
            "minecraft:oak_fence_gate",
        ] {
            let state = registry.default_state(name).unwrap();
            game.world.set(8, 64, 8, state).unwrap();
            assert_eq!(
                game.breeze_ground(&game.mobs[0]),
                ([8, 64, 8], state),
                "{name}"
            );
        }
        // The same breeze on the full block's top reads that block.
        game.world.set(8, 64, 8, stone).unwrap();
        game.mobs[0].position = Vec3::new(8.5, 65.0, 8.5);
        assert_eq!(game.breeze_ground(&game.mobs[0]), ([8, 64, 8], stone));
    }
}
