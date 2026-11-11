//! The particles that tick on their own (`tick_simple`): the behaviours that read and write only
//! their own particle, the world and the sprite sets, which `ParticleEngine::tick_all` runs on several
//! threads in a busy scene.
use super::{Behaviour, Particle, ParticleEngine, ParticleLevel, Sprite, mod_curve};

/// The live particles it takes before `tick_all` uses more than one thread: a scoped thread costs
/// tens of microseconds to start and join, and a simple particle about a tenth of one.
pub(super) const PARALLEL_MIN: usize = 2048;
/// Threads for ticking particles: half the machine's workers, at most four (the chunk workers and
/// the render thread need cores too); `HORIZON_WORKER_THREADS` caps them as it does every pool.
pub(super) fn parallel_threads() -> usize {
    (horizon_threads::worker_count() / 2).clamp(1, 4)
}
pub(super) fn sprite_from_age_in(set_sprites: &[Vec<u32>], p: &mut Particle) {
    if p.removed {
        return;
    }
    if let Some(set) = p.set {
        // A sprite set's index can outlive the reload that rebuilt them.
        let Some(sprites) = set_sprites.get(set as usize) else {
            return;
        };
        let lifetime = p.lifetime.max(1);
        let index = if p.frame_ticks > 0 {
            (p.age.max(0) as i64 / i64::from(p.frame_ticks)) % sprites.len() as i64
        } else {
            (p.age.max(0) as i64 * (sprites.len() as i64 - 1) / i64::from(lifetime))
                .clamp(0, sprites.len() as i64 - 1)
        };
        p.sprite = Sprite::Index(sprites[index as usize]);
    }
}
/// The tick of the behaviours that read and write only their own particle, the world and the engine's
/// sprite sets: no particle spawned, no sound, nothing drawn from the engine's random. Those are the
/// bulk of a busy scene (flames, smoke, poofs, dust, crits), and `ParticleEngine::tick` runs them on
/// several threads. Any other behaviour is left alone, for `ParticleEngine::tick_particle`.
pub(super) fn tick_simple(
    set_sprites: &[Vec<u32>],
    p: &mut Particle,
    level: &ParticleLevel,
) -> Simple {
    match p.behaviour {
        Behaviour::Base => p.base_tick(level),
        Behaviour::Mod { size, alpha } => {
            p.base_tick(level);
            p.size = mod_curve(size, p.age, p.lifetime);
            p.rgba[3] = mod_curve(alpha, p.age, p.lifetime);
            sprite_from_age_in(set_sprites, p);
        }
        Behaviour::Animated => {
            p.base_tick(level);
            sprite_from_age_in(set_sprites, p);
        }
        Behaviour::Snowflake => {
            p.base_tick(level);
            sprite_from_age_in(set_sprites, p);
            p.velocity[0] *= f64::from(0.95f32);
            p.velocity[1] *= f64::from(0.9f32);
            p.velocity[2] *= f64::from(0.95f32);
        }
        Behaviour::Crit => {
            p.base_tick(level);
            p.rgba[1] *= 0.96;
            p.rgba[2] *= 0.9;
        }
        Behaviour::Spark { fade, trail, .. } => {
            p.base_tick(level);
            sprite_from_age_in(set_sprites, p);
            if p.age > p.lifetime / 2 {
                p.rgba[3] = 1.0 - (p.age as f32 - (p.lifetime / 2) as f32) / p.lifetime as f32;
                if let Some(f) = fade {
                    for i in 0..3 {
                        p.rgba[i] += (f[i] - p.rgba[i]) * 0.2;
                    }
                }
            }
            if trail && p.age < p.lifetime / 2 && (p.age + p.lifetime) % 2 == 0 {
                return Simple::SparkTrail;
            }
        }
        _ => return Simple::No,
    }
    Simple::Done
}
/// The same world with a one-chunk lookup cache of its own, for a worker thread.
///
/// `ParticleLevel::cached_chunk` is a `Cell`, which a level shared between threads may not hold:
/// each thread gets a view with an empty cache, built here on the engine thread and moved in. A
/// cache they shared could not hold "the last chunk looked up" for any one of them in any case.
fn thread_view<'a>(level: &ParticleLevel<'a>) -> ParticleLevel<'a> {
    ParticleLevel {
        world: level.world,
        facts: level.facts,
        collision: level.collision,
        player: level.player,
        cached_chunk: std::cell::Cell::new(None),
    }
}
/// What `tick_simple` did with a particle.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Simple {
    /// Not one of its behaviours: the engine ticks it.
    No,
    /// Ticked completely.
    Done,
    /// Ticked, except that a firework spark's trail wants a child. Only the engine can make one:
    /// it takes the next seed, so the children come in the order the sparks have.
    SparkTrail,
}
impl ParticleEngine {
    /// What `tick_simple` leaves to the engine: a spark's trail child.
    pub(super) fn finish_simple(&mut self, p: &Particle, simple: Simple) {
        if simple != Simple::SparkTrail {
            return;
        }
        if let Behaviour::Spark { fade, twinkle, .. } = p.behaviour {
            let mut child = self.spark(p.position, [0.0; 3], p.rgba, fade, false, twinkle);
            child.age = child.lifetime / 2;
            self.pending.push(child);
        }
    }
    /// One tick of every live particle. The ones `tick_simple` covers are independent of each
    /// other and of the engine, so with enough of them they tick on several threads; every other
    /// one then ticks here, in the order it had, so the particles, the engine's random and what
    /// is spawned come out as when all of them ticked one after another.
    pub(super) fn tick_all(&mut self, particles: &mut [Particle], level: &ParticleLevel) {
        if particles.is_empty() || particles.len() < self.parallel_min || self.parallel_threads < 2
        {
            for p in particles {
                self.tick_particle(p, level);
            }
            return;
        }
        let sets = &self.set_sprites;
        let per = particles.len().div_ceil(self.parallel_threads);
        let mut handled = vec![Simple::No; particles.len()];
        std::thread::scope(|scope| {
            let run = |part: &mut [Particle], done: &mut [Simple], level: &ParticleLevel| {
                for (p, done) in part.iter_mut().zip(done) {
                    *done = tick_simple(sets, p, level);
                }
            };
            let mut parts = particles.chunks_mut(per).zip(handled.chunks_mut(per));
            let own = parts.next();
            for (part, done) in parts {
                let view = thread_view(level);
                scope.spawn(move || run(part, done, &view));
            }
            // This thread takes the first share, instead of waiting for the others.
            if let Some((part, done)) = own {
                run(part, done, level);
            }
        });
        for (p, done) in particles.iter_mut().zip(handled) {
            match done {
                Simple::No => self.tick_particle(p, level),
                simple => self.finish_simple(p, simple),
            }
        }
    }
}
