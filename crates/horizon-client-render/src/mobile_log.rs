//! Bounded, privacy-filtered mobile diagnostics. Desktop logging is unchanged.
use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
};
pub const LIMIT: usize = 1024 * 1024;
/// Fail closed for authentication messages rather than guessing where a secret ends.
pub fn redact(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    if [
        "accesstoken", "refreshtoken", "idtoken", "token =", "token :", "user code", "access_token",
        "refresh_token",
        "id_token",
        "token:",
        "token=",
        "token\"",
        "authorization",
        "bearer",
        "password",
        "secret",
        "cookie",
        "xbl3.0",
        "device_code",
        "user_code",
        "sessionid",
        "session_id",
        "accesskey",
        "api_key",
    ]
    .iter()
    .any(|s| lower.contains(s))
    {
        return "[redacted sensitive log message]".to_owned();
    }
    text.lines()
        .map(|line| {
            line.split_whitespace()
                .map(|word| {
                    if word.len() > 128
                        || word.contains('@')
                        || word.contains("http://")
                        || word.contains("https://")
                        || word.contains("/Users/")
                        || word.contains("/var/mobile/")
                        || word.starts_with("eyJ")
                    {
                        "[redacted]".to_owned()
                    } else {
                        word.to_owned()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
pub fn rotate(file: &mut File, dir: &Path) -> io::Result<()> {
    if file.metadata()?.len() > 0 {
        // A pre-upgrade file can be huge. Only read a bounded tail, then scrub it.
        let len = file.metadata()?.len();
        file.seek(SeekFrom::Start(len.saturating_sub(LIMIT as u64)))?;
        let mut bytes = Vec::new();
        file.take(LIMIT as u64).read_to_end(&mut bytes)?;
        let text = String::from_utf8_lossy(&bytes);
        // Drop the first partial line so a cut-off secret cannot escape its label.
        let text = if len > LIMIT as u64 {
            text.split_once('\n').map_or("", |(_, tail)| tail)
        } else {
            &text
        };
        let mut safe = redact(text);
        if safe.len() > LIMIT {
            let mut end = LIMIT;
            while !safe.is_char_boundary(end) {
                end -= 1;
            }
            safe.truncate(end);
        }
        fs::write(dir.join("previous.log"), safe)?;
    }
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    Ok(())
}
pub fn append(file: &mut File, dir: &Path, text: &str) -> io::Result<()> {
    let mut safe = redact(text);
    if safe.len() > 32 * 1024 {
        let mut end = 32 * 1024;
        while !safe.is_char_boundary(end) {
            end -= 1;
        }
        safe.truncate(end);
        safe.push_str("\n[message truncated]");
    }
    if file.metadata()?.len() + safe.len() as u64 + 1 > LIMIT as u64 {
        rotate(file, dir)?;
    }
    writeln!(file, "{safe}")
}
