//! Splitscreen on an AirPlay display (iPhone and iPad; Horizon's own): the display splits top and bottom, the
//! first player on top with the controller, and a second player below whose controller is the phone: while
//! they play, its touch controls are drawn on the phone alone (horizon-airplay); with a screen open (their
//! inventory, a chest, the pause menu) their game draws on the phone instead, the screen over the panorama and
//! never the world, and they tap it there. The second player is a second client `Game` joined to the window's
//! integrated server through memory, as a LAN guest joins it, with its own renderer on the display's bottom
//! half, no sound (the first player's plays) and no controller. The pause menu starts and ends it
//! (game/screens.rs); the second player leaving the world, the first leaving theirs, or the AirPlay display
//! going away ends it too.
use super::{Game, UiResources};
use horizon_client::{Result, audio::Audio, hud::HudAtlas};
use std::time::Instant;
use winit::{application::ApplicationHandler, event::WindowEvent};

#[derive(Default)]
pub(super) struct Splitscreen {
    /// This game is the second player.
    pub guest: bool,
    /// The second player plays alongside this one.
    pub active: bool,
    /// The pause menu asked to start (true) or end (false) it.
    pub request: Option<bool>,
}

/// The second player's name: the first player's with a 2, offline as a LAN guest's.
fn guest_name(player: &str) -> String {
    let base: String = player.chars().take(15).collect();
    let name = format!("{base}2");
    if name == player {
        format!("{base}3")
    } else {
        name
    }
}

impl<'a> Game<'a> {
    /// Splitscreen can start: the world is this window's own and on an AirPlay display.
    pub(super) fn splitscreen_available(&self) -> bool {
        !self.splitscreen.guest
            && !self.options.split_guest
            && !self.options.paced_by_split
            && self.integrated.is_some()
            && self.pending_save.is_none()
            && self.title.is_none()
            && !horizon_ios_launcher::covered()
            && self.on_airplay()
    }

    /// The second player, joined to this game's integrated server and drawing on the TV's bottom half.
    fn splitscreen_guest(&mut self) -> Result<Game<'a>> {
        let server = self
            .integrated
            .as_ref()
            .ok_or("no world of this window's own")?;
        let window = self.window.clone().ok_or("no window")?;
        let player = horizon_client::auth::current()
            .map_or_else(|| self.options.player_name(), |a| a.name.clone());
        let name = guest_name(&player);
        let pipe = server.join_splitscreen(horizon_protocol::crypto::offline_uuid(&name))?;
        // A client and nothing else: no control socket, no benchmark, no world of its own to open.
        let mut options = self.options.clone();
        options.control = None;
        options.benchmark = None;
        options.frames = None;
        options.world_frames = None;
        options.report = None;
        options.server = None;
        options.world = None;
        options.player_name = Some(name.clone());
        let mut settings = self.settings.clone();
        // The controller is the first player's.
        settings.controller.enabled = false;
        let vanilla = self.vanilla_root();
        let root = super::packs::startup_root(
            &vanilla,
            &self.settings.resource_packs,
            &self.server_packs.active,
        )?;
        let mut atlas = HudAtlas::build_over(&root, &vanilla)?;
        atlas.font.uniform = self.settings.force_unicode;
        atlas.font.japanese = self.settings.japanese_glyphs;
        let gui = UiResources {
            atlas,
            panorama: self.gui.panorama.clone(),
        };
        let audio = Audio::silent(&self.options.resources);
        // The resources outlive both games; the second borrows them as the first does.
        let res = &self.resource_source;
        let mut guest = Game::new(res, self.pack.clone(), gui, options, settings, audio, None)?;
        guest.splitscreen.guest = true;
        guest.focused = self.focused;
        guest.ui.vanilla_root = vanilla;
        guest.renderer = Some(guest.new_renderer(window.clone())?);
        guest.world_renderer_ready = true;
        guest.window = Some(window);
        guest.apply_settings();
        guest.previous = Instant::now();
        guest.connect_local(pipe, &name)?;
        log::info!("Horizon: splitscreen: {name} joins on the phone");
        Ok(guest)
    }

    /// The second player has left the world (Disconnect, or the server sent them away).
    fn splitscreen_left(&self) -> bool {
        self.remote.is_none() || self.title.is_some() || self.error.is_some()
    }

    fn end_splitscreen(&mut self) {
        if let Some(server) = &self.integrated {
            server.leave_splitscreen();
        }
        self.splitscreen.active = false;
        horizon_airplay::phone_shows_game(false);
        horizon_airplay::phone_controls(None);
        if let Some(server) = &self.integrated {
            server.set_paused(server_paused(self.pause, false));
        }
        log::info!("Horizon: splitscreen ended");
    }
    /// Release the memory connection before dropping its renderer and client state.
    pub(super) fn stop_splitscreen(&mut self) {
        if let Some(mut guest) = self.splitscreen_guest.take() {
            if let Some(mut remote) = guest.remote.take() {
                remote.session.disconnect();
            }
            self.end_splitscreen();
        }
    }

    pub(super) fn poll_splitscreen(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let mut guest = self.splitscreen_guest.take();
        update(self, &mut guest);
        if let Some(second) = &mut guest {
            second.about_to_wait(event_loop);
        }
        update(self, &mut guest);
        self.splitscreen_guest = guest;
        // Starting or ending the second client changes the TV layout and phone controls.
        self.sync_airplay();
    }
}

/// Starts and ends the second player, once a frame after both games' own.
pub(super) fn update<'a>(host: &mut Game<'a>, guest: &mut Option<Box<Game<'a>>>) {
    let request = host.splitscreen.request.take();
    if guest.is_some()
        && (request == Some(false)
            || guest.as_ref().is_some_and(|g| g.splitscreen_left())
            || host.integrated.is_none()
            || host.pending_save.is_some()
            || !host.on_airplay())
    {
        // Dropping the guest closes its connection, and the server sees the player leave.
        if let Some(mut second) = guest.take() {
            if let Some(error) = second.error.take() {
                log::warn!("Horizon: splitscreen guest: {error}");
            }
            if let Some(mut remote) = second.remote.take() {
                remote.session.disconnect();
            }
        }
        host.end_splitscreen();
    }
    if request == Some(true) && guest.is_none() && host.splitscreen_available() {
        match host.splitscreen_guest() {
            Ok(g) => {
                *guest = Some(Box::new(g));
                // The phone hands over every held input; no finger or menu drag survives it.
                host.touch = Default::default();
                host.keys.clear();
                host.physical_keys.clear();
                host.left = false;
                host.right = false;
                host.reset_inventory_clicks();
                host.stop_mining();
                host.splitscreen.active = true;
                if let Some(server) = &host.integrated {
                    server.set_paused(false);
                }
            }
            Err(e) => {
                log::warn!("Horizon: splitscreen: {e}");
                host.end_splitscreen();
            }
        }
    }
    if let Some(g) = guest {
        // Playing: the bottom half, once the first player's game has split the display (its frame after the
        // start), and the buttons on the phone. A screen open: the phone, which shows it.
        let menu = !g.touch_playing();
        let half = horizon_airplay::second_screen().filter(|_| !menu);
        super::airplay::sync(g.renderer.as_mut(), g.window.as_ref(), &mut g.airplay, half);
        horizon_airplay::phone_shows_game(menu);
        let buttons = if menu { Vec::new() } else { g.phone_buttons() };
        horizon_airplay::phone_controls(Some(&buttons));
    }
}

/// Input from the phone's window: the second player's, while there is one. The rest (redraws, resizes,
/// focus) goes to both games: the first draws on the AirPlay display whenever the window redraws.
pub(super) fn guest_input(event: &WindowEvent) -> bool {
    matches!(
        event,
        WindowEvent::Touch(_)
            | WindowEvent::KeyboardInput { .. }
            | WindowEvent::Ime(_)
            | WindowEvent::ModifiersChanged(_)
            | WindowEvent::MouseInput { .. }
            | WindowEvent::MouseWheel { .. }
            | WindowEvent::CursorMoved { .. }
            | WindowEvent::CursorEntered { .. }
            | WindowEvent::CursorLeft { .. }
    )
}

/// Native lifecycle and redraw events apply to each renderer on the shared phone window.
pub(super) fn shared_event(event: &WindowEvent) -> bool {
    matches!(
        event,
        WindowEvent::Resized(_)
            | WindowEvent::ScaleFactorChanged { .. }
            | WindowEvent::Focused(_)
            | WindowEvent::Occluded(_)
            | WindowEvent::RedrawRequested
    )
}

/// A pause menu cannot stop the world while another local player is connected.
pub(super) fn server_paused(paused: bool, active: bool) -> bool {
    paused && !active
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_player_keeps_the_integrated_world_running() {
        assert!(server_paused(true, false));
        assert!(!server_paused(true, true));
        assert!(!server_paused(false, true));
        assert!(!server_paused(false, false));
    }

    #[test]
    fn focus_resize_and_redraw_are_shared_while_cursor_input_is_not() {
        for event in [
            WindowEvent::Focused(false),
            WindowEvent::RedrawRequested,
            WindowEvent::Resized(winit::dpi::PhysicalSize::new(1280, 720)),
        ] {
            assert!(shared_event(&event));
            assert!(!guest_input(&event));
        }
        let event = WindowEvent::CursorLeft {
            device_id: winit::event::DeviceId::dummy(),
        };
        assert!(guest_input(&event));
        assert!(!shared_event(&event));
        assert!(!guest_input(&WindowEvent::CloseRequested));
        assert!(!shared_event(&WindowEvent::CloseRequested));
    }

    #[test]
    fn the_second_player_is_the_first_with_a_two() {
        assert_eq!(guest_name("Steve"), "Steve2");
        assert_eq!(guest_name("ABCDEFGHIJKLMNOP"), "ABCDEFGHIJKLMNO2");
        assert_eq!(guest_name("ABCDEFGHIJKLMNO2"), "ABCDEFGHIJKLMNO3");
    }
}
