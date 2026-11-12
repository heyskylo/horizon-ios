//! Each player's wire bytes per block of the capture, as `NetTraffic`, with the player's name, client brand and view distance.
use crate::server_perf::spans::BLOCK_S;
use horizon_perf_format::v1::{NetTraffic, PlayerTraffic, Side};

/// One connection as read now: its wire bytes (received, sent) since it opened, and who plays on it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Conn {
    pub id: u32,
    pub bytes: (u64, u64),
    pub name: String,
    /// The client's `minecraft:brand`, empty until it sends one.
    pub brand: String,
    /// The chunk radius this client is sent.
    pub view_distance: u32,
}

/// The open connections, read together.
pub type Wire = Vec<Conn>;

struct Player {
    conn: u32,
    name: String,
    brand: String,
    view_distance: u32,
    joined_t: f64,
    left_t: Option<f64>,
    last: (u64, u64),
    bytes_in: Vec<u64>,
    bytes_out: Vec<u64>,
}

/// Turns the running byte counts of the connections, read now and then, into bytes per block; a leaver's bytes since the last read are lost.
#[derive(Default)]
pub struct TrafficRecorder {
    players: Vec<Player>,
}

fn block_of(t: f64) -> usize {
    (t.max(0.0) / f64::from(BLOCK_S)) as usize
}

impl TrafficRecorder {
    /// `wire` is read as the capture starts: those players were on already, and only the bytes after it count.
    pub fn new(mut wire: Wire) -> Self {
        wire.sort_by_key(|c| c.id);
        Self {
            players: wire
                .into_iter()
                .map(|c| Player {
                    conn: c.id,
                    name: c.name,
                    brand: c.brand,
                    view_distance: c.view_distance,
                    joined_t: 0.0,
                    left_t: None,
                    last: c.bytes,
                    bytes_in: Vec::new(),
                    bytes_out: Vec::new(),
                })
                .collect(),
        }
    }

    /// The connections' counts `t` seconds into the capture; a new connection counts from zero, an unlisted one has left.
    pub fn sample(&mut self, t: f64, wire: &[Conn]) {
        let block = block_of(t);
        for c in wire {
            let (now_in, now_out) = c.bytes;
            let at = match self.players.iter().position(|p| p.conn == c.id) {
                Some(at) => at,
                None => {
                    self.players.push(Player {
                        conn: c.id,
                        name: String::new(),
                        brand: String::new(),
                        view_distance: 0,
                        joined_t: t,
                        left_t: None,
                        last: (0, 0),
                        bytes_in: Vec::new(),
                        bytes_out: Vec::new(),
                    });
                    self.players.len() - 1
                }
            };
            let p = &mut self.players[at];
            if !c.name.is_empty() {
                p.name.clone_from(&c.name);
            }
            if !c.brand.is_empty() {
                p.brand.clone_from(&c.brand);
            }
            p.view_distance = c.view_distance;
            p.bytes_in.resize(block + 1, 0);
            p.bytes_out.resize(block + 1, 0);
            p.bytes_in[block] += now_in.saturating_sub(p.last.0);
            p.bytes_out[block] += now_out.saturating_sub(p.last.1);
            p.last = (now_in, now_out);
        }
        for p in &mut self.players {
            if p.left_t.is_none() && !wire.iter().any(|c| c.id == p.conn) {
                p.left_t = Some(t);
            }
        }
    }

    /// The capture's traffic once it ended `t` seconds in; every player's arrays are as long as the capture.
    pub fn finish(self, t: f64) -> NetTraffic {
        let blocks = block_of(t) + 1;
        NetTraffic {
            side: Side::Server as i32,
            block_s: BLOCK_S,
            players: self
                .players
                .into_iter()
                .enumerate()
                .map(|(i, mut p)| {
                    p.bytes_in.resize(blocks, 0);
                    p.bytes_out.resize(blocks, 0);
                    PlayerTraffic {
                        index: i as u32 + 1,
                        joined_t: p.joined_t,
                        left_t: p.left_t,
                        bytes_in: p.bytes_in,
                        bytes_out: p.bytes_out,
                        packets_in: Vec::new(),
                        packets_out: Vec::new(),
                        name: p.name,
                        brand: p.brand,
                        view_distance: p.view_distance,
                    }
                })
                .collect(),
        }
    }
}
