use std::io::Write;

use super::{CliError, Env, Flag, failed, json_line, parse, resolve, side_flag};
use crate::diff::{DEFAULT_LIMIT, DiffOptions};
use crate::ops;

pub(super) fn run(args: &[String], env: &Env, out: &mut dyn Write) -> Result<(), CliError> {
    let parsed = parse(
        args,
        &[
            Flag::MinMs,
            Flag::Side,
            Flag::Thread,
            Flag::Limit,
            Flag::Dir,
            Flag::Json,
            Flag::Calls,
        ],
    )?;
    let [a, b] = parsed.positional.as_slice() else {
        return Err(CliError::Usage("diff needs two files: A B".into()));
    };
    let dir = parsed.flags.get(&Flag::Dir).map(String::as_str);
    let (path_a, path_b) = (
        resolve(Some(a), dir, env, true)?,
        resolve(Some(b), dir, env, true)?,
    );
    let options = DiffOptions {
        side: side_flag(&parsed)?,
        thread: parsed.flags.get(&Flag::Thread).cloned(),
        min_ms: parsed.number(Flag::MinMs)?.unwrap_or(0.0),
        limit: parsed.number(Flag::Limit)?.unwrap_or(DEFAULT_LIMIT),
        calls: parsed.flags.contains_key(&Flag::Calls),
    };
    let answer = ops::compare(&path_a, &path_b, &options).map_err(failed)?;
    let text = if parsed.flags.contains_key(&Flag::Json) {
        json_line(&answer.reply())?
    } else {
        answer.text()
    };
    out.write_all(text.as_bytes()).map_err(failed)
}
