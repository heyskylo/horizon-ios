//! The server's ClientboundPlayerAbilitiesPacket on the local player (`ClientPacketListener
//! .handlePlayerAbilities`).
use super::*;

/// `Abilities.walkingSpeed` before the server sends one.
const WALKING_SPEED: f32 = 0.1;

/// The walking speed `AbstractClientPlayer.getFieldOfViewModifier` divides the movement speed by:
/// the last abilities packet's (`Abilities.setWalkingSpeed`), else the default.
fn walking_speed(abilities: Option<&play::PlayerAbilities>) -> f32 {
    abilities.map_or(WALKING_SPEED, |a| a.walking_speed)
}

impl Game<'_> {
    /// ClientPacketListener.handlePlayerAbilities: the player flies, may fly
    /// and is invulnerable as the server says (bits 2, 4 and 1). The walking
    /// speed is read where the field of view needs it
    /// ([`Game::remote_walking_speed`]), the instabuild bit where the packet
    /// is handled (`Game::creative`).
    pub(super) fn apply_remote_abilities(&mut self) {
        let (Some(player), Some(a)) = (
            self.player.as_mut(),
            self.remote.as_ref().and_then(|r| r.abilities.as_ref()),
        ) else {
            return;
        };
        player.invulnerable = a.flags & 1 != 0;
        player.flying = a.flags & 2 != 0;
        player.may_fly = a.flags & 4 != 0;
        player.flying_speed = a.flying_speed;
    }

    /// `Abilities.getWalkingSpeed()` of the local player: the server's, on a server.
    pub(in crate::game) fn remote_walking_speed(&self) -> f32 {
        walking_speed(self.remote.as_ref().and_then(|r| r.abilities.as_ref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::camera::{FovPlayer, field_of_view_modifier};

    fn fov(walking: f32, movement: f64) -> f32 {
        let player = FovPlayer {
            flying: false,
            walking_speed: walking,
            movement_speed: movement,
            using: None,
        };
        field_of_view_modifier(&player, true, 1.0)
    }

    #[test]
    fn the_field_of_view_divides_by_the_servers_walking_speed() {
        // A server that sets the walking speed to 0.2 and the movement speed to 0.2: vanilla's
        // getFieldOfViewModifier is (0.2 / 0.2 + 1) / 2 = 1. Kept at 0.1 it would be
        // (0.2 / 0.1 + 1) / 2 = 1.5, a zoomed-out view.
        let packet = play::PlayerAbilities {
            flags: 0,
            flying_speed: 0.05,
            walking_speed: 0.2,
        };
        assert_eq!(fov(walking_speed(Some(&packet)), 0.2), 1.0);
        // Before any packet: vanilla's default 0.1, a still player at the base speed 0.1.
        assert_eq!(walking_speed(None), 0.1);
        assert_eq!(fov(walking_speed(None), 0.1), 1.0);
    }
}
