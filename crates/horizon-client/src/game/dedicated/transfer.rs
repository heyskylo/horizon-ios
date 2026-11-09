//! A proxy's requests on the control endpoint, for a player it moves between two servers:
//! `player-expect <uuid> [seconds]` (the target is told who is coming; its login is held),
//! `player-drop <uuid>` (the target forgets it, and lets a held player go),
//! `player-export <uuid> [freeze]` (the source answers with the player's record) and
//! `player-release <uuid> <hand-over> moved [announce] | abort` (the source lets the player go),
//! `player-import <uuid> <json>` and `player-confirm <uuid> <hand-over>` (the target takes the
//! record and lets the held player in), and `transfer-status` (the counts a proxy reads before
//! it sends players here). They run
//! between two ticks, on the server thread, so the player is never caught half-way through a
//! tick. The rules are `lan/arrivals.rs`'s. All of them are refused while `server.toml`'s
//! `[transfer]` is off, and those that bring a player in once the server has begun to stop.
use crate::game::{Dim, Game, lan::LanHost};
use horizon_client::lan::arrivals;
use horizon_protocol::packets::play::{self, Clientbound as C};
use serde_json::{Value, json};
use std::time::Instant;

impl Game<'_> {
    /// A proxy's `player-*` request; the reply is one JSON line, `{"ok": true, ...}` or
    /// `{"error": "..."}`. Each request writes one line to the log with what became of it
    /// (`arrivals::request_log`): a hand-over that failed leaves a trace.
    pub(super) fn player_request(&mut self, verb: &str, args: &str) -> Value {
        // The name is read before the request runs: a release or a drop takes the player away.
        let name = args
            .split_whitespace()
            .next()
            .and_then(arrivals::parse_uuid)
            .and_then(|uuid| {
                self.lan
                    .as_ref()?
                    .hub
                    .guests
                    .iter()
                    .find(|g| g.uuid == uuid)
                    .map(|g| g.name.clone())
            });
        let reply = self.run_player_request(verb, args);
        let (level, line) = arrivals::request_log(verb, name.as_deref(), args, &reply);
        super::log(level, &line);
        reply
    }

    /// Once a stop has begun (`/stop` from the console, a player or the control endpoint, or a stop
    /// signal), the transfers know it before the next request is answered: what would bring a player
    /// in is refused from then on, and the ways out stay open (`arrivals::Transfers::stopping`).
    pub(super) fn note_stopping(&mut self, stop: bool) {
        let stopping = stop || horizon_threads::stop_requests() > 0;
        if let Some(transfers) = self.lan.as_mut().and_then(|h| h.hub.transfers.as_mut()) {
            transfers.stopping |= stopping;
        }
    }

    /// Whether the tick loop ends now. A stop (`/stop`, a stop signal) ends it at once, unless
    /// `[transfer]` is on and players are here: the stop then waits for a proxy to move them out
    /// (`arrivals::StopWait`), every login refused, the server ticking on. A second `stop` ends the
    /// wait; a second signal ends the process (`horizon_threads::catch_stop_signals`).
    pub(super) fn stop_now(&mut self, state: &super::ServerState) -> bool {
        let asked = std::mem::take(&mut state.lists.lock().unwrap().stop);
        let stop = asked || horizon_threads::stop_requests() > 0;
        let Some(host) = self.lan.as_mut() else {
            return stop;
        };
        let waiting = host
            .hub
            .transfers
            .as_ref()
            .is_some_and(|t| t.stop_wait.is_some());
        if !stop && !waiting {
            return false;
        }
        let (placed, frozen) = arrivals::placed_and_frozen(&host.hub);
        let Some(transfers) = host.hub.transfers.as_mut() else {
            return stop;
        };
        let now = Instant::now();
        // A freeze lasts ticks: how long one takes now, for `check_freeze`.
        let period = arrivals::tick_period(self.clock.tick_duration(), state.tps());
        match &mut transfers.stop_wait {
            None => {
                transfers.stopping = true;
                let wait = arrivals::StopWait::start(&transfers.settings, now, period);
                let Some(wait) = wait.filter(|_| placed > 0) else {
                    return true;
                };
                transfers.stop_wait = Some(wait);
                host.hub.lan.shared.refuse_logins();
                super::log(
                    "INFO",
                    &arrivals::stop_wait_line(transfers.settings.stop_wait),
                );
                false
            }
            Some(_) if asked => {
                super::log("INFO", "A second stop: stopping now");
                true
            }
            Some(wait) => {
                wait.period = period;
                let over = wait.over(now, placed, frozen);
                if over && frozen > 0 {
                    super::log(
                        "WARN",
                        &format!(
                            "Stopping with {frozen} hand-over(s) still frozen: their players are saved here"
                        ),
                    );
                }
                over
            }
        }
    }

    /// `transfer-status`: the counts a proxy reads before it sends players here
    /// (`arrivals::status_request`). A proxy asks it every few seconds, so it writes nothing to
    /// the log.
    pub(super) fn transfer_status(&self) -> Value {
        match self.lan.as_ref() {
            Some(host) => arrivals::status_request(&host.hub, Instant::now()),
            None => json!({ "error": "the server is not open to players" }),
        }
    }

    fn run_player_request(&mut self, verb: &str, args: &str) -> Value {
        let Some(host) = self.lan.as_ref() else {
            return json!({ "error": "the server is not open to players" });
        };
        if host.hub.transfers.is_none() {
            return json!({ "error": arrivals::OFF });
        }
        match verb {
            "player-expect" => match self.lan.as_mut() {
                Some(host) => arrivals::expect_request(&mut host.hub, args, Instant::now()),
                None => json!({ "error": "the server is not open to players" }),
            },
            "player-drop" => match self.lan.as_mut() {
                Some(host) => arrivals::drop_request(&mut host.hub, args),
                None => json!({ "error": "the server is not open to players" }),
            },
            "player-export" => self.player_export(args),
            "player-import" => self.player_import(args),
            "player-confirm" => self.player_confirm(args),
            "player-release" => match self.lan.as_mut() {
                Some(host) => arrivals::release_request(&mut host.hub, args),
                None => json!({ "error": "the server is not open to players" }),
            },
            other => json!({ "error": format!("unknown request {other:?}") }),
        }
    }

    /// `player-export <uuid> [freeze]`: closes the player's menus as vanilla does when a menu closes
    /// (the cursor stack and the crafting grids go back into the inventory), then answers with the
    /// player's record (`arrivals::export_record`). With `freeze`, a hand-over starts: the player's
    /// own inputs stop being acted on until it ends, and a later export answers with the same record.
    fn player_export(&mut self, args: &str) -> Value {
        let mut parts = args.split_whitespace();
        let Some(uuid) = parts.next().and_then(arrivals::parse_uuid) else {
            return json!({ "error": "player-export needs the player's unique id" });
        };
        let freeze = match parts.next() {
            None => false,
            Some("freeze") => true,
            Some(_) => return json!({ "error": "the only option of player-export is freeze" }),
        };
        if parts.next().is_some() {
            return json!({ "error": "player-export takes a unique id and at most freeze" });
        }
        let Some(mut host) = self.lan.take() else {
            return json!({ "error": "the server is not open to players" });
        };
        let reply = self.export_from(&mut host, uuid, freeze);
        self.lan = Some(host);
        reply
    }

    /// `player-import <uuid> <json>`: applies a record to the held player (`lan/import.rs`), with
    /// its advancements and statistics, which need the game's tree and counters, and the mods'
    /// attachments. Unless the request says `"place": false`, it then places the player as
    /// `player-confirm` does, its file written first.
    fn player_import(&mut self, args: &str) -> Value {
        let Some((id, request)) = args.trim().split_once(char::is_whitespace) else {
            return json!({ "error": "player-import needs a unique id and a request" });
        };
        let Some(uuid) = arrivals::parse_uuid(id) else {
            return json!({ "error": "player-import needs the player's unique id" });
        };
        let request: Value = match serde_json::from_str(request.trim()) {
            Ok(v) => v,
            Err(e) => return json!({ "error": format!("the request is not JSON: {e}") }),
        };
        let Some(mut host) = self.lan.take() else {
            return json!({ "error": "the server is not open to players" });
        };
        // A retry after the commit, whose reply was lost: the player is placed from this
        // hand-over already, and the answer is the same.
        let retried = request["record"]["handover"]
            .as_str()
            .and_then(arrivals::parse_handover)
            .and_then(|id| host.hub.placed_arrival(uuid, id).map(|g| (id, g)));
        if let Some((id, i)) = retried {
            let reply = json!({
                "ok": true,
                "handover": arrivals::handover_text(id),
                "placed": true,
                "announced": host.hub.guests[i].arrival_announce,
                "recentered": host.hub.guests[i].arrival_recentered,
                "retried": true,
            });
            self.lan = Some(host);
            return reply;
        }
        let result = horizon_client::lan::import::import_player(&mut host.hub, uuid, &request);
        let reply = match result {
            Ok(done) => {
                self.take_arrival_records(&mut host, uuid, &done);
                let handover = arrivals::handover_text(done.handover);
                let placed = if done.place {
                    self.place_arrival(&mut host, uuid, done.handover)
                } else {
                    Ok(false)
                };
                match placed {
                    Ok(placed) => json!({
                        "ok": true,
                        "handover": handover,
                        "parts": done.parts,
                        "placed": placed,
                        "announced": done.announce,
                        "recentered": done.recentered,
                    }),
                    Err(e) => json!({ "error": e }),
                }
            }
            Err(e) => json!({ "error": e }),
        };
        self.lan = Some(host);
        reply
    }

    /// What an import brought that the game keeps itself: the advancements (`PlayerAdvancements
    /// .applyFrom` drops those this server lacks), the statistics, and the attachments of a whole
    /// import, which saves copy from the game's own map (`guest_attachments_into`).
    fn take_arrival_records(
        &mut self,
        host: &mut LanHost,
        uuid: u128,
        done: &horizon_client::lan::import::Imported,
    ) {
        let tree = self.advancement_tree();
        let Some(g) = host.hub.guests.iter_mut().find(|g| g.uuid == uuid) else {
            return;
        };
        if let Some(saved) = &done.advancements {
            let mut progress = horizon_client::advancements::Progress::from_json(saved);
            progress
                .criteria
                .retain(|id, _| tree.by_id.contains_key(id));
            g.advancements = progress;
            g.advancements_loaded = true;
        }
        if done.attachments {
            self.guest_attachments.insert(uuid, g.attachments.clone());
        }
        if let Some(stats) = &done.stats {
            let play = host.play.entry(g.conn).or_default();
            play.stats = crate::game::stats::Stats::from_json(stats);
            play.stats.mark_all_dirty();
        }
    }

    /// `player-confirm <uuid> <hand-over>`: places a held player whose record of that hand-over was
    /// imported without placing it.
    fn player_confirm(&mut self, args: &str) -> Value {
        let Some(mut host) = self.lan.take() else {
            return json!({ "error": "the server is not open to players" });
        };
        let reply = match arrivals::confirm_args(&host.hub, args) {
            // A retry after the commit, whose reply was lost: the same answer.
            Ok((uuid, id)) if host.hub.placed_arrival(uuid, id).is_some() => {
                let i = host.hub.placed_arrival(uuid, id).unwrap_or_default();
                json!({
                    "ok": true,
                    "placed": true,
                    "announced": host.hub.guests[i].arrival_announce,
                    "retried": true,
                })
            }
            Ok((uuid, id)) => match self.place_arrival(&mut host, uuid, id) {
                Ok(_) => {
                    let announced = host
                        .hub
                        .guests
                        .iter()
                        .find(|g| g.uuid == uuid)
                        .is_some_and(|g| g.arrival_announce);
                    json!({ "ok": true, "placed": true, "announced": announced })
                }
                Err(e) => json!({ "error": e }),
            },
            Err(e) => json!({ "error": e }),
        };
        self.lan = Some(host);
        reply
    }

    /// The hand-over's commit point on the target: the arrival's player file, advancements and
    /// statistics are written, synchronously, then it is placed. Only then does the reply say
    /// `ok`, so a proxy that sends `player-release moved` to the source after it knows a copy of
    /// the player is on disk here. When the file cannot be written the player stays held and
    /// imported: the proxy can confirm again or drop it.
    fn place_arrival(&mut self, host: &mut LanHost, uuid: u128, id: u128) -> Result<bool, String> {
        let i = host.hub.imported_arrival(uuid, id)?;
        let mut g = host.hub.guests.remove(i);
        self.guest_attachments_into(&mut g);
        // An autosave of this player still in flight must not land after this write.
        let _ = self.finish_player_autosaves(true);
        let written = horizon_client::lan::playerdata::save_now(&g, &self.world_root)
            .and_then(|()| self.write_arrival_records(host, &g));
        let name = g.name.clone();
        host.hub.guests.insert(i, g);
        written?;
        let announced = host.hub.confirm_arrival(uuid, id)?;
        self.arrival_placed(uuid, &name, announced);
        Ok(true)
    }

    /// What the game does once a held player is placed: the log's lines, and the mods' "logged
    /// in" (`native_player_logged_in`), which a held login did not send, so a placed arrival
    /// is heard of as any login and its leaving later is not the mods' first word of it.
    fn arrival_placed(&mut self, uuid: u128, name: &str, announced: bool) {
        super::log("INFO", &arrivals::placed_line(name));
        if announced {
            super::log("INFO", &format!("System chat: {name} joined the game"));
        }
        self.native_player_logged_in(uuid, name);
    }

    fn export_from(&mut self, host: &mut LanHost, uuid: u128, freeze: bool) -> Value {
        let i = match arrivals::check_exportable(&host.hub, uuid) {
            Ok(i) => i,
            Err(why) => return json!({ "error": why }),
        };
        if freeze && let Err(why) = arrivals::check_freeze(&host.hub, i, Instant::now()) {
            return json!({ "error": why });
        }
        // A repeat while the hand-over lasts: the same record, and nothing touched.
        if let Some(h) = &host.hub.guests[i].handover {
            return arrivals::export_reply(&host.hub, i, h.record.clone());
        }
        let conn = host.hub.guests[i].conn;
        let window = host.hub.guests[i].menu.as_ref().map(|m| m.window);
        // The menu closes in the player's own level, as when it leaves (`game/lan.rs`'s close of
        // every guest): a chest's lid, a villager's trade and a dropped cursor stack belong there.
        let back = self.dim;
        let level = Dim::from_id(&host.hub.guests[i].dimension);
        if let Err(e) = self.enter_level(Some(host), level) {
            return json!({ "error": format!("the player's level could not be opened: {e}") });
        }
        // Nothing is sent to the client by this: it is for a player who is leaving.
        self.guest_close_all(host, conn);
        // The inventory's own 2x2 grid too (`InventoryMenu.removed`): `guest_close_all` empties it
        // only with a real `ContainerClose(0)`, and a frozen client's is dropped. Its items are
        // not in the player compound, so they would be lost with `player-release moved`.
        self.lan_crafting_closed(host, conn, Some(0));
        if let Err(e) = self.enter_level(Some(host), back) {
            log::error!("could not reopen {}: {e}", back.id());
        }
        let Some(i) = host.hub.guests.iter().position(|g| g.conn == conn) else {
            return json!({ "error": "that player is not connected to this server" });
        };
        // The player may stay (a hand-over that does not finish): its client closes the menu too
        // and shows the inventory as it now is.
        if let Some(container) = window {
            host.hub
                .send_to(conn, &C::ContainerClose(play::ContainerClose { container }));
        }
        host.hub.send_inventory(i);
        // What a mod attached to the player since it joined, as a save writes it.
        self.guest_attachments_into(&mut host.hub.guests[i]);
        let advancements = if self.adv.tree.list.is_empty() {
            Value::Null
        } else {
            host.hub.guests[i]
                .advancements
                .to_json(&self.adv.tree, horizon_world::DATA_VERSION)
        };
        let stats = host
            .play
            .get(&conn)
            .map_or(Value::Null, |p| p.stats.to_json());
        let records = arrivals::Records {
            advancements,
            stats,
        };
        match arrivals::export_record(&mut host.hub, i, freeze, records) {
            Ok(record) => arrivals::export_reply(&host.hub, i, record),
            Err(e) => json!({ "error": e }),
        }
    }
}
