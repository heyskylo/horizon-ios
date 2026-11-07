use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use horizon_perf_format::Field;
use horizon_perf_format::v1::{Kind, Side};

use crate::error::side_name;
use crate::follow::{Entry, Follower};

pub const EXTENSION: &str = "hzperf";

const PEEK_CHUNK: usize = 64 * 1024;

/// Every `.hzperf` file directly in `dir`, newest modification first.
pub fn captures_in(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files: Vec<(SystemTime, PathBuf)> = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == EXTENSION) && path.is_file() {
            let modified = fs::metadata(&path)
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            files.push((modified, path));
        }
    }
    files.sort_by(|a, b| b.cmp(a));
    Ok(files.into_iter().map(|(_, path)| path).collect())
}

/// What a file holds, read as far as it has been written.
pub struct Peek {
    pub size: u64,
    pub finished: bool,
    pub error: Option<String>,
    /// Per side, server first: its kind and, once stopped, its duration.
    pub sides: Vec<(Side, Kind, Option<f64>)>,
}

pub fn peek(path: &Path) -> io::Result<Peek> {
    let mut file = fs::File::open(path)?;
    let mut follower = Follower::new();
    let mut sides: Vec<(Side, Kind, Option<f64>)> = Vec::new();
    let mut chunk = vec![0u8; PEEK_CHUNK];
    let mut size = 0u64;
    let mut error = None;
    loop {
        let n = file.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        match follower.push(&chunk[..n]) {
            Ok(entries) => note_sides(&mut sides, entries),
            Err(e) => {
                error = Some(e.to_string());
                break;
            }
        }
    }
    sides.sort_by_key(|s| std::cmp::Reverse(s.0 as i32));
    Ok(Peek {
        size,
        finished: error.is_none() && follower.is_finished(),
        error,
        sides,
    })
}

fn note_sides(sides: &mut Vec<(Side, Kind, Option<f64>)>, entries: Vec<Entry>) {
    for entry in entries {
        let Entry::Field(field) = entry else { continue };
        let side = field.side();
        if !sides.iter().any(|s| s.0 == side) {
            sides.push((side, Kind::Unspecified, None));
        }
        let slot = sides.iter_mut().find(|s| s.0 == side).unwrap();
        match field {
            Field::Start(s) => slot.1 = s.kind(),
            Field::Stop(s) => slot.2 = Some(s.t),
            _ => {}
        }
    }
}

/// The newest capture in `dir`; with `finished_only`, newest that every side has stopped writing.
pub fn latest(dir: &Path, finished_only: bool) -> Result<PathBuf, String> {
    let files = captures_in(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    files
        .into_iter()
        .find(|path| !finished_only || peek(path).is_ok_and(|p| p.finished))
        .ok_or_else(|| {
            let what = if finished_only {
                "finished captures"
            } else {
                "captures"
            };
            format!("no {what} in {}", dir.display())
        })
}

pub fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Dedicated => "dedicated",
        Kind::Integrated => "integrated",
        Kind::Client => "client",
        Kind::Unspecified => "unknown",
    }
}

/// One line for `horizon-perf list`.
pub fn list_line(path: &Path, peek: &Peek) -> String {
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let state = match &peek.error {
        Some(e) => format!("unreadable: {e}"),
        None if peek.finished => "finished".to_string(),
        None => "writing".to_string(),
    };
    let sides = peek
        .sides
        .iter()
        .map(|(side, kind, seconds)| {
            let label = if *kind == Kind::Unspecified {
                side_name(*side)
            } else {
                kind_label(*kind)
            };
            match seconds {
                Some(s) => format!("{label} {s:.1} s"),
                None => label.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(" + ");
    let mut line = format!("{name}  {}  {state}", size_label(peek.size));
    if !sides.is_empty() {
        line.push_str("  ");
        line.push_str(&sides);
    }
    line
}

fn size_label(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    let b = bytes as f64;
    if b >= KIB * KIB {
        format!("{:.1} MiB", b / (KIB * KIB))
    } else if b >= KIB {
        format!("{:.1} KiB", b / KIB)
    } else {
        format!("{bytes} B")
    }
}
