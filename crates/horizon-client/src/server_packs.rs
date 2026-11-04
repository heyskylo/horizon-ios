//! Server resource packs (`ServerPackManager`, `DownloadedPackSource`): the
//! packs a server pushes, the player's answer to its prompt, their downloads,
//! their activation on top of the player's own packs, and the status each one
//! reports back to the server. The game drives it: it shows the prompt,
//! starts the downloads [`ServerPacks::tick`] asks for and reloads the
//! resources with [`ServerPacks::active`] when it asks for a reload.
pub mod download;

use std::{collections::VecDeque, path::PathBuf};

/// `ServerboundResourcePackPacket.Action`, by its protocol number.
pub mod action {
    pub const SUCCESSFULLY_LOADED: i32 = 0;
    pub const DECLINED: i32 = 1;
    pub const FAILED_DOWNLOAD: i32 = 2;
    pub const ACCEPTED: i32 = 3;
    pub const DOWNLOADED: i32 = 4;
    pub const INVALID_URL: i32 = 5;
    pub const FAILED_RELOAD: i32 = 6;
    pub const DISCARDED: i32 = 7;
}

/// `ServerPackManager.PackPromptStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prompt {
    Pending,
    Allowed,
    Declined,
}
impl Prompt {
    /// `ConnectScreen.convertPackStatus`: a server list entry's setting
    /// (servers.dat `acceptTextures`; none is Prompt).
    pub fn for_server(accept_textures: Option<bool>) -> Self {
        match accept_textures {
            Some(true) => Self::Allowed,
            Some(false) => Self::Declined,
            None => Self::Pending,
        }
    }
}

/// `ClientCommonPacketListenerImpl.parseResourcePackUrl`: only http and https.
pub fn valid_url(url: &str) -> bool {
    url.split_once(':').is_some_and(|(scheme, _)| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    })
}

/// `DownloadedPackSource.tryParseSha1Hash`: 40 hex digits, else no check.
pub fn parse_sha1(hash: &str) -> Option<String> {
    (hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| hash.to_ascii_lowercase())
}

/// One pack to download (`DownloadQueue.DownloadRequest`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub id: u128,
    pub url: String,
    pub sha1: Option<String>,
}

/// `DownloadQueue.BatchResult`: the file each downloaded pack is in, and the
/// packs that failed.
#[derive(Debug, Default)]
pub struct Batch {
    pub downloaded: Vec<(u128, PathBuf)>,
    pub failed: Vec<u128>,
}

/// What the game has to do after a [`ServerPacks::tick`].
#[derive(Debug, Default, PartialEq)]
pub struct Tick {
    /// Download these, then hand the result to [`ServerPacks::downloaded`].
    pub download: Vec<Request>,
    /// Reload the resources with these packs on top, bottom first
    /// (`PackReloadConfig.scheduleReload`), then call [`ServerPacks::reloaded`].
    pub reload: Option<Vec<(u128, PathBuf)>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Download {
    Requested,
    Pending,
    Done,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Activation {
    Inactive,
    Pending,
    Active,
}
/// `ServerPackManager.RemovalReason`, with the status it reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Removal {
    DownloadFailed,
    ActivationFailed,
    Declined,
    Discarded,
    ServerRemoved,
    ServerReplaced,
}
impl Removal {
    fn reply(self) -> Option<i32> {
        match self {
            Self::DownloadFailed => Some(action::FAILED_DOWNLOAD),
            Self::ActivationFailed => Some(action::FAILED_RELOAD),
            Self::Declined => Some(action::DECLINED),
            Self::Discarded => Some(action::DISCARDED),
            Self::ServerRemoved | Self::ServerReplaced => None,
        }
    }
}

/// The identity of one pack instance, independent of its wire UUID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Generation(u64);

/// `ServerPackManager.ServerPackData`.
#[derive(Debug)]
struct Pack {
    generation: Generation,
    id: u128,
    url: String,
    sha1: Option<String>,
    path: Option<PathBuf>,
    removal: Option<Removal>,
    download: Download,
    activation: Activation,
    accepted: bool,
}
impl Pack {
    fn remove(&mut self, reason: Removal) {
        self.removal.get_or_insert(reason);
    }
    fn removed(&self) -> bool {
        self.removal.is_some()
    }
}

/// The reload in progress: the packs it loads and unloads (the callbacks
/// `triggerReloadIfNeeded` gives `scheduleReload`).
#[derive(Debug)]
struct Reload {
    load: Vec<Generation>,
    unload: Vec<Generation>,
    /// It reloads what was active before a failed one (`isRecovery`).
    recovery: bool,
}

/// `ServerPackManager` with the feedback `DownloadedPackSource` gives it.
#[derive(Debug)]
pub struct ServerPacks {
    packs: Vec<Pack>,
    next_generation: u64,
    /// The native download queue completes batches in request order. Retain each batch's
    /// pack identities even after replacement/disconnect removes those packs from `packs`.
    downloads: VecDeque<Vec<(u128, Generation)>>,
    prompt: Prompt,
    reload: Option<Reload>,
    /// `registerForUpdate`: something changed, [`Self::tick`] again.
    update: bool,
    /// Report to the server (`configureForServerControl`); otherwise the
    /// statuses are only logged, as for a local world.
    reporting: bool,
    replies: Vec<(u128, i32)>,
}
impl Default for ServerPacks {
    fn default() -> Self {
        Self {
            packs: Vec::new(),
            next_generation: 0,
            downloads: VecDeque::new(),
            prompt: Prompt::Pending,
            reload: None,
            update: false,
            reporting: false,
            replies: Vec::new(),
        }
    }
}
impl ServerPacks {
    fn generation(&mut self) -> Generation {
        let generation = Generation(self.next_generation);
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .expect("resource pack generation overflow");
        generation
    }
    fn report(&mut self, id: u128, status: i32) {
        log::debug!("Horizon: server pack {id:032x} changed status to {status}");
        if self.reporting {
            self.replies.push((id, status));
        }
    }
    /// The statuses to send the server, oldest first.
    pub fn take_replies(&mut self) -> Vec<(u128, i32)> {
        std::mem::take(&mut self.replies)
    }
    /// Whether [`Self::tick`] has something to do.
    pub fn wants_tick(&self) -> bool {
        self.update
    }

    /// `DownloadedPackSource.configureForServerControl`.
    pub fn connect(&mut self, prompt: Prompt) {
        self.reporting = true;
        match prompt {
            Prompt::Allowed => self.allow(),
            Prompt::Declined => self.reject(),
            Prompt::Pending => self.prompt = Prompt::Pending,
        }
    }
    /// `DownloadedPackSource.configureForLocalWorld`: single player's own
    /// server; its packs are taken, and nothing is reported.
    pub fn local_world(&mut self) {
        self.reporting = false;
        self.allow();
    }
    /// `DownloadedPackSource.cleanupAfterDisconnect`: every pack goes, and
    /// the next server starts from its own setting.
    pub fn disconnect(&mut self) {
        self.pop_all();
        self.reporting = false;
        self.replies.clear();
        self.prompt = Prompt::Pending;
    }

    pub fn push(&mut self, id: u128, url: &str, sha1: Option<String>) {
        if self.prompt == Prompt::Declined {
            self.report(id, action::DECLINED);
            return;
        }
        for pack in self.packs.iter_mut().filter(|p| p.id == id) {
            pack.remove(Removal::ServerReplaced);
        }
        let generation = self.generation();
        self.packs.push(Pack {
            generation,
            id,
            url: url.to_owned(),
            sha1,
            path: None,
            removal: None,
            download: Download::Requested,
            activation: Activation::Inactive,
            accepted: false,
        });
        if self.prompt == Prompt::Allowed {
            self.accept(self.packs.len() - 1);
        }
        self.update = true;
    }
    /// `ServerPackManager.pushLocalPack`: a pack already on disk, taken as
    /// downloaded (a world's own `resourcepacks/resources.zip`).
    pub fn push_local(&mut self, id: u128, path: PathBuf) {
        if self.prompt == Prompt::Declined {
            self.report(id, action::DECLINED);
            return;
        }
        for pack in self.packs.iter_mut().filter(|p| p.id == id) {
            pack.remove(Removal::ServerReplaced);
        }
        let generation = self.generation();
        self.packs.push(Pack {
            generation,
            id,
            // Never downloaded: the file is already on disk.
            url: String::new(),
            sha1: None,
            path: Some(path),
            removal: None,
            download: Download::Done,
            activation: Activation::Inactive,
            accepted: false,
        });
        if self.prompt == Prompt::Allowed {
            self.accept(self.packs.len() - 1);
        }
        self.update = true;
    }
    /// What the player answered for this server's packs (splitscreen: the others follow P1).
    pub fn prompt(&self) -> Prompt {
        self.prompt
    }
    fn accept(&mut self, i: usize) {
        self.packs[i].accepted = true;
        let id = self.packs[i].id;
        self.report(id, action::ACCEPTED);
    }
    pub fn pop(&mut self, id: u128) {
        if let Some(pack) = self.packs.iter_mut().find(|p| !p.removed() && p.id == id) {
            pack.remove(Removal::ServerRemoved);
            self.update = true;
        }
    }
    pub fn pop_all(&mut self) {
        for pack in &mut self.packs {
            pack.remove(Removal::ServerRemoved);
        }
        self.update = true;
    }
    /// `allowServerPacks`: the prompt said yes (or the server is Enabled).
    pub fn allow(&mut self) {
        self.prompt = Prompt::Allowed;
        for i in 0..self.packs.len() {
            if !self.packs[i].accepted && !self.packs[i].removed() {
                self.accept(i);
            }
        }
        self.update = true;
    }
    /// `rejectServerPacks`: the prompt said no (or the server is Disabled).
    pub fn reject(&mut self) {
        self.prompt = Prompt::Declined;
        for pack in self.packs.iter_mut().filter(|p| !p.accepted) {
            pack.remove(Removal::Declined);
        }
        self.update = true;
    }

    /// `ServerPackManager.tick`: start the downloads the accepted packs
    /// need; once none is left, reload if the active packs should change;
    /// then forget the packs that are gone, reporting why.
    pub fn tick(&mut self) -> Tick {
        self.update = false;
        let mut out = Tick::default();
        let mut downloading = false;
        let mut requested = Vec::new();
        for pack in self.packs.iter_mut().filter(|p| !p.removed() && p.accepted) {
            downloading |= pack.download != Download::Done;
            if pack.download == Download::Requested {
                pack.download = Download::Pending;
                requested.push((pack.id, pack.generation));
                out.download.push(Request {
                    id: pack.id,
                    url: pack.url.clone(),
                    sha1: pack.sha1.clone(),
                });
            }
        }
        if !requested.is_empty() {
            self.downloads.push_back(requested);
        }
        if !downloading {
            out.reload = self.reload_if_needed();
        }
        let mut gone = Vec::new();
        self.packs.retain(|p| {
            let keep = p.activation != Activation::Inactive || p.removal.is_none();
            if !keep && let Some(reply) = p.removal.and_then(Removal::reply) {
                gone.push((p.id, reply));
            }
            keep
        });
        for (id, reply) in gone {
            self.report(id, reply);
        }
        out
    }
    fn reload_if_needed(&mut self) -> Option<Vec<(u128, PathBuf)>> {
        if self.reload.is_some()
            || self
                .packs
                .iter()
                .any(|p| p.activation == Activation::Pending)
        {
            return None;
        }
        let mut changed = false;
        let (mut load, mut unload) = (Vec::new(), Vec::new());
        for pack in &self.packs {
            let wanted = pack.accepted && pack.download == Download::Done && !pack.removed();
            match pack.activation {
                Activation::Inactive if wanted => {
                    load.push(pack.generation);
                    changed = true;
                }
                Activation::Active if wanted => load.push(pack.generation),
                Activation::Active => {
                    unload.push(pack.generation);
                    changed = true;
                }
                _ => {}
            }
        }
        if !changed {
            return None;
        }
        for pack in &mut self.packs {
            let loading = load.contains(&pack.generation) && pack.activation != Activation::Active;
            if loading || unload.contains(&pack.generation) {
                pack.activation = Activation::Pending;
            }
        }
        let paths = self.paths(&load);
        self.reload = Some(Reload {
            load,
            unload,
            recovery: false,
        });
        Some(paths)
    }
    /// The packs to load (`PackReloadConfig.Callbacks.packsToLoad`), bottom first.
    fn paths(&self, generations: &[Generation]) -> Vec<(u128, PathBuf)> {
        generations
            .iter()
            .filter_map(|generation| {
                let pack = self.packs.iter().find(|p| p.generation == *generation)?;
                Some((pack.id, pack.path.clone()?))
            })
            .collect()
    }

    /// `ServerPackManager.onDownload`. Results follow the request batches' order, as the native
    /// [`download::Queue`] guarantees, so a result can be paired with its original pack instances.
    pub fn downloaded(&mut self, batch: Batch) {
        let Some(requested) = self.downloads.pop_front() else {
            return;
        };
        if !batch.failed.is_empty() {
            for pack in self
                .packs
                .iter_mut()
                .filter(|p| p.activation != Activation::Active)
            {
                pack.remove(if batch.failed.contains(&pack.id) {
                    Removal::DownloadFailed
                } else {
                    Removal::Discarded
                });
            }
        }
        let mut done = Vec::new();
        for (id, path) in batch.downloaded {
            let Some((_, generation)) = requested.iter().find(|(requested, _)| *requested == id)
            else {
                continue;
            };
            if let Some(pack) = self
                .packs
                .iter_mut()
                .find(|p| p.generation == *generation && p.download == Download::Pending)
            {
                pack.download = Download::Done;
                pack.path = Some(path);
                if !pack.removed() {
                    done.push(id);
                }
            }
        }
        for id in done {
            self.report(id, action::DOWNLOADED);
        }
        self.update = true;
    }

    /// The reload [`Tick::reload`] asked for finished (`Callbacks.onSuccess`),
    /// or its packs could not be loaded (`onFailure`). After a first failure
    /// the packs that were active are loaded again: the packs to reload with
    /// are returned, and this is called again with that reload's outcome.
    pub fn reloaded(&mut self, ok: bool) -> Option<Vec<(u128, PathBuf)>> {
        let mut reload = self.reload.take()?;
        self.update = true;
        if ok {
            let mut applied = Vec::new();
            for pack in &mut self.packs {
                if reload.load.contains(&pack.generation) {
                    pack.activation = Activation::Active;
                    if pack.removal.is_none() {
                        applied.push(pack.id);
                    }
                } else if reload.unload.contains(&pack.generation) {
                    pack.activation = Activation::Inactive;
                }
            }
            for id in applied {
                self.report(id, action::SUCCESSFULLY_LOADED);
            }
            return None;
        }
        if reload.recovery {
            // onFailure(true): nothing of the server's is loaded.
            for pack in &mut self.packs {
                if pack.activation == Activation::Pending {
                    pack.activation = Activation::Inactive;
                }
            }
            return None;
        }
        reload.load.clear();
        for pack in &mut self.packs {
            match pack.activation {
                Activation::Inactive => pack.remove(Removal::Discarded),
                Activation::Pending => {
                    pack.activation = Activation::Inactive;
                    pack.remove(Removal::ActivationFailed);
                }
                Activation::Active => reload.load.push(pack.generation),
            }
        }
        reload.recovery = true;
        let paths = self.paths(&reload.load);
        self.reload = Some(reload);
        Some(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use action::*;

    fn ok(id: u128) -> Batch {
        Batch {
            downloaded: vec![(id, PathBuf::from(format!("/p/{id}")))],
            failed: vec![],
        }
    }
    /// Runs the updates as `createUpdateScheduler` does, downloading and
    /// reloading with `download` and `reload` as the game would.
    fn settle(
        sp: &mut ServerPacks,
        download: &mut dyn FnMut(&[Request]) -> Batch,
        reload: &mut dyn FnMut(&[(u128, PathBuf)]) -> bool,
    ) {
        let mut rounds = 0;
        while sp.wants_tick() {
            rounds += 1;
            assert!(rounds < 50);
            let t = sp.tick();
            if !t.download.is_empty() {
                let batch = download(&t.download);
                sp.downloaded(batch);
            }
            if let Some(mut packs) = t.reload {
                while let Some(back) = sp.reloaded(reload(&packs)) {
                    packs = back;
                }
            }
        }
    }

    #[test]
    fn an_enabled_server_s_pack_is_accepted_downloaded_and_loaded() {
        let mut sp = ServerPacks::default();
        sp.connect(Prompt::Allowed);
        sp.push(1, "http://a/p.zip", None);
        let mut loaded = Vec::new();
        settle(&mut sp, &mut |r| ok(r[0].id), &mut |p| {
            loaded = p.to_vec();
            true
        });
        assert_eq!(
            sp.take_replies(),
            [(1, ACCEPTED), (1, DOWNLOADED), (1, SUCCESSFULLY_LOADED)]
        );
        assert_eq!(loaded, [(1, PathBuf::from("/p/1"))]);
    }

    #[test]
    fn the_prompt_holds_packs_until_answered() {
        let mut sp = ServerPacks::default();
        sp.connect(Prompt::Pending);
        sp.push(1, "http://a/p.zip", None);
        settle(&mut sp, &mut |_| unreachable!(), &mut |_| unreachable!());
        assert!(sp.take_replies().is_empty());
        sp.reject();
        settle(&mut sp, &mut |_| unreachable!(), &mut |_| unreachable!());
        assert_eq!(sp.take_replies(), [(1, DECLINED)]);
        // Declined: later pushes are declined at once.
        sp.push(2, "http://a/q.zip", None);
        assert_eq!(sp.take_replies(), [(2, DECLINED)]);
    }

    #[test]
    fn a_failed_download_discards_the_rest_of_the_batch() {
        let mut sp = ServerPacks::default();
        sp.connect(Prompt::Allowed);
        sp.push(1, "http://a/p.zip", None);
        sp.push(2, "http://a/dead.zip", None);
        settle(
            &mut sp,
            &mut |_| Batch {
                downloaded: vec![(1, PathBuf::from("/p/1"))],
                failed: vec![2],
            },
            &mut |_| unreachable!(),
        );
        assert_eq!(
            sp.take_replies(),
            [
                (1, ACCEPTED),
                (2, ACCEPTED),
                (1, DISCARDED),
                (2, FAILED_DOWNLOAD)
            ]
        );
    }

    #[test]
    fn a_pack_that_does_not_load_fails_its_reload() {
        let mut sp = ServerPacks::default();
        sp.connect(Prompt::Allowed);
        sp.push(1, "http://a/p.zip", None);
        let mut reloads = Vec::new();
        settle(&mut sp, &mut |r| ok(r[0].id), &mut |p| {
            reloads.push(p.to_vec());
            reloads.len() > 1
        });
        assert_eq!(
            sp.take_replies(),
            [(1, ACCEPTED), (1, DOWNLOADED), (1, FAILED_RELOAD)]
        );
        // The recovery reload has none of the server's packs.
        assert_eq!(reloads[1], []);
    }

    #[test]
    fn popping_and_disconnecting_unload_the_packs() {
        let mut sp = ServerPacks::default();
        sp.connect(Prompt::Allowed);
        sp.push(1, "http://a/p.zip", None);
        sp.push(2, "http://a/q.zip", None);
        let last = std::cell::RefCell::new(Vec::new());
        let mut reload = |p: &[(u128, PathBuf)]| {
            *last.borrow_mut() = p.to_vec();
            true
        };
        settle(
            &mut sp,
            &mut |r| Batch {
                downloaded: r
                    .iter()
                    .map(|q| (q.id, PathBuf::from(format!("/p/{}", q.id))))
                    .collect(),
                failed: vec![],
            },
            &mut reload,
        );
        sp.take_replies();
        sp.pop(1);
        settle(&mut sp, &mut |_| unreachable!(), &mut reload);
        // Callbacks.onSuccess reports every pack the reload loaded, again.
        assert_eq!(sp.take_replies(), [(2, SUCCESSFULLY_LOADED)]);
        assert_eq!(*last.borrow(), [(2, PathBuf::from("/p/2"))]);
        sp.disconnect();
        settle(&mut sp, &mut |_| unreachable!(), &mut reload);
        assert!(last.borrow().is_empty());
        assert!(sp.packs.is_empty());
    }

    #[test]
    fn a_pushed_id_replaces_its_pack() {
        let mut sp = ServerPacks::default();
        sp.connect(Prompt::Allowed);
        sp.push(1, "http://a/old.zip", None);
        sp.push(1, "http://a/new.zip", None);
        let mut urls = Vec::new();
        settle(
            &mut sp,
            &mut |r| {
                urls.extend(r.iter().map(|q| q.url.clone()));
                ok(1)
            },
            &mut |_| true,
        );
        assert_eq!(urls, ["http://a/new.zip"]);
    }

    fn local_active(sp: &mut ServerPacks, id: u128, path: &str) {
        sp.push_local(id, PathBuf::from(path));
        settle(sp, &mut |_| unreachable!(), &mut |_| true);
        sp.take_replies();
    }

    #[test]
    fn replacing_an_active_uuid_loads_the_new_path_and_pop_unloads_it() {
        let mut sp = ServerPacks::default();
        sp.connect(Prompt::Allowed);
        local_active(&mut sp, 1, "/p/old");
        sp.push_local(1, PathBuf::from("/p/new"));
        assert_eq!(sp.tick().reload, Some(vec![(1, PathBuf::from("/p/new"))]));
        assert_eq!(sp.reloaded(true), None);
        assert_eq!(sp.tick().reload, None);
        assert_eq!(sp.packs.len(), 1);
        assert_eq!(sp.packs[0].path, Some(PathBuf::from("/p/new")));
        assert_eq!(sp.packs[0].activation, Activation::Active);
        assert_eq!(sp.take_replies(), [(1, ACCEPTED), (1, SUCCESSFULLY_LOADED)]);
        sp.pop(1);
        assert_eq!(sp.tick().reload, Some(vec![]));
        sp.reloaded(true);
        sp.tick();
        assert!(sp.packs.is_empty());
        assert!(sp.take_replies().is_empty());
    }

    #[test]
    fn a_pending_reload_cannot_activate_a_later_generation_with_the_same_uuid() {
        let mut sp = ServerPacks::default();
        sp.connect(Prompt::Allowed);
        local_active(&mut sp, 1, "/p/first");
        sp.push_local(1, PathBuf::from("/p/second"));
        assert_eq!(
            sp.tick().reload,
            Some(vec![(1, PathBuf::from("/p/second"))])
        );
        sp.push_local(1, PathBuf::from("/p/third"));
        assert_eq!(sp.tick().reload, None);
        assert_eq!(sp.reloaded(true), None);
        let newest = sp.packs.iter().find(|p| !p.removed()).unwrap();
        assert_eq!(newest.path, Some(PathBuf::from("/p/third")));
        assert_eq!(newest.activation, Activation::Inactive);
        assert_eq!(sp.take_replies(), [(1, ACCEPTED), (1, ACCEPTED)]);
        assert_eq!(sp.tick().reload, Some(vec![(1, PathBuf::from("/p/third"))]));
        sp.reloaded(true);
        sp.tick();
        assert_eq!(sp.take_replies(), [(1, SUCCESSFULLY_LOADED)]);
        assert_eq!(sp.packs.len(), 1);
        assert_eq!(sp.packs[0].path, Some(PathBuf::from("/p/third")));
    }

    #[test]
    fn a_replaced_download_result_only_completes_its_requested_generation() {
        let mut sp = ServerPacks::default();
        sp.connect(Prompt::Allowed);
        sp.push(1, "https://example.com/old.zip", None);
        assert_eq!(sp.tick().download[0].url, "https://example.com/old.zip");
        sp.push(1, "https://example.com/new.zip", None);
        assert_eq!(sp.tick().download[0].url, "https://example.com/new.zip");
        assert_eq!(sp.packs.len(), 1); // The removed old download has already been forgotten.
        sp.downloaded(Batch {
            downloaded: vec![(1, PathBuf::from("/p/old"))],
            failed: vec![],
        });
        assert_eq!(sp.packs[0].path, None);
        assert_eq!(sp.packs[0].download, Download::Pending);
        assert_eq!(sp.take_replies(), [(1, ACCEPTED), (1, ACCEPTED)]);
        assert_eq!(sp.tick().reload, None);
        sp.downloaded(Batch {
            downloaded: vec![(1, PathBuf::from("/p/new"))],
            failed: vec![],
        });
        assert_eq!(sp.tick().reload, Some(vec![(1, PathBuf::from("/p/new"))]));
        sp.reloaded(true);
        sp.tick();
        assert_eq!(
            sp.take_replies(),
            [(1, DOWNLOADED), (1, SUCCESSFULLY_LOADED)]
        );
        assert!(sp.downloads.is_empty());
    }

    #[test]
    fn disconnect_keeps_download_batch_identity_until_the_old_result_arrives() {
        let mut sp = ServerPacks::default();
        sp.connect(Prompt::Allowed);
        sp.push(0, "https://example.com/first-server.zip", None);
        assert_eq!(sp.tick().download.len(), 1);
        sp.disconnect();
        sp.tick();
        assert!(sp.packs.is_empty());
        sp.connect(Prompt::Allowed);
        sp.push(0, "https://example.com/second-server.zip", None);
        assert_eq!(sp.tick().download.len(), 1);
        sp.downloaded(Batch {
            downloaded: vec![(0, PathBuf::from("/p/first-server"))],
            failed: vec![],
        });
        assert_eq!(sp.tick().reload, None);
        assert_eq!(sp.packs[0].path, None);
        assert_eq!(sp.take_replies(), [(0, ACCEPTED)]);
        sp.downloaded(Batch {
            downloaded: vec![(0, PathBuf::from("/p/second-server"))],
            failed: vec![],
        });
        assert_eq!(
            sp.tick().reload,
            Some(vec![(0, PathBuf::from("/p/second-server"))])
        );
        sp.reloaded(true);
        sp.tick();
        assert_eq!(
            sp.take_replies(),
            [(0, DOWNLOADED), (0, SUCCESSFULLY_LOADED)]
        );
        sp.disconnect();
        settle(&mut sp, &mut |_| unreachable!(), &mut |paths| {
            assert!(paths.is_empty());
            true
        });
        assert!(sp.packs.is_empty());
        assert!(sp.downloads.is_empty());
        assert!(sp.take_replies().is_empty());
    }

    #[test]
    fn a_failed_replacement_recovers_only_the_other_previously_active_pack() {
        let mut sp = ServerPacks::default();
        sp.connect(Prompt::Allowed);
        local_active(&mut sp, 1, "/p/old");
        local_active(&mut sp, 2, "/p/kept");
        sp.push_local(1, PathBuf::from("/p/broken"));
        assert_eq!(
            sp.tick().reload,
            Some(vec![
                (2, PathBuf::from("/p/kept")),
                (1, PathBuf::from("/p/broken"))
            ])
        );
        assert_eq!(
            sp.reloaded(false),
            Some(vec![(2, PathBuf::from("/p/kept"))])
        );
        assert_eq!(sp.reloaded(true), None);
        sp.tick();
        assert_eq!(
            sp.take_replies(),
            [(1, ACCEPTED), (2, SUCCESSFULLY_LOADED), (1, FAILED_RELOAD)]
        );
        assert_eq!(sp.packs.len(), 1);
        assert_eq!(sp.packs[0].id, 2);
        assert_eq!(sp.packs[0].activation, Activation::Active);
        assert_eq!(sp.packs[0].path, Some(PathBuf::from("/p/kept")));
    }

    #[test]
    fn a_failed_old_batch_keeps_vanillas_failure_rule_for_current_inactive_packs() {
        let mut sp = ServerPacks::default();
        sp.connect(Prompt::Allowed);
        sp.push(1, "https://example.com/old.zip", None);
        sp.tick();
        sp.push(1, "https://example.com/new.zip", None);
        sp.push_local(2, PathBuf::from("/p/not-active"));
        sp.tick();
        sp.take_replies();
        sp.downloaded(Batch {
            downloaded: vec![],
            failed: vec![1],
        });
        assert_eq!(sp.tick().reload, None);
        assert_eq!(sp.take_replies(), [(1, FAILED_DOWNLOAD), (2, DISCARDED)]);
        assert!(sp.packs.is_empty());
        // The second queued result still has its own identity, even after failure cleanup.
        sp.downloaded(ok(1));
        sp.tick();
        assert!(sp.packs.is_empty());
        assert!(sp.downloads.is_empty());
        assert!(sp.take_replies().is_empty());
    }

    #[test]
    fn an_old_failed_batch_after_reconnect_keeps_vanillas_uuid_failure_result() {
        // The actual 26.3 manager keeps its callbacks through cleanup/popAll and a new
        // server's configureForServerControl. Its global failure rule includes new packs.
        let mut sp = ServerPacks::default();
        sp.connect(Prompt::Allowed);
        sp.push(0, "https://example.com/old-server.zip", None);
        sp.tick();
        sp.disconnect();
        sp.tick();
        sp.connect(Prompt::Allowed);
        sp.push(0, "https://example.com/new-server.zip", None);
        sp.tick();
        sp.take_replies();
        sp.downloaded(Batch {
            downloaded: vec![],
            failed: vec![0],
        });
        assert_eq!(sp.tick().reload, None);
        assert_eq!(sp.take_replies(), [(0, FAILED_DOWNLOAD)]);
        assert!(sp.packs.is_empty());
        sp.downloaded(Batch {
            downloaded: vec![(0, PathBuf::from("/p/new-server"))],
            failed: vec![],
        });
        assert_eq!(sp.tick().reload, None);
        assert!(sp.take_replies().is_empty());
        assert!(sp.packs.is_empty());
        assert!(sp.downloads.is_empty());
    }

    #[test]
    fn urls_and_hashes_are_read_as_vanilla_reads_them() {
        assert!(valid_url("https://example.com/p.zip"));
        assert!(valid_url("HTTP://example.com/p.zip"));
        assert!(!valid_url("ftp://example.com/p.zip"));
        assert!(!valid_url("example.com/p.zip"));
        let sha1 = "0123456789ABCDEF0123456789abcdef01234567";
        assert_eq!(parse_sha1(sha1), Some(sha1.to_ascii_lowercase()));
        assert_eq!(parse_sha1("abc"), None);
    }
}
