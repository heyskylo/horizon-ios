//! `horizon-report [SECONDS]` and `horizon-trace [SECONDS]` on the dedicated server's console, and `HORIZON_TRACE`: record a window of ticks into `reports/profile-<time>Z.hzperf`.
use super::super::{
    Dim, Game,
    perf_remote::{Asked, Done},
    profile_capture::shown,
    tick_profile,
};
use horizon_client::client_perf::request::Threads;
use horizon_client::perf_net::Refusal;
use horizon_client::server_perf::{
    self, CaptureEnd, CaptureOptions, SamplerOptions, ServerPerf, WorldCounts,
    traffic::Wire,
    world::{LevelCounts, MobCost, MobCosts, WorkerJobs},
};
use horizon_client::{spans, streaming::job_stats};
use horizon_perf_format::{
    files,
    v1::{GpuMode, HorizonOptions, Start, StopReason, Trigger},
};
use horizon_world::level::BlockId;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// The longest window a report records.
pub(super) const MAX_SECONDS: u32 = 600;

/// What `horizon-report` was asked.
#[derive(Debug, PartialEq)]
pub(super) enum Command {
    Record {
        seconds: u32,
        samples: bool,
        timeline: bool,
    },
    Stop,
    Status,
}

/// What a capture is asked to record, and how it was asked.
pub(super) struct Plan {
    pub seconds: u32,
    pub samples: bool,
    pub timeline: bool,
    /// Where the capture is written when it is not `reports/`.
    pub file: Option<PathBuf>,
    pub trigger: Trigger,
    pub sampler: SamplerOptions,
}

/// The longest `horizon-trace` records when no seconds are given.
const TRACE_DEFAULT_SECONDS: u32 = 30;

/// The words after `horizon-trace`: `[SECONDS | stop]`, a timeline-only capture.
pub(super) fn parse_trace(words: &[&str]) -> Result<Command, ()> {
    match words {
        [] => Ok(Command::Record {
            seconds: TRACE_DEFAULT_SECONDS,
            samples: false,
            timeline: true,
        }),
        ["stop"] => Ok(Command::Stop),
        [seconds] => match seconds.parse::<u32>() {
            Ok(seconds) if (1..=MAX_SECONDS).contains(&seconds) => Ok(Command::Record {
                seconds,
                samples: false,
                timeline: true,
            }),
            _ => Err(()),
        },
        _ => Err(()),
    }
}

/// The words after `horizon-report`: `[SECONDS | stop | status] [--no-samples] [--timeline] [--profile]`, flags anywhere.
pub(super) fn parse(words: &[&str]) -> Result<Command, ()> {
    let mut samples = true;
    let mut timeline = false;
    let mut positional = Vec::new();
    for &word in words {
        match word {
            "--no-samples" => samples = false,
            "--timeline" => timeline = true,
            // Samples are on by default now; the old switch is accepted and does nothing.
            "--profile" => {}
            flag if flag.starts_with("--") => return Err(()),
            word => positional.push(word),
        }
    }
    match positional[..] {
        [] => Ok(Command::Record {
            seconds: 60,
            samples,
            timeline,
        }),
        ["stop"] => Ok(Command::Stop),
        ["status"] => Ok(Command::Status),
        [seconds] => match seconds.parse::<u32>() {
            Ok(seconds) if (1..=MAX_SECONDS).contains(&seconds) => Ok(Command::Record {
                seconds,
                samples,
                timeline,
            }),
            _ => Err(()),
        },
        _ => Err(()),
    }
}

/// Logs the captures whose files are done; with `wait`, every one still being finished.
pub(in crate::game) fn log_saved(perf: &mut ServerPerf, wait: bool) -> Vec<Done> {
    perf.take_saved(wait)
        .into_iter()
        .map(|saved| match saved {
            Ok(saved) => {
                for path in &saved.deleted {
                    log::info!("Horizon: removed the oldest capture {}", path.display());
                }
                log::info!("Horizon: {}", files::saved_line(&saved.path));
                Done::Saved {
                    path: shown(&saved.path),
                    file: saved.path.clone(),
                    removed: saved.deleted.iter().map(|p| shown(p)).collect(),
                }
            }
            Err(e) => {
                log::error!("Horizon: could not write the performance capture: {e}");
                Done::Failed
            }
        })
        .collect()
}

/// Registry ids for the loose entities a level counts apart from its mobs: items, orbs, projectiles, minecarts, boats.
const LOOSE_ENTITIES: [&str; 5] = [
    "minecraft:item",
    "minecraft:experience_orb",
    "horizon:projectile",
    "minecraft:minecart",
    "minecraft:boat",
];

/// A report being recorded: what the game side needs while the capture runs.
pub(in crate::game) struct Recording {
    seconds: u32,
    /// Whether it turned the spans of every thread on, which it turns off again.
    all_threads: bool,
    started: Instant,
    ticks: u64,
    /// The ticks whose mobs were timed one by one ([`tick_profile::SAMPLE_EVERY`]).
    sampled_ticks: u64,
}

impl Recording {
    pub(super) fn start(seconds: u32) -> Self {
        tick_profile::want_spans(tick_profile::SPANS_REPORT, true);
        Self::new(seconds, true)
    }
    /// A recording beside a window: the calling thread's spans are on already (`spans::record_here`), the other threads' stay off.
    pub(in crate::game) fn here(seconds: u32) -> Self {
        Self::new(seconds, false)
    }
    fn new(seconds: u32, all_threads: bool) -> Self {
        job_stats::enable(true);
        job_stats::take();
        tick_profile::take();
        Self {
            seconds,
            all_threads,
            started: Instant::now(),
            ticks: 0,
            sampled_ticks: 0,
        }
    }
    /// Whether the next tick's mobs are timed one by one.
    pub(in crate::game) fn sample_next(&self) -> bool {
        self.ticks.is_multiple_of(tick_profile::SAMPLE_EVERY as u64)
    }
    pub(in crate::game) fn add(&mut self, sampled: bool) {
        self.ticks += 1;
        self.sampled_ticks += u64::from(sampled);
    }
    pub(super) fn done(&self) -> bool {
        self.started.elapsed() >= Duration::from_secs(u64::from(self.seconds))
    }
    /// `horizon-report status`: how far the recording is.
    pub(super) fn status(&self) -> String {
        format!(
            "Recording a performance report: {} of {} s, {} ticks (horizon-report stop writes it now)",
            self.started.elapsed().as_secs(),
            self.seconds,
            self.ticks
        )
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        if self.all_threads {
            tick_profile::want_spans(tick_profile::SPANS_REPORT, false);
        }
        job_stats::enable(false);
    }
}

/// What one span costs on this thread with spans on: the report's own overhead. The spans it
/// records are thrown away.
fn span_cost_nanos() -> f64 {
    const N: u32 = 5_000;
    let kept = spans::take_raw();
    let timer = Instant::now();
    for _ in 0..N {
        drop(spans::guard("calibration"));
    }
    let per = timer.elapsed().as_nanos() as f64 / f64::from(N);
    spans::take_raw();
    debug_assert!(kept.is_empty(), "the tick takes its spans");
    per
}

impl Game<'_> {
    /// Starts a report: the recording the game side needs and the capture beside it.
    pub(super) fn record_report(
        &self,
        state: &mut super::ServerState,
        plan: &Plan,
    ) -> Option<PathBuf> {
        state.report = Some(Recording::start(plan.seconds));
        self.start_perf_capture(state, plan)
    }

    /// Begins and ends the captures guests asked for, between two ticks.
    pub(super) fn serve_perf_requests(&mut self, state: &mut super::ServerState) {
        self.pump_perf_files();
        for asked in self.take_perf_asked() {
            match asked {
                Asked::Start { conn, name, wanted } => {
                    if state.report.is_some() || state.perf.capturing() || state.perf.finishing() {
                        self.perf_refused(conn, Refusal::AlreadyRunning);
                        continue;
                    }
                    let options = wanted.options();
                    let plan = Plan {
                        seconds: options.seconds.unwrap_or(MAX_SECONDS),
                        samples: options.samples,
                        timeline: options.timeline,
                        file: None,
                        trigger: Trigger::Remote,
                        sampler: SamplerOptions {
                            hz: options.hz,
                            over_ms: options.over_ms,
                            wall: options.wall,
                            all_threads: options.threads == Threads::All,
                        },
                    };
                    match self.record_report(state, &plan) {
                        Some(path) => self.perf_started(conn, name, &wanted, &shown(&path)),
                        None => {
                            state.report = None;
                            log::error!("Horizon: {name}'s server profile did not start");
                            self.perf_refused(conn, Refusal::Failed);
                        }
                    }
                }
                Asked::Stop => self.finish_perf_capture(state, StopReason::Normal),
            }
        }
    }

    /// Opens the `.hzperf` that records beside a report, and logs where it goes.
    pub(super) fn start_perf_capture(
        &self,
        state: &mut super::ServerState,
        plan: &Plan,
    ) -> Option<PathBuf> {
        let options = self.capture_options(
            plan,
            (state.view, state.max_players, state.compression),
            state.horizon,
        );
        let t = state.perf_epoch.elapsed().as_secs_f64();
        match state.perf.start_capture(t, Path::new("reports"), options) {
            Ok(path) => {
                log::info!("Horizon: capturing to {}", path.display());
                Some(path)
            }
            Err(e) => {
                log::error!("Horizon: could not start the performance capture: {e}");
                None
            }
        }
    }

    /// Every block's name by the id the sampler's context holds.
    pub(in crate::game) fn block_names(&self) -> Vec<String> {
        let table = &self.res.behaviours().table;
        (0..table.block_count() as u16)
            .map(|id| table.info(BlockId(id)).name.to_string())
            .collect()
    }

    /// What a capture of `plan` records of the server as it starts: the typed allowlists, the world and the connections.
    fn capture_options(
        &self,
        plan: &Plan,
        (view, max_players, compression): ((u32, u32), usize, i32),
        horizon: HorizonOptions,
    ) -> CaptureOptions {
        let start = Start {
            build: Some(server_perf::build()),
            machine: Some(server_perf::machine(server_perf::hardware())),
            settings: Some(server_perf::settings(
                view.0,
                view.1,
                max_players as u32,
                compression,
            )),
            game_rules: Some(server_perf::game_rules(&self.ui.rules)),
            horizon_options: Some(horizon),
            players: self.lan.as_ref().map_or(0, |h| h.hub.guests.len() as u32),
            ..Start::default()
        };
        CaptureOptions {
            window_s: Some(f64::from(plan.seconds)),
            trigger: plan.trigger,
            start,
            samples: plan.samples,
            sampler: plan.sampler,
            block_names: self.block_names(),
            levels: self.level_counts(),
            wire: self.wire_bytes(),
            gpu: GpuMode::Unspecified,
            timeline: plan.timeline.then(Default::default),
            file: plan.file.clone(),
        }
    }

    /// What the health samples count: players, chunks and mobs in every level.
    pub(in crate::game) fn perf_world_counts(&self) -> WorldCounts {
        let (mut chunks, mut entities) = (self.world.chunks.len(), self.mobs.len());
        for dim in self.levels.parked_dims() {
            chunks += self.levels.parked_world(dim).map_or(0, |w| w.chunks.len());
            entities += self.levels.parked_mobs(dim).map_or(0, <[_]>::len);
        }
        WorldCounts {
            players: self.lan.as_ref().map_or(0, |h| h.hub.guests.len() as u32),
            chunks: chunks as u32,
            entities: entities as u32,
        }
    }

    /// The level in place and the parked ones: their chunks and their entities and block entities by registry id.
    pub(in crate::game) fn level_counts(&self) -> Vec<LevelCounts> {
        let level = |dim: Dim,
                     world: &horizon_client::world::World,
                     mobs: &[horizon_client::mobs::Mob],
                     loose: [usize; 5]| {
            let mut entities: BTreeMap<String, u32> = BTreeMap::new();
            for m in mobs.iter().filter(|m| !m.removed) {
                *entities.entry(m.kind.id.clone()).or_default() += 1;
            }
            for (id, n) in LOOSE_ENTITIES.into_iter().zip(loose) {
                if n > 0 {
                    *entities.entry(id.to_owned()).or_default() += n as u32;
                }
            }
            let mut block_entities: BTreeMap<String, u32> = BTreeMap::new();
            for entity in world.block_entities.values() {
                *block_entities.entry(entity.id().to_owned()).or_default() += 1;
            }
            LevelCounts {
                dimension: dim.id().to_owned(),
                chunks: world.chunks.len() as u32,
                entities: entities.into_iter().collect(),
                block_entities: block_entities.into_iter().collect(),
            }
        };
        let mut levels = vec![level(
            self.dim,
            &self.world,
            &self.mobs,
            [
                self.dropped.len(),
                self.orbs.len(),
                self.creatures.projectiles.len(),
                self.carts.len(),
                self.boats.len(),
            ],
        )];
        for dim in self.levels.parked_dims() {
            if let (Some(world), Some(mobs)) =
                (self.levels.parked_world(dim), self.levels.parked_mobs(dim))
            {
                let loose = self.levels.parked_entity_counts(dim).unwrap_or_default();
                levels.push(level(dim, world, mobs, loose));
            }
        }
        levels
    }

    /// Each player's wire bytes (in, out) by connection.
    pub(in crate::game) fn wire_bytes(&self) -> Wire {
        self.lan.as_ref().map_or_else(Vec::new, |h| {
            h.hub
                .guests
                .iter()
                .filter_map(|g| Some((g.conn, g.sender.wire_bytes()?)))
                .collect()
        })
    }

    /// Ends the capture and the recording beside it, if they run; `log_saved` reports the file when it is done.
    pub(super) fn finish_perf_capture(&self, state: &mut super::ServerState, reason: StopReason) {
        let recording = state.report.take();
        if !state.perf.capturing() {
            return;
        }
        let end = recording.map_or_else(CaptureEnd::default, |r| self.capture_end(&r));
        let t = state.perf_epoch.elapsed().as_secs_f64();
        state.perf.begin_stop(t, reason, end);
    }

    /// What a finished recording leaves for the capture: the world as it stands, the sampled mob costs and the workers' time.
    pub(in crate::game) fn capture_end(&self, recording: &Recording) -> CaptureEnd {
        let span_ns = span_cost_nanos();
        let mob_ns = tick_profile::sample_cost_nanos();
        // Off unless `HORIZON_SPIKES` or a mod (`native_spans`) still wants them.
        if recording.all_threads {
            tick_profile::want_spans(tick_profile::SPANS_REPORT, false);
        }
        job_stats::enable(false);
        CaptureEnd {
            levels: self.level_counts(),
            mob_costs: MobCosts {
                costs: tick_profile::take()
                    .into_iter()
                    .map(|c| MobCost {
                        dimension: c.dimension.to_owned(),
                        kind: c.kind,
                        nanos: c.nanos,
                        mob_ticks: c.mob_ticks,
                    })
                    .collect(),
                sampled_ticks: recording.sampled_ticks,
            },
            jobs: job_stats::take()
                .into_iter()
                .map(|(kind, nanos, count)| WorkerJobs {
                    kind: kind.to_owned(),
                    nanos,
                    count,
                })
                .collect(),
            wire: self.wire_bytes(),
            span_ns,
            mob_ns,
            gpu: None,
            bench: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_words_pick_the_window_and_the_samples() {
        let record = |seconds, samples| {
            Ok(Command::Record {
                seconds,
                samples,
                timeline: false,
            })
        };
        assert_eq!(parse(&[]), record(60, true));
        assert_eq!(parse(&["120"]), record(120, true));
        assert_eq!(parse(&["--no-samples", "30"]), record(30, false));
        assert_eq!(parse(&["30", "--no-samples"]), record(30, false));
        assert_eq!(parse(&["--profile"]), record(60, true));
        assert_eq!(
            parse(&["--timeline", "10"]),
            Ok(Command::Record {
                seconds: 10,
                samples: true,
                timeline: true
            })
        );
        assert_eq!(parse(&["stop"]), Ok(Command::Stop));
        assert_eq!(parse(&["status", "--no-samples"]), Ok(Command::Status));
        for bad in [
            &["0"][..],
            &["601"],
            &["abc"],
            &["--fast"],
            &["30", "60"],
            &["stop", "5"],
        ] {
            assert_eq!(parse(bad), Err(()), "{bad:?}");
        }
    }

    #[test]
    fn trace_words_pick_a_timeline_only_window() {
        let trace = |seconds| {
            Ok(Command::Record {
                seconds,
                samples: false,
                timeline: true,
            })
        };
        assert_eq!(parse_trace(&[]), trace(30));
        assert_eq!(parse_trace(&["45"]), trace(45));
        assert_eq!(parse_trace(&["600"]), trace(600));
        assert_eq!(parse_trace(&["stop"]), Ok(Command::Stop));
        for bad in [&["0"][..], &["601"], &["x"], &["10", "20"], &["stop", "1"]] {
            assert_eq!(parse_trace(bad), Err(()), "{bad:?}");
        }
    }

    /// One tick in four has its mobs timed one by one, starting with the first.
    #[test]
    fn one_tick_in_four_is_sampled() {
        let mut r = Recording {
            seconds: 1,
            all_threads: false,
            started: Instant::now(),
            ticks: 0,
            sampled_ticks: 0,
        };
        for _ in 0..20 {
            let sampled = r.sample_next();
            r.add(sampled);
        }
        assert_eq!((r.ticks, r.sampled_ticks), (20, 5));
    }

    /// A recording that ends without `capture_end` (the capture failed to start, or shutdown) leaves spans off.
    #[test]
    fn a_dropped_recording_turns_spans_off() {
        drop(Recording::start(1));
        assert!(!tick_profile::wanted(tick_profile::SPANS_REPORT));
    }

    /// On this machine, timing a span and a mob each cost well under a millisecond.
    #[test]
    fn the_prices_of_a_span_and_a_mob_sample_are_small() {
        let (span_ns, mob_ns) = (span_cost_nanos(), tick_profile::sample_cost_nanos());
        assert!(span_ns > 0.0 && span_ns < 50_000.0, "{span_ns}");
        assert!(mob_ns > 0.0 && mob_ns < 50_000.0, "{mob_ns}");
    }

    fn server_game(root: &Path) -> Game<'static> {
        use super::super::super::{Audio, Options, Progress, Settings, load_resources};
        let settings = Settings::default();
        let loaded = load_resources(
            root.to_path_buf(),
            None,
            root,
            &settings,
            Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.to_path_buf();
        options.execution_role =
            super::super::super::execution_role::ExecutionRole::IntegratedServer;
        let resources = Box::leak(Box::new(loaded.resources));
        Game::new(
            resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(root),
            None,
        )
        .unwrap()
    }

    /// A server that holds a seed, a world name, a player's name, a named mob and a sign writes a capture with none of them.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_capture_of_a_server_with_names_text_and_a_seed_holds_none_of_them() {
        use horizon_client::containers::BlockEntity;
        use horizon_client::signs::{SignData, SignText};
        use std::io::Read;
        let root = PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let mut game = server_game(&root);
        game.level.name = "SecretWorldName".into();
        game.level.seed = 8_675_309;
        game.options.player_name = Some("SteveSecret".into());
        let kind = game.res.mob_types().get("minecraft:cow").unwrap().clone();
        let at = horizon_core::geometry::Vec3::new(0.5, 4.0, 0.5);
        let mut cow = horizon_client::mobs::Mob::new(1, kind, at, 0.0);
        cow.custom_name = Some("Zorblax".into());
        game.mobs.push(cow);
        let sign = SignData {
            front: SignText {
                lines: [
                    "SecretSignText".into(),
                    String::new(),
                    String::new(),
                    String::new(),
                ],
                ..SignText::default()
            },
            ..SignData::default()
        };
        game.world
            .block_entities
            .insert((1, 64, 1), BlockEntity::Sign(sign));

        let dir = std::env::temp_dir().join(format!("hz-privacy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut perf = server_perf::ServerPerf::new(0.0, 250.0);
        let horizon = server_perf::horizon_options(12, 0, true);
        let plan = Plan {
            seconds: 2,
            samples: true,
            timeline: false,
            file: None,
            trigger: Trigger::Console,
            sampler: SamplerOptions::default(),
        };
        let options = game.capture_options(&plan, ((10, 8), 20, 256), horizon);
        perf.start_capture(0.0, &dir, options).unwrap();
        for i in 0..40 {
            let detail = server_perf::TickDetail {
                cpu_ms: None,
                spans: vec![spans::Span {
                    name: "tick",
                    ms: 10.0,
                    cpu_ms: 9.0,
                    depth: 0,
                }],
            };
            let t = f64::from(i) / 20.0;
            perf.tick(
                t,
                10.0,
                Some(detail),
                || game.perf_world_counts(),
                || game.wire_bytes(),
            );
        }
        let recording = Recording {
            seconds: 2,
            all_threads: false,
            started: Instant::now(),
            ticks: 40,
            sampled_ticks: 10,
        };
        let end = game.capture_end(&recording);
        let saved = perf
            .stop_capture(2.0, StopReason::Normal, end)
            .unwrap()
            .unwrap();

        let gz = std::fs::read(&saved.path).unwrap();
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(&gz[..])
            .read_to_end(&mut bytes)
            .unwrap();
        let has = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
        for secret in [
            "SecretWorldName",
            "SteveSecret",
            "Zorblax",
            "SecretSignText",
            "8675309",
        ] {
            assert!(!has(secret.as_bytes()), "the capture holds {secret}");
        }
        // The entities and block entities are there, by type only.
        let c = horizon_perf::read_capture(&gz).unwrap();
        let text = |id: u32| c.strings.iter().find(|s| s.id == id).unwrap().text.as_str();
        let stop = c.world.last().unwrap();
        let types = |counts: &[horizon_perf_format::v1::TypeCount]| {
            counts
                .iter()
                .map(|t| (text(t.r#type), t.count))
                .collect::<Vec<_>>()
        };
        assert_eq!(text(stop.dims[0].dimension), "minecraft:overworld");
        assert_eq!(types(&stop.dims[0].entities), [("minecraft:cow", 1)]);
        assert_eq!(types(&stop.dims[0].block_entities).len(), 1);
        // Every string is a registry id, a span or job name, or the thread: no free text.
        for s in &c.strings {
            let plain = s
                .text
                .chars()
                .all(|ch| matches!(ch, 'a'..='z' | '0'..='9' | '_' | '.' | ':' | '/' | '-'));
            assert!(plain || s.text == "Server thread", "{}", s.text);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
