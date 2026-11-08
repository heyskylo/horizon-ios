//! `profile` against the server this window is connected to (docs/perf-platform.md §4.2): the request over `horizon:perf`, its answer, and the warnings a refusal leaves in the client's capture.
use super::Game;
use super::profile_capture::reply_text;
use crate::Result;
use horizon_client::client_perf::request::{
    Failed, GpuSwitch, ProfileSide, StartOptions, Started, Stopped,
};
use horizon_client::perf_net::{CHANNEL, Outcome, Refusal, Reply, Request, Unsent, Wanted};
use horizon_perf_format::{files, v1::WarningCode};
use horizon_protocol::packets::{configuration::CustomPayload, play::Serverbound as S};
use std::sync::mpsc::Sender;
use std::time::Instant;

const MARK_SERVER_STARTED: &str = "server_started";

fn refusal_text(why: Refusal) -> String {
    let (key, default) = match why {
        Refusal::Denied => (
            "horizon.perf.denied",
            "a server profile needs operator level 4",
        ),
        Refusal::AlreadyRunning => (
            "horizon.perf.already_running",
            "the server is already running a profile",
        ),
        Refusal::NotRunning => (
            "horizon.perf.not_running",
            "no server profile of yours is running",
        ),
        Refusal::Failed => (
            "horizon.perf.failed",
            "the server could not write the profile; see its log",
        ),
    };
    horizon_client::lang::get_or(key, default)
}

fn not_horizon_text() -> String {
    horizon_client::lang::get_or(
        "horizon.perf.not_horizon",
        "this server isn't running Horizon",
    )
}

fn unsent_text(why: Unsent) -> String {
    match why {
        Unsent::NotHorizon => not_horizon_text(),
        Unsent::Busy => "a server profile is already running or still being saved".to_owned(),
        Unsent::NothingToStop => "no server profile is running".to_owned(),
    }
}

fn warning_of(why: Refusal) -> Option<WarningCode> {
    match why {
        Refusal::Denied => Some(WarningCode::ServerDenied),
        Refusal::AlreadyRunning => Some(WarningCode::ServerAlreadyRunning),
        Refusal::NotRunning | Refusal::Failed => None,
    }
}

impl Game<'_> {
    /// Whether this window is a client of a server it does not run itself.
    pub(super) fn profiles_remote_server(&self) -> bool {
        self.integrated.is_none() && self.remote.is_some()
    }

    fn server_brand(&self) -> &str {
        self.remote.as_ref().map_or("", |r| r.brand.as_str())
    }

    fn send_perf(&self, request: &Request) {
        self.send(S::CustomPayload(CustomPayload {
            channel: CHANNEL.into(),
            data: request.encode(),
        }));
    }

    /// `profile server start` on a server: the answer comes back through `reply`.
    pub(super) fn start_remote_only(
        &mut self,
        options: &StartOptions,
        reply: &Sender<String>,
    ) -> Result<Option<serde_json::Value>> {
        if !self.profiles_remote_server() {
            return Err("profile server: no server to reach; open a world or join a server".into());
        }
        if self.profile.perf.capturing() || self.profile.perf.saving() {
            return Err("a capture is already running or still being saved".into());
        }
        let brand = self.server_brand().to_owned();
        self.profile
            .server_link
            .ask(&brand, Instant::now())
            .map_err(unsent_text)?;
        self.send_perf(&Request::Start(Wanted::from(options)));
        self.profile.server_reply = Some(reply.clone());
        self.profile.server_joined = false;
        Ok(None)
    }

    /// A window capture on a server also asks the server for its side; a refusal is a warning in the file.
    pub(super) fn join_remote_server(&mut self, options: &StartOptions) {
        if !self.profiles_remote_server() {
            return;
        }
        let brand = self.server_brand().to_owned();
        match self.profile.server_link.ask(&brand, Instant::now()) {
            Ok(()) => {
                self.send_perf(&Request::Start(Wanted::from(options)));
                self.profile.server_joined = true;
            }
            Err(Unsent::NotHorizon) => self.warn_server(WarningCode::ServerNotHorizon),
            Err(_) => {}
        }
    }

    fn warn_server(&mut self, code: WarningCode) {
        let t = self.profile.perf.now();
        self.profile.perf.warn(t, code);
    }

    /// `profile server stop` on a server: the file's name comes back through `reply`.
    pub(super) fn stop_remote_only(&mut self, reply: &Sender<String>) -> Result<()> {
        self.profile
            .server_link
            .stop(Instant::now())
            .map_err(unsent_text)?;
        self.send_perf(&Request::Stop);
        self.profile.server_reply = Some(reply.clone());
        Ok(())
    }

    /// The window capture ends: so does the server's side that asked with it.
    pub(super) fn stop_joined_server(&mut self) {
        if self.profile.server_joined && self.profile.server_link.stop(Instant::now()).is_ok() {
            self.send_perf(&Request::Stop);
        }
    }

    /// A `horizon:perf` payload from the server.
    pub(super) fn remote_perf_reply(&mut self, data: &[u8]) {
        let Ok(reply) = Reply::decode(data) else {
            log::debug!("Horizon: a bad server profile reply");
            return;
        };
        let waiting = self.profile.server_reply.take();
        let joined = self.profile.server_joined;
        match self.profile.server_link.answer(reply, Instant::now()) {
            Outcome::Started {
                path,
                seconds,
                samples,
            } => {
                if joined {
                    let t = self.profile.perf.now();
                    self.profile.perf.mark(t, MARK_SERVER_STARTED);
                }
                if let Some(waiting) = waiting {
                    let _ = waiting.send(reply_text(&Started {
                        ok: true,
                        sides: vec![ProfileSide::Server],
                        path,
                        seconds,
                        samples,
                        gpu: GpuSwitch::Off,
                    }));
                }
            }
            Outcome::Finished { path, removed } => {
                log::info!("Horizon: the server saved its profile to {path}");
                self.profile.server_joined = false;
                if let Some(waiting) = waiting {
                    let _ = waiting.send(reply_text(&Stopped {
                        ok: true,
                        sides: vec![ProfileSide::Server],
                        path,
                        open_at: files::PERF_UI_URL.to_owned(),
                        removed,
                    }));
                }
            }
            Outcome::Refused(why) => {
                if joined {
                    if let Some(code) = warning_of(why) {
                        self.warn_server(code);
                    }
                    self.profile.server_joined = false;
                }
                log::warn!(
                    "Horizon: the server refused the profile: {}",
                    refusal_text(why)
                );
                if let Some(waiting) = waiting {
                    let _ = waiting.send(reply_text(&Failed {
                        error: refusal_text(why),
                    }));
                }
            }
            Outcome::Stray => self.profile.server_reply = waiting,
        }
    }

    /// Once per loop turn: a request the server never answered, or a connection that ended under one.
    pub(super) fn poll_server_link(&mut self) {
        let lost = self.remote.is_none() && !self.profile.server_link.is_idle();
        let silent = self.profile.server_link.timed_out(Instant::now());
        if !lost && !silent {
            return;
        }
        if lost {
            self.profile.server_link.reset();
        }
        self.profile.server_joined = false;
        if let Some(waiting) = self.profile.server_reply.take() {
            let error = if lost {
                "the connection to the server ended".to_owned()
            } else {
                "the server did not answer; it may not run a Horizon with server profiles"
                    .to_owned()
            };
            let _ = waiting.send(reply_text(&Failed { error }));
        }
    }
}
