//! The server connection's received events, once a frame (`poll_remote`): chunk packets
//! within their per-frame budget, then every other packet in order (`handle_packet`).
use super::*;

impl Game<'_> {
    /// A headless command oracle has a real connection but no movement tick.
    /// Acknowledge loaded terrain without predicting or sending movement.
    pub(in crate::game) fn poll_stationary_client(&mut self) -> Result<()> {
        self.poll_remote()?;
        let loaded = self.player.as_ref().is_some_and(|p| {
            self.streaming.is_lit(
                (p.position.x.floor() as i32) >> 4,
                (p.position.z.floor() as i32) >> 4,
            )
        });
        if loaded && self.remote.as_ref().is_some_and(|r| !r.loaded_sent) {
            self.send(S::PlayerLoaded(Empty));
            let r = self.remote.as_mut().unwrap();
            r.loaded_sent = true;
            r.level_load.finish();
        }
        Ok(())
    }

    /// Process what the network thread received (once per frame, like the
    /// client's packet processor), in order. Chunk packets get
    /// `CHUNK_PACKETS_PER_FRAME`: once a frame has spent it, the events from
    /// the next chunk packet on wait for the next frame, unless they have
    /// already waited `BACKLOG_LIMIT`. A chunk packet still decoding holds
    /// back what follows it.
    pub(in crate::game) fn poll_remote(&mut self) -> Result<()> {
        let polled = self.poll_remote_events();
        // The block events and block changes of a piston clock start and end
        // thousands of moves at once: the collision view and the renderer
        // take them all in one sync, before the tick or frame reads them.
        if std::mem::take(&mut self.moving_pistons_stale) {
            self.sync_moving_pistons();
        }
        polled
    }
    fn poll_remote_events(&mut self) -> Result<()> {
        let Some(r) = self.remote.as_mut() else {
            return Ok(());
        };
        let mut events = std::mem::take(&mut r.backlog);
        // Chunk packets start decoding now; the frame that reaches one in
        // order inserts its column.
        for event in r.session.poll() {
            events.push_back(match event {
                Event::Packet(p) if matches!(*p, C::LevelChunkWithLight(_)) => {
                    match r.decoder.submit(r.chunk_format.clone(), p) {
                        None => Received::Chunk(r.chunk_format.clone()),
                        Some(p) => Received::Event(Event::Packet(p)),
                    }
                }
                event => Received::Event(event),
            });
        }
        let overdue = r
            .backlog_since
            .is_some_and(|since| since.elapsed() >= BACKLOG_LIMIT);
        let mut chunks = Duration::ZERO;
        // A run of ForgetLevelChunk (thousands when the server's view moves far: a
        // teleport at 32 chunks) is forgotten at once, before whatever follows it.
        let mut forgotten = horizon_client::fast_hash::FxHashSet::default();
        while let Some(item) = events.pop_front() {
            if let Received::Event(Event::Packet(packet)) = &item
                && let C::ForgetLevelChunk(p) = &**packet
            {
                self.native_client_packet(packet);
                forgotten.insert((p.pos[0], p.pos[1]));
                continue;
            }
            horizon_client::spans::time("pr_forget", || self.forget_remote_columns(&mut forgotten));
            let chunk = matches!(item, Received::Chunk(_));
            if chunk && !overdue && chunks >= CHUNK_PACKETS_PER_FRAME {
                events.push_front(item);
                break;
            }
            let started = chunk.then(Instant::now);
            let event = match item {
                Received::Event(event) => event,
                Received::Chunk(format) => {
                    // A column still decoding waits for the next frame, with
                    // what came after it, overdue or not.
                    let Some(next) = self.remote.as_ref().map(|r| r.decoder.next()) else {
                        break;
                    };
                    let (packet, column) = match next {
                        Next::Ready(decoded) => decoded,
                        Next::Decoding => {
                            events.push_front(Received::Chunk(format));
                            break;
                        }
                        Next::Lost => {
                            log::warn!("Horizon: the chunk decoder stopped; a column is missing");
                            continue;
                        }
                    };
                    self.native_client_packet(&packet);
                    if let C::LevelChunkWithLight(p) = *packet {
                        horizon_client::spans::time("pr_chunk", || {
                            self.remote_chunk(&p, Some((&format, column)))
                        });
                    }
                    chunks += started.map_or(Duration::ZERO, |t| t.elapsed());
                    continue;
                }
            };
            match event {
                Event::Status(s) => {
                    if let Some(r) = self.remote.as_mut() {
                        r.status = s;
                    }
                }
                Event::Joined {
                    registries,
                    name,
                    uuid,
                } => {
                    self.set_registries(registries);
                    self.extras_joined();
                    // handleLogin: the level loading screen replaces any dialog.
                    self.close_dialogs();
                    if let Some(r) = self.remote.as_mut() {
                        r.joined = true;
                        // handleLogin: startWaitingForNewLevel with Reason.OTHER.
                        r.profile = uuid;
                        r.profile_name = name.clone();
                        // LevelLoadingScreen's line: no player name.
                        r.status = horizon_client::lang::get_or(
                            "multiplayer.downloadingTerrain",
                            "Loading terrain...",
                        );
                    }
                }
                Event::Features(features) => {
                    if let Some(remote) = &mut self.remote {
                        remote.features = features;
                    }
                }
                Event::Brand(brand) => {
                    if let Some(remote) = &mut self.remote {
                        remote.brand = brand;
                    }
                }
                Event::QuietCommands => {
                    if let Some(remote) = &mut self.remote {
                        remote.quiet_commands = true;
                    }
                }
                Event::ModNet => {
                    if let Some(remote) = &mut self.remote {
                        remote.mod_net = true;
                    }
                }
                Event::FarTerrain(offer) => {
                    if let Some(remote) = &mut self.remote {
                        remote.far_terrain = Some(offer);
                    }
                }
                Event::Reconfiguring => {
                    // handleConfigurationStart: the held-back chat shows, then
                    // clearClientLevel opens ServerReconfigScreen over no level.
                    self.flush_chat_queue();
                    self.send_chat_acknowledgement();
                    self.close_dialogs();
                    // The server configuring us now (a proxy's next one)
                    // offers its own far terrain, or none, and announces its
                    // own features again (a Horizon server's quiet commands).
                    if let Some(remote) = &mut self.remote {
                        remote.far_terrain = None;
                        remote.quiet_commands = false;
                        remote.mod_net = false;
                    }
                    self.clear_lod();
                    if let Some(remote) = &self.remote {
                        remote.session.reset_far_terrain();
                    }
                    self.player = None;
                    self.screen = Screen::Playing;
                    self.capture(false);
                    // clearClientLevel drops the client level now, so its
                    // unload comes before the configuration screen, not
                    // after the next login's load.
                    if let Some(old) = self.remote.as_mut().and_then(|r| r.heard_level.take()) {
                        self.native_client_level_unload(&old);
                    }
                    let ticks = self.ticks;
                    if let Some(r) = self.remote.as_mut() {
                        r.reconfiguring = Some(ticks);
                        r.chat_tree_received = false;
                    }
                }
                Event::Reconfigured(registries) => {
                    // handleConfigurationStart → clearClientLevel, and the next
                    // ClientPacketListener starts with no players or scoreboard,
                    // no waypoints (its own ClientWaypointManager: a proxy's next
                    // server never untracks the last one's), and its new
                    // ClientSuggestionProvider with no custom chat completions.
                    // clearClientLevel clears the narrator.
                    self.clear_narrator();
                    self.hud_disconnected();
                    self.score.board = Default::default();
                    self.clear_waypoints();
                    if let Some(r) = self.remote.as_mut() {
                        r.attachments.clear();
                        r.players.clear();
                        r.tab_players.clear();
                        r.listed.clear();
                        r.signatures = Default::default();
                        r.custom_completions.clear();
                    }
                    self.ui.registries.custom_completions.clear();
                    self.set_registries(registries);
                }
                // handleTransfer: "Cannot transfer to server from
                // singleplayer" (no ServerData), so the world just closes.
                Event::Transfer { host, port } => {
                    if let Some(r) = self.remote.as_mut()
                        && r.session.address != "local"
                    {
                        r.transfer = Some(format!("{host}:{port}"));
                    }
                }
                Event::PackPush(p) => self.handle_pack_push(p),
                Event::PackPop(id) => self.handle_pack_pop(id),
                Event::ServerLinks(links) => {
                    if let Some(r) = self.remote.as_mut() {
                        r.server_links = super::super::server_links::trusted(links);
                    }
                }
                Event::ShowDialog(dialog) => self.show_dialog(&dialog),
                Event::ClearDialog => self.clear_dialog(),
                Event::CodeOfConduct(text) => self.handle_code_of_conduct(text),
                Event::DecodeError(e) => {
                    if let Some(r) = self.remote.as_mut() {
                        r.decode_errors += 1;
                        if r.decode_errors <= 20 {
                            log::warn!("Horizon: skipped undecodable packet {e}");
                        }
                    }
                }
                Event::Disconnected {
                    reason,
                    key,
                    from_server,
                } => {
                    log::info!("Horizon: disconnected: {reason}");
                    // ClientboundTransferPacket left a target: connect there
                    // instead of showing the disconnect, the stored cookies
                    // carried over (ClientCommonPacketListenerImpl.
                    // handleTransfer → ConnectScreen.startConnecting).
                    let transfer = self.remote.as_mut().and_then(|r| {
                        r.transfer.take().map(|address| {
                            (address, r.session.name.clone(), r.session.cookies.clone())
                        })
                    });
                    if let Some((address, name, cookies)) = transfer {
                        self.leave_server(None)?;
                        self.connect_server_with_cookies(&address, &name, Some(cookies))?;
                        return Ok(());
                    }
                    let retry = self.reconnect_after(&reason, key.as_deref(), from_server);
                    self.leave_server(Some(reason))?;
                    self.offer_reconnect(retry);
                    return Ok(());
                }
                Event::Packet(packet) => self.handle_packet(*packet)?,
            }
            chunks += started.map_or(Duration::ZERO, |t| t.elapsed());
            if self.remote.is_none() {
                break;
            }
        }
        if self.remote.is_some() {
            horizon_client::spans::time("pr_forget", || self.forget_remote_columns(&mut forgotten));
        }
        if let Some(r) = self.remote.as_mut() {
            r.backlog_since = if events.is_empty() {
                None
            } else {
                Some(r.backlog_since.unwrap_or_else(Instant::now))
            };
            r.backlog = events;
        }
        self.send_suggestion_request();
        Ok(())
    }
}
