//! `horizon-server service install|uninstall|print`: run the server at boot or login under the system's own
//! service manager (docs/server.md, section 3): a systemd unit on Linux, a launchd agent on macOS, a task at
//! logon on Windows. The service runs `horizon-server run`, which restarts the game if it crashes; the
//! service manager restarts the supervisor itself.

use std::{
    path::{Path, PathBuf},
    process::Command,
};

/// The service's name for a server folder: `horizon-server-<folder>`, in letters, digits and dashes.
pub fn name(dir: &Path) -> String {
    let folder = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut clean: String = folder
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    clean = clean.trim_matches('-').to_owned();
    if clean.is_empty() {
        clean = "server".into();
    }
    format!("horizon-server-{clean}")
}

/// The program the service runs: the `horizon-server` on the PATH (the installer's launcher, which keeps
/// running the current version after an update), else this program, through the installer's `current`
/// link when it runs from an installed release.
fn program() -> Result<PathBuf, String> {
    let on_path = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .map(|d| d.join(format!("horizon-server{}", std::env::consts::EXE_SUFFIX)))
        .find(|p| p.is_file());
    match on_path {
        Some(path) => Ok(path),
        None => std::env::current_exe()
            .map(|exe| through_current(&exe))
            .map_err(|e| e.to_string()),
    }
}

/// `<dir>/releases/<tag>/horizon-server` as `<dir>/current/horizon-server` while `current` points at that
/// release (install.sh and install.ps1's layout), so an update applies at the next restart. Linux reports
/// the running program with links resolved, so the release's own path is what this program sees there.
fn through_current(exe: &Path) -> PathBuf {
    let current = (|| {
        let release = exe.parent()?;
        let releases = release.parent()?;
        if releases.file_name()? != "releases" {
            return None;
        }
        let current = releases.parent()?.join("current");
        let name = exe.file_name()?;
        let same = std::fs::canonicalize(&current).ok()? == std::fs::canonicalize(release).ok()?;
        same.then(|| current.join(name))
    })();
    current.unwrap_or_else(|| exe.to_path_buf())
}

/// A systemd unit: a user unit, or with `user` a system unit running as that user.
pub fn systemd_unit(program: &Path, dir: &Path, user: Option<&str>) -> String {
    let (run_as, wanted_by) = match user {
        Some(user) => (format!("User={user}\n"), "multi-user.target"),
        None => (String::new(), "default.target"),
    };
    format!(
        "[Unit]
Description=Horizon server in {dir}
Documentation=https://github.com/horizon-rust/horizon/blob/main/docs/server.md
Wants=network-online.target
After=network-online.target

[Service]
Type=simple
{run_as}WorkingDirectory={dir}
ExecStart=\"{program}\" --dir \"{dir}\" run
# SIGTERM makes the supervisor stop the game cleanly; saving a big world takes a moment.
TimeoutStopSec=180
Restart=on-failure
RestartSec=10

[Install]
WantedBy={wanted_by}
",
        dir = dir.display(),
        program = program.display(),
    )
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A launchd agent: started at login, and again if the supervisor exits with an error.
pub fn launchd_plist(label: &str, program: &Path, dir: &Path) -> String {
    let log = dir.join("logs/supervisor.log");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{label}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{program}</string>
		<string>--dir</string>
		<string>{dir}</string>
		<string>run</string>
	</array>
	<key>WorkingDirectory</key>
	<string>{dir}</string>
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<dict>
		<key>SuccessfulExit</key>
		<false/>
	</dict>
	<key>ExitTimeOut</key>
	<integer>180</integer>
	<key>StandardOutPath</key>
	<string>{log}</string>
	<key>StandardErrorPath</key>
	<string>{log}</string>
</dict>
</plist>
"#,
        program = xml(&program.display().to_string()),
        dir = xml(&dir.display().to_string()),
        log = xml(&log.display().to_string()),
    )
}

fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set".into())
}

fn launchd_path(label: &str) -> Result<PathBuf, String> {
    Ok(home()?
        .join("Library/LaunchAgents")
        .join(format!("{label}.plist")))
}

fn launchd_label(dir: &Path) -> String {
    format!(
        "dev.horizon.server.{}",
        name(dir).trim_start_matches("horizon-server-")
    )
}

fn systemd_path(service: &str, system: bool) -> Result<PathBuf, String> {
    Ok(if system {
        PathBuf::from("/etc/systemd/system")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map_or_else(
                || home().map(|h| h.join(".config")),
                |c| Ok(PathBuf::from(c)),
            )?
            .join("systemd/user")
    }
    .join(format!("{service}.service")))
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let status = crate::process::command(program)
        .args(args)
        .status()
        .map_err(|e| format!("{program}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`{program} {}` failed ({status})", args.join(" ")))
    }
}

/// A step that may fail harmlessly (stopping what isn't running): its output would only alarm.
fn quiet(program: &str, args: &[&str]) {
    let _ = crate::process::command(program)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// What `service print` shows and `service install` writes, and where.
fn definition(dir: &Path, system: bool) -> Result<(PathBuf, String), String> {
    let program = program()?;
    if cfg!(target_os = "macos") {
        if system {
            return Err("on macOS the server runs as a login agent: drop --system".into());
        }
        let label = launchd_label(dir);
        Ok((launchd_path(&label)?, launchd_plist(&label, &program, dir)))
    } else if cfg!(windows) {
        Err(
            "on Windows, `service install` creates a task at logon; there is no file to print"
                .into(),
        )
    } else {
        let user = system.then(|| std::env::var("USER").unwrap_or_else(|_| "root".into()));
        Ok((
            systemd_path(&name(dir), system)?,
            systemd_unit(&program, dir, user.as_deref()),
        ))
    }
}

pub fn command(dir: &Path, args: &[String]) -> Result<(), String> {
    let dir = dir
        .canonicalize()
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    crate::config::Config::load(&dir)?;
    let system = args.iter().any(|a| a == "--system");
    let service = name(&dir);
    match args.first().map(String::as_str) {
        Some("print") => {
            let (path, text) = definition(&dir, system)?;
            println!("# {}\n{text}", path.display());
            Ok(())
        }
        Some("install") => install(&dir, &service, system),
        Some("uninstall") => uninstall(&dir, &service, system),
        _ => Err("use: service install|uninstall|print [--system]".into()),
    }
}

fn install(dir: &Path, service: &str, system: bool) -> Result<(), String> {
    if cfg!(windows) {
        let program = program()?;
        let action = format!("\"{}\" --dir \"{}\" run", program.display(), dir.display());
        run(
            "schtasks",
            &[
                "/Create", "/TN", service, "/TR", &action, "/SC", "ONLOGON", "/RL", "LIMITED", "/F",
            ],
        )?;
        run("schtasks", &["/Run", "/TN", service])?;
        println!("Installed the task {service}: the server starts when you log in, and now.");
        return Ok(());
    }
    let (path, text) = definition(dir, system)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::create_dir_all(dir.join("logs")).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| {
        format!(
            "{}: {e}{}",
            path.display(),
            if system { " (--system needs sudo)" } else { "" }
        )
    })?;
    if cfg!(target_os = "macos") {
        let uid = String::from_utf8_lossy(
            &Command::new("id")
                .arg("-u")
                .output()
                .map_err(|e| e.to_string())?
                .stdout,
        )
        .trim()
        .to_owned();
        quiet(
            "launchctl",
            &[
                "bootout",
                &format!("gui/{uid}"),
                &path.display().to_string(),
            ],
        );
        run(
            "launchctl",
            &[
                "bootstrap",
                &format!("gui/{uid}"),
                &path.display().to_string(),
            ],
        )?;
        println!(
            "Installed {}: the server starts at login, and now. Its output: logs/supervisor.log",
            path.display()
        );
    } else {
        let scope: &[&str] = if system { &[] } else { &["--user"] };
        run("systemctl", &[scope, &["daemon-reload"]].concat())?;
        run(
            "systemctl",
            &[scope, &["enable", "--now", service]].concat(),
        )?;
        println!(
            "Installed {}: the server starts at boot, and now.",
            path.display()
        );
        if !system {
            println!(
                "To keep it running while you are logged out: loginctl enable-linger $USER. \
                 Its log: journalctl --user -u {service}"
            );
        }
    }
    Ok(())
}

fn uninstall(dir: &Path, service: &str, system: bool) -> Result<(), String> {
    if cfg!(windows) {
        quiet("schtasks", &["/End", "/TN", service]);
        run("schtasks", &["/Delete", "/TN", service, "/F"])?;
        println!("Removed the task {service}.");
        return Ok(());
    }
    let (path, _) = definition(dir, system)?;
    if cfg!(target_os = "macos") {
        let uid = String::from_utf8_lossy(
            &Command::new("id")
                .arg("-u")
                .output()
                .map_err(|e| e.to_string())?
                .stdout,
        )
        .trim()
        .to_owned();
        quiet(
            "launchctl",
            &[
                "bootout",
                &format!("gui/{uid}"),
                &path.display().to_string(),
            ],
        );
    } else {
        let scope: &[&str] = if system { &[] } else { &["--user"] };
        quiet(
            "systemctl",
            &[scope, &["disable", "--now", service]].concat(),
        );
    }
    std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !cfg!(target_os = "macos") {
        let scope: &[&str] = if system { &[] } else { &["--user"] };
        quiet("systemctl", &[scope, &["daemon-reload"]].concat());
    }
    println!(
        "Removed {}; the server's folder is untouched.",
        path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_folder() {
        assert_eq!(
            name(Path::new("/srv/My Server!")),
            "horizon-server-my-server"
        );
        assert_eq!(name(Path::new("/")), "horizon-server-server");
    }

    #[test]
    fn units_run_the_supervisor() {
        let unit = systemd_unit(
            Path::new("/home/a/.local/bin/horizon-server"),
            Path::new("/srv/mc"),
            Some("mc"),
        );
        assert!(
            unit.contains("ExecStart=\"/home/a/.local/bin/horizon-server\" --dir \"/srv/mc\" run")
        );
        assert!(unit.contains("User=mc\n"));
        assert!(unit.contains("WantedBy=multi-user.target"));
        let plist = launchd_plist(
            "dev.horizon.server.mc",
            Path::new("/opt/h/horizon-server"),
            Path::new("/srv/R&D"),
        );
        assert!(plist.contains("<string>/srv/R&amp;D</string>"));
        assert!(plist.contains("<string>/srv/R&amp;D/logs/supervisor.log</string>"));
    }

    #[cfg(unix)]
    #[test]
    fn an_installed_release_runs_through_current() {
        let root =
            std::env::temp_dir().join(format!("horizon-service-current-{}", std::process::id()));
        let release = root.join("releases/v1");
        std::fs::create_dir_all(&release).unwrap();
        let exe = release.join("horizon-server");
        std::fs::write(&exe, "").unwrap();
        // Not the release in use (or no installer layout): this program as it is.
        assert_eq!(through_current(&exe), exe);
        std::os::unix::fs::symlink(&release, root.join("current")).unwrap();
        assert_eq!(through_current(&exe), root.join("current/horizon-server"));
        let other = root.join("bin/horizon-server");
        assert_eq!(through_current(&other), other);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
