use std::io::Write;
use std::path::Path;

use super::{CliError, Env, Flag, failed, parse, resolve};
use crate::ops;

pub(super) fn run(args: &[String], env: &Env, out: &mut dyn Write) -> Result<(), CliError> {
    let parsed = parse(args, &[Flag::Name, Flag::Body, Flag::Dir])?;
    let usage = || CliError::Usage("attach FILE --name NAME --body JSONFILE".into());
    let (Some(name), Some(body)) = (parsed.flags.get(&Flag::Name), parsed.flags.get(&Flag::Body))
    else {
        return Err(usage());
    };
    let Some(file) = parsed.one_file()? else {
        return Err(usage());
    };
    let dir = parsed.flags.get(&Flag::Dir).map(String::as_str);
    let path = resolve(Some(file), dir, env, true)?;
    let size = ops::attach(&path, name, Path::new(body)).map_err(failed)?;
    writeln!(
        out,
        "Attached {name} ({size} bytes) to {}",
        ops::file_name(&path)
    )
    .map_err(failed)
}
