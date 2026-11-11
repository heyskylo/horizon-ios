//! Recording a session's packets as `.mcpr` and playing them back in a `Game`: the `/studio record`
//! and `/studio replay` commands (chat and `horizon-ctl`), the replay's clock and its world. This is
//! platform (docs/layers.md, "Replays"); Studio's editor (`studio.rs`) builds on it.

use super::studio::studio_recording_metadata;
use super::{Game, PendingWorldEntry, Screen};
use horizon_client::{
    Result,
    studio::{ReplayDriver, generate_recording_filename},
    text::Text,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

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
            "freecam" | "keyframe" | "path" if super::studio::EDITOR => {
                return self.studio_editor_command(&words);
            }
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
                        self.system_message(Text::tr("horizon.studio.record.saving", &[&path]));
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
                    if status["finishing"].as_bool() == Some(true) {
                        let path = status["path"].as_str().unwrap_or_default();
                        self.system_message(Text::tr("horizon.studio.record.saving", &[&path]));
                    } else if status["active"].as_bool() == Some(true) {
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
                    if let Some(r) = self.replay.driver.as_mut() {
                        r.set_paused(true);
                        self.system_message(Text::translate("horizon.studio.replay.paused", &[]));
                    } else {
                        self.system_message(Text::translate("horizon.studio.replay.inactive", &[]));
                    }
                }
                Some("resume") => {
                    if let Some(r) = self.replay.driver.as_mut() {
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
                        let new_speed = if let Some(r) = self.replay.driver.as_mut() {
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
                        if self.replay.driver.is_some() {
                            match self.seek_studio_replay(ms) {
                                Ok(()) => {
                                    let actual_ms = self
                                        .replay
                                        .driver
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
                    if let Some(r) = self.replay.driver.as_ref() {
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
        self.replay.virtual_tick_ms = 0.0;
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
            .replay
            .driver
            .as_ref()
            .map_or(0, ReplayDriver::position_ms);
        let packets = if self.replay.standalone {
            let packets = self
                .replay
                .driver
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
            self.replay
                .driver
                .as_mut()
                .expect("replay checked by caller")
                .seek_forward(target_ms)?
        };

        self.dispatch_studio_replay_packets(packets);
        self.reset_studio_playback_ticks();
        self.replay.last_tick = Some(std::time::Instant::now());
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
        let Some(driver) = self.replay.driver.as_mut() else {
            self.replay.last_tick = None;
            return Ok(());
        };

        let now = std::time::Instant::now();
        let delta = match self.replay.last_tick {
            Some(last) => now
                .saturating_duration_since(last)
                .min(Duration::from_millis(250)),
            None => Duration::from_millis(16),
        };
        self.replay.last_tick = Some(now);

        let ready_packets = match driver.advance(delta) {
            Ok(pkts) => pkts,
            Err(e) => {
                log::warn!("Studio replay advance error: {e}");
                return Ok(());
            }
        };

        self.dispatch_studio_replay_packets(ready_packets);
        let paused = self
            .replay
            .driver
            .as_ref()
            .is_some_and(|replay| replay.is_paused());
        if self.player_model.is_none() {
            self.tick_player_model();
        }
        if !paused {
            let speed = self
                .replay
                .driver
                .as_ref()
                .map_or(1.0, |r| r.speed() as f32)
                .max(0.1);
            self.replay.virtual_tick_ms += delta.as_secs_f32() * 1000.0 * speed;
            while self.replay.virtual_tick_ms >= 50.0 {
                self.replay.virtual_tick_ms -= 50.0;
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
        self.studio_replay_advanced();
        Ok(())
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
                    if let Some(r) = self.replay.driver.as_mut() {
                        r.set_paused(true);
                        Ok(Some(json!({ "ok": true, "paused": true })))
                    } else {
                        Ok(Some(json!({ "error": "no active replay" })))
                    }
                }
                Some("resume") => {
                    if let Some(r) = self.replay.driver.as_mut() {
                        r.set_paused(false);
                        Ok(Some(json!({ "ok": true, "paused": false })))
                    } else {
                        Ok(Some(json!({ "error": "no active replay" })))
                    }
                }
                Some("speed") => {
                    let spd_str = words.get(3).ok_or("missing speed value")?;
                    let spd: f64 = spd_str.parse().map_err(|_| "invalid speed")?;
                    if let Some(r) = self.replay.driver.as_mut() {
                        r.set_speed(spd);
                        Ok(Some(json!({ "ok": true, "speed": r.speed() })))
                    } else {
                        Ok(Some(json!({ "error": "no active replay" })))
                    }
                }
                Some("seek") => {
                    let ms_str = words.get(3).ok_or("missing seek timestamp")?;
                    let ms: u32 = ms_str.parse().map_err(|_| "invalid timestamp")?;
                    let pos = if let Some(r) = self.replay.driver.as_mut() {
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
                    if let Some(r) = self.replay.driver.as_ref() {
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
                        Ok(Some(json!({ "ok": true, "active": false, "error": null })))
                    }
                }
                Some("close") => {
                    let closed = self.close_studio_replay();
                    Ok(Some(json!({ "ok": true, "closed": closed })))
                }
                _ => Ok(Some(json!({ "error": "unknown studio replay subcommand" }))),
            },
            Some("screen") if super::studio::EDITOR => self.studio_screen_control(words),
            _ => Ok(Some(json!({ "error": "unknown studio command" }))),
        }
    }

    pub(super) fn start_studio_recording(&mut self, name: Option<&str>) -> Result<Value> {
        let Some(r) = self.remote.as_ref() else {
            return Ok(json!({ "ok": false, "error": "no active world session" }));
        };
        if r.session.recorder_status().is_some() {
            return Ok(json!({ "ok": false, "error": "recording already active" }));
        }
        let recordings = PathBuf::from("recordings");
        std::fs::create_dir_all(&recordings)?;
        let path = recordings.join(generate_recording_filename(name));
        let is_singleplayer = r.session.address.is_empty() || r.session.address == "local";
        let metadata = studio_recording_metadata(is_singleplayer);
        let recorder = horizon_client::studio::StudioRecorder::start(path.clone(), metadata);
        match recorder {
            Ok(recorder) => {
                if let Err(error) = r.session.start_recording(recorder, |recorder| {
                    self.dump_world_snapshot_to_recorder(recorder);
                }) {
                    return Ok(json!({ "ok": false, "error": error.to_string() }));
                }
                self.studio_recording_notice_pending = false;
                Ok(json!({ "ok": true, "path": path.display().to_string() }))
            }
            Err(error) => Ok(json!({ "ok": false, "error": error.to_string() })),
        }
    }

    pub(super) fn stop_studio_recording(&mut self) -> Result<Value> {
        let Some(r) = self.remote.as_ref() else {
            return Ok(json!({ "ok": false, "error": "no active world session" }));
        };
        match r.session.request_recording_stop() {
            Some(path) => {
                self.studio_recording_notice_pending = true;
                Ok(json!({ "ok": true, "finishing": true, "path": path.display().to_string() }))
            }
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
                "finishing": status.finishing,
                "path": status.path.display().to_string(),
                "elapsed_ms": status.elapsed.as_millis(),
                "packet_count": status.packet_count,
                "bytes_written": status.bytes_written,
                "error": status.error,
            })
        } else {
            json!({ "ok": true, "active": false })
        }
    }

    pub(super) fn open_studio_replay(&mut self, path: &str) -> Result<Value> {
        if let Err(error) = self.save_studio_camera_track() {
            return Ok(json!({ "ok": false, "error": error.to_string() }));
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

        let standalone = self.replay.driver.is_none() && self.remote.is_none();
        if self.remote.is_none()
            && let Err(error) = self.init_remote_for_replay()
        {
            log::error!("Failed to initialize studio replay session: {error}");
            return Ok(json!({ "ok": false, "error": error.to_string() }));
        }

        let duration_ms = driver.duration_ms();
        let packets = driver.packet_count();
        self.studio_replay_opened(&driver);
        if self.replay.driver.is_none() {
            self.replay.previous_camera_type = Some(self.camera_type);
            self.replay.standalone = standalone;
        }
        self.replay.driver = Some(driver);
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
        self.studio_replay_closing();
        let closed = self.replay.driver.take().is_some();
        if !closed {
            return false;
        }

        // A standalone replay owns a synthetic Remote connection. Treat that
        // connection as replay state even if the saved origin flag was lost;
        // sending it through leave_server() opens the Multiplayer screen.
        let standalone = self.replay.standalone
            || self
                .remote
                .as_ref()
                .is_some_and(|remote| remote.brand == "horizon-studio");
        self.replay.standalone = false;
        if let Some(camera_type) = self.replay.previous_camera_type.take() {
            self.camera_type = camera_type;
        }
        self.replay.last_tick = None;
        self.studio_replay_closed();
        self.replay.virtual_tick_ms = 0.0;
        self.capture(false);
        if standalone {
            if let Some(mut remote) = self.remote.take() {
                // Replay sessions can themselves have an active/finishing
                // recorder. Keep its owned drain alive across session removal.
                self.retire_recording(&remote.session);
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

impl Game<'_> {
    pub(super) fn poll_studio_recording(&mut self) {
        if !self.studio_recording_notice_pending {
            return;
        }
        let Some(remote) = self.remote.as_ref() else {
            self.studio_recording_notice_pending = false;
            return;
        };
        let Some(result) = remote.session.poll_recording_stop() else {
            return;
        };
        self.studio_recording_notice_pending = false;
        match result {
            Ok(path) => {
                let path = path.display().to_string();
                self.system_message(Text::tr("horizon.studio.record.saved", &[&path]));
            }
            Err(error) => {
                let error = error.to_string();
                self.system_message(Text::tr("horizon.studio.record.error", &[&error]));
            }
        }
    }
}
