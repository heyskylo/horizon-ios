//! Local particle requests from native client hooks.
use super::Game;
use horizon_client::particles::ParticleOptions;
use horizon_items::firework::FireworkExplosion;
use horizon_protocol::entity_data::PositionSource;

fn finite_options(options: &ParticleOptions) -> bool {
    options.color.into_iter().all(f32::is_finite)
        && options.to_color.into_iter().all(f32::is_finite)
        && options.scale.is_finite()
        && options
            .trail
            .as_ref()
            .is_none_or(|(at, _)| at.iter().all(|v| v.is_finite()))
        && options
            .geyser
            .is_none_or(|(_, impulse)| impulse.is_finite())
        && options.vibration.as_ref().is_none_or(|(source, at, _)| {
            at.iter().all(|v| v.is_finite())
                && match source {
                    PositionSource::Block(_) => true,
                    PositionSource::Entity { y_offset, .. } => y_offset.is_finite(),
                }
        })
}

/// The most explosions one `native_client_fireworks` call draws (a crafted
/// rocket holds at most 7 stars; a mod's show can be larger).
pub(crate) const MAX_CLIENT_FIREWORK_EXPLOSIONS: usize = 64;

/// Finite numbers and at most [`MAX_CLIENT_FIREWORK_EXPLOSIONS`] explosions.
fn client_fireworks_ok(at: [f64; 3], velocity: [f64; 3], explosions: &[FireworkExplosion]) -> bool {
    at.into_iter().chain(velocity).all(f64::is_finite)
        && explosions.len() <= MAX_CLIENT_FIREWORK_EXPLOSIONS
}

impl Game<'_> {
    /// `ClientLevel.addParticle`: request one particle in this client's loaded world.
    /// Options and provider input slots pass through unchanged, including encoded
    /// color or size values in `velocity`. Normal distance, settings and population
    /// limits still apply. True means submitted, not necessarily visible: an unknown
    /// provider or missing sprite set draws nothing. False on a server, without a
    /// client player/level, or for non-finite numeric input. API @API:client_particles@.
    pub(super) fn native_add_particle(
        &mut self,
        options: &ParticleOptions,
        at: [f64; 3],
        velocity: [f64; 3],
    ) -> bool {
        if !self.native_window()
            || self.player.is_none()
            || !at.into_iter().all(f64::is_finite)
            || !velocity.into_iter().all(f64::is_finite)
            || !finite_options(options)
        {
            return false;
        }
        self.particles.add(options, at, velocity);
        true
    }
    /// `ClientLevel.createFireworks`: a firework's bursts at `at` in this
    /// client's world, as a rocket's explosion draws them (the client-only
    /// `FireworkParticles.Starter`): each explosion's shape, colours, fade
    /// colours, trail and twinkle, one after another, with the blast and
    /// twinkle sounds; no explosions puffs a little smoke, as vanilla's. Only
    /// the picture and the sound: no entity, no damage, nothing on the
    /// server, and other players see nothing (a mod's server half tells each
    /// client half to draw it). `velocity` is the rocket's, carried by the
    /// sparks. True means queued; false on a server, without a client player
    /// or level, for non-finite input and for more than
    /// [`MAX_CLIENT_FIREWORK_EXPLOSIONS`] explosions. API
    /// @API:client_fireworks@.
    pub(super) fn native_client_fireworks(
        &mut self,
        at: [f64; 3],
        velocity: [f64; 3],
        explosions: &[FireworkExplosion],
    ) -> bool {
        if !self.native_window()
            || self.player.is_none()
            || !client_fireworks_ok(at, velocity, explosions)
        {
            return false;
        }
        let camera = self.eye_position().unwrap_or(at);
        self.particles
            .fireworks(at, velocity, explosions.to_vec(), camera);
        true
    }
}

#[cfg(test)]
#[path = "client_fireworks_tests.rs"]
mod client_fireworks_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::particles::ParticleType;

    #[test]
    fn nonfinite_nested_particle_parameters_are_rejected() {
        let good = ParticleOptions::simple(ParticleType::Smoke);
        assert!(finite_options(&good));
        let mut bad = good.clone();
        bad.to_color[0] = f32::NAN;
        assert!(!finite_options(&bad));
        bad = good.clone();
        bad.trail = Some(([f64::INFINITY, 0.0, 0.0], 3));
        assert!(!finite_options(&bad));
        bad = good.clone();
        bad.geyser = Some((1, f32::NAN));
        assert!(!finite_options(&bad));
        bad = good;
        bad.vibration = Some((
            PositionSource::Entity {
                id: 1,
                y_offset: f32::NAN,
            },
            [0.0; 3],
            3,
        ));
        assert!(!finite_options(&bad));
        bad.vibration = Some((PositionSource::Block([0; 3]), [0.0, f64::NAN, 0.0], 3));
        assert!(!finite_options(&bad));
    }
}
