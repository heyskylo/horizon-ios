//! The benchmark report format, version 1 (docs/benchmark-report.md).
//!
//! One JSON file per run, the same for the game, the launcher and `tools/perf_bench.py`: the local log
//! a player can attach, and, with consent, what the telemetry upload sends ([`Report::for_upload`]).
//! It holds summaries and fixed-edge histogram counts, never raw frame streams.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

mod hardware;

pub use hardware::{power_source, Display, Gpu, Hardware};
/// The process probes, from the platform (`horizon-system`).
pub use horizon_telemetry::system::{cpu_seconds, load_average, rss_bytes};

pub const FORMAT: &str = "horizon-benchmark-report";
pub const VERSION: u32 = 1;

/// Lower edges of the frame-time histogram buckets, in milliseconds; the last bucket is 1000 ms and
/// over. Fixed by the format version so histograms from any number of reports add up.
pub const HISTOGRAM_EDGES_MS: [f64; 24] = [
    0.0, 1.0, 2.0, 3.0, 4.17, 5.0, 6.0, 7.0, 8.33, 10.0, 12.0, 14.0, 16.67, 20.0, 25.0, 33.33,
    40.0, 50.0, 66.67, 100.0, 150.0, 250.0, 500.0, 1000.0,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Game,
    Launcher,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct App {
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mods: Vec<Mod>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mod {
    pub name: String,
    pub version: String,
}

/// What has to match for two reports to be compared. Every field is optional.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Conditions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_distance: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simulation_distance: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vsync: Option<bool>,
    /// Absent: unlimited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fps_cap: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offscreen: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeats: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warmup_s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_avg_1m: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
}

/// A measurement over a run's repeats.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub n: usize,
    pub mean: f64,
    /// Sample standard deviation; `None` for a single value.
    pub stdev: Option<f64>,
    pub min: f64,
    pub max: f64,
    /// Half-width of the 95% confidence interval of the mean (Student's t); `None` for a single value.
    pub ci95: Option<f64>,
}

impl Summary {
    /// `None` for no values.
    pub fn of(values: &[f64]) -> Option<Summary> {
        let n = values.len();
        if n == 0 {
            return None;
        }
        let mean = values.iter().sum::<f64>() / n as f64;
        let (stdev, ci95) = if n > 1 {
            let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
            let sd = var.sqrt();
            (Some(sd), Some(t95(n - 1) * sd / (n as f64).sqrt()))
        } else {
            (None, None)
        };
        Some(Summary {
            n,
            mean: round(mean),
            stdev: stdev.map(round),
            min: round(values.iter().copied().fold(f64::INFINITY, f64::min)),
            max: round(values.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
            ci95: ci95.map(round),
        })
    }
}

/// Two-sided 95% critical value of Student's t for `df` degrees of freedom.
fn t95(df: usize) -> f64 {
    const TABLE: [f64; 30] = [
        12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228, 2.201, 2.179, 2.160,
        2.145, 2.131, 2.120, 2.110, 2.101, 2.093, 2.086, 2.080, 2.074, 2.069, 2.064, 2.060, 2.056,
        2.052, 2.048, 2.045, 2.042,
    ];
    TABLE.get(df.wrapping_sub(1)).copied().unwrap_or(1.96)
}

/// One measured phase of one repeat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frames {
    pub phase: String,
    pub repeat: u32,
    pub frames: usize,
    pub seconds: f64,
    pub avg_fps: f64,
    pub low_1pct_fps: f64,
    pub low_01pct_fps: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub p999_ms: f64,
    pub max_ms: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_p50_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_p50_ms: Option<f64>,
    /// Frame counts per bucket of [`HISTOGRAM_EDGES_MS`].
    pub histogram: Vec<u64>,
}

impl Frames {
    /// The figures of a phase from its frame times in milliseconds (time between the starts of
    /// consecutive frames); `None` for no frames. CPU and GPU times are per frame too, when measured.
    pub fn of(
        phase: &str,
        repeat: u32,
        frame_ms: &[f32],
        cpu_ms: &[f32],
        gpu_ms: &[f32],
    ) -> Option<Frames> {
        let times = sorted(frame_ms);
        let n = times.len();
        if n == 0 {
            return None;
        }
        let total: f64 = times.iter().sum();
        // The average frame time of the slowest `share` of frames, as frames per second.
        let low = |share: f64| {
            let slowest = &times[n - ((n as f64 * share).ceil() as usize).max(1)..];
            1000.0 * slowest.len() as f64 / slowest.iter().sum::<f64>()
        };
        let mut histogram = vec![0; HISTOGRAM_EDGES_MS.len()];
        for &t in &times {
            histogram[HISTOGRAM_EDGES_MS.partition_point(|&edge| edge <= t).max(1) - 1] += 1;
        }
        Some(Frames {
            phase: phase.to_owned(),
            repeat,
            frames: n,
            seconds: round(total / 1000.0),
            avg_fps: round(n as f64 * 1000.0 / total),
            low_1pct_fps: round(low(0.01)),
            low_01pct_fps: round(low(0.001)),
            p50_ms: round(percentile(&times, 0.5)),
            p95_ms: round(percentile(&times, 0.95)),
            p99_ms: round(percentile(&times, 0.99)),
            p999_ms: round(percentile(&times, 0.999)),
            max_ms: round(times[n - 1]),
            cpu_p50_ms: (!cpu_ms.is_empty()).then(|| round(percentile(&sorted(cpu_ms), 0.5))),
            gpu_p50_ms: (!gpu_ms.is_empty()).then(|| round(percentile(&sorted(gpu_ms), 0.5))),
            histogram,
        })
    }
}

fn sorted(ms: &[f32]) -> Vec<f64> {
    let mut v: Vec<f64> = ms
        .iter()
        .map(|&m| f64::from(m))
        .filter(|m| m.is_finite())
        .collect();
    v.sort_by(f64::total_cmp);
    v
}

/// Three decimals: plenty for milliseconds and FPS, short in JSON, and parsed back exactly.
fn round(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Nearest-rank percentile of sorted, non-empty values.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    let n = sorted.len();
    sorted[((n as f64 * p).ceil() as usize).clamp(1, n) - 1]
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub format: String,
    pub version: u32,
    pub kind: Kind,
    /// When the run ended, UTC, `YYYY-MM-DDTHH:MM:SSZ`.
    pub created: String,
    pub app: App,
    pub hardware: Hardware,
    #[serde(default)]
    pub conditions: Conditions,
    /// Scalar measurements by name; the unit is the name's suffix (`_ms`, `_mb`...).
    #[serde(default)]
    pub metrics: BTreeMap<String, Summary>,
    #[serde(default)]
    pub frames: Vec<Frames>,
    /// Local only: a name the person running it chose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Local only: logs the run wrote next to the report.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub local: Vec<PathBuf>,
}

impl Report {
    /// An empty report for this machine, created now.
    pub fn new(kind: Kind, app: App) -> Report {
        Self::with_hardware(kind, app, Hardware::detect())
    }

    /// Starts a report using supplied hardware, without running system probes.
    /// Interactive apps can collect the remaining machine details after their first frame.
    pub fn with_hardware(kind: Kind, app: App, hardware: Hardware) -> Report {
        Report {
            format: FORMAT.to_owned(),
            version: VERSION,
            kind,
            created: utc_now(),
            app,
            hardware,
            conditions: Conditions::default(),
            metrics: BTreeMap::new(),
            frames: Vec::new(),
            label: None,
            local: Vec::new(),
        }
    }

    /// Records a measurement over the repeats; no values records nothing.
    pub fn metric(&mut self, name: &str, values: &[f64]) {
        if let Some(summary) = Summary::of(values) {
            self.metrics.insert(name.to_owned(), summary);
        }
    }

    /// Reads a report, rejecting other formats and newer versions.
    pub fn parse(json: &str) -> Result<Report, String> {
        let report: Report = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if report.format != FORMAT {
            return Err(format!(
                "not a benchmark report: format {:?}",
                report.format
            ));
        }
        if report.version > VERSION {
            return Err(format!(
                "report version {} is newer than {VERSION}",
                report.version
            ));
        }
        Ok(report)
    }

    /// The report without its local-only fields, dated to the day: what an upload sends.
    pub fn for_upload(&self) -> Report {
        let day = self.created.get(..10).unwrap_or_default();
        Report {
            label: None,
            local: Vec::new(),
            created: format!("{day}T00:00:00Z"),
            ..self.clone()
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a report always serializes")
    }

    /// Writes `<dir>/<kind>-<created>-<scenario>.json` and returns its path.
    pub fn write_to(&self, dir: &Path) -> io::Result<PathBuf> {
        std::fs::create_dir_all(dir)?;
        let kind = match self.kind {
            Kind::Game => "game",
            Kind::Launcher => "launcher",
        };
        let stamp: String = self.created.chars().filter(char::is_ascii_digit).collect();
        let scenario: String = self
            .conditions
            .scenario
            .as_deref()
            .unwrap_or("run")
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let path = dir.join(format!("{kind}-{stamp}-{scenario}.json"));
        std::fs::write(&path, self.to_json() + "\n")?;
        Ok(path)
    }
}

/// The current time, UTC, `YYYY-MM-DDTHH:MM:SSZ`.
pub fn utc_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    utc(secs)
}

fn utc(secs: u64) -> String {
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_dates() {
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(utc(1_791_983_007), "2026-10-14T13:03:27Z");
    }

    #[test]
    fn summary_of_repeats() {
        assert_eq!(Summary::of(&[]), None);
        let one = Summary::of(&[5.0]).unwrap();
        assert_eq!((one.n, one.mean, one.stdev, one.ci95), (1, 5.0, None, None));
        let s = Summary::of(&[2.0, 4.0, 6.0]).unwrap();
        assert_eq!((s.mean, s.min, s.max), (4.0, 2.0, 6.0));
        assert_eq!(s.stdev, Some(2.0));
        assert_eq!(s.ci95, Some(4.969), "4.303 * 2 / sqrt(3)");
    }

    #[test]
    fn frame_figures_and_histogram() {
        // 998 frames of 2 ms, one of 20 ms, one of 1500 ms.
        let mut ms = vec![2.0f32; 998];
        ms.extend([20.0, 1500.0]);
        let f = Frames::of("flight", 0, &ms, &[], &[]).unwrap();
        assert_eq!(f.frames, 1000);
        assert_eq!(f.seconds, 3.516);
        assert_eq!((f.p50_ms, f.p999_ms, f.max_ms), (2.0, 20.0, 1500.0));
        // Slowest 1% = 10 frames: 8 of 2 ms, 20 ms, 1500 ms.
        assert_eq!(f.low_1pct_fps, 6.51, "1000 * 10 / 1536");
        // Slowest 0.1% = the 1500 ms frame.
        assert_eq!(f.low_01pct_fps, 0.667);
        assert_eq!(f.histogram.len(), HISTOGRAM_EDGES_MS.len());
        assert_eq!(f.histogram[2], 998, "2 ms falls in [2, 3)");
        assert_eq!(f.histogram[13], 1, "20 ms falls in [20, 25)");
        assert_eq!(f.histogram[23], 1, "1500 ms falls in [1000, inf)");
        assert_eq!(f.histogram.iter().sum::<u64>(), 1000);
        assert_eq!((f.cpu_p50_ms, f.gpu_p50_ms), (None, None));
        assert_eq!(Frames::of("x", 0, &[], &[], &[]), None);
    }

    #[test]
    fn round_trip_and_upload_strips_local_fields() {
        let mut r = Report::new(
            Kind::Game,
            App {
                name: "horizon-client".into(),
                version: "0.1.0".into(),
                ..App::default()
            },
        );
        r.conditions.scenario = Some("flight".into());
        r.metric("startup.title_ms", &[800.0, 820.0]);
        r.frames
            .extend(Frames::of("village", 0, &[2.0, 3.0], &[1.0], &[]));
        r.label = Some("my run".into());
        r.local.push("/home/someone/logs/latest.log".into());
        let back = Report::parse(&r.to_json()).unwrap();
        assert_eq!(back, r);
        r.created = "2026-10-15T14:03:27Z".into();
        let upload = r.for_upload();
        assert_eq!(upload.created, "2026-10-15T00:00:00Z", "the day only");
        let upload = upload.to_json();
        assert!(
            !upload.contains("my run") && !upload.contains("someone"),
            "{upload}"
        );
    }

    #[test]
    fn rejects_other_formats_and_newer_versions() {
        let r = Report::new(Kind::Launcher, App::default());
        let newer = r.to_json().replace("\"version\": 1", "\"version\": 2");
        assert!(Report::parse(&newer).unwrap_err().contains("newer"));
        let other = r.to_json().replace(FORMAT, "something-else");
        assert!(Report::parse(&other).is_err());
    }

    #[test]
    fn writes_into_a_folder() {
        let dir = std::env::temp_dir().join(format!("horizon-bench-test-{}", std::process::id()));
        let mut r = Report::new(Kind::Launcher, App::default());
        r.created = "2026-10-15T14:03:27Z".into();
        r.conditions.scenario = Some("cold start".into());
        let path = r.write_to(&dir).unwrap();
        assert_eq!(
            path.file_name().unwrap(),
            "launcher-20261015140327-cold_start.json"
        );
        assert_eq!(
            Report::parse(&std::fs::read_to_string(&path).unwrap()).unwrap(),
            r
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
