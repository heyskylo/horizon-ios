//! Studio recording commands.

use super::Game;
use horizon_client::{
    Result,
    studio::{Metadata, generate_recording_filename},
    text::Text,
};
use serde_json::{Value, json};
use std::path::PathBuf;

pub const HELP: &[&str] = &[
    "studio record start [NAME]     start recording clientbound packets into an .mcpr archive",
    "studio record stop             stop recording and finalize the archive",
    "studio record status           show recording elapsed time, packets, and size",
];

fn studio_recording_metadata(is_singleplayer: bool) -> Metadata {
    let server_name = if is_singleplayer {
        "Singleplayer World"
    } else {
        "Multiplayer Server"
    };
    Metadata::new(
        server_name.into(),
        is_singleplayer,
        horizon_core::release::VERSION.into(),
    )
}

impl Game<'_> {
    /// Handles Horizon's `/studio record` command without intercepting server
    /// commands that merely share its prefix.
    pub(super) fn studio_command(&mut self, command: &str) -> bool {
        let Some(rest) = command.strip_prefix("studio") else {
            return false;
        };
        if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
            return false;
        }
        let words: Vec<&str> = rest.split_whitespace().collect();
        if words.first().copied() != Some("record") {
            self.system_message(Text::translate("horizon.studio.record.usage", &[]));
            return true;
        }

        match words.get(1).copied() {
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
                    let elapsed = (status["elapsed_ms"].as_u64().unwrap_or(0) / 1000).to_string();
                    let packets = status["packet_count"].as_u64().unwrap_or(0).to_string();
                    let size = (status["bytes_written"].as_u64().unwrap_or(0) / 1024).to_string();
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
        }
        true
    }

    /// Control socket command handler for `studio record`.
    pub(super) fn studio_control(&mut self, words: &[&str]) -> Result<Option<Value>> {
        if words.first().copied() != Some("studio") {
            return Ok(None);
        }
        let value = match words.get(1).copied() {
            Some("record") => match words.get(2).copied() {
                Some("start") => self.start_studio_recording(words.get(3).copied())?,
                Some("stop") => self.stop_studio_recording()?,
                Some("status") => self.studio_recording_status(),
                _ => json!({ "error": "unknown studio record subcommand" }),
            },
            _ => json!({ "error": "unknown studio command" }),
        };
        Ok(Some(value))
    }

    pub(super) fn start_studio_recording(&mut self, name: Option<&str>) -> Result<Value> {
        let Some(remote) = self.remote.as_ref() else {
            return Ok(json!({ "ok": false, "error": "no active world session" }));
        };
        if remote.session.is_recording() {
            return Ok(json!({ "ok": false, "error": "recording already active" }));
        }
        let recordings = PathBuf::from("recordings");
        std::fs::create_dir_all(&recordings)?;
        let path = recordings.join(generate_recording_filename(name));
        let is_singleplayer =
            remote.session.address.is_empty() || remote.session.address == "local";
        let metadata = studio_recording_metadata(is_singleplayer);
        match remote.session.start_recording(&path, metadata) {
            Ok(()) => Ok(json!({ "ok": true, "path": path.display().to_string() })),
            Err(error) => Ok(json!({ "ok": false, "error": error.to_string() })),
        }
    }

    pub(super) fn stop_studio_recording(&mut self) -> Result<Value> {
        let Some(remote) = self.remote.as_ref() else {
            return Ok(json!({ "ok": false, "error": "no active world session" }));
        };
        match remote.session.stop_recording() {
            Some(Ok(path)) => Ok(json!({ "ok": true, "path": path.display().to_string() })),
            Some(Err(error)) => Ok(json!({ "ok": false, "error": error.to_string() })),
            None => Ok(json!({ "ok": false, "error": "no active recording" })),
        }
    }

    pub(super) fn studio_recording_status(&self) -> Value {
        if let Some(remote) = self.remote.as_ref()
            && let Some(status) = remote.session.recorder_status()
        {
            json!({
                "ok": true,
                "active": status.active,
                "path": status.path.display().to_string(),
                "elapsed_ms": status.elapsed.as_millis(),
                "packet_count": status.packet_count,
                "bytes_written": status.bytes_written,
                "error": status.error,
            })
        } else {
            json!({ "ok": true, "active": false, "error": null })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::studio_recording_metadata;

    #[test]
    fn recording_metadata_does_not_store_server_addresses_or_player_names() {
        let multiplayer = studio_recording_metadata(false);
        assert_eq!(multiplayer.server_name, "Multiplayer Server");
        assert!(multiplayer.players.is_empty());

        let singleplayer = studio_recording_metadata(true);
        assert_eq!(singleplayer.server_name, "Singleplayer World");
        assert!(singleplayer.players.is_empty());
    }
}
