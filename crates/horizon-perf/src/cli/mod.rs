mod attach;
mod diff;
mod export;
pub mod files;
mod query;
#[cfg(not(target_arch = "wasm32"))]
mod sharing;

use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use horizon_perf_format::v1::Side;
use serde::Serialize;

use crate::follow::Follower;
use crate::mcp;
use crate::ops;
use crate::request::{self, RequestError};
use crate::summary::{Baseline, DEFAULT_BUDGET, SummaryOptions};
use crate::watch::Watcher;
use crate::wire::{GroupBy, HotBy, Op, SampleBy, SideName, vocab};

/// The usage text, with the ops and groupings listed from their enums.
pub fn usage() -> String {
    let by = GroupBy::ALL
        .iter()
        .map(|g| g.as_str())
        .collect::<Vec<_>>()
        .join("|");
    let by_list = |all: &[SampleBy]| all.iter().map(|b| b.as_str()).collect::<Vec<_>>().join("|");
    let hot = HotBy::ALL
        .iter()
        .map(|h| h.as_str())
        .collect::<Vec<_>>()
        .join("|");
    let split = by_list(SampleBy::ALL);
    let group_by: Vec<SampleBy> = SampleBy::ALL
        .iter()
        .copied()
        .filter(|b| b.groups())
        .collect();
    let group = by_list(&group_by);
    format!(
        "\
usage:
  horizon-perf summary [FILE|latest] [--baseline FILE] [--budget TOKENS] [--side client|server] [--thread T] [--slow|--all] [--dir DIR]
  horizon-perf query   OP [ARGS] [FILE...] [FILTERS]   JSON always
  horizon-perf diff    A B [--min-ms X] [--calls] [--side client|server] [--thread T] [--limit N] [--json] [--dir DIR]
  horizon-perf list    [DIR]
  horizon-perf follow  [FILE|latest] [--every S] [--json] [--dir DIR]
  horizon-perf attach  FILE --name NAME --body JSONFILE   adds the JSON file to a finished capture as an attachment
  horizon-perf export  [FILE|latest] --pprof|--folded|--perfetto [--out PATH] [--side client|server] [--thread T] [--select KIND] [--slow] [--dir DIR]
  horizon-perf upload  [FILE|latest] [--dir DIR]         temporary one-hour upload; prints its link and delete token as JSON
  horizon-perf share   [FILE|latest] [--dir DIR]         upload and share for 30 days; prints its link and delete token as JSON
  horizon-perf open    [FILE|latest] [--dir DIR]         serves the file once on 127.0.0.1 and opens the viewer on it; waits for the fetch
  horizon-perf mcp                                     MCP server on stdio: perf_summary, perf_query, perf_diff, perf_list, perf_follow

OPs: {ops}.
FILTERS: --side client|server --thread T --from T --to T (seconds; negative counts from the end; mark:NAME) --limit N --dir DIR
  spans: --slow --names a,b   ticks: --worst N --over MS   series: FIELD... --every S (h:NAME, span:NAME, x:NAME)
  memory: --diff [LABEL_A LABEL_B] --min-mb X   world: --by {by} [LABEL]
  samples (--select cpu|running|waiting|wall|alloc, --slow, --where k=v):
    hot [--by {hot}] [--match TEXT]   tree [ID] [--depth N] [--min-pct X]   callers ID [--depth N]   callees ID [--depth N]
    split [ID] --by {split}   group --by {group}   frames NAME... [FILE...] (an f: id, a function name or /regex/)

export writes stdout unless --out is given. pprof carries every kind of sample; folded counts --select (cpu by default); perfetto is the timeline.
diff compares B against A (A is the earlier run): two captures, or two benchmark reports (benchmarks/*.json). --calls adds the sampled functions (f:) and call paths (p:). summary --baseline FILE compares against an earlier capture.
FILE defaults to latest: the newest finished .hzperf in --dir, else in $HORIZON_HOME/reports, else in ./reports.
follow's latest is the newest .hzperf, finished or not.",
        ops = Op::answered()
    )
}

vocab! {
    /// A `--flag` of any command.
    Flag {
        Side = "side",
        Thread = "thread",
        Select = "select",
        Where = "where",
        From = "from",
        To = "to",
        Limit = "limit",
        Dir = "dir",
        Names = "names",
        Worst = "worst",
        Over = "over",
        Every = "every",
        MinMb = "min-mb",
        MinMs = "min-ms",
        By = "by",
        Match = "match",
        Depth = "depth",
        MinPct = "min-pct",
        Budget = "budget",
        Baseline = "baseline",
        Name = "name",
        Body = "body",
        Slow = "slow",
        All = "all",
        Diff = "diff",
        Json = "json",
        Calls = "calls",
        Out = "out",
        Pprof = "pprof",
        Folded = "folded",
        Perfetto = "perfetto",
    }
}

impl Flag {
    /// A switch stands alone; every other flag takes the value after it.
    fn is_switch(self) -> bool {
        match self {
            Flag::Slow
            | Flag::All
            | Flag::Diff
            | Flag::Json
            | Flag::Calls
            | Flag::Pprof
            | Flag::Folded
            | Flag::Perfetto => true,
            Flag::Side
            | Flag::Thread
            | Flag::Select
            | Flag::Where
            | Flag::From
            | Flag::To
            | Flag::Limit
            | Flag::Dir
            | Flag::Names
            | Flag::Worst
            | Flag::Over
            | Flag::Every
            | Flag::MinMb
            | Flag::MinMs
            | Flag::By
            | Flag::Match
            | Flag::Depth
            | Flag::MinPct
            | Flag::Budget
            | Flag::Baseline
            | Flag::Name
            | Flag::Body
            | Flag::Out => false,
        }
    }
}

/// One reply as the line it is printed as.
fn json_line<T: Serialize>(reply: &T) -> Result<String, CliError> {
    ops::json_text(reply)
        .map(|text| text + "\n")
        .map_err(failed)
}

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
        #[cfg(not(target_arch = "wasm32"))]
        "upload" | "share" => sharing::run(rest, env, out, command == "share"),
        #[cfg(not(target_arch = "wasm32"))]
        "open" => sharing::open(rest, env, out),
        "list" => list(rest, env, out),
        "follow" => follow(rest, env, out),
        "query" => query::query(rest, env, out),
        "diff" => diff::run(rest, env, out),
        "attach" => attach::run(rest, env, out),
        "export" => export::run(rest, env, out),
        "mcp" => {
            if !rest.is_empty() {
                return Err(CliError::Usage("mcp takes no arguments".into()));
            }
            mcp::serve(&mut std::io::stdin().lock(), out, env).map_err(failed)
        }
        "help" | "--help" | "-h" => writeln!(out, "{}", usage()).map_err(failed),
        other => Err(CliError::Usage(format!("unknown command {other}"))),
    }
}

struct Parsed {
    positional: Vec<String>,
    flags: HashMap<Flag, String>,
}

/// Splits `args` into positionals and the flags in `allowed`; a repeated `--where` joins its values.
fn parse(args: &[String], allowed: &[Flag]) -> Result<Parsed, CliError> {
    let mut parsed = Parsed {
        positional: Vec::new(),
        flags: HashMap::new(),
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let Some(text) = arg.strip_prefix("--") else {
            parsed.positional.push(arg.clone());
            continue;
        };
        let (name, inline) = match text.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (text, None),
        };
        let Some(flag) = Flag::from_str(name).ok().filter(|f| allowed.contains(f)) else {
            return Err(CliError::Usage(format!("unknown option --{name}")));
        };
        if flag.is_switch() {
            if inline.is_some() {
                return Err(CliError::Usage(format!("unknown option --{name}")));
            }
            parsed.flags.insert(flag, String::new());
            continue;
        }
        let value = match inline {
            Some(v) => v,
            None => args
                .next()
                .cloned()
                .ok_or_else(|| CliError::Usage(format!("--{flag} needs a value")))?,
        };
        match parsed.flags.get(&flag) {
            Some(earlier) if flag == Flag::Where => {
                let joined = format!("{earlier},{value}");
                parsed.flags.insert(flag, joined);
            }
            _ => {
                parsed.flags.insert(flag, value);
            }
        }
    }
    Ok(parsed)
}

impl Parsed {
    fn number<T: std::str::FromStr>(&self, flag: Flag) -> Result<Option<T>, CliError> {
        self.flags
            .get(&flag)
            .map(|v| {
                v.parse()
                    .map_err(|_| CliError::Usage(format!("--{flag} needs a number, got {v}")))
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

pub(crate) fn resolve(
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

fn side_flag(parsed: &Parsed) -> Result<Option<Side>, CliError> {
    let Some(text) = parsed.flags.get(&Flag::Side) else {
        return Ok(None);
    };
    let name = SideName::from_str(text)
        .map_err(|_| CliError::Usage(format!("--side is client or server, got {text}")))?;
    request::side(Some(name)).map_err(|e| CliError::Usage(format!("--{e}, got {text}")))
}

impl From<RequestError> for CliError {
    fn from(e: RequestError) -> Self {
        CliError::Usage(e.0)
    }
}

fn summary(args: &[String], env: &Env, out: &mut dyn Write) -> Result<(), CliError> {
    let parsed = parse(
        args,
        &[
            Flag::Budget,
            Flag::Side,
            Flag::Thread,
            Flag::Dir,
            Flag::Baseline,
            Flag::Slow,
            Flag::All,
        ],
    )?;
    if parsed.flags.contains_key(&Flag::Slow) && parsed.flags.contains_key(&Flag::All) {
        return Err(CliError::Usage(
            "--slow and --all exclude each other".into(),
        ));
    }
    let options = SummaryOptions {
        budget_tokens: parsed.number(Flag::Budget)?.unwrap_or(DEFAULT_BUDGET),
        side: side_flag(&parsed)?,
        thread: parsed.flags.get(&Flag::Thread).cloned(),
        slow: parsed.flags.contains_key(&Flag::Slow),
        baseline: None,
    };
    let dir = parsed.flags.get(&Flag::Dir).map(String::as_str);
    let baseline = parsed
        .flags
        .get(&Flag::Baseline)
        .map(|file| -> Result<Baseline, CliError> {
            let path = resolve(Some(file), dir, env, true)?;
            ops::baseline(&path).map_err(failed)
        })
        .transpose()?;
    let options = SummaryOptions {
        baseline,
        ..options
    };
    let path = resolve(parsed.one_file()?, dir, env, true)?;
    let text = ops::summary(&path, &options).map_err(failed)?;
    out.write_all(text.as_bytes()).map_err(failed)
}

fn list(args: &[String], env: &Env, out: &mut dyn Write) -> Result<(), CliError> {
    let parsed = parse(args, &[])?;
    let dir = reports_dir(parsed.one_file()?, env);
    for listing in ops::list(&dir).map_err(failed)? {
        writeln!(out, "{}", listing.line).map_err(failed)?;
    }
    Ok(())
}

fn follow(args: &[String], env: &Env, out: &mut dyn Write) -> Result<(), CliError> {
    let parsed = parse(args, &[Flag::Every, Flag::Dir, Flag::Json])?;
    let every: f64 = parsed.number(Flag::Every)?.unwrap_or(1.0);
    let path = resolve(
        parsed.one_file()?,
        parsed.flags.get(&Flag::Dir).map(String::as_str),
        env,
        false,
    )?;
    let mut file =
        File::open(&path).map_err(|e| failed(format!("cannot open {}: {e}", path.display())))?;
    let mut follower = Follower::new();
    let mut watcher = Watcher::new(every, parsed.flags.contains_key(&Flag::Json));
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
