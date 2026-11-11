use crate::gamerules::GameRules as Rules;
use horizon_perf_format::Gauges;
use horizon_perf_format::v1::{Build, GameRules, HorizonOptions, Machine, Settings};

use std::sync::OnceLock;

use super::WorldCounts;

/// The disk the game runs from, where its worlds are saved: used and total MiB.
pub(super) fn disk_mb() -> Option<(u64, u64)> {
    const MIB: u64 = 1 << 20;
    let dir = std::env::current_dir().ok()?;
    let total = fs4::total_space(&dir).ok()?;
    let free = fs4::available_space(&dir).ok()?;
    Some((total.saturating_sub(free) / MIB, total / MIB))
}

/// Reads the process and the machine for the health gauges; the CPU figures are rates since the last read.
#[derive(Default)]
pub(super) struct SystemProbe {
    last: Option<(f64, f64, Option<(u64, u64)>)>,
}

impl SystemProbe {
    pub(super) fn read(&mut self, t: f64, counts: WorldCounts) -> Gauges {
        let pid = std::process::id();
        let cpu_s = horizon_bench::cpu_seconds(pid);
        let stat = std::fs::read_to_string("/proc/stat")
            .ok()
            .and_then(|s| parse_cpu_stat(&s));
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get()) as f64;
        let mut g = Gauges {
            rss_mb: horizon_bench::rss_bytes(pid).map_or(0.0, |b| (b >> 20) as f32),
            players: counts.players,
            chunks: counts.chunks,
            entities: counts.entities,
            ..Gauges::default()
        };
        if let Some((used, swap)) = std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|s| parse_meminfo(&s))
        {
            g.physical_used_mb = used as f32;
            g.swap_used_mb = swap as f32;
        }
        if let Some((last_t, last_cpu, last_stat)) = self.last
            && t > last_t
        {
            if let Some(cpu) = cpu_s {
                g.cpu_process_cores = ((cpu - last_cpu) / (t - last_t)) as f32;
            }
            if let (Some((busy, total)), Some((last_busy, last_total))) = (stat, last_stat)
                && total > last_total
            {
                g.cpu_system_cores =
                    ((busy - last_busy) as f64 / (total - last_total) as f64 * threads) as f32;
            }
        }
        self.last = Some((t, cpu_s.unwrap_or(0.0), stat));
        g
    }
}

/// A `SystemProbe` on a thread of its own, for the sides whose frame must not wait on the probes (macOS runs `ps`).
pub(super) struct BackgroundProbe {
    requests: std::sync::mpsc::Sender<(f64, WorldCounts)>,
    latest: std::sync::Arc<std::sync::Mutex<Gauges>>,
}

impl BackgroundProbe {
    pub(super) fn spawn() -> Self {
        let (requests, wanted) = std::sync::mpsc::channel::<(f64, WorldCounts)>();
        let latest = std::sync::Arc::new(std::sync::Mutex::new(Gauges::default()));
        let shared = std::sync::Arc::clone(&latest);
        let worker = std::thread::Builder::new()
            .name("horizon-perf-gauges".into())
            .spawn(move || {
                super::profiler_thread();
                let mut probe = SystemProbe::default();
                for (t, counts) in wanted {
                    let gauges = probe.read(t, counts);
                    *shared.lock().unwrap_or_else(|e| e.into_inner()) = gauges;
                }
            });
        if let Err(e) = worker {
            log::warn!("Horizon: no thread for the health gauges ({e})");
        }
        Self { requests, latest }
    }

    /// Asks for a reading at `t`; the answer shows in [`BackgroundProbe::latest`] a moment later.
    pub(super) fn request(&self, t: f64, counts: WorldCounts) {
        let _ = self.requests.send((t, counts));
    }

    pub(super) fn latest(&self) -> Gauges {
        *self.latest.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// `/proc/stat`'s first line as (busy, total) jiffies: everything but idle and iowait is busy.
pub(super) fn parse_cpu_stat(text: &str) -> Option<(u64, u64)> {
    let fields: Vec<u64> = text
        .lines()
        .next()?
        .strip_prefix("cpu ")?
        .split_whitespace()
        .filter_map(|f| f.parse().ok())
        .collect();
    if fields.len() < 5 {
        return None;
    }
    let total: u64 = fields.iter().take(8).sum();
    Some((total - fields[3] - fields[4], total))
}

/// `/proc/meminfo` as (physical MB in use, swap MB in use); in use is total minus available.
pub(super) fn parse_meminfo(text: &str) -> Option<(u64, u64)> {
    let kb = |key: &str| -> Option<u64> {
        text.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    };
    let used = kb("MemTotal")?.checked_sub(kb("MemAvailable")?)?;
    let swap = kb("SwapTotal")?.saturating_sub(kb("SwapFree")?);
    Some((used >> 10, swap >> 10))
}

/// The cgroup v2 `cpu.max` as cores: "200000 100000" is 2; "max 100000" is no limit.
pub(super) fn parse_cgroup_cpu(text: &str) -> Option<f32> {
    let mut words = text.split_whitespace();
    let quota: f64 = words.next()?.parse().ok()?;
    let period: f64 = words.next()?.parse().ok()?;
    (period > 0.0).then(|| (quota / period) as f32)
}

/// The cgroup v2 `memory.max` in MB; "max" is no limit.
pub(super) fn parse_cgroup_memory(text: &str) -> Option<u64> {
    text.trim().parse::<u64>().ok().map(|b| b >> 20)
}

static HARDWARE: OnceLock<horizon_bench::Hardware> = OnceLock::new();

/// Describes the hardware on a thread of its own: the probes run commands, which the tick must not wait for.
pub fn warm_hardware() {
    std::thread::spawn(|| {
        HARDWARE.get_or_init(horizon_bench::Hardware::detect);
    });
}

/// The hardware, described now if nothing has yet: the probes run commands, so not on a tick.
pub fn hardware_now() -> &'static horizon_bench::Hardware {
    HARDWARE.get_or_init(horizon_bench::Hardware::detect)
}

/// The hardware, once `warm_hardware` has described it.
pub fn hardware() -> Option<&'static horizon_bench::Hardware> {
    HARDWARE.get()
}

/// What the machine and its container give the process, for `Start.machine`.
pub fn machine(hardware: Option<&horizon_bench::Hardware>) -> Machine {
    let cgroup = |name: &str| std::fs::read_to_string(format!("/sys/fs/cgroup/{name}")).ok();
    let mut m = Machine {
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        cpu_threads: std::thread::available_parallelism().map_or(0, |n| n.get() as u32),
        cgroup_cpu_cores: cgroup("cpu.max").and_then(|t| parse_cgroup_cpu(&t)),
        cgroup_memory_mb: cgroup("memory.max").and_then(|t| parse_cgroup_memory(&t)),
        ..Machine::default()
    };
    if let Some(h) = hardware {
        m.os_version = h.os_version.clone().unwrap_or_default();
        m.cpu = h.cpu.clone().unwrap_or_default();
        m.cpu_cores = h.cpu_cores.map_or(0, u32::from);
        m.ram_mb = h.ram_mb.unwrap_or(0);
    }
    m
}

/// The build this server runs, for `Start.build`.
pub fn build() -> Build {
    Build {
        version: crate::crash::version(),
        profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
        .to_owned(),
        target: format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
        ..Build::default()
    }
}

/// The server settings a capture records: the typed allowlist, nothing else from `server.properties`.
pub fn settings(view: u32, simulation: u32, max_players: u32, compression: i32) -> Settings {
    Settings {
        view_distance: Some(view),
        simulation_distance: Some(simulation),
        max_players: Some(max_players),
        network_compression_threshold: Some(compression),
    }
}

/// The game rules a capture records: the typed allowlist, nothing else from the world's rule set.
pub fn game_rules(rules: &Rules) -> GameRules {
    GameRules {
        random_tick_speed: Some(rules.int("random_tick_speed")),
        max_entity_cramming: Some(rules.int("max_entity_cramming")),
        fire_spread_radius_around_player: Some(rules.int("fire_spread_radius_around_player")),
        advance_time: Some(rules.bool("advance_time")),
        mob_griefing: Some(rules.bool("mob_griefing")),
        spawn_mobs: Some(rules.bool("spawn_mobs")),
        spawn_monsters: Some(rules.bool("spawn_monsters")),
        spawner_blocks_work: Some(rules.bool("spawner_blocks_work")),
    }
}

/// The Horizon switches a capture records: the typed allowlist, never the environment or `server.toml`.
pub fn horizon_options(
    max_view_distance: u32,
    lod_distance: u32,
    anticheat: bool,
) -> HorizonOptions {
    HorizonOptions {
        max_view_distance: Some(max_view_distance),
        lod_distance: Some(lod_distance),
        anticheat: Some(anticheat),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_stat_busy_excludes_idle_and_iowait() {
        // user nice system idle iowait irq softirq steal
        let text = "cpu  100 20 30 700 50 5 5 90 0 0\ncpu0 1 1 1 1 1 1 1 1 0 0\n";
        assert_eq!(parse_cpu_stat(text), Some((250, 1000)));
        assert_eq!(parse_cpu_stat("intr 1 2 3"), None);
    }

    #[test]
    fn meminfo_used_is_total_minus_available() {
        let text = "MemTotal:       16384000 kB\nMemFree: 1 kB\nMemAvailable:   8192000 kB\nSwapTotal:      4096000 kB\nSwapFree:       3072000 kB\n";
        assert_eq!(parse_meminfo(text), Some((8000, 1000)));
    }

    #[test]
    fn cgroup_limits() {
        assert_eq!(parse_cgroup_cpu("250000 100000\n"), Some(2.5));
        assert_eq!(parse_cgroup_cpu("max 100000\n"), None);
        assert_eq!(parse_cgroup_memory("2147483648\n"), Some(2048));
        assert_eq!(parse_cgroup_memory("max\n"), None);
    }
}
