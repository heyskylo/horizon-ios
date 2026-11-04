//! The screen a phone shows when it has no game files, and the wait for someone to bring them.
//!
//! `Loading` usually ends one of two ways: a game, or an exit. On a phone the second is worse than
//! the first sounds — the app vanishes and nothing says why, and there is no terminal in the
//! sandbox to read the log in. So a phone that found nothing to load keeps its window, says what it
//! is missing and how to send it, and starts the loader again the moment the files arrive. That
//! restart is the same one a resource-pack change already makes (`restart_menu_resources`), which is
//! why nothing here needs new machinery in the loader itself.
//!
//! Everything drawn is drawn from `horizon_client::import::screen`, with the one atlas that exists
//! before Mojang's files do.
use super::*;

/// The wait: the listener that takes an archive, the atlas the screen draws with, and the words it
/// says.
pub(super) struct Import {
    /// `None` when the port could not be opened, which leaves the cable route and the folder path
    /// on the screen. The screen never promises a page nobody is serving.
    session: Option<horizon_client::import::http::Session>,
    atlas: HudAtlas,
    /// Where an archive has to arrive: the folder the Files app shows.
    home: std::path::PathBuf,
    /// The loader's own words about why there is nothing to show.
    reason: String,
    /// The last upload that was refused, and why.
    error: Option<String>,
    received: u64,
    total: u64,
    /// The game files coming from the download server (`import::download`): the first door, tried
    /// before the screen asks anyone to send them. `None` once it is over or failed.
    download: Option<horizon_client::import::download::Download>,
}

/// Whether this run has downloaded the game files already.
static DOWNLOADED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl Loading {
    /// Stop waiting for a game that cannot start and wait for files instead. The caller has already
    /// said on screen that this is what a phone does (`import::waits_on_screen`).
    pub(super) fn wait_for_files(&mut self, reason: String) {
        let home = horizon_client::import::home();
        let atlas = match HudAtlas::without_pack(&home) {
            Ok(atlas) => atlas,
            Err(error) => {
                // Without even the synthetic atlas there is nothing to draw with, and an exit with
                // the reason in the log is all that is left.
                log::error!("Horizon: the import screen has no font to draw with: {error}");
                self.error = Some(error);
                return;
            }
        };
        horizon_client::ui_theme::reset_atlas();
        // Download them first; the Wi-Fi and cable doors open only if that fails. Once a run: files that
        // downloaded and still don't load would otherwise be downloaded again and again.
        let url = horizon_client::import::download::url(horizon_core::VANILLA_VERSION);
        let mut error = None;
        let download = if DOWNLOADED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            log::warn!(
                "Horizon: the downloaded game files did not load ({reason}); not downloading again"
            );
            error = Some(format!(
                "The game files downloaded but did not load: {reason}"
            ));
            None
        } else {
            match horizon_client::import::download::Download::start(url.clone(), home.clone()) {
                Ok(download) => {
                    log::info!("Horizon: no game files ({reason}); downloading {url}");
                    Some(download)
                }
                Err(error) => {
                    log::warn!("Horizon: could not start downloading the game files: {error}");
                    None
                }
            }
        };
        let session = if download.is_some() {
            // On a phone the download shows in SwiftUI over this screen; it asks `download-state`.
            horizon_ios_launcher::present_download();
            None
        } else {
            open_session(&home)
        };
        log::warn!(
            "Horizon: no game files to load ({reason}); waiting for an import in {}",
            home.display()
        );
        self.import = Some(Import {
            session,
            atlas,
            home,
            reason,
            error,
            received: 0,
            total: 0,
            download,
        });
    }

    /// Whether a Linux game with nothing to load downloads its files instead of exiting: when nobody
    /// named a `--resources` folder and this is a player's run, not a test's (offscreen or counted
    /// frames), which must fail fast rather than fetch hundreds of megabytes.
    pub(super) fn downloads_game_files(&self) -> bool {
        cfg!(target_os = "linux")
            && self
                .options
                .as_ref()
                .is_some_and(|o| !o.resources_given && !o.offscreen && o.frames.is_none())
    }

    /// Whether this window is waiting for files, which is when the loader must be left alone: its
    /// channel is closed, and reading it would look like a new failure every frame.
    pub(super) fn importing(&self) -> bool {
        self.import.is_some()
    }

    /// Take whatever the listener has said. An archive that turned into game files ends the wait.
    pub(super) fn poll_import(&mut self) -> Result<()> {
        let Some(import) = &mut self.import else {
            return Ok(());
        };
        let mut ready = false;
        if let Some(result) = import.download.as_ref().and_then(|d| d.poll()) {
            import.download = None;
            match result {
                Ok(placed) => {
                    log::info!(
                        "Horizon: downloaded {} ({} files) to {}",
                        placed.kind.label(),
                        placed.files,
                        placed.path.display()
                    );
                    ready |= placed.kind == horizon_client::import::Kind::Resources;
                    if !ready {
                        import.error =
                            Some("The download was not game files. Send them another way.".into());
                    }
                }
                Err(error) => {
                    log::warn!("Horizon: the game files did not download: {error}");
                    import.error = Some(format!("The game files did not download: {error}"));
                }
            }
            if !ready {
                import.session = open_session(&import.home);
            }
        }
        if let Some(session) = &mut import.session {
            for news in session.poll() {
                match news {
                    horizon_client::import::http::News::Progress { received, total } => {
                        import.received = received;
                        import.total = total;
                    }
                    horizon_client::import::http::News::Installed(placed) => {
                        log::info!(
                            "Horizon: imported {} ({} files) to {}",
                            placed.kind.label(),
                            placed.files,
                            placed.path.display()
                        );
                        // A world or a pack is in place already; the title screen lists it as soon
                        // as there is a title screen. The game files are the only thing that can
                        // start the game.
                        ready |= placed.kind == horizon_client::import::Kind::Resources;
                    }
                    horizon_client::import::http::News::Refused { error } => {
                        import.error = Some(error);
                    }
                }
            }
        }
        if ready {
            return self.files_arrived();
        }
        Ok(())
    }

    /// Game files are in place (over Wi-Fi, or downloaded by the phone's launcher): stop waiting and load them.
    pub(super) fn files_arrived(&mut self) -> Result<()> {
        // Dropping the listener closes the port before the game starts looking at anything.
        let home = self
            .import
            .take()
            .map(|import| import.home)
            .unwrap_or_default();
        self.load_the_files_that_arrived(home)
    }

    /// The download, as the phone's SwiftUI download screen reads it (`download-state`): what it is doing and how
    /// far along, `failed` with why, or `done` once there is nothing left to wait for.
    pub(super) fn download_state(&self) -> serde_json::Value {
        use horizon_client::import::download::Stage;
        let Some(import) = &self.import else {
            return serde_json::json!({ "phase": "done", "progress": self.progress.fraction() });
        };
        match import.download.as_ref().map(|d| d.stage()) {
            Some(Stage::Downloading { received, total }) => serde_json::json!({
                "phase": "downloading",
                "received": received,
                "total": total,
                "version": horizon_core::VANILLA_VERSION,
            }),
            Some(Stage::Installing) => serde_json::json!({ "phase": "installing" }),
            None => serde_json::json!({
                "phase": "failed",
                "error": import.error.as_deref().unwrap_or(&import.reason),
            }),
        }
    }

    /// Try the download again after it failed (`download-retry`): it goes on from what arrived the last time.
    pub(super) fn retry_download(&mut self) -> Result<()> {
        let Some(import) = &mut self.import else {
            return Ok(());
        };
        if import.download.is_some() {
            return Ok(());
        }
        let url = horizon_client::import::download::url(horizon_core::VANILLA_VERSION);
        import.download = Some(horizon_client::import::download::Download::start(
            url,
            import.home.clone(),
        )?);
        // The other doors close while it runs, as they were before the first try.
        import.session = None;
        import.error = None;
        Ok(())
    }

    /// Why the loader found nothing to start, as the screen says it.
    pub(super) fn import_reason(&self) -> Option<&str> {
        self.import.as_ref().map(|import| import.reason.as_str())
    }

    /// Resolve newly installed files before a loader captures its vanilla base.
    /// An existing explicit base wins; a staged pack overlay is never the base.
    pub(super) fn refresh_import_root(&mut self, home: &std::path::Path) -> Result<()> {
        let options = self.options.as_mut().ok_or("loading already finished")?;
        let root = horizon_client::import::resources_root(&self.vanilla_root, Some(home))
            // A concurrent publish briefly has only resources.previous. Keep
            // the intended import destination; the worker's lease waits for
            // publication before it reads, without blocking the frame thread.
            .or_else(|| home.is_dir().then(|| home.join("resources")));
        if let Some(root) = root {
            options.resources = root.clone();
            self.vanilla_root = root;
        }
        Ok(())
    }

    /// Start the loader again on the files that arrived, as if the first attempt had found them.
    fn load_the_files_that_arrived(&mut self, home: std::path::PathBuf) -> Result<()> {
        self.loaded = None;
        self.delivered = false;
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        self.progress = Progress::new();
        self.refresh_import_root(&home)?;
        let options = self.options.take().ok_or("loading already finished")?;
        let spawned = startup_menu::spawn_loader(
            &options,
            &self.settings,
            &self.vanilla_root,
            self.started,
            &self.progress,
        );
        // A failed thread spawn must not consume the options needed by retry
        // or by a native cancellation returning to the idle launcher.
        self.options = Some(options);
        let (receiver, handle) = spawned?;
        self.receiver = receiver;
        self.handle = Some(handle);
        log::info!("Horizon: loading the imported game files");
        Ok(())
    }

    /// One frame of the wait.
    pub(super) fn draw_import(&self, batch: &mut HudBatch) {
        let Some(import) = &self.import else { return };
        if let Some(download) = &import.download {
            let (line, fraction) = horizon_client::import::screen::download_line(&download.stage());
            horizon_client::import::screen::draw_download(
                batch,
                &import.atlas,
                horizon_core::VANILLA_VERSION,
                &line,
                fraction,
            );
            return;
        }
        let address = import
            .session
            .as_ref()
            .and_then(|session| session.address().map(|host| (host, session.port())));
        horizon_client::import::screen::draw(
            batch,
            &import.atlas,
            &horizon_client::import::screen::Status {
                version: horizon_core::VANILLA_VERSION,
                reason: Some(&import.reason),
                folder: &import.home,
                address,
                pin: import.session.as_ref().map(|session| session.pin()),
                received: import.received,
                total: import.total,
                error: import.error.as_deref(),
            },
        );
    }
}

/// The Wi-Fi door: a listener for an upload. Its address goes to the log too: a phone plugged into a Mac
/// gets its Documents folder, and a headless run has no screen at all.
fn open_session(home: &std::path::Path) -> Option<horizon_client::import::http::Session> {
    match horizon_client::import::http::Session::start(home) {
        Ok(session) => {
            log::info!(
                "Horizon: post an archive to http://{}:{}/upload with the code {}",
                session.address().unwrap_or("this device"),
                session.port(),
                session.pin()
            );
            Some(session)
        }
        Err(error) => {
            log::warn!("Horizon: no upload port: {error}");
            None
        }
    }
}
