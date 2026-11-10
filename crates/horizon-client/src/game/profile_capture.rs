//! Captures (docs/perf-platform.md §4.1): `horizon-ctl profile`, `/profiler` and `HORIZON_PROFILE`. The window writes its own side; in single player the server thread writes its side into the same file.
use super::Game;
use super::integrated::{
    NO_ANSWER,
    capture::{Live, ServerSide, Target},
};
use super::memory::{MEMORY_EVERY_S, flatten_report};
use super::perf_remote::Served;
use crate::Result;
use horizon_client::client_perf::{
    self, ClientPerf,
    request::{
        self, Failed, GpuSwitch, Marked, Opened, ProfileSide, Request, Shared, StartOptions,
        Started, Status, Stopped, Traced,
    },
};
use horizon_client::command::{Arg, Dispatcher, PROFILER_GPU, Registries};
use horizon_client::hud::MARK_SPRITE;
use horizon_client::perf_net::Link;
use horizon_client::server_perf::{self, TickDetail, WorldCounts};
use horizon_client::spans::{self, Span};
use horizon_client::text::{
    ClickEvent, DARK_GRAY, GOLD, GRAY, InlineObject, RED, Style, Text, WHITE,
};
use horizon_perf_format::{
    files,
    v1::{BenchResult, StopReason, Trigger},
};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    mpsc::{Receiver, Sender, TryRecvError},
};
use std::time::Instant;

/// The folder `HORIZON_PROFILE` names; captures go there instead of the game home's `reports/`.
const ENV_DIR: &str = "HORIZON_PROFILE";
/// The file `HORIZON_TRACE` names: a timeline-only capture written there.
const ENV_TRACE: &str = "HORIZON_TRACE";

const TRACE_USAGE: &str = "trace [FILE [SECONDS] | stop]";

/// What the `trace` words ask for.
#[derive(Debug, PartialEq, Eq)]
enum Trace<'a> {
    Status,
    Stop,
    Start { file: &'a str, seconds: Option<u32> },
}

/// The words after `trace`; SECONDS is a whole number in 1..=`MAX_WINDOW_S`, as for `profile --seconds`.
fn parse_trace<'a>(words: &[&'a str]) -> std::result::Result<Trace<'a>, &'static str> {
    match words {
        [] => Ok(Trace::Status),
        ["stop"] => Ok(Trace::Stop),
        [file] => Ok(Trace::Start {
            file,
            seconds: None,
        }),
        [file, seconds] => match seconds.parse::<u32>() {
            Ok(s) if s >= 1 && f64::from(s) <= server_perf::MAX_WINDOW_S => Ok(Trace::Start {
                file,
                seconds: Some(s),
            }),
            _ => Err(TRACE_USAGE),
        },
        _ => Err(TRACE_USAGE),
    }
}

const NO_CAPTURE: &str = "no capture is running; start one with profile start";

/// Whether the single-player server thread writes into the window's capture.
enum ServerLink {
    /// The capture is of the window alone.
    Off,
    /// The capture wants the server thread's side, which is asked to join once there is a server thread.
    Pending,
}

/// Which capture a `profile stop` ends.
#[derive(Debug, PartialEq, Eq)]
enum StopRoute {
    Window,
    ServerOnly,
    /// A capture of the connected server alone.
    Remote,
}

/// A capture of the server thread alone, which writes its own file.
struct ServerOnly {
    live: Arc<Live>,
    path: PathBuf,
    since: Instant,
    /// The window's game clock when the capture began.
    began: f64,
}

/// What a player asked of a saved capture, through the captures hook.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Job {
    Open,
    Share,
}

/// The game's capture state: the health history, the running capture and who is waiting on it.
pub(super) struct Profile {
    /// The server thread's health history and its side of a capture; set on its own Game only.
    pub(super) server: Option<ServerSide>,
    link: ServerLink,
    /// The server thread's side of the running or last window capture, once it was asked.
    joined: Option<Arc<Live>>,
    server_only: Option<ServerOnly>,
    pub(super) perf: ClientPerf,
    started_by: Trigger,
    /// When the running capture takes its next memory snapshot, and how many it has taken.
    memory_due: Option<(f64, u32)>,
    /// Where the answer to a `/profiler` request that finishes later arrives.
    chat_answer: Option<Receiver<String>>,
    jobs: VecDeque<(PathBuf, Job)>,
    job: Option<(PathBuf, Job, Receiver<std::result::Result<String, String>>)>,
    last_capture: Option<PathBuf>,
    /// `profile stop` waits for the file to be whole.
    reply: Option<Sender<String>>,
    spans_were_on: bool,
    /// The file the running capture was asked to write by `trace` or `HORIZON_TRACE`.
    trace_file: Option<String>,
    hardware_warmed: bool,
    env_tried: bool,
    /// What the running capture asks the renderer to time.
    gpu: GpuSwitch,
    /// The guests' requests to this server and the capture they ask of it (perf_remote.rs).
    pub(super) served: Served,
    /// This client's capture on the server it is connected to.
    pub(super) server_link: Link,
    /// `profile` waits for the server's answer to its start, or for its file after a stop.
    pub(super) server_reply: Option<Sender<String>>,
    /// Whether the server's capture belongs to a window capture, whose stop ends it.
    pub(super) server_joined: bool,
    /// The window capture's file, held open for the server's part until it has all arrived.
    pub(super) server_sink: Option<super::perf_link::Sink>,
    /// The server's file while it arrives.
    pub(super) incoming: Option<super::perf_link::Incoming>,
}

impl Profile {
    pub(super) fn new(slow_ms: f32) -> Self {
        Self {
            server: None,
            link: ServerLink::Off,
            joined: None,
            server_only: None,
            perf: ClientPerf::new(slow_ms),
            started_by: Trigger::Unspecified,
            memory_due: None,
            chat_answer: None,
            jobs: VecDeque::new(),
            job: None,
            last_capture: None,
            reply: None,
            spans_were_on: false,
            trace_file: None,
            hardware_warmed: false,
            env_tried: false,
            gpu: GpuSwitch::Off,
            served: Served::default(),
            server_link: Link::default(),
            server_reply: None,
            server_joined: false,
            server_sink: None,
            incoming: None,
        }
    }

    /// The GPU timing the running capture needs; off when none runs.
    pub(super) fn gpu(&self) -> GpuSwitch {
        self.gpu
    }

    fn server_live(&self) -> bool {
        self.perf.capturing() && self.joined.as_ref().is_some_and(|l| l.active())
    }

    fn server_only_live(&self) -> bool {
        self.server_only.as_ref().is_some_and(|s| s.live.active())
    }

    /// Whether the last window capture holds the server thread's side.
    fn served(&self) -> bool {
        self.joined.as_ref().is_some_and(|l| l.wrote())
    }

    /// Where a `profile stop` for `side` (or for any side) goes.
    fn stop_route(
        &self,
        side: Option<ProfileSide>,
    ) -> std::result::Result<StopRoute, &'static str> {
        match side {
            Some(ProfileSide::Client) if !self.perf.capturing() => {
                Err("no client capture is running")
            }
            Some(ProfileSide::Server) if !self.sides().contains(&ProfileSide::Server) => {
                Err("no server capture is running")
            }
            _ if self.perf.capturing() => Ok(StopRoute::Window),
            _ if self.server_only_live() => Ok(StopRoute::ServerOnly),
            _ if self.server_link.running() => Ok(StopRoute::Remote),
            _ => Err(NO_CAPTURE),
        }
    }

    /// The reply to `profile status`; a finished server-only capture is not reported.
    fn status(&self) -> Status {
        let client = self.perf.status(self.perf.now());
        let alone = self
            .server_only
            .as_ref()
            .filter(|s| client.is_none() && s.live.active());
        let remote = self
            .server_link
            .status(Instant::now())
            .filter(|_| client.is_none() && alone.is_none());
        Status {
            ok: true,
            capturing: client.is_some() || alone.is_some() || remote.is_some(),
            sides: self.sides(),
            path: client
                .as_ref()
                .map(|s| shown(&s.path))
                .or_else(|| alone.map(|s| shown(&s.path)))
                .or_else(|| remote.map(|(path, _)| path.to_owned())),
            elapsed_s: client
                .as_ref()
                .map(|s| s.elapsed_s)
                .or_else(|| alone.map(|s| s.since.elapsed().as_secs_f64()))
                .or_else(|| remote.map(|(_, elapsed)| elapsed)),
            bytes: client
                .as_ref()
                .map(|s| s.bytes)
                .or_else(|| alone.and_then(|s| Some(std::fs::metadata(&s.path).ok()?.len()))),
        }
    }

    /// Marks the window's capture if `side` wants it; also the `t` of a server mark that is due, for the caller to post. Every `t` is on the game clock.
    fn mark(&mut self, side: Option<ProfileSide>, name: &str) -> (Option<f64>, Option<f64>) {
        let want = |s| side.is_none_or(|only| only == s);
        let t = self.perf.now();
        let client = (want(ProfileSide::Client) && self.perf.mark(t, name)).then_some(t);
        let server = if !want(ProfileSide::Server) {
            None
        } else if self.server_live() {
            Some(t)
        } else {
            self.server_only
                .as_ref()
                .filter(|s| s.live.active())
                .map(|s| s.began + s.since.elapsed().as_secs_f64())
        };
        (client, server)
    }

    /// The sides a capture is writing, server first.
    fn sides(&self) -> Vec<ProfileSide> {
        let server = self.server_live() || self.server_only_live() || self.server_link.running();
        [
            server.then_some(ProfileSide::Server),
            self.perf.capturing().then_some(ProfileSide::Client),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

pub(super) fn reports_dir() -> PathBuf {
    client_perf::reports_dir(
        std::env::var_os(ENV_DIR).as_deref(),
        &horizon_client::logging::game_home(),
    )
}

/// The path as the player sees it: under the game home's folder when it is there.
pub(super) fn shown(path: &Path) -> String {
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

fn read_bench(path: &str) -> Result<BenchResult> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    Ok(client_perf::bench_result(text).map_err(|e| format!("{path}: {e}"))?)
}

fn json_of<T: serde::Serialize>(reply: &T) -> Result<Option<serde_json::Value>> {
    Ok(Some(serde_json::to_value(reply)?))
}

pub(super) fn reply_text<T: serde::Serialize>(reply: &T) -> String {
    serde_json::to_string(reply).unwrap_or_default()
}

/// The fields of a deferred `profile` reply that chat shows.
#[derive(serde::Deserialize)]
struct ChatAnswer {
    error: Option<String>,
    path: Option<String>,
    removed: Option<Vec<String>>,
}

/// The words after `/profiler` when `command`, without its slash, is that command.
pub(super) fn profiler_args(command: &str) -> Option<&str> {
    command
        .strip_prefix("profiler")
        .filter(|rest| rest.is_empty() || rest.starts_with(' '))
}

fn unavailable() -> String {
    horizon_client::lang::get("horizon.perf.unavailable")
}

/// A path a request names, relative to the game's home.
fn game_path(file: &Path) -> PathBuf {
    horizon_client::logging::game_home().join(file)
}

/// Why a `/profiler open` or `share` found no capture.
#[derive(Debug, PartialEq, Eq)]
enum Missing {
    Last,
    Named(String),
}

/// The capture `id` names, a capture file in `reports` (this session's `last` one when no id).
fn resolve_capture(
    last: Option<&Path>,
    reports: &Path,
    id: Option<&str>,
) -> std::result::Result<PathBuf, Missing> {
    let Some(id) = id else {
        return last.map(Path::to_path_buf).ok_or(Missing::Last);
    };
    if let Some(last) = last.filter(|p| p.file_stem().is_some_and(|s| s == id)) {
        return Ok(last.to_path_buf());
    }
    let name = format!("{id}.hzperf");
    let path = reports.join(&name);
    if files::is_capture_name(&name) && path.is_file() {
        Ok(path)
    } else {
        Err(Missing::Named(id.to_owned()))
    }
}

/// A bracketed chat button that runs `command` when clicked.
fn capture_button(label: &str, hover: &str, command: String) -> Text {
    Text::translate(label, &[]).with_style(&Style {
        color: Some(GOLD),
        underlined: Some(true),
        click: Some(ClickEvent::RunCommand(command)),
        hover: Some(Box::new(Text::translate(hover, &[]))),
        ..Style::default()
    })
}

/// The captures hook's answer: the share link for [`Job::Share`], nothing for [`Job::Open`].
fn run_job(job: Job, file: &Path) -> std::result::Result<String, String> {
    let captures = horizon_hooks::engine::captures()
        .ok_or_else(|| horizon_client::lang::get("horizon.perf.unavailable"))?;
    match job {
        Job::Open => captures.open(file).map(|()| String::new()),
        Job::Share => captures.share(file),
    }
}

/// A command in a chat line that runs when clicked.
fn command_link(command: &str) -> Text {
    Text::styled(
        command,
        Style {
            color: Some(GOLD),
            underlined: Some(true),
            click: Some(ClickEvent::RunCommand(command.to_owned())),
            ..Style::default()
        },
    )
}

/// A `/profiler` line the tree parsed, as the words `request::parse` reads: `[SIDE] ACTION [--gpu=MODE] [NAME]`.
fn profiler_words(literals: &[String], name: Option<&str>) -> Vec<String> {
    let mut words: Vec<String> = literals
        .iter()
        .skip(1)
        .filter(|w| matches!(w.as_str(), "client" | "server"))
        .cloned()
        .collect();
    words.extend(literals.get(1).cloned());
    let [passes, detail] = PROFILER_GPU;
    words.extend(literals.iter().find_map(|w| {
        (w == passes)
            .then(|| "--gpu=passes".to_owned())
            .or_else(|| (w == detail).then(|| "--gpu=detail".to_owned()))
    }));
    words.extend(name.map(str::to_owned));
    words
}

/// Spark's chat shape: a dim tag round Horizon's mark, then the line in gray.
fn profiler_line(body: Text, color: u32) -> Text {
    let mark = Style {
        color: Some(WHITE),
        inline_object: Some(InlineObject::Atlas {
            atlas: "minecraft:gui".into(),
            sprite: MARK_SPRITE.into(),
            fallback: "H".into(),
        }),
        ..Style::default()
    };
    Text::colored("[", DARK_GRAY)
        .append(Text::styled("\u{fffc}", mark))
        .append(Text::colored("] ", DARK_GRAY))
        .append(body.with_style(&Style::color(color)))
}

impl Game<'_> {
    /// `profile [client|server] start|stop|status|mark`; None while a `stop` waits for its file.
    pub(super) fn profile_command(
        &mut self,
        words: &[&str],
        reply: &Sender<String>,
    ) -> Result<Option<serde_json::Value>> {
        self.run_profile(words, reply, Trigger::Ctl)
    }

    fn run_profile(
        &mut self,
        words: &[&str],
        reply: &Sender<String>,
        trigger: Trigger,
    ) -> Result<Option<serde_json::Value>> {
        let parsed = request::parse(words)?;
        match parsed.request {
            Request::Start(options) => {
                if parsed.side == Some(ProfileSide::Server) && self.integrated.is_none() {
                    return self.start_remote_only(&options, reply);
                }
                let started = self.start_profile(parsed.side, &options, trigger)?;
                json_of(&started)
            }
            Request::Stop { bench } => {
                match self.profile.stop_route(parsed.side)? {
                    StopRoute::Window => {
                        let bench = bench.map(|path| read_bench(&path)).transpose()?;
                        self.stop_profile_with(StopReason::Normal, bench.into_iter().collect())?;
                        self.profile.reply = Some(reply.clone());
                    }
                    StopRoute::Remote => self.stop_remote_only(reply)?,
                    StopRoute::ServerOnly => {
                        let integrated = self.integrated.as_ref().ok_or(NO_CAPTURE)?;
                        let reply = reply.clone();
                        if !integrated
                            .post(move |g| g.stop_server_capture(StopReason::Normal, Some(reply)))
                        {
                            return Err(NO_ANSWER.into());
                        }
                    }
                }
                Ok(None)
            }
            Request::Status => json_of(&self.profile.status()),
            Request::Open(file) => {
                let captures = horizon_hooks::engine::captures().ok_or_else(unavailable)?;
                captures.open(&game_path(&file))?;
                json_of(&Opened {
                    ok: true,
                    viewer: captures.viewer().to_owned(),
                })
            }
            Request::Share(file) => {
                let file = game_path(&file);
                let reply = reply.clone();
                horizon_hooks::engine::captures().ok_or_else(unavailable)?;
                std::thread::Builder::new()
                    .name("perf-share".into())
                    .spawn(move || {
                        let _ = reply.send(match run_job(Job::Share, &file) {
                            Ok(url) => reply_text(&Shared { ok: true, url }),
                            Err(error) => reply_text(&Failed { error }),
                        });
                    })?;
                Ok(None)
            }
            Request::Mark(name) => {
                let (client, server) = self.profile.mark(parsed.side, &name);
                let posted = server.filter(|_| {
                    self.integrated.as_ref().is_some_and(|integrated| {
                        let label = name.clone();
                        integrated.post(move |g| g.mark_server_capture(&label))
                    })
                });
                let t = client.or(posted).ok_or(NO_CAPTURE)?;
                json_of(&Marked { ok: true, name, t })
            }
        }
    }

    /// `trace [FILE [SECONDS] | stop]`: a timeline-only capture written to FILE; None while a `stop` waits for its file.
    pub(super) fn trace_command(
        &mut self,
        words: &[&str],
        reply: &Sender<String>,
    ) -> Result<Option<serde_json::Value>> {
        match parse_trace(words)? {
            Trace::Status => json_of(&Traced {
                ok: true,
                tracing: self.profile.trace_file.is_some(),
                file: self.profile.trace_file.clone(),
            }),
            Trace::Stop => {
                if self.profile.trace_file.is_none() {
                    return Err("no trace is being written".into());
                }
                self.stop_profile(StopReason::Normal)?;
                self.profile.reply = Some(reply.clone());
                Ok(None)
            }
            Trace::Start { file, seconds } => {
                let options = StartOptions {
                    seconds,
                    samples: false,
                    timeline: true,
                    file: Some(PathBuf::from(file)),
                    ..StartOptions::default()
                };
                self.start_profile(None, &options, Trigger::Ctl)
                    .map_err(|e| format!("trace {file}: {e}"))?;
                json_of(&Traced {
                    ok: true,
                    tracing: true,
                    file: Some(file.to_owned()),
                })
            }
        }
    }

    fn profile_busy(&self) -> bool {
        self.profile.perf.capturing()
            || self.profile.perf.saving()
            || self.profile.server_only_live()
            || !self.profile.server_link.is_idle()
    }

    fn start_profile(
        &mut self,
        side: Option<ProfileSide>,
        options: &StartOptions,
        trigger: Trigger,
    ) -> Result<Started> {
        if self.profile_busy() {
            return Err("a capture is already running or still being saved".into());
        }
        if side == Some(ProfileSide::Server) {
            return self.start_server_only(options, trigger);
        }
        let trigger = if trigger == Trigger::Ctl && self.options.benchmark.is_some() {
            Trigger::Benchmark
        } else {
            trigger
        };
        self.profile.server_only = None;
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
        self.profile.trace_file = options.file.as_ref().map(|f| f.display().to_string());
        self.profile.spans_were_on = spans::recording_here();
        spans::record_here(true);
        self.profile.joined = None;
        self.profile.link = if side.is_none() {
            ServerLink::Pending
        } else {
            ServerLink::Off
        };
        self.join_server_capture();
        if side.is_none() {
            self.join_remote_server(options);
        }
        log::info!("Horizon: profiling the client to {}", path.display());
        Ok(Started {
            ok: true,
            sides: [
                self.profile.served().then_some(ProfileSide::Server),
                Some(ProfileSide::Client),
            ]
            .into_iter()
            .flatten()
            .collect(),
            path: shown(&path),
            seconds: options.seconds,
            samples: options.samples,
            gpu: options.gpu,
        })
    }

    /// `profile server start` in single player: the server thread writes a file of its own.
    fn start_server_only(&mut self, options: &StartOptions, trigger: Trigger) -> Result<Started> {
        let integrated = self
            .integrated
            .as_ref()
            .ok_or("profile server: no single-player world is open")?;
        let began = self.profile.perf.now();
        let live = Live::new();
        let target = Target::Own {
            options: options.clone(),
            trigger,
            dir: reports_dir(),
        };
        let handoff = live.clone();
        let path = integrated
            .call(move |g| g.start_server_capture(target, handoff))
            .ok_or(NO_ANSWER)??;
        let started = Started {
            ok: true,
            sides: vec![ProfileSide::Server],
            path: shown(&path),
            seconds: options.seconds,
            samples: options.samples,
            gpu: GpuSwitch::Off,
        };
        self.profile.server_only = Some(ServerOnly {
            live,
            path,
            since: Instant::now(),
            began,
        });
        Ok(started)
    }

    /// Asks the server thread to write its side into the running capture, once there is a server thread to ask.
    fn join_server_capture(&mut self) {
        if !matches!(self.profile.link, ServerLink::Pending) {
            return;
        }
        let (Some(integrated), Some(join)) = (&self.integrated, self.profile.perf.join()) else {
            return;
        };
        let live = Live::new();
        let handoff = live.clone();
        let posted = integrated.post(move |g| {
            if let Err(e) = g.start_server_capture(Target::Join(join), handoff) {
                log::warn!("Horizon: the server thread is not captured: {e}");
            }
        });
        if posted {
            self.profile.joined = Some(live);
            self.profile.link = ServerLink::Off;
        }
    }

    /// Ends the window's capture and, when the server thread joined it, the server's side too.
    fn stop_profile(&mut self, reason: StopReason) -> Result<()> {
        self.stop_profile_with(reason, Vec::new())
    }

    fn stop_profile_with(&mut self, reason: StopReason, bench: Vec<BenchResult>) -> Result<()> {
        if self.profile.perf.capturing() {
            self.take_memory_snapshot("stop");
        }
        let t = self.profile.perf.now();
        let gpu = self.gpu_readout();
        let server = self.profile.server_live();
        if !self.profile.perf.stop_with(t, reason, gpu, bench) {
            return Err("no capture is running".into());
        }
        if server && let Some(integrated) = &self.integrated {
            integrated.post(move |g| g.stop_server_capture(reason, None));
        }
        self.stop_joined_server();
        self.profile.link = ServerLink::Off;
        self.profile.gpu = GpuSwitch::Off;
        Ok(())
    }

    /// The pass timings the running capture asked for, as the renderer shows them now.
    fn gpu_readout(&self) -> Option<horizon_client::gfx::pack_timing::GpuReadout> {
        gpu_readout(self.profile.gpu, self.renderer.as_ref())
    }

    /// `/profiler start|stop|status|mark`: the requests of `horizon-ctl profile`, answered in chat.
    pub(super) fn profiler_chat(&mut self, args: &str) {
        let line = format!("profiler{args}");
        let tree = Dispatcher::profiler();
        let parse = tree.parse(&line, &Registries::default());
        let (literals, name, id) = match parse.executable() {
            Ok((context, _)) => (
                context.literals.clone(),
                match context.get("name") {
                    Some(Arg::Str(name)) => Some(name.clone()),
                    _ => None,
                },
                match context.get("id") {
                    Some(Arg::Str(id)) => Some(id.clone()),
                    _ => None,
                },
            ),
            Err(e) => return self.syntax_failure(&e, &line),
        };
        if let Some(job) = match literals.get(1).map(String::as_str) {
            Some("open") => Some(Job::Open),
            Some("share") => Some(Job::Share),
            _ => None,
        } {
            self.profiler_job(job, id.as_deref());
            return;
        }
        let owned = profiler_words(&literals, name.as_deref());
        let words: Vec<&str> = owned.iter().map(String::as_str).collect();
        let parsed = match request::parse(&words) {
            Ok(parsed) => parsed,
            Err(e) => return self.profiler_error(&e.to_string()),
        };
        let running = self.profile.status().capturing;
        let refused = match parsed.request {
            Request::Start(_) if running => Some(Text::translate(
                "horizon.perf.already",
                &[command_link("/profiler stop")],
            )),
            Request::Stop { .. } if !running => Some(Text::translate(
                "horizon.perf.nothing",
                &[command_link("/profiler start")],
            )),
            Request::Mark(_) if !running => Some(Text::translate("horizon.perf.mark_idle", &[])),
            _ => None,
        };
        if let Some(refused) = refused {
            self.profiler_refused(refused);
            return;
        }
        let (reply, answer) = std::sync::mpsc::channel();
        match self.run_profile(&words, &reply, Trigger::Command) {
            Ok(None) => {
                if matches!(parsed.request, Request::Stop { .. }) {
                    self.profiler_say(Text::translate("horizon.perf.saving", &[]));
                }
                self.profile.chat_answer = Some(answer);
            }
            Ok(Some(_)) => match parsed.request {
                Request::Start(_) => self.profiler_started(parsed.side),
                Request::Status => self.profiler_status(),
                Request::Mark(name) => self.profiler_say(Text::translate(
                    "horizon.perf.marked",
                    &[Text::literal(name)],
                )),
                Request::Stop { .. } | Request::Open(_) | Request::Share(_) => {}
            },
            Err(e) => self.profiler_error(&e.to_string()),
        }
    }

    fn profiler_say(&mut self, line: Text) {
        self.system_message(profiler_line(line, GRAY));
    }

    fn profiler_refused(&mut self, line: Text) {
        self.system_message(profiler_line(line, RED));
    }

    fn profiler_started(&mut self, side: Option<ProfileSide>) {
        let key = match side {
            None => "horizon.perf.started",
            Some(ProfileSide::Client) => "horizon.perf.started.client",
            Some(ProfileSide::Server) => "horizon.perf.started.server",
        };
        self.profiler_say(Text::translate(key, &[command_link("/profiler stop")]));
    }

    fn profiler_status(&mut self) {
        let status = self.profile.status();
        let client = status.sides.contains(&ProfileSide::Client);
        let server = status.sides.contains(&ProfileSide::Server);
        let line = match status.elapsed_s.filter(|_| status.capturing) {
            Some(elapsed) => Text::translate(
                match (client, server) {
                    (true, false) => "horizon.perf.running.client",
                    (false, true) => "horizon.perf.running.server",
                    _ => "horizon.perf.running.both",
                },
                &[Text::literal(format!("{elapsed:.0}"))],
            ),
            None => Text::translate("horizon.perf.idle", &[command_link("/profiler start")]),
        };
        self.profiler_say(line);
    }

    fn profiler_error(&mut self, error: &str) {
        self.profiler_refused(Text::translate(
            "horizon.perf.error",
            &[Text::literal(error)],
        ));
    }

    /// A capture from chat that asked for both sides, told the server refused to join it.
    pub(super) fn profiler_client_only(&mut self, why: &str) {
        if self.profile.started_by == Trigger::Command {
            self.profiler_refused(Text::translate(
                "horizon.perf.client_only",
                &[Text::literal(why)],
            ));
        }
    }

    fn profiler_saved(&mut self, path: &str) {
        let full = horizon_client::logging::game_home().join(path);
        self.profile.last_capture = Some(full.clone());
        let mut line = Text::translate("horizon.perf.saved", &[Self::capture_file_link(&full)]);
        if horizon_hooks::engine::captures().is_some()
            && let Some(id) = full.file_stem().and_then(|s| s.to_str())
        {
            line = line
                .append(Text::literal(" "))
                .append(capture_button(
                    "horizon.perf.open_button",
                    "horizon.perf.open_hover",
                    format!("/profiler open {id}"),
                ))
                .append(Text::literal(" "))
                .append(capture_button(
                    "horizon.perf.share_button",
                    "horizon.perf.share_hover",
                    format!("/profiler share {id}"),
                ));
        }
        self.profiler_say(line);
    }

    /// The saved file as a chat link that opens its folder.
    fn capture_file_link(path: &Path) -> Text {
        let folder = path.parent().unwrap_or(path).to_string_lossy().into_owned();
        let home = horizon_client::logging::game_home();
        let shown = path
            .strip_prefix(&home)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        Text::styled(
            shown,
            Style {
                underlined: Some(true),
                click: Some(ClickEvent::OpenFile(folder)),
                ..Style::default()
            },
        )
    }

    fn capture_named(&self, id: Option<&str>) -> std::result::Result<PathBuf, Text> {
        resolve_capture(self.profile.last_capture.as_deref(), &reports_dir(), id).map_err(
            |missing| match missing {
                Missing::Last => Text::translate("horizon.perf.share_missing", &[]),
                Missing::Named(id) => {
                    Text::translate("horizon.perf.not_found", &[Text::literal(id)])
                }
            },
        )
    }

    /// `/profiler open|share [ID]`: runs the captures hook on a saved capture, off the game thread.
    fn profiler_job(&mut self, job: Job, id: Option<&str>) {
        if horizon_hooks::engine::captures().is_none() {
            return self.profiler_refused(Text::translate("horizon.perf.unavailable", &[]));
        }
        match self.capture_named(id) {
            Ok(path) => self.queue_profile_job(path, job),
            Err(refused) => self.profiler_refused(refused),
        }
    }

    fn queue_profile_job(&mut self, path: PathBuf, job: Job) {
        let queued = self
            .profile
            .jobs
            .iter()
            .any(|(p, j)| *p == path && *j == job);
        let active = self
            .profile
            .job
            .as_ref()
            .is_some_and(|(p, j, _)| *p == path && *j == job);
        if !queued && !active {
            self.profile.jobs.push_back((path, job));
            if job == Job::Share {
                self.profiler_say(Text::translate("horizon.perf.uploading", &[]));
            }
        }
    }

    fn poll_profile_job(&mut self) {
        if let Some((file, job, receiver)) = &self.profile.job {
            let (file, job) = (file.clone(), *job);
            let result = match receiver.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    Err(horizon_client::lang::get("horizon.perf.upload_interrupted"))
                }
            };
            self.profile.job = None;
            match (job, result) {
                (Job::Open, Ok(_)) => {
                    self.profiler_say(Text::translate("horizon.perf.opened", &[]))
                }
                (Job::Open, Err(error)) => self.profiler_refused(Text::translate(
                    "horizon.perf.open_failed",
                    &[Text::literal(error)],
                )),
                (Job::Share, Ok(url)) => {
                    let saved = Text::translate(
                        "horizon.perf.saved",
                        &[Text::literal(Self::capture_file_link(&file).plain())],
                    );
                    let link = Text::styled(
                        &url,
                        Style {
                            color: Some(GRAY),
                            underlined: Some(true),
                            click: Some(ClickEvent::OpenUrl(url.clone())),
                            hover: Some(Box::new(saved)),
                            ..Style::default()
                        },
                    );
                    log::info!("Horizon: shared performance capture: {url}");
                    self.profiler_say(Text::translate("horizon.perf.shared", &[]));
                    self.system_message(link);
                }
                (Job::Share, Err(error)) => self.profiler_refused(Text::translate(
                    "horizon.perf.upload_failed",
                    &[
                        Text::literal(error),
                        Self::capture_file_link(&file),
                        command_link("/profiler share"),
                    ],
                )),
            }
        }
        if let Some((path, job)) = self.profile.jobs.pop_front() {
            let (send, receive) = std::sync::mpsc::channel();
            let file = path.clone();
            match std::thread::Builder::new()
                .name("perf-share".into())
                .spawn(move || {
                    let _ = send.send(run_job(job, &file));
                }) {
                Ok(_) => self.profile.job = Some((path, job, receive)),
                Err(error) => self.profiler_refused(Text::translate(
                    "horizon.perf.error",
                    &[Text::literal(error.to_string())],
                )),
            }
        }
    }

    fn take_memory_snapshot(&mut self, label: &str) {
        let rows = flatten_report(&self.memory_report());
        let t = self.profile.perf.now();
        self.profile.perf.memory_snapshot(t, label, &rows);
    }

    /// A capture's memory snapshots: one as it starts, then every `MEMORY_EVERY_S`.
    fn poll_memory_snapshot(&mut self) {
        if !self.profile.perf.capturing() {
            self.profile.memory_due = None;
            return;
        }
        let now = self.profile.perf.now();
        match self.profile.memory_due {
            None => {
                self.take_memory_snapshot("start");
                self.profile.memory_due = Some((now + MEMORY_EVERY_S, 0));
            }
            Some((due, taken)) if now >= due => {
                let taken = taken + 1;
                self.take_memory_snapshot(&format!(
                    "{}s",
                    u64::from(taken) * MEMORY_EVERY_S as u64
                ));
                self.profile.memory_due = Some((due + MEMORY_EVERY_S, taken));
            }
            Some(_) => {}
        }
    }

    fn poll_chat_answer(&mut self) {
        let Some(answer) = &self.profile.chat_answer else {
            return;
        };
        let text = match answer.try_recv() {
            Ok(text) => text,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.profile.chat_answer = None;
                return;
            }
        };
        self.profile.chat_answer = None;
        match serde_json::from_str::<ChatAnswer>(&text) {
            Ok(ChatAnswer { error: Some(e), .. }) => self.profiler_error(&e),
            Ok(ChatAnswer {
                removed: Some(_),
                path: Some(path),
                ..
            }) => self.profiler_saved(&path),
            _ => self.profiler_started(Some(ProfileSide::Server)),
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
        // Spans exist only during a capture, which keeps the worst frames whatever the slow threshold is.
        let detail = (!frame_spans.is_empty()).then(|| TickDetail {
            cpu_ms: Some(cpu_ms),
            spans: frame_spans.to_vec(),
        });
        let t = self.profile.perf.now();
        if self.profile.gpu != GpuSwitch::Off
            && let Some(renderer) = self.renderer.as_mut()
        {
            let frames = renderer.take_timed_frames();
            self.profile.perf.gpu_frames(t, cpu_ms, &frames);
        }
        let connection = self
            .remote
            .as_ref()
            .and_then(|r| r.session.counters.lock().unwrap().clone());
        self.profile.perf.set_connection(connection);
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
        if matches!(self.profile.link, ServerLink::Pending) {
            self.join_server_capture();
        }
        if !self.profile.hardware_warmed {
            self.profile.hardware_warmed = true;
            server_perf::warm_hardware();
        }
        if !self.profile.env_tried && server_perf::hardware().is_some() {
            self.profile.env_tried = true;
            let named = |var: &str| std::env::var_os(var).filter(|v| !v.is_empty());
            let gpu = client_perf::env_gpu(
                std::env::var_os(client_perf::ENV_PASS_TIMING).as_deref(),
                std::env::var_os(client_perf::ENV_PASS_DETAIL).as_deref(),
            );
            let (var, options) = if named(ENV_DIR).is_some() || gpu != GpuSwitch::Off {
                (
                    ENV_DIR,
                    StartOptions {
                        gpu,
                        ..StartOptions::default()
                    },
                )
            } else if let Some(file) = named(ENV_TRACE) {
                let options = StartOptions {
                    samples: false,
                    timeline: true,
                    file: Some(PathBuf::from(file)),
                    ..StartOptions::default()
                };
                (ENV_TRACE, options)
            } else {
                return;
            };
            if let Err(e) = self.start_profile(None, &options, Trigger::Env) {
                log::warn!("Horizon: {var}: {e}");
            }
        }
    }

    /// Once per loop turn: the captures whose files are whole, told to whoever waits for them.
    pub(super) fn poll_profile(&mut self) {
        self.poll_server_link();
        self.poll_chat_answer();
        self.poll_memory_snapshot();
        for saved in self.profile.perf.take_saved(false) {
            self.profile_saved(saved);
        }
        self.poll_profile_job();
    }

    /// Ends the capture a closing game still has, and waits for its file.
    pub(super) fn finish_profile(&mut self) {
        if self.profile.perf.capturing() {
            let _ = self.stop_profile(StopReason::Shutdown);
        }
        self.abandon_part();
        // The file is whole only once the server thread's side is, which a closing game's server has no more ticks for.
        if let Some(integrated) = &self.integrated {
            integrated.request_stop();
        }
        for saved in self.profile.perf.take_saved(true) {
            self.profile_saved(saved);
        }
    }

    fn profile_saved(&mut self, saved: std::io::Result<horizon_client::server_perf::Saved>) {
        self.stop_joined_server();
        spans::record_here(self.profile.spans_were_on);
        self.profile.trace_file = None;
        let reply = self.profile.reply.take();
        let in_chat = self.profile.started_by == Trigger::Command && reply.is_none();
        match saved {
            Ok(saved) => {
                for path in &saved.deleted {
                    log::info!("Horizon: removed the oldest capture {}", path.display());
                }
                log::info!("Horizon: {}", files::saved_line(&saved.path));
                self.profile.last_capture =
                    Some(horizon_client::logging::game_home().join(shown(&saved.path)));
                if let Some(reply) = reply {
                    let _ = reply.send(reply_text(&Stopped {
                        ok: true,
                        sides: [
                            self.profile.served().then_some(ProfileSide::Server),
                            Some(ProfileSide::Client),
                        ]
                        .into_iter()
                        .flatten()
                        .collect(),
                        path: shown(&saved.path),
                        open_at: horizon_client::client_perf::request::viewer(),
                        removed: saved.deleted.iter().map(|p| shown(p)).collect(),
                    }));
                }
                if in_chat {
                    self.profiler_saved(&shown(&saved.path));
                }
            }
            Err(e) => {
                log::error!("Horizon: could not write the performance capture: {e}");
                if let Some(reply) = reply {
                    let _ = reply.send(reply_text(&Failed {
                        error: e.to_string(),
                    }));
                }
                if in_chat {
                    self.profiler_error(&e.to_string());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat_request(line: &str) -> std::result::Result<request::Parsed, String> {
        let tree = Dispatcher::profiler();
        let parse = tree.parse(line, &Registries::default());
        let (context, _) = parse.executable().map_err(|e| e.message.plain())?;
        let name = match context.get("name") {
            Some(Arg::Str(name)) => Some(name.as_str()),
            _ => None,
        };
        let owned = profiler_words(&context.literals, name);
        let words: Vec<&str> = owned.iter().map(String::as_str).collect();
        request::parse(&words).map_err(|e| e.to_string())
    }

    #[test]
    fn a_chat_profiler_line_becomes_the_request_it_names() {
        let started = |line| match chat_request(line).unwrap() {
            request::Parsed {
                side,
                request: Request::Start(options),
            } => (side, options.gpu),
            other => panic!("{line}: {other:?}"),
        };
        assert_eq!(started("profiler start"), (None, GpuSwitch::Off));
        assert_eq!(started("profiler start gpu"), (None, GpuSwitch::Passes));
        assert_eq!(
            started("profiler start client gpu_detail"),
            (Some(ProfileSide::Client), GpuSwitch::Detail)
        );
        assert_eq!(
            chat_request("profiler stop server").unwrap().side,
            Some(ProfileSide::Server)
        );
        assert_eq!(
            chat_request("profiler mark slow chunk").unwrap().request,
            Request::Mark("slow chunk".into())
        );
        for line in [
            "profiler start --gpu",
            "profiler start --seconds 30",
            "profiler stop gpu",
        ] {
            assert!(chat_request(line).is_err(), "{line}");
        }
    }

    #[test]
    fn open_and_share_find_a_named_capture_in_the_reports_folder_or_the_last_one() {
        let dir = std::env::temp_dir().join(format!("horizon-resolve-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let name = "profile-20251107-143844Z";
        std::fs::write(dir.join(format!("{name}.hzperf")), b"x").unwrap();
        std::fs::write(dir.join("notes.hzperf"), b"x").unwrap();
        let found = |id| resolve_capture(None, &dir, id);
        assert_eq!(found(Some(name)), Ok(dir.join(format!("{name}.hzperf"))));
        assert_eq!(found(None), Err(Missing::Last));
        for id in ["notes", "profile-20251107-000000Z", "..", "../notes"] {
            assert_eq!(found(Some(id)), Err(Missing::Named(id.into())), "{id}");
        }
        let last = dir.join("trace.hzperf");
        assert_eq!(resolve_capture(Some(&last), &dir, None), Ok(last.clone()));
        assert_eq!(resolve_capture(Some(&last), &dir, Some("trace")), Ok(last));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn start<'a>(
        file: &'a str,
        seconds: Option<u32>,
    ) -> std::result::Result<Trace<'a>, &'static str> {
        Ok(Trace::Start { file, seconds })
    }

    #[test]
    fn trace_words() {
        assert_eq!(parse_trace(&[]), Ok(Trace::Status));
        assert_eq!(parse_trace(&["stop"]), Ok(Trace::Stop));
        assert_eq!(
            parse_trace(&["/tmp/a.hzperf"]),
            start("/tmp/a.hzperf", None)
        );
        assert_eq!(parse_trace(&["a", "2"]), start("a", Some(2)));
        assert_eq!(parse_trace(&["a", "1"]), start("a", Some(1)));
        assert_eq!(parse_trace(&["a", "600"]), start("a", Some(600)));
    }

    #[test]
    fn trace_seconds_out_of_range_are_usage_errors() {
        for bad in ["0", "601", "99999", "-1", "1.5", "x", ""] {
            assert_eq!(
                parse_trace(&["a", bad]),
                Err(TRACE_USAGE),
                "seconds {bad:?}"
            );
        }
        assert_eq!(parse_trace(&["a", "2", "3"]), Err(TRACE_USAGE));
    }
    use crate::game::integrated::capture::Phase;
    use std::time::Duration;

    fn live(phase: Phase) -> Arc<Live> {
        let live = Live::new();
        live.set(phase);
        live
    }

    fn server_only(phase: Phase, running_for: Duration, path: PathBuf) -> ServerOnly {
        ServerOnly {
            live: live(phase),
            path,
            since: Instant::now() - running_for,
            began: 0.0,
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("hz-profile-route-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A window capture that the server thread joined, in `phase`.
    fn joined(dir: &Path, phase: Phase) -> Profile {
        let mut profile = Profile::new(50.0);
        let options = StartOptions {
            samples: false,
            ..StartOptions::default()
        };
        std::thread::sleep(Duration::from_millis(30));
        let t = profile.perf.now();
        profile
            .perf
            .start(
                t,
                dir,
                Trigger::Ctl,
                &options,
                Default::default(),
                Vec::new(),
            )
            .unwrap();
        profile.joined = Some(live(phase));
        profile
    }

    #[test]
    fn a_server_mark_in_a_joined_capture_is_stamped_on_the_window_clock() {
        let dir = scratch("joined-mark");
        let mut profile = joined(&dir, Phase::Running);
        // A finished server-only capture, 1000 s old, must not lend its clock.
        profile.server_only = Some(server_only(
            Phase::Ended,
            Duration::from_secs(1000),
            dir.join("old.hzperf"),
        ));
        let before = profile.perf.now();
        let (client, server) = profile.mark(Some(ProfileSide::Server), "boss");
        assert_eq!(client, None);
        let t = server.expect("a joined capture takes a server mark");
        // The capture began 30 ms into the game, so a capture-relative `t` would fall short of `before`.
        assert!(
            (before..=profile.perf.now()).contains(&t),
            "{t} vs {before}"
        );

        let (client, server) = profile.mark(None, "both");
        assert_eq!(client, server);
        assert!(client.is_some());

        let (client, server) = profile.mark(Some(ProfileSide::Client), "mine");
        assert!(client.is_some());
        assert_eq!(server, None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_mark_is_refused_when_no_running_capture_has_the_side() {
        let dir = scratch("no-mark");
        let mut profile = joined(&dir, Phase::Failed);
        assert_eq!(
            profile.mark(Some(ProfileSide::Server), "x"),
            (None, None),
            "the server never joined"
        );
        profile.server_only = Some(server_only(
            Phase::Ended,
            Duration::from_secs(5),
            dir.join("a"),
        ));
        assert_eq!(profile.mark(Some(ProfileSide::Server), "x"), (None, None));
        assert_eq!(Profile::new(50.0).mark(None, "x"), (None, None));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_server_only_mark_is_stamped_on_the_window_clock() {
        let mut profile = Profile::new(50.0);
        let mut alone = server_only(Phase::Running, Duration::from_secs(30), PathBuf::from("a"));
        alone.began = 100.0;
        profile.server_only = Some(alone);
        let (client, server) = profile.mark(None, "x");
        assert_eq!(client, None);
        assert!((130.0..140.0).contains(&server.unwrap()), "{server:?}");
    }

    #[test]
    fn status_reports_only_captures_that_are_running() {
        let dir = scratch("status");
        let file = dir.join("capture.hzperf");
        std::fs::write(&file, [0u8; 7]).unwrap();
        let mut profile = Profile::new(50.0);
        profile.server_only = Some(server_only(
            Phase::Running,
            Duration::from_secs(4),
            file.clone(),
        ));
        let running = profile.status();
        assert!(running.capturing);
        assert_eq!(running.sides, [ProfileSide::Server]);
        assert_eq!(running.path, Some(shown(&file)));
        assert_eq!(running.bytes, Some(7));
        assert!(running.elapsed_s.unwrap() >= 4.0);

        // After `profile server stop`, or when a timed capture ends, the old file is not reported.
        profile.server_only.as_ref().unwrap().live.set(Phase::Ended);
        let done = profile.status();
        assert!(!done.capturing);
        assert!(done.sides.is_empty());
        assert_eq!((done.path, done.elapsed_s, done.bytes), (None, None, None));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_joined_capture_lists_the_server_first_until_the_server_ends() {
        let dir = scratch("sides");
        let profile = joined(&dir, Phase::Running);
        let status = profile.status();
        assert!(status.capturing);
        assert_eq!(status.sides, [ProfileSide::Server, ProfileSide::Client]);
        assert!(status.path.is_some());
        profile.joined.as_ref().unwrap().set(Phase::Ended);
        assert_eq!(profile.status().sides, [ProfileSide::Client]);
        assert!(
            profile.served(),
            "an ended server side still holds its part of the file"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stop_goes_to_the_capture_that_is_running() {
        let dir = scratch("stop");
        let none = Profile::new(50.0);
        for side in [None, Some(ProfileSide::Client), Some(ProfileSide::Server)] {
            assert!(none.stop_route(side).is_err(), "{side:?}");
        }

        let both = joined(&dir, Phase::Running);
        for side in [None, Some(ProfileSide::Client), Some(ProfileSide::Server)] {
            assert_eq!(both.stop_route(side), Ok(StopRoute::Window), "{side:?}");
        }

        let alone = joined(&dir, Phase::Failed);
        assert_eq!(alone.stop_route(None), Ok(StopRoute::Window));
        assert_eq!(
            alone.stop_route(Some(ProfileSide::Client)),
            Ok(StopRoute::Window)
        );
        assert!(alone.stop_route(Some(ProfileSide::Server)).is_err());

        let mut server = Profile::new(50.0);
        server.server_only = Some(server_only(
            Phase::Running,
            Duration::ZERO,
            PathBuf::from("a"),
        ));
        assert_eq!(server.stop_route(None), Ok(StopRoute::ServerOnly));
        assert_eq!(
            server.stop_route(Some(ProfileSide::Server)),
            Ok(StopRoute::ServerOnly)
        );
        assert!(server.stop_route(Some(ProfileSide::Client)).is_err());

        server.server_only.as_ref().unwrap().live.set(Phase::Ended);
        for side in [None, Some(ProfileSide::Server)] {
            assert!(server.stop_route(side).is_err(), "{side:?}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
