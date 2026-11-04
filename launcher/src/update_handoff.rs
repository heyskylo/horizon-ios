//! The launcher's own update from its window (#9287). The window downloads and checks the new
//! launcher in the background ([`backend::stage_update`]) and says "A new version is available".
//! Update starts this launcher's file again as the installer and quits:
//! `horizon-launcher apply-update <tag> <staged file> [--restart [-- <the window's arguments>]]`.
//! The installer waits until the window's process has exited (its stdin is a pipe only that
//! process holds, which closes with it), puts the new launcher in place ([`backend::place_staged`],
//! the same code `horizon-launcher update` uses) and starts it. When that fails it starts the old
//! one again, which says why ([`take_failure`]). A window closed with an update staged hands it
//! over the same way without the restart ([`on_quit`]), so a launcher nobody updates by hand still
//! updates itself.
//!
//! `main` handles the command before the CLI parser, so MCP and the completions never list it.
//! Every launcher keeps it: the installer is whatever launcher file is in place when Update is
//! pressed.
use crate::backend::{self, Staged};
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;

pub const COMMAND: &str = "apply-update";
const RESTART: &str = "--restart";

/// How long the installer waits for the window to exit before it goes on: the replacement works
/// under a running launcher too (the old file is moved aside, not overwritten).
const WAIT: Duration = Duration::from_secs(60);

/// The update a closing window hands over ([`on_quit`]).
static PENDING: Mutex<Option<Staged>> = Mutex::new(None);

pub fn set_pending(staged: Option<Staged>) {
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = staged;
}

/// The window is closing: a staged update goes in once it has exited, and the next start is the
/// new launcher.
pub fn on_quit() {
    let staged = PENDING.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(staged) = staged {
        let _ = hand_over(&staged, false);
    }
}

/// Update was pressed: starts the installer for the staged update with a restart (true; the
/// caller quits right after), or reports that none is staged (false: the file in place already is
/// the new launcher, another window or `horizon-launcher update` put it there).
pub fn start_now() -> Result<bool, String> {
    let staged = PENDING.lock().unwrap_or_else(|e| e.into_inner()).take();
    match staged {
        Some(staged) => hand_over(&staged, true).map(|()| true),
        None => Ok(false),
    }
}

/// Starts the installer for `staged`; the caller quits right after. With `restart` the installer
/// starts the new launcher with this one's arguments.
pub fn hand_over(staged: &Staged, restart: bool) -> Result<(), String> {
    let installer = backend::update_target();
    let mut cmd = crate::process::command(&installer);
    cmd.args(installer_args(staged, restart, std::env::args_os().skip(1)))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Not in the terminal's process group: closing the window's terminal doesn't stop it.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Could not start {}: {e}", installer.display()))?;
    // The pipe's other end stays open until this process exits: the installer's sign to go on.
    std::mem::forget(child.stdin.take());
    Ok(())
}

fn installer_args(
    staged: &Staged,
    restart: bool,
    window_args: impl IntoIterator<Item = OsString>,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        COMMAND.into(),
        staged.tag.clone().into(),
        staged.payload.clone().into(),
    ];
    if restart {
        args.push(RESTART.into());
        args.push("--".into());
        args.extend(window_args);
    }
    args
}

/// What the installer was asked: the update, and the arguments to start the new launcher with.
#[derive(Debug, PartialEq)]
struct Request {
    staged: Staged,
    restart: Option<Vec<OsString>>,
}

/// `args` after the program's name; None unless they are an installer's whole command line.
fn parse(args: impl IntoIterator<Item = OsString>) -> Option<Request> {
    let mut args = args.into_iter();
    if args.next()? != COMMAND {
        return None;
    }
    let tag = args.next()?.into_string().ok()?;
    let payload = PathBuf::from(args.next()?);
    let restart = match args.next() {
        None => None,
        Some(flag) if flag == RESTART => match args.next() {
            None => Some(Vec::new()),
            Some(dashes) if dashes == "--" => Some(args.collect()),
            Some(_) => return None,
        },
        Some(_) => return None,
    };
    Some(Request {
        staged: Staged { tag, payload },
        restart,
    })
}

/// The installer, when this process was started as one (Some(exit code)); None otherwise.
pub fn run() -> Option<i32> {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if args.first()? != COMMAND {
        return None;
    }
    let Some(request) = parse(args) else {
        eprintln!(
            "usage: horizon-launcher {COMMAND} <tag> <staged file> [{RESTART} [-- <arguments>]]"
        );
        return Some(2);
    };
    wait_for_window(WAIT);
    let result = install(&request.staged);
    // Gone either way: a failed update is downloaded again at the next try.
    let _ = fs::remove_dir_all(backend::update_work_dir());
    if let Err(e) = &result {
        record_failure(e);
    }
    if let Some(args) = request.restart {
        let mut cmd = crate::process::command(backend::update_target());
        cmd.args(args).stdin(Stdio::null());
        if let Err(e) = cmd.spawn() {
            record_failure(&format!("{e}"));
        }
    }
    Some(i32::from(result.is_err()))
}

fn wait_for_window(limit: Duration) {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
        let _ = tx.send(());
    });
    let _ = rx.recv_timeout(limit);
}

fn install(staged: &Staged) -> Result<(), String> {
    // Only what a launcher downloaded and checked goes in, never a file from elsewhere.
    let work = backend::update_work_dir();
    if !staged.payload.starts_with(&work) || !staged.payload.exists() {
        return Err(format!(
            "{} is not a launcher update this launcher downloaded.",
            staged.payload.display()
        ));
    }
    backend::place_staged(staged)
}

fn failure_file() -> PathBuf {
    backend::data_dir().join(".launcher-update-failed")
}

fn record_failure(error: &str) {
    let _ = fs::create_dir_all(backend::data_dir());
    let _ = fs::write(failure_file(), error);
}

/// Why the last update from the window didn't go in, once: the file is removed as it is read.
pub fn take_failure() -> Option<String> {
    let error = fs::read_to_string(failure_file()).ok()?;
    let _ = fs::remove_file(failure_file());
    Some(error.trim().to_owned()).filter(|e| !e.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staged() -> Staged {
        Staged {
            tag: "launcher@0.2.41".into(),
            payload: PathBuf::from("/data/.launcher-update/unpacked/Horizon.app"),
        }
    }

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    /// What the window starts is what the installer reads back, the window's own arguments
    /// included (even one that looks like the installer's flags).
    #[test]
    fn the_installer_reads_the_command_line_the_window_gives_it() {
        let window = os(&["--restart", "play"]);
        let args = installer_args(&staged(), true, window.clone());
        assert_eq!(
            parse(args),
            Some(Request {
                staged: staged(),
                restart: Some(window),
            })
        );
        // Closing the window: no restart.
        let args = installer_args(&staged(), false, os(&["play"]));
        assert_eq!(
            parse(args),
            Some(Request {
                staged: staged(),
                restart: None,
            })
        );
    }

    #[test]
    fn other_command_lines_are_not_the_installer() {
        assert_eq!(parse(os(&["update"])), None);
        assert_eq!(parse(os(&[COMMAND, "launcher@0.2.41"])), None);
        assert_eq!(parse(os(&[COMMAND, "t", "/p", "--force"])), None);
        assert_eq!(parse(os(&[COMMAND, "t", "/p", RESTART, "x"])), None);
    }

    /// The installer puts in only what a launcher staged in its own update folder: a command line
    /// naming any other file changes nothing.
    #[test]
    fn the_installer_takes_only_a_staged_update() {
        let elsewhere = std::env::temp_dir().join("horizon-launcher-not-staged");
        fs::write(&elsewhere, b"not a launcher").unwrap();
        let result = install(&Staged {
            tag: "launcher@9.9.9".into(),
            payload: elsewhere.clone(),
        });
        assert!(result.is_err_and(|e| e.contains("not a launcher update")));
        assert!(elsewhere.exists());
        let _ = fs::remove_file(elsewhere);
    }
}
