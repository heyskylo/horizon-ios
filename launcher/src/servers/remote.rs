//! A server on another computer (a VPS, a home server), reached over SSH: the launcher runs the same
//! `horizon-server` commands there as `ssh HOST horizon-server --dir DIR ...`, with the system's `ssh` and the
//! player's own keys and `~/.ssh/config`. `BatchMode` keeps ssh from asking for a password, so the launcher
//! never sees or stores one. The other computer runs Linux or macOS (a POSIX shell runs the command there).

use super::Status;
use crate::i18n::{tr, trf};
use std::ffi::OsStr;
use std::process::{Command, Stdio};

/// The options every connection takes: never prompt, give up on a dead address in 10 s, and notice a
/// dropped connection within a minute.
const OPTIONS: [&str; 8] = [
    "-o",
    "BatchMode=yes",
    "-o",
    "ConnectTimeout=10",
    "-o",
    "ServerAliveInterval=15",
    "-o",
    "ServerAliveCountMax=4",
];

/// `install.sh` puts `horizon-server` in `~/.local/bin`, which a command run over ssh (no login shell)
/// often doesn't have on its PATH.
const PATH: &str = "PATH=\"$HOME/.local/bin:$PATH\"";

/// What the check prints when the folder holds no `server.toml`.
const NO_SERVER: &str = "horizon-server: no server in this folder";

/// Checks what the player typed as the computer: `user@host`, `host`, or a `Host` name from
/// `~/.ssh/config`. Nothing that ssh could read as an option or that holds spaces.
pub fn check_host(host: &str) -> Result<(), String> {
    if host.is_empty() {
        return Err(
            tr("Type the computer: user@address, or a Host name from your ~/.ssh/config.").into(),
        );
    }
    if host.starts_with('-') || host.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(trf(
            "{host} isn't a computer ssh can connect to.",
            &[("host", &format!("{host:?}"))],
        ));
    }
    Ok(())
}

/// The server's folder there, as the remote shell gets it: `~/x` is written `x`, since a command run
/// over ssh starts in the home folder.
fn remote_dir(dir: &str) -> Result<&str, String> {
    let dir = dir.trim();
    let dir = dir.strip_prefix("~/").unwrap_or(dir);
    if dir.is_empty() || dir == "~" || dir.chars().any(char::is_control) {
        return Err(tr("Type the server's folder on that computer, like /srv/minecraft.").into());
    }
    Ok(dir)
}

/// One argument for a POSIX shell, in single quotes.
pub fn quote(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', r"'\''"))
}

/// ssh's arguments for `horizon-server --dir <dir> <args>` on `host`.
pub fn ssh_args(host: &str, dir: &str, args: &[&str]) -> Result<Vec<String>, String> {
    check_host(host)?;
    let dir = remote_dir(dir)?;
    let mut line = format!("{PATH} exec horizon-server --dir {}", quote(dir));
    for arg in args {
        line.push(' ');
        line.push_str(&quote(arg));
    }
    let mut out: Vec<String> = OPTIONS.iter().map(|s| s.to_string()).collect();
    out.extend(["-T".into(), "--".into(), host.into(), line]);
    Ok(out)
}

/// `horizon-server --dir <dir> <args>` on `host`, through `ssh`.
pub fn command(host: &str, dir: &str, args: &[&str]) -> Result<Command, String> {
    command_with("ssh", host, dir, args)
}

fn command_with(
    ssh: impl AsRef<OsStr>,
    host: &str,
    dir: &str,
    args: &[&str],
) -> Result<Command, String> {
    let mut cmd = crate::process::command(ssh);
    cmd.args(ssh_args(host, dir, args)?);
    Ok(cmd)
}

/// Connects and asks the server there for its status, as the Add dialog does before it adds a server.
pub fn check(host: &str, dir: &str) -> Result<Status, String> {
    check_with("ssh", host, dir)
}

fn check_with(ssh: impl AsRef<OsStr>, host: &str, dir: &str) -> Result<Status, String> {
    let mut args = ssh_args(host, dir, &["status", "--json"])?;
    // `status` answers "stopped" for any folder: say so when there is no server in it at all.
    let folder = quote(remote_dir(dir)?);
    let line = args.pop().unwrap_or_default();
    args.push(format!(
        "[ -f {folder}/server.toml ] || {{ echo \"{NO_SERVER}\" >&2; exit 3; }}; {line}"
    ));
    let output = crate::process::command(ssh)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => tr("This computer has no ssh program. It comes with Windows 10 and later (Settings › Optional features › OpenSSH Client), macOS and Linux.").to_string(),
            _ => trf("Couldn't run ssh: {error}", &[("error", &e.to_string())]),
        })?;
    let out = String::from_utf8_lossy(&output.stdout);
    let err = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(explain(host, dir, output.status.code(), &err));
    }
    let value: serde_json::Value = out
        .lines()
        .rev()
        .find_map(|l| serde_json::from_str(l).ok())
        .ok_or_else(|| {
            trf(
                "horizon-server on {host} answered something unexpected.",
                &[("host", host)],
            )
        })?;
    Ok(Status::parse(&value))
}

/// What went wrong, in plain words, from ssh's exit code and what it or the remote shell printed.
pub fn explain(host: &str, dir: &str, code: Option<i32>, stderr: &str) -> String {
    let has = |s: &str| stderr.contains(s);
    if has(NO_SERVER) {
        return trf(
            "There's no server in {dir} on {host}. Check the folder, or set one up there with horizon-server init.",
            &[("dir", dir), ("host", host)],
        );
    }
    if has("Permission denied") {
        return trf(
            "{host} didn't accept your SSH key. Set one up once with ssh-copy-id {host} (or your host's control panel), then try again.",
            &[("host", host)],
        );
    }
    if has("Host key verification failed") || has("REMOTE HOST IDENTIFICATION HAS CHANGED") {
        return trf(
            "ssh doesn't know {host}'s key yet, or it changed. Connect once in a terminal with ssh {host} to check and accept it.",
            &[("host", host)],
        );
    }
    if has("Could not resolve hostname") {
        return trf(
            "There's no computer called {host}. Check the address.",
            &[("host", host)],
        );
    }
    if has("timed out") || has("Connection refused") || has("No route to host") {
        return trf(
            "Can't reach {host}. It may be off, or its SSH port closed.",
            &[("host", host)],
        );
    }
    if code == Some(127) || has("command not found") {
        return trf(
            "{host} has no horizon-server. Install it there first, as the hosting guide says.",
            &[("host", host)],
        );
    }
    if code == Some(255) {
        let said = stderr.lines().rev().find(|l| !l.trim().is_empty());
        return match said {
            Some(said) => trf(
                "Couldn't connect to {host}: {reason}",
                &[("host", host), ("reason", said.trim())],
            ),
            None => trf("Couldn't connect to {host}.", &[("host", host)]),
        };
    }
    let said = stderr
        .lines()
        .rev()
        .find_map(|l| l.strip_prefix("horizon-server: "))
        .map(str::to_string)
        .unwrap_or_else(|| {
            trf(
                "horizon-server on {host} failed in {dir}.",
                &[("host", host), ("dir", dir)],
            )
        });
    super::capitalised(&said)
}

#[cfg(test)]
#[path = "remote_tests.rs"]
mod tests;
