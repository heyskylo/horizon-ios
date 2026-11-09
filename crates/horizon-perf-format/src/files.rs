use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;

/// Where the web UI that opens a capture lives.
pub const PERF_UI_URL: &str = "https://playsunrise.net/perf";
/// Captures a `reports/` folder keeps.
pub const KEEP_FILES: usize = 30;
pub const KEEP_BYTES: u64 = 1 << 30;

const PREFIX: &str = "profile-";
const SUFFIX: &str = "Z.hzperf";

/// `profile-YYYYMMDD-HHMMSSZ.hzperf` for a UTC time in seconds since the Unix epoch.
pub fn file_name(unix_secs: i64) -> String {
    let days = unix_secs.div_euclid(86_400);
    let of_day = unix_secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{PREFIX}{y:04}{m:02}{d:02}-{:02}{:02}{:02}{SUFFIX}",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60
    )
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Whether a file name is exactly one that `file_name` makes: a renamed capture is not.
pub fn is_capture_name(name: &str) -> bool {
    let Some(stamp) = name
        .strip_prefix(PREFIX)
        .and_then(|n| n.strip_suffix(SUFFIX))
    else {
        return false;
    };
    let b = stamp.as_bytes();
    b.len() == 15
        && b[8] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 8 || c.is_ascii_digit())
}

/// Creates the folder and a new capture file in it named for `unix_secs`, or the next free second.
pub fn create(dir: &Path, unix_secs: i64) -> io::Result<(File, PathBuf)> {
    fs::create_dir_all(dir)?;
    for second in unix_secs..unix_secs + 1000 {
        let path = dir.join(file_name(second));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((file, path)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("no free capture file name"))
}

/// Creates the folder and the capture file at exactly `path`, replacing a file that is there.
pub fn create_at(path: &Path) -> io::Result<File> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    File::create(path)
}

/// Deletes the oldest captures beyond `max_files` or `max_bytes` together, never `keep` or a file that isn't named like a capture.
pub fn retain(
    dir: &Path,
    keep: &Path,
    max_files: usize,
    max_bytes: u64,
) -> io::Result<Vec<PathBuf>> {
    let mut files: Vec<(String, PathBuf, u64)> = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let meta = entry.metadata()?;
        if meta.is_file() && is_capture_name(&name) {
            files.push((name, entry.path(), meta.len()));
        }
    }
    files.sort();
    let mut count = files.len();
    let mut bytes: u64 = files.iter().map(|f| f.2).sum();
    let mut deleted = Vec::new();
    for (_, path, len) in files {
        if count <= max_files && bytes <= max_bytes {
            break;
        }
        if path == keep {
            continue;
        }
        fs::remove_file(&path)?;
        count -= 1;
        bytes -= len;
        deleted.push(path);
    }
    Ok(deleted)
}

/// The line a finished capture prints.
pub fn saved_line(path: &Path) -> String {
    format!(
        "Saved {}. Open it at {PERF_UI_URL} (the file stays on your computer)",
        path.display()
    )
}

/// Whether the file's gzip stream has ended: false for a capture still being written.
pub fn is_complete(path: &Path) -> io::Result<bool> {
    if fs::metadata(path)?.len() == 0 {
        return Ok(false);
    }
    let mut gz = GzDecoder::new(File::open(path)?);
    let mut buf = [0u8; 64 * 1024];
    loop {
        match gz.read(&mut buf) {
            Ok(0) => return Ok(true),
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(false),
            Err(e) => return Err(e),
        }
    }
}
