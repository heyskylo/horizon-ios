//! Where particles come into the engine: a server's particle packet and a single particle made
//! where the game asks for one.
use super::*;

impl ParticleEngine {
    /// `ClientPacketListener.handleParticleEvent` for `ServerLevel.sendParticles`.
    pub fn send_particles(
        &mut self,
        options: &ParticleOptions,
        at: [f64; 3],
        spread: [f64; 3],
        speed: f64,
        count: u32,
    ) {
        if count == 0 {
            let v = [speed * spread[0], speed * spread[1], speed * spread[2]];
            self.add(options, at, v);
            return;
        }
        for _ in 0..count {
            let r = &mut self.random;
            let p = [
                at[0] + r.next_gaussian() * spread[0],
                at[1] + r.next_gaussian() * spread[1],
                at[2] + r.next_gaussian() * spread[2],
            ];
            let v = [
                r.next_gaussian() * speed,
                r.next_gaussian() * speed,
                r.next_gaussian() * speed,
            ];
            self.add(options, p, v);
        }
    }
    /// `ParticleEngine.createParticle`: always spawned, but `ParticleEngine.add`
    /// drops a spore blossom air particle once `ParticleLimit.SPORE_BLOSSOM` is full.
    pub fn create(&mut self, options: &ParticleOptions, p: [f64; 3], v: [f64; 3]) {
        if let Some(requests) = &mut self.created_requests {
            requests.push((options.clone(), p, v));
            return;
        }
        let seed = self.seeds.next_long();
        if let Some(mut particle) = self.make(options, p, v, seed) {
            *self.made.entry(options.ty.name()).or_default() += 1;
            // Resource packs' Polytone particle modifiers (#11486), once as it spawns.
            if let Some(registry) = &self.polytone
                && registry.modifies_particles()
            {
                let id = match &options.mod_particle {
                    Some(name) => name.clone(),
                    None => format!("minecraft:{}", options.ty.name()),
                };
                polytone::modify(registry, &id, options.source.as_deref(), &mut particle);
            }
            // The provider has drawn, so rejecting here keeps vanilla's RNG order.
            // shortcut: counts live plus queued particles by scanning instead of
            // vanilla's counted map; use a counter if spore spawning shows in profiles.
            if particle.ty == ParticleType::SporeBlossomAir
                && self.particles.len() + self.pending.len() >= SPORE_BLOSSOM_LIMIT
                && self
                    .particles
                    .iter()
                    .chain(self.pending.iter())
                    .filter(|q| q.ty == ParticleType::SporeBlossomAir)
                    .count()
                    >= SPORE_BLOSSOM_LIMIT
            {
                return;
            }
            self.pending.push(particle);
        }
    }
}
