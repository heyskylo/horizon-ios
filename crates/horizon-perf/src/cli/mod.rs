pub mod files;
mod query;

use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use horizon_perf_format::v1::Side;

use crate::follow::Follower;
use crate::read::read_capture;
use crate::summary::{DEFAULT_BUDGET, SummaryOptions, summarize};
use crate::watch::Watcher;

pub const USAGE: &str = "\
usage:
  horizon-perf summary [FILE|latest] [--budget TOKENS] [--side client|server] [--thread T] [--slow|--all] [--dir DIR]
  horizon-perf query   OP [ARGS] [FILE...] [FILTERS]   JSON always
  horizon-perf list    [DIR]
  horizon-perf follow  [FILE|latest] [--every S] [--json] [--dir DIR]

OPs: spans, ticks, tick, series, world, net, meta, memory, marks, warnings.
FILTERS: --side client|server --thread T --from T --to T (seconds; negative counts from the end; mark:NAME) --limit N --dir DIR
  spans: --slow --names a,b   ticks: --worst N --over MS   series: FIELD... --every S (h:NAME, span:NAME, x:NAME)
  memory: --diff [LABEL_A LABEL_B] --min-mb X   world: --by dimension|entity|block_entity|cost [LABEL]

FILE defaults to latest: the newest finished .hzperf in --dir, else in $HORIZON_HOME/reports, else in ./reports.
follow's latest is the newest .hzperf, finished or not.";

const FOLLOW_CHUNK: usize = 64 * 1024;

/// What the process around the command supplies.
pub struct Env {
    pub horizon_home: Option<PathBuf>,
    /// How long `follow` waits when the file has nothing new.
    pub poll: Duration,
}

impl Env {
    pub fn from_process() -> Self {
        Self {
            horizon_home: std::env::var_os("HORIZON_HOME")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            poll: Duration::from_millis(200),
        }
    }
}

#[derive(Debug)]
pub enum CliError {
    Usage(String),
    Failed(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Usage(m) | CliError::Failed(m) => f.write_str(m),
        }
    }
}

fn failed(e: impl fmt::Display) -> CliError {
    CliError::Failed(e.to_string())
}

pub fn run(args: &[String], env: &Env, out: &mut dyn Write) -> Result<(), CliError> {
    let Some((command, rest)) = args.split_first() else {
        return Err(CliError::Usage("no command given".into()));
    };
    match command.as_str() {
        "summary" => summary(rest, env, out),
        "list" => list(rest, env, out),
        "follow" => follow(rest, env, out),
        "query" => query::query(rest, env, out),
        "help" | "--help" | "-h" => writeln!(out, "{USAGE}").map_err(failed),
        other => Err(CliError::Usage(format!("unknown command {other}"))),
    }
}

struct Parsed {
    positional: Vec<String>,
    flags: HashMap<String, String>,
}

fn parse(args: &[String], valued: &[&str], switches: &[&str]) -> Result<Parsed, CliError> {
    let mut parsed = Parsed {
        positional: Vec::new(),
        flags: HashMap::new(),
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let Some(flag) = arg.strip_prefix("--") else {
            parsed.positional.push(arg.clone());
            continue;
        };
        let (name, inline) = match flag.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (flag, None),
        };
        if name == "where"
            && valued.contains(&"where")
            && let Some(earlier) = parsed.flags.get("where").cloned()
        {
            let value = inline
                .clone()
                .or_else(|| args.next().cloned())
                .unwrap_or_default();
            parsed
                .flags
                .insert("where".into(), format!("{earlier},{value}"));
        } else if switches.contains(&name) && inline.is_none() {
            parsed.flags.insert(name.into(), String::new());
        } else if valued.contains(&name) {
            let value = match inline {
                Some(v) => v,
                None => args
                    .next()
                    .cloned()
                    .ok_or_else(|| CliError::Usage(format!("--{name} needs a value")))?,
            };
            parsed.flags.insert(name.into(), value);
        } else {
            return Err(CliError::Usage(format!("unknown option --{name}")));
        }
    }
    Ok(parsed)
}

impl Parsed {
    fn number<T: std::str::FromStr>(&self, name: &str) -> Result<Option<T>, CliError> {
        self.flags
            .get(name)
            .map(|v| {
                v.parse()
                    .map_err(|_| CliError::Usage(format!("--{name} needs a number, got {v}")))
            })
            .transpose()
    }

    fn one_file(&self) -> Result<Option<&str>, CliError> {
        match self.positional.as_slice() {
            [] => Ok(None),
            [file] => Ok(Some(file)),
            _ => Err(CliError::Usage("give one FILE".into())),
        }
    }
}

/// `--dir`, else `$HORIZON_HOME/reports` when that exists, else `./reports`.
pub fn reports_dir(dir: Option<&str>, env: &Env) -> PathBuf {
    if let Some(dir) = dir {
        return PathBuf::from(dir);
    }
    if let Some(home) = &env.horizon_home {
        let reports = home.join("reports");
        if reports.is_dir() {
            return reports;
        }
    }
    PathBuf::from("reports")
}

fn resolve(
    file: Option<&str>,
    dir: Option<&str>,
    env: &Env,
    finished_only: bool,
) -> Result<PathBuf, CliError> {
    match file {
        None | Some("latest") => {
            files::latest(&reports_dir(dir, env), finished_only).map_err(CliError::Failed)
        }
        Some(path) => Ok(PathBuf::from(path)),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

fn side_flag(parsed: &Parsed) -> Result<Option<Side>, CliError> {
    match parsed.flags.get("side").map(String::as_str) {
        None => Ok(None),
        Some("server") => Ok(Some(Side::Server)),
        Some("client") => Ok(Some(Side::Client)),
        Some(other) => Err(CliError::Usage(format!(
            "--side is client or server, got {other}"
        ))),
    }
}

fn summary(args: &[String], env: &Env, out: &mut dyn Write) -> Result<(), CliError> {
    let parsed = parse(args, &["budget", "side", "thread", "dir"], &["slow", "all"])?;
    if parsed.flags.contains_key("slow") && parsed.flags.contains_key("all") {
        return Err(CliError::Usage(
            "--slow and --all exclude each other".into(),
        ));
    }
    let options = SummaryOptions {
        budget_tokens: parsed.number("budget")?.unwrap_or(DEFAULT_BUDGET),
        side: side_flag(&parsed)?,
        thread: parsed.flags.get("thread").cloned(),
        slow: parsed.flags.contains_key("slow"),
    };
    let path = resolve(
        parsed.one_file()?,
        parsed.flags.get("dir").map(String::as_str),
        env,
        true,
    )?;
    let bytes =
        std::fs::read(&path).map_err(|e| failed(format!("cannot read {}: {e}", path.display())))?;
    let capture = read_capture(&bytes).map_err(failed)?;
    let text = summarize(&capture, &file_name(&path), &options).map_err(failed)?;
    out.write_all(text.as_bytes()).map_err(failed)
}

fn list(args: &[String], env: &Env, out: &mut dyn Write) -> Result<(), CliError> {
    let parsed = parse(args, &[], &[])?;
    let dir = reports_dir(parsed.one_file()?, env);
    let paths = files::captures_in(&dir)
        .map_err(|e| failed(format!("cannot read {}: {e}", dir.display())))?;
    for path in paths {
        let line = match files::peek(&path) {
            Ok(peek) => files::list_line(&path, &peek),
            Err(e) => format!("{}  unreadable: {e}", file_name(&path)),
        };
        writeln!(out, "{line}").map_err(failed)?;
    }
    Ok(())
}

fn follow(args: &[String], env: &Env, out: &mut dyn Write) -> Result<(), CliError> {
    let parsed = parse(args, &["every", "dir"], &["json"])?;
    let every: f64 = parsed.number("every")?.unwrap_or(1.0);
    let path = resolve(
        parsed.one_file()?,
        parsed.flags.get("dir").map(String::as_str),
        env,
        false,
    )?;
    let mut file =
        File::open(&path).map_err(|e| failed(format!("cannot open {}: {e}", path.display())))?;
    let mut follower = Follower::new();
    let mut watcher = Watcher::new(every, parsed.flags.contains_key("json"));
    let mut chunk = vec![0u8; FOLLOW_CHUNK];
    loop {
        let n = file.read(&mut chunk).map_err(failed)?;
        if n == 0 {
            std::thread::sleep(env.poll);
            continue;
        }
        for entry in follower.push(&chunk[..n]).map_err(failed)? {
            for line in watcher.feed(&entry) {
                writeln!(out, "{line}").map_err(failed)?;
            }
        }
        out.flush().map_err(failed)?;
        if follower.is_finished() || follower.stream_complete() {
            return Ok(());
        }
    }
}
