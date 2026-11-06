//! What server mods send one player (docs/native-mods.md, "Server hooks for
//! plugin ports: per-player feedback"): sounds (`native_play_sound_to`). The
//! hub is out of the game while a guest's turn runs, and they need it (registry
//! ids, the player's position), so they wait here and leave at the tick's end,
//! from whichever hook sent them.
use super::{Game, lan::LanHost};

/// One thing a mod sends one player.
pub(super) enum PlayerFx {
    /// At `at`, or where the player is when it leaves (None).
    Sound {
        sound: String,
        source: i32,
        at: Option<[f64; 3]>,
        volume: f32,
        pitch: f32,
    },
}

/// What waits for the hub, by player UUID, in the order the mods sent it.
#[derive(Default)]
pub(super) struct NativePlayerFx {
    queued: Vec<(u128, PlayerFx)>,
}

impl Game<'_> {
    /// Whether a per-player helper takes something for `uuid`: on a server,
    /// for a player who is on; inside a guest's turn, where the other players
    /// are out of reach, for anyone (the flush drops it for a player who
    /// isn't on).
    pub(super) fn player_fx_reaches(&self, uuid: u128) -> bool {
        if self.remote.is_some() {
            return false;
        }
        if self.lan_acting_uuid == Some(uuid) {
            return true;
        }
        match &self.lan {
            Some(host) => host.hub.guests.iter().any(|g| g.placed && g.uuid == uuid),
            None => self.lan_acting.is_some(),
        }
    }

    pub(super) fn queue_native_player_fx(&mut self, uuid: u128, fx: PlayerFx) {
        self.native_player_fx.queued.push((uuid, fx));
    }

    /// What the mods sent single players this tick leaves.
    pub(super) fn flush_native_player_fx(&mut self, host: &mut LanHost) {
        if self.native_player_fx.queued.is_empty() {
            return;
        }
        for (uuid, fx) in std::mem::take(&mut self.native_player_fx.queued) {
            let Some((conn, position)) = host
                .hub
                .guests
                .iter()
                .find(|g| g.placed && g.uuid == uuid)
                .map(|g| (g.conn, g.position))
            else {
                continue;
            };
            let packet = match fx {
                PlayerFx::Sound {
                    sound,
                    source,
                    at,
                    volume,
                    pitch,
                } => {
                    // As `flush_native_sounds`: an unknown `minecraft:` event is
                    // dropped, a mod's goes by name.
                    if sound.starts_with("minecraft:")
                        && host
                            .hub
                            .registry_id("minecraft:sound_event", &sound)
                            .is_none()
                    {
                        continue;
                    }
                    let seed = host.hub.next_sound_seed();
                    host.hub.sound_packet(
                        &sound,
                        at.unwrap_or(position),
                        volume,
                        pitch,
                        source,
                        seed,
                    )
                }
            };
            host.hub.send_to(conn, &packet);
        }
    }
}
