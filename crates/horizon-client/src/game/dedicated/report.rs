//! `horizon-report [SECONDS]` on the dedicated server's console (and `horizon-server report`): records every
//! tick's time and spans for a window (60 s unless told; `horizon-report stop` ends it early, `horizon-report
//! status` says how far it is), then writes a performance report to share, `reports/perf-<time>.json` and a
//! short `.txt` beside it in the server's folder. It says where the ticks' time went (the spans of
//! `HORIZON_SPIKES`, each level's share), what each mob type costs (sampled, tick_profile.rs), what the chunk
//! workers spent their CPU on (load, generate, decorate, light...), the network bytes per player, and what
//! recording all this cost the tick. It shows the worst ticks with their spans, counts the entities by type and
//! the chunks of each level, and gives the memory, the CPU and the view settings. It holds no player names,
//! UUIDs or addresses, no seed and no world name. It runs beside the game: nothing restarts.
use super::super::{Dim, Game, tick_profile};
use horizon_client::{
    spans::{self, Span},
    streaming::job_stats,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// The longest window a report records.
pub(super) const MAX_SECONDS: u32 = 600;
/// The worst ticks a report lists with their spans.
const WORST: usize = 10;
/// The mob types (per level) a report lists with their cost.
const MOB_TYPES: usize = 40;

struct Tick {
    wall_ms: f32,
    cpu_ms: Option<f32>,
    spans: Vec<Span>,
}

/// A report being recorded.
pub(super) struct Recording {
    seconds: u32,
    started: Instant,
    cpu_s: Option<f64>,
    /// Whether spans were on before (`HORIZON_SPIKES`): they stay on after.
    spans_were_on: bool,
    ticks: Vec<Tick>,
    /// The ticks whose mobs were timed one by one ([`tick_profile::SAMPLE_EVERY`]).
    sampled_ticks: u64,
    /// The spans recorded, before they are summed by name: what timing them cost.
    spans_recorded: u64,
    /// Each player's wire bytes (in, out) by connection when the recording started.
    wire_start: BTreeMap<u32, (u64, u64)>,
}

impl Recording {
    pub(super) fn start(seconds: u32, wire: Vec<(u32, (u64, u64))>) -> Self {
        let spans_were_on = spans::enabled();
        tick_profile::want_spans(tick_profile::SPANS_REPORT, true);
        job_stats::enable(true);
        job_stats::take();
        tick_profile::take();
        Self {
            seconds,
            started: Instant::now(),
            cpu_s: horizon_bench::cpu_seconds(std::process::id()),
            spans_were_on,
            ticks: Vec::with_capacity(seconds as usize * 20),
            sampled_ticks: 0,
            spans_recorded: 0,
            wire_start: wire.into_iter().collect(),
        }
    }
    /// Whether the next tick's mobs are timed one by one.
    pub(super) fn sample_next(&self) -> bool {
        self.ticks.len().is_multiple_of(tick_profile::SAMPLE_EVERY)
    }
    pub(super) fn add(
        &mut self,
        wall_ms: f32,
        cpu_ms: Option<f32>,
        spans: Vec<Span>,
        recorded: usize,
        sampled: bool,
    ) {
        self.ticks.push(Tick {
            wall_ms,
            cpu_ms,
            spans,
        });
        self.sampled_ticks += u64::from(sampled);
        self.spans_recorded += recorded as u64;
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
            self.ticks.len()
        )
    }
}

fn pct(sorted: &[f32], q: f64) -> f64 {
    let i = ((sorted.len() as f64 * q) as usize).min(sorted.len().saturating_sub(1));
    sorted.get(i).map_or(0.0, |&v| f64::from(v))
}
fn round(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// The tick times and spans of a recording, as the report's `ticks` and `spans`.
fn tick_summary(r: &Recording, window_s: f64) -> (Value, Value, Value) {
    let mut walls: Vec<f32> = r.ticks.iter().map(|t| t.wall_ms).collect();
    walls.sort_by(f32::total_cmp);
    let n = r.ticks.len().max(1) as f64;
    let mean = |f: &dyn Fn(&Tick) -> f64| r.ticks.iter().map(f).sum::<f64>() / n;
    let ticks = json!({
        "count": r.ticks.len(),
        "tps": round(r.ticks.len() as f64 / window_s.max(1e-9)),
        "mspt": {
            "average": round(mean(&|t| f64::from(t.wall_ms))),
            "p50": round(pct(&walls, 0.5)),
            "p95": round(pct(&walls, 0.95)),
            "p99": round(pct(&walls, 0.99)),
            "max": round(walls.last().copied().map_or(0.0, f64::from)),
        },
        "cpu_ms_average": round(mean(&|t| f64::from(t.cpu_ms.unwrap_or(t.wall_ms)))),
        "over_50_ms": walls.iter().filter(|&&w| w > 50.0).count(),
        "over_100_ms": walls.iter().filter(|&&w| w > 100.0).count(),
    });
    let mut totals: BTreeMap<&str, (f64, f64, usize)> = BTreeMap::new();
    for t in &r.ticks {
        for s in &t.spans {
            let e = totals.entry(s.name).or_default();
            e.0 += f64::from(s.ms);
            e.1 += f64::from(s.cpu_ms);
            e.2 += 1;
        }
    }
    let mut by_time: Vec<_> = totals.into_iter().collect();
    by_time.sort_by(|a, b| b.1.0.total_cmp(&a.1.0));
    let tick_ms = mean(&|t| f64::from(t.wall_ms)).max(1e-9);
    let spans = Value::Array(
        by_time
            .iter()
            .map(|(name, (ms, cpu, seen))| {
                json!({
                    "name": name,
                    "ms_per_tick": round(ms / n),
                    "cpu_ms_per_tick": round(cpu / n),
                    "share_of_tick": round(ms / n / tick_ms),
                    "ticks_seen": seen,
                })
            })
            .collect(),
    );
    let mut worst: Vec<&Tick> = r.ticks.iter().collect();
    worst.sort_by(|a, b| b.wall_ms.total_cmp(&a.wall_ms));
    let worst = Value::Array(
        worst
            .into_iter()
            .take(WORST)
            .map(|t| {
                json!({
                    "ms": round(f64::from(t.wall_ms)),
                    "cpu_ms": t.cpu_ms.map(|c| round(f64::from(c))),
                    "spans": t.spans.iter().take(8).map(|s| json!([s.name, round(f64::from(s.ms))])).collect::<Vec<_>>(),
                })
            })
            .collect(),
    );
    (ticks, spans, worst)
}

/// The sampled mob costs: per level and type, the milliseconds a tick, the mobs ticking, and the
/// microseconds one mob costs. Costliest first, the first [`MOB_TYPES`].
fn mob_costs(costs: &[tick_profile::MobCost], sampled_ticks: u64) -> Value {
    let ticks = sampled_ticks.max(1) as f64;
    Value::Array(
        costs
            .iter()
            .take(MOB_TYPES)
            .map(|c| {
                json!({
                    "dimension": c.dimension,
                    "type": c.kind,
                    "ms_per_tick": round(c.nanos as f64 / ticks / 1e6),
                    "mobs_ticked": round(c.mob_ticks as f64 / ticks),
                    "us_per_mob": round(c.nanos as f64 / c.mob_ticks.max(1) as f64 / 1e3),
                })
            })
            .collect(),
    )
}

/// The chunk workers' CPU time by job kind (`streaming::job_stats`): cores used, jobs, ms a job.
fn worker_jobs(jobs: &[(&'static str, u64, u64)], window_s: f64) -> Value {
    Value::Array(
        jobs.iter()
            .filter(|(_, _, count)| *count > 0)
            .map(|&(kind, nanos, count)| {
                json!({
                    "kind": kind,
                    "jobs": count,
                    "jobs_per_second": round(count as f64 / window_s.max(1e-9)),
                    "cpu_cores": round(nanos as f64 / 1e9 / window_s.max(1e-9)),
                    "ms_per_job": round(nanos as f64 / count as f64 / 1e6),
                })
            })
            .collect(),
    )
}

/// The players' network use over the window, from the wire bytes of the players on at both ends
/// (in KiB a second: average, P50, P95, max), and what all players on at the end sent since the
/// start (or since they joined).
fn network(start: &BTreeMap<u32, (u64, u64)>, end: &[(u32, (u64, u64))], window_s: f64) -> Value {
    let kib_s = |bytes: u64| bytes as f64 / 1024.0 / window_s.max(1e-9);
    let mut out = Vec::new();
    let mut inn = Vec::new();
    let (mut total_out, mut total_in) = (0u64, 0u64);
    for (conn, (now_in, now_out)) in end {
        let (was_in, was_out) = start.get(conn).copied().unwrap_or((0, 0));
        let (d_in, d_out) = (
            now_in.saturating_sub(was_in),
            now_out.saturating_sub(was_out),
        );
        total_in += d_in;
        total_out += d_out;
        if start.contains_key(conn) {
            out.push(kib_s(d_out) as f32);
            inn.push(kib_s(d_in) as f32);
        }
    }
    let stats = |v: &mut Vec<f32>| {
        v.sort_by(f32::total_cmp);
        let avg = v.iter().fold(0.0, |sum, &x| sum + f64::from(x)) / v.len().max(1) as f64;
        json!({
            "average": round(avg),
            "p50": round(pct(v, 0.5)),
            "p95": round(pct(v, 0.95)),
            "max": round(v.last().copied().map_or(0.0, f64::from)),
        })
    };
    json!({
        "players_whole_window": out.len(),
        "out_kib_per_s_per_player": stats(&mut out),
        "in_kib_per_s_per_player": stats(&mut inn),
        "total_out_kib_per_s": round(kib_s(total_out)),
        "total_in_kib_per_s": round(kib_s(total_in)),
    })
}

/// What recording cost the tick, priced on this machine at the end of the recording: every
/// sampled mob two clock reads, every span inside the tick two clocks and two thread-CPU clocks.
/// A dedicated server records spans whenever `HORIZON_SPIKES` is on (by default, for its slow-tick
/// line): then they are not the report's cost, and `ms_per_tick` leaves them out.
fn overhead(r: &Recording, mob_samples: u64, span_ns: f64, mob_ns: f64) -> Value {
    let n = r.ticks.len().max(1) as f64;
    let mspt = r.ticks.iter().map(|t| f64::from(t.wall_ms)).sum::<f64>() / n;
    let spans_per_tick = r.spans_recorded as f64 / n;
    let mobs_per_tick = mob_samples as f64 / n;
    let spans_ms = spans_per_tick * span_ns / 1e6;
    let mobs_ms = mobs_per_tick * mob_ns / 1e6;
    let ms = mobs_ms + if r.spans_were_on { 0.0 } else { spans_ms };
    let small = |v: f64| (v * 1e4).round() / 1e4;
    json!({
        "span_ns": round(span_ns),
        "spans_per_tick": round(spans_per_tick),
        "spans_ms_per_tick": small(spans_ms),
        "spans_on_without_report": r.spans_were_on,
        "mob_sample_ns": round(mob_ns),
        "mob_samples_per_tick": round(mobs_per_tick),
        "mob_samples_ms_per_tick": small(mobs_ms),
        "ms_per_tick": small(ms),
        "share_of_mspt": small(ms / mspt.max(1e-9)),
    })
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
    /// The level in place and the parked ones: their chunks and their entities by type.
    fn report_levels(&self) -> Value {
        let mut levels = Vec::new();
        let here = |dim: Dim,
                    world: &horizon_client::world::World,
                    mobs: &[horizon_client::mobs::Mob]| {
            let mut by_type: BTreeMap<&str, usize> = BTreeMap::new();
            for m in mobs.iter().filter(|m| !m.removed) {
                *by_type.entry(&m.kind.id).or_default() += 1;
            }
            let mut by_type: Vec<_> = by_type.into_iter().collect();
            by_type.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
            json!({
                "dimension": dim.id(),
                "chunks": world.chunks.len(),
                "block_entities": world.block_entities.len(),
                "mobs": mobs.iter().filter(|m| !m.removed).count(),
                "mobs_by_type": by_type.into_iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>(),
            })
        };
        let mut current = here(self.dim, &self.world, &self.mobs);
        current["item_entities"] = json!(self.dropped.len());
        current["experience_orbs"] = json!(self.orbs.len());
        current["projectiles"] = json!(self.creatures.projectiles.len());
        current["minecarts"] = json!(self.carts.len());
        current["boats"] = json!(self.boats.len());
        levels.push(current);
        for dim in self.levels.parked_dims() {
            if let (Some(world), Some(mobs)) =
                (self.levels.parked_world(dim), self.levels.parked_mobs(dim))
            {
                let mut level = here(dim, world, mobs);
                if let Some([items, orbs, projectiles, carts, boats]) =
                    self.levels.parked_entity_counts(dim)
                {
                    level["item_entities"] = json!(items);
                    level["experience_orbs"] = json!(orbs);
                    level["projectiles"] = json!(projectiles);
                    level["minecarts"] = json!(carts);
                    level["boats"] = json!(boats);
                }
                levels.push(level);
            }
        }
        Value::Array(levels)
    }

    /// Each player's wire bytes (in, out) by connection.
    pub(super) fn wire_bytes(&self) -> Vec<(u32, (u64, u64))> {
        self.lan.as_ref().map_or_else(Vec::new, |h| {
            h.hub
                .guests
                .iter()
                .filter_map(|g| Some((g.conn, g.sender.wire_bytes()?)))
                .collect()
        })
    }

    /// The report of a finished recording: written to `reports/` in the server's folder, its summary logged.
    pub(super) fn finish_report(&mut self, r: Recording, view: (u32, u32), max_players: usize) {
        let span_ns = span_cost_nanos();
        let mob_ns = tick_profile::sample_cost_nanos();
        // Off unless `HORIZON_SPIKES` or a mod (`native_spans`) still wants them.
        tick_profile::want_spans(tick_profile::SPANS_REPORT, false);
        job_stats::enable(false);
        let window_s = r.started.elapsed().as_secs_f64();
        let (ticks, spans, worst) = tick_summary(&r, window_s);
        let costs = tick_profile::take();
        let mob_samples = costs.iter().map(|c| c.mob_ticks).sum();
        let jobs = job_stats::take();
        let cpu = horizon_bench::cpu_seconds(std::process::id())
            .zip(r.cpu_s)
            .map(|(end, start)| round((end - start) / window_s.max(1e-9)));
        let players = self.lan.as_ref().map_or(0, |h| h.hub.guests.len());
        let report = json!({
            "format": 2,
            "horizon": horizon_client::crash::version(),
            "minecraft": horizon_core::VANILLA_VERSION,
            "window_s": round(window_s),
            "ticks": ticks,
            "spans": spans,
            "worst_ticks": worst,
            "mob_costs": mob_costs(&costs, r.sampled_ticks),
            "mob_sampled_ticks": r.sampled_ticks,
            "chunk_workers": worker_jobs(&jobs, window_s),
            "network": network(&r.wire_start, &self.wire_bytes(), window_s),
            "overhead": overhead(&r, mob_samples, span_ns, mob_ns),
            "players": players,
            "levels": self.report_levels(),
            "memory": { "rss_mb": horizon_bench::rss_bytes(std::process::id()).map(|b| b >> 20) },
            "process": process_limits(),
            "cpu_cores_used": cpu,
            "config": {
                "view_distance": view.0,
                "simulation_distance": view.1,
                "max_players": max_players,
            },
        });
        let text = summary(&report);
        // The machine's description runs commands: off the tick.
        std::thread::spawn(move || {
            let mut report = report;
            let hw = horizon_bench::Hardware::detect();
            report["machine"] = json!({
                "os": hw.os, "arch": hw.arch, "cpu": hw.cpu, "cpu_threads": hw.cpu_threads, "ram_mb": hw.ram_mb,
            });
            match write_report(Path::new("reports"), &report, &text) {
                Ok(path) => log::info!(
                    "Horizon: performance report written to {}\n{text}",
                    path.display()
                ),
                Err(e) => log::error!("Horizon: could not write the performance report: {e}"),
            }
        });
    }
}

/// A few lines for the console: the tick rate and times, the five costliest spans, the worst tick.
fn summary(r: &Value) -> String {
    let t = &r["ticks"];
    let m = &t["mspt"];
    let mut out = format!(
        "{} players, TPS {}, MSPT average {} ms, P50 {}, P95 {}, P99 {}, max {} ({} ticks over 50 ms, {} over 100 ms)\n",
        r["players"],
        t["tps"],
        m["average"],
        m["p50"],
        m["p95"],
        m["p99"],
        m["max"],
        t["over_50_ms"],
        t["over_100_ms"],
    );
    let span_list = |levels: bool, n: usize| -> Vec<String> {
        r["spans"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|s| s["name"].as_str().unwrap_or("").starts_with("level_") == levels)
            .take(n)
            .map(|s| {
                format!(
                    "{} {} ms",
                    s["name"].as_str().unwrap_or(""),
                    s["ms_per_tick"]
                )
            })
            .collect()
    };
    out += &format!("per tick: {}\n", span_list(false, 5).join(", "));
    let levels = span_list(true, 3);
    if !levels.is_empty() {
        out += &format!("levels: {}\n", levels.join(", "));
    }
    if let Some(w) = r["worst_ticks"].get(0) {
        out += &format!("worst tick {} ms: {}\n", w["ms"], w["spans"]);
    }
    let costs: Vec<String> = r["mob_costs"]
        .as_array()
        .into_iter()
        .flatten()
        .take(5)
        .map(|c| {
            format!(
                "{} {} ms ({} x {} us)",
                c["type"].as_str().unwrap_or(""),
                c["ms_per_tick"],
                c["mobs_ticked"],
                c["us_per_mob"]
            )
        })
        .collect();
    if !costs.is_empty() {
        out += &format!("mobs: {}\n", costs.join(", "));
    }
    let workers: Vec<String> = r["chunk_workers"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|w| {
            format!(
                "{} {}/s {} cores",
                w["kind"].as_str().unwrap_or(""),
                w["jobs_per_second"],
                w["cpu_cores"]
            )
        })
        .collect();
    if !workers.is_empty() {
        out += &format!("chunk workers: {}\n", workers.join(", "));
    }
    let net = &r["network"];
    if net.is_object() {
        out += &format!(
            "network: {} KiB/s out in all, {} KiB/s a player (P95 {}, max {})\n",
            net["total_out_kib_per_s"],
            net["out_kib_per_s_per_player"]["average"],
            net["out_kib_per_s_per_player"]["p95"],
            net["out_kib_per_s_per_player"]["max"]
        );
    }
    if let Some(o) = r["overhead"].as_object() {
        out += &format!(
            "recording cost {} ms a tick ({} of MSPT)\n",
            o["ms_per_tick"], o["share_of_mspt"]
        );
    }
    let mobs: u64 = r["levels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| l["mobs"].as_u64())
        .sum();
    let chunks: u64 = r["levels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| l["chunks"].as_u64())
        .sum();
    out += &format!(
        "{mobs} mobs, {chunks} chunks, {} MB resident, {} cores of CPU",
        r["memory"]["rss_mb"], r["cpu_cores_used"]
    );
    let process = &r["process"];
    if let Some(threads) = process["threads"].as_u64() {
        out += &format!("\n{threads} threads");
        for (key, name) in [
            ("cgroup_pids_current", "container tasks"),
            ("cgroup_pids_max", "container task limit"),
            ("max_processes", "max processes (soft / hard)"),
        ] {
            if let Some(value) = process[key].as_str() {
                out += &format!(", {name} {value}");
            }
        }
    }
    out
}

/// What the system lets this process start: its threads now, and the limits
/// a container puts on them and on memory. A server out of threads fails to
/// start the ones it needs (EAGAIN). Read on Linux; null elsewhere.
fn process_limits() -> Value {
    let read = |path: &str| std::fs::read_to_string(path).ok();
    let threads = read("/proc/self/status").and_then(|s| {
        s.lines()
            .find_map(|l| l.strip_prefix("Threads:"))
            .and_then(|v| v.trim().parse::<u64>().ok())
    });
    // "Max processes   <soft>   <hard>   processes"
    let max_processes = read("/proc/self/limits").and_then(|s| {
        let line = s.lines().find(|l| l.starts_with("Max processes"))?;
        let words: Vec<&str> = line.split_whitespace().skip(2).take(2).collect();
        Some(words.join(" / "))
    });
    let cgroup = |name: &str| read(&format!("/sys/fs/cgroup/{name}")).map(|s| s.trim().to_owned());
    json!({
        "threads": threads,
        "max_processes": max_processes,
        "cgroup_pids_current": cgroup("pids.current"),
        "cgroup_pids_max": cgroup("pids.max"),
        "cgroup_memory_current": cgroup("memory.current"),
        "cgroup_memory_max": cgroup("memory.max"),
    })
}

/// The longest `horizon-trace` writes for.
pub(super) const TRACE_MAX_SECONDS: u32 = 300;

/// `horizon-trace SECONDS`: every thread's spans for `seconds`, to `reports/trace-<time>.json`
/// (Perfetto opens it).
pub(super) fn start_trace(seconds: u32) -> std::io::Result<PathBuf> {
    let dir = Path::new("reports");
    std::fs::create_dir_all(dir)?;
    let stamp = chrono::Utc::now().format("%Y-%m-%d_%H-%M-%SZ");
    let path = dir.join(format!("trace-{stamp}.json"));
    spans::trace::start(&path, Some(seconds))?;
    Ok(path)
}

fn write_report(dir: &Path, report: &Value, text: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stamp = chrono::Utc::now().format("%Y-%m-%d_%H-%M-%SZ");
    let path = dir.join(format!("perf-{stamp}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(report)? + "\n")?;
    std::fs::write(path.with_extension("txt"), format!("{text}\n"))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(name: &'static str, ms: f32) -> Span {
        Span {
            name,
            ms,
            cpu_ms: ms,
            depth: 0,
        }
    }

    #[test]
    fn ticks_and_spans_are_summed_per_tick() {
        let mut r = Recording {
            seconds: 1,
            started: Instant::now(),
            cpu_s: None,
            spans_were_on: true,
            ticks: Vec::new(),
            sampled_ticks: 0,
            spans_recorded: 0,
            wire_start: BTreeMap::new(),
        };
        for i in 0..20 {
            let wall = if i == 7 { 120.0 } else { 10.0 };
            let sampled = r.sample_next();
            r.add(
                wall,
                Some(wall - 1.0),
                vec![span("tick_mobs", wall / 2.0), span("tick_lan", 2.0)],
                2,
                sampled,
            );
        }
        assert_eq!(r.sampled_ticks, 5, "one tick in four");
        assert_eq!(r.spans_recorded, 40);
        let (ticks, spans, worst) = tick_summary(&r, 1.0);
        assert_eq!(ticks["count"], 20);
        assert_eq!(ticks["tps"], 20.0);
        assert_eq!(ticks["over_100_ms"], 1);
        assert_eq!(ticks["mspt"]["max"], 120.0);
        assert_eq!(spans[0]["name"], "tick_mobs");
        assert_eq!(spans[0]["ms_per_tick"], 7.75);
        assert_eq!(spans[1]["ms_per_tick"], 2.0);
        assert_eq!(worst[0]["ms"], 120.0);
        assert_eq!(worst[0]["spans"][0], json!(["tick_mobs", 60.0]));
        let text = summary(
            &json!({"ticks": ticks, "spans": spans, "worst_ticks": worst, "players": 3,
            "levels": [{"mobs": 5, "chunks": 9}], "memory": {"rss_mb": 300}, "cpu_cores_used": 1.5}),
        );
        assert!(text.starts_with("3 players, TPS 20.0"), "{text}");
        assert!(
            text.contains("per tick: tick_mobs 7.75 ms, tick_lan 2.0 ms"),
            "{text}"
        );
        assert!(
            text.ends_with("5 mobs, 9 chunks, 300 MB resident, 1.5 cores of CPU"),
            "{text}"
        );
        let text = summary(
            &json!({"ticks": ticks, "spans": spans, "worst_ticks": worst,
            "players": 3, "levels": [], "memory": {"rss_mb": 300}, "cpu_cores_used": 1.5,
            "process": {"threads": 140, "cgroup_pids_current": "160", "cgroup_pids_max": "512",
                "max_processes": "unlimited / unlimited"}}),
        );
        assert!(
            text.ends_with(
                "140 threads, container tasks 160, container task limit 512, \
                 max processes (soft / hard) unlimited / unlimited"
            ),
            "{text}"
        );
    }

    /// A mob type's sampled time is spread over the sampled ticks; one mob's cost over its ticks.
    #[test]
    fn mob_costs_are_per_sampled_tick_and_per_mob() {
        let costs = [
            tick_profile::MobCost {
                dimension: "minecraft:overworld",
                kind: "minecraft:villager".into(),
                // 10 sampled ticks of 200 villagers at 25 us each.
                nanos: 10 * 200 * 25_000,
                mob_ticks: 10 * 200,
            },
            tick_profile::MobCost {
                dimension: "minecraft:the_end",
                kind: "minecraft:enderman".into(),
                nanos: 10 * 40 * 2_000,
                mob_ticks: 10 * 40,
            },
        ];
        let v = mob_costs(&costs, 10);
        assert_eq!(v[0]["type"], "minecraft:villager");
        assert_eq!(v[0]["ms_per_tick"], 5.0);
        assert_eq!(v[0]["mobs_ticked"], 200.0);
        assert_eq!(v[0]["us_per_mob"], 25.0);
        assert_eq!(v[1]["dimension"], "minecraft:the_end");
        assert_eq!(v[1]["ms_per_tick"], 0.08);
        assert_eq!(mob_costs(&[], 0), json!([]));
    }

    /// Per-player rates count only the players on for the whole window; the total also counts
    /// those who joined during it, from zero.
    #[test]
    fn network_rates_per_player_and_in_all() {
        let start = BTreeMap::from([(1, (0, 0)), (2, (5_000, 50_000)), (9, (1, 1))]);
        let end = [
            (1, (10 * 1024, 100 * 1024 * 10)),
            (2, (5_000 + 20 * 1024, 50_000 + 300 * 1024 * 10)),
            // Joined during the window.
            (3, (0, 600 * 1024 * 10)),
        ];
        let v = network(&start, &end, 10.0);
        assert_eq!(v["players_whole_window"], 2);
        assert_eq!(v["out_kib_per_s_per_player"]["average"], 200.0);
        assert_eq!(v["out_kib_per_s_per_player"]["max"], 300.0);
        assert_eq!(v["in_kib_per_s_per_player"]["average"], 1.5);
        assert_eq!(v["total_out_kib_per_s"], 1000.0);
    }

    /// Worker kinds that ran no job are left out; CPU time over the window is cores.
    #[test]
    fn worker_jobs_give_cores_and_job_times() {
        let jobs = [
            ("load", 0, 0),
            ("generate", 30_000_000_000, 6_000),
            ("light", 2_000_000_000, 4_000),
        ];
        let v = worker_jobs(&jobs, 60.0);
        assert_eq!(v.as_array().unwrap().len(), 2);
        assert_eq!(v[0]["kind"], "generate");
        assert_eq!(v[0]["cpu_cores"], 0.5);
        assert_eq!(v[0]["jobs_per_second"], 100.0);
        assert_eq!(v[0]["ms_per_job"], 5.0);
        assert_eq!(v[1]["ms_per_job"], 0.5);
    }

    /// The recording's cost: spans and mob samples a tick at their measured price, against MSPT.
    #[test]
    fn overhead_is_measured_against_the_tick() {
        let mut r = Recording {
            seconds: 1,
            started: Instant::now(),
            cpu_s: None,
            spans_were_on: true,
            ticks: Vec::new(),
            sampled_ticks: 0,
            spans_recorded: 0,
            wire_start: BTreeMap::new(),
        };
        for _ in 0..4 {
            r.add(50.0, None, Vec::new(), 100, false);
        }
        // Spans were off before: 100 spans at 200 ns + 1,000 mob samples a tick at 50 ns = 0.07 ms
        // of 50.
        r.spans_were_on = false;
        let v = overhead(&r, 4_000, 200.0, 50.0);
        assert_eq!(v["spans_ms_per_tick"], 0.02);
        assert_eq!(v["ms_per_tick"], 0.07);
        assert_eq!(v["share_of_mspt"], 0.0014);
        // Spans on anyway (a dedicated server's slow-tick line): only the mob samples are the report's.
        r.spans_were_on = true;
        let v = overhead(&r, 4_000, 200.0, 50.0);
        assert_eq!(v["ms_per_tick"], 0.05);
        assert_eq!(v["share_of_mspt"], 0.001);
        // On this machine, both prices are well under a microsecond's order.
        let (span_ns, mob_ns) = (span_cost_nanos(), tick_profile::sample_cost_nanos());
        assert!(span_ns > 0.0 && span_ns < 50_000.0, "{span_ns}");
        assert!(mob_ns > 0.0 && mob_ns < 50_000.0, "{mob_ns}");
    }

    /// The summary lists the levels apart from the systems, and the new sections when present.
    #[test]
    fn summary_lists_levels_mobs_workers_network_and_cost() {
        let r = json!({
            "ticks": {"tps": 20.0, "mspt": {"average": 40.0, "p50": 39.0, "p95": 45.0, "p99": 50.0, "max": 60.0},
                "over_50_ms": 1, "over_100_ms": 0},
            "spans": [{"name": "level_overworld", "ms_per_tick": 30.0}, {"name": "tick_mobs", "ms_per_tick": 20.0},
                {"name": "level_the_end", "ms_per_tick": 9.0}],
            "worst_ticks": [],
            "mob_costs": [{"type": "minecraft:villager", "ms_per_tick": 5.0, "mobs_ticked": 200.0, "us_per_mob": 25.0}],
            "chunk_workers": [{"kind": "generate", "jobs_per_second": 100.0, "cpu_cores": 0.5}],
            "network": {"total_out_kib_per_s": 1000.0, "out_kib_per_s_per_player": {"average": 200.0, "p95": 300.0, "max": 300.0}},
            "overhead": {"ms_per_tick": 0.07, "share_of_mspt": 0.0014},
            "players": 2, "levels": [], "memory": {"rss_mb": 1}, "cpu_cores_used": 1.0,
        });
        let text = summary(&r);
        assert!(text.contains("per tick: tick_mobs 20.0 ms\n"), "{text}");
        assert!(
            text.contains("levels: level_overworld 30.0 ms, level_the_end 9.0 ms\n"),
            "{text}"
        );
        assert!(
            text.contains("mobs: minecraft:villager 5.0 ms (200.0 x 25.0 us)\n"),
            "{text}"
        );
        assert!(
            text.contains("chunk workers: generate 100.0/s 0.5 cores\n"),
            "{text}"
        );
        assert!(
            text.contains(
                "network: 1000.0 KiB/s out in all, 200.0 KiB/s a player (P95 300.0, max 300.0)"
            ),
            "{text}"
        );
        assert!(
            text.contains("recording cost 0.07 ms a tick (0.0014 of MSPT)"),
            "{text}"
        );
    }
}
