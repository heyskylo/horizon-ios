//! What a command and its MCP tool both do: open the files, call the library, return its reply.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use horizon_perf_format::v1::{Attachment, Capture};
use serde::{Deserialize, Serialize};

use crate::attach;
use crate::bare;
use crate::bench_diff::{self, BenchDiff, BenchReply, BenchReport};
use crate::cli::files::{self, Peek};
use crate::diff::{Diff, DiffEnvelope, DiffOptions, diff};
use crate::export::{self, ExportOptions, Format};
use crate::follow::Follower;
use crate::query::{self, Loaded, Query, QueryError, Row};
use crate::read::read_capture;
use crate::summary::{Baseline, SummaryOptions, summarize};
use crate::watch::{Event, Watcher};
use crate::wire::{Envelope, OpName};

const FOLLOW_CHUNK: usize = 64 * 1024;

/// The name a file is listed under in replies.
pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

/// One reply as the line it is printed as, without the newline.
pub fn json_text<T: Serialize>(reply: &T) -> Result<String, String> {
    serde_json::to_string(reply).map_err(|e| e.to_string())
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(link) = path
        .to_str()
        .filter(|s| s.starts_with("https://") || s.starts_with("http://"))
    {
        return crate::sharing::download(link);
    }
    std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

/// A file `diff` accepts: a finished capture or a benchmark report.
pub enum Report {
    Capture(Box<Capture>),
    Bench(BenchReport),
}

/// A capture file, or a bare benchmark report read as the capture it stands for.
fn read_any(path: &Path) -> Result<Capture, String> {
    let bytes = read_bytes(path)?;
    if bench_diff::looks_like_report(&bytes) {
        return bare::capture_of(&bytes);
    }
    read_capture(&bytes).map_err(|e| e.to_string())
}

pub fn load(path: &Path) -> Result<Report, String> {
    let bytes = read_bytes(path)?;
    if bench_diff::looks_like_report(&bytes) {
        return bench_diff::parse(&bytes)
            .map(Report::Bench)
            .map_err(|e| format!("{}: {e}", path.display()));
    }
    read_capture(&bytes)
        .map(|c| Report::Capture(Box::new(c)))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Adds the JSON document in `body` to the capture at `path` as an attachment called `name`; returns the body's size in bytes.
pub fn attach(path: &Path, name: &str, body: &Path) -> Result<usize, String> {
    let text = std::fs::read_to_string(body)
        .map_err(|e| format!("cannot read {}: {e}", body.display()))?;
    serde_json::from_str::<serde::de::IgnoredAny>(&text)
        .map_err(|e| format!("{} is not JSON: {e}", body.display()))?;
    let size = text.len();
    let attachment = Attachment {
        name: name.to_string(),
        media_type: attach::JSON_MEDIA_TYPE.to_string(),
        body: text,
    };
    let rewritten = attach::attach(&read_bytes(path)?, &attachment)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut scratch = path.as_os_str().to_owned();
    scratch.push(".attach");
    let scratch = PathBuf::from(scratch);
    std::fs::write(&scratch, rewritten)
        .and_then(|()| std::fs::rename(&scratch, path))
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(size)
}

/// The earlier capture a summary compares against.
pub fn baseline(path: &Path) -> Result<Baseline, String> {
    match load(path)? {
        Report::Capture(capture) => Ok(Baseline {
            name: file_name(path),
            capture: *capture,
        }),
        Report::Bench(_) => Err(format!(
            "{} is a benchmark report: summary --baseline needs a capture",
            path.display()
        )),
    }
}

pub fn summary(path: &Path, options: &SummaryOptions) -> Result<String, String> {
    let capture = read_any(path)?;
    summarize(&capture, &file_name(path), options).map_err(|e| e.to_string())
}

/// `summary` for a capture already decoded, as the wasm boundary and the tests hold it.
pub fn summary_capture(
    capture: &Capture,
    name: &str,
    options: &SummaryOptions,
) -> Result<String, String> {
    summarize(capture, name, options).map_err(|e| e.to_string())
}

/// The bytes of `format`'s export of the capture at `path`.
pub fn export(path: &Path, format: Format, options: &ExportOptions) -> Result<Vec<u8>, String> {
    let capture =
        read_capture(&read_bytes(path)?).map_err(|e| format!("{}: {e}", path.display()))?;
    export::export(&capture, format, options).map_err(|e| e.to_string())
}

/// `export` for a capture already decoded.
pub fn export_capture(
    capture: &Capture,
    format: Format,
    options: &ExportOptions,
) -> Result<Vec<u8>, String> {
    export::export(capture, format, options).map_err(|e| e.to_string())
}

/// A query's reply; a failed query has its error envelope and the message.
pub struct QueryAnswer {
    pub envelope: Envelope<Row>,
    pub error: Option<String>,
}

impl QueryAnswer {
    pub fn json(&self) -> Result<String, String> {
        json_text(&self.envelope)
    }
}

/// The failure envelope of an op that is not one this tool answers.
pub fn unknown_op(text: &str) -> QueryAnswer {
    let message = format!(
        "unknown query op {text}; ops: {}",
        crate::wire::Op::answered()
    );
    QueryAnswer {
        envelope: QueryError::new(&message).envelope(OpName::Other(text.to_string())),
        error: Some(message),
    }
}

pub fn query(paths: &[PathBuf], q: &Query) -> Result<QueryAnswer, String> {
    let mut loaded = Vec::new();
    for path in paths {
        let capture = read_any(path).map_err(|e| format!("{}: {e}", path.display()))?;
        loaded.push(Loaded {
            file: file_name(path),
            capture: std::sync::Arc::new(capture),
        });
    }
    Ok(query_loaded(q, &loaded))
}

/// The query over captures already decoded: `query` without the files, for the wasm boundary.
pub fn query_loaded(q: &Query, loaded: &[Loaded]) -> QueryAnswer {
    match query::run(q, loaded) {
        Ok(envelope) => QueryAnswer {
            envelope,
            error: None,
        },
        Err(e) => QueryAnswer {
            envelope: e.envelope(OpName::Query(q.op)),
            error: Some(e.message),
        },
    }
}

/// A diff reply body: a capture diff or a benchmark report diff.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DiffReply {
    Capture(DiffEnvelope),
    Bench(BenchReply),
}

pub enum DiffAnswer {
    Captures {
        diff: Diff,
        a: String,
        b: String,
    },
    Bench {
        diff: BenchDiff,
        a: String,
        b: String,
    },
}

impl DiffAnswer {
    pub fn text(&self) -> String {
        match self {
            DiffAnswer::Captures { diff, a, b } => diff.text(a, b),
            DiffAnswer::Bench { diff, a, b } => diff.text(a, b),
        }
    }

    pub fn reply(&self) -> DiffReply {
        match self {
            DiffAnswer::Captures { diff, a, b } => DiffReply::Capture(diff.envelope(a, b)),
            DiffAnswer::Bench { diff, a, b } => DiffReply::Bench(diff.reply(a, b)),
        }
    }
}

pub fn compare(a: &Path, b: &Path, options: &DiffOptions) -> Result<DiffAnswer, String> {
    let (name_a, name_b) = (file_name(a), file_name(b));
    match (load(a)?, load(b)?) {
        (Report::Capture(x), Report::Capture(y)) => {
            compare_captures((&name_a, &x), (&name_b, &y), options)
        }
        (Report::Bench(x), Report::Bench(y)) => Ok(DiffAnswer::Bench {
            diff: bench_diff::diff(&x, &y, options),
            a: name_a,
            b: name_b,
        }),
        _ => Err(
            "cannot compare a capture with a benchmark report: give two of the same kind".into(),
        ),
    }
}

/// Two captures already decoded, each with the name it is shown under: `compare` without the files.
pub fn compare_captures(
    a: (&str, &Capture),
    b: (&str, &Capture),
    options: &DiffOptions,
) -> Result<DiffAnswer, String> {
    Ok(DiffAnswer::Captures {
        diff: diff(a.1, b.1, options).map_err(|e| e.to_string())?,
        a: a.0.to_string(),
        b: b.0.to_string(),
    })
}

/// A capture file in a directory, as `list` shows it.
pub struct Listing {
    pub path: PathBuf,
    pub peek: Option<Peek>,
    pub line: String,
    pub modified_unix_s: Option<u64>,
}

pub fn list(dir: &Path) -> Result<Vec<Listing>, String> {
    let paths =
        files::captures_in(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    Ok(paths
        .into_iter()
        .map(|path| {
            let modified_unix_s = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs());
            let (peek, line) = match files::peek(&path) {
                Ok(peek) => {
                    let line = files::list_line(&path, &peek);
                    (Some(peek), line)
                }
                Err(e) => (None, format!("{}  unreadable: {e}", file_name(&path))),
            };
            Listing {
                path,
                peek,
                line,
                modified_unix_s,
            }
        })
        .collect())
}

/// What a capture wrote after a cursor; `cursor` is the next call's.
pub struct FollowBatch {
    pub events: Vec<Event>,
    pub cursor: u64,
    pub done: bool,
}

/// The events of the top-level fields from index `after` on, reading the file from its start each time.
pub fn follow_since(path: &Path, after: u64) -> Result<FollowBatch, String> {
    let bytes = read_bytes(path)?;
    let mut follower = Follower::new();
    let mut watcher = Watcher::new(0.0, true);
    let (mut index, mut events) = (0u64, Vec::new());
    for chunk in bytes.chunks(FOLLOW_CHUNK) {
        for entry in follower.push(chunk).map_err(|e| e.to_string())? {
            let event = watcher.event(&entry);
            if index >= after {
                events.extend(event);
            }
            index += 1;
        }
    }
    if after > index {
        return Err(format!(
            "cursor {after} is ahead of the capture, which has {index} fields so far"
        ));
    }
    Ok(FollowBatch {
        events,
        cursor: index,
        done: follower.is_finished() || follower.stream_complete(),
    })
}
