//! Offline data relocation: the window exits before the worker takes the migration lock.
use crate::{
    backend,
    data_location::{self, Roots},
    process,
};
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

fn remap(value: &mut Value, source: &Roots, target: &Roots) {
    let Some(text) = value.as_str() else { return };
    let path = Path::new(text);
    // The build directory can be nested in the launcher directory: the most specific wins.
    let mut pairs = [
        (&source.launcher, &target.launcher),
        (&source.game, &target.game),
        (&source.build, &target.build),
    ];
    pairs.sort_by_key(|(old, _)| std::cmp::Reverse(old.components().count()));
    for (old, new) in pairs {
        #[cfg(windows)]
        let matches = Path::new(&path.to_string_lossy().to_lowercase())
            .starts_with(old.to_string_lossy().to_lowercase());
        #[cfg(not(windows))]
        let matches = path.starts_with(old);
        if matches {
            let rest: PathBuf = path.components().skip(old.components().count()).collect();
            *value = Value::String(new.join(rest).to_string_lossy().into_owned());
            return;
        }
    }
}

/// Only fields whose schema denotes a filesystem path; never commands, URLs or arbitrary text.
pub fn prepare_paths(stage: &Roots, source: &Roots, target: &Roots) -> Result<(), String> {
    for name in ["settings.json", "servers.json", "instances.json"] {
        let path = stage.launcher.join(name);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let mut value: Value =
            serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        match name {
            "settings.json" => {
                for key in ["minecraft_dir", "server_dir", "background"] {
                    if let Some(v) = value.get_mut(key) {
                        remap(v, source, target);
                    }
                }
            }
            "servers.json" => {
                if let Some(entries) = value.as_array_mut() {
                    for entry in entries {
                        if let Some(v) = entry.get_mut("dir") {
                            remap(v, source, target);
                        }
                    }
                }
            }
            "instances.json" => {
                if let Some(entries) = value.get_mut("instances").and_then(Value::as_object_mut) {
                    for entry in entries.values_mut() {
                        if let Some(v) = entry.get_mut("java").and_then(|j| j.get_mut("home")) {
                            remap(v, source, target);
                        }
                    }
                }
            }
            _ => unreachable!(),
        }
        let bytes = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

/// Refuse live games/build tools even if the window that started them has already exited.
fn ensure_idle(source: &Roots) -> Result<(), String> {
    #[cfg(windows)]
    let output = process::command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", "Get-CimInstance Win32_Process | Select-Object Name,ExecutablePath,CommandLine | ConvertTo-Json -Compress"])
        .output();
    #[cfg(not(windows))]
    let output = process::command("ps")
        .args(["-A", "-o", "comm=,args="])
        .output();
    let output = output.map_err(|e| format!("Could not check running games and builds: {e}"))?;
    if !output.status.success() {
        return Err("Could not check running games and builds; no files were moved.".into());
    }
    let roots: Vec<_> = [&source.launcher, &source.game, &source.build]
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/").to_lowercase())
        .collect();
    #[cfg(windows)]
    let processes: Vec<(String, String)> = {
        let value: Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| format!("Could not read running processes: {e}"))?;
        let entries = value.as_array().cloned().unwrap_or_else(|| vec![value]);
        entries
            .into_iter()
            .map(|p| {
                (
                    p["Name"].as_str().unwrap_or("").to_owned(),
                    format!(
                        "{} {}",
                        p["ExecutablePath"].as_str().unwrap_or(""),
                        p["CommandLine"].as_str().unwrap_or("")
                    ),
                )
            })
            .collect()
    };
    #[cfg(not(windows))]
    let processes: Vec<(String, String)> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().split_once(char::is_whitespace))
        .map(|(name, command)| (name.to_owned(), command.to_owned()))
        .collect();
    for (name, command) in processes {
        let name = Path::new(&name)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        let name = name.strip_suffix(".exe").unwrap_or(&name);
        let command = command.replace('\\', "/").to_lowercase();
        let managed = roots.iter().any(|root| command.contains(root));
        if matches!(
            name,
            "horizon-client" | "horizon-server" | "horizon-prepare"
        ) || (managed
            && matches!(
                name,
                "java" | "javaw" | "cargo" | "rustc" | "horizon-mixins"
            ))
        {
            return Err(format!(
                "Close games, servers and native builds before moving data ({name} is running)."
            ));
        }
    }
    Ok(())
}

/// Handled before normal startup, so this process owns no shared data-location lease.
pub fn run_worker() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--relocate-data")) {
        return None;
    }
    let target = args.next().map(PathBuf::from);
    let restart = args.next().is_some_and(|a| a == "--restart");
    let result = (|| {
        let target = target.ok_or("--relocate-data needs an empty destination folder")?;
        data_location::validate()?;
        let source = Roots {
            launcher: backend::data_dir(),
            game: backend::game_home(),
            build: backend::build_dir(),
        };
        ensure_idle(&source)?;
        let backup = format!(
            "{}\n{}\n{}",
            source.launcher.display(),
            source.game.display(),
            source.build.display()
        );
        let destination = data_location::migrate_current_with_prepare(
            &source,
            &target,
            |message| eprintln!("{message}"),
            |stage, target| {
                ensure_idle(&source)?;
                prepare_paths(stage, &source, target)
            },
        )?;
        println!("{}", destination.launcher.display());
        Ok::<_, String>(backup)
    })();
    if let Err(error) = &result {
        eprintln!("Data was not relocated: {error}");
    }
    let code = i32::from(result.is_err());
    if restart {
        let mut command = process::command(backend::exe_path());
        command
            .env_remove("HORIZON_RELOCATION_ERROR")
            .env_remove("HORIZON_RELOCATION_BACKUP");
        match &result {
            Ok(backup) => {
                command.env("HORIZON_RELOCATION_BACKUP", backup);
            }
            Err(error) => {
                command.env("HORIZON_RELOCATION_ERROR", error);
            }
        }
        if let Err(e) = command.spawn() {
            eprintln!("Could not reopen the launcher: {e}");
            return Some(1);
        }
    }
    Some(code)
}
