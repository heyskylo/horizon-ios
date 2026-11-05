//! A proxy's requests on the control endpoint, for a player it moves between two servers:
//! `player-expect <uuid> [seconds]` (the target is told who is coming) and
//! `player-export <uuid> [freeze]` (the source answers with the player's record). They run
//! between two ticks, on the server thread, so the player is never caught half-way through a
//! tick. The rules are `lan/arrivals.rs`'s. All of them are refused while `server.toml`'s
//! `[transfer]` is off.
use crate::game::{Dim, Game, lan::LanHost};
use horizon_client::lan::arrivals;
use horizon_protocol::packets::play::{self, Clientbound as C};
use serde_json::{Value, json};
use std::time::Instant;

impl Game<'_> {
    /// A proxy's `player-*` request; the reply is one JSON line, `{"ok": true, ...}` or
    /// `{"error": "..."}`.
    pub(super) fn player_request(&mut self, verb: &str, args: &str) -> Value {
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
            "player-export" => self.player_export(args),
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

    fn export_from(&mut self, host: &mut LanHost, uuid: u128, freeze: bool) -> Value {
        let i = match arrivals::check_exportable(&host.hub, uuid) {
            Ok(i) => i,
            Err(why) => return json!({ "error": why }),
        };
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
