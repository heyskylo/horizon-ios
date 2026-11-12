use super::rows::{NetPlayer, NetTotal, Row};
use super::{Ctx, QueryError, Reply, num, opt_num};
use crate::wire::Unit;

const KIB: f64 = 1024.0;

/// What one player moved in the selected window.
pub(crate) struct PlayerRate {
    pub index: u32,
    pub name: String,
    pub brand: String,
    pub view_distance: u32,
    pub joined_t: f64,
    pub left_t: Option<f64>,
    pub bytes_out: u64,
    pub bytes_in: u64,
    pub packets_out: u64,
    pub packets_in: u64,
    /// Seconds the player was on inside the window.
    pub seconds: f64,
    pub peak_out_bytes_per_s: f64,
}

impl PlayerRate {
    pub fn out_kib_s(&self) -> f64 {
        self.bytes_out as f64 / KIB / self.seconds.max(1e-9)
    }

    pub fn in_kib_s(&self) -> f64 {
        self.bytes_in as f64 / KIB / self.seconds.max(1e-9)
    }
}

/// Per player, the whole blocks that overlap `from..to`, and the seconds the player was on inside them.
pub(crate) fn rates(
    net: &horizon_perf_format::v1::NetTraffic,
    from: f64,
    to: f64,
) -> Vec<PlayerRate> {
    let block_s = f64::from(net.block_s).max(1e-9);
    net.players
        .iter()
        .map(|p| {
            let on = (p.joined_t.max(from), p.left_t.unwrap_or(to).min(to));
            let (mut out, mut inn, mut pout, mut pin) = (0u64, 0u64, 0u64, 0u64);
            let mut peak = 0.0f64;
            let blocks = p.bytes_out.len().max(p.bytes_in.len());
            for b in 0..blocks {
                let (start, end) = (b as f64 * block_s, (b + 1) as f64 * block_s);
                if end <= from || start >= to {
                    continue;
                }
                let get = |v: &[u64]| v.get(b).copied().unwrap_or(0);
                out += get(&p.bytes_out);
                inn += get(&p.bytes_in);
                pout += u64::from(p.packets_out.get(b).copied().unwrap_or(0));
                pin += u64::from(p.packets_in.get(b).copied().unwrap_or(0));
                peak = peak.max(get(&p.bytes_out) as f64 / block_s);
            }
            PlayerRate {
                index: p.index,
                name: p.name.clone(),
                brand: p.brand.clone(),
                view_distance: p.view_distance,
                joined_t: p.joined_t,
                left_t: p.left_t,
                bytes_out: out,
                bytes_in: inn,
                packets_out: pout,
                packets_in: pin,
                seconds: (on.1 - on.0).max(0.0),
                peak_out_bytes_per_s: peak,
            }
        })
        .collect()
}

/// Bytes and packets per player (by join-order id, with name and client brand) and all players together.
pub(super) fn net(ctx: &Ctx) -> Result<Reply, QueryError> {
    let side = ctx.side(0)?;
    let Some(net) = ctx.views[0].capture.net.iter().find(|n| n.side() == side) else {
        return Err(QueryError::new(
            "this capture has no net section for the selected side",
        ));
    };
    let window = ctx.window(0, side, 0.0)?;
    let (from, to) = (window.from.max(0.0), window.to.min(ctx.views[0].end(side)));
    let mut players = rates(net, from, to);
    let span = (to - from).max(1e-9);
    let (out, inn): (u64, u64) = players
        .iter()
        .fold((0, 0), |a, p| (a.0 + p.bytes_out, a.1 + p.bytes_in));
    let all = NetTotal {
        id: "n:all".to_string(),
        players: players.len(),
        out_bytes: out,
        in_bytes: inn,
        out_kib_s: num(out as f64 / KIB / span),
        in_kib_s: num(inn as f64 / KIB / span),
    };
    players.sort_by(|a, b| {
        b.out_kib_s()
            .total_cmp(&a.out_kib_s())
            .then(a.index.cmp(&b.index))
    });
    let mut rows = vec![Row::NetTotal(all)];
    rows.extend(
        players
            .iter()
            .take(ctx.q.filters.limit)
            .map(|p| Row::NetPlayer(player_row(p))),
    );
    let mut reply = Reply::new(Unit::KibPerS, rows);
    reply.window = Some(crate::view::Window { from, to });
    Ok(reply)
}

pub(crate) fn player_id(index: u32) -> String {
    crate::ids::Id::new(crate::ids::Kind::Player, format!("p{index}")).to_string()
}

fn player_row(p: &PlayerRate) -> NetPlayer {
    NetPlayer {
        id: player_id(p.index),
        index: p.index,
        name: p.name.clone(),
        brand: p.brand.clone(),
        view_distance: p.view_distance,
        joined_t: num(p.joined_t),
        left_t: opt_num(p.left_t),
        out_bytes: p.bytes_out,
        in_bytes: p.bytes_in,
        packets_out: p.packets_out,
        packets_in: p.packets_in,
        out_kib_s: num(p.out_kib_s()),
        in_kib_s: num(p.in_kib_s()),
        peak_out_kib_s: num(p.peak_out_bytes_per_s / KIB),
    }
}
