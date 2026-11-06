//! Options > Mods > Browse mods (docs/design/mod-hotload.md): reading the
//! registry's index and installing a content mod run on a worker
//! (`horizon_client::mod_registry`); each frame takes what it finished into the
//! options screen's `mod_browser`. The window's game and the startup menu each
//! have one.
use super::Game;
use horizon_client::mod_registry::Cancellation;
use horizon_client::{
    lang,
    mod_browser::{Listing, ModBrowser, Status},
    mod_list::ModList,
    mod_registry::{self, Done},
};
use horizon_mods::index::Index;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex, OnceLock,
    mpsc::{Receiver, TryRecvError, channel},
};
use std::thread::JoinHandle;

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
    producers: Vec<Arc<Producer>>,
    closed: bool,
}

impl Jobs {
    /// Browse mods opened: the index, read again from the registry (the
    /// launcher's copy when it can't be reached).
    pub(super) fn browse(&mut self, ui: &mut ModBrowser, home: PathBuf) {
        if self.closed || self.running.is_some() {
            return;
        }
        if ui.status != Status::Ready {
            ui.status = Status::Loading;
        }
        self.spawn("mod-browser-index", move |cancel| {
            let read = mod_registry::load_index_managed(&home, &mod_registry::location(), &cancel)
                .map(|index| {
                    let listings = mod_registry::listings(&index, &home);
                    (Arc::new(index), listings)
                });
            Finished::Index(read)
        });
    }

    /// Install or update `id` from the index read last.
    pub(super) fn install(&mut self, ui: &mut ModBrowser, home: PathBuf, id: String) {
        let Some(index) = self.index.clone() else {
            return;
        };
        if self.closed || self.running.is_some() {
            return;
        }
        ui.installing = Some(id.clone());
        ui.message = None;
        self.spawn("mod-browser-install", move |cancel| {
            let done = mod_registry::install_managed(
                &index,
                &home,
                &id,
                &|url, max| mod_registry::download_managed(url, max, &cancel),
                &cancel,
            );
            let listings = if cancel.cancelled() {
                Vec::new()
            } else {
                mod_registry::listings(&index, &home)
            };
            Finished::Installed(done, listings)
        });
    }

    fn spawn(
        &mut self,
        name: &str,
        work: impl FnOnce(Arc<Cancellation>) -> Finished + Send + 'static,
    ) {
        if self.closed {
            return;
        }
        self.producers.retain(|producer| !producer.finished());
        let producer = Arc::new(Producer {
            cancel: Arc::new(Cancellation::default()),
            thread: Mutex::new(None),
        });
        let cancel = producer.cancel.clone();
        let (send, receive) = channel();
        self.running = Some(receive);
        match std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                let _ = send.send(work(cancel));
            }) {
            Ok(thread) => {
                *producer.thread.lock().unwrap() = Some(thread);
                registry()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(producer.clone());
                self.producers.push(producer);
            }
            Err(error) => log::warn!("Horizon: {name} did not start: {error}"),
        }
    }

    /// Per-owner cancellation, used for a guest or loading/game shutdown. A
    /// dropped UI alone does not cancel an install the player already requested.
    pub(super) fn shutdown(&mut self) -> bool {
        self.closed = true;
        for producer in &self.producers {
            producer.cancel.cancel();
        }
        self.producers.retain(|producer| !producer.finished());
        self.producers.is_empty()
    }

    #[cfg(test)]
    pub(super) fn blocked_test_install() -> (Self, std::sync::mpsc::Sender<()>) {
        let mut jobs = Self::default();
        let (entered, ready) = channel();
        let (release, waiting) = channel();
        jobs.spawn("mod-browser-test", move |_| {
            entered.send(()).unwrap();
            waiting.recv().unwrap();
            Finished::Installed(
                Ok(Done {
                    id: "ruby".into(),
                    version: "1".into(),
                    verified: false,
                }),
                Vec::new(),
            )
        });
        ready
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        (jobs, release)
    }

    /// What finished since the last frame, into `ui`. An installed mod is
    /// turned on in `mods`; the caller saves the settings and lists the mods
    /// again when this returns true.
    pub(super) fn poll(&mut self, ui: &mut ModBrowser, mods: &mut ModList) -> bool {
        poll_retired_workers();
        self.producers.retain(|producer| !producer.finished());
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

struct Producer {
    cancel: Arc<Cancellation>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

/// An independent owned job, sharing the browser's shutdown registry without
/// occupying its install slot. Catalogue refreshes must not swallow installs.
pub(super) struct OwnedJob<T> {
    receive: Receiver<T>,
    producer: Arc<Producer>,
}
impl<T: Send + 'static> OwnedJob<T> {
    pub(super) fn spawn(
        name: &str,
        work: impl FnOnce(Arc<Cancellation>) -> T + Send + 'static,
    ) -> std::io::Result<Self> {
        let producer = Arc::new(Producer {
            cancel: Arc::new(Cancellation::default()),
            thread: Mutex::new(None),
        });
        let cancel = producer.cancel.clone();
        let (send, receive) = channel();
        let thread = std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                let _ = send.send(work(cancel));
            })?;
        *producer.thread.lock().unwrap() = Some(thread);
        registry()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(producer.clone());
        Ok(Self { receive, producer })
    }
    pub(super) fn poll(&self) -> Result<T, TryRecvError> {
        self.receive.try_recv()
    }
    pub(super) fn shutdown(&self) -> bool {
        self.producer.cancel.cancel();
        self.producer.finished()
    }
}
impl<T> Drop for OwnedJob<T> {
    fn drop(&mut self) {
        self.producer.cancel.cancel();
    }
}
impl Producer {
    fn finished(&self) -> bool {
        let Ok(mut handle) = self.thread.try_lock() else {
            return false;
        };
        if handle.as_ref().is_some_and(|thread| !thread.is_finished()) {
            return false;
        }
        if let Some(thread) = handle.take() {
            let _ = thread.join();
        }
        true
    }
}
fn registry() -> &'static Mutex<Vec<Arc<Producer>>> {
    static REGISTRY: OnceLock<Mutex<Vec<Arc<Producer>>>> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}
/// Full engine retirement: includes producers whose old startup/title owner
/// disappeared. Polling never waits for active IO or filesystem transactions.
pub(super) fn shutdown_workers() -> bool {
    let Ok(mut producers) = registry().try_lock() else {
        return false;
    };
    for producer in producers.iter() {
        producer.cancel.cancel();
    }
    producers.retain(|producer| !producer.finished());
    producers.is_empty()
}
pub(super) fn poll_retired_workers() {
    if let Ok(mut producers) = registry().try_lock() {
        producers.retain(|producer| !producer.finished());
    }
}

impl Game<'_> {
    pub(super) fn shutdown_mod_browser(&mut self) -> bool {
        let browser = self.mods.jobs.shutdown();
        let catalog = self.shutdown_mod_catalog();
        browser && catalog
    }
    /// Each frame: what the browser's worker finished.
    pub(super) fn drive_mod_browser(&mut self) {
        self.poll_mod_catalog();
        let installed = self
            .mods
            .jobs
            .poll(&mut self.ui.options.mod_browser, &mut self.settings.mods);
        if installed {
            self.settings.save(&Self::options_path());
            self.start_mods();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };
    fn wait(mut condition: impl FnMut() -> bool) {
        let end = Instant::now() + Duration::from_secs(5);
        while !condition() {
            assert!(Instant::now() < end, "worker did not finish");
            std::thread::yield_now();
        }
    }
    #[test]
    fn startup_handoff_keeps_the_running_install_and_applies_it_once() {
        let mut startup = Jobs::default();
        let (entered, ready) = mpsc::channel();
        let (release, wait_for_release) = mpsc::channel();
        startup.spawn("mod-browser-test", move |_| {
            entered.send(()).unwrap();
            wait_for_release.recv().unwrap();
            Finished::Installed(
                Ok(Done {
                    id: "ruby".into(),
                    version: "1".into(),
                    verified: false,
                }),
                Vec::new(),
            )
        });
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        // This is the ownership move StartupMenu::handoff performs.
        let mut game = std::mem::take(&mut startup);
        drop(startup);
        let mut ui = ModBrowser {
            installing: Some("ruby".into()),
            ..Default::default()
        };
        let mut mods = ModList::default();
        assert!(!game.poll(&mut ui, &mut mods));
        release.send(()).unwrap();
        wait(|| game.poll(&mut ui, &mut mods));
        assert!(ui.installing.is_none());
        assert!(mods.enabled.contains("ruby"));
        assert!(!game.poll(&mut ui, &mut mods));
        wait(|| game.shutdown());
    }
    #[test]
    fn shutdown_polls_without_joining_an_active_producer_and_closes_submissions() {
        let mut jobs = Jobs::default();
        let (entered, ready) = mpsc::channel();
        let (release, wait_for_release) = mpsc::channel();
        jobs.spawn("mod-browser-test", move |cancel| {
            entered.send(()).unwrap();
            wait_for_release.recv().unwrap();
            assert!(cancel.cancelled());
            Finished::Index(Err("cancelled".into()))
        });
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(!jobs.shutdown(), "active producer must remain owned");
        jobs.spawn("mod-browser-forbidden", |_| {
            panic!("retiring owner accepted new work")
        });
        release.send(()).unwrap();
        wait(|| jobs.shutdown());
    }
    #[test]
    fn navigation_does_not_detach_or_cancel_an_accepted_install() {
        let mut jobs = Jobs::default();
        let (release, wait_for_release) = mpsc::channel();
        jobs.spawn("mod-browser-test", move |cancel| {
            wait_for_release.recv().unwrap();
            assert!(!cancel.cancelled());
            Finished::Index(Err("finished".into()))
        });
        let producer = jobs.producers[0].clone();
        drop(jobs);
        assert!(!producer.finished());
        assert!(
            registry()
                .lock()
                .unwrap()
                .iter()
                .any(|p| Arc::ptr_eq(p, &producer))
        );
        release.send(()).unwrap();
        wait(|| producer.finished());
        poll_retired_workers();
    }
}
