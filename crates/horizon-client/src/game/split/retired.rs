//! Departed players no longer receive frames, but keep ownership of in-flight work.
use super::{Game, Split};

mod cohort;

/// The same all-producer gate for active guests on Quit and departed guests each frame.
pub(super) fn workers_stopped(game: &mut Game<'_>) -> bool {
    game.audio.stop_all();
    let mut stopped = game.shutdown_server_pack_worker();
    stopped &= game.shutdown_culling_workers();
    stopped &= game.shutdown_far_terrain();
    stopped &= game.shutdown_skin_workers();
    stopped &= game.cosmetics.shutdown();
    stopped &= game.shutdown_mod_browser();
    stopped &= game.shutdown_recording_workers();
    if let Some(renderer) = &mut game.renderer {
        stopped &= renderer.shutdown_pack_compiler();
    }
    stopped &= game
        .rest_loader
        .as_ref()
        .is_none_or(|loader| loader.ready().unwrap_or(false));
    stopped &= game.shutdown_integrated_workers();
    if stopped && let Some(loader) = game.rest_loader.take() {
        let _ = loader.finish();
    }
    stopped
}

impl<'a> Split<'a> {
    /// Called only after the seat, server membership and focused input have been removed.
    pub(super) fn retire_guest(&mut self, mut game: Box<Game<'a>>) {
        if let Some(remote) = &mut game.remote {
            remote.session.disconnect();
        }
        game.audio.stop_all();
        game.release_keyboard_mouse();
        game.keyboard_mouse = false;
        game.keys.clear();
        game.touch = Default::default();
        game.pad_close_output();
        game.pad.release_hardware();
        game.pad = Default::default();
        // The control socket/seat can immediately be reused by a joining player.
        game.control = None;
        // Guests render offscreen; never hide/close their shared compositor window.
        game.window = None;
        self.retired_guests.push(game);
        self.poll_retired_guests();
    }

    pub(super) fn poll_retired_guests(&mut self) -> bool {
        cohort::poll(&mut self.retired_guests, |game| {
            // Release the offscreen renderer's Window Arc once its own producers stop,
            // even when a skin download or another Game-owned service is still draining.
            let culling = game.shutdown_culling_workers();
            let compiler = game
                .renderer
                .as_mut()
                .is_none_or(|r| r.shutdown_pack_compiler());
            if culling && compiler {
                game.renderer = None;
            }
            workers_stopped(game)
        })
    }
}
