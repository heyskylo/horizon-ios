//! A content switch publishes only after both the GPU and its original server
//! accept it. CPU rollback resources retain the previous immutable generation.
use super::{Game, ReloadKind, StagedPack};
use crate::game::{functions::PreparedReload, integrated::Poster};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, TryRecvError, channel},
};

pub(in crate::game) struct CancelOnDrop(Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

pub(in crate::game) enum Transaction {
    Preparing {
        kind: ReloadKind,
        staged: Box<StagedPack>,
        poster: Poster,
        receive: Receiver<Result<PreparedReload, String>>,
        cancel: CancelOnDrop,
    },
    Committing {
        kind: ReloadKind,
        rollback: Box<StagedPack>,
        poster: Poster,
        receive: Receiver<Result<(), String>>,
        cancel: CancelOnDrop,
    },
}

impl Game<'_> {
    pub(in crate::game) fn content_reload_busy(&self) -> bool {
        self.mods.transaction.is_some()
    }
    pub(in crate::game) fn content_reload_progress(&self) -> Option<(&'static str, f32)> {
        self.content_reload_busy()
            .then_some((super::RELOAD_LINES[3], 1.0))
    }

    pub(super) fn begin_content_reload(&mut self, kind: ReloadKind, mut staged: StagedPack) {
        self.begin_reload_input_block();
        let changed = staged.pack.content.as_ref().is_some_and(|next| {
            self.mods
                .content
                .as_ref()
                .is_none_or(|old| old.selection != next.selection)
        });
        if changed && self.remote.is_some() && self.integrated.is_none() {
            self.restage_applied_content(
                kind,
                "content mods cannot be switched on another server".into(),
            );
            return;
        }
        if changed
            && self.remote.is_none()
            && self.integrated.is_none()
            && !self.world_root.as_os_str().is_empty()
        {
            // This branch is an authoritative headless/server Game. A window
            // enters worlds through Integrated and never performs this IO.
            if self.options.execution_role.is_client() {
                self.restage_applied_content(
                    kind,
                    "content reload is waiting for its authoritative server".into(),
                );
                return;
            }
            let Some(rollback) = staged.rollback.take() else {
                self.restage_applied_content(
                    kind,
                    "content reload has no rollback generation".into(),
                );
                return;
            };
            let prepared =
                match self.prepare_content_reload(staged.pack.content.as_ref().unwrap().clone()) {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        self.restage_applied_content(kind, error.to_string());
                        return;
                    }
                };
            let previous_root = self.options.resources.clone();
            if let Err(error) = self.apply_staged_pack(staged) {
                self.finish_content_reload(kind, Err(error.to_string()));
                return;
            }
            self.options.resources = previous_root;
            match self.commit_content_reload(prepared) {
                Ok(()) => self.finish_content_reload(kind, Ok(())),
                Err(error) => self.rollback_content_reload(kind, rollback, error.to_string()),
            }
            return;
        }
        if !changed || self.integrated.is_none() {
            let result = self.apply_staged_pack(staged).map_err(|e| e.to_string());
            self.finish_content_reload(kind, result);
            return;
        }
        if staged.rollback.is_none() {
            self.restage_applied_content(kind, "content reload has no rollback generation".into());
            return;
        }
        let poster = self.integrated.as_ref().unwrap().poster();
        let content = staged.pack.content.as_ref().unwrap().clone();
        let cancel = CancelOnDrop(Arc::new(AtomicBool::new(false)));
        let token = cancel.0.clone();
        let (send, receive) = channel();
        if !poster.post(move |game| {
            if token.load(Ordering::Acquire) {
                return;
            }
            let prepared = game
                .prepare_content_reload(content)
                .map_err(|e| e.to_string());
            if !token.load(Ordering::Acquire) {
                let _ = send.send(prepared);
            }
        }) {
            self.restage_applied_content(
                kind,
                "the original server stopped before resource preparation".into(),
            );
            return;
        }
        self.mods.transaction = Some(Transaction::Preparing {
            kind,
            staged: Box::new(staged),
            poster,
            receive,
            cancel,
        });
    }

    pub(super) fn poll_content_reload(&mut self) {
        let Some(transaction) = self.mods.transaction.take() else {
            return;
        };
        match transaction {
            Transaction::Preparing {
                kind,
                mut staged,
                poster,
                receive,
                cancel,
            } => {
                let result = match receive.try_recv() {
                    Ok(result) => result,
                    Err(TryRecvError::Empty) => {
                        self.mods.transaction = Some(Transaction::Preparing {
                            kind,
                            staged,
                            poster,
                            receive,
                            cancel,
                        });
                        return;
                    }
                    Err(TryRecvError::Disconnected) => {
                        Err("server resource preparation stopped".into())
                    }
                };
                // Discard obsolete local results before any GPU/server publication.
                if kind == ReloadKind::Selection && self.pending_pack_reload {
                    return;
                }
                if !self
                    .integrated
                    .as_ref()
                    .is_some_and(|server| poster.is_for(server))
                {
                    self.restage_applied_content(
                        kind,
                        "the world changed during resource preparation".into(),
                    );
                    return;
                }
                let prepared = match result {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        self.restage_applied_content(kind, error);
                        return;
                    }
                };
                let rollback = staged.rollback.take().unwrap();
                if let Err(error) = self.apply_staged_pack(*staged) {
                    self.finish_content_reload(kind, Err(error.to_string()));
                    return;
                }
                let token = cancel.0.clone();
                let (send, receive) = channel();
                if !poster.post(move |game| {
                    if token.load(Ordering::Acquire) {
                        return;
                    }
                    let result = game
                        .commit_content_reload(prepared)
                        .map_err(|e| e.to_string());
                    let _ = send.send(result);
                }) {
                    self.rollback_content_reload(
                        kind,
                        rollback,
                        "the original server stopped before resource commit".into(),
                    );
                    return;
                }
                self.mods.transaction = Some(Transaction::Committing {
                    kind,
                    rollback,
                    poster,
                    receive,
                    cancel,
                });
            }
            Transaction::Committing {
                kind,
                rollback,
                poster,
                receive,
                cancel,
            } => {
                if !self
                    .integrated
                    .as_ref()
                    .is_some_and(|server| poster.is_for(server))
                {
                    self.rollback_content_reload(
                        kind,
                        rollback,
                        "the original server changed before resource acknowledgment".into(),
                    );
                    return;
                }
                match receive.try_recv() {
                    Ok(Ok(())) => self.finish_content_reload(kind, Ok(())),
                    Ok(Err(error)) => self.rollback_content_reload(kind, rollback, error),
                    Err(TryRecvError::Disconnected) => self.rollback_content_reload(
                        kind,
                        rollback,
                        "server resource commit stopped".into(),
                    ),
                    Err(TryRecvError::Empty) => {
                        self.mods.transaction = Some(Transaction::Committing {
                            kind,
                            rollback,
                            poster,
                            receive,
                            cancel,
                        });
                    }
                }
            }
        }
    }

    fn rollback_content_reload(
        &mut self,
        kind: ReloadKind,
        rollback: Box<StagedPack>,
        error: String,
    ) {
        if let Err(rollback_error) = self.apply_staged_pack(*rollback) {
            self.error = Some(format!("content reload failed ({error}); restoring the previous renderer failed: {rollback_error}").into());
            return;
        }
        self.restage_applied_content(kind, error);
    }

    /// Context/mod-data rejection does not make a valid resource pack bad.
    /// Rebuild the same request over the actual running content; preserve its
    /// Server callback until that request really finishes.
    fn restage_applied_content(&mut self, kind: ReloadKind, error: String) {
        log::warn!("Horizon: content switch deferred: {error}");
        self.ui.options.mods_note = Some(error);
        let selection = self
            .mods
            .content
            .as_ref()
            .map(|content| (content.selection.clone(), content.catalog.clone()))
            .unwrap_or_default();
        if let Err(error) = self.spawn_pack_reload_using(kind, selection) {
            self.finish_content_reload(kind, Err(error.to_string()));
        }
    }
}

#[cfg(test)]
mod transaction_tests {
    use super::*;
    use crate::game::{
        Audio, Game, Options, PendingWorldEntry, Progress, Settings, UiResources,
        execution_role::ExecutionRole, integrated::Integrated, load_resources, mods,
    };
    use std::path::PathBuf;

    struct TestLoop;
    impl crate::game::event_loop::EventLoopCtl for TestLoop {
        fn exit(&self) {
            panic!("input disarm must not exit the game");
        }
        fn set_control_flow(&self, _: winit::event_loop::ControlFlow) {}
    }

    fn stage(game: &Game<'_>, next: Arc<mods::PreparedContent>) -> StagedPack {
        let mut staged = super::super::stage_root(
            &game.vanilla_root(),
            next.root.clone(),
            Some(next),
            &game.pack.assets,
            &game.res,
            &game.pack.context,
            false,
            false,
            &Progress::new(),
        )
        .unwrap();
        staged.rollback = Some(Box::new(
            super::super::stage_root(
                &game.vanilla_root(),
                game.options.resources.clone(),
                game.mods.content.clone(),
                &game.pack.assets,
                &game.res,
                &game.pack.context,
                false,
                false,
                &Progress::new(),
            )
            .unwrap(),
        ));
        staged
    }

    fn wait_pack(game: &mut Game<'_>) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while game.pack_reload.is_some() {
            assert!(
                std::time::Instant::now() < deadline,
                "resource restage did not finish"
            );
            game.drive_pack_reload();
            std::thread::yield_now();
        }
        assert!(!game.content_reload_busy());
    }

    #[test]
    #[ignore = "requires prepared HORIZON_TEST_RESOURCES and isolated HORIZON_HOME"]
    fn owned_content_transaction_preserves_generations_and_blocks_input_until_ack() {
        assert!(std::env::var_os("HORIZON_HOME").is_some());
        let root = PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let mut loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let initial = Arc::new(mods::PreparedContent {
            root: root.clone(),
            catalog: Default::default(),
            selection: Default::default(),
            lease: Default::default(),
            data_changed: false,
        });
        loaded.pack.content = Some(initial.clone());
        let make_game = |role| {
            let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
            options.resources = root.clone();
            options.execution_role = role;
            let mut game = Game::new(
                &loaded.resources,
                loaded.pack.clone(),
                UiResources::server(&root).unwrap(),
                options,
                Settings::default(),
                Audio::silent(&root),
                None,
            )
            .unwrap();
            game.ui.vanilla_root = root.clone();
            game
        };
        let mut client = make_game(ExecutionRole::Client);
        let mut server = make_game(ExecutionRole::IntegratedServer);
        let world = Game::game_dir().join("transaction-world");
        std::fs::create_dir_all(&world).unwrap();
        server.world_root = world;
        let mods_dir = Game::game_dir().join(horizon_client::mod_list::DIR);
        std::fs::create_dir_all(&mods_dir).unwrap();
        let manifest = "id = \"ruby\"\nname = \"Ruby\"\nversion = \"1.0.0\"\napi = \"0.2\"\nside = \"both\"\ncapabilities = [\"registries\"]\n";
        let bytes = horizon_mods::Package::write_files(
            manifest,
            None,
            &[(
                "assets/ruby/lang/en_us.json",
                b"{\"ruby.fixture\":\"new generation\"}",
            )],
        )
        .unwrap();
        std::fs::write(mods_dir.join("ruby.hmod"), bytes).unwrap();
        let catalog = mods::catalog::read(&Game::game_dir(), &[], &Default::default()).unwrap();
        let selection = mods::catalog::Selection::wanted(
            &std::collections::BTreeSet::from(["ruby".into()]),
            &catalog,
        );
        let next = mods::rebuild_content(&root, &selection, catalog).unwrap();
        // A startup fallback describes the root actually loaded, not the
        // rejected candidate. Its retained catalogue still permits retrying
        // the same enabled ID once its content is usable.
        let fallback = mods::content_for_loaded_root(&root, next.clone()).unwrap();
        assert_eq!(fallback.root, root);
        assert!(fallback.selection.entries.is_empty());
        assert!(!fallback.selection.enabled.contains("ruby"));
        client.mods = mods::ModsState::with_content(Some(fallback));
        client.settings.mods.enabled = selection.enabled.clone();
        client.switch_mods();
        assert!(
            client.pack_reload.is_some(),
            "fallback must not permanently disable later live switches"
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !client.pack_reload.as_mut().unwrap().shutdown() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        client.pack_reload = None;
        client.mods = mods::ModsState::with_content(Some(initial.clone()));
        client.settings.mods.enabled.clear();
        let (session, tasks) = Integrated::controlled_test_session();
        client.integrated = Some(session);
        client.player = Some(
            horizon_client::motion::PlayerMotion::new(
                &client.motion,
                horizon_core::geometry::Vec3::ZERO,
            )
            .unwrap(),
        );
        client.title = None;
        client.open_chat("keep", false);
        client.begin_content_reload(ReloadKind::Selection, stage(&client, next.clone()));
        assert!(client.content_reload_busy());
        client.ime_commit("ignored");
        assert_eq!(client.ui.chat_screen.as_ref().unwrap().input.value, "keep");
        let entry = PendingWorldEntry::Server {
            address: "localhost:25565".into(),
            name: "deferred".into(),
            realm: false,
            lan: false,
            cookies: None,
        };
        assert!(client.defer_world_entry(entry).unwrap());
        client.resume_world_entry().unwrap();
        assert!(client.pending_entry.is_some());
        client.poll_content_reload();
        assert_eq!(client.options.resources, root);
        assert!(tasks.run_next(&mut server));
        client.poll_content_reload();
        assert_eq!(client.options.resources, next.root);
        assert_eq!(
            client.mods.content.as_ref().unwrap().root,
            root,
            "GPU apply is not the server acknowledgment"
        );
        assert!(client.content_reload_busy());
        assert!(tasks.run_next(&mut server));
        client.poll_content_reload();
        assert!(!client.content_reload_busy());
        assert_eq!(client.mods.content.as_ref().unwrap().root, next.root);
        assert_eq!(server.options.resources, next.root);
        assert!(client.pending_entry.take().is_some());
        client.finish_reload_input_disarm(&TestLoop);
        client.title = None;
        client.open_chat("keep", false);
        client.ime_commit(" accepted");
        assert_eq!(
            client.ui.chat_screen.as_ref().unwrap().input.value,
            "keep accepted"
        );

        client.settings.resource_packs = vec!["vanilla".into()];
        // A real server generation change between preparation and commit
        // rejects before mutation; the client restores its actual old resources.
        client.begin_content_reload(ReloadKind::Selection, stage(&client, initial.clone()));
        assert!(tasks.run_next(&mut server));
        client.poll_content_reload();
        server.damage_tag_generation = server.damage_tag_generation.wrapping_add(1);
        assert!(tasks.run_next(&mut server));
        client.poll_content_reload();
        assert!(!client.content_reload_busy());
        assert!(client.error.is_none());
        assert_eq!(client.options.resources, next.root);
        assert_eq!(client.mods.content.as_ref().unwrap().root, next.root);
        assert_eq!(server.options.resources, next.root);
        assert_eq!(client.settings.resource_packs, ["vanilla"]);
        wait_pack(&mut client);
        assert_eq!(client.options.resources, next.root);

        // The latest choice supersedes a fully prepared result before GPU
        // publication; no commit enters the original server's task queue.
        client.begin_content_reload(ReloadKind::Selection, stage(&client, initial.clone()));
        assert!(tasks.run_next(&mut server));
        client.pending_pack_reload = true;
        client.poll_content_reload();
        assert!(!tasks.run_next(&mut server));
        assert_eq!(client.options.resources, next.root);
        client.pending_pack_reload = false;

        // A finished acknowledgment belongs to its original server only.
        client.begin_content_reload(ReloadKind::Selection, stage(&client, initial.clone()));
        assert!(tasks.run_next(&mut server));
        client.poll_content_reload();
        assert!(tasks.run_next(&mut server));
        let (replacement, replacement_tasks) = Integrated::controlled_test_session();
        client.integrated = Some(replacement);
        client.poll_content_reload();
        assert_eq!(client.options.resources, next.root);
        assert_eq!(client.mods.content.as_ref().unwrap().root, next.root);
        assert!(!replacement_tasks.run_next(&mut server));
        wait_pack(&mut client);
        // Reset the controlled server's committed generation for cancellation.
        let prepared = server.prepare_content_reload(next.clone()).unwrap();
        server.commit_content_reload(prepared).unwrap();

        // Dropping a queued transaction cancels its commit callback. Engine
        // shutdown may discard client resources only after the server drains.
        client.begin_content_reload(ReloadKind::Selection, stage(&client, initial.clone()));
        assert!(replacement_tasks.run_next(&mut server));
        client.poll_content_reload();
        client.mods.transaction = None;
        assert!(replacement_tasks.run_next(&mut server));
        assert_eq!(server.options.resources, next.root);
        // Shutdown discards that unpublished client half. Restore it here only
        // to exercise the separate authority-transition path on the same Game.
        client
            .apply_staged_pack(stage(&client, next.clone()))
            .unwrap();
        client.integrated = None;
        let (pipe, peer) = horizon_protocol::connection::Pipe::pair();
        client.connect_local(pipe, "AuthorityTransition").unwrap();
        client.begin_content_reload(ReloadKind::Server, stage(&client, initial.clone()));
        assert!(
            client.pack_reload.is_some(),
            "a valid Server request must restage, not fail because of stale local mod intent"
        );
        assert_eq!(client.settings.resource_packs, ["vanilla"]);
        assert_eq!(client.mods.content.as_ref().unwrap().root, next.root);
        wait_pack(&mut client);
        assert_eq!(client.settings.resource_packs, ["vanilla"]);
        assert_eq!(client.options.resources, next.root);
        client.leave_server(None).unwrap();
        peer.shutdown();
    }
}
