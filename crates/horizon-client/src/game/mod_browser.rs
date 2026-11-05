//! Options > Mods > Browse mods (docs/design/mod-hotload.md): reading the
//! registry's index and installing a content mod run on a worker
//! (`horizon_client::mod_registry`); each frame takes what it finished into the
//! options screen's `mod_browser`. The window's game and the startup menu each
//! have one.
use super::Game;
use horizon_client::{
    lang,
    mod_browser::{Listing, ModBrowser, Status},
    mod_list::ModList,
    mod_registry::{self, Done},
};
use horizon_mods::index::Index;
use std::path::PathBuf;
use std::sync::{
    Arc,
    mpsc::{Receiver, TryRecvError, channel},
};

/// What the worker sends back: the index and its listings, or an install's
/// outcome with the listings after it.
enum Finished {
    Index(Result<(Arc<Index>, Vec<Listing>), String>),
    Installed(Result<Done, String>, Vec<Listing>),
}

/// One job at a time, and the index read last.
#[derive(Default)]
pub(super) struct Jobs {
    running: Option<Receiver<Finished>>,
    index: Option<Arc<Index>>,
}

impl Jobs {
    /// Browse mods opened: the index, read again from the registry (the
    /// launcher's copy when it can't be reached).
    pub(super) fn browse(&mut self, ui: &mut ModBrowser, home: PathBuf) {
        if self.running.is_some() {
            return;
        }
        if ui.status != Status::Ready {
            ui.status = Status::Loading;
        }
        let (send, receive) = channel();
        self.running = Some(receive);
        spawn("mod-browser-index", move || {
            let read = mod_registry::load_index(&home, &mod_registry::location()).map(|index| {
                let listings = mod_registry::listings(&index, &home);
                (Arc::new(index), listings)
            });
            let _ = send.send(Finished::Index(read));
        });
    }

    /// Install or update `id` from the index read last.
    pub(super) fn install(&mut self, ui: &mut ModBrowser, home: PathBuf, id: String) {
        let Some(index) = self.index.clone() else {
            return;
        };
        if self.running.is_some() {
            return;
        }
        ui.installing = Some(id.clone());
        ui.message = None;
        let (send, receive) = channel();
        self.running = Some(receive);
        spawn("mod-browser-install", move || {
            let done = mod_registry::install(&index, &home, &id, &mod_registry::download);
            let listings = mod_registry::listings(&index, &home);
            let _ = send.send(Finished::Installed(done, listings));
        });
    }

    /// What finished since the last frame, into `ui`. An installed mod is
    /// turned on in `mods`; the caller saves the settings and lists the mods
    /// again when this returns true.
    pub(super) fn poll(&mut self, ui: &mut ModBrowser, mods: &mut ModList) -> bool {
        let Some(running) = &self.running else {
            return false;
        };
        let finished = match running.try_recv() {
            Ok(finished) => finished,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => {
                self.running = None;
                ui.installing = None;
                if ui.status == Status::Loading {
                    ui.status = Status::Failed("the worker stopped".into());
                }
                return false;
            }
        };
        self.running = None;
        match finished {
            Finished::Index(Ok((index, listings))) => {
                self.index = Some(index);
                ui.listings = listings;
                ui.status = Status::Ready;
                false
            }
            Finished::Index(Err(why)) => {
                log::warn!("Horizon: the mod registry wasn't read: {why}");
                if ui.status != Status::Ready {
                    ui.status = Status::Failed(why);
                }
                false
            }
            Finished::Installed(done, listings) => {
                ui.installing = None;
                ui.listings = listings;
                match done {
                    Ok(done) => {
                        log::info!(
                            "Horizon: installed the mod {} {} from the registry ({})",
                            done.id,
                            done.version,
                            if done.verified { "signed" } else { "unsigned" }
                        );
                        mods.enabled.insert(done.id.clone());
                        mods.disabled.remove(&done.id);
                        let name = ui
                            .listing(&done.id)
                            .map_or(done.id.clone(), |l| l.name.clone());
                        let text = lang::tr("horizon.mods.browse.done", &[&name, &done.version]);
                        ui.message = Some((text, true));
                        true
                    }
                    Err(why) => {
                        log::warn!("Horizon: a registry mod wasn't installed: {why}");
                        let text = lang::tr("horizon.mods.browse.install_failed", &[&why]);
                        ui.message = Some((text, false));
                        false
                    }
                }
            }
        }
    }
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(e) = std::thread::Builder::new().name(name.into()).spawn(work) {
        log::warn!("Horizon: {name} did not start: {e}");
    }
}

impl Game<'_> {
    /// Each frame: what the browser's worker finished.
    pub(super) fn drive_mod_browser(&mut self) {
        let installed = self
            .mods
            .jobs
            .poll(&mut self.ui.options.mod_browser, &mut self.settings.mods);
        if installed {
            self.settings.save(&Self::options_path());
            self.start_mods();
            self.switch_mods();
            // A mod with blocks, items or worldgen waits for a restart: the
            // project page says so instead of "installed and on".
            if let (Some(note), Some((message, _))) = (
                &self.ui.options.mods_note,
                &mut self.ui.options.mod_browser.message,
            ) {
                *message = format!("{message} {note}");
            }
        }
    }
}
