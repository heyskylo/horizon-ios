//! The five tools: each parses its arguments into a typed struct and calls the function its CLI command calls.

use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::cli::files::EXTENSION;
use crate::cli::{Env, resolve};
use crate::diff::{DEFAULT_LIMIT, DiffOptions};
use crate::ops::{self, DiffReply};
use crate::request::{self, QueryRequest};
use crate::summary::{DEFAULT_BUDGET, SummaryOptions};
use crate::watch::Event;
use crate::wire::{KindName, Op, SideName, vocab};

vocab! {
    /// The tools this server lists.
    ToolName {
        Summary = "perf_summary",
        Query = "perf_query",
        Diff = "perf_diff",
        List = "perf_list",
        Follow = "perf_follow",
    }
}

impl ToolName {
    pub fn description(self) -> &'static str {
        match self {
            ToolName::Summary => {
                "The agent report of a capture: health, where the tick goes, hot functions, worst ticks and the exact drill-down queries. Start here."
            }
            ToolName::Query => {
                "One query over captures, answered as JSON rows with stable ids that other queries accept. The same reply as `horizon-perf query`."
            }
            ToolName::Diff => {
                "Compare capture B against A (or two benchmark reports) with block statistics; rows carry a verdict, plus the text table."
            }
            ToolName::List => {
                "The captures in a folder, newest first, with their state, sides, kind, length and size."
            }
            ToolName::Follow => {
                "Health samples, slow events, marks and warnings written to a capture since the cursor of the last call; the next cursor, and done once every side has stopped."
            }
        }
    }
}

/// The result of a tool call: its text, and whether it is an error the agent should read.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub text: String,
    pub is_error: bool,
}

impl Outcome {
    fn ok(text: String) -> Self {
        Self {
            text,
            is_error: false,
        }
    }

    fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SummaryArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<SideName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub slow: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryArgs {
    pub op: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    #[serde(default)]
    pub filters: QueryRequest,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiffArgs {
    pub a: String,
    pub b: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<SideName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FollowArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<u64>,
}

/// What `perf_diff` returns: the text table and the JSON reply `diff --json` prints.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiffToolReply {
    pub text: String,
    pub reply: DiffReply,
}

vocab! {
    /// How far a listed capture got.
    ListState {
        Finished = "finished",
        Writing = "writing",
        Unreadable = "unreadable",
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListSide {
    pub side: SideName,
    pub kind: KindName,
    /// Set once the side has stopped.
    pub seconds: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListRow {
    pub file: String,
    pub path: String,
    pub state: ListState,
    pub error: Option<String>,
    pub size: Option<u64>,
    pub modified_unix_s: Option<u64>,
    pub sides: Vec<ListSide>,
    /// The line `horizon-perf list` prints for it.
    pub line: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListReply {
    pub captures: Vec<ListRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FollowReply {
    pub events: Vec<Event>,
    pub cursor: u64,
    pub done: bool,
}

/// What a tool may open.
#[derive(Clone, Copy, PartialEq)]
enum Opens {
    Capture,
    CaptureOrReport,
}

const REPORT_EXTENSION: &str = "json";

/// The path, if it is a file the tools open: `.hzperf`, or for a diff a benchmark report `.json`.
fn accept(path: PathBuf, opens: Opens) -> Result<PathBuf, String> {
    let extension = path.extension().and_then(|e| e.to_str());
    let allowed = extension == Some(EXTENSION)
        || (opens == Opens::CaptureOrReport && extension == Some(REPORT_EXTENSION));
    if allowed {
        return Ok(path);
    }
    let what = match opens {
        Opens::Capture => format!(".{EXTENSION} captures"),
        Opens::CaptureOrReport => {
            format!(".{EXTENSION} captures and benchmark .{REPORT_EXTENSION} reports")
        }
    };
    Err(format!(
        "refusing {}: these tools open only {what}",
        path.display()
    ))
}

fn open(
    file: Option<&str>,
    dir: Option<&str>,
    env: &Env,
    finished_only: bool,
    opens: Opens,
) -> Result<PathBuf, String> {
    let path = resolve(file, dir, env, finished_only).map_err(|e| e.to_string())?;
    accept(path, opens)
}

fn json<T: Serialize>(reply: &T) -> Result<Outcome, String> {
    ops::json_text(reply).map(Outcome::ok)
}

pub fn summary(args: SummaryArgs, env: &Env) -> Result<Outcome, String> {
    let path = open(args.file.as_deref(), None, env, true, Opens::Capture)?;
    let baseline = match args.baseline.as_deref() {
        None => None,
        Some(file) => {
            let path = open(Some(file), None, env, true, Opens::Capture)?;
            Some(ops::baseline(&path)?)
        }
    };
    let options = SummaryOptions {
        budget_tokens: args.budget.unwrap_or(DEFAULT_BUDGET),
        side: request::side(args.side).map_err(|e| e.to_string())?,
        thread: args.thread,
        slow: args.slow,
        baseline,
    };
    ops::summary(&path, &options).map(Outcome::ok)
}

pub fn query(args: QueryArgs, env: &Env) -> Result<Outcome, String> {
    let Ok(op) = Op::from_str(&args.op) else {
        let answer = ops::unknown_op(&args.op);
        return Ok(Outcome {
            text: answer.json()?,
            is_error: true,
        });
    };
    let query = args
        .filters
        .into_query(op, args.args)
        .map_err(|e| e.to_string())?;
    let paths: Vec<PathBuf> = if args.files.is_empty() {
        vec![open(None, None, env, true, Opens::Capture)?]
    } else {
        args.files
            .iter()
            .map(|file| open(Some(file), None, env, true, Opens::Capture))
            .collect::<Result<_, _>>()?
    };
    let answer = ops::query(&paths, &query)?;
    Ok(Outcome {
        text: answer.json()?,
        is_error: answer.error.is_some(),
    })
}

pub fn diff(args: DiffArgs, env: &Env) -> Result<Outcome, String> {
    let a = open(Some(&args.a), None, env, true, Opens::CaptureOrReport)?;
    let b = open(Some(&args.b), None, env, true, Opens::CaptureOrReport)?;
    let options = DiffOptions {
        side: request::side(args.side).map_err(|e| e.to_string())?,
        thread: args.thread,
        min_ms: args.min_ms.unwrap_or(0.0),
        limit: args.limit.unwrap_or(DEFAULT_LIMIT),
        calls: false,
    };
    let answer = ops::compare(&a, &b, &options)?;
    json(&DiffToolReply {
        text: answer.text(),
        reply: answer.reply(),
    })
}

pub fn list(args: ListArgs, env: &Env) -> Result<Outcome, String> {
    let dir = crate::cli::reports_dir(args.dir.as_deref(), env);
    let captures = ops::list(&dir)?
        .into_iter()
        .map(|listing| {
            let (state, error, size, sides) = match &listing.peek {
                None => (ListState::Unreadable, None, None, Vec::new()),
                Some(peek) => (
                    match (&peek.error, peek.finished) {
                        (Some(_), _) => ListState::Unreadable,
                        (None, true) => ListState::Finished,
                        (None, false) => ListState::Writing,
                    },
                    peek.error.clone(),
                    Some(peek.size),
                    peek.sides
                        .iter()
                        .map(|&(side, kind, seconds)| ListSide {
                            side: side.into(),
                            kind: kind.into(),
                            seconds,
                        })
                        .collect(),
                ),
            };
            ListRow {
                file: ops::file_name(&listing.path),
                path: listing.path.display().to_string(),
                state,
                error,
                size,
                modified_unix_s: listing.modified_unix_s,
                sides,
                line: listing.line,
            }
        })
        .collect();
    json(&ListReply { captures })
}

pub fn follow(args: FollowArgs, env: &Env) -> Result<Outcome, String> {
    let path = open(args.file.as_deref(), None, env, false, Opens::Capture)?;
    let batch = ops::follow_since(&path, args.cursor.unwrap_or(0))?;
    json(&FollowReply {
        events: batch.events,
        cursor: batch.cursor,
        done: batch.done,
    })
}

/// Runs `tool` on its JSON arguments; a refusal or a failure is an error outcome the agent can read.
pub fn call(tool: ToolName, arguments: &str, env: &Env) -> Result<Outcome, ParamError> {
    fn parsed<T: for<'de> Deserialize<'de>>(tool: ToolName, text: &str) -> Result<T, ParamError> {
        serde_json::from_str(text)
            .map_err(|e| ParamError(format!("invalid arguments for {tool}: {e}")))
    }
    let result = match tool {
        ToolName::Summary => summary(parsed(tool, arguments)?, env),
        ToolName::Query => query(parsed(tool, arguments)?, env),
        ToolName::Diff => diff(parsed(tool, arguments)?, env),
        ToolName::List => list(parsed(tool, arguments)?, env),
        ToolName::Follow => follow(parsed(tool, arguments)?, env),
    };
    Ok(result.unwrap_or_else(Outcome::error))
}

/// Arguments that do not fit the tool's schema: a protocol error, not a tool failure.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamError(pub String);
