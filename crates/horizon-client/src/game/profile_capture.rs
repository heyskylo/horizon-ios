//! Client captures (docs/perf-platform.md §4.1): `horizon-ctl profile`, F3+O and `HORIZON_PROFILE`.
use super::Game;
use crate::Result;
use horizon_client::client_perf::{
    self, ClientPerf,
    request::{
        self, Failed, GpuSwitch, Marked, ProfileSide, Request, StartOptions, Started, Status,
        Stopped,
    },
};
use horizon_client::server_perf::{self, TickDetail, WorldCounts};
use horizon_client::spans::{self, Span};
use horizon_client::text::{ClickEvent, Style, Text};
use horizon_perf_format::{
    files,
    v1::{StopReason, Trigger},
};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

/// The folder `HORIZON_PROFILE` names; captures go there instead of the game home's `reports/`.
const ENV_DIR: &str = "HORIZON_PROFILE";

/// The game's capture state: the health history, the running capture and who is waiting on it.
pub(super) struct Profile {
    perf: ClientPerf,
    started_by: Trigger,
    stopped_by_key: bool,
    /// `profile stop` waits for the file to be whole.
    reply: Option<Sender<String>>,
    spans_were_on: bool,
    hardware_warmed: bool,
    env_tried: bool,
    /// What the running capture asks the renderer to time.
    gpu: GpuSwitch,
}

impl Profile {
    pub(super) fn new(slow_ms: f32) -> Self {
        Self {
            perf: ClientPerf::new(slow_ms),
            started_by: Trigger::Unspecified,
            stopped_by_key: false,
            reply: None,
            spans_were_on: false,
            hardware_warmed: false,
            env_tried: false,
            gpu: GpuSwitch::Off,
        }
    }

    /// The GPU timing the running capture needs; off when none runs.
    pub(super) fn gpu(&self) -> GpuSwitch {
        self.gpu
    }
}

fn reports_dir() -> PathBuf {
    client_perf::reports_dir(
        std::env::var_os(ENV_DIR).as_deref(),
        &horizon_client::logging::game_home(),
    )
}

/// The path as the player sees it: under the game home's folder when it is there.
fn shown(path: &Path) -> String {
    path.strip_prefix(horizon_client::logging::game_home())
        .unwrap_or(path)
        .display()
        .to_string()
}

fn gpu_readout(
    wanted: GpuSwitch,
    renderer: Option<&horizon_client::gfx::Renderer>,
) -> Option<horizon_client::gfx::pack_timing::GpuReadout> {
    renderer.filter(|_| wanted != GpuSwitch::Off)?.gpu_readout()
}

fn json_of<T: serde::Serialize>(reply: &T) -> Result<Option<serde_json::Value>> {
    Ok(Some(serde_json::to_value(reply)?))
}

fn reply_text<T: serde::Serialize>(reply: &T) -> String {
    serde_json::to_string(reply).unwrap_or_default()
}

impl Game<'_> {
    /// `profile [client] start|stop|status|mark`; None while a `stop` waits for its file.
    pub(super) fn profile_command(
        &mut self,
        words: &[&str],
        reply: &Sender<String>,
    ) -> Result<Option<serde_json::Value>> {
        let parsed = request::parse(words)?;
        if parsed.side == Some(ProfileSide::Server) {
            return Err("profile server: the server side is not captured from a window yet; use profile client".into());
        }
        match parsed.request {
            Request::Start(options) => {
                let started = self.start_profile(&options, Trigger::Ctl)?;
                json_of(&started)
            }
            Request::Stop => {
                self.stop_profile(StopReason::Normal)?;
                self.profile.reply = Some(reply.clone());
                Ok(None)
            }
            Request::Status => {
                let t = self.profile.perf.now();
                let status = self.profile.perf.status(t);
                json_of(&Status {
                    ok: true,
                    capturing: status.is_some(),
                    sides: status.iter().map(|_| ProfileSide::Client).collect(),
                    path: status.as_ref().map(|s| shown(&s.path)),
                    elapsed_s: status.as_ref().map(|s| s.elapsed_s),
                    bytes: status.as_ref().map(|s| s.bytes),
                })
            }
            Request::Mark(name) => {
                let t = self.profile.perf.now();
                if !self.profile.perf.mark(t, &name) {
                    return Err("no capture is running; start one with profile start".into());
                }
                json_of(&Marked { ok: true, name, t })
            }
        }
    }

    fn start_profile(&mut self, options: &StartOptions, trigger: Trigger) -> Result<Started> {
        if self.profile.perf.capturing() || self.profile.perf.saving() {
            return Err("a capture is already running or still being saved".into());
        }
        let start = client_perf::start_message(
            self.integrated.is_some(),
            horizon_client::telemetry::gpu_name().as_deref(),
        );
        let t = self.profile.perf.now();
        let path =
            self.profile
                .perf
                .start(t, &reports_dir(), trigger, options, start, Vec::new())?;
        self.profile.started_by = trigger;
        self.profile.gpu = options.gpu;
        self.profile.spans_were_on = spans::recording_here();
        spans::record_here(true);
        log::info!("Horizon: profiling the client to {}", path.display());
        Ok(Started {
            ok: true,
            side: ProfileSide::Client,
            path: shown(&path),
            seconds: options.seconds,
            samples: options.samples,
            gpu: options.gpu,
        })
    }

    fn stop_profile(&mut self, reason: StopReason) -> Result<()> {
        let t = self.profile.perf.now();
        let gpu = self.gpu_readout();
        if !self.profile.perf.stop_with_gpu(t, reason, gpu) {
            return Err("no capture is running".into());
        }
        self.profile.gpu = GpuSwitch::Off;
        Ok(())
    }

    /// The pass timings the running capture asked for, as the renderer shows them now.
    fn gpu_readout(&self) -> Option<horizon_client::gfx::pack_timing::GpuReadout> {
        gpu_readout(self.profile.gpu, self.renderer.as_ref())
    }

    /// F3+O: starts a capture, or stops the running one and says where it went.
    pub(super) fn profile_key(&mut self) {
        let outcome = if self.profile.perf.capturing() {
            self.profile.stopped_by_key = true;
            self.stop_profile(StopReason::Normal).map(|()| None)
        } else {
            self.start_profile(&StartOptions::default(), Trigger::Key)
                .map(|_| Some(Text::translate("horizon.perf.started", &[])))
        };
        match outcome {
            Ok(Some(message)) => self.system_message(message),
            Ok(None) => {}
            Err(e) => self.system_message(Text::translate(
                "horizon.perf.error",
                &[Text::literal(e.to_string())],
            )),
        }
    }

    /// One rendered frame: `wall_ms` long, `cpu_ms` of it the CPU's own work.
    pub(super) fn record_perf_frame(
        &mut self,
        wall_ms: f32,
        cpu_ms: f32,
        gpu_ms: Option<f32>,
        frame_spans: &[Span],
    ) {
        let slow = wall_ms > self.options.spikes.unwrap_or(client_perf::SLOW_FRAME_MS);
        let detail = slow.then(|| TickDetail {
            cpu_ms: Some(cpu_ms),
            spans: spans::merge(frame_spans),
        });
        let t = self.profile.perf.now();
        let (wanted, renderer) = (self.profile.gpu, self.renderer.as_ref());
        self.profile.perf.frame_with_gpu(
            t,
            wall_ms,
            cpu_ms,
            gpu_ms,
            detail,
            || WorldCounts {
                players: self.lan.as_ref().map_or(0, |h| h.hub.guests.len() as u32),
                chunks: self.world.chunks.len() as u32,
                entities: self.mobs.len() as u32,
            },
            || gpu_readout(wanted, renderer),
        );
        if !self.profile.perf.capturing() {
            self.profile.gpu = GpuSwitch::Off;
        }
        if !self.profile.hardware_warmed {
            self.profile.hardware_warmed = true;
            server_perf::warm_hardware();
        }
        if !self.profile.env_tried && server_perf::hardware().is_some() {
            self.profile.env_tried = true;
            let gpu = client_perf::env_gpu(
                std::env::var_os(client_perf::ENV_PASS_TIMING).as_deref(),
                std::env::var_os(client_perf::ENV_PASS_DETAIL).as_deref(),
            );
            if (std::env::var_os(ENV_DIR).is_some_and(|d| !d.is_empty()) || gpu != GpuSwitch::Off)
                && let Err(e) = self.start_profile(
                    &StartOptions {
                        gpu,
                        ..StartOptions::default()
                    },
                    Trigger::Env,
                )
            {
                log::warn!("Horizon: {ENV_DIR}: {e}");
            }
        }
    }

    /// Once per loop turn: the captures whose files are whole, told to whoever waits for them.
    pub(super) fn poll_profile(&mut self) {
        for saved in self.profile.perf.take_saved(false) {
            self.profile_saved(saved);
        }
    }

    /// Ends the capture a closing game still has, and waits for its file.
    pub(super) fn finish_profile(&mut self) {
        if self.profile.perf.capturing() {
            let _ = self.stop_profile(StopReason::Shutdown);
        }
        for saved in self.profile.perf.take_saved(true) {
            self.profile_saved(saved);
        }
    }

    fn profile_saved(&mut self, saved: std::io::Result<horizon_client::server_perf::Saved>) {
        spans::record_here(self.profile.spans_were_on);
        let reply = self.profile.reply.take();
        let by_key = std::mem::take(&mut self.profile.stopped_by_key)
            || (self.profile.started_by == Trigger::Key && reply.is_none());
        match saved {
            Ok(saved) => {
                for path in &saved.deleted {
                    log::info!("Horizon: removed the oldest capture {}", path.display());
                }
                log::info!("Horizon: {}", files::saved_line(&saved.path));
                if let Some(reply) = reply {
                    let _ = reply.send(reply_text(&Stopped {
                        ok: true,
                        side: ProfileSide::Client,
                        path: shown(&saved.path),
                        open_at: files::PERF_UI_URL.to_owned(),
                        removed: saved.deleted.iter().map(|p| shown(p)).collect(),
                    }));
                }
                if by_key {
                    self.system_message(Text::translate(
                        "horizon.perf.saved",
                        &[Text::literal(shown(&saved.path))],
                    ));
                    let link = Text::styled(
                        files::PERF_UI_URL,
                        Style {
                            underlined: Some(true),
                            click: Some(ClickEvent::OpenUrl(files::PERF_UI_URL.to_owned())),
                            ..Style::default()
                        },
                    );
                    self.system_message(Text::translate("horizon.perf.open_in_browser", &[link]));
                }
            }
            Err(e) => {
                log::error!("Horizon: could not write the performance capture: {e}");
                if let Some(reply) = reply {
                    let _ = reply.send(reply_text(&Failed {
                        error: e.to_string(),
                    }));
                }
                if by_key {
                    self.system_message(Text::translate(
                        "horizon.perf.error",
                        &[Text::literal(e.to_string())],
                    ));
                }
            }
        }
    }
}
