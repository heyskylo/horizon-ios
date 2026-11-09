//! Sounds and particles in a level for native server mods, through a
//! `LevelRef` (docs/native-mods.md, "Entity and level handles", #7387).
//! Every player in range hears or sees them, as `ServerLevel.playSound` and
//! `sendParticles` send them. They return false on a client, for a handle of
//! an earlier open, or for arguments the network can't carry.
use super::native_hooks::{mod_particles_ok, mod_sound_ok};
use super::{Dim, Game, creatures::NativeSound, lan_particles::QueuedParticle};
use horizon_client::mod_api::{LevelRef, ParticleOptions};
use horizon_world::registry::BlockStateId;

impl Game<'_> {
    /// The level's dimension id, for a handle of this open on the server.
    fn native_level_dimension(&self, level: LevelRef) -> Option<&'static str> {
        self.native_level_world(level)?;
        [Dim::Overworld, Dim::Nether, Dim::End]
            .into_iter()
            .map(Dim::id)
            .find(|id| *id == level.dimension())
    }
    /// Plays sound event `sound` at `at` for the players in this level
    /// within range (16 blocks, times the volume above 1), as
    /// `ServerLevel.playSound(null, ...)` does: `source` indexes
    /// `SoundSource` (`master` 0, `blocks` 4...). A `minecraft:` event must be
    /// registered; a mod's goes by name and is heard where its resources
    /// define it. True means queued, not heard. API @API:level_effects@.
    pub(super) fn native_level_play_sound(
        &mut self,
        level: LevelRef,
        sound: &str,
        at: [f64; 3],
        source: usize,
        volume: f32,
        pitch: f32,
    ) -> bool {
        let Some(dimension) = self.native_level_dimension(level) else {
            return false;
        };
        if !mod_sound_ok(sound, at, source, volume, pitch) {
            return false;
        }
        self.creatures.lan_sounds.push(NativeSound {
            dimension,
            sound: sound.to_owned(),
            at,
            source: source as i32,
            volume,
            pitch,
        });
        true
    }
    /// Sends `count` particles around `at` to the players in this level
    /// within 32 blocks, as `ServerLevel.sendParticles` does: each offset is
    /// a Gaussian times `spread`, and `speed` scales their motion (a zero
    /// count sends one particle moving along `spread`). Block particle types
    /// need `state`; item, trail and vibration particles aren't supported.
    /// True means queued, not drawn. API @API:level_effects@.
    pub(super) fn native_level_send_particles(
        &mut self,
        level: LevelRef,
        options: &ParticleOptions,
        state: Option<BlockStateId>,
        at: [f64; 3],
        spread: [f64; 3],
        speed: f64,
        count: i32,
    ) -> bool {
        let Some(dimension) = self.native_level_dimension(level) else {
            return false;
        };
        let registry = self.pack.assets.registry;
        let state_ok = |id| registry.state(id).is_some();
        if !mod_particles_ok(options, state, state_ok, at, spread, speed) {
            return false;
        }
        self.creatures.lan_particles.push(QueuedParticle {
            dimension,
            options: options.clone(),
            state,
            at,
            spread,
            speed: [speed; 3],
            count,
            randomization: 0,
        });
        true
    }
}
