use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use super::{CliError, Env, Flag, Parsed, failed, json_line, parse, resolve, side_flag};
use crate::ops;
use crate::request::{BoundArg, QueryRequest};
use crate::wire::{ContextKey, Op, Select, SideName};

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
    arg == "latest"
        || arg.ends_with(".hzperf")
        || arg.starts_with("http://")
        || arg.starts_with("https://")
        || Path::new(arg).is_file()
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

fn request(parsed: &Parsed) -> Result<QueryRequest, CliError> {
    let text = |flag: Flag| parsed.flags.get(&flag).cloned();
    let select = text(Flag::Select)
        .map(|t| {
            Select::from_str(&t).map_err(|_| {
                CliError::Usage(format!("--select is {}, got {t}", Select::spellings()))
            })
        })
        .transpose()?;
    Ok(QueryRequest {
        side: side_flag(parsed)?.map(SideName::from),
        thread: text(Flag::Thread),
        select,
        slow: parsed.flags.contains_key(&Flag::Slow),
        where_: parsed
            .flags
            .get(&Flag::Where)
            .map(|w| where_pairs(w))
            .transpose()?
            .unwrap_or_default(),
        from: text(Flag::From).map(BoundArg::Text),
        to: text(Flag::To).map(BoundArg::Text),
        limit: parsed.number(Flag::Limit)?,
        names: text(Flag::Names)
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
        by: text(Flag::By),
        matching: text(Flag::Match),
        depth: parsed.number(Flag::Depth)?,
        min_pct: parsed.number(Flag::MinPct)?,
        self_share: false,
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
        let answer = ops::unknown_op(op);
        out.write_all(json_line(&answer.envelope)?.as_bytes())
            .map_err(failed)?;
        return Err(CliError::Failed(answer.error.unwrap_or_default()));
    };
    let (files, plain): (Vec<&String>, Vec<&String>) = rest.iter().partition(|a| is_capture(a));
    let q = request(&parsed)?.into_query(op_known, plain.into_iter().cloned().collect())?;
    let dir = parsed.flags.get(&Flag::Dir).map(String::as_str);
    let paths: Vec<PathBuf> = if files.is_empty() {
        vec![resolve(None, dir, env, true)?]
    } else {
        files
            .iter()
            .map(|f| resolve(Some(f), dir, env, true))
            .collect::<Result<_, _>>()?
    };
    let answer = ops::query(&paths, &q).map_err(failed)?;
    out.write_all(json_line(&answer.envelope)?.as_bytes())
        .map_err(failed)?;
    match answer.error {
        None => Ok(()),
        Some(message) => Err(CliError::Failed(message)),
    }
}
