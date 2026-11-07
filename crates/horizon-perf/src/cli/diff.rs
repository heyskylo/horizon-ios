use std::io::Write;
use std::path::Path;

use horizon_perf_format::v1::Capture;
use serde_json::Value;

use super::{CliError, Env, failed, file_name, parse, resolve, side_flag};
use crate::bench_diff;
use crate::diff::{DEFAULT_LIMIT, DiffOptions, diff};
use crate::read::read_capture;

/// A file `diff` accepts: a finished capture or a benchmark report.
pub(super) enum Report {
    Capture(Box<Capture>),
    Bench(Value),
}

pub(super) fn load(path: &Path) -> Result<Report, CliError> {
    let bytes =
        std::fs::read(path).map_err(|e| failed(format!("cannot read {}: {e}", path.display())))?;
    if bench_diff::looks_like_report(&bytes) {
        return bench_diff::parse(&bytes)
            .map(Report::Bench)
            .map_err(|e| failed(format!("{}: {e}", path.display())));
    }
    read_capture(&bytes)
        .map(|c| Report::Capture(Box::new(c)))
        .map_err(|e| failed(format!("{}: {e}", path.display())))
}

pub(super) fn run(args: &[String], env: &Env, out: &mut dyn Write) -> Result<(), CliError> {
    let parsed = parse(
        args,
        &["min-ms", "side", "thread", "limit", "dir"],
        &["json"],
    )?;
    let [a, b] = parsed.positional.as_slice() else {
        return Err(CliError::Usage("diff needs two files: A B".into()));
    };
    let dir = parsed.flags.get("dir").map(String::as_str);
    let (path_a, path_b) = (
        resolve(Some(a), dir, env, true)?,
        resolve(Some(b), dir, env, true)?,
    );
    let options = DiffOptions {
        side: side_flag(&parsed)?,
        thread: parsed.flags.get("thread").cloned(),
        min_ms: parsed.number("min-ms")?.unwrap_or(0.0),
        limit: parsed.number("limit")?.unwrap_or(DEFAULT_LIMIT),
    };
    let (name_a, name_b) = (file_name(&path_a), file_name(&path_b));
    let as_json = parsed.flags.contains_key("json");
    let text = match (load(&path_a)?, load(&path_b)?) {
        (Report::Capture(x), Report::Capture(y)) => {
            let d = diff(&x, &y, &options).map_err(failed)?;
            if as_json {
                format!("{}\n", d.json(&name_a, &name_b))
            } else {
                d.text(&name_a, &name_b)
            }
        }
        (Report::Bench(x), Report::Bench(y)) => {
            let d = bench_diff::diff(&x, &y, &options);
            if as_json {
                format!("{}\n", d.json(&name_a, &name_b))
            } else {
                d.text(&name_a, &name_b)
            }
        }
        _ => {
            return Err(failed(
                "cannot compare a capture with a benchmark report: give two of the same kind",
            ));
        }
    };
    out.write_all(text.as_bytes()).map_err(failed)
}
