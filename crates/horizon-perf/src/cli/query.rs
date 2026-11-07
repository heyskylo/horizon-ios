use std::io::Write;
use std::path::{Path, PathBuf};

use super::{CliError, Env, Parsed, failed, file_name, parse, resolve, side_flag};
use crate::query::{self, Bound, DEFAULT_LIMIT, Filters, Loaded, Query};
use crate::read::read_capture;

const VALUED: [&str; 14] = [
    "side", "thread", "select", "where", "from", "to", "limit", "dir", "names", "worst", "over",
    "every", "min-mb", "by",
];
const SWITCHES: [&str; 3] = ["slow", "all", "diff"];
const SELECTS: [&str; 6] = ["cpu", "running", "waiting", "wall", "alloc", "live"];
const WHERE_KEYS: [&str; 6] = ["span", "entity", "block", "chunk", "dim", "dimension"];

/// A positional is a capture when it says so: `latest`, a `.hzperf` name or a file that exists.
fn is_capture(arg: &str) -> bool {
    arg == "latest" || arg.ends_with(".hzperf") || Path::new(arg).is_file()
}

fn bound(parsed: &Parsed, name: &str) -> Result<Option<Bound>, CliError> {
    let Some(text) = parsed.flags.get(name) else {
        return Ok(None);
    };
    if let Some(mark) = text.strip_prefix("mark:") {
        return Ok(Some(Bound::Mark(mark.to_string())));
    }
    text.parse()
        .map(|s| Some(Bound::Seconds(s)))
        .map_err(|_| CliError::Usage(format!("--{name} needs seconds or mark:NAME, got {text}")))
}

fn where_pairs(text: &str) -> Result<Vec<(String, String)>, CliError> {
    text.split(',')
        .filter(|p| !p.is_empty())
        .map(|pair| {
            let (key, value) = pair
                .split_once('=')
                .ok_or_else(|| CliError::Usage(format!("--where needs key=value, got {pair}")))?;
            if !WHERE_KEYS.contains(&key) {
                return Err(CliError::Usage(format!(
                    "--where keys are {}, got {key}",
                    WHERE_KEYS.join(", ")
                )));
            }
            Ok((key.to_string(), value.to_string()))
        })
        .collect()
}

fn build(parsed: &Parsed, args: Vec<String>, op: String) -> Result<Query, CliError> {
    let select = parsed
        .flags
        .get("select")
        .cloned()
        .unwrap_or_else(|| "cpu".into());
    if !SELECTS.contains(&select.as_str()) {
        return Err(CliError::Usage(format!(
            "--select is {}, got {select}",
            SELECTS.join(", ")
        )));
    }
    let filters = Filters {
        side: side_flag(parsed)?,
        thread: parsed.flags.get("thread").cloned(),
        select,
        slow: parsed.flags.contains_key("slow"),
        where_: parsed
            .flags
            .get("where")
            .map(|w| where_pairs(w))
            .transpose()?
            .unwrap_or_default(),
        from: bound(parsed, "from")?,
        to: bound(parsed, "to")?,
        limit: parsed.number("limit")?.unwrap_or(DEFAULT_LIMIT),
    };
    Ok(Query {
        op,
        args,
        filters,
        names: parsed
            .flags
            .get("names")
            .map(|n| {
                n.split(',')
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        worst: parsed.number("worst")?,
        over: parsed.number("over")?,
        every: parsed.number("every")?,
        diff: parsed.flags.contains_key("diff"),
        min_mb: parsed.number("min-mb")?,
        by: parsed.flags.get("by").cloned(),
    })
}

pub(super) fn query(args: &[String], env: &Env, out: &mut dyn Write) -> Result<(), CliError> {
    let parsed = parse(args, &VALUED, &SWITCHES)?;
    let (op, rest) = parsed
        .positional
        .split_first()
        .ok_or_else(|| CliError::Usage("query needs an OP".into()))?;
    let (files, plain): (Vec<&String>, Vec<&String>) = rest.iter().partition(|a| is_capture(a));
    let q = build(&parsed, plain.into_iter().cloned().collect(), op.clone())?;
    let dir = parsed.flags.get("dir").map(String::as_str);
    let paths: Vec<PathBuf> = if files.is_empty() {
        vec![resolve(None, dir, env, true)?]
    } else {
        files
            .iter()
            .map(|f| resolve(Some(f), dir, env, true))
            .collect::<Result<_, _>>()?
    };
    let mut loaded = Vec::new();
    for path in &paths {
        let bytes = std::fs::read(path)
            .map_err(|e| failed(format!("cannot read {}: {e}", path.display())))?;
        let capture =
            read_capture(&bytes).map_err(|e| failed(format!("{}: {e}", path.display())))?;
        loaded.push(Loaded {
            file: file_name(path),
            capture,
        });
    }
    match query::run(&q, &loaded) {
        Ok(reply) => writeln!(out, "{reply}").map_err(failed),
        Err(e) => {
            writeln!(out, "{}", e.to_json(&q.op)).map_err(failed)?;
            Err(CliError::Failed(e.message))
        }
    }
}
