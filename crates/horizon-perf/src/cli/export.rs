use std::io::Write;
use std::str::FromStr;

use super::{CliError, Env, Flag, failed, parse, resolve, side_flag};
use crate::export::{ExportOptions, Format};
use crate::ops;
use crate::wire::Select;

const FORMATS: [(Flag, Format); 3] = [
    (Flag::Pprof, Format::Pprof),
    (Flag::Folded, Format::Folded),
    (Flag::Perfetto, Format::Perfetto),
];

pub(super) fn run(args: &[String], env: &Env, out: &mut dyn Write) -> Result<(), CliError> {
    let parsed = parse(
        args,
        &[
            Flag::Pprof,
            Flag::Folded,
            Flag::Perfetto,
            Flag::Out,
            Flag::Side,
            Flag::Thread,
            Flag::Select,
            Flag::Slow,
            Flag::Dir,
        ],
    )?;
    let asked: Vec<Format> = FORMATS
        .iter()
        .filter(|(flag, _)| parsed.flags.contains_key(flag))
        .map(|(_, format)| *format)
        .collect();
    let [format] = asked.as_slice() else {
        return Err(CliError::Usage(
            "export needs exactly one of --pprof, --folded, --perfetto".into(),
        ));
    };
    let select = parsed
        .flags
        .get(&Flag::Select)
        .map(|t| {
            Select::from_str(t).map_err(|_| {
                CliError::Usage(format!("--select is {}, got {t}", Select::spellings()))
            })
        })
        .transpose()?
        .unwrap_or(Select::Cpu);
    let options = ExportOptions {
        side: side_flag(&parsed)?,
        thread: parsed.flags.get(&Flag::Thread).cloned(),
        select,
        slow: parsed.flags.contains_key(&Flag::Slow),
    };
    let dir = parsed.flags.get(&Flag::Dir).map(String::as_str);
    let path = resolve(parsed.one_file()?, dir, env, true)?;
    let bytes = ops::export(&path, *format, &options).map_err(failed)?;
    match parsed.flags.get(&Flag::Out) {
        None => out.write_all(&bytes).map_err(failed),
        Some(target) => {
            std::fs::write(target, &bytes)
                .map_err(|e| failed(format!("cannot write {target}: {e}")))?;
            writeln!(out, "wrote {target} ({} bytes)", bytes.len()).map_err(failed)
        }
    }
}
