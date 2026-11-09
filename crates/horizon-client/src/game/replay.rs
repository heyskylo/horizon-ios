//! Replay playback's state in a `Game`. Recording a session's packets as `.mcpr` and playing them
//! back is platform (docs/layers.md, "Replays"); Studio's editor (`studio.rs`) builds on it.

use super::Game;
use horizon_client::studio::ReplayDriver;

/// The packet replay a `Game` plays, if any, and its playback clock.
#[derive(Default)]
pub(super) struct Replay {
    /// The packet replay playing, if any.
    pub(super) driver: Option<ReplayDriver>,
    /// Camera perspective to restore after leaving replay playback.
    pub(super) previous_camera_type: Option<super::perspective::CameraType>,
    /// Replay opened from the title screen owns the synthetic remote session.
    pub(super) standalone: bool,
    pub(super) last_tick: Option<std::time::Instant>,
    pub(super) virtual_tick_ms: f32,
}

impl Replay {
    /// A replay plays: the world shown is the recording's.
    pub(super) fn playing(&self) -> bool {
        self.driver.is_some()
    }
    /// How far the replay's player is between its last two ticks, while a replay plays: it
    /// moves on the replay's clock, not the game's.
    pub(super) fn player_alpha(&self) -> Option<f64> {
        self.playing()
            .then(|| (f64::from(self.virtual_tick_ms) / 50.0).clamp(0.0, 1.0))
    }
}

impl Game<'_> {
    /// How far the window's own player model is between two ticks: on the replay's clock while a
    /// replay plays, else the game's.
    pub(super) fn own_player_alpha(&self) -> f64 {
        self.replay
            .player_alpha()
            .unwrap_or_else(|| self.clock.camera_fraction())
    }
}
