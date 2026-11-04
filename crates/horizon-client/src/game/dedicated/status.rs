//! What `horizon-server` asks the dedicated server on its control endpoint (`--control-socket`, docs/server.md
//! section 2): `server-status` answers with the tick rate, the tick times `/tick query` reports, the players
//! (including their positions) and the loaded chunks; `console <line>` runs a console command and answers with the lines it logged;
//! `server-events <since>` answers the typed events after that number (events.rs); `management <connection>
//! <json>` edits the lists as vanilla's management protocol does (management.rs).
use super::{ServerState, capture_log};
use crate::game::Game;
use serde_json::{Value, json};
use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

/// The window the tick rate is measured over.
const TPS_WINDOW: Duration = Duration::from_secs(5);

impl ServerState {
    /// One pass of the server loop ended.
    pub(super) fn ticked(&mut self, at: Instant) {
        self.ticks.push_back(at);
        while self
            .ticks
            .front()
            .is_some_and(|t| at.duration_since(*t) > TPS_WINDOW)
        {
            self.ticks.pop_front();
        }
    }
    /// Ticks per second over the last few seconds.
    fn tps(&self) -> f64 {
        match (self.ticks.front(), self.ticks.back()) {
            (Some(first), Some(last)) if self.ticks.len() > 1 => {
                (self.ticks.len() - 1) as f64 / last.duration_since(*first).as_secs_f64().max(1e-9)
            }
            _ => 0.0,
        }
    }
}

impl Game<'_> {
    /// Answers the requests waiting on the control endpoint.
    pub(super) fn poll_server_control(&mut self, state: &mut ServerState) {
        while let Some(request) = self.control.as_ref().and_then(|c| c.try_recv()) {
            let reply = match request.line.trim().split_once(' ') {
                _ if request.line.trim() == "server-status" => self.server_status(state),
                Some(("server-events", since)) => {
                    super::events::since(since.trim().parse().unwrap_or(0))
                }
                Some(("management", rest)) => self.management(state, rest),
                Some(("console", line)) => {
                    // Command feedback is logged as system chat; the reply carries the text alone.
                    let output: Vec<String> = capture_log(|| self.console_command(state, line))
                        .into_iter()
                        .map(|l| {
                            l.strip_prefix("System chat: ")
                                .map_or(l.clone(), str::to_owned)
                        })
                        .collect();
                    json!({ "output": output })
                }
                // An RCON command: run as the console, with the `Rcon` source, whose feedback the
                // operators are told of when `broadcast-rcon-to-ops` is on.
                Some(("rcon", line)) => {
                    let output: Vec<String> = capture_log(|| self.rcon_line(state, line))
                        .into_iter()
                        .map(|l| {
                            l.strip_prefix("System chat: ")
                                .map_or(l.clone(), str::to_owned)
                        })
                        .collect();
                    json!({ "output": output })
                }
                _ => json!({ "error": format!("unknown request {:?}", request.line) }),
            };
            let _ = request.reply.send(reply.to_string());
        }
    }

    /// Each level's loaded chunks and entities (players and block entities apart), the level in place
    /// first: what the Prometheus text reports by dimension.
    fn status_levels(&self) -> Value {
        let mut levels = vec![json!({
            "dimension": self.dim.id(),
            "chunks": self.world.chunks.len(),
            "entities": self.entity_count(),
        })];
        for dim in self.levels.parked_dims() {
            if let (Some(world), Some(entities)) = (
                self.levels.parked_world(dim),
                self.levels.parked_entity_count(dim),
            ) {
                levels.push(json!({
                    "dimension": dim.id(),
                    "chunks": world.chunks.len(),
                    "entities": entities,
                }));
            }
        }
        Value::Array(levels)
    }

    /// The entities of the level in place: mobs, items, orbs, projectiles, vehicles, falling blocks,
    /// hangings, armor stands, cushions, tridents, rockets and primed TNT.
    fn entity_count(&self) -> usize {
        self.mobs.iter().filter(|m| !m.removed).count()
            + self.dropped.len()
            + self.orbs.len()
            + self.creatures.projectiles.len()
            + self.carts.len()
            + self.boats.len()
            + self.falling.len()
            + self.paintings.len()
            + self.item_frames.len()
            + self.armor_stands.len()
            + self.cushions.len()
            + self.tridents.len()
            + self.rockets.len()
            + self.tnts.len()
    }

    fn server_status(&self, state: &ServerState) -> Value {
        let millis = |nanos: u64| nanos as f64 / 1e6;
        let samples = self.clock.sorted_samples();
        // The percentiles `/tick query` prints (TickCommand.percentileToString).
        let at = |q: f64| {
            let i = ((samples.len() as f64 * q) as usize).min(samples.len().saturating_sub(1));
            samples.get(i).copied().map_or(0.0, millis)
        };
        let (players, max_players) = self.lan.as_ref().map_or((Vec::new(), 0), |h| {
            let players = h
                .hub
                .guests
                .iter()
                .map(|g| {
                    player_status(
                        &g.name,
                        g.uuid,
                        &g.address.ip().to_string(),
                        g.latency.load(Ordering::Relaxed),
                        g.position,
                    )
                })
                .collect();
            (players, h.hub.lan.shared.max_players())
        });
        json!({
            "tps": state.tps(),
            "target_tps": self.clock.rate(),
            "mspt": {
                "average": millis(self.clock.average_nanos()),
                "p50": at(0.5),
                "p95": at(0.95),
                "p99": at(0.99),
            },
            "players": players,
            "max_players": max_players,
            "chunks": self.world.chunks.len(),
            "levels": self.status_levels(),
            "paused": state.paused,
        })
    }
}

fn player_status(name: &str, uuid: u128, address: &str, ping_ms: i32, position: [f64; 3]) -> Value {
    json!({
        "name": name,
        "uuid": horizon_protocol::crypto::uuid_string(uuid),
        "address": address,
        "ping_ms": ping_ms,
        "position": position,
    })
}

#[cfg(test)]
mod tests {
    use super::player_status;
    use serde_json::json;

    #[test]
    fn player_status_includes_coordinates() {
        assert_eq!(
            player_status("Alex", 1, "203.0.113.7", 42, [1.5, 64.0, -2.25]),
            json!({
                "name": "Alex",
                "uuid": "00000000-0000-0000-0000-000000000001",
                "address": "203.0.113.7",
                "ping_ms": 42,
                "position": [1.5, 64.0, -2.25],
            })
        );
    }
}
