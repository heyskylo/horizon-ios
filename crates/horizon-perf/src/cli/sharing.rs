use std::io::Write;

use super::{CliError, Env, Flag, failed, json_line, parse, resolve};

pub(super) fn run(
    args: &[String],
    env: &Env,
    out: &mut dyn Write,
    share: bool,
) -> Result<(), CliError> {
    let parsed = parse(args, &[Flag::Dir])?;
    let path = resolve(
        parsed.one_file()?,
        parsed.flags.get(&Flag::Dir).map(String::as_str),
        env,
        true,
    )?;
    let bytes = std::fs::read(&path).map_err(failed)?;
    let endpoint =
        std::env::var("PERF_UPLOAD_URL").unwrap_or_else(|_| super::DEFAULT_UPLOAD_URL.to_string());
    let reply = crate::sharing::upload(&endpoint, &bytes, share).map_err(failed)?;
    out.write_all(json_line(&reply)?.as_bytes()).map_err(failed)
}
