//! Horizon: native Rust client for Minecraft Java Edition 26.3 gameplay.
// Android's entry point has to be an unmangled symbol; everywhere else there is no unsafe code at all.
#![cfg_attr(not(target_os = "android"), forbid(unsafe_code))]
#[cfg(target_os = "android")]
mod android;
mod game;

use horizon_client::{crash, logging, ram_limit};

#[global_allocator]
static ALLOCATOR: horizon_threads::Allocator = horizon_threads::Allocator;

/// The mod API level this game serves, kept in the program for the launcher
/// to read without running it (`horizon_mods::api_level::MARK`). `main`
/// reads it, so the linker keeps it.
static MOD_API_LEVEL: [u8; 27] = horizon_mods::api_level::MARK;

/// Start the game where a phone lets it write.
///
/// iOS hands an app a working directory of `/`, which is read-only, while the game builds its world saves,
/// its `options.txt`, its `.horizon-cache` and its `resourcepacks/` from `current_dir()` -- so every one of
/// those is a path it cannot create. The writable place is the app's data container, and `$HOME` names it,
/// so the fix is one call made before anything reads a path: work in `Documents`. The launcher runs the game
/// inside its game home instead; a phone has no launcher, and `Documents` is the folder the Files app shows,
/// which is where a player drops a resource pack or a world.
///
/// Prepared resources are looked for by `Options::read`, which checks a `resources/` beside the
/// executable first and then the `resources/` that `horizon_client::import` unpacks into this very
/// folder -- so both the assets shipped in the bundle and the ones carried in by a player are found
/// with no argument to pass.
#[cfg(target_os = "ios")]
fn into_app_sandbox() -> Result<std::path::PathBuf, String> {
    let documents = std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(|home| std::path::PathBuf::from(home).join("Documents"))
        .ok_or("iOS gave this app no HOME")?;
    let describe = |error: std::io::Error| format!("{}: {error}", documents.display());
    std::fs::create_dir_all(&documents).map_err(describe)?;
    std::env::set_current_dir(&documents).map_err(describe)?;
    Ok(documents)
}

fn main() {
    horizon_threads::configure_allocator();
    // Before the event loop: iOS asks for the AirPlay scene's delegate by name when a display connects.
    horizon_airplay::register();
    horizon_airplay::audio::start();
    std::hint::black_box(&MOD_API_LEVEL);
    #[cfg(target_os = "ios")]
    let documents = match into_app_sandbox() {
        Ok(documents) => documents,
        Err(error) => {
            eprintln!("Horizon: {error}");
            std::process::exit(1);
        }
    };
    // `android_main` already moved into the app's files folder.
    #[cfg(target_os = "android")]
    let documents = std::env::current_dir().unwrap_or_default();
    horizon_client::telemetry::mark_start();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let game_args = args.iter().take_while(|a| *a != "--dedicated");
    let help = game_args
        .clone()
        .any(|a| a == "--help" || a == "-h" || a == "--launch-api");
    // --home before the log opens in it.
    if let Some(dir) = game_args.clone().skip_while(|a| *a != "--home").nth(1) {
        logging::set_game_home(std::path::absolute(dir).unwrap_or_else(|_| dir.into()));
    }
    // The dedicated server logs in its own folder (--game-dir's), as vanilla's does; the game in its home.
    let (home, side, thread) = if args.iter().any(|a| a == "--dedicated") {
        let folder = game_args
            .clone()
            .skip_while(|a| *a != "--game-dir" && *a != "--gameDir")
            .nth(1)
            .and_then(|dir| std::path::absolute(dir).ok())
            .unwrap_or_else(|| ".".into());
        (folder, "server", "Server thread")
    } else {
        // --player-dir too (a game's, not a server's): a player of a shared game folder keeps
        // its log and crash reports in its own; crash telemetry stays with the home's consent.
        if let Some(dir) = game_args
            .clone()
            .skip_while(|a| *a != "--player-dir")
            .nth(1)
        {
            logging::set_player_dir(std::path::absolute(dir).unwrap_or_else(|_| dir.into()));
        }
        (logging::game_home(), "client", "Render thread")
    };
    if !help {
        logging::init(
            logging::player_dir_override().unwrap_or(home.as_path()),
            thread,
        );
        // The log's first line names the build, so a bug report that carries the log says which one it was.
        log::info!("Horizon {}", crash::version());
        crash::install(home.clone(), side);
        ram_limit::start(&home, side);
        // The Microsoft account a last start kept (the title screen's face button), signed in again.
        #[cfg(not(target_os = "ios"))]
        if side == "client" {
            horizon_client::msa::restore(&home);
        }
    }
    // What a phone does with its files before the game looks for them, once the log can say so.
    #[cfg(any(target_os = "ios", target_os = "android"))]
    if !help {
        // A swap that a flat battery or a crash left half done is finished, or the old tree put back,
        // before anything reads the folder it was working on.
        match horizon_client::import::recover(&documents) {
            Ok(Some(path)) => log::info!("Horizon: finished the import at {}", path.display()),
            Ok(None) => {}
            Err(error) => log::error!("Horizon: could not finish the last import: {error}"),
        }
        // The cable's door: an archive dragged into `Documents` is installed here, so the launch it was
        // dragged for is the one that finds it. `scan` says what it took and what it left alone.
        horizon_client::import::scan(&documents);
    }
    raise_open_file_limit();
    if let Err(error) = game::run() {
        // A bad command line is the caller's mistake, not a crash: no report.
        let usage = error.is::<game::UsageError>();
        log::error!("Horizon: {error}");
        logging::flush();
        // The report's path is the last line on stderr (crash::SAVED_LINE).
        if !help && !usage {
            crash::report_error(&home, side, &*error);
        }
        // Started from Explorer, the console window closes with the game:
        // keep the message on screen until it is read.
        #[cfg(windows)]
        if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
            eprintln!("Press Enter to close.");
            let _ = std::io::stdin().read_line(&mut String::new());
        }
        std::process::exit(if usage {
            game::exit_code::USAGE
        } else {
            game::exit_code::ERROR
        });
    }
    logging::flush();
}

/// The JVM raises its open-file limit to the hard limit as it starts
/// (`-XX:+MaxFDLimit`, on by default), so a vanilla server may hold as many
/// sockets and region files as the host allows, not the soft default (often
/// 1024 in a container). The same here; macOS caps it at `OPEN_MAX`, as the
/// JVM does there.
fn raise_open_file_limit() {
    #[cfg(unix)]
    {
        use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};
        let limit = getrlimit(Resource::Nofile);
        let hard = if cfg!(target_os = "macos") {
            Some(limit.maximum.map_or(10_240, |h| h.min(10_240)))
        } else {
            limit.maximum
        };
        let raised = match (limit.current, hard) {
            (Some(soft), Some(hard)) => hard > soft,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if raised {
            let to = Rlimit {
                current: hard,
                maximum: limit.maximum,
            };
            match setrlimit(Resource::Nofile, to) {
                Ok(()) => log::info!(
                    "Open file limit raised from {:?} to {hard:?}",
                    limit.current
                ),
                Err(e) => log::warn!("Open file limit stays at {:?}: {e}", limit.current),
            }
        }
    }
}
