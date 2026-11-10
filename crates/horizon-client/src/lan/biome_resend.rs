//! The `ChunksBiomes` bodies `resend_biomes` sends: the columns a guest has been
//! sent, encoded once for every set of columns the guests share.

use crate::fast_hash::FxHashSet;
use horizon_protocol::packets::play::{self, Clientbound as C};

/// The body each receiver is sent: its columns of `data` that are in `sent`, in
/// `data`'s order; `None` when it has none of them. Receivers with the same
/// columns get the same bytes, encoded once.
pub(super) fn bodies<'a>(
    sent: impl Iterator<Item = &'a FxHashSet<(i32, i32)>>,
    data: &[((i32, i32), play::ChunkBiomeData)],
) -> Vec<Option<Vec<u8>>> {
    let mut encoded: Vec<(Vec<usize>, Vec<u8>)> = Vec::new();
    sent.map(|sent| {
        let picks: Vec<usize> = data
            .iter()
            .enumerate()
            .filter(|(_, (key, _))| sent.contains(key))
            .map(|(i, _)| i)
            .collect();
        if picks.is_empty() {
            return None;
        }
        let at = match encoded.iter().position(|(p, _)| *p == picks) {
            Some(at) => at,
            None => {
                let chunks: Vec<_> = picks.iter().map(|&i| data[i].1.clone()).collect();
                encoded.push((picks, C::ChunksBiomes(play::ChunksBiomes { chunks }).encode()));
                encoded.len() - 1
            }
        };
        Some(encoded[at].1.clone())
    })
    .collect()
}
