use std::io::Write;

use serde::Serialize;

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
    let endpoint =
        std::env::var("PERF_UPLOAD_URL").unwrap_or_else(|_| horizon_perf_share::captures_url());
    let reply = if share {
        horizon_perf_share::share_saved(&endpoint, &path).map_err(failed)?
    } else {
        let bytes = std::fs::read(&path).map_err(failed)?;
        horizon_perf_share::upload(&endpoint, &bytes, false).map_err(failed)?
    };
    out.write_all(json_line(&reply)?.as_bytes()).map_err(failed)
}

#[derive(Serialize)]
struct Opened {
    viewer: &'static str,
    file: String,
    fetched: bool,
}

pub(super) fn open(args: &[String], env: &Env, out: &mut dyn Write) -> Result<(), CliError> {
    let parsed = parse(args, &[Flag::Dir])?;
    let path = resolve(
        parsed.one_file()?,
        parsed.flags.get(&Flag::Dir).map(String::as_str),
        env,
        true,
    )?;
    let offer = horizon_perf_share::offer(&path, horizon_perf_share::viewer()).map_err(failed)?;
    horizon_perf_share::open_in_browser(&offer.url).map_err(failed)?;
    let reply = Opened {
        viewer: horizon_perf_share::viewer(),
        file: path.display().to_string(),
        fetched: offer.wait(),
    };
    out.write_all(json_line(&reply)?.as_bytes()).map_err(failed)
}
