//! Horizon Studio, the replay editor: its free camera, camera path, timeline and top bar, and the
//! `/studio freecam|keyframe|path` and `horizon-ctl studio screen` commands. It builds on the
//! platform's replay recording and playback (`replay.rs`).

use super::{Game, Screen, packs};
use horizon_client::{
    Result,
    studio::{ReplayDriver, TimelineAction},
    text::Text,
};
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};

/// Studio's state in a `Game`: the replay editor's free camera, timeline and top bar. Studio is a
/// Horizon extra (docs/layers.md: the replay editor) on top of the platform's replay
/// (`replay.rs`); the engine only asks it whether the free camera flies.
#[derive(Default)]
pub(super) struct Studio {
    pub(super) freecam: horizon_client::studio::StudioFreecam,
    pub(super) timeline: horizon_client::studio::TimelineUi,
    pub(super) topbar: horizon_client::studio::TopBarUi,
    pub(super) rmb_down: bool,
    pub(super) camera_track_dirty: bool,
}

impl Studio {
    /// The free camera flies, apart from the player.
    pub(super) fn flying(&self) -> bool {
        self.freecam.active
    }
}

pub const HELP: &[&str] = &[
    "studio record start [NAME]     start recording clientbound packets into .mcpr archive",
    "studio record stop             stop recording and finalize .mcpr archive",
    "studio record status           query recording status (active, elapsed, packets, bytes)",
    "studio replay open PATH        open .mcpr recording archive for playback",
    "studio replay pause            pause replay playback",
    "studio replay resume           resume replay playback",
    "studio replay speed MULTIPLIER set replay playback speed (0.01..20.0)",
    "studio replay seek MS          seek replay playback to timestamp in milliseconds",
    "studio replay status           query replay playback status",
    "studio replay close            close active replay session",
    "studio freecam <on|off>        enable or disable the Studio free camera",
    "studio keyframe <add|remove INDEX|clear> edit the replay camera path",
    "studio path <lock|unlock>      play or stop the camera path",
    "studio screen open             open the Studio script screen (Screen::Studio)",
    "studio screen close            close the Studio script screen back to Playing",
    "studio screen status           report whether Screen::Studio is open",
];

/// What `Game::open_studio_screen` does over `screen` (`blocked`: an overlay,
/// the title screen, no window or no player, the world loading): `None` opens
/// it, `Some` answers without changing anything (true: already open).
pub(super) fn studio_screen_opens(screen: Screen, blocked: bool) -> Option<bool> {
    if screen == Screen::Studio {
        return Some(true);
    }
    if screen != Screen::Playing || blocked {
        return Some(false);
    }
    None
}

impl Game<'_> {
    /// Studio is up (its bars, its camera path, its mouse): a replay plays or the free camera flies.
    pub(super) fn studio_up(&self) -> bool {
        self.replay.playing() || self.studio.flying()
    }

    /// Opens the Studio script screen (`Screen::Studio`) over the world.
    ///
    /// Horizon Studio script windows (`ui.open`) use this slot instead of the
    /// chat slot: the mouse is freed, the world keeps running (as with
    /// `Screen::Chat`), but `draw_chat_screen` is not invoked for
    /// `Screen::Studio` (see the draw match in `game.rs`), so chat history
    /// and the empty chat input never draw behind the window. Escape closes
    /// it back to `Screen::Playing` (see `play_keys.rs`), as does
    /// `close_studio_screen`. True when open: false while another screen is,
    /// over the title screen, a book, a sign editor, a dialog or a prompt,
    /// while the world loads, and anywhere but the window's own client.
    pub(super) fn open_studio_screen(&mut self) -> bool {
        // What `native_mod_screen_open` refuses over the world: the Studio
        // screen would be up with no hook to draw or click it.
        let blocked = self.title.is_some()
            || self.renderer.is_none()
            || self.lan_acting.is_some()
            || self.player.is_none()
            || self.screen_over_playing()
            || self.remote_loading();
        if let Some(open) = studio_screen_opens(self.screen, blocked) {
            return open;
        }
        self.screen = Screen::Studio;
        self.keys.clear();
        self.left = false;
        self.right = false;
        self.mining = None;
        self.capture(false);
        true
    }
    /// Closes the Studio script screen, if open, returning to `Screen::Playing`.
    pub(super) fn close_studio_screen(&mut self) {
        if self.screen == Screen::Studio {
            self.screen = Screen::Playing;
            self.capture(true);
        }
    }
    /// Whether the Studio script screen is currently open.
    pub(super) fn studio_screen_open(&self) -> bool {
        self.screen == Screen::Studio
    }

    /// Studio's editor words of `/studio` (`freecam`, `keyframe`, `path`).
    pub(super) fn studio_editor_command(&mut self, words: &[&str]) -> bool {
        match words[0] {
            "freecam" => match words.get(1).copied() {
                Some("on") => {
                    if self.replay.driver.is_none() {
                        self.system_message(Text::translate(
                            "horizon.studio.freecam.replay_required",
                            &[],
                        ));
                    } else if let Some((position, yaw, pitch)) = self.camera_view(1.0) {
                        self.studio.freecam.activate(
                            position,
                            yaw,
                            pitch,
                            self.settings.fov.clamp(1.0, 179.0),
                        );
                        self.capture(true);
                        self.system_message(Text::translate("horizon.studio.freecam.enabled", &[]));
                    } else {
                        self.system_message(Text::translate(
                            "horizon.studio.freecam.unavailable",
                            &[],
                        ));
                    }
                }
                Some("off") => {
                    self.studio.freecam.deactivate();
                    self.capture(false);
                    self.system_message(Text::translate("horizon.studio.freecam.disabled", &[]));
                }
                _ => self.system_message(Text::translate("horizon.studio.freecam.usage", &[])),
            },
            "keyframe" => {
                let Some(driver) = self.replay.driver.as_ref() else {
                    self.system_message(Text::translate("horizon.studio.replay.inactive", &[]));
                    return true;
                };
                let time_ms = driver.position_ms();
                match words.get(1).copied() {
                    Some("add") => {
                        if !self.studio.freecam.active
                            && let Some((position, yaw, pitch)) = self.camera_view(1.0)
                        {
                            self.studio.freecam.activate(
                                position,
                                yaw,
                                pitch,
                                self.settings.fov.clamp(1.0, 179.0),
                            );
                        }
                        self.capture(true);
                        self.studio.freecam.add_keyframe_at_current(time_ms);
                        self.studio.camera_track_dirty = true;
                        self.system_message(Text::tr(
                            "horizon.studio.keyframe.added",
                            &[&time_ms.to_string()],
                        ));
                    }
                    Some("clear") => {
                        self.studio.freecam.track.clear();
                        self.studio.camera_track_dirty = true;
                        self.system_message(Text::translate(
                            "horizon.studio.keyframe.cleared",
                            &[],
                        ));
                    }
                    Some("remove") => {
                        let Some(index) =
                            words.get(2).and_then(|value| value.parse::<usize>().ok())
                        else {
                            self.system_message(Text::translate(
                                "horizon.studio.keyframe.remove_usage",
                                &[],
                            ));
                            return true;
                        };
                        if index >= self.studio.freecam.track.keyframes.len() {
                            self.system_message(Text::translate(
                                "horizon.studio.keyframe.index_invalid",
                                &[],
                            ));
                            return true;
                        }
                        self.studio.freecam.track.keyframes.remove(index);
                        self.studio.camera_track_dirty = true;
                        self.system_message(Text::tr(
                            "horizon.studio.keyframe.removed",
                            &[&index.to_string()],
                        ));
                    }
                    _ => self.system_message(Text::translate("horizon.studio.keyframe.usage", &[])),
                }
            }
            "path" => match words.get(1).copied() {
                Some("lock") if !self.studio.freecam.track.is_empty() => {
                    self.studio.freecam.active = true;
                    self.studio.freecam.locked_to_path = true;
                    self.capture(true);
                    self.system_message(Text::translate("horizon.studio.path.locked", &[]));
                }
                Some("unlock") => {
                    self.studio.freecam.locked_to_path = false;
                    self.system_message(Text::translate("horizon.studio.path.unlocked", &[]));
                }
                _ => self.system_message(Text::translate("horizon.studio.path.usage", &[])),
            },
            _ => {}
        }
        true
    }

    /// Apply a menu action from the Studio top bar.
    pub(super) fn handle_studio_topbar_action(
        &mut self,
        action: horizon_client::studio::TopBarAction,
    ) {
        use horizon_client::studio::TopBarAction;
        match action {
            TopBarAction::None => {}
            TopBarAction::OpenPauseMenu => {
                self.pause_focus = None;
                self.screen = Screen::Paused;
                self.pause_menu = true;
                self.capture(false);
            }
            TopBarAction::ExitStudio => {
                if self.replay.driver.is_some() {
                    self.close_studio_replay();
                } else {
                    self.studio.freecam.deactivate();
                    self.capture(false);
                }
            }
            TopBarAction::OpenRecordingsFolder => {
                let directory = PathBuf::from("recordings");
                if let Err(error) = std::fs::create_dir_all(&directory) {
                    log::warn!("Studio: could not create recordings folder: {error}");
                } else {
                    packs::open_folder(&directory);
                }
            }
            TopBarAction::ResetCameraToPlayer => {
                let (position, yaw, pitch) = if let Some(player) = &self.player {
                    (
                        glam::DVec3::new(
                            player.position.x,
                            player.position.y + 1.62,
                            player.position.z,
                        ),
                        self.yaw,
                        self.pitch,
                    )
                } else {
                    (glam::DVec3::ZERO, 0.0, 0.0)
                };
                self.studio
                    .freecam
                    .activate(position, yaw, pitch, self.settings.fov);
            }
            TopBarAction::ToggleLockToPath => {
                self.studio.freecam.locked_to_path = !self.studio.freecam.locked_to_path;
            }
            TopBarAction::ToggleFreecam => {
                if self.studio.freecam.active {
                    self.studio.freecam.deactivate();
                    self.capture(false);
                } else {
                    let (position, yaw, pitch) =
                        self.camera_view(0.0)
                            .unwrap_or((glam::DVec3::ZERO, 0.0, 0.0));
                    self.studio
                        .freecam
                        .activate(position, yaw, pitch, self.settings.fov);
                    self.capture(true);
                }
            }
            TopBarAction::AddKeyframe => {
                self.studio
                    .freecam
                    .add_keyframe_at_current(self.studio.timeline.current_time_ms as u32);
                self.studio.camera_track_dirty = true;
            }
            TopBarAction::AddMarker => self.add_studio_marker(),
            TopBarAction::ClearKeyframes => {
                self.studio.freecam.track.keyframes.clear();
                self.studio.timeline.selected_keyframe = None;
                self.studio.camera_track_dirty = true;
            }
            TopBarAction::ToggleTimeline => {
                self.studio.timeline.visible = !self.studio.timeline.visible;
            }
            TopBarAction::SplitClip => {
                self.studio
                    .timeline
                    .split_at(self.studio.timeline.current_time_ms);
            }
        }
    }

    /// Apply an action from the replay timeline HUD.
    pub(super) fn handle_studio_timeline_action(
        &mut self,
        action: horizon_client::studio::TimelineAction,
    ) {
        use horizon_client::studio::TimelineAction;
        match action {
            TimelineAction::PauseForSeek => {
                self.studio.timeline.playing = false;
                if let Some(replay) = self.replay.driver.as_mut() {
                    replay.set_paused(true);
                }
            }
            TimelineAction::Seek(time_ms) => {
                let time_ms = if self.replay.standalone {
                    time_ms
                } else {
                    let current =
                        self.replay
                            .driver
                            .as_ref()
                            .map_or(0, ReplayDriver::position_ms) as u64;
                    time_ms.max(current)
                };
                self.studio.timeline.current_time_ms = time_ms;
                if let Err(error) = self.seek_studio_replay(time_ms.min(u32::MAX as u64) as u32) {
                    log::warn!("Studio timeline seek failed: {error}");
                }
            }
            TimelineAction::TogglePlay => {
                self.studio.timeline.playing = !self.studio.timeline.playing;
                if let Some(replay) = self.replay.driver.as_mut() {
                    replay.set_paused(!self.studio.timeline.playing);
                }
            }
            TimelineAction::StepBack => {
                let time_ms = self.studio.timeline.current_time_ms.saturating_sub(1000);
                self.handle_studio_timeline_action(TimelineAction::Seek(time_ms));
            }
            TimelineAction::StepForward => {
                let time_ms = self
                    .studio
                    .timeline
                    .current_time_ms
                    .saturating_add(1000)
                    .min(self.studio.timeline.duration_ms);
                self.handle_studio_timeline_action(TimelineAction::Seek(time_ms));
            }
            TimelineAction::SeekStart => {
                self.handle_studio_timeline_action(TimelineAction::Seek(0))
            }
            TimelineAction::SeekEnd => self.handle_studio_timeline_action(TimelineAction::Seek(
                self.studio.timeline.duration_ms,
            )),
            TimelineAction::SetSpeed(speed) => {
                self.studio.timeline.speed = speed;
                if let Some(replay) = self.replay.driver.as_mut() {
                    replay.set_speed(speed);
                }
            }
            TimelineAction::ToggleFreecam => {
                if self.studio.freecam.active {
                    self.studio.freecam.deactivate();
                } else {
                    let (position, yaw, pitch) =
                        self.camera_view(0.0)
                            .unwrap_or((glam::DVec3::ZERO, 0.0, 0.0));
                    self.studio
                        .freecam
                        .activate(position, yaw, pitch, self.settings.fov);
                }
            }
            TimelineAction::AddCameraKeyframe => {
                self.studio
                    .freecam
                    .add_keyframe_at_current(self.studio.timeline.current_time_ms as u32);
                self.studio.camera_track_dirty = true;
            }
            TimelineAction::RemoveCameraKeyframe(index) => {
                if index < self.studio.freecam.track.keyframes.len() {
                    self.studio.freecam.track.keyframes.remove(index);
                    self.studio.camera_track_dirty = true;
                }
                self.studio.timeline.selected_keyframe = None;
            }
            TimelineAction::SelectKeyframe(index) => {
                self.studio.timeline.selected_keyframe = Some(index);
            }
            TimelineAction::KeyframeMoved => {
                self.studio.camera_track_dirty = true;
            }
            TimelineAction::SplitClip(time_ms) => self.studio.timeline.split_at(time_ms),
            TimelineAction::None => {}
        }
    }

    /// Add a bookmark at the replay playhead and save it in the `.mcpr` archive.
    pub(super) fn add_studio_marker(&mut self) {
        let Some(replay) = self.replay.driver.as_mut() else {
            return;
        };
        let label = Text::translate(
            "horizon.studio.timeline.marker",
            &[Text::literal((replay.markers().len() + 1).to_string())],
        )
        .plain();
        match replay.add_marker(label) {
            Ok(_) => self.studio.timeline.markers = replay.markers().to_vec(),
            Err(error) => log::warn!("Studio: could not save replay marker: {error}"),
        }
    }

    /// `horizon-ctl studio screen <open|close|status>`.
    pub(super) fn studio_screen_control(&mut self, words: &[&str]) -> Result<Option<Value>> {
        match words.get(2).copied() {
            Some("open") => {
                let open = self.open_studio_screen();
                Ok(Some(
                    json!({ "ok": true, "open": open, "screen": if self.studio_screen_open() { "studio" } else { "playing" } }),
                ))
            }
            Some("close") => {
                let was_open = self.studio_screen_open();
                self.close_studio_screen();
                Ok(Some(
                    json!({ "ok": true, "was_open": was_open, "screen": if self.studio_screen_open() { "studio" } else { "playing" } }),
                ))
            }
            Some("status") => Ok(Some(
                json!({ "ok": true, "open": self.studio_screen_open(), "screen": if self.studio_screen_open() { "studio" } else { "playing" } }),
            )),
            _ => Ok(Some(json!({ "error": "unknown studio screen subcommand" }))),
        }
    }

    /// Saves the free camera's path into the replay playing, when it changed.
    pub(super) fn save_studio_camera_track(&mut self) -> std::io::Result<()> {
        if self.studio.camera_track_dirty
            && let Some(driver) = self.replay.driver.as_mut()
        {
            driver.save_camera_track(&self.studio.freecam.track)?;
            self.studio.camera_track_dirty = false;
        }
        Ok(())
    }

    /// A replay opened: the timeline shows it and the free camera takes its saved path.
    pub(super) fn studio_replay_opened(&mut self, driver: &ReplayDriver) {
        self.studio.timeline.duration_ms = u64::from(driver.duration_ms());
        self.studio.timeline.current_time_ms = 0;
        self.studio.timeline.playing = !driver.is_paused();
        self.studio.timeline.markers = driver.markers().to_vec();
        self.studio.freecam.track = driver.camera_track().cloned().unwrap_or_default();
        self.studio.camera_track_dirty = false;
    }

    /// The replay moved on: a camera locked to its path follows it, and the timeline shows where
    /// it is.
    pub(super) fn studio_replay_advanced(&mut self) {
        if self.studio.freecam.locked_to_path
            && let Some(position) = self
                .replay
                .driver
                .as_ref()
                .map(|replay| replay.position_ms())
        {
            self.studio.freecam.update_spline_playback(position);
        }
        if let Some((position, duration, paused, markers)) =
            self.replay.driver.as_ref().map(|replay| {
                (
                    replay.position_ms(),
                    replay.duration_ms(),
                    replay.is_paused(),
                    replay.markers().to_vec(),
                )
            })
        {
            self.studio.timeline.current_time_ms = u64::from(position);
            self.studio.timeline.duration_ms = u64::from(duration);
            self.studio.timeline.playing = !paused;
            self.studio.timeline.markers = markers;
        }
    }

    /// The replay closes: its camera path is saved (a failure is only logged).
    pub(super) fn studio_replay_closing(&mut self) {
        if let Err(error) = self.save_studio_camera_track() {
            log::warn!("Studio: could not save camera path: {error}");
        }
        self.studio.camera_track_dirty = false;
    }

    /// The replay closed: the timeline empties and the free camera lands.
    pub(super) fn studio_replay_closed(&mut self) {
        self.studio.timeline.current_time_ms = 0;
        self.studio.timeline.playing = false;
        self.studio.timeline.markers.clear();
        self.studio.timeline.split_points.clear();
        self.studio.timeline.selected_keyframe = None;
        self.studio.freecam.deactivate();
    }
}

/// What the engine's frame, render and window code hand to Studio.
impl Game<'_> {
    /// The top bar and the timeline over the HUD, while Studio is up in play.
    pub(super) fn draw_studio_bars(&mut self, batch: &mut super::HudBatch) {
        if self.ui.hide_gui || self.screen != Screen::Playing || !self.studio_up() {
            return;
        }
        self.studio.topbar.render(
            batch,
            &self.gui.atlas,
            self.cursor,
            self.studio.freecam.locked_to_path,
        );
        self.studio
            .timeline
            .render(batch, &self.gui.atlas, self.cursor, &self.studio.freecam);
    }

    /// The camera path among the debug lines, while Studio is up.
    pub(super) fn studio_camera_path(&self, gizmos: &mut horizon_client::gizmos::Gizmos) {
        if self.studio_up() {
            horizon_client::studio::render_camera_path(
                &self.studio.freecam.track,
                gizmos,
                None,
                100,
            );
        }
    }

    /// The pointer moved while the timeline's playhead or a keyframe is dragged: Studio took it.
    pub(super) fn studio_mouse_dragged(&mut self, width: f32) -> bool {
        if self.screen != Screen::Playing
            || !(self.studio.timeline.dragging_playhead
                || self.studio.timeline.dragging_keyframe.is_some())
        {
            return false;
        }
        let action =
            self.studio
                .timeline
                .handle_mouse_drag(self.cursor, width, &mut self.studio.freecam);
        if matches!(action, TimelineAction::KeyframeMoved) {
            self.studio.camera_track_dirty = true;
        }
        true
    }

    /// A mouse button while Studio is up: true when Studio took it (its bars, the right button's
    /// mouse look, every click while a replay plays).
    pub(super) fn studio_mouse_input(
        &mut self,
        pressed: bool,
        button: winit::event::MouseButton,
    ) -> bool {
        if !pressed && button == winit::event::MouseButton::Left {
            let action = self.studio.timeline.handle_mouse_up();
            if !matches!(action, TimelineAction::None) {
                self.handle_studio_timeline_action(action);
            }
        }
        if (self.studio.freecam.active || self.replay.driver.is_some())
            && button == winit::event::MouseButton::Right
        {
            if let Some(renderer) = &self.renderer {
                let batch = super::HudBatch::new([renderer.config.width, renderer.config.height]);
                if self.studio.timeline.visible
                    && self.cursor.1 >= batch.height - self.studio.timeline.height()
                {
                    if pressed
                        && let Some(index) = self.studio.timeline.keyframe_at_cursor(
                            self.cursor,
                            batch.width,
                            batch.height,
                            &self.studio.freecam,
                        )
                    {
                        if index < self.studio.freecam.track.keyframes.len() {
                            self.studio.freecam.track.keyframes.remove(index);
                            self.studio.camera_track_dirty = true;
                        }
                        self.studio.timeline.selected_keyframe = None;
                    }
                    return true;
                }
            }
            if pressed {
                self.studio.topbar.close_dropdown();
                self.studio.rmb_down = true;
                self.capture(true);
                return true;
            } else if self.studio.rmb_down {
                self.studio.rmb_down = false;
                self.capture(false);
                return true;
            }
        }
        if pressed
            && button == winit::event::MouseButton::Left
            && self.screen == Screen::Playing
            && (self.replay.driver.is_some() || self.studio.freecam.active)
        {
            let size = self
                .renderer
                .as_ref()
                .map(|renderer| [renderer.config.width, renderer.config.height])
                .unwrap_or([1, 1]);
            let batch = super::HudBatch::new(size);
            let had_open_menu = self.studio.topbar.open_menu.is_some();
            let topbar_action =
                self.studio
                    .topbar
                    .handle_mouse_click(self.cursor, batch.width, batch.height);
            if !matches!(topbar_action, horizon_client::studio::TopBarAction::None) {
                self.handle_studio_topbar_action(topbar_action);
                return true;
            }
            if had_open_menu || self.cursor.1 <= self.studio.topbar.height {
                return true;
            }
            if self.studio.timeline.visible {
                let action = self.studio.timeline.handle_mouse_down(
                    self.cursor,
                    batch.width,
                    batch.height,
                    &self.studio.freecam,
                );
                if !matches!(action, TimelineAction::None) {
                    self.handle_studio_timeline_action(action);
                    return true;
                }
                if self.cursor.1 >= batch.height - self.studio.timeline.height() {
                    return true;
                }
            }
        }
        if self.screen == Screen::Playing && self.replay.driver.is_some() {
            return true;
        }
        false
    }

    /// The free camera flies by the movement keys, once per frame.
    pub(super) fn studio_fly(&mut self, elapsed: Duration) {
        if !self.studio.flying() || self.screen != Screen::Playing || self.dialog_open() {
            return;
        }
        use winit::keyboard::KeyCode;
        let held = |key| f64::from(u8::from(self.keys.raw(&key)));
        let forward = held(KeyCode::KeyW) - held(KeyCode::KeyS);
        let strafe = held(KeyCode::KeyD) - held(KeyCode::KeyA);
        let vertical = held(KeyCode::KeyE) - held(KeyCode::KeyQ) + held(KeyCode::Space);
        let fast = self.keys.raw(&KeyCode::ControlRight);
        self.studio.freecam.update_flight_motion(
            forward,
            strafe,
            vertical,
            fast,
            elapsed.as_secs_f64().min(0.1),
        );
    }

    /// A key pressed while a replay plays: Studio's shortcuts, and nothing else reaches play.
    pub(super) fn studio_key(&mut self, key: winit::keyboard::KeyCode) {
        use winit::keyboard::KeyCode;
        match key {
            KeyCode::Escape => {
                self.close_studio_replay();
            }
            KeyCode::KeyP => {
                self.pause_focus = None;
                self.screen = Screen::Paused;
                self.pause_menu = true;
                self.capture(false);
            }
            KeyCode::KeyF => self.handle_studio_timeline_action(TimelineAction::ToggleFreecam),
            KeyCode::KeyT => self.studio.timeline.visible = !self.studio.timeline.visible,
            KeyCode::KeyM => self.add_studio_marker(),
            KeyCode::Home => self.handle_studio_topbar_action(
                horizon_client::studio::TopBarAction::ResetCameraToPlayer,
            ),
            KeyCode::KeyL => {
                self.studio.freecam.locked_to_path = !self.studio.freecam.locked_to_path;
            }
            KeyCode::KeyK => self.handle_studio_timeline_action(TimelineAction::AddCameraKeyframe),
            KeyCode::Delete => {
                if let Some(index) = self.studio.timeline.selected_keyframe {
                    self.handle_studio_timeline_action(TimelineAction::RemoveCameraKeyframe(index));
                }
            }
            _ => {}
        }
    }
}
