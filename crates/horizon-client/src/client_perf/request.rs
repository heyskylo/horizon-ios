//! `horizon-ctl profile`: the words the game receives, as typed requests, and the typed replies.
use horizon_perf_format::v1::GpuMode;
use serde::{Deserialize, Serialize};
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::str::FromStr;

/// The longest name a mark takes; it is the one free text in a capture.
pub const MARK_NAME_MAX: usize = 100;
/// The sampler's rate bounds, per second.
pub const HZ_RANGE: RangeInclusive<u32> = 10..=1000;

/// Which side `profile` addresses; no side means every side the game can reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProfileSide {
    Client,
    Server,
}

impl FromStr for ProfileSide {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "client" => Ok(Self::Client),
            "server" => Ok(Self::Server),
            other => Err(format!("unknown side {other}; use client or server")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Threads {
    Main,
    All,
}

impl FromStr for Threads {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "main" => Ok(Self::Main),
            "all" => Ok(Self::All),
            other => Err(format!("unknown threads {other}; use main or all")),
        }
    }
}

/// What `--gpu` records: nothing, the passes' times, or also their parts and counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuSwitch {
    #[default]
    Off,
    Passes,
    Detail,
}

impl FromStr for GpuSwitch {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "passes" => Ok(Self::Passes),
            "detail" => Ok(Self::Detail),
            other => Err(format!(
                "unknown --gpu value {other}; use --gpu, --gpu=passes or --gpu=detail"
            )),
        }
    }
}

impl GpuSwitch {
    pub fn mode(self) -> GpuMode {
        match self {
            Self::Off => GpuMode::Off,
            Self::Passes => GpuMode::Passes,
            Self::Detail => GpuMode::Detail,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Start,
    Stop,
    Status,
    Mark,
    Open,
    Share,
}

impl FromStr for Action {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "start" => Ok(Self::Start),
            "stop" => Ok(Self::Stop),
            "status" => Ok(Self::Status),
            "mark" => Ok(Self::Mark),
            "open" => Ok(Self::Open),
            "share" => Ok(Self::Share),
            other => Err(format!("unknown profile action {other}")),
        }
    }
}

/// The switches of `profile start`.
#[derive(Clone, Debug, PartialEq)]
pub struct StartOptions {
    /// Unset runs until `stop`, or the longest window.
    pub seconds: Option<u32>,
    pub hz: Option<u32>,
    pub over_ms: Option<f32>,
    pub threads: Threads,
    pub wall: bool,
    pub samples: bool,
    pub gpu: GpuSwitch,
    /// Also record every thread's spans (`--timeline`), for 60 s at most.
    pub timeline: bool,
    /// Write the capture here instead of into the reports folder: `trace` and `HORIZON_TRACE` only, never a profile switch.
    pub file: Option<PathBuf>,
}

impl Default for StartOptions {
    fn default() -> Self {
        Self {
            seconds: None,
            hz: None,
            over_ms: None,
            threads: Threads::Main,
            wall: false,
            samples: true,
            gpu: GpuSwitch::Off,
            timeline: false,
            file: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Start(StartOptions),
    /// With the path of a benchmark report to embed in the capture.
    Stop {
        bench: Option<String>,
    },
    Status,
    Mark(String),
    /// Open the saved capture in the viewer, through the captures hook.
    Open(PathBuf),
    /// Upload the saved capture, through the captures hook; the reply is the link.
    Share(PathBuf),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    pub side: Option<ProfileSide>,
    pub request: Request,
}

const USAGE: &str = "profile [client|server] start [--seconds S] [--hz N] [--over MS] [--threads main|all] [--wall] [--gpu[=detail]] [--no-samples] [--timeline] | stop [--bench REPORT] | status | mark NAME | open FILE | share FILE";

/// The words after `profile`; the side comes first or as `--side`, and the switches follow `start`.
pub fn parse(words: &[&str]) -> Result<Parsed, String> {
    let (mut side, words) = match words.first().map(|w| w.parse::<ProfileSide>()) {
        Some(Ok(side)) => (Some(side), &words[1..]),
        _ => (None, words),
    };
    let Some((action, rest)) = words.split_first() else {
        return Err(format!("usage: {USAGE}"));
    };
    let action: Action = action.parse().map_err(|e| format!("{e}; usage: {USAGE}"))?;
    if action == Action::Mark {
        return mark(side, rest);
    }
    if matches!(action, Action::Open | Action::Share) {
        let file = rest.join(" ");
        if file.is_empty() {
            return Err(format!("{} needs a FILE; usage: {USAGE}", words[0]));
        }
        let file = PathBuf::from(file);
        let request = match action {
            Action::Open => Request::Open(file),
            _ => Request::Share(file),
        };
        return Ok(Parsed {
            side: None,
            request,
        });
    }
    let mut options = StartOptions::default();
    let mut switched = false;
    let mut bench = None;
    let mut words = rest.iter().copied().peekable();
    while let Some(word) = words.next() {
        let mut value = || {
            words
                .next()
                .ok_or_else(|| format!("{word} needs a value; usage: {USAGE}"))
        };
        match word {
            "--side" => side = Some(value()?.parse()?),
            "--seconds" => options.seconds = Some(number(word, value()?, 1..=600)?),
            "--hz" => options.hz = Some(number(word, value()?, HZ_RANGE)?),
            "--over" => {
                let ms: f32 = value()?
                    .parse()
                    .map_err(|_| "--over must be a number of milliseconds".to_owned())?;
                if !ms.is_finite() || ms <= 0.0 {
                    return Err("--over must be more than 0".into());
                }
                options.over_ms = Some(ms);
            }
            "--threads" => options.threads = value()?.parse()?,
            "--wall" => options.wall = true,
            "--gpu" => {
                options.gpu = match words.next_if(|w| w.parse::<GpuSwitch>().is_ok()) {
                    Some(kind) => kind.parse()?,
                    None => GpuSwitch::Passes,
                }
            }
            _ if word.starts_with("--gpu=") => options.gpu = word["--gpu=".len()..].parse()?,
            "--no-samples" => options.samples = false,
            "--timeline" => options.timeline = true,
            "--bench" => {
                // The line arrives split on whitespace, so a run of spaces or a tab in the path becomes one space.
                let path = words.by_ref().collect::<Vec<_>>().join(" ");
                if path.is_empty() {
                    return Err(format!("--bench needs a report path; usage: {USAGE}"));
                }
                bench = Some(path);
                continue;
            }
            other => return Err(format!("unexpected {other}; usage: {USAGE}")),
        }
        switched |= word != "--side";
    }
    let request = match action {
        Action::Start if bench.is_some() => {
            return Err(format!("only stop takes --bench; usage: {USAGE}"));
        }
        Action::Start => Request::Start(options),
        _ if switched => return Err(format!("only start takes options; usage: {USAGE}")),
        Action::Stop => Request::Stop { bench },
        _ if bench.is_some() => return Err(format!("only stop takes --bench; usage: {USAGE}")),
        _ => Request::Status,
    };
    Ok(Parsed { side, request })
}

fn mark(mut side: Option<ProfileSide>, mut words: &[&str]) -> Result<Parsed, String> {
    if let [flag @ "--side", value, rest @ ..] = words {
        let _ = flag;
        side = Some(value.parse()?);
        words = rest;
    }
    let name = words.join(" ");
    if name.is_empty() {
        return Err("mark needs a NAME".into());
    }
    if name.chars().count() > MARK_NAME_MAX || name.chars().any(char::is_control) {
        return Err(format!(
            "a mark name is at most {MARK_NAME_MAX} characters without control characters"
        ));
    }
    Ok(Parsed {
        side,
        request: Request::Mark(name),
    })
}

fn number(flag: &str, text: &str, range: RangeInclusive<u32>) -> Result<u32, String> {
    match text.parse::<u32>() {
        Ok(n) if range.contains(&n) => Ok(n),
        _ => Err(format!(
            "{flag} must be a whole number from {} to {}",
            range.start(),
            range.end()
        )),
    }
}

/// The reply to `profile start`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Started {
    pub ok: bool,
    /// The sides asked for, server first; the server thread's join is confirmed by `status` and `stop`.
    pub sides: Vec<ProfileSide>,
    pub path: String,
    pub seconds: Option<u32>,
    pub samples: bool,
    pub gpu: GpuSwitch,
}

/// The reply to `profile stop`, once the file is whole.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stopped {
    pub ok: bool,
    pub sides: Vec<ProfileSide>,
    pub path: String,
    /// The viewer page, when the build has a part that opens captures.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_at: Option<String>,
    pub removed: Vec<String>,
}

/// The viewer page a registered captures hook offers.
pub fn viewer() -> Option<String> {
    horizon_hooks::engine::captures().map(|c| c.viewer().to_owned())
}

/// The reply to `profile status`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub ok: bool,
    pub capturing: bool,
    pub sides: Vec<ProfileSide>,
    pub path: Option<String>,
    pub elapsed_s: Option<f64>,
    pub bytes: Option<u64>,
}

/// The reply of a deferred `profile stop` that failed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Failed {
    pub error: String,
}

/// The reply to `trace`: whether a timeline capture runs, and where it writes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Traced {
    pub ok: bool,
    pub tracing: bool,
    pub file: Option<String>,
}

/// The reply to `profile open`: the viewer the capture was opened in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Opened {
    pub ok: bool,
    pub viewer: String,
}

/// The reply to `profile share`: the capture's share link.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shared {
    pub ok: bool,
    pub url: String,
}

/// The reply to `profile mark`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Marked {
    pub ok: bool,
    pub name: String,
    /// Seconds on the game clock, which counts from when the game began, for every side.
    pub t: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(line: &str) -> Result<Parsed, String> {
        parse(&line.split_whitespace().collect::<Vec<_>>())
    }

    #[test]
    fn open_and_share_take_a_file_with_spaces_kept() {
        assert_eq!(
            p("open reports/a.hzperf").unwrap().request,
            Request::Open(PathBuf::from("reports/a.hzperf"))
        );
        assert_eq!(
            p("share /tmp/my  capture.hzperf").unwrap().request,
            Request::Share(PathBuf::from("/tmp/my capture.hzperf"))
        );
        assert!(p("open").is_err());
        assert!(p("share").is_err());
    }

    #[test]
    fn the_side_comes_first_or_as_a_flag() {
        let a = p("client start --seconds 30").unwrap();
        let b = p("start --side client --seconds 30").unwrap();
        assert_eq!(a, b);
        assert_eq!(a.side, Some(ProfileSide::Client));
        assert_eq!(p("start").unwrap().side, None);
        assert_eq!(p("server stop").unwrap().side, Some(ProfileSide::Server));
        assert_eq!(
            p("stop --side client").unwrap().side,
            Some(ProfileSide::Client)
        );
    }

    #[test]
    fn every_start_switch_lands_in_its_field() {
        let Request::Start(o) = p(
            "start --hz 500 --over 12.5 --threads all --wall --gpu=detail --no-samples --timeline --seconds 9",
        )
        .unwrap()
        .request
        else {
            panic!("not a start");
        };
        assert_eq!(
            o,
            StartOptions {
                seconds: Some(9),
                hz: Some(500),
                over_ms: Some(12.5),
                threads: Threads::All,
                wall: true,
                samples: false,
                gpu: GpuSwitch::Detail,
                timeline: true,
                file: None,
            }
        );
    }

    #[test]
    fn a_mark_takes_the_rest_of_the_line_as_its_name() {
        assert_eq!(
            p("mark   loaded the nether").unwrap().request,
            Request::Mark("loaded the nether".into())
        );
        assert_eq!(
            p("client mark --wall").unwrap(),
            Parsed {
                side: Some(ProfileSide::Client),
                request: Request::Mark("--wall".into())
            }
        );
        assert_eq!(
            p("mark --side client the nether").unwrap(),
            Parsed {
                side: Some(ProfileSide::Client),
                request: Request::Mark("the nether".into())
            }
        );
        assert!(p("mark").is_err());
        assert!(p("mark --side client").is_err());
        assert!(p(&format!("mark {}", "x".repeat(MARK_NAME_MAX + 1))).is_err());
        assert!(p(&format!("mark {}", "x".repeat(MARK_NAME_MAX))).is_ok());
    }

    #[test]
    fn stop_takes_the_rest_of_the_line_as_a_report_path() {
        assert_eq!(p("stop").unwrap().request, Request::Stop { bench: None });
        let with = p("client stop --bench /home/a b/games/benchmarks/game-1.json").unwrap();
        assert_eq!(with.side, Some(ProfileSide::Client));
        assert_eq!(
            with.request,
            Request::Stop {
                bench: Some("/home/a b/games/benchmarks/game-1.json".into())
            }
        );
        for line in [
            "stop --bench",
            "start --bench x.json",
            "status --bench x.json",
        ] {
            assert!(p(line).is_err(), "{line:?} was accepted");
        }
    }

    #[test]
    fn bad_words_are_refused_with_the_usage() {
        for line in [
            "",
            "client",
            "begin",
            "start --seconds 0",
            "start --seconds 601",
            "start --seconds",
            "start --hz 5",
            "start --over 0",
            "start --over nan",
            "start --threads some",
            "start --gpu=all",
            "start --gpu=",
            "stop now",
            "stop --wall",
            "status --hz 100",
            "start --side everyone",
        ] {
            assert!(p(line).is_err(), "{line:?} was accepted");
        }
        assert!(p("start --gpu=all").unwrap_err().contains("--gpu=detail"));
        assert!(
            p("start --gpu fast")
                .unwrap_err()
                .contains("usage: profile")
        );
    }

    #[test]
    fn gpu_alone_records_the_passes_and_detail_asks_for_more() {
        let gpu = |line: &str| match p(line).unwrap().request {
            Request::Start(o) => o.gpu,
            other => panic!("{other:?}"),
        };
        assert_eq!(gpu("start"), GpuSwitch::Off);
        assert_eq!(gpu("start --gpu"), GpuSwitch::Passes);
        assert_eq!(
            gpu("client start --seconds 5 --gpu=detail"),
            GpuSwitch::Detail
        );
        assert_eq!(gpu("start --gpu detail --wall"), GpuSwitch::Detail);
        assert_eq!(gpu("start --gpu --wall"), GpuSwitch::Passes);
        assert!(p("stop --gpu").is_err(), "only start takes options");
    }
}
