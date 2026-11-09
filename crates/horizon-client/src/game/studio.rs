//! In-game Video Studio: Recording & Replay Subsystem
//!
//! Provides in-game and control-socket commands to record network packet streams
//! into standard `.mcpr` containers and drive timeline-based replay playback.

use super::{Game, PendingWorldEntry, Screen, packs};
use horizon_client::{
    Result,
    studio::{Metadata, ReplayDriver},
    text::Text,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Studio's state in a `Game`: the replay playing, its free camera, timeline and top bar. Studio is
/// a Horizon extra (docs/layers.md: the replay editor); the engine only asks it whether a replay
/// plays or the free camera flies.
#[derive(Default)]
pub(super) struct Studio {
    /// The packet replay playing, if any.
    pub(super) replay: Option<ReplayDriver>,
    pub(super) freecam: horizon_client::studio::StudioFreecam,
    pub(super) timeline: horizon_client::studio::TimelineUi,
    pub(super) topbar: horizon_client::studio::TopBarUi,
    pub(super) rmb_down: bool,
    pub(super) camera_track_dirty: bool,
    /// Camera perspective to restore after leaving replay playback.
    pub(super) replay_previous_camera_type: Option<super::perspective::CameraType>,
    /// Replay opened from the title screen owns the synthetic remote session.
    pub(super) replay_standalone: bool,
    pub(super) last_replay_tick: Option<std::time::Instant>,
    pub(super) virtual_tick_ms: f32,
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

    /// In-game chat command handler: `/studio <record|replay> ...`.
    /// Returns true if the command was recognized and handled.
    pub(super) fn studio_command(&mut self, command: &str) -> bool {
        let Some(rest) = command.strip_prefix("studio") else {
            return false;
        };
        if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
            return false;
        }
        let words: Vec<&str> = rest.split_whitespace().collect();
        if words.is_empty() {
            self.system_message(Text::translate("horizon.studio.replay.usage", &[]));
            return true;
        }

        match words[0] {
            "freecam" => match words.get(1).copied() {
                Some("on") => {
                    if self.studio.replay.is_none() {
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
                let Some(driver) = self.studio.replay.as_ref() else {
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
            "record" => match words.get(1).copied() {
                Some("start") => match self.start_studio_recording(words.get(2).copied()) {
                    Ok(v) if v["ok"].as_bool() == Some(true) => {
                        let path = v["path"].as_str().unwrap_or_default();
                        self.system_message(Text::tr("horizon.studio.record.started", &[&path]));
                    }
                    Ok(v) => {
                        let error = v["error"].as_str().unwrap_or_default();
                        self.system_message(Text::tr("horizon.studio.record.error", &[&error]));
                    }
                    Err(error) => {
                        self.system_message(Text::tr("horizon.studio.record.error", &[&error]));
                    }
                },
                Some("stop") => match self.stop_studio_recording() {
                    Ok(v) if v["ok"].as_bool() == Some(true) => {
                        let path = v["path"].as_str().unwrap_or_default();
                        self.system_message(Text::tr("horizon.studio.record.saved", &[&path]));
                    }
                    Ok(v) => {
                        let error = v["error"].as_str().unwrap_or_default();
                        self.system_message(Text::tr("horizon.studio.record.error", &[&error]));
                    }
                    Err(error) => {
                        self.system_message(Text::tr("horizon.studio.record.error", &[&error]));
                    }
                },
                Some("status") => {
                    let status = self.studio_recording_status();
                    if status["active"].as_bool() == Some(true) {
                        let elapsed =
                            (status["elapsed_ms"].as_u64().unwrap_or(0) / 1000).to_string();
                        let packets = status["packet_count"].as_u64().unwrap_or(0).to_string();
                        let size =
                            (status["bytes_written"].as_u64().unwrap_or(0) / 1024).to_string();
                        let path = status["path"].as_str().unwrap_or_default();
                        self.system_message(Text::tr(
                            "horizon.studio.record.status.active",
                            &[&elapsed, &packets, &size, &path],
                        ));
                    } else if let Some(error) = status["error"].as_str() {
                        self.system_message(Text::tr("horizon.studio.record.error", &[&error]));
                    } else {
                        self.system_message(Text::translate(
                            "horizon.studio.record.status.inactive",
                            &[],
                        ));
                    }
                }
                _ => self.system_message(Text::translate("horizon.studio.record.usage", &[])),
            },
            "replay" => match words.get(1).copied() {
                Some("open") => {
                    let Some(path) = words.get(2) else {
                        self.system_message(Text::translate(
                            "horizon.studio.replay.open.usage",
                            &[],
                        ));
                        return true;
                    };
                    match self.open_studio_replay(path) {
                        Ok(v) if v["ok"].as_bool() == Some(true) => {
                            let duration_s = v["duration_ms"].as_f64().unwrap_or(0.0) / 1000.0;
                            let packets = v["packet_count"].as_u64().unwrap_or(0);
                            let duration = format!("{duration_s:.1}");
                            let packets = packets.to_string();
                            self.system_message(Text::tr(
                                "horizon.studio.replay.opened",
                                &[path, &duration, &packets],
                            ));
                        }
                        Ok(v) => {
                            let err = v["error"].as_str().unwrap_or_default();
                            self.system_message(Text::tr(
                                "horizon.studio.replay.open_error",
                                &[&err],
                            ));
                        }
                        Err(e) => {
                            let error = e.to_string();
                            self.system_message(Text::tr(
                                "horizon.studio.replay.open_error",
                                &[&error],
                            ));
                        }
                    }
                }
                Some("pause") => {
                    if let Some(r) = self.studio.replay.as_mut() {
                        r.set_paused(true);
                        self.system_message(Text::translate("horizon.studio.replay.paused", &[]));
                    } else {
                        self.system_message(Text::translate("horizon.studio.replay.inactive", &[]));
                    }
                }
                Some("resume") => {
                    if let Some(r) = self.studio.replay.as_mut() {
                        r.set_paused(false);
                        self.system_message(Text::translate("horizon.studio.replay.resumed", &[]));
                    } else {
                        self.system_message(Text::translate("horizon.studio.replay.inactive", &[]));
                    }
                }
                Some("speed") => {
                    let Some(spd_str) = words.get(2) else {
                        self.system_message(Text::translate(
                            "horizon.studio.replay.speed.usage",
                            &[],
                        ));
                        return true;
                    };
                    if let Ok(spd) = spd_str.parse::<f64>() {
                        let new_speed = if let Some(r) = self.studio.replay.as_mut() {
                            r.set_speed(spd);
                            Some(r.speed())
                        } else {
                            None
                        };
                        if let Some(speed) = new_speed {
                            let speed = format!("{speed:.2}");
                            self.system_message(Text::tr(
                                "horizon.studio.replay.speed.set",
                                &[&speed],
                            ));
                        } else {
                            self.system_message(Text::translate(
                                "horizon.studio.replay.inactive",
                                &[],
                            ));
                        }
                    } else {
                        self.system_message(Text::translate(
                            "horizon.studio.replay.speed.invalid",
                            &[],
                        ));
                    }
                }
                Some("seek") => {
                    let Some(ms_str) = words.get(2) else {
                        self.system_message(Text::translate(
                            "horizon.studio.replay.seek.usage",
                            &[],
                        ));
                        return true;
                    };
                    if let Ok(ms) = ms_str.parse::<u32>() {
                        if self.studio.replay.is_some() {
                            match self.seek_studio_replay(ms) {
                                Ok(()) => {
                                    let actual_ms = self
                                        .studio
                                        .replay
                                        .as_ref()
                                        .map_or(ms, |replay| ms.min(replay.duration_ms()));
                                    let ms_text = actual_ms.to_string();
                                    let seconds = format!("{:.1}", actual_ms as f32 / 1000.0);
                                    self.system_message(Text::tr(
                                        "horizon.studio.replay.seeked",
                                        &[&ms_text, &seconds],
                                    ));
                                }
                                Err(e) => {
                                    if e.kind() == std::io::ErrorKind::Unsupported {
                                        self.system_message(Text::translate(
                                            "horizon.studio.replay.seek.backward_requires_standalone",
                                            &[],
                                        ));
                                    } else {
                                        let error = e.to_string();
                                        self.system_message(Text::tr(
                                            "horizon.studio.replay.seek.error",
                                            &[&error],
                                        ));
                                    }
                                }
                            }
                        } else {
                            self.system_message(Text::translate(
                                "horizon.studio.replay.inactive",
                                &[],
                            ));
                        }
                    } else {
                        self.system_message(Text::translate(
                            "horizon.studio.replay.seek.invalid",
                            &[],
                        ));
                    }
                }
                Some("status") => {
                    if let Some(r) = self.studio.replay.as_ref() {
                        let position = r.position_ms().to_string();
                        let duration = r.duration_ms().to_string();
                        let progress = format!("{:.1}", r.progress() * 100.0);
                        let speed = format!("{:.2}", r.speed());
                        let paused = r.is_paused().to_string();
                        let finished = r.is_finished().to_string();
                        self.system_message(Text::tr(
                            "horizon.studio.replay.status",
                            &[&position, &duration, &progress, &speed, &paused, &finished],
                        ));
                    } else {
                        self.system_message(Text::translate("horizon.studio.replay.inactive", &[]));
                    }
                }
                Some("close") => {
                    if self.close_studio_replay() {
                        self.system_message(Text::translate("horizon.studio.replay.closed", &[]));
                    } else {
                        self.system_message(Text::translate("horizon.studio.replay.inactive", &[]));
                    }
                }
                _ => {
                    self.system_message(Text::translate("horizon.studio.replay.usage", &[]));
                }
            },
            _ => {
                self.system_message(Text::translate("horizon.studio.replay.usage", &[]));
            }
        }
        true
    }

    /// Reset playback tick accumulator and align positions on seek.
    pub(super) fn reset_studio_playback_ticks(&mut self) {
        self.studio.virtual_tick_ms = 0.0;
        if let Some(player) = &self.player {
            self.previous_position = player.position;
            if let Some(m) = self.player_model.as_mut() {
                m.previous = player.position;
                m.position = player.position;
                m.limb_speed = 0.0;
                m.previous_limb_speed = 0.0;
            }
        }
    }

    /// Reconstruct replay state at a target timestamp. Standalone replays can
    /// safely reset their synthetic remote; an in-world replay can only seek
    /// forward because its pre-replay world must remain intact.
    pub(super) fn seek_studio_replay(&mut self, target_ms: u32) -> std::io::Result<()> {
        let current_ms = self
            .studio
            .replay
            .as_ref()
            .map_or(0, ReplayDriver::position_ms);
        let packets = if self.studio.replay_standalone {
            let packets = self
                .studio
                .replay
                .as_mut()
                .expect("replay checked by caller")
                .seek_and_collect(target_ms)?;
            self.init_remote_for_replay()
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            packets
        } else {
            if target_ms < current_ms {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "backward seek requires a standalone replay",
                ));
            }
            self.studio
                .replay
                .as_mut()
                .expect("replay checked by caller")
                .seek_forward(target_ms)?
        };

        self.dispatch_studio_replay_packets(packets);
        self.reset_studio_playback_ticks();
        self.studio.last_replay_tick = Some(std::time::Instant::now());
        Ok(())
    }

    fn dispatch_studio_replay_packets(
        &mut self,
        packets: Vec<horizon_client::studio::mcpr::TimestampedPacket>,
    ) {
        for pkt in packets {
            match horizon_protocol::packets::play::Clientbound::decode(pkt.id, &pkt.payload) {
                Ok(packet) => {
                    let _ = self.handle_packet(packet);
                }
                Err(e) => {
                    let name = horizon_protocol::ids::name(
                        horizon_protocol::State::Play,
                        horizon_protocol::Direction::Clientbound,
                        pkt.id,
                    )
                    .unwrap_or("?");
                    log::debug!(
                        "Studio replay skipped packet {name} (id {:#x}): {e}",
                        pkt.id
                    );
                }
            }
        }
    }

    /// Advance active replay playback by elapsed frame time and dispatch ready packets.
    pub(super) fn advance_studio_replay(&mut self) -> Result<()> {
        let Some(driver) = self.studio.replay.as_mut() else {
            self.studio.last_replay_tick = None;
            return Ok(());
        };

        let now = std::time::Instant::now();
        let delta = match self.studio.last_replay_tick {
            Some(last) => now
                .saturating_duration_since(last)
                .min(Duration::from_millis(250)),
            None => Duration::from_millis(16),
        };
        self.studio.last_replay_tick = Some(now);

        let ready_packets = match driver.advance(delta) {
            Ok(pkts) => pkts,
            Err(e) => {
                log::warn!("Studio replay advance error: {e}");
                return Ok(());
            }
        };

        self.dispatch_studio_replay_packets(ready_packets);
        if self.studio.freecam.locked_to_path
            && let Some(position) = self
                .studio
                .replay
                .as_ref()
                .map(|replay| replay.position_ms())
        {
            self.studio.freecam.update_spline_playback(position);
        }
        let paused = self
            .studio
            .replay
            .as_ref()
            .is_some_and(|replay| replay.is_paused());
        if self.player_model.is_none() {
            self.tick_player_model();
        }
        if !paused {
            let speed = self
                .studio
                .replay
                .as_ref()
                .map_or(1.0, |r| r.speed() as f32)
                .max(0.1);
            self.studio.virtual_tick_ms += delta.as_secs_f32() * 1000.0 * speed;
            while self.studio.virtual_tick_ms >= 50.0 {
                self.studio.virtual_tick_ms -= 50.0;
                if let Some(player) = &self.player {
                    let current_pos = player.position;
                    let (dx, dz) = (
                        current_pos.x - self.previous_position.x,
                        current_pos.z - self.previous_position.z,
                    );
                    let walked = (dx * dx + dz * dz).sqrt() as f32;
                    if let Some(m) = self.player_model.as_mut() {
                        m.previous = self.previous_position;
                        m.position = current_pos;
                        m.previous_limb_speed = m.limb_speed;
                        let target_speed = (walked * 4.0).min(1.0);
                        m.limb_speed += (target_speed - m.limb_speed) * 0.4;
                        if m.limb_speed < 0.001 {
                            m.limb_speed = 0.0;
                        }
                        m.limb_position += m.limb_speed;
                        m.previous_yaw = m.yaw;
                        m.previous_head_yaw = m.head_yaw;
                        m.previous_head_pitch = m.head_pitch;
                        m.previous_body_yaw = m.body_yaw;
                        m.yaw = self.yaw;
                        m.head_yaw = self.yaw;
                        m.head_pitch = self.pitch;
                        let mut target = m.body_yaw;
                        if dx * dx + dz * dz > 0.0025 {
                            let walk = (dz.atan2(dx) as f32).to_degrees() - 90.0;
                            let facing =
                                ((self.yaw - walk + 180.0).rem_euclid(360.0) - 180.0).abs();
                            target = if 95.0 < facing && facing < 265.0 {
                                walk - 180.0
                            } else {
                                walk
                            };
                        }
                        let diff = (target - m.body_yaw + 180.0).rem_euclid(360.0) - 180.0;
                        m.body_yaw += diff * 0.3;
                        let body_diff = (self.yaw - m.body_yaw + 180.0).rem_euclid(360.0) - 180.0;
                        if body_diff.abs() > 50.0 {
                            m.body_yaw += body_diff - body_diff.signum() * 50.0;
                        }
                        m.age = m.age.wrapping_add(1);
                    }
                    self.previous_position = current_pos;
                }
            }
        } else if let Some(m) = self.player_model.as_mut() {
            m.previous_limb_speed = m.limb_speed;
        }
        if let Some((position, duration, paused, markers)) =
            self.studio.replay.as_ref().map(|replay| {
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
        Ok(())
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
                if self.studio.replay.is_some() {
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
                if let Some(replay) = self.studio.replay.as_mut() {
                    replay.set_paused(true);
                }
            }
            TimelineAction::Seek(time_ms) => {
                let time_ms = if self.studio.replay_standalone {
                    time_ms
                } else {
                    let current =
                        self.studio
                            .replay
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
                if let Some(replay) = self.studio.replay.as_mut() {
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
                if let Some(replay) = self.studio.replay.as_mut() {
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
        let Some(replay) = self.studio.replay.as_mut() else {
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

    /// Control socket command handler for `studio`.
    pub(super) fn studio_control(&mut self, words: &[&str]) -> Result<Option<Value>> {
        if words.first().copied() != Some("studio") {
            return Ok(None);
        }
        match words.get(1).copied() {
            Some("record") => match words.get(2).copied() {
                Some("start") => {
                    let name = words.get(3).copied();
                    let res = self.start_studio_recording(name)?;
                    Ok(Some(res))
                }
                Some("stop") => {
                    let res = self.stop_studio_recording()?;
                    Ok(Some(res))
                }
                Some("status") => {
                    let res = self.studio_recording_status();
                    Ok(Some(res))
                }
                _ => Ok(Some(json!({ "error": "unknown studio record subcommand" }))),
            },
            Some("replay") => match words.get(2).copied() {
                Some("open") => {
                    let path = words.get(3).ok_or("missing path for studio replay open")?;
                    let res = self.open_studio_replay(path)?;
                    Ok(Some(res))
                }
                Some("pause") => {
                    if let Some(r) = self.studio.replay.as_mut() {
                        r.set_paused(true);
                        Ok(Some(json!({ "ok": true, "paused": true })))
                    } else {
                        Ok(Some(json!({ "error": "no active replay" })))
                    }
                }
                Some("resume") => {
                    if let Some(r) = self.studio.replay.as_mut() {
                        r.set_paused(false);
                        Ok(Some(json!({ "ok": true, "paused": false })))
                    } else {
                        Ok(Some(json!({ "error": "no active replay" })))
                    }
                }
                Some("speed") => {
                    let spd_str = words.get(3).ok_or("missing speed value")?;
                    let spd: f64 = spd_str.parse().map_err(|_| "invalid speed")?;
                    if let Some(r) = self.studio.replay.as_mut() {
                        r.set_speed(spd);
                        Ok(Some(json!({ "ok": true, "speed": r.speed() })))
                    } else {
                        Ok(Some(json!({ "error": "no active replay" })))
                    }
                }
                Some("seek") => {
                    let ms_str = words.get(3).ok_or("missing seek timestamp")?;
                    let ms: u32 = ms_str.parse().map_err(|_| "invalid timestamp")?;
                    let pos = if let Some(r) = self.studio.replay.as_mut() {
                        r.seek(ms).map_err(|e| format!("{e}"))?;
                        Some(r.position_ms())
                    } else {
                        None
                    };
                    if let Some(pos) = pos {
                        self.reset_studio_playback_ticks();
                        Ok(Some(json!({ "ok": true, "position_ms": pos })))
                    } else {
                        Ok(Some(json!({ "error": "no active replay" })))
                    }
                }
                Some("status") => {
                    if let Some(r) = self.studio.replay.as_ref() {
                        Ok(Some(json!({
                            "ok": true,
                            "active": true,
                            "position_ms": r.position_ms(),
                            "duration_ms": r.duration_ms(),
                            "progress": r.progress(),
                            "speed": r.speed(),
                            "paused": r.is_paused(),
                            "finished": r.is_finished(),
                        })))
                    } else {
                        Ok(Some(json!({ "ok": true, "active": false })))
                    }
                }
                Some("close") => {
                    let closed = self.close_studio_replay();
                    Ok(Some(json!({ "ok": true, "closed": closed })))
                }
                _ => Ok(Some(json!({ "error": "unknown studio replay subcommand" }))),
            },
            Some("screen") => match words.get(2).copied() {
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
            },
            _ => Ok(Some(json!({ "error": "unknown studio command" }))),
        }
    }

    pub(super) fn start_studio_recording(&mut self, name: Option<&str>) -> Result<Value> {
        let Some(r) = self.remote.as_ref() else {
            return Ok(json!({ "ok": false, "error": "no active world session" }));
        };
        if r.session.is_recording() {
            return Ok(json!({ "ok": false, "error": "recording already active" }));
        }
        let _ = std::fs::create_dir_all("recordings");
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let filename = match name {
            Some(n) if !n.is_empty() => format!("{}_{}.mcpr", n, ts),
            _ => format!("recording_{}.mcpr", ts),
        };
        let path = PathBuf::from("recordings").join(filename);
        let server_address = if r.session.address.is_empty() {
            "local".to_owned()
        } else {
            r.session.address.clone()
        };
        let is_singleplayer = server_address == "local";
        let meta = Metadata::new(
            server_address,
            is_singleplayer,
            horizon_core::release::VERSION.into(),
        );
        match horizon_client::studio::StudioRecorder::start(path.clone(), meta) {
            Ok(recorder) => {
                r.session.start_recording(recorder, |recorder| {
                    self.dump_world_snapshot_to_recorder(recorder);
                });
                Ok(json!({ "ok": true, "path": path.display().to_string() }))
            }
            Err(e) => Ok(json!({ "ok": false, "error": e.to_string() })),
        }
    }

    pub(super) fn stop_studio_recording(&mut self) -> Result<Value> {
        let Some(r) = self.remote.as_ref() else {
            return Ok(json!({ "ok": false, "error": "no active world session" }));
        };
        match r.session.stop_recording() {
            Some(Ok(path)) => Ok(json!({ "ok": true, "path": path.display().to_string() })),
            Some(Err(e)) => Ok(json!({ "ok": false, "error": e.to_string() })),
            None => Ok(json!({ "ok": false, "error": "no active recording" })),
        }
    }

    pub(super) fn studio_recording_status(&self) -> Value {
        if let Some(r) = self.remote.as_ref()
            && let Some(status) = r.session.recorder_status()
        {
            json!({
                "ok": true,
                "active": status.active,
                "path": status.path.display().to_string(),
                "elapsed_ms": status.elapsed.as_millis(),
                "packet_count": status.packet_count,
                "bytes_written": status.bytes_written,
            })
        } else {
            json!({ "ok": true, "active": false })
        }
    }

    pub(super) fn open_studio_replay(&mut self, path: &str) -> Result<Value> {
        if self.studio.camera_track_dirty
            && let Some(active) = self.studio.replay.as_mut()
        {
            if let Err(error) = active.save_camera_track(&self.studio.freecam.track) {
                return Ok(json!({ "ok": false, "error": error.to_string() }));
            }
            self.studio.camera_track_dirty = false;
        }
        // The recordings browser can be opened while the title screen is
        // available but the slower world tables are still loading. Replay
        // setup builds the normal client registries, which require `rest`.
        if self.remote.is_none() && self.res.rest.get().is_none() {
            let pending = PendingWorldEntry::StudioReplay(PathBuf::from(path));
            if self.defer_world_entry(pending)? {
                return Ok(json!({ "ok": true, "pending": true }));
            }
            self.join_world_rest()?;
        }

        // Validate and index the archive before replacing the title menu or active world. A
        // temp-file, metadata, or protocol error must leave the current screen usable.
        let driver = match ReplayDriver::open(Path::new(path)) {
            Ok(driver) => driver,
            Err(error) => {
                log::error!("Failed to open studio replay '{path}': {error}");
                return Ok(json!({ "ok": false, "error": error.to_string() }));
            }
        };

        let standalone = self.studio.replay.is_none() && self.remote.is_none();
        if self.remote.is_none()
            && let Err(error) = self.init_remote_for_replay()
        {
            log::error!("Failed to initialize studio replay session: {error}");
            return Ok(json!({ "ok": false, "error": error.to_string() }));
        }

        let duration_ms = driver.duration_ms();
        let packets = driver.packet_count();
        self.studio.timeline.duration_ms = u64::from(duration_ms);
        self.studio.timeline.current_time_ms = 0;
        self.studio.timeline.playing = !driver.is_paused();
        self.studio.timeline.markers = driver.markers().to_vec();
        self.studio.freecam.track = driver.camera_track().cloned().unwrap_or_default();
        self.studio.camera_track_dirty = false;
        if self.studio.replay.is_none() {
            self.studio.replay_previous_camera_type = Some(self.camera_type);
            self.studio.replay_standalone = standalone;
        }
        self.studio.replay = Some(driver);
        self.camera_type = super::perspective::CameraType::FirstPerson;
        self.title = None;
        self.screen = Screen::Playing;
        self.capture(false);
        self.dragging = false;
        // Apply timestamp-zero login, player pose, and chunk packets before the first rendered
        // frame so playback opens at the recorded player's viewpoint.
        let _ = self.advance_studio_replay();
        self.reset_studio_playback_ticks();
        log::info!("Studio: Opened replay '{path}' ({duration_ms} ms, {packets} packets)");
        Ok(json!({
            "ok": true,
            "duration_ms": duration_ms,
            "packet_count": packets,
        }))
    }
    pub(super) fn close_studio_replay(&mut self) -> bool {
        if self.studio.camera_track_dirty
            && let Some(driver) = self.studio.replay.as_mut()
            && let Err(error) = driver.save_camera_track(&self.studio.freecam.track)
        {
            log::warn!("Studio: could not save camera path: {error}");
        }
        self.studio.camera_track_dirty = false;
        let closed = self.studio.replay.take().is_some();
        if !closed {
            return false;
        }

        // A standalone replay owns a synthetic Remote connection. Treat that
        // connection as replay state even if the saved origin flag was lost;
        // sending it through leave_server() opens the Multiplayer screen.
        let standalone = self.studio.replay_standalone
            || self
                .remote
                .as_ref()
                .is_some_and(|remote| remote.brand == "horizon-studio");
        self.studio.replay_standalone = false;
        if let Some(camera_type) = self.studio.replay_previous_camera_type.take() {
            self.camera_type = camera_type;
        }
        self.studio.last_replay_tick = None;
        self.studio.timeline.current_time_ms = 0;
        self.studio.timeline.playing = false;
        self.studio.timeline.markers.clear();
        self.studio.timeline.split_points.clear();
        self.studio.timeline.selected_keyframe = None;
        self.studio.freecam.deactivate();
        self.studio.virtual_tick_ms = 0.0;
        self.capture(false);
        if standalone {
            if let Some(mut remote) = self.remote.take() {
                remote.session.disconnect();
            }
            self.using = None;
            self.end_mod_use();
            if let Err(error) = self.finish_close_world() {
                log::error!("Failed to close standalone Studio replay cleanly: {error}");
            }
            self.reset_studio_hand_state();
            // A title-screen replay has no previous world perspective to
            // restore. Start the next world in first person with visible hands.
            self.camera_type = super::perspective::CameraType::FirstPerson;
            if let Some(title) = self.title.as_mut() {
                title.mode = horizon_client::title::Mode::Main;
            }
        } else if self.integrated.is_none() && self.lan.is_none() {
            let _ = self.close_world();
            if let Some(title) = self.title.as_mut() {
                title.mode = horizon_client::title::Mode::Main;
            }
        }
        closed
    }
    /// Dump current player pose, dimension login, center chunk, and resident loaded chunks
    /// into the recording stream at timestamp 0ms so replays accurately reconstruct the world.
    pub(super) fn dump_world_snapshot_to_recorder(
        &self,
        recorder: &horizon_client::studio::StudioRecorder,
    ) {
        let Some(r) = self.remote.as_ref() else {
            return;
        };
        let record_cb = |pkt: horizon_protocol::packets::play::Clientbound| {
            let encoded = pkt.encode();
            let mut reader = horizon_protocol::buf::Reader::new(&encoded);
            if let Ok(id) = reader.varint() {
                let payload = reader.rest();
                recorder.record_packet_at(0, id, payload);
            }
        };

        // 1. C::Login packet baseline
        let login = horizon_protocol::packets::play::Clientbound::Login(
            horizon_protocol::packets::play::Login {
                player_id: r.entity_id,
                hardcore: self.level.hardcore,
                levels: vec!["minecraft:overworld".into()],
                max_players: 20,
                chunk_radius: self.settings.render_distance,
                simulation_distance: self.settings.simulation_distance,
                reduced_debug_info: false,
                show_death_screen: true,
                limited_crafting: false,
                spawn: horizon_protocol::packets::play::SpawnInfo {
                    dimension_type: 0,
                    dimension: "minecraft:overworld".into(),
                    seed: 0,
                    game_type: 0,
                    previous_game_type: None,
                    debug: false,
                    flat: false,
                    last_death: None,
                    portal_cooldown: 0,
                    sea_level: 63,
                },
                online_mode: false,
                enforces_secure_chat: false,
            },
        );
        record_cb(login);

        // 2. Center chunk and player position
        let (pos, yaw, pitch) = if let Some(p) = &self.player {
            (
                glam::DVec3::new(p.position.x, p.position.y, p.position.z),
                self.yaw,
                self.pitch,
            )
        } else {
            (glam::DVec3::ZERO, self.yaw, self.pitch)
        };
        let chunk_x = (pos.x.floor() as i32) >> 4;
        let chunk_z = (pos.z.floor() as i32) >> 4;

        let center_chunk = horizon_protocol::packets::play::Clientbound::SetChunkCacheCenter(
            horizon_protocol::packets::play::SetChunkCacheCenter {
                x: chunk_x,
                z: chunk_z,
            },
        );
        record_cb(center_chunk);

        let player_pos = horizon_protocol::packets::play::Clientbound::PlayerPosition(
            horizon_protocol::packets::play::PlayerPosition {
                teleport: 0,
                position: [pos.x, pos.y, pos.z],
                delta: [0.0, 0.0, 0.0],
                yaw,
                pitch,
                relatives: 0,
            },
        );
        record_cb(player_pos);

        // 3. Encode resident loaded chunks into LevelChunkWithLight packets
        let biome_names: Vec<String> = world_biomes!(self)
            .list
            .iter()
            .map(|b| b.name.clone())
            .collect();
        let registry: Vec<String> = r
            .registries
            .registries
            .get("minecraft:worldgen/biome")
            .map(|v| v.iter().map(|(n, _)| n.clone()).collect())
            .unwrap_or_else(|| biome_names.clone());

        let encoder = crate::lan::chunks::ChunkEncoder::with_signs(
            self.res.facts().states.iter().map(|s| {
                (
                    s.name.clone(),
                    s.fluid_level > 0 || s.waterlogged,
                    s.sign_form(),
                )
            }),
            &r.builtin,
            &biome_names,
            &registry,
        );

        let mut count = 0;
        for chunk in self.world.chunks.values() {
            let level_chunk = encoder.encode(chunk, &self.world.block_entities);
            record_cb(
                horizon_protocol::packets::play::Clientbound::LevelChunkWithLight(level_chunk),
            );
            count += 1;
        }
        log::info!(
            "Studio Recorder: Dumped initial world snapshot ({count} chunks) at timestamp 0ms"
        );
    }
}
