//! The mouse in play: the look it turns, the pointer grab (`MouseHandler.grabMouse` /
//! `releaseMouse`), the cursor icon, and pausing a world whose window loses focus.
use super::*;

fn studio_suppresses_focus_pause(replay_active: bool, freecam_active: bool) -> bool {
    replay_active || freecam_active
}

impl Game<'_> {
    /// The options `MouseHandler.turnPlayer` reads.
    pub(super) fn look_options(&self) -> LookOptions {
        LookOptions {
            sensitivity: f64::from(self.settings.sensitivity),
            smooth_camera: self.smooth_camera,
            scoping: self.scoping(),
            invert_x: self.settings.invert_x,
            invert_y: self.settings.invert_y,
        }
    }
    /// `MouseHandler.handleAccumulatedMovement` and `Entity.turn`, once per frame.
    /// An agent moving the mouse (`mouse-move`) counts as a grabbed pointer in
    /// an active window, as it cannot grab the real one.
    pub(super) fn apply_mouse_look(&mut self) {
        let look = self.look_options();
        let time = self.started.elapsed().as_secs_f64();
        let pointer = self.captured || self.agent_pointer;
        let grabbed = pointer
            && self.screen == Screen::Playing
            && (self.player.is_some() || self.studio.flying());
        let active = self.focused || self.agent_pointer;
        if let Some((xo, yo)) = self
            .mouse
            .handle_accumulated_movement(time, active, grabbed, look)
        {
            self.look_turn(xo, yo);
        }
    }
    /// A look from any device, in `Entity.turn` units: the tutorial's "look
    /// around" step hears it and Horizon's zoom slows it, for the mouse and a
    /// controller alike.
    pub(super) fn look_turn(&mut self, x: f64, y: f64) {
        self.tutorial.mouse(x, y);
        let zoom = self.zoom_turn_scale();
        self.turn_player(x * zoom, y * zoom);
    }
    /// `Minecraft.pause`: a single-player game with a pausing screen open.
    pub(super) fn pauses_game(&self) -> bool {
        self.has_singleplayer_server()
            && !self.is_published()
            && !self.has_split_players()
            && self.title.is_none()
            && (self.screen.is_pause_screen() || self.book.is_some() || self.win_screen_pauses())
    }
    /// A screen open over gameplay while `screen` stays `Playing`: a book or
    /// lectern, a sign, a server dialog, the code of conduct, a link or
    /// server pack prompt (vanilla's `Minecraft.screen` is set for each).
    pub(super) fn screen_over_playing(&self) -> bool {
        self.book.is_some() || self.sign_edit.is_some() || self.prompt_open()
    }
    /// A prompt is up: over the world or over another open screen, it takes the input and is
    /// answered first, in the order of `handle_window_event`'s arms and of what draws on top.
    pub(super) fn prompt_open(&self) -> bool {
        self.dialog_open()
            || self.code_of_conduct.is_some()
            || self.link_prompt.is_some()
            || self.command_prompt.is_some()
            || self.server_packs.prompt.is_some()
    }
    /// `Minecraft.pauseIfInactive`: a window without focus for more than 500 ms
    /// pauses the game. An open screen (books, signs, the pack prompt, a server
    /// dialog) stays instead of pausing (`Gui.setPauseScreen`).
    pub(super) fn pause_if_inactive(&mut self) {
        if self
            .focus_lost
            .is_some_and(|t| t.elapsed() > Duration::from_millis(500))
            && self.control.is_none()
            && self.screen == Screen::Playing
            && !studio_suppresses_focus_pause(self.replay.playing(), self.studio.flying())
            && self.player.is_some()
            && !self.options.no_pause
            && self.settings.file.bool("pauseOnLostFocus")
            && !self.pad_keeps_playing_unfocused()
            && self.book.is_none()
            && self.sign_edit.is_none()
            && self.server_packs.prompt.is_none()
            && !self.dialog_open()
        {
            self.pause_focus = None;
            self.screen = Screen::Paused;
        }
    }
    /// `KeyMapping.setAll`: every key physically held is held again, as the
    /// mapping it is bound to, so a key pressed or kept down behind a screen
    /// counts once the game has the keyboard back. A sneak or sprint that
    /// toggles keeps its toggle state (`shouldSetOnIngameFocus`).
    pub(super) fn restore_held_keys(&mut self) {
        let held: Vec<KeyCode> = self.physical_keys.iter().copied().collect();
        let toggles = [self.settings.toggle_crouch, self.settings.toggle_sprint];
        for key in held {
            for mapped in self.settings.keys.translate_all(key) {
                match mapped {
                    Ok(slot_key @ (KeyCode::ShiftLeft | KeyCode::ControlLeft)) => {
                        let slot = usize::from(slot_key == KeyCode::ControlLeft);
                        if !toggles[slot] {
                            self.toggles.0[slot].set_down(true, false);
                            self.sync_toggle(slot);
                        }
                    }
                    Ok(other) => {
                        self.keys.insert(other);
                    }
                    Err(_) => {}
                }
            }
        }
    }
    /// `MouseHandler.grabMouse` / `releaseMouse`. Both put the pointer at
    /// the window's centre, so a screen opened from gameplay (inventory,
    /// chest, pause, chat, sign, book, death) starts with the cursor in the
    /// middle instead of wherever the system last left it. Releasing when
    /// already released does nothing, so moving from screen to screen keeps
    /// the cursor where it is. Like GLFW's `glfwSetCursorPos`, nothing warps
    /// while the window is unfocused (losing focus releases the pointer).
    pub(super) fn capture(&mut self, capture: bool) {
        // A guest's action (`as_guest`) opens and closes screens on the
        // guest's own client: the window's pointer stays the host's (#687).
        if self.lan_acting.is_some() {
            return;
        }
        // MouseHandler.grabMouse never captures an inactive window, and vanilla
        // grabs only on `setScreen(null)`: the pack prompt and server dialogs
        // are screens over the world that keep the pointer.
        if capture
            && ((!self.focused && !self.unclaimed_window && !self.scripted_run())
                || self.server_packs.prompt.is_some()
                || self.dialog_open()
                || self.command_prompt.is_some())
        {
            return;
        }
        // The container screen's press state belongs to the screen: a screen
        // that takes the pointer over starts with its own state, as
        // `AbstractContainerScreen` does, so a press whose release never
        // arrived (the pointer left the window) cannot wedge the next
        // screen's quick move and clicks.
        self.reset_inventory_clicks();
        if !capture {
            self.stop_mining();
            self.studio.rmb_down = false;
        }
        // The real pointer takes over from an agent's.
        self.agent_pointer = false;
        // Splitscreen: another player has the pointer; this one's menus leave it alone.
        if !self.keyboard_mouse {
            self.captured = false;
            self.capture_mode = None;
            return;
        }
        if capture && !self.captured && !cfg!(target_os = "macos") {
            // MouseHandler.grabMouse: KeyMapping.setAll unless
            // InputQuirks.RESTORE_KEY_STATE_AFTER_MOUSE_GRAB is off (macOS).
            self.restore_held_keys();
        }
        let Some(window) = &self.window else { return };
        let size = self.renderer.as_ref().map_or_else(
            || window.inner_size().into(),
            |r| [r.config.width, r.config.height],
        );
        let [vx, vy] = self.view_at.map(f64::from);
        let centre = winit::dpi::PhysicalPosition::new(
            vx + f64::from(size[0]) / 2.0,
            vy + f64::from(size[1]) / 2.0,
        );
        let warp = |window: &Window| {
            let _ = window.set_cursor_position(centre);
        };
        let scale = HudBatch::new(size).scale;
        let was_grabbed = self.captured;
        if self.internal_grab() {
            // Its grab is internal (an agent turns the view with `mouse-move`). A grab the
            // window already held when a scripted run began stays the system's until the
            // run ends (`settle_after_run`).
            if capture {
                self.mouse.set_ignore_first_move();
            }
            self.captured = capture;
        } else if capture {
            self.mouse.set_ignore_first_move();
            // macOS warps by re-associating the mouse with the pointer,
            // which undoes a Locked grab and leaves the pointer free: warp
            // first there. Wayland moves a pointer only while it is locked,
            // so everywhere else warp after grabbing (Windows does both).
            #[cfg(target_os = "macos")]
            warp(window);
            // Windows clips a Locked pointer to a 1x1 rectangle where it is, and `SetCursorPos`
            // stays inside that clip: warp first, or the pointer stays frozen wherever it was
            // (over another window, on another monitor) instead of the window's centre (#6076).
            #[cfg(windows)]
            if self.focused {
                warp(window);
            }
            self.capture_mode = if window.set_cursor_grab(CursorGrabMode::Locked).is_ok() {
                Some(CursorGrabMode::Locked)
            } else if window.set_cursor_grab(CursorGrabMode::Confined).is_ok() {
                Some(CursorGrabMode::Confined)
            } else {
                None
            };
            self.captured = self.capture_mode.is_some();
            // Windows already warped above, inside the clip's own position.
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            if self.captured && self.focused {
                warp(window);
            }
        } else {
            if self.captured {
                // Wayland moves a pointer only as the locked pointer's
                // position hint, which the compositor applies as the lock
                // goes (sway and Hyprland do): set it before ungrabbing.
                // X11, Windows and macOS warp, so warp again once free
                // (Wayland refuses that one).
                if self.focused {
                    warp(window);
                }
                let _ = window.set_cursor_grab(CursorGrabMode::None);
                if self.focused {
                    warp(window);
                }
            } else {
                let _ = window.set_cursor_grab(CursorGrabMode::None);
            }
            self.captured = false;
            self.capture_mode = None;
        }
        // MouseHandler.grabMouse and releaseMouse: the mouse position is
        // the window's centre whenever the pointer's grab state changes, so
        // a screen opened from gameplay starts with the cursor in the middle
        // and the game starts with it away from the screen's last spot.
        if let Some(pos) = horizon_client::mouse::mouse_position_after_grab_change(
            capture,
            was_grabbed,
            size,
            scale,
        ) {
            self.cursor = pos;
        }
        if !self.internal_grab() {
            window.set_cursor_visible(!self.captured);
        }
    }
    /// The game's pointer grab leaves the system pointer where the person at the computer has
    /// it: a genuinely hidden `--offscreen` run, an agent's window no one has clicked yet, or a
    /// scripted run, which plays the game while the person may be using the computer.
    pub(super) fn internal_grab(&self) -> bool {
        (self.options.offscreen && !self.split_window)
            || self.unclaimed_window
            || self.scripted_run()
    }
    /// Once a scripted run is over, the person's last click into the window or away from it
    /// applies (`held_focus`), and the system pointer follows the grab the run left inside the
    /// game, as if the window had taken or released it now.
    pub(super) fn settle_after_run(&mut self, event_loop: &dyn EventLoopCtl) {
        if self.scripted_run() {
            return;
        }
        if let Some(focused) = self.focus_after_run.take() {
            let id = self
                .window
                .as_ref()
                .map_or_else(WindowId::dummy, |w| w.id());
            self.handle_window_event(
                event_loop,
                id,
                WindowEvent::Focused(focused),
                input_trace::InputOrigin::User,
            );
        }
        // Outside a run, a window whose grab is the system's holds it exactly while the game
        // has it grabbed; only a run leaves the two apart.
        if !self.internal_grab() && self.captured != self.capture_mode.is_some() {
            let grab = self.captured;
            self.captured = self.capture_mode.is_some();
            self.capture(grab);
        }
    }
    /// A Linux window manager can release the native pointer grab while the
    /// window remains focused. Reapply the recorded mode on the next gameplay
    /// click or wheel event, without warping the pointer or consuming input.
    pub(super) fn reassert_pointer_capture(&mut self) {
        #[cfg(target_os = "linux")]
        if self.screen == Screen::Playing
            && self.focused
            && self.captured
            && !self.options.offscreen
            && !self.internal_grab()
            && let (Some(window), Some(mode)) = (&self.window, self.capture_mode)
        {
            let other = match mode {
                CursorGrabMode::Locked => CursorGrabMode::Confined,
                CursorGrabMode::Confined => CursorGrabMode::Locked,
                CursorGrabMode::None => return,
            };
            self.capture_mode = if window.set_cursor_grab(mode).is_ok() {
                Some(mode)
            } else if window.set_cursor_grab(other).is_ok() {
                Some(other)
            } else {
                None
            };
            self.captured = self.capture_mode.is_some();
            window.set_cursor_visible(!self.captured);
        }
    }
    /// `GuiGraphicsExtractor.applyCursor` -> `Window.selectCursor` at the end
    /// of a frame: the cursor the frame's widgets asked for, set on the window
    /// only when it changes.
    pub(super) fn apply_cursor_icon(&mut self) {
        use horizon_client::widgets::Cursor;
        let wanted = horizon_client::widgets::take_cursor();
        // The widgets under the keyboard's hidden pointer aren't under the
        // mouse (`handleCursor` asks only for a hovered widget).
        let effective = if self.settings.allow_cursor_changes && !self.key_focus_driving() {
            wanted
        } else {
            Cursor::Arrow
        };
        if effective == self.cursor_icon || !self.keyboard_mouse {
            return;
        }
        if let Some(window) = &self.window {
            window.set_cursor(match effective {
                Cursor::Arrow => winit::window::CursorIcon::Default,
                Cursor::PointingHand => winit::window::CursorIcon::Pointer,
                Cursor::NotAllowed => winit::window::CursorIcon::NotAllowed,
            });
        }
        self.cursor_icon = effective;
    }
    /// LevelLoadingScreen.onClose -> Gui.setScreen(null) -> grabMouse.
    pub(super) fn capture_loaded_world(&mut self) {
        // Loading may finish after focus was lost or another screen opened.
        if self.title.is_none() && self.screen == Screen::Playing {
            self.capture(true);
        }
    }
    /// The window came back into focus after holding the pointer: the system
    /// dropped the grab with the focus, and vanilla's grab outlives it
    /// (`MouseHandler.mouseGrabbed` stays true while GLFW keeps the cursor
    /// mode), so the game takes the pointer again without waiting for a
    /// click. A screen that is open keeps it free, as releasing the pointer
    /// for that screen did: it is the screen, not the focus, that shows it.
    pub(super) fn regrab_on_refocus(&mut self) {
        if self.studio_up() {
            self.regrab_on_refocus = false;
            self.studio.rmb_down = false;
            self.capture(false);
            return;
        }
        if std::mem::take(&mut self.regrab_on_refocus) && self.screen == Screen::Playing {
            self.capture(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::studio_suppresses_focus_pause;

    #[test]
    fn studio_replay_and_freecam_never_pause_on_focus_loss() {
        assert!(studio_suppresses_focus_pause(true, false));
        assert!(studio_suppresses_focus_pause(false, true));
        assert!(!studio_suppresses_focus_pause(false, false));
    }
}
