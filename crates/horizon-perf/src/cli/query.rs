use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use super::{CliError, Env, Flag, Parsed, failed, file_name, json_line, parse, resolve, side_flag};
use crate::query::{self, Bound, DEFAULT_LIMIT, Filters, Loaded, Query, QueryError};
use crate::read::read_capture;
use crate::wire::{ContextKey, GroupBy, HotBy, Op, OpName, SampleBy, Select};

const FLAGS: [Flag; 17] = [
    Flag::Side,
    Flag::Thread,
    Flag::Select,
    Flag::Where,
    Flag::From,
    Flag::To,
    Flag::Limit,
    Flag::Dir,
    Flag::Names,
    Flag::Worst,
    Flag::Over,
    Flag::Every,
    Flag::MinMb,
    Flag::By,
    Flag::Match,
    Flag::Depth,
    Flag::MinPct,
];
const SWITCHES: [Flag; 3] = [Flag::Slow, Flag::All, Flag::Diff];

/// A positional is a capture when it says so: `latest`, a `.hzperf` name or a file that exists.
fn is_capture(arg: &str) -> bool {
    arg == "latest" || arg.ends_with(".hzperf") || Path::new(arg).is_file()
}

fn bound(parsed: &Parsed, flag: Flag) -> Result<Option<Bound>, CliError> {
    let Some(text) = parsed.flags.get(&flag) else {
        return Ok(None);
    };
    if let Some(mark) = text.strip_prefix("mark:") {
        return Ok(Some(Bound::Mark(mark.to_string())));
    }
    text.parse()
        .map(|s| Some(Bound::Seconds(s)))
        .map_err(|_| CliError::Usage(format!("--{flag} needs seconds or mark:NAME, got {text}")))
}

/// `k=v[,k=v]`; a piece without `=` continues the value before it, as in a chunk's `dim/x,z`.
fn where_pairs(text: &str) -> Result<BTreeMap<ContextKey, Vec<String>>, CliError> {
    let mut pairs: Vec<String> = Vec::new();
    for piece in text.split(',').filter(|p| !p.is_empty()) {
        match pairs.last_mut() {
            Some(last) if !piece.contains('=') => {
                last.push(',');
                last.push_str(piece);
            }
            _ => pairs.push(piece.to_string()),
        }
    }
    let mut out: BTreeMap<ContextKey, Vec<String>> = BTreeMap::new();
    for pair in pairs {
        let (key, value) = pair
            .split_once('=')
            .ok_or_else(|| CliError::Usage(format!("--where needs key=value, got {pair}")))?;
        let key = ContextKey::from_str(key).map_err(|_| {
            CliError::Usage(format!(
                "--where keys are {}, got {key}",
                ContextKey::spellings()
            ))
        })?;
        out.entry(key).or_default().push(value.to_string());
    }
    Ok(out)
}

fn build(parsed: &Parsed, args: Vec<String>, op: Op) -> Result<Query, CliError> {
    let select = match parsed.flags.get(&Flag::Select) {
        None => Select::Cpu,
        Some(text) => Select::from_str(text).map_err(|_| {
            CliError::Usage(format!("--select is {}, got {text}", Select::spellings()))
        })?,
    };
    let by_text = parsed.flags.get(&Flag::By);
    let usage =
        |what: &str, spellings: String| CliError::Usage(format!("--by is {spellings}, got {what}"));
    let (mut by, mut hot_by, mut by_sample) = (None, None, None);
    if let Some(text) = by_text {
        match op {
            Op::Hot => {
                hot_by = Some(HotBy::from_str(text).map_err(|_| usage(text, HotBy::spellings()))?);
            }
            Op::Split | Op::Group => {
                let picked = SampleBy::from_str(text)
                    .ok()
                    .filter(|b| op == Op::Split || b.groups());
                by_sample = Some(picked.ok_or_else(|| {
                    let ok: Vec<&str> = SampleBy::ALL
                        .iter()
                        .filter(|b| op == Op::Split || b.groups())
                        .map(|b| b.as_str())
                        .collect();
                    usage(text, ok.join(", "))
                })?);
            }
            _ => {
                by = Some(GroupBy::from_str(text).map_err(|_| usage(text, GroupBy::spellings()))?);
            }
        }
    }
    let filters = Filters {
        side: side_flag(parsed)?,
        thread: parsed.flags.get(&Flag::Thread).cloned(),
        select,
        slow: parsed.flags.contains_key(&Flag::Slow),
        where_: parsed
            .flags
            .get(&Flag::Where)
            .map(|w| where_pairs(w))
            .transpose()?
            .unwrap_or_default(),
        from: bound(parsed, Flag::From)?,
        to: bound(parsed, Flag::To)?,
        limit: parsed.number(Flag::Limit)?.unwrap_or(DEFAULT_LIMIT),
    };
    Ok(Query {
        op,
        args,
        filters,
        names: parsed
            .flags
            .get(&Flag::Names)
            .map(|n| {
                n.split(',')
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        worst: parsed.number(Flag::Worst)?,
        over: parsed.number(Flag::Over)?,
        every: parsed.number(Flag::Every)?,
        diff: parsed.flags.contains_key(&Flag::Diff),
        min_mb: parsed.number(Flag::MinMb)?,
        by,
        hot_by,
        by_sample,
        matching: parsed.flags.get(&Flag::Match).cloned(),
        depth: parsed.number(Flag::Depth)?,
        min_pct: parsed.number(Flag::MinPct)?,
    })
}

pub(super) fn query(args: &[String], env: &Env, out: &mut dyn Write) -> Result<(), CliError> {
    let allowed: Vec<Flag> = FLAGS.iter().chain(&SWITCHES).copied().collect();
    let parsed = parse(args, &allowed)?;
    let (op, rest) = parsed
        .positional
        .split_first()
        .ok_or_else(|| CliError::Usage("query needs an OP".into()))?;
    let Ok(op_known) = Op::from_str(op) else {
        let message = format!("unknown query op {op}; ops: {}", Op::answered());
        let reply = QueryError::new(&message).envelope(OpName::Other(op.clone()));
        out.write_all(json_line(&reply)?.as_bytes())
            .map_err(failed)?;
        return Err(CliError::Failed(message));
    };
    let (files, plain): (Vec<&String>, Vec<&String>) = rest.iter().partition(|a| is_capture(a));
    let q = build(&parsed, plain.into_iter().cloned().collect(), op_known)?;
    let dir = parsed.flags.get(&Flag::Dir).map(String::as_str);
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
        Ok(reply) => out.write_all(json_line(&reply)?.as_bytes()).map_err(failed),
        Err(e) => {
            let reply = e.envelope(OpName::Query(q.op));
            out.write_all(json_line(&reply)?.as_bytes())
                .map_err(failed)?;
            Err(CliError::Failed(e.message))
        }
    }
}
