//! `horizon-ctl profile`: the words the game receives, as typed requests, and the typed replies.
use serde::{Deserialize, Serialize};
use std::ops::RangeInclusive;
use std::str::FromStr;

/// The longest name a mark takes; it is the one free text in a capture.
pub const MARK_NAME_MAX: usize = 100;
/// The sampler's rate bounds, per second.
pub const HZ_RANGE: RangeInclusive<u32> = 10..=1000;

/// Which side `profile` addresses; no side means the sides the game can reach.
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Start,
    Stop,
    Status,
    Mark,
}

impl FromStr for Action {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "start" => Ok(Self::Start),
            "stop" => Ok(Self::Stop),
            "status" => Ok(Self::Status),
            "mark" => Ok(Self::Mark),
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
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Start(StartOptions),
    Stop,
    Status,
    Mark(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    pub side: Option<ProfileSide>,
    pub request: Request,
}

const USAGE: &str = "profile [client|server] start [--seconds S] [--hz N] [--over MS] [--threads main|all] [--wall] [--no-samples] | stop | status | mark NAME";

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
    let mut options = StartOptions::default();
    let mut switched = false;
    let mut words = rest.iter().copied();
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
            "--no-samples" => options.samples = false,
            other => return Err(format!("unexpected {other}; usage: {USAGE}")),
        }
        switched |= word != "--side";
    }
    let request = match action {
        Action::Start => Request::Start(options),
        _ if switched => return Err(format!("only start takes options; usage: {USAGE}")),
        Action::Stop => Request::Stop,
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
    pub side: ProfileSide,
    pub path: String,
    pub seconds: Option<u32>,
    pub samples: bool,
}

/// The reply to `profile stop`, once the file is whole.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stopped {
    pub ok: bool,
    pub side: ProfileSide,
    pub path: String,
    pub open_at: String,
    pub removed: Vec<String>,
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

/// The reply to `profile mark`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Marked {
    pub ok: bool,
    pub name: String,
    pub t: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(line: &str) -> Result<Parsed, String> {
        parse(&line.split_whitespace().collect::<Vec<_>>())
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
        let Request::Start(o) =
            p("start --hz 500 --over 12.5 --threads all --wall --no-samples --seconds 9")
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
            "start --gpu",
            "stop now",
            "stop --wall",
            "status --hz 100",
            "start --side everyone",
        ] {
            assert!(p(line).is_err(), "{line:?} was accepted");
        }
        assert!(p("start --gpu").unwrap_err().contains("usage: profile"));
    }
}
