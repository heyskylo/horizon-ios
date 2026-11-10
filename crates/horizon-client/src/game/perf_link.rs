//! `profile` against the server this window is connected to (docs/perf-platform.md §4.2): the request over `horizon:perf`, its answer, and the warnings a refusal leaves in the client's capture.
use super::Game;
use super::profile_capture::{reply_text, reports_dir, shown};
use crate::Result;
use horizon_client::client_perf::request::{
    Failed, GpuSwitch, ProfileSide, StartOptions, Started, Stopped,
};
use horizon_client::perf_net::{CHANNEL, Outcome, Refusal, Reply, Request, Unsent, Wanted};
use horizon_perf_format::{FieldSender, PartMerger, RawSend, files, v1::WarningCode};
use horizon_protocol::packets::{configuration::CustomPayload, play::Serverbound as S};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

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

/// The window capture's file, which the server's part is written into.
pub(super) struct Sink {
    pub(super) sender: FieldSender,
    pub(super) origin: Instant,
}

impl Sink {
    /// Closes the file's server side as far as it got, and records why it is short.
    fn incomplete(&self, merger: &PartMerger) {
        for field in merger.cut_short(self.origin.elapsed().as_secs_f64()) {
            self.sender.send(field);
        }
    }
}

enum Dest {
    /// Appended to the window's file, whole fields at a time; `held` waits for room in the writer's channel.
    Window { sink: Sink, held: Vec<u8> },
    /// Saved as it came, as a file of its own.
    Own { file: File, path: PathBuf },
}

/// The server's file as it arrives.
pub(super) struct Incoming {
    merger: PartMerger,
    dest: Dest,
    reply: Option<Sender<String>>,
    /// The last chunk has arrived and was checked whole.
    whole: bool,
}

fn part_text() -> String {
    horizon_client::lang::get_or(
        "horizon.perf.part_incomplete",
        "the server's part of the profile did not arrive whole",
    )
}

fn unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
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
        if let Reply::Chunk(chunk) = reply {
            self.part_chunk(&chunk);
            return;
        }
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
                    self.profile.server_sink = self.profile.perf.join().map(|j| Sink {
                        sender: j.sender,
                        origin: j.origin,
                    });
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
            Outcome::Finished { path, bytes, .. } => {
                log::info!("Horizon: the server saved its profile to {path}");
                self.profile.server_joined = false;
                if bytes == 0 {
                    self.profile.server_reply = waiting;
                    self.abandon_part();
                } else {
                    self.begin_part(waiting);
                }
            }
            Outcome::Refused(why) => {
                if joined {
                    if let Some(code) = warning_of(why) {
                        self.warn_server(code);
                    }
                    self.profile.server_joined = false;
                    self.profiler_client_only(&refusal_text(why));
                }
                if self.profile.incoming.is_some() || self.profile.server_sink.is_some() {
                    self.profile.server_reply = waiting;
                    self.abandon_part();
                    return;
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

    /// The server's file is about to arrive: into the window's capture if it still holds the file open, else into one of its own.
    fn begin_part(&mut self, waiting: Option<Sender<String>>) {
        let dest = match self.profile.server_sink.take() {
            Some(sink) => Ok(Dest::Window {
                sink,
                held: Vec::new(),
            }),
            None => files::create(&reports_dir(), unix_secs())
                .map(|(file, path)| Dest::Own { file, path }),
        };
        match dest {
            Ok(dest) => {
                self.profile.incoming = Some(Incoming {
                    merger: PartMerger::new(),
                    dest,
                    reply: waiting,
                    whole: false,
                });
            }
            Err(e) => {
                log::warn!("Horizon: the server's profile cannot be saved: {e}");
                self.profile.server_link.reset();
                if let Some(waiting) = waiting {
                    let _ = waiting.send(reply_text(&Failed {
                        error: e.to_string(),
                    }));
                }
            }
        }
    }

    /// The next bytes of the server's file.
    fn part_chunk(&mut self, chunk: &[u8]) {
        use horizon_client::perf_net::Received;
        if self.profile.incoming.is_none() {
            return;
        }
        let received = self.profile.server_link.chunk(chunk.len(), Instant::now());
        let step = match received {
            Received::Stray => return,
            Received::TooMuch => Err("the server sent more than it announced".to_owned()),
            Received::More | Received::Last => self.take_part(chunk, received == Received::Last),
        };
        if let Err(why) = step {
            log::warn!("Horizon: the server's profile part is unusable: {why}");
            self.abandon_part();
        }
    }

    fn take_part(&mut self, chunk: &[u8], last: bool) -> std::result::Result<(), String> {
        let Some(incoming) = &mut self.profile.incoming else {
            return Ok(());
        };
        let fields = incoming.merger.push(chunk).map_err(|e| e.to_string())?;
        match &mut incoming.dest {
            Dest::Window { held, .. } => held.extend_from_slice(&fields),
            Dest::Own { file, .. } => file.write_all(chunk).map_err(|e| e.to_string())?,
        }
        if last {
            std::mem::take(&mut incoming.merger)
                .finish()
                .map_err(|e| e.to_string())?;
            incoming.whole = true;
        }
        self.settle_part()
    }

    /// Hands the window's writer what the server's file has yielded, and completes the transfer once all of it is in.
    fn settle_part(&mut self) -> std::result::Result<(), String> {
        let Some(incoming) = &mut self.profile.incoming else {
            return Ok(());
        };
        if let Dest::Window { sink, held } = &mut incoming.dest
            && !held.is_empty()
        {
            match sink.sender.try_send_raw(std::mem::take(held)) {
                RawSend::Sent => {}
                RawSend::Full(back) => *held = back,
                RawSend::Closed => return Err("the capture's file is closed".to_owned()),
            }
        }
        let drained = match &incoming.dest {
            Dest::Window { held, .. } => held.is_empty(),
            Dest::Own { .. } => true,
        };
        if incoming.whole && drained {
            self.part_done();
        }
        Ok(())
    }

    /// The server's file is whole: the window's capture can finish, or the file of its own is kept.
    fn part_done(&mut self) {
        let Some(Incoming { dest, reply, .. }) = self.profile.incoming.take() else {
            return;
        };
        let Dest::Own { file, path } = dest else {
            log::info!("Horizon: the server's profile is part of this capture");
            return;
        };
        let kept = file.sync_all().and_then(|()| {
            let dir = path.parent().map(PathBuf::from).unwrap_or_default();
            files::retain(&dir, &path, files::KEEP_FILES, files::KEEP_BYTES)
        });
        let text = match kept {
            Ok(deleted) => {
                log::info!("Horizon: {}", files::saved_line(&path));
                reply_text(&Stopped {
                    ok: true,
                    sides: vec![ProfileSide::Server],
                    path: shown(&path),
                    open_at: horizon_client::client_perf::request::viewer(),
                    removed: deleted.iter().map(|p| shown(p)).collect(),
                })
            }
            Err(e) => reply_text(&Failed {
                error: e.to_string(),
            }),
        };
        if let Some(reply) = reply {
            let _ = reply.send(text);
        }
    }

    /// The server's part will not arrive whole: what did arrive stays in the window's capture, with the warning, or the file of its own is removed.
    pub(super) fn abandon_part(&mut self) {
        let waiting = self.profile.server_reply.take();
        let sink = self.profile.server_sink.take();
        if let Some(Incoming {
            dest,
            reply,
            merger,
            ..
        }) = self.profile.incoming.take()
        {
            match dest {
                Dest::Window { sink, held } => {
                    if !held.is_empty() {
                        let _ = sink.sender.try_send_raw(held);
                    }
                    sink.incomplete(&merger);
                }
                Dest::Own { file, path } => {
                    drop(file);
                    let _ = std::fs::remove_file(path);
                    if let Some(reply) = reply {
                        let _ = reply.send(reply_text(&Failed { error: part_text() }));
                    }
                }
            }
        }
        if let Some(sink) = sink {
            sink.incomplete(&PartMerger::new());
        }
        if let Some(waiting) = waiting {
            let _ = waiting.send(reply_text(&Failed { error: part_text() }));
        }
    }

    /// Once per loop turn: a request the server never answered, or a connection that ended under one.
    pub(super) fn poll_server_link(&mut self) {
        if self.profile.incoming.is_some() && self.settle_part().is_err() {
            self.abandon_part();
        }
        let now = Instant::now();
        let lost = self.remote.is_none() && !self.profile.server_link.is_idle();
        let silent = self.profile.server_link.timed_out(now);
        let stalled = self.profile.server_link.stalled(now);
        if !lost && !silent && !stalled {
            return;
        }
        if lost {
            self.profile.server_link.reset();
        }
        if lost || stalled {
            self.abandon_part();
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
