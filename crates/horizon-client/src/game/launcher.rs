//! The phone's launcher (packaging/ios/Launcher), answered: it speaks the control socket's language through
//! `horizon_ios_launcher`'s queue, so `worlds`, `open`, `connect`, `options-set` and the rest mean what they
//! mean to horizon-ctl. Four lines are its own: `launcher-start`, which lets the game load (it waits for the
//! player to start it); `launcher-state`, which says whether the game waits to be started, is loading, waits
//! for files or is at its title; `import PATH`, which installs an archive the launcher downloaded; and
//! `launcher-world {json}`, Select World's actions for folders with spaces in their names.
//!
//! The loading window answers the start, the state and imports before there is a `Game` to ask; anything else
//! waits for the title.
use super::event_loop::EventLoopCtl;
use super::{Game, Loading};
use horizon_client::{
    Result,
    import::{Installed, Kind, Pack},
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    sync::mpsc::{self, Receiver, TryRecvError},
};

/// The portrait launcher and landscape game share a UIWindow. Permit the
/// launcher's orientation before acknowledging its presentation request.
fn launcher_visibility(window: Option<&winit::window::Window>, visible: bool) -> bool {
    if !visible {
        if horizon_ios_launcher::cancellation_requested() {
            return false;
        }
        horizon_ios_launcher::commit_start();
    }
    horizon_ios_launcher::set_covered(visible);
    #[cfg(target_os = "ios")]
    if let Some(window) = window {
        use winit::platform::ios::{ValidOrientations, WindowExtIOS};
        window.set_valid_orientations(if visible {
            ValidOrientations::LandscapeAndPortrait
        } else {
            ValidOrientations::Landscape
        });
    }
    #[cfg(not(target_os = "ios"))]
    let _ = window;
    true
}

/// What came back for a launcher request so far: a control answer, or an import's result.
enum Waiting {
    Control(Receiver<String>),
    Import(Receiver<std::result::Result<Installed, String>>),
    CancelStart(String),
}

thread_local! {
    /// Launcher requests whose answers have not come back yet. Only the main thread touches them.
    static PENDING: RefCell<Vec<(Waiting, horizon_ios_launcher::Request)>> = const { RefCell::new(Vec::new()) };
}

/// Answer what has come back and keep waiting on the rest. True when an import brought game files.
fn deliver() -> bool {
    let mut resources = false;
    let pending = PENDING.with_borrow_mut(std::mem::take);
    let mut still = Vec::new();
    for (waiting, request) in pending {
        match waiting {
            Waiting::CancelStart(owner) => still.push((Waiting::CancelStart(owner), request)),
            Waiting::Control(answers) => match answers.try_recv() {
                Ok(answer) => request.answer(&answer),
                // Dropping the request answers it with an error.
                Err(TryRecvError::Disconnected) => {}
                Err(TryRecvError::Empty) => still.push((Waiting::Control(answers), request)),
            },
            Waiting::Import(result) => match result.try_recv() {
                Ok(Ok(installed)) => {
                    resources |= installed.kind == Kind::Resources;
                    request.answer(&installed_json(&installed).to_string());
                }
                Ok(Err(error)) => request.answer(&json!({ "error": error }).to_string()),
                Err(TryRecvError::Disconnected) => {}
                Err(TryRecvError::Empty) => still.push((Waiting::Import(result), request)),
            },
        }
    }
    PENDING.with_borrow_mut(|pending| pending.extend(still));
    resources
}

/// Complete cancellation only after the supervisor has dropped the old engine.
pub(super) fn acknowledge_cancelled_start(owner: &str) {
    let pending = PENDING.with_borrow_mut(std::mem::take);
    let mut still = Vec::new();
    for (waiting, request) in pending {
        if matches!(&waiting, Waiting::CancelStart(id) if id == owner) {
            request.answer(r#"{"ok":true,"status":"stopped"}"#);
        } else {
            still.push((waiting, request));
        }
    }
    PENDING.with_borrow_mut(|pending| pending.extend(still));
}

fn start_owner(line: &str) -> Option<&str> {
    line.split_whitespace().nth(1)
}

fn allowed_while_stopping(command: &str) -> bool {
    matches!(
        command,
        "launcher-state"
            | "launcher-visible"
            | "launcher-cancel-start"
            | "launcher-session"
            | "download-state"
            | "game-files-source"
            | "options-get"
            | "worlds"
            | "servers"
            | "server-ping"
            | "import"
            | "import-game-files"
    )
}

fn commit_start(request: horizon_ios_launcher::Request) {
    let committed =
        start_owner(&request.line).is_some_and(horizon_ios_launcher::commit_start_owned);
    request.answer(
        &json!({ "ok": committed, "status": if committed { "committed" } else { "stale" } })
            .to_string(),
    );
}

fn cancel_start(request: horizon_ios_launcher::Request) {
    let Some(owner) = start_owner(&request.line).map(str::to_owned) else {
        request.answer(r#"{"error":"a launch identity is required"}"#);
        return;
    };
    // A visible game has already passed the native presentation commit point.
    if !horizon_ios_launcher::covered() {
        horizon_ios_launcher::commit_start();
    }
    match horizon_ios_launcher::request_cancel(&owner) {
        horizon_ios_launcher::CancelDecision::Accepted => {
            PENDING.with_borrow_mut(|pending| {
                pending.push((Waiting::CancelStart(owner), request));
            });
        }
        horizon_ios_launcher::CancelDecision::Stale => {
            request.answer(r#"{"ok":true,"status":"stale"}"#);
        }
        horizon_ios_launcher::CancelDecision::Committed => {
            request.answer(r#"{"ok":true,"status":"committed"}"#);
        }
    }
}

/// A native import owns its destination until its completion is delivered.
fn importing_archive() -> bool {
    PENDING.with_borrow(|pending| {
        pending
            .iter()
            .any(|(waiting, _)| matches!(waiting, Waiting::Import(_)))
    })
}

/// Install the archive named by `import PATH` on a worker: a prepared tree is hundreds of megabytes.
fn start_import(request: horizon_ios_launcher::Request) {
    if importing_archive() {
        request.answer(r#"{"error":"Wait for the current import to finish."}"#);
        return;
    }
    let command = request.line.split_whitespace().next().unwrap_or_default();
    let resources_only = command == "import-game-files";
    let path = request
        .line
        .trim_start()
        .strip_prefix(command)
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(std::path::PathBuf::from);
    let Some(path) = path else {
        request.answer(r#"{"error":"import PATH: the archive to install"}"#);
        return;
    };
    let home = horizon_client::import::home();
    let (sender, receiver) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("horizon-import".into())
        .spawn(move || {
            let installed =
                install_archive(&path, &home, resources_only).map_err(|e| e.to_string());
            let _ = sender.send(installed);
        });
    match spawned {
        Ok(_) => PENDING.with_borrow_mut(|p| p.push((Waiting::Import(receiver), request))),
        Err(error) => request.answer(&json!({ "error": error.to_string() }).to_string()),
    }
}

fn install_archive(
    path: &std::path::Path,
    home: &std::path::Path,
    resources_only: bool,
) -> Result<Installed> {
    let pack = Pack::open(path)?;
    if resources_only && pack.kind != Kind::Resources {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "The automatic download did not contain game files.",
        )
        .into());
    }
    pack.install(home)
}

fn installed_json(installed: &Installed) -> Value {
    json!({
        "ok": true,
        "kind": match installed.kind {
            Kind::Resources => "resources",
            Kind::World => "world",
            Kind::ResourcePack => "resource_pack",
        },
        "name": installed.path.file_name().map(|n| n.to_string_lossy().into_owned()),
        "files": installed.files,
        "bytes": installed.bytes,
    })
}

impl Game<'_> {
    /// The launcher's requests, once a frame: its own lines here, the rest through the control socket's
    /// dispatcher, as if horizon-ctl had sent them.
    pub(super) fn poll_launcher(&mut self, event_loop: &dyn EventLoopCtl) {
        for request in horizon_ios_launcher::take() {
            let command = request.line.split_whitespace().next().unwrap_or_default();
            if self.quit_to_launcher && !allowed_while_stopping(command) {
                request.answer(r#"{"error":"The game is stopping. Try again shortly."}"#);
                continue;
            }
            match command {
                "launcher-visible" => {
                    let accepted = launcher_visibility(
                        self.window.as_deref(),
                        request.line.ends_with(" true"),
                    );
                    request.answer(&json!({ "ok": accepted }).to_string());
                }

                "launcher-state" => {
                    let state = json!({
                        "phase": if self.title.is_some() { "title" } else { "world" },
                        "screen": self.screen_name(),
                        "player": self.options.player_name(),
                        "version": horizon_core::version::VERSION_ID,
                    });
                    request.answer(&state.to_string());
                }
                "import" | "import-game-files" => start_import(request),
                // The game is up: the download screen has nothing left to show.
                "download-state" => request.answer(r#"{"phase":"done"}"#),
                "launcher-start" => {
                    let accepted = !self.quit_to_launcher
                        && horizon_ios_launcher::start_owned(start_owner(&request.line));
                    request.answer(
                        &json!({
                            "ok": accepted,
                            "phase": if self.title.is_some() { "title" } else { "world" },
                        })
                        .to_string(),
                    );
                }
                "launcher-commit-start" => commit_start(request),
                "launcher-cancel-start" => {
                    // World entry is a committed action. A stale native task
                    // must not tear down an already opened world or server.
                    if self.title.is_none() {
                        horizon_ios_launcher::commit_start();
                    }
                    cancel_start(request);
                    if horizon_ios_launcher::cancellation_requested() {
                        self.quit_to_launcher = true;
                    }
                }
                "launcher-session" => session_request(request),
                "launcher-world" => self.launcher_world(request),
                _ => {
                    let (reply, answers) = mpsc::channel();
                    let line = request.line.clone();
                    self.answer(horizon_client::control::Request { line, reply }, event_loop);
                    PENDING.with_borrow_mut(|p| p.push((Waiting::Control(answers), request)));
                }
            }
        }
        // Game files that arrive while the game runs are read on its next start.
        deliver();
    }

    /// `launcher-world`: Select World's Play by folder here; the rest is files (`world_files`).
    fn launcher_world(&mut self, request: horizon_ios_launcher::Request) {
        let Some(request) = world_files(request) else {
            // Renamed or deleted: Select World reads the saves again.
            if let Some(title) = self.title.as_mut() {
                title.worlds_changed();
            }
            return;
        };
        // Only Play is left: it needs the title screen.
        let args = world_args(&request.line);
        let folder = args["folder"].as_str().unwrap_or_default().to_owned();
        let answer = if self.title.is_none() {
            json!({ "error": "a world is already open" })
        } else {
            match self.open_world(&folder, None, None, false, None) {
                Ok(()) => json!({ "ok": true }),
                Err(e) => json!({ "error": e.to_string() }),
            }
        };
        request.answer(&answer.to_string());
    }
}

/// `launcher-world {"action": "open"|"rename"|"backup"|"delete", "folder": ..., "name": ...}`'s arguments: Select
/// World and Edit World by folder, for folders the control socket's word-split lines cannot name ("New World").
fn world_args(line: &str) -> Value {
    line.trim_start()
        .strip_prefix("launcher-world")
        .and_then(|rest| serde_json::from_str(rest).ok())
        .unwrap_or_default()
}

/// Rename, Back Up and Delete, which are files and need no game; the request back for Play, which does.
fn world_files(request: horizon_ios_launcher::Request) -> Option<horizon_ios_launcher::Request> {
    let args = world_args(&request.line);
    let (Some(action), Some(folder)) = (args["action"].as_str(), args["folder"].as_str()) else {
        request.answer(r#"{"error":"launcher-world {\"action\", \"folder\"}"}"#);
        return None;
    };
    let saves = std::env::current_dir().unwrap_or_default().join("saves");
    let done = match action {
        "open" => return Some(request),
        "rename" => {
            let name = args["name"].as_str().unwrap_or_default().trim();
            if name.is_empty() {
                Err("a world needs a name".into())
            } else {
                horizon_client::title::world_management::rename(&saves, folder, name)
            }
        }
        "delete" => horizon_client::title::world_management::delete(&saves, folder),
        "backup" => {
            // A big world takes a while to zip: off the main thread, as Make Backup does.
            let (reply, answers) = mpsc::channel();
            let folder = folder.to_owned();
            std::thread::spawn(move || {
                let value = match horizon_client::title::world_management::backup(&saves, &folder) {
                    Ok(bytes) => json!({ "ok": true, "bytes": bytes }),
                    Err(e) => json!({ "error": format!("backup of {folder} failed: {e}") }),
                };
                let _ = reply.send(value.to_string());
            });
            PENDING.with_borrow_mut(|p| p.push((Waiting::Control(answers), request)));
            return None;
        }
        _ => Err(format!("launcher-world: no action {action}").into()),
    };
    let answer = match done {
        Ok(()) => json!({ "ok": true }),
        Err(e) => json!({ "error": e.to_string() }),
    };
    request.answer(&answer.to_string());
    None
}

/// `launcher-session {"name", "uuid", "access_token", "user_type": "msa", "xuid"}`: the Microsoft account the
/// phone's launcher signed in (the session file's fields, docs/microsoft-account.md), kept in memory only;
/// `launcher-session off` plays offline again. The token is never echoed back.
fn session_request(request: horizon_ios_launcher::Request) {
    let rest = request
        .line
        .trim_start()
        .strip_prefix("launcher-session")
        .map(str::trim)
        .unwrap_or_default();
    let answer = if rest == "off" {
        horizon_client::auth::clear_current();
        json!({ "ok": true })
    } else {
        match horizon_client::auth::AuthSession::parse(rest) {
            Ok(session) => {
                let name = session.name.clone();
                horizon_client::auth::set_current(session);
                json!({ "ok": true, "name": name })
            }
            Err(e) => json!({ "error": e }),
        }
    };
    request.answer(&answer.to_string());
}

/// The control lines that are only files (worlds, servers, pings: `saves_control::file_command`), answered
/// without a game; the request back for anything else.
fn file_request(request: horizon_ios_launcher::Request) -> Option<horizon_ios_launcher::Request> {
    let words: Vec<&str> = request.line.split_whitespace().collect();
    let (reply, answers) = mpsc::channel();
    match super::saves_control::file_command(&words, &request.line, &reply) {
        Ok(None) => Some(request),
        Ok(Some(Some(value))) => {
            request.answer(&value.to_string());
            None
        }
        // Answered later, from a worker.
        Ok(Some(None)) => {
            PENDING.with_borrow_mut(|p| p.push((Waiting::Control(answers), request)));
            None
        }
        Err(e) => {
            request.answer(&json!({ "error": e.to_string() }).to_string());
            None
        }
    }
}

impl Loading {
    /// `options-get KEY...` and `options-set KEY VALUE` before the game exists, on the settings it will be
    /// made with, saved to options.txt as the options screen saves them (game/options.rs does the same in a
    /// running game, where a change also applies at once).
    fn launcher_options(&mut self, line: &str) -> Value {
        let words: Vec<&str> = line.split_whitespace().collect();
        let mut file = self.settings.to_file();
        if words[0] == "options-get" {
            let mut options = serde_json::Map::new();
            for key in &words[1..] {
                match file.describe(key) {
                    Some(described) => options.insert((*key).to_owned(), described),
                    None => return json!({ "error": format!("unknown option {key}") }),
                };
            }
            return json!({ "options": options });
        }
        let Some(key) = words.get(1).copied() else {
            return json!({ "error": "options-set KEY VALUE" });
        };
        let value = line
            .trim_start()
            .strip_prefix("options-set")
            .and_then(|rest| rest.trim_start().strip_prefix(key))
            .map(str::trim)
            .unwrap_or_default();
        if let Err(e) = file.set_text(key, value) {
            return json!({ "error": e });
        }
        let mut settings = horizon_client::settings::Settings::from_file(file);
        // Horizon's own files are not in options.txt.
        std::mem::swap(&mut settings.horizon, &mut self.settings.horizon);
        std::mem::swap(&mut settings.controller, &mut self.settings.controller);
        std::mem::swap(&mut settings.extras, &mut self.settings.extras);
        self.settings = settings;
        self.settings.save(
            &std::env::current_dir()
                .unwrap_or_default()
                .join("options.txt"),
        );
        json!({ "ok": true, "key": key, "option": self.settings.to_file().describe(key) })
    }

    /// The launcher's requests before the game exists: the start, its state, imports (how a phone with no game
    /// files gets them from the launcher), the options, and what is only files: worlds, servers, pings.
    /// Game files that arrive end the wait for them.
    pub(super) fn poll_launcher(&mut self) -> Result<()> {
        for request in horizon_ios_launcher::take() {
            let command = request.line.split_whitespace().next().unwrap_or_default();
            if horizon_ios_launcher::cancellation_requested() && !allowed_while_stopping(command) {
                request.answer(r#"{"error":"The game is stopping. Try again shortly."}"#);
                continue;
            }
            match command {
                "launcher-visible" => {
                    let accepted = launcher_visibility(
                        self.window.as_deref(),
                        request.line.ends_with(" true"),
                    );
                    request.answer(&json!({ "ok": accepted }).to_string());
                }

                "game-files-source" => request.answer(
                    &json!({
                        "url": horizon_client::import::download::url(horizon_core::VANILLA_VERSION),
                        "version": horizon_core::VANILLA_VERSION,
                    })
                    .to_string(),
                ),
                "launcher-start" => {
                    if importing_archive() {
                        request.answer(r#"{"error":"Wait for the current import to finish."}"#);
                    } else {
                        if horizon_ios_launcher::waiting() {
                            self.refresh_import_root(&horizon_client::import::home())?;
                        }
                        let accepted =
                            horizon_ios_launcher::start_owned(start_owner(&request.line));
                        request.answer(&json!({ "ok": accepted, "phase": "loading" }).to_string());
                    }
                }
                "launcher-commit-start" => commit_start(request),
                "launcher-cancel-start" => cancel_start(request),
                "launcher-state" => {
                    let state = if horizon_ios_launcher::waiting() {
                        json!({ "phase": "waiting" })
                    } else if self.importing() {
                        json!({ "phase": "needs_files", "reason": self.import_reason() })
                    } else {
                        json!({ "phase": "loading", "progress": self.progress.fraction() })
                    };
                    request.answer(&state.to_string());
                }
                "download-state" => request.answer(&self.download_state().to_string()),
                "download-retry" => {
                    let answer = match self.retry_download() {
                        Ok(()) => json!({ "ok": true }),
                        Err(e) => json!({ "error": e.to_string() }),
                    };
                    request.answer(&answer.to_string());
                }
                "import" | "import-game-files" => start_import(request),
                "launcher-session" => session_request(request),
                "options-get" | "options-set" => {
                    let answer = self.launcher_options(&request.line);
                    request.answer(&answer.to_string());
                }
                "shader-pack" => {
                    let words: Vec<&str> = request.line.split_whitespace().collect();
                    let answer = match super::shader_packs::command(
                        &mut self.settings,
                        &words,
                        &request.line,
                    ) {
                        Ok(value) => value.unwrap_or_default(),
                        Err(e) => json!({ "error": e.to_string() }),
                    };
                    request.answer(&answer.to_string());
                }
                "launcher-world" => {
                    if let Some(request) = world_files(request) {
                        request.answer(
                            &json!({ "error": "start the game to play a world", "starting": true })
                                .to_string(),
                        );
                    }
                }
                _ => {
                    if let Some(request) = file_request(request) {
                        request.answer(
                            &json!({ "error": "the game is still starting", "starting": true })
                                .to_string(),
                        );
                    }
                }
            }
        }
        if deliver() && self.importing() && !horizon_ios_launcher::cancellation_requested() {
            self.files_arrived()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn automatic_import_rejects_world_before_writing_but_manual_import_allows_it() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "horizon-auto-import-{}-{nonce}",
            std::process::id()
        ));
        let world = root.join("world");
        std::fs::create_dir_all(&world).unwrap();
        std::fs::write(world.join("level.dat"), b"synthetic world fixture").unwrap();
        let archive = root.join("world.zip");
        horizon_client::import::pack(&world, &archive, |_, _| {}).unwrap();
        let home = root.join("home");
        let error = super::install_archive(&archive, &home, true).unwrap_err();
        assert!(error.to_string().contains("did not contain game files"));
        assert!(
            !home.exists(),
            "Wrong-kind download must not touch player data"
        );
        assert!(archive.exists());
        let installed = super::install_archive(&archive, &home, false).unwrap();
        assert_eq!(installed.kind, horizon_client::import::Kind::World);
        assert!(installed.path.join("level.dat").is_file());
        std::fs::remove_dir_all(root).unwrap();
    }
}
